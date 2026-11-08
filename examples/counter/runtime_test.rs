use counter_ui::Counter;
use std::{cell::Cell, rc::Rc};
use xrack_runtime::{Length, Value};

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
