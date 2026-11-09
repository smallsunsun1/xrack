use counter_ui::{Counter, DesktopCounter, SecondaryWindowDemo};
use std::{cell::Cell, rc::Rc};
use xrack_runtime::{Component, Length, Value, VecModel};

#[test]
fn secondary_window_preserves_its_component_and_writes_back_approved_closure() {
    let mut parent = SecondaryWindowDemo::new();
    assert!(parent.secondary_windows().unwrap().is_empty());
    assert!(parent.render().find("inspector").is_none());
    assert!(parent.dispatch("show-inspector", "clicked"));
    assert!(parent.get_inspecting());
    let windows = parent.secondary_windows().unwrap();
    assert!(Rc::ptr_eq(&windows, &parent.secondary_windows().unwrap()));
    let mut child = windows[0].mount().unwrap();
    let scene = child.render_shared();
    assert!(child.edit_at(&scene, &[0, 1], "kept draft".into()));

    parent.set_count(3);
    assert!(child.scene_dirty());
    assert_eq!(
        child.render().find("shared").unwrap().text("text"),
        Some("共享计数 · 3")
    );
    assert!(parent.dispatch("hide-inspector", "clicked"));
    assert!(!parent.get_inspecting());
    drop(child);
    assert!(parent.secondary_windows().unwrap().is_empty());

    parent.set_inspecting(true);
    let reopened = parent.secondary_windows().unwrap();
    assert_eq!(reopened[0].id(), windows[0].id());
    let mut child = reopened[0].mount().unwrap();
    assert_eq!(
        child.render().find("note").unwrap().text("text"),
        Some("kept draft")
    );
    assert!(child.dispatch("allow", "clicked"));
    assert!(parent.dispatch("close-inspector", "clicked"));
    assert!(child.take_window_close_request());
    assert!(!child.close_requested());
    assert!(parent.get_inspecting());
    assert!(child.dispatch("allow", "clicked"));
    assert!(child.close_requested());
    reopened[0].closed();
    assert!(!parent.get_inspecting());
    assert!(parent.secondary_windows().unwrap().is_empty());
}

#[test]
fn a_shared_model_invalidates_both_desktop_scenes_independently() {
    let model = VecModel::from(vec!["initial".to_owned()]);
    let mut first = DesktopCounter::new();
    let mut second = DesktopCounter::new();
    first.bind_model_activity(model.clone());
    second.bind_model_activity(model.clone());
    let first_frame = first.render_shared();
    let second_frame = second.render_shared();
    assert!(!first.scene_dirty() && !second.scene_dirty());

    assert!(model.set_row(0, "changed".into()));
    assert!(first.scene_dirty() && second.scene_dirty());
    assert!(!Rc::ptr_eq(&first_frame, &first.render_shared()));
    assert!(!first.scene_dirty() && second.scene_dirty());
    assert!(!Rc::ptr_eq(&second_frame, &second.render_shared()));
    assert!(!second.scene_dirty());
}

#[test]
fn generated_component_updates_bindings_and_dispatches_events() {
    let mut ui = Counter::new();
    assert_eq!(ui.get_count(), 0);
    assert_eq!(ui.get_doubled(), 0);
    let scene = ui.render();
    assert_eq!(scene.properties["width"], Value::Length(Length(320.0)));
    assert_eq!(
        scene.find("label").unwrap().properties["text"],
        Value::String("计数：0".into())
    );
    ui.set_count(4);
    assert_eq!(ui.get_doubled(), 8);
    assert!(ui.dispatch("add", "clicked"));
    assert_eq!(ui.get_count(), 5);
    assert_eq!(ui.get_doubled(), 10);
    assert_eq!(
        ui.render().find("label").unwrap().properties["text"],
        Value::String("计数：5".into())
    );
    assert!(!ui.dispatch("missing", "clicked"));
    let observed = Rc::new(Cell::new(0));
    let capture = observed.clone();
    ui.on_increment(move |step| capture.set(step));
    ui.invoke_increment(3);
    assert_eq!(ui.get_count(), 8);
    assert_eq!(observed.get(), 3);
    ui.reset_count();
    assert_eq!(ui.get_count(), 0);
    assert_eq!(ui.get_doubled(), 0);
}
