#![cfg(test)]

#[allow(dead_code)]
use xrack_testing as ui_driver;

use std::{cell::Cell, rc::Rc};
use ui_driver::UiDriver;
use xrack_interpreter::Program;
use xrack_layout::{Size, TextMeasurer};
use xrack_runtime::{Component, Node, Value};

const SOURCE: &str = include_str!("loader.xr");

struct Text;

impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 7.0).min(width), 16.0)
    }
}

fn assert_unloaded(scene: &Node) {
    let loader = scene.find("area").unwrap();
    assert!(loader.find("increment").is_none());
    assert!(loader.find("summary-label").is_none());
}

fn exercise<C: Component>(
    component: C,
    set_open: impl Fn(&mut C, bool),
    set_page: impl Fn(&mut C, &str),
    set_caption: impl Fn(&mut C, &str),
    counters: impl Fn(&C) -> (i64, i64, i64),
    status: impl Fn(&C) -> String,
    source_reads: &Cell<usize>,
) {
    let mut driver = UiDriver::new(component, Text, Size::new(480.0, 270.0));
    assert_unloaded(driver.scene());
    assert_eq!(status(driver.component()), "null");
    assert_eq!(source_reads.get(), 0);
    driver.update(|ui| set_open(ui, true));
    assert_eq!(status(driver.component()), "ready");
    assert_eq!(source_reads.get(), 1);
    assert_eq!(driver.node("caption-label").text("text"), Some("First"));
    assert_eq!(driver.focused(), Some("search"));
    assert_eq!(counters(driver.component()), (0, -1, 1));
    assert_eq!(driver.node("increment").text("text"), Some("Count 0"));
    driver.click("increment");
    assert_eq!(counters(driver.component()), (1, 1, 1));
    let old_scene = driver.scene().clone();
    let old_button = driver.path("increment");
    driver.update(|ui| set_caption(ui, "Updated"));
    assert_eq!(driver.node("caption-label").text("text"), Some("Updated"));
    assert_eq!(driver.node("increment").text("text"), Some("Count 1"));
    assert_eq!(source_reads.get(), 1);

    driver.update(|ui| set_page(ui, "summary"));
    assert_eq!(source_reads.get(), 2);
    assert!(driver.scene().find("increment").is_none());
    assert_eq!(driver.node("summary-label").text("text"), Some("Summary"));
    assert_eq!(driver.focused(), Some("summary-action"));
    assert_eq!(counters(driver.component()), (1, 1, 2));
    driver.update(|ui| set_page(ui, "details"));
    assert!(driver.scene().find("summary-label").is_none());
    assert_eq!(driver.node("increment").text("text"), Some("Count 0"));
    driver.update(|ui| assert!(!ui.dispatch_at(&old_scene, &old_button, "clicked")));
    assert_eq!(counters(driver.component()), (1, 1, 3));
    driver.click("increment");
    assert_eq!(counters(driver.component()), (2, 1, 3));
    driver.update(|ui| set_open(ui, false));
    assert_unloaded(driver.scene());
    assert_eq!(driver.focused(), None);
    driver.update(|ui| set_open(ui, true));
    assert_eq!(driver.node("increment").text("text"), Some("Count 0"));
    assert_eq!(driver.focused(), Some("search"));
    assert_eq!(counters(driver.component()), (2, 1, 4));
    driver.update(|ui| set_page(ui, "unknown"));
    assert_unloaded(driver.scene());
    assert_eq!(driver.focused(), None);
    assert_eq!(counters(driver.component()), (2, 1, 4));
    assert_eq!(status(driver.component()), "error");
    driver.update(|ui| set_open(ui, false));
    assert_eq!(status(driver.component()), "null");
    driver.update(|ui| {
        set_open(ui, true);
        set_page(ui, "");
    });
    assert_unloaded(driver.scene());
    assert_eq!(status(driver.component()), "null");
}

#[test]
fn loader_mounts_and_drops_instances_in_aot() {
    let mut ui = loader_ui::LoaderDemo::new();
    let reads = Rc::new(Cell::new(0));
    ui.on_source_for({
        let reads = reads.clone();
        move |page| {
            reads.set(reads.get() + 1);
            page
        }
    });
    exercise(
        ui,
        |ui, value| ui.set_open(value),
        |ui, value| ui.set_page(value.into()),
        |ui, value| ui.set_caption(value.into()),
        |ui| (ui.get_clicks(), ui.get_count(), ui.get_loads()),
        |ui| ui.get_load_state(),
        &reads,
    );
}

#[test]
fn loader_mounts_and_drops_instances_in_interpreter() {
    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("LoaderDemo")
        .unwrap();
    let reads = Rc::new(Cell::new(0));
    ui.on("source-for", {
        let reads = reads.clone();
        Rc::new(move |args| {
            let [Value::String(page)] = args else {
                panic!("expected a page name")
            };
            reads.set(reads.get() + 1);
            Ok(Some(Value::String(page.clone())))
        })
    })
    .unwrap();
    exercise(
        ui,
        |ui, value| ui.set("open", Value::Bool(value)).unwrap(),
        |ui, value| ui.set("page", Value::String(value.into())).unwrap(),
        |ui, value| ui.set("caption", Value::String(value.into())).unwrap(),
        |ui| {
            let Value::Int(clicks) = ui.get("clicks").unwrap() else {
                panic!("expected clicks")
            };
            let Value::Int(count) = ui.get("count").unwrap() else {
                panic!("expected count")
            };
            let Value::Int(loads) = ui.get("loads").unwrap() else {
                panic!("expected loads")
            };
            (clicks, count, loads)
        },
        |ui| {
            let Value::String(status) = ui.get("load-state").unwrap() else {
                panic!("expected loader status")
            };
            status
        },
        &reads,
    );
}

fn exercise_deferred<C: Component>(
    component: C,
    set_enabled: impl Fn(&mut C, bool),
    set_page: impl Fn(&mut C, &str),
    set_deferred: impl Fn(&mut C, bool),
    status: impl Fn(&C) -> String,
    loads: impl Fn(&C) -> i64,
) {
    let mut driver = UiDriver::new(component, Text, Size::new(480.0, 270.0));
    assert_eq!(status(driver.component()), "null");
    driver.update(|ui| set_enabled(ui, true));
    assert_eq!(status(driver.component()), "loading");
    assert_eq!(driver.node("state-label").text("text"), Some("loading"));
    assert_unloaded(driver.scene());
    assert_eq!(loads(driver.component()), 0);

    let superseded = driver.scene().clone();
    driver.update(|ui| set_page(ui, "summary"));
    assert!(!xrack_runtime::finish_loader_frame(&superseded));
    assert_eq!(status(driver.component()), "loading");
    assert_eq!(loads(driver.component()), 0);
    assert_unloaded(driver.scene());

    for _ in 0..8 {
        if status(driver.component()) == "ready" {
            break;
        }
        assert!(driver.finish_loader_frame());
    }
    assert_eq!(status(driver.component()), "ready");
    assert_eq!(driver.node("state-label").text("text"), Some("ready"));
    assert_eq!(driver.node("summary-label").text("text"), Some("Summary"));
    assert_eq!(driver.focused(), Some("summary-action"));
    assert_eq!(loads(driver.component()), 1);
    assert!(!driver.finish_loader_frame());

    driver.update(|ui| set_page(ui, "details"));
    let canceled = driver.scene().clone();
    assert_eq!(status(driver.component()), "loading");
    driver.update(|ui| set_enabled(ui, false));
    assert!(!xrack_runtime::finish_loader_frame(&canceled));
    assert_eq!(status(driver.component()), "null");
    assert_eq!(loads(driver.component()), 1);

    driver.update(|ui| set_enabled(ui, true));
    assert_eq!(status(driver.component()), "loading");
    let bypassed = driver.scene().clone();
    driver.update(|ui| set_deferred(ui, false));
    assert!(!xrack_runtime::finish_loader_frame(&bypassed));
    assert_eq!(status(driver.component()), "ready");
    assert_eq!(
        driver.node("caption-label").text("text"),
        Some("Deferred details")
    );
    assert_eq!(driver.focused(), Some("search"));
    assert_eq!(loads(driver.component()), 2);

    driver.update(|ui| set_deferred(ui, true));
    assert_eq!(status(driver.component()), "ready");
    assert_eq!(loads(driver.component()), 2);
    assert!(!driver.finish_loader_frame());
    driver.update(|ui| set_page(ui, "missing"));
    assert_eq!(status(driver.component()), "error");
    assert!(!driver.finish_loader_frame());
    assert_unloaded(driver.scene());

    driver.update(|ui| set_page(ui, "heavy"));
    assert!(driver.finish_loader_frame());
    assert_eq!(status(driver.component()), "loading");
    assert_eq!(loads(driver.component()), 2);
    assert!(driver.scene().find("heavy-ready").is_none());
    let partial = driver.scene().clone();
    driver.update(|ui| set_enabled(ui, false));
    assert!(!xrack_runtime::finish_loader_frame(&partial));
    assert_eq!(status(driver.component()), "null");
    assert_eq!(loads(driver.component()), 2);

    driver.update(|ui| set_enabled(ui, true));
    assert!(driver.finish_loader_frame());
    assert_eq!(status(driver.component()), "loading");
    for _ in 0..64 {
        if status(driver.component()) == "ready" {
            break;
        }
        assert!(driver.scene().find("heavy-ready").is_none());
        assert_eq!(loads(driver.component()), 2);
        assert!(driver.finish_loader_frame());
    }
    assert_eq!(status(driver.component()), "ready");
    assert_eq!(driver.node("heavy-ready").text("text"), Some("Heavy page"));
    assert_eq!(loads(driver.component()), 3);
    assert!(!driver.finish_loader_frame());
}

#[test]
fn deferred_loader_cancels_and_mounts_on_a_later_frame_in_aot() {
    exercise_deferred(
        loader_ui::DeferredLoaderDemo::new(),
        |ui, value| ui.set_enabled(value),
        |ui, value| ui.set_page(value.into()),
        |ui, value| ui.set_defer_load(value),
        |ui| ui.get_load_state(),
        |ui| ui.get_loads(),
    );
}

#[test]
fn deferred_loader_cancels_and_mounts_on_a_later_frame_in_interpreter() {
    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("DeferredLoaderDemo")
        .unwrap();
    exercise_deferred(
        ui,
        |ui, value| ui.set("enabled", Value::Bool(value)).unwrap(),
        |ui, value| ui.set("page", Value::String(value.into())).unwrap(),
        |ui, value| ui.set("defer-load", Value::Bool(value)).unwrap(),
        |ui| {
            let Value::String(status) = ui.get("load-state").unwrap() else {
                panic!("expected status")
            };
            status
        },
        |ui| {
            let Value::Int(loads) = ui.get("loads").unwrap() else {
                panic!("expected load count")
            };
            loads
        },
    );
}

#[test]
fn loader_selects_first_candidate_by_default() {
    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    let generated = loader_ui::DefaultLoaderDemo::new();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("DefaultLoaderDemo")
        .unwrap();
    for ui in [&generated as &dyn Component, &interpreted] {
        let scene = ui.render();
        for id in ["single", "multiple"] {
            let loader = scene.find(id).unwrap();
            assert!(loader.find("summary-label").is_some(), "{id}");
            assert!(loader.find("increment").is_none(), "{id}");
        }
    }
}

#[test]
fn loader_adopts_content_size_or_fills_explicit_size_in_both_backends() {
    fn check(component: impl Component) {
        let driver = UiDriver::new(component, Text, Size::new(480.0, 270.0));
        for (id, width, height) in [
            ("natural", 50.0, 40.0),
            ("wide", 160.0, 40.0),
            ("sized", 160.0, 90.0),
        ] {
            let bounds = driver.bounds(id);
            assert_eq!((bounds.width, bounds.height), (width, height), "{id}");
            let loader = driver.node(id);
            assert_eq!(loader.children[0].children[0].kind, "Rectangle");
            let mut path = driver.path(id);
            path.extend([0, 0]);
            assert_eq!(
                driver.layout_at(&path).unwrap().bounds,
                bounds,
                "{id} content"
            );
        }
    }
    check(loader_ui::LoaderSizingDemo::new());
    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    check(
        Program::from_module(&preview.module)
            .unwrap()
            .create("LoaderSizingDemo")
            .unwrap(),
    );
}

#[test]
fn loader_rejects_invalid_candidate_declarations() {
    for (members, expected) in [
        (
            "source: \"one\"; Child {}",
            "Loader candidates must be named",
        ),
        (
            "first := Child {} first := Child {}",
            "duplicate Loader candidate",
        ),
        (
            "Text {}",
            "Loader candidates must be declared component instances",
        ),
        ("if true: Child {}", "without if or for"),
        (
            "active <=> root.open; Child {}",
            "Loader active accepts one one-way binding",
        ),
        ("ChildWindow {}", "Loader delegates cannot contain a window"),
        ("status: \"ready\";", "Loader status is read-only"),
        (
            "loaded => { self.status = \"ready\"; } Child {}",
            "read-only",
        ),
    ] {
        let source = format!(
            "component Child inherits VerticalLayout {{}} component ChildWindow inherits Window {{}} \
             export component App inherits Window {{ in-out property <bool> open: once false; area := Loader {{ {members} }} }}",
        );
        let document = xrack_compiler::parse(&source).unwrap();
        let error = xrack_compiler::lower(&document, "loader-invalid.xr").unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

const EXTERNAL_PANEL: &str = r#"
export component RemotePanel inherits VerticalLayout {
    in-out property <int> count: once 0;
    remote-button := Button {
        text: "Clicks " + root.count;
        clicked => { root.count += 1; }
    }
}
"#;

#[test]
fn typed_loader_handle_expires_even_if_the_previous_item_is_retained() {
    let mut generated = loader_ui::LoaderDemo::new();
    generated.on_source_for(|page| page);
    generated.set_open(true);
    generated.render();
    let handle = generated.loader_item::<loader_ui::Details>("area").unwrap();
    let previous = handle.upgrade().unwrap();
    assert!(generated.loader_item::<String>("area").is_none());
    generated.set_page("summary".into());
    generated.render();
    assert!(handle.upgrade().is_none());
    assert_eq!(previous.get_count(), 0);

    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    let interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("LoaderDemo")
        .unwrap();
    interpreted
        .on("source-for", Rc::new(|values| Ok(values.first().cloned())))
        .unwrap();
    interpreted.set("open", Value::Bool(true)).unwrap();
    interpreted.try_render().unwrap();
    let handle = interpreted.loader_item("area").unwrap();
    let previous = handle.upgrade().unwrap();
    interpreted
        .set("page", Value::String("summary".into()))
        .unwrap();
    interpreted.try_render().unwrap();
    assert!(handle.upgrade().is_none());
    assert_eq!(previous.get("count").unwrap(), Value::Int(0));
}

fn exercise_external<C: Component>(
    ui: C,
    set_source: impl Fn(&mut C, &str),
    status: impl Fn(&C) -> String,
    loads: impl Fn(&C) -> i64,
    item: impl Fn(&C) -> Option<xrack_runtime::LoaderItem<xrack_interpreter::Instance>>,
    source: &str,
) {
    let mut driver = UiDriver::new(ui, Text, Size::new(480.0, 270.0));
    assert_eq!(status(driver.component()), "null");
    let missing =
        std::env::temp_dir().join(format!("xrack-loader-missing-{}.xr", std::process::id()));
    driver.update(|ui| set_source(ui, &missing.to_string_lossy()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while status(driver.component()) == "loading" && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        driver.finish_loader_frame();
    }
    assert_eq!(status(driver.component()), "error");
    assert_eq!(loads(driver.component()), 0);
    assert!(item(driver.component()).is_none());
    driver.update(|ui| set_source(ui, source));
    assert_eq!(status(driver.component()), "loading");
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while status(driver.component()) == "loading" && std::time::Instant::now() < limit {
        std::thread::sleep(std::time::Duration::from_millis(5));
        driver.finish_loader_frame();
    }
    assert_eq!(status(driver.component()), "ready", "{source}");
    assert_eq!(loads(driver.component()), 1);
    assert_eq!(driver.node("remote-button").text("text"), Some("Clicks 0"));
    let handle = item(driver.component()).unwrap();
    let mounted = handle.upgrade().unwrap();
    let previous = driver.scene().clone();
    let path = driver.path("remote-button");
    driver.click("remote-button");
    assert_eq!(driver.node("remote-button").text("text"), Some("Clicks 1"));
    assert_eq!(mounted.get("count").unwrap(), Value::Int(1));
    driver.update(|ui| set_source(ui, ""));
    assert!(handle.upgrade().is_none());
    assert_eq!(status(driver.component()), "null");
    assert!(driver.scene().find("remote-button").is_none());
    driver.update(|ui| assert!(!ui.dispatch_at(&previous, &path, "clicked")));
}

#[test]
fn aot_loader_mounts_a_runtime_file() {
    let location =
        std::env::temp_dir().join(format!("xrack-loader-test-{}.xr", std::process::id()));
    struct RemoveFile(std::path::PathBuf);
    impl Drop for RemoveFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = RemoveFile(location.clone());
    std::fs::write(&location, EXTERNAL_PANEL).unwrap();
    xrack_interpreter::install_external_loader();
    exercise_external(
        loader_ui::ExternalLoaderDemo::new(),
        |ui, source| ui.set_address(source.into()),
        |ui| ui.get_load_state(),
        |ui| ui.get_loads(),
        |ui| ui.loader_item::<xrack_interpreter::Instance>("external"),
        &format!("{}#RemotePanel", location.display()),
    );
}

#[test]
fn interpreter_loader_mounts_an_http_source() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 1024];
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            EXTERNAL_PANEL.len(),
            EXTERNAL_PANEL
        )
        .unwrap();
    });

    let preview = xrack_compiler::prepare_preview(SOURCE, "loader.xr", Default::default()).unwrap();
    let ui = Program::from_module(&preview.module)
        .unwrap()
        .create("ExternalLoaderDemo")
        .unwrap();
    exercise_external(
        ui,
        |ui, source| ui.set("address", Value::String(source.into())).unwrap(),
        |ui| {
            let Value::String(value) = ui.get("load-state").unwrap() else {
                panic!("expected status")
            };
            value
        },
        |ui| {
            let Value::Int(value) = ui.get("loads").unwrap() else {
                panic!("expected load count")
            };
            value
        },
        |ui| ui.loader_item("external"),
        &format!("http://{address}/panel.xr"),
    );
    server.join().unwrap();
}
