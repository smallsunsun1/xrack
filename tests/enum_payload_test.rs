use enum_payload_ui::{Details, EnumPayloadDemo, Job};
use xrack_interpreter::{Instance, Program};
use xrack_runtime::{Component, RecordValue, Value};

const SOURCE: &str = include_str!("enum_payload.xr");

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn interpreted() -> TestResult<Instance> {
    let preview = xrack_compiler::prepare_preview(SOURCE, "enum_payload.xr", Default::default())?;
    Ok(Program::from_module(&preview.module)?.create("EnumPayloadDemo")?)
}

fn record(name: &str, fields: &[(&str, Value)]) -> Value {
    Value::Record(RecordValue::new(
        name,
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    ))
}

fn job(tag: i64, payload: &[(&str, Value)]) -> Value {
    let mut fields = vec![("discriminant", Value::Int(tag))];
    fields.extend_from_slice(payload);
    record("Job", &fields)
}

#[test]
fn rust_enum_payload_and_interpreter_have_the_same_state_and_output() -> TestResult {
    let mut aot = EnumPayloadDemo::new();
    let mut interpreter = interpreted()?;

    assert!(matches!(Job::default(), Job::Pending(ref text) if text.is_empty()));
    assert_eq!(aot.get_summary(), "pending ");
    assert_eq!(
        interpreter.get("job"),
        Ok(job(0, &[("_0", Value::String(String::new()))]))
    );

    let details = Details {
        title: "card".into(),
        priority: 7,
    };
    aot.set_job(Job::Ready(details, 2));
    let ready = job(
        1,
        &[
            (
                "_0",
                record(
                    "Details",
                    &[
                        ("title", Value::String("card".into())),
                        ("priority", Value::Int(7)),
                    ],
                ),
            ),
            ("_1", Value::Int(2)),
        ],
    );
    interpreter.set("job", ready.clone()).unwrap();
    assert_eq!(
        interpreter.invoke("echo", &[ready]),
        Ok(Some(interpreter.get("job").unwrap()))
    );
    assert_eq!(aot.get_summary(), "card 2 7");
    assert_eq!(
        interpreter.get("summary"),
        Ok(Value::String("card 2 7".into()))
    );
    for ui in [&mut aot as &mut dyn Component, &mut interpreter] {
        let scene = ui.render();
        assert_eq!(scene.find("ready").unwrap().text("text"), Some("card 2"));
        assert!(scene.find("waiting").is_none());
        assert!(ui.dispatch("ready", "clicked"));
    }
    assert_eq!(aot.get_samples(), 2);
    assert_eq!(interpreter.get("samples"), Ok(Value::Int(2)));
    assert_eq!(aot.invoke_once(), 11);
    assert_eq!(interpreter.invoke("once", &[]), Ok(Some(Value::Int(11))));
    assert_eq!(aot.get_samples(), 3);
    assert_eq!(interpreter.get("samples"), Ok(Value::Int(3)));
    assert_eq!(aot.get_failures(), 0);
    assert_eq!(interpreter.get("failures"), Ok(Value::Int(0)));

    let failed = aot.invoke_fail();
    assert_eq!(
        failed,
        Job::Failed {
            message: "disk".into(),
            retriable: true
        }
    );
    let host = interpreter.invoke("fail", &[]).unwrap().unwrap();
    assert_eq!(
        host,
        job(
            2,
            &[
                ("message", Value::String("disk".into())),
                ("retriable", Value::Bool(true))
            ]
        )
    );
    aot.set_job(failed);
    interpreter.set("job", host).unwrap();
    for ui in [&aot as &dyn Component, &interpreter] {
        let scene = ui.render();
        assert_eq!(
            scene.find("error-label").unwrap().text("text"),
            Some("disk true")
        );
    }
    Ok(())
}

#[test]
fn host_rejects_invalid_tag_and_inactive_or_ill_typed_payload() -> TestResult {
    let interpreter = interpreted()?;
    for value in [
        job(-1, &[]),
        job(99, &[]),
        job(1, &[("_1", Value::Int(2))]),
        job(0, &[("_0", Value::Int(2))]),
        job(3, &[("message", Value::String("extra".into()))]),
        job(
            2,
            &[
                ("message", Value::String("disk".into())),
                ("retriable", Value::Int(1)),
            ],
        ),
    ] {
        assert!(interpreter.set("job", value.clone()).is_err());
        assert!(interpreter.invoke("echo", &[value]).is_err());
        assert_eq!(
            interpreter.get("summary"),
            Ok(Value::String("pending ".into()))
        );
    }
    Ok(())
}

#[test]
fn source_rejects_incomplete_patterns_and_payload_type_mismatches() {
    for (original, invalid, diagnostic) in [
        (
            "Ready(details, count) => details.title",
            "Ready(details) => details.title",
            "must destructure 2 payload",
        ),
        (
            "Failed { retriable: _, message: reason }",
            "Failed { retriable: _, unknown: reason }",
            "unknown payload field",
        ),
        (
            "Job::Failed { retriable: true, message: ",
            "Job::Failed { retriable: \"yes\", message: ",
            "type mismatch: expected bool",
        ),
        (
            "Ready(details, count) => details.title",
            "Ready(details, details) => details.title",
            "duplicate match binding",
        ),
        (
            "root.samples += count;",
            r#"details.title = "changed";"#,
            "match bindings cannot be assigned",
        ),
    ] {
        let error = xrack_compiler::compile(&SOURCE.replace(original, invalid), "enum_payload.xr")
            .err()
            .expect("expected invalid enum source to fail");
        assert!(error.to_string().contains(diagnostic), "{error}");
    }
}

#[test]
fn table_history_replays_enum_tag_and_payload_together() {
    use xrack_runtime::{TableEditHistory, TableHistoryPayload, VecModel};

    let rust_variant = Job::Ready(
        Details {
            title: "document".into(),
            priority: 4,
        },
        1,
    );
    assert_eq!(rust_variant.history_payload_bytes(), "document".len());

    let Value::Record(before) = record(
        "Row",
        &[("job", job(0, &[("_0", Value::String("queued".into()))]))],
    ) else {
        panic!("expected a row")
    };
    let Value::Record(after) = record(
        "Row",
        &[(
            "job",
            job(
                2,
                &[
                    ("message", Value::String("missing".into())),
                    ("retriable", Value::Bool(true)),
                ],
            ),
        )],
    ) else {
        panic!("expected a row")
    };
    let model = VecModel::from(vec![before.clone()]);
    let history = TableEditHistory::default();
    let key = model.row_key(0).unwrap();
    assert!(history.commit_cell(
        &model,
        0,
        key,
        |_| Some(after.clone()),
        |before, after, patch| {
            patch.record_changes_with(before, after, |name| name == "Job");
        }
    ));
    assert!(history.replay(&model, false));
    assert_eq!(model.row(0), Some(before));
    assert!(history.replay(&model, true));
    assert_eq!(model.row(0), Some(after));
}

#[test]
fn interpreter_rejects_invalid_variant_from_host_callback_or_raw_mlir() -> TestResult {
    let interpreter = interpreted()?;
    interpreter
        .on("sample", std::rc::Rc::new(|_| Ok(Some(job(4, &[])))))
        .unwrap();
    let error = interpreter.invoke("once", &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("invalid Job variant discriminant")
    );

    let preview =
        xrack_compiler::prepare_preview(SOURCE, "enum_payload.xr", Default::default()).unwrap();
    let mlir = preview.module.to_mlir();
    assert!(mlir.contains("tag = 1 : i64"));
    let invalid = mlir.replacen("tag = 1 : i64", "tag = 400 : i64", 1);
    assert!(xrack_interpreter::load_mlir(&invalid).is_err());
    Ok(())
}
