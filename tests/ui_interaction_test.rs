pub mod ui_driver;

use async_ui::AsyncDemo;
use controls_ui::{
    CalendarAgendaDemo, CalendarDemo, CheckGroupDemo, CheckStateDemo, ColorDemo, DecimalPrice,
    DecimalSpinDemo, DelayDemo, FontPickerDemo, GroupBoxDemo, MonthGridDemo, ReviewRow,
    SnapSlidersDemo, TimeDemo, TimerDemo, TumblerDemo,
};
use counter_ui::Counter;
use delegates_ui::SwipeDemo;
use dialog_ui::{DialogDemo, PopupDemo, StandardDialogDemo};
use form_ui::{
    CloseGuardDemo, DragDropDemo, EditorCommandsDemo, EditorMenuDemo, FileDialogDemo, FileDropDemo,
    MessageDialogDemo, ValidationDemo, VirtualDragDemo,
};
use layout_ui::{
    ComponentStatesDemo, FocusScopesDemo, FramelessDemo, GeometryDemo, ScopedKeysDemo, StatesDemo,
};
use list_ui::{DirectoryItem, GridDemo, ItemDelegatesDemo, ScrollPositionDemo, StandardListDemo};
use menu_ui::{ActionGroupDemo, MenuDemo, ShortcutDemo};
use navigation_ui::{PageDemo, StackDemo, StackGuardDemo, TabBarDemo};
use records_ui::{DelegateTableDemo, NestedRecordsDemo, QuantityPaste, RecordTableDemo, StockRow};
use selection_ui::{EditableChoiceDemo, GroupedRadiosDemo, RecordComboDemo, SelectionDemo};
use split_ui::SplitDemo;
use std::time::{Duration, Instant};
use table_ui::{InventoryRow, StandardTableDemo, TableDemo, TableSortViewsDemo};
use tooltip_ui::TooltipDemo;
use tree_ui::{
    DelegateTreeDemo, SizedBranch, TreeBranchesDemo, TreeCommandsDemo, TreeDemo, TreeRecord,
};
use ui_driver::UiDriver;
use xrack_layout::{
    ActivationKey, CalendarGeometry, ChoiceKey, EditAction, EditSource, Interaction, MenuKey,
    NavigationKey, Point, Rect, ScrollGestureKind, ShortcutOutcome, Size, SliderKey, SpinGeometry,
    TableGeometry, TableSelectionKey, TextMeasurer, TreeRowGeometry, layout_tooltip, prepare_scene,
    prepare_scene_with_input,
};
use xrack_runtime::{
    CalendarAction, CheckState, Color, Component, ControlAction, Date, DialogRole, Orientation,
    Property, RangeAction, RowSelection, SelectionTarget, SpinAction, StandardButtonKind,
    SwipeSide, TableSelectionAction, TableSort, Time, TreeAction, TreeSelectionAction,
};

struct FixedText;

#[test]
fn wheel_handlers_use_local_coordinates_bubble_and_keep_default_scrolling() {
    use layout_ui::WheelDemo;
    use xrack_runtime::{EventResult, KeyModifiers, Length, WheelEvent};
    let mut ui = WheelDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(460.0, 300.0);
    let prepare = |ui: &WheelDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let bounds = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let point = Point {
        x: bounds.x + 80.0,
        y: bounds.y + 60.0,
    };
    input.pointer_move(&scene, &layout, point);
    let zoom = WheelEvent {
        delta_y: Length(-120.0),
        modifiers: KeyModifiers {
            control: true,
            ..KeyModifiers::default()
        },
        ..WheelEvent::default()
    };
    let route = input.wheel_route(&scene, &layout, zoom).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Accept);
    assert_eq!((ui.get_inner_events(), ui.get_outer_events()), (1, 0));
    assert_eq!(
        (ui.get_last_x(), ui.get_last_y()),
        (Length(80.0), Length(60.0))
    );
    assert_eq!(ui.get_zoom(), 1.5);
    assert_eq!(ui.get_pan_x(), Length(-40.0));
    assert_eq!(ui.get_pan_y(), Length(-30.0));
    assert!(input.scroll_offsets().is_empty());

    let (scene, layout) = prepare(&ui, &input);
    let scroll = WheelEvent {
        delta_y: Length(28.0),
        lines_y: 1.0,
        is_line: true,
        ..WheelEvent::default()
    };
    let route = input.wheel_route(&scene, &layout, scroll).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Reject);
    assert_eq!(ui.get_last_lines(), 1.0);
    assert_eq!((ui.get_inner_events(), ui.get_outer_events()), (2, 1));
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 28.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&28.0));

    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    let horizontal = WheelEvent {
        delta_x: Length(20.0),
        ..WheelEvent::default()
    };
    let route = input.wheel_route(&scene, &layout, horizontal).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Reject);
    assert_eq!((ui.get_inner_events(), ui.get_outer_events()), (2, 2));
    ui.set_zoom_enabled(false);
    let (scene, layout) = prepare(&ui, &input);
    let route = input.wheel_route(&scene, &layout, zoom).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Reject);
    assert_eq!((ui.get_inner_events(), ui.get_outer_events()), (2, 3));

    let mut modal_scene = (*scene).clone();
    let mut dialog = xrack_runtime::Node::new("Dialog", Some("modal"));
    dialog.properties.insert("open", true.into());
    dialog.properties.insert("width", Length(100.0).into());
    dialog.properties.insert("height", Length(100.0).into());
    modal_scene.children.push(dialog);
    let layout = xrack_layout::layout_with_input(&modal_scene, size, &mut FixedText, &input);
    assert!(input.wheel_route(&modal_scene, &layout, zoom).is_none());
}

#[test]
fn touch_area_scroll_routes_to_the_outer_handler_then_default_scroll() {
    use layout_ui::PointerDemo;
    use xrack_runtime::{EventResult, KeyModifiers, Length, WheelEvent};

    let mut ui = PointerDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(420.0, 270.0);
    let prepare = |ui: &PointerDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let area = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let point = Point {
        x: area.x + 70.0,
        y: area.y + 35.0,
    };
    input.pointer_move(&scene, &layout, point);
    let event = WheelEvent {
        delta_y: Length(28.0),
        ..Default::default()
    };
    let control = WheelEvent {
        modifiers: KeyModifiers {
            control: true,
            ..Default::default()
        },
        ..event
    };
    let shift = WheelEvent {
        modifiers: KeyModifiers {
            shift: true,
            ..Default::default()
        },
        ..event
    };
    for (payload, expected, inner, outer) in [
        (control, EventResult::Accept, 1, 0),
        (shift, EventResult::Accept, 2, 1),
        (event, EventResult::Reject, 3, 2),
    ] {
        let route = input.wheel_route(&scene, &layout, payload).unwrap();
        assert_eq!(route.dispatch(&mut ui, &scene), expected);
        assert_eq!(
            (ui.get_scroll_events(), ui.get_outer_wheel_events()),
            (inner, outer)
        );
        assert_eq!(
            (ui.get_wheel_x(), ui.get_wheel_y()),
            (Length(70.0), Length(35.0))
        );
    }
    assert!(ui.get_wheel_tint());
    assert!(input.scroll_offsets().is_empty());
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 28.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&28.0));

    let (scene, layout) = prepare(&ui, &input);
    let viewport = layout.at_path(&[1]).unwrap().bounds;
    let clipped = Point {
        x: point.x,
        y: viewport.y - 4.0,
    };
    assert!(layout.at_path(&[1, 0, 0]).unwrap().bounds.contains(clipped));
    input.pointer_move(&scene, &layout, clipped);
    assert!(input.wheel_route(&scene, &layout, control).is_none());

    ui.set_area_enabled(false);
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    let route = input.wheel_route(&scene, &layout, control).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Reject);
    assert_eq!(
        (ui.get_scroll_events(), ui.get_outer_wheel_events()),
        (3, 3)
    );

    ui.set_area_enabled(true);
    let (scene, _) = prepare(&ui, &input);
    let mut scene = scene.as_ref().clone();
    scene.children[1].children[0].children[0]
        .properties
        .insert("transform-scale", 1.25.into());
    let layout = xrack_layout::layout_with_input(&scene, size, &mut FixedText, &input);
    let area = layout.at_path(&[1, 0, 0]).unwrap();
    let local = Point {
        x: area.bounds.x + 72.0,
        y: area.bounds.y + 36.0,
    };
    let transformed = area.map_to_window(local);
    assert_ne!(transformed, local);
    input.pointer_move(&scene, &layout, transformed);
    let route = input.wheel_route(&scene, &layout, control).unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Accept);
    assert!((ui.get_wheel_x().0 - 72.0).abs() < 0.001);
    assert!((ui.get_wheel_y().0 - 36.0).abs() < 0.001);
}

#[test]
fn touch_area_button_filter_routes_press_without_changing_hover_and_wheel() {
    use layout_ui::PointerPassThroughDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::{
        CursorShape, EventResult, Length, PointerButton, PointerEvent, PointerEventKind, WheelEvent,
    };

    let size = Size::new(420.0, 210.0);
    let mut driver = UiDriver::new(PointerPassThroughDemo::new(), FixedText, size);
    driver.click("only-secondary");
    driver.click("lower-button");
    assert_eq!(driver.component().get_lower_clicks(), 1);
    assert_eq!(driver.component().get_overlay_clicks(), 0);

    let mut ui = PointerPassThroughDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &PointerPassThroughDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let point = Point { x: 156.0, y: 100.0 };
    let now = Instant::now();
    let (pressed, down) = input
        .begin_pointer_sequence(&scene, &layout, point, PointerEvent::default(), now)
        .unwrap();
    assert!(down.apply(&mut ui, &scene));
    ui.set_secondary_only(true);
    let (scene, layout) = prepare(&ui, &input);
    assert!(!pressed.is_live(&input, &scene, &layout));
    assert!(pressed.cancel(Default::default()).apply(&mut ui, &scene));

    input.pointer_move(&scene, &layout, point);
    let mut hover = HoverObservers::default();
    hover.sync(&input, &scene, &layout, Some(point));
    assert!(ui.get_overlay_hovered());
    assert_eq!(hover.cursor_shape(), CursorShape::Grab);
    let wheel = WheelEvent {
        delta_y: Length(28.0),
        ..Default::default()
    };
    assert_eq!(
        input
            .wheel_route(&scene, &layout, wheel)
            .unwrap()
            .dispatch(&mut ui, &scene),
        EventResult::Accept
    );
    assert_eq!(ui.get_overlay_wheel_events(), 1);
    for button in [PointerButton::Primary, PointerButton::Middle] {
        assert!(
            input
                .begin_pointer_sequence(
                    &scene,
                    &layout,
                    point,
                    PointerEvent {
                        button,
                        ..Default::default()
                    },
                    now
                )
                .is_none()
        );
    }
    let (mut secondary, down) = input
        .begin_pointer_sequence(
            &scene,
            &layout,
            point,
            PointerEvent {
                button: PointerButton::Secondary,
                ..Default::default()
            },
            now,
        )
        .unwrap();
    assert_eq!(secondary.target().id, Some("overlay"));
    assert!(down.apply(&mut ui, &scene));
    let up = secondary
        .sample(
            &input,
            &scene,
            &layout,
            point,
            PointerEventKind::Up,
            Default::default(),
        )
        .unwrap();
    assert!(up.apply(&mut ui, &scene));
    assert_eq!(ui.get_secondary_releases(), 1);

    assert!(input.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    assert!(input.end_scroll_gesture(&scene, &layout, point, now).tap);
    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    assert_eq!(
        input.pointer_release_target().unwrap().id,
        Some("lower-button")
    );
}

#[test]
fn overlapping_touch_areas_route_middle_and_secondary_to_different_targets() {
    use xrack_runtime::{Length, Node, PointerButton, PointerButtons, PointerEvent};

    let mut scene = Node::new("Window", None);
    let mut editor = Node::new("LineEdit", Some("editor"));
    editor.properties.insert("width", Length(140.0).into());
    editor.properties.insert("height", Length(45.0).into());
    editor.properties.insert("text", "hello".to_owned().into());
    scene.children.push(editor);
    for (id, button) in [
        ("middle", PointerButton::Middle),
        ("right", PointerButton::Secondary),
    ] {
        let mut area = Node::new("TouchArea", Some(id));
        area.properties.insert("width", Length(140.0).into());
        area.properties.insert("height", Length(45.0).into());
        area.properties
            .insert("accepted-buttons", PointerButtons::from([button]).into());
        area.pointer_handle = Some(Default::default());
        scene.children.push(area);
    }
    let mut input = Interaction::default();
    let size = Size::new(160.0, 100.0);
    let layout = xrack_layout::layout_with_input(&scene, size, &mut FixedText, &input);
    let point = Point { x: 40.0, y: 20.0 };
    let now = Instant::now();
    for (button, id) in [
        (PointerButton::Secondary, "right"),
        (PointerButton::Middle, "middle"),
    ] {
        let event = PointerEvent {
            button,
            ..Default::default()
        };
        let (sequence, _) = input
            .begin_pointer_sequence(&scene, &layout, point, event, now)
            .unwrap();
        assert_eq!(sequence.target().id, Some(id));
    }
    input.pointer_move(&scene, &layout, point);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("editor"));
    assert_eq!(
        input.pointer_hover_target(&scene, &layout).unwrap().id,
        Some("right")
    );
    assert!(!input.open_context_menu(&scene, &layout, false));

    scene.children[2].properties.insert(
        "accepted-buttons",
        PointerButtons::from([PointerButton::Primary]).into(),
    );
    let layout = xrack_layout::layout_with_input(&scene, size, &mut FixedText, &input);
    input.pointer_move(&scene, &layout, point);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("right"));
    assert!(input.pointer_context_menu_changes_focus(&scene, &layout));
    assert!(input.open_context_menu(&scene, &layout, false));
    assert_eq!(input.focused.as_ref().unwrap().id, Some("editor"));
}

#[test]
fn disabled_touch_area_passes_input_to_siblings_and_keeps_children_active() {
    use layout_ui::PointerPassThroughDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::{EventResult, Length, WheelEvent};

    let size = Size::new(420.0, 210.0);
    let mut driver = UiDriver::new(PointerPassThroughDemo::new(), FixedText, size);
    driver.click("overlay");
    assert_eq!(driver.component().get_overlay_clicks(), 1);
    assert_eq!(driver.component().get_lower_clicks(), 0);
    driver.click("enable-overlay");
    assert!(!driver.component().get_overlay_enabled());
    driver.click("lower-button");
    driver.click("child-button");
    assert_eq!(driver.component().get_overlay_clicks(), 1);
    assert_eq!(driver.component().get_lower_clicks(), 1);
    assert_eq!(driver.component().get_child_clicks(), 1);

    let mut ui = PointerPassThroughDemo::new();
    let mut input = Interaction::default();
    let mut observers = HoverObservers::default();
    let point = Point { x: 156.0, y: 100.0 };
    let event = WheelEvent {
        delta_y: Length(28.0),
        ..Default::default()
    };
    for enabled in [true, false] {
        ui.set_overlay_enabled(enabled);
        let (scene, layout) =
            prepare_scene_with_input(&ui, ui.render_shared(), size, &mut FixedText, &input);
        input.pointer_move(&scene, &layout, point);
        for delivery in observers.sync(&input, &scene, &layout, Some(point)) {
            delivery.apply(&mut ui, &scene);
        }
        assert_eq!(ui.get_overlay_hovered(), enabled);
        assert_eq!(ui.get_lower_hovered(), !enabled);
        assert_eq!(
            input
                .wheel_route(&scene, &layout, event)
                .unwrap()
                .dispatch(&mut ui, &scene),
            EventResult::Accept
        );
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
        assert!(input.end_scroll_gesture(&scene, &layout, point, now).tap);
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(
            target.id,
            Some(if enabled { "overlay" } else { "lower-button" })
        );
    }
    assert_eq!(ui.get_overlay_wheel_events(), 1);
    assert_eq!(ui.get_lower_wheel_events(), 1);
}

#[test]
fn hover_state_tracks_nested_geometry_and_leaves_click_delivery_to_children() {
    use layout_ui::HoverDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::{CursorShape, Length};
    let mut ui = HoverDemo::new();
    let mut input = Interaction::default();
    let mut observers = HoverObservers::default();
    let size = Size::new(420.0, 240.0);
    let prepare = |ui: &HoverDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    let (_, layout) = prepare(&ui, &input);
    let button = layout.at_path(&[1, 0, 0, 0]).unwrap().bounds;
    let point = Point {
        x: button.x + 30.0,
        y: button.y + 15.0,
    };
    let sync = |ui: &mut HoverDemo, input: &Interaction, observers: &mut HoverObservers, point| {
        let (scene, layout) = prepare(ui, input);
        for delivery in observers.sync(input, &scene, &layout, point) {
            delivery.apply(ui, &scene);
        }
    };

    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(ui.get_area_hovered() && ui.get_button_hovered());
    assert_eq!(observers.cursor_shape(), CursorShape::Pointer);
    assert!(ui.get_both_on_entry());
    assert_eq!((ui.get_area_entries(), ui.get_button_entries()), (1, 1));
    assert_eq!(
        (ui.get_position_x(), ui.get_position_y()),
        (Length(56.0), Length(79.0))
    );
    sync(&mut ui, &input, &mut observers, Some(point));
    assert_eq!(ui.get_button_moves(), 0);
    let point = Point {
        x: point.x + 8.0,
        y: point.y,
    };
    sync(&mut ui, &input, &mut observers, Some(point));
    assert_eq!(ui.get_button_moves(), 1);

    ui.set_block_inner(true);
    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(!ui.get_area_hovered() && ui.get_button_hovered());
    assert_eq!(ui.get_area_exits(), 1);
    assert_eq!(ui.get_position_x(), Length(0.0));
    ui.set_block_inner(false);
    ui.set_inner_offset(Length(150.0));
    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(ui.get_area_hovered() && !ui.get_button_hovered());
    assert_eq!(ui.get_button_exits(), 1);
    ui.set_inner_offset(Length(0.0));
    sync(&mut ui, &input, &mut observers, Some(point));
    assert_eq!(ui.get_button_entries(), 2);
    ui.set_inner_enabled(false);
    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(!ui.get_button_hovered());
    assert_eq!(ui.get_button_exits(), 2);
    assert_eq!(observers.cursor_shape(), CursorShape::Crosshair);

    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.id, Some("preview"));
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(ui.get_clicks(), 1);
    assert!(ui.get_preview_open());
    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(!ui.get_area_hovered());
    assert_eq!(ui.get_area_exits(), 2);
    ui.set_preview_open(false);
    sync(&mut ui, &input, &mut observers, Some(point));
    assert!(ui.get_area_hovered());
    sync(&mut ui, &input, &mut observers, None);
    assert!(!ui.get_area_hovered());
    assert_eq!(ui.get_area_exits(), 3);
    assert_eq!(observers.cursor_shape(), CursorShape::Auto);
}

#[test]
fn item_delegates_activate_rows_and_edit_the_original_record() {
    let viewport = Size::new(644.0, 398.0);
    let mut ui = UiDriver::new(ItemDelegatesDemo::new(), FixedText, viewport);
    let item = [0, 0, 0, 1, 1, 0];
    let radio = [0, 0, 1, 1, 2, 0];
    let check = [0, 1, 0, 1, 0, 0];
    let switch = [0, 1, 1, 1, 0, 0];
    ui.click_at_path(&item);
    assert_eq!(ui.component().get_current_action(), 1);
    assert_eq!(ui.component().get_last_action(), "显示最近文件 · 2");
    assert!(
        ui.scene()
            .at_path(&item)
            .unwrap()
            .boolean("highlighted", false)
    );
    ui.click_at_path(&radio);
    assert_eq!(ui.component().get_current_mode(), 2);
    assert!(
        ui.scene()
            .at_path(&radio)
            .unwrap()
            .boolean("checked", false)
    );
    assert!(
        !ui.scene()
            .at_path(&[0, 0, 1, 1, 0, 0])
            .unwrap()
            .boolean("checked", false)
    );

    let model = ui.component().model_settings();
    let original = model.row_key(0);
    ui.click_at_path(&check);
    assert_eq!(model.row(0).unwrap().selected, CheckState::PartiallyChecked);
    ui.click_at_path(&switch);
    assert!(!model.row(0).unwrap().enabled);
    assert_eq!(model.row_key(0), original);
    assert_eq!(ui.component().get_edits(), 2);

    let stale = ui.scene().clone();
    ui.update(|component| {
        let mut row = model.row(0).unwrap();
        row.selected = CheckState::Checked;
        assert!(model.set_row(0, row));
        assert!(!component.dispatch_at(&stale, &check, "clicked"));
        model.sort_by(|left, right| right.label.cmp(&left.label));
        assert_ne!(model.row_key(0), original);
        assert!(!component.dispatch_at(&stale, &switch, "clicked"));
    });
    assert_eq!(ui.component().get_edits(), 2);
}

#[test]
fn list_keyboard_navigation_exposes_the_current_item_delegate_for_activation() {
    let mut ui = ItemDelegatesDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(644.0, 398.0);
    let prepare = |ui: &ItemDelegatesDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let view = [0, 0, 0, 1];
    let target = input.accessibility_target(&scene, &layout, &view).unwrap();
    input.accessibility_focus(target.clone());
    let navigation = input
        .list_key(&scene, &layout, NavigationKey::Down)
        .unwrap();
    assert!(ui.control_at(&scene, &navigation.path, navigation.action));
    let (scene, layout) = prepare(&ui, &input);
    let delegate = input
        .current_item_delegate(&scene, &layout, &target)
        .unwrap();
    assert_eq!(delegate.path, [0, 0, 0, 1, 1, 0]);
    assert!(ui.dispatch_at(&scene, &delegate.path, "clicked"));
    assert_eq!(ui.get_last_action(), "显示最近文件 · 2");
}

#[test]
fn generated_tray_menu_reuses_action_bindings_and_callback_routes() {
    use std::{cell::Cell, rc::Rc};
    use tray_ui::TrayDemo;

    let mut tray = TrayDemo::new();
    let opened = Rc::new(Cell::new(false));
    let opened_by_menu = opened.clone();
    tray.on_open_window(move || opened_by_menu.set(true));
    let scene = tray.render_shared();
    assert_eq!(scene.kind, "SystemTrayIcon");
    assert_eq!(scene.children[0].kind, "Menu");
    assert!(!scene.children[0].children[0].boolean("checked", false));
    assert!(tray.control_at(&scene, &[0, 0], ControlAction::ActivateMenuItem));
    assert!(tray.get_paused());
    let scene = tray.render_shared();
    assert_eq!(scene.text("tooltip"), Some("XRack is paused"));
    assert!(scene.children[0].children[0].boolean("checked", false));
    assert!(tray.control_at(&scene, &[0, 1], ControlAction::ActivateMenuItem));
    assert!(opened.get());
    opened.set(false);
    assert!(tray.dispatch_at(&scene, &[], "clicked"));
    assert!(opened.get());
    assert!(!tray.control_at(&scene, &[0, 2, 0], ControlAction::ActivateMenuItem));
}

#[test]
fn changing_locale_refreshes_the_tray_and_nested_window_bindings_without_resetting_state() {
    use tray_ui::{TrayDemo, TrayWindow};
    use xrack_runtime::LocaleContext;

    let locale = LocaleContext::default();
    let tray = TrayDemo::new_with_locale(locale.clone());
    let mut window = TrayWindow::new();
    let label = |window: &TrayWindow| {
        window
            .render()
            .find("counter")
            .unwrap()
            .text("text")
            .unwrap()
            .to_owned()
    };
    // Resolve the scene before mounting, so the child must move its existing
    // language subscription to the context supplied by the desktop.
    window.set_count(1);
    assert!(label(&window).ends_with(" click"));
    window.set_locale_context(locale.clone());
    window.set_count(3);
    assert!(label(&window).ends_with(" clicks"));
    assert_eq!(
        tray.render_shared().text("tooltip"),
        Some("XRack is running")
    );

    locale.set_locale("zh-CN").unwrap();
    let scene = window.render_shared();
    assert_eq!(scene.text("title"), Some("XRack · 系统托盘"));
    assert_eq!(
        scene.find("hint").unwrap().text("text"),
        Some("The tray icon stays open when this window closes.")
    );
    assert!(label(&window).contains('3') && label(&window).ends_with("次"));
    assert_eq!(window.get_count(), 3);
    let tray = tray.render_shared();
    assert_eq!(tray.text("tooltip"), Some("XRack 正在运行"));
    assert_eq!(
        tray.children[0].children[1].text("text"),
        Some("打开 XRack 窗口")
    );

    locale.set_locale("en-US").unwrap();
    assert!(label(&window).ends_with(" clicks"));
}

#[test]
fn generated_flickable_cancel_flick_stops_its_pending_inertia() {
    use layout_ui::FlickableDemo;
    use xrack_runtime::ScrollRequests;

    let mut ui = FlickableDemo::new();
    let scene = ui.render_shared();
    let viewport = Size::new(360.0, 280.0);
    let now = Instant::now();
    let mut input = Interaction::default();
    let frame = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let start = Point { x: 140.0, y: 122.0 };
    let end = Point { x: 140.0, y: 66.0 };
    assert!(input.begin_scroll_gesture(&scene, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &frame, end, now + Duration::from_millis(20)));
    assert!(ui.get_dragging() && ui.get_moving() && !ui.get_flicking());
    let frame = xrack_layout::layout_with_input(&scene, viewport, &mut FixedText, &input);
    input.end_scroll_gesture(&scene, &frame, end, now + Duration::from_millis(28));
    assert!(!ui.get_dragging() && ui.get_flicking() && ui.get_moving());
    let requests = ScrollRequests::default();
    let _context = requests.enter();
    ui.invoke_stop();
    assert!(input.apply_scroll_requests(&scene, &frame, requests.drain()));
    assert!(!ui.get_flicking() && !ui.get_moving());
    assert!(!input.advance_scroll_motion(&scene, &frame, now + Duration::from_millis(60)));
}

#[test]
fn attached_scroll_bar_snaps_live_and_keeps_nondivisible_endpoints_reachable() {
    use list_ui::ScrollBarsDemo;
    use xrack_runtime::{Length, ScrollSnapMode};

    let mut ui = UiDriver::new(ScrollBarsDemo::new(), FixedText, Size::new(352.0, 272.0));
    ui.update(|component| component.set_snap_mode(ScrollSnapMode::Always));
    assert!(ui.drag_scroll_bar_with("vertical-bar", 0.34, false, |ui| {
        assert!((ui.component().get_vertical().0 + 117.6).abs() < 0.001);
        assert_eq!(ui.component().get_scrolls(), 1);
        true
    }));
    assert_eq!(ui.component().get_scrolls(), 1);
    assert!(ui.drag_scroll_bar("vertical-bar", 0.98, true));
    assert_eq!(ui.component().get_vertical(), Length(-392.0));
    assert_eq!(ui.component().get_scrolls(), 2);
    assert_eq!(ui.scroll_bar_key(NavigationKey::Up), Some(true));
    assert!((ui.component().get_vertical().0 + 215.6).abs() < 0.001);
}

#[test]
fn numeric_controls_adjust_by_wheel_and_yield_to_the_viewport_at_boundaries() {
    use controls_ui::WheelControlsDemo;
    use xrack_layout::WheelAdjustment;
    use xrack_runtime::{KeyModifiers, Length, WheelEvent};

    let mut ui = WheelControlsDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(460.0, 310.0);
    let now = Instant::now();
    let prepare = |ui: &WheelControlsDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    let mut time = now;
    let mut send = |ui: &mut WheelControlsDemo,
                    input: &mut Interaction,
                    path: &[usize],
                    x: f32,
                    event: WheelEvent| {
        let (scene, layout) = prepare(ui, input);
        let bounds = layout.at_path(path).unwrap().bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bounds.x + bounds.width * x,
                y: bounds.y + bounds.height * 0.5,
            },
        );
        let adjustment = input.wheel_adjustment(&scene, &layout, event, time);
        time += Duration::from_millis(10);
        match &adjustment {
            Some(WheelAdjustment::Range(input)) => {
                assert!(ui.adjust_range_at(&scene, &input.path, input.action))
            }
            Some(WheelAdjustment::Spin(input)) => {
                assert!(ui.control_at(&scene, &input.path, input.action))
            }
            _ => {}
        }
        adjustment
    };
    let slider = [1, 0, 1];
    let half = WheelEvent {
        delta_y: Length(-14.0),
        ..Default::default()
    };
    assert!(matches!(
        send(&mut ui, &mut input, &slider, 0.5, half),
        Some(WheelAdjustment::Pending)
    ));
    assert_eq!(ui.get_slider_value(), 4.0);
    assert!(matches!(
        send(&mut ui, &mut input, &slider, 0.5, half),
        Some(WheelAdjustment::Range(_))
    ));
    assert_eq!(ui.get_slider_value(), 5.0);
    assert_eq!((ui.get_changes(), ui.get_releases()), (1, 0));
    assert!(send(&mut ui, &mut input, &slider, 0.5, half).is_none());

    let lines = |count: f64| WheelEvent {
        is_line: true,
        lines_y: -count,
        delta_y: Length(-28.0 * count),
        ..Default::default()
    };
    let interval = [1, 0, 3];
    let adjusted = send(&mut ui, &mut input, &interval, 0.8, lines(-1.0));
    assert!(
        matches!(adjusted, Some(WheelAdjustment::Range(input)) if input.path.last() == Some(&1))
    );
    assert_eq!((ui.get_interval_start(), ui.get_interval_end()), (2.0, 7.0));
    send(&mut ui, &mut input, &[1, 0, 4], 0.5, lines(3.0));
    assert_eq!(ui.get_dial_value(), 1.0);
    send(&mut ui, &mut input, &[1, 0, 5], 0.5, lines(2.0));
    assert_eq!(ui.get_spin_value(), 14);
    send(&mut ui, &mut input, &[1, 0, 6], 0.5, lines(2.0));
    assert_eq!(ui.get_decimal_value(), 0.0);
    assert_eq!(ui.get_edits(), 2);
    ui.set_spin_read_only(true);
    assert!(send(&mut ui, &mut input, &[1, 0, 5], 0.5, lines(1.0)).is_none());
    assert!(
        send(
            &mut ui,
            &mut input,
            &slider,
            0.5,
            WheelEvent {
                modifiers: KeyModifiers {
                    shift: true,
                    ..Default::default()
                },
                ..lines(-1.0)
            }
        )
        .is_none()
    );
    ui.set_slider_value(0.0);
    assert!(send(&mut ui, &mut input, &slider, 0.5, lines(-1.0)).is_none());
    let (_, layout) = prepare(&ui, &input);
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 28.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&28.0));

    ui.set_wheel_enabled(false);
    assert!(send(&mut ui, &mut input, &interval, 0.8, lines(1.0)).is_none());
}

#[test]
fn release_snapping_respects_touch_cancellation_and_live_control_state() {
    use list_ui::StandaloneScrollDemo;

    let mut ui = UiDriver::new(
        StandaloneScrollDemo::new(),
        FixedText,
        Size::new(352.0, 272.0),
    );
    ui.click("snap-toggle");
    assert!(ui.drag_scroll_bar_with("bar", 0.36, true, |ui| {
        assert!((ui.component().get_position() - 0.27).abs() < 0.000001);
        assert_eq!(ui.component().get_changes(), 1);
        true
    }));
    assert_eq!(ui.component().get_position(), 0.28125);
    assert_eq!(ui.component().get_changes(), 2);

    assert!(ui.drag_scroll_bar_with("bar", 0.18, true, |_| false));
    assert!((ui.component().get_position() - 0.135).abs() < 0.000001);
    assert_eq!(ui.component().get_changes(), 3);
    assert!(ui.drag_scroll_bar_with("bar", 0.64, false, |ui| {
        ui.update(|component| component.set_interactive(false));
        true
    }));
    assert!((ui.component().get_position() - 0.48).abs() < 0.000001);
    assert_eq!(ui.component().get_changes(), 4);
    assert!(!ui.input().scroll_bar_captured());
}

#[test]
fn standalone_scroll_controls_update_bindings_and_clamp_the_live_range() {
    use list_ui::StandaloneScrollDemo;

    let mut ui = UiDriver::new(
        StandaloneScrollDemo::new(),
        FixedText,
        Size::new(352.0, 272.0),
    );
    let bar = ui.scroll_bar("bar").unwrap();
    assert!((bar.thumb.height - bar.track.height * 0.4).abs() < 0.001);
    assert!(!ui.drag_scroll_bar("indicator", 1.0, true));
    assert!(ui.drag_scroll_bar("bar", 1.0, false));
    assert_eq!(ui.component().get_offset(), 0.75);
    assert_eq!(ui.component().get_last_change(), 0.75);
    assert_eq!(ui.component().get_changes(), 1);
    assert_eq!(ui.scroll_bar("indicator").unwrap().scroll.offset, 0.75);
    assert_eq!(ui.scroll_bar_key(NavigationKey::End), Some(false));
    assert_eq!(ui.scroll_bar_key(NavigationKey::Up), Some(true));
    assert!((ui.component().get_position() - 0.625).abs() < 0.000001);
    assert!(ui.drag_scroll_bar("bar", 0.0, true));
    assert_eq!(ui.component().get_position(), 0.0);
    ui.click("bar");
    assert!((ui.component().get_position() - 0.25).abs() < 0.000001);

    ui.update(|component| component.invoke_jump());
    assert!((ui.component().get_before_jump() - 0.25).abs() < 0.000001);
    assert_eq!(ui.component().get_after_jump(), 0.5);
    assert_eq!(ui.component().get_changes(), 4);
    ui.update(|component| component.set_thumb_size(0.75));
    assert_eq!(ui.component().get_size(), 0.75);
    assert_eq!(ui.component().get_position(), 0.25);
    assert_eq!(ui.component().get_offset(), 0.5);
    ui.update(|component| component.set_interactive(false));
    assert!(!ui.drag_scroll_bar("bar", 0.0, false));
    assert_eq!(ui.component().get_position(), 0.25);
    ui.update(|component| component.set_thumb_size(1.5));
    assert_eq!(ui.component().get_size(), 1.0);
    assert_eq!(ui.component().get_position(), 0.0);
    assert!(ui.scroll_bar("bar").is_none());
    assert!(ui.drag_scroll_bar("horizontal-bar", 1.0, false));
    assert_eq!(ui.component().get_horizontal_position(), 0.5);
    assert_eq!(ui.component().get_offset(), 0.5);
}

#[test]
fn external_scroll_bars_share_offsets_with_passive_indicators_and_the_viewport() {
    use list_ui::ScrollBarsDemo;
    use xrack_runtime::Length;

    let mut ui = UiDriver::new(ScrollBarsDemo::new(), FixedText, Size::new(352.0, 272.0));
    assert!((ui.component().get_vertical_size() - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(ui.component().get_vertical_position(), 0.0);
    ui.tab();
    assert_eq!(ui.focused(), Some("vertical-bar"));
    assert!(!ui.drag_scroll_bar("vertical-indicator", 1.0, true));
    assert_eq!(ui.component().get_vertical(), Length(0.0));

    assert!(ui.drag_scroll_bar("vertical-bar", 1.0, false));
    assert_eq!(ui.component().get_vertical(), Length(-392.0));
    assert_eq!(ui.component().get_scrolls(), 1);
    assert!((ui.component().get_vertical_position() - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(
        ui.component().get_last_scroll_position(),
        ui.component().get_vertical_position()
    );
    let bar = ui.scroll_bar("vertical-bar").unwrap();
    let indicator = ui.scroll_bar("vertical-indicator").unwrap();
    assert_eq!(bar.scroll, indicator.scroll);
    assert_eq!(bar.thumb.y, indicator.thumb.y);
    assert_eq!(ui.scroll_bar_key(NavigationKey::PageUp), Some(true));
    assert_eq!(ui.component().get_vertical(), Length(-196.0));
    assert_eq!(ui.component().get_scrolls(), 2);

    assert!(ui.drag_scroll_bar("horizontal-bar", 1.0, true));
    assert_eq!(ui.focused(), Some("horizontal-bar"));
    assert_eq!(ui.component().get_horizontal(), Length(-260.0));
    assert_eq!(ui.component().get_vertical(), Length(-196.0));
    assert_eq!(ui.component().get_scrolls(), 3);
    ui.update(|component| component.set_readout_axis(Orientation::Horizontal));
    assert_eq!(ui.component().get_readout_size(), 0.5);
    assert_eq!(ui.component().get_readout_position(), 0.5);
    assert!(ui.scroll("canvas", Point { x: 0.0, y: 36.0 }));
    assert_eq!(
        ui.scroll_bar("vertical-indicator").unwrap().scroll.offset,
        232.0
    );
    assert!((ui.component().get_vertical_position() - 232.0 / 588.0).abs() < 1e-12);
    assert_eq!(ui.component().get_readout_position(), 0.5);

    ui.update(|component| component.set_scroll_enabled(false));
    assert!(!ui.drag_scroll_bar("vertical-bar", 1.0, false));
    ui.update(|component| {
        component.set_scroll_enabled(true);
        component.set_canvas_height(Length(196.0));
    });
    assert!(ui.scroll_bar("vertical-bar").is_none());
    assert!(ui.scroll_bar("vertical-indicator").is_none());
    assert!(ui.scroll_bar("horizontal-bar").is_some());
    assert_eq!(ui.component().get_vertical_size(), 1.0);
    assert_eq!(ui.component().get_vertical_position(), 0.0);
    ui.update(|component| component.set_canvas_visible(false));
    assert_eq!(ui.component().get_readout_size(), 0.0);
    assert_eq!(ui.component().get_readout_position(), 0.0);
    ui.update(|component| component.set_canvas_visible(true));
    assert_eq!(ui.component().get_readout_size(), 0.5);
    assert_eq!(ui.component().get_readout_position(), 0.5);
}

#[test]
fn nested_virtual_lists_keep_scroll_focus_and_actions_with_their_outer_row() {
    use list_ui::NestedListsDemo;
    use xrack_runtime::Orientation;

    let mut ui = NestedListsDemo::new();
    ui.set_albums((0..10_000).map(|index| format!("album {index}")).collect());
    ui.set_tracks((0..50_000).map(|index| format!("track {index}")).collect());
    let albums = ui.model_albums();
    let mut input = Interaction::default();
    let outer = [1];
    let first = [1, 0, 0, 1];
    let second = [1, 1, 0, 1];
    let button = [1, 0, 0, 1, 0, 0];
    let viewport = Size::new(580.0, 330.0);
    let prepare = |ui: &NestedListsDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };

    let (initial, layout) = prepare(&ui, &input);
    assert_eq!(initial.at_path(&outer).unwrap().children.len(), 2);
    let one = initial.at_path(&first).unwrap();
    let two = initial.at_path(&second).unwrap();
    assert_eq!(one.list.unwrap().row_count, 50_000);
    assert_eq!(one.children.len(), 4);
    assert_eq!(two.children.len(), 4);
    assert_eq!(one.children[0].row_key, two.children[0].row_key);
    assert_ne!(
        initial.row_key_at(&button),
        initial.row_key_at(&[1, 1, 0, 1, 0, 0])
    );
    assert!(ui.dispatch_at(&initial, &button, "clicked"));
    assert_eq!(ui.get_last_played(), "album 0 · track 0");

    let point = layout.at_path(&first).unwrap().bounds;
    input.pointer_move(
        &initial,
        &layout,
        Point {
            x: point.x + 12.0,
            y: point.y + 8.0,
        },
    );
    assert!(input.scroll_y(&layout, 72.0));
    assert!(!input.scroll_offsets().contains_key(outer.as_slice()));
    let (scrolled, layout) = prepare(&ui, &input);
    input.reconcile(&scrolled, &layout);
    assert_eq!(scrolled.at_path(&first).unwrap().list.unwrap().first_row, 3);
    assert_eq!(
        scrolled.at_path(&second).unwrap().list.unwrap().first_row,
        0
    );

    assert!(input.accessibility_scroll_to(&layout, &outer, Orientation::Vertical, 140.0));
    let (next, layout) = prepare(&ui, &input);
    assert_eq!(next.at_path(&outer).unwrap().list.unwrap().first_row, 1);
    assert_eq!(next.at_path(&first).unwrap().list.unwrap().first_row, 0);
    assert!(input.accessibility_scroll_to(&layout, &outer, Orientation::Vertical, 0.0));
    let (restored, layout) = prepare(&ui, &input);
    assert_eq!(restored.at_path(&first).unwrap().list.unwrap().first_row, 3);

    let target = input
        .accessibility_target(&restored, &layout, &button)
        .unwrap();
    input.focus_target(&restored, &layout, target.clone());
    albums.insert(0, "inserted".into());
    assert!(!ui.dispatch_at(&restored, &button, "clicked"));
    let (shifted, layout) = prepare(&ui, &input);
    input.reconcile(&shifted, &layout);
    let focus = input.focused.as_ref().unwrap();
    assert_eq!(focus.path, [1, 1, 0, 1, 0, 0]);
    assert_eq!(focus.row_key, target.row_key);
    assert_eq!(shifted.at_path(&first).unwrap().list.unwrap().first_row, 0);
    assert_eq!(shifted.at_path(&second).unwrap().list.unwrap().first_row, 3);
    assert!(ui.dispatch_at(&shifted, &focus.path, "clicked"));
    assert_eq!(ui.get_last_played(), "album 0 · track 3");

    assert!(input.accessibility_scroll_to(&layout, &second, Orientation::Vertical, 0.0));
    let (restored, _) = prepare(&ui, &input);
    assert_eq!(
        restored.at_path(&[1, 1, 0, 1, 0, 0]).unwrap().text("text"),
        Some("track 0 · 1")
    );
    let (_, layout) = prepare(&ui, &input);
    assert!(input.accessibility_scroll_to(&layout, &second, Orientation::Vertical, 72.0));
    let (anchored, layout) = prepare(&ui, &input);
    input.reconcile(&anchored, &layout);
    let before = anchored.at_path(&second).unwrap();
    let key = before.children[0].row_key;
    ui.model_tracks().insert(0, "new track".into());
    let (anchored, _) = prepare(&ui, &input);
    let after = anchored.at_path(&second).unwrap();
    assert_eq!(after.children[0].row_key, key);
    assert_eq!(after.list.unwrap().first_row, 4);
}

#[test]
fn source_table_scroll_follows_the_captured_record_through_insertion_and_sorting() {
    use table_ui::{RowVisibilityDemo, RowVisibilityFlags, RowVisibilityRecord};
    use xrack_runtime::{ControlAction, ScrollRequests};

    let mut ui = RowVisibilityDemo::new();
    ui.set_records(
        (0..80)
            .map(|index| RowVisibilityRecord {
                name: format!("Record {index}"),
                rank: 80 - index,
                flags: RowVisibilityFlags {
                    shown: index % 7 != 1,
                },
            })
            .collect(),
    );
    let model = ui.model_records();
    let source = 51;
    let key = model.row_key(source).unwrap();
    let mut input = Interaction::default();
    let requests = ScrollRequests::default();
    let viewport = Size::new(720.0, 320.0);
    let prepare = |ui: &RowVisibilityDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    {
        let _active = requests.enter();
        ui.invoke_locate_source_row(source as i64);
        ui.invoke_locate_source_cell(source as i64, 1);
    }
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(&scene, &[2], ControlAction::SortColumn(1)));
    model.insert(
        0,
        RowVisibilityRecord {
            name: "Inserted".into(),
            rank: 100,
            flags: RowVisibilityFlags { shown: true },
        },
    );
    let shifted = model.row_index(key).unwrap();
    assert_eq!(shifted, source + 1);
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let (scene, layout) = prepare(&ui, &input);
    for path in [[1, 0], [2, 0]] {
        assert_eq!(scene.at_path(&path).unwrap().row_key, Some(key));
    }
    assert!(
        layout
            .at_path(&[2])
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .horizontal
            .offset
            > 0.0
    );

    {
        let _active = requests.enter();
        ui.invoke_locate_source_cell(shifted as i64, 1);
    }
    let mut target = model.row(shifted).unwrap();
    target.flags.shown = false;
    model.set_row(shifted, target);
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    assert!(!input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let table = layout.at_path(&[2]).unwrap();
    assert_eq!(table.scroll.unwrap().offset, 0.0);
    assert_eq!(table.table.as_ref().unwrap().horizontal.offset, 0.0);

    {
        let _active = requests.enter();
        ui.invoke_locate_source_cell(shifted as i64, 1);
    }
    model.remove(shifted);
    let (scene, layout) = prepare(&ui, &input);
    assert!(!input.apply_scroll_requests(&scene, &layout, requests.drain()));
}

#[test]
fn source_selection_maps_the_live_list_and_tree_projections() {
    use list_ui::ListVisibilityDemo;
    use tree_ui::VisibilityTreeDemo;

    let mut list = ListVisibilityDemo::new();
    assert_eq!(list.get_visible_index(), 1);
    assert_eq!(list.get_selected_source(), 0);
    list.invoke_select_source(2);
    assert_eq!(list.get_standard_index(), 1);
    assert_eq!(list.get_selected_source(), 2);
    let scene = list.render_shared();
    for path in [[1], [2], [3]] {
        assert_eq!(scene.at_path(&path).unwrap().int("current-index"), Some(1));
    }
    list.invoke_select_source(1);
    assert_eq!(list.get_standard_index(), 1);
    let model = list.model_records();
    let mut preceding = model.row(0).unwrap();
    preceding.flags.shown = false;
    model.set_row(0, preceding);
    assert_eq!(list.get_visible_index(), 0);
    assert_eq!(list.get_selected_source(), 3);
    list.invoke_select_source(2);
    assert_eq!(list.get_selected_source(), 2);

    let mut tree = VisibilityTreeDemo::new();
    assert_eq!(tree.get_visible_index(), -1);
    assert_eq!(tree.get_source_index(), 4);
    tree.invoke_select_source(2);
    assert_eq!(tree.get_current(), -1);
    tree.invoke_initialize();
    assert_eq!(tree.get_visible_index(), 2);
    assert_eq!(tree.get_source_index(), 1);
    tree.invoke_select_source(2);
    assert_eq!(tree.get_current(), 3);
    let (scene, _) = prepare_scene_with_input(
        &tree,
        tree.render_shared(),
        Size::new(540.0, 312.0),
        &mut FixedText,
        &Interaction::default(),
    );
    let selection = scene
        .at_path(&[1])
        .unwrap()
        .tree_selection
        .as_ref()
        .unwrap();
    assert_eq!(selection.keys().copied().collect::<Vec<_>>(), [3]);

    let model = tree.model_records();
    let mut branch = model.row(1).unwrap();
    branch.visibility.shown = false;
    model.set_row(1, branch);
    assert_eq!(tree.get_visible_index(), -1);
    assert_eq!(tree.get_source_index(), 3);
    tree.invoke_select_source(3);
    assert_eq!(tree.get_current(), 4);
    tree.invoke_select_source(2);
    assert_eq!(tree.get_current(), 4);
}

#[test]
fn source_cell_selection_follows_table_sorting_and_filters() {
    use table_ui::RowVisibilityDemo;
    use xrack_runtime::ControlAction;

    let mut ui = RowVisibilityDemo::new();
    let model = ui.model_records();
    let first = model.row_key(0).unwrap();
    let second = model.row_key(1).unwrap();
    ui.set_inspected_source(0);
    assert_eq!(ui.get_custom_visible(), 0);
    assert_eq!(ui.get_standard_visible(), 0);
    assert_eq!(ui.get_standard_source(), 0);
    let (scene, _) = prepare_scene_with_input(
        &ui,
        ui.render_shared(),
        Size::new(720.0, 320.0),
        &mut FixedText,
        &Interaction::default(),
    );
    assert!(ui.control_at(&scene, &[2], ControlAction::SortColumn(1)));
    assert_eq!(ui.get_custom_visible(), 0);
    assert_eq!(ui.get_standard_visible(), 1);
    assert_eq!(ui.get_standard_source(), 1);
    ui.invoke_select_source(0, 1);
    assert!(
        ui.table_selection("standard")
            .unwrap()
            .contains_cell(first, 1)
    );

    ui.set_query("工作站".into());
    assert_eq!(ui.get_standard_visible(), -1);
    assert_eq!(ui.get_standard_source(), 1);
    ui.invoke_select_source(1, 0);
    ui.invoke_select_source(0, 1);
    ui.invoke_select_source(-1, 1);
    assert!(
        ui.table_selection("standard")
            .unwrap()
            .contains_cell(second, 0)
    );
    ui.set_query("".into());
    let mut record = model.row(0).unwrap();
    record.flags.shown = false;
    model.set_row(0, record);
    assert_eq!(ui.get_custom_visible(), -1);
    assert_eq!(ui.get_standard_visible(), -1);
    ui.set_inspected_view(12);
    assert_eq!(ui.get_standard_source(), -1);
}

#[test]
fn source_list_scroll_resolves_the_live_projection_and_rejects_replaced_models() {
    use list_ui::{ListVisibilityDemo, ListVisibilityFlags, ListVisibilityRecord};
    use xrack_runtime::{Length, ScrollRequests, VecModel};

    let records: Vec<_> = (0..64)
        .map(|index| ListVisibilityRecord {
            label: format!("Record {index}"),
            height: Length(24.0),
            flags: ListVisibilityFlags {
                shown: index % 4 != 1,
            },
        })
        .collect();
    let mut ui = ListVisibilityDemo::new();
    ui.set_records(records.clone());
    let model = ui.model_records();
    let key = model.row_key(30).unwrap();
    let requests = ScrollRequests::default();
    let viewport = Size::new(660.0, 320.0);
    let mut input = Interaction::default();
    let prepare = |ui: &ListVisibilityDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    {
        let _active = requests.enter();
        ui.invoke_locate_source(30);
    }
    let mut first = model.row(0).unwrap();
    first.flags.shown = false;
    model.set_row(0, first);
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let (scene, _) = prepare(&ui, &input);
    for view in 1..=3 {
        let view = scene.at_path(&[view]).unwrap();
        assert!(view.children.iter().any(|row| row.row_key == Some(key)));
    }
    {
        let _active = requests.enter();
        ui.invoke_locate_source(30);
    }
    ui.bind_model_records(VecModel::from(records));
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    assert!(!input.apply_scroll_requests(&scene, &layout, requests.drain()));
}

#[test]
fn source_tree_scroll_resolves_a_queued_row_after_expansion_and_insertion() {
    use tree_ui::{BranchVisibility, VisibilityBranch, VisibilityTreeDemo};
    use xrack_runtime::ScrollRequests;

    let mut ui = VisibilityTreeDemo::new();
    let model = ui.model_records();
    for key in 6..40 {
        model.push(VisibilityBranch {
            key,
            parent: -1,
            label: format!("Row {key}"),
            visibility: BranchVisibility { shown: true },
        });
    }
    let key = model.row_key(2).unwrap();
    let requests = ScrollRequests::default();
    {
        let _active = requests.enter();
        ui.invoke_locate_source(2);
    }
    ui.invoke_initialize();
    model.insert(
        0,
        VisibilityBranch {
            key: 100,
            parent: -1,
            label: "Inserted".into(),
            visibility: BranchVisibility { shown: true },
        },
    );
    let viewport = Size::new(540.0, 312.0);
    let mut input = Interaction::default();
    let prepare = |ui: &VisibilityTreeDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&[1, 0]).unwrap().row_key, Some(key));
    assert_eq!(layout.at_path(&[1]).unwrap().scroll.unwrap().offset, 84.0);
    {
        let _active = requests.enter();
        ui.invoke_locate_source(3);
    }
    let mut branch = model.row(2).unwrap();
    branch.visibility.shown = false;
    model.set_row(2, branch);
    let (scene, layout) = prepare(&ui, &input);
    assert!(!input.apply_scroll_requests(&scene, &layout, requests.drain()));
}

#[test]
fn filtered_lists_route_edits_and_grid_state_by_source_record() {
    use list_ui::ListVisibilityDemo;
    use xrack_runtime::{ControlAction, ListMetrics};

    let mut ui = ListVisibilityDemo::new();
    let model = ui.model_records();
    let viewport = Size::new(660.0, 320.0);
    let mut input = Interaction::default();
    let prepare = |ui: &ListVisibilityDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, _) = prepare(&ui, &input);
    for slot in 1..=3 {
        let view = scene.at_path(&[slot]).unwrap();
        assert_eq!(view.list.unwrap().row_count, 3);
        assert_eq!(view.children[1].row_key, model.row_key(2));
    }
    assert_eq!(
        ListMetrics::row_height_at(scene.at_path(&[1]).unwrap(), 1),
        52.0
    );
    assert_eq!(
        ListMetrics::row_height_at(scene.at_path(&[3]).unwrap(), 1),
        52.0
    );
    let tile = [2, 1, 0];
    assert!(ui.dispatch_at(&scene, &tile, "clicked"));
    assert_eq!(ui.get_last_source_index(), 2);
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&tile).unwrap().text("text"), Some("图片 · 1"));

    assert!(ui.dispatch_at(&scene, &[1, 1, 0], "clicked"));
    assert!(!model.row(2).unwrap().flags.shown);
    assert!(!model.row(1).unwrap().flags.shown);
    assert_eq!(ui.get_last_source_index(), 2);
    assert!(!ui.dispatch_at(&scene, &tile, "clicked"));
    assert!(!ui.control_at(&scene, &[3], ControlAction::Select(1)));

    let (scene, layout) = prepare(&ui, &input);
    for slot in 1..=3 {
        let view = scene.at_path(&[slot]).unwrap();
        assert_eq!(view.list.unwrap().row_count, 2);
        assert_eq!(view.children[1].row_key, model.row_key(3));
    }
    let target = input.accessibility_target(&scene, &layout, &[3]).unwrap();
    input.focus_target(&scene, &layout, target);
    let next = input
        .list_key(&scene, &layout, NavigationKey::Down)
        .unwrap();
    assert_eq!(next.action, ControlAction::Select(1));
    assert!(ui.control_at(&scene, &next.path, next.action));
    assert_eq!(ui.get_standard_index(), 1);

    let mut record = model.row(2).unwrap();
    record.flags.shown = true;
    model.set_row(2, record);
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&tile).unwrap().text("text"), Some("图片 · 1"));
    assert_eq!(model.len(), 4);
}

#[test]
fn filtered_list_scroll_anchor_survives_a_hidden_preceding_record() {
    use list_ui::{ListVisibilityDemo, ListVisibilityFlags, ListVisibilityRecord};
    use xrack_runtime::{Length, Orientation};

    let mut ui = ListVisibilityDemo::new();
    ui.set_records(
        (0..64)
            .map(|index| ListVisibilityRecord {
                label: format!("Row {index}"),
                height: Length(24.0),
                flags: ListVisibilityFlags { shown: true },
            })
            .collect(),
    );
    let model = ui.model_records();
    let path = [1];
    let viewport = Size::new(660.0, 320.0);
    let mut input = Interaction::default();
    let prepare = |ui: &ListVisibilityDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (_, layout) = prepare(&ui, &input);
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Vertical, 127.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let anchor = scene.at_path(&path).unwrap().children[0].row_key;
    assert_eq!(anchor, model.row_key(5));
    let mut first = model.row(0).unwrap();
    first.flags.shown = false;
    model.set_row(0, first);
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&path).unwrap().children[0].row_key, anchor);
    assert_eq!(layout.at_path(&path).unwrap().scroll.unwrap().offset, 103.0);
}

#[test]
fn hiding_a_tree_branch_keeps_selection_and_expansion_and_cancels_the_draft() {
    use tree_ui::{BranchVisibility, VisibilityBranch, VisibilityTreeDemo};
    use xrack_runtime::{
        ControlAction, Orientation, RowSelection, TreeAction, TreeSelectionAction,
    };

    let mut ui = VisibilityTreeDemo::new();
    ui.invoke_initialize();
    let model = ui.model_records();
    let path = [1];
    let viewport = Size::new(540.0, 312.0);
    let mut input = Interaction::default();
    let prepare = |ui: &VisibilityTreeDemo, input: &Interaction| {
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
            .map(|item| item.key)
            .collect::<Vec<_>>()
    };
    let leaf = model.row_key(2).unwrap();
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3, 4, 5]);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Row {
            key: 3,
            row: leaf,
            operation: RowSelection::Replace
        }))
    ));
    let (scene, layout) = prepare(&ui, &input);
    let cell = [1, 2, 0, 0];
    let target = input.accessibility_target(&scene, &layout, &cell).unwrap();
    input.focus_target(&scene, &layout, target);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("未提交"),
        EditSource::Keyboard,
        Instant::now()
    ));
    let mut branch = model.row(1).unwrap();
    branch.visibility.shown = false;
    model.set_row(1, branch.clone());
    assert!(!ui.commit_table_cell(&scene, &cell, "计划文档", "过期提交".into()));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(keys(&scene), [1, 4, 5]);
    assert!(input.editor.is_none());
    assert_eq!(model.row(2).unwrap().label, "计划文档");
    assert_eq!(ui.get_current(), 3);
    assert_eq!(
        scene
            .at_path(&path)
            .unwrap()
            .tree_selection
            .as_ref()
            .unwrap()
            .get(&3),
        Some(&leaf)
    );

    ui.set_query("计划".into());
    let (scene, _) = prepare(&ui, &input);
    assert!(keys(&scene).is_empty());
    branch.visibility.shown = true;
    model.set_row(1, branch.clone());
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3]);
    ui.set_query(String::new());
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(keys(&scene), [1, 2, 3, 4, 5]);

    for key in 6..50 {
        model.push(VisibilityBranch {
            key,
            parent: -1,
            label: format!("Row {key}"),
            visibility: BranchVisibility { shown: true },
        });
    }
    let (_, layout) = prepare(&ui, &input);
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Vertical, 287.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let anchor = scene.at_path(&path).unwrap().children[0].row_key;
    branch.visibility.shown = false;
    model.set_row(1, branch);
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&path).unwrap().children[0].row_key, anchor);
    assert_eq!(layout.at_path(&path).unwrap().scroll.unwrap().offset, 231.0);
    ui.set_branch_root(2);
    let (scene, _) = prepare(&ui, &input);
    assert!(keys(&scene).is_empty());
}

#[test]
fn hiding_table_records_keeps_source_keys_selection_heights_and_the_viewport_anchor() {
    use table_ui::{RowVisibilityDemo, RowVisibilityFlags, RowVisibilityRecord};
    use xrack_runtime::{
        ControlAction, Length, ListMetrics, Orientation, RowSelection, TableSelectionAction,
    };

    let mut ui = RowVisibilityDemo::new();
    ui.set_records(
        (0..50)
            .map(|index| RowVisibilityRecord {
                name: format!("{} {index}", if index % 2 == 0 { "Keep" } else { "Other" }),
                rank: 50 - index,
                flags: RowVisibilityFlags { shown: index != 1 },
            })
            .collect(),
    );
    let model = ui.model_records();
    let custom = [1];
    let standard = [2];
    let viewport = Size::new(720.0, 320.0);
    let mut input = Interaction::default();
    let prepare = |ui: &RowVisibilityDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    ui.invoke_resize_row(19, Length(52.0));
    let (scene, layout) = prepare(&ui, &input);
    let view = scene.at_path(&custom).unwrap();
    assert_eq!(view.list.unwrap().row_count, 49);
    assert_eq!(view.children[1].row_key, model.row_key(2));
    assert_eq!(ListMetrics::row_height_at(view, 19), 52.0);
    assert!(!view.boolean("movable-rows", false));
    assert!(input.accessibility_scroll_to(&layout, &custom, Orientation::Vertical, 126.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let anchor = scene.at_path(&custom).unwrap().children[0].row_key;
    let mut first = model.row(0).unwrap();
    first.flags.shown = false;
    model.set_row(0, first);
    assert!(!ui.control_at(&scene, &custom, ControlAction::Select(0)));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let view = scene.at_path(&custom).unwrap();
    assert_eq!(view.children[0].row_key, anchor);
    assert_eq!(
        layout.at_path(&custom).unwrap().scroll.unwrap().offset,
        102.0
    );
    assert_eq!(ListMetrics::row_height_at(view, 18), 52.0);

    let mut resized = model.row(20).unwrap();
    resized.flags.shown = false;
    model.set_row(20, resized.clone());
    let (hidden, _) = prepare(&ui, &input);
    assert_eq!(hidden.at_path(&custom).unwrap().list.unwrap().row_count, 47);
    resized.flags.shown = true;
    model.set_row(20, resized);
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        ListMetrics::row_height_at(scene.at_path(&custom).unwrap(), 18),
        52.0
    );

    assert!(ui.control_at(&scene, &standard, ControlAction::SortColumn(1)));
    ui.set_query(" Other ".into());
    let (scene, _) = prepare(&ui, &input);
    let view = scene.at_path(&standard).unwrap();
    let selected = model.row_key(49).unwrap();
    assert_eq!(view.list.unwrap().row_count, 24);
    assert_eq!(view.children[0].row_key, Some(selected));
    assert!(ui.control_at(
        &scene,
        &standard,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace
        })
    ));
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        ui.copy_table_at(&scene, &standard, &[0]).as_deref(),
        Some("Other 49")
    );
    let mut record = model.row(49).unwrap();
    record.flags.shown = false;
    model.set_row(49, record.clone());
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        scene.at_path(&standard).unwrap().list.unwrap().row_count,
        23
    );
    assert!(
        ui.table_selection("standard")
            .unwrap()
            .contains_cell(selected, 0)
    );
    assert!(ui.copy_table_at(&scene, &standard, &[0]).is_none());
    record.flags.shown = true;
    model.set_row(49, record);
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        scene.at_path(&standard).unwrap().children[0].row_key,
        Some(selected)
    );
    assert_eq!(
        ui.copy_table_at(&scene, &standard, &[0]).as_deref(),
        Some("Other 49")
    );
    assert_eq!(model.len(), 50);
}

#[test]
fn hiding_table_columns_keeps_order_widths_and_selection_by_declaration_index() {
    use table_ui::WideTableDemo;
    use xrack_runtime::{ControlAction, RowSelection, TableSelectionAction};

    let mut ui = WideTableDemo::new();
    let mut input = Interaction::default();
    let path = [1];
    let viewport = Size::new(620.0, 320.0);
    let prepare = |ui: &WideTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 136
        }
    ));
    assert!(ui.control_at(&scene, &path, ControlAction::MoveColumn { from: 11, to: 1 }));
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace
        })
    ));
    ui.set_show_number(false);
    ui.set_show_february(false);
    let (scene, layout) = prepare(&ui, &input);
    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(table.visible_column_count(), 10);
    assert_eq!(
        table.visual_columns().take(3).collect::<Vec<_>>(),
        [0, 11, 1]
    );
    assert_eq!(
        table.visible_columns().take(3).collect::<Vec<_>>(),
        [11, 1, 3]
    );
    assert_eq!(table.visible_column_index(11), Some(0));
    assert!(table.is_frozen(11));
    assert_eq!(table.columns[0].width, 0.0);
    assert!(scene.at_path(&[1, 0, 0, 0]).is_none());
    assert_eq!(
        ui.table_selection("sheet").unwrap().current_column(),
        Some(0)
    );
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target);
    let (_, copied_columns) = input.table_copy_target(&scene, &layout).unwrap();
    assert!(!copied_columns.contains(&0) && !copied_columns.contains(&2));
    assert!(ui.copy_table_at(&scene, &path, &copied_columns).is_none());
    let next = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Right, RowSelection::Move)
        .unwrap();
    assert!(matches!(
        next.action,
        ControlAction::TableSelection(TableSelectionAction::Cell { column: 11, .. })
    ));
    assert!(ui.control_at(&scene, &next.path, next.action));

    ui.set_show_number(true);
    let (scene, layout) = prepare(&ui, &input);
    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert!(table.is_frozen(0));
    assert_eq!(table.columns[0].width, 136.0);
    assert_eq!(table.visible_column_index(11), Some(1));
    assert!(scene.at_path(&[1, 0, 0, 0]).is_some());
    assert_eq!(
        ui.table_selection("sheet").unwrap().current_column(),
        Some(11)
    );
}

#[test]
fn hiding_the_column_with_a_standard_table_draft_cancels_that_edit() {
    use xrack_runtime::{RowSelection, SelectionTarget, TableSelectionAction};

    let mut ui = StandardTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_edit_columns(true);
    ui.set_inventory(vec![InventoryRow {
        name: "Device".into(),
        units: 7,
        ..Default::default()
    }]);
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 0,
            column: 1,
            operation: RowSelection::Replace
        }
    ));
    let viewport = Size::new(620.0, 380.0);
    let mut input = Interaction::default();
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let path = [0, 1];
    let cell = [0, 1, 0, 0, 1];
    let (scene, layout) = prepare(&ui, &input);
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("91"),
        EditSource::Keyboard,
        Instant::now()
    ));
    ui.set_show_quantity(false);
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(scene.at_path(&cell).is_none());
    assert!(input.editor.is_none());
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(ui.model_inventory().row(0).unwrap().units, 7);
    ui.set_show_quantity(true);
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&cell).unwrap().text("text"), Some("7"));
}

#[test]
fn wide_table_projects_only_visible_cells_and_copies_offscreen_columns() {
    use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
    use table_ui::WideTableDemo;
    use xrack_runtime::{ControlAction, Orientation, RowSelection, TableSelectionAction};

    let mut ui = WideTableDemo::new();
    let calls = Rc::new(RefCell::new(BTreeMap::new()));
    let count = calls.clone();
    ui.on_cell_text(move |row, column| {
        *count.borrow_mut().entry((row, column)).or_insert(0) += 1;
        format!("{row}:{column}")
    });
    let table = [1];
    let viewport = Size::new(620.0, 320.0);
    let mut input = Interaction::default();
    let prepare = |ui: &WideTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let sheet = scene.at_path(&table).unwrap();
    let first = &sheet.children[0].children[0];
    let visible = first
        .children
        .iter()
        .filter(|cell| cell.is_visible())
        .count();
    assert_eq!(sheet.list.unwrap().row_count, 50_000);
    assert_eq!(first.children.len(), 5);
    assert_eq!(layout.at_path(&[1, 0, 0]).unwrap().children.len(), 5);
    assert_eq!(
        first.table_column_window.logical_len(first.children.len()),
        12
    );
    assert_eq!(visible, 5);
    assert_eq!(calls.borrow().len(), sheet.children.len() * visible);
    assert!(first.child_at(11).is_none());
    assert!(calls.borrow().keys().all(|&(_, column)| column < 5));

    let maximum = layout
        .at_path(&table)
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .horizontal
        .max_offset;
    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Horizontal, maximum));
    let (scene, layout) = prepare(&ui, &input);
    let first = scene.at_path(&[1, 0, 0]).unwrap();
    assert_eq!(first.child_at(0).unwrap().text("text"), Some("0:0"));
    assert!(first.child_at(1).is_none());
    assert_eq!(first.child_at(11).unwrap().text("text"), Some("0:11"));
    assert_eq!(calls.borrow()[&(0, 0)], 1);

    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Horizontal, 0.0));
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        scene.at_path(&[1, 0, 0, 1]).unwrap().text("text"),
        Some("0:1")
    );
    assert_eq!(calls.borrow()[&(0, 1)], 1);
    assert!(ui.control_at(
        &scene,
        &table,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 400,
            column: 11,
            operation: RowSelection::Replace
        })
    ));
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(
        ui.copy_table_at(&scene, &table, &[11]).as_deref(),
        Some("400:11")
    );
    assert_eq!(
        ui.list_rows(&table, 0..1)[0].children[11].text("text"),
        Some("0:11")
    );
    let (scene, _) = prepare(&ui, &input);
    assert!(scene.at_path(&[1, 0, 0, 11]).is_none());
    assert!(ui.control_at(
        &scene,
        &table,
        ControlAction::MoveColumn { from: 11, to: 1 }
    ));
    let (scene, layout) = prepare(&ui, &input);
    let columns = layout.at_path(&table).unwrap().table.as_ref().unwrap();
    assert_eq!(
        columns.visual_columns().take(2).collect::<Vec<_>>(),
        [0, 11]
    );
    assert_eq!(
        scene.at_path(&[1, 0, 0, 11]).unwrap().text("text"),
        Some("0:11")
    );
    assert!(scene.at_path(&[1, 0, 0, 4]).is_none());
    let far = [1, 0, 0, 11];
    let cell = layout.at_path(&far).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 10.0,
            y: cell.y + 8.0,
        },
    );
    assert_eq!(input.hovered.as_ref().unwrap().path, far);
}

#[test]
fn a_table_cell_delegate_keeps_its_state_after_its_column_leaves_the_viewport() {
    use xrack_runtime::Orientation;

    let mut ui = DelegateTableDemo::new();
    let mut input = Interaction::default();
    let table = [0, 1];
    let star = [0, 1, 0, 0, 0, 1];
    let viewport = Size::new(244.0, 338.0);
    let prepare = |ui: &DelegateTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&star).unwrap().text("text"), Some("★ 0"));
    assert!(ui.dispatch_at(&scene, &star, "clicked"));
    let maximum = layout
        .at_path(&table)
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .horizontal
        .max_offset;
    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Horizontal, maximum));
    let (scene, layout) = prepare(&ui, &input);
    assert!(scene.at_path(&star).is_none());
    assert!(!ui.dispatch_at(&scene, &star, "clicked"));
    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Horizontal, 0.0));
    let (scene, _) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&star).unwrap().text("text"), Some("★ 1"));
}

#[test]
fn nested_table_routes_tab_edit_copy_paste_and_history_to_its_own_instance() {
    use list_ui::NestedDataDemo;
    use xrack_runtime::{ControlAction, RowSelection, TableSelectionAction};

    let mut ui = NestedDataDemo::new();
    let groups = ui.model_groups();
    let records = ui.model_records();
    let mut input = Interaction::default();
    let first = [1, 0, 0, 1];
    let second = [1, 1, 0, 1];
    let viewport = Size::new(720.0, 364.0);
    let prepare = |ui: &NestedDataDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &first,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace
        })
    ));
    let (scene, layout) = prepare(&ui, &input);
    let table = input.accessibility_target(&scene, &layout, &first).unwrap();
    input.focus_target(&scene, &layout, table);
    let (copy_path, columns) = input.table_copy_target(&scene, &layout).unwrap();
    assert_eq!(copy_path, first);
    assert_eq!(
        ui.copy_table_at(&scene, &copy_path, &columns).as_deref(),
        Some("资料")
    );

    assert!(input.begin_table_edit(&scene, &layout));
    let origin = input.tab_edit_origin(&scene).unwrap();
    let next = input
        .next_table_edit_target(&ui, &scene, &layout, &origin, false)
        .unwrap();
    assert!(input.cancel_table_edit());
    assert!(input.begin_table_edit_target(&scene, &layout, &next));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("27"),
        EditSource::Keyboard,
        Instant::now()
    ));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(records.row(0).unwrap().quantity, 27);

    let (scene, _) = prepare(&ui, &input);
    let paste = ui
        .prepare_table_paste(&scene, &first, 0, vec![0, 1])
        .unwrap();
    assert!(paste("改名\t18"));
    assert_eq!(records.row(0).unwrap().title, "改名");
    assert_eq!(records.row(0).unwrap().quantity, 18);
    assert!(!ui.replay_table_edit_at(&scene, &second, false));
    assert!(ui.replay_table_edit_at(&scene, &first, false));
    assert_eq!(records.row(0).unwrap().quantity, 27);
    assert_eq!(records.row(0).unwrap().title, "资料");
    assert!(ui.replay_table_edit_at(&scene, &first, true));

    let (scene, _) = prepare(&ui, &input);
    let stale = ui.prepare_table_paste(&scene, &first, 0, vec![0]).unwrap();
    groups.insert(0, "新项目".into());
    assert!(!stale("过期内容"));
    assert!(ui.copy_table_at(&scene, &first, &[0]).is_none());
    let (scene, _) = prepare(&ui, &input);
    assert!(!ui.replay_table_edit_at(&scene, &first, false));
    assert!(ui.replay_table_edit_at(&scene, &second, false));
    assert_eq!(records.row(0).unwrap().title, "资料");
}

#[test]
fn nested_tables_and_trees_keep_scroll_and_expansion_when_outer_rows_move() {
    use list_ui::{NestedDataDemo, WorkspaceRecord};
    use xrack_runtime::Orientation;

    let mut ui = NestedDataDemo::new();
    ui.set_records(
        (1..=40)
            .map(|key| WorkspaceRecord {
                key,
                parent: if (2..=14).contains(&key) { 1 } else { -1 },
                title: format!("Record {key}"),
                quantity: key,
                note: String::new(),
            })
            .collect(),
    );
    let mut input = Interaction::default();
    let first_table = [1, 0, 0, 1];
    let second_table = [1, 1, 0, 1];
    let first_tree = [1, 0, 0, 2];
    let second_tree = [1, 1, 0, 2];
    let viewport = Size::new(720.0, 364.0);
    let prepare = |ui: &NestedDataDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let mut first_row = first_tree.to_vec();
    first_row.push(0);
    let row = scene.at_path(&first_row).unwrap();
    let item = layout.at_path(&first_row).unwrap();
    let indicator = TreeRowGeometry::new(row, item.bounds).unwrap().indicator;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: indicator.x + indicator.width * 0.5,
            y: indicator.y + indicator.height * 0.5,
        },
    );
    let target = input.hovered.as_ref().unwrap();
    let expand = input.tree_pointer_action(&scene, &layout, target).unwrap();
    assert_eq!(expand.path, first_tree);
    assert!(ui.control_at(&scene, &expand.path, expand.action));
    let (scene, layout) = prepare(&ui, &input);
    assert!(
        scene
            .at_path(&first_tree)
            .unwrap()
            .tree
            .as_ref()
            .unwrap()
            .is_expanded(1)
    );
    assert!(
        !scene
            .at_path(&second_tree)
            .unwrap()
            .tree
            .as_ref()
            .unwrap()
            .is_expanded(1)
    );
    assert!(input.accessibility_scroll_to(&layout, &first_table, Orientation::Horizontal, 80.0));
    assert!(input.accessibility_scroll_to(&layout, &first_table, Orientation::Vertical, 48.0));
    assert!(input.accessibility_scroll_to(&layout, &first_tree, Orientation::Vertical, 72.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    ui.model_groups().insert(0, "New".into());
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let offset = |path: &[usize], axis| {
        layout
            .at_path(path)
            .unwrap()
            .scroll_on(axis)
            .unwrap()
            .offset
    };
    assert_eq!(offset(&first_table, Orientation::Horizontal), 0.0);
    assert_eq!(offset(&first_table, Orientation::Vertical), 0.0);
    assert_eq!(offset(&second_table, Orientation::Horizontal), 80.0);
    assert_eq!(offset(&second_table, Orientation::Vertical), 48.0);
    assert_eq!(offset(&second_tree, Orientation::Vertical), 72.0);
    assert!(
        scene
            .at_path(&second_tree)
            .unwrap()
            .tree
            .as_ref()
            .unwrap()
            .is_expanded(1)
    );
    ui.set_branch(1);
    let (scene, layout) = prepare(&ui, &input);
    let tree = scene.at_path(&second_tree).unwrap();
    assert_eq!(tree.list.unwrap().row_count, 13);
    let scroll = layout.at_path(&second_tree).unwrap().scroll.unwrap();
    assert!(scroll.max_offset > 0.0);
    assert_eq!(scroll.offset, 0.0);
}

#[test]
fn list_anchors_preserve_a_partial_row_and_fall_back_to_a_visible_neighbor() {
    use list_ui::AnchoredListDemo;
    use xrack_runtime::Length;

    let mut ui = AnchoredListDemo::new();
    ui.set_records(
        (0..400)
            .map(|rank| DirectoryItem {
                label: rank.to_string(),
                rank,
            })
            .collect(),
    );
    let model = ui.model_records();
    let mut driver = UiDriver::new(ui, FixedText, Size::new(560.0, 274.0));
    driver.update(|ui| ui.invoke_position(10));
    assert!(driver.scroll("anchored", Point { x: 0.0, y: 7.0 }));
    let row = model.row_key(10).unwrap();
    assert_eq!(driver.node("anchored").children[0].row_key, Some(row));
    assert_eq!(driver.component().get_anchored_y(), Length(-327.0));
    driver.update(|_| {
        assert!(model.insert(
            0,
            DirectoryItem {
                label: "new".into(),
                rank: -1
            }
        ))
    });
    assert_eq!(driver.node("anchored").children[0].row_key, Some(row));
    assert_eq!(driver.component().get_anchored_y(), Length(-359.0));
    assert_eq!(driver.component().get_plain_y(), Length(-320.0));

    driver.update(|_| assert!(model.sort_by(|a, b| b.rank.cmp(&a.rank))));
    let index = model.row_index(row).unwrap();
    let offset = index as f64 * 32.0 + 7.0;
    assert_eq!(driver.node("anchored").children[0].row_key, Some(row));
    assert_eq!(driver.component().get_anchored_y(), Length(-offset));
    let neighbor = model.row_key(index + 1).unwrap();
    driver.update(|_| assert!(model.remove(index).is_some()));
    assert_eq!(driver.node("anchored").children[1].row_key, Some(neighbor));
    assert_eq!(
        driver.component().get_anchored_y(),
        Length(-(offset - 32.0))
    );
    assert_eq!(driver.component().get_scroll_events(), 1);
    assert!(driver.node("anchored").children.len() <= 6);
    driver.update(|ui| ui.invoke_position(50));
    assert_eq!(driver.node("anchored").list.unwrap().first_row, 50);
    assert_eq!(driver.component().get_anchored_y(), Length(-1600.0));
    assert!(driver.scroll("anchored", Point { x: 0.0, y: 25.0 }));
    let row = driver.node("anchored").children[0].row_key;
    driver.update(|ui| ui.set_anchor_row_height(Length(12.0)));
    assert_eq!(driver.node("anchored").children[0].row_key, row);
    assert_eq!(driver.component().get_anchored_y(), Length(-611.0));

    driver.update(|ui| ui.set_anchor_comparison(true));
    let plain_row = driver.node("plain").children[0].row_key;
    driver.update(|_| {
        assert!(model.insert(
            0,
            DirectoryItem {
                label: "again".into(),
                rank: -2
            }
        ))
    });
    assert_eq!(driver.node("plain").children[0].row_key, plain_row);
    assert_eq!(driver.component().get_plain_y(), Length(-1632.0));
}

#[test]
fn grid_anchors_preserve_the_visible_record_when_the_column_count_changes() {
    let mut ui = GridDemo::new();
    ui.set_preserve_viewport(true);
    ui.set_records(
        (0..100)
            .map(|rank| DirectoryItem {
                label: rank.to_string(),
                rank,
            })
            .collect(),
    );
    let mut input = Interaction::default();
    let path = [0, 1];
    let wide = Size::new(620.0, 420.0);
    let narrow = Size::new(340.0, 420.0);
    let prepare = |input: &Interaction, viewport| {
        prepare_scene_with_input(&ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&input, wide);
    input.reconcile(&scene, &layout);
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Vertical, 281.0));
    let (scene, layout) = prepare(&input, wide);
    input.reconcile(&scene, &layout);
    let first = scene.at_path(&path).unwrap().children[0].row_key;
    let (scene, layout) = prepare(&input, narrow);
    assert_eq!(scene.at_path(&path).unwrap().children[0].row_key, first);
    assert_eq!(layout.at_path(&path).unwrap().scroll.unwrap().offset, 557.0);
}

#[test]
fn tree_anchors_follow_a_visible_record_when_a_preceding_branch_collapses() {
    let mut ui = TreeDemo::new();
    ui.set_preserve_viewport(true);
    ui.set_records(
        (0..48)
            .map(|id| TreeRecord {
                id,
                parent_id: if (1..11).contains(&id) { 0 } else { -1 },
                label: id.to_string(),
                ..TreeRecord::default()
            })
            .collect(),
    );
    let viewport = Size::new(600.0, 370.0);
    let path = [0, 1];
    let prepare = |ui: &TreeDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    let folder = scene.at_path(&path).unwrap().tree.as_ref().unwrap().rows()[0];
    let expansion = |expanded| {
        ControlAction::Tree(TreeAction::SetExpanded {
            key: folder.key,
            row: folder.row,
            expanded,
        })
    };
    assert!(ui.control_at(&scene, &path, expansion(true)));
    let (_, layout) = prepare(&ui, &input);
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Vertical, 481.0));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let row = scene.at_path(&path).unwrap().children[0].row_key;
    assert!(ui.control_at(&scene, &path, expansion(false)));
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&path).unwrap().children[0].row_key, row);
    assert_eq!(layout.at_path(&path).unwrap().scroll.unwrap().offset, 141.0);
}

#[test]
fn left_drag_claims_the_viewport_only_after_the_click_threshold() {
    use xrack_runtime::Length;

    let mut driver = UiDriver::new(
        ScrollPositionDemo::new(),
        FixedText,
        Size::new(540.0, 300.0),
    );
    assert!(!driver.drag_viewport_from("canvas-button", Point { x: 0.0, y: -3.0 }));
    assert_eq!(driver.component().get_preview_clicks(), 1);
    assert_eq!(driver.component().get_preview_y(), Length(0.0));
    assert!(driver.drag_viewport_from("canvas-button", Point { x: 0.0, y: -48.0 }));
    assert_eq!(driver.component().get_preview_clicks(), 1);
    assert_eq!(driver.component().get_preview_y(), Length(-40.0));
    assert_eq!(driver.component().get_preview_scrolls(), 1);

    driver.click("reset");
    assert!(driver.drag_viewport_from("canvas-button", Point { x: 0.0, y: 40.0 }));
    assert_eq!(driver.component().get_preview_y(), Length(0.0));
    assert_eq!(driver.component().get_preview_scrolls(), 1);
    driver.update(|ui| ui.set_pan_enabled(false));
    assert!(!driver.drag_viewport_from("canvas-button", Point { x: 0.0, y: -48.0 }));
    assert_eq!(driver.component().get_preview_y(), Length(0.0));
    assert_eq!(driver.component().get_preview_clicks(), 1);
}

#[test]
fn shared_scroll_bindings_preserve_requested_position_across_independent_clamps() {
    use list_ui::LinkedScrollDemo;
    use xrack_runtime::Length;

    let mut driver = UiDriver::new(LinkedScrollDemo::new(), FixedText, Size::new(540.0, 270.0));
    assert_eq!(driver.component().get_large_y(), Length(-36.0));
    assert_eq!(driver.component().get_small_y(), Length(-36.0));
    assert_eq!(driver.component().get_follower_y(), Length(-36.0));

    assert!(driver.scroll("large", Point { x: 30.0, y: 20.0 }));
    assert_eq!(driver.component().get_horizontal(), Length(-30.0));
    assert_eq!(driver.component().get_vertical(), Length(-56.0));
    assert_eq!(driver.component().get_small_y(), Length(-56.0));
    assert_eq!(driver.component().get_large_writeback(), Length(-56.0));
    assert_eq!(driver.component().get_large_scrolls(), 1);
    assert_eq!(driver.component().get_small_scrolls(), 0);

    assert!(driver.scroll("follower", Point { x: 0.0, y: 30.0 }));
    assert_eq!(driver.component().get_follower_y(), Length(-86.0));
    assert_eq!(driver.component().get_vertical(), Length(-56.0));
    driver.click("set-deep");
    assert_eq!(driver.component().get_large_y(), Length(-170.0));
    assert_eq!(driver.component().get_small_y(), Length(-60.0));
    assert_eq!(driver.component().get_follower_y(), Length(-86.0));
    assert_eq!(driver.component().get_vertical(), Length(-170.0));

    driver.update(|ui| ui.set_small_content(Length(120.0)));
    assert_eq!(driver.component().get_small_y(), Length(0.0));
    assert_eq!(driver.component().get_vertical(), Length(-170.0));
    driver.update(|ui| ui.set_small_content(Length(300.0)));
    assert_eq!(driver.component().get_small_y(), Length(-170.0));
    assert!(driver.scroll("small", Point { x: 0.0, y: -12.0 }));
    assert_eq!(driver.component().get_large_y(), Length(-158.0));
    assert_eq!(driver.component().get_small_scrolls(), 1);
    driver.click("move-small");
    assert_eq!(driver.component().get_vertical(), Length(-40.0));
    assert_eq!(driver.component().get_large_y(), Length(-40.0));
    assert_eq!(driver.component().get_small_y(), Length(-40.0));
    assert_eq!(driver.component().get_horizontal(), Length(-30.0));
    assert_eq!(driver.component().get_large_scrolls(), 1);
    assert_eq!(driver.component().get_small_scrolls(), 1);
}

#[test]
fn scrolled_observes_the_committed_position_once_for_both_axes() {
    use xrack_runtime::Length;

    let mut driver = UiDriver::new(
        ScrollPositionDemo::new(),
        FixedText,
        Size::new(540.0, 300.0),
    );
    assert_eq!(driver.component().get_preview_scrolls(), 0);
    driver.click("jump");
    assert_eq!(driver.component().get_preview_scrolls(), 0);
    assert_eq!(driver.component().get_records_scrolls(), 0);
    driver.click("reset");
    assert!(driver.scroll("preview", Point { x: 45.0, y: 25.0 }));
    assert_eq!(driver.component().get_preview_scrolls(), 1);
    assert_eq!(driver.component().get_last_preview_x(), Length(-45.0));
    assert_eq!(driver.component().get_last_preview_y(), Length(-25.0));
    assert!(driver.scroll("preview", Point { x: 500.0, y: 500.0 }));
    assert_eq!(driver.component().get_preview_scrolls(), 2);
    assert_eq!(driver.component().get_last_preview_x(), Length(-160.0));
    assert_eq!(driver.component().get_last_preview_y(), Length(-120.0));
    assert!(!driver.scroll("preview", Point { x: 5.0, y: 5.0 }));
    driver.update(|ui| ui.set_preview_enabled(false));
    assert!(!driver.scroll("preview", Point { x: -45.0, y: -25.0 }));
    assert_eq!(driver.component().get_preview_scrolls(), 2);
    assert!(driver.scroll("records", Point { x: 0.0, y: 64.0 }));
    assert_eq!(driver.component().get_records_y(), Length(-64.0));
    assert_eq!(driver.component().get_records_scrolls(), 1);
    assert_eq!(driver.component().get_current(), 0);
}

#[test]
fn programmatic_scroll_moves_virtual_content_and_publishes_clamped_geometry() {
    use xrack_runtime::{Length, ScrollAlignment};

    let mut driver = UiDriver::new(
        ScrollPositionDemo::new(),
        FixedText,
        Size::new(540.0, 300.0),
    );
    assert_eq!(driver.component().get_records_height(), Length(120.0));
    assert_eq!(driver.node("records").list.unwrap().first_row, 0);
    let preview_x = Property::default();
    assert_eq!(
        preview_x.get(|| driver.component().get_preview_x()),
        Length(0.0)
    );
    driver.update(|ui| ui.invoke_scroll_preview(Length(0.0), Length(25.0)));
    assert!(!preview_x.is_dirty());
    assert_eq!(driver.component().get_preview_y(), Length(-25.0));

    driver.click("jump");
    assert_eq!(driver.component().get_preview_x(), Length(-96.0));
    assert_eq!(driver.component().get_preview_y(), Length(-60.0));
    assert_eq!(driver.component().get_records_y(), Length(-215_952.0));
    assert_eq!(driver.component().get_records_target(), Length(-215_952.0));
    assert_eq!(
        driver.node("list-offset").text("text"),
        Some("列表 Y -215952")
    );
    let list = driver.node("records");
    assert_eq!(list.list.unwrap().first_row, 8_998);
    assert!(list.children.len() < 10);
    assert_eq!(driver.component().get_current(), 0);

    driver.update(|ui| ui.invoke_position(9002, ScrollAlignment::Visible));
    assert_eq!(driver.component().get_records_y(), Length(-215_952.0));
    driver.update(|ui| ui.invoke_position(9003, ScrollAlignment::Contain));
    assert_eq!(driver.component().get_records_y(), Length(-215_976.0));
    driver.update(|ui| ui.invoke_position(-1, ScrollAlignment::Beginning));
    assert_eq!(driver.component().get_records_y(), Length(-215_976.0));

    driver.update(|ui| ui.invoke_resize_and_position(18, 17));
    assert_eq!(driver.node("records").list.unwrap().row_count, 18);
    assert_eq!(driver.component().get_records_y(), Length(-312.0));
    driver.update(|ui| ui.invoke_scroll_preview(Length(f64::MAX), Length(-30.0)));
    assert_eq!(driver.component().get_preview_x(), Length(-160.0));
    assert_eq!(driver.component().get_preview_y(), Length(0.0));
    driver.click("reset");
    assert_eq!(driver.component().get_records_y(), Length(0.0));
    assert_eq!(driver.component().get_preview_x(), Length(0.0));
}

#[test]
fn table_cell_positioning_reveals_virtual_rows_without_changing_selection() {
    use xrack_runtime::{Length, RowSelection, ScrollAlignment, TableSelectionAction};
    let mut ui = StandardTableDemo::new();
    ui.set_position_tools(true);
    ui.set_target_row(900);
    ui.set_window_width(Length(500.0));
    ui.set_pinned(1);
    ui.set_row_header_size(Length(56.0));
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        (0..1000)
            .map(|index| InventoryRow {
                name: format!("Device {index}"),
                units: index,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_set_height(0, Length(96.0));
    ui.invoke_set_height(899, Length(80.0));
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 5,
            column: 1,
            operation: RowSelection::Replace,
        }
    ));
    let viewport = Size::new(500.0, 380.0);
    let mut driver = UiDriver::new(ui, FixedText, viewport);
    let selected = driver
        .node("inventory-table")
        .table_selection
        .as_ref()
        .unwrap()
        .cell_keys(1)
        .cloned();
    let changes = driver.component().get_cell_changes();
    driver.click("reveal-last");
    assert!(driver.component().get_table_x().0 < 0.0);
    assert!(driver.component().get_table_y().0 < -28_000.0);
    let table = driver.node("inventory-table");
    let list = table.list.unwrap();
    assert!(list.first_row > 880 && list.first_row <= 900);
    assert!(table.children.len() < 12);
    let layout =
        xrack_layout::layout_with_input(driver.scene(), viewport, &mut FixedText, driver.input());
    let geometry = layout.at_path(&[0, 1]).unwrap().table.as_ref().unwrap();
    let pane = geometry.scrollable_body();
    assert!(geometry.columns[3].x >= pane.x);
    assert!(geometry.columns[3].x + geometry.columns[3].width <= pane.x + pane.width + 0.01);
    let row = layout
        .at_path(&[0, 1, 900 - list.first_row])
        .unwrap()
        .bounds;
    assert!(row.y >= geometry.body.y);
    assert!(row.y + row.height <= geometry.body.y + geometry.body.height + 0.01);
    assert_eq!(driver.component().get_current(), 5);
    assert_eq!(driver.component().get_cell_changes(), changes);
    assert_eq!(
        table
            .table_selection
            .as_ref()
            .unwrap()
            .cell_keys(1)
            .cloned(),
        selected
    );

    let before = (
        driver.component().get_table_x(),
        driver.component().get_table_y(),
    );
    for (row, column) in [(-1, 0), (0, -1), (1000, 0), (0, 4)] {
        driver.update(|ui| {
            ui.invoke_position_cell(
                row,
                column,
                ScrollAlignment::Beginning,
                ScrollAlignment::Beginning,
            )
        });
        assert_eq!(
            (
                driver.component().get_table_x(),
                driver.component().get_table_y()
            ),
            before
        );
    }
    // The same logical column moves to the leading edge of the scrolling pane.
    driver.update(|ui| {
        ui.invoke_move_visible_column(3, 1);
        ui.invoke_position_column(3, ScrollAlignment::Beginning);
    });
    assert_eq!(driver.component().get_table_x(), Length(0.0));
    assert_eq!(driver.component().get_table_y(), before.1);
    driver.update(|ui| ui.invoke_position_column(2, ScrollAlignment::End));
    let x = driver.component().get_table_x();
    assert!(x.0 < 0.0);
    driver.update(|ui| {
        ui.invoke_move_visible_column(3, 0);
        ui.invoke_position_cell(10, 3, ScrollAlignment::Beginning, ScrollAlignment::Center);
    });
    assert_eq!(driver.component().get_table_x(), x);
    assert!(driver.component().get_table_y().0 > before.1.0);
    driver.update(|ui| ui.invoke_position_row(0, ScrollAlignment::Beginning));
    assert_eq!(driver.component().get_table_y(), Length(0.0));
    assert_eq!(driver.component().get_table_x(), x);
    assert_eq!(driver.component().get_current(), 5);
    assert_eq!(driver.component().get_selected_items(), 1);
}

#[test]
fn focus_observers_publish_both_sides_before_callbacks_and_keep_within_stable() {
    use xrack_layout::FocusObservers;
    use xrack_runtime::{FocusEvent, FocusReason, Node};

    let mut ui = FocusScopesDemo::new();
    let mut input = Interaction::default();
    let mut observers = FocusObservers::default();
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, Size::new(492.0, 272.0), &mut FixedText);
    let locate = |scene: &Node, pane: &str, field: &str| {
        let index = scene
            .children
            .iter()
            .position(|node| node.id == Some(pane))
            .unwrap();
        let child = scene.children[index]
            .children
            .iter()
            .position(|node| node.id == Some(field))
            .unwrap();
        [index, child]
    };
    assert!(observers.sync(&input, &scene, &layout, true).is_empty());
    input.focus_next(&scene, &layout, false);
    let events = observers.sync(&input, &scene, &layout, true);
    assert!(ui.get_left_active());
    assert_eq!(ui.get_left_visits(), 0);
    for event in events {
        event.apply(&mut ui, &scene);
    }
    assert_eq!(ui.get_left_visits(), 1);
    let updated = ui.render_shared();
    assert_eq!(
        updated.find("left").unwrap().color("background"),
        Some(Color(0x355372ff))
    );
    let query = locate(&scene, "left", "query");
    assert_eq!(
        updated.at_path(&query).unwrap().color("background"),
        Some(Color(0x203a52ff))
    );

    let notes = locate(&scene, "left", "notes");
    let target = input.accessibility_target(&scene, &layout, &notes).unwrap();
    input.focus_target(&scene, &layout, target);
    let events = observers.sync(&input, &scene, &layout, true);
    assert_eq!(
        events.iter().map(|event| event.event).collect::<Vec<_>>(),
        [
            FocusEvent::Lost(FocusReason::Accessibility),
            FocusEvent::Within {
                focused: false,
                reason: FocusReason::Accessibility
            },
            FocusEvent::Within {
                focused: true,
                reason: FocusReason::Accessibility
            },
            FocusEvent::Gained(FocusReason::Accessibility),
        ]
    );
    for event in events {
        event.apply(&mut ui, &scene);
    }
    assert!(ui.get_left_active());
    assert_eq!(ui.get_left_visits(), 1);
    let updated = ui.render_shared();
    assert_eq!(
        updated.at_path(&notes).unwrap().color("background"),
        Some(Color(0x203a52ff))
    );
    assert_eq!(
        updated.at_path(&query).unwrap().color("background"),
        Some(Color(0x19293cff))
    );

    let right = locate(&scene, "right", "query");
    let target = input.accessibility_target(&scene, &layout, &right).unwrap();
    input.focus_target(&scene, &layout, target);
    let events = observers.sync(&input, &scene, &layout, true);
    assert!(!ui.get_left_active());
    assert!(ui.get_right_active());
    for event in events {
        event.apply(&mut ui, &scene);
    }
    assert_eq!(ui.get_right_focus_reason(), FocusReason::Accessibility);
    assert_eq!(ui.get_right_visits(), 1);

    for event in observers.sync(&input, &scene, &layout, false) {
        event.apply(&mut ui, &scene);
    }
    assert!(!ui.get_right_active());
    assert_eq!(ui.get_right_focus_reason(), FocusReason::WindowActivation);
    assert_eq!(input.focused.as_ref().unwrap().path, right);
    for event in observers.sync(&input, &scene, &layout, true) {
        event.apply(&mut ui, &scene);
    }
    assert!(ui.get_right_active());
    assert_eq!(ui.get_right_visits(), 2);
}

#[test]
fn focused_key_route_captures_outer_first_and_bubbles_inner_first() {
    use xrack_runtime::{EventResult, KeyEvent, KeyModifiers, KeyPhase};

    let mut ui = ScopedKeysDemo::new();
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, Size::new(512.0, 254.0), &mut FixedText);
    let mut input = Interaction::default();
    input.focus_next(&scene, &layout, false);
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("field")
    );
    let route = input.key_route(&scene, &layout).unwrap();
    let key = |key: &str, text: &str| KeyEvent {
        key: key.into(),
        text: text.into(),
        ..Default::default()
    };
    let dispatch = |ui: &mut ScopedKeysDemo, phase, event: &KeyEvent| {
        route.dispatch(ui, &scene, &layout, phase, event).unwrap()
    };

    let blocked = key("x", "x");
    assert_eq!(
        dispatch(&mut ui, KeyPhase::CapturePressed, &blocked),
        EventResult::Accept
    );
    assert_eq!(ui.get_trace(), "外层捕获 内层捕获 ");
    assert_eq!(
        dispatch(&mut ui, KeyPhase::CaptureReleased, &blocked),
        EventResult::Accept
    );
    assert_eq!(ui.get_accepted_releases(), 1);

    ui.set_trace("".into());
    let f2 = key("F2", "");
    assert_eq!(
        dispatch(&mut ui, KeyPhase::CapturePressed, &f2),
        EventResult::Reject
    );
    assert_eq!(
        dispatch(&mut ui, KeyPhase::Pressed, &f2),
        EventResult::Accept
    );
    assert_eq!(ui.get_trace(), "外层捕获 内层捕获 内层冒泡 外层冒泡 ");
    assert_eq!(
        dispatch(&mut ui, KeyPhase::Released, &f2),
        EventResult::Accept
    );
    assert!(ui.get_trace().ends_with("外层释放 "));

    ui.set_trace("".into());
    let primary = KeyEvent {
        modifiers: KeyModifiers {
            primary: true,
            ..Default::default()
        },
        ..key("k", "")
    };
    assert_eq!(
        dispatch(&mut ui, KeyPhase::CapturePressed, &primary),
        EventResult::Accept
    );
    assert_eq!(ui.get_trace(), "外层捕获 ");

    input.sync_editor(&scene);
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("ni".into(), Some((2, 2)));
    assert!(input.key_route(&scene, &layout).is_none());
}

#[test]
fn key_capture_stays_inside_the_active_modal_overlay() {
    use xrack_runtime::{EventResult, KeyEvent, KeyPhase, Length, Node};

    #[derive(Default)]
    struct Recorder(Vec<&'static str>);
    impl Component for Recorder {
        fn render(&self) -> Node {
            Node::new("Window", None)
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn key_event_at(
            &mut self,
            scene: &Node,
            path: &[usize],
            _: KeyPhase,
            _: &KeyEvent,
        ) -> EventResult {
            self.0.push(scene.at_path(path).unwrap().id.unwrap());
            EventResult::Reject
        }
    }
    let box_node = |kind, id, width, height| {
        let mut node = Node::new(kind, Some(id));
        node.properties.insert("width", Length(width).into());
        node.properties.insert("height", Length(height).into());
        node
    };
    let mut scene = Node::new("Window", None);
    let mut background = box_node("FocusScope", "background", 480.0, 280.0);
    background.key_handlers = KeyPhase::CapturePressed.mask();
    let mut modal = box_node("Dialog", "modal", 320.0, 180.0);
    modal.properties.insert("open", true.into());
    let mut inside = box_node("FocusScope", "inside", 200.0, 80.0);
    inside.key_handlers = KeyPhase::CapturePressed.mask();
    inside
        .children
        .push(box_node("Button", "action", 100.0, 42.0));
    modal.children.push(inside);
    background.children.push(modal);
    scene.children.push(background);
    let layout = xrack_layout::layout(&scene, Size::new(480.0, 280.0), &mut FixedText);
    let mut input = Interaction::default();
    input.reconcile(&scene, &layout);
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("action")
    );
    let route = input.key_route(&scene, &layout).unwrap();
    let mut recorder = Recorder::default();
    assert_eq!(
        route.dispatch(
            &mut recorder,
            &scene,
            &layout,
            KeyPhase::CapturePressed,
            &KeyEvent::default()
        ),
        Some(EventResult::Reject)
    );
    assert_eq!(recorder.0, ["inside"]);
}

#[test]
fn application_window_places_chrome_around_scrollable_content_and_keeps_modal_geometry() {
    use navigation_ui::ApplicationWindowDemo;
    use xrack_layout::LayoutObservers;
    use xrack_runtime::Length;

    let mut ui = ApplicationWindowDemo::new();
    let mut input = Interaction::default();
    let mut observers = LayoutObservers::default();
    let prepare = |ui: &ApplicationWindowDemo, input: &Interaction, height| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(500.0, height),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input, 340.0);
    assert_eq!(scene.kind, "Window");
    assert!(scene.application_window);
    observers.sync(&scene, &layout);
    assert_eq!(
        layout.children[0].bounds,
        Rect::new(0.0, 302.0, 500.0, 38.0)
    );
    assert_eq!(
        layout.children[1].bounds,
        Rect::new(20.0, 98.0, 468.0, 186.0)
    );
    assert_eq!(layout.children[2].bounds, Rect::new(0.0, 0.0, 500.0, 32.0));
    assert_eq!(layout.children[3].bounds, Rect::new(0.0, 32.0, 500.0, 48.0));
    assert_eq!(
        xrack_layout::child_reading_order(&scene).collect::<Vec<_>>(),
        [2, 3, 1, 4, 0]
    );
    assert_eq!(
        (ui.get_body_width(), ui.get_body_height()),
        (Length(468.0), Length(186.0))
    );
    input.pointer_move(&scene, &layout, Point { x: 60.0, y: 190.0 });
    assert!(input.scroll_wheel(&layout, Point { x: 0.0, y: 28.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&28.0));
    let (_, scrolled) = prepare(&ui, &input, 340.0);
    assert_eq!(scrolled.children[3].bounds, layout.children[3].bounds);
    assert_eq!(scrolled.children[0].bounds, layout.children[0].bounds);

    ui.set_show_menu(false);
    ui.set_show_footer(false);
    let (scene, resized) = prepare(&ui, &input, 340.0);
    observers.sync(&scene, &resized);
    assert_eq!(
        resized.children[1].bounds,
        Rect::new(20.0, 66.0, 468.0, 262.0)
    );
    assert_eq!(ui.get_body_height(), Length(262.0));

    ui.set_dialog_open(true);
    let (_, modal) = prepare(&ui, &input, 340.0);
    assert_eq!(
        modal.children[4].bounds,
        Rect::new(130.0, 110.0, 240.0, 120.0)
    );
    ui.set_dialog_open(false);
    ui.set_show_menu(true);
    ui.set_show_footer(true);
    let (scene, tiny) = prepare(&ui, &input, 50.0);
    observers.sync(&scene, &tiny);
    assert_eq!(tiny.children[2].bounds.height, 32.0);
    assert_eq!(tiny.children[3].bounds.height, 18.0);
    assert_eq!(tiny.children[0].bounds.height, 0.0);
    assert_eq!(ui.get_body_height(), Length(0.0));

    ui.set_allow_close(false);
    assert!(!ui.close_requested());
    assert_eq!(ui.get_close_attempts(), 1);
    ui.set_allow_close(true);
    assert!(ui.close_requested());
    assert_eq!(ui.get_close_attempts(), 2);
}

#[test]
fn application_window_reads_menu_header_content_and_footer_in_visual_order() {
    use navigation_ui::ApplicationWindowDemo;
    let mut ui = UiDriver::new(
        ApplicationWindowDemo::new(),
        FixedText,
        Size::new(500.0, 340.0),
    );
    for target in ["file", "head-action", "body-action", "foot-action"] {
        ui.tab();
        assert_eq!(ui.focused(), Some(target));
    }
    ui.click("file");
    assert!(ui.click_menu_row("刷新"));
    assert_eq!(ui.component().get_menu_actions(), 1);
    ui.click("head-action");
    assert!(ui.component().get_dialog_open());
    assert!(!ui.can_hit("foot-action"));
}

#[test]
fn pane_and_frame_fit_their_content_and_block_clicks_to_underlying_items() {
    use controls_ui::PaneFrameDemo;
    use xrack_layout::LayoutObservers;
    use xrack_runtime::Length;

    let size = Size::new(460.0, 270.0);
    let mut ui = PaneFrameDemo::new();
    let mut input = Interaction::default();
    let mut geometry = LayoutObservers::default();
    let prepare = |ui: &PaneFrameDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    assert_eq!(ui.get_pane_inner_width(), Length(0.0));
    let (scene, layout) = prepare(&ui, &input);
    geometry.sync(&scene, &layout);
    let pane = [2];
    let frame = [2, 0];
    let button = [2, 0, 0];
    assert_eq!(
        layout.at_path(&pane).unwrap().bounds,
        Rect::new(20.0, 50.0, 420.0, 164.0)
    );
    assert_eq!(
        layout.at_path(&frame).unwrap().bounds,
        Rect::new(48.0, 74.0, 142.0, 60.0)
    );
    assert_eq!(
        layout.at_path(&button).unwrap().bounds,
        Rect::new(60.0, 88.0, 122.0, 38.0)
    );
    assert_eq!(ui.get_pane_inner_width(), Length(390.0));
    assert_eq!(
        (ui.get_frame_inner_width(), ui.get_frame_inner_height()),
        (Length(126.0), Length(44.0))
    );
    for (point, expected) in [
        (Point { x: 78.0, y: 104.0 }, Some("action")),
        (Point { x: 393.0, y: 155.0 }, None),
        (Point { x: 445.0, y: 155.0 }, Some("underlying")),
    ] {
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target();
        assert_eq!(target.and_then(|target| target.id), expected);
    }
    ui.set_left_padding(Length(32.0));
    ui.set_content_size(Length(350.0));
    let (scene, layout) = prepare(&ui, &input);
    geometry.sync(&scene, &layout);
    assert_eq!(layout.at_path(&pane).unwrap().bounds.width, 392.0);
    assert_eq!(layout.at_path(&button).unwrap().bounds.x, 72.0);
    assert_eq!(ui.get_pane_inner_width(), Length(350.0));

    let mut driver = UiDriver::new(PaneFrameDemo::new(), FixedText, size);
    driver.click("action");
    assert_eq!(driver.component().get_inner_clicks(), 1);
    ui.set_content_enabled(false);
    let (scene, layout) = prepare(&ui, &input);
    assert!(!layout.at_path(&button).unwrap().enabled);
    input.pointer_move(&scene, &layout, Point { x: 83.0, y: 104.0 });
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.pointer_release_target().is_none());
}

#[test]
fn layout_geometry_is_published_to_both_component_scopes_after_layout() {
    use xrack_layout::LayoutObservers;
    use xrack_runtime::Length;

    let mut ui = GeometryDemo::new();
    let input = Interaction::default();
    let mut observations = LayoutObservers::default();
    let prepare = |ui: &GeometryDemo, width| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(width, 290.0),
            &mut FixedText,
            &input,
        )
    };
    assert!(!ui.get_panel_ready());
    assert_eq!(ui.get_panel_width(), Length(0.0));
    let (scene, layout) = prepare(&ui, 500.0);
    let panel = layout.at_path(&[0, 0, 0]).unwrap();
    let parent = layout.at_path(&[0, 0]).unwrap();
    observations.sync(&scene, &layout);
    assert!(ui.get_panel_ready());
    assert_eq!(
        ui.get_panel_x(),
        Length(f64::from(panel.bounds.x - parent.bounds.x))
    );
    assert_eq!(ui.get_panel_width(), Length(f64::from(panel.bounds.width)));
    assert_eq!(ui.get_component_width(), ui.get_panel_width());
    assert_eq!(ui.get_panel_window_width(), ui.get_panel_width());
    let text = ui.render_shared();
    assert!(
        text.find("status")
            .unwrap()
            .text("text")
            .unwrap()
            .contains("布局宽度")
    );
    let first_width = ui.get_panel_width();

    ui.set_rotated(true);
    let (scene, layout) = prepare(&ui, 600.0);
    let panel = layout.at_path(&[0, 0, 0]).unwrap();
    observations.sync(&scene, &layout);
    assert_eq!(ui.get_panel_width(), Length(f64::from(panel.bounds.width)));
    assert!(
        ui.get_panel_width().0 > first_width.0,
        "before {first_width:?}, after {:?}",
        ui.get_panel_width()
    );
    assert_eq!(ui.get_panel_width(), ui.get_component_width());
    assert!((ui.get_panel_window_width().0 - f64::from(panel.bounds.height)).abs() < 0.01);

    ui.invoke_show_details();
    let (scene, layout) = prepare(&ui, 600.0);
    observations.sync(&scene, &layout);
    assert!(ui.get_tip_ready());
    assert_eq!(ui.get_tip_width(), ui.get_panel_width());
    let (scene, layout) = prepare(&ui, 600.0);
    observations.sync(&scene, &layout);
    assert!(!ui.scene_dirty());

    ui.set_show_panel(false);
    let (scene, layout) = prepare(&ui, 600.0);
    observations.sync(&scene, &layout);
    assert!(!ui.get_panel_ready());
    assert_eq!(ui.get_panel_width(), Length(0.0));
    assert_eq!(ui.get_component_width(), Length(0.0));
    assert_eq!(ui.get_panel_window_width(), Length(0.0));
}

#[test]
fn editor_context_menus_share_text_commands_and_keep_custom_menus() {
    let mut driver = UiDriver::new(EditorMenuDemo::new(), FixedText, Size::new(540.0, 420.0));
    let enabled = |driver: &UiDriver<_, _>, label| {
        driver.menus()[0]
            .rows
            .iter()
            .find(|row| row.label == label)
            .unwrap()
            .enabled
    };
    assert!(driver.context_menu("entry"));
    assert_eq!(driver.focused(), Some("entry"));
    assert!(!enabled(&driver, "Copy"));
    assert!(!enabled(&driver, "Undo"));
    assert!(driver.click_menu_row("Select All"));
    let original = driver.component().get_document();
    assert_eq!(
        driver.input().editor.as_ref().unwrap().selection(&original),
        0..original.len()
    );
    driver.fill_focused("Edited");
    assert!(driver.keyboard_context_menu());
    assert!(enabled(&driver, "Undo"));
    driver.menu_key(MenuKey::Accept);
    assert_eq!(driver.component().get_document(), original);
    assert!(driver.keyboard_context_menu());
    assert!(driver.click_menu_row("Redo"));
    assert_eq!(driver.component().get_document(), "Edited");

    for (editor, copy, paste) in [("read-only", true, false), ("secret", false, true)] {
        assert!(driver.context_menu(editor));
        assert!(driver.click_menu_row("Select All"));
        assert!(driver.keyboard_context_menu());
        assert_eq!(enabled(&driver, "Copy"), copy);
        assert_eq!(enabled(&driver, "Paste"), paste);
        assert!(!enabled(&driver, "Cut"));
        driver.menu_key(MenuKey::Escape);
    }
    assert!(driver.context_menu("custom"));
    assert_eq!(driver.menus()[0].rows[0].label, "Custom action");
    assert!(driver.click_menu_row("Custom action"));
    assert_eq!(driver.component().get_custom_actions(), 1);
    assert!(!driver.context_menu("quiet"));
    assert!(driver.context_menu("entry"));
    driver.update(|ui| ui.set_document("Updated from Rust".into()));
    assert!(!driver.input().menu_open());
}

#[test]
fn spin_box_context_menu_replays_the_draft_before_commit() {
    let mut ui = EditorMenuDemo::new();
    let (scene, layout) = prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(540.0, 420.0),
        &mut FixedText,
        &Default::default(),
    );
    let mut input = Interaction::default();
    let path = [0, 4];
    let spin = scene.at_path(&path).unwrap();
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target);
    input.edit_spin(spin, EditAction::ReplaceAll("315"));
    assert_eq!(input.editor_value(spin), "315");
    for (command, redo, expected) in [
        (xrack_runtime::TextCommand::Undo, false, "42"),
        (xrack_runtime::TextCommand::Redo, true, "315"),
    ] {
        assert!(input.open_context_menu(&scene, &layout, true));
        assert_eq!(
            input
                .menu_key(&scene, &layout, MenuKey::Accept)
                .unwrap()
                .action,
            ControlAction::TextCommand(command)
        );
        assert!(input.replay_text_edit(&mut ui, &scene, redo));
        assert_eq!(input.editor_value(spin), expected);
        assert_eq!(ui.get_quantity(), 42);
    }
    let commit = input.commit_spin_draft().unwrap();
    assert!(ui.control_at(&scene, &commit.path, commit.action));
    assert_eq!(ui.get_quantity(), 315);
}

#[test]
fn editor_commands_select_utf8_ranges_and_share_edit_history() {
    use xrack_runtime::{FocusAction, FocusRequests, TextCommand};

    let mut ui = EditorCommandsDemo::new();
    let mut input = Interaction::default();
    let queue = FocusRequests::default();
    let _scope = queue.enter();
    let prepare = |ui: &EditorCommandsDemo, input: &mut Interaction| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(620.0, 240.0), &mut FixedText);
        input.reconcile(&scene, &layout);
        (scene, layout)
    };
    let (scene, layout) = prepare(&ui, &mut input);
    ui.invoke_paste_selected();
    assert!(
        input
            .apply_focus_request(&scene, &layout, queue.drain().remove(0))
            .is_none()
    );
    ui.invoke_select_prefix();
    input.apply_focus_requests(&scene, &layout, queue.drain());
    let editor_path = input.focused.as_ref().unwrap().path.clone();
    let node = scene.at_path(&editor_path).unwrap();
    let original = ui.get_document();
    let handle = node.focus_handle.clone().unwrap();
    assert_eq!(input.editor.as_ref().unwrap().selection(&original), 0..9);

    let copy_path = [0, 0, 4];
    let button = layout.at_path(&copy_path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: button.x + button.width / 2.0,
            y: button.y + button.height / 2.0,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, editor_path);
    assert_eq!(input.editor.as_ref().unwrap().selection(&original), 0..9);
    let pressed = input.pointer_release_target().unwrap();
    assert_eq!(pressed.id, Some("copy-selection"));
    assert!(ui.dispatch_at(&scene, &pressed.path, "clicked"));
    assert_eq!(
        input.apply_focus_request(&scene, &layout, queue.drain().remove(0)),
        Some(TextCommand::Copy)
    );

    handle.request(FocusAction::Text(TextCommand::SetSelection {
        anchor: 10,
        focus: -2,
    }));
    input.apply_focus_requests(&scene, &layout, queue.drain());
    let edit = input.editor.as_ref().unwrap();
    assert_eq!(edit.anchor(&original), 9);
    assert_eq!(edit.caret(&original), 0);

    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("新："),
        EditSource::Atomic,
        Instant::now()
    ));
    assert_eq!(ui.get_document(), "新：在这里编辑文本");
    let (scene, layout) = prepare(&ui, &mut input);
    ui.invoke_undo_edit();
    assert_eq!(
        input.apply_focus_request(&scene, &layout, queue.drain().remove(0)),
        Some(TextCommand::Undo)
    );
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.get_document(), original);
    assert_eq!(input.editor.as_ref().unwrap().selection(&original), 0..9);
}

#[test]
fn focus_scopes_use_instance_identity_and_restore_only_eligible_children() {
    use xrack_runtime::FocusRequests;
    fn focused_name<'a>(input: &Interaction, scene: &'a xrack_runtime::Node) -> Option<&'a str> {
        scene
            .at_path(&input.focused.as_ref()?.path)?
            .text("accessible-name")
    }

    let mut ui = FocusScopesDemo::new();
    let mut input = Interaction::default();
    let queue = FocusRequests::default();
    let prepare = |ui: &FocusScopesDemo, input: &mut Interaction| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(492.0, 272.0), &mut FixedText);
        input.reconcile(&scene, &layout);
        (scene, layout)
    };
    let dispatch = |ui: &mut FocusScopesDemo, id| {
        let _scope = queue.enter();
        assert!(ui.dispatch(id, "clicked"));
    };

    let (scene, layout) = prepare(&ui, &mut input);
    input.focus_next(&scene, &layout, false);
    assert_eq!(focused_name(&input, &scene), Some("左侧 查询"));
    input.focus_next(&scene, &layout, false);
    assert_eq!(focused_name(&input, &scene), Some("右侧 查询"));
    input.focus_next(&scene, &layout, true);
    assert_eq!(focused_name(&input, &scene), Some("右侧 备注"));

    dispatch(&mut ui, "jump-left");
    input.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&input, &scene), Some("左侧 查询"));
    input.pointer_move(&scene, &layout, Point { x: 44.0, y: 75.0 });
    input.pointer_press_in_scene(&scene, &layout);
    assert_eq!(focused_name(&input, &scene), Some("左侧 备注"));
    dispatch(&mut ui, "jump-right");
    input.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&input, &scene), Some("右侧 备注"));
    dispatch(&mut ui, "jump-left");
    input.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&input, &scene), Some("左侧 备注"));

    {
        let _scope = queue.enter();
        ui.invoke_clear_left();
    }
    input.apply_focus_requests(&scene, &layout, queue.drain());
    assert!(input.focused.is_none());

    let mut fresh = Interaction::default();
    dispatch(&mut ui, "disable-left");
    let (scene, layout) = prepare(&ui, &mut fresh);
    fresh.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&fresh, &scene), Some("左侧 备注"));
    dispatch(&mut ui, "jump-right");
    fresh.apply_focus_requests(&scene, &layout, queue.drain());
    dispatch(&mut ui, "jump-left");
    ui.set_left_visible(false);
    let (scene, layout) = prepare(&ui, &mut fresh);
    fresh.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&fresh, &scene), Some("右侧 查询"));
    ui.set_left_visible(true);
    ui.set_left_query_enabled(true);
    let (scene, layout) = prepare(&ui, &mut fresh);
    fresh.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&fresh, &scene), Some("右侧 查询"));
    dispatch(&mut ui, "jump-left");
    fresh.apply_focus_requests(&scene, &layout, queue.drain());
    assert_eq!(focused_name(&fresh, &scene), Some("左侧 备注"));

    let mut modal_scene = scene.as_ref().clone();
    let mut dialog = xrack_runtime::Node::new("Dialog", Some("modal"));
    dialog.properties.insert("open", true.into());
    modal_scene.children.push(dialog);
    let modal_layout = xrack_layout::layout(&modal_scene, Size::new(492.0, 272.0), &mut FixedText);
    fresh.reconcile(&modal_scene, &modal_layout);
    assert_eq!(
        fresh.focused.as_ref().and_then(|target| target.id),
        Some("modal")
    );
    dispatch(&mut ui, "jump-left");
    fresh.apply_focus_requests(&modal_scene, &modal_layout, queue.drain());
    assert_eq!(
        fresh.focused.as_ref().and_then(|target| target.id),
        Some("modal")
    );
}

#[test]
fn states_choose_the_first_condition_and_only_evaluate_the_selected_binding() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::Length;

    let mut ui = StatesDemo::new();
    let calls = Rc::new(Cell::new(0));
    let called = calls.clone();
    ui.on_base_width(move || {
        called.set(called.get() + 1);
        Length(96.0)
    });
    ui.set_level(2);
    let alert = ui.render_shared();
    assert_eq!(alert.find("tile").unwrap().length("width"), Some(280.0));
    assert_eq!(alert.find("caption").unwrap().text("text"), Some("告警"));
    assert!(alert.find("badge").unwrap().is_visible());
    let leds = &alert.find("status-leds").unwrap().children[0].children;
    assert_eq!(leds[0].color("background"), Some(Color(0x314051ff)));
    assert_eq!(leds[2].color("background"), Some(Color(0x9adcb7ff)));
    assert_eq!(calls.get(), 0);
    ui.set_level(1);
    let warning = ui.render_shared();
    assert_eq!(warning.find("tile").unwrap().length("width"), Some(168.0));
    assert!(!warning.find("badge").unwrap().is_visible());
    assert_eq!(calls.get(), 0);
    ui.set_level(0);
    let normal = ui.render_shared();
    assert_eq!(normal.find("tile").unwrap().length("width"), Some(96.0));
    assert_eq!(normal.find("caption").unwrap().text("text"), Some("正常"));
    assert_eq!(calls.get(), 1);
    assert!(Rc::ptr_eq(&normal, &ui.render_shared()));
    assert_eq!(calls.get(), 1);

    let mut bound = StatesDemo::new();
    bound.set_level(2);
    let previous = bound.render_shared();
    bound.set_baseline(Length(124.0));
    assert!(Rc::ptr_eq(&previous, &bound.render_shared()));
    bound.set_level(0);
    assert_eq!(
        bound.render().find("tile").unwrap().length("width"),
        Some(124.0)
    );
}

#[test]
fn custom_chrome_queues_window_commands_and_only_changes_labels_after_host_feedback() {
    use xrack_runtime::{WindowCommand, WindowStatus};

    let mut driver = UiDriver::new(FramelessDemo::new(), FixedText, Size::new(540.0, 320.0));
    assert_eq!(driver.node("maximize-window").text("text"), Some("□"));
    driver.click("maximize-window");
    driver.click("toggle-full-screen");
    assert_eq!(driver.node("maximize-window").text("text"), Some("□"));
    assert_eq!(
        driver.node("toggle-full-screen").text("text"),
        Some("进入全屏")
    );
    driver.update(|ui| {
        assert_eq!(
            ui.take_window_commands(),
            [
                WindowCommand::ToggleMaximized,
                WindowCommand::ToggleFullScreen
            ]
        );
        assert!(ui.take_window_commands().is_empty());
        ui.publish_window_status(WindowStatus {
            active: true,
            minimized: Some(false),
            maximized: true,
            full_screen: false,
        });
    });
    assert_eq!(driver.node("maximize-window").text("text"), Some("❐"));
    assert_eq!(
        driver.node("window-status").text("text"),
        Some("最大化 · 当前窗口")
    );
    driver.update(|ui| {
        ui.publish_window_status(WindowStatus {
            full_screen: true,
            ..WindowStatus::default()
        })
    });
    assert_eq!(
        driver.node("toggle-full-screen").text("text"),
        Some("退出全屏")
    );
    driver.click("restore-window");
    driver.click("minimize-window");
    driver.update(|ui| {
        assert_eq!(
            ui.take_window_commands(),
            [WindowCommand::Restore, WindowCommand::Minimize]
        )
    });
    let mut other = FramelessDemo::new();
    assert!(other.take_window_commands().is_empty());
    assert_eq!(
        other
            .render()
            .find("toggle-full-screen")
            .unwrap()
            .text("text"),
        Some("进入全屏")
    );
}

#[test]
fn custom_window_chrome_routes_background_drags_and_keeps_controls_interactive() {
    use xrack_layout::WindowResizeEdge;

    let mut ui = FramelessDemo::new();
    let viewport = Size::new(540.0, 320.0);
    let frame = |ui: &FramelessDemo| {
        prepare_scene(
            ui,
            ui.render_shared(),
            viewport,
            &mut FixedText,
            &Default::default(),
        )
    };
    let (scene, layout) = frame(&ui);
    let mut input = Interaction::default();
    let title = Point { x: 60.0, y: 30.0 };
    let corner = Point { x: 539.0, y: 319.0 };
    input.pointer_move(&scene, &layout, corner);
    assert_eq!(
        input.window_resize_edge(&scene, &layout),
        Some(WindowResizeEdge::SouthEast)
    );
    input.pointer_move(&scene, &layout, Point { x: 1.0, y: 1.0 });
    assert_eq!(
        input.window_resize_edge(&scene, &layout),
        Some(WindowResizeEdge::NorthWest)
    );
    input.pointer_move(&scene, &layout, title);
    assert_eq!(input.window_resize_edge(&scene, &layout), None);
    assert!(input.arm_window_move(&scene, &layout));
    assert!(!input.take_window_move(&scene, &layout, Point { x: 62.0, y: 31.0 }));
    assert!(input.take_window_move(&scene, &layout, Point { x: 68.0, y: 31.0 }));
    assert!(!input.take_window_move(&scene, &layout, Point { x: 78.0, y: 31.0 }));

    input.pointer_move(&scene, &layout, title);
    assert!(input.arm_window_move(&scene, &layout));
    ui.set_can_move(false);
    let (scene, layout) = frame(&ui);
    assert!(!input.take_window_move(&scene, &layout, Point { x: 78.0, y: 31.0 }));
    assert!(!input.arm_window_move(&scene, &layout));
    let mut driver = UiDriver::new(ui, FixedText, viewport);
    driver.click("header-action");
    assert_eq!(driver.component().get_header_clicks(), 1);
    driver.update(|ui| ui.set_can_move(true));
    driver.click("header-action");
    assert_eq!(driver.component().get_header_clicks(), 2);

    let mut ui = FramelessDemo::new();
    for (frameless, resizable, modal) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        ui.set_frameless(frameless);
        ui.set_can_resize(resizable);
        ui.set_modal(modal);
        let (scene, layout) = frame(&ui);
        input.reconcile(&scene, &layout);
        input.pointer_move(&scene, &layout, corner);
        assert_eq!(input.window_resize_edge(&scene, &layout), None);
        if modal {
            input.pointer_move(&scene, &layout, title);
            assert!(!input.arm_window_move(&scene, &layout));
        }
    }
}

#[test]
fn component_states_share_lazy_inputs_and_respect_explicit_writes() {
    use std::{cell::Cell, rc::Rc};

    let mut ui = ComponentStatesDemo::new();
    let selections = Rc::new(Cell::new(0));
    let called = selections.clone();
    ui.on_warning_active(move |active| {
        called.set(called.get() + 1);
        active
    });
    let base_reads = Rc::new(Cell::new(0));
    let called = base_reads.clone();
    ui.on_normal_caption(move |text| {
        called.set(called.get() + 1);
        text
    });
    ui.set_warning(true);
    assert_eq!(ui.get_effective_caption(), "需要检查");
    let scene = ui.render_shared();
    let banner = scene.find("banner").unwrap();
    assert_eq!(
        banner.find("badge-text").unwrap().text("text"),
        Some("需要检查")
    );
    assert_eq!(
        banner.children[0].color("background"),
        Some(Color(0xba4b5dff))
    );
    assert_eq!(banner.length("width"), Some(300.0));
    assert_eq!(banner.state_timings.as_ref().unwrap()["width"].case, 0);
    assert_eq!(selections.get(), 1);
    assert_eq!(base_reads.get(), 0);
    ui.set_baseline("已经恢复".into());
    assert!(Rc::ptr_eq(&scene, &ui.render_shared()));
    ui.set_warning(false);
    assert_eq!(ui.get_effective_caption(), "已经恢复");
    assert_eq!(base_reads.get(), 1);
    assert_eq!(
        ui.render_shared().find("banner").unwrap().length("width"),
        Some(200.0)
    );

    ui.set_warning(true);
    ui.invoke_set_banner("手动标签".into());
    assert_eq!(ui.get_effective_caption(), "手动标签");
    ui.set_warning(false);
    assert_eq!(ui.get_effective_caption(), "手动标签");
}

#[test]
fn repeated_component_states_use_each_rows_live_inputs() {
    let mut driver = UiDriver::new(
        ComponentStatesDemo::new(),
        FixedText,
        Size::new(520.0, 306.0),
    );
    let labels = |driver: &UiDriver<_, _>| {
        driver
            .node("queue")
            .children
            .iter()
            .map(|row| {
                row.find("badge-text")
                    .unwrap()
                    .text("text")
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(&driver), ["监控", "存储", "网络"]);
    driver.click("toggle");
    assert_eq!(labels(&driver), ["监控", "存储 · 待检查", "网络"]);
    driver.click("toggle");
    assert_eq!(labels(&driver), ["监控", "存储", "网络"]);
}

#[test]
fn native_file_drop_filters_a_whole_batch_and_revalidates_the_release_route() {
    use std::sync::Arc;
    use xrack_layout::FileDragInput;

    let path = |name| std::env::temp_dir().join(name).to_str().unwrap().to_owned();
    let paths: Arc<[String]> = [path("照片 1.PNG"), path("two.jpg")].into();
    let mixed = [paths[0].clone(), path("notes.txt")];
    let images = Point { x: 100.0, y: 110.0 };
    let documents = Point { x: 330.0, y: 110.0 };
    let mut ui = FileDropDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &FileDropDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(536.0, 310.0), &mut FixedText);
        (scene, layout)
    };
    let dispatch =
        |ui: &mut FileDropDemo, scene: &xrack_runtime::Node, update: xrack_layout::DragUpdate| {
            for event in update.events {
                assert!(event.apply(ui, scene));
            }
        };
    let (scene, layout) = prepare(&ui);
    let scope = input.file_drop_scope(&layout);
    assert!(scope.target(&scene, &layout, images, &mixed).is_none());
    assert!(
        scope
            .target(&scene, &layout, images, &["relative.png".into()])
            .is_none()
    );
    let target = scope.target(&scene, &layout, images, &paths).unwrap();
    assert_eq!(target.id, Some("images"));
    assert!(scope.target(&scene, &layout, documents, &paths).is_none());

    let move_to = |sequence, point| FileDragInput::Moved {
        sequence,
        point,
        paths: paths.clone(),
    };
    let drop_at = |sequence| FileDragInput::Dropped {
        sequence,
        point: images,
        paths: paths.clone(),
        target: target.clone(),
    };
    dispatch(
        &mut ui,
        &scene,
        input.file_drag_event(&scene, &layout, move_to(1, images)),
    );
    assert_eq!(ui.get_preview(), paths.as_ref());
    dispatch(
        &mut ui,
        &scene,
        input.file_drag_event(&scene, &layout, move_to(1, documents)),
    );
    assert!(ui.get_preview().is_empty());
    dispatch(
        &mut ui,
        &scene,
        input.file_drag_event(&scene, &layout, move_to(1, images)),
    );
    dispatch(
        &mut ui,
        &scene,
        input.file_drag_event(&scene, &layout, drop_at(1)),
    );
    assert!(
        input
            .file_drag_event(&scene, &layout, drop_at(1))
            .events
            .is_empty()
    );
    assert_eq!(ui.get_paths(), paths.as_ref());
    assert_eq!(ui.get_zone(), "图片");
    assert_eq!(ui.get_batches(), 1);
    assert!(ui.get_preview().is_empty());

    dispatch(
        &mut ui,
        &scene,
        input.file_drag_event(&scene, &layout, move_to(2, images)),
    );
    ui.set_images_enabled(false);
    let (current, layout) = prepare(&ui);
    dispatch(
        &mut ui,
        &current,
        input.file_drag_event(&current, &layout, drop_at(2)),
    );
    assert_eq!(ui.get_batches(), 1);
    assert!(ui.get_preview().is_empty());
}

#[test]
fn dragging_preserves_clicks_negotiates_targets_and_freezes_the_payload() {
    use xrack_runtime::DragAction;

    let mut ui = DragDropDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(560.0, 258.0);
    let start = Point { x: 102.0, y: 117.0 };
    let tiny = Point { x: 105.0, ..start };
    let target = Point { x: 420.0, y: 117.0 };
    let prepare = |ui: &DragDropDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
        (scene, layout)
    };
    let dispatch =
        |ui: &mut DragDropDemo, scene: &xrack_runtime::Node, update: xrack_layout::DragUpdate| {
            assert!(update.captured);
            for event in update.events {
                assert!(event.apply(ui, scene));
            }
        };
    let (scene, layout) = prepare(&ui);
    input.pointer_move(&scene, &layout, start);
    input.pointer_press();
    input.arm_drag(&scene, &layout, start);
    assert!(!input.drag_motion(&scene, &layout, tiny, None).captured);
    input.pointer_move(&scene, &layout, tiny);
    assert!(!input.finish_drag(&scene, &layout, tiny, None).captured);
    let clicked = input.pointer_release_target().unwrap();
    assert!(ui.dispatch_at(&scene, &clicked.path, "clicked"));
    assert_eq!(ui.get_clicks(), 1);

    input.pointer_move(&scene, &layout, start);
    input.pointer_press();
    input.arm_drag(&scene, &layout, start);
    dispatch(
        &mut ui,
        &scene,
        input.drag_motion(&scene, &layout, target, Some(DragAction::Copy)),
    );
    assert_eq!(ui.get_trace(), "start-enter");
    assert_eq!(ui.get_last_x().0, 94.0);
    assert!(input.pressed.is_none());
    ui.set_payload("new value".into());
    let (scene, layout) = prepare(&ui);
    dispatch(
        &mut ui,
        &scene,
        input.finish_drag(&scene, &layout, target, Some(DragAction::Copy)),
    );
    assert_eq!(ui.get_trace(), "start-enter-drop-finish");
    assert_eq!(ui.get_received(), "XR-42");
    assert_eq!(ui.get_result(), DragAction::Copy);
    assert!(ui.get_card_present());

    input.arm_drag(&scene, &layout, start);
    dispatch(
        &mut ui,
        &scene,
        input.drag_motion(&scene, &layout, target, Some(DragAction::Link)),
    );
    assert_eq!(input.drag_action(), Some(DragAction::None));
    assert_eq!(ui.get_trace(), "start");
    dispatch(
        &mut ui,
        &scene,
        input.drag_motion(&scene, &layout, target, None),
    );
    dispatch(&mut ui, &scene, input.cancel_drag());
    assert_eq!(ui.get_result(), DragAction::None);
    assert_eq!(ui.get_trace(), "start-enter-exit-finish");
    assert_eq!(ui.get_received(), "XR-42");

    input.arm_drag(&scene, &layout, start);
    dispatch(
        &mut ui,
        &scene,
        input.drag_motion(&scene, &layout, target, None),
    );
    ui.set_target_enabled(false);
    let (scene, layout) = prepare(&ui);
    let leaving = input.reconcile_drag(&scene, &layout, None);
    assert_eq!(leaving.events.len(), 1);
    assert!(
        leaving
            .events
            .into_iter()
            .next()
            .unwrap()
            .apply(&mut ui, &scene)
    );
    dispatch(
        &mut ui,
        &scene,
        input.finish_drag(&scene, &layout, target, None),
    );
    assert_eq!(ui.get_trace(), "start-enter-exit-finish");
    assert_eq!(ui.get_result(), DragAction::None);

    ui.set_source_enabled(false);
    let (scene, layout) = prepare(&ui);
    input.arm_drag(&scene, &layout, start);
    assert!(input.drag_position().is_none());
    let mut driver = UiDriver::new(ui, FixedText, viewport);
    driver.click("card");
    assert_eq!(driver.component().get_clicks(), 2);
}

#[test]
fn recycled_drag_routes_cannot_dispatch_to_a_different_model_row() {
    use xrack_runtime::DragEvent;

    let mut ui = VirtualDragDemo::new();
    let mut input = Interaction::default();
    let prepare = |ui: &VirtualDragDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(360.0, 180.0),
            &mut FixedText,
            input,
        )
    };
    let point = Point { x: 194.0, y: 32.0 };
    let drag = |input: &mut Interaction,
                scene: &xrack_runtime::Node,
                layout: &xrack_layout::LayoutNode| {
        input.arm_drag(scene, layout, Point { x: 68.0, y: 34.0 });
        assert!(input.drag_motion(scene, layout, point, None).captured);
        input
            .finish_drag(scene, layout, point, None)
            .events
            .into_iter()
            .find(|event| matches!(event.event, DragEvent::Dropped { .. }))
            .unwrap()
    };

    let (scene, layout) = prepare(&ui, &input);
    let drop = drag(&mut input, &scene, &layout);
    assert!(drop.target.row_key.is_some());
    let target = drop.target.path.clone();
    assert!(drop.apply(&mut ui, &scene));
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(
        scene.at_path(&target).unwrap().children[0].text("text"),
        Some("old:ticket")
    );

    let stale = drag(&mut input, &scene, &layout);
    assert!(ui.model_items().sort_by(|a, b| a.cmp(b)));
    let (current, _) = prepare(&ui, &input);
    assert_ne!(scene.row_key_at(&target), current.row_key_at(&target));
    assert!(!stale.apply(&mut ui, &current));
    assert_eq!(
        current.at_path(&target).unwrap().children[0].text("text"),
        Some("new:")
    );
}

#[test]
fn a_close_guard_can_keep_the_window_open_and_retry_after_confirmation() {
    use xrack_runtime::{NativeDialogQueue, NativeDialogResult};

    let mut ui = CloseGuardDemo::new();
    assert!(ui.close_requested());
    assert!(ui.render_shared().native_dialogs.is_empty());
    ui.set_dirty(true);
    ui.set_draft("草稿".into());

    assert!(ui.dispatch("close-window", "clicked"));
    assert!(ui.dispatch("close-window", "clicked"));
    assert!(ui.take_window_close_request());
    assert!(!ui.take_window_close_request());
    assert!(!ui.close_requested());

    let mut queue = NativeDialogQueue::default();
    let scene = ui.render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let request = queue.begin_next().unwrap();
    assert!(queue.complete(
        request.id,
        NativeDialogResult::Button(StandardButtonKind::Cancel)
    ));
    assert!(ui.get_dirty());
    assert_eq!(ui.get_draft(), "草稿");
    assert!(!ui.take_window_close_request());

    assert!(!ui.close_requested());
    let scene = ui.render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let request = queue.begin_next().unwrap();
    assert!(queue.complete(
        request.id,
        NativeDialogResult::Button(StandardButtonKind::Ok)
    ));
    assert!(!ui.get_dirty());
    assert!(ui.take_window_close_request());
    assert!(ui.close_requested());
    assert_eq!(ui.get_requests(), 4);
    assert!(ui.render_shared().native_dialogs.is_empty());
}

#[test]
fn native_dialogs_snapshot_options_serialize_panels_and_discard_abandoned_results() {
    use xrack_runtime::{
        FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
    };

    let mut first = FileDialogDemo::new();
    let mut second = FileDialogDemo::new();
    assert!(first.render_shared().native_dialogs.is_empty());
    first.set_start_directory("/workspace".into());
    first.invoke_choose(FileDialogMode::OpenFiles);
    let initial = first.render_shared();
    first.set_start_directory("/changed".into());
    first.invoke_choose(FileDialogMode::SaveFile);
    let first_scene = first.render_shared();
    assert_eq!(first_scene.native_dialogs, initial.native_dialogs);
    assert!(
        first_scene
            .children
            .iter()
            .all(|node| node.kind != "FileDialog")
    );

    second.invoke_choose(FileDialogMode::SaveFile);
    let second_scene = second.render_shared();
    let mut queue = NativeDialogQueue::default();
    queue.sync(&[
        &first_scene.native_dialogs[0],
        &second_scene.native_dialogs[0],
    ]);
    let request = queue.begin_next().unwrap();
    let NativeDialogOptions::File(options) = request.options.as_ref() else {
        panic!("expected a file dialog")
    };
    assert_eq!(options.mode, FileDialogMode::OpenFiles);
    assert_eq!(options.directory, "/workspace");
    assert!(options.file_name.is_empty());
    assert!(queue.begin_next().is_none());
    assert!(!queue.complete(
        second_scene.native_dialogs[0].id(),
        NativeDialogResult::Canceled
    ));

    queue.sync(&[&second_scene.native_dialogs[0]]);
    assert!(request.is_abandoned());
    assert!(queue.complete(
        request.id,
        NativeDialogResult::Files(vec!["/stale.txt".into()])
    ));
    assert!(first.get_paths().is_empty());
    assert!(first.render_shared().native_dialogs.is_empty());

    let saving = queue.begin_next().unwrap();
    let NativeDialogOptions::File(options) = saving.options.as_ref() else {
        panic!("expected a file dialog")
    };
    assert_eq!(options.mode, FileDialogMode::SaveFile);
    assert_eq!(options.file_name, "notes.txt");
    assert!(queue.complete(
        saving.id,
        NativeDialogResult::Files(vec!["/notes.txt".into()])
    ));
    assert_eq!(second.get_paths(), ["/notes.txt"]);
    assert_eq!(second.get_status(), "已选择");

    second.invoke_choose(FileDialogMode::SelectFolder);
    let scene = second.render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let folder = queue.begin_next().unwrap();
    assert_ne!(folder.id, saving.id);
    assert!(!queue.complete(saving.id, NativeDialogResult::Canceled));
    assert!(queue.complete(folder.id, NativeDialogResult::Canceled));
    assert_eq!(second.get_paths(), ["/notes.txt"]);
    assert_eq!(second.get_status(), "已取消");
}

#[test]
fn file_dialog_commits_paths_before_callbacks_and_preserves_the_last_accepted_result() {
    use xrack_runtime::{
        FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
    };
    let mut ui = FileDialogDemo::new();
    let mut queue = NativeDialogQueue::default();
    assert!(!ui.get_pending());
    assert!(ui.get_paths().is_empty());
    assert!(ui.get_first_path().is_empty());
    ui.invoke_choose(FileDialogMode::SelectFolders);
    assert!(ui.get_pending());
    let scene = ui.render_shared();
    let registration = scene.native_dialogs[0].clone();
    queue.sync(&[&registration]);
    let request = queue.begin_next().unwrap();
    let NativeDialogOptions::File(options) = request.options.as_ref() else {
        panic!("expected FileDialog")
    };
    assert_eq!(options.mode, FileDialogMode::SelectFolders);
    let paths = ["/workspace/design", "/workspace/src"];
    assert!(queue.complete(
        request.id,
        NativeDialogResult::Files(paths.map(str::to_owned).into())
    ));
    assert!(!ui.get_pending());
    assert_eq!(ui.get_paths(), paths);
    assert_eq!(ui.get_first_path(), paths[0]);
    assert_eq!(ui.get_callback_path(), paths[0]);
    assert_eq!(ui.get_status(), "已选择");

    for result in [
        NativeDialogResult::Canceled,
        NativeDialogResult::Failed("Panel unavailable".into()),
    ] {
        ui.invoke_choose(FileDialogMode::OpenFile);
        assert!(ui.get_pending());
        assert!(
            !registration
                .clone()
                .deliver(NativeDialogResult::Files(vec!["/stale".into()]))
        );
        assert!(ui.get_pending());
        let scene = ui.render_shared();
        queue.sync(&[&scene.native_dialogs[0]]);
        let request = queue.begin_next().unwrap();
        assert!(queue.complete(request.id, result));
        assert!(!ui.get_pending());
        assert_eq!(ui.get_paths(), paths);
    }
    ui.invoke_choose(FileDialogMode::SelectFolders);
    let scene = ui.render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let request = queue.begin_next().unwrap();
    queue.sync(&[]);
    assert!(queue.complete(
        request.id,
        NativeDialogResult::Files(vec!["/abandoned".into()])
    ));
    assert!(!ui.get_pending());
    assert_eq!(ui.get_paths(), paths);
}

#[test]
fn messages_and_file_panels_share_request_order_and_preserve_each_button_result() {
    use xrack_runtime::{
        FileDialogMode, MessageButtons, MessageLevel, NativeDialogOptions, NativeDialogQueue,
        NativeDialogResult,
    };

    let mut message = MessageDialogDemo::new();
    message.set_confirm_enabled(false);
    message.invoke_ask();
    assert!(message.render_shared().native_dialogs.is_empty());
    message.set_confirm_enabled(true);
    message.invoke_ask();
    let message_scene = message.render_shared();

    let mut files = FileDialogDemo::new();
    files.invoke_choose(FileDialogMode::OpenFile);
    let file_scene = files.render_shared();
    let mut queue = NativeDialogQueue::default();
    queue.sync(&[
        &file_scene.native_dialogs[0],
        &message_scene.native_dialogs[0],
    ]);
    let prompt = queue.begin_next().unwrap();
    let NativeDialogOptions::Message(options) = prompt.options.as_ref() else {
        panic!("the message was requested first")
    };
    assert_eq!(options.buttons, MessageButtons::YesNoCancel);
    assert_eq!(options.level, MessageLevel::Warning);
    assert!(queue.begin_next().is_none());
    assert!(queue.complete(
        prompt.id,
        NativeDialogResult::Button(StandardButtonKind::No)
    ));
    assert_eq!(message.get_answer(), StandardButtonKind::No);
    assert_eq!(message.get_rejects(), 1);

    let file = queue.begin_next().unwrap();
    assert!(matches!(
        file.options.as_ref(),
        NativeDialogOptions::File(_)
    ));
    assert!(queue.complete(file.id, NativeDialogResult::Canceled));
    assert_eq!(files.get_status(), "已取消");

    for button in [StandardButtonKind::Yes, StandardButtonKind::Cancel] {
        message.invoke_ask();
        let scene = message.render_shared();
        queue.sync(&[&scene.native_dialogs[0]]);
        let current = queue.begin_next().unwrap();
        assert!(queue.complete(current.id, NativeDialogResult::Button(button)));
        assert_eq!(message.get_answer(), button);
    }
    assert_eq!(message.get_accepts(), 1);
    assert_eq!(message.get_rejects(), 2);
    assert_eq!(message.get_status(), "已取消");

    message.invoke_ask();
    let scene = message.render_shared();
    queue.sync(&[&scene.native_dialogs[0]]);
    let closing = queue.begin_next().unwrap();
    queue.close();
    assert!(closing.is_abandoned());
    assert!(message.render_shared().native_dialogs.is_empty());
    assert!(!queue.complete(
        closing.id,
        NativeDialogResult::Button(StandardButtonKind::Yes)
    ));
    assert_eq!(message.get_accepts(), 1);
}

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
    assert_eq!(menu.rows.as_ref(), ["cloud files", "cloud storage"]);
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
fn custom_calendar_cells_share_date_geometry_and_keep_grid_activation() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::{CalendarAction, Date};
    let mut ui = CalendarAgendaDemo::new();
    let mut input = Interaction::default();
    let path = [6];
    let prepare = |ui: &CalendarAgendaDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(576.0, 478.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let node = scene.at_path(&path).unwrap();
    let bounds = layout.at_path(&path).unwrap().bounds;
    let geometry = CalendarGeometry::new(node, bounds);
    let review = ui.get_review_date();
    let day = geometry.days().find(|day| day.date == review).unwrap();
    let cells = node.calendar_cells().unwrap();
    assert_eq!(cells.len(), 42);
    assert_eq!(cells[day.slot].text("accessible-description"), Some("评审"));
    assert_eq!(cells[day.slot].children[0].text("text"), Some("12"));
    let item = layout.at_path(&[6, 0, day.slot]).unwrap();
    assert_eq!(item.bounds, day.bounds);
    assert_eq!(item.clip, day.bounds);
    let before = cells[day.slot].properties.get("background").cloned();
    let point = Point {
        x: day.bounds.x + 18.0,
        y: day.bounds.y + 34.0,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    let action = input.control_part_release(&scene, &layout).unwrap();
    assert_eq!(action.path, path);
    assert_eq!(
        action.action,
        ControlAction::Calendar(CalendarAction::Activate(review))
    );
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_activations(), 1);
    assert_eq!(ui.get_selected(), review);

    ui.set_review_label("复盘".into());
    let (scene, _) = prepare(&ui, &input);
    let cell = &scene.at_path(&path).unwrap().calendar_cells().unwrap()[day.slot];
    assert_eq!(cell.text("accessible-description"), Some("复盘"));
    assert_ne!(cell.properties.get("background").cloned(), before);
    ui.set_month(Date::MAX.first_of_month());
    let (scene, layout) = prepare(&ui, &input);
    let node = scene.at_path(&path).unwrap();
    let cells = node.calendar_cells().unwrap();
    let geometry = CalendarGeometry::new(node, layout.at_path(&path).unwrap().bounds);
    assert_eq!(cells.len(), 42);
    assert_eq!(
        cells.iter().filter(|cell| cell.is_visible()).count(),
        geometry.days().count()
    );
    assert!(!cells.last().unwrap().is_visible());
}

#[test]
fn month_grid_composes_headers_and_separates_selection_activation_and_browsing() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::{CalendarAction, Date};
    let mut ui = MonthGridDemo::new();
    let mut input = Interaction::default();
    let path = [6];
    let date = |year, month, day| Date::new(year, month, day).unwrap();
    let january = date(2021, 1, 1);
    let prepare = |ui: &MonthGridDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(444.0, 372.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let grid = CalendarGeometry::new(
        scene.at_path(&path).unwrap(),
        layout.at_path(&path).unwrap().bounds,
    );
    let weekdays = CalendarGeometry::new(
        scene.at_path(&[4]).unwrap(),
        layout.at_path(&[4]).unwrap().bounds,
    );
    let weeks = CalendarGeometry::new(
        scene.at_path(&[5]).unwrap(),
        layout.at_path(&[5]).unwrap().bounds,
    );
    assert!(!grid.has_header());
    assert_eq!(grid.days().count(), 42);
    let first = grid.days().next().unwrap();
    assert_eq!(first.date, date(2020, 12, 27));
    assert!(!first.enabled);
    for ((heading, _), day) in weekdays
        .weekdays(scene.at_path(&[4]).unwrap())
        .zip(grid.days().take(7))
    {
        assert_eq!((heading.x, heading.width), (day.bounds.x, day.bounds.width));
    }
    assert_eq!(
        weekdays
            .weekdays(scene.at_path(&[4]).unwrap())
            .next()
            .unwrap()
            .1,
        "日"
    );
    let weeks: Vec<_> = weeks.weeks().collect();
    assert_eq!((weeks[0].year, weeks[0].number), (2020, 53));
    assert_eq!((weeks[1].year, weeks[1].number), (2021, 1));
    assert_eq!(weeks.len(), 6);
    for (week, day) in weeks.iter().zip(grid.days().step_by(7)) {
        assert_eq!(
            (week.bounds.y, week.bounds.height),
            (day.bounds.y, day.bounds.height)
        );
    }

    let adjacent = date(2020, 12, 31);
    let rect = grid
        .action_bounds(CalendarAction::Activate(adjacent))
        .unwrap();
    let point = Point {
        x: rect.x + 14.0,
        y: rect.y + 14.0,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    let action = input.control_part_release(&scene, &layout).unwrap();
    assert_eq!(
        action.action,
        ControlAction::Calendar(CalendarAction::Activate(adjacent))
    );
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_selected(), adjacent);
    assert_eq!(ui.get_month(), january);
    assert_eq!(
        (ui.get_changes(), ui.get_browses(), ui.get_activations()),
        (1, 0, 1)
    );

    let (scene, layout) = prepare(&ui, &input);
    let activation = input
        .calendar_activate(&scene, &layout, input.focused.as_ref().unwrap())
        .unwrap();
    assert!(ui.control_at(&scene, &path, activation.action));
    assert_eq!((ui.get_changes(), ui.get_activations()), (1, 2));
    let browse = input
        .calendar_key(&scene, &layout, NavigationKey::PageDown)
        .unwrap();
    assert!(ui.control_at(&scene, &path, browse.action));
    assert_eq!(ui.get_selected(), adjacent);
    assert_eq!(ui.get_month(), date(2021, 2, 1));
    assert!(!ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_browses(), 1);
    let (scene, layout) = prepare(&ui, &input);
    let home = input
        .calendar_key(&scene, &layout, NavigationKey::Home)
        .unwrap();
    assert!(ui.control_at(&scene, &path, home.action));
    assert_eq!(ui.get_selected(), date(2021, 2, 1));
    assert_eq!((ui.get_changes(), ui.get_activations()), (2, 2));

    ui.set_latest(date(2021, 1, 20));
    let (scene, layout) = prepare(&ui, &input);
    let grid = CalendarGeometry::new(
        scene.at_path(&path).unwrap(),
        layout.at_path(&path).unwrap().bounds,
    );
    assert_eq!(grid.state.month, date(2021, 2, 1));
    assert!(grid.days().all(|day| !day.enabled));
    assert!(
        input
            .calendar_key(&scene, &layout, NavigationKey::Right)
            .is_none()
    );
    assert!(
        input
            .calendar_key(&scene, &layout, NavigationKey::PageUp)
            .is_some()
    );
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
fn color_popup_edits_a_session_draft_and_preserves_hidden_alpha() {
    let mut ui = ColorDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(478.0, 384.0);
    let launcher = [2];
    let dialog = [5];
    let red = [5, 0, 1, 1];
    let green = [5, 0, 2, 2];
    let alpha = [5, 0, 4, 1];
    let preview = [5, 0, 0, 0];
    let original = ui.get_selected();

    let closed = ui.render_shared();
    assert!(closed.at_path(&dialog).unwrap().children.is_empty());
    let layout = xrack_layout::layout(&closed, viewport, &mut FixedText);
    input.accessibility_focus(
        input
            .accessibility_target(&closed, &layout, &launcher)
            .unwrap(),
    );
    assert!(ui.dispatch_at(&closed, &launcher, "clicked"));

    let old = ui.render_shared();
    let layout = xrack_layout::layout(&old, viewport, &mut FixedText);
    input.reconcile(&old, &layout);
    assert!(input.is_focused(&[5, 0, 0, 1]));
    input.focus_next(&old, &layout, false);
    assert!(input.is_focused(&red));
    let action = input
        .slider_key(&old, &layout, SliderKey::End, true)
        .unwrap();
    assert!(ui.adjust_range_at(&old, &action.path, action.action));
    assert!(!ui.adjust_range_at(&old, &red, RangeAction::Decrease));
    let draft = ui.render_shared();
    assert!(ui.control_at(&draft, &green, ControlAction::Spin(SpinAction::Commit(23))));
    assert_eq!(ui.get_selected(), original);
    let draft = ui.render_shared();
    assert_eq!(
        draft.at_path(&preview).unwrap().color("color"),
        Some(Color(0xff17c080))
    );
    let layout = xrack_layout::layout(&draft, viewport, &mut FixedText);
    let escape = input.dialog_escape(&draft, &layout).unwrap();
    assert!(ui.control_at(&draft, &escape.path, escape.action));
    assert_eq!(ui.get_cancels(), 1);
    assert_eq!(ui.get_selected(), original);
    let closed = ui.render_shared();
    let layout = xrack_layout::layout(&closed, viewport, &mut FixedText);
    input.reconcile(&closed, &layout);
    assert!(input.is_focused(&launcher));

    ui.set_picker_open(true);
    let shown = ui.render_shared();
    assert!(!ui.adjust_range_at(&old, &alpha, RangeAction::Maximum));
    assert!(!ui.control_at(&old, &dialog, ControlAction::Dialog(DialogRole::Accept)));
    assert!(ui.adjust_range_at(&shown, &alpha, RangeAction::SetFraction(0.25)));
    let shown = ui.render_shared();
    ui.set_alpha_visible(false);
    assert!(!ui.adjust_range_at(&shown, &alpha, RangeAction::Maximum));
    let draft = ui.render_shared();
    assert!(draft.at_path(&alpha).is_none());
    assert!(ui.control_at(&draft, &green, ControlAction::Spin(SpinAction::Commit(0))));
    let draft = ui.render_shared();
    assert!(ui.control_at(&draft, &dialog, ControlAction::Dialog(DialogRole::Accept)));
    assert_eq!(ui.get_selected(), Color(0x4000c040));
    assert_eq!(ui.get_accepted_color(), ui.get_selected());
    assert_eq!(ui.get_accepts(), 1);
    assert!(!ui.get_picker_open());
}

#[test]
fn color_picker_hex_preserves_alpha_and_blocks_accept_until_valid() {
    let mut driver = UiDriver::new(ColorDemo::new(), FixedText, Size::new(478.0, 384.0));
    let hex = [5, 0, 0, 1];
    let preview = [5, 0, 0, 0];
    let dialog = [5];
    let accept = [5, 1, 1];
    let original = driver.component().get_selected();
    driver.click("open-color");
    assert!(driver.input().is_focused(&hex));
    let old = driver.scene().clone();

    driver.fill_focused("#11223");
    assert_eq!(
        driver.scene().at_path(&preview).unwrap().color("color"),
        Some(original)
    );
    assert!(
        !driver
            .scene()
            .at_path(&accept)
            .unwrap()
            .boolean("enabled", true)
    );
    driver.update(|ui| {
        assert!(!ui.control_at(&old, &dialog, ControlAction::Dialog(DialogRole::Accept)));
        assert!(!ui.edit_at(&old, &hex, "#00FF0080".into()));
    });
    driver.fill_focused("#11223372");
    assert_eq!(
        driver.scene().at_path(&preview).unwrap().color("color"),
        Some(Color(0x11223372))
    );
    assert!(
        driver
            .scene()
            .at_path(&accept)
            .unwrap()
            .boolean("enabled", true)
    );
    assert_eq!(driver.component().get_selected(), original);
    driver.update(|ui| ui.set_alpha_visible(false));
    assert_eq!(
        driver.scene().at_path(&hex).unwrap().text("text"),
        Some("#112233")
    );

    driver.fill_focused("AB34CD");
    assert_eq!(
        driver.scene().at_path(&preview).unwrap().color("color"),
        Some(Color(0xab34cd72))
    );
    driver.click_at_path(&accept);
    assert_eq!(driver.component().get_selected(), Color(0xab34cd72));
    assert_eq!(driver.component().get_accepted_color(), Color(0xab34cd72));
    assert_eq!(driver.component().get_accepts(), 1);
    assert!(!driver.component().get_picker_open());
}

#[test]
fn color_plane_keeps_hue_through_black_and_drags_both_axes() {
    use xrack_layout::ColorPlaneGeometry;

    let mut ui = ColorDemo::new();
    ui.set_selected(Color(0x00000040));
    ui.set_picker_open(true);
    ui.set_plane_visible(true);
    ui.set_alpha_visible(false);
    let viewport = Size::new(478.0, 540.0);
    let plane = [5, 0, 5, 0];
    let hue = [5, 0, 5, 1];
    let preview = [5, 0, 0, 0];
    let before = ui.render_shared();
    assert!(ui.adjust_range_at(&before, &hue, RangeAction::SetFraction(1.0 / 3.0)));
    let scene = ui.render_shared();
    assert_eq!(
        scene.at_path(&preview).unwrap().color("color"),
        Some(Color(0x00000040))
    );

    let mut input = Interaction::default();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let geometry = ColorPlaneGeometry::new(layout.at_path(&plane).unwrap().bounds);
    input.reconcile(&scene, &layout);
    input.pointer_move(&scene, &layout, geometry.point(1.0, 1.0));
    input.pointer_press_in_scene(&scene, &layout);
    let action = input.slider_press(&scene, &layout).unwrap();
    assert_eq!(action.path, plane);
    assert!(ui.adjust_range_at(&scene, &action.path, action.action));
    assert!(!ui.adjust_range_at(&scene, &plane, RangeAction::SetPoint { x: 0.5, y: 0.5 }));
    let green = ui.render_shared();
    assert_eq!(
        green.at_path(&preview).unwrap().color("color"),
        Some(Color(0x00ff0040))
    );

    input.reconcile(&green, &layout);
    input.pointer_move(&green, &layout, geometry.point(1.0, 0.0));
    let drag = input.slider_drag(&green, &layout).unwrap();
    assert!(ui.adjust_range_at(&green, &drag.path, drag.action));
    let black = ui.render_shared();
    assert_eq!(
        black.at_path(&preview).unwrap().color("color"),
        Some(Color(0x00000040))
    );
    assert_eq!(
        input.slider_release(&black, &layout).unwrap().action,
        RangeAction::Release
    );
    let up = input
        .slider_key(&black, &layout, SliderKey::Up, true)
        .unwrap();
    assert!(ui.adjust_range_at(&black, &up.path, up.action));
    let draft = ui.render_shared();
    assert_eq!(
        draft.at_path(&preview).unwrap().color("color"),
        Some(Color(0x00030040))
    );
    assert_eq!(ui.get_selected(), Color(0x00000040));
    ui.set_plane_visible(false);
    assert!(!ui.adjust_range_at(&draft, &hue, RangeAction::Maximum));
}

#[test]
fn font_picker_selects_a_catalog_family_and_commits_the_draft_as_one_value() {
    use xrack_runtime::{Font, Length, SpinDecimal};

    struct CatalogText([String; 2]);
    impl TextMeasurer for CatalogText {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            FixedText.measure(text, size, width)
        }

        fn font_families(&self) -> &[String] {
            &self.0
        }
    }

    let text = CatalogText(["Catalog Sans".into(), "Catalog Serif".into()]);
    let mut driver = UiDriver::new(FontPickerDemo::new(), text, Size::new(510.0, 430.0));
    let original = driver.component().get_selected();
    let popup = [3];
    let family = [3, 0, 0, 1];
    let size = [3, 0, 1, 1];
    let weight = [3, 0, 2, 1];
    let style = [3, 0, 3, 1];
    let preview = [3, 0, 4];

    driver.click("open-font");
    let old = driver.scene().clone();
    assert_eq!(
        old.at_path(&family).unwrap().strings("model"),
        ["sans-serif", "Catalog Sans", "Catalog Serif",]
    );
    driver.click_at_path(&family);
    driver.choice_key(ChoiceKey::End);
    driver.choice_key(ChoiceKey::Accept);
    assert_eq!(
        driver
            .scene()
            .at_path(&preview)
            .unwrap()
            .font("font")
            .unwrap()
            .family,
        "Catalog Serif"
    );
    assert_eq!(driver.component().get_selected(), original);
    driver.escape();
    assert_eq!(driver.component().get_cancels(), 1);
    assert_eq!(driver.component().get_selected(), original);
    assert_eq!(driver.focused(), Some("open-font"));

    driver.click("open-font");
    let fresh = driver.scene().clone();
    driver.update(|ui| {
        assert!(!ui.control_at(&old, &family, ControlAction::Select(1)));
        assert!(!ui.control_at(&old, &popup, ControlAction::Dialog(DialogRole::Accept)));
        assert!(ui.control_at(&fresh, &family, ControlAction::Select(1)));
        assert!(!ui.control_at(&fresh, &family, ControlAction::Select(2)));
        let value = SpinDecimal::new(31.5).unwrap();
        assert!(ui.control_at(
            &fresh,
            &size,
            ControlAction::Spin(SpinAction::CommitDecimal(value))
        ));
        assert!(ui.control_at(
            &fresh,
            &weight,
            ControlAction::Spin(SpinAction::Commit(650))
        ));
    });
    driver.click_at_path(&style);
    driver.choice_key(ChoiceKey::End);
    driver.choice_key(ChoiceKey::Accept);
    let expected = Font {
        family: "Catalog Sans".into(),
        size: Length(31.5),
        weight: 650,
        italic: true,
    };
    assert_eq!(
        driver.scene().at_path(&preview).unwrap().font("font"),
        Some(&expected)
    );
    assert_eq!(driver.component().get_selected(), original);

    driver.click_at_path(&[3, 1, 1]);
    assert_eq!(driver.component().get_selected(), expected);
    assert_eq!(driver.scene().children[0].font("font"), Some(&expected));
    assert_eq!(driver.component().get_accepted_font(), expected);
    assert_eq!(driver.component().get_accepts(), 1);
    assert!(!driver.component().get_picker_open());
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
fn check_group_master_summarizes_and_updates_the_bound_members_across_layouts() {
    let mut driver = UiDriver::new(CheckGroupDemo::new(), FixedText, Size::new(520.0, 266.0));
    assert_eq!(
        driver.node("select-all").check_state(),
        CheckState::PartiallyChecked
    );
    driver.update(|ui| ui.set_show_validation(false));
    assert_eq!(
        driver.component().get_summary(),
        CheckState::PartiallyChecked
    );
    assert!(!driver.can_hit("include-metadata"));
    driver.click("select-all");
    assert!(
        driver.component().get_assets()
            && driver.component().get_metadata()
            && driver.component().get_background_work()
    );
    assert_eq!(driver.component().get_validation(), CheckState::Checked);
    assert_eq!(driver.component().get_master_result(), CheckState::Checked);
    assert_eq!(driver.node("select-all").check_state(), CheckState::Checked);
    assert_eq!(driver.component().get_child_edits(), 0);
    driver.click("select-all");
    assert_eq!(driver.component().get_summary(), CheckState::Unchecked);
    assert_eq!(driver.component().get_validation(), CheckState::Unchecked);
    assert!(!driver.component().get_background_work());

    driver.update(|ui| ui.set_allow_metadata(true));
    driver.tab();
    assert_eq!(driver.focused(), Some("include-assets"));
    driver.tab();
    assert_eq!(driver.focused(), Some("include-metadata"));
    driver.click("include-assets");
    assert_eq!(driver.component().get_child_edits(), 1);
    assert_eq!(
        driver.component().get_summary(),
        CheckState::PartiallyChecked
    );
    assert!(!driver.component().get_metadata());
    driver.update(|ui| ui.invoke_apply_all(CheckState::PartiallyChecked));
    assert_eq!(
        driver.component().get_master_result(),
        CheckState::PartiallyChecked
    );
    driver.update(|ui| ui.invoke_apply_all(CheckState::Checked));
    assert_eq!(
        driver.component().get_before_bulk(),
        CheckState::PartiallyChecked
    );
    assert_eq!(driver.component().get_master_result(), CheckState::Checked);
    assert_eq!(driver.component().get_child_edits(), 1);
    driver.update(|ui| ui.set_validation(CheckState::PartiallyChecked));
    assert_eq!(
        driver.node("select-all").check_state(),
        CheckState::PartiallyChecked
    );
    driver.update(|ui| ui.set_group_enabled(false));
    assert!(!driver.can_hit("include-assets"));
    assert!(!driver.can_hit("use-background"));
}

#[test]
fn button_group_uses_one_tab_stop_and_keeps_values_and_slots_when_options_change() {
    let mut driver = UiDriver::new(GroupedRadiosDemo::new(), FixedText, Size::new(540.0, 280.0));
    assert!(driver.node("balanced").boolean("checked", false));
    driver.update(|ui| ui.set_balanced_label("推荐".into()));
    assert_eq!(driver.node("balanced").text("text"), Some("推荐"));
    assert!(driver.node("balanced").boolean("checked", false));
    driver.tab();
    assert_eq!(driver.focused(), Some("balanced"));
    assert!(driver.group_key(NavigationKey::Right));
    assert_eq!(driver.focused(), Some("fast"));
    assert_eq!(driver.component().get_mode(), "fast");
    assert!(driver.group_key(NavigationKey::Down));
    assert_eq!(driver.focused(), Some("audit"));
    assert_eq!(driver.component().get_last_mode(), "audit");
    driver.update(|ui| ui.set_show_audit(false));
    assert_eq!(driver.component().get_mode(), "audit");
    assert_eq!(driver.focused(), Some("fast"));
    driver.update(|ui| ui.set_show_audit(true));
    assert!(driver.node("audit").boolean("checked", false));
    driver.update(|ui| ui.set_can_audit(false));
    assert!(driver.group_key(NavigationKey::Right));
    assert_eq!(driver.focused(), Some("balanced"));
    assert_eq!(driver.component().get_mode(), "balanced");
    driver.tab();
    assert_eq!(driver.focused(), Some("high-quality"));
    driver.update(|ui| ui.set_show_high(false));
    assert_eq!(driver.component().get_quality(), 1);
    assert_eq!(driver.focused(), Some("low-quality"));
    driver.click("ultra-quality");
    assert_eq!(driver.component().get_quality(), 2);
    assert_eq!(driver.component().get_mode(), "balanced");
    driver.update(|ui| ui.set_mode_enabled(false));
    assert!(!driver.can_hit("fast"));
    assert!(driver.can_hit("ultra-quality"));
}

#[test]
fn record_combo_checks_enabled_rows_on_hover_accept_and_commit() {
    let mut ui = RecordComboDemo::new();
    let model = ui.model_options();
    let path = [1];
    let render = |ui: &RecordComboDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(472.0, 296.0), &mut FixedText);
        (scene, layout)
    };
    let (scene, layout) = render(&ui);
    let mut input = Interaction::default();
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target.clone());
    ui.set_code(300);
    let (scene, layout) = render(&ui);
    let selection = input.choice_key(&scene, &layout, ChoiceKey::Down).unwrap();
    assert_eq!(selection.action, ControlAction::Select(4));
    assert!(ui.control_at(&scene, &path, selection.action));
    assert_eq!(ui.get_code(), 500);

    ui.set_code(400);
    let (scene, layout) = render(&ui);
    assert_eq!(
        xrack_runtime::combo_text(scene.at_path(&path).unwrap()),
        "Build archive"
    );
    assert!(input.open_choice(&scene, &layout, target));
    let menu = input.choice_menu(&scene, &layout).unwrap();
    assert_eq!(menu.selected, Some(3));
    assert_eq!(menu.highlighted, Some(4));
    assert!(!menu.row_enabled(3 - menu.first));
    let row_point = |row: usize| {
        let bounds = menu.row_bounds(row - menu.first);
        Point {
            x: bounds.x + 12.0,
            y: bounds.y + 8.0,
        }
    };
    input.pointer_move(&scene, &layout, row_point(3));
    assert!(input.choice_press(&scene, &layout));
    assert!(input.choice_release(&scene, &layout).is_none());
    assert!(input.choice_open());
    assert!(!ui.control_at(&scene, &path, ControlAction::Select(3)));

    input.pointer_move(&scene, &layout, row_point(4));
    assert!(input.choice_press(&scene, &layout));
    let mut row = model.row(4).unwrap();
    row.access.available = false;
    assert!(model.set_row(4, row));
    assert!(ui.scene_dirty());
    assert!(!ui.control_at(&scene, &path, ControlAction::Select(4)));
    let (updated, layout) = render(&ui);
    input.reconcile(&updated, &layout);
    assert!(input.choice_release(&updated, &layout).is_none());
    assert!(
        input
            .choice_key(&updated, &layout, ChoiceKey::Accept)
            .is_none()
    );
    assert_eq!(ui.get_code(), 400);
    let replacement = model.snapshot();
    ui.set_options(replacement);
    assert!(!ui.control_at(&updated, &path, ControlAction::Select(5)));
    let disabled = model
        .snapshot()
        .into_iter()
        .map(|mut row| {
            row.access.available = false;
            row
        })
        .collect();
    ui.set_options(disabled);
    let (scene, layout) = render(&ui);
    input.reconcile(&scene, &layout);
    assert!(input.choice_open());
    input.choice_key(&scene, &layout, ChoiceKey::End);
    assert_eq!(
        input.choice_menu(&scene, &layout).unwrap().highlighted,
        None
    );
    assert!(
        input
            .choice_key(&scene, &layout, ChoiceKey::Accept)
            .is_none()
    );
}

#[test]
fn combo_typeahead_wraps_enabled_rows_and_only_commits_a_closed_selection() {
    let mut ui = RecordComboDemo::new();
    let path = [1];
    let render = |ui: &RecordComboDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(472.0, 296.0), &mut FixedText);
        (scene, layout)
    };
    let (scene, layout) = render(&ui);
    let mut input = Interaction::default();
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target.clone());
    let now = Instant::now();
    for (key, offset, expected) in [
        ("B", 0, 500),
        ("o", 100, 600),
        ("b", 1_000, 500),
        ("B", 1_100, 600),
    ] {
        let (scene, layout) = render(&ui);
        let action = input
            .choice_typeahead(&scene, &layout, key, now + Duration::from_millis(offset))
            .unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));
        assert_eq!(ui.get_code(), expected);
    }
    let (scene, layout) = render(&ui);
    assert!(input.open_choice(&scene, &layout, target));
    assert!(
        input
            .choice_typeahead(&scene, &layout, "BU", now + Duration::from_secs(2))
            .is_none()
    );
    assert_eq!(
        input.choice_menu(&scene, &layout).unwrap().highlighted,
        Some(4)
    );
    assert_eq!(ui.get_code(), 600);
    assert!(
        input
            .choice_key(&scene, &layout, ChoiceKey::Escape)
            .is_none()
    );
    assert_eq!(ui.get_code(), 600);
}

#[test]
fn editable_record_combo_does_not_accept_a_disabled_label_as_custom_text() {
    let mut ui = RecordComboDemo::new();
    ui.set_editable(true);
    let path = [1];
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, Size::new(472.0, 296.0), &mut FixedText);
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target);
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("Bu"),
        EditSource::Atomic,
        Instant::now()
    ));
    let scene = ui.render_shared();
    let completion = input
        .combo_completion(scene.at_path(&path).unwrap(), &path)
        .unwrap();
    assert_eq!(completion.suffix(), "ild");
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("Build archive"),
        EditSource::Atomic,
        Instant::now()
    ));
    let scene = ui.render_shared();
    assert!(!ui.control_at(&scene, &path, ControlAction::AcceptComboText));
    assert_eq!(ui.get_code(), 200);
    assert_eq!(ui.get_accepted_text(), "");
    assert_eq!(ui.get_selected_code(), -1);

    let model = ui.model_options();
    let mut row = model.row(0).unwrap();
    row.access.available = false;
    assert!(model.set_row(0, row));
    let scene = ui.render_shared();
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("共享工作区"),
        EditSource::Atomic,
        Instant::now()
    ));
    let scene = ui.render_shared();
    assert!(ui.control_at(&scene, &path, ControlAction::AcceptComboText));
    assert_eq!(ui.get_code(), 100);
}

#[test]
fn record_combo_tracks_keys_and_guards_pressed_rows_across_reorder() {
    use xrack_runtime::{combo_index, combo_text};

    let mut ui = RecordComboDemo::new();
    let model = ui.model_options();
    let path = [1];
    let viewport = Size::new(472.0, 296.0);
    let render = |ui: &RecordComboDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
        (scene, layout)
    };
    let (scene, layout) = render(&ui);
    let node = scene.at_path(&path).unwrap();
    assert_eq!(combo_index(node), Some(0));
    assert_eq!(combo_text(node), "共享工作区");
    let mut first = model.row(0).unwrap();
    first.notes = "unrelated field".into();
    assert!(model.set_row(0, first));
    assert!(!ui.scene_dirty());
    assert!(std::rc::Rc::ptr_eq(&scene, &ui.render_shared()));

    let mut input = Interaction::default();
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    assert!(input.open_choice(&scene, &layout, target));
    let first = input.choice_menu(&scene, &layout).unwrap().row_bounds(0);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: first.x + 14.0,
            y: first.y + 10.0,
        },
    );
    assert!(input.choice_press(&scene, &layout));
    assert!(model.sort_by(|a, b| a.code.cmp(&b.code)));
    let (scene, layout) = render(&ui);
    input.reconcile(&scene, &layout);
    let menu = input.choice_menu(&scene, &layout).unwrap();
    assert_eq!(menu.highlighted, Some(1));
    assert_eq!(menu.selected, Some(1));
    assert!(
        input.choice_release(&scene, &layout).is_none(),
        "the same label now belongs to a different record"
    );
    assert_eq!(ui.get_code(), 200);

    input.choice_key(&scene, &layout, ChoiceKey::Home);
    let selection = input
        .choice_key(&scene, &layout, ChoiceKey::Accept)
        .unwrap();
    assert!(ui.control_at(&scene, &selection.path, selection.action));
    assert_eq!(ui.get_code(), 100);
    assert_eq!(ui.get_selected_code(), 100);
    assert_eq!(ui.get_selected_label(), "共享工作区");

    ui.set_editable(true);
    let edit_and_accept = |ui: &mut RecordComboDemo, input: &mut Interaction, text| {
        let (scene, layout) = render(ui);
        input.reconcile(&scene, &layout);
        input.sync_editor(&scene);
        assert!(input.edit_text(
            ui,
            &scene,
            EditAction::ReplaceAll(text),
            EditSource::Atomic,
            Instant::now()
        ));
        let (scene, layout) = render(ui);
        let accepted = input
            .choice_key(&scene, &layout, ChoiceKey::Accept)
            .unwrap();
        assert!(ui.control_at(&scene, &accepted.path, accepted.action));
    };
    edit_and_accept(&mut ui, &mut input, "custom");
    assert_eq!(ui.get_accepted_text(), "custom");
    assert_eq!(ui.get_code(), 100);
    edit_and_accept(&mut ui, &mut input, "归档");
    assert_eq!(ui.get_code(), 300);
    assert_eq!(ui.get_selected_code(), 300);
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
fn tree_commands_reveal_ancestors_and_restore_current_to_a_visible_parent() {
    use xrack_runtime::TreeCommand;

    let mut ui = UiDriver::new(TreeCommandsDemo::new(), FixedText, Size::new(610.0, 440.0));
    assert_eq!(
        ui.component()
            .tree_snapshot("directory")
            .unwrap()
            .visible_index(103),
        None
    );
    ui.click("reveal-node");
    assert_eq!(ui.component().get_current(), 103);
    assert_eq!(ui.component().get_expansions(), 1);
    assert_eq!(ui.component().get_last_event_key(), 103);
    assert!(
        ui.component()
            .tree_snapshot("directory")
            .unwrap()
            .is_expanded(100)
    );
    assert_eq!(
        ui.node("node-status").text("text"),
        Some("节点 103 · 深度 3 · 显示行 3")
    );
    ui.click("collapse-all");
    assert_eq!(ui.component().get_current(), 1);
    assert_eq!(ui.component().get_collapses(), 1);
    assert_eq!(ui.component().get_last_event_key(), -1);
    ui.click("expand-all");
    assert_eq!(ui.component().get_expansions(), 2);
    assert_eq!(
        ui.component()
            .tree_snapshot("directory")
            .unwrap()
            .rows()
            .len(),
        8
    );
    ui.update(|view| {
        assert!(view.model_records().sort_by(|a, b| b.key.cmp(&a.key)));
        assert!(view.command_tree("directory", TreeCommand::Collapse(1)));
    });
    let collapsed = ui.component().tree_snapshot("directory").unwrap();
    assert!(collapsed.is_expanded(100));
    assert_eq!(collapsed.visible_index(103), None);
    ui.update(|view| {
        assert!(view.command_tree("directory", TreeCommand::Expand(1)));
    });
    assert!(
        ui.component()
            .tree_snapshot("directory")
            .unwrap()
            .visible_index(103)
            .is_some()
    );
}

#[test]
fn tree_reveal_scrolls_to_the_requested_key_and_keeps_row_virtualization() {
    let mut component = TreeCommandsDemo::new();
    component.set_records(
        (0..10_001)
            .map(|key| SizedBranch {
                key,
                parent: if key == 0 { -1 } else { 0 },
                label: format!("目录 {key}"),
                height: xrack_runtime::Length(34.0),
            })
            .collect(),
    );
    let mut ui = UiDriver::new(component, FixedText, Size::new(610.0, 440.0));
    ui.update(|view| view.invoke_reveal(9000));
    let directory = ui.node("directory");
    assert_eq!(ui.component().get_current(), 9000);
    assert_eq!(directory.tree.as_ref().unwrap().rows().len(), 10_001);
    assert!(directory.list.unwrap().first_row > 8980);
    assert!(directory.children.len() < 16);
    assert_eq!(ui.component().get_expansions(), 1);
}

#[test]
fn tree_branch_views_share_records_and_keep_independent_navigation_state() {
    use xrack_runtime::TreeCommand;

    let mut ui = UiDriver::new(TreeBranchesDemo::new(), FixedText, Size::new(760.0, 430.0));
    ui.update(|view| {
        view.invoke_initialize();
        assert!(view.command_tree("branch", TreeCommand::ExpandTo(103)));
    });
    assert_eq!(
        ui.component().tree_snapshot("branch").unwrap().depth(10),
        Some(0)
    );
    ui.click("source-branch");
    let source = ui.component().tree_snapshot("branch").unwrap();
    assert_eq!(
        source.rows().iter().map(|row| row.key).collect::<Vec<_>>(),
        [100, 103, 104]
    );
    assert_eq!(source.depth(100), Some(0));
    assert_eq!(source.rows()[0].parent, None);
    let old = ui.scene().clone();
    let branch_path = [0, 2, 1, 1];
    ui.update(|view| {
        let model = view.model_records();
        let mut row = model.row(3).unwrap();
        row.label = "updated-window.rs".into();
        assert!(model.set_row(3, row));
    });
    let label = |node: &xrack_runtime::Node, row: usize| {
        node.children[row].children[0].children[0]
            .text("text")
            .unwrap()
            .to_owned()
    };
    assert_eq!(label(ui.node("branch"), 1), "updated-window.rs");
    assert_eq!(label(ui.node("overview"), 3), "updated-window.rs");

    ui.click("docs-branch");
    assert_eq!(
        ui.component().tree_snapshot("branch").unwrap().rows()[0].key,
        20
    );
    ui.update(|view| {
        assert!(!view.control_at(&old, &branch_path, ControlAction::Select(1)));
        assert!(!view.command_tree("branch", TreeCommand::ExpandTo(103)));
    });
    ui.click("source-branch");
    ui.update(|view| {
        assert!(view.command_tree("branch", TreeCommand::CollapseRecursively(-1)));
    });
    assert!(
        !ui.component()
            .tree_snapshot("branch")
            .unwrap()
            .is_expanded(100)
    );
    assert!(
        ui.component()
            .tree_snapshot("overview")
            .unwrap()
            .is_expanded(100)
    );

    ui.update(|view| view.set_current(100));
    let scene = ui.scene();
    let layout = xrack_layout::layout(scene, Size::new(760.0, 430.0), &mut FixedText);
    let mut input = Interaction::default();
    input.accessibility_focus(
        input
            .accessibility_target(scene, &layout, &branch_path)
            .unwrap(),
    );
    assert!(
        input
            .tree_key(scene, &layout, NavigationKey::Left)
            .is_none()
    );
}

#[test]
fn searching_a_tree_branch_reveals_matches_and_restores_the_saved_disclosure() {
    let mut ui = UiDriver::new(TreeBranchesDemo::new(), FixedText, Size::new(760.0, 430.0));
    ui.click("source-branch");
    assert_eq!(ui.node("branch").children.len(), 1);
    ui.click("branch-filter");
    ui.fill_focused("WINDOW");
    let tree = ui.node("branch").tree.as_ref().unwrap();
    assert_eq!(
        tree.rows().iter().map(|row| row.key).collect::<Vec<_>>(),
        [100, 103]
    );
    assert!(tree.is_expanded(100));
    assert_eq!(ui.node("overview").children.len(), 2);

    ui.update(|view| {
        let model = view.model_records();
        let mut row = model.row(4).unwrap();
        row.label = "window-accessibility.rs".into();
        assert!(model.set_row(4, row));
    });
    assert_eq!(ui.node("branch").children.len(), 3);
    ui.update(|view| view.set_query(String::new()));
    assert_eq!(ui.node("branch").children.len(), 1);
    assert!(!ui.node("branch").tree.as_ref().unwrap().is_expanded(100));
}

#[test]
fn changing_tree_branch_returns_an_unbound_scroll_view_to_the_top() {
    let mut component = TreeBranchesDemo::new();
    let mut records = vec![
        SizedBranch {
            key: 1,
            parent: -1,
            label: "项目".into(),
            height: xrack_runtime::Length(34.0),
        },
        SizedBranch {
            key: 2,
            parent: -1,
            label: "文档".into(),
            height: xrack_runtime::Length(34.0),
        },
    ];
    records.extend((0..1000).map(|index| SizedBranch {
        key: 1000 + index,
        parent: if index < 500 { 1 } else { 2 },
        label: format!("记录 {index}"),
        height: xrack_runtime::Length(34.0),
    }));
    component.set_records(records);
    let mut ui = UiDriver::new(component, FixedText, Size::new(760.0, 430.0));
    assert!(ui.scroll("branch", Point { x: 0.0, y: 3000.0 }));
    assert!(ui.node("branch").list.unwrap().first_row > 50);
    ui.click("docs-branch");
    assert_eq!(ui.node("branch").list.unwrap().first_row, 0);
    assert_eq!(ui.node("branch").tree.as_ref().unwrap().rows()[0].key, 1500);
    assert!(ui.node("branch").children.len() < 16);
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
    assert_eq!(
        input.scroll_bar_press(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::Unchanged)
    );
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bar.track.x + 40.0,
            y: bar.track.y + bar.track.height + 20.0,
        },
    );
    assert_eq!(
        input.scroll_bar_drag(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::ViewportMoved)
    );
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
fn absolute_split_resize_updates_the_same_proportions_as_keyboard_steps() {
    use split_ui::CustomSplitDemo;

    let mut ui = CustomSplitDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(640.0, 240.0);
    let prepare = |ui: &CustomSplitDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    let handle = [0, 3];
    let target = input
        .accessibility_target(&scene, &layout, &handle)
        .unwrap();
    let range = layout.split_handle_range(&handle).unwrap();
    input.focus_target(&scene, &layout, target.clone());
    let key = input
        .split_key(&scene, &layout, NavigationKey::Right)
        .unwrap();
    let absolute = input
        .split_set_value(&scene, &layout, &target, range.value + 8.0)
        .unwrap();
    assert_eq!(absolute.weights, key.weights);
    assert!(absolute.sizes.is_none());
    assert!(ui.resize_split_at(&scene, &absolute.path, &absolute.weights));
    assert!(!ui.get_dragging());
    assert!(
        ui.save_split_state("work")
            .unwrap()
            .starts_with("xrack-split/1;")
    );
    let (_, layout) = prepare(&ui, &input);
    let after = layout.split_handle_range(&handle).unwrap();
    assert!((after.value - range.value - 8.0).abs() < 0.01);
}

#[test]
fn custom_split_handles_keep_independent_hover_and_capture_state() {
    use split_ui::CustomSplitDemo;
    use xrack_runtime::Color;

    let mut ui = CustomSplitDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(640.0, 240.0);
    let (scene, layout) =
        prepare_scene_with_input(&ui, ui.render_shared(), viewport, &mut FixedText, &input);
    let first = [0, 3];
    let second = [0, 4];
    let point = |path: &[usize]| {
        let bounds = layout.at_path(path).unwrap().bounds;
        Point {
            x: bounds.x + 5.0,
            y: bounds.y + 32.0,
        }
    };
    let color = |slot| {
        ui.render_shared()
            .at_path(&[0, slot, 0])
            .unwrap()
            .color("background")
            .unwrap()
    };
    assert_eq!(color(3), Color(0x694b8eff));
    assert_eq!(color(4), Color(0x485e7eff));
    input.pointer_move(&scene, &layout, point(&first));
    assert!(input.is_hovered(&first));
    assert_eq!(color(3), Color(0x58c9c7ff));
    assert_eq!(color(4), Color(0x485e7eff));
    input.pointer_press_in_scene(&scene, &layout);
    input.split_press(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, first);
    assert!(ui.get_dragging());
    input.pointer_move(&scene, &layout, point(&second));
    assert_eq!(color(3), Color(0xffbf69ff));
    assert_eq!(color(4), Color(0x58c9c7ff));
    assert!(input.finish_split_resize());
    assert_eq!(color(3), Color(0x694b8eff));
    input.pointer_leave();
    assert_eq!(color(4), Color(0x485e7eff));

    input.pointer_move(&scene, &layout, point(&first));
    input.pointer_press_in_scene(&scene, &layout);
    input.split_press(&scene, &layout);
    assert!(ui.get_dragging());
    ui.set_show_center(false);
    let next = ui.render_shared();
    assert_ne!(
        next.at_path(&[0, 3, 0]).unwrap().color("background"),
        Some(Color(0xffbf69ff))
    );
    assert!(next.at_path(&[0, 4, 0]).is_none());
    let (_, next_layout) =
        prepare_scene_with_input(&ui, next.clone(), viewport, &mut FixedText, &input);
    input.reconcile(&next, &next_layout);
    assert!(!ui.get_dragging());
    assert!(input.pointer_release_target().is_none());
}

#[test]
fn split_hit_size_keeps_visual_bounds_and_tracks_the_drag_lifetime() {
    use split_ui::FillSplitDemo;

    let mut ui = FillSplitDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(680.0, 320.0);
    let prepared = |ui: &FillSplitDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let path = [0, 2, 3];
    let (scene, layout) = prepared(&ui, &input);
    let handle = layout.at_path(&path).unwrap().bounds;
    let point = Point {
        x: handle.x - 6.0,
        y: handle.y + 20.0,
    };
    assert_eq!(handle.width, 8.0);
    assert!(!handle.contains(point));
    assert!(!ui.get_dragging());
    input.pointer_move(&scene, &layout, point);
    assert!(input.is_hovered(&path));
    input.pointer_press_in_scene(&scene, &layout);
    input.split_press(&scene, &layout);
    assert!(ui.get_dragging());
    assert!(input.split_drag(&scene, &layout).is_none());

    ui.set_show_editor(false);
    let (scene, layout) = prepared(&ui, &input);
    input.reconcile(&scene, &layout);
    assert!(!ui.get_dragging());
    assert!(!input.split_resize_active());
    assert!(input.pointer_release_target().is_none());

    ui.set_show_editor(true);
    ui.set_handle_target(xrack_runtime::Length(2.0));
    let (scene, layout) = prepared(&ui, &input);
    assert_eq!(layout.at_path(&path).unwrap().bounds, handle);
    input.pointer_move(&scene, &layout, point);
    assert!(!input.is_hovered(&path));
    let target = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, target);
    assert!(
        input
            .split_key(&scene, &layout, NavigationKey::Right)
            .is_some()
    );
    assert!(!ui.get_dragging());
}

#[test]
fn fill_split_keeps_resized_side_lengths_and_restores_them_by_pane_id() {
    use split_ui::FillSplitDemo;

    let mut ui = FillSplitDemo::new();
    let mut input = Interaction::default();
    let layout_for = |ui: &FillSplitDemo, width, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(width, 320.0),
            &mut FixedText,
            input,
        )
    };
    let widths = |layout: &xrack_layout::LayoutNode| {
        let view = layout.at_path(&[0, 2]).unwrap();
        std::array::from_fn::<_, 3, _>(|slot| view.children[slot].bounds.width)
    };
    let (scene, layout) = layout_for(&ui, 680.0, &input);
    assert_eq!(widths(&layout), [120.0, 356.0, 160.0]);
    let (_, wide) = layout_for(&ui, 880.0, &input);
    assert_eq!(widths(&wide), [120.0, 556.0, 160.0]);

    let handle = input
        .accessibility_target(&scene, &layout, &[0, 2, 3])
        .unwrap();
    input.focus_target(&scene, &layout, handle);
    let resize = input
        .split_key(&scene, &layout, NavigationKey::Right)
        .unwrap();
    let sizes = resize.sizes.as_ref().unwrap();
    assert_eq!(sizes.as_slice(), &[(0, 128.0), (1, 348.0)]);
    assert!(!ui.resize_split_at(&scene, &resize.path, &resize.weights));
    assert!(ui.resize_split_sizes_at(&scene, &resize.path, sizes));
    assert!(!ui.resize_split_sizes_at(&scene, &resize.path, sizes));
    assert_eq!(ui.get_edits(), 1);
    let (_, resized) = layout_for(&ui, 880.0, &input);
    assert_eq!(widths(&resized), [128.0, 548.0, 160.0]);
    let (_, narrow) = layout_for(&ui, 390.0, &input);
    let [left, center, right] = widths(&narrow);
    assert!(left >= 80.0 && right >= 100.0);
    assert_eq!(center, 130.0);
    assert!((left + center + right - 346.0).abs() < 0.01);
    let saved = ui.save_split_state("work").unwrap();
    assert!(saved.starts_with("xrack-split/2;"));
    assert!(saved.contains(";sidebar=128"));
    assert!(saved.contains(";inspector=-"));
    let mut restored = FillSplitDemo::new();
    assert!(
        !restored.restore_split_state("work", &saved.replacen("xrack-split/2", "xrack-split/1", 1))
    );
    assert!(restored.restore_split_state("work", &saved));
    let (_, restored_layout) = layout_for(&restored, 680.0, &input);
    assert_eq!(widths(&restored_layout), [128.0, 348.0, 160.0]);
    restored.set_show_editor(false);
    let (_, fallback) = layout_for(&restored, 680.0, &input);
    assert_eq!(widths(&fallback), [128.0, 0.0, 516.0]);

    restored.set_show_editor(true);
    let (scene, narrow) = layout_for(&restored, 390.0, &input);
    let before = widths(&narrow);
    let handle = input
        .accessibility_target(&scene, &narrow, &[0, 2, 3])
        .unwrap();
    input.focus_target(&scene, &narrow, handle);
    let resize = input
        .split_key(&scene, &narrow, NavigationKey::Left)
        .unwrap();
    let sizes = resize.sizes.as_deref().unwrap();
    assert_eq!(sizes.len(), 3);
    assert!(restored.resize_split_sizes_at(&scene, &resize.path, sizes));
    let (_, narrow) = layout_for(&restored, 390.0, &input);
    assert_eq!(
        widths(&narrow),
        [before[0] - 8.0, before[1] + 8.0, before[2]]
    );
}

#[test]
fn split_state_restores_hidden_panes_in_a_fresh_component_and_keeps_constraints() {
    let viewport = Size::new(760.0, 420.0);
    let work = [0, 2];
    let mut source = SplitDemo::new();
    let initial = source.render_shared();
    let mut input = Interaction::default();
    let layout = xrack_layout::layout(&initial, viewport, &mut FixedText);
    let handle = layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .handles[0]
        .slot;
    let handle_path = [0, 2, handle];
    input.accessibility_focus(
        input
            .accessibility_target(&initial, &layout, &handle_path)
            .unwrap(),
    );
    let resize = input
        .split_key(&initial, &layout, NavigationKey::Right)
        .unwrap();
    assert!(source.resize_split_at(&initial, &resize.path, &resize.weights));
    let saved = source.save_split_state("work").unwrap();
    assert!(saved.contains("preview-pane="));
    assert!(!source.resize_split_at(&initial, &resize.path, &resize.weights));
    let edited = source.render_shared();
    let expected = xrack_layout::layout(&edited, viewport, &mut FixedText);

    let mut restored = SplitDemo::new();
    restored.set_show_preview(false);
    assert!(restored.restore_split_state("work", &saved));
    assert!(restored.restore_split_state("work", &saved));
    assert!(!restored.restore_split_state("work", "xrack-split/1;navigation=NaN"));
    assert_eq!(restored.get_edits(), 1);
    restored.set_show_preview(true);
    let scene = restored.render_shared();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    for slot in 0..3 {
        let pane = [0, 2, slot];
        let width = layout.at_path(&pane).unwrap().bounds.width;
        assert!((width - expected.at_path(&pane).unwrap().bounds.width).abs() < 0.05);
    }
    let narrow = xrack_layout::layout(&scene, Size::new(500.0, 420.0), &mut FixedText);
    assert!(narrow.at_path(&[0, 2, 0]).unwrap().bounds.width >= 120.0);
    assert!(narrow.at_path(&[0, 2, 1]).unwrap().bounds.width >= 140.0);
}

#[test]
fn split_demo_can_save_and_restore_its_layout_from_dsl_callbacks() {
    let mut ui = UiDriver::new(SplitDemo::new(), FixedText, Size::new(760.0, 420.0));
    ui.click("save-layout");
    let saved = ui.component().get_saved_layout();
    assert_eq!(saved, ui.component().save_split_state("work").unwrap());
    ui.update(|view| {
        assert!(view.restore_split_state(
            "work",
            "xrack-split/1;navigation=0.4;preview-pane=0.2;detail=0.4",
        ));
    });
    assert_ne!(ui.component().save_split_state("work").unwrap(), saved);
    ui.click("restore-layout");
    assert_eq!(ui.component().save_split_state("work").unwrap(), saved);
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
        &[0, 1, 0][..],
        &[0, 1, 1][..],
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
fn custom_table_cells_share_model_identity_and_keep_local_state_through_scroll_and_sort() {
    let mut ui = DelegateTableDemo::new();
    ui.set_records(
        (0..200)
            .map(|index| StockRow {
                name: format!("{index:03}"),
                units: index,
                available: true,
                ..StockRow::default()
            })
            .collect(),
    );
    let model = ui.model_records();
    let key = model.row_key(0).unwrap();
    let table = [0, 1];
    let text = [0, 1, 0, 0, 0, 0];
    let star = [0, 1, 0, 0, 0, 1];
    let spin = [0, 1, 0, 0, 1, 0, 0];
    let checkbox = [0, 1, 0, 0, 2];
    let mut input = Interaction::default();
    let prepare = |ui: &DelegateTableDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(620.0, 338.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, layout) = prepare(&ui, &input);
    let bounds = layout.at_path(&star).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    let hit = input.pointer_release_target().unwrap();
    assert_eq!(hit.path, star);
    let selection = input
        .table_delegate_selection(&scene, &layout, &hit, RowSelection::Replace)
        .unwrap();
    assert_eq!(
        selection.action,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace,
        })
    );
    assert!(ui.control_at(&scene, &selection.path, selection.action));
    assert_eq!(ui.get_selected_row(), 0);
    assert!(ui.dispatch_at(&scene, &star, "clicked"));
    assert!(ui.edit_at(&scene, &text, "000!".into()));
    // A reused scene still resolves the same live row and supplies its latest value.
    assert!(ui.dispatch_at(&scene, &text, "accepted"));
    assert_eq!(ui.get_committed_name(), "000!");
    assert!(ui.control_at(&scene, &spin, ControlAction::Spin(SpinAction::Increase)));
    assert_eq!(ui.get_committed_units(), 1);
    assert!(ui.dispatch_at(&scene, &checkbox, "clicked"));
    assert!(!model.row(0).unwrap().available);

    let (selected, layout) = prepare(&ui, &input);
    assert_eq!(
        ui.copy_table_at(&selected, &table, &[0, 1, 2]).as_deref(),
        Some("000!")
    );
    assert_eq!(selected.at_path(&star).unwrap().text("text"), Some("★ 1"));
    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Vertical, 4_000.0));
    let (away, layout) = prepare(&ui, &input);
    assert!(away.at_path(&table).unwrap().list.unwrap().first_row > 0);
    assert!(away.at_path(&table).unwrap().children.len() < 15);

    assert!(model.sort_by(|a, b| b.name.cmp(&a.name)));
    assert!(!ui.dispatch_at(&selected, &star, "clicked"));
    assert!(input.accessibility_scroll_to(&layout, &table, Orientation::Vertical, 100_000.0));
    let (moved, _) = prepare(&ui, &input);
    assert_eq!(model.row_key(199), Some(key));
    let slot = 199 - moved.at_path(&table).unwrap().list.unwrap().first_row;
    assert_eq!(
        moved.at_path(&[0, 1, slot, 0, 0, 1]).unwrap().text("text"),
        Some("★ 1")
    );
    assert!(ui.dispatch_at(&moved, &[0, 1, slot, 0, 0, 0], "accepted"));
    assert_eq!(ui.get_committed_row(), 199);
    assert_eq!(ui.get_committed_name(), "000!");
}

#[test]
fn nested_table_editors_and_paste_history_update_only_changed_fields() {
    let mut ui = NestedRecordsDemo::new();
    ui.on_decode_quantity(|text| {
        let value = text.strip_suffix('件').and_then(|text| text.parse().ok());
        QuantityPaste {
            accepted: value.is_some(),
            value: value.unwrap_or_default(),
        }
    });
    let table = [0, 2];
    let model = ui.model_records();
    let first = model.row_key(0).unwrap();
    let scene = prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(840.0, 580.0),
        &mut FixedText,
        &Default::default(),
    )
    .0;
    let mut latest = model.row(0).unwrap();
    latest.stock.note = "updated separately".into();
    assert!(model.set_row(0, latest));
    assert!(ui.control_at(
        &scene,
        &[0, 2, 0, 0, 0, 0],
        ControlAction::Spin(SpinAction::Increase)
    ));
    assert!(ui.dispatch_at(&scene, &[0, 2, 0, 0, 1], "clicked"));
    let row = model.row(0).unwrap();
    assert_eq!(
        (
            row.stock.units,
            row.stock.available,
            row.stock.note.as_str()
        ),
        (13, false, "updated separately")
    );

    let paste = ui
        .prepare_table_paste(&ui.render_shared(), &table, 0, vec![0, 1, 2, 3])
        .unwrap();
    assert!(paste("29件\ttrue\tstock note\treserve note"));
    assert!(model.sort_by(|left, right| right.location.key.cmp(&left.location.key)));
    let index = model.row_index(first).unwrap();
    let mut row = model.row(index).unwrap();
    assert_eq!((row.stock.units, row.stock.available), (29, true));
    assert_eq!(row.stock.note, "stock note");
    assert_eq!(row.reserve.note, "reserve note");
    row.reserve.units = 44;
    assert!(model.set_row(index, row));

    assert!(ui.replay_table_paste_at(&ui.render_shared(), &table, false));
    let row = model.row(index).unwrap();
    assert_eq!(
        (
            row.stock.units,
            row.stock.available,
            row.stock.note.as_str()
        ),
        (13, false, "updated separately")
    );
    assert_eq!((row.reserve.units, row.reserve.note.as_str()), (44, "备用"));
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, false));
    assert!(model.row(index).unwrap().stock.available);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, false));
    assert_eq!(model.row(index).unwrap().stock.units, 12);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, true));
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, true));
    assert!(ui.replay_table_paste_at(&ui.render_shared(), &table, true));
    let row = model.row(index).unwrap();
    assert_eq!(
        (
            row.stock.units,
            row.stock.available,
            row.stock.note.as_str()
        ),
        (29, true, "stock note")
    );
    assert_eq!(
        (row.reserve.units, row.reserve.note.as_str()),
        (44, "reserve note")
    );
}

#[test]
fn nested_roles_project_shared_model_values_and_tree_edit_updates_other_views() {
    let mut ui = NestedRecordsDemo::new();
    let list = [0, 1, 0];
    let tree = [0, 1, 1];
    let table = [0, 3];
    let model = ui.model_records();
    let prepare = |ui: &NestedRecordsDemo| {
        prepare_scene(
            ui,
            ui.render_shared(),
            Size::new(840.0, 580.0),
            &mut FixedText,
            &Default::default(),
        )
        .0
    };
    let scene = prepare(&ui);
    assert_eq!(
        xrack_runtime::combo_text(scene.at_path(&[0, 0]).unwrap()),
        "仓库 A"
    );
    assert_eq!(
        scene
            .at_path(&list)
            .unwrap()
            .row_heights
            .as_ref()
            .unwrap()
            .height(1),
        42.0
    );
    assert!(ui.control_at(
        &scene,
        &tree,
        ControlAction::Tree(TreeAction::SetExpanded {
            key: 1,
            row: model.row_key(0).unwrap(),
            expanded: true,
        })
    ));
    let scene = prepare(&ui);
    let child = [0, 1, 1, 1, 0, 0];
    assert_eq!(scene.at_path(&child).unwrap().text("text"), Some("货位 B"));
    assert_eq!(
        scene
            .at_path(&tree)
            .unwrap()
            .row_heights
            .as_ref()
            .unwrap()
            .height(1),
        42.0
    );
    assert!(ui.commit_table_cell(&scene, &child, "货位 B", "Moved shelf".into()));
    assert!(ui.control_at(&scene, &table, ControlAction::Select(1)));
    ui.set_selected_location(2);
    let scene = prepare(&ui);
    assert_eq!(
        xrack_runtime::combo_text(scene.at_path(&[0, 0]).unwrap()),
        "Moved shelf"
    );
    assert_eq!(
        ui.list_rows(&list, 1..2)[0].text("text"),
        Some("Moved shelf")
    );
    assert_eq!(
        ui.list_rows(&table, 1..2)[0].children[1].text("text"),
        Some("5")
    );
    assert_eq!(
        ui.copy_table_at(&scene, &table, &[0, 1, 2]).as_deref(),
        Some("Moved shelf\t5\t待检")
    );
    assert_eq!(model.row(1).unwrap().location.key, 2);
}

#[test]
fn tree_component_editors_resolve_the_model_index_below_an_expanded_branch() {
    let mut ui = DelegateTreeDemo::new();
    let model = ui.model_records();
    let root = model.row_key(1).unwrap();
    let mut input = Interaction::default();
    let path = [0];
    let prepare = |ui: &DelegateTreeDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(360.0, 250.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::Tree(TreeAction::SetExpanded {
            key: 1,
            row: root,
            expanded: true,
        })
    ));
    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let child = [0, 1, 0, 0, 0, 0];
    assert_eq!(scene.at_path(&child).unwrap().text("text"), Some("子记录"));
    assert!(ui.edit_at(&scene, &child, "已编辑".into()));
    assert!(ui.dispatch_at(&scene, &child, "accepted"));
    assert_eq!(ui.get_committed_index(), 0);
    assert_eq!(model.row(0).unwrap().label, "已编辑");
    assert_eq!(model.row(1).unwrap().label, "目录");
}

#[test]
fn table_drag_selection_crosses_frozen_panes_and_virtual_rows() {
    table_selection_across_panes(false);
}

#[test]
fn table_touch_selection_crosses_frozen_panes_and_virtual_rows() {
    table_selection_across_panes(true);
}

fn table_selection_across_panes(touch: bool) {
    let mut ui = StandardTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_pinned(1);
    ui.set_inventory(
        (0..1_000)
            .map(|units| InventoryRow {
                units,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_move_visible_column(3, 0);
    let path = [0, 1];
    let viewport = Size::new(470.0, 330.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (mut scene, mut layout) = prepare(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert!(table.is_frozen(3));
    let body_top = table.body.y;
    let start = Point {
        x: table.columns[3].x + 24.0,
        y: table.body.y + 16.0,
    };
    let edge = Point {
        x: table.body.x + table.body.width - 16.0,
        y: table.body.y + table.body.height - 6.0,
    };
    let now = Instant::now();
    let drag_started = if touch {
        assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
        let hold = input.table_touch_hold_deadline().unwrap();
        assert_eq!(hold.duration_since(now), Duration::from_millis(500));
        assert!(!input.advance_table_touch_hold(&scene, &layout, hold - Duration::from_millis(1)));
        assert!(input.advance_table_touch_hold(&scene, &layout, hold));
        assert!(input.move_scroll_gesture(&scene, &layout, edge, hold));
        hold
    } else {
        input.pointer_move(&scene, &layout, start);
        input.pointer_press_in_scene(&scene, &layout);
        assert!(input.arm_table_drag_selection(&scene, &layout, false));
        input.pointer_move(&scene, &layout, edge);
        assert!(input.move_table_drag_selection(&layout, now));
        now
    };
    assert!(
        input
            .table_drag_selection_deadline(&scene, &layout)
            .is_some()
    );
    let mut advanced_x = false;
    let mut frame_time = drag_started;
    for frame in 1..20 {
        frame_time = drag_started + Duration::from_millis(frame * 20);
        if input.advance_table_drag_selection(&scene, &layout, frame_time) {
            (scene, layout) = prepare(&ui, &input);
            advanced_x |= layout
                .at_path(&path)
                .unwrap()
                .table
                .as_ref()
                .unwrap()
                .horizontal
                .offset
                > 0.0;
        }
    }
    let node = scene.at_path(&path).unwrap();
    assert!(advanced_x);
    assert!(node.list.unwrap().first_row > 0);
    assert!(node.table_selection.as_ref().unwrap().is_empty());
    let previews = input.table_drag_preview(&path, node, layout.at_path(&path).unwrap());
    assert_eq!(previews.len(), 2);
    assert!(previews.iter().all(|rect| rect.y >= body_top));
    let released = if touch {
        let outcome = input.end_scroll_gesture(&scene, &layout, edge, frame_time);
        assert!(!outcome.tap && !outcome.scrolled);
        assert!(!input.touch_table_selection_claimed());
        outcome.control.unwrap()
    } else {
        input.finish_table_drag_selection(&scene, &layout).unwrap()
    };
    let ControlAction::TableSelection(TableSelectionAction::DragRange {
        anchor,
        end,
        additive: false,
    }) = released.action
    else {
        panic!("expected a table range");
    };
    assert_eq!(anchor, (0, 3));
    assert!(end.0 >= node.list.unwrap().first_row && end.1 == 2);
    assert!(ui.control_at(&scene, &released.path, released.action));
    let (scene, _) = prepare(&ui, &input);
    let selection = scene
        .at_path(&path)
        .unwrap()
        .table_selection
        .as_ref()
        .unwrap();
    assert_eq!(selection.len(), (end.0 + 1) * 4);
    assert_eq!(ui.get_current(), end.0 as i64);
    assert_eq!(ui.get_selections(), 1);
}

#[test]
fn table_touch_hold_distinguishes_tap_scroll_and_cancelled_selection() {
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records((0..100).rev().map(|row| format!("{row:03}")).collect());
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let prepare = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let start = Point {
        x: table.columns[0].x + 20.0,
        y: item.children[2].bounds.y + 16.0,
    };
    let now = Instant::now();
    let short = now + Duration::from_millis(80);
    let hold = now + Duration::from_millis(500);
    let begin = |input: &mut Interaction| {
        assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    };
    begin(&mut input);
    let outcome = input.end_scroll_gesture(&scene, &layout, start, short);
    assert!(outcome.tap);
    assert!(outcome.control.is_none());

    begin(&mut input);
    let up = Point {
        y: start.y - 55.0,
        ..start
    };
    assert!(input.move_scroll_gesture(&scene, &layout, up, short));
    assert!(input.table_touch_hold_deadline().is_none());
    assert!(!input.advance_table_touch_hold(&scene, &layout, hold));
    let outcome = input.end_scroll_gesture(&scene, &layout, up, hold);
    assert!(!outcome.tap && outcome.control.is_none());
    let (scrolled, _) = prepare(&ui, &input);
    assert!(scrolled.at_path(&path).unwrap().list.unwrap().first_row > 0);

    input = Interaction::default();
    begin(&mut input);
    assert!(input.advance_table_touch_hold(&scene, &layout, hold));
    assert!(input.cancel_table_drag_selection());
    let outcome = input.end_scroll_gesture(&scene, &layout, start, hold);
    assert!(!outcome.tap && outcome.control.is_none());

    begin(&mut input);
    assert!(input.advance_table_touch_hold(&scene, &layout, hold));
    ui.model_records().sort_by(|left, right| left.cmp(right));
    let (scene, layout) = prepare(&ui, &input);
    let node = scene.at_path(&path).unwrap();
    assert!(
        input
            .table_drag_preview(&path, node, layout.at_path(&path).unwrap())
            .is_empty()
    );
    let outcome = input.end_scroll_gesture(&scene, &layout, start, hold);
    assert!(!outcome.tap && outcome.control.is_none());
    assert_eq!(ui.get_selected_count(), 0);
}

#[test]
fn table_selection_handles_resize_the_range_with_touch_and_mouse() {
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(["d", "c", "b", "a"].map(str::to_owned).to_vec());
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let prepare = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let cell = |layout: &xrack_layout::LayoutNode, row: usize, column: usize| {
        let item = layout.at_path(&path).unwrap();
        let column = item.table.as_ref().unwrap().columns[column];
        Point {
            x: column.x + column.width * 0.5,
            y: item.children[row].bounds.y + 16.0,
        }
    };
    let handles =
        |input: &Interaction, scene: &xrack_runtime::Node, layout: &xrack_layout::LayoutNode| {
            input
                .table_selection_handles(
                    &path,
                    scene.at_path(&path).unwrap(),
                    layout.at_path(&path).unwrap(),
                )
                .map(|rect| Point {
                    x: rect.x + rect.width * 0.5,
                    y: rect.y + rect.height * 0.5,
                })
                .collect::<Vec<_>>()
        };
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    let start = cell(&layout, 2, 0);
    let end = cell(&layout, 0, 1);
    let now = Instant::now();
    let hold = now + Duration::from_millis(500);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    assert!(input.advance_table_touch_hold(&scene, &layout, hold));
    assert!(input.move_scroll_gesture(&scene, &layout, end, hold));
    assert!(handles(&input, &scene, &layout).is_empty());
    let action = input
        .end_scroll_gesture(&scene, &layout, end, hold)
        .control
        .unwrap();
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_selected_count(), 6);

    let (scene, layout) = prepare(&ui, &input);
    let [top_left, _] = handles(&input, &scene, &layout).try_into().unwrap();
    assert!(input.begin_scroll_gesture(&scene, &layout, top_left, hold, ScrollGestureKind::Touch));
    assert!(input.touch_table_selection_claimed());
    let outcome = input.end_scroll_gesture(&scene, &layout, top_left, hold);
    assert!(!outcome.tap && outcome.control.is_none());
    assert!(input.begin_scroll_gesture(&scene, &layout, top_left, hold, ScrollGestureKind::Touch));
    assert!(input.touch_table_selection_claimed());
    let end = cell(&layout, 1, 1);
    assert!(input.move_scroll_gesture(&scene, &layout, end, hold));
    assert_eq!(ui.get_selected_count(), 6);
    let action = input
        .end_scroll_gesture(&scene, &layout, end, hold)
        .control
        .unwrap();
    assert_eq!(
        action.action,
        ControlAction::TableSelection(TableSelectionAction::DragRange {
            anchor: (2, 1),
            end: (1, 1),
            additive: false,
        })
    );
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_selected_count(), 2);

    let (scene, layout) = prepare(&ui, &input);
    let [_, bottom_right] = handles(&input, &scene, &layout).try_into().unwrap();
    input.pointer_move(&scene, &layout, bottom_right);
    assert!(input.press_table_selection_handle(&scene, &layout));
    input.pointer_move(&scene, &layout, cell(&layout, 3, 1));
    assert!(input.move_table_drag_selection(&layout, hold));
    let action = input.finish_table_drag_selection(&scene, &layout).unwrap();
    assert_eq!(
        action.action,
        ControlAction::TableSelection(TableSelectionAction::DragRange {
            anchor: (1, 1),
            end: (3, 1),
            additive: false,
        })
    );
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_selected_count(), 3);

    let (scene, layout) = prepare(&ui, &input);
    let [top_left, _] = handles(&input, &scene, &layout).try_into().unwrap();
    input.pointer_move(&scene, &layout, top_left);
    assert!(input.press_table_selection_handle(&scene, &layout));
    ui.model_records().sort_by(|left, right| left.cmp(right));
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, cell(&layout, 0, 0));
    assert!(input.finish_table_drag_selection(&scene, &layout).is_none());
    assert!(handles(&input, &scene, &layout).is_empty());
    assert_eq!(ui.get_selected_count(), 3);
}

#[test]
fn table_drag_selection_cancels_cleanly_and_rejects_a_reordered_model() {
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(["c", "b", "a"].map(str::to_owned).to_vec());
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let prepare = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let initial = ui.render_shared();
    assert!(ui.control_at(
        &initial,
        &path,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 1,
            operation: RowSelection::Replace
        })
    ));
    let (scene, layout) = prepare(&ui, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let start = Point {
        x: table.columns[0].x + 20.0,
        y: item.children[1].bounds.y + 16.0,
    };
    let end = Point {
        x: start.x,
        y: item.children[2].bounds.y + 16.0,
    };
    let begin = |input: &mut Interaction| {
        input.pointer_move(&scene, &layout, start);
        input.pointer_press_in_scene(&scene, &layout);
        assert!(input.arm_table_drag_selection(&scene, &layout, true));
        input.pointer_move(&scene, &layout, end);
        assert!(input.move_table_drag_selection(&layout, Instant::now()));
        assert!(input.is_focused(&path));
    };
    begin(&mut input);
    assert!(input.cancel_table_drag_selection());
    assert!(input.pointer_release_target().is_none());
    assert_eq!(ui.get_selected_count(), 1);

    begin(&mut input);
    let released = input.finish_table_drag_selection(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &released.path, released.action));
    assert_eq!(ui.get_selected_count(), 3);

    begin(&mut input);
    ui.model_records().sort_by(|a, b| a.cmp(b));
    let (scene, layout) = prepare(&ui, &input);
    let node = scene.at_path(&path).unwrap();
    assert!(
        input
            .table_drag_preview(&path, node, layout.at_path(&path).unwrap())
            .is_empty()
    );
    assert!(input.finish_table_drag_selection(&scene, &layout).is_none());
    assert_eq!(ui.get_selected_count(), 3);
}

#[test]
fn table_copy_reads_offscreen_rows_and_uses_visual_column_order() {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..1_000)
            .map(|index| InventoryRow {
                name: format!("Device {index}"),
                units: index,
                available: index % 2 == 0,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_move_visible_column(3, 0);
    let path = [0, 1];
    let viewport = Size::new(620.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    let first = scene.at_path(&path).unwrap().children[0].row_key;
    for (index, operation) in [(0, RowSelection::Replace), (999, RowSelection::Toggle)] {
        let (scene, _) = prepare(&ui, &input);
        assert!(ui.control_at(
            &scene,
            &path,
            ControlAction::TableSelection(TableSelectionAction::Row { index, operation },)
        ));
    }
    let (scene, layout) = prepare(&ui, &input);
    assert!(
        !scene
            .at_path(&path)
            .unwrap()
            .children
            .iter()
            .any(|row| row.row_key == first)
    );
    input.focus_next(&scene, &layout, false);
    let (target, columns) = input.table_copy_target(&scene, &layout).unwrap();
    assert_eq!(target, path);
    assert_eq!(columns, [3, 0, 1, 2]);
    assert_eq!(
        ui.copy_table_at(&scene, &target, &columns).as_deref(),
        Some("00:00:00\tDevice 0\t0\ttrue\n00:00:00\tDevice 999\t999\tfalse")
    );
}

#[test]
fn copied_table_cells_keep_holes_and_quote_embedded_delimiters() {
    let mut ui = StandardTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(vec![
        InventoryRow {
            name: "Alpha\tBeta\n\"quoted\"".into(),
            ..InventoryRow::default()
        },
        InventoryRow::default(),
        InventoryRow {
            available: true,
            ..InventoryRow::default()
        },
    ]);
    ui.invoke_move_visible_column(2, 0);
    ui.invoke_move_visible_column(2, 1);
    let path = [0, 1];
    let viewport = Size::new(620.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    for (row, column, operation) in [(0, 0, RowSelection::Replace), (2, 2, RowSelection::Toggle)] {
        let (scene, _) = prepare(&ui, &input);
        assert!(ui.control_at(
            &scene,
            &path,
            ControlAction::TableSelection(TableSelectionAction::Cell {
                row,
                column,
                operation
            },)
        ));
    }
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let (target, columns) = input.table_copy_target(&scene, &layout).unwrap();
    assert_eq!(columns, [2, 1, 0, 3]);
    assert_eq!(
        ui.copy_table_at(&scene, &target, &columns).as_deref(),
        Some("\t\t\"Alpha\tBeta\n\"\"quoted\"\"\"\n\t\t\ntrue\t\t")
    );
}

#[test]
fn custom_table_copies_project_values_but_do_not_override_text_editing() {
    let mut ui = TableDemo::new();
    ui.set_records(vec!["Edited record".into()]);
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let prepare = |ui: &TableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(&scene, &path, ControlAction::Select(0)));
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let (target, columns) = input.table_copy_target(&scene, &layout).unwrap();
    assert_eq!(
        ui.copy_table_at(&scene, &target, &columns).as_deref(),
        Some("Edited record\t#1")
    );
    ui.set_show_sequence(false);
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(
        ui.copy_table_at(&scene, &target, &columns).as_deref(),
        Some("Edited record\t")
    );
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.table_copy_target(&scene, &layout).is_none());
}

#[test]
fn table_paste_history_replays_sparse_rows_after_sort_and_preserves_other_fields() {
    let mut ui = DelegateTableDemo::new();
    ui.set_records(vec![
        StockRow {
            name: "A".into(),
            units: 10,
            ..StockRow::default()
        },
        StockRow {
            name: "B".into(),
            units: 20,
            available: true,
            ..StockRow::default()
        },
        StockRow {
            name: "Z".into(),
            ..StockRow::default()
        },
    ]);
    ui.on_decode_quantity(|text, _, _| {
        let value = text.parse::<i64>().ok();
        QuantityPaste {
            accepted: value.is_some(),
            value: value.unwrap_or_default(),
        }
    });
    let table = [0, 1];
    let model = ui.model_records();
    let first = model.row_key(0).unwrap();
    let second = model.row_key(1).unwrap();
    let scene = ui.render_shared();
    let paste = ui
        .prepare_table_paste(&scene, &table, 0, vec![0, 1, 2])
        .unwrap();
    assert!(paste("Y\t11\ttrue\nC\t21\tfalse"));
    assert!(model.sort_by(|a, b| a.name.cmp(&b.name)));
    assert!(model.insert(
        1,
        StockRow {
            name: "Inserted".into(),
            ..StockRow::default()
        }
    ));
    let first_index = model.row_index(first).unwrap();
    let second_index = model.row_index(second).unwrap();
    assert_eq!((first_index, second_index), (2, 0));
    let mut record = model.row(first_index).unwrap();
    record.owner = "Later edit".into();
    assert!(model.set_row(first_index, record));
    ui.invoke_move_visible_column(0, 2);
    ui.set_lock_names(true);

    let scene = ui.render_shared();
    assert!(ui.replay_table_paste_at(&scene, &table, false));
    let first_row = model.row(first_index).unwrap();
    let second_row = model.row(second_index).unwrap();
    assert_eq!(
        (
            first_row.name.as_str(),
            first_row.units,
            first_row.available
        ),
        ("A", 10, false)
    );
    assert_eq!(
        (
            second_row.name.as_str(),
            second_row.units,
            second_row.available
        ),
        ("B", 20, true)
    );
    assert_eq!(first_row.owner, "Later edit");
    assert_eq!(model.row(1).unwrap().name, "Inserted");
    assert_eq!(model.row_key(first_index), Some(first));
    assert_eq!(model.row_key(second_index), Some(second));
    assert!(ui.replay_table_paste_at(&ui.render_shared(), &table, true));
    let first_row = model.row(first_index).unwrap();
    let second_row = model.row(second_index).unwrap();
    assert_eq!(
        (
            first_row.name.as_str(),
            first_row.units,
            first_row.available
        ),
        ("Y", 11, true)
    );
    assert_eq!(
        (
            second_row.name.as_str(),
            second_row.units,
            second_row.available
        ),
        ("C", 21, false)
    );
    assert_eq!(first_row.owner, "Later edit");
}

#[test]
fn table_paste_history_rejects_a_conflicting_batch_without_partially_restoring_it() {
    let mut ui = DelegateTableDemo::new();
    let table = [0, 1];
    let model = ui.model_records();
    let paste = |ui: &DelegateTableDemo, text| {
        ui.prepare_table_paste(&ui.render_shared(), &table, 0, vec![0])
            .unwrap()(text)
    };
    assert!(paste(&ui, "First\nSecond"));
    let mut row = model.row(1).unwrap();
    row.name = "Conflict".into();
    assert!(model.set_row(1, row));
    assert!(!ui.replay_table_paste_at(&ui.render_shared(), &table, false));
    assert_eq!(model.row(0).unwrap().name, "First");
    assert_eq!(model.row(1).unwrap().name, "Conflict");

    assert!(paste(&ui, "Again\nRemoved"));
    assert!(model.remove(1).is_some());
    assert!(!ui.replay_table_paste_at(&ui.render_shared(), &table, false));
    assert_eq!(model.row(0).unwrap().name, "Again");

    assert!(paste(&ui, "Before replacement"));
    ui.set_records(model.snapshot());
    assert!(!ui.replay_table_paste_at(&ui.render_shared(), &table, false));
    assert_eq!(
        ui.model_records().row(0).unwrap().name,
        "Before replacement"
    );
}

#[test]
fn custom_clipboard_paste_validates_mixed_cells_and_offscreen_rows_before_writing() {
    let mut ui = DelegateTableDemo::new();
    ui.set_records(
        (0..64)
            .map(|index| StockRow {
                name: format!("Old {index}"),
                units: index * 2,
                ..StockRow::default()
            })
            .collect(),
    );
    ui.on_decode_quantity(|text, units, index| {
        assert_eq!(units, index * 2);
        let decoded = text
            .strip_suffix('件')
            .and_then(|number| number.trim().parse::<i64>().ok());
        QuantityPaste {
            accepted: decoded.is_some(),
            value: decoded.unwrap_or_default(),
        }
    });
    let model = ui.model_records();
    let table = [0, 1];
    ui.invoke_move_visible_column(2, 0);
    let mut input = Interaction::default();
    let prepare = |ui: &DelegateTableDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(620.0, 338.0),
            &mut FixedText,
            input,
        )
    };
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &table,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 2,
            operation: RowSelection::Replace,
        })
    ));
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let target = input.table_paste_target(&scene, &layout).unwrap();
    assert_eq!(target.path, table);
    assert_eq!(target.columns, [2, 0, 1]);
    assert!(scene.at_path(&table).unwrap().children.len() < 20);
    let original = model.snapshot();
    let stamp = model.edit_stamp();
    let keys: Vec<_> = (0..64).map(|index| model.row_key(index)).collect();
    let prepare_paste = || {
        ui.prepare_table_paste(&scene, &table, target.row, target.columns.clone())
            .unwrap()
    };
    let payload = |invalid: bool| {
        (0..40)
            .map(|index| {
                let quantity = if invalid && index == 30 {
                    "invalid".into()
                } else {
                    format!("{}件", 100 + index)
                };
                format!("TRUE\t\"New\t{index}\"\t{quantity}")
            })
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    assert!(!prepare_paste()(&payload(true)));
    assert_eq!(model.snapshot(), original);
    assert_eq!(model.edit_stamp(), stamp);
    assert!(prepare_paste()(&payload(false)));
    for (index, key) in keys.into_iter().enumerate() {
        let record = model.row(index).unwrap();
        assert_eq!(model.row_key(index), key);
        if index < 40 {
            assert_eq!(record.name, format!("New\t{index}"));
            assert_eq!(record.units, 100 + index as i64);
            assert!(record.available);
        } else {
            assert_eq!(record, original[index]);
        }
    }
    assert_eq!(ui.get_committed_row(), -1);
}

#[test]
fn custom_clipboard_paste_honors_live_policy_visibility_and_model_revision() {
    let mut ui = DelegateTableDemo::new();
    ui.on_decode_quantity(|_, _, _| QuantityPaste {
        accepted: true,
        value: 55,
    });
    let model = ui.model_records();
    let path = [0, 1];
    let prepare = |ui: &DelegateTableDemo, columns| {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, columns)
            .unwrap()
    };
    let original = model.snapshot();
    let pending = prepare(&ui, vec![0]);
    ui.set_lock_names(true);
    assert!(!pending("Locked"));
    ui.set_lock_names(false);
    assert!(!prepare(&ui, vec![0])("\"\""));
    ui.set_show_quantity(false);
    assert!(!prepare(&ui, vec![0, 1])("Valid name\t55件"));
    assert_eq!(model.snapshot(), original);
    let pending = prepare(&ui, vec![0]);
    let mut changed = model.row(1).unwrap();
    changed.owner = "External update".into();
    assert!(model.set_row(1, changed));
    assert!(!pending("Stale clipboard result"));
    assert_eq!(model.row(0).unwrap(), original[0]);
}

#[test]
fn table_paste_updates_typed_fields_in_visual_order_including_offscreen_rows() {
    let mut ui = RecordTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(vec![
        StockRow {
            name: "Keep".into(),
            ..StockRow::default()
        };
        24
    ]);
    ui.invoke_move_visible_column(0, 4);
    let path = [0, 1];
    let viewport = Size::new(760.0, 330.0);
    let prepare = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 1,
            operation: RowSelection::Replace,
        }),
    ));
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    let target = input.table_paste_target(&scene, &layout).unwrap();
    assert_eq!(target.path, path);
    assert_eq!(target.row, 0);
    assert_eq!(target.columns, [1, 2, 3, 4, 0]);
    assert!(scene.at_path(&path).unwrap().children.len() < 20);
    let model = ui.model_records();
    let keys: Vec<_> = (0..24).map(|index| model.row_key(index)).collect();
    let paste = ui
        .prepare_table_paste(&scene, &target.path, target.row, target.columns)
        .unwrap();
    let payload = (0..20)
        .map(|row| format!("\"Owner\t{row}\"\t{row}\tTRUE\t0.75"))
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(paste(&payload));
    for (index, key) in keys.into_iter().enumerate() {
        assert_eq!(model.row_key(index), key);
        let row = model.row(index).unwrap();
        assert_eq!(row.name, "Keep");
        if index < 20 {
            assert_eq!(row.owner, format!("Owner\t{index}"));
            assert_eq!(row.units, index as i64);
            assert!(row.available);
            assert_eq!(row.load, 0.75);
        } else {
            assert_eq!(row.owner, "");
        }
    }
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.table_paste_target(&scene, &layout).is_none());
}

#[test]
fn table_paste_rejects_the_entire_block_and_expires_on_model_changes() {
    let mut ui = RecordTableDemo::new();
    let model = ui.model_records();
    let path = [0, 1];
    let paste = |ui: &RecordTableDemo, row, columns| {
        ui.prepare_table_paste(&ui.render_shared(), &path, row, columns)
            .unwrap()
    };
    let original = model.snapshot();
    let stamp = model.edit_stamp();
    for text in [
        "valid\t15\ninvalid\tNaN",
        "valid\t15\ninvalid\t1000",
        "valid\t15\nragged",
    ] {
        assert!(!paste(&ui, 0, vec![0, 2])(text));
    }
    assert!(!paste(&ui, 2, vec![0])("fits\noutside"));
    assert_eq!(model.snapshot(), original);
    assert_eq!(model.edit_stamp(), stamp);

    let pending = paste(&ui, 0, vec![0, 1]);
    ui.set_lock_owner(true);
    assert!(!pending("Changed name\tRead-only owner"));
    assert_eq!(model.snapshot(), original);

    let pending = paste(&ui, 0, vec![0]);
    let mut row = model.row(1).unwrap();
    row.units += 1;
    assert!(model.set_row(1, row));
    assert!(!pending("Stale paste"));
    assert_eq!(model.row(0).unwrap(), original[0]);

    let pending = paste(&ui, 0, vec![0]);
    ui.set_records(original.clone());
    assert!(!pending("Old model"));
    assert_eq!(ui.model_records().snapshot(), original);
}

#[test]
fn standard_table_pastes_in_visual_order_and_replays_by_record_key() {
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_paste_cells(true);
    ui.set_auto_sort_fields(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_pinned(1);
    ui.set_inventory(
        (0..24)
            .map(|index| InventoryRow {
                name: format!("Device {index:02}"),
                units: 24 - index,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_sort_rows(1, false);
    ui.invoke_move_visible_column(0, 3);
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 1,
            column: 1,
            operation: RowSelection::Replace,
        }
    ));
    let model = ui.model_inventory();
    let keys: Vec<_> = (3..=22)
        .rev()
        .map(|index| model.row_key(index).unwrap())
        .collect();
    let mut input = Interaction::default();
    let (scene, layout) = prepare_scene_with_input(
        &ui,
        ui.render_shared(),
        Size::new(620.0, 380.0),
        &mut FixedText,
        &input,
    );
    input.focus_next(&scene, &layout, false);
    let target = input.table_paste_target(&scene, &layout).unwrap();
    assert_eq!(target.row, 1);
    assert_eq!(target.columns, [1, 2, 3, 0]);
    let table = scene.at_path(&target.path).unwrap();
    assert!(table.children.len() < keys.len());
    let paste = ui
        .prepare_table_paste(&scene, &target.path, target.row, target.columns)
        .unwrap();
    let payload = (0..keys.len())
        .map(|row| {
            let available = if row % 2 == 0 { "TRUE" } else { "false" };
            format!("{}\t{available}\t09:10:{row:02}", 200 + row)
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(paste(&payload));
    let verify_paste = || {
        for (offset, key) in keys.iter().copied().enumerate() {
            let row = model.row(model.row_index(key).unwrap()).unwrap();
            assert_eq!(row.units, 200 + offset as i64);
            assert_eq!(row.available, offset % 2 == 0);
            assert_eq!(row.refreshed, Time::new(9, 10, offset as i64).unwrap());
        }
    };
    verify_paste();
    assert_eq!(model.row(23).unwrap().units, 1);
    assert_eq!(model.row(2).unwrap().units, 22);
    let index = model.row_index(keys[0]).unwrap();
    let mut updated = model.row(index).unwrap();
    updated.name = "External name".into();
    assert!(model.set_row(index, updated));
    assert!(model.sort_by(|a, b| b.name.cmp(&a.name)));
    ui.invoke_move_visible_column(1, 2);
    assert!(ui.replay_table_paste_at(&ui.render_shared(), &target.path, false));
    for (offset, key) in keys.iter().copied().enumerate() {
        let row = model.row(model.row_index(key).unwrap()).unwrap();
        assert_eq!(row.units, 2 + offset as i64);
        assert!(!row.available);
        assert_eq!(row.refreshed, Time::default());
    }
    assert_eq!(
        model.row(model.row_index(keys[0]).unwrap()).unwrap().name,
        "External name"
    );
    assert!(ui.replay_table_paste_at(&ui.render_shared(), &target.path, true));
    verify_paste();
    assert_eq!(
        model.row(model.row_index(keys[0]).unwrap()).unwrap().name,
        "External name"
    );
    let index = model.row_index(*keys.last().unwrap()).unwrap();
    let mut changed = model.row(index).unwrap();
    changed.refreshed = Time::new(12, 0, 0).unwrap();
    assert!(model.set_row(index, changed));
    let conflicted = model.snapshot();
    assert!(!ui.replay_table_paste_at(&ui.render_shared(), &target.path, false));
    assert_eq!(model.snapshot(), conflicted);
}

#[test]
fn standard_table_rejects_invalid_and_stale_pastes_without_partial_writes() {
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_paste_cells(true);
    ui.set_inventory(vec![InventoryRow::default(); 3]);
    let path = [0, 1];
    let model = ui.model_inventory();
    let before = model.snapshot();
    let stamp = model.edit_stamp();
    let paste = |ui: &StandardTableDemo| {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, vec![1, 3])
            .unwrap()
    };
    assert!(!paste(&ui)("16\t09:00:00\n17\t25:00:00"));
    assert!(!paste(&ui)("16\t09:00:00\nNaN\t09:00:00"));
    let pending = paste(&ui);
    ui.set_lock_refresh_time(true);
    assert!(!pending("16\t09:00:00"));
    ui.set_lock_refresh_time(false);
    let pending = paste(&ui);
    ui.set_paste_cells(false);
    assert!(!pending("16\t09:00:00"));
    ui.set_paste_cells(true);
    let pending = paste(&ui);
    ui.invoke_move_visible_column(0, 2);
    assert!(!pending("16\t09:00:00"));
    assert_eq!(model.snapshot(), before);
    assert_eq!(model.edit_stamp(), stamp);
    assert!(!ui.replay_table_paste_at(&ui.render_shared(), &path, false));
    let pending = paste(&ui);
    let mut changed = model.row(2).unwrap();
    changed.name = "External update".into();
    assert!(model.set_row(2, changed));
    assert!(!pending("16\t09:00:00"));
    assert_eq!(model.row(0), Some(before[0].clone()));
}

#[test]
fn standard_table_undo_and_redo_interleave_committed_cells_and_batches() {
    use xrack_runtime::{FocusRequests, ScrollRequests};
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(vec![
        InventoryRow {
            name: "A".into(),
            units: 10,
            ..InventoryRow::default()
        },
        InventoryRow {
            name: "B".into(),
            units: 20,
            ..InventoryRow::default()
        },
    ]);
    let model = ui.model_inventory();
    let first = model.row_key(0).unwrap();
    let second = model.row_key(1).unwrap();
    let table = [0, 1];
    let mut input = Interaction::default();
    let focus = FocusRequests::default();
    let scroll = ScrollRequests::default();
    let _focus = focus.enter();
    let _scroll = scroll.enter();
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(620.0, 380.0),
            &mut FixedText,
            input,
        )
    };

    ui.invoke_edit_inventory_cell(0, 1);
    let (mut scene, mut layout) = prepare(&ui, &input);
    if input.apply_scroll_requests(&scene, &layout, scroll.drain()) {
        (scene, layout) = prepare(&ui, &input);
    }
    input.apply_focus_requests(&scene, &layout, focus.drain());
    assert!(input.table_cell_editing(&[0, 1, 0, 0, 1]));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("11"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(input.table_edit_history_target(&scene, &layout).is_none());
    assert!(input.commit_table_edit(&mut ui, &scene));
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(
        input.table_edit_history_target(&scene, &layout),
        Some(table.as_slice())
    );
    assert!(ui.replay_table_edit_at(&scene, &table, false));
    assert_eq!(model.row(0).unwrap().units, 10);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, true));
    assert_eq!(model.row(0).unwrap().units, 11);

    ui.set_paste_cells(true);
    let paste = ui
        .prepare_table_paste(&ui.render_shared(), &table, 0, vec![1, 2])
        .unwrap();
    assert!(paste("12\ttrue\n22\tfalse"));
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.commit_table_cell(&scene, &[0, 1, 1, 0, 1], "22", "23".into()));
    let mut row = model.row(0).unwrap();
    row.name = "Updated outside the table".into();
    assert!(model.set_row(0, row));
    assert!(model.sort_by(|a, b| b.units.cmp(&a.units)));
    ui.set_paste_cells(false);

    let values = || {
        let a = model.row(model.row_index(first).unwrap()).unwrap();
        let b = model.row(model.row_index(second).unwrap()).unwrap();
        assert_eq!(a.name, "Updated outside the table");
        (a.units, a.available, b.units)
    };
    assert_eq!(values(), (12, true, 23));
    for expected in [(12, true, 22), (11, false, 20), (10, false, 20)] {
        assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, false));
        assert_eq!(values(), expected);
    }
    for expected in [(11, false, 20), (12, true, 22), (12, true, 23)] {
        assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, true));
        assert_eq!(values(), expected);
    }
}

#[test]
fn standard_table_filter_preserves_hidden_selection_height_and_manual_order() {
    let mut ui = StandardTableDemo::new();
    ui.set_filter_tools(true);
    ui.set_move_rows(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        ["Device A", "Server B", "Server C", "Device D"]
            .into_iter()
            .map(|name| InventoryRow {
                name: name.into(),
                ..InventoryRow::default()
            })
            .collect(),
    );
    let model = ui.model_inventory();
    let keys: Vec<_> = (0..4).map(|row| model.row_key(row).unwrap()).collect();
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace,
        }
    ));
    ui.invoke_set_height(0, xrack_runtime::Length(72.0));
    ui.invoke_move_visible_row(0, 2);
    let mut driver = UiDriver::new(ui, FixedText, Size::new(620.0, 380.0));
    let displayed = |driver: &UiDriver<StandardTableDemo, FixedText>| {
        driver
            .node("inventory-table")
            .children
            .iter()
            .map(|row| row.row_key.unwrap())
            .collect::<Vec<_>>()
    };
    let table = [0, 1];
    assert_eq!(displayed(&driver), [keys[1], keys[2], keys[0], keys[3]]);
    driver.click("filter-inventory");
    driver.fill_focused("  seRVer  ");
    assert_eq!(displayed(&driver), [keys[1], keys[2]]);
    assert_eq!(model.len(), 4);
    assert_eq!(driver.component().get_selected_items(), 1);
    assert!(
        driver
            .component()
            .table_selection("inventory-table")
            .unwrap()
            .contains_cell(keys[0], 0)
    );
    assert!(
        driver
            .component()
            .copy_table_at(driver.scene(), &table, &[0])
            .is_none()
    );
    assert!(!driver.node("inventory-table").boolean("movable-rows", true));
    driver.update(|ui| ui.invoke_move_visible_row(0, 1));
    assert_eq!(displayed(&driver), [keys[1], keys[2]]);

    driver.update(|ui| ui.set_filter_query(String::new()));
    assert_eq!(displayed(&driver), [keys[1], keys[2], keys[0], keys[3]]);
    assert_eq!(
        driver
            .node("inventory-table")
            .row_heights
            .as_ref()
            .unwrap()
            .height(2),
        72.0
    );
    assert_eq!(
        driver
            .component()
            .copy_table_at(driver.scene(), &table, &[0])
            .as_deref(),
        Some("Device A")
    );
    driver.update(|ui| ui.set_filter_query("server".into()));
    driver.update(|_| assert!(model.remove(0).is_some()));
    assert_eq!(driver.component().get_selected_items(), 0);
    driver.update(|ui| ui.set_filter_query(String::new()));
    assert_eq!(displayed(&driver), [keys[1], keys[2], keys[3]]);
    assert!(driver.node("inventory-table").row_heights.is_none());
}

#[test]
fn sorted_filtered_table_commits_and_pastes_using_source_record_keys() {
    let mut ui = StandardTableDemo::new();
    ui.set_auto_sort_fields(true);
    ui.set_edit_columns(true);
    ui.set_paste_cells(true);
    ui.set_inventory(
        [("Server A", 30), ("Other", 20), ("Server B", 10)]
            .into_iter()
            .map(|(name, units)| InventoryRow {
                name: name.into(),
                units,
                ..InventoryRow::default()
            })
            .collect(),
    );
    let model = ui.model_inventory();
    ui.invoke_sort_rows(1, false);
    ui.set_filter_query("server".into());
    let table = [0, 1];
    let paste = |ui: &StandardTableDemo, row| {
        ui.prepare_table_paste(&ui.render_shared(), &table, row, vec![1])
            .unwrap()
    };
    assert!(!paste(&ui, 1)("11\n31"));
    assert_eq!(
        model
            .snapshot()
            .iter()
            .map(|row| row.units)
            .collect::<Vec<_>>(),
        [30, 20, 10]
    );
    assert!(paste(&ui, 0)("11\n31"));
    assert_eq!(
        model
            .snapshot()
            .iter()
            .map(|row| row.units)
            .collect::<Vec<_>>(),
        [31, 20, 11]
    );
    let (scene, _) = prepare_scene_with_input(
        &ui,
        ui.render_shared(),
        Size::new(620.0, 380.0),
        &mut FixedText,
        &Interaction::default(),
    );
    let cell = [0, 1, 0, 0, 1];
    assert_eq!(scene.row_key_at(&cell), model.row_key(2));
    assert!(ui.commit_table_cell(&scene, &cell, "11", "99".into()));
    assert_eq!(model.row(2).unwrap().units, 99);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, false));
    assert_eq!(model.row(2).unwrap().units, 11);
    let pending = paste(&ui, 0);
    ui.set_filter_query("Other".into());
    assert!(!pending("100"));
    assert_eq!(model.row(1).unwrap().units, 20);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &table, false));
    assert_eq!(
        model
            .snapshot()
            .iter()
            .map(|row| row.units)
            .collect::<Vec<_>>(),
        [30, 20, 10]
    );
}

#[test]
fn table_tab_editing_continues_when_the_committed_record_leaves_the_filter() {
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        ["Server A", "Server B"]
            .into_iter()
            .map(|name| InventoryRow {
                name: name.into(),
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.set_filter_query("server".into());
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace,
        }
    ));
    let model = ui.model_inventory();
    let mut driver = UiDriver::new(ui, FixedText, Size::new(620.0, 380.0));
    driver.tab();
    assert!(driver.begin_table_edit());
    driver.fill_focused("Device A");
    driver.tab();
    assert_eq!(model.row(0).unwrap().name, "Device A");
    let target = driver.input().focused.as_ref().unwrap();
    assert_eq!(target.row_key, model.row_key(1));
    assert_eq!(target.path, [0, 1, 0, 0, 0]);
    assert!(driver.input().table_cell_editing(&target.path));
}

#[test]
fn table_history_toolbar_tracks_edits_conflicts_and_model_replacement() {
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_inventory(vec![InventoryRow {
        name: "Device".into(),
        units: 10,
        ..InventoryRow::default()
    }]);
    let mut driver = UiDriver::new(ui, FixedText, Size::new(620.0, 380.0));
    let model = driver.component().model_inventory();
    let buttons = |driver: &UiDriver<StandardTableDemo, FixedText>| {
        (
            driver.node("undo-edit").boolean("enabled", true),
            driver.node("redo-edit").boolean("enabled", true),
        )
    };
    assert_eq!(buttons(&driver), (false, false));
    let scene = driver.scene().clone();
    driver.update(|ui| assert!(ui.commit_table_cell(&scene, &[0, 1, 0, 0, 1], "10", "11".into())));
    assert_eq!(buttons(&driver), (true, false));
    driver.click("undo-edit");
    assert_eq!(model.row(0).unwrap().units, 10);
    assert_eq!(buttons(&driver), (false, true));
    driver.click("redo-edit");
    assert_eq!(model.row(0).unwrap().units, 11);
    assert_eq!(buttons(&driver), (true, false));

    driver.update(|_| {
        let mut row = model.row(0).unwrap();
        row.units = 25;
        assert!(model.set_row(0, row));
    });
    assert_eq!(buttons(&driver), (true, false));
    driver.click("undo-edit");
    assert_eq!(model.row(0).unwrap().units, 25);
    assert_eq!(buttons(&driver), (false, false));

    let scene = driver.scene().clone();
    driver.update(|ui| assert!(ui.commit_table_cell(&scene, &[0, 1, 0, 0, 1], "25", "30".into())));
    assert_eq!(buttons(&driver), (true, false));
    driver.update(|ui| ui.set_inventory(vec![InventoryRow::default()]));
    assert_eq!(buttons(&driver), (false, false));
}

#[test]
fn record_row_heights_keep_virtual_anchors_and_manual_table_overrides() {
    use table_ui::{LogRow, ProjectedRowsDemo};
    use xrack_runtime::Length;

    let mut ui = ProjectedRowsDemo::new();
    ui.set_entries(
        (0..50_000)
            .map(|index| LogRow {
                message: index.to_string(),
                severity: "info".into(),
                height: Length(if index == 30_000 { 64.0 } else { 0.0 }),
            })
            .collect(),
    );
    let model = ui.model_entries();
    let mut driver = UiDriver::new(ui, FixedText, Size::new(660.0, 310.0));
    driver.update(|ui| ui.invoke_center_row(30_000));
    let offset = driver.component().get_offset();
    let anchor = driver.node("table").children[0].row_key;
    assert!(driver.node("table").children.len() < 10);

    let previous = driver.component().render_shared();
    let mut unrelated = model.row(49_999).unwrap();
    unrelated.message = "Updated text".into();
    assert!(model.set_row(49_999, unrelated));
    assert!(std::rc::Rc::ptr_eq(
        &previous,
        &driver.component().render_shared()
    ));
    driver.update(|_| {
        let mut first = model.row(0).unwrap();
        first.height = Length(96.0);
        assert!(model.set_row(0, first));
    });
    assert_eq!(driver.component().get_offset(), Length(offset.0 - 64.0));
    assert_eq!(driver.node("table").children[0].row_key, anchor);
    assert_eq!(
        driver.node("list").row_heights.as_ref().unwrap().height(0),
        96.0
    );

    driver.update(|ui| ui.invoke_set_height(30_000, Length(80.0)));
    driver.update(|_| {
        let mut row = model.row(30_000).unwrap();
        row.height = Length(120.0);
        assert!(model.set_row(30_000, row));
    });
    assert_eq!(
        driver
            .node("table")
            .row_heights
            .as_ref()
            .unwrap()
            .height(30_000),
        80.0
    );
    assert_eq!(
        driver
            .node("list")
            .row_heights
            .as_ref()
            .unwrap()
            .height(30_000),
        120.0
    );
    driver.update(|ui| ui.invoke_set_height(30_000, Length(-1.0)));
    assert_eq!(
        driver
            .node("table")
            .row_heights
            .as_ref()
            .unwrap()
            .height(30_000),
        120.0
    );
}

#[test]
fn tree_row_height_fields_follow_visible_order_without_rebuilding_topology() {
    use tree_ui::SizedTreeDemo;
    use xrack_runtime::Length;

    let mut ui = SizedTreeDemo::new();
    let viewport = Size::new(320.0, 240.0);
    let input = Interaction::default();
    let prepare = |ui: &SizedTreeDemo| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, &input)
    };
    let (scene, layout) = prepare(&ui);
    let first = scene.children[0].tree.as_ref().unwrap().rows()[0];
    assert_eq!(layout.at_path(&[0, 1]).unwrap().bounds.height, 44.0);
    assert!(ui.control_at(
        &scene,
        &[0],
        ControlAction::Tree(TreeAction::SetExpanded {
            key: first.key,
            row: first.row,
            expanded: true,
        })
    ));
    let (scene, layout) = prepare(&ui);
    assert_eq!(layout.at_path(&[0, 1]).unwrap().bounds.height, 76.0);
    assert_eq!(layout.at_path(&[0, 2]).unwrap().bounds.height, 44.0);
    let tree = scene.children[0].tree.as_ref().unwrap().clone();
    let model = ui.model_records();
    let mut branch = model.row(1).unwrap();
    branch.height = Length(88.0);
    assert!(model.set_row(1, branch));
    let (scene, layout) = prepare(&ui);
    assert!(std::sync::Arc::ptr_eq(
        &tree,
        scene.children[0].tree.as_ref().unwrap()
    ));
    assert_eq!(layout.at_path(&[0, 1]).unwrap().bounds.height, 88.0);
    assert!(ui.control_at(
        &scene,
        &[0],
        ControlAction::Tree(TreeAction::SetExpanded {
            key: first.key,
            row: first.row,
            expanded: false,
        })
    ));
    let (_, layout) = prepare(&ui);
    assert_eq!(layout.at_path(&[0, 1]).unwrap().bounds.height, 44.0);
}

#[test]
fn row_resize_keeps_record_identity_through_an_insert_during_the_drag() {
    let mut ui = StandardTableDemo::new();
    ui.set_resize_rows(true);
    ui.set_pinned(1);
    ui.set_inventory(
        (0..80)
            .map(|units| InventoryRow {
                units,
                ..InventoryRow::default()
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
    let key = scene.at_path(&path).unwrap().children[0].row_key.unwrap();
    let row = layout.at_path(&[0, 1, 0]).unwrap().bounds;
    let start = Point {
        x: row.x + 55.0,
        y: row.y + row.height - 1.0,
    };
    input.pointer_move(&scene, &layout, start);
    assert!(input.table_row_resize_cursor());
    input.pointer_press_in_scene(&scene, &layout);
    let end = Point {
        x: start.x,
        y: start.y + 40.0,
    };
    input.pointer_move(&scene, &layout, end);
    let stale = input.table_row_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        stale.action,
        ControlAction::ResizeRow {
            row: 0,
            key,
            height: 72
        }
    );
    let model = ui.model_inventory();
    assert!(model.insert(0, InventoryRow::default()));
    assert!(!ui.control_at(&scene, &stale.path, stale.action));
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, end);
    let resized = input.table_row_resize_drag(&scene, &layout).unwrap();
    assert_eq!(
        resized.action,
        ControlAction::ResizeRow {
            row: 1,
            key,
            height: 72
        }
    );
    assert!(ui.control_at(&scene, &resized.path, resized.action));
    assert!(input.finish_table_resize());
    assert!(input.pointer_release_target().is_none());
    let (scene, layout) = prepare(&ui, &input);
    assert_eq!(scene.at_path(&path).unwrap().children[1].row_key, Some(key));
    assert_eq!(layout.at_path(&[0, 1, 0]).unwrap().bounds.height, 32.0);
    assert_eq!(layout.at_path(&[0, 1, 1]).unwrap().bounds.height, 72.0);
    assert_eq!(ui.get_current(), -1);
}

#[test]
fn programmatic_heights_position_unmaterialized_rows_and_drive_page_navigation() {
    use xrack_runtime::{Length, ScrollRequests};

    let mut ui = StandardTableDemo::new();
    ui.set_inventory(vec![InventoryRow::default(); 1_000]);
    ui.invoke_set_height(0, Length(160.0));
    ui.invoke_set_height(899, Length(80.0));
    let path = [0, 1];
    let viewport = Size::new(620.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    ui.set_current(0);
    let (scene, layout) = prepare(&ui, &input);
    assert!(scene.at_path(&path).unwrap().children.len() < 10);
    input.focus_next(&scene, &layout, false);
    let body_height = layout
        .at_path(&path)
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .body
        .height;
    let expected = 1 + ((body_height - 160.0).max(0.0) / 32.0).floor() as usize;
    let page = input
        .list_key(&scene, &layout, NavigationKey::PageDown)
        .unwrap();
    assert_eq!(page.action, ControlAction::Select(expected));
    let requests = ScrollRequests::default();
    {
        let _scope = requests.enter();
        ui.invoke_center_row(900);
    }
    let (scene, layout) = prepare(&ui, &input);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let (scene, layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let list = scene.at_path(&path).unwrap().list.unwrap();
    let row = layout
        .at_path(&[0, 1, 900 - list.first_row])
        .unwrap()
        .bounds;
    assert!(
        (row.y + row.height / 2.0 - (geometry.body.y + geometry.body.height / 2.0)).abs() < 0.01
    );
    let offset = geometry.body.height;
    assert!(
        (layout.at_path(&path).unwrap().scroll.unwrap().offset
            - (900.0 * 32.0 + 176.0 + (32.0 - offset) / 2.0))
            .abs()
            < 0.01
    );
    ui.invoke_set_height(899, Length(-1.0));
    let scene = ui.render_shared();
    let heights = scene.at_path(&path).unwrap().row_heights.as_ref().unwrap();
    assert_eq!(heights.height(899), 32.0);
}

#[test]
fn frozen_cells_keep_their_hit_targets_and_edit_the_original_field() {
    let mut ui = RecordTableDemo::new();
    ui.set_pinned(1);
    let viewport = Size::new(760.0, 330.0);
    let path = [0, 1];
    let prepare = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 320
        }
    ));
    let (_, layout) = prepare(&ui, &input);
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Horizontal, 70.0));
    let (scene, layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let seam = geometry.frozen_boundary().unwrap();
    let cell_path = [0, 1, 0, 0, 0];
    let body = Point {
        x: seam - 20.0,
        y: geometry.body.y + 12.0,
    };
    assert!(geometry.columns[1].x < body.x);
    assert_eq!(layout.at_path(&[0, 1, 0, 0, 1]).unwrap().clip.x, seam);
    input.pointer_move(&scene, &layout, body);
    assert_eq!(input.hovered.as_ref().unwrap().path, cell_path);
    let model = ui.model_records();
    let old = model.row(0).unwrap();
    assert!(ui.commit_table_cell(&scene, &cell_path, &old.name, "Frozen name".into()));
    let edited = model.row(0).unwrap();
    assert_eq!(edited.name, "Frozen name");
    assert_eq!(edited.owner, old.owner);

    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: body.x,
            y: geometry.header.y + 12.0,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    let sort = input
        .table_header_release(&scene, RowSelection::Replace, false)
        .unwrap();
    assert_eq!(sort.action, ControlAction::SortColumn(0));
    ui.invoke_move_visible_column(0, 1);
    let (_, layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert!(geometry.is_frozen(1) && !geometry.is_frozen(0));
    assert_eq!(geometry.columns[1].x, geometry.body.x);
}

#[test]
fn dragging_columns_preserves_cell_selection_widths_and_sort_identity() {
    let mut ui = StandardTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        (0..24)
            .map(|units| InventoryRow {
                name: format!("Device {units}"),
                units,
                ..InventoryRow::default()
            })
            .collect(),
    );
    let sorted = std::rc::Rc::new(std::cell::Cell::new(None));
    let capture = sorted.clone();
    ui.on_sort_requested(move |column, descending| capture.set(Some((column, descending))));
    let path = [0, 1];
    let viewport = Size::new(620.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, _) = prepare(&ui, &input);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 210
        }
    ));
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace
        },)
    ));
    let (scene, layout) = prepare(&ui, &input);
    let view = layout.at_path(&path).unwrap();
    let geometry = view.table.as_ref().unwrap();
    let header_point = |column: usize, fraction: f32| Point {
        x: geometry.columns[column].x + geometry.columns[column].width * fraction,
        y: geometry.header.y + 12.0,
    };
    input.pointer_move(&scene, &layout, header_point(0, 0.5));
    input.pointer_press_in_scene(&scene, &layout);
    assert!(!input.table_column_move_drag(&layout, Instant::now()));
    input.pointer_move(&scene, &layout, header_point(2, 0.75));
    assert!(input.table_column_move_drag(&layout, Instant::now()));
    assert_eq!(
        input.table_column_move_marker(&path, scene.at_path(&path).unwrap(), view),
        Some(geometry.columns[3].x)
    );
    let moved = input.finish_table_column_move(&scene, &layout).unwrap();
    assert_eq!(moved.action, ControlAction::MoveColumn { from: 0, to: 2 });
    assert!(ui.control_at(&scene, &moved.path, moved.action));
    assert!(!ui.control_at(&scene, &moved.path, moved.action));
    assert_eq!(ui.get_moved_column(), 0);
    assert_eq!(ui.get_moved_position(), 2);
    assert_eq!(sorted.get(), None);

    let (scene, layout) = prepare(&ui, &input);
    input.reconcile(&scene, &layout);
    let table = scene.at_path(&path).unwrap();
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(geometry.visual_columns().collect::<Vec<_>>(), [1, 2, 0, 3]);
    assert_eq!(geometry.columns[0].width, 210.0);
    let row = &table.children[0];
    let row_key = row.row_key.unwrap();
    assert_eq!(
        xrack_layout::child_reading_order(&row.children[0]).collect::<Vec<_>>(),
        [1, 2, 0, 3]
    );
    assert!(row.children[0].children[0].boolean("table-selected", false));
    let extend = input
        .list_key_with_selection(&scene, &layout, NavigationKey::Left, RowSelection::Extend)
        .unwrap();
    assert_eq!(
        extend.action,
        ControlAction::TableSelection(TableSelectionAction::Cell {
            row: 0,
            column: 2,
            operation: RowSelection::Extend
        })
    );
    assert!(ui.control_at(&scene, &extend.path, extend.action));
    let (scene, layout) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    let selection = table.table_selection.as_ref().unwrap();
    assert!(selection.contains_cell(row_key, 0));
    assert!(selection.contains_cell(row_key, 2));
    assert!(!selection.contains_cell(row_key, 1));
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: geometry.columns[0].x + 30.0,
            y: geometry.header.y + 12.0,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    let sort = input
        .table_header_release(&scene, RowSelection::Replace, false)
        .unwrap();
    assert_eq!(sort.action, ControlAction::SortColumn(0));
    assert!(ui.control_at(&scene, &sort.path, sort.action));
    assert_eq!(sorted.get(), Some((0, false)));

    ui.invoke_move_visible_column(2, 0);
    let (scene, layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(geometry.visual_columns().collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(
        scene
            .at_path(&path)
            .unwrap()
            .table_header
            .as_ref()
            .unwrap()
            .sort
            .unwrap()
            .column,
        0
    );
    assert_eq!(ui.get_moved_position(), 0);
    ui.invoke_move_visible_column(-1, 0);
    assert_eq!(ui.get_moved_position(), 0);
}

#[test]
fn dragging_near_the_header_edge_scrolls_to_offscreen_columns() {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(vec![InventoryRow::default(); 10]);
    let path = [0, 1];
    let viewport = Size::new(380.0, 380.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (mut scene, mut layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let y = geometry.header.y + 12.0;
    let edge = Point {
        x: geometry.header.x + geometry.header.width - 2.0,
        y,
    };
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: geometry.columns[0].x + 36.0,
            y,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    input.pointer_move(&scene, &layout, edge);
    let now = Instant::now();
    assert!(input.table_column_move_drag(&layout, now));
    let wake = input.table_column_move_deadline(&scene, &layout).unwrap();
    assert!(wake > now);
    assert!(!input.advance_table_column_move(&scene, &layout, now));
    for _ in 0..32 {
        let Some(wake) = input.table_column_move_deadline(&scene, &layout) else {
            break;
        };
        assert!(input.advance_table_column_move(&scene, &layout, wake));
        (scene, layout) = prepare(&ui, &input);
        input.reconcile(&scene, &layout);
    }
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert!(geometry.horizontal.offset > 0.0);
    assert_eq!(geometry.horizontal.offset, geometry.horizontal.max_offset);
    assert_eq!(geometry.visual_columns().collect::<Vec<_>>(), [0, 1, 2, 3]);
    let moved = input.finish_table_column_move(&scene, &layout).unwrap();
    assert_eq!(moved.action, ControlAction::MoveColumn { from: 0, to: 3 });
    assert!(ui.control_at(&scene, &moved.path, moved.action));
    assert_eq!(ui.get_moved_position(), 3);
}

#[test]
fn moved_table_cells_still_edit_their_declared_record_fields() {
    let mut ui = RecordTableDemo::new();
    let viewport = Size::new(760.0, 330.0);
    let path = [0, 1];
    ui.invoke_move_visible_column(0, 4);
    let (scene, layout) = prepare_scene(
        &ui,
        ui.render_shared(),
        viewport,
        &mut FixedText,
        &Default::default(),
    );
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(geometry.visual_index(0), Some(4));
    assert_eq!(
        layout.at_path(&[0, 1, 0, 0, 0]).unwrap().bounds.x,
        geometry.columns[0].x
    );
    let model = ui.model_records();
    let original = model.row(0).unwrap();
    assert!(ui.commit_table_cell(&scene, &[0, 1, 0, 0, 0], &original.name, "New name".into()));
    let edited = model.row(0).unwrap();
    assert_eq!(edited.name, "New name");
    assert_eq!(edited.owner, original.owner);
}

#[test]
fn dragging_a_row_header_reorders_the_view_across_virtual_rows() {
    move_table_row_across_virtual_rows(false);
}

#[test]
fn touching_a_row_header_reorders_the_view_across_virtual_rows() {
    move_table_row_across_virtual_rows(true);
}

fn move_table_row_across_virtual_rows(touch: bool) {
    let mut ui = StandardTableDemo::new();
    ui.set_row_header_size(xrack_runtime::Length(88.0));
    ui.set_move_rows(true);
    ui.set_pinned(1);
    ui.set_inventory(
        (0..1_000)
            .map(|units| InventoryRow {
                units,
                name: format!("Device {units}"),
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_set_height(0, xrack_runtime::Length(60.0));
    let path = [0, 1];
    let viewport = Size::new(510.0, 330.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    assert!(ui.control_at(&ui.render_shared(), &path, ControlAction::Select(0)));
    let (mut scene, mut layout) = prepare(&ui, &input);
    let model = ui.model_inventory();
    let first = model.row_key(0).unwrap();
    let item = layout.at_path(&path).unwrap();
    let gutter = item.table.as_ref().unwrap().row_header;
    let start = Point {
        x: gutter.x + 20.0,
        y: item.children[0].bounds.y + 16.0,
    };
    let edge = Point {
        y: gutter.y + gutter.height - 6.0,
        ..start
    };
    let now = Instant::now();
    let now = if touch {
        assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
        let hold = input.table_touch_hold_deadline().unwrap();
        assert_eq!(hold.duration_since(now), Duration::from_millis(500));
        assert!(!input.advance_table_touch_hold(&scene, &layout, hold - Duration::from_millis(1)));
        assert!(input.advance_table_touch_hold(&scene, &layout, hold));
        assert!(input.touch_table_drag_claimed());
        assert!(!input.touch_table_selection_claimed());
        assert!(input.table_row_move_deadline(&scene, &layout).is_none());
        assert!(input.move_scroll_gesture(&scene, &layout, edge, hold));
        hold
    } else {
        input.pointer_move(&scene, &layout, start);
        input.pointer_press_in_scene(&scene, &layout);
        assert!(!input.table_row_move_claimed());
        input.pointer_move(&scene, &layout, edge);
        assert!(input.table_row_move_drag(&layout, now));
        now
    };
    for frame in 1..20 {
        if input.advance_table_row_move(&scene, &layout, now + Duration::from_millis(frame * 20)) {
            (scene, layout) = prepare(&ui, &input);
        }
    }
    let node = scene.at_path(&path).unwrap();
    assert!(node.list.unwrap().first_row > 0);
    assert!(node.children.len() < 12);
    let marker = input
        .table_row_move_marker(&path, node, layout.at_path(&path).unwrap())
        .unwrap();
    assert_eq!(marker.x, gutter.x);
    assert!(marker.width > gutter.width);
    assert_eq!(ui.get_current(), 0);
    assert_eq!(ui.get_moved_row_to(), -1);
    let action = if touch {
        let release =
            input.end_scroll_gesture(&scene, &layout, edge, now + Duration::from_millis(400));
        assert!(!release.tap && !release.scrolled);
        assert!(!input.touch_table_drag_claimed());
        assert!(input.pointer_position().is_none());
        release.control.unwrap()
    } else {
        input.finish_table_row_move(&scene, &layout).unwrap()
    };
    let ControlAction::MoveRow {
        from: 0, to, row, ..
    } = action.action
    else {
        panic!("expected a row move")
    };
    assert_eq!(row, first);
    assert!(to >= node.list.unwrap().first_row);
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(model.row_key(0), Some(first));
    assert_eq!(ui.get_current(), to as i64);
    assert_eq!(
        (
            ui.get_moved_model_row(),
            ui.get_moved_row_from(),
            ui.get_moved_row_to()
        ),
        (0, 0, to as i64)
    );
    let (scene, _) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(xrack_runtime::ListMetrics::row_height_at(table, to), 60.0);
    assert!(
        table
            .table_selection
            .as_ref()
            .unwrap()
            .keys()
            .contains(&first)
    );
    assert_eq!(ui.list_rows(&path, to..to + 1)[0].row_key, Some(first));
    assert!(!ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.list_rows(&path, 0..1)[0].row_key, model.row_key(1));
    model.push(InventoryRow::default());
    assert_eq!(ui.list_rows(&path, 0..1)[0].row_key, Some(first));
}

#[test]
fn keyboard_reorders_the_current_row_even_when_it_is_offscreen() {
    let mut ui = StandardTableDemo::new();
    ui.set_move_rows(true);
    ui.set_inventory(
        (0..1000)
            .map(|units| InventoryRow {
                units,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.set_current(800);
    let path = [0, 1];
    let mut input = Interaction::default();
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        let mut scene = ui.render_shared();
        std::rc::Rc::make_mut(&mut scene).children[0].children[1]
            .properties
            .insert("follow-current", false.into());
        prepare_scene_with_input(ui, scene, Size::new(510.0, 330.0), &mut FixedText, input)
    };
    let (scene, layout) = prepare(&ui, &input);
    input.focus_next(&scene, &layout, false);
    assert!(input.is_focused(&path));
    let node = scene.at_path(&path).unwrap();
    assert_eq!(node.list.unwrap().first_row, 0);
    assert!(node.children.len() < 12);
    let row = ui.model_inventory().row_key(800).unwrap();
    let action = input
        .table_row_move_key(&scene, &layout, NavigationKey::Down)
        .unwrap();
    assert_eq!(
        action.action,
        ControlAction::MoveRow {
            from: 800,
            to: 801,
            row,
            stamp: node.standard_list_stamp.unwrap(),
        }
    );
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_current(), 801);
    assert_eq!(ui.list_rows(&path, 801..802)[0].row_key, Some(row));
    let (scene, layout) = prepare(&ui, &input);
    assert!(!ui.control_at(&scene, &path, action.action));
    let action = input
        .table_row_move_key(&scene, &layout, NavigationKey::Up)
        .unwrap();
    assert!(ui.control_at(&scene, &path, action.action));
    assert_eq!(ui.get_current(), 800);
    ui.set_current(0);
    let (scene, layout) = prepare(&ui, &input);
    assert!(
        input
            .table_row_move_key(&scene, &layout, NavigationKey::Up)
            .is_none()
    );
    ui.set_move_rows(false);
    let (scene, layout) = prepare(&ui, &input);
    assert!(
        input
            .table_row_move_key(&scene, &layout, NavigationKey::Down)
            .is_none()
    );
}

#[test]
fn ordered_table_clipboard_writes_to_source_keys_and_rejects_old_requests() {
    let mut ui = RecordTableDemo::new();
    let path = [0, 1];
    let model = ui.model_records();
    let original = model.snapshot();
    let keys = (0..3).map(|row| model.row_key(row)).collect::<Vec<_>>();
    assert!(ui.control_at(&ui.render_shared(), &path, ControlAction::Select(1)));
    let old_scene = ui.render_shared();
    let pending = ui
        .prepare_table_paste(&old_scene, &path, 0, vec![0])
        .unwrap();
    ui.invoke_move_visible_row(0, 2);
    assert!(!pending("stale"));
    assert!(
        ui.prepare_table_paste(&old_scene, &path, 0, vec![0])
            .is_none()
    );
    assert!(ui.copy_table_at(&old_scene, &path, &[0]).is_none());
    assert_eq!(model.snapshot(), original);
    assert_eq!(
        ui.list_rows(&path, 0..3)
            .iter()
            .map(|row| row.row_key)
            .collect::<Vec<_>>(),
        [keys[1], keys[2], keys[0]]
    );
    let scene = ui.render_shared();
    assert_eq!(
        ui.copy_table_at(&scene, &path, &[0]).as_deref(),
        Some(original[1].name.as_str())
    );
    let paste = ui
        .prepare_table_paste(&ui.render_shared(), &path, 1, vec![0])
        .unwrap();
    assert!(paste("Last source\nFirst source"));
    assert_eq!(model.row(0).unwrap().name, "First source");
    assert_eq!(model.row(1).unwrap(), original[1]);
    assert_eq!(model.row(2).unwrap().name, "Last source");
    assert!(ui.replay_table_paste_at(&ui.render_shared(), &path, false));
    assert_eq!(model.snapshot(), original);

    let mut input = Interaction::default();
    let (scene, layout) = prepare_scene_with_input(
        &ui,
        ui.render_shared(),
        Size::new(760.0, 330.0),
        &mut FixedText,
        &input,
    );
    let cell = layout.at_path(&[0, 1, 0, 0, 0]).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 24.0,
            y: cell.y + 16.0,
        },
    );
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.begin_table_edit(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("Edited at visual row zero"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(0).unwrap(), original[0]);
    assert_eq!(model.row(1).unwrap().name, "Edited at visual row zero");
    assert_eq!(model.row_key(1), keys[1]);
}

#[test]
fn row_headers_share_virtual_rows_freeze_and_select_across_offscreen_columns() {
    let mut ui = StandardTableDemo::new();
    ui.set_row_header_size(xrack_runtime::Length(88.0));
    ui.set_resize_rows(true);
    ui.set_move_rows(true);
    ui.set_pinned(1);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        (0..1000)
            .map(|units| InventoryRow {
                units,
                name: format!("Device {units}"),
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_move_visible_column(3, 0);
    ui.invoke_set_height(0, xrack_runtime::Length(58.0));
    let path = [0, 1];
    let viewport = Size::new(490.0, 330.0);
    let prepare = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), viewport, &mut FixedText, input)
    };
    let mut input = Interaction::default();
    let (scene, layout) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    let item = layout.at_path(&path).unwrap();
    let geometry = item.table.as_ref().unwrap();
    let gutter = geometry.row_header;
    let frozen = geometry.columns[3];
    let scrolling = geometry.columns[1];
    assert!(geometry.is_frozen(3));
    assert_eq!(geometry.body.x, gutter.x + gutter.width);
    assert_eq!(geometry.header.x, geometry.body.x);
    assert_eq!(item.children[0].bounds.height, 58.0);
    assert_eq!(table.children[0].text("row-header-text"), Some("Device 0"));
    assert!(input.accessibility_scroll_to(&layout, &path, Orientation::Horizontal, 120.0));
    let (mut scene, mut layout) = prepare(&ui, &input);
    let geometry = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(geometry.row_header, gutter);
    assert_eq!(geometry.columns[3], frozen);
    assert!(geometry.columns[1].x < scrolling.x);

    for (slot, operation, count) in [
        (0, RowSelection::Replace, 4),
        (1, RowSelection::Extend, 8),
        (0, RowSelection::Toggle, 4),
    ] {
        let row = layout.at_path(&path).unwrap().children[slot].bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: gutter.x + 16.0,
                y: row.y + 16.0,
            },
        );
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        let action = input
            .select_group_option_with_selection(&scene, &layout, &target, operation)
            .unwrap();
        assert_eq!(action.path, path);
        assert!(ui.control_at(&scene, &action.path, action.action));
        (scene, layout) = prepare(&ui, &input);
        let table = scene.at_path(&path).unwrap();
        assert_eq!(table.table_selection.as_ref().unwrap().len(), count);
    }
    let item = layout.at_path(&path).unwrap();
    let row = item.children[0].bounds;
    let grip = Point {
        x: gutter.x + 16.0,
        y: row.y + row.height - 1.0,
    };
    input.pointer_move(&scene, &layout, grip);
    assert!(input.table_row_resize_cursor());
    input.pointer_press_in_scene(&scene, &layout);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            y: grip.y + 12.0,
            ..grip
        },
    );
    let resize = input.table_row_resize_drag(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    assert!(input.finish_table_resize());
    (scene, layout) = prepare(&ui, &input);
    assert_eq!(
        layout.at_path(&path).unwrap().children[0].bounds.height,
        70.0
    );

    let now = Instant::now();
    let start = Point {
        x: gutter.x + 16.0,
        y: gutter.y + 86.0,
    };
    let end = Point {
        y: start.y - 90.0,
        ..start
    };
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    assert!(input.table_touch_hold_deadline().is_some());
    assert!(input.move_scroll_gesture(&scene, &layout, end, now + Duration::from_millis(80)));
    assert!(input.table_touch_hold_deadline().is_none());
    assert!(!input.table_row_move_claimed());
    let (scene, layout) = prepare(&ui, &input);
    let table = scene.at_path(&path).unwrap();
    let first = table.list.unwrap().first_row;
    assert!(first > 0);
    assert!(table.children.len() < 12);
    assert_eq!(
        table.children[0].text("row-header-text"),
        Some(format!("Device {first}").as_str())
    );
    assert_eq!(
        layout
            .at_path(&path)
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .row_header,
        gutter
    );
}

#[test]
fn automatic_table_sort_preserves_source_order_and_selection_across_live_edits() {
    let mut view = TableSortViewsDemo::new();
    view.set_left_current(0);
    view.invoke_initialize();
    assert_eq!(view.get_left_current(), 3);
    let mut ui = UiDriver::new(view, FixedText, Size::new(680.0, 370.0));
    let model = ui.component().model_inventory();
    let source_keys = (0..4)
        .map(|index| model.row_key(index).unwrap())
        .collect::<Vec<_>>();
    let visual_keys = |node: &xrack_runtime::Node| {
        node.children
            .iter()
            .map(|row| row.row_key.unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        visual_keys(ui.node("quantity-table")),
        [
            source_keys[2],
            source_keys[1],
            source_keys[3],
            source_keys[0]
        ]
    );
    assert_eq!(
        visual_keys(ui.node("name-table")),
        [
            source_keys[1],
            source_keys[2],
            source_keys[3],
            source_keys[0]
        ]
    );
    assert!(!ui.node("quantity-table").boolean("movable-rows", false));

    let snapshot = ui.scene().clone();
    let path = [0, 1, 0, 1];
    ui.update(|view| {
        assert!(view.control_at(&snapshot, &path, ControlAction::Select(1)));
    });
    let stamp = ui.node("name-table").standard_list_stamp;
    ui.update(|view| {
        let model = view.model_inventory();
        let mut alpha = model.row(1).unwrap();
        alpha.units = 40;
        assert!(model.set_row(1, alpha));
    });
    assert_eq!(
        visual_keys(ui.node("quantity-table")),
        [
            source_keys[2],
            source_keys[3],
            source_keys[0],
            source_keys[1]
        ]
    );
    assert_eq!(ui.node("name-table").standard_list_stamp, stamp);
    assert_eq!(model.row(1).unwrap().name, "Alpha");
    assert!(
        ui.node("quantity-table")
            .table_selection
            .as_ref()
            .unwrap()
            .keys()
            .contains(&source_keys[1])
    );
    ui.update(|view| {
        assert!(!view.control_at(&snapshot, &path, ControlAction::SortColumn(1)));
        view.set_left_current(3);
    });
    ui.click("clear-quantity-sort");
    assert_eq!(visual_keys(ui.node("quantity-table")), source_keys);
    assert_eq!(ui.component().get_left_current(), 1);
    assert!(ui.node("quantity-table").boolean("movable-rows", false));
    assert!(
        ui.node("quantity-table")
            .table_selection
            .as_ref()
            .unwrap()
            .keys()
            .contains(&source_keys[1])
    );
}

#[test]
fn programmatic_table_sort_uses_declaration_columns_and_publishes_state_before_callbacks() {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        [9, 2, 6]
            .map(|units| InventoryRow {
                name: format!("Device {units}"),
                units,
                available: true,
                refreshed: Time::default(),
            })
            .to_vec(),
    );
    let model = ui.model_inventory();
    let rows = model.clone();
    let original_first = model.row_key(0);
    let owner = ui.as_weak();
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let events = calls.clone();
    ui.on_sort_requested(move |column, descending| {
        let ui = owner.upgrade().unwrap();
        assert_eq!(ui.get_sort_column(), column);
        assert_eq!(ui.get_sort_descending(), descending);
        assert_eq!(column, 1);
        rows.sort_by(|a, b| {
            if descending {
                b.units.cmp(&a.units)
            } else {
                a.units.cmp(&b.units)
            }
        });
        events.set(events.get() + 1);
    });

    ui.invoke_move_visible_column(1, 0);
    assert_eq!(ui.get_sort_column(), -1);
    ui.invoke_sort_rows(1, false);
    assert_eq!(ui.get_sort_column(), 1);
    assert!(!ui.get_sort_descending());
    assert_eq!(model.row(0).unwrap().units, 2);
    assert_eq!(model.row_key(2), original_first);
    let sort = Some(TableSort {
        column: 1,
        descending: false,
    });
    assert_eq!(ui.table_sort("inventory-table"), sort);
    assert!(!ui.set_table_sort("inventory-table", sort));
    assert!(!ui.set_table_sort(
        "inventory-table",
        Some(TableSort {
            column: 20,
            descending: false
        })
    ));
    assert_eq!(calls.get(), 1);
    let scene = ui.render_shared();
    let table = scene.at_path(&[0, 1]).unwrap();
    assert_eq!(table.table_column_order.visual_index(4, 1), Some(0));
    assert_eq!(table.table_header.as_ref().unwrap().sort, sort);

    ui.set_quantity_sortable(false);
    ui.invoke_sort_rows(1, true);
    assert_eq!(calls.get(), 1);
    ui.invoke_clear_sort_indicator();
    assert_eq!(ui.get_sort_column(), -1);
    assert_eq!(model.row(0).unwrap().units, 2);
    assert!(!ui.control_at(&scene, &[0, 1], ControlAction::SortColumn(1)));
    ui.set_quantity_sortable(true);
    assert!(ui.set_table_sort(
        "inventory-table",
        Some(TableSort {
            column: 1,
            descending: true
        })
    ));
    assert_eq!(model.row(0).unwrap().units, 9);
    assert_eq!(calls.get(), 2);
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
fn table_edit_triggers_distinguish_first_click_selected_click_and_typing() {
    let mut ui = RecordTableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_selected_click_edit(true);
    ui.set_double_click_edit(false);
    ui.set_key_edit(false);
    let mut input = Interaction::default();
    let render = |ui: &RecordTableDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(760.0, 330.0),
            &mut FixedText,
            input,
        )
    };
    let click = |ui: &mut RecordTableDemo, input: &mut Interaction, path: &[usize]| {
        let (scene, layout) = render(ui, input);
        input.reconcile(&scene, &layout);
        let cell = layout.at_path(path).unwrap().bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: cell.x + 20.0,
                y: cell.y + 12.0,
            },
        );
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.path, path);
        let started = input.table_cell_click(&scene, &layout, Some(&target), Instant::now());
        let select = input
            .select_group_option_with_selection(&scene, &layout, &target, RowSelection::Replace)
            .unwrap();
        ui.control_at(&scene, &select.path, select.action);
        started
    };
    let name = [0, 1, 0, 0, 0];
    let owner = [0, 1, 0, 0, 1];
    assert!(!click(&mut ui, &mut input, &name));
    assert!(click(&mut ui, &mut input, &name));
    assert!(input.cancel_table_edit());
    // Selecting one cell does not make a different cell in the same row editable by tapping.
    assert!(!click(&mut ui, &mut input, &owner));
    let (scene, layout) = render(&ui, &input);
    assert!(!input.begin_table_edit(&scene, &layout));
    assert!(!input.begin_table_typing(&scene, &layout));
    ui.set_typing_edit(true);
    let (scene, layout) = render(&ui, &input);
    assert!(input.begin_table_typing(&scene, &layout));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("研发组"),
        EditSource::Keyboard,
        Instant::now()
    ));
    assert_eq!(input.editor_value(scene.at_path(&owner).unwrap()), "研发组");
    assert_eq!(ui.model_records().row(0).unwrap().owner, "网络组");
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(ui.model_records().row(0).unwrap().owner, "研发组");

    ui.set_click_edit(true);
    ui.set_lock_owner(true);
    assert!(!click(&mut ui, &mut input, &owner));
    assert!(click(&mut ui, &mut input, &[0, 1, 1, 0, 0]));
}

#[test]
fn table_edit_request_targets_a_virtual_row_after_positioning() {
    use xrack_runtime::{FocusRequests, ScrollRequests};
    let mut ui = RecordTableDemo::new();
    ui.set_records(
        (0..200)
            .map(|row| StockRow {
                name: format!("Device {row}"),
                owner: format!("Owner {row}"),
                ..StockRow::default()
            })
            .collect(),
    );
    ui.set_key_edit(false);
    ui.set_double_click_edit(false);
    let model = ui.model_records();
    let mut input = Interaction::default();
    let focus = FocusRequests::default();
    let scroll = ScrollRequests::default();
    let _focus_context = focus.enter();
    let _scroll_context = scroll.enter();
    let flush = |ui: &RecordTableDemo, input: &mut Interaction| {
        let prepare = |input: &Interaction| {
            prepare_scene_with_input(
                ui,
                ui.render_shared(),
                Size::new(760.0, 330.0),
                &mut FixedText,
                input,
            )
        };
        let (mut scene, mut layout) = prepare(input);
        if input.apply_scroll_requests(&scene, &layout, scroll.drain()) {
            (scene, layout) = prepare(input);
        }
        input.reconcile(&scene, &layout);
        input.apply_focus_requests(&scene, &layout, focus.drain());
        scene
    };

    ui.invoke_start_edit(199, 1);
    flush(&ui, &mut input);
    assert!(input.focused.is_none());
    ui.invoke_move_visible_column(1, 0);
    ui.invoke_reveal_and_edit(199, 1);
    let scene = flush(&ui, &mut input);
    let target = input.focused.clone().unwrap();
    assert_eq!(target.row_key, model.row_key(199));
    assert_eq!(target.path.last(), Some(&1));
    assert!(input.table_cell_editing(&target.path));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("draft"),
        EditSource::Atomic,
        Instant::now()
    ));
    ui.invoke_start_edit(-1, 0);
    ui.invoke_start_edit(199, 2);
    let scene = flush(&ui, &mut input);
    assert_eq!(input.focused.as_ref(), Some(&target));
    assert_eq!(
        input.editor_value(scene.at_path(&target.path).unwrap()),
        "draft"
    );
    ui.invoke_cancel_edit();
    flush(&ui, &mut input);
    assert!(!input.table_cell_editing(&target.path));
    assert_eq!(model.row(199).unwrap().owner, "Owner 199");

    ui.set_lock_owner(true);
    ui.invoke_start_edit(199, 1);
    flush(&ui, &mut input);
    assert!(!input.table_cell_editing(&target.path));
}

#[test]
fn table_tab_editing_skips_unwritable_rows_and_scrolls_both_directions() {
    let mut ui = RecordTableDemo::new();
    ui.set_editing_tools(true);
    ui.set_lock_owner(true);
    ui.set_lock_inactive_names(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(
        (0..40)
            .map(|index| StockRow {
                name: format!("Device {index}"),
                available: index == 0 || index == 20,
                ..StockRow::default()
            })
            .collect(),
    );
    assert!(ui.select_table(
        "stock",
        TableSelectionAction::Cell {
            row: 0,
            column: 0,
            operation: RowSelection::Replace,
        }
    ));
    let model = ui.model_records();
    let mut driver = UiDriver::new(ui, FixedText, Size::new(760.0, 330.0));
    driver.tab();
    assert!(driver.begin_table_edit());
    driver.fill_focused("Renamed");
    driver.tab();
    assert_eq!(model.row(0).unwrap().name, "Renamed");
    let target = driver.input().focused.as_ref().unwrap();
    assert!(driver.input().table_cell_editing(&target.path));
    assert_eq!(target.row_key, model.row_key(20));
    assert_eq!(target.path.last(), Some(&0));
    assert!(driver.node("stock").list.unwrap().first_row > 0);
    assert!(driver.node("stock").children.len() < 20);

    driver.tab_with_direction(true);
    let target = driver.input().focused.as_ref().unwrap();
    assert!(driver.input().table_cell_editing(&target.path));
    assert_eq!(target.row_key, model.row_key(0));
    assert_eq!(driver.node("stock").list.unwrap().first_row, 0);
    assert_eq!(model.row(20).unwrap().name, "Device 20");
    driver.tab_with_direction(true);
    assert!(driver.input().editor.is_none());
}

#[test]
fn standard_table_tab_editing_keeps_invalid_drafts_and_follows_sorted_identity() {
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_auto_sort_fields(true);
    ui.set_lock_refresh_time(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        (0..80)
            .map(|index| InventoryRow {
                name: format!("Device {index}"),
                units: index + 10,
                ..InventoryRow::default()
            })
            .collect(),
    );
    ui.invoke_sort_rows(1, false);
    ui.invoke_move_visible_column(0, 3);
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 0,
            column: 1,
            operation: RowSelection::Replace,
        }
    ));
    let model = ui.model_inventory();
    let key = model.row_key(0);
    let mut driver = UiDriver::new(ui, FixedText, Size::new(620.0, 380.0));
    driver.tab();
    assert!(driver.begin_table_edit());
    driver.fill_focused("invalid");
    let original = driver.input().focused.clone().unwrap();
    driver.tab();
    assert_eq!(driver.input().focused.as_ref(), Some(&original));
    assert!(driver.input().table_cell_editing(&original.path));
    assert_eq!(model.row(0).unwrap().units, 10);

    driver.fill_focused("500");
    driver.tab();
    assert_eq!(model.row(0).unwrap().units, 500);
    let target = driver.input().focused.as_ref().unwrap();
    assert_eq!(target.row_key, key);
    assert_eq!(target.path.last(), Some(&2));
    assert!(driver.input().table_cell_editing(&target.path));
    assert!(driver.node("inventory-table").list.unwrap().first_row > 60);

    driver.tab();
    let target = driver.input().focused.as_ref().unwrap();
    assert_eq!(target.row_key, key);
    assert_eq!(target.path.last(), Some(&0));
    assert!(driver.input().table_cell_editing(&target.path));
    driver.tab_with_direction(true);
    let target = driver.input().focused.as_ref().unwrap();
    assert_eq!(target.row_key, key);
    assert_eq!(target.path.last(), Some(&2));
    assert!(driver.input().table_cell_editing(&target.path));
}

#[test]
fn editable_standard_columns_validate_drafts_and_write_the_sorted_source_record() {
    use xrack_runtime::{FocusRequests, ScrollRequests, editable_table_cell};
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_auto_sort_fields(true);
    ui.set_target(SelectionTarget::Cells);
    ui.set_inventory(
        (0..200)
            .map(|row| InventoryRow {
                name: format!("Device {row}"),
                units: 200 - row,
                ..InventoryRow::default()
            })
            .collect(),
    );
    let model = ui.model_inventory();
    let key = model.row_key(0).unwrap();
    ui.invoke_sort_rows(1, false);
    ui.invoke_move_visible_column(1, 0);
    let mut input = Interaction::default();
    let focus = FocusRequests::default();
    let scroll = ScrollRequests::default();
    let _focus_context = focus.enter();
    let _scroll_context = scroll.enter();
    let render = |ui: &StandardTableDemo, input: &Interaction| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(620.0, 380.0),
            &mut FixedText,
            input,
        )
    };
    let flush = |ui: &StandardTableDemo, input: &mut Interaction| {
        let (mut scene, mut layout) = render(ui, input);
        if input.apply_scroll_requests(&scene, &layout, scroll.drain()) {
            (scene, layout) = render(ui, input);
        }
        input.reconcile(&scene, &layout);
        input.apply_focus_requests(&scene, &layout, focus.drain());
        scene
    };
    ui.invoke_edit_inventory_cell(199, 1);
    let scene = flush(&ui, &mut input);
    let target = input.focused.clone().unwrap();
    assert_eq!(target.row_key, Some(key));
    assert_eq!(target.path.last(), Some(&1));
    let cell = scene.at_path(&target.path).unwrap();
    assert_eq!(cell.text("text"), Some("200"));
    assert!(editable_table_cell(cell));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("not an integer"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert!(input.table_cell_edit_invalid(cell, &target.path));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert!(input.table_cell_editing(&target.path));
    assert_eq!(model.row(0).unwrap().units, 200);

    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("17"),
        EditSource::Atomic,
        Instant::now()
    ));
    let mut updated = model.row(0).unwrap();
    updated.name = "External name".into();
    assert!(model.set_row(0, updated));
    let scene = flush(&ui, &mut input);
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(0).unwrap().name, "External name");
    assert_eq!(model.row(0).unwrap().units, 17);
    assert_eq!(model.row_key(0), Some(key));

    // The same record is now at the first occurrence of quantity 17 in the sorted view.
    ui.invoke_edit_inventory_cell(16, 1);
    let scene = flush(&ui, &mut input);
    let target = input.focused.clone().unwrap();
    assert_eq!(target.row_key, Some(key));
    assert!(input.table_cell_editing(&target.path));
    let mut changed = model.row(0).unwrap();
    changed.units = 18;
    assert!(model.set_row(0, changed));
    assert!(!ui.commit_table_cell(&scene, &target.path, "17", "19".into()));
    assert_eq!(model.row(0).unwrap().units, 18);

    ui.invoke_edit_inventory_cell(17, 1);
    let scene = flush(&ui, &mut input);
    let target = input.focused.clone().unwrap();
    assert_eq!(target.row_key, Some(key));
    ui.set_edit_columns(false);
    assert!(!ui.commit_table_cell(&scene, &target.path, "18", "21".into()));
    assert_eq!(model.row(0).unwrap().units, 18);
    let scene = flush(&ui, &mut input);
    assert!(!input.table_cell_editing(&target.path));
    assert!(
        scene
            .find("inventory-table")
            .unwrap()
            .children
            .iter()
            .flat_map(|row| row.children[0].children.iter())
            .all(|cell| !editable_table_cell(cell))
    );
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
fn stack_page_guards_keep_an_inactive_draft_and_deliver_lifecycle_after_commit() {
    let mut ui = UiDriver::new(StackGuardDemo::new(), FixedText, Size::new(560.0, 360.0));
    assert_eq!(ui.component().get_lifecycle(), "");
    assert_eq!(ui.component().get_editor_index(), -1);
    assert!(!ui.component().get_editor_active());

    ui.click("edit-note");
    assert_eq!(ui.component().get_editor_index(), 1);
    assert!(ui.component().get_editor_active());
    assert_eq!(
        ui.component().get_lifecycle(),
        " home deactivated editor activated changed editor 2"
    );
    assert_eq!(ui.focused(), Some("note-field"));
    ui.fill_focused("未保存的笔记");
    let editing = ui.component().stack_snapshot("navigation").unwrap();
    assert!(!ui.back());
    assert!(ui.node("confirm-leave").is_visible());
    assert_eq!(ui.component().get_last_destination(), "home");
    assert!(ui.component().get_last_removing());
    assert!(std::sync::Arc::ptr_eq(
        &editing,
        &ui.component().stack_snapshot("navigation").unwrap()
    ));
    assert_eq!(ui.component().get_changes(), 1);
    ui.click("keep-editing");
    assert_eq!(ui.focused(), Some("note-field"));

    ui.click("note-preview");
    assert_eq!(ui.component().get_last_destination(), "preview");
    assert!(!ui.component().get_last_removing());
    assert!(!ui.component().get_editor_active());
    assert_eq!(ui.component().get_editor_index(), 1);
    assert_eq!(ui.component().get_draft(), "未保存的笔记");
    assert!(ui.node("editor").children.is_empty());

    let previewing = ui.component().stack_snapshot("navigation").unwrap();
    ui.click("preview-home");
    assert!(ui.node("confirm-leave").is_visible());
    assert!(std::sync::Arc::ptr_eq(
        &previewing,
        &ui.component().stack_snapshot("navigation").unwrap()
    ));
    assert_eq!(ui.component().get_changes(), 2);
    assert_eq!(
        ui.component().get_lifecycle(),
        " home deactivated editor activated changed editor 2 editor deactivated preview activated changed preview 3"
    );

    ui.click("discard-note");
    assert_eq!(ui.component().get_editor_index(), -1);
    assert_eq!(ui.component().get_draft(), ui.component().get_saved());
    assert_eq!(ui.component().get_changes(), 3);
    assert_eq!(
        ui.component().get_lifecycle(),
        " home deactivated editor activated changed editor 2 editor deactivated preview activated changed preview 3 preview deactivated preview removed editor removed home activated changed home 1"
    );
    assert_eq!(
        ui.node("route-status").text("text"),
        Some("当前页 home · 编辑页索引 -1")
    );
    assert!(ui.node("preview").children.is_empty());
    assert!(!ui.node("confirm-leave").is_visible());
}

#[test]
fn rust_can_veto_a_navigation_through_a_page_guard_alias() {
    use xrack_runtime::StackNavigation;

    let mut ui = StackGuardDemo::new();
    assert!(ui.navigate_stack("navigation", StackNavigation::Push("editor")));
    assert!(ui.navigate_stack("navigation", StackNavigation::Push("preview")));
    let current = ui.stack_snapshot("navigation").unwrap();
    ui.on_can_leave_preview(|destination, removing| {
        assert_eq!(destination, "editor");
        assert!(removing);
        false
    });
    assert!(!ui.navigate_stack("navigation", StackNavigation::Pop));
    assert!(std::sync::Arc::ptr_eq(
        &current,
        &ui.stack_snapshot("navigation").unwrap()
    ));
    ui.on_can_leave_preview(|_, _| true);
    assert!(ui.navigate_stack("navigation", StackNavigation::Pop));
    assert!(ui.get_editor_active());
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
fn shortcuts_share_actions_and_respect_sequence_focus_and_modal_scope() {
    let mut ui = ShortcutDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(532.0, 344.0);
    let now = Instant::now();
    let press = |ui: &mut ShortcutDemo, input: &mut Interaction, key: &str, repeat, when| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
        input.reconcile(&scene, &layout);
        let result = input.shortcut_press(&scene, &layout, key.parse().unwrap(), repeat, when);
        let events: &[_] = match &result {
            ShortcutOutcome::Activate(event) => std::slice::from_ref(event),
            ShortcutOutcome::Ambiguous(events) => events,
            _ => &[],
        };
        for event in events {
            assert!(ui.control_at(&scene, &event.path, event.action));
        }
        result
    };
    assert_eq!(
        press(&mut ui, &mut input, "Primary+K", false, now),
        ShortcutOutcome::Consumed
    );
    assert_eq!(
        press(&mut ui, &mut input, "Primary+K", true, now),
        ShortcutOutcome::Consumed
    );
    assert!(matches!(
        press(&mut ui, &mut input, "Primary+P", false, now),
        ShortcutOutcome::Activate(_)
    ));
    assert_eq!(ui.get_commands(), 1);
    assert!(matches!(
        press(&mut ui, &mut input, "F9", false, now),
        ShortcutOutcome::Activate(_)
    ));
    assert_eq!(
        ui.get_commands(),
        2,
        "the linked menu and shortcut invoke one Action"
    );
    assert_eq!(
        press(&mut ui, &mut input, "F9", true, now),
        ShortcutOutcome::Consumed
    );
    press(&mut ui, &mut input, "Primary+K", false, now);
    assert_eq!(
        press(
            &mut ui,
            &mut input,
            "Primary+P",
            false,
            now + Duration::from_secs(2)
        ),
        ShortcutOutcome::Unmatched
    );
    press(&mut ui, &mut input, "Primary+K", false, now);
    ui.set_command_enabled(false);
    assert_eq!(
        press(&mut ui, &mut input, "Primary+P", false, now),
        ShortcutOutcome::Unmatched
    );
    assert_eq!(ui.get_commands(), 2);
    assert!(matches!(
        press(&mut ui, &mut input, "F7", true, now),
        ShortcutOutcome::Activate(_)
    ));
    assert_eq!(ui.get_repeats(), 1);

    assert_eq!(
        press(&mut ui, &mut input, "F8", false, now),
        ShortcutOutcome::Unmatched
    );
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let entry = [4, 3, 0];
    assert_eq!(scene.at_path(&entry).unwrap().id, Some("entry"));
    input.accessibility_focus(input.accessibility_target(&scene, &layout, &entry).unwrap());
    assert!(matches!(
        press(&mut ui, &mut input, "F8", false, now),
        ShortcutOutcome::Activate(_)
    ));
    assert_eq!(ui.get_focused_hits(), 1);

    ui.set_conflict(true);
    let conflict = press(&mut ui, &mut input, "F6", false, now);
    assert!(matches!(conflict, ShortcutOutcome::Ambiguous(events) if events.len() == 2));
    assert_eq!(ui.get_conflicts(), 2);
    assert_eq!(ui.get_window_hits(), 0);

    ui.set_dialog_open(true);
    assert!(matches!(
        press(&mut ui, &mut input, "F6", false, now),
        ShortcutOutcome::Activate(_)
    ));
    assert_eq!(ui.get_dialog_hits(), 1);
    assert_eq!(ui.get_window_hits(), 0);
    assert_eq!(
        press(&mut ui, &mut input, "F8", false, now),
        ShortcutOutcome::Unmatched
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
fn round_buttons_use_their_painted_shape_and_share_action_activation() {
    use menu_ui::RoundToolbarDemo;
    use xrack_runtime::Length;

    let mut ui = RoundToolbarDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(480.0, 280.0);
    let prepare = |ui: &RoundToolbarDemo, input: &Interaction| {
        prepare_scene_with_input(ui, ui.render_shared(), size, &mut FixedText, input)
    };
    for (point, id) in [
        (Point { x: 25.0, y: 57.0 }, "underlying"),
        (Point { x: 56.0, y: 88.0 }, "circle"),
    ] {
        let (scene, layout) = prepare(&ui, &input);
        input.pointer_move(&scene, &layout, point);
        input.pointer_press_in_scene(&scene, &layout);
        let target = input.pointer_release_target().unwrap();
        assert_eq!(target.id, Some(id));
        assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    }
    assert_eq!(ui.get_underlying_clicks(), 1);
    assert_eq!(ui.get_activations(), 1);
    assert!(ui.get_show_grid());

    let (scene, layout) = prepare(&ui, &input);
    let circle = scene
        .children
        .iter()
        .position(|node| node.id == Some("circle"))
        .unwrap();
    let target = input
        .accessibility_target(&scene, &layout, &[circle])
        .unwrap();
    input.focus_target(&scene, &layout, target);
    input.key_press_with_scene(&scene, ActivationKey::Space);
    let target = input
        .key_release_with_scene(&scene, ActivationKey::Space)
        .unwrap();
    assert_eq!(target.id, Some("circle"));
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert!(!ui.get_show_grid());
    assert_eq!(ui.get_activations(), 2);

    let (scene, layout) = prepare(&ui, &input);
    let ShortcutOutcome::Activate(event) = input.shortcut_press(
        &scene,
        &layout,
        "Primary+G".parse().unwrap(),
        false,
        Instant::now(),
    ) else {
        panic!("round button must expose the linked shortcut");
    };
    assert!(ui.control_at(&scene, &event.path, event.action));
    assert_eq!(ui.get_activations(), 3);

    let corner = Point { x: 25.0, y: 144.0 };
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, corner);
    assert!(input.hovered.is_none());
    ui.set_corner_radius(Length(0.0));
    let (scene, layout) = prepare(&ui, &input);
    input.pointer_move(&scene, &layout, corner);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("custom"));

    let mut driver = UiDriver::new(RoundToolbarDemo::new(), FixedText, size);
    assert!(!driver.can_hit("vertical-line"));
    driver.click("command");
    assert_eq!(driver.focused(), Some("command"));
    driver.tab();
    assert_eq!(driver.focused(), Some("tools"));
}

#[test]
fn action_groups_share_exclusive_state_across_menu_toolbar_and_shortcuts() {
    let mut ui = UiDriver::new(ActionGroupDemo::new(), FixedText, Size::new(620.0, 300.0));
    assert_eq!(ui.scene().children.len(), 1);
    assert_eq!(
        ui.node("detail-item").action,
        ui.node("detail-button").action
    );
    let check = |ui: &UiDriver<ActionGroupDemo, FixedText>, compact: bool| {
        for id in ["detail-item", "detail-button"] {
            assert_eq!(ui.node(id).boolean("checked", false), !compact);
        }
        for id in ["compact-item", "compact-button"] {
            assert_eq!(ui.node(id).boolean("checked", false), compact);
        }
    };
    check(&ui, false);
    ui.click("compact-button");
    check(&ui, true);
    assert_eq!(ui.component().get_mode(), "compact");
    assert_eq!(ui.component().get_mode_at_action(), "compact");
    assert_eq!(ui.component().get_last_command(), "compact");
    assert_eq!(ui.component().get_mode_activations(), 1);
    assert!(ui.menu_shortcut("Primary+2".parse().unwrap()));
    check(&ui, true);
    assert_eq!(ui.component().get_mode_activations(), 2);

    ui.click("modes-menu");
    assert!(ui.click_menu_row("详细"));
    check(&ui, false);
    assert_eq!(ui.component().get_mode_at_action(), "detail");
    assert_eq!(ui.component().get_mode_activations(), 3);
    ui.click("refresh-button");
    check(&ui, false);
    assert_eq!(ui.component().get_last_command(), "refresh-command");
    assert_eq!(ui.component().get_mode_activations(), 4);

    ui.update(|component| component.set_mode("compact".into()));
    check(&ui, true);
    assert_eq!(ui.component().get_mode_activations(), 4);
    let stale = ui.scene().clone();
    let compact_path = [0, 2, 1];
    assert_eq!(
        stale.at_path(&compact_path).unwrap().id,
        Some("compact-button")
    );
    ui.update(|component| {
        component.set_modes_enabled(false);
        assert!(!component.control_at(&stale, &compact_path, ControlAction::ActivateAction));
    });
    assert!(!ui.can_hit("compact-button"));
    assert!(!ui.menu_shortcut("Primary+1".parse().unwrap()));
    check(&ui, true);
    ui.update(|component| {
        component.set_modes_enabled(true);
        component.set_compact_enabled(false);
    });
    assert!(!ui.can_hit("compact-button"));
    assert!(ui.menu_shortcut("Primary+1".parse().unwrap()));
    check(&ui, false);
    assert_eq!(ui.component().get_mode_activations(), 5);

    ui.click("grid-button");
    ui.click("guides-button");
    assert!(ui.component().get_show_grid() && ui.component().get_show_guides());
    ui.click("grid-button");
    assert!(!ui.component().get_show_grid() && ui.component().get_show_guides());
    assert_eq!(ui.component().get_overlay_activations(), 3);
    assert_eq!(ui.component().get_last_command(), "grid");
    check(&ui, false);
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

#[test]
fn styled_text_links_share_pointer_keyboard_and_stale_document_guards() {
    use layout_ui::StyledTextDemo;
    use xrack_runtime::StyledText;

    let mut ui = StyledTextDemo::new();
    let document = "Before [first link](#first) after\n\n[second link](#second)";
    ui.set_body(StyledText::from_markdown(document).unwrap());
    let mut input = Interaction::default();
    let frame = |ui: &StyledTextDemo| {
        let scene = ui.render_shared();
        let layout = xrack_layout::layout(&scene, Size::new(460.0, 268.0), &mut FixedText);
        (scene, layout)
    };
    let path = [1];
    let (scene, layout) = frame(&ui);
    let first = layout
        .at_path(&path)
        .unwrap()
        .text_link_regions()
        .next()
        .unwrap()
        .1;
    let point = Point {
        x: first.x + first.width * 0.5,
        y: first.y + first.height * 0.5,
    };
    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    let action = input.control_part_release(&scene, &layout).unwrap();
    assert_eq!(action.action, ControlAction::ActivateTextLink(0));
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_last_link(), "#first");

    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().text_link_index(), Some(1));
    input.key_press_with_scene(&scene, ActivationKey::Enter);
    let target = input
        .key_release_with_scene(&scene, ActivationKey::Enter)
        .unwrap();
    let action = input
        .text_link_activation(&scene, &layout, &target)
        .unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_last_link(), "#second");

    ui.set_compact(true);
    let (scene, layout) = frame(&ui);
    input.reconcile(&scene, &layout);
    assert!(input.text_link_target(&scene, &layout, &path, 1).is_none());
    assert!(
        input
            .text_link_activation(&scene, &layout, &target)
            .is_none()
    );

    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    ui.set_body(StyledText::from_markdown(&document.replace("#first", "#different")).unwrap());
    let (scene, layout) = frame(&ui);
    input.reconcile(&scene, &layout);
    input.pointer_move(&scene, &layout, point);
    assert!(input.control_part_release(&scene, &layout).is_none());
    assert_eq!(ui.get_last_link(), "#second");

    ui.set_links_enabled(false);
    let (scene, layout) = frame(&ui);
    input.reconcile(&scene, &layout);
    input.pointer_move(&scene, &layout, point);
    assert!(input.hovered.is_none());
    input.focus_next(&scene, &layout, false);
    assert_eq!(input.focused.as_ref().unwrap().text_link_index(), None);
}

#[test]
fn sibling_stacking_controls_pointer_order_without_changing_tab_order() {
    use layout_ui::CompositingDemo;

    let mut ui = UiDriver::new(CompositingDemo::new(), FixedText, Size::new(460.0, 292.0));
    ui.tab();
    assert_eq!(ui.focused(), Some("first"));
    ui.tab();
    assert_eq!(ui.focused(), Some("second"));

    assert!(ui.can_hit("first"));
    assert!(!ui.can_hit("second"));
    ui.click("first");
    assert_eq!(ui.component().get_first_clicks(), 1);
    ui.click("toggle-first");
    assert_eq!(ui.node("first").opacity(), 0.0);
    ui.click("first");
    assert_eq!(ui.component().get_first_clicks(), 2);

    ui.click("swap-order");
    assert!(!ui.can_hit("first"));
    ui.click("second");
    assert_eq!(ui.component().get_second_clicks(), 1);

    let mut tied = CompositingDemo::new();
    tied.set_first_layer(1.0);
    let mut tied = UiDriver::new(tied, FixedText, Size::new(460.0, 292.0));
    tied.click("second");
    assert_eq!(
        tied.component().get_second_clicks(),
        1,
        "declaration-order tie break"
    );
}

#[test]
fn slider_previews_snap_and_commit_only_on_release() {
    use xrack_layout::SliderGeometry;

    let mut ui = SnapSlidersDemo::new();
    let viewport = Size::new(500.0, 290.0);
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, viewport, &mut FixedText);
    let path = [0, 3];
    let geometry = SliderGeometry::new(
        layout.at_path(&path).unwrap().bounds,
        Orientation::Horizontal,
    );
    let mut input = Interaction::default();
    input.pointer_move(&scene, &layout, geometry.position(0.2));
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.slider_press(&scene, &layout).is_none());
    input.pointer_move(&scene, &layout, geometry.position(0.46));
    assert!(input.slider_drag(&scene, &layout).is_none());
    let draft = input.slider_preview(&path).unwrap();
    assert!((draft - 0.46).abs() < 0.001);
    assert_eq!(ui.get_level(), 20.0);
    assert_eq!(ui.get_level_changes(), 0);
    let commit = input.slider_release(&scene, &layout).unwrap();
    assert!(matches!(commit.action, RangeAction::CommitFraction(_)));
    assert!(ui.adjust_range_at(&scene, &commit.path, commit.action));
    assert_eq!(ui.get_level(), 50.0);
    assert_eq!(ui.get_level_changes(), 1);
    assert_eq!(ui.get_level_releases(), 1);
    assert_eq!(ui.get_released_level(), 50.0);
    assert!(input.slider_preview(&path).is_none());

    let scene = ui.render_shared();
    input.pointer_move(&scene, &layout, geometry.position(0.85));
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.slider_press(&scene, &layout).is_none());
    assert!(input.cancel_slider_preview());
    assert!(input.slider_release(&scene, &layout).is_none());
    assert_eq!(ui.get_level(), 50.0);
}

#[test]
fn range_slider_snaps_on_release_and_clamps_accessible_values() {
    use xrack_layout::SliderGeometry;

    let mut ui = SnapSlidersDemo::new();
    ui.set_live_update(true);
    let scene = ui.render_shared();
    let layout = xrack_layout::layout(&scene, Size::new(500.0, 290.0), &mut FixedText);
    let path = [0, 5];
    let geometry = SliderGeometry::new(
        layout.at_path(&path).unwrap().bounds,
        Orientation::Horizontal,
    );
    let mut input = Interaction::default();
    input.pointer_move(&scene, &layout, geometry.position(0.2));
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.slider_press(&scene, &layout).is_none());
    input.pointer_move(&scene, &layout, geometry.position(0.46));
    let drag = input.slider_drag(&scene, &layout).unwrap();
    assert!(ui.adjust_range_at(&scene, &drag.path, drag.action));
    assert!((ui.get_lower() - 46.0).abs() < 0.001);
    let scene = ui.render_shared();
    let commit = input.slider_release(&scene, &layout).unwrap();
    assert!(ui.adjust_range_at(&scene, &commit.path, commit.action));
    assert_eq!((ui.get_lower(), ui.get_upper()), (50.0, 80.0));
    assert_eq!(ui.get_released_lower(), 50.0);
    assert_eq!(ui.get_band_changes(), 2);

    let scene = ui.render_shared();
    assert!(ui.adjust_range_at(&scene, &[0, 5, 1], RangeAction::SetValue(27.0)));
    assert_eq!((ui.get_lower(), ui.get_upper()), (50.0, 50.0));
    assert_eq!(ui.get_band_changes(), 3);
}

#[test]
fn table_commands_keep_the_current_cell_independent_from_the_selection() {
    let mut component = StandardTableDemo::new();
    component.set_inventory(
        (0..3)
            .map(|units| InventoryRow {
                name: format!("Device {units}"),
                units,
                available: true,
                refreshed: Time::default(),
            })
            .collect(),
    );
    component.set_target(SelectionTarget::Cells);
    component.set_selection_tools(true);
    let mut ui = UiDriver::new(component, FixedText, Size::new(620.0, 380.0));
    assert_eq!(ui.component().get_selected_items(), 0);
    assert!(!ui.component().get_has_selection());
    ui.click("select-block");
    assert_eq!(ui.component().get_selected_items(), 4);
    assert!(ui.component().get_has_selection());
    assert_eq!(
        (
            ui.component().get_current(),
            ui.component().get_current_column()
        ),
        (1, 1)
    );
    assert_eq!(ui.component().get_cell_changes(), 1);
    let selection = ui.component().table_selection("inventory-table").unwrap();
    let key = ui.component().model_inventory().row_key(0).unwrap();
    assert!(selection.contains_cell(key, 0));
    ui.click("move-cursor");
    assert_eq!(
        (
            ui.component().get_current(),
            ui.component().get_current_column()
        ),
        (2, 2)
    );
    assert_eq!(ui.component().get_selected_items(), 4);
    assert_eq!(ui.component().get_cell_changes(), 2);
    ui.update(|view| view.invoke_move_visible_column(0, 3));
    assert!(
        ui.component()
            .table_selection("inventory-table")
            .unwrap()
            .contains_cell(key, 0)
    );
    ui.click("clear-selection");
    assert_eq!(ui.component().get_selected_items(), 0);
    assert!(!ui.component().get_has_selection());
    assert_eq!(
        (
            ui.component().get_current(),
            ui.component().get_current_column()
        ),
        (2, 2)
    );
    assert_eq!(ui.component().get_cell_changes(), 2);

    ui.update(|view| {
        assert!(view.select_table(
            "inventory-table",
            TableSelectionAction::Cell {
                row: 0,
                column: 3,
                operation: RowSelection::Replace,
            }
        ));
        assert!(view.model_inventory().sort_by(|a, b| b.units.cmp(&a.units)));
    });
    assert!(
        ui.component()
            .table_selection("inventory-table")
            .unwrap()
            .contains_cell(key, 3)
    );
    ui.update(|view| {
        assert_eq!(view.model_inventory().remove(2).unwrap().units, 0);
    });
    assert_eq!(ui.component().get_selected_items(), 0);
}

#[test]
fn touch_area_model_preserves_contact_rows_and_invalidates_only_the_moved_row() {
    use layout_ui::PointerDemo;
    use std::cell::Cell;
    use xrack_runtime::{Length, ListModel, PointerEvent, PointerEventKind};

    let mut ui = PointerDemo::new();
    let model = ui.model_touches();
    assert_eq!(model.row_count(), 0);
    let input = Interaction::default();
    let (scene, layout) = prepare_scene_with_input(
        &ui,
        ui.render_shared(),
        Size::new(420.0, 270.0),
        &mut FixedText,
        &input,
    );
    let area = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let point = Point {
        x: area.x + 70.0,
        y: area.y + 40.0,
    };
    let now = Instant::now();
    let begin = |touch_finger_id| {
        input
            .begin_pointer_sequence(
                &scene,
                &layout,
                point,
                PointerEvent {
                    is_touch: true,
                    touch_finger_id,
                    ..Default::default()
                },
                now,
            )
            .unwrap()
    };
    let (mut first, down) = begin(41);
    assert!(down.apply(&mut ui, &scene));
    let (mut second, down) = begin(42);
    assert!(down.apply(&mut ui, &scene));
    assert_eq!(ui.get_active_touches(), 2);
    assert_eq!(ui.get_last_finger_id(), 42);
    assert!(model == ui.model_touches());
    let keys = [model.row_key(0), model.row_key(1)];
    assert_ne!(keys[0], keys[1]);
    assert_eq!(model.item(0).id, 41);
    assert_eq!(model.item(1).id, 42);
    let reads = Cell::new(0);
    let cached_x = Property::default();
    let first_x = || {
        reads.set(reads.get() + 1);
        model.item(0).x
    };
    assert_eq!(cached_x.get(first_x), Length(70.0));
    let outside = Point {
        x: area.x + area.width + 4.0,
        y: point.y,
    };
    let motion = second
        .sample(
            &input,
            &scene,
            &layout,
            outside,
            PointerEventKind::Move,
            Default::default(),
        )
        .unwrap();
    assert!(motion.apply(&mut ui, &scene));
    assert_eq!(ui.get_last_finger_id(), 42);
    assert!(!ui.get_inside() && ui.get_contains_press());
    assert_eq!(model.row_key(1), keys[1]);
    let moved = model.item(1);
    assert_eq!(moved.x, Length(f64::from(area.width + 4.0)));
    assert_eq!((moved.start_x, moved.start_y), (Length(70.0), Length(40.0)));
    assert!(!moved.inside);
    assert_eq!(cached_x.get(first_x), Length(70.0));
    assert_eq!(reads.get(), 1);
    let up = first
        .sample(
            &input,
            &scene,
            &layout,
            point,
            PointerEventKind::Up,
            Default::default(),
        )
        .unwrap();
    assert!(up.apply(&mut ui, &scene));
    assert_eq!(ui.get_active_touches(), 1);
    assert!(ui.get_pressed() && !ui.get_contains_press());
    assert_eq!(model.row_key(0), keys[1]);
    assert_eq!(model.index_of_key(keys[0]), None);
    assert!(second.cancel(Default::default()).apply(&mut ui, &scene));
    assert_eq!(ui.get_active_touches(), 0);
    assert_eq!(ui.get_last_finger_id(), 42);
    assert!(model.snapshot().is_empty());
    assert!(!ui.get_pressed());
}

#[test]
fn touch_area_cancels_pending_long_press_after_motion_or_early_release() {
    use layout_ui::PointerDemo;
    use xrack_runtime::{PointerEvent, PointerEventKind};

    let mut ui = PointerDemo::new();
    let input = Interaction::default();
    let size = Size::new(420.0, 270.0);
    ui.set_hold_interval(120);
    let (scene, layout) =
        prepare_scene_with_input(&ui, ui.render_shared(), size, &mut FixedText, &input);
    let area = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let start = Point {
        x: area.x + 25.0,
        y: area.y + 20.0,
    };
    let now = Instant::now();
    let deadline = now + Duration::from_millis(120);
    for moved in [true, false] {
        let (mut sequence, down) = input
            .begin_pointer_sequence(&scene, &layout, start, PointerEvent::default(), now)
            .unwrap();
        assert!(down.apply(&mut ui, &scene));
        assert_eq!(sequence.press_and_hold_deadline(), Some(deadline));
        if moved {
            for point in [
                Point {
                    x: start.x + 7.0,
                    ..start
                },
                start,
            ] {
                let motion = sequence
                    .sample(
                        &input,
                        &scene,
                        &layout,
                        point,
                        PointerEventKind::Move,
                        Default::default(),
                    )
                    .unwrap();
                assert!(motion.apply(&mut ui, &scene));
            }
            assert!(sequence.press_and_hold_deadline().is_none());
            assert!(!sequence.advance_press_and_hold(&input, &scene, &layout, &mut ui, deadline));
        }
        let up = sequence
            .sample(
                &input,
                &scene,
                &layout,
                start,
                PointerEventKind::Up,
                Default::default(),
            )
            .unwrap();
        assert!(up.apply(&mut ui, &scene));
        assert!(sequence.press_and_hold_deadline().is_none());
        assert!(!sequence.advance_press_and_hold(&input, &scene, &layout, &mut ui, deadline));
    }
    assert_eq!(ui.get_holds(), 0);

    ui.set_hold_interval(0);
    let (scene, layout) =
        prepare_scene_with_input(&ui, ui.render_shared(), size, &mut FixedText, &input);
    let (sequence, down) = input
        .begin_pointer_sequence(&scene, &layout, start, PointerEvent::default(), now)
        .unwrap();
    assert!(down.apply(&mut ui, &scene));
    assert!(sequence.press_and_hold_deadline().is_none());
    assert!(sequence.cancel(Default::default()).apply(&mut ui, &scene));
}

#[test]
fn touch_area_keeps_pointer_coordinates_and_cancels_when_disabled() {
    use layout_ui::PointerDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::{KeyModifiers, Length, PointerButton, PointerEvent, PointerEventKind};

    let mut ui = PointerDemo::new();
    let input = Interaction::default();
    let prepare = |ui: &PointerDemo| {
        prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(420.0, 270.0),
            &mut FixedText,
            &input,
        )
    };
    let (scene, layout) = prepare(&ui);
    let bounds = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let point = Point {
        x: bounds.x + 20.0,
        y: bounds.y + 15.0,
    };
    let mut hover = HoverObservers::default();
    assert!(!ui.get_has_hover() && !ui.get_pressed());
    hover.sync(&input, &scene, &layout, Some(point));
    assert!(ui.get_has_hover());
    assert_eq!(ui.get_mouse_x(), Length(20.0));
    assert_eq!(ui.get_mouse_y(), Length(15.0));
    assert_eq!(ui.get_pressed_x(), Length::default());
    let modifiers = KeyModifiers {
        shift: true,
        ..Default::default()
    };
    let event = PointerEvent {
        button: PointerButton::Secondary,
        modifiers,
        ..Default::default()
    };
    let (mut sequence, down) = input
        .begin_pointer_sequence(&scene, &layout, point, event, Instant::now())
        .unwrap();
    assert!(down.apply(&mut ui, &scene));
    assert_eq!(ui.get_phase(), PointerEventKind::Down);
    assert_eq!(ui.get_last_button(), PointerButton::Secondary);
    assert_eq!(ui.get_pointer_x(), Length(20.0));
    assert_eq!(ui.get_pointer_y(), Length(15.0));
    assert!(ui.get_inside() && ui.get_shift_held());
    assert!(!ui.get_touch());
    assert!(ui.get_pressed() && ui.get_contains_press() && ui.get_pressed_in_event());
    assert_eq!(ui.get_pressed_x(), Length(20.0));
    assert_eq!(ui.get_pressed_y(), Length(15.0));

    let outside = Point {
        x: bounds.x + bounds.width + 9.0,
        y: point.y,
    };
    hover.sync(&input, &scene, &layout, Some(outside));
    assert!(!ui.get_has_hover());
    assert_eq!(ui.get_mouse_x(), Length(20.0));
    for kind in [PointerEventKind::Move, PointerEventKind::Up] {
        let delivery = sequence
            .sample(&input, &scene, &layout, outside, kind, Default::default())
            .unwrap();
        assert!(delivery.apply(&mut ui, &scene));
        assert_eq!(ui.get_phase(), kind);
        assert!(!ui.get_inside() && !ui.get_shift_held());
        assert_eq!(ui.get_pointer_x(), Length(f64::from(bounds.width + 9.0)));
        assert_eq!(ui.get_mouse_x(), ui.get_pointer_x());
        assert_eq!(ui.get_pressed(), kind == PointerEventKind::Move);
        assert_eq!(ui.get_pressed_in_event(), kind == PointerEventKind::Move);
        assert!(!ui.get_contains_press());
        assert_eq!(ui.get_pressed_x(), Length(20.0));
    }
    assert_eq!(ui.get_moves(), 1);
    assert_eq!(ui.get_clicks(), 0);

    let (sequence, down) = input
        .begin_pointer_sequence(&scene, &layout, point, event, Instant::now())
        .unwrap();
    assert!(down.apply(&mut ui, &scene));
    hover.sync(&input, &scene, &layout, Some(point));
    ui.set_area_enabled(false);
    let (disabled, disabled_layout) = prepare(&ui);
    hover.sync(&input, &disabled, &disabled_layout, Some(point));
    assert!(!ui.get_has_hover());
    assert!(!sequence.is_live(&input, &disabled, &disabled_layout));
    assert!(sequence.cancel(modifiers).apply(&mut ui, &disabled));
    assert_eq!(ui.get_phase(), PointerEventKind::Cancel);
    assert!(!ui.get_pressed() && !ui.get_pressed_in_event() && !ui.get_contains_press());
    assert!(!ui.get_inside());
    assert_eq!(ui.get_events(), 5);
    assert_eq!(ui.get_clicks(), 0);

    let (sequence, _) = input
        .begin_pointer_sequence(&scene, &layout, point, event, Instant::now())
        .unwrap();
    let mut replacement = scene.as_ref().clone();
    replacement.children[1].children[0].children[0].pointer_handle = Some(Default::default());
    assert!(!sequence.cancel(modifiers).apply(&mut ui, &replacement));
    assert_eq!(ui.get_events(), 5);
    ui.set_area_enabled(true);
    let (enabled, _) = prepare(&ui);
    assert!(ui.dispatch_at(&enabled, &[1, 0, 0], "double-clicked"));
    assert_eq!(ui.get_double_clicks(), 1);
}

#[test]
fn clearing_a_single_row_selection_keeps_the_row_cursor() {
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..3)
            .map(|units| InventoryRow {
                name: units.to_string(),
                units,
                available: true,
                refreshed: Time::default(),
            })
            .collect(),
    );
    ui.set_target(SelectionTarget::Rows);
    ui.set_multiple(false);
    ui.set_current(1);
    assert_eq!(ui.table_selection("inventory-table").unwrap().len(), 1);
    assert!(
        ui.render_shared()
            .find("inventory-table")
            .unwrap()
            .table_selection
            .is_none()
    );
    assert!(ui.select_table("inventory-table", TableSelectionAction::Clear));
    assert_eq!(ui.get_current(), 1);
    assert!(ui.table_selection("inventory-table").unwrap().is_empty());
    let scene = ui.render_shared();
    assert!(
        scene
            .find("inventory-table")
            .unwrap()
            .table_selection
            .as_ref()
            .unwrap()
            .is_empty()
    );
}
