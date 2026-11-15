use xrack_layout::{ActivationKey, Interaction, Point, Size, SliderKey, TextMeasurer, layout};
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
        (
            "component X inherits Rectangle { Button { ToggleIndicator {} } }",
            "static child of CheckBox or RadioButton or Switch",
        ),
        (
            "component X inherits Rectangle { Switch { if true : ToggleIndicator {} } }",
            "requires a static child",
        ),
        (
            "component X inherits Rectangle { CheckBox { ToggleIndicator {} ToggleIndicator {} } }",
            "at most one ToggleIndicator",
        ),
        (
            "component X inherits Rectangle { CheckBox { SwitchThumb {} } }",
            "static child of Switch",
        ),
        (
            "component X inherits Rectangle { Switch { SwitchThumb {} SwitchThumb {} } }",
            "at most one SwitchThumb",
        ),
        (
            "component X inherits Rectangle { Switch { if true : SwitchThumb {} } }",
            "requires a static child",
        ),
        (
            "component X inherits Rectangle { Slider { SpinUpIndicator {} } }",
            "static child of SpinBox or DoubleSpinBox",
        ),
        (
            "component X inherits Rectangle { SpinBox { SpinDownIndicator {} SpinDownIndicator {} } }",
            "at most one SpinDownIndicator",
        ),
        (
            "component X inherits Rectangle { RangeSlider { RangeUpperHandle {} RangeUpperHandle {} } }",
            "at most one RangeUpperHandle",
        ),
        (
            "component X inherits Rectangle { TabBar { TabButton { ButtonContent {} ButtonContent {} } } }",
            "at most one ButtonContent",
        ),
        (
            "component X inherits Rectangle { TabBar { TabButton { if true : ButtonContent {} } } }",
            "requires a static child",
        ),
        (
            "component X inherits Rectangle { Slider { RangeLowerHandle {} } }",
            "static child of RangeSlider",
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
    assert_eq!(
        preview.components,
        [
            "SlotDemo",
            "GlobalDemo",
            "ToggleIndicatorDemo",
            "SpinIndicatorDemo",
            "RangeHandleDemo",
            "TabContentDemo"
        ]
    );
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

#[test]
fn replacement_toggle_indicators_keep_pointer_keyboard_and_group_behavior() {
    use xrack_runtime::{CheckState, Value};
    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let mut interpreted = xrack_interpreter::load_mlir(&ir.to_mlir())
        .unwrap()
        .create("ToggleIndicatorDemo")
        .unwrap();
    let mut generated = slots_ui::ToggleIndicatorDemo::new();
    let viewport = Size::new(420.0, 260.0);

    for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
        let mut input = Interaction::default();
        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let checkbox_mark = frame.at_path(&[0, 0, 0]).unwrap().bounds;
        assert_eq!((checkbox_mark.width, checkbox_mark.height), (34.0, 26.0));
        input.pointer_move(
            &scene,
            &frame,
            Point {
                x: checkbox_mark.x + 8.0,
                y: checkbox_mark.y + 8.0,
            },
        );
        input.pointer_press_in_scene(&scene, &frame);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.element_id(), Some("master"));
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));

        let scene = ui.render();
        assert_eq!(
            scene.find("master").unwrap().check_state(),
            CheckState::PartiallyChecked
        );
        assert_eq!(
            scene.at_path(&[0, 0, 0, 0, 0]).unwrap().text("text"),
            Some("–")
        );
        let frame = layout(&scene, viewport, &mut Text);
        let radio_mark = frame.at_path(&[0, 1, 1, 0]).unwrap().bounds;
        input.pointer_move(
            &scene,
            &frame,
            Point {
                x: radio_mark.x + 8.0,
                y: radio_mark.y + 8.0,
            },
        );
        input.pointer_press_in_scene(&scene, &frame);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.element_id(), Some("shared"));
        let action = input.select_group_option(&scene, &frame, &target).unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));

        let scene = ui.render();
        assert_eq!(
            scene.at_path(&[0, 1, 1, 0, 0, 0]).unwrap().text("text"),
            Some("S")
        );
        let frame = layout(&scene, viewport, &mut Text);
        let target = input.accessibility_target(&scene, &frame, &[0, 2]).unwrap();
        input.accessibility_focus(target);
        input.key_press_with_scene(&scene, ActivationKey::Space);
        let target = input
            .key_release_with_scene(&scene, ActivationKey::Space)
            .unwrap();
        assert_eq!(target.element_id(), Some("online"));
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
        let scene = ui.render();
        assert_eq!(
            scene.at_path(&[0, 2, 0, 0, 0]).unwrap().text("text"),
            Some("ON")
        );
    }
    assert_eq!(generated.get_tri(), CheckState::PartiallyChecked);
    assert_eq!(generated.get_profile(), 1);
    assert!(generated.get_network());
    assert_eq!(
        interpreted.get("tri"),
        Ok(Value::CheckState(CheckState::PartiallyChecked))
    );
    assert_eq!(interpreted.get("profile"), Ok(Value::Int(1)));
    assert_eq!(interpreted.get("network"), Ok(Value::Bool(true)));

    generated.set_option_enabled(false);
    interpreted
        .set("option-enabled", Value::Bool(false))
        .unwrap();
    for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let mark = frame.at_path(&[0, 2, 0]).unwrap().bounds;
        let mut input = Interaction::default();
        input.pointer_move(
            &scene,
            &frame,
            Point {
                x: mark.x + 8.0,
                y: mark.y + 8.0,
            },
        );
        input.pointer_press_in_scene(&scene, &frame);
        assert!(input.pointer_release_target().is_none());
        assert!(
            ui.render()
                .find("online")
                .unwrap()
                .boolean("checked", false)
        );
    }
}

#[test]
fn spin_indicators_share_geometry_with_hits_and_preserve_editing() {
    use std::time::{Duration, Instant};
    use xrack_layout::{EditAction, ScrollGestureKind, SpinGeometry};
    use xrack_runtime::{LayoutDirection, Value};

    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let program = xrack_interpreter::load_mlir(&ir.to_mlir()).unwrap();
    let viewport = Size::new(440.0, 250.0);

    fn step(
        ui: &mut dyn Component,
        input: &mut Interaction,
        viewport: Size,
        path: &[usize],
        upward: bool,
        expected_width: f32,
    ) -> bool {
        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let spin = frame.at_path(path).unwrap();
        let geometry = SpinGeometry::for_layout(spin, scene.at_path(path).unwrap());
        let bounds = if upward { geometry.up } else { geometry.down };
        // Click in the portion of the replacement outside the stock 28 px hit area.
        let rtl = spin.layout_direction == LayoutDirection::RightToLeft;
        let point = Point {
            x: if rtl {
                bounds.x + bounds.width - 5.0
            } else {
                bounds.x + 5.0
            },
            y: bounds.y + bounds.height * 0.5,
        };
        assert_eq!(bounds.width, expected_width);
        if rtl {
            assert_eq!(geometry.up.x, spin.bounds.x);
            assert_eq!(geometry.up.x + geometry.up.width, geometry.text.x);
        } else {
            assert_eq!(geometry.text.x + geometry.text.width, geometry.up.x);
        }
        if let Some(part) = spin.child_at(usize::from(!upward)) {
            assert_eq!(part.bounds, bounds);
        }
        input.pointer_move(&scene, &frame, point);
        input.pointer_press_in_scene(&scene, &frame);
        input.spin_press(&scene, &frame);
        input.spin_release(&scene, &frame).is_some_and(|action| {
            assert_eq!(action.path, path);
            ui.control_at(&scene, &action.path, action.action)
        })
    }

    for rtl in [false, true] {
        let mut generated = slots_ui::SpinIndicatorDemo::new();
        generated.set_rtl(rtl);
        let mut interpreted = program.create("SpinIndicatorDemo").unwrap();
        interpreted.set("rtl", rtl.into()).unwrap();
        let quantity = [0, 1];
        let price = [0, 2];
        for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
            let mut input = Interaction::default();
            assert!(step(ui, &mut input, viewport, &quantity, true, 52.0));
            assert!(step(ui, &mut input, viewport, &quantity, false, 52.0));
            assert!(step(ui, &mut input, viewport, &price, true, 64.0));
            assert!(step(ui, &mut input, viewport, &price, false, 64.0));

            let scene = ui.render();
            let frame = layout(&scene, viewport, &mut Text);
            let spin = scene.find("quantity-field").unwrap();
            let geom = SpinGeometry::for_layout(frame.at_path(&quantity).unwrap(), spin);
            let edge = Point {
                x: geom.up.x + 5.0,
                y: geom.down.y,
            };
            let now = Instant::now();
            let press = Point {
                y: edge.y - 1.0,
                ..edge
            };
            let release = Point {
                y: edge.y + 1.0,
                ..edge
            };
            assert!(input.begin_scroll_gesture(
                &scene,
                &frame,
                press,
                now,
                ScrollGestureKind::Touch
            ));
            let tap =
                input.end_scroll_gesture(&scene, &frame, release, now + Duration::from_millis(20));
            assert!(!tap.tap && tap.control.is_none());
            let target = input
                .accessibility_target(&scene, &frame, &quantity)
                .unwrap();
            input.accessibility_focus(target);
            input.edit_spin(spin, EditAction::SelectAll);
            input.edit_spin(spin, EditAction::Insert("7"));
            let commit = input.commit_spin_draft().unwrap();
            assert!(ui.control_at(&scene, &commit.path, commit.action));
        }
        assert_eq!(generated.get_quantity(), 7);
        assert_eq!(generated.get_price(), 1.25);
        assert_eq!(generated.get_edits(), 5);
        assert_eq!(interpreted.get("quantity"), Ok(Value::Int(7)));
        assert_eq!(interpreted.get("price"), Ok(Value::Float(1.25)));
        assert_eq!(interpreted.get("edits"), Ok(Value::Int(5)));

        generated.set_locked(true);
        interpreted.set("locked", true.into()).unwrap();
        for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
            assert!(!step(
                ui,
                &mut Interaction::default(),
                viewport,
                &quantity,
                true,
                52.0
            ));
        }
    }
}

#[test]
fn custom_range_handles_follow_touch_preview_and_keep_two_endpoint_controls() {
    use xrack_layout::{SliderGeometry, layout_with_input};
    use xrack_runtime::Value;

    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let mut interpreted = xrack_interpreter::load_mlir(&ir.to_mlir())
        .unwrap()
        .create("RangeHandleDemo")
        .unwrap();
    let mut generated = slots_ui::RangeHandleDemo::new();
    let viewport = Size::new(440.0, 300.0);
    let lower = [0, 1, 0];

    for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
        let mut input = Interaction::default();
        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let band = scene.find("band").unwrap();
        let track = SliderGeometry::for_node(frame.at_path(&[0, 1]).unwrap().bounds, band);
        let center = track.position(0.2);
        let start = Point {
            x: center.x + 15.0,
            y: center.y,
        };
        let before = frame.at_path(&lower).unwrap().bounds;
        assert!(before.contains(start), "the artwork extends the hit region");
        assert_eq!(before.width, 42.0);
        assert_eq!(frame.at_path(&[0, 1, 2]).unwrap().bounds, before);
        assert!(input.touch_range_press(&scene, &frame, start));
        assert!(input.slider_press(&scene, &frame).is_none());
        assert!(input.is_focused(&lower));
        let next = track.position(0.47);
        input.pointer_move(
            &scene,
            &frame,
            Point {
                x: next.x + 15.0,
                y: next.y,
            },
        );
        assert!(input.slider_drag(&scene, &frame).is_none());
        assert_eq!(
            ui.render().find("band").unwrap().number("lower-value"),
            Some(20.0)
        );

        let preview = layout_with_input(&scene, viewport, &mut Text, &input);
        let draft = preview.at_path(&lower).unwrap().bounds;
        assert_eq!(preview.at_path(&[0, 1, 2]).unwrap().bounds, draft);
        assert!((draft.x + draft.width * 0.5 - next.x).abs() < 0.01);
        assert_eq!(
            preview.at_path(&[0, 1, 1]).unwrap().bounds,
            frame.at_path(&[0, 1, 1]).unwrap().bounds,
        );

        let release = input.slider_release(&scene, &preview).unwrap();
        assert_eq!(release.path, lower);
        assert!(ui.adjust_range_at(&scene, &release.path, release.action));
        input.finish_touch_range();

        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let upper = [0, 2, 1];
        let bounds = frame.at_path(&upper).unwrap().bounds;
        assert_eq!(bounds.height, 38.0);
        assert_eq!(frame.at_path(&[0, 2, 2]).unwrap().bounds, bounds);
        let target = input.accessibility_target(&scene, &frame, &upper).unwrap();
        input.accessibility_focus(target);
        let adjust = input
            .slider_key(&scene, &frame, SliderKey::Up, true)
            .unwrap();
        assert_eq!(adjust.path, upper);
        assert!(ui.adjust_range_at(&scene, &adjust.path, adjust.action));
        let scene = ui.render();
        let next = layout(&scene, viewport, &mut Text)
            .at_path(&upper)
            .unwrap()
            .bounds;
        assert!(next.y < bounds.y);
    }
    assert_eq!(generated.get_low(), 50.0);
    assert_eq!(generated.get_high(), 80.0);
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(generated.get_releases(), 1);
    assert_eq!(interpreted.get("low"), Ok(Value::Float(50.0)));
    assert_eq!(interpreted.get("high"), Ok(Value::Float(80.0)));
    assert_eq!(interpreted.get("changes"), Ok(Value::Int(1)));
    assert_eq!(interpreted.get("releases"), Ok(Value::Int(1)));
}

#[test]
fn tab_content_uses_repeated_row_context_and_keeps_navigation_in_the_tab_bar() {
    use xrack_layout::NavigationKey;
    use xrack_runtime::{Color, Orientation, Value};

    let doc = xrack_compiler::parse(SOURCE).unwrap();
    let ir = xrack_compiler::lower(&doc, "slots.xr").unwrap();
    let mut interpreted = xrack_interpreter::load_mlir(&ir.to_mlir())
        .unwrap()
        .create("TabContentDemo")
        .unwrap();
    let mut generated = slots_ui::TabContentDemo::new();
    let viewport = Size::new(432.0, 246.0);

    for ui in [&mut generated as &mut dyn Component, &mut interpreted] {
        let mut input = Interaction::default();
        let scene = ui.render();
        let bar = scene.find("tabs").unwrap();
        for (index, label) in ["Overview", "Reports", "Settings"].into_iter().enumerate() {
            let content = bar.children[index].control_part("ButtonContent").unwrap();
            assert_eq!(content.children[0].children[0].text("text"), Some(label));
        }
        let frame = layout(&scene, viewport, &mut Text);
        let first = frame.at_path(&[0, 0]).unwrap().bounds;
        let adornment = frame.at_path(&[0, 0, 0, 0]).unwrap().bounds;
        assert_eq!(first.width, 120.0);
        assert_eq!(adornment.width, 100.0);
        assert_eq!(frame.at_path(&[3, 0]).unwrap().bounds.height, 44.0);
        input.pointer_move(
            &scene,
            &frame,
            Point {
                x: adornment.x + 10.0,
                y: adornment.y + 9.0,
            },
        );
        input.pointer_press_in_scene(&scene, &frame);
        let pressed = input.pointer_release_target().unwrap();
        assert_eq!(pressed.path, [0, 0]);
        input.accessibility_focus(pressed);
        let action = input
            .group_key(&scene, &frame, NavigationKey::Right)
            .unwrap();
        assert_eq!(action.path, [0]);
        assert!(ui.control_at(&scene, &action.path, action.action));
        assert!(input.is_focused(&[0, 2]));

        let scene = ui.render();
        let frame = layout(&scene, viewport, &mut Text);
        let tab_bar = frame.at_path(&[0]).unwrap();
        assert!(tab_bar.scroll_on(Orientation::Horizontal).unwrap().offset > 0.0);
        let selected = frame.at_path(&[0, 2]).unwrap().bounds;
        assert_eq!(selected.intersect(tab_bar.bounds), selected);
        assert_eq!(
            scene.at_path(&[0, 2, 0, 0]).unwrap().color("background"),
            Some(Color(0x208c78ff)),
        );
        assert!(
            frame
                .at_path(&[3])
                .unwrap()
                .scroll_on(Orientation::Vertical)
                .unwrap()
                .offset
                > 0.0,
        );
    }
    assert_eq!(generated.get_selected(), 2);
    assert_eq!(interpreted.get("selected"), Ok(Value::Int(2)));

    let names = vec!["Overview".into(), "Reports".into(), "Build queue".into()];
    generated.set_sections(names.clone());
    interpreted
        .set("sections", Value::StringList(names))
        .unwrap();
    for ui in [&generated as &dyn Component, &interpreted] {
        let scene = ui.render();
        let badge = scene.at_path(&[0, 2, 0, 0]).unwrap();
        assert_eq!(badge.children[0].text("text"), Some("Build queue"));
        assert_eq!(badge.color("background"), Some(Color(0x208c78ff)));
    }
}

#[test]
fn mlir_requires_a_switch_parent_for_the_thumb_slot() {
    let doc = xrack_compiler::parse(
        "export component X inherits Window { Switch { SwitchThumb {} ToggleIndicator {} } }",
    )
    .unwrap();
    let ir = xrack_compiler::lower(&doc, "switch-slot.xr")
        .unwrap()
        .to_mlir();
    let valid = xrack_compiler::prepare_mlir_preview(&ir, "switch-slot.mlir").unwrap();
    assert_eq!(valid.components, ["X"]);
    let wrong_owner = ir.replace("kind = \"Switch\"", "kind = \"CheckBox\"");
    assert_ne!(wrong_owner, ir);
    let error = xrack_compiler::prepare_mlir_preview(&wrong_owner, "switch-slot.mlir")
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("static child of Switch"),
        "{error}"
    );
}
