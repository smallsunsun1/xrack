#![cfg(test)]

use std::time::{Duration, Instant};

use mobile_table_ui::MobileTable;
use xrack_interpreter::load_mlir;
use xrack_layout::{
    EditAction, EditSource, FocusObservers, Interaction, ScrollGestureKind, TableEditCommit,
    TextPosition, TreeRowGeometry,
};
use xrack_runtime::{Component, Orientation, TextCommand};
use xrack_testing::{Point, Size, UiDriver};

fn exercise(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    let path = ui.path("inventory");
    let table = ui.layout_at(&path).unwrap().table.as_ref().unwrap();
    assert!(table.is_frozen(0));
    let start = Point {
        x: table.columns[0].x + 22.0,
        y: table.body.y + 22.0,
    };
    let end = Point {
        x: table.columns[1].x + 22.0,
        y: start.y + 44.0,
    };
    let mut input = Interaction::default();
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        now,
        ScrollGestureKind::Touch
    ));
    let hold = input.table_touch_hold_deadline().unwrap();
    assert_eq!(hold.duration_since(now), Duration::from_millis(500));
    assert!(input.advance_table_touch_hold(ui.scene(), ui.layout(), hold));
    assert!(input.move_scroll_gesture(ui.scene(), ui.layout(), end, hold));
    assert!(input.touch_table_selection_claimed());
    assert_eq!(ui.node("selection-label").text("text"), Some("Cells 0"));
    let release = input.end_scroll_gesture(ui.scene(), ui.layout(), end, hold);
    assert!(!release.tap && !release.scrolled);
    let action = release.control.unwrap();
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    assert_eq!(ui.node("selection-label").text("text"), Some("Cells 4"));
    assert_eq!(
        ui.component()
            .copy_table_at(ui.scene(), &path, &[0, 1, 2, 3])
            .as_deref(),
        Some("Pump\t12\nValve\t8"),
    );

    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: end.x,
            y: start.y,
        },
    );
    input.pointer_press_in_scene(ui.scene(), ui.layout());
    assert!(input.begin_table_edit(ui.scene(), ui.layout()));
    let scene = ui.scene().clone();
    ui.update(|app| {
        assert!(input.edit_text(
            app,
            &scene,
            EditAction::ReplaceAll("invalid"),
            EditSource::Atomic,
            now
        ));
        assert_eq!(input.finish_table_edit(app, &scene), TableEditCommit::Kept);
    });
    assert_eq!(ui.node("units-label").text("text"), Some("Pump units 12"));
    let scene = ui.scene().clone();
    ui.update(|app| {
        assert!(input.edit_text(
            app,
            &scene,
            EditAction::ReplaceAll("47"),
            EditSource::Atomic,
            now
        ));
        assert_eq!(
            input.finish_table_edit(app, &scene),
            TableEditCommit::Changed
        );
    });
    assert_eq!(ui.node("units-label").text("text"), Some("Pump units 47"));
}

#[test]
fn mobile_table_preserves_touch_selection_tsv_and_typed_editing()
-> Result<(), Box<dyn std::error::Error>> {
    exercise(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn directory_row(ui: &UiDriver<impl Component>, key: i64) -> Vec<usize> {
    let directory = ui.node("directory");
    let row = directory.tree.as_ref().unwrap().visible_index(key).unwrap();
    let mut path = ui.path("directory");
    path.push(row - directory.list.unwrap().first_row);
    path
}

fn toggle_directory_branch(ui: &mut UiDriver<impl Component>, key: i64, rtl: bool) {
    let path = directory_row(ui, key);
    let scene = ui.scene().clone();
    let row = scene.at_path(&path).unwrap();
    let item = ui.layout_at(&path).unwrap();
    let geometry = TreeRowGeometry::for_layout(row, item).unwrap();
    assert_eq!(geometry.indicator.x > geometry.content.x, rtl);
    let indicator = geometry.indicator;
    let point = item.map_to_window(Point {
        x: indicator.x + indicator.width * 0.5,
        y: indicator.y + indicator.height * 0.5,
    });
    let mut input = Interaction::default();
    input.pointer_move(&scene, ui.layout(), point);
    input.pointer_press_in_scene(&scene, ui.layout());
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, path);
    let change = input
        .tree_pointer_action(&scene, ui.layout(), &target)
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &change.path, change.action)));
}

fn exercise_directory(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-tree");
    let rows = |ui: &UiDriver<_>| {
        ui.node("directory")
            .tree
            .as_ref()
            .unwrap()
            .rows()
            .iter()
            .map(|row| row.key)
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&ui), [10, 30]);
    toggle_directory_branch(&mut ui, 10, false);
    toggle_directory_branch(&mut ui, 20, false);
    assert_eq!(rows(&ui), [10, 20, 21, 22, 30]);
    ui.click_at_path(&directory_row(&ui, 21));
    assert_eq!(ui.node("tree-selection").text("text"), Some("Selected 21"));
    assert_eq!(ui.node("tree-cursor").text("text"), Some("Cursor 21"));

    toggle_directory_branch(&mut ui, 10, false);
    assert_eq!(rows(&ui), [10, 30]);
    assert_eq!(ui.node("tree-cursor").text("text"), Some("Cursor 10"));
    toggle_directory_branch(&mut ui, 10, false);
    assert_eq!(rows(&ui), [10, 20, 21, 22, 30]);
    ui.click("filter-archive");
    assert_eq!(rows(&ui), [30, 31]);
    assert_eq!(ui.node("tree-selection").text("text"), Some("Selected 21"));
    ui.click("filter-archive");
    assert_eq!(rows(&ui), [10, 20, 21, 22, 30]);
    ui.click("insert-report");
    assert_eq!(rows(&ui), [10, 20, 23, 21, 22, 30]);
    ui.click("mirror-tree");
    toggle_directory_branch(&mut ui, 10, true);
    assert_eq!(rows(&ui), [10, 30]);
    toggle_directory_branch(&mut ui, 10, true);
    assert_eq!(rows(&ui), [10, 20, 23, 21, 22, 30]);
    for (id, label) in [
        ("tree-selection", "Selected 21"),
        ("tree-expanded", "Expanded 4"),
        ("tree-collapsed", "Collapsed 2"),
        ("tree-changes", "Changes 1"),
    ] {
        assert_eq!(ui.node(id).text("text"), Some(label), "{id}");
    }
}

#[test]
fn mobile_directory_preserves_selected_key_through_projection_changes()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_directory(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_directory(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn split_drag(ui: &mut UiDriver<impl Component>, id: &str, delta: Point, rtl: bool) {
    let path = ui.path(id);
    let item = ui.layout_at(&path).unwrap();
    let geometry = item.split.as_ref().unwrap();
    let handle = geometry.handles[0].bounds;
    let mut start = Point {
        x: handle.x + handle.width / 2.0,
        y: handle.y + handle.height / 2.0,
    };
    if geometry.orientation == Orientation::Horizontal {
        start.x += if rtl { -12.0 } else { 12.0 };
        start.y -= 76.0;
    } else {
        start.y += 11.0;
    }
    let start = item.map_to_window(start);
    let mut input = Interaction::default();
    assert!(input.touch_split_press(ui.scene(), ui.layout(), start));
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: start.x + delta.x,
            y: start.y + delta.y,
        },
    );
    let update = input.split_drag(ui.scene(), ui.layout()).unwrap();
    let scene = ui.scene().clone();
    ui.update(|component| {
        assert!(component.resize_split_at(&scene, &update.path, &update.weights));
    });
    assert!(input.finish_split_resize());
    assert!(!input.split_resize_active());
}

fn exercise_split(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-split");
    let assert_size = |ui: &UiDriver<_>, wide, high| {
        assert_eq!(ui.node("split-width").text("text"), Some(wide));
        assert_eq!(ui.node("split-height").text("text"), Some(high));
    };
    assert_size(&ui, "Browse width 112", "Edit height 120");
    split_drag(&mut ui, "columns", Point { x: 90.0, y: 28.0 }, false);
    assert_size(&ui, "Browse width 166", "Edit height 120");
    ui.click("save-split");
    split_drag(&mut ui, "rows", Point { x: -20.0, y: -90.0 }, false);
    assert_size(&ui, "Browse width 166", "Edit height 64");
    split_drag(&mut ui, "columns", Point { x: -112.0, y: 0.0 }, false);
    assert_size(&ui, "Browse width 80", "Edit height 64");
    ui.click("restore-split");
    assert_size(&ui, "Browse width 166", "Edit height 120");
    ui.click("mirror-split");
    assert!(ui.bounds("browse").x > ui.bounds("edit-pane").x);
    split_drag(&mut ui, "columns", Point { x: 112.0, y: 0.0 }, true);
    assert_size(&ui, "Browse width 80", "Edit height 120");
    ui.click("restore-split");
    assert_size(&ui, "Browse width 166", "Edit height 120");
}

#[test]
fn mobile_split_touches_bound_and_restore_both_axes() -> Result<(), Box<dyn std::error::Error>> {
    exercise_split(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_split(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_transfer(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-transfer");
    ui.click("drag-card");
    let assert_label = |ui: &UiDriver<_>, id, label| {
        assert_eq!(ui.node(id).text("text"), Some(label), "{id}");
    };
    assert_label(&ui, "transfer-taps", "Taps 1");
    assert_label(&ui, "transfer-starts", "Starts 0");

    for (target, hover, canceled, result, previous_drops, drops) in [
        (
            "other-bay",
            "No target",
            false,
            "Transfer canceled",
            "Drops 0",
            "Drops 0",
        ),
        (
            "copy-bay",
            "Over copy",
            false,
            "Copied Card A",
            "Drops 0",
            "Drops 1",
        ),
        (
            "archive-bay",
            "Over archive",
            true,
            "Transfer canceled",
            "Drops 1",
            "Drops 1",
        ),
        (
            "archive-bay",
            "Over archive",
            false,
            "Archived Card A",
            "Drops 1",
            "Drops 2",
        ),
    ] {
        let point = |id| {
            let item = ui.layout_at(&ui.path(id)).unwrap();
            let bounds = item.bounds;
            item.map_to_window(Point {
                x: bounds.x + bounds.width / 2.0,
                y: bounds.y + bounds.height / 2.0,
            })
        };
        let start = point("drag-card");
        let end = point(target);
        let mut input = Interaction::default();
        input.arm_drag(ui.scene(), ui.layout(), start);
        let drag = input.drag_motion(ui.scene(), ui.layout(), end, None);
        assert!(drag.captured);
        let scene = ui.scene().clone();
        ui.update(|component| {
            for event in drag.events {
                event.apply(component, &scene);
            }
        });
        assert_label(&ui, "transfer-hover", hover);
        assert_label(&ui, "transfer-drops", previous_drops);

        let finish = if canceled {
            input.cancel_drag()
        } else {
            input.finish_drag(ui.scene(), ui.layout(), end, None)
        };
        assert!(finish.captured);
        let scene = ui.scene().clone();
        ui.update(|component| {
            for event in finish.events {
                assert!(event.apply(component, &scene));
            }
        });
        assert_label(&ui, "transfer-result", result);
        assert_label(&ui, "transfer-hover", "No target");
        assert_label(&ui, "transfer-drops", drops);
    }
    assert_label(&ui, "transfer-taps", "Taps 1");
    assert_label(&ui, "transfer-starts", "Starts 4");
    assert_label(&ui, "transfer-finishes", "Finishes 4");
    assert_label(&ui, "transfer-cancels", "Cancels 2");
    assert!(
        !ui.layout_at(&ui.path("drag-card"))
            .is_some_and(|item| item.visible)
    );
}

#[test]
fn mobile_transfer_preserves_drop_order_and_cancels_without_moving_source()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_transfer(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_transfer(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_canvas(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-canvas");
    let assert_offset = |ui: &UiDriver<_>, x, y| {
        assert_eq!(ui.node("map-x").text("text"), Some(x));
        assert_eq!(ui.node("map-y").text("text"), Some(y));
    };
    assert_offset(&ui, "Offset X 0", "Offset Y 0");
    let horizontal = ui.scroll_bar("map-horizontal").unwrap().thumb;
    let vertical = ui.scroll_bar("map-vertical").unwrap().thumb;
    let pan = |ui: &mut UiDriver<_>| {
        let item = ui.layout_at(&ui.path("map-tile")).unwrap();
        let bounds = item.bounds;
        let start = item.map_to_window(Point {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        });
        let end = Point {
            x: start.x - 72.0,
            y: start.y - 52.0,
        };
        let mut input = Interaction::default();
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(
            ui.scene(),
            ui.layout(),
            start,
            now,
            ScrollGestureKind::Touch
        ));
        input.arm_drag(ui.scene(), ui.layout(), start);
        assert!(
            !input
                .drag_motion(ui.scene(), ui.layout(), end, None)
                .captured
        );
        let moved = input.move_scroll_gesture(
            ui.scene(),
            ui.layout(),
            end,
            now + Duration::from_millis(20),
        );
        let outcome = input.end_scroll_gesture(
            ui.scene(),
            ui.layout(),
            end,
            now + Duration::from_millis(180),
        );
        assert!(!outcome.tap);
        ui.update(|_| {});
        moved
    };
    ui.click("map-tile");
    assert!(pan(&mut ui));
    assert_offset(&ui, "Offset X 66", "Offset Y 47");
    assert!(ui.scroll_bar("map-horizontal").unwrap().thumb.x > horizontal.x);
    assert!(ui.scroll_bar("map-vertical").unwrap().thumb.y > vertical.y);
    assert_eq!(ui.node("map-taps").text("text"), Some("Taps 1"));

    ui.click("reset-map");
    ui.click("lock-map");
    assert!(!pan(&mut ui));
    assert_offset(&ui, "Offset X 0", "Offset Y 0");
    ui.click("map-tile");
    assert_eq!(ui.node("map-taps").text("text"), Some("Taps 2"));

    assert!(ui.drag_scroll_bar("map-horizontal", 1.0, true));
    assert_offset(&ui, "Offset X 248", "Offset Y 0");
    assert!(ui.drag_scroll_bar("map-vertical", 1.0, true));
    assert_offset(&ui, "Offset X 248", "Offset Y 192");
    ui.click("reset-map");
    assert_offset(&ui, "Offset X 0", "Offset Y 0");
    assert_eq!(ui.scroll_bar("map-horizontal").unwrap().thumb, horizontal);
    assert_eq!(ui.scroll_bar("map-vertical").unwrap().thumb, vertical);
}

#[test]
fn mobile_canvas_keeps_pan_and_attached_scroll_bars_in_sync()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_canvas(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_canvas(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_refresh(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-refresh");
    let feed = ui.path("refresh-feed");
    let item = ui.layout_at(&feed).unwrap();
    assert_eq!(item.scroll.unwrap().max_offset, 0.0);
    let start = Point {
        x: item.bounds.x + item.bounds.width / 2.0,
        y: item.bounds.y + 24.0,
    };
    let pull = |ui: &mut UiDriver<_>, distance, cancel, progress| {
        let mut input = Interaction::default();
        input.set_reduce_motion(true);
        let scene = ui.scene().clone();
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(
            &scene,
            ui.layout(),
            start,
            now,
            ScrollGestureKind::Touch
        ));
        let end = Point {
            y: start.y + distance,
            ..start
        };
        input.move_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(70));
        assert_eq!(
            input.pull_to_refresh_progress(scene.at_path(&feed).unwrap()),
            progress
        );
        assert!(input.take_scroll_events(&scene, ui.layout()).is_empty());
        if cancel {
            input.cancel_scroll_gesture();
        } else {
            input.end_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(200));
        }
        let deliveries = input.take_scroll_events(&scene, ui.layout());
        ui.update(|component| {
            for delivery in &deliveries {
                assert!(delivery.apply(component, &scene));
            }
        });
        deliveries.len()
    };

    assert_eq!(pull(&mut ui, 110.0, true, Some(1.0)), 0);
    assert_eq!(pull(&mut ui, 110.0, false, Some(1.0)), 1);
    assert_eq!(
        ui.node("refresh-status").text("text"),
        Some("Loading readings")
    );
    assert_eq!(pull(&mut ui, 110.0, false, None), 0);
    assert_eq!(ui.node("refresh-requests").text("text"), Some("Requests 1"));
    ui.click("finish-refresh");
    assert_eq!(
        ui.node("refresh-status").text("text"),
        Some("Readings ready")
    );
    assert_eq!(
        ui.node("refresh-feed")
            .at_path(&[0, 0, 0])
            .and_then(|text| text.text("text")),
        Some("Revision 1")
    );
    assert_eq!(pull(&mut ui, 110.0, false, Some(1.0)), 1);
    assert_eq!(ui.node("refresh-requests").text("text"), Some("Requests 2"));
}

#[test]
fn mobile_refresh_uses_the_same_short_list_gesture_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_refresh(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_refresh(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_catalog(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-catalog");
    let grid = ui.node("catalog");
    assert!(
        grid.children.len() < 14,
        "the grid should only materialize visible rows"
    );
    let valve = grid.children[1].row_key.unwrap();
    let valve_row = ui.find_row_path("catalog", valve, None).unwrap();
    let pump_row = ui
        .find_row_path("catalog", grid.children[0].row_key.unwrap(), None)
        .unwrap();
    assert_eq!(
        ui.layout_at(&valve_row).unwrap().bounds.y,
        ui.layout_at(&pump_row).unwrap().bounds.y
    );
    ui.click_at_path(&valve_row);
    let assert_selection = |ui: &UiDriver<_>, index| {
        assert_eq!(ui.node("catalog-key").text("text"), Some("Key 20"));
        assert_eq!(ui.node("catalog-index").text("text"), Some(index));
        assert_eq!(ui.node("catalog-choices").text("text"), Some("Choices 1"));
    };
    assert_selection(&ui, "Index 1");
    let pin = ui
        .find_row_path("catalog", valve, Some("pin-part"))
        .unwrap();
    ui.click_at_path(&pin);
    let assert_pinned = |ui: &UiDriver<_>| {
        let pin = ui
            .find_row_path("catalog", valve, Some("pin-part"))
            .unwrap();
        assert_eq!(
            ui.scene().at_path(&pin).unwrap().text("text"),
            Some("Unpin Valve")
        );
        let row = ui.find_row_path("catalog", valve, None).unwrap();
        assert!(ui.scene().at_path(&row).unwrap().boolean("selected", false));
    };
    assert_pinned(&ui);

    assert!(ui.scroll("catalog", Point { x: 0.0, y: 330.0 }));
    assert!(ui.find_row_path("catalog", valve, None).is_err());
    assert_selection(&ui, "Index 1");
    ui.click("move-valve");
    assert_selection(&ui, "Index 10");
    assert_pinned(&ui);

    ui.click("toggle-valve");
    assert_selection(&ui, "Index -1");
    assert!(ui.find_row_path("catalog", valve, None).is_err());
    ui.click("toggle-valve");
    assert_selection(&ui, "Index 10");
    assert_pinned(&ui);
}

#[test]
fn mobile_catalog_keeps_selection_and_delegate_state_by_record()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_catalog(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_catalog(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_inspection_log(component: impl Component) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-log");
    let path = ui.path("log");
    let view = ui.layout_at(&path).unwrap();
    assert_eq!(view.list_section_headers()[0].label.as_ref(), "Intake");
    assert!(!view.list_section_headers()[0].pinned);
    assert!(ui.scroll("log", Point { x: 0.0, y: 90.0 }));

    let seal = ui
        .node("log")
        .children
        .iter()
        .find(|row| {
            row.at_path(&[0, 0]).and_then(|title| title.text("text")) == Some("Seal intake")
        })
        .unwrap()
        .row_key
        .unwrap();
    let seal_y = |ui: &UiDriver<_>| {
        let row = ui.find_row_path("log", seal, None).unwrap();
        ui.layout_at(&row).unwrap().bounds.y
    };
    let initial_y = seal_y(&ui);
    let assert_anchor = |ui: &UiDriver<_>, count, offset, entries| {
        let view = ui.layout_at(&path).unwrap();
        let headers = view.list_section_headers();
        assert_eq!(headers[0].label.as_ref(), "Intake");
        assert!(headers[0].pinned);
        assert_eq!(headers[0].row_count, count);
        assert_eq!(headers[0].bounds.y, view.bounds.y);
        assert_eq!(headers[1].label.as_ref(), "Bench");
        assert_eq!(headers[1].first_row, count);
        assert_eq!(view.scroll.unwrap().offset, offset);
        assert_eq!(seal_y(ui), initial_y);
        assert_eq!(ui.node("log-count").text("text"), Some(entries));
        assert_eq!(ui.node("log-scrolls").text("text"), Some("Scrolls 1"));
    };
    assert_anchor(&ui, 4, 90.0, "Entries 12");
    ui.click("add-intake");
    assert_anchor(&ui, 5, 134.0, "Entries 13");
    ui.click("filter-bracket");
    assert_anchor(&ui, 4, 90.0, "Entries 12");
    ui.click("filter-bracket");
    assert_anchor(&ui, 5, 134.0, "Entries 13");

    assert!(ui.scroll("log", Point { x: 0.0, y: 100.0 }));
    let view = ui.layout_at(&path).unwrap();
    let headers = view.list_section_headers();
    assert_eq!(headers[0].bounds.y, view.bounds.y - 14.0);
    assert_eq!(
        headers[0].bounds.y + headers[0].bounds.height,
        headers[1].bounds.y
    );
    assert!(!headers[1].pinned);
    assert!(ui.scroll("log", Point { x: 0.0, y: 34.0 }));
    let view = ui.layout_at(&path).unwrap();
    let first = &view.list_section_headers()[0];
    assert_eq!(first.label.as_ref(), "Bench");
    assert_eq!(first.first_row, 5);
    assert!(first.pinned);
    assert_eq!(
        first.content().unwrap().0.children[0].text("text"),
        Some("Bench 4 pinned")
    );
    assert_eq!(ui.node("log-scrolls").text("text"), Some("Scrolls 3"));
}

#[test]
fn mobile_log_preserves_section_anchors_and_hands_off_the_pinned_heading()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_inspection_log(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_inspection_log(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}

fn exercise_bulletin<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(390.0, 740.0));
    ui.click("open-bulletin");
    let path = ui.path("bulletin");
    let view = ui.layout_at(&path).unwrap();
    let points = [0, 1].map(|index| {
        let (_, link) = view.text_link_regions().find(|(i, _)| *i == index).unwrap();
        view.map_to_window(Point {
            x: link.x + link.width / 2.0,
            y: link.y + link.height / 2.0,
        })
    });
    let mut input = Interaction::default();
    let tap = |ui: &mut UiDriver<C>, input: &mut Interaction, point| {
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(
            ui.scene(),
            ui.layout(),
            point,
            now,
            ScrollGestureKind::Touch
        ));
        assert!(
            input
                .end_scroll_gesture(
                    ui.scene(),
                    ui.layout(),
                    point,
                    now + Duration::from_millis(120)
                )
                .tap
        );
        input.pointer_move(ui.scene(), ui.layout(), point);
        input.pointer_press_in_scene(ui.scene(), ui.layout());
        let Some(action) = input.control_part_release(ui.scene(), ui.layout()) else {
            return false;
        };
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        true
    };

    for (point, target) in points
        .into_iter()
        .zip(["Target #procedure", "Target #limits"])
    {
        assert!(tap(&mut ui, &mut input, point));
        assert_eq!(ui.node("bulletin-target").text("text"), Some(target));
    }
    assert_eq!(ui.node("bulletin-links").text("text"), Some("Links 2"));
    ui.click("compact-bulletin");
    assert!(
        ui.layout_at(&path)
            .unwrap()
            .text_link_regions()
            .next()
            .is_none()
    );
    for point in points {
        assert!(!tap(&mut ui, &mut input, point));
    }
    assert_eq!(ui.node("bulletin-links").text("text"), Some("Links 2"));
    ui.click("compact-bulletin");

    let target = input
        .touch_text_target(ui.scene(), ui.layout(), points[0])
        .unwrap();
    assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target.clone()));
    input.sync_editor(ui.scene());
    let text = input.editor_value(ui.node("bulletin")).into_owned();
    input.editor.as_mut().unwrap().place_caret(
        &text,
        TextPosition {
            offset: text.find("Procedure").unwrap() + 2,
            trailing: false,
        },
        false,
    );
    input.apply_text_selection(ui.scene(), TextCommand::SelectWord);
    assert_eq!(
        &text[input.editor.as_ref().unwrap().selection(&text)],
        "Procedure"
    );
    let _ = FocusObservers::default().sync(&input, ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert_eq!(
        ui.node("bulletin-selection").text("text"),
        Some("Selected Procedure")
    );
    assert_eq!(ui.node("bulletin-links").text("text"), Some("Links 2"));
    assert!(input.open_touch_text_menu(ui.scene(), ui.layout(), target, points[0]));
    assert!(input.reconcile_text_menu(ui.scene(), ui.layout()));
    let panels = input.menu_panels(ui.scene(), ui.layout());
    let menu_enabled = |label| {
        panels[0]
            .rows
            .iter()
            .find(|row| row.label == label)
            .unwrap()
            .enabled
    };
    assert!(menu_enabled("Copy"));
    assert!(!menu_enabled("Cut"));
    assert!(!menu_enabled("Paste"));
}

#[test]
fn mobile_bulletin_links_and_word_selection_share_the_document()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_bulletin(MobileTable::new());
    let module = xrack_compiler::compile(include_str!("table.xr"), "table.xr")?;
    exercise_bulletin(load_mlir(&module.mlir)?.create("MobileTable")?);
    Ok(())
}
