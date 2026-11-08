pub mod ui_driver;

use async_ui::AsyncDemo;
use controls_ui::{
    CalendarDemo, CheckStateDemo, DecimalPrice, DecimalSpinDemo, DelayDemo, GroupBoxDemo,
    ReviewRow, TimeDemo, TimerDemo, TumblerDemo,
};
use counter_ui::Counter;
use delegates_ui::SwipeDemo;
use dialog_ui::{DialogDemo, PopupDemo, StandardDialogDemo};
use form_ui::ValidationDemo;
use list_ui::{DirectoryItem, GridDemo, StandardListDemo};
use menu_ui::MenuDemo;
use navigation_ui::{PageDemo, StackDemo, TabBarDemo};
use records_ui::{RecordTableDemo, StockRow};
use selection_ui::{EditableChoiceDemo, SelectionDemo};
use split_ui::SplitDemo;
use std::time::{Duration, Instant};
use table_ui::{InventoryRow, StandardTableDemo, TableDemo};
use tooltip_ui::TooltipDemo;
use tree_ui::{TreeDemo, TreeRecord};
use ui_driver::UiDriver;
use xrack_layout::{
    ActivationKey, CalendarGeometry, ChoiceKey, EditAction, EditSource, Interaction, MenuKey,
    NavigationKey, Point, Rect, ScrollGestureKind, Size, SliderKey, SpinGeometry, TableGeometry,
    TableSelectionKey, TextMeasurer, TreeRowGeometry, layout_tooltip, prepare_scene,
    prepare_scene_with_input,
};
use xrack_runtime::{
    CalendarAction, CheckState, Component, ControlAction, Date, DialogRole, Orientation, Property,
    RangeAction, RowSelection, SelectionTarget, SpinAction, StandardButtonKind, SwipeSide,
    TableSelectionAction, TableSort, Time, TreeAction, TreeSelectionAction,
};

struct FixedText;

#[test]
fn constrained_editors_validate_atomic_input_without_losing_selection_or_history() {
    let mut ui = ValidationDemo::new();
    ui.set_note_limit(1);
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let (_, layout) = prepare_scene_with_input(
        &ui,
        scene.clone(),
        Size::new(424.0, 338.0),
        &mut FixedText,
        &input,
    );
    let now = Instant::now();
    let edit = |input: &mut Interaction, ui: &mut ValidationDemo, action, source| {
        let scene = ui.render_shared();
        input.edit_text(ui, &scene, action, source, now)
    };
    let focus_next = |input: &mut Interaction, ui: &ValidationDemo, id| {
        let scene = ui.render_shared();
        input.focus_next(&scene, &layout, false);
        input.sync_editor(&scene);
        assert_eq!(
            input.focused.as_ref().and_then(|target| target.id),
            Some(id)
        );
    };
    let submit = |input: &Interaction, ui: &mut ValidationDemo| {
        let scene = ui.render_shared();
        ui.dispatch_at(&scene, &input.focused.as_ref().unwrap().path, "accepted")
    };

    focus_next(&mut input, &ui, "code-field");
    edit(
        &mut input,
        &mut ui,
        EditAction::SelectAll,
        EditSource::Keyboard,
    );
    for invalid in ["12A4", "12345", "１２３４", "-123"] {
        assert!(!edit(
            &mut input,
            &mut ui,
            EditAction::Insert(invalid),
            EditSource::Atomic
        ));
        assert_eq!(ui.get_code(), "12");
        assert_eq!(input.editor.as_ref().unwrap().selection("12"), 0..2);
    }
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Insert("0123"),
        EditSource::Atomic
    ));
    assert_eq!(ui.get_code(), "0123");

    focus_next(&mut input, &ui, "amount-field");
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Insert("-,"),
        EditSource::Keyboard
    ));
    assert!(!submit(&input, &mut ui));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("二十五".into(), None);
    assert!(!edit(
        &mut input,
        &mut ui,
        EditAction::Insert("二十五"),
        EditSource::Atomic
    ));
    assert!(input.editor.as_ref().unwrap().preedit.is_empty());
    assert_eq!(ui.get_amount(), "-,");
    assert_eq!(ui.get_amount_edits(), 1);
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Insert("25"),
        EditSource::Atomic
    ));
    assert!(submit(&input, &mut ui));
    assert_eq!(ui.get_submitted_amount(), "-,25");
    let scene = ui.render_shared();
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_amount(), "-,");
    let scene = ui.render_shared();
    assert!(input.replay_text_edit(&mut ui, &scene, true));
    assert_eq!(ui.get_amount(), "-,25");

    focus_next(&mut input, &ui, "note-field");
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Insert("e"),
        EditSource::Keyboard
    ));
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Insert("\u{301}"),
        EditSource::Atomic
    ));
    assert!(!edit(
        &mut input,
        &mut ui,
        EditAction::Insert("\n"),
        EditSource::Keyboard
    ));
    assert_eq!(ui.get_note(), "e\u{301}");
    ui.set_note("👨‍👩‍👧‍👦甲乙".into());
    edit(
        &mut input,
        &mut ui,
        EditAction::End(false),
        EditSource::Keyboard,
    );
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Backspace,
        EditSource::Keyboard
    ));
    assert_eq!(ui.get_note(), "👨‍👩‍👧‍👦甲");
    assert!(edit(
        &mut input,
        &mut ui,
        EditAction::Backspace,
        EditSource::Keyboard
    ));
    assert_eq!(ui.get_note(), "👨‍👩‍👧‍👦");
}

#[test]
fn nonmodal_popup_restores_focus_and_passes_through_the_outside_press() {
    let mut driver = UiDriver::new(PopupDemo::new(), FixedText, Size::new(480.0, 300.0));
    driver.click("filter-anchor");
    assert!(driver.component().get_filters_open());
    assert_eq!(driver.component().get_opens(), 1);
    assert_eq!(driver.focused(), Some("query-field"));
    driver.fill_focused("network");
    driver.tab();
    assert_eq!(driver.focused(), Some("finish-filter"));
    driver.tab();
    assert_eq!(driver.focused(), Some("query-field"));
    driver.click("finish-filter");
    assert_eq!(driver.component().get_query(), "network");
    assert!(!driver.component().get_filters_open());
    assert_eq!(driver.focused(), Some("filter-anchor"));
    assert_eq!(driver.component().get_closes(), 1);

    driver.click("filter-anchor");
    driver.click("filter-anchor");
    assert_eq!(driver.component().get_opens(), 2);
    assert!(driver.component().get_filters_open());
    driver.click("background-action");
    assert_eq!(driver.component().get_closes(), 2);
    assert_eq!(driver.component().get_background_clicks(), 1);
    assert_eq!(driver.focused(), Some("background-action"));
}

#[test]
fn modal_popup_blocks_background_controls_and_dismisses_with_escape() {
    let mut ui = PopupDemo::new();
    ui.set_modal(true);
    let mut driver = UiDriver::new(ui, FixedText, Size::new(480.0, 300.0));
    driver.click("filter-anchor");
    assert_eq!(driver.focused(), Some("query-field"));
    assert!(!driver.can_hit("background-action"));
    driver.escape();
    assert!(!driver.component().get_filters_open());
    assert_eq!(driver.component().get_background_clicks(), 0);
    assert_eq!(driver.focused(), Some("filter-anchor"));
}

#[test]
fn compiled_timer_restarts_pauses_and_stops_single_shots() {
    use xrack_runtime::TimerQueue;

    let mut ui = TimerDemo::new();
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut queue = TimerQueue::default();
    let sync = |queue: &mut TimerQueue, ui: &TimerDemo, millis| {
        let scene = ui.render_shared();
        assert!(scene.children.iter().all(|node| node.kind != "Timer"));
        queue.sync(at(millis), &scene.timers);
    };
    sync(&mut queue, &ui, 0);
    assert_eq!(queue.deadline(), Some(at(250)));
    assert!(queue.next_due(at(249)).is_none());
    assert!(queue.next_due(at(250)).unwrap().fire());
    assert_eq!(ui.get_remaining(), 3);
    assert!(ui.dispatch("pause-countdown", "clicked"));
    assert!(!ui.get_ticking());
    sync(&mut queue, &ui, 300);
    assert_eq!(queue.deadline(), None);
    assert!(ui.dispatch("resume-countdown", "clicked"));
    sync(&mut queue, &ui, 400);
    assert_eq!(queue.deadline(), Some(at(650)));
    assert!(ui.dispatch("resume-countdown", "clicked"));
    sync(&mut queue, &ui, 500);
    assert_eq!(queue.deadline(), Some(at(650)));
    let old = queue.next_due(at(650)).unwrap();
    assert!(ui.dispatch("restart-countdown", "clicked"));
    assert!(!old.fire());
    assert_eq!(ui.get_remaining(), 4);
    sync(&mut queue, &ui, 650);
    assert_eq!(queue.deadline(), Some(at(900)));

    assert!(ui.dispatch("start-pulse", "clicked"));
    sync(&mut queue, &ui, 660);
    assert_eq!(queue.deadline(), Some(at(780)));
    assert!(ui.get_pulse_pending());
    assert!(queue.next_due(at(780)).unwrap().fire());
    assert!(!ui.get_pulse_pending());
    assert_eq!(ui.get_pulses(), 1);
    sync(&mut queue, &ui, 780);
    ui.set_interval_ms(0);
    sync(&mut queue, &ui, 810);
    assert!(queue.deadline().is_none());

    assert!(ui.dispatch("start-pulse", "clicked"));
    sync(&mut queue, &ui, 820);
    let delivery = queue.next_due(at(940)).unwrap();
    drop(ui);
    assert!(!delivery.fire());
}

#[test]
fn swipe_delegate_routes_horizontal_and_vertical_gestures_and_hides_uncommitted_actions() {
    let mut ui = SwipeDemo::new();
    ui.set_messages((0..10_000).map(|index| format!("Mail {index}")).collect());
    let prepare = |ui: &SwipeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(560.0, 370.0),
            &mut FixedText,
            input,
        )
    };
    let now = Instant::now();
    let row = [0, 2, 0, 0];
    let right = [0, 2, 0, 0, 2];
    let archive = [0, 2, 0, 0, 2, 0];
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    let bounds = layout.at_path(&row).unwrap().bounds;
    let start = Point {
        x: bounds.x + 180.0,
        y: bounds.y + 22.0,
    };
    assert!(!layout.at_path(&right).unwrap().visible);
    assert!(!ui.dispatch_at(&scene, &archive, "clicked"));
    assert!(input.begin_scroll_gesture(
        &scene,
        &layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    let short = Point {
        x: start.x - 28.0,
        ..start
    };
    assert!(input.move_scroll_gesture(&scene, &layout, short, now + Duration::from_millis(20)));
    let (scene, preview) = prepare(&ui, &input);
    let panel = preview.at_path(&right).unwrap();
    assert!(panel.visible);
    assert!(!panel.enabled);
    assert_eq!(panel.clip.width, 28.0);
    assert!(!ui.dispatch_at(&scene, &archive, "clicked"));
    let release =
        input.end_scroll_gesture(&scene, &preview, short, now + Duration::from_millis(30));
    assert!(!release.tap && release.control.is_none());
    let (scene, layout) = prepare(&ui, &input);
    assert!(!layout.at_path(&right).unwrap().visible);

    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    let vertical = Point {
        x: start.x - 12.0,
        y: start.y - 48.0,
    };
    let end = input.end_scroll_gesture(&scene, &layout, vertical, now + Duration::from_millis(40));
    assert!(end.scrolled && end.control.is_none() && !end.tap);
    assert!(input.scroll_offsets()[&[0, 2][..]] > 30.0);

    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.begin_scroll_gesture(
        &scene,
        &layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    let far = Point {
        x: start.x - 65.0,
        ..start
    };
    let end = input.end_scroll_gesture(&scene, &layout, far, now + Duration::from_millis(40));
    assert!(!end.tap);
    let action = end.control.unwrap();
    assert_eq!(
        action.action,
        ControlAction::SwipeDelegate(SwipeSide::Right)
    );
    assert!(ui.control_at(&scene, &action.path, action.action));
    let (scene, layout) = prepare(&ui, &input);
    assert!(layout.at_path(&right).unwrap().enabled);
    assert!(input.is_focused(&row));
    let close = input
        .swipe_delegate_key(&scene, &layout, NavigationKey::Right)
        .unwrap();
    assert!(ui.control_at(&scene, &close.path, close.action));
    let (scene, layout) = prepare(&ui, &input);
    let open = input
        .swipe_delegate_key(&scene, &layout, NavigationKey::Left)
        .unwrap();
    assert!(ui.control_at(&scene, &open.path, open.action));
    let (scene, layout) = prepare(&ui, &input);
    let bounds = layout.at_path(&archive).unwrap().bounds;
    let point = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, archive);
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.is_focused(&row));
    assert!(!layout.at_path(&right).unwrap().visible);
    assert_eq!(
        scene.at_path(&[0, 2, 0, 0, 1, 0]).unwrap().text("text"),
        Some("Mail 0 · 已归档 · 点击 0")
    );
}

#[test]
fn swipe_delegate_preview_and_commit_reject_a_reused_virtual_slot() {
    let mut ui = SwipeDemo::new();
    ui.set_messages((0..30).map(|index| format!("Mail {index}")).collect());
    let prepare = |ui: &SwipeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(560.0, 370.0),
            &mut FixedText,
            input,
        )
    };
    let mut input = Interaction::default();
    let now = Instant::now();
    let row = [0, 2, 0, 0];
    let (old, frame) = prepare(&ui, &input);
    let key = old.row_key_at(&row);
    let bounds = frame.at_path(&row).unwrap().bounds;
    let start = Point {
        x: bounds.x + 180.0,
        y: bounds.y + 22.0,
    };
    let moved = Point {
        x: start.x + 68.0,
        ..start
    };
    assert!(input.begin_scroll_gesture(&old, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&old, &frame, moved, now + Duration::from_millis(20)));
    assert!(ui.model_messages().insert(0, "New message".into()));
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.row_key_at(&[0, 2, 1, 0]), key);
    assert!(!layout.at_path(&[0, 2, 0, 0, 0]).unwrap().visible);
    let mut fresh = input.clone();
    let action = input
        .end_scroll_gesture(&old, &frame, moved, now + Duration::from_millis(40))
        .control
        .unwrap();
    assert!(!ui.control_at(&old, &action.path, action.action));
    assert!(
        fresh
            .end_scroll_gesture(&scene, &layout, moved, now + Duration::from_millis(40))
            .control
            .is_none()
    );

    let original = [0, 2, 1, 0];
    assert!(ui.control_at(
        &scene,
        &original,
        ControlAction::SwipeDelegate(SwipeSide::Left)
    ));
    assert!(ui.model_messages().remove(0).is_some());
    let (scene, layout) = prepare(&ui, &fresh);
    assert_eq!(scene.row_key_at(&row), key);
    assert_eq!(scene.at_path(&row).unwrap().swipe_side(), SwipeSide::Left);
    assert!(layout.at_path(&[0, 2, 0, 0, 0]).unwrap().enabled);
}

#[test]
fn standard_dialog_buttons_apply_reset_help_discard_and_restore_focus() {
    let mut driver = UiDriver::new(
        StandardDialogDemo::new(),
        FixedText,
        Size::new(620.0, 380.0),
    );
    driver.click("open-standard");
    assert_eq!(driver.focused(), Some("setting-name"));
    assert_eq!(xrack_runtime::button_text(driver.node("save")), "Save");
    driver.fill_focused("Applied");
    driver.click("apply");
    assert!(driver.component().get_settings_open());
    assert_eq!(driver.component().get_applies(), 1);
    assert_eq!(driver.component().get_saved(), "Applied");
    driver.click("setting-name");
    driver.fill_focused("Uncommitted");
    driver.click("reset");
    assert_eq!(driver.component().get_resets(), 1);
    assert_eq!(driver.component().get_draft(), "Applied");
    driver.click("help");
    assert_eq!(driver.component().get_helps(), 1);
    assert!(driver.component().get_settings_open());
    driver.click("discard");
    assert!(!driver.component().get_settings_open());
    assert_eq!(driver.component().get_discards(), 1);
    assert_eq!(driver.component().get_saved(), "Applied");
    assert_eq!(driver.component().get_rejects(), 0);
    assert_eq!(driver.focused(), Some("open-standard"));
    driver.click("open-standard");
    driver.click("save");
    assert_eq!(driver.component().get_accepts(), 1);
    assert!(!driver.component().get_settings_open());
    driver.click("open-standard");
    driver.escape();
    assert_eq!(driver.component().get_rejects(), 1);
    assert_eq!(driver.component().get_discards(), 1);
}

#[test]
fn destructive_button_requires_explicit_focus_and_rejects_a_changed_pressed_kind() {
    let mut ui = StandardDialogDemo::new();
    ui.set_settings_open(true);
    ui.set_danger_only(true);
    let prepare = |ui: &StandardDialogDemo| {
        let scene = ui.render_shared();
        let frame = xrack_layout::layout(&scene, Size::new(620.0, 380.0), &mut FixedText);
        (scene, frame)
    };
    let mut input = Interaction::default();
    let (scene, frame) = prepare(&ui);
    input.reconcile(&scene, &frame);
    assert!(input.is_focused(&[3]));
    assert!(input.dialog_accept(&scene, &frame).is_none());
    input.focus_next(&scene, &frame, false);
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("discard")
    );

    ui.set_danger_only(false);
    ui.set_show_editor(false);
    let mut input = Interaction::default();
    let mut keyboard = Interaction::default();
    let (scene, frame) = prepare(&ui);
    input.reconcile(&scene, &frame);
    keyboard.reconcile(&scene, &frame);
    assert_eq!(
        keyboard.focused.as_ref().and_then(|target| target.id),
        Some("save")
    );
    keyboard.key_press_with_scene(&scene, ActivationKey::Enter);
    let save = [3, 2, 0];
    let bounds = frame.at_path(&save).unwrap().bounds;
    let point = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    input.pointer_move(&scene, &frame, point);
    input.pointer_press();
    ui.set_preferred_kind(StandardButtonKind::Discard);
    let (scene, frame) = prepare(&ui);
    input.reconcile(&scene, &frame);
    assert!(input.is_hovered(&save));
    assert!(input.pointer_release_target().is_none());
    assert!(
        keyboard
            .key_release_with_scene(&scene, ActivationKey::Enter)
            .is_none()
    );
    assert!(input.dialog_accept(&scene, &frame).is_none());
    assert_eq!(ui.get_discards(), 0);
    assert!(ui.get_settings_open());

    ui.set_preferred_kind(StandardButtonKind::Save);
    ui.set_save_enabled(false);
    let (scene, frame) = prepare(&ui);
    input.reconcile(&scene, &frame);
    assert!(input.dialog_accept(&scene, &frame).is_none());
}

#[test]
fn grid_view_virtualizes_reflows_and_preserves_delegate_state_by_model_key() {
    let mut demo = GridDemo::new();
    demo.set_records(
        (0..50_000)
            .map(|rank| DirectoryItem {
                label: format!("Tile {rank}"),
                rank,
            })
            .collect(),
    );
    let wide = Size::new(620.0, 420.0);
    let narrow = Size::new(340.0, 420.0);
    let path = [0, 1];
    let prepare = |demo: &GridDemo, input: &Interaction, size| {
        prepare_scene_with_input(demo, demo.render_shared(), size, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, frame) = prepare(&demo, &input, wide);
    let grid = scene.at_path(&path).unwrap();
    assert!(grid.children.len() <= 20);
    let first_key = grid.children[0].row_key;
    let cells = &frame.at_path(&path).unwrap().children;
    assert_eq!(cells[3].bounds.y, cells[0].bounds.y);
    assert_eq!(cells[3].bounds.x, cells[0].bounds.x + 3.0 * 136.0);
    assert_eq!(cells[4].bounds.y, cells[0].bounds.y + 92.0);

    let point = Point {
        x: cells[1].bounds.x + 8.0,
        y: cells[1].bounds.y + 8.0,
    };
    input.pointer_move(&scene, &frame, point);
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let action = input.select_group_option(&scene, &frame, &target).unwrap();
    assert_eq!(action.action, ControlAction::Select(1));
    assert!(demo.control_at(&scene, &action.path, action.action));
    let (scene, frame) = prepare(&demo, &input, wide);
    let action = input.list_key(&scene, &frame, NavigationKey::Down).unwrap();
    assert_eq!(action.action, ControlAction::Select(5));
    assert!(demo.control_at(&scene, &action.path, action.action));
    assert!(demo.dispatch_at(&scene, &[0, 1, 0, 0, 1], "clicked"));

    demo.set_current(49_999);
    let (scene, frame) = prepare(&demo, &input, wide);
    let grid = scene.at_path(&path).unwrap();
    assert!(grid.list.unwrap().first_row >= 49_980);
    assert!(grid.children.last().unwrap().boolean("selected", false));
    assert_eq!(demo.get_selections(), 2);
    let item = frame.at_path(&path).unwrap();
    input.pointer_move(
        &scene,
        &frame,
        Point {
            x: item.bounds.x + 14.0,
            y: item.bounds.y + 20.0,
        },
    );
    input.reconcile(&scene, &frame);
    assert!(input.scroll_y(&frame, -460.0));
    let (scrolled, frame) = prepare(&demo, &input, wide);
    input.reconcile(&scrolled, &frame);
    assert!(
        !scrolled
            .at_path(&path)
            .unwrap()
            .children
            .iter()
            .any(|item| item.boolean("selected", false))
    );
    let (resized, _) = prepare(&demo, &input, narrow);
    let grid = resized.at_path(&path).unwrap();
    assert!(grid.children.len() <= 10);
    assert!(grid.children.last().unwrap().boolean("selected", false));

    let model = demo.model_records();
    assert!(model.insert(
        0,
        DirectoryItem {
            label: "New tile".into(),
            rank: -1
        }
    ));
    assert!(!demo.control_at(&resized, &path, ControlAction::Select(49_998)));
    demo.set_current(0);
    let (scene, frame) = prepare(&demo, &input, narrow);
    let grid = scene.at_path(&path).unwrap();
    assert_eq!(grid.list.unwrap().first_row, 0);
    assert_eq!(grid.children[1].row_key, first_key);
    assert!(grid.children[1].children[0].children[1].boolean("checked", false));
    let cells = &frame.at_path(&path).unwrap().children;
    assert_eq!(cells[2].bounds.x, cells[0].bounds.x);
    assert_eq!(cells[2].bounds.y, cells[0].bounds.y + 92.0);
}

#[test]
fn page_keeps_chrome_fixed_and_tabs_in_visual_order() {
    let component = PageDemo::new();
    let viewport = Size::new(360.0, 240.0);
    let scene = component.render_shared();
    let page = &scene.children[0];
    assert_eq!(page.id, Some("settings"));
    let mut input = Interaction::default();
    let frame = xrack_layout::layout_with_input(&scene, viewport, &mut FixedText, &input);
    let [footer, content, header] = frame.children[0].children.as_slice() else {
        panic!("expected three page regions")
    };
    assert_eq!(header.bounds.x, 0.0);
    assert_eq!(header.bounds.width, viewport.width);
    assert!(
        header.bounds.height >= 40.0,
        "header uses toolbar's preferred height"
    );
    assert_eq!(footer.bounds, Rect::new(0.0, 196.0, 360.0, 44.0));
    assert_eq!(content.bounds.x, 12.0);
    assert_eq!(content.bounds.y, header.bounds.height + 18.0);
    assert_eq!(content.bounds.width, 336.0);
    assert!((content.bounds.y + content.bounds.height - (footer.bounds.y - 18.0)).abs() < 0.001);
    assert!((content.bounds.height - content.clip.height).abs() < 0.001);
    input.pointer_move(
        &scene,
        &frame,
        Point {
            x: 240.0,
            y: content.bounds.y + 50.0,
        },
    );
    assert!(input.scroll_y(&frame, 100.0));
    let scrolled = xrack_layout::layout_with_input(&scene, viewport, &mut FixedText, &input);
    assert_eq!(scrolled.children[0].children[0].bounds, footer.bounds);
    assert_eq!(scrolled.children[0].children[2].bounds, header.bounds);
    assert_eq!(
        scrolled.children[0].children[1].scroll.unwrap().offset,
        100.0
    );

    let mut driver = UiDriver::new(component, FixedText, viewport);
    for id in ["header-action", "option", "save"] {
        driver.tab();
        assert_eq!(driver.focused(), Some(id));
    }
    driver.click("save");
    assert_eq!(driver.component().get_saves(), 1);
    driver.click("header-action");
    assert!(!driver.component().get_show_header());
    driver.tab();
    assert_eq!(driver.focused(), Some("option"));
}

#[test]
fn standard_list_reveals_programmatic_selection_and_keeps_manual_scroll_between_changes() {
    let mut ui = StandardListDemo::new();
    ui.set_records(
        (0..50_000)
            .map(|index| DirectoryItem {
                label: format!("Row {index}"),
                rank: index,
            })
            .collect(),
    );
    let mut input = Interaction::default();
    let prepare = |ui: &StandardListDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(584.0, 355.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, _) = prepare(&ui, &input);
    let view = scene
        .children
        .iter()
        .position(|node| node.id == Some("records-view"))
        .unwrap();
    let path = [view];
    let model = scene.at_path(&path).unwrap();
    assert_eq!(model.list.unwrap().row_count, 50_000);
    assert!(model.children.len() <= 8);
    assert_eq!(
        scene.find("categories-view").unwrap().children[0].children[0].text("text"),
        Some("全部")
    );

    ui.set_current(49_999);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let model = scene.at_path(&path).unwrap();
    assert!(model.list.unwrap().first_row > 49_990);
    assert!(model.children.last().unwrap().boolean("selected", false));
    let bounds = layout.at_path(&path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + 16.0,
            y: bounds.y + 16.0,
        },
    );
    assert!(input.scroll_y(&layout, -960.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(scene.at_path(&path).unwrap().list.unwrap().first_row < 49_980);

    ui.set_current(0);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(scene.at_path(&path).unwrap().list.unwrap().first_row, 0);
    assert_eq!(ui.get_selections(), 0);
    input.accessibility_focus(input.accessibility_target(&scene, &layout, &path).unwrap());
    let select = input.list_key(&scene, &layout, NavigationKey::End).unwrap();
    assert!(ui.control_at(&scene, &select.path, select.action));
    assert_eq!(ui.get_current(), 49_999);
    assert_eq!(ui.get_selections(), 1);

    let (stale, _) = prepare(&ui, &input);
    assert!(ui.model_records().insert(
        0,
        DirectoryItem {
            label: "Inserted".into(),
            rank: -1
        }
    ));
    assert!(!ui.control_at(&stale, &path, ControlAction::Select(0)));
    assert_eq!(ui.get_current(), 49_999);
}

#[test]
fn checkable_group_box_keeps_content_and_skips_it_when_tabbing_or_triggering_shortcuts() {
    let mut driver = UiDriver::new(GroupBoxDemo::new(), FixedText, Size::new(470.0, 376.0));
    driver.tab();
    assert_eq!(driver.focused(), Some("delivery"));
    driver.tab();
    assert_eq!(driver.focused(), Some("address-field"));
    driver.click("delivery");
    assert!(!driver.component().get_active());
    assert!(!driver.component().get_last_active());
    assert_eq!(driver.component().get_group_changes(), 1);
    assert!(!driver.can_hit("address-field"));
    assert!(!driver.menu_shortcut("Primary+Shift+P".parse().unwrap()));
    driver.tab();
    assert_eq!(driver.focused(), Some("audit-toggle"));

    driver.click("delivery");
    assert!(driver.component().get_last_active());
    assert_eq!(
        driver.node("address-field").text("text"),
        Some("notify@example.invalid")
    );
    driver.tab();
    assert_eq!(driver.focused(), Some("address-field"));
    assert!(driver.menu_shortcut("Primary+Shift+P".parse().unwrap()));
    assert!(driver.component().get_preview_open());
}

#[test]
fn disabling_a_group_removes_nested_overlays_and_returns_focus_to_its_header() {
    let mut ui = GroupBoxDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &GroupBoxDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(470.0, 376.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let group = scene
        .children
        .iter()
        .position(|node| node.id == Some("delivery"))
        .unwrap();
    let field = [group, 0];
    let field_rect = layout.at_path(&field).unwrap().bounds;
    let header = xrack_layout::group_header(
        scene.at_path(&[group]).unwrap(),
        layout.at_path(&[group]).unwrap().bounds,
    );
    assert!(header.y + header.height <= field_rect.y);
    input.focus_next(&scene, &layout, false);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, field);
    ui.set_active(false);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, [group]);
    let disabled = layout.at_path(&field).unwrap();
    assert_eq!(disabled.bounds, field_rect);
    assert!(disabled.visible && !disabled.enabled);
    assert!(!ui.edit_at(&scene, &field, "blocked".into()));

    input.key_press(ActivationKey::Space);
    let target = input.key_release_target(ActivationKey::Space).unwrap();
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(ui.get_active());
    ui.set_preview_open(true);
    let (_, layout) = prepare(&ui, &input);
    assert_eq!(layout.dialog_paths(), &[vec![group, 2]]);
    ui.set_active(false);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(layout.dialog_paths().is_empty());
    assert!(ui.get_preview_open());
    assert!(!ui.dispatch_at(&scene, &[group, 1], "clicked"));
}

#[test]
fn projected_check_state_follows_its_children_after_a_user_click() {
    let mut driver = UiDriver::new(CheckStateDemo::new(), FixedText, Size::new(452.0, 350.0));
    assert_eq!(
        driver.node("master").check_state(),
        CheckState::PartiallyChecked
    );
    driver.click("master");
    assert!(driver.component().get_first() && driver.component().get_second());
    assert_eq!(driver.node("master").check_state(), CheckState::Checked);
    assert_eq!(driver.component().get_proposals(), 1);

    driver.click("first-item");
    assert!(!driver.component().get_first());
    assert_eq!(driver.component().get_first_edits(), 1);
    assert_eq!(
        driver.component().get_last_first_state(),
        CheckState::Unchecked
    );
    assert_eq!(
        driver.node("master").check_state(),
        CheckState::PartiallyChecked
    );
    driver.click("first-item");
    assert_eq!(
        driver.component().get_last_first_state(),
        CheckState::Checked
    );
    assert_eq!(driver.component().get_first_edits(), 2);

    for state in [
        CheckState::PartiallyChecked,
        CheckState::Checked,
        CheckState::Unchecked,
    ] {
        driver.click("cycle");
        assert_eq!(driver.component().get_cycle_state(), state);
        assert_eq!(driver.node("cycle").check_state(), state);
    }
    assert_eq!(driver.component().get_cycle_edits(), 3);
}

#[test]
fn check_state_row_edits_keep_the_latest_other_fields() {
    let mut ui = CheckStateDemo::new();
    let model = ui.model_reviews();
    let path = [5, 0, 0, 1];
    let prepare = |ui: &CheckStateDemo| {
        prepare_scene(
            ui,
            ui.render_shared(),
            Size::new(452.0, 350.0),
            &mut FixedText,
            &Default::default(),
        )
        .0
    };
    let scene = prepare(&ui);
    assert_eq!(
        scene.at_path(&path).unwrap().check_state(),
        CheckState::PartiallyChecked
    );
    assert!(model.set_row(
        0,
        ReviewRow {
            title: "新标题".into(),
            state: CheckState::PartiallyChecked
        }
    ));
    assert!(ui.dispatch_at(&scene, &path, "clicked"));
    assert_eq!(
        model.row(0),
        Some(ReviewRow {
            title: "新标题".into(),
            state: CheckState::Checked
        })
    );

    let stale = prepare(&ui);
    assert!(model.set_row(
        0,
        ReviewRow {
            title: "新标题".into(),
            state: CheckState::Unchecked
        }
    ));
    assert!(!ui.dispatch_at(&stale, &path, "clicked"));
    assert_eq!(model.row(0).unwrap().state, CheckState::Unchecked);
}

#[test]
fn delay_button_requires_a_continuous_hold_and_fires_once() {
    let mut ui = DelayDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &DelayDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(400.0, 214.0),
            &mut FixedText,
            input,
        )
    };
    let path = [1];
    let point = Point { x: 62.0, y: 79.0 };
    let now = Instant::now();
    let half = now + Duration::from_millis(400);
    let complete = now + Duration::from_millis(800);
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    input.begin_pointer_delay(&scene, &layout, now);
    assert_eq!(input.delay_deadline(), Some(complete));
    assert!(input.advance_delay(&scene, &layout, half).is_none());
    assert_eq!(input.delay_progress(&path), Some((0.5, true)));
    input.pointer_move(&scene, &layout, Point { x: 370.0, ..point });
    input.pointer_move(&scene, &layout, point);
    assert!(input.advance_delay(&scene, &layout, complete).is_none());
    assert!(input.pointer_release_target().is_none());

    input.pointer_press();
    input.begin_pointer_delay(&scene, &layout, now);
    assert!(input.pointer_release_target().is_none());
    assert!(input.advance_delay(&scene, &layout, complete).is_none());

    input.pointer_press();
    input.begin_pointer_delay(&scene, &layout, now);
    let confirm = input.advance_delay(&scene, &layout, complete).unwrap();
    assert!(ui.control_at(&scene, &confirm.path, confirm.action));
    assert_eq!(ui.get_activations(), 1);
    assert!(ui.get_confirmed());
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(
        input
            .advance_delay(&scene, &layout, complete + Duration::from_millis(100))
            .is_none()
    );
    assert!(input.pointer_release_target().is_none());
    input.pointer_press();
    input.begin_pointer_delay(&scene, &layout, complete);
    let reset = input.pointer_release_target().unwrap();
    assert!(ui.control_at(&scene, &reset.path, ControlAction::DelayButton(false)));
    assert!(!ui.get_confirmed());
    assert_eq!(ui.get_activations(), 1);
}

#[test]
fn delay_button_cancels_when_disabled_and_suppresses_touch_taps_after_activation() {
    use xrack_layout::ScrollGestureKind;

    let mut ui = DelayDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &DelayDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(400.0, 214.0),
            &mut FixedText,
            input,
        )
    };
    let now = Instant::now();
    let complete = now + Duration::from_millis(800);
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    input.key_press(ActivationKey::Space);
    input.begin_key_delay(&scene, &layout, ActivationKey::Space, now);
    ui.set_allow_confirm(false);
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.advance_delay(&scene, &layout, complete).is_none());
    assert!(input.key_release_target(ActivationKey::Space).is_none());
    assert_eq!(ui.get_activations(), 0);

    ui.set_allow_confirm(true);
    let point = Point { x: 56.0, y: 82.0 };
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    let touch_up = input.end_scroll_gesture(&scene, &layout, point, complete);
    assert!(touch_up.control.is_none() && !touch_up.tap);
    assert!(input.advance_delay(&scene, &layout, complete).is_none());

    assert!(input.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    let action = input.advance_delay(&scene, &layout, complete).unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    let (scene, layout) = prepare(&ui, &input);
    let touch_up =
        input.end_scroll_gesture(&scene, &layout, point, complete + Duration::from_millis(1));
    assert!(touch_up.control.is_none() && !touch_up.tap);
    assert!(ui.get_confirmed());
    assert_eq!(ui.get_activations(), 1);

    ui.set_confirmed(false);
    let (scene, layout) = prepare(&ui, &input);
    input.key_press(ActivationKey::Enter);
    input.begin_key_delay(&scene, &layout, ActivationKey::Enter, now);
    let action = input.advance_delay(&scene, &layout, complete).unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert!(input.key_release_target(ActivationKey::Enter).is_none());
    assert_eq!(ui.get_activations(), 2);
}

#[test]
fn tumbler_accumulates_wheel_pixels_and_commits_once_when_the_drag_snaps() {
    use xrack_layout::{ScrollGestureKind, TumblerScroll};

    let mut ui = TumblerDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &TumblerDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(388.0, 324.0),
            &mut FixedText,
            input,
        )
    };
    let path = [2];
    let (scene, layout) = prepare(&ui, &input);
    let center = layout
        .at_path(&path)
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap()
        .center;
    let point = Point {
        x: center.x + 16.0,
        y: center.y + center.height * 0.5,
    };
    input.pointer_move(&scene, &layout, point);
    for _ in 0..2 {
        assert!(matches!(
            input.tumbler_scroll(&scene, &layout, center.height * 0.4, false),
            TumblerScroll::Pending
        ));
    }
    let TumblerScroll::Select(action) =
        input.tumbler_scroll(&scene, &layout, center.height * 0.4, false)
    else {
        panic!("expected one wheel step")
    };
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_minute(), 31);

    ui.set_minute(59);
    let (scene, layout) = prepare(&ui, &input);
    let TumblerScroll::Select(action) = input.tumbler_scroll(&scene, &layout, 1.0, true) else {
        panic!("expected a wraparound step")
    };
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_minute(), 0);

    let (scene, layout) = prepare(&ui, &input);
    let now = Instant::now();
    let moved = Point {
        y: point.y - center.height * 2.4,
        ..point
    };
    assert!(input.begin_scroll_gesture(
        &scene,
        &layout,
        point,
        now,
        ScrollGestureKind::MouseTumbler
    ));
    assert!(input.move_scroll_gesture(&scene, &layout, moved, now + Duration::from_millis(40)));
    let (preview, preview_layout) = prepare(&ui, &input);
    let wheel = preview_layout
        .at_path(&path)
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap();
    assert_eq!(ui.get_minute(), 0);
    assert!(
        wheel
            .rows
            .iter()
            .any(|row| row.index == 2 && row.displacement.abs() < 0.5)
    );
    let release = input.end_scroll_gesture(
        &preview,
        &preview_layout,
        moved,
        now + Duration::from_millis(48),
    );
    let action = release.control.unwrap();
    assert!(release.scrolled);
    assert!(input.tumbler_preview().is_none());
    assert!(ui.control_at(&preview, &action.path, action.action));
    assert_eq!(ui.get_minute(), 2);
    assert_eq!(ui.get_changes(), 3);

    let (scene, layout) = prepare(&ui, &input);
    let page = input
        .tumbler_key(&scene, &layout, NavigationKey::PageDown)
        .unwrap();
    assert!(ui.control_at(&scene, &page.path, page.action));
    assert_eq!(ui.get_minute(), 7);
    ui.set_cycle(false);
    ui.set_minute(59);
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    assert!(matches!(
        input.tumbler_scroll(&scene, &layout, 1.0, true),
        TumblerScroll::Unhandled
    ));
}

#[test]
fn tumbler_discards_a_drag_and_row_click_from_an_old_model_order() {
    use xrack_layout::ScrollGestureKind;

    let mut ui = TumblerDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &TumblerDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(388.0, 324.0),
            &mut FixedText,
            input,
        )
    };
    let path = [1];
    let (scene, layout) = prepare(&ui, &input);
    let wheel = layout.at_path(&path).unwrap().tumbler.as_ref().unwrap();
    let old_click = wheel
        .rows
        .iter()
        .find(|row| row.index == 9)
        .unwrap()
        .action();
    let point = Point {
        x: wheel.center.x + 12.0,
        y: wheel.center.y + 12.0,
    };
    let moved = Point {
        y: point.y - 42.0,
        ..point
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &layout, moved, now + Duration::from_millis(30)));
    assert!(ui.model_hours().insert(0, "inserted".into()));
    let (next, layout) = prepare(&ui, &input);
    let release = input.end_scroll_gesture(&next, &layout, moved, now + Duration::from_millis(40));
    assert!(release.control.is_none());
    assert!(input.tumbler_preview().is_none());
    assert!(!ui.control_at(&scene, &path, old_click));
    assert_eq!(ui.get_hour(), 8);
    assert_eq!(ui.get_changes(), 0);

    let current_click = layout
        .at_path(&path)
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .find(|row| row.index == 9)
        .unwrap()
        .action();
    ui.set_hours(
        (0..24)
            .map(|index| format!("replacement {index}"))
            .collect(),
    );
    assert!(!ui.control_at(&next, &path, current_click));
    assert_eq!(ui.get_hour(), 8);
}

#[test]
fn search_field_only_opens_matching_results_and_undoes_candidate_selection() {
    use std::{cell::RefCell, rc::Rc};

    let mut ui = AsyncDemo::new();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let capture = requests.clone();
    ui.on_search(move |query| capture.borrow_mut().push(query));
    let mut input = Interaction::default();
    let prepare = |ui: &AsyncDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(540.0, 350.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("query-field"));
    let now = Instant::now();
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("cloud"),
        EditSource::Atomic,
        now
    ));
    assert_eq!(*requests.borrow(), ["cloud"]);
    ui.set_suggestions(vec!["old answer".into()]);
    ui.set_result_query("XRack".into());
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.choice_menu(&scene, &layout).is_none());

    ui.set_suggestions(vec!["cloud files".into(), "cloud storage".into()]);
    ui.set_result_query("cloud".into());
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let menu = input.choice_menu(&scene, &layout).unwrap();
    assert_eq!(menu.highlighted, None);
    assert_eq!(menu.rows, ["cloud files", "cloud storage"]);
    let submit = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert_eq!(submit.action, ControlAction::SubmitSearch);
    assert!(ui.control_at(&scene, &submit.path, submit.action));
    assert_eq!(ui.get_query(), "cloud");

    input.choice_key(&scene, &layout, ChoiceKey::Down);
    assert_eq!(
        input.choice_menu(&scene, &layout).unwrap().highlighted,
        Some(0)
    );
    let row = input.choice_menu(&scene, &layout).unwrap().row_bounds(0);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: row.x + 8.0,
            y: row.y + 8.0,
        },
    );
    assert!(input.choice_press(&scene, &layout));
    ui.set_suggestions(vec![String::new(), "cloud services".into()]);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(
        input.choice_menu(&scene, &layout).unwrap().highlighted,
        None
    );
    assert!(input.choice_release(&scene, &layout).is_none());
    input.choice_key(&scene, &layout, ChoiceKey::Down);
    let select = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert_eq!(select.action, ControlAction::Select(1));
    assert!(input.select_search_suggestion(&mut ui, &scene, 1, now));
    assert_eq!(ui.get_query(), "cloud services");
    assert_eq!(*requests.borrow(), ["cloud", "cloud", "cloud services"]);

    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.choice_menu(&scene, &layout).is_none());
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_query(), "cloud");
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.choice_open());
    input.choice_key(&scene, &layout, ChoiceKey::Escape);
    input.reconcile(&scene, &layout);
    assert!(!input.choice_open());
}

#[test]
fn search_clear_click_passes_through_its_popup_and_is_one_edit() {
    let mut ui = AsyncDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &AsyncDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(540.0, 350.0),
            &mut FixedText,
            input,
        )
    };
    ui.set_suggestions(vec!["XRack docs".into()]);
    ui.set_result_query("XRack".into());
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    input.choice_key(&scene, &layout, ChoiceKey::Open);
    let target = input.focused.as_ref().unwrap().path.clone();
    let clear = xrack_layout::SearchGeometry::new(layout.at_path(&target).unwrap().bounds).clear;
    let point = Point {
        x: clear.x + clear.width * 0.5,
        y: clear.y + clear.height * 0.5,
    };
    assert!(input.choice_open());
    input.pointer_move(&scene, &layout, point);
    assert!(!input.choice_press(&scene, &layout));
    input.pointer_press();
    let click = input.control_part_release(&scene, &layout).unwrap();
    assert_eq!(click.action, ControlAction::ClearSearch);
    assert_eq!(click.path, target);
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll(""),
        EditSource::Atomic,
        Instant::now()
    ));
    assert_eq!(ui.get_query(), "");
    assert_eq!(input.focused.as_ref().unwrap().path, target);
    let (scene, _) = prepare(&ui, &input);
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_query(), "XRack");
}

#[test]
fn decimal_spin_box_uses_the_same_precision_for_clicks_drafts_and_commits() {
    let mut ui = DecimalSpinDemo::new();
    let mut input = Interaction::default();
    let path = [1];
    let prepare = |ui: &DecimalSpinDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(480.0, 306.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let up = SpinGeometry::new(layout.at_path(&path).unwrap().bounds).up;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: up.x + 6.0,
            y: up.y + 6.0,
        },
    );
    input.pointer_press();
    input.spin_press(&scene, &layout);
    let step = input.spin_release(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &step.path, step.action));
    assert_eq!(ui.get_level(), 0.3);

    let (scene, _) = prepare(&ui, &input);
    let spin = scene.at_path(&path).unwrap();
    assert_eq!(input.editor_value(spin), "0,30");
    input.edit_spin(spin, EditAction::SelectAll);
    input.edit_spin(spin, EditAction::Insert("1.25"));
    assert_eq!(input.editor_value(spin), "0,30");
    input.edit_spin(spin, EditAction::Insert("-1,236"));
    assert_eq!(input.editor_value(spin), "-1,236");
    assert_eq!(ui.get_level(), 0.3);
    let commit = input.commit_spin_draft().unwrap();
    assert!(ui.control_at(&scene, &commit.path, commit.action));
    assert_eq!(ui.get_level(), -1.24);
    assert_eq!(ui.get_edits(), 2);

    let (scene, _) = prepare(&ui, &input);
    let spin = scene.at_path(&path).unwrap();
    input.edit_spin(spin, EditAction::SelectAll);
    input.edit_spin(spin, EditAction::Insert("1,5"));
    let stale = input.commit_spin_draft().unwrap();
    ui.set_level(1.1);
    assert!(!ui.control_at(&scene, &stale.path, stale.action));
    assert_eq!(ui.get_level(), 1.1);
    ui.set_locked(true);
    let (scene, _) = prepare(&ui, &input);
    assert!(!ui.control_at(&scene, &path, ControlAction::Spin(SpinAction::Increase)));
}

#[test]
fn decimal_record_editor_preserves_other_fields_and_rejects_a_replaced_value() {
    let mut ui = DecimalSpinDemo::new();
    let model = ui.model_prices();
    let key = model.row_key(0).unwrap();
    let path = [5, 0, 0, 1];
    let mut input = Interaction::default();
    let prepare = |ui: &DecimalSpinDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(480.0, 306.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.accessibility_focus(target);
    let spin = scene.at_path(&path).unwrap();
    input.edit_spin(spin, EditAction::SelectAll);
    input.edit_spin(spin, EditAction::Insert("12,75"));
    assert!(model.set_row(
        0,
        DecimalPrice {
            label: "已改名".into(),
            price: 12.5
        }
    ));
    let action = input.commit_spin_draft().unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(model.row_key(0), Some(key));
    assert_eq!(
        model.row(0),
        Some(DecimalPrice {
            label: "已改名".into(),
            price: 12.75
        })
    );

    let (scene, _) = prepare(&ui, &input);
    let spin = scene.at_path(&path).unwrap();
    input.edit_spin(spin, EditAction::SelectAll);
    input.edit_spin(spin, EditAction::Insert("12,99"));
    assert!(model.set_row(
        0,
        DecimalPrice {
            label: "已改名".into(),
            price: 13.0
        }
    ));
    let stale = input.commit_spin_draft().unwrap();
    assert!(!ui.control_at(&scene, &stale.path, stale.action));
    assert_eq!(model.row(0).unwrap().price, 13.0);
}

#[test]
fn calendar_browses_without_committing_and_rejects_stale_day_presses() {
    let mut ui = CalendarDemo::new();
    let mut input = Interaction::default();
    let day = |month, day| Date::new(2024, month, day).unwrap();
    let prepare = |ui: &CalendarDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(440.0, 460.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("calendar"));
    let right = input
        .calendar_key(&scene, &layout, NavigationKey::Right)
        .unwrap();
    assert!(ui.control_at(&scene, &right.path, right.action));
    assert_eq!(ui.get_selected(), day(2, 29));
    assert_eq!(ui.get_picked_day(), 29);

    let (scene, layout) = prepare(&ui, &input);
    let browse = input
        .calendar_key(&scene, &layout, NavigationKey::PageDown)
        .unwrap();
    assert!(ui.control_at(&scene, &browse.path, browse.action));
    assert_eq!(ui.get_month(), day(3, 1));
    assert_eq!(ui.get_selected(), day(2, 29));
    assert_eq!((ui.get_date_changes(), ui.get_month_changes()), (1, 1));
    let (scene, layout) = prepare(&ui, &input);
    let geometry = CalendarGeometry::new(scene.at_path(&[1]).unwrap(), layout.children[1].bounds);
    let target = geometry
        .action_bounds(CalendarAction::Select(day(3, 18)))
        .unwrap();
    let point = Point {
        x: target.x + target.width * 0.5,
        y: target.y + target.height * 0.5,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    let click = input.control_part_release(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &click.path, click.action));
    assert_eq!(ui.get_selected(), day(3, 18));

    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    ui.set_month(day(4, 1));
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.control_part_release(&scene, &layout).is_none());
    let outside = ControlAction::Calendar(CalendarAction::Select(day(4, 16)));
    assert!(!ui.control_at(&scene, &[1], outside));
    assert_eq!(ui.get_selected(), day(3, 18));

    ui.set_selected(day(2, 1));
    let (scene, _) = prepare(&ui, &input);
    let first_allowed = ControlAction::Calendar(CalendarAction::Select(day(2, 5)));
    assert!(ui.control_at(&scene, &[1], first_allowed));
    assert_eq!(ui.get_selected(), day(2, 5));
}

#[test]
fn date_picker_popup_browses_and_commits_against_live_bounds() {
    let mut ui = CalendarDemo::new();
    let mut input = Interaction::default();
    let day = |month, day| Date::new(2024, month, day).unwrap();
    let calendar = [3, 0];
    let prepare = |ui: &CalendarDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(440.0, 460.0), &mut FixedText);
        (scene, layout)
    };
    let (scene, layout) = prepare(&ui);
    assert!(scene.at_path(&[3]).unwrap().children.is_empty());
    input.accessibility_focus(input.accessibility_target(&scene, &layout, &[2]).unwrap());
    assert!(ui.dispatch_at(&scene, &[2], "clicked"));
    let (first, layout) = prepare(&ui);
    input.reconcile(&first, &layout);
    assert!(input.is_focused(&calendar));
    let browse = input
        .calendar_key(&first, &layout, NavigationKey::PageDown)
        .unwrap();
    assert!(ui.control_at(&first, &browse.path, browse.action));
    let (scene, layout) = prepare(&ui);
    let node = scene.at_path(&calendar).unwrap();
    assert_eq!(node.date("display-month"), Some(day(3, 1)));
    assert_eq!(node.date("date"), Some(day(2, 28)));
    assert_eq!(ui.get_month(), day(2, 1));
    let selection = CalendarAction::Select(day(3, 12));
    let rect = CalendarGeometry::new(node, layout.at_path(&calendar).unwrap().bounds)
        .action_bounds(selection)
        .unwrap();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: rect.x + rect.width * 0.5,
            y: rect.y + rect.height * 0.5,
        },
    );
    input.pointer_press();
    let action = input.control_part_release(&scene, &layout).unwrap();
    assert_eq!(action.action, ControlAction::Calendar(selection));
    assert!(ui.control_at(&scene, &action.path, action.action));
    let (draft, layout) = prepare(&ui);
    assert_eq!(
        draft.at_path(&calendar).unwrap().date("date"),
        Some(day(3, 12))
    );
    assert_eq!(ui.get_selected(), day(2, 28));
    let escape = input.dialog_escape(&draft, &layout).unwrap();
    assert!(ui.control_at(&draft, &escape.path, escape.action));
    let (scene, layout) = prepare(&ui);
    input.reconcile(&scene, &layout);
    assert!(input.is_focused(&[2]));
    assert!(!ui.get_picker_open());
    assert_eq!(ui.get_cancels(), 1);
    assert_eq!(ui.get_selected(), day(2, 28));

    ui.set_selected(day(3, 8));
    ui.set_picker_open(true);
    let (scene, layout) = prepare(&ui);
    input.reconcile(&scene, &layout);
    assert_eq!(
        scene.at_path(&calendar).unwrap().date("date"),
        Some(day(3, 8))
    );
    let accept = input.dialog_accept(&scene, &layout).unwrap();
    assert!(!ui.control_at(&draft, &accept.path, accept.action));
    assert!(!ui.control_at(&first, &calendar, browse.action));
    assert!(ui.control_at(&scene, &calendar, ControlAction::Calendar(selection)));
    let (scene, _) = prepare(&ui);
    ui.set_latest(day(3, 5));
    assert!(!ui.control_at(
        &scene,
        &calendar,
        ControlAction::Calendar(CalendarAction::Select(day(3, 4)))
    ));
    assert!(ui.control_at(&scene, &accept.path, accept.action));
    assert_eq!(ui.get_selected(), day(3, 5));
    assert_eq!(ui.get_month(), day(3, 5));
    assert_eq!(ui.get_accepts(), 1);
    assert!(!ui.get_picker_open());
}

#[test]
fn time_picker_keeps_seconds_and_handles_midnight_noon_and_format_changes() {
    let mut ui = TimeDemo::new();
    let mut input = Interaction::default();
    let hour = [1, 0];
    let period = [1, 3];
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, Size::new(520.0, 372.0), &mut FixedText);
    input.accessibility_focus(input.accessibility_target(&scene, &layout, &hour).unwrap());
    let next = input
        .tumbler_key(&scene, &layout, NavigationKey::Down)
        .unwrap();
    assert!(ui.control_at(&scene, &next.path, next.action));
    assert_eq!(ui.get_selected(), Time::new(0, 58, 41).unwrap());
    assert!(!ui.control_at(&scene, &next.path, next.action));

    let old = ui.render_shared();
    let old_action = xrack_runtime::Tumbler::from_node(old.at_path(&hour).unwrap())
        .select(5)
        .unwrap();
    ui.set_use_24(false);
    assert!(!ui.control_at(&old, &hour, old_action));
    let scene = ui.render_shared();
    let wheel = xrack_runtime::Tumbler::from_node(scene.at_path(&hour).unwrap());
    assert_eq!(
        wheel.label(wheel.selected().unwrap()).as_deref(),
        Some("12")
    );
    let wheel = xrack_runtime::Tumbler::from_node(scene.at_path(&period).unwrap());
    assert!(ui.control_at(&scene, &period, wheel.select(1).unwrap()));
    assert_eq!(ui.get_selected(), Time::new(12, 58, 41).unwrap());
    assert_eq!(ui.get_changes(), 2);

    ui.set_seconds(true);
    ui.set_inline_enabled(false);
    let scene = ui.render_shared();
    let seconds = [1, 2];
    let wheel = xrack_runtime::Tumbler::from_node(scene.at_path(&seconds).unwrap());
    assert!(!ui.control_at(&scene, &seconds, wheel.select(23).unwrap()));
    ui.set_inline_enabled(true);
    let scene = ui.render_shared();
    let wheel = xrack_runtime::Tumbler::from_node(scene.at_path(&seconds).unwrap());
    assert!(ui.control_at(&scene, &seconds, wheel.select(23).unwrap()));
    assert_eq!(ui.get_selected(), Time::new(12, 58, 23).unwrap());
}

#[test]
fn time_popup_commits_once_cancels_and_ignores_actions_from_a_previous_session() {
    let mut ui = TimeDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(520.0, 372.0);
    let dialog = [5];
    let minute = [5, 0, 1];
    let original = ui.get_selected();
    let scene = ui.render_shared();
    assert!(scene.at_path(&dialog).unwrap().children.is_empty());
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    input.accessibility_focus(input.accessibility_target(&scene, &layout, &[2]).unwrap());
    assert!(ui.dispatch_at(&scene, &[2], "clicked"));
    let old = ui.render_shared();
    let layout = xrack_layout::layout(&old, viewport, &mut FixedText);
    input.reconcile(&old, &layout);
    assert!(input.is_focused(&[5, 0, 0]));
    let choose = |scene: &xrack_runtime::Node, index| {
        xrack_runtime::Tumbler::from_node(scene.at_path(&minute).unwrap())
            .select(index)
            .unwrap()
    };
    assert!(ui.control_at(&old, &minute, choose(&old, 5)));
    assert_eq!(ui.get_selected(), original);
    let draft = ui.render_shared();
    let layout = xrack_layout::layout(&draft, viewport, &mut FixedText);
    let escape = input.dialog_escape(&draft, &layout).unwrap();
    assert!(ui.control_at(&draft, &escape.path, escape.action));
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    input.reconcile(&scene, &layout);
    assert!(input.is_focused(&[2]));
    assert_eq!(ui.get_cancels(), 1);
    assert_eq!(ui.get_selected(), original);

    let initial = Time::new(7, 10, 29).unwrap();
    ui.set_selected(initial);
    ui.set_picker_open(true);
    let scene = ui.render_shared();
    assert_eq!(
        scene.at_path(&minute).unwrap().int("current-index"),
        Some(10)
    );
    assert!(!ui.control_at(&old, &minute, choose(&old, 15)));
    assert!(!ui.control_at(&old, &dialog, ControlAction::Dialog(DialogRole::Accept)));
    assert!(ui.control_at(&scene, &minute, choose(&scene, 42)));
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let bounds = layout.at_path(&[5, 1, 1]).unwrap().bounds;
    input.reconcile(&scene, &layout);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let action = input.dialog_button(&scene, &layout, &target).unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_selected(), Time::new(7, 42, 29).unwrap());
    assert_eq!(ui.get_accepted_minute(), 42);
    assert_eq!(ui.get_accepts(), 1);
    assert!(!ui.get_picker_open());
    assert!(!ui.control_at(&scene, &action.path, action.action));
}

#[test]
fn tab_bar_reveals_offscreen_tabs_and_shares_selection_with_swipe_view() {
    let mut driver = UiDriver::new(TabBarDemo::new(), FixedText, Size::new(536.0, 318.0));
    driver.tab();
    assert_eq!(driver.focused(), Some("overview-tab"));
    assert!(driver.group_key(NavigationKey::Right));
    assert_eq!(driver.component().get_page(), 3);
    assert_eq!(driver.focused(), Some("reports-tab"));
    assert!(driver.group_key(NavigationKey::End));
    assert_eq!(driver.component().get_page(), 5);
    assert_eq!(driver.focused(), Some("preferences-tab"));
    assert!(driver.can_hit("preferences-tab"));
    assert!(
        driver
            .input()
            .horizontal_scroll_offsets()
            .values()
            .any(|&offset| offset > 0.0)
    );
    let pages = &driver.node("workspace").children;
    assert!(pages[0].children.is_empty());
    assert!(!pages[5].children.is_empty());

    assert!(driver.group_key(NavigationKey::Home));
    assert_eq!(driver.component().get_page(), 0);
    driver.click("reports-tab");
    assert_eq!(driver.component().get_page(), 3);
    assert_eq!(driver.component().get_changes(), 4);

    driver.tab();
    assert_eq!(driver.focused(), Some("workspace"));
    driver.tab();
    assert!(driver.group_key(NavigationKey::End));
    assert_eq!(driver.component().get_filter(), 2);
    assert_eq!(driver.component().get_page(), 3);
    assert_eq!(
        driver.node("filters-bar").children[2].text("text"),
        Some("收藏")
    );
}

#[test]
fn combo_completion_accepts_only_a_current_painted_candidate_and_undoes_atomically() {
    let mut ui = EditableChoiceDemo::new();
    ui.set_draft("研".into());
    let mut input = Interaction::default();
    let viewport = Size::new(470.0, 278.0);
    let (scene, layout) =
        prepare_scene_with_input(&ui, ui.render_shared(), viewport, &mut FixedText, &input);
    input.focus_next(&scene, &layout, false);
    input.sync_editor(&scene);
    let path = [0, 1];
    let candidate = |scene: &xrack_runtime::Node, input: &Interaction| {
        input.combo_completion(scene.at_path(&path).unwrap(), &path)
    };
    let first = candidate(&scene, &input).unwrap();
    assert_eq!(first.suffix(), "发");
    let now = Instant::now();
    assert!(!input.accept_combo_completion(&mut ui, &scene, now));
    input.set_painted_combo_completion(Some(first));
    ui.bind_model_workspaces(xrack_runtime::VecModel::from(vec![
        "研发预览".into(),
        "总部".into(),
    ]));
    let scene = ui.render_shared();
    assert!(!input.accept_combo_completion(&mut ui, &scene, now));
    assert_eq!(ui.get_draft(), "研");
    assert_eq!(ui.get_edits(), 0);

    let next = candidate(&scene, &input).unwrap();
    assert_eq!(next.suffix(), "发预览");
    input.set_painted_combo_completion(Some(next.clone()));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("发".into(), None);
    assert!(candidate(&scene, &input).is_none());
    assert!(!input.accept_combo_completion(&mut ui, &scene, now));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit(String::new(), None);

    input.set_painted_combo_completion(Some(next.clone()));
    assert!(input.accept_combo_completion(&mut ui, &scene, now));
    assert_eq!(ui.get_draft(), "研发预览");
    assert_eq!(ui.get_active(), 0);
    assert_eq!(ui.get_edits(), 1);
    assert_eq!(ui.get_accepts(), 0);
    let scene = ui.render_shared();
    assert!(candidate(&scene, &input).is_none());
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_draft(), "研");

    let scene = ui.render_shared();
    input.set_painted_combo_completion(Some(next));
    assert!(input.dismiss_combo_completion(&scene));
    assert!(candidate(&scene, &input).is_none());
    assert_eq!(ui.get_draft(), "研");
}

#[test]
fn editable_combo_box_separates_draft_selection_and_acceptance() {
    use xrack_layout::ComboGeometry;
    use xrack_runtime::{combo_index, text_input_value};

    let mut ui = EditableChoiceDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(470.0, 278.0);
    let render = |ui: &EditableChoiceDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let linked = [0, 1];
    let automatic = [0, 3];
    let (scene, layout) = render(&ui, &input);
    let geometry = ComboGeometry::new(layout.at_path(&linked).unwrap().bounds);
    let center = |rect: Rect| Point {
        x: rect.x + rect.width * 0.5,
        y: rect.y + rect.height * 0.5,
    };

    input.pointer_move(&scene, &layout, center(geometry.entry));
    input.pointer_press();
    input.pointer_move(&scene, &layout, center(geometry.trigger));
    let target = input.pointer_release_target().unwrap();
    assert!(!input.open_choice_at_pointer(&scene, &layout, target));
    assert_eq!(input.focused.as_ref().unwrap().path, linked);
    let now = Instant::now();
    input.edit_text(
        &mut ui,
        &scene,
        EditAction::SelectAll,
        EditSource::Keyboard,
        now,
    );
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("研发"),
        EditSource::Keyboard,
        now
    ));
    assert_eq!(ui.get_active(), 0);
    assert_eq!(ui.get_draft(), "研发");
    let (scene, _) = render(&ui, &input);
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_draft(), "总部");
    let (scene, layout) = render(&ui, &input);
    assert!(input.replay_text_edit(&mut ui, &scene, true));
    let (scene, _) = render(&ui, &input);
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("预览".into(), None);
    assert!(
        input
            .choice_key(&scene, &layout, ChoiceKey::Accept)
            .is_none()
    );
    assert_eq!(ui.get_accepts(), 0);
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("预览"),
        EditSource::Atomic,
        now
    ));
    let (scene, layout) = render(&ui, &input);
    let accept = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert_eq!(accept.action, ControlAction::AcceptComboText);
    assert!(ui.control_at(&scene, &accept.path, accept.action));
    assert_eq!(ui.get_active(), 3);
    assert_eq!(ui.get_last_accepted(), "研发预览");
    assert_eq!(ui.get_accepts(), 1);
    assert_eq!(ui.get_selections(), 0);

    ui.set_draft("研".into());
    let (scene, layout) = render(&ui, &input);
    input.pointer_move(
        &scene,
        &layout,
        center(ComboGeometry::new(layout.at_path(&linked).unwrap().bounds).trigger),
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert!(input.open_choice_at_pointer(&scene, &layout, target));
    assert_eq!(
        input.choice_menu(&scene, &layout).unwrap().highlighted,
        Some(2)
    );
    let choose = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert!(ui.control_at(&scene, &choose.path, choose.action));
    assert_eq!(ui.get_draft(), "研发");
    assert_eq!(ui.get_active(), 2);
    assert_eq!(ui.get_selections(), 1);

    ui.set_active_value("设计".into());
    let (scene, layout) = render(&ui, &input);
    assert_eq!(text_input_value(scene.at_path(&automatic).unwrap()), "设计");
    input.pointer_move(
        &scene,
        &layout,
        center(ComboGeometry::new(layout.at_path(&automatic).unwrap().bounds).entry),
    );
    input.pointer_press();
    input.pointer_release_target();
    input.edit_text(
        &mut ui,
        &scene,
        EditAction::SelectAll,
        EditSource::Keyboard,
        now,
    );
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("临时 工作区"),
        EditSource::Atomic,
        now
    ));
    ui.set_active_value("研发".into());
    ui.bind_model_workspaces(xrack_runtime::VecModel::from(vec![
        "研发".into(),
        "设计".into(),
    ]));
    let (scene, layout) = render(&ui, &input);
    assert_eq!(
        text_input_value(scene.at_path(&automatic).unwrap()),
        "临时 工作区"
    );
    let accept = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert!(ui.control_at(&scene, &accept.path, accept.action));
    assert_eq!(ui.get_active_value(), "临时 工作区");
    let scene = ui.render_shared();
    assert_eq!(combo_index(scene.at_path(&automatic).unwrap()), None);
    assert_eq!(
        text_input_value(scene.at_path(&automatic).unwrap()),
        "临时 工作区"
    );
    assert!(ui.control_at(&scene, &automatic, ControlAction::Select(0)));
    ui.set_active_value("设计".into());
    let scene = ui.render_shared();
    assert_eq!(text_input_value(scene.at_path(&automatic).unwrap()), "设计");
}

impl TextMeasurer for FixedText {
    fn measure(&mut self, text: &str, font_size: f32, width: f32) -> Size {
        Size::new(
            (text.chars().count() as f32 * font_size * 0.55).min(width),
            font_size * 1.4,
        )
    }
}

#[test]
fn tooltip_delay_timeout_and_edge_placement_use_the_same_layout() {
    let mut ui = TooltipDemo::new();
    let viewport = Size::new(510.0, 260.0);
    let render = |ui: &TooltipDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    assert!(!layout.at_path(&[1, 0]).unwrap().visible);
    let now = Instant::now();
    let tick = |ms| now + Duration::from_millis(ms);
    input.pointer_move(&scene, &layout, Point { x: 35.0, y: 75.0 });
    assert!(!input.update_tooltip(&scene, &layout, now));
    assert_eq!(input.tooltip_deadline(), Some(tick(500)));
    input.pointer_move(&scene, &layout, Point { x: 40.0, y: 80.0 });
    assert!(!input.update_tooltip(&scene, &layout, tick(499)));
    assert_eq!(input.tooltip_deadline(), Some(tick(500)));
    assert!(input.update_tooltip(&scene, &layout, tick(500)));
    assert_eq!(input.tooltip_deadline(), Some(tick(2000)));
    assert_eq!(
        input
            .tooltip_popup(&scene, &layout)
            .unwrap()
            .node
            .text("text"),
        Some("等待后显示的操作说明")
    );

    ui.set_note("实时更新".into());
    let (scene, layout) = render(&ui, &input);
    assert!(!input.update_tooltip(&scene, &layout, tick(600)));
    assert_eq!(
        input
            .tooltip_popup(&scene, &layout)
            .unwrap()
            .node
            .text("text"),
        Some("实时更新")
    );
    assert!(input.update_tooltip(&scene, &layout, tick(2000)));
    assert!(input.tooltip_popup(&scene, &layout).is_none());
    assert!(!input.update_tooltip(&scene, &layout, tick(4000)));
    assert_eq!(input.tooltip_deadline(), None);

    let point = Point { x: 480.0, y: 225.0 };
    input.pointer_move(&scene, &layout, point);
    assert!(!input.update_tooltip(&scene, &layout, tick(4100)));
    assert!(input.update_tooltip(&scene, &layout, tick(4400)));
    let popup = input.tooltip_popup(&scene, &layout).unwrap();
    assert_eq!(popup.node.children[0].kind, "VerticalLayout");
    let viewport = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let (surface, popup_layout) = layout_tooltip(popup, viewport, &mut FixedText);
    assert_eq!(
        surface.children[0].children[0].text("text"),
        Some("预览信息")
    );
    assert!(popup_layout.bounds.width > 0.0 && popup_layout.bounds.height > 0.0);
    assert_eq!(popup_layout.bounds.intersect(viewport), popup_layout.bounds);
    assert!(popup_layout.bounds.y + popup_layout.bounds.height < point.y);
    assert!(input.pointer_leave());
    assert!(input.tooltip_popup(&scene, &layout).is_none());
}

#[test]
fn tooltip_focus_and_modal_scope_keep_background_hints_hidden() {
    let mut ui = TooltipDemo::new();
    let viewport = Size::new(510.0, 260.0);
    let render = |ui: &TooltipDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    let now = Instant::now();
    let tick = |ms| now + Duration::from_millis(ms);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("action"));
    input.update_tooltip(&scene, &layout, now);
    assert!(input.update_tooltip(&scene, &layout, tick(500)));
    assert!(input.tooltip_popup(&scene, &layout).is_some());
    assert!(input.dismiss_tooltip());
    assert!(!input.update_tooltip(&scene, &layout, tick(501)));
    assert!(input.tooltip_popup(&scene, &layout).is_none());

    input.pointer_move(&scene, &layout, Point { x: 35.0, y: 75.0 });
    input.update_tooltip(&scene, &layout, tick(600));
    assert!(input.update_tooltip(&scene, &layout, tick(1100)));
    ui.set_dialog_open(true);
    let (scene, layout) = render(&ui, &input);
    assert!(input.tooltip_popup(&scene, &layout).is_none());
    input.reconcile(&scene, &layout);
    assert!(input.update_tooltip(&scene, &layout, tick(1101)));
    assert_eq!(input.tooltip_deadline(), None);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("done"));
    input.update_tooltip(&scene, &layout, tick(1200));
    assert!(input.update_tooltip(&scene, &layout, tick(1300)));
    assert_eq!(
        input
            .tooltip_popup(&scene, &layout)
            .unwrap()
            .node
            .text("text"),
        Some("关闭当前对话框")
    );
}

#[test]
fn tooltip_timer_restarts_when_a_virtual_row_slot_changes_identity() {
    use xrack_runtime::{Node, VecModel};
    let model = VecModel::from(vec!["first", "second"]);
    let mut scene = Node::new("Window", None);
    let mut row = Node::new("ListRow", None);
    row.row_key = model.row_key(0);
    let mut tip = Node::new("Tooltip", None);
    tip.properties.insert("text", "first".to_owned().into());
    row.children.push(tip);
    scene.children.push(row);
    let layout = xrack_layout::layout(&scene, Size::new(220.0, 160.0), &mut FixedText);
    let mut input = Interaction::default();
    input.pointer_move(&scene, &layout, Point { x: 24.0, y: 24.0 });
    let now = Instant::now();
    input.update_tooltip(&scene, &layout, now);
    assert!(input.update_tooltip(&scene, &layout, now + Duration::from_millis(600)));
    scene.children[0].row_key = model.row_key(1);
    scene.children[0].children[0]
        .properties
        .insert("text", "second".to_owned().into());
    assert!(input.tooltip_popup(&scene, &layout).is_none());
    assert!(input.update_tooltip(&scene, &layout, now + Duration::from_millis(601)));
    assert!(input.tooltip_popup(&scene, &layout).is_none());
    assert!(input.update_tooltip(&scene, &layout, now + Duration::from_millis(1201)));
    assert_eq!(
        input
            .tooltip_popup(&scene, &layout)
            .unwrap()
            .node
            .text("text"),
        Some("second")
    );
}

#[test]
fn tree_keyboard_and_disclosures_preserve_expansion_across_reorder() {
    let mut ui = TreeDemo::new();
    ui.set_multiple(false);
    let viewport = Size::new(600.0, 370.0);
    let path = [0, 1];
    let mut input = Interaction::default();
    let render = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let keys = |scene: &xrack_runtime::Node| {
        scene
            .at_path(&path)
            .unwrap()
            .tree
            .as_ref()
            .unwrap()
            .rows()
            .iter()
            .map(|row| row.key)
            .collect::<Vec<_>>()
    };
    let (scene, layout) = render(&ui, &input);
    assert_eq!(keys(&scene), [1, 5]);
    assert!(scene.at_path(&path).unwrap().tree_selection.is_none());
    input.focus_next(&scene, &layout, false);
    let first = input
        .list_key(&scene, &layout, NavigationKey::Home)
        .unwrap();
    assert!(ui.control_at(&scene, &first.path, first.action));
    for expected in [1, 2, 2, 3] {
        let (scene, layout) = render(&ui, &input);
        let action = input
            .tree_key(&scene, &layout, NavigationKey::Right)
            .unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));
        assert_eq!(ui.get_current(), expected);
    }
    assert_eq!(ui.get_expansions(), 2);
    let model = ui.model_records();
    assert!(model.sort_by(|a, b| b.id.cmp(&a.id)));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(keys(&scene), [5, 1, 4, 2, 3]);
    assert_eq!(ui.get_current(), 3);

    let row_path = [0, 1, 1];
    let row = scene.at_path(&row_path).unwrap();
    let item = layout.at_path(&row_path).unwrap();
    let geometry = TreeRowGeometry::new(row, item.bounds).unwrap();
    let disclosure = Point {
        x: geometry.indicator.x + 10.0,
        y: item.bounds.y + 10.0,
    };
    input.pointer_move(&scene, &layout, disclosure);
    input.pointer_press();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: geometry.content.x + 10.0,
            y: disclosure.y,
        },
    );
    assert!(input.pointer_release_target().is_none());
    input.pointer_move(&scene, &layout, disclosure);
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let collapse = input.tree_pointer_action(&scene, &layout, &target).unwrap();
    assert!(ui.control_at(&scene, &collapse.path, collapse.action));
    assert_eq!(ui.get_current(), 1);
    assert_eq!(ui.get_collapses(), 1);
    let (scene, layout) = render(&ui, &input);
    assert_eq!(keys(&scene), [5, 1]);
    let expand = input.tree_toggle(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &expand.path, expand.action));
    let (stale, _) = render(&ui, &input);
    assert_eq!(keys(&stale), [5, 1, 4, 2, 3]);
    ui.set_records(model.snapshot());
    assert!(!ui.control_at(&stale, &collapse.path, collapse.action));
    assert_eq!(keys(&render(&ui, &input).0), [5, 1]);
}

#[test]
fn tree_multi_selection_tracks_folded_and_reordered_rows() {
    let mut ui = TreeDemo::new();
    let path = [0, 1];
    let mut input = Interaction::default();
    let render = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(600.0, 370.0),
            &mut FixedText,
            input,
        )
    };
    let keys = |scene: &xrack_runtime::Node| {
        scene
            .at_path(&path)
            .unwrap()
            .tree_selection
            .as_ref()
            .unwrap()
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    let expand = |ui: &mut TreeDemo, input: &Interaction, key, expanded| {
        let (scene, _) = render(ui, input);
        let tree = scene.at_path(&path).unwrap().tree.as_ref().unwrap();
        let item = tree.rows()[tree.visible_index(key).unwrap()];
        assert!(ui.control_at(
            &scene,
            &path,
            ControlAction::Tree(TreeAction::SetExpanded {
                key,
                row: item.row,
                expanded,
            })
        ));
    };
    expand(&mut ui, &input, 1, true);
    expand(&mut ui, &input, 2, true);
    let (scene, layout) = render(&ui, &input);
    input.focus_next(&scene, &layout, false);
    for (key, operation) in [
        (NavigationKey::Home, RowSelection::Replace),
        (NavigationKey::Down, RowSelection::Extend),
        (NavigationKey::Down, RowSelection::Extend),
        (NavigationKey::End, RowSelection::Move),
    ] {
        let (scene, layout) = render(&ui, &input);
        let action = input
            .list_key_with_selection(&scene, &layout, key, operation)
            .unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    assert_eq!(ui.get_current(), 5);
    assert_eq!(ui.get_selected_count(), 3);
    let (scene, layout) = render(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3]);
    let cell = layout.at_path(&[0, 1, 3, 0, 0, 0]).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 12.0,
            y: cell.y + 12.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let toggle = input
        .select_group_option_with_selection(&scene, &layout, &target, RowSelection::Toggle)
        .unwrap();
    assert!(ui.control_at(&scene, &toggle.path, toggle.action));
    assert_eq!(ui.get_current(), 4);
    assert_eq!(ui.get_selected_count(), 4);
    let before_fold = render(&ui, &input).0;

    expand(&mut ui, &input, 1, false);
    assert_eq!(ui.get_current(), 1);
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(keys(&scene), [1, 2, 3, 4]);
    let view = scene.at_path(&path).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        before_fold
            .at_path(&path)
            .unwrap()
            .tree_selection
            .as_ref()
            .unwrap(),
        view.tree_selection.as_ref().unwrap(),
    ));
    assert!(view.children[0].boolean("selected", false));
    assert!(!view.children[1].boolean("selected", false));
    let model = ui.model_records();
    assert!(model.sort_by(|a, b| b.id.cmp(&a.id)));
    expand(&mut ui, &input, 1, true);
    let (scene, layout) = render(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3, 4]);
    let all = input
        .tree_selection_key(&scene, &layout, TableSelectionKey::All)
        .unwrap();
    assert!(ui.control_at(&scene, &all.path, all.action));
    let (scene, layout) = render(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3, 4, 5]);
    let clear = input
        .tree_selection_key(&scene, &layout, TableSelectionKey::Clear)
        .unwrap();
    assert!(ui.control_at(&scene, &clear.path, clear.action));
    assert_eq!(ui.get_current(), 1);
    assert_eq!(ui.get_selected_count(), 0);

    let branch = scene.at_path(&path).unwrap().tree.as_ref().unwrap().rows()[1];
    let replace = ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Row {
        key: branch.key,
        row: branch.row,
        operation: RowSelection::Replace,
    }));
    assert!(ui.control_at(&scene, &path, replace));
    assert_eq!(keys(&render(&ui, &input).0), [1]);
    let removed = model.remove(branch.model_index).unwrap();
    assert!(keys(&render(&ui, &input).0).is_empty());
    let mut replacement = model.snapshot();
    replacement.push(removed);
    ui.set_records(replacement);
    assert!(!ui.control_at(&scene, &path, replace));
    assert!(!ui.control_at(&scene, &all.path, all.action));
    assert!(keys(&render(&ui, &input).0).is_empty());
}

#[test]
fn tree_rename_follows_identity_and_checks_the_original_field() {
    let mut ui = TreeDemo::new();
    let path = [0, 1];
    let mut input = Interaction::default();
    let render = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(600.0, 370.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, _) = render(&ui, &input);
    let root = scene.at_path(&path).unwrap().tree.as_ref().unwrap().rows()[0];
    let expansion = |expanded| {
        ControlAction::Tree(TreeAction::SetExpanded {
            key: root.key,
            row: root.row,
            expanded,
        })
    };
    assert!(ui.control_at(&scene, &path, expansion(true)));
    let (scene, layout) = render(&ui, &input);
    let cell_path = [0, 1, 1, 0, 0, 0];
    let cell = layout.at_path(&cell_path).unwrap().bounds;
    let now = Instant::now();
    for elapsed in [0, 100] {
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: cell.x + 12.0,
                y: cell.y + 12.0,
            },
        );
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.path, cell_path);
        assert_eq!(
            input.table_cell_click(
                &scene,
                &layout,
                Some(&target),
                now + Duration::from_millis(elapsed)
            ),
            elapsed != 0
        );
    }
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("新名称"),
        EditSource::Atomic,
        now
    ));
    let model = ui.model_records();
    let identity = model.row_key(1);
    let mut external = model.row(1).unwrap();
    assert_eq!(external.label, "源码");
    external.detail = "保留外部修改".into();
    assert!(model.set_row(1, external));
    assert!(model.sort_by(|a, b| b.id.cmp(&a.id)));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let moved = input.focused.as_ref().unwrap();
    assert_ne!(moved.path, cell_path);
    assert_eq!(moved.row_key, identity);
    assert!(input.table_cell_editing(&moved.path));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(4).unwrap().label, "新名称");
    assert_eq!(model.row(4).unwrap().detail, "保留外部修改");
    assert_eq!(model.row(3).unwrap().label, "main.rs");

    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("过期草稿"),
        EditSource::Atomic,
        now
    ));
    let mut external = model.row(4).unwrap();
    external.label = "外部重命名".into();
    assert!(model.set_row(4, external));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(4).unwrap().label, "外部重命名");

    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(ui.control_at(&scene, &path, expansion(false)));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.editor.is_none());
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(input.focused.as_ref().unwrap().path, path);
}

#[test]
fn nested_tree_editors_follow_the_current_row_and_commit_independent_fields() {
    let mut ui = TreeDemo::new();
    let mut input = Interaction::default();
    let path = [0, 1];
    let render = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(600.0, 370.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, _) = render(&ui, &input);
    let root = scene.at_path(&path).unwrap().tree.as_ref().unwrap().rows()[0];
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::Tree(TreeAction::SetExpanded {
            key: root.key,
            row: root.row,
            expanded: true,
        })
    ));
    let (scene, _) = render(&ui, &input);
    assert!(ui.control_at(&scene, &path, ControlAction::Select(1)));
    assert_eq!(ui.get_current(), 2);
    let (old_scene, layout) = render(&ui, &input);
    let old_spin = [0, 1, 1, 0, 0, 3];
    input.focus_next(&old_scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, path);
    input.focus_next(&old_scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, [0, 1, 1, 0, 0, 2]);
    assert!(input.tree_toggle(&old_scene, &layout).is_none());
    input.focus_next(&old_scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, old_spin);
    let node = old_scene.at_path(&old_spin).unwrap();
    input.edit_spin(node, EditAction::SelectAll);
    input.edit_spin(node, EditAction::Insert("17"));

    let model = ui.model_records();
    let key = model.row_key(1);
    assert!(model.set_row(
        1,
        TreeRecord {
            detail: String::new(),
            ..model.row(1).unwrap()
        }
    ));
    assert!(model.sort_by(|left, right| right.id.cmp(&left.id)));
    assert!(!ui.control_at(
        &old_scene,
        &old_spin,
        ControlAction::Spin(SpinAction::Increase)
    ));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let spin = [0, 1, 3, 0, 0, 3];
    assert!(!scene.at_path(&[0, 1, 3, 0, 0, 1]).unwrap().is_visible());
    assert_eq!(input.focused.as_ref().unwrap().path, spin);
    assert_eq!(input.focused.as_ref().unwrap().row_key, key);
    assert_eq!(input.editor_value(scene.at_path(&spin).unwrap()), "17");
    let draft = input.commit_spin_draft().unwrap();
    assert!(ui.control_at(&scene, &draft.path, draft.action));
    assert_eq!(model.row(4).unwrap().quota, 17);
    assert!(model.row(4).unwrap().detail.is_empty());

    let (scene, layout) = render(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let slider = [0, 1, 3, 0, 0, 4];
    assert_eq!(input.focused.as_ref().unwrap().path, slider);
    let step = input
        .slider_key(&scene, &layout, SliderKey::Right, true)
        .unwrap();
    assert!(ui.adjust_range_at(&scene, &step.path, step.action));
    assert!((model.row(4).unwrap().load - 0.75).abs() < 1e-9);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, [0, 2]);

    let (scene, layout) = render(&ui, &input);
    let checkbox = [0, 1, 3, 0, 0, 2];
    let bounds = layout.at_path(&checkbox).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + 12.0,
            y: bounds.y + 12.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, checkbox);
    assert!(
        input
            .select_group_option(&scene, &layout, &target)
            .is_none()
    );
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(!ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(!model.row(4).unwrap().active);
    assert_eq!(ui.get_current(), 2);
    let (scene, _) = render(&ui, &input);
    assert!(!ui.adjust_range_at(&scene, &slider, RangeAction::SetFraction(1.0)));
    ui.set_records(model.snapshot());
    assert!(!ui.control_at(&scene, &spin, ControlAction::Spin(SpinAction::Increase)));
    assert_eq!(ui.model_records().row(4).unwrap().quota, 17);
}

#[test]
fn tree_materializes_only_rows_in_the_viewport_after_expansion() {
    let mut ui = TreeDemo::new();
    let records = (0..=10_000)
        .map(|id| TreeRecord {
            id,
            parent_id: if id == 0 { -1 } else { 0 },
            label: format!("Row {id}"),
            detail: String::new(),
            active: true,
            quota: 0,
            load: 0.0,
        })
        .collect();
    ui.set_records(records);
    let path = [0, 1];
    let viewport = Size::new(600.0, 370.0);
    let mut input = Interaction::default();
    let render = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, _) = render(&ui, &input);
    let tree = scene.at_path(&path).unwrap();
    let root = tree.tree.as_ref().unwrap().rows()[0];
    assert_eq!(tree.children.len(), 1);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::Tree(TreeAction::SetExpanded {
            key: root.key,
            row: root.row,
            expanded: true
        })
    ));
    let cache = Property::default();
    let prepared = || cache.get(|| render(&ui, &input));
    let (before, _) = prepared();
    let tree = before.at_path(&path).unwrap();
    assert_eq!(tree.tree.as_ref().unwrap().rows().len(), 10_001);
    assert!(tree.children.len() < 12);

    let base = ui.render_shared();
    let model = ui.model_records();
    let mut offscreen = model.row(9_999).unwrap();
    offscreen.label = "视口外的更新".into();
    assert!(model.set_row(9_999, offscreen.clone()));
    assert!(std::rc::Rc::ptr_eq(&base, &ui.render_shared()));
    assert!(std::rc::Rc::ptr_eq(&before, &prepared().0));

    let mut visible = model.row(1).unwrap();
    visible.label = "视口内的更新".into();
    assert!(model.set_row(1, visible));
    let (updated, _) = prepared();
    assert!(std::rc::Rc::ptr_eq(&base, &ui.render_shared()));
    assert!(!std::rc::Rc::ptr_eq(&before, &updated));
    assert_eq!(
        updated.at_path(&[0, 1, 1, 0, 0, 0]).unwrap().text("text"),
        Some("视口内的更新"),
    );

    offscreen.parent_id = 10_000;
    assert!(model.set_row(9_999, offscreen));
    assert!(!std::rc::Rc::ptr_eq(&base, &ui.render_shared()));
    let (scene, layout) = prepared();
    let tree = scene.at_path(&path).unwrap().tree.as_ref().unwrap();
    assert!(tree.is_descendant(9_999, 10_000));
    assert_eq!(tree.visible_index(9_999), None);
    input.focus_next(&scene, &layout, false);
    let bar = layout
        .at_path(&path)
        .unwrap()
        .scroll_bars(scene.at_path(&path).unwrap())
        .next()
        .unwrap();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bar.thumb.x + 4.0,
            y: bar.thumb.y + 6.0,
        },
    );
    assert_eq!(input.scroll_bar_press(&scene, &layout), Some(false));
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bar.track.x + 40.0,
            y: bar.track.y + bar.track.height + 20.0,
        },
    );
    assert_eq!(input.scroll_bar_drag(&scene, &layout), Some(true));
    assert!(input.finish_scroll_bar());
    let (scene, layout) = render(&ui, &input);
    let tree = scene.at_path(&path).unwrap();
    assert!(tree.list.unwrap().first_row > 9_980);
    assert!(tree.children.len() < 12);
    assert_eq!(ui.get_current(), -1);
    let end = input.list_key(&scene, &layout, NavigationKey::End).unwrap();
    assert!(ui.control_at(&scene, &end.path, end.action));
    let (scene, _) = render(&ui, &input);
    let tree = scene.at_path(&path).unwrap();
    assert_eq!(ui.get_current(), 10_000);
    assert!(tree.children.len() < 12);
    assert!(tree.children.last().unwrap().boolean("selected", false));
}

#[test]
fn split_drag_resizes_adjacent_panes_and_persists_proportions() {
    let mut ui = SplitDemo::new();
    let viewport = Size::new(760.0, 420.0);
    let work = [0, 2];
    let mut input = Interaction::default();
    let render = |ui: &SplitDemo, viewport: Size, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let widths = |layout: &xrack_layout::LayoutNode| {
        let item = layout.at_path(&work).unwrap();
        [
            item.children[0].bounds.width,
            item.children[1].bounds.width,
            item.children[2].bounds.width,
        ]
    };
    let (scene, layout) = render(&ui, viewport, &input);
    let before = widths(&layout);
    let handle = layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .handles[0]
        .bounds;
    let point = Point {
        x: handle.x + 2.0,
        y: handle.y + 10.0,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    input.split_press(&scene, &layout);
    assert_eq!(
        input.split_orientation(&scene),
        Some(Orientation::Horizontal)
    );
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: point.x + 40.0,
            y: -10.0,
        },
    );
    let drag = input.split_drag(&scene, &layout).unwrap();
    assert_eq!(drag.path, work);
    assert!(ui.resize_split_at(&scene, &drag.path, &drag.weights));
    assert_eq!(ui.get_edits(), 1);
    assert!(input.finish_split_resize());
    let (scene, layout) = render(&ui, viewport, &input);
    let after = widths(&layout);
    for (actual, expected) in after
        .into_iter()
        .zip([before[0] + 40.0, before[1] - 40.0, before[2]])
    {
        assert!((actual - expected).abs() < 0.05, "{actual} != {expected}");
    }
    let (_, wide) = render(&ui, Size::new(840.0, 420.0), &input);
    let wide = widths(&wide);
    let ratio = after[0] / after.iter().sum::<f32>();
    assert!((wide[0] / wide.iter().sum::<f32>() - ratio).abs() < 0.001);

    let handle = layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .handles[0]
        .bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: handle.x + 3.0,
            y: handle.y + 10.0,
        },
    );
    input.pointer_press();
    input.split_press(&scene, &layout);
    ui.set_show_preview(false);
    let (hidden, hidden_layout) = render(&ui, viewport, &input);
    input.reconcile(&hidden, &hidden_layout);
    assert!(input.split_drag(&hidden, &hidden_layout).is_none());
    assert!(!input.finish_split_resize());
    let hidden_view = hidden_layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap();
    assert_eq!(hidden_view.handles.len(), 1);
    assert_eq!(widths(&hidden_layout)[1], 0.0);
    ui.set_show_preview(true);
    let (_, restored) = render(&ui, viewport, &input);
    for (actual, expected) in widths(&restored).into_iter().zip(after) {
        assert!((actual - expected).abs() < 0.05);
    }

    let (_, cramped) = render(&ui, Size::new(220.0, 420.0), &input);
    let item = cramped.at_path(&work).unwrap();
    assert!(item.children.iter().all(
        |child| child.bounds.x + child.bounds.width <= item.bounds.x + item.bounds.width + 0.01
    ));
}

#[test]
fn split_handles_are_keyboard_focusable_in_pane_order_and_respect_limits() {
    let mut ui = SplitDemo::new();
    let viewport = Size::new(760.0, 420.0);
    let mut input = Interaction::default();
    let render = |ui: &SplitDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = render(&ui, &input);
    for path in [
        &[0, 1][..],
        &[0, 2, 3][..],
        &[0, 2, 4][..],
        &[0, 2, 2, 2][..],
    ] {
        input.focus_next(&scene, &layout, false);
        assert_eq!(input.focused.as_ref().unwrap().path, path);
    }
    assert!(
        input
            .split_key(&scene, &layout, NavigationKey::Right)
            .is_none()
    );
    let home = input
        .split_key(&scene, &layout, NavigationKey::Home)
        .unwrap();
    assert_eq!(home.path, [0, 2, 2]);
    assert!(ui.resize_split_at(&scene, &home.path, &home.weights));
    let (scene, layout) = render(&ui, &input);
    let detail = layout.at_path(&[0, 2, 2]).unwrap();
    assert!((detail.children[0].bounds.height - 70.0).abs() < 0.05);
    let end = input
        .split_key(&scene, &layout, NavigationKey::End)
        .unwrap();
    assert!(ui.resize_split_at(&scene, &end.path, &end.weights));
    let (_, layout) = render(&ui, &input);
    let detail = layout.at_path(&[0, 2, 2]).unwrap();
    assert!((detail.children[1].bounds.height - 56.0).abs() < 0.05);
    assert_eq!(ui.get_edits(), 2);
    ui.set_split_enabled(false);
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(
        input
            .split_key(&scene, &layout, NavigationKey::Up)
            .is_none()
    );
    assert!(!ui.resize_split_at(&scene, &end.path, &end.weights));
}

#[test]
fn standard_table_projects_records_and_guards_selection_after_sorting() {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..50_000)
            .map(|index| InventoryRow {
                name: format!("Device {index:05}"),
                units: 50_000 - index,
                available: index % 2 == 0,
                refreshed: Time {
                    hour: 9,
                    minute: 30,
                    second: 0,
                },
            })
            .collect(),
    );
    let path = [0, 1];
    let viewport = Size::new(620.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(table.list.unwrap().row_count, 50_000);
    assert!(table.children.len() <= 10);
    assert!(
        ui.render_shared()
            .at_path(&path)
            .unwrap()
            .children
            .is_empty()
    );
    let values: Vec<_> = table.children[0].children[0]
        .children
        .iter()
        .map(|cell| cell.text("text").unwrap())
        .collect();
    assert_eq!(values, ["Device 00000", "50000", "true", "09:30:00"]);
    let first = table.children[0].row_key;
    input.focus_next(&scene, &layout, false);
    assert!(input.is_focused(&path));
    let selected = input
        .list_key(&scene, &layout, NavigationKey::Home)
        .unwrap();
    assert!(ui.control_at(&scene, &selected.path, selected.action));
    assert_eq!(ui.get_selections(), 1);
    input.focus_next(&scene, &layout, false);
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("jump")
    );

    let model = ui.model_inventory();
    let rows = model.clone();
    ui.on_sort_requested(move |column, descending| {
        assert_eq!(column, 1);
        assert!(!descending);
        rows.sort_by(|a, b| a.units.cmp(&b.units));
    });
    let (scene, layout) = prepare(&ui, &input);
    let column = layout
        .at_path(&path)
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .columns[1];
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: column.x + column.width * 0.5,
            y: column.y + 10.0,
        },
    );
    input.pointer_press();
    let sort = input
        .table_header_release(&scene, RowSelection::Replace, false)
        .unwrap();
    assert!(ui.control_at(&scene, &sort.path, sort.action));
    assert_eq!(model.row_key(49_999), first);
    assert!(!ui.control_at(&scene, &path, ControlAction::Select(1)));

    ui.set_current(49_999);
    let (scene, _) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert!(table.list.unwrap().first_row >= 49_990);
    let last = table.children.last().unwrap();
    assert_eq!(last.row_key, first);
    assert!(last.boolean("selected", false));
    assert!(last.boolean("current", false));
    assert_eq!(ui.get_selections(), 1);
}

#[test]
fn table_sort_and_selection_use_visible_rows_with_sticky_aligned_columns() {
    let mut ui = TableDemo::new();
    ui.set_records(
        (0..10_000)
            .rev()
            .map(|row| format!("Record {row:05}"))
            .collect(),
    );
    let model = ui.model_records();
    let first_key = model.row_key(0);
    let records = model.clone();
    ui.on_sort_requested(move |column, descending| {
        assert_eq!(column, 0);
        records.sort_by(|a, b| if descending { b.cmp(a) } else { a.cmp(b) });
    });

    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene(
            ui,
            ui.render_shared(),
            viewport,
            &mut FixedText,
            input.scroll_offsets(),
        )
    };
    let path = [0, 2];
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    let item = layout.at_path(&path).unwrap();
    let geometry = TableGeometry::new(table, item.bounds).unwrap();
    assert_eq!(table.list.unwrap().row_count, 10_000);
    assert!((3..12).contains(&table.children.len()));
    assert!(
        ui.render_shared()
            .at_path(&path)
            .unwrap()
            .children
            .is_empty()
    );
    assert_eq!(item.children[0].bounds.y, geometry.body.y);
    for (cell, header) in item.children[0].children[0]
        .children
        .iter()
        .zip(&geometry.columns)
    {
        assert_eq!(cell.bounds.x, header.x);
        assert_eq!(cell.bounds.width, header.width);
    }
    let point = |column: usize| Point {
        x: geometry.columns[column].x + 10.0,
        y: geometry.columns[column].y + 12.0,
    };
    input.pointer_move(&scene, &layout, point(0));
    input.pointer_press();
    input.pointer_move(&scene, &layout, point(1));
    assert!(input.table_sort_release().is_none());
    input.pointer_press();
    assert!(input.table_sort_release().is_none());
    input.pointer_release_target();

    input.pointer_move(&scene, &layout, point(0));
    input.pointer_press();
    let sort = input.table_sort_release().unwrap();
    assert_eq!(sort.action, ControlAction::SortColumn(0));
    assert!(ui.control_at(&scene, &sort.path, sort.action));
    assert_eq!(model.row_key(9_999), first_key);
    let (scene, _) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(
        table.table_header.as_ref().unwrap().sort,
        Some(TableSort {
            column: 0,
            descending: false
        })
    );
    assert_eq!(
        table.children[0].children[0].children[0].text("text"),
        Some("Record 00000")
    );

    input.pointer_press();
    let sort = input.table_sort_release().unwrap();
    assert!(ui.control_at(&scene, &sort.path, sort.action));
    assert_eq!(model.row_key(0), first_key);
    let (scene, layout) = render(&ui, &input);
    assert!(
        scene
            .at_path(&path)
            .unwrap()
            .table_header
            .as_ref()
            .unwrap()
            .sort
            .unwrap()
            .descending
    );

    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: geometry.body.x + 10.0,
            y: geometry.body.y + 40.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let selection = input.select_group_option(&scene, &layout, &target).unwrap();
    assert_eq!(selection.action, ControlAction::Select(1));
    assert!(ui.control_at(&scene, &selection.path, selection.action));
    let (scene, layout) = render(&ui, &input);
    let action = input.list_key(&scene, &layout, NavigationKey::End).unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_current(), 9_999);
    assert_eq!(ui.get_selections(), 2);
    let (scene, layout) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert!(table.list.unwrap().first_row > 9_980);
    assert!(table.children.last().unwrap().boolean("selected", false));
    assert_eq!(
        layout.at_path(&path).unwrap().children[0].clip.y,
        geometry.body.y
    );
}

#[test]
fn table_multi_selection_follows_records_and_rejects_stale_viewports() {
    let mut ui = TableDemo::new();
    ui.set_records(["a", "b", "c", "d", "e", "f"].map(str::to_owned).to_vec());
    let model = ui.model_records();
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let indices = |ui: &TableDemo| {
        ui.render_shared()
            .at_path(&path)
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .indices(&model)
    };
    let mut input = Interaction::default();
    let click = |ui: &mut TableDemo, input: &mut Interaction, row: usize, operation| {
        let (scene, layout) = render(ui, input);
        input.reconcile(&scene, &layout);
        let item = layout.at_path(&path).unwrap();
        let bounds = item.children[row].bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bounds.x + 30.0,
                y: bounds.y + 10.0,
            },
        );
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        let action = input
            .select_group_option_with_selection(&scene, &layout, &target, operation)
            .unwrap();
        assert_eq!(
            action.action,
            ControlAction::TableSelection(TableSelectionAction::Row {
                index: row,
                operation
            })
        );
        assert!(ui.control_at(&scene, &action.path, action.action));
    };
    click(&mut ui, &mut input, 1, RowSelection::Replace);
    click(&mut ui, &mut input, 3, RowSelection::Toggle);
    assert_eq!(indices(&ui), [1, 3]);
    click(&mut ui, &mut input, 5, RowSelection::Extend);
    assert_eq!(indices(&ui), [3, 4, 5]);
    click(&mut ui, &mut input, 1, RowSelection::AddRange);
    assert_eq!(indices(&ui), [1, 2, 3, 4, 5]);
    click(&mut ui, &mut input, 2, RowSelection::Toggle);
    assert_eq!(indices(&ui), [1, 3, 4, 5]);
    assert_eq!(ui.get_selected_count(), 4);

    let stale = ui.render_shared();
    assert!(model.sort_by(|a, b| b.cmp(a)));
    assert_eq!(indices(&ui), [0, 1, 2, 4]);
    let replace = ControlAction::TableSelection(TableSelectionAction::Row {
        index: 0,
        operation: RowSelection::Replace,
    });
    assert!(!ui.control_at(&stale, &path, replace));
    assert_eq!(indices(&ui), [0, 1, 2, 4]);

    // The range anchor still identifies "c", now at index 3.
    click(&mut ui, &mut input, 2, RowSelection::Extend);
    assert_eq!(indices(&ui), [2, 3]);
    assert!(model.insert(0, "new".into()));
    assert_eq!(indices(&ui), [3, 4]);
    assert_eq!(model.remove(3).as_deref(), Some("d"));
    assert_eq!(indices(&ui), [3]);
    let (scene, _) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert!(table.children[3].boolean("selected", false));
    assert!(!table.children[2].boolean("selected", false));
    ui.set_records(vec!["c".into()]);
    assert!(
        ui.render_shared()
            .at_path(&path)
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .keys()
            .is_empty()
    );
}

#[test]
fn table_keyboard_selection_keeps_offscreen_rows_and_separates_current_row() {
    let mut ui = TableDemo::new();
    ui.set_records((0..10_000).map(|row| format!("Row {row}")).collect());
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let first = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Home, RowSelection::Replace)
        .unwrap();
    assert!(ui.control_at(&scene, &first.path, first.action));
    let (scene, layout) = render(&ui, &input);
    let range = input
        .list_key_with_selection(&scene, &layout, NavigationKey::End, RowSelection::Extend)
        .unwrap();
    assert!(ui.control_at(&scene, &range.path, range.action));
    let (scene, _) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(ui.get_current(), 9_999);
    assert_eq!(table.table_selection.as_ref().unwrap().keys().len(), 10_000);
    assert!(table.children.len() < 12);
    assert!(table.children.last().unwrap().boolean("current", false));

    let toggle = input
        .table_selection_key(&scene, TableSelectionKey::Toggle)
        .unwrap();
    assert!(ui.control_at(&scene, &toggle.path, toggle.action));
    assert_eq!(ui.get_current(), 9_999);
    assert_eq!(ui.get_selected_count(), 9_999);
    let (scene, layout) = render(&ui, &input);
    assert!(
        !scene
            .at_path(&path)
            .unwrap()
            .children
            .last()
            .unwrap()
            .boolean("selected", false)
    );
    let movement = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Home, RowSelection::Move)
        .unwrap();
    assert!(ui.control_at(&scene, &movement.path, movement.action));
    assert_eq!(ui.get_current(), 0);
    assert_eq!(ui.get_selected_count(), 9_999);
    let (scene, _) = render(&ui, &input);
    let select_all = input
        .table_selection_key(&scene, TableSelectionKey::All)
        .unwrap();
    assert!(ui.control_at(&scene, &select_all.path, select_all.action));
    assert_eq!(ui.get_selected_count(), 10_000);
    let model = ui.model_records();
    model.push("added later".into());
    let (scene, _) = render(&ui, &input);
    assert!(
        !scene
            .at_path(&path)
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .keys()
            .contains(&model.row_key(10_000).unwrap())
    );
    let clear = input
        .table_selection_key(&scene, TableSelectionKey::Clear)
        .unwrap();
    assert!(ui.control_at(&scene, &clear.path, clear.action));
    assert_eq!(ui.get_selected_count(), 0);
    assert_eq!(ui.get_current(), 0);
}

#[test]
fn table_cells_extend_rectangles_and_reveal_the_keyboard_column() {
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(["a", "b", "c", "d"].map(str::to_owned).to_vec());
    let model = ui.model_records();
    let first = model.row_key(0).unwrap();
    let third = model.row_key(2).unwrap();
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    for (row, column, operation) in [
        (0, 0, RowSelection::Replace),
        (2, 1, RowSelection::Extend),
        (2, 0, RowSelection::Toggle),
    ] {
        let (scene, layout) = render(&ui, &input);
        let item = layout.at_path(&path).unwrap();
        let table = item.table.as_ref().unwrap();
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: table.columns[column].x + 12.0,
                y: item.children[row].bounds.y + 12.0,
            },
        );
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        let action = input
            .select_group_option_with_selection(&scene, &layout, &target, operation)
            .unwrap();
        assert_eq!(
            action.action,
            ControlAction::TableSelection(TableSelectionAction::Cell {
                row,
                column,
                operation
            })
        );
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    assert_eq!(ui.get_selected_count(), 5);
    assert!(model.sort_by(|a, b| b.cmp(a)));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let table = scene.at_path(&path).unwrap();
    let view = table.table_selection.as_ref().unwrap();
    assert!(view.contains_cell(first, 0));
    assert!(!view.contains_cell(third, 0));
    assert!(view.contains_cell(third, 1));
    assert!(!table.children[1].children[0].children[0].boolean("table-selected", false));
    assert!(table.children[1].children[0].children[1].boolean("table-selected", false));
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 640
        }
    ));

    let (scene, layout) = render(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    assert!(item.table.as_ref().unwrap().columns[1].x >= item.bounds.x + item.bounds.width);
    let right = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Right, RowSelection::Replace)
        .unwrap();
    assert!(ui.control_at(&scene, &right.path, right.action));
    let (scene, layout) = render(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert!(table.horizontal.offset > 0.0);
    assert_eq!(
        table.columns[1].intersect(item.bounds).width,
        table.columns[1].width
    );
    assert_eq!(
        scene
            .at_path(&path)
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .current_column(),
        Some(1)
    );
    assert!(!input.begin_table_edit(&scene, &layout));

    let left = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Left, RowSelection::Replace)
        .unwrap();
    assert!(ui.control_at(&scene, &left.path, left.action));
    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(
        input
            .table_selection_key(&scene, TableSelectionKey::All)
            .is_none()
    );
}

#[test]
fn table_column_headers_select_and_sort_independently() {
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Columns);
    ui.set_records(["b", "a"].map(str::to_owned).to_vec());
    let model = ui.model_records();
    let records = model.clone();
    ui.on_sort_requested(move |column, descending| {
        assert_eq!(column, 0);
        assert!(!descending);
        records.sort_by(String::cmp);
    });
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    for (column, operation, sort) in [
        (1, RowSelection::Replace, false),
        (0, RowSelection::Extend, false),
        (0, RowSelection::Replace, true),
    ] {
        let (scene, layout) = render(&ui, &input);
        let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: table.columns[column].x + 12.0,
                y: table.header.y + 10.0,
            },
        );
        input.pointer_press();
        let action = input.table_header_release(&scene, operation, sort).unwrap();
        let expected = if sort {
            ControlAction::SortColumn(column)
        } else {
            ControlAction::TableSelection(TableSelectionAction::Column { column, operation })
        };
        assert_eq!(action.action, expected);
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    assert_eq!(model.snapshot(), ["a", "b"]);
    assert_eq!(ui.get_selected_count(), 2);
    assert_eq!(ui.get_current(), -1);
    ui.set_records(vec!["new".into()]);
    let (scene, _) = render(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    let selection = table.table_selection.as_ref().unwrap();
    assert_eq!(
        selection.columns().iter().copied().collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(
        table.children[0].children[0]
            .children
            .iter()
            .all(|cell| cell.boolean("table-selected", false))
    );
    ui.set_target(SelectionTarget::Cells);
    let (scene, _) = render(&ui, &input);
    assert!(
        scene
            .at_path(&path)
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn table_resize_captures_pointer_and_scrolls_headers_with_cells() {
    let mut ui = TableDemo::new();
    ui.set_records((0..100).map(|row| format!("Row {row}")).collect());
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let grip = |table: &TableGeometry| Point {
        x: table.columns[0].x + table.columns[0].width - 1.0,
        y: table.header.y + 10.0,
    };
    let start = grip(geometry);
    input.pointer_move(&scene, &layout, start);
    assert!(input.table_resize_cursor());
    assert_eq!(input.table_resize_column(&path), Some(0));
    assert!(!input.table_column_hovered(&path, 0));
    input.pointer_press();
    assert!(input.table_resize_drag(&scene, &layout).is_none());
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: start.x + 1_000.0,
            y: 0.0,
        },
    );
    assert!(input.hovered.is_none());
    let resize = input.table_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        resize.action,
        ControlAction::ResizeColumn {
            column: 0,
            width: 640
        }
    );
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    assert!(input.finish_table_resize());
    assert!(!input.table_resize_cursor());
    assert!(input.table_sort_release().is_none());
    assert!(!ui.control_at(
        &scene,
        &path,
        ControlAction::ResizeColumn {
            column: 1,
            width: 50
        }
    ));

    ui.set_records((0..200).map(|row| format!("New {row}")).collect());
    let (scene, layout) = render(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert_eq!(
        (table.columns[0].width, table.columns[1].width),
        (640.0, 148.0)
    );
    assert!(
        scene
            .at_path(&path)
            .unwrap()
            .table_header
            .as_ref()
            .unwrap()
            .sort
            .is_none()
    );
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: item.bounds.x + 10.0,
            y: table.body.y + 10.0,
        },
    );
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 1_000.0 }, true));
    assert!(input.scroll_y(&layout, 64.0));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert_eq!(table.horizontal.offset, table.horizontal.max_offset);
    assert_eq!(item.scroll.unwrap().offset, 64.0);
    assert!(!input.scroll_wheel(&layout, Point { x: 0.0, y: 32.0 }, true));
    assert_eq!(input.scroll_offsets().get(path.as_slice()), Some(&64.0));
    for (cell, column) in item.children[0].children[0]
        .children
        .iter()
        .zip(&table.columns)
    {
        assert_eq!((cell.bounds.x, cell.bounds.width), (column.x, column.width));
    }
    assert_eq!(item.children[0].clip.y, table.body.y);
    assert_eq!(scene.at_path(&path).unwrap().list.unwrap().first_row, 2);

    let start = grip(table);
    input.pointer_move(&scene, &layout, start);
    input.pointer_press();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: start.x - 10.0,
            y: start.y,
        },
    );
    let resize = input.table_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        resize.action,
        ControlAction::ResizeColumn {
            column: 0,
            width: 630
        }
    );
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: start.x - 20.0,
            y: start.y,
        },
    );
    let resize = input.table_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        resize.action,
        ControlAction::ResizeColumn {
            column: 0,
            width: 620
        }
    );
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: start.x - 1_000.0,
            y: start.y,
        },
    );
    let resize = input.table_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        resize.action,
        ControlAction::ResizeColumn {
            column: 0,
            width: 140
        }
    );
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    assert!(input.finish_table_resize());
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let item = layout.at_path(&path).unwrap();
    assert_eq!(item.table.as_ref().unwrap().horizontal.offset, 0.0);
    assert!(input.horizontal_scroll_offsets().is_empty());
    assert_eq!(item.scroll.unwrap().offset, 64.0);
    let body = item.table.as_ref().unwrap().body;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: body.x + 10.0,
            y: body.y + 10.0,
        },
    );
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 32.0 }, true));
    assert_eq!(input.scroll_offsets().get(path.as_slice()), Some(&96.0));
}

#[test]
fn table_cell_edit_tracks_row_identity_and_rejects_conflicting_commits() {
    let mut ui = TableDemo::new();
    ui.set_records(vec!["zulu".into(), "beta".into(), "alpha".into()]);
    let model = ui.model_records();
    let edited_key = model.row_key(0);
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    let now = Instant::now();
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let point = Point {
        x: table.body.x + 12.0,
        y: table.body.y + 10.0,
    };
    let click = |input: &mut Interaction, when| {
        input.pointer_move(&scene, &layout, point);
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        let editing = input.table_cell_click(&scene, &layout, Some(&target), when);
        let action = input.select_group_option(&scene, &layout, &target).unwrap();
        assert_eq!(action.action, ControlAction::Select(0));
        editing
    };
    assert!(!click(&mut input, now));
    assert!(click(&mut input, now + Duration::from_millis(120)));
    let cell_path = input.focused.as_ref().unwrap().path.clone();
    assert!(input.table_cell_editing(&cell_path));
    assert_eq!(input.focused.as_ref().unwrap().row_key, edited_key);
    let cell = scene.at_path(&cell_path).unwrap();
    assert_eq!(input.editor.as_ref().unwrap().selection("zulu"), 0..4);
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("改名"),
        EditSource::Atomic,
        now
    ));
    assert_eq!(input.editor_value(cell), "改名");
    assert_eq!(model.row(0).as_deref(), Some("zulu"));
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(input.editor_value(cell), "zulu");
    assert!(input.replay_text_edit(&mut ui, &scene, true));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("预".into(), Some((0, 3)));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert!(input.table_cell_editing(&cell_path));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit(String::new(), None);

    assert!(model.sort_by(String::cmp));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let moved = input.focused.as_ref().unwrap();
    assert_ne!(moved.path, cell_path);
    assert_eq!(moved.row_key, edited_key);
    assert!(input.table_cell_editing(&moved.path));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.snapshot(), ["alpha", "beta", "改名"]);

    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("放弃"),
        EditSource::Keyboard,
        now
    ));
    assert!(input.cancel_table_edit());
    assert_eq!(model.row(2).as_deref(), Some("改名"));
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("冲突"),
        EditSource::Keyboard,
        now
    ));
    assert!(model.set_row(2, "外部更新".into()));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(2).as_deref(), Some("外部更新"));

    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("旧快照"),
        EditSource::Keyboard,
        now
    ));
    assert!(model.insert(0, "new".into()));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.snapshot(), ["new", "alpha", "beta", "外部更新"]);

    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.begin_table_edit(&scene, &layout));
    assert_eq!(model.remove(3).as_deref(), Some("外部更新"));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, path);
    assert!(!input.cancel_table_edit());
}

#[test]
fn record_cell_edit_preserves_other_fields_and_detects_field_conflicts() {
    let mut ui = RecordTableDemo::new();
    let model = ui.model_records();
    let key = model.row_key(0).unwrap();
    let viewport = Size::new(650.0, 330.0);
    let render = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let name = [0, 1, 0, 0, 0];
    let owner = [0, 1, 0, 0, 1];
    let units = [0, 1, 0, 0, 2];
    let click = |input: &mut Interaction,
                 scene: &xrack_runtime::Node,
                 layout: &xrack_layout::LayoutNode,
                 path: &[usize]| {
        let cell = layout.at_path(path).unwrap().bounds;
        input.pointer_move(
            scene,
            layout,
            Point {
                x: cell.x + 12.0,
                y: cell.y + 10.0,
            },
        );
        input.pointer_press();
        assert_eq!(input.pointer_release_target().unwrap().path, path);
    };
    let (scene, layout) = render(&ui, &input);
    assert_eq!(scene.at_path(&units).unwrap().int("value"), Some(12));
    click(&mut input, &scene, &layout, &name);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("核心交换机"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(model.set_row(
        0,
        StockRow {
            units: 17,
            ..model.row(0).unwrap()
        }
    ));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.table_cell_editing(&name));
    assert_eq!(scene.at_path(&units).unwrap().int("value"), Some(17));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row_key(0), Some(key));
    assert_eq!(
        model.row(0).unwrap(),
        StockRow {
            name: "核心交换机".into(),
            owner: "网络组".into(),
            units: 17,
            available: true,
            load: 0.25,
        }
    );

    let (scene, layout) = render(&ui, &input);
    click(&mut input, &scene, &layout, &owner);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("数据中心"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(model.set_row(
        0,
        StockRow {
            owner: "平台组".into(),
            ..model.row(0).unwrap()
        }
    ));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("基础设施组"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(
        model.row(0).unwrap(),
        StockRow {
            name: "核心交换机".into(),
            owner: "基础设施组".into(),
            units: 17,
            available: true,
            load: 0.25,
        }
    );

    let (scene, layout) = render(&ui, &input);
    click(&mut input, &scene, &layout, &units);
    assert!(!input.begin_table_edit(&scene, &layout));
    assert!(!ui.commit_table_cell(&scene, &units, "17", "99".into()));
}

#[test]
fn record_spin_and_toggle_edit_their_own_fields_and_guard_stale_edits() {
    let mut ui = RecordTableDemo::new();
    let model = ui.model_records();
    let key = model.row_key(0).unwrap();
    let viewport = Size::new(650.0, 330.0);
    let render = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let units = [0, 1, 0, 0, 2];
    let available = [0, 1, 0, 0, 3];
    let (scene, layout) = render(&ui, &input);
    let up = SpinGeometry::new(layout.at_path(&units).unwrap().bounds).up;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: up.x + 6.0,
            y: up.y + 6.0,
        },
    );
    input.pointer_press();
    input.spin_press(&scene, &layout);
    let step = input.spin_release(&scene, &layout).unwrap();
    assert_eq!(step.action, ControlAction::Spin(SpinAction::Increase));
    assert!(ui.control_at(&scene, &step.path, step.action));
    assert_eq!(model.row(0).unwrap().units, 13);

    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let node = scene.at_path(&units).unwrap();
    input.edit_spin(node, EditAction::SelectAll);
    input.edit_spin(node, EditAction::Insert("27"));
    assert!(model.set_row(
        0,
        StockRow {
            owner: "平台组".into(),
            ..model.row(0).unwrap()
        }
    ));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(input.editor_value(scene.at_path(&units).unwrap()), "27");
    let commit = input.commit_spin_draft().unwrap();
    assert_eq!(
        commit.action,
        ControlAction::Spin(SpinAction::CommitIfCurrent {
            original: 13,
            value: 27
        })
    );
    assert!(ui.control_at(&scene, &commit.path, commit.action));
    assert_eq!(model.row(0).unwrap().units, 27);
    assert_eq!(model.row(0).unwrap().owner, "平台组");

    let (scene, _) = render(&ui, &input);
    let node = scene.at_path(&units).unwrap();
    input.edit_spin(node, EditAction::SelectAll);
    input.edit_spin(node, EditAction::Insert("32"));
    assert!(model.set_row(
        0,
        StockRow {
            units: 41,
            ..model.row(0).unwrap()
        }
    ));
    let stale = input.commit_spin_draft().unwrap();
    assert!(!ui.control_at(&scene, &stale.path, stale.action));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    let node = scene.at_path(&units).unwrap();
    input.edit_spin(node, EditAction::SelectAll);
    input.edit_spin(node, EditAction::Insert("42"));
    assert!(model.set_row(
        0,
        StockRow {
            units: 43,
            ..model.row(0).unwrap()
        }
    ));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.commit_spin_draft().is_none());
    assert_eq!(input.editor_value(scene.at_path(&units).unwrap()), "43");

    let cell = layout.at_path(&available).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 12.0,
            y: cell.y + 10.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, available);
    assert!(
        input
            .select_group_option(&scene, &layout, &target)
            .is_none()
    );
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(!ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(!model.row(0).unwrap().available);
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    input.key_press(ActivationKey::Space);
    let target = input.key_release_target(ActivationKey::Space).unwrap();
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(model.row_key(0), Some(key));
    let row = model.row(0).unwrap();
    assert!(row.available);
    assert_eq!(row.units, 43);
    assert_eq!(row.owner, "平台组");
}

#[test]
fn record_slider_keeps_field_updates_independent_and_rejects_old_values() {
    let mut ui = RecordTableDemo::new();
    let model = ui.model_records();
    let viewport = Size::new(760.0, 330.0);
    let render = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let path = [0, 1, 0, 0, 4];
    let (scene, layout) = render(&ui, &input);
    let bounds = layout.at_path(&path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        },
    );
    input.pointer_press();
    let drag = input.slider_press(&scene, &layout).unwrap();
    assert_eq!(drag.path, path);
    assert!(model.set_row(
        0,
        StockRow {
            units: 23,
            ..model.row(0).unwrap()
        }
    ));
    assert!(ui.adjust_range_at(&scene, &drag.path, drag.action));
    assert_eq!(model.row(0).unwrap().load, 0.5);
    assert_eq!(model.row(0).unwrap().units, 23);
    assert!(!ui.adjust_range_at(&scene, &path, RangeAction::Maximum));

    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    input.slider_release(&scene, &layout).unwrap();
    let key = input
        .slider_key(&scene, &layout, NavigationKey::End, true)
        .unwrap();
    assert!(ui.adjust_range_at(&scene, &key.path, key.action));
    assert_eq!(model.row(0).unwrap().load, 1.0);
    assert_eq!(model.row(0).unwrap().units, 23);
}

#[test]
fn table_edit_starts_on_the_keyboard_selected_row_and_cancels_offscreen() {
    let mut ui = TableDemo::new();
    ui.set_records((0..100).map(|index| format!("Row {index}")).collect());
    let viewport = Size::new(560.0, 370.0);
    let render = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = render(&ui, &input);
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("records-table"));
    let select = input.list_key(&scene, &layout, NavigationKey::End).unwrap();
    assert!(ui.control_at(&scene, &select.path, select.action));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(input.begin_table_edit(&scene, &layout));
    assert_eq!(
        input.focused.as_ref().unwrap().row_key,
        ui.model_records().row_key(99)
    );
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("draft"),
        EditSource::Keyboard,
        Instant::now()
    ));
    let cell = layout
        .at_path(&input.focused.as_ref().unwrap().path)
        .unwrap();
    let visible = cell.bounds.intersect(cell.clip);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: visible.x + 5.0,
            y: visible.y + 5.0,
        },
    );
    assert!(input.scroll_y(&layout, -10_000.0));
    let (scene, layout) = render(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("records-table"));
    assert!(!input.cancel_table_edit());
    assert_eq!(ui.model_records().row(99).as_deref(), Some("Row 99"));
}

#[test]
fn counter_click_updates_dependent_labels() {
    let mut ui = UiDriver::new(Counter::new(), FixedText, Size::new(320.0, 180.0));
    assert_eq!(ui.node("label").text("text"), Some("计数：0"));
    ui.click("add");
    ui.click("add");
    assert_eq!(ui.node("label").text("text"), Some("计数：2"));
    assert_eq!(ui.node("status").text("text"), Some("加倍：4"));
    assert_eq!(ui.focused(), Some("add"));
}

#[test]
fn rename_workflow_routes_pointer_text_and_keyboard_by_named_targets() {
    let mut ui = UiDriver::new(DialogDemo::new(), FixedText, Size::new(640.0, 390.0));
    ui.click("opener");
    assert_eq!(ui.focused(), Some("name-field"));
    assert!(!ui.can_hit("background-action"));
    ui.fill_focused("新工作区");
    assert_eq!(ui.node("name-field").text("text"), Some("新工作区"));
    ui.tab();
    assert_eq!(ui.focused(), Some("help-opener"));
    ui.click("help-opener");
    assert!(ui.node("help").is_visible());
    ui.escape();
    assert_eq!(ui.focused(), Some("help-opener"));
    ui.click("save");
    assert_eq!(ui.focused(), Some("opener"));
    assert!(ui.can_hit("background-action"));
    assert!(!ui.node("rename").is_visible());
    ui.click("opener");
    assert_eq!(ui.node("name-field").text("text"), Some("新工作区"));
}

#[test]
fn drawer_and_dialog_restore_focus_in_open_order() {
    let mut ui = UiDriver::new(DialogDemo::new(), FixedText, Size::new(640.0, 390.0));
    assert!(ui.node("drawer").children.is_empty());
    ui.click("drawer-opener");
    assert_eq!(ui.focused(), Some("drawer-rename"));
    assert!(!ui.can_hit("background-action"));
    ui.tab();
    assert_eq!(ui.focused(), Some("drawer-close"));
    ui.tab();
    assert_eq!(ui.focused(), Some("drawer-rename"));
    ui.click("drawer-rename");
    assert_eq!(ui.focused(), Some("name-field"));
    assert!(ui.node("drawer").is_visible());
    assert!(!ui.can_hit("drawer-close"));
    ui.escape();
    assert_eq!(ui.focused(), Some("drawer-rename"));
    assert!(ui.can_hit("drawer-close"));
    ui.escape();
    assert_eq!(ui.focused(), Some("drawer-opener"));
    assert!(ui.can_hit("background-action"));
    assert!(ui.node("drawer").children.is_empty());
}

#[test]
fn navigation_stack_restores_focus_and_page_state_through_replace_and_pop() {
    let mut ui = UiDriver::new(StackDemo::new(), FixedText, Size::new(590.0, 324.0));
    let routes = |ui: &UiDriver<StackDemo, FixedText>| {
        ui.node("navigation")
            .stack
            .as_ref()
            .unwrap()
            .routes()
            .to_vec()
    };
    assert_eq!(routes(&ui), ["overview"]);
    assert!(ui.node("details").children.is_empty());
    ui.click("overview-next");
    assert_eq!(routes(&ui), ["overview", "details"]);
    assert_eq!(ui.focused(), Some("add"));
    ui.click("add");
    ui.click("detail-next");
    assert_eq!(routes(&ui), ["overview", "details", "review"]);
    assert!(ui.node("details").children.is_empty());
    assert_eq!(ui.focused(), Some("review-replace"));
    ui.click("review-replace");
    assert_eq!(routes(&ui), ["overview", "details", "summary"]);
    assert_eq!(ui.focused(), Some("summary-back"));
    assert!(ui.back());
    assert_eq!(routes(&ui), ["overview", "details"]);
    assert_eq!(ui.focused(), Some("detail-next"));
    assert_eq!(
        ui.node("detail-counter").children[0].text("text"),
        Some("此页计数：1")
    );
    assert!(ui.back());
    assert_eq!(ui.focused(), Some("overview-next"));
    assert!(!ui.back());

    ui.click("overview-next");
    ui.click("detail-next");
    ui.click("review-home");
    assert_eq!(routes(&ui), ["overview"]);
    assert_eq!(ui.focused(), Some("overview-next"));
}

#[test]
fn stack_status_and_back_button_bind_to_live_routes() {
    let viewport = Size::new(590.0, 324.0);
    let mut linked = StackDemo::new();
    linked.set_start_route("summary".into());
    let mut ui = UiDriver::new(linked, FixedText, viewport);
    assert_eq!(
        ui.node("stack-status").text("text"),
        Some("栈顶 summary · 深度 1 · 导航 0 次")
    );
    assert!(!ui.can_hit("summary-back"));

    let mut invalid_path = StackDemo::new();
    invalid_path.set_preview_path(vec!["overview".into(), "details".into(), "missing".into()]);
    let mut ui = UiDriver::new(invalid_path, FixedText, viewport);
    ui.click("overview-deep-link");
    assert_eq!(
        ui.node("stack-status").text("text"),
        Some("栈顶 overview · 深度 1 · 导航 0 次")
    );

    let mut ui = UiDriver::new(StackDemo::new(), FixedText, viewport);
    ui.click("overview-deep-link");
    assert_eq!(
        ui.node("navigation").stack.as_ref().unwrap().routes(),
        ["overview", "details", "review"]
    );
    assert_eq!(
        ui.node("stack-status").text("text"),
        Some("栈顶 review · 深度 3 · 导航 1 次")
    );
    assert!(ui.node("details").children.is_empty());
    ui.click("review-home");
    assert_eq!(
        ui.node("stack-status").text("text"),
        Some("栈顶 overview · 深度 1 · 导航 2 次")
    );
    assert_eq!(ui.focused(), Some("overview-deep-link"));
}

#[test]
fn combo_keyboard_cancel_and_accept_have_different_commit_behavior() {
    let mut ui = UiDriver::new(SelectionDemo::new(), FixedText, Size::new(560.0, 350.0));
    ui.click("workspace-box");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Escape);
    assert_eq!(ui.node("workspace-box").int("current-index"), Some(1));
    ui.click("workspace-box");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert_eq!(ui.node("workspace-box").int("current-index"), Some(11));
    assert_eq!(ui.focused(), Some("workspace-box"));
    ui.click("lock-toggle");
    assert!(!ui.can_hit("workspace-box"));
}

#[test]
fn menu_bar_skips_disabled_items_and_navigates_submenus() {
    let mut ui = UiDriver::new(MenuDemo::new(), FixedText, Size::new(620.0, 366.0));
    ui.tab();
    assert_eq!(ui.focused(), Some("file-menu"));
    ui.menu_key(MenuKey::Down);
    assert_eq!(ui.menus().len(), 1);
    assert!(!ui.click_menu_row("导入"));
    assert_eq!(ui.node("status").text("text"), Some("请选择一条命令"));
    ui.menu_key(MenuKey::Down);
    ui.menu_key(MenuKey::Right);
    assert_eq!(ui.menus().len(), 2);
    ui.menu_key(MenuKey::Escape);
    assert_eq!(ui.menus().len(), 1);
    ui.menu_key(MenuKey::Right);
    ui.menu_key(MenuKey::End);
    ui.menu_key(MenuKey::Accept);
    assert_eq!(ui.node("status").text("text"), Some("已导出图像"));
    assert!(ui.menus().is_empty());
    assert_eq!(ui.focused(), Some("file-menu"));

    ui.click("view-menu");
    assert!(ui.click_menu_row("显示网格"));
    assert!(ui.node("grid-item").boolean("checked", false));
    assert_eq!(ui.node("status").text("text"), Some("已显示网格"));
    ui.click("file-menu");
    ui.click("background-button");
    assert!(ui.menus().is_empty());
    assert_eq!(
        ui.node("background-button").text("text"),
        Some("背景计数 · 0")
    );
    ui.click("background-button");
    assert_eq!(
        ui.node("background-button").text("text"),
        Some("背景计数 · 1")
    );
}

#[test]
fn menu_shortcuts_dispatch_in_the_active_scope_and_ignore_conflicts() {
    let viewport = Size::new(620.0, 366.0);
    let mut ui = UiDriver::new(MenuDemo::new(), FixedText, viewport);
    let new = "Primary+N".parse().unwrap();
    let grid = "Primary+G".parse().unwrap();
    let context = "Primary+R".parse().unwrap();

    assert!(ui.menu_shortcut(new));
    assert_eq!(ui.node("status").text("text"), Some("已新建"));
    assert!(!ui.menu_shortcut("Primary+I".parse().unwrap()));
    assert!(!ui.menu_shortcut("Primary+E".parse().unwrap()));
    assert!(ui.menu_shortcut("shift+primary+e".parse().unwrap()));
    assert_eq!(ui.node("status").text("text"), Some("已导出图像"));
    assert!(ui.menu_shortcut(grid));
    assert!(ui.node("grid-item").boolean("checked", false));
    assert_eq!(ui.node("status").text("text"), Some("已显示网格"));

    assert!(!ui.menu_shortcut(context));
    assert!(ui.context_menu("workspace"));
    assert!(!ui.menu_shortcut(new));
    assert!(ui.menu_shortcut(context));
    assert!(ui.menus().is_empty());
    assert_eq!(ui.node("status").text("text"), Some("已刷新工作区"));

    ui.click("dialog-button");
    assert!(!ui.menu_shortcut(new));
    assert!(!ui.menu_shortcut(context));
    assert!(ui.keyboard_context_menu());
    assert!(ui.menu_shortcut(context));
    assert_eq!(ui.node("status").text("text"), Some("已检查属性"));
    assert_eq!(ui.focused(), Some("inspector-area"));
    ui.escape();
    assert!(ui.menu_shortcut(new));

    let mut demo = MenuDemo::new();
    demo.set_new_shortcut("Primary+G".into());
    let mut conflict = UiDriver::new(demo, FixedText, viewport);
    assert!(!conflict.menu_shortcut(grid));
    assert!(!conflict.menu_shortcut(new));
    assert!(!conflict.node("grid-item").boolean("checked", false));
    assert_eq!(conflict.node("status").text("text"), Some("请选择一条命令"));
}

#[test]
fn action_shares_state_across_presentations_and_rejects_stale_activation() {
    let viewport = Size::new(620.0, 366.0);
    let shortcut = "Primary+G".parse().unwrap();
    let mut ui = UiDriver::new(MenuDemo::new(), FixedText, viewport);
    assert_eq!(
        ui.scene().children.len(),
        1,
        "Action does not occupy a scene slot"
    );
    assert_eq!(ui.node("grid-button").kind, "ToolButton");
    assert!(ui.node("grid-button").image("icon").is_some());
    let identity = ui.node("grid-button").action.unwrap();
    assert_eq!(ui.node("grid-item").action, Some(identity));
    ui.click("grid-button");
    assert!(ui.node("grid-item").boolean("checked", false));
    assert_eq!(ui.component().get_grid_activations(), 1);
    assert!(ui.menu_shortcut(shortcut));
    assert!(!ui.node("grid-button").boolean("checked", true));
    assert_eq!(ui.component().get_grid_activations(), 2);
    ui.click("view-menu");
    assert!(ui.click_menu_row("显示网格"));
    assert!(ui.node("grid-button").boolean("checked", false));
    assert_eq!(ui.component().get_grid_activations(), 3);

    let mut second = MenuDemo::new();
    let mut scene = (*second.render_shared()).clone();
    let button_path = [0, 2, 0];
    let button = scene.at_path(&button_path).unwrap();
    assert_eq!(button.id, Some("grid-button"));
    assert_ne!(button.action, Some(identity));
    assert!(!button.boolean("checked", true));
    let view_menu = &mut scene.children[0].children[0].children[1];
    assert_eq!(view_menu.id, Some("view-menu"));
    view_menu
        .properties
        .insert("visible", xrack_runtime::Value::Bool(false));
    let input = Interaction::default();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let from_button = input.menu_shortcut(&scene, &layout, shortcut).unwrap();
    assert_eq!(from_button.path, button_path);
    assert_eq!(from_button.action, ControlAction::ActivateAction);
    assert!(second.control_at(&scene, &from_button.path, from_button.action));
    assert!(second.get_show_grid());
    assert_eq!(second.get_grid_activations(), 1);

    second.set_grid_enabled(false);
    assert!(!second.dispatch_at(&scene, &button_path, "clicked"));
    assert!(!second.control_at(&scene, &from_button.path, from_button.action));
    assert_eq!(second.get_grid_activations(), 1);
    let disabled = second.render_shared();
    assert!(
        !disabled
            .at_path(&button_path)
            .unwrap()
            .boolean("enabled", true)
    );
    let layout = xrack_layout::layout(&disabled, viewport, &mut FixedText);
    assert!(input.menu_shortcut(&disabled, &layout, shortcut).is_none());
}

#[test]
fn context_menu_respects_modal_scope_and_restores_focus() {
    let mut ui = UiDriver::new(MenuDemo::new(), FixedText, Size::new(620.0, 366.0));
    assert!(ui.context_menu("workspace"));
    assert!(ui.click_menu_row("刷新"));
    assert_eq!(ui.node("status").text("text"), Some("已刷新工作区"));

    ui.click("dialog-button");
    assert_eq!(ui.focused(), Some("inspector-area"));
    assert!(ui.keyboard_context_menu());
    assert_eq!(ui.menus()[0].rows[0].label, "检查属性");
    assert!(ui.click_menu_row("检查属性"));
    assert_eq!(ui.node("status").text("text"), Some("已检查属性"));
    assert!(ui.node("inspector").is_visible());
    assert_eq!(ui.focused(), Some("inspector-area"));
    ui.escape();
    assert_eq!(ui.focused(), Some("dialog-button"));
}
