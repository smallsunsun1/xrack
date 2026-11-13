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
fn static_integer_repeats_share_only_equal_passive_scenes() {
    let module = raw(
        r#"
        component Badge inherits Text { text: "Delegate"; }
        export component App inherits Window {
            in property <int> live: 3;
            for row in 4 : Text { text: "Fixed"; }
            for row in 4 : Text { text: "Fixed"; }
            for row in 5 : Text { text: "Fixed"; }
            for row in 4 : Text { text: "Row " + row; }
            for row in live : Text { text: "Dynamic count"; }
            ListView { for row in 4 : Badge {} }
            for row in 129 : Text { text: "Large"; }
            ListView { for row in 4 : Text { text: "Virtualized"; } }
        }
        "#,
        "static-integer-repeats.xr",
    )
    .optimize()
    .unwrap();
    let elements: Vec<_> = module
        .operations()
        .find(|op| op.attribute("sym_name") == Some("App"))
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap()
        .children()
        .filter(|op| matches!(op.name(), "xr.element" | "xr.instance"))
        .collect();
    let count = |op: OperationRef<'_>| op.integer_attribute("xr.static_repeat_count");
    assert_eq!(
        elements.iter().copied().map(count).collect::<Vec<_>>(),
        [Some(4), Some(4), Some(5), None, None, None, None, None]
    );
    let group = |op: OperationRef<'_>| op.integer_attribute("xr.static_subtree_group");
    assert!(group(elements[0]).is_some());
    assert_eq!(group(elements[0]), group(elements[1]));
    assert_ne!(group(elements[0]), group(elements[2]));
    for view in [elements[5], elements[7]] {
        assert!(view.children().all(|op| count(op).is_none()));
    }
    let rust = codegen::generate(&module).unwrap().to_string();
    assert_eq!(rust.matches("static_repeat_scene").count(), 3);
    assert_eq!(rust.matches("xrack_runtime :: Repeater <").count(), 4);
    assert_eq!(
        rust.matches("xrack_runtime :: ComponentRepeater <").count(),
        1
    );
}

#[test]
fn reactive_gradients_and_translations_can_use_shared_component_caches() {
    let module = raw(
        r#"
        translations "catalog.toml";
        export component App inherits Window {
            in property <color> accent: #48a;
            pure callback tint() -> color;
            Rectangle { background: @linear-gradient(90deg, root.accent, #fff); }
            Rectangle { background: @linear-gradient(90deg, root.accent, #fff); }
            for row in 5 : Rectangle {
                background: @linear-gradient(90deg, root.accent, #fff);
                Text { text: @tr("title"); }
            }
            for row in 5 : Rectangle {
                background: @linear-gradient(90deg, root.accent, #fff);
                Text { text: @tr("title"); }
            }
            for row in 5 : Rectangle { background: @linear-gradient(90deg, root.tint(), #fff); }
        }
        "#,
        "reactive-visuals.xr",
    )
    .optimize()
    .unwrap();
    let rectangles: Vec<_> = module
        .operations()
        .find(|op| op.name() == "xr.component")
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap()
        .children()
        .filter(|op| op.name() == "xr.element")
        .collect();
    let binding = |index: usize| member(rectangles[index], "xr.bind", "background");
    let pair = |first, second, attribute| {
        assert!(binding(first).attribute(attribute).is_some());
        assert_eq!(
            binding(first).attribute(attribute),
            binding(second).attribute(attribute)
        );
    };
    pair(0, 1, "xr.repeat_invariant_group");
    pair(0, 2, "xr.repeat_invariant_group");
    pair(2, 3, "xr.repeat_invariant_group");
    assert_eq!(binding(4).attribute("xr.repeat_invariant_group"), None);
    let translation = |index: usize| {
        let text = rectangles[index]
            .children()
            .find(|op| op.name() == "xr.element")
            .unwrap();
        member(text, "xr.bind", "text").attribute("xr.repeat_invariant_group")
    };
    assert!(translation(2).is_some());
    assert_eq!(translation(2), translation(3));
}

#[test]
fn unused_private_bindings_release_their_initializer_graph() {
    let unoptimized = raw(
        r#"
        export component App inherits Window {
            in property <int> seed: 10;
            property <int> dead-input: root.seed + 1;
            property <int> dead-middle: root.dead-input + root.dead-input;
            property <string> dead-tip: "unused marker " + root.dead-middle;
            property <int> dead-call: root.compute(root.dead-middle);
            property <[string]> dead-model: ["one", "two"];
            property <int> live: root.seed * 2;
            out property <int> result: root.live + 3;
            property <int> written;
            property <bool> linked;
            pure callback compute(int) -> int;
            callback change();
            change => { root.written = 2; }
            CheckBox { checked <=> root.linked; }
        }
        "#,
        "unused-private-bindings.xr",
    );
    assert_eq!(
        unoptimized
            .operations()
            .next()
            .unwrap()
            .children()
            .filter(|op| op.name() == "xr.property")
            .count(),
        10
    );
    let optimized = unoptimized.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let properties: Vec<_> = component
        .children()
        .filter(|op| op.name() == "xr.property")
        .map(|op| op.attribute("name").unwrap())
        .collect();
    assert_eq!(properties, ["seed", "live", "result", "written", "linked"]);
    assert!(!component.children().any(|op| op.name() == "xr.call"));
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert!(!rust.contains("dead_middle"));
    assert!(!rust.contains("unused marker"));
}

#[test]
fn state_merging_requires_adjacent_identical_overrides_and_no_transitions() {
    let optimized = raw(
        r#"
        component Badge inherits Text { in property <string> caption; text: root.caption; }
        export component App inherits Window {
            in property <int> mode: 0;
            panel := Rectangle { width: 20px; }
            badge := Badge { caption: "base"; }
            states [
                first when root.mode == 1: { panel.width: 40px; badge.caption: "same"; }
                second when root.mode == 2: { panel.width: 40px; badge.caption: "same";
                    in { animate panel.width { duration: 180ms; enabled: false; } } }
                different when root.mode == 3: { panel.width: 80px; badge.caption: "same"; }
                later when root.mode == 4: { panel.width: 40px; badge.caption: "same"; }
            ]
        }
        component Timed inherits Window {
            in property <int> mode: 0;
            panel := Rectangle { width: 20px; }
            states [
                first when root.mode == 1: { panel.width: 40px;
                    out { animate panel.width { duration: 160ms; enabled: true; } } }
                second when root.mode == 2: { panel.width: 40px; }
            ]
        }
        "#,
        "merged-state-cases.xr",
    )
    .optimize()
    .unwrap();
    let window = |name| {
        optimized
            .operations()
            .find(|op| op.attribute("sym_name") == Some(name))
            .unwrap()
            .children()
            .find(|op| op.name() == "xr.element")
            .unwrap()
    };
    fn selector(window: OperationRef<'_>) -> OperationRef<'_> {
        window
            .children()
            .find(|op| op.name() == "xr.state_select")
            .unwrap()
    }
    let app = window("App");
    let merged = selector(app);
    assert_eq!(
        merged.string_array_attribute("names").unwrap(),
        ["first", "different", "later"]
    );
    let condition = definition(merged, 0);
    assert_eq!(condition.attribute("value"), Some("||"));
    for (i, number) in ["1", "2"].into_iter().enumerate() {
        assert_eq!(
            definition(definition(condition, i), 1).attribute("value"),
            Some(number)
        );
    }
    for (target, operation) in [("panel", "xr.state_set"), ("badge", "xr.instance_state")] {
        let element = app
            .children()
            .find(|op| op.attribute("id") == Some(target))
            .unwrap();
        assert!(!element.children().any(|op| op.name() == "xr.state_timing"));
        let overrides: Vec<_> = element
            .children()
            .filter(|op| op.name() == operation)
            .map(|op| op.integer_attribute("case_index").unwrap())
            .collect();
        assert_eq!(overrides, [0, 1, 2]);
    }
    let timed = window("Timed");
    assert_eq!(
        selector(timed).string_array_attribute("names").unwrap(),
        ["first", "second"]
    );
    let panel = timed
        .children()
        .find(|op| op.attribute("id") == Some("panel"))
        .unwrap();
    let timing = member(panel, "xr.state_timing", "width");
    assert_eq!(timing.attribute("direction"), Some("out"));
    assert!(timing.operand(2).is_none());
}

#[test]
fn boolean_selects_lower_to_short_circuit_operators() {
    let optimized = raw(
        r#"
        export component App inherits Window {
            in property <bool> condition;
            in property <bool> value;
            out property <bool> left-true: root.condition ? true : root.value;
            out property <bool> left-false: root.condition ? false : root.value;
            out property <bool> right-true: root.condition ? root.value : true;
            out property <bool> right-false: root.condition ? root.value : false;
            out property <bool> inverse: root.condition ? false : true;
            out property <bool> comparison: true != root.condition;
            out property <bool> same: root.condition ? true : true;
        }
        "#,
        "boolean-selects.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let initial = |name| definition(member(component, "xr.property", name), 0);
    for (name, operator, negated) in [
        ("left-true", "||", false),
        ("left-false", "&&", true),
        ("right-true", "||", true),
        ("right-false", "&&", false),
    ] {
        let expression = initial(name);
        assert_eq!(expression.attribute("value"), Some(operator));
        let left = definition(expression, 0);
        let read = if negated {
            assert_eq!(left.attribute("value"), Some("!"));
            definition(left, 0)
        } else {
            left
        };
        assert_eq!(read.attribute("value"), Some("condition"));
        assert_eq!(definition(expression, 1).attribute("value"), Some("value"));
    }
    for name in ["inverse", "comparison"] {
        assert_eq!(initial(name).attribute("value"), Some("!"));
    }
    assert_eq!(initial("same").name(), "xr.select");
}

#[test]
fn static_resource_groups_are_shared_across_components() {
    let module = raw(
        r#"
        component First inherits Window {
            out property <length> measured: observed.layout-width;
            Label { text: "shared"; width: 80px; height: 24px; }
            Rectangle { background: @radial-gradient(circle, #111, #eee); }
            observed := Rectangle { width: 120px; height: 24px; }
        }
        component Second inherits Window {
            Label { text: "shared"; width: 80px; height: 24px; }
            Rectangle { background: @radial-gradient(circle, #111, #eee); }
            Label { text: "different"; width: 80px; height: 24px; }
        }
        "#,
        "shared-static-resources.xr",
    )
    .optimize()
    .unwrap();
    let visual_children = |name| {
        module
            .operations()
            .find(|op| op.attribute("sym_name") == Some(name))
            .unwrap()
            .children()
            .find(|op| op.name() == "xr.element")
            .unwrap()
            .children()
            .filter(|op| op.name() == "xr.element")
            .collect::<Vec<_>>()
    };
    let first = visual_children("First");
    let second = visual_children("Second");
    for index in [0, 1] {
        let group = first[index]
            .integer_attribute("xr.static_subtree_group")
            .unwrap();
        assert_eq!(
            second[index].integer_attribute("xr.static_subtree_group"),
            Some(group)
        );
    }
    assert_eq!(first[2].attribute("xr.static_subtree"), None);
    assert_eq!(second[2].integer_attribute("xr.static_subtree_group"), None);
    let gradient_group = |node| {
        definition(member(node, "xr.bind", "background"), 0)
            .integer_attribute("xr.static_gradient_group")
    };
    assert!(gradient_group(first[1]).is_some());
    assert_eq!(gradient_group(first[1]), gradient_group(second[1]));
    let rust = codegen::generate(&module).unwrap().to_string();
    assert_eq!(rust.matches("static __XR_GRADIENT_").count(), 1);
    assert_eq!(rust.matches("static __XR_STATIC_SCENE_").count(), 2);
}

#[test]
fn fixed_image_source_plan_requires_a_stable_box_and_a_plain_image() {
    let optimized = raw(
        r#"
        component App inherits Window {
            in property <image> picture;
            in property <length> live-width: 100px;
            fixed := Image { source: root.picture; width: 100px; height: 50px; }
            intrinsic := Image { source: root.picture; width: 100px; }
            changing := Image { source: root.picture; width: root.live-width; height: 50px; }
            animated := AnimatedImage { source: root.picture; width: 100px; height: 50px; }
        }
        "#,
        "fixed-image-plans.xr",
    )
    .optimize()
    .unwrap();
    let root = optimized
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    for (id, expected) in [
        ("fixed", Some("paint")),
        ("intrinsic", None),
        ("changing", None),
        ("animated", None),
    ] {
        let image = root
            .children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap();
        assert_eq!(
            member(image, "xr.bind", "source").attribute("xr.update_effect"),
            expected
        );
    }
}

#[test]
fn builtin_literal_fields_fold_and_leave_public_values_reactive() {
    let module = raw(
        r#"
        component App inherits Window {
            property <Time> fixed: Time { hour: 30, minute: -2, second: 99 };
            in-out property <Time> live: Time { hour: 30, minute: -2, second: 99 };
            property <Date> today: Date { year: 2026, month: 11, day: 12 };
            property <Font> face: Font { family: "sans-serif", size: 18px, weight: 500, italic: false };
            out property <int> hour: root.fixed.hour;
            out property <int> minute: root.fixed.minute;
            out property <int> second: root.fixed.second;
            out property <int> active-hour: root.live.hour;
            out property <int> day: root.today.day;
            out property <length> size: root.face.size;
        }
        "#,
        "builtin-projections.xr",
    )
    .optimize()
    .unwrap();
    let component = module.operations().next().unwrap();
    for (property, value) in [
        ("hour", "23"),
        ("minute", "0"),
        ("second", "59"),
        ("day", "12"),
        ("size", "18"),
    ] {
        let result = definition(member(component, "xr.property", property), 0);
        assert_eq!(result.name(), "xr.literal");
        assert_eq!(result.attribute("value"), Some(value));
    }
    for property in ["fixed", "today", "face"] {
        assert!(
            !component
                .children()
                .any(|op| op.attribute("name") == Some(property))
        );
    }
    let live = definition(member(component, "xr.property", "active-hour"), 0);
    assert_eq!(live.name(), "xr.time_field");
}

#[test]
fn empty_repeat_plan_keeps_virtual_templates_and_normalizes_known_paint_order() {
    let optimized = raw(
        r#"
        component App inherits Window {
            in-out property <int> count: 0;
            for row in 0 : Text { text: "" + row; }
            for item in [] : Label { text: item; }
            for row in root.count : Text { text: "" + row; }
            ListView { for row in 0 : Text {} }
            sorted := Rectangle {
                Rectangle { z: 0; }
                Rectangle { z: 1; }
            }
            unsorted := Rectangle {
                Rectangle { z: 2; }
                Rectangle { z: 1; }
            }
        }
        "#,
        "empty-repeat-plans.xr",
    )
    .optimize()
    .unwrap();
    let root = optimized
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let children: Vec<_> = root
        .children()
        .filter(|op| op.name() == "xr.element")
        .collect();
    fn mark<'m>(element: OperationRef<'m>) -> Option<&'m str> {
        element
            .children()
            .find(|op| op.name() == "xr.repeat")
            .and_then(|repeat| repeat.attribute("xr.empty_repeat"))
    }
    assert_eq!(mark(children[0]), Some("true"));
    assert_eq!(mark(children[1]), Some("true"));
    assert_eq!(mark(children[2]), None);
    let virtual_row = children[3]
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    assert_eq!(mark(virtual_row), None);
    assert_eq!(
        children[4].i64_array_attribute("xr.static_z_order"),
        Some(vec![])
    );
    assert_eq!(
        children[5].i64_array_attribute("xr.static_z_order"),
        Some(vec![1, 0])
    );
    let rust = codegen::generate(&optimized).unwrap().to_string();
    assert!(rust.contains("__xr_empty_repeat_"));
    assert!(rust.contains("set_natural_paint_order"));
    let unplanned = Module::parse(&optimized.to_mlir()).unwrap();
    let fallback = codegen::generate(&unplanned).unwrap().to_string();
    let repeater_fields = |source: &str| source.matches("xrack_runtime :: Repeater <").count();
    assert_eq!(repeater_fields(&fallback), repeater_fields(&rust) + 2);
}

#[test]
fn scalar_record_projections_share_slots_and_hoist_the_row_independent_prefix() {
    let optimized = raw(
        r#"
        struct Strings { prefix: string, suffix: string }
        component App inherits Window {
            in-out property <Strings> style: Strings { prefix: "x", suffix: "y" };
            in property <[string]> items: ["one", "two"];
            Text { text: root.style.prefix + root.style.suffix; }
            Text { text: root.style.prefix + root.style.suffix; }
            for item in root.items : Text { text: root.style.prefix + item; }
        }
        "#,
        "record-cache-plans.xr",
    )
    .optimize()
    .unwrap();
    let root = optimized
        .operations()
        .find(|op| op.name() == "xr.component")
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let texts: Vec<_> = root
        .children()
        .filter(|op| op.name() == "xr.element")
        .collect();
    let group = |text| member(text, "xr.bind", "text").attribute("xr.visual_cache_group");
    let shared = group(texts[0]).unwrap();
    assert_eq!(group(texts[1]), Some(shared));
    let prefix = definition(definition(member(texts[2], "xr.bind", "text"), 0), 0);
    assert_eq!(prefix.name(), "xr.get_field");
    assert!(prefix.attribute("xr.repeat_subexpression_group").is_some());
}

#[test]
fn private_alias_chains_forward_reads_writes_and_public_endpoints() {
    let optimized = raw(
        r#"
        component App inherits Window {
            property <int> storage: 7;
            property <int> middle <=> root.storage;
            property <int> tail <=> root.middle;
            in-out property <int> exposed <=> root.tail;
            out property <int> observed: root.tail;
            callback step();
            step => { root.tail += 2; }
            property <int> fixed: 11;
            property <int> private-alias <=> root.fixed;
            out property <int> constant: root.private-alias + 1;
            entry := TextInput { text: "value"; }
            property <string> control-alias <=> entry.text;
            out property <string> edited: root.control-alias;
        }
        "#,
        "private-aliases.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    for alias in ["middle", "tail", "private-alias"] {
        assert!(
            !component
                .children()
                .any(|op| { op.name() == "xr.property" && op.attribute("name") == Some(alias) })
        );
    }
    let exposed = member(component, "xr.property", "exposed");
    assert_eq!(exposed.attribute("alias"), Some("true"));
    assert_eq!(definition(exposed, 0).attribute("value"), Some("storage"));
    let observed = member(component, "xr.property", "observed");
    assert_eq!(definition(observed, 0).attribute("value"), Some("storage"));
    let step = member(component, "xr.callback", "step");
    assert!(
        step.children()
            .any(|op| { op.name() == "xr.assign" && op.attribute("name") == Some("storage") })
    );
    let constant = definition(member(component, "xr.property", "constant"), 0);
    assert_eq!(constant.name(), "xr.literal");
    assert_eq!(constant.attribute("value"), Some("12"));
    assert_eq!(
        definition(member(component, "xr.property", "control-alias"), 0).name(),
        "xr.control_read"
    );
}

#[test]
fn fixed_text_update_plan_requires_two_stable_absolute_dimensions() {
    let optimized = raw(
        r#"
        component App inherits Window {
            in property <string> message: "status";
            in property <length> height-input: 30px;
            in property <bool> compact: false;
            states [ compact when root.compact: { stateful.height: 18px; } ]

            fixed := Text { text: root.message; width: 120px; height: 24px; }
            grid := GridLayout {
                label := Label { text: root.message; width: 120px; height: 24px; }
            }
            intrinsic := Text { text: root.message; width: 120px; }
            relative := Label { text: root.message; width: 50%; height: 24px; }
            dynamic := Text { text: root.message; width: 120px; height: root.height-input; }
            stateful := Text { text: root.message; width: 120px; height: 24px; }
            animated := Text {
                text: root.message; width: 120px; height: 24px;
                animate width { duration: 120ms; }
            }
        }
        "#,
        "fixed-text-updates.xr",
    )
    .optimize()
    .unwrap();
    let root = optimized
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    fn element<'m>(parent: OperationRef<'m>, id: &str) -> OperationRef<'m> {
        parent
            .children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap()
    }
    let text_effect = |element| member(element, "xr.bind", "text").attribute("xr.update_effect");
    assert_eq!(text_effect(element(root, "fixed")), Some("paint"));
    assert_eq!(
        text_effect(element(element(root, "grid"), "label")),
        Some("paint")
    );
    for id in ["intrinsic", "relative", "dynamic", "stateful", "animated"] {
        assert_eq!(text_effect(element(root, id)), None, "{id}");
    }
}

#[test]
fn record_projection_folding_keeps_writable_storage_and_other_field_evaluation() {
    let optimized = raw(
        r#"
        struct Pair { count: int, label: string }
        component App inherits Window {
            property <Pair> immutable: Pair { count: 7, label: "preset" };
            property <Pair> writable: Pair { count: 8, label: "old" };
            in-out property <Pair> linked <=> root.writable;
            property <Pair> assigned: Pair { count: 6, label: "old" };
            callback change();
            change => { root.assigned = Pair { count: 10, label: "new" }; }
            property <Pair> dynamic: Pair { count: 9, label: root.label() };
            pure callback label() -> string;
            label => { "dynamic" }

            out property <int> fixed: root.immutable.count;
            out property <int> updated: root.writable.count;
            out property <int> assigned-count: root.assigned.count;
            out property <int> evaluated: root.dynamic.count;
        }
        "#,
        "record-projections.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized
        .operations()
        .find(|op| op.name() == "xr.component")
        .unwrap();
    let fixed = definition(member(component, "xr.property", "fixed"), 0);
    assert_eq!(fixed.name(), "xr.literal");
    assert_eq!(fixed.attribute("value"), Some("7"));
    assert!(
        !component
            .children()
            .any(|op| op.attribute("name") == Some("immutable"))
    );
    for (property, owner) in [
        ("updated", "writable"),
        ("assigned-count", "assigned"),
        ("evaluated", "dynamic"),
    ] {
        let field = definition(member(component, "xr.property", property), 0);
        assert_eq!(field.name(), "xr.get_field");
        let record = definition(field, 0);
        assert_eq!(record.name(), "xr.read");
        assert_eq!(record.attribute("value"), Some(owner));
    }
    let dynamic = definition(member(component, "xr.property", "dynamic"), 0);
    assert_eq!(definition(dynamic, 1).name(), "xr.call");
}

#[test]
fn optimized_scroll_targets_keep_viewport_identity_and_reject_foreign_ids() {
    let optimized = raw(
        r#"
        component App inherits Window {
            canvas := ScrollView { content-height: 400px; }
            panel := Rectangle {}
            ScrollBar { target: canvas; }
            ScrollIndicator { target: canvas; }
        }
        "#,
        "scroll-targets.xr",
    )
    .optimize()
    .unwrap();
    let text = optimized.to_mlir();
    let corrupted = text.replacen("target = \"canvas\"", "target = \"panel\"", 1);
    assert_ne!(text, corrupted);
    let result = Module::parse(&corrupted).and_then(|module| module.verify());
    assert!(result.unwrap_err().contains("requires a static viewport"));
    assert_eq!(text.matches("\"xr.scroll_target\"").count(), 2);
}

#[test]
fn table_sort_reads_remain_live_across_a_sort_command() {
    let optimized = raw(
        r#"
        component App inherits Window {
            in property <[string]> records;
            out property <int> before;
            out property <int> after;
            callback sort();
            sort => {
                root.before = table.current-sort-column;
                table.sort-by(0, false);
                root.after = table.current-sort-column;
            }
            table := TableView {
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { Text { text: row; } }
            }
        }
        "#,
        "table-sort-order.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let callback = member(component, "xr.callback", "sort");
    let operations: Vec<_> = callback
        .children()
        .filter(|op| matches!(op.name(), "xr.table_sort_read" | "xr.table_sort_command"))
        .map(|op| op.name())
        .collect();
    assert_eq!(
        operations,
        [
            "xr.table_sort_read",
            "xr.table_sort_command",
            "xr.table_sort_read"
        ]
    );
}

#[test]
fn split_state_reads_stay_on_both_sides_of_a_restore() {
    let optimized = raw(
        r#"
        component App inherits Window {
            out property <string> before;
            out property <string> after;
            in property <string> saved;
            callback restore();
            restore => {
                root.before = work.save-state();
                work.restore-state(root.saved);
                root.after = work.save-state();
            }
            work := SplitView { left := Rectangle {} right := Rectangle {} }
        }
        "#,
        "split-state-order.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let callback = member(component, "xr.callback", "restore");
    let operations: Vec<_> = callback
        .children()
        .filter(|op| matches!(op.name(), "xr.split_state_read" | "xr.split_restore"))
        .map(|op| op.name())
        .collect();
    assert_eq!(
        operations,
        [
            "xr.split_state_read",
            "xr.split_restore",
            "xr.split_state_read"
        ]
    );
}

#[test]
fn tree_queries_keep_their_order_across_an_expansion_command() {
    let optimized = raw(
        r#"
        struct Entry { id: int, parent: int }
        component App inherits Window {
            property <[Entry]> records;
            out property <int> before: -1;
            out property <int> after: -1;
            callback reveal();
            reveal => {
                root.before = directory.row-for-key(2);
                directory.expand-to(2);
                root.after = directory.row-for-key(2);
            }
            directory := TreeView {
                for row in root.records : TreeRow {
                    key: row.id; parent-key: row.parent; Text { text: "row"; }
                }
            }
        }
        "#,
        "tree-query-order.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized
        .operations()
        .find(|op| op.name() == "xr.component")
        .unwrap();
    let callback = member(component, "xr.callback", "reveal");
    let operations: Vec<_> = callback
        .children()
        .filter(|op| matches!(op.name(), "xr.tree_query" | "xr.tree_command"))
        .map(|op| op.name())
        .collect();
    assert_eq!(
        operations,
        ["xr.tree_query", "xr.tree_command", "xr.tree_query"]
    );
    assert!(codegen::generate(&optimized).is_ok());
}

#[test]
fn scroll_range_reads_remain_live_across_writes() {
    let optimized = raw(
        r#"component Demo inherits Window {
            in-out property <float> offset: 0;
            out property <float> before: 0;
            out property <float> after: 0;
            bar := ScrollBar { position <=> root.offset; size: 0.25; }
            callback jump();
            jump() => {
                root.before = bar.position;
                root.offset = 0.5;
                root.after = bar.position;
            }
        }"#,
        "scroll-range-effects.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let jump = member(component, "xr.callback", "jump");
    assert_eq!(
        jump.children()
            .filter(|op| op.name() == "xr.scroll_control_read")
            .count(),
        2
    );
}

#[test]
fn cse_preserves_scroll_command_order_and_live_viewport_reads() {
    let optimized = raw(
        r#"export component Demo inherits Window {
            out property <length> position: view.content-x + view.content-y;
            view := ScrollView {}
            jump := Button { clicked() => {
                view.scroll-to(8px, 12px);
                view.scroll-to(8px, 12px);
                view.scroll-to(16px, 20px);
            } }
        }"#,
        "scroll-effects.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let position = definition(member(component, "xr.property", "position"), 0);
    assert_eq!(
        definition(position, 0).attribute("value"),
        Some("content-x")
    );
    assert_eq!(
        definition(position, 1).attribute("value"),
        Some("content-y")
    );
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let button = window
        .children()
        .find(|op| op.attribute("id") == Some("jump"))
        .unwrap();
    let handler = member(button, "xr.handler", "clicked");
    let offsets: Vec<_> = handler
        .children()
        .filter(|op| op.name() == "xr.scroll_command")
        .map(|op| definition(op, 0).attribute("value").unwrap())
        .collect();
    assert_eq!(offsets, ["8", "8", "16"]);
}

#[test]
fn table_positioning_effects_keep_order_and_independent_axis_arguments() {
    let optimized = raw(
        r#"component Demo inherits Window {
        table := TableView {
            TableColumn { title: "Name"; }
            for row in 10000 : TableRow { Text { text: "" + row; } }
        }
        callback jump();
        jump() => {
            table.position-view-at-cell(9000, 0, end, center);
            table.position-view-at-cell(9000, 0, end, center);
            table.position-view-at-column(0, visible);
            table.position-view-at-row(0, beginning);
        }
    }"#,
        "table-position.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let jump = member(component, "xr.callback", "jump");
    let commands: Vec<_> = jump
        .children()
        .filter(|op| op.name() == "xr.scroll_command")
        .collect();
    assert_eq!(
        commands
            .iter()
            .map(|op| op.attribute("method").unwrap())
            .collect::<Vec<_>>(),
        [
            "position-view-at-cell",
            "position-view-at-cell",
            "position-view-at-column",
            "position-view-at-row"
        ]
    );
    assert_eq!(commands[0].operand_count(), 4);
    assert_eq!(definition(commands[0], 2).attribute("value"), Some("end"));
    assert_eq!(
        definition(commands[0], 3).attribute("value"),
        Some("center")
    );
}

#[test]
fn static_gradient_plans_exclude_live_colors() {
    let input = raw(
        r#"export component Demo inherits Window {
            in-out property <color> tint: #abc;
            Rectangle { background: @linear-gradient(90deg, #000, #fff); }
            Rectangle { background: @radial-gradient(circle, #000, #fff); }
            Rectangle { background: @radial-gradient(circle, #000, root.tint); }
            Rectangle { background: @linear-gradient(90deg, root.tint, #fff); }
        }"#,
        "gradient-plans.xr",
    );
    let optimized = input.optimize().unwrap();
    let window = optimized
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let markers: Vec<_> = window
        .children()
        .filter(|op| op.name() == "xr.element")
        .map(|element| {
            definition(member(element, "xr.bind", "background"), 0).attribute("xr.static_gradient")
        })
        .collect();
    assert_eq!(markers, [Some("true"), Some("true"), None, None]);
    let cache_count = |module| {
        codegen::generate(module)
            .unwrap()
            .to_string()
            .matches("OnceLock")
            .count()
    };
    assert_eq!(cache_count(&optimized), 4);
    assert_eq!(
        cache_count(&Module::parse(&optimized.to_mlir()).unwrap()),
        0
    );
}

#[test]
fn unconditional_literal_states_become_bindings_and_unlock_scene_plans() {
    let optimized = raw(
        r#"
        component Badge inherits Rectangle {}
        export component App inherits Window {
            in property <string> message: "Ready";
            in property <bool> live: false;
            grid := GridLayout {
                tile := Rectangle { col: 0; width: 20px; height: 20px; }
            }
            status := Text { text: root.message; width: 120px; }
            badge := Badge {}
            states [
                ignored when false: { tile.col: 3; }
                settled when true: { tile.col: 1; status.height: 24px; badge.width: 70px; }
                later when root.live: { tile.col: 4; }
            ]
        }
        component Timed inherits Window {
            panel := Rectangle { width: 20px; }
            states [ settled when true: {
                panel.width: 40px;
                in { animate panel.width { duration: 160ms; } }
            } ]
        }
        component DynamicValue inherits Window {
            in property <length> target-width: 120px;
            panel := Rectangle { width: 20px; }
            states [ settled when true: { panel.width: root.target-width; } ]
        }
        "#,
        "materialize-states.xr",
    )
    .optimize()
    .unwrap();
    let window = |name| {
        optimized
            .operations()
            .find(|op| op.attribute("sym_name") == Some(name))
            .unwrap()
            .children()
            .find(|op| op.name() == "xr.element")
            .unwrap()
    };
    fn child<'m>(parent: OperationRef<'m>, id: &str) -> OperationRef<'m> {
        parent
            .children()
            .find(|op| op.attribute("id") == Some(id))
            .unwrap()
    }
    let app = window("App");
    assert!(!app.children().any(|op| op.name() == "xr.state_select"));
    let grid = child(app, "grid");
    assert_eq!(grid.attribute("xr.static_grid"), Some("true"));
    assert_eq!(grid.attribute("xr.static_subtree"), Some("true"));
    let tile = child(grid, "tile");
    let col = member(tile, "xr.bind", "col");
    assert_eq!(definition(col, 0).attribute("value"), Some("1"));
    assert!(!tile.children().any(|op| op.name() == "xr.state_set"));
    let text = member(child(app, "status"), "xr.bind", "text");
    assert_eq!(text.attribute("xr.update_effect"), Some("paint"));
    let width = member(child(app, "badge"), "xr.layout_bind", "width");
    assert_eq!(definition(width, 0).attribute("value"), Some("70"));
    let timed = window("Timed");
    assert!(timed.children().any(|op| op.name() == "xr.state_select"));
    let timing = child(timed, "panel");
    assert!(timing.children().any(|op| op.name() == "xr.state_timing"));
    let dynamic = child(window("DynamicValue"), "panel");
    assert!(dynamic.children().any(|op| op.name() == "xr.state_set"));
}

#[test]
fn state_overrides_keep_grid_placement_and_subtree_values_live() {
    let module = raw(
        r#"export component Demo inherits Window {
            in property <bool> compact: false;
            grid := GridLayout { tile := Rectangle { col: 0; width: 16px; height: 16px; } }
            states [ small when root.compact: { tile.col: 1; tile.width: 12px; } ]
        }"#,
        "state-plan.xr",
    )
    .optimize()
    .unwrap();
    let window = module
        .operations()
        .next()
        .unwrap()
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let grid = window
        .children()
        .find(|op| op.attribute("id") == Some("grid"))
        .unwrap();
    let tile = grid
        .children()
        .find(|op| op.attribute("id") == Some("tile"))
        .unwrap();
    assert!(grid.attribute("xr.static_grid").is_none());
    assert!(tile.attribute("xr.static_subtree").is_none());
    let col = member(tile, "xr.state_set", "col");
    assert_eq!(definition(col, 0).name(), "xr.state_select");
}

#[test]
fn simplify_visuals_removes_disabled_timing_and_preserves_delayed_jumps() {
    let input = raw(
        r#"export component Demo inherits Window {
            in property <bool> motion: true;
            Rectangle {
                animate width { duration: 200ms; enabled: false; }
                animate height { duration: 200ms; enabled: true; }
                animate x { delay: 100ms; }
                animate y { duration: 200ms; enabled: root.motion; }
            }
        }"#,
        "animation-pass.xr",
    );
    let optimized = input.optimize().unwrap();
    let component = optimized.operations().next().unwrap();
    let window = component
        .children()
        .find(|op| op.name() == "xr.element")
        .unwrap();
    let item = window
        .children()
        .find(|op| op.attribute("kind") == Some("Rectangle"))
        .unwrap();
    let animations: Vec<_> = item
        .children()
        .filter(|op| op.name() == "xr.animate")
        .collect();
    assert_eq!(animations.len(), 3);
    assert!(member(item, "xr.animate", "height").operand(1).is_none());
    assert_eq!(
        member(item, "xr.animate", "x").integer_attribute("delay"),
        Some(100)
    );
    assert_eq!(
        definition(member(item, "xr.animate", "y"), 1).name(),
        "xr.read"
    );
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
        out property <int> wrap-add: 9223372036854775807 + 1;
        out property <int> wrap-subtract: (0 - 9223372036854775807 - 1) - 1;
        out property <int> wrap-multiply: 4611686018427387904 * 2;
        out property <int> wrap-negate: -(0 - 9223372036854775807 - 1);
        out property <int> div-zero: 12 / 0;
        out property <int> rem-zero: 12 % 0;
        out property <int> div-overflow: (0 - 9223372036854775807 - 1) / -1;
        out property <int> rem-overflow: (0 - 9223372036854775807 - 1) % -1;
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
    for (property, value) in [
        ("wrap-add", "-9223372036854775808"),
        ("wrap-subtract", "9223372036854775807"),
        ("wrap-multiply", "-9223372036854775808"),
        ("wrap-negate", "-9223372036854775808"),
    ] {
        let initializer = definition(member(component, "xr.property", property), 0);
        assert_eq!(initializer.name(), "xr.literal");
        assert_eq!(initializer.attribute("value"), Some(value));
    }
    for (property, operator) in [
        ("div-zero", "/"),
        ("rem-zero", "%"),
        ("div-overflow", "/"),
        ("rem-overflow", "%"),
    ] {
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

#[test]
fn table_selection_reads_stay_live_across_a_selection_command() {
    let optimized = raw(
        r#"component App inherits Window {
            in property <[string]> records;
            out property <int> before;
            out property <int> after;
            callback select();
            select => {
                root.before = table.selection-count;
                table.select-cell(0, 0);
                root.after = table.selection-count;
            }
            table := TableView {
                selection-target: cells;
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { Text { text: row; } }
            }
        }"#,
        "table-selection-order.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let callback = member(component, "xr.callback", "select");
    let operations: Vec<_> = callback
        .children()
        .filter(|op| {
            matches!(
                op.name(),
                "xr.table_selection_read" | "xr.table_selection_command"
            )
        })
        .map(|op| op.name())
        .collect();
    assert_eq!(
        operations,
        [
            "xr.table_selection_read",
            "xr.table_selection_command",
            "xr.table_selection_read"
        ]
    );
}

#[test]
fn table_history_reads_observe_replay_in_the_same_callback() {
    let optimized = raw(
        r#"component App inherits Window {
            in-out property <[string]> records: ["One"];
            out property <bool> before;
            out property <bool> after;
            callback revert();
            revert => {
                root.before = table.can-undo;
                table.undo();
                root.after = table.can-undo;
            }
            table := TableView {
                undoable: true;
                TableColumn { title: "Name"; }
                for row in root.records : TableRow { TableCell { text <=> row; } }
            }
        }"#,
        "table-history-order.xr",
    )
    .optimize()
    .unwrap();
    let component = optimized.operations().next().unwrap();
    let callback = member(component, "xr.callback", "revert");
    let operations: Vec<_> = callback
        .children()
        .map(|op| op.name())
        .filter(|name| matches!(*name, "xr.table_history_read" | "xr.table_history_command"))
        .collect();
    assert_eq!(
        operations,
        [
            "xr.table_history_read",
            "xr.table_history_command",
            "xr.table_history_read"
        ]
    );
}

#[test]
fn parsed_operator_signatures_match_xr_expression_types() {
    let module = |operator: &str, lhs: (&str, &str), rhs: Option<(&str, &str)>, result: &str| {
        let (left_type, left_value) = lhs;
        let (right, operation) = if let Some((right_type, right_value)) = rhs {
            (
                format!(
                    r#"%right = "xr.literal"() {{value = "{right_value}"}} : () -> !xr.{right_type}"#
                ),
                format!(
                    r#""xr.binary"(%left, %right) {{value = "{operator}"}} : (!xr.{left_type}, !xr.{right_type}) -> !xr.{result}"#
                ),
            )
        } else {
            (
                String::new(),
                format!(
                    r#""xr.unary"(%left) {{value = "{operator}"}} : (!xr.{left_type}) -> !xr.{result}"#
                ),
            )
        };
        format!(
            r#"module {{
              "xr.component"() ({{
                %left = "xr.literal"() {{value = "{left_value}"}} : () -> !xr.{left_type}
                {right}
                %value = {operation}
                "xr.property"(%value) {{name = "result", visibility = "out"}} : (!xr.{result}) -> ()
                "xr.element"() ({{ "xr.end"() : () -> () }}) {{id = "root", key = "root", kind = "Window"}} : () -> ()
                "xr.end"() : () -> ()
              }}) {{sym_name = "Probe", exported = "true"}} : () -> ()
            }}"#,
        )
    };
    let int = ("int", "8");
    let float = ("float", "2.5");
    let length = ("length", "12");
    let percent = ("percent", "15");
    let boolean = ("bool", "true");
    let string = ("string", r#"\22hello\22"#);
    for (operator, left, right, result) in [
        ("+", int, None, "int"),
        ("-", length, None, "length"),
        ("!", boolean, None, "bool"),
        ("==", percent, Some(percent), "bool"),
        ("<", length, Some(length), "bool"),
        ("/", length, Some(length), "float"),
        ("/", length, Some(float), "length"),
        ("*", float, Some(length), "length"),
        ("%", int, Some(int), "int"),
        ("&&", boolean, Some(boolean), "bool"),
        ("+", string, Some(string), "string"),
    ] {
        let parsed = Module::parse(&module(operator, left, right, result)).unwrap();
        codegen::generate(&parsed).unwrap();
        codegen::generate(&parsed.optimize().unwrap()).unwrap();
    }
    for (operator, left, right, result) in [
        ("!", int, None, "int"),
        ("-", percent, None, "percent"),
        ("?", boolean, None, "bool"),
        ("+", percent, Some(percent), "percent"),
        ("<", percent, Some(percent), "bool"),
        ("==", int, Some(float), "bool"),
        ("/", length, Some(length), "length"),
        ("+", float, Some(length), "length"),
        ("/", float, Some(length), "length"),
        ("%", float, Some(float), "float"),
        ("+", int, Some(int), "bool"),
        ("-", string, Some(string), "string"),
        ("||", boolean, Some(boolean), "int"),
        ("??", int, Some(int), "int"),
    ] {
        let source = module(operator, left, right, result);
        let error = Module::parse(&source).expect_err(&source);
        assert!(
            error.contains("invalid operand or result type for operator"),
            "{error}"
        );
    }
}
