#![cfg(test)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use touch_ui::TouchDemo;
use xrack_interpreter::{HostValue, load_mlir};
use xrack_layout::{
    ActivationKey, CalendarGeometry, EditAction, Interaction, MenuKey, NavigationKey, Rect,
    ScrollGestureEnd, ScrollGestureKind, TextMeasurer, layout_with_input, set_window_safe_area,
};
use xrack_runtime::{
    CalendarAction, Color, Component, ControlAction, Date, DialogRole, DragEvent, FileDialogMode,
    MessageButtons, NativeDialogOptions, NativeDialogQueue, NativeDialogResult, SceneTransition,
    SpinAction, StandardButtonKind, SwipeSide, TextCommand, Tumbler, text_input_value,
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

    let bounds = ui.layout_at(&row).unwrap().bounds;
    let start = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let end = Point {
        x: start.x - 110.0,
        ..start
    };
    let now = std::time::Instant::now();
    let mut interrupted = Interaction::default();
    assert!(interrupted.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        now,
        xrack_layout::ScrollGestureKind::Touch,
    ));
    assert!(interrupted.move_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(40),
    ));
    assert!(interrupted.has_swipe_preview());
    interrupted.cancel_scroll_gesture();
    assert!(!interrupted.has_swipe_preview());
    let release = interrupted.end_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(80),
    );
    assert!(!release.tap && release.control.is_none());
    assert!(!ui.layout_at(&action).unwrap().enabled);
    assert_eq!(
        ui.node("task-list").list.map(|list| list.row_count),
        Some(9)
    );

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

struct ScaledText(f32);

impl TextMeasurer for ScaledText {
    fn text_scale(&self) -> f32 {
        self.0
    }

    fn font_size(&self, node: &xrack_runtime::Node) -> f32 {
        node.font_size().unwrap_or(xrack_layout::DEFAULT_FONT_SIZE) * self.0
    }

    fn measure(&mut self, value: &str, font_size: f32, max_width: f32) -> Size {
        FixedText.measure(value, font_size, max_width)
    }
}

fn exercise_scaled_context_menu<C: Component>(component: C) {
    let size = Size::new(744.0, 460.0);
    let mut ui = UiDriver::headless(component, size);
    let mut scene = ui.component().render_shared();
    set_window_safe_area(Rc::make_mut(&mut scene), [12.0, 54.0, 8.0, 34.0]);
    let mut input = Interaction::default();
    let normal = layout_with_input(&scene, size, &mut ScaledText(1.0), &input);
    let bounds = normal.at_path(&ui.path("review-trigger")).unwrap().bounds;
    let point = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let trigger = input
        .touch_context_menu_target(&scene, &normal, point)
        .unwrap();
    assert!(input.open_touch_context_menu(&scene, &normal, trigger, point));
    let panels = input.menu_panels(&scene, &normal);
    let panel = &panels[0];
    let normal_bounds = panel.bounds;
    let normal_count = panel.rows.len();
    assert_eq!(panel.rows[0].label, "Record review");
    assert_eq!(panel.rows[0].font_size, 15.0);
    assert_eq!(panel.row_bounds(0).height, 44.0);
    let entry = panel.entry(0);

    let large = layout_with_input(&scene, size, &mut ScaledText(3.0), &input);
    input.reconcile(&scene, &large);
    let review = ui.path("review-trigger");
    let review_node = scene.at_path(&review).unwrap();
    let review_bounds = large.at_path(&review).unwrap().bounds;
    assert_eq!(review_bounds.height, 44.0);
    let review_content = xrack_layout::ButtonContent::new(review_node);
    let requested = ScaledText(3.0).font_size(review_node);
    let (area, fitted) = review_content.scaled_label_metrics(review_bounds, requested);
    assert!(fitted < requested);
    assert!((fitted * 1.4 - area.height).abs() < 0.01);
    assert_eq!(review_content.label, "Review options");
    let panels = input.menu_panels(&scene, &large);
    let panel = &panels[0];
    assert_eq!(panel.entry(0), entry);
    assert_eq!(panel.rows[0].font_size, 45.0);
    assert!(panel.row_bounds(0).height >= 45.0 * 1.4);
    assert!(panel.bounds.width > normal_bounds.width);
    assert!(panel.rows.len() < normal_count);
    assert!(panel.bounds.x >= 16.0 && panel.bounds.y >= 58.0);
    assert!(panel.bounds.x + panel.bounds.width <= size.width - 12.0);
    assert!(panel.bounds.y + panel.bounds.height <= size.height - 38.0);

    assert!(input.menu_key(&scene, &large, MenuKey::End).is_none());
    let panels = input.menu_panels(&scene, &large);
    let last = &panels[0];
    assert!(last.row_index(0) > 0);
    assert!(last.row_highlighted(last.rows.len() - 1));
    assert!(input.menu_key(&scene, &large, MenuKey::Home).is_none());
    let trigger = input.menu_trigger().cloned().unwrap();
    let action = input
        .accessibility_menu_entry(&scene, &large, &trigger, &entry, true)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert_eq!(ui.node("review-count").text("text"), Some("Reviews 1"));
}

fn exercise_scaled_choice_popups<C: Component>(component: C) {
    let size = Size::new(390.0, 600.0);
    let mut ui = UiDriver::headless(component, size);
    for (id, first_label, last_label) in [
        ("category", "Boilers", "Valves"),
        ("lookup", "Pump 01", "Pump 12"),
    ] {
        if id == "lookup" {
            ui.click(id);
            ui.fill_focused("Pump");
        }
        let mut scene = ui.component().render_shared();
        if id == "lookup" {
            set_window_safe_area(Rc::make_mut(&mut scene), [10.0, 48.0, 10.0, 170.0]);
        }
        let path = ui.path(id);
        let mut input = Interaction::default();
        let standard = layout_with_input(&scene, size, &mut ScaledText(1.0), &input);
        input.reconcile(&scene, &standard);
        let target = input
            .accessibility_target(&scene, &standard, &path)
            .unwrap();
        assert!(input.open_choice(&scene, &standard, target.clone()));
        let popup = input.choice_menu(&scene, &standard).unwrap();
        assert_eq!(popup.rows[0], first_label);
        assert_eq!(popup.row_bounds(0).height, 28.0);
        let standard_width = popup.bounds.width;
        let standard_rows = popup.rows.len();

        let large = layout_with_input(&scene, size, &mut ScaledText(3.0), &input);
        input.reconcile(&scene, &large);
        let popup = input
            .choice_menu(&scene, &large)
            .unwrap_or_else(|| panic!("{id} closed after relayout"));
        assert_eq!(popup.font_size, 42.0);
        assert!(popup.row_bounds(0).height >= 42.0 * 1.4);
        assert!(popup.bounds.width >= standard_width);
        assert!(popup.rows.len() < standard_rows);
        if id == "lookup" {
            assert!(popup.bounds.y >= 48.0);
            assert!(popup.bounds.y + popup.bounds.height <= 430.0);
        }
        let offsets = input.scroll_offsets().clone();
        while let Some(popup) = input
            .choice_menu(&scene, &large)
            .filter(|popup| popup.can_scroll(false))
        {
            if id == "category" {
                let first = popup.first;
                let row = popup.row_bounds(popup.rows.len() - 1);
                let start = Point {
                    x: row.x + row.width * 0.5,
                    y: row.y + row.height * 0.5,
                };
                let end = Point {
                    x: start.x,
                    y: start.y - row.height * 1.5,
                };
                let now = Instant::now();
                assert!(input.begin_scroll_gesture(
                    &scene,
                    &large,
                    start,
                    now,
                    ScrollGestureKind::Touch
                ));
                assert!(input.move_scroll_gesture(
                    &scene,
                    &large,
                    end,
                    now + Duration::from_millis(100)
                ));
                let outcome =
                    input.end_scroll_gesture(&scene, &large, end, now + Duration::from_millis(140));
                assert!(!outcome.tap && outcome.control.is_none());
                assert!(input.choice_menu(&scene, &large).unwrap().first > first);
            } else {
                assert!(input.choice_page(&scene, &large, false));
            }
        }
        assert_eq!(input.focused.as_ref(), Some(&target));
        assert_eq!(input.scroll_offsets(), &offsets);

        let popup = input.choice_menu(&scene, &large).unwrap();
        assert_eq!(popup.rows.last().unwrap(), last_label);
        let row = popup.row_bounds(popup.rows.len() - 1);
        input.pointer_move(
            &scene,
            &large,
            Point {
                x: row.x + row.width * 0.5,
                y: row.y + row.height * 0.5,
            },
        );
        assert!(input.choice_press(&scene, &large));
        let action = input.choice_release(&scene, &large).unwrap();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        assert!(!input.choice_open());
        if id == "category" {
            assert_eq!(ui.node("category").int("current-index"), Some(12));
        } else {
            assert_eq!(ui.node("lookup").text("text"), Some("Pump 12"));
            assert_eq!(
                ui.node("submitted-choice").text("text"),
                Some("Choose a suggestion")
            );
            let selected = ui.component().render_shared();
            ui.update(|app| assert!(app.control_at(&selected, &path, ControlAction::SubmitSearch)));
            assert_eq!(
                ui.node("submitted-choice").text("text"),
                Some("Submitted: Pump 12")
            );
        }
    }
}

#[test]
fn choice_popups_scale_scroll_and_commit_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    exercise_scaled_choice_popups(native_actions_ui::ChoicePopupDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_scaled_choice_popups(load_mlir(&module.mlir)?.create("ChoicePopupDemo")?);
    Ok(())
}

fn exercise_scaled_tabs<C: Component>(component: C) {
    let size = Size::new(324.0, 644.0);
    let mut ui = UiDriver::headless(component, size);
    let mut menu = Interaction::default();
    let scene = ui.scene().clone();
    let bounds = ui.bounds("review-trigger");
    let point = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let trigger = menu
        .touch_context_menu_target(&scene, ui.layout(), point)
        .unwrap();
    assert!(menu.open_touch_context_menu(&scene, ui.layout(), trigger.clone(), point));
    let panels = menu.menu_panels(&scene, ui.layout());
    let panel = &panels[0];
    let row = panel
        .rows
        .iter()
        .position(|row| row.label == "Manage tabs")
        .unwrap();
    let entry = panel.entry(row);
    let action = menu
        .accessibility_menu_entry(&scene, ui.layout(), &trigger, &entry, true)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    let bar = ui.path("tabs-view");
    let mut draft = bar.clone();
    draft.push(1);
    let mut results = bar.clone();
    results.push(2);
    let mut input = Interaction::default();
    let scene = ui.component().render_shared();
    let normal = layout_with_input(&scene, size, &mut ScaledText(1.0), &input);
    input.reconcile(&scene, &normal);
    let large = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    input.reconcile(&scene, &large);
    let before = normal.at_path(&draft).unwrap();
    let after = large.at_path(&draft).unwrap();
    assert!(after.tab_header.unwrap().height > before.tab_header.unwrap().height);
    assert_eq!(after.tab_close.unwrap().width, 44.0);
    let draft_target = input.accessibility_target(&scene, &large, &draft).unwrap();
    input.focus_target(&scene, &large, draft_target);
    let navigate = input
        .group_key(&scene, &large, NavigationKey::Right)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &navigate.path, navigate.action)));
    assert_eq!(ui.node("tabs-view").text("current-value"), Some("results"));

    let scene = ui.component().render_shared();
    let large = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    input.reconcile(&scene, &large);
    let close = input
        .accessibility_tab_close_target(&scene, &large, &results)
        .unwrap();
    let action = input.tab_close_action(&scene, &large, &close).unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert_eq!(ui.node("tabs-view").text("current-value"), Some("pinned"));
    assert_eq!(ui.node("tabs-view").children.len(), 2);
}

#[test]
fn scaled_tab_strip_keeps_navigation_and_close_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_scaled_tabs(native_actions_ui::NativeActionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_scaled_tabs(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

fn exercise_floating_choice_popups<C: Component>(component: C) {
    let size = Size::new(520.0, 700.0);
    let mut ui = UiDriver::headless(component, size);
    for (id, last_label) in [("category", "Valves"), ("lookup", "Pump 12")] {
        if id == "lookup" {
            ui.click(id);
            ui.fill_focused("Pump");
        }
        let mut input = Interaction::default();
        let scene = ui.component().render_shared();
        let layout = layout_with_input(&scene, size, &mut FixedText, &input);
        input.reconcile(&scene, &layout);
        let path = ui.path(id);
        let target = input.accessibility_target(&scene, &layout, &path).unwrap();
        assert!(input.open_choice(&scene, &layout, target.clone()));
        let original = input.choice_menu(&scene, &layout).unwrap().bounds;
        assert!(
            input
                .choice_key(&scene, &layout, xrack_layout::ChoiceKey::Down)
                .is_none()
        );
        let highlight = input.choice_menu(&scene, &layout).unwrap().highlighted;
        assert!(highlight.is_some());

        let keyboard = Rect::new(original.x + 16.0, original.y + 25.0, original.width, 240.0);
        input.set_floating_keyboard(Some(keyboard));
        let layout = layout_with_input(&scene, size, &mut FixedText, &input);
        input.reconcile(&scene, &layout);
        let popup = input
            .choice_menu(&scene, &layout)
            .unwrap_or_else(|| panic!("{id} closed when the floating keyboard moved over it"));
        assert_ne!(popup.bounds, original);
        let intersection = popup.bounds.intersect(keyboard);
        assert!(intersection.width == 0.0 || intersection.height == 0.0);
        assert_eq!(popup.highlighted, highlight);
        assert_eq!(input.focused.as_ref(), Some(&target));
        assert!(input.scroll_offsets().is_empty());

        input.set_floating_keyboard(None);
        let restored = layout_with_input(&scene, size, &mut FixedText, &input);
        input.reconcile(&scene, &restored);
        assert_eq!(
            input.choice_menu(&scene, &restored).unwrap().bounds,
            original
        );

        input.set_floating_keyboard(Some(keyboard));
        while input.choice_page(&scene, &layout, false) {}
        let popup = input.choice_menu(&scene, &layout).unwrap();
        assert_eq!(popup.rows.last().map(String::as_str), Some(last_label));
        let bounds = popup.row_bounds(popup.rows.len() - 1);
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bounds.x + bounds.width * 0.5,
                y: bounds.y + bounds.height * 0.5,
            },
        );
        assert!(input.choice_press(&scene, &layout));
        let action = input.choice_release(&scene, &layout).unwrap();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        assert!(!input.choice_open());
        if id == "category" {
            assert_eq!(ui.node(id).int("current-index"), Some(12));
        } else {
            assert_eq!(ui.node(id).text("text"), Some(last_label));
            let selected = ui.component().render_shared();
            ui.update(|app| assert!(app.control_at(&selected, &path, ControlAction::SubmitSearch)));
            assert_eq!(
                ui.node("submitted-choice").text("text"),
                Some("Submitted: Pump 12")
            );
        }
    }
}

#[test]
fn choice_popups_avoid_a_floating_keyboard_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_floating_choice_popups(native_actions_ui::ChoicePopupDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_floating_choice_popups(load_mlir(&module.mlir)?.create("ChoicePopupDemo")?);
    Ok(())
}

#[test]
fn context_menu_tracks_system_text_size_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    exercise_scaled_context_menu(native_actions_ui::NativeActionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_scaled_context_menu(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

fn exercise_scaled_inspection_table<C: Component>(component: C) {
    let size = Size::new(402.0, 820.0);
    let mut ui = UiDriver::headless(component, size);
    let path = ui.path("instrument-table");
    let mut sensor = path.clone();
    sensor.extend([1, 0, 0]);
    let mut location = path.clone();
    location.extend([0, 0, 2]);
    let mut input = Interaction::default();
    let scene = ui.scene().clone();
    let frame = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    let table = frame.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(table.header.height, 40.0);
    let header = table.columns[0];
    input.pointer_move(
        &scene,
        &frame,
        Point {
            x: header.x + header.width * 0.5,
            y: header.y + header.height * 0.5,
        },
    );
    input.pointer_press_in_scene(&scene, &frame);
    let action = input
        .table_header_release(&scene, xrack_runtime::RowSelection::Replace, true)
        .unwrap();
    assert_eq!(action.action, ControlAction::SortColumn(0));
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert_eq!(
        ui.scene().at_path(&sensor).unwrap().text("text"),
        Some("Sensor")
    );

    let scene = ui.scene().clone();
    let frame = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    let target = input
        .accessibility_target(&scene, &frame, &location)
        .unwrap();
    assert!(input.begin_accessible_table_edit(&scene, &frame, target));
    assert_eq!(
        input.editor_value(scene.at_path(&location).unwrap()),
        "Pump room"
    );
    ui.update(|app| {
        assert!(input.edit_text(
            app,
            &scene,
            EditAction::Insert("Bay 7"),
            xrack_layout::EditSource::Atomic,
            Instant::now(),
        ));
        assert!(input.commit_table_edit(app, &scene));
    });
    assert_eq!(
        ui.scene().at_path(&location).unwrap().text("text"),
        Some("Bay 7")
    );
}

#[test]
fn scaled_inspection_table_can_sort_and_edit_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_scaled_inspection_table(native_actions_ui::NativeActionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_scaled_inspection_table(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

fn exercise_compact_rows<C: Component>(component: C) {
    let size = Size::new(402.0, 720.0);
    let mut ui = UiDriver::headless(component, size);
    let list = ui.path("inspections-list");
    let item = [list.as_slice(), &[1]].concat();
    let tree = ui.path("hierarchy");
    let branch = [tree.as_slice(), &[0]].concat();
    let note = [tree.as_slice(), &[1, 0, 1]].concat();
    let mut input = Interaction::default();
    let scene = ui.scene().clone();
    let layout = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    assert_eq!(layout.at_path(&item).unwrap().bounds.height, 32.0);
    let row = input.accessibility_target(&scene, &layout, &item).unwrap();
    let select = input.select_group_option(&scene, &layout, &row).unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &select.path, select.action)));
    assert_eq!(ui.node("selected-item").text("text"), Some("Selected 1"));

    let scene = ui.scene().clone();
    let layout = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    let row = input
        .accessibility_target(&scene, &layout, &branch)
        .unwrap();
    let expand = input
        .tree_row_expansion(&scene, &layout, &row, true)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &expand.path, expand.action)));
    assert_eq!(ui.scene().row_key_at(&branch), row.row_key);

    let scene = ui.scene().clone();
    let layout = layout_with_input(&scene, size, &mut ScaledText(2.6), &input);
    let editor = input.accessibility_target(&scene, &layout, &note).unwrap();
    assert!(input.begin_accessible_table_edit(&scene, &layout, editor));
    assert_eq!(
        input.editor_value(scene.at_path(&note).unwrap()),
        "Verify the pressure calibration"
    );
    ui.update(|app| {
        assert!(input.edit_text(
            app,
            &scene,
            EditAction::Insert("Ready"),
            xrack_layout::EditSource::Atomic,
            Instant::now(),
        ));
        assert!(input.commit_table_edit(app, &scene));
    });
    assert_eq!(
        ui.scene().at_path(&note).unwrap().text("text"),
        Some("Ready")
    );
}

#[test]
fn compact_rows_select_expand_and_edit_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    exercise_compact_rows(native_actions_ui::CompactRowsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_compact_rows(load_mlir(&module.mlir)?.create("CompactRowsDemo")?);
    Ok(())
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

fn exercise_folder_dialog<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 430.0));
    let mut queue = NativeDialogQueue::default();
    for (button, mode, result, expected) in [
        (
            "one",
            FileDialogMode::SelectFolder,
            NativeDialogResult::Files(vec!["/tmp/Pump notes".into()]),
            "Imported 1",
        ),
        (
            "many",
            FileDialogMode::SelectFolders,
            NativeDialogResult::Files(vec!["/tmp/Pump notes".into(), "/tmp/Valve notes".into()]),
            "Imported 2",
        ),
        (
            "many",
            FileDialogMode::SelectFolders,
            NativeDialogResult::Canceled,
            "Folder selection canceled",
        ),
        (
            "one",
            FileDialogMode::SelectFolder,
            NativeDialogResult::Failed("Folder provider unavailable".into()),
            "Folder provider unavailable",
        ),
    ] {
        ui.click(button);
        let scene = ui.component().render_shared();
        queue.sync(&[&scene.native_dialogs[0]]);
        let request = queue.begin_next().unwrap();
        let NativeDialogOptions::File(options) = request.options.as_ref() else {
            panic!("expected a folder dialog");
        };
        assert_eq!(options.mode, mode);
        assert!(queue.complete(request.id, result));
        ui.update(|_| {});
        assert_eq!(ui.node("result").text("text"), Some(expected));
    }
}

#[test]
fn declarative_folder_dialog_matches_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_folder_dialog(touch_ui::FolderDialogDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_folder_dialog(load_mlir(&module.mlir)?.create("FolderDialogDemo")?);
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
    show_saved_folder: impl FnOnce(&mut C),
    set_pending: impl FnOnce(&mut C),
    calls: &Cell<u32>,
    reloads: &Cell<u32>,
) {
    let mut ui = UiDriver::headless(component, Size::new(402.0, 680.0));
    ui.click("notes-toggle");
    assert!(!ui.node("import-folder").boolean("visible", true));
    assert!(!ui.node("reload-folder").boolean("visible", true));
    ui.update(show_folder);
    ui.click("import-folder");
    assert_eq!(calls.get(), 1);
    assert!(!ui.node("reload-folder").boolean("visible", true));
    ui.update(show_saved_folder);
    ui.click("reload-folder");
    assert_eq!(reloads.get(), 1);

    ui.click("note-lock");
    assert!(!ui.node("import-folder").boolean("enabled", true));
    assert!(!ui.node("reload-folder").boolean("enabled", true));
    assert_eq!(calls.get(), 1);

    ui.click("note-lock");
    ui.click("import-note");
    assert!(!ui.node("import-folder").boolean("enabled", true));
    assert!(!ui.node("reload-folder").boolean("enabled", true));
    let mut queue = NativeDialogQueue::default();
    let scene = ui.component().render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let request = queue.begin_next().unwrap();
    assert!(queue.complete(request.id, NativeDialogResult::Canceled));
    ui.update(|_| {});
    assert!(ui.node("import-folder").boolean("enabled", false));
    assert!(ui.node("reload-folder").boolean("enabled", false));

    ui.update(set_pending);
    assert!(!ui.node("import-folder").boolean("enabled", true));
    assert!(!ui.node("import-note").boolean("enabled", true));
    assert!(!ui.node("reload-folder").boolean("enabled", true));
    assert_eq!(calls.get(), 1);
    assert_eq!(reloads.get(), 1);
}

#[test]
fn folder_button_honors_host_availability_and_note_locks() -> Result<(), Box<dyn std::error::Error>>
{
    let calls = Rc::new(Cell::new(0));
    let reloads = Rc::new(Cell::new(0));
    let mut compiled = TouchDemo::new();
    compiled.on_import_folder({
        let calls = calls.clone();
        move || calls.set(calls.get() + 1)
    });
    compiled.on_reload_folder({
        let reloads = reloads.clone();
        move || reloads.set(reloads.get() + 1)
    });
    choose_note_folder(
        compiled,
        |app| app.set_folder_import_available(true),
        |app| app.set_saved_folder_available(true),
        |app| app.set_transfer_pending(true),
        &calls,
        &reloads,
    );
    calls.set(0);
    reloads.set(0);

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("TouchDemo")?;
    interpreted.on("import-folder", {
        let calls = calls.clone();
        Rc::new(move |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        })
    })?;
    interpreted.on("reload-folder", {
        let reloads = reloads.clone();
        Rc::new(move |_| {
            reloads.set(reloads.get() + 1);
            Ok(None)
        })
    })?;
    choose_note_folder(
        interpreted,
        |app| {
            app.set("folder-import-available", HostValue::Bool(true))
                .unwrap()
        },
        |app| {
            app.set("saved-folder-available", HostValue::Bool(true))
                .unwrap()
        },
        |app| app.set("transfer-pending", HostValue::Bool(true)).unwrap(),
        &calls,
        &reloads,
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

fn queue_native_panels<C: Component>(
    component: C,
    calls: &RefCell<Vec<(&'static str, bool)>>,
    complete: impl Fn(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    ui.click("start");
    assert!(!ui.can_hit("start"));
    assert!(calls.borrow().is_empty());
    let mut dialogs = NativeDialogQueue::default();
    let scene = ui.component().render_shared();
    dialogs.sync(&[&scene.native_dialogs[0]]);
    let notice = dialogs.begin_next().unwrap();
    assert!(
        matches!(notice.options.as_ref(), NativeDialogOptions::Message(options) if options.title == "System panel queue")
    );
    ui.advance_time(Duration::from_millis(250));
    assert_eq!(*calls.borrow(), [("queue", false)]);
    assert!(dialogs.complete(
        notice.id,
        NativeDialogResult::Button(StandardButtonKind::Ok)
    ));
    ui.update(|_| {});
    assert!(!ui.can_hit("start"));
    ui.update(&complete);
    assert!(ui.can_hit("start"));

    calls.borrow_mut().clear();
    ui.click("timed");
    assert_eq!(*calls.borrow(), [("queue", true)]);
    assert!(!ui.can_hit("start"));
    ui.advance_time(Duration::from_millis(6500));
    assert_eq!(*calls.borrow(), [("queue", true), ("abort", true)]);
    assert!(ui.scene().native_dialogs.is_empty());
    ui.advance_time(Duration::from_millis(6500));
    assert_eq!(
        *calls.borrow(),
        [("queue", true), ("abort", true), ("abort", false)]
    );
    ui.update(complete);
    assert!(!ui.can_hit("start"));
    dialogs.sync(&[&ui.scene().native_dialogs[0]]);
    let notice = dialogs.begin_next().unwrap();
    assert!(dialogs.complete(
        notice.id,
        NativeDialogResult::Button(StandardButtonKind::Ok)
    ));
    ui.update(|_| {});
    assert!(ui.can_hit("start"));
}

#[test]
fn native_panel_demo_requests_host_work_while_its_notice_is_pending()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = touch_ui::NativePanelQueueDemo::new();
    compiled.on_queue_panels({
        let calls = calls.clone();
        move |timed| calls.borrow_mut().push(("queue", timed))
    });
    compiled.on_abort_panel({
        let calls = calls.clone();
        move |files| calls.borrow_mut().push(("abort", files))
    });
    queue_native_panels(compiled, &calls, |app| app.set_panels_left(0));

    calls.borrow_mut().clear();
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("NativePanelQueueDemo")?;
    for (callback, name) in [("queue-panels", "queue"), ("abort-panel", "abort")] {
        interpreted.on(callback, {
            let calls = calls.clone();
            Rc::new(move |arguments| {
                let [HostValue::Bool(value)] = arguments else {
                    return Err("expected a boolean".into());
                };
                calls.borrow_mut().push((name, *value));
                Ok(None)
            })
        })?;
    }
    queue_native_panels(interpreted, &calls, |app| {
        app.set("panels-left", HostValue::Int(0)).unwrap()
    });
    Ok(())
}

fn remove_page_with_panel<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    for (button, is_file) in [("open-page-notice", false), ("open-page-files", true)] {
        ui.click(button);
        ui.click("show-inspection-dialog");
        let mut queue = NativeDialogQueue::default();
        queue.sync(&xrack_layout::WindowEffects::collect(ui.scene(), ui.layout()).dialogs);
        let page = queue
            .begin_next()
            .unwrap_or_else(|| panic!("No dialog from {button}"));
        assert_eq!(
            matches!(page.options.as_ref(), NativeDialogOptions::File(_)),
            is_file
        );

        ui.advance_time(Duration::from_millis(7500));
        queue.sync(&xrack_layout::WindowEffects::collect(ui.scene(), ui.layout()).dialogs);
        assert!(page.is_abandoned(), "{button} left an active dialog");
        assert!(queue.begin_next().is_none());
        let stale = if is_file {
            NativeDialogResult::Files(vec!["stale.txt".into()])
        } else {
            NativeDialogResult::Button(StandardButtonKind::Ok)
        };
        assert!(queue.complete(page.id, stale));
        ui.update(|_| {});
        assert_eq!(
            ui.node("page-status").text("text"),
            Some("Inspection page closed")
        );

        let next = queue.begin_next().unwrap();
        assert!(
            matches!(next.options.as_ref(), NativeDialogOptions::Message(options)
            if options.title == "System panel queue")
        );
        assert!(queue.complete(next.id, NativeDialogResult::Button(StandardButtonKind::Ok)));
        ui.update(|_| {});
        assert!(ui.can_hit(button));
    }
}

#[test]
fn removing_a_page_retires_only_its_own_dialog_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    remove_page_with_panel(touch_ui::NativePanelQueueDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    remove_page_with_panel(load_mlir(&module.mlir)?.create("NativePanelQueueDemo")?);
    Ok(())
}

fn contains(outer: Rect, inner: Rect) -> bool {
    let epsilon = 0.01;
    inner.x + epsilon >= outer.x
        && inner.y + epsilon >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + epsilon
        && inner.y + inner.height <= outer.y + outer.height + epsilon
}

fn edit_marker_color<C: Component>(
    component: C,
    requests: &Cell<usize>,
    pending: impl Fn(&mut C, bool),
    preview: impl FnOnce(&mut C),
    finish: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    assert_eq!(
        ui.node("swatch").color("background"),
        Some(Color(0x2d638880))
    );
    assert_eq!(ui.node("readout").text("text"), Some("#2D638880"));
    ui.click("alpha");
    assert!(!ui.node("alpha").boolean("checked", true));
    ui.click("edit");
    assert_eq!(requests.get(), 1);
    ui.update(|app| pending(app, true));
    assert!(!ui.can_hit("edit"));
    assert!(!ui.can_hit("alpha"));
    assert_eq!(
        ui.node("swatch").color("background"),
        Some(Color(0x2d638880))
    );
    ui.update(preview);
    assert_eq!(
        ui.node("swatch").color("background"),
        Some(Color(0xff638880))
    );
    assert_eq!(ui.node("readout").text("text"), Some("Preview #FF638880"));
    ui.update(|app| pending(app, false));
    assert_eq!(
        ui.node("swatch").color("background"),
        Some(Color(0x2d638880))
    );
    assert_eq!(ui.node("readout").text("text"), Some("#2D638880"));

    ui.click("edit");
    assert_eq!(requests.get(), 2);
    ui.update(|app| pending(app, true));
    ui.update(finish);
    assert!(ui.can_hit("edit"));
    assert_eq!(
        ui.node("swatch").color("background"),
        Some(Color(0xff638880))
    );
    assert_eq!(ui.node("readout").text("text"), Some("#FF638880"));
}

#[test]
fn native_color_demo_keeps_preview_and_committed_color_separate_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let requests = Rc::new(Cell::new(0));
    let mut compiled = touch_ui::NativeColorDemo::new();
    compiled.on_edit_color({
        let requests = requests.clone();
        move || requests.set(requests.get() + 1)
    });
    edit_marker_color(
        compiled,
        &requests,
        |app, pending| app.set_pending(pending),
        |app| {
            app.set_preview_color(Color(0xff638880));
            app.set_preview_readout("#FF638880".into());
        },
        |app| {
            app.set_marker_color(Color(0xff638880));
            app.set_color_readout("#FF638880".into());
            app.set_pending(false);
        },
    );

    requests.set(0);
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreted = load_mlir(&module.mlir)?.create("NativeColorDemo")?;
    interpreted.on("edit-color", {
        let requests = requests.clone();
        Rc::new(move |_| {
            requests.set(requests.get() + 1);
            Ok(None)
        })
    })?;
    edit_marker_color(
        interpreted,
        &requests,
        |app, pending| app.set("pending", HostValue::Bool(pending)).unwrap(),
        |app| {
            app.set("preview-color", HostValue::Color(Color(0xff638880)))
                .unwrap();
            app.set("preview-readout", HostValue::String("#FF638880".into()))
                .unwrap();
        },
        |app| {
            app.set("marker-color", HostValue::Color(Color(0xff638880)))
                .unwrap();
            app.set("color-readout", HostValue::String("#FF638880".into()))
                .unwrap();
            app.set("pending", HostValue::Bool(false)).unwrap();
        },
    );
    Ok(())
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

fn exercise_pointer_inspection<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    let mut input = Interaction::default();
    let mut observers = xrack_layout::HoverObservers::default();
    let sensor = ui.bounds("sensor");
    let log = ui.bounds("log");
    let hover = Point {
        x: sensor.x + sensor.width * 0.25,
        y: sensor.y + sensor.height * 0.5,
    };
    let moved = Point {
        x: sensor.x + sensor.width * 0.75,
        y: hover.y,
    };
    let outside = Point {
        x: log.x + log.width * 0.5,
        y: log.y + log.height * 0.5,
    };
    for point in [hover, moved, outside] {
        let scene = ui.scene().clone();
        input.pointer_move(&scene, ui.layout(), point);
        let events = observers.sync(&input, &scene, ui.layout(), Some(point));
        ui.update(|component| {
            for event in events {
                event.apply(component, &scene);
            }
        });
        assert_eq!(
            ui.node("pointer-state").text("text"),
            Some(if point == outside {
                "Sensor idle"
            } else {
                "Sensor hovered"
            })
        );
    }
    assert_eq!(ui.node("moved").text("text"), Some("Pointer moves 1"));
    assert_eq!(ui.node("inspected").text("text"), Some("Inspections 0"));
    assert_eq!(
        ui.node("log-hover-state").text("text"),
        Some("Pointer over log")
    );

    let scene = ui.scene().clone();
    let wheel = xrack_runtime::WheelEvent {
        delta_y: xrack_runtime::Length(60.0),
        ..Default::default()
    };
    let route = input.wheel_route(&scene, ui.layout(), wheel).unwrap();
    ui.update(|component| {
        assert_eq!(
            route.dispatch(component, &scene),
            xrack_runtime::EventResult::Reject
        );
    });
    assert_eq!(ui.node("wheel-count").text("text"), Some("Wheel events 1"));
    assert!(ui.scroll("log", Point { x: 0.0, y: 60.0 }));
    assert_eq!(ui.node("log-position").text("text"), Some("Log scrolled"));

    let scene = ui.scene().clone();
    let exits = observers.sync(&input, &scene, ui.layout(), None);
    ui.update(|component| {
        for exit in exits {
            exit.apply(component, &scene);
        }
    });
    assert_eq!(
        ui.node("log-hover-state").text("text"),
        Some("Pointer left log")
    );

    assert!(ui.context_menu("inspect"));
    assert_eq!(ui.menus()[0].rows[0].label, "Open sensor log");
    ui.menu_key(MenuKey::Down);
    ui.menu_key(MenuKey::Accept);
    assert!(ui.menus().is_empty());
    assert_eq!(
        ui.node("sensor-log-count").text("text"),
        Some("Sensor log opens 1")
    );
    assert_eq!(ui.node("inspected").text("text"), Some("Inspections 0"));
    ui.click("inspect");
    assert_eq!(ui.node("inspected").text("text"), Some("Inspections 1"));
}

#[test]
fn pointer_demo_routes_hover_wheel_and_context_menu_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_pointer_inspection(touch_ui::PointerInspectionDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_pointer_inspection(load_mlir(&module.mlir)?.create("PointerInspectionDemo")?);
    Ok(())
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

fn exercise_large_type_pickers<C: Component>(component: C, size: Size) {
    let landscape = size.width > size.height;
    let mut ui = UiDriver::headless(component, size);
    let insets = if landscape {
        [44.0, 8.0, 44.0, 20.0]
    } else {
        [0.0, 38.0, 0.0, 20.0]
    };
    let safe = Rect::new(
        insets[0],
        insets[1],
        size.width - insets[0] - insets[2],
        size.height - insets[1] - insets[3],
    );
    let scaled = &mut ScaledText(2.6);
    let normal = &mut ScaledText(1.0);
    let mut input = Interaction::default();
    input.set_reduce_motion(true);
    let center = |rect: Rect| Point {
        x: rect.x + rect.width * 0.5,
        y: rect.y + rect.height * 0.5,
    };
    ui.click("readings-toggle");
    ui.click("choose-date");
    let (calendar, _, save) = picker_paths(&ui, "inspection-date-picker", "Save date");
    let mut scene = ui.component().render_shared();
    set_window_safe_area(Rc::make_mut(&mut scene), insets.map(f64::from));
    let original = layout_with_input(&scene, size, normal, &input);
    let large = layout_with_input(&scene, size, scaled, &input);
    let normal_calendar = original.at_path(&calendar).unwrap();
    let body = large.at_path(&calendar).unwrap();
    let save_bounds = large.at_path(&save).unwrap().bounds;
    assert!(contains(safe, body.bounds) && contains(safe, save_bounds));
    if landscape {
        assert!(body.bounds.x + body.bounds.width <= save_bounds.x);
    } else {
        assert!(body.bounds.height > normal_calendar.bounds.height);
    }
    let geometry = CalendarGeometry::for_layout(scene.at_path(&calendar).unwrap(), body);
    assert_eq!(geometry.days().count(), 42);
    let leap_day = Date::new(2024, 2, 29).unwrap();
    let action = CalendarAction::Select(leap_day);
    let cell = geometry.action_bounds(action).unwrap();
    assert!(contains(safe, cell));
    input.pointer_move(&scene, &large, center(cell));
    input.pointer_press_in_scene(&scene, &large);
    let selection = input.control_part_release(&scene, &large).unwrap();
    assert_eq!(selection.action, ControlAction::Calendar(action));
    ui.update(|app| assert!(app.control_at(&scene, &selection.path, selection.action)));
    assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-28"));
    ui.click_at_path(&save);
    assert_eq!(ui.node("choose-date").text("text"), Some("2024-02-29"));

    ui.click("choose-time");
    let (mut minute, _, save) = picker_paths(&ui, "inspection-time-picker", "Save time");
    minute.push(1);
    let mut scene = ui.component().render_shared();
    set_window_safe_area(Rc::make_mut(&mut scene), insets.map(f64::from));
    let original = layout_with_input(&scene, size, normal, &input);
    let large = layout_with_input(&scene, size, scaled, &input);
    let wheel = large.at_path(&minute).unwrap().tumbler.as_ref().unwrap();
    let normal_wheel = original.at_path(&minute).unwrap().tumbler.as_ref().unwrap();
    assert!(wheel.center.height > normal_wheel.center.height);
    assert_eq!(wheel.selected, Some(30));
    assert!(contains(safe, wheel.center));
    assert!(contains(safe, large.at_path(&save).unwrap().bounds));
    if landscape {
        assert!(wheel.rows.len() < normal_wheel.rows.len());
    }

    let target = input.accessibility_target(&scene, &large, &minute).unwrap();
    input.focus_target(&scene, &large, target);
    let page = input
        .tumbler_key(&scene, &large, NavigationKey::PageDown)
        .unwrap();
    assert!(matches!(
        page.action,
        ControlAction::PickTumbler { index, .. } if index == 30 + wheel.rows.len()
    ));

    let row = wheel.center;
    let start = Point {
        x: center(row).x,
        y: row.y + row.height * 0.875,
    };
    let end = Point {
        y: start.y - 0.75 * row.height,
        ..start
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &large, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &large, end, now + Duration::from_millis(300)));
    let selection = input
        .end_scroll_gesture(&scene, &large, end, now + Duration::from_millis(400))
        .control
        .unwrap();
    assert!(matches!(
        selection.action,
        ControlAction::PickTumbler { index: 31, .. }
    ));
    ui.update(|app| assert!(app.control_at(&scene, &selection.path, selection.action)));
    assert_eq!(ui.node("choose-time").text("text"), Some("12:30:41"));
    ui.click_at_path(&save);
    assert_eq!(ui.node("choose-time").text("text"), Some("12:31:41"));
}

#[test]
fn large_type_pickers_preserve_touch_and_keyboard_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let interpreter = load_mlir(&module.mlir)?;
    for size in [Size::new(375.0, 667.0), Size::new(760.0, 358.0)] {
        exercise_large_type_pickers(TouchDemo::new(), size);
        exercise_large_type_pickers(interpreter.create("TouchDemo")?, size);
    }
    Ok(())
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

fn exercise_pointer_styles<C: Component>(component: C) {
    use xrack_runtime::{CursorShape, Orientation, PointerButton, PointerEvent};

    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    let mut input = Interaction::default();
    let mut hover = xrack_layout::HoverObservers::default();
    let point_for = |ui: &UiDriver<C>, path: &[usize]| {
        let item = ui.layout_at(path).unwrap();
        item.map_to_window(Point {
            x: item.bounds.x + item.bounds.width * 0.5,
            y: item.bounds.y + item.bounds.height * 0.5,
        })
    };
    for (id, orientation) in [
        ("columns", Orientation::Horizontal),
        ("rows", Orientation::Vertical),
    ] {
        let mut path = ui.path(id);
        path.push(
            ui.node(id)
                .children
                .iter()
                .position(|child| child.kind == "SplitHandle")
                .unwrap(),
        );
        let point = point_for(&ui, &path);
        input.pointer_move(ui.scene(), ui.layout(), point);
        assert_eq!(input.split_orientation(ui.scene()), Some(orientation));
        input.pointer_press_in_scene(ui.scene(), ui.layout());
        input.split_press(ui.scene(), ui.layout());
        let end = match orientation {
            Orientation::Horizontal => Point {
                x: point.x + 28.0,
                y: point.y,
            },
            Orientation::Vertical => Point {
                x: point.x,
                y: point.y + 20.0,
            },
        };
        input.pointer_move(ui.scene(), ui.layout(), end);
        let drag = input.split_drag(ui.scene(), ui.layout()).unwrap();
        let scene = ui.scene().clone();
        ui.update(|component| {
            assert!(component.resize_split_at(&scene, &drag.path, &drag.weights))
        });
        assert!(input.finish_split_resize());
        let moved = point_for(&ui, &path);
        match orientation {
            Orientation::Horizontal => assert!(moved.x > point.x),
            Orientation::Vertical => assert!(moved.y > point.y),
        }
    }

    let path = ui.path("name-field");
    input.pointer_move(ui.scene(), ui.layout(), point_for(&ui, &path));
    let target = input.hovered.as_ref().unwrap();
    assert_eq!(target.path, path);
    assert!(input.is_text_editor(ui.node("name-field"), target));

    let point = point_for(&ui, &ui.path("canvas"));
    input.pointer_move(ui.scene(), ui.layout(), point);
    hover.sync(&input, ui.scene(), ui.layout(), Some(point));
    assert_eq!(hover.cursor_shape(), CursorShape::Crosshair);

    let point = point_for(&ui, &ui.path("clear-name"));
    input.pointer_move(ui.scene(), ui.layout(), point);
    hover.sync(&input, ui.scene(), ui.layout(), Some(point));
    assert_eq!(hover.cursor_shape(), CursorShape::Auto);
    for (is_touch, label) in [(false, "Input Mouse"), (true, "Input Touch")] {
        let point = point_for(&ui, &ui.path("input-probe"));
        let (sequence, down) = input
            .begin_pointer_sequence(
                ui.scene(),
                ui.layout(),
                point,
                PointerEvent {
                    button: PointerButton::Primary,
                    is_touch,
                    touch_finger_id: if is_touch { 1 } else { 0 },
                    ..Default::default()
                },
                Instant::now(),
            )
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|component| assert!(down.apply(component, &scene)));
        assert_eq!(ui.node("input-source").text("text"), Some(label));
        ui.update(|component| {
            assert!(sequence.cancel(Default::default()).apply(component, &scene));
        });
    }

    ui.click("name-field");
    ui.edit_focused(EditAction::Home(false));
    ui.edit_focused(EditAction::End(true));
    let mut focus = xrack_layout::FocusObservers::default();
    focus.sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert_eq!(
        ui.node("selection").text("text"),
        Some("Selected Pump inspection")
    );
    ui.click("clear-name");
    assert_eq!(text_input_value(ui.node("name-field")), "");
}

#[test]
fn pointer_style_regions_match_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_pointer_styles(native_actions_ui::PointerStyleDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_pointer_styles(load_mlir(&module.mlir)?.create("PointerStyleDemo")?);
    Ok(())
}

fn exercise_keyboard_selection<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
    let mut focus = xrack_layout::FocusObservers::default();
    for (id, name, selection, changed) in [
        ("line", "Line", "[1, 5)", "AXB"),
        ("notes", "Notes", "[7, 11)", "First\nAXB"),
    ] {
        ui.click(&format!("focus-{id}"));
        ui.edit_focused(EditAction::End(false));
        ui.edit_focused(EditAction::Left(false));
        ui.edit_focused(EditAction::Left(true));
        focus.sync(ui.input(), ui.scene(), ui.layout(), true);
        ui.update(|_| {});
        let selection_text = format!("{name} selection: 🧪 {selection}");
        let edit_count = format!("{name} edits: 0");
        assert_eq!(
            ui.node(&format!("{id}-selection")).text("text"),
            Some(selection_text.as_str())
        );
        assert_eq!(
            ui.node(&format!("{id}-edit-count")).text("text"),
            Some(edit_count.as_str())
        );
        assert!(ui.edit_focused(EditAction::Insert("X")));
        assert_eq!(text_input_value(ui.node(id)), changed);
    }
}

#[test]
fn keyboard_selection_updates_bindings_without_editing_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_keyboard_selection(native_actions_ui::NativeSelectionDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_keyboard_selection(load_mlir(&module.mlir)?.create("NativeSelectionDemo")?);
    Ok(())
}

fn exercise_system_direction<C: Component>(mut component: C) {
    use xrack_runtime::{LayoutDirection, RangeAction, Value};

    let first_action = [0, 1, 0];
    let second_action = [0, 1, 1];
    let batch_number = [0, 3, 0];
    let check_digit = [0, 3, 1];
    let slider = [0, 4];
    let original = component.render_shared();
    assert_eq!(original.layout_direction(), None);
    let mut fixed = original.clone();
    Rc::make_mut(&mut fixed).properties.insert(
        "layout-direction",
        Value::LayoutDirection(LayoutDirection::LeftToRight),
    );
    let prepared =
        xrack_layout::apply_system_layout_direction(fixed.clone(), LayoutDirection::RightToLeft);
    assert!(Rc::ptr_eq(&prepared, &fixed));

    assert!(component.dispatch_at(&original, &first_action, "clicked"));
    for (direction, action, value) in [
        (LayoutDirection::LeftToRight, RangeAction::Increase, 30.0),
        (LayoutDirection::RightToLeft, RangeAction::Decrease, 25.0),
    ] {
        let base = component.render_shared();
        let scene = xrack_layout::apply_system_layout_direction(base.clone(), direction);
        assert_eq!(base.layout_direction(), None);
        let mut input = Interaction::default();
        let layout = layout_with_input(&scene, Size::new(390.0, 380.0), &mut FixedText, &input);
        let x = |path: &[usize]| layout.at_path(path).unwrap().bounds.x;
        assert_eq!(
            x(&first_action) > x(&second_action),
            direction == LayoutDirection::RightToLeft
        );
        assert!(x(&batch_number) < x(&check_digit));
        assert_eq!(
            scene.find("status").unwrap().text("text"),
            Some("Selected: Review")
        );
        let target = input
            .accessibility_target(&scene, &layout, &slider)
            .unwrap();
        input.focus_target(&scene, &layout, target);
        let adjustment = input
            .slider_key(&scene, &layout, NavigationKey::Right, true)
            .unwrap();
        assert_eq!(adjustment.action, action);
        assert!(component.adjust_range_at(&scene, &adjustment.path, adjustment.action));
        assert_eq!(
            component
                .render()
                .find("progress-slider")
                .unwrap()
                .number("value"),
            Some(value)
        );
    }
}

#[test]
fn system_direction_mirrors_inherited_controls_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_system_direction(native_actions_ui::SystemDirectionDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_system_direction(load_mlir(&module.mlir)?.create("SystemDirectionDemo")?);
    Ok(())
}

fn exercise_emoji_editing<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 460.0));
    assert_eq!(ui.node("compass").text("text"), Some("🧭"));
    assert_eq!(ui.node("technician").text("text"), Some("👩🏽‍💻"));
    assert_eq!(ui.node("flag").text("text"), Some("🇨🇳"));
    ui.click("sample");
    ui.edit_focused(EditAction::End(false));
    ui.edit_focused(EditAction::Left(false));
    ui.edit_focused(EditAction::Left(true));
    assert!(ui.edit_focused(EditAction::Insert("👩🏽‍💻🇨🇳")));
    assert_eq!(text_input_value(ui.node("sample")), "A👩🏽‍💻🇨🇳B");
    ui.edit_focused(EditAction::Left(true));
    assert!(ui.edit_focused(EditAction::Insert("X")));
    assert_eq!(text_input_value(ui.node("sample")), "A👩🏽‍💻XB");
    ui.edit_focused(EditAction::Left(false));
    ui.edit_focused(EditAction::Left(true));
    assert!(ui.edit_focused(EditAction::Insert("🧪")));
    assert_eq!(text_input_value(ui.node("sample")), "A🧪XB");
}

#[test]
fn emoji_sequences_edit_as_graphemes_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_emoji_editing(native_actions_ui::EmojiDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_emoji_editing(load_mlir(&module.mlir)?.create("EmojiDemo")?);
    Ok(())
}

fn exercise_marked_text<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
    let mut input = Interaction::default();
    let mut focus = xrack_layout::FocusObservers::default();
    for (id, name, start, original, committed) in [
        ("line", "Line", 1, "A🧪B", "A試験B"),
        ("notes", "Notes", 7, "First\nA🧪B", "First\nA試験B"),
    ] {
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &ui.path(id))
            .unwrap();
        input.focus_target(ui.scene(), ui.layout(), target);
        input.sync_editor(ui.scene());
        input.apply_text_selection(
            ui.scene(),
            TextCommand::SetSelection {
                anchor: start,
                focus: start + 4,
            },
        );
        focus.sync(&input, ui.scene(), ui.layout(), true);
        ui.update(|_| {});
        let copy_state = format!("{id}-copy-state");
        let available = format!("{name} copy available");
        assert_eq!(ui.node(&copy_state).text("text"), Some(available.as_str()));

        assert!(input.begin_text_composition(ui.scene(), 1));
        assert!(input.update_text_composition(ui.scene(), 1, "shi".into()));
        assert!(input.update_text_composition(ui.scene(), 1, "試験".into()));
        focus.sync(&input, ui.scene(), ui.layout(), true);
        ui.update(|_| {});
        let blocked = format!("{name} copy blocked");
        let count = format!("{id}-edit-count");
        let no_edits = format!("{name} edits: 0");
        assert_eq!(ui.node(&copy_state).text("text"), Some(blocked.as_str()));
        assert_eq!(ui.node(&count).text("text"), Some(no_edits.as_str()));
        assert_eq!(text_input_value(ui.node(id)), original);

        assert_eq!(input.finish_text_composition(ui.scene(), 1), (true, true));
        let scene = ui.scene().clone();
        ui.update(|component| {
            assert!(input.edit_text(
                component,
                &scene,
                EditAction::Insert("試験"),
                xrack_layout::EditSource::Ime,
                Instant::now(),
            ));
        });
        let one_edit = format!("{name} edits: 1");
        assert_eq!(ui.node(&count).text("text"), Some(one_edit.as_str()));
        assert_eq!(text_input_value(ui.node(id)), committed);
    }
}

#[test]
fn marked_text_keeps_original_values_until_confirmed_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_marked_text(native_actions_ui::NativeSelectionDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_marked_text(load_mlir(&module.mlir)?.create("NativeSelectionDemo")?);
    Ok(())
}

fn exercise_model_replacement_during_composition<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
    let mut input = Interaction::default();
    for (id, name, expected) in [
        ("line", "Line", "Updated line"),
        ("notes", "Notes", "Updated notes"),
    ] {
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &ui.path(id))
            .unwrap();
        input.focus_target(ui.scene(), ui.layout(), target);
        input.sync_editor(ui.scene());
        assert!(input.begin_text_composition(ui.scene(), 1));
        assert!(input.update_text_composition(ui.scene(), 1, "stale".into()));

        ui.click(&format!("replace-{id}"));
        input.sync_editor(ui.scene());
        assert!(input.editor.as_ref().unwrap().preedit.is_empty());
        assert_eq!(input.finish_text_composition(ui.scene(), 1), (false, false));
        assert_eq!(text_input_value(ui.node(id)), expected);
        let no_edits = format!("{name} edits: 0");
        assert_eq!(
            ui.node(&format!("{id}-edit-count")).text("text"),
            Some(no_edits.as_str())
        );
    }
}

#[test]
fn replacing_model_text_invalidates_composition_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_model_replacement_during_composition(native_actions_ui::NativeSelectionDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_model_replacement_during_composition(
        load_mlir(&module.mlir)?.create("NativeSelectionDemo")?,
    );
    Ok(())
}

fn exercise_clipboard_authorization_demo<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 320.0));
    let mut input = Interaction::default();
    let mut focus = xrack_layout::FocusObservers::default();
    let requests = xrack_runtime::FocusRequests::default();
    let click = |ui: &mut UiDriver<C>, id| {
        let scene = ui.scene().clone();
        let path = ui.path(id);
        ui.update(|component| {
            let _scope = requests.enter();
            assert!(component.dispatch_at(&scene, &path, "clicked"));
        });
        requests.drain()
    };

    let selection = click(&mut ui, "select-emoji");
    input.apply_focus_requests(ui.scene(), ui.layout(), selection);
    focus.sync(&input, ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert_eq!(ui.node("selection").text("text"), Some("Selected 🧪"));

    let only_command = |ui: &mut UiDriver<C>, input: &mut Interaction, id| {
        let mut requests = click(ui, id).into_iter();
        let command = input.apply_focus_request(ui.scene(), ui.layout(), requests.next().unwrap());
        assert!(requests.next().is_none());
        command
    };
    assert_eq!(
        only_command(&mut ui, &mut input, "paste"),
        Some(TextCommand::Paste)
    );
    let context =
        xrack_layout::TextEditContext::for_clipboard(&input, ui.scene(), TextCommand::Paste)
            .expect("the paste target should be the selected emoji");
    assert_eq!(context.selection(), "🧪");
    assert!(context.is_current(&input, ui.scene()));

    let scene = ui.scene().clone();
    ui.update(|component| {
        assert!(input.edit_text(
            component,
            &scene,
            EditAction::Insert("clipboard sample"),
            xrack_layout::EditSource::Atomic,
            Instant::now(),
        ));
    });
    assert_eq!(
        text_input_value(ui.node("draft-field")),
        "Aclipboard sampleB"
    );
    assert_eq!(ui.node("edit-count").text("text"), Some("Edits 1"));
    focus.sync(&input, ui.scene(), ui.layout(), true);
    ui.update(|_| {});

    assert_eq!(
        only_command(&mut ui, &mut input, "undo"),
        Some(TextCommand::Undo)
    );
    let scene = ui.scene().clone();
    ui.update(|component| assert!(input.replay_text_edit(component, &scene, false)));
    focus.sync(&input, ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert_eq!(text_input_value(ui.node("draft-field")), "A🧪B");
    assert_eq!(ui.node("selection").text("text"), Some("Selected 🧪"));
}

#[test]
fn cross_app_paste_demo_keeps_the_same_selection_and_history_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_clipboard_authorization_demo(native_actions_ui::ClipboardAuthorizationDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_clipboard_authorization_demo(
        load_mlir(&module.mlir)?.create("ClipboardAuthorizationDemo")?,
    );
    Ok(())
}

fn exercise_keyboard_options<C: Component>(component: C) {
    use xrack_runtime::KeyboardCapitalization;

    let mut ui = UiDriver::headless(component, Size::new(390.0, 520.0));
    let title = ui.node("draft-title");
    assert_eq!(
        title.keyboard_capitalization(),
        KeyboardCapitalization::Words
    );
    assert!(title.boolean("auto-correct", false));
    assert!(title.boolean("spell-check", false));
    assert!(title.keyboard_navigation());
    ui.click("draft-title");
    ui.fill_focused("Ring 7");
    ui.click("toggle-keyboard");
    let title = ui.node("draft-title");
    assert_eq!(
        title.keyboard_capitalization(),
        KeyboardCapitalization::Characters
    );
    assert!(!title.boolean("auto-correct", true));
    assert!(!title.boolean("spell-check", true));
    assert_eq!(title.text("text"), Some("Ring 7"));
    let note = ui.node("review-note");
    assert_eq!(note.keyboard_capitalization(), KeyboardCapitalization::None);
    assert!(note.boolean("spell-check", false));
    assert!(note.keyboard_navigation());
    assert_eq!(note.return_key(), xrack_runtime::ReturnKey::Auto);
    let code = ui.node("draft-secret");
    assert!(!code.keyboard_navigation());
    assert_eq!(code.return_key(), xrack_runtime::ReturnKey::Done);
    ui.click("review-note");
    assert_eq!(
        ui.input()
            .adjacent_text_fields(ui.scene(), ui.layout())
            .map(|target| target.map(|target| target.path)),
        [Some(ui.path("draft-title")), Some(ui.path("draft-secret"))],
    );
    let path = ui.path("review-note");
    for (draft, valid) in [("x", false), ("x\ny", true)] {
        ui.fill_focused(draft);
        ui.update(|component| {
            let scene = component.render_shared();
            assert_eq!(
                component.control_at(&scene, &path, ControlAction::ValidateTextSubmission),
                valid,
            );
        });
    }
}

#[test]
fn keyboard_assistance_bindings_match_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_keyboard_options(native_actions_ui::KeyboardOptionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_keyboard_options(load_mlir(&module.mlir)?.create("KeyboardOptionsDemo")?);
    Ok(())
}

fn exercise_long_note_caret<C: Component>(component: C) {
    let size = Size::new(390.0, 286.0);
    let mut ui = UiDriver::headless(component, size);
    ui.click("long-note");
    let draft = (1..=17)
        .map(|step| format!("Inspection step {step}"))
        .collect::<Vec<_>>()
        .join("\n");
    ui.fill_focused(&draft);
    assert_eq!(ui.node("long-note").text("text"), Some(draft.as_str()));

    let scene = ui.scene();
    let path = ui.path("long-note");
    let viewport = ui.path("note-viewport");
    let mut input = Interaction::default();
    input.reconcile(scene, ui.layout());
    let target = input
        .accessibility_target(scene, ui.layout(), &path)
        .unwrap();
    input.focus_target(scene, ui.layout(), target);
    let frame = layout_with_input(scene, size, &mut FixedText, &input);
    let caret = |frame: &xrack_layout::LayoutNode, y| {
        let bounds = frame.at_path(&path).unwrap().bounds;
        Rect::new(bounds.x + 8.0, bounds.y + y, 1.0, 24.0)
    };
    assert!(input.reveal_focused_editor_caret(scene, &frame, caret(&frame, 404.0)));
    assert!(input.scroll_offsets()[&viewport] > 100.0);
    let revealed = layout_with_input(scene, size, &mut FixedText, &input);
    assert_eq!(
        revealed
            .at_path(&path)
            .unwrap()
            .clip_rect(caret(&revealed, 404.0)),
        caret(&revealed, 404.0),
    );
    assert!(!input.reveal_focused_editor_caret(scene, &revealed, caret(&revealed, 404.0)));
    assert!(input.reveal_focused_editor_caret(scene, &revealed, caret(&revealed, 8.0)));
    assert!(input.scroll_offsets()[&viewport] <= 8.0);
    assert!(input.is_focused(&path));
}

#[test]
fn a_long_note_scrolls_to_the_caret_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_long_note_caret(native_actions_ui::LongNoteDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_long_note_caret(load_mlir(&module.mlir)?.create("LongNoteDemo")?);
    Ok(())
}

fn exercise_floating_note_caret<C: Component>(component: C) {
    let size = Size::new(744.0, 1133.0);
    let ui = UiDriver::headless(component, size);
    let scene = ui.scene();
    let path = ui.path("long-note");
    let viewport = ui.path("note-viewport");
    let mut input = Interaction::default();
    let original = layout_with_input(scene, size, &mut FixedText, &input);
    let target = input.accessibility_target(scene, &original, &path).unwrap();
    input.focus_target(scene, &original, target.clone());
    let offsets = input.scroll_offsets().clone();
    let note = original.at_path(&path).unwrap().bounds;
    let form = original.at_path(&viewport).unwrap().bounds;
    assert!(note.height > form.height);
    let caret_at = |frame: &xrack_layout::LayoutNode, x, y| {
        let bounds = frame.at_path(&path).unwrap().bounds;
        Rect::new(bounds.x + x, bounds.y + y, 1.0, 24.0)
    };
    let caret = caret_at(&original, 8.0, 60.0);
    let keyboard = Rect::new(caret.x - 8.0, caret.y - 8.0, 160.0, 112.0);
    assert!(!input.avoid_floating_keyboard(scene, &original, Some(keyboard), None));
    assert!(input.avoid_floating_keyboard(scene, &original, Some(keyboard), Some(caret)));

    let shifted = layout_with_input(scene, size, &mut FixedText, &input);
    let shifted_note = shifted.at_path(&path).unwrap();
    let shifted_caret = caret_at(&shifted, 8.0, 60.0);
    assert!(shifted_note.bounds.intersect(keyboard).height > 0.0);
    assert_eq!(shifted_caret.intersect(keyboard).height, 0.0);
    assert_eq!(
        shifted_note.visible_window_bounds(shifted_caret),
        shifted_caret
    );
    assert_eq!(shifted.at_path(&viewport).unwrap().bounds, form);
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert_eq!(input.scroll_offsets(), &offsets);
    assert!(!input.avoid_floating_keyboard(scene, &shifted, Some(keyboard), Some(shifted_caret)));

    let outside_keyboard = caret_at(&shifted, 200.0, 60.0);
    assert!(input.avoid_floating_keyboard(scene, &shifted, Some(keyboard), Some(outside_keyboard)));
    let restored = layout_with_input(scene, size, &mut FixedText, &input);
    assert_eq!(restored.at_path(&path).unwrap().bounds, note);

    let clipped_caret = caret_at(&restored, 8.0, 280.0);
    let keyboard = Rect::new(clipped_caret.x - 8.0, clipped_caret.y - 40.0, 160.0, 112.0);
    assert!(!input.avoid_floating_keyboard(scene, &restored, Some(keyboard), Some(clipped_caret)));
    assert_eq!(input.scroll_offsets(), &offsets);
}

#[test]
fn floating_keyboard_avoids_the_visible_multiline_caret_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_floating_note_caret(native_actions_ui::LongNoteDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_floating_note_caret(load_mlir(&module.mlir)?.create("LongNoteDemo")?);
    Ok(())
}

fn exercise_accessible_navigation<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 660.0));
    let finish_mount = |ui: &mut UiDriver<C>| {
        for _ in 0..8 {
            if ui.node("tool-state").text("text") == Some("Tool ready") {
                return;
            }
            assert!(ui.finish_loader_frame());
        }
        panic!("the tool did not mount");
    };
    finish_mount(&mut ui);
    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(
        ui.node("latest-reading").text("text"),
        Some("Pressure recorded")
    );
    ui.click("load-flow");
    finish_mount(&mut ui);
    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(
        ui.node("latest-reading").text("text"),
        Some("Flow recorded")
    );
    ui.click("unload-tool");
    assert_eq!(ui.node("tool-state").text("text"), Some("Tool null"));

    let go_back = |ui: &mut UiDriver<C>| {
        let scene = ui.scene().clone();
        let back = ui
            .input()
            .stack_back_in_window(&scene, ui.layout())
            .unwrap();
        ui.update(|app| {
            app.control_at(&scene, &back.path, back.action);
        });
    };
    ui.click("open-detail");
    ui.click("lock-back");
    go_back(&mut ui);
    assert_eq!(ui.node("page-state").text("text"), Some("Page detail"));
    assert_eq!(
        ui.node("back-status").text("text"),
        Some("Blocked returns 1")
    );
    ui.click("open-dialog");
    assert!(ui.input().dialog_escape(ui.scene(), ui.layout()).is_none());
    assert!(
        ui.input()
            .stack_back_in_window(ui.scene(), ui.layout())
            .is_none()
    );
    ui.click("dismiss-overlay");
    ui.click("lock-back");
    ui.click("open-dialog");
    let scene = ui.scene().clone();
    let dismiss = ui.input().dialog_escape(&scene, ui.layout()).unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &dismiss.path, dismiss.action)));
    assert_eq!(ui.node("page-state").text("text"), Some("Page detail"));
    go_back(&mut ui);
    assert_eq!(ui.node("page-state").text("text"), Some("Page summary"));
    assert!(
        ui.input()
            .stack_back_in_window(ui.scene(), ui.layout())
            .is_none()
    );
}

#[test]
fn accessible_navigation_and_loader_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_accessible_navigation(native_actions_ui::NativeNavigationDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_accessible_navigation(load_mlir(&module.mlir)?.create("NativeNavigationDemo")?);
    Ok(())
}

fn exercise_accessible_numeric_controls<C: Component>(component: C) {
    use xrack_runtime::{IndicatorState, RangeAction, Time, indicator_state, spin_text};

    let mut ui = UiDriver::headless(component, Size::new(402.0, 760.0));
    assert_eq!(
        indicator_state(ui.node("transfer")),
        Some(IndicatorState::Determinate(0.35))
    );
    let mut lower = ui.path("band");
    lower.push(0);
    for path in [ui.path("gain"), lower.clone()] {
        let scene = ui.component().render_shared();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &path, RangeAction::Increase)));
    }
    assert_eq!(ui.node("gain").number("value"), Some(1.0));
    assert_eq!(
        indicator_state(ui.node("transfer")),
        Some(IndicatorState::Indeterminate)
    );
    assert_eq!(ui.node("band").number("lower-value"), Some(6.0));
    let scene = ui.component().render_shared();
    let gain = ui.path("gain");
    ui.update(|app| {
        assert!(!app.adjust_range_at(&scene, &lower, RangeAction::Increase));
        assert!(app.adjust_range_at(&scene, &gain, RangeAction::Increase));
    });
    assert_eq!(ui.node("band").number("upper-value"), Some(6.0));
    assert_eq!(
        indicator_state(ui.node("transfer")),
        Some(IndicatorState::Determinate(1.0))
    );

    let offset = ui.path("offset");
    let fixed = ui.path("fixed-count");
    let scene = ui.component().render_shared();
    let increment = ControlAction::Spin(SpinAction::Increase);
    ui.update(|app| {
        assert!(app.control_at(&scene, &offset, increment));
        assert!(!app.control_at(&scene, &fixed, increment));
    });
    assert_eq!(spin_text(ui.node("offset")), "1,00");
    assert_eq!(spin_text(ui.node("fixed-count")), "7");

    let mut minute = ui.path("clock");
    minute.push(1);
    let scene = ui.component().render_shared();
    let wheel = Tumbler::from_node(scene.at_path(&minute).unwrap());
    let action = wheel.step(1).and_then(|index| wheel.select(index)).unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &minute, action)));
    assert_eq!(
        ui.node("clock").time("time"),
        Some(Time {
            hour: 14,
            minute: 0,
            second: 0
        })
    );
}

#[test]
fn adjustable_value_changes_match_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_accessible_numeric_controls(native_actions_ui::AdjustableValuesDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_accessible_numeric_controls(load_mlir(&module.mlir)?.create("AdjustableValuesDemo")?);
    Ok(())
}

fn exercise_drawer_preview<C: Component>(component: C) {
    let size = Size::new(402.0, 874.0);
    let mut ui = UiDriver::headless(component, size);
    let path = ui.path("tools-drawer");
    let mut input = Interaction::default();
    let mut scene = ui.component().render_shared();
    let insets = [0.0, 52.0, 0.0, 34.0];
    set_window_safe_area(Rc::make_mut(&mut scene), insets);
    let layout = layout_with_input(&scene, size, &mut FixedText, &input);
    let start = Point { x: 200.0, y: 834.0 };
    let short = Point {
        y: start.y - 28.0,
        ..start
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    let far = Point {
        y: start.y - 90.0,
        ..start
    };
    assert!(input.move_scroll_gesture(&scene, &layout, far, now + Duration::from_millis(70)));
    assert!(input.move_scroll_gesture(&scene, &layout, short, now + Duration::from_millis(140)));

    let (node, viewport, reveal) = input.opening_drawer_preview(&scene, &layout).unwrap();
    assert_eq!(node.element_id(), Some("tools-drawer"));
    assert_eq!(viewport.y + viewport.height, 840.0);
    assert!((reveal * 214.0 - 28.0).abs() < 0.01);
    assert!(!node.boolean("open", false) && node.children.is_empty());
    assert!(layout.dialog_paths().is_empty());
    let (_, shell, offset) =
        xrack_layout::layout_drawer_opening(node, viewport, reveal, &mut FixedText);
    assert!((shell.bounds.y + offset.y - 812.0).abs() < 0.01);
    assert!(shell.children.is_empty());
    let release =
        input.end_scroll_gesture(&scene, &layout, short, now + Duration::from_millis(180));
    assert!(release.scrolled && !release.tap && release.control.is_none());
    assert!(input.drawer_preview().is_none());

    let time = now + Duration::from_millis(300);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, time, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &layout, far, time + Duration::from_millis(140)));
    let rotated = layout_with_input(&scene, Size::new(874.0, 402.0), &mut FixedText, &input);
    let stale = input.end_scroll_gesture(&scene, &rotated, far, time + Duration::from_millis(180));
    assert!(stale.control.is_none());
    assert!(input.opening_drawer_preview(&scene, &rotated).is_none());

    let time = now + Duration::from_secs(1);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, time, ScrollGestureKind::Touch));
    let release = input.end_scroll_gesture(
        &scene,
        &layout,
        Point { y: 710.0, ..start },
        time + Duration::from_millis(150),
    );
    let opened = release.control.unwrap();
    assert_eq!(opened.action, ControlAction::Drawer(true));
    ui.update(|app| assert!(app.control_at(&scene, &opened.path, opened.action)));

    let mut scene = ui.component().render_shared();
    set_window_safe_area(Rc::make_mut(&mut scene), insets);
    let layout = layout_with_input(&scene, size, &mut FixedText, &input);
    input.reconcile(&scene, &layout);
    let panel = layout.at_path(&path).unwrap().bounds;
    let child_path = ui.path("run-tool");
    let child = layout.at_path(&child_path).unwrap().bounds;
    let start = Point {
        x: 200.0,
        y: panel.y + 5.0,
    };
    let short = Point {
        y: start.y + 28.0,
        ..start
    };
    let time = now + Duration::from_secs(2);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, time, ScrollGestureKind::Touch));
    let focus = input.focused.clone();
    assert!(focus.is_some());
    let far = Point {
        y: start.y + 214.0,
        ..start
    };
    assert!(input.move_scroll_gesture(&scene, &layout, far, time + Duration::from_millis(60)));
    let offscreen = layout_with_input(&scene, size, &mut FixedText, &input);
    input.reconcile(&scene, &offscreen);
    assert_eq!(input.focused, focus);
    assert!(input.move_scroll_gesture(&scene, &layout, short, time + Duration::from_millis(100)));
    let shifted = layout_with_input(&scene, size, &mut FixedText, &input);
    assert!((shifted.at_path(&path).unwrap().bounds.y - panel.y - 28.0).abs() < 0.01);
    assert!((shifted.at_path(&child_path).unwrap().bounds.y - child.y - 28.0).abs() < 0.01);
    assert_eq!(shifted.dialog_paths(), [path.clone()]);
    let release =
        input.end_scroll_gesture(&scene, &shifted, short, time + Duration::from_millis(150));
    assert!(release.scrolled && !release.tap && release.control.is_none());
    let restored = layout_with_input(&scene, size, &mut FixedText, &input);
    assert_eq!(restored.at_path(&path).unwrap().bounds, panel);
    assert_eq!(restored.at_path(&child_path).unwrap().bounds, child);
    input.reconcile(&scene, &restored);
    assert_eq!(input.focused, focus);

    let time = now + Duration::from_secs(3);
    assert!(input.begin_scroll_gesture(&scene, &restored, start, time, ScrollGestureKind::Touch));
    let release = input.end_scroll_gesture(
        &scene,
        &restored,
        Point {
            y: start.y + 110.0,
            ..start
        },
        time + Duration::from_millis(150),
    );
    let closed = release.control.unwrap();
    assert_eq!(closed.action, ControlAction::Drawer(false));
    ui.update(|app| assert!(app.control_at(&scene, &closed.path, closed.action)));
    assert!(ui.node("tools-drawer").children.is_empty());
    assert!(!ui.node("tools-drawer").boolean("open", false));
}

#[test]
fn drawer_previews_and_commits_match_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_drawer_preview(native_actions_ui::NativeActionsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_drawer_preview(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

fn exercise_hardware_commands<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 430.0));
    ui.click("commands");
    assert!(ui.menu_shortcut("F2".parse().unwrap()));
    assert_eq!(ui.node("refresh-count").text("text"), Some("Refreshes 1"));
    assert!(ui.keyboard_context_menu());
    assert!(ui.click_menu_row("Open runbook"));
    assert_eq!(ui.node("runbook-count").text("text"), Some("Runbooks 1"));
    ui.click("field");
    assert!(ui.menu_shortcut("F2".parse().unwrap()));
    assert!(ui.menu_shortcut("Primary+F12".parse().unwrap()));
    assert_eq!(ui.node("refresh-count").text("text"), Some("Refreshes 2"));
    assert_eq!(ui.node("snapshot-count").text("text"), Some("Snapshots 1"));
    assert_eq!(ui.node("field").text("text"), Some("Draft"));
}

#[test]
fn hardware_commands_match_in_both_backends() -> Result<(), Box<dyn std::error::Error>> {
    exercise_hardware_commands(native_actions_ui::HardwareCommandsDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_hardware_commands(load_mlir(&module.mlir)?.create("HardwareCommandsDemo")?);
    Ok(())
}

fn exercise_touch_tooltip<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 500.0));
    let bounds = ui.bounds("inspect");
    let point = Point {
        x: bounds.x + 20.0,
        y: bounds.y + 20.0,
    };
    let now = Instant::now();
    let mut input = Interaction::default();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        point,
        now,
        ScrollGestureKind::Touch
    ));
    let (candidate, deadline) = input
        .touch_tooltip_at(ui.scene(), ui.layout(), point, now)
        .unwrap();
    assert_eq!(deadline - now, Duration::from_millis(650));
    assert!(input.show_touch_tooltip(ui.scene(), ui.layout(), candidate, point, deadline));
    input.cancel_scroll_gesture();
    assert!(
        !input
            .end_scroll_gesture(ui.scene(), ui.layout(), point, deadline)
            .tap
    );
    assert_eq!(ui.node("count").text("text"), Some("Inspections 0"));
    let popup = input.tooltip_popup(ui.scene(), ui.layout()).unwrap();
    assert_eq!(popup.node.text("text"), Some("Capture the current reading"));
    assert!(!input.update_tooltip(ui.scene(), ui.layout(), deadline));
    let expiry = deadline + Duration::from_secs(5);
    assert_eq!(input.tooltip_deadline(), Some(expiry));
    assert!(input.update_tooltip(ui.scene(), ui.layout(), expiry));
    assert!(input.tooltip_popup(ui.scene(), ui.layout()).is_none());

    ui.click("inspect");
    assert_eq!(ui.node("count").text("text"), Some("Inspections 1"));
    let (candidate, _) = input
        .touch_tooltip_at(ui.scene(), ui.layout(), point, expiry)
        .unwrap();
    assert!(ui.scroll("view", Point { x: 0.0, y: 500.0 }));
    assert!(!input.show_touch_tooltip(ui.scene(), ui.layout(), candidate, point, expiry));
}

#[test]
fn touch_tooltips_use_the_same_content_and_tap_routing_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_touch_tooltip(native_actions_ui::TouchTooltipDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_touch_tooltip(load_mlir(&module.mlir)?.create("TouchTooltipDemo")?);
    Ok(())
}

fn exercise_text_drag<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 410.0));
    for (action, id, expected) in [
        ("select-sample", "sample", Some("🧪")),
        ("select-notes", "notes", Some("Pump\nCheck")),
        ("select-locked", "locked", Some("Locked")),
        ("select-protected", "protected", None),
    ] {
        ui.click(action);
        let copy =
            xrack_layout::TextEditContext::for_clipboard(ui.input(), ui.scene(), TextCommand::Copy);
        assert_eq!(
            copy.as_ref().map(xrack_layout::TextEditContext::selection),
            expected
        );
        let bounds = ui.bounds(id);
        let point = Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        };
        let target = ui.input().file_drop_scope(ui.layout()).text_drag_source(
            ui.scene(),
            ui.layout(),
            point,
        );
        assert_eq!(
            target.map(|target| target.path),
            expected.map(|_| ui.path(id))
        );
    }
}

#[test]
fn text_drag_uses_the_same_copyable_selection_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_text_drag(native_actions_ui::TextDragDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_text_drag(load_mlir(&module.mlir)?.create("TextDragDemo")?);
    Ok(())
}

fn exercise_text_drop<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(440.0, 400.0));
    let mut input = Interaction::default();
    let point_for = |ui: &UiDriver<C>, id| {
        let rect = ui.bounds(id);
        Point {
            x: rect.x + 4.0,
            y: rect.y + rect.height * 0.5,
        }
    };
    let scope = input.file_drop_scope(ui.layout());
    for id in ["read-only", "disabled"] {
        assert!(
            scope
                .text_target(ui.scene(), ui.layout(), point_for(&ui, id))
                .is_none()
        );
    }
    let point = point_for(&ui, "draft");
    let target = scope.text_target(ui.scene(), ui.layout(), point).unwrap();
    assert_eq!(target.path, ui.path("draft"));
    assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target.clone()));
    input.sync_editor(ui.scene());

    let pending =
        xrack_layout::TextDrop::capture(&input, ui.scene(), ui.layout(), point, target.clone())
            .unwrap();
    assert!(
        pending
            .destination(&input, ui.scene(), ui.layout(), &scope)
            .is_some()
    );
    input.apply_text_selection(ui.scene(), TextCommand::SelectAll);
    assert!(
        pending
            .destination(&input, ui.scene(), ui.layout(), &scope)
            .is_none()
    );

    let pending =
        xrack_layout::TextDrop::capture(&input, ui.scene(), ui.layout(), point, target).unwrap();
    assert!(
        pending
            .destination(&input, ui.scene(), ui.layout(), &scope)
            .is_some()
    );
    // The native test resolves this initial caret from UIKit coordinates and the real font.
    input
        .editor
        .as_mut()
        .unwrap()
        .set_selection("A🧪B", 0, Default::default());
    let scene = ui.scene().clone();
    ui.update(|component| {
        assert!(input.edit_text(
            component,
            &scene,
            EditAction::Insert("巡检🧭"),
            xrack_layout::EditSource::Atomic,
            Instant::now(),
        ));
    });
    assert_eq!(text_input_value(ui.node("draft")), "巡检🧭A🧪B");
    assert_eq!(ui.node("edit-count").text("text"), Some("Drop edits 1"));
    let scene = ui.scene().clone();
    ui.update(|component| assert!(input.replay_text_edit(component, &scene, false)));
    assert_eq!(text_input_value(ui.node("draft")), "A🧪B");
    assert_eq!(ui.node("edit-count").text("text"), Some("Drop edits 2"));

    let point = point_for(&ui, "notes");
    let scope = input.file_drop_scope(ui.layout());
    let target = scope.text_target(ui.scene(), ui.layout(), point).unwrap();
    let pending =
        xrack_layout::TextDrop::capture(&input, ui.scene(), ui.layout(), point, target.clone())
            .unwrap();
    assert!(
        pending
            .destination(&input, ui.scene(), ui.layout(), &scope)
            .is_some()
    );
    assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target));
    assert!(
        pending
            .destination(&input, ui.scene(), ui.layout(), &scope)
            .is_none()
    );
    let scene = ui.scene().clone();
    ui.update(|component| {
        assert!(input.edit_text(
            component,
            &scene,
            EditAction::Insert("First\nSecond 🧭"),
            xrack_layout::EditSource::Atomic,
            Instant::now(),
        ));
    });
    assert_eq!(text_input_value(ui.node("notes")), "First\nSecond 🧭");
    assert_eq!(ui.node("edit-count").text("text"), Some("Drop edits 3"));
}

#[test]
fn text_drop_respects_editor_state_and_undo_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_text_drop(native_actions_ui::TextDropDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_text_drop(load_mlir(&module.mlir)?.create("TextDropDemo")?);
    Ok(())
}

fn exercise_text_history<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
    let mut input = Interaction::default();
    for (id, original, changed) in [
        ("line", "A🧪B", "A🧪B!"),
        ("notes", "First\nA🧪B", "First\nA🧪B!"),
    ] {
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &ui.path(id))
            .unwrap();
        input.focus_target(ui.scene(), ui.layout(), target.clone());
        input.sync_editor(ui.scene());
        let available = |ui: &UiDriver<C>, input: &Interaction| {
            let node = ui.node(id);
            let value = input.editor_value(node);
            let commands =
                input.text_command_availability(ui.scene(), &target, node, &value, false);
            (commands.undo, commands.redo)
        };
        assert_eq!(available(&ui, &input), (false, false));

        let scene = ui.scene().clone();
        ui.update(|component| {
            assert!(input.edit_text(
                component,
                &scene,
                EditAction::Insert("!"),
                xrack_layout::EditSource::Atomic,
                Instant::now(),
            ));
        });
        assert_eq!(text_input_value(ui.node(id)), changed);
        assert_eq!(available(&ui, &input), (true, false));

        input.apply_text_selection(ui.scene(), TextCommand::SelectAll);
        for command in [TextCommand::Copy, TextCommand::Cut] {
            let context = xrack_layout::TextEditContext::for_clipboard(&input, ui.scene(), command)
                .expect("selected text should be available to the clipboard");
            assert_eq!(context.selection(), changed);
        }

        for (redo, text, commands) in [
            (false, original, (false, true)),
            (true, changed, (true, false)),
        ] {
            let scene = ui.scene().clone();
            ui.update(|component| assert!(input.replay_text_edit(component, &scene, redo)));
            assert_eq!(available(&ui, &input), commands);
            assert_eq!(text_input_value(ui.node(id)), text);
        }
        assert!(input.begin_text_composition(ui.scene(), 1));
        assert!(input.update_text_composition(ui.scene(), 1, "candidate".into()));
        assert_eq!(available(&ui, &input), (false, false));
        let scene = ui.scene().clone();
        ui.update(|component| assert!(!input.replay_text_edit(component, &scene, false)));
        assert!(input.cancel_text_composition(Some(1)));
        assert_eq!(available(&ui, &input), (true, false));
    }
}

#[test]
fn native_editor_undo_redo_uses_the_same_history_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_text_history(native_actions_ui::NativeSelectionDemo::new());
    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise_text_history(load_mlir(&module.mlir)?.create("NativeSelectionDemo")?);
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

        let native = input.take_native_text_menu().unwrap();
        assert!(!input.menu_open());
        let copy = native
            .entries()
            .find(|(_, label)| *label == "Copy")
            .unwrap()
            .0;
        assert_eq!(
            native.entries().any(|(_, label)| label == "Cut"),
            !read_only
        );
        assert_eq!(
            native.entries().any(|(_, label)| label == "Paste"),
            !read_only
        );
        if read_only {
            input.apply_text_selection(ui.scene(), TextCommand::ClearSelection);
            assert!(
                input
                    .native_text_menu_input(native, ui.scene(), ui.layout(), copy)
                    .is_none()
            );
        } else {
            let action = input
                .native_text_menu_input(native, ui.scene(), ui.layout(), copy)
                .unwrap();
            assert_eq!(action.action, ControlAction::TextCommand(TextCommand::Copy));
        }
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
        let [previous_id, next_id] = match mode {
            xrack_runtime::InputType::Email => [None, Some("contact-tel")],
            xrack_runtime::InputType::Tel => [Some("contact-email"), Some("contact-url")],
            _ => [Some("contact-tel"), None],
        };
        let return_key = if next_id.is_some() {
            xrack_runtime::ReturnKey::Next
        } else {
            xrack_runtime::ReturnKey::Done
        };
        assert_eq!(ui.node(id).return_key(), return_key);
        assert_eq!(
            ui.input()
                .adjacent_text_fields(ui.scene(), ui.layout())
                .map(|target| target.map(|target| target.path)),
            [previous_id, next_id].map(|id| id.map(|id| ui.path(id))),
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

    assert!(
        scene
            .at_path(&viewport)
            .unwrap()
            .boolean("dismiss-keyboard-on-drag", false)
    );
    let start = Point {
        x: field.bounds.x + field.bounds.width / 2.0,
        y: field.bounds.y + field.bounds.height / 2.0,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &visible, start, now, ScrollGestureKind::Touch));
    let jitter = Point {
        y: start.y - 3.0,
        ..start
    };
    input.move_scroll_gesture(&scene, &visible, jitter, now + Duration::from_millis(40));
    assert!(!input.take_scroll_keyboard_dismissal());
    let dragged = Point {
        y: start.y - 45.0,
        ..start
    };
    input.move_scroll_gesture(&scene, &visible, dragged, now + Duration::from_millis(100));
    assert!(input.take_scroll_keyboard_dismissal());
    assert_eq!(input.focused.as_ref(), Some(&target));
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

    assert_eq!(
        input.focus_adjacent_text_field(&scene, &frame, true),
        Some(true)
    );
    let frame = layout_with_input(&scene, landscape, &mut FixedText, &input);
    input.reconcile(&scene, &frame);
    assert!(input.is_focused(&telephone));
    assert_visible(&frame, &telephone);

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
    assert!(input.avoid_floating_keyboard(scene, &unshifted, Some(keyboard), None));
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
    assert!(!input.avoid_floating_keyboard(scene, &shifted, Some(keyboard), None));

    let away = Rect {
        y: 700.0,
        ..keyboard
    };
    assert!(input.avoid_floating_keyboard(scene, &shifted, Some(away), None));
    let restored = layout_with_input(scene, size, &mut FixedText, &input);
    assert_eq!(restored.at_path(&path).unwrap().bounds, original);
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert_eq!(scene.at_path(&path).unwrap().text("text"), Some("73"));

    let high_keyboard = Rect {
        y: 20.0,
        ..keyboard
    };
    assert!(input.avoid_floating_keyboard(scene, &restored, Some(high_keyboard), None));
    let shifted = layout_with_input(scene, size, &mut FixedText, &input);
    let editor = shifted.at_path(&path).unwrap();
    assert!(editor.bounds.y >= high_keyboard.y + high_keyboard.height);
    assert_eq!(editor.visible_window_bounds(editor.bounds), editor.bounds);
    assert!(input.avoid_floating_keyboard(scene, &shifted, None, None));
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

fn exercise_photo_preview<C: Component>(
    component: C,
    requests: &Cell<usize>,
    saved: &Cell<Option<i64>>,
    batches: &Cell<usize>,
    shares: &Cell<usize>,
    update: impl Fn(&mut C, bool, i64, &str, Option<Vec<touch_ui::InspectionPhoto>>),
) {
    use xrack_runtime::{Image, ImageFormat};
    let photos = [[96, 64], [64, 96]]
        .into_iter()
        .enumerate()
        .map(|(index, size)| touch_ui::InspectionPhoto {
            preview: Image::Embedded {
                format: ImageFormat::Svg,
                bytes: br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96"><rect width="96" height="96" fill="green"/></svg>"#,
                size,
            },
            has_preview: true,
            details: format!("Preview {}", index + 1),
        })
        .collect::<Vec<_>>();
    let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
    ui.click("choose");
    assert_eq!(requests.get(), 1);
    ui.update(|app| update(app, true, 0, "No photos selected", None));
    assert!(!ui.can_hit("choose"));
    assert!(!ui.node("gallery").boolean("visible", true));
    ui.update(|app| update(app, false, 0, "Imported 2 photos", Some(photos)));
    assert!(ui.can_hit("choose"));
    assert_eq!(
        ui.node("import-status").text("text"),
        Some("Imported 2 photos")
    );
    assert_eq!(ui.node("photo-position").text("text"), Some("Photo 1 of 2"));
    let mut first = ui.path("gallery");
    first.extend([0, 0, 0]);
    assert_eq!(
        ui.scene()
            .at_path(&first)
            .unwrap()
            .image("source")
            .unwrap()
            .size(),
        [96, 64]
    );

    ui.update(|app| update(app, false, 1, "Imported 2 photos", None));
    assert_eq!(ui.node("photo-position").text("text"), Some("Photo 2 of 2"));
    let mut second = ui.path("gallery");
    second.extend([1, 0, 0]);
    assert_eq!(
        ui.scene()
            .at_path(&second)
            .unwrap()
            .image("source")
            .unwrap()
            .size(),
        [64, 96]
    );
    ui.click("save");
    assert_eq!(saved.get(), Some(1));
    ui.click("save-all");
    assert_eq!(batches.get(), 1);
    ui.click("share-originals");
    assert_eq!(shares.get(), 1);
    ui.update(|app| update(app, true, 1, "Imported 2 photos", None));
    assert!(!ui.can_hit("share-originals"));
    ui.update(|app| update(app, false, 1, "Share canceled", None));
    assert!(ui.can_hit("share-originals"));
    assert_eq!(ui.node("photo-position").text("text"), Some("Photo 2 of 2"));
    ui.update(|app| update(app, false, 1, "Photo selection canceled", None));
    assert_eq!(ui.node("photo-position").text("text"), Some("Photo 2 of 2"));
    let (_, landscape) = xrack_layout::prepare_scene_with_input(
        ui.component(),
        ui.component().render_shared(),
        Size::new(700.0, 330.0),
        &mut FixedText,
        &Interaction::default(),
    );
    let save = landscape.at_path(&ui.path("save")).unwrap();
    let visible = save.visible_window_bounds(save.bounds);
    assert!((visible.width - save.bounds.width).abs() < 0.01);
    assert!((visible.height - save.bounds.height).abs() < 0.01);
    assert!(
        landscape
            .at_path(&ui.path("gallery"))
            .unwrap()
            .bounds
            .height
            >= 64.0
    );
}

#[test]
fn photo_picker_demo_projects_host_state_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    let calls = Rc::new(Cell::new(0));
    let saved = Rc::new(Cell::new(None));
    let batches = Rc::new(Cell::new(0));
    let shares = Rc::new(Cell::new(0));
    let mut app = touch_ui::PhotoPickerDemo::new();
    app.on_choose_photos({
        let calls = calls.clone();
        move || calls.set(calls.get() + 1)
    });
    app.on_save_original({
        let saved = saved.clone();
        move |index| saved.set(Some(index))
    });
    app.on_save_all({
        let batches = batches.clone();
        move || batches.set(batches.get() + 1)
    });
    app.on_share_originals({
        let shares = shares.clone();
        move || shares.set(shares.get() + 1)
    });
    exercise_photo_preview(
        app,
        &calls,
        &saved,
        &batches,
        &shares,
        |app, pending, index, status, photos| {
            app.set_pending(pending);
            app.set_photo_status(status.into());
            if let Some(photos) = photos {
                app.set_photos(photos);
            }
            app.set_current_photo(index);
        },
    );

    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    let app = load_mlir(&module.mlir)?.create("PhotoPickerDemo")?;
    calls.set(0);
    saved.set(None);
    batches.set(0);
    shares.set(0);
    app.on("choose-photos", {
        let calls = calls.clone();
        Rc::new(move |_| {
            calls.set(calls.get() + 1);
            Ok(None)
        })
    })?;
    app.on("save-original", {
        let saved = saved.clone();
        Rc::new(move |args| {
            if let [HostValue::Int(index)] = args {
                saved.set(Some(*index));
            }
            Ok(None)
        })
    })?;
    app.on("save-all", {
        let batches = batches.clone();
        Rc::new(move |_| {
            batches.set(batches.get() + 1);
            Ok(None)
        })
    })?;
    app.on("share-originals", {
        let shares = shares.clone();
        Rc::new(move |_| {
            shares.set(shares.get() + 1);
            Ok(None)
        })
    })?;
    exercise_photo_preview(
        app,
        &calls,
        &saved,
        &batches,
        &shares,
        |app, pending, index, status, photos| {
            use xrack_runtime::{RecordList, RecordValue};
            if let Some(photos) = photos {
                let records = photos
                    .into_iter()
                    .map(|photo| {
                        RecordValue::new(
                            "InspectionPhoto",
                            [
                                ("preview".to_owned(), photo.preview.into()),
                                ("has-preview".to_owned(), photo.has_preview.into()),
                                ("details".to_owned(), photo.details.into()),
                            ]
                            .into(),
                        )
                    })
                    .collect();
                app.set(
                    "photos",
                    HostValue::RecordList(RecordList::new("InspectionPhoto", records)),
                )
                .unwrap();
            }
            app.set("pending", pending.into()).unwrap();
            app.set("current-photo", index.into()).unwrap();
            app.set("photo-status", status.to_owned().into()).unwrap();
        },
    );
    Ok(())
}

fn exercise_accessible_list_scroll<C: Component>(app: C) {
    let size = Size::new(390.0, 540.0);
    let mut input = Interaction::default();
    let mut text = FixedText;
    let base = app.render_shared();
    let prepare = |input: &Interaction, text: &mut FixedText| {
        xrack_layout::prepare_scene_with_input(&app, base.clone(), size, text, input)
    };
    let (initial, layout) = prepare(&input, &mut text);
    let queue = [0, 2];
    let first = [0, 2, 0];
    let first_row = initial.row_key_at(&first).unwrap();
    let list = layout.at_path(&queue).unwrap();
    let distance = list
        .scroll_on(xrack_runtime::Orientation::Vertical)
        .unwrap()
        .max_offset;
    assert!(input.accessibility_scroll_to(
        &layout,
        &queue,
        xrack_runtime::Orientation::Vertical,
        distance
    ));
    let (scrolled, layout) = prepare(&input, &mut text);
    assert_eq!(scrolled.at_path(&queue).unwrap().list.unwrap().first_row, 2);
    assert_ne!(scrolled.row_key_at(&first), Some(first_row));
    assert_eq!(
        scrolled.find("order-status").unwrap().text("text"),
        Some("Moves: 0 · Selected row: 1")
    );
    assert!(input.focused.is_none());
    assert!(input.accessibility_scroll_to(
        &layout,
        &queue,
        xrack_runtime::Orientation::Vertical,
        0.0
    ));
    let (restored, _) = prepare(&input, &mut text);
    assert_eq!(restored.row_key_at(&first), Some(first_row));
}

#[test]
fn virtual_list_accessibility_scroll_preserves_selection_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_accessible_list_scroll(touch_ui::ListReorderDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_accessible_list_scroll(load_mlir(&module.mlir)?.create("ListReorderDemo")?);
    Ok(())
}

fn exercise_list_reorder<C: Component>(app: C) {
    let size = Size::new(390.0, 540.0);
    let mut ui = UiDriver::headless(app, size);
    let queue = ui.path("check-queue");
    let mark = |index| {
        let mut path = queue.clone();
        path.extend([index, 0, 0, 0, 0]);
        path
    };
    let title = |ui: &UiDriver<C>, index| {
        ui.scene()
            .at_path(&mark(index))
            .unwrap()
            .text("text")
            .unwrap()
            .to_owned()
    };
    assert_eq!(title(&ui, 0), "Pump pressure");
    ui.click_at_path(&mark(0));
    assert_eq!(title(&ui, 0), "Pump pressure · marked");
    let row = ui.scene().row_key_at(&mark(0)).unwrap();
    let point = |index| {
        let bounds = ui.layout_at(&mark(index)).unwrap().bounds;
        Point {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        }
    };
    let (start, destination) = (point(0), point(2));
    let now = Instant::now();
    let scene = ui.scene().clone();
    let mut input = Interaction::default();
    assert!(input.begin_scroll_gesture(&scene, ui.layout(), start, now, ScrollGestureKind::Touch));
    input.arm_touch_drag(&scene, ui.layout(), start, now);
    let swipe = Point {
        y: start.y - 56.0,
        ..start
    };
    let early = now + Duration::from_millis(90);
    assert!(
        !input
            .drag_motion_at(&scene, ui.layout(), swipe, None, early)
            .captured
    );
    assert!(input.move_scroll_gesture(&scene, ui.layout(), swipe, early));
    let late = now + Duration::from_millis(460);
    assert!(
        !input
            .drag_motion_at(&scene, ui.layout(), destination, None, late)
            .captured
    );

    let mut input = Interaction::default();
    input.arm_touch_drag(&scene, ui.layout(), start, now);
    let captured = input.drag_motion_at(&scene, ui.layout(), destination, None, late);
    assert!(captured.captured);
    ui.update(|component| {
        for event in captured.events {
            event.apply(component, &scene);
        }
    });
    let scene = ui.scene().clone();
    let release = input.finish_drag(&scene, ui.layout(), destination, None);
    assert!(release.captured);
    ui.update(|component| {
        for event in release.events {
            event.apply(component, &scene);
        }
    });
    assert_eq!(title(&ui, 0), "Door interlock");
    assert_eq!(title(&ui, 1), "Water temperature");
    assert_eq!(title(&ui, 2), "Pump pressure · marked");
    assert_eq!(ui.scene().row_key_at(&mark(2)), Some(row));
    assert_eq!(
        ui.node("order-status").text("text"),
        Some("Moves: 1 · Selected row: 3")
    );
    let mut up = queue.clone();
    up.extend([2, 0, 0, 1]);
    ui.click_at_path(&up);
    assert_eq!(title(&ui, 1), "Pump pressure · marked");
    assert_eq!(
        ui.node("order-status").text("text"),
        Some("Moves: 2 · Selected row: 2")
    );
}

#[test]
fn touch_list_reordering_keeps_model_identity_and_delegate_state_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_list_reorder(touch_ui::ListReorderDemo::new());
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_list_reorder(load_mlir(&module.mlir)?.create("ListReorderDemo")?);
    Ok(())
}

fn exercise_edge_drag<C: Component>(app: C, previews: impl Fn(&C) -> i64) {
    use xrack_layout::prepare_scene_with_input;
    use xrack_runtime::DragAction;

    let size = Size::new(390.0, 540.0);
    let mut ui = UiDriver::headless(app, size);
    let queue = ui.path("check-queue");
    let marker = |slot| {
        let mut path = queue.clone();
        path.extend([slot, 0, 0, 0, 0]);
        path
    };
    let first = marker(0);
    ui.click_at_path(&first);
    assert!(ui.node("check-queue").row_locator.is_some());
    let bounds = ui.layout_at(&first).unwrap().bounds;
    let viewport = ui.layout_at(&queue).unwrap().bounds;
    let start = Point {
        x: bounds.x + 30.0,
        y: bounds.y + bounds.height / 2.0,
    };
    let edge = Point {
        y: viewport.y + viewport.height - 15.0,
        ..start
    };
    let mut scene = Rc::new(ui.scene().clone());
    let old_row = scene.row_key_at(&first).unwrap();
    let mut layout = ui.layout().clone();
    let mut input = Interaction::default();
    let now = Instant::now();
    input.arm_touch_drag(&scene, &layout, start, now);
    let captured = input.drag_motion_at(
        &scene,
        &layout,
        edge,
        None,
        now + Duration::from_millis(450),
    );
    assert!(captured.captured);
    ui.update(|component| {
        for event in captured.events {
            event.apply(component, &scene);
        }
    });
    assert_eq!(
        ui.scene()
            .at_path(&first[..first.len() - 1])
            .unwrap()
            .float("opacity"),
        Some(0.6)
    );
    assert_eq!(previews(ui.component()), 1);

    let hovered_row = |scene: &xrack_runtime::Node, input: &Interaction| {
        scene
            .at_path(&queue)?
            .materialized_children()
            .find_map(|(slot, row)| {
                let target = row.child_at(0)?;
                let mut path = queue.clone();
                path.extend([slot, 0]);
                input
                    .is_drop_target(target, &path)
                    .then_some((row.row_key?, slot))
            })
    };
    let mut shifted_while_hovered = false;

    for ms in (480..900).step_by(20) {
        let old = hovered_row(&scene, &input).unwrap();
        if !input.advance_drag_scroll(&scene, &layout, now + Duration::from_millis(ms)) {
            break;
        }
        (scene, layout) = prepare_scene_with_input(
            ui.component(),
            ui.component().render_shared(),
            size,
            &mut FixedText,
            &input,
        );
        let update = input.reconcile_drag(&scene, &layout, None);
        assert!(
            update
                .events
                .iter()
                .all(|event| !matches!(event.event, DragEvent::Finished(_)))
        );
        assert_eq!(input.drag_action(), Some(DragAction::Move));
        let current = hovered_row(&scene, &input).unwrap();
        if old.0 == current.0 && old.1 != current.1 {
            shifted_while_hovered = true;
            assert!(update.events.iter().all(|event| {
                !matches!(event.event, DragEvent::Entered { .. } | DragEvent::Exited)
            }));
        }
        ui.update(|component| {
            for event in update.events {
                event.apply(component, &scene);
            }
        });
        assert_eq!(previews(ui.component()), 1);
    }
    assert!(shifted_while_hovered);
    let list = scene.at_path(&queue).unwrap().list.unwrap();
    assert!(list.first_row > 0);
    assert_ne!(scene.row_key_at(&first), Some(old_row));
    assert!(input.drag_scroll_deadline(&scene, &layout).is_none());
    let release = input.finish_drag(&scene, &layout, edge, None);
    assert!(release.captured);
    let mut committed = false;
    let mut finished = false;
    ui.update(|component| {
        for event in release.events {
            let dropped = matches!(event.event, DragEvent::Dropped { .. });
            let cleanup = matches!(event.event, DragEvent::Finished(DragAction::Move));
            let handled = event.apply(component, &scene);
            committed |= dropped && handled;
            finished |= cleanup && handled;
        }
    });
    assert!(committed && finished);
    assert_eq!(previews(ui.component()), 0);
    assert_eq!(
        ui.node("order-status").text("text"),
        Some("Moves: 1 · Selected row: 7")
    );
    let (scene, _) = prepare_scene_with_input(
        ui.component(),
        ui.component().render_shared(),
        size,
        &mut FixedText,
        &input,
    );
    let last = marker(list.row_count - 1 - list.first_row);
    assert_eq!(scene.row_key_at(&last), Some(old_row));
    assert_eq!(
        scene.at_path(&last).unwrap().text("text"),
        Some("Pump pressure · marked")
    );
    assert_eq!(
        scene
            .at_path(&last[..last.len() - 1])
            .unwrap()
            .float("opacity"),
        Some(1.0)
    );
    assert!(input.drag_scroll_deadline(&scene, &layout).is_none());
}

#[test]
fn edge_drag_scrolls_to_virtual_rows_and_finishes_the_original_source()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_edge_drag(touch_ui::ListReorderDemo::new(), |app| {
        app.get_active_previews()
    });
    let module = xrack_compiler::compile(include_str!("touch.xr"), "touch.xr")?;
    exercise_edge_drag(
        load_mlir(&module.mlir)?.create("ListReorderDemo")?,
        |app| match app.get("active-previews") {
            Ok(HostValue::Int(value)) => value,
            result => panic!("Expected an integer preview count, got {result:?}"),
        },
    );
    Ok(())
}

fn exercise_native_action_fixture<C: Component>(app: C) {
    use xrack_layout::{NavigationKey, tooltip_text};
    use xrack_runtime::Orientation;

    let mut ui = UiDriver::headless(app, Size::new(390.0, 720.0));
    assert_eq!(
        tooltip_text(ui.node("review-trigger")),
        Some("Record or browse an inspection review")
    );
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Record review"));
    assert_eq!(ui.node("review-count").text("text"), Some("Reviews 1"));
    assert_eq!(
        tooltip_text(ui.node("review-trigger")),
        Some("Choose how to follow up on the review")
    );
    let clear = ui.node("reset-sort");
    assert_eq!(
        tooltip_text(clear),
        Some("Return to the original row order")
    );
    assert_eq!(
        clear.text("accessible-description"),
        Some("Restore the original instrument order")
    );

    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Read service log"));
    for id in ["log-heading", "log-parts"] {
        assert_eq!(ui.node(id).int("accessible-heading-level"), Some(2));
    }
    assert_eq!(ui.node("log-morning").int("accessible-heading-level"), None);
    let visible = |ui: &UiDriver<C>, id: &str| {
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
    assert!(ui.scroll("log-viewport", Point { x: 0.0, y: 170.0 }));
    assert!(ui.scroll_to_top());
    assert!(visible(&ui, "log-heading"));
    assert!(visible(&ui, "log-night"));
    assert!(!visible(&ui, "log-morning"));
    assert!(!visible(&ui, "log-parts"));
    assert!(!ui.scroll_to_top());
    ui.escape();

    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Report page controls"));
    let markers = ui.path("report-markers");
    assert_eq!(
        xrack_runtime::page_indicator_step(ui.node("report-markers"), false),
        None
    );
    for (forward, index, status) in [
        (true, 1, "Page changes: 1"),
        (true, 2, "Page changes: 2"),
        (false, 1, "Page changes: 3"),
    ] {
        assert_eq!(
            xrack_runtime::page_indicator_step(ui.node("report-markers"), forward),
            Some(index)
        );
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &markers, ControlAction::Select(index))));
        assert_eq!(
            xrack_runtime::group_index(ui.node("report-cards")),
            Some(index)
        );
        assert_eq!(ui.node("report-page-status").text("text"), Some(status));
        if index == 2 {
            assert_eq!(
                xrack_runtime::page_indicator_step(ui.node("report-markers"), true),
                None
            );
        }
    }
    ui.click("report-page-lock");
    assert_eq!(
        xrack_runtime::page_indicator_step(ui.node("report-markers"), false),
        None
    );
    let scene = ui.scene().clone();
    ui.update(|app| assert!(!app.control_at(&scene, &markers, ControlAction::Select(0))));
    assert_eq!(xrack_runtime::group_index(ui.node("report-cards")), Some(1));
    assert_eq!(
        ui.node("report-page-status").text("text"),
        Some("Page changes: 3")
    );
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

    let cell = [row(0).as_slice(), &[0, 2]].concat();
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

    let mut reorder = Interaction::default();
    let scene = ui.scene().clone();
    let draft = [strip.as_slice(), &[1]].concat();
    let target = reorder
        .accessibility_target(&scene, ui.layout(), &draft)
        .unwrap();
    let action = reorder
        .tab_move_target(&scene, ui.layout(), &target, NavigationKey::Right)
        .flatten()
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert_eq!(
        ui.node("overflow-status").text("text"),
        Some("Moved tab 2 to 3")
    );
    let moved = [strip.as_slice(), &[2]].concat();
    assert_eq!(ui.scene().row_key_at(&moved), target.row_key);
    let bar = ui.node("overflow-tabs");
    assert_eq!(xrack_runtime::group_index(bar), Some(0));
    assert!(reorder.reveal_tab(bar, ui.layout_at(&strip).unwrap(), &strip, 2));
    let revealed = layout_with_input(
        ui.scene(),
        Size::new(390.0, 720.0),
        &mut FixedText,
        &reorder,
    );
    assert!(
        reorder
            .accessibility_target(ui.scene(), &revealed, &moved)
            .is_some()
    );

    ui.escape();
    assert!(ui.context_menu("review-trigger"));
    assert!(ui.click_menu_row("Disclosure controls"));
    let details = ui.path("details");
    let optional = ui.path("optional");
    let scene = ui.scene().clone();
    assert!(ui.find_path("network-details").is_err());
    assert_eq!(
        ui.node("checks-status").properties.get("accessible-live"),
        Some(&xrack_runtime::Value::AccessibleLive(
            xrack_runtime::AccessibleLive::Off
        )),
    );
    ui.update(|app| assert!(app.control_at(&scene, &details, ControlAction::Expander(true))));
    ui.click("network-details");
    assert_eq!(ui.node("checks-status").text("text"), Some("Checks 1"));
    assert_eq!(
        ui.node("checks-status").properties.get("accessible-live"),
        Some(&xrack_runtime::Value::AccessibleLive(
            xrack_runtime::AccessibleLive::Polite
        )),
    );
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
fn accessible_table_column_moves_preserve_the_declared_sort_column_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
        let path = ui.path("instrument-table");
        let input = Interaction::default();
        let scene = ui.scene().clone();
        let target = input
            .accessibility_target(&scene, ui.layout(), &path)
            .unwrap();
        for (direction, destination, status, order) in [
            (NavigationKey::Right, 2, "Moved column 1 to 3", [2, 0]),
            (NavigationKey::Left, 1, "Moved column 1 to 2", [0, 2]),
        ] {
            let scene = ui.scene().clone();
            let action = input
                .table_column_header_move(&scene, ui.layout(), &target, 0, direction)
                .unwrap();
            let ControlAction::MoveColumn { to, .. } = action.action else {
                panic!("expected a table column move");
            };
            assert_eq!(to, destination);
            ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
            assert_eq!(ui.node("row-movement").text("text"), Some(status));
            let geometry = ui.layout_at(&path).unwrap().table.as_ref().unwrap();
            assert_eq!(geometry.visible_columns().collect::<Vec<_>>(), order);
            let scene = ui.scene().clone();
            ui.update(|app| assert!(app.control_at(&scene, &path, ControlAction::SortColumn(0))));
            let sort = ui
                .node("instrument-table")
                .table_header
                .as_ref()
                .unwrap()
                .sort;
            assert_eq!(sort.map(|sort| sort.column), Some(0));
        }
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::NativeActionsDemo::new());
    exercise(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

#[test]
fn accessible_table_resizes_obey_limits_and_follow_the_row_key_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
        let input = Interaction::default();
        let path = ui.path("instrument-table");
        let table = input
            .accessibility_target(ui.scene(), ui.layout(), &path)
            .unwrap();
        assert!(
            input
                .table_column_header_resize(ui.scene(), ui.layout(), &table, 0, true)
                .is_none()
        );
        ui.click("resize-headers");

        for (increase, expected) in [(true, 124.0), (false, 116.0), (false, 112.0)] {
            let scene = ui.scene().clone();
            let action = input
                .table_column_header_resize(&scene, ui.layout(), &table, 0, increase)
                .unwrap();
            ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
            assert_eq!(
                ui.layout_at(&path).unwrap().table.as_ref().unwrap().columns[0].width,
                expected
            );
        }
        for column in 0..3 {
            assert!(
                input
                    .table_column_header_resize(ui.scene(), ui.layout(), &table, column, false)
                    .is_none()
            );
        }

        let row = [path.as_slice(), &[1]].concat();
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &row)
            .unwrap();
        for (increase, expected) in [
            (true, 51.0),
            (false, 43.0),
            (false, 35.0),
            (false, 27.0),
            (false, 19.0),
            (false, 12.0),
        ] {
            let scene = ui.scene().clone();
            let action = input
                .table_row_header_resize(&scene, ui.layout(), &target, increase)
                .unwrap();
            ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
            assert_eq!(ui.layout_at(&row).unwrap().bounds.height, expected);
        }
        assert!(
            input
                .table_row_header_resize(ui.scene(), ui.layout(), &target, false)
                .is_none()
        );
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &path, ControlAction::SortColumn(0))));
        let moved = [path.as_slice(), &[2]].concat();
        assert_eq!(ui.scene().row_key_at(&moved), target.row_key);
        assert_eq!(ui.layout_at(&moved).unwrap().bounds.height, 12.0);
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::NativeActionsDemo::new());
    exercise(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
    Ok(())
}

#[test]
fn transparent_buttons_keep_input_and_disabled_state_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
        assert!(ui.can_hit("plain-action"));
        assert!(!ui.can_hit("unavailable-action"));
        ui.click("plain-action");
        assert_eq!(ui.node("shape-status").text("text"), Some("Actions 1"));
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::ButtonShapesDemo::new());
    exercise(load_mlir(&module.mlir)?.create("ButtonShapesDemo")?);
    Ok(())
}

#[test]
fn menu_paging_preserves_the_scope_and_command_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 540.0));
        let mut input = Interaction::default();
        let scene = ui.scene().clone();
        let layout = ui.layout();
        let trigger = input
            .accessibility_target(&scene, layout, &ui.path("review-commands"))
            .unwrap();
        input.accessibility_focus(trigger.clone());
        assert!(input.open_context_menu(&scene, layout, true));
        let trigger = input.menu_trigger().unwrap().clone();
        let (path, session) = {
            let panel = &input.menu_panels(&scene, layout)[0];
            assert!(!panel.can_scroll(true));
            assert!(panel.can_scroll(false));
            (panel.path().to_vec(), panel.session())
        };
        assert!(!input.accessibility_menu_page(
            &scene,
            layout,
            &trigger,
            &path,
            session + 1,
            false
        ));
        assert!(input.accessibility_menu_page(&scene, layout, &trigger, &path, session, false));
        let panels = input.menu_panels(&scene, layout);
        let root = &panels[0];
        assert!(root.visible_range().start > 0);
        assert!(!root.can_scroll(false));
        assert_eq!(ui.node("menu-paging-status").text("text"), Some("Ready"));
        assert!(input.accessibility_menu_page(&scene, layout, &trigger, &path, session, true));
        let (parent_range, submenu) = {
            let panel = &input.menu_panels(&scene, layout)[0];
            assert_eq!(panel.visible_range().start, 0);
            assert_eq!(
                panel.highlighted,
                Some(panel.row_index(panel.rows.len() - 1))
            );
            (panel.visible_range(), panel.entry(0))
        };
        assert!(
            input
                .accessibility_menu_entry(&scene, layout, &trigger, &submenu, true)
                .is_none()
        );
        assert!(input.accessibility_menu_page(
            &scene,
            layout,
            &trigger,
            &submenu.path,
            session,
            false,
        ));
        let command = {
            let panels = input.menu_panels(&scene, layout);
            assert_eq!(panels[0].visible_range(), parent_range);
            let panel = &panels[1];
            assert!(!panel.can_scroll(false));
            let command = panel.entry(panel.rows.len() - 1);
            assert_eq!(command.label, "Apply archive filter");
            command
        };
        let action = input
            .accessibility_menu_entry(&scene, layout, &trigger, &command, true)
            .unwrap();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        assert_eq!(
            ui.node("menu-paging-status").text("text"),
            Some("Filter applied")
        );
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::MenuPagingDemo::new());
    exercise(load_mlir(&module.mlir)?.create("MenuPagingDemo")?);
    Ok(())
}

#[test]
fn swipe_side_actions_do_not_activate_the_buttons_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_runtime::SwipeSide;

    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 600.0));
        let list = ui.path("documents");
        let first = [list.as_slice(), &[0, 0]].concat();
        let next = [list.as_slice(), &[1, 0]].concat();
        let input = Interaction::default();
        let target = input
            .accessibility_target(ui.scene(), ui.layout(), &first)
            .unwrap();
        let next_target = input
            .accessibility_target(ui.scene(), ui.layout(), &next)
            .unwrap();
        assert!(
            input
                .swipe_delegate_accessibility_side(
                    ui.scene(),
                    ui.layout(),
                    &next_target,
                    SwipeSide::Left
                )
                .is_none()
        );
        for side in [SwipeSide::Left, SwipeSide::Right, SwipeSide::Closed] {
            let scene = ui.scene().clone();
            let action = input
                .swipe_delegate_accessibility_side(&scene, ui.layout(), &target, side)
                .unwrap();
            ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
            let row = ui.scene().at_path(&first).unwrap();
            assert_eq!(row.swipe_side(), side);
            assert_eq!(ui.node("swipe-result").text("text"), Some("Ready"));
            assert!(
                input
                    .swipe_delegate_accessibility_side(ui.scene(), ui.layout(), &target, side)
                    .is_none()
            );
            assert_eq!(ui.scene().row_key_at(&first), target.row_key);
        }
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::SwipeActionsDemo::new());
    exercise(load_mlir(&module.mlir)?.create("SwipeActionsDemo")?);
    Ok(())
}

#[test]
fn scaled_group_headers_keep_their_controls_reachable_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::new(component, ScaledText(2.6), Size::new(390.0, 700.0));
        for id in ["inspection-options", "network-settings"] {
            let panel = ui.layout_at(&ui.path(id)).unwrap();
            assert_eq!(panel.group_title().unwrap().1.len(), 2);
            assert!(panel.group_header(ui.node(id)).height > 110.0);
        }
        assert!(ui.node("inspection-options").expander_collapsed());
        ui.click("inspection-options");
        assert!(!ui.node("inspection-options").expander_collapsed());
        let header = ui
            .layout_at(&ui.path("inspection-options"))
            .unwrap()
            .group_header(ui.node("inspection-options"));
        assert!(ui.bounds("record-inspection").y >= header.y + header.height);
        ui.click("record-inspection");
        assert_eq!(ui.node("group-result").text("text"), Some("Recorded"));
        ui.click("network-settings");
        assert!(!ui.can_hit("check-network"));
        ui.click("network-settings");
        ui.click("check-network");
        assert_eq!(
            ui.node("group-result").text("text"),
            Some("Network checked")
        );
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::GroupTitlesDemo::new());
    exercise(load_mlir(&module.mlir)?.create("GroupTitlesDemo")?);
    Ok(())
}

#[test]
fn modal_titles_reserve_space_for_their_actions_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let size = Size::new(390.0, 700.0);
        let mut ui = UiDriver::headless(component, size);
        for (open, overlay, action, expected_lines, result) in [
            (
                "review-trigger",
                "review-dialog",
                "save-review",
                2,
                "Review saved",
            ),
            (
                "evidence-trigger",
                "evidence-drawer",
                "keep-evidence",
                1,
                "Evidence kept",
            ),
        ] {
            ui.click(open);
            let path = ui.path(overlay);
            let scene = ui.scene();
            let frame = layout_with_input(scene, size, &mut ScaledText(2.6), ui.input());
            let panel = frame.at_path(&path).unwrap();
            let node = ui.node(overlay);
            assert_eq!(panel.dialog_title_lines().len(), expected_lines);
            if overlay == "evidence-drawer" {
                assert!(panel.dialog_title_lines().last().unwrap().ellipsis);
            }
            let button = ui.path(action);
            let bounds = frame.at_path(&button).unwrap().bounds;
            assert!(bounds.y >= panel.bounds.y + panel.dialog_title_height(node));
            assert!(
                ui.input()
                    .accessibility_target(scene, &frame, &button)
                    .is_some()
            );
            ui.click(action);
            assert_eq!(ui.node("result-label").text("text"), Some(result));
        }
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::ModalTitlesDemo::new());
    exercise(load_mlir(&module.mlir)?.create("ModalTitlesDemo")?);
    Ok(())
}

#[test]
fn compact_toggles_keep_touch_and_keyboard_activation_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 700.0));
        for id in ["include-check", "alerts-switch", "east", "west", "blocked"] {
            assert_eq!(ui.bounds(id).height, 44.0);
        }
        ui.tab();
        assert!(ui.activate(ActivationKey::Space));
        assert!(ui.node("include-check").boolean("checked", false));
        ui.tab();
        assert!(ui.activate(ActivationKey::Space));
        assert!(ui.node("alerts-switch").boolean("checked", false));
        ui.click("west");
        assert_eq!(ui.node("region").int("current-index"), Some(1));
        let mut input = Interaction::default();
        let blocked = ui.bounds("blocked");
        input.pointer_move(
            ui.scene(),
            ui.layout(),
            Point {
                x: blocked.x + blocked.width * 0.5,
                y: blocked.y + blocked.height * 0.5,
            },
        );
        input.pointer_press_in_scene(ui.scene(), ui.layout());
        assert!(input.pointer_release_target().is_none());
        assert!(ui.node("blocked").boolean("checked", false));
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::CompactTogglesDemo::new());
    exercise(load_mlir(&module.mlir)?.create("CompactTogglesDemo")?);
    Ok(())
}

#[test]
fn mobile_appearance_switches_the_palette_and_preserves_the_form_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_runtime::ColorScheme;

    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
        assert_eq!(ui.scene().color_scheme(), Some(ColorScheme::System));
        ui.click("appearance-draft");
        ui.fill_focused("Saved inspection draft");
        for (id, scheme) in [
            ("dark-mode", ColorScheme::Dark),
            ("light-mode", ColorScheme::Light),
            ("system-mode", ColorScheme::System),
        ] {
            ui.click(id);
            assert_eq!(ui.scene().color_scheme(), Some(scheme));
            assert_eq!(
                ui.node("appearance-draft").text("text"),
                Some("Saved inspection draft")
            );
        }
        ui.click("appearance-details");
        assert!(ui.node("appearance-dialog").boolean("open", false));
        ui.escape();
        assert!(!ui.node("appearance-dialog").boolean("open", false));
        assert_eq!(
            ui.node("appearance-draft").text("text"),
            Some("Saved inspection draft")
        );
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::MobileAppearance::new());
    exercise(load_mlir(&module.mlir)?.create("MobileAppearance")?);
    Ok(())
}

#[test]
fn sign_in_input_purposes_preserve_traits_and_edit_validation_on_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_runtime::{InputPurpose, InputType};

    fn exercise(component: impl Component) {
        let mut ui = UiDriver::headless(component, Size::new(390.0, 720.0));
        assert!(ui.context_menu("review-trigger"));
        assert!(ui.click_menu_row("Sign-in fields"));
        assert_eq!(ui.focused(), Some("sign-in-account"));
        for (id, purpose, input_type) in [
            ("sign-in-account", InputPurpose::Username, InputType::Email),
            (
                "sign-in-password",
                InputPurpose::CurrentPassword,
                InputType::Password,
            ),
            (
                "sign-in-new-password",
                InputPurpose::NewPassword,
                InputType::Password,
            ),
            (
                "verification-code",
                InputPurpose::OneTimeCode,
                InputType::Number,
            ),
            ("sign-in-note", InputPurpose::Auto, InputType::Text),
        ] {
            assert_eq!(ui.node(id).input_purpose(), purpose);
            assert_eq!(ui.node(id).input_type(), input_type);
        }
        ui.fill_focused("agent@example.test");
        ui.click("verification-code");
        assert!(!ui.edit_focused(EditAction::ReplaceAll("ABC")));
        ui.fill_focused("123456");
        assert!(!ui.edit_focused(EditAction::Insert("7")));
        assert_eq!(ui.node("verification-code").text("text"), Some("123456"));
        assert_eq!(
            ui.node("sign-in-account").text("text"),
            Some("agent@example.test")
        );
        ui.escape();
        assert!(!ui.node("sign-in-dialog").boolean("open", false));
    }

    let module = xrack_compiler::compile(include_str!("native_actions.xr"), "native_actions.xr")?;
    exercise(native_actions_ui::NativeActionsDemo::new());
    exercise(load_mlir(&module.mlir)?.create("NativeActionsDemo")?);
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
