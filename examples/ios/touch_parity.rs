#![cfg(test)]

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};
use touch_ui::TouchDemo;
use xrack_interpreter::{HostValue, load_mlir};
use xrack_layout::{
    ActivationKey, CalendarGeometry, EditAction, Interaction, MenuKey, NavigationKey, Rect,
    ScrollGestureEnd, ScrollGestureKind, layout_with_input, set_window_safe_area,
};
use xrack_runtime::{
    CalendarAction, Component, ControlAction, Date, DialogRole, FileDialogMode, MessageButtons,
    NativeDialogOptions, NativeDialogQueue, NativeDialogResult, SceneTransition, SpinAction,
    StandardButtonKind, SwipeSide, TextCommand, Tumbler, text_input_value,
};
use xrack_testing::{FixedText, Point, Size, UiDriver};

fn exercise<C: Component>(component: C, size: Size, wide: bool) -> UiDriver<C> {
    let mut ui = UiDriver::headless(component, size);
    let summary = ui.bounds("summary");
    let list = ui.bounds("task-list");
    if wide {
        assert!(list.x >= summary.x + summary.width);
    } else {
        assert!(list.y >= summary.y + summary.height);
    }

    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(9)
    );
    let mut row = ui.path("task-list");
    row.extend([0, 0, 0]);
    let first = ui.layout_at(&row).unwrap().bounds;
    let slot = row.len() - 3;
    row[slot] = 1;
    let second = ui.layout_at(&row).unwrap().bounds;
    assert_eq!(first.width, second.width);
    assert!(first.y + first.height <= second.y);
    assert!(ui.scroll("task-list", Point { x: 0.0, y: 200.0 }));
    assert!(
        ui.node("task-list")
            .list
            .is_some_and(|list| list.first_row > 0)
    );
    ui.scroll("task-list", Point { x: 0.0, y: -2000.0 });
    ui.click("entry");
    ui.fill_focused("  Replace sensor  ");
    ui.click("add");
    assert_eq!(ui.node("entry").text("text"), Some(""));
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(10)
    );

    let mut task = ui.path("task-list");
    task.extend([0, 0, 0, 0, 0]);
    let progress = ui.path("round-progress");
    let previous = ui.scene().clone();
    ui.click_at_path(&task);
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(9)
    );
    let next = ui.scene();
    let transition = SceneTransition::between(&previous, next);
    let middle = transition.frame(next, Duration::from_millis(400));
    let value = middle
        .at_path(&progress)
        .unwrap()
        .float("progress")
        .unwrap();
    assert!(value > 3.0 && value < 4.0, "{value}");
    assert_eq!(ui.node("round-progress").float("progress"), Some(4.0));
    ui.click("completed-tab");
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(4)
    );
    ui.click_at_path(&task);
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(3)
    );
    ui.click("open-tab");
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(10)
    );
    ui
}

#[test]
fn task_board_behaves_the_same_in_portrait_and_landscape() -> Result<(), Box<dyn std::error::Error>>
{
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let program = load_mlir(&module.mlir)?;
    for (size, wide) in [
        (Size::new(402.0, 780.0), false),
        (Size::new(402.0, 500.0), false),
        (Size::new(760.0, 380.0), true),
    ] {
        let mut compiled = TouchDemo::new();
        compiled.set_wide(wide);
        let ui = exercise(compiled, size, wide);
        assert_eq!(ui.component().get_remaining(), 10);
        assert_eq!(ui.component().get_resolved(), 3);

        let interpreted = program.create("TouchDemo")?;
        interpreted.set("wide", HostValue::Bool(wide))?;
        let ui = exercise(interpreted, size, wide);
        assert_eq!(ui.component().get("remaining")?, HostValue::Int(10));
        assert_eq!(ui.component().get("resolved")?, HostValue::Int(3));
    }
    Ok(())
}

fn swipe_row<C: Component>(ui: &mut UiDriver<C>, path: &[usize], delta: Point, side: SwipeSide) {
    let scene = ui.scene().clone();
    let bounds = ui.layout_at(path).unwrap().bounds;
    let start = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let end = Point {
        x: start.x + delta.x,
        y: start.y + delta.y,
    };
    let mut input = Interaction::default();
    let now = std::time::Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        ui.layout(),
        start,
        now,
        xrack_layout::ScrollGestureKind::Touch,
    ));
    assert!(input.move_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(40)));
    let result =
        input.end_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(80));
    assert!(!result.tap);
    let action = result
        .control
        .expect("a row swipe should commit to its delegate");
    assert_eq!(action.action, ControlAction::SwipeDelegate(side));
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
}

fn exercise_task_swipe<C: Component>(component: C) -> UiDriver<C> {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 780.0));
    let mut row = ui.path("task-list");
    row.extend([0, 0, 0]);
    let mut action = row.clone();
    action.extend([1, 0]);

    for (tab, label, count) in [("open-tab", "Complete", 8), ("completed-tab", "Reopen", 3)] {
        ui.click(tab);
        swipe_row(&mut ui, &row, Point { x: -110.0, y: 0.0 }, SwipeSide::Right);
        assert!(ui.layout_at(&action).unwrap().enabled);
        assert_eq!(
            ui.scene().at_path(&action).unwrap().text("text"),
            Some(label)
        );
        swipe_row(
            &mut ui,
            &action,
            Point { x: 90.0, y: 0.0 },
            SwipeSide::Closed,
        );
        assert!(!ui.layout_at(&action).unwrap().enabled);
        swipe_row(&mut ui, &row, Point { x: -110.0, y: 0.0 }, SwipeSide::Right);
        ui.click_at_path(&action);
        assert_eq!(
            ui.node("task-list").list.map(|list| list.row_count),
            Some(count),
        );
    }
    ui.click("open-tab");
    swipe_row(&mut ui, &row, Point { x: -110.0, y: 0.0 }, SwipeSide::Right);
    let old = ui.scene().clone();
    let bounds = ui.layout_at(&action).unwrap().bounds;
    let start = Point {
        x: bounds.x + 20.0,
        y: bounds.y + bounds.height * 0.5,
    };
    let end = Point {
        x: start.x + 90.0,
        ..start
    };
    let now = std::time::Instant::now();
    let mut input = Interaction::default();
    assert!(input.begin_scroll_gesture(
        &old,
        ui.layout(),
        start,
        now,
        xrack_layout::ScrollGestureKind::Touch,
    ));
    assert!(input.move_scroll_gesture(&old, ui.layout(), end, now + Duration::from_millis(40)));
    let previous_row = old.row_key_at(&row);
    let mut body = row.clone();
    body.extend([0, 0]);
    ui.click_at_path(&body);
    assert_ne!(ui.scene().row_key_at(&row), previous_row);
    assert!(
        input
            .end_scroll_gesture(
                ui.scene(),
                ui.layout(),
                end,
                now + Duration::from_millis(80),
            )
            .control
            .is_none()
    );
    ui.click("completed-tab");
    ui.click_at_path(&body);
    ui.click("open-tab");
    ui
}

#[test]
fn task_swipes_keep_inner_buttons_and_model_updates_consistent()
-> Result<(), Box<dyn std::error::Error>> {
    let compiled = exercise_task_swipe(TouchDemo::new());
    assert_eq!(compiled.component().get_remaining(), 9);
    assert_eq!(compiled.component().get_resolved(), 3);
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = exercise_task_swipe(load_mlir(&module.mlir)?.create("TouchDemo")?);
    assert_eq!(interpreted.component().get("remaining")?, HostValue::Int(9));
    assert_eq!(interpreted.component().get("resolved")?, HostValue::Int(3));
    Ok(())
}

fn exercise_task_keyboard<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 780.0));
    let mut row = ui.path("task-list");
    row.extend([0, 0, 0]);
    let mut body = row.clone();
    body.extend([0, 0]);
    let mut input = Interaction::default();
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &body)
        .unwrap();
    input.accessibility_focus(target);
    for key in [
        NavigationKey::Left,
        NavigationKey::Right,
        NavigationKey::Left,
    ] {
        let scene = ui.scene().clone();
        let action = input.swipe_delegate_key(&scene, ui.layout(), key).unwrap();
        assert_eq!(action.path, row);
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    }
    let scene = ui.scene().clone();
    let action = input.dismiss_focused_swipe(&scene, ui.layout()).unwrap();
    assert_eq!(action.path, row);
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert!(
        input
            .dismiss_focused_swipe(ui.scene(), ui.layout())
            .is_none()
    );
    ui.click("notes-toggle");
    assert!(
        input
            .swipe_delegate_key(ui.scene(), ui.layout(), NavigationKey::Left)
            .is_none()
    );
}

#[test]
fn task_keyboard_navigation_reaches_the_swipe_row_from_its_button()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_task_keyboard(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_task_keyboard(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

fn exercise_task_menu<C: Component>(component: C) -> UiDriver<C> {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 780.0));
    for (tab, command, expected_rows) in [
        ("open-tab", "Mark complete", 8),
        ("completed-tab", "Reopen task", 3),
    ] {
        ui.click(tab);
        let mut row = ui.path("task-list");
        row.extend([0, 0, 0]);
        let bounds = ui.layout_at(&row).unwrap().bounds;
        let origin = Point {
            x: bounds.x + 24.0,
            y: bounds.y + 20.0,
        };
        let mut input = Interaction::default();
        let target = input
            .touch_context_menu_target(ui.scene(), ui.layout(), origin)
            .unwrap();
        assert!(input.open_touch_context_menu(ui.scene(), ui.layout(), target, origin));
        let panels = input.menu_panels(ui.scene(), ui.layout());
        assert_eq!(panels[0].rows[0].label, command);
        assert!(panels[0].bounds.height >= 44.0);
        input.menu_key(ui.scene(), ui.layout(), MenuKey::Down);
        let action = input
            .menu_key(ui.scene(), ui.layout(), MenuKey::Accept)
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| {
            assert!(app.control_at(&scene, &action.path, action.action));
        });
        assert_eq!(
            ui.node("task-list").list.map(|list| list.row_count),
            Some(expected_rows)
        );
    }
    ui
}

#[test]
fn task_menu_moves_the_same_model_rows_on_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    let compiled = exercise_task_menu(TouchDemo::new());
    assert_eq!(compiled.component().get_remaining(), 9);
    assert_eq!(compiled.component().get_resolved(), 3);

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = exercise_task_menu(load_mlir(&module.mlir)?.create("TouchDemo")?);
    assert_eq!(interpreted.component().get("remaining")?, HostValue::Int(9));
    assert_eq!(interpreted.component().get("resolved")?, HostValue::Int(3));
    Ok(())
}

fn edit_notes<C: Component>(component: C) -> UiDriver<C> {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 520.0));
    ui.click("notes-toggle");
    ui.click("notes-editor");
    ui.fill_focused("巡检通过\nRecheck the seal 🚰");
    ui.click("note-lock");
    assert!(ui.node("notes-editor").boolean("read-only", false));
    ui.click("note-lock");
    assert!(!ui.node("notes-editor").boolean("read-only", false));

    ui.click("note-category");
    ui.fill_focused("封条");
    assert_eq!(ui.node("note-category").text("edit-text"), Some("封条"));
    let category_path = ui.path("note-category");
    ui.update(|component| {
        let scene = component.render_shared();
        assert!(component.control_at(&scene, &category_path, ControlAction::Select(2)));
    });
    assert_eq!(text_input_value(ui.node("note-category")), "Urgent");
    ui.click("notes-toggle");
    ui.click("entry");
    ui.fill_focused("Check the seal");
    ui.click("notes-toggle");
    assert_eq!(
        ui.node("notes-editor").text("text"),
        Some("巡检通过\nRecheck the seal 🚰")
    );
    ui
}

#[test]
fn multiline_notes_and_popup_match_on_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    let compiled = edit_notes(TouchDemo::new());
    assert_eq!(compiled.component().get_note_category(), 2);
    assert_eq!(compiled.component().get_draft(), "Check the seal");

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = edit_notes(load_mlir(&module.mlir)?.create("TouchDemo")?);
    assert_eq!(
        interpreted.component().get("note-category")?,
        HostValue::Int(2)
    );
    assert_eq!(
        interpreted.component().get("draft")?,
        HostValue::String("Check the seal".into())
    );
    Ok(())
}

fn import_notes<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("notes-toggle");
    let mut queue = NativeDialogQueue::default();
    for (result, button, status) in [
        (
            NativeDialogResult::Files(note_paths()),
            Some(StandardButtonKind::Yes),
            "2 notes imported",
        ),
        (
            NativeDialogResult::Files(note_paths()),
            Some(StandardButtonKind::No),
            "Current notes kept",
        ),
        (
            NativeDialogResult::Files(note_paths()),
            Some(StandardButtonKind::Cancel),
            "Replacement canceled",
        ),
        (NativeDialogResult::Canceled, None, "Import canceled"),
        (
            NativeDialogResult::Failed("Unavailable".into()),
            None,
            "Unavailable",
        ),
    ] {
        ui.click("import-note");
        assert!(!ui.node("import-note").boolean("enabled", true));
        let scene = ui.component().render_shared();
        queue.sync(&[&scene.native_dialogs[0]]);
        let request = queue.begin_next().unwrap();
        let NativeDialogOptions::File(options) = request.options.as_ref() else {
            panic!("expected a file dialog");
        };
        assert_eq!(options.mode, FileDialogMode::OpenFiles);
        assert_eq!(options.name_filters, ["Text notes (*.txt *.md)"]);
        assert!(queue.complete(request.id, result));
        ui.update(|_| {});
        if let Some(button) = button {
            assert!(!ui.node("import-note").boolean("enabled", true));
            let scene = ui.component().render_shared();
            queue.sync(&[&scene.native_dialogs[0]]);
            let request = queue.begin_next().unwrap();
            let NativeDialogOptions::Message(options) = request.options.as_ref() else {
                panic!("expected a message dialog");
            };
            assert_eq!(options.buttons, MessageButtons::YesNoCancel);
            assert_eq!(options.text, "2 selected files");
            assert!(queue.complete(request.id, NativeDialogResult::Button(button)));
            ui.update(|_| {});
        }
        assert!(ui.node("import-note").boolean("enabled", true));
        assert_eq!(ui.node("import-status").text("text"), Some(status));
    }
}

fn note_paths() -> Vec<String> {
    vec!["/incoming/round.txt".into(), "/incoming/pressure.md".into()]
}

#[test]
fn native_note_import_callbacks_match_on_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    let reads = Rc::new(Cell::new(0));
    let mut compiled = TouchDemo::new();
    compiled.on_read_notes({
        let reads = reads.clone();
        move |paths| {
            reads.set(reads.get() + 1);
            paths == note_paths()
        }
    });
    import_notes(compiled);
    assert_eq!(reads.replace(0), 1);

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("TouchDemo")?;
    interpreted.on("read-notes", {
        let reads = reads.clone();
        Rc::new(move |args| {
            reads.set(reads.get() + 1);
            Ok(Some((args == [HostValue::StringList(note_paths())]).into()))
        })
    })?;
    import_notes(interpreted);
    assert_eq!(reads.get(), 1);
    Ok(())
}

fn transfer_notes<C: Component>(
    component: C,
    set_status: impl FnOnce(&mut C, String),
    set_pending: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("notes-toggle");
    assert!(!ui.node("share-note").boolean("enabled", true));
    assert!(!ui.node("export-note").boolean("enabled", true));
    ui.click("notes-editor");
    ui.fill_focused("巡检通过\nValve 7");
    ui.click("share-note");
    ui.click("export-note");
    let status = "The selected document exceeds the requested byte limit. Choose a folder with fewer inspection notes.";
    ui.update(|app| set_status(app, status.into()));
    assert_eq!(ui.node("transfer-status").text("text"), Some(status));
    for id in ["import-note", "share-note", "export-note"] {
        let bounds = ui.bounds(id);
        assert!(
            bounds.x + bounds.width <= 402.0,
            "{id} is clipped by the window"
        );
        assert!(ui.can_hit(id));
    }
    ui.update(set_pending);
    for button in ["import-note", "share-note", "export-note"] {
        assert!(!ui.node(button).boolean("enabled", true));
    }
}

#[test]
fn note_transfer_passes_the_same_text_and_destination_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let expected = [
        ("巡检通过\nValve 7".to_owned(), false),
        ("巡检通过\nValve 7".to_owned(), true),
    ];
    let calls = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut compiled = TouchDemo::new();
    compiled.on_transfer_note({
        let calls = calls.clone();
        move |text, export| calls.borrow_mut().push((text, export))
    });
    transfer_notes(compiled, TouchDemo::set_transfer_status, |app| {
        app.set_transfer_pending(true)
    });
    assert_eq!(calls.take(), expected);

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("TouchDemo")?;
    interpreted.on("transfer-note", {
        let calls = calls.clone();
        Rc::new(move |args| match args {
            [HostValue::String(text), HostValue::Bool(export)] => {
                calls.borrow_mut().push((text.clone(), *export));
                Ok(None)
            }
            _ => Err("expected the note text and export flag".into()),
        })
    })?;
    transfer_notes(
        interpreted,
        |app, status| app.set("transfer-status", status.into()).unwrap(),
        |app| app.set("transfer-pending", HostValue::Bool(true)).unwrap(),
    );
    assert_eq!(calls.take(), expected);
    Ok(())
}

fn choose_note_folder<C: Component>(
    component: C,
    show_folder: impl FnOnce(&mut C),
    set_pending: impl FnOnce(&mut C),
    calls: &Cell<u32>,
) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("notes-toggle");
    assert!(!ui.node("import-folder").boolean("visible", true));
    ui.update(show_folder);
    ui.click("import-folder");
    assert_eq!(calls.get(), 1);

    ui.click("note-lock");
    assert!(!ui.node("import-folder").boolean("enabled", true));
    assert_eq!(calls.get(), 1);

    ui.click("note-lock");
    ui.click("import-note");
    assert!(!ui.node("import-folder").boolean("enabled", true));
    let mut queue = NativeDialogQueue::default();
    let scene = ui.component().render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let request = queue.begin_next().unwrap();
    assert!(queue.complete(request.id, NativeDialogResult::Canceled));
    ui.update(|_| {});
    assert!(ui.node("import-folder").boolean("enabled", false));

    ui.update(set_pending);
    assert!(!ui.node("import-folder").boolean("enabled", true));
    assert!(!ui.node("import-note").boolean("enabled", true));
    assert_eq!(calls.get(), 1);
}

#[test]
fn folder_button_honors_host_availability_and_note_locks() -> Result<(), Box<dyn std::error::Error>>
{
    let calls = Rc::new(Cell::new(0));
    let mut compiled = TouchDemo::new();
    compiled.on_import_folder({
        let calls = calls.clone();
        move || calls.set(calls.get() + 1)
    });
    choose_note_folder(
        compiled,
        |app| app.set_folder_import_available(true),
        |app| app.set_transfer_pending(true),
        &calls,
    );
    calls.set(0);

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("TouchDemo")?;
    interpreted.on("import-folder", {
        let calls = calls.clone();
        Rc::new(move |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        })
    })?;
    choose_note_folder(
        interpreted,
        |app| {
            app.set("folder-import-available", HostValue::Bool(true))
                .unwrap()
        },
        |app| app.set("transfer-pending", HostValue::Bool(true)).unwrap(),
        &calls,
    );
    Ok(())
}

fn picker_paths<C: Component>(
    ui: &UiDriver<C>,
    id: &str,
    accept: &str,
) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let popup = ui.path(id);
    let mut body = popup.clone();
    body.push(0);
    let action = |label| {
        let mut path = popup.clone();
        path.push(1);
        let slot = ui
            .scene()
            .at_path(&path)
            .unwrap()
            .children
            .iter()
            .position(|button| button.text("text") == Some(label))
            .unwrap();
        path.push(slot);
        path
    };
    (body, action("Cancel"), action(accept))
}

fn contains(outer: Rect, inner: Rect) -> bool {
    let epsilon = 0.01;
    inner.x + epsilon >= outer.x
        && inner.y + epsilon >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + epsilon
        && inner.y + inner.height <= outer.y + outer.height + epsilon
}

fn select_inspection_date<C: Component>(ui: &mut UiDriver<C>, path: &[usize], date: Date) {
    let scene = ui.scene().clone();
    let node = scene.at_path(path).unwrap();
    let layout = ui.layout_at(path).unwrap();
    let calendar = CalendarGeometry::for_layout(node, layout);
    let action = CalendarAction::Select(date);
    let cell = calendar.action_bounds(action).unwrap();
    let mut input = Interaction::default();
    let point = Point {
        x: cell.x + cell.width * 0.5,
        y: cell.y + cell.height * 0.5,
    };
    input.pointer_move(&scene, ui.layout(), point);
    input.pointer_press_in_scene(&scene, ui.layout());
    let click = input.control_part_release(&scene, ui.layout()).unwrap();
    assert_eq!(click.action, ControlAction::Calendar(action));
    ui.update(|app| assert!(app.control_at(&scene, &click.path, click.action)));
}

fn select_inspection_time<C: Component>(
    ui: &mut UiDriver<C>,
    path: &[usize],
    wheel: usize,
    value: usize,
) {
    let mut path = path.to_vec();
    path.push(wheel);
    let scene = ui.scene().clone();
    let action = Tumbler::from_node(scene.at_path(&path).unwrap())
        .select(value)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &path, action)));
}

fn exercise_schedule<C: Component>(component: C, size: Size, wide: bool) {
    let leap = Date::new(2024, 2, 29).unwrap();
    let saturday = Date::new(2024, 3, 2).unwrap();
    let mut ui = UiDriver::headless(component, size);
    ui.click("readings-toggle");

    for confirm in [false, true] {
        ui.click("choose-date");
        let (calendar, cancel, save) = picker_paths(&ui, "inspection-date-picker", "Save date");
        let dialog = ui.bounds("inspection-date-picker");
        let body = ui.layout_at(&calendar).unwrap().bounds;
        let cancel_bounds = ui.layout_at(&cancel).unwrap().bounds;
        let save_bounds = ui.layout_at(&save).unwrap().bounds;
        assert!(contains(dialog, body));
        for button in [cancel_bounds, save_bounds] {
            assert!(button.width >= 44.0 && button.height >= 44.0);
            assert!(contains(dialog, button), "{button:?} outside {dialog:?}");
            if wide {
                assert!(body.x + body.width <= button.x);
            } else {
                assert!(body.y + body.height <= button.y);
            }
        }
        let calendar_node = ui.scene().at_path(&calendar).unwrap();
        assert!(
            CalendarGeometry::new(calendar_node, body)
                .action_bounds(CalendarAction::Select(saturday))
                .is_none()
        );
        select_inspection_date(&mut ui, &calendar, leap);
        assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-28"));
        ui.click_at_path(if confirm { &save } else { &cancel });
        assert_eq!(
            ui.node("schedule-result").text("text"),
            Some(if confirm {
                "Date saved"
            } else {
                "Date unchanged"
            })
        );
    }
    assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-29"));

    ui.click("choose-date");
    let (calendar, _, _) = picker_paths(&ui, "inspection-date-picker", "Save date");
    select_inspection_date(&mut ui, &calendar, Date::new(2024, 2, 28).unwrap());
    ui.escape();
    assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-29"));
    assert_eq!(
        ui.node("schedule-result").text("text"),
        Some("Date unchanged")
    );

    ui.click("choose-date");
    let (calendar, cancel, _) = picker_paths(&ui, "inspection-date-picker", "Save date");
    let mut input = Interaction::default();
    input.reconcile(ui.scene(), ui.layout());
    for month in [3, 4] {
        let scene = ui.scene().clone();
        let browse = input
            .calendar_key(&scene, ui.layout(), NavigationKey::PageDown)
            .unwrap();
        ui.update(|app| assert!(app.control_at(&scene, &browse.path, browse.action)));
        input.reconcile(ui.scene(), ui.layout());
        let target = input.focused.as_ref().unwrap();
        assert_eq!(target.path, calendar);
        let cursor = input
            .calendar_activate(ui.scene(), ui.layout(), target)
            .unwrap();
        assert_eq!(
            cursor.action,
            ControlAction::Calendar(CalendarAction::Select(Date::new(2024, month, 1).unwrap()))
        );
        assert_eq!(
            ui.scene().at_path(&calendar).unwrap().date("date"),
            Some(leap)
        );
    }
    ui.click_at_path(&cancel);
    assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-29"));

    ui.click("choose-time");
    let (wheels, cancel, save) = picker_paths(&ui, "inspection-time-picker", "Save time");
    let dialog = ui.bounds("inspection-time-picker");
    for slot in 0..4 {
        let mut path = wheels.clone();
        path.push(slot);
        let rect = ui.layout_at(&path).unwrap().bounds;
        assert!(rect.width >= 44.0 && rect.height > 0.0);
        assert!(contains(dialog, rect));
    }
    select_inspection_time(&mut ui, &wheels, 1, 31);
    select_inspection_time(&mut ui, &wheels, 3, 0);
    assert!(!ui.scene().at_path(&save).unwrap().boolean("enabled", true));
    ui.click_at_path(&cancel);
    assert_eq!(ui.node("choose-time").text("text"), Some("12:30:41"));
    assert_eq!(
        ui.node("schedule-result").text("text"),
        Some("Time unchanged")
    );

    ui.click("choose-time");
    let (wheels, _, _) = picker_paths(&ui, "inspection-time-picker", "Save time");
    select_inspection_time(&mut ui, &wheels, 1, 31);
    ui.escape();
    assert_eq!(ui.node("choose-time").text("text"), Some("12:30:41"));
    assert_eq!(
        ui.node("schedule-result").text("text"),
        Some("Time unchanged")
    );

    ui.click("choose-time");
    let (wheels, _, save) = picker_paths(&ui, "inspection-time-picker", "Save time");
    select_inspection_time(&mut ui, &wheels, 1, 31);
    assert_eq!(ui.node("choose-time").text("text"), Some("12:30:41"));
    ui.click_at_path(&save);
    assert_eq!(ui.node("choose-time").text("text"), Some("12:31:41"));
    assert_eq!(ui.node("schedule-result").text("text"), Some("Time saved"));
}

fn flick_time_wheel<C: Component>(
    ui: &UiDriver<C>,
    input: &mut Interaction,
    path: &[usize],
    direction: f32,
) -> (Instant, ScrollGestureEnd) {
    let scene = ui.scene();
    let layout = ui.layout();
    let geometry = layout.at_path(path).unwrap().tumbler.as_ref().unwrap();
    let center = geometry.center;
    let start = Point {
        x: center.x + center.width * 0.5,
        y: center.y + center.height * 0.5,
    };
    let moved = |rows| Point {
        y: start.y - direction * rows * center.height,
        ..start
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(scene, layout, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(scene, layout, moved(0.6), now + Duration::from_millis(16)));
    assert!(input.move_scroll_gesture(scene, layout, moved(1.6), now + Duration::from_millis(32)));
    assert!(input.tumbler_preview().is_some());
    let release = now + Duration::from_millis(36);
    (
        release,
        input.end_scroll_gesture(scene, layout, moved(1.6), release),
    )
}

fn exercise_reduced_time_wheel<C: Component>(component: C, reduce_before_drag: bool) {
    let mut ui = UiDriver::headless(component, Size::new(760.0, 358.0));
    ui.click("readings-toggle");
    ui.click("choose-time");
    let (mut minute, _, save) = picker_paths(&ui, "inspection-time-picker", "Save time");
    minute.push(1);
    let scene = ui.scene().clone();
    let layout = ui.layout();
    let mut input = Interaction::default();
    input.set_reduce_motion(reduce_before_drag);
    let (release, end) = flick_time_wheel(&ui, &mut input, &minute, 1.0);
    let choice = if reduce_before_drag {
        end.control.expect("reduced motion commits on release")
    } else {
        assert!(end.control.is_none() && input.scroll_motion_active());
        input.set_reduce_motion(true);
        input
            .advance_tumbler_motion(&scene, layout, release)
            .control
            .expect("enabling reduced motion commits the visible row")
    };
    assert_eq!(choice.path, minute);
    assert!(input.tumbler_preview().is_none() && !input.scroll_motion_active());
    assert!(
        input
            .advance_tumbler_motion(&scene, layout, release + Duration::from_secs(1))
            .control
            .is_none()
    );
    ui.update(|app| assert!(app.control_at(&scene, &choice.path, choice.action)));
    assert_eq!(ui.node("choose-time").text("text"), Some("12:30:41"));
    ui.click_at_path(&save);
    assert_eq!(ui.node("choose-time").text("text"), Some("12:32:41"));
}

#[test]
fn reduced_motion_snaps_the_time_wheel_to_the_visible_row() -> Result<(), Box<dyn std::error::Error>>
{
    let compiled = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let program = load_mlir(&compiled.mlir)?;
    for reduce_before_drag in [true, false] {
        exercise_reduced_time_wheel(TouchDemo::new(), reduce_before_drag);
        exercise_reduced_time_wheel(program.create("TouchDemo")?, reduce_before_drag);
    }
    Ok(())
}

fn exercise_time_picker_action_during_flick<C: Component>(
    component: C,
    confirm: bool,
    valid_time: bool,
    key: Option<ActivationKey>,
) {
    let mut ui = UiDriver::headless(component, Size::new(760.0, 358.0));
    ui.click("readings-toggle");
    ui.click("choose-time");
    let (mut wheel, cancel, save) = picker_paths(&ui, "inspection-time-picker", "Save time");
    wheel.push(usize::from(valid_time));
    let button = if confirm {
        save.clone()
    } else {
        cancel.clone()
    };
    let mut input = Interaction::default();
    // Swipe the hour wheel back from noon to 10 PM for the invalid case.
    let direction = if valid_time { 1.0 } else { -1.0 };
    let (released, end) = flick_time_wheel(&ui, &mut input, &wheel, direction);
    assert!(end.control.is_none() && input.scroll_motion_active());
    let pressed = released + Duration::from_millis(8);
    let scene = ui.scene().clone();
    let layout = ui.layout();
    let target = if let Some(key) = key {
        // The minute and hour wheels start earlier in the dialog's Tab order.
        for _ in 0..6 {
            input.cancel_scroll_gesture_for_picker_key(&scene, layout, false);
            input.focus_next(&scene, layout, false);
            assert!(input.scroll_motion_active());
            if input
                .focused
                .as_ref()
                .is_some_and(|target| target.path == button)
            {
                break;
            }
        }
        input.cancel_scroll_gesture_for_picker_key(&scene, layout, true);
        assert_eq!(input.scroll_motion_active(), confirm);
        input.key_press_with_scene(&scene, key);
        input.key_release_with_scene(&scene, key).unwrap()
    } else {
        let rect = ui.layout_at(&button).unwrap().bounds;
        let point = Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        };
        assert!(input.begin_scroll_gesture(
            &scene,
            layout,
            point,
            pressed,
            ScrollGestureKind::Touch
        ));
        assert!(
            input.scroll_motion_active(),
            "a picker action must preserve the wheel motion"
        );
        let tap =
            input.end_scroll_gesture(&scene, layout, point, pressed + Duration::from_millis(10));
        assert!(
            tap.tap,
            "tap on picker button {button:?} at {point:?} must survive"
        );
        input.accessibility_target(&scene, layout, &button).unwrap()
    };
    assert_eq!(target.path, button);
    let action = input.dialog_button(&scene, layout, &target).unwrap();

    if confirm {
        let choice = input
            .settle_time_picker_wheel(&scene, layout, &action.path)
            .unwrap();
        assert_eq!(choice.path, wheel);
        assert!(!input.scroll_motion_active());
        ui.update(|app| assert!(app.control_at(&scene, &choice.path, choice.action)));
    }
    let latest = ui.scene().clone();
    let accepted = confirm && valid_time;
    ui.update(|app| {
        assert_eq!(
            app.control_at(&latest, &action.path, action.action),
            accepted || !confirm
        );
    });
    assert_eq!(
        ui.node("choose-time").text("text"),
        Some(if accepted { "12:32:41" } else { "12:30:41" })
    );
    if !confirm {
        ui.click("choose-time");
        // Even the same wheel and index in a reopened dialog cannot receive
        // an old flick that survived until the next animation tick.
        assert!(
            input
                .advance_tumbler_motion(ui.scene(), ui.layout(), pressed + Duration::from_secs(1))
                .control
                .is_none()
        );
        assert!(!input.scroll_motion_active());
    } else if !valid_time {
        assert!(!ui.scene().at_path(&save).unwrap().boolean("enabled", true));
        assert_eq!(action.action, ControlAction::Dialog(DialogRole::Accept));
        ui.click_at_path(&cancel);
        assert_eq!(
            ui.node("schedule-result").text("text"),
            Some("Time unchanged")
        );
    }
}

#[test]
fn time_picker_buttons_respect_the_visible_wheel_and_discard_old_flicks()
-> Result<(), Box<dyn std::error::Error>> {
    let compiled = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let program = load_mlir(&compiled.mlir)?;
    for (confirm, valid_time, key) in [
        (true, true, None),
        (true, false, None),
        (false, true, None),
        (true, true, Some(ActivationKey::Enter)),
        (true, false, Some(ActivationKey::Enter)),
        (true, true, Some(ActivationKey::Space)),
        (false, true, Some(ActivationKey::Space)),
    ] {
        exercise_time_picker_action_during_flick(TouchDemo::new(), confirm, valid_time, key);
        exercise_time_picker_action_during_flick(
            program.create("TouchDemo")?,
            confirm,
            valid_time,
            key,
        );
    }
    Ok(())
}

#[test]
fn mobile_pickers_fit_and_preserve_the_same_draft_rules() -> Result<(), Box<dyn std::error::Error>>
{
    let compiled = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let program = load_mlir(&compiled.mlir)?;
    for (size, wide) in [
        (Size::new(320.0, 510.0), false),
        (Size::new(760.0, 358.0), true),
    ] {
        let mut aot = TouchDemo::new();
        aot.set_wide(wide);
        exercise_schedule(aot, size, wide);
        let interpreted = program.create("TouchDemo")?;
        interpreted.set("wide", HostValue::Bool(wide))?;
        exercise_schedule(interpreted, size, wide);
    }
    Ok(())
}

fn enter_readings<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 460.0));
    ui.click("readings-toggle");
    for (id, draft) in [
        ("samples", "999"),
        ("pressure-reading", "3,75"),
        ("temperature-reading", "-2.4"),
    ] {
        let mut input = Interaction::default();
        let path = ui.path(id);
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &path)
            .unwrap();
        input.focus_target(ui.scene(), ui.layout(), target);
        input.sync_editor(ui.scene());
        input.edit_spin(ui.node(id), EditAction::ReplaceAll(draft));
        input.edit_spin(ui.node(id), EditAction::Insert("x"));
        assert_eq!(input.editor_value(ui.node(id)), draft);
        let update = input.commit_spin_draft().unwrap();
        ui.update(|component| {
            let scene = component.render_shared();
            assert!(component.control_at(&scene, &update.path, update.action));
        });
    }
    assert_eq!(ui.node("samples").int("value"), Some(12));
    assert_eq!(ui.node("pressure-reading").number("value"), Some(3.75));
    assert_eq!(ui.node("temperature-reading").number("value"), Some(-2.4));
}

fn step_readings<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 460.0));
    ui.click("readings-toggle");
    for (id, draft, step, expected) in [
        ("samples", "7", SpinAction::Increase, 8.0),
        ("pressure-reading", "4,25", SpinAction::Decrease, 3.25),
        ("temperature-reading", "-", SpinAction::Decrease, -1.0),
    ] {
        let mut input = Interaction::default();
        let path = ui.path(id);
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &path)
            .unwrap();
        input.focus_target(ui.scene(), ui.layout(), target);
        input.sync_editor(ui.scene());
        input.edit_spin(ui.node(id), EditAction::ReplaceAll(draft));
        assert_eq!(input.editor_value(ui.node(id)), draft);
        let scene = ui.scene().clone();
        let action = input.resolve_spin_action(&scene, &path, step).unwrap();
        input.discard_spin_draft();
        ui.update(|app| assert!(app.control_at(&scene, &path, ControlAction::Spin(action))));
        let node = ui.node(id);
        assert_eq!(
            node.number("value")
                .or_else(|| node.int("value").map(|value| value as f64)),
            Some(expected)
        );
    }
}

#[test]
fn numeric_readings_commit_the_same_values_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    enter_readings(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    enter_readings(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

#[test]
fn numeric_stepping_uses_the_same_visible_draft_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    step_readings(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    step_readings(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

fn note_selection_menu<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("notes-toggle");
    ui.click("notes-editor");
    ui.fill_focused("Valve pressure");
    for read_only in [false, true] {
        if read_only {
            ui.click("note-lock");
        }
        let bounds = ui.bounds("notes-editor");
        let point = Point {
            x: bounds.x + 10.0,
            y: bounds.y + 10.0,
        };
        let mut input = Interaction::default();
        let target = input
            .touch_text_target(ui.scene(), ui.layout(), point)
            .unwrap();
        assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target.clone()));
        input.sync_editor(ui.scene());
        input.apply_text_selection(ui.scene(), TextCommand::SelectWord);
        let value = text_input_value(ui.node("notes-editor"));
        let selection = input.editor.as_ref().unwrap().selection(value);
        assert_eq!(&value[selection], "pressure");
        assert!(input.open_touch_text_menu(ui.scene(), ui.layout(), target, point));
        let panels = input.menu_panels(ui.scene(), ui.layout());
        let rows = &panels[0].rows;
        assert!(rows.iter().all(|row| row.shortcut.is_none()));
        let enabled = |label| rows.iter().any(|row| row.label == label && row.enabled);
        assert!(enabled("Copy"));
        assert_eq!(enabled("Cut"), !read_only);
        assert_eq!(enabled("Paste"), !read_only);
    }
}

#[test]
fn touch_selection_and_read_only_commands_match_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    note_selection_menu(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    note_selection_menu(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

fn submit_reading_fields<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("readings-toggle");
    for (id, draft, accepted, status) in [
        ("form-code", "7", false, ""),
        ("form-code", "73", true, "Code 73"),
        ("form-reading", "0,5", false, "Code 73"),
        ("form-reading", "3,75", true, "Reading 3,75"),
    ] {
        ui.click(id);
        ui.fill_focused(draft);
        let path = ui.path(id);
        ui.update(|component| {
            let scene = component.render_shared();
            assert_eq!(
                component.control_at(&scene, &path, ControlAction::ValidateTextSubmission),
                accepted
            );
            assert_eq!(component.dispatch_at(&scene, &path, "accepted"), accepted);
        });
        assert_eq!(ui.node("submitted-form").text("text"), Some(status));
    }
    ui.click("form-search");
    ui.fill_focused("valve");
    let path = ui.path("form-search");
    ui.update(|component| {
        let scene = component.render_shared();
        assert!(component.control_at(&scene, &path, ControlAction::SubmitSearch));
    });
    assert_eq!(
        ui.node("submitted-form").text("text"),
        Some("Search: valve")
    );

    ui.click("contact-details");
    for (id, mode, value, accepted, status) in [
        (
            "contact-email",
            xrack_runtime::InputType::Email,
            "Field.Team+ops@example.org",
            true,
            "Email: Field.Team+ops@example.org",
        ),
        (
            "contact-url",
            xrack_runtime::InputType::Url,
            "https://example.org/a?q=XRack#top",
            true,
            "Website: https://example.org/a?q=XRack#top",
        ),
        (
            "contact-tel",
            xrack_runtime::InputType::Tel,
            "1",
            false,
            "Website: https://example.org/a?q=XRack#top",
        ),
        (
            "contact-tel",
            xrack_runtime::InputType::Tel,
            "+86 (10) 5550-1200",
            true,
            "Telephone: +86 (10) 5550-1200",
        ),
    ] {
        assert_eq!(ui.node(id).input_type(), mode);
        ui.click(id);
        let next_id = match mode {
            xrack_runtime::InputType::Email => Some("contact-tel"),
            xrack_runtime::InputType::Tel => Some("contact-url"),
            _ => None,
        };
        let return_key = if next_id.is_some() {
            xrack_runtime::ReturnKey::Next
        } else {
            xrack_runtime::ReturnKey::Done
        };
        assert_eq!(ui.node(id).return_key(), return_key);
        assert_eq!(
            ui.input()
                .next_text_field(ui.scene(), ui.layout())
                .map(|target| target.path),
            next_id.map(|id| ui.path(id)),
        );
        ui.fill_focused(value);
        assert_eq!(ui.node(id).text("text"), Some(value));
        let path = ui.path(id);
        ui.update(|component| {
            let scene = component.render_shared();
            assert_eq!(
                component.control_at(&scene, &path, ControlAction::ValidateTextSubmission),
                accepted,
            );
            assert_eq!(component.dispatch_at(&scene, &path, "accepted"), accepted);
        });
        assert_eq!(ui.node("contact-result").text("text"), Some(status));
    }
}

#[test]
fn ios_form_validation_and_submissions_match_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    submit_reading_fields(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    submit_reading_fields(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

fn keep_reading_visible_across_keyboard_resize<C: Component>(
    component: C,
    compact: impl FnOnce(&mut C),
) {
    let size = Size::new(667.0, 375.0);
    let mut ui = UiDriver::headless(component, size);
    ui.click("readings-toggle");
    ui.click("form-reading");
    ui.fill_focused("4,75");
    let path = ui.path("form-reading");
    let viewport = ui.path("readings-viewport");
    let mut input = Interaction::default();
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &path)
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), target.clone());
    ui.update(compact);
    let mut scene = ui.scene().clone();
    set_window_safe_area(&mut scene, [0.0, 0.0, 0.0, 244.0]);
    let clipped = layout_with_input(&scene, size, &mut FixedText, &input);
    let before = clipped.at_path(&path).unwrap();
    assert!(before.visible_window_bounds(before.bounds).height < before.bounds.height);

    assert!(input.reveal_focused_editor(&scene, &clipped));
    let visible = layout_with_input(&scene, size, &mut FixedText, &input);
    input.reconcile(&scene, &visible);
    let field = visible.at_path(&path).unwrap();
    assert_eq!(field.visible_window_bounds(field.bounds), field.bounds);
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert!(
        input
            .scroll_offsets()
            .get(&viewport)
            .is_some_and(|offset| *offset > 0.0)
    );
    assert_eq!(scene.at_path(&path).unwrap().text("text"), Some("4,75"));
}

#[test]
fn ios_form_keeps_the_focused_editor_visible_after_keyboard_resize()
-> Result<(), Box<dyn std::error::Error>> {
    keep_reading_visible_across_keyboard_resize(TouchDemo::new(), |app| app.set_compact(true));
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let component = load_mlir(&module.mlir)?.create("TouchDemo")?;
    keep_reading_visible_across_keyboard_resize(component, |app| {
        app.set("compact", HostValue::Bool(true)).unwrap();
    });
    Ok(())
}

fn keep_contact_visible_during_landscape_next<C: Component>(
    component: C,
    compact: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
    ui.click("readings-toggle");
    ui.click("contact-details");
    ui.click("contact-email");
    ui.fill_focused("inspector@example.org");
    let email = ui.path("contact-email");
    let telephone = ui.path("contact-tel");
    let website = ui.path("contact-url");
    let mut input = ui.input().clone();
    ui.update(compact);
    let mut scene = ui.scene().clone();
    let landscape = Size::new(667.0, 375.0);
    set_window_safe_area(&mut scene, [0.0, 0.0, 0.0, 244.0]);
    let frame = layout_with_input(&scene, landscape, &mut FixedText, &input);
    input.reveal_focused_editor(&scene, &frame);
    let frame = layout_with_input(&scene, landscape, &mut FixedText, &input);
    input.reconcile(&scene, &frame);
    assert!(input.is_focused(&email));

    let assert_visible = |frame: &xrack_layout::LayoutNode, path: &[usize]| {
        let field = frame.at_path(path).unwrap();
        let visible = field.visible_window_bounds(field.bounds);
        assert!(
            (visible.width - field.bounds.width).abs() < 0.05
                && (visible.height - field.bounds.height).abs() < 0.05,
            "visible {visible:?} of {:?} for {path:?}",
            field.bounds
        );
        assert!(field.bounds.height >= 38.0);
    };
    assert_visible(&frame, &email);
    assert_eq!(input.focus_next_text_field(&scene, &frame), Some(true));
    let frame = layout_with_input(&scene, landscape, &mut FixedText, &input);
    input.reconcile(&scene, &frame);
    assert!(input.is_focused(&telephone));
    assert_visible(&frame, &telephone);
    assert_eq!(input.focus_next_text_field(&scene, &frame), Some(true));
    let frame = layout_with_input(&scene, landscape, &mut FixedText, &input);
    input.reconcile(&scene, &frame);
    assert!(input.is_focused(&website));
    assert_visible(&frame, &website);
    assert_eq!(
        scene.at_path(&email).unwrap().text("text"),
        Some("inspector@example.org")
    );

    input.clear_focus();
    set_window_safe_area(&mut scene, [0.0; 4]);
    let expanded = layout_with_input(&scene, landscape, &mut FixedText, &input);
    assert_visible(&expanded, &ui.path("close-contact"));
    assert_visible(&expanded, &email);
}

#[test]
fn ios_contact_form_reveals_each_next_field_in_a_compact_landscape_dialog()
-> Result<(), Box<dyn std::error::Error>> {
    keep_contact_visible_during_landscape_next(TouchDemo::new(), |app| app.set_compact(true));
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let component = load_mlir(&module.mlir)?.create("TouchDemo")?;
    keep_contact_visible_during_landscape_next(component, |app| {
        app.set("compact", HostValue::Bool(true)).unwrap();
    });
    Ok(())
}

fn avoid_floating_keyboard_in_readings<C: Component>(component: C) {
    let size = Size::new(744.0, 1133.0);
    let mut ui = UiDriver::headless(component, size);
    ui.click("readings-toggle");
    ui.click("form-search");
    ui.fill_focused("73");
    let path = ui.path("form-search");
    let viewport = ui.path("readings-viewport");
    let navigation = ui.path("notes-toggle");
    let scene = ui.scene();
    let mut input = Interaction::default();
    let unshifted = layout_with_input(scene, size, &mut FixedText, &input);
    let target = input
        .accessibility_target(scene, &unshifted, &path)
        .unwrap();
    input.focus_target(scene, &unshifted, target.clone());
    let original = unshifted.at_path(&path).unwrap().bounds;
    let form = unshifted.at_path(&viewport).unwrap();
    assert_eq!(
        form.scroll_on(xrack_runtime::Orientation::Vertical)
            .unwrap()
            .max_offset,
        0.0
    );
    let keyboard = Rect::new(original.x - 32.0, original.y - 24.0, 320.0, 280.0);
    assert!(input.avoid_floating_keyboard(scene, &unshifted, Some(keyboard)));
    let shifted = layout_with_input(scene, size, &mut FixedText, &input);
    let editor = shifted.at_path(&path).unwrap();
    assert_eq!(editor.bounds.intersect(keyboard).height, 0.0);
    assert_eq!(editor.visible_window_bounds(editor.bounds), editor.bounds);
    assert_eq!(shifted.at_path(&viewport).unwrap().bounds, form.bounds);
    assert_eq!(
        shifted.at_path(&navigation).unwrap().bounds,
        unshifted.at_path(&navigation).unwrap().bounds
    );
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert!(input.scroll_offsets().is_empty());
    assert!(!input.avoid_floating_keyboard(scene, &shifted, Some(keyboard)));

    let away = Rect {
        y: 700.0,
        ..keyboard
    };
    assert!(input.avoid_floating_keyboard(scene, &shifted, Some(away)));
    let restored = layout_with_input(scene, size, &mut FixedText, &input);
    assert_eq!(restored.at_path(&path).unwrap().bounds, original);
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert_eq!(scene.at_path(&path).unwrap().text("text"), Some("73"));

    let high_keyboard = Rect {
        y: 20.0,
        ..keyboard
    };
    assert!(input.avoid_floating_keyboard(scene, &restored, Some(high_keyboard)));
    let shifted = layout_with_input(scene, size, &mut FixedText, &input);
    let editor = shifted.at_path(&path).unwrap();
    assert!(editor.bounds.y >= high_keyboard.y + high_keyboard.height);
    assert_eq!(editor.visible_window_bounds(editor.bounds), editor.bounds);
    assert!(input.avoid_floating_keyboard(scene, &shifted, None));
    let restored = layout_with_input(scene, size, &mut FixedText, &input);
    assert_eq!(restored.at_path(&path).unwrap().bounds, original);
}

#[test]
fn floating_keyboard_repositions_only_the_active_readings_viewport()
-> Result<(), Box<dyn std::error::Error>> {
    avoid_floating_keyboard_in_readings(TouchDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    avoid_floating_keyboard_in_readings(load_mlir(&module.mlir)?.create("TouchDemo")?);
    Ok(())
}

fn exercise_native_action_fixture(app: impl Component) {
    use xrack_layout::NavigationKey;
    use xrack_runtime::Orientation;

    let mut ui = UiDriver::headless(app, Size::new(390.0, 720.0));
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Record review"));
    assert_eq!(ui.node("review-count").text("text"), Some("Reviews 1"));

    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Read service log"));
    let visible = |ui: &UiDriver<_>, id: &str| {
        let item = ui.layout_at(&ui.path(id)).unwrap();
        let visible = item.visible_window_bounds(item.bounds);
        visible.width > 0.0 && visible.height > 0.0
    };
    assert!(visible(&ui, "log-morning"));
    assert!(!visible(&ui, "log-night"));
    assert!(!visible(&ui, "log-parts"));
    assert!(ui.scroll("log-hours", Point { x: 152.0, y: 0.0 }));
    assert!(visible(&ui, "log-night"));
    assert!(visible(&ui, "log-heading"));
    assert!(ui.scroll("log-viewport", Point { x: 0.0, y: 170.0 }));
    assert!(visible(&ui, "log-parts"));
    assert!(!visible(&ui, "log-night"));
    assert!(ui.scroll("log-viewport", Point { x: 0.0, y: -170.0 }));
    assert!(visible(&ui, "log-night"));
    assert!(!visible(&ui, "log-morning"));
    ui.escape();

    let table = ui.path("instrument-table");
    let row = |slot| [table.as_slice(), &[slot]].concat();
    let moved = ui.scene().at_path(&row(1)).unwrap().row_key;
    for (slot, direction, expected) in [
        (1, NavigationKey::Up, "Moved row 2 to 1"),
        (0, NavigationKey::Down, "Moved row 1 to 2"),
    ] {
        let target = ui
            .input()
            .accessibility_target(ui.scene(), ui.layout(), &row(slot))
            .unwrap();
        assert_eq!(target.row_key, moved);
        let action = ui
            .input()
            .table_row_header_move(ui.scene(), ui.layout(), &target, direction)
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        assert_eq!(ui.node("row-movement").text("text"), Some(expected));
    }

    let instruments = |ui: &UiDriver<_>| {
        [0, 1, 2].map(|slot| {
            ui.scene()
                .at_path(&[row(slot).as_slice(), &[0, 0]].concat())
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        })
    };
    for (descending, order) in [
        (false, ["Gauge", "Sensor", "Valve"]),
        (true, ["Valve", "Sensor", "Gauge"]),
    ] {
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &table, ControlAction::SortColumn(0))));
        assert_eq!(
            ui.node("instrument-table")
                .table_header
                .as_ref()
                .unwrap()
                .sort,
            Some(xrack_runtime::TableSort {
                column: 0,
                descending
            })
        );
        assert_eq!(instruments(&ui), order);
    }
    ui.click("reset-sort");
    assert!(
        ui.node("instrument-table")
            .table_header
            .as_ref()
            .unwrap()
            .sort
            .is_none()
    );
    assert_eq!(instruments(&ui), ["Gauge", "Valve", "Sensor"]);

    let cell = [row(0).as_slice(), &[0, 1]].concat();
    let mut input = xrack_layout::Interaction::default();
    for (draft, save) in [("Bay 7", true), ("Discard", false)] {
        use xrack_layout::{EditAction, EditSource};
        let scene = ui.scene().clone();
        let target = input
            .accessibility_target(&scene, ui.layout(), &cell)
            .unwrap();
        assert!(input.begin_accessible_table_edit(&scene, ui.layout(), target.clone()));
        ui.update(|app| {
            assert!(input.edit_text(
                app,
                &scene,
                EditAction::Insert(draft),
                EditSource::Atomic,
                std::time::Instant::now(),
            ));
            if save {
                assert!(input.commit_table_edit(app, &scene));
            } else {
                assert!(input.cancel_table_edit());
            }
        });
        assert_eq!(input.focused.as_ref(), Some(&target));
        assert_eq!(ui.scene().row_key_at(&cell), target.row_key);
        assert_eq!(
            ui.scene().at_path(&cell).unwrap().text("text"),
            Some("Bay 7")
        );
    }

    let tree = ui.path("inspection-tree");
    let branch = [tree.as_slice(), &[0]].concat();
    let identity = ui.scene().row_key_at(&branch);
    for (expanded, keys) in [(true, &[1, 2, 3, 4][..]), (false, &[1, 4][..])] {
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &branch)
            .unwrap();
        assert_eq!(target.row_key, identity);
        let action = input
            .tree_row_expansion(ui.scene(), ui.layout(), &target, expanded)
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        assert_eq!(
            ui.node("inspection-tree")
                .tree
                .as_ref()
                .unwrap()
                .rows()
                .iter()
                .map(|row| row.key)
                .collect::<Vec<_>>(),
            keys
        );
    }

    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Manage tabs"));
    input.reconcile(ui.scene(), ui.layout());
    let tabs = ui.path("tabs-view");
    let draft = [tabs.as_slice(), &[1, 0, 0]].concat();
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &draft)
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), target.clone());
    input.remember_focus_scope(ui.scene(), ui.layout());
    assert_eq!(input.focused.as_ref(), Some(&target));
    for (slot, expected) in [(2, "draft"), (1, "pinned")] {
        let path = [tabs.as_slice(), &[slot]].concat();
        let closing = input
            .accessibility_tab_close_target(ui.scene(), ui.layout(), &path)
            .unwrap();
        let action = input
            .tab_close_action(ui.scene(), ui.layout(), &closing)
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        input.reconcile(ui.scene(), ui.layout());
        assert!(
            input
                .tab_close_action(ui.scene(), ui.layout(), &closing)
                .is_none()
        );
        let selected = xrack_runtime::group_index(ui.node("tabs-view")).unwrap();
        assert_eq!(
            ui.node("tabs-view")
                .child_at(selected)
                .unwrap()
                .text("value"),
            Some(expected)
        );
        if slot == 2 {
            assert_eq!(input.focused.as_ref(), Some(&target));
        } else {
            assert!(
                input
                    .focused
                    .as_ref()
                    .unwrap()
                    .path
                    .starts_with(&[tabs.as_slice(), &[selected]].concat())
            );
        }
    }

    ui.escape();
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Browse narrow tabs"));
    let strip = ui.path("overflow-tabs");
    let archive = [strip.as_slice(), &[3]].concat();
    let visible = |layout: &xrack_layout::LayoutNode| {
        layout.at_path(&archive).is_some_and(|item| {
            item.visible_window_bounds(item.tab_header.unwrap_or(item.bounds))
                .width
                > 0.0
        })
    };
    assert!(!visible(ui.layout()));
    let scroll = ui
        .layout()
        .at_path(&strip)
        .unwrap()
        .scroll_on(Orientation::Horizontal)
        .unwrap();
    assert!(input.accessibility_scroll_to(
        ui.layout(),
        &strip,
        Orientation::Horizontal,
        scroll.max_offset
    ));
    let scrolled = xrack_layout::layout_with_input(
        ui.scene(),
        Size::new(390.0, 720.0),
        &mut FixedText,
        &input,
    );
    assert!(visible(&scrolled));
    assert_eq!(
        xrack_runtime::group_index(ui.node("overflow-tabs")),
        Some(0)
    );

    ui.escape();
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Disclosure controls"));
    let details = ui.path("details");
    let optional = ui.path("optional");
    let scene = ui.scene().clone();
    assert!(ui.find_path("network-details").is_err());
    ui.update(|app| assert!(app.control_at(&scene, &details, ControlAction::Expander(true))));
    ui.click("network-details");
    assert_eq!(ui.node("checks-status").text("text"), Some("Checks 1"));
    input.reconcile(ui.scene(), ui.layout());
    let field = ui.path("network-details");
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &field)
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), target);
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &details, ControlAction::Expander(false))));
    input.reconcile(ui.scene(), ui.layout());
    assert_eq!(input.focused.as_ref().unwrap().path, details);
    assert!(ui.find_path("network-details").is_err());

    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.dispatch_at(&scene, &optional, "clicked")));
    assert!(!ui.can_hit("test-settings"));
    assert_eq!(ui.node("checks-status").text("text"), Some("Checks 1"));
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.dispatch_at(&scene, &optional, "clicked")));
    ui.click("test-settings");
    assert_eq!(ui.node("checks-status").text("text"), Some("Checks 2"));

    ui.escape();
    input.reconcile(ui.scene(), ui.layout());
    let opener = ui.path("review-trigger");
    let original = input
        .accessibility_target(ui.scene(), ui.layout(), &opener)
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), original.clone());
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Open tools drawer"));
    input.reconcile(ui.scene(), ui.layout());
    assert!(
        input
            .accessibility_target(ui.scene(), ui.layout(), &opener)
            .is_none()
    );
    assert_eq!(input.focused.as_ref().unwrap().path, ui.path("run-tool"));
    ui.click("run-tool");
    let close = input.dialog_escape(ui.scene(), ui.layout()).unwrap();
    assert_eq!(close.action, ControlAction::Drawer(false));
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &close.path, close.action)));
    input.reconcile(ui.scene(), ui.layout());
    assert_eq!(input.focused.as_ref(), Some(&original));
    assert_eq!(
        ui.node("tool-closures").text("text"),
        Some("Tool closures 1")
    );
}

#[test]
fn native_action_demo_preserves_row_identity_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_native_action_fixture(native_actions_ui::NativeActionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_native_action_fixture(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

#[test]
fn inspection_cards_share_navigation_and_callbacks_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise<C: Component>(component: C) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
        assert!(ui.context_menu("review-trigger"));
        assert!(ui.click_menu_row("Inspect cards"));
        let path = ui.path("inspection-cards");
        assert!(ui.can_hit("card-summary"));
        let select = |ui: &mut UiDriver<C>, key| {
            let index = xrack_layout::swipe_view_selection(
                ui.node("inspection-cards"),
                ui.layout_at(&path).unwrap(),
                key,
            )
            .unwrap();
            let scene = ui.scene().clone();
            ui.update(|app| assert!(app.control_at(&scene, &path, ControlAction::Select(index))));
        };
        select(&mut ui, NavigationKey::Right);
        assert_eq!(
            xrack_runtime::group_index(ui.node("inspection-cards")),
            Some(3)
        );
        assert_eq!(ui.node("card-status").text("text"), Some("Card changes: 1"));
        ui.click("mirror-cards");
        select(&mut ui, NavigationKey::Right);
        assert_eq!(
            xrack_runtime::group_index(ui.node("inspection-cards")),
            Some(0)
        );
        assert_eq!(ui.node("card-status").text("text"), Some("Card changes: 2"));
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::NativeActionsDemo::new());
    exercise(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}
