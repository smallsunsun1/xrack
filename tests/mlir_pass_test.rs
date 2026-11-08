use xrack_compiler::{
    codegen,
    mlir::{self, Module, OperationRef},
    parse,
};

fn raw(source: &str, filename: &str) -> Module {
    let module = mlir::lower_module(&parse(source).unwrap(), filename).unwrap();
    module.verify().unwrap();
    module
}

fn member<'m>(parent: OperationRef<'m>, kind: &str, name: &str) -> OperationRef<'m> {
    parent
        .children()
        .find(|op| op.name() == kind && op.attribute("name") == Some(name))
        .unwrap()
}

fn definition<'m>(op: OperationRef<'m>, index: usize) -> OperationRef<'m> {
    op.operand(index)
        .and_then(|value| value.defining_operation())
        .unwrap()
}

#[test]
fn optimization_preserves_stack_reads_on_each_side_of_navigation() {
    let optimized = raw(
        r#"
        component App inherits Window {
            out property <int> before: 0;
            out property <int> after: 0;
            nav := StackView {
                home := StackPage {}
                detail := StackPage {}
            }
            go := Button {
                clicked() => {
                    root.before = nav.depth;
                    nav.push("detail");
                    root.after = nav.depth;
                }
            }
        }
        "#,
        "stack-reads.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let button = window
        .children()
        .find(|op| op.attribute("id") == Some("go"))
        .unwrap();
    let handler = member(button, "xr.handler", "clicked");
    let before = member(handler, "xr.assign", "before");
    let after = member(handler, "xr.assign", "after");
    assert_eq!(definition(before, 0).name(), "xr.stack_read");
    assert_eq!(definition(after, 0).name(), "xr.stack_read");
    assert_ne!(before.operand(0), after.operand(0));
    let sequence: Vec<_> = handler
        .children()
        .map(|op| op.name())
        .filter(|name| matches!(*name, "xr.stack_read" | "xr.assign" | "xr.navigate"))
        .collect();
    assert_eq!(
        sequence,
        [
            "xr.stack_read",
            "xr.assign",
            "xr.navigate",
            "xr.stack_read",
            "xr.assign"
        ]
    );
    assert!(codegen::generate(&optimized).is_ok());
}

#[test]
fn folding_and_cse_change_the_native_values_used_by_rust_codegen() {
    let input = raw(
        r#"export component App inherits Window {
        out property <string> label: 1 + 2 * 3 == 7 && !false ? "selected" : "discarded";
        out property <int> first: 7;
        out property <int> second: 7;
        out property <int> signed-remainder: -7 % 3;
    }"#,
        "passes.xr",
    );
    let component = input.operations().next().unwrap();
    assert_eq!(
        definition(member(component, "xr.property", "label"), 0).name(),
        "xr.select"
    );
    assert_ne!(
        member(component, "xr.property", "first").operand(0),
        member(component, "xr.property", "second").operand(0)
    );
    assert!(
        codegen::generate(&input)
            .unwrap()
            .to_string()
            .contains("discarded")
    );

    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let label = definition(member(component, "xr.property", "label"), 0);
    assert_eq!(label.name(), "xr.literal");
    assert_eq!(label.attribute("value"), Some("\"selected\""));
    let remainder = definition(member(component, "xr.property", "signed-remainder"), 0);
    assert_eq!(remainder.name(), "xr.literal");
    assert_eq!(remainder.attribute("value"), Some("-1"));
    let first = member(component, "xr.property", "first")
        .operand(0)
        .unwrap();
    let second = member(component, "xr.property", "second")
        .operand(0)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.type_name(), "!xr.int");
    assert_eq!(
        component
            .children()
            .filter(|op| op.name() == "xr.literal" && op.attribute("value") == Some("7"))
            .count(),
        1
    );
    assert!(
        !codegen::generate(&optimized)
            .unwrap()
            .to_string()
            .contains("discarded")
    );
}

#[test]
fn optimization_preserves_dynamic_conditions_and_trapping_operations() {
    let input = raw(
        r#"export component App inherits Window {
        out property <int> count: 0;
        callback condition() -> bool;
        condition => { root.count += 1; true }
        callback choose();
        choose() => { root.count = root.condition() ? 7 : 7; }
        out property <int> overflow: 9223372036854775807 + 1;
        out property <int> div-zero: 12 / 0;
    }"#,
        "effects.xr",
    );
    // CSE has made both select arms identical before the second pipeline run.
    let optimized = input.optimize().unwrap().optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let choose = member(component, "xr.callback", "choose");
    let select = choose
        .children()
        .find(|op| op.name() == "xr.select")
        .unwrap();
    assert_eq!(select.operand(1), select.operand(2));
    assert_eq!(definition(select, 0).name(), "xr.call");
    for (property, operator) in [("overflow", "+"), ("div-zero", "/")] {
        let initializer = definition(member(component, "xr.property", property), 0);
        assert_eq!(initializer.name(), "xr.binary");
        assert_eq!(initializer.attribute("value"), Some(operator));
    }
}

#[test]
fn operation_views_keep_block_order_and_distinguish_reads_across_writes() {
    let module = raw(
        r#"export component App inherits Window {
        in-out property <int> count: 1;
        callback step();
        step() => { root.count += 1; root.count += 1; }
    }"#,
        "ordered.xr",
    )
    .optimize()
    .unwrap();
    let component = module.operations().next().unwrap();
    let callback = member(component, "xr.callback", "step");
    assert_eq!(callback.regions().count(), 1);
    let region = callback.regions().next().unwrap();
    assert_eq!(region.blocks().count(), 1);
    let mut events = Vec::new();
    for op in region.blocks().next().unwrap().operations() {
        match op.name() {
            "xr.read" => events.push((op.name(), op.result(0).unwrap())),
            "xr.assign" => {
                let sum = definition(op, 0);
                events.push((op.name(), sum.operand(0).unwrap()));
            }
            _ => {}
        }
    }
    assert_eq!(
        events.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["xr.read", "xr.assign", "xr.read", "xr.assign"]
    );
    assert_eq!(events[0].1, events[1].1);
    assert_eq!(events[2].1, events[3].1);
    assert_ne!(events[0].1, events[2].1);
    assert_eq!(callback.span().line, 3);
    assert_eq!(
        callback
            .children()
            .find(|op| op.name() == "xr.assign")
            .unwrap()
            .span()
            .line,
        4
    );
}
