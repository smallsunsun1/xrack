#![cfg(test)]

use std::time::{Duration, Instant};

use mobile_controls_ui::MobileControls;
use xrack_interpreter::load_mlir;
use xrack_layout::{
    ActivationKey, ButtonClick, ButtonHoldEvent, ChoiceKey, EditAction, Interaction,
    ScrollGestureKind, SliderGeometry, SpinGeometry, ToggleGeometry,
};
use xrack_runtime::{ButtonAnimations, CheckState, Color, Component, ControlAction};
use xrack_testing::{Point, Size, UiDriver};

fn slide<C: Component>(ui: &mut UiDriver<C>, id: &str, from: f64, to: f64) {
    let path = ui.path(id);
    let geometry = SliderGeometry::for_node(ui.bounds(id), ui.node(id));
    let mut input = Interaction::default();
    input.pointer_move(ui.scene(), ui.layout(), geometry.position(from));
    input.pointer_press_in_scene(ui.scene(), ui.layout());
    if let Some(action) = input.slider_press(ui.scene(), ui.layout()) {
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
    }
    input.pointer_move(ui.scene(), ui.layout(), geometry.position(to));
    if let Some(action) = input.slider_drag(ui.scene(), ui.layout()) {
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
    }
    if let Some(action) = input.slider_release(ui.scene(), ui.layout()) {
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
    }
    assert_eq!(ui.path(id), path);
}

fn step<C: Component>(ui: &mut UiDriver<C>, id: &str, up: bool) -> bool {
    let mut input = Interaction::default();
    let geometry = SpinGeometry::for_node(ui.bounds(id), ui.node(id));
    let rect = if up { geometry.up } else { geometry.down };
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        },
    );
    input.pointer_press_in_scene(ui.scene(), ui.layout());
    if !input.spin_press(ui.scene(), ui.layout()) {
        return false;
    }
    let action = input.spin_release(ui.scene(), ui.layout()).unwrap();
    let scene = ui.scene().clone();
    let mut changed = false;
    ui.update(|app| changed = app.control_at(&scene, &action.path, action.action));
    changed
}

fn exercise<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    slide(&mut ui, "volume-slider", 0.25, 0.75);
    assert_eq!(ui.node("volume-label").text("text"), Some("Volume 75"));
    assert_eq!(ui.node("changes-label").text("text"), Some("Changes 1"));
    assert_eq!(ui.node("releases-label").text("text"), Some("Releases 1"));
    assert_eq!(ui.node("meter").float("progress"), Some(0.75));
    ui.click("mute");
    slide(&mut ui, "volume-slider", 0.75, 0.10);
    assert_eq!(ui.node("volume-label").text("text"), Some("Volume 75"));
    assert_eq!(ui.node("changes-label").text("text"), Some("Changes 1"));
    assert_eq!(ui.node("releases-label").text("text"), Some("Releases 1"));
    ui.click("mute");

    slide(&mut ui, "range", 0.20, 0.40);
    assert_eq!(ui.node("range-label").text("text"), Some("Range 40 - 75"));
    assert!(step(&mut ui, "count-input", true));
    assert!(step(&mut ui, "gain-input", false));
    assert_eq!(ui.node("count-label").text("text"), Some("Count 3"));
    assert_eq!(ui.node("gain-label").text("text"), Some("Gain 1"));
    ui.click("priority-choice");
    ui.choice_key(ChoiceKey::Down);
    ui.choice_key(ChoiceKey::Accept);
    assert_eq!(
        ui.node("choice-label").text("text"),
        Some("Priority Urgent")
    );

    ui.click("lock");
    assert!(!step(&mut ui, "count-input", true));
    assert!(!step(&mut ui, "gain-input", false));
    slide(&mut ui, "range", 0.40, 0.60);
    assert_eq!(ui.node("range-label").text("text"), Some("Range 40 - 75"));
    assert_eq!(ui.node("count-label").text("text"), Some("Count 3"));
    assert_eq!(ui.node("gain-label").text("text"), Some("Gain 1"));
    assert!(!ui.can_hit("priority-choice"));
    ui.click("lock");

    ui.click("opener");
    assert!(!ui.can_hit("background-action"));
    ui.click("apply");
    assert_eq!(ui.node("accepted-label").text("text"), Some("Accepted 1"));
    ui.click("opener");
    ui.click("cancel");
    assert_eq!(ui.node("rejected-label").text("text"), Some("Rejected 1"));
    ui.click("opener");
    ui.escape();
    assert_eq!(ui.node("rejected-label").text("text"), Some("Rejected 2"));
    ui.click("background-action");
    assert_eq!(
        ui.node("background-action").text("text"),
        Some("Background 1")
    );
}

fn drag_wheel<C: Component>(ui: &mut UiDriver<C>, id: &str, delta: Point, cancel: bool) {
    let item = ui.layout_at(&ui.path(id)).unwrap();
    let rect = item.tumbler.as_ref().unwrap().center;
    let start = item.map_to_window(Point {
        x: rect.x + rect.width / 2.0,
        y: rect.y + rect.height / 2.0,
    });
    let end = Point {
        x: start.x + delta.x,
        y: start.y + delta.y,
    };
    let now = Instant::now();
    let mut input = Interaction::default();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        now,
        ScrollGestureKind::Touch
    ));
    let finish = now + Duration::from_millis(100);
    input.move_scroll_gesture(ui.scene(), ui.layout(), end, finish);
    if cancel {
        assert!(input.tumbler_preview().is_some());
        input.cancel_scroll_gesture();
        assert!(input.tumbler_preview().is_none());
    }
    let end = input.end_scroll_gesture(ui.scene(), ui.layout(), end, finish);
    if let Some(control) = end.control {
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.control_at(&scene, &control.path, control.action)));
    }
}

fn assert_wheel_state(
    ui: &UiDriver<impl Component>,
    wheel: i64,
    wheel_commits: i64,
    card: i64,
    card_commits: i64,
) {
    for (id, value) in [
        ("wheel-value", format!("Wheel index {wheel}")),
        ("wheel-count", format!("Wheel commits {wheel_commits}")),
        ("card-value", format!("Card code {card}")),
        ("card-count", format!("Card commits {card_commits}")),
    ] {
        assert_eq!(ui.node(id).text("text"), Some(value.as_str()), "{id}");
    }
}

fn exercise_selection_wheels(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-wheels");
    assert_wheel_state(&ui, 0, 0, 20, 0);
    let down = Point { x: 0.0, y: 36.0 };
    let up = Point { x: 0.0, y: -36.0 };
    let left = Point { x: -80.0, y: 0.0 };
    let right = Point { x: 80.0, y: 0.0 };
    drag_wheel(&mut ui, "wheel", down, false);
    assert_wheel_state(&ui, 0, 0, 20, 0);
    ui.click("wrap");
    drag_wheel(&mut ui, "wheel", down, false);
    assert_wheel_state(&ui, 4, 1, 20, 0);
    drag_wheel(&mut ui, "wheel", up, false);
    assert_wheel_state(&ui, 0, 2, 20, 0);
    drag_wheel(&mut ui, "gallery", left, false);
    assert_wheel_state(&ui, 0, 2, 30, 1);
    ui.click("mirror");
    drag_wheel(&mut ui, "gallery", right, false);
    assert_wheel_state(&ui, 0, 2, 40, 2);
    ui.click("lock-wheels");
    drag_wheel(&mut ui, "wheel", up, false);
    drag_wheel(&mut ui, "gallery", right, false);
    assert_wheel_state(&ui, 0, 2, 40, 2);
    ui.click("lock-wheels");
    drag_wheel(&mut ui, "wheel", up, true);
    assert_wheel_state(&ui, 0, 2, 40, 2);
    drag_wheel(&mut ui, "wheel", up, false);
    assert_wheel_state(&ui, 1, 3, 40, 2);
}

fn exercise_touch_transforms(app: impl Component) {
    use xrack_layout::scale_rotate_gestures::{ContactId, Context, Gestures, TouchPhase};

    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-gestures");
    ui.click("probe");
    let pad = ui.bounds("gesture");
    let first = Point {
        x: pad.x + 68.0,
        y: pad.y + 42.0,
    };
    let second = Point {
        x: first.x + 60.0,
        y: first.y,
    };
    let mut gestures = Gestures::default();
    let input = Interaction::default();
    let assert_label = |ui: &UiDriver<_>, id, text| {
        assert_eq!(ui.node(id).text("text"), Some(text), "{id}");
    };

    for (distance, angle, cancelled) in [(90.0, "Angle 90", false), (60.0, "Angle 180", true)] {
        let moved = Point {
            x: first.x,
            y: first.y + distance,
        };
        for (finger, phase, point, consumed) in [
            (1, TouchPhase::Started, first, false),
            (2, TouchPhase::Started, second, true),
            (2, TouchPhase::Moved, moved, true),
        ] {
            let context = Context {
                scene: ui.scene(),
                layout: ui.layout(),
                input: &input,
            };
            let (used, events) = gestures.touch(&context, ContactId((), finger), phase, point);
            assert_eq!(used, consumed);
            let scene = ui.scene().clone();
            ui.update(|app| {
                for event in events.into_iter().flatten() {
                    assert!(event.apply(app, &scene));
                }
            });
        }
        assert_label(&ui, "gesture-state", "Gesture active");
        assert_label(&ui, "angle-label", angle);
        assert_label(&ui, "zoom-label", "Zoom 150");

        let events = if cancelled {
            [gestures.reset(), None]
        } else {
            let context = Context {
                scene: ui.scene(),
                layout: ui.layout(),
                input: &input,
            };
            let (used, events) =
                gestures.touch(&context, ContactId((), 2), TouchPhase::Ended, moved);
            assert!(used);
            gestures.touch(&context, ContactId((), 1), TouchPhase::Ended, first);
            events
        };
        let scene = ui.scene().clone();
        ui.update(|app| {
            for event in events.into_iter().flatten() {
                assert!(event.apply(app, &scene));
            }
        });
        assert_label(&ui, "gesture-state", "Gesture idle");
        assert_label(&ui, "angle-label", "Angle 90");
        assert_label(&ui, "ends-label", "Ends 1");
    }
    assert_label(&ui, "starts-label", "Starts 2");
    assert_label(&ui, "cancels-label", "Cancels 1");
    assert_label(&ui, "taps-label", "Taps 1");
    ui.click("probe");
    assert_label(&ui, "taps-label", "Taps 2");
}

#[test]
fn mobile_controls_share_aot_and_interpreter_input_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    exercise(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

#[test]
fn selection_wheel_touches_preserve_bindings_and_commit_counts()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_selection_wheels(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_selection_wheels(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

#[test]
fn scale_rotate_touches_preserve_committed_transform_on_cancel()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_touch_transforms(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_touch_transforms(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_trackpad_transforms(app: impl Component) {
    use xrack_layout::scale_rotate_gestures::{Context, Gestures, TouchPhase, TrackpadDelta};

    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-gestures");
    let pad = ui.bounds("gesture");
    let mut input = Interaction::default();
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: pad.x + 92.0,
            y: pad.y + 96.0,
        },
    );
    let mut gestures = Gestures::default();

    for (phase, delta, id, value) in [
        (
            TouchPhase::Started,
            TrackpadDelta::Scale(0.25),
            "zoom-label",
            "Zoom 125",
        ),
        (
            TouchPhase::Started,
            TrackpadDelta::Rotation(0.0),
            "starts-label",
            "Starts 1",
        ),
        (
            TouchPhase::Moved,
            TrackpadDelta::Rotation(-30.0),
            "angle-label",
            "Angle 30",
        ),
        (
            TouchPhase::Ended,
            TrackpadDelta::Scale(0.20),
            "gesture-state",
            "Gesture active",
        ),
        (
            TouchPhase::Ended,
            TrackpadDelta::Rotation(0.0),
            "ends-label",
            "Ends 1",
        ),
        (
            TouchPhase::Started,
            TrackpadDelta::Scale(0.20),
            "zoom-label",
            "Zoom 180",
        ),
        (
            TouchPhase::Cancelled,
            TrackpadDelta::Scale(0.0),
            "cancels-label",
            "Cancels 1",
        ),
    ] {
        let events = gestures.trackpad(
            &Context {
                scene: ui.scene(),
                layout: ui.layout(),
                input: &input,
            },
            (),
            phase,
            delta,
        );
        let scene = ui.scene().clone();
        ui.update(|app| {
            for event in events.into_iter().flatten() {
                assert!(event.apply(app, &scene));
            }
        });
        assert_eq!(ui.node(id).text("text"), Some(value), "{id}");
    }
    assert_eq!(ui.node("zoom-label").text("text"), Some("Zoom 150"));
    assert_eq!(ui.node("angle-label").text("text"), Some("Angle 30"));
    ui.click("probe");
    assert_eq!(ui.node("taps-label").text("text"), Some("Taps 1"));
}

#[test]
fn scale_rotate_trackpad_channels_share_a_gesture_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_trackpad_transforms(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_trackpad_transforms(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_dial(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-dial");
    let dial = ui.layout_at(&ui.path("gain-dial")).unwrap();
    let bounds = dial.bounds;
    let center = dial.map_to_window(Point {
        x: bounds.x + bounds.width / 2.0,
        y: bounds.y + bounds.height / 2.0,
    });
    let point = |(dx, dy)| Point {
        x: center.x + dx,
        y: center.y + dy,
    };
    let mut input = Interaction::default();
    let assert_label = |ui: &UiDriver<_>, id, text| {
        assert_eq!(ui.node(id).text("text"), Some(text), "{id}");
    };

    for (mode, start, moves, value, releases) in [
        (
            None,
            (56.0, 0.0),
            [(0.0, -56.0), (-56.0, 0.0)],
            "Dial 75",
            "Dial releases 1",
        ),
        (
            Some("horizontal-mode"),
            (0.0, 0.0),
            [(0.0, 30.0), (84.0, 30.0)],
            "Dial 35",
            "Dial releases 2",
        ),
        (
            Some("vertical-mode"),
            (0.0, 0.0),
            [(30.0, 0.0), (30.0, -70.0)],
            "Dial 85",
            "Dial releases 3",
        ),
    ] {
        if let Some(mode) = mode {
            ui.click(mode);
        }
        assert!(input.touch_range_press(ui.scene(), ui.layout(), point(start)));
        if let Some(action) = input.slider_press(ui.scene(), ui.layout()) {
            let scene = ui.scene().clone();
            ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
            assert_label(&ui, "dial-value", "Dial 25");
        }
        for offset in moves {
            input.pointer_move(ui.scene(), ui.layout(), point(offset));
            if let Some(action) = input.slider_drag(ui.scene(), ui.layout()) {
                let scene = ui.scene().clone();
                ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
            }
        }
        assert_label(&ui, "dial-value", value);
        let action = input.slider_release(ui.scene(), ui.layout()).unwrap();
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
        input.finish_touch_range();
        assert_label(&ui, "dial-releases", releases);
    }
    assert_label(&ui, "dial-last-release", "Released at 85");
    ui.click("lock-dial");
    assert!(!input.touch_range_press(ui.scene(), ui.layout(), center));
    ui.click("lock-dial");
    assert!(input.touch_range_press(ui.scene(), ui.layout(), center));
    assert!(input.slider_press(ui.scene(), ui.layout()).is_none());
    input.pointer_move(ui.scene(), ui.layout(), point((0.0, 21.0)));
    let action = input.slider_drag(ui.scene(), ui.layout()).unwrap();
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.adjust_range_at(&scene, &action.path, action.action)));
    input.finish_touch_range();
    assert!(input.slider_release(ui.scene(), ui.layout()).is_none());
    assert_label(&ui, "dial-value", "Dial 70");
    assert_label(&ui, "dial-releases", "Dial releases 3");
    assert_label(&ui, "dial-last-release", "Released at 85");
}

#[test]
fn mobile_dial_touches_preserve_mode_wrap_and_release_contract()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_dial(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_dial(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn separate_coincident_thumbs<C: Component>(app: C, direction: f32) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    let path = ui.path("range");
    let geometry = SliderGeometry::for_node(ui.bounds("range"), ui.node("range"));
    let start = geometry.position(0.5);
    let mut input = Interaction::default();
    assert!(input.touch_range_press(ui.scene(), ui.layout(), start));
    assert!(input.slider_press(ui.scene(), ui.layout()).is_none());
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: start.x - direction * 2.0,
            y: start.y + 6.0,
        },
    );
    assert!(input.slider_drag(ui.scene(), ui.layout()).is_none());
    assert!(input.slider_preview_position().is_none());

    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: start.x + direction * 64.0,
            ..start
        },
    );
    let drag = input.slider_drag(ui.scene(), ui.layout()).unwrap();
    let handle = usize::from(direction > 0.0);
    assert_eq!(drag.path, [path.clone(), vec![handle]].concat());
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.adjust_range_at(&scene, &drag.path, drag.action)));
    let release = input.slider_release(ui.scene(), ui.layout()).unwrap();
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.adjust_range_at(&scene, &release.path, release.action)));
    input.finish_touch_range();
    assert_eq!(
        ui.node("range-label").text("text"),
        Some(if direction > 0.0 {
            "Range 50 - 70"
        } else {
            "Range 30 - 50"
        })
    );
}

#[test]
fn coincident_range_thumbs_ignore_touch_jitter_until_direction_is_clear()
-> Result<(), Box<dyn std::error::Error>> {
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    let program = load_mlir(&module.mlir)?;
    for direction in [-1.0, 1.0] {
        let mut aot = MobileControls::new();
        aot.set_lower(50.0);
        aot.set_upper(50.0);
        separate_coincident_thumbs(aot, direction);
        let interpreted = program.create("MobileControls")?;
        interpreted.set("lower", xrack_interpreter::HostValue::Float(50.0))?;
        interpreted.set("upper", xrack_interpreter::HostValue::Float(50.0))?;
        separate_coincident_thumbs(interpreted, direction);
    }
    Ok(())
}

fn exercise_range_handles<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-dial");
    ui.click("open-range");
    let path = ui.path("band");
    let mut input = Interaction::default();
    let state = |ui: &UiDriver<_>, values, preview, (edits, releases)| {
        assert_eq!(ui.node("band-value").text("text"), Some(values));
        assert_eq!(ui.node("band-preview").text("text"), Some(preview));
        assert_eq!(
            ui.node("band-order").text("text"),
            Some("Callbacks ordered")
        );
        assert_eq!(
            ui.node("band-edits").text("text"),
            Some(format!("Range edits {edits}").as_str())
        );
        assert_eq!(
            ui.node("band-releases").text("text"),
            Some(format!("Range releases {releases}").as_str())
        );
    };
    let press = |input: &mut Interaction, ui: &UiDriver<_>, fraction| {
        let geometry = SliderGeometry::for_node(ui.bounds("band"), ui.node("band"));
        assert!(input.touch_range_press(ui.scene(), ui.layout(), geometry.position(fraction)));
        assert!(input.slider_press(ui.scene(), ui.layout()).is_none());
        geometry
    };
    let drag = |input: &mut Interaction, ui: &mut UiDriver<_>, point| {
        input.pointer_move(ui.scene(), ui.layout(), point);
        assert!(input.slider_drag(ui.scene(), ui.layout()).is_none());
        ui.update(|_| {});
    };
    let release = |input: &mut Interaction, ui: &mut UiDriver<C>, thumb| {
        let release = input.slider_release(ui.scene(), ui.layout()).unwrap();
        assert_eq!(release.path, [path.clone(), vec![thumb]].concat());
        let scene = ui.scene().clone();
        ui.update(|app| assert!(app.adjust_range_at(&scene, &release.path, release.action)));
        input.finish_touch_range();
    };

    state(&ui, "Band 50 to 50", "Preview idle", (0, 0));
    let track = press(&mut input, &ui, 0.5);
    let center = track.position(0.5);
    drag(
        &mut input,
        &mut ui,
        Point {
            x: center.x + 3.0,
            y: center.y + 5.0,
        },
    );
    assert!(!input.has_slider_preview());
    drag(&mut input, &mut ui, track.position(0.28));
    state(&ui, "Band 50 to 50", "Preview lower", (0, 0));
    let (thumb, fraction) = input.slider_preview_position().unwrap();
    assert_eq!(thumb, [path.clone(), vec![0]].concat());
    assert!((fraction - 0.28).abs() < 1e-6);
    release(&mut input, &mut ui, 0);
    state(&ui, "Band 30 to 50", "Preview idle", (1, 1));

    ui.click("mirror-range");
    let track = press(&mut input, &ui, 0.30);
    assert!(track.position(0.30).x > track.position(0.5).x);
    drag(&mut input, &mut ui, track.position(0.18));
    state(&ui, "Band 30 to 50", "Preview lower", (1, 1));
    assert!(input.cancel_slider_preview());
    assert!(input.slider_release(ui.scene(), ui.layout()).is_none());
    input.finish_touch_range();
    ui.update(|_| {});
    state(&ui, "Band 30 to 50", "Preview idle", (1, 1));

    ui.click("reset-range");
    state(&ui, "Band 50 to 50", "Preview idle", (1, 1));
    let track = press(&mut input, &ui, 0.5);
    drag(&mut input, &mut ui, track.position(0.72));
    state(&ui, "Band 50 to 50", "Preview upper", (1, 1));
    release(&mut input, &mut ui, 1);
    state(&ui, "Band 50 to 70", "Preview idle", (2, 2));
    press(&mut input, &ui, 0.7);
    release(&mut input, &mut ui, 1);
    state(&ui, "Band 50 to 70", "Preview idle", (2, 3));
}

#[test]
fn range_touch_preview_and_callbacks_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_range_handles(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_range_handles(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_export_group(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    let verify = |ui: &UiDriver<_>, summary, bulk, items| {
        let (caption, color) = match summary {
            CheckState::Unchecked => ("None selected", Color(0x8e9caeff)),
            CheckState::PartiallyChecked => ("Some selected", Color(0xaa621dff)),
            CheckState::Checked => ("All selected", Color(0x27845fff)),
        };
        assert_eq!(ui.node("export-summary").text("text"), Some(caption));
        let master = ui.node("select-export");
        assert_eq!(master.check_state(), summary);
        assert_eq!(
            master
                .control_part("ToggleIndicator")
                .unwrap()
                .color("background"),
            Some(color)
        );
        assert_eq!(
            ui.node("bulk-edits-label").text("text"),
            Some(format!("Bulk edits {bulk}").as_str())
        );
        assert_eq!(
            ui.node("item-edits-label").text("text"),
            Some(format!("Item edits {items}").as_str())
        );
        assert_eq!(ui.node("group-order").text("text"), Some("Order intact"));
    };
    ui.click("open-export");
    verify(&ui, CheckState::PartiallyChecked, 0, 0);
    assert!(!ui.can_hit("export-manifest"));
    assert!(ui.find_path("export-review").is_err());
    ui.click("select-export");
    verify(&ui, CheckState::Checked, 1, 0);
    assert!(ui.node("export-manifest").boolean("checked", false));
    assert!(ui.node("export-compress").boolean("checked", false));
    ui.click("reveal-review");
    assert_eq!(ui.node("export-review").check_state(), CheckState::Checked);

    let path = ui.path("export-compress");
    let item = ui.layout_at(&path).unwrap();
    let geometry = ToggleGeometry::new(
        ui.node("export-compress"),
        item.bounds,
        item.layout_direction,
    );
    let thumb = geometry.switch_thumb(true, None);
    let start = item.map_to_window(Point {
        x: thumb.x + thumb.width * 0.5,
        y: thumb.y + thumb.height * 0.5,
    });
    let end = Point {
        x: start.x - 26.0,
        ..start
    };
    let now = Instant::now();
    let mut input = Interaction::default();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        now,
        ScrollGestureKind::Touch
    ));
    assert!(input.move_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(80)
    ));
    assert!(input.has_switch_preview());
    verify(&ui, CheckState::Checked, 1, 0);
    assert!(ui.node("export-compress").boolean("checked", false));
    let release = input.end_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        now + Duration::from_millis(150),
    );
    assert!(!release.tap);
    let action = release.control.unwrap();
    assert_eq!(action.path, path);
    assert_eq!(action.action, ControlAction::SetSwitch(false));
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    verify(&ui, CheckState::PartiallyChecked, 1, 1);
    assert_eq!(
        ui.node("compression-result").text("text"),
        Some("Switch reported off")
    );

    ui.click("select-export");
    verify(&ui, CheckState::Checked, 2, 1);
    ui.click("select-export");
    verify(&ui, CheckState::Unchecked, 3, 1);
    assert_eq!(
        ui.node("export-review").check_state(),
        CheckState::Unchecked
    );
    ui.click("export-review");
    verify(&ui, CheckState::PartiallyChecked, 3, 2);
    assert_eq!(
        ui.node("export-review").check_state(),
        CheckState::PartiallyChecked
    );
    ui.click("export-review");
    verify(&ui, CheckState::PartiallyChecked, 3, 3);
    assert_eq!(ui.node("export-review").check_state(), CheckState::Checked);
    ui.click("lock-export");
    for id in [
        "select-export",
        "export-review",
        "export-labels",
        "export-compress",
    ] {
        assert!(
            !ui.can_hit(id),
            "{id} accepted input while the group was locked"
        );
    }
    verify(&ui, CheckState::PartiallyChecked, 3, 3);
    ui.click("lock-export");
    ui.click("export-review");
    verify(&ui, CheckState::Unchecked, 3, 4);
    ui.escape();
    assert!(ui.can_hit("open-export"));
}

#[test]
fn mobile_export_group_preserves_bulk_and_touch_semantics_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_export_group(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_export_group(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_connection_profile(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-connection");
    let collapsed_y = ui.bounds("connection-draft").y;
    assert!(ui.find_path("endpoint").is_err());
    ui.click("details");
    assert_eq!(ui.node("connection-folds").text("text"), Some("Folds 1"));
    let expanded_y = ui.bounds("connection-draft").y;
    assert!(expanded_y - collapsed_y > 80.0);
    ui.click("endpoint");
    assert!(ui.edit_focused(EditAction::Insert("devhost")));
    let stale = ui.scene().clone();
    let apply = ui.path("apply-target");

    ui.click("fold-target");
    assert_eq!(ui.focused(), Some("details"));
    assert_eq!(ui.bounds("connection-draft").y, collapsed_y);
    assert!(ui.find_path("endpoint").is_err());
    assert_eq!(
        ui.node("connection-draft").text("text"),
        Some("Draft devhost")
    );
    assert_eq!(ui.node("connection-folds").text("text"), Some("Folds 1"));
    ui.update(|app| assert!(!app.dispatch_at(&stale, &apply, "clicked")));
    assert_eq!(ui.node("connection-saves").text("text"), Some("Saves 0"));

    assert!(ui.activate(ActivationKey::Enter));
    assert_eq!(ui.node("endpoint").text("text"), Some("devhost"));
    assert_eq!(ui.node("connection-folds").text("text"), Some("Folds 2"));
    ui.click("endpoint");
    let field = ui.bounds("endpoint");
    ui.click("disable-delivery");
    assert_eq!(ui.focused(), Some("delivery"));
    assert_eq!(ui.node("delivery").text("title"), Some("Delivery off"));
    assert_eq!(ui.bounds("connection-draft").y, expanded_y);
    assert_eq!(ui.bounds("endpoint"), field);
    assert!(!ui.can_hit("apply-target"));
    assert_eq!(ui.node("connection-groups").text("text"), Some("Groups 0"));

    assert!(ui.activate(ActivationKey::Space));
    assert_eq!(ui.node("delivery").text("title"), Some("Delivery on"));
    assert_eq!(ui.node("endpoint").text("text"), Some("devhost"));
    assert_eq!(ui.node("connection-groups").text("text"), Some("Groups 1"));
    ui.click("apply-target");
    assert_eq!(ui.node("connection-saves").text("text"), Some("Saves 1"));
    ui.escape();
    assert!(ui.can_hit("open-connection"));
}

#[test]
fn connection_profile_reflows_and_preserves_draft_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_connection_profile(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_connection_profile(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn deliver_lifecycle<C: Component>(ui: &mut UiDriver<C>, input: &mut Interaction) {
    let scene = ui.scene().clone();
    let layout = ui.layout().clone();
    ui.update(|app| {
        input.dispatch_button_press_events(app, &scene, &layout);
    });
}

fn exercise_tag_format<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-dial");
    ui.click("open-tag-format");
    ui.click("asset-tag");
    let path = ui.path("asset-tag");
    let submit = |ui: &mut UiDriver<C>, accepted| {
        ui.update(|app| {
            let scene = app.render_shared();
            assert_eq!(
                app.control_at(&scene, &path, ControlAction::ValidateTextSubmission),
                accepted,
            );
            assert_eq!(app.dispatch_at(&scene, &path, "accepted"), accepted);
        });
    };

    assert!(ui.edit_focused(EditAction::Insert("AB1")));
    assert_eq!(ui.node("tag-draft").text("text"), Some("Draft AB-1"));
    assert_eq!(ui.node("tag-ready").text("text"), Some("Tag incomplete"));
    submit(&mut ui, false);
    assert_eq!(ui.node("tag-saves").text("text"), Some("Saves 0"));
    assert_eq!(ui.node("tag-finishes").text("text"), Some("Finishes 0"));
    assert!(!ui.edit_focused(EditAction::Insert("你好🚰")));
    assert_eq!(ui.node("tag-draft").text("text"), Some("Draft AB-1"));
    assert!(ui.edit_focused(EditAction::Insert("47")));
    assert_eq!(ui.node("tag-ready").text("text"), Some("Tag ready"));
    assert!(!ui.edit_focused(EditAction::ReplaceAll("ab147")));
    assert_eq!(ui.node("tag-draft").text("text"), Some("Draft AB-147"));
    submit(&mut ui, true);
    assert_eq!(ui.node("tag-saved").text("text"), Some("Saved AB-147"));
    assert_eq!(ui.node("tag-saves").text("text"), Some("Saves 1"));
    assert_eq!(ui.node("tag-finishes").text("text"), Some("Finishes 1"));
    assert_eq!(
        ui.node("tag-finish").text("text"),
        Some("Finished after save")
    );
}

#[test]
fn formatted_tag_keeps_invalid_edits_atomic_and_submits_in_order_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_tag_format(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_tag_format(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_button_holds<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-button-holds");
    let center = |ui: &UiDriver<C>, id: &str| {
        let bounds = ui.bounds(id);
        Point {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        }
    };
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut input = Interaction::default();
    input.reconcile(ui.scene(), ui.layout());
    let press = |ui: &mut UiDriver<C>, input: &mut Interaction, point, ms| {
        assert!(input.begin_scroll_gesture(
            ui.scene(),
            ui.layout(),
            point,
            at(ms),
            ScrollGestureKind::Touch,
        ));
        deliver_lifecycle(ui, input);
    };
    let tick = |ui: &mut UiDriver<C>, input: &mut Interaction, ms| {
        let scene = ui.scene().clone();
        let (target, event) = input.advance_button_hold(&scene, ui.layout(), at(ms))?;
        ui.update(|app| assert!(app.dispatch_at(&scene, &target.path, event.handler())));
        deliver_lifecycle(ui, input);
        Some(event)
    };
    let end = |ui: &mut UiDriver<C>, input: &mut Interaction, point, ms, tap| {
        let outcome = input.end_scroll_gesture(ui.scene(), ui.layout(), point, at(ms));
        assert_eq!(outcome.tap, tap);
        deliver_lifecycle(ui, input);
        if tap {
            let path = ui.path("inspect-button");
            let scene = ui.scene().clone();
            ui.update(|app| assert!(app.dispatch_at(&scene, &path, "clicked")));
        }
    };
    let verify = |ui: &UiDriver<C>, labels: &[(&str, &str)]| {
        for &(id, caption) in labels {
            assert_eq!(ui.node(id).text("text"), Some(caption), "{id}");
        }
        assert_eq!(ui.node("hold-order").text("text"), Some("Order intact"));
    };

    let inspect = center(&ui, "inspect-button");
    press(&mut ui, &mut input, inspect, 0);
    verify(
        &ui,
        &[
            ("hold-state", "Inspection held"),
            ("press-count", "Presses 1"),
        ],
    );
    assert_eq!(tick(&mut ui, &mut input, 5999), None);
    end(&mut ui, &mut input, inspect, 5999, true);
    verify(
        &ui,
        &[("release-count", "Releases 1"), ("tap-count", "Taps 1")],
    );

    press(&mut ui, &mut input, inspect, 10000);
    assert_eq!(
        tick(&mut ui, &mut input, 16000),
        Some(ButtonHoldEvent::PressAndHold)
    );
    assert_eq!(tick(&mut ui, &mut input, 18000), None);
    end(&mut ui, &mut input, inspect, 18100, false);
    verify(
        &ui,
        &[
            ("hold-count", "Holds 1"),
            ("release-count", "Releases 2"),
            ("tap-count", "Taps 1"),
        ],
    );

    press(&mut ui, &mut input, inspect, 20000);
    let outside = Point {
        y: inspect.y + 64.0,
        ..inspect
    };
    input.move_scroll_gesture(ui.scene(), ui.layout(), outside, at(20200));
    deliver_lifecycle(&mut ui, &mut input);
    assert_eq!(tick(&mut ui, &mut input, 26000), None);
    end(&mut ui, &mut input, outside, 26100, false);
    verify(
        &ui,
        &[
            ("cancel-count", "Cancels 1"),
            ("hold-count", "Holds 1"),
            ("tap-count", "Taps 1"),
        ],
    );

    press(&mut ui, &mut input, inspect, 30000);
    input.cancel_scroll_gesture();
    deliver_lifecycle(&mut ui, &mut input);
    assert_eq!(tick(&mut ui, &mut input, 36000), None);
    verify(
        &ui,
        &[("cancel-count", "Cancels 2"), ("press-count", "Presses 4")],
    );

    let step = center(&ui, "repeat-step");
    press(&mut ui, &mut input, step, 40000);
    assert_eq!(tick(&mut ui, &mut input, 40449), None);
    for ms in [40450, 40670, 40890] {
        assert_eq!(tick(&mut ui, &mut input, ms), Some(ButtonHoldEvent::Repeat));
    }
    assert_eq!(tick(&mut ui, &mut input, 42000), None);
    end(&mut ui, &mut input, step, 42100, false);
    assert!(!ui.can_hit("repeat-step"));
    verify(
        &ui,
        &[
            ("repeat-count", "Steps 3"),
            ("repeat-state", "Step saw press"),
            ("repeat-cancel-count", "Step cancels 1"),
            ("repeat-release-count", "Step releases 0"),
        ],
    );
}

#[test]
fn button_holds_distinguish_click_repeat_and_cancel_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_button_holds(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_button_holds(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_delay_confirmation<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-button-holds");
    ui.click("open-delay-confirmation");
    let path = ui.path("confirmation");
    let bounds = ui.bounds("confirmation");
    let point = Point {
        x: bounds.x + bounds.width / 2.0,
        y: bounds.y + bounds.height / 2.0,
    };
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut input = Interaction::default();
    input.reconcile(ui.scene(), ui.layout());
    let begin = |ui: &UiDriver<C>, input: &mut Interaction, time| {
        assert!(input.begin_scroll_gesture(
            ui.scene(),
            ui.layout(),
            point,
            at(time),
            ScrollGestureKind::Touch,
        ));
    };
    let verify = |ui: &UiDriver<C>, checked, count| {
        assert_eq!(ui.node("confirmation").boolean("checked", false), checked);
        assert_eq!(ui.node("confirmation-count").text("text"), Some(count));
        assert_eq!(
            ui.node("confirmation-order").text("text"),
            Some("Order intact")
        );
    };

    begin(&ui, &mut input, 0);
    assert_eq!(input.delay_deadline(), Some(at(8200)));
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(4100))
            .is_none()
    );
    assert_eq!(input.delay_progress(&path), Some((0.5, true)));
    let release = input.end_scroll_gesture(ui.scene(), ui.layout(), point, at(4200));
    assert!(!release.tap && release.control.is_none());
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(8200))
            .is_none()
    );
    assert_eq!(input.delay_progress(&path), None);
    verify(&ui, false, "Confirmations 0");

    begin(&ui, &mut input, 9000);
    let moved = Point {
        y: point.y - 124.0,
        ..point
    };
    assert!(input.move_scroll_gesture(ui.scene(), ui.layout(), moved, at(9450)));
    assert!(
        input
            .scroll_offsets()
            .get(&ui.path("choice-scroll"))
            .is_some_and(|&offset| offset > 0.0)
    );
    assert_eq!(input.delay_progress(&path), None);
    let release = input.end_scroll_gesture(ui.scene(), ui.layout(), moved, at(9500));
    assert!(!release.tap && release.control.is_none());

    ui.click("schedule-lock");
    input = Interaction::default();
    input.reconcile(ui.scene(), ui.layout());
    begin(&ui, &mut input, 18000);
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(22100))
            .is_none()
    );
    assert_eq!(input.delay_progress(&path), Some((0.5, true)));
    ui.advance_time(Duration::from_millis(7000));
    assert!(!ui.can_hit("confirmation"));
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(26200))
            .is_none()
    );
    assert_eq!(input.delay_progress(&path), None);
    input.end_scroll_gesture(ui.scene(), ui.layout(), point, at(26300));
    verify(&ui, false, "Confirmations 0");
    ui.click("lock-confirmation");

    begin(&ui, &mut input, 28000);
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(36199))
            .is_none()
    );
    let action = input
        .advance_delay(ui.scene(), ui.layout(), at(36200))
        .unwrap();
    assert_eq!(action.path, path);
    assert_eq!(action.action, ControlAction::DelayButton(true));
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &action.path, action.action)));
    let release = input.end_scroll_gesture(ui.scene(), ui.layout(), point, at(36500));
    assert!(!release.tap && release.control.is_none());
    verify(&ui, true, "Confirmations 1");
    begin(&ui, &mut input, 37000);
    let reset = input.end_scroll_gesture(ui.scene(), ui.layout(), point, at(37100));
    assert!(reset.tap && reset.control.is_none());
    let scene = ui.scene().clone();
    ui.update(|app| assert!(app.control_at(&scene, &path, ControlAction::DelayButton(false))));
    verify(&ui, false, "Confirmations 1");

    begin(&ui, &mut input, 38000);
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(42100))
            .is_none()
    );
    assert_eq!(input.delay_progress(&path), Some((0.5, true)));
    input.cancel_scroll_gesture();
    assert_eq!(input.delay_deadline(), None);
    assert_eq!(input.delay_progress(&path), None);
    assert!(
        input
            .advance_delay(ui.scene(), ui.layout(), at(46200))
            .is_none()
    );
    verify(&ui, false, "Confirmations 1");
    ui.escape();
    assert!(ui.can_hit("open-delay-confirmation"));
}

#[test]
fn delay_confirmation_shares_touch_progress_and_cancel_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_delay_confirmation(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_delay_confirmation(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_click_routes<C: Component>(app: C) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-button-holds");
    ui.click("open-click-routes");
    let path = ui.path("pair-target");
    let bounds = ui.bounds("pair-target");
    let start = Instant::now();
    let mut input = Interaction::default();
    input.reconcile(ui.scene(), ui.layout());
    let touch = |ui: &mut UiDriver<C>, input: &mut Interaction, offset, millis| {
        let point = Point {
            x: bounds.x + bounds.width / 2.0 + offset,
            y: bounds.y + bounds.height / 2.0,
        };
        let now = start + Duration::from_millis(millis);
        assert!(input.begin_scroll_gesture(
            ui.scene(),
            ui.layout(),
            point,
            now,
            ScrollGestureKind::Touch,
        ));
        deliver_lifecycle(ui, input);
        let end = now + Duration::from_millis(40);
        assert!(
            input
                .end_scroll_gesture(ui.scene(), ui.layout(), point, end)
                .tap
        );
        deliver_lifecycle(ui, input);
        let scene = ui.scene().clone();
        let target = input
            .accessibility_target(&scene, ui.layout(), &path)
            .unwrap();
        let click = ButtonClick::new(&scene, &target, true, (point, end)).unwrap();
        ui.update(|app| assert!(app.dispatch_at(&scene, &path, "clicked")));
        click
    };
    let verify = |ui: &UiDriver<C>, labels: &[(&str, &str)]| {
        for &(id, caption) in labels {
            assert_eq!(ui.node(id).text("text"), Some(caption), "{id}");
        }
        assert_eq!(ui.node("pair-order").text("text"), Some("Order intact"));
    };

    let first = touch(&mut ui, &mut input, 0.0, 0);
    let second = touch(&mut ui, &mut input, 0.0, 160);
    assert!(second.follows(&first));
    let scene = ui.scene().clone();
    let layout = ui.layout().clone();
    ui.update(|app| assert!(second.deliver(app, &input, &scene, &layout)));
    verify(
        &ui,
        &[
            ("pair-count", "Pairs 1"),
            ("paired-after", "Paired after 2"),
            ("pair-commands", "Commands 2"),
        ],
    );
    let first = touch(&mut ui, &mut input, -12.0, 1000);
    let second = touch(&mut ui, &mut input, 12.0, 1160);
    assert!(!second.follows(&first));
    ui.click("sync-pair-click");
    verify(
        &ui,
        &[("pair-commands", "Commands 5"), ("pair-count", "Pairs 1")],
    );

    let handle = ui.node("pair-target").button_press_handle.clone().unwrap();
    let mut animations = ButtonAnimations::default();
    animations.sync([&handle]);
    ui.click("animate-pair");
    assert!(handle.pressed());
    let deadline = animations.deadline().unwrap();
    let scene = ui.scene().clone();
    assert!(
        animations
            .next(&scene, deadline - Duration::from_millis(1))
            .is_none()
    );
    let release = animations.next(&scene, deadline).unwrap();
    ui.update(|app| release.apply(app, &scene));
    assert!(!handle.pressed());
    verify(
        &ui,
        &[
            ("pair-commands", "Commands 6"),
            ("pair-count", "Pairs 1"),
            ("pair-releases", "Releases 6"),
        ],
    );

    ui.click("cancel-pair");
    let scene = ui.scene().clone();
    let due = animations.deadline().unwrap();
    let canceled = animations
        .next(&scene, due - Duration::from_millis(1))
        .unwrap();
    ui.update(|app| canceled.apply(app, &scene));
    assert!(!handle.pressed());
    assert!(animations.deadline().is_none());
    assert!(!ui.can_hit("pair-target"));
    verify(
        &ui,
        &[
            ("pair-presses", "Presses 7"),
            ("pair-releases", "Releases 6"),
            ("pair-cancels", "Cancels 1"),
            ("pair-commands", "Commands 6"),
            ("pair-count", "Pairs 1"),
        ],
    );
}

#[test]
fn click_routes_preserve_action_and_animation_order_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_click_routes(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_click_routes(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}

fn exercise_password_panel(app: impl Component) {
    let mut ui = UiDriver::headless(app, Size::new(390.0, 740.0));
    ui.click("open-button-holds");
    ui.click("open-password");
    ui.click("clipboard-entry");
    ui.click("clear-probe");
    assert_eq!(ui.focused(), Some("clipboard-entry"));
    assert_eq!(ui.node("probe-result").text("text"), Some("Probe empty"));
    ui.click("password-entry");
    assert!(ui.edit_focused(EditAction::Insert("moss7")));
    let entry = ui.node("password-entry");
    assert!(entry.is_password() && entry.password_masked());
    assert_eq!(entry.text("password-character"), Some("X"));
    assert_eq!(ui.node("password-result").text("text"), Some("Bound match"));
    ui.click("reveal-password");
    assert_eq!(ui.focused(), Some("password-entry"));
    let entry = ui.node("password-entry");
    assert!(entry.is_password() && !entry.password_masked());
    assert_eq!(entry.text("text"), Some("moss7"));
    ui.click("reveal-password");
    assert!(ui.node("password-entry").password_masked());
    assert_eq!(ui.node("password-result").text("text"), Some("Bound match"));
}

#[test]
fn password_panel_preserves_its_binding_and_type_when_revealed_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_password_panel(MobileControls::new());
    let module = xrack_compiler::compile(include_str!("controls.xr"), "controls.xr")?;
    exercise_password_panel(load_mlir(&module.mlir)?.create("MobileControls")?);
    Ok(())
}
