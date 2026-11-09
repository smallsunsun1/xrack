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

fn visual_slots(module: &Module) -> usize {
    let rust = codegen::generate(module).unwrap().to_string();
    rust.split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0].starts_with("__visual_") && pair[1] == ":")
        .count()
}

#[test]
fn equivalent_visual_bindings_share_one_cache_across_sibling_elements() {
    let input = raw(
        r#"export component Demo inherits Window {
            in-out property <int> count: 0;
            first := Text { text: "Count " + root.count; }
            second := Text { text: "Count " + root.count; }
        }"#,
        "shared-visuals.xr",
    );
    assert_eq!(visual_slots(&input), 2);
    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let binding = |id| {
        let element = window
            .children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap();
        member(element, "xr.bind", "text")
    };
    let first = binding("first");
    let second = binding("second");
    assert_ne!(first.operand(0), second.operand(0));
    let group = first.attribute("xr.visual_cache_group");
    assert!(group.is_some());
    assert_eq!(group, second.attribute("xr.visual_cache_group"));
    assert_eq!(visual_slots(&optimized), 1);

    // Textual IR can carry arbitrary annotations. Only a cache plan generated
    // by the optimization pipeline may change codegen behavior.
    let reparsed = Module::parse(&optimized.to_mlir()).unwrap();
    assert_eq!(visual_slots(&reparsed), 2);
    assert_eq!(visual_slots(&reparsed.optimize().unwrap()), 1);
}

#[test]
fn visual_cache_groups_are_separate_for_each_repeat_and_exclude_calls() {
    let module = raw(
        r#"export component Demo inherits Window {
            in-out property <int> count: 0;
            pure callback format-count(int) -> string;
            a := Text { text: "Count " + root.format-count(root.count); }
            b := Text { text: "Count " + root.format-count(root.count); }
            for item in ["A", "B"] : VerticalLayout {
                Text { text: "Row " + item; }
                Text { text: "Row " + item; }
                Text { text: "Detail " + item; }
            }
            for item in ["X", "Y"] : VerticalLayout {
                Text { text: "Row " + item; }
                Text { text: "Row " + item; }
            }
        }"#,
        "scoped-visuals.xr",
    )
    .optimize()
    .unwrap();
    let component = module.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let elements: Vec<_> = window
        .children()
        .filter(|op| op.name() == "xr.element")
        .collect();
    for call in &elements[..2] {
        assert_eq!(
            member(*call, "xr.bind", "text").attribute("xr.visual_cache_group"),
            None
        );
    }
    let groups: Vec<_> = elements[2..]
        .iter()
        .map(|repeat| {
            let rows: Vec<_> = repeat
                .children()
                .filter(|op| op.name() == "xr.element")
                .collect();
            let group = |element| {
                member(element, "xr.bind", "text")
                    .attr("xr.visual_cache_group")
                    .unwrap()
            };
            assert_eq!(group(rows[0]), group(rows[1]));
            if let Some(different) = rows.get(2) {
                assert_eq!(
                    member(*different, "xr.bind", "text").attribute("xr.visual_cache_group"),
                    None
                );
            }
            group(rows[0])
        })
        .collect();
    assert_ne!(groups[0], groups[1]);
    assert_eq!(visual_slots(&module), 5);
}

#[test]
fn row_invariant_bindings_share_a_component_cache_across_repeats() {
    let input = raw(
        r#"export component Demo inherits Window {
            in-out property <string> caption: "shared";
            for item in ["A", "B"] : VerticalLayout {
                Text { text: "Caption " + root.caption; }
                Text { text: "Row " + item; }
            }
            for item in ["X", "Y"] : VerticalLayout {
                Text { text: "Caption " + root.caption; }
                Text { text: "Row " + item; }
            }
        }"#,
        "repeat-invariants.xr",
    );
    assert_eq!(visual_slots(&input), 4);
    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let groups: Vec<_> = window
        .children()
        .filter(|op| op.name() == "xr.element")
        .map(|repeat| {
            let labels: Vec<_> = repeat
                .children()
                .filter(|op| op.name() == "xr.element")
                .collect();
            let invariant = member(labels[0], "xr.bind", "text");
            assert_eq!(
                member(labels[1], "xr.bind", "text").attribute("xr.repeat_invariant_group"),
                None
            );
            invariant.attr("xr.repeat_invariant_group").unwrap()
        })
        .collect();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0], groups[1]);
    assert_eq!(visual_slots(&optimized), 3);
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert_eq!(
        rust.matches("self . __state . __visuals . __visual_0 . get")
            .count(),
        2
    );
}

#[test]
fn scene_plans_mark_paint_static_subtrees_and_only_invariant_subexpressions() {
    let optimized = raw(
        r#"export component Demo inherits Window {
            in-out property <string> caption: "shared";
            in-out property <color> tint: #abc;
            in-out property <length> size: 40px;
            swatch := Rectangle { background: root.tint; width: root.size; }
            panel := VerticalLayout { Text { text: "static"; } }
            for item in ["A", "B"] : Text { text: ("Caption " + root.caption) + item; }
        }"#,
        "scene-plans.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let elements: Vec<_> = window
        .children()
        .filter(|op| op.name() == "xr.element")
        .collect();
    assert_eq!(
        member(elements[0], "xr.bind", "background").attribute("xr.update_effect"),
        Some("paint")
    );
    assert_eq!(
        member(elements[0], "xr.bind", "width").attribute("xr.update_effect"),
        None
    );
    assert_eq!(elements[0].attribute("xr.static_subtree"), None);
    assert_eq!(elements[1].attribute("xr.static_subtree"), Some("true"));
    let static_child = elements[1]
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    assert_eq!(static_child.attribute("xr.static_subtree"), None);
    let binding = member(elements[2], "xr.bind", "text");
    let expression = definition(binding, 0);
    assert_eq!(binding.attribute("xr.repeat_invariant_group"), None);
    assert_eq!(expression.attribute("xr.repeat_subexpression_group"), None);
    assert!(
        definition(expression, 0)
            .attribute("xr.repeat_subexpression_group")
            .is_some()
    );
    assert_eq!(
        definition(expression, 1).attribute("xr.repeat_subexpression_group"),
        None
    );
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert!(rust.contains("self . __state . __visuals . __visual_0 . get"));
    assert!(rust.contains("get_or_init"));
}

#[test]
fn precompiles_constant_paths_and_plans_only_fixed_grids() {
    let module = raw(
        r#"export component Demo inherits Window {
            in-out property <string> path-source: "M0 0 L10 10";
            in-out property <bool> show: true;
            fixed-path := Path { commands: "M0 0 " + "L10 10"; }
            live-path := Path { commands: root.path-source; }
            fixed-grid := GridLayout {
                Rectangle { rowspan: 2; }
                Row { Text { colspan: 2; text: "fixed"; } }
            }
            live-grid := GridLayout { Rectangle { visible: root.show; } }
        }"#,
        "fixed-geometry.xr",
    )
    .optimize()
    .unwrap();
    let root = module
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let element = |id| {
        root.children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap()
    };
    let path = |id| member(element(id), "xr.bind", "commands").attribute("xr.precompiled_path");
    assert_eq!(path("fixed-path"), Some("true"));
    assert_eq!(path("live-path"), None);
    assert_eq!(
        element("fixed-grid").attribute("xr.static_grid"),
        Some("true")
    );
    assert_eq!(element("live-grid").attribute("xr.static_grid"), None);
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
fn constant_conversions_and_string_expressions_reach_rust_as_literals() {
    let input = raw(
        r#"export component App inherits Window {
            out property <length> width-value: (20px + 4px) / 2;
            out property <float> ratio: 25%;
            out property <float> rounded: 9007199254740993;
            out property <float> negative-zero: -0.0;
            out property <float> infinity: 1.0 / 0.0;
            out property <bool> comparison: 2.5px < 3px;
            out property <bool> escaped-equality: "\u0041" == "A";
            out property <string> caption: "版" + 2 + "\n" + true;
        }"#,
        "scalar-folding.xr",
    );
    let rust_before = codegen::generate(&input).unwrap().to_string();
    assert!(rust_before.contains("format !"));

    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let initializer = |name| definition(member(component, "xr.property", name), 0);
    for (name, value) in [
        ("width-value", 12.0_f64),
        ("ratio", 0.25),
        ("rounded", 9_007_199_254_740_992.0),
        ("negative-zero", -0.0),
    ] {
        let op = initializer(name);
        assert_eq!(op.name(), "xr.literal", "{name}");
        let actual = op.attribute("value").unwrap().parse::<f64>().unwrap();
        assert_eq!(actual.to_bits(), value.to_bits(), "{name}");
    }
    assert_eq!(initializer("infinity").name(), "xr.binary");
    for name in ["comparison", "escaped-equality"] {
        assert_eq!(initializer(name).name(), "xr.literal");
        assert_eq!(initializer(name).attribute("value"), Some("true"));
    }
    assert_eq!(initializer("caption").name(), "xr.literal");
    assert_eq!(
        initializer("caption").attribute("value"),
        Some("\"版2\\ntrue\"")
    );
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert!(rust.contains("版2"));
    assert!(!rust.contains("format !"));
}

#[test]
fn identities_keep_live_reads_and_calls_and_prune_dead_operands() {
    let optimized = raw(
        r#"export component App inherits Window {
            in-out property <int> count: 2;
            in-out property <bool> enabled: true;
            in-out property <string> caption: "app";
            pure callback probe() -> bool;
            callback ping;
            callback run;
            run => { root.ping(); }
            out property <int> identity: 0 + (count - 0) * 1 / 1;
            out property <int> zero: count * 0;
            out property <bool> condition: !!(enabled && true) || false;
            out property <bool> must-call: root.probe() && false;
            out property <bool> never-call: false && root.probe();
            out property <string> same: "" + caption + "";
        }"#,
        "identities.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let initializer = |name| definition(member(component, "xr.property", name), 0);
    for (name, property) in [
        ("identity", "count"),
        ("condition", "enabled"),
        ("same", "caption"),
    ] {
        let op = initializer(name);
        assert_eq!(op.name(), "xr.read", "{name}");
        assert_eq!(op.attribute("value"), Some(property));
    }
    assert_eq!(initializer("zero").name(), "xr.binary");
    let call = initializer("must-call");
    assert_eq!(call.name(), "xr.binary");
    assert_eq!(definition(call, 0).name(), "xr.call");
    assert_eq!(initializer("never-call").name(), "xr.literal");
    assert_eq!(
        component
            .children()
            .filter(|op| op.name() == "xr.call")
            .count(),
        1
    );
    assert!(!component.children().any(|op| op.name() == "xr.unary"));
    let run = member(component, "xr.callback", "run");
    assert!(run.children().any(|op| op.name() == "xr.invoke"));
}

#[test]
fn only_unconditional_visual_guards_are_removed() {
    let input = raw(
        r#"export component App inherits Window {
            in property <bool> show: true;
            if 1 + 2 == 3 : always := Button { clicked() => {} }
            if false : hidden := Text { text: "not-rendered-marker"; }
            if root.show : dynamic := Text { text: "dynamic"; }
        }"#,
        "visual-guards.xr",
    );
    let rust_before = codegen::generate(&input).unwrap().to_string();
    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let element = |id| {
        window
            .children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap()
    };
    let has_guard = |id| element(id).children().any(|op| op.name() == "xr.show_if");
    assert!(!has_guard("always"));
    assert!(has_guard("hidden"));
    assert!(has_guard("dynamic"));
    assert_eq!(
        member(element("hidden"), "xr.show_if", "visible").attribute("xr.static_hidden"),
        Some("true")
    );
    assert_eq!(
        member(element("dynamic"), "xr.show_if", "visible").attribute("xr.static_hidden"),
        None
    );
    assert_eq!(
        member(element("always"), "xr.handler", "clicked").name(),
        "xr.handler"
    );
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert_eq!(rust_before.matches("Node :: hidden").count(), 3);
    assert_eq!(rust.matches("Node :: hidden").count(), 2);
    assert!(rust_before.contains("not-rendered-marker"));
    assert!(!rust.contains("not-rendered-marker"));
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
