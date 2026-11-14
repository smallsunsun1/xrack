use xrack_compiler::{lower, mlir, parse};
use xrack_interpreter::Program;
use xrack_runtime::Component;

const SOURCE: &str = include_str!("ui_log.xr");

#[test]
fn display_repeat_reuses_bindings_for_equal_rows_across_model_replacement() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::{Value, VecModel};

    let program = xrack_interpreter::load_mlir(
        &lower(&parse(SOURCE).unwrap(), "tests/ui_log.xr")
            .unwrap()
            .to_mlir(),
    )
    .unwrap();
    let interpreted = program.create("DisplayRepeatCache").unwrap();
    let mut generated = ui_log::DisplayRepeatCache::new();
    let actual_calls = Rc::new(RefCell::new(Vec::new()));
    let expected_calls = Rc::new(RefCell::new(Vec::new()));
    interpreted
        .on("describe", {
            let calls = actual_calls.clone();
            Rc::new(move |args| {
                let [Value::String(text)] = args else {
                    unreachable!()
                };
                calls.borrow_mut().push(text.clone());
                Ok(Some(text.clone().into()))
            })
        })
        .unwrap();
    generated.on_describe({
        let calls = expected_calls.clone();
        move |text| {
            calls.borrow_mut().push(text.clone());
            text
        }
    });
    let check = |generated: &ui_log::DisplayRepeatCache, labels: [&str; 2], calls: &[&str]| {
        for ui in [&interpreted as &dyn Component, generated] {
            let scene = ui.render_shared();
            let rows = &scene.children[0].children;
            assert_eq!(
                [rows[0].text("text"), rows[1].text("text")],
                labels.map(Some)
            );
        }
        assert_eq!(expected_calls.take(), calls);
        assert_eq!(actual_calls.take(), calls);
    };
    check(&generated, ["B0", "A1"], &["B0", "A1"]);
    let old_actual = interpreted.render_shared().children[0].children[0].row_key;
    let old_expected = generated.render_shared().children[0].children[0].row_key;

    let items = vec!["B".to_owned(), "A".to_owned()];
    interpreted
        .set("items", Value::StringModel(VecModel::from(items.clone())))
        .unwrap();
    generated.set_items(items);
    check(&generated, ["B0", "A1"], &[]);
    assert_ne!(
        interpreted.render_shared().children[0].children[0].row_key,
        old_actual
    );
    assert_ne!(
        generated.render_shared().children[0].children[0].row_key,
        old_expected
    );

    let actual_model = interpreted.string_model("items").unwrap();
    let expected_model = generated.model_items();
    assert!(actual_model.sort_by(String::cmp));
    assert!(expected_model.sort_by(String::cmp));
    check(&generated, ["A0", "B1"], &["A0", "B1"]);

    assert!(actual_model.set_row(1, "C".into()));
    assert!(expected_model.set_row(1, "C".into()));
    check(&generated, ["A0", "C1"], &["C1"]);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn standalone_mlir_validates_repeat_parameters_before_selecting_a_backend() {
    let doc = parse(
        r#"export component Rows inherits Window {
            for row-item[row-index] in ["entry"] : Text { text: row-item + row-index; }
        }"#,
    )
    .unwrap();
    let raw = mlir::lower(&doc, "repeat-parameters.xr").unwrap();
    let module = mlir::Module::parse(&raw).unwrap();
    assert!(xrack_compiler::codegen::generate(&module).is_ok());
    let instance = xrack_interpreter::load_mlir(&raw)
        .unwrap()
        .create("Rows")
        .unwrap();
    assert_eq!(
        instance.try_render().unwrap().children[0].children[0].text("text"),
        Some("entry0")
    );

    let repeat = raw
        .lines()
        .find(|line| line.contains("\"xr.repeat\""))
        .unwrap();
    let unused_item = parse(
        r#"export component Rows inherits Window {
            for row-item in ["entry"] : Text { text: "fixed"; }
        }"#,
    )
    .unwrap();
    let unused_item = mlir::lower(&unused_item, "repeat-parameters.xr").unwrap();
    let empty_item = unused_item.replace("item = \"row-item\"", "item = \"\"");
    assert_ne!(empty_item, unused_item);
    // Rename parameter uses along with their declarations so name lookup still succeeds.
    for (text, expected) in [
        (
            raw.replace("row-item", "row/item"),
            "invalid repeat item name",
        ),
        (
            raw.replace("row-index", "root"),
            "invalid repeat index name",
        ),
        (
            raw.replace("row-index", "row_item"),
            "repeat item and index must have different names",
        ),
        (empty_item, "invalid repeat item name"),
        (
            raw.replace(repeat, &format!("{repeat}\n{repeat}")),
            "element has more than one repeat",
        ),
    ] {
        assert_ne!(text, raw);
        let diagnostic = mlir::Module::parse(&text).unwrap_err();
        assert!(diagnostic.contains("repeat-parameters.xr"), "{diagnostic}");
        assert!(diagnostic.contains(expected), "{diagnostic}");
        assert_eq!(
            xrack_interpreter::load_mlir(&text).err(),
            Some(xrack_interpreter::Error::Invalid(diagnostic))
        );
    }
}

#[test]
fn generated_and_interpreted_handlers_emit_the_same_ordered_events() {
    let document = parse(SOURCE).unwrap();
    let ir = lower(&document, "tests/ui_log.xr").unwrap().to_mlir();
    assert!(ir.contains("!xr.loglevel"));
    assert_eq!(ir.matches(r#""xr.round""#).count(), 1);
    assert_eq!(ir.matches("\"xr.log\"").count(), 6);
    let module = mlir::Module::parse(&ir).unwrap();
    let mut interpreted = Program::from_module(&module)
        .unwrap()
        .create("LogDemo")
        .unwrap();
    let mut generated = ui_log::LogDemo::new();
    assert_eq!(generated.get_rounded(), 3);
    assert_eq!(generated.get_constant_rounded(), -3);
    assert_eq!(generated.get_pi(), std::f64::consts::PI);
    generated.set_measurement(-1.5);
    assert_eq!(generated.get_rounded(), -2);
    assert_eq!(
        interpreted.get("rounded").unwrap(),
        xrack_runtime::Value::Int(3)
    );
    interpreted
        .set("measurement", xrack_runtime::Value::Float(-1.5))
        .unwrap();
    assert_eq!(
        interpreted.get("rounded").unwrap(),
        xrack_runtime::Value::Int(-2)
    );
    for (input, expected) in [(1e30, i64::MAX), (-1e30, i64::MIN)] {
        generated.set_measurement(input);
        interpreted
            .set("measurement", xrack_runtime::Value::Float(input))
            .unwrap();
        assert_eq!(generated.get_rounded(), expected);
        assert_eq!(
            interpreted.get("rounded").unwrap(),
            xrack_runtime::Value::Int(expected)
        );
    }
    generated.set_diagnostic_level(xrack_runtime::LogLevel::Warn);
    interpreted
        .set(
            "diagnostic-level",
            xrack_runtime::Value::LogLevel(xrack_runtime::LogLevel::Warn),
        )
        .unwrap();

    fn events(component: &mut dyn Component) -> Vec<String> {
        use std::io::Write;
        use std::sync::{Arc, Mutex};

        struct Writer(Arc<Mutex<Vec<u8>>>);
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(bytes)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let output = Arc::new(Mutex::new(Vec::new()));
        let buffer = Arc::clone(&output);
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Writer(buffer.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let text = |node: xrack_runtime::Node| {
                node.find("increment")
                    .unwrap()
                    .text("text")
                    .unwrap()
                    .to_owned()
            };
            assert_eq!(text(component.render()), "Count { 0 }");
            assert!(component.dispatch("increment", "clicked"));
            assert_eq!(text(component.render()), "Count { 1 }");
        });
        let buffer = output.lock().unwrap();
        std::str::from_utf8(&buffer)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
    let generated = events(&mut generated);
    let interpreted = events(&mut interpreted);
    assert_eq!(generated, interpreted);
    let expected = [
        ("TRACE", "start", 13),
        ("WARN", "before: 0", 14),
        ("INFO", "after: 1", 16),
        ("WARN", "warning", 17),
        ("ERROR", "error", 18),
        ("INFO", "user callback", 9),
    ];
    assert_eq!(generated.len(), expected.len());
    for (event, (level, message, line)) in generated.iter().zip(expected) {
        assert!(event.trim_start().starts_with(level), "{event}");
        assert!(event.contains("xrack::ui"), "{event}");
        assert!(event.contains(message), "{event}");
        assert!(event.contains("component=\"LogDemo\""), "{event}");
        assert!(event.contains("ui_log.xr"), "{event}");
        assert!(event.contains(&format!("line={line} ")), "{event}");
    }
    let invalid_ir = ir.replacen("value = \"trace\"", "value = \"invalid\"", 1);
    assert!(
        mlir::process(&invalid_ir)
            .unwrap_err()
            .contains("literal spelling does not match")
    );
}

#[test]
fn logging_rejects_invalid_levels_arguments_and_pure_contexts() {
    assert!(parse(r#"component App inherits Window { title: @format("old"); }"#).is_err());
    for (source, expected) in [
        (
            "width: Math.round(2px) * 1px;",
            "Math.round requires an int or float",
        ),
        (
            r#"title: Strings.format("{}", Math.missing);"#,
            "unknown built-in value",
        ),
        (
            "callback run; run => { Log.write(LogLevel.nope, \"x\"); }",
            "unknown LogLevel value",
        ),
        (
            "callback run; run => { Log.debug(); }",
            "expects one message",
        ),
        (
            "callback run; run => { Log.write(LogLevel.info); }",
            "expects a level and a message",
        ),
        ("callback run; run => { Log.warn(true); }", "type mismatch"),
        (
            r#"callback run; run => { Log.write("info", "x"); }"#,
            "type mismatch: expected loglevel",
        ),
        (
            "callback run; run => { Log.unknown(\"x\"); }",
            "unknown built-in function",
        ),
        (
            "pure callback run; run => { Log.info(\"x\"); }",
            "not allowed in a pure callback",
        ),
        (
            "title: Log.error(\"x\");",
            "can only be used as a statement",
        ),
        (
            "title: Strings.format(\"{}\");",
            "more placeholders than arguments",
        ),
        (
            "title: Strings.format(\"x\", 1);",
            "more arguments than placeholders",
        ),
        (
            "title: Strings.format(\"{name}\", 1);",
            "accepts {}, {:.N}, {{ and }}",
        ),
        ("width: Math.clamp(10px, 0, 20px);", "share a numeric type"),
        (
            "width: Math.sin(12px) * 1px;",
            "expected float, found length",
        ),
        ("background: Colors.rgb(1, 2);", "expects 3 arguments"),
        (
            "background: Colors.rgba(1, 2, 3, true);",
            "expected float, found bool",
        ),
        ("title: I18n.locale(1);", "expects 0 arguments"),
        (
            "title: I18n.format-number(2px, 2);",
            "I18n.format-number requires an int or float",
        ),
        (
            "title: I18n.format-number(2, 1.5);",
            "expected int, found float",
        ),
        (
            "width: I18n.parse-number(42, 0) * 1px;",
            "expected string, found int",
        ),
        (
            r#"title: Model.at(["one"], 0, 42);"#,
            "expected string, found int",
        ),
        (r#"title: Strings.contains(1, "s");"#, "type mismatch"),
        (
            "width: Model.count(3) * 1px;",
            "requires a string or record model",
        ),
        (r#"title: Strings.format("{:.21}", 1);"#, "between 0 and 20"),
        (
            r#"title: Strings.format("{:.2}", "a");"#,
            "requires an int or float",
        ),
        (
            "title: Strings.format(root.title);",
            "expects a string literal",
        ),
    ] {
        let input = format!("component App inherits Window {{ {source} }}");
        let document = parse(&input).unwrap();
        let error = lower(&document, "log-errors.xr").unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn standard_library_preserves_units_unicode_precision_and_live_model_counts() {
    use xrack_runtime::{Length, Value};

    let document = parse(SOURCE).unwrap();
    let ir = lower(&document, "tests/ui_log.xr").unwrap().to_mlir();
    let module = mlir::Module::parse(&ir).unwrap();
    let interpreted = Program::from_module(&module)
        .unwrap()
        .create("BuiltinDemo")
        .unwrap();
    let mut generated = ui_log::BuiltinDemo::new();
    for (name, actual, expected) in [
        (
            "round-down",
            Value::Int(generated.get_round_down()),
            Value::Int(-3),
        ),
        (
            "round-up",
            Value::Int(generated.get_round_up()),
            Value::Int(-2),
        ),
        (
            "minimum",
            Value::Float(generated.get_minimum()),
            Value::Float(-2.5),
        ),
        (
            "maximum",
            Value::Float(generated.get_maximum()),
            Value::Float(-1.0),
        ),
        (
            "width-limit",
            Value::Length(generated.get_width_limit()),
            Value::Length(Length(96.0)),
        ),
        (
            "magnitude",
            Value::Int(generated.get_magnitude()),
            Value::Int(i64::MAX),
        ),
        (
            "percent-magnitude",
            Value::Float(generated.get_percent_magnitude()),
            Value::Float(0.25),
        ),
        (
            "trimmed",
            Value::String(generated.get_trimmed()),
            Value::String("中文👩‍💻é".into()),
        ),
        (
            "empty",
            Value::Bool(generated.get_empty()),
            Value::Bool(false),
        ),
        (
            "matches",
            Value::Bool(generated.get_matches()),
            Value::Bool(true),
        ),
        (
            "characters",
            Value::Int(generated.get_characters()),
            Value::Int(4),
        ),
        (
            "fixed",
            Value::String(generated.get_fixed()),
            Value::String("-2.50 9007199254740993.000 {8}".into()),
        ),
    ] {
        assert_eq!(actual, expected, "{name}");
        assert_eq!(interpreted.get(name).unwrap(), expected, "{name}");
    }

    let names = xrack_runtime::VecModel::from(vec!["one".to_owned(), "two".to_owned()]);
    generated.bind_model_names(names.clone());
    let interpreted_names = interpreted.string_model("names").unwrap();
    let rows = xrack_runtime::VecModel::from(vec![ui_log::BuiltinRow {
        label: "one".into(),
    }]);
    generated.bind_model_rows(rows.clone());
    let interpreted_rows = interpreted.record_model("rows").unwrap();
    use std::{cell::Cell, rc::Rc};
    let generated_reads = Rc::new(Cell::new(0));
    let interpreted_reads = Rc::new(Cell::new(0));
    {
        let reads = Rc::clone(&generated_reads);
        generated.on_observe_count(move |value| {
            reads.set(reads.get() + 1);
            value
        });
    }
    {
        let reads = Rc::clone(&interpreted_reads);
        interpreted
            .on(
                "observe-count",
                Rc::new(move |args| {
                    reads.set(reads.get() + 1);
                    Ok(Some(args[0].clone()))
                }),
            )
            .unwrap();
    }
    assert_eq!(generated.get_name_count(), 2);
    assert_eq!(interpreted.get("name-count").unwrap(), Value::Int(2));
    assert_eq!(generated.get_row_count(), 1);
    assert_eq!(interpreted.get("row-count").unwrap(), Value::Int(1));
    assert!(names.set_row(0, "edited".into()));
    assert!(interpreted_names.set_row(0, "edited".into()));
    assert_eq!(generated.get_name_count(), 2);
    assert_eq!(interpreted.get("name-count").unwrap(), Value::Int(2));
    assert_eq!(generated_reads.get(), 1);
    assert_eq!(interpreted_reads.get(), 1);
    names.push("three".into());
    interpreted_names.push("three".into());
    rows.push(ui_log::BuiltinRow {
        label: "two".into(),
    });
    interpreted_rows
        .model()
        .push(interpreted_rows.model().row(0).unwrap());
    assert_eq!(generated.get_name_count(), 3);
    assert_eq!(interpreted.get("name-count").unwrap(), Value::Int(3));
    assert_eq!(generated.get_row_count(), 2);
    assert_eq!(interpreted.get("row-count").unwrap(), Value::Int(2));
    names.remove(0);
    interpreted_names.remove(0);
    assert_eq!(generated.get_name_count(), 2);
    assert_eq!(interpreted.get("name-count").unwrap(), Value::Int(2));
    assert_eq!(generated_reads.get(), 3);
    assert_eq!(interpreted_reads.get(), 3);
}

#[test]
fn signed_zero_math_matches_with_and_without_mlir_folding() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::Value;

    let doc = parse(SOURCE).unwrap();
    let raw = xrack_compiler::mlir::lower_module(&doc, "tests/ui_log.xr").unwrap();
    let source = raw.to_mlir();
    let optimized = xrack_compiler::mlir::process(&source).unwrap();
    let component = optimized
        .operations()
        .find(|op| op.attribute("sym_name") == Some("SignedZeroMath"))
        .unwrap();
    for property in component.children().filter(|op| {
        op.name() == "xr.property" && op.attribute("name").unwrap().starts_with("fixed-")
    }) {
        assert_eq!(
            property
                .operand(0)
                .unwrap()
                .defining_operation()
                .unwrap()
                .name(),
            "xr.literal"
        );
    }

    let from_raw = Program::from_module(&raw)
        .unwrap()
        .create("SignedZeroMath")
        .unwrap();
    let from_text = xrack_interpreter::load_mlir(&optimized.to_mlir())
        .unwrap()
        .create("SignedZeroMath")
        .unwrap();
    let mut compiled = ui_log::SignedZeroMath::new();
    let raw_reads = Rc::new(RefCell::new(Vec::new()));
    let text_reads = Rc::new(RefCell::new(Vec::new()));
    let compiled_reads = Rc::new(RefCell::new(Vec::new()));
    for (component, reads) in [(&from_raw, &raw_reads), (&from_text, &text_reads)] {
        for (name, value) in [("positive", 0.0), ("negative", -0.0)] {
            let reads = reads.clone();
            component
                .on(
                    name,
                    Rc::new(move |_| {
                        reads.borrow_mut().push(name);
                        Ok(Some(Value::Float(value)))
                    }),
                )
                .unwrap();
        }
    }
    compiled.on_positive({
        let reads = compiled_reads.clone();
        move || {
            reads.borrow_mut().push("positive");
            0.0
        }
    });
    compiled.on_negative({
        let reads = compiled_reads.clone();
        move || {
            reads.borrow_mut().push("negative");
            -0.0
        }
    });

    for (name, generated, expected) in [
        ("fixed-minimum", compiled.get_fixed_minimum(), -0.0_f64),
        ("fixed-maximum", compiled.get_fixed_maximum(), 0.0),
        ("fixed-low", compiled.get_fixed_low(), -0.0),
        ("fixed-high", compiled.get_fixed_high(), 0.0),
        ("sampled-minimum", compiled.get_sampled_minimum(), -0.0),
        ("sampled-maximum", compiled.get_sampled_maximum(), 0.0),
        ("sampled-low", compiled.get_sampled_low(), -0.0),
        ("sampled-high", compiled.get_sampled_high(), 0.0),
    ] {
        assert_eq!(generated.to_bits(), expected.to_bits(), "generated {name}");
        for component in [&from_raw, &from_text] {
            let Value::Float(value) = component.get(name).unwrap() else {
                panic!("expected a float for {name}");
            };
            assert_eq!(value.to_bits(), expected.to_bits(), "interpreted {name}");
        }
    }
    let expected = [
        "positive", "negative", "negative", "positive", "positive", "negative", "positive",
        "negative",
    ];
    for reads in [&raw_reads, &text_reads, &compiled_reads] {
        assert_eq!(*reads.borrow(), expected);
    }
}

#[test]
fn signed_zero_writes_invalidate_bindings_in_both_backends() {
    use xrack_runtime::{Length, Value};

    let raw = mlir::lower_module(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let interpret = |module: &mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("SignedZeroProperties")
            .unwrap()
    };
    let unoptimized = interpret(&raw);
    let interpreted = [unoptimized, interpret(&raw.optimize().unwrap())];
    let mut generated = ui_log::SignedZeroProperties::new();
    let check = |generated: &ui_log::SignedZeroProperties, text: &str| {
        assert_eq!(generated.get_display(), text);
        assert!(generated.get_equal());
        assert_eq!(
            generated.render_shared().children[1].text("text"),
            Some(text)
        );
        for ui in &interpreted {
            assert_eq!(ui.get("display"), Ok(text.to_owned().into()));
            assert_eq!(ui.get("equal"), Ok(Value::Bool(true)));
            assert_eq!(
                ui.try_render().unwrap().children[1].text("text"),
                Some(text)
            );
        }
    };
    check(&generated, "0 inf 0");

    for (value, texts) in [
        (-0.0, ["-0 inf 0", "-0 -inf 0", "-0 -inf -0"]),
        (0.0, ["0 -inf -0", "0 inf -0", "0 inf 0"]),
        (-0.0, ["-0 inf 0", "-0 -inf 0", "-0 -inf -0"]),
    ] {
        let writes: [(&str, Value, fn(&mut ui_log::SignedZeroProperties, f64)); 3] = [
            ("number", Value::Float(value), |ui, value| {
                ui.set_number(value)
            }),
            ("extent", Value::Length(Length(value)), |ui, value| {
                ui.set_extent(Length(value))
            }),
            ("level", Value::Float(value), |ui, value| {
                ui.set_level(value)
            }),
        ];
        for ((name, input, write), text) in writes.into_iter().zip(texts) {
            write(&mut generated, value);
            for ui in &interpreted {
                ui.set(name, input.clone()).unwrap();
            }
            check(&generated, text);
        }
        // Writing the same bit pattern keeps the cached scene.
        let previous = generated.render_shared();
        let scenes = interpreted.each_ref().map(|ui| ui.render_shared());
        generated.set_number(value);
        for (ui, scene) in interpreted.iter().zip(&scenes) {
            ui.set("number", Value::Float(value)).unwrap();
            assert!(std::rc::Rc::ptr_eq(&ui.render_shared(), scene));
        }
        assert!(std::rc::Rc::ptr_eq(&generated.render_shared(), &previous));
    }

    generated.reset_number();
    generated.reset_extent();
    generated.reset_level();
    for ui in &interpreted {
        for name in ["number", "extent", "level"] {
            ui.reset(name).unwrap();
        }
    }
    check(&generated, "0 inf 0");
}

#[test]
fn split_handle_render_stops_at_a_failed_binding_and_retries_it() {
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    use xrack_runtime::Value;

    let document = parse(SOURCE).unwrap();
    let raw = mlir::lower_module(&document, "tests/ui_log.xr").unwrap();
    let program = Program::from_module(&raw).unwrap();
    let optimized = raw.optimize().unwrap();
    let optimized = xrack_interpreter::load_mlir(&optimized.to_mlir()).unwrap();
    let labels = |scene: &xrack_runtime::Node| {
        let split = scene.find("split").unwrap();
        split.children[3..]
            .iter()
            .map(|handle| {
                handle.children[0].children[0]
                    .text("text")
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };

    let mut generated = ui_log::SplitHandleRetry::new();
    let calls = Rc::new(RefCell::new(Vec::new()));
    generated.on_handle_label({
        let calls = calls.clone();
        move |index| {
            let mut calls = calls.borrow_mut();
            calls.push(index);
            assert!(calls.len() != 1, "first handle failed");
            format!("handle {index}")
        }
    });
    assert!(catch_unwind(AssertUnwindSafe(|| generated.render_shared())).is_err());
    assert_eq!(*calls.borrow(), [0]);
    let expected = labels(&generated.render_shared());
    assert_eq!(expected, ["handle 0", "handle 1"]);
    assert_eq!(*calls.borrow(), [0, 0, 1]);

    for program in [program, optimized] {
        let interpreted = program.create("SplitHandleRetry").unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        interpreted
            .on("handle-label", {
                let calls = calls.clone();
                Rc::new(move |args| {
                    let [Value::Int(index)] = args else {
                        panic!("expected the handle index");
                    };
                    let mut calls = calls.borrow_mut();
                    calls.push(*index);
                    if calls.len() == 1 {
                        return Err("first handle failed".into());
                    }
                    Ok(Some(format!("handle {index}").into()))
                })
            })
            .unwrap();
        assert_eq!(
            interpreted.try_render().unwrap_err(),
            xrack_interpreter::Error::Evaluation("first handle failed".into()),
        );
        assert_eq!(*calls.borrow(), [0]);
        let scene = interpreted.try_render().unwrap();
        assert_eq!(labels(&scene), expected);
        assert!(Rc::ptr_eq(&scene, &interpreted.try_render().unwrap()));
        assert_eq!(*calls.borrow(), [0, 0, 1]);
    }
}

#[test]
fn split_fill_keeps_its_declared_slot_while_the_pane_is_hidden() {
    let mut generated = ui_log::ConditionalFillSplit::new();
    let raw = mlir::lower_module(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let interpret = |module: &mlir::Module| {
        Program::from_module(module)
            .unwrap()
            .create("ConditionalFillSplit")
            .unwrap()
    };
    let mut unoptimized = interpret(&raw);
    let optimized = raw.optimize().unwrap();
    let mut from_text = xrack_interpreter::load_mlir(&optimized.to_mlir())
        .unwrap()
        .create("ConditionalFillSplit")
        .unwrap();

    let check = |scene: &xrack_runtime::Node, visible| {
        let split = scene.find("split").unwrap();
        assert!(split.has_split_fill());
        assert_eq!(split.split_fill_slot(), Some(1));
        assert_eq!(split.children[1].is_visible(), visible);
        assert_eq!(
            split
                .split_panes()
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            if visible { vec![0, 1, 2] } else { vec![0, 2] }
        );
    };
    let sizes = [Some(45.0), None, Some(80.0)];
    for (step, show) in [true, false, true].into_iter().enumerate() {
        generated.set_show_center(show);
        for ui in [&mut unoptimized, &mut from_text] {
            ui.set("show-center", show.into()).unwrap();
        }
        for ui in [
            &mut generated as &mut dyn Component,
            &mut unoptimized,
            &mut from_text,
        ] {
            let scene = ui.render_shared();
            check(&scene, show);
            if step == 1 {
                assert!(ui.resize_split_sizes_at(&scene, &[0], &[(0, 45.0), (2, 80.0)]));
                assert!(!ui.resize_split_sizes_at(&scene, &[0], &[(0, 45.0), (2, 80.0)]));
            }
            if step > 0 {
                assert_eq!(
                    ui.render_shared()
                        .find("split")
                        .unwrap()
                        .split_sizes
                        .as_deref(),
                    Some(sizes.as_slice())
                );
            }
        }
    }
    assert_eq!(unoptimized.take_error(), None);
    assert_eq!(from_text.take_error(), None);
}

#[test]
fn standalone_mlir_host_split_methods_use_public_ids() {
    let raw = mlir::lower(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let text = raw
        .replace(
            "id = \"split\", key = \"split\", kind = \"SplitView\"",
            "id = \"split\", key = \"internal-split\", kind = \"SplitView\"",
        )
        .replace(
            "owner = \"split\", value = \"\"",
            "owner = \"internal-split\", value = \"\"",
        )
        .replace("{owner = \"split\"}", "{owner = \"internal-split\"}");
    assert_ne!(raw, text);
    let module = mlir::Module::parse(&text).unwrap();
    assert!(xrack_compiler::codegen::generate(&module).is_ok());
    let interpreted = xrack_interpreter::load_mlir(&text)
        .unwrap()
        .create("ConditionalFillSplit")
        .unwrap();
    let mut generated = ui_log::ConditionalFillSplit::new();
    let layout = generated.save_split_state("split").unwrap();
    assert_eq!(interpreted.save_split_state("split"), Ok(layout.clone()));
    assert!(interpreted.save_split_state("internal-split").is_err());
    assert_eq!(generated.save_split_state("internal-split"), None);

    let updated = xrack_runtime::encode_split_sizes(
        &["@0", "center", "@2"],
        Some(&[Some(45.0), None, Some(80.0)]),
    );
    assert!(generated.restore_split_state("split", &updated));
    assert_eq!(interpreted.restore_split_state("split", &updated), Ok(true));
    assert_eq!(interpreted.get("layout"), Ok(generated.get_layout().into()));
    generated.set_saved_layout(layout.clone());
    interpreted
        .set("saved-layout", layout.clone().into())
        .unwrap();
    generated.invoke_restore();
    interpreted.invoke("restore", &[]).unwrap();
    assert_eq!(generated.get_layout(), layout);
    assert_eq!(interpreted.get("layout"), Ok(layout.into()));
}

#[test]
fn expanded_standard_library_matches_in_both_execution_paths() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{Color, Length, LocaleContext, TranslationCatalog, Value};

    let module = lower(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let locale = LocaleContext::default();
    let mut aot = ui_log::StandardLibraryDemo::new_with_locale(locale.clone());
    let interpreter = Program::from_module(&module)
        .unwrap()
        .create_with_locale("StandardLibraryDemo", locale.clone())
        .unwrap();
    for (name, actual, expected) in [
        ("magnitude", aot.get_magnitude(), 13_f64.sqrt()),
        ("trig", aot.get_trig(), 2.0),
        ("degrees", aot.get_degrees(), 180.0),
        ("logarithms", aot.get_logarithms(), 4.0),
        ("hypot", aot.get_hypot(), 5.0),
        ("constants", aot.get_constants(), 1.0),
        ("sign", aot.get_sign(), 1.0),
    ] {
        let Value::Float(interpreted) = interpreter.get(name).unwrap() else {
            panic!("{name} should be a float");
        };
        assert!((actual - expected).abs() < 1e-12, "{name}: {actual}");
        assert_eq!(actual, interpreted, "{name}");
    }
    for (name, actual, expected) in [
        ("mod-int", Value::Int(aot.get_mod_int()), Value::Int(1)),
        ("mod-zero", Value::Int(aot.get_mod_zero()), Value::Int(0)),
        (
            "mod-length",
            Value::Length(aot.get_mod_length()),
            Value::Length(Length(1.0)),
        ),
        ("finite", Value::Bool(aot.get_finite()), Value::Bool(true)),
        (
            "replaced",
            Value::String(aot.get_replaced()),
            Value::String("XX".into()),
        ),
        ("ends", Value::Bool(aot.get_ends()), Value::Bool(true)),
        (
            "rgba",
            Value::Color(aot.get_rgba()),
            Value::Color(Color(0x0080ff80)),
        ),
        (
            "mixed",
            Value::Color(aot.get_mixed()),
            Value::Color(Color(0x80808080)),
        ),
    ] {
        assert_eq!(actual, expected, "{name}");
        assert_eq!(interpreter.get(name).unwrap(), expected, "{name}");
    }
    aot.set_sample(-1.0);
    interpreter.set("sample", Value::Float(-1.0)).unwrap();
    assert!(!aot.get_finite());
    assert_eq!(interpreter.get("finite").unwrap(), Value::Bool(false));
    for (input, integer, float, valid_int, valid_float) in [
        (" 42 ", 42, 42.0, true, true),
        ("1.25e2", -1, 125.0, false, true),
        ("NaN", -1, -1.0, false, false),
        ("1,5", -1, -1.0, false, false),
    ] {
        aot.set_input(input.into());
        interpreter
            .set("input", Value::String(input.into()))
            .unwrap();
        assert_eq!(aot.get_parsed_int(), integer);
        assert_eq!(aot.get_parsed_float(), float);
        assert_eq!(aot.get_valid_int(), valid_int);
        assert_eq!(aot.get_valid_float(), valid_float);
        assert_eq!(interpreter.get("parsed-int").unwrap(), Value::Int(integer));
        assert_eq!(
            interpreter.get("parsed-float").unwrap(),
            Value::Float(float)
        );
        assert_eq!(
            interpreter.get("valid-int").unwrap(),
            Value::Bool(valid_int)
        );
        assert_eq!(
            interpreter.get("valid-float").unwrap(),
            Value::Bool(valid_float)
        );
    }

    let generated_fallbacks = Rc::new(Cell::new(0));
    let interpreted_fallbacks = Rc::new(Cell::new(0));
    let observe = |count: Rc<Cell<i32>>| {
        move || {
            count.set(count.get() + 1);
            "fallback".to_owned()
        }
    };
    aot.on_missing(observe(generated_fallbacks.clone()));
    let missing = observe(interpreted_fallbacks.clone());
    interpreter
        .on(
            "missing",
            Rc::new(move |_| Ok(Some(Value::String(missing())))),
        )
        .unwrap();
    let aot_model = aot.model_names();
    let interpreted_model = interpreter.string_model("names").unwrap();
    assert_eq!(aot.get_selected_name(), "one");
    assert_eq!(
        interpreter.get("selected-name").unwrap(),
        Value::String("one".into())
    );
    assert_eq!(generated_fallbacks.get(), 0);
    assert_eq!(interpreted_fallbacks.get(), 0);
    assert_eq!(aot.get_selected_row().label, "record");
    let Value::Record(row) = interpreter.get("selected-row").unwrap() else {
        panic!("record row expected");
    };
    assert_eq!(row.field("label"), Some(&Value::String("record".into())));
    assert_eq!(aot.get_selected_label(), "record");
    assert_eq!(
        interpreter.get("selected-label"),
        Ok("record".to_owned().into())
    );
    let interpreted_rows = interpreter.record_model("rows").unwrap();
    let bad_row = row.with_field(&["label".into()], 123_i64.into()).unwrap();
    interpreted_rows.model().push(bad_row.clone());
    aot.model_rows().push(ui_log::BuiltinRow {
        label: "second".into(),
    });
    assert_eq!(
        interpreter.get("selected-label"),
        Ok(aot.get_selected_label().into())
    );
    assert_eq!(
        interpreter.get("selected-label"),
        Ok("record".to_owned().into())
    );
    interpreter.set("index", 1_i64.into()).unwrap();
    aot.set_index(1);
    let error = interpreter.get("selected-label").unwrap_err();
    assert!(
        error.to_string().contains("model BuiltinRow[1].label"),
        "{error}"
    );
    assert_eq!(aot.get_selected_label(), "second");
    assert!(
        interpreted_rows.model().set_row(
            1,
            row.with_field(&["label".into()], "second".to_owned().into())
                .unwrap()
        )
    );
    assert_eq!(
        interpreter.get("selected-label"),
        Ok(aot.get_selected_label().into())
    );
    assert!(interpreted_rows.model().set_row(1, bad_row));
    interpreter.set("index", 2_i64.into()).unwrap();
    aot.set_index(2);
    assert_eq!(aot.get_selected_label(), "fallback");
    assert_eq!(
        interpreter.get("selected-label"),
        Ok(aot.get_selected_label().into())
    );
    interpreted_rows.model().remove(1).unwrap();
    aot.model_rows().remove(1).unwrap();
    interpreter.set("index", 0_i64.into()).unwrap();
    aot.set_index(0);
    assert!(aot_model.set_row(0, "edited".into()));
    assert!(interpreted_model.set_row(0, "edited".into()));
    assert_eq!(aot.get_selected_name(), "edited");
    assert_eq!(
        interpreter.get("selected-name").unwrap(),
        Value::String("edited".into())
    );
    aot.set_index(-1);
    interpreter.set("index", Value::Int(-1)).unwrap();
    assert_eq!(aot.get_selected_name(), "fallback");
    assert_eq!(
        interpreter.get("selected-name").unwrap(),
        Value::String("fallback".into())
    );
    assert_eq!(generated_fallbacks.get(), 1);
    assert_eq!(interpreted_fallbacks.get(), 1);
    assert_eq!(aot.get_selected_row().label, "fallback");
    assert!(!aot.get_names_empty());
    assert_eq!(interpreter.get("names-empty").unwrap(), Value::Bool(false));
    aot.set_names(vec![]);
    interpreter.set("names", Value::StringList(vec![])).unwrap();
    assert!(aot.get_names_empty());
    assert_eq!(interpreter.get("names-empty").unwrap(), Value::Bool(true));
    aot.set_index(0);
    interpreter.set("index", Value::Int(0)).unwrap();
    assert_eq!(aot.get_selected_name(), "fallback");
    assert_eq!(
        interpreter.get("selected-name").unwrap(),
        Value::String("fallback".into())
    );
    aot.model_names().push("new".into());
    interpreter
        .string_model("names")
        .unwrap()
        .push("new".into());
    assert_eq!(aot.get_selected_name(), "new");
    assert_eq!(
        interpreter.get("selected-name").unwrap(),
        Value::String("new".into())
    );

    let catalog = TranslationCatalog::from_owned(
        "en-US",
        [
            ("en-US".into(), "greeting = Hello".into()),
            ("fr".into(), "greeting = Bonjour".into()),
        ]
        .into(),
    )
    .unwrap();
    let localized = Program::from_module_with_translations(&module, Some(catalog))
        .unwrap()
        .create_with_locale("StandardLibraryDemo", locale.clone())
        .unwrap();
    for (update, requested, negotiated) in [
        (None, "und", "en-US"),
        (Some("fr-CA"), "fr-CA", "fr"),
        (Some("zh-CN"), "zh-CN", "en-US"),
    ] {
        if let Some(update) = update {
            locale.set_locale(update).unwrap();
        }
        assert_eq!(aot.get_language(), requested);
        assert_eq!(
            interpreter.get("language").unwrap(),
            Value::String(requested.into())
        );
        assert_eq!(
            localized.get("language").unwrap(),
            Value::String(negotiated.into())
        );
    }
}

#[test]
fn localized_numbers_update_in_both_execution_paths() {
    use xrack_runtime::{LocaleContext, TranslationCatalog, Value};

    let module = lower(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let locale = LocaleContext::default();
    let mut aot = ui_log::LocalizedNumberDemo::new_with_locale(locale.clone());
    let interpreter = Program::from_module(&module)
        .unwrap()
        .create_with_locale("LocalizedNumberDemo", locale.clone())
        .unwrap();
    let catalog =
        TranslationCatalog::from_owned("fr", [("fr".into(), "title = Test".into())].into())
            .unwrap();
    let with_catalog = Program::from_module_with_translations(&module, Some(catalog))
        .unwrap()
        .create_with_locale("LocalizedNumberDemo", locale.clone())
        .unwrap();
    assert_eq!(aot.get_parsed(), -99.0);
    assert_eq!(interpreter.get("parsed").unwrap(), Value::Float(-99.0));
    assert_eq!(with_catalog.get("parsed").unwrap(), Value::Float(1.5));

    for (language, input, display, parsed, round_trip) in [
        ("en-US", " +1,234.5 ", "1,234.50", 1234.5, "1,234.5"),
        (
            "fr-FR",
            "1\u{202f}234,5",
            "1\u{202f}234,50",
            1234.5,
            "1\u{202f}234,5",
        ),
        (
            "fr-CA",
            "1\u{a0}234,5",
            "1\u{a0}234,50",
            1234.5,
            "1\u{a0}234,5",
        ),
        ("de-DE", "1.234,5", "1.234,50", 1234.5, "1.234,5"),
        ("de-CH", "1'234.5", "1'234.50", 1234.5, "1'234.5"),
        ("hi-IN", "12,34,567.5", "1,234.50", 1234567.5, "12,34,567.5"),
        (
            "ar-EG",
            "\u{61c}-١٬٢٣٤٫٥",
            "١٬٢٣٤٫٥٠",
            -1234.5,
            "\u{61c}-١٬٢٣٤٫٥",
        ),
        ("en-US", "1,2.5", "1,234.50", -99.0, "-99.0"),
        ("fr-FR", "1.5", "1\u{202f}234,50", -99.0, "-99,0"),
        ("fr-FR", "NaN", "1\u{202f}234,50", -99.0, "-99,0"),
        ("fr-FR", "1,2\u{202f}3", "1\u{202f}234,50", -99.0, "-99,0"),
    ] {
        locale.set_locale(language).unwrap();
        aot.set_input(input.into());
        interpreter
            .set("input", Value::String(input.into()))
            .unwrap();
        with_catalog
            .set("input", Value::String(input.into()))
            .unwrap();
        assert_eq!(aot.get_parsed(), parsed, "{language}: {input}");
        assert_eq!(aot.get_display(), display, "{language}");
        assert_eq!(aot.get_round_trip(), round_trip, "{language}: {input}");
        for ui in [&interpreter, &with_catalog] {
            assert_eq!(
                ui.get("parsed").unwrap(),
                Value::Float(parsed),
                "{language}: {input}"
            );
            assert_eq!(
                ui.get("display").unwrap(),
                Value::String(display.into()),
                "{language}"
            );
            assert_eq!(
                ui.get("round-trip").unwrap(),
                Value::String(round_trip.into()),
                "{language}"
            );
        }
    }

    aot.set_input("1,5".into());
    interpreter
        .set("input", Value::String("1,5".into()))
        .unwrap();
    assert_eq!(aot.get_parsed(), 1.5);
    assert_eq!(interpreter.get("parsed").unwrap(), Value::Float(1.5));
    // No input property changes here. Locale is the only dependency invalidated.
    locale.set_locale("en-US").unwrap();
    assert_eq!(aot.get_parsed(), -99.0);
    assert_eq!(interpreter.get("parsed").unwrap(), Value::Float(-99.0));
    assert_eq!(aot.get_large_number(), "9,007,199,254,740,993");
    assert_eq!(
        interpreter.get("large-number").unwrap(),
        Value::String("9,007,199,254,740,993".into())
    );
    for (decimals, display) in [(-1, "1,234"), (30, "1,234.50000000000000000000")] {
        aot.set_decimals(decimals);
        interpreter.set("decimals", Value::Int(decimals)).unwrap();
        assert_eq!(aot.get_display(), display);
        assert_eq!(
            interpreter.get("display").unwrap(),
            Value::String(display.into())
        );
    }
}

#[test]
fn date_arithmetic_handles_month_ends_leap_years_and_invalid_input_in_both_backends() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{Date, Value};

    let module = lower(&parse(SOURCE).unwrap(), "tests/ui_log.xr").unwrap();
    let interpreted = Program::from_module(&module)
        .unwrap()
        .create("DateArithmeticDemo")
        .unwrap();
    let mut generated = ui_log::DateArithmeticDemo::new();

    let date = |year, month, day| Date { year, month, day };
    let fallback = date(2001, 1, 1);
    let generated_calls = Rc::new(Cell::new(0));
    let interpreted_calls = Rc::new(Cell::new(0));
    let count = generated_calls.clone();
    generated.on_missing_date(move || {
        count.set(count.get() + 1);
        fallback
    });
    let count = interpreted_calls.clone();
    interpreted
        .on(
            "missing-date",
            Rc::new(move |_| {
                count.set(count.get() + 1);
                Ok(Some(Value::Date(fallback)))
            }),
        )
        .unwrap();

    for (input, offset, weekday, day_result, month_result) in [
        (
            date(2024, 1, 31),
            1,
            3,
            Some(date(2024, 2, 1)),
            Some(date(2024, 2, 29)),
        ),
        (
            date(2023, 1, 31),
            1,
            2,
            Some(date(2023, 2, 1)),
            Some(date(2023, 2, 28)),
        ),
        (
            date(2024, 3, 31),
            -1,
            7,
            Some(date(2024, 3, 30)),
            Some(date(2024, 2, 29)),
        ),
        (
            date(2024, 2, 29),
            12,
            4,
            Some(date(2024, 3, 12)),
            Some(date(2025, 2, 28)),
        ),
        (
            date(2024, 2, 28),
            1,
            3,
            Some(date(2024, 2, 29)),
            Some(date(2024, 3, 28)),
        ),
        (
            date(2000, 2, 28),
            1,
            1,
            Some(date(2000, 2, 29)),
            Some(date(2000, 3, 28)),
        ),
        (
            date(1900, 2, 28),
            1,
            3,
            Some(date(1900, 3, 1)),
            Some(date(1900, 3, 28)),
        ),
        (
            date(2023, 12, 31),
            1,
            7,
            Some(date(2024, 1, 1)),
            Some(date(2024, 1, 31)),
        ),
        (date(1900, 2, 29), 1, 0, None, None),
        (date(2024, 13, 1), 1, 0, None, None),
        (Date::MIN, -1, 1, None, None),
        (Date::MAX, 1, 5, None, None),
        (date(2024, 1, 1), i64::MIN, 1, None, None),
        (date(2024, 1, 1), i64::MAX, 1, None, None),
        (
            date(2024, 1, 1),
            0,
            1,
            Some(date(2024, 1, 1)),
            Some(date(2024, 1, 1)),
        ),
    ] {
        // Read the bindings again after host-side input changes in both execution modes.
        generated.set_selected_date(input);
        generated.set_offset(offset);
        interpreted
            .set("selected-date", Value::Date(input))
            .unwrap();
        interpreted.set("offset", Value::Int(offset)).unwrap();
        let before = generated_calls.get();
        assert_eq!(interpreted_calls.get(), before);
        let expected_calls =
            before + i32::from(day_result.is_none()) + i32::from(month_result.is_none());
        let day_result = day_result.unwrap_or(fallback);
        let month_result = month_result.unwrap_or(fallback);
        assert_eq!(generated.get_valid_date(), weekday != 0, "{input}");
        assert_eq!(generated.get_day_of_week(), weekday, "{input}");
        assert_eq!(generated.get_day_result(), day_result, "{input}");
        assert_eq!(generated.get_month_result(), month_result, "{input}");
        for (name, expected) in [
            ("valid-date", Value::Bool(weekday != 0)),
            ("day-of-week", Value::Int(weekday)),
            ("day-result", Value::Date(day_result)),
            ("month-result", Value::Date(month_result)),
        ] {
            assert_eq!(interpreted.get(name).unwrap(), expected, "{input}: {name}");
        }
        assert_eq!(generated_calls.get(), expected_calls);
        assert_eq!(interpreted_calls.get(), expected_calls);
    }
}
