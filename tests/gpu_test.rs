#![cfg(test)]

mod gpu_readback;
mod gpu_text_scale;

#[test]
fn refresh_indicator_animates_in_both_backends_and_respects_reduced_motion() {
    use std::time::Duration;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut aot = list_ui::RefreshDemo::new();
    aot.invoke_refresh();
    let interpreted = interpret_example("examples/list/list.xr")
        .create("RefreshDemo")
        .unwrap();
    interpreted.invoke("refresh", &[]).unwrap();

    let scene = aot.render_shared();
    let mut input = Interaction::default();
    let first = renderer
        .snapshot_at(&scene, 390, 540, 1.0, &input, Duration::ZERO)
        .unwrap();
    let next = renderer
        .snapshot_at(&scene, 390, 540, 1.0, &input, Duration::from_millis(600))
        .unwrap();
    assert_ne!(first.rgba, next.rgba);
    let other = renderer
        .snapshot_at(
            &interpreted.render(),
            390,
            540,
            1.0,
            &input,
            Duration::from_millis(600),
        )
        .unwrap();
    assert_same_pixels(&next, &other);

    input.set_reduce_motion(true);
    let still = renderer
        .snapshot_at(&scene, 390, 540, 1.0, &input, Duration::from_millis(600))
        .unwrap();
    assert_same_pixels(&still, &first);
}

use async_ui::AsyncDemo;
use bundled_ui::BundledFontsDemo;
use controls_ui::{
    CalendarAgendaDemo, CalendarDemo, CheckStateDemo, ColorDemo, ControlsDemo, DecimalSpinDemo,
    DelayDemo, FontPickerDemo, GroupBoxDemo, IndicatorsDemo, MonthGridDemo, SnapSlidersDemo,
    ThemeDemo, TimeDemo, TimerDemo, TumblerDemo,
};
use counter_ui::Counter;
use delegates_ui::{DelegatesDemo, SwipeDemo};
use dialog_ui::{DialogDemo, PopupDemo, StandardDialogDemo};
use form_ui::{BidiEditorDemo, DragDropDemo, FileDropDemo, FormDemo, PasswordDemo};
use layout_ui::{
    AnimationsDemo, FlexDemo, FlowDemo, FontsDemo, LabelsDemo, LayoutDemo, StatesDemo,
};
use list_ui::{DirectoryItem, GridDemo, ItemDelegatesDemo, ListDemo, StandardListDemo};
use media_ui::MediaDemo;
use menu_ui::{ActionGroupDemo, MenuDemo};
use navigation_ui::{NavigationDemo, PageDemo, StackDemo, StackGuardDemo, TabBarDemo};
use notes_ui::NotesDemo;
use painting_ui::Gallery;
use records_ui::RecordTableDemo;
use selection_ui::{EditableChoiceDemo, SelectionDemo};
use split_ui::SplitDemo;
use table_ui::{InventoryRow, StandardTableDemo, TableDemo, TableSortViewsDemo};
use tooltip_ui::TooltipDemo;
use tree_ui::{TreeBranchesDemo, TreeCommandsDemo, TreeDemo};
use ui_driver::UiDriver;
use xrack_layout::{
    ChoiceKey, DialGeometry, EditAction, EditSource, Interaction, MenuKey, NavigationKey, Point,
    RangeInput, Size, SliderGeometry, SliderKey, SpinGeometry, TextAreaMotion, TextEdit,
    TextMeasurer, TextPosition, font_size,
};
pub use xrack_testing as ui_driver;

fn interpret_example(file: &str) -> xrack_interpreter::Program {
    let source = std::fs::read_to_string(file).unwrap();
    let mut preview = xrack_compiler::prepare_preview(&source, file, Default::default()).unwrap();
    let translations = preview.take_translation_catalog();
    let (program, _) = xrack_interpreter::Program::from_module_with_file_resources(
        &preview.module,
        file,
        translations,
    )
    .unwrap();
    program
}

fn assert_same_pixels(
    actual: &xrack_renderer_femtovg::Snapshot,
    expected: &xrack_renderer_femtovg::Snapshot,
) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert_eq!(actual.rgba.len(), expected.rgba.len());
    if let Some(byte) = actual
        .rgba
        .iter()
        .zip(&expected.rgba)
        .position(|(actual, expected)| actual != expected)
    {
        let index = byte / 4;
        let width = actual.width as usize;
        let (x, y) = ((index % width) as u32, (index / width) as u32);
        panic!(
            "different pixel at ({x}, {y}): interpreted {:?}, compiled {:?}",
            actual.pixel(x, y),
            expected.pixel(x, y)
        );
    }
}

#[test]
fn slotted_content_and_replacement_controls_paint_identically_in_both_backends() {
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let size = Size::new(480.0, 270.0);
    let aot = slots_ui::SlotDemo::new();
    let interpreter = interpret_example("examples/slots/slots.xr")
        .create("SlotDemo")
        .unwrap();
    let (scene, _) = renderer.prepare_scene(&aot, aot.render_shared(), size, &input);
    let aot = renderer.snapshot(&scene, 480, 270, 1.0, &input).unwrap();
    let (scene, _) =
        renderer.prepare_scene(&interpreter, interpreter.render_shared(), size, &input);
    let interpreted = renderer.snapshot(&scene, 480, 270, 1.0, &input).unwrap();
    assert_same_pixels(&interpreted, &aot);
    assert!(
        aot.rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [0xfd, 0x9f, 0x58, 0xff])
    );
}

#[test]
fn custom_spin_indicators_paint_identically_and_keep_the_other_default_arrow() {
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(440.0, 250.0);
    let mut aot = slots_ui::SpinIndicatorDemo::new();
    let interpreter = interpret_example("examples/slots/slots.xr")
        .create("SpinIndicatorDemo")
        .unwrap();
    let text = xrack_renderer_femtovg::TextSystem::system().unwrap();

    for rtl in [false, true] {
        aot.set_rtl(rtl);
        interpreter.set("rtl", rtl.into()).unwrap();
        let mut input = Interaction::default();
        let (scene, layout) = renderer.prepare_scene(&aot, aot.render_shared(), size, &input);
        let path = [0, 2];
        let price = layout.at_path(&path).unwrap();
        let node = scene.at_path(&path).unwrap();
        let geometry = SpinGeometry::for_layout(price, node);
        let snapshot = renderer.snapshot(&scene, 440, 250, 1.0, &input).unwrap();
        let (interpreted_scene, _) =
            renderer.prepare_scene(&interpreter, interpreter.render_shared(), size, &input);
        let interpreted = renderer
            .snapshot(&interpreted_scene, 440, 250, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreted, &snapshot);

        let paint_at = |rect: xrack_layout::Rect| {
            snapshot.pixel((rect.x + 10.0) as u32, (rect.y + 8.0) as u32)
        };
        assert_eq!(paint_at(geometry.up), Some([0x26, 0x3d, 0x50, 0xff]));
        assert_ne!(paint_at(geometry.down), paint_at(geometry.up));
        let target = input.accessibility_target(&scene, &layout, &path).unwrap();
        input.accessibility_focus(target.clone());
        input.sync_editor(&scene);
        let caret = text
            .editor_ime_area(&mut input, &scene, &layout, true)
            .unwrap();
        assert_eq!(caret.x > geometry.text.x + geometry.text.width * 0.5, rtl);
        assert!(
            caret.x >= geometry.text.x
                && caret.x + caret.width <= geometry.text.x + geometry.text.width
        );
        let button = Point {
            x: geometry.up.x + 4.0,
            y: geometry.up.y + 4.0,
        };
        assert!(
            xrack_renderer_femtovg::PointerCaret::new(
                &input, &scene, &layout, &target, button, button, false
            )
            .is_none()
        );
        let entry = Point {
            x: caret.x,
            y: caret.y + caret.height * 0.5,
        };
        let caret_hit = xrack_renderer_femtovg::PointerCaret::new(
            &input, &scene, &layout, &target, entry, entry, false,
        )
        .unwrap();
        assert!(caret_hit.place(&text, &mut input, &scene, &layout));
    }
}

#[test]
fn custom_range_handle_follows_the_same_rendered_preview_in_both_backends() {
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(440.0, 300.0);
    let mut aot = slots_ui::RangeHandleDemo::new();
    let interpreter = interpret_example("examples/slots/slots.xr")
        .create("RangeHandleDemo")
        .unwrap();
    fn drag(renderer: &mut Renderer, ui: &impl Component, size: Size) -> (Interaction, f32) {
        let mut input = Interaction::default();
        let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        let track = SliderGeometry::for_node(
            layout.at_path(&[0, 1]).unwrap().bounds,
            scene.find("band").unwrap(),
        );
        input.pointer_move(&scene, &layout, track.position(0.2));
        input.pointer_press_in_scene(&scene, &layout);
        assert!(input.slider_press(&scene, &layout).is_none());
        let next = track.position(0.48);
        input.pointer_move(&scene, &layout, next);
        assert!(input.slider_drag(&scene, &layout).is_none());
        (input, next.x)
    }
    let (base, original) =
        renderer.prepare_scene(&aot, aot.render_shared(), size, &Interaction::default());
    let original = original.at_path(&[0, 1, 0]).unwrap().bounds;
    assert_eq!(
        base.at_path(&[0, 0]).unwrap().text("text"),
        Some("Budget: 20 – 80")
    );
    let (mut input, expected) = drag(&mut renderer, &aot, size);
    let (mut interpreted_input, _) = drag(&mut renderer, &interpreter, size);

    let (scene, layout) = renderer.prepare_scene(&aot, aot.render_shared(), size, &input);
    assert_eq!(
        scene.at_path(&[0, 0]).unwrap().text("text"),
        Some("Budget: 20 – 80")
    );
    let moved = layout.at_path(&[0, 1, 0]).unwrap().bounds;
    assert!((moved.x + moved.width * 0.5 - expected).abs() < 0.01);
    assert!(moved.x > original.x);
    let painted = renderer
        .snapshot_prepared(&scene, layout, 440, 300, 1.0, &input)
        .unwrap();
    let (scene, layout) = renderer.prepare_scene(
        &interpreter,
        interpreter.render_shared(),
        size,
        &interpreted_input,
    );
    let interpreted = renderer
        .snapshot_prepared(&scene, layout, 440, 300, 1.0, &interpreted_input)
        .unwrap();
    assert_same_pixels(&interpreted, &painted);

    let center = |rect: xrack_layout::Rect| {
        painted.pixel(
            (rect.x + rect.width * 0.5) as u32,
            (rect.y + rect.height * 0.5) as u32,
        )
    };
    let custom = Some([0x27, 0x7f, 0x93, 0xff]);
    assert_eq!(center(moved), custom);
    let old_edge = Point {
        x: original.x + 6.0,
        y: original.y + original.height * 0.5,
    };
    assert!(!moved.contains(old_edge));
    assert_ne!(painted.pixel(old_edge.x as u32, old_edge.y as u32), custom);
    assert!((aot.get_preview_lower() - 0.48).abs() < 1e-6);
    assert_eq!(aot.get_low(), 20.0);
    assert!(input.cancel_slider_preview());
    assert_eq!(aot.get_preview_lower(), 0.2);
    assert!(interpreted_input.cancel_slider_preview());
    assert_eq!(interpreter.get("preview-lower"), Ok(0.2_f64.into()));
    aot.set_rtl(true);
    interpreter.set("rtl", true.into()).unwrap();
    let mut observers = xrack_layout::LayoutObservers::default();
    let (scene, layout) = renderer.prepare_scene(&aot, aot.render_shared(), size, &input);
    observers.sync(&scene, &layout);
    assert_eq!(aot.get_horizontal_visual(), 0.8);
    assert!((aot.get_vertical_visual() - 0.3).abs() < 1e-6);
    let (scene, layout) = renderer.prepare_scene(
        &interpreter,
        interpreter.render_shared(),
        size,
        &interpreted_input,
    );
    observers.sync(&scene, &layout);
    assert_eq!(interpreter.get("horizontal-visual"), Ok(0.8_f64.into()));
}

#[test]
fn repeated_tab_content_paints_the_revealed_selection_identically_in_both_backends() {
    use xrack_runtime::{Component, Value};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(432.0, 246.0);
    let input = Interaction::default();
    let mut aot = slots_ui::TabContentDemo::new();
    let interpreter = interpret_example("examples/slots/slots.xr")
        .create("TabContentDemo")
        .unwrap();
    for (requested, selected) in [(2, 2), (1, 0)] {
        aot.set_selected(requested);
        interpreter.set("selected", Value::Int(requested)).unwrap();
        let (scene, layout) = renderer.prepare_scene(&aot, aot.render_shared(), size, &input);
        let bar = layout.at_path(&[0]).unwrap();
        let art = layout.at_path(&[0, selected, 0, 0]).unwrap().bounds;
        assert_eq!(art.intersect(bar.bounds), art);
        assert_eq!(art.width, 100.0);
        let compiled = renderer
            .snapshot_prepared(&scene, layout, 432, 246, 1.0, &input)
            .unwrap();
        assert_eq!(
            compiled.pixel((art.x + 9.0) as u32, (art.y + art.height * 0.5) as u32),
            Some([0x20, 0x8c, 0x78, 0xff]),
        );
        let (scene, layout) =
            renderer.prepare_scene(&interpreter, interpreter.render_shared(), size, &input);
        let interpreted = renderer
            .snapshot_prepared(&scene, layout, 432, 246, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreted, &compiled);
        assert_eq!(aot.get_selected(), requested);
        assert_eq!(interpreter.get("selected"), Ok(Value::Int(requested)));
    }
}

#[test]
fn disabled_touch_area_keeps_its_border_and_the_child_buttons_enabled_style() {
    use layout_ui::PointerPassThroughDemo;
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PointerPassThroughDemo::new();
    let input = Interaction::default();
    let (scene, _) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(420.0, 210.0), &input);
    let enabled = renderer.snapshot(&scene, 420, 210, 1.0, &input).unwrap();
    ui.set_overlay_enabled(false);
    let (scene, _) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(420.0, 210.0), &input);
    let disabled = renderer.snapshot(&scene, 420, 210, 1.0, &input).unwrap();
    assert_eq!(enabled.pixel(64, 69), Some([0x81, 0xb5, 0xf3, 255]));
    assert_eq!(disabled.pixel(64, 69), Some([0x9c, 0xdb, 0xbd, 255]));
    assert_eq!(enabled.pixel(298, 96), disabled.pixel(298, 96));
}

#[test]
fn touch_point_model_draws_each_contact_at_its_coordinates() {
    use layout_ui::PointerDemo;
    use xrack_runtime::{Component, PointerEvent, PointerEventKind};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PointerDemo::new();
    let input = Interaction::default();
    let size = Size::new(420.0, 270.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let begin = |touch_finger_id, point| {
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
                std::time::Instant::now(),
            )
            .unwrap()
    };
    let (first, down) = begin(101, Point { x: 100.0, y: 100.0 });
    assert!(down.apply(&mut ui, &scene));
    let point = Point { x: 215.0, y: 125.0 };
    let (mut second, down) = begin(102, point);
    assert!(down.apply(&mut ui, &scene));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let both = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    assert_eq!(both.pixel(125, 100), Some([0x3a, 0x56, 0x76, 255]));
    assert_eq!(both.pixel(100, 100), Some([0xf6, 0xce, 0x83, 255]));
    assert_eq!(both.pixel(215, 125), Some([0xf6, 0xce, 0x83, 255]));
    let up = second
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
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let one = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    assert_eq!(one.pixel(125, 100), Some([0x25, 0x44, 0x6a, 255]));
    assert_eq!(one.pixel(100, 100), both.pixel(100, 100));
    assert_eq!(one.pixel(215, 125), one.pixel(125, 100));
    assert!(first.cancel(Default::default()).apply(&mut ui, &scene));
}

#[test]
fn touch_area_paints_hover_press_and_release_from_reactive_state() {
    use layout_ui::PointerDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::{
        Component, EventResult, KeyModifiers, Length, PointerEvent, PointerEventKind, WheelEvent,
    };

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PointerDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(420.0, 270.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let area = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let point = Point {
        x: area.x + 70.0,
        y: area.y + 35.0,
    };
    let before = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    let mut hover = HoverObservers::default();
    hover.sync(&input, &scene, &layout, Some(point));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let hovered = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    let now = std::time::Instant::now();
    let (mut sequence, down) = input
        .begin_pointer_sequence(&scene, &layout, point, PointerEvent::default(), now)
        .unwrap();
    assert!(down.apply(&mut ui, &scene));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let after = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    let (x, y) = (point.x as u32, point.y as u32);
    assert_eq!(before.pixel(x + 18, y), Some([0x25, 0x37, 0x4c, 255]));
    assert_eq!(hovered.pixel(x + 18, y), Some([0x2c, 0x42, 0x5b, 255]));
    assert_eq!(after.pixel(x + 18, y), Some([0x25, 0x44, 0x6a, 255]));
    assert_eq!(after.pixel(x, y), Some([0x9c, 0xdb, 0xbd, 255]));
    let deadline = now + std::time::Duration::from_millis(800);
    assert!(!sequence.advance_press_and_hold(
        &input,
        &scene,
        &layout,
        &mut ui,
        deadline - std::time::Duration::from_millis(1)
    ));
    assert!(sequence.advance_press_and_hold(&input, &scene, &layout, &mut ui, deadline));
    assert!(!sequence.advance_press_and_hold(&input, &scene, &layout, &mut ui, deadline));
    assert_eq!(ui.get_holds(), 1);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let held = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    assert_eq!(held.pixel(x + 18, y), Some([0x60, 0x42, 0x77, 255]));
    let up = sequence
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
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let released = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    assert_eq!(released.pixel(x + 18, y), hovered.pixel(x + 18, y));
    assert_eq!(released.pixel(x, y), hovered.pixel(x, y));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    input.pointer_move(&scene, &layout, point);
    let route = input
        .wheel_route(
            &scene,
            &layout,
            WheelEvent {
                delta_y: Length(28.0),
                modifiers: KeyModifiers {
                    control: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Accept);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let tinted = renderer.snapshot(&scene, 420, 270, 1.0, &input).unwrap();
    assert_eq!(tinted.pixel(x + 18, y), Some([0x32, 0x54, 0x49, 255]));
    assert!(input.scroll_offsets().is_empty());
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("pointer-event.png"))
            .unwrap();
    }
}

#[test]
fn wheel_zoom_keeps_the_pointer_anchor_and_paints_the_scaled_canvas() {
    use layout_ui::WheelDemo;
    use xrack_runtime::{Component, EventResult, KeyModifiers, Length, WheelEvent};
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = WheelDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(460.0, 300.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let canvas = layout.at_path(&[1, 0, 0]).unwrap().bounds;
    let before = renderer.snapshot(&scene, 460, 300, 1.0, &input).unwrap();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: canvas.x + 80.0,
            y: canvas.y + 60.0,
        },
    );
    let route = input
        .wheel_route(
            &scene,
            &layout,
            WheelEvent {
                delta_y: Length(-120.0),
                modifiers: KeyModifiers {
                    control: true,
                    ..KeyModifiers::default()
                },
                ..WheelEvent::default()
            },
        )
        .unwrap();
    assert_eq!(route.dispatch(&mut ui, &scene), EventResult::Accept);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let after = renderer.snapshot(&scene, 460, 300, 1.0, &input).unwrap();
    let (x, y) = ((canvas.x + 31.0) as u32, (canvas.y + 47.0) as u32);
    assert_eq!(before.pixel(x, y), Some([0x25, 0x37, 0x4c, 255]));
    assert_eq!(after.pixel(x, y), Some([0x49, 0x69, 0xdf, 255]));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("wheel-zoom.png"))
            .unwrap();
    }
}

#[test]
fn hover_bindings_change_painted_controls_without_replacing_their_geometry() {
    use layout_ui::HoverDemo;
    use xrack_layout::HoverObservers;
    use xrack_runtime::Component;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = HoverDemo::new();
    let input = Interaction::default();
    let size = Size::new(420.0, 240.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let area = layout.at_path(&[1]).unwrap().bounds;
    let button = layout.at_path(&[1, 0, 0, 0]).unwrap().bounds;
    let point = Point {
        x: button.x + 30.0,
        y: button.y + 15.0,
    };
    let before = renderer.snapshot(&scene, 420, 240, 1.0, &input).unwrap();
    for event in HoverObservers::default().sync(&input, &scene, &layout, Some(point)) {
        event.apply(&mut ui, &scene);
    }
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    assert_eq!(layout.at_path(&[1, 0, 0, 0]).unwrap().bounds, button);
    let after = renderer.snapshot(&scene, 420, 240, 1.0, &input).unwrap();
    let sample = ((area.x + 12.0) as u32, (area.y + 40.0) as u32);
    assert_eq!(
        before.pixel(sample.0, sample.1),
        Some([0x25, 0x37, 0x4c, 255])
    );
    assert_eq!(
        after.pixel(sample.0, sample.1),
        Some([0x25, 0x44, 0x6a, 255])
    );
    assert_eq!(
        after.pixel(point.x as u32, point.y as u32),
        Some([0x9c, 0xdb, 0xbd, 255])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("hover-handler.png"))
            .unwrap();
    }
}

#[test]
fn flickable_overshoot_rebounds_and_supports_stationary_edge_feedback() {
    use layout_ui::FlickableDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;
    use xrack_runtime::{Component, Length, ScrollRequests};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    for shift_content in [true, false] {
        let mut ui = FlickableDemo::new();
        ui.set_shift_content(shift_content);
        let scene = ui.render_shared();
        let viewport = Size::new(360.0, 280.0);
        let mut input = Interaction::default();
        let layout = xrack_layout::layout(&scene, viewport, &mut renderer.text);
        let now = Instant::now();
        let start = Point { x: 170.0, y: 66.0 };
        let end = Point { x: 170.0, y: 146.0 };
        let original = renderer.snapshot(&scene, 360, 280, 1.0, &input).unwrap();
        assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
        assert!(input.move_scroll_gesture(&scene, &layout, end, now + Duration::from_millis(20)));
        let shifted = renderer.snapshot(&scene, 360, 280, 1.0, &input).unwrap();
        assert_eq!(original.pixel(170, 20), Some([0x25, 0x33, 0x4c, 0xff]));
        assert_eq!(
            shifted.pixel(170, 20),
            if shift_content {
                Some([0x19, 0x24, 0x38, 0xff])
            } else {
                original.pixel(170, 20)
            }
        );
        assert!(ui.get_overshoot_y().0 < 0.0 && ui.get_dragging());
        assert_eq!(ui.get_y_offset(), Length(0.0));
        assert!(input.scroll_offsets().is_empty());

        if shift_content {
            input.end_scroll_gesture(&scene, &layout, end, now + Duration::from_millis(25));
        } else {
            let requests = ScrollRequests::default();
            let _context = requests.enter();
            ui.invoke_return_to_edge();
            assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
        }
        assert!(ui.get_rebounding());
        assert!(input.advance_scroll_motion(
            &scene,
            &layout,
            Instant::now() + Duration::from_millis(240)
        ));
        let returned = renderer.snapshot(&scene, 360, 280, 1.0, &input).unwrap();
        assert_eq!(returned.pixel(170, 20), original.pixel(170, 20));
        assert_eq!(ui.get_overshoot_y(), Length(0.0));
        assert_eq!(ui.get_y_offset(), Length(0.0));
        assert!(!ui.get_rebounding());
        assert_eq!(ui.get_flicks(), 0);
        input.cancel_scroll_gesture();
    }
}

#[test]
fn flickable_mouse_pan_moves_pixels_and_preserves_wheel_and_child_taps() {
    use layout_ui::FlickableDemo;
    use xrack_runtime::{FlickDirection, Length};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        FlickableDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(360.0, 280.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    assert!(ui.drag_viewport_from("canvas", Point { x: -80.0, y: 0.0 }));
    assert_eq!(ui.component().get_x_offset(), Length(-72.0));
    assert_eq!(ui.component().get_flicks(), 1);
    assert!(!ui.component().get_moving());
    let moved = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    assert_eq!(before.pixel(100, 90), Some([0x25, 0x33, 0x4c, 0xff]));
    assert_eq!(moved.pixel(100, 90), Some([0x77, 0xd6, 0xb3, 0xff]));

    ui.update(|component| component.set_pan_direction(FlickDirection::Vertical));
    assert!(!ui.drag_viewport_from("canvas", Point { x: -40.0, y: 0.0 }));
    assert_eq!(ui.component().get_x_offset(), Length(-72.0));
    assert!(ui.scroll("canvas", Point { x: 20.0, y: 0.0 }));
    assert_eq!(ui.component().get_x_offset(), Length(-92.0));
    ui.update(|component| {
        component.invoke_reset();
        component.set_pan_enabled(false);
    });
    assert_eq!(ui.component().get_x_offset(), Length(0.0));
    assert!(!ui.drag_viewport_from("canvas", Point { x: 0.0, y: -50.0 }));
    ui.click("tap");
    assert_eq!(ui.component().get_taps(), 1);
    assert!(ui.scroll("canvas", Point { x: 0.0, y: 30.0 }));
    assert_eq!(ui.component().get_y_offset(), Length(-30.0));
    assert_eq!(ui.component().get_flicks(), 3);
}

#[test]
fn swipe_handler_moves_the_card_during_preview_and_commits_on_release() {
    use layout_ui::SwipeGestureDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;
    use xrack_runtime::{Component, Length};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let viewport = Size::new(360.0, 280.0);
    let mut ui = UiDriver::new(
        SwipeGestureDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        viewport,
    );
    let before = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    let layout = xrack_layout::layout(ui.scene(), viewport, &mut renderer.text);
    let mut input = Interaction::default();
    let now = Instant::now();
    let start = Point { x: 120.0, y: 105.0 };
    let end = Point { x: 180.0, y: 105.0 };
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        &layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    input.move_scroll_gesture(ui.scene(), &layout, end, now + Duration::from_millis(10));
    ui.update(|component| {
        let scene = component.render_shared();
        for event in input.take_swipe_events() {
            event.apply(component, &scene);
        }
    });
    assert!(ui.component().get_swiping());
    assert_eq!(ui.component().get_shift(), Length(0.0));
    let preview = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    assert_eq!(before.pixel(240, 105), Some([0x25, 0x33, 0x4c, 0xff]));
    assert_eq!(preview.pixel(240, 105), Some([0x77, 0xd6, 0xb3, 0xff]));
    let outcome =
        input.end_scroll_gesture(ui.scene(), &layout, end, now + Duration::from_millis(20));
    assert!(!outcome.tap);
    ui.update(|component| {
        let scene = component.render_shared();
        for event in input.take_swipe_events() {
            event.apply(component, &scene);
        }
    });
    assert!(!ui.component().get_swiping());
    assert_eq!(ui.component().get_shift(), Length(60.0));
    assert_eq!(ui.component().get_swipes(), 1);
    let after = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    assert_eq!(preview.pixel(240, 105), after.pixel(240, 105));

    assert!(input.begin_scroll_gesture(
        ui.scene(),
        &layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    input.move_scroll_gesture(ui.scene(), &layout, end, now + Duration::from_millis(10));
    ui.update(|component| {
        let scene = component.render_shared();
        for event in input.take_swipe_events() {
            event.apply(component, &scene);
        }
        component.invoke_cancel_swipe();
        input.reconcile_swipe(&scene, &layout);
        for event in input.take_swipe_events() {
            event.apply(component, &scene);
        }
    });
    assert!(!ui.component().get_swiping());
    assert_eq!(ui.component().get_cancellations(), 1);
    assert_eq!(ui.component().get_shift(), Length(60.0));
}

#[test]
fn scale_rotate_callbacks_drive_the_rendered_transform_and_reset_live_metrics() {
    use layout_ui::ScaleRotateDemo;
    use xrack_runtime::{Component, Length, ScaleRotateSample};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        ScaleRotateDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(360.0, 280.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    ui.update(|component| {
        let scene = component.render_shared();
        let handle = scene.children[0].scale_rotate_handle.as_ref().unwrap();
        handle.publish(Some(ScaleRotateSample::default()));
        assert!(component.dispatch_at(&scene, &[0], "started"));
        handle.publish(Some(ScaleRotateSample {
            scale: 1.5,
            rotation: 90.0,
            center_x: Length(164.0),
            center_y: Length(110.0),
        }));
        assert!(component.dispatch_at(&scene, &[0], "updated"));
    });
    assert_eq!(ui.component().get_zoom(), 1.5);
    assert_eq!(ui.component().get_angle(), 90.0);
    assert!(ui.component().get_recognizing());
    assert_eq!(ui.component().get_gesture_center_x(), Length(164.0));
    let transformed = renderer
        .snapshot(ui.scene(), 360, 280, 1.0, ui.input())
        .unwrap();
    assert_eq!(before.pixel(160, 105), Some([0xf0, 0xb6, 0x67, 0xff]));
    assert_eq!(transformed.pixel(210, 96), Some([0xf0, 0xb6, 0x67, 0xff]));
    assert_ne!(before.pixel(210, 96), transformed.pixel(210, 96));
    ui.update(|component| {
        let scene = component.render_shared();
        scene.children[0]
            .scale_rotate_handle
            .as_ref()
            .unwrap()
            .publish(None);
        assert!(component.dispatch_at(&scene, &[0], "ended"));
    });
    assert!(!ui.component().get_recognizing());
    assert_eq!(ui.component().get_ends(), 1);
    assert_eq!(ui.component().get_zoom(), 1.5);
}

#[test]
fn standalone_scroll_controls_repaint_clipped_content_before_and_after_snap() {
    use list_ui::StandaloneScrollDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        StandaloneScrollDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(352.0, 272.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 352, 272, 1.0, ui.input())
        .unwrap();
    ui.click("snap-toggle");
    assert!(ui.drag_scroll_bar_with("bar", 0.94, false, |ui| {
        let dragging = renderer
            .snapshot(ui.scene(), 352, 272, 1.0, ui.input())
            .unwrap();
        assert_eq!(dragging.pixel(60, 70), Some([0x26, 0x37, 0x50, 0xff]));
        true
    }));
    let after = renderer
        .snapshot(ui.scene(), 352, 272, 1.0, ui.input())
        .unwrap();
    assert_eq!(before.pixel(60, 70), Some([0x59, 0xbf, 0x94, 0xff]));
    assert_eq!(after.pixel(60, 70), Some([0x72, 0xa9, 0xee, 0xff]));
    assert_ne!(before.pixel(320, 35), after.pixel(320, 35));
    assert_eq!(before.pixel(320, 35), after.pixel(320, 190));
}

#[test]
fn external_scroll_bar_and_indicator_repaint_with_the_canvas() {
    use list_ui::ScrollBarsDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        ScrollBarsDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(352.0, 272.0),
    );
    ui.update(|_| {});
    let before = renderer
        .snapshot(ui.scene(), 352, 272, 1.0, ui.input())
        .unwrap();
    assert!(ui.drag_scroll_bar("vertical-bar", 1.0, false));
    let after = renderer
        .snapshot(ui.scene(), 352, 272, 1.0, ui.input())
        .unwrap();
    assert_eq!(before.pixel(60, 70), Some([0x59, 0xbf, 0x94, 0xff]));
    assert_eq!(after.pixel(60, 70), Some([0x72, 0xa9, 0xee, 0xff]));
    assert_ne!(before.pixel(295, 190), after.pixel(295, 190));
    assert_ne!(before.pixel(320, 35), after.pixel(320, 35));
    assert_eq!(before.pixel(320, 35), after.pixel(320, 190));
    assert_ne!(before.pixel(52, 252), after.pixel(52, 252));
    assert_eq!(before.pixel(52, 252), after.pixel(242, 252));
}

#[test]
fn scroll_position_repaints_the_requested_content_and_virtual_rows() {
    use list_ui::ScrollPositionDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut driver = UiDriver::new(
        ScrollPositionDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(540.0, 300.0),
    );
    let first = renderer
        .snapshot(driver.scene(), 540, 300, 1.0, driver.input())
        .unwrap();
    driver.click("jump");
    let jumped = renderer
        .snapshot(driver.scene(), 540, 300, 1.0, driver.input())
        .unwrap();
    assert_eq!(first.pixel(60, 96), Some([0x26, 0x37, 0x50, 0xff]));
    assert_eq!(jumped.pixel(60, 96), Some([0x46, 0x87, 0xda, 0xff]));
    let rows = driver.node("records");
    assert_eq!(rows.list.unwrap().first_row, 8_998);
    assert!(
        rows.children
            .iter()
            .any(|row| { row.children[0].children[0].text("text") == Some("记录 9000") })
    );
    driver.click("reset");
    assert!(driver.drag_viewport_from("canvas-button", Point { x: -96.0, y: -60.0 }));
    let user_scrolled = renderer
        .snapshot(driver.scene(), 540, 300, 1.0, driver.input())
        .unwrap();
    assert_eq!(user_scrolled.pixel(60, 96), Some([0x49, 0xb9, 0x89, 0xff]));
}

#[test]
fn prepending_rows_preserves_anchored_pixels_and_virtualizes_the_rest() {
    use list_ui::AnchoredListDemo;

    let mut ui = AnchoredListDemo::new();
    ui.set_records(
        (0..1000)
            .map(|rank| DirectoryItem {
                label: rank.to_string(),
                rank,
            })
            .collect(),
    );
    let model = ui.model_records();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut driver = UiDriver::new(
        ui,
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(560.0, 274.0),
    );
    driver.update(|ui| ui.invoke_position(500));
    assert!(driver.scroll("anchored", Point { x: 0.0, y: 7.0 }));
    let before = renderer
        .snapshot(driver.scene(), 560, 274, 1.0, driver.input())
        .unwrap();
    driver.update(|_| {
        assert!(model.insert(
            0,
            DirectoryItem {
                label: "New row".into(),
                rank: -1
            }
        ))
    });
    let after = renderer
        .snapshot(driver.scene(), 560, 274, 1.0, driver.input())
        .unwrap();
    assert_eq!(before.pixel(190, 56), Some([0x26, 0x37, 0x50, 0xff]));
    assert_eq!(after.pixel(190, 56), before.pixel(190, 56));
    assert_eq!(driver.node("anchored").list.unwrap().first_row, 501);
    assert!(driver.node("anchored").children.len() <= 6);
}

#[test]
fn linked_scroll_repaints_both_viewports_from_one_gesture() {
    use list_ui::LinkedScrollDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut driver = UiDriver::new(
        LinkedScrollDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(540.0, 270.0),
    );
    let before = renderer
        .snapshot(driver.scene(), 540, 270, 1.0, driver.input())
        .unwrap();
    assert!(driver.scroll("large", Point { x: 0.0, y: 20.0 }));
    let after = renderer
        .snapshot(driver.scene(), 540, 270, 1.0, driver.input())
        .unwrap();
    assert_eq!(before.pixel(80, 123), Some([0x26, 0x37, 0x50, 0xff]));
    assert_eq!(after.pixel(80, 123), Some([0x49, 0xb9, 0x89, 0xff]));
    assert_eq!(before.pixel(245, 102), Some([0x30, 0x45, 0x65, 0xff]));
    assert_eq!(after.pixel(245, 102), Some([0xef, 0xab, 0x56, 0xff]));
}

#[test]
fn focus_observation_repaints_the_active_scope_and_clears_on_window_blur() {
    use layout_ui::FocusScopesDemo;
    use xrack_layout::FocusObservers;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = FocusScopesDemo::new();
    let mut input = Interaction::default();
    let mut observers = FocusObservers::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 492, 272, 1.0, &input).unwrap();
    input.focus_next(&scene, &before.layout, false);
    for event in observers.sync(&input, &scene, &before.layout, true) {
        event.apply(&mut ui, &scene);
    }
    let active = ui.render_shared();
    let focused = renderer.snapshot(&active, 492, 272, 1.0, &input).unwrap();
    assert_eq!(before.pixel(25, 55), Some([0x26, 0x36, 0x4e, 0xff]));
    assert_eq!(focused.pixel(25, 55), Some([0x35, 0x53, 0x72, 0xff]));
    assert_eq!(focused.pixel(263, 55), before.pixel(263, 55));
    for event in observers.sync(&input, &active, &focused.layout, false) {
        event.apply(&mut ui, &active);
    }
    let blurred = renderer
        .snapshot(&ui.render(), 492, 272, 1.0, &input)
        .unwrap();
    assert_eq!(blurred.pixel(25, 55), before.pixel(25, 55));
}

#[test]
fn preview_window_commands_update_bindings_and_restore_a_minimized_window() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Component, WindowCommand, WindowLifecycle, WindowMetric, WindowStatus};

    #[derive(Clone, Default)]
    struct WindowProbe(std::rc::Rc<WindowLifecycle>);

    impl Component for WindowProbe {
        fn render(&self) -> Node {
            let mut window = Node::new("Window", None);
            window.properties.insert("width", Length(160.0).into());
            window.properties.insert("height", Length(100.0).into());
            window
        }

        fn take_window_commands(&mut self) -> Vec<WindowCommand> {
            self.0.take_commands()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn publish_window_status(&self, status: WindowStatus) {
            self.0.publish(status);
        }
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = WindowProbe::default();
    let mut preview = PreviewScene::new(Box::new(ui.clone()));
    assert!(ui.0.get(WindowMetric::MinimizedKnown));
    ui.0.request(WindowCommand::Maximize);
    preview.render_animated(&mut renderer).unwrap();
    assert!(ui.0.get(WindowMetric::Maximized));
    preview.set_window_active(false);
    assert!(!ui.0.get(WindowMetric::Active));
    assert!(ui.0.get(WindowMetric::Maximized));
    preview.set_window_active(true);
    ui.0.request(WindowCommand::Minimize);
    preview.render_animated(&mut renderer).unwrap();
    assert!(ui.0.get(WindowMetric::Minimized));
    assert!(!ui.0.get(WindowMetric::Active));
    preview.request_window_restore();
    preview.render_animated(&mut renderer).unwrap();
    assert!(!ui.0.get(WindowMetric::Minimized));
    assert!(!ui.0.get(WindowMetric::Maximized));
    assert!(ui.0.get(WindowMetric::Active));
    ui.0.request(WindowCommand::Maximize);
    ui.0.request(WindowCommand::Restore);
    preview.render_animated(&mut renderer).unwrap();
    assert!(!ui.0.get(WindowMetric::Maximized));
}

#[test]
fn preview_retries_cancelled_visible_rows_when_the_base_scene_is_clean() {
    use std::{cell::Cell, rc::Rc};
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{
        Component, Length, ListMetrics, ListRowRenderer, Node, Repeater, VecModel,
    };

    struct ListUi(Rc<Node>);

    impl Component for ListUi {
        fn render(&self) -> Node {
            self.0.as_ref().clone()
        }

        fn render_shared(&self) -> Rc<Node> {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let rows = VecModel::from(vec!["first".to_owned(), "second".to_owned()]);
    let repeater = Repeater::<_, ()>::default();
    let pending = Cell::new(true);
    let mut list = Node::new("ListView", None);
    list.list = Some(ListMetrics {
        row_count: 2,
        first_row: 0,
    });
    list.properties.insert("width", Length(160.0).into());
    list.properties.insert("height", Length(60.0).into());
    list.properties.insert("row-height", Length(24.0).into());
    list.list_row_renderer = Some(ListRowRenderer::new(move |range| {
        repeater.render_range(
            || rows.clone(),
            range,
            |_, item, _| {
                let text = item();
                if text == "second" && pending.replace(false) {
                    assert!(rows.set_row(0, "updated".into()));
                }
                let mut label = Node::new("Text", None);
                label.properties.insert("text", text.into());
                label
            },
        )
    }));
    let mut window = Node::new("Window", None);
    window.properties.insert("width", Length(160.0).into());
    window.properties.insert("height", Length(60.0).into());
    window.children.push(list);
    let mut preview = PreviewScene::new(Box::new(ListUi(Rc::new(window))));
    preview.render_animated(&mut renderer).unwrap();
    let (scene, layout, _) = preview.inspect_frame().unwrap();
    let rows = &scene.children[0].children;
    assert!(!layout.has_missing_list_rows());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].children[0].text("text"), Some("updated"));
    assert_eq!(rows[1].children[0].text("text"), Some("second"));
}

#[test]
fn preview_renders_at_display_density_and_keeps_logical_hit_testing() {
    use xrack_renderer_femtovg::preview::PreviewScene;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = Counter::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let pixel_size = |png: &[u8]| {
        assert_eq!(&png[12..16], b"IHDR");
        [
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
        ]
    };
    assert_eq!(
        pixel_size(preview.render_scaled(&mut renderer, 2.0).unwrap()),
        [640, 360]
    );
    assert_eq!(preview.logical_size(), [320, 180]);
    let layout = renderer.layout(&ui.render(), Size::new(320.0, 180.0));
    let button = layout.children[0].children[2].bounds;
    preview.click(Point {
        x: button.x + button.width * 0.5,
        y: button.y + button.height * 0.5,
    });
    assert_eq!(ui.get_count(), 1);
    assert_eq!(
        pixel_size(preview.render_scaled(&mut renderer, 1.5).unwrap()),
        [480, 270]
    );
    assert_eq!(preview.logical_size(), [320, 180]);
}

#[test]
fn preview_keyboard_capture_and_clipboard_results_share_the_focused_editor() {
    use layout_ui::ScopedKeysDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{KeyEvent, KeyModifiers};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = ScopedKeysDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(Point { x: 350.0, y: 190.0 });
    preview.render_animated(&mut renderer).unwrap();

    let key = |key: &str, text: &str| KeyEvent {
        key: key.into(),
        text: text.into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key("x", "x"), true));
    assert!(preview.keyboard(&mut renderer, &key("x", ""), false));
    assert_eq!(ui.get_accepted_releases(), 1);
    assert_eq!(ui.get_text(), "");
    assert!(preview.keyboard(&mut renderer, &key("a", "a"), true));
    assert_eq!(ui.get_text(), "a");
    preview.render_animated(&mut renderer).unwrap();

    let command = |name| KeyEvent {
        modifiers: KeyModifiers {
            control: true,
            primary: true,
            ..Default::default()
        },
        ..key(name, "")
    };
    assert!(preview.keyboard(&mut renderer, &command("a"), true));
    assert!(preview.keyboard(&mut renderer, &command("x"), true));
    let cut = preview.take_clipboard_request().unwrap();
    assert_eq!(cut.text.as_deref(), Some("a"));
    assert!(!preview.finish_clipboard(cut.id + 1, true, ""));
    assert_eq!(ui.get_text(), "a");
    assert!(preview.finish_clipboard(cut.id, true, ""));
    assert_eq!(ui.get_text(), "");
    preview.render_animated(&mut renderer).unwrap();

    assert!(preview.keyboard(&mut renderer, &command("v"), true));
    let paste = preview.take_clipboard_request().unwrap();
    assert!(paste.text.is_none());
    assert!(preview.finish_clipboard(paste.id, true, "剪贴板"));
    assert_eq!(ui.get_text(), "剪贴板");
}

#[test]
fn preview_context_menu_selects_and_copies_text_from_the_focused_editor() {
    use form_ui::EditorMenuDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{KeyEvent, KeyModifiers};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = EditorMenuDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.open_context_menu(Some(Point { x: 120.0, y: 72.0 })));
    assert!(preview.menu_open());
    preview.render_animated(&mut renderer).unwrap();
    let key = |key: &str| KeyEvent {
        key: key.into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key("End"), true));
    assert!(preview.keyboard(&mut renderer, &key("Enter"), true));
    assert!(!preview.menu_open());
    preview.render_animated(&mut renderer).unwrap();

    let copy = KeyEvent {
        modifiers: KeyModifiers {
            control: true,
            primary: true,
            ..Default::default()
        },
        ..key("c")
    };
    assert!(preview.keyboard(&mut renderer, &copy, true));
    let request = preview.take_clipboard_request().unwrap();
    assert_eq!(request.text.as_deref(), Some(ui.get_document().as_str()));
}

#[test]
fn preview_native_close_guard_waits_for_the_current_dialog_result() {
    use form_ui::CloseGuardDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{NativeDialogOptions, NativeDialogResult, StandardButtonKind};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CloseGuardDemo::new();
    ui.set_dirty(true);
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();

    assert!(ui.dispatch("close-window", "clicked"));
    preview.render_animated(&mut renderer).unwrap();
    assert!(!preview.window_closed());
    let first = preview.begin_native_dialog().unwrap();
    assert!(matches!(
        first.options.as_ref(),
        NativeDialogOptions::Message(_)
    ));
    assert!(preview.begin_native_dialog().is_none());
    assert!(preview.finish_native_dialog(
        first.id,
        NativeDialogResult::Button(StandardButtonKind::Cancel)
    ));
    preview.render_animated(&mut renderer).unwrap();
    assert!(!preview.window_closed() && ui.get_dirty());

    assert!(ui.dispatch("close-window", "clicked"));
    preview.render_animated(&mut renderer).unwrap();
    let current = preview.begin_native_dialog().unwrap();
    assert_ne!(first.id, current.id);
    assert!(
        !preview.finish_native_dialog(first.id, NativeDialogResult::Button(StandardButtonKind::Ok))
    );
    assert!(preview.finish_native_dialog(
        current.id,
        NativeDialogResult::Button(StandardButtonKind::Ok)
    ));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.window_closed() && !ui.get_dirty());
}

#[test]
fn preview_calendar_keyboard_commits_only_the_accepted_popup_draft() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Date, KeyEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CalendarDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let send = |preview: &mut PreviewScene, renderer: &mut Renderer, name: &str, pressed| {
        let event = KeyEvent {
            key: name.into(),
            ..Default::default()
        };
        assert!(preview.keyboard(renderer, &event, pressed));
        preview.render_animated(renderer).unwrap();
    };

    ui.set_picker_open(true);
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.control_navigation_enabled());
    send(&mut preview, &mut renderer, "ArrowRight", true);
    assert_eq!(ui.get_selected(), Date::new(2024, 2, 28).unwrap());
    send(&mut preview, &mut renderer, "Escape", true);
    assert_eq!(ui.get_cancels(), 1);
    assert_eq!(ui.get_selected(), Date::new(2024, 2, 28).unwrap());

    ui.set_picker_open(true);
    preview.render_animated(&mut renderer).unwrap();
    send(&mut preview, &mut renderer, "ArrowRight", true);
    send(&mut preview, &mut renderer, "Tab", true);
    send(&mut preview, &mut renderer, "Tab", true);
    send(&mut preview, &mut renderer, "Enter", true);
    assert_eq!(ui.get_accepts(), 0);
    send(&mut preview, &mut renderer, "Enter", false);
    assert_eq!(ui.get_accepts(), 1);
    assert_eq!(ui.get_selected(), Date::new(2024, 2, 29).unwrap());
}

#[test]
fn expander_reflows_the_group_preserves_drafts_and_rejects_stale_child_input() {
    use controls_ui::ExpanderDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    fn exercise(
        renderer: &mut Renderer,
        mut ui: Box<dyn Component>,
        mut programmatic: impl FnMut(bool),
    ) {
        let group = [1, 1];
        let collapsed = ui.render_shared();
        assert!(ui.control_at(&collapsed, &group, ControlAction::Expander(true)));
        let stale = ui.render_shared();
        assert!(!ui.control_at(&stale, &group, ControlAction::Expander(true)));
        assert!(stale.at_path(&[1, 1, 0, 1]).is_some());
        programmatic(false);
        assert!(!ui.dispatch_at(&stale, &[1, 1, 0, 1], "clicked"));

        let mut preview = PreviewScene::new(ui);
        preview.render_animated(renderer).unwrap();
        let after_y = |preview: &PreviewScene| {
            preview
                .inspect_frame()
                .unwrap()
                .1
                .at_path(&[1, 2])
                .unwrap()
                .bounds
                .y
        };
        let draft = |preview: &PreviewScene| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("after-details")
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        };
        let collapsed_y = after_y(&preview);
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert!(scene.find("endpoint").is_none());
        let header = layout
            .at_path(&group)
            .unwrap()
            .group_header(scene.at_path(&group).unwrap());
        preview.click(Point {
            x: header.x + 10.0,
            y: header.y + 10.0,
        });
        preview.render_animated(renderer).unwrap();
        assert!(after_y(&preview) > collapsed_y);
        let area = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[1, 1, 0, 0])
            .unwrap()
            .bounds;
        preview.click(Point {
            x: area.x + 20.0,
            y: area.y + 10.0,
        });
        preview.render_animated(renderer).unwrap();
        preview.edit(EditAction::SelectAll);
        preview.paste("draft.example");
        preview.render_animated(renderer).unwrap();
        let key = |preview: &mut PreviewScene, renderer: &mut Renderer, name: &str| {
            let event = KeyEvent {
                key: name.into(),
                ..Default::default()
            };
            preview.keyboard(renderer, &event, true);
            preview.keyboard(renderer, &event, false);
            preview.render_animated(renderer).unwrap();
        };
        key(&mut preview, renderer, "F8");
        let (scene, _, focus) = preview.inspect_frame().unwrap();
        assert_eq!(focus, Some(group.as_slice()));
        assert!(scene.find("endpoint").is_none());
        assert_eq!(after_y(&preview), collapsed_y);
        assert_eq!(draft(&preview), "草稿 · draft.example");
        key(&mut preview, renderer, "Enter");
        assert_eq!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("endpoint")
                .unwrap()
                .text("text"),
            Some("draft.example")
        );
        key(&mut preview, renderer, " ");
        assert!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("endpoint")
                .is_none()
        );
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = ExpanderDemo::new();
    let mut handle = aot.as_weak().upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), |value| {
        handle.set_details_open(value)
    });
    assert_eq!(handle.get_expansion_changes(), 4);
    assert_eq!(handle.get_saves(), 0);
    let interpreter = interpret_example("examples/controls/controls.xr")
        .create("ExpanderDemo")
        .unwrap();
    let handle = interpreter.clone();
    exercise(&mut renderer, Box::new(interpreter), |value| {
        handle.set("details-open", value.into()).unwrap()
    });
    assert_eq!(
        handle.get("expansion-changes").unwrap(),
        xrack_runtime::Value::Int(4)
    );
    assert_eq!(handle.get("saves").unwrap(), xrack_runtime::Value::Int(0));
}

#[test]
fn calendar_browser_virtualizes_months_and_keeps_each_grid_fixed() {
    use controls_ui::CalendarBrowserDemo;
    use xrack_layout::{CalendarGeometry, LayoutNode};
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Date, Node};

    fn visible_months(node: &Node, layout: &LayoutNode) -> Vec<Date> {
        if !layout.visible {
            return Vec::new();
        }
        node.date("display-month")
            .filter(|_| node.kind == "MonthGrid")
            .into_iter()
            .chain(node.materialized_children().flat_map(|(index, child)| {
                layout
                    .child_at(index)
                    .map_or_else(Vec::new, |item| visible_months(child, item))
            }))
            .collect()
    }

    fn grid_at<'a>(
        node: &'a Node,
        layout: &'a LayoutNode,
        month: Date,
    ) -> Option<(&'a Node, &'a LayoutNode)> {
        if node.kind == "MonthGrid" && node.date("display-month") == Some(month) {
            return Some((node, layout));
        }
        node.materialized_children()
            .find_map(|(index, child)| grid_at(child, layout.child_at(index)?, month))
    }

    fn exercise(renderer: &mut Renderer, component: Box<dyn Component>, reverse: impl FnOnce()) {
        let date = |year, month, day| Date::new(year, month, day).unwrap();
        let february = date(2024, 2, 1);
        let march = date(2024, 3, 1);
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert_eq!(
            scene.find("months-view").unwrap().list.unwrap().row_count,
            1200
        );
        let before = visible_months(scene, layout);
        assert!(before.contains(&february) && before.contains(&march));
        assert!(before.len() <= 3, "{before:?}");
        let (grid, layout) = grid_at(scene, layout, february).unwrap();
        let geometry = CalendarGeometry::for_layout(grid, layout);
        assert!(
            !geometry
                .days()
                .find(|day| day.date == march)
                .unwrap()
                .enabled
        );
        assert!(
            !geometry
                .days()
                .find(|day| day.date == date(2024, 2, 29))
                .unwrap()
                .enabled
        );
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let (grid, layout) = grid_at(scene, layout, march).unwrap();
        let geometry = CalendarGeometry::for_layout(grid, layout);
        let friday = geometry.days().find(|day| day.date == march).unwrap();
        assert!(friday.enabled);
        preview.click(Point {
            x: friday.bounds.x + 4.0,
            y: friday.bounds.y + 4.0,
        });
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert_eq!(
            grid_at(scene, layout, february).unwrap().0.date("date"),
            Some(march)
        );
        assert_eq!(
            grid_at(scene, layout, march).unwrap().0.date("date"),
            Some(march)
        );
        let initial_row = scene.find("months-view").unwrap().int("current-index");
        let april = date(2024, 4, 1);
        let key = |preview: &mut PreviewScene, renderer: &mut Renderer, name: &str| {
            let event = xrack_runtime::KeyEvent {
                key: name.into(),
                ..Default::default()
            };
            assert!(preview.keyboard(renderer, &event, true));
            preview.keyboard(renderer, &event, false);
            preview.render_animated(renderer).unwrap();
        };
        let focused_month = |preview: &PreviewScene| {
            let (scene, _, focus) = preview.inspect_frame().unwrap();
            focus
                .and_then(|path| scene.at_path(path))
                .and_then(|node| node.date("display-month"))
        };
        key(&mut preview, renderer, "PageDown");
        assert_eq!(focused_month(&preview), Some(april));
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert_eq!(
            grid_at(scene, layout, april).unwrap().0.date("date"),
            Some(march)
        );
        assert_eq!(
            scene.find("months-view").unwrap().int("current-index"),
            initial_row
        );
        key(&mut preview, renderer, "Enter");
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert_eq!(
            grid_at(scene, layout, april).unwrap().0.date("date"),
            Some(april)
        );
        key(&mut preview, renderer, "PageUp");
        assert_eq!(focused_month(&preview), Some(march));
        key(&mut preview, renderer, "PageUp");
        assert_eq!(focused_month(&preview), Some(february));
        key(&mut preview, renderer, "PageUp");
        assert_eq!(focused_month(&preview), Some(february));
        preview.click(Point { x: 155.0, y: 62.0 });
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let (grid, item) = grid_at(scene, layout, march).unwrap();
        let geometry = CalendarGeometry::for_layout(grid, item);
        let monday = geometry
            .days()
            .find(|day| day.date == date(2024, 3, 4))
            .unwrap();
        preview.click(Point {
            x: monday.bounds.x + 4.0,
            y: monday.bounds.y + 4.0,
        });
        preview.render_animated(renderer).unwrap();
        let label = |preview: &PreviewScene| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .child_at(0)
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        };
        assert_eq!(label(&preview), "请选择终点 · 2024-03-04");
        key(&mut preview, renderer, "PageUp");
        key(&mut preview, renderer, "End");
        assert_eq!(label(&preview), "请选择终点 · 2024-03-04");
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let (grid, item) = grid_at(scene, layout, february).unwrap();
        let geometry = CalendarGeometry::for_layout(grid, item);
        let leap_day = geometry
            .days()
            .find(|day| day.date == date(2024, 2, 29))
            .unwrap();
        assert!(leap_day.highlight.in_range);
        assert!(!leap_day.enabled);
        key(&mut preview, renderer, "Enter");
        assert_eq!(label(&preview), "区间 · 2024-02-28 → 2024-03-04");
        preview.click(Point { x: 280.0, y: 62.0 });
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let after = visible_months(scene, layout);
        assert_eq!(after.first(), Some(&date(2080, 2, 1)), "{after:?}");
        assert!(after.len() <= 3);
        reverse();
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert!(visible_months(scene, layout).is_empty());
        assert!(!scene.find("jump-month").unwrap().boolean("enabled", true));
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let reversed = Date::new(2020, 1, 1).unwrap();
    let aot = CalendarBrowserDemo::new();
    assert_eq!(aot.get_jump_index(), 672);
    let weak = aot.as_weak();
    exercise(&mut renderer, Box::new(aot), || {
        weak.upgrade().unwrap().set_range_end(reversed)
    });
    let interpreter = interpret_example("examples/controls/controls.xr")
        .create("CalendarBrowserDemo")
        .unwrap();
    let host = interpreter.clone();
    exercise(&mut renderer, Box::new(interpreter), || {
        host.set("range-end", reversed.into()).unwrap()
    });
}

#[test]
fn date_exclusions_guard_live_calendar_and_picker_routes_in_both_backends() {
    use xrack_layout::CalendarGeometry;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{CalendarAction, Date, KeyEvent, VecModel};

    fn exercise(
        renderer: &mut Renderer,
        mut component: Box<dyn Component>,
        weekdays: VecModel<i64>,
        closed_dates: VecModel<Date>,
    ) {
        let date = |day| Date::new(2024, 3, day).unwrap();
        let before = component.render_shared();
        assert!(
            xrack_runtime::CalendarState::from_node(before.find("calendar").unwrap())
                .allows(date(2))
        );
        weekdays.push(6);
        weekdays.push(7);
        let saturday = ControlAction::Calendar(CalendarAction::Select(date(2)));
        assert!(!component.control_at(&before, &[1], saturday));
        let sunday = component.render_shared();
        closed_dates.push(date(4));
        let monday = ControlAction::Calendar(CalendarAction::Select(date(4)));
        assert!(!component.control_at(&sunday, &[1], monday));
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let grid = CalendarGeometry::for_layout(
            scene.find("calendar").unwrap(),
            layout.at_path(&[1]).unwrap(),
        );
        let weekend = grid.days().find(|day| day.date == date(2)).unwrap();
        assert!(!weekend.enabled);
        preview.click(Point {
            x: weekend.bounds.x + 4.0,
            y: weekend.bounds.y + 4.0,
        });
        preview.render_animated(renderer).unwrap();
        assert_eq!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("calendar")
                .unwrap()
                .date("date"),
            Some(date(1))
        );
        let press = |preview: &mut PreviewScene, renderer: &mut Renderer, key: &str, pressed| {
            assert!(preview.keyboard(
                renderer,
                &KeyEvent {
                    key: key.into(),
                    ..Default::default()
                },
                pressed
            ));
            preview.render_animated(renderer).unwrap();
        };
        // Ensure focus is on a selectable day before moving across the weekend.
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let grid = CalendarGeometry::for_layout(
            scene.find("calendar").unwrap(),
            layout.at_path(&[1]).unwrap(),
        );
        let friday = grid.action_bounds(CalendarAction::Select(date(1))).unwrap();
        preview.click(Point {
            x: friday.x + 4.0,
            y: friday.y + 4.0,
        });
        preview.render_animated(renderer).unwrap();
        press(&mut preview, renderer, "ArrowRight", true);
        assert_eq!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("calendar")
                .unwrap()
                .date("date"),
            Some(date(5))
        );
        preview.click(Point { x: 40.0, y: 410.0 });
        preview.render_animated(renderer).unwrap();
        closed_dates.push(date(5));
        preview.render_animated(renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let picker = scene.find("picker").unwrap();
        assert!(picker.is_visible());
        assert!(!layout.at_path(&[3, 1, 1]).unwrap().enabled);
        assert_eq!(picker.children[0].date("date"), Some(date(5)));
        let grid =
            CalendarGeometry::for_layout(&picker.children[0], layout.at_path(&[3, 0]).unwrap());
        assert_eq!(grid.cursor(), Some(date(6)));
        press(&mut preview, renderer, "ArrowRight", true);
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        assert_eq!(scene.at_path(&[3, 0]).unwrap().date("date"), Some(date(6)));
        assert_eq!(scene.find("calendar").unwrap().date("date"), Some(date(5)));
        assert!(layout.at_path(&[3, 1, 1]).unwrap().enabled);
        press(&mut preview, renderer, "Tab", true);
        press(&mut preview, renderer, "Tab", true);
        press(&mut preview, renderer, "Enter", true);
        press(&mut preview, renderer, "Enter", false);
        let scene = preview.inspect_frame().unwrap().0;
        assert!(!scene.find("picker").unwrap().is_visible());
        assert_eq!(scene.find("calendar").unwrap().date("date"), Some(date(6)));
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let friday = Date::new(2024, 3, 1).unwrap();
    let mut aot = CalendarDemo::new();
    aot.set_selected(friday);
    aot.set_month(friday);
    let weekdays = aot.model_blocked_weekdays();
    let closed_dates = aot.model_closed_dates();
    exercise(&mut renderer, Box::new(aot), weekdays, closed_dates);
    let program = interpret_example("examples/controls/controls.xr");
    let interpreter = program.create("CalendarDemo").unwrap();
    interpreter.set("selected", friday.into()).unwrap();
    interpreter.set("month", friday.into()).unwrap();
    let weekdays = VecModel::default();
    interpreter
        .set(
            "blocked-weekdays",
            xrack_runtime::Value::IntModel(weekdays.clone()),
        )
        .unwrap();
    let closed_dates = interpreter.date_model("closed-dates").unwrap();
    exercise(&mut renderer, Box::new(interpreter), weekdays, closed_dates);
}

#[test]
fn preview_color_hex_blocks_confirmation_until_the_draft_is_valid() {
    use xrack_layout::EditAction;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ColorDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    ui.set_picker_open(true);
    preview.render_animated(&mut renderer).unwrap();
    preview.edit(EditAction::SelectAll);
    preview.edit(EditAction::Insert("#12"));
    preview.render_animated(&mut renderer).unwrap();

    let enter = KeyEvent {
        key: "Enter".into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &enter, true));
    preview.render_animated(&mut renderer).unwrap();
    assert!(ui.get_picker_open());
    assert_eq!(ui.get_accepts(), 0);
    assert_eq!(ui.get_selected(), Color(0x4080c080));

    preview.edit(EditAction::SelectAll);
    preview.edit(EditAction::Insert("#12AB349A"));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.keyboard(&mut renderer, &enter, true));
    assert!(!ui.get_picker_open());
    assert_eq!(ui.get_accepts(), 1);
    assert_eq!(ui.get_selected(), Color(0x12ab349a));
}

#[test]
fn preview_double_press_selects_a_word_and_drag_extends_to_whole_words() {
    use form_ui::EditorSelectionDemo;
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, KeyModifiers, PointerButton, PointerEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = EditorSelectionDemo::new();
    ui.set_document("alpha beta gamma".into());
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let bounds = preview
        .inspect_frame()
        .unwrap()
        .1
        .at_path(&[0, 0])
        .unwrap()
        .bounds;
    preview.click(Point {
        x: bounds.x + 10.0,
        y: bounds.y + 10.0,
    });
    preview.render_animated(&mut renderer).unwrap();

    // Measure the hits using the same shaped caret coordinates as the preview.
    let mut caret_x = |key: &str, word| {
        let event = KeyEvent {
            key: key.into(),
            modifiers: KeyModifiers {
                control: word,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(preview.keyboard_for_platform(&mut renderer, &event, true, false));
        preview.render_animated(&mut renderer).unwrap();
        preview.ime_area().unwrap().x
    };
    let start = caret_x("Home", false);
    let first_end = caret_x("ArrowRight", true);
    let second_end = caret_x("ArrowRight", true);
    let first = Point {
        x: start.midpoint(first_end),
        y: bounds.y + 10.0,
    };
    let second = Point {
        x: first_end.midpoint(second_end),
        y: bounds.y + 10.0,
    };
    preview.click(first);
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "");

    let contact = PointerContact {
        id: 19,
        primary: true,
        event: PointerEvent {
            button: PointerButton::Primary,
            ..Default::default()
        },
    };
    assert!(preview.press_contact(contact, first, false));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "alpha");
    assert!(preview.move_contact(contact, second));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "alpha beta");
    assert!(preview.move_contact(contact, first));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "alpha");
    preview.finish_contact(contact, first, false, false);
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "alpha");
}

#[test]
fn triple_click_and_paragraph_drag_match_both_backends() {
    use form_ui::LongTextSelectionDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::{FocusObservers, Interaction, TextPosition, layout};
    use xrack_renderer_femtovg::{PointerCaret, TextSystem};

    const DOCUMENT: &str = "First row\nSecond row 🚰\nLast row";

    fn exercise<C: Component>(app: C, selected: impl Fn(&C) -> String) {
        let mut text = TextSystem::system().unwrap();
        let scene = app.render_shared();
        let frame = layout(&scene, Size::new(496.0, 342.0), &mut text);
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &frame, &[0]).unwrap();
        input.focus_target(&scene, &frame, target);
        input.sync_editor(&scene);
        let mut pointer_at = |offset| {
            input.editor.as_mut().unwrap().set_selection(
                DOCUMENT,
                offset,
                TextPosition {
                    offset,
                    trailing: false,
                },
            );
            let caret = text
                .editor_ime_area(&mut input, &scene, &frame, true)
                .unwrap();
            Point {
                x: caret.x + 1.0,
                y: caret.y + caret.height / 2.0,
            }
        };
        let middle = pointer_at(DOCUMENT.find("Second").unwrap() + 2);
        let last = pointer_at(DOCUMENT.find("Last").unwrap() + 1);
        let first = pointer_at(1);
        let mut focus = FocusObservers::default();
        let start = Instant::now();
        for (index, expected) in ["", "Second", "Second row 🚰"].into_iter().enumerate() {
            input.pointer_move(&scene, &frame, middle);
            input.pointer_press_in_scene(&scene, &frame);
            let now = start + Duration::from_millis(index as u64 * 85);
            assert!(
                PointerCaret::from_pressed(&input, &scene, &frame, middle, false)
                    .unwrap()
                    .mouse_press(&mut input, now)
                    .place(&text, &mut input, &scene, &frame)
            );
            focus.sync(&input, &scene, &frame, true);
            assert_eq!(selected(&app), expected);
            if index < 2 {
                input.pointer_release_target();
            }
        }
        for (point, expected) in [
            (last, "Second row 🚰\nLast row"),
            (first, "First row\nSecond row 🚰"),
            (middle, "Second row 🚰"),
        ] {
            input.pointer_move(&scene, &frame, point);
            assert!(
                PointerCaret::from_pressed(&input, &scene, &frame, point, true)
                    .unwrap()
                    .during_drag()
                    .place(&text, &mut input, &scene, &frame)
            );
            focus.sync(&input, &scene, &frame, true);
            assert_eq!(selected(&app), expected);
        }
        input.pointer_release_target();
    }

    let mut aot = LongTextSelectionDemo::new();
    aot.set_document(DOCUMENT.into());
    exercise(aot, LongTextSelectionDemo::get_selected);
    let program = interpret_example("examples/form/form.xr");
    let interpreted = program.create("LongTextSelectionDemo").unwrap();
    interpreted
        .set("document", DOCUMENT.to_owned().into())
        .unwrap();
    exercise(interpreted, |component| {
        match component.get("selected").unwrap() {
            Value::String(text) => text,
            value => panic!("expected string, got {value:?}"),
        }
    });
}

#[test]
fn touch_handles_use_the_active_edits_line_geometry_and_selection() {
    use xrack_layout::{Interaction, TextPosition, layout};
    use xrack_renderer_femtovg::TextSystem;
    use xrack_runtime::{Length, Node};

    let mut text = TextSystem::system().unwrap();
    for (kind, value) in [
        ("TextEdit", "Valve\nPressure"),
        ("LineEdit", "Valve pressure"),
    ] {
        let mut scene = Node::new("Window", None);
        let mut field = Node::new(kind, Some("entry"));
        field.properties.insert("text", value.to_owned().into());
        field.properties.insert("width", Length(210.0).into());
        field.properties.insert("height", Length(96.0).into());
        scene.children.push(field);
        let layout = layout(&scene, Size::new(240.0, 140.0), &mut text);
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &layout, &[0]).unwrap();
        input.focus_target(&scene, &layout, target);
        input.sync_editor(&scene);
        input.editor.as_mut().unwrap().set_selection(
            value,
            value.len(),
            TextPosition {
                offset: 0,
                trailing: false,
            },
        );
        assert!(
            text.touch_selection_handles(&scene, &layout, &input)
                .is_empty()
        );

        input.set_touch_selection_handles(true);
        let handles = text.touch_selection_handles(&scene, &layout, &input);
        let [start, end] = handles.as_slice() else {
            panic!("expected two touch selection handles for {kind}");
        };
        assert_eq!(start.position.offset, 0);
        assert_eq!(end.position.offset, value.len());
        assert!(start.center.y < start.caret.y);
        assert!(end.center.y > end.caret.y);
        assert_eq!(end.caret.y > start.caret.y, kind == "TextEdit");
        input.clear_focus();
        assert!(
            text.touch_selection_handles(&scene, &layout, &input)
                .is_empty()
        );
    }
}

#[test]
fn preview_mouse_selection_modes_match_between_interpreter_and_aot() {
    use form_ui::EditorSelectionDemo;
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, KeyModifiers, MouseSelectionMode, PointerButton, PointerEvent};

    fn selected(preview: &PreviewScene) -> &str {
        preview
            .inspect_frame()
            .unwrap()
            .0
            .find("selection-text")
            .unwrap()
            .text("text")
            .unwrap()
            .strip_prefix("选中文字：")
            .unwrap()
    }

    fn exercise(renderer: &mut Renderer, component: Box<dyn Component>, mode: MouseSelectionMode) {
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        let bounds = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[0, 0])
            .unwrap()
            .bounds;
        let y = bounds.y + 10.0;
        preview.click(Point {
            x: bounds.x + bounds.width - 15.0,
            y,
        });
        preview.render_animated(renderer).unwrap();

        let mut caret_x = |key: &str, word: bool, shift: bool| {
            let event = KeyEvent {
                key: key.into(),
                modifiers: KeyModifiers {
                    control: word,
                    shift,
                    ..Default::default()
                },
                ..Default::default()
            };
            assert!(preview.keyboard_for_platform(renderer, &event, true, false));
            preview.render_animated(renderer).unwrap();
            preview.ime_area().unwrap().x
        };
        let start = caret_x("Home", false, false);
        let first_end = caret_x("ArrowRight", true, false);
        let second_end = caret_x("ArrowRight", true, false);
        let first = Point {
            x: start.midpoint(first_end),
            y,
        };
        let second = Point {
            x: first_end.midpoint(second_end),
            y,
        };

        let contact = PointerContact {
            id: 20,
            primary: true,
            event: PointerEvent {
                button: PointerButton::Primary,
                ..Default::default()
            },
        };
        assert!(preview.press_contact(contact, first, false));
        preview.render_animated(renderer).unwrap();
        assert_eq!(selected(&preview), "");
        let press_caret = preview.ime_area().unwrap().x;
        assert!(preview.move_contact(contact, second));
        preview.render_animated(renderer).unwrap();
        match mode {
            MouseSelectionMode::Characters => {
                let selection = selected(&preview);
                assert!(!selection.is_empty() && "alpha beta".contains(selection));
                assert_ne!(selection, "alpha beta");
            }
            MouseSelectionMode::Words => assert_eq!(selected(&preview), "alpha beta"),
            MouseSelectionMode::Caret => {
                assert_eq!(selected(&preview), "");
                assert_eq!(preview.ime_area().unwrap().x, press_caret);
            }
        }
        preview.finish_contact(contact, second, false, false);
        preview.render_animated(renderer).unwrap();

        preview.click(first);
        preview.render_animated(renderer).unwrap();
        let shifted = PointerContact {
            id: 21,
            event: PointerEvent {
                modifiers: KeyModifiers {
                    shift: true,
                    ..Default::default()
                },
                ..contact.event
            },
            ..contact
        };
        assert!(preview.press_contact(shifted, second, false));
        preview.render_animated(renderer).unwrap();
        if mode == MouseSelectionMode::Words {
            assert_eq!(selected(&preview), "alpha beta");
        }
        if mode == MouseSelectionMode::Caret {
            assert_eq!(selected(&preview), "");
        }
        preview.finish_contact(shifted, second, false, false);

        if mode == MouseSelectionMode::Caret {
            for _ in 0..2 {
                preview.click(first);
                preview.render_animated(renderer).unwrap();
                assert_eq!(selected(&preview), "");
            }
            let extend = KeyEvent {
                key: "Home".into(),
                modifiers: KeyModifiers {
                    shift: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            assert!(preview.keyboard_for_platform(renderer, &extend, true, false));
            preview.render_animated(renderer).unwrap();
            assert!(!selected(&preview).is_empty());
        }
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let program = interpret_example("examples/form/form.xr");
    for mode in [
        MouseSelectionMode::Characters,
        MouseSelectionMode::Words,
        MouseSelectionMode::Caret,
    ] {
        let mut aot = EditorSelectionDemo::new();
        aot.set_document("alpha beta gamma".into());
        aot.set_mouse_mode(mode);
        exercise(&mut renderer, Box::new(aot), mode);
        let interpreted = program.create("EditorSelectionDemo").unwrap();
        interpreted
            .set("document", "alpha beta gamma".to_owned().into())
            .unwrap();
        interpreted.set("mouse-mode", mode.into()).unwrap();
        exercise(&mut renderer, Box::new(interpreted), mode);
    }
}

#[test]
fn persistent_text_selection_survives_toolbar_focus_in_both_backends() {
    use form_ui::PersistentSelectionDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;

    fn description(preview: &PreviewScene) -> &str {
        preview
            .inspect_frame()
            .unwrap()
            .0
            .find("selection-text")
            .unwrap()
            .text("text")
            .unwrap()
    }

    fn press(preview: &mut PreviewScene, renderer: &mut Renderer, x: f32, y: f32) {
        preview.click(Point { x, y });
        preview.render_animated(renderer).unwrap();
    }

    fn exercise(renderer: &mut Renderer, ui: Box<dyn Component>, change: impl FnOnce()) {
        let mut preview = PreviewScene::new(ui);
        preview.render_animated(renderer).unwrap();
        press(&mut preview, renderer, 80.0, 153.0);
        assert_eq!(description(&preview), "选中：e\u{301} · 已记录：");
        preview.set_window_active(false);
        preview.render_animated(renderer).unwrap();
        assert_eq!(description(&preview), "选中：e\u{301} · 已记录：");
        preview.set_window_active(true);
        preview.render_animated(renderer).unwrap();
        press(&mut preview, renderer, 228.0, 153.0);
        assert_eq!(description(&preview), "选中：e\u{301} · 已记录：e\u{301}");
        assert_eq!(preview.inspect_frame().unwrap().2, Some(&[2][..]));
        press(&mut preview, renderer, 390.0, 153.0);
        assert_eq!(description(&preview), "选中：e\u{301} · 已记录：e\u{301}");
        assert_eq!(preview.inspect_frame().unwrap().2, Some(&[0][..]));

        press(&mut preview, renderer, 228.0, 153.0);
        change();
        preview.render_animated(renderer).unwrap();
        assert_eq!(description(&preview), "选中： · 已记录：e\u{301}");
        press(&mut preview, renderer, 390.0, 153.0);
        assert_eq!(description(&preview), "选中： · 已记录：e\u{301}");
        preview.edit(EditAction::Insert("!"));
        preview.render_animated(renderer).unwrap();
        let scene = preview.inspect_frame().unwrap().0;
        assert_eq!(
            scene.find("entry").unwrap().text("text"),
            Some("e\u{301}lan!")
        );
        press(&mut preview, renderer, 80.0, 153.0);
        assert_eq!(description(&preview), "选中：e\u{301} · 已记录：e\u{301}");
        press(&mut preview, renderer, 100.0, 196.0);
        assert_eq!(description(&preview), "选中： · 已记录：e\u{301}");
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = PersistentSelectionDemo::new();
    let mut control = aot.as_weak().upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), move || {
        control.set_document("e\u{301}lan".into())
    });
    let interpreted = interpret_example("examples/form/form.xr")
        .create("PersistentSelectionDemo")
        .unwrap();
    let control = interpreted.clone();
    exercise(&mut renderer, Box::new(interpreted), move || {
        control
            .set("document", "e\u{301}lan".to_owned().into())
            .unwrap()
    });
}

#[test]
fn retained_selection_is_painted_after_editor_focus_moves() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = form_ui::PersistentSelectionDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let initial = renderer.snapshot(&scene, 480, 264, 1.0, &input).unwrap();
    let toolbar = input
        .accessibility_target(&scene, &initial.layout, &[2])
        .unwrap();
    input.focus_target(&scene, &initial.layout, toolbar.clone());
    let without = renderer.snapshot(&scene, 480, 264, 1.0, &input).unwrap();
    let editor = input
        .accessibility_target(&scene, &initial.layout, &[0])
        .unwrap();
    input.focus_target(&scene, &initial.layout, editor);
    input.sync_editor(&scene);
    input.apply_text_selection(&scene, xrack_runtime::TextCommand::SelectAll);
    input.focus_target(&scene, &initial.layout, toolbar);
    let retained = renderer.snapshot(&scene, 480, 264, 1.0, &input).unwrap();
    let changed = retained
        .rgba
        .chunks_exact(4)
        .zip(without.rgba.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 100,
        "retained selection changed only {changed} pixels"
    );
}

#[test]
fn long_text_drag_autoscrolls_without_more_pointer_events_in_both_backends() {
    use form_ui::LongTextSelectionDemo;
    use std::time::{Duration, Instant};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{MouseSelectionMode, PointerButton, PointerEvent};

    fn exercise(
        renderer: &mut Renderer,
        component: Box<dyn Component>,
        selected: impl Fn() -> String,
        horizontal: bool,
        mode: MouseSelectionMode,
    ) {
        let mut preview = PreviewScene::new(component);
        preview.advance(renderer);
        let rect = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[0])
            .unwrap()
            .bounds;
        let start = Point {
            x: rect.x + 7.0,
            y: rect.y + 10.0,
        };
        let outside = if horizontal {
            Point {
                x: rect.x + rect.width + 110.0,
                y: start.y,
            }
        } else {
            Point {
                x: start.x + 9.0,
                y: rect.y + rect.height + 70.0,
            }
        };
        let contact = PointerContact {
            id: 31,
            primary: true,
            event: PointerEvent {
                button: PointerButton::Primary,
                ..Default::default()
            },
        };
        assert!(preview.press_contact(contact, start, false));
        let now = Instant::now();
        preview.advance_at(renderer, now);
        assert!(preview.move_contact(contact, outside));
        preview.advance_at(renderer, now);
        let before = selected();
        if mode != MouseSelectionMode::Caret {
            assert!(
                preview.next_frame_delay().is_some(),
                "edge drag needs another frame"
            );
        }

        for frame in 1..=6 {
            preview.advance_at(renderer, now + Duration::from_millis(32 * frame));
        }
        let during = selected();
        if mode == MouseSelectionMode::Caret {
            assert!(during.is_empty());
        } else {
            assert!(!before.is_empty());
            assert!(
                during.len() > before.len(),
                "selection stayed at {during:?}"
            );
            assert!(
                during.starts_with(&before),
                "edge motion lost the selection anchor"
            );
            assert_eq!(during.contains('\n'), !horizontal);
        }

        let stop = now + Duration::from_millis(224);
        if horizontal {
            assert!(preview.move_contact(contact, start));
            preview.advance_at(renderer, stop);
            let inside = selected();
            preview.advance_at(renderer, stop + Duration::from_millis(64));
            assert_eq!(selected(), inside);
        }
        preview.finish_contact(
            contact,
            if horizontal { start } else { outside },
            false,
            false,
        );
        preview.advance_at(renderer, stop + Duration::from_millis(96));
        let released = selected();
        preview.advance_at(renderer, stop + Duration::from_millis(160));
        assert_eq!(selected(), released);
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let program = interpret_example("examples/form/form.xr");
    for (horizontal, mode) in [
        (false, MouseSelectionMode::Characters),
        (true, MouseSelectionMode::Words),
        (false, MouseSelectionMode::Caret),
    ] {
        let mut aot = LongTextSelectionDemo::new();
        aot.set_mouse_mode(mode);
        let control = aot.as_weak().upgrade().unwrap();
        exercise(
            &mut renderer,
            Box::new(aot),
            || control.get_selected(),
            horizontal,
            mode,
        );
        let interpreted = program.create("LongTextSelectionDemo").unwrap();
        interpreted.set("mouse-mode", mode.into()).unwrap();
        let control = interpreted.clone();
        exercise(
            &mut renderer,
            Box::new(interpreted),
            || match control.get("selected").unwrap() {
                Value::String(value) => value,
                value => panic!("expected selected string, got {value:?}"),
            },
            horizontal,
            mode,
        );
    }
}

#[test]
fn touch_selection_handles_autoscroll_until_the_finger_leaves_or_lifts() {
    use form_ui::LongTextSelectionDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::{FocusObservers, Rect, TextPosition, layout};
    use xrack_renderer_femtovg::{PointerCaret, TextSystem};
    use xrack_runtime::{MouseSelectionMode, text_input_value};

    fn exercise(app: impl Component, selected: impl Fn(bool) -> String) {
        let mut text = TextSystem::system().unwrap();
        let scene = app.render_shared();
        let layout = layout(&scene, Size::new(496.0, 342.0), &mut text);
        for (path, horizontal) in [([0], false), ([1], true)] {
            let mut input = Interaction::default();
            let mut focus = FocusObservers::default();
            let target = input.accessibility_target(&scene, &layout, &path).unwrap();
            input.focus_target(&scene, &layout, target.clone());
            input.sync_editor(&scene);
            let value = text_input_value(target.node_in(&scene).unwrap());
            input.editor.as_mut().unwrap().set_selection(
                value,
                0,
                TextPosition {
                    offset: 2,
                    trailing: true,
                },
            );
            input.set_touch_selection_handles(true);
            let handle = text
                .touch_selection_handles(&scene, &layout, &input)
                .pop()
                .unwrap();
            let Rect {
                x,
                y,
                width,
                height,
            } = layout.at_path(&path).unwrap().bounds;
            let edge = if horizontal {
                Point {
                    x: x + width + 75.0,
                    y: handle.caret.y,
                }
            } else {
                Point {
                    x: x + width / 2.0,
                    y: y + height + 75.0,
                }
            };
            let place = |input: &mut Interaction, point| {
                PointerCaret::new(input, &scene, &layout, &target, handle.caret, point, true)
                    .unwrap()
                    .during_touch_drag()
                    .place(&text, input, &scene, &layout)
            };
            assert!(input.pressed.is_none());
            assert!(place(&mut input, edge));
            focus.sync(&input, &scene, &layout, true);
            let initial = selected(horizontal);
            for _ in 0..5 {
                let now =
                    input.text_selection_scroll_deadline().unwrap() + Duration::from_millis(15);
                assert!(PointerCaret::advance_drag_scroll(
                    &text, &mut input, &scene, &layout, now
                ));
            }
            focus.sync(&input, &scene, &layout, true);
            let extended = selected(horizontal);
            assert!(extended.len() > initial.len() && extended.starts_with(&initial));
            assert_eq!(extended.contains('\n'), !horizontal);
            if horizontal {
                assert!(place(&mut input, handle.caret));
                assert!(input.text_selection_scroll_deadline().is_none());
                assert!(place(&mut input, edge));
            }
            input.finish_touch_selection_scroll();
            focus.sync(&input, &scene, &layout, true);
            let released = selected(horizontal);
            assert!(!PointerCaret::advance_drag_scroll(
                &text,
                &mut input,
                &scene,
                &layout,
                Instant::now() + Duration::from_secs(1)
            ));
            focus.sync(&input, &scene, &layout, true);
            assert_eq!(selected(horizontal), released);
        }
    }

    let mut generated = LongTextSelectionDemo::new();
    generated.set_mouse_mode(MouseSelectionMode::Caret);
    let component = generated.as_weak().upgrade().unwrap();
    exercise(generated, |line| {
        if line {
            component.get_line_selected()
        } else {
            component.get_selected()
        }
    });
    let module = interpret_example("examples/form/form.xr");
    let interpreted = module.create("LongTextSelectionDemo").unwrap();
    interpreted
        .set("mouse-mode", MouseSelectionMode::Caret.into())
        .unwrap();
    let state = interpreted.clone();
    exercise(interpreted, |line| {
        match state
            .get(if line { "line-selected" } else { "selected" })
            .unwrap()
        {
            Value::String(value) => value,
            value => panic!("expected selected text, got {value:?}"),
        }
    });
}

#[test]
fn native_edit_menu_tracks_visible_selection_after_relayout_in_both_backends() {
    use form_ui::LongTextSelectionDemo;
    use xrack_layout::{Rect, TextPosition, layout};
    use xrack_renderer_femtovg::TextSystem;
    use xrack_runtime::{Length, text_input_value};

    fn exercise(component: impl Component) {
        let mut text = TextSystem::system().unwrap();
        let mut scene = component.render();
        let size = Size::new(496.0, 346.0);
        let frame = layout(&scene, size, &mut text);
        let mut input = Interaction::default();
        let target = input.accessibility_target(&scene, &frame, &[0]).unwrap();
        input.focus_target(&scene, &frame, target.clone());
        input.sync_editor(&scene);
        let selection = 0..6;
        let caret = TextPosition {
            offset: selection.end,
            trailing: true,
        };
        input.editor.as_mut().unwrap().set_selection(
            text_input_value(&scene.children[0]),
            selection.start,
            caret,
        );
        let first = text
            .editor_menu_anchor(&scene, &frame, &mut input, None)
            .unwrap();
        assert!(first.width > 1.0 && first.height > 1.0);

        let editor = &mut scene.children[0];
        editor.properties.insert("x", Length(49.0).into());
        editor.properties.insert("y", Length(60.0).into());
        let moved = layout(&scene, size, &mut text);
        let anchor = text
            .editor_menu_anchor(&scene, &moved, &mut input, None)
            .unwrap();
        assert!((anchor.x - first.x - 29.0).abs() < 0.1);
        assert!((anchor.y - first.y - 42.0).abs() < 0.1);

        let keyboard = Rect::new(
            anchor.x - 4.0,
            anchor.y - 4.0,
            anchor.width / 2.0 + 4.0,
            anchor.height + 8.0,
        );
        let visible = text
            .editor_menu_anchor(&scene, &moved, &mut input, Some(keyboard))
            .unwrap();
        assert_eq!(visible.intersect(anchor), visible);
        assert_eq!(visible.intersect(keyboard).width, 0.0);
        assert!(
            text.editor_menu_anchor(&scene, &moved, &mut input, Some(anchor))
                .is_none()
        );
        assert_eq!(input.focused.as_ref(), Some(&target));
        assert_eq!(
            input
                .editor
                .as_ref()
                .unwrap()
                .selection(text_input_value(&scene.children[0])),
            selection
        );

        input.apply_text_selection(&scene, xrack_runtime::TextCommand::ClearSelection);
        let caret_anchor = text
            .editor_menu_anchor(&scene, &moved, &mut input, None)
            .unwrap();
        assert_eq!(caret_anchor.width, 1.0);

        scene.children[0]
            .properties
            .insert("y", Length(370.0).into());
        let clipped = layout(&scene, size, &mut text);
        assert!(
            text.editor_menu_anchor(&scene, &clipped, &mut input, None)
                .is_none()
        );
    }

    exercise(LongTextSelectionDemo::new());
    exercise(
        interpret_example("examples/form/form.xr")
            .create("LongTextSelectionDemo")
            .unwrap(),
    );
}

#[test]
fn single_line_drag_scrolls_ltr_and_rtl_in_both_backends() {
    use form_ui::LongTextSelectionDemo;
    use std::time::{Duration, Instant};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, MouseSelectionMode, PointerButton, PointerEvent};

    fn exercise(
        renderer: &mut Renderer,
        ui: Box<dyn Component>,
        selected: impl Fn() -> String,
        rtl: bool,
    ) {
        let mut preview = PreviewScene::new(ui);
        preview.advance(renderer);
        let bounds = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[1])
            .unwrap()
            .bounds;
        let y = bounds.y + bounds.height * 0.5;
        let right = bounds.x + bounds.width;
        if rtl {
            preview.click(Point {
                x: (bounds.x + right) * 0.5,
                y,
            });
            preview.advance(renderer);
            let home = KeyEvent {
                key: "Home".into(),
                ..Default::default()
            };
            assert!(preview.keyboard_for_platform(renderer, &home, true, false));
            preview.advance(renderer);
        }

        let start = Point {
            x: if rtl { right - 7.0 } else { bounds.x + 7.0 },
            y,
        };
        let outside = Point {
            x: if rtl { bounds.x - 104.0 } else { right + 104.0 },
            y,
        };
        let contact = PointerContact {
            id: 39,
            primary: true,
            event: PointerEvent {
                button: PointerButton::Primary,
                ..Default::default()
            },
        };
        let clock = Instant::now();
        assert!(preview.press_contact(contact, start, false));
        preview.advance_at(renderer, clock);
        assert!(preview.move_contact(contact, outside));
        preview.advance_at(renderer, clock);
        let first = selected();
        assert!(!first.is_empty());
        if !rtl {
            assert!(first.starts_with("Project Atlas"), "{first:?}");
        }
        assert!(preview.next_frame_delay().is_some());
        for i in 1..=6 {
            preview.advance_at(renderer, clock + Duration::from_millis(i * 28));
        }
        let expanded = selected();
        assert!(
            expanded.len() > first.len(),
            "rtl={rtl}: {first:?} -> {expanded:?}"
        );
        assert!(expanded.starts_with(&first));
        let caret = preview.ime_area().unwrap();
        preview.finish_contact(contact, outside, false, false);
        preview.advance_at(renderer, clock + Duration::from_millis(220));
        assert_eq!(preview.ime_area(), Some(caret));
        assert_eq!(selected(), expanded);
        preview.advance_at(renderer, clock + Duration::from_millis(280));
        assert_eq!(selected(), expanded);
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let program = interpret_example("examples/form/form.xr");
    let arabic = "نص عربي طويل لاختبار التحديد عند حافة حقل النص ".repeat(12);
    for rtl in [false, true] {
        let mut aot = LongTextSelectionDemo::new();
        let interpreted = program.create("LongTextSelectionDemo").unwrap();
        if rtl {
            aot.set_mouse_mode(MouseSelectionMode::Words);
            aot.set_line(arabic.clone());
            interpreted
                .set("mouse-mode", MouseSelectionMode::Words.into())
                .unwrap();
            interpreted.set("line", arabic.clone().into()).unwrap();
        }
        let control = aot.as_weak().upgrade().unwrap();
        exercise(
            &mut renderer,
            Box::new(aot),
            || control.get_line_selected(),
            rtl,
        );
        let control = interpreted.clone();
        exercise(
            &mut renderer,
            Box::new(interpreted),
            || match control.get("line-selected").unwrap() {
                Value::String(value) => value,
                value => panic!("expected the selected string, got {value:?}"),
            },
            rtl,
        );
    }
}

#[test]
fn pattern_validation_preserves_prefixes_and_rechecks_dynamic_rules_in_both_backends() {
    use form_ui::PatternValidationDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{InputValidity, InputValidity::*, KeyEvent};

    fn exercise(
        renderer: &mut Renderer,
        ui: Box<dyn Component>,
        read: impl Fn() -> (String, bool, i64, InputValidity),
        mut set_pattern: impl FnMut(&str),
    ) {
        let mut preview = PreviewScene::new(ui);
        preview.advance(renderer);
        preview.click(Point { x: 35.0, y: 72.0 });
        preview.advance(renderer);
        let enter = KeyEvent {
            key: "Enter".into(),
            ..Default::default()
        };
        preview.edit(EditAction::Insert("AB-12"));
        preview.advance(renderer);
        assert_eq!(read(), ("AB-12".into(), false, 0, Intermediate));
        preview.keyboard_for_platform(renderer, &enter, true, false);
        preview.advance(renderer);
        assert_eq!(read(), ("AB-12".into(), false, 0, Intermediate));
        preview.edit(EditAction::Insert("x"));
        preview.advance(renderer);
        assert_eq!(read().0, "AB-12");

        assert!(preview.begin_text_composition(renderer, 91));
        assert!(preview.update_text_composition(91, "3".into()));
        preview.advance(renderer);
        assert!(!read().1);
        assert!(preview.finish_text_composition(91, "3"));
        preview.advance(renderer);
        assert_eq!(read(), ("AB-123".into(), true, 0, Acceptable));
        assert!(preview.keyboard_for_platform(renderer, &enter, true, false));
        preview.advance(renderer);
        assert_eq!(read(), ("AB-123".into(), true, 1, Acceptable));

        set_pattern("[甲乙]{2}-[0-9]{3}");
        preview.advance(renderer);
        assert_eq!(read(), ("AB-123".into(), false, 1, Invalid));
        preview.edit(EditAction::SelectAll);
        preview.paste("甲乙-304");
        preview.advance(renderer);
        assert_eq!(read(), ("甲乙-304".into(), true, 1, Acceptable));
        set_pattern("[");
        preview.advance(renderer);
        assert_eq!(read(), ("甲乙-304".into(), false, 1, Invalid));
        set_pattern("");
        preview.advance(renderer);
        assert_eq!(read(), ("甲乙-304".into(), true, 1, Acceptable));
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = PatternValidationDemo::new();
    let control = aot.as_weak().upgrade().unwrap();
    let mut setter = aot.as_weak().upgrade().unwrap();
    exercise(
        &mut renderer,
        Box::new(aot),
        || {
            (
                control.get_code(),
                control.get_ready(),
                control.get_submissions(),
                control.get_validation(),
            )
        },
        |pattern| setter.set_pattern(pattern.into()),
    );
    let interpreted = interpret_example("examples/form/form.xr")
        .create("PatternValidationDemo")
        .unwrap();
    let control = interpreted.clone();
    exercise(
        &mut renderer,
        Box::new(interpreted),
        || {
            let Value::String(code) = control.get("code").unwrap() else {
                panic!("expected code")
            };
            let Value::Bool(ready) = control.get("ready").unwrap() else {
                panic!("expected readiness")
            };
            let Value::Int(submissions) = control.get("submissions").unwrap() else {
                panic!("expected count")
            };
            let Value::InputValidity(validity) = control.get("validation").unwrap() else {
                panic!("expected validity")
            };
            (code, ready, submissions, validity)
        },
        |pattern| control.set("pattern", pattern.to_owned().into()).unwrap(),
    );
}

#[test]
fn text_commands_edit_unfocused_fields_in_order_and_undo_in_both_backends() {
    use form_ui::TextOperationsDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;

    fn exercise(
        renderer: &mut Renderer,
        ui: Box<dyn Component>,
        values: impl Fn() -> (String, String, String),
    ) {
        let mut preview = PreviewScene::new(ui);
        preview.advance(renderer);
        let click = |preview: &mut PreviewScene, renderer: &mut Renderer, x: f32, y: f32| {
            preview.click(Point { x, y });
            preview.advance(renderer);
        };
        click(&mut preview, renderer, 90.0, 141.0);
        assert_eq!(preview.inspect_frame().unwrap().2, Some(&[2][..]));
        assert_eq!(values().0, "Éclair 👩‍💻!");
        assert_eq!(values().2, "[Éclair 👩‍💻][#Éclair 👩‍💻][Éclair 👩‍💻][Éclair 👩‍💻!]");
        click(&mut preview, renderer, 240.0, 141.0);
        assert_eq!(values().1, "2026-12-17");
        assert_eq!(preview.inspect_frame().unwrap().2, Some(&[3][..]));
        for expected in ["Éclair 👩‍💻", "#Éclair 👩‍💻", "Éclair 👩‍💻", "éclair 👩‍💻"]
        {
            click(&mut preview, renderer, 408.0, 141.0);
            assert_eq!(values().0, expected);
        }
        preview.replay_edit(true);
        preview.advance(renderer);
        assert_eq!(values().0, "Éclair 👩‍💻");
        click(&mut preview, renderer, 81.0, 192.0);
        assert_eq!(values().0, "");
        assert_eq!(preview.inspect_frame().unwrap().2, Some(&[5][..]));
        click(&mut preview, renderer, 408.0, 141.0);
        assert_eq!(values().0, "Éclair 👩‍💻");
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = TextOperationsDemo::new();
    let control = aot.as_weak().upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), || {
        (
            control.get_document(),
            control.get_date(),
            control.get_edits(),
        )
    });
    let interpreted = interpret_example("examples/form/form.xr")
        .create("TextOperationsDemo")
        .unwrap();
    let control = interpreted.clone();
    let get = |name| match control.get(name).unwrap() {
        Value::String(value) => value,
        value => panic!("expected string, got {value:?}"),
    };
    exercise(&mut renderer, Box::new(interpreted), || {
        (get("document"), get("date"), get("edits"))
    });
}

#[test]
fn preview_overwrite_and_select_word_match_between_interpreter_and_aot() {
    use form_ui::EditorSelectionDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;

    const ORIGINAL: &str = "e\u{301}👩‍💻 cat";
    fn value(preview: &PreviewScene) -> &str {
        preview
            .inspect_frame()
            .unwrap()
            .0
            .find("entry")
            .unwrap()
            .text("text")
            .unwrap()
    }
    fn exercise(renderer: &mut Renderer, ui: Box<dyn Component>) {
        let mut preview = PreviewScene::new(ui);
        preview.render_animated(renderer).unwrap();
        let bounds = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[0, 0])
            .unwrap()
            .bounds;
        preview.click(Point {
            x: bounds.x + 10.0,
            y: bounds.y + 10.0,
        });
        preview.render_animated(renderer).unwrap();
        preview.edit(EditAction::Home(false));
        preview.edit(EditAction::Insert("A"));
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "A👩‍💻 cat");
        preview.edit(EditAction::Insert("B"));
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "AB cat");
        preview.replay_edit(false);
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), ORIGINAL);
        preview.replay_edit(true);
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "AB cat");

        preview.edit(EditAction::Home(false));
        assert!(preview.begin_text_composition(renderer, 81));
        assert!(preview.update_text_composition(81, "猫".into()));
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "AB cat");
        assert!(preview.finish_text_composition(81, "猫"));
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "猫B cat");
        preview.edit(EditAction::Home(false));
        preview.paste("!");
        preview.render_animated(renderer).unwrap();
        assert_eq!(value(&preview), "!猫B cat");

        preview.edit(EditAction::End(false));
        preview.render_animated(renderer).unwrap();
        let bounds = preview
            .inspect_frame()
            .unwrap()
            .1
            .at_path(&[0, 5, 1])
            .unwrap()
            .bounds;
        preview.click(Point {
            x: bounds.x + 10.0,
            y: bounds.y + 10.0,
        });
        preview.render_animated(renderer).unwrap();
        let scene = preview.inspect_frame().unwrap().0;
        assert_eq!(
            scene.find("selection-text").unwrap().text("text"),
            Some("选中文字：cat")
        );
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut aot = EditorSelectionDemo::new();
    aot.set_document(ORIGINAL.into());
    aot.set_overwrite(true);
    exercise(&mut renderer, Box::new(aot));

    let interpreted = interpret_example("examples/form/form.xr")
        .create("EditorSelectionDemo")
        .unwrap();
    interpreted
        .set("document", ORIGINAL.to_owned().into())
        .unwrap();
    interpreted.set("overwrite", true.into()).unwrap();
    exercise(&mut renderer, Box::new(interpreted));
}

#[test]
fn preview_word_shortcuts_preserve_unicode_and_use_the_clients_platform() {
    use form_ui::EditorSelectionDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{KeyEvent, KeyModifiers};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = EditorSelectionDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let original = "alpha e\u{301}clair 👩‍💻";
    ui.set_document(original.into());

    preview.render_animated(&mut renderer).unwrap();
    let bounds = preview
        .inspect_frame()
        .unwrap()
        .1
        .at_path(&[0, 0])
        .unwrap()
        .bounds;
    preview.click(Point {
        x: bounds.x + 5.0,
        y: bounds.y + 5.0,
    });
    preview.render_animated(&mut renderer).unwrap();
    let event = |key: &str, modifiers| KeyEvent {
        key: key.into(),
        modifiers,
        ..Default::default()
    };
    let end = event("End", KeyModifiers::default());
    assert!(preview.keyboard_for_platform(&mut renderer, &end, true, true));
    assert!(preview.keyboard_for_platform(
        &mut renderer,
        &event(
            "ArrowLeft",
            KeyModifiers {
                alt: true,
                shift: true,
                ..Default::default()
            }
        ),
        true,
        true,
    ));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected(), "👩‍💻");
    assert!(preview.keyboard_for_platform(
        &mut renderer,
        &event(
            "Backspace",
            KeyModifiers {
                alt: true,
                ..Default::default()
            }
        ),
        true,
        true,
    ));
    assert_eq!(ui.get_document(), "alpha e\u{301}clair ");
    preview.render_animated(&mut renderer).unwrap();
    preview.replay_edit(false);
    assert_eq!(ui.get_document(), original);
    preview.render_animated(&mut renderer).unwrap();

    assert!(preview.keyboard_for_platform(&mut renderer, &end, true, false));
    assert!(preview.keyboard_for_platform(
        &mut renderer,
        &event(
            "Backspace",
            KeyModifiers {
                control: true,
                ..Default::default()
            }
        ),
        true,
        false,
    ));
    assert_eq!(ui.get_document(), "alpha e\u{301}clair ");
}

#[test]
fn preview_links_keyboard_tabs_and_middle_button_swipe_pages() {
    use navigation_ui::TabBarDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = TabBarDemo::new();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(Point { x: 64.0, y: 75.0 });
    preview.render_animated(&mut renderer).unwrap();
    let right = KeyEvent {
        key: "ArrowRight".into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &right, true));
    assert_eq!(ui.get_page(), 3);
    preview.render_animated(&mut renderer).unwrap();

    let start = Point { x: 460.0, y: 165.0 };
    let end = Point { x: 225.0, y: 165.0 };
    assert!(preview.begin_pan(start));
    preview.move_pointer(end);
    assert_eq!(ui.get_page(), 3);
    assert!(preview.release_pointer(end, false));
    assert_eq!(ui.get_page(), 4);
    assert_eq!(ui.get_changes(), 1);
    preview.render_animated(&mut renderer).unwrap();
}

#[test]
fn preview_stack_renders_slides_and_routes_keyboard_back_to_the_current_stack() {
    use navigation_ui::StackDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Component, KeyEvent, KeyModifiers};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = StackDemo::new();
    let (_, layout) = renderer.prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(590.0, 324.0),
        &Interaction::default(),
    );
    let button = layout.at_path(&[0, 2, 0, 0, 1]).unwrap().bounds;
    let point = Point {
        x: button.x + button.width / 2.0,
        y: button.y + button.height / 2.0,
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(point);
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.next_frame_delay().is_some());
    assert_eq!(
        ui.stack_snapshot("navigation").unwrap().current(),
        Some("details")
    );

    let back = KeyEvent {
        key: "ArrowLeft".into(),
        modifiers: KeyModifiers {
            alt: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &back, true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(
        ui.stack_snapshot("navigation").unwrap().current(),
        Some("overview")
    );
    assert_eq!(ui.get_navigations(), 2);
    assert!(preview.next_frame_delay().is_some());
}

#[test]
fn preview_selects_a_table_delegate_cell_before_clicking_its_button() {
    use records_ui::DelegateTableDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::Component;

    let ui = DelegateTableDemo::new();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let size = Size::new(620.0, 338.0);
    let (_, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let star = [0, 1, 0, 0, 0, 1];
    let button = layout.at_path(&star).unwrap().bounds;
    let point = Point {
        x: button.x + button.width / 2.0,
        y: button.y + button.height / 2.0,
    };

    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_row(), -1);
    preview.click(point);
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_row(), 0);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    assert_eq!(scene.at_path(&star).unwrap().text("text"), Some("★ 1"));
}

#[test]
fn preview_touch_scroll_cancels_the_captured_area_before_release() {
    use layout_ui::PointerDemo;
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{PointerEvent, PointerEventKind};

    let ui = PointerDemo::new();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let start = Point { x: 80.0, y: 128.0 };
    let end = Point { x: 80.0, y: 67.0 };
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent {
            is_touch: true,
            ..Default::default()
        },
    };
    assert!(preview.press_contact(contact, start, false));
    assert_eq!(ui.get_phase(), PointerEventKind::Down);
    assert_eq!(ui.get_active_touches(), 1);
    assert!(preview.move_contact(contact, end));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_phase(), PointerEventKind::Cancel);
    assert!(preview.finish_contact(contact, end, false, false));
    assert_eq!(ui.get_active_touches(), 0);
    assert_eq!(ui.get_clicks(), 0);
}

#[test]
fn frameless_title_area_covers_the_resized_client_width() {
    use layout_ui::FramelessDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let scene = FramelessDemo::new().render();
    let input = Interaction::default();
    for width in [540, 720] {
        let frame = renderer.snapshot(&scene, width, 320, 1.0, &input).unwrap();
        assert_eq!(frame.pixel(width - 2, 12), Some([0x22, 0x36, 0x50, 0xff]));
        assert_eq!(frame.pixel(width - 2, 64), Some([0x14, 0x1d, 0x2c, 0xff]));
    }
}

#[test]
fn a_state_animates_instance_layout_and_repaints_the_component_input() {
    use layout_ui::ComponentStatesDemo;
    use std::time::Duration;
    use xrack_runtime::SceneTransition;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let mut ui = ComponentStatesDemo::new();
    let start = ui.render();
    let banner = [start
        .children
        .iter()
        .position(|node| node.id == Some("banner"))
        .unwrap()];
    let normal = renderer.snapshot(&start, 520, 306, 1.0, &input).unwrap();
    ui.set_warning(true);
    let warning = ui.render();
    let timeline = SceneTransition::between(&start, &warning);
    let frame = timeline.frame(&warning, Duration::from_millis(130));
    let halfway = renderer.snapshot(&frame, 520, 306, 1.0, &input).unwrap();
    assert_eq!(halfway.layout.at_path(&banner).unwrap().bounds.width, 250.0);
    assert_eq!(normal.pixel(260, 53), Some([0x14, 0x1d, 0x2c, 0xff]));
    assert_eq!(halfway.pixel(260, 53), Some([0xba, 0x4b, 0x5d, 0xff]));
    assert_eq!(halfway.pixel(300, 53), normal.pixel(300, 53));
}

#[test]
fn state_exit_retargets_from_the_current_frame_and_restores_the_live_base() {
    use std::time::Duration;
    use xrack_runtime::{Length, SceneTransition};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let mut ui = StatesDemo::new();
    let start = ui.render();
    let tile = [start
        .children
        .iter()
        .position(|node| node.id == Some("tile"))
        .unwrap()];
    ui.set_level(2);
    let alarm = ui.render();
    let mut timeline = SceneTransition::between(&start, &alarm);
    let time = Duration::from_millis;
    let inside = renderer
        .snapshot(&timeline.frame(&alarm, time(200)), 518, 234, 1.0, &input)
        .unwrap();
    assert_eq!(inside.layout.at_path(&tile).unwrap().bounds.width, 188.0);
    assert_eq!(inside.pixel(36, 70), Some([0xba, 0x4b, 0x5d, 0xff]));
    ui.set_baseline(Length(124.0));
    ui.set_level(0);
    let normal = ui.render();
    timeline.retarget(&alarm, &normal, time(200));
    assert_eq!(
        timeline
            .frame(&normal, time(200))
            .find("tile")
            .unwrap()
            .length("width"),
        Some(188.0)
    );
    let exit = renderer
        .snapshot(&timeline.frame(&normal, time(300)), 518, 234, 1.0, &input)
        .unwrap();
    assert_eq!(exit.layout.at_path(&tile).unwrap().bounds.width, 156.0);
    assert_eq!(
        timeline
            .frame(&normal, time(400))
            .find("tile")
            .unwrap()
            .length("width"),
        Some(124.0)
    );
    assert!(!timeline.running(time(400)));
}

#[test]
fn easing_delay_and_independent_updates_render_on_one_scene_clock() {
    use std::time::Duration;
    use xrack_runtime::SceneTransition;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = AnimationsDemo::new();
    let input = Interaction::default();
    let start = ui.render();
    let find = |id| {
        [start
            .children
            .iter()
            .position(|node| node.id == Some(id))
            .unwrap()]
    };
    let linear = find("linear");
    let eased = find("eased");
    ui.set_expanded(true);
    let expanded = ui.render();
    let mut timeline = SceneTransition::between(&start, &expanded);
    let time = Duration::from_millis;
    let delayed = renderer
        .snapshot(&timeline.frame(&expanded, time(100)), 546, 288, 1.0, &input)
        .unwrap();
    assert_eq!(delayed.layout.at_path(&eased).unwrap().bounds.width, 80.0);
    assert!((delayed.layout.at_path(&linear).unwrap().bounds.width - 120.0).abs() < 0.01);

    ui.set_changes(1);
    let updated = ui.render();
    timeline.retarget(&expanded, &updated, time(250));
    let middle = renderer
        .snapshot(&timeline.frame(&updated, time(420)), 546, 288, 1.0, &input)
        .unwrap();
    let bounds = middle.layout.at_path(&eased).unwrap().bounds;
    assert!((bounds.width - 254.0).abs() < 0.01);
    assert!((bounds.x - 53.0).abs() < 0.01);
    assert_ne!(middle.pixel(73, 160), delayed.pixel(73, 160));

    ui.set_expanded(false);
    let reversed = ui.render();
    timeline.retarget(&updated, &reversed, time(420));
    let immediate = timeline.frame(&reversed, time(420));
    assert!(
        (immediate.find("eased").unwrap().length("width").unwrap() - bounds.width).abs() < 0.01
    );
    ui.set_motion(false);
    let disabled = ui.render();
    timeline.retarget(&reversed, &disabled, time(450));
    assert!(!timeline.running(time(450)));
    assert_eq!(
        timeline
            .frame(&disabled, time(450))
            .find("eased")
            .unwrap()
            .length("width"),
        Some(80.0)
    );
}

#[test]
fn accepting_drop_area_paints_above_its_child_and_clears_on_cancel() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let scene = DragDropDemo::new().render();
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 560, 258, 1.0, &input).unwrap();
    input.arm_drag(&scene, &before.layout, Point { x: 100.0, y: 117.0 });
    assert!(
        input
            .drag_motion(&scene, &before.layout, Point { x: 420.0, y: 117.0 }, None)
            .captured
    );
    let hover = renderer.snapshot(&scene, 560, 258, 1.0, &input).unwrap();
    assert_ne!(hover.pixel(328, 110), before.pixel(328, 110));
    assert_eq!(hover.pixel(520, 140), before.pixel(520, 140));
    assert!(input.cancel_drag().captured);
    let after = renderer.snapshot(&scene, 560, 258, 1.0, &input).unwrap();
    assert_eq!(before.rgba, after.rgba);
}

#[test]
fn file_drag_highlight_tracks_the_accepting_area() {
    use std::sync::Arc;
    use xrack_layout::FileDragInput;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let scene = FileDropDemo::new().render();
    let mut input = Interaction::default();
    let initial = renderer.snapshot(&scene, 536, 310, 1.0, &input).unwrap();
    let paths: Arc<[String]> = [std::env::temp_dir()
        .join("photo.png")
        .to_str()
        .unwrap()
        .into()]
    .into();
    let movement = |x| FileDragInput::Moved {
        sequence: 1,
        point: Point { x, y: 110.0 },
        paths: paths.clone(),
    };
    assert!(
        !input
            .file_drag_event(&scene, &initial.layout, movement(120.0))
            .events
            .is_empty()
    );
    let accepted = renderer.snapshot(&scene, 536, 310, 1.0, &input).unwrap();
    assert_ne!(accepted.pixel(26, 110), initial.pixel(26, 110));
    assert_eq!(accepted.pixel(280, 110), initial.pixel(280, 110));
    assert!(
        !input
            .file_drag_event(&scene, &initial.layout, movement(330.0))
            .events
            .is_empty()
    );
    let rejected = renderer.snapshot(&scene, 536, 310, 1.0, &input).unwrap();
    assert_eq!(rejected.rgba, initial.rgba);
}

#[test]
fn bundled_font_collection_shapes_and_paints_without_system_fonts() {
    let text = xrack_renderer_femtovg::TextSystem::bundled_only();
    let mut renderer = pollster::block_on(Renderer::with_text(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
        text,
    ))
    .unwrap();
    let scene = BundledFontsDemo::new().render();
    assert_eq!(scene.fonts.unwrap().sources().len(), 1);
    let input = Interaction::default();
    let frame = renderer.snapshot(&scene, 420, 190, 1.0, &input).unwrap();
    let width = |id| renderer.text.advance_node(scene.find(id).unwrap(), "WWWW");
    assert!((width("regular") - 48.0).abs() < 0.1);
    assert!((width("bold") - 60.0).abs() < 0.1);
    let regular = frame.layout.at_path(&[0, 0]).unwrap().bounds;
    let bold = frame.layout.at_path(&[0, 1]).unwrap().bounds;
    assert!((regular.width - 120.0).abs() < 0.1);
    assert!((bold.width - 150.0).abs() < 0.1);
    let background = frame.pixel(0, 0);
    for bounds in [regular, bold] {
        assert!(
            (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
                (bounds.x as u32..(bounds.x + bounds.width) as u32)
                    .any(|x| frame.pixel(x, y) != background)
            })
        );
    }

    let ui = interpret_example("examples/layout/bundled.xr")
        .create("BundledFontsDemo")
        .unwrap();
    let loaded = ui.try_render().unwrap();
    assert_eq!(loaded.loaded_fonts.as_ref().unwrap().sources().len(), 1);
    let interpreted = renderer.snapshot(&loaded, 420, 190, 1.0, &input).unwrap();
    assert_same_pixels(&interpreted, &frame);
    assert_eq!(interpreted.layout, frame.layout);

    let blank = xrack_runtime::Node::new("Window", None);
    renderer.snapshot(&blank, 420, 190, 1.0, &input).unwrap();
    assert!(renderer.text.shape("WWWW", 20.0).is_err());
    let restored = renderer.snapshot(&scene, 420, 190, 1.0, &input).unwrap();
    assert_eq!(frame.rgba, restored.rgba);
}

#[test]
fn font_picker_prepares_system_and_embedded_families_and_paints_the_draft() {
    use xrack_runtime::{Length, Node, SpinAction, SpinDecimal};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = FontPickerDemo::new();
    ui.set_picker_open(true);
    let original = ui.get_selected();
    let embedded = BundledFontsDemo::new().render().fonts;
    let input = Interaction::default();
    let viewport = Size::new(510.0, 430.0);
    let family = [3, 0, 0, 0, 1];
    let size = [3, 0, 0, 1, 1];
    let preview = [3, 0, 0, 4];
    let prepare = |ui: &FontPickerDemo, renderer: &mut Renderer, fonts| {
        let mut scene = ui.render();
        scene.fonts = fonts;
        renderer.prepare_scene(ui, scene.into(), viewport, &input).0
    };
    let system = prepare(&ui, &mut renderer, None);
    let system_names = system.at_path(&family).unwrap().strings("model");
    assert!(system_names.len() > 1);
    assert!(!system_names.iter().any(|family| family == "XRack Demo"));
    let scene = prepare(&ui, &mut renderer, embedded);
    let names = scene.at_path(&family).unwrap().strings("model");
    assert!(system_names.iter().all(|family| names.contains(family)));
    let index = names
        .iter()
        .position(|family| family == "XRack Demo")
        .unwrap();
    let before = renderer.snapshot(&scene, 510, 430, 1.0, &input).unwrap();

    assert!(ui.control_at(&scene, &family, ControlAction::Select(index)));
    let value = SpinDecimal::new(44.0).unwrap();
    assert!(ui.control_at(
        &scene,
        &size,
        ControlAction::Spin(SpinAction::CommitDecimal(value))
    ));
    let draft = prepare(&ui, &mut renderer, embedded);
    let after = renderer.snapshot(&draft, 510, 430, 1.0, &input).unwrap();
    let sample = draft.at_path(&preview).unwrap();
    assert_eq!(sample.font("font").unwrap().family, "XRack Demo");
    assert!((renderer.text.advance_node(sample, "WWWW") - 105.6).abs() < 0.2);
    let area = after.layout.at_path(&preview).unwrap().bounds;
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| before.pixel(x, y) != after.pixel(x, y))
    }));
    assert_eq!(ui.get_selected(), original);

    let chosen = sample.font("font").unwrap().clone();
    let mut window = Node::new("Window", None);
    window.fonts = embedded;
    window
        .properties
        .insert("default-font", chosen.clone().into());
    renderer.text.set_window(&window);
    let mut label = Node::new("Text", None);
    assert!((renderer.text.advance_node(&label, "WWWW") - 105.6).abs() < 0.2);
    label.properties.insert("font", chosen.into());
    label.properties.insert("font-size", Length(24.0).into());
    label.properties.insert("font-weight", 700_i64.into());
    assert!((renderer.text.advance_node(&label, "WWWW") - 72.0).abs() < 0.2);

    let reset = prepare(&ui, &mut renderer, None);
    assert!(
        !renderer
            .text
            .font_families()
            .iter()
            .any(|name| name == "XRack Demo")
    );
    assert_eq!(
        reset
            .at_path(&family)
            .unwrap()
            .strings("model")
            .first()
            .map(String::as_str),
        Some("XRack Demo")
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("font-picker.png"))
            .unwrap();
    }
}

#[test]
fn window_fonts_update_layout_gpu_paint_and_editor_geometry_together() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = FontsDemo::new();
    let input = Interaction::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 500, 390, 1.0, &input).unwrap();
    let label = [0, 0];
    let overridden = [0, 1];
    let entry = [0, 2];
    let area = [0, 3];
    let bounds = |frame: &xrack_renderer_femtovg::Snapshot, path: &[usize]| {
        frame.layout.at_path(path).unwrap().bounds
    };
    let edit = TextEdit::at_end("WWWWiiii");
    let caret =
        |text: &xrack_renderer_femtovg::TextSystem, scene: &xrack_runtime::Node, entry_bounds| {
            text.single_line_caret_x(
                scene.at_path(&entry).unwrap(),
                entry_bounds,
                "WWWWiiii",
                Some(&edit),
            )
        };
    let paragraph =
        |text: &xrack_renderer_femtovg::TextSystem, scene: &xrack_runtime::Node, bounds| {
            let node = scene.at_path(&area).unwrap();
            text.text_area_layout(node, bounds, node.text("text").unwrap())
        };
    let caret_before = caret(&renderer.text, &scene, bounds(&before, &entry));
    let area_before = paragraph(&renderer.text, &scene, bounds(&before, &area));

    ui.set_mono(true);
    let mono = ui.render_shared();
    let after = renderer.snapshot(&mono, 500, 390, 1.0, &input).unwrap();
    let measure = |value| {
        renderer
            .text
            .advance_node(mono.at_path(&entry).unwrap(), value)
    };
    assert!((measure("WWWW") - measure("iiii")).abs() < 0.5);
    assert_ne!(bounds(&before, &label).width, bounds(&after, &label).width);
    assert_eq!(bounds(&before, &overridden), bounds(&after, &overridden));
    let area_after = paragraph(&renderer.text, &mono, bounds(&after, &area));
    assert_eq!(area_before.line_height, area_after.line_height);
    assert_ne!(area_before.lines, area_after.lines);
    let entry_bounds = bounds(&after, &entry);
    let caret_after = caret(&renderer.text, &mono, entry_bounds);
    assert!((caret_after - (entry_bounds.x + 6.0 + measure("WWWWiiii"))).abs() < 0.01);
    assert!((caret_after - caret_before).abs() > 0.5);
    let hit = renderer.text.single_line_hit_with_text(
        mono.at_path(&entry).unwrap(),
        entry_bounds,
        Point {
            x: entry_bounds.x + 6.0 + measure("WWWW"),
            y: entry_bounds.y,
        },
        None,
        "WWWWiiii",
    );
    assert_eq!(hit.offset, 4);
    let rect = entry_bounds.inset(8.0);
    assert!((rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
        (rect.x as u32..(rect.x + rect.width) as u32)
            .any(|x| before.pixel(x, y) != after.pixel(x, y))
    }));

    ui.set_large(true);
    let large = renderer
        .snapshot(&ui.render(), 500, 390, 1.0, &input)
        .unwrap();
    assert!(bounds(&large, &label).height > bounds(&after, &label).height);
    assert!(bounds(&large, &entry).height > bounds(&after, &entry).height);
    assert_eq!(
        bounds(&large, &overridden).height,
        bounds(&after, &overridden).height
    );
}

#[test]
fn grid_view_paints_selection_and_reuses_frames_for_offscreen_value_changes() {
    use std::rc::Rc;
    use xrack_runtime::Property;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut demo = GridDemo::new();
    demo.set_records(
        (0..50_000)
            .map(|rank| DirectoryItem {
                label: format!("Tile {rank}"),
                rank,
            })
            .collect(),
    );
    let viewport = Size::new(620.0, 420.0);
    let input = Interaction::default();
    let prepared = Property::default();
    let prepare = |demo: &GridDemo, renderer: &mut Renderer| {
        prepared
            .get(|| Rc::new(renderer.prepare_scene(demo, demo.render_shared(), viewport, &input)))
    };
    let first = prepare(&demo, &mut renderer);
    let model = demo.model_records();
    assert!(model.set_row(
        49_999,
        DirectoryItem {
            label: "Offscreen edit".into(),
            rank: 49_999
        }
    ));
    assert!(Rc::ptr_eq(&first, &prepare(&demo, &mut renderer)));
    let before = renderer.snapshot(&first.0, 620, 420, 1.0, &input).unwrap();
    let cells = &first.1.at_path(&[0, 1]).unwrap().children;
    let cell = cells[1].bounds;
    let point = ((cell.x + 110.0) as u32, (cell.y + 24.0) as u32);
    let gap = ((cell.x - 4.0) as u32, point.1);
    assert_eq!(
        before.pixel(point.0, point.1),
        Some([0x30, 0x45, 0x65, 255])
    );
    assert_eq!(before.pixel(gap.0, gap.1), Some([0x27, 0x35, 0x4d, 255]));
    demo.set_current(1);
    let selected = prepare(&demo, &mut renderer);
    let after = renderer
        .snapshot(&selected.0, 620, 420, 1.0, &input)
        .unwrap();
    assert_ne!(
        before.pixel(point.0, point.1),
        after.pixel(point.0, point.1)
    );
    assert_eq!(before.pixel(gap.0, gap.1), after.pixel(gap.0, gap.1));
}

#[test]
fn page_paints_bands_and_clips_the_body_across_resize_and_visibility() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut demo = PageDemo::new();
    let input = Interaction::default();
    let mut scene = demo.render();
    // Oversize the compiled body and start it above its viewport. Page still
    // clips it to the content region before painting or dispatching input.
    let body = &mut scene.children[0].children[1];
    body.properties
        .insert("y", xrack_runtime::Length(-80.0).into());
    body.properties
        .insert("height", xrack_runtime::Length(400.0).into());
    let frame = renderer.snapshot(&scene, 360, 240, 1.0, &input).unwrap();
    assert_eq!(frame.pixel(300, 20), Some([0x25, 0x32, 0x49, 255]));
    assert_eq!(frame.pixel(300, 110), Some([0x38, 0x60, 0x55, 255]));
    assert_eq!(frame.pixel(4, 110), Some([0x14, 0x1d, 0x2a, 255]));
    assert_eq!(frame.pixel(300, 230), Some([0x25, 0x3f, 0x51, 255]));

    demo.set_show_header(false);
    let frame = renderer
        .snapshot(&demo.render(), 420, 300, 1.0, &input)
        .unwrap();
    assert_eq!(frame.pixel(390, 20), Some([0x38, 0x60, 0x55, 255]));
    assert_eq!(frame.pixel(390, 290), Some([0x25, 0x3f, 0x51, 255]));
}

#[test]
fn item_delegates_paint_selection_and_trailing_check_indicators() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        ItemDelegatesDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(644.0, 398.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 644, 398, 1.0, ui.input())
        .unwrap();
    let path = [0, 1, 0, 1, 0, 0];
    let bounds = before.layout.at_path(&path).unwrap().bounds;
    ui.click_at_path(&path);
    let after = renderer
        .snapshot(ui.scene(), 644, 398, 1.0, ui.input())
        .unwrap();
    let y = (bounds.y + bounds.height * 0.5) as u32;
    let indicator = (bounds.x + bounds.width - 21.0) as u32;
    let background = (bounds.x + bounds.width * 0.5) as u32;
    assert_ne!(before.pixel(indicator, y), after.pixel(indicator, y));
    assert_ne!(before.pixel(background, y), after.pixel(background, y));
    assert_ne!(after.pixel(indicator, y), after.pixel(background, y));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("item-delegates.png"))
            .unwrap();
    }
}

#[test]
fn standard_list_reads_only_visible_labels_and_paints_model_row_updates() {
    use std::rc::Rc;
    use xrack_runtime::Property;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardListDemo::new();
    ui.set_records(
        (0..50_000)
            .map(|index| DirectoryItem {
                label: format!("Row {index}"),
                rank: index,
            })
            .collect(),
    );
    let input = Interaction::default();
    let viewport = Size::new(584.0, 355.0);
    let prepared = Property::default();
    let prepare = |renderer: &mut Renderer| {
        prepared.get(|| Rc::new(renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input)))
    };
    let first = prepare(&mut renderer);
    let before = renderer.snapshot(&first.0, 584, 355, 1.0, &input).unwrap();
    let records = ui.model_records();
    assert!(records.set_row(
        49_999,
        DirectoryItem {
            label: "Offscreen edit".into(),
            rank: 49_999
        }
    ));
    assert!(Rc::ptr_eq(&first, &prepare(&mut renderer)));
    assert!(records.set_row(
        0,
        DirectoryItem {
            label: "Visible edit".into(),
            rank: 0
        }
    ));
    let update = prepare(&mut renderer);
    assert!(!Rc::ptr_eq(&first, &update));
    let after = renderer.snapshot(&update.0, 584, 355, 1.0, &input).unwrap();
    let list = update.0.find("records-view").unwrap();
    assert!(list.children.len() <= 8);
    assert_eq!(
        list.children[0].children[0].text("text"),
        Some("Visible edit")
    );
    assert_ne!(before.rgba, after.rgba);
}

#[test]
fn custom_progress_artwork_mirrors_and_uses_the_indicator_clock() {
    use std::time::Duration;
    use xrack_runtime::Value;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut generated = controls_ui::CustomProgressDemo::new();
    let interpreted = interpret_example("examples/controls/controls.xr")
        .create("CustomProgressDemo")
        .unwrap();
    let input = Interaction::default();
    let sample = |renderer: &mut Renderer, scene: &xrack_runtime::Node, time| {
        renderer
            .snapshot_at(scene, 440, 252, 1.0, &input, time)
            .unwrap()
    };
    let zero = Duration::ZERO;
    let midpoint = Duration::from_millis(700);
    let track = [0x27, 0x3b, 0x55, 0xff];
    let fill = [0x53, 0xaf, 0x9b, 0xff];
    for rtl in [true, false] {
        generated.set_rtl(rtl);
        interpreted.set("rtl", Value::Bool(rtl)).unwrap();
        let left = sample(&mut renderer, &generated.render(), zero);
        assert_same_pixels(&left, &sample(&mut renderer, &interpreted.render(), zero));
        let filled_x = if rtl { 394 } else { 46 };
        let unfilled_x = if rtl { 46 } else { 394 };
        assert_eq!(left.pixel(filled_x, 70), Some(fill));
        assert_eq!(left.pixel(unfilled_x, 70), Some(track));
        assert_eq!(left.pixel(55, 110), Some([0xdc, 0x99, 0x57, 0xff]));
    }

    generated.set_running(true);
    interpreted.set("running", true.into()).unwrap();
    let start = sample(&mut renderer, &generated.render(), zero);
    assert!(renderer.needs_redraw());
    let end = sample(&mut renderer, &generated.render(), midpoint);
    assert!(renderer.needs_redraw());
    assert_eq!(start.pixel(30, 70), Some([0xe3, 0xff, 0xf7, 0xff]));
    assert_eq!(end.pixel(30, 70), Some(track));
    assert_eq!(end.pixel(322, 70), Some([0xe3, 0xff, 0xf7, 0xff]));
    assert_same_pixels(
        &end,
        &sample(&mut renderer, &interpreted.render(), midpoint),
    );

    let mut viewport = xrack_runtime::Node::new("Rectangle", None);
    viewport
        .properties
        .insert("width", xrack_runtime::Length(90.0).into());
    viewport
        .properties
        .insert("height", xrack_runtime::Length(90.0).into());
    viewport.properties.insert("clip", true.into());
    viewport
        .children
        .push(generated.render().find("styled-progress").unwrap().clone());
    let mut cropped = xrack_runtime::Node::new("Window", None);
    cropped.children.push(viewport);
    assert_eq!(
        sample(&mut renderer, &cropped, zero).pixel(30, 70),
        Some([0xe3, 0xff, 0xf7, 0xff])
    );
    assert_eq!(
        sample(&mut renderer, &cropped, midpoint).pixel(30, 70),
        Some(track)
    );
    assert!(renderer.needs_redraw());

    generated.set_animated(false);
    interpreted.set("animated", false.into()).unwrap();
    let stopped = sample(&mut renderer, &generated.render(), zero);
    assert!(!renderer.needs_redraw());
    assert_same_pixels(
        &stopped,
        &sample(&mut renderer, &generated.render(), midpoint),
    );
    assert_same_pixels(
        &stopped,
        &sample(&mut renderer, &interpreted.render(), midpoint),
    );
    assert_eq!(stopped.pixel(176, 70), Some([0xe3, 0xff, 0xf7, 0xff]));
}

#[test]
fn indicators_use_explicit_snapshot_time_and_stop_requesting_frames_when_static() {
    use std::time::Duration;
    use xrack_renderer_femtovg::Snapshot;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = IndicatorsDemo::new();
    let input = Interaction::default();
    let scene = ui.render_shared();
    let sample = |renderer: &mut Renderer, scene: &xrack_runtime::Node, time| {
        renderer
            .snapshot_at(scene, 452, 286, 1.0, &input, time)
            .unwrap()
    };
    let zero = Duration::ZERO;
    let later = Duration::from_millis(350);
    let first = sample(&mut renderer, &scene, zero);
    assert!(renderer.needs_redraw());
    let next = sample(&mut renderer, &scene, later);
    let changed = |id: &str, first: &Snapshot, next: &Snapshot| {
        let index = scene
            .children
            .iter()
            .position(|node| node.id == Some(id))
            .unwrap();
        let area = first.layout.children[index].bounds;
        (area.y as u32..(area.y + area.height) as u32).any(|y| {
            (area.x as u32..(area.x + area.width) as u32)
                .any(|x| first.pixel(x, y) != next.pixel(x, y))
        })
    };
    assert!(changed("busy", &first, &next));
    assert!(changed("hybrid-ring", &first, &next));
    assert!(changed("activity-bar", &first, &next));
    assert!(!changed("progress-ring", &first, &next));

    ui.set_animated(false);
    let paused = ui.render_shared();
    let still = sample(&mut renderer, &paused, zero);
    assert!(!renderer.needs_redraw());
    assert_eq!(still.rgba, sample(&mut renderer, &paused, later).rgba);
    let busy = scene
        .children
        .iter()
        .position(|node| node.id == Some("busy"))
        .unwrap();
    assert!(still.layout.children[busy].visible);
    ui.set_running(false);
    ui.set_animated(true);
    let stopped = sample(&mut renderer, &ui.render(), later);
    assert!(!renderer.needs_redraw());
    assert!(!stopped.layout.children[busy].visible);
    assert_eq!(
        stopped.layout.children[busy].bounds,
        first.layout.children[busy].bounds
    );
    assert!(changed("busy", &still, &stopped));

    let mut offscreen = xrack_runtime::Node::new("Window", None);
    let mut indicator = scene.children[busy].clone();
    indicator
        .properties
        .insert("x", xrack_runtime::Length(900.0).into());
    offscreen.children.push(indicator);
    sample(&mut renderer, &offscreen, later);
    assert!(!renderer.needs_redraw());
}

#[test]
fn increased_contrast_strengthens_default_editor_edges_focus_and_scroll_thumb() {
    use list_ui::ScrollBarsDemo;
    use xrack_runtime::ColorScheme;

    fn contrast(a: [u8; 4], b: [u8; 4]) -> f64 {
        let luminance = |[r, g, b, _]: [u8; 4]| {
            let linear = |value: u8| {
                let value = f64::from(value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = ThemeDemo::new();
    let scene = ui.render_shared();
    let input = Interaction::default();
    for scheme in [ColorScheme::Light, ColorScheme::Dark] {
        renderer.set_system_color_scheme(scheme);
        let normal = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
        let mut editing = Interaction::default();
        let field = editing
            .accessibility_target(&scene, &normal.layout, &[5])
            .unwrap();
        editing.focus_target(&scene, &normal.layout, field);
        assert!(editing.is_focused(&[5]));
        let normal_focus = renderer.snapshot(&scene, 446, 318, 1.0, &editing).unwrap();
        assert!(renderer.set_system_increased_contrast(true));
        let high = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
        let high_focus = renderer.snapshot(&scene, 446, 318, 1.0, &editing).unwrap();
        let background = normal.pixel(10, 130).unwrap();
        let field_fill = normal.pixel(410, 130).unwrap();
        // Sample a straight section of the left border, away from antialiased corners.
        let edge_contrast = |edge| contrast(background, edge).min(contrast(field_fill, edge));
        let normal_edge = (18..=23)
            .map(|x| edge_contrast(normal.pixel(x, 130).unwrap()))
            .fold(1.0, f64::max);
        let high_edge = (18..=23)
            .map(|x| edge_contrast(high.pixel(x, 130).unwrap()))
            .fold(1.0, f64::max);
        assert!(
            high_edge >= 3.0 && high_edge > normal_edge + 0.5,
            "{scheme:?}: {normal_edge} -> {high_edge}"
        );
        assert_eq!(normal.pixel(10, 130), high.pixel(10, 130));
        assert_eq!(normal.pixel(410, 130), high.pixel(410, 130));
        let focus_contrast = |pixel| contrast(field_fill, pixel);
        let normal_ring = (21..=24)
            .map(|x| focus_contrast(normal_focus.pixel(x, 130).unwrap()))
            .fold(1.0, f64::max);
        let high_ring = (21..=24)
            .map(|x| focus_contrast(high_focus.pixel(x, 130).unwrap()))
            .fold(1.0, f64::max);
        assert!(
            high_ring > normal_ring,
            "{scheme:?} focus: {normal_ring} -> {high_ring}"
        );
        let heading_ink = |frame: &xrack_renderer_femtovg::Snapshot| {
            (16..43)
                .flat_map(|y| (20..176).map(move |x| (x, y)))
                .map(|(x, y)| contrast(background, frame.pixel(x, y).unwrap()))
                .fold(1.0, f64::max)
        };
        assert!(heading_ink(&high) >= 4.5 && heading_ink(&high) > heading_ink(&normal));
        assert_eq!(normal.pixel(30, 72), high.pixel(30, 72));
        assert_eq!(high.pixel(30, 282), Some([188, 80, 134, 255]));
        assert!(std::rc::Rc::ptr_eq(&scene, &ui.render_shared()));
        assert!(renderer.set_system_increased_contrast(false));
        let restored = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
        assert_eq!(restored.pixel(20, 130), normal.pixel(20, 130));
    }

    let scroll = ScrollBarsDemo::new().render_shared();
    let normal = renderer.snapshot(&scroll, 352, 272, 1.0, &input).unwrap();
    assert!(renderer.set_system_increased_contrast(true));
    let high = renderer.snapshot(&scroll, 352, 272, 1.0, &input).unwrap();
    let thumb = |frame: &xrack_renderer_femtovg::Snapshot| {
        contrast(
            frame.pixel(295, 35).unwrap(),
            frame.pixel(295, 190).unwrap(),
        )
    };
    assert!(thumb(&high) >= 3.0 && thumb(&high) > thumb(&normal));

    let mut ui = ThemeDemo::new();
    ui.set_accent(Color(0xffffffff));
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let closed = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    input.pointer_move(&scene, &closed.layout, Point { x: 80.0, y: 181.0 });
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert!(input.open_choice_at_pointer(&scene, &closed.layout, target));
    let choice = input.choice_menu(&scene, &closed.layout).unwrap();
    let selected = choice.row_bounds(choice.highlighted.unwrap() - choice.first);
    let menu = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    let fill = menu
        .pixel(
            (selected.x + selected.width - 14.0) as u32,
            (selected.y + selected.height * 0.5) as u32,
        )
        .unwrap();
    assert!(
        contrast([255; 4], fill) >= 4.5,
        "readability on white accent: {fill:?}"
    );
}

#[test]
fn window_palette_updates_controls_and_overlays_without_rebuilding_the_scene() {
    use xrack_runtime::ColorScheme;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ThemeDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    assert!(renderer.set_system_color_scheme(ColorScheme::Dark));
    let dark = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    assert!(renderer.set_system_color_scheme(ColorScheme::Light));
    let light = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    assert!(std::rc::Rc::ptr_eq(&scene, &ui.render_shared()));
    assert_eq!(dark.pixel(10, 10), Some([24, 32, 43, 255]));
    assert_eq!(light.pixel(10, 10), Some([243, 246, 251, 255]));
    assert_ne!(dark.pixel(410, 132), light.pixel(410, 132));
    assert_eq!(dark.pixel(30, 282), Some([188, 80, 134, 255]));
    assert_eq!(dark.pixel(30, 282), light.pixel(30, 282));
    assert_eq!(
        dark.layout.at_path(&[5]).unwrap().bounds,
        light.layout.at_path(&[5]).unwrap().bounds
    );

    input.pointer_move(&scene, &light.layout, Point { x: 80.0, y: 181.0 });
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert!(input.open_choice_at_pointer(&scene, &light.layout, target));
    let menu = input.choice_menu(&scene, &light.layout).unwrap();
    let (x, y) = (
        (menu.bounds.x + 1.0) as u32,
        (menu.bounds.y + menu.bounds.height * 0.5) as u32,
    );
    let light_menu = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    assert_eq!(light_menu.pixel(x, y), Some([255, 255, 255, 255]));
    renderer.set_system_color_scheme(ColorScheme::Dark);
    let dark_menu = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    assert_eq!(dark_menu.pixel(x, y), Some([39, 53, 77, 255]));

    input.dismiss_choice();
    ui.set_scheme(ColorScheme::Light);
    ui.set_accent(Color(0xf5a63bff));
    let scene = ui.render_shared();
    let forced = renderer.snapshot(&scene, 446, 318, 1.0, &input).unwrap();
    assert_eq!(forced.pixel(10, 10), light.pixel(10, 10));
    assert_eq!(light.pixel(30, 72), Some([62, 101, 232, 255]));
    assert_eq!(forced.pixel(30, 72), Some([245, 166, 59, 255]));
    assert_eq!(ui.get_draft(), "保留输入状态");
    assert!(ui.get_selected());
}

#[test]
fn color_picker_paints_channel_gradients_and_alpha_over_checkerboard() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ColorDemo::new();
    ui.set_picker_open(true);
    let input = Interaction::default();
    let scene = ui.render_shared();
    let translucent = renderer.snapshot(&scene, 478, 384, 1.0, &input).unwrap();
    let swatch = |snapshot: &xrack_renderer_femtovg::Snapshot, column: f32| {
        let bounds = snapshot.layout.at_path(&[5, 0, 0, 0, 0]).unwrap().bounds;
        let x = bounds.x + bounds.width * (column + 0.5) / 8.0;
        let y = bounds.y + bounds.height * 0.25;
        snapshot.pixel(x as u32, y as u32).unwrap()
    };
    assert_ne!(swatch(&translucent, 0.0), swatch(&translucent, 1.0));
    let red = translucent.layout.at_path(&[5, 0, 0, 1, 1]).unwrap().bounds;
    let slider = SliderGeometry::new(red, Orientation::Horizontal);
    let red_at = |fraction| {
        let point = slider.position(fraction);
        translucent.pixel(point.x as u32, point.y as u32).unwrap()[0]
    };
    let (low, high) = (red_at(0.1), red_at(0.9));
    assert!(
        high.saturating_sub(low) > 100,
        "red gradient ranges from {low} to {high}"
    );

    assert!(ui.adjust_range_at(&scene, &[5, 0, 0, 4, 1], RangeAction::Maximum));
    let opaque = renderer
        .snapshot(&ui.render_shared(), 478, 384, 1.0, &input)
        .unwrap();
    assert_eq!(swatch(&opaque, 0.0), [64, 128, 192, 255]);
    assert_eq!(swatch(&opaque, 0.0), swatch(&opaque, 1.0));
    assert_eq!(ui.get_selected(), Color(0x4080c080));
}

#[test]
fn hsv_color_picker_paints_its_plane_and_synchronizes_hex() {
    use xrack_layout::ColorPlaneGeometry;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ColorDemo::new();
    ui.set_plane_visible(true);
    ui.set_picker_open(true);
    ui.set_selected(Color(0xff000080));
    let input = Interaction::default();
    let plane = [5, 0, 0, 5, 0];
    let hue = [5, 0, 0, 5, 1];
    let hex = [5, 0, 0, 0, 1];
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 478, 540, 1.0, &input).unwrap();
    let geometry = ColorPlaneGeometry::new(before.layout.at_path(&plane).unwrap().bounds);
    let pixel = |snapshot: &xrack_renderer_femtovg::Snapshot, s, v| {
        let point = geometry.point(s, v);
        snapshot.pixel(point.x as u32, point.y as u32).unwrap()
    };
    let pale = pixel(&before, 0.08, 0.92);
    let red = pixel(&before, 0.92, 0.92);
    let dark = pixel(&before, 0.92, 0.08);
    assert!(pale[1] > 190 && pale[2] > 190, "{pale:?}");
    assert!(red[0] > 210 && red[1] < 45 && red[2] < 45, "{red:?}");
    assert!(dark[0] < 50 && dark[1] < 30 && dark[2] < 30, "{dark:?}");

    assert!(ui.adjust_range_at(&scene, &hue, RangeAction::SetFraction(1.0 / 3.0)));
    let draft = ui.render_shared();
    assert_eq!(draft.at_path(&hex).unwrap().text("text"), Some("#00FF0080"));
    let after = renderer.snapshot(&draft, 478, 540, 1.0, &input).unwrap();
    let green = pixel(&after, 0.92, 0.92);
    assert!(
        green[1] > 210 && green[0] < 45 && green[2] < 45,
        "{green:?}"
    );
    assert_eq!(ui.get_selected(), Color(0xff000080));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("color-picker.png"))
            .unwrap();
    }
}

#[test]
fn application_window_keeps_its_bands_outside_the_scrolled_content() {
    use navigation_ui::ApplicationWindowDemo;
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = ApplicationWindowDemo::new();
    let scene = ui.render();
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 500, 340, 1.0, &input).unwrap();
    for (x, y, expected) in [
        (470, 16, [0x27, 0x35, 0x4d, 255]),
        (470, 50, [0x30, 0x47, 0x65, 255]),
        (470, 318, [0x29, 0x39, 0x4c, 255]),
        (9, 160, [0x18, 0x23, 0x31, 255]),
    ] {
        assert_eq!(before.pixel(x, y), Some(expected));
    }
    input.pointer_move(&scene, &before.layout, Point { x: 415.0, y: 180.0 });
    assert!(input.scroll_wheel(&before.layout, Point { x: 0.0, y: 95.0 }, false));
    let after = renderer.snapshot(&scene, 500, 340, 1.0, &input).unwrap();
    for (x, y) in [(470, 16), (470, 50), (470, 318), (9, 160)] {
        assert_eq!(after.pixel(x, y), before.pixel(x, y));
    }
    assert_ne!(before.pixel(49, 130), after.pixel(49, 130));
    assert_eq!(after.pixel(49, 130), Some([0x1d, 0x2b, 0x3a, 255]));
}

#[test]
fn round_toolbar_paints_the_button_cutout_and_both_separator_orientations() {
    use menu_ui::RoundToolbarDemo;
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let scene = RoundToolbarDemo::new().render();
    let input = Interaction::default();
    let frame = renderer.snapshot(&scene, 480, 280, 1.0, &input).unwrap();
    assert_eq!(frame.pixel(25, 57), Some([0x38, 0x49, 0x60, 255]));
    assert_eq!(frame.pixel(51, 69), Some([0x49, 0x69, 0xdf, 255]));
    let line = |toolbar| {
        let parent = scene
            .children
            .iter()
            .position(|node| node.id == Some(toolbar))
            .unwrap();
        frame.layout.at_path(&[parent, 1]).unwrap().bounds
    };
    let vertical = line("horizontal");
    let horizontal = line("vertical");
    assert_eq!(vertical.width, 12.0);
    assert_eq!(horizontal.height, 12.0);
    let x = (vertical.x + vertical.width * 0.5) as u32;
    let y = (vertical.y + vertical.height * 0.5) as u32;
    assert_eq!(frame.pixel(x, y), Some([0xe4, 0xb6, 0x78, 255]));
    assert_ne!(frame.pixel(x, vertical.y as u32 + 1), frame.pixel(x, y));
    let x = (horizontal.x + horizontal.width * 0.5) as u32;
    let y = (horizontal.y + horizontal.height * 0.5) as u32;
    assert_eq!(frame.pixel(x, y), Some([0x9c, 0xdb, 0xbd, 255]));
    assert_ne!(frame.pixel(horizontal.x as u32 + 1, y), frame.pixel(x, y));
}

#[test]
fn pane_and_frame_paint_their_surfaces_and_clip_children_to_rounded_bounds() {
    use controls_ui::PaneFrameDemo;
    use xrack_runtime::{Component, Length, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = PaneFrameDemo::new();
    let input = Interaction::default();
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 460, 270, 1.0, &input).unwrap();
    assert_eq!(before.pixel(408, 178), Some([0x27, 0x35, 0x4d, 255]));
    assert_eq!(before.pixel(110, 74), Some([0x75, 0x85, 0x9a, 255]));
    assert_eq!(before.pixel(52, 102), Some([0x30, 0x3d, 0x52, 255]));

    let mut scene = scene;
    let mut swatch = Node::new("Rectangle", None);
    for (name, value) in [("x", -8.0), ("y", -8.0), ("width", 22.0), ("height", 18.0)] {
        swatch.properties.insert(name, Length(value).into());
    }
    swatch
        .properties
        .insert("background", xrack_runtime::Color(0xd76555ff).into());
    scene.children[2].children[0].children.push(swatch);
    let clipped = renderer.snapshot(&scene, 460, 270, 1.0, &input).unwrap();
    assert_eq!(clipped.pixel(49, 75), before.pixel(49, 75));
    assert_eq!(clipped.pixel(60, 83), Some([0xd7, 0x65, 0x55, 255]));
}

#[test]
fn disabled_group_preserves_the_content_geometry_and_draws_disabled_text() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = GroupBoxDemo::new();
    let input = Interaction::default();
    let scene = ui.render_shared();
    let group = scene
        .children
        .iter()
        .position(|node| node.id == Some("delivery"))
        .unwrap();
    let path = [group, 0];
    let before = renderer.snapshot(&scene, 470, 376, 1.0, &input).unwrap();
    let area = before.layout.at_path(&path).unwrap().bounds;
    ui.set_active(false);
    let disabled = renderer
        .snapshot(&ui.render(), 470, 376, 1.0, &input)
        .unwrap();
    assert_eq!(disabled.layout.at_path(&path).unwrap().bounds, area);
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| before.pixel(x, y) != disabled.pixel(x, y))
    }));
    assert_eq!(ui.get_address(), "notify@example.invalid");
}

#[test]
fn checkbox_paints_distinct_unchecked_partial_and_checked_marks() {
    use xrack_runtime::CheckState;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CheckStateDemo::new();
    let input = Interaction::default();
    let unchecked = renderer
        .snapshot(&ui.render(), 452, 350, 1.0, &input)
        .unwrap();
    ui.set_cycle_state(CheckState::PartiallyChecked);
    let partial = renderer
        .snapshot(&ui.render(), 452, 350, 1.0, &input)
        .unwrap();
    ui.set_cycle_state(CheckState::Checked);
    let checked = renderer
        .snapshot(&ui.render(), 452, 350, 1.0, &input)
        .unwrap();
    let different_mark = |a: &xrack_renderer_femtovg::Snapshot,
                          b: &xrack_renderer_femtovg::Snapshot| {
        (174..192).any(|y| (30..48).any(|x| a.pixel(x, y) != b.pixel(x, y)))
    };
    assert!(different_mark(&unchecked, &partial));
    assert!(different_mark(&partial, &checked));
}

#[test]
fn delay_button_fills_only_the_elapsed_part_of_its_width() {
    use std::time::{Duration, Instant};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = DelayDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let layout = renderer.layout(&scene, Size::new(400.0, 214.0));
    input.pointer_move(&scene, &layout, Point { x: 60.0, y: 80.0 });
    input.pointer_press();
    let now = Instant::now();
    input.begin_pointer_delay(&scene, &layout, now);
    let before = renderer.snapshot(&scene, 400, 214, 1.0, &input).unwrap();
    assert!(
        input
            .advance_delay(&scene, &layout, now + Duration::from_millis(400))
            .is_none()
    );
    let half = renderer.snapshot(&scene, 400, 214, 1.0, &input).unwrap();
    assert_ne!(before.pixel(42, 69), half.pixel(42, 69));
    assert_eq!(before.pixel(235, 69), half.pixel(235, 69));
    assert_eq!(ui.get_activations(), 0);
}

#[test]
fn tumbler_paints_a_drag_preview_without_committing_the_index() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = TumblerDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 388, 324, 1.0, &input).unwrap();
    let wheel = before
        .layout
        .at_path(&[2])
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap();
    let point = Point {
        x: wheel.center.x + 18.0,
        y: wheel.center.y + 12.0,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        &before.layout,
        point,
        now,
        ScrollGestureKind::Touch
    ));
    let moved = Point {
        y: point.y - wheel.center.height * 0.6,
        ..point
    };
    assert!(input.move_scroll_gesture(
        &scene,
        &before.layout,
        moved,
        now + Duration::from_millis(25)
    ));
    let preview = renderer.snapshot(&scene, 388, 324, 1.0, &input).unwrap();
    assert_ne!(before.rgba, preview.rgba);
    assert_eq!(ui.get_minute(), 30);
    assert_eq!(ui.get_changes(), 0);
    input.cancel_scroll_gesture();
    let restored = renderer.snapshot(&scene, 388, 324, 1.0, &input).unwrap();
    assert_eq!(restored.rgba, before.rgba);
}

#[test]
fn path_view_draws_the_selected_record_on_the_curve_and_the_line() {
    use navigation_ui::PathViewDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PathViewDemo::new();
    let input = Interaction::default();
    let curved = renderer
        .snapshot(&ui.render_shared(), 640, 336, 1.0, &input)
        .unwrap();
    let wheel = curved
        .layout
        .at_path(&[0])
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap();
    let hero = wheel.rows.iter().find(|row| row.index == 2).unwrap();
    let x = (hero.bounds.x + 9.0) as u32;
    let y = (hero.bounds.y + 12.0) as u32;
    assert_eq!(curved.pixel(x, y), Some([0x7d, 0x62, 0xba, 255]));

    ui.set_curved(false);
    let line = renderer
        .snapshot(&ui.render_shared(), 640, 336, 1.0, &input)
        .unwrap();
    let wheel = line.layout.at_path(&[0]).unwrap().tumbler.as_ref().unwrap();
    let hero = wheel.rows.iter().find(|row| row.index == 2).unwrap();
    assert_eq!(line.pixel(x, y), Some([0x11, 0x1c, 0x2a, 255]));
    assert_eq!(
        line.pixel(x, (hero.bounds.y + 12.0) as u32),
        Some([0x7d, 0x62, 0xba, 255])
    );
    assert_eq!(ui.get_selected(), 30);
    assert_eq!(ui.get_changes(), 0);
}

#[test]
fn custom_tumbler_artwork_paints_visible_rows_and_follows_drag_displacement() {
    use controls_ui::RecordTumblerDemo;
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RecordTumblerDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let frame = renderer.snapshot(&scene, 410, 294, 1.0, &input).unwrap();
    assert_eq!(frame.pixel(78, 94), Some([0x32, 0x68, 0x98, 255]));
    assert_eq!(frame.pixel(78, 20), Some([0x16, 0x20, 0x34, 255]));
    let wheel = frame
        .layout
        .at_path(&[0])
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap();
    let point = Point {
        x: wheel.center.x + 18.0,
        y: wheel.center.y + 12.0,
    };
    let moved = Point {
        y: point.y + wheel.center.height * 0.65,
        ..point
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        &frame.layout,
        point,
        now,
        ScrollGestureKind::Touch
    ));
    assert!(input.move_scroll_gesture(
        &scene,
        &frame.layout,
        moved,
        now + Duration::from_millis(25)
    ));
    let preview = renderer.snapshot(&scene, 410, 294, 1.0, &input).unwrap();
    assert_ne!(frame.rgba, preview.rgba);
    let wheel = preview
        .layout
        .at_path(&[0])
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap();
    let selected = wheel.rows.iter().find(|row| row.index == 2).unwrap();
    let (artwork, _) = selected.content().unwrap();
    assert!(selected.displacement > 0.5 && artwork.opacity() < 0.9);
    assert_eq!(ui.get_offset(), 480);
    let last = Point {
        y: point.y + wheel.center.height * 0.75,
        ..point
    };
    assert!(input.move_scroll_gesture(
        &scene,
        &preview.layout,
        last,
        now + Duration::from_millis(34)
    ));
    let release = input.end_scroll_gesture(
        &scene,
        &preview.layout,
        last,
        now + Duration::from_millis(40),
    );
    assert!(release.control.is_none());
    let coast =
        input.advance_tumbler_motion(&scene, &preview.layout, now + Duration::from_millis(90));
    assert!(coast.scrolled && coast.control.is_none());
    let moving = renderer.snapshot(&scene, 410, 294, 1.0, &input).unwrap();
    assert_ne!(preview.rgba, moving.rgba);
    assert_eq!(moving.pixel(78, 20), Some([0x16, 0x20, 0x34, 255]));
    assert_eq!(ui.get_offset(), 480);
    let settled =
        input.advance_tumbler_motion(&scene, &moving.layout, now + Duration::from_millis(700));
    let action = settled.control.unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_offset(), -300);
    assert_eq!(ui.get_reported_offset(), -300);
    assert_eq!(ui.get_commits(), 1);
}

#[test]
fn editor_insets_share_geometry_and_keep_placeholder_ink_separate() {
    use form_ui::EditorInsetsDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::Color;

    fn crop_diff(
        a: &xrack_renderer_femtovg::Snapshot,
        b: &xrack_renderer_femtovg::Snapshot,
        mut y: std::ops::Range<u32>,
    ) -> bool {
        y.any(|y| (24..408).any(|x| a.pixel(x, y) != b.pixel(x, y)))
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut aot = EditorInsetsDemo::new();
    let parsed = interpret_example("examples/form/form.xr");
    let interpreter = parsed.create("EditorInsetsDemo").unwrap();
    let input = Interaction::default();
    let mut draw = |component: &dyn Component| {
        renderer
            .snapshot(&component.render_shared(), 432, 350, 1.0, &input)
            .unwrap()
    };
    let empty = draw(&aot);
    assert_same_pixels(&empty, &draw(&interpreter));
    let ink = Color(0x09e6e2ff);
    aot.set_hint(ink);
    interpreter.set("hint", ink.into()).unwrap();
    let painted = draw(&aot);
    assert_same_pixels(&painted, &draw(&interpreter));
    assert!(crop_diff(&empty, &painted, 42..106));
    assert!(crop_diff(&empty, &painted, 124..238));

    aot.set_account("WW".into());
    interpreter.set("account", "WW".to_owned().into()).unwrap();
    let typed = draw(&aot);
    assert_same_pixels(&typed, &draw(&interpreter));
    aot.set_hint(Color(0xffb554ff));
    assert!(!crop_diff(&typed, &draw(&aot), 42..106));

    for ui in [Box::new(aot) as Box<dyn Component>, Box::new(interpreter)] {
        let mut preview = PreviewScene::new(ui);
        preview.render_animated(&mut renderer).unwrap();
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let note = scene.find("note-field").unwrap();
        let sized =
            renderer
                .text
                .text_area_layout(note, layout.at_path(&[2]).unwrap().bounds, "WW");
        assert_eq!(
            sized.area,
            xrack_layout::Rect::new(48.0, 146.0, 282.0, 76.0)
        );
        preview.click(Point { x: 60.0, y: 80.0 });
        preview.render_animated(&mut renderer).unwrap();
        let ime = preview.ime_area().unwrap();
        assert_eq!(ime.x, 72.0);
        assert!(ime.y >= 66.0 && ime.y + ime.height <= 100.0, "{ime:?}");
        preview.edit(EditAction::Insert("!"));
        preview.render_animated(&mut renderer).unwrap();
        assert_eq!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("account-field")
                .unwrap()
                .text("text"),
            Some("!WW")
        );
    }
}

#[test]
fn password_rendering_and_hit_testing_use_the_same_grapheme_mask() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PasswordDemo::new();
    let secret = "e\u{301}👩‍💻界";
    ui.set_secret(secret.into());
    let scene = ui.render_shared();
    let input = Interaction::default();
    let hidden = renderer.snapshot(&scene, 392, 254, 1.0, &input).unwrap();
    let path = [1];
    let node = scene.at_path(&path).unwrap();
    let area = hidden.layout.at_path(&path).unwrap().bounds;
    let x = area.x + 6.0 + renderer.text.advance("•", font_size(node));
    assert_eq!(
        renderer
            .text
            .single_line_hit(
                node,
                area,
                Point {
                    x,
                    y: area.y + 12.0
                },
                None
            )
            .offset,
        "e\u{301}".len()
    );
    ui.set_secret("abc".into());
    let same_mask = renderer
        .snapshot(&ui.render(), 392, 254, 1.0, &input)
        .unwrap();
    assert_eq!(hidden.rgba, same_mask.rgba);

    let composing = |renderer: &mut Renderer, value: &str, preedit: &str| {
        let mut input = Interaction::default();
        input.focus_next(&scene, &hidden.layout, false);
        let mut edit = TextEdit::at_end(value);
        edit.set_preedit(preedit.into(), Some((preedit.len(), preedit.len())));
        input.editor = Some(edit);
        renderer.snapshot(&scene, 392, 254, 1.0, &input).unwrap()
    };
    assert_eq!(
        composing(&mut renderer, secret, "か").rgba,
        composing(&mut renderer, secret, "🙂").rgba
    );

    ui.set_secret(secret.into());
    ui.set_mask_character("🔒".into());
    let custom = ui.render_shared();
    let custom_frame = renderer.snapshot(&custom, 392, 254, 1.0, &input).unwrap();
    let node = custom.at_path(&path).unwrap();
    assert_eq!(node.text("password-character"), Some("🔒"));
    let x = area.x + 6.0 + renderer.text.advance_node(node, "🔒");
    assert_eq!(
        renderer
            .text
            .single_line_hit(
                node,
                area,
                Point {
                    x,
                    y: area.y + 12.0
                },
                None
            )
            .offset,
        "e\u{301}".len()
    );
    let program = interpret_example("examples/form/form.xr");
    let interpreted = program.create("PasswordDemo").unwrap();
    interpreted.set("secret", secret.to_owned().into()).unwrap();
    interpreted
        .set("mask-character", "🔒".to_owned().into())
        .unwrap();
    let interpreted_frame = renderer
        .snapshot(&interpreted.render_shared(), 392, 254, 1.0, &input)
        .unwrap();
    assert_same_pixels(&custom_frame, &interpreted_frame);
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| custom_frame.pixel(x, y) != hidden.pixel(x, y))
    }));

    let mut keyboard = Interaction::default();
    keyboard.focus_next(&custom, &custom_frame.layout, false);
    keyboard.editor = Some(TextEdit::at_end(secret));
    assert!(renderer.text.move_editor_caret(
        &mut keyboard,
        &custom,
        &custom_frame.layout,
        TextAreaMotion::Left,
        true
    ));
    assert_eq!(
        keyboard.editor.as_ref().unwrap().selection(secret),
        secret.len() - "界".len()..secret.len()
    );
    assert!(renderer.text.move_editor_caret(
        &mut keyboard,
        &custom,
        &custom_frame.layout,
        TextAreaMotion::WordLeft,
        true
    ));
    assert_eq!(
        keyboard.editor.as_ref().unwrap().selection(secret),
        0..secret.len()
    );

    ui.set_reveal(true);
    let shown = renderer
        .snapshot(&ui.render(), 392, 254, 1.0, &input)
        .unwrap();
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| shown.pixel(x, y) != hidden.pixel(x, y))
    }));
}

#[test]
fn time_field_preserves_hidden_seconds_and_rechecks_the_open_popup_range() {
    use form_ui::TimeFieldDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{KeyEvent, Time};

    fn exercise(
        renderer: &mut Renderer,
        component: Box<dyn Component>,
        mut maximum: impl FnMut(Time),
    ) {
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        let label = |preview: &PreviewScene, index| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .child_at(index)
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        };
        let key = |preview: &mut PreviewScene, renderer: &mut Renderer, name: &str| {
            let event = KeyEvent {
                key: name.into(),
                ..Default::default()
            };
            preview.keyboard(renderer, &event, true);
            preview.keyboard(renderer, &event, false);
            preview.render_animated(renderer).unwrap();
        };
        let paste = |preview: &mut PreviewScene, renderer: &mut Renderer, value: &str| {
            preview.click(Point { x: 85.0, y: 78.0 });
            preview.render_animated(renderer).unwrap();
            preview.edit(EditAction::SelectAll);
            preview.paste(value);
            preview.render_animated(renderer).unwrap();
        };
        let accept_enabled = |preview: &PreviewScene| {
            let popup = preview
                .inspect_frame()
                .unwrap()
                .0
                .find("time-picker")
                .unwrap();
            assert!(popup.is_visible());
            popup
                .child_at(1)
                .unwrap()
                .child_at(1)
                .unwrap()
                .boolean("enabled", true)
        };
        assert_eq!(label(&preview, 3), "已提交 · 13:30:42");
        paste(&mut preview, renderer, "2400");
        assert_eq!(label(&preview, 2), "请检查时间或营业时段");
        key(&mut preview, renderer, "Enter");
        assert_eq!(label(&preview, 3), "已提交 · 13:30:42");
        key(&mut preview, renderer, "Escape");
        assert_eq!(label(&preview, 2), "可提交");
        paste(&mut preview, renderer, "1459");
        key(&mut preview, renderer, "Enter");
        assert_eq!(label(&preview, 3), "已提交 · 14:59:42");
        preview.click(Point { x: 75.0, y: 198.0 });
        preview.render_animated(renderer).unwrap();
        assert_eq!(
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find("second-entry")
                .unwrap()
                .text("text"),
            Some("14:59:42")
        );
        for incomplete_or_late in ["1459", "173001"] {
            paste(&mut preview, renderer, incomplete_or_late);
            assert_eq!(label(&preview, 2), "请检查时间或营业时段");
        }
        paste(&mut preview, renderer, "153005");
        key(&mut preview, renderer, "Enter");
        assert_eq!(label(&preview, 3), "已提交 · 15:30:05");
        paste(&mut preview, renderer, "240000");
        let clock = Point { x: 380.0, y: 76.0 };
        preview.click(clock);
        preview.render_animated(renderer).unwrap();
        assert!(accept_enabled(&preview));
        key(&mut preview, renderer, "Escape");
        assert_eq!(label(&preview, 2), "请检查时间或营业时段");
        assert_eq!(label(&preview, 3), "已提交 · 15:30:05");
        preview.click(clock);
        preview.render_animated(renderer).unwrap();
        maximum(Time::new(15, 0, 0).unwrap());
        preview.render_animated(renderer).unwrap();
        assert!(!accept_enabled(&preview));
        key(&mut preview, renderer, "ArrowUp");
        assert!(accept_enabled(&preview));
        for _ in 0..5 {
            key(&mut preview, renderer, "Tab");
        }
        key(&mut preview, renderer, "Enter");
        assert_eq!(label(&preview, 3), "已提交 · 14:30:05");
        assert_eq!(label(&preview, 2), "可提交");
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = TimeFieldDemo::new();
    let mut handle = aot.as_weak().upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), |value| {
        handle.set_latest(value)
    });
    assert_eq!(handle.get_commits(), 3);
    let interpreter = interpret_example("examples/form/form.xr")
        .create("TimeFieldDemo")
        .unwrap();
    let handle = interpreter.clone();
    exercise(&mut renderer, Box::new(interpreter), |value| {
        handle.set("latest", value.into()).unwrap()
    });
    assert_eq!(handle.get("commits").unwrap(), xrack_runtime::Value::Int(3));
}

#[test]
fn date_field_keeps_drafts_separate_and_shares_keyboard_and_picker_rules() {
    use form_ui::DateFieldDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Date, KeyEvent, VecModel};

    fn exercise(renderer: &mut Renderer, component: Box<dyn Component>, closed: VecModel<Date>) {
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        let visible = |preview: &PreviewScene, id: &str| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .find(id)
                .is_some_and(|node| node.is_visible())
        };
        let display = |preview: &PreviewScene| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .child_at(3)
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        };
        let validation = |preview: &PreviewScene| {
            preview
                .inspect_frame()
                .unwrap()
                .0
                .child_at(2)
                .unwrap()
                .text("text")
                .unwrap()
                .to_owned()
        };
        let key = |preview: &mut PreviewScene, renderer: &mut Renderer, name: &str| {
            let event = KeyEvent {
                key: name.into(),
                ..Default::default()
            };
            preview.keyboard(renderer, &event, true);
            preview.keyboard(renderer, &event, false);
            preview.render_animated(renderer).unwrap();
        };
        let paste = |preview: &mut PreviewScene, renderer: &mut Renderer, value: &str| {
            preview.click(Point { x: 85.0, y: 78.0 });
            preview.render_animated(renderer).unwrap();
            preview.edit(EditAction::SelectAll);
            preview.paste(value);
            preview.render_animated(renderer).unwrap();
        };
        assert_eq!(display(&preview), "已提交 · 2024-02-28");
        for invalid in ["20240230", "20240229"] {
            paste(&mut preview, renderer, invalid);
            assert_eq!(validation(&preview), "请检查日期或停约规则");
            key(&mut preview, renderer, "Enter");
            assert_eq!(display(&preview), "已提交 · 2024-02-28");
        }
        closed.clear();
        preview.render_animated(renderer).unwrap();
        assert_eq!(validation(&preview), "可提交");
        closed.push(Date::new(2024, 2, 29).unwrap());
        preview.render_animated(renderer).unwrap();
        assert_eq!(validation(&preview), "请检查日期或停约规则");
        key(&mut preview, renderer, "Escape");
        assert_eq!(validation(&preview), "可提交");
        paste(&mut preview, renderer, "20240301");
        assert_eq!(display(&preview), "已提交 · 2024-02-28");
        key(&mut preview, renderer, "Enter");
        assert_eq!(display(&preview), "已提交 · 2024-03-01");
        paste(&mut preview, renderer, "20240229");
        let calendar = Point { x: 380.0, y: 76.0 };
        preview.click(calendar);
        preview.render_animated(renderer).unwrap();
        assert!(visible(&preview, "date-picker"));
        key(&mut preview, renderer, "Escape");
        assert!(!visible(&preview, "date-picker"));
        assert_eq!(display(&preview), "已提交 · 2024-03-01");
        assert_eq!(validation(&preview), "请检查日期或停约规则");
        preview.click(calendar);
        preview.render_animated(renderer).unwrap();
        key(&mut preview, renderer, "ArrowRight");
        key(&mut preview, renderer, "Tab");
        key(&mut preview, renderer, "Tab");
        key(&mut preview, renderer, "Enter");
        assert_eq!(display(&preview), "已提交 · 2024-03-04");
        assert_eq!(validation(&preview), "可提交");
        paste(&mut preview, renderer, "20240230");
        preview.click(Point { x: 80.0, y: 198.0 });
        preview.render_animated(renderer).unwrap();
        assert_eq!(display(&preview), "已提交 · 2024-03-04");
        assert_eq!(validation(&preview), "可提交");
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let aot = DateFieldDemo::new();
    let closed = aot.model_closed_dates();
    let weak = aot.as_weak();
    let owner = weak.upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), closed);
    assert_eq!(owner.get_commits(), 2);
    let interpreter = interpret_example("examples/form/form.xr")
        .create("DateFieldDemo")
        .unwrap();
    let closed = interpreter.date_model("closed-dates").unwrap();
    let owner = interpreter.clone();
    exercise(&mut renderer, Box::new(interpreter), closed);
    assert_eq!(owner.get("commits").unwrap(), xrack_runtime::Value::Int(2));
}

#[test]
fn password_echo_uses_the_preview_deadline_in_both_backends() {
    use std::time::{Duration, Instant};
    use xrack_renderer_femtovg::preview::PreviewScene;

    fn exercise(renderer: &mut Renderer, component: Box<dyn Component>) {
        let mut preview = PreviewScene::new(component);
        preview.render_animated(renderer).unwrap();
        preview.click(Point { x: 40.0, y: 80.0 });
        preview.render_animated(renderer).unwrap();
        preview.edit(EditAction::Insert("👩‍💻"));
        let visible = preview.render_animated(renderer).unwrap().to_vec();
        let delay = preview
            .next_frame_delay()
            .expect("password echo schedules a repaint");
        assert!(delay <= Duration::from_millis(750));
        preview.advance_at(renderer, Instant::now() + Duration::from_secs(1));
        let hidden = preview.draw_prepared(renderer).unwrap().to_vec();
        assert_ne!(visible, hidden);
        assert!(preview.next_frame_delay().is_none());

        // A paste of the same glyph stays masked even for a one-grapheme paste.
        preview.edit(EditAction::SelectAll);
        preview.paste("👨‍💻");
        assert_eq!(preview.render_animated(renderer).unwrap(), &hidden);
        assert!(preview.next_frame_delay().is_none());
        preview.edit(EditAction::SelectAll);
        preview.edit(EditAction::Insert("W"));
        assert!(preview.render_animated(renderer).unwrap() != hidden);
        preview.set_window_active(false);
        preview.render_animated(renderer).unwrap();
        preview.set_window_active(true);
        assert_eq!(preview.render_animated(renderer).unwrap(), &hidden);
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut aot = PasswordDemo::new();
    aot.set_briefly_reveal(true);
    exercise(&mut renderer, Box::new(aot));
    let interpreted = interpret_example("examples/form/form.xr")
        .create("PasswordDemo")
        .unwrap();
    interpreted.set("briefly-reveal", true.into()).unwrap();
    exercise(&mut renderer, Box::new(interpreted));
}

#[test]
fn search_field_paints_the_matching_candidate_popup() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = AsyncDemo::new();
    ui.set_suggestions(vec!["candidate for XRack".into()]);
    ui.set_result_query("older query".into());
    let mut input = Interaction::default();
    let viewport = Size::new(540.0, 350.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.focus_next(&scene, &layout, false);
    input.choice_key(&scene, &layout, ChoiceKey::Open);
    assert!(input.choice_menu(&scene, &layout).is_none());
    let before = renderer.snapshot(&scene, 540, 350, 1.0, &input).unwrap();
    ui.set_result_query("XRack".into());
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&scene, &layout);
    let popup = input.choice_menu(&scene, &layout).unwrap();
    let x = popup.bounds.x as u32 + 3;
    let y = popup.bounds.y as u32 + 8;
    let after = renderer.snapshot(&scene, 540, 350, 1.0, &input).unwrap();
    assert_ne!(before.pixel(x, y), after.pixel(x, y));
}

#[test]
fn decimal_spin_box_paints_its_configured_decimal_separator() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = DecimalSpinDemo::new();
    let input = Interaction::default();
    let before = renderer
        .snapshot(&ui.render(), 480, 306, 1.0, &input)
        .unwrap();
    ui.set_use_comma(false);
    let after = renderer
        .snapshot(&ui.render(), 480, 306, 1.0, &input)
        .unwrap();
    let area = SpinGeometry::new(before.layout.at_path(&[1]).unwrap().bounds).text;
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| before.pixel(x, y) != after.pixel(x, y))
    }));
    assert_eq!(ui.get_level(), 0.2);
}

#[test]
fn custom_calendar_content_paints_and_updates_the_corresponding_date() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::Date;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CalendarAgendaDemo::new();
    let input = Interaction::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 576, 478, 1.0, &input).unwrap();
    let path = [6];
    let geometry = CalendarGeometry::new(
        scene.at_path(&path).unwrap(),
        before.layout.at_path(&path).unwrap().bounds,
    );
    let chip = |day| {
        let rect = geometry
            .days()
            .find(|cell| cell.date == Date::new(2026, 11, day).unwrap())
            .unwrap()
            .bounds;
        ((rect.x + 12.0) as u32, (rect.y + 34.0) as u32)
    };
    let review = chip(12);
    let release = chip(19);
    assert_ne!(
        before.pixel(review.0, review.1),
        before.pixel(release.0, release.1)
    );
    let slot = geometry
        .days()
        .find(|day| day.date == ui.get_review_date())
        .unwrap()
        .slot;
    let mut oversized = scene.clone();
    let month = &mut std::rc::Rc::make_mut(&mut oversized).children[6];
    let marker = &mut month.children[0].children[slot].children[1];
    marker
        .properties
        .insert("x", xrack_runtime::Length(-140.0).into());
    marker
        .properties
        .insert("width", xrack_runtime::Length(210.0).into());
    let clipped = renderer
        .snapshot(&oversized, 576, 478, 1.0, &input)
        .unwrap();
    let neighbor = chip(11);
    assert_eq!(
        before.pixel(neighbor.0, neighbor.1),
        clipped.pixel(neighbor.0, neighbor.1)
    );
    ui.set_review_date(Date::new(2027, 1, 1).unwrap());
    let after = renderer
        .snapshot(&ui.render_shared(), 576, 478, 1.0, &input)
        .unwrap();
    assert_ne!(
        before.pixel(review.0, review.1),
        after.pixel(review.0, review.1)
    );
    assert_eq!(
        before.pixel(release.0, release.1),
        after.pixel(release.0, release.1)
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("calendar-agenda.png"))
            .unwrap();
    }
}

#[test]
fn month_grid_renders_a_selected_adjacent_day_with_composed_headers() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::{CalendarAction, Date};
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = MonthGridDemo::new();
    let input = Interaction::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 444, 372, 1.0, &input).unwrap();
    let path = [6];
    let geometry = CalendarGeometry::new(
        scene.at_path(&path).unwrap(),
        before.layout.at_path(&path).unwrap().bounds,
    );
    let adjacent = Date::new(2020, 12, 31).unwrap();
    let action = CalendarAction::Activate(adjacent);
    let rect = geometry.action_bounds(action).unwrap();
    let sample = ((rect.x + 10.0) as u32, (rect.y + 9.0) as u32);
    assert!(ui.control_at(&scene, &path, ControlAction::Calendar(action)));
    let after = renderer
        .snapshot(&ui.render_shared(), 444, 372, 1.0, &input)
        .unwrap();
    assert_ne!(
        before.pixel(sample.0, sample.1),
        after.pixel(sample.0, sample.1)
    );
    assert_eq!(before.pixel(20, 64), after.pixel(20, 64));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("month-grid.png"))
            .unwrap();
    }
}

#[test]
fn calendar_draws_the_selected_leap_day_using_its_hit_geometry() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::{CalendarAction, Date};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CalendarDemo::new();
    let input = Interaction::default();
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 440, 460, 1.0, &input).unwrap();
    let geometry = CalendarGeometry::new(
        scene.at_path(&[1]).unwrap(),
        before.layout.children[1].bounds,
    );
    let pixel = |date| {
        let bounds = geometry
            .action_bounds(CalendarAction::Select(date))
            .unwrap();
        (bounds.x as u32 + 8, bounds.y as u32 + 8)
    };
    let (previous_x, previous_y) = pixel(ui.get_selected());
    let leap_day = Date::new(2024, 2, 29).unwrap();
    let (x, y) = pixel(leap_day);
    let selected = before.pixel(previous_x, previous_y).unwrap();
    assert_ne!(selected, before.pixel(x, y).unwrap());
    ui.set_selected(leap_day);
    let after = renderer
        .snapshot(&ui.render(), 440, 460, 1.0, &input)
        .unwrap();
    assert_eq!(after.pixel(x, y), Some(selected));
    assert_ne!(after.pixel(previous_x, previous_y), Some(selected));
    let mut highlighted = ui.render();
    let calendar = &mut highlighted.children[1];
    let start = Date::new(2024, 2, 26).unwrap();
    calendar.properties.insert("highlight-start", start.into());
    calendar.properties.insert("highlight-end", leap_day.into());
    let highlighted = renderer
        .snapshot(&highlighted, 440, 460, 1.0, &input)
        .unwrap();
    let (x, y) = pixel(Date::new(2024, 2, 27).unwrap());
    assert_ne!(highlighted.pixel(x, y), after.pixel(x, y));
}

#[test]
fn date_picker_popup_draws_the_leap_day_draft_before_commit() {
    use xrack_layout::CalendarGeometry;
    use xrack_runtime::{CalendarAction, Date};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CalendarDemo::new();
    let input = Interaction::default();
    let original = ui.get_selected();
    ui.set_picker_open(true);
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 440, 460, 1.0, &input).unwrap();
    assert_eq!(before.layout.dialog_paths(), &[vec![3]]);
    let path = [3, 0];
    let geometry = CalendarGeometry::new(
        scene.at_path(&path).unwrap(),
        before.layout.at_path(&path).unwrap().bounds,
    );
    let day = Date::new(2024, 2, 29).unwrap();
    let action = CalendarAction::Select(day);
    let target = geometry.action_bounds(action).unwrap();
    let initial = geometry
        .action_bounds(CalendarAction::Select(original))
        .unwrap();
    let selected = before.pixel(initial.x as u32 + 8, initial.y as u32 + 8);
    let (x, y) = (target.x as u32 + 8, target.y as u32 + 8);
    assert_ne!(before.pixel(x, y), selected);
    assert!(ui.control_at(&scene, &path, ControlAction::Calendar(action)));
    let after = renderer
        .snapshot(&ui.render(), 440, 460, 1.0, &input)
        .unwrap();
    assert_eq!(after.pixel(x, y), selected);
    assert_eq!(ui.get_selected(), original);
}

#[test]
fn time_popup_paints_visible_wheels_and_the_draft_before_accepting() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TimeDemo::new();
    let input = Interaction::default();
    let capture = |ui: &TimeDemo, renderer: &mut Renderer| {
        renderer
            .snapshot(&ui.render(), 520, 372, 1.0, &input)
            .unwrap()
    };
    let inline = capture(&ui, &mut renderer);
    ui.set_use_24(false);
    ui.set_picker_open(true);
    let before = capture(&ui, &mut renderer);
    assert_ne!(before.pixel(5, 5), inline.pixel(5, 5));
    assert_eq!(before.layout.dialog_paths(), &[vec![5]]);
    assert!(!before.layout.at_path(&[5, 0, 2]).unwrap().visible);
    assert!(before.layout.at_path(&[5, 0, 3]).unwrap().visible);
    let minute = [5, 0, 1];
    let area = before
        .layout
        .at_path(&minute)
        .unwrap()
        .tumbler
        .as_ref()
        .unwrap()
        .center;
    let scene = ui.render_shared();
    let wheel = xrack_runtime::Tumbler::from_node(scene.at_path(&minute).unwrap());
    let committed = ui.get_selected();
    assert!(ui.control_at(&scene, &minute, wheel.select(24).unwrap()));
    let after = capture(&ui, &mut renderer);
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| before.pixel(x, y) != after.pixel(x, y))
    }));
    assert_eq!(ui.get_selected(), committed);
}

#[test]
fn source_cell_scroll_paints_the_queued_record_after_its_source_index_moves() {
    use table_ui::{RowVisibilityDemo, RowVisibilityFlags, RowVisibilityRecord};
    use xrack_runtime::ScrollRequests;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RowVisibilityDemo::new();
    ui.set_records(
        (0..64)
            .map(|index| RowVisibilityRecord {
                name: format!("Device {index}"),
                rank: index,
                flags: RowVisibilityFlags {
                    shown: index % 7 != 1,
                },
            })
            .collect(),
    );
    let model = ui.model_records();
    let key = model.row_key(37).unwrap();
    let viewport = Size::new(720.0, 320.0);
    let mut input = Interaction::default();
    let (initial, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&initial, 720, 320, 1.0, &input).unwrap();
    let requests = ScrollRequests::default();
    {
        let _active = requests.enter();
        ui.invoke_locate_source_cell(37, 1);
    }
    model.remove(0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert!(input.apply_scroll_requests(&scene, &layout, requests.drain()));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 720, 320, 1.0, &input).unwrap();
    assert_eq!(scene.at_path(&[2, 0]).unwrap().row_key, Some(key));
    assert_eq!(model.row_index(key), Some(36));
    let row = layout.at_path(&[2, 0]).unwrap().bounds;
    assert!((row.y as u32..(row.y + row.height) as u32).any(|y| {
        (row.x as u32..(row.x + row.width) as u32).any(|x| before.pixel(x, y) != after.pixel(x, y))
    }));

    ui.invoke_select_source(36, 1);
    assert!(
        ui.table_selection("standard")
            .unwrap()
            .contains_cell(key, 1)
    );
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let selected = renderer.snapshot(&scene, 720, 320, 1.0, &input).unwrap();
    let row = layout.at_path(&[2, 0]).unwrap().bounds;
    assert!((row.y as u32..(row.y + row.height) as u32).any(|y| {
        (row.x as u32..(row.x + row.width) as u32)
            .any(|x| after.pixel(x, y) != selected.pixel(x, y))
    }));
}

#[test]
fn filtered_list_and_grid_draw_the_next_visible_record_in_the_freed_slot() {
    use list_ui::ListVisibilityDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ListVisibilityDemo::new();
    let viewport = Size::new(660.0, 320.0);
    let input = Interaction::default();
    let model = ui.model_records();
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 660, 320, 1.0, &input).unwrap();
    assert!(ui.dispatch_at(&scene, &[1, 0, 0], "clicked"));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 660, 320, 1.0, &input).unwrap();
    for view in 1..=3 {
        assert_eq!(scene.at_path(&[view, 0]).unwrap().row_key, model.row_key(2));
        let bounds = layout.at_path(&[view, 0, 0]).unwrap().bounds;
        assert!(
            (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
                (bounds.x as u32..(bounds.x + bounds.width) as u32)
                    .any(|x| before.pixel(x, y) != after.pixel(x, y))
            })
        );
    }
}

#[test]
fn hidden_tree_subtrees_release_their_gpu_row_slots() {
    use tree_ui::VisibilityTreeDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = VisibilityTreeDemo::new();
    ui.invoke_initialize();
    let viewport = Size::new(540.0, 312.0);
    let input = Interaction::default();
    let model = ui.model_records();
    let (initial, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&initial, 540, 312, 1.0, &input).unwrap();
    assert_eq!(initial.at_path(&[1, 1]).unwrap().row_key, model.row_key(1));
    let mut branch = model.row(1).unwrap();
    branch.visibility.shown = false;
    model.set_row(1, branch);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 540, 312, 1.0, &input).unwrap();
    assert_eq!(scene.at_path(&[1]).unwrap().list.unwrap().row_count, 3);
    assert_eq!(scene.at_path(&[1, 1]).unwrap().row_key, model.row_key(3));
    let bounds = layout.at_path(&[1, 1, 0, 0]).unwrap().bounds;
    assert!(
        (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
            (bounds.x as u32..(bounds.x + bounds.width) as u32)
                .any(|x| before.pixel(x, y) != after.pixel(x, y))
        })
    );
}

#[test]
fn hidden_table_records_release_their_slots_in_both_gpu_views() {
    use table_ui::RowVisibilityDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = RowVisibilityDemo::new();
    let viewport = Size::new(720.0, 320.0);
    let input = Interaction::default();
    let model = ui.model_records();
    let (initial, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&initial, 720, 320, 1.0, &input).unwrap();
    let first_key = model.row_key(0);
    let next_key = model.row_key(1);
    let mut first = model.row(0).unwrap();
    first.flags.shown = false;
    model.set_row(0, first);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 720, 320, 1.0, &input).unwrap();
    for table in [1, 2] {
        let path = [table, 0];
        assert_eq!(initial.at_path(&path).unwrap().row_key, first_key);
        assert_eq!(scene.at_path(&path).unwrap().row_key, next_key);
        assert_eq!(scene.at_path(&[table]).unwrap().list.unwrap().row_count, 2);
        let bounds = layout.at_path(&[table, 0, 0, 0]).unwrap().bounds;
        assert!(
            (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
                (bounds.x as u32..(bounds.x + bounds.width) as u32)
                    .any(|x| before.pixel(x, y) != after.pixel(x, y))
            })
        );
    }
    assert_eq!(model.len(), 4);
}

#[test]
fn hiding_a_frozen_column_repaints_the_next_visible_column_at_the_left_edge() {
    use table_ui::WideTableDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = WideTableDemo::new();
    let viewport = Size::new(620.0, 320.0);
    let input = Interaction::default();
    let (initial, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&initial, 620, 320, 1.0, &input).unwrap();
    let first = layout.at_path(&[1, 0, 0, 0]).unwrap().bounds;
    ui.set_show_number(false);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 620, 320, 1.0, &input).unwrap();
    assert!(scene.at_path(&[1, 0, 0, 0]).is_none());
    assert!(
        layout
            .at_path(&[1])
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .is_frozen(1)
    );
    let next = layout.at_path(&[1, 0, 0, 1]).unwrap().bounds;
    assert_eq!(next.x, first.x);
    let changed = (first.y as u32..(first.y + first.height) as u32).any(|y| {
        (first.x as u32..(first.x + first.width) as u32)
            .any(|x| before.pixel(x, y) != after.pixel(x, y))
    });
    assert!(changed);
}

#[test]
fn wide_table_draws_newly_materialized_columns_beside_its_frozen_column() {
    use table_ui::WideTableDemo;
    use xrack_runtime::Orientation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = WideTableDemo::new();
    let viewport = Size::new(620.0, 320.0);
    let mut input = Interaction::default();
    let (initial, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let geometry = layout.at_path(&[1]).unwrap().table.as_ref().unwrap();
    assert!(initial.at_path(&[1, 0, 0, 11]).is_none());
    let before = renderer.snapshot(&initial, 620, 320, 1.0, &input).unwrap();
    assert!(input.accessibility_scroll_to(
        &layout,
        &[1],
        Orientation::Horizontal,
        geometry.horizontal.max_offset,
    ));
    let (scrolled, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let end = scrolled.at_path(&[1, 0, 0, 11]).unwrap();
    assert_eq!(end.text("text"), Some("1 / 12"));
    let after = renderer.snapshot(&scrolled, 620, 320, 1.0, &input).unwrap();
    renderer.set_display_batching_enabled(false);
    let direct = renderer.snapshot(&scrolled, 620, 320, 1.0, &input).unwrap();
    assert_eq!(after.rgba, direct.rgba);
    let fixed = layout.at_path(&[1, 0, 0, 0]).unwrap().bounds;
    let moving = layout.at_path(&[1, 0, 0, 11]).unwrap().bounds;
    let changes = |rect: xrack_layout::Rect| {
        (rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
            (rect.x as u32..(rect.x + rect.width) as u32)
                .any(|x| before.pixel(x, y) != after.pixel(x, y))
        })
    };
    assert!(!changes(fixed));
    assert!(changes(moving));
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn table_cells_batch_around_clipped_text_with_matching_pixels() {
    use std::rc::Rc;
    use table_ui::WideTableDemo;
    use xrack_runtime::Orientation;

    let mut renderer = pollster::block_on(Renderer::with_text(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
        xrack_renderer_femtovg::TextSystem::bundled_only(),
    ))
    .unwrap();
    let ui = WideTableDemo::new();
    let mut base = ui.render_shared();
    let window = Rc::make_mut(&mut base);
    window.fonts = BundledFontsDemo::new().render().fonts;
    window
        .properties
        .insert("default-font-family", "XRack Demo".to_owned().into());
    let viewport = Size::new(620.0, 320.0);
    let mut input = Interaction::default();
    let (_, layout) = renderer.prepare_scene(&ui, base.clone(), viewport, &input);
    // At 110px only the trailing part of the first scrolling column is
    // visible. Its short labels are wholly behind the frozen pane.
    for (offset, scroll_y) in [
        (0.0, 0.0),
        (19.0, 0.0),
        (110.0, 0.0),
        (112.0, 24.0),
        (113.0, 17.0),
    ] {
        if offset > 0.0 {
            assert!(input.accessibility_scroll_to(&layout, &[1], Orientation::Horizontal, offset));
        }
        if scroll_y > 0.0 {
            assert!(input.accessibility_scroll_to(&layout, &[1], Orientation::Vertical, scroll_y));
        }
        let (mut scene, _) = renderer.prepare_scene(&ui, base.clone(), viewport, &input);
        let table = Rc::make_mut(&mut scene).child_at_mut(1).unwrap();
        let mut rows = 0;
        for row in table
            .children
            .iter_mut()
            .filter(|row| row.kind == "ListRow")
        {
            if rows == 2 && offset > 0.0 {
                row.properties.insert("selected", true.into());
            }
            rows += 1;
            for (column, cell) in row.children[0].children.iter_mut().enumerate() {
                let text = if matches!(column, 2 | 4) {
                    "W".repeat(80)
                } else {
                    "WiWW".to_owned()
                };
                cell.properties.insert("text", text.into());
            }
        }
        for scale in [1.0, 2.0] {
            let (width, height) = ((620.0 * scale) as u32, (320.0 * scale) as u32);
            renderer.set_display_batching_enabled(true);
            let cold = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            let previous = renderer.display_batch_stats();
            let batched = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            let next = renderer.display_batch_stats();
            assert!(
                next.glyph_batches > previous.glyph_batches,
                "scale={scale}, offset={offset}"
            );
            assert!(
                next.clipped_text_runs - previous.clipped_text_runs >= rows,
                "expected clipped text to batch across rows at scale={scale}, offset={offset}"
            );
            assert!(
                next.text_runs - previous.text_runs
                    >= 2 * (next.glyph_batches - previous.glyph_batches)
            );
            renderer.set_display_batching_enabled(false);
            let direct = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            assert_same_pixels(&cold, &direct);
            assert_same_pixels(&batched, &direct);
            if offset == 0.0 && scale == 2.0 {
                check_table_glyph_mask_formats(&mut renderer, &scene, viewport, &input);
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn check_table_glyph_mask_formats(
    renderer: &mut Renderer,
    scene: &xrack_runtime::Node,
    viewport: Size,
    input: &Interaction,
) {
    let scale = 2.0;
    let layout = renderer.layout_with_input(scene, viewport, input);
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("table glyph mask format test"),
            size: wgpu::Extent3d {
                width: (viewport.width * scale) as u32,
                height: (viewport.height * scale) as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        renderer.set_display_batching_enabled(false);
        renderer
            .draw_with_layout(scene, &layout, &target, scale, input)
            .unwrap();
        let direct = gpu_readback::read_texture(renderer, &target).unwrap();

        renderer.set_display_batching_enabled(true);
        // Include both the first draw in this format and draws after prewarming.
        for frame in 0..4 {
            renderer
                .draw_with_layout(scene, &layout, &target, scale, input)
                .unwrap();
            let batched = gpu_readback::read_texture(renderer, &target).unwrap();
            let mismatch = direct.iter().zip(&batched).position(|(a, b)| a != b);
            assert!(
                mismatch.is_none(),
                "{format:?} frame {frame}: channel {mismatch:?}"
            );
        }
    }
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn mixed_table_text_matches_after_prewarming_between_the_frozen_panes() {
    use std::rc::Rc;
    use table_ui::WideTableDemo;
    use xrack_runtime::{Orientation, TextHorizontalAlignment};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    // Keep the reference atlas independent of the one being prewarmed.
    let mut reference = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    reference.set_display_batching_enabled(false);
    let ui = WideTableDemo::new();
    let viewport = Size::new(508.0, 281.0);
    let mut input = Interaction::default();
    let (_, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let sheet = layout.at_path(&[1]).unwrap();
    let padding_only =
        sheet.table.as_ref().unwrap().columns[5].x - (sheet.clip.x + sheet.clip.width) + 2.0;
    for (x, y, opacity) in [
        (0.0, 0.0, 1.0),
        (padding_only, 7.0, 1.0),
        (19.0, 17.0, 1.0),
        (110.0, 41.0, 1.0),
        (27.0, 53.0, 0.65),
    ] {
        if x != 0.0 {
            assert!(input.accessibility_scroll_to(&layout, &[1], Orientation::Horizontal, x));
            assert!(input.accessibility_scroll_to(&layout, &[1], Orientation::Vertical, y));
        }
        let (mut scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
        let table = Rc::make_mut(&mut scene).child_at_mut(1).unwrap();
        table.properties.insert("opacity", opacity.into());
        for (row, item) in table.children.iter_mut().enumerate() {
            if item.kind != "ListRow" {
                continue;
            }
            for (column, cell) in item.children[0].children.iter_mut().enumerate() {
                cell.properties
                    .insert("text", format!("{row}:{column} שלום · 构建").into());
                if column % 2 == 0 {
                    cell.properties.insert(
                        "horizontal-alignment",
                        TextHorizontalAlignment::Center.into(),
                    );
                }
            }
        }

        let direct = reference.snapshot(&scene, 1016, 562, 2.0, &input).unwrap();
        if x == padding_only {
            let item = direct.layout.at_path(&[1]).unwrap();
            let delegate = scene.at_path(&[1, 0, 0, 5]).unwrap();
            let cell = item.at_path(&[0, 0, 5]).unwrap();
            let text = xrack_layout::text_editor_viewport(delegate, cell.bounds);
            assert_eq!(cell.bounds.intersect(cell.clip).width, 2.0);
            assert_eq!(text.intersect(cell.clip).width, 0.0);
        }
        for frame in 0..4 {
            let before = renderer.display_batch_stats().text_runs;
            let mut masked = None;
            let sample = xrack_runtime::update_profile::measure(|| {
                masked = Some(renderer.snapshot(&scene, 1016, 562, 2.0, &input).unwrap());
            })
            .unwrap();
            if opacity == 1.0 {
                assert_eq!(
                    sample.gpu_submissions, 1,
                    "table masks share the scene submission"
                );
            }
            assert_same_pixels(&masked.unwrap(), &direct);
            if x == padding_only && frame == 3 {
                let after = renderer.display_batch_stats().text_runs;
                assert!(
                    after - before >= 24,
                    "clipped padding split the table text batch"
                );
            }
        }
        if x == padding_only {
            let batched = renderer.snapshot(&scene, 2032, 1124, 4.0, &input).unwrap();
            let direct = reference.snapshot(&scene, 2032, 1124, 4.0, &input).unwrap();
            assert_same_pixels(&batched, &direct);
        }
    }
}

#[test]
fn table_background_clear_matches_regular_paint() {
    use xrack_runtime::{Color, Length, Node, TableHeader};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    for (wrapped, width, background) in [
        (false, 180.0, None),
        (true, 180.0, Some(Color(0x406280ff))),
        (true, 156.0, Some(Color(0x406280ff))),
        (true, 180.0, Some(Color(0x40628080))),
    ] {
        let mut table = Node::new("TableView", None);
        table.properties.insert("width", Length(width).into());
        table.properties.insert("height", Length(136.0).into());
        if let Some(background) = background {
            table.properties.insert("background", background.into());
        }
        table.table_header = Some(Box::new(TableHeader {
            columns: (0..3)
                .map(|_| {
                    let mut column = Node::new("TableColumn", None);
                    column.properties.insert("width", Length(64.0).into());
                    column
                })
                .collect(),
            sort: None,
            column_widths: Vec::new(),
        }));
        let scene = if wrapped {
            let mut window = Node::new("Window", None);
            window
                .properties
                .insert("background", Color(0xe49d65ff).into());
            let mut overlay = Node::new("Rectangle", None);
            for (name, value) in [("x", 14.0), ("y", 46.0), ("width", 38.0), ("height", 24.0)] {
                overlay.properties.insert(name, Length(value).into());
            }
            overlay
                .properties
                .insert("background", Color(0xa3d8baff).into());
            window.children.extend([table, overlay]);
            window
        } else {
            table
        };
        for scale in [1.0, 2.0] {
            let (width, height) = ((180.0 * scale) as u32, (136.0 * scale) as u32);
            renderer.set_display_batching_enabled(true);
            let promoted = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            renderer.set_display_batching_enabled(false);
            let direct = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            assert_same_pixels(&promoted, &direct);
        }
    }
}

#[test]
fn table_header_dividers_share_a_batch_with_identical_pixels() {
    use xrack_layout::Point;
    use xrack_runtime::{
        ColorScheme, LayoutDirection, Length, Node, Orientation, TableHeader, TableSort,
    };

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut scene = Node::new("TableView", None);
    scene
        .properties
        .insert("color-scheme", ColorScheme::System.into());
    scene.properties.insert("frozen-columns", 2_i64.into());
    scene.properties.insert("resizable-columns", true.into());
    scene.table_header = Some(Box::new(TableHeader {
        columns: (0..18)
            .map(|index| {
                let mut column = Node::new("TableColumn", None);
                column.properties.insert("width", Length(46.0).into());
                column
                    .properties
                    .insert("title", format!("F{index}").into());
                column
            })
            .collect(),
        sort: Some(TableSort {
            column: 6,
            descending: false,
        }),
        column_widths: Vec::new(),
    }));
    let viewport = Size::new(620.0, 160.0);
    let mut input = Interaction::default();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Horizontal, 12.5));
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    input.pointer_move(&scene, &layout, Point { x: 44.0, y: 12.0 });
    assert_eq!(input.table_resize_column(&[]), Some(0));
    input.pointer_press();
    renderer.set_system_color_scheme(ColorScheme::Dark);
    let before = renderer.rectangle_batch_draws();
    let uploads = renderer.rectangle_palette_uploads();
    let glyph_batches = renderer.display_batch_stats().glyph_batches;
    let batched = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
    assert!(renderer.rectangle_batch_draws() > before);
    if !cfg!(any(target_os = "android", target_os = "ios")) {
        assert!(renderer.display_batch_stats().glyph_batches > glyph_batches);
    }
    assert_eq!(renderer.rectangle_palette_uploads(), uploads + 1);
    let repeated = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
    assert_eq!(repeated.rgba, batched.rgba);
    assert_eq!(renderer.rectangle_palette_uploads(), uploads + 1);
    renderer.set_display_batching_enabled(false);
    let direct = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
    assert_eq!(batched.rgba, direct.rgba);
    renderer.set_display_batching_enabled(true);
    renderer.set_system_color_scheme(ColorScheme::Light);
    let light = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
    assert_eq!(renderer.rectangle_palette_uploads(), uploads + 2);
    renderer.set_display_batching_enabled(false);
    let direct_light = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
    assert_eq!(light.rgba, direct_light.rgba);
    assert_ne!(light.rgba, batched.rgba);

    // Passive headers may batch their captions in addition to the dividers.
    scene.table_header.as_mut().unwrap().sort = None;
    for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
        scene
            .properties
            .insert("layout-direction", direction.into());
        let mut input = Interaction::default();
        let layout = renderer.layout_with_input(&scene, viewport, &input);
        assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Horizontal, 34.5));
        renderer.set_display_batching_enabled(true);
        let batched = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
        renderer.set_display_batching_enabled(false);
        let direct = renderer.snapshot(&scene, 1240, 320, 2.0, &input).unwrap();
        assert_same_pixels(&batched, &direct);
    }
}

#[test]
fn scrolling_a_nested_table_preserves_its_frozen_column_and_the_next_table() {
    use list_ui::NestedDataDemo;
    use xrack_layout::Rect;
    use xrack_runtime::Orientation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = NestedDataDemo::new();
    let viewport = Size::new(720.0, 364.0);
    let mut input = Interaction::default();
    let a = [1, 0, 0, 1];
    let b = [1, 1, 0, 1];
    let (initial, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&initial, 720, 364, 1.0, &input).unwrap();
    let original = layout.at_path(&a).unwrap().table.as_ref().unwrap();
    assert!(input.accessibility_scroll_to(&layout, &a, Orientation::Horizontal, 75.0));
    let (scrolled, after_layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scrolled, 720, 364, 1.0, &input).unwrap();
    let next = after_layout.at_path(&a).unwrap().table.as_ref().unwrap();
    assert_eq!(next.columns[0], original.columns[0]);
    assert!((original.columns[1].x - next.columns[1].x - 75.0).abs() < 0.01);
    let other = after_layout.at_path(&b).unwrap();
    assert_eq!(
        other.scroll_on(Orientation::Horizontal).unwrap().offset,
        0.0
    );
    let changes = |bounds: Rect| {
        (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
            (bounds.x as u32..(bounds.x + bounds.width) as u32)
                .any(|x| before.pixel(x, y) != after.pixel(x, y))
        })
    };
    assert!(changes(after_layout.at_path(&a).unwrap().bounds));
    assert!(!changes(other.bounds));
}

#[test]
fn nested_virtual_list_scroll_repaints_only_its_own_viewport() {
    use list_ui::NestedListsDemo;
    use xrack_runtime::{Component, Orientation};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = NestedListsDemo::new();
    let interpreted = interpret_example("examples/list/list.xr")
        .create("NestedListsDemo")
        .unwrap();
    let viewport = Size::new(580.0, 330.0);
    let mut input = Interaction::default();
    let mut interpreted_input = Interaction::default();
    let a = [1, 0, 0, 1];
    let b = [1, 1, 0, 1];
    let (initial, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer
        .snapshot_prepared(&initial, layout.clone(), 580, 330, 1.0, &input)
        .unwrap();
    let (initial, interpreted_layout) = renderer.prepare_scene(
        &interpreted,
        interpreted.try_render().unwrap(),
        viewport,
        &interpreted_input,
    );
    let interpreted_before = renderer
        .snapshot_prepared(
            &initial,
            interpreted_layout.clone(),
            580,
            330,
            1.0,
            &interpreted_input,
        )
        .unwrap();
    assert_same_pixels(&interpreted_before, &before);
    assert!(input.accessibility_scroll_to(&layout, &a, Orientation::Vertical, 48.0));
    assert!(interpreted_input.accessibility_scroll_to(
        &interpreted_layout,
        &a,
        Orientation::Vertical,
        48.0
    ));
    let (scrolled, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer
        .snapshot_prepared(&scrolled, layout.clone(), 580, 330, 1.0, &input)
        .unwrap();
    let (interpreted_scrolled, interpreted_layout) = renderer.prepare_scene(
        &interpreted,
        interpreted.render_shared(),
        viewport,
        &interpreted_input,
    );
    let interpreted_after = renderer
        .snapshot_prepared(
            &interpreted_scrolled,
            interpreted_layout,
            580,
            330,
            1.0,
            &interpreted_input,
        )
        .unwrap();
    assert_same_pixels(&interpreted_after, &after);
    for scene in [&scrolled, &interpreted_scrolled] {
        assert_eq!(scene.at_path(&a).unwrap().list.unwrap().first_row, 2);
        assert_eq!(scene.at_path(&b).unwrap().list.unwrap().first_row, 0);
    }
    assert_eq!(interpreted.take_error(), None);
    let changed = |path: &[usize]| {
        let bounds = layout.at_path(path).unwrap().bounds;
        (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
            (bounds.x as u32..(bounds.x + bounds.width) as u32)
                .any(|x| before.pixel(x, y) != after.pixel(x, y))
        })
    };
    assert!(changed(&a));
    assert!(!changed(&b));
}

#[test]
fn list_view_only_builds_visible_rows_and_selects_absolute_indices() {
    use std::{cell::Cell, rc::Rc};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ListDemo::new();
    let evaluations = Rc::new(Cell::new(0));
    let count = evaluations.clone();
    ui.on_row_caption(move |index| {
        count.set(count.get() + 1);
        format!("记录 {}", index + 1)
    });
    let mut input = Interaction::default();
    let viewport = Size::new(540.0, 380.0);
    let path = [0, 2];
    let base = ui.render_shared();
    assert!(base.at_path(&path).unwrap().children.is_empty());
    assert_eq!(evaluations.get(), 0);
    let (first, layout) = renderer.prepare_scene(&ui, base, viewport, &input);
    let records = first.at_path(&path).unwrap();
    let item = layout.at_path(&path).unwrap();
    let first_count = records.children.len();
    assert_eq!(records.list.unwrap().row_count, 100_000);
    assert_eq!(records.list.unwrap().first_row, 0);
    assert!((6..=12).contains(&first_count));
    assert_eq!(evaluations.get(), first_count);
    let point = Point {
        x: item.bounds.x + 18.0,
        y: item.bounds.y + 10.0,
    };
    input.pointer_move(&first, &layout, point);
    input.pointer_press();
    assert_eq!(input.focused.as_ref().unwrap().path, path);

    assert!(input.scroll_y(&layout, 32.0));
    let (next, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&next, &layout);
    assert_eq!(next.at_path(&path).unwrap().list.unwrap().first_row, 1);
    assert_eq!(evaluations.get(), first_count + 1);
    assert!(input.pointer_release_target().is_none());

    assert!(input.scroll_y(&layout, 319_968.0));
    let (far, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&far, &layout);
    assert_eq!(far.at_path(&path).unwrap().list.unwrap().first_row, 10_000);
    assert_eq!(evaluations.get(), 2 * first_count + 1);
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    let action = input.select_group_option(&far, &layout, &target).unwrap();
    assert_eq!(action.path, path);
    assert_eq!(action.action, ControlAction::Select(10_000));
    assert!(ui.control_at(&far, &action.path, action.action));
    assert_eq!(ui.get_current(), 10_000);

    let (selected, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert!(selected.at_path(&path).unwrap().children[0].boolean("selected", false));
    assert_eq!(evaluations.get(), 2 * first_count + 1);
    let action = input
        .list_key(&selected, &layout, NavigationKey::End)
        .unwrap();
    assert!(ui.control_at(&selected, &action.path, action.action));
    assert_eq!(ui.get_current(), 99_999);
    assert_eq!(ui.get_selections(), 2);
    let (last, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&last, &layout);
    let records = last.at_path(&path).unwrap();
    let last_row = records.children.last().unwrap();
    assert!(last_row.boolean("selected", false));
    assert_eq!(
        last_row.children[0].children[0].text("text"),
        Some("记录 100000")
    );
    assert_eq!(
        evaluations.get(),
        2 * first_count + 1 + records.children.len()
    );

    let frame = renderer.snapshot(&last, 540, 380, 1.0, &input).unwrap();
    assert_eq!(frame.layout, layout);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("list-demo.png"))
            .unwrap();
    }

    let before = evaluations.get();
    let (clipped, _) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(540.0, 60.0), &input);
    assert!(clipped.at_path(&path).unwrap().children.is_empty());
    assert_eq!(evaluations.get(), before);

    ui.set_count(4);
    let (shorter, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_eq!(layout.at_path(&path).unwrap().scroll.unwrap().offset, 0.0);
    let records = shorter.at_path(&path).unwrap();
    assert_eq!(records.list.unwrap().first_row, 0);
    assert_eq!(records.children.len(), 4);
}
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    Color, Component, ControlAction, DialInputMode, DialogRole, Length, Node, Orientation,
    RangeAction, RowSelection, SceneTransition, SelectionTarget, SpinAction, TableSelectionAction,
    TextWrap, TreeAction, TreeSelectionAction, Value, VecModel,
};

#[test]
fn flow_layout_rewraps_the_command_bar_and_keeps_hit_targets_in_sync() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut demo = FlowDemo::new();
    let scene = demo.render_shared();
    let mut input = Interaction::default();
    let wide = renderer.snapshot(&scene, 570, 260, 1.0, &input).unwrap();
    let compact = renderer.snapshot(&scene, 360, 260, 1.0, &input).unwrap();
    let button = [0, 1, 2];
    let wide_export = wide.layout.at_path(&button).unwrap().bounds;
    let wrapped_export = compact.layout.at_path(&button).unwrap().bounds;
    let first = compact.layout.at_path(&[0, 1, 0]).unwrap().bounds;
    let wide_status = wide.layout.at_path(&[0, 2]).unwrap().bounds;
    let compact_status = compact.layout.at_path(&[0, 2]).unwrap().bounds;
    assert!(wrapped_export.y > wide_export.y);
    assert_eq!(wrapped_export.x, first.x);
    assert_eq!(compact_status.y - wide_status.y, 50.0);
    assert!(compact_status.y > wrapped_export.y + wrapped_export.height);
    let point = Point {
        x: wrapped_export.x + 12.0,
        y: wrapped_export.y + 12.0,
    };
    assert_eq!(
        compact.pixel(point.x as u32, point.y as u32),
        Some([0x51, 0x3b, 0x86, 255])
    );
    input.pointer_move(&scene, &compact.layout, point);
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, button);
    assert!(demo.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(demo.get_selected(), "已导出");
}

#[test]
fn editable_combo_clips_completion_and_ime_preedit_to_its_entry() {
    use xrack_layout::ComboGeometry;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = EditableChoiceDemo::new();
    ui.set_draft("研".into());
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let layout = renderer.layout_with_input(&scene, Size::new(470.0, 278.0), &input);
    let geometry = ComboGeometry::new(layout.at_path(&[0, 1]).unwrap().bounds);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: geometry.entry.x + 12.0,
            y: geometry.entry.y + 12.0,
        },
    );
    input.pointer_press();
    input.pointer_release_target();
    input.sync_editor(&scene);
    let before = renderer.snapshot(&scene, 470, 278, 1.0, &input).unwrap();
    let painted = renderer.take_painted_combo_completion().unwrap();
    assert_eq!(painted.suffix(), "发");
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("中文预览".into(), Some((6, 6)));
    let composing = renderer.snapshot(&scene, 470, 278, 1.0, &input).unwrap();
    assert!(renderer.take_painted_combo_completion().is_none());
    let changed_pixels = |rect: xrack_layout::Rect, frame: &xrack_renderer_femtovg::Snapshot| {
        let left = rect.x as u32;
        let right = (rect.x + rect.width) as u32;
        let top = rect.y as u32;
        let bottom = (rect.y + rect.height) as u32;
        (top..bottom)
            .flat_map(|y| (left..right).map(move |x| (x, y)))
            .filter(|&(x, y)| before.pixel(x, y) != frame.pixel(x, y))
            .count()
    };
    assert!(changed_pixels(geometry.entry, &composing) > 0);
    assert_eq!(changed_pixels(geometry.trigger, &composing), 0);

    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit(String::new(), None);
    input.set_painted_combo_completion(Some(painted));
    assert!(input.dismiss_combo_completion(&scene));
    let dismissed = renderer.snapshot(&scene, 470, 278, 1.0, &input).unwrap();
    assert!(changed_pixels(geometry.entry, &dismissed) > 0);
    assert_eq!(changed_pixels(geometry.trigger, &dismissed), 0);

    let long = "研".repeat(25);
    ui.set_draft(long.clone());
    ui.bind_model_workspaces(xrack_runtime::VecModel::from(vec![format!("{long}发")]));
    input.editor = Some(TextEdit::at_end(&long));
    let scene = ui.render_shared();
    assert!(
        input
            .combo_completion(scene.at_path(&[0, 1]).unwrap(), &[0, 1])
            .is_some()
    );
    renderer.snapshot(&scene, 220, 278, 1.0, &input).unwrap();
    assert!(renderer.take_painted_combo_completion().is_none());
}

#[test]
fn check_group_master_repaints_mixed_and_checked_marks() {
    use controls_ui::CheckGroupDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        CheckGroupDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(520.0, 266.0),
    );
    let mixed = renderer
        .snapshot(ui.scene(), 520, 266, 1.0, ui.input())
        .unwrap();
    let rect = mixed.layout.at_path(&[0, 1]).unwrap().bounds;
    ui.click("select-all");
    let checked = renderer
        .snapshot(ui.scene(), 520, 266, 1.0, ui.input())
        .unwrap();
    let x = rect.x as u32;
    let y = (rect.y + rect.height * 0.5) as u32;
    assert!(
        (y.saturating_sub(8)..y + 8)
            .any(|y| (x + 8..x + 25).any(|x| mixed.pixel(x, y) != checked.pixel(x, y)))
    );
    assert_eq!(
        ui.node("run-validation").check_state(),
        xrack_runtime::CheckState::Checked
    );
    assert!(ui.node("use-background").boolean("checked", false));
}

#[test]
fn radio_selection_paints_across_independent_layout_columns() {
    use selection_ui::GroupedRadiosDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        GroupedRadiosDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(540.0, 280.0),
    );
    let balanced = renderer
        .snapshot(ui.scene(), 540, 280, 1.0, ui.input())
        .unwrap();
    let dot = |path: &[usize]| {
        let rect = balanced.layout.at_path(path).unwrap().bounds;
        ((rect.x + 14.0) as u32, (rect.y + rect.height * 0.5) as u32)
    };
    let (fast_x, fast_y) = dot(&[0, 1, 0, 0]);
    let (balanced_x, balanced_y) = dot(&[0, 1, 1, 0]);
    ui.update(|ui| ui.set_mode("fast".into()));
    let fast = renderer
        .snapshot(ui.scene(), 540, 280, 1.0, ui.input())
        .unwrap();
    assert_ne!(balanced.pixel(fast_x, fast_y), fast.pixel(fast_x, fast_y));
    assert_ne!(
        balanced.pixel(balanced_x, balanced_y),
        fast.pixel(balanced_x, balanced_y)
    );
    assert_eq!(
        balanced.pixel(balanced_x, balanced_y),
        fast.pixel(fast_x, fast_y)
    );
}

#[test]
fn styled_combo_popup_paints_the_panel_border_and_scroll_indicator() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let viewport = Size::new(420.0, 304.0);
    let mut generated = UiDriver::new(
        selection_ui::ComboPopupStyleDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        viewport,
    );
    let mut interpreted = UiDriver::new(
        interpret_example("examples/selection/selection.xr")
            .create("ComboPopupStyleDemo")
            .unwrap(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        viewport,
    );
    generated.click("popup-choice");
    interpreted.click("popup-choice");
    let draw = |renderer: &mut Renderer, scene: &xrack_runtime::Node, input: &Interaction| {
        renderer.snapshot(scene, 420, 304, 1.0, input).unwrap()
    };
    let start = draw(&mut renderer, generated.scene(), generated.input());
    assert_same_pixels(
        &start,
        &draw(&mut renderer, interpreted.scene(), interpreted.input()),
    );
    let menu = generated
        .input()
        .choice_menu(generated.scene(), &start.layout)
        .unwrap();
    let x = menu.bounds.x as u32 + 25;
    let y = menu.bounds.y as u32;
    assert_eq!(start.pixel(x, y + 1), Some([0xde, 0x9f, 0x53, 0xff]));
    assert_eq!(start.pixel(x, y + 5), Some([0x26, 0x39, 0x53, 0xff]));
    let thumb = menu.scroll_indicator().unwrap();
    let normal_width = menu.bounds.width;
    let normal_rows = menu.rows.len();
    let normal_row = menu.row_bounds(0);
    let indicator_pixel = ((thumb.x + thumb.width * 0.5) as u32, (thumb.y + 8.0) as u32);
    assert_ne!(
        start.pixel(indicator_pixel.0, indicator_pixel.1),
        Some([0x26, 0x39, 0x53, 0xff])
    );
    generated.choice_key(ChoiceKey::End);
    interpreted.choice_key(ChoiceKey::End);
    let end = draw(&mut renderer, generated.scene(), generated.input());
    assert_same_pixels(
        &end,
        &draw(&mut renderer, interpreted.scene(), interpreted.input()),
    );
    assert_eq!(
        end.pixel(indicator_pixel.0, indicator_pixel.1),
        Some([0x26, 0x39, 0x53, 0xff])
    );

    generated.choice_key(ChoiceKey::Home);
    interpreted.choice_key(ChoiceKey::Home);
    assert!(renderer.set_text_scale(2.0));
    let larger = draw(&mut renderer, generated.scene(), generated.input());
    let large_menu = generated
        .input()
        .choice_menu(generated.scene(), &larger.layout)
        .unwrap();
    assert_eq!(large_menu.rows[0], "Alpha");
    assert_eq!(large_menu.row_bounds(0).height, 64.0);
    assert_eq!(large_menu.bounds.width, normal_width);
    assert!(large_menu.bounds.height <= 120.0);
    assert!(large_menu.rows.len() < normal_rows);
    let ink = |frame: &xrack_renderer_femtovg::Snapshot, row: xrack_layout::Rect| {
        let left = row.x as u32;
        ((row.y + 4.0) as u32..(row.y + row.height - 4.0) as u32)
            .filter(|&y| {
                (left + 10..left + 106).any(|x| frame.pixel(x, y) != frame.pixel(left + 6, y))
            })
            .count()
    };
    assert!(ink(&larger, large_menu.row_bounds(0)) * 2 > ink(&start, normal_row) * 3);
    assert_same_pixels(
        &larger,
        &draw(&mut renderer, interpreted.scene(), interpreted.input()),
    );
}

#[test]
fn system_choice_marks_separate_the_selection_from_keyboard_highlight_in_both_directions() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let program = interpret_example("examples/selection/selection.xr");
    for rtl in [false, true] {
        let viewport = Size::new(420.0, 304.0);
        let mut native = selection_ui::ComboPopupStyleDemo::new();
        native.set_rtl(rtl);
        let interpreted = program.create("ComboPopupStyleDemo").unwrap();
        interpreted.set("rtl", rtl.into()).unwrap();
        let mut native = UiDriver::headless(native, viewport);
        let mut interpreted = UiDriver::headless(interpreted, viewport);
        native.click("popup-choice");
        interpreted.click("popup-choice");
        native.choice_key(ChoiceKey::Down);
        interpreted.choice_key(ChoiceKey::Down);
        let snapshot = |renderer: &mut Renderer| {
            renderer
                .snapshot(native.scene(), 420, 304, 1.0, native.input())
                .unwrap()
        };
        let plain = snapshot(&mut renderer);
        let menu = native
            .input()
            .choice_menu(native.scene(), &plain.layout)
            .unwrap();
        assert_eq!((menu.selected, menu.highlighted), (Some(0), Some(1)));
        let selected = menu.row_bounds(0);
        let highlighted = menu.row_bounds(1);

        assert!(renderer.set_system_differentiate_without_color(true));
        let marked = snapshot(&mut renderer);
        assert_eq!(plain.layout, marked.layout);
        assert_same_pixels(
            &marked,
            &renderer
                .snapshot(interpreted.scene(), 420, 304, 1.0, interpreted.input())
                .unwrap(),
        );
        let mark_x = if rtl {
            selected.x + 10.0
        } else {
            selected.x + selected.width - 10.0
        } as u32;
        let changed_at_mark = |row: xrack_layout::Rect| {
            let center_y = (row.y + row.height * 0.5) as u32;
            (center_y - 4..center_y + 5).any(|y| plain.pixel(mark_x, y) != marked.pixel(mark_x, y))
        };
        assert!(changed_at_mark(selected));
        assert!(!changed_at_mark(highlighted));
        let center = (highlighted.x + highlighted.width * 0.5) as u32;
        let edge = (highlighted.y + 2.0) as u32;
        assert_ne!(plain.pixel(center, edge), marked.pixel(center, edge));

        assert!(renderer.set_system_differentiate_without_color(false));
        assert_same_pixels(&plain, &snapshot(&mut renderer));
    }
}

#[test]
fn combo_option_artwork_matches_between_backends_and_reflects_popup_state() {
    use selection_ui::RecordComboDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        RecordComboDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(472.0, 296.0),
    );
    let mut interpreted = UiDriver::new(
        interpret_example("examples/selection/selection.xr")
            .create("RecordComboDemo")
            .unwrap(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(472.0, 296.0),
    );
    ui.click("choice");
    interpreted.click("choice");
    ui.choice_key(ChoiceKey::End);
    interpreted.choice_key(ChoiceKey::End);
    let disabled = renderer
        .snapshot(ui.scene(), 472, 296, 1.0, ui.input())
        .unwrap();
    let counterpart = renderer
        .snapshot(interpreted.scene(), 472, 296, 1.0, interpreted.input())
        .unwrap();
    assert_same_pixels(&disabled, &counterpart);
    let (row, plain) = {
        let menu = ui
            .input()
            .choice_menu(ui.scene(), &disabled.layout)
            .unwrap();
        let index = menu
            .rows
            .iter()
            .position(|label| label == "Build archive")
            .unwrap();
        assert!(!menu.row_enabled(index));
        let pixel_at = |index| {
            let row = menu.row_bounds(index);
            disabled.pixel(row.x as u32 + 3, row.y as u32 + 12)
        };
        assert_eq!(pixel_at(0), Some([0x29, 0x4c, 0x65, 0xff]));
        assert_eq!(pixel_at(5), Some([0x24, 0x6a, 0x79, 0xff]));
        (menu.row_bounds(index), menu.row_bounds(4))
    };
    ui.update(|ui| {
        let options = ui.model_options();
        let mut row = options.row(3).unwrap();
        row.access.available = true;
        assert!(options.set_row(3, row));
    });
    let enabled = renderer
        .snapshot(ui.scene(), 472, 296, 1.0, ui.input())
        .unwrap();
    let x = row.x as u32;
    let y = row.y as u32;
    assert_eq!(
        disabled.pixel(x + 4, y + 12),
        Some([0x43, 0x37, 0x46, 0xff])
    );
    assert_eq!(enabled.pixel(x + 4, y + 12), Some([0x20, 0x31, 0x4a, 0xff]));
    let accent = xrack_runtime::Color(0x79513fff);
    ui.update(|ui| ui.set_option_fill(accent));
    interpreted.update(|ui| ui.set("option-fill", accent.into()).unwrap());
    assert!(ui.input().choice_artwork_dirty(ui.scene(), &enabled.layout));
    assert!(
        interpreted
            .input()
            .choice_artwork_dirty(interpreted.scene(), &counterpart.layout)
    );
    let sample = |renderer: &mut Renderer, scene: &xrack_runtime::Node, input: &Interaction| {
        renderer
            .snapshot(scene, 472, 296, 1.0, input)
            .unwrap()
            .pixel(plain.x as u32 + 3, plain.y as u32 + 12)
    };
    assert_eq!(
        sample(&mut renderer, ui.scene(), ui.input()),
        Some([0x79, 0x51, 0x3f, 0xff])
    );
    assert_eq!(
        sample(&mut renderer, interpreted.scene(), interpreted.input()),
        Some([0x79, 0x51, 0x3f, 0xff])
    );
    assert!(!ui.input().choice_artwork_dirty(ui.scene(), &enabled.layout));
    ui.update(|ui| {
        let model = ui.model_options();
        let mut row = model.row(4).unwrap();
        row.notes = "紧急".into();
        assert!(model.set_row(4, row));
    });
    interpreted.update(|ui| {
        let options = ui.record_model("options").unwrap();
        let model = options.model();
        let row = model
            .row(4)
            .unwrap()
            .with_field(&["notes".into()], "紧急".to_owned().into())
            .unwrap();
        assert!(model.set_row(4, row));
    });
    assert!(ui.input().choice_artwork_dirty(ui.scene(), &enabled.layout));
    assert!(
        interpreted
            .input()
            .choice_artwork_dirty(interpreted.scene(), &counterpart.layout)
    );
    assert_eq!(
        sample(&mut renderer, ui.scene(), ui.input()),
        Some([0x9b, 0x4d, 0x42, 0xff])
    );
    assert_eq!(
        sample(&mut renderer, interpreted.scene(), interpreted.input()),
        Some([0x9b, 0x4d, 0x42, 0xff])
    );
    ui.choice_key(ChoiceKey::Accept);
    interpreted.choice_key(ChoiceKey::Accept);
    assert_eq!(ui.component().get_code(), 600);
    assert_eq!(
        interpreted.component().get("code").unwrap(),
        xrack_runtime::Value::Int(600)
    );
    let display = renderer
        .snapshot(ui.scene(), 472, 296, 1.0, ui.input())
        .unwrap();
    let area = display.layout.at_path(&[1, 0]).unwrap().bounds;
    let pixel = ((area.x + area.width - 8.0) as u32, (area.y + 8.0) as u32);
    assert_eq!(
        display.pixel(pixel.0, pixel.1),
        Some([0x2b, 0x46, 0x59, 0xff])
    );
    for choice in [
        ui.scene().child_at(1).unwrap(),
        interpreted.scene().child_at(1).unwrap(),
    ] {
        let caption = choice
            .combo_content()
            .unwrap()
            .child_at(0)
            .unwrap()
            .child_at(1)
            .unwrap();
        assert_eq!(caption.text("text"), Some("Board"));
    }
    ui.click("enable-editing");
    interpreted.click("enable-editing");
    let editor = renderer
        .snapshot(ui.scene(), 472, 296, 1.0, ui.input())
        .unwrap();
    let interpreted_editor = renderer
        .snapshot(interpreted.scene(), 472, 296, 1.0, interpreted.input())
        .unwrap();
    assert_same_pixels(&editor, &interpreted_editor);
    assert_ne!(
        display.pixel(pixel.0, pixel.1),
        editor.pixel(pixel.0, pixel.1)
    );
    assert_eq!(
        xrack_runtime::text_input_value(ui.scene().child_at(1).unwrap()),
        "Board"
    );
}

#[test]
fn editor_context_menu_is_drawn_over_the_form_and_clears_on_escape() {
    use form_ui::EditorMenuDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        EditorMenuDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(540.0, 420.0),
    );
    assert!(ui.context_menu("entry"));
    let bounds = ui.menus()[0].bounds;
    let open = renderer
        .snapshot(ui.scene(), 540, 420, 1.0, ui.input())
        .unwrap();
    ui.menu_key(MenuKey::Escape);
    let closed = renderer
        .snapshot(ui.scene(), 540, 420, 1.0, ui.input())
        .unwrap();
    let inside = (bounds.x as u32 + 10, bounds.y as u32 + 10);
    assert_ne!(
        open.pixel(inside.0, inside.1),
        closed.pixel(inside.0, inside.1)
    );
    assert_eq!(open.pixel(8, 8), closed.pixel(8, 8));
}

#[test]
fn exclusive_action_group_repaints_both_toolbar_choices_after_a_shortcut() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        ActionGroupDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(620.0, 300.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 620, 300, 1.0, ui.input())
        .unwrap();
    assert!(ui.menu_shortcut("Primary+2".parse().unwrap()));
    let after = renderer
        .snapshot(ui.scene(), 620, 300, 1.0, ui.input())
        .unwrap();
    for slot in 0..2 {
        let button = before.layout.at_path(&[0, 2, slot]).unwrap().bounds;
        let (x, y) = ((button.x + 8.0) as u32, (button.y + 8.0) as u32);
        assert_ne!(before.pixel(x, y), after.pixel(x, y));
    }
    ui.click("modes-menu");
    let panel = ui.menus();
    assert_eq!(panel[0].rows[0].checked, Some(false));
    assert_eq!(panel[0].rows[1].checked, Some(true));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("action-group.png"))
            .unwrap();
    }
}

#[test]
fn cascaded_menu_panels_render_and_stay_inside_the_viewport() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        MenuDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(620.0, 366.0),
    );
    let before = renderer
        .snapshot(ui.scene(), 620, 366, 1.0, ui.input())
        .unwrap();
    ui.click("file-menu");
    ui.menu_key(MenuKey::Down);
    ui.menu_key(MenuKey::Right);
    let panels = ui.menus();
    assert_eq!(panels.len(), 2);
    let shortcut_label = if cfg!(target_os = "macos") {
        "Cmd+N"
    } else {
        "Ctrl+N"
    };
    assert_eq!(panels[0].rows[0].shortcut.as_deref(), Some(shortcut_label));
    let shortcut_row = panels[0].row_bounds(0);
    for panel in panels {
        assert!(panel.bounds.x >= 0.0 && panel.bounds.y >= 0.0);
        assert!(panel.bounds.x + panel.bounds.width <= 620.0);
        assert!(panel.bounds.y + panel.bounds.height <= 366.0);
    }
    let frame = renderer
        .snapshot(ui.scene(), 620, 366, 1.0, ui.input())
        .unwrap();
    assert_ne!(before.rgba, frame.rgba);
    let right = (shortcut_row.x + shortcut_row.width) as u32;
    let top = shortcut_row.y as u32;
    let glyph_pixels = (top + 5..top + 23)
        .flat_map(|y| (right - 66..right - 12).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.pixel(x, y) != Some([0x25, 0x33, 0x4b, 255]))
        .count();
    assert!(glyph_pixels > 20, "shortcut column has no visible glyphs");
    let grid = before.layout.at_path(&[0, 2, 0]).unwrap().bounds;
    let sample = ((grid.x + 10.0) as u32, (grid.y + 10.0) as u32);
    assert!(ui.menu_shortcut("Primary+G".parse().unwrap()));
    let checked = renderer
        .snapshot(ui.scene(), 620, 366, 1.0, ui.input())
        .unwrap();
    assert_ne!(
        before.pixel(sample.0, sample.1),
        checked.pixel(sample.0, sample.1)
    );
    let has_icon = |frame: &xrack_renderer_femtovg::Snapshot, rect: xrack_layout::Rect| {
        let x = rect.x as u32..(rect.x + rect.width) as u32;
        let y = rect.y as u32..(rect.y + rect.height) as u32;
        y.flat_map(|y| x.clone().map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y) == Some([0xff, 0xb4, 0x54, 255]))
            .count()
            > 4
    };
    assert!(has_icon(&checked, grid));
    ui.click("view-menu");
    let panels = ui.menus();
    let panel = &panels[0];
    assert_eq!(
        panel.rows[0].icon,
        ui.node("grid-button").image_asset("icon")
    );
    assert_eq!(panel.rows[0].checked, Some(true));
    let icon = panel.icon_bounds(0);
    let menu_frame = renderer
        .snapshot(ui.scene(), 620, 366, 1.0, ui.input())
        .unwrap();
    assert!(has_icon(&menu_frame, icon));
    ui.menu_key(MenuKey::Escape);
    let narrow = renderer
        .snapshot(ui.scene(), 220, 366, 1.0, ui.input())
        .unwrap();
    let bar = narrow.layout.at_path(&[0, 2]).unwrap();
    assert!(bar.children[2].bounds.y > bar.children[0].bounds.y);
    assert!(bar.bounds.height > checked.layout.at_path(&[0, 2]).unwrap().bounds.height);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("menu-demo.png"))
            .unwrap();
    }
}

#[test]
fn custom_split_handles_paint_their_own_templates_and_state() {
    use split_ui::CustomSplitDemo;
    use xrack_runtime::Color;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CustomSplitDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(640.0, 240.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let handles = [3, 4].map(|slot| layout.at_path(&[0, slot]).unwrap().bounds);
    let first = handles[0];
    let hit = Point {
        x: first.x - 6.0,
        y: first.y + 12.0,
    };
    assert!(!first.contains(hit));
    input.pointer_move(&scene, &layout, hit);
    input.pointer_press_in_scene(&scene, &layout);
    input.split_press(&scene, &layout);
    ui.set_grip_color(Color(0xe8ed9cff));
    let active = renderer
        .snapshot(&ui.render_shared(), 640, 240, 1.0, &input)
        .unwrap();
    for (bounds, expected) in handles
        .into_iter()
        .zip([[0xff, 0xbf, 0x69, 255], [0x48, 0x5e, 0x7e, 255]])
    {
        let x = bounds.x as u32 + 6;
        let y = bounds.y as u32;
        assert_eq!(active.pixel(x, y + 12), Some(expected));
        assert_eq!(active.pixel(x, y + 34), Some([0xe8, 0xed, 0x9c, 255]));
    }
    assert_eq!(
        active.pixel(hit.x as u32, hit.y as u32),
        Some([0x26, 0x37, 0x4e, 255])
    );
}

#[test]
fn fill_split_keeps_the_side_divider_in_place_after_window_resize() {
    use split_ui::FillSplitDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = FillSplitDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(680.0, 320.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let work = [0, 2];
    let handle = layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .handles[0]
        .bounds;
    let point = Point {
        x: handle.x - 4.0,
        y: handle.y + 56.0,
    };
    assert!(!handle.contains(point));
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    input.split_press(&scene, &layout);
    let active = renderer
        .snapshot(&ui.render_shared(), 680, 320, 1.0, &input)
        .unwrap();
    assert_eq!(
        active.pixel(handle.x as u32 + 4, point.y as u32),
        Some([0xff, 0xbf, 0x69, 255])
    );
    assert_eq!(
        active.pixel(point.x as u32, point.y as u32),
        Some([0x26, 0x37, 0x4e, 255])
    );
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: point.x + 32.0,
            ..point
        },
    );
    let resize = input.split_drag(&scene, &layout).unwrap();
    assert!(ui.resize_split_sizes_at(&scene, &resize.path, resize.sizes.as_deref().unwrap()));
    assert!(input.finish_split_resize());

    let scene = ui.render_shared();
    let normal = renderer.snapshot(&scene, 680, 320, 1.0, &input).unwrap();
    let wide = renderer.snapshot(&scene, 880, 320, 1.0, &input).unwrap();
    for snapshot in [&normal, &wide] {
        let work = snapshot.layout.at_path(&work).unwrap();
        let after = work.split.as_ref().unwrap().handles[0].bounds;
        assert_eq!(after.x, handle.x + 32.0);
        assert_eq!(work.children[2].bounds.width, 160.0);
        assert_eq!(
            snapshot.pixel(point.x as u32 + 16, point.y as u32),
            Some([0x26, 0x37, 0x4e, 255])
        );
        assert_eq!(
            snapshot.pixel(handle.x as u32 + 54, point.y as u32),
            Some([0x30, 0x46, 0x63, 255])
        );
        assert_eq!(
            snapshot.pixel(after.x as u32 + 4, point.y as u32),
            Some([0x81, 0xa6, 0xe1, 255])
        );
    }
}

#[test]
fn split_view_renders_the_dragged_divider_and_nested_panels() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SplitDemo::new();
    let viewport = Size::new(760.0, 420.0);
    let work = [0, 2];
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let handle = layout
        .at_path(&work)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .handles[0]
        .bounds;
    let before = renderer.snapshot(&scene, 760, 420, 1.0, &input).unwrap();
    let point = Point {
        x: handle.x + 5.0,
        y: handle.y + 60.0,
    };
    assert_eq!(
        before.pixel(point.x as u32 + 20, point.y as u32),
        Some([0x30, 0x46, 0x63, 255])
    );
    input.pointer_move(&scene, &layout, point);
    input.pointer_press();
    input.split_press(&scene, &layout);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: point.x + 45.0,
            y: point.y,
        },
    );
    let resize = input.split_drag(&scene, &layout).unwrap();
    assert!(ui.resize_split_at(&scene, &resize.path, &resize.weights));
    assert!(input.finish_split_resize());
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&scene, &layout);
    let frame = renderer.snapshot(&scene, 760, 420, 1.0, &input).unwrap();
    let item = layout.at_path(&work).unwrap();
    let handle = item.split.as_ref().unwrap().handles[0].bounds;
    assert!((handle.x + 5.0 - (point.x + 45.0)).abs() < 0.05);
    assert_eq!(
        frame.pixel(point.x as u32 + 20, point.y as u32),
        Some([0x22, 0x32, 0x49, 255])
    );
    let nested = item.children[2].split.as_ref().unwrap().handles[0].bounds;
    assert_eq!(
        frame.pixel(
            (nested.x + 30.0) as u32,
            (nested.y + nested.height * 0.5) as u32
        ),
        Some([0x82, 0xa3, 0xe4, 255])
    );
    let saved = ui.save_split_state("work").unwrap();
    let mut restored = SplitDemo::new();
    restored.set_saved_layout(saved.clone());
    assert!(restored.restore_split_state("work", &saved));
    let restored_frame = renderer
        .snapshot(
            &restored.render_shared(),
            760,
            420,
            1.0,
            &Interaction::default(),
        )
        .unwrap();
    assert_eq!(
        restored_frame
            .layout
            .at_path(&work)
            .unwrap()
            .split
            .as_ref()
            .unwrap()
            .handles[0]
            .bounds,
        handle,
    );
    assert_eq!(
        restored_frame.pixel(point.x as u32 + 20, point.y as u32),
        frame.pixel(point.x as u32 + 20, point.y as u32),
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        restored_frame
            .save_png(std::path::Path::new(&path).join("split-workspace.png"))
            .unwrap();
    }
}

#[test]
fn tooltip_overlay_renders_custom_content_inside_the_window() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = TooltipDemo::new();
    let viewport = Size::new(510.0, 260.0);
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 510, 260, 1.0, &input).unwrap();
    input.pointer_move(&scene, &layout, Point { x: 480.0, y: 225.0 });
    let now = std::time::Instant::now();
    assert!(!input.update_tooltip(&scene, &layout, now));
    assert!(input.update_tooltip(&scene, &layout, now + std::time::Duration::from_millis(300)));
    let view = xrack_layout::Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let (_, overlay) = xrack_layout::layout_tooltip(
        input.tooltip_popup(&scene, &layout).unwrap(),
        view,
        &mut renderer.text,
    );
    assert!(overlay.bounds.y + overlay.bounds.height < 225.0);
    assert_eq!(overlay.bounds.intersect(view), overlay.bounds);
    let frame = renderer.snapshot(&scene, 510, 260, 1.0, &input).unwrap();
    let sample = (
        (overlay.bounds.x + 4.0) as u32,
        (overlay.bounds.y + 6.0) as u32,
    );
    assert_eq!(
        frame.pixel(sample.0, sample.1),
        Some([0x25, 0x33, 0x4b, 255])
    );
    assert_ne!(
        before.pixel(sample.0, sample.1),
        frame.pixel(sample.0, sample.1)
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("tooltip-overlay.png"))
            .unwrap();
    }
    assert!(input.dismiss_tooltip());
    let after = renderer.snapshot(&scene, 510, 260, 1.0, &input).unwrap();
    assert_eq!(after.rgba, before.rgba);
}

#[test]
fn programmatic_tree_expansion_paints_the_requested_deep_node() {
    use xrack_runtime::{ScrollRequests, TreeCommand};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TreeCommandsDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(610.0, 440.0);
    let path = [0, 2];
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 610, 440, 1.0, &input).unwrap();
    assert_eq!(scene.at_path(&path).unwrap().children.len(), 2);
    let scroll = ScrollRequests::default();
    {
        let _context = scroll.enter();
        ui.invoke_reveal(103);
    }
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.apply_scroll_requests(&scene, &layout, scroll.drain());
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let frame = renderer.snapshot(&scene, 610, 440, 1.0, &input).unwrap();
    assert_eq!(scene.at_path(&path).unwrap().children.len(), 7);
    assert_eq!(ui.get_current(), 103);
    let tree = frame.layout.at_path(&path).unwrap();
    let left = tree.children[0].children[0].bounds.x;
    let target = tree.children[3].children[0].bounds.x;
    assert!((target - left - 48.0).abs() < 0.1);
    assert_ne!(before.rgba, frame.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("tree-commands.png"))
            .unwrap();
    }
    assert!(ui.command_tree("directory", TreeCommand::CollapseRecursively(-1)));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let collapsed = renderer.snapshot(&scene, 610, 440, 1.0, &input).unwrap();
    assert_eq!(scene.at_path(&path).unwrap().children.len(), 2);
    assert_eq!(ui.get_current(), 1);
    assert_ne!(frame.rgba, collapsed.rgba);
}

#[test]
fn rooted_tree_views_paint_shared_rows_with_relative_indentation() {
    use xrack_runtime::TreeCommand;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TreeBranchesDemo::new();
    ui.invoke_initialize();
    assert!(ui.command_tree("branch", TreeCommand::ExpandTo(103)));
    ui.set_folder(10);
    let input = Interaction::default();
    let viewport = Size::new(760.0, 430.0);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let frame = renderer.snapshot(&scene, 760, 430, 1.0, &input).unwrap();
    let overview = [0, 2, 0, 1];
    let branch = [0, 2, 1, 1];
    let global = scene.at_path(&overview).unwrap();
    let scoped = scene.at_path(&branch).unwrap();
    assert_eq!(scoped.children.len(), 3);
    assert_eq!(scoped.children[1].row_key, global.children[3].row_key);
    let area = frame.layout.at_path(&branch).unwrap();
    let first = area.children[0].children[0].bounds.x;
    let child = area.children[1].children[0].bounds.x;
    assert!((child - first - 16.0).abs() < 0.1);
    assert_eq!(scoped.tree.as_ref().unwrap().rows()[1].depth, 1);
    ui.set_query("WINDOW".into());
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_eq!(
        scene.at_path(&overview).unwrap().children.len(),
        global.children.len()
    );
    assert_eq!(scene.at_path(&branch).unwrap().children.len(), 2);
    let filtered = renderer.snapshot(&scene, 760, 430, 1.0, &input).unwrap();
    assert_ne!(filtered.rgba, frame.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("tree-branches.png"))
            .unwrap();
        filtered
            .save_png(std::path::Path::new(&path).join("tree-filter.png"))
            .unwrap();
    }
}

#[test]
fn tree_view_renders_disclosures_and_indents_expanded_rows() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TreeDemo::new();
    let viewport = Size::new(600.0, 370.0);
    let path = [0, 1];
    let mut input = Interaction::default();
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    for slot in [0, 1] {
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
        let row_path = [0, 1, slot];
        let bounds = layout.at_path(&row_path).unwrap().bounds;
        let geometry =
            xrack_layout::TreeRowGeometry::new(scene.at_path(&row_path).unwrap(), bounds).unwrap();
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: geometry.indicator.x + 10.0,
                y: geometry.indicator.y + 10.0,
            },
        );
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        let action = input.tree_pointer_action(&scene, &layout, &target).unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let tree = layout.at_path(&path).unwrap();
    assert_eq!(scene.at_path(&path).unwrap().children.len(), 5);
    for index in 0..2 {
        let left = tree.children[index].children[0].bounds.x;
        let next = tree.children[index + 1].children[0].bounds.x;
        assert_eq!(next - left, 18.0);
    }
    let frame = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    assert_ne!(frame.rgba, before.rgba);
    let root = [0, 1, 0];
    let geometry = xrack_layout::TreeRowGeometry::new(
        scene.at_path(&root).unwrap(),
        layout.at_path(&root).unwrap().bounds,
    )
    .unwrap();
    let center = (
        (geometry.indicator.x + 11.0) as u32,
        (geometry.indicator.y + 16.0) as u32,
    );
    assert_eq!(
        frame.pixel(center.0, center.1),
        Some([0xc0, 0xd1, 0xea, 255])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("tree-directory.png"))
            .unwrap();
    }
    assert!(ui.control_at(&scene, &path, ControlAction::Select(1)));
    let leaf = scene.at_path(&path).unwrap().tree.as_ref().unwrap().rows()[2];
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Row {
            key: leaf.key,
            row: leaf.row,
            operation: RowSelection::Toggle,
        }))
    ));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let selected = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    let branch = layout.at_path(&[0, 1, 1]).unwrap().bounds;
    assert_eq!(
        selected.pixel((branch.x + 1.0) as u32, (branch.y + 6.0) as u32),
        Some([0x9a, 0xad, 0xff, 255])
    );
    let detail = layout.at_path(&[0, 1, 2, 0, 0, 1]).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: detail.x + 8.0,
            y: detail.y + 12.0,
        },
    );
    input.pointer_press();
    assert_eq!(input.pointer_release_target().unwrap().path, [0, 1, 2]);
    assert_eq!(input.focused.as_ref().unwrap().path, path);
    assert!(input.begin_table_edit(&scene, &layout));
    let original = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("应用入口.rs"),
        EditSource::Atomic,
        std::time::Instant::now(),
    ));
    let draft = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    assert_ne!(draft.rgba, original.rgba);
    assert_eq!(
        ui.model_records().row(leaf.model_index).unwrap().label,
        "main.rs"
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        draft
            .save_png(std::path::Path::new(&path).join("tree-rename.png"))
            .unwrap();
    }
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(
        ui.model_records().row(leaf.model_index).unwrap().label,
        "应用入口.rs"
    );

    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let original = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    let active = [0, 1, 2, 0, 0, 2];
    let quota = [0, 1, 2, 0, 0, 3];
    let load = [0, 1, 2, 0, 0, 4];
    assert!(ui.control_at(&scene, &quota, ControlAction::Spin(SpinAction::Commit(17))));
    assert!(ui.adjust_range_at(&scene, &load, RangeAction::SetFraction(0.8)));
    assert!(ui.dispatch_at(&scene, &active, "clicked"));
    let record = ui.model_records().row(leaf.model_index).unwrap();
    assert_eq!(record.quota, 17);
    assert!((record.load - 0.8).abs() < 1e-9);
    assert!(!record.active);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let edited = renderer.snapshot(&scene, 600, 370, 1.0, &input).unwrap();
    for path in [active, quota, load] {
        let rect = layout.at_path(&path).unwrap().bounds;
        let changed = (rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
            (rect.x as u32..(rect.x + rect.width) as u32)
                .any(|x| original.pixel(x, y) != edited.pixel(x, y))
        });
        assert!(
            changed,
            "expected the edited control to repaint at {path:?}"
        );
    }
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        edited
            .save_png(std::path::Path::new(&path).join("tree-editors.png"))
            .unwrap();
    }
}

#[test]
fn anchored_popup_flips_above_near_the_window_edge_and_only_dims_when_modal() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = PopupDemo::new();
    let input = Interaction::default();
    let capture = |ui: &PopupDemo, renderer: &mut Renderer| {
        renderer
            .snapshot(&ui.render(), 480, 300, 1.0, &input)
            .unwrap()
    };
    let initial = capture(&ui, &mut renderer);
    ui.set_filters_open(true);
    let popup = capture(&ui, &mut renderer);
    assert_eq!(popup.layout.overlay_paths(), &[vec![3, 0]]);
    assert!(popup.layout.dialog_paths().is_empty());
    assert_eq!(
        popup.layout.at_path(&[3, 0]).unwrap().bounds,
        xrack_layout::Rect::new(260.0, 76.0, 212.0, 136.0)
    );
    assert_eq!(popup.pixel(270, 84), Some([0x2b, 0x4e, 0x58, 255]));
    assert_eq!(popup.pixel(24, 240), initial.pixel(24, 240));
    ui.set_modal(true);
    let modal = capture(&ui, &mut renderer);
    assert_eq!(modal.layout.dialog_paths(), &[vec![3, 0]]);
    assert_ne!(modal.pixel(24, 240), initial.pixel(24, 240));
    assert_eq!(modal.pixel(270, 84), popup.pixel(270, 84));
}

#[test]
fn label_elision_changes_gpu_pixels_and_keeps_the_measured_line_plan() {
    use xrack_layout::LabelLayout;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = LabelsDemo::new();
    let scene = ui.render_shared();
    let input = Interaction::default();
    let before = renderer.snapshot(&scene, 300, 310, 1.0, &input).unwrap();
    let path = [0, 1, 0];
    let bounds = before.layout.at_path(&path).unwrap().bounds;
    let plan = |node: &xrack_runtime::Node, bounds: xrack_layout::Rect| {
        LabelLayout::new(node, Size::new(bounds.width, bounds.height), |value| {
            renderer.text.advance(value, font_size(node))
        })
        .lines
        .into_iter()
        .map(|line| (line.text.into_owned(), line.origin))
        .collect::<Vec<_>>()
    };
    let title = plan(scene.at_path(&path).unwrap(), bounds);
    assert_eq!(title.len(), 1);
    assert!(title[0].0.ends_with('…'));
    assert!(title[0].1.y > 0.0);
    let summary_path = [0, 2, 0];
    let summary = plan(
        scene.at_path(&summary_path).unwrap(),
        before.layout.at_path(&summary_path).unwrap().bounds,
    );
    assert_eq!(summary.len(), 2);
    assert!(summary[1].0.ends_with('…'));
    assert!(summary[0].1.y > 0.0);

    ui.set_elided(false);
    let after = renderer
        .snapshot(&ui.render(), 300, 310, 1.0, &input)
        .unwrap();
    assert_eq!(after.layout.at_path(&path).unwrap().bounds, bounds);
    assert!(
        (bounds.y as u32..(bounds.y + bounds.height) as u32).any(|y| {
            (bounds.x as u32..(bounds.x + bounds.width) as u32)
                .any(|x| before.pixel(x, y) != after.pixel(x, y))
        })
    );
}

#[test]
fn flexbox_wrap_and_reverse_share_gpu_geometry_with_hit_testing() {
    use xrack_runtime::FlexboxLayoutDirection;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = FlexDemo::new();
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let wide = renderer.snapshot(&scene, 740, 390, 1.0, &input).unwrap();
    let path = [0, 2];
    let wide_items = &wide.layout.at_path(&path).unwrap().children;
    assert!(wide_items[1].bounds.x < wide_items[0].bounds.x);
    assert!(wide_items[1].bounds.width > 160.0);
    assert_eq!(wide_items[3].bounds.width, 120.0);

    ui.set_direction(FlexboxLayoutDirection::RowReverse);
    let scene = ui.render_shared();
    let narrow = renderer.snapshot(&scene, 390, 390, 1.0, &input).unwrap();
    let items = &narrow.layout.at_path(&path).unwrap().children;
    assert!(items[1].bounds.x > items[0].bounds.x);
    assert!(items[2].bounds.y > items[1].bounds.y);
    assert!(items[2].bounds.x > items[3].bounds.x);
    let item = items[2].bounds;
    assert_eq!(
        narrow.pixel(item.x as u32 + 8, item.y as u32 + 8),
        Some([0x51, 0x3b, 0x86, 255])
    );
    input.pointer_move(
        &scene,
        &narrow.layout,
        Point {
            x: item.x + item.width * 0.5,
            y: item.y + item.height * 0.5,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.id, Some("flex-export"));
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(ui.get_status(), "已导出");
}

#[test]
fn timer_tick_updates_the_progress_indicator_in_the_next_gpu_frame() {
    use std::time::{Duration, Instant};
    use xrack_runtime::TimerQueue;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = TimerDemo::new();
    let scene = ui.render_shared();
    let input = Interaction::default();
    let before = renderer.snapshot(&scene, 460, 240, 1.0, &input).unwrap();
    let mut queue = TimerQueue::default();
    let start = Instant::now();
    queue.sync(start, &scene.timers);
    assert!(
        queue
            .next_due(start + Duration::from_millis(250))
            .unwrap()
            .fire()
    );
    let after = renderer
        .snapshot(&ui.render_shared(), 460, 240, 1.0, &input)
        .unwrap();
    let bar = before.layout.at_path(&[0, 1]).unwrap().bounds;
    let x = (bar.x + bar.width * 0.875) as u32;
    let y = (bar.y + bar.height * 0.5) as u32;
    assert_ne!(before.pixel(x, y), after.pixel(x, y));
}

#[test]
fn swipe_delegate_paints_only_the_exposed_part_of_the_action_panel() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;
    use xrack_runtime::SwipeSide;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SwipeDemo::new();
    ui.set_messages(vec!["Mail preview".into()]);
    let viewport = Size::new(560.0, 370.0);
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let path = [0, 2, 0, 0];
    let panel = [0, 2, 0, 0, 2];
    let bounds = layout.at_path(&path).unwrap().bounds;
    let start = Point {
        x: bounds.x + 180.0,
        y: bounds.y + 18.0,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    let moved = Point {
        x: start.x - 52.0,
        ..start
    };
    assert!(input.move_scroll_gesture(&scene, &layout, moved, now + Duration::from_millis(20)));
    let preview = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(preview.layout.at_path(&panel).unwrap().clip.width, 52.0);
    let y = bounds.y as u32 + 5;
    let right = (bounds.x + bounds.width) as u32;
    assert_eq!(preview.pixel(right - 60, y), Some([0x26, 0x37, 0x50, 255]));
    assert_ne!(preview.pixel(right - 20, y), preview.pixel(right - 60, y));
    let action = input
        .end_scroll_gesture(
            &scene,
            &preview.layout,
            moved,
            now + Duration::from_millis(40),
        )
        .control
        .unwrap();
    assert_eq!(
        action.action,
        ControlAction::SwipeDelegate(SwipeSide::Right)
    );
    assert!(ui.control_at(&scene, &action.path, action.action));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let settled = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(settled.pixel(right - 60, y), Some([0x35, 0x5b, 0x50, 255]));
    assert!(settled.layout.at_path(&panel).unwrap().enabled);
}

#[test]
fn standard_dialog_button_box_wraps_and_distinguishes_danger_on_gpu() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardDialogDemo::new();
    ui.set_settings_open(true);
    let scene = ui.render_shared();
    let input = Interaction::default();
    let wide = renderer.snapshot(&scene, 620, 380, 1.0, &input).unwrap();
    let narrow = renderer.snapshot(&scene, 350, 380, 1.0, &input).unwrap();
    let box_path = [3, 2];
    let wide_box = wide.layout.at_path(&box_path).unwrap();
    let narrow_box = narrow.layout.at_path(&box_path).unwrap();
    let order =
        xrack_layout::child_reading_order(scene.at_path(&box_path).unwrap()).collect::<Vec<_>>();
    assert_eq!(order[0], 5);
    assert_eq!(order.last(), Some(&0));
    let positions: Vec<_> = order
        .iter()
        .map(|&index| narrow_box.children[index].bounds)
        .collect();
    assert!(
        positions
            .windows(2)
            .all(|pair| pair[0].y < pair[1].y || pair[0].y == pair[1].y && pair[0].x < pair[1].x)
    );
    assert!(positions.last().unwrap().y > positions[0].y);
    assert!(narrow_box.bounds.height > wide_box.bounds.height);
    let pixel = |index: usize| {
        let bounds = wide_box.children[index].bounds;
        wide.pixel(bounds.x as u32 + 8, bounds.y as u32 + 8)
    };
    assert_eq!(pixel(0), Some([0x53, 0x6d, 0xfe, 255]));
    assert_eq!(pixel(1), Some([0x38, 0x49, 0x66, 255]));
    assert_eq!(pixel(3), Some([0xa6, 0x38, 0x43, 255]));
}

#[test]
fn record_height_changes_move_the_painted_boundary_of_a_virtual_list() {
    use table_ui::{LogRow, ProjectedRowsDemo};
    use xrack_runtime::Length;

    let mut ui = ProjectedRowsDemo::new();
    ui.set_entries(vec![
        LogRow {
            message: "Warning".into(),
            severity: "warn".into(),
            height: Length(60.0),
        },
        LogRow {
            message: "Info".into(),
            severity: "info".into(),
            height: Length(0.0),
        },
    ]);
    let model = ui.model_entries();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut driver = UiDriver::new(
        ui,
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(660.0, 310.0),
    );
    let before = renderer
        .snapshot(driver.scene(), 660, 310, 1.0, driver.input())
        .unwrap();
    driver.update(|_| {
        let mut row = model.row(0).unwrap();
        row.height = Length(20.0);
        assert!(model.set_row(0, row));
    });
    let after = renderer
        .snapshot(driver.scene(), 660, 310, 1.0, driver.input())
        .unwrap();
    assert_eq!(before.pixel(603, 84), Some([0x40, 0x59, 0x71, 0xff]));
    assert_eq!(after.pixel(603, 84), Some([0x26, 0x37, 0x50, 0xff]));
}

#[test]
fn resizing_a_table_row_repaints_the_new_boundary() {
    let mut ui = TableDemo::new();
    ui.set_resize_rows(true);
    ui.set_pinned(1);
    ui.set_records(vec!["A".into(), "B".into(), "C".into()]);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let mut input = Interaction::default();
    let (scene, frame) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let row = frame.at_path(&[0, 2, 0]).unwrap().bounds;
    let sample = ((row.x + 120.0) as u32, (row.y + 45.0) as u32);
    let before = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        before.pixel(sample.0, sample.1),
        Some([0x22, 0x32, 0x49, 0xff])
    );
    let start = Point {
        x: row.x + 100.0,
        y: row.y + 31.0,
    };
    input.pointer_move(&scene, &frame, start);
    input.pointer_press_in_scene(&scene, &frame);
    input.pointer_move(
        &scene,
        &frame,
        Point {
            x: start.x,
            y: start.y + 40.0,
        },
    );
    let action = input.table_row_resize_drag(&scene, &frame).unwrap();
    assert_eq!(action.path, path);
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert!(input.finish_table_resize());
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        after.pixel(sample.0, sample.1),
        Some([0x26, 0x37, 0x50, 0xff])
    );
    assert_eq!(
        after.layout.at_path(&[0, 2, 0]).unwrap().bounds.height,
        72.0
    );
}

#[test]
fn frozen_table_pane_clips_scrolled_and_transformed_cell_pixels() {
    let mut ui = TableDemo::new();
    ui.set_pinned(1);
    ui.set_records(vec!["A".into(), "B".into()]);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let mut input = Interaction::default();
    let base = ui.render_shared();
    assert!(ui.control_at(
        &base,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 440
        }
    ));
    let (scene, frame) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let geometry = frame.at_path(&path).unwrap().table.as_ref().unwrap();
    let seam = geometry.frozen_boundary().unwrap();
    let pixel = ((seam - 15.0) as u32, (geometry.body.y + 18.0) as u32);
    let initial = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        initial.pixel(pixel.0, pixel.1),
        Some([0x26, 0x37, 0x50, 0xff])
    );
    assert!(input.accessibility_scroll_to(
        &frame,
        &path,
        xrack_runtime::Orientation::Horizontal,
        60.0
    ));
    let (mut scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let scrolled = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        scrolled.pixel(pixel.0, pixel.1),
        initial.pixel(pixel.0, pixel.1)
    );
    let row =
        &mut std::rc::Rc::make_mut(&mut scene).children[0].children[2].children[0].children[0];
    row.children[1]
        .properties
        .insert("transform-rotation", 22.0_f64.into());
    let rotated = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert!(
        rotated
            .layout
            .at_path(&[0, 2, 0, 0, 1])
            .unwrap()
            .table_pane_clip()
            .is_some()
    );
    assert_eq!(
        rotated.pixel(pixel.0, pixel.1),
        initial.pixel(pixel.0, pixel.1)
    );
}

#[test]
fn table_row_numbers_paint_a_fixed_gutter_and_selection_state() {
    let mut ui = TableDemo::new();
    ui.set_row_header_size(xrack_runtime::Length(64.0));
    ui.set_records(vec!["A".into(), "B".into()]);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 2];
    let viewport = Size::new(560.0, 370.0);
    let mut input = Interaction::default();
    let base = ui.render_shared();
    assert!(ui.control_at(
        &base,
        &path,
        ControlAction::ResizeColumn {
            column: 0,
            width: 440
        }
    ));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let gutter = table.row_header;
    let row = &scene.at_path(&path).unwrap().children[0];
    assert_eq!(row.text("row-header-text"), Some("1"));
    let before = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert!(input.accessibility_scroll_to(
        &layout,
        &path,
        xrack_runtime::Orientation::Horizontal,
        140.0
    ));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let scrolled = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    for y in (gutter.y + 2.0) as u32..(gutter.y + 30.0) as u32 {
        for x in (gutter.x + 1.0) as u32..(gutter.x + gutter.width - 1.0) as u32 {
            assert_eq!(scrolled.pixel(x, y), before.pixel(x, y));
        }
    }
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::TableSelection(xrack_runtime::TableSelectionAction::Row {
            index: 0,
            operation: xrack_runtime::RowSelection::Replace
        },)
    ));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let selected = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    let x = (gutter.x + 42.0) as u32;
    let y = (gutter.y + 16.0) as u32;
    assert_ne!(selected.pixel(x, y), scrolled.pixel(x, y));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        selected
            .save_png(std::path::Path::new(&path).join("table-row-headers.png"))
            .unwrap();
    }
}

#[test]
fn row_reorder_insertion_line_spans_the_gutter_and_frozen_panes() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;
    let mut ui = StandardTableDemo::new();
    ui.set_row_header_size(xrack_runtime::Length(84.0));
    ui.set_move_rows(true);
    ui.set_pinned(1);
    ui.set_inventory(
        (0..4)
            .map(|units| InventoryRow {
                units,
                name: format!("Device {units}"),
                ..InventoryRow::default()
            })
            .collect(),
    );
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 1];
    let mut input = Interaction::default();
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(540.0, 380.0), &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let seam = table.frozen_boundary().unwrap();
    let start = Point {
        x: table.row_header.x + 20.0,
        y: item.children[0].bounds.y + 16.0,
    };
    let end = Point {
        y: item.children[2].bounds.y + 22.0,
        ..start
    };
    let baseline = renderer.snapshot(&scene, 540, 380, 1.0, &input).unwrap();
    let now = Instant::now();
    let hold = now + Duration::from_millis(500);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    assert!(input.advance_table_touch_hold(&scene, &layout, hold));
    let held = renderer.snapshot(&scene, 540, 380, 1.0, &input).unwrap();
    let source = ((start.x + 35.0) as u32, start.y as u32);
    assert_ne!(
        baseline.pixel(source.0, source.1),
        held.pixel(source.0, source.1)
    );
    assert!(input.move_scroll_gesture(&scene, &layout, end, hold));
    let marker = input
        .table_row_move_marker(&path, scene.at_path(&path).unwrap(), item)
        .unwrap();
    let preview = renderer.snapshot(&scene, 540, 380, 1.0, &input).unwrap();
    assert!(input.cancel_table_touch_drag());
    let cancelled = renderer.snapshot(&scene, 540, 380, 1.0, &input).unwrap();
    let y = (marker.y + 1.0) as u32;
    for x in [start.x, seam - 12.0, seam + 12.0] {
        assert_ne!(preview.pixel(x as u32, y), cancelled.pixel(x as u32, y));
    }
    assert_eq!(ui.get_moved_row_to(), -1);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        preview
            .save_png(std::path::Path::new(&path).join("table-row-move.png"))
            .unwrap();
    }
}

#[test]
fn moved_table_columns_repaint_their_cells_below_the_new_header_positions() {
    let mut ui = TableDemo::new();
    ui.set_records(vec!["A".into(), "B".into()]);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 2];
    let mut input = Interaction::default();
    let viewport = Size::new(560.0, 370.0);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    let table = before
        .layout
        .at_path(&path)
        .unwrap()
        .table
        .as_ref()
        .unwrap();
    let start = Point {
        x: table.columns[0].x + 40.0,
        y: table.header.y + 12.0,
    };
    let end = Point {
        x: table.columns[1].x + 120.0,
        y: start.y,
    };
    let pixel = ((table.body.x + 120.0) as u32, (table.body.y + 26.0) as u32);
    assert_eq!(
        before.pixel(pixel.0, pixel.1),
        Some([0x26, 0x37, 0x50, 0xff])
    );
    input.pointer_move(&scene, &before.layout, start);
    input.pointer_press_in_scene(&scene, &before.layout);
    input.pointer_move(&scene, &before.layout, end);
    assert!(input.table_column_move_drag(&before.layout, std::time::Instant::now()));
    let action = input
        .finish_table_column_move(&scene, &before.layout)
        .unwrap();
    assert!(ui.control_at(&scene, &action.path, action.action));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        after.pixel(pixel.0, pixel.1),
        Some([0x2b, 0x3c, 0x57, 0xff])
    );
    let table = after.layout.at_path(&path).unwrap().table.as_ref().unwrap();
    assert_eq!(table.visual_columns().collect::<Vec<_>>(), [1, 0]);
}

#[test]
fn standard_table_paints_projected_cells_and_ignores_offscreen_value_edits() {
    use std::rc::Rc;
    use xrack_runtime::{Property, Time};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..50_000)
            .map(|index| InventoryRow {
                name: format!("Device {index:05}"),
                units: index,
                available: true,
                refreshed: Time::default(),
            })
            .collect(),
    );
    let input = Interaction::default();
    let viewport = Size::new(620.0, 380.0);
    let prepared = Property::default();
    let prepare = |ui: &StandardTableDemo, renderer: &mut Renderer| {
        prepared.get(|| Rc::new(renderer.prepare_scene(ui, ui.render_shared(), viewport, &input)))
    };
    let first = prepare(&ui, &mut renderer);
    let model = ui.model_inventory();
    for (index, label) in [(49_999, "Offscreen"), (0, "Visible change")] {
        let mut row = model.row(index).unwrap();
        row.name = label.to_owned();
        assert!(model.set_row(index, row));
        assert_eq!(Rc::ptr_eq(&first, &prepare(&ui, &mut renderer)), index != 0);
    }
    let after = prepare(&ui, &mut renderer);
    let path = [0, 1, 0, 0, 0];
    assert_eq!(
        after.0.at_path(&path).unwrap().text("text"),
        Some("Visible change")
    );
    let before_pixels = renderer.snapshot(&first.0, 620, 380, 1.0, &input).unwrap();
    let after_pixels = renderer.snapshot(&after.0, 620, 380, 1.0, &input).unwrap();
    let cell = after.1.at_path(&path).unwrap().bounds;
    assert!((cell.y as u32..(cell.y + cell.height) as u32).any(|y| {
        (cell.x as u32..(cell.x + cell.width) as u32)
            .any(|x| before_pixels.pixel(x, y) != after_pixels.pixel(x, y))
    }));

    ui.set_filter_query(" OffSCREEN ".into());
    let filtered = prepare(&ui, &mut renderer);
    let table = filtered.0.find("inventory-table").unwrap();
    assert_eq!(table.list.unwrap().row_count, 1);
    assert_eq!(table.children.len(), 1);
    assert_eq!(table.children[0].row_key, model.row_key(49_999));
    assert_eq!(
        filtered.0.at_path(&path).unwrap().text("text"),
        Some("Offscreen")
    );
    let filtered_pixels = renderer
        .snapshot(&filtered.0, 620, 380, 1.0, &input)
        .unwrap();
    assert!((cell.y as u32..(cell.y + cell.height) as u32).any(|y| {
        (cell.x as u32..(cell.x + cell.width) as u32)
            .any(|x| after_pixels.pixel(x, y) != filtered_pixels.pixel(x, y))
    }));
}

#[test]
fn standard_table_shows_the_invalid_typed_draft_until_it_is_corrected() {
    use xrack_runtime::{Component, FocusRequests, ScrollRequests};
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardTableDemo::new();
    ui.set_edit_columns(true);
    ui.set_inventory(vec![InventoryRow {
        name: "交换机".into(),
        units: 12,
        available: true,
        ..InventoryRow::default()
    }]);
    let focus = FocusRequests::default();
    let scroll = ScrollRequests::default();
    let _focus = focus.enter();
    let _scroll = scroll.enter();
    let mut input = Interaction::default();
    let viewport = Size::new(620.0, 380.0);
    ui.invoke_edit_inventory_cell(0, 1);
    let (mut scene, mut layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    if input.apply_scroll_requests(&scene, &layout, scroll.drain()) {
        (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    }
    input.apply_focus_requests(&scene, &layout, focus.drain());
    let path = input.focused.as_ref().unwrap().path.clone();
    assert!(input.table_cell_editing(&path));
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("12x"),
        EditSource::Atomic,
        std::time::Instant::now()
    ));
    assert!(!input.commit_table_edit(&mut ui, &scene));
    assert_eq!(ui.model_inventory().row(0).unwrap().units, 12);
    assert!(input.table_cell_editing(&path));
    let frame = renderer.snapshot(&scene, 620, 380, 1.0, &input).unwrap();
    assert!(
        frame
            .rgba
            .as_chunks::<4>()
            .0
            .contains(&[0xa6, 0x38, 0x43, 0xff])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("standard-table-invalid-edit.png"))
            .unwrap();
    }
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::ReplaceAll("24"),
        EditSource::Atomic,
        std::time::Instant::now()
    ));
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(ui.model_inventory().row(0).unwrap().units, 24);
}

#[test]
fn shared_table_model_paints_independent_sorted_views() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableSortViewsDemo::new();
    ui.invoke_initialize();
    let input = Interaction::default();
    let viewport = Size::new(680.0, 370.0);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let frame = renderer.snapshot(&scene, 680, 370, 1.0, &input).unwrap();
    let quantity = scene.find("quantity-table").unwrap();
    let name = scene.find("name-table").unwrap();
    assert_eq!(quantity.children[0].row_key, name.children[1].row_key);
    assert_eq!(
        quantity.children[0].children[0].children[0].text("text"),
        Some("Beta")
    );
    assert_eq!(
        name.children[0].children[0].children[0].text("text"),
        Some("Alpha")
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-sorted-views.png"))
            .unwrap();
    }
}

#[test]
fn programmatic_sort_marks_the_same_header_after_column_reordering() {
    use xrack_runtime::Time;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        [10, 4, 8]
            .map(|units| InventoryRow {
                name: format!("Device {units}"),
                units,
                available: true,
                refreshed: Time::default(),
            })
            .to_vec(),
    );
    ui.set_sort_tools(true);
    ui.invoke_move_visible_column(1, 0);
    let model = ui.model_inventory();
    ui.on_sort_requested(move |column, descending| {
        assert_eq!(column, 1);
        assert!(!descending);
        model.sort_by(|a, b| a.units.cmp(&b.units));
    });
    let input = Interaction::default();
    let viewport = Size::new(620.0, 380.0);
    let path = [0, 1];
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 620, 380, 1.0, &input).unwrap();
    ui.invoke_sort_rows(1, false);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let frame = renderer.snapshot(&scene, 620, 380, 1.0, &input).unwrap();
    let header = frame
        .layout
        .at_path(&path)
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .columns[1];
    assert!(
        (header.y as u32..(header.y + header.height) as u32).any(|y| {
            (header.x as u32..(header.x + header.width) as u32)
                .any(|x| before.pixel(x, y) != frame.pixel(x, y))
        })
    );
    let table = scene.at_path(&path).unwrap();
    assert_eq!(table.table_column_order.visual_index(4, 1), Some(0));
    assert_eq!(
        table.children[0].children[0].children[1].text("text"),
        Some("4")
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-sort-state.png"))
            .unwrap();
    }
}

#[test]
fn table_header_stays_above_partially_scrolled_rows_on_gpu() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableDemo::new();
    ui.set_records((0..100).map(|row| format!("记录 {row:03}")).collect());
    let viewport = Size::new(560.0, 370.0);
    let path = [0, 2];
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let bounds = layout.at_path(&path).unwrap().bounds;
    let header_pixel = ((bounds.x + 250.0) as u32, (bounds.y + 16.0) as u32);
    let before = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        before.pixel(header_pixel.0, header_pixel.1),
        Some([0x31, 0x43, 0x63, 255])
    );
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + 30.0,
            y: bounds.y + 64.0,
        },
    );
    assert!(input.scroll_y(&layout, 145.0));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(table.list.unwrap().first_row, 4);
    let first_row = &layout.at_path(&path).unwrap().children[0];
    assert!(first_row.bounds.y < first_row.clip.y);
    let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        frame.pixel(header_pixel.0, header_pixel.1),
        before.pixel(header_pixel.0, header_pixel.1)
    );
    assert_ne!(frame.rgba, before.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-demo.png"))
            .unwrap();
    }

    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let grip = Point {
        x: table.columns[0].x + table.columns[0].width - 1.0,
        y: table.header.y + 10.0,
    };
    input.pointer_move(&scene, &layout, grip);
    input.pointer_press();
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: grip.x + 120.0,
            y: grip.y,
        },
    );
    let resize = input.table_resize_drag(&scene, &layout).unwrap();
    assert!(ui.control_at(&scene, &resize.path, resize.action));
    assert!(input.finish_table_resize());
    let (_, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert!(input.scroll_x(&layout, 64.0));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.pointer_leave();
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert_eq!(table.horizontal.offset, 64.0);
    assert_eq!(scene.at_path(&path).unwrap().list.unwrap().first_row, 4);
    let split = table.columns[1].x as u32;
    let row_y = (table.body.y + 6.0) as u32;
    let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(frame.layout, layout);
    assert_eq!(
        frame.pixel(split - 1, header_pixel.1),
        Some([0x63, 0x7c, 0xa4, 255])
    );
    assert_eq!(frame.pixel(split - 2, row_y), Some([0x26, 0x37, 0x50, 255]));
    assert_eq!(frame.pixel(split + 2, row_y), Some([0x2b, 0x3c, 0x57, 255]));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-resized.png"))
            .unwrap();
    }

    let bars: Vec<_> = item.scroll_bars(scene.at_path(&path).unwrap()).collect();
    let [horizontal, vertical] = bars.as_slice() else {
        panic!("expected both table scroll bars")
    };
    assert_eq!(horizontal.orientation, Orientation::Horizontal);
    assert_eq!(vertical.orientation, Orientation::Vertical);
    assert!(vertical.track.y >= table.body.y);
    assert_eq!(horizontal.track.intersect(vertical.track).width, 0.0);
    let grab = |bar: &xrack_layout::ScrollBarGeometry| Point {
        x: bar.thumb.x + bar.thumb.width * 0.5,
        y: bar.thumb.y + bar.thumb.height * 0.5,
    };
    input.pointer_move(&scene, &layout, grab(horizontal));
    assert_eq!(
        input.scroll_bar_press(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::Unchanged)
    );
    input.pointer_move(&scene, &layout, Point { x: 800.0, y: -40.0 });
    assert_eq!(
        input.scroll_bar_drag(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::ViewportMoved)
    );
    assert!(input.finish_scroll_bar());
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert_eq!(table.horizontal.offset, table.horizontal.max_offset);
    assert_eq!(item.scroll.unwrap().offset, 145.0);
    for (cell, column) in item.children[0].children[0]
        .children
        .iter()
        .zip(&table.columns)
    {
        assert_eq!(cell.bounds.x, column.x);
    }
    let vertical = item
        .scroll_bars(scene.at_path(&path).unwrap())
        .find(|bar| bar.orientation == Orientation::Vertical)
        .unwrap();
    input.pointer_move(&scene, &layout, grab(&vertical));
    assert_eq!(
        input.scroll_bar_press(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::Unchanged)
    );
    input.pointer_move(&scene, &layout, Point { x: 20.0, y: 560.0 });
    assert_eq!(
        input.scroll_bar_drag(&scene, &layout),
        Some(xrack_layout::ScrollBarUpdate::ViewportMoved)
    );
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let item = layout.at_path(&path).unwrap();
    let scroll = item.scroll.unwrap();
    assert_eq!(scroll.offset, scroll.max_offset);
    let table = scene.at_path(&path).unwrap();
    assert_eq!(table.list.unwrap().first_row + table.children.len(), 100);
    assert!(table.children.len() < 12);
    let vertical = item
        .scroll_bars(table)
        .find(|bar| bar.orientation == Orientation::Vertical)
        .unwrap();
    let sample = grab(&vertical);
    let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_eq!(
        frame.pixel(sample.x as u32, sample.y as u32),
        Some([0xad, 0xc2, 0xff, 255])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-scroll-bars.png"))
            .unwrap();
    }
    assert!(input.finish_scroll_bar());
}

#[test]
fn table_cell_draft_draws_before_the_model_changes() {
    use std::time::Instant;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableDemo::new();
    ui.set_records(vec!["待编辑的名称".into(), "另一条记录".into()]);
    let mut input = Interaction::default();
    let (scene, layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), Size::new(560.0, 370.0), &input);
    let path = [0, 2, 0, 0, 0];
    let cell = layout.at_path(&path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 40.0,
            y: cell.y + 16.0,
        },
    );
    input.pointer_press();
    assert!(input.pointer_release_target().is_some());
    assert!(input.begin_table_edit(&scene, &layout));
    let before = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("新的名称"),
        EditSource::Atomic,
        Instant::now()
    ));
    assert_eq!(ui.model_records().row(0).as_deref(), Some("待编辑的名称"));
    let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    assert_ne!(frame.rgba, before.rgba);
    assert_eq!(
        input.editor_value(scene.at_path(&path).unwrap()),
        "新的名称"
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-edit.png"))
            .unwrap();
    }
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(ui.model_records().row(0).as_deref(), Some("新的名称"));
}

#[test]
fn record_table_renders_field_updates_and_draft_independently() {
    use xrack_runtime::FocusRequests;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RecordTableDemo::new();
    ui.set_editing_tools(true);
    let model = ui.model_records();
    let viewport = Size::new(650.0, 330.0);
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let owner = [0, 1, 0, 0, 1];
    let requests = FocusRequests::default();
    let _focus = requests.enter();
    ui.invoke_start_edit(0, 1);
    input.apply_focus_requests(&scene, &layout, requests.drain());
    assert!(input.table_cell_editing(&owner));
    let before = renderer.snapshot(&scene, 650, 330, 1.0, &input).unwrap();
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("基础设施组"),
        EditSource::Atomic,
        std::time::Instant::now()
    ));
    let mut row = model.row(0).unwrap();
    row.units = 17;
    assert!(model.set_row(0, row));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&scene, &layout);
    assert!(input.table_cell_editing(&owner));
    assert_eq!(
        scene.at_path(&[0, 1, 0, 0, 2]).unwrap().int("value"),
        Some(17)
    );
    assert_eq!(model.row(0).unwrap().owner, "网络组");
    let frame = renderer.snapshot(&scene, 650, 330, 1.0, &input).unwrap();
    assert_ne!(frame.rgba, before.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-record-edit.png"))
            .unwrap();
    }
    assert!(input.commit_table_edit(&mut ui, &scene));
    assert_eq!(model.row(0).unwrap().owner, "基础设施组");
    assert_eq!(model.row(0).unwrap().units, 17);
}

#[test]
fn record_table_renders_numeric_toggle_and_range_edits() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RecordTableDemo::new();
    let viewport = Size::new(760.0, 330.0);
    let input = Interaction::default();
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before = renderer.snapshot(&scene, 760, 330, 1.0, &input).unwrap();
    let units = [0, 1, 0, 0, 2];
    let available = [0, 1, 0, 0, 3];
    let load = [0, 1, 0, 0, 4];
    assert!(ui.control_at(&scene, &units, ControlAction::Spin(SpinAction::Commit(26))));
    assert!(ui.dispatch_at(&scene, &available, "clicked"));
    assert!(ui.adjust_range_at(&scene, &load, RangeAction::SetFraction(0.75)));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_eq!(scene.at_path(&units).unwrap().int("value"), Some(26));
    assert!(!scene.at_path(&available).unwrap().boolean("checked", true));
    assert_eq!(scene.at_path(&load).unwrap().number("value"), Some(0.75));
    let frame = renderer.snapshot(&scene, 760, 330, 1.0, &input).unwrap();
    for path in [&units[..], &available[..], &load[..]] {
        let bounds = layout.at_path(path).unwrap().bounds;
        assert!(
            (bounds.x as u32..(bounds.x + bounds.width) as u32).any(|x| {
                (bounds.y as u32..(bounds.y + bounds.height) as u32)
                    .any(|y| frame.pixel(x, y) != before.pixel(x, y))
            })
        );
    }
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-record-editors.png"))
            .unwrap();
    }
}

#[test]
fn table_multiselection_paints_disjoint_rows() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableDemo::new();
    ui.set_records(
        ["草稿", "进行中", "已审核", "已归档"]
            .map(str::to_owned)
            .to_vec(),
    );
    let viewport = Size::new(560.0, 370.0);
    let path = [0, 2];
    let mut input = Interaction::default();
    for (row, operation) in [(0, RowSelection::Replace), (2, RowSelection::Toggle)] {
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
        let bounds = layout.at_path(&path).unwrap().children[row].bounds;
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bounds.x + 30.0,
                y: bounds.y + 15.0,
            },
        );
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        let action = input
            .select_group_option_with_selection(&scene, &layout, &target, operation)
            .unwrap();
        assert!(ui.control_at(&scene, &action.path, action.action));
    }
    input.pointer_leave();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    let table = layout.at_path(&path).unwrap();
    let accent = Some([0x9a, 0xad, 0xff, 255]);
    for (row, selected) in [(0, true), (1, false), (2, true), (3, false)] {
        let bounds = table.children[row].bounds;
        let pixel = frame.pixel((bounds.x + 1.0) as u32, (bounds.y + 16.0) as u32);
        assert_eq!(pixel == accent, selected, "row {row}");
    }
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-multiselect.png"))
            .unwrap();
    }
}

#[test]
fn table_grid_selection_paints_only_the_selected_cells_and_headers() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableDemo::new();
    ui.set_records(["草稿", "已审核", "已归档"].map(str::to_owned).to_vec());
    let viewport = Size::new(560.0, 370.0);
    let path = [0, 2];
    let input = Interaction::default();
    let accent = Some([0x9a, 0xad, 0xff, 255]);
    for (mode, action, filename) in [
        (
            SelectionTarget::Cells,
            TableSelectionAction::Cell {
                row: 1,
                column: 1,
                operation: RowSelection::Replace,
            },
            "table-cell-selection.png",
        ),
        (
            SelectionTarget::Columns,
            TableSelectionAction::Column {
                column: 1,
                operation: RowSelection::Replace,
            },
            "table-column-selection.png",
        ),
    ] {
        ui.set_target(mode);
        let scene = ui.render_shared();
        assert!(ui.control_at(&scene, &path, ControlAction::TableSelection(action)));
        let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
        let frame = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
        let item = layout.at_path(&path).unwrap();
        let table = item.table.as_ref().unwrap();
        let x = (table.columns[1].x + 1.0) as u32;
        for (row, item) in item.children.iter().enumerate() {
            let y = (item.bounds.y + 16.0) as u32;
            assert_ne!(frame.pixel((table.columns[0].x + 1.0) as u32, y), accent);
            assert_eq!(
                frame.pixel(x, y) == accent,
                mode == SelectionTarget::Columns || row == 1
            );
        }
        assert_eq!(
            frame.pixel(x + 105, (table.header.y + 10.0) as u32),
            Some(if mode == SelectionTarget::Columns {
                [0x45, 0x5a, 0x89, 255]
            } else {
                [0x31, 0x43, 0x63, 255]
            })
        );
        if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
            frame
                .save_png(std::path::Path::new(&path).join(filename))
                .unwrap();
        }
    }
}

#[test]
fn table_drag_selection_previews_only_the_clipped_table_body() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TableDemo::new();
    ui.set_target(SelectionTarget::Cells);
    ui.set_records(["first", "second", "third"].map(str::to_owned).to_vec());
    let mut input = Interaction::default();
    let viewport = Size::new(560.0, 370.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let path = [0, 2];
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let x = table.columns[0].x + 75.0;
    let start = Point {
        x,
        y: item.children[0].bounds.y + 16.0,
    };
    let end = Point {
        x,
        y: item.children[1].bounds.y + 16.0,
    };
    let header = (x as u32, (table.header.y + 5.0) as u32);
    let outside = (x as u32, (item.children[2].bounds.y + 12.0) as u32);
    for device in ["mouse", "touch"] {
        let now = Instant::now();
        if device == "touch" {
            assert!(input.begin_scroll_gesture(
                &scene,
                &layout,
                start,
                now,
                ScrollGestureKind::Touch
            ));
            let hold = now + Duration::from_millis(500);
            assert!(input.advance_table_touch_hold(&scene, &layout, hold));
            assert!(input.move_scroll_gesture(&scene, &layout, end, hold));
        } else {
            input.pointer_move(&scene, &layout, start);
            input.pointer_press_in_scene(&scene, &layout);
            assert!(input.arm_table_drag_selection(&scene, &layout, false));
            input.pointer_move(&scene, &layout, end);
            assert!(input.move_table_drag_selection(&layout, now));
        }
        let preview = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
        assert!(input.cancel_table_drag_selection());
        let cancelled = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
        assert_ne!(
            preview.pixel(x as u32, start.y as u32),
            cancelled.pixel(x as u32, start.y as u32),
            "{device}"
        );
        assert_eq!(
            preview.pixel(header.0, header.1),
            cancelled.pixel(header.0, header.1),
            "{device}"
        );
        assert_eq!(
            preview.pixel(outside.0, outside.1),
            cancelled.pixel(outside.0, outside.1),
            "{device}"
        );
        if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
            preview
                .save_png(
                    std::path::Path::new(&path).join(format!("table-drag-selection-{device}.png")),
                )
                .unwrap();
        }
    }

    assert!(ui.control_at(
        &scene,
        &path,
        xrack_runtime::ControlAction::TableSelection(
            xrack_runtime::TableSelectionAction::DragRange {
                anchor: (0, 0),
                end: (1, 0),
                additive: false,
            }
        ),
    ));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let with_handles = renderer.snapshot(&scene, 560, 370, 1.0, &input).unwrap();
    let without_handles = renderer
        .snapshot(&scene, 560, 370, 1.0, &Interaction::default())
        .unwrap();
    let handles = input
        .table_selection_handles(
            &path,
            scene.at_path(&path).unwrap(),
            layout.at_path(&path).unwrap(),
        )
        .collect::<Vec<_>>();
    assert_eq!(handles.len(), 2);
    for rect in handles {
        let x = (rect.x + rect.width * 0.5) as u32;
        let y = (rect.y + rect.height * 0.5) as u32;
        assert_ne!(with_handles.pixel(x, y), without_handles.pixel(x, y));
    }
}

#[test]
fn authored_selection_handles_paint_their_background_and_children() {
    use table_ui::SelectionHandlesDemo;
    use xrack_runtime::{ControlAction, TableSelectionAction};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SelectionHandlesDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(632.0, 310.0);
    let path = [1];
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let focus = input.accessibility_target(&scene, &layout, &path).unwrap();
    input.focus_target(&scene, &layout, focus);
    assert!(ui.control_at(
        &scene,
        &path,
        ControlAction::TableSelection(TableSelectionAction::DragRange {
            anchor: (0, 0),
            end: (1, 1),
            additive: false,
        }),
    ));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let shot = renderer.snapshot(&scene, 632, 310, 1.0, &input).unwrap();
    let handles = input
        .table_selection_handle_parts(
            &path,
            scene.at_path(&path).unwrap(),
            layout.at_path(&path).unwrap(),
        )
        .collect::<Vec<_>>();
    assert_eq!(handles.len(), 2);
    for (index, bounds) in handles {
        let x = (bounds.x + bounds.width * 0.5) as u32;
        let y = (bounds.y + bounds.height * 0.5) as u32;
        let colors = [[241, 154, 55, 255], [61, 227, 200, 255]];
        let actual = shot.pixel(x, y).unwrap();
        assert!(
            actual
                .into_iter()
                .zip(colors[index])
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{actual:?}"
        );
    }
}

#[test]
fn stack_layout_paints_only_the_selected_item() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut window = xrack_runtime::Node::new("Window", None);
    let mut layers = xrack_runtime::Node::new("StackLayout", None);
    for packed in [0xc8482aff, 0x3577d0ff] {
        let mut child = xrack_runtime::Node::new("Rectangle", None);
        child
            .properties
            .insert("background", xrack_runtime::Color(packed).into());
        layers.children.push(child);
    }
    window.children.push(layers);
    for (index, expected) in [[200, 72, 42, 255], [53, 119, 208, 255]]
        .into_iter()
        .enumerate()
    {
        window.children[0]
            .properties
            .insert("current-index", (index as i64).into());
        let shot = renderer
            .snapshot(&window, 114, 76, 1.0, &Interaction::default())
            .unwrap();
        let pixel = shot.pixel(55, 35).unwrap();
        assert!(
            pixel
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{pixel:?}"
        );
    }
}

#[test]
fn counter_click_updates_both_bindings_and_the_frame() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        Counter::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(320.0, 180.0),
    );
    assert_eq!(ui.node("label").text("text"), Some("计数：0"));
    let before = renderer
        .snapshot(ui.scene(), 320, 180, 1.0, ui.input())
        .unwrap();
    ui.click("add");
    ui.click("add");
    assert_eq!(ui.node("label").text("text"), Some("计数：2"));
    assert_eq!(ui.node("status").text("text"), Some("加倍：4"));
    assert_eq!(ui.focused(), Some("add"));
    let after = renderer
        .snapshot(ui.scene(), 320, 180, 1.0, ui.input())
        .unwrap();
    assert_ne!(before.rgba, after.rgba);
}

#[test]
fn modal_dialogs_capture_input_and_restore_focus_in_order() {
    fn press(
        input: &mut Interaction,
        scene: &Node,
        layout: &xrack_layout::LayoutNode,
        path: &[usize],
    ) {
        let item = layout.at_path(path).unwrap();
        let rect = item.bounds.intersect(item.clip);
        input.pointer_move(
            scene,
            layout,
            Point {
                x: rect.x + rect.width * 0.5,
                y: rect.y + rect.height * 0.5,
            },
        );
        input.pointer_press();
        assert_eq!(input.pointer_release_target().unwrap().path, path);
    }
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = DialogDemo::new();
    let mut input = Interaction::default();
    let viewport = Size::new(640.0, 390.0);
    let opener = [0, 1, 0];
    let dialog = [0, 2, 1];
    let field = [0, 2, 1, 1];
    let help_button = [0, 2, 1, 2];
    let help = [0, 2, 1, 4];
    let accept = [0, 2, 1, 3, 1];
    let closed = ui.render_shared();
    assert!(closed.at_path(&dialog).unwrap().children.is_empty());
    let closed_layout = renderer.layout_with_input(&closed, viewport, &input);
    assert!(closed_layout.dialog_paths().is_empty());
    let before = renderer.snapshot(&closed, 640, 390, 1.0, &input).unwrap();
    press(&mut input, &closed, &closed_layout, &opener);
    assert!(ui.dispatch_at(&closed, &opener, "clicked"));

    let opened = ui.render_shared();
    let layout = renderer.layout_with_input(&opened, viewport, &input);
    input.reconcile(&opened, &layout);
    assert_eq!(layout.dialog_paths(), [dialog.to_vec()]);
    assert_eq!(input.focused.as_ref().unwrap().path, field);
    assert_eq!(layout.at_path(&dialog).unwrap().clip, layout.bounds);
    assert_eq!(
        layout.at_path(&[0, 2]).unwrap().bounds,
        closed_layout.at_path(&[0, 2]).unwrap().bounds
    );
    let frame = renderer.snapshot(&opened, 640, 390, 1.0, &input).unwrap();
    assert_ne!(&frame.rgba[..4], &before.rgba[..4]);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("dialog-demo.png"))
            .unwrap();
    }

    let backdrop = Point { x: 22.0, y: 63.0 };
    input.pointer_move(&opened, &layout, backdrop);
    assert!(input.hovered.is_none());
    assert!(input.dialog_pointer_press(&layout));
    assert_eq!(input.focused.as_ref().unwrap().path, field);
    let body = layout.at_path(&field).unwrap().bounds;
    input.pointer_move(
        &opened,
        &layout,
        Point {
            x: body.x + 8.0,
            y: body.y + 8.0,
        },
    );
    assert!(input.dialog_pointer_release(&opened, &layout).is_none());
    assert!(input.pointer_release_target().is_none());
    for _ in 0..6 {
        input.focus_next(&opened, &layout, false);
        assert!(input.focused.as_ref().unwrap().path.starts_with(&dialog));
    }

    press(&mut input, &opened, &layout, &help_button);
    assert!(ui.dispatch_at(&opened, &help_button, "clicked"));
    let stacked = ui.render_shared();
    let layout = renderer.layout_with_input(&stacked, viewport, &input);
    input.reconcile(&stacked, &layout);
    assert_eq!(input.dialog_layers(&layout).last(), Some(help.as_slice()));
    assert!(input.focused.as_ref().unwrap().path.starts_with(&help));
    let escape = input.dialog_escape(&stacked, &layout).unwrap();
    assert_eq!(escape.path, help);
    assert!(ui.control_at(&stacked, &escape.path, escape.action));
    let opened = ui.render_shared();
    let layout = renderer.layout_with_input(&opened, viewport, &input);
    input.reconcile(&opened, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, help_button);

    assert!(ui.edit_at(&opened, &field, "已重命名".into()));
    let edited = ui.render_shared();
    let layout = renderer.layout_with_input(&edited, viewport, &input);
    input.reconcile(&edited, &layout);
    press(&mut input, &edited, &layout, &accept);
    let action = input
        .dialog_button(&edited, &layout, input.focused.as_ref().unwrap())
        .unwrap();
    assert_eq!(action.action, ControlAction::Dialog(DialogRole::Accept));
    assert!(ui.control_at(&edited, &action.path, action.action));
    assert!(!ui.control_at(&edited, &action.path, action.action));
    assert_eq!(ui.get_saved(), "已重命名");
    assert_eq!(ui.get_accepted_count(), 1);
    let closed = ui.render_shared();
    let layout = renderer.layout_with_input(&closed, viewport, &input);
    input.reconcile(&closed, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, opener);

    ui.set_rename_open(true);
    ui.set_outside_closes(false);
    for enabled in [false, true] {
        ui.set_outside_closes(enabled);
        let opened = ui.render_shared();
        let layout = renderer.layout_with_input(&opened, viewport, &input);
        input.reconcile(&opened, &layout);
        input.pointer_move(&opened, &layout, backdrop);
        assert!(input.dialog_pointer_press(&layout));
        let action = input.dialog_pointer_release(&opened, &layout);
        assert_eq!(action.is_some(), enabled);
        if let Some(action) = action {
            assert!(ui.control_at(&opened, &action.path, action.action));
        }
    }
    assert_eq!(ui.get_rejected_count(), 1);
    assert_eq!(ui.get_background_clicks(), 0);
}

#[test]
fn drawer_swipe_opens_the_panel_and_shades_the_window() {
    use std::time::{Duration, Instant};
    use xrack_layout::{Rect, ScrollGestureKind};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = DialogDemo::new();
    let viewport = Size::new(640.0, 390.0);
    let mut input = Interaction::default();
    let path = [0, 4];
    let scene = ui.render_shared();
    assert!(scene.at_path(&path).unwrap().children.is_empty());
    let before = renderer.snapshot(&scene, 640, 390, 1.0, &input).unwrap();
    let now = Instant::now();
    let from = Point { x: 4.0, y: 120.0 };
    let short = Point { x: 44.0, y: 120.0 };
    assert!(input.begin_scroll_gesture(
        &scene,
        &before.layout,
        from,
        now,
        ScrollGestureKind::Touch
    ));
    assert!(input.move_scroll_gesture(
        &scene,
        &before.layout,
        short,
        now + Duration::from_millis(30)
    ));
    let opening = renderer.snapshot(&scene, 640, 390, 1.0, &input).unwrap();
    assert!(opening.layout.dialog_paths().is_empty());
    assert_eq!(ui.get_drawer_opens(), 0);
    assert_ne!(opening.pixel(8, 376), before.pixel(8, 376));
    assert_ne!(opening.pixel(630, 376), before.pixel(630, 376));
    let cancelled = input.end_scroll_gesture(
        &scene,
        &opening.layout,
        short,
        now + Duration::from_millis(50),
    );
    assert!(cancelled.scrolled && !cancelled.tap && cancelled.control.is_none());
    let restored = renderer.snapshot(&scene, 640, 390, 1.0, &input).unwrap();
    assert_same_pixels(&restored, &before);
    assert!(input.begin_scroll_gesture(
        &scene,
        &before.layout,
        from,
        now,
        ScrollGestureKind::MousePan
    ));
    let action = input
        .end_scroll_gesture(
            &scene,
            &before.layout,
            Point { x: 115.0, y: 120.0 },
            now + Duration::from_millis(50),
        )
        .control
        .unwrap();
    assert_eq!(action.path, path);
    assert!(ui.control_at(&scene, &action.path, action.action));
    assert_eq!(ui.get_drawer_opens(), 1);
    let opened = ui.render_shared();
    let layout = renderer.layout_with_input(&opened, viewport, &input);
    input.reconcile(&opened, &layout);
    assert_eq!(
        layout.at_path(&path).unwrap().bounds,
        Rect::new(0.0, 0.0, 320.0, 390.0)
    );
    assert_eq!(input.focused.as_ref().unwrap().id, Some("drawer-rename"));
    let frame = renderer.snapshot(&opened, 640, 390, 1.0, &input).unwrap();
    assert_eq!(frame.pixel(8, 376), Some([0x27, 0x37, 0x51, 255]));
    assert_eq!(opening.pixel(8, 376), frame.pixel(8, 376));
    assert_ne!(frame.pixel(630, 376), before.pixel(630, 376));
    assert_ne!(frame.pixel(630, 376), opening.pixel(630, 376));
    let from = Point { x: 314.0, y: 120.0 };
    let short = Point { x: 284.0, y: 120.0 };
    assert!(input.begin_scroll_gesture(&opened, &layout, from, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&opened, &layout, short, now + Duration::from_millis(30)));
    let closing = renderer.snapshot(&opened, 640, 390, 1.0, &input).unwrap();
    assert_eq!(closing.layout.at_path(&path).unwrap().bounds.x, -30.0);
    assert_ne!(closing.pixel(306, 376), frame.pixel(306, 376));
    assert_eq!(ui.get_drawer_closes(), 0);
    input.cancel_scroll_gesture();
    let restored = renderer.snapshot(&opened, 640, 390, 1.0, &input).unwrap();
    assert_same_pixels(&restored, &frame);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("drawer.png"))
            .unwrap();
    }
    input.pointer_move(&opened, &layout, Point { x: 605.0, y: 340.0 });
    assert!(input.dialog_pointer_press(&layout));
    let close = input.dialog_pointer_release(&opened, &layout).unwrap();
    assert_eq!(close.action, ControlAction::Drawer(false));
    assert!(ui.control_at(&opened, &close.path, close.action));
    assert_eq!(ui.get_drawer_closes(), 1);
    assert!(
        ui.render_shared()
            .at_path(&path)
            .unwrap()
            .children
            .is_empty()
    );
}

#[test]
fn bidi_editor_uses_shaped_glyph_edges_for_hit_testing_selection_and_vertical_movement() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = BidiEditorDemo::new();
    let value = "AאבB";
    let document = format!("{value}\n{value}\nمرحبا 123");
    ui.set_line(value.into());
    ui.set_document(document.clone());
    let scene = ui.render_shared();
    let path = [0, 1];
    let node = scene.at_path(&path).unwrap();
    let mut input = Interaction::default();
    let layout = renderer.layout_with_input(&scene, Size::new(520.0, 300.0), &input);
    let bounds = layout.at_path(&path).unwrap().bounds;
    let visual = renderer.text.single_line_layout(node, value);
    let first_rtl = &visual.graphemes()[1];
    let second_rtl = &visual.graphemes()[2];
    assert!(first_rtl.leading > first_rtl.trailing);
    assert!(first_rtl.span().start >= second_rtl.span().end - 0.01);
    let spans = visual.selection_spans(0..3);
    assert_eq!(spans.len(), 2);
    let padding = bounds.x + 6.0;
    let left_side = TextPosition {
        offset: 1,
        trailing: true,
    };
    let right_side = TextPosition {
        offset: 1,
        trailing: false,
    };
    let mut edit = TextEdit::at_end(value);
    edit.place_caret(value, left_side, false);
    let left = renderer
        .text
        .single_line_caret_x(node, bounds, value, Some(&edit));
    edit.place_caret(value, right_side, false);
    let right = renderer
        .text
        .single_line_caret_x(node, bounds, value, Some(&edit));
    assert!(right - left > 5.0);
    let x = first_rtl.leading * 0.9 + first_rtl.trailing * 0.1;
    let point = Point {
        x: padding + x,
        y: bounds.y + bounds.height * 0.5,
    };
    assert_eq!(
        renderer.text.single_line_hit(node, bounds, point, None),
        right_side
    );
    assert_eq!(visual.step(right_side, false).unwrap().offset, 3);

    input.pointer_move(&scene, &layout, point);
    input.pointer_press_in_scene(&scene, &layout);
    input.sync_editor(&scene);
    let before = renderer.snapshot(&scene, 520, 300, 1.0, &input).unwrap();
    input.editor.as_mut().unwrap().set_selection(
        value,
        0,
        TextPosition {
            offset: 3,
            trailing: true,
        },
    );
    let selected = renderer.snapshot(&scene, 520, 300, 1.0, &input).unwrap();
    let y = (bounds.y + ((bounds.height - font_size(node) * 1.4) * 0.5).max(0.0)) as u32;
    let changed = |x: f32| {
        let x = (padding + x) as u32;
        (y + 2..y + 8).any(|row| before.pixel(x, row) != selected.pixel(x, row))
    };
    for span in &spans {
        assert!(changed(span.start.midpoint(span.end)));
    }
    assert!(!changed(spans[0].end.midpoint(spans[1].start)));

    edit.place_caret(
        value,
        TextPosition {
            offset: 5,
            trailing: false,
        },
        false,
    );
    edit.set_preedit("ש".into(), Some((2, 2)));
    let composed = renderer.text.single_line_layout(node, "AאבשB");
    let ime_cursor = TextPosition {
        offset: 7,
        trailing: true,
    };
    let x = renderer
        .text
        .single_line_caret_x(node, bounds, value, Some(&edit));
    assert!((x - (padding + composed.caret_x(ime_cursor))).abs() < 0.01);

    let paragraph_path = [0, 3];
    let paragraph = scene.at_path(&paragraph_path).unwrap();
    let paragraph_bounds = layout.at_path(&paragraph_path).unwrap().bounds;
    let area = renderer
        .text
        .text_area_layout(paragraph, paragraph_bounds, &document);
    let advance = |text: &str| renderer.text.advance_node(paragraph, text);
    edit.place_caret(&document, right_side, false);
    let original = area.caret_point(&document, edit.position(&document), advance);
    area.move_caret(&document, &mut edit, TextAreaMotion::Down, false, advance);
    let next = area.caret_point(&document, edit.position(&document), advance);
    assert!((next.x - original.x).abs() < 0.01);
    assert_eq!(next.y, area.line_height);
    let rtl = area.visual_line(2).unwrap();
    assert!(rtl.is_rtl());
    assert!(rtl.caret_x(TextPosition::default()) > 0.0);
}

#[test]
fn multiline_editor_draws_selection_and_preedit_then_commits_text() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = NotesDemo::new();
    let note = format!("{}\n{}", ui.get_note(), "文档还有下一行。\n".repeat(18));
    ui.set_note(note.clone());
    let scene = ui.render_shared();
    let path = [0, 2];
    let node = scene.at_path(&path).unwrap();
    let viewport = Size::new(590.0, 376.0);
    let mut input = Interaction::default();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    let bounds = layout.at_path(&path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + 20.0,
            y: bounds.y + 14.0,
        },
    );
    input.pointer_press();
    input.sync_editor(&scene);
    let target = input.focused.clone().unwrap();
    assert_eq!(target.path, path);
    let area = renderer.text.text_area_layout(node, bounds, &note);
    let cached = renderer.text.text_area_layout(node, bounds, &note);
    assert!(std::sync::Arc::ptr_eq(&area.lines, &cached.lines));
    let advance = |text: &str| renderer.text.advance(text, font_size(node));
    assert!(area.max_scroll.y > 0.0);
    let hit = area.hit(
        &note,
        Point {
            x: area.area.x,
            y: area.area.y,
        },
        Point::default(),
        advance,
    );
    assert_eq!(hit.offset, 0);
    let editor = input.editor.as_mut().unwrap();
    editor.place_caret(&note, hit, false);
    area.move_caret(&note, editor, TextAreaMotion::Down, true, advance);
    area.move_caret(&note, editor, TextAreaMotion::Down, true, advance);
    assert!(editor.selection(&note).end > "项目笔记\n".len());
    let selected = renderer.snapshot(&scene, 590, 376, 1.0, &input).unwrap();
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        selected
            .save_png(std::path::Path::new(&path).join("notes-demo.png"))
            .unwrap();
    }

    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("中文预编辑".into(), Some((6, 6)));
    let (composition_layout, caret) =
        renderer
            .text
            .text_area_caret(node, bounds, input.editor.as_ref());
    assert_eq!(caret.unwrap().y, 0.0);
    assert!(composition_layout.max_scroll.y > 0.0);
    let composing = renderer.snapshot(&scene, 590, 376, 1.0, &input).unwrap();
    assert_ne!(selected.rgba, composing.rgba);
    assert_eq!(ui.get_note(), note);
    assert!(input.edit_text(
        &mut ui,
        &scene,
        EditAction::Insert("已提交\r\n新一行"),
        EditSource::Atomic,
        std::time::Instant::now()
    ));
    let committed = ui.get_note();
    assert!(ui.get_note().starts_with("已提交\n新一行"));
    assert_eq!(ui.get_last_edit(), committed);
    assert_eq!(ui.get_edits(), 1);

    let edited = ui.render_shared();
    let edited_node = edited.at_path(&path).unwrap();
    let editor = input.editor.as_mut().unwrap();
    editor.place_caret(
        &committed,
        TextPosition {
            offset: committed.len(),
            trailing: false,
        },
        false,
    );
    let (editor_layout, caret) =
        renderer
            .text
            .text_area_caret(edited_node, bounds, input.editor.as_ref());
    let scroll = editor_layout.keep_visible(Point::default(), caret.unwrap());
    assert_eq!(scroll.y, editor_layout.max_scroll.y);
    input.set_text_scroll(&target, scroll);
    let scrolled = renderer.snapshot(&edited, 590, 376, 1.0, &input).unwrap();
    assert_ne!(selected.rgba, scrolled.rgba);
    assert!(input.replay_text_edit(&mut ui, &edited, false));
    assert_eq!(ui.get_note(), note);
    assert_eq!(ui.get_last_edit(), note);
    let undone = ui.render_shared();
    assert!(input.replay_text_edit(&mut ui, &undone, true));
    assert_eq!(ui.get_note(), committed);

    ui.set_wrap_mode(TextWrap::NoWrap);
    ui.set_locked(true);
    let locked = ui.render_shared();
    assert_eq!(locked.at_path(&path).unwrap().text_wrap(), TextWrap::NoWrap);
    assert!(!ui.edit_at(&locked, &path, "拒绝编辑".into()));
    assert!(!input.replay_text_edit(&mut ui, &locked, false));
    assert_eq!(ui.get_edits(), 3);
}

#[test]
fn editable_list_delegates_keep_state_by_model_row_identity() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = DelegatesDemo::new();
    ui.set_records((0..1_000).map(|i| format!("记录 {i}")).collect());
    let model = ui.model_records();
    let key = model.row_key(0);
    let viewport = Size::new(630.0, 396.0);
    let list = [0, 2];
    let field = [0, 2, 0, 0, 1];
    let mut input = Interaction::default();
    let (initial, initial_layout) =
        renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_eq!(initial.row_key_at(&field), key);
    let bounds = initial_layout.at_path(&field).unwrap().bounds;
    input.pointer_move(
        &initial,
        &initial_layout,
        Point {
            x: bounds.x + 10.0,
            y: bounds.y + 8.0,
        },
    );
    input.pointer_press();
    input.sync_editor(&initial);
    let now = std::time::Instant::now();
    input.edit_text(
        &mut ui,
        &initial,
        EditAction::SelectAll,
        EditSource::Keyboard,
        now,
    );
    assert!(input.edit_text(
        &mut ui,
        &initial,
        EditAction::Insert("用户已编辑"),
        EditSource::Atomic,
        now
    ));
    assert!(ui.dispatch_at(&initial, &[0, 2, 0, 0, 2], "clicked"));
    assert!(ui.dispatch_at(&initial, &[0, 2, 0, 0, 3], "clicked"));
    let (edited, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let bounds = layout.at_path(&field).unwrap().bounds;
    input.pointer_move(
        &edited,
        &layout,
        Point {
            x: bounds.x + 10.0,
            y: bounds.y + 8.0,
        },
    );
    input.pointer_press();
    assert_eq!(input.focused.as_ref().unwrap().row_key, key);

    assert!(model.insert(0, "新记录".into()));
    assert!(!ui.edit_at(&edited, &field, "过期事件".into()));
    let (shifted, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&shifted, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, [0, 2, 1, 0, 1]);
    assert_eq!(input.focused.as_ref().unwrap().row_key, key);
    assert!(input.pointer_release_target().is_none());

    let row = |scene: &Node| scene.at_path(&[0, 2, 1, 0]).unwrap().clone();
    let shifted_row = row(&shifted);
    assert_eq!(shifted_row.children[0].text("text"), Some("#2"));
    assert_eq!(shifted_row.children[1].text("text"), Some("用户已编辑"));
    assert!(shifted_row.children[2].boolean("checked", false));
    assert_eq!(shifted_row.children[3].text("text"), Some("加一 · 1"));
    assert_eq!(
        shifted.at_path(&field).unwrap().text("text"),
        Some("新记录")
    );

    assert!(input.scroll_y(&layout, 46.0 * 50.0));
    let (offscreen, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&offscreen, &layout);
    assert_eq!(
        offscreen.at_path(&list).unwrap().list.unwrap().first_row,
        50
    );
    assert_eq!(input.focused.as_ref().unwrap().path, list);
    assert!(input.scroll_y(&layout, -46.0 * 50.0));
    let (returned, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    input.reconcile(&returned, &layout);
    assert_eq!(row(&returned), shifted_row);
    let frame = renderer.snapshot(&returned, 630, 396, 1.0, &input).unwrap();
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("delegates-demo.png"))
            .unwrap();
    }

    let moved_field = [0, 2, 1, 0, 1];
    let bounds = layout.at_path(&moved_field).unwrap().bounds;
    input.pointer_move(
        &returned,
        &layout,
        Point {
            x: bounds.x + 10.0,
            y: bounds.y + 8.0,
        },
    );
    input.pointer_press();
    assert!(input.replay_text_edit(&mut ui, &returned, false));
    let (undone, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_eq!(
        undone.at_path(&moved_field).unwrap().text("text"),
        Some("记录 0")
    );
    assert!(input.replay_text_edit(&mut ui, &undone, true));

    let removed = model.remove(1).unwrap();
    assert!(model.insert(1, removed));
    assert!(!ui.edit_at(&returned, &[0, 2, 1, 0, 1], "过期事件".into()));
    let (replaced, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert_ne!(replaced.row_key_at(&[0, 2, 1, 0, 1]), key);
    let replacement = row(&replaced);
    assert_eq!(replacement.children[1].text("text"), Some("记录 0"));
    assert!(!replacement.children[2].boolean("checked", false));
    assert_eq!(replacement.children[3].text("text"), Some("加一 · 0"));
}

#[test]
fn large_string_model_updates_one_visible_row_without_losing_selection() {
    use std::{cell::Cell, rc::Rc};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = AsyncDemo::new();
    let model = VecModel::from(
        (0..100_000)
            .map(|i| format!("记录 {i}"))
            .collect::<Vec<_>>(),
    );
    ui.bind_model_results(model.clone());
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    ui.on_result_label(move |text| {
        calls.set(calls.get() + 1);
        text
    });
    let viewport = Size::new(540.0, 350.0);
    let path = [0, 3];
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let bounds = layout.at_path(&path).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: bounds.x + 20.0,
            y: bounds.y + 10.0,
        },
    );
    assert!(input.scroll_y(&layout, 30.0 * 45_000.0));
    let (scrolled, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    assert!(ui.control_at(&scrolled, &path, ControlAction::Select(45_000)));
    let base = ui.render_shared();
    let (before, layout) = renderer.prepare_scene(&ui, base.clone(), viewport, &input);
    assert_eq!(
        before.at_path(&path).unwrap().list.unwrap().first_row,
        45_000
    );
    assert!(
        before
            .at_path(&[0, 3, 0])
            .unwrap()
            .boolean("selected", false)
    );
    let before_frame = renderer.snapshot(&before, 540, 350, 1.0, &input).unwrap();
    let before_count = count.get();

    assert!(model.set_row(0, "屏外记录".into()));
    assert!(model.set_row(45_000, "后台更新的记录".into()));
    assert!(Rc::ptr_eq(&base, &ui.render_shared()));
    assert_eq!(count.get(), before_count);
    let (after, after_layout) = renderer.prepare_scene(&ui, base, viewport, &input);
    assert_eq!(layout, after_layout);
    assert_eq!(ui.get_selection(), 45_000);
    assert!(
        after
            .at_path(&[0, 3, 0])
            .unwrap()
            .boolean("selected", false)
    );
    assert_eq!(
        after.at_path(&[0, 3, 0, 0, 0]).unwrap().text("text"),
        Some("后台更新的记录")
    );
    assert_eq!(
        before.at_path(&[0, 3, 0, 0, 0]).unwrap().text("text"),
        Some("记录 45000")
    );
    assert_eq!(count.get(), before_count + 1);
    let after_frame = renderer.snapshot(&after, 540, 350, 1.0, &input).unwrap();
    assert_ne!(before_frame.rgba, after_frame.rgba);
}

#[test]
fn groups_keep_exclusive_selection_and_lazy_pages() {
    use std::{cell::Cell, rc::Rc};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = NavigationDemo::new();
    let visits = Rc::new(Cell::new([0, 0]));
    let observed = visits.clone();
    ui.on_visit_page(move |index| {
        let mut counts = observed.get();
        counts[index as usize] += 1;
        observed.set(counts);
        if index == 0 {
            "切换标签页后，计数仍会保留".into()
        } else {
            "只有当前页内容参与求值".into()
        }
    });
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 632, 358, 1.0, &input).unwrap();
    assert_eq!(visits.get(), [1, 0]);
    let group = [2, 0];
    let pages = [3, 0];
    let add = [3, 0, 0, 0, 1];
    let layout = &before.layout;
    assert_eq!(scene.at_path(&group).unwrap().children.len(), 3);
    assert!(!layout.at_path(&[3, 0, 1]).unwrap().visible);
    assert!(layout.at_path(&[3, 0, 1]).unwrap().tab_header.is_some());

    input.focus_next(&scene, layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, [2, 0, 0]);
    let selection = input
        .group_key(&scene, layout, NavigationKey::Down)
        .unwrap();
    assert_eq!(selection.path, group);
    assert!(ui.control_at(&scene, &selection.path, selection.action));
    assert_eq!(ui.get_profile(), 1);
    assert_eq!(ui.get_profile_label(), "高性能");
    let selected = ui.render_shared();
    let options = &selected.at_path(&group).unwrap().children;
    assert!(!options[0].boolean("checked", false));
    assert!(options[1].boolean("checked", false));

    assert!(ui.dispatch_at(&selected, &add, "clicked"));
    let changed = ui.render_shared();
    input.focus_next(&changed, layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, [3, 0, 0]);
    let header = layout.at_path(&[3, 0, 1]).unwrap().tab_header.unwrap();
    input.pointer_move(
        &changed,
        layout,
        Point {
            x: header.x + 12.0,
            y: header.y + 10.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, [3, 0, 1]);
    let next = input
        .select_group_option(&changed, layout, &target)
        .unwrap();
    assert_eq!(next.path, pages);
    assert!(ui.control_at(&changed, &next.path, next.action));
    let settings = ui.render_shared();
    assert_eq!(ui.get_page(), 1);
    assert_eq!(ui.get_changes(), 1);
    assert_eq!(visits.get(), [1, 1]);
    assert!(settings.at_path(&add).is_none());
    assert!(!ui.dispatch_at(&settings, &add, "clicked"));
    let settings_frame = renderer.snapshot(&settings, 632, 358, 1.0, &input).unwrap();
    assert_ne!(settings_frame.rgba, before.rgba);
    assert!(!ui.control_at(&settings, &pages, ControlAction::Select(2)));
    let next = input
        .group_key(&settings, &settings_frame.layout, NavigationKey::Right)
        .unwrap();
    assert_eq!(next.action, ControlAction::Select(0));
    assert!(ui.control_at(&settings, &next.path, next.action));
    let overview = ui.render_shared();
    assert_eq!(
        overview.find("counter").unwrap().children[0].text("text"),
        Some("此页计数：1")
    );
    assert_eq!(visits.get(), [1, 1]);

    let area = layout.at_path(&add).unwrap().bounds;
    input.pointer_move(
        &overview,
        layout,
        Point {
            x: area.x + 8.0,
            y: area.y + 8.0,
        },
    );
    input.pointer_press();
    assert_eq!(input.focused.as_ref().unwrap().path, add);
    ui.set_page(1);
    ui.set_vertical_tabs(true);
    let scene = ui.render_shared();
    let after = renderer.snapshot(&scene, 632, 358, 1.0, &input).unwrap();
    input.reconcile(&scene, &after.layout);
    assert_eq!(input.focused.as_ref().unwrap().path, [3, 0, 1]);
    let page = after.layout.at_path(&[3, 0, 1]).unwrap();
    let header = page.tab_header.unwrap();
    assert_eq!(page.bounds.x, header.x + header.width);

    let item = after.layout.at_path(&[2, 0, 0]).unwrap().bounds;
    input.pointer_move(
        &scene,
        &after.layout,
        Point {
            x: item.x + 8.0,
            y: item.y + 8.0,
        },
    );
    input.pointer_press();
    ui.set_profiles(vec!["新选项".into(), "高性能".into()]);
    let changed_model = ui.render_shared();
    let updated_layout = renderer.layout(&changed_model, xrack_layout::Size::new(632.0, 358.0));
    input.reconcile(&changed_model, &updated_layout);
    assert!(input.pointer_release_target().is_none());
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("navigation-demo.png"))
            .unwrap();
    }
}

#[test]
fn tab_bar_clips_overflow_and_paints_selection_after_a_bound_index_change() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TabBarDemo::new();
    let input = Interaction::default();
    let before = renderer
        .snapshot(&ui.render(), 536, 318, 1.0, &input)
        .unwrap();
    let selected = before.pixel(38, 61);
    assert_ne!(selected, before.pixel(164, 61));
    assert_eq!(before.pixel(513, 65), before.pixel(519, 65));

    ui.set_page(5);
    let after = renderer
        .snapshot(&ui.render(), 536, 318, 1.0, &input)
        .unwrap();
    let bar = after.layout.at_path(&[1]).unwrap();
    assert!(
        bar.scroll_on(xrack_runtime::Orientation::Horizontal)
            .unwrap()
            .offset
            > 0.0
    );
    let last = bar.children[5].bounds;
    assert_eq!(last.intersect(bar.bounds), last);
    assert_eq!(after.pixel(last.x as u32 + 14, 61), selected);
    assert_eq!(after.pixel(513, 65), after.pixel(519, 65));
}

#[test]
fn swipe_view_switches_lazily_and_restores_page_component_state() {
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = NavigationDemo::new();
    let calls = Rc::new([Cell::new(0), Cell::new(0), Cell::new(0)]);
    let capture = calls.clone();
    ui.on_visit_swipe_page(move |index| {
        let counter = &capture[index as usize];
        counter.set(counter.get() + 1);
        format!("当前页绑定 {index}")
    });
    let mut input = Interaction::default();
    let viewport = Size::new(632.0, 568.0);
    let view = [7, 0];
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 632, 568, 1.0, &input).unwrap();
    assert_eq!(calls[0].get(), 1);
    assert_eq!(calls[1].get(), 0);
    assert_eq!(calls[2].get(), 0);
    assert!(scene.at_path(&[7, 0, 1]).unwrap().children.is_empty());
    let button = [7, 0, 0, 0, 1];
    let bounds = before.layout.at_path(&button).unwrap().bounds;
    input.pointer_move(
        &scene,
        &before.layout,
        Point {
            x: bounds.x + 12.0,
            y: bounds.y + 12.0,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, button);
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));

    let scene = ui.render_shared();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    let bounds = layout.at_path(&view).unwrap().bounds;
    let start = Point {
        x: bounds.x + 490.0,
        y: bounds.y + 98.0,
    };
    let end = Point {
        x: start.x - 180.0,
        y: start.y,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::MousePan));
    let swipe = input
        .end_scroll_gesture(&scene, &layout, end, now + Duration::from_millis(65))
        .control
        .unwrap();
    assert_eq!(swipe.path, view);
    assert!(ui.control_at(&scene, &swipe.path, swipe.action));
    assert_eq!(ui.get_swipe_page(), 1);
    assert_eq!(ui.get_swipe_changes(), 1);
    let scene = ui.render_shared();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    input.reconcile(&scene, &layout);
    assert_eq!(input.focused.as_ref().unwrap().path, view);
    assert!(scene.at_path(&[7, 0, 0]).unwrap().children.is_empty());
    assert_eq!(calls[1].get(), 1);
    assert_eq!(calls[2].get(), 0);
    let frame = renderer.snapshot(&scene, 632, 568, 1.0, &input).unwrap();
    assert_ne!(before.rgba, frame.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("swipe-view.png"))
            .unwrap();
    }

    ui.set_show_middle(false);
    ui.set_vertical_swipes(true);
    let scene = ui.render_shared();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    assert_eq!(
        xrack_runtime::group_index(scene.at_path(&view).unwrap()),
        Some(0)
    );
    assert_eq!(
        scene.at_path(&[7, 0, 0, 0, 0]).unwrap().text("text"),
        Some("此页计数：1")
    );
    assert!(
        input
            .swipe_key(&scene, &layout, NavigationKey::Right)
            .is_none()
    );
    let down = input
        .swipe_key(&scene, &layout, NavigationKey::Down)
        .unwrap();
    assert_eq!(down.action, ControlAction::Select(2));
    assert!(ui.control_at(&scene, &down.path, down.action));
    assert_eq!(ui.get_swipe_changes(), 2);
    assert_eq!(ui.get_swipe_page(), 2);
    assert_eq!(calls[2].get(), 0);
    let final_page = ui.render_shared();
    assert!(final_page.at_path(&[7, 0, 0]).unwrap().children.is_empty());
    assert_eq!(calls[2].get(), 1);
    let layout = renderer.layout_with_input(&final_page, viewport, &input);
    let bounds = layout.at_path(&view).unwrap().bounds;
    let start = Point {
        x: bounds.x + 390.0,
        y: bounds.y + 24.0,
    };
    let end = Point {
        x: start.x,
        y: start.y + 74.0,
    };
    assert!(input.begin_scroll_gesture(&final_page, &layout, start, now, ScrollGestureKind::Touch));
    let up = input
        .end_scroll_gesture(&final_page, &layout, end, now + Duration::from_millis(45))
        .control
        .unwrap();
    assert_eq!(up.action, ControlAction::Select(0));
    assert!(ui.control_at(&final_page, &up.path, up.action));

    ui.set_swipe_interactive(false);
    let scene = ui.render_shared();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    assert!(
        input
            .swipe_key(&scene, &layout, NavigationKey::Down)
            .is_none()
    );
    assert!(!input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::MousePan));
    ui.set_swipe_page(2);
    assert_eq!(
        xrack_runtime::group_index(ui.render().at_path(&view).unwrap()),
        Some(2)
    );
}

#[test]
fn page_indicator_and_swipe_view_share_the_current_page() {
    use std::time::{Duration, Instant};
    use xrack_layout::{PageIndicatorGeometry, ScrollGestureKind};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = NavigationDemo::new();
    let viewport = Size::new(632.0, 598.0);
    let mut input = Interaction::default();
    let indicator = [7, 1];
    let view = [7, 0];
    let scene = ui.render_shared();
    let initial = renderer.snapshot(&scene, 632, 598, 1.0, &input).unwrap();
    let geometry = |scene: &Node, layout: &xrack_layout::LayoutNode| {
        PageIndicatorGeometry::new(
            scene.at_path(&indicator).unwrap(),
            layout.at_path(&indicator).unwrap().bounds,
        )
        .unwrap()
    };
    let markers: Vec<_> = geometry(&scene, &initial.layout).markers().collect();
    let pixel = |frame: &xrack_renderer_femtovg::Snapshot, point: Point| {
        frame.pixel(point.x as u32, point.y as u32)
    };
    assert_eq!(
        pixel(&initial, markers[0].center),
        Some([0x53, 0x6d, 0xfe, 255])
    );
    assert_eq!(
        pixel(&initial, markers[1].center),
        Some([0x75, 0x85, 0x9a, 255])
    );

    input.pointer_move(&scene, &initial.layout, markers[1].center);
    input.pointer_press();
    let clicked = input.page_marker_release(&scene, &initial.layout).unwrap();
    assert_eq!(clicked.path, indicator);
    assert!(ui.control_at(&scene, &clicked.path, clicked.action));
    assert!(!ui.control_at(&scene, &clicked.path, clicked.action));
    assert_eq!(ui.get_indicator_changes(), 1);
    assert_eq!(ui.get_swipe_page(), 1);
    let scene = ui.render_shared();
    assert_eq!(
        xrack_runtime::group_index(scene.at_path(&view).unwrap()),
        Some(1)
    );
    assert!(!ui.control_at(&scene, &indicator, ControlAction::Select(3)));
    ui.set_indicator_count(1);
    assert!(!ui.control_at(&scene, &indicator, ControlAction::Select(2)));
    ui.set_indicator_count(3);

    let layout = renderer.layout_with_input(&scene, viewport, &input);
    input.reconcile(&scene, &layout);
    let bounds = layout.at_path(&view).unwrap().bounds;
    let start = Point {
        x: bounds.x + 520.0,
        y: bounds.y + 90.0,
    };
    let end = Point {
        x: start.x - 180.0,
        y: start.y,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::MousePan));
    let swipe = input
        .end_scroll_gesture(&scene, &layout, end, now + Duration::from_millis(50))
        .control
        .unwrap();
    assert_eq!(swipe.path, view);
    assert!(ui.control_at(&scene, &swipe.path, swipe.action));
    assert_eq!(ui.get_swipe_changes(), 1);
    assert_eq!(ui.get_indicator_changes(), 1);
    let scene = ui.render_shared();
    let current = renderer.snapshot(&scene, 632, 598, 1.0, &input).unwrap();
    let marker = geometry(&scene, &current.layout)
        .markers()
        .find(|marker| marker.selected)
        .unwrap();
    assert_eq!(marker.index, 2);
    assert_eq!(
        pixel(&current, marker.center),
        Some([0x53, 0x6d, 0xfe, 255])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        current
            .save_png(std::path::Path::new(&path).join("page-indicator.png"))
            .unwrap();
    }
    let home = input
        .page_indicator_key(&scene, &current.layout, NavigationKey::Home)
        .unwrap();
    assert_eq!(home.path, indicator);
    assert!(ui.control_at(&scene, &home.path, home.action));
    assert_eq!(ui.get_swipe_page(), 0);
    assert_eq!(ui.get_indicator_changes(), 2);
}

#[test]
fn swipe_page_slides_follow_the_axis_and_right_to_left_direction() {
    use std::time::{Duration, Instant};
    use xrack_layout::{ScrollGestureKind, StackTransitions};
    use xrack_runtime::{Color, LayoutDirection, Length, Node, Orientation, SwipePreviewHandle};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();

    let viewport = Size::new(160.0, 124.0);
    let outgoing = [0xff, 0x00, 0x00, 0xff];
    let incoming = [0x00, 0x00, 0xff, 0xff];
    for (axis, direction, physical, start, end) in [
        (
            Orientation::Horizontal,
            LayoutDirection::LeftToRight,
            1.0,
            (31, 50),
            (128, 50),
        ),
        (
            Orientation::Horizontal,
            LayoutDirection::RightToLeft,
            -1.0,
            (128, 50),
            (31, 50),
        ),
        (
            Orientation::Vertical,
            LayoutDirection::LeftToRight,
            1.0,
            (80, 25),
            (80, 98),
        ),
    ] {
        let mut before = Node::new("Window", None);
        before
            .properties
            .insert("background", Color(0xffffffff).into());
        let mut view = Node::new("SwipeView", Some("pages"));
        view.properties.extend([
            ("layout-direction", direction.into()),
            ("orientation", axis.into()),
            ("transition-duration", 100_i64.into()),
            ("x", Length(20.0).into()),
            ("y", Length(16.0).into()),
            ("width", Length(120.0).into()),
            ("height", Length(90.0).into()),
        ]);
        for (id, color) in [("first", outgoing), ("second", incoming)] {
            let mut page = Node::new("SwipePage", Some(id));
            let mut face = Node::new("Rectangle", None);
            face.properties
                .insert("background", Color(u32::from_be_bytes(color)).into());
            face.properties.insert("width", Length(120.0).into());
            face.properties.insert("height", Length(90.0).into());
            page.children.push(face);
            view.children.push(page);
        }
        view.enable_swipe_preview(SwipePreviewHandle::default());
        before.children.push(view);
        let mut input = Interaction::default();
        let before_layout = renderer.layout_with_input(&before, viewport, &input);
        let touch = match axis {
            Orientation::Horizontal => Point {
                x: 80.0 + 40.0 * physical,
                y: 50.0,
            },
            Orientation::Vertical => Point { x: 80.0, y: 88.0 },
        };
        let dragged = match axis {
            Orientation::Horizontal => Point {
                x: touch.x - 60.0 * physical,
                y: touch.y,
            },
            Orientation::Vertical => Point {
                x: touch.x,
                y: touch.y - 45.0,
            },
        };
        let gesture = Instant::now();
        assert!(input.begin_scroll_gesture(
            &before,
            &before_layout,
            touch,
            gesture,
            ScrollGestureKind::Touch
        ));
        assert!(input.move_scroll_gesture(
            &before,
            &before_layout,
            dragged,
            gesture + Duration::from_millis(20)
        ));
        before.children[0].mount_swipe_pages(|_, _| {});
        let during = renderer.snapshot(&before, 160, 124, 1.0, &input).unwrap();
        assert_eq!(during.pixel(start.0, start.1), Some(outgoing));
        assert_eq!(during.pixel(end.0, end.1), Some(incoming));
        assert_eq!(during.pixel(19, 15), Some([255; 4]));
        assert_eq!(xrack_runtime::group_index(&before.children[0]), Some(0));
        let release = input.end_scroll_gesture(
            &before,
            &before_layout,
            dragged,
            gesture + Duration::from_millis(45),
        );
        assert!(release.control.is_some());
        let mut after = before.clone();
        after.children[0]
            .properties
            .insert("current-index", 1_i64.into());
        after.children[0].mount_swipe_pages(|_, _| {});
        let after_layout = renderer.layout_with_input(&after, viewport, &input);
        let mut transitions = StackTransitions::default();
        let now = Instant::now();
        transitions.update_with_input(
            (&before, &before_layout),
            (&after, &after_layout),
            now,
            Some(&input),
        );
        assert_eq!(
            transitions.at(&[0]).unwrap().incoming_offset(),
            physical * 0.5
        );
        let continued = renderer
            .snapshot_with_transitions(&after, 160, 124, 1.0, &input, &transitions)
            .unwrap();
        assert_eq!(
            continued.pixel(start.0, start.1),
            during.pixel(start.0, start.1)
        );
        assert_eq!(continued.pixel(end.0, end.1), during.pixel(end.0, end.1));

        transitions = StackTransitions::default();
        transitions.update((&before, &before_layout), (&after, &after_layout), now);
        assert_eq!(transitions.at(&[0]).unwrap().incoming_offset(), physical);

        let midpoint = now + Duration::from_millis(20);
        transitions.advance(midpoint);
        let frame = renderer
            .snapshot_with_transitions(&after, 160, 124, 1.0, &input, &transitions)
            .unwrap();
        assert_eq!(
            frame.pixel(start.0, start.1),
            Some(outgoing),
            "{axis:?} {direction:?}"
        );
        assert_eq!(
            frame.pixel(end.0, end.1),
            Some(incoming),
            "{axis:?} {direction:?}"
        );
        assert_eq!(frame.pixel(19, 15), Some([255; 4]));

        transitions.update((&after, &after_layout), (&before, &before_layout), midpoint);
        let interrupted = renderer
            .snapshot_with_transitions(&before, 160, 124, 1.0, &input, &transitions)
            .unwrap();
        assert_eq!(interrupted.pixel(start.0, start.1), Some(outgoing));
        assert_eq!(interrupted.pixel(end.0, end.1), Some(incoming));
        transitions.advance(midpoint + Duration::from_millis(100));
        assert!(!transitions.is_active());
    }
}

#[test]
fn stack_slide_rebases_on_the_presented_frame_and_preserves_keyboard_navigation() {
    use std::{
        cell::RefCell,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_layout::{ScrollGestureKind, StackTransitions};
    use xrack_runtime::StackNavigation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StackDemo::new();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let capture = calls.clone();
    ui.on_visit_route(move |route| {
        capture.borrow_mut().push(route.clone());
        route
    });
    let mut input = Interaction::default();
    let mut slides = StackTransitions::default();
    let stack = [0, 2];
    let overview = ui.render_shared();
    assert_eq!(
        overview.at_path(&stack).unwrap().int("transition-duration"),
        Some(220)
    );
    let layout = renderer.layout_with_input(&overview, Size::new(590.0, 324.0), &input);
    let bounds = layout.at_path(&stack).unwrap().bounds;
    let button = layout.at_path(&[0, 2, 0, 0, 1]).unwrap().bounds;
    let point = Point {
        x: button.x + 10.0,
        y: button.y + 10.0,
    };
    input.pointer_move(&overview, &layout, point);
    input.pointer_press();
    assert_eq!(input.pointer_release(), Some("overview-next"));

    let start = Instant::now();
    assert!(ui.navigate_stack("navigation", StackNavigation::Push("details")));
    let details = ui.render_shared();
    let detail_layout = renderer.layout_with_input(&details, Size::new(590.0, 324.0), &input);
    slides.update((&overview, &layout), (&details, &detail_layout), start);
    assert!(input.sync_stack_transitions(&slides));
    input.reconcile(&details, &detail_layout);
    assert!(input.pointer_on_stack_slide());
    let detail_focus = input.focused.clone().unwrap();
    assert!(detail_focus.path.starts_with(&[0, 2, 1]));
    input.pointer_press();
    assert!(input.pointer_release().is_none());
    assert_eq!(input.focused.as_ref(), Some(&detail_focus));
    assert!(!input.begin_scroll_gesture(
        &details,
        &detail_layout,
        point,
        start,
        ScrollGestureKind::Touch
    ));

    let middle = start + Duration::from_millis(45);
    slides.advance(middle);
    let outgoing = [0x26, 0x37, 0x50, 255];
    let incoming = [0x25, 0x3f, 0x51, 255];
    let before = renderer
        .snapshot_with_transitions(&details, 1180, 648, 2.0, &input, &slides)
        .unwrap();
    let y = ((bounds.y + bounds.height - 20.0) * 2.0) as u32;
    let left = ((bounds.x + bounds.width * 0.2) * 2.0) as u32;
    let right = ((bounds.x + bounds.width * 0.8) * 2.0) as u32;
    assert_eq!(before.pixel(left, y), Some(outgoing));
    assert_eq!(before.pixel(right, y), Some(incoming));
    assert_eq!(&*calls.borrow(), &["overview", "details"]);
    assert_eq!(renderer.stack_layer_stats().rasterized, 1);
    let repeated = renderer
        .snapshot_with_transitions(&details, 1180, 648, 2.0, &input, &slides)
        .unwrap();
    assert_eq!(before.pixel(left, y), repeated.pixel(left, y));
    assert_eq!(renderer.stack_layer_stats().rasterized, 1);
    assert_eq!(renderer.stack_layer_stats().reused, 1);

    let back = input.stack_back(&details, &detail_layout).unwrap();
    assert!(ui.control_at(&details, &back.path, back.action));
    let restored = ui.render_shared();
    let restored_layout = renderer.layout_with_input(&restored, Size::new(590.0, 324.0), &input);
    slides.update(
        (&details, &detail_layout),
        (&restored, &restored_layout),
        middle,
    );
    assert_eq!(slides.at(&stack).unwrap().incoming_offset(), -1.0);
    input.reconcile(&restored, &restored_layout);
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("overview-next")
    );
    let rebased = renderer
        .snapshot_with_transitions(&restored, 1180, 648, 2.0, &input, &slides)
        .unwrap();
    for x in ((bounds.x * 2.0) as u32)..(((bounds.x + bounds.width) * 2.0) as u32) {
        assert_eq!(before.pixel(x, y), rebased.pixel(x, y), "x={x}");
    }
    let outside = (bounds.x * 2.0) as u32 - 2;
    let settled = renderer
        .snapshot(&restored, 1180, 648, 2.0, &input)
        .unwrap();
    assert_eq!(before.pixel(outside, y), settled.pixel(outside, y));
    assert_eq!(rebased.pixel(outside, y), settled.pixel(outside, y));

    slides.advance(middle + Duration::from_millis(220));
    assert!(!slides.is_active());
    assert!(input.sync_stack_transitions(&slides));
    input.reconcile(&restored, &restored_layout);
    assert!(!input.pointer_on_stack_slide());
    input.pointer_press();
    assert_eq!(input.pointer_release(), Some("overview-next"));
}

#[test]
fn retained_vector_paths_follow_paint_and_geometry_changes()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node, StaticVectorPath, VectorSegment};

    const COMMANDS: &str = "M2 2 H22 V22 H2 Z";
    static TRIANGLE: StaticVectorPath = StaticVectorPath {
        commands: COMMANDS,
        bounds: [2.0, 2.0, 20.0, 20.0],
        segments: &[
            VectorSegment::MoveTo([2.0, 2.0]),
            VectorSegment::LineTo([22.0, 2.0]),
            VectorSegment::LineTo([2.0, 22.0]),
            VectorSegment::Close,
        ],
    };
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut renderer = Renderer::with_device(gpu.clone())?;
    let input = Interaction::default();
    let mut scene = Node::new("Window", None);
    let mut path = Node::new("Path", None);
    path.properties
        .insert("commands", COMMANDS.to_owned().into());
    path.properties.insert("width", Length(72.0).into());
    path.properties.insert("height", Length(72.0).into());
    path.properties.insert("fill", Color(0x526ff4ff).into());
    path.properties.insert("viewbox-width", 24.0_f64.into());
    path.properties.insert("viewbox-height", 24.0_f64.into());
    scene.children.push(path);

    let first = renderer.snapshot(&scene, 100, 90, 1.0, &input)?;
    assert_eq!(first.pixel(28, 28), Some([0x52, 0x6f, 0xf4, 0xff]));

    scene.children[0]
        .properties
        .insert("fill", Color(0xf9775cff).into());
    let recolored = renderer.snapshot(&scene, 100, 90, 1.0, &input)?;
    assert_eq!(recolored.pixel(28, 28), Some([0xf9, 0x77, 0x5c, 0xff]));
    assert_eq!(recolored.layout.bounds, first.layout.bounds);

    scene.children[0].set_precompiled_path(&TRIANGLE);
    let replaced = renderer.snapshot(&scene, 100, 90, 1.0, &input)?;
    assert_eq!(replaced.pixel(16, 16), Some([0xf9, 0x77, 0x5c, 0xff]));
    assert_ne!(replaced.pixel(57, 57), recolored.pixel(57, 57));

    let path = &mut scene.children[0];
    path.properties.insert("x", Length(9.0).into());
    path.properties.insert("viewbox-width", 32.0_f64.into());
    path.properties.insert("stroke", Color(0x121728ff).into());
    path.properties.insert("stroke-width", Length(2.0).into());

    let warmed = renderer.snapshot(&scene, 200, 180, 2.0, &input)?;
    let mut fresh = Renderer::with_device(gpu)?;
    fresh.set_vector_path_cache_enabled(false);
    let expected = fresh.snapshot(&scene, 200, 180, 2.0, &input)?;
    assert_ne!(warmed.rgba, recolored.rgba);
    assert_same_pixels(&expected, &warmed);
    assert_same_pixels(
        &expected,
        &renderer.snapshot(&scene, 200, 180, 2.0, &input)?,
    );
    Ok(())
}

#[test]
fn vector_masks_match_translucent_paint_and_clipping() -> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Brush, Color, GradientStop, Length, Node};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut cached = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_vector_mask_cache_enabled(false);
    let input = Interaction::default();
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0x334f79ff).into());
    let mut parent = Node::new("Rectangle", None);
    parent.properties.insert("width", Length(87.0).into());
    parent.properties.insert("height", Length(38.0).into());
    parent.properties.insert("clip", true.into());
    let commands =
        "M3 8 C3 1 22 1 22 8 C24 10 17 16 21 22 C10 26 1 24 4 16 C7 13 3 13 3 8 Z ".repeat(5);
    for x in [4.0, 39.0, 71.0] {
        let mut node = Node::new("Path", None);
        node.properties.insert("commands", commands.clone().into());
        node.properties.insert("x", Length(x).into());
        node.properties.insert("y", Length(4.0).into());
        node.properties.insert("width", Length(32.0).into());
        node.properties.insert("height", Length(32.0).into());
        node.properties.insert("viewbox-width", 24.0_f64.into());
        node.properties.insert("viewbox-height", 24.0_f64.into());
        node.properties.insert("fill", Color(0xf0856599).into());
        node.properties.insert("stroke", Color(0x54deb97a).into());
        node.properties.insert("stroke-width", Length(1.5).into());
        parent.children.push(node);
    }
    scene.children.push(parent);

    let assert_close = |case: &str,
                        before: &xrack_renderer_femtovg::Snapshot,
                        after: &xrack_renderer_femtovg::Snapshot| {
        assert_eq!(before.rgba.len(), after.rgba.len());
        let max_diff = before
            .rgba
            .iter()
            .zip(&after.rgba)
            .map(|(a, b)| a.abs_diff(*b))
            .max();
        assert!(
            // An 8-bit mask introduces rounding before the final blend.
            max_diff.is_some_and(|error| error <= 3),
            "{case}: largest channel error: {max_diff:?}"
        );
    };
    for scale in [1.0, 2.0] {
        let first = &mut scene.children[0].children[0];
        first.properties.insert("fill", Color(0xf0856599).into());
        first.properties.insert("stroke", Color(0x54deb97a).into());
        let size = (96.0 * scale) as u32;
        let height = (46.0 * scale) as u32;
        let stats = cached.vector_mask_stats();
        let expected = direct.snapshot(&scene, size, height, scale, &input)?;
        let actual = cached.snapshot(&scene, size, height, scale, &input)?;
        assert_close("clipped masks", &expected, &actual);
        assert_eq!(cached.vector_mask_stats().rasterized - stats.rasterized, 2);
        assert!(cached.vector_mask_stats().reused > stats.reused);

        let changed = &mut scene.children[0].children[0];
        changed.properties.insert("fill", Color(0xffe77b99).into());
        changed
            .properties
            .insert("stroke", Color(0x2815d57a).into());
        let stats = cached.vector_mask_stats();
        let expected = direct.snapshot(&scene, size, height, scale, &input)?;
        let recolored = cached.snapshot(&scene, size, height, scale, &input)?;
        assert_close("recolored masks", &expected, &recolored);
        assert_ne!(actual.rgba, recolored.rgba);
        assert_eq!(cached.vector_mask_stats().rasterized, stats.rasterized);
    }

    for node in &mut scene.children[0].children {
        node.properties.insert("stroke", Color(0x2815d5cc).into());
    }
    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let different_alpha = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_close("updated source alpha", &expected, &different_alpha);
    assert_eq!(cached.vector_mask_stats().rasterized - stats.rasterized, 2);

    let parent = &mut scene.children[0];
    parent.properties.insert("opacity", 0.6_f64.into());
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let grouped = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_close("group opacity", &expected, &grouped);
    assert_ne!(different_alpha.rgba, grouped.rgba);

    scene.children[0]
        .properties
        .insert("border-radius", Length(12.0).into());
    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let rounded = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_same_pixels(&expected, &rounded);
    assert_eq!(cached.vector_mask_stats().direct - stats.direct, 3);

    let parent = &mut scene.children[0];
    parent.properties.remove("opacity");
    parent.properties.remove("border-radius");
    cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    let parent = &mut scene.children[0];
    let original_fill = parent.children[0]
        .properties
        .insert(
            "fill",
            Brush::Linear {
                angle_degrees: 45.0,
                stops: [
                    GradientStop {
                        color: Color(0x1742ee99),
                        offset: 0.0,
                    },
                    GradientStop {
                        color: Color(0xd438ae99),
                        offset: 0.38,
                    },
                    GradientStop {
                        color: Color(0xfad94499),
                        offset: 1.0,
                    },
                ]
                .into(),
            }
            .into(),
        )
        .ok_or("path should have a solid fill")?;
    let original_stroke = parent.children[1]
        .properties
        .insert(
            "stroke",
            Brush::Radial {
                stops: [
                    GradientStop {
                        color: Color(0x4adcb3cc),
                        offset: 0.0,
                    },
                    GradientStop {
                        color: Color(0x282368cc),
                        offset: 1.0,
                    },
                ]
                .into(),
            }
            .into(),
        )
        .ok_or("path should have a solid stroke")?;

    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let gradients = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_close("linear fill and radial stroke", &expected, &gradients);
    assert_eq!(cached.vector_mask_stats().direct, stats.direct);
    assert_eq!(cached.vector_mask_stats().rasterized, stats.rasterized);
    assert!(cached.vector_mask_stats().reused > stats.reused);

    let parent = &mut scene.children[0];
    parent.children[0].properties.insert("fill", original_fill);
    parent.children[1]
        .properties
        .insert("stroke", original_stroke);
    let fill = parent.children[0]
        .properties
        .insert(
            "fill",
            Brush::Linear {
                angle_degrees: 45.0,
                stops: [
                    GradientStop {
                        color: Color(0x552ceaff),
                        offset: 0.0,
                    },
                    GradientStop {
                        color: Color(0xfa940a89),
                        offset: 1.0,
                    },
                ]
                .into(),
            }
            .into(),
        )
        .ok_or("path should have a solid fill")?;
    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let gradient = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_close("varying-alpha gradient fallback", &expected, &gradient);
    assert_eq!(cached.vector_mask_stats().direct - stats.direct, 1);
    scene.children[0].children[0]
        .properties
        .insert("fill", fill);

    scene.children[0].children[0]
        .properties
        .insert("x", Length(4.25).into());
    scene.children[0].children[1]
        .properties
        .insert("transform-rotation", 12.0_f64.into());
    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let adjusted = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_close("fractional origin and rotation", &expected, &adjusted);
    assert_eq!(cached.vector_mask_stats().direct - stats.direct, 2);

    scene.children[0]
        .properties
        .insert("width", Length(87.25).into());
    let stats = cached.vector_mask_stats();
    let expected = direct.snapshot(&scene, 192, 92, 2.0, &input)?;
    let fractional_clip = cached.snapshot(&scene, 192, 92, 2.0, &input)?;
    assert_same_pixels(&expected, &fractional_clip);
    assert_eq!(cached.vector_mask_stats().direct - stats.direct, 3);
    Ok(())
}

#[test]
#[ignore = "manual GPU measurement for animated fills on fixed vector paths"]
fn benchmark_vector_geometry() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    use xrack_runtime::{Brush, Color, GradientStop, Length, Node, Value};

    let gradient = std::env::var_os("XRACK_VECTOR_BENCH_GRADIENT").is_some();
    let viewport = Size::new(640.0, 480.0);
    let mut scene = Node::new("Window", None);
    for row in 0..12 {
        for column in 0..16 {
            let mut icon = Node::new("Path", None);
            for (name, length) in [
                ("x", f64::from(column * 40 + 2)),
                ("y", f64::from(row * 40 + 2)),
                ("width", 36.0),
                ("height", 36.0),
            ] {
                icon.properties.insert(name, Length(length).into());
            }
            icon.properties.insert("viewbox-width", 24.0_f64.into());
            icon.properties.insert("viewbox-height", 24.0_f64.into());
            icon.properties.insert("stroke-width", Length(1.0).into());
            icon.properties.insert("stroke", Color(0x121728ff).into());
            scene.children.push(icon);
        }
    }
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Vector geometry benchmark"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 960,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    for shapes in [1, 12] {
        // Explicit subpath starts keep the single-shape case compatible with Qt Quick and Slint.
        let commands: Vec<_> = (0..shapes).map(|index| {
            let control = 17.0 + index as f32 / 4.0;
            format!("M12 4 C{control} 1 23 7 20 12 C23 17 17 23 12 20 C7 23 1 17 4 12 C1 7 7 1 12 4 Z ").repeat(8)
        }).collect();
        for (index, icon) in scene.children.iter_mut().enumerate() {
            icon.properties
                .insert("commands", commands[index % shapes].clone().into());
        }
        for (mode, retained, masks) in [
            ("direct", false, false),
            ("paths", true, false),
            ("masks", true, true),
        ] {
            renderer.set_vector_path_cache_enabled(retained);
            renderer.set_vector_mask_cache_enabled(masks);
            let before = renderer.vector_mask_stats();
            let mut submit = Vec::with_capacity(60);
            let mut total = Vec::with_capacity(60);
            for index in 0..70 {
                let rgb = if index % 2 == 0 {
                    0x526ff4ff
                } else {
                    0xf9775cff
                };
                let fill: Value = if gradient {
                    Brush::Linear {
                        angle_degrees: 90.0,
                        stops: [
                            GradientStop {
                                color: Color(rgb),
                                offset: 0.0,
                            },
                            GradientStop {
                                color: Color(0x663aacff),
                                offset: 0.45,
                            },
                            GradientStop {
                                color: Color(0xf5e7afff),
                                offset: 1.0,
                            },
                        ]
                        .into(),
                    }
                    .into()
                } else {
                    Color(rgb).into()
                };
                for icon in &mut scene.children {
                    icon.properties.insert("fill", fill.clone());
                }
                let start = Instant::now();
                renderer.draw_with_layout(&scene, &layout, &texture, 2.0, &input)?;
                let submitted = start.elapsed();
                renderer.device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })?;
                if index >= 10 {
                    submit.push(submitted);
                    total.push(start.elapsed());
                }
            }
            for (label, samples) in [("submit", &mut submit), ("GPU completion", &mut total)] {
                samples.sort_unstable();
                println!(
                    "vector geometry shapes={shapes} gradient={gradient} mode={mode} {label}: p50={:.2}ms p95={:.2}ms",
                    samples[29].as_secs_f64() * 1000.0,
                    samples[56].as_secs_f64() * 1000.0,
                );
            }
            let after = renderer.vector_mask_stats();
            if masks {
                assert_eq!(after.rasterized - before.rasterized, shapes as u64 * 2);
                assert!(after.reused - before.reused > 10_000);
            }
        }
    }
    Ok(())
}

fn long_styled_text_markdown(paragraphs: usize) -> Result<String, std::fmt::Error> {
    use std::fmt::Write;
    let mut source = String::new();
    for index in 0..paragraphs {
        writeln!(
            source,
            "**{index:04}** 构建日志 · `cargo test` · [source](https://example.invalid/{index}) · שלום 🌐\n"
        )?;
    }
    Ok(source)
}

fn long_styled_text_scene(
    size: Size,
    paragraphs: usize,
) -> Result<xrack_runtime::Node, Box<dyn std::error::Error>> {
    use xrack_runtime::{Color, Length, Node, StyledText};

    let mut document = Node::new("StyledText", None);
    document.properties.insert(
        "text",
        StyledText::from_markdown(&long_styled_text_markdown(paragraphs)?)?.into(),
    );
    document
        .properties
        .insert("width", Length(f64::from(size.width - 40.0)).into());
    document.properties.insert("selectable", true.into());
    document
        .properties
        .insert("color", Color(0x18233dff).into());
    let mut scroll = Node::new("ScrollView", None);
    scroll
        .properties
        .insert("width", Length(f64::from(size.width)).into());
    scroll
        .properties
        .insert("height", Length(f64::from(size.height)).into());
    scroll.children.push(document);
    let mut scene = Node::new("Window", None);
    scene.children.push(scroll);
    Ok(scene)
}

#[test]
fn clipped_long_styled_text_preserves_selection_ink_through_transforms()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Length, Orientation, TextCommand};

    let size = Size::new(480.0, 320.0);
    let mut scene = long_styled_text_scene(size, 300)?;
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut renderer = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_styled_text_culling_enabled(false);

    for (scale, rotation, scale_x, scale_y, nested_rotation) in [
        (1.0, 0.0, 1.0, 1.0, 0.0),
        (2.0, 0.0, 1.0, 1.0, 0.0),
        (2.0, 12.0, 1.0, 1.0, 0.0),
        (1.5, -8.0, 0.82, 1.24, -3.0),
        (1.0, 0.0, -0.77, 1.225, 0.0),
        (1.25, 7.5, 1.18, -0.86, 0.0),
        (2.0, 0.0, 0.56, 0.56, 0.0),
    ] {
        let viewport = &mut scene.children[0];
        viewport
            .properties
            .insert("transform-rotation", rotation.into());
        viewport
            .properties
            .insert("transform-scale-x", scale_x.into());
        viewport
            .properties
            .insert("transform-scale-y", scale_y.into());
        let document = &mut viewport.children[0];
        document
            .properties
            .insert("transform-rotation", nested_rotation.into());
        document
            .properties
            .insert("transform-origin-y", Length(365.25).into());
        scene.properties.insert(
            "default-font-size",
            Length(12.0 + 2.0 * f64::from(scale)).into(),
        );
        let mut input = Interaction::default();
        let initial = renderer.layout_with_input(&scene, size, &input);
        let target = input
            .accessibility_target(&scene, &initial, &[0, 0])
            .ok_or("missing text selection target")?;
        input.focus_target(&scene, &initial, target);
        input.sync_editor(&scene);
        let text = scene.children[0].children[0]
            .text("text")
            .ok_or("missing rich text")?;
        input.apply_text_selection(
            &scene,
            TextCommand::SetSelection {
                anchor: 0,
                focus: text.len() as i64,
            },
        );
        let selected = input
            .displayed_editor(&scene.children[0].children[0], &[0, 0])
            .ok_or("missing styled-text selection")?;
        assert_eq!(selected.selection(text), 0..text.len());
        let plan = initial.children[0].children[0]
            .styled_text
            .as_ref()
            .ok_or("missing initial rich-text layout")?;
        for offset in [365.25, 939.5, 117.0] {
            assert!(input.accessibility_scroll_to(&initial, &[0], Orientation::Vertical, offset));
            let width = (size.width * scale) as u32;
            let height = (size.height * scale) as u32;
            let expected = direct.snapshot(&scene, width, height, scale, &input)?;
            let before = renderer.styled_text_culled_fragments();
            let clipped = renderer.snapshot(&scene, width, height, scale, &input)?;
            assert_same_pixels(&clipped, &expected);
            assert_eq!(clipped.layout, expected.layout);
            let scrolled = clipped.layout.children[0].children[0]
                .styled_text
                .as_ref()
                .ok_or("missing scrolled rich-text layout")?;
            assert!(std::rc::Rc::ptr_eq(plan, scrolled));
            if !cfg!(any(target_os = "ios", target_os = "android")) {
                assert!(renderer.styled_text_culled_fragments() > before + 100);
            }
        }
    }
    Ok(())
}

#[test]
fn batched_rectangles_preserve_clipping_and_paint_order() -> Result<(), Box<dyn std::error::Error>>
{
    use rectangle_animation_ui::RectangleAnimation;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_rectangle_optimization_enabled(false);
    let input = Interaction::default();
    let mut aot = RectangleAnimation::new();
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("RectangleAnimation")?;
    for (rounded, opacity, shifted, offset) in [
        (false, 1.0, false, 0.0),
        (false, 1.0, true, 0.0),
        (true, 1.0, false, 0.0),
        (true, 1.0, true, 0.0),
        (true, 0.64, false, 0.0),
        (true, 0.64, true, 0.0),
        (true, 1.0, false, 0.25),
        (true, 0.64, true, 0.173),
    ] {
        aot.set_rounded(rounded);
        aot.set_group_opacity(opacity);
        aot.set_shifted(shifted);
        aot.set_round_offset(Length(offset));
        interpreter.set("rounded", rounded.into())?;
        interpreter.set("group-opacity", opacity.into())?;
        interpreter.set("shifted", shifted.into())?;
        interpreter.set("round-offset", Length(offset).into())?;
        let window = Component::render(&aot);
        let expected = direct.snapshot(&window, 192, 144, 2.0, &input)?;
        let batch_draws = |renderer: &Renderer| {
            if rounded {
                renderer.rounded_rectangle_batch_draws()
            } else {
                renderer.rectangle_batch_draws()
            }
        };
        let before = batch_draws(&optimized);
        let actual = optimized.snapshot(&window, 192, 144, 2.0, &input)?;
        assert_eq!(
            batch_draws(&optimized) - before,
            2,
            "rounded={rounded}, opacity={opacity}, x={offset}"
        );
        assert_eq!(actual.layout, expected.layout);
        let interpreted = optimized.snapshot(&interpreter.render(), 192, 144, 2.0, &input)?;
        assert_eq!(batch_draws(&optimized) - before, 4);
        assert_same_pixels(&actual, &interpreted);
        for (index, (actual_pixel, expected_pixel)) in actual
            .rgba
            .chunks_exact(4)
            .zip(expected.rgba.chunks_exact(4))
            .enumerate()
        {
            // The color atlas and the general painter's offscreen layer round
            // RGBA8 at different points before source-over blending.
            assert!(
                actual_pixel
                    .iter()
                    .zip(expected_pixel)
                    .all(|(a, b)| a.abs_diff(*b) <= 2),
                "rounded={rounded}, opacity={opacity}, shifted={shifted}, x={offset}, pixel ({}, {}): {actual_pixel:?} vs {expected_pixel:?}",
                index % 192,
                index / 192,
            );
        }
    }
    Ok(())
}

#[test]
fn rounded_atlas_reuses_fractional_tiles_across_frames() -> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::RectangleAnimation;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut retained = Renderer::with_device(gpu.clone())?;
    let mut reset = Renderer::with_device(gpu)?;
    reset.set_fractional_rounded_tile_retention_enabled(false);
    let mut aot = RectangleAnimation::new();
    aot.set_rounded(true);
    aot.set_group_opacity(0.64);
    let input = Interaction::default();
    for phase in [0, 0].into_iter().chain(1..16).chain([12, 15, 15]) {
        aot.set_round_offset(Length(f64::from(2 * phase + 1) / 64.0));
        let scene = aot.render_shared();
        let actual = retained.snapshot(&scene, 192, 144, 2.0, &input)?;
        let expected = reset.snapshot(&scene, 192, 144, 2.0, &input)?;
        assert_same_pixels(&actual, &expected);
    }
    assert!(reset.rounded_tile_rasterizations() > 0);
    assert_eq!(retained.rounded_tile_rasterizations(), 0);
    Ok(())
}

#[test]
#[ignore = "manual GPU measurement of rectangle motion and opacity"]
fn benchmark_rectangle_animation() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    use xrack_runtime::{Color, Length, Node};

    let viewport = Size::new(640.0, 480.0);
    let rounded = std::env::var_os("XRACK_RECTANGLE_BENCH_ROUNDED").is_some();
    let subpixel = std::env::var_os("XRACK_RECTANGLE_BENCH_SUBPIXEL").is_some();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Rectangle animation benchmark"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 960,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xffff_ffff).into());
    for index in 0..600 {
        let mut tile = Node::new("Rectangle", None);
        tile.properties.insert("width", Length(16.0).into());
        tile.properties.insert("height", Length(14.0).into());
        if rounded {
            tile.properties.insert("border-radius", Length(2.0).into());
        }
        tile.properties.insert(
            "background",
            Color([0x2868_d4ff, 0xb83c_5aff, 0x25a7_6aff, 0xdd9b_28ff][index % 4]).into(),
        );
        scene.children.push(tile);
    }
    let update = |scene: &mut Node, phase, fading| {
        for (n, tile) in scene.children.iter_mut().enumerate() {
            let shift = if subpixel {
                ((phase * 137 + n * 27) % 1000) as f64 * 0.001
            } else {
                0.0
            };
            let x = ((n % 30) * 21 + 3 + (phase + n) % 5) as f64 + shift;
            let y = ((n / 30) * 22 + 3 + (phase / 3 + n * 3) % 5) as f64 + shift;
            tile.properties.insert("x", Length(x).into());
            tile.properties.insert("y", Length(y).into());
            let alpha = if fading {
                0.50 + ((phase + n) % 8) as f64 * 0.05
            } else {
                1.0
            };
            tile.properties.insert("opacity", alpha.into());
        }
    };
    update(&mut scene, 0, false);
    let proof = renderer.snapshot(&scene, 1280, 960, 2.0, &input)?;
    assert_eq!(proof.pixel(12, 12), Some([0x28, 0x68, 0xd4, 255]));
    update(&mut scene, 1, true);
    let proof = renderer.snapshot(&scene, 1280, 960, 2.0, &input)?;
    let blended = proof.pixel(12, 12).ok_or("missing animated tile")?;
    assert!(blended[0] > 0x28 && blended[0] < 0xff);
    for fading in [false, true] {
        let mut samples = [Vec::new(), Vec::new(), Vec::new()];
        for phase in 0..70 {
            let start = Instant::now();
            update(&mut scene, phase, fading);
            let layout = renderer.layout_with_input(&scene, viewport, &input);
            let prepared = start.elapsed();
            renderer.draw_with_layout(&scene, &layout, &texture, 2.0, &input)?;
            let submitted = start.elapsed();
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            if phase >= 10 {
                for (series, elapsed) in
                    samples
                        .iter_mut()
                        .zip([prepared, submitted - prepared, start.elapsed()])
                {
                    series.push(elapsed);
                }
            }
        }
        for (stage, samples) in ["prepare", "submit", "total"].into_iter().zip(&mut samples) {
            samples.sort_unstable();
            println!(
                "rectangle animation fading={fading} {stage}: p50={:.3}ms p95={:.3}ms",
                samples[29].as_secs_f64() * 1000.0,
                samples[56].as_secs_f64() * 1000.0,
            );
        }
    }
    Ok(())
}

#[test]
fn opaque_window_background_clear_matches_the_regular_painter() {
    use xrack_runtime::{Brush, Color, Length, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let mut scene = Node::new("Window", None);
    let mut child = Node::new("Rectangle", None);
    child.properties.insert("x", Length(8.0).into());
    child.properties.insert("y", Length(4.0).into());
    child.properties.insert("width", Length(24.0).into());
    child.properties.insert("height", Length(18.0).into());
    child
        .properties
        .insert("background", Color(0x33dd_6677).into());
    scene.children.push(child);
    for (color, opacity, radius, viewport) in [
        (Color(0x2255_88ff), 1.0, 0.0, Size::new(80.0, 48.0)),
        (Color(0x2255_8880), 1.0, 0.0, Size::new(80.0, 48.0)),
        (Color(0x2255_88ff), 0.6, 0.0, Size::new(80.0, 48.0)),
        (Color(0x2255_88ff), 1.0, 10.0, Size::new(80.0, 48.0)),
        (Color(0x2255_88ff), 1.0, 0.0, Size::new(60.0, 40.0)),
    ] {
        scene.properties.insert("opacity", opacity.into());
        scene
            .properties
            .insert("border-radius", Length(radius).into());
        scene
            .properties
            .insert("background", Brush::Solid(color).into());
        let mut layout = renderer.layout_with_input(&scene, viewport, &input);
        let solid = renderer
            .snapshot_prepared(&scene, layout.clone(), 160, 96, 2.0, &input)
            .unwrap();

        // Clipping one device-pixel column makes the same brush use the
        // regular painter and leaves the rest of the frame comparable.
        layout.clip.width = layout.clip.width.min(79.5);
        let reference = renderer
            .snapshot_prepared(&scene, layout, 160, 96, 2.0, &input)
            .unwrap();
        let backdrop = xrack_renderer_femtovg::Theme::default()
            .window
            .0
            .to_be_bytes();
        assert_eq!(reference.pixel(159, 50), Some(backdrop));
        for (actual, expected) in solid
            .rgba
            .chunks_exact(640)
            .zip(reference.rgba.chunks_exact(640))
        {
            assert_eq!(&actual[..636], &expected[..636]);
        }
        if opacity == 1.0 && color.0 & 0xff == 0xff && radius == 0.0 && viewport.width == 80.0 {
            assert_eq!(solid.pixel(159, 50), Some(color.0.to_be_bytes()));
        }
    }
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn outline_labels_batch_around_clipped_labels_with_matching_pixels() {
    use xrack_runtime::{Color, Length, Node, TextHorizontalAlignment, TextVerticalAlignment};

    let mut renderer = pollster::block_on(Renderer::with_text(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
        xrack_renderer_femtovg::TextSystem::bundled_only(),
    ))
    .unwrap();
    let mut scene = Node::new("Window", None);
    scene.fonts = BundledFontsDemo::new().render().fonts;
    scene
        .properties
        .insert("default-font-family", "XRack Demo".to_owned().into());
    for (x, width) in [(4.0, 20.0), (28.0, 4.0), (52.0, 20.0), (76.0, 20.0)] {
        let mut label = Node::new("Label", None);
        for (name, value) in [
            ("x", x),
            ("y", 4.0),
            ("width", width),
            ("height", 24.0),
            ("font-size", 8.0),
        ] {
            label.properties.insert(name, Length(value).into());
        }
        label.properties.insert(
            "horizontal-alignment",
            TextHorizontalAlignment::Center.into(),
        );
        label
            .properties
            .insert("vertical-alignment", TextVerticalAlignment::Center.into());
        label.properties.insert("color", Color(0x70db_57cc).into());
        label.properties.insert("text", "WW".to_owned().into());
        scene.children.push(label);
    }
    let input = Interaction::default();
    for (x, runs) in [(76.0, 3), (28.0, 2)] {
        for (scale, shift) in [
            (1.0, 0.0),
            (1.25, 0.0),
            (2.0, 0.0),
            (1.3, 0.125),
            (2.0, 0.125),
        ] {
            for (label, x) in scene.children.iter_mut().zip([4.0, 28.0, 52.0, x]) {
                label.properties.insert("x", Length(x + shift).into());
            }
            let (width, height) = ((100.0 * scale) as u32, (32.0 * scale) as u32);
            renderer.set_display_batching_enabled(true);
            let before = renderer.display_batch_stats();
            let batched = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            let after = renderer.display_batch_stats();
            assert_eq!(
                after.glyph_batches - before.glyph_batches,
                1,
                "scale {scale}, shift {shift}, x {x}"
            );
            assert_eq!(
                after.text_runs - before.text_runs,
                runs,
                "scale {scale}, shift {shift}, x {x}"
            );
            renderer.set_display_batching_enabled(false);
            let direct = renderer
                .snapshot(&scene, width, height, scale, &input)
                .unwrap();
            assert_same_pixels(&batched, &direct);
        }
    }
}

#[test]
fn retained_label_layout_matches_fresh_rendering_after_text_style_and_size_changes() {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, LayoutDirection, Length, Node, TextOverflow, TextWrap};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut renderer = Renderer::with_device(gpu.clone()).unwrap();
    let input = Interaction::default();
    let mut scene = Node::new("Window", None);
    let mut label = Node::new("Label", None);
    label.properties.insert("width", Length(54.0).into());
    label.properties.insert("height", Length(28.0).into());
    label.properties.insert("font-size", Length(11.0).into());
    label
        .properties
        .insert("text", "A title with overflow".to_owned().into());
    label
        .properties
        .insert("overflow", TextOverflow::Elide.into());
    scene.children.push(label);

    let layout = renderer.layout_with_input(&scene, Size::new(80.0, 50.0), &input);
    let first = renderer
        .snapshot_prepared(&scene, layout.clone(), 160, 100, 2.0, &input)
        .unwrap();
    scene.children[0]
        .properties
        .insert("text", "New words change the ellipsis".to_owned().into());
    let changed = renderer
        .snapshot_prepared(&scene, layout, 160, 100, 2.0, &input)
        .unwrap();
    assert_ne!(first.rgba, changed.rgba);
    let mut fresh = Renderer::with_device(gpu.clone()).unwrap();
    let expected = fresh.snapshot(&scene, 160, 100, 2.0, &input).unwrap();
    assert_same_pixels(&changed, &expected);
    scene.children[0]
        .properties
        .insert("color", Color(0xffbe_52ff).into());
    let tinted = renderer.snapshot(&scene, 160, 100, 2.0, &input).unwrap();
    assert_ne!(changed.rgba, tinted.rgba);
    assert_same_pixels(
        &tinted,
        &fresh.snapshot(&scene, 160, 100, 2.0, &input).unwrap(),
    );

    for (width, wrap, direction, scale, bold) in [
        (
            54.0,
            TextWrap::WordWrap,
            LayoutDirection::LeftToRight,
            1.0,
            false,
        ),
        (
            38.0,
            TextWrap::WordWrap,
            LayoutDirection::LeftToRight,
            1.0,
            false,
        ),
        (
            54.0,
            TextWrap::NoWrap,
            LayoutDirection::RightToLeft,
            1.0,
            false,
        ),
        (
            54.0,
            TextWrap::NoWrap,
            LayoutDirection::RightToLeft,
            1.2,
            true,
        ),
    ] {
        let label = &mut scene.children[0];
        label.properties.insert("width", Length(width).into());
        label.properties.insert("wrap", wrap.into());
        scene
            .properties
            .insert("layout-direction", direction.into());
        renderer.set_text_scale(scale);
        renderer.set_system_bold_text(bold);
        let mut fresh = Renderer::with_device(gpu.clone()).unwrap();
        fresh.set_text_scale(scale);
        fresh.set_system_bold_text(bold);
        let expected = fresh.snapshot(&scene, 160, 100, 2.0, &input).unwrap();
        let actual = renderer.snapshot(&scene, 160, 100, 2.0, &input).unwrap();
        assert_same_pixels(&actual, &expected);
    }
}

#[test]
fn distinct_pngs_share_an_image_batch_without_changing_pixels() {
    use xrack_runtime::{
        Color, Image, ImageFit, ImageFormat, ImageRendering, Length, LoadedImage, Node, Orientation,
    };

    // Two 3×5 PNGs with distinct colors and alternating alpha levels.
    const RED: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 5, 8, 6,
        0, 0, 0, 128, 113, 86, 162, 0, 0, 0, 31, 73, 68, 65, 84, 120, 156, 99, 120, 38, 233, 246,
        31, 136, 29, 128, 120, 21, 3, 140, 1, 18, 100, 128, 49, 64, 130, 12, 56, 149, 1, 0, 164,
        164, 28, 153, 64, 188, 218, 32, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    const BLUE: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 5, 8, 6,
        0, 0, 0, 128, 113, 86, 162, 0, 0, 0, 31, 73, 68, 65, 84, 120, 156, 99, 144, 171, 61, 251,
        31, 136, 29, 128, 120, 21, 3, 140, 1, 18, 100, 128, 49, 64, 130, 12, 56, 149, 1, 0, 212,
        176, 30, 166, 15, 240, 97, 2, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut renderer = Renderer::with_device(gpu.clone()).unwrap();
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0x241e_34ff).into());
    for (x, bytes) in [(3.0, RED), (8.0, BLUE), (16.0, RED)] {
        let mut image = Node::new("Image", None);
        let asset = Image::Loaded(LoadedImage::new(ImageFormat::Png, bytes.to_vec(), [3, 5]));
        image.properties.insert("source", asset.into());
        image.properties.insert("x", Length(x).into());
        image.properties.insert("y", Length(2.0).into());
        image.properties.insert("width", Length(9.0).into());
        image.properties.insert("height", Length(15.0).into());
        scene.children.push(image);
    }
    // Keep the differently sampled image outside the linear batch.
    scene.children[2]
        .properties
        .insert("image-rendering", ImageRendering::Pixelated.into());
    let input = Interaction::default();
    let compare = |renderer: &mut Renderer,
                   scene: &Node,
                   input: &Interaction,
                   [width, height]: [u32; 2],
                   quads: u64| {
        let before = renderer.display_batch_stats();
        renderer.set_display_batching_enabled(true);
        let batched = renderer.snapshot(scene, width, height, 2.0, input).unwrap();
        let after = renderer.display_batch_stats();
        assert_eq!(
            after.image_batches - before.image_batches,
            u64::from(quads > 0)
        );
        assert_eq!(after.images - before.images, quads);
        renderer.set_display_batching_enabled(false);
        let direct = renderer.snapshot(scene, width, height, 2.0, input).unwrap();
        assert_eq!(batched.layout, direct.layout);
        assert_same_pixels(&batched, &direct);
        batched
    };
    for x in [3.0, 2.0] {
        scene.children[0].properties.insert("x", Length(x).into());
        compare(&mut renderer, &scene, &input, [56, 40], 2);
    }

    // An inactive conditional slot is retained in the scene for routing.
    let mut sparse = scene.clone();
    sparse.children.insert(1, Node::hidden());
    compare(&mut renderer, &sparse, &input, [56, 40], 2);

    let mut grouped = scene.clone();
    for child in &mut grouped.children {
        let mut cell = Node::new("Rectangle", None);
        cell.properties.insert("width", Length(28.0).into());
        cell.properties.insert("height", Length(20.0).into());
        cell.children.push(child.clone());
        let mut wrapper = cell.clone();
        wrapper.children = vec![cell];
        *child = wrapper;
    }
    compare(&mut renderer, &grouped, &input, [56, 40], 2);

    // Reject the second group after finding its first image. The first
    // group's queued image must keep its place and the second must paint once.
    let mut decoration = Node::new("Rectangle", None);
    decoration.properties.insert("x", Length(6.0).into());
    decoration.properties.insert("y", Length(8.0).into());
    decoration.properties.insert("width", Length(10.0).into());
    decoration.properties.insert("height", Length(4.0).into());
    decoration
        .properties
        .insert("border-width", Length(1.0).into());
    decoration
        .properties
        .insert("background", Color(0x40eb_65a0).into());
    grouped.children[1].children.push(decoration);
    compare(&mut renderer, &grouped, &input, [56, 40], 0);

    let mut mixed = scene.clone();
    for (index, child) in mixed.children.iter_mut().enumerate() {
        let mut cell = Node::new("Rectangle", None);
        cell.properties
            .insert("x", Length(index as f64 * 18.0).into());
        cell.properties.insert("width", Length(16.0).into());
        cell.properties.insert("height", Length(20.0).into());
        let mut background = Node::new("Rectangle", None);
        background.properties.insert("width", Length(16.0).into());
        background.properties.insert("height", Length(20.0).into());
        background
            .properties
            .insert("background", Color(0x38b1_a9aa).into());
        let mut image = child.clone();
        image.properties.insert("x", Length(3.0).into());
        cell.children = vec![background, image];
        *child = cell;
    }
    compare(&mut renderer, &mixed, &input, [108, 40], 2);
    let mut two_rows = mixed.clone();
    two_rows.children.truncate(2);
    for child in &mixed.children[..2] {
        let mut next_row = child.clone();
        next_row.properties.insert("y", Length(20.0).into());
        two_rows.children.push(next_row);
    }
    compare(&mut renderer, &two_rows, &input, [108, 80], 4);

    let mut labeled = two_rows.clone();
    for cell in &mut labeled.children {
        let mut label = Node::new("Label", None);
        for (name, value) in [
            ("x", 2.0),
            ("y", 9.0),
            ("width", 11.0),
            ("height", 8.0),
            ("font-size", 7.0),
        ] {
            label.properties.insert(name, Length(value).into());
        }
        label
            .properties
            .insert("text", "A clipped title".to_owned().into());
        label.properties.insert("color", Color(0xffe3_73ff).into());
        cell.children.push(label);
    }
    compare(&mut renderer, &labeled, &input, [108, 80], 4);
    let mut fractional = labeled.clone();
    for (index, cell) in fractional.children.iter_mut().enumerate() {
        let x = (index % 2) as f64 * 18.0 + if index % 2 == 0 { 0.125 } else { 0.0 };
        cell.properties.insert("x", Length(x).into());
    }
    // At 2×, quarter-pixel edges miss the AA fringe.
    compare(&mut renderer, &fractional, &input, [108, 80], 4);

    let mut coverage = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    for (phase, (scale, [shift_x, shift_y])) in [
        (2.0, [0.25, 0.25]),
        (2.0, [0.268, 0.234]),
        (2.0, [0.21, 0.281]),
        (1.0, [0.5, 0.5]),
        (1.25, [0.421875, 0.3]),
    ]
    .into_iter()
    .enumerate()
    {
        for (index, cell) in fractional.children.iter_mut().enumerate() {
            let x = (index % 2) as f64 * 18.0 + shift_x;
            let y = (index / 2) as f64 * 20.0 + shift_y;
            cell.properties.insert("x", Length(x).into());
            cell.properties.insert("y", Length(y).into());
        }
        if phase == 2 {
            fractional.children[1]
                .properties
                .insert("x", Length(5.268).into());
        }
        let width = (54.0 * scale) as u32;
        let height = (42.0 * scale) as u32;
        coverage.set_display_batching_enabled(true);
        let before = coverage.display_batch_stats();
        let actual = coverage
            .snapshot(&fractional, width, height, scale, &input)
            .unwrap();
        let after = coverage.display_batch_stats();
        if phase != 2 {
            assert_eq!(
                after.image_batches - before.image_batches,
                1,
                "phase {phase}"
            );
            assert_eq!(after.images - before.images, 4, "phase {phase}");
        }
        coverage.set_display_batching_enabled(false);
        let expected = coverage
            .snapshot(&fractional, width, height, scale, &input)
            .unwrap();
        assert_eq!(expected.layout, actual.layout);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "phase {phase}, pixel ({}, {}), channel {}: {actual} vs {expected}",
                index / 4 % width as usize,
                index / 4 / width as usize,
                index % 4
            );
        }
    }
    labeled.children.truncate(2);
    labeled.children[1]
        .properties
        .insert("x", Length(6.0).into());
    compare(&mut renderer, &labeled, &input, [108, 40], 0);

    // The second cell's background now covers part of the first image.
    mixed.children[1].properties.insert("x", Length(6.0).into());
    compare(&mut renderer, &mixed, &input, [108, 40], 0);

    let mut scrolled = scene.clone();
    let mut view = Node::new("Flickable", None);
    for (name, length) in [
        ("x", 3.0),
        ("y", 4.0),
        ("width", 28.0),
        ("height", 12.0),
        ("content-height", 48.0),
    ] {
        view.properties.insert(name, Length(length).into());
    }
    view.children = std::mem::take(&mut scrolled.children);
    view.children[0]
        .properties
        .insert("width", Length(12.0).into());
    view.children[0]
        .properties
        .insert("image-fit", ImageFit::Cover.into());
    scrolled.children.push(view);
    let mut scroll_input = Interaction::default();
    let mut frame = compare(&mut renderer, &scrolled, &scroll_input, [72, 48], 2);
    for (offset, quads) in [(4.0, 2), (5.5, 2), (5.25, 0)] {
        assert!(scroll_input.accessibility_scroll_to(
            &frame.layout,
            &[0],
            Orientation::Vertical,
            offset
        ));
        let next = compare(&mut renderer, &scrolled, &scroll_input, [72, 48], quads);
        assert_ne!(frame.rgba, next.rgba);
        frame = next;
    }

    // A scaled subtree has a projected ancestor clip as well as a different
    // canvas transform. The PNGs align to device pixels in both axes.
    let mut grid = Node::new("Rectangle", None);
    for (name, length) in [
        ("x", 3.0),
        ("y", 2.0),
        ("width", 28.0),
        ("height", 20.0),
        ("transform-origin-x", 0.0),
        ("transform-origin-y", 0.0),
    ] {
        grid.properties.insert(name, Length(length).into());
    }
    grid.properties.insert("transform-scale-x", 2.0_f64.into());
    grid.properties.insert("transform-scale-y", 1.5_f64.into());
    grid.children = std::mem::take(&mut scene.children);
    scene.children.push(grid);
    let square = compare(&mut renderer, &scene, &input, [128, 72], 2);

    scene.children[0].properties.insert("clip", true.into());
    scene.children[0]
        .properties
        .insert("border-radius", Length(10.0).into());
    let rounded = compare(&mut renderer, &scene, &input, [128, 72], 2);
    assert_ne!(square.rgba, rounded.rgba);

    // Fractional device edges and rotated parents keep the path painter's AA.
    scene.children[0]
        .properties
        .insert("x", Length(3.25).into());
    compare(&mut renderer, &scene, &input, [128, 72], 0);
    scene.children[0]
        .properties
        .insert("transform-rotation", 20.0_f64.into());
    compare(&mut renderer, &scene, &input, [128, 72], 0);

    // A larger image-only group can submit the atlas as instances. The
    // overlapping alpha edges, clip, and sibling order must still match.
    const INSTANCES: usize = 64;
    let grid = &mut scene.children[0];
    grid.properties.remove("transform-rotation");
    grid.properties.remove("border-radius");
    grid.properties.insert("width", Length(64.0).into());
    grid.properties.insert("height", Length(84.0).into());
    let templates = grid.children[..2].to_vec();
    grid.children = (0..INSTANCES)
        .map(|index| {
            let mut image = templates[index % 2].clone();
            image
                .properties
                .insert("x", Length((index % 8 * 8) as f64).into());
            image
                .properties
                .insert("y", Length((index / 8 * 10) as f64).into());
            image
                .properties
                .insert("z", ((INSTANCES - index) as f64).into());
            image
        })
        .collect();
    let full_upload = INSTANCES * 9 * size_of::<f32>();
    for (x, upload) in [
        (3.0, Some(full_upload)),
        (3.0, Some(0)),
        (3.25, None),
        (7.0, Some(full_upload)),
    ] {
        scene.children[0].properties.insert("x", Length(x).into());
        renderer.set_display_batching_enabled(true);
        renderer.set_cached_layer_compositing_enabled(true);
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(renderer.snapshot(&scene, 276, 266, 2.0, &input).unwrap());
        })
        .unwrap();
        if let Some(upload) = upload {
            assert_eq!(work.quad_batches, 1, "x={x}");
            assert_eq!(work.vertex_bytes, upload, "x={x}");
        } else {
            assert_ne!(work.vertex_bytes, full_upload, "x={x}");
        }
        renderer.set_cached_layer_compositing_enabled(false);
        let expected = renderer.snapshot(&scene, 276, 266, 2.0, &input).unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        for (offset, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "x={x} channel {offset}: {actual} vs {expected}"
            );
        }
    }

    let mut view = Node::new("Flickable", None);
    for (name, length) in [
        ("x", 3.0),
        ("y", 4.0),
        ("width", 66.0),
        ("height", 83.0),
        ("content-height", 120.0),
        ("transform-origin-x", 0.0),
        ("transform-origin-y", 0.0),
    ] {
        view.properties.insert(name, Length(length).into());
    }
    view.properties.insert("transform-scale", 1.0_f64.into());
    for index in 0..96 {
        let mut cell = Node::new("Rectangle", None);
        for (name, length) in [
            ("x", (index % 8 * 8) as f64),
            ("y", (index / 8 * 8) as f64),
            ("width", 10.0),
            ("height", 16.0),
        ] {
            cell.properties.insert(name, Length(length).into());
        }
        cell.properties.insert("z", ((96 - index) as f64).into());
        let mut image = templates[index % 2].clone();
        image.properties.remove("x");
        image.properties.remove("y");
        if index % 3 == 0 {
            let mut group = Node::new("Rectangle", None);
            group.properties.insert("width", Length(10.0).into());
            group.properties.insert("height", Length(16.0).into());
            group.children.push(image);
            cell.children.push(group);
        } else {
            cell.children.push(image);
        }
        view.children.push(cell);
    }
    scene.children[0] = view;
    let mut scroll_input = Interaction::default();
    let mut layout = renderer.layout_with_input(&scene, Size::new(72.0, 92.0), &scroll_input);
    let retained = renderer
        .device
        .features()
        .contains(wgpu::Features::IMMEDIATES);
    for (offset, native, edited, reused) in [
        (0.0, true, false, false),
        (7.0, true, false, false),
        (7.25, false, false, false),
        (12.0, true, false, true),
        (14.0, true, true, false),
        (16.0, true, false, false),
        (18.0, true, false, true),
    ] {
        if edited {
            scene.children[0].children[67].children[0] = templates[0].clone();
            let image = &mut scene.children[0].children[67].children[0];
            image.properties.remove("x");
            image.properties.remove("y");
        }
        if offset > 0.0 {
            assert!(scroll_input.accessibility_scroll_to(
                &layout,
                &[0],
                Orientation::Vertical,
                offset
            ));
        }
        renderer.set_cached_layer_compositing_enabled(true);
        let before = renderer.display_batch_stats();
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(
                renderer
                    .snapshot(&scene, 144, 184, 2.0, &scroll_input)
                    .unwrap(),
            );
        })
        .unwrap();
        let images = (renderer.display_batch_stats().images - before.images) as usize;
        let expected_bytes = if retained && reused {
            0
        } else {
            images * 9 * size_of::<f32>()
        };
        assert_eq!(
            images >= INSTANCES && work.vertex_bytes == expected_bytes,
            native,
            "offset={offset}, images={images}, vertex bytes={}",
            work.vertex_bytes
        );
        renderer.set_cached_layer_compositing_enabled(false);
        let expected = renderer
            .snapshot(&scene, 144, 184, 2.0, &scroll_input)
            .unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        for (offset, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "scroll channel {offset}: {actual} vs {expected}"
            );
        }
        layout = actual.layout;
    }

    // A caller can submit a different clip with the same scene snapshot.
    let image = &mut layout.children[0].children[16].children[0];
    image.clip = image.clip.intersect(xrack_layout::Rect::new(
        image.bounds.x,
        image.bounds.y,
        4.5,
        image.bounds.height,
    ));
    let expected = renderer
        .snapshot_prepared(&scene, layout.clone(), 144, 184, 2.0, &scroll_input)
        .unwrap();
    renderer.set_cached_layer_compositing_enabled(true);
    let actual = renderer
        .snapshot_prepared(&scene, layout, 144, 184, 2.0, &scroll_input)
        .unwrap();
    assert!(
        actual
            .rgba
            .iter()
            .zip(&expected.rgba)
            .all(|(actual, expected)| actual.abs_diff(*expected) <= 1),
        "the supplied clip must invalidate the retained batch",
    );

    for (index, cell) in scene.children[0].children.iter_mut().enumerate() {
        let tint = if index % 2 == 0 {
            Color(0x73a9_c58c)
        } else {
            Color(0x2d62_9aff)
        };
        cell.properties.insert("background", tint.into());
    }
    let mut reference = Renderer::with_device(gpu).unwrap();
    reference.set_cached_layer_compositing_enabled(false);
    let mut layout = renderer.layout_with_input(&scene, Size::new(72.0, 92.0), &scroll_input);
    for (offset, expected_upload) in [(20.0, None), (22.0, Some(192)), (24.0, Some(0))] {
        assert!(
            scroll_input.accessibility_scroll_to(&layout, &[0], Orientation::Vertical, offset,)
        );
        renderer.set_cached_layer_compositing_enabled(true);
        let before = renderer.display_batch_stats().images;
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(
                renderer
                    .snapshot(&scene, 144, 184, 2.0, &scroll_input)
                    .unwrap(),
            );
        })
        .unwrap();
        if let Some(quads) = expected_upload.filter(|_| retained) {
            assert_eq!(renderer.display_batch_stats().images - before, 96);
            assert_eq!(work.quad_batches, 1);
            assert_eq!(work.vertex_bytes, quads * 9 * size_of::<f32>());
        }
        let expected = reference
            .snapshot(&scene, 144, 184, 2.0, &scroll_input)
            .unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        for (channel, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "colored cells scroll={offset}, channel {channel}: {actual} vs {expected}",
            );
        }
        layout = actual.layout;
    }

    // Surround the scrolling grid with layers on both sides in paint order.
    for (x, y, width, height, z, tint) in [
        (0.0, 0.0, 72.0, 92.0, -1.0, Color(0x4356_6aff)),
        (56.0, 8.0, 16.0, 64.0, 1.0, Color(0xe9cd_8c80)),
    ] {
        let mut panel = Node::new("Rectangle", None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            panel.properties.insert(name, Length(value).into());
        }
        panel.properties.insert("z", z.into());
        panel.properties.insert("background", tint.into());
        scene.children.push(panel);
    }
    let mut title = Node::new("Label", None);
    title.properties.insert("width", Length(70.0).into());
    title.properties.insert("height", Length(20.0).into());
    title.properties.insert("z", 2.0_f64.into());
    title.properties.insert("text", "Images".to_owned().into());
    scene.children.push(title);
    for offset in [26.0, 28.0, 30.0, 32.0, 34.0, 35.0, 36.0, 37.0] {
        if offset == 35.0 {
            let mut wrapper = Node::new("Rectangle", None);
            wrapper.properties.insert("width", Length(72.0).into());
            wrapper.properties.insert("height", Length(92.0).into());
            wrapper.children = std::mem::take(&mut scene.children);
            scene.children.push(wrapper);
            layout = renderer.layout_with_input(&scene, Size::new(72.0, 92.0), &scroll_input);
        }
        let path: &[usize] = if offset >= 35.0 { &[0, 0] } else { &[0] };
        assert!(scroll_input.accessibility_scroll_to(&layout, path, Orientation::Vertical, offset));
        let before = renderer.display_batch_stats().images;
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(
                renderer
                    .snapshot(&scene, 144, 184, 2.0, &scroll_input)
                    .unwrap(),
            );
        })
        .unwrap();
        if retained && matches!(offset, 34.0 | 37.0) {
            assert_eq!(renderer.display_batch_stats().images - before, 96);
            assert_eq!(work.vertex_bytes, 0);
        }
        let expected = reference
            .snapshot(&scene, 144, 184, 2.0, &scroll_input)
            .unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        assert!(
            actual
                .rgba
                .iter()
                .zip(&expected.rgba)
                .all(|(actual, expected)| actual.abs_diff(*expected) <= 1),
            "window layers must preserve paint order at scroll={offset}"
        );
        layout = actual.layout;
    }

    for (offset, underneath) in [(28.0, false), (29.0, false), (30.0, true), (31.0, true)] {
        let title = &mut scene.children[0].children[3];
        title
            .properties
            .insert("text", format!("Images {offset}").into());
        title
            .properties
            .insert("z", if underneath { -0.5_f64 } else { 2.0 }.into());
        assert!(scroll_input.accessibility_scroll_to(
            &layout,
            &[0, 0],
            Orientation::Vertical,
            offset,
        ));
        let before = renderer.display_batch_stats().images;
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(
                renderer
                    .snapshot(&scene, 144, 184, 2.0, &scroll_input)
                    .unwrap(),
            );
        })
        .unwrap();
        if retained {
            assert_eq!(renderer.display_batch_stats().images - before, 96);
            if offset == 29.0 {
                assert_eq!(work.vertex_bytes, 0);
            }
        }
        let expected = reference
            .snapshot(&scene, 144, 184, 2.0, &scroll_input)
            .unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        for (channel, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "changing title underneath={underneath}, scroll={offset}, channel {channel}",
            );
        }
        layout = actual.layout;
    }

    let left = scene.children[0].children[0].clone();
    let mut right = left.clone();
    right.properties.insert("x", Length(56.0).into());
    right.properties.insert("y", Length(10.0).into());
    right.children[26]
        .properties
        .insert("background", Color(0xba2e_4f93).into());
    let mut divider = Node::new("Rectangle", None);
    for (name, value) in [("x", 60.0), ("width", 6.0), ("height", 96.0)] {
        divider.properties.insert(name, Length(value).into());
    }
    divider
        .properties
        .insert("background", Color(0x172f_5eff).into());
    let mut window = Node::new("Window", None);
    window
        .properties
        .insert("background", Color(0xe5ea_f4ff).into());
    window.children.extend([left, divider, right]);
    let mut input = Interaction::default();
    let mut layout = renderer.layout_with_input(&window, Size::new(128.0, 96.0), &input);
    for (frame, (index, offset)) in [
        (0, 0.0),
        (0, 5.0),
        (2, 7.0),
        (0, 11.0),
        (2, 18.0),
        (0, 11.25),
        (0, 14.0),
        (2, 18.25),
        (2, 22.0),
        (0, 16.0),
    ]
    .into_iter()
    .enumerate()
    {
        if frame == 9 {
            let source = templates[1].properties.get("source").unwrap().clone();
            window.children[0].children[26].children[0]
                .properties
                .insert("source", source);
        }
        if frame != 0 {
            assert!(input.accessibility_scroll_to(
                &layout,
                &[index],
                Orientation::Vertical,
                offset
            ));
        }
        let before = renderer.display_batch_stats().images;
        let mut actual = None;
        let work = xrack_runtime::update_profile::measure(|| {
            actual = Some(renderer.snapshot(&window, 256, 192, 2.0, &input).unwrap());
        })
        .unwrap();
        if retained && matches!(frame, 2 | 3 | 4) {
            assert_eq!(renderer.display_batch_stats().images - before, 192);
            assert_eq!(work.vertex_bytes, 0, "one pane scrolled on frame {frame}");
        }
        if retained && matches!(frame, 6 | 8) {
            assert_eq!(renderer.display_batch_stats().images - before, 192);
            assert!(
                work.geometry_reuses >= 2,
                "panes lost their buffers on frame {frame}"
            );
        }
        if retained && matches!(frame, 5 | 7 | 9) {
            assert!(
                work.geometry_reuses >= 1,
                "stable pane lost its buffer on frame {frame}"
            );
            assert_eq!(work.gpu_submissions, if frame == 7 { 1 } else { 2 });
        }
        let expected = reference.snapshot(&window, 256, 192, 2.0, &input).unwrap();
        let actual = actual.unwrap();
        assert_eq!(actual.layout, expected.layout);
        for (channel, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "split view frame {frame}, channel {channel}"
            );
        }
        layout = actual.layout;
    }
}

#[test]
fn full_image_atlas_recycles_retired_tiles_without_overwriting_current_draws() {
    use xrack_runtime::{Color, Image, ImageFormat, Length, LoadedImage, Node};

    // Solid 64×64 grayscale PNGs. The atlas fits 15×15 tiles including gutters.
    const DARK: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 64, 0, 0, 0, 64, 8,
        0, 0, 0, 0, 143, 2, 46, 2, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 204, 65, 17, 0, 0,
        12, 2, 32, 131, 216, 63, 167, 33, 246, 219, 65, 0, 210, 163, 8, 4, 2, 129, 64, 32, 16, 8,
        4, 2, 129, 64, 32, 248, 30, 12, 229, 136, 192, 16, 151, 126, 19, 123, 0, 0, 0, 0, 73, 69,
        78, 68, 174, 66, 96, 130,
    ];
    const LIGHT: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 64, 0, 0, 0, 64, 8,
        0, 0, 0, 0, 143, 2, 46, 2, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 204, 65, 17, 0, 0,
        12, 2, 32, 251, 167, 52, 138, 33, 246, 219, 65, 0, 210, 163, 8, 4, 2, 129, 64, 32, 16, 8,
        4, 2, 129, 64, 32, 248, 30, 12, 186, 218, 0, 211, 0, 109, 180, 183, 0, 0, 0, 0, 73, 69, 78,
        68, 174, 66, 96, 130,
    ];
    const SIDE: usize = 15;
    const TILES: usize = SIDE * SIDE;
    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut renderer = Renderer::with_device(gpu.clone()).unwrap();
    let mut direct = Renderer::with_device(gpu).unwrap();
    direct.set_display_batching_enabled(false);
    let asset =
        |bytes: &[u8]| Image::Loaded(LoadedImage::new(ImageFormat::Png, bytes.to_vec(), [64, 64]));
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xffff_ffff).into());
    for index in 0..TILES {
        let mut tile = Node::new("Image", None);
        tile.properties
            .insert("x", Length((index % SIDE * 6) as f64).into());
        tile.properties
            .insert("y", Length((index / SIDE * 6) as f64).into());
        tile.properties.insert("width", Length(4.0).into());
        tile.properties.insert("height", Length(4.0).into());
        tile.properties.insert("source", asset(DARK).into());
        scene.children.push(tile);
    }

    let input = Interaction::default();
    for frame in 0..3 {
        if frame > 0 {
            // Interleave new content with cached tiles so each later upload
            // happens while earlier atlas quads are still queued for drawing.
            let pixels = if frame == 1 { LIGHT } else { DARK };
            for tile in scene.children.iter_mut().skip(frame % 2).step_by(2) {
                tile.properties.insert("source", asset(pixels).into());
            }
        }
        let before = renderer.display_batch_stats();
        let actual = renderer.snapshot(&scene, 180, 180, 2.0, &input).unwrap();
        let after = renderer.display_batch_stats();
        assert_eq!(
            after.image_batches - before.image_batches,
            1,
            "frame {frame}"
        );
        assert_eq!(after.images - before.images, TILES as u64, "frame {frame}");
        let expected = direct.snapshot(&scene, 180, 180, 2.0, &input).unwrap();
        assert_same_pixels(&actual, &expected);
    }
}

#[test]
#[ignore = "manual GPU measurement; XRACK_IMAGE_BENCH_ASSETS contains numbered 16×16 PNGs"]
fn benchmark_moving_image_grid() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        path::PathBuf,
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_runtime::{
        Color, Component, Image, ImageFormat, Length, LoadedImage, Node, Orientation,
    };

    struct GridScene(Rc<Node>);
    impl Component for GridScene {
        fn render(&self) -> Node {
            self.0.as_ref().clone()
        }

        fn render_shared(&self) -> Rc<Node> {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let assets = PathBuf::from(
        std::env::var_os("XRACK_IMAGE_BENCH_ASSETS").ok_or("set XRACK_IMAGE_BENCH_ASSETS")?,
    );
    let frame_period = std::env::var("XRACK_IMAGE_BENCH_PERIOD_US")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .map_or(Duration::ZERO, Duration::from_micros);
    let asynchronous = std::env::var_os("XRACK_IMAGE_BENCH_ASYNC").is_some();
    const TILES: usize = 600;
    const FRAMES: usize = 70;
    let scaled = std::env::var_os("XRACK_IMAGE_BENCH_SCALED").is_some();
    let scrolling = std::env::var_os("XRACK_IMAGE_BENCH_SCROLL").is_some();
    let chrome = scrolling && std::env::var_os("XRACK_IMAGE_BENCH_CHROME").is_some();
    let split = scrolling && std::env::var_os("XRACK_IMAGE_BENCH_SPLIT").is_some();
    let split_churn = split && std::env::var_os("XRACK_IMAGE_BENCH_SPLIT_CHURN").is_some();
    let split_fractional =
        split && std::env::var_os("XRACK_IMAGE_BENCH_SPLIT_FRACTIONAL").is_some();
    let dynamic_title = chrome && std::env::var_os("XRACK_IMAGE_BENCH_DYNAMIC_TITLE").is_some();
    let hidden_slots = std::env::var_os("XRACK_IMAGE_BENCH_HIDDEN").is_some();
    let labeled = std::env::var_os("XRACK_IMAGE_BENCH_LABELED").is_some();
    let label_room = std::env::var_os("XRACK_IMAGE_BENCH_LABEL_ROOM").is_some();
    let subpixel = std::env::var_os("XRACK_IMAGE_BENCH_SUBPIXEL");
    let continuous = subpixel.as_deref().is_some_and(|mode| mode == "continuous");
    let subpixel = subpixel.is_some();
    let labels_only = labeled && std::env::var_os("XRACK_IMAGE_BENCH_LABELS_ONLY").is_some();
    let mixed = labeled || std::env::var_os("XRACK_IMAGE_BENCH_MIXED").is_some();
    let grouped = mixed || std::env::var_os("XRACK_IMAGE_BENCH_GROUPED").is_some();
    let prepare_scene = std::env::var_os("XRACK_IMAGE_BENCH_PREPARED").is_some();
    let factor = if scaled { 2.0 } else { 1.0 };
    // Keep the first tile visible and scroll by 100 images each frame. This
    // requires 7,500 assets and eventually fills the atlas with retired images.
    let advance = if std::env::var_os("XRACK_IMAGE_BENCH_CHURN").is_some() {
        100
    } else {
        0
    };
    let images = (0..TILES + (FRAMES - 1) * advance)
        .map(|index| {
            let bytes = std::fs::read(assets.join(format!("{index}.png")))?;
            Ok(Image::Loaded(LoadedImage::new(
                ImageFormat::Png,
                bytes,
                [16, 16],
            )))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let viewport = Size::new(
        (if chrome { 744.0 } else { 640.0 }) + if split { 640.0 } else { 0.0 },
        if chrome { 524.0 } else { 480.0 },
    );
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut input = Interaction::default();
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Moving image grid benchmark"),
        size: wgpu::Extent3d {
            width: (viewport.width * 2.0) as u32,
            height: (viewport.height * 2.0) as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xffff_ffff).into());
    if scaled || scrolling {
        let mut grid = Node::new(if scrolling { "Flickable" } else { "Rectangle" }, None);
        for (name, value) in [
            ("width", 640.0 / factor),
            ("height", if scrolling { 350.0 } else { 480.0 } / factor),
            ("transform-origin-x", 0.0),
            ("transform-origin-y", 0.0),
        ] {
            grid.properties.insert(name, Length(value).into());
        }
        if scrolling {
            grid.properties
                .insert("content-height", Length(480.0 / factor).into());
        }
        if chrome {
            grid.properties.insert("x", Length(104.0).into());
            grid.properties.insert("y", Length(44.0).into());
        }
        grid.properties.insert("transform-scale", factor.into());
        scene.children.push(grid);
    }
    let tiles = if scaled || scrolling {
        &mut scene.children[0].children
    } else {
        &mut scene.children
    };
    for (index, asset) in images[..TILES].iter().enumerate() {
        let mut image = Node::new("Image", None);
        image.properties.insert("source", asset.clone().into());
        image
            .properties
            .insert("width", Length(16.0 / factor).into());
        image
            .properties
            .insert("height", Length(16.0 / factor).into());
        let mut tile = if grouped {
            let mut cell = Node::new("Rectangle", None);
            cell.properties
                .insert("width", Length(16.0 / factor).into());
            cell.properties
                .insert("height", Length(16.0 / factor).into());
            if mixed {
                let mut background = Node::new("Rectangle", None);
                background
                    .properties
                    .insert("width", Length(16.0 / factor).into());
                background
                    .properties
                    .insert("height", Length(16.0 / factor).into());
                background
                    .properties
                    .insert("background", Color(0x2038_6266).into());
                cell.children.push(background);
            }
            if !labels_only {
                cell.children.push(image);
            }
            if labeled {
                let mut label = Node::new("Label", None);
                let (y, height) = if label_room { (2.0, 14.0) } else { (8.0, 8.0) };
                for (name, value) in [
                    ("x", 1.0),
                    ("y", y),
                    ("width", 14.0),
                    ("height", height),
                    ("font-size", 7.0),
                ] {
                    label.properties.insert(name, Length(value / factor).into());
                }
                label
                    .properties
                    .insert("text", (index % 10).to_string().into());
                label.properties.insert("color", Color(0x1929_5aff).into());
                cell.children.push(label);
            }
            cell
        } else {
            image
        };
        if scrolling {
            let x = (index % 30) * 21 + 3;
            let y = (index / 30) * 22 + 3;
            tile.properties
                .insert("x", Length(x as f64 / factor).into());
            tile.properties
                .insert("y", Length(y as f64 / factor).into());
        }
        tiles.push(tile);
        if hidden_slots && index + 1 < TILES {
            tiles.push(Node::hidden());
        }
    }
    if chrome {
        for (title, y, width, height, tint) in [
            (
                "Images",
                0.0,
                f64::from(viewport.width),
                44.0,
                Color(0xe1e8_f4ff),
            ),
            ("Library", 44.0, 104.0, 480.0, Color(0xedf1_f7ff)),
        ] {
            let mut panel = Node::new("Rectangle", None);
            for (name, value) in [("y", y), ("width", width), ("height", height)] {
                panel.properties.insert(name, Length(value).into());
            }
            panel.properties.insert("background", tint.into());
            let mut label = Node::new("Label", None);
            for (name, value) in [
                ("x", 12.0),
                ("y", 10.0),
                ("width", 80.0),
                ("height", 24.0),
                ("font-size", 16.0),
            ] {
                label.properties.insert(name, Length(value).into());
            }
            label.properties.insert("color", Color(0x2432_46ff).into());
            label.properties.insert("text", title.to_owned().into());
            panel.children.push(label);
            scene.children.push(panel);
        }
    }
    let second_view = split.then(|| {
        let mut gallery = scene.children[0].clone();
        gallery
            .properties
            .insert("x", Length(if chrome { 744.0 } else { 640.0 }).into());
        let index = scene.children.len();
        scene.children.push(gallery);
        index
    });
    let mut grid = GridScene(Rc::new(if prepare_scene {
        scene.into_shared()
    } else {
        scene
    }));
    let mut layout = renderer.layout_with_input(&grid.0, viewport, &input);
    let mut samples: [Vec<Duration>; 6] = std::array::from_fn(|_| Vec::new());
    let wait = || wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    };
    let before = renderer.display_batch_stats();
    for phase in 0..FRAMES {
        let start = Instant::now();
        if dynamic_title {
            Rc::make_mut(&mut grid.0).children[1].children[0]
                .properties
                .insert("text", format!("Images {phase}").into());
        }
        if split_churn && !labels_only {
            let tile = &mut Rc::make_mut(&mut grid.0).children[0].children[TILES / 2];
            let image = if grouped {
                &mut tile.children[usize::from(mixed)]
            } else {
                tile
            };
            image
                .properties
                .insert("source", images[phase].clone().into());
        }
        if scrolling && phase > 0 {
            let (index, offset) = match second_view {
                Some(index) if phase % 2 == 1 => (index, phase.div_ceil(2) * 2),
                Some(_) => (0, (phase / 2) * 3),
                None => (0, (phase * 3) % 120),
            };
            assert!(input.accessibility_scroll_to(
                &layout,
                &[index],
                Orientation::Vertical,
                (offset as f32
                    + if split_fractional && index == 0 {
                        0.2375
                    } else {
                        0.0
                    })
                    / factor as f32,
            ));
        }
        if !scrolling || advance != 0 {
            let scene = Rc::make_mut(&mut grid.0);
            let tiles = if scaled || scrolling {
                &mut scene.children[0].children
            } else {
                &mut scene.children
            };
            for (index, tile) in tiles
                .iter_mut()
                .filter(|tile| tile.kind != "Empty")
                .enumerate()
            {
                if !scrolling {
                    let shift = if continuous {
                        (phase as f64 * 0.137 + index as f64 * 0.027).fract()
                    } else if subpixel {
                        ((phase + index) % 4) as f64 * 0.125
                    } else {
                        0.0
                    };
                    let x = ((index % 30) * 21 + 3 + (phase + index) % 5) as f64 + shift;
                    let y = ((index / 30) * 22 + 3 + (phase / 3 + index * 3) % 5) as f64 + shift;
                    tile.properties.insert("x", Length(x / factor).into());
                    tile.properties.insert("y", Length(y / factor).into());
                }
                if index != 0 && advance != 0 && !labels_only {
                    let image = if grouped {
                        &mut tile.children[usize::from(mixed)]
                    } else {
                        tile
                    };
                    image
                        .properties
                        .insert("source", images[phase * advance + index].clone().into());
                }
            }
        }
        let scene;
        (scene, layout) = if prepare_scene {
            renderer.prepare_scene(&grid, grid.render_shared(), viewport, &input)
        } else {
            (
                grid.render_shared(),
                renderer.layout_with_input(&grid.0, viewport, &input),
            )
        };
        let prepared = start.elapsed();
        renderer.draw_with_layout(&scene, &layout, &target, 2.0, &input)?;
        let submitted = start.elapsed();
        let completed = if asynchronous {
            None
        } else {
            renderer.device.poll(wait())?;
            Some(start.elapsed())
        };
        if phase == 0
            && let Some(completed) = completed
        {
            println!(
                "moving images first frame: submit={:.3}ms render={:.3}ms total={:.3}ms",
                (submitted - prepared).as_secs_f64() * 1000.0,
                (completed - prepared).as_secs_f64() * 1000.0,
                completed.as_secs_f64() * 1000.0,
            );
        }
        if phase >= 10 {
            for (series, elapsed) in
                samples
                    .iter_mut()
                    .zip([prepared, submitted - prepared, submitted])
            {
                series.push(elapsed);
            }
            if let Some(completed) = completed {
                for (series, elapsed) in samples[3..].iter_mut().zip([
                    completed - prepared,
                    completed - submitted,
                    completed,
                ]) {
                    series.push(elapsed);
                }
            }
        }
        if !frame_period.is_zero() {
            std::thread::sleep(frame_period.saturating_sub(start.elapsed()));
        }
    }
    if asynchronous {
        renderer.device.poll(wait())?;
    }
    let after = renderer.display_batch_stats();
    println!(
        "moving images: {} quads in {} batches over 70 frames",
        after.images - before.images,
        after.image_batches - before.image_batches
    );
    if labeled {
        println!(
            "moving labels: {} runs in {} glyph batches over 70 frames",
            after.text_runs - before.text_runs,
            after.glyph_batches - before.glyph_batches,
        );
    }
    for (stage, samples) in ["prepare", "submit", "cpu-total", "render", "wait", "total"]
        .into_iter()
        .zip(&mut samples)
    {
        if samples.is_empty() {
            continue;
        }
        samples.sort_unstable();
        println!(
            "moving images {stage}: p50={:.3}ms p95={:.3}ms",
            samples[29].as_secs_f64() * 1000.0,
            samples[56].as_secs_f64() * 1000.0,
        );
    }
    Ok(())
}

#[test]
fn moving_opacity_layers_match_fresh_rasters_and_refresh_after_changes()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_renderer = Renderer::with_device(gpu.clone())?;
    let mut interpreter_renderer = Renderer::with_device(gpu.clone())?;
    let mut fresh = Renderer::with_device(gpu)?;
    let input = Interaction::default();
    let mut aot = LayerMotion::new();
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("LayerMotion")?;

    for (offset, width, fade, edited, rasterized, reused) in [
        (8.0, 49.0, 0.61, false, true, false),
        (13.5, 49.0, 0.73, false, false, true),
        (21.0, 49.0, 0.56, false, false, true),
        (21.25, 49.0, 0.66, false, true, false),
        (24.25, 49.0, 0.58, false, false, true),
        (-4.0, 49.0, 0.75, false, true, false),
        (9.0, 49.0, 0.73, false, true, false),
        (10.0, 51.0, 0.61, false, false, false),
        (17.0, 51.0, 0.66, false, true, false),
        (20.0, 51.0, 0.66, true, false, false),
        (14.0, 51.0, 0.78, true, true, false),
    ] {
        let mobile = cfg!(any(target_os = "ios", target_os = "android"));
        let reused = reused || mobile && matches!(offset, 17.0 | 14.0);
        let rasterized = !reused && (mobile || rasterized);
        aot.set_offset(Length(offset));
        aot.set_card_width(Length(width));
        aot.set_fade(fade);
        aot.set_edited(edited);
        interpreter.set("offset", Length(offset).into())?;
        interpreter.set("card-width", Length(width).into())?;
        interpreter.set("fade", fade.into())?;
        interpreter.set("edited", edited.into())?;
        let viewport = Size::new(96.0, 72.0);
        let (scene, prepared) =
            aot_renderer.prepare_scene(&aot, aot.render_shared(), viewport, &input);
        let before = aot_renderer.opacity_layer_stats();
        let actual = aot_renderer.snapshot(&scene, 192, 144, 2.0, &input)?;
        assert_eq!(prepared, actual.layout);
        let after = aot_renderer.opacity_layer_stats();
        assert_eq!(
            after.reused - before.reused,
            u64::from(reused),
            "x={offset}"
        );
        assert_eq!(
            after.rasterized - before.rasterized,
            u64::from(rasterized),
            "x={offset}"
        );
        let (interpreted_scene, interpreted_layout) = interpreter_renderer.prepare_scene(
            &interpreter,
            interpreter.render_shared(),
            viewport,
            &input,
        );
        let interpreted =
            interpreter_renderer.snapshot(&interpreted_scene, 192, 144, 2.0, &input)?;
        assert_eq!(interpreted_layout, interpreted.layout);
        assert_eq!(
            interpreter_renderer.opacity_layer_stats().reused,
            after.reused
        );
        assert_same_pixels(&actual, &interpreted);

        fresh.set_layer_budget(32 * 1024 * 1024);
        let expected = fresh.snapshot(&scene, 192, 144, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        assert_same_pixels(&actual, &expected);
    }
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn groups_with_changing_pixel_phases_share_a_capture_and_retain_after_settling()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length, Node};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut compiled_renderer = Renderer::with_device(gpu.clone())?;
    let mut interpreted_renderer = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_opacity_atlas_enabled(false);
    let input = Interaction::default();
    let mut compiled = LayerMotion::new();
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("LayerMotion")?;

    let pair = |mut scene: Node| {
        let mut card = scene.children[0].clone();
        let x = f64::from(card.length("x").unwrap());
        card.properties.insert("x", Length(x + 22.0).into());
        card.properties.insert("y", Length(25.0).into());
        card.properties.insert("opacity", 0.72_f64.into());
        scene.children.push(card);
        scene
    };

    for (frame, x) in [8.0, 8.25, 8.375, 8.0625, 8.4375, 8.4375, 8.9375]
        .into_iter()
        .enumerate()
    {
        let offset = Length(x);
        compiled.set_offset(offset);
        interpreter.set("offset", offset.into())?;
        let scene = pair(compiled.render());
        let interpreted_scene = pair(interpreter.render());
        let before_layers = compiled_renderer.opacity_layer_stats();
        let before_atlas = compiled_renderer.opacity_atlas_stats();
        let actual = compiled_renderer.snapshot(&scene, 192, 144, 2.0, &input)?;
        let native = interpreted_renderer.snapshot(&interpreted_scene, 192, 144, 2.0, &input)?;
        assert_same_pixels(&actual, &native);
        let after_layers = compiled_renderer.opacity_layer_stats();
        let after_atlas = compiled_renderer.opacity_atlas_stats();
        match frame {
            2..=4 => {
                assert_eq!(after_layers.rasterized - before_layers.rasterized, 0);
                assert_eq!(after_atlas.groups - before_atlas.groups, 2);
                assert_eq!(after_atlas.submissions - before_atlas.submissions, 1);
            }
            5 => assert_eq!(after_layers.rasterized - before_layers.rasterized, 2),
            6 => assert_eq!(after_layers.reused - before_layers.reused, 2),
            _ => {}
        }
        let expected = direct.snapshot(&scene, 192, 144, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        for (i, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                a.abs_diff(*b) <= 1,
                "offset {x}, pixel ({}, {}), channel {}: {a} vs {b}",
                i / 4 % 192,
                i / 4 / 192,
                i % 4,
            );
        }
    }
    Ok(())
}

#[test]
fn hinted_layer_reuses_fractional_translation_and_refreshes_content()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_renderer = Renderer::with_device(gpu.clone())?;
    let mut interpreter_renderer = Renderer::with_device(gpu)?;
    let mut aot = LayerMotion::new();
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("LayerMotion")?;
    aot.set_cache_layer(true);
    interpreter.set("cache-layer", true.into())?;
    let input = Interaction::default();
    let mut initial = None;
    for (offset, edited, opacity, reused) in [
        (8.0, false, 1.0, false),
        (12.125, false, 1.0, true),
        (20.381, false, 0.61, true),
        (27.25, true, 0.61, false),
        (23.4375, true, 0.61, false),
        (18.125, true, 0.61, true),
        (-4.25, true, 0.61, true),
    ] {
        aot.set_offset(Length(offset));
        aot.set_edited(edited);
        aot.set_fade(opacity);
        interpreter.set("offset", Length(offset).into())?;
        interpreter.set("edited", edited.into())?;
        interpreter.set("fade", opacity.into())?;
        let before = aot_renderer.opacity_layer_stats();
        let actual = aot_renderer.snapshot(&aot.render_shared(), 192, 144, 2.0, &input)?;
        let interpreted =
            interpreter_renderer.snapshot(&interpreter.render_shared(), 192, 144, 2.0, &input)?;
        assert_same_pixels(&actual, &interpreted);
        assert_eq!(
            aot_renderer.opacity_layer_stats().reused - before.reused,
            u64::from(reused),
            "x={offset}",
        );
        let pixel = actual.pixel(((offset + 18.0) * 2.0) as u32, 59).unwrap();
        if edited {
            assert!(pixel[0] > pixel[2], "x={offset} pixel={pixel:?}");
        } else {
            assert!(pixel[0] < pixel[2], "x={offset} pixel={pixel:?}");
        }
        let edge = actual.pixel(((offset + 45.0) * 2.0) as u32, 95).unwrap();
        assert!(edge[0] < 200, "x={offset} edge={edge:?}");
        assert_eq!(actual.pixel(170, 60), Some([0xe7, 0xe9, 0xee, 0xff]));
        if offset == 8.0 {
            initial = Some(actual);
        } else if offset == 12.125 {
            // The opaque rectangle's interior is unchanged after resampling.
            let source = initial.as_ref().and_then(|frame| frame.pixel(52, 59));
            assert_eq!(Some(pixel), source);
        }
    }
    Ok(())
}

#[test]
fn hinted_layers_share_textures_without_sharing_placement_or_lifetime()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut shared = Renderer::with_device(gpu.clone())?;
    let mut separate = Renderer::with_device(gpu)?;
    separate.set_hinted_layer_sharing_enabled(false);
    let mut aot = LayerMotion::new();
    aot.set_cache_layer(true);
    let mut scene = aot.render();
    let mut front = scene.children[0].clone();
    front.set_instance_layout_property("x", Length(41.5).into());
    front.set_instance_layout_property("opacity", 0.42_f64.into());
    scene.children.push(front);
    let input = Interaction::default();
    let capture =
        |renderer: &mut Renderer, scene: &Node| renderer.snapshot(scene, 192, 144, 2.0, &input);
    assert_same_pixels(
        &capture(&mut shared, &scene)?,
        &capture(&mut separate, &scene)?,
    );
    assert_eq!(shared.opacity_layer_stats().rasterized, 1);
    assert_eq!(shared.opacity_layer_stats().reused, 1);
    assert_eq!(separate.opacity_layer_stats().rasterized, 2);

    // The front layer continues to draw after the original owner is released.
    scene.children[0].properties.insert("visible", false.into());
    for x in [40.125, 35.375] {
        scene.children[1].set_instance_layout_property("x", Length(x).into());
        assert_same_pixels(
            &capture(&mut shared, &scene)?,
            &capture(&mut separate, &scene)?,
        );
    }
    assert_eq!(shared.opacity_layer_stats().rasterized, 1);

    scene.children[0].properties.insert("visible", true.into());
    scene.children[0]
        .properties
        .insert("background", Color(0xb84c75ff).into());
    assert_same_pixels(
        &capture(&mut shared, &scene)?,
        &capture(&mut separate, &scene)?,
    );
    assert_eq!(shared.opacity_layer_stats().rasterized, 2);

    // Equal paint at a different initial pixel phase needs its own texture.
    let mut fractional = scene.children[1].clone();
    fractional.set_instance_layout_property("x", Length(16.375).into());
    scene.children.push(fractional);
    assert_same_pixels(
        &capture(&mut shared, &scene)?,
        &capture(&mut separate, &scene)?,
    );
    assert_eq!(shared.opacity_layer_stats().rasterized, 3);
    Ok(())
}

#[test]
fn compositor_culls_clear_color_and_fully_occluded_layers() -> Result<(), Box<dyn std::error::Error>>
{
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node, update_profile};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_cached_layer_compositing_enabled(false);
    direct.set_layer_budget(0);

    let rect = |[x, y, width, height]: [f64; 4], tint| {
        let mut item = Node::new("Rectangle", None);
        for (name, length) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            item.properties.insert(name, Length(length).into());
        }
        item.properties.insert("background", Color(tint).into());
        item
    };
    let background = 0xdbe5f3ff;
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(background).into());
    let behind = rect([5.0, 5.0, 112.0, 87.0], 0x72584eff);
    let cover = rect([0.0, 0.0, 128.0, 112.0], 0x52637aff);
    let mut panel = rect([9.0, 7.0, 108.0, 88.0], background);
    panel.properties.insert("clip", true.into());
    panel
        .properties
        .insert("border-radius", Length(14.0).into());
    for index in 0..12 {
        let x = (index % 4) as f64 * 26.0 + 2.0;
        let y = (index / 4) as f64 * 23.0 + 2.0;
        let tint = [0x225481ff, 0x677934ff][index % 2];
        let mut card = rect([x, y, 23.0, 20.0], tint);
        card.properties.insert("opacity", 0.62_f64.into());
        card.properties.insert("cache-rendering-hint", true.into());
        card.children.push(rect([4.0, 5.0, 14.0, 10.0], 0xeca37fff));
        panel.children.push(card);
    }
    scene.children = vec![behind, panel, cover];
    let input = Interaction::default();
    for (behind, cover, expected_batches) in [
        (false, None, 1),
        (true, None, 3),
        (true, Some(0x52637aff), 1),
        (true, Some(0x52637a80), 4),
    ] {
        scene.children[0]
            .properties
            .insert("visible", behind.into());
        let foreground = &mut scene.children[2];
        foreground
            .properties
            .insert("visible", cover.is_some().into());
        if let Some(tint) = cover {
            foreground
                .properties
                .insert("background", Color(tint).into());
        }
        let mut first = None;
        let first_sample = update_profile::measure(|| {
            first = Some(optimized.snapshot(&scene, 256, 224, 2.0, &input));
        })
        .ok_or("missing first compositor profile")?;
        first.ok_or("missing first compositor frame")??;
        let mut rounded_tiles = optimized.rounded_tile_rasterizations();
        for _ in 0..2 {
            optimized.snapshot(&scene, 256, 224, 2.0, &input)?;
            rounded_tiles += optimized.rounded_tile_rasterizations();
        }
        if !behind && cover.is_none() {
            assert_eq!(rounded_tiles, 0);
        }
        if cover == Some(0x52637a80) {
            // The opaque cover stopped using the round clip. Restoring its
            // content needs one mask capture and one window submission.
            assert_eq!(first_sample.gpu_submissions, 2);
        }
        let mut actual = None;
        let sample = update_profile::measure(|| {
            actual = Some(optimized.snapshot(&scene, 256, 224, 2.0, &input));
        })
        .ok_or("missing compositor profile")?;
        let actual = actual.ok_or("missing compositor frame")??;
        let expected = direct.snapshot(&scene, 256, 224, 2.0, &input)?;
        assert_eq!(
            sample.quad_batches, expected_batches,
            "behind={behind}, cover={cover:?}"
        );
        assert_eq!(actual.layout, expected.layout);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "behind={behind}, cover={cover:?}, byte={index}: {actual} vs {expected}"
            );
        }
    }
    Ok(())
}

#[test]
fn cached_layer_compositor_preserves_order_across_mixed_display_lists()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_layout::Rect;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{
        Color, Image, ImageFit, ImageFormat, Length, LoadedImage, Node, update_profile,
    };

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut batched = Renderer::with_device(gpu.clone())?;
    let mut separate = Renderer::with_device(gpu)?;
    separate.set_cached_layer_compositing_enabled(false);
    let rect = |[x, y, width, height]: [f64; 4], color: u32| {
        let mut node = Node::new("Rectangle", None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            node.properties.insert(name, Length(value).into());
        }
        node.properties.insert("background", Color(color).into());
        node.properties.insert("border-radius", Length(3.0).into());
        node
    };
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xe7e9eeff).into());
    for index in 0..16 {
        let x = 3.0 + (index % 4) as f64 * 29.0;
        let y = 4.0 + (index / 4) as f64 * 27.0;
        let tint = [0x263854ff, 0x3e7c65ff][index % 2];
        let mut card = rect([x, y, 24.0, 21.0], tint);
        card.properties.insert("cache-rendering-hint", true.into());
        card.properties
            .insert("opacity", (0.5 + index as f64 * 0.06).into());
        card.children.push(rect([4.0, 4.0, 14.0, 12.0], 0xebaa58cf));
        scene.children.push(card);
    }
    let mut behind = rect([1.0, 1.0, 101.0, 24.0], 0x315d9fa1);
    let mut between = rect([18.0, 49.0, 69.0, 22.0], 0xa3426bdd);
    let mut in_front = rect([20.0, 76.0, 49.0, 34.0], 0xf2975880);
    for item in [&mut behind, &mut between, &mut in_front] {
        item.properties.insert("visible", false.into());
    }
    let mask = Image::Loaded(LoadedImage::new(
        ImageFormat::Png,
        std::fs::read("examples/media/assets/color-mask.png")?,
        [16, 12],
    ));
    let sprite = Image::Loaded(LoadedImage::new(
        ImageFormat::Png,
        std::fs::read("examples/media/assets/sprite-atlas.png")?,
        [8, 8],
    ));
    let image = |source: Image, x: f64, y: f64| {
        let mut node = Node::new("Image", None);
        for (name, value) in [("x", x), ("y", y), ("width", 12.75), ("height", 10.375)] {
            node.properties.insert(name, Length(value).into());
        }
        node.properties.insert("source", source.into());
        node.properties.insert("image-fit", ImageFit::Fill.into());
        node.properties.insert("visible", false.into());
        node
    };
    scene.children.splice(
        0..0,
        [
            behind,
            image(mask.clone(), 11.25, 6.25),
            image(sprite.clone(), 40.25, 7.25),
        ],
    );
    scene.children.splice(
        11..11,
        [
            between,
            image(sprite.clone(), 21.25, 57.25),
            image(mask, 76.25, 62.25),
        ],
    );
    scene.children.push(in_front);
    let input = Interaction::default();
    for (frame, expected_batches) in [
        (0, 0),
        (1, 1),
        (2, 1),
        (3, 0),
        (4, 1),
        (5, 1),
        (6, 0),
        (7, 0),
        (8, 0),
        (9, 0),
    ] {
        let x = match frame {
            0 => 3.0,
            2 | 3 => 24.375,
            _ => 3.125,
        };
        scene.children[3].properties.insert("x", Length(x).into());
        for slot in [0, 11, 22] {
            if frame <= 6 {
                scene.children[slot]
                    .properties
                    .insert("visible", (frame == 3 || frame >= 6).into());
            }
        }
        for slot in [1, 2, 12, 13] {
            scene.children[slot]
                .properties
                .insert("visible", (frame == 3).into());
        }
        if frame == 5 {
            let outline = Rect::new(7.0, 7.0, 16.0, 14.0);
            batched.set_inspection_outline(Some(outline));
            separate.set_inspection_outline(Some(outline));
        } else if frame == 9 {
            scene.children[11]
                .properties
                .insert("background", Color(0xc16268dd).into());
        }
        let before = batched.display_batch_stats();
        let mut result = None;
        let sample = update_profile::measure(|| {
            result = Some(batched.snapshot(&scene, 256, 224, 2.0, &input));
        })
        .unwrap();
        let actual = result.unwrap()?;
        let expected = separate.snapshot(&scene, 256, 224, 2.0, &input)?;
        let cached_rectangles = matches!(frame, 7 | 8);
        if !cached_rectangles {
            assert_same_pixels(&actual, &expected);
        } else {
            for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "channel {index}: {actual} vs {expected}"
                );
            }
        }
        let image_batches = batched.display_batch_stats().image_batches - before.image_batches;
        assert_eq!(image_batches, if frame == 3 { 2 } else { 0 });
        if !cached_rectangles {
            assert_eq!(
                sample.quad_batches,
                expected_batches + image_batches as usize,
                "frame {frame}"
            );
        } else {
            assert!(
                (1..19).contains(&sample.quad_batches),
                "frame {frame}: {} draw batches",
                sample.quad_batches,
            );
        }
        if frame != 0 && frame != 7 {
            assert_eq!(sample.gpu_submissions, 1, "frame {frame}");
        }
    }

    for slot in [1, 2, 12, 13] {
        let image = &mut scene.children[slot];
        for name in ["x", "y"] {
            let length = f64::from(image.length(name).unwrap()).round();
            image.properties.insert(name, Length(length).into());
        }
        image.properties.insert("width", Length(13.0).into());
        image.properties.insert("height", Length(10.0).into());
        image.properties.insert("visible", true.into());
    }
    let mut label = Node::new("Label", None);
    for (name, value) in [
        ("x", 18.0),
        ("y", 64.0),
        ("width", 60.0),
        ("height", 16.0),
        ("font-size", 12.0),
    ] {
        label.properties.insert(name, Length(value).into());
    }
    label.properties.insert("text", "Ready".to_owned().into());
    label.properties.insert("color", Color(0xfffffff0).into());
    scene.children.push(label);

    for revision in 0..4 {
        match revision {
            1 => {
                scene.children[1]
                    .properties
                    .insert("source", sprite.clone().into());
            }
            2 => {
                scene.children[23]
                    .properties
                    .insert("text", "Updated".to_owned().into());
            }
            3 => {
                assert!(batched.set_text_scale(1.25));
                assert!(separate.set_text_scale(1.25));
            }
            _ => {}
        }
        for frame in 0..4 {
            let mut result = None;
            let sample = update_profile::measure(|| {
                result = Some(batched.snapshot(&scene, 256, 224, 2.0, &input));
            })
            .unwrap();
            let actual = result.unwrap()?;
            let expected = separate.snapshot(&scene, 256, 224, 2.0, &input)?;
            assert_eq!(actual.layout, expected.layout);
            for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "revision {revision} frame {frame} channel {index}: {actual} vs {expected}"
                );
            }
            if frame == 3 {
                assert!(
                    (3..24).contains(&sample.quad_batches),
                    "revision {revision}"
                );
                assert_eq!(sample.gpu_submissions, 1, "revision {revision}");
            }
        }
    }
    Ok(())
}

#[test]
fn changing_solid_and_capsule_layers_batch_in_paint_order() -> Result<(), Box<dyn std::error::Error>>
{
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node, update_profile};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_cached_layer_compositing_enabled(false);

    let positioned = |kind, [x, y, width, height]: [f64; 4]| {
        let mut item = Node::new(kind, None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            item.properties.insert(name, Length(value).into());
        }
        item
    };
    let mut panel = positioned("Rectangle", [4.0, 4.0, 116.0, 96.0]);
    panel.properties.insert("clip", true.into());
    for index in 0..16 {
        let mut card = positioned(
            "Rectangle",
            [
                (index % 4 * 26) as f64 + 5.0,
                (index / 4 * 19) as f64 + 8.0,
                22.0,
                15.0,
            ],
        );
        card.properties.insert("opacity", 0.6_f64.into());
        card.properties
            .insert("background", Color(0x273b65ff).into());
        card.properties.insert("cache-rendering-hint", true.into());
        let mut mark = positioned("Rectangle", [3.0, 3.0, 12.0, 8.0]);
        mark.properties
            .insert("background", Color(0xc1a466cc).into());
        card.children.push(mark);
        panel.children.push(card);
    }
    let mut status = positioned("Rectangle", [2.0, 1.0, 112.0, 5.0]);
    status
        .properties
        .insert("background", Color(0x60c897ff).into());
    panel.children.insert(0, status);
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xe7e9eeff).into());
    scene.children.push(panel);
    let input = Interaction::default();

    for (opacity, y, z) in [
        (0.25, 1.0, 0.0),
        (0.50, 12.0, 1.0),
        (0.75, 12.0, -1.0),
        (0.65, 1.0, 0.0),
    ] {
        let status = &mut scene.children[0].children[0];
        for (name, value) in [
            ("y", Length(y).into()),
            ("z", z.into()),
            ("opacity", opacity.into()),
            ("width", Length((60.0 + 64.0_f64 * opacity).round()).into()),
        ] {
            status.properties.insert(name, value);
        }
        for _ in 0..3 {
            optimized.snapshot(&scene, 256, 224, 2.0, &input)?;
        }
        let mut actual = None;
        let sample = update_profile::measure(|| {
            actual = Some(optimized.snapshot(&scene, 256, 224, 2.0, &input));
        })
        .ok_or("missing frame profile")?;
        let actual = actual.ok_or("missing rendered frame")??;
        let expected = direct.snapshot(&scene, 256, 224, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        assert_eq!(sample.quad_batches, 2, "opacity={opacity}");
        assert_eq!(sample.gpu_submissions, 1);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "opacity={opacity} byte={index}"
            );
        }
    }
    let mut bar = positioned("ProgressIndicator", [4.0, 26.0, 104.0, 8.0]);
    bar.properties.insert("z", 1.0_f64.into());
    bar.properties.insert("color", Color(0x60c897ff).into());
    bar.properties
        .insert("track-color", Color(0x3b588bb9).into());
    scene.children[0].children[0] = bar;
    for scale in [1.0, 1.5, 2.0] {
        let width = (128.0 * scale) as u32;
        let height = (112.0 * scale) as u32;
        for direction in [
            xrack_runtime::LayoutDirection::LeftToRight,
            xrack_runtime::LayoutDirection::RightToLeft,
        ] {
            let status = &mut scene.children[0].children[0];
            status
                .properties
                .insert("layout-direction", direction.into());
            status
                .properties
                .insert("progress", (44.125_f64 / 104.0).into());
            for _ in 0..3 {
                optimized.snapshot(&scene, width, height, scale, &input)?;
            }
            for fill in [44.125, 50.125, 56.125] {
                scene.children[0].children[0]
                    .properties
                    .insert("progress", (fill / 104.0_f64).into());
                let mut actual = None;
                let sample = update_profile::measure(|| {
                    actual = Some(optimized.snapshot(&scene, width, height, scale, &input));
                })
                .ok_or("missing frame profile")?;
                let actual = actual.ok_or("missing rendered frame")??;
                let expected = direct.snapshot(&scene, width, height, scale, &input)?;
                assert_eq!(actual.layout, expected.layout);
                assert!(
                    sample.quad_batches >= 2,
                    "scale={scale} direction={direction:?} fill={fill}"
                );
                assert_eq!(sample.gpu_submissions, 1);
                assert_eq!(optimized.rounded_tile_rasterizations(), 0);
                for (index, (actual, expected)) in
                    actual.rgba.iter().zip(&expected.rgba).enumerate()
                {
                    assert!(
                        actual.abs_diff(*expected) <= 1,
                        "scale={scale} direction={direction:?} fill={fill} byte={index}: {actual} vs {expected}"
                    );
                }
            }
        }
    }
    scene.children[0].children[0]
        .properties
        .insert("indeterminate", true.into());
    for elapsed in [
        std::time::Duration::from_millis(137),
        std::time::Duration::from_millis(634),
    ] {
        let actual = optimized.snapshot_at(&scene, 256, 224, 2.0, &input, elapsed)?;
        let expected = direct.snapshot_at(&scene, 256, 224, 2.0, &input, elapsed)?;
        assert!(optimized.needs_redraw());
        assert_eq!(actual.layout, expected.layout);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "animated byte={index}: {actual} vs {expected}"
            );
        }
    }
    let status = &mut scene.children[0].children[0];
    status.properties.insert("indeterminate", false.into());
    status.properties.insert("progress", 0.02_f64.into());
    let actual = optimized.snapshot(&scene, 256, 224, 2.0, &input)?;
    let expected = direct.snapshot(&scene, 256, 224, 2.0, &input)?;
    for (actual, expected) in actual.rgba.iter().zip(&expected.rgba) {
        assert!(actual.abs_diff(*expected) <= 1);
    }

    let panel = &mut scene.children[0];
    panel
        .properties
        .insert("border-radius", Length(20.0).into());
    panel.children[0].properties.insert("y", Length(3.0).into());
    panel.children[0]
        .properties
        .insert("progress", 0.63_f64.into());
    let content = std::mem::take(&mut panel.children);
    for kind in ["Flickable", "ScrollView"] {
        let mut scroll = positioned(kind, [0.0, 0.0, 116.0, 96.0]);
        scroll
            .properties
            .insert("content-height", Length(128.0).into());
        if kind == "ScrollView" {
            for name in ["horizontal-scroll-bar-policy", "vertical-scroll-bar-policy"] {
                scroll
                    .properties
                    .insert(name, xrack_runtime::ScrollBarPolicy::AlwaysOff.into());
            }
        }
        scroll.children.clone_from(&content);
        scene.children[0].children = vec![scroll];

        let mut scroll_input = Interaction::default();
        let mut layout =
            optimized.layout_with_input(&scene, Size::new(128.0, 112.0), &scroll_input);
        for _ in 0..3 {
            optimized.snapshot(&scene, 256, 224, 2.0, &scroll_input)?;
        }
        for offset in [0.0, 2.25, 5.0, 5.75, 6.75, 7.75, 0.0] {
            if layout
                .at_path(&[0, 0])
                .and_then(|view| view.scroll_on(xrack_runtime::Orientation::Vertical))
                .is_some_and(|scroll| scroll.offset != offset)
            {
                assert!(scroll_input.accessibility_scroll_to(
                    &layout,
                    &[0, 0],
                    xrack_runtime::Orientation::Vertical,
                    offset
                ));
            }
            let mut actual = None;
            let sample = update_profile::measure(|| {
                actual = Some(optimized.snapshot(&scene, 256, 224, 2.0, &scroll_input));
            })
            .ok_or("missing scroll frame profile")?;
            let actual = actual.ok_or("missing scrolled frame")??;
            let expected = direct.snapshot(&scene, 256, 224, 2.0, &scroll_input)?;
            assert_eq!(actual.layout, expected.layout);
            assert_eq!(sample.quad_batches, 2, "{kind} offset={offset}");
            if matches!(offset, 6.75 | 7.75) {
                assert_eq!(
                    optimized.rounded_tile_rasterizations(),
                    0,
                    "{kind} offset={offset}"
                );
            }

            for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "{kind} offset={offset} byte={index}: {actual} vs {expected}"
                );
            }
            layout = actual.layout;
        }
    }

    // Establish three cap phases in the stationary viewport. On the first
    // fractional scroll pose, warm those phases in the same atlas pass.
    scene.children[0].children[0].children[0]
        .properties
        .insert("color", Color(0xbc7461d3).into());
    let mut scrolling = Interaction::default();
    for fill in [44.125, 44.25, 44.375] {
        scene.children[0].children[0].children[0]
            .properties
            .insert("progress", (fill / 104.0_f64).into());
        optimized.snapshot(&scene, 256, 224, 2.0, &scrolling)?;
    }
    let layout = optimized.layout_with_input(&scene, Size::new(128.0, 112.0), &scrolling);
    assert!(scrolling.accessibility_scroll_to(
        &layout,
        &[0, 0],
        xrack_runtime::Orientation::Vertical,
        0.25,
    ));
    for (fill, warmed) in [(44.125, false), (44.25, true), (44.375, true)] {
        scene.children[0].children[0].children[0]
            .properties
            .insert("progress", (fill / 104.0_f64).into());
        let mut shot = None;
        let sample = update_profile::measure(|| {
            shot = Some(optimized.snapshot(&scene, 256, 224, 2.0, &scrolling));
        })
        .ok_or("missing fractional scroll profile")?;
        let actual = shot.ok_or("missing fractional scroll frame")??;
        let expected = direct.snapshot(&scene, 256, 224, 2.0, &scrolling)?;
        assert!(sample.quad_batches >= 2);
        assert_eq!(
            optimized.rounded_tile_rasterizations() == 0,
            warmed,
            "fill={fill}"
        );
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(actual.abs_diff(*expected) <= 1, "fill={fill} byte={index}");
        }
    }

    // RGB changes reuse the same alpha tiles. A forced packing reset must
    // clear the reused regions before drawing the new mask.
    scene.children[0].children[0].children[0]
        .properties
        .insert("color", Color(0x8062cbd3).into());
    for reset in [false, true] {
        optimized.set_fractional_rounded_tile_retention_enabled(!reset);
        let actual = optimized.snapshot(&scene, 256, 224, 2.0, &scrolling)?;
        let expected = direct.snapshot(&scene, 256, 224, 2.0, &scrolling)?;
        assert_eq!(optimized.rounded_tile_rasterizations() > 0, reset);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "reset={reset} byte={index}"
            );
        }
    }

    Ok(())
}

#[test]
fn translated_rounded_viewport_reuses_mask_and_subtree_rasters()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node, update_profile};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut retained = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu.clone())?;
    direct.set_cached_layer_compositing_enabled(false);
    direct.set_layer_budget(0);

    let rectangle = |[x, y, width, height]: [f64; 4]| {
        let mut node = Node::new("Rectangle", None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            node.properties.insert(name, Length(value).into());
        }
        node
    };
    let mut outer = rectangle([8.0, 6.0, 104.0, 68.0]);
    outer.properties.insert("clip", true.into());
    outer
        .properties
        .insert("border-radius", Length(24.0).into());
    let mut inner = rectangle([4.0, 3.0, 96.0, 62.0]);
    inner.properties.insert("clip", true.into());
    inner
        .properties
        .insert("border-radius", Length(18.0).into());
    for index in 0..12 {
        let mut card = rectangle([(index % 4 * 23) as f64, (index / 4 * 22) as f64, 21.0, 17.0]);
        card.properties
            .insert("background", Color(0x355582ff).into());
        card.properties.insert("opacity", 0.55_f64.into());
        card.properties.insert("cache-rendering-hint", true.into());
        let mut swatch = rectangle([4.0, 4.0, 13.0, 8.0]);
        let tint = [0xefa65eff, 0x7fd5a4ff][index % 2];
        swatch.properties.insert("background", Color(tint).into());
        card.children.push(swatch);
        inner.children.push(card);
    }
    outer.children.push(inner);
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xf0eae3ff).into());
    scene.children.push(outer);
    let input = Interaction::default();
    for _ in 0..3 {
        retained.snapshot(&scene, 240, 180, 2.0, &input)?;
    }

    let mut previous = None;
    for (x, y, inner_x, reuse) in [
        (8.5, 6.5, 4.0, true),
        (11.0, 8.0, 4.0, true),
        (13.0, 9.0, 4.0, true),
        (8.0, 6.0, 4.0, true),
        (13.0, 9.0, 4.5, false),
        (10.5, 7.5, 4.5, true),
    ] {
        let outer = &mut scene.children[0];
        outer.properties.insert("x", Length(x).into());
        outer.properties.insert("y", Length(y).into());
        outer.children[0]
            .properties
            .insert("x", Length(inner_x).into());
        let before = retained.opacity_layer_stats();
        let mut actual = None;
        let sample = update_profile::measure(|| {
            actual = Some(retained.snapshot(&scene, 240, 180, 2.0, &input));
        })
        .ok_or("missing viewport profile")?;
        let actual = actual.ok_or("missing viewport frame")??;
        let expected = direct.snapshot(&scene, 240, 180, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        assert!(sample.quad_batches > 0);
        assert_eq!(
            sample.gpu_submissions == 1,
            reuse,
            "position={x},{y} inner={inner_x}"
        );
        assert_eq!(retained.opacity_layer_stats().rasterized, before.rasterized);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "position={x},{y} inner={inner_x} byte={index}: {actual} vs {expected}"
            );
        }
        if let Some(last) = previous.replace(actual.rgba) {
            assert_ne!(previous.as_ref(), Some(&last));
        }
    }
    // Two viewports with distinct card textures share a batch and use
    // different clip sample origins.
    let mut adjacent = scene.children[0].clone();
    adjacent.properties.insert("x", Length(128.5).into());
    scene.children.push(adjacent);
    for _ in 0..3 {
        retained.snapshot(&scene, 480, 180, 2.0, &input)?;
    }
    let mut actual = None;
    let sample = update_profile::measure(|| {
        actual = Some(retained.snapshot(&scene, 480, 180, 2.0, &input));
    })
    .ok_or("missing shared-mask profile")?;
    let actual = actual.ok_or("missing shared-mask frame")??;
    let expected = direct.snapshot(&scene, 480, 180, 2.0, &input)?;
    assert_eq!(sample.gpu_submissions, 1);
    assert_eq!(sample.quad_batches, 1);
    for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
        assert!(
            actual.abs_diff(*expected) <= 1,
            "shared-mask byte={index}: {actual} vs {expected}"
        );
    }

    // Paint both viewports to exercise the sliced background through an
    // ancestor clip. Tint changes preserve the geometry in the atlas.
    let mut painted = Renderer::with_device(gpu)?;
    scene.children.truncate(1);
    scene.children[0].children[0]
        .properties
        .insert("background", Color(0x4f6b8bff).into());
    for frame in 0..17 {
        let panel = &mut scene.children[0];
        let x = 8.0 + f64::from(frame) * 0.5;
        let color = if frame < 6 { 0x17515fff } else { 0x593e77ff };
        panel
            .properties
            .insert("x", Length(if frame == 13 { x + 0.125 } else { x }).into());
        panel.properties.insert(
            "width",
            Length(if frame < 9 { 104.0 } else { 108.0 }).into(),
        );
        if frame == 9 {
            panel
                .properties
                .insert("border-top-left-radius", Length(31.5).into());
        }
        panel.properties.insert("background", Color(color).into());
        let before = painted.opacity_layer_stats();
        let mut actual = None;
        let sample = update_profile::measure(|| {
            actual = Some(painted.snapshot(&scene, 264, 180, 2.0, &input));
        })
        .ok_or("missing painted viewport profile")?;
        let actual = actual.ok_or("missing painted viewport frame")??;
        let expected = direct.snapshot(&scene, 264, 180, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            // Check the panel's exposed bottom edge at the fractional phase.
            // Sampled child tiles have their own filtering on this frame.
            if frame == 13 && index / (264 * 4) < 146 {
                continue;
            }
            assert!(
                actual.abs_diff(*expected) <= 1,
                "painted viewport frame={frame} byte={index}: {actual} vs {expected}"
            );
        }
        if matches!(frame, 4..=8 | 11 | 12 | 16) {
            assert!(sample.quad_batches >= 2, "frame={frame}");
            assert_eq!(sample.gpu_submissions, 1, "frame={frame}");
            assert_eq!(painted.opacity_layer_stats().rasterized, before.rasterized);
            assert_eq!(painted.rounded_tile_rasterizations(), 0, "frame={frame}");
        }
    }

    // Keep all edges on whole device pixels at both additional scales.
    let panel = &mut scene.children[0];
    panel.properties.insert("y", Length(8.0).into());
    let inner = &mut panel.children[0];
    inner.properties.insert("x", Length(4.0).into());
    inner.properties.insert("y", Length(4.0).into());
    inner.properties.insert("height", Length(64.0).into());
    for (index, card) in inner.children.iter_mut().enumerate() {
        card.properties
            .insert("x", Length((index % 4 * 24) as f64).into());
        card.properties
            .insert("y", Length((index / 4 * 20) as f64).into());
        card.properties.insert("width", Length(20.0).into());
        card.properties.insert("height", Length(16.0).into());
    }
    // Put solid rectangles on both sides of the retained viewports in paint
    // order and make the scene span several compositor reorder windows.
    let tile = |index: usize| {
        let x = (index % 24 * 4 + 8) as f64;
        let y = (index / 24 * 8 + 4) as f64;
        let mut tile = rectangle([x, y, 4.0, 4.0]);
        let color = [0x1d5c8280, 0xb04744ff][index % 2];
        tile.properties.insert("background", Color(color).into());
        tile
    };
    scene.children.splice(..0, (0..72).map(tile));
    scene.children.extend((72..144).map(tile));
    for scale in [1.0, 1.25] {
        let [width, height] = [132.0, 92.0].map(|extent| (extent * scale) as u32);
        for _ in 0..3 {
            painted.snapshot(&scene, width, height, scale, &input)?;
        }
        for changed in [false, true] {
            if changed {
                let tile = scene.children.last_mut().ok_or("missing final tile")?;
                let tint = if scale == 1.0 { 0x49a369ff } else { 0xa05a91ff };
                tile.properties.insert("background", Color(tint).into());
            }
            let rectangle_draws = painted.rectangle_batch_draws();
            let mut actual = None;
            let sample = update_profile::measure(|| {
                actual = Some(painted.snapshot(&scene, width, height, scale, &input));
            })
            .ok_or("missing scaled viewport profile")?;
            let actual = actual.ok_or("missing scaled viewport frame")??;
            let expected = direct.snapshot(&scene, width, height, scale, &input)?;
            assert!(sample.quad_batches >= 2, "scale={scale}");
            assert_eq!(sample.gpu_submissions, 1, "scale={scale}");
            if changed
                && painted
                    .device
                    .features()
                    .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
            {
                assert!(sample.vertex_bytes >= 48 && sample.vertex_bytes.is_multiple_of(48));
            } else {
                assert_eq!(
                    sample.vertex_bytes,
                    if changed { 48 } else { 0 },
                    "scale={scale}"
                );
            }
            assert_eq!(
                painted.rectangle_batch_draws(),
                rectangle_draws,
                "scale={scale}"
            );
            assert_eq!(actual.layout, expected.layout);
            for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "painted viewport scale={scale} byte={index}: {actual} vs {expected}"
                );
            }
        }
    }

    Ok(())
}

#[test]
fn cached_layers_flatten_nested_panels_in_paint_order() -> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node, update_profile};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let node = |kind, [x, y, width, height]: [f64; 4]| {
        let mut item = Node::new(kind, None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            item.properties.insert(name, Length(value).into());
        }
        item
    };
    let make_panel = |x: f64, z: f64, tint: u32| {
        let mut panel = node("Rectangle", [x, 6.0, 64.0, 84.0]);
        panel.properties.insert("background", Color(tint).into());
        panel.properties.insert("z", z.into());
        let mut layout = node("VerticalLayout", [2.0, 2.0, 60.0, 80.0]);
        layout.properties.insert("padding", Length(2.0).into());
        layout.properties.insert("spacing", Length(2.0).into());
        for row in 0..4 {
            let mut card = node("Rectangle", [0.0, 0.0, 54.0, 16.0]);
            card.properties
                .insert("background", Color(0x263854ff).into());
            card.properties.insert("border-radius", Length(4.0).into());
            card.properties.insert("cache-rendering-hint", true.into());
            card.properties
                .insert("opacity", (0.55 + f64::from(row) * 0.08).into());
            let mut mark = node("Rectangle", [5.0, 5.0, 19.0, 7.0]);
            mark.properties
                .insert("background", Color(0x81b4e8ff).into());
            card.children.push(mark);
            layout.children.push(card);
        }
        let mut badge = layout.children[0].clone();
        badge.properties.insert("x", Length(2.0).into());
        badge.properties.insert("y", Length(54.0).into());
        badge.properties.insert("width", Length(16.0).into());
        badge.children[0]
            .properties
            .insert("width", Length(6.0).into());
        panel.children.push(layout);
        panel.children.push(badge);
        panel
    };
    for track_nodes in [false, true] {
        let mut batched = Renderer::with_device(gpu.clone())?;
        let mut separate = Renderer::with_device(gpu.clone())?;
        batched.set_retained_geometry_enabled(track_nodes);
        separate.set_retained_geometry_enabled(track_nodes);
        separate.set_cached_layer_compositing_enabled(false);
        let mut scene = Node::new("Window", None);
        scene
            .properties
            .insert("background", Color(0xe7e9eeff).into());
        scene.children = vec![
            make_panel(55.0, 1.0_f64, 0xa0805cc0),
            make_panel(4.0, -1.0_f64, 0x82badbdd),
        ];
        let input = Interaction::default();
        let mut old_mark = None;
        let mut old_corner = None;
        for frame in 0..28 {
            if frame == 4 {
                scene.children[0].children[0].children[0]
                    .properties
                    .insert("opacity", 0.9_f64.into());
                scene.children[0].children[1]
                    .properties
                    .insert("x", Length(2.123).into());
            } else if matches!(frame, 5 | 6 | 7) {
                let x = [57.123, 66.125, 2.123][frame - 5];
                scene.children[0].children[1]
                    .properties
                    .insert("x", Length(x).into());
            } else if frame == 8 {
                scene.children[0]
                    .properties
                    .insert("background", Color(0x465c9ec0).into());
            } else if frame == 10 {
                let badge = &mut scene.children[0].children[1];
                badge.properties.insert("x", Length(57.123).into());
                badge.children[0]
                    .properties
                    .insert("background", Color(0xfc783eff).into());
            } else if frame == 11 {
                scene.children[0].properties.insert("clip", true.into());
                scene.children[0]
                    .properties
                    .insert("border-radius", Length(7.0).into());
            } else if matches!(frame, 12 | 15 | 16 | 17) {
                let (x, y) = match frame {
                    15 => (57.123, 1.125),
                    16 => (66.125, 1.125),
                    _ => (2.123, 54.0),
                };
                let badge = &mut scene.children[0].children[1];
                badge.properties.insert("x", Length(x).into());
                badge.properties.insert("y", Length(y).into());
            } else if frame == 18 {
                let panel = &mut scene.children[0];
                let mut viewport = node("Rectangle", [2.0, 2.0, 60.0, 80.0]);
                viewport.properties.insert("clip", true.into());
                viewport
                    .properties
                    .insert("border-radius", Length(9.0).into());
                viewport
                    .properties
                    .insert("border-top-left-radius", Length(14.0).into());
                let mut layout = std::mem::replace(&mut panel.children[0], viewport);
                layout.properties.insert("x", Length(0.0).into());
                layout.properties.insert("y", Length(0.0).into());
                panel.children[0].children.push(layout);
            } else if frame == 23 {
                scene.children[0].children[0]
                    .properties
                    .insert("border-top-left-radius", Length(18.0).into());
            }
            let before = batched.opacity_layer_stats();
            let mut result = None;
            let sample = update_profile::measure(|| {
                result = Some(batched.snapshot(&scene, 256, 192, 2.0, &input));
            })
            .unwrap();
            let actual = result.unwrap()?;
            let expected = separate.snapshot(&scene, 256, 192, 2.0, &input)?;
            assert_eq!(actual.layout, expected.layout);
            for (byte, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "frame {frame} byte {byte}: {actual} vs {expected}"
                );
            }
            if (3..=7).contains(&frame) || (15..=17).contains(&frame) || matches!(frame, 22 | 27) {
                assert!(sample.quad_batches > 0, "frame {frame}");
                assert_eq!(sample.gpu_submissions, 1, "frame {frame}");
                assert_eq!(batched.opacity_layer_stats().rasterized, before.rasterized);
                assert_eq!(
                    actual.pixel(244, 129),
                    Some([0xe7, 0xe9, 0xee, 0xff]),
                    "frame {frame}"
                );
            } else if matches!(frame, 8 | 10 | 11) {
                assert_eq!(sample.quad_batches, 0, "frame {frame}");
            }
            if frame == 5 {
                old_mark = actual.pixel(236, 133);
            } else if frame == 10 {
                assert_ne!(actual.pixel(236, 133), old_mark);
            } else if frame == 22 {
                old_corner = actual.pixel(122, 26);
            } else if frame == 27 {
                assert_ne!(actual.pixel(122, 26), old_corner);
            }
        }
    }
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn moving_opacity_tiles_require_equal_paint_and_pixel_phase() {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Length, Node};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut shared = Renderer::with_device(gpu.clone()).unwrap();
    let mut separate = Renderer::with_device(gpu).unwrap();
    separate.set_opacity_tile_sharing_enabled(false);
    let input = Interaction::default();
    let rect = |kind, [x, y, width, height]: [f64; 4]| {
        let mut node = Node::new(kind, None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            node.properties.insert(name, Length(value).into());
        }
        node
    };
    for (frame, x) in [8.0, 8.25, 8.375, 8.0625, 8.4375].into_iter().enumerate() {
        let mut scene = Node::new("Window", None);
        scene
            .properties
            .insert("background", Color(0xe7e9eeff).into());
        let mut a = rect("Rectangle", [x, 7.0, 49.0, 43.0]);
        a.properties.insert("background", Color(0x263854ff).into());
        a.properties.insert("border-radius", Length(5.0).into());
        a.properties.insert("opacity", 0.61_f64.into());
        let mut text = rect("Text", [6.0, 4.0, 38.0, 14.0]);
        text.properties.insert("text", "Load".to_owned().into());
        text.properties.insert("color", Color(0xffffffff).into());
        text.properties.insert("font-size", Length(10.0).into());
        a.children.push(text);
        let mut b = a.clone();
        let phase_shift = if frame == 4 { 0.125 } else { 0.0 };
        b.properties
            .insert("x", Length(x + 22.0 + phase_shift).into());
        b.properties.insert("y", Length(25.0).into());
        b.properties.insert("opacity", 0.72_f64.into());
        if frame == 3 {
            b.children[0]
                .properties
                .insert("text", "Busy".to_owned().into());
        }
        scene.children = vec![a, b];
        let before = shared.opacity_atlas_stats();
        let actual = shared.snapshot(&scene, 192, 144, 2.0, &input).unwrap();
        let expected = separate.snapshot(&scene, 192, 144, 2.0, &input).unwrap();
        let after = shared.opacity_atlas_stats();
        assert_eq!(
            after.reused_tiles - before.reused_tiles,
            u64::from(frame == 2),
            "frame {frame}"
        );
        if frame >= 2 {
            assert_eq!(after.groups - before.groups, 2);
        }
        assert_eq!(actual.layout, expected.layout);
        for (index, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                a.abs_diff(*b) <= 1,
                "frame {frame}, channel {index}: {a} vs {b}"
            );
        }
    }
}

#[test]
fn shadowed_groups_reuse_filtered_pixels_and_track_visible_changes()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut compiled_renderer = Renderer::with_device(gpu.clone())?;
    let mut interpreter_renderer = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_layer_budget(0);
    let input = Interaction::default();
    let mut compiled = LayerMotion::new();
    compiled.set_shadowed(true);
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("LayerMotion")?;
    interpreter.set("shadowed", true.into())?;

    for (x, width, fade, tint, edited, rasterized, reused, masks) in [
        (22.0, 49.0, 1.0, 0x1017_2666, false, 1, 0, 1),
        (23.0, 49.0, 0.61, 0x1017_2666, false, 0, 1, 1),
        (26.0, 49.0, 1.0, 0x1017_2666, false, 0, 1, 1),
        (26.0, 49.0, 0.77, 0x7f70_2e66, false, 0, 0, 1),
        (24.0, 49.0, 0.78, 0x7f70_2e66, false, 1, 0, 1),
        (-3.0, 49.0, 0.71, 0x7f70_2e66, false, 1, 0, 1),
        (22.0, 49.0, 0.64, 0x7f70_2e66, true, 0, 0, 1),
        (24.0, 49.0, 0.72, 0x7f70_2e66, false, 0, 0, 1),
        (25.25, 49.0, 0.72, 0x7f70_2e66, true, 0, 0, 1),
        (26.173, 49.0, 0.72, 0x7f70_2e66, true, 0, 0, 1),
        (23.0, 49.0, 0.88, 0x7f70_2e66, true, 1, 0, 1),
        (24.0, 53.0, 0.88, 0x7f70_2e66, true, 0, 0, 2),
        (26.0, 53.0, 0.72, 0x7f70_2e66, true, 1, 0, 2),
    ] {
        compiled.set_offset(Length(x));
        compiled.set_card_width(Length(width));
        compiled.set_fade(fade);
        compiled.set_edited(edited);
        compiled.set_shadow_tint(Color(tint));
        interpreter.set("offset", Length(x).into())?;
        interpreter.set("card-width", Length(width).into())?;
        interpreter.set("fade", fade.into())?;
        interpreter.set("edited", edited.into())?;
        interpreter.set("shadow-tint", Color(tint).into())?;
        let scene = compiled.render_shared();
        let (pixel_width, pixel_height) = (248, 192);
        let before = compiled_renderer.opacity_layer_stats();
        let actual = compiled_renderer.snapshot(&scene, pixel_width, pixel_height, 2.0, &input)?;
        let after = compiled_renderer.opacity_layer_stats();
        assert_eq!(after.rasterized - before.rasterized, rasterized, "x={x}");
        assert_eq!(after.reused - before.reused, reused, "x={x}");
        assert_eq!(
            compiled_renderer.shadow_mask_stats().rasterized,
            masks,
            "x={x}"
        );
        let interpreted = interpreter_renderer.snapshot(
            &interpreter.render(),
            pixel_width,
            pixel_height,
            2.0,
            &input,
        )?;
        assert_same_pixels(&actual, &interpreted);

        let expected = direct.snapshot(&scene, pixel_width, pixel_height, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        for (index, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                a.abs_diff(*b) <= 2,
                "x={x} pixel ({}, {}) channel {}: {a} vs {b}",
                (index / 4) % pixel_width as usize,
                (index / 4) / pixel_width as usize,
                index % 4,
            );
        }
    }
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn fractional_rectangles_share_the_transient_opacity_atlas() {
    use xrack_runtime::{Color, Length, Node};

    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0x314567ff).into());
    for index in 0..12 {
        let mut rectangle = Node::new("Rectangle", None);
        for (name, value) in [
            ("x", 12.125 + f64::from(index % 4) * 22.25),
            ("y", 10.375 + f64::from(index / 4) * 19.125),
            ("width", 25.125),
            ("height", 17.625),
        ] {
            rectangle.properties.insert(name, Length(value).into());
        }
        rectangle.properties.insert(
            "background",
            Color([0x2a68d6ff, 0xec7847b9, 0x62d439ff][index as usize % 3]).into(),
        );
        rectangle
            .properties
            .insert("opacity", (0.47 + f64::from(index % 3) * 0.18).into());
        scene.children.push(rectangle);
    }
    let mut clipped = Node::new("Window", None);
    clipped.properties = scene.properties.clone();
    let mut viewport = Node::new("Rectangle", None);
    for (name, value) in [
        ("x", 4.375),
        ("y", 3.625),
        ("width", 100.3125),
        ("height", 60.1875),
    ] {
        viewport.properties.insert(name, Length(value).into());
    }
    viewport.properties.insert("clip", true.into());
    viewport.children = scene.children.clone();
    clipped.children.push(viewport);
    for scale in [1.0, 1.25, 2.0, 3.0] {
        let mut renderer = pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
        .unwrap();
        let width = (124.0 * scale) as u32;
        let height = (84.0 * scale) as u32;
        let input = Interaction::default();
        renderer.set_rectangle_optimization_enabled(false);
        renderer.set_opacity_atlas_enabled(false);
        let original = renderer
            .snapshot(&scene, width, height, scale, &input)
            .unwrap();
        renderer.set_opacity_atlas_enabled(true);
        let before = renderer.opacity_atlas_stats().groups;
        let shortcut = renderer
            .snapshot(&scene, width, height, scale, &input)
            .unwrap();
        assert!(renderer.opacity_atlas_stats().groups >= before + 2);
        let max_error = original
            .rgba
            .iter()
            .zip(&shortcut.rgba)
            .map(|(&first, &second)| first.abs_diff(second))
            .max()
            .unwrap();
        assert!(max_error <= 1, "RGBA error at {scale}x: {max_error}");

        renderer.set_rectangle_optimization_enabled(true);
        let before = renderer.rectangle_batch_draws();
        let groups = renderer.opacity_atlas_stats().groups;
        let batched = renderer
            .snapshot(&scene, width, height, scale, &input)
            .unwrap();
        assert_eq!(renderer.rectangle_batch_draws() - before, 1);
        assert_eq!(renderer.opacity_atlas_stats().groups, groups);
        let check = |actual: &xrack_renderer_femtovg::Snapshot,
                     expected: &xrack_renderer_femtovg::Snapshot| {
            assert_eq!(actual.layout, expected.layout);
            for (index, (actual, expected)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "batched opacity at {scale}x, pixel ({}, {}), channel {}: {actual} vs {expected}",
                    index / 4 % width as usize,
                    index / 4 / width as usize,
                    index % 4,
                );
            }
        };
        check(&batched, &original);

        renderer.set_rectangle_optimization_enabled(false);
        let clipped_original = renderer
            .snapshot(&clipped, width, height, scale, &input)
            .unwrap();
        renderer.set_rectangle_optimization_enabled(true);
        let before = renderer.rectangle_batch_draws();
        let clipped_batch = renderer
            .snapshot(&clipped, width, height, scale, &input)
            .unwrap();
        assert!(renderer.rectangle_batch_draws() > before);
        check(&clipped_batch, &clipped_original);
    }
}

#[test]
fn fractional_square_backgrounds_batch_without_reusing_pixel_phases() {
    use xrack_runtime::{Color, Length, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0x3145_67ff).into());
    for index in 0..24 {
        let x = if index == 1 {
            // Blend through the first tile at a different pixel phase.
            13.25
        } else {
            4.125 + f64::from(index % 6) * 16.0 + f64::from(index) / 128.0
        };
        let y = 4.375 + f64::from(index / 6) * 16.0 - f64::from(index) / 256.0;
        let width = 12.0 + f64::from(index % 7) / 64.0;
        let height = 12.0 + f64::from(index % 5) / 32.0;
        let mut cell = Node::new("Rectangle", None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            cell.properties.insert(name, Length(value).into());
        }
        let mut background = Node::new("Rectangle", None);
        background.properties.insert("width", Length(width).into());
        background
            .properties
            .insert("height", Length(height).into());
        background.properties.insert(
            "background",
            Color(if index % 2 == 0 {
                0xec78_47b9
            } else {
                0x39b5_ceaa
            })
            .into(),
        );
        cell.children.push(background);
        scene.children.push(cell);
    }
    let input = Interaction::default();
    for scale in [1.0, 1.25, 2.0] {
        let (width, height) = ((102.0 * scale) as u32, (72.0 * scale) as u32);
        renderer.set_rectangle_optimization_enabled(true);
        renderer.set_display_batching_enabled(true);
        let before = renderer.rectangle_batch_draws();
        let actual = renderer
            .snapshot(&scene, width, height, scale, &input)
            .unwrap();
        assert_eq!(renderer.rectangle_batch_draws() - before, 1);
        renderer.set_rectangle_optimization_enabled(false);
        renderer.set_display_batching_enabled(false);
        let expected = renderer
            .snapshot(&scene, width, height, scale, &input)
            .unwrap();
        assert_eq!(actual.layout, expected.layout);
        let (index, max_error) = actual
            .rgba
            .iter()
            .zip(&expected.rgba)
            .map(|(actual, expected)| actual.abs_diff(*expected))
            .enumerate()
            .max_by_key(|(_, error)| *error)
            .unwrap();
        assert!(
            max_error <= 1,
            "fractional fill at {scale}x differs by {max_error} at ({}, {}) channel {}: {} vs {}",
            index / 4 % width as usize,
            index / 4 / width as usize,
            index % 4,
            actual.rgba[index],
            expected.rgba[index]
        );
    }
}

#[test]
fn shadow_masks_preserve_fractional_coverage_across_device_scales()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::LayerMotion;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Color, Component, Length};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut cached = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_layer_budget(0);
    let input = Interaction::default();
    let mut ui = LayerMotion::new();
    ui.set_shadowed(true);
    ui.set_card_width(Length(49.35));
    ui.set_fade(0.83);

    for (scale, alpha, new_masks) in [
        (1.0_f32, 0xff, 4),
        (1.4, 0x66, 4),
        (2.0, 0xff, 4),
        (2.0, 0x66, 1),
    ] {
        ui.set_shadow_tint(Color(0x110d_3500 | alpha));
        let width = (124.0 * scale).round() as u32;
        let height = (96.0 * scale).round() as u32;
        let before = cached.shadow_mask_stats().rasterized;
        for x in [22.0, 23.125, 24.25, 25.431] {
            ui.set_offset(Length(x));
            let scene = ui.render_shared();
            let actual = cached.snapshot(&scene, width, height, scale, &input)?;
            let expected = direct.snapshot(&scene, width, height, scale, &input)?;
            assert_eq!(actual.layout, expected.layout);
            for (index, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 2,
                    "scale={scale} x={x} pixel ({}, {}) channel {}: {a} vs {b}",
                    index / 4 % width as usize,
                    index / 4 / width as usize,
                    index % 4,
                );
            }
        }
        assert_eq!(cached.shadow_mask_stats().rasterized - before, new_masks);
    }
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn transient_opacity_atlas_preserves_order_across_an_interleaved_capture()
-> Result<(), Box<dyn std::error::Error>> {
    use rectangle_animation_ui::TransientOpacityAtlasDemo;
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Component, Length, Node, update_profile};

    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut interpreted = Renderer::with_device(gpu.clone())?;
    interpreted.set_atlas_batching_enabled(false);
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_opacity_atlas_enabled(false);
    let input = Interaction::default();
    let snapshot = |renderer: &mut Renderer, scene: &Node| {
        let mut result = None;
        let sample = update_profile::measure(|| {
            result = Some(renderer.snapshot(scene, 256, 192, 2.0, &input));
        })
        .unwrap();
        result.unwrap().map(|frame| (frame, sample.gpu_submissions))
    };
    let mut aot = TransientOpacityAtlasDemo::new();
    let program = interpret_example("tests/rectangle_animation.xr");
    let interpreter = program.create("TransientOpacityAtlasDemo")?;
    for (revision, x, groups, submissions) in [
        (0, 8.0, 0, 0),
        (1, 10.25, 0, 0),
        (2, -6.375, 2, 2),
        (3, 23.125, 2, 1),
    ] {
        aot.set_revision(revision);
        aot.set_first_x(Length(x));
        interpreter.set("revision", revision.into())?;
        interpreter.set("first-x", Length(x).into())?;
        let scene = aot.render_shared();
        let before = optimized.opacity_atlas_stats();
        let (actual, batched_submissions) = snapshot(&mut optimized, &scene)?;
        let after = optimized.opacity_atlas_stats();
        assert_eq!(after.groups - before.groups, groups, "revision={revision}");
        assert_eq!(
            after.submissions - before.submissions,
            submissions,
            "revision={revision}"
        );
        let (native, separate_submissions) = snapshot(&mut interpreted, &interpreter.render())?;
        if groups > 0 {
            assert!(
                batched_submissions < separate_submissions,
                "revision={revision} batched={batched_submissions} separate={separate_submissions}"
            );
        }
        assert_same_pixels(&actual, &native);
        let expected = direct.snapshot(&scene, 256, 192, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        for (index, (a, b)) in actual.rgba.iter().zip(&expected.rgba).enumerate() {
            assert!(
                a.abs_diff(*b) <= 2,
                "revision={revision} pixel ({}, {}) channel {}: {a} vs {b}",
                index / 4 % 256,
                index / 4 / 256,
                index % 4
            );
        }
    }
    assert!(optimized.rounded_rectangle_batch_draws() > 0);
    Ok(())
}

#[test]
#[ignore = "manual GPU measurement of animated groups with static contents"]
fn benchmark_translucent_card_animation() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        rc::Rc,
        time::{Duration, Instant},
    };
    use xrack_runtime::{
        Color, Component, Image, ImageFit, ImageFormat, Length, LoadedImage, Node,
    };

    struct Cards;
    impl Component for Cards {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let sample_frames = std::env::var_os("XRACK_CARD_BENCH_SAMPLE_FRAMES").is_some();
    if sample_frames {
        println!(
            "layout node bytes={}",
            size_of::<xrack_layout::LayoutNode>()
        );
    }
    let shadow_mode = std::env::var("XRACK_CARD_BENCH_SHADOW").ok();
    let shadow = shadow_mode.is_some();
    let fading = shadow_mode.as_deref() != Some("opaque");
    let dynamic = std::env::var_os("XRACK_CARD_BENCH_DYNAMIC").is_some();
    let scroll_progress = std::env::var_os("XRACK_CARD_BENCH_SCROLL_PROGRESS").is_some();
    let moving_clip = scroll_progress && std::env::var_os("XRACK_CARD_BENCH_MOVING_CLIP").is_some();
    let static_scroll_content =
        scroll_progress && std::env::var_os("XRACK_CARD_BENCH_STATIC_SCROLL_CONTENT").is_some();
    let painted_clip =
        scroll_progress && std::env::var_os("XRACK_CARD_BENCH_PAINTED_CLIP").is_some();
    let dense_grid = scroll_progress && std::env::var_os("XRACK_CARD_BENCH_DENSE_GRID").is_some();
    let opaque_overlay =
        painted_clip && std::env::var_os("XRACK_CARD_BENCH_OPAQUE_OVERLAY").is_some();
    let viewport_background = if opaque_overlay {
        0xeef4ffff
    } else {
        0xffffffff
    };
    let steady_scroll =
        scroll_progress && std::env::var_os("XRACK_CARD_BENCH_STEADY_SCROLL").is_some();
    let live_progress =
        scroll_progress || std::env::var_os("XRACK_CARD_BENCH_LIVE_PROGRESS").is_some();
    let live_stripe = std::env::var_os("XRACK_CARD_BENCH_LIVE_STRIPE").is_some();
    let cache_layers = std::env::var_os("XRACK_CARD_BENCH_CACHE_LAYERS").is_some();
    let rounded_clip = std::env::var_os("XRACK_CARD_BENCH_ROUNDED_CLIP").is_some();
    let clipped = rounded_clip || std::env::var_os("XRACK_CARD_BENCH_CLIPPED").is_some();
    let nested = clipped || std::env::var_os("XRACK_CARD_BENCH_NESTED").is_some();
    let widgets = nested || std::env::var_os("XRACK_CARD_BENCH_WIDGETS").is_some();
    let mixed = widgets || std::env::var_os("XRACK_CARD_BENCH_MIXED").is_some();
    let subpixel = std::env::var("XRACK_CARD_BENCH_SUBPIXEL").ok();
    let card_count = std::env::var("XRACK_CARD_BENCH_COUNT")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(24);
    if !(1..=24).contains(&card_count) {
        return Err("XRACK_CARD_BENCH_COUNT must be between 1 and 24".into());
    }

    let rect = |[x, y, width, height, radius]: [f64; 5], color| {
        let mut node = Node::new("Rectangle", None);
        for (name, length) in [
            ("x", x),
            ("y", y),
            ("width", width),
            ("height", height),
            ("border-radius", radius),
        ] {
            node.properties.insert(name, Length(length).into());
        }
        node.properties.insert("background", Color(color).into());
        node
    };
    let mut scene = Node::new("Window", None);
    scene
        .properties
        .insert("background", Color(0xffff_ffff).into());
    for index in 0..card_count {
        let width = if shadow { 94.0 } else { 100.0 };
        let mut card = rect([0.0, 0.0, width, 93.0, 8.0], 0x2638_54ff);
        if cache_layers {
            card.properties.insert("cache-rendering-hint", true.into());
        }
        if shadow {
            card.properties
                .insert("drop-shadow-blur", Length(12.0).into());
            card.properties
                .insert("drop-shadow-offset-y", Length(4.0).into());
            card.properties
                .insert("drop-shadow-color", Color(0x1017_2666).into());
        }
        let mut title = Node::new("Text", None);
        for (name, value) in [
            ("x", 7.0),
            ("y", 5.0),
            ("width", 88.0),
            ("height", 16.0),
            ("font-size", 12.0),
        ] {
            title.properties.insert(name, Length(value).into());
        }
        title
            .properties
            .insert("font-family", "Menlo".to_owned().into());
        title
            .properties
            .insert("text", "CPU load".to_owned().into());
        title.mark_paint_only("text");
        title.properties.insert("color", Color(0xffff_ffff).into());
        card.children.push(title);
        for dot in 0..25 {
            let x = (dot % 5 * 18 + 7) as f64;
            let y = (dot / 5 * 12 + 26) as f64;
            let color = [0x659b_e7ff, 0xc778_9aff, 0x5eb9_9eff, 0xf2bc_60ff][(index + dot) % 4];
            card.children.push(rect([x, y, 14.0, 9.0, 2.0], color));
        }
        scene.children.push(card);
    }
    if mixed {
        let mut divider = rect([5.0, 196.0, 630.0, 41.0, 4.0], 0x722a648c);
        divider.set_static_id(Some("divider"));
        scene.children.insert(card_count / 2, divider);
    }
    if widgets {
        let mut label = Node::new("Label", Some("status"));
        for (name, value) in [
            ("x", 28.0),
            ("y", 208.0),
            ("width", 176.0),
            ("height", 18.0),
            ("font-size", 12.0),
        ] {
            label.properties.insert(name, Length(value).into());
        }
        label
            .properties
            .insert("text", "Ready · 24 panels".to_owned().into());
        label
            .properties
            .insert("font-family", "Menlo".to_owned().into());
        label.properties.insert("color", Color(0xffffffff).into());
        label.properties.insert(
            "vertical-alignment",
            xrack_runtime::TextVerticalAlignment::Top.into(),
        );

        let mut image = Node::new("Image", Some("badge"));
        for (name, value) in [
            ("x", 560.0),
            ("y", 202.0),
            ("width", 32.0),
            ("height", 24.0),
        ] {
            image.properties.insert(name, Length(value).into());
        }
        let source = Image::Loaded(LoadedImage::new(
            ImageFormat::Png,
            std::fs::read("examples/media/assets/color-mask.png")?,
            [16, 12],
        ));
        image.properties.insert("source", source.into());
        image.properties.insert("image-fit", ImageFit::Fill.into());
        let after_divider = card_count / 2 + 1;
        scene
            .children
            .splice(after_divider..after_divider, [label, image]);
    }
    if nested {
        let last_cards = scene.children.split_off(card_count / 2 + 3);
        let decorations = scene.children.split_off(card_count / 2);
        let panel = |children, y, height| {
            let mut panel = Node::new("Rectangle", Some("panel"));
            let width = if clipped { 629.0 } else { 640.0 };
            for (name, value) in [("y", y), ("width", width), ("height", height)] {
                panel.properties.insert(name, Length(value).into());
            }
            if clipped {
                panel.properties.insert("clip", true.into());
            }
            if rounded_clip {
                panel
                    .properties
                    .insert("border-radius", Length(32.0).into());
            }
            panel.children = children;
            panel
        };
        let first = panel(std::mem::take(&mut scene.children), 0.0, 220.0);
        scene.children.push(first);
        scene.children.extend(decorations);
        scene.children.push(panel(last_cards, 220.0, 260.0));
    }
    if live_stripe {
        let mut status = rect([90.0, 417.0, 460.0, 10.0, 0.0], 0x68c8aa9c);
        status.set_static_id(Some("live-stripe"));
        scene.children.insert(card_count / 2, status);
    }
    if live_progress {
        let mut status = Node::new("ProgressIndicator", Some("live-progress"));
        for (name, value) in [
            ("x", if scroll_progress { 8.0 } else { 90.0 }),
            ("y", if scroll_progress { 8.0 } else { 417.0 }),
            ("width", 460.0),
            ("height", 10.0),
        ] {
            status.properties.insert(name, Length(value).into());
        }
        status.properties.insert("color", Color(0x68c8aaff).into());
        status
            .properties
            .insert("track-color", Color(0x17263bac).into());
        scene.children.push(status);
    }
    if scroll_progress {
        let mut viewport = Node::new("Rectangle", None);
        viewport.properties.insert("clip", true.into());
        viewport
            .properties
            .insert("border-radius", Length(32.0).into());
        if painted_clip {
            viewport
                .properties
                .insert("background", Color(viewport_background).into());
        }
        let mut scroll = Node::new("Flickable", None);
        for (name, value) in [
            ("width", 638.0),
            ("height", 450.0),
            ("content-height", 480.0),
        ] {
            scroll.properties.insert(name, Length(value).into());
        }
        viewport.properties.insert("width", Length(638.0).into());
        viewport.properties.insert("height", Length(450.0).into());
        scroll.children = std::mem::take(&mut scene.children);
        viewport.children.push(scroll);
        scene.children.push(viewport);
    }
    if dense_grid {
        for index in 0..288 {
            let x = (index % 48 * 13 + 8) as f64;
            let y = (index / 48 * 4 + 452) as f64;
            let color = [0x4f77_8cff, 0xab8d_61ff, 0x6d8b_74ff, 0x745a_8aff][index % 4];
            scene.children.push(rect([x, y, 8.0, 3.0, 0.0], color));
        }
    }
    if opaque_overlay {
        scene
            .children
            .push(rect([350.0, 44.0, 288.0, 306.0, 0.0], 0x52637aff));
    }
    let update_card = |card: &mut Node, index: usize, origin_y: f64, frame: usize, moving| {
        let (dx, dy) = if moving {
            if let Some(mode) = &subpixel {
                let phase_offset = if mode == "independent" {
                    (index % 4) as f64 * 0.63
                } else {
                    0.0
                };
                let phase = frame as f64 * 0.14 + phase_offset;
                (2.0 + 1.75 * phase.sin(), 2.0 + 1.65 * phase.cos())
            } else {
                (
                    ((frame + index) % 5) as f64,
                    ((frame / 3 + index) % 5) as f64,
                )
            }
        } else {
            (0.0, 0.0)
        };
        let (left, top, stride_x, stride_y) = if shadow {
            (22, 22, 100, 106)
        } else {
            (2, 6, 105, 111)
        };
        let x = ((index % 6) * stride_x + left) as f64 + dx;
        let y = ((index / 6) * stride_y + top) as f64 + dy - origin_y;
        card.properties.insert("x", Length(x).into());
        card.properties.insert("y", Length(y).into());
        card.properties.insert(
            "opacity",
            (if fading {
                0.55 + ((frame + index) % 5) as f64 * 0.08
            } else {
                1.0
            })
            .into(),
        );
        if dynamic {
            card.children[0]
                .properties
                .insert("text", format!("CPU {frame:02}%").into());
        }
    };
    let move_viewport = |scene: &mut Node, frame, moving| {
        if moving_clip {
            let y = if moving {
                (frame % 31) as f64 * 0.5
            } else {
                0.0
            };
            scene.children[0].properties.insert("y", Length(y).into());
        }
    };
    let update = |scene: &mut Node, frame, moving| {
        move_viewport(scene, frame, moving);

        let mut index = 0;
        let children = if scroll_progress {
            &mut scene.children[0].children[0].children
        } else {
            &mut scene.children
        };
        for node in children {
            match node.element_id() {
                None => {
                    update_card(node, index, 0.0, frame, moving);
                    index += 1;
                }
                Some("panel") => {
                    let y = f64::from(node.length("y").unwrap());
                    for card in &mut node.children {
                        update_card(card, index, y, frame, moving);
                        index += 1;
                    }
                }
                Some("live-progress") => {
                    node.properties
                        .insert("progress", (0.2 + (frame % 60) as f64 / 90.0).into());
                }
                Some("live-stripe") => {
                    node.properties
                        .insert("width", Length(92.0 + (frame % 60) as f64 * 5.0).into());
                    node.properties
                        .insert("opacity", (0.55 + (frame % 5) as f64 * 0.08).into());
                }
                Some(_) => {}
            }
        }
    };
    let viewport = Size::new(640.0, 480.0);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    renderer
        .set_opacity_atlas_enabled(std::env::var_os("XRACK_CARD_BENCH_NO_OPACITY_ATLAS").is_none());
    renderer.set_opacity_tile_sharing_enabled(
        std::env::var_os("XRACK_CARD_BENCH_NO_TILE_SHARING").is_none(),
    );
    renderer.set_atlas_batching_enabled(std::env::var_os("XRACK_CARD_BENCH_EAGER_ATLAS").is_none());
    renderer.set_cached_layer_compositing_enabled(
        std::env::var_os("XRACK_CARD_BENCH_NO_LAYER_COMPOSITING").is_none(),
    );
    renderer.set_hinted_layer_sharing_enabled(
        std::env::var_os("XRACK_CARD_BENCH_NO_LAYER_SHARING").is_none(),
    );
    renderer.set_fractional_rounded_tile_retention_enabled(
        std::env::var_os("XRACK_CARD_BENCH_NO_ROUNDED_RETENTION").is_none(),
    );
    renderer.set_capsule_warmup_enabled(
        std::env::var_os("XRACK_CARD_BENCH_NO_CAPSULE_WARMUP").is_none(),
    );
    let mut input = Interaction::default();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Card animation benchmark"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 960,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    update(&mut scene, 0, false);
    let start = Instant::now();
    let proof = renderer.snapshot(&scene, 1280, 960, 2.0, &input)?;
    if dense_grid {
        assert_eq!(proof.pixel(18, 906), Some([0x4f, 0x77, 0x8c, 0xff]));
    }
    if opaque_overlay {
        assert_eq!(proof.pixel(1024, 200), Some([0x52, 0x63, 0x7a, 0xff]));
    }
    if live_progress {
        let point = if scroll_progress {
            (144, 26)
        } else {
            (208, 844)
        };
        assert_eq!(proof.pixel(point.0, point.1), Some([104, 200, 170, 255]));
    }
    if rounded_clip {
        assert_eq!(proof.pixel(9, 21), Some([255; 4]));
    }
    println!(
        "card animation first frame: {:.3}ms, layer captures={}",
        start.elapsed().as_secs_f64() * 1000.0,
        renderer.opacity_layer_stats().rasterized
    );
    for (point, source) in [
        ((64, 54), [0x26_u8, 0x38, 0x54]),
        ((30, 69), [0x65, 0x9b, 0xe7]),
    ] {
        let point = if shadow {
            (point.0 + 40, point.1 + 32)
        } else {
            point
        };
        let actual = proof.pixel(point.0, point.1).ok_or("missing card pixel")?;
        for ((actual, source), background) in actual
            .iter()
            .zip(source)
            .zip(viewport_background.to_be_bytes())
        {
            let opacity = if fading { 0.55 } else { 1.0 };
            let expected = (f32::from(source) * opacity + f32::from(background) * (1.0 - opacity))
                .round() as u8;
            assert!(
                actual.abs_diff(expected) <= 2,
                "{point:?}: {actual} vs {expected}"
            );
        }
    }
    if shadow {
        let shadow_pixel = proof.pixel(138, 246).ok_or("missing shadow pixel")?;
        assert!(
            shadow_pixel[0] < 250 && shadow_pixel[0] > 150,
            "{shadow_pixel:?}"
        );
    }
    let mut base = Rc::new(scene);
    let mut previous_layout = proof.layout;
    let mut paint_frame = |renderer: &mut Renderer,
                           base: &mut Rc<Node>,
                           frame,
                           moving|
     -> Result<[Duration; 4], Box<dyn std::error::Error>> {
        let start = Instant::now();
        if scroll_progress && moving {
            let offset = (frame % 24) as f32 * 0.5 + (frame % 2) as f32 * 0.25;
            input.accessibility_scroll_to(
                &previous_layout,
                &[0, 0],
                xrack_runtime::Orientation::Vertical,
                offset,
            );
        }
        if static_scroll_content {
            move_viewport(Rc::make_mut(base), frame, moving);
        } else {
            update(Rc::make_mut(base), frame, moving);
        }
        let updated = if sample_frames {
            start.elapsed()
        } else {
            Duration::ZERO
        };
        let (scene, layout) = renderer.prepare_scene(&Cards, base.clone(), viewport, &input);
        let prepared = start.elapsed();
        renderer.draw_with_layout(&scene, &layout, &texture, 2.0, &input)?;
        let submitted = start.elapsed();
        renderer.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        if scroll_progress {
            previous_layout = layout;
        }
        Ok([prepared, submitted - prepared, start.elapsed(), updated])
    };
    for moving in [false, true] {
        if moving && steady_scroll {
            // Populate the recurring scroll and fill phases before timing.
            for frame in 0..120 {
                paint_frame(&mut renderer, &mut base, frame, true)?;
            }
        }
        let before = renderer.opacity_layer_stats();
        let before_masks = renderer.shadow_mask_stats();
        let before_atlas = renderer.opacity_atlas_stats();
        let mut samples = [Vec::new(), Vec::new(), Vec::new()];
        let mut rounded_tiles = 0;
        let mut frame_samples = Vec::new();
        for frame in 0..70 {
            let elapsed = paint_frame(&mut renderer, &mut base, frame, moving)?;
            let tiles = renderer.rounded_tile_rasterizations();
            if sample_frames {
                frame_samples.push((frame, tiles, elapsed));
            }
            if frame >= 10 {
                rounded_tiles += tiles;
                for (series, duration) in samples.iter_mut().zip(elapsed) {
                    series.push(duration);
                }
            }
        }
        for (frame, tiles, [prepared, submitted, total, updated]) in frame_samples {
            let wait = total.saturating_sub(prepared + submitted);
            let layout = prepared - updated;
            println!(
                "card frame moving={moving} frame={frame} tiles={tiles} total_us={} prepare_us={} submit_us={} wait_us={} update_us={} layout_us={}",
                total.as_micros(),
                prepared.as_micros(),
                submitted.as_micros(),
                wait.as_micros(),
                updated.as_micros(),
                layout.as_micros()
            );
        }
        let after = renderer.opacity_layer_stats();
        let after_masks = renderer.shadow_mask_stats();
        let after_atlas = renderer.opacity_atlas_stats();
        println!("card animation moving={moving} rounded tiles rasterized={rounded_tiles}");
        println!(
            "card animation moving={moving} dynamic={dynamic} rasterizations={} reuses={}",
            after.rasterized - before.rasterized,
            after.reused - before.reused
        );
        if shadow {
            println!(
                "card animation moving={moving} shadow masks rasterized={} reused={}",
                after_masks.rasterized - before_masks.rasterized,
                after_masks.reused - before_masks.reused
            );
        }
        println!(
            "card animation moving={moving} opacity atlas groups={} submissions={}",
            after_atlas.groups - before_atlas.groups,
            after_atlas.submissions - before_atlas.submissions
        );
        println!(
            "card animation moving={moving} opacity tiles reused={}",
            after_atlas.reused_tiles - before_atlas.reused_tiles,
        );
        for (stage, samples) in ["prepare", "submit", "total"].into_iter().zip(&mut samples) {
            samples.sort_unstable();
            println!(
                "card animation moving={moving} {stage}: p50={:.3}ms p95={:.3}ms",
                samples[29].as_secs_f64() * 1000.0,
                samples[56].as_secs_f64() * 1000.0
            );
        }
    }
    if shadow {
        for cache in [false, true] {
            let budget = if cache { 32 * 1024 * 1024 } else { 0 };
            let mut samples = Vec::new();
            for frame in 0..24 {
                renderer.set_layer_budget(budget);
                let [_, _, total, _] = paint_frame(&mut renderer, &mut base, frame, true)?;
                if frame >= 4 {
                    samples.push(total);
                }
            }
            samples.sort_unstable();
            println!(
                "shadow frame with empty layer cache enabled={cache}: p50={:.3}ms p95={:.3}ms",
                samples[9].as_secs_f64() * 1000.0,
                samples[18].as_secs_f64() * 1000.0,
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "manual GPU measurement of clipped long rich-text output"]
fn benchmark_long_styled_text_paint() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    use xrack_runtime::Orientation;

    let size = Size::new(640.0, 480.0);
    let scene = long_styled_text_scene(size, 1200)?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Long rich-text paint benchmark"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 960,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    for enabled in [false, true] {
        renderer.set_styled_text_culling_enabled(enabled);
        let mut input = Interaction::default();
        let mut layout = renderer.layout_with_input(&scene, size, &input);
        let mut prepare = Vec::with_capacity(40);
        let mut submission = Vec::with_capacity(40);
        let mut completed = Vec::with_capacity(40);
        let before = renderer.styled_text_culled_fragments();
        for index in 0..50 {
            let start = Instant::now();
            assert!(input.accessibility_scroll_to(
                &layout,
                &[0],
                Orientation::Vertical,
                720.0 + index as f32 * 62.0,
            ));
            layout = renderer.layout_with_input(&scene, size, &input);
            let prepared = start.elapsed();
            renderer.draw_with_layout(&scene, &layout, &texture, 2.0, &input)?;
            let submitted = start.elapsed();
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            if index == 0 {
                println!(
                    "long rich-text culling={enabled} first frame after layout: {:.2}ms",
                    (start.elapsed() - prepared).as_secs_f64() * 1000.0
                );
            } else if index >= 10 {
                prepare.push(prepared);
                submission.push(submitted - prepared);
                completed.push(start.elapsed());
            }
        }
        for (stage, samples) in [
            ("layout", &mut prepare),
            ("submit", &mut submission),
            ("total", &mut completed),
        ] {
            samples.sort_unstable();
            println!(
                "long rich-text culling={enabled} 1200 paragraphs {stage}: p50={:.2}ms p95={:.2}ms",
                samples[19].as_secs_f64() * 1000.0,
                samples[37].as_secs_f64() * 1000.0,
            );
        }
        let culled = renderer.styled_text_culled_fragments() - before;
        println!("long rich-text culling={enabled} skipped fragments: {culled}");
        if enabled && !cfg!(any(target_os = "ios", target_os = "android")) {
            assert!(culled > 100_000);
        }
    }
    Ok(())
}

#[test]
fn styled_text_incremental_layout_matches_fresh_renders_after_edits()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{MarkdownStream, Orientation, StyledText};

    let size = Size::new(640.0, 480.0);
    let prefix = long_styled_text_markdown(200)?;
    let mut scene = long_styled_text_scene(size, 200)?;
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut renderer = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_styled_text_incremental_enabled(false);

    let original = format!("{prefix}尾段 · **build** · שלום");
    let appended = format!("{original}，接着输出 \r\n\r\n[结尾](end.xr#15) · 😀");
    let latin = "test **bidirectional layout** before\u{2029}after a separator\n\n".repeat(200);
    let mixed = format!("{latin}first right-to-left paragraph: שלום");
    let cases = [
        (original, false),
        (appended.clone(), true),
        (format!("{appended} · 完成报告"), true),
        (appended.replacen("**0000**", "*0000*", 1), false),
        (
            appended.replacen("example.invalid/0", "example.invalid/updated", 1),
            false,
        ),
        (latin, false),
        (mixed, true),
    ];
    let mut stream = MarkdownStream::default();
    for (source, should_reuse) in cases {
        scene.children[0].children[0]
            .properties
            .insert("text", stream.update(&source)?.into());
        let mut reference = scene.clone();
        reference.children[0].children[0]
            .properties
            .insert("text", StyledText::from_markdown(&source)?.into());
        let before = renderer.styled_text_reused_fragments();
        let mut input = Interaction::default();
        let layout = renderer.layout_with_input(&scene, size, &input);
        let end = layout
            .at_path(&[0])
            .and_then(|viewport| viewport.scroll_on(Orientation::Vertical))
            .ok_or("missing rich-text viewport")?
            .max_offset;
        for offset in [720.25, end - 2.25] {
            assert!(input.accessibility_scroll_to(&layout, &[0], Orientation::Vertical, offset));
            let optimized = renderer.snapshot(&scene, 1280, 960, 2.0, &input)?;
            let expected = direct.snapshot(&reference, 1280, 960, 2.0, &input)?;
            assert_eq!(optimized.layout, expected.layout);
            assert_same_pixels(&optimized, &expected);
        }
        assert_eq!(
            renderer.styled_text_reused_fragments() > before,
            should_reuse
        );
    }
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn styled_text_reuses_selection_geometry_on_resize_and_invalidates_on_font_change()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_layout::{LayoutNode, VisualTextLine};
    use xrack_runtime::Length;

    fn first_shape(layout: &LayoutNode) -> &VisualTextLine {
        let plan = layout
            .at_path(&[0, 0])
            .unwrap()
            .styled_text
            .as_ref()
            .unwrap();
        let (shape, range, _) = plan.fragments[0].selection_geometry().unwrap();
        assert_eq!(range, 0..4);
        shape
    }

    let size = Size::new(640.0, 480.0);
    let mut scene = long_styled_text_scene(size, 200)?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let initial = renderer.layout_with_input(&scene, size, &input);

    scene.children[0].children[0]
        .properties
        .insert("width", Length(280.0).into());
    let narrow = renderer.layout_with_input(&scene, size, &input);
    assert!(std::ptr::eq(first_shape(&initial), first_shape(&narrow)));
    scene.children[0].children[0]
        .properties
        .insert("width", Length(440.0).into());
    let wide = renderer.layout_with_input(&scene, size, &input);
    assert!(std::ptr::eq(first_shape(&narrow), first_shape(&wide)));

    scene.children[0].children[0]
        .properties
        .insert("font-size", Length(24.0).into());
    let larger = renderer.layout_with_input(&scene, size, &input);
    assert_ne!(first_shape(&wide), first_shape(&larger));
    Ok(())
}

#[test]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn large_styled_text_keeps_culling_and_selection_when_the_tail_changes()
-> Result<(), Box<dyn std::error::Error>> {
    use xrack_renderer_femtovg::RenderDevice;
    use xrack_runtime::{Length, Orientation, StyledText, TextCommand, ViewportHandle};

    let size = Size::new(480.0, 320.0);
    let mut source = long_styled_text_markdown(6000)?;
    let mut scene = long_styled_text_scene(size, 6000)?;
    let handle = ViewportHandle::default();
    handle.bind_offset(
        Orientation::Vertical,
        || Length(-f64::from(f32::MAX)),
        |_| {},
    );
    scene.children[0].viewport_handle = Some(handle);
    let gpu = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut optimized = Renderer::with_device(gpu.clone())?;
    let mut direct = Renderer::with_device(gpu)?;
    direct.set_styled_text_culling_enabled(false);
    direct.set_styled_text_incremental_enabled(false);

    for appended in [false, true] {
        if appended {
            source.push_str("**Latest update** · [build](out.xr:4) · שלום 🌐\n");
            scene.children[0].children[0]
                .properties
                .insert("text", StyledText::from_markdown(&source)?.into());
        }
        let culling_before = optimized.styled_text_culled_fragments();
        let reuse_before = optimized.styled_text_reused_fragments();
        let mut input = Interaction::default();
        let layout = optimized.layout_with_input(&scene, size, &input);
        let target = input
            .accessibility_target(&scene, &layout, &[0, 0])
            .ok_or("missing text selection target")?;
        input.focus_target(&scene, &layout, target);
        input.sync_editor(&scene);
        let text = scene.children[0].children[0]
            .text("text")
            .ok_or("missing rich text")?;
        let anchor = text
            .trim_end()
            .rfind('\n')
            .ok_or("missing final paragraph")?
            + 1;
        input.apply_text_selection(
            &scene,
            TextCommand::SetSelection {
                anchor: anchor as i64,
                focus: text.len() as i64,
            },
        );
        let actual = optimized.snapshot(&scene, 960, 640, 2.0, &input)?;
        let expected = direct.snapshot(&scene, 960, 640, 2.0, &input)?;
        assert_eq!(actual.layout, expected.layout);
        assert_same_pixels(&actual, &expected);
        assert!(optimized.styled_text_culled_fragments() - culling_before > 32_768);
        if appended {
            assert!(optimized.styled_text_reused_fragments() - reuse_before > 32_768);
        }
    }
    Ok(())
}

#[test]
#[ignore = "manual timing of rich-text links and selection near the end of a document"]
fn benchmark_long_styled_text_interaction() -> Result<(), Box<dyn std::error::Error>> {
    use std::{hint::black_box, time::Instant};
    use xrack_layout::{TextAreaMotion, TextEdit};
    use xrack_runtime::{Length, Orientation, ViewportHandle};

    let size = Size::new(640.0, 480.0);
    let mut scene = long_styled_text_scene(size, 10_000)?;
    let handle = ViewportHandle::default();
    handle.bind_offset(
        Orientation::Vertical,
        || Length(-f64::from(f32::MAX)),
        |_| {},
    );
    scene.children[0].viewport_handle = Some(handle);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    fn measure(stage: &str, mut action: impl FnMut(usize)) {
        let mut samples = Vec::with_capacity(200);
        for index in 0..210 {
            let start = Instant::now();
            action(index);
            if index >= 10 {
                samples.push(start.elapsed());
            }
        }
        samples.sort_unstable();
        println!(
            "{stage}: p50={:.3}ms p95={:.3}ms",
            samples[99].as_secs_f64() * 1000.0,
            samples[189].as_secs_f64() * 1000.0
        );
    }

    for angle in [0.0, 7.0] {
        scene.children[0]
            .properties
            .insert("transform-rotation", angle.into());
        let mut input = Interaction::default();
        let layout = renderer.layout_with_input(&scene, size, &input);
        let item = layout.at_path(&[0, 0]).ok_or("missing rich-text layout")?;
        let plan = item.styled_text.as_ref().ok_or("missing text geometry")?;
        let text = scene.children[0].children[0]
            .text("text")
            .ok_or("missing rich text")?;
        let points: Vec<_> = item
            .text_link_regions()
            .filter_map(|(link, rect)| {
                let center = Point {
                    x: rect.x + rect.width * 0.5,
                    y: rect.y + rect.height * 0.5,
                };
                let window = item.map_to_window(center);
                (item.contains_clip_point(window)
                    && window.x >= 0.0
                    && window.x < size.width
                    && window.y >= 0.0
                    && window.y < size.height)
                    .then_some((
                        link,
                        window,
                        Point {
                            x: center.x - item.bounds.x,
                            y: center.y - item.bounds.y,
                        },
                    ))
            })
            .collect();
        assert!(points.len() > 3 && points[0].0 > 9900);
        let mut editor = TextEdit::default();
        editor.place_caret(
            text,
            plan.selection_hit(points[points.len() / 2].2)
                .ok_or("missing caret")?,
            false,
        );
        println!(
            "rich-text interaction: rotation={angle}, fragments={}, visible links={}",
            plan.fragments.len(),
            points.len()
        );
        measure("link regions", |_| {
            black_box(item.text_link_regions().count());
        });
        measure("pointer move", |i| {
            let (link, point, _) = points[i % points.len()];
            input.pointer_move(&scene, &layout, point);
            assert_eq!(
                input
                    .hovered
                    .as_ref()
                    .and_then(|target| target.text_link_index()),
                Some(link)
            );
        });
        measure("tab focus", |_| {
            black_box(input.focus_next(&scene, &layout, false));
        });
        assert!(
            input
                .focused
                .as_ref()
                .and_then(|target| target.text_link_index())
                .is_some()
        );
        measure("selection hit", |i| {
            black_box(plan.selection_hit(points[i % points.len()].2));
        });
        measure("caret step", |i| {
            let motion = if i % 2 == 0 {
                TextAreaMotion::Right
            } else {
                TextAreaMotion::Left
            };
            assert!(plan.move_selection(text, &mut editor, motion, false));
        });
    }
    Ok(())
}

#[test]
#[ignore = "manual GPU measurement of continuously updated rich-text output"]
fn benchmark_styled_text_append() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fmt::Write as _,
        time::{Duration, Instant},
    };
    use xrack_runtime::{Length, MarkdownStream, Orientation, StyledText, ViewportHandle};

    let paragraphs = std::env::var("XRACK_STYLED_TEXT_PARAGRAPHS")
        .map_or(Ok(1200_usize), |value| value.parse())?;
    if !(100..=12_000).contains(&paragraphs) {
        return Err("XRACK_STYLED_TEXT_PARAGRAPHS must be between 100 and 12000".into());
    }
    let modes: &[bool] = if std::env::var_os("XRACK_STYLED_TEXT_OPTIMIZED_ONLY").is_some() {
        &[true]
    } else {
        &[false, true]
    };
    let follow_latest = std::env::var_os("XRACK_STYLED_TEXT_LATEST").is_some();
    let streaming = std::env::var_os("XRACK_STYLED_TEXT_STREAM").is_some();
    let transform = std::env::var("XRACK_STYLED_TEXT_TRANSFORM").unwrap_or_default();
    if !matches!(transform.as_str(), "" | "rotate" | "zoom") {
        return Err("XRACK_STYLED_TEXT_TRANSFORM must be rotate or zoom".into());
    }
    let size = Size::new(640.0, 480.0);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Rich-text append benchmark"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 960,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    println!(
        "rich-text append: {paragraphs} paragraphs, follow_latest={follow_latest}, transform={transform}, streaming={streaming}"
    );
    for &enabled in modes {
        renderer.set_styled_text_incremental_enabled(enabled);
        let before = renderer.styled_text_reused_fragments();
        let before_culling = renderer.styled_text_culled_fragments();
        let mut source = long_styled_text_markdown(paragraphs)?;
        let mut scene = long_styled_text_scene(size, paragraphs)?;
        let mut stream = streaming.then(MarkdownStream::default);
        if let Some(stream) = &mut stream {
            scene.children[0].children[0]
                .properties
                .insert("text", stream.update(&source)?.into());
        }
        if follow_latest {
            let handle = ViewportHandle::default();
            handle.bind_offset(
                Orientation::Vertical,
                || Length(-f64::from(f32::MAX)),
                |_| {},
            );
            scene.children[0].viewport_handle = Some(handle);
        }
        let mut input = Interaction::default();
        let mut layout = renderer.layout_with_input(&scene, size, &input);
        let mut stats: [Vec<Duration>; 5] = std::array::from_fn(|_| Vec::new());
        for index in 0..35 {
            let start = Instant::now();
            let viewport = &mut scene.children[0];
            match transform.as_str() {
                "rotate" => {
                    let angle = 7.0 * (f64::from(index) * 0.17).sin();
                    viewport
                        .properties
                        .insert("transform-rotation", angle.into());
                }
                "zoom" => {
                    let scale = 0.75 + f64::from(index) / 68.0;
                    viewport.properties.insert("transform-scale", scale.into());
                }
                _ => {}
            }
            writeln!(
                source,
                "**{index}** 继续输出 · `bazel test` · [报告](report-{index}) · שלום\n"
            )?;
            let document = match &mut stream {
                Some(stream) => stream.update(&source)?,
                None => StyledText::from_markdown(&source)?,
            };
            scene.children[0].children[0]
                .properties
                .insert("text", document.into());
            let parsed = start.elapsed();
            if !follow_latest {
                assert!(input.accessibility_scroll_to(
                    &layout,
                    &[0],
                    Orientation::Vertical,
                    720.0 + index as f32 * 33.0,
                ));
            }
            layout = renderer.layout_with_input(&scene, size, &input);
            let prepared = start.elapsed();
            renderer.draw_with_layout(&scene, &layout, &texture, 2.0, &input)?;
            let submitted = start.elapsed();
            renderer.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })?;
            let finished = start.elapsed();
            if index >= 5 {
                for (samples, value) in stats.iter_mut().zip([
                    parsed,
                    prepared - parsed,
                    submitted - prepared,
                    finished - submitted,
                    finished,
                ]) {
                    samples.push(value);
                }
            }
        }
        if follow_latest {
            let scroll = layout.children[0]
                .scroll_on(Orientation::Vertical)
                .ok_or("missing rich-text viewport")?;
            assert!(scroll.max_offset > 0.0 && scroll.offset == scroll.max_offset);
        }
        for (stage, samples) in ["markdown", "layout", "submit", "gpu wait", "total"]
            .into_iter()
            .zip(&mut stats)
        {
            samples.sort_unstable();
            println!(
                "rich-text append incremental={enabled} {stage}: p50={:.2}ms p95={:.2}ms",
                samples[14].as_secs_f64() * 1000.0,
                samples[28].as_secs_f64() * 1000.0
            );
        }
        let reused = renderer.styled_text_reused_fragments() - before;
        println!(
            "rich-text append incremental={enabled} reused fragments: {reused}, culled fragments: {}",
            renderer.styled_text_culled_fragments() - before_culling
        );
        if enabled && paragraphs == 1200 {
            assert!(reused > 100_000);
        }
    }
    Ok(())
}

#[test]
#[ignore = "manual GPU performance measurement"]
fn benchmark_stack_compositing() {
    use std::time::{Duration, Instant};
    use xrack_layout::StackTransitions;
    use xrack_runtime::{Length, Node, StackNavigation};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StackDemo::new();
    let input = Interaction::default();
    let viewport = Size::new(590.0, 324.0);
    let mut outgoing = ui.render();
    let page = outgoing
        .child_at_mut(0)
        .unwrap()
        .child_at_mut(2)
        .unwrap()
        .child_at_mut(0)
        .unwrap();
    for row in 0..14 {
        for column in 0..10 {
            let mut text = Node::new("Text", None);
            text.properties
                .insert("x", Length(4.0 + f64::from(column) * 47.0).into());
            text.properties
                .insert("y", Length(5.0 + f64::from(row) * 15.0).into());
            text.properties
                .insert("text", format!("{row}.{column}").into());
            page.children.push(text);
        }
    }
    let outgoing_layout = renderer.layout_with_input(&outgoing, viewport, &input);
    assert!(ui.navigate_stack("navigation", StackNavigation::Push("details")));
    let incoming = ui.render_shared();
    let incoming_layout = renderer.layout_with_input(&incoming, viewport, &input);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Stack compositor benchmark"),
        size: wgpu::Extent3d {
            width: 1180,
            height: 648,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let mut measure = |budget| {
        renderer.set_layer_budget(budget);
        let stats = renderer.stack_layer_stats();
        let start = Instant::now();
        let mut slides = StackTransitions::default();
        slides.update(
            (&outgoing, &outgoing_layout),
            (&incoming, &incoming_layout),
            start,
        );
        let mut submit = Vec::new();
        let mut completed = Vec::new();
        for step in 0..60 {
            slides.advance(start + Duration::from_millis(1 + step * 3));
            let begin = Instant::now();
            renderer
                .draw_with_transitions(&incoming, &incoming_layout, &texture, 2.0, &input, &slides)
                .unwrap();
            let submitted = begin.elapsed().as_secs_f64() * 1000.0;
            renderer
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            if step >= 8 {
                submit.push(submitted);
                completed.push(begin.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let p95 = |mut values: Vec<f64>| {
            values.sort_by(f64::total_cmp);
            values[(values.len() * 95).div_ceil(100) - 1]
        };
        let end = renderer.stack_layer_stats();
        (
            p95(submit),
            p95(completed),
            end.rasterized - stats.rasterized,
            end.reused - stats.reused,
            end.direct - stats.direct,
        )
    };
    let direct = measure(0);
    let cached = measure(32 * 1024 * 1024);
    assert_eq!((direct.2, direct.3, direct.4), (0, 0, 60));
    assert_eq!((cached.2, cached.3, cached.4), (1, 59, 0));
    println!(
        "stack direct: submit p95 {:.3} ms, GPU completion p95 {:.3} ms",
        direct.0, direct.1
    );
    println!(
        "stack cached: submit p95 {:.3} ms, GPU completion p95 {:.3} ms",
        cached.0, cached.1
    );
}

#[test]
#[ignore = "manual scroll layout performance measurement"]
fn benchmark_scroll_row_layout() {
    use std::time::Instant;
    use xrack_layout::{RowLayoutCache, prepare_scene_with_input, prepare_scene_with_row_cache};
    use xrack_renderer_femtovg::TextSystem;
    use xrack_runtime::{
        Component, Length, ListMetrics, ListRowRenderer, Node, Orientation, VecModel,
    };

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let model = VecModel::from((0..50_000).collect::<Vec<_>>());
    let mut list = Node::new("ListView", None);
    list.properties.insert("row-height", Length(24.0).into());
    list.list = Some(ListMetrics {
        row_count: model.len(),
        first_row: 0,
    });
    list.list_row_renderer = Some(ListRowRenderer::new(move |range| {
        range
            .map(|index| {
                let mut row = Node::new("HorizontalLayout", None);
                row.row_key = model.row_key(index);
                for column in 0..8 {
                    let mut label = Node::new("Text", None);
                    label
                        .properties
                        .insert("text", format!("Record {index}, field {column}").into());
                    row.children.push(label);
                }
                row
            })
            .collect()
    }));
    let view = View(list);
    let base = view.render_shared();
    let viewport = Size::new(1200.0, 1152.0);
    let measure = |cached| {
        let mut text = TextSystem::system().unwrap();
        let mut input = Interaction::default();
        let mut cache = RowLayoutCache::default();
        let mut layout = xrack_layout::layout(&base, viewport, &mut text);
        let mut times = Vec::new();
        for step in 1..=120 {
            assert!(input.accessibility_scroll_to(
                &layout,
                &[],
                Orientation::Vertical,
                step as f32 * 4.0,
            ));
            let start = Instant::now();
            let result = if cached {
                prepare_scene_with_row_cache(
                    &view,
                    base.clone(),
                    viewport,
                    &mut text,
                    &input,
                    &mut cache,
                )
            } else {
                prepare_scene_with_input(&view, base.clone(), viewport, &mut text, &input)
            };
            if step > 20 {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            layout = result.1;
        }
        times.sort_by(f64::total_cmp);
        (times[(times.len() * 95).div_ceil(100) - 1], cache.stats())
    };
    let direct = measure(false);
    let cached = measure(true);
    assert!(cached.1.reused > 5000);
    println!(
        "scroll layout p95 direct {:.3} ms, cached {:.3} ms; {:?}",
        direct.0, cached.0, cached.1
    );
    profile_scroll_frames(
        "dense list",
        &view,
        viewport,
        &[],
        Orientation::Vertical,
        4.0,
        FRAME_PROFILE_COUNT,
    );
    profile_scroll_frames(
        "dense list fast scroll",
        &view,
        viewport,
        &[],
        Orientation::Vertical,
        36.0,
        FRAME_PROFILE_COUNT,
    );
    profile_scroll_frames(
        "wide table",
        &table_ui::WideTableDemo::new(),
        Size::new(620.0, 320.0),
        &[1],
        Orientation::Horizontal,
        4.0,
        FRAME_PROFILE_COUNT,
    );
}

#[test]
#[ignore = "manual GPU measurement of a large table's scrolling"]
fn benchmark_wide_table_scroll() {
    benchmark_wide_table(false);
}

#[test]
#[ignore = "manual GPU measurement of a large table's column resizing"]
fn benchmark_wide_table_resize() {
    benchmark_wide_table(true);
}

fn benchmark_wide_table(resize: bool) {
    use std::rc::Rc;
    use xrack_runtime::{
        Component, ControlAction, LayoutDirection, Length, ListMetrics, ListRowRenderer, Node,
        Orientation, Property, StandardTableRowCache, TableColumnWindow, TableHeader,
        TableRowOrder, TextHorizontalAlignment, VecModel, resize_table_column,
    };

    struct Sheet(Rc<Node>);
    impl Component for Sheet {
        fn render(&self) -> Node {
            (*self.0).clone()
        }
        fn render_shared(&self) -> Rc<Node> {
            self.0.clone()
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let proof = std::env::var_os("XRACK_TABLE_BENCH_PIXELS").is_some();
    let text = std::env::var("XRACK_TABLE_BENCH_TEXT").ok();
    let (mixed, cjk) = match text.as_deref() {
        Some("ascii") => (false, false),
        Some("mixed") => (true, false),
        Some("cjk") => (false, true),
        None => (proof, false),
        Some(value) => panic!("unsupported table benchmark text: {value}"),
    };
    let vertical = std::env::var_os("XRACK_TABLE_BENCH_VERTICAL").is_some();
    for (count, step) in [(512, 137.0), (8192, 1229.0)] {
        let model = Rc::new(VecModel::from((0..50_000).collect::<Vec<usize>>()));
        let rows = TableRowOrder::default().rows(model);
        let mut table = Node::new("TableView", None);
        table.properties.insert("__standard-table", true.into());
        if proof && std::env::var_os("XRACK_TABLE_BENCH_RTL").is_some() {
            table
                .properties
                .insert("layout-direction", LayoutDirection::RightToLeft.into());
        }
        table.properties.insert("frozen-columns", 2_i64.into());
        table.properties.insert("row-height", Length(24.0).into());
        table.properties.insert("resizable-columns", true.into());
        table.list = Some(ListMetrics {
            row_count: 50_000,
            first_row: 0,
        });
        table.table_header = Some(Box::new(TableHeader {
            columns: (0..count)
                .map(|index| {
                    let mut column = Node::new("TableColumn", None);
                    column
                        .properties
                        .insert("width", Length(80.0 + (index % 5) as f64).into());
                    column
                        .properties
                        .insert("title", format!("Field {index}").into());
                    column
                        .properties
                        .insert("visible", (index % 19 != 7).into());
                    column.into_shared()
                })
                .collect(),
            sort: None,
            column_widths: Vec::new(),
        }));
        let cache = StandardTableRowCache::default();
        table.list_row_renderer = Some(ListRowRenderer::with_columns(
            move |range, columns: &TableColumnWindow| {
                let mut projected = cache.render(
                    || rows.clone(),
                    range,
                    columns,
                    None,
                    count,
                    |_| None,
                    |row, column| {
                        if cjk {
                            // Vary the character range across the scrolled rows.
                            let start = 0x4e00 + (row / 24) % 160 * 96;
                            let letters: String = (0..4)
                                .map(|index| {
                                    let offset = (row * 7 + column * 13 + index * 17) % 96;
                                    char::from_u32((start + offset) as u32).unwrap()
                                })
                                .collect();
                            format!("{letters}{}", row % 100)
                        } else if mixed {
                            format!("{row}:{column} שלום · 构建")
                        } else {
                            format!("{row}:{column}")
                        }
                    },
                    |_| None,
                );
                if mixed {
                    for row in &mut projected {
                        for (index, cell) in row.children.iter_mut().enumerate() {
                            if columns.logical_index(index) % 2 == 0 {
                                cell.properties.insert(
                                    "horizontal-alignment",
                                    TextHorizontalAlignment::Center.into(),
                                );
                            }
                        }
                    }
                }
                assert!(projected.len() <= 25);
                assert!(projected.iter().all(|row| row.children.len() <= 15));
                projected
            },
        ));
        let viewport = Size::new(950.0, 580.0);
        let sheet = Sheet(Rc::new(table));
        if resize {
            let widths = Property::default();
            let mut grip = None;
            profile_frames(
                &format!("50,000 rows x {count} columns, resize"),
                &sheet,
                viewport,
                FRAME_PROFILE_COUNT,
                |input, scene, layout, frame| {
                    let (start, direction) = *grip.get_or_insert_with(|| {
                        let table = layout.table.as_ref().unwrap();
                        let column = table.columns[0];
                        let rtl = sheet.0.layout_direction() == Some(LayoutDirection::RightToLeft);
                        let point = xrack_layout::Point {
                            x: if rtl {
                                column.x + 1.0
                            } else {
                                column.x + column.width - 1.0
                            },
                            y: table.header.y + 12.0,
                        };
                        input.pointer_move(scene, layout, point);
                        assert_eq!(input.table_resize_column(&[]), Some(0));
                        input.pointer_press();
                        (point, if rtl { -1.0 } else { 1.0 })
                    });
                    let phase = ((frame - 1) % 60 + 1) as i32;
                    let delta = (30 - (30 - phase).abs()) * 3 + 3;
                    assert_eq!(layout.table.as_ref().unwrap().columns[0].width, {
                        widths
                            .get(Vec::new)
                            .first()
                            .copied()
                            .flatten()
                            .unwrap_or(80) as f32
                    });
                    input.pointer_move(
                        scene,
                        layout,
                        xrack_layout::Point {
                            x: start.x + direction * delta as f32,
                            y: start.y,
                        },
                    );
                    let action = input.table_resize_drag(scene, layout).unwrap().action;
                    assert_eq!(
                        action,
                        ControlAction::ResizeColumn {
                            column: 0,
                            width: (80 + delta) as u32,
                        }
                    );
                    assert!(resize_table_column(&widths, 0, (80 + delta) as u32));
                    let mut table = sheet.render();
                    table.table_header.as_mut().unwrap().column_widths = widths.get(Vec::new);
                    Rc::new(table)
                },
            );
        } else {
            let (axis, step) = if vertical {
                (Orientation::Vertical, 17.0)
            } else {
                (Orientation::Horizontal, step)
            };
            profile_scroll_frames(
                &format!("50,000 rows x {count} columns"),
                &sheet,
                viewport,
                &[],
                axis,
                step,
                if vertical && cjk {
                    1080
                } else {
                    FRAME_PROFILE_COUNT
                },
            );
        }
    }
}

const FRAME_PROFILE_COUNT: usize = 120;

fn profile_scroll_frames(
    label: &str,
    component: &impl xrack_runtime::Component,
    viewport: Size,
    path: &[usize],
    axis: xrack_runtime::Orientation,
    pixels_per_frame: f32,
    frames: usize,
) {
    let base = component.render_shared();
    profile_frames(
        label,
        component,
        viewport,
        frames,
        |input, _, layout, step| {
            assert!(input.accessibility_scroll_to(
                layout,
                path,
                axis,
                step as f32 * pixels_per_frame
            ));
            base.clone()
        },
    );
}

fn profile_frames(
    label: &str,
    component: &impl xrack_runtime::Component,
    viewport: Size,
    frames: usize,
    mut update: impl FnMut(
        &mut Interaction,
        &xrack_runtime::Node,
        &xrack_layout::LayoutNode,
        usize,
    ) -> std::rc::Rc<xrack_runtime::Node>,
) {
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use xrack_layout::StackTransitions;
    use xrack_renderer_femtovg::WindowDamage;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let window_entry = std::env::var_os("XRACK_TABLE_BENCH_WINDOW_ENTRY").is_some();
    let capture = std::env::var_os("XRACK_TABLE_BENCH_PIXELS");
    let target_format = std::env::var("XRACK_TABLE_BENCH_TARGET").ok();
    let format = match target_format.as_deref() {
        None | Some("rgba") => wgpu::TextureFormat::Rgba8Unorm,
        Some("rgba-srgb") => wgpu::TextureFormat::Rgba8UnormSrgb,
        Some("bgra") => wgpu::TextureFormat::Bgra8Unorm,
        Some("bgra-srgb") => wgpu::TextureFormat::Bgra8UnormSrgb,
        Some(value) => panic!("unsupported table benchmark target: {value}"),
    };
    println!("{label}: {frames} frames, {format:?}");
    let scale = 2.0;
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if capture.is_some() {
        usage |= wgpu::TextureUsages::COPY_SRC;
    }
    let descriptor = wgpu::TextureDescriptor {
        label: Some("XRack frame profile"),
        size: wgpu::Extent3d {
            width: (viewport.width * scale) as u32,
            height: (viewport.height * scale) as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    };
    let target = renderer.device.create_texture(&descriptor);
    let alternate = window_entry.then(|| renderer.device.create_texture(&descriptor));
    let mut frame_target = &target;
    let mut base = component.render_shared();
    let mut input = Interaction::default();
    let mut scene = base.clone();
    let mut layout = Rc::new(renderer.layout_with_input(&base, viewport, &input));
    let transitions = StackTransitions::default();
    let mut damage_counts = [0; 3];
    let mut first_frames = [0.0; 20];
    let mut samples: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
    for step in 1..=frames {
        let start = Instant::now();
        base = update(&mut input, &scene, &layout, step);
        let updated = Instant::now();
        let next;
        (scene, next) = renderer.prepare_scene(component, base.clone(), viewport, &input);
        let next = Rc::new(next);
        input.reconcile(&scene, &next);
        let prepared = Instant::now();
        frame_target = alternate
            .as_ref()
            .filter(|_| step % 2 != 0)
            .unwrap_or(&target);
        let damage = if window_entry {
            Some(
                renderer
                    .draw_window_prepared(&scene, &next, frame_target, scale, &input, &transitions)
                    .unwrap(),
            )
        } else {
            renderer
                .draw_with_layout(&scene, &next, frame_target, scale, &input)
                .unwrap();
            None
        };
        let submitted = Instant::now();
        renderer
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .unwrap();
        let complete = Instant::now();
        if step <= first_frames.len() {
            first_frames[step - 1] = (complete - start).as_secs_f64() * 1000.0;
        } else {
            if let Some(damage) = damage {
                let index = match damage {
                    WindowDamage::Full => 0,
                    WindowDamage::Partial { .. } | WindowDamage::PartialRegions(_) => 1,
                    WindowDamage::Unchanged => 2,
                };
                damage_counts[index] += 1;
            }
            for (samples, elapsed) in samples.iter_mut().zip([
                updated - start,
                prepared - updated,
                submitted - prepared,
                complete - submitted,
                complete - start,
            ]) {
                samples.push(elapsed.as_secs_f64() * 1000.0);
            }
        }
        layout = next;
    }
    for values in &mut samples {
        values.sort_by(f64::total_cmp);
    }
    let percentile = |values: &[f64], p: usize| values[(values.len() * p).div_ceil(100) - 1];
    let [update, prepare, submit, gpu_wait, total] =
        samples.each_ref().map(|values| percentile(values, 95));
    let tail = percentile(&samples[4], 99);
    println!("{label} frame p99 at 2x: total {tail:.3} ms");
    println!(
        "{label} frame p95 at 2x: update {update:.3} ms, prepare {prepare:.3} ms, submit {submit:.3} ms, GPU wait {gpu_wait:.3} ms, total {total:.3} ms"
    );
    first_frames.sort_by(f64::total_cmp);
    let first_p95 = first_frames[(first_frames.len() * 95).div_ceil(100) - 1];
    let first_max = first_frames[first_frames.len() - 1];
    println!("{label} first 20 frames at 2x: p95 {first_p95:.3} ms, max {first_max:.3} ms");
    if window_entry {
        let [full, partial, unchanged] = damage_counts;
        println!("{label} damage: {full} full, {partial} partial, {unchanged} unchanged");
    }
    if let Some(directory) = capture {
        let mut pixels = gpu_readback::read_texture(&renderer, frame_target).unwrap();
        if matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        let path = std::path::PathBuf::from(directory).join(format!("{label}.rgba"));
        std::fs::write(path, pixels).unwrap();
    }
}

#[test]
fn a_vetoed_stack_navigation_paints_the_confirmation_over_the_same_page() {
    use xrack_runtime::StackNavigation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StackGuardDemo::new();
    assert!(ui.navigate_stack("navigation", StackNavigation::Push("editor")));
    ui.set_draft("新的界面设计".into());
    let original = ui.render_shared();
    let input = Interaction::default();
    let before = renderer.snapshot(&original, 560, 360, 1.0, &input).unwrap();
    assert!(before.layout.dialog_paths().is_empty());
    let current = ui.stack_snapshot("navigation").unwrap();
    assert!(!ui.navigate_stack("navigation", StackNavigation::Pop));
    let scene = ui.render_shared();
    let blocked = renderer.snapshot(&scene, 560, 360, 1.0, &input).unwrap();
    assert_eq!(blocked.layout.dialog_paths(), &[vec![4]]);
    assert!(std::sync::Arc::ptr_eq(
        &current,
        &ui.stack_snapshot("navigation").unwrap()
    ));
    assert_eq!(
        scene.find("note-field").unwrap().text("text"),
        Some("新的界面设计")
    );
    assert_ne!(before.pixel(30, 280), blocked.pixel(30, 280));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        blocked
            .save_png(std::path::Path::new(&path).join("stack-guard.png"))
            .unwrap();
    }
}

#[test]
fn navigation_stack_renders_only_the_top_and_rejects_a_stale_back_action() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_runtime::StackNavigation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StackDemo::new();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let capture = calls.clone();
    ui.on_visit_route(move |route| {
        capture.borrow_mut().push(route.clone());
        format!("当前页面 · {route}")
    });
    let initial = ui.render_shared();
    let input = Interaction::default();
    let initial_frame = renderer.snapshot(&initial, 590, 324, 1.0, &input).unwrap();
    assert_eq!(&*calls.borrow(), &["overview"]);
    let bad = ["overview", "details", "missing"].map(str::to_owned);
    assert!(!ui.navigate_stack("navigation", StackNavigation::ResetTo(&bad)));
    assert!(Rc::ptr_eq(&initial, &ui.render_shared()));
    let path_to_review = ["overview", "details", "review"].map(str::to_owned);
    assert!(ui.navigate_stack("navigation", StackNavigation::ResetTo(&path_to_review)));
    assert_eq!(ui.get_navigations(), 1);
    let scene = ui.render_shared();
    let path = [0, 2];
    let snapshot = scene.at_path(&path).unwrap().stack.as_ref().unwrap();
    assert_eq!(snapshot.routes(), ["overview", "details", "review"]);
    assert_eq!(&*calls.borrow(), &["overview", "review"]);
    let stale = ControlAction::StackBack {
        revision: snapshot.revision(),
    };
    assert!(ui.dispatch_at(&scene, &[0, 2, 2, 0, 1], "clicked"));
    assert!(!ui.control_at(&scene, &path, stale));
    assert_eq!(ui.get_navigations(), 2);

    let summary = ui.render_shared();
    let stack = ui.stack_snapshot("navigation").unwrap();
    assert_eq!(stack.routes(), ["overview", "details", "summary"]);
    assert_eq!(&*calls.borrow(), &["overview", "review", "summary"]);
    let frame = renderer.snapshot(&summary, 590, 324, 1.0, &input).unwrap();
    assert_ne!(initial_frame.rgba, frame.rgba);
    let bounds = frame.layout.at_path(&path).unwrap().bounds;
    assert_eq!(
        frame.pixel(
            (bounds.x + 12.0) as u32,
            (bounds.y + bounds.height - 12.0) as u32
        ),
        Some([0x2b, 0x48, 0x55, 255])
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("stack-view.png"))
            .unwrap();
    }
    assert!(ui.control_at(
        &summary,
        &path,
        ControlAction::StackBack {
            revision: stack.revision()
        }
    ));
    let details = ui.render_shared();
    assert_eq!(
        details
            .at_path(&path)
            .unwrap()
            .stack
            .as_ref()
            .unwrap()
            .current(),
        Some("details")
    );
    assert_eq!(calls.borrow().last().map(String::as_str), Some("details"));
    assert!(!ui.navigate_stack("navigation", StackNavigation::PopTo("review")));
    assert!(ui.navigate_stack("navigation", StackNavigation::Reset("overview")));
    assert_eq!(
        ui.stack_snapshot("navigation").unwrap().routes(),
        ["overview"]
    );
}

#[test]
fn numeric_drafts_and_choice_overlays_reach_generated_bindings() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SelectionDemo::new();
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let before = renderer.snapshot(&scene, 560, 350, 1.0, &input).unwrap();
    let spin = [2, 1];
    let combo = [3, 1];
    let layout = &before.layout;
    let bounds = layout.at_path(&spin).unwrap().bounds;
    let up = SpinGeometry::new(bounds).up;
    input.pointer_move(
        &scene,
        layout,
        Point {
            x: up.x + 4.0,
            y: up.y + 4.0,
        },
    );
    input.pointer_press();
    input.spin_press(&scene, layout);
    let step = input.spin_release(&scene, layout).unwrap();
    assert!(ui.control_at(&scene, &step.path, step.action));
    assert_eq!(ui.get_quantity(), 15);
    assert_eq!(ui.get_edit_count(), 1);

    let scene = ui.render_shared();
    let node = scene.at_path(&spin).unwrap();
    input.sync_editor(&scene);
    input.edit_spin(node, EditAction::SelectAll);
    input.edit_spin(node, EditAction::Insert("-"));
    input.edit_spin(node, EditAction::Insert("oops"));
    assert_eq!(input.editor_value(node), "-");
    input.edit_spin(node, EditAction::Insert("18"));
    assert_eq!(ui.get_quantity(), 15);
    let commit = input.commit_spin_draft().unwrap();
    assert!(ui.control_at(&scene, &commit.path, commit.action));
    assert_eq!(ui.get_quantity(), -18);
    assert_eq!(ui.get_edit_count(), 2);
    ui.set_quantity(98);
    assert!(ui.control_at(
        &ui.render_shared(),
        &spin,
        ControlAction::Spin(SpinAction::Increase)
    ));
    assert_eq!(ui.get_quantity(), -20);

    let bounds = layout.at_path(&combo).unwrap().bounds;
    let anchor = Point {
        x: bounds.x + 12.0,
        y: bounds.y + 12.0,
    };
    let open = |input: &mut Interaction, scene: &Node| {
        input.pointer_move(scene, layout, anchor);
        input.pointer_press();
        let target = input.pointer_release_target().unwrap();
        assert!(input.open_choice(scene, layout, target));
    };
    let scene = ui.render_shared();
    open(&mut input, &scene);
    let popup = input.choice_menu(&scene, layout).unwrap();
    assert_eq!(popup.selected, Some(1));
    assert!(popup.rows.len() < scene.at_path(&combo).unwrap().strings("model").len());
    assert!(popup.bounds.y + popup.bounds.height > layout.at_path(&[3]).unwrap().bounds.y + 118.0);
    let overlay = renderer.snapshot(&scene, 560, 350, 1.0, &input).unwrap();
    assert_ne!(overlay.pixel(516, 244), before.pixel(516, 244));
    let compact = renderer.layout_with_input(&scene, xrack_layout::Size::new(560.0, 230.0), &input);
    let above = input.choice_menu(&scene, &compact).unwrap().bounds;
    assert!(above.y + above.height <= compact.at_path(&combo).unwrap().bounds.y);
    assert!(input.choice_scroll(&scene, layout, 6));
    let menu = input.choice_menu(&scene, layout).unwrap();
    let row = menu.row_bounds(2);
    input.pointer_move(
        &scene,
        layout,
        Point {
            x: row.x + 9.0,
            y: row.y + 9.0,
        },
    );
    assert!(input.choice_press(&scene, layout));
    let selected = input.choice_release(&scene, layout).unwrap();
    assert!(ui.control_at(&scene, &selected.path, selected.action));
    assert_eq!(ui.get_choice(), 8);
    assert_eq!(ui.get_selected_label(), "反馈");
    assert_eq!(ui.get_underlay_clicks(), 0);

    let scene = ui.render_shared();
    open(&mut input, &scene);
    input.pointer_move(&scene, layout, Point { x: 292.0, y: 240.0 });
    assert!(input.choice_press(&scene, layout));
    assert!(!input.choice_open());
    assert!(input.pointer_release_target().is_none());
    assert_eq!(ui.get_underlay_clicks(), 0);
    open(&mut input, &scene);
    let menu = input.choice_menu(&scene, layout).unwrap();
    let row = menu.row_bounds(0);
    input.pointer_move(
        &scene,
        layout,
        Point {
            x: row.x + 9.0,
            y: row.y + 9.0,
        },
    );
    assert!(input.choice_press(&scene, layout));
    let mut model = ui.get_workspaces();
    model[menu.first] = "更新后的选项".into();
    ui.set_workspaces(model);
    let scene = ui.render_shared();
    assert!(input.choice_release(&scene, layout).is_none());
    assert_eq!(ui.get_choice(), 8);
    assert!(input.choice_key(&scene, layout, ChoiceKey::End).is_none());
    let selected = input.choice_key(&scene, layout, ChoiceKey::Accept).unwrap();
    assert!(ui.control_at(&scene, &selected.path, selected.action));
    assert_eq!(ui.get_selected_label(), "帮助");

    open(&mut input, &ui.render_shared());
    ui.set_locked(true);
    let locked = ui.render_shared();
    let locked_layout =
        renderer.layout_with_input(&locked, xrack_layout::Size::new(560.0, 350.0), &input);
    input.reconcile(&locked, &locked_layout);
    assert!(!input.choice_open());
    assert!(!ui.control_at(&locked, &spin, ControlAction::Spin(SpinAction::Increase)));
    assert!(!ui.control_at(&locked, &combo, ControlAction::Select(0)));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        overlay
            .save_png(std::path::Path::new(&path).join("selection-demo.png"))
            .unwrap();
    }
}

#[test]
fn wheel_adjustment_moves_the_numeric_thumb_in_the_rendered_frame() {
    use controls_ui::WheelControlsDemo;
    use xrack_layout::WheelAdjustment;
    use xrack_runtime::{Length, WheelEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = WheelControlsDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(460.0, 310.0);
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let bounds = layout.at_path(&[1, 0, 1]).unwrap().bounds;
    let geometry = SliderGeometry::new(bounds, Orientation::Horizontal);
    let old = geometry.position(0.8);
    let next = geometry.position(1.0);
    input.pointer_move(&scene, &layout, old);
    let before = renderer.snapshot(&scene, 460, 310, 1.0, &input).unwrap();
    let Some(WheelAdjustment::Range(change)) = input.wheel_adjustment(
        &scene,
        &layout,
        WheelEvent {
            delta_y: Length(-28.0),
            is_line: true,
            lines_y: -1.0,
            ..Default::default()
        },
        std::time::Instant::now(),
    ) else {
        panic!("wheel must reach the slider");
    };
    assert!(ui.adjust_range_at(&scene, &change.path, change.action));
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let after = renderer.snapshot(&scene, 460, 310, 1.0, &input).unwrap();
    let old = (old.x as u32, old.y as u32);
    let next = (next.x as u32, next.y as u32);
    assert_eq!(before.pixel(old.0, old.1), after.pixel(next.0, next.1));
    assert_ne!(before.pixel(next.0, next.1), after.pixel(next.0, next.1));
    assert_ne!(before.pixel(old.0, old.1), after.pixel(old.0, old.1));
}

#[test]
fn range_controls_share_pointer_keyboard_and_rendering() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ControlsDemo::new();
    let mut input = Interaction::default();
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 540, 376, 1.0, &input).unwrap();
    let horizontal = [2, 1];
    let vertical = [3, 1];
    assert_eq!(before.pixel(70, 248), Some([0x53, 0x6d, 0xfe, 255]));
    assert_eq!(before.pixel(220, 248), Some([0x35, 0x41, 0x5b, 255]));
    assert!(!renderer.needs_redraw());

    let apply = |ui: &mut ControlsDemo, event: RangeInput| {
        ui.adjust_range_at(&ui.render(), &event.path, event.action)
    };
    let bounds = before.layout.at_path(&horizontal).unwrap().bounds;
    let thumb = SliderGeometry::new(bounds, Orientation::Horizontal).position(0.25);
    input.pointer_move(
        &scene,
        &before.layout,
        Point {
            x: thumb.x + 4.0,
            y: thumb.y,
        },
    );
    input.pointer_press();
    assert!(input.slider_press(&scene, &before.layout).is_none());
    assert!(!apply(
        &mut ui,
        input.slider_drag(&scene, &before.layout).unwrap()
    ));
    input.pointer_move(
        &scene,
        &before.layout,
        Point {
            x: bounds.x + bounds.width + 10.0,
            y: thumb.y,
        },
    );
    assert!(input.hovered.is_none());
    assert!(apply(
        &mut ui,
        input.slider_drag(&scene, &before.layout).unwrap()
    ));
    assert_eq!(ui.get_volume(), 100.0);
    assert_eq!(ui.get_changes(), 1);
    assert_eq!(ui.get_releases(), 0);
    let released = input.slider_release(&ui.render(), &before.layout).unwrap();
    assert!(apply(&mut ui, released));
    assert_eq!(ui.get_releases(), 1);
    assert_eq!(ui.get_last_release(), 100.0);

    for _ in 0..2 {
        let key = input
            .slider_key(&ui.render(), &before.layout, SliderKey::Left, true)
            .unwrap();
        assert!(apply(&mut ui, key));
    }
    assert_eq!(ui.get_volume(), 90.0);
    assert_eq!(ui.get_changes(), 3);
    let released = input
        .slider_key(&ui.render(), &before.layout, SliderKey::Left, false)
        .unwrap();
    assert!(apply(&mut ui, released));
    assert!(
        input
            .slider_key(&ui.render(), &before.layout, SliderKey::Left, false)
            .is_none()
    );
    assert_eq!(ui.get_releases(), 2);
    assert_eq!(ui.get_last_release(), 90.0);

    let bounds = before.layout.at_path(&vertical).unwrap().bounds;
    let top = SliderGeometry::new(bounds, Orientation::Vertical).position(1.0);
    input.pointer_move(&ui.render(), &before.layout, top);
    input.pointer_press();
    let press = input.slider_press(&ui.render(), &before.layout).unwrap();
    assert!(apply(&mut ui, press));
    assert_eq!(ui.get_level(), 100.0);
    assert!(
        input
            .slider_key(&ui.render(), &before.layout, SliderKey::Left, true)
            .is_none()
    );
    input.slider_release(&ui.render(), &before.layout).unwrap();
    assert!(ui.dispatch_at(&ui.render(), &[2, 2], "clicked"));
    assert!(ui.get_muted());
    assert!(!ui.adjust_range_at(&ui.render(), &horizontal, RangeAction::Minimum));

    assert!(ui.dispatch_at(&ui.render(), &[2, 5], "clicked"));
    renderer
        .snapshot(&ui.render(), 540, 376, 1.0, &input)
        .unwrap();
    assert!(renderer.needs_redraw());
    ui.set_busy(false);
    renderer
        .snapshot(&ui.render(), 540, 376, 1.0, &input)
        .unwrap();
    assert!(!renderer.needs_redraw());
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("controls-demo.png"))
            .unwrap();
    }
}

#[test]
fn preview_drag_commits_deferred_numeric_values_only_on_release() {
    use controls_ui::SnapSlidersDemo;
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Component, Orientation};

    let ui = SnapSlidersDemo::new();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let (_, layout) = renderer.prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(500.0, 290.0),
        &Interaction::default(),
    );
    let geometry = SliderGeometry::new(
        layout.at_path(&[0, 3]).unwrap().bounds,
        Orientation::Horizontal,
    );
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let original = preview.render_animated(&mut renderer).unwrap().to_vec();
    assert!(preview.begin_pointer(geometry.position(0.2)));
    assert!(preview.move_pointer(geometry.position(0.46)));
    let draft = preview.render_animated(&mut renderer).unwrap();
    assert_ne!(draft, original);
    assert_eq!(ui.get_level(), 20.0);
    assert_eq!(ui.get_level_changes(), 0);
    assert!(preview.release_pointer(geometry.position(0.46), false));
    assert_eq!(ui.get_level(), 50.0);
    assert_eq!(ui.get_level_releases(), 1);
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.begin_pointer(geometry.position(0.5)));
    assert!(preview.move_pointer(geometry.position(0.85)));
    assert!(preview.cancel_pointer());
    assert!(!preview.release_pointer(geometry.position(0.85), false));
    assert_eq!(ui.get_level(), 50.0);
    assert_eq!(ui.get_level_releases(), 1);
}

#[test]
fn deferred_slider_paints_the_snapped_preview_before_the_model_changes() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SnapSlidersDemo::new();
    let scene = ui.render_shared();
    let mut input = Interaction::default();
    let viewport = Size::new(500.0, 290.0);
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    let path = [0, 3];
    let geometry = SliderGeometry::new(
        layout.at_path(&path).unwrap().bounds,
        Orientation::Horizontal,
    );
    let before = renderer.snapshot(&scene, 500, 290, 1.0, &input).unwrap();
    input.pointer_move(&scene, &layout, geometry.position(0.46));
    input.pointer_press_in_scene(&scene, &layout);
    assert!(input.slider_press(&scene, &layout).is_none());
    let preview = renderer.snapshot(&scene, 500, 290, 1.0, &input).unwrap();
    let snapped = geometry.position(0.5);
    assert_ne!(
        before.pixel(snapped.x as u32, snapped.y as u32),
        preview.pixel(snapped.x as u32, snapped.y as u32),
    );
    assert_eq!(ui.get_level(), 20.0);
    assert!(std::rc::Rc::ptr_eq(&scene, &ui.render_shared()));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        preview
            .save_png(std::path::Path::new(&path).join("slider-snap-preview.png"))
            .unwrap();
    }
    let release = input.slider_release(&scene, &layout).unwrap();
    assert!(ui.adjust_range_at(&scene, &release.path, release.action));
    assert_eq!(ui.get_level(), 50.0);
}

#[test]
fn range_slider_keeps_order_and_lets_overlapping_handles_separate() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ControlsDemo::new();
    ui.on_interval_caption(|lower, upper| format!("区间 {lower:.1} – {upper:.1}"));
    let mut input = Interaction::default();
    let path = [5, 1];
    let lower = [5, 1, 0];
    let upper = [5, 1, 1];
    let vertical = [6, 1];
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 540, 510, 1.0, &input).unwrap();
    let geometry = SliderGeometry::new(
        before.layout.at_path(&path).unwrap().bounds,
        Orientation::Horizontal,
    );
    let sample = |frame: &xrack_renderer_femtovg::Snapshot, fraction| {
        let p = geometry.position(fraction);
        frame.pixel(p.x as u32, p.y as u32)
    };
    assert_eq!(sample(&before, 0.5), Some([0x53, 0x6d, 0xfe, 255]));
    assert_eq!(sample(&before, 0.1), Some([0x35, 0x41, 0x5b, 255]));

    for _ in 0..5 {
        input.focus_next(&scene, &before.layout, false);
    }
    assert_eq!(input.focused.as_ref().unwrap().path, lower);
    input.focus_next(&scene, &before.layout, false);
    assert_eq!(input.focused.as_ref().unwrap().path, upper);
    input.focus_next(&scene, &before.layout, true);
    let key = input
        .slider_key(&scene, &before.layout, SliderKey::End, true)
        .unwrap();
    assert_eq!(key.path, lower);
    assert!(ui.adjust_range_at(&scene, &key.path, key.action));
    assert_eq!([ui.get_lower(), ui.get_upper()], [75.0, 75.0]);
    let scene = ui.render();
    let layout = renderer.layout_with_input(&scene, Size::new(540.0, 510.0), &input);
    let release = input
        .slider_key(&scene, &layout, SliderKey::End, false)
        .unwrap();
    assert!(ui.adjust_range_at(&scene, &release.path, release.action));
    assert_eq!(
        [ui.get_released_lower(), ui.get_released_upper()],
        [75.0, 75.0]
    );

    input.pointer_move(&scene, &layout, geometry.position(0.75));
    input.pointer_press();
    assert!(input.slider_press(&scene, &layout).is_none());
    input.pointer_move(&scene, &layout, geometry.position(0.25));
    let drag = input.slider_drag(&scene, &layout).unwrap();
    assert_eq!(drag.path, lower);
    assert!(ui.adjust_range_at(&scene, &drag.path, drag.action));
    assert!((ui.get_lower() - 25.0).abs() < 1e-5);
    assert_eq!(ui.get_upper(), 75.0);
    let release = input.slider_release(&ui.render(), &layout).unwrap();
    assert!(ui.adjust_range_at(&ui.render(), &release.path, release.action));
    assert_eq!(ui.get_interval_changes(), 2);
    assert_eq!(ui.get_interval_releases(), 2);

    let scene = ui.render();
    let layout = renderer.layout_with_input(&scene, Size::new(540.0, 510.0), &input);
    input.pointer_move(&scene, &layout, geometry.position(0.9));
    input.pointer_press();
    let click = input.slider_press(&scene, &layout).unwrap();
    assert_eq!(click.path, upper);
    assert!(ui.adjust_range_at(&scene, &click.path, click.action));
    assert!((ui.get_upper() - 90.0).abs() < 1e-5);
    assert!((ui.get_lower() - 25.0).abs() < 1e-5);
    let release = input.slider_release(&ui.render(), &layout).unwrap();
    assert!(ui.adjust_range_at(&ui.render(), &release.path, release.action));

    let scene = ui.render();
    let layout = renderer.layout_with_input(&scene, Size::new(540.0, 510.0), &input);
    let vertical_geometry = SliderGeometry::new(
        layout.at_path(&vertical).unwrap().bounds,
        Orientation::Vertical,
    );
    input.pointer_move(&scene, &layout, vertical_geometry.position(0.7));
    input.pointer_press();
    assert!(input.slider_press(&scene, &layout).is_none());
    assert!(input.slider_release(&scene, &layout).is_some());
    let key = input
        .slider_key(&scene, &layout, SliderKey::Up, true)
        .unwrap();
    assert_eq!(key.path, [6, 1, 1]);
    assert!(ui.adjust_range_at(&scene, &key.path, key.action));
    let scene = ui.render();
    let node = scene.at_path(&vertical).unwrap();
    assert_eq!(
        [node.number("lower-value"), node.number("upper-value")],
        [Some(30.0), Some(80.0)]
    );
    let frame = renderer.snapshot(&scene, 540, 510, 1.0, &input).unwrap();
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("range-slider.png"))
            .unwrap();
    }

    ui.set_lower(90.0);
    ui.set_upper(20.0);
    assert!(ui.adjust_range_at(&ui.render(), &upper, RangeAction::Increase));
    assert_eq!([ui.get_lower(), ui.get_upper()], [20.0, 25.0]);
    ui.set_muted(true);
    assert!(!ui.adjust_range_at(&ui.render(), &upper, RangeAction::Maximum));
}

#[test]
fn dials_handle_limits_wrap_and_relative_dragging() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ControlsDemo::new();
    let mut input = Interaction::default();
    let rotary = [7, 3];
    let phase = [7, 4];
    let linear = [7, 5];
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 540, 652, 1.0, &input).unwrap();
    let layout = &before.layout;
    let geometry = |path: &[usize]| {
        DialGeometry::new(
            layout.at_path(path).unwrap().bounds,
            scene.at_path(path).unwrap(),
        )
    };
    let apply = |ui: &mut ControlsDemo, event: RangeInput| {
        ui.adjust_range_at(&ui.render(), &event.path, event.action)
    };
    let near = |actual: f64, expected: f64| {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}")
    };
    let rotary_geometry = geometry(&rotary);
    let filled = rotary_geometry.position(0.25);
    let empty = rotary_geometry.position(0.8);
    assert_eq!(
        before.pixel(filled.x as u32, filled.y as u32),
        Some([0x53, 0x6d, 0xfe, 255])
    );
    assert_eq!(
        before.pixel(empty.x as u32, empty.y as u32),
        Some([0x58, 0x65, 0x82, 255])
    );

    input.pointer_move(&scene, layout, rotary_geometry.position(0.75));
    input.pointer_press();
    let press = input.slider_press(&scene, layout).unwrap();
    assert_eq!(press.path, rotary);
    assert!(apply(&mut ui, press));
    near(ui.get_rotation(), 25.0);
    let gap = Point {
        x: rotary_geometry.center.x,
        y: rotary_geometry.center.y + rotary_geometry.track_radius,
    };
    for point in [rotary_geometry.position(0.95), gap] {
        let scene = ui.render();
        input.pointer_move(&scene, layout, point);
        assert!(apply(&mut ui, input.slider_drag(&scene, layout).unwrap()));
    }
    assert_eq!(ui.get_rotation(), 50.0);
    let scene = ui.render();
    input.pointer_move(&scene, layout, rotary_geometry.position(0.05));
    assert!(input.slider_drag(&scene, layout).is_none());
    input.pointer_move(&scene, layout, gap);
    assert!(apply(&mut ui, input.slider_drag(&scene, layout).unwrap()));
    assert!(ui.get_rotation() > 0.0 && ui.get_rotation() < 50.0);
    let scene = ui.render();
    input.pointer_move(&scene, layout, rotary_geometry.center);
    assert!(input.slider_drag(&scene, layout).is_none());
    input.pointer_move(&scene, layout, rotary_geometry.position(0.3));
    assert!(input.slider_drag(&scene, layout).is_none());
    assert!(apply(
        &mut ui,
        input.slider_release(&scene, layout).unwrap()
    ));
    assert_eq!(ui.get_dial_releases(), 1);
    near(ui.get_dial_last_release(), ui.get_rotation());
    assert_eq!(ui.get_dial_changes(), 4);
    for (key, expected) in [
        (SliderKey::Home, -50.0),
        (SliderKey::Up, -45.0),
        (SliderKey::End, 50.0),
    ] {
        let scene = ui.render();
        assert!(apply(
            &mut ui,
            input.slider_key(&scene, layout, key, true).unwrap()
        ));
        near(ui.get_rotation(), expected);
        let release = input.slider_key(&ui.render(), layout, key, false).unwrap();
        assert!(apply(&mut ui, release));
    }

    let phase_geometry = geometry(&phase);
    let scene = ui.render();
    input.pointer_move(&scene, layout, phase_geometry.position(0.95));
    input.pointer_press();
    assert!(input.slider_press(&scene, layout).is_none());
    input.pointer_move(&scene, layout, phase_geometry.position(0.05));
    assert!(apply(&mut ui, input.slider_drag(&scene, layout).unwrap()));
    near(
        ui.render()
            .at_path(&phase)
            .unwrap()
            .number("value")
            .unwrap(),
        5.0,
    );
    assert!(input.slider_release(&ui.render(), layout).is_some());
    for (key, expected) in [
        (SliderKey::End, 100.0),
        (SliderKey::Right, 0.0),
        (SliderKey::Left, 100.0),
    ] {
        let scene = ui.render();
        let press = input.slider_key(&scene, layout, key, true).unwrap();
        assert!(apply(&mut ui, press));
        assert_eq!(
            ui.render().at_path(&phase).unwrap().number("value"),
            Some(expected)
        );
    }

    let linear_geometry = geometry(&linear);
    let center = linear_geometry.center;
    let quarter = linear_geometry.face_radius * 0.5;
    for (mode, inactive, active, expected) in [
        (
            DialInputMode::Horizontal,
            Point {
                x: center.x,
                y: center.y - quarter,
            },
            Point {
                x: center.x + quarter,
                y: center.y - quarter,
            },
            60.0,
        ),
        (
            DialInputMode::Vertical,
            Point {
                x: center.x + quarter,
                y: center.y,
            },
            Point {
                x: center.x + quarter,
                y: center.y - quarter,
            },
            85.0,
        ),
    ] {
        ui.set_linear_mode(mode);
        let scene = ui.render();
        input.pointer_move(&scene, layout, center);
        input.pointer_press();
        assert!(input.slider_press(&scene, layout).is_none());
        input.pointer_move(&scene, layout, inactive);
        assert!(input.slider_drag(&scene, layout).is_none());
        input.pointer_move(&scene, layout, active);
        assert!(apply(&mut ui, input.slider_drag(&scene, layout).unwrap()));
        near(
            ui.render()
                .at_path(&linear)
                .unwrap()
                .number("value")
                .unwrap(),
            expected,
        );
        assert!(input.slider_release(&ui.render(), layout).is_some());
    }
    ui.set_muted(true);
    assert!(!ui.adjust_range_at(&ui.render(), &rotary, RangeAction::Decrease));
    let frame = renderer
        .snapshot(&ui.render(), 540, 652, 1.0, &input)
        .unwrap();
    assert_ne!(before.rgba, frame.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("dial.png"))
            .unwrap();
    }
}

#[test]
fn form_links_text_and_reflows_imported_grid_and_model() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut form = FormDemo::new();
    let scene = form.render();
    assert!(scene.find("preview").is_none());
    assert_eq!(
        scene.find("label").unwrap().children[0].text("text"),
        Some("搜索关键词")
    );
    let field = [0, 2, 0, 1];
    let meter = [0, 4, 0];
    let scroll = [0, 6];
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 560, 428, 1.0, &input).unwrap();
    let bounds = before.layout.at_path(&field).unwrap().bounds;
    assert!(bounds.x > before.layout.at_path(&[0, 2, 0, 0]).unwrap().bounds.x);
    assert_eq!(before.layout.at_path(&meter).unwrap().bounds.width, 256.0);
    input.pointer_move(
        &scene,
        &before.layout,
        Point {
            x: bounds.x + 10.0,
            y: bounds.y + 15.0,
        },
    );
    input.pointer_press();
    input.sync_editor(&scene);
    assert!(input.is_focused(&field));
    input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("ni".into(), Some((2, 2)));
    let preedit = renderer.snapshot(&scene, 560, 428, 1.0, &input).unwrap();
    assert_ne!(before.rgba, preedit.rgba);
    assert!(form.get_query().is_empty());

    let value = input
        .editor
        .as_mut()
        .unwrap()
        .apply("", EditAction::Insert("你好"))
        .unwrap();
    assert!(form.edit_at(&scene, &field, value));
    assert_eq!(form.get_query(), "你好");
    assert_eq!(form.get_edits(), 1);
    let updated = form.render();
    assert_eq!(
        updated.find("preview").unwrap().text("text"),
        Some("预览：你好")
    );
    let transition = SceneTransition::between(&scene, &updated);
    let frame = transition.frame(&updated, std::time::Duration::from_millis(110));
    let mid = renderer.snapshot(&frame, 560, 428, 1.0, &input).unwrap();
    assert_eq!(mid.layout.at_path(&meter).unwrap().bounds.width, 320.0);
    assert!(form.dispatch_at(&updated, &field, "accepted"));
    assert_eq!(form.get_submissions(), 1);
    assert_eq!(
        form.get_last_result(),
        2,
        "child and root calls run before the result is read"
    );
    assert_eq!(form.invoke_result_length(4), 5);
    form.on_result_length(|value| value * 2);
    assert_eq!(form.invoke_result_length(4), 8);
    assert!(form.dispatch_at(&form.render(), &field, "accepted"));
    assert_eq!(
        form.get_last_result(),
        4,
        "DSL invocation reaches the Rust handler"
    );
    let scene = form.render();
    let after = renderer.snapshot(&scene, 560, 428, 1.0, &input).unwrap();
    let viewport = after.layout.at_path(&scroll).unwrap();
    assert!(viewport.scroll.unwrap().max_offset > 0.0);
    input.pointer_move(
        &scene,
        &after.layout,
        Point {
            x: viewport.bounds.x + 10.0,
            y: viewport.bounds.y + 10.0,
        },
    );
    assert!(input.scroll_y(&after.layout, 35.0));
    let scrolled = renderer.snapshot(&scene, 560, 428, 1.0, &input).unwrap();
    assert_eq!(
        scrolled
            .layout
            .at_path(&scroll)
            .unwrap()
            .scroll
            .unwrap()
            .offset,
        35.0
    );
    assert_ne!(scrolled.rgba, after.rgba);
    form.set_suggestions(vec!["更新后的模型".into()]);
    let updated = form.render();
    let model = updated.at_path(&[0, 6, 0, 0]).unwrap();
    assert_eq!(model.children.len(), 1);
    assert_eq!(model.children[0].text("text"), Some("1  更新后的模型"));
    assert!(form.dispatch("show-list", "clicked"));
    assert!(!form.get_show_suggestions());
    assert!(form.render().find("suggestions-view").is_none());
    form.set_query(String::new());
    assert!(form.render().find("preview").is_none());
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("form-demo.png"))
            .unwrap();
    }
}

#[test]
fn input_backspace_preserves_unicode_grapheme_boundaries() {
    let text = "搜👩‍💻";
    let mut editor = TextEdit::at_end(text);
    assert_eq!(
        editor.apply(text, EditAction::Backspace).as_deref(),
        Some("搜")
    );
}

#[test]
fn layout_example_reflows_instances_and_paints_aligned_cards() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .expect("GPU test requires an adapter and installed fonts");
    let mut demo = LayoutDemo::new();
    let input = Interaction::default();
    let scene = demo.render();
    let wide = renderer.snapshot(&scene, 620, 338, 1.0, &input).unwrap();
    let cards = &wide.layout.at_path(&[0, 2]).unwrap().children;
    assert_eq!(cards[0].bounds.width, 128.0);
    assert_eq!(cards[1].bounds.width, 150.0);
    assert_eq!(cards[2].bounds.width, 250.0);
    assert!(cards[1].bounds.y > cards[0].bounds.y);
    let y = (cards[0].bounds.y + 25.0) as u32;
    assert_eq!(wide.pixel(42, y), Some([0x31, 0x4b, 0x80, 255]));

    let narrow = renderer.snapshot(&scene, 500, 338, 1.0, &input).unwrap();
    let narrower = &narrow.layout.at_path(&[0, 2]).unwrap().children;
    assert_eq!(narrower[0].bounds.width, 128.0);
    assert!(narrower[1].bounds.width < 150.0);
    assert!(narrower[2].bounds.width < 250.0);

    assert!(demo.dispatch("toggle", "clicked"));
    let centered = renderer
        .snapshot(&demo.render(), 620, 338, 1.0, &input)
        .unwrap();
    let centered_cards = &centered.layout.at_path(&[0, 2]).unwrap().children;
    assert_eq!(centered_cards[0].bounds.x, 100.0);
    assert_eq!(centered_cards[1].bounds.width, 124.0);
    assert_eq!(centered.pixel(42, y), Some([0x10, 0x17, 0x25, 255]));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        wide.save_png(std::path::Path::new(&path).join("layout-demo.png"))
            .unwrap();
    }
}

#[test]
fn scroll_view_bars_reveal_a_button_at_the_far_corner() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = LayoutDemo::new();
    let viewport = Size::new(620.0, 590.0);
    let mut input = Interaction::default();
    let path = [0, 5];
    let action = [0, 5, 0, 3];
    let scene = ui.render();
    let before = renderer.snapshot(&scene, 620, 590, 1.0, &input).unwrap();
    let item = before.layout.at_path(&path).unwrap();
    assert_eq!(item.children[0].bounds.width, 880.0);
    assert_eq!(item.children[0].bounds.height, 340.0);
    assert_eq!(
        before
            .layout
            .at_path(&action)
            .unwrap()
            .bounds
            .intersect(item.bounds)
            .width,
        0.0
    );
    for orientation in [Orientation::Horizontal, Orientation::Vertical] {
        let layout = renderer.layout_with_input(&scene, viewport, &input);
        let bar = layout
            .at_path(&path)
            .unwrap()
            .scroll_bars(scene.at_path(&path).unwrap())
            .find(|bar| bar.orientation == orientation)
            .unwrap();
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bar.thumb.x + bar.thumb.width * 0.5,
                y: bar.thumb.y + bar.thumb.height * 0.5,
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
                x: bar.track.x + bar.track.width + 30.0,
                y: bar.track.y + bar.track.height + 30.0,
            },
        );
        assert_eq!(
            input.scroll_bar_drag(&scene, &layout),
            Some(xrack_layout::ScrollBarUpdate::ViewportMoved)
        );
        assert!(input.finish_scroll_bar());
    }
    let layout = renderer.layout_with_input(&scene, viewport, &input);
    let item = layout.at_path(&path).unwrap();
    for axis in [Orientation::Horizontal, Orientation::Vertical] {
        let scroll = item.scroll_on(axis).unwrap();
        assert_eq!(scroll.offset, scroll.max_offset);
    }
    let button = layout.at_path(&action).unwrap().bounds;
    let visible = button.intersect(item.bounds);
    assert!((visible.width - button.width).abs() < 0.001);
    assert!((visible.height - button.height).abs() < 0.001);
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: button.x + button.width * 0.5,
            y: button.y + button.height * 0.5,
        },
    );
    input.pointer_press();
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.path, action);
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(ui.get_canvas_activations(), 1);
    let after = renderer
        .snapshot(&ui.render(), 620, 590, 1.0, &input)
        .unwrap();
    assert_ne!(before.rgba, after.rgba);
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        after
            .save_png(std::path::Path::new(&path).join("scroll-view.png"))
            .unwrap();
    }
    let scene = ui.render();
    let viewport = after.layout.at_path(&path).unwrap().bounds;
    let start = Point {
        x: viewport.x + 70.0,
        y: viewport.y + 55.0,
    };
    let end = Point {
        x: start.x + 55.0,
        y: start.y + 35.0,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        &after.layout,
        start,
        now,
        ScrollGestureKind::MousePan
    ));
    assert!(input.move_scroll_gesture(&scene, &after.layout, end, now + Duration::from_millis(35)));
    let dragged = renderer.layout_with_input(&scene, Size::new(620.0, 590.0), &input);
    assert!(
        !input
            .end_scroll_gesture(&scene, &dragged, end, now + Duration::from_millis(40))
            .tap
    );
    assert!(input.advance_scroll_motion(&scene, &dragged, now + Duration::from_millis(56)));
    let coasting = renderer.snapshot(&scene, 620, 590, 1.0, &input).unwrap();
    let current = coasting.layout.at_path(&path).unwrap();
    let old = dragged.at_path(&path).unwrap();
    for axis in [Orientation::Horizontal, Orientation::Vertical] {
        assert!(current.scroll_on(axis).unwrap().offset < old.scroll_on(axis).unwrap().offset);
    }
    assert_ne!(after.rgba, coasting.rgba);
}

#[test]
fn real_gpu_rendering_unicode_clicks_dpi_and_clipping() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .expect("GPU test requires an adapter and installed fonts");
    eprintln!("GPU adapter: {:?}", renderer.adapter.get_info());
    let shaped = renderer.text.shape("XRack 中文计数", 20.0).unwrap();
    assert!(!shaped.glyphs.is_empty());
    assert!(
        shaped.glyphs.iter().all(|g| g.glyph_id != 0),
        "font fallback must provide real glyphs, not tofu boxes"
    );
    let mut ui = Counter::new();
    let scene = ui.render();
    let mut input = Interaction::default();
    let before = renderer.snapshot(&scene, 320, 180, 1.0, &input).unwrap();
    assert_eq!(before.pixel(1, 1), Some([0x18, 0x20, 0x2b, 255]));
    let button = &before.layout.children[0].children[2];
    assert!(button.bounds.height >= 40.0);
    let p = Point {
        x: button.bounds.x + 12.0,
        y: button.bounds.y + button.bounds.height * 0.5,
    };
    assert_eq!(
        before.pixel(p.x as u32, p.y as u32),
        Some([0x53, 0x6d, 0xfe, 255])
    );
    input.pointer_move(&scene, &before.layout, p);
    input.pointer_press();
    assert!(ui.dispatch(input.pointer_release().unwrap(), "clicked"));
    assert_eq!(ui.get_count(), 1);
    assert_eq!(ui.get_doubled(), 2);
    let after = renderer
        .snapshot(&ui.render(), 320, 180, 1.0, &input)
        .unwrap();
    assert_ne!(
        before.rgba, after.rgba,
        "the generated binding must change actual pixels"
    );
    let hidpi = renderer
        .snapshot(&ui.render(), 640, 360, 2.0, &input)
        .unwrap();
    assert_eq!(hidpi.layout.bounds, after.layout.bounds);
    assert_eq!(
        hidpi.layout.children[0].children[2].bounds,
        after.layout.children[0].children[2].bounds
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("counter-before.png"))
            .unwrap();
        after
            .save_png(std::path::Path::new(&path).join("counter-after.png"))
            .unwrap();
        hidpi
            .save_png(std::path::Path::new(&path).join("counter-hidpi.png"))
            .unwrap();
    }
    let mut root = Node::new("Window", Some("root"));
    let mut panel = Node::new("Rectangle", Some("clip"));
    panel
        .properties
        .insert("width", Value::Length(Length(30.0)));
    panel
        .properties
        .insert("height", Value::Length(Length(20.0)));
    let mut child = Node::new("Rectangle", Some("overflow"));
    for (key, n) in [("x", 20.0), ("width", 30.0), ("height", 20.0)] {
        child.properties.insert(key, Value::Length(Length(n)));
    }
    child
        .properties
        .insert("background", Value::Color(Color(0x00ff00ff)));
    panel.children.push(child);
    root.children.push(panel);
    let clipped = renderer
        .snapshot(&root, 128, 64, 1.0, &Interaction::default())
        .unwrap();
    assert_eq!(clipped.pixel(25, 10), Some([0, 255, 0, 255]));
    assert_eq!(clipped.pixel(35, 10), Some([0x18, 0x20, 0x2b, 255]));
    assert!(renderer.snapshot(&root, 0, 1, 1.0, &input).is_err());
    assert!(renderer.snapshot(&root, 10, 10, f32::NAN, &input).is_err());
}

#[test]
fn gallery_renders_typed_gradients_and_individual_corners() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .expect("GPU test requires an adapter and installed fonts");
    let mut gallery = Gallery::new();
    let input = Interaction::default();
    let before = renderer
        .snapshot(&gallery.render(), 600, 358, 1.0, &input)
        .unwrap();
    let backdrop = [0x10, 0x15, 0x21, 255];

    assert_eq!(before.pixel(35, 103), Some(backdrop), "rounded top-left");
    assert_ne!(
        before.pixel(288, 253),
        Some(backdrop),
        "square bottom-right"
    );

    let start = before.pixel(50, 180).unwrap();
    let middle = before.pixel(162, 180).unwrap();
    let end = before.pixel(280, 180).unwrap();
    assert!(start[2] > start[0] && middle[1] > middle[0] && end[0] > end[2]);
    let center = before.pixel(438, 178).unwrap();
    let edge = before.pixel(318, 178).unwrap();
    assert!(center[0] > edge[0] && center[1] > edge[1]);

    let border = before.pixel(50, 297).unwrap();
    assert!(border[2] > 200, "gradient border: {border:?}");
    assert_eq!(before.pixel(50, 311), Some([0x16, 0x1e, 0x31, 255]));

    assert!(gallery.dispatch("toggle", "clicked"));
    let after = renderer
        .snapshot(&gallery.render(), 600, 358, 1.0, &input)
        .unwrap();
    assert!(after.pixel(280, 180).unwrap()[1] > end[1]);
    assert_eq!(after.pixel(438, 178), Some(center));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        before
            .save_png(std::path::Path::new(&path).join("painting-gallery.png"))
            .unwrap();
    }
}

#[test]
fn a_fixed_image_paints_its_new_source_with_the_previous_layout() {
    use media_ui::NineSliceDemo;
    use xrack_runtime::{Length, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut scene = Node::new("Window", None);
    let mut image = Node::new("Image", None);
    image.properties.insert("x", Length(8.0).into());
    image.properties.insert("y", Length(8.0).into());
    image.properties.insert("width", Length(48.0).into());
    image.properties.insert("height", Length(32.0).into());
    image.mark_paint_only("source");
    scene.children.push(image);
    let input = Interaction::default();
    let before = renderer.snapshot(&scene, 64, 48, 1.0, &input).unwrap();
    let mut updated = scene.clone();
    updated.children[0]
        .properties
        .insert("source", NineSliceDemo::new().get_frame().into());
    assert!(scene.same_frame_inputs(&updated));
    let after = renderer
        .snapshot_prepared(&updated, before.layout.clone(), 64, 48, 1.0, &input)
        .unwrap();
    assert_eq!(before.layout, after.layout);
    assert_eq!(before.pixel(0, 0), after.pixel(0, 0));
    assert!((8..40).any(|y| (8..56).any(|x| before.pixel(x, y) != after.pixel(x, y))));
}

#[test]
fn media_renders_embedded_images_and_vector_paths() {
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .expect("GPU test requires an adapter and installed fonts");
    let mut demo = MediaDemo::new();
    let mut interpreted = interpret_example("examples/media/media.xr")
        .create("MediaDemo")
        .unwrap();
    let scene = demo.render();
    let interpreted_scene = interpreted.try_render().unwrap();
    let input = Interaction::default();
    // Compare the geometry producers on the direct path, independently of
    // whether their first frame would populate an 8-bit coverage texture.
    renderer.set_vector_mask_cache_enabled(false);
    let snapshot = renderer.snapshot(&scene, 660, 376, 1.0, &input).unwrap();
    let interpreted_snapshot = renderer
        .snapshot(&interpreted_scene, 660, 376, 1.0, &input)
        .unwrap();
    assert_same_pixels(&interpreted_snapshot, &snapshot);
    assert_eq!(interpreted_snapshot.layout, snapshot.layout);
    let card = [0x1d, 0x29, 0x3d, 255];

    let mut badge = scene.find("badge").unwrap().clone();
    let commands = badge.text("commands").unwrap().to_owned();
    let plan = badge.precompiled_path(&commands).unwrap();
    assert!(!plan.segments.is_empty());
    // Exercise the precomputed tight bounds as well as the normalized segments.
    badge.properties.remove("viewbox-width");
    badge.properties.remove("viewbox-height");
    let mut fallback = xrack_runtime::Node::new("Path", badge.id);
    fallback.properties = badge.properties.clone();
    assert!(
        fallback
            .precompiled_path(fallback.text("commands").unwrap())
            .is_none()
    );
    let mut loaded = fallback.clone();
    loaded.set_loaded_vector_path(xrack_runtime::LoadedVectorPath::new(
        commands,
        plan.bounds,
        plan.segments.to_vec(),
    ));
    let loaded_pixels = renderer.snapshot(&loaded, 128, 128, 1.0, &input).unwrap();
    let planned_pixels = renderer.snapshot(&badge, 128, 128, 1.0, &input).unwrap();
    let parsed_pixels = renderer.snapshot(&fallback, 128, 128, 1.0, &input).unwrap();
    let corner = planned_pixels.pixel(0, 0).unwrap();
    assert!(
        planned_pixels
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel != corner)
    );
    assert_eq!(planned_pixels.rgba, parsed_pixels.rgba);
    assert_eq!(planned_pixels.rgba, loaded_pixels.rgba);
    badge
        .properties
        .insert("commands", xrack_runtime::Value::String("M0 0 L1 1".into()));
    assert!(
        badge
            .precompiled_path(badge.text("commands").unwrap())
            .is_none()
    );

    assert_eq!(
        scene
            .find("png-image")
            .unwrap()
            .image("source")
            .unwrap()
            .size(),
        [96, 48]
    );
    assert_eq!(
        snapshot.pixel(123, 120),
        Some(card),
        "PNG contain letterbox"
    );
    assert_ne!(snapshot.pixel(123, 166), Some(card), "PNG image");
    assert_eq!(snapshot.pixel(253, 110), Some(card), "transparent SVG");
    assert_eq!(snapshot.pixel(330, 186), Some([0xff, 0xe5, 0x96, 255]));
    assert_ne!(snapshot.pixel(537, 143), Some(card), "gradient path fill");
    assert_eq!(snapshot.pixel(537, 188), Some(card), "path hole");
    let stroke = snapshot.pixel(474, 173).unwrap();
    assert!(
        stroke[0] > 140 && stroke[1] > 210,
        "path stroke: {stroke:?}"
    );

    assert!(demo.dispatch("toggle", "clicked"));
    assert!(interpreted.dispatch("toggle", "clicked"));
    let cropped = renderer
        .snapshot(&demo.render(), 660, 376, 1.0, &input)
        .unwrap();
    let interpreted_cropped = renderer
        .snapshot(&interpreted.try_render().unwrap(), 660, 376, 1.0, &input)
        .unwrap();
    assert_same_pixels(&interpreted_cropped, &cropped);
    assert_ne!(
        cropped.pixel(123, 120),
        Some(card),
        "PNG cover fills the viewport"
    );
    assert_eq!(
        cropped.pixel(34, 120),
        Some(card),
        "PNG cover clips at the viewport"
    );

    let hidpi = renderer.snapshot(&scene, 1320, 752, 2.0, &input).unwrap();
    let interpreted_hidpi = renderer
        .snapshot(&interpreted_scene, 1320, 752, 2.0, &input)
        .unwrap();
    assert_same_pixels(&interpreted_hidpi, &hidpi);
    assert_eq!(hidpi.layout.bounds, snapshot.layout.bounds);
    assert_eq!(hidpi.pixel(660, 372), Some([0xff, 0xe5, 0x96, 255]));
    assert_eq!(hidpi.pixel(1074, 376), Some(card));

    renderer.set_vector_mask_cache_enabled(true);
    let cold = renderer.snapshot(&scene, 660, 376, 1.0, &input).unwrap();
    assert_same_pixels(&cold, &snapshot);
    let stats = renderer.vector_mask_stats();
    let warmed = renderer.snapshot(&scene, 660, 376, 1.0, &input).unwrap();
    assert_eq!(
        renderer.vector_mask_stats().rasterized - stats.rasterized,
        2
    );
    let warmed_interpreter = renderer
        .snapshot(&interpreted_scene, 660, 376, 1.0, &input)
        .unwrap();
    assert_same_pixels(&warmed_interpreter, &warmed);
    assert!(renderer.vector_mask_stats().reused > stats.reused);
    assert!(
        warmed
            .rgba
            .iter()
            .zip(&snapshot.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 3)
    );

    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        snapshot
            .save_png(std::path::Path::new(&path).join("media-demo.png"))
            .unwrap();
    }
}

#[test]
fn styled_text_shapes_bidi_spans_and_elides_without_losing_the_source_text() {
    use layout_ui::StyledTextDemo;
    use xrack_layout::Rect;
    use xrack_renderer_femtovg::Snapshot;
    use xrack_runtime::{Color, StyledRun, StyledText, TextStyle};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StyledTextDemo::new();
    let initial = ui.get_body();
    assert!(initial.text().contains("粗体、斜体、xrackc"));
    assert!(initial.runs().iter().any(|run| run.style.monospace));

    let body = "א ב\n\nHIDDEN";
    let start_of_tail = body.find("HIDDEN").unwrap();
    let span = |start, end, rgba| StyledRun {
        start,
        end,
        style: TextStyle {
            color: Some(Color(rgba)),
            bold: true,
            ..Default::default()
        },
    };
    ui.set_body(
        StyledText::from_runs(
            body,
            vec![
                span(0, "א".len(), 0xff0000ff),
                span("א ".len(), "א ב".len(), 0x00ff00ff),
                span(start_of_tail, body.len(), 0x0050ffff),
            ],
        )
        .unwrap(),
    );

    let colored_x = |snapshot: &Snapshot, rect: Rect, channel: usize| {
        (rect.x as u32..(rect.x + rect.width) as u32).find(|&x| {
            (rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
                let pixel = snapshot.pixel(x, y).unwrap();
                pixel[channel] > 100
                    && (0..3)
                        .filter(|&other| other != channel)
                        .all(|other| u16::from(pixel[channel]) > u16::from(pixel[other]) + 48)
            })
        })
    };
    let input = Interaction::default();
    let scene = ui.render_shared();
    let full = renderer.snapshot(&scene, 460, 268, 1.0, &input).unwrap();
    let bounds = full.layout.at_path(&[1]).unwrap().bounds;
    let red = colored_x(&full, bounds, 0).expect("red alef");
    let green = colored_x(&full, bounds, 1).expect("green bet");
    assert!(green < red, "right-to-left style order");
    assert!(
        colored_x(&full, bounds, 2).is_some(),
        "visible second paragraph"
    );

    ui.set_compact(true);
    let scene = ui.render_shared();
    let clipped = renderer.snapshot(&scene, 460, 268, 1.0, &input).unwrap();
    let visible = clipped.layout.at_path(&[1]).unwrap().bounds;
    assert!(visible.height < bounds.height);
    assert_eq!(colored_x(&clipped, visible, 2), None);
    assert_eq!(scene.at_path(&[1]).unwrap().text("text"), Some(body));
}

#[test]
fn text_selection_colors_keep_shaping_and_match_between_backends() {
    use form_ui::SelectionColorsDemo;
    use xrack_layout::Rect;
    use xrack_renderer_femtovg::Snapshot;
    use xrack_runtime::{Color, Node, TextCommand};

    const HIGHLIGHT: [u8; 4] = [0x70, 0x46, 0xbf, 0xff];
    const SELECTED: [u8; 4] = [0xff, 0xeb, 0x9a, 0xff];
    const NORMAL: [u8; 4] = [0x83, 0xc7, 0xff, 0xff];
    const LINK: [u8; 4] = [0xed, 0xb0, 0x85, 0xff];

    fn selected_frame(renderer: &mut Renderer, scene: &Node, field: usize) -> Snapshot {
        let mut input = Interaction::default();
        let initial = renderer.snapshot(scene, 496, 316, 1.0, &input).unwrap();
        let path = [field];
        let target = input
            .accessibility_target(scene, &initial.layout, &path)
            .unwrap();
        input.focus_target(scene, &initial.layout, target);
        input.sync_editor(scene);
        let text = scene.at_path(&path).unwrap().text("text").unwrap();
        let anchor = text.find("office").unwrap() + 2;
        let focus = text.find("Done").unwrap();
        input.apply_text_selection(
            scene,
            TextCommand::SetSelection {
                anchor: anchor as i64,
                focus: focus as i64,
            },
        );
        renderer.snapshot(scene, 496, 316, 1.0, &input).unwrap()
    }

    fn count(frame: &Snapshot, bounds: Rect, rgba: [u8; 4]) -> usize {
        (bounds.y as u32..(bounds.y + bounds.height) as u32)
            .flat_map(|y| (bounds.x as u32..(bounds.x + bounds.width) as u32).map(move |x| (x, y)))
            .filter(|(x, y)| frame.pixel(*x, *y) == Some(rgba))
            .count()
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut aot = SelectionColorsDemo::new();
    let interpreted = interpret_example("examples/form/form.xr")
        .create("SelectionColorsDemo")
        .unwrap();
    let opaque_aot = aot.render_shared();
    let opaque_interp = interpreted.render_shared();
    aot.set_selected_ink(Color(0x12ab3400));
    interpreted
        .set("selected-ink", Color(0x12ab3400).into())
        .unwrap();
    let transparent_aot = aot.render_shared();
    let transparent_interp = interpreted.render_shared();
    assert!(opaque_aot.same_frame_inputs(&transparent_aot));
    assert!(opaque_interp.same_frame_inputs(&transparent_interp));

    for field in 0..3 {
        let opaque = selected_frame(&mut renderer, &opaque_aot, field);
        assert_same_pixels(
            &opaque,
            &selected_frame(&mut renderer, &opaque_interp, field),
        );
        let transparent = selected_frame(&mut renderer, &transparent_aot, field);
        assert_same_pixels(
            &transparent,
            &selected_frame(&mut renderer, &transparent_interp, field),
        );
        let bounds = opaque.layout.at_path(&[field]).unwrap().bounds;
        let glyph_pixels = count(&opaque, bounds, SELECTED);
        assert!(
            glyph_pixels > 12,
            "field {field} is missing the selected glyph color"
        );
        assert!(count(&opaque, bounds, HIGHLIGHT) > glyph_pixels);
        assert_eq!(count(&transparent, bounds, SELECTED), 0);
        assert!(count(&transparent, bounds, HIGHLIGHT) > count(&opaque, bounds, HIGHLIGHT));
        assert!(count(&opaque, bounds, NORMAL) > 12);
        for original in [NORMAL, LINK] {
            assert_eq!(
                count(&opaque, bounds, original),
                count(&transparent, bounds, original),
                "normal ink leaked behind transparent selection in field {field}",
            );
        }
    }
}

#[test]
fn selectable_styled_text_keeps_link_activation_and_copies_plain_text_in_both_backends() {
    use layout_ui::StyledTextDemo;
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, KeyModifiers, PointerButton, PointerEvent, StyledText};

    fn exercise(
        renderer: &mut Renderer,
        ui: Box<dyn Component>,
        document: &str,
        read: impl Fn() -> (String, String),
    ) {
        let mut preview = PreviewScene::new(ui);
        preview.advance(renderer);
        let (scene, layout, _) = preview.inspect_frame().unwrap();
        let body = layout.at_path(&[1]).unwrap();
        let link = body.text_link_regions().next().unwrap().1;
        let start = Point {
            x: link.x + 2.0,
            y: link.y + link.height / 2.0,
        };
        let end = Point {
            x: body.bounds.x + body.bounds.width - 2.0,
            y: body.bounds.y + body.bounds.height - 2.0,
        };
        let mut native = Interaction::default();
        native.pointer_move(scene, layout, start);
        native.pointer_press_in_scene(scene, layout);
        native.pointer_move(
            scene,
            layout,
            Point {
                x: link.x + link.width - 2.0,
                y: start.y,
            },
        );
        assert!(
            native
                .hovered
                .as_ref()
                .is_some_and(|target| target.text_link_index() == Some(0))
        );
        assert!(native.pointer_release_target().is_none());
        let contact = PointerContact {
            id: 41,
            primary: true,
            event: PointerEvent {
                button: PointerButton::Primary,
                ..Default::default()
            },
        };
        assert!(preview.press_contact(contact, start, false));
        preview.advance(renderer);
        assert!(preview.move_contact(contact, end));
        preview.advance(renderer);
        preview.finish_contact(contact, end, false, false);
        preview.advance(renderer);
        let (selected, link) = read();
        assert!(selected.contains("code"), "{selected:?}");
        assert!(document.contains(&selected));
        assert!(link.is_empty());

        let key = |key: &str| KeyEvent {
            key: key.into(),
            modifiers: KeyModifiers {
                control: true,
                primary: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(preview.keyboard_for_platform(renderer, &key("c"), true, false));
        assert_eq!(
            preview.take_clipboard_request().unwrap().text.as_deref(),
            Some(selected.as_str())
        );
        assert!(preview.keyboard_for_platform(renderer, &key("a"), true, false));
        preview.advance(renderer);
        assert_eq!(read().0, document);
        let home = KeyEvent {
            key: "Home".into(),
            ..Default::default()
        };
        assert!(preview.keyboard_for_platform(renderer, &home, true, false));
        preview.advance(renderer);
        let extend = KeyEvent {
            key: "ArrowRight".into(),
            modifiers: KeyModifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        };
        for _ in 0..5 {
            assert!(preview.keyboard_for_platform(renderer, &extend, true, false));
            preview.advance(renderer);
        }
        assert_eq!(read().0, "Bold ");
        preview.click(start);
        preview.advance(renderer);
        assert_eq!(read().1, "#target");
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let document = StyledText::from_markdown(
        "**Bold** [long link](#target), éclair and `code`.\n\nمرحبا XRack",
    )
    .unwrap();
    let mut aot = StyledTextDemo::new();
    aot.set_selectable(true);
    aot.set_body(document.clone());
    let control = aot.as_weak().upgrade().unwrap();
    exercise(&mut renderer, Box::new(aot), document.text(), || {
        (control.get_selected(), control.get_last_link())
    });
    let interpreted = interpret_example("examples/layout/layout.xr")
        .create("StyledTextDemo")
        .unwrap();
    interpreted.set("selectable", true.into()).unwrap();
    interpreted.set("body", document.clone().into()).unwrap();
    let control = interpreted.clone();
    let get = |name| match control.get(name).unwrap() {
        Value::String(value) => value,
        value => panic!("expected string, got {value:?}"),
    };
    exercise(
        &mut renderer,
        Box::new(interpreted),
        document.text(),
        || (get("selected"), get("last-link")),
    );
}

#[test]
fn wrapped_text_link_geometry_tracks_gpu_paint_and_focus() {
    use layout_ui::StyledTextDemo;
    use xrack_runtime::StyledText;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StyledTextDemo::new();
    ui.set_body(StyledText::from_markdown(
        "See [**wrapping links** follow the paint geometry across lines of inline text](#wrapped).",
    ).unwrap());
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let normal = renderer.snapshot(&scene, 460, 268, 1.0, &input).unwrap();
    let link_bounds: Vec<_> = normal
        .layout
        .at_path(&[1])
        .unwrap()
        .text_link_regions()
        .filter_map(|(index, region)| (index == 0).then_some(region))
        .collect();
    assert!(link_bounds.iter().any(|region| region.y > link_bounds[0].y));
    for rect in &link_bounds {
        assert!(
            (rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
                (rect.x as u32..(rect.x + rect.width) as u32).any(|x| {
                    let [r, g, b, _] = normal.pixel(x, y).unwrap();
                    r > 100 && g > r.saturating_add(45) && b >= g
                })
            }),
            "missing link-colored pixels in {rect:?}"
        );
        input.pointer_move(
            &scene,
            &normal.layout,
            Point {
                x: rect.x + rect.width * 0.5,
                y: rect.y + rect.height * 0.5,
            },
        );
        assert_eq!(input.hovered.as_ref().unwrap().text_link_index(), Some(0));
    }
    input.focus_next(&scene, &normal.layout, false);
    let focused = renderer.snapshot(&scene, 460, 268, 1.0, &input).unwrap();
    assert!(link_bounds.iter().any(|rect| {
        (rect.y as u32..(rect.y + rect.height) as u32).any(|y| {
            (rect.x as u32..(rect.x + rect.width) as u32)
                .any(|x| normal.pixel(x, y) != focused.pixel(x, y))
        })
    }));
}

#[test]
fn nine_slice_preserves_asymmetric_corners_and_tiles_each_axis() {
    use media_ui::NineSliceDemo;
    use xrack_runtime::{ImageTileMode, Length, NineSlice, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = NineSliceDemo::new();
    let input = Interaction::default();
    let snapshot = |renderer: &mut Renderer, ui: &NineSliceDemo| {
        renderer
            .snapshot(&ui.render_shared(), 460, 268, 1.0, &input)
            .unwrap()
    };
    let stretch = snapshot(&mut renderer, &ui);
    let interpreted = interpret_example("examples/media/media.xr")
        .create("NineSliceDemo")
        .unwrap();
    let loaded = |renderer: &mut Renderer| {
        renderer
            .snapshot(&interpreted.try_render().unwrap(), 460, 268, 1.0, &input)
            .unwrap()
    };
    assert_same_pixels(&loaded(&mut renderer), &stretch);
    let first = Some([0x6b, 0x3d, 0x92, 255]);
    let second = Some([0xd4, 0x4b, 0x76, 255]);
    let top_left = Some([0xff, 0xd1, 0x77, 255]);
    let top_right = Some([0xfc, 0x88, 0x6d, 255]);
    let bottom_left = Some([0x74, 0xcb, 0xf8, 255]);
    let bottom_right = Some([0x90, 0xe4, 0xa1, 255]);

    assert_eq!(stretch.pixel(26, 65), top_left);
    assert_eq!(stretch.pixel(224, 65), top_right);
    assert_eq!(stretch.pixel(26, 190), bottom_left);
    assert_eq!(stretch.pixel(224, 190), bottom_right);
    assert_eq!(stretch.pixel(53, 65), first);
    assert_eq!(stretch.pixel(218, 65), second);

    ui.set_horizontal_mode(ImageTileMode::Repeat);
    interpreted
        .set("horizontal-mode", ImageTileMode::Repeat.into())
        .unwrap();
    let repeat = snapshot(&mut renderer, &ui);
    assert_same_pixels(&loaded(&mut renderer), &repeat);
    assert_eq!(repeat.pixel(37, 65), first);
    assert_eq!(repeat.pixel(45, 65), second);
    assert_eq!(repeat.pixel(53, 65), first);
    assert_eq!(repeat.pixel(213, 65), first);
    assert_eq!(repeat.pixel(224, 65), top_right);
    assert_eq!(repeat.pixel(26, 93), stretch.pixel(26, 93));

    ui.set_horizontal_mode(ImageTileMode::Round);
    ui.set_vertical_mode(ImageTileMode::Repeat);
    interpreted
        .set("horizontal-mode", ImageTileMode::Round.into())
        .unwrap();
    interpreted
        .set("vertical-mode", ImageTileMode::Repeat.into())
        .unwrap();
    let round = snapshot(&mut renderer, &ui);
    assert_same_pixels(&loaded(&mut renderer), &round);
    assert_eq!(round.pixel(218, 65), second);
    assert_eq!(round.pixel(26, 72), round.pixel(26, 92));
    assert_ne!(round.pixel(26, 72), round.pixel(26, 82));
    let hidpi = renderer
        .snapshot(&ui.render_shared(), 920, 536, 2.0, &input)
        .unwrap();
    assert_eq!(hidpi.pixel(52, 130), top_left);
    assert_eq!(hidpi.pixel(448, 380), bottom_right);

    ui.set_panel_width(Length(8.0));
    ui.set_panel_height(Length(6.0));
    let tiny = snapshot(&mut renderer, &ui);
    assert_eq!(tiny.pixel(26, 65), top_left);
    assert_eq!(tiny.pixel(30, 65), top_right);
    assert_eq!(tiny.pixel(26, 68), bottom_left);
    assert_eq!(tiny.pixel(30, 68), bottom_right);
    assert_eq!(tiny.pixel(32, 68), Some([0x10, 0x17, 0x25, 255]));

    // A Rust application can apply the same metadata to an embedded PNG.
    let scene = MediaDemo::new().render_shared();
    let png = scene.find("png-image").unwrap().image("source").unwrap();
    let mut original = Node::new("Image", None);
    original.properties.insert("source", png.clone().into());
    let reference = renderer.snapshot(&original, 96, 48, 1.0, &input).unwrap();
    ui.set_frame(
        png.with_nine_slice(NineSlice {
            top: 4,
            right: 6,
            bottom: 8,
            left: 10,
        })
        .unwrap(),
    );
    ui.set_panel_width(Length(203.0));
    ui.set_panel_height(Length(129.0));
    let png = snapshot(&mut renderer, &ui);
    assert_eq!(png.pixel(26, 65), reference.pixel(2, 1));
    assert_eq!(png.pixel(224, 65), reference.pixel(93, 1));
}

#[test]
fn animated_gif_samples_composited_frames_and_freezes_offscreen_and_when_paused() {
    use media_ui::AnimatedImageDemo;
    use std::time::{Duration, Instant};
    use xrack_runtime::{Image, ImagePlaybackQueue};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = AnimatedImageDemo::new();
    ui.set_passes(0);
    let mut playback = ImagePlaybackQueue::default();
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let sync = |playback: &mut ImagePlaybackQueue, ui: &AnimatedImageDemo, ms| {
        let scene = ui.render_shared();
        let pulse = scene.find("pulse").unwrap();
        playback.sync(
            at(ms),
            [(pulse.image_playback.as_ref().unwrap(), pulse.is_visible())],
        );
    };
    let input = Interaction::default();
    let capture = |renderer: &mut Renderer, ui: &AnimatedImageDemo| {
        renderer
            .snapshot(&ui.render_shared(), 460, 258, 1.0, &input)
            .unwrap()
    };
    let coral = Some([0xfc, 0x88, 0x6d, 255]);
    let teal = Some([0x39, 0xd6, 0xbc, 255]);
    let blue = Some([0x51, 0x6c, 0xfe, 255]);

    sync(&mut playback, &ui, 0);
    assert_eq!(ui.get_frame_total(), 3);
    assert_eq!(playback.deadline(), Some(at(80)));
    let initial = capture(&mut renderer, &ui);
    assert_eq!(initial.pixel(60, 80), coral);
    assert_eq!(initial.pixel(190, 80), coral);

    assert!(!playback.advance(at(79)));
    assert!(playback.advance(at(80)));
    assert_eq!(ui.get_frame(), 1);
    let first = capture(&mut renderer, &ui);
    assert_eq!(first.pixel(60, 80), teal);
    assert_eq!(first.pixel(190, 80), coral);
    assert_eq!(first.pixel(300, 80), coral);
    let mut cropped = ui.render();
    let pulse = cropped
        .children
        .iter_mut()
        .find(|node| node.id == Some("pulse"))
        .unwrap();
    pulse.properties.insert("source-clip-x", 16_i64.into());
    let cropped_frame = renderer.snapshot(&cropped, 460, 258, 1.0, &input).unwrap();
    assert_eq!(
        cropped_frame.pixel(60, 80),
        coral,
        "cropping follows the sampled GIF frame"
    );

    let pulse = cropped
        .children
        .iter_mut()
        .find(|node| node.id == Some("pulse"))
        .unwrap();
    pulse
        .properties
        .insert("colorize", xrack_runtime::Color(0xffd177ff).into());
    let tinted = renderer.snapshot(&cropped, 460, 258, 1.0, &input).unwrap();
    assert_eq!(tinted.pixel(60, 80), Some([255, 209, 119, 255]));
    assert_eq!(
        tinted.pixel(300, 80),
        coral,
        "the static preview keeps the source color"
    );

    ui.set_pause(true);
    sync(&mut playback, &ui, 100);
    assert!(playback.deadline().is_none());
    assert!(!playback.advance(at(1000)));
    assert_eq!(ui.get_frame(), 1);

    ui.set_pause(false);
    ui.set_rate(2.0);
    sync(&mut playback, &ui, 1000);
    assert_eq!(playback.deadline(), Some(at(1050)));
    assert!(playback.advance(at(1050)));
    assert_eq!(ui.get_frame(), 2);
    let second = capture(&mut renderer, &ui);
    assert_eq!(second.pixel(60, 80), coral, "restore-previous GIF disposal");
    assert_eq!(second.pixel(190, 80), blue);

    ui.set_show(false);
    sync(&mut playback, &ui, 1060);
    assert!(playback.deadline().is_none());
    assert!(!playback.advance(at(2000)));
    assert_eq!(ui.get_frame_total(), 3);
    ui.set_show(true);
    sync(&mut playback, &ui, 2000);
    assert_eq!(playback.deadline(), Some(at(2020)));
    assert!(playback.advance(at(2020)));
    assert_eq!(ui.get_frame(), 0);

    assert!(playback.advance(at(2150)));
    assert_eq!(ui.get_frame(), 2);
    assert!(playback.next_finished().unwrap().finish());
    assert_eq!(ui.get_completions(), 1);
    assert!(playback.next_finished().is_none());
    assert!(playback.deadline().is_none());

    ui.set_running(false);
    sync(&mut playback, &ui, 2150);
    assert_eq!(ui.get_frame(), 0);
    ui.set_animation(Image::Empty);
    sync(&mut playback, &ui, 2150);
    assert_eq!(ui.get_frame_total(), 0);
}

#[test]
fn interpreted_and_compiled_gif_paint_the_same_frames_and_completion() {
    use media_ui::AnimatedImageDemo;
    use std::time::{Duration, Instant};
    use xrack_runtime::{Component, ImagePlaybackQueue};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let interpreted = interpret_example("examples/media/media.xr")
        .create("AnimatedImageDemo")
        .unwrap();
    let mut generated = AnimatedImageDemo::new();
    interpreted.set("passes", 1_i64.into()).unwrap();
    generated.set_passes(1);
    let mut actual_clock = ImagePlaybackQueue::default();
    let mut expected_clock = ImagePlaybackQueue::default();
    let start = Instant::now();
    let input = Interaction::default();
    let sample = |clock: &mut ImagePlaybackQueue, scene: &xrack_runtime::Node, now| {
        let image = scene.find("pulse").unwrap();
        clock.sync(
            now,
            [(image.image_playback.as_ref().unwrap(), image.is_visible())],
        );
        clock.advance(now);
        clock.next_finished().is_some_and(|event| event.finish())
    };
    let mut previous_color = None;
    for (elapsed, completed) in [(0, false), (80, false), (200, false), (400, true)] {
        let now = start + Duration::from_millis(elapsed);
        assert_eq!(
            sample(&mut actual_clock, &interpreted.try_render().unwrap(), now),
            completed
        );
        assert_eq!(
            sample(&mut expected_clock, &generated.render_shared(), now),
            completed
        );
        assert_eq!(actual_clock.deadline(), expected_clock.deadline());
        let actual = renderer
            .snapshot(&interpreted.try_render().unwrap(), 460, 258, 1.0, &input)
            .unwrap();
        let expected = renderer
            .snapshot(&generated.render_shared(), 460, 258, 1.0, &input)
            .unwrap();
        assert_same_pixels(&actual, &expected);
        let color = actual.pixel(60, 80).unwrap();
        if elapsed < 400 {
            assert_ne!(Some(color), previous_color);
        }
        previous_color = Some(color);
    }
    assert_eq!(generated.get_completions(), 1);
    assert_eq!(interpreted.get("completions"), Ok(1_i64.into()));
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn sprite_atlas_clips_before_layout_fit_and_isolates_gpu_sampling() {
    use media_ui::{NineSliceDemo, SpriteAtlasDemo};
    use xrack_runtime::{ImageFit, ImageRendering, ImageTileMode};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = SpriteAtlasDemo::new();
    let input = Interaction::default();
    let capture = |renderer: &mut Renderer, ui: &SpriteAtlasDemo| {
        renderer
            .snapshot(&ui.render_shared(), 460, 282, 1.0, &input)
            .unwrap()
    };
    let coral = Some([252, 136, 109, 255]);
    let gold = Some([255, 209, 119, 255]);
    let blue = Some([81, 108, 254, 255]);
    let teal = Some([57, 214, 188, 255]);
    let panel = Some([0x26, 0x36, 0x53, 255]);

    assert_eq!((ui.get_atlas_width(), ui.get_atlas_height()), (8, 8));
    let nearest = capture(&mut renderer, &ui);
    assert_eq!(nearest.layout.at_path(&[3]).unwrap().bounds.height, 48.0);
    assert_eq!(nearest.pixel(260, 90), panel, "right-aligned contain");
    assert_eq!(nearest.pixel(334, 90), coral);
    assert_eq!(nearest.pixel(399, 90), gold);

    ui.set_sampling(ImageRendering::Smooth);
    let smooth = capture(&mut renderer, &ui);
    assert_ne!(smooth.pixel(334, 90), coral);
    assert_eq!(
        smooth.pixel(399, 90),
        gold,
        "the next atlas cell cannot bleed through"
    );
    ui.set_column(1);
    let second = capture(&mut renderer, &ui);
    assert_eq!(second.pixel(280, 90), blue);
    assert_eq!(second.pixel(399, 90), teal);

    ui.set_cell_height(2);
    let shallow = capture(&mut renderer, &ui);
    assert_eq!(shallow.layout.at_path(&[3]).unwrap().bounds.height, 24.0);
    assert_eq!(shallow.pixel(260, 90), panel, "bottom-aligned contain");
    assert_eq!(shallow.pixel(260, 140), blue);
    ui.set_sampling(ImageRendering::Pixelated);
    ui.set_fit_mode(ImageFit::Cover);
    let cover = capture(&mut renderer, &ui);
    assert_eq!(cover.pixel(255, 70), blue);
    assert_eq!(cover.pixel(395, 70), teal);
    ui.set_fit_mode(ImageFit::Contain);

    ui.set_column(-1);
    let empty = capture(&mut renderer, &ui);
    assert_eq!(empty.layout.at_path(&[3]).unwrap().bounds.height, 0.0);
    assert_eq!(empty.pixel(260, 140), panel);

    ui.set_column(0);
    ui.set_cell_height(4);
    ui.set_sampling(ImageRendering::Pixelated);
    ui.set_tile_mode(ImageTileMode::Repeat);
    let repeat = capture(&mut renderer, &ui);
    assert_eq!(repeat.pixel(251, 90), coral);
    assert_eq!(repeat.pixel(253, 90), gold);
    assert_eq!(repeat.pixel(399, 90), coral);
    ui.set_tile_mode(ImageTileMode::Round);
    assert_eq!(capture(&mut renderer, &ui).pixel(399, 90), gold);

    let border = NineSliceDemo::new();
    let scene = border.render_shared();
    ui.set_tile_mode(ImageTileMode::Stretch);
    ui.set_sampling(ImageRendering::Smooth);
    ui.set_atlas(scene.children[2].image("source").unwrap());
    ui.set_column(4);
    let svg = capture(&mut renderer, &ui);
    assert_eq!(svg.pixel(290, 90), Some([0x6b, 0x3d, 0x92, 255]));
    assert_eq!(svg.pixel(390, 90), Some([0xd4, 0x4b, 0x76, 255]));

    ui.set_atlas(border.get_frame());
    ui.set_column(0);
    let sliced_corner = capture(&mut renderer, &ui);
    assert_eq!(sliced_corner.pixel(260, 140), gold);
    assert_eq!(sliced_corner.pixel(390, 140), gold);
}

#[test]
fn image_colorize_preserves_alpha_and_source_gradients_across_shared_textures() {
    use media_ui::{ImageColorizeDemo, NineSliceDemo};
    use xrack_runtime::{Brush, Color, GradientStop, ImageTileMode};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ImageColorizeDemo::new();
    let input = Interaction::default();
    let capture = |renderer: &mut Renderer, ui: &ImageColorizeDemo| {
        renderer
            .snapshot(&ui.render_shared(), 624, 266, 1.0, &input)
            .unwrap()
    };
    let bg = Some([16, 23, 37, 255]);
    let teal = Some([57, 214, 188, 255]);
    let initial = capture(&mut renderer, &ui);
    assert_eq!(initial.pixel(31, 90), Some([244, 95, 81, 255]));
    assert_eq!(initial.pixel(183, 90), teal);
    let half_alpha = initial.pixel(222, 90).unwrap();
    for (channel, expected) in half_alpha.into_iter().zip([37u8, 119, 113, 255]) {
        assert!(
            channel.abs_diff(expected) <= 1,
            "half-alpha color {half_alpha:?}"
        );
    }
    assert_eq!(initial.pixel(251, 90), bg);
    let original_scene = ui.render_shared();
    ui.set_accent(Brush::Solid(Color(0x39d6bc80)));
    let translucent = capture(&mut renderer, &ui);
    assert_eq!(translucent.pixel(183, 90), Some(half_alpha));
    assert_eq!(translucent.pixel(251, 90), bg);

    let vertical = Brush::Linear {
        angle_degrees: 180.0,
        stops: [
            GradientStop {
                offset: 0.0,
                color: Color(0xff0000ff),
            },
            GradientStop {
                offset: 1.0,
                color: Color(0x0000ffff),
            },
        ]
        .into(),
    };
    ui.set_accent(vertical);
    assert!(original_scene.same_frame_inputs(&ui.render_shared()));
    let gradients = capture(&mut renderer, &ui);
    let top = gradients.pixel(183, 69).unwrap();
    let bottom = gradients.pixel(183, 170).unwrap();
    assert!(top[0] > 200 && top[2] < 60, "top gradient {top:?}");
    assert!(
        bottom[2] > 200 && bottom[0] < 60,
        "bottom gradient {bottom:?}"
    );
    assert_eq!(gradients.pixel(251, 90), bg);
    assert_eq!(gradients.pixel(338, 95), initial.pixel(338, 95));
    assert_eq!(gradients.pixel(436, 95), initial.pixel(436, 95));
    assert_ne!(gradients.pixel(540, 121), bg);
    assert_eq!(gradients.pixel(484, 65), bg);
    let again = capture(&mut renderer, &ui);
    assert_eq!(again.rgba, gradients.rgba);

    let mut layered = ui.render();
    layered
        .children
        .iter_mut()
        .find(|node| node.id == Some("accent-preview"))
        .unwrap()
        .properties
        .insert("opacity", 0.5_f64.into());
    let faded = renderer.snapshot(&layered, 624, 266, 1.0, &input).unwrap();
    let faded_top = faded.pixel(183, 69).unwrap();
    for (index, background) in [16, 23, 37].into_iter().enumerate() {
        let expected = ((u16::from(top[index]) + background) / 2) as u8;
        assert!(faded_top[index].abs_diff(expected) <= 1);
    }
    assert_eq!(faded.pixel(338, 95), gradients.pixel(338, 95));

    ui.set_clip_x(12);
    ui.set_clip_width(4);
    ui.set_tile_mode(ImageTileMode::Repeat);
    let tiled = capture(&mut renderer, &ui);
    let right = tiled.pixel(329, 95).unwrap();
    assert!(
        right[2] > right[0].saturating_add(60),
        "source-space color {right:?}"
    );
    assert_eq!(tiled.pixel(329, 95), tiled.pixel(333, 95));
    ui.set_accent(Brush::Solid(Color(0x00000000)));
    let original = capture(&mut renderer, &ui);
    assert_eq!(original.pixel(183, 90), Some([247, 184, 77, 255]));

    ui.set_mask(NineSliceDemo::new().get_frame());
    ui.set_clip_x(0);
    ui.set_clip_width(32);
    ui.set_tile_mode(ImageTileMode::Stretch);
    let sliced = capture(&mut renderer, &ui);
    let left_of_cut = sliced.pixel(337, 65).unwrap();
    let right_of_cut = sliced.pixel(338, 65).unwrap();
    assert!(left_of_cut[0].abs_diff(right_of_cut[0]) <= 12);
    assert!(left_of_cut[0] > right_of_cut[2]);
    assert_eq!(sliced.pixel(326, 65), bg);
}

#[test]
fn whole_subtree_opacity_and_sibling_z_match_gpu_and_input() {
    use layout_ui::CompositingDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = CompositingDemo::new();
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let half = renderer.snapshot(&scene, 460, 292, 1.0, &input).unwrap();
    let assert_rgb = |pixel: Option<[u8; 4]>, expected: [u8; 4]| {
        let pixel = pixel.unwrap();
        assert!(
            pixel
                .iter()
                .zip(expected)
                .all(|(actual, target)| actual.abs_diff(target) <= 1),
            "{pixel:?} != {expected:?}"
        );
    };
    assert_rgb(half.pixel(36, 66), [136, 12, 19, 255]);
    assert_rgb(half.pixel(78, 91), [8, 12, 146, 255]);
    assert_rgb(half.pixel(108, 108), [8, 75, 82, 255]);
    assert_eq!(half.pixel(312, 92), Some([57, 214, 188, 255]));
    let point = Point { x: 312.0, y: 92.0 };
    input.pointer_move(&scene, &half.layout, point);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("first"));

    let stats = renderer.opacity_layer_stats();
    assert_eq!(stats.rasterized, 1);
    ui.set_fade(0.25);
    let quarter = renderer
        .snapshot(&ui.render_shared(), 460, 292, 1.0, &input)
        .unwrap();
    assert_eq!(quarter.layout, half.layout);
    assert_rgb(quarter.pixel(78, 91), [12, 17, 92, 255]);
    assert_eq!(renderer.opacity_layer_stats().rasterized, stats.rasterized);
    assert_eq!(renderer.opacity_layer_stats().reused, stats.reused + 1);

    let mut changed = ui.render();
    changed.children[2].children[0]
        .properties
        .insert("background", xrack_runtime::Color(0xffff00ff).into());
    let refreshed = renderer.snapshot(&changed, 460, 292, 1.0, &input).unwrap();
    assert_rgb(refreshed.pixel(78, 91), [76, 81, 28, 255]);
    if cfg!(any(target_os = "ios", target_os = "android")) {
        assert_eq!(
            renderer.opacity_layer_stats().rasterized,
            stats.rasterized + 1
        );
    } else {
        assert_eq!(renderer.opacity_layer_stats().rasterized, stats.rasterized);
        assert_eq!(renderer.opacity_atlas_stats().groups, 0);
    }

    ui.set_fade(0.0);
    let clear = ui.render_shared();
    assert!(
        scene
            .find("composite")
            .unwrap()
            .same_frame_inputs(clear.find("composite").unwrap())
    );
    let invisible = renderer
        .snapshot(&clear, 460, 292, 1.0, &Interaction::default())
        .unwrap();
    assert_eq!(invisible.layout, half.layout);
    assert_eq!(invisible.pixel(78, 91), Some([16, 23, 37, 255]));

    ui.set_first_layer(0.0);
    let behind = ui.render_shared();
    let reordered = renderer
        .snapshot(&behind, 460, 292, 1.0, &Interaction::default())
        .unwrap();
    assert_eq!(reordered.pixel(312, 92), Some([255, 209, 119, 255]));
    input.reconcile(&behind, &reordered.layout);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("second"));

    ui.set_fade(1.0);
    let opaque = renderer
        .snapshot(&ui.render_shared(), 920, 584, 2.0, &Interaction::default())
        .unwrap();
    assert_eq!(opaque.pixel(156, 182), Some([0, 0, 255, 255]));
    assert_rgb(opaque.pixel(216, 216), [0, 128, 127, 255]);
}

#[test]
fn rectangle_shadows_respect_alpha_rounding_and_ancestor_clips() {
    use layout_ui::RectangleShadowDemo;
    use xrack_runtime::{Brush, Color, GradientStop, Length};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RectangleShadowDemo::new();
    let input = Interaction::default();
    let capture = |renderer: &mut Renderer, ui: &RectangleShadowDemo| {
        renderer
            .snapshot(&ui.render_shared(), 460, 268, 1.0, &input)
            .unwrap()
    };
    let panel = Some([38, 54, 83, 255]);
    let window = Some([16, 23, 37, 255]);
    let teal = Some([57, 214, 188, 255]);
    let surface = Some([224, 232, 255, 255]);
    let inset = Some([32, 48, 86, 255]);

    let scene = ui.render_shared();
    let hard = capture(&mut renderer, &ui);
    assert_eq!(hard.pixel(90, 100), surface);
    assert_eq!(hard.pixel(175, 133), teal);
    assert_eq!(hard.pixel(185, 133), panel);
    assert_eq!(hard.pixel(267, 90), inset);
    assert_eq!(hard.pixel(320, 108), surface);
    assert_eq!(hard.pixel(263, 73), panel, "rounded inner clip");
    ui.set_inset_spread(Length(6.0));
    assert_eq!(capture(&mut renderer, &ui).pixel(278, 110), inset);
    ui.set_inset_spread(Length(-6.0));
    assert_eq!(capture(&mut renderer, &ui).pixel(272, 110), surface);
    ui.set_inset_spread(Length(0.0));
    let mut pointer = Interaction::default();
    pointer.pointer_move(&scene, &hard.layout, Point { x: 175.0, y: 133.0 });
    assert!(
        pointer.hovered.is_none(),
        "shadow pixels are outside the hit box"
    );
    pointer.pointer_move(&scene, &hard.layout, Point { x: 103.0, y: 108.0 });
    assert_eq!(pointer.hovered.as_ref().unwrap().id, Some("hit-card"));

    ui.set_shift(Length(48.0));
    let shifted = capture(&mut renderer, &ui);
    assert_eq!(shifted.pixel(205, 133), teal);
    assert_eq!(shifted.pixel(211, 133), window);
    assert_eq!(shifted.layout, hard.layout);
    assert!(
        scene
            .find("card")
            .unwrap()
            .same_frame_inputs(ui.render_shared().find("card").unwrap())
    );

    ui.set_shift(Length(16.0));
    ui.set_spread(Length(5.0));
    assert_eq!(capture(&mut renderer, &ui).pixel(183, 133), teal);
    ui.set_spread(Length(-6.0));
    assert_eq!(capture(&mut renderer, &ui).pixel(176, 133), panel);
    ui.set_spread(Length(0.0));

    ui.set_surface(Brush::Solid(Color(0xe0e8ff80)));
    let translucent = capture(&mut renderer, &ui);
    let translucent_shadow = translucent.pixel(175, 133).unwrap();
    for (channel, expected) in translucent_shadow.into_iter().zip([48u8, 134, 136, 255]) {
        assert!(channel.abs_diff(expected) <= 1, "{translucent_shadow:?}");
    }
    ui.set_surface(Brush::Linear {
        angle_degrees: 180.0,
        stops: [
            GradientStop {
                offset: 0.0,
                color: Color(0xe0e8ff00),
            },
            GradientStop {
                offset: 1.0,
                color: Color(0xe0e8ffff),
            },
        ]
        .into(),
    });
    let gradient = capture(&mut renderer, &ui);
    let top = gradient.pixel(175, 103).unwrap();
    let bottom = gradient.pixel(175, 151).unwrap();
    assert!(
        bottom[1] > top[1] + 50,
        "source alpha gradient: {top:?}, {bottom:?}"
    );
    ui.set_surface(Brush::default());
    assert_eq!(capture(&mut renderer, &ui).pixel(175, 133), panel);
    ui.set_outline(Length(4.0));
    let border_only = capture(&mut renderer, &ui);
    assert_eq!(border_only.pixel(178, 133), teal);
    assert_eq!(border_only.pixel(171, 133), panel);
    ui.set_outline(Length(0.0));
    ui.set_surface(Brush::Solid(Color(0xe0e8ffff)));

    let mut outside = ui.render();
    let offscreen = &mut outside.children[2].children[0];
    offscreen.properties.insert("x", Length(190.0).into());
    offscreen
        .properties
        .insert("drop-shadow-offset-x", Length(-40.0).into());
    let shadow_only = renderer.snapshot(&outside, 460, 268, 1.0, &input).unwrap();
    assert_eq!(shadow_only.pixel(200, 133), teal);
    assert_eq!(shadow_only.pixel(211, 133), window);

    ui.set_blur(Length(12.0));
    ui.set_inset_blur(Length(12.0));
    let blurred = capture(&mut renderer, &ui);
    let near = blurred.pixel(182, 123).unwrap();
    let far = blurred.pixel(193, 123).unwrap();
    assert!(
        near[1] > far[1] && far[1] >= 54 && near[1] < 214,
        "{near:?}, {far:?}"
    );
    let soft_inset = blurred.pixel(274, 100).unwrap();
    assert!(soft_inset[0] > 32 && soft_inset[0] < 224, "{soft_inset:?}");
    assert_eq!(blurred.pixel(263, 73), panel);

    ui.set_card_alpha(0.5);
    let faded = capture(&mut renderer, &ui);
    let halo = faded.pixel(175, 123).unwrap();
    let opaque_halo = blurred.pixel(175, 123).unwrap();
    assert!(halo[1] > 54 && halo[1] < opaque_halo[1]);
    let hidpi = renderer
        .snapshot(&ui.render_shared(), 920, 536, 2.0, &input)
        .unwrap();
    assert_eq!(hidpi.pixel(526, 146), panel);
    assert!(hidpi.pixel(350, 246).unwrap()[1] > 54);
}

#[test]
fn rectangle_clip_and_overflow_share_geometry_with_pointer_routing() {
    use layout_ui::ClippingDemo;
    use xrack_runtime::Length;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = ClippingDemo::new();
    let mut input = Interaction::default();
    let frame = |renderer: &mut Renderer, scene: &xrack_runtime::Node| {
        renderer
            .snapshot(scene, 468, 264, 1.0, &Interaction::default())
            .unwrap()
    };
    let bg = Some([16, 23, 37, 255]);
    let rim = Some([119, 141, 171, 255]);
    let teal = Some([57, 214, 188, 255]);
    let gold = Some([255, 209, 119, 255]);

    let scene = ui.render_shared();
    let clipped = frame(&mut renderer, &scene);
    assert_eq!(clipped.pixel(154, 109), teal);
    assert_eq!(clipped.pixel(176, 109), rim);
    assert_eq!(clipped.pixel(218, 109), bg);
    assert_eq!(clipped.pixel(60, 72), rim);
    assert_eq!(clipped.pixel(409, 109), Some([124, 147, 247, 255]));
    assert_eq!(clipped.pixel(425, 109), bg);
    let corner = Point { x: 60.0, y: 72.0 };
    input.pointer_move(&scene, &clipped.layout, corner);
    assert!(input.hovered.is_none());
    let outside = Point { x: 218.0, y: 109.0 };
    input.pointer_move(&scene, &clipped.layout, outside);
    assert!(input.hovered.is_none());

    ui.set_clipped(false);
    let scene = ui.render_shared();
    let open = frame(&mut renderer, &scene);
    assert_eq!(open.pixel(218, 109), teal);
    assert_eq!(open.pixel(60, 72), gold);
    input.reconcile(&scene, &open.layout);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("spill"));
    input.pointer_press_in_scene(&scene, &open.layout);
    let target = input.pointer_release_target().unwrap();
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(ui.get_hits(), 1);

    input.pointer_move(&scene, &open.layout, corner);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("corner"));

    ui.set_fade(0.5);
    let faded = frame(&mut renderer, &ui.render_shared());
    let pixel = faded.pixel(218, 109).unwrap();
    assert!(
        pixel
            .into_iter()
            .zip([37u8, 119, 113, 255])
            .all(|(a, b)| a.abs_diff(b) <= 1),
        "{pixel:?}"
    );

    // The parent itself is offscreen, but its overflow remains paintable and interactive.
    let mut offscreen = ui.render();
    let clipper = &mut offscreen.children[1];
    clipper.properties.insert("x", Length(-200.0).into());
    clipper.children[0]
        .properties
        .insert("x", Length(230.0).into());
    let overflow_only = frame(&mut renderer, &offscreen);
    assert_ne!(overflow_only.pixel(55, 109), bg);
    input.pointer_move(
        &offscreen,
        &overflow_only.layout,
        Point { x: 55.0, y: 109.0 },
    );
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("spill"));
}

#[test]
fn transformed_controls_keep_layout_geometry_and_share_exact_hit_coordinates() {
    use layout_ui::TransformDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = TransformDemo::new();
    let mut input = Interaction::default();
    let scene = ui.render_shared();
    let snapshot = renderer.snapshot(&scene, 460, 286, 1.0, &input).unwrap();
    let tile = snapshot.layout.at_path(&[1, 0]).unwrap();
    assert_eq!(
        tile.bounds,
        xrack_layout::Rect::new(110.0, 80.0, 88.0, 56.0)
    );
    let tile_bounds = tile.window_bounds(tile.bounds);
    assert_eq!(tile_bounds, xrack_layout::Rect::new(54.0, 80.0, 56.0, 88.0));
    let hot = snapshot.layout.at_path(&[1, 0, 0]).unwrap();
    let mapped = hot.map_to_window(Point { x: 139.0, y: 99.0 });
    assert_eq!(mapped, Point { x: 91.0, y: 109.0 });
    assert_eq!(snapshot.pixel(91, 109), Some([57, 214, 188, 255]));
    assert_eq!(snapshot.pixel(139, 99), Some([38, 54, 83, 255]));
    // The tiny red rectangle starts in a rounded corner of the tile.
    assert_eq!(snapshot.pixel(109, 81), Some([38, 54, 83, 255]));
    // The innermost rectangle uses a negative X scale.
    assert_eq!(snapshot.pixel(86, 152), Some([255, 209, 119, 255]));
    assert_ne!(snapshot.pixel(86, 140), Some([255, 209, 119, 255]));

    input.pointer_move(&scene, &snapshot.layout, mapped);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("hot"));
    input.pointer_press_in_scene(&scene, &snapshot.layout);
    let target = input.pointer_release_target().unwrap();
    assert!(ui.dispatch_at(&scene, &target.path, "clicked"));
    assert_eq!(ui.get_clicks(), 1);

    let ruler = snapshot.layout.at_path(&[1, 1]).unwrap();
    let on_slider = |fraction: f32| {
        ruler.map_to_window(Point {
            x: ruler.bounds.x + ruler.bounds.width * fraction,
            y: ruler.bounds.y + ruler.bounds.height * 0.5,
        })
    };
    input.pointer_move(&scene, &snapshot.layout, on_slider(0.5));
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("ruler"));
    input.pointer_press_in_scene(&scene, &snapshot.layout);
    let action = input.slider_press(&scene, &snapshot.layout).unwrap();
    assert!(ui.adjust_range_at(&scene, &action.path, action.action));
    assert!((ui.get_amount() - 0.5).abs() < 0.04);
    input.pointer_move(&scene, &snapshot.layout, on_slider(0.8));
    let action = input.slider_drag(&scene, &snapshot.layout).unwrap();
    assert!(ui.adjust_range_at(&scene, &action.path, action.action));
    assert!(ui.get_amount() > 0.78);

    ui.set_scale_factor(0.0);
    let collapsed = ui.render_shared();
    let snapshot = renderer
        .snapshot(&collapsed, 460, 286, 1.0, &Interaction::default())
        .unwrap();
    assert_eq!(
        snapshot.layout.at_path(&[1, 0]).unwrap().bounds,
        tile.bounds
    );
    assert_eq!(snapshot.pixel(91, 109), Some([38, 54, 83, 255]));
    input.reconcile(&collapsed, &snapshot.layout);
    input.pointer_move(&collapsed, &snapshot.layout, mapped);
    assert!(input.hovered.is_none());
}

#[test]
fn transformed_clip_pixels_and_input_match_between_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot = layout_ui::TransformDemo::new();
    let mut interpreted = interpret_example("examples/layout/layout.xr").create("TransformDemo")?;
    let mut clicks = 0;
    for (angle, zoom, dpi) in [(90.0, 1.0, 1), (37.0, 0.75, 2), (37.0, 0.0, 2)] {
        aot.set_angle(angle);
        aot.set_scale_factor(zoom);
        interpreted.set("angle", angle.into())?;
        interpreted.set("scale-factor", zoom.into())?;
        let aot_scene = aot.render_shared();
        let interpreter_scene = interpreted.render_shared();
        let input = Interaction::default();
        let compiled = renderer.snapshot(&aot_scene, 460 * dpi, 286 * dpi, dpi as f32, &input)?;
        let dynamic =
            renderer.snapshot(&interpreter_scene, 460 * dpi, 286 * dpi, dpi as f32, &input)?;
        assert_same_pixels(&compiled, &dynamic);

        let path = [1, 0, 0];
        let layout = compiled.layout.at_path(&path).ok_or("missing AOT button")?;
        let mirror = dynamic
            .layout
            .at_path(&path)
            .ok_or("missing interpreter button")?;
        assert_eq!(
            layout.visible_window_bounds(layout.bounds),
            mirror.visible_window_bounds(mirror.bounds)
        );
        assert_eq!(layout.visible, zoom > 0.0);
        let local = Point {
            x: layout.bounds.x + layout.bounds.width / 2.0,
            y: layout.bounds.y + layout.bounds.height / 2.0,
        };
        let window = layout.map_to_window(local);
        assert_eq!(window, mirror.map_to_window(local));
        if zoom == 0.0 {
            assert!(layout.map_from_window(window).is_none());
            assert!(mirror.map_from_window(window).is_none());
            continue;
        }
        for (component, scene, frame) in [
            (&mut aot as &mut dyn Component, &aot_scene, &compiled.layout),
            (
                &mut interpreted as &mut dyn Component,
                &interpreter_scene,
                &dynamic.layout,
            ),
        ] {
            let mut input = Interaction::default();
            input.pointer_move(scene, frame, window);
            input.pointer_press_in_scene(scene, frame);
            let target = input
                .pointer_release_target()
                .ok_or("transformed button missed input")?;
            assert_eq!(target.path, path);
            assert!(component.dispatch_at(scene, &target.path, "clicked"));
        }
        clicks += 1;
        assert_eq!(aot.get_clicks(), clicks);
        assert_eq!(
            interpreted.get("clicks")?,
            xrack_runtime::Value::Int(clicks)
        );
    }
    Ok(())
}

#[test]
fn programmatic_selection_and_cursor_use_independent_gpu_markers() {
    use xrack_runtime::{RowSelection, TableSelectionAction, Time};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..4)
            .map(|units| InventoryRow {
                name: format!("Device {units}"),
                units,
                available: true,
                refreshed: Time::default(),
            })
            .collect(),
    );
    ui.set_target(xrack_runtime::SelectionTarget::Cells);
    ui.set_selection_tools(true);
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::DragRange {
            anchor: (0, 0),
            end: (1, 1),
            additive: false,
        }
    ));
    assert!(ui.select_table(
        "inventory-table",
        TableSelectionAction::Cell {
            row: 2,
            column: 2,
            operation: RowSelection::Move,
        }
    ));
    let input = Interaction::default();
    let viewport = Size::new(620.0, 380.0);
    let (scene, _) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let table = scene.find("inventory-table").unwrap();
    assert_eq!(table.table_selection.as_ref().unwrap().len(), 4);
    assert!(table.children[0].children[0].children[0].boolean("table-selected", false));
    assert!(!table.children[2].children[0].children[2].boolean("table-selected", false));
    assert!(table.children[2].children[0].children[2].boolean("table-current", false));
    let frame = renderer.snapshot(&scene, 620, 380, 1.0, &input).unwrap();
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-selection-api.png"))
            .unwrap();
    }
}

#[test]
fn table_cell_positioning_draws_the_last_virtual_row_beside_a_frozen_column() {
    use xrack_layout::ViewportObservers;
    use xrack_runtime::{
        Component, Length, ScrollAlignment, ScrollRequests, SelectionTarget, Time,
    };
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..50_000)
            .map(|index| InventoryRow {
                name: format!("Device {index:05}"),
                units: index,
                available: true,
                refreshed: Time {
                    hour: 9,
                    ..Time::default()
                },
            })
            .collect(),
    );
    ui.set_position_tools(true);
    ui.set_target_row(49_999);
    ui.set_window_width(Length(500.0));
    ui.set_pinned(1);
    ui.set_row_header_size(Length(56.0));
    ui.set_target(SelectionTarget::Cells);
    let viewport = Size::new(500.0, 380.0);
    let mut input = Interaction::default();
    let requests = ScrollRequests::default();
    let _scroll = requests.enter();
    let (before, before_layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let before_frame = renderer.snapshot(&before, 500, 380, 1.0, &input).unwrap();
    // A request made before the next frame is resolved using that frame's geometry.
    ui.invoke_position_cell(49_999, 3, ScrollAlignment::End, ScrollAlignment::End);
    assert!(input.apply_scroll_requests(&before, &before_layout, requests.drain()));
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    ViewportObservers::default().sync(&scene, &layout);
    // Publish the measured offsets before rebuilding labels that bind to them.
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let table = scene.at_path(&[0, 1]).unwrap();
    let metrics = table.list.unwrap();
    assert!(metrics.first_row > 49_980 && table.children.len() < 12);
    assert_eq!(metrics.first_row + table.children.len(), 50_000);
    let geometry = layout.at_path(&[0, 1]).unwrap().table.as_ref().unwrap();
    assert_eq!(geometry.columns[0].x, geometry.body.x);
    let scrolling = geometry.scrollable_body();
    assert!(geometry.columns[3].x >= scrolling.x);
    assert!(
        (geometry.columns[3].x + geometry.columns[3].width - scrolling.x - scrolling.width).abs()
            < 0.01
    );
    assert_eq!(ui.get_current(), -1);
    assert_eq!(ui.get_selected_items(), 0);
    assert!(ui.get_table_y().0 < -1_599_000.0);
    let frame = renderer.snapshot(&scene, 500, 380, 1.0, &input).unwrap();
    let changed = (geometry.body.y as u32..(geometry.body.y + geometry.body.height) as u32)
        .flat_map(|y| {
            (geometry.body.x as u32..(geometry.body.x + geometry.body.width) as u32)
                .map(move |x| (x, y))
        })
        .filter(|&(x, y)| frame.pixel(x, y) != before_frame.pixel(x, y))
        .count();
    assert!(
        changed > 100,
        "the last virtual rows must reach the rendered image"
    );
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
            .save_png(std::path::Path::new(&path).join("table-position-cell.png"))
            .unwrap();
    }
}

#[test]
fn preview_routes_swipe_and_two_finger_transforms_without_an_extra_click() {
    use layout_ui::{ScaleRotateDemo, SwipeGestureDemo};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Length, PointerEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let swipe = SwipeGestureDemo::new();
    let mut preview = PreviewScene::new(Box::new(swipe.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let mouse = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    let start = Point { x: 120.0, y: 105.0 };
    let end = Point { x: 180.0, ..start };
    assert!(preview.press_contact(mouse, start, false));
    assert!(preview.move_contact(mouse, end));
    preview.render_animated(&mut renderer).unwrap();
    assert!(swipe.get_swiping());
    assert!(preview.finish_contact(mouse, end, false, false));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(swipe.get_shift(), Length(60.0));
    assert_eq!(swipe.get_swipes(), 1);
    assert!(!swipe.get_swiping());

    let mut pinch = ScaleRotateDemo::new();
    let mut preview = PreviewScene::new(Box::new(pinch.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let first = PointerContact {
        event: PointerEvent {
            is_touch: true,
            ..Default::default()
        },
        ..mouse
    };
    let second = PointerContact {
        id: 2,
        primary: false,
        ..first
    };
    let a = Point { x: 56.0, y: 52.0 };
    let b = Point { x: 116.0, y: 52.0 };
    let c = Point { x: 56.0, y: 142.0 };
    preview.press_contact(first, a, false);
    assert!(preview.press_contact(second, b, false));
    assert_eq!(pinch.get_starts(), 1);
    assert!(preview.move_contact(second, c));
    assert_eq!(pinch.get_zoom(), 1.5);
    assert_eq!(pinch.get_angle(), 90.0);
    assert!(preview.finish_contact(second, c, false, false));
    assert!(preview.finish_contact(first, a, true, false));
    assert_eq!(pinch.get_ends(), 1);
    assert_eq!(pinch.get_taps(), 0);
    assert!(!pinch.get_recognizing());

    preview.press_contact(first, a, false);
    assert!(preview.press_contact(second, b, false));
    pinch.set_gestures_enabled(false);
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(pinch.get_cancellations(), 1);
    assert!(!pinch.get_recognizing());
}

#[test]
fn preview_uses_shared_internal_and_file_drop_routing() {
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{DragAction, KeyModifiers, PointerEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let card = DragDropDemo::new();
    let mut preview = PreviewScene::new(Box::new(card.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent {
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let start = Point { x: 102.0, y: 117.0 };
    let target = Point { x: 420.0, y: 117.0 };
    preview.press_contact(contact, start, false);
    assert!(preview.move_contact(contact, target));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(card.get_trace(), "start-enter");
    assert!(preview.finish_contact(contact, target, false, false));
    assert_eq!(card.get_received(), "XR-42");
    assert_eq!(card.get_result(), DragAction::Copy);
    assert_eq!(card.get_trace(), "start-enter-drop-finish");

    let files = FileDropDemo::new();
    let mut preview = PreviewScene::new(Box::new(files.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    let point = Point { x: 120.0, y: 110.0 };
    let path = std::env::temp_dir()
        .join("photo.png")
        .to_string_lossy()
        .into_owned();
    assert!(preview.file_drag(1, point, vec![path.clone()], false));
    assert_eq!(files.get_preview(), std::slice::from_ref(&path));
    assert!(preview.file_drag(1, point, vec![path.clone()], true));
    assert_eq!(files.get_paths(), [path]);
    assert_eq!(files.get_batches(), 1);
    assert!(files.get_preview().is_empty());
}

#[test]
fn preview_table_range_drag_supports_copy_and_followup_keyboard_selection() {
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Component, KeyEvent, KeyModifiers, PointerEvent, SelectionTarget};

    let mut ui = TableDemo::new();
    ui.set_records(["first", "second", "third"].map(str::to_owned).to_vec());
    ui.set_target(SelectionTarget::Cells);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let (_, layout) = renderer.prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(560.0, 370.0),
        &Interaction::default(),
    );
    let table = layout.at_path(&[0, 2]).unwrap();
    let x = table.table.as_ref().unwrap().columns[0].x + 75.0;
    let start = Point {
        x,
        y: table.children[0].bounds.y + 16.0,
    };
    let end = Point {
        x,
        y: table.children[1].bounds.y + 16.0,
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 0);
    assert!(preview.finish_contact(contact, end, false, false));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 2);

    let control = KeyModifiers {
        control: true,
        ..Default::default()
    };
    let key = |name: &str, modifiers| KeyEvent {
        key: name.into(),
        modifiers,
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key("c", control), true));
    assert_eq!(
        preview.take_clipboard_request().unwrap().text.as_deref(),
        Some("first\nsecond")
    );
    assert!(preview.keyboard(&mut renderer, &key("a", control), true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 6);

    assert!(preview.keyboard(&mut renderer, &key("Escape", KeyModifiers::default()), true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 0);
    assert!(preview.keyboard(
        &mut renderer,
        &key("ArrowDown", KeyModifiers::default()),
        true
    ));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 1);
}

#[test]
fn preview_scrollbar_keeps_capture_until_release_and_uses_keyboard_focus() {
    use list_ui::StandaloneScrollDemo;
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, PointerEvent};

    let mut ui = StandaloneScrollDemo::new();
    ui.set_snap_on_release(true);
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    preview.render_animated(&mut renderer).unwrap();
    let start = Point { x: 295.0, y: 34.0 };
    let end = Point { x: 330.0, y: 97.0 };
    assert!(preview.press_contact(contact, start, false));
    assert_eq!(ui.get_position(), 0.0);
    assert!(preview.move_contact(contact, end));
    let drag_position = ui.get_position();
    assert!(drag_position > 0.3 && drag_position < 0.7);
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.finish_contact(contact, end, false, false));
    let snapped = ui.get_position() / 0.125;
    assert_eq!(snapped, snapped.round());
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.keyboard(
        &mut renderer,
        &KeyEvent {
            key: "End".into(),
            ..Default::default()
        },
        true
    ));
    assert_eq!(ui.get_position(), 0.75);
}

#[test]
fn preview_table_column_resize_updates_the_live_layout_after_pointer_capture() {
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Component, PointerEvent};

    let mut ui = TableDemo::new();
    ui.set_records((0..20).map(|row| format!("Row {row}")).collect());
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let viewport = Size::new(560.0, 370.0);
    let input = Interaction::default();
    let (_, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let table = layout.at_path(&[0, 2]).unwrap().table.as_ref().unwrap();
    let before = table.columns[0].width;
    let start = Point {
        x: table.columns[0].x + before - 1.0,
        y: table.header.y + 10.0,
    };
    let end = Point {
        x: start.x + 42.0,
        y: start.y,
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.finish_contact(contact, end, false, false));
    preview.render_animated(&mut renderer).unwrap();
    let (_, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let after = layout
        .at_path(&[0, 2])
        .unwrap()
        .table
        .as_ref()
        .unwrap()
        .columns[0]
        .width;
    assert_eq!(after, before + 42.0);
    assert_eq!(ui.get_selected_count(), 0);
}

#[test]
fn preview_table_header_reorder_keeps_unclaimed_clicks_and_alt_sort() {
    use std::{cell::RefCell, rc::Rc};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Component, KeyModifiers, PointerEvent, SelectionTarget};

    let mut ui = TableDemo::new();
    ui.set_records(vec!["first".into(), "second".into()]);
    ui.set_target(SelectionTarget::Columns);
    let sorts = Rc::new(RefCell::new(Vec::new()));
    ui.on_sort_requested({
        let sorts = sorts.clone();
        move |column, descending| sorts.borrow_mut().push((column, descending))
    });
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(560.0, 370.0);
    let input = Interaction::default();
    let path = [0, 2];
    let prepare = |renderer: &mut Renderer, ui: &TableDemo| {
        renderer.prepare_scene(ui, ui.render_shared(), size, &input)
    };
    let (_, layout) = prepare(&mut renderer, &ui);
    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let header_point =
        |geometry: &xrack_layout::TableGeometry, index: usize, fraction: f32| Point {
            x: geometry.columns[index].x + geometry.columns[index].width * fraction,
            y: geometry.header.y + geometry.header.height * 0.5,
        };
    let start = header_point(table, 0, 0.5);
    let end = header_point(table, 1, 0.85);
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.finish_contact(contact, end, false, false));
    preview.render_animated(&mut renderer).unwrap();

    let (scene, layout) = prepare(&mut renderer, &ui);
    assert_eq!(
        scene
            .at_path(&path)
            .unwrap()
            .table_column_order
            .visual_index(2, 0),
        Some(1)
    );
    assert!(sorts.borrow().is_empty());
    let table = layout.at_path(&path).unwrap().table.as_ref().unwrap();
    let sortable = header_point(table, 0, 0.5);
    assert!(preview.press_contact(contact, sortable, false));
    assert!(preview.finish_contact(contact, sortable, true, false));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_selected_count(), 1);
    assert!(sorts.borrow().is_empty());

    let alt = PointerContact {
        event: PointerEvent {
            modifiers: KeyModifiers {
                alt: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..contact
    };
    assert!(preview.press_contact(alt, sortable, false));
    assert!(preview.finish_contact(alt, sortable, true, false));
    assert_eq!(sorts.borrow().as_slice(), &[(0, false)]);
}

#[test]
fn preview_table_row_reorder_keeps_the_moved_key_and_cancels_without_reordering() {
    use table_ui::{InventoryRow, StandardTableDemo};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Component, KeyEvent, KeyModifiers, Length, PointerEvent};

    let mut ui = StandardTableDemo::new();
    ui.set_inventory(
        (0..8)
            .map(|row| InventoryRow {
                name: format!("Row {row}"),
                ..Default::default()
            })
            .collect(),
    );
    ui.set_row_header_size(Length(36.0));
    ui.set_move_rows(true);
    ui.set_current(0);
    let first = ui.model_inventory().row_key(0).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(620.0, 380.0);
    let input = Interaction::default();
    let path = [0, 1];
    let prepare = |renderer: &mut Renderer, ui: &StandardTableDemo| {
        renderer.prepare_scene(ui, ui.render_shared(), size, &input)
    };
    let row_point = |layout: &xrack_layout::LayoutNode, row: usize, fraction: f32| {
        let item = layout.at_path(&path).unwrap();
        let gutter = item.table.as_ref().unwrap().row_header;
        let row = item.children[row].bounds;
        Point {
            x: gutter.x + 16.0,
            y: row.y + row.height * fraction,
        }
    };

    let (_, layout) = prepare(&mut renderer, &ui);
    let start = row_point(&layout, 0, 0.5);
    let end = row_point(&layout, 2, 0.85);
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get_moved_row_to(), -1);
    assert!(preview.finish_contact(contact, end, false, false));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!((ui.get_moved_row_from(), ui.get_moved_row_to()), (0, 2));

    let (scene, layout) = prepare(&mut renderer, &ui);
    let row_keys = |scene: &xrack_runtime::Node| {
        scene
            .at_path(&path)
            .unwrap()
            .children
            .iter()
            .map(|row| row.row_key)
            .collect::<Vec<_>>()
    };
    let after = row_keys(&scene);
    assert_eq!(after[2], Some(first));
    let start = row_point(&layout, 0, 0.5);
    let end = row_point(&layout, 3, 0.85);
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    assert!(preview.finish_contact(contact, end, false, true));
    preview.render_animated(&mut renderer).unwrap();
    let (scene, _) = prepare(&mut renderer, &ui);
    assert_eq!(row_keys(&scene), after);

    let key = KeyEvent {
        key: "ArrowUp".into(),
        modifiers: KeyModifiers {
            alt: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key, true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!((ui.get_moved_row_from(), ui.get_moved_row_to()), (2, 1));
    let (scene, _) = prepare(&mut renderer, &ui);
    assert_eq!(row_keys(&scene)[1], Some(first));
}

#[test]
fn preview_composition_paints_without_writing_and_commits_once_to_the_original_editor() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::Component;

    let mut ui = BidiEditorDemo::new();
    ui.set_line("original".into());
    ui.set_document("other editor".into());
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let (_, layout) = renderer.prepare_scene(
        &ui,
        ui.render_shared(),
        Size::new(520.0, 300.0),
        &Interaction::default(),
    );
    let editor_point = |index| {
        let bounds = layout.at_path(&[0, index]).unwrap().bounds;
        Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        }
    };
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(editor_point(1));
    preview.render_animated(&mut renderer).unwrap();
    preview.edit(EditAction::SelectAll);
    let selection = preview.render_animated(&mut renderer).unwrap().to_vec();
    let original_caret = preview.ime_area().unwrap();

    assert!(preview.begin_text_composition(&mut renderer, 1));
    assert!(preview.update_text_composition(1, "ni".into()));
    let preedit = preview.render_animated(&mut renderer).unwrap().to_vec();
    assert_ne!(selection, preedit);
    let composition_caret = preview.ime_area().unwrap();
    assert_eq!(original_caret.y, composition_caret.y);
    assert_ne!(original_caret.x, composition_caret.x);
    assert_eq!(ui.get_line(), "original");
    assert!(preview.update_text_composition(1, "你好".into()));
    assert!(preview.finish_text_composition(1, "你好"));
    assert!(!preview.finish_text_composition(1, "你好"));
    assert_eq!(ui.get_line(), "你好");
    preview.render_animated(&mut renderer).unwrap();
    preview.replay_edit(false);
    assert_eq!(ui.get_line(), "original");
    preview.render_animated(&mut renderer).unwrap();

    assert!(preview.begin_text_composition(&mut renderer, 2));
    assert!(preview.update_text_composition(2, "discard".into()));
    assert!(preview.cancel_text_composition(2));
    assert!(!preview.finish_text_composition(2, "discard"));
    assert_eq!(ui.get_line(), "original");

    assert!(preview.begin_text_composition(&mut renderer, 3));
    assert!(preview.update_text_composition(3, "stale".into()));
    preview.click(editor_point(3));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.ime_area().unwrap().y > original_caret.y);
    assert!(preview.begin_text_composition(&mut renderer, 4));
    assert!(!preview.update_text_composition(3, "stale".into()));
    assert!(!preview.finish_text_composition(3, "stale"));
    preview.edit(EditAction::SelectAll);
    assert!(!preview.finish_text_composition(4, "stale"));
    assert!(preview.begin_text_composition(&mut renderer, 5));
    assert!(preview.finish_text_composition(5, "新内容"));
    assert_eq!(ui.get_line(), "original");
    assert_eq!(ui.get_document(), "新内容");

    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.begin_text_composition(&mut renderer, 6));
    assert!(preview.update_text_composition(6, "stale".into()));
    ui.set_document("updated by the component".into());
    preview.render_animated(&mut renderer).unwrap();
    assert!(!preview.finish_text_composition(6, "stale"));
    assert_eq!(ui.get_document(), "updated by the component");
}

#[test]
fn preview_places_the_text_caret_and_preserves_drag_and_shift_anchors() {
    use xrack_layout::{Interaction, TextEdit, TextPosition};
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{Component, KeyEvent, KeyModifiers, PointerEvent};

    let mut ui = BidiEditorDemo::new();
    ui.set_line("abcdef".into());
    let document = "alpha\nβeta\ngamma";
    ui.set_document(document.into());
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(520.0, 300.0);
    let input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), size, &input);
    let line_path = [0, 1];
    let line = layout.at_path(&line_path).unwrap().bounds;
    let entry = scene.at_path(&line_path).unwrap();
    let value = ui.get_line();
    let line_point = |offset| {
        let mut editor = TextEdit::at_end(&value);
        editor.place_caret(
            &value,
            TextPosition {
                offset,
                trailing: false,
            },
            false,
        );
        Point {
            x: renderer
                .text
                .single_line_caret_x(entry, line, &value, Some(&editor)),
            y: line.y + line.height * 0.5,
        }
    };
    let first = line_point(0);
    let start = line_point(1);
    let end = line_point(4);
    let extend = line_point(5);
    let paragraph_path = [0, 3];
    let paragraph = layout.at_path(&paragraph_path).unwrap().bounds;
    let node = scene.at_path(&paragraph_path).unwrap();
    let geometry = renderer.text.text_area_layout(node, paragraph, document);
    let paragraph_point = |offset| {
        let caret = geometry.caret_point(
            document,
            TextPosition {
                offset,
                trailing: false,
            },
            |text| renderer.text.advance_node(node, text),
        );
        Point {
            x: geometry.area.x + caret.x,
            y: geometry.area.y + caret.y + geometry.line_height * 0.5,
        }
    };
    let multiline_start = paragraph_point(1);
    let multiline_end = paragraph_point("alpha\nβ".len());
    let copy = KeyEvent {
        key: "c".into(),
        modifiers: KeyModifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(first);
    assert!(preview.keyboard(
        &mut renderer,
        &KeyEvent {
            key: "X".into(),
            text: "X".into(),
            ..Default::default()
        },
        true
    ));
    assert_eq!(ui.get_line(), "Xabcdef");
    ui.set_line("abcdef".into());
    preview.render_animated(&mut renderer).unwrap();

    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent::default(),
    };
    assert!(preview.press_contact(contact, start, false));
    assert!(preview.move_contact(contact, end));
    assert!(preview.finish_contact(contact, end, false, false));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.keyboard(&mut renderer, &copy, true));
    assert_eq!(
        preview.take_clipboard_request().unwrap().text.as_deref(),
        Some("bcd")
    );

    let shift = PointerContact {
        event: PointerEvent {
            modifiers: KeyModifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..contact
    };
    assert!(preview.press_contact(shift, extend, false));
    assert!(preview.finish_contact(shift, extend, true, false));
    assert!(preview.keyboard(&mut renderer, &copy, true));
    assert_eq!(
        preview.take_clipboard_request().unwrap().text.as_deref(),
        Some("bcde")
    );

    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.press_contact(contact, multiline_start, false));
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.move_contact(contact, multiline_end));
    assert!(preview.finish_contact(contact, multiline_end, false, false));
    assert!(preview.keyboard(&mut renderer, &copy, true));
    assert_eq!(
        preview.take_clipboard_request().unwrap().text.as_deref(),
        Some("lpha\nβ")
    );
}

#[test]
fn preview_multiline_editor_uses_shaped_vertical_navigation_and_wheel_scroll() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::{Component, KeyEvent, KeyModifiers, Length, WheelEvent};

    let mut ui = BidiEditorDemo::new();
    ui.set_document("same\nsame\nlast".into());
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let path = [0, 3];
    let point_at = |renderer: &mut Renderer, ui: &BidiEditorDemo, offset| {
        let input = Interaction::default();
        let (scene, layout) =
            renderer.prepare_scene(ui, ui.render_shared(), Size::new(520.0, 300.0), &input);
        let bounds = layout.at_path(&path).unwrap().bounds;
        let node = scene.at_path(&path).unwrap();
        let value = node.text("text").unwrap();
        let geometry = renderer.text.text_area_layout(node, bounds, value);
        let caret = geometry.caret_point(
            value,
            TextPosition {
                offset,
                trailing: false,
            },
            |text| renderer.text.advance_node(node, text),
        );
        Point {
            x: geometry.area.x + caret.x,
            y: geometry.area.y + caret.y + geometry.line_height * 0.5,
        }
    };
    let point = point_at(&mut renderer, &ui, 1);
    let mut preview = PreviewScene::new(Box::new(ui.as_weak().upgrade().unwrap()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(point);
    preview.render_animated(&mut renderer).unwrap();
    assert!(preview.control_navigation_enabled());
    let down = KeyEvent {
        key: "ArrowDown".into(),
        modifiers: KeyModifiers {
            shift: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let copy = KeyEvent {
        key: "c".into(),
        modifiers: KeyModifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &down, true));
    assert!(preview.keyboard(&mut renderer, &copy, true));
    assert_eq!(
        preview.take_clipboard_request().unwrap().text.as_deref(),
        Some("ame\ns")
    );

    let long = (0..40)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    ui.set_document(long.clone());
    preview.render_animated(&mut renderer).unwrap();
    let top = point_at(&mut renderer, &ui, 0);
    preview.wheel_event_with_renderer(
        &mut renderer,
        top,
        WheelEvent {
            delta_y: Length(180.0),
            ..Default::default()
        },
    );
    preview.render_animated(&mut renderer).unwrap();
    preview.click(top);
    assert!(preview.keyboard(
        &mut renderer,
        &KeyEvent {
            key: "X".into(),
            text: "X".into(),
            ..Default::default()
        },
        true
    ));
    let edited = ui.get_document();
    let insertion = edited.find('X').expect("text inserted");
    assert!(insertion > 0 && insertion < long.len());
    assert_eq!(edited.replacen('X', "", 1), long);
}

#[test]
fn media_texture_updates_pixels_without_affecting_layout() {
    use xrack_runtime::{MediaFrame, MediaSlot, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut root = Node::new("Window", None);
    let mut media = Node::new("MediaPlayer", Some("video"));
    let slot = std::rc::Rc::new(MediaSlot::default());
    slot.populate(&mut media, "video.xr");
    let frame = |pixel: [u8; 4]| MediaFrame::new([4, 4], pixel.repeat(16)).unwrap();
    media.media_frame = Some(frame([255, 0, 0, 255]));
    root.children.push(media);
    let before = root.clone();
    let input = Interaction::default();
    let first = renderer.snapshot(&before, 320, 180, 1.0, &input).unwrap();
    root.children[0].media_frame = Some(frame([0, 255, 0, 255]));
    assert!(before.same_frame_inputs(&root));
    assert!(!before.children[0].same_paint_except_opacity(&root.children[0]));
    let next = renderer.snapshot(&root, 320, 180, 1.0, &input).unwrap();
    assert_eq!(first.pixel(160, 90), Some([255, 0, 0, 255]));
    assert_eq!(next.pixel(160, 90), Some([0, 255, 0, 255]));
}

#[test]
fn inherited_text_alignment_keeps_drawn_text_links_and_editor_hits_in_sync() {
    use xrack_layout::{Interaction, Point, Size};
    use xrack_renderer_femtovg::PointerCaret;
    use xrack_runtime::Component;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();

    fn exercise(renderer: &mut Renderer, ui: &mut impl Component) -> [Vec<u8>; 2] {
        let size = Size::new(500.0, 300.0);
        let mut images = [Vec::new(), Vec::new()];
        for (iteration, rtl) in [true, false].into_iter().enumerate() {
            let input = Interaction::default();
            let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
            let slot = |id| {
                scene
                    .children
                    .iter()
                    .position(|node| node.element_id() == Some(id))
                    .unwrap()
            };
            let details = layout.at_path(&[slot("details")]).unwrap();
            let (_, link) = details.text_link_regions().next().unwrap();
            assert_eq!(link.x > details.bounds.x + details.bounds.width * 0.5, rtl);

            for id in ["line-edit", "notes-edit"] {
                let path = [slot(id)];
                let item = layout.at_path(&path).unwrap();
                let mut input = Interaction::default();
                let target = input.accessibility_target(&scene, &layout, &path).unwrap();
                input.accessibility_focus(target.clone());
                input.sync_editor(&scene);
                let point = Point {
                    x: item.bounds.x + 80.0,
                    y: item.bounds.y + 10.0,
                };
                let pointer =
                    PointerCaret::new(&input, &scene, &layout, &target, point, point, false)
                        .unwrap();
                assert!(pointer.place(&renderer.text, &mut input, &scene, &layout));
                let value = scene.at_path(&path).unwrap().text("text").unwrap();
                assert_eq!(
                    input.editor.as_ref().unwrap().caret(value),
                    if rtl { 0 } else { 3 }
                );
                let ime = renderer
                    .text
                    .editor_ime_area(&mut input, &scene, &layout, true)
                    .unwrap();
                assert_eq!(ime.x > item.bounds.x + item.bounds.width * 0.5, rtl);

                if id == "line-edit" {
                    let before = renderer.snapshot(&scene, 500, 300, 1.0, &input).unwrap();
                    input.apply_text_selection(&scene, xrack_runtime::TextCommand::SelectAll);
                    let selected = renderer.snapshot(&scene, 500, 300, 1.0, &input).unwrap();
                    let mut changed = 0;
                    for y in (item.bounds.y as u32 + 6)
                        ..((item.bounds.y + item.bounds.height) as u32 - 6)
                    {
                        for x in (item.bounds.x as u32 + 6)
                            ..((item.bounds.x + item.bounds.width) as u32 - 6)
                        {
                            if before.pixel(x, y) != selected.pixel(x, y) {
                                assert_eq!(x as f32 > item.bounds.x + item.bounds.width * 0.5, rtl);
                                changed += 1;
                            }
                        }
                    }
                    assert!(changed > 0);
                }
            }

            images[iteration] = renderer
                .snapshot(&scene, 500, 300, 1.0, &input)
                .unwrap()
                .rgba;
            if rtl {
                assert!(ui.dispatch("toggle-direction", "clicked"));
            }
        }
        assert_ne!(images[0], images[1]);
        images
    }

    let mut native = form_ui::TextAlignmentDemo::new();
    let mut interpreted = interpret_example("examples/form/form.xr")
        .create("TextAlignmentDemo")
        .unwrap();
    let expected = exercise(&mut renderer, &mut native);
    assert_eq!(exercise(&mut renderer, &mut interpreted), expected);
    assert_eq!(interpreted.take_error(), None);
}

#[test]
fn inherited_rtl_mirrors_slider_fill_and_preserves_interval_fill() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = controls_ui::DirectionalSlidersDemo::new();
    let source = include_str!("../examples/controls/controls.xr");
    let compiled =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let interpreter = xrack_interpreter::Program::from_module(&compiled.module)
        .unwrap()
        .create("DirectionalSlidersDemo")
        .unwrap();
    let input = Interaction::default();
    let fill = Some([0x53, 0x6d, 0xfe, 0xff]);
    let rail = Some([0x35, 0x41, 0x5b, 0xff]);

    for rtl in [true, false] {
        native.set_rtl(rtl);
        interpreter.set("rtl", rtl.into()).unwrap();
        let native = renderer
            .snapshot(&native.render(), 400, 260, 1.0, &input)
            .unwrap();
        let interpreter = renderer
            .snapshot(&interpreter.render(), 400, 260, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreter, &native);
        let sample = |path: &[usize], fraction: f64| {
            let bounds = native.layout.at_path(path).unwrap().bounds;
            let physical = SliderGeometry::new(bounds, Orientation::Horizontal);
            let point = physical.position(fraction);
            native.pixel(point.x as u32, point.y as u32)
        };
        assert_eq!(sample(&[1, 0], 0.08), if rtl { rail } else { fill });
        assert_eq!(sample(&[1, 0], 0.92), if rtl { fill } else { rail });
        assert_eq!(sample(&[1, 1], 0.5), fill);
        assert_eq!(sample(&[1, 1], 0.08), rail);
        assert_eq!(sample(&[1, 1], 0.92), rail);
    }
}

#[test]
fn rtl_tab_headers_paint_on_the_selected_logical_side_in_both_backends() {
    use xrack_runtime::Orientation;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = navigation_ui::DirectionalTabsDemo::new();
    let filename = "examples/navigation/navigation.xr";
    let source = include_str!("../examples/navigation/navigation.xr");
    let preview = xrack_compiler::prepare_preview(source, filename, Default::default()).unwrap();
    let interpreted = xrack_interpreter::Program::from_module(&preview.module)
        .unwrap()
        .create("DirectionalTabsDemo")
        .unwrap();
    let input = Interaction::default();
    for (page, vertical) in [(0_i64, false), (5, true)] {
        native.set_page(page);
        native.set_vertical(vertical);
        interpreted.set("page", page.into()).unwrap();
        interpreted.set("vertical", vertical.into()).unwrap();
        let actual = renderer
            .snapshot(&interpreted.render(), 624, 264, 1.0, &input)
            .unwrap();
        let expected = renderer
            .snapshot(&native.render(), 624, 264, 1.0, &input)
            .unwrap();
        assert_same_pixels(&actual, &expected);
        let bar = actual.layout.child_at(2).unwrap();
        let scroll = bar.scroll_on(Orientation::Horizontal).unwrap();
        let header = bar.child_at(page as usize).unwrap().bounds;
        assert_eq!(header.intersect(bar.bounds), header);
        let scrolled = if page == 0 { scroll.max_offset } else { 0.0 };
        assert_eq!(scroll.offset, scrolled);
        let widget = actual.layout.child_at(4).unwrap();
        let selected = widget.child_at(0).unwrap().tab_header.unwrap();
        assert!((selected.x + selected.width - widget.bounds.x - widget.bounds.width).abs() < 0.01);
        assert_ne!(
            actual.pixel(selected.x as u32 + 5, selected.y as u32 + 5),
            actual.pixel(
                widget.bounds.x as u32 + 5,
                (widget.bounds.y + widget.bounds.height - 5.0) as u32
            ),
        );
        assert!(renderer.set_system_differentiate_without_color(true));
        let marked = renderer
            .snapshot(&native.render(), 624, 264, 1.0, &input)
            .unwrap();
        assert_eq!(expected.layout, marked.layout);
        assert_same_pixels(
            &marked,
            &renderer
                .snapshot(&interpreted.render(), 624, 264, 1.0, &input)
                .unwrap(),
        );
        for rect in [header, selected] {
            let x = (rect.x + rect.width * 0.5) as u32;
            let y = (rect.y + rect.height - 4.0) as u32;
            assert_ne!(actual.pixel(x, y), marked.pixel(x, y));
        }
        assert!(renderer.set_system_differentiate_without_color(false));
    }
}

#[test]
fn rtl_feedback_mirrors_default_and_custom_indicators_in_both_backends() {
    use xrack_layout::{PageIndicatorGeometry, ToggleGeometry};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = controls_ui::DirectionalFeedbackDemo::new();
    let source = include_str!("../examples/controls/controls.xr");
    let compiled =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let interpreter = xrack_interpreter::Program::from_module(&compiled.module)
        .unwrap()
        .create("DirectionalFeedbackDemo")
        .unwrap();
    let input = Interaction::default();

    let mut knobs = Vec::new();
    for rtl in [true, false] {
        native.set_rtl(rtl);
        interpreter.set("rtl", rtl.into()).unwrap();
        let scene = native.render();
        let native = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
        let interpreted = renderer
            .snapshot(&interpreter.render(), 464, 310, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreted, &native);
        let sample = |x: f32, y: f32| native.pixel(x as u32, y as u32);
        let toggle = |path: &[usize]| {
            let item = native.layout.at_path(path).unwrap();
            ToggleGeometry::new(
                scene.at_path(path).unwrap(),
                item.bounds,
                item.layout_direction,
            )
        };

        let check = toggle(&[1, 0]);
        assert_eq!(check.indicator.x > check.label.x, rtl);
        assert_eq!(
            sample(check.indicator.x + 9.0, check.indicator.y + 9.0),
            Some([0x38, 0x47, 0x61, 0xff])
        );

        let wifi = toggle(&[1, 1]);
        let bounds = wifi.indicator;
        assert_eq!(bounds.x > wifi.label.x, rtl);
        let (left, right) = (
            sample(bounds.x + 9.0, bounds.y + 9.0),
            sample(bounds.x + bounds.width - 9.0, bounds.y + 9.0),
        );
        assert_ne!(left, right);
        knobs.push((left, right));

        let share = toggle(&[1, 2]);
        let part = native.layout.at_path(&[1, 2, 0]).unwrap().bounds;
        assert_eq!(part, share.indicator);
        assert_eq!(share.label.x > part.x, rtl);
        assert_eq!(
            sample(part.x + 13.0, part.y + 10.0),
            Some([0x78, 0x50, 0x75, 0xff])
        );

        let path = [1, 4];
        let indicator = native.layout.at_path(&path).unwrap();
        let markers: Vec<_> = PageIndicatorGeometry::with_layout_direction(
            scene.at_path(&path).unwrap(),
            indicator.bounds,
            indicator.layout_direction,
        )
        .unwrap()
        .markers()
        .collect();
        assert_eq!(markers[0].center.x > markers[3].center.x, rtl);
        assert_eq!(
            sample(markers[0].center.x, markers[0].center.y),
            Some([0xf6, 0xad, 0x55, 0xff])
        );

        let meter = native.layout.at_path(&[1, 5]).unwrap().bounds;
        let foreground = Some([0x67, 0xd4, 0xcf, 0xff]);
        let track = Some([0x35, 0x41, 0x5b, 0xff]);
        assert_eq!(
            sample(meter.x + meter.width * 0.12, meter.y + 6.0),
            if rtl { track } else { foreground },
        );
        assert_eq!(
            sample(meter.x + meter.width * 0.88, meter.y + 6.0),
            if rtl { foreground } else { track },
        );
    }
    assert_eq!(knobs[0], (knobs[1].1, knobs[1].0));
}

#[test]
fn system_switch_labels_follow_state_and_direction_in_both_backends() {
    use controls_ui::DirectionalFeedbackDemo;
    use xrack_layout::ToggleGeometry;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = DirectionalFeedbackDemo::new();
    let interpreter = interpret_example("examples/controls/controls.xr")
        .create("DirectionalFeedbackDemo")
        .unwrap();
    let input = Interaction::default();
    let path = [1, 1];
    for (rtl, checked, enabled) in [
        (false, true, true),
        (false, false, true),
        (true, true, false),
        (true, false, true),
    ] {
        ui.set_rtl(rtl);
        ui.set_wireless(checked);
        ui.set_wireless_enabled(enabled);
        interpreter.set("rtl", rtl.into()).unwrap();
        interpreter.set("wireless", checked.into()).unwrap();
        interpreter.set("wireless-enabled", enabled.into()).unwrap();
        let scene = ui.render_shared();
        let before = renderer.snapshot(&scene, 928, 620, 2.0, &input).unwrap();
        let item = before.layout.at_path(&path).unwrap();
        let track = ToggleGeometry::new(
            scene.at_path(&path).unwrap(),
            item.bounds,
            item.layout_direction,
        )
        .indicator;
        let marker_x = track.x + if rtl == checked { 27.0 } else { 9.0 };
        let marker_y = track.y + 9.0;
        let x = (marker_x * 2.0) as u32;
        let y = (marker_y * 2.0) as u32;
        let thumb_x = ((2.0 * track.x + track.width - marker_x) * 2.0) as u32;

        assert!(renderer.set_system_switch_labels(true));
        let labeled = renderer.snapshot(&scene, 928, 620, 2.0, &input).unwrap();
        assert_eq!(
            before.layout.at_path(&path).unwrap().bounds,
            labeled.layout.at_path(&path).unwrap().bounds
        );
        assert_eq!(before.pixel(thumb_x, y), labeled.pixel(thumb_x, y));
        if checked {
            assert_ne!(before.pixel(x, y), labeled.pixel(x, y));
            assert_eq!(before.pixel(x + 7, y), labeled.pixel(x + 7, y));
        } else {
            assert_eq!(before.pixel(x, y), labeled.pixel(x, y));
            assert_ne!(before.pixel(x + 7, y), labeled.pixel(x + 7, y));
        }
        // The second switch in the demo supplies its own track and thumb.
        let custom = before.layout.at_path(&[1, 7]).unwrap().bounds;
        for y in (2.0 * custom.y) as u32..(2.0 * (custom.y + custom.height)) as u32 {
            for x in (2.0 * custom.x) as u32..(2.0 * (custom.x + custom.width)) as u32 {
                assert_eq!(before.pixel(x, y), labeled.pixel(x, y));
            }
        }
        let interpreted = renderer
            .snapshot(&interpreter.render(), 928, 620, 2.0, &input)
            .unwrap();
        assert_same_pixels(&labeled, &interpreted);
        assert!(std::rc::Rc::ptr_eq(&scene, &ui.render_shared()));
        assert_eq!(ui.get_wireless_changes(), 0);
        assert!(!renderer.set_system_switch_labels(true));
        assert!(renderer.set_system_switch_labels(false));
        let restored = renderer.snapshot(&scene, 928, 620, 2.0, &input).unwrap();
        assert_same_pixels(&restored, &before);
    }
}

#[test]
fn switch_thumb_follows_the_drag_before_the_bound_value_changes() {
    use std::time::{Duration, Instant};
    use xrack_layout::{ScrollGestureKind, ToggleGeometry};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let native = controls_ui::DirectionalFeedbackDemo::new();
    let source = include_str!("../examples/controls/controls.xr");
    let compiled =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let interpreted = xrack_interpreter::Program::from_module(&compiled.module)
        .unwrap()
        .create("DirectionalFeedbackDemo")
        .unwrap();
    let mut input = Interaction::default();
    let scene = native.render();
    let before = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    let path = [1, 1];
    let item = before.layout.at_path(&path).unwrap();
    let track = ToggleGeometry::new(
        scene.at_path(&path).unwrap(),
        item.bounds,
        item.layout_direction,
    )
    .indicator;
    let y = track.y + track.height * 0.5;
    let start = Point {
        x: track.x + 18.0,
        y,
    };
    let moved = Point {
        x: start.x + 13.5,
        y,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        &before.layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    assert!(input.move_scroll_gesture(
        &scene,
        &before.layout,
        moved,
        now + Duration::from_millis(30)
    ));

    let preview = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    let counterpart = renderer
        .snapshot(&interpreted.render(), 464, 310, 1.0, &input)
        .unwrap();
    assert_same_pixels(&counterpart, &preview);
    assert_eq!(
        preview.layout.at_path(&path).unwrap().switch_position,
        Some(0.25)
    );
    assert!(native.get_wireless());
    assert_eq!(native.get_wireless_changes(), 0);
    let initial = (track.x + 9.0) as u32;
    let shifted = (track.x + 22.5) as u32;
    assert_eq!(
        before.pixel(initial, y as u32),
        preview.pixel(shifted, y as u32)
    );
    assert_ne!(
        before.pixel(initial, y as u32),
        preview.pixel(initial, y as u32)
    );
    input.cancel_scroll_gesture();
    let restored = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    assert_same_pixels(&restored, &before);
}

#[test]
fn switch_thumb_paints_above_its_custom_track_in_both_backends() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let native = controls_ui::DirectionalFeedbackDemo::new();
    let source = include_str!("../examples/controls/controls.xr");
    let compiled =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let interpreted = xrack_interpreter::Program::from_module(&compiled.module)
        .unwrap()
        .create("DirectionalFeedbackDemo")
        .unwrap();
    let mut input = Interaction::default();
    let scene = native.render();
    let before = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    let thumb = [1, 7, 0];
    let bounds = before.layout.at_path(&thumb).unwrap().bounds;
    let start = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let end = Point {
        x: start.x - 24.0,
        ..start
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &scene,
        &before.layout,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    assert!(input.move_scroll_gesture(
        &scene,
        &before.layout,
        end,
        now + Duration::from_millis(30)
    ));
    let moved = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    let interpreted = renderer
        .snapshot(&interpreted.render(), 464, 310, 1.0, &input)
        .unwrap();
    assert_same_pixels(&interpreted, &moved);
    assert_eq!(
        moved.layout.at_path(&thumb).unwrap().bounds.x,
        bounds.x - 24.0
    );
    let thumb_fill = Some([0xf6, 0xad, 0x55, 0xff]);
    let track_fill = Some([0x41, 0x4f, 0x65, 0xff]);
    assert_eq!(before.pixel(start.x as u32, start.y as u32), thumb_fill);
    assert_eq!(moved.pixel(end.x as u32, end.y as u32), thumb_fill);
    assert_eq!(moved.pixel(start.x as u32, start.y as u32), track_fill);
    assert!(!native.get_custom_enabled());

    input.cancel_scroll_gesture();
    let restored = renderer.snapshot(&scene, 464, 310, 1.0, &input).unwrap();
    assert_same_pixels(&restored, &before);
}

#[test]
fn a_virtual_switch_thumb_drag_bypasses_cached_geometry_for_its_row() {
    use std::time::{Duration, Instant};
    use xrack_layout::ScrollGestureKind;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = ItemDelegatesDemo::new();
    let mut input = Interaction::default();
    let size = Size::new(644.0, 398.0);
    let root = ui.render_shared();
    let path = [0, 1, 1, 1, 0, 0];
    let thumb = [0, 1, 1, 1, 0, 0, 0];

    let (scene, before) = renderer.prepare_scene(&ui, root.clone(), size, &input);
    let bounds = before.at_path(&thumb).unwrap().bounds;
    let start = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    let checked = scene.at_path(&path).unwrap().boolean("checked", false);
    let dx = if checked { -12.0 } else { 12.0 };
    let end = Point {
        x: start.x + dx,
        ..start
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &before, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &before, end, now + Duration::from_millis(30)));
    let (_, preview) = renderer.prepare_scene(&ui, root.clone(), size, &input);
    assert!(preview.at_path(&path).unwrap().switch_position.is_some());
    let moved = preview.at_path(&thumb).unwrap().bounds;
    assert!((moved.x - bounds.x - dx).abs() < 0.01);

    input.cancel_scroll_gesture();
    let (_, restored) = renderer.prepare_scene(&ui, root, size, &input);
    assert_eq!(restored.at_path(&thumb).unwrap().bounds, bounds);
}

#[test]
fn rtl_combo_paints_inline_completions_on_each_drafts_visual_side() {
    use xrack_layout::TextEdit;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = selection_ui::DirectionalChoiceDemo::new();
    let interpreted = interpret_example("examples/selection/selection.xr")
        .create("DirectionalChoiceDemo")
        .unwrap();
    let size = Size::new(464.0, 284.0);
    let path = [1, 0];
    for (draft, suffix, key) in [
        ("Al", "pha", NavigationKey::Right),
        ("של", "ום", NavigationKey::Left),
    ] {
        native.set_draft(draft.into());
        interpreted.set("draft", draft.to_owned().into()).unwrap();
        let scene = native.render_shared();
        let mut input = Interaction::default();
        let layout = renderer.layout_with_input(&scene, size, &input);
        let target = input.accessibility_target(&scene, &layout, &path).unwrap();
        input.accessibility_focus(target);
        input.editor = Some(TextEdit::at_end(draft));
        let frame = renderer.snapshot(&scene, 464, 284, 1.0, &input).unwrap();
        let indicator = layout.at_path(&[1, 0, 0]).unwrap().bounds;
        assert_eq!(
            frame.pixel((indicator.x + 6.0) as u32, (indicator.y + 6.0) as u32),
            Some([0x43, 0x75, 0x99, 0xff])
        );
        let painted = renderer.take_painted_combo_completion().unwrap();
        assert_eq!(painted.suffix(), suffix);
        let interpreted = renderer
            .snapshot(&interpreted.render(), 464, 284, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreted, &frame);
        input.set_painted_combo_completion(Some(painted));
        assert!(input.combo_completion_accepts(key));
        let opposite = if key == NavigationKey::Left {
            NavigationKey::Right
        } else {
            NavigationKey::Left
        };
        assert!(!input.combo_completion_accepts(opposite));
        assert!(input.accept_combo_completion(&mut native, &scene, std::time::Instant::now()));
        assert_eq!(native.get_draft(), format!("{draft}{suffix}"));
    }
}

#[test]
fn rtl_menus_paint_mirrored_icons_and_cascades_in_both_backends() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(620.0, 366.0);
    let text = xrack_renderer_femtovg::TextSystem::system().unwrap();
    let mut native = MenuDemo::new();
    native.set_rtl(true);
    native.set_show_grid(true);
    let interpreted = interpret_example("examples/menu/menu.xr")
        .create("MenuDemo")
        .unwrap();
    interpreted.set("rtl", true.into()).unwrap();
    interpreted.set("show-grid", true.into()).unwrap();
    let mut aot = UiDriver::new(
        native,
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        size,
    );
    let mut interpreter = UiDriver::new(interpreted, text, size);
    aot.click("view-menu");
    interpreter.click("view-menu");
    let aot_frame = renderer
        .snapshot(aot.scene(), 620, 366, 1.0, aot.input())
        .unwrap();
    let interpreted_frame = renderer
        .snapshot(interpreter.scene(), 620, 366, 1.0, interpreter.input())
        .unwrap();
    assert_same_pixels(&interpreted_frame, &aot_frame);
    let panels = aot.menus();
    let panel = &panels[0];
    let icon = panel.icon_bounds(0);
    assert!(icon.x > panel.bounds.x + panel.bounds.width * 0.5);
    assert_eq!(panel.rows[0].checked, Some(true));
    assert!((icon.x as u32..(icon.x + icon.width) as u32).any(|x| {
        (icon.y as u32..(icon.y + icon.height) as u32)
            .any(|y| aot_frame.pixel(x, y) == Some([0xff, 0xb4, 0x54, 255]))
    }));

    aot.menu_key(MenuKey::Escape);
    interpreter.menu_key(MenuKey::Escape);
    aot.click("file-menu");
    interpreter.click("file-menu");
    for key in [MenuKey::Down, MenuKey::Left] {
        aot.menu_key(key);
        interpreter.menu_key(key);
    }
    let panels = aot.menus();
    assert_eq!(panels.len(), 2);
    assert_eq!(
        panels[1].bounds.x + panels[1].bounds.width,
        panels[0].bounds.x
    );
    let aot_frame = renderer
        .snapshot(aot.scene(), 620, 366, 1.0, aot.input())
        .unwrap();
    let interpreted_frame = renderer
        .snapshot(interpreter.scene(), 620, 366, 1.0, interpreter.input())
        .unwrap();
    assert_same_pixels(&interpreted_frame, &aot_frame);
}

#[test]
fn rtl_calendar_templates_paint_the_same_day_on_the_mirrored_cell() {
    use xrack_layout::CalendarGeometry;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = controls_ui::DirectionalCalendarDemo::new();
    let source = include_str!("../examples/controls/controls.xr");
    let preview =
        xrack_compiler::prepare_preview(source, "controls.xr", Default::default()).unwrap();
    let interpreted = xrack_interpreter::Program::from_module(&preview.module)
        .unwrap()
        .create("DirectionalCalendarDemo")
        .unwrap();
    let input = Interaction::default();
    let path = [1, 1, 1, 1];
    for rtl in [false, true] {
        native.set_rtl(rtl);
        interpreted.set("rtl", rtl.into()).unwrap();
        let scene = native.render();
        let frame = renderer.snapshot(&scene, 724, 362, 1.0, &input).unwrap();
        let interpreted = renderer
            .snapshot(&interpreted.render(), 724, 362, 1.0, &input)
            .unwrap();
        assert_same_pixels(&interpreted, &frame);
        let item = frame.layout.at_path(&path).unwrap();
        let geometry = CalendarGeometry::for_layout(scene.at_path(&path).unwrap(), item);
        let today = geometry.days().find(|day| day.selected).unwrap();
        assert_eq!(item.at_path(&[0, today.slot]).unwrap().bounds, today.bounds);
        assert_eq!(
            today.bounds.x > item.bounds.x + item.bounds.width * 0.5,
            rtl
        );
        assert_eq!(
            frame.pixel((today.bounds.x + 7.0) as u32, (today.bounds.y + 7.0) as u32),
            Some([0xd1, 0x7e, 0x42, 0xff]),
        );
    }
}

#[test]
fn rtl_custom_split_handles_keep_their_scene_index_and_theme() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = split_ui::CustomSplitDemo::new();
    native.set_rtl(true);
    let interpreter = interpret_example("examples/split/split.xr")
        .create("CustomSplitDemo")
        .unwrap();
    interpreter.set("rtl", true.into()).unwrap();
    let input = Interaction::default();
    let frames = [
        renderer
            .snapshot(&native.render(), 640, 240, 1.0, &input)
            .unwrap(),
        renderer
            .snapshot(&interpreter.render(), 640, 240, 1.0, &input)
            .unwrap(),
    ];
    assert_same_pixels(&frames[0], &frames[1]);
    let split = frames[0]
        .layout
        .child_at(0)
        .unwrap()
        .split
        .as_ref()
        .unwrap();
    let first = split.handles[0].bounds;
    let second = split.handles[1].bounds;
    assert!(first.x > second.x);
    let pixel = |bounds: xrack_layout::Rect| {
        frames[0].pixel((bounds.x + 2.0) as u32, (bounds.y + 12.0) as u32)
    };
    assert_eq!(pixel(first), Some([0x69, 0x4b, 0x8e, 0xff]));
    assert_eq!(pixel(second), Some([0x48, 0x5e, 0x7e, 0xff]));
}

#[test]
fn animated_button_press_paints_in_the_absence_of_pointer_input() {
    use controls_ui::ProgrammaticButtonsDemo;
    use xrack_runtime::Component;

    let mut native = ProgrammaticButtonsDemo::new();
    let compiled = xrack_compiler::prepare_preview(
        include_str!("../examples/controls/controls.xr"),
        "controls.xr",
        Default::default(),
    )
    .unwrap();
    let interpreted = xrack_interpreter::Program::from_module(&compiled.module)
        .unwrap()
        .create("ProgrammaticButtonsDemo")
        .unwrap();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    fn render(
        renderer: &mut Renderer,
        ui: &impl Component,
    ) -> (xrack_renderer_femtovg::Snapshot, (u32, u32)) {
        let input = Interaction::default();
        let (scene, layout) =
            renderer.prepare_scene(ui, ui.render_shared(), Size::new(466.0, 261.0), &input);
        let content = scene
            .children
            .iter()
            .position(|child| child.kind == "VerticalLayout")
            .unwrap();
        let target = [content, 0, 0];
        assert_eq!(scene.at_path(&target).unwrap().element_id(), Some("target"));
        let bounds = layout.at_path(&target).unwrap().bounds;
        let probe = ((bounds.x + 9.0) as u32, (bounds.y + 9.0) as u32);
        (
            renderer
                .snapshot_prepared(&scene, layout, 466, 261, 1.0, &input)
                .unwrap(),
            probe,
        )
    }
    let (idle, point) = render(&mut renderer, &native);
    native.invoke_run_animated();
    interpreted.invoke("run-animated", &[]).unwrap();
    let (held, _) = render(&mut renderer, &native);
    let (other, _) = render(&mut renderer, &interpreted);
    assert_same_pixels(&held, &other);
    assert_ne!(idle.pixel(point.0, point.1), held.pixel(point.0, point.1));
}

#[test]
fn button_appearance_uses_accent_neutral_and_flat_surfaces_in_both_backends() {
    use menu_ui::ButtonAppearanceDemo;
    use xrack_runtime::Component;

    let mut native = ButtonAppearanceDemo::new();
    let mut interpreted = interpret_example("examples/menu/menu.xr")
        .create("ButtonAppearanceDemo")
        .unwrap();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let size = Size::new(440.0, 250.0);
    let mut input = Interaction::default();
    fn render(
        renderer: &mut Renderer,
        ui: &impl Component,
        input: &Interaction,
    ) -> xrack_renderer_femtovg::Snapshot {
        let size = Size::new(440.0, 250.0);
        let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, input);
        renderer
            .snapshot_prepared(&scene, layout, 440, 250, 1.0, input)
            .unwrap()
    }
    let idle = render(&mut renderer, &native, &input);
    assert_same_pixels(&idle, &render(&mut renderer, &interpreted, &input));
    assert_eq!(idle.pixel(28, 30), Some([0xf2, 0xa3, 0x54, 255]));
    assert_eq!(idle.pixel(165, 30), Some([0x38, 0x49, 0x66, 255]));
    let canvas = Some([0x18, 0x23, 0x31, 255]);
    assert_eq!(idle.pixel(302, 30), canvas);

    let before = native.render_shared();
    let interpreted_before = interpreted.render_shared();
    native.set_emphasize(false);
    interpreted.set("emphasize", false.into()).unwrap();
    let neutral = render(&mut renderer, &native, &input);
    assert_same_pixels(&neutral, &render(&mut renderer, &interpreted, &input));
    assert_eq!(neutral.pixel(28, 30), Some([0x38, 0x49, 0x66, 255]));
    native.set_flat_save(true);
    interpreted.set("flat-save", true.into()).unwrap();
    assert!(
        before
            .find("save")
            .unwrap()
            .same_frame_inputs(native.render_shared().find("save").unwrap())
    );
    assert!(
        interpreted_before
            .find("save")
            .unwrap()
            .same_frame_inputs(interpreted.render_shared().find("save").unwrap())
    );
    let cleared = render(&mut renderer, &native, &input);
    assert_same_pixels(&cleared, &render(&mut renderer, &interpreted, &input));
    assert_eq!(cleared.pixel(28, 30), canvas);

    let (scene, layout) = renderer.prepare_scene(&native, native.render_shared(), size, &input);
    input.pointer_move(&scene, &layout, Point { x: 332.0, y: 44.0 });
    let hover = render(&mut renderer, &native, &input);
    assert_ne!(hover.pixel(302, 30), canvas);
    input.pointer_press_in_scene(&scene, &layout);
    let held = render(&mut renderer, &native, &input);
    assert_ne!(held.pixel(302, 30), hover.pixel(302, 30));
    let target = input.pointer_release_target().unwrap();
    assert!(native.dispatch_at(&scene, &target.path, "clicked"));
    let interpreted_scene = interpreted.render_shared();
    assert!(interpreted.dispatch_at(&interpreted_scene, &target.path, "clicked"));
    assert!(native.get_pinned());

    input.pointer_move(&scene, &layout, Point { x: 0.0, y: 0.0 });
    native.set_save_enabled(false);
    interpreted.set("save-enabled", false.into()).unwrap();
    let checked = render(&mut renderer, &native, &input);
    assert_same_pixels(&checked, &render(&mut renderer, &interpreted, &input));
    assert_eq!(checked.pixel(28, 30), canvas);
    assert_ne!(checked.pixel(302, 30), canvas);
}

#[test]
fn button_display_paints_stacked_tint_and_mirrors_beside_mode_in_both_backends() {
    use menu_ui::ButtonDisplayDemo;
    use xrack_runtime::Component;

    let mut native = ButtonDisplayDemo::new();
    let interpreted = interpret_example("examples/menu/menu.xr")
        .create("ButtonDisplayDemo")
        .unwrap();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let size = Size::new(475.0, 300.0);
    for rtl in [false, true] {
        native.set_rtl(rtl);
        interpreted.set("rtl", rtl.into()).unwrap();
        let (scene, layout) = renderer.prepare_scene(&native, native.render_shared(), size, &input);
        let bounds = |slot| layout.at_path(&[0, 0, slot]).unwrap().bounds;
        let (beside, stacked, text) = (bounds(0), bounds(1), bounds(3));
        let frame = renderer
            .snapshot_prepared(&scene, layout, 475, 300, 1.0, &input)
            .unwrap();
        let (scene, layout) =
            renderer.prepare_scene(&interpreted, interpreted.render_shared(), size, &input);
        let other = renderer
            .snapshot_prepared(&scene, layout, 475, 300, 1.0, &input)
            .unwrap();
        assert_same_pixels(&frame, &other);
        let count = |x: std::ops::Range<u32>, y: std::ops::Range<u32>, color| {
            x.flat_map(|x| y.clone().map(move |y| (x, y)))
                .filter(|&(x, y)| frame.pixel(x, y) == Some(color))
                .count()
        };
        let center = (beside.x + beside.width * 0.5) as u32;
        let vertical = beside.y.ceil() as u32..(beside.y + beside.height).floor() as u32;
        let gold = [0xff, 0xb4, 0x54, 255];
        let leading = if rtl {
            center..(beside.x + beside.width).floor() as u32
        } else {
            beside.x.ceil() as u32..center
        };
        let trailing = if rtl {
            beside.x.ceil() as u32..center
        } else {
            center..(beside.x + beside.width).floor() as u32
        };
        assert!(count(leading, vertical.clone(), gold) > 0);
        assert_eq!(count(trailing, vertical, gold), 0);
        assert!(
            count(
                stacked.x.ceil() as u32..(stacked.x + stacked.width).floor() as u32,
                stacked.y.ceil() as u32..(stacked.y + stacked.height * 0.5).floor() as u32,
                [0xe7, 0xa6, 0x46, 255]
            ) > 0
        );
        assert_eq!(
            count(
                text.x.ceil() as u32..(text.x + text.width).floor() as u32,
                text.y.ceil() as u32..(text.y + text.height).floor() as u32,
                gold
            ),
            0
        );
    }
}

#[test]
fn radio_group_title_paints_at_the_logical_start_edge() {
    use xrack_runtime::{LayoutDirection, Length, Node};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let mut scene = Node::new("Window", None);
    let mut group = Node::new("RadioGroup", None);
    for (name, length) in [
        ("x", 10.0),
        ("y", 8.0),
        ("width", 200.0),
        ("height", 50.0),
        ("padding-left", 6.0),
        ("padding-right", 14.0),
    ] {
        group.properties.insert(name, Length(length).into());
    }
    group.properties.insert("title", "Title".to_owned().into());
    scene.children.push(group);
    for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
        scene.children[0]
            .properties
            .insert("layout-direction", direction.into());
        let frame = renderer.snapshot(&scene, 220, 70, 1.0, &input).unwrap();
        let background = frame.pixel(5, 5);
        let ink = |xs: std::ops::Range<u32>| {
            xs.flat_map(|x| (8..32).map(move |y| (x, y)))
                .filter(|&(x, y)| frame.pixel(x, y) != background)
                .count()
        };
        let (leading, trailing) = if direction == LayoutDirection::LeftToRight {
            (ink(10..105), ink(115..210))
        } else {
            (ink(115..210), ink(10..105))
        };
        assert!(leading > 0);
        assert_eq!(trailing, 0);
    }
}

#[test]
fn rtl_tree_paints_the_collapsed_arrow_toward_its_children() {
    fn snapshot(renderer: &mut Renderer, ui: &impl Component) -> xrack_renderer_femtovg::Snapshot {
        let input = Interaction::default();
        let size = Size::new(380.0, 220.0);
        let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        let row = [1, 0, 0];
        let item = layout.at_path(&row).unwrap();
        let geometry =
            xrack_layout::TreeRowGeometry::for_layout(scene.at_path(&row).unwrap(), item).unwrap();
        assert_eq!(
            geometry.indicator.x + geometry.indicator.width,
            item.bounds.x + item.bounds.width - 4.0
        );
        let center = (
            (geometry.indicator.x + geometry.indicator.width * 0.5) as u32,
            (geometry.indicator.y + geometry.indicator.height * 0.5) as u32,
        );
        let frame = renderer
            .snapshot_prepared(&scene, layout, 380, 220, 1.0, &input)
            .unwrap();
        // Above the tip, the filled side of the left-facing triangle is on the right.
        let tint = Some([0xc0, 0xd1, 0xea, 255]);
        assert_eq!(frame.pixel(center.0 + 1, center.1 - 3), tint);
        assert_ne!(frame.pixel(center.0 - 2, center.1 - 3), tint);
        frame
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let native = tree_ui::DirectionalTreeDemo::new();
    let interpreted = interpret_example("examples/tree/tree.xr")
        .create("DirectionalTreeDemo")
        .unwrap();
    let native = snapshot(&mut renderer, &native);
    let interpreted = snapshot(&mut renderer, &interpreted);
    assert_same_pixels(&interpreted, &native);
}

#[test]
fn preview_renames_the_selected_rtl_tree_record_with_f2() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = interpret_example("examples/tree/tree.xr")
        .create("DirectionalTreeDemo")
        .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.clone()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(Point { x: 208.0, y: 67.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("current"), Ok(Value::Int(1)));
    for (key, text) in [("F2", ""), ("s", "s"), ("Enter", "")] {
        assert!(preview.keyboard(
            &mut renderer,
            &KeyEvent {
                key: key.into(),
                text: text.into(),
                ..Default::default()
            },
            true
        ));
        preview.render_animated(&mut renderer).unwrap();
    }
    let Value::RecordList(model) = ui.get("branches").unwrap() else {
        panic!("expected tree model");
    };
    let record = model.model().row(0).unwrap();
    assert_eq!(record.field("label"), Some(&Value::String("s".into())));
}

#[cfg(feature = "media_backend")]
#[test]
fn preview_plays_nonvisual_sound_effect_from_click_and_shortcut() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    xrack_media::install();
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = interpret_example("examples/sound_effect/sound_effect.xr")
        .create("SoundDemo")
        .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.clone()));
    preview.render_animated(&mut renderer).unwrap();
    let (scene, layout, _) = preview.inspect_frame().unwrap();
    assert!(scene.find("confirmation").is_none());
    let column = scene
        .children
        .iter()
        .position(|child| child.kind == "VerticalLayout")
        .unwrap();
    let path = [column, 0];
    assert_eq!(scene.at_path(&path).unwrap().element_id(), Some("confirm"));
    let bounds = layout.at_path(&path).unwrap().bounds;
    let point = Point {
        x: bounds.x + bounds.width / 2.0,
        y: bounds.y + bounds.height / 2.0,
    };

    let wait = |preview: &mut PreviewScene, renderer: &mut Renderer, playing| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            preview.advance(renderer);
            assert_eq!(ui.get("sound-error"), Ok(Value::String(String::new())));
            if ui.get("sound-ready") == Ok(Value::Bool(true))
                && ui.get("sound-playing") == Ok(Value::Bool(playing))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected sound-playing={playing}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    };
    wait(&mut preview, &mut renderer, false);
    preview.click(point);
    wait(&mut preview, &mut renderer, true);
    wait(&mut preview, &mut renderer, false);
    assert!(preview.keyboard(
        &mut renderer,
        &KeyEvent {
            key: "F2".into(),
            ..Default::default()
        },
        true
    ));
    wait(&mut preview, &mut renderer, true);
}

#[test]
fn tree_disclosure_replaces_the_native_triangle_and_repaints_on_expansion() {
    fn snapshot(
        renderer: &mut Renderer,
        ui: &impl Component,
        expected: [u8; 4],
    ) -> (std::rc::Rc<Node>, xrack_renderer_femtovg::Snapshot) {
        let input = Interaction::default();
        let size = Size::new(360.0, 214.0);
        let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        let bounds = layout.at_path(&[2, 0, 0, 0, 0]).unwrap().bounds;
        let frame = renderer
            .snapshot_prepared(&scene, layout, 360, 214, 1.0, &input)
            .unwrap();
        assert_eq!(
            frame.pixel((bounds.x + 4.0) as u32, (bounds.y + 6.0) as u32),
            Some(expected)
        );
        (scene, frame)
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = tree_ui::TreeDisclosureDemo::new();
    let mut interpreted = interpret_example("examples/tree/tree.xr")
        .create("TreeDisclosureDemo")
        .unwrap();
    let (native_scene, native_frame) = snapshot(&mut renderer, &native, [0xb1, 0x5d, 0x1e, 255]);
    let (interpreted_scene, interpreted_frame) =
        snapshot(&mut renderer, &interpreted, [0xb1, 0x5d, 0x1e, 255]);
    assert_same_pixels(&interpreted_frame, &native_frame);

    let path = [2, 0];
    let action = |scene: &Node| {
        let tree = scene.at_path(&path).unwrap().tree.as_ref().unwrap();
        let row = tree.rows()[0];
        ControlAction::Tree(TreeAction::SetExpanded {
            key: row.key,
            row: row.row,
            expanded: true,
        })
    };
    assert!(native.control_at(&native_scene, &path, action(&native_scene)));
    assert!(interpreted.control_at(&interpreted_scene, &path, action(&interpreted_scene)));
    let (_, native) = snapshot(&mut renderer, &native, [0, 0x7d, 0x66, 255]);
    let (_, interpreted) = snapshot(&mut renderer, &interpreted, [0, 0x7d, 0x66, 255]);
    assert_same_pixels(&interpreted, &native);
}

#[test]
fn preview_grid_keeps_modifier_selection_separate_from_item_activation() {
    use xrack_renderer_femtovg::preview::{PointerContact, PreviewScene};
    use xrack_runtime::{KeyEvent, KeyModifiers, PointerButton, PointerEvent};

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = interpret_example("examples/list/list.xr")
        .create("MultiSelectDemo")
        .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.clone()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(Point { x: 280.0, y: 92.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("tile-key"), Ok(Value::Int(10)));
    assert_eq!(ui.get("opened"), Ok(Value::Int(1)));
    let second = Point { x: 426.0, y: 92.0 };
    let contact = PointerContact {
        id: 0,
        primary: true,
        event: PointerEvent {
            button: PointerButton::Primary,
            modifiers: KeyModifiers {
                control: true,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    preview.press_contact(contact, second, false);
    preview.finish_contact(contact, second, true, false);
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("tile-key"), Ok(Value::Int(30)));
    assert_eq!(ui.get("grid-selection"), Ok(Value::Int(2)));
    assert_eq!(ui.get("opened"), Ok(Value::Int(1)));

    let key = KeyEvent {
        key: "ArrowDown".into(),
        modifiers: KeyModifiers {
            shift: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key, true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("tile-key"), Ok(Value::Int(50)));
    assert_eq!(ui.get("grid-selection"), Ok(Value::Int(3)));
    assert_eq!(ui.get("opened"), Ok(Value::Int(1)));
    preview.click(Point { x: 270.0, y: 51.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("actual-grid-selection"), Ok(Value::Int(6)));
    assert_eq!(ui.get("actual-list-selection"), Ok(Value::Int(6)));
    preview.click(Point { x: 404.0, y: 51.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("actual-grid-selection"), Ok(Value::Int(0)));
    assert_eq!(ui.get("actual-list-selection"), Ok(Value::Int(0)));
    assert_eq!(ui.get("tile-key"), Ok(Value::Int(50)));
}

#[test]
fn preview_list_typeahead_accepts_committed_text_and_cancels_an_old_focus_session() {
    use xrack_renderer_femtovg::preview::PreviewScene;
    use xrack_runtime::KeyEvent;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let ui = interpret_example("examples/list/list.xr")
        .create("TypeaheadDemo")
        .unwrap();
    let mut preview = PreviewScene::new(Box::new(ui.clone()));
    preview.render_animated(&mut renderer).unwrap();
    preview.click(Point { x: 100.0, y: 92.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(0)));
    assert!(preview.ime_area().is_some());
    let mut key = KeyEvent {
        key: "a".into(),
        text: "a".into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key, true));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(1)));

    key.modifiers.control = true;
    preview.keyboard(&mut renderer, &key, true);
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(1)));
    assert!(preview.begin_text_composition(&mut renderer, 17));
    assert!(preview.update_text_composition(17, "部".into()));
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(1)));
    assert!(preview.finish_text_composition(17, "部署"));
    preview.render_animated(&mut renderer).unwrap();
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(4)));

    assert!(preview.begin_text_composition(&mut renderer, 18));
    preview.click(Point { x: 240.0, y: 92.0 });
    preview.render_animated(&mut renderer).unwrap();
    assert!(!preview.finish_text_composition(18, "文档"));
    assert_eq!(ui.get("selected-row"), Ok(Value::Int(4)));
    let key = KeyEvent {
        key: "b".into(),
        text: "b".into(),
        ..Default::default()
    };
    assert!(preview.keyboard(&mut renderer, &key, true));
    assert_eq!(ui.get("selected-key"), Ok(Value::Int(40)));
}

#[test]
fn sectioned_list_paints_a_pinned_heading_above_virtual_rows_in_both_backends() {
    fn snapshot(renderer: &mut Renderer, ui: &impl Component) -> xrack_renderer_femtovg::Snapshot {
        let size = Size::new(530.0, 246.0);
        let mut input = Interaction::default();
        let (scene, frame) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        input.pointer_move(&scene, &frame, Point { x: 350.0, y: 104.0 });
        assert!(input.scroll_y(&frame, 50.0));
        input.pointer_leave();
        let (scene, frame) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        let header = &frame.at_path(&[3]).unwrap().list_section_headers()[0];
        assert!(header.pinned);
        assert_eq!(header.bounds.y, 62.0);
        let capture = renderer
            .snapshot_prepared(&scene, frame, 530, 246, 1.0, &input)
            .unwrap();
        assert_eq!(capture.pixel(492, 68), Some([0x81, 0x53, 0x29, 255]));
        assert_eq!(capture.pixel(225, 68), Some([0x25, 0x33, 0x4b, 255]));
        assert_eq!(capture.pixel(492, 94), Some([0x29, 0x43, 0x64, 255]));
        capture
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let native = list_ui::SectionedListDemo::new();
    let interpreted = interpret_example("examples/list/list.xr")
        .create("SectionedListDemo")
        .unwrap();
    let native = snapshot(&mut renderer, &native);
    let interpreted = snapshot(&mut renderer, &interpreted);
    assert_same_pixels(&interpreted, &native);
}

#[test]
fn rtl_scroll_canvas_starts_at_its_right_edge_and_grid_paints_on_the_same_side() {
    fn snapshot(
        renderer: &mut Renderer,
        ui: &impl Component,
        rtl: bool,
    ) -> xrack_renderer_femtovg::Snapshot {
        let input = Interaction::default();
        let size = Size::new(510.0, 238.0);
        let (scene, layout) = renderer.prepare_scene(ui, ui.render_shared(), size, &input);
        let canvas = layout.at_path(&[2]).unwrap();
        let bounds = canvas.bounds;
        assert_eq!(canvas.children[1].bounds.x == bounds.x, !rtl);
        assert_eq!(
            canvas.children[2].bounds.x + canvas.children[2].bounds.width
                == bounds.x + bounds.width,
            rtl
        );
        let grid = layout.at_path(&[4]).unwrap();
        assert_eq!(grid.children[0].bounds.x > grid.children[1].bounds.x, rtl);
        let frame = renderer
            .snapshot_prepared(&scene, layout, 510, 238, 1.0, &input)
            .unwrap();
        let (x, color) = if rtl {
            (230, [0x81, 0x53, 0x29, 255])
        } else {
            (75, [0x38, 0x66, 0x97, 255])
        };
        assert_eq!(frame.pixel(x, 137), Some(color));
        frame
    }

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut native = list_ui::DirectionalScrollDemo::new();
    let interpreted = interpret_example("examples/list/list.xr")
        .create("DirectionalScrollDemo")
        .unwrap();
    for rtl in [true, false] {
        native.set_rtl(rtl);
        interpreted.set("rtl", Value::Bool(rtl)).unwrap();
        let native = snapshot(&mut renderer, &native, rtl);
        let interpreted = snapshot(&mut renderer, &interpreted, rtl);
        assert_same_pixels(&interpreted, &native);
    }
}

#[test]
fn rtl_table_keeps_frozen_and_virtualized_cells_on_their_rendered_columns() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let native = table_ui::DirectionalTableDemo::new();
    let interpreted = interpret_example("examples/table/table.xr")
        .create("DirectionalTableDemo")
        .unwrap();
    let size = Size::new(520.0, 300.0);
    let mut input = Interaction::default();
    let path = [2, 0];
    let (scene, layout) = renderer.prepare_scene(&native, native.render_shared(), size, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    let frozen = table.columns[0];
    let point = Point {
        x: table.body.x + 24.0,
        y: table.body.y + 20.0,
    };
    input.pointer_move(&scene, &layout, point);
    assert!(input.scroll_x(&layout, -60.0));
    let (scene, layout) = renderer.prepare_scene(&native, native.render_shared(), size, &input);
    let item = layout.at_path(&path).unwrap();
    let table = item.table.as_ref().unwrap();
    assert_eq!(table.columns[0], frozen);
    assert!(table.row_header.x > table.body.x);
    assert_eq!(table.horizontal.offset, 60.0);
    assert_eq!(item.at_path(&[0, 0, 0]).unwrap().bounds.x, frozen.x);
    let aot = renderer
        .snapshot_prepared(&scene, layout, 520, 300, 1.0, &input)
        .unwrap();
    let (scene, layout) =
        renderer.prepare_scene(&interpreted, interpreted.render_shared(), size, &input);
    let interpreted = renderer
        .snapshot_prepared(&scene, layout, 520, 300, 1.0, &input)
        .unwrap();
    assert_same_pixels(&interpreted, &aot);
}

#[test]
fn custom_scroll_artwork_moves_with_the_thumb_and_hides_with_the_viewport() {
    use list_ui::ScrollVisualMetricsDemo;

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let program = interpret_example("examples/list/list.xr");
    let inputs = Interaction::default();
    for (rtl, visible, left, right) in [
        (
            false,
            true,
            [0x59, 0xbf, 0x94, 0xff],
            [0x26, 0x37, 0x50, 0xff],
        ),
        (
            true,
            true,
            [0x26, 0x37, 0x50, 0xff],
            [0x59, 0xbf, 0x94, 0xff],
        ),
        (
            true,
            false,
            [0xf0, 0xf3, 0xfa, 0xff],
            [0xf0, 0xf3, 0xfa, 0xff],
        ),
    ] {
        let mut aot = ScrollVisualMetricsDemo::new();
        aot.set_rtl(rtl);
        aot.set_show_canvas(visible);
        let interpreted = program.create("ScrollVisualMetricsDemo").unwrap();
        interpreted.set("rtl", Value::Bool(rtl)).unwrap();
        interpreted
            .set("show-canvas", Value::Bool(visible))
            .unwrap();
        for scene in [aot.render_shared(), interpreted.render_shared()] {
            let image = renderer.snapshot(&scene, 332, 180, 1.0, &inputs).unwrap();
            assert_eq!(image.pixel(60, 88), Some(left));
            assert_eq!(image.pixel(165, 88), Some(right));
            let indicator_x = if rtl { 140 } else { 70 };
            assert_eq!(
                image.pixel(indicator_x, 104),
                Some([0xe5, 0x85, 0x4f, 0xff])
            );
            assert_eq!(image.pixel(20, 104), Some([0xf0, 0xf3, 0xfa, 0xff]));
        }
    }
}

#[test]
fn custom_scroll_thumb_repaints_while_captured() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = UiDriver::new(
        list_ui::ScrollVisualMetricsDemo::new(),
        xrack_renderer_femtovg::TextSystem::system().unwrap(),
        Size::new(332.0, 180.0),
    );
    let thumb_pixel = |renderer: &mut Renderer, ui: &UiDriver<_, _>| {
        let geometry = ui.scroll_bar("bar").unwrap();
        let image = renderer
            .snapshot(ui.scene(), 332, 180, 1.0, ui.input())
            .unwrap();
        image.pixel(geometry.thumb.x as u32 + 10, 88)
    };
    assert_eq!(
        thumb_pixel(&mut renderer, &ui),
        Some([0x59, 0xbf, 0x94, 0xff])
    );
    assert!(ui.drag_scroll_bar_with("bar", 0.55, true, |ui| {
        assert_eq!(
            thumb_pixel(&mut renderer, ui),
            Some([0x72, 0xa9, 0xee, 0xff])
        );
        true
    }));
    ui.update(|_| {});
    assert_eq!(
        thumb_pixel(&mut renderer, &ui),
        Some([0x59, 0xbf, 0x94, 0xff])
    );
}

#[test]
fn reserved_scroll_view_gutters_clip_contents_and_mirror_in_rtl() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut generated = list_ui::ScrollGuttersDemo::new();
    let interpreted = interpret_example("examples/list/list.xr")
        .create("ScrollGuttersDemo")
        .unwrap();
    for rtl in [false, true] {
        generated.set_rtl(rtl);
        interpreted.set("rtl", Value::Bool(rtl)).unwrap();
        let mut images = Vec::new();
        for scene in [generated.render_shared(), interpreted.render_shared()] {
            let image = renderer
                .snapshot(&scene, 370, 200, 1.0, &Interaction::default())
                .unwrap();
            let (lane, interior) = if rtl { (25, 194) } else { (194, 25) };
            assert_eq!(image.pixel(interior, 56), Some([0x26, 0x37, 0x50, 0xff]));
            assert_ne!(image.pixel(lane, 56), image.pixel(interior, 56));
            assert_eq!(image.pixel(lane, 123), Some([0xdd, 0xe8, 0xf1, 0xff]));
            images.push(image);
        }
        assert_eq!(images[0].pixel(100, 56), images[1].pixel(100, 56));
    }
}
