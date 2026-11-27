#![cfg(test)]

#[allow(dead_code)]
use xrack_testing as ui_driver;

use ui_driver::UiDriver;
use xrack_interpreter::Program;
use xrack_layout::{Size, TextMeasurer};
use xrack_runtime::{Component, Value};

const SOURCE: &str = include_str!("dynamic_children.xr");

struct Text;

impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 7.0).min(width), 16.0)
    }
}

fn exercise<C: Component>(
    ui: C,
    populate: impl Fn(&mut C),
    reorder: impl Fn(&mut C),
    detach: impl Fn(&mut C),
    state: impl Fn(&C) -> (i64, i64, bool, bool),
) {
    let mut driver = UiDriver::new(ui, Text, Size::new(480.0, 270.0));
    assert_eq!(state(driver.component()), (-1, 0, false, false));
    driver.update(populate);
    assert_eq!(state(driver.component()), (3, 3, true, false));
    assert_eq!(driver.node("left").children.len(), 2);
    assert_eq!(driver.node("right").children.len(), 1);
    assert_eq!(
        driver.node("left").children[1]
            .find("card")
            .unwrap()
            .text("text"),
        Some("Second: 4")
    );
    assert_eq!(
        driver.node("right").children[0]
            .find("card")
            .unwrap()
            .text("text"),
        Some("Right: 0")
    );

    let mut first_button = driver.path("left");
    first_button.extend([0, 0]);
    driver.click_at_path(&first_button);
    assert_eq!(state(driver.component()), (4, 3, true, false));
    let old_scene = driver.scene().clone();

    driver.update(reorder);
    let mut moved = driver.path("left");
    moved.extend([1, 0]);
    assert_eq!(
        driver
            .input()
            .focused
            .as_ref()
            .map(|target| target.path.as_slice()),
        Some(moved.as_slice()),
    );

    assert_eq!(
        driver.node("left").children[0]
            .find("card")
            .unwrap()
            .text("text"),
        Some("Second: 4")
    );
    assert_eq!(
        driver.node("left").children[1]
            .find("card")
            .unwrap()
            .text("text"),
        Some("First: 4")
    );
    driver.update(detach);
    assert_eq!(state(driver.component()), (-1, 3, false, false));
    assert_eq!(driver.node("left").children.len(), 1);
    driver.update(|ui| assert!(!ui.dispatch_at(&old_scene, &first_button, "clicked")));
    assert_eq!(
        driver.node("left").children[0]
            .find("card")
            .unwrap()
            .text("text"),
        Some("Second: 4")
    );
}

#[test]
fn aot_mounts_and_manages_typed_component_instances() {
    exercise(
        dynamic_children_ui::DynamicDemo::new(),
        |ui| ui.invoke_populate(),
        |ui| ui.invoke_reorder(),
        |ui| ui.invoke_detach(),
        |ui| {
            (
                ui.get_first_hits(),
                ui.get_returned(),
                ui.get_first_mounted(),
                ui.get_in_wrong_container(),
            )
        },
    );
}

#[test]
fn interpreter_mounts_and_manages_typed_component_instances() {
    let preview =
        xrack_compiler::prepare_preview(SOURCE, "dynamic_children.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("DynamicDemo")
        .unwrap();
    exercise(
        ui,
        |ui| {
            ui.invoke("populate", &[]).unwrap();
        },
        |ui| {
            ui.invoke("reorder", &[]).unwrap();
        },
        |ui| {
            ui.invoke("detach", &[]).unwrap();
        },
        |ui| match (
            ui.get("first-hits"),
            ui.get("returned"),
            ui.get("first-mounted"),
            ui.get("in-wrong-container"),
        ) {
            (
                Ok(Value::Int(hits)),
                Ok(Value::Int(returned)),
                Ok(Value::Bool(mounted)),
                Ok(Value::Bool(wrong)),
            ) => (hits, returned, mounted, wrong),
            result => panic!("unexpected state: {result:?}"),
        },
    );
}

fn exercise_loading<C: Component>(
    ui: C,
    later: impl Fn(&mut C),
    cancel: impl Fn(&mut C),
    load_file: impl Fn(&mut C, String),
    remove_file: impl Fn(&mut C),
    snapshot: impl Fn(&C) -> (String, String, i64),
) {
    let mut driver = UiDriver::new(ui, Text, Size::new(480.0, 270.0));
    driver.update(&later);
    assert_eq!(
        snapshot(driver.component()),
        ("loading".into(), "null".into(), 0)
    );
    let canceled = driver.scene().clone();
    driver.update(cancel);
    assert!(!xrack_runtime::finish_loader_frame(&canceled));
    assert_eq!(
        snapshot(driver.component()),
        ("null".into(), "null".into(), 0)
    );

    driver.update(later);
    for _ in 0..8 {
        if snapshot(driver.component()).0 == "ready" {
            break;
        }
        assert!(driver.finish_loader_frame());
    }
    assert_eq!(
        snapshot(driver.component()),
        ("ready".into(), "null".into(), 1)
    );
    assert_eq!(
        driver.node("left").find("card").unwrap().text("text"),
        Some("Later: 0")
    );

    let path = std::env::temp_dir().join(format!(
        "xrack-dynamic-{}-{:?}.xr",
        std::process::id(),
        std::thread::current().id()
    ));
    struct RemoveFile(std::path::PathBuf);
    impl Drop for RemoveFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _file = RemoveFile(path.clone());
    std::fs::write(&path, r#"export component Panel inherits VerticalLayout { remote := Text { text <- "File component"; } }"#).unwrap();
    driver.update(|ui| load_file(ui, path.to_string_lossy().into_owned()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while snapshot(driver.component()).1 == "loading" && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        driver.finish_loader_frame();
    }
    assert_eq!(
        snapshot(driver.component()),
        ("ready".into(), "ready".into(), 1)
    );
    assert_eq!(
        driver.node("right").find("remote").unwrap().text("text"),
        Some("File component")
    );
    driver.update(remove_file);
    assert!(driver.scene().find("remote").is_none());
    assert_eq!(snapshot(driver.component()).1, "null");
}

#[test]
fn both_backends_cancel_pending_children_and_mount_external_sources() {
    xrack_interpreter::install_external_loader();
    exercise_loading(
        dynamic_children_ui::DynamicDemo::new(),
        |ui| ui.invoke_load_later(),
        |ui| ui.invoke_cancel_later(),
        |ui, path| ui.invoke_load_file(path),
        |ui| ui.invoke_remove_file(),
        |ui| {
            (
                ui.get_pending_state(),
                ui.get_external_state(),
                ui.get_mounted_count(),
            )
        },
    );

    let preview =
        xrack_compiler::prepare_preview(SOURCE, "dynamic_children.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("DynamicDemo")
        .unwrap();
    exercise_loading(
        ui,
        |ui| {
            ui.invoke("load-later", &[]).unwrap();
        },
        |ui| {
            ui.invoke("cancel-later", &[]).unwrap();
        },
        |ui, path| {
            ui.invoke("load-file", &[Value::String(path)]).unwrap();
        },
        |ui| {
            ui.invoke("remove-file", &[]).unwrap();
        },
        |ui| match (
            ui.get("pending-state"),
            ui.get("external-state"),
            ui.get("mounted-count"),
        ) {
            (Ok(Value::String(pending)), Ok(Value::String(external)), Ok(Value::Int(count))) => {
                (pending, external, count)
            }
            result => panic!("unexpected state: {result:?}"),
        },
    );
}

#[test]
fn rejects_invalid_factory_inputs_and_effectful_bindings() {
    for (part, replacement, expected) in [
        ("title: \"First\"", "title: false", "expected string"),
        ("title: \"First\"", "unknown: 1", "unknown component input"),
        (
            "out property <int> returned: 0;",
            "out property <DetailsHandle> invalid: left.mount(Details {}); out property <int> returned: 0;",
            "effectful callback",
        ),
    ] {
        let source = SOURCE.replace(part, replacement);
        let document = xrack_compiler::parse(&source).unwrap();
        let error = xrack_compiler::lower(&document, "dynamic-invalid.xr").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}
