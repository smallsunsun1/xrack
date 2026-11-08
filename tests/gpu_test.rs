use async_ui::AsyncDemo;
pub mod ui_driver;
use controls_ui::{
    CalendarDemo, CheckStateDemo, ControlsDemo, DecimalSpinDemo, DelayDemo, GroupBoxDemo,
    IndicatorsDemo, TimeDemo, TimerDemo, TumblerDemo,
};
use counter_ui::Counter;
use delegates_ui::{DelegatesDemo, SwipeDemo};
use dialog_ui::{DialogDemo, PopupDemo, StandardDialogDemo};
use form_ui::{FormDemo, PasswordDemo};
use layout_ui::{FlexDemo, FlowDemo, LabelsDemo, LayoutDemo};
use list_ui::{DirectoryItem, GridDemo, ListDemo, StandardListDemo};
use media_ui::MediaDemo;
use menu_ui::MenuDemo;
use navigation_ui::{NavigationDemo, PageDemo, StackDemo, TabBarDemo};
use notes_ui::NotesDemo;
use painting_ui::Gallery;
use records_ui::RecordTableDemo;
use selection_ui::{EditableChoiceDemo, SelectionDemo};
use split_ui::SplitDemo;
use table_ui::{InventoryRow, StandardTableDemo, TableDemo};
use tooltip_ui::TooltipDemo;
use tree_ui::TreeDemo;
use ui_driver::UiDriver;
use xrack_layout::{
    ChoiceKey, DialGeometry, EditAction, EditSource, Interaction, MenuKey, NavigationKey, Point,
    RangeInput, Size, SliderGeometry, SliderKey, SpinGeometry, TextAreaMotion, TextEdit,
    TextPosition, font_size,
};

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
    let hidden = renderer.snapshot(&scene, 392, 214, 1.0, &input).unwrap();
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
        .snapshot(&ui.render(), 392, 214, 1.0, &input)
        .unwrap();
    assert_eq!(hidden.rgba, same_mask.rgba);

    let composing = |renderer: &mut Renderer, value: &str, preedit: &str| {
        let mut input = Interaction::default();
        input.focus_next(&scene, &hidden.layout, false);
        let mut edit = TextEdit::at_end(value);
        edit.set_preedit(preedit.into(), Some((preedit.len(), preedit.len())));
        input.editor = Some(edit);
        renderer.snapshot(&scene, 392, 214, 1.0, &input).unwrap()
    };
    assert_eq!(
        composing(&mut renderer, secret, "か").rgba,
        composing(&mut renderer, secret, "🙂").rgba
    );

    ui.set_secret(secret.into());
    ui.set_reveal(true);
    let shown = renderer
        .snapshot(&ui.render(), 392, 214, 1.0, &input)
        .unwrap();
    assert!((area.y as u32..(area.y + area.height) as u32).any(|y| {
        (area.x as u32..(area.x + area.width) as u32)
            .any(|x| shown.pixel(x, y) != hidden.pixel(x, y))
    }));
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
    assert_eq!(panel.rows[0].icon, ui.node("grid-button").image("icon"));
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
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        frame
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
    let prepare = |renderer: &mut Renderer| {
        prepared.get(|| Rc::new(renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input)))
    };
    let first = prepare(&mut renderer);
    let model = ui.model_inventory();
    for (index, label) in [(49_999, "Offscreen"), (0, "Visible change")] {
        let mut row = model.row(index).unwrap();
        row.name = label.to_owned();
        assert!(model.set_row(index, row));
        assert_eq!(Rc::ptr_eq(&first, &prepare(&mut renderer)), index != 0);
    }
    let after = prepare(&mut renderer);
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
    assert_eq!(input.scroll_bar_press(&scene, &layout), Some(false));
    input.pointer_move(&scene, &layout, Point { x: 800.0, y: -40.0 });
    assert_eq!(input.scroll_bar_drag(&scene, &layout), Some(true));
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
    assert_eq!(input.scroll_bar_press(&scene, &layout), Some(false));
    input.pointer_move(&scene, &layout, Point { x: 20.0, y: 560.0 });
    assert_eq!(input.scroll_bar_drag(&scene, &layout), Some(true));
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
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut ui = RecordTableDemo::new();
    let model = ui.model_records();
    let viewport = Size::new(650.0, 330.0);
    let mut input = Interaction::default();
    let (scene, layout) = renderer.prepare_scene(&ui, ui.render_shared(), viewport, &input);
    let owner = [0, 1, 0, 0, 1];
    let cell = layout.at_path(&owner).unwrap().bounds;
    input.pointer_move(
        &scene,
        &layout,
        Point {
            x: cell.x + 16.0,
            y: cell.y + 14.0,
        },
    );
    input.pointer_press();
    input.pointer_release_target().unwrap();
    assert!(input.begin_table_edit(&scene, &layout));
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
    assert_ne!(frame.pixel(630, 376), before.pixel(630, 376));
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
        assert_eq!(input.scroll_bar_press(&scene, &layout), Some(false));
        input.pointer_move(
            &scene,
            &layout,
            Point {
                x: bar.track.x + bar.track.width + 30.0,
                y: bar.track.y + bar.track.height + 30.0,
            },
        );
        assert_eq!(input.scroll_bar_drag(&scene, &layout), Some(true));
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
fn media_renders_embedded_images_and_vector_paths() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .expect("GPU test requires an adapter and installed fonts");
    let mut demo = MediaDemo::new();
    let scene = demo.render();
    let input = Interaction::default();
    let snapshot = renderer.snapshot(&scene, 660, 376, 1.0, &input).unwrap();
    let card = [0x1d, 0x29, 0x3d, 255];

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
    let cropped = renderer
        .snapshot(&demo.render(), 660, 376, 1.0, &input)
        .unwrap();
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
    assert_eq!(hidpi.layout.bounds, snapshot.layout.bounds);
    assert_eq!(hidpi.pixel(660, 372), Some([0xff, 0xe5, 0x96, 255]));
    assert_eq!(hidpi.pixel(1074, 376), Some(card));
    if let Ok(path) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
        snapshot
            .save_png(std::path::Path::new(&path).join("media-demo.png"))
            .unwrap();
    }
}
