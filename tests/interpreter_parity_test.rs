mod interpreter_examples;
#[allow(dead_code)]
mod ui_driver;

use counter_ui::Counter;
use xrack_interpreter::{Instance, Program};
use xrack_runtime::{Component, Node, Value};

fn assert_same_scene(interpreted: &Node, generated: &Node) {
    assert_eq!(interpreted.kind, generated.kind);
    assert_eq!(
        interpreted.element_id(),
        generated.element_id(),
        "{}",
        generated.kind
    );
    assert!(
        interpreted
            .properties
            .keys()
            .eq(generated.properties.keys()),
        "{} properties differ: {:?} vs {:?}",
        generated.kind,
        interpreted.properties.keys().collect::<Vec<_>>(),
        generated.properties.keys().collect::<Vec<_>>(),
    );
    for (&name, value) in &generated.properties {
        if name == "__picker-session" {
            assert!(interpreted.int(name).is_some_and(|id| id > 0));
            assert!(generated.int(name).is_some_and(|id| id > 0));
        } else if matches!(value, Value::ComboModel(_)) {
            assert!(matches!(interpreted.properties[name], Value::ComboModel(_)));
            use xrack_runtime::{
                choice_enabled, combo_count, combo_index, combo_label, combo_value,
            };
            assert_eq!(combo_count(interpreted), combo_count(generated));
            assert_eq!(combo_index(interpreted), combo_index(generated));
            for index in 0..combo_count(generated) {
                assert_eq!(
                    combo_label(interpreted, index),
                    combo_label(generated, index)
                );
                assert_eq!(
                    combo_value(interpreted, index),
                    combo_value(generated, index)
                );
                assert_eq!(
                    choice_enabled(interpreted, index),
                    choice_enabled(generated, index)
                );
            }
        } else if matches!(value, Value::Image(_)) {
            let interpreted = interpreted.image_asset(name).unwrap();
            let generated = generated.image_asset(name).unwrap();
            if matches!(value, Value::Image(xrack_runtime::Image::Animated(_))) {
                assert_eq!(interpreted.size(), generated.size(), "{name}");
                assert_eq!(interpreted.frame_count(), generated.frame_count(), "{name}");
                for index in 0..generated.frame_count() {
                    assert_eq!(
                        interpreted.frame(index),
                        generated.frame(index),
                        "{name} frame {index}"
                    );
                }
            } else {
                assert_eq!(interpreted, generated, "{name}");
            }
        } else {
            assert_eq!(&interpreted.properties[name], value, "{name}");
        }
    }
    assert_eq!(interpreted.animations, generated.animations);
    assert_eq!(interpreted.state_timings, generated.state_timings);
    assert_eq!(interpreted.children.len(), generated.children.len());
    for (interpreted, generated) in interpreted.children.iter().zip(&generated.children) {
        assert_same_scene(interpreted, generated);
    }
}

fn assert_same_counter(interpreted: &Instance, generated: &Counter) {
    assert_eq!(
        interpreted.get("doubled").unwrap(),
        Value::Int(generated.get_doubled())
    );
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn scalar_enum_and_default_literals_match_generated_rust() {
    use xrack_runtime::{
        Color, Length, MessageButtons, PointerEvent, StandardButtonKind, TouchPoint,
    };

    let source = include_str!("expressions.xr");
    let raw = xrack_compiler::mlir::lower_module(
        &xrack_compiler::parse(source).unwrap(),
        "expressions.xr",
    )
    .unwrap();
    let interpret = |module: &xrack_compiler::mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("LiteralDecodingParity")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let optimized = interpret(&raw.optimize().unwrap());
    let generated = expressions_ui::LiteralDecodingParity::new();
    for interpreted in [unoptimized, optimized] {
        for (name, actual, expected) in [
            (
                "integer",
                Value::Int(generated.get_integer()),
                Value::Int(i64::MAX),
            ),
            (
                "fraction",
                Value::Float(generated.get_fraction()),
                Value::Float(0.125),
            ),
            (
                "extent",
                Value::Length(generated.get_extent()),
                Value::Length(Length(125.0)),
            ),
            (
                "tint",
                Value::Color(generated.get_tint()),
                Value::Color(Color(0xaabb0099)),
            ),
            (
                "caption",
                Value::String(generated.get_caption()),
                Value::String("版\n\"A\"".into()),
            ),
            (
                "button",
                Value::StandardButtonKind(generated.get_button()),
                Value::StandardButtonKind(StandardButtonKind::YesToAll),
            ),
            (
                "buttons",
                Value::MessageButtons(generated.get_buttons()),
                Value::MessageButtons(MessageButtons::YesNoCancel),
            ),
            (
                "pointer",
                Value::PointerEvent(generated.get_pointer()),
                Value::PointerEvent(PointerEvent::default()),
            ),
            (
                "touch",
                Value::TouchPoint(generated.get_touch()),
                Value::TouchPoint(TouchPoint::default()),
            ),
        ] {
            assert_eq!(actual, expected, "generated {name}");
            assert_eq!(interpreted.get(name), Ok(expected), "interpreted {name}");
        }
        let Value::Float(zero) = interpreted.get("signed-zero").unwrap() else {
            panic!("expected a float");
        };
        assert_eq!(zero.to_bits(), (-0.0_f64).to_bits());
        assert_eq!(generated.get_signed_zero().to_bits(), zero.to_bits());
    }
}

#[test]
fn mixed_color_and_brush_operands_match_before_and_after_optimization() {
    use xrack_runtime::{Brush, Color, GradientStop};

    let raw = xrack_compiler::mlir::lower_module(
        &xrack_compiler::parse(include_str!("expressions.xr")).unwrap(),
        "expressions.xr",
    )
    .unwrap();
    let interpret = |module: &xrack_compiler::mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("BrushPromotionParity")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let optimized = interpret(&raw.optimize().unwrap());
    let mut generated = expressions_ui::BrushPromotionParity::new();
    let red = Brush::Solid(Color(0xff0000ff));
    let check = |first: &Brush,
                 second: &Brush,
                 equal: bool,
                 generated: &expressions_ui::BrushPromotionParity| {
        assert_eq!(&generated.get_first(), first);
        assert_eq!(&generated.get_second(), second);
        assert_eq!(
            (generated.get_left(), generated.get_right()),
            (equal, !equal)
        );
        for ui in [&unoptimized, &optimized] {
            assert_eq!(ui.get("first"), Ok(Value::Brush(first.clone())));
            assert_eq!(ui.get("second"), Ok(Value::Brush(second.clone())));
            assert_eq!(ui.get("left"), Ok(Value::Bool(equal)));
            assert_eq!(ui.get("right"), Ok(Value::Bool(!equal)));
            assert_same_scene(&ui.try_render().unwrap(), &generated.render());
        }
    };
    check(&red, &red, true, &generated);

    let gradient = Brush::Linear {
        angle_degrees: 45.0,
        stops: [
            GradientStop {
                color: Color(0xff0000ff),
                offset: 0.0,
            },
            GradientStop {
                color: Color(0x0000ffff),
                offset: 1.0,
            },
        ]
        .into(),
    };
    for ui in [&unoptimized, &optimized] {
        ui.set("paint", Value::Brush(gradient.clone())).unwrap();
    }
    generated.set_paint(gradient.clone());
    check(&red, &gradient, false, &generated);
    for ui in [&unoptimized, &optimized] {
        ui.set("use-color", false.into()).unwrap();
    }
    generated.set_use_color(false);
    check(&gradient, &red, false, &generated);
}

#[test]
fn integer_arithmetic_matches_before_and_after_optimization() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use xrack_interpreter::Error;

    let source = include_str!("expressions.xr");
    let raw = xrack_compiler::mlir::lower_module(
        &xrack_compiler::parse(source).unwrap(),
        "expressions.xr",
    )
    .unwrap();
    let interpret = |module: &xrack_compiler::mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("IntegerArithmeticParity")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let optimized = raw.optimize().unwrap();
    let instances = [unoptimized, interpret(&optimized)];
    let mut generated = expressions_ui::IntegerArithmeticParity::new();
    let check = |name, actual: Value, expected: Value| {
        assert_eq!(actual, expected, "generated {name}");
        for ui in &instances {
            assert_eq!(ui.get(name), Ok(expected.clone()), "interpreted {name}");
        }
    };

    for (name, value, expected) in [
        ("fixed-sum", generated.get_fixed_sum(), i64::MIN),
        (
            "fixed-difference",
            generated.get_fixed_difference(),
            i64::MAX,
        ),
        ("fixed-product", generated.get_fixed_product(), i64::MIN),
        ("fixed-negated", generated.get_fixed_negated(), i64::MIN),
        ("sum", generated.get_sum(), i64::MIN),
    ] {
        check(name, value.into(), expected.into());
    }
    for ui in &instances {
        ui.set("left", i64::MIN.into()).unwrap();
    }
    generated.set_left(i64::MIN);
    check(
        "difference",
        generated.get_difference().into(),
        i64::MAX.into(),
    );
    check("negated", generated.get_negated().into(), i64::MIN.into());
    for ui in &instances {
        ui.set("right", (-1_i64).into()).unwrap();
    }
    generated.set_right(-1);
    check("product", generated.get_product().into(), i64::MIN.into());

    let check_failure = |name, message: &str, call: &dyn Fn()| {
        let panic = catch_unwind(AssertUnwindSafe(call)).unwrap_err();
        let actual = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied());
        assert_eq!(actual, Some(message));
        for ui in &instances {
            assert_eq!(ui.get(name), Err(Error::Evaluation(message.into())));
        }
    };
    for denominator in [-1, 0] {
        for ui in &instances {
            ui.set("right", denominator.into()).unwrap();
        }
        generated.set_right(denominator);
        check_failure("quotient", "invalid integer division", &|| {
            generated.get_quotient();
        });
        check_failure("remainder", "invalid integer remainder", &|| {
            generated.get_remainder();
        });
    }
    for ui in &instances {
        ui.set("left", (-17_i64).into()).unwrap();
        ui.set("right", 5.into()).unwrap();
    }
    generated.set_left(-17);
    generated.set_right(5);
    check("quotient", generated.get_quotient().into(), (-3_i64).into());
    check(
        "remainder",
        generated.get_remainder().into(),
        (-2_i64).into(),
    );

    check("guarded", generated.get_guarded().into(), 42.into());
    check("short-and", generated.get_short_and().into(), false.into());
    check("short-or", generated.get_short_or().into(), true.into());
    for ui in &instances {
        ui.set("evaluate-right", true.into()).unwrap();
    }
    generated.set_evaluate_right(true);
    check_failure("guarded", "invalid integer division", &|| {
        generated.get_guarded();
    });
    check_failure("short-and", "invalid integer division", &|| {
        generated.get_short_and();
    });
    check_failure("short-or", "invalid integer remainder", &|| {
        generated.get_short_or();
    });
}

#[test]
fn verified_hexadecimal_literals_are_decoded_in_both_backends() {
    use xrack_runtime::Length;

    let source = r#"export component HexNumbers inherits Window {
        out property <float> scalar: 1.5;
        out property <length> distance: 2.5px;
        out property <float> zero: 3.5;
        out property <float> reciprocal: 1.0 / root.zero;
        out property <float> smallest: 4.5;
        out property <float> folded: 6.5 * 2.0;
        ProgressIndicator { progress: 65%; }
    }"#;
    let module =
        xrack_compiler::mlir::lower_module(&xrack_compiler::parse(source).unwrap(), "hex.xr")
            .unwrap();
    let mut text = module.to_mlir();
    for (decimal, hex) in [
        ("1.5", "0x1p-10"),
        ("2.5", "0X1.2P+3"),
        ("3.5", "-0x0p0"),
        ("4.5", "0x1p-1074"),
        ("6.5", "0x1p-10"),
        ("65", "0x1p+5"),
    ] {
        let original = format!(r#"value = "{decimal}""#);
        assert!(text.contains(&original), "missing {original}");
        text = text.replace(&original, &format!(r#"value = "{hex}""#));
    }
    let raw = xrack_compiler::mlir::Module::parse(&text).unwrap();
    let check = |module: &xrack_compiler::mlir::Module| {
        let interpreted = Program::from_module(module)
            .unwrap()
            .create("HexNumbers")
            .unwrap();
        for (name, expected) in [
            ("scalar", 1.0 / 1024.0),
            ("zero", -0.0),
            ("reciprocal", f64::NEG_INFINITY),
            ("smallest", f64::from_bits(1)),
            ("folded", 1.0 / 512.0),
        ] {
            let Value::Float(actual) = interpreted.get(name).unwrap() else {
                panic!("{name} must be a float");
            };
            assert_eq!(actual.to_bits(), expected.to_bits(), "{name}");
        }
        assert_eq!(interpreted.get("distance"), Ok(Value::Length(Length(9.0))));
        assert_eq!(
            interpreted.try_render().unwrap().children[0].number("progress"),
            Some(0.32)
        );
        let rust = xrack_compiler::codegen::generate(module)
            .unwrap()
            .to_string();
        assert!(rust.contains("0.0009765625f64"), "{rust}");
    };
    check(&raw);
    check(&raw.optimize().unwrap());
    assert!(xrack_compiler::mlir::Module::parse(&text.replace("0x1p-10", "0x1p+1024")).is_err());
}

#[test]
fn input_literals_and_record_defaults_match_generated_runtime_types() {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("InputDefaultsParity")
        .unwrap();
    let generated = expressions_ui::InputDefaultsParity::new();
    for (name, value) in [
        ("key", Value::KeyEvent(generated.get_key())),
        ("wheel", Value::WheelEvent(generated.get_wheel())),
        ("modifiers", Value::KeyModifiers(generated.get_modifiers())),
    ] {
        assert_eq!(actual.get(name), Ok(value));
    }
    let Value::Record(record) = actual.get("snapshot").unwrap() else {
        panic!("expected a record");
    };
    let expected = generated.get_snapshot();
    for (name, value) in [
        ("key", Value::KeyEvent(expected.key)),
        ("wheel", Value::WheelEvent(expected.wheel)),
        ("modifiers", Value::KeyModifiers(expected.modifiers)),
        ("pointer", Value::PointerEvent(expected.pointer)),
        ("point", Value::TouchPoint(expected.point)),
        ("buttons", Value::PointerButtons(expected.buttons)),
        ("kind", Value::PointerEventKind(expected.kind)),
        ("response", Value::EventResult(expected.response)),
        ("image", Value::ImageAsset(expected.image.into())),
        ("label", Value::StyledText(expected.label)),
    ] {
        assert_eq!(record.field(name), Some(&value), "{name}");
    }
}

#[test]
fn nested_builtin_fields_match_generated_rust_and_follow_input_updates() {
    use xrack_runtime::{
        Date, Font, KeyEvent, KeyModifiers, Length, PointerButton, PointerEvent, RecordValue, Time,
        TouchPoint, WheelEvent,
    };

    let raw = xrack_compiler::mlir::lower_module(
        &xrack_compiler::parse(include_str!("expressions.xr")).unwrap(),
        "expressions.xr",
    )
    .unwrap();
    let interpret = |module: &xrack_compiler::mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("BuiltinFieldParity")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let optimized = interpret(&raw.optimize().unwrap());
    let mut generated = expressions_ui::BuiltinFieldParity::new();
    for id in [19_i64, 42] {
        let key = KeyEvent {
            text: format!("key-{id}"),
            modifiers: KeyModifiers {
                shift: id == 19,
                ..Default::default()
            },
            ..Default::default()
        };
        let modifiers = KeyModifiers {
            meta: true,
            ..Default::default()
        };
        let wheel = WheelEvent {
            delta_x: Length(-2.5),
            lines_y: 3.5,
            ..Default::default()
        };
        let pointer = PointerEvent {
            button: PointerButton::Secondary,
            is_touch: id == 19,
            touch_finger_id: id,
            ..Default::default()
        };
        let point = TouchPoint {
            start_y: Length(12.0),
            inside: true,
            ..Default::default()
        };
        let date = Date::new(2026, 11, 13).unwrap();
        let time = Time::new(9, 45, id).unwrap();
        let font = Font {
            family: "serif".into(),
            size: Length(18.0),
            weight: 600,
            italic: true,
        };

        for ui in [&unoptimized, &optimized] {
            let Value::Record(snapshot) = ui.get("snapshot").unwrap() else {
                panic!("expected an input snapshot");
            };
            let mut fields: std::collections::BTreeMap<_, _> = snapshot
                .fields()
                .map(|(name, value)| (name.to_owned(), value.clone()))
                .collect();
            fields.extend([
                ("key".into(), Value::KeyEvent(key.clone())),
                ("modifiers".into(), Value::KeyModifiers(modifiers)),
                ("wheel".into(), Value::WheelEvent(wheel)),
                ("pointer".into(), Value::PointerEvent(pointer)),
                ("point".into(), Value::TouchPoint(point)),
            ]);
            ui.set(
                "snapshot",
                Value::Record(RecordValue::new(snapshot.name(), fields)),
            )
            .unwrap();
            ui.set("date", date.into()).unwrap();
            ui.set("time", time.into()).unwrap();
            ui.set("font", font.clone().into()).unwrap();
        }
        generated.set_snapshot(expressions_ui::InputSnapshotParity {
            key,
            modifiers,
            wheel,
            pointer,
            point,
            ..Default::default()
        });
        generated.set_date(date);
        generated.set_time(time);
        generated.set_font(font);
        for (name, actual, expected) in [
            (
                "text",
                Value::String(generated.get_text()),
                Value::String(format!("key-{id}")),
            ),
            ("chord", generated.get_chord().into(), (id == 19).into()),
            ("delta", generated.get_delta().into(), Length(-2.5).into()),
            ("lines", generated.get_lines().into(), 3.5_f64.into()),
            (
                "button",
                generated.get_button().into(),
                PointerButton::Secondary.into(),
            ),
            ("touch", generated.get_touch().into(), (id == 19).into()),
            ("contact", generated.get_contact().into(), id.into()),
            ("start", generated.get_start().into(), Length(12.0).into()),
            ("year", generated.get_year().into(), 2026_i64.into()),
            ("seconds", generated.get_seconds().into(), id.into()),
            (
                "family",
                generated.get_family().into(),
                "serif".to_owned().into(),
            ),
            ("size", generated.get_size().into(), Length(18.0).into()),
            ("italic", generated.get_italic().into(), true.into()),
        ] {
            assert_eq!(actual, expected, "generated {name}");
            for ui in [&unoptimized, &optimized] {
                assert_eq!(ui.get(name), Ok(expected.clone()), "interpreted {name}");
            }
        }
    }
}

#[test]
fn list_equality_keeps_scene_cached_and_tracks_live_models() {
    use expressions_ui::ListEqualityBindings;
    use std::rc::Rc;
    use xrack_runtime::{PointerEvent, PointerEventKind, PointerHandle};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ListEqualityBindings")
        .unwrap();
    let mut generated = ListEqualityBindings::new();
    let pointer = PointerHandle::default();
    actual
        .set("points", Value::TouchPoints(pointer.touch_points()))
        .unwrap();
    generated.bind_model_points(pointer.touch_points());

    let scene_text = |text| {
        let actual_scene = actual.try_render().unwrap();
        let generated_scene = generated.render_shared();
        assert_same_scene(&actual_scene, &generated_scene);
        assert_eq!(actual_scene.children[0].text("text"), Some(text));
        assert!(!actual.scene_dirty());
        assert!(!generated.scene_dirty());
        assert!(Rc::ptr_eq(&actual_scene, &actual.try_render().unwrap()));
        assert!(Rc::ptr_eq(&generated_scene, &generated.render_shared()));
    };
    scene_text("true/false/true");

    assert!(
        actual
            .string_model("strings")
            .unwrap()
            .set_row(0, "edited".into())
    );
    assert!(generated.model_strings().set_row(0, "edited".into()));
    scene_text("false/false/true");

    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected a record model");
    };
    let row = rows
        .model()
        .row(0)
        .unwrap()
        .with_field(&["text".into()], "edited".to_owned().into())
        .unwrap();
    assert!(rows.model().set_row(0, row));
    let mut row = generated.model_rows().row(0).unwrap();
    row.text = "edited".into();
    assert!(generated.model_rows().set_row(0, row));
    scene_text("false/true/true");

    pointer.publish_event(&PointerEvent {
        kind: PointerEventKind::Down,
        is_touch: true,
        touch_finger_id: 1,
        ..Default::default()
    });
    scene_text("false/true/false");

    let valid = rows.model().row(0).unwrap();
    let invalid = valid
        .with_field(&["text".into()], Value::Bool(false))
        .unwrap();
    assert!(rows.model().set_row(0, invalid));
    assert!(
        actual
            .try_render()
            .unwrap_err()
            .to_string()
            .contains("model CallbackSnapshotRow[0].text")
    );
    assert!(rows.model().set_row(0, valid));
    scene_text("false/true/false");
}

#[test]
fn host_bound_models_keep_row_keys_and_reset_to_the_declared_defaults() {
    use expressions_ui::{CallbackSnapshotRow, ListEqualityBindings};
    use xrack_runtime::{
        ListModel, PointerEvent, PointerEventKind, PointerHandle, RecordList, RecordValue, VecModel,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ListEqualityBindings")
        .unwrap();
    let mut generated = ListEqualityBindings::new();
    let row = |text: &str| {
        RecordValue::new(
            "CallbackSnapshotRow",
            [("text".into(), text.to_owned().into())].into(),
        )
    };
    let dynamic_rows = VecModel::from(vec![row("before")]);
    let compiled_rows = VecModel::from(vec![CallbackSnapshotRow {
        text: "before".into(),
    }]);
    let dynamic_key = dynamic_rows.row_key(0).unwrap();
    let compiled_key = compiled_rows.row_key(0).unwrap();
    actual
        .set(
            "rows",
            Value::RecordList(RecordList::from_model(
                "CallbackSnapshotRow",
                dynamic_rows.clone(),
            )),
        )
        .unwrap();
    generated.bind_model_rows(compiled_rows.clone());

    let pointer = PointerHandle::default();
    let points = pointer.touch_points();
    actual
        .set("points", Value::TouchPoints(points.clone()))
        .unwrap();
    generated.bind_model_points(points.clone());
    assert!(actual.record_model("points").is_err());
    assert!(actual.touch_points_model("rows").is_err());
    let check = || {
        let rows = actual.record_model("rows").unwrap();
        assert_eq!(rows.model(), &dynamic_rows);
        assert_eq!(rows.model().row_key(0), Some(dynamic_key));
    };
    check();
    assert_eq!(generated.model_rows(), compiled_rows);
    assert_eq!(generated.model_rows().row_key(0), Some(compiled_key));
    assert_eq!(actual.touch_points_model("points").unwrap(), points);
    assert_eq!(generated.model_points(), points);
    assert_same_scene(&actual.render(), &generated.render());

    assert!(dynamic_rows.set_row(0, row("edited")));
    assert!(compiled_rows.set_row(
        0,
        CallbackSnapshotRow {
            text: "edited".into()
        }
    ));
    pointer.publish_event(&PointerEvent {
        kind: PointerEventKind::Down,
        is_touch: true,
        touch_finger_id: 1,
        ..Default::default()
    });
    check();
    assert_eq!(generated.model_rows().row_key(0), Some(compiled_key));
    let touch_key = points.row_key(0);
    assert_eq!(
        actual.touch_points_model("points").unwrap().row_key(0),
        touch_key
    );
    assert_eq!(generated.model_points().row_key(0), touch_key);
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(actual.get("different-rows"), Ok(true.into()));
    assert_eq!(actual.get("no-points"), Ok(false.into()));

    actual.reset("rows").unwrap();
    actual.reset("points").unwrap();
    generated.reset_rows();
    generated.reset_points();
    assert_ne!(
        actual.record_model("rows").unwrap().model().row_key(0),
        Some(dynamic_key)
    );
    assert_ne!(generated.model_rows().row_key(0), Some(compiled_key));
    assert_eq!(actual.touch_points_model("points").unwrap().row_count(), 0);
    assert_eq!(generated.model_points().row_count(), 0);
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(actual.get("different-rows"), Ok(false.into()));
    assert_eq!(actual.get("no-points"), Ok(true.into()));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn callback_lists_capture_values_in_argument_order_and_detach_returned_models() {
    use expressions_ui::CallbackModelSnapshots;
    use std::rc::Rc;
    use xrack_runtime::{PointerEvent, PointerEventKind, PointerHandle, RecordList};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("CallbackModelSnapshots")
        .unwrap();
    let mut generated = CallbackModelSnapshots::new();
    let rows = |ui: &Instance, name| match ui.get(name).unwrap() {
        Value::RecordList(rows) => rows,
        _ => panic!("expected record rows"),
    };
    let actual_strings = actual.string_model("strings").unwrap();
    let actual_rows = rows(&actual, "rows");
    let generated_strings = generated.model_strings();
    let generated_rows = generated.model_rows();
    let change_row = |rows: &RecordList, text: &str| {
        let model = rows.model();
        let row = model
            .row(0)
            .unwrap()
            .with_field(&["text".into()], Value::String(text.into()))
            .unwrap();
        assert!(model.set_row(0, row));
    };
    actual
        .on("change-models", {
            let strings = actual_strings.clone();
            let rows = actual_rows.clone();
            Rc::new(move |_| {
                assert!(strings.set_row(0, "changed".into()));
                change_row(&rows, "changed");
                Ok(Some(Value::Bool(true)))
            })
        })
        .unwrap();
    generated.on_change_models({
        let strings = generated_strings.clone();
        let rows = generated_rows.clone();
        move || {
            assert!(strings.set_row(0, "changed".into()));
            let mut row = rows.row(0).unwrap();
            row.text = "changed".into();
            assert!(rows.set_row(0, row));
            true
        }
    });

    actual.invoke("run", &[]).unwrap();
    generated.invoke_run();
    assert!(generated.get_saw_original());
    assert_eq!(actual.get("saw-original"), Ok(true.into()));

    assert!(actual_strings.set_row(0, "later".into()));
    change_row(&actual_rows, "later");
    assert!(generated_strings.set_row(0, "later".into()));
    let mut row = generated_rows.row(0).unwrap();
    row.text = "later".into();
    assert!(generated_rows.set_row(0, row));
    assert_eq!(generated.get_saved_strings(), ["changed"]);
    assert_eq!(
        actual.get("saved-strings"),
        Ok(Value::StringList(generated.get_saved_strings()))
    );
    let saved = rows(&actual, "saved-rows").model().row(0).unwrap();
    let generated_saved = generated.get_saved_rows();
    assert_eq!(generated_saved[0].text, "changed");
    assert_eq!(
        saved.field("text"),
        Some(&Value::String(generated_saved[0].text.clone()))
    );

    actual
        .on(
            "compare",
            Rc::new(move |args| {
                let [
                    Value::StringList(_),
                    Value::RecordList(rows),
                    Value::Bool(_),
                ] = args
                else {
                    panic!("expected callback snapshots");
                };
                change_row(rows, "callback edit");
                Ok(Some(true.into()))
            }),
        )
        .unwrap();
    let original_row = actual_rows.model().row(0).unwrap();
    assert_eq!(
        actual.invoke(
            "compare",
            &[
                Value::StringModel(actual_strings),
                Value::RecordList(actual_rows.clone()),
                false.into(),
            ],
        ),
        Ok(Some(true.into()))
    );
    assert_eq!(actual_rows.model().row(0), Some(original_row));

    let pointer = PointerHandle::default();
    let event = PointerEvent {
        kind: PointerEventKind::Down,
        is_touch: true,
        touch_finger_id: 1,
        ..Default::default()
    };
    pointer.publish_event(&event);
    let points = pointer.touch_points();
    let expected = generated.invoke_copy_points(points.snapshot());
    let Ok(Some(Value::TouchPoints(copied))) =
        actual.invoke("copy-points", &[Value::TouchPoints(points.clone())])
    else {
        panic!("expected a touch point snapshot");
    };
    pointer.publish_event(&PointerEvent {
        kind: PointerEventKind::Up,
        ..event
    });
    assert!(points.snapshot().is_empty());
    assert_eq!(expected.len(), 1);
    assert_eq!(copied.snapshot(), expected);

    let Value::TouchPoints(empty) = actual.get("empty-points").unwrap() else {
        panic!("expected a touch point list");
    };
    assert!(generated.get_empty_points().is_empty());
    assert_eq!(empty.snapshot(), generated.get_empty_points());
    assert_eq!(
        actual.get("styled-copy"),
        Ok(Value::StyledText(generated.get_styled_copy()))
    );
}

#[test]
fn nested_records_compare_host_and_loaded_images_like_generated_rust() {
    use expressions_ui::{ComparedCard, ComparedPicture, ImageRecordEqualityParity};
    use xrack_runtime::{Image, ImageAsset, ImageFormat, LoadedImage, RecordList, RecordValue};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ImageRecordEqualityParity")
        .unwrap();
    let mut generated = ImageRecordEqualityParity::new();
    let record = |source: Value, score: f64| {
        let picture = RecordValue::new(
            "ComparedPicture",
            [("source".into(), source), ("score".into(), score.into())].into(),
        );
        RecordValue::new(
            "ComparedCard",
            [("picture".into(), Value::Record(picture))].into(),
        )
    };
    let card = |source, score| ComparedCard {
        picture: ComparedPicture { source, score },
    };
    let embedded = Image::Embedded {
        format: ImageFormat::Png,
        bytes: b"same image bytes",
        size: [2, 3],
    };
    let loaded = ImageAsset::Loaded(LoadedImage::new(
        ImageFormat::Png,
        b"same image bytes".to_vec(),
        [2, 3],
    ));
    let left = record(Value::ImageAsset(loaded.clone()), 1.0);
    let right = record(Value::Image(embedded), 1.0);
    actual.set("original", Value::Record(left.clone())).unwrap();
    actual
        .set("candidate", Value::Record(right.clone()))
        .unwrap();
    generated.set_original(card(embedded, 1.0));
    generated.set_candidate(card(embedded, 1.0));

    let candidates = RecordList::new("ComparedCard", vec![right]);
    actual
        .set(
            "originals",
            Value::RecordList(RecordList::new("ComparedCard", vec![left])),
        )
        .unwrap();
    actual
        .set("candidates", Value::RecordList(candidates.clone()))
        .unwrap();
    generated.set_originals(vec![card(embedded, 1.0)]);
    generated.set_candidates(vec![card(embedded, 1.0)]);
    for (name, generated_result, expected) in [
        ("same-source", generated.get_same_source(), true),
        ("same-record", generated.get_same_record(), true),
        ("different-record", generated.get_different_record(), false),
        ("same-list", generated.get_same_list(), true),
    ] {
        assert_eq!(generated_result, expected, "{name}");
        assert_eq!(actual.get(name), Ok(expected.into()), "{name}");
    }

    assert!(
        !candidates
            .model()
            .set_row(0, record(Value::ImageAsset(loaded), 1.0))
    );
    assert!(
        candidates
            .model()
            .set_row(0, record(Value::Image(Image::Empty), 1.0))
    );
    assert!(
        generated
            .model_candidates()
            .set_row(0, card(Image::Empty, 1.0))
    );
    assert_eq!(
        actual.get("same-list"),
        Ok(generated.get_same_list().into())
    );
    assert_eq!(actual.get("same-list"), Ok(false.into()));

    let nan = record(Value::Image(embedded), f64::NAN);
    actual.set("original", Value::Record(nan.clone())).unwrap();
    actual.set("candidate", Value::Record(nan)).unwrap();
    generated.set_original(card(embedded, f64::NAN));
    generated.set_candidate(card(embedded, f64::NAN));
    assert!(!generated.get_same_record());
    assert_eq!(actual.get("same-record"), Ok(false.into()));
}

#[test]
fn equivalent_image_writes_keep_the_same_scene_in_both_backends() {
    use std::rc::Rc;
    use xrack_runtime::Image;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("FixedImageUpdates")
        .unwrap();
    let mut generated = expressions_ui::FixedImageUpdates::new();
    let before = actual.render_shared();
    let expected_before = generated.render_shared();

    actual.set("picture", Value::Image(Image::Empty)).unwrap();
    generated.set_picture(Image::Empty);
    assert!(!generated.scene_dirty());
    assert!(!actual.scene_dirty());
    assert!(Rc::ptr_eq(&expected_before, &generated.render_shared()));
    assert!(Rc::ptr_eq(&before, &actual.render_shared()));
}

#[test]
fn host_record_inputs_require_the_generated_field_schema() {
    use std::{cell::Cell, rc::Rc};
    use xrack_interpreter::Error;
    use xrack_runtime::{RecordList, RecordValue};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ImageRecordEqualityParity")
        .unwrap();
    let mut generated = expressions_ui::ImageRecordEqualityParity::new();
    let Value::Record(valid) = actual.get("original").unwrap() else {
        panic!("expected a default card");
    };
    let bad = valid
        .with_field(
            &["picture".into(), "score".into()],
            Value::String("wrong".into()),
        )
        .unwrap();
    let require_path = |error: Error, path: &str| {
        assert!(
            matches!(&error, Error::Evaluation(message) if message.contains(path)),
            "{error}"
        );
    };

    let before = actual.render_shared();
    require_path(
        actual
            .set("original", Value::Record(bad.clone()))
            .unwrap_err(),
        "property original.picture.score",
    );
    assert!(Rc::ptr_eq(&before, &actual.render_shared()));
    assert_eq!(actual.get("original"), Ok(Value::Record(valid.clone())));
    require_path(
        actual
            .set(
                "original",
                Value::Record(RecordValue::new("ComparedCard", Default::default())),
            )
            .unwrap_err(),
        "property original.picture",
    );
    let extra = RecordValue::new(
        "ComparedCard",
        [
            ("picture".into(), valid.field("picture").unwrap().clone()),
            ("extra".into(), Value::Bool(true)),
        ]
        .into(),
    );
    require_path(
        actual.set("original", Value::Record(extra)).unwrap_err(),
        "property original.extra",
    );

    let before_list = actual.get("originals").unwrap();
    let invalid_rows = RecordList::new("ComparedCard", vec![valid.clone(), bad.clone()]);
    require_path(
        actual
            .set("originals", Value::RecordList(invalid_rows))
            .unwrap_err(),
        "property originals[1].picture.score",
    );
    assert_eq!(actual.get("originals"), Ok(before_list));
    let live = RecordList::new("ComparedCard", vec![valid.clone()]);
    actual
        .set("originals", Value::RecordList(live.clone()))
        .unwrap();
    assert_eq!(actual.get("originals"), Ok(Value::RecordList(live.clone())));
    assert!(live.model().set_row(0, bad.clone()));
    require_path(
        actual.get("originals").unwrap_err(),
        "property originals[0].picture.score",
    );
    require_path(
        actual.get("same-list").unwrap_err(),
        "model ComparedCard[0].picture.score",
    );
    let observed = Rc::new(Cell::new(0));
    actual
        .on("observe-list", {
            let observed = observed.clone();
            Rc::new(move |_| {
                observed.set(observed.get() + 1);
                Ok(None)
            })
        })
        .unwrap();
    require_path(
        actual.invoke("publish", &[]).unwrap_err(),
        "callback observe-list argument 1[0].picture.score",
    );
    assert_eq!(observed.get(), 0);
    assert!(live.model().set_row(0, valid.clone()));
    assert_eq!(actual.get("originals"), Ok(Value::RecordList(live)));
    actual.invoke("publish", &[]).unwrap();
    assert_eq!(observed.get(), 1);

    require_path(
        actual
            .invoke("receive", &[Value::Record(bad.clone())])
            .unwrap_err(),
        "callback receive argument 1.picture.score",
    );
    assert_eq!(actual.get("received"), Ok(generated.get_received().into()));
    actual
        .invoke("receive", &[Value::Record(valid.clone())])
        .unwrap();
    generated.invoke_receive(generated.get_original());
    assert_eq!(actual.get("received"), Ok(generated.get_received().into()));
    assert_eq!(generated.get_received(), 1);

    actual
        .on(
            "supply",
            Rc::new(move |_| Ok(Some(Value::Record(bad.clone())))),
        )
        .unwrap();
    require_path(
        actual.invoke("supply", &[]).unwrap_err(),
        "callback supply return value.picture.score",
    );
    let returned = valid.clone();
    actual
        .on(
            "supply",
            Rc::new(move |_| Ok(Some(Value::Record(returned.clone())))),
        )
        .unwrap();
    assert_eq!(actual.invoke("supply", &[]), Ok(Some(Value::Record(valid))));
}

#[test]
fn live_record_models_validate_before_rendering_or_sorting() {
    use std::rc::Rc;
    use xrack_runtime::{ControlAction, RecordValue};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("LiveRecordValidationParity")
        .unwrap();
    let mut generated = expressions_ui::LiveRecordValidationParity::new();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected live rows");
    };
    let model = rows.model();
    let key = model.row_key(1);
    let score = ["picture".into(), "score".into()];
    let valid = model.row(1).unwrap();
    let invalid = valid.with_field(&score, "wrong".to_owned().into()).unwrap();
    let compare = |actual: &Instance, generated: &expressions_ui::LiveRecordValidationParity| {
        assert_same_scene(&actual.try_render().unwrap(), &generated.render());
    };
    compare(&actual, &generated);

    let before = actual.try_render().unwrap();
    let changed = valid.with_field(&score, 3.0.into()).unwrap();
    assert!(model.set_row(1, changed));
    let generated_original = generated.model_rows().row(1).unwrap();
    let mut changed = generated_original.clone();
    changed.picture.score = 3.0;
    assert!(generated.model_rows().set_row(1, changed));
    assert!(Rc::ptr_eq(&before, &actual.try_render().unwrap()));
    assert!(model.set_row(1, valid.clone()));
    assert!(generated.model_rows().set_row(1, generated_original));

    let require_row = |error: xrack_interpreter::Error, row: usize| {
        let path = format!("[{row}].picture.score");
        assert!(error.to_string().contains(&path), "{error}");
    };
    let render_error = || match actual.try_render() {
        Err(error) => error,
        Ok(_) => panic!("rendered an invalid record model"),
    };
    for mode in [0, 1, 2] {
        actual.set("mode", mode.into()).unwrap();
        generated.set_mode(mode);
        compare(&actual, &generated);
        let old = actual.try_render().unwrap();
        assert!(model.set_row(1, invalid.clone()));
        require_row(render_error(), 1);
        if mode == 1 {
            let mut ui = actual.clone();
            assert!(!ui.control_at(&old, &[1], ControlAction::SortColumn(0)));
            require_row(actual.take_error().unwrap(), 1);
        }
        assert!(model.set_row(1, valid.clone()));
        assert_eq!(model.row_key(1), key);
        compare(&actual, &generated);
        if mode == 1 {
            let mut ui = actual.clone();
            assert!(ui.control_at(
                &actual.try_render().unwrap(),
                &[1],
                ControlAction::SortColumn(0)
            ));
            assert!(generated.control_at(&generated.render(), &[1], ControlAction::SortColumn(0)));
            compare(&actual, &generated);
        }
    }

    let bad = RecordValue::new("OtherRow", Default::default());
    model.push(bad);
    let error = render_error().to_string();
    assert!(
        error.contains("[2]") && error.contains("OtherRow"),
        "{error}"
    );
    assert!(model.insert(0, valid));
    let error = render_error().to_string();
    assert!(
        error.contains("[3]") && error.contains("OtherRow"),
        "{error}"
    );
    model.remove(3).unwrap();
    model.remove(0).unwrap();
    compare(&actual, &generated);
}

#[test]
fn record_model_validation_checks_only_inserted_or_edited_rows() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{RecordList, RecordRowValidator, RecordValue};

    let row = |value| RecordValue::new("Row", [("value".into(), Value::Int(value))].into());
    let list = RecordList::new("Row", (0..64).map(row).collect());
    let checks = Rc::new(Cell::new(0));
    let validator: Rc<RecordRowValidator> = Rc::new({
        let checks = checks.clone();
        move |_, _| {
            checks.set(checks.get() + 1);
            Ok(())
        }
    });
    list.validate_rows(&validator).unwrap();
    assert_eq!(checks.get(), 64);
    let model = list.model();
    assert!(model.set_row(1, row(64)));
    assert!(model.insert(2, row(65)));
    assert!(model.sort_by(|left, right| {
        let value = |row: &RecordValue| match row.field("value") {
            Some(Value::Int(value)) => *value,
            _ => unreachable!(),
        };
        value(left).cmp(&value(right))
    }));
    model.remove(0).unwrap();
    list.validate_rows(&validator).unwrap();
    list.validate_rows_untracked(&validator).unwrap();
    assert_eq!(checks.get(), 66);
}

#[test]
fn interpreted_and_generated_counter_match_across_property_and_event_updates() {
    let source = include_str!("../examples/counter/counter.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "counter.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("Counter")
        .unwrap();
    let mut generated = Counter::new();

    assert_same_counter(&interpreted, &generated);
    interpreted.set("count", Value::Int(21)).unwrap();
    generated.set_count(21);
    assert_same_counter(&interpreted, &generated);

    let scene = interpreted.try_render().unwrap();
    assert!(interpreted.dispatch_at(&scene, &[0, 2], "clicked"));
    assert!(generated.dispatch("add", "clicked"));
    assert_same_counter(&interpreted, &generated);
}

#[test]
fn named_counter_controls_produce_the_same_targets_in_both_modes() {
    use xrack_layout::{Interaction, Point, Size};

    let source = include_str!("../examples/counter/counter.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "counter.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("Counter")
        .unwrap();
    let mut compiled = Counter::new();

    fn press_add(ui: &mut impl Component) -> xrack_layout::Target {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(420.0, 220.0));
        let path = [0, 2];
        let bounds = layout.at_path(&path).unwrap().bounds;
        let point = Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        };
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.element_id(), Some("add"));
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
        target
    }

    assert_eq!(press_add(&mut interpreted), press_add(&mut compiled));
    assert_same_counter(&interpreted, &compiled);
}

fn direct_dispatch_fixture() -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create("DirectDispatchScene")
        .unwrap()
}

#[test]
fn named_dispatch_matches_static_instance_routes_and_hidden_handlers() {
    let mut actual = direct_dispatch_fixture();
    let mut generated = expressions_ui::DirectDispatchScene::new();
    for (id, event, expected) in [
        ("hidden-root-button", "clicked", true),
        ("middle/leaf/hidden-button", "clicked", true),
        ("middle/leaf/hidden-switch", "clicked", true),
        ("middle/leaf/hidden-switch", "clicked", true),
        ("middle/missing/hidden-switch", "clicked", false),
        ("middle/leaf/hidden-switch", "accepted", false),
        ("range", "changed", false),
    ] {
        assert_eq!(generated.dispatch(id, event), expected, "{id} {event}");
        assert_eq!(actual.dispatch(id, event), expected, "{id} {event}");
    }
    assert_eq!(actual.get("calls").unwrap(), Value::Int(21));
    assert_eq!(actual.get("endings").unwrap(), Value::Int(10));
    assert_eq!(actual.get("checked").unwrap(), Value::Bool(false));
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn path_dispatch_delivers_terminal_events_and_leaves_typed_handlers_untouched() {
    let mut actual = direct_dispatch_fixture();
    let mut generated = expressions_ui::DirectDispatchScene::new();
    for (path, event, expected) in [
        (&[0][..], "clicked", false),
        (&[2][..], "started", false),
        (&[2][..], "ended", true),
        (&[2][..], "cancelled", true),
        (&[3][..], "changed", false),
    ] {
        assert_eq!(
            generated.dispatch_at(&generated.render(), path, event),
            expected,
            "{path:?} {event}"
        );
        assert_eq!(
            actual.dispatch_at(&actual.render(), path, event),
            expected,
            "{path:?} {event}"
        );
    }
    assert_eq!(actual.get("endings").unwrap(), Value::Int(3));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn interpreted_grid_and_z_plans_match_compiled_and_dynamic_layouts() {
    use xrack_layout::{Interaction, Point, Size, layout};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let interpreted = program.create("GridPlans").unwrap();
    let mut compiled = expressions_ui::GridPlans::new();
    let viewport = Size::new(160.0, 200.0);
    for show_extra in [true, false] {
        interpreted
            .set("show-extra", Value::Bool(show_extra))
            .unwrap();
        compiled.set_show_extra(show_extra);
        let scene = interpreted.render();
        let expected = compiled.render();
        let plan = scene.children[0].static_grid().unwrap();
        assert_eq!(plan, expected.children[0].static_grid().unwrap());
        assert_eq!(plan.len(), 4);
        assert!(scene.children[1].static_grid().is_none());
        let frame = layout(&scene, viewport, &mut FixedText);
        assert_eq!(frame, layout(&expected, viewport, &mut FixedText));

        let mut fallback = scene.clone();
        fallback.children[0]
            .properties
            .insert("visible", Value::Bool(true));
        assert!(fallback.children[0].static_grid().is_none());
        assert_eq!(frame, layout(&fallback, viewport, &mut FixedText));
        fallback.children[0].children[0]
            .properties
            .insert("rowspan", Value::Int(1));
        assert_ne!(
            frame.at_path(&[0, 2, 1]).unwrap().bounds,
            layout(&fallback, viewport, &mut FixedText)
                .at_path(&[0, 2, 1])
                .unwrap()
                .bounds,
        );
    }

    let mut interpreted = program.create("PaintOrderPlans").unwrap();
    let mut compiled = expressions_ui::PaintOrderPlans::new();
    let initial = interpreted.render();
    assert_eq!(initial.static_paint_order(), Some(&[1, 0][..]));
    assert_eq!(initial.children[0].static_paint_order(), Some(&[1, 0][..]));
    assert!(initial.children[1].static_paint_order().is_none());

    fn click(ui: &mut impl Component, point: Point) -> Vec<usize> {
        let mut input = Interaction::default();
        let scene = ui.render();
        let frame = layout(&scene, Size::new(240.0, 100.0), &mut FixedText);
        input.pointer_move(&scene, &frame, point);
        input.pointer_press_in_scene(&scene, &frame);
        let target = input.pointer_release_target().unwrap();
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
        target.path
    }

    for (point, path, winner) in [
        (Point { x: 20.0, y: 20.0 }, [0, 0], "upper"),
        (Point { x: 140.0, y: 20.0 }, [1, 1], "fixed-peer"),
    ] {
        assert_eq!(click(&mut interpreted, point), path);
        assert_eq!(click(&mut compiled, point), path);
        assert_eq!(
            interpreted.get("winner").unwrap(),
            Value::String(winner.into())
        );
        assert_eq!(compiled.get_winner(), winner);
    }
    interpreted.set("live-z", Value::Float(3.0)).unwrap();
    compiled.set_live_z(3.0);
    let point = Point { x: 140.0, y: 20.0 };
    assert_eq!(click(&mut interpreted, point), [1, 0]);
    assert_eq!(click(&mut compiled, point), [1, 0]);
    assert_eq!(
        interpreted.get("winner").unwrap(),
        Value::String("changing".into())
    );

    let mut edited = interpreted.render();
    edited.children[0].children[0]
        .properties
        .insert("z", Value::Float(-1.0));
    assert!(edited.static_paint_order().is_none());
    assert!(edited.children[0].static_paint_order().is_none());
    let frame = layout(&edited, Size::new(240.0, 100.0), &mut FixedText);
    assert_eq!(
        frame.children[0].child_paint_order().collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn component_instances_keep_child_grid_and_paint_plans_with_layout_overrides() {
    use xrack_layout::{Size, layout};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("InstanceScenePlans")
        .unwrap();
    let mut generated = expressions_ui::InstanceScenePlans::new();
    for (wide, width) in [(false, 100.0), (true, 150.0)] {
        ui.set("wide", wide.into()).unwrap();
        generated.set_wide(wide);
        let interpreted = ui.try_render().unwrap();
        let compiled = generated.render_shared();
        assert_same_scene(&interpreted, &compiled);
        let a = interpreted.find("grid").unwrap();
        let b = compiled.find("grid").unwrap();
        for grid in [a, b] {
            assert_eq!(grid.length("width"), Some(width));
            assert!(grid.instance_geometry_handle.is_some());
            assert_eq!(
                grid.static_grid()
                    .expect("instance lost its grid plan")
                    .len(),
                2
            );
            assert_eq!(grid.static_paint_order(), Some(&[1, 0][..]));
        }
        assert_eq!(a.static_grid(), b.static_grid());
        let viewport = Size::new(200.0, 100.0);
        assert_eq!(
            layout(&interpreted, viewport, &mut FixedText),
            layout(&compiled, viewport, &mut FixedText),
        );
    }
}

#[test]
fn interpreted_static_scenes_and_gradients_share_only_immutable_storage() {
    use std::sync::Arc;
    use xrack_runtime::Brush;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let a = program.create("InternedScenes").unwrap().render();
    let b = program.create("InternedScenes").unwrap().render();
    assert_same_scene(&a, &expressions_ui::InternedScenes::new().render());
    assert!(a.children[0].shares_storage_with(&a.children[1]));
    assert!(a.children[0].shares_storage_with(&b.children[1]));
    fn stops(node: &Node) -> &Arc<[xrack_runtime::GradientStop]> {
        let Some(Value::Brush(Brush::Linear { stops, .. })) = node.properties.get("background")
        else {
            panic!("expected linear gradient");
        };
        stops
    }
    assert!(Arc::ptr_eq(stops(&a.children[2]), stops(&b.children[3])));
    let mut changed = a.children[0].clone();
    changed
        .properties
        .insert("text", Value::String("changed".into()));
    assert_eq!(b.children[1].text("text"), Some("Same"));
    assert!(!changed.shares_storage_with(&b.children[1]));
    assert_ne!(
        a.find("observed").unwrap().geometry_handle,
        b.find("observed").unwrap().geometry_handle,
    );

    let instance = program.create("ScenePlans").unwrap();
    let before = instance.render();
    instance
        .set("tint", Value::Color(xrack_runtime::Color(0x112233ff)))
        .unwrap();
    let after = instance.render();
    assert!(
        before
            .find("panel")
            .unwrap()
            .shares_storage_with(after.find("panel").unwrap())
    );
}

#[test]
fn projected_virtual_rows_use_the_model_that_produced_their_indices() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    for view in [0, 1] {
        let actual = program.create("VirtualModelSampleParity").unwrap();
        let mut compiled = expressions_ui::VirtualModelSampleParity::new();
        let actual_scene = actual.try_render().unwrap();
        let compiled_scene = compiled.render_shared();
        let actual_reads = Rc::new(RefCell::new(Vec::new()));
        let compiled_reads = Rc::new(RefCell::new(Vec::new()));
        actual
            .on("sample", {
                let owner = actual.as_weak();
                let reads = actual_reads.clone();
                Rc::new(move |args| {
                    let [Value::Int(revision), rows] = args else {
                        unreachable!()
                    };
                    reads.borrow_mut().push(*revision);
                    owner
                        .upgrade()
                        .unwrap()
                        .set("revision", (revision + 1).into())
                        .unwrap();
                    Ok(Some(rows.clone()))
                })
            })
            .unwrap();
        compiled.on_sample({
            let owner = compiled.as_weak();
            let reads = compiled_reads.clone();
            move |revision, rows| {
                reads.borrow_mut().push(revision);
                owner.upgrade().unwrap().set_revision(revision + 1);
                rows
            }
        });
        for (revision, label) in [(0, "first"), (1, "second")] {
            let interpreted = actual_scene.list_rows_at(&[view], 0..1);
            let generated = compiled_scene.list_rows_at(&[view], 0..1);
            assert_eq!(actual_reads.take(), [revision], "interpreted view {view}");
            assert_eq!(compiled_reads.take(), [revision], "generated view {view}");
            assert_eq!(interpreted[0].children[0].text("text"), Some(label));
            assert_same_scene(&interpreted[0], &generated[0]);
        }
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn ordinary_repeat_uses_one_model_sample_per_render() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RepeatModelReadParity")
        .unwrap();
    let mut generated = expressions_ui::RepeatModelReadParity::new();
    let interpreted_reads = Rc::new(RefCell::new(Vec::new()));
    let generated_reads = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("sample-count", {
            let weak = interpreted.as_weak();
            let reads = interpreted_reads.clone();
            Rc::new(move |args| {
                let [Value::Int(revision)] = args else {
                    unreachable!()
                };
                reads.borrow_mut().push(*revision);
                weak.upgrade()
                    .unwrap()
                    .set("revision", (revision + 1).into())
                    .unwrap();
                Ok(Some((revision + 2).into()))
            })
        })
        .unwrap();
    generated.on_sample_count({
        let weak = generated.as_weak();
        let reads = generated_reads.clone();
        move |revision| {
            reads.borrow_mut().push(revision);
            weak.upgrade().unwrap().set_revision(revision + 1);
            revision + 2
        }
    });
    for revision in 0..2 {
        let expected = generated.render_shared();
        let actual = interpreted.try_render().unwrap();
        assert_eq!(generated_reads.take(), [revision]);
        assert_eq!(interpreted_reads.take(), [revision]);
        assert_eq!(actual.children[0].children.len(), revision as usize + 2);
        assert_same_scene(&actual, &expected);
        assert!(interpreted.scene_dirty());
    }
}

#[test]
fn interpreted_empty_repeat_plans_keep_virtual_templates_and_event_slots() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("EmptyOrdinaryRepeats")
        .unwrap();
    let mut compiled = expressions_ui::EmptyOrdinaryRepeats::new();
    let reads = Rc::new(Cell::new(0));
    let generated_reads = Rc::new(Cell::new(0));
    interpreted
        .on("describe", {
            let reads = reads.clone();
            Rc::new(move |args| {
                let [Value::Int(index)] = args else {
                    panic!("expected row index");
                };
                reads.set(reads.get() + 1);
                Ok(Some(Value::String(format!("row {index}"))))
            })
        })
        .unwrap();
    compiled.on_describe({
        let generated_reads = generated_reads.clone();
        move |index| {
            generated_reads.set(generated_reads.get() + 1);
            format!("row {index}")
        }
    });
    let scene = interpreted.render();
    assert_same_scene(&scene, &compiled.render());
    assert!(
        scene.children[..4]
            .iter()
            .all(|child| child.kind == "Empty")
    );
    assert_eq!(scene.children[4].list.unwrap().row_count, 0);
    assert!(scene.children[5].has_natural_paint_order());
    assert_eq!((reads.get(), generated_reads.get()), (0, 0));

    interpreted.set("count", Value::Int(2)).unwrap();
    compiled.set_count(2);
    let scene = interpreted.render();
    let generated = compiled.render();
    assert_same_scene(&scene, &generated);
    assert_eq!(scene.at_path(&[3, 1]).unwrap().text("text"), Some("row 1"));
    assert_eq!((reads.get(), generated_reads.get()), (2, 2));
    assert!(interpreted.dispatch_at(&scene, &[6], "clicked"));
    assert!(compiled.dispatch_at(&generated, &[6], "clicked"));
    assert_eq!(
        interpreted.get("clicks"),
        Ok(Value::Int(compiled.get_clicks()))
    );
}

#[test]
fn planned_visual_bindings_match_generated_updates_and_keep_branches_lazy() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ScenePlans")
        .unwrap();
    let serialized = xrack_compiler::mlir::Module::parse(&preview.module.to_mlir()).unwrap();
    let unplanned = Program::from_module(&serialized)
        .unwrap()
        .create("ScenePlans")
        .unwrap();
    let unplanned_calls = Rc::new(Cell::new(0));
    unplanned
        .on("describe", {
            let calls = unplanned_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(Value::String("shared".into())))
            })
        })
        .unwrap();
    let mut expected = expressions_ui::ScenePlans::new();
    let actual_calls = Rc::new(Cell::new(0));
    let expected_calls = Rc::new(Cell::new(0));
    actual
        .on("describe", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(Value::String("shared".into())))
            })
        })
        .unwrap();
    expected.on_describe({
        let calls = expected_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            "shared".into()
        }
    });
    assert_same_scene(&actual.render(), &expected.render());
    assert_same_scene(&actual.render(), &unplanned.render());
    assert_eq!(
        (
            actual_calls.get(),
            expected_calls.get(),
            unplanned_calls.get()
        ),
        (0, 0, 0)
    );
    for show in [true, false, true] {
        actual.set("show-caption", Value::Bool(show)).unwrap();
        expected.set_show_caption(show);
        unplanned.set("show-caption", Value::Bool(show)).unwrap();
        assert_same_scene(&actual.render(), &expected.render());
        assert_same_scene(&actual.render(), &unplanned.render());
        assert_eq!(
            (
                actual_calls.get(),
                expected_calls.get(),
                unplanned_calls.get()
            ),
            (1, 1, 1)
        );
    }

    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RecordVisualCaches")
        .unwrap();
    let mut expected = expressions_ui::RecordVisualCaches::new();
    assert_same_scene(&actual.render(), &expected.render());
    let style = xrack_runtime::RecordValue::new(
        "VisualStrings",
        [
            ("prefix".into(), Value::String("new ".into())),
            ("suffix".into(), Value::String("tail".into())),
        ]
        .into(),
    );
    actual.set("style", Value::Record(style)).unwrap();
    expected.set_style(expressions_ui::VisualStrings {
        prefix: "new ".into(),
        suffix: "tail".into(),
    });
    assert_same_scene(&actual.render(), &expected.render());
}

#[test]
fn ungrouped_ssa_visuals_and_control_defaults_preserve_callback_evaluation() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::Length;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("OperandCacheProbe")
        .unwrap();
    let mut expected = expressions_ui::OperandCacheProbe::new();
    let raw = xrack_compiler::mlir::Module::parse(&preview.module.to_mlir()).unwrap();
    let raw = Program::from_module(&raw)
        .unwrap()
        .create("OperandCacheProbe")
        .unwrap();

    fn install(ui: &Instance, name: &str, prefix: &'static str, calls: Rc<Cell<usize>>) {
        ui.on(
            name,
            Rc::new(move |arguments| {
                let [Value::String(value)] = arguments else {
                    panic!("expected a string");
                };
                calls.set(calls.get() + 1);
                Ok(Some(Value::String(format!("{prefix}{value}"))))
            }),
        )
        .unwrap();
    }
    fn counted(prefix: &'static str, calls: Rc<Cell<usize>>) -> impl Fn(String) -> String {
        move |value| {
            calls.set(calls.get() + 1);
            format!("{prefix}{value}")
        }
    }

    let calls = || std::array::from_fn::<_, 3, _>(|_| Rc::new(Cell::new(0)));
    let interpreted_calls = calls();
    let generated_calls = calls();
    let raw_calls = calls();
    for (ui, calls) in [(&actual, &interpreted_calls), (&raw, &raw_calls)] {
        install(ui, "describe", "text:", calls[0].clone());
        install(ui, "field-label", "field:", calls[1].clone());
        install(ui, "extra-label", "extra:", calls[2].clone());
    }
    expected.on_describe(counted("text:", generated_calls[0].clone()));
    expected.on_field_label(counted("field:", generated_calls[1].clone()));
    expected.on_extra_label(counted("extra:", generated_calls[2].clone()));
    let observed = |counts: &[Rc<Cell<usize>>; 3]| counts.each_ref().map(|value| value.get());
    let check = |expected: &expressions_ui::OperandCacheProbe, counts: [usize; 3]| {
        let compiled = expected.render();
        assert_same_scene(&actual.render(), &compiled);
        assert_same_scene(&raw.render(), &compiled);
        for observed_calls in [&interpreted_calls, &generated_calls, &raw_calls] {
            assert_eq!(observed(observed_calls), counts);
        }
    };

    check(&expected, [1, 1, 0]);
    for ui in [&actual, &raw] {
        ui.set("panel-width", Value::Length(Length(120.0))).unwrap();
    }
    expected.set_panel_width(Length(120.0));
    check(&expected, [1, 1, 0]);

    for ui in [&actual, &raw] {
        ui.set("payload", Value::String("second".into())).unwrap();
        ui.set("show-extra", Value::Bool(true)).unwrap();
    }
    expected.set_payload("second".into());
    expected.set_show_extra(true);
    check(&expected, [2, 2, 1]);

    for ui in [&actual, &raw] {
        ui.set("show-extra", Value::Bool(false)).unwrap();
    }
    expected.set_show_extra(false);
    check(&expected, [2, 2, 1]);
    install(
        &actual,
        "extra-label",
        "changed:",
        interpreted_calls[2].clone(),
    );
    install(&raw, "extra-label", "changed:", raw_calls[2].clone());
    expected.on_extra_label(counted("changed:", generated_calls[2].clone()));
    check(&expected, [2, 2, 1]);
    for ui in [&actual, &raw] {
        ui.set("show-extra", Value::Bool(true)).unwrap();
    }
    expected.set_show_extra(true);
    check(&expected, [2, 2, 2]);
}

#[test]
fn gradient_scene_matches_generated_gallery_after_accent_change() {
    let source = include_str!("../examples/painting/gallery.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "gallery.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("Gallery")
        .unwrap();
    let mut generated = painting_ui::Gallery::new();

    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert!(interpreted.dispatch("toggle", "clicked"));
    assert!(generated.dispatch("toggle", "clicked"));
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn animation_targets_and_timing_match_generated_scene() {
    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("AnimationsDemo")
        .unwrap();
    let mut generated = layout_ui::AnimationsDemo::new();

    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert!(interpreted.dispatch("toggle", "clicked"));
    assert!(generated.dispatch("toggle", "clicked"));
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert!(interpreted.dispatch("motion", "clicked"));
    assert!(generated.dispatch("motion", "clicked"));
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn animation_metadata_and_enabled_reads_match_across_optimization() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{Animation, Easing};

    let raw = xrack_compiler::mlir::lower_module(
        &xrack_compiler::parse(include_str!("expressions.xr")).unwrap(),
        "expressions.xr",
    )
    .unwrap();
    let interpret = |module: &xrack_compiler::mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("AnimationTimingParity")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let optimized = interpret(&raw.optimize().unwrap());
    let instances = [&unoptimized, &optimized];
    let reads = [
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(RefCell::new(Vec::new())),
    ];
    for (ui, reads) in instances.into_iter().zip(&reads) {
        ui.on("motion", {
            let reads = reads.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                reads.borrow_mut().push(*tag);
                Ok(Some((*tag != 4).into()))
            })
        })
        .unwrap();
    }
    let mut generated = expressions_ui::AnimationTimingParity::new();
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    generated.on_motion({
        let reads = compiled_reads.clone();
        move |tag| {
            reads.borrow_mut().push(tag);
            tag != 4
        }
    });
    let expected = generated.render_shared();
    let node = &expected.children[0];
    assert_eq!(node.animations.len(), 1);
    assert_eq!(
        node.animations.get("y"),
        Some(&Animation::new(0, 20, Easing::LINEAR))
    );
    let timing = node.state_timings.as_ref().unwrap()["width"];
    assert_eq!(timing.enter, Some(Animation::new(40, 0, Easing::LINEAR)));
    assert_eq!(timing.exit, None);
    for ui in instances {
        assert_same_scene(&ui.try_render().unwrap(), &expected);
    }
    for reads in reads.iter().chain([&compiled_reads]) {
        assert_eq!(reads.take(), [3, 4, 1, 2]);
    }
    for ui in instances {
        assert_same_scene(&ui.try_render().unwrap(), &generated.render_shared());
    }
    for reads in reads.iter().chain([&compiled_reads]) {
        assert!(reads.take().is_empty());
    }
}

#[test]
fn styled_text_and_link_callback_match_generated_component() {
    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StyledTextDemo")
        .unwrap();
    let mut generated = layout_ui::StyledTextDemo::new();

    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&scene, &expected);
    let action = xrack_runtime::ControlAction::ActivateTextLink(0);
    assert!(interpreted.control_at(&scene, &[1], action));
    assert!(generated.control_at(&expected, &[1], action));
    assert_eq!(
        interpreted.get("last-link").unwrap(),
        Value::String(generated.get_last_link())
    );
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn styled_text_rejects_a_link_whose_destination_changed_since_render() {
    use xrack_runtime::{ControlAction::ActivateTextLink, StyledText};

    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StyledTextDemo")
        .unwrap();
    let mut generated = layout_ui::StyledTextDemo::new();
    let scene = interpreted.try_render().unwrap();
    let expected = generated.render_shared();
    let document = scene.children[1].styled_text("text").unwrap();
    let mut links = document.links().to_vec();
    links[0].href = "#replacement".into();
    let next = StyledText::from_parts(document.text(), document.runs().to_vec(), links).unwrap();
    interpreted.set("body", next.clone().into()).unwrap();
    generated.set_body(next);

    for (ui, old_scene) in [
        (&mut interpreted as &mut dyn Component, &*scene),
        (&mut generated as &mut dyn Component, &*expected),
    ] {
        assert!(!ui.control_at(old_scene, &[1], ActivateTextLink(0)));
        assert!(!ui.control_at(old_scene, &[1], ActivateTextLink(1)));
    }
    assert_eq!(interpreted.get("last-link"), Ok(String::new().into()));
    assert_eq!(generated.get_last_link(), "");

    let scene = interpreted.try_render().unwrap();
    let expected = generated.render_shared();
    assert!(interpreted.control_at(&scene, &[1], ActivateTextLink(0)));
    assert!(generated.control_at(&expected, &[1], ActivateTextLink(0)));
    assert_eq!(
        interpreted.get("last-link"),
        Ok("#replacement".to_owned().into())
    );
    assert_eq!(generated.get_last_link(), "#replacement");
    assert!(interpreted.control_at(&scene, &[1], ActivateTextLink(1)));
    assert!(generated.control_at(&expected, &[1], ActivateTextLink(1)));
    assert_eq!(
        interpreted.get("last-link"),
        Ok("#manual".to_owned().into())
    );
    assert_eq!(generated.get_last_link(), "#manual");
    assert_same_scene(
        &interpreted.try_render().unwrap(),
        &generated.render_shared(),
    );
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn linked_slider_matches_generated_compositing_demo() {
    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("CompositingDemo")
        .unwrap();
    let mut generated = layout_ui::CompositingDemo::new();

    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&scene, &expected);
    let action = xrack_runtime::RangeAction::SetFraction(0.75);
    assert!(interpreted.adjust_range_at(&scene, &[4], action));
    assert!(generated.adjust_range_at(&expected, &[4], action));
    assert_eq!(
        interpreted.get("fade").unwrap(),
        Value::Float(generated.get_fade())
    );
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn nested_repetition_matches_generated_scene_and_model_replacement() {
    use std::rc::Rc;

    let source = include_str!("expressions.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "expressions.xr", Default::default()).unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("LazyList")
        .unwrap();
    let mut generated = expressions_ui::LazyList::new();
    generated.on_row_label(|item, index, suffix| format!("{index} {item}{suffix}"));
    generated.on_row_index(|index| index.to_string());
    generated.on_detail_label(|item, index, detail| format!("{index} {item}: {detail}"));
    interpreted
        .on(
            "row-label",
            Rc::new(|values| {
                let [
                    Value::String(item),
                    Value::Int(index),
                    Value::String(suffix),
                ] = values
                else {
                    unreachable!()
                };
                Ok(Some(Value::String(format!("{index} {item}{suffix}"))))
            }),
        )
        .unwrap();
    interpreted
        .on(
            "row-index",
            Rc::new(|values| {
                let [Value::Int(index)] = values else {
                    unreachable!()
                };
                Ok(Some(Value::String(index.to_string())))
            }),
        )
        .unwrap();
    interpreted
        .on(
            "detail-label",
            Rc::new(|values| {
                let [Value::String(item), Value::Int(index), Value::Int(detail)] = values else {
                    unreachable!()
                };
                Ok(Some(Value::String(format!("{index} {item}: {detail}"))))
            }),
        )
        .unwrap();

    for items in [vec!["A", "B", "C"], vec!["D", "E", "F", "G"], vec![]] {
        let items: Vec<String> = items.into_iter().map(str::to_owned).collect();
        interpreted
            .set("items", Value::StringList(items.clone()))
            .unwrap();
        generated.set_items(items);
        assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    }
}

#[test]
fn static_repeat_snapshots_match_dynamic_rendering_and_keep_row_keys() {
    let source = include_str!("expressions.xr");
    let document = xrack_compiler::parse(source).unwrap();
    let raw = xrack_compiler::mlir::lower_module(&document, "expressions.xr").unwrap();
    let ordinary = Program::from_module(&raw)
        .unwrap()
        .create("StaticRepeatScenes")
        .unwrap();
    let raw_scene = ordinary.try_render().unwrap();
    let optimized = raw.optimize().unwrap();
    let interpreted = Program::from_module(&optimized)
        .unwrap()
        .create("StaticRepeatScenes")
        .unwrap();
    let mut generated = expressions_ui::StaticRepeatScenes::new();
    let before = interpreted.try_render().unwrap();
    let generated_before = generated.render();
    assert_same_scene(&raw_scene, &before);
    assert_same_scene(&before, &generated_before);

    for scene in [&before, &generated_before] {
        let first = &scene.children[1];
        let second = &scene.children[2];
        assert_eq!(first.children.len(), 48);
        assert!(first.shares_storage_with(second));
        assert!(first.children[0].children[0].shares_storage_with(&first.children[47].children[0]));
        for (expected, row) in raw_scene.children[1].children.iter().zip(&first.children) {
            assert!(row.row_key.is_some());
            assert_eq!(row.row_key, expected.row_key);
        }
        assert_eq!(scene.children[3].kind, "Rectangle");
        assert_eq!(scene.children[3].children[0].length("x"), Some(2.0));
        assert_eq!(scene.children[4].children[2].text("text"), Some("Row 2"));
    }

    interpreted
        .set("caption", Value::String("After".into()))
        .unwrap();
    generated.set_caption("After".into());
    let after = interpreted.try_render().unwrap();
    let generated_after = generated.render();
    assert_same_scene(&after, &generated_after);
    assert_eq!(after.children[0].text("text"), Some("After"));
    assert!(before.children[1].shares_storage_with(&after.children[1]));
    assert!(generated_before.children[1].shares_storage_with(&generated_after.children[1]));
}

#[test]
fn verified_visibility_guards_apply_in_order_to_normal_and_repeated_elements() {
    use std::{cell::RefCell, rc::Rc};

    for repeated in [false, true] {
        let repeat = if repeated {
            r#"%count = "xr.literal"() {value = "2"} : () -> !xr.int
               "xr.repeat"(%count) {name = "model", item = "row", index = ""} : (!xr.int) -> ()"#
        } else {
            ""
        };
        let text = format!(
            r#"
        module {{
          "xr.component"() ({{
            "xr.callback"() ({{
              %value = "xr.literal"() {{value = "true"}} : () -> !xr.bool
              "xr.return"(%value) {{name = "result"}} : (!xr.bool) -> ()
              "xr.end"() : () -> ()
            }}) {{name = "first", parameters = "[]", pure = "true"}} : () -> ()
            "xr.callback"() ({{
              %value = "xr.literal"() {{value = "false"}} : () -> !xr.bool
              "xr.return"(%value) {{name = "result"}} : (!xr.bool) -> ()
              "xr.end"() : () -> ()
            }}) {{name = "second", parameters = "[]", pure = "true"}} : () -> ()
            "xr.element"() ({{
              "xr.element"() ({{
                {repeat}
                %first = "xr.call"() {{owner = "root", value = "first"}} : () -> !xr.bool
                "xr.show_if"(%first) {{name = "visible"}} : (!xr.bool) -> ()
                %second = "xr.call"() {{owner = "root", value = "second"}} : () -> !xr.bool
                "xr.show_if"(%second) {{name = "visible"}} : (!xr.bool) -> ()
                "xr.end"() : () -> ()
              }}) {{id = "", key = "guarded", kind = "Rectangle"}} : () -> ()
              "xr.end"() : () -> ()
            }}) {{id = "root", key = "root", kind = "Window"}} : () -> ()
            "xr.end"() : () -> ()
          }}) {{sym_name = "GuardedScene", exported = "true"}} : () -> ()
        }}"#
        );
        let raw = xrack_compiler::mlir::Module::parse(&text).unwrap();
        let optimized = xrack_compiler::mlir::Module::parse(&text)
            .unwrap()
            .optimize()
            .unwrap();
        for module in [&raw, &optimized] {
            let rust = xrack_compiler::codegen::generate(module)
                .unwrap()
                .to_string();
            let render = rust
                .split("fn __render_scene")
                .nth(1)
                .unwrap()
                .split("fn __copy_table_path")
                .next()
                .unwrap();
            let first = render.find("__xr_invoke_cb_726f6f74_first ()").unwrap();
            let second = render.find("__xr_invoke_cb_726f6f74_second ()").unwrap();
            assert!(
                first < second && render[first..second].contains("&&"),
                "{render}"
            );
            let ui = Program::from_module(module)
                .unwrap()
                .create("GuardedScene")
                .unwrap();
            let calls = Rc::new(RefCell::new(Vec::new()));
            for (name, result) in [("first", false), ("second", true)] {
                let calls = calls.clone();
                ui.on(
                    name,
                    Rc::new(move |_| {
                        calls.borrow_mut().push(name);
                        Ok(Some(result.into()))
                    }),
                )
                .unwrap();
            }
            let scene = ui.try_render().unwrap();
            let child = &scene.children[0];
            if repeated {
                assert_eq!(child.children.len(), 2);
                assert!(child.children.iter().all(|row| !row.is_visible()));
                assert_eq!(*calls.borrow(), ["first", "first"]);
            } else {
                assert!(!child.is_visible());
                assert_eq!(*calls.borrow(), ["first"]);
            }
        }
    }
}

#[test]
fn dynamic_gradient_angle_is_sampled_before_color_stops() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{Brush, Color};

    let text = r##"
    module {
      "xr.component"() ({
        "xr.callback"() ({
          %value = "xr.literal"() {value = "90"} : () -> !xr.float
          "xr.return"(%value) {name = "result"} : (!xr.float) -> ()
          "xr.end"() : () -> ()
        }) {name = "angle", parameters = "[]", pure = "true"} : () -> ()
        "xr.callback"() ({
          %value = "xr.literal"() {value = "#112233"} : () -> !xr.color
          "xr.return"(%value) {name = "result"} : (!xr.color) -> ()
          "xr.end"() : () -> ()
        }) {name = "color", parameters = "[]", pure = "true"} : () -> ()
        "xr.element"() ({
          %angle = "xr.call"() {owner = "root", value = "angle"} : () -> !xr.float
          %color = "xr.call"() {owner = "root", value = "color"} : () -> !xr.color
          %start = "xr.literal"() {value = "0"} : () -> !xr.float
          %stop = "xr.gradient_stop"(%color, %start) {value = ""} : (!xr.color, !xr.float) -> !xr.gradientstop
          %brush = "xr.linear_gradient"(%angle, %stop) {value = ""} : (!xr.float, !xr.gradientstop) -> !xr.brush
          "xr.bind"(%brush) {name = "background"} : (!xr.brush) -> ()
          "xr.end"() : () -> ()
        }) {id = "root", key = "root", kind = "Window"} : () -> ()
        "xr.end"() : () -> ()
      }) {sym_name = "GradientOrder", exported = "true"} : () -> ()
    }"##;
    let raw = xrack_compiler::mlir::Module::parse(text).unwrap();
    let optimized = xrack_compiler::mlir::Module::parse(text)
        .unwrap()
        .optimize()
        .unwrap();
    for module in [&raw, &optimized] {
        let rust = xrack_compiler::codegen::generate(module)
            .unwrap()
            .to_string();
        let brush = rust
            .split(":: xrack_runtime :: Brush :: Linear")
            .nth(1)
            .unwrap();
        let angle = brush.find("__xr_invoke_cb_726f6f74_angle").unwrap();
        let color = brush.find("__xr_invoke_cb_726f6f74_color").unwrap();
        assert!(angle < color);
        let program = Program::from_module(module).unwrap();
        let instance = program.create("GradientOrder").unwrap();
        let original = instance.try_render().unwrap();
        let Some(Value::Brush(Brush::Linear { stops, .. })) = original.properties.get("background")
        else {
            panic!("expected linear gradient");
        };
        assert_eq!(stops[0].color, Color(0x112233ff));
        let calls = Rc::new(RefCell::new(Vec::new()));
        instance
            .on("angle", {
                let calls = calls.clone();
                Rc::new(move |_| {
                    calls.borrow_mut().push("angle");
                    Ok(Some(45.0.into()))
                })
            })
            .unwrap();
        instance
            .on("color", {
                let calls = calls.clone();
                Rc::new(move |_| {
                    calls.borrow_mut().push("color");
                    Ok(Some(Color(0x445566ff).into()))
                })
            })
            .unwrap();
        let scene = instance.try_render().unwrap();
        assert_eq!(*calls.borrow(), ["angle", "color"]);
        let Some(Value::Brush(Brush::Linear {
            angle_degrees,
            stops,
        })) = scene.properties.get("background")
        else {
            panic!("expected linear gradient");
        };
        assert_eq!(*angle_degrees, 45.0);
        assert_eq!(stops[0].color, Color(0x445566ff));
        assert_eq!(stops[0].offset, 0.0);
    }
}

#[test]
fn reactive_gradient_stops_are_shared_per_component_and_refresh_after_color_changes() {
    use std::sync::Arc;
    use xrack_runtime::{Brush, Color, GradientStop};

    fn stops(node: &Node) -> &Arc<[GradientStop]> {
        match node.properties.get("background").unwrap() {
            Value::Brush(Brush::Linear { stops, .. }) => stops,
            _ => panic!("expected a linear gradient"),
        }
    }
    fn check(scene: &Node) {
        assert!(Arc::ptr_eq(
            stops(&scene.children[0]),
            stops(&scene.children[1])
        ));
        let first = stops(&scene.children[2].children[0]);
        assert!(Arc::ptr_eq(first, stops(&scene.children[0])));
        for row in scene.children[2..]
            .iter()
            .flat_map(|repeat| &repeat.children)
        {
            assert!(Arc::ptr_eq(first, stops(row)));
        }
    }

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let interpreted = program.create("ReactiveBrushScenes").unwrap();
    let another = program.create("ReactiveBrushScenes").unwrap();
    let mut generated = expressions_ui::ReactiveBrushScenes::new();
    let first = interpreted.try_render().unwrap();
    let generated_first = generated.render();
    assert_same_scene(&first, &generated_first);
    check(&first);
    check(&generated_first);
    assert!(!Arc::ptr_eq(
        stops(&first.children[0]),
        stops(&another.try_render().unwrap().children[0])
    ));

    let color = Color(0x112233ff);
    interpreted.set("accent", Value::Color(color)).unwrap();
    generated.set_accent(color);
    let next = interpreted.try_render().unwrap();
    let generated_next = generated.render();
    assert_same_scene(&next, &generated_next);
    check(&next);
    check(&generated_next);
    assert_eq!(stops(&next.children[2].children[0])[0].color, color);
    assert!(!Arc::ptr_eq(
        stops(&first.children[2].children[0]),
        stops(&next.children[2].children[0])
    ));
}

#[test]
fn merged_state_cases_and_boolean_selects_keep_predicates_lazy_in_both_modes() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("MergedStateCases")
        .unwrap();
    let mut generated = expressions_ui::MergedStateCases::new();
    let interpreted_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    let matches = |candidate, mode| candidate == mode || (mode == 4 && candidate < 3);
    interpreted
        .on("probe", {
            let calls = interpreted_calls.clone();
            Rc::new(move |values| {
                let [Value::Int(candidate), Value::Int(mode)] = values else {
                    unreachable!()
                };
                calls.borrow_mut().push(*candidate);
                Ok(Some(Value::Bool(matches(*candidate, *mode))))
            })
        })
        .unwrap();
    generated.on_probe({
        let calls = generated_calls.clone();
        move |candidate, mode| {
            calls.borrow_mut().push(candidate);
            matches(candidate, mode)
        }
    });

    for (mode, label, predicates) in [
        (1, "Shared", &[1][..]),
        (2, "Shared", &[1, 2][..]),
        (3, "Third", &[1, 2, 3][..]),
        (4, "Shared", &[1][..]),
        (0, "Base", &[1, 2, 3][..]),
    ] {
        interpreted.set("mode", Value::Int(mode)).unwrap();
        generated.set_mode(mode);
        let actual = interpreted.try_render().unwrap();
        assert_same_scene(&actual, &generated.render());
        assert_eq!(actual.find("panel").unwrap().text("text"), Some(label));
        assert_eq!(interpreted_calls.take(), predicates);
        assert_eq!(generated_calls.take(), predicates);

        let expected = mode == 1 || mode == 4;
        assert_eq!(interpreted.get("flag").unwrap(), Value::Bool(expected));
        assert_eq!(generated.get_flag(), expected);
        let predicates: &[i64] = if mode == 1 { &[] } else { &[4] };
        assert_eq!(interpreted_calls.take(), predicates);
        assert_eq!(generated_calls.take(), predicates);
    }
}

#[test]
fn action_presentation_samples_bindings_in_the_same_order() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ActionPresentationParity")
        .unwrap();
    let mut generated = expressions_ui::ActionPresentationParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    let read = |gate: &Cell<bool>, tag| {
        if tag == 1 {
            gate.set(true);
        }
        tag != 2 || gate.get()
    };
    interpreted
        .on("read-flag", {
            let calls = actual_calls.clone();
            let gate = Cell::new(false);
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*tag);
                Ok(Some(read(&gate, *tag).into()))
            })
        })
        .unwrap();
    interpreted
        .on("read-label", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push(4);
                Ok(Some(Value::String("Run".into())))
            })
        })
        .unwrap();
    generated.on_read_flag({
        let calls = expected_calls.clone();
        let gate = Cell::new(false);
        move |tag| {
            calls.borrow_mut().push(tag);
            read(&gate, tag)
        }
    });
    generated.on_read_label({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push(4);
            "Run".into()
        }
    });

    let actual = interpreted.try_render().unwrap();
    let expected = generated.render_shared();
    assert_same_scene(&actual, &expected);
    let button = actual.find("button").unwrap();
    assert!(button.boolean("enabled", false));
    assert!(button.boolean("checkable", false));
    assert!(button.boolean("checked", false));
    assert_eq!(button.text("text"), Some("Run"));
    assert_eq!(expected_calls.take(), [1, 2, 3, 4]);
    assert_eq!(actual_calls.take(), [1, 2, 3, 4]);

    assert_same_scene(
        &interpreted.try_render().unwrap(),
        &generated.render_shared(),
    );
    assert!(actual_calls.take().is_empty());
    assert!(expected_calls.take().is_empty());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn disabled_actions_skip_stateful_button_enabled_bindings() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ActionStateParity")
        .unwrap();
    let mut generated = expressions_ui::ActionStateParity::new();
    let interpreted_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("allow-button", {
            let calls = interpreted_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*tag);
                Ok(Some((*tag == 1).into()))
            })
        })
        .unwrap();
    generated.on_allow_button({
        let calls = generated_calls.clone();
        move |tag| {
            calls.borrow_mut().push(tag);
            tag == 1
        }
    });

    for (command, alternate, enabled, expected_calls) in [
        (false, true, false, &[][..]),
        (false, false, false, &[][..]),
        (true, false, false, &[2][..]),
        (true, true, true, &[1][..]),
        (false, true, false, &[][..]),
    ] {
        interpreted.set("command-enabled", command.into()).unwrap();
        generated.set_command_enabled(command);
        interpreted.set("alternate", alternate.into()).unwrap();
        generated.set_alternate(alternate);

        let scene = interpreted.try_render().unwrap();
        let generated_scene = generated.render();
        assert_same_scene(&scene, &generated_scene);
        assert_eq!(
            scene.find("button").unwrap().boolean("enabled", true),
            enabled
        );
        assert_eq!(generated_calls.take(), expected_calls);
        assert_eq!(interpreted_calls.take(), expected_calls);
        assert_eq!(interpreted.dispatch_at(&scene, &[0], "clicked"), enabled);
        assert_eq!(
            generated.dispatch_at(&generated_scene, &[0], "clicked"),
            enabled
        );
    }
    assert_eq!(generated.get_activations(), 1);
    assert_eq!(interpreted.get("activations"), Ok(1_i64.into()));

    interpreted.set("command-enabled", true.into()).unwrap();
    generated.set_command_enabled(true);
    let stale = interpreted.render_shared();
    let compiled_stale = generated.render_shared();
    interpreted_calls.take();
    generated_calls.take();
    interpreted.set("alternate", false.into()).unwrap();
    generated.set_alternate(false);
    assert!(!interpreted.dispatch_at(&stale, &[0], "clicked"));
    assert!(!generated.dispatch_at(&compiled_stale, &[0], "clicked"));
    assert!(interpreted_calls.take().is_empty());
    assert!(generated_calls.take().is_empty());
    assert_same_scene(&interpreted.render_shared(), &generated.render_shared());
    assert!(interpreted_calls.take().is_empty());
    assert!(generated_calls.take().is_empty());
    assert_eq!(generated.get_activations(), 1);
    assert_eq!(interpreted.get("activations"), Ok(1_i64.into()));

    interpreted.set("alternate", true.into()).unwrap();
    generated.set_alternate(true);
    assert!(interpreted.dispatch_at(&stale, &[0], "clicked"));
    assert!(generated.dispatch_at(&compiled_stale, &[0], "clicked"));
    assert_eq!(interpreted_calls.take(), [1]);
    assert_eq!(generated_calls.take(), [1]);
    assert_same_scene(&interpreted.render_shared(), &generated.render_shared());
    assert!(interpreted_calls.take().is_empty());
    assert!(generated_calls.take().is_empty());
    assert_eq!(generated.get_activations(), 2);
    assert_eq!(interpreted.get("activations"), Ok(2_i64.into()));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn visual_input_rechecks_nonpersistent_state_predicates() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("InputStatePredicateParity")
        .unwrap();
    let mut generated = expressions_ui::InputStatePredicateParity::new();
    let locked = Rc::new(Cell::new(false));
    actual
        .on("locked", {
            let locked = locked.clone();
            Rc::new(move |_| Ok(Some(locked.get().into())))
        })
        .unwrap();
    generated.on_locked({
        let locked = locked.clone();
        move || locked.get()
    });
    let actual_scene = actual.render_shared();
    let generated_scene = generated.render_shared();

    for value in [true, false, true] {
        locked.set(value);
        assert_eq!(
            generated.dispatch_at(&generated_scene, &[0], "clicked"),
            !value
        );
        assert_eq!(actual.dispatch_at(&actual_scene, &[0], "clicked"), !value);
    }
    assert_eq!(actual.get("clicks"), Ok(1_i64.into()));
    assert_eq!(generated.get_clicks(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn visual_state_predicates_run_at_the_declaration_on_each_scene_rebuild() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StateOrderParity")
        .unwrap();
    let mut generated = expressions_ui::StateOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("condition", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(mode)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push("condition");
                Ok(Some((*mode == 1).into()))
            })
        })
        .unwrap();
    interpreted
        .on("label", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                calls.borrow_mut().push("label");
                Ok(Some(args[0].clone()))
            })
        })
        .unwrap();
    generated.on_condition({
        let calls = expected_calls.clone();
        move |mode| {
            calls.borrow_mut().push("condition");
            mode == 1
        }
    });
    generated.on_label({
        let calls = expected_calls.clone();
        move |caption| {
            calls.borrow_mut().push("label");
            caption
        }
    });

    for caption in ["Before", "After"] {
        interpreted
            .set("caption", Value::String(caption.into()))
            .unwrap();
        generated.set_caption(caption.into());
        let actual = interpreted.try_render().unwrap();
        let expected = generated.render();
        assert_same_scene(&actual, &expected);
        assert_eq!(
            actual.find("target").unwrap().text("text"),
            Some("Selected")
        );
        assert_eq!(expected_calls.take(), ["condition", "label"]);
        assert_eq!(actual_calls.take(), ["condition", "label"]);
    }
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert!(expected_calls.take().is_empty());
    assert!(actual_calls.take().is_empty());

    // A binding can invalidate the predicate while the same scene is being built.
    interpreted
        .on("label", {
            let owner = Cell::new(Some(interpreted.clone()));
            Rc::new(move |args| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("mode", 0_i64.into())
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(args[0].clone()))
            })
        })
        .unwrap();
    generated.on_label({
        let owner = generated.as_weak();
        move |caption| {
            owner.upgrade().unwrap().set_mode(0);
            caption
        }
    });
    let actual = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&actual, &expected);
    assert_eq!(
        actual.find("target").unwrap().text("text"),
        Some("Selected")
    );
    assert_eq!(interpreted.get("mode"), Ok(0_i64.into()));
    assert_eq!(generated.get_mode(), 0);

    let actual = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&actual, &expected);
    assert_eq!(actual.find("target").unwrap().text("text"), Some("Base"));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn repeated_state_selectors_match_compiler_independently_for_each_index() {
    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StatesDemo")
        .unwrap();
    let mut generated = layout_ui::StatesDemo::new();

    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    for button in ["warning", "critical", "normal"] {
        assert!(interpreted.dispatch(button, "clicked"));
        assert!(generated.dispatch(button, "clicked"));
        assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    }
}

#[test]
fn forwarded_alias_and_state_input_match_generated_component() {
    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ComponentStatesDemo")
        .unwrap();
    let mut generated = layout_ui::ComponentStatesDemo::new();

    let check = |interpreted: &Instance, generated: &layout_ui::ComponentStatesDemo| {
        assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
        assert_eq!(
            interpreted.get("effective-caption").unwrap(),
            Value::String(generated.get_effective_caption()),
        );
    };
    check(&interpreted, &generated);
    assert!(interpreted.dispatch("toggle", "clicked"));
    assert!(generated.dispatch("toggle", "clicked"));
    check(&interpreted, &generated);

    let text = "manually set".to_owned();
    interpreted
        .invoke("set-banner", &[Value::String(text.clone())])
        .unwrap();
    generated.invoke_set_banner(text);
    check(&interpreted, &generated);
    assert!(interpreted.dispatch("toggle", "clicked"));
    assert!(generated.dispatch("toggle", "clicked"));
    check(&interpreted, &generated);
}

#[test]
fn record_grid_rows_and_selection_match_compiler() {
    let source = include_str!("../examples/list/list.xr");
    let preview = xrack_compiler::prepare_preview(source, "list.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("GridDemo")
        .unwrap();
    let mut generated = list_ui::GridDemo::new();
    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&scene, &expected);
    let path = &[0, 1];
    let actual_rows = interpreted.list_rows(path, 0..2);
    let expected_rows = generated.list_rows(path, 0..2);
    assert_eq!(actual_rows.len(), expected_rows.len());
    for (actual, expected) in actual_rows.iter().zip(&expected_rows) {
        assert_same_scene(actual, expected);
    }
    let action = xrack_runtime::ControlAction::Select(1);
    assert!(interpreted.control_at(&scene, path, action));
    assert!(generated.control_at(&expected, path, action));
    assert_eq!(
        interpreted.get("selections").unwrap(),
        Value::Int(generated.get_selections())
    );
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn combo_text_accepts_the_first_string_match_and_keeps_the_submitted_draft() {
    use xrack_runtime::{ControlAction, text_input_value};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ComboTextParity")
        .unwrap();
    let mut compiled = expressions_ui::ComboTextParity::new();
    let path = &[0];

    for ui in [&mut interpreted as &mut dyn Component, &mut compiled] {
        assert!(ui.control_at(&ui.render(), path, ControlAction::Select(1)));
        assert_eq!(
            ui.render().at_path(path).unwrap().int("current-index"),
            Some(1)
        );
        assert!(ui.control_at(&ui.render(), path, ControlAction::AcceptComboText));
    }
    assert_eq!(compiled.get_selection(), 0);
    assert_eq!(interpreted.get("selection"), Ok(0_i64.into()));
    assert_eq!(compiled.get_accepted(), "A");
    assert_eq!(interpreted.get("accepted"), Ok(Value::String("A".into())));

    for ui in [&mut interpreted as &mut dyn Component, &mut compiled] {
        assert!(ui.edit_at(&ui.render(), path, "submitted".into()));
        let snapshot = ui.render();
        assert!(ui.edit_at(&snapshot, path, "new draft".into()));
        assert!(ui.control_at(&snapshot, path, ControlAction::AcceptComboText));
        assert_eq!(
            text_input_value(ui.render().at_path(path).unwrap()),
            "submitted"
        );
    }
    assert_eq!(compiled.get_selection(), -1);
    assert_eq!(interpreted.get("selection"), Ok((-1_i64).into()));
    assert_eq!(compiled.get_accepted(), "submitted");
    assert_eq!(
        interpreted.get("accepted"),
        Ok(Value::String("submitted".into()))
    );
    assert_eq!(compiled.get_selections(), 1);
    assert_eq!(interpreted.get("selections"), Ok(1_i64.into()));
    assert_same_scene(&interpreted.render(), &compiled.render());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn combo_callbacks_observe_selection_and_explicit_edit_text_in_order() {
    use xrack_runtime::ControlAction::{AcceptComboText, Select};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ComboCommitParity")
        .unwrap();
    let mut generated = expressions_ui::ComboCommitParity::new();
    let path = [0];
    for ui in [&mut actual as &mut dyn Component, &mut generated] {
        assert!(ui.control_at(&ui.render_shared(), &path, Select(1)));
        assert!(ui.edit_at(&ui.render_shared(), &path, "A".into()));
        assert!(ui.control_at(&ui.render_shared(), &path, AcceptComboText));
        assert!(ui.edit_at(&ui.render_shared(), &path, "free".into()));
        assert!(ui.control_at(&ui.render_shared(), &path, AcceptComboText));
    }
    let events = "select:B:2:B;value:2:2:B;value:1:1:A;accept:A:1:A;accept:free:1:free;";
    assert_eq!(actual.get("events"), Ok(events.to_owned().into()));
    assert_eq!(generated.get_events(), events);
    assert_eq!(actual.get("current"), Ok(1_i64.into()));
    assert_eq!(generated.get_current(), 1);
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn combo_actions_check_live_availability_and_editability() {
    use expressions_ui::ComboTextParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction::{AcceptComboText, Select};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ComboTextParity")
        .unwrap();
    let mut expected = ComboTextParity::new();
    let path = [0];
    assert!(actual.edit_at(&actual.render_shared(), &path, "B".into()));
    assert!(expected.edit_at(&expected.render_shared(), &path, "B".into()));
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();

    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    for action in [Select(2), AcceptComboText] {
        assert!(!actual.control_at(&actual_scene, &path, action));
        assert!(!expected.control_at(&expected_scene, &path, action));
    }
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    actual.set("can-edit", false.into()).unwrap();
    expected.set_can_edit(false);
    assert!(!actual.control_at(&actual_scene, &path, AcceptComboText));
    assert!(!expected.control_at(&expected_scene, &path, AcceptComboText));

    actual
        .on("allow-edit", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("allowed", false.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    expected.on_allow_edit({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_allowed(false);
            true
        }
    });
    assert!(!actual.control_at(&actual_scene, &path, AcceptComboText));
    assert!(!expected.control_at(&expected_scene, &path, AcceptComboText));
    assert_eq!(actual.get("allowed"), Ok(false.into()));
    assert!(!expected.get_allowed());
    assert_eq!(actual.get("selection"), Ok(0_i64.into()));
    assert_eq!(expected.get_selection(), 0);
    assert_eq!(actual.get("selections"), Ok(0_i64.into()));
    assert_eq!(expected.get_selections(), 0);
    assert_eq!(actual.get("accepted"), Ok(Value::String(String::new())));
    assert!(expected.get_accepted().is_empty());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn string_combo_rejects_input_from_a_changed_model_snapshot() {
    use expressions_ui::ComboTextParity;
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ComboTextParity")
        .unwrap();
    let mut generated = ComboTextParity::new();
    let path = [0];
    assert!(actual.edit_at(&actual.render_shared(), &path, "B".into()));
    assert!(generated.edit_at(&generated.render_shared(), &path, "B".into()));
    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    assert!(
        actual
            .string_model("choices")
            .unwrap()
            .set_row(2, "C".into())
    );
    assert!(generated.model_choices().set_row(2, "C".into()));
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*stale),
        (&mut generated, &*compiled_stale),
    ] {
        assert!(!ui.control_at(scene, &path, ControlAction::Select(2)));
        assert!(!ui.control_at(scene, &path, ControlAction::AcceptComboText));
        let current = ui.render_shared();
        assert!(ui.control_at(&current, &path, ControlAction::Select(2)));
        assert!(ui.control_at(&ui.render_shared(), &path, ControlAction::AcceptComboText));
    }
    assert_eq!(actual.get("selection"), Ok(2_i64.into()));
    assert_eq!(generated.get_selection(), 2);
    assert_eq!(actual.get("selections"), Ok(1_i64.into()));
    assert_eq!(generated.get_selections(), 1);
    assert_eq!(actual.get("accepted"), Ok(Value::String("C".into())));
    assert_eq!(generated.get_accepted(), "C");

    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    let choices = generated.get_choices();
    actual
        .set("choices", Value::StringList(choices.clone()))
        .unwrap();
    generated.set_choices(choices);
    assert!(!actual.control_at(&stale, &path, ControlAction::Select(1)));
    assert!(!generated.control_at(&compiled_stale, &path, ControlAction::Select(1)));
    assert_same_scene(&actual.render_shared(), &generated.render_shared());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn record_choice_matches_compiler_for_value_roles_and_disabled_rows() {
    use xrack_runtime::{
        ControlAction, choice_enabled, combo_count, combo_index, combo_label, combo_value,
    };

    let source = include_str!("../examples/selection/selection.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "selection.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RecordComboDemo")
        .unwrap();
    let mut generated = selection_ui::RecordComboDemo::new();
    let path = &[1];
    let check = |scene: &Node, expected: &Node| {
        let actual = scene.at_path(path).unwrap();
        let expected = expected.at_path(path).unwrap();
        assert_eq!(combo_count(actual), combo_count(expected));
        assert_eq!(combo_index(actual), combo_index(expected));
        for index in 0..combo_count(expected) {
            assert_eq!(combo_label(actual, index), combo_label(expected, index));
            assert_eq!(combo_value(actual, index), combo_value(expected, index));
            assert_eq!(
                choice_enabled(actual, index),
                choice_enabled(expected, index)
            );
        }
    };
    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    check(&scene, &expected);
    assert!(!interpreted.control_at(&scene, path, ControlAction::Select(3)));
    assert!(!generated.control_at(&expected, path, ControlAction::Select(3)));
    assert!(interpreted.control_at(&scene, path, ControlAction::Select(1)));
    assert!(generated.control_at(&expected, path, ControlAction::Select(1)));
    assert_eq!(
        interpreted.get("code").unwrap(),
        Value::Int(generated.get_code())
    );
    assert_eq!(
        interpreted.get("selected-code").unwrap(),
        Value::Int(generated.get_selected_code())
    );
    check(&interpreted.try_render().unwrap(), &generated.render());

    interpreted.set("editable", Value::Bool(true)).unwrap();
    generated.set_editable(true);
    for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
        assert!(ui.control_at(&ui.render(), path, ControlAction::AcceptComboText));
        assert!(ui.edit_at(&ui.render(), path, "Build archive".into()));
        assert!(!ui.control_at(&ui.render(), path, ControlAction::AcceptComboText));
    }
    assert_eq!(generated.get_code(), 100);
    assert_eq!(interpreted.get("code"), Ok(100_i64.into()));
    assert_eq!(generated.get_accepted_text(), "共享工作区");
    assert_eq!(
        interpreted.get("accepted-text"),
        Ok(Value::String("共享工作区".into()))
    );

    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert!(interpreted.edit_at(&scene, path, "Build".into()));
    assert!(generated.edit_at(&expected, path, "Build".into()));
    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert!(interpreted.control_at(&scene, path, ControlAction::AcceptComboText));
    assert!(generated.control_at(&expected, path, ControlAction::AcceptComboText));
    assert_eq!(
        interpreted.get("code").unwrap(),
        Value::Int(generated.get_code())
    );
    assert_eq!(
        interpreted.get("accepted-text").unwrap(),
        Value::String(generated.get_accepted_text())
    );
    check(&interpreted.try_render().unwrap(), &generated.render());
}

#[test]
fn standard_list_matches_compiler_for_string_and_record_models() {
    let source = include_str!("../examples/list/list.xr");
    let preview = xrack_compiler::prepare_preview(source, "list.xr", Default::default()).unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StandardListDemo")
        .unwrap();
    let mut generated = list_ui::StandardListDemo::new();
    let scene = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&scene, &expected);
    for path in [&[1][..], &[2][..]] {
        let actual = interpreted.list_rows(path, 0..3);
        let expected = generated.list_rows(path, 0..3);
        assert_eq!(actual.len(), 3);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_same_scene(actual, expected);
        }
    }
    let action = xrack_runtime::ControlAction::Select(2);
    assert!(interpreted.control_at(&scene, &[2], action));
    assert!(generated.control_at(&expected, &[2], action));
    assert_eq!(
        interpreted.get("selections").unwrap(),
        Value::Int(generated.get_selections())
    );
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
}

fn standalone_fixture(path: &str, name: &str, resources: &[&str]) -> std::path::PathBuf {
    use std::{fs, path::Path};

    let directory = std::env::var_os("TEST_TMPDIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(format!("xrack-mlir-{}-{name}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let base = Path::new(path).parent().unwrap();
    for resource in resources {
        let copy = directory.join(resource);
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        fs::copy(base.join(resource), copy).unwrap();
    }
    let source = fs::read_to_string(path).unwrap();
    let module = xrack_compiler::prepare_preview(&source, path, Default::default())
        .unwrap()
        .module;
    let file = directory.join("standalone.mlir");
    fs::write(&file, module.to_mlir()).unwrap();
    file
}

#[test]
fn standalone_mlir_loads_fonts_and_reactive_translations_like_generated_ui() {
    use xrack_runtime::LocaleContext;

    let font_file = standalone_fixture("examples/layout/bundled.xr", "fonts", &["fonts/demo.ttc"]);
    let interpreted = xrack_interpreter::load_mlir_file(&font_file)
        .unwrap()
        .create("BundledFontsDemo")
        .unwrap()
        .try_render()
        .unwrap();
    let generated = bundled_ui::BundledFontsDemo::new().render();
    assert_same_scene(&interpreted, &generated);
    let loaded = interpreted.loaded_fonts.as_ref().unwrap().sources();
    let embedded = generated.fonts.unwrap().sources();
    assert_eq!(loaded.len(), 1);
    assert!(
        loaded
            .iter()
            .map(|font| font.as_slice())
            .eq(embedded.iter().copied())
    );

    let font_ir = std::fs::read_to_string(&font_file).unwrap();
    for id in ["", "main-window"] {
        let edited = font_ir.replace("id = \"root\"", &format!("id = \"{id}\""));
        assert_ne!(edited, font_ir);
        let module = xrack_compiler::mlir::process(&edited).unwrap();
        let (program, _) =
            Program::from_module_with_file_resources(&module, &font_file, None).unwrap();
        let scene = program.create("BundledFontsDemo").unwrap().render_shared();
        assert_eq!(scene.element_id(), (!id.is_empty()).then_some(id));
        assert_eq!(scene.loaded_fonts.as_ref().unwrap().sources().len(), 1);

        let filename = font_file.to_str().unwrap();
        let rust = xrack_compiler::generate_rust(&module, filename, None)
            .unwrap()
            .to_string();
        assert!(
            rust.contains("set_embedded_fonts (& __XR_FONTS)"),
            "missing font attachment for root ID {id:?}"
        );
    }

    let locale_file = standalone_fixture(
        "examples/tray/tray.xr",
        "locale",
        &[
            "assets/status.svg",
            "i18n/catalog.toml",
            "i18n/en-US.ftl",
            "i18n/zh-CN.ftl",
        ],
    );
    let program = xrack_interpreter::load_mlir_file(&locale_file).unwrap();
    let locale = LocaleContext::default();
    locale.set_locale("en-US").unwrap();
    let mut interpreted = program
        .create_with_locale("TrayWindow", locale.clone())
        .unwrap();
    let mut generated = tray_ui::TrayWindow::new_with_locale(locale.clone());
    let tray = program
        .create_with_locale("TrayDemo", locale.clone())
        .unwrap();
    let generated_tray = tray_ui::TrayDemo::new_with_locale(locale.clone());
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert_same_scene(&tray.try_render().unwrap(), &generated_tray.render());
    assert!(interpreted.dispatch("add", "clicked"));
    assert!(generated.dispatch("add", "clicked"));
    locale.set_locale("zh-CN").unwrap();
    let scene = interpreted.try_render().unwrap();
    assert_eq!(scene.text("title"), Some("XRack · 系统托盘"));
    assert_eq!(
        scene.find("hint").unwrap().text("text"),
        Some("The tray icon stays open when this window closes.")
    );
    assert_eq!(interpreted.get("count"), Ok(Value::Int(1)));
    assert_same_scene(&scene, &generated.render());
    assert_same_scene(&tray.try_render().unwrap(), &generated_tray.render());

    let invalid = r#"
        translations "i18n/catalog.toml";
        export component MissingTranslation inherits Window {
            title: true ? "Fixed" : @tr("missing-from-default");
        }
    "#;
    let path = locale_file.with_file_name("invalid.mlir");
    let file = path.to_str().unwrap();
    let document = xrack_compiler::parse(invalid).unwrap();
    let module = xrack_compiler::mlir::lower_module(&document, file).unwrap();
    let raw = module.to_mlir();
    std::fs::write(&path, &raw).unwrap();
    let compiler = xrack_compiler::prepare_mlir_preview(&raw, file)
        .err()
        .unwrap();
    let interpreter = xrack_interpreter::load_mlir_file(path).err().unwrap();
    let xrack_compiler::CompilerError::Resources(message) = compiler else {
        panic!("expected a translation resource error");
    };
    assert_eq!(interpreter.to_string(), message);
    assert!(interpreter.to_string().contains("missing-from-default"));

    // The IR is relocatable with its resources. The original source tree
    // is still visible here, so removal proves there is no source-path fallback.
    std::fs::remove_file(locale_file.with_file_name("assets").join("status.svg")).unwrap();
    let error = xrack_interpreter::load_mlir_file(&locale_file)
        .err()
        .unwrap();
    assert!(error.to_string().contains("assets/status.svg"), "{error}");
}

fn standalone_media_module(name: &str) -> std::path::PathBuf {
    standalone_fixture(
        "examples/media/media.xr",
        name,
        &[
            "assets/color-mask.png",
            "assets/landscape.png",
            "assets/nine-slice.svg",
            "assets/orbit.svg",
            "assets/pulse.gif",
            "assets/sprite-atlas.png",
        ],
    )
}

#[test]
fn host_supplied_nine_slice_assets_are_validated_before_instantiation() {
    use std::collections::BTreeMap;
    use xrack_runtime::{ImageAsset, ImageFormat, LoadedAnimation, LoadedImage, NineSlice};

    let source = r#"export component Frame inherits Window {
        Image { source: @image-url("frame.svg", nine-slice(4)); }
    }"#;
    let document = xrack_compiler::parse(source).unwrap();
    let module = xrack_compiler::lower(&document, "host-frame.xr").unwrap();
    let load = |asset: Option<ImageAsset>| {
        Program::from_module_with_assets(
            &module,
            None,
            None,
            asset
                .into_iter()
                .map(|image| ("frame.svg".into(), image))
                .collect(),
        )
    };
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#;
    let image = ImageAsset::Loaded(LoadedImage::new(ImageFormat::Svg, svg.to_vec(), [16, 16]));
    let program = load(Some(image)).unwrap();
    let scene = program.create("Frame").unwrap().try_render().unwrap();
    let image = scene.children[0].image_asset("source").unwrap();
    assert_eq!(image.size(), [16, 16]);
    assert_eq!(
        image.pixels().unwrap().edges,
        Some(NineSlice {
            top: 4,
            right: 4,
            bottom: 4,
            left: 4
        })
    );

    let missing =
        xrack_compiler::codegen::generate_with_images(&module, &BTreeMap::new()).unwrap_err();
    assert_eq!(load(None).err().unwrap().to_string(), missing);
    for (asset, diagnostic) in [
        (
            ImageAsset::Loaded(LoadedImage::new(ImageFormat::Svg, svg.to_vec(), [8, 16])),
            "must leave a nonempty center",
        ),
        (
            ImageAsset::Animated(std::rc::Rc::new(LoadedAnimation::new(
                [16, 16],
                vec![(100, vec![])],
                0,
            ))),
            "requires a static PNG or SVG source",
        ),
    ] {
        let error = load(Some(asset))
            .err()
            .expect("asset validation should fail at load time");
        assert!(error.to_string().contains(diagnostic), "{error}");
    }
}

#[test]
fn animated_image_equality_matches_across_file_and_host_representations() {
    use expressions_ui::{ComparedCard, ComparedPicture, ImageRecordEqualityParity};
    use xrack_runtime::{Image, ImageAsset, RecordValue};

    let file = standalone_media_module("animated-image-equality");
    let load_animation = || {
        xrack_interpreter::load_mlir_file(&file)
            .unwrap()
            .create("AnimatedImageDemo")
            .unwrap()
            .get("animation")
            .unwrap()
    };
    let loaded = load_animation();
    assert!(matches!(loaded, Value::ImageAsset(ImageAsset::Animated(_))));
    let embedded = media_ui::AnimatedImageDemo::new().get_animation();
    assert!(matches!(embedded, Image::Animated(_)));

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ImageRecordEqualityParity")
        .unwrap();
    let mut generated = ImageRecordEqualityParity::new();
    let dynamic_card = |image| {
        let picture = RecordValue::new(
            "ComparedPicture",
            [("source".into(), image), ("score".into(), 1.0.into())].into(),
        );
        Value::Record(RecordValue::new(
            "ComparedCard",
            [("picture".into(), Value::Record(picture))].into(),
        ))
    };
    let compiled_card = |source| ComparedCard {
        picture: ComparedPicture { source, score: 1.0 },
    };
    interpreted.set("original", dynamic_card(loaded)).unwrap();
    generated.set_original(compiled_card(embedded));
    for (dynamic, compiled, equal) in [
        (Value::Image(embedded), embedded, true),
        (Value::Image(Image::Empty), Image::Empty, false),
        (load_animation(), embedded, true),
    ] {
        interpreted.set("candidate", dynamic_card(dynamic)).unwrap();
        generated.set_candidate(compiled_card(compiled));
        for (name, result, expected) in [
            ("same-source", generated.get_same_source(), equal),
            ("same-record", generated.get_same_record(), equal),
            ("different-record", generated.get_different_record(), !equal),
        ] {
            assert_eq!(result, expected, "generated {name}");
            assert_eq!(interpreted.get(name), Ok(result.into()), "{name}");
        }
    }
}

#[test]
fn animated_image_state_source_controls_playback_and_completion() {
    use std::time::{Duration, Instant};
    use xrack_runtime::ImagePlaybackQueue;

    let file = standalone_media_module("animated-image-state");
    let media = xrack_interpreter::load_mlir_file(&file).unwrap();
    let animation = media
        .create("AnimatedImageDemo")
        .unwrap()
        .get("animation")
        .unwrap();
    let generated_animation = media_ui::AnimatedImageDemo::new().get_animation();
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("AnimatedImageStateParity")
        .unwrap();
    let mut generated = expressions_ui::AnimatedImageStateParity::new();
    actual.set("animation", animation).unwrap();
    generated.set_animation(generated_animation);
    let end = Instant::now() + Duration::from_secs(1);
    for ui in [&actual as &dyn Component, &generated] {
        let scene = ui.render_shared();
        for id in ["empty-handler", "no-handler"] {
            let mut queue = ImagePlaybackQueue::default();
            let playback = scene.find(id).unwrap().image_playback.as_ref().unwrap();
            queue.sync(end - Duration::from_secs(1), [(playback, true)]);
            assert!(queue.advance(end));
            assert!(!queue.next_finished().unwrap().finish(), "{id}");
        }
    }
    let mut actual_queue = ImagePlaybackQueue::default();
    let mut generated_queue = ImagePlaybackQueue::default();
    let now = Instant::now();
    let sync = |queue: &mut ImagePlaybackQueue, ui: &dyn Component, now| {
        let scene = ui.render_shared();
        let image = scene.find("player").unwrap();
        queue.sync(now, image.image_playback.as_ref().map(|slot| (slot, true)));
    };
    sync(&mut actual_queue, &actual, now);
    sync(&mut generated_queue, &generated, now);
    assert!(actual_queue.deadline().is_none());
    assert!(generated_queue.deadline().is_none());

    actual.set("show-animation", true.into()).unwrap();
    generated.set_show_animation(true);
    assert_same_scene(&actual.render(), &generated.render());
    sync(&mut actual_queue, &actual, now);
    sync(&mut generated_queue, &generated, now);
    assert!(generated_queue.deadline().is_some());
    assert_eq!(actual_queue.deadline(), generated_queue.deadline());
    assert!(generated.get_frame_total() > 1);
    assert_eq!(
        actual.get("frame-total"),
        Ok(generated.get_frame_total().into())
    );

    let end = now + Duration::from_secs(1);
    for queue in [&mut actual_queue, &mut generated_queue] {
        assert!(queue.advance(end));
        assert!(queue.next_finished().unwrap().finish());
        assert!(queue.next_finished().is_none());
        assert!(queue.deadline().is_none());
    }
    assert_eq!(actual.get("completions"), Ok(1_i64.into()));
    assert_eq!(generated.get_completions(), 1);

    actual.set("show-animation", false.into()).unwrap();
    generated.set_show_animation(false);
    sync(&mut actual_queue, &actual, end);
    sync(&mut generated_queue, &generated, end);
    assert_eq!(actual.get("frame-total"), Ok(0_i64.into()));
    assert_eq!(generated.get_frame_total(), 0);
    assert!(actual_queue.deadline().is_none());
    assert!(generated_queue.deadline().is_none());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn standalone_mlir_routes_anonymous_handlers_by_scene_path() {
    let source = r#"
        export component App inherits Window {
            out property <string> events: "";
            first := Button { clicked => { root.events += "first;"; } }
            second := Button { clicked => { root.events += "second;"; } }
            toggle := CheckBox { toggled => { root.events += "toggle;"; } }
            left := LineEdit { edited => { root.events += "left;"; } }
            right := LineEdit {
                edited => { root.events += "right;"; }
                accepted => { root.events += "accept;"; }
            }
            collision := LineEdit { text: "reserved"; }
        }
    "#;
    let raw =
        xrack_compiler::mlir::lower(&xrack_compiler::parse(source).unwrap(), "events.xr").unwrap();
    let anonymous = ["first", "second", "toggle", "left", "right"]
        .into_iter()
        .fold(raw, |text, id| {
            text.replace(&format!("id = \"{id}\", key = \"{id}\""), "id = \"\"")
        });
    let anonymous = anonymous.replace("key = \"collision\"", "key = \"__xr_anon_0\"");
    let module = xrack_compiler::mlir::process(&anonymous).unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    assert!(ui.dispatch("", "clicked"));
    assert!(ui.dispatch_at(&ui.render_shared(), &[1], "clicked"));
    assert!(ui.dispatch_at(&ui.render_shared(), &[2], "clicked"));
    assert!(ui.edit_at(&ui.render_shared(), &[4], "draft".into()));
    let scene = ui.render_shared();
    assert_eq!(scene.at_path(&[3]).unwrap().text("text"), Some(""));
    assert_eq!(scene.at_path(&[4]).unwrap().text("text"), Some("draft"));
    assert_eq!(scene.at_path(&[5]).unwrap().text("text"), Some("reserved"));
    assert!(!ui.dispatch_at(&ui.render_shared(), &[3], "accepted"));
    assert!(ui.dispatch_at(&ui.render_shared(), &[4], "accepted"));
    assert_eq!(
        ui.get("events"),
        Ok("first;second;toggle;right;accept;".to_owned().into())
    );

    let rust = xrack_compiler::codegen::generate(&module)
        .unwrap()
        .to_string();
    for (slot, event) in [
        (0, "clicked"),
        (1, "clicked"),
        (2, "toggled"),
        (3, "edited"),
        (4, "edited"),
        (4, "accepted"),
    ] {
        assert!(
            rust.contains(&format!("([{}usize] , \"{event}\") =>", slot)),
            "missing {event} handler for scene slot {slot}"
        );
    }
}

#[test]
fn standalone_mlir_state_selector_can_supply_a_binding_value() {
    let raw = r#"
        module {
          "xr.component"() <{exported = "true", sym_name = "App"}> ({
            %true = "xr.literal"() <{value = "true"}> : () -> !xr.bool
            "xr.property"(%true) <{name = "active", visibility = "in-out"}> : (!xr.bool) -> ()
            "xr.element"() <{id = "root", kind = "Window"}> ({
              "xr.element"() <{id = "", kind = "Text"}> ({
                %active = "xr.read"() <{owner = "root", value = "active"}> : () -> !xr.bool
                %base = "xr.literal"() <{value = "\22inactive\22"}> : () -> !xr.string
                %text = "xr.literal"() <{value = "\22active\22"}> : () -> !xr.string
                %selected = "xr.state_select"(%active) <{names = ["active"], owner = ""}> : (!xr.bool) -> !xr.int
                "xr.bind"(%base) <{name = "text"}> : (!xr.string) -> ()
                "xr.state_set"(%selected, %text) <{case_index = 0 : i64, name = "text"}> : (!xr.int, !xr.string) -> ()
                "xr.bind"(%selected) <{name = "font-weight"}> : (!xr.int) -> ()
                "xr.end"() : () -> ()
              }) : () -> ()
              "xr.end"() : () -> ()
            }) : () -> ()
            "xr.end"() : () -> ()
          }) : () -> ()
        }
    "#;
    use xrack_compiler::mlir::{Module, process};
    let optimized = process(raw).unwrap();
    let roundtrip = Module::parse(&optimized.to_mlir()).unwrap();
    for module in [Module::parse(raw).unwrap(), optimized, roundtrip] {
        let ui = Program::from_module(&module)
            .unwrap()
            .create("App")
            .unwrap();
        for (active, text, weight) in [(true, "active", 0), (false, "inactive", -1)] {
            ui.set("active", active.into()).unwrap();
            let scene = ui.try_render().unwrap();
            let panel = scene.at_path(&[0]).unwrap();
            assert_eq!(panel.text("text"), Some(text));
            assert_eq!(panel.int("font-weight"), Some(weight));
        }
        xrack_compiler::codegen::generate(&module).unwrap();
    }
}

#[test]
fn media_assets_and_gif_clock_match_generated_components() {
    use std::time::{Duration, Instant};
    use xrack_runtime::ImagePlaybackQueue;

    let file = standalone_media_module("media");
    let program = xrack_interpreter::load_mlir_file(&file).unwrap();
    let interpreted_scene = program.create("MediaDemo").unwrap().try_render().unwrap();
    let compiled_scene = media_ui::MediaDemo::new().render();
    let mut badge = interpreted_scene.find("badge").unwrap().clone();
    let commands = badge.text("commands").unwrap();
    let actual = badge.vector_path_geometry(commands).unwrap();
    let expected = compiled_scene
        .find("badge")
        .unwrap()
        .vector_path_geometry(commands)
        .unwrap();
    assert_eq!(actual, expected);
    assert!(!actual.1.is_empty());
    badge
        .properties
        .insert("commands", Value::String("M0 0 L1 1".into()));
    assert!(
        badge
            .vector_path_geometry(badge.text("commands").unwrap())
            .is_none()
    );

    let mut interpreted = program.create("NineSliceDemo").unwrap();
    let mut generated = media_ui::NineSliceDemo::new();
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());
    assert!(interpreted.dispatch("cycle-horizontal", "clicked"));
    assert!(generated.dispatch("cycle-horizontal", "clicked"));
    assert_same_scene(&interpreted.try_render().unwrap(), &generated.render());

    let interpreted = program.create("AnimatedImageDemo").unwrap();
    let mut generated = media_ui::AnimatedImageDemo::new();
    interpreted.set("passes", Value::Int(1)).unwrap();
    generated.set_passes(1);
    let mut interpreted_queue = ImagePlaybackQueue::default();
    let mut generated_queue = ImagePlaybackQueue::default();
    let started = Instant::now();
    for ms in [0, 100, 210, 270] {
        let now = started + Duration::from_millis(ms);
        for (queue, scene) in [
            (&mut interpreted_queue, interpreted.try_render().unwrap()),
            (&mut generated_queue, generated.render_shared()),
        ] {
            queue.sync(
                now,
                scene.children[1]
                    .image_playback
                    .as_ref()
                    .map(|slot| (slot, true)),
            );
            queue.advance(now);
            if let Some(finished) = queue.next_finished() {
                assert!(finished.finish());
            }
        }
        let interpreted_scene = interpreted.try_render().unwrap();
        let generated_scene = generated.render();
        let actual = interpreted_scene.children[1].image_asset("source").unwrap();
        let expected = generated_scene.children[1].image_asset("source").unwrap();
        assert_eq!(actual.frame_count(), expected.frame_count());
        for frame in 0..actual.frame_count() {
            assert_eq!(actual.frame(frame), expected.frame(frame));
        }
        assert_eq!(
            interpreted.get("frame"),
            Ok(Value::Int(generated.get_frame()))
        );
        assert_eq!(
            interpreted.get("frame-total"),
            Ok(Value::Int(generated.get_frame_total()))
        );
        assert_eq!(
            interpreted.get("completions"),
            Ok(Value::Int(generated.get_completions()))
        );
    }
    assert_eq!(interpreted.get("completions"), Ok(Value::Int(1)));
    assert!(interpreted_queue.deadline().is_none());

    let source = std::fs::read_to_string("examples/media/media.xr")
        .unwrap()
        .replace("nine-slice(4 6 8 10)", "nine-slice(400)");
    let path = file.with_file_name("invalid.mlir");
    let filename = path.to_str().unwrap();
    let document = xrack_compiler::parse(&source).unwrap();
    let module = xrack_compiler::mlir::lower_module(&document, filename).unwrap();
    std::fs::write(&path, module.to_mlir()).unwrap();
    let error = xrack_interpreter::load_mlir_file(path).err().unwrap();
    assert!(error.to_string().contains("must leave a nonempty center"));
}

struct FixedText;

impl xrack_layout::TextMeasurer for FixedText {
    fn measure(&mut self, text: &str, font_size: f32, width: f32) -> xrack_layout::Size {
        xrack_layout::Size::new(
            (text.chars().count() as f32 * font_size * 0.55).min(width),
            font_size * 1.4,
        )
    }
}

#[test]
fn observed_pane_content_geometry_matches_compiled_layout() {
    use controls_ui::PaneFrameDemo;
    use xrack_layout::Size;
    use xrack_runtime::Length;

    let source = include_str!("../examples/controls/controls.xr");
    let module =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let instance = Program::from_module(&module.module)
        .unwrap()
        .create("PaneFrameDemo")
        .unwrap();
    let generated = PaneFrameDemo::new();
    assert_eq!(instance.get("pane-inner-width"), Ok(Length(0.0).into()));
    assert_eq!(generated.get_pane_inner_width(), Length(0.0));

    let size = Size::new(460.0, 270.0);
    let mut instance = ui_driver::UiDriver::new(instance, FixedText, size);
    let mut generated = ui_driver::UiDriver::new(generated, FixedText, size);
    let compare = |instance: &Instance, generated: &PaneFrameDemo| {
        for (name, expected) in [
            ("pane-inner-width", generated.get_pane_inner_width()),
            ("frame-inner-width", generated.get_frame_inner_width()),
            ("frame-inner-height", generated.get_frame_inner_height()),
        ] {
            assert_eq!(instance.get(name), Ok(expected.into()), "{name}");
        }
    };
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("pane-inner-width"),
        Ok(Length(390.0).into())
    );
    instance.update(|ui| {
        ui.set("left-padding", Length(32.0).into()).unwrap();
        ui.set("content-size", Length(350.0).into()).unwrap();
    });
    generated.update(|ui| {
        ui.set_left_padding(Length(32.0));
        ui.set_content_size(Length(350.0));
    });
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("pane-inner-width"),
        Ok(Length(350.0).into())
    );
}

#[test]
fn standalone_scroll_bar_uses_live_position_and_rejects_stale_settings() {
    use list_ui::StandaloneScrollDemo;
    use xrack_runtime::RangeAction;

    let mut actual = interpret_list_fixture("StandaloneScrollDemo");
    let mut generated = StandaloneScrollDemo::new();
    let path = [1];
    actual.set("offset", 0.125.into()).unwrap();
    generated.set_offset(0.125);
    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    actual.set("offset", 0.5.into()).unwrap();
    generated.set_offset(0.5);
    assert!(actual.adjust_range_at(&stale, &path, RangeAction::Increase));
    assert!(generated.adjust_range_at(&compiled_stale, &path, RangeAction::Increase));
    assert_eq!(actual.get("offset"), Ok(0.625.into()));
    assert_eq!(generated.get_offset(), 0.625);

    actual.set("thumb-size", 0.75.into()).unwrap();
    generated.set_thumb_size(0.75);
    assert!(!actual.adjust_range_at(&stale, &path, RangeAction::SetFraction(1.0)));
    assert!(!generated.adjust_range_at(&compiled_stale, &path, RangeAction::SetFraction(1.0)));
    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    assert!(actual.adjust_range_at(&scene, &path, RangeAction::SetFraction(0.5)));
    assert!(generated.adjust_range_at(&compiled_scene, &path, RangeAction::SetFraction(0.5)));
    assert_eq!(actual.get("offset"), Ok(0.125.into()));
    assert_eq!(generated.get_offset(), 0.125);

    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    actual.set("interactive", false.into()).unwrap();
    generated.set_interactive(false);
    assert!(!actual.adjust_range_at(&scene, &path, RangeAction::Increase));
    assert!(!generated.adjust_range_at(&compiled_scene, &path, RangeAction::Increase));
    actual.set("interactive", true.into()).unwrap();
    generated.set_interactive(true);
    actual.set("snap-on-release", true.into()).unwrap();
    generated.set_snap_on_release(true);
    assert!(!actual.adjust_range_at(&scene, &path, RangeAction::SetFraction(1.0)));
    assert!(!generated.adjust_range_at(&compiled_scene, &path, RangeAction::SetFraction(1.0)));

    assert_eq!(actual.get("changes"), Ok(2_i64.into()));
    assert_eq!(generated.get_changes(), 2);
    assert_same_scene(&actual.render_shared(), &generated.render_shared());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn standalone_scroll_controls_read_position_before_size() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ScrollReadOrderParity")
        .unwrap();
    let mut generated = expressions_ui::ScrollReadOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    let read = |calls: &RefCell<Vec<i64>>, tag: i64| {
        let mut calls = calls.borrow_mut();
        calls.push(tag);
        if tag % 2 == 1 {
            0.9
        } else if calls.contains(&(tag - 1)) {
            0.25
        } else {
            0.75
        }
    };
    let install = |generated: &mut expressions_ui::ScrollReadOrderParity| {
        interpreted
            .on("read-metric", {
                let calls = actual_calls.clone();
                Rc::new(move |args| {
                    let [Value::Int(tag)] = args else {
                        unreachable!()
                    };
                    Ok(Some(Value::Float(read(&calls, *tag))))
                })
            })
            .unwrap();
        generated.on_read_metric({
            let calls = expected_calls.clone();
            move |tag| read(&calls, tag)
        });
    };
    install(&mut generated);

    assert_eq!(generated.get_bar_position(), 0.75);
    assert_eq!(interpreted.get("bar-position"), Ok(0.75.into()));
    assert_eq!(generated.get_indicator_position(), 0.75);
    assert_eq!(interpreted.get("indicator-position"), Ok(0.75.into()));
    assert_eq!(expected_calls.take(), [1, 2, 3, 4]);
    assert_eq!(actual_calls.take(), [1, 2, 3, 4]);

    install(&mut generated);
    assert_eq!(generated.get_indicator_size(), 0.75);
    assert_eq!(interpreted.get("indicator-size"), Ok(0.75.into()));
    assert_eq!(expected_calls.take(), [4]);
    assert_eq!(actual_calls.take(), [4]);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn viewport_offset_retries_failed_bindings_until_the_first_success() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;
    use xrack_runtime::{Length, Orientation::Vertical};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ViewportOffsetRetryParity")
        .unwrap();
    let mut generated = expressions_ui::ViewportOffsetRetryParity::new();
    let actual_calls = Rc::new(Cell::new(0));
    let generated_calls = Rc::new(Cell::new(0));
    interpreted
        .on("read-offset", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let count = calls.get();
                calls.set(count + 1);
                if count == 0 {
                    return Err("offset unavailable".into());
                }
                Ok(Some(args[0].clone()))
            })
        })
        .unwrap();
    generated.on_read_offset({
        let calls = generated_calls.clone();
        move |offset| {
            let count = calls.get();
            calls.set(count + 1);
            assert_ne!(count, 0, "offset unavailable");
            offset
        }
    });
    let actual = interpreted.render_shared().children[0]
        .viewport_handle
        .clone()
        .unwrap();
    let expected = generated.render_shared().children[0]
        .viewport_handle
        .clone()
        .unwrap();

    assert!(catch_unwind(AssertUnwindSafe(|| expected.bound_offset(Vertical))).is_err());
    assert_eq!(actual.bound_offset(Vertical), Some(0.0));
    assert_eq!(
        interpreted.take_error(),
        Some(Error::Evaluation("offset unavailable".into()))
    );
    for _ in 0..2 {
        assert_eq!(expected.bound_offset(Vertical), Some(48.0));
        assert_eq!(actual.bound_offset(Vertical), Some(48.0));
    }
    assert_eq!(actual_calls.get(), 2);
    assert_eq!(generated_calls.get(), 2);
    assert_eq!(interpreted.take_error(), None);

    interpreted.set("offset", Length(-96.0).into()).unwrap();
    generated.set_offset(Length(-96.0));
    assert_eq!(expected.bound_offset(Vertical), Some(96.0));
    assert_eq!(actual.bound_offset(Vertical), Some(96.0));
    actual.write_bound_offset(Vertical, 16.0);
    expected.write_bound_offset(Vertical, 16.0);
    interpreted.set("offset", Length(-120.0).into()).unwrap();
    generated.set_offset(Length(-120.0));
    assert_eq!(expected.bound_offset(Vertical), Some(16.0));
    assert_eq!(actual.bound_offset(Vertical), Some(16.0));
    assert_eq!(actual_calls.get(), 3);
    assert_eq!(generated_calls.get(), 3);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn linked_scroll_updates_and_user_notifications_match_compiled_component() {
    use list_ui::LinkedScrollDemo;
    use xrack_layout::{Point, Size};
    use xrack_runtime::Length;

    let source = include_str!("../examples/list/list.xr");
    let module = xrack_compiler::prepare_preview(source, "list.xr", Default::default()).unwrap();
    let instance = Program::from_module(&module.module)
        .unwrap()
        .create("LinkedScrollDemo")
        .unwrap();
    let size = Size::new(540.0, 270.0);
    let mut instance = ui_driver::UiDriver::new(instance, FixedText, size);
    let mut generated = ui_driver::UiDriver::new(LinkedScrollDemo::new(), FixedText, size);
    let compare = |instance: &Instance, generated: &LinkedScrollDemo| {
        for (name, expected) in [
            ("large-y", generated.get_large_y().into()),
            ("small-y", generated.get_small_y().into()),
            ("follower-y", generated.get_follower_y().into()),
            ("vertical", generated.get_vertical().into()),
            ("horizontal", generated.get_horizontal().into()),
            ("large-writeback", generated.get_large_writeback().into()),
            ("large-scrolls", Value::Int(generated.get_large_scrolls())),
            ("small-scrolls", Value::Int(generated.get_small_scrolls())),
        ] {
            assert_eq!(instance.get(name), Ok(expected), "{name}");
        }
    };
    compare(instance.component(), generated.component());

    for (point, delta) in [
        (Point { x: 96.0, y: 125.0 }, Point { x: 30.0, y: 20.0 }),
        (Point { x: 414.0, y: 125.0 }, Point { x: 0.0, y: 30.0 }),
    ] {
        assert!(instance.scroll_at(point, delta));
        assert!(generated.scroll_at(point, delta));
        compare(instance.component(), generated.component());
    }
    instance.update(|ui| ui.set("vertical", Length(-170.0).into()).unwrap());
    generated.update(|ui| ui.set_vertical(Length(-170.0)));
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("small-y"),
        Ok(Length(-60.0).into())
    );
    assert_eq!(
        instance.component().get("follower-y"),
        Ok(Length(-86.0).into())
    );

    instance.update(|ui| assert!(ui.dispatch("move-small", "clicked")));
    generated.update(|ui| assert!(ui.dispatch("move-small", "clicked")));
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("vertical"),
        Ok(Length(-40.0).into())
    );
    assert_eq!(instance.component().get("large-scrolls"), Ok(Value::Int(1)));
    assert_eq!(instance.component().get("small-scrolls"), Ok(Value::Int(0)));
}

#[test]
fn scroll_commands_position_virtual_rows_after_model_update() {
    use list_ui::ScrollPositionDemo;
    use xrack_layout::Size;
    use xrack_runtime::Length;

    let source = include_str!("../examples/list/list.xr");
    let module = xrack_compiler::prepare_preview(source, "list.xr", Default::default()).unwrap();
    let instance = Program::from_module(&module.module)
        .unwrap()
        .create("ScrollPositionDemo")
        .unwrap();
    let size = Size::new(540.0, 300.0);
    let mut instance = ui_driver::UiDriver::new(instance, FixedText, size);
    let mut generated = ui_driver::UiDriver::new(ScrollPositionDemo::new(), FixedText, size);
    let compare = |instance: &Instance, generated: &ScrollPositionDemo| {
        for (name, expected) in [
            ("records-y", generated.get_records_y().into()),
            ("records-target", generated.get_records_target().into()),
            ("records-height", generated.get_records_height().into()),
            ("preview-x", generated.get_preview_x().into()),
            ("preview-y", generated.get_preview_y().into()),
            (
                "records-scrolls",
                Value::Int(generated.get_records_scrolls()),
            ),
        ] {
            assert_eq!(instance.get(name), Ok(expected), "{name}");
        }
    };
    instance.update(|ui| assert!(ui.dispatch("jump", "clicked")));
    generated.update(|ui| assert!(ui.dispatch("jump", "clicked")));
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("records-y"),
        Ok(Length(-215_952.0).into())
    );
    let list = &instance.scene().children[2];
    assert_eq!(list.list.unwrap().first_row, 8_998);
    assert!(list.children.len() < 10);

    instance.update(|ui| {
        ui.invoke("resize-and-position", &[Value::Int(18), Value::Int(17)])
            .unwrap();
    });
    generated.update(|ui| ui.invoke_resize_and_position(18, 17));
    compare(instance.component(), generated.component());
    assert_eq!(instance.scene().children[2].list.unwrap().row_count, 18);
    assert_eq!(
        instance.component().get("records-y"),
        Ok(Length(-312.0).into())
    );
}

#[test]
fn layout_feedback_tracks_child_and_instance_geometry_and_popup_lifetime() {
    use layout_ui::GeometryDemo;
    use xrack_layout::Size;
    use xrack_runtime::{ControlAction, Length};

    let source = include_str!("../examples/layout/layout.xr");
    let module = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let instance = Program::from_module(&module.module)
        .unwrap()
        .create("GeometryDemo")
        .unwrap();
    let size = Size::new(500.0, 290.0);
    let mut instance = ui_driver::UiDriver::new(instance, FixedText, size);
    let mut generated = ui_driver::UiDriver::new(GeometryDemo::new(), FixedText, size);

    let compare = |instance: &Instance, generated: &GeometryDemo| {
        for (name, expected) in [
            ("panel-ready", generated.get_panel_ready().into()),
            ("panel-x", generated.get_panel_x().into()),
            ("panel-width", generated.get_panel_width().into()),
            (
                "panel-window-width",
                generated.get_panel_window_width().into(),
            ),
            ("component-width", generated.get_component_width().into()),
            ("tip-width", generated.get_tip_width().into()),
            ("tip-ready", generated.get_tip_ready().into()),
        ] {
            assert_eq!(instance.get(name), Ok(expected), "{name}");
        }
    };
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("panel-ready"),
        Ok(Value::Bool(true))
    );
    assert_eq!(
        instance.component().get("tip-ready"),
        Ok(Value::Bool(false))
    );

    instance.update(|ui| ui.set("rotated", true.into()).unwrap());
    generated.update(|ui| ui.set_rotated(true));
    compare(instance.component(), generated.component());
    assert_ne!(
        instance.component().get("panel-width"),
        instance.component().get("panel-window-width")
    );
    assert_eq!(
        instance.component().get("panel-width"),
        instance.component().get("component-width")
    );

    instance.update(|ui| {
        ui.invoke("show-details", &[]).unwrap();
    });
    generated.update(|ui| ui.invoke_show_details());
    compare(instance.component(), generated.component());
    assert_eq!(instance.component().get("tip-ready"), Ok(Value::Bool(true)));
    assert_eq!(
        instance.component().get("tip-width"),
        instance.component().get("panel-width")
    );

    instance.update(|ui| {
        let scene = ui.render_shared();
        assert!(ui.control_at(&scene, &[0, 2, 0], ControlAction::Popup(false)));
        assert!(!ui.control_at(&ui.render_shared(), &[0, 2, 0], ControlAction::Popup(false)));
        ui.set("show-panel", false.into()).unwrap();
    });
    generated.update(|ui| {
        assert!(ui.control_at(&ui.render_shared(), &[0, 2, 0], ControlAction::Popup(false)));
        ui.set_show_panel(false);
    });
    compare(instance.component(), generated.component());
    assert_eq!(
        instance.component().get("panel-width"),
        Ok(Length(0.0).into())
    );
    assert_eq!(
        instance.component().get("tip-ready"),
        Ok(Value::Bool(false))
    );
}

fn interpret_list_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/list/list.xr");
    let preview = xrack_compiler::prepare_preview(source, "list.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

fn prepare_list(
    component: &impl Component,
    input: &xrack_layout::Interaction,
    size: xrack_layout::Size,
) -> (std::rc::Rc<Node>, xrack_layout::LayoutNode) {
    xrack_layout::prepare_scene_with_input(
        component,
        component.render_shared(),
        size,
        &mut FixedText,
        input,
    )
}

#[test]
fn list_selection_uses_live_rows_and_emits_changed_once() {
    use xrack_runtime::ControlAction::Select;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ListSelectionParity")
        .unwrap();
    let mut generated = expressions_ui::ListSelectionParity::new();
    let actual_model = actual.string_model("labels").unwrap();
    let generated_model = generated.model_labels();

    let before_remove = actual.render_shared();
    let expected_before_remove = generated.render_shared();
    actual.set("allowed", false.into()).unwrap();
    generated.set_allowed(false);
    assert!(!actual.control_at(&before_remove, &[0], Select(2)));
    assert!(!generated.control_at(&expected_before_remove, &[0], Select(2)));
    actual.set("allowed", true.into()).unwrap();
    generated.set_allowed(true);
    assert_eq!(actual_model.remove(2), Some("C".into()));
    assert_eq!(generated_model.remove(2), Some("C".into()));
    assert!(!actual.control_at(&before_remove, &[0], Select(2)));
    assert!(!generated.control_at(&expected_before_remove, &[0], Select(2)));
    assert_eq!(actual.get("selected"), Ok(Value::Int(-1)));
    assert_eq!(generated.get_selected(), -1);

    let before_sort = actual.render_shared();
    let expected_before_sort = generated.render_shared();
    assert!(actual_model.sort_by(|a, b| b.cmp(a)));
    assert!(generated_model.sort_by(|a, b| b.cmp(a)));
    assert!(!actual.control_at(&before_sort, &[0], Select(1)));
    assert!(!generated.control_at(&expected_before_sort, &[0], Select(1)));

    let current = actual.render_shared();
    let expected_current = generated.render_shared();
    assert!(actual_model.set_row(1, "updated A".into()));
    assert!(generated_model.set_row(1, "updated A".into()));
    assert!(actual.control_at(&current, &[0], Select(1)));
    assert!(generated.control_at(&expected_current, &[0], Select(1)));
    assert!(!actual.control_at(&current, &[0], Select(1)));
    assert!(!generated.control_at(&expected_current, &[0], Select(1)));
    assert_eq!(actual.get("selected"), Ok(Value::Int(1)));
    assert_eq!(generated.get_selected(), 1);
    assert_eq!(actual.get("changes"), Ok(Value::Int(1)));
    assert_eq!(generated.get_changes(), 1);

    let selected_scene = actual.render_shared();
    let expected_selected_scene = generated.render_shared();
    actual.set("selected", 0.into()).unwrap();
    generated.set_selected(0);
    assert!(actual.control_at(&selected_scene, &[0], Select(1)));
    assert!(generated.control_at(&expected_selected_scene, &[0], Select(1)));
    assert_eq!(actual.get("changes"), Ok(Value::Int(2)));
    assert_eq!(generated.get_changes(), 2);

    let replacement = xrack_runtime::VecModel::from(actual_model.snapshot());
    let expected_replacement = xrack_runtime::VecModel::from(generated_model.snapshot());
    actual
        .set("labels", Value::StringModel(replacement))
        .unwrap();
    generated.bind_model_labels(expected_replacement);
    assert!(!actual.control_at(&selected_scene, &[0], Select(0)));
    assert!(!generated.control_at(&expected_selected_scene, &[0], Select(0)));
    assert_eq!(actual.get("selected"), Ok(Value::Int(1)));
    assert_eq!(generated.get_selected(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn list_selection_rejects_rows_removed_while_reading_current_index() {
    use expressions_ui::ListSelectionParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction::Select;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ListSelectionParity")
        .unwrap();
    let mut expected = ListSelectionParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();

    let actual_model = actual.string_model("labels").unwrap();
    let expected_model = expected.model_labels();
    actual
        .on("read-selection", {
            let model = Cell::new(Some(actual_model.clone()));
            Rc::new(move |_| {
                if let Some(model) = model.take() {
                    assert_eq!(model.remove(2), Some("C".into()));
                }
                Ok(Some((-1_i64).into()))
            })
        })
        .unwrap();
    expected.on_read_selection({
        let model = Cell::new(Some(expected_model.clone()));
        move || {
            if let Some(model) = model.take() {
                assert_eq!(model.remove(2), Some("C".into()));
            }
            -1
        }
    });
    assert!(!actual.control_at(&actual_scene, &[0], Select(2)));
    assert!(!expected.control_at(&expected_scene, &[0], Select(2)));
    assert_eq!(actual_model.len(), 2);
    assert_eq!(expected_model.len(), 2);
    assert_eq!(actual.get("selected"), Ok((-1_i64).into()));
    assert_eq!(expected.get_selected(), -1);
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(expected.get_changes(), 0);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn page_indicator_selection_uses_live_count_and_interactive_flag() {
    use navigation_ui::NavigationDemo;
    use xrack_runtime::ControlAction::Select;

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/navigation/navigation.xr"),
        "navigation.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("NavigationDemo")
        .unwrap();
    let mut expected = NavigationDemo::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let path = [7, 1];
    actual.set("swipe-interactive", false.into()).unwrap();
    expected.set_swipe_interactive(false);
    assert!(!actual.control_at(&actual_scene, &path, Select(2)));
    assert!(!expected.control_at(&expected_scene, &path, Select(2)));
    actual.set("swipe-interactive", true.into()).unwrap();
    expected.set_swipe_interactive(true);
    actual.set("indicator-count", 2_i64.into()).unwrap();
    expected.set_indicator_count(2);
    assert!(!actual.control_at(&actual_scene, &path, Select(2)));
    assert!(!expected.control_at(&expected_scene, &path, Select(2)));
    assert!(actual.control_at(&actual_scene, &path, Select(1)));
    assert!(expected.control_at(&expected_scene, &path, Select(1)));
    assert!(!actual.control_at(&actual_scene, &path, Select(1)));
    assert!(!expected.control_at(&expected_scene, &path, Select(1)));
    assert_eq!(actual.get("swipe-page"), Ok(1_i64.into()));
    assert_eq!(expected.get_swipe_page(), 1);
    assert_eq!(actual.get("indicator-changes"), Ok(1_i64.into()));
    assert_eq!(expected.get_indicator_changes(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn filtered_list_delegates_keep_source_index_height_and_state() {
    use list_ui::ListVisibilityDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, Length, ListMetrics, Property};

    let mut interpreted = interpret_list_fixture("ListVisibilityDemo");
    let mut generated = ListVisibilityDemo::new();
    let Value::RecordList(records) = interpreted.get("records").unwrap() else {
        panic!("expected a record list");
    };
    let actual_model = records.model();
    let expected_model = generated.model_records();
    let input = Interaction::default();
    let size = Size::new(660.0, 320.0);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    for slot in 1..=3 {
        let actual = &actual.children[slot];
        let expected = &expected.children[slot];
        assert_eq!(actual.list, expected.list);
        assert_eq!(actual.list.unwrap().row_count, 3);
        assert_eq!(actual.children[1].row_key, actual_model.row_key(2));
        assert_eq!(expected.children[1].row_key, expected_model.row_key(2));
    }
    let tile = [2, 1, 0];
    let check = [1, 1, 0];
    assert!(interpreted.dispatch_at(&actual, &tile, "clicked"));
    assert!(generated.dispatch_at(&expected, &tile, "clicked"));
    assert_eq!(
        interpreted.get("last-source-index"),
        Ok(Value::Int(generated.get_last_source_index()))
    );
    assert_eq!(interpreted.get("last-source-index"), Ok(Value::Int(2)));
    assert!(interpreted.dispatch_at(&actual, &check, "clicked"));
    assert!(generated.dispatch_at(&expected, &check, "clicked"));
    assert!(!interpreted.dispatch_at(&actual, &tile, "clicked"));
    assert!(!interpreted.control_at(&actual, &[3], ControlAction::Select(1)));
    assert_eq!(interpreted.get("visible-index"), Ok(Value::Int(-1)));
    assert_eq!(generated.get_visible_index(), -1);
    interpreted
        .invoke("select-source", &[Value::Int(2)])
        .unwrap();
    generated.invoke_select_source(2);
    assert_eq!(interpreted.get("standard-index"), Ok(Value::Int(0)));
    assert_eq!(generated.get_standard_index(), 0);

    let flags = ["flags".into(), "shown".into()];
    let row = actual_model.row(2).unwrap();
    assert_eq!(row.field_path(&flags), Some(&Value::Bool(false)));
    assert!(actual_model.set_row(2, row.with_field(&flags, true.into()).unwrap()));
    let mut row = expected_model.row(2).unwrap();
    row.flags.shown = true;
    assert!(expected_model.set_row(2, row));
    interpreted
        .invoke("select-source", &[Value::Int(2)])
        .unwrap();
    generated.invoke_select_source(2);
    assert_eq!(interpreted.get("standard-index"), Ok(Value::Int(1)));
    assert_eq!(
        interpreted.get("selected-source"),
        Ok(Value::Int(generated.get_selected_source()))
    );
    assert_eq!(interpreted.get("selected-source"), Ok(Value::Int(2)));

    let visible_index = Property::default();
    assert_eq!(
        visible_index.get(|| interpreted.get("visible-index")),
        Ok(Value::Int(1))
    );
    let height = ["height".into()];
    let row = actual_model
        .row(2)
        .unwrap()
        .with_field(&height, Length(67.0).into())
        .unwrap();
    assert!(actual_model.set_row(2, row));
    let mut row = expected_model.row(2).unwrap();
    row.height = Length(67.0);
    assert!(expected_model.set_row(2, row));
    assert!(!visible_index.is_dirty());
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(
        actual.at_path(&tile).unwrap().text("text"),
        Some("图片 · 1")
    );
    assert_eq!(
        actual.at_path(&tile).unwrap().text("text"),
        expected.at_path(&tile).unwrap().text("text")
    );
    for slot in [1, 3] {
        assert_eq!(ListMetrics::row_height_at(&actual.children[slot], 1), 67.0);
        assert_eq!(
            ListMetrics::row_height_at(&expected.children[slot], 1),
            67.0
        );
    }
}

#[test]
fn direct_scroll_commands_preserve_operand_and_queue_order() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{Length, ScrollAction, ScrollAlignment, ScrollRequests};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ScrollCommandParity")
        .unwrap();
    let mut compiled = expressions_ui::ScrollCommandParity::new();
    let actual_reads = Rc::new(RefCell::new(Vec::new()));
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    let offset = |reads: &RefCell<Vec<i64>>, tag| {
        reads.borrow_mut().push(tag);
        Length(tag as f64 * 10.0)
    };
    actual
        .on("offset", {
            let reads = actual_reads.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                Ok(Some(offset(&reads, *tag).into()))
            })
        })
        .unwrap();
    compiled.on_offset({
        let reads = compiled_reads.clone();
        move |tag| offset(&reads, tag)
    });

    let queue = ScrollRequests::default();
    let _context = queue.enter();
    actual.invoke("issue", &[]).unwrap();
    let interpreted = queue.drain();
    compiled.invoke_issue();
    let generated = queue.drain();
    let expected = [
        ScrollAction::CancelFlick,
        ScrollAction::To {
            x: Length(10.0),
            y: Length(20.0),
        },
        ScrollAction::ReturnToBounds,
        ScrollAction::ToIndex {
            index: 1,
            alignment: ScrollAlignment::Beginning,
        },
        ScrollAction::ToColumn {
            column: 0,
            alignment: ScrollAlignment::End,
        },
        ScrollAction::ToCell {
            row: 1,
            column: 0,
            horizontal: ScrollAlignment::Center,
            vertical: ScrollAlignment::Contain,
        },
    ];
    for (commands, scene) in [
        (interpreted, actual.render_shared()),
        (generated, compiled.render_shared()),
    ] {
        assert_eq!(commands.len(), expected.len());
        for (index, (command, action)) in commands.iter().zip(expected).enumerate() {
            assert_eq!(command.action, action);
            assert_eq!(
                Some(&command.target),
                scene.children[usize::from(index >= 3)]
                    .viewport_handle
                    .as_ref()
            );
        }
    }
    assert_eq!(actual_reads.take(), [1, 2]);
    assert_eq!(compiled_reads.take(), [1, 2]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn source_cell_scroll_rejects_negative_positions_before_resolving_rows() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use xrack_runtime::{ScrollAction, ScrollAlignment, ScrollRequests};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let interpreted_model = interpreted.record_model("rows").unwrap();
    let compiled_model = generated.model_rows();
    let reads = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let args = [
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(RefCell::new(Vec::new())),
    ];
    interpreted
        .on("source", {
            let reads = reads[0].clone();
            let rows = interpreted_model.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(Value::RecordList(rows.clone())))
            })
        })
        .unwrap();
    generated.on_source({
        let reads = reads[1].clone();
        move || {
            reads.set(reads.get() + 1);
            compiled_model.snapshot()
        }
    });
    interpreted
        .on("selection-position", {
            let args = args[0].clone();
            Rc::new(move |values| {
                let [Value::Int(index)] = values else {
                    unreachable!()
                };
                args.borrow_mut().push(*index);
                Ok(Some((*index).into()))
            })
        })
        .unwrap();
    generated.on_selection_position({
        let args = args[1].clone();
        move |index| {
            args.borrow_mut().push(index);
            index
        }
    });

    let requests = ScrollRequests::default();
    let _context = requests.enter();
    for (row, column) in [(-1_i64, 0_i64), (0, -1)] {
        interpreted
            .invoke("reveal-source-cell", &[row.into(), column.into()])
            .unwrap();
        generated.invoke_reveal_source_cell(row, column);
        assert!(requests.is_empty());
        for args in &args {
            assert_eq!(args.take(), [row, column]);
        }
        assert_eq!(reads.each_ref().map(|reads| reads.get()), [0, 0]);
    }

    interpreted
        .invoke("reveal-source-cell", &[1_i64.into(), 0_i64.into()])
        .unwrap();
    generated.invoke_reveal_source_cell(1, 0);
    let actions = requests.drain();
    assert_eq!(actions.len(), 2);
    for request in actions {
        assert!(matches!(
            request.action,
            ScrollAction::ToCellAtRowKey {
                source_index: 1,
                column: 0,
                horizontal: ScrollAlignment::Contain,
                vertical: ScrollAlignment::Beginning,
                ..
            }
        ));
    }
    assert_eq!(reads.each_ref().map(|reads| reads.get()), [1, 1]);
}

#[test]
fn queued_source_scroll_uses_row_key_after_the_projection_changes() {
    use list_ui::{ListVisibilityDemo, ListVisibilityFlags, ListVisibilityRecord};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{Length, RecordList, RecordValue, ScrollRequests, VecModel};

    let interpreted = interpret_list_fixture("ListVisibilityDemo");
    let mut generated = ListVisibilityDemo::new();
    let items: Vec<_> = (0..64)
        .map(|index| ListVisibilityRecord {
            label: format!("record {index}"),
            height: Length(24.0),
            flags: ListVisibilityFlags {
                shown: index % 4 != 1,
            },
        })
        .collect();
    let dynamic = |items: &[ListVisibilityRecord]| {
        RecordList::new(
            "ListVisibilityRecord",
            items
                .iter()
                .map(|item| {
                    let flags = RecordValue::new(
                        "ListVisibilityFlags",
                        [("shown".into(), item.flags.shown.into())].into(),
                    );
                    RecordValue::new(
                        "ListVisibilityRecord",
                        [
                            ("label".into(), item.label.clone().into()),
                            ("height".into(), item.height.into()),
                            ("flags".into(), Value::Record(flags)),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )
    };
    let records = dynamic(&items);
    interpreted
        .set("records", Value::RecordList(records.clone()))
        .unwrap();
    generated.set_records(items.clone());
    let actual_model = records.model();
    let expected_model = generated.model_records();
    let actual_requests = ScrollRequests::default();
    let expected_requests = ScrollRequests::default();
    let size = Size::new(660.0, 320.0);
    {
        let _context = actual_requests.enter();
        interpreted
            .invoke("locate-source", &[Value::Int(30)])
            .unwrap();
    }
    {
        let _context = expected_requests.enter();
        generated.invoke_locate_source(30);
    }
    let row = actual_model
        .row(0)
        .unwrap()
        .with_field(&["flags".into(), "shown".into()], false.into())
        .unwrap();
    assert!(actual_model.set_row(0, row));
    let mut row = expected_model.row(0).unwrap();
    row.flags.shown = false;
    assert!(expected_model.set_row(0, row));

    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let (actual, layout) = prepare_list(&interpreted, &actual_input, size);
    assert!(actual_input.apply_scroll_requests(&actual, &layout, actual_requests.drain()));
    let (expected, layout) = prepare_list(&generated, &expected_input, size);
    assert!(expected_input.apply_scroll_requests(&expected, &layout, expected_requests.drain()));
    let (actual, _) = prepare_list(&interpreted, &actual_input, size);
    let (expected, _) = prepare_list(&generated, &expected_input, size);
    for slot in 1..=3 {
        let actual = &actual.children[slot];
        let expected = &expected.children[slot];
        assert_eq!(actual.list, expected.list);
        assert!(
            actual
                .children
                .iter()
                .any(|row| row.row_key == actual_model.row_key(30))
        );
        assert!(
            expected
                .children
                .iter()
                .any(|row| row.row_key == expected_model.row_key(30))
        );
    }

    {
        let _context = actual_requests.enter();
        interpreted
            .invoke("locate-source", &[Value::Int(30)])
            .unwrap();
    }
    {
        let _context = expected_requests.enter();
        generated.invoke_locate_source(30);
    }
    interpreted
        .set("records", Value::RecordList(dynamic(&items)))
        .unwrap();
    generated.bind_model_records(VecModel::from(items));
    let mut fresh = Interaction::default();
    let (actual, layout) = prepare_list(&interpreted, &fresh, size);
    assert!(!fresh.apply_scroll_requests(&actual, &layout, actual_requests.drain()));
    let (expected, layout) = prepare_list(&generated, &fresh, size);
    assert!(!fresh.apply_scroll_requests(&expected, &layout, expected_requests.drain()));
}

#[test]
fn list_rows_only_materializes_views_in_the_current_scene() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("MountedListParity")
        .unwrap();
    let mut generated = expressions_ui::MountedListParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("row-label", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(row)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*row);
                Ok(Some(Value::String(row.to_string())))
            })
        })
        .unwrap();
    generated.on_row_label({
        let calls = expected_calls.clone();
        move |row| {
            calls.borrow_mut().push(row);
            row.to_string()
        }
    });
    let path = &[0, 1, 0];
    let row_counts = |generated: &dyn Component| {
        [
            interpreted.list_rows(path, 1..3).len(),
            generated.list_rows(path, 1..3).len(),
        ]
    };

    assert_eq!(row_counts(&generated), [0, 0]);
    assert!(actual_calls.take().is_empty());
    assert!(expected_calls.take().is_empty());

    interpreted.set("page", 1_i64.into()).unwrap();
    generated.set_page(1);
    let actual = interpreted.list_rows(path, 1..3);
    let expected = generated.list_rows(path, 1..3);
    assert_eq!(actual.len(), 2);
    assert_eq!(expected.len(), 2);
    for (actual, expected) in actual.iter().zip(&expected) {
        assert_same_scene(actual, expected);
    }
    assert_eq!(actual_calls.take(), [1, 2]);
    assert_eq!(expected_calls.take(), [1, 2]);

    interpreted.set("show-list", false.into()).unwrap();
    generated.set_show_list(false);
    assert_eq!(row_counts(&generated), [0, 0]);
    assert!(actual_calls.take().is_empty());
    assert!(expected_calls.take().is_empty());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn nested_virtual_list_callbacks_reach_the_correct_outer_delegate() {
    use list_ui::NestedListsDemo;
    use xrack_layout::{Interaction, Size};

    let mut interpreted = interpret_list_fixture("NestedListsDemo");
    let mut generated = NestedListsDemo::new();
    let size = Size::new(580.0, 330.0);
    let input = Interaction::default();
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    for (outer, album) in [(0, "文档"), (1, "图片")] {
        let path = [1, outer, 0, 1, 0, 0];
        assert!(interpreted.dispatch_at(&actual, &path, "clicked"));
        assert!(generated.dispatch_at(&expected, &path, "clicked"));
        assert_eq!(
            interpreted.get("last-played"),
            Ok(generated.get_last_played().into())
        );
        assert_eq!(
            interpreted.get("last-played"),
            Ok(format!("{album} · 条目 0").into())
        );
    }
}

fn interpret_table_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/table/table.xr");
    let preview = xrack_compiler::prepare_preview(source, "table.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

fn table_row_selection_fixture() -> (Instance, expressions_ui::TableRowSelectionParity) {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableRowSelectionParity")
        .unwrap();
    (actual, expressions_ui::TableRowSelectionParity::new())
}

#[test]
fn table_single_selection_checks_live_cursor_and_emits_one_row_event() {
    use xrack_runtime::ControlAction::Select;

    let (mut actual, mut expected) = table_row_selection_fixture();
    let path = [0];
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    assert!(
        actual_scene
            .at_path(&path)
            .unwrap()
            .table_selection
            .is_none()
    );
    assert!(
        expected_scene
            .at_path(&path)
            .unwrap()
            .table_selection
            .is_none()
    );
    assert!(actual.control_at(&actual_scene, &path, Select(1)));
    assert!(expected.control_at(&expected_scene, &path, Select(1)));
    assert!(!actual.control_at(&actual_scene, &path, Select(1)));
    assert!(!expected.control_at(&expected_scene, &path, Select(1)));

    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    actual.set("current", 0_i64.into()).unwrap();
    expected.set_current(0);
    assert!(actual.control_at(&actual_scene, &path, Select(1)));
    assert!(expected.control_at(&expected_scene, &path, Select(1)));
    assert_eq!(actual.get("current"), Ok(1_i64.into()));
    assert_eq!(expected.get_current(), 1);
    assert_eq!(actual.get("row-events"), Ok(2_i64.into()));
    assert_eq!(expected.get_row_events(), 2);
    assert_eq!(actual.get("cell-events"), Ok(0_i64.into()));
    assert_eq!(expected.get_cell_events(), 0);
    assert_eq!(actual.get("selection-events"), Ok(0_i64.into()));
    assert_eq!(expected.get_selection_events(), 0);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_selection_rejects_stale_mode_and_disabled_views() {
    use xrack_runtime::ControlAction::{Select, TableSelection};
    use xrack_runtime::{RowSelection, TableSelectionAction};

    let (mut actual, mut expected) = table_row_selection_fixture();
    let path = [0];
    let actual_plain = actual.render_shared();
    let expected_plain = expected.render_shared();
    actual.set("multiple", true.into()).unwrap();
    expected.set_multiple(true);
    assert!(!actual.control_at(&actual_plain, &path, Select(1)));
    assert!(!expected.control_at(&expected_plain, &path, Select(1)));

    let actual_multiple = actual.render_shared();
    let expected_multiple = expected.render_shared();
    let toggle = TableSelection(TableSelectionAction::Row {
        index: 1,
        operation: RowSelection::Toggle,
    });
    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    assert!(!actual.control_at(&actual_multiple, &path, toggle));
    assert!(!expected.control_at(&expected_multiple, &path, toggle));
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    actual.set("multiple", false.into()).unwrap();
    expected.set_multiple(false);
    assert!(!actual.control_at(&actual_multiple, &path, toggle));
    assert!(!expected.control_at(&expected_multiple, &path, toggle));

    actual.set("multiple", true.into()).unwrap();
    expected.set_multiple(true);
    assert!(actual.control_at(&actual_multiple, &path, toggle));
    assert!(expected.control_at(&expected_multiple, &path, toggle));
    assert_eq!(actual.get("current"), Ok(1_i64.into()));
    assert_eq!(expected.get_current(), 1);
    assert_eq!(actual.get("row-events"), Ok(1_i64.into()));
    assert_eq!(expected.get_row_events(), 1);
    assert_eq!(actual.get("cell-events"), Ok(1_i64.into()));
    assert_eq!(expected.get_cell_events(), 1);
    assert_eq!(actual.get("selection-events"), Ok(1_i64.into()));
    assert_eq!(expected.get_selection_events(), 1);
    assert_same_scene(&actual.render_shared(), &expected.render_shared());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_selection_rejects_rows_removed_during_cursor_read() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction::Select;

    for multiple in [false, true] {
        let (mut actual, mut expected) = table_row_selection_fixture();
        actual.set("multiple", multiple.into()).unwrap();
        expected.set_multiple(multiple);
        let actual_scene = actual.render_shared();
        let expected_scene = expected.render_shared();
        let actual_model = actual.string_model("labels").unwrap();
        let expected_model = expected.model_labels();
        actual
            .on("read-row", {
                let model = Cell::new(Some(actual_model.clone()));
                Rc::new(move |_| {
                    if let Some(model) = model.take() {
                        assert_eq!(model.remove(2), Some("C".into()));
                    }
                    Ok(Some((-1_i64).into()))
                })
            })
            .unwrap();
        expected.on_read_row({
            let model = Cell::new(Some(expected_model.clone()));
            move || {
                if let Some(model) = model.take() {
                    assert_eq!(model.remove(2), Some("C".into()));
                }
                -1
            }
        });
        assert!(!actual.control_at(&actual_scene, &[0], Select(2)));
        assert!(!expected.control_at(&expected_scene, &[0], Select(2)));
        assert_eq!(actual_model.len(), 2);
        assert_eq!(expected_model.len(), 2);
        assert_eq!(actual.get("current"), Ok((-1_i64).into()));
        assert_eq!(expected.get_current(), -1);
        assert_eq!(actual.get("row-events"), Ok(0_i64.into()));
        assert_eq!(expected.get_row_events(), 0);
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn table_selection_rejects_the_previous_column_order() {
    use xrack_runtime::ControlAction::{MoveColumn, Select, TableSelection};
    use xrack_runtime::{RowSelection, TableSelectionAction};

    let mut actual = interpret_table_fixture("TableDemo");
    let mut generated = table_ui::TableDemo::new();
    let records = vec!["first".into(), "second".into()];
    actual
        .set("records", Value::StringList(records.clone()))
        .unwrap();
    generated.set_records(records);
    let path = [0, 2];
    let before = actual.render_shared();
    let expected_before = generated.render_shared();
    let reorder = MoveColumn { from: 0, to: 1 };
    assert!(actual.control_at(&before, &path, reorder));
    assert!(generated.control_at(&expected_before, &path, reorder));

    for action in [
        Select(1),
        TableSelection(TableSelectionAction::Row {
            index: 1,
            operation: RowSelection::Replace,
        }),
    ] {
        assert!(!generated.control_at(&expected_before, &path, action));
        assert!(!actual.control_at(&before, &path, action));
    }
    assert_eq!(actual.get("selections"), Ok(Value::Int(0)));
    assert_eq!(generated.get_selections(), 0);

    let after = actual.render_shared();
    let expected_after = generated.render_shared();
    assert!(actual.control_at(&after, &path, Select(1)));
    assert!(generated.control_at(&expected_after, &path, Select(1)));
    assert_eq!(actual.get("current"), Ok(Value::Int(1)));
    assert_eq!(generated.get_current(), 1);
    assert_eq!(actual.get("selections"), Ok(Value::Int(1)));
    assert_eq!(generated.get_selections(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_row_heights_and_scroll_anchor_match_compiled_mode() {
    use table_ui::{LogRow, ProjectedRowsDemo};
    use xrack_layout::Size;
    use xrack_runtime::{Length, ListMetrics, RecordList, RecordValue};

    let interpreted = interpret_table_fixture("ProjectedRowsDemo");
    let mut generated = ProjectedRowsDemo::new();
    let rows: Vec<_> = (0..128)
        .map(|index| LogRow {
            message: index.to_string(),
            severity: "info".into(),
            height: Length(if index == 30 { 64.0 } else { 0.0 }),
        })
        .collect();
    let records = RecordList::new(
        "LogRow",
        rows.iter()
            .map(|row| {
                RecordValue::new(
                    "LogRow",
                    [
                        ("message".into(), row.message.clone().into()),
                        ("severity".into(), row.severity.clone().into()),
                        ("height".into(), row.height.into()),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    interpreted
        .set("entries", Value::RecordList(records.clone()))
        .unwrap();
    generated.set_entries(rows);
    let model = generated.model_entries();
    let size = Size::new(660.0, 310.0);
    let mut interpreted = ui_driver::UiDriver::new(interpreted, FixedText, size);
    let mut generated = ui_driver::UiDriver::new(generated, FixedText, size);
    interpreted.update(|ui| {
        ui.invoke("center-row", &[Value::Int(30)]).unwrap();
    });
    generated.update(|ui| ui.invoke_center_row(30));
    let anchor = interpreted.scene().children[1].children[0].row_key;
    assert_eq!(
        interpreted.component().get("offset"),
        Ok(generated.component().get_offset().into())
    );
    assert_eq!(
        interpreted.scene().children[1].list,
        generated.scene().children[1].list
    );
    assert!(interpreted.scene().children[1].children.len() < 10);
    interpreted.update(|ui| {
        ui.invoke("set-height", &[Value::Int(30), Length(80.0).into()])
            .unwrap();
    });
    generated.update(|ui| ui.invoke_set_height(30, Length(80.0)));
    interpreted.update(|_| {
        for (row, height) in [(0, 96.0), (30, 120.0)] {
            let record = records
                .model()
                .row(row)
                .unwrap()
                .with_field(&["height".into()], Length(height).into())
                .unwrap();
            assert!(records.model().set_row(row, record));
        }
    });
    generated.update(|_| {
        for (row, height) in [(0, 96.0), (30, 120.0)] {
            let mut record = model.row(row).unwrap();
            record.height = Length(height);
            assert!(model.set_row(row, record));
        }
    });
    let row = |scene: &Node, view, index| {
        ListMetrics::row_height_at(scene.child_at(view).unwrap(), index)
    };
    assert_eq!(row(interpreted.scene(), 1, 30), 80.0);
    assert_eq!(row(interpreted.scene(), 2, 30), 120.0);
    assert_eq!(
        row(interpreted.scene(), 1, 30),
        row(generated.scene(), 1, 30)
    );
    assert_eq!(interpreted.scene().children[1].children[0].row_key, anchor);
    assert_eq!(
        interpreted.component().get("offset"),
        Ok(generated.component().get_offset().into())
    );
    interpreted.update(|ui| {
        ui.invoke("set-height", &[Value::Int(30), Length(-1.0).into()])
            .unwrap();
    });
    generated.update(|ui| ui.invoke_set_height(30, Length(-1.0)));
    assert_eq!(row(interpreted.scene(), 1, 30), 120.0);
    assert_eq!(
        row(interpreted.scene(), 1, 30),
        row(generated.scene(), 1, 30)
    );
}

#[test]
fn tables_project_sort_filter_selection_and_live_record_edits() {
    use table_ui::RowVisibilityDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, Length, ListMetrics};

    let mut interpreted = interpret_table_fixture("RowVisibilityDemo");
    let mut generated = RowVisibilityDemo::new();
    let Value::RecordList(records) = interpreted.get("records").unwrap() else {
        panic!("expected records");
    };
    let expected_model = generated.model_records();
    let size = Size::new(720.0, 320.0);
    let input = Interaction::default();
    interpreted.set("inspected-source", Value::Int(0)).unwrap();
    generated.set_inspected_source(0);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let custom = [1];
    let standard = [2];
    assert_eq!(
        actual.at_path(&standard).unwrap().list,
        expected.at_path(&standard).unwrap().list
    );
    assert!(interpreted.control_at(&actual, &standard, ControlAction::SortColumn(1)));
    assert!(generated.control_at(&expected, &standard, ControlAction::SortColumn(1)));
    assert_eq!(interpreted.get("standard-visible"), Ok(Value::Int(1)));
    assert_eq!(
        interpreted.get("standard-visible"),
        Ok(Value::Int(generated.get_standard_visible()))
    );

    interpreted
        .invoke("resize-row", &[Value::Int(1), Length(52.0).into()])
        .unwrap();
    generated.invoke_resize_row(1, Length(52.0));
    interpreted
        .invoke("select-source", &[Value::Int(0), Value::Int(1)])
        .unwrap();
    generated.invoke_select_source(0, 1);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let table = actual.at_path(&standard).unwrap();
    assert!(
        table
            .table_selection
            .as_ref()
            .unwrap()
            .contains_cell(records.model().row_key(0).unwrap(), 1)
    );
    assert_eq!(
        interpreted.copy_table_at(&actual, &standard, &[0, 1]),
        generated.copy_table_at(&expected, &standard, &[0, 1])
    );
    assert_eq!(
        interpreted
            .copy_table_at(&actual, &standard, &[0, 1])
            .as_deref(),
        Some("3")
    );
    assert_eq!(
        ListMetrics::row_height_at(actual.at_path(&custom).unwrap(), 1),
        52.0
    );

    let cell = [1, 1, 0, 0];
    assert!(interpreted.commit_table_cell(&actual, &cell, "工作站", "新工作站".into()));
    assert!(generated.commit_table_cell(&expected, &cell, "工作站", "新工作站".into()));
    assert!(!interpreted.commit_table_cell(&actual, &cell, "工作站", "过期修改".into()));
    assert_eq!(
        records.model().row(1).unwrap().field("name"),
        Some(&Value::String(expected_model.row(1).unwrap().name))
    );
    interpreted
        .set("query", Value::String("新".into()))
        .unwrap();
    generated.set_query("新".into());
    assert_eq!(interpreted.get("standard-visible"), Ok(Value::Int(-1)));
    assert_eq!(
        interpreted.get("standard-visible"),
        Ok(Value::Int(generated.get_standard_visible()))
    );
    assert!(!interpreted.control_at(&actual, &standard, ControlAction::SortColumn(1)));
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(
        actual.at_path(&standard).unwrap().list.unwrap().row_count,
        1
    );
    assert_eq!(
        actual.at_path(&standard).unwrap().list,
        expected.at_path(&standard).unwrap().list
    );
}

#[test]
fn wide_tables_materialize_only_current_logical_columns() {
    use table_ui::WideTableDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, RowSelection, TableSelectionAction};

    let mut interpreted = interpret_table_fixture("WideTableDemo");
    let mut generated = WideTableDemo::new();
    let size = Size::new(620.0, 320.0);
    let input = Interaction::default();
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    for action in [
        ControlAction::ResizeColumn {
            column: 0,
            width: 136,
        },
        ControlAction::MoveColumn { from: 11, to: 1 },
    ] {
        assert!(interpreted.control_at(&actual, &[1], action));
        assert!(generated.control_at(&expected, &[1], action));
    }
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let select = ControlAction::TableSelection(TableSelectionAction::Cell {
        row: 0,
        column: 11,
        operation: RowSelection::Replace,
    });
    assert!(interpreted.control_at(&actual, &[1], select));
    assert!(generated.control_at(&expected, &[1], select));
    interpreted.set("show-number", false.into()).unwrap();
    interpreted.set("show-february", false.into()).unwrap();
    generated.set_show_number(false);
    generated.set_show_february(false);
    let (actual, actual_layout) = prepare_list(&interpreted, &input, size);
    let (expected, expected_layout) = prepare_list(&generated, &input, size);
    let indices = |layout: &xrack_layout::LayoutNode| {
        layout
            .at_path(&[1])
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .visual_columns()
            .collect::<Vec<_>>()
    };
    assert_eq!(indices(&actual_layout), indices(&expected_layout));
    assert_eq!(
        actual.at_path(&[1, 0, 0, 11]).unwrap().text("text"),
        Some("1 / 12")
    );
    assert!(actual.at_path(&[1, 0, 0, 0]).is_none());
    assert!(actual.at_path(&[1, 0, 0, 2]).is_none());
    assert_eq!(
        actual.at_path(&[1, 0, 0]).unwrap().children.len(),
        expected.at_path(&[1, 0, 0]).unwrap().children.len()
    );
    assert_eq!(
        interpreted.copy_table_at(&actual, &[1], &indices(&actual_layout)),
        generated.copy_table_at(&expected, &[1], &indices(&expected_layout))
    );
}

#[test]
fn shared_records_keep_independent_table_sorting_and_source_order() {
    use table_ui::TableSortViewsDemo;
    use xrack_layout::{Interaction, Size};

    let mut interpreted = interpret_table_fixture("TableSortViewsDemo");
    let mut generated = TableSortViewsDemo::new();
    let Value::RecordList(records) = interpreted.get("inventory").unwrap() else {
        panic!("expected an inventory model");
    };
    let expected_model = generated.model_inventory();
    let size = Size::new(680.0, 370.0);
    let input = Interaction::default();
    interpreted.set("left-current", Value::Int(0)).unwrap();
    generated.set_left_current(0);
    interpreted.invoke("initialize", &[]).unwrap();
    generated.invoke_initialize();
    assert_eq!(interpreted.get("left-current"), Ok(Value::Int(3)));
    assert_eq!(
        interpreted.get("left-current"),
        Ok(Value::Int(generated.get_left_current()))
    );
    let quantity = [0, 1, 0, 1];
    let name = [0, 1, 1, 1];
    let labels = |scene: &Node, path: &[usize]| {
        scene
            .at_path(path)
            .unwrap()
            .children
            .iter()
            .map(|row| {
                row.at_path(&[0, 0])
                    .unwrap()
                    .text("text")
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(
        labels(&actual, &quantity),
        ["Beta", "Alpha", "Gamma", "Zulu"]
    );
    assert_eq!(labels(&actual, &quantity), labels(&expected, &quantity));
    assert_eq!(labels(&actual, &name), labels(&expected, &name));
    let name_stamp = actual.at_path(&name).unwrap().standard_list_stamp;

    let record = records
        .model()
        .row(1)
        .unwrap()
        .with_field(&["units".into()], Value::Int(40))
        .unwrap();
    assert!(records.model().set_row(1, record));
    let mut record = expected_model.row(1).unwrap();
    record.units = 40;
    assert!(expected_model.set_row(1, record));
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(
        labels(&actual, &quantity),
        ["Beta", "Gamma", "Zulu", "Alpha"]
    );
    assert_eq!(labels(&actual, &quantity), labels(&expected, &quantity));
    assert_eq!(
        actual.at_path(&name).unwrap().standard_list_stamp,
        name_stamp
    );
    assert_eq!(labels(&actual, &name), labels(&expected, &name));

    assert!(interpreted.dispatch("clear-quantity-sort", "clicked"));
    assert!(generated.dispatch("clear-quantity-sort", "clicked"));
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(
        labels(&actual, &quantity),
        ["Zulu", "Alpha", "Beta", "Gamma"]
    );
    assert_eq!(labels(&actual, &quantity), labels(&expected, &quantity));
    assert_eq!(labels(&actual, &name), ["Alpha", "Beta", "Gamma", "Zulu"]);
    assert_eq!(
        interpreted.get("left-current"),
        Ok(Value::Int(generated.get_left_current()))
    );
}

#[test]
fn standard_table_paste_and_edit_history_match_compiled_transactions() {
    use table_ui::{InventoryRow, StandardTableDemo};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{RecordList, RecordValue, Time};

    let mut interpreted = interpret_table_fixture("StandardTableDemo");
    let mut generated = StandardTableDemo::new();
    interpreted.set("edit-columns", true.into()).unwrap();
    interpreted.set("paste-cells", true.into()).unwrap();
    generated.set_edit_columns(true);
    generated.set_paste_cells(true);
    let rows = vec![InventoryRow::default(); 3];
    let records = RecordList::new(
        "InventoryRow",
        rows.iter()
            .map(|row| {
                RecordValue::new(
                    "InventoryRow",
                    [
                        ("name".into(), row.name.clone().into()),
                        ("units".into(), row.units.into()),
                        ("available".into(), row.available.into()),
                        ("refreshed".into(), Value::Time(row.refreshed)),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    interpreted
        .set("inventory", Value::RecordList(records.clone()))
        .unwrap();
    generated.set_inventory(rows);
    let actual = records.model();
    let expected = generated.model_inventory();
    let path = [0, 1];
    let paste = |ui: &dyn Component, columns| {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, columns)
            .unwrap()
    };
    let compare = || {
        for (actual, expected) in actual.snapshot().iter().zip(expected.snapshot()) {
            assert_eq!(actual.field("name"), Some(&Value::String(expected.name)));
            assert_eq!(actual.field("units"), Some(&Value::Int(expected.units)));
            assert_eq!(
                actual.field("available"),
                Some(&Value::Bool(expected.available))
            );
            assert_eq!(
                actual.field("refreshed"),
                Some(&Value::Time(expected.refreshed))
            );
        }
    };

    let actual_stamp = actual.edit_stamp();
    for ui in [&interpreted as &dyn Component, &generated] {
        assert!(!paste(ui, vec![1, 3])("16\t09:00:00\n17\t25:00:00"));
    }
    assert_eq!(actual.edit_stamp(), actual_stamp);
    compare();
    let stale = paste(&interpreted, vec![1, 3]);
    let expected_stale = paste(&generated, vec![1, 3]);
    interpreted.set("lock-refresh-time", true.into()).unwrap();
    generated.set_lock_refresh_time(true);
    assert!(!stale("16\t09:00:00"));
    assert!(!expected_stale("16\t09:00:00"));
    interpreted.set("lock-refresh-time", false.into()).unwrap();
    generated.set_lock_refresh_time(false);

    let stale = paste(&interpreted, vec![1, 3]);
    let expected_stale = paste(&generated, vec![1, 3]);
    interpreted
        .invoke("move-visible-column", &[Value::Int(0), Value::Int(2)])
        .unwrap();
    generated.invoke_move_visible_column(0, 2);
    assert!(!stale("16\t09:00:00"));
    assert!(!expected_stale("16\t09:00:00"));
    let stale = paste(&interpreted, vec![1, 3]);
    let expected_stale = paste(&generated, vec![1, 3]);
    let external_name = "External row";
    let row = actual
        .row(2)
        .unwrap()
        .with_field(&["name".into()], Value::String(external_name.into()))
        .unwrap();
    assert!(actual.set_row(2, row));
    let mut row = expected.row(2).unwrap();
    row.name = external_name.into();
    assert!(expected.set_row(2, row));
    assert!(!stale("16\t09:00:00"));
    assert!(!expected_stale("16\t09:00:00"));

    assert!(paste(&interpreted, vec![1, 3])(
        "16\t09:00:00\n22\t12:00:00"
    ));
    assert!(paste(&generated, vec![1, 3])("16\t09:00:00\n22\t12:00:00"));
    compare();
    assert_eq!(
        actual.row(1).unwrap().field("refreshed"),
        Some(&Value::Time(Time::new(12, 0, 0).unwrap()))
    );
    let input = Interaction::default();
    let size = Size::new(620.0, 380.0);
    let (scene, _) = prepare_list(&interpreted, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    let cell = [0, 1, 0, 0, 1];
    assert!(interpreted.commit_table_cell(&scene, &cell, "16", "44".into()));
    assert!(generated.commit_table_cell(&generated_scene, &cell, "16", "44".into()));
    compare();

    let external_name = "External edit";
    let row = actual
        .row(0)
        .unwrap()
        .with_field(&["name".into()], Value::String(external_name.into()))
        .unwrap();
    assert!(actual.set_row(0, row));
    let mut row = expected.row(0).unwrap();
    row.name = external_name.into();
    assert!(expected.set_row(0, row));
    for units in [16, 0] {
        assert!(interpreted.dispatch("undo-edit", "clicked"));
        assert!(generated.dispatch("undo-edit", "clicked"));
        compare();
        assert_eq!(
            actual.row(0).unwrap().field("units"),
            Some(&Value::Int(units))
        );
        assert_eq!(
            actual.row(0).unwrap().field("name"),
            Some(&Value::String(external_name.into()))
        );
    }
    for units in [16, 44] {
        assert!(interpreted.replay_table_edit_at(&interpreted.render_shared(), &path, true));
        assert!(generated.replay_table_edit_at(&generated.render_shared(), &path, true));
        compare();
        assert_eq!(
            actual.row(0).unwrap().field("units"),
            Some(&Value::Int(units))
        );
    }
}

#[test]
fn programmatic_table_edit_follows_scroll_and_resolves_tab_target() {
    use table_ui::{InventoryRow, StandardTableDemo};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{FocusRequests, RecordList, RecordValue, ScrollRequests};

    let interpreted = interpret_table_fixture("StandardTableDemo");
    let mut generated = StandardTableDemo::new();
    interpreted.set("edit-columns", true.into()).unwrap();
    generated.set_edit_columns(true);
    let rows: Vec<_> = (0..64)
        .map(|index| InventoryRow {
            name: format!("row {index}"),
            units: index,
            ..InventoryRow::default()
        })
        .collect();
    let records = RecordList::new(
        "InventoryRow",
        rows.iter()
            .map(|row| {
                RecordValue::new(
                    "InventoryRow",
                    [
                        ("name".into(), row.name.clone().into()),
                        ("units".into(), row.units.into()),
                        ("available".into(), row.available.into()),
                        ("refreshed".into(), Value::Time(row.refreshed)),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    interpreted
        .set("inventory", Value::RecordList(records))
        .unwrap();
    generated.set_inventory(rows);
    fn execute(
        component: &impl Component,
        scroll: ScrollRequests,
        focus: FocusRequests,
    ) -> (Vec<usize>, xrack_runtime::ScrollAction) {
        let mut input = Interaction::default();
        let size = Size::new(620.0, 380.0);
        let (scene, layout) = prepare_list(component, &input, size);
        assert!(input.apply_scroll_requests(&scene, &layout, scroll.drain()));
        let (scene, layout) = prepare_list(component, &input, size);
        input.apply_focus_requests(&scene, &layout, focus.drain());
        let target = input.focused.as_ref().unwrap();
        assert!(input.table_cell_editing(&target.path));
        assert_eq!(
            scene.at_path(&target.path).unwrap().text("text"),
            Some("42")
        );
        let origin = input.tab_edit_origin(&scene).unwrap();
        let next = input
            .next_table_edit_target(component, &scene, &layout, &origin, false)
            .unwrap();
        (target.path.clone(), next.scroll_request().action)
    }
    let make_requests = |start: &mut dyn FnMut()| {
        let scroll = ScrollRequests::default();
        let focus = FocusRequests::default();
        {
            let _scroll = scroll.enter();
            let _focus = focus.enter();
            start();
        }
        (scroll, focus)
    };
    let (scroll, focus) = make_requests(&mut || {
        interpreted
            .invoke("edit-inventory-cell", &[Value::Int(42), Value::Int(1)])
            .unwrap();
    });
    let actual = execute(&interpreted, scroll, focus);
    let (scroll, focus) = make_requests(&mut || generated.invoke_edit_inventory_cell(42, 1));
    assert_eq!(actual, execute(&generated, scroll, focus));
}

#[test]
fn custom_table_typed_paste_matches_compiled_validation_and_replay() {
    use records_ui::RecordTableDemo;

    let source = include_str!("../examples/table/records.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "records.xr", Default::default()).unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RecordTableDemo")
        .unwrap();
    let generated = RecordTableDemo::new();
    let Value::RecordList(actual) = interpreted.get("records").unwrap() else {
        panic!("expected record list");
    };
    let expected = generated.model_records();
    let path = [0, 1];
    let paste = |ui: &dyn Component| {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, vec![0, 1, 2, 3, 4])
            .unwrap()
    };
    let valid = "Router\tOperations\t42\t0\t0.75\nSwitch\tTeam\t14\t1\t0.5";
    let invalid = "Router\tOperations\t42\t0\t0.75\nSwitch\tTeam\t9999\t1\t0.5";
    for ui in [&interpreted as &dyn Component, &generated] {
        assert!(!paste(ui)(invalid));
        assert!(paste(ui)(valid));
    }
    let compare = || {
        for (actual, expected) in actual.model().snapshot().iter().zip(expected.snapshot()) {
            assert_eq!(actual.field("name"), Some(&Value::String(expected.name)));
            assert_eq!(actual.field("owner"), Some(&Value::String(expected.owner)));
            assert_eq!(actual.field("units"), Some(&Value::Int(expected.units)));
            assert_eq!(
                actual.field("available"),
                Some(&Value::Bool(expected.available))
            );
            assert_eq!(actual.field("load"), Some(&Value::Float(expected.load)));
        }
    };
    compare();
    assert_eq!(
        actual.model().row(1).unwrap().field("units"),
        Some(&Value::Int(14))
    );
    for redo in [false, true] {
        assert!(interpreted.replay_table_edit_at(&interpreted.render_shared(), &path, redo));
        assert!(generated.replay_table_edit_at(&generated.render_shared(), &path, redo));
        compare();
    }
}

fn interpret_paste_bounds_fixture() -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create("PasteBoundsParity")
        .unwrap()
}

#[test]
fn numeric_table_paste_validates_text_before_evaluating_bounds() {
    use std::{cell::Cell, rc::Rc};

    let actual = interpret_paste_bounds_fixture();
    let mut generated = expressions_ui::PasteBoundsParity::new();
    let actual_int = Rc::new(Cell::new(0));
    let actual_float = Rc::new(Cell::new(0));
    let generated_int = Rc::new(Cell::new(0));
    let generated_float = Rc::new(Cell::new(0));
    actual
        .on("integer-minimum", {
            let calls = actual_int.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(0.into()))
            })
        })
        .unwrap();
    actual
        .on("decimal-minimum", {
            let calls = actual_float.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(0.0.into()))
            })
        })
        .unwrap();
    generated.on_integer_minimum({
        let calls = generated_int.clone();
        move |_| {
            calls.set(calls.get() + 1);
            0
        }
    });
    generated.on_decimal_minimum({
        let calls = generated_float.clone();
        move |_| {
            calls.set(calls.get() + 1);
            0.0
        }
    });
    let apply = |ui: &dyn Component, columns: Vec<usize>, text: &str| {
        ui.prepare_table_paste(&ui.render_shared(), &[0], 0, columns)
            .unwrap()(text)
    };
    for ui in [&actual as &dyn Component, &generated] {
        assert!(!apply(ui, vec![0], "not a number"));
        assert!(!apply(ui, vec![1], "NaN"));
        assert!(!apply(ui, vec![1], "inf"));
    }
    assert_eq!((generated_int.get(), actual_int.get()), (0, 0));
    assert_eq!((generated_float.get(), actual_float.get()), (0, 0));

    for ui in [&actual as &dyn Component, &generated] {
        assert!(!apply(ui, vec![1], "1.25"));
        assert!(apply(ui, vec![0, 1], "4\t0.75"));
    }
    assert_eq!((generated_int.get(), actual_int.get()), (1, 1));
    assert_eq!((generated_float.get(), actual_float.get()), (2, 2));
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected table rows");
    };
    let compare = || {
        let row = rows.model().row(0).unwrap();
        let expected = generated.model_rows().row(0).unwrap();
        assert_eq!(row.field("count"), Some(&Value::Int(expected.count)));
        assert_eq!(row.field("load"), Some(&Value::Float(expected.load)));
    };
    compare();
    assert!(actual.replay_table_edit_at(&actual.render_shared(), &[0], false));
    assert!(generated.replay_table_edit_at(&generated.render_shared(), &[0], false));
    compare();
    assert_eq!(actual.take_error(), None);
}

#[test]
fn projected_cell_commits_check_the_model_before_and_after_column_permission() {
    use expressions_ui::ProjectedPasteParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{RecordList, VecModel};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ProjectedPasteParity")
        .unwrap();
    let mut generated = ProjectedPasteParity::new();
    let read_rows = |ui: &Instance| {
        let Value::RecordList(rows) = ui.get("rows").unwrap() else {
            panic!("expected rows");
        };
        rows
    };
    let input = Interaction::default();
    let size = Size::new(430.0, 280.0);
    let path = [0, 0, 0, 1];
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_eq!(actual.at_path(&path).unwrap().text("text"), Some("two"));

    let actual_calls = Rc::new(Cell::new(0));
    let expected_calls = Rc::new(Cell::new(0));
    interpreted
        .on("allow-second", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_allow_second({
        let calls = expected_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            true
        }
    });
    interpreted
        .set(
            "rows",
            Value::RecordList(RecordList::new(
                "ProjectedPasteRow",
                read_rows(&interpreted).model().snapshot(),
            )),
        )
        .unwrap();
    generated.bind_model_rows(VecModel::from(generated.get_rows()));
    assert!(!interpreted.commit_table_cell(&actual, &path, "two", "stale".into()));
    assert!(!generated.commit_table_cell(&expected, &path, "two", "stale".into()));
    let stale_checks = (actual_calls.get(), expected_calls.get());

    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let previous = read_rows(&interpreted);
    let compiled_previous = generated.model_rows();
    interpreted
        .on("allow-second", {
            let replacement = RecordList::new("ProjectedPasteRow", previous.model().snapshot());
            let owner = Cell::new(Some(interpreted.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("rows", Value::RecordList(replacement.clone()))
                        .unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_allow_second({
        let replacement = VecModel::from(compiled_previous.snapshot());
        let owner = generated.as_weak();
        move || {
            owner
                .upgrade()
                .unwrap()
                .bind_model_rows(replacement.clone());
            true
        }
    });
    assert_eq!(
        (
            stale_checks,
            [
                interpreted.commit_table_cell(&actual, &path, "two", "detached edit".into()),
                generated.commit_table_cell(&expected, &path, "two", "detached edit".into()),
            ]
        ),
        ((0, 0), [false, false])
    );
    assert_eq!(
        previous.model().row(0).unwrap().field("second"),
        Some(&Value::String("two".into()))
    );
    assert_eq!(compiled_previous.row(0).unwrap().second, "two");
    assert_eq!(
        read_rows(&interpreted)
            .model()
            .row(0)
            .unwrap()
            .field("second"),
        Some(&Value::String("two".into()))
    );
    assert_eq!(generated.model_rows().row(0).unwrap().second, "two");
    assert!(!interpreted.replay_table_edit_at(&interpreted.render_shared(), &[0], false));
    assert!(!generated.replay_table_edit_at(&generated.render_shared(), &[0], false));

    interpreted
        .on("allow-second", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    generated.on_allow_second(|| true);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert!(interpreted.commit_table_cell(&actual, &path, "two", "applied".into()));
    assert!(generated.commit_table_cell(&expected, &path, "two", "applied".into()));
    assert_eq!(
        read_rows(&interpreted)
            .model()
            .row(0)
            .unwrap()
            .field("second"),
        Some(&Value::String("applied".into()))
    );
    assert_eq!(generated.model_rows().row(0).unwrap().second, "applied");
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn custom_table_history_callback_cannot_edit_a_replaced_model() {
    use expressions_ui::HistoryCallbackParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{RecordList, VecModel};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let read_rows = |ui: &Instance| {
        let Value::RecordList(rows) = ui.get("rows").unwrap() else {
            panic!("expected rows");
        };
        rows
    };
    for linked in [false, true] {
        let mut actual = program.create("HistoryCallbackParity").unwrap();
        let mut generated = HistoryCallbackParity::new();
        let input = Interaction::default();
        let size = Size::new(420.0, 240.0);
        let (actual_scene, _) = prepare_list(&actual, &input, size);
        let (generated_scene, _) = prepare_list(&generated, &input, size);
        let previous = read_rows(&actual);
        let compiled_previous = generated.model_rows();
        let replacement = RecordList::new("HistoryCallbackRow", previous.model().snapshot());
        let compiled_replacement = VecModel::from(compiled_previous.snapshot());
        let actual_calls = Rc::new(Cell::new(0));
        let generated_calls = Rc::new(Cell::new(0));
        actual
            .on("record-history", {
                let owner = Cell::new(Some(actual.clone()));
                let replacement = replacement.clone();
                let calls = actual_calls.clone();
                Rc::new(move |_| {
                    calls.set(calls.get() + 1);
                    if let Some(owner) = owner.take() {
                        owner
                            .set("rows", Value::RecordList(replacement.clone()))
                            .unwrap();
                    }
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        generated.on_record_history({
            let owner = generated.as_weak();
            let replacement = compiled_replacement.clone();
            let calls = generated_calls.clone();
            let pending = Cell::new(true);
            move || {
                calls.set(calls.get() + 1);
                if pending.replace(false) {
                    owner
                        .upgrade()
                        .unwrap()
                        .bind_model_rows(replacement.clone());
                }
                true
            }
        });

        let accepted = if linked {
            let path = [0, 0, 0, 1, 0];
            [
                actual.edit_at(&actual_scene, &path, "edited".into()),
                generated.edit_at(&generated_scene, &path, "edited".into()),
            ]
        } else {
            let path = [0, 0, 0, 0];
            [
                actual.commit_table_cell(&actual_scene, &path, "one", "edited".into()),
                generated.commit_table_cell(&generated_scene, &path, "one", "edited".into()),
            ]
        };
        assert_eq!((actual_calls.get(), generated_calls.get()), (1, 1));
        assert_eq!(accepted, [linked; 2]);
        assert_eq!(read_rows(&actual), replacement);
        assert_eq!(generated.model_rows(), compiled_replacement);
        for rows in [&previous, &replacement] {
            let row = rows.model().row(0).unwrap();
            assert_eq!(row.field("direct"), Some(&Value::String("one".into())));
            assert_eq!(row.field("linked"), Some(&Value::String("two".into())));
        }
        for model in [&compiled_previous, &compiled_replacement] {
            let row = model.row(0).unwrap();
            assert_eq!(row.direct, "one");
            assert_eq!(row.linked, "two");
        }
        assert!(!actual.replay_table_edit_at(&actual.render_shared(), &[0], false));
        assert!(!generated.replay_table_edit_at(&generated.render_shared(), &[0], false));
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn linked_table_edit_preserves_field_changes_made_by_history_binding() {
    use std::rc::Rc;
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let size = Size::new(420.0, 240.0);
    let input = Interaction::default();
    let path = [0, 0, 0, 1, 0];
    for changed_field in ["linked", "direct"] {
        let mut interpreted = program.create("HistoryCallbackParity").unwrap();
        let mut compiled = expressions_ui::HistoryCallbackParity::new();
        let (scene, _) = prepare_list(&interpreted, &input, size);
        let (expected, _) = prepare_list(&compiled, &input, size);
        let actual_rows = interpreted.record_model("rows").unwrap();
        let generated_rows = compiled.model_rows();
        interpreted
            .on("record-history", {
                let model = actual_rows.model().clone();
                Rc::new(move |_| {
                    let row = model
                        .row(0)
                        .unwrap()
                        .with_field(&[changed_field.into()], "callback".to_owned().into())
                        .unwrap();
                    assert!(model.set_row(0, row));
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        compiled.on_record_history({
            let model = generated_rows.clone();
            move || {
                let mut row = model.row(0).unwrap();
                if changed_field == "linked" {
                    row.linked = "callback".into();
                } else {
                    row.direct = "callback".into();
                }
                assert!(model.set_row(0, row));
                true
            }
        });
        interpreted.edit_at(&scene, &path, "user".into());
        compiled.edit_at(&expected, &path, "user".into());
        let row = actual_rows.model().row(0).unwrap();
        let compiled_row = generated_rows.row(0).unwrap();
        let linked = if changed_field == "linked" {
            "callback"
        } else {
            "user"
        };
        assert_eq!(row.field("linked"), Some(&Value::String(linked.into())));
        assert_eq!(compiled_row.linked, linked);
        let direct = if changed_field == "direct" {
            "callback"
        } else {
            "one"
        };
        assert_eq!(row.field("direct"), Some(&Value::String(direct.into())));
        assert_eq!(compiled_row.direct, direct);
        interpreted
            .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
            .unwrap();
        compiled.on_record_history(|| true);
        let recorded = changed_field == "direct";
        assert_eq!(
            interpreted.replay_table_edit_at(&scene, &[0], false),
            recorded
        );
        assert_eq!(
            compiled.replay_table_edit_at(&expected, &[0], false),
            recorded
        );
        if recorded {
            let row = actual_rows.model().row(0).unwrap();
            let compiled_row = generated_rows.row(0).unwrap();
            assert_eq!(row.field("direct"), Some(&Value::String("callback".into())));
            assert_eq!(row.field("linked"), Some(&Value::String("two".into())));
            assert_eq!(
                (compiled_row.direct.as_str(), compiled_row.linked.as_str()),
                ("callback", "two")
            );
        }
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn programmatic_undo_stays_with_the_model_present_when_invoked() {
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{RecordList, VecModel};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("HistoryCallbackParity")
        .unwrap();
    let mut compiled = expressions_ui::HistoryCallbackParity::new();
    let input = Interaction::default();
    let size = Size::new(420.0, 240.0);
    let (scene, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&compiled, &input, size);
    let cell = [0, 0, 0, 0];
    assert!(interpreted.commit_table_cell(&scene, &cell, "one", "edited".into()));
    assert!(compiled.commit_table_cell(&expected, &cell, "one", "edited".into()));

    let edited = interpreted.record_model("rows").unwrap();
    let compiled_edited = compiled.model_rows();
    let other = RecordList::new("HistoryCallbackRow", edited.model().snapshot());
    let compiled_other = VecModel::from(compiled_edited.snapshot());
    interpreted
        .set("rows", Value::RecordList(other.clone()))
        .unwrap();
    compiled.bind_model_rows(compiled_other.clone());
    let samples = Rc::new(Cell::new(0));
    let compiled_samples = Rc::new(Cell::new(0));
    interpreted
        .on("record-history", {
            let weak = interpreted.as_weak();
            let edited = edited.clone();
            let samples = samples.clone();
            Rc::new(move |_| {
                if samples.replace(samples.get() + 1) == 0 {
                    weak.upgrade()
                        .unwrap()
                        .set("rows", Value::RecordList(edited.clone()))
                        .unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    compiled.on_record_history({
        let weak = compiled.as_weak();
        let edited = compiled_edited.clone();
        let samples = compiled_samples.clone();
        move || {
            if samples.replace(samples.get() + 1) == 0 {
                weak.upgrade().unwrap().bind_model_rows(edited.clone());
            }
            true
        }
    });

    interpreted.invoke("undo-edit", &[]).unwrap();
    compiled.invoke_undo_edit();
    assert_eq!((samples.get(), compiled_samples.get()), (1, 1));
    assert_eq!(interpreted.record_model("rows").unwrap(), edited);
    assert_eq!(compiled.model_rows(), compiled_edited);
    let assert_edited = |text: &str| {
        assert_eq!(
            edited.model().row(0).unwrap().field("direct"),
            Some(&Value::String(text.into()))
        );
        assert_eq!(compiled_edited.row(0).unwrap().direct, text);
    };
    assert_edited("edited");
    interpreted.invoke("undo-edit", &[]).unwrap();
    compiled.invoke_undo_edit();
    assert_edited("one");
    assert_eq!(
        other.model().row(0).unwrap().field("direct"),
        Some(&Value::String("edited".into()))
    );
    assert_eq!(compiled_other.row(0).unwrap().direct, "edited");
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_history_replay_keeps_the_displayed_model_and_live_availability() {
    use expressions_ui::HistoryCallbackParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::RecordList;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("HistoryCallbackParity")
        .unwrap();
    let mut generated = HistoryCallbackParity::new();
    let input = Interaction::default();
    let size = Size::new(420.0, 240.0);
    let table = [0];
    let cell = [0, 0, 0, 0];
    let (scene, _) = prepare_list(&actual, &input, size);
    let (compiled_scene, _) = prepare_list(&generated, &input, size);
    assert!(actual.commit_table_cell(&scene, &cell, "one", "edited".into()));
    assert!(generated.commit_table_cell(&compiled_scene, &cell, "one", "edited".into()));
    let read_rows = |ui: &Instance| match ui.get("rows").unwrap() {
        Value::RecordList(rows) => rows,
        _ => panic!("expected a table model"),
    };
    let old_model = read_rows(&actual);
    let compiled_old = generated.model_rows();
    let assert_text = |text: &str| {
        assert_eq!(
            old_model.model().row(0).unwrap().field("direct"),
            Some(&Value::String(text.into()))
        );
        assert_eq!(compiled_old.row(0).unwrap().direct, text);
    };

    actual.set("table-enabled", false.into()).unwrap();
    generated.set_table_enabled(false);
    assert!(!actual.replay_table_edit_at(&scene, &table, false));
    assert!(!generated.replay_table_edit_at(&compiled_scene, &table, false));
    assert_text("edited");
    actual.set("table-enabled", true.into()).unwrap();
    generated.set_table_enabled(true);

    let actual_calls = Rc::new(Cell::new(0));
    let compiled_calls = Rc::new(Cell::new(0));
    actual
        .on("record-history", {
            let owner = Cell::new(Some(actual.clone()));
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                if let Some(owner) = owner.take() {
                    owner.set("table-visible", false.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_record_history({
        let owner = generated.as_weak();
        let calls = compiled_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            owner.upgrade().unwrap().set_table_visible(false);
            true
        }
    });
    assert!(!actual.replay_table_edit_at(&scene, &table, false));
    assert!(!generated.replay_table_edit_at(&compiled_scene, &table, false));
    assert_eq!((actual_calls.get(), compiled_calls.get()), (1, 1));
    assert_text("edited");

    actual.set("table-visible", true.into()).unwrap();
    generated.set_table_visible(true);
    actual
        .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    generated.on_record_history(|| true);
    old_model.model().push(old_model.model().row(0).unwrap());
    compiled_old.push(compiled_old.row(0).unwrap());
    assert!(actual.replay_table_edit_at(&scene, &table, false));
    assert!(generated.replay_table_edit_at(&compiled_scene, &table, false));
    assert_text("one");

    actual
        .set(
            "rows",
            Value::RecordList(RecordList::new(
                "HistoryCallbackRow",
                old_model.model().snapshot(),
            )),
        )
        .unwrap();
    generated.set_rows(compiled_old.snapshot());
    let (new_scene, _) = prepare_list(&actual, &input, size);
    let (compiled_new_scene, _) = prepare_list(&generated, &input, size);
    assert!(actual.commit_table_cell(&new_scene, &cell, "one", "new".into()));
    assert!(generated.commit_table_cell(&compiled_new_scene, &cell, "one", "new".into()));
    assert!(!actual.replay_table_edit_at(&scene, &table, false));
    assert!(!generated.replay_table_edit_at(&compiled_scene, &table, false));
    assert!(actual.replay_table_edit_at(&new_scene, &table, false));
    assert!(generated.replay_table_edit_at(&compiled_new_scene, &table, false));
    assert!(actual.replay_table_edit_at(&new_scene, &table, true));
    assert!(generated.replay_table_edit_at(&compiled_new_scene, &table, true));
    assert_eq!(
        read_rows(&actual).model().row(0).unwrap().field("direct"),
        Some(&Value::String("new".into()))
    );
    assert_eq!(generated.get_rows()[0].direct, "new");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn custom_table_edit_reuses_rendered_binding_cache() {
    use expressions_ui::TableBindingCacheParity;
    use std::{cell::RefCell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableBindingCacheParity")
        .unwrap();
    let mut generated = TableBindingCacheParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let compiled_calls = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("sample", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(kind), Value::Bool(value)] = args else {
                    panic!("expected binding arguments");
                };
                calls.borrow_mut().push(*kind);
                Ok(Some((*value).into()))
            })
        })
        .unwrap();
    generated.on_sample({
        let calls = compiled_calls.clone();
        move |kind, value| {
            calls.borrow_mut().push(kind);
            value
        }
    });

    let input = Interaction::default();
    let size = Size::new(420.0, 240.0);
    let (scene, _) = prepare_list(&actual, &input, size);
    let (compiled_scene, _) = prepare_list(&generated, &input, size);
    assert_same_scene(&scene, &compiled_scene);
    let assert_calls = |expected: &[i64]| {
        assert_eq!(actual_calls.take(), expected, "interpreter");
        assert_eq!(compiled_calls.take(), expected, "generated Rust");
    };
    actual_calls.take();
    compiled_calls.take();

    let direct = [0, 0, 0, 0];
    let linked = [0, 0, 0, 1, 0];
    for (ui, old_scene) in [
        (&mut actual as &mut dyn Component, &*scene),
        (&mut generated, &*compiled_scene),
    ] {
        assert!(ui.commit_table_cell(old_scene, &direct, "one", "edited".into()));
        assert!(ui.edit_at(old_scene, &linked, "linked".into()));
    }
    assert_calls(&[]);

    let (scene, _) = prepare_list(&actual, &input, size);
    let (compiled_scene, _) = prepare_list(&generated, &input, size);
    assert_same_scene(&scene, &compiled_scene);
    assert_eq!(scene.at_path(&direct).unwrap().text("text"), Some("edited"));
    assert_eq!(scene.at_path(&linked).unwrap().text("text"), Some("linked"));
    assert_calls(&[]);

    actual.set("table-enabled", false.into()).unwrap();
    generated.set_table_enabled(false);
    assert!(!actual.commit_table_cell(&scene, &direct, "edited", "blocked".into()));
    assert!(!generated.commit_table_cell(&compiled_scene, &direct, "edited", "blocked".into()));
    assert_calls(&[2]);

    actual.set("table-enabled", true.into()).unwrap();
    generated.set_table_enabled(true);
    actual.set("mounted", false.into()).unwrap();
    generated.set_mounted(false);
    assert!(!actual.commit_table_cell(&scene, &direct, "edited", "blocked".into()));
    assert!(!generated.commit_table_cell(&compiled_scene, &direct, "edited", "blocked".into()));
    assert_calls(&[2, 1]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn custom_table_rejects_edits_if_it_becomes_disabled_or_hidden() {
    use expressions_ui::{HistoryCallbackParity, PlainTableAvailability};
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let input = Interaction::default();
    let size = Size::new(420.0, 240.0);
    let direct = [0, 0, 0, 0];
    let linked = [0, 0, 0, 1, 0];
    let check = |ui: &mut dyn Component, scene: &Node| {
        assert!(!ui.commit_table_cell(scene, &direct, "one", "edited".into()));
        assert!(!ui.edit_at(scene, &linked, "edited".into()));
    };
    let read_rows = |ui: &Instance| match ui.get("rows").unwrap() {
        Value::RecordList(rows) => rows.model().snapshot(),
        _ => panic!("expected rows"),
    };

    let mut actual = program.create("PlainTableAvailability").unwrap();
    let mut generated = PlainTableAvailability::new();
    let original = read_rows(&actual);
    let compiled_original = generated.get_rows();
    let (scene, _) = prepare_list(&actual, &input, size);
    let (compiled_scene, _) = prepare_list(&generated, &input, size);
    actual.set("table-enabled", false.into()).unwrap();
    generated.set_table_enabled(false);
    check(&mut actual, &scene);
    check(&mut generated, &compiled_scene);
    assert_eq!(read_rows(&actual), original);
    assert_eq!(generated.get_rows(), compiled_original);

    for path in [&direct[..], &linked[..]] {
        let mut actual = program.create("HistoryCallbackParity").unwrap();
        let mut generated = HistoryCallbackParity::new();
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        let actual_calls = Rc::new(Cell::new(0));
        let generated_calls = Rc::new(Cell::new(0));
        actual
            .on("record-history", {
                let calls = actual_calls.clone();
                let owner = Cell::new(Some(actual.clone()));
                Rc::new(move |_| {
                    calls.set(calls.get() + 1);
                    if let Some(owner) = owner.take() {
                        owner.set("table-visible", false.into()).unwrap();
                    }
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        generated.on_record_history({
            let calls = generated_calls.clone();
            let owner = generated.as_weak();
            move || {
                calls.set(calls.get() + 1);
                owner.upgrade().unwrap().set_table_visible(false);
                true
            }
        });

        actual.set("table-visible", false.into()).unwrap();
        generated.set_table_visible(false);
        check(&mut actual, &scene);
        check(&mut generated, &compiled_scene);
        assert_eq!((actual_calls.get(), generated_calls.get()), (0, 0));
        actual.set("table-visible", true.into()).unwrap();
        generated.set_table_visible(true);
        for (ui, old_scene) in [
            (&mut actual as &mut dyn Component, &*scene),
            (&mut generated, &*compiled_scene),
        ] {
            if path == direct {
                assert!(!ui.commit_table_cell(old_scene, path, "one", "edited".into()));
            } else {
                assert!(ui.edit_at(old_scene, path, "edited".into()));
            }
        }
        assert_eq!((actual_calls.get(), generated_calls.get()), (1, 1));
        assert_eq!(actual.get("table-visible"), Ok(false.into()));
        assert!(!generated.get_table_visible());
        assert_eq!(read_rows(&actual), original);
        assert_eq!(generated.get_rows(), compiled_original);
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn table_cell_commit_rechecks_parent_scope_through_edit_preparation() {
    use expressions_ui::RoutedCellCommitParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    for (column, original, callback) in [(0, "one", "record-history"), (1, "two", "allow-column")] {
        let mut actual = program.create("RoutedCellCommitParity").unwrap();
        let mut expected = RoutedCellCommitParity::new();
        let input = Interaction::default();
        let size = Size::new(640.0, 420.0);
        let (actual_scene, _) = prepare_list(&actual, &input, size);
        let (expected_scene, _) = prepare_list(&expected, &input, size);
        let path = [0, column, 0, 0, 0];
        actual.set("opened", false.into()).unwrap();
        expected.set_opened(false);
        assert!(!actual.commit_table_cell(&actual_scene, &path, original, "closed".into()));
        assert!(!expected.commit_table_cell(&expected_scene, &path, original, "closed".into()));
        actual.set("opened", true.into()).unwrap();
        expected.set_opened(true);

        actual
            .on(callback, {
                let owner = Cell::new(Some(actual.clone()));
                Rc::new(move |_| {
                    if let Some(owner) = owner.take() {
                        owner
                            .set("opened", false.into())
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        let close = {
            let owner = expected.as_weak();
            move || {
                owner.upgrade().unwrap().set_opened(false);
                true
            }
        };
        if column == 0 {
            expected.on_record_history(close);
        } else {
            expected.on_allow_column(close);
        }
        assert!(!actual.commit_table_cell(&actual_scene, &path, original, "invalidated".into()));
        assert!(!expected.commit_table_cell(
            &expected_scene,
            &path,
            original,
            "invalidated".into()
        ));
        assert_eq!(actual.get("opened"), Ok(false.into()));
        assert!(!expected.get_opened());
        let Value::RecordList(rows) = actual.get("rows").unwrap() else {
            panic!("expected records");
        };
        let row = rows.model().row(0).unwrap();
        assert_eq!(row.field("direct"), Some(&Value::String("one".into())));
        assert_eq!(row.field("linked"), Some(&Value::String("two".into())));
        assert_eq!(expected.get_rows()[0].direct, "one");
        assert_eq!(expected.get_rows()[0].linked, "two");
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn text_cell_commit_stops_when_preparation_invalidates_input() {
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let interaction = Interaction::default();
    let size = Size::new(640.0, 420.0);
    for (view, field, original, permission) in [
        (0, "direct", "one", "allow-direct-cell"),
        (1, "linked", "two", "allow-column"),
    ] {
        let mut interpreted = program.create("RoutedCellCommitParity").unwrap();
        let mut compiled = expressions_ui::RoutedCellCommitParity::new();
        let (actual, _) = prepare_list(&interpreted, &interaction, size);
        let (expected, _) = prepare_list(&compiled, &interaction, size);
        let path = [0, view, 0, 0, 0];
        let interpreted_reads = Rc::new(Cell::new(0));
        let compiled_reads = Rc::new(Cell::new(0));
        interpreted
            .on("record-history", {
                let reads = interpreted_reads.clone();
                Rc::new(move |_| {
                    reads.set(reads.get() + 1);
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        compiled.on_record_history({
            let reads = compiled_reads.clone();
            move || {
                reads.set(reads.get() + 1);
                true
            }
        });
        interpreted
            .on(permission, {
                let weak = interpreted.as_weak();
                Rc::new(move |_| {
                    let property = if view == 0 {
                        "direct-row-visible"
                    } else {
                        "opened"
                    };
                    weak.upgrade().unwrap().set(property, false.into()).unwrap();
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        let invalidate = {
            let weak = compiled.as_weak();
            move || {
                let mut ui = weak.upgrade().unwrap();
                if view == 0 {
                    ui.set_direct_row_visible(false);
                } else {
                    ui.set_opened(false);
                }
                true
            }
        };
        if view == 0 {
            compiled.on_allow_direct_cell(invalidate);
        } else {
            compiled.on_allow_column(invalidate);
        }
        assert!(!interpreted.commit_table_cell(&actual, &path, original, "discard".into()));
        assert!(!compiled.commit_table_cell(&expected, &path, original, "discard".into()));
        assert_eq!(
            (interpreted_reads.get(), compiled_reads.get()),
            (0, 0),
            "view {view}"
        );
        let rows = interpreted.record_model("rows").unwrap();
        let actual_text = || rows.model().row(0).unwrap().field(field).cloned();
        let compiled_text = |ui: &expressions_ui::RoutedCellCommitParity| {
            let row = ui.model_rows().row(0).unwrap();
            if view == 0 { row.direct } else { row.linked }
        };
        assert_eq!(actual_text(), Some(original.to_owned().into()));
        assert_eq!(compiled_text(&compiled), original);

        interpreted
            .on(permission, Rc::new(|_| Ok(Some(true.into()))))
            .unwrap();
        if view == 0 {
            compiled.on_allow_direct_cell(|| true);
            interpreted.set("direct-row-visible", true.into()).unwrap();
            compiled.set_direct_row_visible(true);
            interpreted
                .on("record-history", {
                    let weak = interpreted.as_weak();
                    Rc::new(move |_| {
                        weak.upgrade()
                            .unwrap()
                            .set("direct-row-visible", false.into())
                            .unwrap();
                        Ok(Some(true.into()))
                    })
                })
                .unwrap();
            compiled.on_record_history({
                let weak = compiled.as_weak();
                move || {
                    weak.upgrade().unwrap().set_direct_row_visible(false);
                    true
                }
            });
            assert!(!interpreted.commit_table_cell(&actual, &path, original, "discard".into()));
            assert!(!compiled.commit_table_cell(&expected, &path, original, "discard".into()));
            assert_eq!(actual_text(), Some(original.to_owned().into()));
            assert_eq!(compiled_text(&compiled), original);
            interpreted.set("direct-row-visible", true.into()).unwrap();
            compiled.set_direct_row_visible(true);
        } else {
            compiled.on_allow_column(|| true);
            interpreted.set("opened", true.into()).unwrap();
            compiled.set_opened(true);
        }
        interpreted
            .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
            .unwrap();
        compiled.on_record_history(|| true);
        let table = [0, view];
        assert!(!interpreted.replay_table_edit_at(&actual, &table, false));
        assert!(!compiled.replay_table_edit_at(&expected, &table, false));
        assert!(interpreted.commit_table_cell(&actual, &path, original, "saved".into()));
        assert!(compiled.commit_table_cell(&expected, &path, original, "saved".into()));
        assert_eq!(actual_text(), Some("saved".to_owned().into()));
        assert_eq!(compiled_text(&compiled), "saved");
        assert!(interpreted.replay_table_edit_at(&actual, &table, false));
        assert!(compiled.replay_table_edit_at(&expected, &table, false));
        assert_eq!(actual_text(), Some(original.to_owned().into()));
        assert_eq!(compiled_text(&compiled), original);
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn table_paste_and_history_keep_the_live_ancestor_scope() {
    use expressions_ui::RoutedCellCommitParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let mut actual = program.create("RoutedCellCommitParity").unwrap();
    let mut expected = RoutedCellCommitParity::new();
    let size = Size::new(640.0, 420.0);
    let table = [0, 1];
    let capture = |ui: &dyn Component| {
        ui.prepare_table_paste(&ui.render_shared(), &table, 0, vec![0])
            .unwrap()
    };
    let paste = capture(&actual);
    let generated_paste = capture(&expected);
    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    assert!(!paste("closed"));
    assert!(!generated_paste("closed"));
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);

    let paste = capture(&actual);
    let generated_paste = capture(&expected);
    let close = |owner: Instance| {
        let owner = Cell::new(Some(owner));
        Rc::new(move |_: &[Value]| {
            if let Some(owner) = owner.take() {
                owner
                    .set("opened", false.into())
                    .map_err(|error| error.to_string())?;
            }
            Ok(Some(true.into()))
        })
    };
    actual.on("allow-column", close(actual.clone())).unwrap();
    expected.on_allow_column({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_opened(false);
            true
        }
    });
    assert!(!paste("invalidated"));
    assert!(!generated_paste("invalidated"));
    assert_eq!(actual.get("opened"), Ok(false.into()));
    assert!(!expected.get_opened());
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected records");
    };
    assert_eq!(
        rows.model().row(0).unwrap().field("linked"),
        Some(&Value::String("two".into()))
    );
    assert_eq!(expected.get_rows()[0].linked, "two");
    actual
        .on("allow-column", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    expected.on_allow_column(|| true);
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    assert!(capture(&actual)("new"));
    assert!(capture(&expected)("new"));

    let input = Interaction::default();
    let (scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&expected, &input, size);
    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    assert!(!actual.replay_table_edit_at(&scene, &table, false));
    assert!(!expected.replay_table_edit_at(&generated_scene, &table, false));
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    actual.on("record-history", close(actual.clone())).unwrap();
    expected.on_record_history({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_opened(false);
            true
        }
    });
    assert!(!actual.replay_table_edit_at(&scene, &table, false));
    assert!(!expected.replay_table_edit_at(&generated_scene, &table, false));
    assert_eq!(actual.get("opened"), Ok(false.into()));
    assert!(!expected.get_opened());
    assert_eq!(
        rows.model().row(0).unwrap().field("linked"),
        Some(&Value::String("new".into()))
    );
    assert_eq!(expected.get_rows()[0].linked, "new");
    actual
        .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    expected.on_record_history(|| true);
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    assert!(actual.replay_table_edit_at(&scene, &table, false));
    assert!(expected.replay_table_edit_at(&generated_scene, &table, false));
    assert_eq!(
        rows.model().row(0).unwrap().field("linked"),
        Some(&Value::String("two".into()))
    );
    assert_eq!(expected.get_rows()[0].linked, "two");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_copy_and_tab_stop_when_the_parent_overlay_closes() {
    use expressions_ui::RoutedCellCommitParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::TableEditPosition;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    for navigation in [false, true] {
        let actual = program.create("RoutedCellCommitParity").unwrap();
        let mut expected = RoutedCellCommitParity::new();
        let actual_scene = actual.render_shared();
        let expected_scene = expected.render_shared();
        let Value::RecordList(rows) = actual.get("rows").unwrap() else {
            panic!("expected records");
        };
        let origin = |key| TableEditPosition {
            row: 0,
            column: 0,
            key,
        };
        let actual_origin = origin(rows.model().row_key(0).unwrap());
        let expected_origin = origin(expected.model_rows().row_key(0).unwrap());
        let query = |ui: &dyn Component, scene: &Node, origin| {
            if navigation {
                ui.next_table_edit_cell(scene, &[0, 1], origin, &[0, 1], false)
                    .map(|target| target.column.to_string())
            } else {
                ui.copy_table_at(scene, &[0, 0], &[0])
            }
        };
        actual.set("opened", false.into()).unwrap();
        expected.set_opened(false);
        assert_eq!(query(&actual, &actual_scene, actual_origin), None);
        assert_eq!(query(&expected, &expected_scene, expected_origin), None);
        actual.set("opened", true.into()).unwrap();
        expected.set_opened(true);
        let valid = Some(if navigation { "1" } else { "one" }.into());
        assert_eq!(query(&actual, &actual_scene, actual_origin), valid);
        assert_eq!(query(&expected, &expected_scene, expected_origin), valid);

        let callback = if navigation {
            "allow-column"
        } else {
            "copy-value"
        };
        actual
            .on(callback, {
                let owner = Cell::new(Some(actual.clone()));
                Rc::new(move |args| {
                    if let Some(owner) = owner.take() {
                        owner
                            .set("opened", false.into())
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(Some(args.first().cloned().unwrap_or_else(|| true.into())))
                })
            })
            .unwrap();
        let owner = expected.as_weak();
        if navigation {
            expected.on_allow_column(move || {
                owner.upgrade().unwrap().set_opened(false);
                true
            });
        } else {
            expected.on_copy_value(move |value| {
                owner.upgrade().unwrap().set_opened(false);
                value
            });
        }
        assert_eq!(query(&actual, &actual_scene, actual_origin), None);
        assert_eq!(query(&expected, &expected_scene, expected_origin), None);
        assert_eq!(actual.get("opened"), Ok(false.into()));
        assert!(!expected.get_opened());
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn projected_cell_commit_rechecks_table_enabled_after_column_callback() {
    use expressions_ui::ProjectedPasteParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ProjectedPasteParity")
        .unwrap();
    let mut generated = ProjectedPasteParity::new();
    let input = Interaction::default();
    let size = Size::new(430.0, 280.0);
    let path = [0, 0, 0, 1];
    let (actual_scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    let actual_calls = Rc::new(Cell::new(0));
    let generated_calls = Rc::new(Cell::new(0));
    actual
        .on("allow-second", {
            let calls = actual_calls.clone();
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                if let Some(owner) = owner.take() {
                    owner.set("table-enabled", false.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_allow_second({
        let calls = generated_calls.clone();
        let owner = generated.as_weak();
        move || {
            calls.set(calls.get() + 1);
            owner.upgrade().unwrap().set_table_enabled(false);
            true
        }
    });

    actual.set("table-enabled", false.into()).unwrap();
    generated.set_table_enabled(false);
    assert!(!actual.commit_table_cell(&actual_scene, &path, "two", "disabled".into()));
    assert!(!generated.commit_table_cell(&generated_scene, &path, "two", "disabled".into()));
    assert_eq!((actual_calls.get(), generated_calls.get()), (0, 0));

    actual.set("table-enabled", true.into()).unwrap();
    generated.set_table_enabled(true);
    assert!(!actual.commit_table_cell(&actual_scene, &path, "two", "disabled during edit".into()));
    assert!(!generated.commit_table_cell(
        &generated_scene,
        &path,
        "two",
        "disabled during edit".into()
    ));
    assert_eq!((actual_calls.get(), generated_calls.get()), (1, 1));
    assert_eq!(actual.get("table-enabled"), Ok(false.into()));
    assert!(!generated.get_table_enabled());
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    assert_eq!(
        rows.model().row(0).unwrap().field("second"),
        Some(&Value::String("two".into()))
    );
    assert_eq!(generated.model_rows().row(0).unwrap().second, "two");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn projected_table_paste_evaluates_only_columns_with_parsed_cells() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ProjectedPasteParity")
        .unwrap();
    let mut generated = expressions_ui::ProjectedPasteParity::new();
    let scene = actual.render_shared();
    let expected_scene = generated.render_shared();
    let actual_checks = Rc::new(Cell::new(0));
    let expected_checks = Rc::new(Cell::new(0));
    actual
        .on("allow-second", {
            let checks = actual_checks.clone();
            Rc::new(move |_| {
                checks.set(checks.get() + 1);
                Err("second-column guard failed".into())
            })
        })
        .unwrap();
    generated.on_allow_second({
        let checks = expected_checks.clone();
        move || {
            checks.set(checks.get() + 1);
            panic!("second-column guard failed")
        }
    });
    let paste = |ui: &dyn Component, scene: &Node, column, text: &str| {
        ui.prepare_table_paste(scene, &[0], 0, vec![column])
            .unwrap()(text)
    };
    assert_eq!(
        [
            paste(&generated, &expected_scene, 0, "updated"),
            paste(&actual, &scene, 0, "updated"),
        ],
        [true, true]
    );
    assert!(!paste(&actual, &scene, 1, "\"unclosed"));
    assert!(!paste(&generated, &expected_scene, 1, "\"unclosed"));
    assert_eq!((actual_checks.get(), expected_checks.get()), (0, 0));
    assert_eq!(actual.take_error(), None);

    assert!(
        catch_unwind(AssertUnwindSafe(|| paste(
            &generated,
            &expected_scene,
            1,
            "rejected"
        )))
        .is_err()
    );
    assert!(!paste(&actual, &scene, 1, "rejected"));
    assert_eq!((actual_checks.get(), expected_checks.get()), (1, 1));
    assert_eq!(
        actual.take_error(),
        Some(Error::Evaluation("second-column guard failed".into()))
    );
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    assert_eq!(
        rows.model().row(0).unwrap().field("first"),
        Some(&"updated".to_owned().into())
    );
    assert_eq!(
        rows.model().row(0).unwrap().field("second"),
        Some(&"two".to_owned().into())
    );
    assert!(actual.replay_table_edit_at(&scene, &[0], false));
    assert!(generated.replay_table_edit_at(&expected_scene, &[0], false));
    assert!(!actual.replay_table_edit_at(&scene, &[0], false));
    assert!(!generated.replay_table_edit_at(&expected_scene, &[0], false));
    assert_eq!(
        rows.model().row(0).unwrap().field("first"),
        Some(&Value::String(generated.model_rows().row(0).unwrap().first))
    );
}

#[test]
fn table_paste_stops_projection_when_a_callback_changes_the_source_model() {
    use std::{cell::Cell, rc::Rc};

    let actual = interpret_paste_bounds_fixture();
    let mut generated = expressions_ui::PasteBoundsParity::new();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let expected_rows = generated.model_rows();
    let actual_max = Rc::new(Cell::new(0));
    let compiled_max = Rc::new(Cell::new(0));
    actual
        .on("integer-maximum", {
            let reads = actual_max.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(100.into()))
            })
        })
        .unwrap();
    generated.on_integer_maximum({
        let reads = compiled_max.clone();
        move || {
            reads.set(reads.get() + 1);
            100
        }
    });
    actual
        .on("integer-minimum", {
            let model = rows.model().clone();
            Rc::new(move |_| {
                let changed = model
                    .row(0)
                    .unwrap()
                    .with_field(&["count".into()], 7.into())
                    .unwrap();
                assert!(model.set_row(0, changed));
                Ok(Some(0.into()))
            })
        })
        .unwrap();
    generated.on_integer_minimum({
        let model = expected_rows.clone();
        move |_| {
            let mut changed = model.row(0).unwrap();
            changed.count = 7;
            assert!(model.set_row(0, changed));
            0
        }
    });
    let actual_checks = Rc::new(Cell::new(0));
    let expected_checks = Rc::new(Cell::new(0));
    actual
        .on("decimal-minimum", {
            let checks = actual_checks.clone();
            Rc::new(move |_| {
                checks.set(checks.get() + 1);
                Ok(Some(0.0.into()))
            })
        })
        .unwrap();
    generated.on_decimal_minimum({
        let checks = expected_checks.clone();
        move |_| {
            checks.set(checks.get() + 1);
            0.0
        }
    });
    for ui in [&actual as &dyn Component, &generated] {
        let scene = ui.render_shared();
        assert!(!ui
            .prepare_table_paste(&scene, &[0], 0, vec![0, 1])
            .unwrap()("8\t0.75"));
        assert!(!ui.replay_table_edit_at(&scene, &[0], false));
    }
    assert_eq!((actual_checks.get(), expected_checks.get()), (0, 0));
    assert_eq!((actual_max.get(), compiled_max.get()), (0, 0));
    let row = rows.model().row(0).unwrap();
    assert_eq!(row.field("count"), Some(&Value::Int(7)));
    assert_eq!(row.field("load"), Some(&Value::Float(0.5)));
    let expected = expected_rows.row(0).unwrap();
    assert_eq!((expected.count, expected.load), (7, 0.5));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_paste_does_not_commit_to_a_model_replaced_by_a_callback() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::RecordList;

    let actual = interpret_paste_bounds_fixture();
    let mut generated = expressions_ui::PasteBoundsParity::new();
    let Value::RecordList(original) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let expected_original = generated.model_rows();
    let replacement = original
        .model()
        .row(0)
        .unwrap()
        .with_field(&["count".into()], 7.into())
        .unwrap();
    let mut expected_replacement = expected_original.row(0).unwrap();
    expected_replacement.count = 7;
    actual
        .on("integer-minimum", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                owner
                    .take()
                    .unwrap()
                    .set(
                        "rows",
                        Value::RecordList(RecordList::new(
                            "PasteBoundsRow",
                            vec![replacement.clone()],
                        )),
                    )
                    .unwrap();
                Ok(Some(0.into()))
            })
        })
        .unwrap();
    generated.on_integer_minimum({
        let owner = generated.as_weak();
        move |_| {
            owner
                .upgrade()
                .unwrap()
                .set_rows(vec![expected_replacement.clone()]);
            0
        }
    });
    let calls = || Rc::new(Cell::new(0));
    let actual_checks = calls();
    let expected_checks = calls();
    actual
        .on("decimal-minimum", {
            let checks = actual_checks.clone();
            Rc::new(move |_| {
                checks.set(checks.get() + 1);
                Ok(Some(0.0.into()))
            })
        })
        .unwrap();
    generated.on_decimal_minimum({
        let checks = expected_checks.clone();
        move |_| {
            checks.set(checks.get() + 1);
            0.0
        }
    });
    let paste = |ui: &dyn Component| {
        ui.prepare_table_paste(&ui.render_shared(), &[0], 0, vec![0, 1])
            .unwrap()("8\t0.75")
    };
    assert_eq!([paste(&actual), paste(&generated)], [false, false]);
    assert_eq!((actual_checks.get(), expected_checks.get()), (0, 0));
    let row = original.model().row(0).unwrap();
    assert_eq!(row.field("count"), Some(&Value::Int(2)));
    assert_eq!(row.field("load"), Some(&Value::Float(0.5)));
    let expected = expected_original.row(0).unwrap();
    assert_eq!((expected.count, expected.load), (2, 0.5));
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected replacement");
    };
    assert_eq!(
        rows.model().row(0).unwrap().field("count"),
        Some(&Value::Int(7))
    );
    assert_eq!(generated.model_rows().row(0).unwrap().count, 7);
    for ui in [&actual as &dyn Component, &generated] {
        assert!(!ui.replay_table_edit_at(&ui.render_shared(), &[0], false));
    }
    assert_eq!(actual.take_error(), None);
}

fn interpret_stateful_paste_fixture() -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create("StatefulPasteParity")
        .unwrap()
}

#[test]
fn table_paste_respects_live_disabled_ancestors() {
    let actual = interpret_stateful_paste_fixture();
    let mut generated = expressions_ui::StatefulPasteParity::new();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected table rows");
    };
    let expected_rows = generated.model_rows();
    let path = [0];
    let before = actual.render_shared();
    let expected_before = generated.render_shared();
    let paste = actual
        .prepare_table_paste(&before, &path, 0, vec![2])
        .unwrap();
    let expected_paste = generated
        .prepare_table_paste(&expected_before, &path, 0, vec![2])
        .unwrap();

    actual.set("lock-cell", true.into()).unwrap();
    generated.set_lock_cell(true);
    assert_eq!(
        [expected_paste("blocked"), paste("blocked")],
        [false, false]
    );

    actual.set("lock-cell", false.into()).unwrap();
    generated.set_lock_cell(false);
    for ui in [&actual as &dyn Component, &generated] {
        assert!(ui
            .prepare_table_paste(&ui.render_shared(), &path, 0, vec![2])
            .unwrap()("updated"));
    }
    assert_eq!(
        rows.model().row(0).unwrap().field("note"),
        Some(&Value::String(expected_rows.row(0).unwrap().note))
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_paste_rejects_a_permission_change_to_an_already_sampled_row() {
    use std::{cell::Cell, rc::Rc};

    let actual = interpret_stateful_paste_fixture();
    let mut generated = expressions_ui::StatefulPasteParity::new();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let before = rows.model().snapshot();
    let expected_before = generated.get_rows();
    actual
        .on("may-edit", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |args| {
                if matches!(args, [Value::String(name)] if name == "second") {
                    owner.take().unwrap().set("lock-cell", true.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_may_edit({
        let owner = generated.as_weak();
        move |name| {
            if name == "second" {
                owner.upgrade().unwrap().set_lock_cell(true);
            }
            true
        }
    });
    let paste = |ui: &dyn Component| {
        ui.prepare_table_paste(&ui.render_shared(), &[0], 0, vec![2, 0])
            .unwrap()("first note\tA\nsecond note\tB")
    };
    assert_eq!([paste(&actual), paste(&generated)], [false, false]);
    assert_eq!(rows.model().snapshot(), before);
    assert_eq!(generated.get_rows(), expected_before);
    assert_eq!(actual.get("lock-cell"), Ok(true.into()));
    assert!(generated.get_lock_cell());
    for ui in [&actual as &dyn Component, &generated] {
        assert!(!ui.replay_table_edit_at(&ui.render_shared(), &[0], false));
    }

    actual.set("lock-cell", false.into()).unwrap();
    generated.set_lock_cell(false);
    actual
        .on("may-edit", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    generated.on_may_edit(|_| true);
    assert_eq!([paste(&actual), paste(&generated)], [true, true]);
    for (actual, expected) in rows.model().snapshot().iter().zip(generated.get_rows()) {
        assert_eq!(actual.field("name"), Some(&Value::String(expected.name)));
        assert_eq!(actual.field("note"), Some(&Value::String(expected.note)));
    }
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_projections_evaluate_each_state_selector_once_per_sampled_row() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::TableEditPosition;

    let actual = interpret_stateful_paste_fixture();
    let mut generated = expressions_ui::StatefulPasteParity::new();
    let counts = || std::array::from_fn::<_, 3, _>(|_| Rc::new(Cell::new(0)));
    let actual_calls = counts();
    let generated_calls = counts();
    for (name, calls) in ["hide-table", "hide-row", "hide-proxy"]
        .into_iter()
        .zip(&actual_calls)
    {
        let calls = calls.clone();
        actual
            .on(
                name,
                Rc::new(move |_| {
                    calls.set(calls.get() + 1);
                    Ok(Some(false.into()))
                }),
            )
            .unwrap();
    }
    generated.on_hide_table({
        let calls = generated_calls[0].clone();
        move || {
            calls.set(calls.get() + 1);
            false
        }
    });
    let counter = |calls: Rc<Cell<usize>>| {
        move |_: String| {
            calls.set(calls.get() + 1);
            false
        }
    };
    generated.on_hide_row(counter(generated_calls[1].clone()));
    generated.on_hide_proxy(counter(generated_calls[2].clone()));
    let assert_counts = |expected| {
        assert_eq!(
            generated_calls.each_ref().map(|calls| calls.get()),
            expected,
            "generated"
        );
        assert_eq!(
            actual_calls.each_ref().map(|calls| calls.get()),
            expected,
            "interpreted"
        );
    };

    let actual_scene = actual.render_shared();
    let generated_scene = generated.render_shared();
    assert_counts([1, 0, 0]);
    for (ui, scene) in [
        (&actual as &dyn Component, &actual_scene),
        (&generated as &dyn Component, &generated_scene),
    ] {
        let paste = ui.prepare_table_paste(scene, &[0], 0, vec![0, 2]).unwrap();
        assert!(paste("A\talpha\nB\tbeta"));
    }
    assert_counts([1, 2, 2]);

    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let origin = |key| TableEditPosition {
        row: 0,
        column: 0,
        key,
    };
    let actual_next = actual
        .next_table_edit_cell(
            &actual_scene,
            &[0],
            origin(rows.model().row_key(0).unwrap()),
            &[0, 1],
            false,
        )
        .unwrap();
    let generated_next = generated
        .next_table_edit_cell(
            &generated_scene,
            &[0],
            origin(generated.model_rows().row_key(0).unwrap()),
            &[0, 1],
            false,
        )
        .unwrap();
    assert_eq!((actual_next.row, actual_next.column), (0, 1));
    assert_eq!((generated_next.row, generated_next.column), (0, 1));
    assert_counts([1, 3, 2]);

    assert_eq!(
        actual.copy_table_at(&actual_scene, &[0], &[0, 1]),
        Some("A\talpha".into())
    );
    assert_eq!(
        generated.copy_table_at(&generated_scene, &[0], &[0, 1]),
        Some("A\talpha".into())
    );
    assert_counts([1, 4, 2]);

    actual
        .on("hide-row", {
            let calls = actual_calls[1].clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_hide_row({
        let calls = generated_calls[1].clone();
        move |_| {
            calls.set(calls.get() + 1);
            true
        }
    });
    for ui in [&actual as &dyn Component, &generated] {
        let paste = ui
            .prepare_table_paste(&ui.render_shared(), &[0], 0, vec![2])
            .unwrap();
        assert!(!paste("hidden"));
    }
    assert_counts([1, 5, 2]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_tab_navigation_aborts_when_permission_changes_the_model() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_runtime::{RecordList, TableEditPosition, VecModel};

    let read_rows = |ui: &Instance| {
        let Value::RecordList(rows) = ui.get("rows").unwrap() else {
            panic!("expected rows");
        };
        rows
    };
    let search = |ui: &dyn Component, scene: &Node, key| {
        ui.next_table_edit_cell(
            scene,
            &[0],
            TableEditPosition {
                row: 0,
                column: 0,
                key,
            },
            &[0, 1],
            false,
        )
    };
    let mut outcomes = Vec::new();
    for replace in [true, false] {
        let interpreted = interpret_stateful_paste_fixture();
        let mut generated = expressions_ui::StatefulPasteParity::new();
        let rows = read_rows(&interpreted);
        let compiled_model = generated.model_rows();
        let actual = interpreted.render_shared();
        let expected = generated.render_shared();

        let actual_calls = Rc::new(Cell::new(0));
        let compiled_calls = Rc::new(Cell::new(0));
        interpreted
            .on("may-edit", {
                let rows = rows.clone();
                let owner = Cell::new(Some(interpreted.clone()));
                let calls = actual_calls.clone();
                Rc::new(move |_| {
                    let count = calls.get();
                    calls.set(count + 1);
                    if let Some(owner) = owner.take() {
                        let mut data = rows.model().snapshot();
                        data[0] = data[0]
                            .with_field(&["note".into()], Value::String("updated".into()))
                            .unwrap();
                        if replace {
                            owner
                                .set(
                                    "rows",
                                    Value::RecordList(RecordList::new("StatefulPasteRow", data)),
                                )
                                .unwrap();
                        } else {
                            assert!(rows.model().set_row(0, data.remove(0)));
                        }
                    }
                    Ok(Some((count > 0).into()))
                })
            })
            .unwrap();
        generated.on_may_edit({
            let rows = compiled_model.clone();
            let owner = generated.as_weak();
            let calls = compiled_calls.clone();
            move |_| {
                let count = calls.get();
                calls.set(count + 1);
                if count == 0 {
                    let mut data = rows.snapshot();
                    data[0].note = "updated".into();
                    if replace {
                        owner
                            .upgrade()
                            .unwrap()
                            .bind_model_rows(VecModel::from(data));
                    } else {
                        assert!(rows.set_row(0, data.remove(0)));
                    }
                }
                count > 0
            }
        });
        let actual_result = catch_unwind(AssertUnwindSafe(|| {
            search(&interpreted, &actual, rows.model().row_key(0).unwrap())
        }));
        let compiled_result = catch_unwind(AssertUnwindSafe(|| {
            search(&generated, &expected, compiled_model.row_key(0).unwrap())
        }));
        outcomes.push((
            actual_result.ok(),
            compiled_result.ok(),
            actual_calls.get(),
            compiled_calls.get(),
        ));

        interpreted
            .on("may-edit", Rc::new(|_| Ok(Some(true.into()))))
            .unwrap();
        generated.on_may_edit(|_| true);
        assert_eq!(
            read_rows(&interpreted)
                .model()
                .row(0)
                .unwrap()
                .field("note"),
            Some(&Value::String("updated".into())),
        );
        assert_eq!(generated.model_rows().row(0).unwrap().note, "updated");
        let next = search(
            &interpreted,
            &interpreted.render_shared(),
            read_rows(&interpreted).model().row_key(0).unwrap(),
        )
        .unwrap();
        let compiled_next = search(
            &generated,
            &generated.render_shared(),
            generated.model_rows().row_key(0).unwrap(),
        )
        .unwrap();
        assert_eq!((next.row, next.column), (0, 1));
        assert_eq!((compiled_next.row, compiled_next.column), (0, 1));
        assert_eq!(interpreted.take_error(), None);
    }
    assert_eq!(outcomes, [(Some(None), Some(None), 1, 1); 2]);
}

#[test]
fn projected_table_tab_navigation_reads_live_column_permission() {
    use std::rc::Rc;
    use xrack_runtime::TableEditPosition;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ProjectedPasteParity")
        .unwrap();
    let mut generated = expressions_ui::ProjectedPasteParity::new();
    let actual = interpreted.render_shared();
    let expected = generated.render_shared();
    let Value::RecordList(rows) = interpreted.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let search = |ui: &dyn Component, scene: &Node, key| {
        ui.next_table_edit_cell(
            scene,
            &[0],
            TableEditPosition {
                row: 0,
                column: 0,
                key,
            },
            &[0, 1],
            false,
        )
    };

    for allow in [false, true] {
        interpreted
            .on("allow-second", Rc::new(move |_| Ok(Some(allow.into()))))
            .unwrap();
        generated.on_allow_second(move || allow);
        let actual_next = search(&interpreted, &actual, rows.model().row_key(0).unwrap());
        let compiled_next = search(
            &generated,
            &expected,
            generated.model_rows().row_key(0).unwrap(),
        );
        assert_eq!(
            actual_next.map(|target| (target.row, target.column)),
            allow.then_some((0, 1))
        );
        assert_eq!(
            compiled_next.map(|target| (target.row, target.column)),
            allow.then_some((0, 1))
        );
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_tab_navigation_stops_at_the_first_evaluation_error() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_runtime::TableEditPosition;

    let actual = interpret_stateful_paste_fixture();
    let mut generated = expressions_ui::StatefulPasteParity::new();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected table rows");
    };
    let actual_calls = Rc::new(Cell::new(0));
    let generated_calls = Rc::new(Cell::new(0));
    actual
        .on("may-edit", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                calls.set(calls.get() + 1);
                if matches!(args, [Value::String(name)] if name == "first") {
                    return Err("edit guard failed".into());
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_may_edit({
        let calls = generated_calls.clone();
        move |name| {
            calls.set(calls.get() + 1);
            assert_ne!(name, "first", "edit guard failed");
            true
        }
    });
    let next = |ui: &dyn Component, key| {
        ui.next_table_edit_cell(
            &ui.render_shared(),
            &[0],
            TableEditPosition {
                row: 0,
                column: 0,
                key,
            },
            &[0, 1],
            false,
        )
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| next(
            &generated,
            generated.model_rows().row_key(0).unwrap()
        )))
        .is_err()
    );
    assert!(next(&actual, rows.model().row_key(0).unwrap()).is_none());
    assert_eq!((generated_calls.get(), actual_calls.get()), (1, 1));
    assert_eq!(
        actual.take_error(),
        Some(xrack_interpreter::Error::Evaluation(
            "edit guard failed".into()
        ))
    );
}

fn interpret_stepper_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/stepper/stepper.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "stepper.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

#[test]
fn control_aliases_share_defaults_reset_and_callback_storage() {
    use std::{cell::Cell, rc::Rc};
    use stepper_ui::AliasDefaultsDemo;

    let mut actual = interpret_stepper_fixture("AliasDefaultsDemo");
    let mut expected = AliasDefaultsDemo::new();
    let path = [0, 1];
    let compare = |actual: &Instance, expected: &AliasDefaultsDemo| {
        let text = expected.get_document();
        assert_eq!(actual.get("document"), Ok(text.clone().into()));
        assert_eq!(
            actual.render_shared().at_path(&path).unwrap().text("text"),
            Some(text.as_str())
        );
        assert_eq!(
            actual.get("confirmations"),
            Ok(Value::Int(expected.get_confirmations()))
        );
    };
    compare(&actual, &expected);
    assert_eq!(actual.get("document"), Ok("编辑器默认值".to_owned().into()));
    actual
        .set("document", "Host write".to_owned().into())
        .unwrap();
    expected.set_document("Host write".into());
    compare(&actual, &expected);
    assert!(actual.edit_at(&actual.render_shared(), &path, "Control write".into()));
    assert!(expected.edit_at(&expected.render_shared(), &path, "Control write".into()));
    compare(&actual, &expected);
    actual.reset("document").unwrap();
    expected.reset_document();
    compare(&actual, &expected);
    assert!(actual.dispatch_at(&actual.render_shared(), &path, "accepted"));
    assert!(expected.dispatch_at(&expected.render_shared(), &path, "accepted"));
    actual.invoke("confirmed", &[]).unwrap();
    expected.invoke_confirmed();
    compare(&actual, &expected);

    let actual_calls = Rc::new(Cell::new(0));
    actual
        .on("confirmed", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(None)
            })
        })
        .unwrap();
    let expected_calls = Rc::new(Cell::new(0));
    expected.on_confirmed({
        let calls = expected_calls.clone();
        move || calls.set(calls.get() + 1)
    });
    assert!(actual.dispatch_at(&actual.render_shared(), &path, "accepted"));
    assert!(expected.dispatch_at(&expected.render_shared(), &path, "accepted"));
    compare(&actual, &expected);
    assert_eq!((actual_calls.get(), expected_calls.get()), (1, 1));
    assert_eq!(actual.get("confirmations"), Ok(Value::Int(2)));
}

#[test]
fn range_slider_property_aliases_share_storage_through_nested_components() {
    use xrack_runtime::RangeAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RangeAliasParity")
        .unwrap();
    let mut generated = expressions_ui::RangeAliasParity::new();
    let check = |actual: &Instance, expected: &expressions_ui::RangeAliasParity, lower, upper| {
        assert_eq!(actual.get("lower"), Ok(Value::Float(lower)));
        assert_eq!(actual.get("upper"), Ok(Value::Float(upper)));
        assert_eq!(expected.get_lower(), lower);
        assert_eq!(expected.get_upper(), upper);
        assert_same_scene(&actual.try_render().unwrap(), &expected.render());
    };
    check(&interpreted, &generated, 10.0, 90.0);
    let from_text = xrack_interpreter::load_mlir(&preview.module.to_mlir())
        .unwrap()
        .create("RangeAliasParity")
        .unwrap();
    check(&from_text, &generated, 10.0, 90.0);

    interpreted.set("lower", 20.0.into()).unwrap();
    interpreted.set("upper", 80.0.into()).unwrap();
    generated.set_lower(20.0);
    generated.set_upper(80.0);
    check(&interpreted, &generated, 20.0, 80.0);

    for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
        assert!(ui.adjust_range_at(&ui.render(), &[0, 0, 0], RangeAction::SetValue(40.0)));
        assert!(ui.adjust_range_at(&ui.render(), &[0, 0, 1], RangeAction::SetValue(70.0)));
    }
    check(&interpreted, &generated, 40.0, 70.0);

    interpreted.reset("lower").unwrap();
    interpreted.reset("upper").unwrap();
    generated.reset_lower();
    generated.reset_upper();
    check(&interpreted, &generated, 10.0, 90.0);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn controlled_properties_share_rendered_defaults_aliases_and_editor_state() {
    use xrack_runtime::{CheckState, ControlAction};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ControlStorageParity")
        .unwrap();
    let mut generated = expressions_ui::ControlStorageParity::new();
    let compare = |actual: &Instance, generated: &expressions_ui::ControlStorageParity| {
        assert_eq!(actual.get("level"), Ok(generated.get_level().into()));
        assert_eq!(actual.get("amount"), Ok(generated.get_amount().into()));
        assert_same_scene(&actual.try_render().unwrap(), &generated.render());
    };
    compare(&actual, &generated);
    assert_eq!(actual.get("level"), Ok(4.5.into()));
    assert_eq!(actual.get("amount"), Ok(6.into()));
    assert_eq!(
        actual.render_shared().children[2]
            .properties
            .get("check-state"),
        Some(&Value::CheckState(CheckState::Unchecked))
    );
    assert_eq!(actual.render_shared().children[3].text("edit-text"), None);

    actual.set("level", 12.0.into()).unwrap();
    generated.set_level(12.0);
    actual.set("minimum-value", 8.5.into()).unwrap();
    generated.set_minimum_value(8.5);
    compare(&actual, &generated);
    assert_eq!(actual.get("level"), Ok(12.0.into()));
    actual.reset("level").unwrap();
    generated.reset_level();
    compare(&actual, &generated);
    assert_eq!(actual.get("level"), Ok(8.5.into()));

    assert!(actual.dispatch("check", "clicked"));
    assert!(generated.dispatch("check", "clicked"));
    assert_eq!(
        actual.get("last-state"),
        Ok(Value::CheckState(CheckState::PartiallyChecked))
    );
    assert!(actual.edit_at(&actual.render_shared(), &[3], "draft".into()));
    assert!(generated.edit_at(&generated.render(), &[3], "draft".into()));
    compare(&actual, &generated);
    assert!(actual.control_at(&actual.render_shared(), &[3], ControlAction::Select(1)));
    assert!(generated.control_at(&generated.render(), &[3], ControlAction::Select(1)));
    compare(&actual, &generated);
    assert_eq!(actual.render_shared().children[3].text("edit-text"), None);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn failed_property_bindings_retry_without_an_input_change() {
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RetryBindingParity")
        .unwrap();
    let mut compiled = expressions_ui::RetryBindingParity::new();
    let actual_reads = Rc::new(RefCell::new(Vec::new()));
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("source", {
            let reads = actual_reads.clone();
            let seen = RefCell::new(BTreeSet::new());
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                reads.borrow_mut().push(*tag);
                if seen.borrow_mut().insert(*tag) {
                    return Err(format!("sample {tag} failed"));
                }
                Ok(Some(tag.to_string().into()))
            })
        })
        .unwrap();
    compiled.on_source({
        let reads = compiled_reads.clone();
        let seen = RefCell::new(BTreeSet::new());
        move |tag| {
            reads.borrow_mut().push(tag);
            assert!(!seen.borrow_mut().insert(tag), "sample {tag} failed");
            tag.to_string()
        }
    });
    assert_eq!(
        actual.get("observed"),
        Err(Error::Evaluation("sample 1 failed".into()))
    );
    assert!(catch_unwind(AssertUnwindSafe(|| compiled.get_observed())).is_err());
    assert_eq!(actual.get("observed"), Ok("1".to_owned().into()));
    assert_eq!(compiled.get_observed(), "1");
    for tag in [2, 3] {
        assert_eq!(
            actual.try_render().unwrap_err(),
            Error::Evaluation(format!("sample {tag} failed"))
        );
        assert!(actual.scene_dirty());
        assert!(catch_unwind(AssertUnwindSafe(|| compiled.render_shared())).is_err());
    }
    let scene = actual.try_render().unwrap();
    assert_same_scene(&scene, &compiled.render_shared());
    assert_eq!(scene.children[0].text("text"), Some("2"));
    assert_eq!(scene.children[1].text("text"), Some("3"));
    assert!(!actual.scene_dirty());
    assert!(Rc::ptr_eq(&scene, &actual.try_render().unwrap()));
    assert_eq!(actual_reads.take(), [1, 1, 2, 2, 3, 3]);
    assert_eq!(compiled_reads.take(), [1, 1, 2, 2, 3, 3]);
}

#[test]
fn instance_state_targets_validate_aliases_the_same_in_both_backends() {
    let source = |binding, overrides| {
        format!(
            r#"
            component StateTarget inherits Rectangle {{
                in-out property <float> level: 2;
                in-out property <float> first <=> root.level;
                in-out property <float> second <=> root.level;
            }}
            export component StateAliasProbe inherits Window {{
                in-out property <bool> active: true;
                in-out property <float> external: 1;
                out property <float> observed: child.level;
                child := StateTarget {{ {binding} }}
                states [ override when root.active: {{ {overrides} }} ]
            }}
        "#
        )
    };
    let ambiguous = source("", "child.first: 4; child.second: 5;");
    let diagnostic = "states target multiple aliases of the same component input at `second`";
    let compile = xrack_compiler::compile(&ambiguous, "state-alias.xr")
        .err()
        .expect("ambiguous state input was accepted by the compiler");
    assert!(compile.to_string().contains(diagnostic), "{compile}");
    let preview =
        xrack_compiler::prepare_preview(&ambiguous, "state-alias.xr", Default::default()).unwrap();
    let interpreted = Program::from_module(&preview.module)
        .and_then(|program| program.create("StateAliasProbe"))
        .err()
        .expect("ambiguous state input was accepted by the interpreter");
    assert!(
        interpreted.to_string().contains(diagnostic),
        "{interpreted}"
    );

    let source = source("first: root.external;", "child.second: 4;");
    xrack_compiler::compile(&source, "state-alias.xr").unwrap();
    let preview =
        xrack_compiler::prepare_preview(&source, "state-alias.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("StateAliasProbe")
        .unwrap();
    assert_eq!(ui.get("observed"), Ok(4.0.into()));
    ui.set("active", false.into()).unwrap();
    assert_eq!(ui.get("observed"), Ok(1.0.into()));

    // Feed a verified MLIR link directly to the interpreter, bypassing DSL diagnostics.
    let mlir = preview.module.to_mlir();
    assert_eq!(mlir.matches(r#""xr.bind""#).count(), 1);
    let linked = mlir.replacen(r#""xr.bind""#, r#""xr.instance_link""#, 1);
    let error = xrack_interpreter::load_mlir(&linked)
        .and_then(|program| program.create("StateAliasProbe"))
        .err()
        .expect("state input over a two-way link was accepted");
    assert!(
        error
            .to_string()
            .contains("state cannot override two-way input `second`"),
        "{error}"
    );
}

#[test]
fn instance_callback_handlers_validate_shared_alias_targets() {
    let source = |second_owner: &str, handlers: &str| {
        format!(
            r#"
            component CallbackLeaf inherits Rectangle {{
                callback received(int);
            }}
            component CallbackTarget inherits Rectangle {{
                callback first <=> left.received;
                callback second <=> {second_owner}.received;
                left := CallbackLeaf {{}}
                right := CallbackLeaf {{}}
            }}
            export component CallbackAliasProbe inherits Window {{
                out property <int> observed: 0;
                callback fire();
                fire => {{ child.first(5); child.second(7); }}
                child := CallbackTarget {{ {handlers} }}
            }}
        "#
        )
    };
    let first = "first(value) => { root.observed = root.observed * 10 + value; }";
    let both =
        format!("{first} second(value) => {{ root.observed = root.observed * 10 + value + 1; }}");
    let duplicate = source("left", &both);
    let diagnostic = "multiple instance handlers target the same callback";
    let compiled = xrack_compiler::compile(&duplicate, "callback-alias.xr")
        .err()
        .expect("duplicate instance handlers were accepted by the compiler");
    assert!(compiled.to_string().contains(diagnostic), "{compiled}");
    let preview =
        xrack_compiler::prepare_preview(&duplicate, "callback-alias.xr", Default::default())
            .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .and_then(|program| program.create("CallbackAliasProbe"))
        .err()
        .expect("duplicate instance handlers were accepted by the interpreter");
    assert!(
        interpreted.to_string().contains(diagnostic),
        "{interpreted}"
    );

    for (valid, observed) in [(source("left", first), 57), (source("right", &both), 58)] {
        xrack_compiler::compile(&valid, "callback-alias.xr").unwrap();
        let preview =
            xrack_compiler::prepare_preview(&valid, "callback-alias.xr", Default::default())
                .unwrap();
        let ui = Program::from_module(&preview.module)
            .unwrap()
            .create("CallbackAliasProbe")
            .unwrap();
        assert_eq!(ui.invoke("fire", &[]), Ok(None));
        assert_eq!(ui.get("observed"), Ok(Value::Int(observed)));
    }
}

#[test]
fn incoming_callback_precedes_defaults_on_equivalent_nested_aliases() {
    let source = include_str!("expressions.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "expressions.xr", Default::default()).unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("CallbackPrecedenceParity")
        .unwrap();
    let generated = expressions_ui::CallbackPrecedenceParity::new();

    assert_eq!(interpreted.get("overridden"), Ok(31_i64.into()));
    assert_eq!(interpreted.get("fallback"), Ok(11_i64.into()));
    assert_eq!(generated.get_overridden(), 31);
    assert_eq!(generated.get_fallback(), 11);

    let duplicate = source.replace(
        "fallback(value) => { value + 10 }",
        "fallback(value) => { value + 10 } supplied(value) => { value + 40 }",
    );
    let diagnostic = "multiple alias handlers target the same callback";
    let compiled_error = xrack_compiler::compile(&duplicate, "expressions.xr")
        .err()
        .expect("compiler accepted two defaults for the same callback");
    assert!(
        compiled_error.to_string().contains(diagnostic),
        "{compiled_error}"
    );
    let preview =
        xrack_compiler::prepare_preview(&duplicate, "expressions.xr", Default::default()).unwrap();
    let interpreted_error = Program::from_module(&preview.module)
        .and_then(|program| program.create("CallbackPrecedenceParity"))
        .err()
        .expect("interpreter accepted two defaults for the same callback");
    assert!(
        interpreted_error.to_string().contains(diagnostic),
        "{interpreted_error}"
    );
}

#[test]
fn nested_property_aliases_preserve_incoming_links_and_local_defaults() {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("PropertyForwardParity")
        .unwrap();
    let mut generated = expressions_ui::PropertyForwardParity::new();
    let check = |generated: &expressions_ui::PropertyForwardParity, value: i64| {
        assert_eq!(generated.get_external(), value);
        assert_eq!(generated.get_edited(), value);
        assert_eq!(generated.get_overridden(), value);
        assert_eq!(generated.get_fallback(), 20);
        for name in ["external", "edited", "overridden"] {
            assert_eq!(interpreted.get(name), Ok(value.into()), "{name}");
        }
        assert_eq!(interpreted.get("fallback"), Ok(20_i64.into()));
    };
    check(&generated, 30);

    interpreted.set("external", 40_i64.into()).unwrap();
    generated.set_external(40);
    check(&generated, 40);

    interpreted.set("edited", 50_i64.into()).unwrap();
    generated.set_edited(50);
    check(&generated, 50);

    interpreted.reset("edited").unwrap();
    generated.reset_edited();
    check(&generated, 30);

    interpreted.set("edited", 60_i64.into()).unwrap();
    generated.set_edited(60);
    interpreted.reset("external").unwrap();
    generated.reset_external();
    check(&generated, 30);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn pure_callback_alias_switches_reactive_dependency_to_its_override() {
    use std::rc::Rc;
    use stepper_ui::CallbackProbe;

    let actual = interpret_stepper_fixture("CallbackProbe");
    let mut expected = CallbackProbe::new();
    assert_eq!(actual.get("answer"), Ok(Value::Int(6)));
    actual.set("bias", Value::Int(8)).unwrap();
    expected.set_bias(8);
    assert_eq!(actual.get("answer"), Ok(Value::Int(expected.get_answer())));
    assert_eq!(
        actual.invoke("calculation", &[Value::Int(10)]),
        Ok(Some(Value::Int(18)))
    );
    actual
        .on(
            "calculation",
            Rc::new(|args| {
                let [Value::Int(value)] = args else {
                    unreachable!()
                };
                Ok(Some(Value::Int(value * 5)))
            }),
        )
        .unwrap();
    expected.on_calculation(|value| value * 5);
    assert_eq!(actual.get("answer"), Ok(Value::Int(expected.get_answer())));
    let cache = xrack_runtime::Property::default();
    assert_eq!(cache.get(|| actual.get("answer")), Ok(Value::Int(20)));
    actual.set("bias", Value::Int(20)).unwrap();
    assert!(!cache.is_dirty());
    assert_eq!(
        actual.invoke("calculation", &[Value::Int(10)]),
        Ok(Some(Value::Int(50)))
    );
}

fn callback_replacement_fixture() -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create("CallbackReplacementParity")
        .unwrap()
}

#[test]
fn callback_statements_can_replace_the_host_return_handler_for_the_current_call() {
    use std::{cell::Cell, rc::Rc};

    let actual = callback_replacement_fixture();
    let mut generated = expressions_ui::CallbackReplacementParity::new();
    actual
        .on("advance", Rc::new(|_| Ok(Some(10.into()))))
        .unwrap();
    generated.on_advance(|| 10);
    actual
        .on("replace", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    assert_eq!(owner.get("steps"), Ok(1.into()));
                    owner
                        .on("advance", Rc::new(|_| Ok(Some(20.into()))))
                        .unwrap();
                }
                Ok(None)
            })
        })
        .unwrap();
    generated.on_replace({
        let owner = generated.as_weak();
        move || {
            let mut owner = owner.upgrade().unwrap();
            if owner.get_steps() == 1 {
                owner.on_advance(|| 20);
            }
        }
    });
    assert_eq!(generated.invoke_advance(), 20);
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(20.into())));
    assert_eq!(generated.invoke_advance(), 20);
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(20.into())));
    assert_eq!(actual.get("steps"), Ok(2.into()));
    assert_eq!(generated.get_steps(), 2);
}

#[test]
fn weak_host_callbacks_track_properties_without_retaining_components() {
    use std::rc::Rc;

    let actual = callback_replacement_fixture();
    let mut generated = expressions_ui::CallbackReplacementParity::new();
    let actual_weak = actual.as_weak();
    let generated_weak = generated.as_weak();
    actual
        .on("probe", {
            let owner = actual_weak.clone();
            Rc::new(move |_| {
                let owner = owner.upgrade().expect("live callback owner");
                owner
                    .get("steps")
                    .map(Some)
                    .map_err(|error| error.to_string())
            })
        })
        .unwrap();
    generated.on_probe({
        let owner = generated_weak.clone();
        move || owner.upgrade().expect("live callback owner").get_steps()
    });

    assert_eq!(actual.get("observed"), Ok(0_i64.into()));
    assert_eq!(generated.get_observed(), 0);
    for count in 1..=2 {
        assert_eq!(actual.invoke("advance", &[]), Ok(Some((-1_i64).into())));
        assert_eq!(generated.invoke_advance(), -1);
        assert_eq!(actual.get("observed"), Ok(count.into()));
        assert_eq!(generated.get_observed(), count);
    }

    drop(actual);
    drop(generated);
    assert!(actual_weak.upgrade().is_none());
    assert!(generated_weak.upgrade().is_none());
}

#[test]
fn host_callbacks_can_replace_themselves_without_losing_dependency_invalidation() {
    use std::{cell::Cell, rc::Rc};

    let actual = callback_replacement_fixture();
    let mut generated = expressions_ui::CallbackReplacementParity::new();
    actual
        .on("probe", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                owner
                    .take()
                    .unwrap()
                    .on("probe", Rc::new(|_| Ok(Some(20.into()))))
                    .unwrap();
                Ok(Some(10.into()))
            })
        })
        .unwrap();
    generated.on_probe({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().on_probe(|| 20);
            10
        }
    });
    assert_eq!(actual.get("observed"), Ok(10.into()));
    assert_eq!(generated.get_observed(), 10);
    assert_eq!(actual.get("observed"), Ok(20.into()));
    assert_eq!(generated.get_observed(), 20);

    actual
        .on("advance", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                owner
                    .take()
                    .unwrap()
                    .on("advance", Rc::new(|_| Ok(Some(40.into()))))
                    .unwrap();
                Ok(Some(30.into()))
            })
        })
        .unwrap();
    generated.on_advance({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().on_advance(|| 40);
            30
        }
    });
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(30.into())));
    assert_eq!(generated.invoke_advance(), 30);
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(40.into())));
    assert_eq!(generated.invoke_advance(), 40);
}

#[test]
fn effectful_host_callbacks_reject_reentry_and_allow_replacement() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    let actual = callback_replacement_fixture();
    let mut generated = expressions_ui::CallbackReplacementParity::new();
    actual
        .on("advance", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                let Some(owner) = owner.take() else {
                    return Ok(Some(10.into()));
                };
                owner
                    .invoke("advance", &[])
                    .map_err(|error| error.to_string())
            })
        })
        .unwrap();
    generated.on_advance({
        let owner = generated.as_weak();
        let reenter = Cell::new(true);
        move || {
            if reenter.replace(false) {
                owner.upgrade().unwrap().invoke_advance()
            } else {
                10
            }
        }
    });
    let panic = catch_unwind(AssertUnwindSafe(|| generated.invoke_advance())).unwrap_err();
    let diagnostic = "effectful host callback is already running";
    assert_eq!(
        panic.downcast_ref::<String>().map(String::as_str),
        Some(diagnostic)
    );
    let error = actual.invoke("advance", &[]).unwrap_err();
    assert!(error.to_string().contains(diagnostic), "{error}");
    assert_eq!(actual.get("steps"), Ok(generated.get_steps().into()));
    assert_eq!(generated.get_steps(), 2);
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(10.into())));
    assert_eq!(generated.invoke_advance(), 10);

    actual
        .on("advance", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                let owner = owner.take().unwrap();
                owner
                    .on("advance", Rc::new(|_| Ok(Some(20.into()))))
                    .unwrap();
                owner
                    .invoke("advance", &[])
                    .map_err(|error| error.to_string())
            })
        })
        .unwrap();
    generated.on_advance({
        let owner = generated.as_weak();
        move || {
            let mut owner = owner.upgrade().unwrap();
            owner.on_advance(|| 20);
            owner.invoke_advance()
        }
    });
    assert_eq!(actual.invoke("advance", &[]), Ok(Some(20.into())));
    assert_eq!(generated.invoke_advance(), 20);
    assert_eq!(actual.get("steps"), Ok(generated.get_steps().into()));
    assert_eq!(generated.get_steps(), 5);

    actual
        .on("probe", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                let Some(owner) = owner.take() else {
                    return Ok(Some(30.into()));
                };
                owner
                    .invoke("probe", &[])
                    .map_err(|error| error.to_string())
            })
        })
        .unwrap();
    generated.on_probe({
        let owner = generated.as_weak();
        let reenter = Cell::new(true);
        move || {
            if reenter.replace(false) {
                owner.upgrade().unwrap().invoke_probe()
            } else {
                30
            }
        }
    });
    assert_eq!(actual.invoke("probe", &[]), Ok(Some(30.into())));
    assert_eq!(generated.invoke_probe(), 30);
}

#[test]
fn linked_wrappers_and_string_delegates_keep_one_model_and_route_writes() {
    use stepper_ui::LinkedControlsDemo;
    use xrack_layout::{Interaction, Size};

    let mut actual = interpret_stepper_fixture("LinkedControlsDemo");
    let mut expected = LinkedControlsDemo::new();
    let first = [0, 1, 0, 1];
    let second = [0, 2, 0, 1];
    for path in [&first, &second] {
        assert_eq!(
            actual.render_shared().at_path(path).unwrap().text("text"),
            Some("共享草稿")
        );
    }
    assert!(actual.edit_at(&actual.render_shared(), &first, "Linked write".into()));
    assert!(expected.edit_at(&expected.render_shared(), &first, "Linked write".into()));
    assert_eq!(actual.get("read-back"), Ok(expected.get_read_back().into()));
    assert_eq!(
        actual
            .render_shared()
            .at_path(&second)
            .unwrap()
            .text("text"),
        Some("Linked write")
    );
    assert!(actual.dispatch_at(&actual.render_shared(), &first, "accepted"));
    assert!(expected.dispatch_at(&expected.render_shared(), &first, "accepted"));
    assert_eq!(
        actual.get("last-submitted"),
        Ok(expected.get_last_submitted().into())
    );
    actual.invoke("reset-through-second", &[]).unwrap();
    expected.invoke_reset_through_second();
    assert_eq!(actual.get("shared"), Ok(expected.get_shared().into()));

    let actual_tags = actual.string_model("tags").unwrap();
    let expected_tags = expected.model_tags();
    let key = actual_tags.row_key(0).unwrap();
    let input = Interaction::default();
    let size = Size::new(560.0, 340.0);
    let (actual_scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    let tag = [0, 5, 1, 0, 0, 1];
    assert!(actual.edit_at(&actual_scene, &tag, "Tag written by editor".into()));
    assert!(expected.edit_at(&expected_scene, &tag, "Tag written by editor".into()));
    assert_eq!(actual_tags.snapshot(), expected_tags.snapshot());
    assert_eq!(
        actual.get("tags"),
        Ok(Value::StringList(expected.get_tags()))
    );
    assert_eq!(actual_tags.row_key(0), Some(key));

    assert!(actual_tags.insert(0, "Inserted".into()));
    assert!(expected_tags.insert(0, "Inserted".into()));
    assert!(!actual.edit_at(&actual_scene, &tag, "Stale".into()));
    let (current, _) = prepare_list(&actual, &input, size);
    let moved = [0, 5, 1, 1, 0, 1];
    assert!(actual.edit_at(&current, &moved, "Moved row".into()));
    assert_eq!(actual_tags.row_key(1), Some(key));
    assert_eq!(actual_tags.row(1).as_deref(), Some("Moved row"));
    actual
        .set("tags", Value::StringList(actual_tags.snapshot()))
        .unwrap();
    assert!(!actual.edit_at(&current, &moved, "Old model".into()));
    assert_eq!(actual_tags.row(1).as_deref(), Some("Moved row"));
}

#[test]
fn scalar_table_cells_commit_by_row_identity() {
    use table_ui::TableDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::VecModel;

    let mut actual = interpret_table_fixture("TableDemo");
    let mut expected = TableDemo::new();
    let model = VecModel::from(vec!["B".to_owned(), "A".to_owned()]);
    actual
        .set("records", Value::StringModel(model.clone()))
        .unwrap();
    expected.set_records(vec!["B".to_owned(), "A".to_owned()]);
    let expected_model = expected.model_records();
    let key = model.row_key(0);
    let size = Size::new(560.0, 370.0);
    let input = Interaction::default();
    let (scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    let first = [0, 2, 0, 0, 0];
    assert!(actual.commit_table_cell(&scene, &first, "B", "Changed".into()));
    assert!(expected.commit_table_cell(&expected_scene, &first, "B", "Changed".into()));
    assert_eq!(model.snapshot(), expected_model.snapshot());
    assert!(!actual.commit_table_cell(&scene, &first, "Changed", "Mismatched scene".into()));
    assert!(!expected.commit_table_cell(
        &expected_scene,
        &first,
        "Changed",
        "Mismatched scene".into()
    ));
    assert!(model.sort_by(String::cmp));
    assert!(expected_model.sort_by(String::cmp));
    assert!(!actual.commit_table_cell(&scene, &first, "B", "Stale".into()));
    let (scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    let second = [0, 2, 1, 0, 0];
    assert!(actual.commit_table_cell(&scene, &second, "Changed", "Moved".into()));
    assert!(expected.commit_table_cell(&expected_scene, &second, "Changed", "Moved".into()));
    assert_eq!(model.snapshot(), expected_model.snapshot());
    assert_eq!(model.row_key(1), key);
}

#[test]
fn delegated_table_controls_and_clipboard_parsers_share_live_row_links() {
    use records_ui::{DelegateTableDemo, QuantityPaste};
    use std::rc::Rc;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, RecordValue, SpinAction};

    let source = include_str!("../examples/table/records.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "records.xr", Default::default()).unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("DelegateTableDemo")
        .unwrap();
    let mut expected = DelegateTableDemo::new();
    actual
        .on(
            "decode-quantity",
            Rc::new(|args| {
                let [Value::String(text), Value::Int(_), Value::Int(index)] = args else {
                    unreachable!()
                };
                let parsed = text.trim_end_matches('件').parse::<i64>().ok();
                Ok(Some(Value::Record(RecordValue::new(
                    "QuantityPaste",
                    [
                        ("accepted".into(), parsed.is_some().into()),
                        ("value".into(), Value::Int(parsed.unwrap_or(0) + index)),
                    ]
                    .into(),
                ))))
            }),
        )
        .unwrap();
    expected.on_decode_quantity(|text, _, index| {
        let parsed = text.trim_end_matches('件').parse::<i64>().ok();
        QuantityPaste {
            accepted: parsed.is_some(),
            value: parsed.unwrap_or(0) + index,
        }
    });
    let path = [0, 1];
    let paste = |ui: &dyn Component| {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, vec![0, 1])
            .unwrap()
    };
    let stale = paste(&actual);
    let expected_stale = paste(&expected);
    actual.set("show-quantity", false.into()).unwrap();
    expected.set_show_quantity(false);
    assert!(!stale("Hidden\t5件"));
    assert!(!expected_stale("Hidden\t5件"));
    actual.set("show-quantity", true.into()).unwrap();
    expected.set_show_quantity(true);
    assert!(!paste(&actual)("Reject\tinvalid"));
    assert!(!paste(&expected)("Reject\tinvalid"));
    assert!(
        paste(&actual)("Router\t24件\nSwitch\t30件"),
        "{:?}",
        actual.take_error()
    );
    assert!(paste(&expected)("Router\t24件\nSwitch\t30件"));
    let Value::RecordList(actual_model) = actual.get("records").unwrap() else {
        panic!("expected record model")
    };
    let expected_model = expected.model_records();
    let compare = || {
        for row in 0..2 {
            let actual = actual_model.model().row(row).unwrap();
            let expected = expected_model.row(row).unwrap();
            assert_eq!(actual.field("name"), Some(&Value::String(expected.name)));
            assert_eq!(actual.field("units"), Some(&Value::Int(expected.units)));
        }
    };
    compare();
    assert_eq!(
        actual_model.model().row(1).unwrap().field("units"),
        Some(&Value::Int(31))
    );

    let input = Interaction::default();
    let size = Size::new(620.0, 338.0);
    let (scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    let spin = [0, 1, 0, 0, 1, 0, 0];
    let action = ControlAction::Spin(SpinAction::Commit(47));
    assert!(actual.control_at(&scene, &spin, action));
    assert!(expected.control_at(&expected_scene, &spin, action));
    compare();
    assert_eq!(
        actual.get("committed-units"),
        Ok(Value::Int(expected.get_committed_units()))
    );
    assert_eq!(actual.get("committed-units"), Ok(Value::Int(47)));
    assert_eq!(actual.get("committed-row"), Ok(Value::Int(0)));
}

#[test]
fn integer_delegate_drafts_reject_model_changes_without_recording_an_edit() {
    use records_ui::DelegateTableDemo;
    use xrack_layout::{EditAction, Interaction, Size};
    use xrack_runtime::{ControlAction, SpinAction};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/table/records.xr"),
        "records.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("DelegateTableDemo")
        .unwrap();
    let mut generated = DelegateTableDemo::new();
    let Value::RecordList(records) = interpreted.get("records").unwrap() else {
        panic!("expected records");
    };
    let model = records.model();
    let compiled_model = generated.model_records();
    let spin = [0, 1, 0, 0, 1, 0, 0];
    let table = [0, 1];
    let size = Size::new(620.0, 338.0);
    let input = Interaction::default();
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let draft =
        |original, value| ControlAction::Spin(SpinAction::CommitIfCurrent { original, value });
    assert_eq!(actual.at_path(&spin).unwrap().int("value"), Some(12));
    assert_eq!(expected.at_path(&spin).unwrap().int("value"), Some(12));

    let change_row = |units, owner: &str| {
        let record = model
            .row(0)
            .unwrap()
            .with_field(&["units".into()], Value::Int(units))
            .unwrap()
            .with_field(&["owner".into()], Value::String(owner.into()))
            .unwrap();
        assert!(model.set_row(0, record));
        let mut compiled_row = compiled_model.row(0).unwrap();
        compiled_row.units = units;
        compiled_row.owner = owner.into();
        assert!(compiled_model.set_row(0, compiled_row));
    };
    change_row(34, "外部更新");

    assert_eq!(
        [
            interpreted.control_at(&actual, &spin, draft(12, 47)),
            generated.control_at(&expected, &spin, draft(12, 47)),
        ],
        [false, false]
    );
    let check_row = |units, owner: &str| {
        let row = model.row(0).unwrap();
        let compiled = compiled_model.row(0).unwrap();
        assert_eq!(row.field("units"), Some(&Value::Int(units)));
        assert_eq!(compiled.units, units);
        assert_eq!(row.field("owner"), Some(&Value::String(owner.into())));
        assert_eq!(compiled.owner, owner);
    };
    check_row(34, "外部更新");
    assert_eq!(interpreted.get("committed-units"), Ok((-1_i64).into()));
    assert_eq!(generated.get_committed_units(), -1);
    assert!(!interpreted.replay_table_edit_at(&interpreted.render_shared(), &table, false));
    assert!(!generated.replay_table_edit_at(&generated.render_shared(), &table, false));

    let (actual, actual_layout) = prepare_list(&interpreted, &input, size);
    let (expected, expected_layout) = prepare_list(&generated, &input, size);
    change_row(34, "负责人更新");
    for (ui, scene, layout) in [
        (
            &mut interpreted as &mut dyn Component,
            &*actual,
            &actual_layout,
        ),
        (&mut generated, &*expected, &expected_layout),
    ] {
        let mut input = Interaction::default();
        let target = input.accessibility_target(scene, layout, &spin).unwrap();
        input.accessibility_focus(target);
        input.edit_spin(scene.at_path(&spin).unwrap(), EditAction::ReplaceAll("47"));
        let commit = input.commit_spin_draft().unwrap();
        assert_eq!(commit.path, spin);
        assert_eq!(commit.action, draft(34, 47));
        assert!(ui.control_at(scene, &commit.path, commit.action));
    }
    check_row(47, "负责人更新");
    assert_eq!(interpreted.get("committed-units"), Ok(47_i64.into()));
    assert_eq!(generated.get_committed_units(), 47);
    assert!(interpreted.replay_table_edit_at(&interpreted.render_shared(), &table, false));
    assert!(generated.replay_table_edit_at(&generated.render_shared(), &table, false));
    check_row(34, "负责人更新");
    assert!(interpreted.replay_table_edit_at(&interpreted.render_shared(), &table, true));
    assert!(generated.replay_table_edit_at(&generated.render_shared(), &table, true));
    check_row(47, "负责人更新");
    assert_eq!(interpreted.take_error(), None);
}

fn interpret_tree_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/tree/tree.xr");
    let preview = xrack_compiler::prepare_preview(source, "tree.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

fn tree_keys(scene: &Node, path: &[usize]) -> Vec<i64> {
    scene
        .at_path(path)
        .unwrap()
        .tree
        .as_ref()
        .unwrap()
        .rows()
        .iter()
        .map(|row| row.key)
        .collect()
}

fn tree_disclosure(
    scene: &Node,
    path: &[usize],
    key: i64,
    expanded: bool,
) -> xrack_runtime::ControlAction {
    let tree = scene.at_path(path).unwrap().tree.as_ref().unwrap();
    let row = tree.rows()[tree.visible_index(key).unwrap()].row;
    xrack_runtime::ControlAction::Tree(xrack_runtime::TreeAction::SetExpanded {
        key,
        row,
        expanded,
    })
}

fn tree_input_fixture() -> (Instance, expressions_ui::TreeInputParity) {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TreeInputParity")
        .unwrap();
    (actual, expressions_ui::TreeInputParity::new())
}

#[test]
fn tree_dsl_commands_share_argument_order_and_expansion_results() {
    use expressions_ui::TreeInputParity;
    use std::{cell::RefCell, rc::Rc};

    let (actual, mut compiled) = tree_input_fixture();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let compiled_calls = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("query-key", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*tag);
                Ok(Some(1_i64.into()))
            })
        })
        .unwrap();
    compiled.on_query_key({
        let calls = compiled_calls.clone();
        move |tag| {
            calls.borrow_mut().push(tag);
            1
        }
    });

    for (name, invoke, expanded, expansions, collapses) in [
        (
            "expand-subtree",
            TreeInputParity::invoke_expand_subtree as fn(&mut TreeInputParity),
            true,
            1,
            0,
        ),
        (
            "collapse-all",
            TreeInputParity::invoke_collapse_all,
            false,
            1,
            1,
        ),
        (
            "reveal-child",
            TreeInputParity::invoke_reveal_child,
            true,
            2,
            1,
        ),
        (
            "reveal-child",
            TreeInputParity::invoke_reveal_child,
            true,
            2,
            1,
        ),
        (
            "toggle-root",
            TreeInputParity::invoke_toggle_root,
            false,
            2,
            2,
        ),
    ] {
        actual.invoke(name, &[]).unwrap();
        invoke(&mut compiled);
        let keys: &[i64] = if expanded { &[1, 11, 2, 3] } else { &[1, 2, 3] };
        assert_eq!(tree_keys(&actual.render(), &[0]), keys, "{name}");
        assert_eq!(tree_keys(&compiled.render(), &[0]), keys, "{name}");
        assert_eq!(actual.get("expansions"), Ok(Value::Int(expansions)));
        assert_eq!(actual.get("collapses"), Ok(Value::Int(collapses)));
        assert_eq!(compiled.get_expansions(), expansions);
        assert_eq!(compiled.get_collapses(), collapses);
    }
    assert_eq!(*actual_calls.borrow(), [6, 7]);
    assert_eq!(*compiled_calls.borrow(), [6, 7]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tree_input_tracks_row_identity_and_rejects_an_outdated_range() {
    use xrack_runtime::{
        ControlAction, RecordValue, RowSelection, TreeAction, TreeSelectionAction,
    };

    let (mut actual, mut expected) = tree_input_fixture();
    let path = [0];
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records");
    };
    let model = records.model();
    let expected_model = expected.model_records();
    let record_id = |row: &RecordValue| match row.field("id") {
        Some(Value::Int(id)) => *id,
        _ => panic!("missing record id"),
    };
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    assert!(model.sort_by(|a, b| record_id(b).cmp(&record_id(a))));
    assert!(expected_model.sort_by(|a, b| b.id.cmp(&a.id)));
    assert!(actual.control_at(&actual_scene, &path, ControlAction::Select(0)));
    assert!(expected.control_at(&expected_scene, &path, ControlAction::Select(0)));
    assert_eq!(actual.get("current"), Ok(1_i64.into()));
    assert_eq!(expected.get_current(), 1);

    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    actual.set("multiple", true.into()).unwrap();
    expected.set_multiple(true);
    assert!(!actual.control_at(&actual_scene, &path, ControlAction::Select(0)));
    assert!(!expected.control_at(&expected_scene, &path, ControlAction::Select(0)));

    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let extend = |scene: &Node| {
        let snapshot = scene.at_path(&path).unwrap().tree.as_ref().unwrap();
        let item = snapshot.rows()[0];
        ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Row {
            key: item.key,
            row: item.row,
            operation: RowSelection::Extend,
        }))
    };
    assert!(model.sort_by(|a, b| record_id(a).cmp(&record_id(b))));
    assert!(expected_model.sort_by(|a, b| a.id.cmp(&b.id)));
    assert!(!actual.control_at(&actual_scene, &path, extend(&actual_scene)));
    assert!(!expected.control_at(&expected_scene, &path, extend(&expected_scene)));
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    assert!(actual.control_at(&actual_scene, &path, ControlAction::Select(2)));
    assert!(expected.control_at(&expected_scene, &path, ControlAction::Select(2)));
    assert_eq!(actual.get("current"), Ok(3_i64.into()));
    assert_eq!(expected.get_current(), 3);
    assert_eq!(actual.get("changes"), Ok(2_i64.into()));
    assert_eq!(expected.get_changes(), 2);
    assert_eq!(actual.get("selected-count"), Ok(1_i64.into()));
    assert_eq!(expected.get_selected_count(), 1);

    assert!(model.sort_by(|a, b| record_id(b).cmp(&record_id(a))));
    assert!(expected_model.sort_by(|a, b| b.id.cmp(&a.id)));
    let all = ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::All));
    assert!(actual.control_at(&actual_scene, &path, all));
    assert!(expected.control_at(&expected_scene, &path, all));
    assert_eq!(actual.get("selected-count"), Ok(3_i64.into()));
    assert_eq!(expected.get_selected_count(), 3);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tree_view_index_queries_sample_the_index_before_the_projection() {
    use std::{cell::RefCell, rc::Rc};

    for (property, read, input, expected, trace) in [
        (
            "queried-view-index",
            expressions_ui::TreeInputParity::get_queried_view_index
                as fn(&expressions_ui::TreeInputParity) -> i64,
            3,
            0,
            &["index", "root"][..],
        ),
        (
            "queried-view-index",
            expressions_ui::TreeInputParity::get_queried_view_index,
            -1,
            -1,
            &["index"][..],
        ),
        (
            "queried-source-index",
            expressions_ui::TreeInputParity::get_queried_source_index,
            0,
            3,
            &["index", "root"][..],
        ),
        (
            "queried-source-index",
            expressions_ui::TreeInputParity::get_queried_source_index,
            12,
            -1,
            &["index", "root"][..],
        ),
    ] {
        let (interpreted, mut compiled) = tree_input_fixture();
        let actual_calls = Rc::new(RefCell::new(Vec::new()));
        let compiled_calls = Rc::new(RefCell::new(Vec::new()));
        interpreted
            .on("query-key", {
                let calls = actual_calls.clone();
                Rc::new(move |_| {
                    calls.borrow_mut().push("index");
                    Ok(Some(input.into()))
                })
            })
            .unwrap();
        compiled.on_query_key({
            let calls = compiled_calls.clone();
            move |_| {
                calls.borrow_mut().push("index");
                input
            }
        });
        let root = |calls: &RefCell<Vec<&str>>| {
            let mut calls = calls.borrow_mut();
            let key = if calls.last() == Some(&"index") {
                1_i64
            } else {
                -1
            };
            calls.push("root");
            key
        };
        interpreted
            .on("query-root", {
                let calls = actual_calls.clone();
                Rc::new(move |_| Ok(Some(root(&calls).into())))
            })
            .unwrap();
        compiled.on_query_root({
            let calls = compiled_calls.clone();
            move || root(&calls)
        });
        assert_eq!(interpreted.get(property), Ok(expected.into()));
        assert_eq!(read(&compiled), expected);
        assert_eq!(actual_calls.borrow().as_slice(), trace);
        assert_eq!(compiled_calls.borrow().as_slice(), trace);
    }
}

#[test]
fn tree_queries_evaluate_the_key_before_sampling_the_tree() {
    use std::{cell::RefCell, rc::Rc};

    for (property, tag, read, expected_value) in [
        (
            "queried-depth",
            1,
            expressions_ui::TreeInputParity::get_queried_depth
                as fn(&expressions_ui::TreeInputParity) -> i64,
            0,
        ),
        (
            "queried-row",
            2,
            expressions_ui::TreeInputParity::get_queried_row,
            0,
        ),
        (
            "queried-expanded",
            3,
            |ui| i64::from(ui.get_queried_expanded()),
            0,
        ),
    ] {
        let (interpreted, mut compiled) = tree_input_fixture();
        let actual_trace = Rc::new(RefCell::new(Vec::new()));
        let compiled_trace = Rc::new(RefCell::new(Vec::new()));
        interpreted
            .on("query-key", {
                let trace = actual_trace.clone();
                Rc::new(move |args| {
                    assert_eq!(args, &[Value::Int(tag)]);
                    trace.borrow_mut().push("key");
                    Ok(Some(11_i64.into()))
                })
            })
            .unwrap();
        compiled.on_query_key({
            let trace = compiled_trace.clone();
            move |argument| {
                assert_eq!(argument, tag);
                trace.borrow_mut().push("key");
                11
            }
        });
        let branch = |trace: &RefCell<Vec<&str>>| {
            let mut trace = trace.borrow_mut();
            let root = if trace.last() == Some(&"key") { 1 } else { -1 };
            trace.push("root");
            root
        };
        interpreted
            .on("query-root", {
                let trace = actual_trace.clone();
                Rc::new(move |_| Ok(Some(branch(&trace).into())))
            })
            .unwrap();
        compiled.on_query_root({
            let trace = compiled_trace.clone();
            move || branch(&trace)
        });

        let value = if tag == 3 {
            Value::Bool(false)
        } else {
            Value::Int(expected_value)
        };
        assert_eq!(interpreted.get(property), Ok(value), "{property}");
        assert_eq!(read(&compiled), expected_value, "{property}");
        assert_eq!(*actual_trace.borrow(), ["key", "root"], "{property}");
        assert_eq!(*compiled_trace.borrow(), ["key", "root"], "{property}");
    }
}

#[test]
fn tree_input_rechecks_availability_and_model_before_committing() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction::Select;

    let (mut actual, mut expected) = tree_input_fixture();
    let path = [0];
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let expand = tree_disclosure(&actual_scene, &path, 1, true);
    let expected_expand = tree_disclosure(&expected_scene, &path, 1, true);
    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    assert!(!actual.control_at(&actual_scene, &path, expand));
    assert!(!expected.control_at(&expected_scene, &path, expected_expand));
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    assert!(actual.control_at(&actual_scene, &path, expand));
    assert!(expected.control_at(&expected_scene, &path, expected_expand));

    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records");
    };
    let model = records.model();
    let expected_model = expected.model_records();
    actual
        .on("read-key", {
            let model = Cell::new(Some(model.clone()));
            Rc::new(move |_| {
                if let Some(model) = model.take() {
                    assert!(model.remove(3).is_some());
                }
                Ok(Some((-1_i64).into()))
            })
        })
        .unwrap();
    expected.on_read_key({
        let model = Cell::new(Some(expected_model.clone()));
        move || {
            if let Some(model) = model.take() {
                assert!(model.remove(3).is_some());
            }
            -1
        }
    });
    assert_eq!(tree_keys(&actual_scene, &path), [1, 11, 2, 3]);
    assert!(!actual.control_at(&actual_scene, &path, Select(1)));
    assert!(!expected.control_at(&expected_scene, &path, Select(1)));
    assert_eq!(model.len(), 3);
    assert_eq!(expected_model.len(), 3);
    assert_eq!(actual.get("current"), Ok((-1_i64).into()));
    assert_eq!(expected.get_current(), -1);
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(expected.get_changes(), 0);
    assert_eq!(actual.get("expansions"), Ok(1_i64.into()));
    assert_eq!(expected.get_expansions(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tree_commands_reveal_paths_and_restore_current_on_collapse() {
    use tree_ui::TreeCommandsDemo;
    use xrack_layout::Size;

    let actual = interpret_tree_fixture("TreeCommandsDemo");
    let expected = TreeCommandsDemo::new();
    let size = Size::new(610.0, 440.0);
    let mut actual = ui_driver::UiDriver::new(actual, FixedText, size);
    let mut expected = ui_driver::UiDriver::new(expected, FixedText, size);
    let path = [0, 2];
    let compare = |actual: &ui_driver::UiDriver<Instance, FixedText>,
                   expected: &ui_driver::UiDriver<TreeCommandsDemo, FixedText>| {
        assert_eq!(
            tree_keys(actual.scene(), &path),
            tree_keys(expected.scene(), &path)
        );
        for (name, value) in [
            ("current", expected.component().get_current()),
            ("expansions", expected.component().get_expansions()),
            ("collapses", expected.component().get_collapses()),
            ("last-event-key", expected.component().get_last_event_key()),
        ] {
            assert_eq!(
                actual.component().get(name),
                Ok(Value::Int(value)),
                "{name}"
            );
        }
    };
    compare(&actual, &expected);
    actual.update(|ui| {
        ui.invoke("reveal", &[Value::Int(103)]).unwrap();
    });
    expected.update(|ui| ui.invoke_reveal(103));
    compare(&actual, &expected);
    let tree = actual
        .scene()
        .at_path(&path)
        .unwrap()
        .tree
        .as_ref()
        .unwrap();
    assert_eq!(tree.depth(103), Some(3));
    assert!(tree.is_expanded(10));
    assert_eq!(actual.component().get("current"), Ok(Value::Int(103)));
    let actual_scene = actual.scene().clone();
    let expected_scene = expected.scene().clone();
    actual.update(|ui| {
        assert!(ui.control_at(
            &actual_scene,
            &path,
            tree_disclosure(&actual_scene, &path, 100, false)
        ))
    });
    expected.update(|ui| {
        assert!(ui.control_at(
            &expected_scene,
            &path,
            tree_disclosure(&expected_scene, &path, 100, false)
        ))
    });
    compare(&actual, &expected);
    assert_eq!(actual.component().get("current"), Ok(Value::Int(100)));
    actual.update(|ui| assert!(ui.dispatch("collapse-all", "clicked")));
    expected.update(|ui| assert!(ui.dispatch("collapse-all", "clicked")));
    compare(&actual, &expected);
    assert_eq!(tree_keys(actual.scene(), &path), [1, 2]);
    assert_eq!(actual.component().get("current"), Ok(Value::Int(1)));
}

#[test]
fn tree_row_requests_clip_the_end_to_the_visible_rows() {
    let actual = interpret_tree_fixture("VisibilityTreeDemo");
    let mut generated = tree_ui::VisibilityTreeDemo::new();
    actual.invoke("initialize", &[]).unwrap();
    generated.invoke_initialize();
    let path = [1];
    let scene = actual.try_render().unwrap();
    let generated_scene = generated.render_shared();
    let count = scene.at_path(&path).unwrap().list.unwrap().row_count;
    assert!(count > 1);
    let wanted = actual.list_rows(&path, 1..count);
    assert_eq!(wanted.len(), count - 1);
    let actual_rows = actual.list_rows(&path, 1..usize::MAX);
    let generated_rows = generated.list_rows(&path, 1..usize::MAX);
    assert_eq!(actual_rows.len(), wanted.len());
    assert_eq!(generated_rows.len(), wanted.len());
    for ((actual, generated), expected) in actual_rows.iter().zip(&generated_rows).zip(&wanted) {
        assert_same_scene(actual, expected);
        assert_same_scene(actual, generated);
    }

    actual.set("query", "发布说明".to_owned().into()).unwrap();
    generated.set_query("发布说明".into());
    let visible = actual.list_rows(&path, 0..usize::MAX);
    assert!(!visible.is_empty() && visible.len() < count);
    let from_old_scene = scene.list_rows_at(&path, 0..usize::MAX);
    let from_old_generated = generated_scene.list_rows_at(&path, 0..usize::MAX);
    assert_eq!(from_old_scene.len(), visible.len());
    assert_eq!(from_old_generated.len(), visible.len());
    for ((actual, generated), expected) in
        from_old_scene.iter().zip(&from_old_generated).zip(&visible)
    {
        assert_same_scene(actual, expected);
        assert_same_scene(actual, generated);
    }
}

#[test]
fn tree_filter_and_branch_changes_preserve_identity_and_reject_hidden_edits() {
    use tree_ui::VisibilityTreeDemo;
    use xrack_layout::{Interaction, Size};

    let mut actual = interpret_tree_fixture("VisibilityTreeDemo");
    let mut expected = VisibilityTreeDemo::new();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records")
    };
    let model = records.model();
    let expected_model = expected.model_records();
    let size = Size::new(540.0, 312.0);
    let input = Interaction::default();
    let path = [1];
    actual.invoke("initialize", &[]).unwrap();
    expected.invoke_initialize();
    actual.invoke("select-source", &[Value::Int(2)]).unwrap();
    expected.invoke_select_source(2);
    assert_eq!(
        actual.get("current"),
        Ok(Value::Int(expected.get_current()))
    );
    assert_eq!(actual.get("current"), Ok(Value::Int(3)));
    let (before, _) = prepare_list(&actual, &input, size);
    let (expected_before, _) = prepare_list(&expected, &input, size);
    assert_eq!(tree_keys(&before, &path), [1, 2, 3, 4, 5]);
    let leaf = model.row_key(2).unwrap();
    let field = ["visibility".into(), "shown".into()];
    let set_shown = |shown| {
        let row = model
            .row(1)
            .unwrap()
            .with_field(&field, Value::Bool(shown))
            .unwrap();
        assert!(model.set_row(1, row));
        let mut row = expected_model.row(1).unwrap();
        row.visibility.shown = shown;
        assert!(expected_model.set_row(1, row));
    };
    set_shown(false);
    let leaf_path = [1, 2, 0, 0];
    assert!(!actual.commit_table_cell(&before, &leaf_path, "计划文档", "Stale edit".into()));
    assert!(!expected.commit_table_cell(
        &expected_before,
        &leaf_path,
        "计划文档",
        "Stale edit".into()
    ));
    let (hidden, _) = prepare_list(&actual, &input, size);
    let (expected_hidden, _) = prepare_list(&expected, &input, size);
    assert_eq!(tree_keys(&hidden, &path), [1, 4, 5]);
    assert_eq!(
        tree_keys(&hidden, &path),
        tree_keys(&expected_hidden, &path)
    );
    assert_eq!(actual.get("visible-index"), Ok(Value::Int(-1)));
    set_shown(true);
    actual.set("branch-root", Value::Int(1)).unwrap();
    expected.set_branch_root(1);
    actual.set("query", "计划".to_owned().into()).unwrap();
    expected.set_query("计划".into());
    let (filtered, _) = prepare_list(&actual, &input, size);
    let (expected_filtered, _) = prepare_list(&expected, &input, size);
    assert_eq!(tree_keys(&filtered, &path), [2, 3]);
    assert_eq!(
        tree_keys(&filtered, &path),
        tree_keys(&expected_filtered, &path)
    );
    let node = filtered.at_path(&path).unwrap();
    assert!(node.tree.as_ref().unwrap().is_filtering());
    assert_eq!(node.tree_selection.as_ref().unwrap().get(&3), Some(&leaf));
    assert_eq!(
        actual.get("visible-index"),
        Ok(Value::Int(expected.get_visible_index()))
    );
    assert_eq!(
        actual.get("source-index"),
        Ok(Value::Int(expected.get_source_index()))
    );

    actual.set("query", String::new().into()).unwrap();
    expected.set_query(String::new());
    actual.set("branch-root", Value::Int(-1)).unwrap();
    expected.set_branch_root(-1);
    actual
        .set(
            "records",
            Value::RecordList(xrack_runtime::RecordList::new(
                "VisibilityBranch",
                model.snapshot(),
            )),
        )
        .unwrap();
    expected.set_records(expected_model.snapshot());
    let (replaced, _) = prepare_list(&actual, &input, size);
    let (expected_replaced, _) = prepare_list(&expected, &input, size);
    assert_eq!(tree_keys(&replaced, &path), [1, 5]);
    assert_eq!(
        tree_keys(&replaced, &path),
        tree_keys(&expected_replaced, &path)
    );
    assert!(
        replaced
            .at_path(&path)
            .unwrap()
            .tree_selection
            .as_ref()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_live_rows_report_event_errors_and_repaired_rows_keep_their_routes() {
    use xrack_layout::{Interaction, Size};

    let input = Interaction::default();
    let mut list = interpret_list_fixture("ListVisibilityDemo");
    let mut compiled_list = list_ui::ListVisibilityDemo::new();
    let Value::RecordList(records) = list.get("records").unwrap() else {
        panic!("expected list records");
    };
    let rows = records.model();
    let valid = rows.row(2).unwrap();
    let key = rows.row_key(2);
    let size = Size::new(660.0, 320.0);
    let (scene, _) = prepare_list(&list, &input, size);
    let (expected, _) = prepare_list(&compiled_list, &input, size);
    let require_error = |ui: &Instance, path: &str| {
        let error = ui
            .take_error()
            .expect("invalid event data must report an error");
        assert!(error.to_string().contains(path), "{error}");
    };
    let click = [2, 1, 0];
    assert!(rows.set_row(
        2,
        valid.with_field(&["height".into()], true.into()).unwrap()
    ));
    assert!(!list.dispatch_at(&scene, &click, "clicked"));
    require_error(&list, "model ListVisibilityRecord[2].height");
    assert_eq!(list.get("last-source-index"), Ok(Value::Int(-1)));
    assert!(list.list_rows(&[2], 0..2).is_empty());
    require_error(&list, "model ListVisibilityRecord[2].height");
    assert!(rows.set_row(2, valid));
    assert_eq!(rows.row_key(2), key);
    assert!(list.dispatch_at(&scene, &click, "clicked"));
    assert!(compiled_list.dispatch_at(&expected, &click, "clicked"));
    assert_eq!(
        list.get("last-source-index"),
        Ok(compiled_list.get_last_source_index().into())
    );
    assert_eq!(list.take_error(), None);

    let mut tree = interpret_tree_fixture("VisibilityTreeDemo");
    let mut compiled_tree = tree_ui::VisibilityTreeDemo::new();
    tree.invoke("initialize", &[]).unwrap();
    compiled_tree.invoke_initialize();
    let Value::RecordList(records) = tree.get("records").unwrap() else {
        panic!("expected tree records");
    };
    let rows = records.model();
    let valid = rows.row(2).unwrap();
    let size = Size::new(540.0, 312.0);
    let (scene, _) = prepare_list(&tree, &input, size);
    let (expected, _) = prepare_list(&compiled_tree, &input, size);
    let cell = [1, 2, 0, 0];
    assert!(rows.set_row(2, valid.with_field(&["label".into()], true.into()).unwrap()));
    assert!(!tree.commit_table_cell(&scene, &cell, "计划文档", "Repair check".into()));
    require_error(&tree, "model VisibilityBranch[2].label");
    assert!(rows.set_row(2, valid));
    assert!(tree.commit_table_cell(&scene, &cell, "计划文档", "Repair check".into()));
    assert!(compiled_tree.commit_table_cell(&expected, &cell, "计划文档", "Repair check".into()));
    assert_eq!(
        rows.row(2).unwrap().field("label"),
        Some(&Value::String(compiled_tree.get_records()[2].label.clone()))
    );
    assert_eq!(tree.take_error(), None);
}

#[test]
fn tree_cell_stops_reading_descendants_when_the_row_invalidates_input() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TreeEditScopeParity")
        .unwrap();
    let mut compiled = expressions_ui::TreeEditScopeParity::new();
    let input = xrack_layout::Interaction::default();
    let size = xrack_layout::Size::new(320.0, 200.0);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&compiled, &input, size);
    let path = [0, 0, 0, 0];
    assert_eq!(actual.at_path(&path).unwrap().kind, "TreeCell");
    assert_same_scene(&actual, &expected);

    let actual_reads = Rc::new(Cell::new(0));
    let compiled_reads = Rc::new(Cell::new(0));
    interpreted
        .on("cell-enabled", {
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    compiled.on_cell_enabled({
        let reads = compiled_reads.clone();
        move || {
            reads.set(reads.get() + 1);
            true
        }
    });
    interpreted
        .on("row-visible", {
            let owner = interpreted.as_weak();
            Rc::new(move |_| {
                owner.upgrade().unwrap().set("shown", false.into()).unwrap();
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    compiled.on_row_visible({
        let owner = compiled.as_weak();
        move || {
            owner.upgrade().unwrap().set_shown(false);
            true
        }
    });
    assert!(!interpreted.commit_table_cell(&actual, &path, "original", "discard".into()));
    assert!(!compiled.commit_table_cell(&expected, &path, "original", "discard".into()));
    assert_eq!((actual_reads.get(), compiled_reads.get()), (0, 0));
    let rows = interpreted.record_model("rows").unwrap();
    let label = || rows.model().row(0).unwrap().field("label").cloned();
    assert_eq!(label(), Some("original".to_owned().into()));
    assert_eq!(compiled.get_rows()[0].label, "original");

    interpreted
        .on("row-visible", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    compiled.on_row_visible(|| true);
    interpreted.set("shown", true.into()).unwrap();
    compiled.set_shown(true);
    assert!(interpreted.commit_table_cell(&actual, &path, "original", "saved".into()));
    assert!(compiled.commit_table_cell(&expected, &path, "original", "saved".into()));
    assert_eq!((actual_reads.get(), compiled_reads.get()), (1, 1));
    assert_eq!(label(), Some("saved".to_owned().into()));
    assert_eq!(compiled.get_rows()[0].label, "saved");
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn tree_delegate_edit_and_callback_use_the_source_row() {
    use tree_ui::DelegateTreeDemo;
    use xrack_layout::{Interaction, Size};

    let mut actual = interpret_tree_fixture("DelegateTreeDemo");
    let mut expected = DelegateTreeDemo::new();
    let size = Size::new(360.0, 250.0);
    let input = Interaction::default();
    let (before, _) = prepare_list(&actual, &input, size);
    let (expected_before, _) = prepare_list(&expected, &input, size);
    assert!(actual.control_at(&before, &[0], tree_disclosure(&before, &[0], 1, true)));
    assert!(expected.control_at(
        &expected_before,
        &[0],
        tree_disclosure(&expected_before, &[0], 1, true)
    ));
    let (expanded, _) = prepare_list(&actual, &input, size);
    let (expected_expanded, _) = prepare_list(&expected, &input, size);
    let editor = [0, 1, 0, 0, 0, 0];
    assert_eq!(
        expanded.at_path(&editor).unwrap().text("text"),
        Some("子记录")
    );
    assert!(actual.edit_at(&expanded, &editor, "Edited".into()));
    assert!(expected.edit_at(&expected_expanded, &editor, "Edited".into()));
    assert!(actual.dispatch_at(&expanded, &editor, "accepted"));
    assert!(expected.dispatch_at(&expected_expanded, &editor, "accepted"));
    assert_eq!(
        actual.get("committed-index"),
        Ok(Value::Int(expected.get_committed_index()))
    );
    assert_eq!(actual.get("committed-index"), Ok(Value::Int(0)));
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records")
    };
    assert_eq!(
        records.model().row(0).unwrap().field("label"),
        Some(&Value::String(
            expected.model_records().row(0).unwrap().label
        ))
    );

    assert!(actual.control_at(&expanded, &[0], tree_disclosure(&expanded, &[0], 1, false)));
    assert!(expected.control_at(
        &expected_expanded,
        &[0],
        tree_disclosure(&expected_expanded, &[0], 1, false)
    ));
    assert_eq!(
        [
            actual.edit_at(&expanded, &editor, "stale edit".into()),
            expected.edit_at(&expected_expanded, &editor, "stale edit".into()),
        ],
        [false, false]
    );
    assert!(!actual.dispatch_at(&expanded, &editor, "accepted"));
    assert!(!expected.dispatch_at(&expected_expanded, &editor, "accepted"));
    assert_eq!(
        records.model().row(0).unwrap().field("label"),
        Some(&"Edited".to_owned().into())
    );
    assert_eq!(expected.model_records().row(0).unwrap().label, "Edited");
    assert_eq!(actual.take_error(), None);

    let (collapsed, _) = prepare_list(&actual, &input, size);
    let (expected_collapsed, _) = prepare_list(&expected, &input, size);
    assert!(actual.control_at(&collapsed, &[0], tree_disclosure(&collapsed, &[0], 1, true)));
    assert!(expected.control_at(
        &expected_collapsed,
        &[0],
        tree_disclosure(&expected_collapsed, &[0], 1, true)
    ));
    let (reopened, _) = prepare_list(&actual, &input, size);
    let (expected_reopened, _) = prepare_list(&expected, &input, size);
    assert!(actual.edit_at(&reopened, &editor, "Edited again".into()));
    assert!(expected.edit_at(&expected_reopened, &editor, "Edited again".into()));
    assert_eq!(
        records.model().row(0).unwrap().field("label"),
        Some(&Value::String(
            expected.model_records().row(0).unwrap().label
        ))
    );
}

#[test]
fn tree_row_height_projection_keeps_topology_cached() {
    use std::sync::Arc;
    use tree_ui::SizedTreeDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{Length, ListMetrics};

    let mut actual = interpret_tree_fixture("SizedTreeDemo");
    let mut expected = SizedTreeDemo::new();
    let size = Size::new(320.0, 240.0);
    let input = Interaction::default();
    let (before, _) = prepare_list(&actual, &input, size);
    let (expected_before, _) = prepare_list(&expected, &input, size);
    assert!(actual.control_at(&before, &[0], tree_disclosure(&before, &[0], 1, true)));
    assert!(expected.control_at(
        &expected_before,
        &[0],
        tree_disclosure(&expected_before, &[0], 1, true)
    ));
    let (expanded, _) = prepare_list(&actual, &input, size);
    let (expected_expanded, _) = prepare_list(&expected, &input, size);
    let node = expanded.child_at(0).unwrap();
    assert_eq!(ListMetrics::row_height_at(node, 1), 76.0);
    assert_eq!(
        ListMetrics::row_height_at(expected_expanded.child_at(0).unwrap(), 1),
        76.0
    );
    let tree = node.tree.as_ref().unwrap().clone();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records")
    };
    let model = records.model();
    let row = model
        .row(1)
        .unwrap()
        .with_field(&["height".into()], Length(92.0).into())
        .unwrap();
    assert!(model.set_row(1, row));
    let (updated, _) = prepare_list(&actual, &input, size);
    let node = updated.child_at(0).unwrap();
    assert_eq!(ListMetrics::row_height_at(node, 1), 92.0);
    assert!(Arc::ptr_eq(&tree, node.tree.as_ref().unwrap()));
}

#[test]
fn queued_tree_scroll_tracks_source_identity_across_expansion_and_insertion() {
    use tree_ui::{BranchVisibility, VisibilityBranch, VisibilityTreeDemo};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::ScrollRequests;

    let actual = interpret_tree_fixture("VisibilityTreeDemo");
    let mut expected = VisibilityTreeDemo::new();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected records")
    };
    let model = records.model();
    let expected_model = expected.model_records();
    let key_field = ["key".into()];
    let label_field = ["label".into()];
    let dynamic = |key, label: &str| {
        model
            .row(0)
            .unwrap()
            .with_field(&key_field, Value::Int(key))
            .unwrap()
            .with_field(&label_field, Value::String(label.into()))
            .unwrap()
    };
    let compiled = |key, label: &str| VisibilityBranch {
        key,
        parent: -1,
        label: label.into(),
        visibility: BranchVisibility { shown: true },
    };
    for key in 6..40 {
        model.push(dynamic(key, "Row"));
        expected_model.push(compiled(key, "Row"));
    }
    let key = model.row_key(2).unwrap();
    let expected_key = expected_model.row_key(2).unwrap();
    let requests = ScrollRequests::default();
    let expected_requests = ScrollRequests::default();
    {
        let _active = requests.enter();
        actual.invoke("locate-source", &[Value::Int(2)]).unwrap();
    }
    {
        let _active = expected_requests.enter();
        expected.invoke_locate_source(2);
    }
    actual.invoke("initialize", &[]).unwrap();
    expected.invoke_initialize();
    assert!(model.insert(0, dynamic(100, "Inserted")));
    assert!(expected_model.insert(0, compiled(100, "Inserted")));
    let size = Size::new(540.0, 312.0);
    let mut input = Interaction::default();
    let mut expected_input = Interaction::default();
    let (scene, layout) = prepare_list(&actual, &input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    assert!(expected_input.apply_scroll_requests(
        &expected_scene,
        &expected_layout,
        expected_requests.drain()
    ));
    let (scene, layout) = prepare_list(&actual, &input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    assert_eq!(scene.at_path(&[1, 0]).unwrap().row_key, Some(key));
    assert_eq!(
        expected_scene.at_path(&[1, 0]).unwrap().row_key,
        Some(expected_key)
    );
    let offset = layout.at_path(&[1]).unwrap().scroll.unwrap().offset;
    assert_eq!(
        offset,
        expected_layout
            .at_path(&[1])
            .unwrap()
            .scroll
            .unwrap()
            .offset
    );
    assert_eq!(offset, 84.0);
}

fn interpret_split_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/split/split.xr");
    let preview = xrack_compiler::prepare_preview(source, "split.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

fn split_widths(layout: &xrack_layout::LayoutNode) -> [f32; 3] {
    let split = layout.at_path(&[0, 2]).unwrap();
    std::array::from_fn(|index| split.children[index].bounds.width)
}

#[test]
fn split_drag_and_dsl_restore_match_compiled_panes() {
    use split_ui::SplitDemo;
    use xrack_layout::{Interaction, Point, Size};

    let mut actual = interpret_split_fixture("SplitDemo");
    let mut expected = SplitDemo::new();
    let size = Size::new(760.0, 420.0);
    let path = [0, 2];
    fn drag(component: &mut impl Component, size: Size, path: &[usize], offset: f32) -> [f32; 3] {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(component, &input, size);
        let bounds = layout
            .at_path(path)
            .unwrap()
            .split
            .as_ref()
            .unwrap()
            .handles[0]
            .bounds;
        let point = Point {
            x: bounds.x + 3.0,
            y: bounds.y + 12.0,
        };
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        input.split_press(&scene, &layout);
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: point.x + offset,
                y: point.y,
            },
        );
        let drag = input.split_drag(&scene, &layout).unwrap();
        assert!(component.resize_split_at(&scene, &drag.path, &drag.weights));
        assert!(!component.resize_split_at(&scene, &drag.path, &drag.weights));
        assert!(input.finish_split_resize());
        split_widths(&prepare_list(component, &input, size).1)
    }
    let widths = drag(&mut actual, size, &path, 40.0);
    assert_eq!(widths, drag(&mut expected, size, &path, 40.0));
    assert!(actual.dispatch("save-layout", "clicked"));
    assert!(expected.dispatch("save-layout", "clicked"));
    let saved = expected.get_saved_layout();
    assert_eq!(actual.get("saved-layout"), Ok(saved.into()));
    assert_ne!(drag(&mut actual, size, &path, -25.0), widths);
    assert_ne!(drag(&mut expected, size, &path, -25.0), widths);
    assert!(actual.dispatch("restore-layout", "clicked"));
    assert!(expected.dispatch("restore-layout", "clicked"));
    assert_eq!(actual.get("edits"), Ok(Value::Int(expected.get_edits())));
    assert_eq!(actual.get("edits"), Ok(Value::Int(3)));
    let input = Interaction::default();
    assert_eq!(split_widths(&prepare_list(&actual, &input, size).1), widths);
    assert_eq!(
        split_widths(&prepare_list(&expected, &input, size).1),
        widths
    );
}

#[test]
fn fixed_split_sizes_follow_declared_slots_and_reject_stale_updates() {
    use split_ui::FillSplitDemo;
    use xrack_layout::{Interaction, NavigationKey, Size};

    let mut actual = interpret_split_fixture("FillSplitDemo");
    let mut expected = FillSplitDemo::new();
    let size = Size::new(680.0, 320.0);
    let wide = Size::new(880.0, 320.0);
    let path = [0, 2];
    fn resize(component: &mut impl Component, size: Size) -> [f32; 3] {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(component, &input, size);
        assert_eq!(split_widths(&layout), [120.0, 356.0, 160.0]);
        let handle = input
            .accessibility_target(&scene, &layout, &[0, 2, 3])
            .unwrap();
        input.focus_target(&scene, &layout, handle);
        let resize = input
            .split_key(&scene, &layout, NavigationKey::Right)
            .unwrap();
        let sizes = resize.sizes.as_ref().unwrap();
        assert_eq!(sizes.as_slice(), &[(0, 128.0), (1, 348.0)]);
        assert!(!component.resize_split_at(&scene, &resize.path, &resize.weights));
        assert!(component.resize_split_sizes_at(&scene, &resize.path, sizes));
        assert!(!component.resize_split_sizes_at(&scene, &resize.path, sizes));
        split_widths(&prepare_list(component, &input, size).1)
    }
    assert_eq!(resize(&mut actual, size), resize(&mut expected, size));
    assert!(actual.dispatch("save", "clicked"));
    assert!(expected.dispatch("save", "clicked"));
    assert_eq!(
        actual.get("saved-layout"),
        Ok(expected.get_saved_layout().into())
    );
    let input = Interaction::default();
    assert_eq!(
        split_widths(&prepare_list(&actual, &input, wide).1),
        [128.0, 548.0, 160.0]
    );
    actual.set("show-editor", false.into()).unwrap();
    expected.set_show_editor(false);
    let (scene, layout) = prepare_list(&actual, &input, size);
    let (_, expected_layout) = prepare_list(&expected, &input, size);
    assert_eq!(split_widths(&layout), split_widths(&expected_layout));
    assert!(!actual.resize_split_sizes_at(&scene, &path, &[(0, 120.0), (1, 348.0)]));
}

#[test]
fn custom_split_handle_uses_its_own_hover_and_press_state() {
    use split_ui::CustomSplitDemo;
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::Color;

    let actual = interpret_split_fixture("CustomSplitDemo");
    let mut expected = CustomSplitDemo::new();
    let size = Size::new(640.0, 240.0);
    let handle = [0, 3];
    let color = |component: &dyn Component, slot| {
        component
            .render_shared()
            .at_path(&[0, slot, 0])
            .unwrap()
            .color("background")
            .unwrap()
    };
    fn press(component: &impl Component, size: Size, handle: &[usize]) -> Interaction {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(component, &input, size);
        let bounds = layout.at_path(handle).unwrap().bounds;
        let point = Point {
            x: bounds.x + 5.0,
            y: bounds.y + 32.0,
        };
        input.pointer_move(&scene, &layout, point);
        assert_eq!(
            component
                .render_shared()
                .at_path(&[0, 3, 0])
                .unwrap()
                .color("background"),
            Some(Color(0x58c9c7ff))
        );
        input.pointer_press_in_scene(&scene, &layout);
        input.split_press(&scene, &layout);
        input
    }
    assert_eq!(color(&actual, 3), color(&expected, 3));
    assert_eq!(color(&actual, 4), color(&expected, 4));
    let mut input = press(&actual, size, &handle);
    let mut expected_input = press(&expected, size, &handle);
    assert_eq!(actual.get("dragging"), Ok(expected.get_dragging().into()));
    assert_eq!(actual.get("dragging"), Ok(Value::Bool(true)));
    assert_eq!(color(&actual, 3), Color(0xffbf69ff));
    assert_eq!(color(&actual, 3), color(&expected, 3));
    assert_eq!(color(&actual, 4), color(&expected, 4));
    actual.set("show-center", false.into()).unwrap();
    expected.set_show_center(false);
    let (scene, layout) = prepare_list(&actual, &input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    input.reconcile(&scene, &layout);
    expected_input.reconcile(&expected_scene, &expected_layout);
    assert_eq!(actual.get("dragging"), Ok(expected.get_dragging().into()));
    assert_eq!(actual.get("dragging"), Ok(Value::Bool(false)));
    assert!(scene.at_path(&[0, 4, 0]).is_none());
}

fn interpret_controls_fixture(name: &str) -> Instance {
    let source = include_str!("../examples/controls/controls.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

fn sync_test_timers(
    queue: &mut xrack_runtime::TimerQueue,
    component: &impl Component,
    now: std::time::Instant,
) {
    fn collect<'a>(
        scene: &'a Node,
        layout: &xrack_layout::LayoutNode,
        registrations: &mut Vec<&'a xrack_runtime::TimerRegistration>,
    ) {
        registrations.extend(layout.timer_registrations(scene));
        for (child, layout) in scene.children.iter().zip(&layout.children) {
            collect(child, layout, registrations);
        }
    }
    let input = xrack_layout::Interaction::default();
    let (scene, layout) = prepare_list(component, &input, xrack_layout::Size::new(460.0, 240.0));
    let mut registrations = Vec::new();
    collect(&scene, &layout, &mut registrations);
    queue.sync(now, registrations);
}

#[test]
fn timer_start_stop_restart_and_one_shot_match_compiled_delivery() {
    use controls_ui::TimerDemo;
    use std::time::{Duration, Instant};
    use xrack_runtime::TimerQueue;

    let mut actual = interpret_controls_fixture("TimerDemo");
    let mut expected = TimerDemo::new();
    let mut actual_queue = TimerQueue::default();
    let mut expected_queue = TimerQueue::default();
    let started = Instant::now();
    let time = |ms| started + Duration::from_millis(ms);
    let compare = |actual: &Instance, expected: &TimerDemo| {
        for (name, value) in [
            ("remaining", Value::Int(expected.get_remaining())),
            ("pulses", Value::Int(expected.get_pulses())),
            ("ticking", Value::Bool(expected.get_ticking())),
            ("pulse-pending", Value::Bool(expected.get_pulse_pending())),
        ] {
            assert_eq!(actual.get(name), Ok(value), "{name}");
        }
    };
    sync_test_timers(&mut actual_queue, &actual, started);
    sync_test_timers(&mut expected_queue, &expected, started);
    assert_eq!(
        actual.render_shared().children.len(),
        expected.render_shared().children.len()
    );
    assert_eq!(actual_queue.deadline(), Some(time(250)));
    assert_eq!(actual_queue.deadline(), expected_queue.deadline());
    assert!(actual_queue.next_due(time(249)).is_none());
    assert!(actual_queue.next_due(time(250)).unwrap().fire());
    assert!(expected_queue.next_due(time(250)).unwrap().fire());
    compare(&actual, &expected);

    assert!(actual.dispatch("pause-countdown", "clicked"));
    assert!(expected.dispatch("pause-countdown", "clicked"));
    sync_test_timers(&mut actual_queue, &actual, time(300));
    sync_test_timers(&mut expected_queue, &expected, time(300));
    assert!(actual_queue.next_due(time(900)).is_none());
    assert!(expected_queue.next_due(time(900)).is_none());

    assert!(actual.dispatch("start-pulse", "clicked"));
    assert!(expected.dispatch("start-pulse", "clicked"));
    sync_test_timers(&mut actual_queue, &actual, time(900));
    sync_test_timers(&mut expected_queue, &expected, time(900));
    assert!(actual_queue.next_due(time(1020)).unwrap().fire());
    assert!(expected_queue.next_due(time(1020)).unwrap().fire());
    compare(&actual, &expected);
    assert_eq!(actual.get("pulse-pending"), Ok(Value::Bool(false)));
    assert_eq!(actual.get("pulses"), Ok(Value::Int(1)));

    assert!(actual.dispatch("resume-countdown", "clicked"));
    assert!(expected.dispatch("resume-countdown", "clicked"));
    sync_test_timers(&mut actual_queue, &actual, time(1100));
    sync_test_timers(&mut expected_queue, &expected, time(1100));
    assert!(actual.dispatch("resume-countdown", "clicked"));
    assert!(expected.dispatch("resume-countdown", "clicked"));
    sync_test_timers(&mut actual_queue, &actual, time(1200));
    sync_test_timers(&mut expected_queue, &expected, time(1200));
    assert_eq!(actual_queue.deadline(), Some(time(1350)));
    assert_eq!(actual_queue.deadline(), expected_queue.deadline());
    let stale = actual_queue.next_due(time(1350)).unwrap();
    let expected_stale = expected_queue.next_due(time(1350)).unwrap();
    assert!(actual.dispatch("restart-countdown", "clicked"));
    assert!(expected.dispatch("restart-countdown", "clicked"));
    assert!(!stale.fire());
    assert!(!expected_stale.fire());
    compare(&actual, &expected);
    assert_eq!(actual.get("remaining"), Ok(Value::Int(4)));
    sync_test_timers(&mut actual_queue, &actual, time(1350));
    sync_test_timers(&mut expected_queue, &expected, time(1350));
    assert_eq!(actual_queue.deadline(), Some(time(1600)));
    assert_eq!(actual_queue.deadline(), expected_queue.deadline());
}

#[test]
fn timer_restart_during_config_evaluation_rejects_the_old_tick() {
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_runtime::TimerQueue;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TimerRevisionParity")
        .unwrap();
    let mut generated = expressions_ui::TimerRevisionParity::new();
    let mut actual_queue = TimerQueue::default();
    let mut generated_queue = TimerQueue::default();
    let start = Instant::now();
    actual_queue.sync(start, &actual.render_shared().timers);
    generated_queue.sync(start, &generated.render_shared().timers);
    let due = start + Duration::from_millis(10);
    let actual_tick = actual_queue.next_due(due).unwrap();
    let generated_tick = generated_queue.next_due(due).unwrap();
    actual
        .on("interval", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.invoke("restart", &[]).unwrap();
                }
                Ok(Some(10.into()))
            })
        })
        .unwrap();
    generated.on_interval({
        let owner = generated.as_weak();
        let restart = Cell::new(true);
        move || {
            if restart.replace(false) {
                owner.upgrade().unwrap().invoke_restart();
            }
            10
        }
    });
    let rejected = [!actual_tick.fire(), !generated_tick.fire()];
    assert_eq!(rejected, [true, true]);
    assert_eq!(actual.get("ticks"), Ok(0_i64.into()));
    assert_eq!(generated.get_ticks(), 0);
    assert_eq!(actual.get("running"), Ok(true.into()));
    assert!(generated.get_running());

    actual_queue.sync(due, &actual.render_shared().timers);
    generated_queue.sync(due, &generated.render_shared().timers);
    let next = due + Duration::from_millis(10);
    assert_eq!(actual_queue.deadline(), Some(next));
    assert_eq!(generated_queue.deadline(), Some(next));
    assert!(actual_queue.next_due(next).unwrap().fire());
    assert!(generated_queue.next_due(next).unwrap().fire());
    assert_eq!(actual.get("ticks"), Ok(1_i64.into()));
    assert_eq!(generated.get_ticks(), 1);
    assert_eq!(actual.get("running"), Ok(false.into()));
    assert!(!generated.get_running());
    assert_eq!(actual.get("running-at-tick"), Ok(false.into()));
    assert!(!generated.get_running_at_tick());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn timer_start_only_samples_running_when_it_needs_to() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TimerCommandReadParity")
        .unwrap();
    let mut generated = expressions_ui::TimerCommandReadParity::new();
    let actual_reads = Rc::new(Cell::new(0));
    let generated_reads = Rc::new(Cell::new(0));
    let read = |reads: &Cell<i32>| {
        reads.set(reads.get() + 1);
        true
    };
    actual
        .on("initial-running", {
            let reads = actual_reads.clone();
            Rc::new(move |_| Ok(Some(read(&reads).into())))
        })
        .unwrap();
    generated.on_initial_running({
        let reads = generated_reads.clone();
        move || read(&reads)
    });

    actual.invoke("restart", &[]).unwrap();
    generated.invoke_restart();
    assert_eq!((actual_reads.get(), generated_reads.get()), (0, 0));
    actual.reset("running").unwrap();
    generated.reset_running();
    actual.invoke("stop", &[]).unwrap();
    generated.invoke_stop();
    assert_eq!((actual_reads.get(), generated_reads.get()), (0, 0));
    actual.reset("running").unwrap();
    generated.reset_running();
    actual.invoke("start", &[]).unwrap();
    generated.invoke_start();
    assert_eq!((actual_reads.get(), generated_reads.get()), (1, 1));
    assert_eq!(actual.get("running"), Ok(true.into()));
    assert!(generated.get_running());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn delay_button_uses_the_same_hold_cancellation_and_activation_as_compiled_ui() {
    use controls_ui::DelayDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::ControlAction;

    let mut actual = interpret_controls_fixture("DelayDemo");
    let mut expected = DelayDemo::new();
    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let size = Size::new(400.0, 214.0);
    let started = Instant::now();
    let time = |ms| started + Duration::from_millis(ms);
    let path = [1];
    fn press(ui: &impl Component, input: &mut Interaction, size: Size, now: Instant) {
        let (scene, layout) = prepare_list(ui, input, size);
        input.pointer_move(&scene, &layout, Point { x: 90.0, y: 79.0 });
        input.pointer_press_in_scene(&scene, &layout);
        input.begin_pointer_delay(&scene, &layout, now);
    }
    press(&actual, &mut actual_input, size, started);
    press(&expected, &mut expected_input, size, started);
    for input in [&mut actual_input, &mut expected_input] {
        assert_eq!(input.delay_deadline(), Some(time(800)));
        assert!(input.pointer_release_target().is_none());
        assert!(input.delay_deadline().is_none());
    }

    press(&actual, &mut actual_input, size, time(1000));
    press(&expected, &mut expected_input, size, time(1000));
    actual.set("allow-confirm", false.into()).unwrap();
    expected.set_allow_confirm(false);
    let (scene, layout) = prepare_list(&actual, &actual_input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    assert!(
        actual_input
            .advance_delay(&scene, &layout, time(1800))
            .is_none()
    );
    assert!(
        expected_input
            .advance_delay(&expected_scene, &expected_layout, time(1800))
            .is_none()
    );

    actual.set("allow-confirm", true.into()).unwrap();
    expected.set_allow_confirm(true);
    press(&actual, &mut actual_input, size, time(2000));
    press(&expected, &mut expected_input, size, time(2000));
    let (scene, layout) = prepare_list(&actual, &actual_input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    assert!(
        actual_input
            .advance_delay(&scene, &layout, time(2799))
            .is_none()
    );
    assert!(
        expected_input
            .advance_delay(&expected_scene, &expected_layout, time(2799))
            .is_none()
    );
    let action = actual_input
        .advance_delay(&scene, &layout, time(2800))
        .unwrap();
    let expected_action = expected_input
        .advance_delay(&expected_scene, &expected_layout, time(2800))
        .unwrap();
    assert_eq!(action.action, ControlAction::DelayButton(true));
    actual.set("hold-time", 1_600_i64.into()).unwrap();
    expected.set_hold_time(1_600);
    assert!(!actual.control_at(&scene, &action.path, action.action));
    assert!(!expected.control_at(
        &expected_scene,
        &expected_action.path,
        expected_action.action
    ));
    actual.set("hold-time", 800_i64.into()).unwrap();
    expected.set_hold_time(800);
    actual.set("allow-confirm", false.into()).unwrap();
    expected.set_allow_confirm(false);
    assert!(!actual.control_at(&scene, &action.path, action.action));
    assert!(!expected.control_at(
        &expected_scene,
        &expected_action.path,
        expected_action.action
    ));
    actual.set("allow-confirm", true.into()).unwrap();
    expected.set_allow_confirm(true);
    assert!(actual.control_at(&scene, &action.path, action.action));
    assert!(expected.control_at(
        &expected_scene,
        &expected_action.path,
        expected_action.action
    ));
    assert!(actual_input.pointer_release_target().is_none());
    assert!(expected_input.pointer_release_target().is_none());
    assert_eq!(actual.get("confirmed"), Ok(expected.get_confirmed().into()));
    assert_eq!(
        actual.get("activations"),
        Ok(Value::Int(expected.get_activations()))
    );
    assert_eq!(actual.get("activations"), Ok(Value::Int(1)));
    assert!(!actual.control_at(
        &actual.render_shared(),
        &path,
        ControlAction::DelayButton(true)
    ));
    let reset_scene = actual.render_shared();
    let compiled_reset_scene = expected.render_shared();
    actual.set("hold-time", 1_600_i64.into()).unwrap();
    expected.set_hold_time(1_600);
    assert!(actual.control_at(&reset_scene, &path, ControlAction::DelayButton(false)));
    assert!(expected.control_at(
        &compiled_reset_scene,
        &path,
        ControlAction::DelayButton(false)
    ));
    assert_eq!(actual.get("confirmed"), Ok(expected.get_confirmed().into()));
    assert_eq!(actual.get("activations"), Ok(Value::Int(1)));
}

#[test]
fn delay_button_rechecks_availability_after_reading_delay() {
    use expressions_ui::DelayActivationParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("DelayActivationParity")
        .unwrap();
    let mut generated = DelayActivationParity::new();
    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    actual
        .on("read-delay", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("allowed", false.into()).unwrap();
                }
                Ok(Some(800_i64.into()))
            })
        })
        .unwrap();
    generated.on_read_delay({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().set_allowed(false);
            800
        }
    });
    let action = ControlAction::DelayButton(true);
    assert!(!actual.control_at(&scene, &[0], action));
    assert!(!generated.control_at(&compiled_scene, &[0], action));
    assert_eq!(actual.get("allowed"), Ok(false.into()));
    assert!(!generated.get_allowed());
    assert_eq!(actual.get("confirmed"), Ok(false.into()));
    assert!(!generated.get_confirmed());
    assert_eq!(actual.get("activations"), Ok(0_i64.into()));
    assert_eq!(generated.get_activations(), 0);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn progress_and_busy_indicators_share_compiled_visual_state() {
    use controls_ui::IndicatorsDemo;

    let mut actual = interpret_controls_fixture("IndicatorsDemo");
    let mut expected = IndicatorsDemo::new();
    assert_same_scene(&actual.render_shared(), &expected.render_shared());
    actual.set("amount", Value::Float(0.3)).unwrap();
    expected.set_amount(0.3);
    assert!(actual.dispatch("running-toggle", "clicked"));
    assert!(expected.dispatch("running-toggle", "clicked"));
    assert_same_scene(&actual.render_shared(), &expected.render_shared());
    assert!(actual.dispatch("animation-toggle", "clicked"));
    assert!(expected.dispatch("animation-toggle", "clicked"));
    assert_same_scene(&actual.render_shared(), &expected.render_shared());
}

#[test]
fn focus_scope_requests_publish_state_and_dispatch_shortcuts_in_both_modes() {
    use layout_ui::FocusScopesDemo;
    use std::time::Instant;
    use xrack_layout::{FocusObservers, Interaction, ShortcutOutcome, Size};
    use xrack_runtime::{FocusReason, FocusRequests};

    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("FocusScopesDemo")
        .unwrap();
    let mut expected = FocusScopesDemo::new();

    fn exercise(ui: &mut impl Component) -> (Node, Node, String) {
        let mut input = Interaction::default();
        let mut observers = FocusObservers::default();
        let queue = FocusRequests::default();
        let _context = queue.enter();
        let size = Size::new(492.0, 272.0);
        let (scene, layout) = prepare_list(ui, &input, size);
        input.reconcile(&scene, &layout);
        assert!(observers.sync(&input, &scene, &layout, true).is_empty());

        assert!(ui.dispatch_at(&scene, &[3], "clicked"));
        input.apply_focus_requests(&scene, &layout, queue.drain());
        let target = input.focused.as_ref().unwrap();
        let name = scene
            .at_path(&target.path)
            .unwrap()
            .text("accessible-name")
            .unwrap()
            .to_owned();
        for event in observers.sync(&input, &scene, &layout, true) {
            event.apply(ui, &scene);
        }
        let focused = ui.render();
        let (scene, layout) = prepare_list(ui, &input, size);
        let ShortcutOutcome::Activate(activation) = input.shortcut_press(
            &scene,
            &layout,
            "Escape".parse().unwrap(),
            false,
            Instant::now(),
        ) else {
            panic!("expected the shortcut belonging to the focused scope");
        };
        assert!(ui.control_at(&scene, &activation.path, activation.action));
        input.apply_focus_requests(&scene, &layout, queue.drain());
        assert!(input.focused.is_none());
        for event in observers.sync(&input, &scene, &layout, true) {
            event.apply(ui, &scene);
        }
        (focused, ui.render(), name)
    }

    let (focused, cleared, name) = exercise(&mut actual);
    let (expected_focused, expected_cleared, expected_name) = exercise(&mut expected);
    assert_eq!(name, "右侧 查询");
    assert_eq!(name, expected_name);
    assert_same_scene(&focused, &expected_focused);
    assert_same_scene(&cleared, &expected_cleared);
    assert_eq!(actual.get("right-active").unwrap(), Value::Bool(false));
    assert_eq!(
        actual.get("right-visits").unwrap(),
        Value::Int(expected.get_right_visits())
    );
    assert_eq!(
        actual.get("right-focus-reason").unwrap(),
        Value::FocusReason(FocusReason::Programmatic)
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn scoped_keyboard_capture_bubble_and_release_match_compiled_handlers() {
    use layout_ui::ScopedKeysDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{EventResult, KeyEvent, KeyModifiers, KeyPhase};

    let source = include_str!("../examples/layout/layout.xr");
    let preview = xrack_compiler::prepare_preview(source, "layout.xr", Default::default()).unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ScopedKeysDemo")
        .unwrap();
    let mut expected = ScopedKeysDemo::new();

    fn exercise(ui: &mut impl Component) -> Vec<Option<EventResult>> {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(512.0, 254.0));
        input.focus_next(&scene, &layout, false);
        let route = input.key_route(&scene, &layout).unwrap();
        let key = |key: &str, text: &str| KeyEvent {
            key: key.into(),
            text: text.into(),
            ..Default::default()
        };
        [
            (KeyPhase::CapturePressed, key("x", "x")),
            (KeyPhase::CaptureReleased, key("x", "")),
            (KeyPhase::CapturePressed, key("F2", "")),
            (KeyPhase::Pressed, key("F2", "")),
            (KeyPhase::Released, key("F2", "")),
            (
                KeyPhase::CapturePressed,
                KeyEvent {
                    modifiers: KeyModifiers {
                        primary: true,
                        ..Default::default()
                    },
                    ..key("k", "")
                },
            ),
        ]
        .into_iter()
        .map(|(phase, event)| route.dispatch(ui, &scene, &layout, phase, &event))
        .collect()
    }

    let actual_events = exercise(&mut actual);
    assert_eq!(actual_events, exercise(&mut expected));
    assert_eq!(
        actual_events
            .iter()
            .filter(|event| **event == Some(EventResult::Accept))
            .count(),
        5
    );
    assert_eq!(
        actual.get("trace").unwrap(),
        Value::String(expected.get_trace())
    );
    assert_eq!(actual.get("accepted-releases").unwrap(), Value::Int(1));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn terminal_events_follow_the_original_delegate_after_a_model_reorder() {
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{
        DragAction, DragEvent, EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase,
        PointerEvent, PointerEventKind,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("MovedFocusRowParity")
        .unwrap();
    let mut generated = expressions_ui::MovedFocusRowParity::new();
    let size = Size::new(360.0, 240.0);
    let input = Interaction::default();
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    let path = [0, 0, 0, 0];
    let pointer = [0, 0, 0, 1];
    let drag = [0, 0, 0, 2];
    let cancel = PointerEvent {
        kind: PointerEventKind::Cancel,
        ..Default::default()
    };
    let finished = DragEvent::Finished(DragAction::Copy);
    assert_eq!(actual.at_path(&path).unwrap().kind, "FocusScope");
    assert_eq!(expected.at_path(&path).unwrap().kind, "FocusScope");
    for model in [
        interpreted.string_model("labels").unwrap(),
        generated.model_labels(),
    ] {
        assert!(model.sort_by(|left, right| right.cmp(left)));
    }
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let loss = FocusEvent::Lost(FocusReason::LayoutChange);
    let mut delivered = Vec::new();
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*actual),
        (&mut generated, &*expected),
    ] {
        assert_eq!(
            ui.key_event_at(scene, &path, KeyPhase::Pressed, &key),
            EventResult::Reject
        );
        assert!(!ui.pointer_event_at(
            scene,
            &pointer,
            &PointerEvent {
                kind: PointerEventKind::Down,
                ..Default::default()
            }
        ));
        assert!(!ui.drag_event_at(scene, &drag, &DragEvent::Started));
        delivered.push([
            ui.focus_event_at(scene, &path, loss),
            ui.pointer_event_at(scene, &pointer, &cancel),
            ui.drag_event_at(scene, &drag, &finished),
        ]);
    }
    assert_eq!(delivered, [[true; 3]; 2]);
    assert_eq!(
        interpreted.get("events"),
        Ok(Value::String("lost A@1;pointer A@1;drag A@1;".into()))
    );
    assert_eq!(generated.get_events(), "lost A@1;pointer A@1;drag A@1;");

    let replacement = vec!["A".into(), "B".into()];
    interpreted
        .set("labels", Value::StringList(replacement.clone()))
        .unwrap();
    generated.set_labels(replacement);
    assert!(!interpreted.focus_event_at(&actual, &path, loss));
    assert!(!generated.focus_event_at(&expected, &path, loss));
    assert!(!interpreted.pointer_event_at(&actual, &pointer, &cancel));
    assert!(!generated.pointer_event_at(&expected, &pointer, &cancel));
    assert!(!interpreted.drag_event_at(&actual, &drag, &finished));
    assert!(!generated.drag_event_at(&expected, &drag, &finished));
    assert_eq!(
        interpreted.get("events"),
        Ok(Value::String("lost A@1;pointer A@1;drag A@1;".into()))
    );
    assert_eq!(generated.get_events(), "lost A@1;pointer A@1;drag A@1;");
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_delegate_input_rechecks_live_row_ancestors() {
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableDelegateInputParity")
        .unwrap();
    let mut compiled = expressions_ui::TableDelegateInputParity::new();
    let interaction = Interaction::default();
    let size = Size::new(320.0, 200.0);
    let (actual, _) = prepare_list(&interpreted, &interaction, size);
    let (expected, _) = prepare_list(&compiled, &interaction, size);
    let path = [0, 0, 0, 0, 0, 0];
    assert_eq!(actual.at_path(&path).unwrap().kind, "FocusScope");
    assert_eq!(expected.at_path(&path).unwrap().kind, "FocusScope");
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let press = |ui: &mut dyn Component, scene: &Node| {
        ui.key_event_at(scene, &path, KeyPhase::Pressed, &key)
    };
    assert_eq!(press(&mut interpreted, &actual), EventResult::Accept);
    assert_eq!(press(&mut compiled, &expected), EventResult::Accept);
    interpreted.set("pane-enabled", false.into()).unwrap();
    compiled.set_pane_enabled(false);
    assert_eq!(press(&mut interpreted, &actual), EventResult::Reject);
    assert_eq!(
        press(&mut compiled, &expected),
        EventResult::Reject,
        "disabled row ancestor"
    );
    interpreted.set("pane-enabled", true.into()).unwrap();
    compiled.set_pane_enabled(true);
    interpreted.set("delegate-visible", false.into()).unwrap();
    compiled.set_delegate_visible(false);
    assert_eq!(press(&mut interpreted, &actual), EventResult::Reject);
    assert_eq!(
        press(&mut compiled, &expected),
        EventResult::Reject,
        "hidden row instance"
    );
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*actual),
        (&mut compiled, &*expected),
    ] {
        assert!(ui.focus_event_at(scene, &path, FocusEvent::Lost(FocusReason::LayoutChange)));
    }
    assert_eq!(compiled.get_events(), "key A@0;lost A@0;");
    assert_eq!(interpreted.get("events"), Ok(compiled.get_events().into()));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn repeated_input_samples_each_state_predicate_once_per_route() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{EventResult, KeyEvent, KeyPhase};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableRowStatePredicateParity")
        .unwrap();
    let mut compiled = expressions_ui::TableRowStatePredicateParity::new();
    let locked = Rc::new(Cell::new(false));
    let actual_reads = Rc::new(RefCell::new(Vec::new()));
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("row-locked", {
            let locked = locked.clone();
            let reads = actual_reads.clone();
            Rc::new(move |args| {
                let [Value::Int(row)] = args else {
                    unreachable!()
                };
                reads.borrow_mut().push(*row);
                Ok(Some(locked.get().into()))
            })
        })
        .unwrap();
    compiled.on_row_locked({
        let locked = locked.clone();
        let reads = compiled_reads.clone();
        move |row| {
            reads.borrow_mut().push(row);
            locked.get()
        }
    });
    let interaction = Interaction::default();
    let size = Size::new(320.0, 200.0);
    let (actual_scene, _) = prepare_list(&actual, &interaction, size);
    let (compiled_scene, _) = prepare_list(&compiled, &interaction, size);
    actual_reads.take();
    compiled_reads.take();

    let path = [0, 0, 0, 0, 0, 0];
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    for (value, expected) in [
        (true, EventResult::Reject),
        (false, EventResult::Accept),
        (true, EventResult::Reject),
    ] {
        locked.set(value);
        assert_eq!(
            compiled.key_event_at(&compiled_scene, &path, KeyPhase::Pressed, &key),
            expected
        );
        assert_eq!(
            actual.key_event_at(&actual_scene, &path, KeyPhase::Pressed, &key),
            expected
        );
        assert_eq!(compiled_reads.take(), [0]);
        assert_eq!(actual_reads.take(), [0]);
    }
    assert_eq!(compiled.get_events(), "key A;");
    assert_eq!(actual.get("events"), Ok("key A;".to_owned().into()));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn ordinary_repeated_group_boxes_toggle_by_row_identity() {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RepeatedGroupParity")
        .unwrap();
    let mut generated = expressions_ui::RepeatedGroupParity::new();
    let first = [0, 0];
    let second = [0, 1];
    let nested = [0, 0, 0, 0];
    let old = interpreted.render_shared();
    let compiled_old = generated.render_shared();
    assert_same_scene(&old, &compiled_old);
    interpreted.set("allow-rows", false.into()).unwrap();
    generated.set_allow_rows(false);
    assert!(!interpreted.dispatch_at(&old, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_old, &first, "clicked"));
    interpreted.set("allow-rows", true.into()).unwrap();
    generated.set_allow_rows(true);
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*old),
        (&mut generated as &mut dyn Component, &*compiled_old),
    ] {
        assert!(ui.dispatch_at(scene, &nested, "clicked"));
        assert!(ui.dispatch_at(scene, &first, "clicked"));
        assert!(!ui.dispatch_at(scene, &nested, "clicked"));
        assert!(ui.dispatch_at(scene, &second, "clicked"));
    }
    let reordered = interpreted.string_model("labels").unwrap();
    assert!(reordered.sort_by(|left, right| right.cmp(left)));
    assert!(
        generated
            .model_labels()
            .sort_by(|left, right| right.cmp(left))
    );
    assert!(!interpreted.dispatch_at(&old, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_old, &first, "clicked"));
    let actual = interpreted.render_shared();
    let expected = generated.render_shared();
    assert_same_scene(&actual, &expected);
    assert!(actual.at_path(&first).unwrap().boolean("checked", false));
    assert!(!actual.at_path(&second).unwrap().boolean("checked", true));
    assert!(
        !actual
            .at_path(&[0, 1, 0, 0])
            .unwrap()
            .boolean("checked", true)
    );
    assert!(actual.at_path(&nested).unwrap().boolean("checked", false));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_row_group_boxes_keep_independent_state_and_route_their_children() {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RowGroupInputParity")
        .unwrap();
    let mut compiled = expressions_ui::RowGroupInputParity::new();
    let input = xrack_layout::Interaction::default();
    let size = xrack_layout::Size::new(320.0, 200.0);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&compiled, &input, size);
    let group_a = [0, 0, 0, 0];
    let group_b = [0, 1, 0, 0];
    let button_a = [0, 0, 0, 0, 0, 0];
    let button_b = [0, 1, 0, 0, 0, 0];
    let nested_a = [0, 0, 0, 0, 1, 0];
    let nested_b = [0, 1, 0, 0, 1, 0];
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*actual),
        (&mut compiled as &mut dyn Component, &*expected),
    ] {
        assert!(scene.at_path(&group_a).unwrap().boolean("checked", false));
        assert!(!scene.at_path(&group_b).unwrap().boolean("checked", true));
        assert!(ui.dispatch_at(scene, &button_a, "clicked"));
        assert!(!ui.dispatch_at(scene, &button_b, "clicked"));
        assert!(ui.dispatch_at(scene, &nested_a, "clicked"));
        assert!(ui.dispatch_at(scene, &group_a, "clicked"));
        assert!(!ui.dispatch_at(scene, &button_a, "clicked"));
        assert!(!ui.dispatch_at(scene, &nested_a, "clicked"));
        assert!(ui.dispatch_at(scene, &group_b, "clicked"));
    }
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&compiled, &input, size);
    assert_same_scene(&actual, &expected);
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*actual),
        (&mut compiled as &mut dyn Component, &*expected),
    ] {
        assert!(!ui.dispatch_at(scene, &button_a, "clicked"));
        assert!(ui.dispatch_at(scene, &button_b, "clicked"));
    }

    let dynamic_rows = interpreted.record_model("rows").unwrap();
    assert!(dynamic_rows.model().sort_by(|left, right| {
        let name = |row: &xrack_runtime::RecordValue| match row.field("name") {
            Some(Value::String(name)) => name.clone(),
            _ => unreachable!(),
        };
        name(right).cmp(&name(left))
    }));
    assert!(
        compiled
            .model_rows()
            .sort_by(|left, right| right.name.cmp(&left.name))
    );
    assert!(!interpreted.dispatch_at(&actual, &group_a, "clicked"));
    assert!(!compiled.dispatch_at(&expected, &group_a, "clicked"));
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&compiled, &input, size);
    assert_same_scene(&actual, &expected);
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*actual),
        (&mut compiled as &mut dyn Component, &*expected),
    ] {
        assert!(ui.dispatch_at(scene, &button_a, "clicked"));
        assert!(!ui.dispatch_at(scene, &button_b, "clicked"));
    }
    assert_eq!(interpreted.get("hits"), Ok("ABB".to_owned().into()));
    assert_eq!(compiled.get_hits(), "ABB");
    assert!(actual.at_path(&nested_a).unwrap().boolean("checked", false));
    assert!(!actual.at_path(&nested_b).unwrap().boolean("checked", true));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn nested_terminal_events_follow_both_rows_after_reordering() {
    use expressions_ui::NestedMovedFocusParity;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("NestedMovedFocusParity")
        .unwrap();
    let mut generated = NestedMovedFocusParity::new();
    let input = Interaction::default();
    let size = Size::new(360.0, 240.0);
    let (actual_scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    let scope = [0, 0, 0, 0, 0, 0, 0];
    assert_eq!(actual_scene.at_path(&scope).unwrap().kind, "FocusScope");
    assert_eq!(generated_scene.at_path(&scope).unwrap().kind, "FocusScope");
    let inner = [0, 0, 0, 0];
    let actual_rows = actual_scene.list_rows_at(&inner, 1..usize::MAX);
    let generated_rows = generated_scene.list_rows_at(&inner, 1..usize::MAX);
    assert_eq!(actual_rows.len(), 1);
    assert_eq!(generated_rows.len(), 1);
    assert_same_scene(&actual_rows[0], &generated_rows[0]);
    for model in [
        actual.string_model("groups").unwrap(),
        actual.string_model("labels").unwrap(),
        generated.model_groups(),
        generated.model_labels(),
    ] {
        assert!(model.sort_by(|left, right| right.cmp(left)));
    }
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let loss = FocusEvent::Lost(FocusReason::LayoutChange);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut generated, &*generated_scene),
    ] {
        assert_eq!(
            ui.key_event_at(scene, &scope, KeyPhase::Pressed, &key),
            EventResult::Reject
        );
        assert!(ui.focus_event_at(scene, &scope, loss));
    }
    assert_eq!(generated.get_events(), "lost A@1/first@1;");
    assert_eq!(actual.get("events"), Ok(generated.get_events().into()));

    let replacement = vec!["A".into(), "B".into()];
    actual
        .set("labels", Value::StringList(replacement.clone()))
        .unwrap();
    generated.set_labels(replacement);
    assert!(!actual.focus_event_at(&actual_scene, &scope, loss));
    assert!(!generated.focus_event_at(&generated_scene, &scope, loss));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn focus_commands_queue_operands_in_source_order() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{FocusAction, FocusRequests, TextCommand};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("FocusCommandOrderParity")
        .unwrap();
    let mut generated = expressions_ui::FocusCommandOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("offset", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*tag);
                Ok(Some((tag + 1).into()))
            })
        })
        .unwrap();
    generated.on_offset({
        let calls = generated_calls.clone();
        move |tag| {
            calls.borrow_mut().push(tag);
            tag + 1
        }
    });

    let queue = FocusRequests::default();
    let _context = queue.enter();
    actual.invoke("select-range", &[]).unwrap();
    let interpreted = queue.drain();
    generated.invoke_select_range();
    let compiled = queue.drain();
    let actions = [
        FocusAction::Focus,
        FocusAction::Text(TextCommand::SetSelection {
            anchor: 2,
            focus: 3,
        }),
        FocusAction::Clear,
    ];
    for (commands, scene) in [
        (interpreted, actual.render_shared()),
        (compiled, generated.render_shared()),
    ] {
        let target = scene.children[0].focus_handle.as_ref().unwrap();
        assert_eq!(commands.len(), actions.len());
        for (request, action) in commands.iter().zip(actions) {
            assert_eq!(&request.target, target);
            assert_eq!(request.action, action);
        }
    }
    assert_eq!(*actual_calls.borrow(), [1, 2]);
    assert_eq!(*generated_calls.borrow(), [1, 2]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn interpreted_text_commands_use_the_same_selection_and_undo_history() {
    use form_ui::EditorCommandsDemo;
    use std::time::Instant;
    use xrack_layout::{EditAction, EditSource, Interaction, Size};
    use xrack_runtime::{FocusRequests, TextCommand};

    let mut actual = interpret_form_fixture("EditorCommandsDemo");
    let mut expected = EditorCommandsDemo::new();

    fn exercise<T: Component>(
        ui: &mut T,
        select: impl FnOnce(&mut T),
        undo: impl FnOnce(&mut T),
        paste: impl FnOnce(&mut T),
    ) {
        let queue = FocusRequests::default();
        let _context = queue.enter();
        let mut input = Interaction::default();
        let size = Size::new(620.0, 240.0);
        let (scene, layout) = prepare_list(ui, &input, size);
        select(ui);
        input.apply_focus_requests(&scene, &layout, queue.drain());
        let target = input.focused.clone().unwrap();
        let original = scene.at_path(&target.path).unwrap().text("text").unwrap();
        assert_eq!(input.editor.as_ref().unwrap().selection(original), 0..9);
        paste(ui);
        let request = queue.drain().pop().unwrap();
        assert_eq!(
            input.apply_focus_request(&scene, &layout, request),
            Some(TextCommand::Paste)
        );
        assert!(input.edit_text(
            ui,
            &scene,
            EditAction::Insert("新："),
            EditSource::Atomic,
            Instant::now()
        ));
        let (scene, layout) = prepare_list(ui, &input, size);
        assert_eq!(
            scene.at_path(&target.path).unwrap().text("text").unwrap(),
            "新：在这里编辑文本"
        );
        undo(ui);
        let request = queue.drain().pop().unwrap();
        assert_eq!(
            input.apply_focus_request(&scene, &layout, request),
            Some(TextCommand::Undo)
        );
        assert!(input.replay_text_edit(ui, &scene, false));
        let scene = ui.render_shared();
        assert_eq!(
            scene.at_path(&target.path).unwrap().text("text").unwrap(),
            original
        );
        assert_eq!(input.editor.as_ref().unwrap().selection(original), 0..9);
    }

    exercise(
        &mut actual,
        |ui| {
            ui.invoke("select-prefix", &[]).unwrap();
        },
        |ui| {
            ui.invoke("undo-edit", &[]).unwrap();
        },
        |ui| {
            ui.invoke("paste-selected", &[]).unwrap();
        },
    );
    exercise(
        &mut expected,
        EditorCommandsDemo::invoke_select_prefix,
        EditorCommandsDemo::invoke_undo_edit,
        EditorCommandsDemo::invoke_paste_selected,
    );
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

fn interpret_menu_fixture(component: &str) -> Instance {
    let file = "examples/menu/menu.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/menu/menu.xr"),
        file,
        Default::default(),
    )
    .unwrap();
    let assets = xrack_compiler::load_preview_assets(&preview.module, file).unwrap();
    Program::from_module_with_assets(
        &preview.module,
        None,
        None,
        assets
            .images
            .into_iter()
            .map(|(name, image)| (name, image.into_image_asset()))
            .collect(),
    )
    .unwrap()
    .create(component)
    .unwrap()
}

fn deliver_shortcut(
    ui: &mut impl Component,
    input: &mut xrack_layout::Interaction,
    key: &str,
    repeat: bool,
    size: xrack_layout::Size,
    now: std::time::Instant,
) -> xrack_layout::ShortcutOutcome {
    use xrack_layout::ShortcutOutcome;

    let (scene, layout) = prepare_list(ui, input, size);
    input.reconcile(&scene, &layout);
    let outcome = input.shortcut_press(&scene, &layout, key.parse().unwrap(), repeat, now);
    let deliveries = match &outcome {
        ShortcutOutcome::Activate(action) => std::slice::from_ref(action),
        ShortcutOutcome::Ambiguous(actions) => actions,
        _ => &[],
    };
    for action in deliveries {
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    outcome
}

#[test]
fn shared_action_updates_menu_tool_button_and_shortcut_in_both_modes() {
    use menu_ui::MenuDemo;
    use std::time::Instant;
    use xrack_layout::{Interaction, ShortcutOutcome, Size};
    use xrack_runtime::ControlAction;

    let mut actual = interpret_menu_fixture("MenuDemo");
    let mut expected = MenuDemo::new();
    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let size = Size::new(620.0, 366.0);
    let grid_item = [0, 0, 1, 0];
    let grid_button = [0, 2, 0];
    let (scene, _) = prepare_list(&actual, &actual_input, size);
    let (expected_scene, _) = prepare_list(&expected, &expected_input, size);
    assert_eq!(
        expected_scene.at_path(&grid_item).unwrap().id,
        Some("grid-item")
    );
    assert_eq!(
        expected_scene.at_path(&grid_button).unwrap().id,
        Some("grid-button")
    );
    let identity = scene.at_path(&grid_item).unwrap().action.unwrap();
    assert_eq!(scene.at_path(&grid_button).unwrap().action, Some(identity));
    let second = interpret_menu_fixture("MenuDemo");
    assert_ne!(
        second.render().at_path(&grid_item).unwrap().action,
        Some(identity)
    );
    assert_same_scene(&scene, &expected_scene);

    assert!(actual.dispatch_at(&scene, &grid_button, "clicked"));
    assert!(expected.dispatch_at(&expected_scene, &grid_button, "clicked"));
    assert_eq!(actual.get("show-grid").unwrap(), Value::Bool(true));
    let (scene, _) = prepare_list(&actual, &actual_input, size);
    let (expected_scene, _) = prepare_list(&expected, &expected_input, size);
    assert!(actual.control_at(&scene, &grid_item, ControlAction::ActivateMenuItem));
    assert!(expected.control_at(&expected_scene, &grid_item, ControlAction::ActivateMenuItem));
    assert_eq!(actual.get("show-grid").unwrap(), Value::Bool(false));

    let now = Instant::now();
    assert_eq!(
        deliver_shortcut(
            &mut actual,
            &mut actual_input,
            "Primary+G",
            false,
            size,
            now
        ),
        deliver_shortcut(
            &mut expected,
            &mut expected_input,
            "Primary+G",
            false,
            size,
            now
        )
    );
    assert_eq!(actual.get("grid-activations").unwrap(), Value::Int(3));
    assert_eq!(
        actual.get("command").unwrap(),
        Value::String(expected.get_command())
    );
    actual.set("grid-enabled", Value::Bool(false)).unwrap();
    expected.set_grid_enabled(false);
    assert_eq!(
        deliver_shortcut(
            &mut actual,
            &mut actual_input,
            "Primary+G",
            false,
            size,
            now
        ),
        ShortcutOutcome::Unmatched
    );
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn item_delegate_activation_checks_live_row_bindings() {
    use expressions_ui::ItemActivationParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{RecordValue, VecModel};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ItemActivationParity")
        .unwrap();
    let mut generated = ItemActivationParity::new();
    let actual_calls = Rc::new(Cell::new(0));
    let compiled_calls = Rc::new(Cell::new(0));
    actual
        .on("allow-row", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                calls.set(calls.get() + 1);
                Ok(args.first().cloned())
            })
        })
        .unwrap();
    generated.on_allow_row({
        let calls = compiled_calls.clone();
        move |allowed| {
            calls.set(calls.get() + 1);
            allowed
        }
    });
    let input = Interaction::default();
    let size = Size::new(300.0, 120.0);
    let (scene, _) = prepare_list(&actual, &input, size);
    let (compiled_scene, _) = prepare_list(&generated, &input, size);
    assert_eq!((actual_calls.replace(0), compiled_calls.replace(0)), (2, 2));
    let first = [0, 0, 0];
    let second = [0, 1, 0];
    assert!(actual.dispatch_at(&scene, &first, "clicked"));
    assert!(generated.dispatch_at(&compiled_scene, &first, "clicked"));
    assert_eq!((actual_calls.get(), compiled_calls.get()), (0, 0));

    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let model = rows.model();
    let compiled_model = generated.model_rows();
    let set_allowed = |model: &VecModel<RecordValue>, allowed| {
        let row = model
            .row(0)
            .unwrap()
            .with_field(&["allowed".into()], Value::Bool(allowed))
            .unwrap();
        assert!(model.set_row(0, row));
    };
    set_allowed(model, false);
    let mut row = compiled_model.row(0).unwrap();
    row.allowed = false;
    assert!(compiled_model.set_row(0, row));
    assert!(!actual.dispatch_at(&scene, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_scene, &first, "clicked"));
    assert!(!actual.dispatch_at(&scene, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_scene, &first, "clicked"));
    assert_eq!((actual_calls.replace(0), compiled_calls.replace(0)), (1, 1));
    assert!(actual.dispatch_at(&scene, &second, "clicked"));
    assert!(generated.dispatch_at(&compiled_scene, &second, "clicked"));
    assert_eq!((actual_calls.get(), compiled_calls.get()), (0, 0));

    set_allowed(model, true);
    let mut row = compiled_model.row(0).unwrap();
    row.allowed = true;
    assert!(compiled_model.set_row(0, row));
    actual.set("locked", true.into()).unwrap();
    generated.set_locked(true);
    assert!(!actual.dispatch_at(&scene, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_scene, &first, "clicked"));
    assert_eq!((actual_calls.get(), compiled_calls.get()), (0, 0));

    actual.set("locked", false.into()).unwrap();
    generated.set_locked(false);
    actual
        .on("allow-row", {
            let model = model.clone();
            Rc::new(move |args| {
                model.remove(0).unwrap();
                Ok(args.first().cloned())
            })
        })
        .unwrap();
    generated.on_allow_row({
        let model = compiled_model.clone();
        move |allowed| {
            model.remove(0).unwrap();
            allowed
        }
    });
    assert!(!actual.dispatch_at(&scene, &first, "clicked"));
    assert!(!generated.dispatch_at(&compiled_scene, &first, "clicked"));
    assert_eq!(model.len(), 1);
    assert_eq!(compiled_model.len(), 1);
    assert_eq!(actual.get("activations"), Ok(2_i64.into()));
    assert_eq!(generated.get_activations(), 2);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn action_activation_rechecks_availability_and_menu_checkability() {
    use expressions_ui::LiveActionActivationParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("LiveActionActivationParity")
        .unwrap();
    let mut generated = LiveActionActivationParity::new();
    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    let button = [0];
    let item = [1, 0, 0];
    assert_eq!(scene.at_path(&item).unwrap().kind, "MenuItem");
    assert_eq!(compiled_scene.at_path(&item).unwrap().kind, "MenuItem");
    actual
        .on("check-action", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("command-enabled", false.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_check_action({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().set_command_enabled(false);
            true
        }
    });
    assert!(!actual.dispatch_at(&scene, &button, "clicked"));
    assert!(!generated.dispatch_at(&compiled_scene, &button, "clicked"));
    assert_eq!(actual.get("command-enabled"), Ok(false.into()));
    assert!(!generated.get_command_enabled());
    assert_eq!(actual.get("action-checked"), Ok(false.into()));
    assert!(!generated.get_action_checked());
    assert_eq!(actual.get("action-activations"), Ok(0_i64.into()));
    assert_eq!(generated.get_action_activations(), 0);

    actual.set("command-enabled", true.into()).unwrap();
    generated.set_command_enabled(true);
    actual
        .on("check-action", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    generated.on_check_action(|| true);
    assert!(actual.dispatch_at(&actual.render_shared(), &button, "clicked"));
    assert!(generated.dispatch_at(&generated.render_shared(), &button, "clicked"));
    assert_eq!(actual.get("action-checked"), Ok(true.into()));
    assert!(generated.get_action_checked());
    assert_eq!(actual.get("action-activations"), Ok(1_i64.into()));
    assert_eq!(generated.get_action_activations(), 1);

    let activate = ControlAction::ActivateMenuItem;
    let linked_item = [1, 0, 1];
    actual.set("menu-enabled", false.into()).unwrap();
    generated.set_menu_enabled(false);
    assert!(!actual.control_at(&scene, &item, activate));
    assert!(!generated.control_at(&compiled_scene, &item, activate));
    assert!(!actual.control_at(&scene, &linked_item, activate));
    assert!(!generated.control_at(&compiled_scene, &linked_item, activate));
    assert_eq!(actual.get("action-activations"), Ok(1_i64.into()));
    assert_eq!(generated.get_action_activations(), 1);
    actual.set("menu-enabled", true.into()).unwrap();
    generated.set_menu_enabled(true);
    actual.set("menu-checkable", false.into()).unwrap();
    generated.set_menu_checkable(false);
    assert!(actual.control_at(&scene, &item, activate));
    assert!(generated.control_at(&compiled_scene, &item, activate));
    assert_eq!(actual.get("menu-checked"), Ok(false.into()));
    assert!(!generated.get_menu_checked());
    assert_eq!(actual.get("menu-activations"), Ok(1_i64.into()));
    assert_eq!(generated.get_menu_activations(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn menu_activation_aborts_when_checkability_disables_its_parent() {
    use expressions_ui::LiveActionActivationParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("LiveActionActivationParity")
        .unwrap();
    let mut generated = LiveActionActivationParity::new();
    let scene = interpreted.render_shared();
    let generated_scene = generated.render_shared();
    interpreted
        .on("check-menu-item", {
            let owner = Cell::new(Some(interpreted.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("menu-parent-enabled", false.into()).unwrap();
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_check_menu_item({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().set_menu_parent_enabled(false);
            true
        }
    });

    let path = [1, 0, 0];
    let activate = ControlAction::ActivateMenuItem;
    assert!(!interpreted.control_at(&scene, &path, activate));
    assert!(!generated.control_at(&generated_scene, &path, activate));
    assert_eq!(interpreted.get("menu-parent-enabled"), Ok(false.into()));
    assert!(!generated.get_menu_parent_enabled());
    assert_eq!(interpreted.get("menu-checked"), Ok(false.into()));
    assert!(!generated.get_menu_checked());
    assert_eq!(interpreted.get("menu-activations"), Ok(0_i64.into()));
    assert_eq!(generated.get_menu_activations(), 0);

    interpreted.set("menu-parent-enabled", true.into()).unwrap();
    generated.set_menu_parent_enabled(true);
    assert!(interpreted.control_at(&scene, &path, activate));
    assert!(generated.control_at(&generated_scene, &path, activate));
    assert_eq!(interpreted.get("menu-checked"), Ok(true.into()));
    assert!(generated.get_menu_checked());
    assert_eq!(interpreted.get("menu-activations"), Ok(1_i64.into()));
    assert_eq!(generated.get_menu_activations(), 1);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn action_group_selection_precedes_callbacks_and_respects_group_enabled_state() {
    use menu_ui::ActionGroupDemo;
    use std::time::Instant;
    use xrack_layout::{Interaction, Size};

    let mut actual = interpret_menu_fixture("ActionGroupDemo");
    let mut expected = ActionGroupDemo::new();
    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let size = Size::new(620.0, 300.0);
    let now = Instant::now();
    assert_same_scene(&actual.render(), &expected.render());

    assert_eq!(
        deliver_shortcut(
            &mut actual,
            &mut actual_input,
            "Primary+2",
            false,
            size,
            now
        ),
        deliver_shortcut(
            &mut expected,
            &mut expected_input,
            "Primary+2",
            false,
            size,
            now
        )
    );
    for name in ["mode", "mode-at-action", "last-command"] {
        assert_eq!(actual.get(name).unwrap(), Value::String("compact".into()));
    }
    assert_eq!(actual.get("mode-activations").unwrap(), Value::Int(1));

    for path in [[0, 3, 0], [0, 3, 1], [0, 3, 0]] {
        let scene = actual.render();
        let expected_scene = expected.render();
        assert!(actual.dispatch_at(&scene, &path, "clicked"));
        assert!(expected.dispatch_at(&expected_scene, &path, "clicked"));
    }
    assert_eq!(actual.get("show-grid").unwrap(), Value::Bool(false));
    assert_eq!(actual.get("show-guides").unwrap(), Value::Bool(true));
    assert_eq!(actual.get("overlay-activations").unwrap(), Value::Int(3));

    actual.set("modes-enabled", Value::Bool(false)).unwrap();
    expected.set_modes_enabled(false);
    let path = [0, 2, 1];
    let scene = actual.render();
    let expected_scene = expected.render();
    assert_eq!(
        expected_scene.at_path(&path).unwrap().id,
        Some("compact-button")
    );
    assert!(!scene.at_path(&path).unwrap().boolean("enabled", true));
    assert!(!actual.dispatch_at(&scene, &path, "clicked"));
    assert_same_scene(&scene, &expected_scene);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn shortcut_chords_repeat_conflict_and_modal_scope_match_compiled_mode() {
    use menu_ui::ShortcutDemo;
    use std::time::Instant;
    use xrack_layout::{Interaction, ShortcutOutcome, Size};
    use xrack_runtime::{ControlAction, DialogRole};

    let mut actual = interpret_menu_fixture("ShortcutDemo");
    let mut expected = ShortcutDemo::new();
    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let size = Size::new(532.0, 344.0);
    let now = Instant::now();
    fn press_pair(
        (actual, actual_input): (&mut Instance, &mut Interaction),
        (expected, expected_input): (&mut ShortcutDemo, &mut Interaction),
        key: &str,
        repeat: bool,
        size: Size,
        now: Instant,
    ) -> ShortcutOutcome {
        let outcome = deliver_shortcut(actual, actual_input, key, repeat, size, now);
        assert_eq!(
            outcome,
            deliver_shortcut(expected, expected_input, key, repeat, size, now)
        );
        outcome
    }

    macro_rules! press {
        ($key:literal, $repeat:literal) => {
            press_pair(
                (&mut actual, &mut actual_input),
                (&mut expected, &mut expected_input),
                $key,
                $repeat,
                size,
                now,
            )
        };
    }
    assert_eq!(press!("Primary+K", false), ShortcutOutcome::Consumed);
    assert!(matches!(
        press!("Primary+P", false),
        ShortcutOutcome::Activate(_)
    ));
    assert!(matches!(press!("F9", false), ShortcutOutcome::Activate(_)));
    assert_eq!(press!("F9", true), ShortcutOutcome::Consumed);
    assert!(matches!(press!("F7", true), ShortcutOutcome::Activate(_)));
    assert_eq!(press!("F8", false), ShortcutOutcome::Unmatched);

    let (scene, layout) = prepare_list(&actual, &actual_input, size);
    let (expected_scene, expected_layout) = prepare_list(&expected, &expected_input, size);
    let entry = [4, 3, 0];
    actual_input.accessibility_focus(
        actual_input
            .accessibility_target(&scene, &layout, &entry)
            .unwrap(),
    );
    expected_input.accessibility_focus(
        expected_input
            .accessibility_target(&expected_scene, &expected_layout, &entry)
            .unwrap(),
    );
    assert!(matches!(press!("F8", false), ShortcutOutcome::Activate(_)));

    actual.set("conflict", Value::Bool(true)).unwrap();
    expected.set_conflict(true);
    assert!(
        matches!(press!("F6", false), ShortcutOutcome::Ambiguous(actions) if actions.len() == 2)
    );
    actual.set("dialog-open", Value::Bool(true)).unwrap();
    expected.set_dialog_open(true);
    assert!(matches!(press!("F6", false), ShortcutOutcome::Activate(_)));
    assert_eq!(press!("F9", false), ShortcutOutcome::Unmatched);

    assert_eq!(actual.get("commands").unwrap(), Value::Int(2));
    assert_eq!(actual.get("repeats").unwrap(), Value::Int(1));
    assert_eq!(actual.get("focused-hits").unwrap(), Value::Int(1));
    assert_eq!(actual.get("conflicts").unwrap(), Value::Int(2));
    assert_eq!(actual.get("dialog-hits").unwrap(), Value::Int(1));
    assert_eq!(actual.get("window-hits").unwrap(), Value::Int(0));
    let scene = actual.render();
    let expected_scene = expected.render();
    let dialog = [5];
    assert_eq!(
        expected_scene.at_path(&dialog).unwrap().id,
        Some("shortcut-dialog")
    );
    assert!(actual.control_at(&scene, &dialog, ControlAction::Dialog(DialogRole::Reject)));
    assert!(expected.control_at(
        &expected_scene,
        &dialog,
        ControlAction::Dialog(DialogRole::Reject)
    ));
    assert_eq!(actual.get("dialog-open").unwrap(), Value::Bool(false));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn overlay_handlers_observe_committed_open_state_in_both_backends() {
    use xrack_runtime::{ControlAction, DialogCommand, DialogRole};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("OverlayTransitionParity")
        .unwrap();
    let mut generated = expressions_ui::OverlayTransitionParity::new();
    actual.invoke("show-popup", &[]).unwrap();
    generated.invoke_show_popup();

    for ui in [&mut actual as &mut dyn Component, &mut generated] {
        let popup = ui.render_shared();
        assert!(ui.control_at(&popup, &[0, 0], ControlAction::Popup(false)));
        assert!(!ui.control_at(&popup, &[0, 0], ControlAction::Popup(false)));
        let drawer = ui.render_shared();
        assert!(ui.control_at(&drawer, &[1], ControlAction::Drawer(true)));
        assert!(!ui.control_at(&drawer, &[1], ControlAction::Drawer(true)));
        assert!(ui.control_at(&ui.render_shared(), &[1], ControlAction::Drawer(false)));
        let dialog = ui.render_shared();
        assert!(ui.control_at(
            &dialog,
            &[2],
            ControlAction::DialogCommand(DialogCommand::Apply)
        ));
        assert!(ui.control_at(&dialog, &[2], ControlAction::Dialog(DialogRole::Accept)));
        assert!(!ui.control_at(&dialog, &[2], ControlAction::Dialog(DialogRole::Accept)));
    }
    let events = "popup-open:true;popup-close:false;drawer-open:true;drawer-close:false;apply:true;accept:false;";
    assert_eq!(actual.get("events"), Ok(events.to_owned().into()));
    assert_eq!(generated.get_events(), events);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn dialog_button_commands_and_drawer_opening_match_compiled_mode() {
    use dialog_ui::{DialogDemo, StandardDialogDemo};
    use xrack_runtime::{ControlAction, DialogCommand, DrawerEdge};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/dialog/dialog.xr"),
        "dialog.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();

    let mut actual = program.create("StandardDialogDemo").unwrap();
    let mut expected = StandardDialogDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    assert!(actual.dispatch_at(&actual.render(), &[2], "clicked"));
    assert!(expected.dispatch_at(&expected.render(), &[2], "clicked"));
    assert_same_scene(&actual.render(), &expected.render());
    actual.set("draft", Value::String("新名称".into())).unwrap();
    expected.set_draft("新名称".into());
    for command in [
        DialogCommand::Apply,
        DialogCommand::Help,
        DialogCommand::Discard,
    ] {
        let action = ControlAction::DialogCommand(command);
        assert!(actual.control_at(&actual.render(), &[3], action));
        assert!(expected.control_at(&expected.render(), &[3], action));
        assert_same_scene(&actual.render(), &expected.render());
    }
    assert_eq!(actual.get("saved").unwrap(), Value::String("新名称".into()));
    for name in ["applies", "helps", "discards"] {
        assert_eq!(actual.get(name).unwrap(), Value::Int(1));
    }
    assert_eq!(actual.get("settings-open").unwrap(), Value::Bool(false));

    let mut actual = program.create("DialogDemo").unwrap();
    let mut expected = DialogDemo::new();
    actual
        .set("drawer-edge", Value::DrawerEdge(DrawerEdge::Right))
        .unwrap();
    expected.set_drawer_edge(DrawerEdge::Right);
    for open in [true, false] {
        let path = [0, 4];
        let action = ControlAction::Drawer(open);
        assert_eq!(expected.render().at_path(&path).unwrap().id, Some("drawer"));
        assert!(actual.control_at(&actual.render(), &path, action));
        assert!(expected.control_at(&expected.render(), &path, action));
        assert_same_scene(&actual.render(), &expected.render());
    }
    assert_eq!(actual.get("drawer-opens").unwrap(), Value::Int(1));
    assert_eq!(actual.get("drawer-closes").unwrap(), Value::Int(1));
    assert_eq!(actual.take_error(), None);
}

fn interpret_form_fixture(component: &str) -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/form/form.xr"),
        "examples/form/form.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(component)
        .unwrap()
}

#[test]
fn native_dialog_options_are_sampled_in_the_same_order() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{FileDialogMode, MessageButtons, MessageLevel, NativeDialogOptions};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("NativeDialogOptionOrderParity")
        .unwrap();
    let mut generated = expressions_ui::NativeDialogOptionOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    for (name, tag, value) in [
        (
            "file-mode",
            1,
            Value::FileDialogMode(FileDialogMode::SaveFile),
        ),
        ("filters", 5, Value::StringList(vec!["Text (*.txt)".into()])),
        (
            "message-level",
            6,
            Value::MessageLevel(MessageLevel::Warning),
        ),
        (
            "message-buttons",
            7,
            Value::MessageButtons(MessageButtons::OkCancel),
        ),
    ] {
        let calls = actual_calls.clone();
        interpreted
            .on(
                name,
                Rc::new(move |_| {
                    calls.borrow_mut().push(tag);
                    Ok(Some(value.clone()))
                }),
            )
            .unwrap();
    }
    let text = |calls: &RefCell<Vec<i64>>, tag| {
        let mut calls = calls.borrow_mut();
        calls.push(tag);
        format!("{tag}@{}", calls.len())
    };
    interpreted
        .on("text-option", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                Ok(Some(Value::String(text(&calls, *tag))))
            })
        })
        .unwrap();
    generated.on_file_mode({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push(1);
            FileDialogMode::SaveFile
        }
    });
    generated.on_filters({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push(5);
            vec!["Text (*.txt)".into()]
        }
    });
    generated.on_message_level({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push(6);
            MessageLevel::Warning
        }
    });
    generated.on_message_buttons({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push(7);
            MessageButtons::OkCancel
        }
    });
    generated.on_text_option({
        let calls = expected_calls.clone();
        move |tag| text(&calls, tag)
    });

    interpreted.invoke("open-file", &[]).unwrap();
    interpreted.invoke("open-file", &[]).unwrap();
    interpreted.invoke("open-message", &[]).unwrap();
    generated.invoke_open_file();
    generated.invoke_open_file();
    generated.invoke_open_message();
    let order: Vec<_> = (1..=10).collect();
    assert_eq!(expected_calls.take(), order);
    assert_eq!(actual_calls.take(), order);
    for scene in [interpreted.render_shared(), generated.render_shared()] {
        let [file, message] = scene.native_dialogs.as_slice() else {
            panic!("expected two dialogs");
        };
        let file = file.options();
        let message = message.options();
        let NativeDialogOptions::File(file) = file.as_ref() else {
            panic!("expected file options");
        };
        let NativeDialogOptions::Message(message) = message.as_ref() else {
            panic!("expected message options");
        };
        assert_eq!(file.title, "2@2");
        assert_eq!(file.directory, "3@3");
        assert_eq!(file.file_name, "4@4");
        assert_eq!(file.name_filters, ["Text (*.txt)"]);
        assert_eq!(message.title, "8@8");
        assert_eq!(message.text, "9@9");
        assert_eq!(message.informative_text, "10@10");
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn native_dialog_open_ignores_reentry_during_binding_evaluation() {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_runtime::NativeDialogOptions;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let mut outcomes = Vec::new();
    for reenter_from_enabled in [true, false] {
        let interpreted = program.create("NativeDialogOptionOrderParity").unwrap();
        let mut generated = expressions_ui::NativeDialogOptionOrderParity::new();
        let actual_calls = Rc::new(RefCell::new(Vec::new()));
        let compiled_calls = Rc::new(RefCell::new(Vec::new()));

        interpreted
            .on("file-enabled", {
                let calls = actual_calls.clone();
                let owner = Cell::new(reenter_from_enabled.then(|| interpreted.clone()));
                Rc::new(move |_| {
                    calls.borrow_mut().push(0);
                    if let Some(owner) = owner.take() {
                        owner.invoke("open-file", &[]).unwrap();
                    }
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        interpreted
            .on("text-option", {
                let calls = actual_calls.clone();
                let owner = Cell::new((!reenter_from_enabled).then(|| interpreted.clone()));
                Rc::new(move |args| {
                    let [Value::Int(tag)] = args else {
                        unreachable!()
                    };
                    calls.borrow_mut().push(*tag);
                    if *tag == 2
                        && let Some(owner) = owner.take()
                    {
                        assert_eq!(owner.get("file-pending"), Ok(false.into()));
                        owner.invoke("open-file", &[]).unwrap();
                    }
                    Ok(Some(Value::String(tag.to_string())))
                })
            })
            .unwrap();
        generated.on_file_enabled({
            let calls = compiled_calls.clone();
            let owner = generated.as_weak();
            let reenter = Cell::new(reenter_from_enabled);
            move || {
                calls.borrow_mut().push(0);
                if reenter.replace(false) {
                    owner.upgrade().unwrap().invoke_open_file();
                }
                true
            }
        });
        generated.on_text_option({
            let calls = compiled_calls.clone();
            let owner = generated.as_weak();
            let reenter = Cell::new(!reenter_from_enabled);
            move |tag| {
                calls.borrow_mut().push(tag);
                if tag == 2 && reenter.replace(false) {
                    let mut owner = owner.upgrade().unwrap();
                    assert!(!owner.get_file_pending());
                    owner.invoke_open_file();
                }
                tag.to_string()
            }
        });

        let actual = catch_unwind(AssertUnwindSafe(|| interpreted.invoke("open-file", &[])));
        let compiled = catch_unwind(AssertUnwindSafe(|| generated.invoke_open_file()));
        let actual_scene = interpreted.render_shared();
        let compiled_scene = generated.render_shared();
        outcomes.push((
            matches!(actual, Ok(Ok(None))),
            compiled.is_ok(),
            actual_calls.take(),
            compiled_calls.take(),
            actual_scene.native_dialogs.len(),
            compiled_scene.native_dialogs.len(),
        ));
        for scene in [&actual_scene, &compiled_scene] {
            if let Some(dialog) = scene.native_dialogs.first() {
                let options = dialog.options();
                let NativeDialogOptions::File(options) = options.as_ref() else {
                    panic!("expected file options");
                };
                assert_eq!(options.title, "2");
            }
        }
    }
    for outcome in outcomes {
        assert_eq!(
            outcome,
            (true, true, vec![0, 2, 3, 4], vec![0, 2, 3, 4], 1, 1)
        );
    }
}

#[test]
fn native_dialog_open_checks_live_permission_before_publishing() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::NativeDialogResult;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    for (command, pending, title) in [
        ("open-file", "file-pending", 2),
        ("open-message", "message-pending", 8),
    ] {
        let interpreted = program.create("NativeDialogOptionOrderParity").unwrap();
        let mut compiled = expressions_ui::NativeDialogOptionOrderParity::new();
        let actual_checks = Rc::new(Cell::new(0));
        let compiled_checks = Rc::new(Cell::new(0));
        interpreted
            .on("file-enabled", {
                let weak = interpreted.as_weak();
                let checks = actual_checks.clone();
                Rc::new(move |_| {
                    checks.set(checks.get() + 1);
                    Ok(Some(weak.upgrade().unwrap().get("can-open").unwrap()))
                })
            })
            .unwrap();
        compiled.on_file_enabled({
            let weak = compiled.as_weak();
            let checks = compiled_checks.clone();
            move || {
                checks.set(checks.get() + 1);
                weak.upgrade().unwrap().get_can_open()
            }
        });
        interpreted
            .on("text-option", {
                let weak = interpreted.as_weak();
                Rc::new(move |args| {
                    if args == [Value::Int(title)] {
                        weak.upgrade()
                            .unwrap()
                            .set("can-open", false.into())
                            .unwrap();
                    }
                    Ok(Some("draft".to_owned().into()))
                })
            })
            .unwrap();
        compiled.on_text_option({
            let weak = compiled.as_weak();
            move |tag| {
                if tag == title {
                    weak.upgrade().unwrap().set_can_open(false);
                }
                "draft".into()
            }
        });
        let open = |compiled: &mut expressions_ui::NativeDialogOptionOrderParity| {
            interpreted.invoke(command, &[]).unwrap();
            if command == "open-file" {
                compiled.invoke_open_file();
            } else {
                compiled.invoke_open_message();
            }
        };
        open(&mut compiled);
        assert_eq!(interpreted.get("can-open"), Ok(false.into()));
        assert!(!compiled.get_can_open());
        assert_eq!(interpreted.get(pending), Ok(false.into()));
        assert!(interpreted.render_shared().native_dialogs.is_empty());
        assert!(compiled.render_shared().native_dialogs.is_empty());

        interpreted.set("can-open", true.into()).unwrap();
        compiled.set_can_open(true);
        interpreted
            .on(
                "text-option",
                Rc::new(|_| Ok(Some("ready".to_owned().into()))),
            )
            .unwrap();
        compiled.on_text_option(|_| "ready".into());
        open(&mut compiled);
        assert_eq!(interpreted.get(pending), Ok(true.into()));
        let actual = interpreted.render_shared();
        let expected = compiled.render_shared();
        assert_eq!(
            (actual.native_dialogs.len(), expected.native_dialogs.len()),
            (1, 1)
        );
        open(&mut compiled);
        assert_eq!((actual_checks.get(), compiled_checks.get()), (2, 2));
        for scene in [actual, expected] {
            assert!(
                scene.native_dialogs[0]
                    .clone()
                    .deliver(NativeDialogResult::Abandoned)
            );
        }
        assert_eq!(interpreted.get(pending), Ok(false.into()));
        assert!(compiled.render_shared().native_dialogs.is_empty());
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn native_file_dialog_snapshots_options_and_commits_results_before_callbacks() {
    use form_ui::FileDialogDemo;
    use xrack_runtime::{
        FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
        StandardButtonKind,
    };

    let actual = interpret_form_fixture("FileDialogDemo");
    let mut expected = FileDialogDemo::new();
    actual
        .set("start-directory", Value::String("/workspace".into()))
        .unwrap();
    expected.set_start_directory("/workspace".into());

    let open = |mode| {
        actual
            .invoke("choose", &[Value::FileDialogMode(mode)])
            .unwrap()
    };
    open(FileDialogMode::OpenFiles);
    expected.invoke_choose(FileDialogMode::OpenFiles);
    let stale = actual.render_shared().native_dialogs[0].clone();
    let expected_stale = expected.render_shared().native_dialogs[0].clone();
    for registration in [&stale, &expected_stale] {
        assert!(
            !registration
                .clone()
                .deliver(NativeDialogResult::Button(StandardButtonKind::Ok))
        );
    }
    assert_eq!(actual.get("pending").unwrap(), Value::Bool(true));
    assert!(expected.get_pending());
    actual
        .set("start-directory", Value::String("/changed".into()))
        .unwrap();
    expected.set_start_directory("/changed".into());
    open(FileDialogMode::SaveFile);
    expected.invoke_choose(FileDialogMode::SaveFile);

    let mut actual_queue = NativeDialogQueue::default();
    let mut expected_queue = NativeDialogQueue::default();
    for (scene, stale, queue) in [
        (actual.render_shared(), &stale, &mut actual_queue),
        (
            expected.render_shared(),
            &expected_stale,
            &mut expected_queue,
        ),
    ] {
        assert_eq!(scene.native_dialogs.as_slice(), std::slice::from_ref(stale));
        assert_eq!(scene.children.len(), 1);
        queue.sync(&[&scene.native_dialogs[0]]);
        let request = queue.begin_next().unwrap();
        let NativeDialogOptions::File(options) = request.options.as_ref() else {
            panic!("expected a file dialog");
        };
        assert_eq!(options.mode, FileDialogMode::OpenFiles);
        assert_eq!(options.directory, "/workspace");
        assert_eq!(options.name_filters, ["Text files (*.txt *.md)"]);
        assert!(options.file_name.is_empty());
        assert!(queue.begin_next().is_none());
        assert!(queue.complete(
            request.id,
            NativeDialogResult::Files(vec!["/workspace/design.txt".into()])
        ));
    }
    assert_eq!(actual.get("pending").unwrap(), Value::Bool(false));
    assert_eq!(
        actual.get("callback-path").unwrap(),
        Value::String(expected.get_callback_path())
    );
    assert_eq!(
        actual.get("status").unwrap(),
        Value::String("已选择".into())
    );
    assert_eq!(
        actual.get("paths").unwrap(),
        Value::StringList(expected.get_paths())
    );

    open(FileDialogMode::SaveFile);
    expected.invoke_choose(FileDialogMode::SaveFile);
    assert!(!stale.deliver(NativeDialogResult::Files(vec!["/stale".into()])));
    assert!(!expected_stale.deliver(NativeDialogResult::Files(vec!["/stale".into()])));
    for (scene, queue) in [
        (actual.render_shared(), &mut actual_queue),
        (expected.render_shared(), &mut expected_queue),
    ] {
        queue.sync(&[&scene.native_dialogs[0]]);
        let request = queue.begin_next().unwrap();
        let NativeDialogOptions::File(options) = request.options.as_ref() else {
            panic!("expected a save dialog");
        };
        assert_eq!(options.mode, FileDialogMode::SaveFile);
        assert_eq!(options.file_name, "notes.txt");
        queue.sync(&[]);
        assert!(request.is_abandoned());
        assert!(queue.complete(
            request.id,
            NativeDialogResult::Files(vec!["/abandoned".into()])
        ));
    }
    assert_eq!(
        actual.get("paths").unwrap(),
        Value::StringList(vec!["/workspace/design.txt".into()])
    );
    assert_eq!(actual.get("pending").unwrap(), Value::Bool(false));
    assert!(actual.render_shared().native_dialogs.is_empty());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn native_message_buttons_and_close_guard_match_compiled_callbacks() {
    use form_ui::{CloseGuardDemo, MessageDialogDemo};
    use xrack_runtime::{
        MessageButtons, MessageLevel, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
        StandardButtonKind,
    };

    let actual = interpret_form_fixture("MessageDialogDemo");
    let mut expected = MessageDialogDemo::new();
    actual.set("confirm-enabled", Value::Bool(false)).unwrap();
    expected.set_confirm_enabled(false);
    actual.invoke("ask", &[]).unwrap();
    expected.invoke_ask();
    assert!(actual.render_shared().native_dialogs.is_empty());
    assert!(expected.render_shared().native_dialogs.is_empty());
    actual.set("confirm-enabled", Value::Bool(true)).unwrap();
    expected.set_confirm_enabled(true);

    for button in [
        StandardButtonKind::Yes,
        StandardButtonKind::No,
        StandardButtonKind::Cancel,
    ] {
        actual.invoke("ask", &[]).unwrap();
        expected.invoke_ask();
        for scene in [actual.render_shared(), expected.render_shared()] {
            let registration = scene.native_dialogs[0].clone();
            let options = registration.options();
            let NativeDialogOptions::Message(options) = options.as_ref() else {
                panic!("expected a message dialog");
            };
            assert_eq!(options.buttons, MessageButtons::YesNoCancel);
            assert_eq!(options.level, MessageLevel::Warning);
            let mut queue = NativeDialogQueue::default();
            queue.sync(&[&registration]);
            let request = queue.begin_next().unwrap();
            assert!(!queue.complete(request.id, NativeDialogResult::Canceled));
            assert!(queue.begin_next().is_none());
            assert!(queue.complete(request.id, NativeDialogResult::Button(button)));
        }
        assert_eq!(
            actual.get("answer").unwrap(),
            Value::StandardButtonKind(button)
        );
        assert_eq!(
            actual.get("status").unwrap(),
            Value::String(expected.get_status())
        );
    }
    assert_eq!(actual.get("accepts").unwrap(), Value::Int(1));
    assert_eq!(actual.get("rejects").unwrap(), Value::Int(2));

    let mut actual = interpret_form_fixture("CloseGuardDemo");
    let mut expected = CloseGuardDemo::new();
    actual.set("dirty", Value::Bool(true)).unwrap();
    expected.set_dirty(true);
    assert!(actual.dispatch("close-window", "clicked"));
    assert!(expected.dispatch("close-window", "clicked"));
    for ui in [&mut actual as &mut dyn Component, &mut expected] {
        assert!(ui.take_window_close_request());
        assert!(!ui.take_window_close_request());
        assert!(!ui.close_requested());
        let scene = ui.render_shared();
        assert!(
            scene.native_dialogs[0]
                .clone()
                .deliver(NativeDialogResult::Button(StandardButtonKind::Cancel))
        );
        assert!(!ui.take_window_close_request());
        assert!(!ui.close_requested());
        let scene = ui.render_shared();
        assert!(
            scene.native_dialogs[0]
                .clone()
                .deliver(NativeDialogResult::Button(StandardButtonKind::Ok))
        );
        assert!(ui.take_window_close_request());
        assert!(ui.close_requested());
        assert!(ui.render_shared().native_dialogs.is_empty());
    }
    assert_eq!(actual.get("dirty").unwrap(), Value::Bool(false));
    assert_eq!(
        actual.get("requests").unwrap(),
        Value::Int(expected.get_requests())
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn window_status_reactivity_and_command_order_match_compiled_mode() {
    use layout_ui::FramelessDemo;
    use xrack_runtime::{WindowCommand, WindowStatus};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/layout/layout.xr"),
        "layout.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("FramelessDemo")
        .unwrap();
    let mut expected = FramelessDemo::new();
    assert_same_scene(&actual.render(), &expected.render());

    for status in [
        WindowStatus {
            active: true,
            minimized: Some(false),
            maximized: true,
            full_screen: false,
        },
        WindowStatus {
            full_screen: true,
            ..WindowStatus::default()
        },
    ] {
        actual.publish_window_status(status);
        expected.publish_window_status(status);
        assert_same_scene(&actual.render(), &expected.render());
    }
    for (id, path) in [
        ("minimize-window", [0, 1, 2]),
        ("maximize-window", [0, 1, 3]),
        ("toggle-full-screen", [1, 2, 1]),
        ("restore-window", [1, 2, 2]),
        ("close-window", [0, 1, 4]),
    ] {
        assert_eq!(expected.render().at_path(&path).unwrap().id, Some(id));
        assert!(actual.dispatch_at(&actual.render(), &path, "clicked"));
        assert!(expected.dispatch(id, "clicked"));
    }
    assert_eq!(
        actual.take_window_commands(),
        [
            WindowCommand::Minimize,
            WindowCommand::ToggleMaximized,
            WindowCommand::ToggleFullScreen,
            WindowCommand::Restore
        ]
    );
    assert_eq!(
        expected.take_window_commands(),
        [
            WindowCommand::Minimize,
            WindowCommand::ToggleMaximized,
            WindowCommand::ToggleFullScreen,
            WindowCommand::Restore
        ]
    );
    assert!(actual.take_window_close_request() && expected.take_window_close_request());
    assert!(!actual.has_window_commands() && !expected.has_window_commands());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn window_commands_sample_only_their_required_bindings() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::WindowCommand;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("WindowCommandParity")
        .unwrap();
    let mut generated = expressions_ui::WindowCommandParity::new();
    let actual_reads = Rc::new(Cell::new(0));
    let generated_reads = Rc::new(Cell::new(0));
    let read = |reads: &Cell<i32>| {
        reads.set(reads.get() + 1);
        true
    };
    for callback in ["initial-visible", "full-screen-argument"] {
        let reads = actual_reads.clone();
        actual
            .on(callback, Rc::new(move |_| Ok(Some(read(&reads).into()))))
            .unwrap();
    }
    generated.on_initial_visible({
        let reads = generated_reads.clone();
        move || read(&reads)
    });
    generated.on_full_screen_argument({
        let reads = generated_reads.clone();
        move || read(&reads)
    });

    actual.invoke("hide-child", &[]).unwrap();
    generated.invoke_hide_child();
    assert_eq!((actual_reads.get(), generated_reads.get()), (0, 0));
    actual.invoke("close-child", &[]).unwrap();
    generated.invoke_close_child();
    assert!(actual.secondary_windows().unwrap().is_empty());
    assert!(generated.secondary_windows().unwrap().is_empty());
    assert_eq!((actual_reads.get(), generated_reads.get()), (0, 0));
    actual.reset("shown").unwrap();
    generated.reset_shown();
    actual.invoke("close-child", &[]).unwrap();
    generated.invoke_close_child();
    assert_eq!((actual_reads.get(), generated_reads.get()), (1, 1));
    for registrations in [
        actual.secondary_windows().unwrap(),
        generated.secondary_windows().unwrap(),
    ] {
        assert_eq!(registrations.len(), 1);
        assert!(
            registrations[0]
                .mount()
                .unwrap()
                .take_window_close_request()
        );
    }
    actual.invoke("window-commands", &[]).unwrap();
    generated.invoke_window_commands();
    let expected = [
        WindowCommand::Maximize,
        WindowCommand::SetFullScreen(true),
        WindowCommand::Restore,
    ];
    assert_eq!(actual.take_window_commands(), expected);
    assert_eq!(generated.take_window_commands(), expected);
    assert_eq!((actual_reads.get(), generated_reads.get()), (2, 2));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn application_window_body_metrics_and_close_policy_match_compiled_mode() {
    use navigation_ui::ApplicationWindowDemo;
    use xrack_layout::{Interaction, LayoutObservers, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/navigation/navigation.xr"),
        "navigation.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ApplicationWindowDemo")
        .unwrap();
    let mut expected = ApplicationWindowDemo::new();

    let input = Interaction::default();
    let size = Size::new(640.0, 400.0);
    let mut actual_observers = LayoutObservers::default();
    let mut expected_observers = LayoutObservers::default();
    let mut heights = Vec::new();
    for show_header in [true, false] {
        actual.set("show-header", Value::Bool(show_header)).unwrap();
        expected.set_show_header(show_header);
        let (actual_scene, actual_layout) = prepare_list(&actual, &input, size);
        let (expected_scene, expected_layout) = prepare_list(&expected, &input, size);
        assert!(actual_scene.application_window && expected_scene.application_window);
        actual_observers.sync(&actual_scene, &actual_layout);
        expected_observers.sync(&expected_scene, &expected_layout);
        assert_eq!(
            actual.get("body-width").unwrap(),
            Value::Length(expected.get_body_width())
        );
        let height = expected.get_body_height();
        assert_eq!(actual.get("body-height").unwrap(), Value::Length(height));
        assert_same_scene(&actual.render(), &expected.render());
        heights.push(height.0);
    }
    assert!(heights[1] > heights[0]);

    actual.set("allow-close", Value::Bool(false)).unwrap();
    expected.set_allow_close(false);
    assert!(!actual.close_requested() && !expected.close_requested());
    assert_eq!(
        actual.get("close-attempts").unwrap(),
        Value::Int(expected.get_close_attempts())
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn secondary_window_mount_visibility_and_close_guard_match_compiled_mode() {
    use counter_ui::SecondaryWindowDemo;
    use std::rc::Rc;

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/counter/counter.xr"),
        "counter.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("SecondaryWindowDemo")
        .unwrap();
    let mut expected = SecondaryWindowDemo::new();

    fn exercise(ui: &mut impl Component) -> (Node, Node) {
        let show = [0, 3, 0];
        let hide = [0, 3, 1];
        let close = [0, 3, 2];
        let add = [0, 2];
        let note = [0, 1];
        let allow = [0, 2];
        assert!(ui.secondary_windows().unwrap().is_empty());

        assert!(ui.dispatch_at(&ui.render(), &show, "clicked"));
        let first = ui.secondary_windows().unwrap();
        assert_eq!(first.len(), 1);
        let id = first[0].id();
        let mut child = first[0].mount().unwrap();
        assert_eq!(child.render().text("title"), Some("XRack · 辅助窗口"));
        assert!(ui.dispatch_at(&ui.render(), &add, "clicked"));
        assert!(Rc::ptr_eq(&first, &ui.secondary_windows().unwrap()));
        assert_eq!(
            child.render().at_path(&[0, 0]).unwrap().text("text"),
            Some("共享计数 · 1")
        );
        assert!(child.edit_at(&child.render(), &note, "保留草稿".into()));
        assert!(child.dispatch_at(&child.render(), &allow, "clicked"));
        assert!(ui.dispatch_at(&ui.render(), &close, "clicked"));
        assert!(child.take_window_close_request());
        assert!(!child.close_requested());
        assert_eq!(ui.secondary_windows().unwrap()[0].id(), id);

        assert!(ui.dispatch_at(&ui.render(), &hide, "clicked"));
        assert!(ui.secondary_windows().unwrap().is_empty());
        assert!(ui.dispatch_at(&ui.render(), &close, "clicked"));
        assert!(!child.take_window_close_request());
        drop(child);

        assert!(ui.dispatch_at(&ui.render(), &show, "clicked"));
        let visible = ui.secondary_windows().unwrap();
        assert_eq!(visible[0].id(), id);
        let mut child = visible[0].mount().unwrap();
        let scene = child.render();
        assert_eq!(scene.at_path(&note).unwrap().text("text"), Some("保留草稿"));
        assert_eq!(
            scene.at_path(&[0, 3]).unwrap().text("text"),
            Some("关闭请求 · 1")
        );

        assert!(child.dispatch_at(&scene, &allow, "clicked"));
        assert!(ui.dispatch_at(&ui.render(), &close, "clicked"));
        assert!(child.take_window_close_request());
        assert!(child.close_requested());
        visible[0].closed();
        assert!(ui.secondary_windows().unwrap().is_empty());

        assert!(ui.dispatch_at(&ui.render(), &show, "clicked"));
        assert_eq!(ui.secondary_windows().unwrap()[0].id(), id);
        (
            ui.render(),
            ui.secondary_windows().unwrap()[0].mount().unwrap().render(),
        )
    }

    let (parent, child) = exercise(&mut actual);
    let (expected_parent, expected_child) = exercise(&mut expected);
    assert_same_scene(&parent, &expected_parent);
    assert_same_scene(&child, &expected_child);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn mounted_secondary_window_falls_back_after_parent_drop() {
    use expressions_ui::RetainedWindowParent;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RetainedWindowParent")
        .unwrap();
    let mut generated = RetainedWindowParent::new();
    actual.set("count", 19_i64.into()).unwrap();
    generated.set_count(19);
    let mut actual_child = actual.secondary_windows().unwrap()[0].mount().unwrap();
    let mut generated_child = generated.secondary_windows().unwrap()[0].mount().unwrap();

    for child in [&mut actual_child, &mut generated_child] {
        let scene = child.render();
        assert_eq!(scene.children[0].text("text"), Some("19/41/parent"));
        assert_eq!(scene.children[1].text("text"), Some("7/false/0"));
        assert!(child.dispatch_at(&scene, &[2], "clicked"));
    }
    assert_eq!(actual.get("shared"), Ok(Value::String("pinged".into())));
    assert_eq!(generated.get_shared(), "pinged");
    assert_same_scene(&actual_child.render(), &generated_child.render());
    assert!(!actual_child.scene_dirty());
    assert!(!generated_child.scene_dirty());

    drop(actual);
    drop(generated);
    assert!(actual_child.scene_dirty());
    assert!(generated_child.scene_dirty());

    for child in [&mut actual_child, &mut generated_child] {
        let scene = child.render();
        assert_eq!(scene.children[0].text("text"), Some("3/4/local"));
        assert_eq!(scene.children[1].text("text"), Some("0/true/1"));
        assert!(child.dispatch_at(&scene, &[2], "clicked"));
        let scene = child.render();
        assert_eq!(scene.children[1].text("text"), Some("0/true/2"));
        assert!(child.edit_at(&scene, &[3], "ignored".into()));
        assert_eq!(child.render().children[3].text("text"), Some("local"));
    }
    assert_same_scene(&actual_child.render(), &generated_child.render());
}

#[test]
fn slider_interval_dial_and_wrap_match_compiled_controls() {
    use controls_ui::ControlsDemo;
    use xrack_runtime::RangeAction;

    let mut actual = interpret_controls_fixture("ControlsDemo");
    let mut expected = ControlsDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    for (path, action) in [
        (&[2, 1][..], RangeAction::SetFraction(0.42)),
        (&[2, 1], RangeAction::Release),
        (&[5, 1, 0], RangeAction::SetValue(92.0)),
        (&[5, 1, 1], RangeAction::CommitFraction(0.27)),
        (&[7, 3], RangeAction::CommitFraction(0.76)),
        (&[7, 4], RangeAction::Step(2)),
        (&[7, 5], RangeAction::Increase),
    ] {
        let scene = actual.render();
        let expected_scene = expected.render();
        assert_eq!(
            actual.adjust_range_at(&scene, path, action),
            expected.adjust_range_at(&expected_scene, path, action)
        );
        assert_same_scene(&actual.render(), &expected.render());
    }
    for (name, value) in [
        ("last-release", Value::Float(expected.get_last_release())),
        (
            "dial-last-release",
            Value::Float(expected.get_dial_last_release()),
        ),
        (
            "released-lower",
            Value::Float(expected.get_released_lower()),
        ),
        (
            "released-upper",
            Value::Float(expected.get_released_upper()),
        ),
        (
            "interval-changes",
            Value::Int(expected.get_interval_changes()),
        ),
        (
            "interval-releases",
            Value::Int(expected.get_interval_releases()),
        ),
    ] {
        assert_eq!(actual.get(name).unwrap(), value);
    }
    actual.set("muted", Value::Bool(true)).unwrap();
    expected.set_muted(true);
    assert!(!actual.adjust_range_at(&actual.render(), &[5, 1, 0], RangeAction::Increase));
    assert!(!expected.adjust_range_at(&expected.render(), &[5, 1, 0], RangeAction::Increase));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn deferred_slider_and_release_snap_commit_identically() {
    use controls_ui::SnapSlidersDemo;
    use xrack_layout::{Interaction, Size, SliderGeometry};
    use xrack_runtime::{Orientation, RangeAction};

    let mut actual = interpret_controls_fixture("SnapSlidersDemo");
    let mut expected = SnapSlidersDemo::new();

    fn drag(ui: &mut impl Component, path: &[usize], live: bool) {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(500.0, 290.0));
        let geometry = SliderGeometry::new(
            layout.at_path(path).unwrap().bounds,
            Orientation::Horizontal,
        );
        assert!(input.range_pointer_press(&scene, &layout, geometry.position(0.2)));
        assert!(input.slider_press(&scene, &layout).is_none());
        input.pointer_move(&scene, &layout, geometry.position(0.46));
        let draft = input.slider_drag(&scene, &layout);
        if let Some(action) = draft {
            assert!(live);
            assert!(ui.adjust_range_at(&scene, &action.path, action.action));
        } else {
            assert!(!live);
            assert_eq!(
                ui.render().at_path(path).unwrap().number("value"),
                Some(20.0)
            );
        }
        let scene = ui.render_shared();
        let commit = input.slider_release(&scene, &layout).unwrap();
        assert!(matches!(commit.action, RangeAction::CommitFraction(_)));
        assert!(ui.adjust_range_at(&scene, &commit.path, commit.action));
    }

    drag(&mut actual, &[0, 3], false);
    drag(&mut expected, &[0, 3], false);
    assert_eq!(actual.get("level").unwrap(), Value::Float(50.0));
    assert_eq!(actual.get("level-changes").unwrap(), Value::Int(1));
    assert_eq!(actual.get("level-releases").unwrap(), Value::Int(1));
    actual.set("live-update", Value::Bool(true)).unwrap();
    expected.set_live_update(true);
    drag(&mut actual, &[0, 5], true);
    drag(&mut expected, &[0, 5], true);
    assert_eq!(actual.get("lower").unwrap(), Value::Float(50.0));
    assert_eq!(actual.get("band-changes").unwrap(), Value::Int(2));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn range_inputs_reject_old_limits_and_releases_use_live_values() {
    use expressions_ui::RangeOptionsParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::RangeAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RangeOptionsParity")
        .unwrap();
    let mut generated = RangeOptionsParity::new();
    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    actual.set("limit", 20.0.into()).unwrap();
    actual.set("amount", 40.0.into()).unwrap();
    generated.set_limit(20.0);
    generated.set_amount(40.0);
    let slider = [0];
    let dial = [1];
    let upper = [2, 1];
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*stale),
        (&mut generated, &*compiled_stale),
    ] {
        assert!(!ui.adjust_range_at(scene, &slider, RangeAction::CommitFraction(0.75)));
        assert!(!ui.adjust_range_at(scene, &dial, RangeAction::Increase));
        assert!(!ui.adjust_range_at(scene, &upper, RangeAction::CommitFraction(0.75)));
        assert!(ui.adjust_range_at(scene, &slider, RangeAction::Release));
        assert!(ui.adjust_range_at(scene, &upper, RangeAction::Release));
    }
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(generated.get_changes(), 0);
    assert_eq!(actual.get("releases"), Ok(2_i64.into()));
    assert_eq!(generated.get_releases(), 2);
    assert_eq!(actual.get("last-release"), Ok(20.0.into()));
    assert_eq!(generated.get_last_release(), 20.0);

    actual
        .on("after-change", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("limit", 5.0.into()).unwrap();
                }
                Ok(None)
            })
        })
        .unwrap();
    generated.on_after_change({
        let owner = generated.as_weak();
        move || owner.upgrade().unwrap().set_limit(5.0)
    });
    for ui in [&mut actual as &mut dyn Component, &mut generated] {
        assert!(ui.adjust_range_at(
            &ui.render_shared(),
            &slider,
            RangeAction::CommitFraction(0.5)
        ));
    }
    assert_eq!(actual.get("amount"), Ok(10.0.into()));
    assert_eq!(generated.get_amount(), 10.0);
    assert_eq!(actual.get("last-release"), Ok(5.0.into()));
    assert_eq!(generated.get_last_release(), 5.0);
    assert_eq!(actual.get("changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(actual.get("releases"), Ok(3_i64.into()));
    assert_eq!(generated.get_releases(), 3);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn spin_commits_reject_stale_limits_precision_and_editability() {
    use expressions_ui::SpinOptionsParity;
    use xrack_runtime::{ControlAction, SpinAction, SpinDecimal};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("SpinOptionsParity")
        .unwrap();
    let mut generated = SpinOptionsParity::new();
    let integer = [0];
    let decimal = [1];
    let draft = |original, value| {
        ControlAction::Spin(SpinAction::CommitDecimalIfCurrent {
            original: SpinDecimal::new(original),
            value: SpinDecimal::new(value).unwrap(),
        })
    };
    let integer_commit = ControlAction::Spin(SpinAction::CommitIfCurrent {
        original: 10,
        value: 75,
    });
    let decimal_commit = draft(10.0, 75.0);
    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    actual.set("integer-limit", 20_i64.into()).unwrap();
    actual.set("decimal-limit", 20.0.into()).unwrap();
    generated.set_integer_limit(20);
    generated.set_decimal_limit(20.0);
    for (ui, old_scene) in [
        (&mut actual as &mut dyn Component, &*scene),
        (&mut generated, &*compiled_scene),
    ] {
        assert!(!ui.control_at(old_scene, &integer, integer_commit));
        assert!(!ui.control_at(old_scene, &decimal, decimal_commit));
        let current = ui.render_shared();
        assert!(ui.control_at(&current, &integer, integer_commit));
        assert!(ui.control_at(&current, &decimal, decimal_commit));
    }
    assert_eq!(actual.get("amount"), Ok(20_i64.into()));
    assert_eq!(generated.get_amount(), 20);
    assert_eq!(actual.get("level"), Ok(20.0.into()));
    assert_eq!(generated.get_level(), 20.0);

    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    actual.set("precision", 1_i64.into()).unwrap();
    generated.set_precision(1);
    let commit = draft(20.0, 12.345);
    for (ui, old_scene) in [
        (&mut actual as &mut dyn Component, &*scene),
        (&mut generated, &*compiled_scene),
    ] {
        assert!(!ui.control_at(old_scene, &decimal, commit));
        assert!(ui.control_at(&ui.render_shared(), &decimal, commit));
    }
    assert_eq!(actual.get("level"), Ok(12.3.into()));
    assert_eq!(generated.get_level(), 12.3);

    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    actual.set("unlocked", false.into()).unwrap();
    generated.set_unlocked(false);
    let step = ControlAction::Spin(SpinAction::Decrease);
    for (ui, old_scene) in [
        (&mut actual as &mut dyn Component, &*scene),
        (&mut generated, &*compiled_scene),
    ] {
        assert!(!ui.control_at(old_scene, &integer, step));
        assert!(!ui.control_at(old_scene, &decimal, step));
    }
    assert_eq!(actual.get("edits"), Ok(3_i64.into()));
    assert_eq!(generated.get_edits(), 3);
    assert_same_scene(&actual.render_shared(), &generated.render_shared());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn row_toggle_input_rechecks_permissions_settings_and_history() {
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::CheckState;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let size = Size::new(420.0, 300.0);
    fn prepare(ui: &impl Component, size: Size) -> Rc<Node> {
        prepare_list(ui, &Interaction::default(), size).0
    }
    for view in [0, 1] {
        let mut interpreted = program.create("RowToggleInputParity").unwrap();
        let mut compiled = expressions_ui::RowToggleInputParity::new();
        let checked = [0, view, 0, 0, 0];
        let state = [0, view, 0, 0, 1];
        let actual = prepare(&interpreted, size);
        let expected = prepare(&compiled, size);
        let actual_rows = interpreted.record_model("rows").unwrap();
        let compiled_rows = compiled.model_rows();
        let assert_row = |checked, state| {
            let row = actual_rows.model().row(0).unwrap();
            assert_eq!(row.field("checked"), Some(&Value::Bool(checked)));
            assert_eq!(row.field("state"), Some(&Value::CheckState(state)));
            let row = compiled_rows.row(0).unwrap();
            assert_eq!((row.checked, row.state), (checked, state));
        };
        assert_eq!(actual.at_path(&state).unwrap().kind, "CheckBox");
        assert_eq!(expected.at_path(&state).unwrap().kind, "CheckBox");
        let reads = Rc::new(Cell::new(0));
        let compiled_reads = Rc::new(Cell::new(0));
        interpreted
            .on("record-history", {
                let reads = reads.clone();
                Rc::new(move |_| {
                    reads.set(reads.get() + 1);
                    Ok(Some(true.into()))
                })
            })
            .unwrap();
        compiled.on_record_history({
            let reads = compiled_reads.clone();
            move || {
                reads.set(reads.get() + 1);
                true
            }
        });
        interpreted.set("allowed", false.into()).unwrap();
        compiled.set_allowed(false);
        assert!(!interpreted.dispatch_at(&actual, &checked, "clicked"));
        assert!(
            !compiled.dispatch_at(&expected, &checked, "clicked"),
            "view {view} used stale enabled"
        );
        interpreted.set("allowed", true.into()).unwrap();
        compiled.set_allowed(true);
        interpreted.set("three-states", false.into()).unwrap();
        compiled.set_three_states(false);
        assert!(
            !interpreted.dispatch_at(&actual, &state, "clicked"),
            "view {view} used stale tristate"
        );
        assert!(
            !compiled.dispatch_at(&expected, &state, "clicked"),
            "view {view} used stale tristate"
        );
        assert_eq!((reads.get(), compiled_reads.get()), (0, 0));
        assert_row(false, CheckState::Unchecked);

        let actual = prepare(&interpreted, size);
        let expected = prepare(&compiled, size);
        if view == 0 {
            interpreted
                .on("record-history", {
                    let weak = interpreted.as_weak();
                    Rc::new(move |_| {
                        weak.upgrade()
                            .unwrap()
                            .set("allowed", false.into())
                            .unwrap();
                        Ok(Some(true.into()))
                    })
                })
                .unwrap();
            compiled.on_record_history({
                let weak = compiled.as_weak();
                move || {
                    weak.upgrade().unwrap().set_allowed(false);
                    true
                }
            });
            assert!(!interpreted.dispatch_at(&actual, &checked, "clicked"));
            assert!(!compiled.dispatch_at(&expected, &checked, "clicked"));
            assert_row(false, CheckState::Unchecked);
            interpreted.set("allowed", true.into()).unwrap();
            compiled.set_allowed(true);
            interpreted
                .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
                .unwrap();
            compiled.on_record_history(|| true);
        }
        assert!(interpreted.dispatch_at(&actual, &checked, "clicked"));
        assert!(compiled.dispatch_at(&expected, &checked, "clicked"));
        let actual = prepare(&interpreted, size);
        let expected = prepare(&compiled, size);
        assert!(interpreted.dispatch_at(&actual, &state, "clicked"));
        assert!(compiled.dispatch_at(&expected, &state, "clicked"));
        assert_row(true, CheckState::Checked);
        if view == 0 {
            for (ui, scene) in [
                (&interpreted as &dyn Component, &*actual),
                (&compiled, &*expected),
            ] {
                assert!(ui.replay_table_edit_at(scene, &[0, view], false));
            }
            assert_row(true, CheckState::Unchecked);
        }
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn virtual_numeric_editors_recheck_row_availability() {
    use std::rc::Rc;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, RangeAction, SpinAction};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let interaction = Interaction::default();
    for view in [0, 1] {
        let mut interpreted = program.create("RowSpinOptionsParity").unwrap();
        let mut compiled = expressions_ui::RowSpinOptionsParity::new();
        let size = Size::new(480.0, 250.0);
        let (actual, _) = prepare_list(&interpreted, &interaction, size);
        let (expected, _) = prepare_list(&compiled, &interaction, size);
        let spin = [view, 0, 0, 0];
        let slider = [view, 0, 0, 2];
        let click_spin = |ui: &mut dyn Component, scene: &Node| {
            ui.control_at(scene, &spin, ControlAction::Spin(SpinAction::Increase))
        };
        let move_slider = |ui: &mut dyn Component, scene: &Node| {
            ui.adjust_range_at(scene, &slider, RangeAction::CommitFraction(0.4))
        };
        interpreted.set("row-visible", false.into()).unwrap();
        compiled.set_row_visible(false);
        assert!(!click_spin(&mut interpreted, &actual));
        assert!(
            !click_spin(&mut compiled, &expected),
            "hidden row in view {view}"
        );
        assert!(!move_slider(&mut interpreted, &actual));
        assert!(
            !move_slider(&mut compiled, &expected),
            "hidden row in view {view}"
        );
        interpreted.set("row-visible", true.into()).unwrap();
        compiled.set_row_visible(true);
        if view == 0 {
            interpreted
                .on("record-history", {
                    let weak = interpreted.as_weak();
                    Rc::new(move |_| {
                        weak.upgrade()
                            .unwrap()
                            .set("row-visible", false.into())
                            .unwrap();
                        Ok(Some(true.into()))
                    })
                })
                .unwrap();
            compiled.on_record_history({
                let weak = compiled.as_weak();
                move || {
                    weak.upgrade().unwrap().set_row_visible(false);
                    true
                }
            });
            assert!(!move_slider(&mut interpreted, &actual));
            assert!(!move_slider(&mut compiled, &expected));
            interpreted.set("row-visible", true.into()).unwrap();
            compiled.set_row_visible(true);
            interpreted
                .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
                .unwrap();
            compiled.on_record_history(|| true);
        }
        let rows = interpreted.record_model("rows").unwrap();
        assert_eq!(
            rows.model().row(0).unwrap().field("amount"),
            Some(&Value::Int(10))
        );
        assert_eq!(compiled.model_rows().row(0).unwrap().amount, 10);
        assert_eq!(
            rows.model().row(0).unwrap().field("level"),
            Some(&Value::Float(10.0))
        );
        assert_eq!(compiled.model_rows().row(0).unwrap().level, 10.0);
        assert!(move_slider(&mut interpreted, &actual));
        assert!(move_slider(&mut compiled, &expected));
        assert_eq!(
            rows.model().row(0).unwrap().field("level"),
            Some(&Value::Float(40.0))
        );
        assert_eq!(compiled.model_rows().row(0).unwrap().level, 40.0);
        let (actual, _) = prepare_list(&interpreted, &interaction, size);
        let (expected, _) = prepare_list(&compiled, &interaction, size);
        assert!(click_spin(&mut interpreted, &actual));
        assert!(click_spin(&mut compiled, &expected));
        assert_eq!(
            rows.model().row(0).unwrap().field("amount"),
            Some(&Value::Int(11))
        );
        assert_eq!(compiled.model_rows().row(0).unwrap().amount, 11);
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn virtual_range_commits_recheck_row_limits_and_history_callbacks() {
    use expressions_ui::RowSpinOptionsParity;
    use std::rc::Rc;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::RangeAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let input = Interaction::default();
    let size = Size::new(480.0, 250.0);
    for (view, column, kind) in [
        (0, 2, "Slider"),
        (1, 2, "Slider"),
        (0, 3, "Dial"),
        (1, 3, "Dial"),
    ] {
        let mut actual = program.create("RowSpinOptionsParity").unwrap();
        let mut generated = RowSpinOptionsParity::new();
        let range = [view, 0, 0, column];
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        assert_eq!(scene.at_path(&range).unwrap().kind, kind);
        assert_eq!(compiled_scene.at_path(&range).unwrap().kind, kind);

        if kind == "Dial" {
            actual.set("dial-wrap", true.into()).unwrap();
            generated.set_dial_wrap(true);
            assert!(!actual.adjust_range_at(&scene, &range, RangeAction::Increase));
            assert!(!generated.adjust_range_at(&compiled_scene, &range, RangeAction::Increase));
            actual.set("dial-wrap", false.into()).unwrap();
            generated.set_dial_wrap(false);
        }

        let Value::RecordList(rows) = actual.get("rows").unwrap() else {
            panic!("expected rows");
        };
        let model = rows.model();
        let compiled_model = generated.model_rows();
        let set_limit = |model: &xrack_runtime::VecModel<xrack_runtime::RecordValue>,
                         limit: i64| {
            let row = model
                .row(0)
                .unwrap()
                .with_field(&["limit".into()], limit.into())
                .unwrap();
            assert!(model.set_row(0, row));
        };
        set_limit(model, 20);
        let mut row = compiled_model.row(0).unwrap();
        row.limit = 20;
        assert!(compiled_model.set_row(0, row));
        let commit = RangeAction::CommitFraction(0.75);
        assert!(!actual.adjust_range_at(&scene, &range, commit), "{kind}");
        assert!(
            !generated.adjust_range_at(&compiled_scene, &range, commit),
            "{kind}"
        );
        assert_eq!(
            model.row(0).unwrap().field("level"),
            Some(&Value::Float(10.0))
        );
        assert_eq!(compiled_model.row(0).unwrap().level, 10.0);

        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        assert!(actual.adjust_range_at(&scene, &range, commit), "{kind}");
        assert!(
            generated.adjust_range_at(&compiled_scene, &range, commit),
            "{kind}"
        );
        assert_eq!(
            model.row(0).unwrap().field("level"),
            Some(&Value::Float(15.0))
        );
        assert_eq!(compiled_model.row(0).unwrap().level, 15.0);

        if view == 0 {
            let (scene, _) = prepare_list(&actual, &input, size);
            let (compiled_scene, _) = prepare_list(&generated, &input, size);
            actual
                .on("record-history", {
                    let model = model.clone();
                    Rc::new(move |_| {
                        set_limit(&model, 5);
                        Ok(Some(true.into()))
                    })
                })
                .unwrap();
            generated.on_record_history({
                let model = compiled_model.clone();
                move || {
                    let mut row = model.row(0).unwrap();
                    row.limit = 5;
                    assert!(model.set_row(0, row));
                    true
                }
            });
            let commit = RangeAction::CommitFraction(0.5);
            assert!(!actual.adjust_range_at(&scene, &range, commit), "{kind}");
            assert!(
                !generated.adjust_range_at(&compiled_scene, &range, commit),
                "{kind}"
            );
            let row = model.row(0).unwrap();
            assert_eq!(row.field("limit"), Some(&Value::Int(5)));
            assert_eq!(row.field("level"), Some(&Value::Float(15.0)));
            assert_eq!(compiled_model.row(0).unwrap().limit, 5);
            assert_eq!(compiled_model.row(0).unwrap().level, 15.0);

            actual
                .on("record-history", Rc::new(|_| Ok(Some(true.into()))))
                .unwrap();
            generated.on_record_history(|| true);
            let actual_scene = actual.render_shared();
            let compiled_scene = generated.render_shared();
            assert!(actual.replay_table_edit_at(&actual_scene, &[0], false));
            assert!(generated.replay_table_edit_at(&compiled_scene, &[0], false));
            assert_eq!(
                model.row(0).unwrap().field("level"),
                Some(&Value::Float(10.0))
            );
            assert_eq!(compiled_model.row(0).unwrap().level, 10.0);
            assert_eq!(model.row(0).unwrap().field("limit"), Some(&Value::Int(5)));
            assert_eq!(compiled_model.row(0).unwrap().limit, 5);
        }
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn virtual_spin_commits_recheck_row_limits_and_precision() {
    use expressions_ui::RowSpinOptionsParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, SpinAction, SpinDecimal};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let input = Interaction::default();
    let size = Size::new(480.0, 250.0);
    let integer_commit = ControlAction::Spin(SpinAction::CommitIfCurrent {
        original: 10,
        value: 75,
    });
    let decimal_commit = |original, value| {
        ControlAction::Spin(SpinAction::CommitDecimalIfCurrent {
            original: SpinDecimal::new(original),
            value: SpinDecimal::new(value).unwrap(),
        })
    };
    for view in [0, 1] {
        let mut actual = program.create("RowSpinOptionsParity").unwrap();
        let mut generated = RowSpinOptionsParity::new();
        let integer = [view, 0, 0, 0];
        let decimal = [view, 0, 0, 1];
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        let Value::RecordList(rows) = actual.get("rows").unwrap() else {
            panic!("expected rows");
        };
        let row = rows
            .model()
            .row(0)
            .unwrap()
            .with_field(&["limit".into()], 20_i64.into())
            .unwrap();
        assert!(rows.model().set_row(0, row));
        let mut row = generated.model_rows().row(0).unwrap();
        row.limit = 20;
        assert!(generated.model_rows().set_row(0, row));
        for (ui, old_scene) in [
            (&mut actual as &mut dyn Component, &*scene),
            (&mut generated, &*compiled_scene),
        ] {
            assert!(!ui.control_at(old_scene, &integer, integer_commit));
            assert!(!ui.control_at(old_scene, &decimal, decimal_commit(10.0, 75.0)));
        }
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        for (ui, current) in [
            (&mut actual as &mut dyn Component, &*scene),
            (&mut generated, &*compiled_scene),
        ] {
            assert!(ui.control_at(current, &integer, integer_commit));
            assert!(ui.control_at(current, &decimal, decimal_commit(10.0, 75.0)));
        }
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        actual.set("precision", 1_i64.into()).unwrap();
        generated.set_precision(1);
        let commit = decimal_commit(20.0, 12.345);
        assert!(!actual.control_at(&scene, &decimal, commit));
        assert!(!generated.control_at(&compiled_scene, &decimal, commit));
        let (scene, _) = prepare_list(&actual, &input, size);
        let (compiled_scene, _) = prepare_list(&generated, &input, size);
        assert!(actual.control_at(&scene, &decimal, commit));
        assert!(generated.control_at(&compiled_scene, &decimal, commit));
        let row = rows.model().row(0).unwrap();
        assert_eq!(row.field("amount"), Some(&Value::Int(20)));
        assert_eq!(row.field("level"), Some(&Value::Float(12.3)));
        assert_eq!(generated.get_rows()[0].amount, 20);
        assert_eq!(generated.get_rows()[0].level, 12.3);
        if view == 0 {
            let (scene, _) = prepare_list(&actual, &input, size);
            let (compiled_scene, _) = prepare_list(&generated, &input, size);
            actual
                .on("record-history", {
                    let owner = Cell::new(Some(actual.clone()));
                    Rc::new(move |_| {
                        if let Some(owner) = owner.take() {
                            owner.set("precision", 2_i64.into()).unwrap();
                        }
                        Ok(Some(true.into()))
                    })
                })
                .unwrap();
            generated.on_record_history({
                let owner = generated.as_weak();
                move || {
                    owner.upgrade().unwrap().set_precision(2);
                    true
                }
            });
            let commit = decimal_commit(12.3, 11.11);
            assert!(!actual.control_at(&scene, &decimal, commit));
            assert!(!generated.control_at(&compiled_scene, &decimal, commit));
            assert_eq!(actual.get("precision"), Ok(2_i64.into()));
            assert_eq!(generated.get_precision(), 2);
            assert_eq!(
                rows.model().row(0).unwrap().field("level"),
                Some(&Value::Float(12.3))
            );
            assert_eq!(generated.get_rows()[0].level, 12.3);
        }
        assert_eq!(actual.take_error(), None);
    }
}

#[test]
fn decimal_spin_commit_guards_live_values_and_virtual_record_fields() {
    use controls_ui::{DecimalPrice, DecimalSpinDemo};
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, SpinAction, SpinDecimal};

    let mut actual = interpret_controls_fixture("DecimalSpinDemo");
    let mut expected = DecimalSpinDemo::new();
    let path = [1];
    let decimal = |original, value| {
        ControlAction::Spin(SpinAction::CommitDecimalIfCurrent {
            original: SpinDecimal::new(original),
            value: SpinDecimal::new(value).unwrap(),
        })
    };
    for action in [
        ControlAction::Spin(SpinAction::Increase),
        decimal(0.3, -1.236),
    ] {
        assert!(actual.control_at(&actual.render(), &path, action));
        assert!(expected.control_at(&expected.render(), &path, action));
    }
    assert_eq!(actual.get("level").unwrap(), Value::Float(-1.24));
    let scene = actual.render();
    let expected_scene = expected.render();
    actual.set("level", Value::Float(1.1)).unwrap();
    expected.set_level(1.1);
    assert!(!actual.control_at(&scene, &path, decimal(-1.24, 1.5)));
    assert!(!expected.control_at(&expected_scene, &path, decimal(-1.24, 1.5)));

    let Value::RecordList(prices) = actual.get("prices").unwrap() else {
        panic!("expected prices model")
    };
    let model = prices.model();
    let expected_model = expected.model_prices();
    let input = Interaction::default();
    let size = Size::new(480.0, 306.0);
    let cell = [5, 0, 0, 1];
    let (scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    let row = model
        .row(0)
        .unwrap()
        .with_field(&["label".into()], Value::String("已改名".into()))
        .unwrap();
    assert!(model.set_row(0, row));
    assert!(expected_model.set_row(
        0,
        DecimalPrice {
            label: "已改名".into(),
            price: 12.5
        }
    ));
    let commit = decimal(12.5, 12.75);
    assert!(actual.control_at(&scene, &cell, commit));
    assert!(expected.control_at(&expected_scene, &cell, commit));
    assert!(!actual.control_at(&scene, &cell, commit));
    assert!(!expected.control_at(&expected_scene, &cell, commit));
    let row = model.row(0).unwrap();
    assert_eq!(row.field("label"), Some(&Value::String("已改名".into())));
    assert_eq!(row.field("price"), Some(&Value::Float(12.75)));
    assert_eq!(
        actual.get("edits").unwrap(),
        Value::Int(expected.get_edits())
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn search_edit_reuses_the_rendered_live_binding() {
    use std::{cell::Cell, rc::Rc};

    let (mut actual, mut expected) = search_input_fixture();
    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    actual
        .on("live-search", {
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                let count = reads.get();
                reads.set(count + 1);
                Ok(Some((count == 0).into()))
            })
        })
        .unwrap();
    expected.on_live_search({
        let reads = expected_reads.clone();
        move || {
            let count = reads.get();
            reads.set(count + 1);
            count == 0
        }
    });
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    assert!(actual.edit_at(&actual_scene, &[0], "b".into()));
    assert!(expected.edit_at(&expected_scene, &[0], "b".into()));
    assert_eq!(actual_reads.get(), 1);
    assert_eq!(expected_reads.get(), 1);
    assert_eq!(
        actual.get("events"),
        Ok(Value::String("edit:b;search:b;".into()))
    );
    assert_eq!(expected.get_events(), "edit:b;search:b;");
    assert_eq!(actual.take_error(), None);
}

fn search_input_fixture() -> (Instance, expressions_ui::SearchInputParity) {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("SearchInputParity")
        .unwrap();
    (actual, expressions_ui::SearchInputParity::new())
}

#[test]
fn search_actions_recheck_query_suggestion_and_live_flags() {
    use xrack_runtime::ControlAction::{ClearSearch, Select, SubmitSearch};

    let (mut actual, mut expected) = search_input_fixture();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let path = [0];

    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    for action in [Select(0), ClearSearch, SubmitSearch] {
        assert!(!actual.control_at(&actual_scene, &path, action));
        assert!(!expected.control_at(&expected_scene, &path, action));
    }
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    actual.set("locked", true.into()).unwrap();
    expected.set_locked(true);
    for action in [Select(0), ClearSearch] {
        assert!(!actual.control_at(&actual_scene, &path, action));
        assert!(!expected.control_at(&expected_scene, &path, action));
    }
    actual.set("locked", false.into()).unwrap();
    expected.set_locked(false);

    actual.set("query", Value::String("other".into())).unwrap();
    expected.set_query("other".into());
    for action in [Select(0), ClearSearch, SubmitSearch] {
        assert!(!actual.control_at(&actual_scene, &path, action));
        assert!(!expected.control_at(&expected_scene, &path, action));
    }
    actual.set("query", Value::String("a".into())).unwrap();
    expected.set_query("a".into());
    actual
        .set("result-query", Value::String("old".into()))
        .unwrap();
    expected.set_result_query("old".into());
    assert!(!actual.control_at(&actual_scene, &path, Select(0)));
    assert!(!expected.control_at(&expected_scene, &path, Select(0)));
    actual
        .set("result-query", Value::String("a".into()))
        .unwrap();
    expected.set_result_query("a".into());
    assert!(
        actual
            .string_model("suggestions")
            .unwrap()
            .set_row(0, "alpine".into())
    );
    assert!(expected.model_suggestions().set_row(0, "alpine".into()));
    assert!(!actual.control_at(&actual_scene, &path, Select(0)));
    assert!(!expected.control_at(&expected_scene, &path, Select(0)));

    assert!(actual.control_at(&actual.render_shared(), &path, Select(0)));
    assert!(expected.control_at(&expected.render_shared(), &path, Select(0)));
    actual.set("locked", true.into()).unwrap();
    expected.set_locked(true);
    assert!(actual.control_at(&actual.render_shared(), &path, SubmitSearch));
    assert!(expected.control_at(&expected.render_shared(), &path, SubmitSearch));
    actual.set("locked", false.into()).unwrap();
    expected.set_locked(false);
    assert!(actual.control_at(&actual.render_shared(), &path, ClearSearch));
    assert!(expected.control_at(&expected.render_shared(), &path, ClearSearch));
    assert!(actual.control_at(&actual.render_shared(), &path, SubmitSearch));
    assert!(expected.control_at(&expected.render_shared(), &path, SubmitSearch));
    let events = "edit:alpine;select:alpine;search:alpine;accept:alpine;search:alpine;edit:;search:;accept:;search:;";
    assert_eq!(actual.get("events"), Ok(Value::String(events.into())));
    assert_eq!(expected.get_events(), events);
    assert_eq!(actual.get("query"), Ok(Value::String(String::new())));
    assert!(expected.get_query().is_empty());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn live_search_dispatches_edits_and_suggestions_once_in_both_modes() {
    use async_ui::AsyncDemo;
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/async_demo/async.xr"),
        "async.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("AsyncDemo")
        .unwrap();
    let mut expected = AsyncDemo::new();
    let actual_events = Rc::new(RefCell::new(Vec::new()));
    let expected_events = Rc::new(RefCell::new(Vec::new()));
    let events = actual_events.clone();
    actual
        .on(
            "search",
            Rc::new(move |args| {
                let [Value::String(query)] = args else {
                    return Err("expected search text".into());
                };
                events.borrow_mut().push(query.clone());
                Ok(None)
            }),
        )
        .unwrap();
    let events = expected_events.clone();
    expected.on_search(move |query| events.borrow_mut().push(query));
    let field = [0, 1, 0];
    assert!(actual.edit_at(&actual.render(), &field, "cloud".into()));
    assert!(expected.edit_at(&expected.render(), &field, "cloud".into()));
    assert!(!actual.edit_at(&actual.render(), &field, "cloud".into()));
    actual
        .set("suggestions", Value::StringList(vec!["cloud files".into()]))
        .unwrap();
    expected.set_suggestions(vec!["cloud files".into()]);
    assert!(!actual.control_at(&actual.render(), &field, ControlAction::Select(0)));
    actual
        .set("result-query", Value::String("cloud".into()))
        .unwrap();
    expected.set_result_query("cloud".into());
    assert!(actual.control_at(&actual.render(), &field, ControlAction::Select(0)));
    assert!(expected.control_at(&expected.render(), &field, ControlAction::Select(0)));
    assert!(actual.control_at(&actual.render(), &field, ControlAction::ClearSearch));
    assert!(expected.control_at(&expected.render(), &field, ControlAction::ClearSearch));
    assert_eq!(*actual_events.borrow(), ["cloud", "cloud files", ""]);
    assert_eq!(*actual_events.borrow(), *expected_events.borrow());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn wheel_batches_choose_the_same_numeric_target_and_fall_through_at_bounds() {
    use controls_ui::WheelControlsDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::{Interaction, Point, Size, WheelAdjustment};
    use xrack_runtime::{Length, WheelEvent};

    let mut actual = interpret_controls_fixture("WheelControlsDemo");
    let mut expected = WheelControlsDemo::new();
    let mut actual_input = Interaction::default();
    let mut expected_input = Interaction::default();
    let now = Instant::now();

    fn wheel(
        ui: &mut impl Component,
        input: &mut Interaction,
        path: &[usize],
        delta: f64,
        now: Instant,
    ) -> Option<WheelAdjustment> {
        let (scene, layout) = prepare_list(ui, input, Size::new(460.0, 310.0));
        let bounds = layout.at_path(path).unwrap().bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bounds.x + bounds.width * 0.8,
                y: bounds.y + bounds.height * 0.5,
            },
        );
        let event = WheelEvent {
            delta_y: Length(delta),
            ..Default::default()
        };
        let adjustment = input.wheel_adjustment(&scene, &layout, event, now);
        match &adjustment {
            Some(WheelAdjustment::Range(input)) => {
                assert!(ui.adjust_range_at(&scene, &input.path, input.action))
            }
            Some(WheelAdjustment::Spin(input)) => {
                assert!(ui.control_at(&scene, &input.path, input.action))
            }
            _ => {}
        }
        adjustment
    }

    for (step, (path, delta)) in [
        (&[1, 0, 1][..], -14.0),
        (&[1, 0, 1], -14.0),
        (&[1, 0, 1], -28.0),
        (&[1, 0, 3], 28.0),
        (&[1, 0, 4], -84.0),
        (&[1, 0, 5], -56.0),
        (&[1, 0, 6], -56.0),
    ]
    .into_iter()
    .enumerate()
    {
        let now = now + Duration::from_millis(10 * step as u64);
        let outcome = wheel(&mut actual, &mut actual_input, path, delta, now);
        assert_eq!(
            outcome,
            wheel(&mut expected, &mut expected_input, path, delta, now)
        );
        if step == 2 {
            assert!(outcome.is_none());
        }
    }
    for (name, value) in [
        ("slider-value", Value::Float(5.0)),
        ("interval-end", Value::Float(7.0)),
        ("dial-value", Value::Float(1.0)),
        ("spin-value", Value::Int(14)),
        ("decimal-value", Value::Float(0.0)),
        ("releases", Value::Int(0)),
        ("edits", Value::Int(2)),
    ] {
        assert_eq!(actual.get(name).unwrap(), value);
    }
    assert_same_scene(&actual.render(), &expected.render());
}

#[test]
fn single_line_editor_validation_and_untyped_callbacks_match_compiled_mode() {
    use form_ui::{PasswordDemo, ValidationDemo};

    let mut actual = interpret_form_fixture("ValidationDemo");
    let mut expected = ValidationDemo::new();
    for (path, value, accepted) in [
        ([1], "12x", false),
        ([1], "12345", false),
        ([1], "0123", true),
        ([2], "1.25", false),
        ([2], "1,25", true),
        ([2], "1,25", false),
    ] {
        assert_eq!(
            actual.edit_at(&actual.render(), &path, value.into()),
            accepted
        );
        assert_eq!(
            expected.edit_at(&expected.render(), &path, value.into()),
            accepted
        );
        assert_same_scene(&actual.render(), &expected.render());
    }
    assert_eq!(actual.get("amount-edits").unwrap(), Value::Int(1));
    assert!(actual.dispatch_at(&actual.render(), &[2], "accepted"));
    assert!(expected.dispatch_at(&expected.render(), &[2], "accepted"));
    assert_eq!(
        actual.get("submitted-amount").unwrap(),
        Value::String("1,25".into())
    );
    assert!(actual.edit_at(&actual.render(), &[2], "-,".into()));
    assert!(expected.edit_at(&expected.render(), &[2], "-,".into()));
    assert!(!actual.dispatch_at(&actual.render(), &[2], "accepted"));
    assert!(!expected.dispatch_at(&expected.render(), &[2], "accepted"));

    let mut actual = interpret_form_fixture("PasswordDemo");
    let mut expected = PasswordDemo::new();
    assert!(actual.edit_at(&actual.render(), &[1], "秘密".into()));
    assert!(expected.edit_at(&expected.render(), &[1], "秘密".into()));
    assert!(actual.render().at_path(&[1]).unwrap().password_masked());
    assert!(actual.dispatch_at(&actual.render(), &[2], "clicked"));
    assert!(expected.dispatch_at(&expected.render(), &[2], "clicked"));
    assert!(!actual.render().at_path(&[1]).unwrap().password_masked());
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn calendar_and_date_popup_match_across_bounds_and_draft_sessions() {
    use controls_ui::CalendarDemo;
    use xrack_runtime::{CalendarAction, ControlAction, Date, DialogRole};

    let mut actual = interpret_controls_fixture("CalendarDemo");
    let mut expected = CalendarDemo::new();
    let date = |month, day| Date::new(2024, month, day).unwrap();
    assert_same_scene(&actual.render(), &expected.render());
    for action in [
        CalendarAction::Select(date(2, 3)),
        CalendarAction::Select(date(2, 29)),
        CalendarAction::Browse(date(3, 1)),
    ] {
        let action = ControlAction::Calendar(action);
        assert_eq!(
            actual.control_at(&actual.render(), &[1], action),
            expected.control_at(&expected.render(), &[1], action),
        );
        assert_same_scene(&actual.render(), &expected.render());
    }
    assert_eq!(actual.get("selected").unwrap(), Value::Date(date(2, 29)));
    assert_eq!(actual.get("month").unwrap(), Value::Date(date(3, 1)));

    assert!(actual.dispatch_at(&actual.render(), &[2], "clicked"));
    assert!(expected.dispatch_at(&expected.render(), &[2], "clicked"));
    let original = actual.render();
    let expected_original = expected.render();
    let draft = ControlAction::Calendar(CalendarAction::Select(date(4, 10)));
    assert!(actual.control_at(&original, &[3, 0], draft));
    assert!(expected.control_at(&expected_original, &[3, 0], draft));
    assert_eq!(actual.get("selected").unwrap(), Value::Date(date(2, 29)));
    assert_same_scene(&actual.render(), &expected.render());

    let old_draft = actual.render();
    let expected_old_draft = expected.render();
    actual.set("latest", Value::Date(date(4, 5))).unwrap();
    expected.set_latest(date(4, 5));
    assert!(!actual.control_at(&old_draft, &[3, 0], draft));
    assert!(!expected.control_at(&expected_old_draft, &[3, 0], draft));
    let accept = ControlAction::Dialog(DialogRole::Accept);
    // Confirmation uses the current bounds even if the draft was shown before they changed.
    assert!(actual.control_at(&old_draft, &[3], accept));
    assert!(expected.control_at(&expected_old_draft, &[3], accept));
    assert_eq!(expected.get_selected(), date(4, 5));
    assert_eq!(expected.get_accepts(), 1);
    assert_eq!(actual.get("selected").unwrap(), Value::Date(date(4, 5)));
    assert_eq!(actual.get("accepts").unwrap(), Value::Int(1));

    actual.set("picker-open", true.into()).unwrap();
    expected.set_picker_open(true);
    assert_same_scene(&actual.render(), &expected.render());
    assert!(!actual.control_at(&original, &[3], accept));
    assert!(!expected.control_at(&expected_original, &[3], accept));
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    let before_bounds_change = actual.render_shared();
    let expected_before_bounds_change = expected.render_shared();
    actual.set("latest", date(4, 4).into()).unwrap();
    expected.set_latest(date(4, 4));
    assert!(actual.control_at(&before_bounds_change, &[3], cancel));
    assert!(expected.control_at(&expected_before_bounds_change, &[3], cancel));
    assert_eq!(actual.get("cancels").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn calendar_rejects_a_selection_from_before_its_bounds_or_date_changed() {
    use controls_ui::CalendarDemo;
    use xrack_runtime::{CalendarAction, ControlAction, Date};

    let mut interpreted = interpret_controls_fixture("CalendarDemo");
    let mut generated = CalendarDemo::new();
    let calendar = [1];
    let date = Date::new(2024, 4, 10).unwrap();
    let select = ControlAction::Calendar(CalendarAction::Select(date));
    let initial = interpreted.render_shared();
    let compiled_initial = generated.render_shared();

    let limit = Date::new(2024, 3, 1).unwrap();
    interpreted.set("latest", limit.into()).unwrap();
    generated.set_latest(limit);
    let actual = interpreted.control_at(&initial, &calendar, select);
    let compiled = generated.control_at(&compiled_initial, &calendar, select);
    assert_eq!([actual, compiled], [false, false]);
    let original = Date::new(2024, 2, 28).unwrap();
    assert_eq!(interpreted.get("selected"), Ok(original.into()));
    assert_eq!(generated.get_selected(), original);

    interpreted.set("latest", Date::MAX.into()).unwrap();
    generated.set_latest(Date::MAX);
    for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
        let scene = ui.render_shared();
        assert!(ui.control_at(&scene, &calendar, select));
        assert!(!ui.control_at(&scene, &calendar, select));
    }
    assert_eq!(interpreted.get("selected"), Ok(date.into()));
    assert_eq!(generated.get_selected(), date);
    assert_eq!(interpreted.get("date-changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_date_changes(), 1);
    assert_eq!(interpreted.get("month-changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_month_changes(), 1);
    assert_same_scene(&interpreted.render(), &generated.render());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn nested_month_grid_checks_live_input_without_rendering_its_siblings() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{CalendarAction, ControlAction, Date};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("NestedMonthGridParity")
        .unwrap();
    let mut generated = expressions_ui::NestedMonthGridParity::new();
    let actual_calls = Rc::new(Cell::new(0));
    let expected_calls = Rc::new(Cell::new(0));
    interpreted
        .on("sibling", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(revision)] = args else {
                    unreachable!()
                };
                calls.set(calls.get() + 1);
                Ok(Some(Value::String(revision.to_string())))
            })
        })
        .unwrap();
    generated.on_sibling({
        let calls = expected_calls.clone();
        move |revision| {
            calls.set(calls.get() + 1);
            revision.to_string()
        }
    });

    let actual = interpreted.render_shared();
    let expected = generated.render_shared();
    let path = [0, 0];
    interpreted.set("revision", 1_i64.into()).unwrap();
    generated.set_revision(1);
    let date = Date::new(2026, 3, 17).unwrap();
    let select = ControlAction::Calendar(CalendarAction::Select(date));
    assert!(interpreted.control_at(&actual, &path, select));
    assert!(generated.control_at(&expected, &path, select));
    assert_eq!(
        interpreted.get("events"),
        Ok(Value::String("D17@3;M3@17;".into()))
    );
    assert_eq!(generated.get_events(), "D17@3;M3@17;");
    assert_eq!((actual_calls.get(), expected_calls.get()), (1, 1));

    let actual = interpreted.render_shared();
    let expected = generated.render_shared();
    assert_eq!((actual_calls.get(), expected_calls.get()), (2, 2));
    let activate = ControlAction::Calendar(CalendarAction::Activate(date));
    assert!(interpreted.control_at(&actual, &path, activate));
    assert!(generated.control_at(&expected, &path, activate));
    assert_eq!(
        interpreted.get("events"),
        Ok(Value::String("D17@3;M3@17;A17;".into()))
    );
    assert_eq!(generated.get_events(), "D17@3;M3@17;A17;");

    interpreted.set("week-start", 7_i64.into()).unwrap();
    generated.set_week_start(7);
    assert!(!interpreted.control_at(&actual, &path, activate));
    assert!(!generated.control_at(&expected, &path, activate));
    assert_eq!((actual_calls.get(), expected_calls.get()), (2, 2));

    interpreted.set("week-start", 1_i64.into()).unwrap();
    generated.set_week_start(1);
    let april = Date::new(2026, 4, 1).unwrap();
    interpreted.set("month", april.into()).unwrap();
    generated.set_month(april);
    assert!(!interpreted.control_at(&actual, &path, activate));
    assert!(!generated.control_at(&expected, &path, activate));
    assert_same_scene(&interpreted.render(), &generated.render());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn month_grid_retains_visible_adjacent_dates_and_renders_day_cell_bindings() {
    use controls_ui::{CalendarAgendaDemo, MonthGridDemo};
    use xrack_runtime::{CalendarAction, CalendarState, ControlAction, Date};

    let mut actual = interpret_controls_fixture("MonthGridDemo");
    let mut expected = MonthGridDemo::new();
    let path = [6];
    let adjacent = Date::new(2020, 12, 28).unwrap();
    let select = ControlAction::Calendar(CalendarAction::Select(adjacent));
    assert!(actual.control_at(&actual.render(), &path, select));
    assert!(expected.control_at(&expected.render(), &path, select));
    assert_eq!(
        actual.get("month").unwrap(),
        Value::Date(Date::new(2021, 1, 1).unwrap())
    );
    let stale = actual.render();
    let expected_stale = expected.render();
    let activate = ControlAction::Calendar(CalendarAction::Activate(adjacent));
    assert!(actual.control_at(&stale, &path, activate));
    assert!(expected.control_at(&expected_stale, &path, activate));
    actual.set("sunday", false.into()).unwrap();
    expected.set_sunday(false);
    assert!(!actual.control_at(&stale, &path, activate));
    assert!(!expected.control_at(&expected_stale, &path, activate));
    let browse = ControlAction::Calendar(CalendarAction::Browse(Date::new(2021, 3, 1).unwrap()));
    assert!(actual.control_at(&actual.render(), &path, browse));
    assert!(expected.control_at(&expected.render(), &path, browse));
    assert_eq!(actual.get("activations").unwrap(), Value::Int(1));
    assert_eq!(actual.get("browses").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());

    let mut actual = interpret_controls_fixture("CalendarAgendaDemo");
    let mut expected = CalendarAgendaDemo::new();
    let review = Date::new(2026, 11, 12).unwrap();
    let review_description = |ui: &dyn Component| {
        let scene = ui.render();
        let grid = scene.at_path(&path).unwrap();
        let index = CalendarState::from_node(grid)
            .cells(1)
            .iter()
            .position(|date| *date == Some(review))
            .unwrap();
        let cells = grid.calendar_cells().unwrap();
        assert_eq!(cells.len(), 42);
        cells[index]
            .text("accessible-description")
            .unwrap()
            .to_owned()
    };
    assert_eq!(review_description(&actual), "评审");
    actual
        .set("review-label", Value::String("更新的评审".into()))
        .unwrap();
    expected.set_review_label("更新的评审".into());
    let action = ControlAction::Calendar(CalendarAction::Activate(review));
    assert!(actual.control_at(&actual.render(), &path, action));
    assert!(expected.control_at(&expected.render(), &path, action));
    assert_eq!(review_description(&actual), "更新的评审");
    assert_eq!(actual.get("selected").unwrap(), Value::Date(review));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn time_picker_wheels_and_popup_commit_preserve_hidden_seconds() {
    use controls_ui::TimeDemo;
    use xrack_runtime::{ControlAction, DialogRole, Time, Tumbler};

    let mut actual = interpret_controls_fixture("TimeDemo");
    let mut expected = TimeDemo::new();
    let pick = |ui: &mut dyn Component, path: &[usize], index| {
        let scene = ui.render();
        let action = Tumbler::from_node(scene.at_path(path).unwrap())
            .select(index)
            .unwrap();
        ui.control_at(&scene, path, action)
    };
    assert_same_scene(&actual.render(), &expected.render());
    assert!(pick(&mut actual, &[1, 0], 0));
    assert!(pick(&mut expected, &[1, 0], 0));
    let stale = actual.render();
    let expected_stale = expected.render();
    actual.set("use-24", false.into()).unwrap();
    expected.set_use_24(false);
    let action = Tumbler::from_node(stale.at_path(&[1, 0]).unwrap())
        .select(1)
        .unwrap();
    assert!(!actual.control_at(&stale, &[1, 0], action));
    assert!(!expected.control_at(&expected_stale, &[1, 0], action));
    assert!(pick(&mut actual, &[1, 3], 1));
    assert!(pick(&mut expected, &[1, 3], 1));
    let noon = Time::new(12, 58, 41).unwrap();
    assert_eq!(actual.get("selected").unwrap(), Value::Time(noon));

    for role in [DialogRole::Reject, DialogRole::Accept] {
        actual.set("picker-open", true.into()).unwrap();
        expected.set_picker_open(true);
        assert!(pick(&mut actual, &[5, 0, 1], 7));
        assert!(pick(&mut expected, &[5, 0, 1], 7));
        assert_eq!(actual.get("selected").unwrap(), Value::Time(noon));
        assert_same_scene(&actual.render(), &expected.render());
        let action = ControlAction::Dialog(role);
        assert!(actual.control_at(&actual.render(), &[5], action));
        assert!(expected.control_at(&expected.render(), &[5], action));
    }
    assert_eq!(
        actual.get("selected").unwrap(),
        Value::Time(Time::new(12, 7, 41).unwrap())
    );
    assert_eq!(actual.get("accepted-minute").unwrap(), Value::Int(7));
    assert_eq!(actual.get("changes").unwrap(), Value::Int(2));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn time_picker_popup_accept_requires_the_visible_draft_but_cancel_only_requires_the_session() {
    use controls_ui::TimeDemo;
    use xrack_runtime::{ControlAction, DialogRole, Time, Tumbler};

    let mut interpreted = interpret_controls_fixture("TimeDemo");
    let mut generated = TimeDemo::new();
    let dialog = [5];
    let minute = [5, 0, 1];
    let hour = [5, 0, 0];
    let accept = ControlAction::Dialog(DialogRole::Accept);
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    let select = |scene: &Node, path: &[usize], index| {
        Tumbler::from_node(scene.at_path(path).unwrap())
            .select(index)
            .unwrap()
    };
    for role in [DialogRole::Reject, DialogRole::Accept] {
        interpreted.set("picker-open", true.into()).unwrap();
        generated.set_picker_open(true);
        for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
            let old = ui.render_shared();
            assert!(ui.control_at(&old, &minute, select(&old, &minute, 7)));
            assert!(!ui.control_at(&old, &dialog, accept));

            // The minute edit does not invalidate the unchanged hour wheel.
            assert!(ui.control_at(&old, &hour, select(&old, &hour, 22)));
            if role == DialogRole::Reject {
                assert!(ui.control_at(&old, &dialog, cancel));
            } else {
                assert!(ui.control_at(&ui.render_shared(), &dialog, accept));
            }
        }
        let expected = if role == DialogRole::Reject {
            Time::new(23, 58, 41).unwrap()
        } else {
            Time::new(22, 7, 41).unwrap()
        };
        assert_eq!(interpreted.get("selected"), Ok(expected.into()));
        assert_eq!(generated.get_selected(), expected);
        assert_same_scene(&interpreted.render(), &generated.render());
    }
    assert_eq!(interpreted.get("cancels"), Ok(1_i64.into()));
    assert_eq!(generated.get_cancels(), 1);
    assert_eq!(interpreted.get("accepts"), Ok(1_i64.into()));
    assert_eq!(generated.get_accepts(), 1);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn time_picker_popup_rejects_accept_after_its_display_options_change() {
    use controls_ui::TimeDemo;
    use xrack_runtime::{ControlAction, DialogRole, Time, Tumbler};

    let mut interpreted = interpret_controls_fixture("TimeDemo");
    let mut generated = TimeDemo::new();
    let dialog = [5];
    let minute = [5, 0, 1];
    let accept = ControlAction::Dialog(DialogRole::Accept);
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    interpreted.set("picker-open", true.into()).unwrap();
    generated.set_picker_open(true);

    for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
        let scene = ui.render_shared();
        let action = Tumbler::from_node(scene.at_path(&minute).unwrap())
            .select(7)
            .unwrap();
        assert!(ui.control_at(&scene, &minute, action));
    }

    let before_format = interpreted.render_shared();
    let compiled_before_format = generated.render_shared();
    interpreted.set("use-24", false.into()).unwrap();
    generated.set_use_24(false);
    assert_eq!(
        [
            interpreted.control_at(&before_format, &dialog, accept),
            generated.control_at(&compiled_before_format, &dialog, accept),
        ],
        [false, false]
    );

    let before_seconds = interpreted.render_shared();
    let compiled_before_seconds = generated.render_shared();
    interpreted.set("seconds", true.into()).unwrap();
    generated.set_seconds(true);
    assert!(!interpreted.control_at(&before_seconds, &dialog, accept));
    assert!(!generated.control_at(&compiled_before_seconds, &dialog, accept));
    assert_eq!(interpreted.get("picker-open"), Ok(true.into()));
    assert!(generated.get_picker_open());
    assert_eq!(interpreted.get("accepts"), Ok(0_i64.into()));
    assert_eq!(generated.get_accepts(), 0);

    assert!(interpreted.control_at(&interpreted.render_shared(), &dialog, accept));
    assert!(generated.control_at(&generated.render_shared(), &dialog, accept));
    let selected = Time::new(23, 7, 41).unwrap();
    assert_eq!(interpreted.get("selected"), Ok(selected.into()));
    assert_eq!(generated.get_selected(), selected);
    assert_eq!(interpreted.get("accepts"), Ok(1_i64.into()));
    assert_eq!(generated.get_accepts(), 1);

    interpreted.set("picker-open", true.into()).unwrap();
    generated.set_picker_open(true);
    let old = interpreted.render_shared();
    let compiled_old = generated.render_shared();
    interpreted.set("seconds", false.into()).unwrap();
    generated.set_seconds(false);
    assert!(interpreted.control_at(&old, &dialog, cancel));
    assert!(generated.control_at(&compiled_old, &dialog, cancel));
    assert_eq!(interpreted.get("cancels"), Ok(1_i64.into()));
    assert_eq!(generated.get_cancels(), 1);
    assert_same_scene(&interpreted.render(), &generated.render());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn tumbler_rejects_replaced_models_and_preserves_wrapping() {
    use controls_ui::TumblerDemo;
    use xrack_runtime::Tumbler;

    let mut actual = interpret_controls_fixture("TumblerDemo");
    let mut expected = TumblerDemo::new();
    let path = [1];
    let stale = actual.render();
    let expected_stale = expected.render();
    let old_wheel = Tumbler::from_node(stale.at_path(&path).unwrap());
    let expected_old_wheel = Tumbler::from_node(expected_stale.at_path(&path).unwrap());
    let action = old_wheel.select(1).unwrap();
    let expected_action = expected_old_wheel.select(1).unwrap();
    let hours = vec!["zero".into(), "one".into(), "two".into()];
    actual
        .set("hours", Value::StringList(hours.clone()))
        .unwrap();
    expected.set_hours(hours);
    assert!(!actual.control_at(&stale, &path, action));
    assert!(!expected.control_at(&expected_stale, &path, expected_action));
    for ui in [&mut actual as &mut dyn Component, &mut expected] {
        let scene = ui.render();
        let wheel = Tumbler::from_node(scene.at_path(&path).unwrap());
        assert_eq!(wheel.selected(), Some(2));
        assert!(wheel.wraps());
        let next = wheel.step(1).and_then(|index| wheel.select(index)).unwrap();
        assert!(ui.control_at(&scene, &path, next));
        assert!(!ui.control_at(&scene, &path, next));
    }
    assert_eq!(actual.get("hour").unwrap(), Value::Int(0));
    assert_eq!(actual.get("changes").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tumbler_checks_live_count_and_availability_before_selecting() {
    use expressions_ui::TumblerSelectionParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::Tumbler;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TumblerSelectionParity")
        .unwrap();
    let mut expected = TumblerSelectionParity::new();
    let path = [0];
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let action = Tumbler::from_node(actual_scene.at_path(&path).unwrap())
        .select(4)
        .unwrap();

    actual.set("limit", 3_i64.into()).unwrap();
    expected.set_limit(3);
    assert!(!actual.control_at(&actual_scene, &path, action));
    assert!(!expected.control_at(&expected_scene, &path, action));
    actual.set("limit", 5_i64.into()).unwrap();
    expected.set_limit(5);

    actual
        .on("read-count", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("allowed", false.into()).unwrap();
                }
                Ok(Some(5_i64.into()))
            })
        })
        .unwrap();
    expected.on_read_count({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_allowed(false);
            5
        }
    });
    assert!(!actual.control_at(&actual_scene, &path, action));
    assert!(!expected.control_at(&expected_scene, &path, action));
    assert_eq!(actual.get("allowed"), Ok(false.into()));
    assert!(!expected.get_allowed());
    assert_eq!(actual.get("selected"), Ok(0_i64.into()));
    assert_eq!(expected.get_selected(), 0);
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(expected.get_changes(), 0);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn color_picker_reuses_hex_validation_and_hsv_and_channel_drafts() {
    use controls_ui::ColorDemo;
    use xrack_runtime::{Color, ControlAction, DialogRole, RangeAction, SpinAction};

    let mut actual = interpret_controls_fixture("ColorDemo");
    let mut expected = ColorDemo::new();
    let original = Value::Color(Color(0x4080c080));
    let dialog = [5];
    let hex = [5, 0, 0, 1];
    let accept = ControlAction::Dialog(DialogRole::Accept);
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    actual.set("picker-open", true.into()).unwrap();
    expected.set_picker_open(true);
    let stale = actual.render();
    let expected_stale = expected.render();
    assert_same_scene(&stale, &expected_stale);
    assert!(actual.edit_at(&stale, &hex, "#12".into()));
    assert!(expected.edit_at(&expected_stale, &hex, "#12".into()));
    assert!(!actual.edit_at(&stale, &hex, "#010203FF".into()));
    assert!(!expected.edit_at(&expected_stale, &hex, "#010203FF".into()));
    assert_same_scene(&actual.render(), &expected.render());
    assert!(!actual.control_at(&actual.render(), &dialog, accept));
    assert!(!expected.control_at(&expected.render(), &dialog, accept));
    assert!(actual.control_at(&actual.render(), &dialog, cancel));
    assert!(expected.control_at(&expected.render(), &dialog, cancel));
    assert_eq!(actual.get("selected").unwrap(), original);

    actual.set("alpha-visible", false.into()).unwrap();
    actual.set("plane-visible", true.into()).unwrap();
    actual.set("picker-open", true.into()).unwrap();
    expected.set_alpha_visible(false);
    expected.set_plane_visible(true);
    expected.set_picker_open(true);
    assert!(!actual.control_at(&stale, &dialog, accept));
    assert!(!expected.control_at(&expected_stale, &dialog, accept));
    for (path, action) in [
        ([5, 0, 5, 0], RangeAction::SetPoint { x: 0.75, y: 0.6 }),
        ([5, 0, 5, 1], RangeAction::SetValue(120.0)),
        ([5, 0, 1, 1], RangeAction::SetValue(32.0)),
    ] {
        assert!(actual.adjust_range_at(&actual.render(), &path, action));
        assert!(expected.adjust_range_at(&expected.render(), &path, action));
        assert_same_scene(&actual.render(), &expected.render());
    }
    let spin = ControlAction::Spin(SpinAction::Step(2));
    assert!(actual.control_at(&actual.render(), &[5, 0, 3, 2], spin));
    assert!(expected.control_at(&expected.render(), &[5, 0, 3, 2], spin));
    assert_eq!(actual.get("selected").unwrap(), original);
    assert!(actual.control_at(&actual.render(), &dialog, accept));
    assert!(expected.control_at(&expected.render(), &dialog, accept));
    let selected = expected.get_selected();
    assert_eq!(selected.0 & 0xff, 0x80);
    assert_eq!(actual.get("selected").unwrap(), Value::Color(selected));
    assert_eq!(
        actual.get("accepted-color").unwrap(),
        Value::Color(selected)
    );
    assert_eq!(actual.get("cancels").unwrap(), Value::Int(1));

    actual.set("alpha-visible", true.into()).unwrap();
    actual.set("picker-open", true.into()).unwrap();
    expected.set_alpha_visible(true);
    expected.set_picker_open(true);
    assert!(actual.edit_at(&actual.render(), &hex, "#12AB349A".into()));
    assert!(expected.edit_at(&expected.render(), &hex, "#12AB349A".into()));
    assert!(actual.control_at(&actual.render(), &dialog, accept));
    assert!(expected.control_at(&expected.render(), &dialog, accept));
    assert_eq!(
        actual.get("accepted-color").unwrap(),
        Value::Color(Color(0x12ab349a))
    );
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn color_picker_rejects_accept_from_before_the_hex_format_changed() {
    use controls_ui::ColorDemo;
    use xrack_runtime::{Color, ControlAction, DialogRole};

    let mut interpreted = interpret_controls_fixture("ColorDemo");
    let mut generated = ColorDemo::new();
    let dialog = [5];
    let hex = [5, 0, 0, 1];
    let accept = ControlAction::Dialog(DialogRole::Accept);

    interpreted.set("picker-open", true.into()).unwrap();
    generated.set_picker_open(true);
    assert!(interpreted.edit_at(&interpreted.render_shared(), &hex, "#12".into()));
    assert!(generated.edit_at(&generated.render_shared(), &hex, "#12".into()));
    let stale = interpreted.render_shared();
    let generated_stale = generated.render_shared();

    interpreted.set("alpha-visible", false.into()).unwrap();
    generated.set_alpha_visible(false);
    let actual = interpreted.control_at(&stale, &dialog, accept);
    let compiled = generated.control_at(&generated_stale, &dialog, accept);
    assert_eq!([actual, compiled], [false, false]);
    assert_eq!(interpreted.get("picker-open"), Ok(true.into()));
    assert!(generated.get_picker_open());
    assert_eq!(interpreted.get("accepts"), Ok(0_i64.into()));
    assert_eq!(generated.get_accepts(), 0);

    let fresh = interpreted.render_shared();
    let generated_fresh = generated.render_shared();
    assert_same_scene(&fresh, &generated_fresh);
    assert!(interpreted.control_at(&fresh, &dialog, accept));
    assert!(generated.control_at(&generated_fresh, &dialog, accept));
    let original = Color(0x4080c080);
    assert_eq!(interpreted.get("accepted-color"), Ok(original.into()));
    assert_eq!(generated.get_accepted_color(), original);
    assert_eq!(interpreted.get("accepts"), Ok(1_i64.into()));
    assert_eq!(generated.get_accepts(), 1);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn font_picker_uses_layout_catalog_and_commits_the_complete_font_value() {
    use controls_ui::FontPickerDemo;
    use xrack_layout::{Interaction, Size, TextMeasurer};
    use xrack_runtime::{ControlAction, DialogRole, Font, Length, SpinAction, SpinDecimal};

    struct Catalog([String; 2]);
    impl TextMeasurer for Catalog {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            FixedText.measure(text, size, width)
        }
        fn font_families(&self) -> &[String] {
            &self.0
        }
    }
    fn prepare(ui: &impl Component) -> std::rc::Rc<Node> {
        xrack_layout::prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(510.0, 430.0),
            &mut Catalog(["Catalog Sans".into(), "Catalog Serif".into()]),
            &Interaction::default(),
        )
        .0
    }

    let mut actual = interpret_controls_fixture("FontPickerDemo");
    let mut expected = FontPickerDemo::new();
    let original = Value::Font(expected.get_selected());
    let popup = [3];
    let family = [3, 0, 0, 1];
    let accept = ControlAction::Dialog(DialogRole::Accept);
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    assert_same_scene(&actual.render(), &expected.render());
    actual.set("picker-open", true.into()).unwrap();
    expected.set_picker_open(true);
    let stale = prepare(&actual);
    let expected_stale = prepare(&expected);
    assert_same_scene(&stale, &expected_stale);
    assert_eq!(
        stale.at_path(&family).unwrap().strings("model"),
        ["sans-serif", "Catalog Sans", "Catalog Serif"]
    );

    assert!(actual.control_at(&stale, &family, ControlAction::Select(2)));
    assert!(expected.control_at(&expected_stale, &family, ControlAction::Select(2)));
    assert!(actual.control_at(&actual.render(), &popup, cancel));
    assert!(expected.control_at(&expected.render(), &popup, cancel));
    assert_eq!(actual.get("selected").unwrap(), original);
    actual.set("picker-open", true.into()).unwrap();
    expected.set_picker_open(true);
    assert_same_scene(&prepare(&actual), &prepare(&expected));
    assert!(!actual.control_at(&stale, &popup, accept));
    assert!(!expected.control_at(&expected_stale, &popup, accept));

    for (path, action) in [
        (family, ControlAction::Select(2)),
        (
            [3, 0, 1, 1],
            ControlAction::Spin(SpinAction::CommitDecimal(SpinDecimal::new(31.5).unwrap())),
        ),
        ([3, 0, 2, 1], ControlAction::Spin(SpinAction::Commit(750))),
        ([3, 0, 3, 1], ControlAction::Select(1)),
    ] {
        assert!(actual.control_at(&prepare(&actual), &path, action));
        assert!(expected.control_at(&prepare(&expected), &path, action));
        assert_same_scene(&prepare(&actual), &prepare(&expected));
    }
    assert_eq!(actual.get("selected").unwrap(), original);
    assert!(actual.control_at(&actual.render(), &popup, accept));
    assert!(expected.control_at(&expected.render(), &popup, accept));
    assert_eq!(
        actual.get("accepted-font").unwrap(),
        Value::Font(Font {
            family: "Catalog Serif".into(),
            size: Length(31.5),
            weight: 750,
            italic: true,
        })
    );
    assert_eq!(actual.get("cancels").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn theme_updates_window_palette_without_changing_editor_state() {
    use controls_ui::ThemeDemo;
    use xrack_runtime::{Color, ColorScheme};

    let mut actual = interpret_controls_fixture("ThemeDemo");
    let mut expected = ThemeDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    assert!(actual.edit_at(&actual.render(), &[5], "用户草稿".into()));
    assert!(expected.edit_at(&expected.render(), &[5], "用户草稿".into()));
    for path in [[2], [3], [4], [9]] {
        assert!(actual.dispatch_at(&actual.render(), &path, "clicked"));
        assert!(expected.dispatch_at(&expected.render(), &path, "clicked"));
        assert_same_scene(&actual.render(), &expected.render());
    }
    assert_eq!(
        actual.get("scheme").unwrap(),
        Value::ColorScheme(ColorScheme::Dark)
    );
    assert_eq!(
        actual.get("accent").unwrap(),
        Value::Color(Color(0xf5a63bff))
    );
    assert_eq!(
        actual.get("draft").unwrap(),
        Value::String("用户草稿".into())
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn uniform_transform_preserves_hit_testing_and_rotated_slider_coordinates() {
    use layout_ui::TransformDemo;
    use xrack_layout::{Interaction, Point, Size, SliderGeometry};
    use xrack_runtime::Orientation;

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/layout/layout.xr"),
        "layout.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TransformDemo")
        .unwrap();
    let mut expected = TransformDemo::new();

    fn exercise(ui: &mut impl Component) {
        assert!(ui.dispatch_at(&ui.render(), &[3], "clicked"));
        let mut input = Interaction::default();
        let size = Size::new(460.0, 286.0);
        let (scene, layout) = prepare_list(ui, &input, size);
        let hot = layout.at_path(&[1, 0, 0]).unwrap();
        let center = hot.map_to_window(Point {
            x: hot.bounds.x + hot.bounds.width / 2.0,
            y: hot.bounds.y + hot.bounds.height / 2.0,
        });
        input.pointer_move(&scene, &layout, center);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.path, [1, 0, 0]);
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));

        let (scene, layout) = prepare_list(ui, &input, size);
        let path = [1, 1];
        let slider = layout.at_path(&path).unwrap();
        let geometry = SliderGeometry::new(slider.bounds, Orientation::Horizontal);
        let start = slider.map_to_window(geometry.position(0.0));
        let end = slider.map_to_window(geometry.position(0.7));
        assert!(input.range_pointer_press(&scene, &layout, start));
        if let Some(action) = input.slider_press(&scene, &layout) {
            ui.adjust_range_at(&scene, &action.path, action.action);
        }
        input.pointer_move(&scene, &layout, end);
        let drag = input.slider_drag(&scene, &layout).unwrap();
        assert!(ui.adjust_range_at(&scene, &drag.path, drag.action));
        let (scene, layout) = prepare_list(ui, &input, size);
        let release = input.slider_release(&scene, &layout).unwrap();
        assert!(ui.adjust_range_at(&scene, &release.path, release.action));
    }
    exercise(&mut actual);
    exercise(&mut expected);
    assert_eq!(actual.get("clicks").unwrap(), Value::Int(1));
    assert_eq!(actual.get("scale-factor").unwrap(), Value::Float(0.75));
    assert!((expected.get_amount() - 0.7).abs() < 0.001);
    assert_eq!(
        actual.get("amount").unwrap(),
        Value::Float(expected.get_amount())
    );
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn check_group_bulk_updates_include_hidden_members_without_child_events() {
    use controls_ui::CheckGroupDemo;
    use xrack_runtime::CheckState;

    let mut actual = interpret_controls_fixture("CheckGroupDemo");
    let mut expected = CheckGroupDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(
        actual.get("summary").unwrap(),
        Value::CheckState(CheckState::PartiallyChecked)
    );
    actual.set("show-validation", false.into()).unwrap();
    expected.set_show_validation(false);

    for state in [CheckState::Checked, CheckState::PartiallyChecked] {
        actual
            .invoke("apply-all", &[Value::CheckState(state)])
            .unwrap();
        expected.invoke_apply_all(state);
        assert_eq!(
            actual.get("summary").unwrap(),
            Value::CheckState(CheckState::Checked)
        );
        assert_same_scene(&actual.render(), &expected.render());
    }
    for name in ["assets", "metadata", "background-work"] {
        assert_eq!(actual.get(name).unwrap(), Value::Bool(true));
    }
    assert_eq!(
        actual.get("validation").unwrap(),
        Value::CheckState(CheckState::Checked)
    );
    assert_eq!(actual.get("child-edits").unwrap(), Value::Int(0));

    assert!(actual.dispatch_at(&actual.render(), &[0, 1], "clicked"));
    assert!(expected.dispatch_at(&expected.render(), &[0, 1], "clicked"));
    assert_eq!(
        actual.get("master-result").unwrap(),
        Value::CheckState(CheckState::Unchecked)
    );
    actual.set("group-enabled", false.into()).unwrap();
    expected.set_group_enabled(false);
    let scene = actual.render();
    let member = scene
        .at_path(&[0, 2, 0, 0])
        .unwrap()
        .button_group
        .as_ref()
        .unwrap();
    assert!(!member.group.exclusive && !member.group.enabled);
    assert_eq!(member.group.paths().len(), 4);
    assert!(!actual.dispatch_at(&scene, &[0, 2, 0, 0], "clicked"));
    assert_same_scene(&scene, &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn distributed_groups_reject_stale_values_and_disabled_check_groups() {
    use expressions_ui::DistributedSelectionParity;
    use xrack_runtime::ControlAction::ActivateRadioOption;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("DistributedSelectionParity")
        .unwrap();
    let mut expected = DistributedSelectionParity::new();
    let scene = actual.render_shared();
    let generated = expected.render_shared();
    actual.set("label", Value::String("c".into())).unwrap();
    expected.set_label("c".into());
    assert!(!actual.control_at(&scene, &[1], ActivateRadioOption));
    assert!(!expected.control_at(&generated, &[1], ActivateRadioOption));
    assert_eq!(actual.get("choice"), Ok(Value::String("a".into())));
    assert_eq!(expected.get_choice(), "a");
    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    assert!(!actual.dispatch_at(&scene, &[2], "clicked"));
    assert!(!expected.dispatch_at(&generated, &[2], "clicked"));
    assert_eq!(actual.get("checked"), Ok(false.into()));
    assert!(!expected.get_checked());

    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    let scene = actual.render_shared();
    let generated = expected.render_shared();
    assert!(actual.control_at(&scene, &[1], ActivateRadioOption));
    assert!(expected.control_at(&generated, &[1], ActivateRadioOption));
    assert!(!actual.control_at(&scene, &[1], ActivateRadioOption));
    assert!(!expected.control_at(&generated, &[1], ActivateRadioOption));
    assert!(actual.dispatch_at(&scene, &[2], "clicked"));
    assert!(expected.dispatch_at(&generated, &[2], "clicked"));
    assert_eq!(actual.get("selected"), Ok(Value::String("c".into())));
    assert_eq!(expected.get_selected(), "c");
    assert_eq!(actual.get("toggles"), Ok(1_i64.into()));
    assert_eq!(expected.get_toggles(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn distributed_radio_groups_keep_declaration_slots_and_nested_component_paths() {
    use selection_ui::GroupedRadiosDemo;
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/selection/selection.xr"),
        "selection.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("GroupedRadiosDemo")
        .unwrap();
    let mut expected = GroupedRadiosDemo::new();
    assert_same_scene(&actual.render(), &expected.render());

    let activate = ControlAction::ActivateRadioOption;
    assert!(actual.control_at(&actual.render(), &[0, 1, 0, 0], activate));
    assert!(expected.control_at(&expected.render(), &[0, 1, 0, 0], activate));
    assert_eq!(actual.get("mode").unwrap(), Value::String("fast".into()));
    assert_eq!(
        actual.get("last-mode").unwrap(),
        Value::String("fast".into())
    );
    let stale = actual.render();
    let expected_stale = expected.render();
    actual.set("mode-enabled", false.into()).unwrap();
    expected.set_mode_enabled(false);
    assert!(!actual.control_at(&stale, &[0, 1, 1, 0], activate));
    assert!(!expected.control_at(&expected_stale, &[0, 1, 1, 0], activate));

    actual.set("show-high", false.into()).unwrap();
    expected.set_show_high(false);
    let scene = actual.render();
    let path = [0, 2, 3];
    let member = scene.at_path(&path).unwrap().button_group.as_ref().unwrap();
    assert_eq!(member.component_path(&path), Some(&[0, 2][..]));
    assert_eq!(member.index, 2);
    assert!(actual.control_at(&scene, &path, activate));
    assert!(expected.control_at(&expected.render(), &path, activate));
    assert_eq!(actual.get("quality").unwrap(), Value::Int(2));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

fn interpret_navigation_fixture(name: &str) -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/navigation/navigation.xr"),
        "navigation.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

#[test]
fn page_input_rejects_unmounted_children_without_rendering_the_new_page() {
    use expressions_ui::MountedPageInputParity;
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("MountedPageInputParity")
        .unwrap();
    let mut expected = MountedPageInputParity::new();
    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    actual
        .on("page-content", {
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(Value::String("new page".into())))
            })
        })
        .unwrap();
    expected.on_page_content({
        let reads = expected_reads.clone();
        move || {
            reads.set(reads.get() + 1);
            "new page".into()
        }
    });
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let paths = [[0, 0, 0], [1, 0, 0], [2, 0, 0]];
    for path in paths {
        assert!(
            actual.dispatch_at(&actual_scene, &path, "clicked"),
            "{path:?}"
        );
        assert!(
            expected.dispatch_at(&expected_scene, &path, "clicked"),
            "{path:?}"
        );
    }
    actual.set("tab", 1_i64.into()).unwrap();
    expected.set_tab(1);
    actual.set("swipe", 1_i64.into()).unwrap();
    expected.set_swipe(1);
    actual.invoke("advance", &[]).unwrap();
    expected.invoke_advance();
    for path in paths {
        assert!(
            !actual.dispatch_at(&actual_scene, &path, "clicked"),
            "{path:?}"
        );
        assert!(
            !expected.dispatch_at(&expected_scene, &path, "clicked"),
            "{path:?}"
        );
    }
    assert_eq!(actual.get("clicks"), Ok(111_i64.into()));
    assert_eq!(expected.get_clicks(), 111);
    assert_eq!((actual_reads.get(), expected_reads.get()), (0, 0));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn navigation_pages_are_lazy_and_preserve_nested_component_state() {
    use navigation_ui::NavigationDemo;
    use xrack_runtime::ControlAction;

    let mut actual = interpret_navigation_fixture("NavigationDemo");
    let mut expected = NavigationDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    let tabs = [3, 0];
    let counter = [3, 0, 0, 0, 1];
    let swipe = [7, 0];
    for ui in [&mut actual as &mut dyn Component, &mut expected] {
        assert!(ui.dispatch_at(&ui.render(), &counter, "clicked"));
        assert!(ui.control_at(&ui.render(), &tabs, ControlAction::Select(1)));
        assert!(!ui.control_at(&ui.render(), &tabs, ControlAction::Select(2)));
        assert!(ui.render().at_path(&[3, 0, 0]).unwrap().children.is_empty());
        assert!(ui.control_at(&ui.render(), &tabs, ControlAction::Select(0)));
        let scene = ui.render();
        assert_eq!(
            scene.at_path(&[3, 0, 0, 0, 0]).unwrap().text("text"),
            Some("此页计数：1")
        );
        assert!(ui.control_at(&scene, &swipe, ControlAction::Select(1)));
        assert!(ui.render().at_path(&[7, 0, 0]).unwrap().children.is_empty());
    }
    actual.set("show-middle", false.into()).unwrap();
    expected.set_show_middle(false);
    assert_same_scene(&actual.render(), &expected.render());
    let scene = actual.render();
    assert_eq!(
        xrack_runtime::group_index(scene.at_path(&swipe).unwrap()),
        Some(0)
    );
    assert!(actual.control_at(&scene, &[7, 1], ControlAction::Select(2)));
    assert!(expected.control_at(&expected.render(), &[7, 1], ControlAction::Select(2)));
    assert_eq!(actual.get("swipe-page").unwrap(), Value::Int(2));
    assert_eq!(actual.get("indicator-changes").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn swipe_view_mounts_the_available_page_after_applying_states() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_layout::{Size, layout};
    use xrack_runtime::{ControlAction, group_index};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("StatefulSwipeParity")
        .unwrap();
    let mut generated = expressions_ui::StatefulSwipeParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("page-body", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(page)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*page);
                Ok(Some(Value::String(page.to_string())))
            })
        })
        .unwrap();
    generated.on_page_body({
        let calls = expected_calls.clone();
        move |page| {
            calls.borrow_mut().push(page);
            page.to_string()
        }
    });

    interpreted
        .on("page-flag", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                calls.borrow_mut().push(-1);
                Ok(Some(args[0].clone()))
            })
        })
        .unwrap();
    generated.on_page_flag({
        let calls = expected_calls.clone();
        move |active| {
            calls.borrow_mut().push(-1);
            active
        }
    });

    for (hide, disable, active, label, calls) in [
        (false, false, Some(0), Some("0"), &[-1, 0][..]),
        (true, false, Some(1), Some("2"), &[-1, 2][..]),
        (true, true, None, None, &[][..]),
    ] {
        interpreted.set("hide-first", hide.into()).unwrap();
        generated.set_hide_first(hide);
        interpreted.set("disable-second", disable.into()).unwrap();
        generated.set_disable_second(disable);

        let actual = interpreted.try_render().unwrap();
        let expected = generated.render();
        for scene in [&*actual, &expected] {
            assert_eq!(group_index(scene.find("pages").unwrap()), active);
            let laid_out = layout(scene, Size::new(320.0, 200.0), &mut FixedText);
            for index in 0..2 {
                assert_eq!(
                    laid_out.at_path(&[0, index]).unwrap().visible,
                    active == Some(index)
                );
            }
        }
        let mounted = |scene: &Node| {
            scene
                .find("pages")
                .unwrap()
                .children
                .iter()
                .map(|page| {
                    page.children
                        .first()
                        .and_then(|child| child.text("text"))
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>()
        };
        let mut expected_labels = vec![None; 2];
        if let Some(index) = active {
            expected_labels[index] = label.map(str::to_owned);
        }
        assert_eq!(
            [mounted(&actual), mounted(&expected)],
            [expected_labels.clone(), expected_labels]
        );
        assert_same_scene(&actual, &expected);
        assert_eq!(actual_calls.take(), calls);
        assert_eq!(expected_calls.take(), calls);
    }
    assert_eq!(interpreted.get("selection"), Ok(0_i64.into()));
    assert_eq!(generated.get_selection(), 0);
    assert_eq!(interpreted.get("changes"), Ok(0_i64.into()));
    assert_eq!(generated.get_changes(), 0);

    interpreted.set("hide-first", false.into()).unwrap();
    generated.set_hide_first(false);
    interpreted.set("disable-second", false.into()).unwrap();
    generated.set_disable_second(false);
    for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
        assert!(ui.control_at(&ui.render(), &[0], ControlAction::Select(1)));
    }
    let actual = interpreted.try_render().unwrap();
    let expected = generated.render();
    assert_same_scene(&actual, &expected);
    assert_eq!(actual.find("detail").unwrap().text("text"), Some("1"));
    assert_eq!(interpreted.get("changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn tab_bar_repeats_options_and_shares_selection_with_swipe_view() {
    use navigation_ui::TabBarDemo;
    use xrack_runtime::ControlAction;

    let mut actual = interpret_navigation_fixture("TabBarDemo");
    let mut expected = TabBarDemo::new();
    assert_same_scene(&actual.render(), &expected.render());
    for ui in [&mut actual as &mut dyn Component, &mut expected] {
        assert!(!ui.control_at(&ui.render(), &[1], ControlAction::Select(1)));
        assert!(!ui.control_at(&ui.render(), &[1], ControlAction::Select(2)));
        assert!(ui.control_at(&ui.render(), &[1], ControlAction::Select(5)));
        let scene = ui.render();
        let workspace = scene.at_path(&[2]).unwrap();
        assert_eq!(xrack_runtime::group_index(workspace), Some(5));
        assert!(
            workspace.children[..5]
                .iter()
                .all(|page| page.children.is_empty())
        );
        assert_eq!(scene.at_path(&[4]).unwrap().children.len(), 3);
        assert!(ui.control_at(&scene, &[4], ControlAction::Select(2)));
    }
    actual
        .set("filters", Value::StringList(vec!["唯一".into()]))
        .unwrap();
    expected.set_filters(vec!["唯一".into()]);
    assert_eq!(actual.get("filter").unwrap(), Value::Int(2));
    assert_eq!(
        xrack_runtime::group_index(actual.render().at_path(&[4]).unwrap()),
        Some(0)
    );
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn navigation_selection_checks_live_option_and_current_selection() {
    use expressions_ui::NavigationSelectionParity;
    use std::rc::Rc;
    use xrack_runtime::ControlAction::Select;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("NavigationSelectionParity")
        .unwrap();
    let mut expected = NavigationSelectionParity::new();
    let scene = actual.render_shared();
    let generated = expected.render_shared();
    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    actual.set("shown", false.into()).unwrap();
    expected.set_shown(false);
    for path in [[0], [1], [2]] {
        assert!(!actual.control_at(&scene, &path, Select(1)), "{path:?}");
        assert!(
            !expected.control_at(&generated, &path, Select(1)),
            "{path:?}"
        );
    }
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    actual.set("shown", true.into()).unwrap();
    expected.set_shown(true);
    actual
        .on(
            "label",
            Rc::new(|_| Ok(Some(Value::String("replacement".into())))),
        )
        .unwrap();
    expected.on_label(|| "replacement".into());
    assert!(!actual.control_at(&scene, &[0], Select(1)));
    assert!(!expected.control_at(&generated, &[0], Select(1)));
    assert!(
        actual
            .get("events")
            .is_ok_and(|events| events == Value::String(String::new()))
    );
    assert!(expected.get_events().is_empty());

    let scene = actual.render_shared();
    let generated = expected.render_shared();
    for path in [[0], [1], [2]] {
        assert!(actual.control_at(&scene, &path, Select(1)), "{path:?}");
        assert!(
            expected.control_at(&generated, &path, Select(1)),
            "{path:?}"
        );
        assert!(!actual.control_at(&scene, &path, Select(1)), "{path:?}");
        assert!(
            !expected.control_at(&generated, &path, Select(1)),
            "{path:?}"
        );
    }
    assert_eq!(
        actual.get("events"),
        Ok(Value::String("replacement;tab;bar;".into()))
    );
    assert_eq!(expected.get_events(), "replacement;tab;bar;");
    let scene = actual.render_shared();
    let generated = expected.render_shared();
    actual.set("tab", 0_i64.into()).unwrap();
    expected.set_tab(0);
    assert!(actual.control_at(&scene, &[1], Select(1)));
    assert!(expected.control_at(&generated, &[1], Select(1)));
    assert_eq!(
        actual.get("events"),
        Ok(Value::String("replacement;tab;bar;tab;".into()))
    );
    assert_eq!(expected.get_events(), "replacement;tab;bar;tab;");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn repeated_navigation_groups_reject_stale_option_models() {
    use navigation_ui::{NavigationDemo, TabBarDemo};
    use xrack_runtime::ControlAction::Select;

    let mut radio = interpret_navigation_fixture("NavigationDemo");
    let mut compiled_radio = NavigationDemo::new();
    let radio_path = [2, 0];
    let radio_scene = radio.render();
    let compiled_radio_scene = compiled_radio.render();
    let rows = radio.string_model("profiles").unwrap();
    let compiled_rows = compiled_radio.model_profiles();
    let label = "已更新";
    for model in [&rows, &compiled_rows] {
        let key = model.row_key(1).unwrap();
        assert!(model.set_row(1, label.into()));
        assert_eq!(model.row_key(1), Some(key));
    }

    let mut tabs = interpret_navigation_fixture("TabBarDemo");
    let mut compiled_tabs = TabBarDemo::new();
    let tab_path = [4];
    let tab_scene = tabs.render();
    let compiled_tab_scene = compiled_tabs.render();
    let filters = tabs.string_model("filters").unwrap().snapshot();
    tabs.set("filters", Value::StringList(filters.clone()))
        .unwrap();
    compiled_tabs.set_filters(filters);

    assert_eq!(
        [
            radio.control_at(&radio_scene, &radio_path, Select(1)),
            compiled_radio.control_at(&compiled_radio_scene, &radio_path, Select(1)),
            tabs.control_at(&tab_scene, &tab_path, Select(1)),
            compiled_tabs.control_at(&compiled_tab_scene, &tab_path, Select(1)),
        ],
        [false; 4],
    );
    assert_eq!(radio.get("profile"), Ok(0_i64.into()));
    assert_eq!(compiled_radio.get_profile(), 0);
    assert_eq!(radio.get("profile-label"), Ok(Value::String("均衡".into())));
    assert_eq!(compiled_radio.get_profile_label(), "均衡");
    assert_eq!(tabs.get("filter"), Ok(0_i64.into()));
    assert_eq!(compiled_tabs.get_filter(), 0);

    let tab_scene = tabs.render();
    let compiled_tab_scene = compiled_tabs.render();
    assert!(
        tabs.string_model("filters")
            .unwrap()
            .insert(0, "新选项".into())
    );
    assert!(compiled_tabs.model_filters().insert(0, "新选项".into()));
    assert!(!tabs.control_at(&tab_scene, &tab_path, Select(1)));
    assert!(!compiled_tabs.control_at(&compiled_tab_scene, &tab_path, Select(1)));

    for ui in [&mut radio as &mut dyn Component, &mut compiled_radio] {
        assert!(ui.control_at(&ui.render(), &radio_path, Select(1)));
    }
    for ui in [&mut tabs as &mut dyn Component, &mut compiled_tabs] {
        assert!(ui.control_at(&ui.render(), &tab_path, Select(1)));
    }
    assert_eq!(radio.get("profile-label"), Ok(Value::String(label.into())));
    assert_eq!(compiled_radio.get_profile_label(), label);
    assert_eq!(tabs.get("filter"), Ok(1_i64.into()));
    assert_eq!(compiled_tabs.get_filter(), 1);
    assert_eq!(radio.take_error(), None);
    assert_eq!(tabs.take_error(), None);
}

#[test]
fn virtual_check_delegates_dispatch_after_committing_the_row() {
    use expressions_ui::ToggleDelegateEvents;
    use std::rc::Rc;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::CheckState;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ToggleDelegateEvents")
        .unwrap();
    let mut generated = ToggleDelegateEvents::new();
    let input = Interaction::default();
    let size = Size::new(240.0, 180.0);
    let checked = [0, 0, 0];
    let tristate = [1, 0, 0];

    let (scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    for scene in [&scene, &generated_scene] {
        assert_eq!(
            scene
                .dispatch_target(&checked, "clicked")
                .map(|node| node.kind),
            Some("CheckBox")
        );
    }
    assert!(
        actual.dispatch_at(&scene, &checked, "clicked"),
        "{:?}",
        actual.take_error()
    );
    assert!(generated.dispatch_at(&generated_scene, &checked, "clicked"));
    assert!(!actual.dispatch_at(&scene, &checked, "clicked"));
    assert!(!generated.dispatch_at(&generated_scene, &checked, "clicked"));
    assert_eq!(actual.get("trace"), Ok(Value::Int(13)));
    assert_eq!(generated.get_trace(), 13);

    let (scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, scene),
        (&mut generated, generated_scene),
    ] {
        assert!(ui.dispatch_at(&scene, &checked, "toggled"));
        assert!(ui.dispatch_at(&scene, &tristate, "clicked"));
    }
    assert_eq!(actual.get("trace"), Ok(Value::Int(13156)));
    assert_eq!(generated.get_trace(), 13156);
    let row = &generated.get_rows()[0];
    assert!(row.enabled);
    assert_eq!(row.state, CheckState::PartiallyChecked);
    assert_same_scene(
        &prepare_list(&actual, &input, size).0,
        &prepare_list(&generated, &input, size).0,
    );
    let (scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    actual.set("three-states", false.into()).unwrap();
    generated.set_three_states(false);
    assert!(!actual.dispatch_at(&scene, &tristate, "clicked"));
    assert!(!generated.dispatch_at(&generated_scene, &tristate, "clicked"));
    assert_eq!(actual.get("trace"), Ok(13156_i64.into()));
    assert_eq!(generated.get_trace(), 13156);
    actual
        .on("copy-rows", Rc::new(|args| Ok(args.first().cloned())))
        .unwrap();
    generated.on_copy_rows(|rows| rows);
    let (scene, _) = prepare_list(&actual, &input, size);
    let (generated_scene, _) = prepare_list(&generated, &input, size);
    assert_same_scene(&scene, &generated_scene);
    assert!(actual.dispatch_at(&scene, &checked, "clicked"));
    assert!(generated.dispatch_at(&generated_scene, &checked, "clicked"));
    assert_eq!(actual.get("trace"), Ok(Value::Int(1315613)));
    assert_eq!(generated.get_trace(), 1315613);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn item_delegates_share_selection_and_preserve_record_fields() {
    use list_ui::ItemDelegatesDemo;
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{CheckState, ControlAction};

    let mut actual = interpret_list_fixture("ItemDelegatesDemo");
    let mut expected = ItemDelegatesDemo::new();
    let input = Interaction::default();
    let size = Size::new(644.0, 398.0);
    let (scene, _) = prepare_list(&actual, &input, size);
    let (expected_scene, _) = prepare_list(&expected, &input, size);
    assert_same_scene(&scene, &expected_scene);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, scene),
        (&mut expected, expected_scene),
    ] {
        assert!(ui.control_at(&scene, &[0, 0, 0, 1], ControlAction::Select(2)));
        assert!(ui.dispatch_at(&scene, &[0, 0, 0, 1, 2, 0], "clicked"));
        assert!(ui.control_at(&scene, &[0, 0, 1, 1], ControlAction::Select(1)));
        assert!(ui.dispatch_at(&scene, &[0, 1, 0, 1, 0, 0], "clicked"));
    }
    assert_eq!(
        actual.get("last-action").unwrap(),
        Value::String(expected.get_last_action())
    );
    assert_eq!(actual.get("current-mode").unwrap(), Value::Int(1));
    let Value::RecordList(settings) = actual.get("settings").unwrap() else {
        panic!("expected settings")
    };
    assert_eq!(
        settings.model().row(0).unwrap().field("selected"),
        Some(&Value::CheckState(CheckState::PartiallyChecked))
    );
    assert_eq!(actual.get("edits").unwrap(), Value::Int(1));
    assert_same_scene(
        &prepare_list(&actual, &input, size).0,
        &prepare_list(&expected, &input, size).0,
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn stack_commands_keep_page_state_and_reject_stale_back_requests() {
    use navigation_ui::StackDemo;
    use xrack_runtime::{ControlAction, StackNavigation};

    let mut actual = interpret_navigation_fixture("StackDemo");
    let mut expected = StackDemo::new();
    let stack = [0, 2];
    let compare = |actual: &Instance, expected: &StackDemo| {
        let snapshot = actual.stack_snapshot("navigation").unwrap();
        let expected_snapshot = expected.stack_snapshot("navigation").unwrap();
        assert_eq!(snapshot.routes(), expected_snapshot.routes());
        assert_same_scene(&actual.render(), &expected.render());
    };
    compare(&actual, &expected);
    assert_eq!(
        actual.stack_snapshot("navigation").unwrap().routes(),
        ["overview"]
    );
    assert!(
        actual
            .navigate_stack("navigation", StackNavigation::Push("details"))
            .unwrap()
    );
    assert!(expected.navigate_stack("navigation", StackNavigation::Push("details")));
    let counter = [0, 2, 1, 0, 0, 1];
    assert!(actual.dispatch_at(&actual.render(), &counter, "clicked"));
    assert!(expected.dispatch_at(&expected.render(), &counter, "clicked"));
    let stale = actual.render();
    let expected_stale = expected.render();
    let old_back = |scene: &Node| ControlAction::StackBack {
        revision: scene
            .at_path(&stack)
            .unwrap()
            .stack
            .as_ref()
            .unwrap()
            .revision(),
    };
    assert!(
        actual
            .navigate_stack("navigation", StackNavigation::Push("review"))
            .unwrap()
    );
    assert!(expected.navigate_stack("navigation", StackNavigation::Push("review")));
    assert!(!actual.control_at(&stale, &stack, old_back(&stale)));
    assert!(!expected.control_at(&expected_stale, &stack, old_back(&expected_stale)));
    assert!(
        actual
            .render()
            .at_path(&[0, 2, 1])
            .unwrap()
            .children
            .is_empty()
    );
    compare(&actual, &expected);

    assert!(
        actual
            .navigate_stack("navigation", StackNavigation::Replace("summary"))
            .unwrap()
    );
    assert!(expected.navigate_stack("navigation", StackNavigation::Replace("summary")));
    let scene = actual.render();
    let expected_scene = expected.render();
    assert!(actual.control_at(&scene, &stack, old_back(&scene)));
    assert!(expected.control_at(&expected_scene, &stack, old_back(&expected_scene)));
    assert_eq!(
        actual
            .render()
            .at_path(&[0, 2, 1, 0, 0, 0])
            .unwrap()
            .text("text"),
        Some("此页计数：1")
    );
    compare(&actual, &expected);

    let tail = ["review".to_owned(), "summary".to_owned()];
    let deep = [
        "overview".to_owned(),
        "details".to_owned(),
        "review".to_owned(),
    ];
    for (action, result) in [
        (StackNavigation::PushPages(&tail), true),
        (StackNavigation::PopTo("overview"), true),
        (
            StackNavigation::PushPages(&["details".into(), "missing".into()]),
            false,
        ),
        (StackNavigation::ResetTo(&deep), true),
        (StackNavigation::Reset("overview"), true),
        (StackNavigation::Pop, false),
    ] {
        assert_eq!(actual.navigate_stack("navigation", action).unwrap(), result);
        assert_eq!(expected.navigate_stack("navigation", action), result);
        compare(&actual, &expected);
    }
    assert_eq!(
        actual.get("navigations").unwrap(),
        Value::Int(expected.get_navigations())
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn stack_path_commands_sample_once_and_notify_in_lifecycle_order() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("StackPathCommandParity")
        .unwrap();
    let mut compiled = expressions_ui::StackPathCommandParity::new();
    let actual_reads = Rc::new(RefCell::new(Vec::new()));
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    let path = |reads: &RefCell<Vec<i64>>, tag| {
        reads.borrow_mut().push(tag);
        match tag {
            1 => vec!["detail".to_owned(), "review".to_owned()],
            _ => vec!["home".to_owned()],
        }
    };
    actual
        .on("path", {
            let reads = actual_reads.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                Ok(Some(Value::StringList(path(&reads, *tag))))
            })
        })
        .unwrap();
    compiled.on_path({
        let reads = compiled_reads.clone();
        move |tag| path(&reads, tag)
    });
    actual.invoke("append-path", &[]).unwrap();
    compiled.invoke_append_path();
    assert_eq!(
        actual.stack_snapshot("pages").unwrap().routes(),
        ["home", "detail", "review"]
    );
    assert_eq!(
        compiled.stack_snapshot("pages").unwrap().routes(),
        ["home", "detail", "review"]
    );

    actual.invoke("replace-path", &[]).unwrap();
    compiled.invoke_replace_path();
    assert_eq!(actual.stack_snapshot("pages").unwrap().routes(), ["home"]);
    assert_eq!(compiled.stack_snapshot("pages").unwrap().routes(), ["home"]);
    let events = "-home;+review;review:3;-review;remove-review;remove-detail;+home;home:1;";
    assert_eq!(actual.get("events"), Ok(events.to_owned().into()));
    assert_eq!(compiled.get_events(), events);
    assert_eq!(actual_reads.take(), [1, 2]);
    assert_eq!(compiled_reads.take(), [1, 2]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn stack_guards_share_alias_callbacks_and_lifecycle_order() {
    use navigation_ui::StackGuardDemo;
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{ControlAction, DialogRole, StackNavigation};

    let mut actual = interpret_navigation_fixture("StackGuardDemo");
    let mut expected = StackGuardDemo::new();
    let compare = |actual: &Instance, expected: &StackGuardDemo| {
        assert_same_scene(&actual.render(), &expected.render());
        assert_eq!(
            actual.get("lifecycle").unwrap(),
            Value::String(expected.get_lifecycle())
        );
        assert_eq!(
            actual.get("changes").unwrap(),
            Value::Int(expected.get_changes())
        );
        assert_eq!(
            actual.get("editor-index").unwrap(),
            Value::Int(expected.get_editor_index())
        );
        assert_eq!(
            actual.get("editor-active").unwrap(),
            Value::Bool(expected.get_editor_active())
        );
    };
    assert!(
        actual
            .navigate_stack("navigation", StackNavigation::Push("editor"))
            .unwrap()
    );
    assert!(expected.navigate_stack("navigation", StackNavigation::Push("editor")));
    let editor = [2, 1, 0, 1];
    assert!(actual.edit_at(&actual.render(), &editor, "未保存".into()));
    assert!(expected.edit_at(&expected.render(), &editor, "未保存".into()));
    assert!(
        !actual
            .navigate_stack("navigation", StackNavigation::Pop)
            .unwrap()
    );
    assert!(!expected.navigate_stack("navigation", StackNavigation::Pop));
    let dialog = [4];
    assert!(
        actual
            .render()
            .at_path(&dialog)
            .unwrap()
            .boolean("open", false)
    );
    compare(&actual, &expected);
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    assert!(actual.control_at(&actual.render(), &dialog, cancel));
    assert!(expected.control_at(&expected.render(), &dialog, cancel));
    assert_eq!(actual.get("draft").unwrap(), Value::String("未保存".into()));

    assert!(
        actual
            .navigate_stack("navigation", StackNavigation::Push("preview"))
            .unwrap()
    );
    assert!(expected.navigate_stack("navigation", StackNavigation::Push("preview")));
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    let calls = actual_calls.clone();
    actual
        .on(
            "can-leave-preview",
            Rc::new(move |args| {
                let [Value::String(destination), Value::Bool(removing)] = args else {
                    return Err("invalid guard arguments".into());
                };
                calls.borrow_mut().push((destination.clone(), *removing));
                Ok(Some(Value::Bool(false)))
            }),
        )
        .unwrap();
    let calls = expected_calls.clone();
    expected.on_can_leave_preview(move |destination, removing| {
        calls.borrow_mut().push((destination, removing));
        false
    });
    let reset = StackNavigation::Reset("home");
    assert!(!actual.navigate_stack("navigation", reset).unwrap());
    assert!(!expected.navigate_stack("navigation", reset));
    assert_eq!(*actual_calls.borrow(), [("home".into(), true)]);
    assert_eq!(*actual_calls.borrow(), *expected_calls.borrow());
    assert_eq!(
        actual.stack_snapshot("navigation").unwrap().current(),
        Some("preview")
    );
    compare(&actual, &expected);

    actual
        .on(
            "can-leave-preview",
            Rc::new(|_| Ok(Some(Value::Bool(true)))),
        )
        .unwrap();
    expected.on_can_leave_preview(|_, _| true);
    assert!(!actual.navigate_stack("navigation", reset).unwrap());
    assert!(!expected.navigate_stack("navigation", reset));
    assert!(
        actual
            .render()
            .at_path(&dialog)
            .unwrap()
            .boolean("open", false)
    );
    let accept = ControlAction::Dialog(DialogRole::Accept);
    assert!(actual.control_at(&actual.render(), &dialog, accept));
    assert!(expected.control_at(&expected.render(), &dialog, accept));
    assert_eq!(
        actual.stack_snapshot("navigation").unwrap().current(),
        Some("home")
    );
    assert_eq!(actual.get("draft").unwrap(), actual.get("saved").unwrap());
    assert_eq!(actual.get("changes").unwrap(), Value::Int(3));
    assert!(expected.get_lifecycle().ends_with(
        "preview deactivated preview removed editor removed home activated changed home 1"
    ));
    compare(&actual, &expected);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn typed_input_observes_live_disablement_and_delivers_cleanup() {
    use xrack_runtime::{
        DragAction, DragEvent, EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase,
        PointerEvent, PointerEventKind, WheelEvent,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TypedInputAvailabilityParity")
        .unwrap();
    let mut generated = expressions_ui::TypedInputAvailabilityParity::new();
    let interpreted_scene = interpreted.render_shared();
    let generated_scene = generated.render_shared();
    let wheel = WheelEvent::default();
    let reason = FocusReason::LayoutChange;
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let down = PointerEvent {
        kind: PointerEventKind::Down,
        ..Default::default()
    };
    let cancel = PointerEvent {
        kind: PointerEventKind::Cancel,
        ..down
    };
    for (ui, scene) in [
        (&mut interpreted as &mut dyn Component, &*interpreted_scene),
        (&mut generated, &*generated_scene),
    ] {
        assert_eq!(ui.wheel_event_at(scene, &[1], &wheel), EventResult::Accept);
        assert!(ui.focus_event_at(scene, &[2], FocusEvent::Gained(reason)));
        assert_eq!(ui.wheel_event_at(scene, &[0], &wheel), EventResult::Reject);
        assert_eq!(ui.wheel_event_at(scene, &[1], &wheel), EventResult::Reject);
        assert!(!ui.focus_event_at(scene, &[2], FocusEvent::Gained(reason)));
        assert_eq!(
            ui.key_event_at(scene, &[2], KeyPhase::Pressed, &key),
            EventResult::Reject
        );
        assert!(!ui.pointer_event_at(scene, &[3], &down));
        assert!(!ui.drag_event_at(scene, &[4], &DragEvent::Started));
        assert!(ui.focus_event_at(scene, &[2], FocusEvent::Lost(reason)));
        assert!(ui.pointer_event_at(scene, &[3], &cancel));
        assert!(ui.drag_event_at(scene, &[4], &DragEvent::Finished(DragAction::Copy)));
    }
    assert_eq!(
        generated.get_events(),
        "wheel;gain;focus;loss;focus;pointer;finish;"
    );
    assert_eq!(
        interpreted.get("events"),
        Ok(Value::String(generated.get_events()))
    );
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn typed_input_rechecks_ancestors_across_component_boundaries() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase, WheelEvent};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TypedInputAncestorParity")
        .unwrap();
    let mut expected = expressions_ui::TypedInputAncestorParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let wheel = WheelEvent::default();
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let loss = FocusEvent::Lost(FocusReason::LayoutChange);
    let wheel_path = [0, 0, 0];
    let focus_path = [0, 0, 1];

    actual.set("allowed", false.into()).unwrap();
    expected.set_allowed(false);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        assert_eq!(
            ui.wheel_event_at(scene, &wheel_path, &wheel),
            EventResult::Reject
        );
        assert_eq!(
            ui.key_event_at(scene, &focus_path, KeyPhase::Pressed, &key),
            EventResult::Reject
        );
        assert!(ui.focus_event_at(scene, &focus_path, loss));
    }
    actual.set("allowed", true.into()).unwrap();
    expected.set_allowed(true);
    actual.set("shown", false.into()).unwrap();
    expected.set_shown(false);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        assert_eq!(
            ui.wheel_event_at(scene, &wheel_path, &wheel),
            EventResult::Reject
        );
        assert_eq!(
            ui.wheel_event_at(scene, &[1, 0], &wheel),
            EventResult::Accept
        );
    }
    actual.set("shown", true.into()).unwrap();
    expected.set_shown(true);
    assert_eq!(
        actual.wheel_event_at(&actual_scene, &wheel_path, &wheel),
        EventResult::Accept
    );
    assert_eq!(
        expected.wheel_event_at(&expected_scene, &wheel_path, &wheel),
        EventResult::Accept
    );
    actual
        .on("check-wheel", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("allowed", false.into())
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    expected.on_check_wheel({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_allowed(false);
            true
        }
    });
    assert_eq!(
        actual.wheel_event_at(&actual_scene, &wheel_path, &wheel),
        EventResult::Reject
    );
    assert_eq!(
        expected.wheel_event_at(&expected_scene, &wheel_path, &wheel),
        EventResult::Reject
    );
    assert_eq!(actual.get("allowed"), Ok(false.into()));
    assert!(!expected.get_allowed());
    assert_eq!(
        actual.get("events"),
        Ok(Value::String("loss;pass-through;wheel;".into()))
    );
    assert_eq!(expected.get_events(), "loss;pass-through;wheel;");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn typed_input_keeps_the_callers_availability_check() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{Property, RoutedEvent, WheelEvent};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TypedInputAncestorParity")
        .unwrap();
    let mut compiled = expressions_ui::TypedInputAncestorParity::new();
    let path = [0, 0, 0];
    let interpreted_scene = interpreted.render_shared();
    let compiled_scene = compiled.render_shared();
    let allowed = Rc::new(Property::default());
    let compiled_checks = Rc::new(Cell::new(0));
    let interpreted_checks = Rc::new(Cell::new(0));
    interpreted
        .on("check-wheel", {
            let allowed = allowed.clone();
            let checks = interpreted_checks.clone();
            Rc::new(move |_| {
                checks.set(checks.get() + 1);
                allowed.set(false);
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    compiled.on_check_wheel({
        let allowed = allowed.clone();
        let checks = compiled_checks.clone();
        move || {
            checks.set(checks.get() + 1);
            allowed.set(false);
            true
        }
    });

    let wheel = WheelEvent::default();
    let event = RoutedEvent::Wheel(&wheel);
    for (ui, scene, checks) in [
        (
            &mut compiled as &mut dyn Component,
            &*compiled_scene,
            compiled_checks,
        ),
        (&mut interpreted, &*interpreted_scene, interpreted_checks),
    ] {
        allowed.set(true);
        let handled = event.with_input(scene, &path, |input| {
            assert!(input.available(|| allowed.get(|| false)));
            ui.route_input(scene, &path, input)
        });
        assert_eq!(checks.get(), 1);
        assert!(!allowed.get(|| false));
        assert_eq!(handled, Some(false));
    }
    assert_eq!(compiled.get_events(), "");
    assert_eq!(interpreted.get("events"), Ok("".to_owned().into()));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn typed_input_rechecks_overlay_group_and_swipe_scope() {
    use xrack_runtime::{EventResult, FocusEvent, FocusReason, KeyEvent, KeyPhase, SwipeSide};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TypedInputScopeParity")
        .unwrap();
    let mut expected = expressions_ui::TypedInputScopeParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let key = KeyEvent {
        key: "x".into(),
        ..Default::default()
    };
    let paths: [&[usize]; 5] = [&[0, 0], &[1, 0], &[2, 0], &[3, 0, 0], &[4, 0, 0]];
    let content = [4, 1, 0];
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        for path in paths {
            assert_eq!(
                ui.key_event_at(scene, path, KeyPhase::Pressed, &key),
                EventResult::Accept,
                "{path:?}"
            );
        }
    }
    for property in ["checked", "dialog-open", "drawer-open", "popup-open"] {
        actual.set(property, false.into()).unwrap();
    }
    actual.set("side", SwipeSide::Right.into()).unwrap();
    expected.set_checked(false);
    expected.set_dialog_open(false);
    expected.set_drawer_open(false);
    expected.set_popup_open(false);
    expected.set_side(SwipeSide::Right);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        for path in paths {
            assert_eq!(
                ui.key_event_at(scene, path, KeyPhase::Pressed, &key),
                EventResult::Reject,
                "{path:?}"
            );
        }
        assert_eq!(
            ui.key_event_at(scene, &content, KeyPhase::Pressed, &key),
            EventResult::Accept
        );
        assert!(ui.focus_event_at(scene, paths[3], FocusEvent::Lost(FocusReason::LayoutChange)));
    }
    actual.set("checkable", false.into()).unwrap();
    expected.set_checkable(false);
    assert_eq!(
        actual.key_event_at(&actual_scene, paths[0], KeyPhase::Pressed, &key),
        EventResult::Accept
    );
    assert_eq!(
        expected.key_event_at(&expected_scene, paths[0], KeyPhase::Pressed, &key),
        EventResult::Accept
    );
    let right = [4, 2, 0];
    assert_eq!(
        actual.key_event_at(&actual.render_shared(), &right, KeyPhase::Pressed, &key),
        EventResult::Accept
    );
    assert_eq!(
        expected.key_event_at(&expected.render_shared(), &right, KeyPhase::Pressed, &key),
        EventResult::Accept
    );
    let events = "group;dialog;drawer;popup;left;content;lost-popup;group;right;";
    assert_eq!(actual.get("events"), Ok(Value::String(events.into())));
    assert_eq!(expected.get_events(), events);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn untyped_input_rechecks_container_state_and_keeps_gesture_cleanup() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TypedInputScopeParity")
        .unwrap();
    let mut expected = expressions_ui::TypedInputScopeParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let paths: [&[usize]; 5] = [
        &[0, 0, 0],
        &[1, 0, 0],
        &[2, 0, 0],
        &[3, 0, 0, 0],
        &[4, 0, 0, 0],
    ];
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        for path in paths {
            assert!(ui.dispatch_at(scene, path, "clicked"), "{path:?}");
        }
    }
    actual
        .on("read-popup-check", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("popup-open", false.into())
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(false.into()))
            })
        })
        .unwrap();
    expected.on_read_popup_check({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_popup_open(false);
            false
        }
    });
    assert!(!actual.dispatch_at(&actual_scene, &[3, 0, 0, 2], "clicked"));
    assert!(!expected.dispatch_at(&expected_scene, &[3, 0, 0, 2], "clicked"));
    assert_eq!(actual.get("popup-open"), Ok(false.into()));
    assert!(!expected.get_popup_open());
    for property in ["checked", "dialog-open", "drawer-open", "popup-open"] {
        actual.set(property, false.into()).unwrap();
    }
    actual
        .set("side", xrack_runtime::SwipeSide::Right.into())
        .unwrap();
    expected.set_checked(false);
    expected.set_dialog_open(false);
    expected.set_drawer_open(false);
    expected.set_popup_open(false);
    expected.set_side(xrack_runtime::SwipeSide::Right);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        for path in paths {
            assert!(!ui.dispatch_at(scene, path, "clicked"), "{path:?}");
        }
        let hover = [3, 0, 0, 1];
        assert!(!ui.dispatch_at(scene, &hover, "entered"));
        assert!(ui.dispatch_at(scene, &hover, "exited"));
        assert!(ui.dispatch_at(scene, &[4, 1, 0, 0], "clicked"));
    }
    let events = "group;dialog;drawer;popup;left;exited-popup;content;";
    assert_eq!(actual.get("events"), Ok(Value::String(events.into())));
    assert_eq!(expected.get_events(), events);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn text_edits_recheck_live_constraints_and_parent_overlay() {
    use xrack_runtime::InputType;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RoutedEditorParity")
        .unwrap();
    let mut expected = expressions_ui::RoutedEditorParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let field = [0, 0];

    actual.set("locked", true.into()).unwrap();
    expected.set_locked(true);
    assert!(!actual.edit_at(&actual_scene, &field, "12".into()));
    assert!(!expected.edit_at(&expected_scene, &field, "12".into()));
    actual.set("locked", false.into()).unwrap();
    expected.set_locked(false);

    actual
        .set("mode", Value::InputType(InputType::Number))
        .unwrap();
    expected.set_mode(InputType::Number);
    assert!(!actual.edit_at(&actual_scene, &field, "ab".into()));
    assert!(!expected.edit_at(&expected_scene, &field, "ab".into()));
    actual.set("limit", 2_i64.into()).unwrap();
    expected.set_limit(2);
    assert!(!actual.edit_at(&actual_scene, &field, "123".into()));
    assert!(!expected.edit_at(&expected_scene, &field, "123".into()));
    assert!(actual.edit_at(&actual_scene, &field, "12".into()));
    assert!(expected.edit_at(&expected_scene, &field, "12".into()));

    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    assert!(!actual.edit_at(&actual_scene, &field, "23".into()));
    assert!(!expected.edit_at(&expected_scene, &field, "23".into()));
    assert_eq!(actual.get("value"), Ok(Value::String("12".into())));
    assert_eq!(expected.get_value(), "12");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn text_submission_rechecks_the_live_draft_and_constraints() {
    use xrack_runtime::InputType;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("RoutedEditorParity")
        .unwrap();
    let mut generated = expressions_ui::RoutedEditorParity::new();
    let field = [0, 0];

    for (shown, live, mode, limit, separator, accepted) in [
        ("123", "123", InputType::Text, 2, ".", false),
        ("abc", "abc", InputType::Number, 12, ".", false),
        ("1.5", "1.5", InputType::Decimal, 12, ",", false),
        ("12", "abc", InputType::Number, 12, ".", false),
        ("-", "-", InputType::Decimal, 12, ".", false),
        ("12", "34", InputType::Number, 12, ".", true),
        ("1,5", "1,5", InputType::Decimal, 12, ",", true),
    ] {
        interpreted.set("mode", InputType::Text.into()).unwrap();
        generated.set_mode(InputType::Text);
        interpreted.set("limit", 12_i64.into()).unwrap();
        generated.set_limit(12);
        interpreted.set("value", shown.to_owned().into()).unwrap();
        generated.set_value(shown.to_owned());
        let actual_scene = interpreted.render_shared();
        let expected_scene = generated.render_shared();

        interpreted.set("mode", mode.into()).unwrap();
        generated.set_mode(mode);
        interpreted.set("limit", limit.into()).unwrap();
        generated.set_limit(limit);
        interpreted
            .set("separator", separator.to_owned().into())
            .unwrap();
        generated.set_separator(separator.to_owned());
        interpreted.set("value", live.to_owned().into()).unwrap();
        generated.set_value(live.to_owned());

        assert_eq!(
            interpreted.dispatch_at(&actual_scene, &field, "accepted"),
            accepted,
            "interpreted draft {shown:?} with {mode:?}, limit {limit} and value {live:?}"
        );
        assert_eq!(
            generated.dispatch_at(&expected_scene, &field, "accepted"),
            accepted,
            "compiled draft {shown:?} with {mode:?}, limit {limit} and value {live:?}"
        );
        let submitted = if accepted { live } else { "" };
        assert_eq!(
            interpreted.get("submitted"),
            Ok(submitted.to_owned().into())
        );
        assert_eq!(generated.get_submitted(), submitted);
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn control_input_rechecks_parent_overlay_through_combo_preparation() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction::{Drawer, Select};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RoutedChoiceParity")
        .unwrap();
    let mut expected = expressions_ui::RoutedChoiceParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let path = [0, 0, 0];
    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    assert!(!actual.control_at(&actual_scene, &path, Select(1)));
    assert!(!expected.control_at(&expected_scene, &path, Select(1)));
    assert!(actual.control_at(&actual_scene, &[1], Drawer(true)));
    assert!(expected.control_at(&expected_scene, &[1], Drawer(true)));
    assert_eq!(actual.get("drawer-open"), Ok(true.into()));
    assert!(expected.get_drawer_open());

    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    actual
        .on("can-edit", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("opened", false.into())
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(false.into()))
            })
        })
        .unwrap();
    expected.on_can_edit({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_opened(false);
            false
        }
    });
    assert!(!actual.control_at(&actual_scene, &path, Select(1)));
    assert!(!expected.control_at(&expected_scene, &path, Select(1)));
    assert_eq!(actual.get("opened"), Ok(false.into()));
    assert!(!expected.get_opened());
    assert_eq!(actual.get("selection"), Ok(0_i64.into()));
    assert_eq!(expected.get_selection(), 0);
    assert_eq!(actual.get("selected"), Ok(Value::String(String::new())));
    assert!(expected.get_selected().is_empty());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn range_input_rechecks_parent_scope_through_value_preparation() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::RangeAction::SetFraction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RoutedRangeParity")
        .unwrap();
    let mut expected = expressions_ui::RoutedRangeParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let slider = [0, 0, 0];
    let scrollbar = [0, 0, 1];
    let upper = [0, 0, 2, 1];
    let action = SetFraction(0.5);

    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    for path in [&slider[..], &scrollbar, &upper] {
        assert!(!actual.adjust_range_at(&actual_scene, path, action));
        assert!(!expected.adjust_range_at(&expected_scene, path, action));
    }
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    for path in [&scrollbar[..], &upper] {
        assert!(actual.adjust_range_at(&actual_scene, path, action));
        assert!(expected.adjust_range_at(&expected_scene, path, action));
    }
    assert_eq!(actual.get("position"), Ok(expected.get_position().into()));
    assert_eq!(actual.get("upper"), Ok(expected.get_upper().into()));

    actual
        .on("limit", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner
                        .set("opened", false.into())
                        .map_err(|error| error.to_string())?;
                }
                Ok(Some(100.0.into()))
            })
        })
        .unwrap();
    expected.on_limit({
        let owner = expected.as_weak();
        move || {
            owner.upgrade().unwrap().set_opened(false);
            100.0
        }
    });
    assert!(!actual.adjust_range_at(&actual_scene, &slider, action));
    assert!(!expected.adjust_range_at(&expected_scene, &slider, action));
    assert_eq!(actual.get("opened"), Ok(false.into()));
    assert!(!expected.get_opened());
    assert_eq!(actual.get("amount"), Ok(20.0.into()));
    assert_eq!(expected.get_amount(), 20.0);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn split_resize_rechecks_parent_overlay_for_both_resize_modes() {
    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RoutedRangeParity")
        .unwrap();
    let mut expected = expressions_ui::RoutedRangeParity::new();
    let actual_scene = actual.render_shared();
    let expected_scene = expected.render_shared();
    let weights = [(0, 2.0), (1, 1.0)];
    let sizes = [(0, 110.0), (1, 210.0)];

    actual.set("opened", false.into()).unwrap();
    expected.set_opened(false);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        assert!(!ui.resize_split_at(scene, &[0, 1], &weights));
        assert!(!ui.resize_split_sizes_at(scene, &[0, 2], &sizes));
    }
    actual.set("opened", true.into()).unwrap();
    expected.set_opened(true);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, &*actual_scene),
        (&mut expected, &*expected_scene),
    ] {
        assert!(ui.resize_split_at(scene, &[0, 1], &weights));
        assert!(ui.resize_split_sizes_at(scene, &[0, 2], &sizes));
        assert!(!ui.resize_split_at(scene, &[0, 1], &weights));
        assert!(!ui.resize_split_sizes_at(scene, &[0, 2], &sizes));
    }
    assert_eq!(actual.get("split-edits"), Ok(2_i64.into()));
    assert_eq!(expected.get_split_edits(), 2);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn typed_wheel_routes_share_coordinates_modifiers_and_acceptance() {
    use layout_ui::WheelDemo;
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::{EventResult, KeyModifiers, Length, WheelEvent};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/layout/layout.xr"),
        "layout.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("WheelDemo")
        .unwrap();
    let mut expected = WheelDemo::new();

    fn wheel(ui: &mut impl Component, control: bool) -> EventResult {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(460.0, 300.0));
        input.pointer_move(&scene, &layout, Point { x: 80.0, y: 100.0 });
        input
            .wheel_route(
                &scene,
                &layout,
                WheelEvent {
                    delta_y: Length(-28.0),
                    lines_y: -1.0,
                    is_line: true,
                    modifiers: KeyModifiers {
                        control,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap()
            .dispatch(ui, &scene)
    }

    assert_eq!(wheel(&mut actual, false), EventResult::Reject);
    assert_eq!(wheel(&mut expected, false), EventResult::Reject);
    assert_eq!(actual.get("inner-events").unwrap(), Value::Int(1));
    assert_eq!(actual.get("outer-events").unwrap(), Value::Int(1));
    assert_eq!(wheel(&mut actual, true), EventResult::Accept);
    assert_eq!(wheel(&mut expected, true), EventResult::Accept);
    assert_eq!(actual.get("outer-events").unwrap(), Value::Int(1));
    assert_eq!(actual.get("last-lines").unwrap(), Value::Float(-1.0));
    assert_eq!(
        actual.get("last-x").unwrap(),
        Value::Length(expected.get_last_x())
    );
    assert_eq!(
        actual.get("last-y").unwrap(),
        Value::Length(expected.get_last_y())
    );
    assert_eq!(
        actual.get("zoom").unwrap(),
        Value::Float(expected.get_zoom())
    );
    assert!(expected.get_zoom() > 1.0);
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

fn interpret_layout_fixture(name: &str) -> Instance {
    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/layout/layout.xr"),
        "layout.xr",
        Default::default(),
    )
    .unwrap();
    Program::from_module(&preview.module)
        .unwrap()
        .create(name)
        .unwrap()
}

#[test]
fn hover_observers_publish_nested_state_before_callbacks_in_both_modes() {
    use layout_ui::HoverDemo;
    use xrack_layout::{HoverObservers, Interaction, Point, Size};
    use xrack_runtime::CursorShape;

    let mut actual = interpret_layout_fixture("HoverDemo");
    let mut expected = HoverDemo::new();
    let mut actual_hover = HoverObservers::default();
    let mut expected_hover = HoverObservers::default();
    let point = Point { x: 70.0, y: 128.0 };

    fn sync(ui: &mut impl Component, observers: &mut HoverObservers, point: Option<Point>) {
        let input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(420.0, 240.0));
        for event in observers.sync(&input, &scene, &layout, point) {
            event.apply(ui, &scene);
        }
    }

    sync(&mut actual, &mut actual_hover, Some(point));
    sync(&mut expected, &mut expected_hover, Some(point));
    assert_eq!(actual_hover.cursor_shape(), CursorShape::Pointer);
    assert_eq!(actual_hover.cursor_shape(), expected_hover.cursor_shape());
    assert_eq!(actual.get("both-on-entry").unwrap(), Value::Bool(true));
    let next = Some(Point { x: 90.0, y: 132.0 });
    sync(&mut actual, &mut actual_hover, next);
    sync(&mut expected, &mut expected_hover, next);
    assert_eq!(actual.get("button-moves").unwrap(), Value::Int(1));
    actual.set("block-inner", true.into()).unwrap();
    expected.set_block_inner(true);
    sync(&mut actual, &mut actual_hover, next);
    sync(&mut expected, &mut expected_hover, next);
    assert_eq!(actual.get("area-hovered").unwrap(), Value::Bool(false));
    assert_eq!(actual.get("area-exits").unwrap(), Value::Int(1));
    actual.set("inner-enabled", false.into()).unwrap();
    expected.set_inner_enabled(false);
    sync(&mut actual, &mut actual_hover, next);
    sync(&mut expected, &mut expected_hover, next);
    assert_eq!(actual_hover.cursor_shape(), CursorShape::Crosshair);
    assert_eq!(actual.get("button-exits").unwrap(), Value::Int(1));
    sync(&mut actual, &mut actual_hover, None);
    sync(&mut expected, &mut expected_hover, None);
    assert_eq!(actual.get("area-exits").unwrap(), Value::Int(2));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn pointer_sequences_publish_release_before_click_and_keep_touch_rows_live() {
    use layout_ui::PointerDemo;
    use std::time::Instant;
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::{KeyModifiers, PointerEvent, PointerEventKind, TouchPoints};

    let mut actual = interpret_layout_fixture("PointerDemo");
    let mut expected = PointerDemo::new();

    fn exercise(
        ui: &mut impl Component,
        mut points: impl FnMut(&mut dyn Component, &[xrack_runtime::TouchPoint]),
    ) {
        let input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(420.0, 270.0));
        let point = Point { x: 70.0, y: 92.0 };
        let moved = Point { x: 85.0, y: 102.0 };
        let now = Instant::now();
        let modifiers = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        let event = PointerEvent {
            modifiers,
            ..Default::default()
        };
        let (mut mouse, down) = input
            .begin_pointer_sequence(&scene, &layout, point, event, now)
            .unwrap();
        let target = mouse.target().path.clone();
        assert!(down.apply(ui, &scene));
        assert!(
            mouse
                .sample(
                    &input,
                    &scene,
                    &layout,
                    moved,
                    PointerEventKind::Move,
                    modifiers
                )
                .unwrap()
                .apply(ui, &scene)
        );
        assert!(
            mouse
                .sample(
                    &input,
                    &scene,
                    &layout,
                    moved,
                    PointerEventKind::Up,
                    modifiers
                )
                .unwrap()
                .apply(ui, &scene)
        );
        assert!(ui.dispatch_at(&scene, &target, "clicked"));

        let (first, down) = input
            .begin_pointer_sequence(
                &scene,
                &layout,
                point,
                PointerEvent {
                    is_touch: true,
                    touch_finger_id: 1,
                    ..event
                },
                now,
            )
            .unwrap();
        assert!(down.apply(ui, &scene));
        let (mut second, down) = input
            .begin_pointer_sequence(
                &scene,
                &layout,
                moved,
                PointerEvent {
                    is_touch: true,
                    touch_finger_id: 2,
                    ..event
                },
                now,
            )
            .unwrap();
        assert!(down.apply(ui, &scene));
        let handle = scene
            .at_path(&target)
            .unwrap()
            .pointer_handle
            .clone()
            .unwrap();
        let model: TouchPoints = handle.touch_points();
        let both = model.snapshot();
        assert_eq!(both.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
        points(ui, &both);
        assert!(first.cancel(modifiers).apply(ui, &scene));
        let remaining = model.snapshot();
        assert_eq!(remaining.iter().map(|p| p.id).collect::<Vec<_>>(), [2]);
        points(ui, &remaining);
        assert!(
            second
                .sample(
                    &input,
                    &scene,
                    &layout,
                    moved,
                    PointerEventKind::Up,
                    modifiers
                )
                .unwrap()
                .apply(ui, &scene)
        );
        assert!(model.snapshot().is_empty());
        points(ui, &[]);
    }

    let mut actual_states = Vec::new();
    let mut expected_states = Vec::new();
    let capture =
        |states: &mut Vec<_>, ui: &mut dyn Component, points: &[xrack_runtime::TouchPoint]| {
            let scene = ui.render();
            let marks = scene.at_path(&[1, 0, 0, 0, 1]).unwrap();
            assert_eq!(marks.children.len(), points.len());
            states.push((points.to_vec(), marks.clone()));
        };
    exercise(&mut actual, |ui, points| {
        capture(&mut actual_states, ui, points)
    });
    exercise(&mut expected, |ui, points| {
        capture(&mut expected_states, ui, points)
    });
    assert_eq!(actual_states.len(), expected_states.len());
    for ((actual_points, actual_scene), (expected_points, expected_scene)) in
        actual_states.iter().zip(&expected_states)
    {
        assert_eq!(actual_points, expected_points);
        assert_same_scene(actual_scene, expected_scene);
    }
    assert_eq!(actual.get("click-saw-up").unwrap(), Value::Bool(true));
    assert_eq!(
        actual.get("moves").unwrap(),
        Value::Int(expected.get_moves())
    );
    assert_eq!(actual.get("active-touches").unwrap(), Value::Int(0));
    assert_eq!(actual.get("last-finger-id").unwrap(), Value::Int(2));
    assert_eq!(actual.get("shift-held").unwrap(), Value::Bool(true));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn accepted_buttons_pass_primary_clicks_through_to_lower_controls() {
    use layout_ui::PointerPassThroughDemo;
    use std::time::Instant;
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::{KeyModifiers, PointerButton, PointerEvent, PointerEventKind};

    let mut actual = interpret_layout_fixture("PointerPassThroughDemo");
    let mut expected = PointerPassThroughDemo::new();
    actual.set("secondary-only", true.into()).unwrap();
    expected.set_secondary_only(true);
    fn exercise(ui: &mut impl Component) {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(420.0, 210.0));
        let point = Point { x: 65.0, y: 101.0 };
        let now = Instant::now();
        assert!(
            input
                .begin_pointer_sequence(&scene, &layout, point, PointerEvent::default(), now)
                .is_none()
        );
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));

        let event = PointerEvent {
            button: PointerButton::Secondary,
            ..Default::default()
        };
        let (mut secondary, down) = input
            .begin_pointer_sequence(&scene, &layout, point, event, now)
            .unwrap();
        assert!(down.apply(ui, &scene));
        let up = secondary
            .sample(
                &input,
                &scene,
                &layout,
                point,
                PointerEventKind::Up,
                KeyModifiers::default(),
            )
            .unwrap();
        assert!(up.apply(ui, &scene));
    }
    exercise(&mut actual);
    exercise(&mut expected);
    assert_eq!(actual.get("lower-clicks").unwrap(), Value::Int(1));
    assert_eq!(actual.get("overlay-clicks").unwrap(), Value::Int(0));
    assert_eq!(actual.get("secondary-releases").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn swipe_metrics_and_cancellation_match_generated_ui() {
    use std::time::Instant;
    use xrack_layout::{Interaction, Point, ScrollGestureKind, Size};
    use xrack_runtime::Length;

    fn swipe(
        ui: &mut impl Component,
        input: &mut Interaction,
        finish: bool,
    ) -> (std::rc::Rc<Node>, xrack_layout::LayoutNode) {
        let (scene, layout) = prepare_list(ui, input, Size::new(360.0, 280.0));
        let start = Point { x: 120.0, y: 105.0 };
        let end = Point { x: 180.0, ..start };
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(
            &scene,
            &layout,
            start,
            now,
            ScrollGestureKind::MouseControl
        ));
        input.move_scroll_gesture(&scene, &layout, end, now);
        for event in input.take_swipe_events() {
            event.apply(ui, &scene);
        }
        if finish {
            let outcome = input.end_scroll_gesture(&scene, &layout, end, now);
            assert!(!outcome.tap);
            for event in input.take_swipe_events() {
                event.apply(ui, &scene);
            }
        }
        (scene, layout)
    }

    let mut actual = interpret_layout_fixture("SwipeGestureDemo");
    let mut expected = layout_ui::SwipeGestureDemo::new();
    let mut input = Interaction::default();
    let mut other = Interaction::default();
    swipe(&mut actual, &mut input, true);
    swipe(&mut expected, &mut other, true);
    assert_eq!(actual.get("swipes").unwrap(), Value::Int(1));
    assert_eq!(actual.get("shift").unwrap(), Value::Length(Length(60.0)));
    assert_eq!(expected.get_shift(), Length(60.0));
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());

    let (scene, layout) = swipe(&mut actual, &mut input, false);
    let (generated, generated_layout) = swipe(&mut expected, &mut other, false);
    assert_eq!(actual.get("swiping").unwrap(), Value::Bool(true));
    assert!(expected.get_swiping());
    actual.invoke("cancel-swipe", &[]).unwrap();
    expected.invoke_cancel_swipe();
    input.reconcile_swipe(&scene, &layout);
    other.reconcile_swipe(&generated, &generated_layout);
    for event in input.take_swipe_events() {
        event.apply(&mut actual, &scene);
    }
    for event in other.take_swipe_events() {
        event.apply(&mut expected, &generated);
    }
    assert_eq!(actual.get("swiping").unwrap(), Value::Bool(false));
    assert_eq!(actual.get("cancellations").unwrap(), Value::Int(1));
    assert_eq!(expected.get_cancellations(), 1);
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());
}

#[test]
fn scale_rotate_uses_local_centroid_and_resets_after_end_in_both_modes() {
    use xrack_layout::{
        Interaction, Point, Size,
        scale_rotate_gestures::{ContactId, Context, Gestures, TouchPhase},
    };

    fn pinch(ui: &mut impl Component) -> std::rc::Rc<Node> {
        let input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(360.0, 280.0));
        let context = Context {
            scene: &scene,
            layout: &layout,
            input: &input,
        };
        let mut gestures = Gestures::default();
        for (finger, phase, point, consumed) in [
            (1, TouchPhase::Started, Point { x: 100.0, y: 100.0 }, false),
            (2, TouchPhase::Started, Point { x: 160.0, y: 100.0 }, true),
            (2, TouchPhase::Moved, Point { x: 100.0, y: 190.0 }, true),
        ] {
            let (used, events) = gestures.touch(&context, ContactId((), finger), phase, point);
            assert_eq!(used, consumed);
            for event in events.into_iter().flatten() {
                assert!(event.apply(ui, &scene));
            }
        }
        let active = ui.render_shared();
        let (used, events) = gestures.touch(
            &context,
            ContactId((), 2),
            TouchPhase::Ended,
            Point { x: 100.0, y: 190.0 },
        );
        assert!(used);
        for event in events.into_iter().flatten() {
            assert!(event.apply(ui, &scene));
        }
        active
    }

    let mut actual = interpret_layout_fixture("ScaleRotateDemo");
    let mut expected = layout_ui::ScaleRotateDemo::new();
    assert_same_scene(&pinch(&mut actual), &pinch(&mut expected));
    assert_eq!(actual.get("zoom").unwrap(), Value::Float(1.5));
    assert_eq!(actual.get("angle").unwrap(), Value::Float(90.0));
    assert_eq!(actual.get("recognizing").unwrap(), Value::Bool(false));
    assert_eq!(actual.get("ends").unwrap(), Value::Int(1));
    assert_eq!(expected.get_ends(), 1);
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());
}

#[test]
fn swipe_delegate_updates_a_two_way_row_property_and_dispatches_the_exposed_button() {
    use xrack_layout::{Interaction, Size};
    use xrack_runtime::{ControlAction, SwipeSide};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/delegates/delegates.xr"),
        "delegates.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("SwipeDemo")
        .unwrap();
    let mut expected = delegates_ui::SwipeDemo::new();
    actual
        .set("messages", Value::StringList(vec!["Inbox".into()]))
        .unwrap();
    expected.set_messages(vec!["Inbox".into()]);

    fn archive(ui: &mut impl Component) -> std::rc::Rc<Node> {
        let input = Interaction::default();
        let size = Size::new(560.0, 370.0);
        let row = [0, 2, 0, 0];
        let button = [0, 2, 0, 0, 2, 0];
        let (closed, _) = prepare_list(ui, &input, size);
        assert!(ui.control_at(
            &closed,
            &row,
            ControlAction::SwipeDelegate(SwipeSide::Right)
        ));
        assert!(!ui.control_at(&closed, &row, ControlAction::SwipeDelegate(SwipeSide::Left)));
        let (opened, _) = prepare_list(ui, &input, size);
        assert_eq!(opened.at_path(&row).unwrap().swipe_side(), SwipeSide::Right);
        assert!(ui.dispatch_at(&opened, &button, "clicked"));
        let (archived, _) = prepare_list(ui, &input, size);
        assert_eq!(
            archived.at_path(&row).unwrap().swipe_side(),
            SwipeSide::Closed
        );
        archived
    }

    assert_same_scene(&archive(&mut actual), &archive(&mut expected));
}

#[test]
fn internal_and_file_drag_events_match_generated_components() {
    use std::sync::Arc;
    use xrack_layout::{FileDragInput, Interaction, Point, Size};
    use xrack_runtime::DragAction;

    fn drop_card(ui: &mut impl Component) {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(560.0, 258.0));
        let target = Point { x: 420.0, y: 117.0 };
        input.arm_drag(&scene, &layout, Point { x: 102.0, y: 117.0 });
        let action = Some(DragAction::Copy);
        let moved = input.drag_motion(&scene, &layout, target, action);
        assert!(moved.captured);
        for event in moved.events {
            assert!(event.apply(ui, &scene));
        }
        let done = input.finish_drag(&scene, &layout, target, action);
        for event in done.events {
            assert!(event.apply(ui, &scene));
        }
    }

    fn drop_files(ui: &mut impl Component, paths: Arc<[String]>) {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_list(ui, &input, Size::new(536.0, 310.0));
        let point = Point { x: 120.0, y: 110.0 };
        let scope = input.file_drop_scope(&layout);
        let target = scope.target(&scene, &layout, point, &paths).unwrap();
        for command in [
            FileDragInput::Moved {
                sequence: 1,
                point,
                paths: paths.clone(),
            },
            FileDragInput::Dropped {
                sequence: 1,
                point,
                paths,
                target,
            },
        ] {
            for event in input.file_drag_event(&scene, &layout, command).events {
                assert!(event.apply(ui, &scene));
            }
        }
    }

    let mut actual = interpret_form_fixture("DragDropDemo");
    let mut expected = form_ui::DragDropDemo::new();
    drop_card(&mut actual);
    drop_card(&mut expected);
    assert_eq!(
        actual.get("trace").unwrap(),
        Value::String("start-enter-drop-finish".into())
    );
    assert_eq!(
        actual.get("received").unwrap(),
        Value::String(expected.get_received())
    );
    assert_eq!(
        actual.get("result").unwrap(),
        Value::DragAction(expected.get_result())
    );
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());

    let mut actual = interpret_form_fixture("FileDropDemo");
    let mut expected = form_ui::FileDropDemo::new();
    let paths: Arc<[String]> = [std::env::temp_dir()
        .join("photo.png")
        .to_str()
        .unwrap()
        .into()]
    .into();
    drop_files(&mut actual, paths.clone());
    drop_files(&mut expected, paths.clone());
    assert_eq!(actual.get("batches").unwrap(), Value::Int(1));
    assert_eq!(
        actual.get("paths").unwrap(),
        Value::StringList(paths.to_vec())
    );
    assert_eq!(expected.get_paths(), paths.as_ref());
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());
}

#[test]
fn virtual_drop_delivers_only_to_the_row_that_owns_the_capture() {
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::DragEvent;

    fn first_drop(ui: &mut impl Component, reorder: impl FnOnce()) -> std::rc::Rc<Node> {
        let mut input = Interaction::default();
        let size = Size::new(360.0, 180.0);
        let (scene, layout) = prepare_list(ui, &input, size);
        let point = Point { x: 194.0, y: 32.0 };
        input.arm_drag(&scene, &layout, Point { x: 68.0, y: 34.0 });
        assert!(input.drag_motion(&scene, &layout, point, None).captured);
        let event = input
            .finish_drag(&scene, &layout, point, None)
            .events
            .into_iter()
            .find(|event| matches!(event.event, DragEvent::Dropped { .. }))
            .unwrap();
        let target = event.target.path.clone();
        assert!(event.target.row_key.is_some());
        assert!(ui.drag_event_at(&scene, &target, &event.event));
        let (received, _) = prepare_list(ui, &input, size);
        assert_eq!(
            received.at_path(&target).unwrap().children[0].text("text"),
            Some("old:ticket")
        );
        reorder();
        let (reordered, _) = prepare_list(ui, &input, size);
        assert!(!event.apply(ui, &reordered));
        assert_eq!(
            reordered.at_path(&target).unwrap().children[0].text("text"),
            Some("new:")
        );
        reordered
    }

    let mut actual = interpret_form_fixture("VirtualDragDemo");
    let mut expected = form_ui::VirtualDragDemo::new();
    let interpreted = actual.string_model("items").unwrap();
    let generated = expected.model_items();
    let left = first_drop(&mut actual, || {
        assert!(interpreted.sort_by(|a, b| a.cmp(b)))
    });
    let right = first_drop(&mut expected, || {
        assert!(generated.sort_by(|a, b| a.cmp(b)))
    });
    assert_same_scene(&left, &right);
}

#[test]
fn a_swipe_delegate_without_a_binding_keeps_its_open_side() {
    use xrack_runtime::{ControlAction, SwipeSide};

    let source = r#"
        export component Inbox inherits Window {
            row := SwipeDelegate {
                SwipeContent {}
                SwipeRight { width: 60px; Button { text: "Archive"; } }
            }
        }
    "#;
    let preview = xrack_compiler::prepare_preview(source, "inbox.xr", Default::default()).unwrap();
    let mut ui = Program::from_module(&preview.module)
        .unwrap()
        .create("Inbox")
        .unwrap();
    let closed = ui.try_render().unwrap();
    assert!(ui.control_at(
        &closed,
        &[0],
        ControlAction::SwipeDelegate(SwipeSide::Right)
    ));
    let opened = ui.try_render().unwrap();
    assert_eq!(opened.children[0].swipe_side(), SwipeSide::Right);
    assert!(ui.control_at(
        &opened,
        &[0],
        ControlAction::SwipeDelegate(SwipeSide::Closed)
    ));
    assert_eq!(
        ui.try_render().unwrap().children[0].swipe_side(),
        SwipeSide::Closed
    );
}

#[test]
fn clipboard_named_input_and_table_projection_keep_their_own_bindings() {
    use xrack_layout::{Interaction, Size};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("ClipboardBindingParity")
        .unwrap();
    let mut generated = expressions_ui::ClipboardBindingParity::new();
    assert_eq!(generated.get_observed(), 42);
    assert_eq!(interpreted.get("observed"), Ok(42_i64.into()));

    interpreted.set("source", 63_i64.into()).unwrap();
    generated.set_source(63);
    assert_eq!(interpreted.get("observed"), Ok(63_i64.into()));
    assert_eq!(generated.get_observed(), 63);
    let input = Interaction::default();
    let size = Size::new(400.0, 280.0);
    let (actual, _) = prepare_list(&interpreted, &input, size);
    let (expected, _) = prepare_list(&generated, &input, size);
    assert_same_scene(&actual, &expected);
    assert_eq!(
        actual.at_path(&[0, 0]).unwrap().text("text"),
        Some("stored 63")
    );
    let cell = [1, 0, 0, 0, 0];
    assert_eq!(
        actual.at_path(&cell).unwrap().text("text"),
        Some("stored 7")
    );
    let nested = [1, 0, 0, 1, 0, 0];
    assert_eq!(
        actual.at_path(&nested).unwrap().text("text"),
        Some("stored 63")
    );
    let table = &[1];
    for (ui, scene) in [
        (&interpreted as &dyn Component, &actual),
        (&generated, &expected),
    ] {
        assert_eq!(
            ui.copy_table_at(scene, table, &[0]).as_deref(),
            Some("copy projection")
        );
    }
    assert_eq!(interpreted.take_error(), None);

    let source = include_str!("expressions.xr").replacen(
        "child := ClipboardNamedContent { clipboard-text: root.source; }",
        "child := ClipboardNamedContent { clipboard-text: root.source; copy-value: 99; }",
        1,
    );
    let filename = "expressions.xr";
    let expected = "instance binds multiple inputs to the same property";
    let error = xrack_compiler::compile(&source, filename)
        .err()
        .expect("duplicate alias accepted");
    assert!(error.to_string().contains(expected), "{error}");
    let preview = xrack_compiler::prepare_preview(&source, filename, Default::default()).unwrap();
    let error = Program::from_module(&preview.module)
        .and_then(|program| program.create("ClipboardBindingParity"))
        .err()
        .expect("duplicate alias accepted by the interpreter");
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn table_clipboard_projection_is_lazy_in_both_modes() {
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Interaction, Size};

    let source = include_str!("expressions.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "expressions.xr", Default::default()).unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ClipboardProbe")
        .unwrap();
    let mut expected = expressions_ui::ClipboardProbe::new();
    let actual_calls = Rc::new(Cell::new(0));
    let expected_calls = Rc::new(Cell::new(0));
    actual
        .on("format-copy", {
            let calls = actual_calls.clone();
            Rc::new(move |arguments| {
                let [Value::String(text)] = arguments else {
                    panic!("expected cell text");
                };
                calls.set(calls.get() + 1);
                Ok(Some(Value::String(format!("copy: {text}"))))
            })
        })
        .unwrap();
    expected.on_format_copy({
        let calls = expected_calls.clone();
        move |text| {
            calls.set(calls.get() + 1);
            format!("copy: {text}")
        }
    });
    let size = Size::new(320.0, 180.0);
    let input = Interaction::default();
    let (left, _) = prepare_list(&actual, &input, size);
    let (right, _) = prepare_list(&expected, &input, size);
    assert_same_scene(&left, &right);
    assert_eq!((actual_calls.get(), expected_calls.get()), (0, 0));
    assert_eq!(
        actual.copy_table_at(&left, &[0], &[0]),
        Some("copy: item".into())
    );
    assert_eq!(
        expected.copy_table_at(&right, &[0], &[0]),
        Some("copy: item".into())
    );
    assert_eq!((actual_calls.get(), expected_calls.get()), (1, 1));
}

#[test]
fn table_copy_and_navigation_recheck_the_live_view() {
    use expressions_ui::TableReadViewParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{ControlAction, RecordList, TableEditPosition};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableReadViewParity")
        .unwrap();
    let mut generated = TableReadViewParity::new();
    let path = [0];
    let scene = actual.render_shared();
    let compiled_scene = generated.render_shared();
    let Value::RecordList(rows) = actual.get("rows").unwrap() else {
        panic!("expected rows");
    };
    let key = rows.model().row_key(0).unwrap();
    let compiled_key = generated.model_rows().row_key(0).unwrap();
    let read = |ui: &dyn Component, scene: &Node, key, columns: &[usize]| {
        let next = ui.next_table_edit_cell(
            scene,
            &path,
            TableEditPosition {
                row: 0,
                column: columns[0],
                key,
            },
            columns,
            false,
        );
        (
            ui.copy_table_at(scene, &path, columns),
            next.map(|position| (position.row, position.column)),
        )
    };
    let views = [
        (&actual as &dyn Component, &*scene, key),
        (&generated as &dyn Component, &*compiled_scene, compiled_key),
    ];
    for (ui, view, key) in views {
        assert_eq!(
            read(ui, view, key, &[0, 1]),
            (Some("one\ttwo".into()), Some((0, 1)))
        );
    }

    actual.set("copy-enabled", false.into()).unwrap();
    actual.set("tab-enabled", false.into()).unwrap();
    generated.set_copy_enabled(false);
    generated.set_tab_enabled(false);
    assert_eq!(read(&actual, &scene, key, &[0, 1]), (None, None));
    assert_eq!(
        read(&generated, &compiled_scene, compiled_key, &[0, 1]),
        (None, None)
    );
    actual.set("copy-enabled", true.into()).unwrap();
    actual.set("tab-enabled", true.into()).unwrap();
    generated.set_copy_enabled(true);
    generated.set_tab_enabled(true);

    actual.set("table-enabled", false.into()).unwrap();
    generated.set_table_enabled(false);
    assert_eq!(read(&actual, &scene, key, &[0, 1]), (None, None));
    assert_eq!(
        read(&generated, &compiled_scene, compiled_key, &[0, 1]),
        (None, None)
    );
    actual.set("table-enabled", true.into()).unwrap();
    generated.set_table_enabled(true);

    for (name, flag) in [
        ("format-copy", "copy-enabled"),
        ("allow-edit", "tab-enabled"),
    ] {
        let owner = Cell::new(Some(actual.clone()));
        actual
            .on(
                name,
                Rc::new(move |args| {
                    if let Some(owner) = owner.take() {
                        owner.set(flag, false.into()).unwrap();
                    }
                    Ok(Some(args.first().cloned().unwrap_or_else(|| true.into())))
                }),
            )
            .unwrap();
    }
    generated.on_format_copy({
        let owner = generated.as_weak();
        move |text| {
            owner.upgrade().unwrap().set_copy_enabled(false);
            text
        }
    });
    generated.on_allow_edit({
        let owner = generated.as_weak();
        move || {
            owner.upgrade().unwrap().set_tab_enabled(false);
            true
        }
    });
    assert_eq!(read(&actual, &scene, key, &[0, 1]), (None, None));
    assert_eq!(
        read(&generated, &compiled_scene, compiled_key, &[0, 1]),
        (None, None)
    );
    assert_eq!(actual.get("copy-enabled"), Ok(false.into()));
    assert_eq!(actual.get("tab-enabled"), Ok(false.into()));
    assert!(!generated.get_copy_enabled());
    assert!(!generated.get_tab_enabled());

    actual
        .on("format-copy", Rc::new(|args| Ok(args.first().cloned())))
        .unwrap();
    actual
        .on("allow-edit", Rc::new(|_| Ok(Some(true.into()))))
        .unwrap();
    generated.on_format_copy(|text| text);
    generated.on_allow_edit(|| true);
    actual.set("copy-enabled", true.into()).unwrap();
    actual.set("tab-enabled", true.into()).unwrap();
    generated.set_copy_enabled(true);
    generated.set_tab_enabled(true);
    let move_column = ControlAction::MoveColumn { from: 0, to: 1 };
    assert!(actual.control_at(&scene, &path, move_column));
    assert!(generated.control_at(&compiled_scene, &path, move_column));
    assert_eq!(read(&actual, &scene, key, &[0, 1]), (None, None));
    assert_eq!(
        read(&generated, &compiled_scene, compiled_key, &[0, 1]),
        (None, None)
    );
    let reordered = actual.render_shared();
    let compiled_reordered = generated.render_shared();
    assert_eq!(
        read(&actual, &reordered, key, &[1, 0]),
        (Some("two\tone".into()), Some((0, 0)))
    );
    assert_eq!(
        read(&generated, &compiled_reordered, compiled_key, &[1, 0]),
        (Some("two\tone".into()), Some((0, 0)))
    );
    actual
        .set(
            "rows",
            Value::RecordList(RecordList::new(
                "HistoryCallbackRow",
                rows.model().snapshot(),
            )),
        )
        .unwrap();
    generated.set_rows(generated.get_rows());
    assert_eq!(read(&actual, &reordered, key, &[1, 0]), (None, None));
    assert_eq!(
        read(&generated, &compiled_reordered, compiled_key, &[1, 0]),
        (None, None)
    );
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_copy_aborts_if_a_cell_projection_changes_the_model() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{ControlAction, RecordList, RowSelection, TableSelectionAction, VecModel};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let program = Program::from_module(&preview.module).unwrap();
    let read_rows = |ui: &Instance| {
        let Value::RecordList(rows) = ui.get("records").unwrap() else {
            panic!("expected rows");
        };
        rows
    };
    let copy = |ui: &dyn Component, scene: &Node| ui.copy_table_at(scene, &[0], &[0, 1]);
    for replace in [true, false] {
        let mut interpreted = program.create("ClipboardProbe").unwrap();
        let mut generated = expressions_ui::ClipboardProbe::new();
        let actual = interpreted.render_shared();
        let expected = generated.render_shared();
        let rows = read_rows(&interpreted);
        let compiled_rows = generated.model_records();
        let actual_calls = Rc::new(Cell::new(0));
        let compiled_calls = Rc::new(Cell::new(0));

        interpreted
            .on("format-copy", {
                let owner = Cell::new(Some(interpreted.clone()));
                let calls = actual_calls.clone();
                Rc::new(move |args| {
                    calls.set(calls.get() + 1);
                    if let Some(owner) = owner.take() {
                        let row = rows
                            .model()
                            .row(0)
                            .unwrap()
                            .with_field(&["text".into()], Value::String("updated".into()))
                            .unwrap();
                        if replace {
                            let replacement = RecordList::new("ClipboardProbeRow", vec![row]);
                            owner
                                .set("records", Value::RecordList(replacement))
                                .unwrap();
                        } else {
                            assert!(rows.model().set_row(0, row));
                        }
                    }
                    Ok(args.first().cloned())
                })
            })
            .unwrap();
        generated.on_format_copy({
            let owner = generated.as_weak();
            let calls = compiled_calls.clone();
            move |text| {
                let count = calls.get();
                calls.set(count + 1);
                if count == 0 {
                    let mut row = compiled_rows.row(0).unwrap();
                    row.text = "updated".into();
                    if replace {
                        owner
                            .upgrade()
                            .unwrap()
                            .bind_model_records(VecModel::from(vec![row]));
                    } else {
                        assert!(compiled_rows.set_row(0, row));
                    }
                }
                text
            }
        });
        assert_eq!(
            [copy(&interpreted, &actual), copy(&generated, &expected)],
            [None, None]
        );
        assert_eq!((actual_calls.get(), compiled_calls.get()), (1, 1));
        assert_eq!(
            read_rows(&interpreted)
                .model()
                .row(0)
                .unwrap()
                .field("text"),
            Some(&Value::String("updated".into()))
        );
        assert_eq!(generated.model_records().row(0).unwrap().text, "updated");

        interpreted
            .on("format-copy", Rc::new(|args| Ok(args.first().cloned())))
            .unwrap();
        generated.on_format_copy(|text| text);
        for ui in [&mut interpreted as &mut dyn Component, &mut generated] {
            if replace {
                let scene = ui.render_shared();
                let select = ControlAction::TableSelection(TableSelectionAction::Row {
                    index: 0,
                    operation: RowSelection::Replace,
                });
                assert!(ui.control_at(&scene, &[0], select));
            }
            assert_eq!(
                copy(ui, &ui.render_shared()).as_deref(),
                Some("updated\tupdated next")
            );
        }
        assert_eq!(interpreted.take_error(), None);
    }
}

#[test]
fn table_copy_stops_at_the_first_projection_error_and_can_retry() {
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ClipboardProbe")
        .unwrap();
    let mut generated = expressions_ui::ClipboardProbe::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("format-copy", {
            let calls = actual_calls.clone();
            Rc::new(move |arguments| {
                let [Value::String(text)] = arguments else {
                    panic!("expected cell text");
                };
                calls.borrow_mut().push(text.clone());
                Err(format!("cannot copy {text}"))
            })
        })
        .unwrap();
    generated.on_format_copy({
        let calls = generated_calls.clone();
        move |text| {
            calls.borrow_mut().push(text.clone());
            panic!("cannot copy {text}")
        }
    });
    let left = actual.render_shared();
    let right = generated.render_shared();
    let columns = [0, 1];
    assert!(
        catch_unwind(AssertUnwindSafe(|| generated.copy_table_at(
            &right,
            &[0],
            &columns
        )))
        .is_err()
    );
    assert_eq!(actual.copy_table_at(&left, &[0], &columns), None);
    assert_eq!(&*generated_calls.borrow(), &["item"]);
    assert_eq!(&*actual_calls.borrow(), &["item"]);
    assert_eq!(
        actual.take_error(),
        Some(Error::Evaluation("cannot copy item".into()))
    );

    actual
        .on(
            "format-copy",
            Rc::new(|arguments| Ok(arguments.first().cloned())),
        )
        .unwrap();
    generated.on_format_copy(|text| text);
    let expected = Some("item\titem next".to_owned());
    assert_eq!(actual.copy_table_at(&left, &[0], &columns), expected);
    assert_eq!(generated.copy_table_at(&right, &[0], &columns), expected);
    assert_eq!(actual.take_error(), None);

    // Changing the projection itself must also stop the current copy.
    actual
        .on("format-copy", {
            let owner = std::cell::Cell::new(Some(actual.clone()));
            Rc::new(move |args| {
                if let Some(owner) = owner.take() {
                    owner
                        .on(
                            "format-copy",
                            Rc::new(|_| Ok(Some(Value::String("new".into())))),
                        )
                        .unwrap();
                }
                Ok(args.first().cloned())
            })
        })
        .unwrap();
    generated.on_format_copy({
        let owner = generated.as_weak();
        move |text| {
            owner.upgrade().unwrap().on_format_copy(|_| "new".into());
            text
        }
    });
    assert_eq!(actual.copy_table_at(&left, &[0], &columns), None);
    assert_eq!(generated.copy_table_at(&right, &[0], &columns), None);
    let expected = Some("new\tnew".into());
    assert_eq!(actual.copy_table_at(&left, &[0], &columns), expected);
    assert_eq!(generated.copy_table_at(&right, &[0], &columns), expected);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_drag_autoscroll_copy_and_keyboard_selection_match_generated_ui() {
    use std::time::Instant;
    use xrack_layout::{Interaction, NavigationKey, Point, Size};
    use xrack_runtime::{KeyModifiers, RowSelection, SelectionTarget};

    let file = "examples/table/table.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/table/table.xr"),
        file,
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableDemo")
        .unwrap();
    let mut expected = table_ui::TableDemo::new();
    let items: Vec<_> = (0..100).map(|row| format!("Row {row}")).collect();
    actual
        .set("records", Value::StringList(items.clone()))
        .unwrap();
    actual
        .set("target", Value::SelectionTarget(SelectionTarget::Cells))
        .unwrap();
    expected.set_records(items);
    expected.set_target(SelectionTarget::Cells);

    fn exercise(ui: &mut impl Component) -> (f32, String, [usize; 4]) {
        let mut input = Interaction::default();
        let size = Size::new(560.0, 370.0);
        let path = [0, 2];
        let (mut scene, mut layout) = prepare_list(ui, &input, size);
        let table = layout.at_path(&path).unwrap();
        let geometry = table.table.as_ref().unwrap();
        let x = geometry.columns[0].x + 24.0;
        let start = Point {
            x,
            y: table.children[0].bounds.y + 16.0,
        };
        let end = Point {
            x,
            y: geometry.body.y + geometry.body.height - 2.0,
        };
        let visible = scene.at_path(&path).unwrap().children.len();
        input.pointer_move(&scene, &layout, start);
        assert!(input.press_table_drag_selection(&scene, &layout, false));
        input.pointer_move(&scene, &layout, end);
        assert!(input.move_table_drag_selection(&layout, Instant::now()));
        for _ in 0..8 {
            let deadline = input
                .table_drag_selection_deadline(&scene, &layout)
                .unwrap();
            assert!(input.advance_table_drag_selection(&scene, &layout, deadline));
            (scene, layout) = prepare_list(ui, &input, size);
            input.reconcile(&scene, &layout);
        }
        let offset = input.scroll_offsets()[&path[..]];
        assert!(offset > 0.0);
        let drag = input.finish_table_drag_selection(&scene, &layout).unwrap();
        assert!(ui.control_at(&scene, &drag.path, drag.action));
        (scene, layout) = prepare_list(ui, &input, size);
        input.reconcile(&scene, &layout);

        let count = |scene: &Node| {
            scene
                .at_path(&path)
                .unwrap()
                .table_selection
                .as_ref()
                .unwrap()
                .len()
        };
        let dragged = count(&scene);
        assert!(dragged > visible);
        let (table, columns) = input.table_copy_target(&scene, &layout).unwrap();
        let copied = ui.copy_table_at(&scene, &table, &columns).unwrap();
        assert!(copied.starts_with("Row 0\n"));
        assert_eq!(copied.lines().count(), dragged);

        let all = input
            .selection_key_action(
                &scene,
                &layout,
                "a",
                KeyModifiers {
                    control: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(ui.control_at(&scene, &all.path, all.action));
        (scene, layout) = prepare_list(ui, &input, size);
        let selected_all = count(&scene);
        assert_eq!(selected_all, 200);
        let clear = input
            .selection_key_action(&scene, &layout, "Escape", KeyModifiers::default())
            .unwrap();
        assert!(ui.control_at(&scene, &clear.path, clear.action));
        (scene, layout) = prepare_list(ui, &input, size);
        let cleared = count(&scene);
        assert_eq!(cleared, 0);
        let next = input
            .list_key_with_selection(&scene, &layout, NavigationKey::Down, RowSelection::Replace)
            .unwrap();
        assert!(ui.control_at(&scene, &next.path, next.action));
        let selected_next = count(&ui.render());
        assert_eq!(selected_next, 1);
        (
            offset,
            copied,
            [dragged, selected_all, cleared, selected_next],
        )
    }

    assert_eq!(exercise(&mut actual), exercise(&mut expected));
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());
}

#[test]
fn table_header_reorder_and_row_edge_scroll_preserve_the_same_records_in_both_modes() {
    use std::time::Instant;
    use table_ui::{InventoryRow, StandardTableDemo};
    use xrack_layout::{Interaction, Point, Size};
    use xrack_runtime::{ControlAction, Length, RecordList, RecordValue};

    let source = include_str!("../examples/table/table.xr");
    let preview = xrack_compiler::prepare_preview(source, "table.xr", Default::default()).unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("StandardTableDemo")
        .unwrap();
    let mut expected = StandardTableDemo::new();
    let records: Vec<_> = (0..100)
        .map(|row| InventoryRow {
            name: format!("Record {row}"),
            ..Default::default()
        })
        .collect();
    let dynamic = records
        .iter()
        .map(|row| {
            RecordValue::new(
                "InventoryRow",
                [
                    ("name".into(), Value::String(row.name.clone())),
                    ("units".into(), Value::Int(row.units)),
                    ("available".into(), Value::Bool(row.available)),
                    ("refreshed".into(), Value::Time(row.refreshed)),
                ]
                .into(),
            )
        })
        .collect();
    actual
        .set(
            "inventory",
            Value::RecordList(RecordList::new("InventoryRow", dynamic)),
        )
        .unwrap();
    actual.set("current", Value::Int(0)).unwrap();
    actual.set("move-rows", Value::Bool(true)).unwrap();
    actual
        .set("row-header-size", Value::Length(Length(36.0)))
        .unwrap();
    expected.set_inventory(records);
    expected.set_current(0);
    expected.set_move_rows(true);
    expected.set_row_header_size(Length(36.0));

    fn exercise(ui: &mut impl Component) -> (usize, f32, usize) {
        let mut input = Interaction::default();
        let size = Size::new(620.0, 380.0);
        let path = [0, 1];
        let (mut scene, mut layout) = prepare_list(ui, &input, size);
        let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
        let header = |index: usize, fraction: f32| Point {
            x: table.columns[index].x + table.columns[index].width * fraction,
            y: table.header.y + table.header.height / 2.0,
        };
        input.pointer_move(&scene, &layout, header(0, 0.5));
        assert!(input.press_table_reorder(&scene, &layout));
        input.pointer_move(&scene, &layout, header(1, 0.85));
        assert!(input.table_column_move_drag(&layout, Instant::now()));
        let column = input.finish_table_column_move(&scene, &layout).unwrap();
        assert_eq!(column.action, ControlAction::MoveColumn { from: 0, to: 1 });
        assert!(ui.control_at(&scene, &column.path, column.action));
        (scene, layout) = prepare_list(ui, &input, size);

        let table = scene.at_path(&path).unwrap();
        let columns = table.table_header.as_ref().unwrap().columns.len();
        let first = table.children[0].row_key.unwrap();
        let visual = table.table_column_order.visual_index(columns, 0).unwrap();
        assert_eq!(visual, 1);
        let item = layout.at_path(&path).unwrap();
        let gutter = item.table.as_ref().unwrap().row_header;
        let start = Point {
            x: gutter.x + 16.0,
            y: item.children[0].bounds.y + 16.0,
        };
        let edge = Point {
            y: gutter.y + gutter.height - 5.0,
            ..start
        };
        input.pointer_move(&scene, &layout, start);
        assert!(input.press_table_reorder(&scene, &layout));
        input.pointer_move(&scene, &layout, edge);
        assert!(input.table_row_move_drag(&layout, Instant::now()));
        for _ in 0..12 {
            let deadline = input.table_row_move_deadline(&scene, &layout).unwrap();
            assert!(input.advance_table_row_move(&scene, &layout, deadline));
            (scene, layout) = prepare_list(ui, &input, size);
            input.reconcile(&scene, &layout);
        }
        let offset = input.scroll_offsets()[&path[..]];
        assert!(offset > 0.0);
        let reorder = input.finish_table_row_move(&scene, &layout).unwrap();
        let ControlAction::MoveRow {
            from: 0, to, row, ..
        } = reorder.action
        else {
            panic!("expected a row move");
        };
        assert_eq!(row, first);
        assert!(ui.control_at(&scene, &reorder.path, reorder.action));
        let moved = ui.list_rows(&path, to..to + 1);
        assert_eq!(moved[0].row_key, Some(first));
        (visual, offset, to)
    }

    assert_eq!(exercise(&mut actual), exercise(&mut expected));
    assert_eq!(
        actual.get("moved-row-to"),
        Ok(Value::Int(expected.get_moved_row_to()))
    );
    assert_same_scene(&actual.try_render().unwrap(), &expected.render());
}

#[test]
fn table_row_drag_rejects_a_mismatched_origin_before_reading_live_rows() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let scenes = [interpreted.render_shared(), generated.render_shared()];
    let keys = scenes.each_ref().map(|scene| {
        scene.children[0]
            .list_row_renderer
            .as_ref()
            .unwrap()
            .render(0..1)[0]
            .row_key
            .unwrap()
    });
    let rows = interpreted.get("rows").unwrap();
    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    interpreted
        .on("source", {
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(rows.clone()))
            })
        })
        .unwrap();
    generated.on_source({
        let rows = generated.get_rows();
        let reads = expected_reads.clone();
        move || {
            reads.set(reads.get() + 1);
            rows.clone()
        }
    });

    for (ui, scene, row) in [
        (&mut interpreted as &mut dyn Component, &scenes[0], keys[0]),
        (&mut generated, &scenes[1], keys[1]),
    ] {
        let table = &scene.children[0];
        assert!(table.boolean("movable-rows", false));
        let (model, revision) = table.standard_list_stamp.unwrap();
        assert!(!ui.control_at(
            scene,
            &[0],
            ControlAction::MoveRow {
                from: 0,
                to: 1,
                row,
                stamp: (model, revision.wrapping_add(1)),
            },
        ));
    }
    assert_eq!([actual_reads.get(), expected_reads.get()], [0, 0]);
    for (ui, scene, row) in [
        (&mut interpreted as &mut dyn Component, &scenes[0], keys[0]),
        (&mut generated, &scenes[1], keys[1]),
    ] {
        let table = &scene.children[0];
        assert!(!ui.control_at(
            scene,
            &[0],
            ControlAction::MoveRow {
                from: 0,
                to: 1,
                row,
                stamp: table.standard_list_stamp.unwrap(),
            },
        ));
    }
    assert_eq!([actual_reads.get(), expected_reads.get()], [1, 1]);
    assert_eq!(interpreted.get("moves"), Ok(0_i64.into()));
    assert_eq!(generated.get_moves(), 0);
}

#[test]
fn negative_table_height_index_does_not_read_the_model() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::Length;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    interpreted
        .on("source", {
            let rows = interpreted.get("rows").unwrap();
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(rows.clone()))
            })
        })
        .unwrap();
    generated.on_source({
        let rows = generated.get_rows();
        let reads = expected_reads.clone();
        move || {
            reads.set(reads.get() + 1);
            rows.clone()
        }
    });

    for (row, height) in [(-1_i64, 50.0), (0, f64::INFINITY), (0, f64::NAN)] {
        interpreted
            .invoke("set-height", &[row.into(), Length(height).into()])
            .unwrap();
        generated.invoke_set_height(row, Length(height));
    }
    assert_eq!([actual_reads.get(), expected_reads.get()], [0, 0]);

    for height in [50.0, -1.0] {
        interpreted
            .invoke("set-height", &[0_i64.into(), Length(height).into()])
            .unwrap();
        generated.invoke_set_height(0, Length(height));
        let actual = interpreted.try_render().unwrap();
        let expected = generated.render_shared();
        for scene in [&*actual, &*expected] {
            let height_override = scene.children[0]
                .row_heights
                .as_ref()
                .map(|rows| rows.height(0));
            assert_eq!(height_override, (height >= 0.0).then_some(height as f32));
        }
    }
}

#[test]
fn table_row_move_checks_sort_and_indices_before_reading_the_model() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    actual.invoke("sort", &[]).unwrap();
    generated.invoke_sort();
    assert_eq!(actual.get("sort-column"), Ok(0_i64.into()));
    assert_eq!(generated.get_sort_column(), 0);

    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    let rows = actual.get("rows").unwrap();
    actual
        .on("source", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("model");
                Ok(Some(rows.clone()))
            })
        })
        .unwrap();
    generated.on_source({
        let rows = generated.get_rows();
        let calls = generated_calls.clone();
        move || {
            calls.borrow_mut().push("model");
            rows.clone()
        }
    });
    actual
        .on("automatic", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("auto-sort");
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_automatic({
        let calls = generated_calls.clone();
        move || {
            calls.borrow_mut().push("auto-sort");
            true
        }
    });
    actual
        .invoke("move-row", &[0_i64.into(), 1_i64.into()])
        .unwrap();
    generated.invoke_move_row(0, 1);
    assert_eq!(generated_calls.take(), ["auto-sort"]);
    assert_eq!(actual_calls.take(), ["auto-sort"]);
    assert_eq!(actual.get("moves"), Ok(0_i64.into()));
    assert_eq!(generated.get_moves(), 0);

    actual
        .on("automatic", Rc::new(|_| Ok(Some(false.into()))))
        .unwrap();
    generated.on_automatic(|| false);
    actual
        .invoke("move-row", &[(-1_i64).into(), 0_i64.into()])
        .unwrap();
    generated.invoke_move_row(-1, 0);
    assert!(generated_calls.take().is_empty());
    assert!(actual_calls.take().is_empty());
    assert_eq!(actual.get("moves"), Ok(0_i64.into()));
    assert_eq!(generated.get_moves(), 0);

    actual
        .on("selected", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    let rows = owner.get("rows").unwrap();
                    owner
                        .on("source", Rc::new(move |_| Ok(Some(rows.clone()))))
                        .unwrap();
                }
                Ok(Some(0_i64.into()))
            })
        })
        .unwrap();
    generated.on_selected({
        let owner = generated.as_weak();
        let replace = Cell::new(true);
        move || {
            if replace.replace(false) {
                let mut owner = owner.upgrade().unwrap();
                let rows = owner.get_rows();
                owner.on_source(move || rows.clone());
            }
            0
        }
    });
    actual
        .invoke("move-row", &[0_i64.into(), 1_i64.into()])
        .unwrap();
    generated.invoke_move_row(0, 1);
    assert_eq!(actual.get("events"), Ok(String::new().into()));
    assert_eq!(generated.get_events(), "");
    assert_eq!(actual.get("moves"), Ok(0_i64.into()));
    assert_eq!(generated.get_moves(), 0);

    actual
        .invoke("move-row", &[0_i64.into(), 1_i64.into()])
        .unwrap();
    generated.invoke_move_row(0, 1);
    assert_eq!(actual.get("moves"), Ok(1_i64.into()));
    assert_eq!(generated.get_moves(), 1);
    assert_eq!(
        actual.get("events"),
        Ok("cursor:1;move:0/0/1;".to_owned().into())
    );
    assert_eq!(generated.get_events(), "cursor:1;move:0/0/1;");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn table_projection_retries_a_failed_filter_binding_without_changing_inputs() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut compiled = expressions_ui::TableMoveOrderParity::new();
    let actual_scene = interpreted.try_render().unwrap();
    let compiled_scene = compiled.render_shared();
    let actual_calls = Rc::new(Cell::new(0));
    let compiled_calls = Rc::new(Cell::new(0));
    interpreted
        .on("filter", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                let count = calls.get();
                calls.set(count + 1);
                if count == 0 {
                    return Err("filter failed".into());
                }
                Ok(Some("two".to_owned().into()))
            })
        })
        .unwrap();
    compiled.on_filter({
        let calls = compiled_calls.clone();
        move |_| {
            let count = calls.get();
            calls.set(count + 1);
            assert_ne!(count, 0, "filter failed");
            "two".to_owned()
        }
    });

    let rows = |scene: &Node| scene.list_rows_at(&[0], 0..2);
    assert!(rows(&actual_scene).is_empty());
    assert!(catch_unwind(AssertUnwindSafe(|| rows(&compiled_scene))).is_err());
    let actual = rows(&actual_scene);
    let expected = rows(&compiled_scene);
    assert_eq!(actual_calls.get(), 2);
    assert_eq!(compiled_calls.get(), 2);
    assert_eq!(actual.len(), 1);
    assert_eq!(expected.len(), 1);
    assert_eq!(actual[0].children[0].text("text"), Some("two"));
    assert_same_scene(&actual[0], &expected[0]);
    assert_eq!(
        interpreted.take_error(),
        Some(Error::Evaluation("filter failed".into()))
    );
    assert_eq!(rows(&actual_scene).len(), 1);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_sort_can_retry_after_the_new_projection_fails() {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_interpreter::Error;
    use xrack_runtime::TableSort;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    interpreted
        .on("filter", {
            let owner = interpreted.as_weak();
            let fail = Cell::new(true);
            Rc::new(move |_| {
                let owner = owner.upgrade().unwrap();
                if owner.get("sort-column").unwrap() == Value::Int(0) && fail.replace(false) {
                    return Err("sort filter failed".into());
                }
                Ok(Some(String::new().into()))
            })
        })
        .unwrap();
    generated.on_filter({
        let owner = generated.as_weak();
        let fail = Cell::new(true);
        move |_| {
            let owner = owner.upgrade().unwrap();
            assert!(
                owner.get_sort_column() != 0 || !fail.replace(false),
                "sort filter failed"
            );
            String::new()
        }
    });
    let sort = Some(TableSort {
        column: 0,
        descending: true,
    });

    assert_eq!(
        interpreted.set_table_sort("table", sort),
        Err(Error::Evaluation("sort filter failed".into()))
    );
    assert!(catch_unwind(AssertUnwindSafe(|| generated.set_table_sort("table", sort))).is_err());
    assert_eq!(interpreted.table_sort("table"), Ok(None));
    assert_eq!(generated.table_sort("table"), None);
    assert_eq!(interpreted.get("events"), Ok(String::new().into()));
    assert_eq!(generated.get_events(), "");

    assert_eq!(interpreted.set_table_sort("table", sort), Ok(true));
    assert!(generated.set_table_sort("table", sort));
    assert_eq!(interpreted.table_sort("table"), Ok(sort));
    assert_eq!(generated.table_sort("table"), sort);
    let events = "cursor:1;descending:0;";
    assert_eq!(interpreted.get("events"), Ok(events.to_owned().into()));
    assert_eq!(generated.get_events(), events);
    let actual = interpreted.list_rows(&[0], 0..2);
    let expected = generated.list_rows(&[0], 0..2);
    assert_eq!(actual.len(), 2);
    assert_eq!(expected.len(), 2);
    assert_eq!(actual[0].children[0].text("text"), Some("two"));
    for (actual, expected) in actual.iter().zip(&expected) {
        assert_same_scene(actual, expected);
    }
}

#[test]
fn table_sort_rejects_a_model_invalidated_during_cursor_read() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    actual
        .on("selected", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    let rows = owner.get("rows").unwrap();
                    owner
                        .on("source", Rc::new(move |_| Ok(Some(rows.clone()))))
                        .unwrap();
                }
                Ok(Some(0_i64.into()))
            })
        })
        .unwrap();
    generated.on_selected({
        let owner = generated.as_weak();
        let replace = Cell::new(true);
        move || {
            if replace.replace(false) {
                let mut owner = owner.upgrade().unwrap();
                let rows = owner.get_rows();
                owner.on_source(move || rows.clone());
            }
            0
        }
    });
    actual.invoke("sort-reverse", &[]).unwrap();
    generated.invoke_sort_reverse();
    assert_eq!(actual.get("sort-column"), Ok((-1_i64).into()));
    assert_eq!(generated.get_sort_column(), -1);
    assert_eq!(actual.get("events"), Ok(String::new().into()));
    assert_eq!(generated.get_events(), "");

    actual.invoke("sort-reverse", &[]).unwrap();
    generated.invoke_sort_reverse();
    assert_eq!(actual.get("sort-column"), Ok(0_i64.into()));
    assert_eq!(generated.get_sort_column(), 0);
    assert_eq!(
        actual.get("events"),
        Ok("cursor:1;descending:0;".to_owned().into())
    );
    assert_eq!(generated.get_events(), "cursor:1;descending:0;");

    actual.invoke("clear-sort", &[]).unwrap();
    generated.invoke_clear_sort();
    assert_eq!(actual.get("sort-column"), Ok((-1_i64).into()));
    assert_eq!(generated.get_sort_column(), -1);
    assert_eq!(
        actual.get("events"),
        Ok("cursor:1;descending:0;cursor:0;".to_owned().into())
    );
    assert_eq!(generated.get_events(), "cursor:1;descending:0;cursor:0;");
    assert_eq!(actual.take_error(), None);
}

#[test]
fn standard_table_samples_editors_only_for_materialized_columns() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::TableColumnWindow;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableColumnEditorParity")
        .unwrap();
    let mut generated = expressions_ui::TableColumnEditorParity::new();
    let interpreted_scene = interpreted.render_shared();
    let generated_scene = generated.render_shared();
    let interpreted_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("editable-column", {
            let calls = interpreted_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(column)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*column);
                Ok(Some(true.into()))
            })
        })
        .unwrap();
    generated.on_editable_column({
        let calls = generated_calls.clone();
        move |column| {
            calls.borrow_mut().push(column);
            true
        }
    });

    let renderer = |scene: &Node| scene.children[0].list_row_renderer.clone().unwrap();
    for (window, expected) in [([false, true], 1), ([true, false], 0)] {
        let columns = TableColumnWindow::new(window);
        let left = renderer(&interpreted_scene).render_in_columns(0..2, &columns);
        let right = renderer(&generated_scene).render_in_columns(0..2, &columns);
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 2);
        for (left, right) in left.iter().zip(&right) {
            assert_same_scene(left, right);
            assert_eq!(left.children.len(), 1);
            assert!(left.children[0].boolean("editable", false));
        }
        assert_eq!(generated_calls.take(), [expected]);
        assert_eq!(interpreted_calls.take(), [expected]);
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn stale_table_sort_click_does_not_read_the_row_model() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{ControlAction, TableSort};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let scenes = [interpreted.render_shared(), generated.render_shared()];
    let sort = Some(TableSort {
        column: 0,
        descending: true,
    });
    assert_eq!(interpreted.set_table_sort("table", sort), Ok(true));
    assert!(generated.set_table_sort("table", sort));

    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    interpreted
        .on("source", {
            let rows = interpreted.get("rows").unwrap();
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(rows.clone()))
            })
        })
        .unwrap();
    generated.on_source({
        let rows = generated.get_rows();
        let reads = expected_reads.clone();
        move || {
            reads.set(reads.get() + 1);
            rows.clone()
        }
    });

    assert!(!interpreted.control_at(&scenes[0], &[0], ControlAction::SortColumn(0)));
    assert!(!generated.control_at(&scenes[1], &[0], ControlAction::SortColumn(0)));
    assert_eq!([actual_reads.get(), expected_reads.get()], [0, 0]);
    assert_eq!(interpreted.table_sort("table"), Ok(sort));
    assert_eq!(generated.table_sort("table"), sort);
}

#[test]
fn table_drag_commits_check_live_resize_and_reorder_permissions() {
    use xrack_runtime::ControlAction;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableColumnEditorParity")
        .unwrap();
    let mut generated = expressions_ui::TableColumnEditorParity::new();
    let scenes = [interpreted.render_shared(), generated.render_shared()];
    let mut cases = Vec::new();
    for scene in &scenes {
        let table = &scene.children[0];
        let row = table.list_row_renderer.as_ref().unwrap().render(0..1)[0]
            .row_key
            .unwrap();
        cases.push([
            ControlAction::ResizeColumn {
                column: 0,
                width: 180,
            },
            ControlAction::ResizeColumn {
                column: 0,
                width: 180,
            },
            ControlAction::ResizeRow {
                row: 0,
                key: row,
                height: 47,
            },
            ControlAction::MoveColumn { from: 0, to: 1 },
            ControlAction::MoveRow {
                from: 0,
                to: 1,
                row,
                stamp: table.standard_list_stamp.unwrap(),
            },
        ]);
    }
    let path = [0];
    for (case, setter) in [
        (
            0,
            expressions_ui::TableColumnEditorParity::set_resize_columns as fn(&mut _, bool),
        ),
        (
            1,
            expressions_ui::TableColumnEditorParity::set_first_resizable,
        ),
        (2, expressions_ui::TableColumnEditorParity::set_resize_rows),
        (3, expressions_ui::TableColumnEditorParity::set_move_columns),
        (4, expressions_ui::TableColumnEditorParity::set_move_rows),
    ] {
        let flag = [
            "resize-columns",
            "first-resizable",
            "resize-rows",
            "move-columns",
            "move-rows",
        ][case];
        interpreted.set(flag, false.into()).unwrap();
        setter(&mut generated, false);
        let actual = interpreted.control_at(&scenes[0], &path, cases[0][case]);
        let expected = generated.control_at(&scenes[1], &path, cases[1][case]);
        assert_eq!([actual, expected], [false, false], "case {case}");
        interpreted.set(flag, true.into()).unwrap();
        setter(&mut generated, true);
    }
    for case in [1, 2, 3, 4] {
        assert!(interpreted.control_at(&scenes[0], &path, cases[0][case]));
        assert!(generated.control_at(&scenes[1], &path, cases[1][case]));
    }
    for (ui, old) in [
        (&mut interpreted as &mut dyn Component, &scenes[0]),
        (&mut generated, &scenes[1]),
    ] {
        let move_column = ControlAction::MoveColumn { from: 0, to: 1 };
        assert!(!ui.control_at(old, &path, move_column));
        let current = ui.render_shared();
        assert_eq!(
            current.children[0].table_column_order.as_slice(2),
            Some(&[1, 0][..])
        );
        assert!(!ui.control_at(
            &current,
            &path,
            ControlAction::ResizeColumn {
                column: 0,
                width: 180
            }
        ));
    }
    let actual = interpreted.render();
    let expected = generated.render();
    assert_same_scene(&actual, &expected);
    assert_eq!(
        actual.children[0]
            .table_header
            .as_ref()
            .unwrap()
            .column_widths,
        [Some(180)]
    );
    assert_eq!(
        actual.children[0].table_column_order,
        expected.children[0].table_column_order
    );
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn programmatic_table_selection_rejects_an_invalidated_model() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    actual
        .on("selected", {
            let owner = Cell::new(Some(actual.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    let rows = owner.get("rows").unwrap();
                    owner
                        .on("source", Rc::new(move |_| Ok(Some(rows.clone()))))
                        .unwrap();
                }
                Ok(Some(0_i64.into()))
            })
        })
        .unwrap();
    generated.on_selected({
        let owner = generated.as_weak();
        let replace = Cell::new(true);
        move || {
            if replace.replace(false) {
                let mut owner = owner.upgrade().unwrap();
                let rows = owner.get_rows();
                owner.on_source(move || rows.clone());
            }
            0
        }
    });
    actual
        .invoke("select-cell", &[1_i64.into(), 0_i64.into()])
        .unwrap();
    generated.invoke_select_cell(1, 0);
    assert_eq!(actual.get("events"), Ok(String::new().into()));
    assert_eq!(generated.get_events(), "");

    actual
        .invoke("select-cell", &[1_i64.into(), 0_i64.into()])
        .unwrap();
    generated.invoke_select_cell(1, 0);
    assert_eq!(
        actual.get("events"),
        Ok("cursor:1;cell:1/0;".to_owned().into())
    );
    assert_eq!(generated.get_events(), "cursor:1;cell:1/0;");
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn source_table_selection_uses_the_projection_that_resolves_the_source_row() {
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    interpreted.set("query", "two".to_owned().into()).unwrap();
    generated.set_query("two".into());

    interpreted
        .on("selected", {
            let owner = Cell::new(Some(interpreted.clone()));
            Rc::new(move |_| {
                if let Some(owner) = owner.take() {
                    owner.set("query", String::new().into()).unwrap();
                }
                Ok(Some((-1_i64).into()))
            })
        })
        .unwrap();
    generated.on_selected({
        let owner = generated.as_weak();
        let clear = Cell::new(true);
        move || {
            if clear.replace(false) {
                owner.upgrade().unwrap().set_query(String::new());
            }
            -1
        }
    });

    interpreted
        .invoke("select-source", &[1_i64.into()])
        .unwrap();
    generated.invoke_select_source(1);
    assert_eq!(interpreted.get("query"), Ok(String::new().into()));
    assert_eq!(generated.get_query(), "");
    assert_eq!(interpreted.get("events"), Ok(String::new().into()));
    assert_eq!(generated.get_events(), "");

    interpreted
        .invoke("select-source", &[1_i64.into()])
        .unwrap();
    generated.invoke_select_source(1);
    assert_eq!(
        interpreted.get("events"),
        Ok("cursor:1;cell:1/0;".to_owned().into())
    );
    assert_eq!(generated.get_events(), "cursor:1;cell:1/0;");
    let actual = interpreted.render_shared();
    let expected = generated.render_shared();
    let actual_table = &actual.children[0];
    let expected_table = &expected.children[0];
    for table in [actual_table, expected_table] {
        let rows = table.list_rows_at(&[], 0..2);
        let key = rows[1].row_key.unwrap();
        assert_eq!(rows[1].children[0].text("text"), Some("two"));
        let selection = table.table_selection.as_ref().unwrap();
        assert_eq!(selection.len(), 1);
        assert!(selection.contains_cell(key, 0));
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn list_source_selection_cancels_if_cursor_read_removes_a_row() {
    use expressions_ui::ListSelectionParity;
    use std::{cell::Cell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ListSelectionParity")
        .unwrap();
    let mut generated = ListSelectionParity::new();
    let model = actual.string_model("labels").unwrap();
    let generated_model = generated.model_labels();
    actual
        .on("read-selection", {
            let model = Cell::new(Some(model.clone()));
            Rc::new(move |_| {
                if let Some(model) = model.take() {
                    assert_eq!(model.remove(0), Some("A".into()));
                }
                Ok(Some((-1_i64).into()))
            })
        })
        .unwrap();
    generated.on_read_selection({
        let model = Cell::new(Some(generated_model.clone()));
        move || {
            if let Some(model) = model.take() {
                assert_eq!(model.remove(0), Some("A".into()));
            }
            -1
        }
    });

    actual.invoke("select-source", &[1_i64.into()]).unwrap();
    generated.invoke_select_source(1);
    assert_eq!(actual.get("selected"), Ok((-1_i64).into()));
    assert_eq!(generated.get_selected(), -1);
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(generated.get_changes(), 0);

    actual.invoke("select-source", &[0_i64.into()]).unwrap();
    generated.invoke_select_source(0);
    assert_eq!(model.row(0).as_deref(), Some("B"));
    assert_eq!(generated_model.row(0).as_deref(), Some("B"));
    assert_eq!(actual.get("selected"), Ok(0_i64.into()));
    assert_eq!(generated.get_selected(), 0);
    assert_eq!(actual.get("changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tree_source_selection_cancels_if_cursor_read_removes_the_target() {
    use std::{cell::Cell, rc::Rc};

    let (actual, mut generated) = tree_input_fixture();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected a record model");
    };
    actual
        .on("read-key", {
            let model = Cell::new(Some(records.model().clone()));
            Rc::new(move |_| {
                if let Some(model) = model.take() {
                    assert!(model.remove(1).is_some());
                }
                Ok(Some((-1_i64).into()))
            })
        })
        .unwrap();
    generated.on_read_key({
        let model = Cell::new(Some(generated.model_records()));
        move || {
            if let Some(model) = model.take() {
                assert!(model.remove(1).is_some());
            }
            -1
        }
    });

    actual.invoke("select-source", &[1_i64.into()]).unwrap();
    generated.invoke_select_source(1);
    assert_eq!(actual.get("current"), Ok((-1_i64).into()));
    assert_eq!(generated.get_current(), -1);
    assert_eq!(actual.get("changes"), Ok(0_i64.into()));
    assert_eq!(generated.get_changes(), 0);

    actual.invoke("select-source", &[1_i64.into()]).unwrap();
    generated.invoke_select_source(1);
    assert_eq!(actual.get("current"), Ok(3_i64.into()));
    assert_eq!(generated.get_current(), 3);
    assert_eq!(actual.get("changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn tree_collapse_cancels_when_the_current_key_read_changes_the_model() {
    use std::{cell::Cell, rc::Rc};

    let (actual, mut generated) = tree_input_fixture();
    actual.invoke("expand-root", &[]).unwrap();
    generated.invoke_expand_root();
    let Value::RecordList(records) = actual.get("records").unwrap() else {
        panic!("expected a record model");
    };
    actual
        .on("read-key", {
            let model = Cell::new(Some(records.model().clone()));
            Rc::new(move |_| {
                if let Some(model) = model.take() {
                    assert!(model.remove(2).is_some());
                }
                Ok(Some(11_i64.into()))
            })
        })
        .unwrap();
    generated.on_read_key({
        let model = Cell::new(Some(generated.model_records()));
        move || {
            if let Some(model) = model.take() {
                assert!(model.remove(2).is_some());
            }
            11
        }
    });

    actual.invoke("collapse-root", &[]).unwrap();
    generated.invoke_collapse_root();
    assert_eq!(tree_keys(&actual.render(), &[0]), [1, 11, 2]);
    assert_eq!(tree_keys(&generated.render(), &[0]), [1, 11, 2]);
    assert_eq!(actual.get("collapses"), Ok(0_i64.into()));
    assert_eq!(generated.get_collapses(), 0);

    actual.invoke("collapse-root", &[]).unwrap();
    generated.invoke_collapse_root();
    assert_eq!(tree_keys(&actual.render(), &[0]), [1, 2]);
    assert_eq!(tree_keys(&generated.render(), &[0]), [1, 2]);
    for (name, expected) in [
        ("current", generated.get_current()),
        ("current-at-collapse", generated.get_current_at_collapse()),
        ("changes", generated.get_changes()),
        ("collapses", generated.get_collapses()),
    ] {
        assert_eq!(expected, 1, "{name}");
        assert_eq!(actual.get(name), Ok(expected.into()), "{name}");
    }
    assert_eq!(actual.take_error(), None);
}

#[test]
fn render_samples_ordinary_bindings_before_controlled_values() {
    use expressions_ui::ControlRenderOrderParity;
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("ControlRenderOrderParity")
        .unwrap();
    let mut generated = ControlRenderOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    actual
        .on("sample", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::Int(tag)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(*tag);
                Ok(Some((calls.borrow().len() as f64 * 10.0).into()))
            })
        })
        .unwrap();
    generated.on_sample({
        let calls = generated_calls.clone();
        move |tag| {
            calls.borrow_mut().push(tag);
            calls.borrow().len() as f64 * 10.0
        }
    });

    let compiled = generated.render_shared();
    let interpreted = actual.try_render().unwrap();
    assert_eq!(*generated_calls.borrow(), [1, 2, 3]);
    assert_eq!(*actual_calls.borrow(), [1, 2, 3]);
    assert_same_scene(&interpreted, &compiled);
    assert_eq!(interpreted.children[0].number("lower-value"), Some(20.0));
    assert_eq!(interpreted.children[0].number("upper-value"), Some(30.0));

    assert!(actual.dispatch("group", "clicked"));
    assert!(generated.dispatch("group", "clicked"));
    assert!(!generated.get_last_checked());
    assert_eq!(actual.get("last-checked"), Ok(false.into()));
    assert_same_scene(&actual.render(), &generated.render());
    assert_eq!(*actual_calls.borrow(), [1, 2, 3]);
    assert_eq!(*generated_calls.borrow(), [1, 2, 3]);
}

#[test]
fn tumbler_renders_model_before_reading_current_index() {
    use expressions_ui::TumblerModelOrderParity;
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::Tumbler;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TumblerModelOrderParity")
        .unwrap();
    let mut generated = TumblerModelOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let generated_calls = Rc::new(RefCell::new(Vec::new()));
    let items = || ["One", "Two", "Three"].map(str::to_owned).to_vec();
    actual
        .on("read-items", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("items");
                Ok(Some(items().into()))
            })
        })
        .unwrap();
    actual
        .on("read-index", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("index");
                Ok(Some(0_i64.into()))
            })
        })
        .unwrap();
    generated.on_read_items({
        let calls = generated_calls.clone();
        move || {
            calls.borrow_mut().push("items");
            items()
        }
    });
    generated.on_read_index({
        let calls = generated_calls.clone();
        move || {
            calls.borrow_mut().push("index");
            0
        }
    });
    let interpreted = actual.render_shared();
    let compiled = generated.render_shared();
    assert_eq!(*generated_calls.borrow(), ["items", "index"]);
    assert_eq!(*actual_calls.borrow(), ["items", "index"]);
    for (ui, scene) in [
        (&mut actual as &mut dyn Component, interpreted),
        (&mut generated, compiled),
    ] {
        let wheel = Tumbler::from_node(&scene.children[0]);
        assert_eq!(wheel.label(2).as_deref(), Some("Three"));
        assert!(ui.control_at(&scene, &[0], wheel.select(2).unwrap()));
    }
    assert_eq!(actual.get("selected"), Ok(2_i64.into()));
    assert_eq!(generated.get_selected(), 2);
    assert_eq!(actual.get("changes"), Ok(1_i64.into()));
    assert_eq!(generated.get_changes(), 1);
    assert_eq!(*actual_calls.borrow(), ["items", "index"]);
    assert_eq!(*generated_calls.borrow(), ["items", "index"]);
    assert_eq!(actual.take_error(), None);
}

#[test]
fn record_combo_rejects_stale_projected_rows_but_ignores_unrelated_edits() {
    use selection_ui::{RecordComboDemo, WorkspaceOption};
    use xrack_runtime::ControlAction::{AcceptComboText, Select};

    let preview = xrack_compiler::prepare_preview(
        include_str!("../examples/selection/selection.xr"),
        "selection.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("RecordComboDemo")
        .unwrap();
    let mut generated = RecordComboDemo::new();
    let Value::RecordList(options) = actual.get("options").unwrap() else {
        panic!("expected options model");
    };
    let model = options.model();
    let generated_model = generated.model_options();
    let path = [1];
    let update = |field: &[&str], value: Value, update_generated: fn(&mut WorkspaceOption)| {
        let field: Vec<_> = field.iter().map(|name| (*name).into()).collect();
        let row = model.row(4).unwrap().with_field(&field, value).unwrap();
        assert!(model.set_row(4, row));
        let mut row = generated_model.row(4).unwrap();
        update_generated(&mut row);
        assert!(generated_model.set_row(4, row));
    };

    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    update(&["notes"], "irrelevant".to_owned().into(), |row| {
        row.notes = "irrelevant".into();
    });
    assert!(actual.control_at(&stale, &path, Select(1)));
    assert!(generated.control_at(&compiled_stale, &path, Select(1)));

    actual.set("editable", true.into()).unwrap();
    generated.set_editable(true);
    assert!(actual.edit_at(&actual.render_shared(), &path, "Build".into()));
    assert!(generated.edit_at(&generated.render_shared(), &path, "Build".into()));
    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    update(&["code"], 501_i64.into(), |row| row.code = 501);
    for action in [Select(4), AcceptComboText] {
        let interpreted = actual.control_at(&stale, &path, action);
        let compiled = generated.control_at(&compiled_stale, &path, action);
        assert_eq!([interpreted, compiled], [false, false], "{action:?}");
    }
    assert_eq!(actual.get("code"), Ok(100_i64.into()));
    assert_eq!(generated.get_code(), 100);
    assert_eq!(actual.get("accepted-text"), Ok(String::new().into()));
    assert!(generated.get_accepted_text().is_empty());

    assert!(actual.control_at(&actual.render_shared(), &path, AcceptComboText));
    assert!(generated.control_at(&generated.render_shared(), &path, AcceptComboText));
    assert_eq!(actual.get("code"), Ok(501_i64.into()));
    assert_eq!(generated.get_code(), 501);

    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    update(&["title"], "Build next".to_owned().into(), |row| {
        row.title = "Build next".into();
    });
    assert!(!actual.control_at(&stale, &path, Select(4)));
    assert!(!generated.control_at(&compiled_stale, &path, Select(4)));

    let stale = actual.render_shared();
    let compiled_stale = generated.render_shared();
    update(&["access", "available"], false.into(), |row| {
        row.access.available = false;
    });
    assert!(!actual.control_at(&stale, &path, Select(4)));
    assert!(!generated.control_at(&compiled_stale, &path, Select(4)));
    assert!(!actual.control_at(&actual.render_shared(), &path, Select(4)));
    assert!(!generated.control_at(&generated.render_shared(), &path, Select(4)));
    assert_eq!(actual.take_error(), None);
}

#[test]
fn host_table_snapshot_reads_cursor_before_selection_mode() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::SelectionTarget;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));

    interpreted
        .on("selected", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("cursor");
                Ok(Some(0_i64.into()))
            })
        })
        .unwrap();
    interpreted
        .on("selection-mode", {
            let calls = actual_calls.clone();
            Rc::new(move |_| {
                calls.borrow_mut().push("mode");
                Ok(Some(SelectionTarget::Cells.into()))
            })
        })
        .unwrap();
    generated.on_selected({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push("cursor");
            0
        }
    });
    generated.on_selection_mode({
        let calls = expected_calls.clone();
        move || {
            calls.borrow_mut().push("mode");
            SelectionTarget::Cells
        }
    });

    let actual = interpreted.table_selection("table").unwrap();
    let expected = generated.table_selection("table").unwrap();
    assert_eq!(*expected_calls.borrow(), ["cursor", "mode"]);
    assert_eq!(*actual_calls.borrow(), ["cursor", "mode"]);
    assert_eq!(actual.target(), expected.target());
    assert_eq!(actual.current_column(), expected.current_column());
    assert_eq!(actual.len(), expected.len());
}

#[test]
fn rendered_table_selection_uses_the_scene_model_and_mode() {
    use std::rc::Rc;
    use xrack_runtime::SelectionTarget;

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    interpreted
        .on("column-title", {
            let weak = interpreted.as_weak();
            Rc::new(move |_| {
                let ui = weak.upgrade().unwrap();
                ui.set("query", "two".to_owned().into()).unwrap();
                ui.on(
                    "selection-mode",
                    Rc::new(|_| Ok(Some(SelectionTarget::Rows.into()))),
                )
                .unwrap();
                Ok(Some("Name".to_owned().into()))
            })
        })
        .unwrap();
    generated.on_column_title({
        let weak = generated.as_weak();
        move || {
            let mut ui = weak.upgrade().unwrap();
            ui.set_query("two".into());
            ui.on_selection_mode(|| SelectionTarget::Rows);
            "Name".into()
        }
    });

    for ui in [&interpreted as &dyn Component, &generated] {
        let scene = ui.render_shared();
        let table = scene.find("table").unwrap();
        assert_eq!(table.list.unwrap().row_count, 2);
        let selection = table.table_selection.as_ref().unwrap();
        assert_eq!(Some(selection.structure_stamp()), table.standard_list_stamp);
        assert_eq!(selection.target(), table.selection_target());
        assert_eq!(selection.target(), SelectionTarget::Cells);

        let updated = ui.render_shared();
        let table = updated.find("table").unwrap();
        assert_eq!(table.list.unwrap().row_count, 1);
        assert_eq!(
            table.table_selection.as_ref().unwrap().target(),
            SelectionTarget::Rows
        );
    }
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn table_selection_without_cell_handler_does_not_resample_bindings() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{RowSelection, SelectionTarget, TableSelectionAction};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableRowNotificationParity")
        .unwrap();
    let mut generated = expressions_ui::TableRowNotificationParity::new();
    let actual_reads = Rc::new(Cell::new(0));
    let expected_reads = Rc::new(Cell::new(0));
    interpreted
        .on("selection-mode", {
            let reads = actual_reads.clone();
            Rc::new(move |_| {
                reads.set(reads.get() + 1);
                Ok(Some(SelectionTarget::Cells.into()))
            })
        })
        .unwrap();
    generated.on_selection_mode({
        let reads = expected_reads.clone();
        move |_| {
            reads.set(reads.get() + 1);
            SelectionTarget::Cells
        }
    });

    let action = TableSelectionAction::Cell {
        row: 1,
        column: 0,
        operation: RowSelection::Replace,
    };
    assert!(interpreted.select_table("table", action).unwrap());
    assert!(generated.select_table("table", action));
    assert_eq!([actual_reads.get(), expected_reads.get()], [1, 1]);
    assert_eq!(interpreted.get("revision"), Ok(1_i64.into()));
    assert_eq!(generated.get_events(), "row:1;count:1;");
    assert_eq!(interpreted.get("events"), Ok(generated.get_events().into()));
}

#[test]
fn table_range_command_evaluates_operands_before_rejecting_an_invalid_index() {
    use std::{cell::RefCell, rc::Rc};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let actual_reads = Rc::new(RefCell::new(Vec::new()));
    let expected_reads = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("selection-position", {
            let reads = actual_reads.clone();
            Rc::new(move |args| {
                let [Value::Int(position)] = args else {
                    unreachable!()
                };
                reads.borrow_mut().push(*position);
                Ok(Some((*position).into()))
            })
        })
        .unwrap();
    generated.on_selection_position({
        let reads = expected_reads.clone();
        move |position| {
            reads.borrow_mut().push(position);
            position
        }
    });

    for (first, count) in [(-1_i64, 0), (0, 2)] {
        interpreted
            .invoke("select-range", &[first.into(), 1_i64.into()])
            .unwrap();
        generated.invoke_select_range(first, 1);
        assert_eq!(actual_reads.take(), [first, 1]);
        assert_eq!(expected_reads.take(), [first, 1]);
        assert_eq!(interpreted.table_selection("table").unwrap().len(), count);
        assert_eq!(generated.table_selection("table").unwrap().len(), count);
    }
    assert_eq!(interpreted.get("events"), Ok(generated.get_events().into()));
}

#[test]
fn host_table_apis_match_generated_selection_sort_and_callbacks() {
    use xrack_runtime::{RowSelection, TableSelectionAction, TableSort};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("TableMoveOrderParity")
        .unwrap();
    let mut generated = expressions_ui::TableMoveOrderParity::new();
    let id = "table";
    let action = TableSelectionAction::Cell {
        row: 1,
        column: 0,
        operation: RowSelection::Replace,
    };
    assert!(actual.select_table(id, action).unwrap());
    assert!(generated.select_table(id, action));

    let sort = TableSort {
        column: 0,
        descending: true,
    };
    assert_eq!(actual.table_sort(id), Ok(None));
    assert_eq!(generated.table_sort(id), None);
    for (candidate, changed) in [(Some(sort), true), (Some(sort), false), (None, true)] {
        assert_eq!(actual.set_table_sort(id, candidate), Ok(changed));
        assert_eq!(generated.set_table_sort(id, candidate), changed);
        assert_eq!(actual.table_sort(id), Ok(candidate));
        assert_eq!(generated.table_sort(id), candidate);

        let left = actual.table_selection(id).unwrap();
        let right = generated.table_selection(id).unwrap();
        assert_eq!(left.len(), right.len());
        assert_eq!(left.len(), 1);
        assert_eq!(left.current_column(), right.current_column());
        assert_eq!(left.current_column(), Some(0));
        let row = usize::from(candidate.is_none());
        for (key, selection) in [
            (
                actual.list_rows(&[0], row..row + 1)[0].row_key.unwrap(),
                left,
            ),
            (
                generated.list_rows(&[0], row..row + 1)[0].row_key.unwrap(),
                right,
            ),
        ] {
            assert!(selection.contains_cell(key, 0));
        }
    }

    let events = "cursor:1;cell:1/0;cursor:0;descending:0;cursor:1;";
    assert_eq!(actual.get("events"), Ok(events.to_owned().into()));
    assert_eq!(generated.get_events(), events);
    assert!(actual.table_selection("missing").is_err());
    assert!(actual.table_sort("root").is_err());
    assert!(actual.select_table("missing", action).is_err());
    assert!(actual.set_table_sort("root", Some(sort)).is_err());
    assert_eq!(actual.take_error(), None);
}

#[test]
fn host_tree_commands_match_generated_snapshots_and_callbacks() {
    use xrack_runtime::TreeCommand::{Collapse, Expand};

    let (mut interpreted, mut generated) = tree_input_fixture();
    let id = "branches";
    interpreted.set("current", 11_i64.into()).unwrap();
    generated.set_current(11);

    for (command, changed, keys) in [
        (Expand(1), true, &[1, 11, 2, 3][..]),
        (Expand(1), false, &[1, 11, 2, 3]),
        (Collapse(1), true, &[1, 2, 3]),
        (Collapse(99), false, &[1, 2, 3]),
    ] {
        assert_eq!(interpreted.command_tree(id, command), Ok(changed));
        assert_eq!(generated.command_tree(id, command), changed);
        let interpreted = interpreted.tree_snapshot(id).unwrap();
        let generated = generated.tree_snapshot(id).unwrap();
        for snapshot in [&interpreted, &generated] {
            assert!(
                snapshot
                    .rows()
                    .iter()
                    .map(|item| item.key)
                    .eq(keys.iter().copied())
            );
        }
        assert_eq!(interpreted.is_expanded(1), generated.is_expanded(1));
    }
    assert_eq!(generated.get_current(), 1);
    assert_eq!(generated.get_current_at_collapse(), 1);
    for (name, value) in [
        ("current", generated.get_current()),
        ("current-at-collapse", generated.get_current_at_collapse()),
        ("changes", generated.get_changes()),
        ("expansions", generated.get_expansions()),
        ("collapses", generated.get_collapses()),
    ] {
        assert_eq!(value, 1, "{name}");
        assert_eq!(interpreted.get(name), Ok(value.into()), "{name}");
    }
    assert!(interpreted.tree_snapshot("root").is_err());
    assert!(interpreted.command_tree("missing", Expand(1)).is_err());
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn host_split_state_round_trips_across_backends() {
    use xrack_runtime::{encode_split_sizes, encode_split_state};

    macro_rules! round_trip {
        ($component:ident, $encode:ident, $panes:expr, $values:expr, $wrong_mode:expr) => {{
            let interpreted = interpret_split_fixture(stringify!($component));
            let mut generated = split_ui::$component::new();
            let id = "work";
            let initial = interpreted.save_split_state(id).unwrap();
            assert_eq!(
                generated.save_split_state(id).as_deref(),
                Some(initial.as_str())
            );
            let serialized = $encode(&$panes, Some(&$values));
            assert_eq!(interpreted.restore_split_state(id, &serialized), Ok(true));
            let interpreted_state = interpreted.save_split_state(id).unwrap();
            assert!(generated.restore_split_state(id, &interpreted_state));
            assert_eq!(
                generated.save_split_state(id).as_deref(),
                Some(serialized.as_str())
            );
            assert_same_scene(&interpreted.render(), &generated.render());
            assert_eq!(interpreted.get("edits"), Ok(1_i64.into()));
            assert_eq!(generated.get_edits(), 1);

            let compiled_state = generated.save_split_state(id).unwrap();
            assert_eq!(
                interpreted.restore_split_state(id, &compiled_state),
                Ok(true)
            );
            assert!(generated.restore_split_state(id, &compiled_state));
            for invalid in ["invalid", &$wrong_mode] {
                assert_eq!(interpreted.restore_split_state(id, invalid), Ok(false));
                assert!(!generated.restore_split_state(id, invalid));
            }
            assert_eq!(interpreted.get("edits"), Ok(1_i64.into()));
            assert_eq!(generated.get_edits(), 1);
            assert_eq!(interpreted.restore_split_state(id, &initial), Ok(true));
            assert!(generated.restore_split_state(id, &initial));
            assert_eq!(interpreted.get("edits"), Ok(2_i64.into()));
            assert_eq!(generated.get_edits(), 2);
            assert!(interpreted.save_split_state("missing").is_err());
            assert!(interpreted.restore_split_state("root", &initial).is_err());
            assert_eq!(interpreted.take_error(), None);
        }};
    }
    let panes = ["navigation", "preview-pane", "detail"];
    round_trip!(
        SplitDemo,
        encode_split_state,
        panes,
        [Some(1.0), Some(2.0), Some(3.0)],
        encode_split_sizes(&panes, None)
    );
    let panes = ["sidebar", "editor", "inspector"];
    round_trip!(
        FillSplitDemo,
        encode_split_sizes,
        panes,
        [Some(144.0), None, Some(176.0)],
        encode_split_state(&panes, None)
    );
}

#[test]
fn picker_cancel_does_not_evaluate_commit_bindings() {
    use expressions_ui::PickerCancelBindingsParity;
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{ControlAction, Date, DialogRole};

    let preview = xrack_compiler::prepare_preview(
        include_str!("expressions.xr"),
        "expressions.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("PickerCancelBindingsParity")
        .unwrap();
    let mut generated = PickerCancelBindingsParity::new();
    let actual_calls = Rc::new(Cell::new(0));
    let generated_calls = Rc::new(Cell::new(0));
    let actual = interpreted.render_shared();
    let compiled = generated.render_shared();
    let date = Date::new(2030, 12, 31).unwrap();
    for (name, value) in [
        ("color-alpha", Value::Bool(true)),
        ("date-maximum", date.into()),
        ("time-format", true.into()),
    ] {
        let calls = actual_calls.clone();
        interpreted
            .on(
                name,
                Rc::new(move |_| {
                    calls.set(calls.get() + 1);
                    Ok(Some(value.clone()))
                }),
            )
            .unwrap();
    }
    generated.on_color_alpha({
        let calls = generated_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            true
        }
    });
    generated.on_date_maximum({
        let calls = generated_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            date
        }
    });
    generated.on_time_format({
        let calls = generated_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            true
        }
    });

    let cancel = ControlAction::Dialog(DialogRole::Reject);
    for path in [[0], [1], [2]] {
        assert!(interpreted.control_at(&actual, &path, cancel));
        assert!(generated.control_at(&compiled, &path, cancel));
    }
    assert_eq!([actual_calls.get(), generated_calls.get()], [0, 0]);
    assert_eq!(interpreted.get("cancels"), Ok(3_i64.into()));
    assert_eq!(generated.get_cancels(), 3);
    assert_same_scene(&interpreted.render(), &generated.render());
    assert_eq!(interpreted.take_error(), None);
}
