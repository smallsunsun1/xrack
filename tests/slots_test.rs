use xrack_layout::{ActivationKey, Interaction, Size, SliderKey, TextMeasurer, layout};
use xrack_runtime::Component;

const SOURCE: &str = include_str!("../examples/slots/slots.xr");

struct Text;
impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 7.0).min(width), 16.0)
    }
}

#[test]
fn supplied_content_uses_caller_bindings_and_keeps_control_interaction() {
    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let program = xrack_interpreter::Program::from_module(&ir).unwrap();
    let mut ui = slots_ui::SlotDemo::new();
    let mut interpreted = program.create("SlotDemo").unwrap();
    for component in [&mut ui as &mut dyn Component, &mut interpreted] {
        let mut input = Interaction::default();
        let scene = component.render();
        let panel = scene.find("panel").expect("slotted panel");
        assert_eq!(panel.children[0].text("text"), Some("Audio settings"));
        let body = &panel.children[1];
        assert_eq!(body.children[0].text("text"), Some("Volume 40"));
        assert_eq!(body.children[1].kind, "Button");
        assert_eq!(body.children[1].children[0].kind, "ButtonContent");
        assert_eq!(body.children[2].kind, "Slider");
        assert_eq!(body.children[2].children[0].kind, "SliderHandle");
        let frame = layout(&scene, Size::new(480.0, 270.0), &mut Text);
        input.focus_next(&scene, &frame, false);
        assert_eq!(input.focused.as_ref().unwrap().element_id(), Some("apply"));
        input.key_press_with_scene(&scene, ActivationKey::Space);
        let target = input
            .key_release_with_scene(&scene, ActivationKey::Space)
            .unwrap();
        assert!(component.dispatch_at(&scene, &target.path, "clicked"));
        let scene = component.render();
        let frame = layout(&scene, Size::new(480.0, 270.0), &mut Text);
        input.reconcile(&scene, &frame);
        input.focus_next(&scene, &frame, false);
        let target = input.focused.as_ref().unwrap().clone();
        assert_eq!(target.element_id(), Some("volume-slider"));
        let thumb_before = frame
            .at_path(&target.path)
            .unwrap()
            .child_at(0)
            .unwrap()
            .bounds;
        let adjust = input
            .slider_key(&scene, &frame, SliderKey::Right, true)
            .unwrap();
        assert!(component.adjust_range_at(&scene, &adjust.path, adjust.action));
        let scene = component.render();
        let frame = layout(&scene, Size::new(480.0, 270.0), &mut Text);
        let thumb_after = frame
            .at_path(&target.path)
            .unwrap()
            .child_at(0)
            .unwrap()
            .bounds;
        assert!(thumb_after.x > thumb_before.x);
        assert!(thumb_after.width > 0.0);
    }
    assert_eq!(ui.get_applied(), 1);
    assert_eq!(ui.get_volume(), 45.0);
    assert_eq!(
        interpreted.get("applied").unwrap(),
        xrack_runtime::Value::Int(1)
    );
    assert_eq!(
        interpreted.get("volume").unwrap(),
        xrack_runtime::Value::Float(45.0)
    );
}

#[test]
fn slot_validation_rejects_ambiguous_or_conditional_placement() {
    for (source, expected) in [
        (
            "component X inherits Rectangle { @children @children }",
            "only once",
        ),
        (
            "component X inherits Rectangle { if true : Rectangle { @children } }",
            "non-repeated parent",
        ),
        (
            "component X inherits Rectangle { SliderHandle {} }",
            "static child of Slider",
        ),
        (
            "component X inherits Rectangle { Button { ButtonContent {} ButtonContent {} } }",
            "at most one ButtonContent",
        ),
    ] {
        let doc = xrack_compiler::parse(source).unwrap();
        let error = xrack_compiler::lower(&doc, "invalid-slots.xr").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn globals_are_reactive_and_shared_per_instance_tree() {
    use xrack_runtime::Value;
    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let preview = xrack_compiler::prepare_mlir_preview(&ir.to_mlir(), "slots.mlir").unwrap();
    assert_eq!(preview.components, ["SlotDemo", "GlobalDemo"]);
    let program = xrack_interpreter::Program::from_module(&ir).unwrap();
    assert!(
        !program
            .exported_components()
            .any(|name| name == "SharedSettings")
    );
    let mut aot = slots_ui::GlobalDemo::new();
    let untouched_aot = slots_ui::GlobalDemo::new();
    let mut interpreted = program.create("GlobalDemo").unwrap();
    let untouched_interpreter = program.create("GlobalDemo").unwrap();
    aot.global_SharedSettings().set_count(7);
    interpreted
        .global("SharedSettings")
        .unwrap()
        .set("count", Value::Int(7))
        .unwrap();
    for component in [&mut aot as &mut dyn Component, &mut interpreted] {
        let scene = component.render();
        assert_eq!(scene.children[0].text("text"), Some("Shared: 14"));
        assert_eq!(scene.children[1].text("text"), Some("Shared: 14"));
        assert!(component.dispatch("update", "clicked"));
        let scene = component.render();
        assert_eq!(scene.children[0].text("text"), Some("Shared: 16"));
        assert_eq!(scene.children[1].text("text"), Some("Shared: 16"));
    }
    assert_eq!(aot.get_current(), 8);
    assert_eq!(interpreted.get("current").unwrap(), Value::Int(8));
    assert_eq!(untouched_aot.get_current(), 2);
    assert_eq!(untouched_interpreter.get("current").unwrap(), Value::Int(2));
}

#[test]
fn standalone_mlir_rejects_global_property_and_callback_cycles() {
    use xrack_compiler::mlir::{Module, lower};
    use xrack_interpreter::{Error, Program, load_mlir};

    let source = r#"
        export global Left {
            in-out property <int> initial: 4;
            in-out property <int> seed: 4;
            out property <int> value: Right.value;
            pure callback read() -> int;
            read => { Right.read() }
            callback kick();
            kick => { Right.finish(); }
            callback stop();
            stop => { root.seed += 1; }
        }
        export global Right {
            out property <int> value: Left.initial;
            pure callback read() -> int;
            read => { Left.seed }
            callback finish();
            finish => { Left.stop(); }
        }
        export component App inherits Window {
            out property <int> result: Left.read();
        }
    "#;
    let document = xrack_compiler::parse(source).unwrap();
    let raw = lower(&document, "global-graph.xr").unwrap();
    let valid = Module::parse(&raw).unwrap();
    let ui = Program::from_module(&valid).unwrap().create("App").unwrap();
    assert_eq!(ui.get("result"), Ok(4_i64.into()));
    ui.global("Left").unwrap().invoke("kick", &[]).unwrap();
    assert_eq!(ui.get("result"), Ok(5_i64.into()));
    assert!(xrack_compiler::codegen::generate(&valid).is_ok());

    for (from, to, member) in [
        (
            r#""xr.read"() <{owner = "__global_Left", value = "initial"}>"#,
            r#""xr.read"() <{owner = "__global_Left", value = "value"}>"#,
            "Left.root.value",
        ),
        (
            r#""xr.call"() <{owner = "__global_Right", value = "read"}>"#,
            r#""xr.call"() <{owner = "root", value = "read"}>"#,
            "Left.root.read()",
        ),
        (
            r#""xr.read"() <{owner = "__global_Left", value = "seed"}>"#,
            r#""xr.call"() <{owner = "__global_Left", value = "read"}>"#,
            "Left.root.read()",
        ),
        (
            r#""xr.invoke"() <{name = "stop", owner = "__global_Left"}>"#,
            r#""xr.invoke"() <{name = "kick", owner = "__global_Left"}>"#,
            "Left.root.kick()",
        ),
    ] {
        let malformed = raw.replacen(from, to, 1);
        assert_ne!(malformed, raw);
        let diagnostic = Module::parse(&malformed).unwrap_err();
        assert!(diagnostic.contains("global-graph.xr"), "{diagnostic}");
        assert!(
            diagnostic.contains(&format!(
                "cyclic global binding or callback call involving `{member}`"
            )),
            "{diagnostic}"
        );
        assert_eq!(
            load_mlir(&malformed).err(),
            Some(Error::Invalid(diagnostic))
        );
    }
}

#[test]
fn standalone_mlir_rejects_component_property_and_callback_cycles() {
    use xrack_compiler::mlir::{Module, lower};
    use xrack_interpreter::{Error, load_mlir};

    let source = r#"
        component Worker inherits Rectangle {
            in-out property <int> seed: 2;
            pure callback value() -> int;
            value => { root.seed }
        }
        export component App inherits Window {
            in-out property <int> initial: 4;
            out property <int> derived: root.initial;
            out property <int> result: worker.value();
            pure callback read() -> int;
            read => { root.derived }
            worker := Worker { seed: root.initial; }
            states [ alternate when root.initial > 4: { worker.seed: root.derived + 1; } ]
        }
    "#;
    let file = "component-dependencies.xr";
    let document = xrack_compiler::parse(source).unwrap();
    let raw = lower(&document, file).unwrap();
    let module = Module::parse(&raw).unwrap();
    assert!(xrack_compiler::codegen::generate(&module).is_ok());
    let ui = load_mlir(&raw).unwrap().create("App").unwrap();
    assert_eq!(ui.get("result"), Ok(4_i64.into()));
    ui.set("initial", 9_i64.into()).unwrap();
    assert_eq!(ui.get("result"), Ok(10_i64.into()));
    ui.set("initial", 3_i64.into()).unwrap();
    assert_eq!(ui.get("result"), Ok(3_i64.into()));

    let initial_read = r#""xr.read"() <{owner = "root", value = "initial"}>"#;
    let worker_input = raw.rfind(initial_read).unwrap();
    let mut instance_cycle = raw.clone();
    instance_cycle.replace_range(
        worker_input..worker_input + initial_read.len(),
        r#""xr.read"() <{owner = "root", value = "result"}>"#,
    );
    let mut state_cycle = raw.clone();
    let derived_read = r#""xr.read"() <{owner = "root", value = "derived"}>"#;
    let state_input = raw.rfind(derived_read).unwrap();
    state_cycle.replace_range(
        state_input..state_input + derived_read.len(),
        r#""xr.read"() <{owner = "root", value = "result"}>"#,
    );
    let cycles = [
        raw.replacen(
            initial_read,
            r#""xr.read"() <{owner = "root", value = "derived"}>"#,
            1,
        ),
        raw.replacen(
            derived_read,
            r#""xr.call"() <{owner = "root", value = "read"}>"#,
            1,
        ),
        instance_cycle,
        state_cycle,
    ];
    for invalid in cycles {
        assert_ne!(invalid, raw);
        let error = Module::parse(&invalid).unwrap_err();
        assert!(
            error.contains(file) && error.contains("cyclic property binding or callback call"),
            "{error}"
        );
        assert_eq!(load_mlir(&invalid).err(), Some(Error::Invalid(error)));
    }
}

#[test]
fn standalone_mlir_globals_follow_source_structure_and_callback_rules() {
    use xrack_compiler::mlir::{Module, lower};
    use xrack_interpreter::{Error, load_mlir};

    let source = r#"
        export global Settings {
            pure callback score() -> int;
            score => { 7 }
            pure callback local-score <=> root.score;
        }
        export component App inherits Rectangle {
            pure callback score() -> int;
            score => { Settings.score() }
            pure callback exposed <=> root.score;
        }
    "#;
    let file = "global-structure.xr";
    let document = xrack_compiler::parse(source).unwrap();
    let raw = lower(&document, file).unwrap();
    let module = Module::parse(&raw).unwrap();
    assert!(xrack_compiler::codegen::generate(&module).is_ok());
    let ui = load_mlir(&raw).unwrap().create("App").unwrap();
    assert_eq!(ui.invoke("exposed", &[]), Ok(Some(7_i64.into())));
    assert_eq!(
        ui.global("Settings").unwrap().invoke("local-score", &[]),
        Ok(Some(7_i64.into()))
    );

    let alias_error = "global callbacks can be called but cannot be aliased";
    let invalid_source = source.replace("exposed <=> root.score", "exposed <=> Settings.score");
    let source_error = xrack_compiler::compile(&invalid_source, file)
        .err()
        .unwrap();
    assert!(
        source_error.to_string().contains(alias_error),
        "{source_error}"
    );

    let root = r#"kind = "Rectangle"}> ({"#;
    let child = r#"kind = "Rectangle"}> ({
        "xr.element"() <{id = "", kind = "Label"}> ({
          "xr.end"() : () -> ()
        }) : () -> ()"#;
    for (invalid, message) in [
        (
            raw.replace(
                r#"alias_owner = "root", name = "exposed""#,
                r#"alias_owner = "__global_Settings", name = "exposed""#,
            ),
            alias_error,
        ),
        (
            raw.replacen(root, r#"kind = "Window"}> ({"#, 1),
            "global requires an empty Rectangle root",
        ),
        (
            raw.replacen(root, child, 1),
            "global requires an empty Rectangle root",
        ),
    ] {
        assert_ne!(invalid, raw);
        assert!(Module::parse(&invalid).unwrap_err().contains(message));
        assert!(matches!(
            load_mlir(&invalid).err(),
            Some(Error::Invalid(error)) if error.contains(message)
        ));
    }
}

#[test]
fn a_global_upgraded_from_weak_keeps_its_tree_alive() {
    use xrack_runtime::Value;

    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let program = xrack_interpreter::Program::from_module(&ir).unwrap();
    let aot = slots_ui::GlobalDemo::new();
    let interpreted = program.create("GlobalDemo").unwrap();
    let aot_summary = aot.global_SharedSummary().as_weak();
    let interpreted_summary = interpreted.global("SharedSummary").unwrap().as_weak();
    let aot_settings = aot.global_SharedSettings().as_weak();
    let interpreted_settings = interpreted.global("SharedSettings").unwrap().as_weak();
    let mut aot_handle = aot_summary.upgrade().unwrap();
    let interpreted_handle = interpreted_summary.upgrade().unwrap();
    drop(aot);
    drop(interpreted);

    aot_handle.invoke_increment();
    interpreted_handle.invoke("increment", &[]).unwrap();
    assert_eq!(aot_handle.get_value(), 6);
    assert_eq!(interpreted_handle.get("value"), Ok(Value::Int(6)));
    assert_eq!(aot_settings.upgrade().unwrap().get_count(), 3);
    assert_eq!(
        interpreted_settings.upgrade().unwrap().get("count"),
        Ok(Value::Int(3))
    );

    drop(aot_handle);
    drop(interpreted_handle);
    assert!(aot_summary.upgrade().is_none());
    assert!(interpreted_summary.upgrade().is_none());
    assert!(aot_settings.upgrade().is_none());
    assert!(interpreted_settings.upgrade().is_none());
}
