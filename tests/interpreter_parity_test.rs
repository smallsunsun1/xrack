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
    let mut interpreted = program.create("TrayWindow").unwrap();
    let mut generated = tray_ui::TrayWindow::new();
    let tray = program.create("TrayDemo").unwrap();
    let generated_tray = tray_ui::TrayDemo::new();
    let locale = LocaleContext::default();
    for component in [
        &interpreted as &dyn Component,
        &generated,
        &tray,
        &generated_tray,
    ] {
        component.set_locale_context(locale.clone());
    }
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

#[test]
fn media_assets_and_gif_clock_match_generated_components() {
    use std::time::{Duration, Instant};
    use xrack_runtime::ImagePlaybackQueue;

    let file = standalone_fixture(
        "examples/media/media.xr",
        "media",
        &[
            "assets/color-mask.png",
            "assets/landscape.png",
            "assets/nine-slice.svg",
            "assets/orbit.svg",
            "assets/pulse.gif",
            "assets/sprite-atlas.png",
        ],
    );
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
    assert_eq!(actual_model.remove(2), Some("C".into()));
    assert_eq!(generated_model.remove(2), Some("C".into()));
    assert_eq!(
        actual.control_at(&before_remove, &[0], Select(2)),
        generated.control_at(&expected_before_remove, &[0], Select(2)),
        "old scene after shrinking the model"
    );
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
    assert!(actual.control_at(
        &actual.render_shared(),
        &path,
        ControlAction::DelayButton(false)
    ));
    assert!(expected.control_at(
        &expected.render_shared(),
        &path,
        ControlAction::DelayButton(false)
    ));
    assert_eq!(actual.get("confirmed"), Ok(expected.get_confirmed().into()));
    assert_eq!(actual.get("activations"), Ok(Value::Int(1)));
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
fn native_file_dialog_snapshots_options_and_commits_results_before_callbacks() {
    use form_ui::FileDialogDemo;
    use xrack_runtime::{
        FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
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
        MessageButtons, MessageLevel, NativeDialogOptions, NativeDialogResult, StandardButtonKind,
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
            assert!(registration.deliver(NativeDialogResult::Button(button)));
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
    assert!(actual.control_at(&actual.render(), &[3], accept));
    assert!(expected.control_at(&expected.render(), &[3], accept));
    assert_eq!(actual.get("selected").unwrap(), Value::Date(date(4, 5)));
    assert_eq!(actual.get("accepts").unwrap(), Value::Int(1));

    actual.set("picker-open", true.into()).unwrap();
    expected.set_picker_open(true);
    assert_same_scene(&actual.render(), &expected.render());
    assert!(!actual.control_at(&original, &[3], accept));
    assert!(!expected.control_at(&expected_original, &[3], accept));
    let cancel = ControlAction::Dialog(DialogRole::Reject);
    assert!(actual.control_at(&actual.render(), &[3], cancel));
    assert!(expected.control_at(&expected.render(), &[3], cancel));
    assert_eq!(actual.get("cancels").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
    assert_eq!(actual.take_error(), None);
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
    }
    assert_eq!(actual.get("hour").unwrap(), Value::Int(0));
    assert_eq!(actual.get("changes").unwrap(), Value::Int(1));
    assert_same_scene(&actual.render(), &expected.render());
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
