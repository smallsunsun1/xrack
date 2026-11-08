use stepper_ui::StepperDemo;
use xrack_layout::{ActivationKey, Interaction, Point, Size, TextMeasurer, layout};
use xrack_runtime::Component;

struct Measure;

impl TextMeasurer for Measure {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size {
        Size::new(
            (text.chars().count() as f32 * font_size).min(max_width),
            font_size,
        )
    }
}

#[test]
fn clicking_identically_named_buttons_updates_only_the_target_instance() {
    let mut ui = StepperDemo::new();
    let scene = ui.render();
    let viewport = Size::new(480.0, 230.0);
    let bounds = layout(&scene, viewport, &mut Measure);
    let left_button = bounds.at_path(&[0, 0, 0, 1]).unwrap().bounds;
    let mut input = Interaction::default();
    input.pointer_move(
        &scene,
        &bounds,
        Point {
            x: left_button.x + left_button.width * 0.5,
            y: left_button.y + left_button.height * 0.5,
        },
    );
    input.pointer_press();
    let left = input.pointer_release_target().unwrap();
    assert_eq!(left.id, Some("add"));
    assert!(ui.dispatch_at(&scene, &left.path, "clicked"));
    assert_eq!((ui.get_left_count(), ui.get_right_count()), (1, 0));

    ui.set_step(3);
    let scene = ui.render();
    let bounds = layout(&scene, viewport, &mut Measure);
    input.reconcile(&scene, &bounds);
    input.focus_next(&scene, &bounds, false);
    input.key_press(ActivationKey::Space);
    let right = input.key_release_target(ActivationKey::Space).unwrap();
    assert_eq!(right.id, left.id);
    assert_ne!(right.path, left.path);
    assert!(ui.dispatch_at(&scene, &right.path, "clicked"));
    assert_eq!((ui.get_left_count(), ui.get_right_count()), (1, 2));

    assert!(ui.dispatch("left/add", "clicked"));
    assert_eq!((ui.get_left_count(), ui.get_right_count()), (4, 2));
    let scene = ui.render();
    assert_eq!(
        scene.find("summary").unwrap().text("text"),
        Some("父组件读数：左 4，右 2")
    );
    assert_eq!(
        scene
            .find("left")
            .unwrap()
            .find("add")
            .unwrap()
            .text("text"),
        Some("加 3（点击 2 次）")
    );
    assert_eq!(
        scene
            .find("right")
            .unwrap()
            .find("add")
            .unwrap()
            .text("text"),
        Some("加 2（点击 1 次）")
    );
    assert!(ui.dispatch("reset-left", "clicked"));
    assert_eq!((ui.get_left_count(), ui.get_right_count()), (0, 2));
}
