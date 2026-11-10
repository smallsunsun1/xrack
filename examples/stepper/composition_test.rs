use stepper_ui::{AliasDefaultsDemo, CallbackProbe, LinkedControlsDemo, LinkedTask, StepperDemo};
use xrack_layout::{
    ActivationKey, Interaction, Point, Size, TextMeasurer, layout, prepare_scene_with_input,
};
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
fn exported_property_alias_uses_and_resets_the_leaf_control_storage() {
    let mut ui = AliasDefaultsDemo::new();
    assert_eq!(ui.get_document(), "编辑器默认值");
    ui.set_document("外部设置".into());
    let scene = ui.render();
    let path = [0, 1];
    assert_eq!(scene.at_path(&path).unwrap().text("text"), Some("外部设置"));
    assert!(ui.edit_at(&scene, &path, "控件写入".into()));
    assert_eq!(ui.get_document(), "控件写入");
    ui.reset_document();
    assert_eq!(ui.get_document(), "编辑器默认值");
    let scene = ui.render();
    assert!(ui.dispatch_at(&scene, &path, "accepted"));
    ui.invoke_confirmed();
    assert_eq!(ui.get_confirmations(), 2);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let host = calls.clone();
    ui.on_confirmed(move || host.set(host.get() + 1));
    assert!(ui.dispatch_at(&scene, &path, "accepted"));
    assert_eq!(calls.get(), 1);
    assert_eq!(ui.get_confirmations(), 2);
}

#[test]
fn component_links_forward_reads_and_writes_through_wrappers_and_siblings() {
    let mut ui = LinkedControlsDemo::new();
    let first = [0, 1, 0, 1];
    let second = [0, 2, 0, 1];
    let scene = ui.render();
    for path in [&first[..], &second[..]] {
        assert_eq!(scene.at_path(path).unwrap().text("text"), Some("共享草稿"));
    }
    ui.set_shared("Rust 写入".into());
    let scene = ui.render();
    assert_eq!(
        scene.at_path(&first).unwrap().text("text"),
        Some("Rust 写入")
    );
    assert_eq!(ui.get_read_back(), "Rust 写入");
    assert!(ui.edit_at(&scene, &first, "编辑嵌套控件".into()));
    assert_eq!(ui.get_shared(), "编辑嵌套控件");
    assert_eq!(ui.get_read_back(), "编辑嵌套控件");
    assert_eq!(
        ui.render().at_path(&second).unwrap().text("text"),
        Some("编辑嵌套控件")
    );
    let scene = ui.render();
    assert!(ui.dispatch_at(&scene, &first, "accepted"));
    assert_eq!(ui.get_last_submitted(), "编辑嵌套控件");
    ui.invoke_reset_through_second();
    assert_eq!(ui.get_shared(), "由兄弟组件写回");
    assert_eq!(
        ui.render().at_path(&first).unwrap().text("text"),
        Some("由兄弟组件写回")
    );
    assert_eq!(LinkedControlsDemo::new().get_shared(), "共享草稿");
}

#[test]
fn delegate_links_edit_stable_rows_and_reject_old_scene_slots() {
    let mut ui = LinkedControlsDemo::new();
    ui.set_tasks(vec![
        LinkedTask {
            name: "B".into(),
            done: true,
        },
        LinkedTask {
            name: "A".into(),
            done: false,
        },
    ]);
    let records = ui.model_tasks();
    let tags = ui.model_tags();
    let moved = records.row_key(0).unwrap();
    let prepare = |ui: &LinkedControlsDemo| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(560.0, 340.0),
            &mut Measure,
            &Interaction::default(),
        )
        .0
    };
    let first_record = [0, 5, 0, 0, 0, 1];
    let second_record = [0, 5, 0, 1, 0, 1];
    let first_tag = [0, 5, 1, 0, 0, 1];
    let scene = prepare(&ui);
    assert_eq!(
        scene.at_path(&first_record).unwrap().text("text"),
        Some("B")
    );
    assert!(ui.edit_at(&scene, &first_tag, "用户标签".into()));
    assert_eq!(tags.row(0).as_deref(), Some("用户标签"));
    assert!(records.sort_by(|left, right| left.name.cmp(&right.name)));
    assert_eq!(records.row_key(1), Some(moved));
    assert!(!ui.edit_at(&scene, &first_record, "过期".into()));

    let scene = prepare(&ui);
    assert!(ui.edit_at(&scene, &second_record, "已移动".into()));
    assert_eq!(records.row(0).unwrap().name, "A");
    assert_eq!(records.row(1).unwrap().name, "已移动");
    assert!(records.row(1).unwrap().done);
    assert_eq!(records.row_key(1), Some(moved));
    assert!(ui.dispatch_at(&scene, &second_record, "accepted"));
    assert_eq!(ui.get_last_submitted(), "已移动");
    assert_eq!(ui.get_submitted_index(), 1);

    let previous = prepare(&ui);
    ui.set_tasks(records.snapshot());
    assert!(!ui.edit_at(&previous, &second_record, "旧模型".into()));
    assert_eq!(ui.model_tasks().row(1).unwrap().name, "已移动");
}

#[test]
fn pure_callback_aliases_track_the_bound_implementation_and_its_reads() {
    let mut ui = CallbackProbe::new();
    assert_eq!(ui.get_answer(), 6);
    ui.set_bias(8);
    assert_eq!(ui.get_answer(), 12);
    assert_eq!(ui.invoke_calculation(10), 18);
    ui.on_calculation(|value| value * 5);
    assert_eq!(ui.get_answer(), 20);
    ui.set_bias(20);
    assert_eq!(ui.get_answer(), 20);
    assert_eq!(ui.invoke_calculation(10), 50);
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
