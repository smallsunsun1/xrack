#![cfg(test)]

mod navigation_fixture;

use mobile_navigation_ui::MobileNavigation;
use navigation_fixture::interpreted_navigation;
use std::time::{Duration, Instant};
use xrack_interpreter::HostValue;
use xrack_layout::{
    ActivationKey, ChoiceKey, EditAction, EditSource, FocusObservers, Interaction, MenuKey,
    MenuShortcut, PageIndicatorGeometry, ScrollGestureKind, ShortcutOutcome, layout_with_input,
};
use xrack_runtime::{
    CalendarAction, Component, ControlAction, Date, DialogRole, EventResult, KeyEvent, KeyPhase,
    Node, SceneTransition, Tumbler,
};
use xrack_testing::{FixedText, Point, Size, UiDriver};

fn text_path(node: &Node, text: &str) -> Option<Vec<usize>> {
    if node.text("text") == Some(text) {
        return Some(Vec::new());
    }
    node.children.iter().enumerate().find_map(|(slot, child)| {
        let mut path = text_path(child, text)?;
        path.insert(0, slot);
        Some(path)
    })
}

fn click_text<C: Component>(ui: &mut UiDriver<C>, text: &str) {
    let path = text_path(ui.scene(), text).unwrap_or_else(|| panic!("No visible {text:?}"));
    ui.click_at_path(&path);
}

fn require_text<C: Component>(ui: &UiDriver<C>, text: &str) {
    assert!(text_path(ui.scene(), text).is_some(), "No visible {text:?}");
}

fn swipe<C: Component>(ui: &mut UiDriver<C>, start: Point, end: Point) -> bool {
    let scene = ui.scene().clone();
    let mut input = Interaction::default();
    let now = Instant::now();
    if !input.begin_scroll_gesture(&scene, ui.layout(), start, now, ScrollGestureKind::Touch) {
        return false;
    }
    input.move_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(300));
    let event =
        input.end_scroll_gesture(&scene, ui.layout(), end, now + Duration::from_millis(450));
    let Some(action) = event.control else {
        return false;
    };
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    true
}

fn marker<C: Component>(ui: &mut UiDriver<C>, index: usize) {
    let geometry = PageIndicatorGeometry::new(ui.node("dots"), ui.bounds("dots")).unwrap();
    let point = geometry
        .markers()
        .find(|marker| marker.index == index)
        .unwrap()
        .center;
    let scene = ui.scene().clone();
    let mut input = Interaction::default();
    input.pointer_move(&scene, ui.layout(), point);
    input.pointer_press_in_scene(&scene, ui.layout());
    let action = input.control_part_release(&scene, ui.layout()).unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
}

fn exercise_calendar_fields(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-layouts");
    ui.click("open-fields");
    for (field, input, accepted, draft, stored, commits) in [
        (
            "date-entry",
            "20240302",
            false,
            "Date draft 2024-03-02",
            "Stored date 2024-02-28",
            "Date commits 0",
        ),
        (
            "date-entry",
            "20240227",
            true,
            "Date draft 2024-02-27",
            "Stored date 2024-02-27",
            "Date commits 1",
        ),
        (
            "minute-entry",
            "2359",
            false,
            "Time draft 23:59",
            "Stored time 12:30:41",
            "Time commits 0",
        ),
        (
            "minute-entry",
            "1345",
            true,
            "Time draft 13:45",
            "Stored time 13:45:41",
            "Time commits 1",
        ),
    ] {
        ui.click(field);
        ui.fill_focused(input);
        require_text(&ui, draft);
        let path = ui.path(field);
        let scene = ui.scene().clone();
        ui.update(|app| {
            assert_eq!(
                app.control_at(&scene, &path, ControlAction::ValidateTextSubmission),
                accepted
            );
            assert_eq!(app.dispatch_at(&scene, &path, "accepted"), accepted);
        });
        require_text(&ui, stored);
        require_text(&ui, commits);
    }

    ui.click("date-picker-button");
    let date_popup = ui.path("date-picker");
    let mut calendar = date_popup.clone();
    calendar.push(0);
    let scene = ui.scene().clone();
    ui.update(|app| {
        let leap_day = Date::new(2024, 2, 29).unwrap();
        let select = ControlAction::Calendar(CalendarAction::Select(leap_day));
        assert!(app.control_at(&scene, &calendar, select));
    });
    require_text(&ui, "Stored date 2024-02-27");
    let scene = ui.scene().clone();
    ui.update(|app| {
        assert!(app.control_at(
            &scene,
            &date_popup,
            ControlAction::Dialog(DialogRole::Accept)
        ));
    });
    require_text(&ui, "Stored date 2024-02-29");
    require_text(&ui, "Date draft 2024-02-29");
    require_text(&ui, "Date commits 2");

    for (role, stored, draft, commits) in [
        (
            DialogRole::Reject,
            "Stored time 13:45:41",
            "Time draft 13:45",
            "Time commits 1",
        ),
        (
            DialogRole::Accept,
            "Stored time 13:46:41",
            "Time draft 13:46",
            "Time commits 2",
        ),
    ] {
        ui.click("time-picker-button");
        let time_popup = ui.path("time-picker");
        let mut minute = time_popup.clone();
        minute.extend([0, 1]);
        let scene = ui.scene().clone();
        let row = Tumbler::from_node(scene.at_path(&minute).unwrap())
            .row_key(46)
            .unwrap();
        ui.update(|app| {
            assert!(app.control_at(
                &scene,
                &minute,
                ControlAction::PickTumbler { index: 46, row }
            ));
        });
        require_text(&ui, "Stored time 13:45:41");
        let scene = ui.scene().clone();
        ui.update(|app| {
            assert!(app.control_at(&scene, &time_popup, ControlAction::Dialog(role)));
        });
        require_text(&ui, stored);
        require_text(&ui, draft);
        require_text(&ui, commits);
    }
}

#[test]
fn imported_calendar_fields_validate_and_commit_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_calendar_fields(MobileNavigation::new());
    exercise_calendar_fields(interpreted_navigation()?);
    Ok(())
}

fn exercise_document_tabs(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-card-workshop");
    ui.click("open-tab-workshop");
    click_text(&mut ui, "Add Valve");
    require_text(&ui, "Valve total 1");
    let path = ui.path("document-tabs");
    let tabs = ui.layout_at(&path).unwrap();
    let header = |index: usize| {
        let rect = tabs.children[index].tab_header.unwrap();
        Point {
            x: rect.x + rect.width * 0.35,
            y: rect.y + rect.height / 2.0,
        }
    };
    let start = header(1);
    let end = header(2);
    let now = Instant::now();
    let mut interrupted = Interaction::default();
    assert!(interrupted.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        now,
        ScrollGestureKind::Touch,
    ));
    interrupted.move_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(300),
    );
    assert!(interrupted.tab_move_active());
    interrupted.cancel_scroll_gesture();
    assert!(!interrupted.tab_move_active());
    let release = interrupted.end_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(450),
    );
    assert!(!release.tap && release.control.is_none());
    require_text(&ui, "Tab moves 0");
    require_text(&ui, "Valve total 1");
    assert_eq!(
        ui.node("document-tabs").children[1].text("title"),
        Some("Valve")
    );

    assert!(swipe(&mut ui, start, end));
    require_text(&ui, "Active Valve");
    require_text(&ui, "Valve total 1");
    require_text(&ui, "Moved 1 to 2");
    require_text(&ui, "Tab moves 1");
    assert_eq!(
        ui.node("document-tabs").children[2].text("title"),
        Some("Valve")
    );

    for (index, title, active, count) in [(1, "Meter", "Valve", 2), (1, "Valve", "Pump", 1)] {
        let close = ui.layout_at(&path).unwrap().children[index]
            .tab_close
            .unwrap();
        let mut input = Interaction::default();
        input.pointer_move(
            ui.scene(),
            ui.layout(),
            Point {
                x: close.x + close.width / 2.0,
                y: close.y + close.height / 2.0,
            },
        );
        input.pointer_press_in_scene(ui.scene(), ui.layout());
        let target = input.pointer_release_target().unwrap();
        let action = input
            .select_group_option(ui.scene(), ui.layout(), &target)
            .unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
        require_text(&ui, &format!("Closed {title}"));
        require_text(&ui, &format!("Active {active}"));
        require_text(&ui, &format!("Tab count {count}"));
        require_text(&ui, "Tab moves 1");
        require_text(
            &ui,
            if active == "Valve" {
                "Valve total 1"
            } else {
                "Pump total 0"
            },
        );
    }
}

#[test]
fn model_tabs_share_touch_reordering_and_close_results_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_document_tabs(MobileNavigation::new());
    exercise_document_tabs(interpreted_navigation()?);
    Ok(())
}

fn exercise<C: Component>(app: C) -> UiDriver<C> {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("fast");
    require_text(&ui, "Mode Fast");
    assert!(!ui.can_hit("quiet"));
    ui.click("fast");
    ui.click("balanced");
    require_text(&ui, "Mode Balanced");

    click_text(&mut ui, "Add Tab");
    require_text(&ui, "Tab total 1");
    ui.click("settings");
    require_text(&ui, "Settings page");
    assert!(!ui.can_hit("blocked"));
    ui.click("overview");
    require_text(&ui, "Tab total 1");

    click_text(&mut ui, "Add Swipe");
    assert!(swipe(
        &mut ui,
        Point { x: 325.0, y: 425.0 },
        Point { x: 80.0, y: 425.0 }
    ));
    require_text(&ui, "Card Beta");
    require_text(&ui, "Second card");
    ui.click("lock");
    assert!(!swipe(
        &mut ui,
        Point { x: 325.0, y: 425.0 },
        Point { x: 80.0, y: 425.0 }
    ));
    require_text(&ui, "Card Beta");
    ui.click("lock");
    assert!(swipe(
        &mut ui,
        Point { x: 80.0, y: 425.0 },
        Point { x: 325.0, y: 425.0 }
    ));
    require_text(&ui, "Swipe total 1");
    marker(&mut ui, 2);
    require_text(&ui, "Card Gamma");
    marker(&mut ui, 0);
    require_text(&ui, "Swipe total 1");

    ui.click("open-details");
    click_text(&mut ui, "Add Detail");
    ui.click("review-next");
    require_text(&ui, "Depth 3");
    assert!(ui.back());
    require_text(&ui, "Detail total 1");
    require_text(&ui, "Depth 2");

    assert!(swipe(
        &mut ui,
        Point { x: 4.0, y: 430.0 },
        Point { x: 230.0, y: 430.0 }
    ));
    require_text(&ui, "Drawer opens 1");
    ui.click("show-help");
    assert!(!ui.can_hit("close-menu"));
    ui.escape();
    assert!(ui.can_hit("close-menu"));
    ui.escape();
    require_text(&ui, "Drawer closes 1");
    require_text(&ui, "Detail total 1");
    require_text(&ui, "Depth 2");
    ui.click("open-menu");
    ui.escape();
    assert_eq!(ui.focused(), Some("open-menu"));
    assert!(ui.input().stack_back(ui.scene(), ui.layout()).is_none());
    let scene = ui.scene().clone();
    let action = ui
        .input()
        .stack_back_in_window(&scene, ui.layout())
        .unwrap();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    require_text(&ui, "Route home");
    let scene = ui.scene().clone();
    let mut input = Interaction::default();
    let key = MenuShortcut::new(
        xrack_layout::ShortcutKey::Character('K'),
        xrack_layout::ShortcutModifiers {
            ctrl: true,
            ..Default::default()
        },
    )
    .unwrap();
    let ShortcutOutcome::Activate(action) =
        input.shortcut_press(&scene, ui.layout(), key, false, Instant::now())
    else {
        panic!("application shortcut did not resolve")
    };
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    require_text(&ui, "Hotkeys 1");
    ui
}

#[test]
fn navigation_preserves_local_state_and_overlay_order_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let aot = exercise(MobileNavigation::new());
    assert_eq!(aot.component().get_mode_changes(), 2);
    let interpreted = exercise(interpreted_navigation()?);
    assert_eq!(
        interpreted.component().get("mode-changes")?,
        HostValue::Int(2)
    );
    Ok(())
}

fn exercise_loader<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-workshop");
    let finish_mount = |ui: &mut UiDriver<C>| {
        assert_eq!(ui.node("tool-state").text("text"), Some("Loader loading"));
        assert!(ui.find_path("record").is_err());
        for _ in 0..8 {
            if !ui.finish_loader_frame() {
                break;
            }
        }
        // Apply any scene changes published by the loaded callback.
        ui.update(|_| {});
        assert_eq!(ui.node("tool-state").text("text"), Some("Loader ready"));
        assert_eq!(ui.focused(), Some("record"));
    };
    assert_eq!(ui.node("tool-state").text("text"), Some("Loader null"));
    assert_eq!(ui.node("tool-mounts").text("text"), Some("Mounts 0"));
    ui.click("load-pressure");
    finish_mount(&mut ui);
    assert_eq!(ui.node("tool-mounts").text("text"), Some("Mounts 1"));
    ui.click("record");
    require_text(&ui, "Pressure samples 1");
    require_text(&ui, "Pressure 31");

    ui.click("arm-swap");
    let scene = ui.scene().clone();
    let layout = ui.layout().clone();
    let button = layout.at_path(&ui.path("record")).unwrap();
    let bounds = button.bounds;
    let point = button.map_to_window(Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    });
    let mut touch = Interaction::default();
    let now = Instant::now();
    assert!(touch.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    ui.update(|app| assert!(touch.dispatch_button_press_events(app, &scene, &layout)));
    finish_mount(&mut ui);
    assert_eq!(ui.bounds("record"), bounds);
    require_text(&ui, "Record Flow");
    require_text(&ui, "Flow samples 0");
    require_text(&ui, "Mounts 2");
    touch.reconcile(ui.scene(), ui.layout());
    let release =
        touch.end_scroll_gesture(ui.scene(), ui.layout(), point, now + Duration::from_secs(1));
    assert!(!release.tap && release.control.is_none());
    require_text(&ui, "Records 1");
    assert!(ui.activate(ActivationKey::Enter));
    require_text(&ui, "Flow samples 1");
    require_text(&ui, "Flow 61");
    require_text(&ui, "Records 2");

    ui.click("missing-tool");
    require_text(&ui, "Loader error");
    require_text(&ui, "Mounts 2");
    assert!(ui.find_path("record").is_err());
    ui.click("load-pressure");
    let canceled = ui.scene().clone();
    ui.click("unload-tool");
    assert!(!xrack_runtime::finish_loader_frame(&canceled));
    require_text(&ui, "Loader null");
    require_text(&ui, "Mounts 2");
    assert!(ui.find_path("record").is_err());

    ui.click("load-pressure");
    finish_mount(&mut ui);
    require_text(&ui, "Pressure samples 0");
    require_text(&ui, "Mounts 3");
    require_text(&ui, "Records 2");
}

#[test]
fn loader_keeps_a_stale_touch_from_the_replacement_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_loader(MobileNavigation::new());
    exercise_loader(interpreted_navigation()?);
    Ok(())
}

fn exercise_dynamic_cards<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-card-workshop");
    ui.click("add-cards");
    require_text(&ui, "South loading");
    assert!(text_path(ui.scene(), "South count 0").is_none());
    for _ in 0..8 {
        if !ui.finish_loader_frame() {
            break;
        }
    }
    ui.update(|_| {});
    require_text(&ui, "South ready");
    require_text(&ui, "Mounts 2");
    let card_bounds = |ui: &UiDriver<C>, label| {
        let path = text_path(ui.scene(), label).unwrap();
        ui.layout_at(&path).unwrap().bounds
    };
    let north_top = card_bounds(&ui, "North count 0");
    assert!(north_top.y < card_bounds(&ui, "South count 0").y);
    click_text(&mut ui, "North count 0");
    ui.click("move-north");
    assert!(card_bounds(&ui, "North count 1").y > card_bounds(&ui, "South count 0").y);
    assert!(ui.activate(ActivationKey::Enter));
    require_text(&ui, "North count 2");
    require_text(&ui, "South count 0");
    require_text(&ui, "Mounts 2");

    ui.click("move-north");
    assert_eq!(card_bounds(&ui, "North count 2"), north_top);
    ui.click("arm-removal");
    let scene = ui.scene().clone();
    let layout = ui.layout().clone();
    let button = layout
        .at_path(&text_path(&scene, "North count 2").unwrap())
        .unwrap();
    let point = button.map_to_window(Point {
        x: north_top.x + north_top.width / 2.0,
        y: north_top.y + north_top.height / 2.0,
    });
    let mut touch = Interaction::default();
    let now = Instant::now();
    assert!(touch.begin_scroll_gesture(&scene, &layout, point, now, ScrollGestureKind::Touch));
    ui.update(|app| {
        touch.dispatch_button_press_events(app, &scene, &layout);
    });
    ui.advance_time(Duration::from_millis(600));
    require_text(&ui, "Original null");
    assert!(text_path(ui.scene(), "North count 2").is_none());
    assert_eq!(card_bounds(&ui, "South count 0"), north_top);
    touch.reconcile(ui.scene(), ui.layout());
    let release =
        touch.end_scroll_gesture(ui.scene(), ui.layout(), point, now + Duration::from_secs(1));
    assert!(!release.tap && release.control.is_none());
    click_text(&mut ui, "South count 0");
    require_text(&ui, "South count 1");

    ui.click("add-fresh-card");
    ui.update(|_| {});
    require_text(&ui, "Mounts 3");
    assert_eq!(card_bounds(&ui, "North count 0"), north_top);
    ui.click("try-old-card");
    require_text(&ui, "Stale -1");
    require_text(&ui, "North count 0");
    require_text(&ui, "South count 1");
}

#[test]
fn dynamic_cards_keep_focus_and_invalidate_removed_instances_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_dynamic_cards(MobileNavigation::new());
    exercise_dynamic_cards(interpreted_navigation()?);
    Ok(())
}

fn exercise_service_choice<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-service-workshop");
    let path = ui.path("service-choice");
    let mut input = Interaction::default();
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &path)
        .unwrap();
    assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target.clone()));
    input.sync_editor(ui.scene());
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("ni".into(), None);
    assert!(
        input
            .choice_key(ui.scene(), ui.layout(), ChoiceKey::Accept)
            .is_none()
    );
    for caption in ["Code 10", "Draft empty", "Edits 0", "Accepts 0"] {
        require_text(&ui, caption);
    }

    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit(String::new(), None);
    let now = Instant::now();
    let edit = |ui: &mut UiDriver<C>, input: &mut Interaction, text| {
        let scene = ui.scene().clone();
        ui.update(|app| {
            assert!(input.edit_text(
                app,
                &scene,
                EditAction::ReplaceAll(text),
                EditSource::Ime,
                now
            ))
        });
    };
    edit(&mut ui, &mut input, "47");
    require_text(&ui, "Draft 47");
    require_text(&ui, "Code 10");
    require_text(&ui, "Edits 1");
    let completion = input
        .combo_completion(ui.node("service-choice"), &path)
        .unwrap();
    assert_eq!(completion.suffix(), " Service");
    assert!(input.open_choice(ui.scene(), ui.layout(), target));
    let menu = input.choice_menu(ui.scene(), ui.layout()).unwrap();
    assert_eq!(menu.highlighted, Some(2));
    assert!(!menu.row_enabled(1 - menu.first));
    let row_point = |index| {
        let row = menu.row_bounds(index - menu.first);
        Point {
            x: row.x + row.width / 2.0,
            y: row.y + row.height / 2.0,
        }
    };
    let [disabled, enabled] = [1, 2].map(row_point);
    input.pointer_move(ui.scene(), ui.layout(), disabled);
    assert!(input.choice_press(ui.scene(), ui.layout()));
    assert!(input.choice_release(ui.scene(), ui.layout()).is_none());
    assert!(input.choice_open());
    input.pointer_move(ui.scene(), ui.layout(), enabled);
    assert!(input.choice_press(ui.scene(), ui.layout()));
    let select = input.choice_release(ui.scene(), ui.layout()).unwrap();
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &select.path, select.action)));
    for caption in [
        "Code 30",
        "Picks 1",
        "Accepts 0",
        "Draft 47 Service",
        "Selected finished",
    ] {
        require_text(&ui, caption);
    }

    edit(&mut ui, &mut input, "你好🚰");
    require_text(&ui, "Unicode draft");
    require_text(&ui, "Code 30");
    let accept = input
        .choice_key(ui.scene(), ui.layout(), ChoiceKey::Accept)
        .unwrap();
    assert_eq!(accept.action, ControlAction::AcceptComboText);
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &accept.path, accept.action)));
    for caption in [
        "Code 80",
        "Picks 1",
        "Accepts 1",
        "Accepted finished",
        "Unicode accepted",
    ] {
        require_text(&ui, caption);
    }
}

#[test]
fn editable_service_choice_commits_only_enabled_or_accepted_options_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_service_choice(MobileNavigation::new());
    exercise_service_choice(interpreted_navigation()?);
    Ok(())
}

fn exercise_search_responses<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-search-workshop");
    let path = ui.path("parts-query");
    let mut input = Interaction::default();
    let target = input
        .accessibility_target(ui.scene(), ui.layout(), &path)
        .unwrap();
    assert!(input.focus_text_from_touch(ui.scene(), ui.layout(), target));
    input.sync_editor(ui.scene());
    let now = Instant::now();
    let edit = |ui: &mut UiDriver<C>, input: &mut Interaction, text| {
        let scene = ui.scene().clone();
        ui.update(|app| {
            assert!(input.edit_text(
                app,
                &scene,
                EditAction::ReplaceAll(text),
                EditSource::Keyboard,
                now
            ))
        });
        input.reconcile(ui.scene(), ui.layout());
    };
    edit(&mut ui, &mut input, "me");
    require_text(&ui, "Old request queued");
    edit(&mut ui, &mut input, "met");
    ui.advance_time(Duration::from_millis(120));
    input.reconcile(ui.scene(), ui.layout());
    let menu = input.choice_menu(ui.scene(), ui.layout()).unwrap();
    assert_eq!(menu.rows.as_ref(), ["Meter kit", "Meter body"]);
    let row = menu.row_bounds(0);
    let point = Point {
        x: row.x + row.width / 2.0,
        y: row.y + row.height / 2.0,
    };
    input.pointer_move(ui.scene(), ui.layout(), point);
    assert!(input.choice_press(ui.scene(), ui.layout()));

    ui.advance_time(Duration::from_secs(5));
    input.reconcile(ui.scene(), ui.layout());
    require_text(&ui, "Batch me");
    require_text(&ui, "Query met");
    assert!(input.choice_menu(ui.scene(), ui.layout()).is_none());
    assert!(input.choice_release(ui.scene(), ui.layout()).is_none());
    require_text(&ui, "Picks 0");
    require_text(&ui, "Searches 2");
    edit(&mut ui, &mut input, "meter");
    ui.advance_time(Duration::from_millis(120));
    input.reconcile(ui.scene(), ui.layout());
    let menu = input.choice_menu(ui.scene(), ui.layout()).unwrap();
    assert_eq!(menu.rows.as_ref(), ["Meter gasket", "Meter sensor"]);
    input.choice_key(ui.scene(), ui.layout(), ChoiceKey::End);
    let choice = input
        .choice_key(ui.scene(), ui.layout(), ChoiceKey::Accept)
        .unwrap();
    assert_eq!(choice.action, ControlAction::Select(1));
    let scene = ui.scene().clone();
    ui.update(|app| assert!(input.select_search_suggestion(app, &scene, 1, now)));
    for caption in [
        "Query Meter sensor",
        "Sent Meter sensor",
        "Picks 1",
        "Accepts 0",
        "Searches 4",
    ] {
        require_text(&ui, caption);
    }

    let scene = ui.scene().clone();
    ui.update(|app| assert!(input.replay_text_edit(app, &scene, false)));
    input.reconcile(ui.scene(), ui.layout());
    require_text(&ui, "Query meter");
    assert!(
        input
            .choice_menu(ui.scene(), ui.layout())
            .unwrap()
            .highlighted
            .is_none()
    );
    let submit = input
        .choice_key(ui.scene(), ui.layout(), ChoiceKey::Accept)
        .unwrap();
    assert_eq!(submit.action, ControlAction::SubmitSearch);
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &submit.path, submit.action)));
    for caption in [
        "Query meter",
        "Sent meter",
        "Picks 1",
        "Accepts 1",
        "Searches 6",
    ] {
        require_text(&ui, caption);
    }
}

#[test]
fn search_rejects_late_responses_and_undoes_selection_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_search_responses(MobileNavigation::new());
    exercise_search_responses(interpreted_navigation()?);
    Ok(())
}

fn exercise_anchored_popup<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-popup-workshop");
    ui.click("popup-anchor");
    let above_anchor = ui.bounds("popup-anchor");
    let below_popup = ui.bounds("inspection-popup");
    assert_eq!(below_popup.y, above_anchor.y + above_anchor.height + 8.0);
    assert_eq!(ui.focused(), Some("move-popup-anchor"));
    assert_eq!(ui.layout().dialog_paths().len(), 1);
    require_text(&ui, "Opens 1");
    ui.click("move-popup-anchor");
    let lower_anchor = ui.bounds("popup-anchor");
    let flipped = ui.bounds("inspection-popup");
    assert_eq!(flipped.y + flipped.height + 8.0, lower_anchor.y);
    assert!(flipped.x > below_popup.x && flipped.x + flipped.width <= 390.0);
    require_text(&ui, "Opens 1");
    require_text(&ui, "Closes 0");
    ui.click("apply-popup-action");
    require_text(&ui, "Actions 1");
    ui.click("outside-action");
    require_text(&ui, "Closes 1");
    require_text(&ui, "Outside 1");
    assert!(ui.find_path("apply-popup-action").is_err());

    ui.click("modal-mode");
    ui.click("popup-anchor");
    assert_eq!(ui.layout().dialog_paths().len(), 2);
    assert!(!ui.can_hit("outside-action"));
    ui.escape();
    assert_eq!(ui.focused(), Some("popup-anchor"));
    require_text(&ui, "Closes 2");
    require_text(&ui, "Outside 1");
    assert!(ui.activate(ActivationKey::Enter));
    require_text(&ui, "Opens 3");
    ui.escape();
    require_text(&ui, "Closes 3");

    ui.click("modal-mode");
    ui.click("popup-anchor");
    assert_eq!(ui.layout().dialog_paths().len(), 1);
    ui.escape();
    require_text(&ui, "Closes 4");
    assert_eq!(ui.focused(), Some("popup-anchor"));
    assert_eq!(ui.layout().dialog_paths().len(), 1);
    ui.escape();
    assert!(ui.layout().dialog_paths().is_empty());
    assert!(ui.can_hit("open-menu"));
}

#[test]
fn anchored_popup_repositions_and_restores_focus_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_anchored_popup(MobileNavigation::new());
    exercise_anchored_popup(interpreted_navigation()?);
    Ok(())
}

fn exercise_workshop_menu<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-commands");
    let mut touch = Interaction::default();
    let target = touch
        .accessibility_target(ui.scene(), ui.layout(), &ui.path("workshop-menu"))
        .unwrap();
    assert!(touch.open_menu_bar_with_touch(ui.scene(), ui.layout(), target, true));
    let branch = touch.menu_panels(ui.scene(), ui.layout())[0].row_bounds(1);
    assert_eq!(branch.height, 44.0);
    let point = Point {
        x: branch.x + branch.width / 2.0,
        y: branch.y + branch.height / 2.0,
    };
    touch.pointer_move(ui.scene(), ui.layout(), point);
    assert!(touch.menu_press(ui.scene(), ui.layout()));
    assert!(touch.menu_release(ui.scene(), ui.layout()).is_none());
    let menus = touch.menu_panels(ui.scene(), ui.layout());
    assert_eq!(menus.len(), 2);
    assert_eq!(menus[1].row_bounds(0).height, 44.0);

    ui.click("workshop-menu");
    assert_eq!(ui.menus()[0].row_bounds(0).height, 28.0);
    assert!(!ui.click_menu_row("Unavailable"));
    assert_eq!(ui.menus().len(), 1);
    ui.click_menu_row("View mode");
    assert_eq!(ui.menus().len(), 2);
    assert!(ui.click_menu_row("Compact"));
    for caption in [
        "Mode compact",
        "Tool compact",
        "Calls 1",
        "Groups 1",
        "Order ok",
        "Denied 0",
    ] {
        require_text(&ui, caption);
    }
    ui.click("detail-tool");
    require_text(&ui, "Mode detail");
    require_text(&ui, "Tool detail");
    require_text(&ui, "Calls 2");
    assert!(ui.menu_shortcut("Ctrl+2".parse().unwrap()));
    require_text(&ui, "Mode compact");
    require_text(&ui, "Calls 3");
    require_text(&ui, "Groups 3");
    ui.click("lock-modes");
    assert!(!ui.can_hit("detail-tool"));
    assert!(!ui.menu_shortcut("Ctrl+1".parse().unwrap()));
    ui.click("workshop-menu");
    ui.click_menu_row("View mode");
    assert!(!ui.click_menu_row("Detailed"));
    assert_eq!(ui.menus().len(), 2);
    ui.menu_key(MenuKey::Escape);
    assert_eq!(ui.menus().len(), 1);
    ui.menu_key(MenuKey::Escape);
    assert!(ui.menus().is_empty());
    assert_eq!(ui.focused(), Some("workshop-menu"));
    for caption in [
        "Mode compact",
        "Calls 3",
        "Groups 3",
        "Order ok",
        "Denied 0",
    ] {
        require_text(&ui, caption);
    }
    ui.escape();
    assert!(ui.can_hit("open-menu"));
}

#[test]
fn command_workshop_shares_menu_toolbar_and_shortcuts_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_workshop_menu(MobileNavigation::new());
    exercise_workshop_menu(interpreted_navigation()?);
    Ok(())
}

fn exercise_scan<C: Component>(app: C) {
    fn check_progress<C: Component>(ui: &UiDriver<C>, samples: u8) {
        require_text(ui, &format!("Samples {samples} of 4"));
        for id in ["scan-progress", "scan-spinner"] {
            assert_eq!(
                ui.node(id).number("progress"),
                Some(f64::from(samples) / 4.0)
            );
        }
    }

    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-scan");
    check_progress(&ui, 0);
    assert!(!ui.can_hit("resume-scan"));
    assert!(!ui.can_hit("queue-report"));
    ui.click("start-scan");
    ui.advance_time(Duration::from_millis(1199));
    check_progress(&ui, 0);
    ui.advance_time(Duration::from_millis(1));
    check_progress(&ui, 1);
    ui.advance_time(Duration::from_millis(1200));
    check_progress(&ui, 2);
    require_text(&ui, "Scan paused");
    ui.advance_time(Duration::from_secs(8));
    check_progress(&ui, 2);

    ui.click("resume-scan");
    ui.advance_time(Duration::from_millis(1200));
    check_progress(&ui, 3);
    ui.advance_time(Duration::from_millis(1200));
    check_progress(&ui, 4);
    require_text(&ui, "Scan complete");

    ui.click("queue-report");
    require_text(&ui, "Report queued");
    assert!(ui.node("scan-busy").boolean("running", false));
    ui.advance_time(Duration::from_secs(2));
    ui.escape();
    assert!(ui.can_hit("open-menu"));
    ui.advance_time(Duration::from_secs(10));
    ui.click("open-menu");
    ui.click("open-scan");
    check_progress(&ui, 4);
    require_text(&ui, "Reports 0");
    require_text(&ui, "Report queued");
    ui.advance_time(Duration::from_millis(6499));
    require_text(&ui, "Reports 0");
    ui.advance_time(Duration::from_millis(1));
    require_text(&ui, "Report saved");
    require_text(&ui, "Reports 1");
    assert!(!ui.node("scan-busy").boolean("running", true));
    ui.advance_time(Duration::from_secs(13));
    check_progress(&ui, 4);
    require_text(&ui, "Reports 1");
}

#[test]
fn scan_timers_follow_visibility_and_update_indicators_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_scan(MobileNavigation::new());
    exercise_scan(interpreted_navigation()?);
    Ok(())
}

fn exercise_focus_routes<C: Component>(app: C) {
    fn route<C: Component>(ui: &mut UiDriver<C>, phase: KeyPhase, key: &str) -> EventResult {
        let scene = ui.scene().clone();
        let layout = ui.layout().clone();
        let route = ui.input().key_route(&scene, &layout).unwrap();
        let event = KeyEvent {
            key: key.into(),
            text: if key == "F2" { "" } else { key }.into(),
            ..Default::default()
        };
        let mut result = None;
        ui.update(|app| result = route.dispatch(app, &scene, &layout, phase, &event));
        result.unwrap()
    }

    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    let mut observers = FocusObservers::default();
    let mut sync = |ui: &mut UiDriver<C>, active| {
        let events = observers.sync(ui.input(), ui.scene(), ui.layout(), active);
        let scene = ui.scene().clone();
        ui.update(|app| {
            for event in events {
                event.apply(app, &scene);
            }
        });
    };
    ui.click("open-menu");
    ui.click("open-focus");
    ui.click("enter-fields");
    assert_eq!(ui.focused(), Some("stock-code"));
    sync(&mut ui, true);
    require_text(&ui, "Focus code");
    require_text(&ui, "Entries 1");
    assert_eq!(
        route(&mut ui, KeyPhase::CapturePressed, "x"),
        EventResult::Accept
    );
    require_text(&ui, "Key blocked");
    require_text(&ui, "Code empty");
    assert_eq!(
        route(&mut ui, KeyPhase::CapturePressed, "a"),
        EventResult::Reject
    );
    assert!(ui.edit_focused(EditAction::Insert("a")));
    require_text(&ui, "Code a");

    ui.tab();
    assert_eq!(ui.focused(), Some("stock-memo"));
    sync(&mut ui, true);
    require_text(&ui, "Entries 1");
    assert_eq!(
        route(&mut ui, KeyPhase::CapturePressed, "F2"),
        EventResult::Reject
    );
    assert_eq!(route(&mut ui, KeyPhase::Pressed, "F2"), EventResult::Accept);
    require_text(&ui, "F2 routed");
    require_text(&ui, "Blocks 1");
    require_text(&ui, "Routes 1");
    ui.tab_with_direction(true);
    assert_eq!(ui.focused(), Some("stock-code"));
    ui.tab();
    assert_eq!(ui.focused(), Some("stock-memo"));

    ui.click("leave-fields");
    sync(&mut ui, true);
    require_text(&ui, "Focus outside");
    ui.click("enter-fields");
    sync(&mut ui, true);
    require_text(&ui, "Focus memo");
    require_text(&ui, "Entries 2");
    sync(&mut ui, false);
    require_text(&ui, "Focus outside");
    sync(&mut ui, true);
    require_text(&ui, "Focus memo");
    require_text(&ui, "Entries 3");
    assert!(ui.edit_focused(EditAction::Insert("b")));
    require_text(&ui, "Memo b");

    ui.click("lock-memo");
    ui.click("enter-fields");
    assert_eq!(ui.focused(), Some("stock-code"));
    ui.escape();
    assert!(ui.can_hit("open-menu"));
}

#[test]
fn nested_focus_routes_and_remembered_editors_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_focus_routes(MobileNavigation::new());
    exercise_focus_routes(interpreted_navigation()?);
    Ok(())
}

fn exercise_motion<C: Component>(app: C) {
    let viewport = Size::new(390.0, 750.0);
    let mut ui = UiDriver::headless(app, viewport);
    ui.click("open-menu");
    ui.click("open-motion");
    let base = ui.scene().clone();
    let deck = ui.path("deck");
    let button = ui.path("motion-target");
    let baseline = ui.bounds("deck");
    assert_eq!(baseline.width, 72.0);
    ui.click("move-deck");
    let deployed = ui.scene().clone();
    assert_eq!(ui.bounds("deck").width, 168.0);
    assert_eq!(ui.node("motion-target").text("text"), Some("Wide tile"));

    let mut timeline = SceneTransition::between(&base, &deployed);
    let at = Duration::from_millis;
    let halfway = timeline.frame(&deployed, at(5000));
    let mut input = Interaction::default();
    let layout = layout_with_input(&halfway, viewport, &mut FixedText, &input);
    let rectangle = layout.at_path(&deck).unwrap().bounds;
    assert_eq!((rectangle.x - baseline.x, rectangle.width), (52.0, 120.0));
    let gap = Point {
        x: baseline.x + 90.0,
        y: rectangle.y + 42.0,
    };
    assert!(gap.x > baseline.x + baseline.width);
    assert!(gap.x < ui.bounds("deck").x);
    input.reconcile(&halfway, &layout);
    input.pointer_move(&halfway, &layout, gap);
    input.pointer_press_in_scene(&halfway, &layout);
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, button);
    ui.update(|app| assert!(app.dispatch_at(&halfway, &target.path, "clicked")));
    require_text(&ui, "Motion picks 1");

    ui.click("widen-base");
    require_text(&ui, "Base wider");
    assert_eq!(ui.bounds("deck").width, 168.0);
    let wider = ui.scene().clone();
    timeline.retarget(&deployed, &wider, at(5000));
    ui.click("move-deck");
    let stowed = ui.scene().clone();
    timeline.retarget(&wider, &stowed, at(5000));
    let bounds_at = |millis| {
        let scene = timeline.frame(&stowed, at(millis));
        layout_with_input(&scene, viewport, &mut FixedText, &input)
            .at_path(&deck)
            .unwrap()
            .bounds
    };
    assert_eq!(bounds_at(5000), rectangle);
    let returning = bounds_at(10000);
    assert_eq!((returning.x - baseline.x, returning.width), (26.0, 108.0));
    let restored = bounds_at(15000);
    assert_eq!((restored.x, restored.width), (baseline.x, 96.0));
    assert_eq!(ui.node("motion-target").text("text"), Some("Tile"));
    assert!(!timeline.running(at(15000)));
}

#[test]
fn motion_uses_the_animated_hitbox_and_retargets_to_the_live_base_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_motion(MobileNavigation::new());
    exercise_motion(interpreted_navigation()?);
    Ok(())
}

fn exercise_layout_workshop(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 750.0));
    ui.click("open-menu");
    ui.click("open-layouts");
    let x = |ui: &UiDriver<_>, id| ui.bounds(id).x;
    let y = |ui: &UiDriver<_>, id| ui.bounds(id).y;

    assert!(x(&ui, "grid-pump") < x(&ui, "grid-valve"));
    assert!(x(&ui, "flow-ash") < x(&ui, "flow-bay"));
    assert!(x(&ui, "flow-bay") < x(&ui, "flow-fir"));
    assert_eq!(y(&ui, "flow-ash"), y(&ui, "flow-fir"));
    assert!(x(&ui, "flex-blue") > x(&ui, "flex-red"));
    assert!(x(&ui, "flex-red") > x(&ui, "flex-gold"));
    assert_eq!(y(&ui, "flex-blue"), y(&ui, "flex-gold"));
    let pump = ui.bounds("grid-pump");
    ui.click("grid-pump");
    require_text(&ui, "Picked Grid pump");
    ui.click("flex-blue");
    assert_eq!(ui.focused(), Some("flex-blue"));
    ui.tab();
    assert_eq!(ui.focused(), Some("flex-red"));
    assert!(ui.activate(ActivationKey::Enter));
    require_text(&ui, "Picked Flex red");
    require_text(&ui, "Layout clicks 3");

    ui.click("mirror-rows");
    let valve = ui.bounds("grid-valve");
    let physical_center = pump.x + pump.width / 2.0;
    assert!((valve.x..valve.x + valve.width).contains(&physical_center));
    assert!(x(&ui, "grid-pump") > x(&ui, "grid-valve"));
    assert!(x(&ui, "flow-ash") > x(&ui, "flow-bay"));
    assert!(x(&ui, "flow-bay") > x(&ui, "flow-fir"));
    assert!(x(&ui, "flex-blue") < x(&ui, "flex-red"));
    assert!(x(&ui, "flex-red") < x(&ui, "flex-gold"));
    ui.click("grid-valve");
    require_text(&ui, "Picked Grid valve");

    ui.click("width-rows");
    assert_eq!(y(&ui, "flow-ash"), y(&ui, "flow-bay"));
    assert!(y(&ui, "flow-fir") > y(&ui, "flow-bay"));
    assert_eq!(x(&ui, "flow-ash"), x(&ui, "flow-fir"));
    assert_eq!(y(&ui, "flex-blue"), y(&ui, "flex-red"));
    assert!(y(&ui, "flex-gold") > y(&ui, "flex-red"));
    assert_eq!(x(&ui, "flex-blue"), x(&ui, "flex-gold"));
    ui.click("flow-fir");
    require_text(&ui, "Picked Flow fir");
    ui.click("flex-gold");
    require_text(&ui, "Picked Flex gold");
    require_text(&ui, "Layout clicks 6");
    ui.click("width-rows");
    assert_eq!(y(&ui, "flow-ash"), y(&ui, "flow-fir"));
    assert_eq!(y(&ui, "flex-blue"), y(&ui, "flex-gold"));
    require_text(&ui, "Layout clicks 6");
}

#[test]
fn layout_workshop_mirrors_and_wraps_with_consistent_input_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_layout_workshop(MobileNavigation::new());
    exercise_layout_workshop(interpreted_navigation()?);
    Ok(())
}
