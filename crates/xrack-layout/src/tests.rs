use super::*;
use xrack_runtime::{CrossAxisAlignment, LayoutAlignment, Length, Node, Value};

mod stack_back;
mod touch_spin;

#[test]
fn anchor_cycles_fall_back_on_their_own_axis_and_hidden_targets_are_skipped() {
    let box_at = |id, x| {
        let mut child = node("Rectangle", id);
        length(&mut child, "x", x);
        length(&mut child, "width", 20.0);
        length(&mut child, "height", 10.0);
        child
    };
    let mut left = box_at("left", 5.0);
    left.properties
        .insert("anchor-left", "right.right".to_owned().into());
    left.properties
        .insert("anchor-bottom", "parent.bottom".to_owned().into());
    let mut right = box_at("right", 40.0);
    right
        .properties
        .insert("anchor-left", "left.right".to_owned().into());
    let mut follower = box_at("follower", 80.0);
    follower
        .properties
        .insert("anchor-left", "right.right".to_owned().into());
    let mut window = Node::new("Window", None);
    window.children.extend([left, right, follower]);

    let viewport = Size::new(180.0, 100.0);
    let before = layout(&window, viewport, &mut Text);
    assert_eq!(before.children[0].bounds, Rect::new(5.0, 90.0, 20.0, 10.0));
    assert_eq!(before.children[1].bounds.x, 40.0);
    assert_eq!(before.children[2].bounds.x, 80.0);

    boolean(&mut window.children[0], "visible", false);
    let after = layout(&window, viewport, &mut Text);
    assert_eq!(after.children[1].bounds.x, 40.0);
    assert_eq!(after.children[2].bounds.x, 60.0);
}

#[test]
fn scroll_anchors_keep_their_reference_height_when_another_child_overflows() {
    let mut scroll = node("Flickable", "scroll");
    let mut fill = node("Rectangle", "fill");
    fill.properties
        .insert("anchor-fill", "parent".to_owned().into());
    let mut tall = node("Rectangle", "tall");
    length(&mut tall, "height", 320.0);
    scroll.children.extend([fill, tall]);
    let mut window = Node::new("Window", None);
    window.children.push(scroll);

    let frame = layout(&window, Size::new(200.0, 100.0), &mut Text);
    let scroller = &frame.children[0];
    assert_eq!(
        scroller.children[0].bounds,
        Rect::new(0.0, 0.0, 200.0, 100.0)
    );
    assert_eq!(scroller.children[1].bounds.height, 320.0);
}

#[test]
fn moving_small_subtrees_share_the_local_layout_node_budget() {
    struct View;
    impl xrack_runtime::Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }
    struct Metrics(Rc<()>);
    impl TextMeasurer for Metrics {
        fn measure(&mut self, text: &str, font_size: f32, _: f32) -> Size {
            Size::new(text.len() as f32 * font_size * 0.5, font_size)
        }
        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.0.clone())
        }
    }

    let mut cell = Node::new("Rectangle", None);
    length(&mut cell, "width", 16.0);
    length(&mut cell, "height", 16.0);
    for kind in ["Rectangle", "Image", "Label"] {
        let mut leaf = Node::new(kind, None);
        length(&mut leaf, "width", 12.0);
        length(&mut leaf, "height", 12.0);
        cell.children.push(leaf);
    }
    let repeated = vec![cell.into_shared(); 768];
    let mut window = Rc::new(Node::new("Window", None));
    let mut cache = RowLayoutCache::default();
    let mut text = Metrics(Rc::new(()));
    let viewport = Size::new(720.0, 480.0);
    let input = Interaction::default();

    for (frame, (count, reused, narrow_first)) in [
        (768, 0, false),
        (768, 768, false),
        (32, 32, false),
        (768, 32, false),
        (768, 768, false),
        (768, 767, true),
        (768, 768, true),
    ]
    .into_iter()
    .enumerate()
    {
        let scene = Rc::make_mut(&mut window);
        scene.children.clear();
        scene.children.extend_from_slice(&repeated[..count]);
        for (index, tile) in scene.children.iter_mut().enumerate() {
            length(tile, "x", (index % 36 * 19 + frame) as f64 + 0.125);
            length(tile, "y", (index / 36 * 20 + frame) as f64 + 0.25);
        }
        if narrow_first {
            length(&mut scene.children[0], "width", 10.0);
        }
        let caption = format!("Status for frame {frame}");
        for tile in &mut scene.children {
            tile.children[2]
                .properties
                .insert("text", caption.clone().into());
        }
        let before = cache.stats().subtree_reuses;
        let (prepared, cached) = prepare_scene_with_row_cache(
            &View,
            window.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, fresh) =
            prepare_scene_with_input(&View, window.clone(), viewport, &mut text, &input);
        assert_eq!(
            prepared.children[0].children[2].text("text"),
            Some(caption.as_str())
        );
        assert_eq!(cached, fresh, "frame {frame}");
        assert_eq!(
            cache.stats().subtree_reuses - before,
            reused,
            "frame {frame}"
        );
    }
}

#[test]
fn fixed_cross_axis_fill_matches_linear_measurement() {
    for (kind, main_constraint, cross_size) in [
        ("HorizontalLayout", "min-width", "height"),
        ("VerticalLayout", "min-height", "width"),
    ] {
        let mut line = node(kind, "line");
        line.properties
            .insert(cross_size, xrack_runtime::Percent(0.5).into());
        length(&mut line, "spacing", 7.25);
        length(&mut line, "padding", 3.0);
        line.children.push(node("Rectangle", "first"));
        line.children.push(node("Path", "second"));
        let mut hidden = node("Text", "hidden");
        hidden.properties.insert("visible", false.into());
        hidden.properties.insert("text", "label".to_owned().into());
        line.children.push(hidden);

        for mixed in [false, true] {
            if mixed {
                length(&mut line.children[0], main_constraint, 50.0);
                line.children[2].properties.insert("visible", true.into());
            }
            for viewport in [Size::new(4.0, 6.0), Size::new(180.0, 200.0)] {
                let expected = linear::measure(&line, viewport, &mut Text);
                assert_eq!(
                    linear::fill_size_with_fixed_cross_axis(&line, viewport, &mut Text),
                    Some(Size::new(
                        expected.x.clamp(viewport.width),
                        expected.y.clamp(viewport.height),
                    ))
                );
            }
        }
        length(&mut line, main_constraint, 100.0);
        assert!(
            linear::fill_size_with_fixed_cross_axis(&line, Size::new(20.0, 30.0), &mut Text)
                .is_none()
        );
    }
}

#[test]
fn geometric_primitives_preserve_relative_sizes_and_axis_constraints() {
    for kind in ["Rectangle", "Path", "TouchArea"] {
        let mut primitive = node(kind, "primitive");
        let mut child = node("Text", "child");
        length(&mut child, "width", 128.0);
        length(&mut child, "height", 40.0);
        primitive.children.push(child);
        let available = Size::new(200.0, 80.0);
        let natural = linear::measure(&primitive, available, &mut Text);
        assert_eq!(natural.x.preferred, 0.0);
        assert_eq!(natural.y.max, f32::INFINITY);
        assert_eq!(natural.x.stretch, 1.0);

        primitive
            .properties
            .insert("width", xrack_runtime::Percent(0.25).into());
        primitive.properties.insert("vertical-stretch", 2.0.into());
        length(&mut primitive, "min-height", 12.0);
        length(&mut primitive, "preferred-height", 30.0);
        length(&mut primitive, "max-height", 24.0);
        let constrained = linear::measure(&primitive, available, &mut Text);
        assert_eq!(constrained.x.preferred, 50.0);
        assert_eq!(constrained.x.max, 50.0);
        assert_eq!(constrained.y.min, 12.0);
        assert_eq!(constrained.y.preferred, 24.0);
        assert_eq!(constrained.y.stretch, 2.0);
    }
}

#[test]
fn continued_styled_layout_accounts_for_shared_and_replaced_shapes() {
    use xrack_runtime::StyledText;

    let node = node("StyledText", "document");
    let available = Size::new(320.0, f32::INFINITY);
    let font_size = 16.0;
    let advance = |text: &str, _| text.chars().count() as f32 * 7.0;
    let shape =
        |text: &str, _| VisualTextLine::new(text, text.char_indices().map(|(i, _)| (i, 7.0)));
    let indexed = |document: &StyledText| {
        let mut node = node.clone();
        node.properties.insert("text", document.clone().into());
        StyledTextLayout::indexed(&node, document, available, font_size, advance)
    };
    let prefix = "same\nsame\n";
    let previous = StyledText::from_plain_text(format!("{prefix}discarded"));
    let (mut layout, lines) = indexed(&previous);
    let mut allocations = Vec::new();
    layout.shape_selection_recording(&previous, 0, shape, |index, bytes| {
        allocations.push((index, bytes));
    });
    assert_eq!(allocations.len(), 2);
    let tail = StyledText::from_plain_text("新段落");
    let next = StyledText::from_plain_text(format!("{prefix}{}", tail.text()));
    let (mut layout, _, kept) =
        layout.replace_tail(&lines, prefix.len(), indexed(&tail), 0, font_size);
    let retained = allocations.partition_point(|(index, _)| *index < kept);
    assert_eq!(retained, 1);
    let retained_bytes: usize = allocations[..retained].iter().map(|(_, size)| size).sum();
    let new_bytes = layout.shape_selection_recording(&next, kept, shape, |index, _| {
        assert!(index >= kept);
    });
    assert_eq!(
        layout.allocated_bytes(),
        layout.fragments.capacity() * size_of::<StyledFragment>() + retained_bytes + new_bytes
    );
}

#[test]
fn calendar_arrows_start_at_the_disabled_selection_and_skip_excluded_days() {
    use xrack_runtime::{CalendarAction, ControlAction, Date};

    let date = |day| Date::new(2024, 3, day).unwrap();
    for kind in ["Calendar", "MonthGrid"] {
        let mut calendar = node(kind, "calendar");
        length(&mut calendar, "width", 396.0);
        length(&mut calendar, "height", 322.0);
        calendar.properties.insert("date", date(5).into());
        calendar.properties.insert("display-month", date(1).into());
        calendar
            .properties
            .insert("disabled-dates", Value::DateList(vec![date(4), date(5)]));
        calendar
            .properties
            .insert("disabled-weekdays", Value::IntList(vec![6, 7]));
        let mut window = node("Window", "root");
        window.children.push(calendar);
        let layout = layout(&window, Size::new(440.0, 440.0), &mut Text);
        let mut input = Interaction::default();
        let focus = input.accessibility_target(&window, &layout, &[0]).unwrap();
        input.focus_target(&window, &layout, focus);
        for (key, expected) in [
            (NavigationKey::Left, date(1)),
            (NavigationKey::Right, date(6)),
        ] {
            let action = input.calendar_key(&window, &layout, key).unwrap();
            assert_eq!(action.path, [0]);
            assert_eq!(
                action.action,
                ControlAction::Calendar(CalendarAction::Select(expected))
            );
        }
    }
}

#[test]
fn adjacent_text_fields_skip_unavailable_controls_and_stop_in_the_dialog() {
    let mut window = node("Window", "root");
    let mut dialog = node("Dialog", "form");
    boolean(&mut dialog, "open", true);
    length(&mut dialog, "width", 280.0);
    length(&mut dialog, "height", 250.0);
    let mut column = node("VerticalLayout", "fields");
    for (kind, id) in [
        ("LineEdit", "name"),
        ("Button", "show-hint"),
        ("LineEdit", "readonly"),
        ("LineEdit", "disabled"),
        ("LineEdit", "hidden"),
        ("TextInput", "phone"),
    ] {
        let mut field = node(kind, id);
        length(&mut field, "height", 32.0);
        match id {
            "readonly" => boolean(&mut field, "read-only", true),
            "disabled" => boolean(&mut field, "enabled", false),
            "hidden" => boolean(&mut field, "visible", false),
            _ => {}
        }
        column.children.push(field);
    }
    dialog.children.push(column);
    window
        .children
        .extend([dialog, node("TextInput", "background")]);
    let frame = layout(&window, Size::new(360.0, 400.0), &mut Text);
    let mut input = Interaction::default();
    input.reconcile(&window, &frame);
    assert!(input.is_focused(&[0, 0, 0]));
    assert_eq!(input.focus_adjacent_text_field(&window, &frame, true), None);
    assert_eq!(input.focus_next_text_field(&window, &frame), Some(false));
    assert!(input.is_focused(&[0, 0, 5]));
    assert_eq!(input.focus_next_text_field(&window, &frame), None);
    assert_eq!(
        input.focus_adjacent_text_field(&window, &frame, true),
        Some(false)
    );
    assert!(input.is_focused(&[0, 0, 0]));
}

#[test]
fn leaf_placement_matches_general_placement() {
    let bounds = Rect::new(16.0, 14.0, 38.0, 27.0);
    let viewport = Rect::new(0.0, 0.0, 80.0, 64.0);
    for kind in [
        "Rectangle",
        "TableCell",
        "TreeCell",
        "Label",
        "Text",
        "StyledText",
    ] {
        for (scale, clipping, visible, enabled, defer_styled_text) in [
            (1.0, None, true, true, false),
            (1.25, Some(true), true, false, false),
            (0.0, Some(false), true, true, false),
            (1.0, Some(false), false, true, false),
            (1.0, None, true, true, true),
        ] {
            let mut parent = node("Rectangle", "panel");
            parent.properties.insert("transform-scale-x", scale.into());
            let mut space = None;
            let mut ancestor_shapes = Some(Rc::new(ClipChain {
                shape: RoundedClip {
                    bounds: viewport,
                    radii: [9.0; 4],
                },
                parent: None,
                space: None,
            }));
            let (clip, _) = frame_projection::apply(
                &parent,
                viewport,
                viewport,
                &mut space,
                &mut ancestor_shapes,
            );
            let mut leaf = node(kind, "leaf");
            length(&mut leaf, "border-radius", 6.0);
            length(&mut leaf, "font-size", 18.0);
            boolean(&mut leaf, "mouse-drag-pan-enabled", true);
            boolean(&mut leaf, "pull-to-refresh", true);
            if kind == "StyledText" {
                let text = xrack_runtime::StyledText::from_markdown(
                    "**字** [לינק](https://example.invalid)",
                )
                .unwrap();
                leaf.properties.insert("text", text.into());
            }
            if let Some(clipping) = clipping {
                boolean(&mut leaf, "clip", clipping);
                leaf.properties
                    .insert("transform-rotation", 11.0_f64.into());
                leaf.properties
                    .insert("layout-direction", LayoutDirection::LeftToRight.into());
            }
            let mut context = LayoutContext {
                defer_styled_text,
                text: &mut Text,
                layout_direction: Some(LayoutDirection::RightToLeft),
                offsets: &BTreeMap::new(),
                input: None,
                path: vec![0, 0],
                row_key: None,
                anchor: None,
                resolved_list_scroll: None,
                resolved_table: None,
                cache_table_sizes: false,
                row_layouts: None,
                ancestor_shapes,
                space,
                viewport,
            };
            let specialized = context.place(&leaf, bounds, clip, visible, enabled, None);
            context.layout_direction = leaf.layout_direction().or(context.layout_direction);
            let general = context.place_node(&leaf, bounds, clip, visible, enabled, None);
            assert_eq!(
                specialized, general,
                "{kind}, parent scale={scale}, clip={clipping:?}"
            );
        }
    }
}

#[test]
fn cloning_a_cached_layout_projects_each_nested_clip_once() {
    let mut window = node("Window", "root");
    let mut panel = node("Rectangle", "panel");
    length(&mut panel, "x", 12.0);
    length(&mut panel, "width", 80.0);
    length(&mut panel, "height", 60.0);
    length(&mut panel, "border-radius", 9.0);
    boolean(&mut panel, "clip", true);
    let mut label = node("Label", "label");
    label
        .properties
        .insert("text", "cached label".to_owned().into());
    length(&mut label, "y", 16.0);
    panel.children.extend([label.clone(), label.clone(), label]);
    window.children.push(panel);
    let mut local = layout(&window, Size::new(100.0, 80.0), &mut Text);
    let siblings = &mut local.children[0].children;
    assert_eq!(siblings[0].clip, siblings[1].clip);
    siblings[2].clip = siblings[2]
        .clip
        .intersect(Rect::new(28.0, 19.0, 18.0, 27.0));
    assert_ne!(siblings[1].clip, siblings[2].clip);
    let outer_clip = Rc::new(ClipChain {
        shape: RoundedClip {
            bounds: Rect::new(10.0, 7.0, 80.0, 60.0),
            radii: [16.0; 4],
        },
        parent: None,
        space: None,
    });

    for (bounds, clip, ancestors) in [
        (
            Rect::new(4.125, 7.0, 100.0, 80.0),
            Rect::new(0.0, 0.0, 150.0, 130.0),
            None,
        ),
        (
            Rect::new(-12.75, 9.5, 100.0, 80.0),
            Rect::new(0.0, 15.0, 70.0, 55.0),
            Some(&outer_clip),
        ),
    ] {
        let mut expected = local.clone();
        row_layout::place_in_view(&mut expected, bounds, clip, ancestors);
        let projected = row_layout::clone_in_view(&local, bounds, clip, ancestors);
        assert_eq!(projected, expected);
    }
}

#[test]
fn static_absolute_leaves_reuse_layout_until_their_inputs_change() {
    struct View;

    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    struct Metrics {
        revision: Rc<()>,
        scale: f32,
    }

    impl TextMeasurer for Metrics {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            Text.measure(text, size, width)
        }

        fn font_size(&self, node: &Node) -> f32 {
            font_size(node) * self.scale
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.revision.clone())
        }
    }

    let mut window = node("Window", "root");
    let mut flickable = node("Flickable", "view");
    for (name, value) in [
        ("width", 120.0),
        ("height", 90.0),
        ("content-height", 240.0),
    ] {
        length(&mut flickable, name, value);
    }
    window.children.push(flickable);
    for (kind, id, x) in [
        ("Rectangle", "left", 20.0),
        ("Image", "center", 90.0),
        ("AnimatedImage", "right", 150.0),
    ] {
        let mut leaf = node(kind, id);
        length(&mut leaf, "x", x);
        length(&mut leaf, "height", 16.0);
        leaf.properties
            .insert("width", Value::Percent(xrack_runtime::Percent(0.25)));
        window.children.push(leaf);
    }

    let mut root = Rc::new(window);
    let mut viewport = Size::new(180.0, 110.0);
    let mut input = Interaction::default();
    let mut cache = RowLayoutCache::default();
    let mut text = Metrics {
        revision: Rc::new(()),
        scale: 1.0,
    };
    let mut previous = None;
    for (frame, expected_reuses) in [0, 3, 3, 2, 0, 0, 1].into_iter().enumerate() {
        match frame {
            1 => assert!(input.accessibility_scroll_to(
                previous.as_ref().unwrap(),
                &[0],
                Orientation::Vertical,
                34.0
            )),
            2 => length(&mut Rc::make_mut(&mut root).children[0], "y", 7.0),
            3 => length(&mut Rc::make_mut(&mut root).children[2], "x", 81.0),
            4 => viewport.width = 208.0,
            5 => {
                text = Metrics {
                    revision: Rc::new(()),
                    scale: 1.5,
                }
            }
            6 => Rc::make_mut(&mut root).children.swap(1, 2),
            _ => (),
        }
        let before = cache.stats().subtree_reuses;
        let (_, cached) = prepare_scene_with_row_cache(
            &View,
            root.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, fresh) = prepare_scene_with_input(&View, root.clone(), viewport, &mut text, &input);
        assert_eq!(cached, fresh, "frame={frame}");
        assert_eq!(
            cache.stats().subtree_reuses - before,
            expected_reuses,
            "frame={frame}"
        );
        previous = Some(cached);
    }
}

#[test]
fn layout_indexes_drawers_added_to_a_prepared_scene() {
    let mut root = node("Window", "root");
    root.children.push(node("Rectangle", "panel"));
    let viewport = Size::new(320.0, 240.0);
    let initial = layout(&root, viewport, &mut Text);
    assert!(initial.drawer_paths().is_empty());
    assert!(initial.overlay_paths().is_empty());

    let mut updated = root.clone();
    let mut drawer = node("Drawer", "tools");
    length(&mut drawer, "drag-margin", 16.0);
    length(&mut drawer, "width", 120.0);
    updated.children[0].children.push(drawer);
    let closed = layout(&updated, viewport, &mut Text);
    assert_eq!(closed.drawer_paths(), &[vec![0, 0]]);
    assert!(closed.overlay_paths().is_empty());

    boolean(&mut updated.children[0].children[0], "open", true);
    let open = layout(&updated, viewport, &mut Text);
    assert_eq!(open.overlay_paths(), &[vec![0, 0]]);
    let mut cleared = open.clone();
    assert!(Rc::ptr_eq(
        open.details.as_ref().unwrap(),
        cleared.details.as_ref().unwrap()
    ));
    cleared.set_overlay_index(None);
    assert!(cleared.overlay_paths().is_empty());
    assert!(cleared.details.is_none());
    assert_eq!(open.overlay_paths(), &[vec![0, 0]]);
    updated.children[0].children.clear();
    assert_eq!(layout(&updated, viewport, &mut Text), initial);
    assert_eq!(layout(&root, viewport, &mut Text), initial);
}

#[test]
fn drawer_gestures_start_at_the_safe_viewport_edge() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, DrawerEdge};

    let mut window = node("Window", "root");
    let mut drawer = node("Drawer", "tools");
    drawer.properties.insert("edge", DrawerEdge::Bottom.into());
    length(&mut drawer, "height", 140.0);
    length(&mut drawer, "drag-margin", 20.0);
    window.children.push(drawer);
    set_window_safe_area(&mut window, [0.0, 42.0, 0.0, 34.0]);
    let size = Size::new(390.0, 740.0);
    let frame = layout(&window, size, &mut Text);
    let gesture = |scene: &Node, frame: &LayoutNode, start: Point, finish: Point| {
        let mut input = Interaction::default();
        let now = Instant::now();
        input.begin_scroll_gesture(scene, frame, start, now, ScrollGestureKind::Touch);
        input
            .end_scroll_gesture(scene, frame, finish, now + Duration::from_millis(300))
            .control
    };

    let open = gesture(
        &window,
        &frame,
        Point { x: 180.0, y: 700.0 },
        Point { x: 180.0, y: 590.0 },
    )
    .unwrap();
    assert_eq!(open.path, [0]);
    assert_eq!(open.action, ControlAction::Drawer(true));
    assert!(
        gesture(
            &window,
            &frame,
            Point { x: 180.0, y: 733.0 },
            Point { x: 180.0, y: 598.0 }
        )
        .is_none()
    );

    boolean(&mut window.children[0], "open", true);
    let frame = layout(&window, size, &mut Text);
    assert_eq!(
        frame.children[0].bounds,
        Rect::new(0.0, 566.0, 390.0, 140.0)
    );
    let close = gesture(
        &window,
        &frame,
        Point { x: 180.0, y: 573.0 },
        Point { x: 180.0, y: 675.0 },
    )
    .unwrap();
    assert_eq!(close.path, [0]);
    assert_eq!(close.action, ControlAction::Drawer(false));
}

#[test]
fn platform_safe_area_keeps_window_padding_and_centers_the_dialog_in_available_space() {
    let mut window = node("Window", "root");
    length(&mut window, "padding", 8.0);
    let mut content = node("Rectangle", "content");
    length(&mut content, "width", 24.0);
    length(&mut content, "height", 20.0);
    let mut dialog = node("Dialog", "dialog");
    boolean(&mut dialog, "open", true);
    length(&mut dialog, "width", 120.0);
    length(&mut dialog, "height", 80.0);
    window.children.extend([content, dialog]);

    let viewport = Size::new(400.0, 320.0);
    set_window_safe_area(&mut window, [12.0, 42.0, 4.0, 108.0]);
    let safe = layout(&window, viewport, &mut Text);
    assert_eq!(safe.children[0].bounds, Rect::new(20.0, 50.0, 24.0, 20.0));
    assert_eq!(safe.children[1].bounds, Rect::new(144.0, 87.0, 120.0, 80.0));

    set_window_safe_area(&mut window, [0.0; 4]);
    let full = layout(&window, viewport, &mut Text);
    assert_eq!(full.children[0].bounds, Rect::new(8.0, 8.0, 24.0, 20.0));
    assert_eq!(
        full.children[1].bounds,
        Rect::new(140.0, 120.0, 120.0, 80.0)
    );
}

#[test]
fn scrollable_dialog_places_actions_at_the_trailing_side_only_in_short_wide_space() {
    for boxed in [false, true] {
        let mut window = node("Window", "root");
        let mut dialog = node("Dialog", "dialog");
        boolean(&mut dialog, "open", true);
        length(&mut dialog, "width", 350.0);
        length(&mut dialog, "height", 320.0);
        dialog.properties.insert("title", "Form".to_owned().into());

        let mut body = node("ScrollView", "body");
        body.properties.insert("vertical-stretch", 1.0.into());
        let mut field = node("LineEdit", "entry");
        length(&mut field, "height", 44.0);
        body.children.push(field);
        let action = |kind, id| {
            let mut button = node(kind, id);
            length(&mut button, "width", 80.0);
            length(&mut button, "height", 44.0);
            button
        };
        let mut footer = if boxed {
            let mut footer = node("DialogButtonBox", "footer");
            footer.children.extend([
                action("StandardButton", "accept"),
                action("StandardButton", "cancel"),
            ]);
            footer
        } else {
            action("DialogButton", "footer")
        };
        footer.properties.insert("text", "Save".to_owned().into());
        dialog.children.extend([body, footer]);
        window.children.push(dialog);

        let portrait = layout(&window, Size::new(390.0, 480.0), &mut Text);
        let before = portrait.at_path(&[0, 0]).unwrap().bounds;
        let below = portrait.at_path(&[0, 1]).unwrap().bounds;
        assert!(before.height >= 44.0 && before.y + before.height <= below.y);

        for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            window
                .properties
                .insert("layout-direction", direction.into());
            for height in [139.0, 160.0] {
                let short = layout(&window, Size::new(680.0, height), &mut Text);
                let body = short.at_path(&[0, 0]).unwrap().bounds;
                let actions = short.at_path(&[0, 1]).unwrap().bounds;
                assert!(body.width >= 120.0 && body.height >= 44.0, "{body:?}");
                assert_eq!(actions.height, 44.0);
                assert_eq!(body.y + body.height, actions.y + actions.height);
                assert!(match direction {
                    LayoutDirection::LeftToRight => body.x + body.width <= actions.x,
                    LayoutDirection::RightToLeft => actions.x + actions.width <= body.x,
                });
                if boxed {
                    for index in 0..2 {
                        let button = short.at_path(&[0, 1, index]).unwrap().bounds;
                        assert_eq!(button.intersect(actions), button);
                    }
                }
            }
        }

        let narrow = layout(&window, Size::new(165.0, 160.0), &mut Text);
        let body = narrow.at_path(&[0, 0]).unwrap().bounds;
        let actions = narrow.at_path(&[0, 1]).unwrap().bounds;
        assert!(body.y + body.height <= actions.y);
    }
}

#[test]
fn live_native_editor_retains_focus_through_clipping_but_not_disabled_or_hidden_state() {
    let mut window = node("Window", "root");
    let mut dialog = node("Dialog", "form");
    boolean(&mut dialog, "open", true);
    length(&mut dialog, "width", 350.0);
    length(&mut dialog, "height", 320.0);
    dialog.properties.insert("title", "Form".to_owned().into());
    let mut body = node("ScrollView", "body");
    body.properties.insert("vertical-stretch", 1.0.into());
    let mut field = node("LineEdit", "entry");
    length(&mut field, "height", 44.0);
    body.children.push(field);
    let mut action = node("DialogButton", "save");
    length(&mut action, "height", 44.0);
    dialog.children.extend([body, action]);
    window.children.push(dialog);

    let full = layout(&window, Size::new(720.0, 360.0), &mut Text);
    let short = layout(&window, Size::new(720.0, 96.0), &mut Text);
    let path = [0, 0, 0];
    let mut input = Interaction::default();
    input.reconcile(&window, &full);
    let target = input.accessibility_target(&window, &full, &path).unwrap();
    input.focus_target(&window, &full, target.clone());
    assert!(input.accessibility_target(&window, &short, &path).is_none());

    let mut without_native = input.clone();
    without_native.reconcile(&window, &short);
    assert_ne!(without_native.focused.as_ref(), Some(&target));

    input.reconcile_with_native_editor(&window, &short, Some(&target));
    assert_eq!(input.focused.as_ref(), Some(&target));
    input.reconcile_with_native_editor(&window, &full, Some(&target));
    assert_eq!(input.focused.as_ref(), Some(&target));

    for property in ["enabled", "visible"] {
        let mut changed = window.clone();
        boolean(
            &mut changed.children[0].children[0].children[0],
            property,
            false,
        );
        let changed_layout = layout(&changed, Size::new(720.0, 96.0), &mut Text);
        let mut without_editor = input.clone();
        without_editor.reconcile_with_native_editor(&changed, &changed_layout, Some(&target));
        assert_ne!(without_editor.focused.as_ref(), Some(&target));
    }
}

#[test]
fn narrow_picker_with_long_labels_preserves_wheel_height_and_stacks_actions() {
    let mut window = node("Window", "root");
    let mut dialog = node("Dialog", "picker");
    boolean(&mut dialog, "open", true);
    for (property, label) in [
        ("accept-text", "Confirm inspection time"),
        ("cancel-text", "Keep the previous time"),
    ] {
        dialog.properties.insert(property, label.to_owned().into());
    }
    xrack_runtime::TimePopupState::default().populate(&mut dialog, || xrack_runtime::Time::MIN);
    window.children.push(dialog);
    let layout = layout(&window, Size::new(270.0, 500.0), &mut Text);
    let body = layout.at_path(&[0, 0]).unwrap().bounds;
    let first = layout.at_path(&[0, 1, 0]).unwrap().bounds;
    let second = layout.at_path(&[0, 1, 1]).unwrap().bounds;
    assert!(body.height >= 148.0, "{body:?}");
    assert!(body.y + body.height <= first.y.min(second.y));
    assert_eq!(first.x, second.x);
    assert!((first.y - second.y).abs() >= 44.0);
    assert_eq!(first.height, 44.0);
    assert_eq!(second.height, 44.0);
}

#[test]
fn centered_icon_keeps_a_header_free_to_grow_for_larger_text() {
    struct ScaledText;
    impl TextMeasurer for ScaledText {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            Size::new((text.len() as f32 * size * 0.5).min(width), size * 1.4)
        }
        fn font_size(&self, node: &Node) -> f32 {
            super::font_size(node) * 1.5
        }
    }

    let mut row = node("HorizontalLayout", "header");
    length(&mut row, "min-height", 50.0);
    row.properties
        .insert("cross-axis-alignment", CrossAxisAlignment::Center.into());
    let mut icon = node("Rectangle", "icon");
    length(&mut icon, "width", 40.0);
    length(&mut icon, "height", 40.0);
    let mut title = node("VerticalLayout", "title");
    for (name, size) in [("heading", 22.0), ("subtitle", 12.0)] {
        let mut label = node("Label", name);
        label.properties.insert("text", "Field".to_owned().into());
        length(&mut label, "font-size", size);
        title.children.push(label);
    }
    row.children.extend([icon, title]);
    let measured = crate::linear::measure(&row, Size::new(402.0, 874.0), &mut ScaledText);
    let expected = (22.0 + 12.0) * 1.5 * 1.4;
    assert!(
        measured.y.preferred >= expected - 0.01,
        "{:?}",
        measured.y.preferred
    );
    assert!(measured.y.max >= measured.y.preferred);
}

#[test]
fn editor_padding_reserves_space_and_clamps_exhausted_axes() {
    let mut editor = Node::new("LineEdit", None);
    for (property, value) in [
        ("padding", 8.0),
        ("padding-left", 28.0),
        ("padding-bottom", 20.0),
    ] {
        editor.properties.insert(property, Length(value).into());
    }
    assert_eq!(
        text_editor_viewport(&editor, Rect::new(10.0, 20.0, 100.0, 50.0)),
        Rect::new(38.0, 28.0, 64.0, 22.0)
    );
    assert_eq!(
        text_editor_viewport(&editor, Rect::new(10.0, 20.0, 12.0, 4.0)),
        Rect::new(22.0, 24.0, 0.0, 0.0)
    );
    let preferred = crate::linear::measure(&editor, Size::new(320.0, 120.0), &mut Text);
    assert_eq!(preferred.x.preferred, 204.0);
    assert_eq!(preferred.y.preferred, font_size(&editor) * 1.4 + 28.0);
}

struct Text;
impl TextMeasurer for Text {
    fn measure(&mut self, text: &str, _: f32, max_width: f32) -> Size {
        Size::new((text.chars().count() as f32 * 8.0).min(max_width), 10.0)
    }
}
fn node(kind: &'static str, id: &'static str) -> Node {
    Node::new(kind, Some(id))
}
fn length(node: &mut Node, name: &'static str, value: f64) {
    node.properties.insert(name, Value::Length(Length(value)));
}
fn boolean(node: &mut Node, name: &'static str, value: bool) {
    node.properties.insert(name, Value::Bool(value));
}
fn button(id: &'static str) -> Node {
    let mut n = node("Button", id);
    length(&mut n, "width", 80.0);
    length(&mut n, "height", 20.0);
    n
}

#[test]
fn button_content_measures_and_places_display_modes() {
    use xrack_runtime::{ButtonDisplay, Image, ImageFormat, LayoutDirection};

    let mut button = node("ToolButton", "tool");
    button.properties.insert("text", "Grid".to_owned().into());
    button.properties.insert(
        "icon",
        Image::Embedded {
            format: ImageFormat::Svg,
            bytes: b"<svg/>",
            size: [24, 24],
        }
        .into(),
    );
    length(&mut button, "spacing", 7.0);
    let area = Rect::new(10.0, 20.0, 80.0, 60.0);
    let label = Size::new(30.0, 20.0);
    let beside = ButtonContent::new(&button);
    assert_eq!(beside.natural_size(label), Size::new(55.0, 20.0));
    let ltr = beside.place(area, label, 20.0, true, LayoutDirection::LeftToRight);
    let rtl = beside.place(area, label, 20.0, true, LayoutDirection::RightToLeft);
    assert_eq!(ltr.icon.unwrap().x + 18.0 + 7.0, ltr.label.x);
    assert_eq!(rtl.label.x + rtl.label.width + 7.0, rtl.icon.unwrap().x);
    let narrow = beside.place(
        Rect::new(10.0, 20.0, 6.0, 8.0),
        label,
        20.0,
        true,
        LayoutDirection::LeftToRight,
    );
    assert_eq!(narrow.icon.unwrap().x, 10.0);
    assert_eq!(narrow.label.width, 0.0);
    button
        .properties
        .insert("display", ButtonDisplay::TextUnderIcon.into());
    let stacked = ButtonContent::new(&button);
    assert_eq!(stacked.natural_size(label), Size::new(30.0, 45.0));
    let placement = stacked.place(area, label, 20.0, true, LayoutDirection::RightToLeft);
    assert_eq!(placement.icon.unwrap().y + 18.0 + 7.0, placement.label.y);
    button
        .properties
        .insert("display", ButtonDisplay::TextOnly.into());
    button.properties.insert("icon-only", true.into());
    let text_only = ButtonContent::new(&button);
    assert_eq!(text_only.label, "Grid");
    assert!(text_only.icon.is_none());
    button.properties.remove("display");
    assert_eq!(ButtonContent::new(&button).label, "");
    button.properties.remove("icon");
    assert_eq!(ButtonContent::new(&button).label, "Grid");
}

#[test]
fn enlarged_button_content_fits_fixed_height_and_keeps_stacked_icons() {
    use xrack_runtime::{ButtonDisplay, Image, ImageFormat};

    let mut button = node("Button", "review");
    button
        .properties
        .insert("text", "Review options".to_owned().into());
    let bounds = Rect::new(0.0, 0.0, 200.0, 44.0);
    let content = ButtonContent::new(&button);
    let (area, size) = content.scaled_label_metrics(bounds, 40.0);
    assert_eq!(area, Rect::new(10.0, 4.0, 180.0, 36.0));
    assert!((size * 1.4 - 36.0).abs() < 0.01);
    let spacious = Rect::new(0.0, 0.0, 200.0, 90.0);
    assert_eq!(
        content.scaled_label_metrics(spacious, 40.0),
        (spacious.inset(10.0), 40.0)
    );

    length(&mut button, "padding", 10.0);
    let (area, size) = ButtonContent::new(&button).scaled_label_metrics(bounds, 40.0);
    assert_eq!(area, bounds.inset(10.0));
    assert!((size * 1.4 - 24.0).abs() < 0.01);
    button.properties.remove("padding");
    button
        .properties
        .insert("display", ButtonDisplay::TextUnderIcon.into());
    button.properties.insert(
        "icon",
        Image::Embedded {
            format: ImageFormat::Svg,
            bytes: b"<svg/>",
            size: [24, 24],
        }
        .into(),
    );
    let content = ButtonContent::new(&button);
    let (area, size) = content.scaled_label_metrics(bounds, 40.0);
    let line = size * 1.4;
    let placed = content.place(
        area,
        Size::new(80.0, line),
        line,
        true,
        LayoutDirection::LeftToRight,
    );
    let icon = placed.icon.unwrap();
    assert!(icon.height > 0.0);
    assert!((placed.label.height - line).abs() < 0.01);
    assert!(icon.y + icon.height <= placed.label.y);
    assert!(placed.label.y + placed.label.height <= area.y + area.height + 0.01);
}

#[test]
fn focus_scope_enters_radio_group_on_the_selected_option() {
    let mut root = node("Window", "root");
    let mut scope = node("FocusScope", "scope");
    length(&mut scope, "width", 240.0);
    length(&mut scope, "height", 180.0);
    scope.children.push(button("before"));
    let mut group = node("RadioGroup", "options");
    length(&mut group, "y", 40.0);
    length(&mut group, "width", 160.0);
    length(&mut group, "height", 100.0);
    group.children.push(node("RadioButton", "first"));
    group.children.push(node("RadioButton", "second"));
    group.properties.insert("current-index", 1_i64.into());
    let handle = xrack_runtime::FocusHandle::default();
    group.focus_handle = Some(handle.clone());
    scope.initial_focus = Some(handle);
    scope.children.push(group);
    root.children.push(scope);
    let size = Size::new(260.0, 200.0);
    for (enabled, expected) in [(true, "second"), (false, "before")] {
        boolean(&mut root.children[0].children[1], "enabled", enabled);
        let frame = layout(&root, size, &mut Text);
        let mut input = Interaction::default();
        input.focus_next(&root, &frame, false);
        assert_eq!(
            input.focused.as_ref().and_then(Target::element_id),
            Some(expected)
        );
    }
}

#[test]
fn radio_group_measures_its_title_in_both_orientations() {
    let mut group = node("RadioGroup", "options");
    let title = "Long shared rendering quality caption";
    group.properties.insert("title", title.to_owned().into());
    length(&mut group, "padding-left", 6.0);
    length(&mut group, "padding-right", 14.0);
    group.children.push(node("RadioButton", "option"));
    let expected = 8.0 * title.chars().count() as f32 + 20.0;
    for orientation in [
        xrack_runtime::Orientation::Vertical,
        xrack_runtime::Orientation::Horizontal,
    ] {
        group.properties.insert("orientation", orientation.into());
        let metrics = linear::measure(&group, Size::new(400.0, 130.0), &mut Text);
        assert!(metrics.x.preferred >= expected);
    }
}

#[test]
fn inspector_picks_a_disabled_front_element_and_the_view_over_its_virtual_rows() {
    let mut root = node("Window", "root");
    let mut field = button("back");
    field.kind = "SearchField";
    root.children.push(field);
    let mut front = button("front");
    boolean(&mut front, "enabled", false);
    root.children.push(front);
    let mut view = node("ListView", "results");
    length(&mut view, "y", 30.0);
    length(&mut view, "width", 80.0);
    length(&mut view, "height", 55.0);
    view.list = Some(xrack_runtime::ListMetrics {
        row_count: 1,
        first_row: 0,
    });
    view.children.push(button("materialized-row"));
    root.children.push(view);
    let frame = layout(&root, Size::new(120.0, 100.0), &mut Text);
    let input = Interaction::default();
    assert_eq!(
        input.inspection_path(&root, &frame, Point { x: 12.0, y: 8.0 }),
        Some(vec![1])
    );
    assert_eq!(
        input.inspection_path(&root, &frame, Point { x: 12.0, y: 42.0 }),
        Some(vec![2])
    );
    assert_eq!(
        input.inspection_path(&root, &frame, Point { x: 132.0, y: 42.0 }),
        None
    );
}

#[test]
fn fixed_plain_text_boxes_skip_intrinsic_text_measurement() {
    #[derive(Default)]
    struct Measure(usize);
    impl TextMeasurer for Measure {
        fn measure(&mut self, text: &str, _: f32, _: f32) -> Size {
            self.0 += 1;
            Size::new(text.len() as f32 * 10.0, 40.0)
        }
    }

    let mut root = node("VerticalLayout", "root");
    for kind in ["Text", "Label"] {
        let mut text = Node::new(kind, None);
        length(&mut text, "width", 100.0);
        length(&mut text, "height", 24.0);
        text.properties
            .insert("text", "a long message".to_owned().into());
        root.children.push(text);
    }
    let mut measure = Measure::default();
    let viewport = Size::new(180.0, 120.0);
    let fixed = layout(&root, viewport, &mut measure);
    assert_eq!(measure.0, 0);
    for child in fixed.into_children() {
        assert_eq!((child.bounds.width, child.bounds.height), (100.0, 24.0));
    }

    root.children[0].properties.remove("height");
    let intrinsic = layout(&root, viewport, &mut measure);
    assert!(measure.0 > 0);
    assert!(intrinsic.children[0].bounds.height > 24.0);
}

#[test]
fn unchanged_repeat_rows_reuse_layout_after_the_scene_changes() {
    use std::rc::Rc;
    use xrack_runtime::{ListModel, StyledText};

    #[derive(Default)]
    struct Measured {
        placements: usize,
        measurements: usize,
        typography: Rc<()>,
        intrinsic: IntrinsicMeasurements,
    }
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            self.measurements += 1;
            Text.measure(text, size, width)
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.typography.clone())
        }

        fn intrinsic_measurements(&mut self) -> Option<&mut IntrinsicMeasurements> {
            Some(&mut self.intrinsic)
        }

        fn layout_styled_text(&mut self, node: &Node, available: Size) -> Option<StyledTextLayout> {
            self.placements += usize::from(available.height.is_finite());
            Text.layout_styled_text(node, available)
        }
    }
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let row = |index, text: &str| {
        let mut message = node("StyledText", "message");
        length(&mut message, "font-size", 10.0);
        length(&mut message, "width", 140.0);
        length(&mut message, "height", 24.0);
        message
            .properties
            .insert("text", StyledText::from_plain_text(text).into());
        message.set_row_key(Some(3_i64.row_key(index)));
        message.into_shared()
    };
    let mut base = node("VerticalLayout", "entries");
    for index in 0..3 {
        base.children.push(row(index, "History"));
    }
    let viewport = Size::new(140.0, 96.0);
    let mut text = Measured::default();
    let mut cache = RowLayoutCache::default();
    let input = Interaction::default();
    prepare_scene_with_row_cache(
        &View,
        Rc::new(base.clone()),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    assert_eq!(text.placements, 3);

    text.placements = 0;
    base.children[1] = row(1, "A reply spanning more than one line");
    let scene = Rc::new(base);
    let (_, retained) = prepare_scene_with_row_cache(
        &View,
        scene.clone(),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, direct) = prepare_scene_with_input(&View, scene.clone(), viewport, &mut Text, &input);
    assert_eq!(retained, direct);
    assert_eq!(text.placements, 1);

    text.placements = 0;
    text.typography = Rc::new(());
    text.intrinsic = IntrinsicMeasurements::default();
    let (_, relaid) =
        prepare_scene_with_row_cache(&View, scene, viewport, &mut text, &input, &mut cache);
    assert_eq!(relaid, direct);
    assert_eq!(text.placements, 3);

    let mut proportional = row(0, "Measured at two viewport heights");
    let small = Size::new(140.0, 100.0);
    let large = Size::new(140.0, 300.0);
    let fixed = linear::measure(&proportional, small, &mut text);
    let measurements = text.measurements;
    let taller_viewport = linear::measure(&proportional, large, &mut text);
    assert_eq!(fixed.y.preferred, taller_viewport.y.preferred);
    assert_eq!(text.measurements, measurements);

    proportional
        .properties
        .insert("height", xrack_runtime::Percent(0.5).into());
    assert_eq!(
        linear::measure(&proportional, small, &mut text).y.preferred,
        50.0
    );
    assert_eq!(
        linear::measure(&proportional, large, &mut text).y.preferred,
        150.0
    );
}

#[test]
fn animated_groups_reuse_local_layout_and_follow_current_clipping() {
    use xrack_runtime::{Animation, Easing, StyledText};

    #[derive(Default)]
    struct Measured {
        placements: usize,
        typography: Rc<()>,
    }
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            Text.measure(text, size, width)
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.typography.clone())
        }

        fn layout_styled_text(&mut self, node: &Node, available: Size) -> Option<StyledTextLayout> {
            self.placements += usize::from(available.height.is_finite());
            Text.layout_styled_text(node, available)
        }
    }
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let mut message = node("StyledText", "message");
    length(&mut message, "height", 21.0);
    let mut column = node("VerticalLayout", "content");
    length(&mut column, "height", 48.0);
    let mut status = node("Text", "status");
    length(&mut status, "width", 80.0);
    length(&mut status, "height", 14.0);
    status.mark_paint_only("text");
    column.children.extend([message, button("button"), status]);
    let mut group = node("Rectangle", "group");
    length(&mut group, "height", 52.0);
    group.children.push(column);
    let mut window = node("Window", "root");
    window.children.push(group);
    let mut base = Rc::new(window);
    let viewport = Size::new(180.0, 104.0);
    let input = Interaction::default();
    let mut text = Measured::default();
    let mut cache = RowLayoutCache::default();

    for (x, width, opacity, caption, new_fonts, measured) in [
        (8.0, 104.0, 0.7, "Load", false, true),
        (33.5, 104.0, 0.6, "Load", false, false),
        (-8.0, 104.0, 0.8, "Load", false, false),
        (12.0, 96.0, 0.8, "Load", false, true),
        (14.0, 96.0, 0.8, "Changed title", false, true),
        (16.0, 96.0, 0.8, "Changed title", true, true),
        (26.0, 96.0, 1.0, "Changed title", false, false),
        (31.5, 96.0, 1.0, "Changed title", false, false),
    ] {
        let group = &mut Rc::make_mut(&mut base).children[0];
        length(group, "x", x);
        length(group, "width", width);
        group.properties.insert("opacity", opacity.into());
        if opacity == 1.0 {
            group.set_animation("x", Animation::new(200, 0, Easing::LINEAR));
        }
        group.children[0].children[0]
            .properties
            .insert("text", StyledText::from_plain_text(caption).into());
        group.children[0].children[2]
            .properties
            .insert("text", format!("Frame at {x}").into());
        if new_fonts {
            text.typography = Rc::new(());
        }
        let before = text.placements;
        let (scene, retained) = prepare_scene_with_row_cache(
            &View,
            base.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, direct) =
            prepare_scene_with_input(&View, base.clone(), viewport, &mut Text, &input);
        assert!(Rc::ptr_eq(&scene, &base));
        assert_eq!(retained, direct, "group moved to {x}");
        assert_eq!(text.placements > before, measured, "group moved to {x}");
    }
}

#[test]
fn cached_local_and_row_layouts_attach_the_current_rounded_ancestors() {
    use xrack_runtime::ListModel;

    #[derive(Default)]
    struct Metrics(Rc<()>);
    impl TextMeasurer for Metrics {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            Text.measure(text, size, width)
        }
        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.0.clone())
        }
    }
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let mut outer = node("Rectangle", "outer");
    let mut inner = node("Rectangle", "inner");
    for (panel, width, radius) in [(&mut outer, 160.0, 12.0), (&mut inner, 148.0, 5.0)] {
        boolean(panel, "clip", true);
        length(panel, "width", width);
        length(panel, "height", 92.0);
        length(panel, "border-radius", radius);
    }
    let mut group = node("Rectangle", "group");
    length(&mut group, "width", 80.0);
    length(&mut group, "height", 60.0);
    group.properties.insert("opacity", 0.7_f64.into());
    let mut row = node("ListRow", "row");
    row.set_row_key(Some(3_i64.row_key(0)));
    length(&mut row, "x", 40.0);
    length(&mut row, "y", 62.0);
    length(&mut row, "width", 90.0);
    length(&mut row, "height", 24.0);
    for y in [8.0, 24.0, 40.0] {
        let mut label = Node::new("Label", None);
        length(&mut label, "y", y);
        length(&mut label, "width", 52.0);
        length(&mut label, "height", 18.0);
        label.properties.insert("text", "Status".to_owned().into());
        group.children.push(label.clone());
        row.children.push(label);
    }
    inner.children.extend([group, row]);
    outer.children.push(inner);
    let mut window = node("Window", "root");
    window.children.push(outer);
    let mut scene = Rc::new(window);
    let viewport = Size::new(180.0, 104.0);
    let input = Interaction::default();
    let mut cache = RowLayoutCache::default();
    let mut metrics = Metrics::default();

    for (frame, (x, radius)) in [
        (8.125, 12.0),
        (27.625, 12.0),
        (-14.375, 32.0),
        (12.875, 12.0),
    ]
    .into_iter()
    .enumerate()
    {
        let outer = &mut Rc::make_mut(&mut scene).children[0];
        length(outer, "border-radius", radius);
        length(&mut outer.children[0].children[0], "x", x);
        let before = cache.stats();
        let (_, retained) = prepare_scene_with_row_cache(
            &View,
            scene.clone(),
            viewport,
            &mut metrics,
            &input,
            &mut cache,
        );
        let (_, fresh) =
            prepare_scene_with_input(&View, scene.clone(), viewport, &mut Text, &input);
        assert_eq!(retained, fresh, "moving group at x={x}");
        let group = retained.at_path(&[0, 0, 0]).unwrap();
        assert!(group.children[0].has_ancestor_clip_chain());
        assert_eq!(
            group.contains_clip_point(Point { x: 6.0, y: 6.0 }),
            radius == 12.0
        );
        let after = cache.stats();
        if frame != 0 {
            assert!(after.subtree_reuses > before.subtree_reuses);
            assert_eq!(after.reused - before.reused, 1);
            assert_eq!(after.calculated, before.calculated);
        }
    }
}

#[test]
fn preparing_virtual_rows_keeps_unrelated_shared_branches_intact() {
    use xrack_runtime::{ListRowRenderer, SceneFeature};

    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let mut content = node("VerticalLayout", "history");
    content.children.push(button("message"));
    let content = content.into_shared();
    assert!(!content.contains_scene_feature(SceneFeature::VirtualList));

    let mut list = node("ListView", "list");
    list.list = Some(ListMetrics {
        row_count: 4,
        first_row: 0,
    });
    length(&mut list, "height", 48.0);
    length(&mut list, "row-height", 24.0);
    list.list_row_renderer = Some(ListRowRenderer::new(|range| {
        range.map(|_| button("virtual-row")).collect()
    }));
    let mut panel = content.clone();
    panel.children.push(list);
    assert!(panel.contains_scene_feature(SceneFeature::VirtualList));
    assert!(!content.contains_scene_feature(SceneFeature::VirtualList));

    let mut window = node("Window", "window");
    window.children.extend([content.clone(), panel]);
    let (scene, layout) = prepare_scene_with_input(
        &View,
        Rc::new(window),
        Size::new(180.0, 144.0),
        &mut Text,
        &Interaction::default(),
    );
    assert!(scene.children[0].shares_storage_with(&content));
    let list = &scene.children[1].children[1];
    assert_eq!(list.children.len(), 2);
    assert_eq!(
        list.children[0].children[0].element_id(),
        Some("virtual-row")
    );
    assert!(layout.at_path(&[1, 1, 0, 0]).is_some());
}

#[test]
fn scroll_measurements_reuse_unchanged_children_and_recompute_the_extent() {
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    #[derive(Default)]
    struct Measured {
        calls: usize,
        typography: Rc<()>,
    }
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            self.calls += 1;
            Text.measure(text, size, width)
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.typography.clone())
        }
    }

    let mut view = node("ScrollView", "viewport");
    let mut stretch = node("Rectangle", "stretch");
    length(&mut stretch, "width", 2.0);
    view.children.push(stretch);
    for index in 0..66 {
        let mut label = node("Label", "history");
        length(&mut label, "y", f64::from(index) * 20.0);
        label
            .properties
            .insert("width", xrack_runtime::Percent(0.8).into());
        label
            .properties
            .insert("text", "Measured text".to_owned().into());
        view.children.push(label);
    }
    let mut tail = node("Rectangle", "tail");
    length(&mut tail, "y", 1320.0);
    length(&mut tail, "width", 54.0);
    length(&mut tail, "height", 20.0);
    view.children.push(tail);
    let mut window = node("Window", "root");
    window.children.push(view);
    let mut scene = Rc::new(window.into_shared());
    let viewport = Size::new(140.0, 100.0);
    let input = Interaction::default();
    let mut text = Measured::default();
    let mut cache = RowLayoutCache::default();
    let mut previous = None;
    for _ in 0..2 {
        let (_, layout) = prepare_scene_with_row_cache(
            &View,
            scene.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        previous = Some(layout);
    }
    assert!(text.calls > 0);

    for (height, z) in [(90.0, -1.0_f64), (40.0, -1.0), (12.0, 0.0)] {
        let view = &mut Rc::make_mut(&mut scene).children[0];
        let tail = view.children.last_mut().unwrap();
        length(tail, "height", height);
        tail.properties.insert("z", z.into());
        let layout = previous.take().unwrap();
        let old_buffer = layout.children[0].children.as_ptr();
        cache.use_recycled_layout(Some(layout));
        text.calls = 0;
        let (_, cached) = prepare_scene_with_row_cache(
            &View,
            scene.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, fresh) =
            prepare_scene_with_input(&View, scene.clone(), viewport, &mut Text, &input);
        assert_eq!(cached, fresh);
        assert_eq!(
            cached.children[0].paint_order().map(|order| order[0]),
            (z < 0.0).then_some(67),
        );
        assert_eq!(cached.children[0].children.as_ptr(), old_buffer);
        assert_eq!(text.calls, 0);
        previous = Some(cached);
    }

    let narrower = Size::new(86.0, viewport.height);
    let (_, cached) = prepare_scene_with_row_cache(
        &View,
        scene.clone(),
        narrower,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, fresh) = prepare_scene_with_input(&View, scene, narrower, &mut Text, &input);
    assert_eq!(cached, fresh);
}

#[test]
fn passive_scroll_content_repositions_without_measuring_and_updates_after_resize() {
    use xrack_runtime::{Image, ImageFormat};

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    #[derive(Default)]
    struct Measured(usize, Rc<()>);
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            self.0 += 1;
            Text.measure(text, size, width)
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.1.clone())
        }
    }

    let mut view = node("ScrollView", "viewport");
    let mut column = node("VerticalLayout", "content");
    length(&mut column, "spacing", 2.0);
    let source = |size| Image::Embedded {
        format: ImageFormat::Png,
        bytes: &[],
        size,
    };
    let mut image = node("Image", "thumbnail");
    image.properties.insert("source", source([24, 16]).into());
    column.children.push(image);
    for _ in 0..16 {
        let mut label = node("StyledText", "message");
        length(&mut label, "font-size", 10.0);
        label.properties.insert(
            "text",
            xrack_runtime::StyledText::from_markdown("A long message with a [link](file).")
                .unwrap()
                .into(),
        );
        column.children.push(label);
    }
    view.children.push(column);
    let component = View(view);
    let base = component.render_shared();
    let mut input = Interaction::default();
    let mut cache = RowLayoutCache::default();
    let mut text = Measured::default();
    let viewport = Size::new(160.0, 86.0);
    let (_, initial) = prepare_scene_with_row_cache(
        &component,
        base.clone(),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    assert!(text.0 > 0);
    text.0 = 0;
    assert!(input.accessibility_scroll_to(&initial, &[], Orientation::Vertical, 48.0));
    let (_, after_scroll) = prepare_scene_with_row_cache(
        &component,
        base.clone(),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, expected) =
        prepare_scene_with_input(&component, base.clone(), viewport, &mut Text, &input);
    assert_eq!(after_scroll, expected);
    assert_eq!(text.0, 0);

    let replacement = Image::Embedded {
        format: ImageFormat::Png,
        bytes: b"new pixels with the same dimensions",
        size: [24, 16],
    };
    let mut updated = base.clone();
    Rc::make_mut(&mut updated).children[0].children[0]
        .properties
        .insert("source", replacement.clone().into());
    let (prepared, reused) = prepare_scene_with_row_cache(
        &component,
        updated.clone(),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    assert_eq!(reused, after_scroll);
    assert_eq!(text.0, 0);
    assert_eq!(
        prepared.children[0].children[0].image("source"),
        Some(replacement)
    );

    Rc::make_mut(&mut updated).children[0].children[0]
        .properties
        .insert("source", source([24, 28]).into());
    let (_, remeasured) = prepare_scene_with_row_cache(
        &component,
        updated.clone(),
        viewport,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, expected) = prepare_scene_with_input(&component, updated, viewport, &mut Text, &input);
    assert_eq!(remeasured, expected);
    assert_ne!(
        remeasured.children[0].children[0].bounds.height,
        reused.children[0].children[0].bounds.height,
    );

    let narrow = Size::new(112.0, 86.0);
    let (_, resized) = prepare_scene_with_row_cache(
        &component,
        base.clone(),
        narrow,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, expected) = prepare_scene_with_input(&component, base, narrow, &mut Text, &input);
    assert_eq!(resized, expected);
    assert!(text.0 > 0);

    let mut current = Rc::new(component.0.clone());
    Rc::make_mut(&mut current).children[0].children[0]
        .properties
        .insert("source", source([32, 28]).into());
    for spacing in [3.0, 6.0] {
        length(
            &mut Rc::make_mut(&mut current).children[0],
            "spacing",
            spacing,
        );
        let (_, changed) = prepare_scene_with_row_cache(
            &component,
            current.clone(),
            narrow,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, direct) =
            prepare_scene_with_input(&component, current.clone(), narrow, &mut Text, &input);
        assert_eq!(changed, direct);
        assert_ne!(
            changed.at_path(&[0, 0]).unwrap().bounds.height,
            resized.at_path(&[0, 0]).unwrap().bounds.height,
        );
    }
    let (_, stable) = prepare_scene_with_row_cache(
        &component,
        current.clone(),
        narrow,
        &mut text,
        &input,
        &mut cache,
    );
    text.0 = 0;
    assert!(input.accessibility_scroll_to(&stable, &[], Orientation::Vertical, 68.0));
    let (_, scrolled) = prepare_scene_with_row_cache(
        &component,
        current.clone(),
        narrow,
        &mut text,
        &input,
        &mut cache,
    );
    let (_, direct) =
        prepare_scene_with_input(&component, current.clone(), narrow, &mut Text, &input);
    assert_eq!(scrolled, direct);
    assert_eq!(text.0, 0);

    let viewport = Rc::make_mut(&mut current);
    viewport.properties.insert(
        "layout-direction",
        xrack_runtime::LayoutDirection::RightToLeft.into(),
    );
    length(viewport, "content-width", 250.0);
    let mut sibling = node("Rectangle", "tail");
    length(&mut sibling, "x", 104.0);
    length(&mut sibling, "y", 78.0);
    length(&mut sibling, "width", 28.0);
    length(&mut sibling, "height", 32.0);
    viewport.children.extend([Node::hidden(), sibling]);
    let mut last = scrolled;
    for offset in [0.0, 32.25, 58.0] {
        if offset > 0.0 {
            assert!(input.accessibility_scroll_to(&last, &[], Orientation::Horizontal, offset));
        }
        let (_, cached) = prepare_scene_with_row_cache(
            &component,
            current.clone(),
            narrow,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, fresh) =
            prepare_scene_with_input(&component, current.clone(), narrow, &mut Text, &input);
        assert_eq!(cached, fresh);
        assert!(!cached.children[1].visible);
        last = cached;
    }
}

#[test]
fn clipped_scroll_measurements_survive_unrelated_edits_and_viewport_motion() {
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    struct Measured {
        calls: usize,
        revision: Rc<()>,
    }
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            self.calls += 1;
            Text.measure(text, size, width)
        }

        fn typography_revision(&self) -> Option<Rc<()>> {
            Some(self.revision.clone())
        }
    }

    let mut window = node("Window", "root");
    let mut panel = node("Rectangle", "panel");
    length(&mut panel, "width", 132.0);
    length(&mut panel, "height", 116.0);
    length(&mut panel, "border-radius", 24.0);
    boolean(&mut panel, "clip", true);
    let mut view = node("Flickable", "view");
    view.properties
        .insert("width", Value::Percent(xrack_runtime::Percent(1.0)));
    length(&mut view, "height", 80.0);
    length(&mut view, "content-height", 180.0);
    let mut label = node("Label", "label");
    length(&mut label, "width", 86.0);
    length(&mut label, "y", 45.0);
    label
        .properties
        .insert("text", "A measured label".to_owned().into());
    let mut progress = node("ProgressIndicator", "progress");
    length(&mut progress, "width", 60.0);
    length(&mut progress, "height", 8.0);
    let mut nested = node("Flickable", "nested");
    for (name, value) in [
        ("x", 12.0),
        ("y", 24.0),
        ("width", 68.0),
        ("height", 26.0),
        ("content-height", 60.0),
    ] {
        length(&mut nested, name, value);
    }
    nested.children.push(label.clone());
    let mut trailing = label.clone();
    length(&mut trailing, "y", 85.0);
    view.children.extend([label, nested, progress, trailing]);
    panel.children.push(view);
    window
        .children
        .extend([panel, node("Rectangle", "outside")]);

    let mut scene = Rc::new(window);
    let viewport = Size::new(220.0, 150.0);
    let mut cache = RowLayoutCache::default();
    let mut text = Measured {
        calls: 0,
        revision: Rc::new(()),
    };
    let mut input = Interaction::default();
    let mut previous = None;
    for (frame, measures) in [true, false, false, false, true, true, true]
        .into_iter()
        .enumerate()
    {
        match frame {
            1 => assert!(input.accessibility_scroll_to(
                previous.as_ref().unwrap(),
                &[0, 0],
                Orientation::Vertical,
                30.0,
            )),
            2 => length(&mut Rc::make_mut(&mut scene).children[1], "x", 39.0),
            3 => length(&mut Rc::make_mut(&mut scene).children[0], "y", 11.0),
            4 => length(&mut Rc::make_mut(&mut scene).children[0], "width", 142.0),
            5 => text.revision = Rc::new(()),
            6 => {
                Rc::make_mut(&mut scene).children[0].children[0].children[0]
                    .properties
                    .insert("text", "A different label".to_owned().into());
            }
            _ => (),
        }
        text.calls = 0;
        let (_, retained) = prepare_scene_with_row_cache(
            &View,
            scene.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let (_, expected) =
            prepare_scene_with_input(&View, scene.clone(), viewport, &mut Text, &input);
        assert_eq!(retained, expected, "frame={frame}");
        assert_eq!(
            text.calls > 0,
            measures,
            "frame={frame}, measured {}",
            text.calls
        );
        previous = Some(retained);
    }
}

#[test]
fn scrolling_reuses_unchanged_row_layouts_and_recomputes_entering_rows() {
    use xrack_runtime::{ListRowRenderer, VecModel};

    #[derive(Default)]
    struct Measured(usize);
    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, size: f32, width: f32) -> Size {
            self.0 += 1;
            Text.measure(text, size, width)
        }
    }

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let records = VecModel::from(
        (0..50_000)
            .map(|row| format!("Record {row}"))
            .collect::<Vec<_>>(),
    );
    let model = records.clone();
    let mut list = node("ListView", "list");
    list.list = Some(ListMetrics {
        row_count: records.len(),
        first_row: 0,
    });
    length(&mut list, "row-height", 24.0);
    list.list_row_renderer = Some(ListRowRenderer::new(move |range| {
        range
            .map(|index| {
                let mut row = Node::new("HorizontalLayout", None);
                row.row_key = model.row_key(index);
                let mut label = Node::new("Text", None);
                label
                    .properties
                    .insert("text", model.row(index).unwrap().into());
                row.children.extend([label.clone(), label]);
                row
            })
            .collect()
    }));
    let view = View(list);
    let base = view.render_shared();
    let viewport = Size::new(240.0, 96.0);
    let mut input = Interaction::default();
    let mut cache = RowLayoutCache::default();
    let mut text = Measured::default();
    let mut baseline = Measured::default();
    let mut layout = layout_with_input(&base, viewport, &mut Text, &input);
    for offset in [2.0, 4.0, 26.0] {
        assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Vertical, offset));
        let (scene, next) = prepare_scene_with_row_cache(
            &view,
            base.clone(),
            viewport,
            &mut text,
            &input,
            &mut cache,
        );
        let expected =
            prepare_scene_with_input(&view, base.clone(), viewport, &mut baseline, &input);
        assert_eq!(*scene, *expected.0);
        assert_eq!(next, expected.1);
        layout = next;
    }
    let stats = cache.stats();
    assert_eq!((stats.calculated, stats.reused), (6, 9));
    assert!(text.0 > 0 && text.0 < baseline.0);
    eprintln!(
        "scroll text measurements with cache: {}, without: {}",
        text.0, baseline.0
    );

    assert!(records.set_row(1, "Updated".into()));
    let (_, actual) =
        prepare_scene_with_row_cache(&view, base.clone(), viewport, &mut text, &input, &mut cache);
    let (_, expected) =
        prepare_scene_with_input(&view, base.clone(), viewport, &mut baseline, &input);
    assert_eq!(actual, expected);
    assert_eq!(cache.stats().calculated, 7);

    let mut rtl = (*base).clone();
    rtl.properties.insert(
        "layout-direction",
        xrack_runtime::LayoutDirection::RightToLeft.into(),
    );
    let rtl = std::rc::Rc::new(rtl);
    let (_, cached) =
        prepare_scene_with_row_cache(&view, rtl.clone(), viewport, &mut text, &input, &mut cache);
    let (_, expected) = prepare_scene_with_input(&view, rtl, viewport, &mut baseline, &input);
    assert_eq!(cached, expected);
    let row = cached.at_path(&[0, 0]).unwrap();
    assert!(row.children[0].bounds.x > row.children[1].bounds.x);
}

#[test]
fn wide_table_stores_only_the_materialized_scene_and_layout_cells() {
    use std::rc::Rc;
    use xrack_runtime::{
        ListRowRenderer, TableColumnWindow, TableHeader, TableRowOrder, VecModel,
        project_standard_table_rows,
    };

    const COLUMNS: usize = 4096;
    let records = Rc::new(VecModel::from((0..50_000).collect::<Vec<usize>>()));
    let rows = TableRowOrder::default().rows(records);
    let mut table = node("TableView", "wide");
    table.properties.insert("frozen-columns", 1_i64.into());
    length(&mut table, "row-height", 24.0);
    table.list = Some(ListMetrics {
        row_count: 50_000,
        first_row: 0,
    });
    let mut column = Node::new("TableColumn", None);
    length(&mut column, "width", 80.0);
    table.table_header = Some(Box::new(TableHeader {
        columns: vec![column.into_shared(); COLUMNS],
        sort: None,
        column_widths: Vec::new(),
    }));
    table.list_row_renderer = Some(ListRowRenderer::with_columns(
        move |range, columns: &TableColumnWindow| {
            let mut projected = project_standard_table_rows(
                &rows,
                range,
                columns,
                COLUMNS,
                |_| None,
                |row, column| format!("{row}:{column}"),
                |_| None,
            );
            for row in &mut projected {
                for cell in &mut row.children {
                    let mut label = Node::new("Text", None);
                    label
                        .properties
                        .insert("text", cell.text("text").unwrap().to_owned().into());
                    cell.children.push(label);
                }
            }
            projected
        },
    ));

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let view = View(table);
    let viewport = Size::new(330.0, 200.0);
    let mut input = Interaction::default();
    let (scene, layout) =
        prepare_scene_with_input(&view, view.render_shared(), viewport, &mut Text, &input);
    assert_eq!(scene.children[0].children[0].children.len(), 5);
    assert_eq!(layout.children[0].children[0].children.len(), 5);
    let max = layout.table.as_ref().unwrap().horizontal.max_offset;
    assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Horizontal, max));
    let (scene, layout) =
        prepare_scene_with_input(&view, view.render_shared(), viewport, &mut Text, &input);
    let row = scene.at_path(&[0, 0]).unwrap();
    let placed = layout.at_path(&[0, 0]).unwrap();
    let geometry = layout.table.as_ref().unwrap();
    let candidates: Vec<_> = geometry.columns_in_viewport().collect();
    assert_eq!(candidates.first().copied(), Some(0));
    assert_eq!(candidates.last().copied(), Some(COLUMNS - 1));
    assert!(candidates.len() < 8);
    assert_eq!(row.children.len(), 5);
    assert_eq!(placed.children.len(), 5);
    assert_eq!(
        row.table_column_window.logical_len(row.children.len()),
        COLUMNS
    );
    assert_eq!(
        row.child_at(COLUMNS - 1).unwrap().text("text"),
        Some("0:4095")
    );
    assert!(row.child_at(1).is_none());
    assert!(placed.child_at(1).is_none());
    assert!(placed.child_at(COLUMNS - 1).is_some());

    let base = view.render_shared();
    let mut cache = RowLayoutCache::default();
    let mut layout = layout;
    let mut measurements = Vec::new();
    for offset in [2.0, 4.0, 82.0] {
        assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Horizontal, offset));
        let (scene, next) = prepare_scene_with_row_cache(
            &view,
            base.clone(),
            viewport,
            &mut Text,
            &input,
            &mut cache,
        );
        let expected = prepare_scene_with_input(&view, base.clone(), viewport, &mut Text, &input);
        assert_eq!(*scene, *expected.0);
        assert_eq!(next, expected.1);
        assert_eq!(
            next.table.as_ref().unwrap().columns[0],
            layout.table.as_ref().unwrap().columns[0]
        );
        measurements.push((scene.children.len() as u64, cache.stats()));
        layout = next;
    }
    let (rows, first) = measurements[0];
    let (_, same_columns) = measurements[1];
    let (_, next_column) = measurements[2];
    assert_eq!(first.calculated, rows * 5);
    assert_eq!(same_columns.calculated, first.calculated);
    assert_eq!(same_columns.reused, rows * 5);
    assert_eq!(next_column.calculated - same_columns.calculated, rows);
    assert_eq!(next_column.reused - same_columns.reused, rows * 4);

    let columns_before = layout.table.clone();
    let mut retained: Option<Rc<Node>> = None;
    let mut released_header = None;
    for (first_row, offset) in [24.0, 48.0, 72.0].into_iter().enumerate() {
        assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Vertical, offset));
        let (scene, next) = prepare_scene_with_row_cache(
            &view,
            base.clone(),
            viewport,
            &mut Text,
            &input,
            &mut cache,
        );
        let (expected, expected_layout) =
            prepare_scene_with_input(&view, base.clone(), viewport, &mut Text, &input);
        assert_eq!(scene.list.unwrap().first_row, first_row + 1);
        assert_eq!(*scene, *expected);
        assert_eq!(next, expected_layout);
        assert_eq!(next.table, columns_before);
        let header = scene.table_header.as_ref().unwrap().columns.as_ptr();
        if first_row == 1 {
            let held = retained.take().unwrap();
            assert_eq!(held.list.unwrap().first_row, 1);
            let held_header = held.table_header.as_ref().unwrap().columns.as_ptr();
            assert_ne!(held_header, header);
            released_header = Some(held_header);
        } else if first_row == 2 {
            assert_eq!(Some(header), released_header);
        }
        retained = Some(scene);
        layout = next;
    }

    let mut base = base;
    Rc::make_mut(&mut base)
        .table_header
        .as_mut()
        .unwrap()
        .columns[0]
        .properties
        .insert("width", xrack_runtime::Percent(0.25).into());
    for width in [330.0, 480.0] {
        let viewport = Size::new(width, viewport.height);
        let (_, layout) = prepare_scene_with_row_cache(
            &view,
            base.clone(),
            viewport,
            &mut Text,
            &input,
            &mut cache,
        );
        let table = layout.table.as_ref().unwrap();
        assert_eq!(table.columns[0].width, width * 0.25);
        assert!(table.is_frozen(0));
    }
    Rc::make_mut(&mut base)
        .table_header
        .as_mut()
        .unwrap()
        .column_widths = vec![Some(156)];
    let viewport = Size::new(480.0, viewport.height);
    let (scene, resized) =
        prepare_scene_with_row_cache(&view, base.clone(), viewport, &mut Text, &input, &mut cache);
    let expected = prepare_scene_with_input(&view, base.clone(), viewport, &mut Text, &input);
    assert_eq!(*scene, *expected.0);
    assert_eq!(resized, expected.1);
    let geometry = resized.table.as_ref().unwrap();
    assert_eq!(geometry.columns[0].width, 156.0);
    assert!(geometry.is_frozen(0));
    boolean(
        &mut Rc::make_mut(&mut base)
            .table_header
            .as_mut()
            .unwrap()
            .columns[0],
        "visible",
        false,
    );
    let (_, layout) =
        prepare_scene_with_row_cache(&view, base, viewport, &mut Text, &input, &mut cache);
    let table = layout.table.as_ref().unwrap();
    assert_eq!(table.columns[0].width, 0.0);
    assert!(table.is_frozen(1));
}

#[test]
fn vertical_split_hit_chooses_the_nearest_handle_inside_ancestor_clips() {
    let mut root = node("Window", "root");
    let mut clip = node("Rectangle", "clip");
    length(&mut clip, "width", 100.0);
    length(&mut clip, "height", 96.0);
    boolean(&mut clip, "clip", true);
    let mut split = node("SplitView", "split");
    split
        .properties
        .insert("orientation", xrack_runtime::Orientation::Vertical.into());
    length(&mut split, "width", 100.0);
    length(&mut split, "height", 120.0);
    length(&mut split, "handle-hit-size", 64.0);
    for id in ["top", "middle", "bottom"] {
        let mut pane = node("Rectangle", id);
        length(&mut pane, "preferred-height", 34.0);
        split.children.push(pane);
    }
    split.add_split_handles();
    clip.children.push(split);
    root.children.push(clip);

    let frame = layout(&root, Size::new(160.0, 140.0), &mut Text);
    let first = [0, 0, 3];
    let second = [0, 0, 4];
    let top = frame.at_path(&first).unwrap().bounds;
    let bottom = frame.at_path(&second).unwrap().bounds;
    let mid = (top.y + bottom.y + top.height) * 0.5;
    let mut input = Interaction::default();
    for (y, path) in [(mid - 2.0, first), (mid + 2.0, second)] {
        let point = Point { x: 50.0, y };
        assert!(!top.contains(point) && !bottom.contains(point));
        input.pointer_move(&root, &frame, point);
        assert!(input.is_hovered(&path));
    }
    let outside = Point { x: 50.0, y: 100.0 };
    assert!((outside.y - bottom.y - bottom.height * 0.5).abs() < 32.0);
    assert!(!input.touch_split_press(&root, &frame, outside));
}

#[test]
fn fractional_interior_flick_keeps_inertia_without_elastic_residue() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{FlickBoundsBehavior, Orientation, ViewportHandle};
    let handle = ViewportHandle::for_flickable(false);
    let mut root = node("Flickable", "view");
    root.viewport_handle = Some(handle.clone());
    root.properties.insert(
        "bounds-behavior",
        FlickBoundsBehavior::DragAndOvershootBounds.into(),
    );
    length(&mut root, "content-height", 600.0);
    let viewport = Size::new(120.0, 100.0);
    let mut input = Interaction::default();
    let frame = layout(&root, viewport, &mut Text);
    assert!(input.set_viewport_offset(&[], &frame, Orientation::Vertical, 130.0));
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    let now = Instant::now();
    let start = Point { x: 50.0, y: 80.0 };
    let end = Point { x: 50.0, y: 62.13 };
    assert!(input.begin_scroll_gesture(&root, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&root, &frame, end, now + Duration::from_millis(20)));
    assert_eq!(handle.vertical_overshoot(), Length(0.0));
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    input.end_scroll_gesture(&root, &frame, end, now + Duration::from_millis(25));
    assert!(handle.flicking());
    assert!(input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(60)));
    assert!(handle.flicking());
    assert_eq!(handle.vertical_overshoot(), Length(0.0));
}

#[test]
fn elastic_drag_bubbles_first_and_rebounds_on_the_outer_viewport() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{FlickBoundsBehavior, ViewportHandle};

    let outer = ViewportHandle::for_flickable(true);
    let inner = ViewportHandle::for_flickable(true);
    let mut root = node("Flickable", "outer");
    root.viewport_handle = Some(outer.clone());
    root.properties.insert(
        "bounds-behavior",
        FlickBoundsBehavior::DragAndOvershootBounds.into(),
    );
    length(&mut root, "content-height", 190.0);
    let mut child = node("Flickable", "inner");
    child.viewport_handle = Some(inner.clone());
    child.properties.insert(
        "bounds-behavior",
        FlickBoundsBehavior::DragAndOvershootBounds.into(),
    );
    for (name, value) in [
        ("x", 10.0),
        ("y", 20.0),
        ("width", 120.0),
        ("height", 80.0),
        ("content-height", 100.0),
    ] {
        length(&mut child, name, value);
    }
    root.children.push(child);
    let viewport = Size::new(240.0, 160.0);
    let frame = layout(&root, viewport, &mut Text);
    let mut input = Interaction::default();
    let now = Instant::now();
    let start = Point { x: 50.0, y: 65.0 };
    let end = Point { x: 50.0, y: -25.0 };
    assert!(input.begin_scroll_gesture(&root, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&root, &frame, end, now + Duration::from_millis(20)));
    assert_eq!(input.scroll_offsets()[&[0][..]], 20.0);
    assert_eq!(input.scroll_offsets()[&[][..]], 30.0);
    assert_eq!(inner.vertical_overshoot(), Length(0.0));
    let peak = outer.vertical_overshoot().0;
    assert!(peak > 0.0);

    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    let reverse = Point { x: 50.0, y: -12.0 };
    assert!(input.move_scroll_gesture(&root, &frame, reverse, now + Duration::from_millis(30)));
    assert_eq!(input.scroll_offsets()[&[0][..]], 20.0);
    assert_eq!(input.scroll_offsets()[&[][..]], 30.0);
    let remaining = outer.vertical_overshoot().0;
    assert!(remaining > 0.0 && remaining < peak);
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    input.end_scroll_gesture(&root, &frame, reverse, now + Duration::from_millis(35));
    assert!(outer.rebounding() && outer.moving() && !outer.flicking());
    assert!(!inner.moving());
    assert!(input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(100)));
    assert!(outer.vertical_overshoot().0 < remaining);
    let held = outer.vertical_overshoot();
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    let grab = Point { x: 180.0, y: 90.0 };
    assert!(input.begin_scroll_gesture(
        &root,
        &frame,
        grab,
        now + Duration::from_millis(110),
        ScrollGestureKind::Touch
    ));
    assert_eq!(outer.vertical_overshoot(), held);
    assert!(!outer.rebounding());
    assert!(
        !input
            .end_scroll_gesture(&root, &frame, grab, now + Duration::from_millis(120))
            .tap
    );
    assert!(outer.rebounding());
    assert!(input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(340)));
    assert_eq!(outer.vertical_overshoot(), Length(0.0));
    assert!(!outer.rebounding() && !outer.moving() && !input.scroll_motion_active());
    assert_eq!(input.scroll_offsets()[&[][..]], 30.0);
}

#[test]
fn rtl_flickable_keeps_the_physical_elastic_motion_and_reports_logical_overshoot() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{FlickBoundsBehavior, LayoutDirection, Orientation, ViewportHandle};

    let handle = ViewportHandle::for_flickable(true);
    let mut view = node("Flickable", "rtl");
    view.viewport_handle = Some(handle.clone());
    view.properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    view.properties.insert(
        "bounds-behavior",
        FlickBoundsBehavior::DragOverBounds.into(),
    );
    length(&mut view, "content-width", 480.0);
    let mut mark = node("Rectangle", "mark");
    length(&mut mark, "x", 400.0);
    length(&mut mark, "width", 80.0);
    view.children.push(mark);

    let size = Size::new(160.0, 100.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let x = frame.children[0].bounds.x;
    assert_eq!(x, 80.0);
    let start = Point { x: 90.0, y: 50.0 };
    let end = Point { x: 40.0, y: 50.0 };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&view, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&view, &frame, end, now + Duration::from_millis(20)));
    assert!(handle.horizontal_overshoot().0 < 0.0);
    let dragged = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(
        dragged.scroll_on(Orientation::Horizontal).unwrap().offset,
        0.0
    );
    assert!(dragged.children[0].bounds.x < x);

    input.end_scroll_gesture(&view, &dragged, end, now + Duration::from_millis(40));
    assert!(handle.rebounding());
    assert!(input.advance_scroll_motion(&view, &dragged, now + Duration::from_millis(250)));
    assert_eq!(handle.horizontal_overshoot(), Length(0.0));
    assert_eq!(
        layout_with_input(&view, size, &mut Text, &input).children[0]
            .bounds
            .x,
        x
    );

    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert!(input.begin_scroll_gesture(
        &view,
        &frame,
        start,
        now + Duration::from_millis(300),
        ScrollGestureKind::Touch
    ));
    assert!(input.move_scroll_gesture(&view, &frame, end, now + Duration::from_millis(340)));
    view.properties
        .insert("layout-direction", LayoutDirection::LeftToRight.into());
    let flipped = layout_with_input(&view, size, &mut Text, &input);
    input.reconcile(&view, &flipped);
    assert_eq!(handle.horizontal_overshoot(), Length(0.0));
    assert_eq!(flipped.children[0].bounds.x, 400.0);
}

#[test]
fn flick_only_overshoot_starts_at_the_inertial_boundary() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{FlickBoundsBehavior, Orientation, ViewportHandle};

    for (behavior, expect_rebound) in [
        (FlickBoundsBehavior::OvershootBounds, true),
        (FlickBoundsBehavior::DragOverBounds, false),
    ] {
        let handle = ViewportHandle::for_flickable(false);
        let mut root = node("Flickable", "view");
        root.viewport_handle = Some(handle.clone());
        root.properties.insert("bounds-behavior", behavior.into());
        length(&mut root, "content-height", 300.0);
        let viewport = Size::new(120.0, 100.0);
        let mut input = Interaction::default();
        let frame = layout(&root, viewport, &mut Text);
        assert!(input.set_viewport_offset(&[], &frame, Orientation::Vertical, 190.0));
        let frame = layout_with_input(&root, viewport, &mut Text, &input);
        let now = Instant::now();
        let start = Point { x: 50.0, y: 80.0 };
        let end = Point { x: 50.0, y: 62.0 };
        assert!(input.begin_scroll_gesture(&root, &frame, start, now, ScrollGestureKind::Touch));
        assert!(input.move_scroll_gesture(&root, &frame, end, now + Duration::from_millis(20)));
        assert_eq!(handle.vertical_overshoot(), Length(0.0));
        let frame = layout_with_input(&root, viewport, &mut Text, &input);
        input.end_scroll_gesture(&root, &frame, end, now + Duration::from_millis(25));
        assert!(handle.flicking());
        assert_eq!(
            input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(55)),
            expect_rebound
        );
        assert_eq!(handle.rebounding(), expect_rebound);
        assert_eq!(handle.vertical_overshoot().0 > 0.0, expect_rebound);
        assert_eq!(input.scroll_offsets()[&[][..]], 200.0);
        assert!(!handle.flicking());
        input.cancel_scroll_gesture();
        assert!(!handle.moving() && !input.has_overscroll());
    }
}

#[test]
fn scroll_content_measures_only_axes_without_explicit_sizes() {
    #[derive(Default)]
    struct Measured(Vec<(String, f32)>);

    impl TextMeasurer for Measured {
        fn measure(&mut self, text: &str, _: f32, max_width: f32) -> Size {
            self.0.push((text.to_owned(), max_width));
            Size::new(max_width.min(60.0), 24.0)
        }
    }

    for (kind, max_offset) in [("Flickable", 70.0), ("ScrollView", 98.0)] {
        let mut view = node(kind, "view");
        length(&mut view, "content-width", 200.0);
        length(&mut view, "content-height", 160.0);
        let mut fixed = node("Label", "fixed");
        fixed.properties.insert("text", "fixed".to_owned().into());
        for (name, factor) in [("width", 0.5), ("height", 0.25)] {
            fixed
                .properties
                .insert(name, Value::Percent(xrack_runtime::Percent(factor)));
        }
        length(&mut fixed, "x", 124.0);
        length(&mut fixed, "y", 148.0);
        let mut intrinsic = node("Text", "intrinsic");
        intrinsic
            .properties
            .insert("text", "intrinsic".to_owned().into());
        length(&mut intrinsic, "width", 40.0);
        let mut text = Measured::default();
        let size = linear::measure(&fixed, Size::new(200.0, 160.0), &mut text);
        assert_eq!((size.x.preferred, size.y.preferred), (100.0, 40.0));
        assert!(text.0.is_empty());
        view.children = vec![fixed, intrinsic];

        let viewport = Size::new(120.0, 90.0);
        let frame = layout(&view, viewport, &mut text);
        assert_eq!(
            frame.children[0].bounds,
            Rect::new(124.0, 148.0, 100.0, 40.0)
        );
        assert_eq!(frame.children[1].bounds, Rect::new(0.0, 0.0, 40.0, 24.0));
        assert_eq!(frame.scroll.unwrap().max_offset, max_offset);
        assert_eq!(text.0, [("intrinsic".to_owned(), 40.0)]);

        let mut input = Interaction::default();
        assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Vertical, 30.0));
        text.0.clear();
        let shifted = layout_with_input(&view, viewport, &mut text, &input);
        assert_eq!(
            shifted.children[0].bounds,
            Rect::new(124.0, 118.0, 100.0, 40.0)
        );
        assert_eq!(shifted.scroll.unwrap().max_offset, max_offset);
        assert_eq!(text.0, [("intrinsic".to_owned(), 40.0)]);
    }
}

#[test]
fn flickable_clip_and_mouse_pan_are_independent_of_touch() {
    use std::time::{Duration, Instant};

    let mut root = node("Window", "root");
    let mut view = node("Flickable", "view");
    for (name, value) in [("x", 20.0), ("y", 20.0), ("width", 100.0), ("height", 80.0)] {
        length(&mut view, name, value);
    }
    boolean(&mut view, "mouse-drag-pan-enabled", false);
    let mut content = node("Rectangle", "content");
    length(&mut content, "width", 260.0);
    length(&mut content, "height", 240.0);
    view.children.push(content);
    root.children.push(view);
    let viewport = Size::new(200.0, 180.0);
    let frame = layout(&root, viewport, &mut Text);
    assert_eq!(
        frame.children[0].children[0].clip,
        Rect::new(20.0, 20.0, 100.0, 80.0)
    );
    let now = Instant::now();
    let mut input = Interaction::default();
    let start = Point { x: 70.0, y: 60.0 };
    assert!(!input.arm_mouse_viewport_pan(&root, &frame, start, now));
    assert!(!input.begin_scroll_gesture(&root, &frame, start, now, ScrollGestureKind::MousePan));
    assert!(input.begin_scroll_gesture(&root, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(
        &root,
        &frame,
        Point { x: 70.0, y: 20.0 },
        now + Duration::from_millis(25)
    ));
    assert_eq!(input.scroll_offsets()[&[0][..]], 32.0);
    boolean(&mut root.children[0], "clip", false);
    let frame = layout(&root, viewport, &mut Text);
    assert_eq!(
        frame.children[0].children[0].clip,
        Rect::new(0.0, 0.0, 200.0, 180.0)
    );
    assert_eq!(frame.children[0].scroll.unwrap().max_offset, 160.0);
}

#[test]
fn flickable_motion_follows_nested_handoff_and_cancel_targets_the_owner() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{FlickDirection, Orientation, ScrollAction, ScrollRequest, ViewportHandle};

    let outer = ViewportHandle::for_flickable(true);
    let inner = ViewportHandle::for_flickable(true);
    let mut root = node("Flickable", "outer");
    root.viewport_handle = Some(outer.clone());
    length(&mut root, "content-height", 420.0);
    let mut child = node("Flickable", "inner");
    child.viewport_handle = Some(inner.clone());
    child
        .properties
        .insert("flick-direction", FlickDirection::Vertical.into());
    for (name, value) in [
        ("x", 10.0),
        ("y", 20.0),
        ("width", 120.0),
        ("height", 80.0),
        ("content-height", 100.0),
    ] {
        length(&mut child, name, value);
    }
    let mut content = node("Rectangle", "content");
    length(&mut content, "width", 240.0);
    length(&mut content, "height", 240.0);
    child.children.push(content);
    root.children.push(child);

    let viewport = Size::new(240.0, 160.0);
    let mut input = Interaction::default();
    let frame = layout(&root, viewport, &mut Text);
    assert_eq!(frame.children[0].scroll.unwrap().max_offset, 20.0);
    assert_eq!(
        frame.children[0]
            .scroll_on(Orientation::Horizontal)
            .unwrap()
            .max_offset,
        120.0
    );
    assert!(
        frame.children[0]
            .scroll_bars(&root.children[0])
            .next()
            .is_none()
    );
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &root,
        &frame,
        Point { x: 50.0, y: 60.0 },
        now,
        ScrollGestureKind::Touch
    ));
    assert!(input.move_scroll_gesture(
        &root,
        &frame,
        Point { x: 30.0, y: 20.0 },
        now + Duration::from_millis(20)
    ));
    assert!(inner.dragging() && outer.dragging());
    assert_eq!(input.scroll_offsets()[&[0][..]], 20.0);
    assert!(input.horizontal_scroll_offsets().is_empty());

    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    let end = Point { x: 30.0, y: 0.0 };
    assert!(input.move_scroll_gesture(&root, &frame, end, now + Duration::from_millis(40)));
    assert!(!inner.moving() && outer.dragging());
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    input.end_scroll_gesture(&root, &frame, end, now + Duration::from_millis(45));
    assert!(outer.flicking() && !outer.dragging());
    assert!(!input.apply_scroll_requests(
        &root,
        &frame,
        [ScrollRequest {
            target: inner,
            action: ScrollAction::CancelFlick
        }]
    ));
    assert!(input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(70)));
    assert!(input.apply_scroll_requests(
        &root,
        &frame,
        [ScrollRequest {
            target: outer.clone(),
            action: ScrollAction::CancelFlick
        }]
    ));
    assert!(!outer.moving());
    assert!(!input.advance_scroll_motion(&root, &frame, now + Duration::from_millis(100)));
}

#[test]
fn frozen_columns_follow_visual_order_and_leave_room_for_the_scrollable_pane() {
    use xrack_runtime::{Orientation, TableHeader, ViewportHandle};

    let mut table = node("TableView", "table");
    let handle = ViewportHandle::default();
    table.viewport_handle = Some(handle.clone());
    table.properties.insert("frozen-columns", 2_i64.into());
    boolean(&mut table, "resizable-columns", true);
    let columns = (0..4)
        .map(|_| {
            let mut column = Node::new("TableColumn", None);
            length(&mut column, "width", 100.0);
            column
        })
        .collect();
    table.table_header = Some(Box::new(TableHeader {
        columns,
        sort: None,
        column_widths: Vec::new(),
    }));
    let viewport = Size::new(300.0, 180.0);
    let before = layout(&table, viewport, &mut Text);
    let mut input = Interaction::default();
    assert!(input.accessibility_scroll_to(&before, &[], Orientation::Horizontal, 100.0));
    let scrolled = layout_with_input(&table, viewport, &mut Text, &input);
    let geometry = scrolled.table.as_ref().unwrap();
    ViewportObservers::default().sync(&table, &scrolled);
    assert_eq!(handle.visible_width(), Length(300.0));
    assert_eq!(handle.scroll_size(Orientation::Horizontal), 0.5);
    assert_eq!(handle.scroll_position(Orientation::Horizontal), 0.5);
    assert_eq!(geometry.horizontal.offset, 100.0);
    assert_eq!(geometry.frozen_boundary(), Some(200.0));
    assert_eq!(geometry.columns[0].x, 0.0);
    assert_eq!(geometry.columns[1].x, 100.0);
    assert_eq!(geometry.columns[2].x, 100.0);
    assert_eq!(geometry.column_at(Point { x: 145.0, y: 12.0 }), Some(1));
    assert_eq!(
        geometry.resize_at(&table, scrolled.clip, Point { x: 200.0, y: 12.0 }),
        Some(1)
    );
    let bar = scrolled
        .scroll_bars(&table)
        .find(|bar| bar.orientation == Orientation::Horizontal)
        .unwrap();
    assert!(bar.track.x >= 200.0);
    assert_eq!(geometry.reveal_column(0), 100.0);
    assert_eq!(geometry.reveal_column(2), 0.0);
    assert_eq!(
        geometry.column_drop(3, Point { x: 20.0, y: 12.0 }),
        Some((0, 0.0))
    );
    assert_eq!(
        geometry.column_drop(0, Point { x: 275.0, y: 12.0 }),
        Some((3, 300.0))
    );

    table.table_column_order = table.table_column_order.moved(4, 0, 3).unwrap().1;
    let reordered = layout_with_input(&table, viewport, &mut Text, &input);
    let geometry = reordered.table.as_ref().unwrap();
    assert!(geometry.is_frozen(1) && geometry.is_frozen(2) && !geometry.is_frozen(0));
    assert_eq!(geometry.column_at(Point { x: 45.0, y: 12.0 }), Some(1));
    assert_eq!(
        geometry
            .column_window(reordered.clip)
            .columns(4)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let narrow = layout_with_input(&table, Size::new(130.0, 180.0), &mut Text, &input);
    assert_eq!(narrow.table.as_ref().unwrap().frozen_boundary(), None);
    assert!(!narrow.table.as_ref().unwrap().is_frozen(1));
    table.properties.insert("frozen-columns", i64::MAX.into());
    let fitted = layout(&table, Size::new(420.0, 180.0), &mut Text);
    assert!(fitted.table.as_ref().unwrap().is_frozen(0));
    assert_eq!(fitted.table.as_ref().unwrap().scrollable_body().width, 0.0);
}

#[test]
fn table_header_hit_testing_reaches_visible_columns_after_a_far_scroll() {
    use xrack_runtime::{LayoutDirection, Orientation, TableHeader};

    let mut table = node("TableView", "table");
    table.properties.insert("frozen-columns", 2_i64.into());
    boolean(&mut table, "resizable-columns", true);
    let count = 8192;
    let last = count - 2;
    let columns = (0..count)
        .map(|index| {
            let mut column = Node::new("TableColumn", None);
            length(&mut column, "width", 80.0);
            boolean(&mut column, "visible", index % 19 != 7);
            column
        })
        .collect();
    table.table_header = Some(Box::new(TableHeader {
        columns,
        sort: None,
        column_widths: Vec::new(),
    }));
    // Move the last declaration to the frozen pane. The final scrolling
    // column is now the second-to-last declaration.
    table.table_column_order = table
        .table_column_order
        .moved(count, count - 1, 1)
        .unwrap()
        .1;
    let viewport = Size::new(343.0, 180.0);
    for direction in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
        table
            .properties
            .insert("layout-direction", direction.into());
        let before = layout(&table, viewport, &mut Text);
        let end = before.table.as_ref().unwrap().horizontal.max_offset;
        let mut input = Interaction::default();
        assert!(input.accessibility_scroll_to(&before, &[], Orientation::Horizontal, end));
        let scrolled = layout_with_input(&table, viewport, &mut Text, &input);
        let geometry = scrolled.table.as_ref().unwrap();
        assert!(geometry.columns_in_viewport().count() < 10);
        assert!(!geometry.is_frozen(last));
        assert!(geometry.is_frozen(count - 1));
        for column in [0, count - 1, last] {
            let bounds = geometry.header_cell_bounds(column, scrolled.clip).unwrap();
            let center = Point {
                x: bounds.x + bounds.width * 0.5,
                y: bounds.y + bounds.height * 0.5,
            };
            assert_eq!(geometry.column_at(center), Some(column));
            let grip = Point {
                x: if geometry.mirrored() {
                    bounds.x + 1.0
                } else {
                    bounds.x + bounds.width - 1.0
                },
                y: center.y,
            };
            assert_eq!(
                geometry.resize_at(&table, scrolled.clip, grip),
                Some(column)
            );
            input.pointer_move(&table, &scrolled, grip);
            assert_eq!(input.table_resize_column(&[]), Some(column));
        }
    }
}

#[test]
fn table_positioning_uses_the_scrollable_pane_and_latest_queued_offset() {
    use xrack_runtime::{
        ListMetrics, Orientation, ScrollAction, ScrollAlignment, ScrollRequest, TableHeader,
        ViewportHandle,
    };
    let mut table = node("TableView", "table");
    let handle = ViewportHandle::with_scroll_notifications();
    table.viewport_handle = Some(handle.clone());
    table.list = Some(ListMetrics {
        row_count: 50_000,
        first_row: 0,
    });
    table.properties.insert("frozen-columns", 1_i64.into());
    length(&mut table, "row-header-width", 40.0);
    length(&mut table, "row-height", 20.0);
    table.table_header = Some(Box::new(TableHeader {
        columns: [140.0, 100.0, 180.0, 110.0]
            .into_iter()
            .map(|width| {
                let mut column = node("TableColumn", "column");
                length(&mut column, "width", width);
                column
            })
            .collect(),
        sort: None,
        column_widths: Vec::new(),
    }));
    let viewport = Size::new(300.0, 180.0);
    let frame = layout(&table, viewport, &mut Text);
    let mut input = Interaction::default();
    let request = |column, alignment| ScrollRequest {
        target: handle.clone(),
        action: ScrollAction::ToColumn { column, alignment },
    };
    // The 140px frozen column and 40px row header leave a 120px scrolling pane.
    for (alignment, expected) in [
        (ScrollAlignment::Beginning, 100.0),
        (ScrollAlignment::Center, 130.0),
        (ScrollAlignment::End, 160.0),
    ] {
        input.apply_scroll_requests(&table, &frame, [request(2, alignment)]);
        assert_eq!(
            input.horizontal_scroll_offsets().get(&[][..]),
            Some(&expected)
        );
    }
    assert!(input.apply_scroll_requests(
        &table,
        &frame,
        [
            request(3, ScrollAlignment::End), // 270px, capped to the content boundary
            request(2, ScrollAlignment::Visible), // remains partly visible at 270px
        ]
    ));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&270.0));
    assert!(input.apply_scroll_requests(&table, &frame, [request(2, ScrollAlignment::Contain)]));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&100.0));
    assert!(!input.apply_scroll_requests(
        &table,
        &frame,
        [
            request(0, ScrollAlignment::Beginning),
            request(99, ScrollAlignment::End)
        ]
    ));

    let cell = |row, column| ScrollRequest {
        target: handle.clone(),
        action: ScrollAction::ToCell {
            row,
            column,
            horizontal: ScrollAlignment::End,
            vertical: ScrollAlignment::Beginning,
        },
    };
    assert!(!input.apply_scroll_requests(
        &table,
        &frame,
        [cell(-1, 3), cell(49_000, 9), cell(50_000, 2)]
    ));
    assert!(input.scroll_offsets().is_empty());
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&100.0));
    assert!(input.apply_scroll_requests(&table, &frame, [cell(49_000, 0)]));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&100.0));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&980_000.0));
    assert!(input.take_scroll_events(&table, &frame).is_empty());
    let scrolled = layout_with_input(&table, viewport, &mut Text, &input);
    ViewportObservers::default().sync(&table, &scrolled);
    assert_eq!(handle.content_x(), Length(-100.0));
    assert_eq!(handle.content_y(), Length(-980_000.0));
    assert_eq!(
        scrolled.scroll_on(Orientation::Horizontal).unwrap().offset,
        100.0
    );
}

#[test]
fn enlarged_default_table_header_keeps_room_for_rows_and_respects_fixed_height() {
    struct Scaled(f32);
    impl TextMeasurer for Scaled {
        fn text_scale(&self) -> f32 {
            self.0
        }
        fn measure(&mut self, value: &str, size: f32, width: f32) -> Size {
            Text.measure(value, size, width)
        }
    }

    let mut table = node("TableView", "table");
    let mut column = node("TableColumn", "instrument");
    length(&mut column, "width", 120.0);
    table.table_header = Some(Box::new(xrack_runtime::TableHeader {
        columns: vec![column],
        sort: None,
        column_widths: vec![],
    }));
    let size = Size::new(180.0, 140.0);
    let regular = layout(&table, size, &mut Scaled(1.0));
    let enlarged = layout(&table, size, &mut Scaled(2.6));
    let normal = regular.table.as_ref().unwrap();
    let large = enlarged.table.as_ref().unwrap();
    assert_eq!(normal.header.height, 34.0);
    assert!(large.header.height >= 60.0);
    assert_eq!(large.body.y, large.header.y + large.header.height);
    assert_eq!(large.columns[0].width, normal.columns[0].width);
    assert_eq!(large.column_at(Point { x: 70.0, y: 51.0 }), Some(0));

    let compact = layout(&table, Size::new(180.0, 60.0), &mut Scaled(2.6));
    assert_eq!(compact.table.as_ref().unwrap().body.height, 26.0);
    length(&mut table, "header-height", 40.0);
    let fixed = layout(&table, size, &mut Scaled(2.6));
    assert_eq!(fixed.table.as_ref().unwrap().header.height, 40.0);
}

#[test]
fn table_header_drop_targets_only_shown_columns_in_full_permutation_order() {
    use xrack_runtime::TableHeader;

    let mut table = node("TableView", "table");
    let columns = [true, false, true, false]
        .into_iter()
        .map(|shown| {
            let mut column = Node::new("TableColumn", None);
            length(&mut column, "width", 100.0);
            boolean(&mut column, "visible", shown);
            column
        })
        .collect();
    table.table_header = Some(Box::new(TableHeader {
        columns,
        sort: None,
        column_widths: Vec::new(),
    }));
    let bounds = Rect::new(0.0, 0.0, 250.0, 120.0);
    let geometry = TableGeometry::new(&table, bounds).unwrap();
    assert_eq!(geometry.column_at(Point { x: 100.0, y: 8.0 }), Some(2));
    assert_eq!(
        geometry.column_drop(0, Point { x: 180.0, y: 8.0 }),
        Some((2, 200.0))
    );
    assert_eq!(geometry.visible_columns().collect::<Vec<_>>(), [0, 2]);
    for column in &mut table.table_header.as_mut().unwrap().columns {
        boolean(column, "visible", false);
    }
    let geometry = TableGeometry::new(&table, bounds).unwrap();
    assert_eq!(geometry.visible_column_count(), 0);
    assert_eq!(geometry.scrollable_body().width, 0.0);
    assert!(geometry.column_drop(0, Point { x: 80.0, y: 8.0 }).is_none());
}

#[test]
fn prepared_table_rows_survive_scroll_and_refresh_when_their_inputs_change() {
    use xrack_runtime::{
        ListRowRenderer, StandardTableRowCache, TableHeader, TableRowOrder, VecModel,
    };

    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            Node::hidden()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let model = Rc::new(VecModel::from((0..20).collect::<Vec<i64>>()));
    let rows = TableRowOrder::default().rows(model.clone());
    let projection = StandardTableRowCache::default();
    let mut table = node("TableView", "table");
    length(&mut table, "row-height", 24.0);
    length(&mut table, "row-header-width", 40.0);
    table.list = Some(ListMetrics {
        row_count: 20,
        first_row: 0,
    });
    table.table_header = Some(Box::new(TableHeader {
        columns: vec![node("TableColumn", "value"), node("TableColumn", "other")],
        column_widths: Vec::new(),
        sort: None,
    }));
    table.list_row_renderer = Some(ListRowRenderer::with_columns(move |range, columns| {
        projection.render(
            || rows.clone(),
            range,
            columns,
            None,
            2,
            |_| None,
            |value, _| value.to_string(),
            |_| None,
        )
    }));
    let mut scene = Rc::new(table);
    let viewport = Size::new(200.0, 130.0);
    let mut input = Interaction::default();
    let mut cache = RowLayoutCache::default();
    let prepare = |scene: &Rc<Node>, input: &Interaction, cache: &mut RowLayoutCache| {
        prepare_scene_with_row_cache(&View, scene.clone(), viewport, &mut Text, input, cache)
    };
    prepare(&scene, &input, &mut cache);
    let (initial, layout) = prepare(&scene, &input, &mut cache);
    assert!(input.accessibility_scroll_to(&layout, &[], Orientation::Vertical, 24.0));
    let (scrolled, _) = prepare(&scene, &input, &mut cache);
    assert_eq!(scrolled.list.unwrap().first_row, 1);
    assert!(scrolled.children[0].shares_storage_with(&initial.children[1]));
    assert_eq!(scrolled.children[0].text("row-header-text"), Some("2"));

    assert!(model.set_row(1, 91));
    let (edited, _) = prepare(&scene, &input, &mut cache);
    assert!(!edited.children[0].shares_storage_with(&scrolled.children[0]));
    assert_eq!(
        edited.children[0].children[0].children[0].text("text"),
        Some("91")
    );
    assert_eq!(
        scrolled.children[0].children[0].children[0].text("text"),
        Some("1")
    );
    assert!(edited.children[1].shares_storage_with(&scrolled.children[1]));

    Rc::make_mut(&mut scene)
        .properties
        .insert("current-row", 1_i64.into());
    let (selected, cached) = prepare(&scene, &input, &mut cache);
    assert!(selected.children[0].boolean("selected", false));
    assert!(!edited.children[0].boolean("selected", false));
    assert!(selected.children[1].shares_storage_with(&edited.children[1]));
    let (fresh, direct) =
        prepare_scene_with_input(&View, scene.clone(), viewport, &mut Text, &input);
    assert_eq!(*selected, *fresh);
    assert_eq!(cached, direct);

    let (_, order) = scene.table_column_order.moved(2, 0, 1).unwrap();
    Rc::make_mut(&mut scene).table_column_order = order;
    let (reordered, cached) = prepare(&scene, &input, &mut cache);
    let (fresh, direct) = prepare_scene_with_input(&View, scene, viewport, &mut Text, &input);
    assert_eq!(*reordered, *fresh);
    assert_eq!(cached, direct);
    assert_eq!(
        reordered.children[0].children[0]
            .table_column_order
            .as_slice(2),
        Some(&[1, 0][..]),
    );
}

#[test]
fn table_cell_positioning_updates_bound_offsets_and_ignores_hidden_targets() {
    use std::{cell::Cell, rc::Rc};
    use xrack_runtime::{
        ListMetrics, Orientation, ScrollAction, ScrollAlignment, ScrollRequest, TableHeader,
        ViewportHandle,
    };
    let handle = ViewportHandle::default();
    let offsets = Rc::new([Cell::new(Length(-5.0)), Cell::new(Length(-10.0))]);
    for (index, axis) in [Orientation::Horizontal, Orientation::Vertical]
        .into_iter()
        .enumerate()
    {
        let read = offsets.clone();
        let write = offsets.clone();
        handle.bind_offset(
            axis,
            move || read[index].get(),
            move |value| write[index].set(value),
        );
    }
    let mut table = node("TableView", "table");
    table.viewport_handle = Some(handle.clone());
    table.list = Some(ListMetrics {
        row_count: 200,
        first_row: 0,
    });
    length(&mut table, "row-height", 30.0);
    table.table_header = Some(Box::new(TableHeader {
        columns: (0..3)
            .map(|_| {
                let mut column = node("TableColumn", "column");
                length(&mut column, "width", 100.0);
                column
            })
            .collect(),
        sort: None,
        column_widths: Vec::new(),
    }));
    let viewport = Size::new(150.0, 120.0);
    let frame = layout(&table, viewport, &mut Text);
    let mut input = Interaction::default();
    let action = ScrollAction::ToCell {
        row: 99,
        column: 2,
        horizontal: ScrollAlignment::Center,
        vertical: ScrollAlignment::Center,
    };
    assert!(input.apply_scroll_requests(
        &table,
        &frame,
        [ScrollRequest {
            target: handle.clone(),
            action
        }]
    ));
    // Horizontal centering is clamped to the final content edge.
    assert_eq!(offsets[0].get(), Length(-150.0));
    assert_eq!(offsets[1].get(), Length(-2942.0));
    let frame = layout_with_input(&table, viewport, &mut Text, &input);
    assert!(!input.apply_scroll_requests(
        &table,
        &frame,
        [ScrollRequest {
            target: ViewportHandle::default(),
            action
        }]
    ));
    boolean(
        &mut table.table_header.as_mut().unwrap().columns[1],
        "visible",
        false,
    );
    let frame = layout_with_input(&table, viewport, &mut Text, &input);
    assert!(!input.apply_scroll_requests(
        &table,
        &frame,
        [ScrollRequest {
            target: handle.clone(),
            action: ScrollAction::ToCell {
                row: 0,
                column: 1,
                horizontal: ScrollAlignment::Beginning,
                vertical: ScrollAlignment::Beginning,
            }
        }]
    ));
    assert_eq!(offsets[0].get(), Length(-150.0));
    assert_eq!(offsets[1].get(), Length(-2942.0));
    boolean(&mut table, "visible", false);
    let frame = layout_with_input(&table, viewport, &mut Text, &input);
    assert!(!input.apply_scroll_requests(
        &table,
        &frame,
        [ScrollRequest {
            target: handle,
            action: ScrollAction::ToCell {
                row: 0,
                column: 0,
                horizontal: ScrollAlignment::Beginning,
                vertical: ScrollAlignment::Beginning,
            }
        }]
    ));
    assert_eq!(offsets[0].get(), Length(-150.0));
    assert_eq!(offsets[1].get(), Length(-2942.0));
}

#[test]
fn release_snapping_uses_the_last_drag_point_after_pointer_leave() {
    use xrack_runtime::{RangeAction, ScrollSnapMode};

    let mut root = node("ScrollBar", "bar");
    root.properties.insert("size", 0.25_f64.into());
    root.properties.insert("step-size", 0.3_f64.into());
    root.properties
        .insert("snap-mode", ScrollSnapMode::OnRelease.into());
    let layout = layout(&root, Size::new(12.0, 120.0), &mut Text);
    let start = Point { x: 6.0, y: 15.0 };
    let mut input = Interaction::default();
    input.pointer_move(&root, &layout, start);
    assert_eq!(
        input.scroll_bar_press(&root, &layout),
        Some(ScrollBarUpdate::Unchanged)
    );
    assert_eq!(
        input.release_scroll_bar(&root, &layout),
        Some(ScrollBarUpdate::Unchanged)
    );
    input.scroll_bar_press(&root, &layout).unwrap();
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: start.x,
            y: 45.6,
        },
    );
    assert!(matches!(
        input.scroll_bar_drag(&root, &layout),
        Some(ScrollBarUpdate::Range(_))
    ));
    input.pointer_leave();
    let update = input.release_scroll_bar(&root, &layout).unwrap();
    assert_eq!(
        update,
        ScrollBarUpdate::Range(RangeInput {
            path: vec![],
            action: RangeAction::SetFraction(0.3)
        })
    );
    assert!(!input.scroll_bar_captured());
}

#[test]
fn vertical_tab_bar_reveals_the_selected_tab_and_scrolls_on_its_axis() {
    let mut bar = node("TabBar", "bar");
    bar.properties
        .insert("orientation", Value::Orientation(Orientation::Vertical));
    bar.properties.insert("current-index", 2_i64.into());
    for id in ["first", "middle", "last"] {
        let mut tab = node("TabButton", id);
        length(&mut tab, "height", 40.0);
        bar.children.push(tab);
    }
    let size = Size::new(100.0, 80.0);
    let frame = layout(&bar, size, &mut Text);
    assert_eq!(frame.scroll.unwrap().offset, 40.0);
    assert!(frame.scroll_on(Orientation::Horizontal).is_none());
    assert_eq!(frame.children[2].bounds, Rect::new(0.0, 40.0, 100.0, 40.0));
    let mut input = Interaction::default();
    assert!(!input.focus_next(&bar, &frame, false));
    assert_eq!(input.focused.as_ref().unwrap().id, Some("last"));
    assert_eq!(
        input
            .group_key(&bar, &frame, NavigationKey::Up)
            .unwrap()
            .action,
        xrack_runtime::ControlAction::Select(1)
    );
    assert_eq!(
        input
            .group_key(&bar, &frame, NavigationKey::Home)
            .unwrap()
            .action,
        xrack_runtime::ControlAction::Select(0)
    );
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    assert_eq!(frame.scroll.unwrap().offset, 0.0);
    assert_eq!(input.focused.unwrap().id, Some("first"));
}

#[test]
fn a_tab_close_press_cannot_activate_the_body_or_a_changed_value() {
    let mut bar = node("TabBar", "tabs");
    boolean(&mut bar, "tabs-closable", true);
    let mut tab = node("TabButton", "note");
    tab.properties.insert("value", "one".to_owned().into());
    bar.children.push(tab);
    let mut input = Interaction::default();
    let size = Size::new(180.0, 40.0);
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    let close = frame.children[0].tab_close.unwrap();
    let at = Point {
        x: close.x + close.width * 0.5,
        y: close.y + close.height * 0.5,
    };
    input.pointer_move(&bar, &frame, at);
    input.pointer_press_in_scene(&bar, &frame);
    input.pointer_move(&bar, &frame, Point { x: 25.0, y: at.y });
    assert!(input.pointer_release_target().is_none());

    input.pointer_move(&bar, &frame, at);
    input.pointer_press_in_scene(&bar, &frame);
    bar.children[0]
        .properties
        .insert("value", "two".to_owned().into());
    let changed = layout_with_input(&bar, size, &mut Text, &input);
    input.pointer_move(&bar, &changed, at);
    assert!(input.pointer_release_target().is_none());
}

#[test]
fn editor_multi_click_requires_a_completed_click_on_the_same_model_row() {
    use std::time::{Duration, Instant};
    use xrack_runtime::VecModel;

    let mut scene = node("Window", "window");
    let rows = VecModel::from(vec![0_i64, 1]);
    let mut row = node("ListRow", "row");
    row.set_row_key(rows.row_key(0));
    length(&mut row, "height", 60.0);
    let mut editor = node("LineEdit", "editor");
    length(&mut editor, "width", 160.0);
    length(&mut editor, "height", 36.0);
    row.children.push(editor);
    scene.children.push(row);
    let size = Size::new(240.0, 100.0);
    let point = Point { x: 20.0, y: 18.0 };
    let now = Instant::now();
    let mut input = Interaction::default();
    let press = |input: &mut Interaction, scene: &Node, at, selecting| {
        let frame = layout(scene, size, &mut Text);
        input.pointer_move(scene, &frame, point);
        input.pointer_press_in_scene(scene, &frame);
        let target = input.pressed.clone().unwrap();
        assert_eq!(target.path, [0, 0]);
        input.editor_mouse_press(&target, point, at, selecting)
    };
    assert_eq!(press(&mut input, &scene, now, false), 1);
    assert!(input.pointer_release_target().is_some());
    let at = |ms| now + Duration::from_millis(ms);
    assert_eq!(press(&mut input, &scene, at(85), false), 2);
    assert!(input.pointer_release_target().is_some());
    assert_eq!(press(&mut input, &scene, at(100), false), 3);
    assert!(input.pointer_release_target().is_some());

    assert_eq!(press(&mut input, &scene, at(125), false), 1);
    let frame = layout(&scene, size, &mut Text);
    input.pointer_move(&scene, &frame, Point { x: 80.0, y: 18.0 });
    input.pointer_move(&scene, &frame, point);
    assert!(input.pointer_release_target().is_some());
    assert_eq!(press(&mut input, &scene, at(150), false), 1);
    assert!(input.pointer_release_target().is_some());

    scene.children[0].set_row_key(rows.row_key(1));
    assert_eq!(press(&mut input, &scene, at(200), false), 1);
    assert!(input.pointer_release_target().is_some());
    assert_eq!(press(&mut input, &scene, at(300), true), 0);
}

#[test]
fn stack_layout_measures_the_stack_and_places_only_the_current_child() {
    let mut stack = node("StackLayout", "layers");
    length(&mut stack, "padding", 5.0);
    stack.properties.insert(
        "layout-direction",
        xrack_runtime::LayoutDirection::RightToLeft.into(),
    );
    let mut first = node("Rectangle", "first");
    length(&mut first, "width", 100.0);
    length(&mut first, "height", 40.0);
    let mut second = node("StackItem", "second");
    length(&mut second, "preferred-width", 178.0);
    length(&mut second, "min-height", 64.0);
    length(&mut second, "max-width", 192.0);
    boolean(&mut second, "enabled", false);
    stack.children.extend([first, second]);

    let available = Size::new(240.0, 140.0);
    let preferred = crate::linear::measure(&stack, available, &mut Text);
    assert_eq!(preferred.x.preferred, 188.0);
    assert_eq!(preferred.y.min, 74.0);

    let view = layout(&stack, available, &mut Text);
    assert!(view.children[0].visible);
    assert!(!view.children[1].visible);
    assert_eq!(view.children[0].bounds, Rect::new(135.0, 5.0, 100.0, 40.0));

    stack.properties.insert("current-index", 1_i64.into());
    let view = layout(&stack, available, &mut Text);
    assert!(!view.children[0].visible);
    assert!(view.children[1].visible);
    assert!(!view.children[1].enabled);
    assert_eq!(view.children[1].bounds, Rect::new(43.0, 5.0, 192.0, 130.0));
    assert_eq!(
        crate::linear::measure(&stack, available, &mut Text)
            .x
            .preferred,
        188.0
    );

    stack.properties.insert("current-index", (-1_i64).into());
    assert!(
        layout(&stack, available, &mut Text)
            .children
            .iter()
            .all(|child| !child.visible)
    );
}

#[test]
fn page_focus_skips_disabled_controls_and_replaced_rows() {
    use xrack_runtime::VecModel;

    let model = VecModel::from(vec![0_i64, 1, 2]);
    for (kind, page, replace_row) in [
        ("StackLayout", "StackItem", false),
        ("StackLayout", "StackItem", true),
        ("SwipeView", "SwipePage", false),
        ("SwipeView", "SwipePage", true),
    ] {
        let mut stack = node(kind, "layers");
        let mut first = node(page, "first");
        first.set_row_key(model.row_key(0));
        let mut details = button("details");
        length(&mut details, "y", 30.0);
        first.children.extend([button("edit"), details]);
        let mut second = node(page, "second");
        second.set_row_key(model.row_key(1));
        second.children.push(button("open"));
        stack.children.extend([first, second]);

        let size = Size::new(240.0, 140.0);
        let mut input = Interaction::default();
        let frame = layout_with_input(&stack, size, &mut Text, &input);
        let details = input.accessibility_target(&stack, &frame, &[0, 1]).unwrap();
        input.focus_target(&stack, &frame, details);
        assert!(input.is_focused(&[0, 1]));

        stack.properties.insert("current-index", 1_i64.into());
        let frame = layout_with_input(&stack, size, &mut Text, &input);
        input.reconcile(&stack, &frame);
        assert!(input.is_focused(&[1, 0]));

        if replace_row {
            stack.children[0].set_row_key(model.row_key(2));
        } else {
            boolean(&mut stack.children[0].children[1], "enabled", false);
        }
        stack.properties.insert("current-index", 0_i64.into());
        let frame = layout_with_input(&stack, size, &mut Text, &input);
        input.reconcile(&stack, &frame);
        assert!(input.is_focused(&[0, 0]));
    }
}

#[test]
fn overflowing_tab_widget_scrolls_only_the_header_and_reveals_keyboard_selection() {
    let mut widget = node("TabWidget", "tabs");
    widget.properties.insert("current-index", 0_i64.into());
    for id in ["a", "b", "c", "d", "e", "f"] {
        let mut tab = node("Tab", id);
        tab.properties.insert("title", format!("Page {id}").into());
        tab.children.push(button("page-button"));
        widget.children.push(tab);
    }
    let size = Size::new(168.0, 140.0);
    let point = Point { x: 56.0, y: 18.0 };
    let mut input = Interaction::default();
    let initial = layout_with_input(&widget, size, &mut Text, &input);
    let scroll = initial.scroll_on(Orientation::Horizontal).unwrap();
    assert_eq!(scroll.offset, 0.0);
    assert!(scroll.max_offset > size.width);
    assert!(!crate::input::eligible(
        &widget.children[5],
        &initial.children[5]
    ));

    input.pointer_move(&widget, &initial, Point { y: 56.0, ..point });
    assert!(!input.scroll_wheel(&initial, Point { x: 0.0, y: 90.0 }, true));
    input.pointer_move(&widget, &initial, point);
    assert!(input.scroll_wheel(
        &initial,
        Point {
            x: 0.0,
            y: scroll.max_offset
        },
        true
    ));
    let scrolled = layout_with_input(&widget, size, &mut Text, &input);
    assert!(!crate::input::eligible(
        &widget.children[0],
        &scrolled.children[0]
    ));
    assert!(crate::input::eligible(
        &widget.children[5],
        &scrolled.children[5]
    ));
    assert!(scrolled.children[0].visible);
    assert_eq!(scrolled.children[0].bounds, initial.children[0].bounds);

    input.reconcile(&widget, &scrolled);
    assert!(input.focus_next(&widget, &scrolled, false));
    let focused = layout_with_input(&widget, size, &mut Text, &input);
    assert_eq!(
        focused.scroll_on(Orientation::Horizontal).unwrap().offset,
        0.0
    );
    let action = input
        .group_key(&widget, &focused, NavigationKey::End)
        .unwrap();
    assert_eq!(action.action, xrack_runtime::ControlAction::Select(5));
    widget.properties.insert("current-index", 5_i64.into());
    let selected = layout_with_input(&widget, size, &mut Text, &input);
    assert!(crate::input::eligible(
        &widget.children[5],
        &selected.children[5]
    ));
    assert!(selected.children[5].visible);
}

#[test]
fn tab_reflow_reveals_the_selection_after_the_system_text_size_changes() {
    struct Scaled(f32);
    impl TextMeasurer for Scaled {
        fn text_scale(&self) -> f32 {
            self.0
        }
        fn measure(&mut self, label: &str, size: f32, width: f32) -> Size {
            Text.measure(label, size, width)
        }
    }

    let size = Size::new(168.0, 156.0);
    let mut tabs = node("TabWidget", "tabs");
    tabs.properties.insert("current-index", 3_i64.into());
    tabs.properties.insert("tabs-closable", true.into());
    for title in ["Summary", "Measurements", "Settings", "Results"] {
        let mut page = Node::new("Tab", None);
        page.properties.insert("title", title.to_owned().into());
        tabs.children.push(page);
    }
    let mut input = Interaction::default();
    let initial = layout_with_input(&tabs, size, &mut Scaled(1.0), &input);
    assert!(input.accessibility_scroll_to(&initial, &[], Orientation::Horizontal, 0.0));
    let browsed = layout_with_input(&tabs, size, &mut Scaled(1.0), &input);
    input.reconcile(&tabs, &browsed);
    assert!(browsed.children[3].tab_header.unwrap().x >= size.width);

    let scaled = layout_with_input(&tabs, size, &mut Scaled(2.75), &input);
    let header = scaled.children[3].tab_header.unwrap();
    let close = scaled.children[3].tab_close.unwrap();
    assert_eq!(header.intersect(scaled.tab_viewport()), header);
    assert!(header.height > initial.children[3].tab_header.unwrap().height);
    assert_eq!((close.width, close.height), (44.0, 44.0));
    assert_eq!(tabs.int("current-index"), Some(3));

    let short = layout_with_input(&tabs, Size::new(168.0, 72.0), &mut Scaled(2.75), &input);
    assert!(short.children[3].bounds.height >= 38.0);

    let mut bar = tabs.clone();
    bar.kind = "TabBar";
    assert!(
        selection::preferred_tab_view_size(&bar, &Scaled(2.75)).height
            > selection::preferred_tab_view_size(&bar, &Scaled(1.0)).height
    );
}

#[test]
fn rtl_tab_reorder_scrolls_toward_hidden_tabs_and_skips_disabled_targets() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, LayoutDirection, VecModel};

    let mut bar = node("TabBar", "bar");
    bar.properties.insert(
        "layout-direction",
        Value::LayoutDirection(LayoutDirection::RightToLeft),
    );
    bar.properties.insert("reorderable", true.into());
    bar.properties.insert("current-index", 0_i64.into());
    let model = VecModel::from(vec![0_i64, 1, 2, 3, 4]);
    bar.snapshot_group_options(&model);
    for (index, id) in ["first", "disabled", "third", "fourth", "last"]
        .into_iter()
        .enumerate()
    {
        let mut tab = node("TabButton", id);
        tab.set_row_key(model.row_key(index));
        length(&mut tab, "width", 70.0);
        if index == 1 {
            boolean(&mut tab, "enabled", false);
        }
        bar.children.push(tab);
    }
    let size = Size::new(160.0, 44.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    input.focus_next(&bar, &frame, false);
    assert!(matches!(
        input.tab_move_key(&bar, &frame, NavigationKey::Right),
        Some(None)
    ));
    let key = input
        .tab_move_key(&bar, &frame, NavigationKey::Left)
        .unwrap()
        .unwrap();
    let ControlAction::MoveTab(movement) = key.action else {
        panic!("expected move");
    };
    assert_eq!((movement.from, movement.to), (0, 2));

    let start = Point { x: 126.0, y: 22.0 };
    let edge = Point { x: 6.0, y: 22.0 };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&bar, &frame, start, now, ScrollGestureKind::MouseControl));
    assert!(input.move_scroll_gesture(&bar, &frame, edge, now));
    let initial = frame.scroll_on(Orientation::Horizontal).unwrap().offset;
    let mut frame = frame;
    for step in 1..=24 {
        let time = now + Duration::from_millis(step * 20);
        input.advance_scroll_motion(&bar, &frame, time);
        frame = layout_with_input(&bar, size, &mut Text, &input);
    }
    let scroll = frame.scroll_on(Orientation::Horizontal).unwrap();
    assert!(initial - scroll.offset > 100.0, "{scroll:?}");
    let marker = input.tab_move_marker(&bar, &frame, &[]).unwrap();
    assert!(marker.x < 65.0, "{marker:?}");
    let release = input.end_scroll_gesture(&bar, &frame, edge, now + Duration::from_millis(500));
    let ControlAction::MoveTab(movement) = release.control.unwrap().action else {
        panic!("expected drop");
    };
    assert_eq!((movement.from, movement.to), (0, 4));
}

#[test]
fn a_bound_tab_change_reveals_the_selection_without_locking_manual_scroll() {
    let mut bar = node("TabBar", "bar");
    for id in ["first", "middle", "last"] {
        let mut tab = node("TabButton", id);
        length(&mut tab, "width", 80.0);
        bar.children.push(tab);
    }
    let size = Size::new(160.0, 44.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Horizontal, 80.0));
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    input.reconcile(&bar, &frame);
    assert_eq!(
        frame.scroll_on(Orientation::Horizontal).unwrap().offset,
        80.0
    );
    assert_eq!(frame.children[0].bounds.intersect(frame.bounds).width, 0.0);

    bar.properties.insert("current-index", 1_i64.into());
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    input.reconcile(&bar, &frame);
    assert_eq!(
        frame.scroll_on(Orientation::Horizontal).unwrap().offset,
        80.0
    );

    bar.properties.insert("current-index", 0_i64.into());
    let frame = layout_with_input(&bar, size, &mut Text, &input);
    assert_eq!(
        frame.scroll_on(Orientation::Horizontal).unwrap().offset,
        0.0
    );
    assert_eq!(
        frame.children[0].bounds.intersect(frame.bounds),
        frame.children[0].bounds
    );
}

#[test]
fn a_nonmodal_popup_inside_a_dialog_can_receive_input_beyond_the_dialog_bounds() {
    use xrack_runtime::PopupPlacement;

    let mut window = node("Window", "root");
    let mut dialog = node("Dialog", "dialog");
    boolean(&mut dialog, "open", true);
    length(&mut dialog, "width", 140.0);
    length(&mut dialog, "height", 110.0);
    let mut anchor = button("anchor");
    let mut popup = node("PopupWindow", "popup");
    boolean(&mut popup, "open", true);
    length(&mut popup, "width", 112.0);
    length(&mut popup, "height", 72.0);
    popup
        .properties
        .insert("placement", PopupPlacement::Right.into());
    popup.children.push(button("popup-action"));
    anchor.children.push(popup);
    dialog.children.push(anchor);
    window.children.push(dialog);
    let frame = layout(&window, Size::new(400.0, 260.0), &mut Text);
    let button = frame.at_path(&[0, 0, 0, 0]).unwrap().bounds;
    let point = Point {
        x: button.x + button.width * 0.5,
        y: button.y + button.height * 0.5,
    };
    assert!(!frame.children[0].bounds.contains(point));
    let mut input = Interaction::default();
    input.reconcile(&window, &frame);
    input.pointer_move(&window, &frame, point);
    assert_eq!(
        input.hovered.as_ref().and_then(|target| target.id),
        Some("popup-action")
    );
    assert!(!input.dialog_pointer_press(&frame));
}

#[test]
fn anchored_popup_escapes_scroll_clipping_only_while_the_anchor_is_visible() {
    let mut window = node("Window", "root");
    let mut scroll = node("ScrollView", "scroll");
    length(&mut scroll, "x", 20.0);
    length(&mut scroll, "y", 20.0);
    length(&mut scroll, "width", 200.0);
    length(&mut scroll, "height", 100.0);
    length(&mut scroll, "content-height", 280.0);
    let mut anchor = button("anchor");
    length(&mut anchor, "x", 20.0);
    length(&mut anchor, "y", 130.0);
    let mut popup = node("PopupWindow", "popup");
    boolean(&mut popup, "open", true);
    length(&mut popup, "width", 130.0);
    length(&mut popup, "height", 60.0);
    anchor.children.push(popup);
    scroll.children.push(anchor);
    window.children.push(scroll);
    let viewport = Size::new(300.0, 200.0);
    let closed = layout(&window, viewport, &mut Text);
    assert!(closed.overlay_paths().is_empty());
    let offsets = BTreeMap::from([(vec![0], 100.0)]);
    let shown = layout_with_scroll(&window, viewport, &mut Text, &offsets);
    assert_eq!(shown.overlay_paths(), &[vec![0, 0, 0]]);
    assert!(shown.dialog_paths().is_empty());
    let popup = shown.at_path(&[0, 0, 0]).unwrap();
    assert_eq!(popup.bounds, Rect::new(40.0, 78.0, 130.0, 60.0));
    assert_eq!(popup.clip, shown.bounds);
}

#[test]
fn styled_label_keeps_graphemes_and_ellipsizes_the_last_fitting_line() {
    use unicode_segmentation::UnicodeSegmentation;
    use xrack_runtime::{TextOverflow, TextWrap};

    let mut label = node("Label", "label");
    label.properties.insert("text", "A👩‍💻BCD".to_owned().into());
    label
        .properties
        .insert("overflow", TextOverflow::Elide.into());
    length(&mut label, "font-size", 10.0);
    let advance = |text: &str| text.graphemes(true).count() as f32 * 10.0;
    assert_eq!(elide_to_width("A👩‍💻BCD", 30.0, advance), "A👩‍💻…");
    let elided = LabelLayout::new(&label, Size::new(30.0, 20.0), advance);
    assert_eq!(elided.lines[0].text, "A👩‍💻…");
    assert_eq!(elided.size.width, 30.0);
    assert!(
        LabelLayout::new(&label, Size::new(6.0, 20.0), advance).lines[0]
            .text
            .is_empty()
    );

    label
        .properties
        .insert("text", "alpha beta gamma".to_owned().into());
    label.properties.insert("wrap", TextWrap::WordWrap.into());
    label.properties.insert("max-lines", 2_i64.into());
    let layout = LabelLayout::new(&label, Size::new(50.0, 100.0), advance);
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| &*line.text)
            .collect::<Vec<_>>(),
        ["alpha", "beta…"]
    );
    let short = LabelLayout::new(&label, Size::new(50.0, 20.0), advance);
    assert_eq!(short.lines[0].text, "alph…");
}

#[test]
fn label_alignment_follows_each_paragraph_direction_and_block_height() {
    let mut label = node("Label", "label");
    length(&mut label, "font-size", 10.0);
    label
        .properties
        .insert("text", "abc\r\nשלום".to_owned().into());
    label.properties.insert(
        "vertical-alignment",
        xrack_runtime::TextVerticalAlignment::Bottom.into(),
    );
    let layout = LabelLayout::new(&label, Size::new(120.0, 70.0), |text| {
        text.chars().count() as f32 * 10.0
    });
    assert_eq!(layout.lines[0].origin, Point { x: 0.0, y: 42.0 });
    assert_eq!(layout.lines[1].origin, Point { x: 80.0, y: 56.0 });
    assert_eq!(layout.size, Size::new(40.0, 28.0));
}

#[test]
fn a_wrapped_label_is_remeasured_after_horizontal_layout_assigns_its_width() {
    let mut root = node("Window", "root");
    let mut row = node("HorizontalLayout", "row");
    let mut sidebar = button("sidebar");
    length(&mut sidebar, "width", 100.0);
    let mut label = node("Label", "label");
    length(&mut label, "font-size", 10.0);
    label
        .properties
        .insert("text", "abcdefghijklmnopqr".to_owned().into());
    label
        .properties
        .insert("wrap", xrack_runtime::TextWrap::CharWrap.into());
    row.children.extend([sidebar, label]);
    root.children.push(row);
    let frame = layout(&root, Size::new(160.0, 100.0), &mut Text);
    assert_eq!(
        frame.children[0].children[1].bounds,
        Rect::new(100.0, 0.0, 60.0, 42.0)
    );
}

#[test]
fn flexbox_grows_to_caps_and_shrinks_in_proportion_to_preferred_size() {
    use xrack_runtime::FlexboxLayoutWrap;

    let mut flex = node("FlexboxLayout", "flex");
    flex.properties
        .insert("flex-wrap", FlexboxLayoutWrap::NoWrap.into());
    flex.properties
        .insert("alignment", LayoutAlignment::Stretch.into());
    length(&mut flex, "spacing-horizontal", 10.0);
    for (id, preferred, min, max, stretch) in [
        ("first", 60.0, 30.0, 75.0, 1.0),
        ("second", 90.0, 40.0, 180.0, 2.0),
        ("fixed-preference", 30.0, 25.0, 80.0, 0.0),
    ] {
        let mut item = node("Rectangle", id);
        length(&mut item, "preferred-width", preferred);
        length(&mut item, "min-width", min);
        length(&mut item, "max-width", max);
        length(&mut item, "height", 20.0);
        item.properties
            .insert("horizontal-stretch", Value::Float(stretch));
        flex.children.push(item);
    }
    let widths = |size| {
        layout(&flex, Size::new(size, 50.0), &mut Text)
            .into_children()
            .into_iter()
            .map(|child| child.bounds.width)
            .collect::<Vec<_>>()
    };
    assert_eq!(widths(320.0), [75.0, 180.0, 30.0]);
    assert_eq!(widths(150.0), [42.0, 63.0, 25.0]);
    assert_eq!(widths(90.0), [30.0, 40.0, 25.0]);
}

#[test]
fn flexbox_stretches_lines_and_honors_an_items_cross_alignment() {
    let mut flex = node("FlexboxLayout", "flex");
    length(&mut flex, "spacing-vertical", 10.0);
    for id in ["first", "centered", "third"] {
        let mut item = node("Rectangle", id);
        length(&mut item, "width", 80.0);
        length(&mut item, "preferred-height", 20.0);
        flex.children.push(item);
    }
    flex.children[1].properties.insert(
        "cross-axis-self-alignment",
        CrossAxisAlignment::Center.into(),
    );
    let frame = layout(&flex, Size::new(180.0, 120.0), &mut Text);
    assert_eq!(frame.children[0].bounds, Rect::new(0.0, 0.0, 80.0, 55.0));
    assert_eq!(frame.children[1].bounds, Rect::new(80.0, 17.5, 80.0, 20.0));
    assert_eq!(frame.children[2].bounds, Rect::new(0.0, 65.0, 80.0, 55.0));
}

#[test]
fn reversed_flex_lines_place_items_from_the_logical_start_and_keep_focus_order() {
    use xrack_runtime::{FlexboxLayoutDirection, FlexboxLayoutWrap};

    let mut flex = node("FlexboxLayout", "flex");
    flex.properties
        .insert("flex-direction", FlexboxLayoutDirection::RowReverse.into());
    flex.properties
        .insert("flex-wrap", FlexboxLayoutWrap::WrapReverse.into());
    flex.properties.insert(
        "cross-axis-line-alignment",
        LayoutAlignment::SpaceBetween.into(),
    );
    length(&mut flex, "padding", 10.0);
    length(&mut flex, "spacing-horizontal", 8.0);
    length(&mut flex, "spacing-vertical", 6.0);
    let mut first = button("logical-first");
    first.properties.insert("layout-order", (-1_i64).into());
    flex.children = vec![button("second"), first, button("third")];
    let frame = layout(&flex, Size::new(190.0, 120.0), &mut Text);
    assert_eq!(frame.children[1].bounds, Rect::new(100.0, 90.0, 80.0, 20.0));
    assert_eq!(frame.children[0].bounds, Rect::new(12.0, 90.0, 80.0, 20.0));
    assert_eq!(frame.children[2].bounds, Rect::new(100.0, 10.0, 80.0, 20.0));
    let mut input = Interaction::default();
    input.focus_next(&flex, &frame, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("logical-first"));
    input.focus_next(&flex, &frame, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("second"));

    flex.properties.insert(
        "flex-direction",
        FlexboxLayoutDirection::ColumnReverse.into(),
    );
    let frame = layout(&flex, Size::new(190.0, 74.0), &mut Text);
    assert_eq!(frame.children[1].bounds, Rect::new(100.0, 44.0, 80.0, 20.0));
    assert_eq!(frame.children[0].bounds, Rect::new(100.0, 18.0, 80.0, 20.0));
    assert_eq!(frame.children[2].bounds, Rect::new(10.0, 44.0, 80.0, 20.0));
}

#[test]
fn vertical_flow_wraps_columns_and_skips_hidden_and_zero_sized_items() {
    let mut root = node("Window", "root");
    let mut flow = node("FlowLayout", "flow");
    flow.properties
        .insert("orientation", Value::Orientation(Orientation::Vertical));
    length(&mut flow, "height", 60.0);
    length(&mut flow, "padding", 5.0);
    length(&mut flow, "horizontal-spacing", 12.0);
    length(&mut flow, "vertical-spacing", 5.0);
    let mut hidden = button("hidden");
    boolean(&mut hidden, "visible", false);
    let mut zero = button("zero");
    length(&mut zero, "width", 0.0);
    let mut first = button("first");
    first.properties.insert("layout-order", Value::Int(-1));
    flow.children = vec![button("second"), hidden, zero, first, button("third")];
    root.children.push(flow);

    let layout = layout(&root, Size::new(300.0, 120.0), &mut Text);
    let flow = &layout.children[0];
    assert_eq!(flow.bounds, Rect::new(0.0, 0.0, 182.0, 60.0));
    assert_eq!(flow.children[3].bounds, Rect::new(5.0, 5.0, 80.0, 20.0));
    assert_eq!(flow.children[0].bounds, Rect::new(5.0, 30.0, 80.0, 20.0));
    assert_eq!(flow.children[4].bounds, Rect::new(97.0, 5.0, 80.0, 20.0));
    assert!(!flow.children[1].visible);
    assert_eq!(flow.children[2].bounds.width, 0.0);
}

#[test]
fn flow_inside_horizontal_layout_uses_its_assigned_width() {
    let mut root = node("Window", "root");
    let mut row = node("HorizontalLayout", "row");
    let mut sidebar = button("sidebar");
    length(&mut sidebar, "width", 100.0);
    let mut flow = node("FlowLayout", "flow");
    length(&mut flow, "horizontal-spacing", 10.0);
    length(&mut flow, "vertical-spacing", 5.0);
    flow.children = vec![button("first"), button("second")];
    row.children = vec![sidebar, flow];
    root.children.push(row);

    let layout = layout(&root, Size::new(250.0, 80.0), &mut Text);
    let row = &layout.children[0];
    assert_eq!(row.bounds.height, 45.0);
    let flow = &row.children[1];
    assert_eq!(flow.bounds, Rect::new(100.0, 0.0, 150.0, 45.0));
    let wrapped = &flow.children[1];
    assert_eq!(wrapped.bounds, Rect::new(100.0, 25.0, 80.0, 20.0));
    assert_eq!(wrapped.clip.intersect(wrapped.bounds), wrapped.bounds);
}

#[test]
fn vertical_toolbar_wraps_when_its_commands_outgrow_the_available_height() {
    let mut bar = node("ToolBar", "bar");
    bar.properties
        .insert("orientation", Value::Orientation(Orientation::Vertical));
    boolean(&mut bar, "wrap", true);
    for id in ["first", "second", "third"] {
        let mut tool = node("ToolButton", id);
        length(&mut tool, "width", 30.0);
        length(&mut tool, "height", 28.0);
        bar.children.push(tool);
    }
    let layout = layout(&bar, Size::new(100.0, 80.0), &mut Text);
    assert_eq!(layout.children[0].bounds, Rect::new(6.0, 6.0, 30.0, 28.0));
    assert_eq!(layout.children[1].bounds, Rect::new(6.0, 40.0, 30.0, 28.0));
    assert_eq!(layout.children[2].bounds, Rect::new(42.0, 6.0, 30.0, 28.0));
}

#[test]
fn scroll_bar_captures_drag_and_preserves_focus_when_policy_changes() {
    use xrack_runtime::{Orientation, ScrollBarPolicy, ViewportHandle};

    let mut root = node("Window", "root");
    root.children.push(button("opener"));
    let mut view = node("ScrollView", "view");
    view.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    length(&mut view, "x", 20.0);
    length(&mut view, "y", 30.0);
    length(&mut view, "width", 100.0);
    length(&mut view, "height", 90.0);
    length(&mut view, "viewport-height", 500.0);
    let mut under_bar = button("under-bar");
    length(&mut under_bar, "x", 80.0);
    length(&mut under_bar, "width", 20.0);
    length(&mut under_bar, "height", 90.0);
    view.children.push(under_bar);
    root.children.push(view);
    let viewport = Size::new(150.0, 150.0);
    let render =
        |root: &Node, input: &Interaction| layout_with_input(root, viewport, &mut Text, input);
    let bar = |root: &Node, layout: &LayoutNode| {
        layout.children[1]
            .scroll_bars(&root.children[1])
            .next()
            .unwrap()
    };
    let mut input = Interaction::default();
    let layout = render(&root, &input);
    input.focus_next(&root, &layout, false);
    let opener = input.focused.clone();
    let track = bar(&root, &layout).track;
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: track.x + 4.0,
            y: track.y + 65.0,
        },
    );
    assert!(input.hovered.is_none());
    assert!(input.scroll_bar_highlighted(&[1], Orientation::Vertical));
    assert_eq!(
        input.scroll_bar_press(&root, &layout),
        Some(ScrollBarUpdate::ViewportMoved)
    );
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&90.0));
    assert_eq!(input.focused, opener);
    assert!(input.pointer_release_target().is_none());

    let layout = render(&root, &input);
    assert_eq!(input.take_scroll_events(&root, &layout).len(), 1);
    let thumb = bar(&root, &layout).thumb;
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: thumb.x + 4.0,
            y: thumb.y + 5.0,
        },
    );
    assert_eq!(
        input.scroll_bar_press(&root, &layout),
        Some(ScrollBarUpdate::Unchanged)
    );
    input.pointer_move(&root, &layout, Point { x: 0.0, y: 300.0 });
    assert_eq!(
        input.scroll_bar_drag(&root, &layout),
        Some(ScrollBarUpdate::ViewportMoved)
    );
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&410.0));
    let layout = render(&root, &input);
    assert_eq!(input.take_scroll_events(&root, &layout).len(), 1);
    let end = bar(&root, &layout);
    assert!((end.thumb.y + end.thumb.height - (end.track.y + end.track.height)).abs() < 0.01);

    let policy = "vertical-scroll-bar-policy";
    root.children[1]
        .properties
        .insert(policy, ScrollBarPolicy::AlwaysOff.into());
    let layout = render(&root, &input);
    input.reconcile(&root, &layout);
    assert!(
        layout.children[1]
            .scroll_bars(&root.children[1])
            .next()
            .is_none()
    );
    root.children[1]
        .properties
        .insert(policy, ScrollBarPolicy::AsNeeded.into());
    let layout = render(&root, &input);
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: track.x + 4.0,
            y: track.y + 2.0,
        },
    );
    assert_eq!(
        input.scroll_bar_drag(&root, &layout),
        Some(ScrollBarUpdate::Unchanged)
    );
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&410.0));
    assert!(input.finish_scroll_bar());
    assert_eq!(input.focused, opener);

    root.children[1].properties.remove("viewport-height");
    root.children[1]
        .properties
        .insert(policy, ScrollBarPolicy::AlwaysOn.into());
    let layout = render(&root, &input);
    let full = bar(&root, &layout);
    assert_eq!(full.thumb, full.track);
    assert!(!full.scrollable());
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: full.track.x + 4.0,
            y: full.track.y + 10.0,
        },
    );
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("under-bar"));
    assert_eq!(input.scroll_bar_press(&root, &layout), None);
}

#[test]
fn scroll_view_extents_keep_percentage_children_stable_and_clamp_on_shrink() {
    use xrack_runtime::{Orientation, Percent};
    let mut view = node("ScrollView", "view");
    let mut wide = node("Rectangle", "wide");
    for (property, value) in [
        ("x", 140.0),
        ("y", 90.0),
        ("width", 120.0),
        ("height", 40.0),
    ] {
        length(&mut wide, property, value);
    }
    let mut relative = node("Rectangle", "relative");
    length(&mut relative, "x", 20.0);
    length(&mut relative, "height", 12.0);
    relative.properties.insert("width", Percent(1.0).into());
    view.children.extend([wide, relative]);
    let viewport = Size::new(100.0, 60.0);
    let mut input = Interaction::default();
    let render =
        |view: &Node, input: &Interaction| layout_with_input(view, viewport, &mut Text, input);
    let before = render(&view, &input);
    assert_eq!(
        before
            .scroll_on(Orientation::Horizontal)
            .unwrap()
            .max_offset,
        160.0
    );
    assert_eq!(before.scroll.unwrap().max_offset, 70.0);
    assert_eq!(before.children[1].bounds.width, 100.0);
    input.pointer_move(&view, &before, Point { x: 10.0, y: 10.0 });
    assert!(input.scroll_wheel(&before, Point { x: 500.0, y: 500.0 }, false));
    let end = render(&view, &input);
    assert_eq!(end.children[0].bounds, Rect::new(-20.0, 20.0, 120.0, 40.0));
    assert_eq!(end.children[0].clip, Rect::new(0.0, 0.0, 100.0, 60.0));

    boolean(&mut view.children[0], "visible", false);
    let shrunk = render(&view, &input);
    input.reconcile(&view, &shrunk);
    let x = shrunk.scroll_on(Orientation::Horizontal).unwrap();
    assert_eq!((x.offset, x.max_offset), (20.0, 20.0));
    assert_eq!(shrunk.scroll.unwrap().offset, 0.0);
    assert_eq!(shrunk.children[1].bounds, Rect::new(0.0, 0.0, 100.0, 12.0));
    view.properties.insert("content-width", Percent(1.5).into());
    let explicit = render(&view, &input);
    assert_eq!(explicit.children[1].bounds.width, 150.0);
    assert_eq!(
        explicit
            .scroll_on(Orientation::Horizontal)
            .unwrap()
            .max_offset,
        70.0
    );
}

#[test]
fn left_mouse_pan_respects_editors_scrollbars_and_nested_opt_in() {
    use std::time::{Duration, Instant};

    let mut outer = node("ScrollView", "outer");
    length(&mut outer, "content-height", 420.0);
    let mut inner = node("ScrollView", "inner");
    for (property, value) in [
        ("width", 120.0),
        ("height", 100.0),
        ("content-height", 140.0),
    ] {
        length(&mut inner, property, value);
    }
    boolean(&mut inner, "mouse-drag-pan-enabled", true);
    let mut editor = node("LineEdit", "editor");
    for (property, value) in [("x", 10.0), ("y", 10.0), ("width", 80.0), ("height", 24.0)] {
        length(&mut editor, property, value);
    }
    inner.children.push(editor);
    outer.children.push(inner);
    let viewport = Size::new(160.0, 120.0);
    let before = layout(&outer, viewport, &mut Text);
    let now = Instant::now();
    let press = |input: &mut Interaction, scene: &Node, frame: &LayoutNode, point| {
        input.pointer_move(scene, frame, point);
        input.pointer_press_in_scene(scene, frame);
        input.arm_mouse_viewport_pan(scene, frame, point, now)
    };
    let mut input = Interaction::default();
    assert!(!press(
        &mut input,
        &outer,
        &before,
        Point { x: 20.0, y: 22.0 }
    ));
    assert_eq!(
        input.focused.as_ref().and_then(|target| target.id),
        Some("editor")
    );
    input.cancel_press();
    let bar = before.children[0]
        .scroll_bars(&outer.children[0])
        .next()
        .unwrap();
    assert!(!press(
        &mut input,
        &outer,
        &before,
        Point {
            x: bar.track.x + 1.0,
            y: bar.track.y + 1.0
        }
    ));
    input.cancel_press();

    let start = Point { x: 60.0, y: 70.0 };
    let end = Point { x: 60.0, y: -30.0 };
    assert!(press(&mut input, &outer, &before, start));
    assert!(input.move_scroll_gesture(&outer, &before, end, now + Duration::from_millis(20)));
    assert!(input.mouse_viewport_pan_claimed());
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&40.0));
    assert!(!input.scroll_offsets().contains_key(&[][..]));

    boolean(&mut outer, "mouse-drag-pan-enabled", true);
    let before = layout(&outer, viewport, &mut Text);
    let mut input = Interaction::default();
    assert!(press(&mut input, &outer, &before, start));
    assert!(input.move_scroll_gesture(&outer, &before, end, now + Duration::from_millis(20)));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&52.0));
    boolean(&mut outer, "mouse-drag-pan-enabled", false);
    boolean(&mut outer.children[0], "mouse-drag-pan-enabled", false);
    let next = layout_with_input(&outer, viewport, &mut Text, &input);
    let release = input.end_scroll_gesture(&outer, &next, end, now + Duration::from_millis(25));
    assert!(!release.tap);
    assert!(!input.scroll_motion_active());
}

#[test]
fn an_ancestor_consumes_scroll_before_a_nested_view_can_refresh() {
    use std::time::{Duration, Instant};

    let mut outer = node("ScrollView", "outer");
    length(&mut outer, "content-height", 640.0);
    let mut inner = node("ScrollView", "inner");
    inner.viewport_handle = Some(xrack_runtime::ViewportHandle::default());
    length(&mut inner, "y", 140.0);
    length(&mut inner, "width", 240.0);
    length(&mut inner, "height", 180.0);
    boolean(&mut inner, "pull-to-refresh", true);
    outer.children.push(inner);

    for (outer_offset, refresh) in [(120.0, false), (0.0, true)] {
        let mut input = Interaction::default();
        let size = Size::new(320.0, 365.0);
        let initial = layout_with_input(&outer, size, &mut Text, &input);
        input.accessibility_scroll_to(&initial, &[], Orientation::Vertical, outer_offset);
        let layout = layout_with_input(&outer, size, &mut Text, &input);
        let bounds = layout.at_path(&[0]).unwrap().bounds;
        let start = Point {
            x: 80.0,
            y: bounds.y + 28.0,
        };
        let end = Point {
            y: start.y + 100.0,
            ..start
        };
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(&outer, &layout, start, now, ScrollGestureKind::Touch));
        input.move_scroll_gesture(&outer, &layout, end, now + Duration::from_millis(90));
        let layout = layout_with_input(&outer, size, &mut Text, &input);
        input.reconcile(&outer, &layout);
        input.end_scroll_gesture(&outer, &layout, end, now + Duration::from_millis(120));
        assert_eq!(input.refresh_request.is_some(), refresh);
    }
}

#[test]
fn a_nested_touch_pan_only_dismisses_the_keyboard_when_it_reaches_the_opt_in_viewport() {
    use std::time::{Duration, Instant};

    let mut outer = node("ScrollView", "outer");
    boolean(&mut outer, "dismiss-keyboard-on-drag", true);
    let mut inner = node("ScrollView", "inner");
    length(&mut inner, "y", 100.0);
    length(&mut inner, "width", 200.0);
    length(&mut inner, "height", 140.0);
    length(&mut inner, "content-height", 300.0);
    outer.children.push(inner);

    let size = Size::new(320.0, 360.0);
    for (kind, offset, content_height, dismisses) in [
        (ScrollGestureKind::Touch, 40.0, 640.0, false),
        (ScrollGestureKind::Touch, 0.0, 640.0, true),
        (ScrollGestureKind::Touch, 0.0, 200.0, true),
        (ScrollGestureKind::MousePan, 0.0, 640.0, false),
    ] {
        length(&mut outer, "content-height", content_height);
        let mut input = Interaction::default();
        let frame = layout_with_input(&outer, size, &mut Text, &input);
        input.accessibility_scroll_to(&frame, &[0], Orientation::Vertical, offset);
        let frame = layout_with_input(&outer, size, &mut Text, &input);
        let start = Point { x: 50.0, y: 126.0 };
        let end = Point { y: 164.0, ..start };
        let now = Instant::now();
        assert!(input.begin_scroll_gesture(&outer, &frame, start, now, kind));
        input.move_scroll_gesture(&outer, &frame, end, now + Duration::from_millis(40));
        assert_eq!(input.take_scroll_keyboard_dismissal(), dismisses);
        assert!(!input.take_scroll_keyboard_dismissal());
    }
}

#[test]
fn scroll_to_top_uses_the_largest_outer_vertical_viewport() {
    let scroll_view = |id, x, width, height| {
        let mut view = node("ScrollView", id);
        length(&mut view, "x", x);
        length(&mut view, "width", width);
        length(&mut view, "height", height);
        length(&mut view, "content-height", 700.0);
        view
    };
    let mut window = node("Window", "root");
    let sidebar = scroll_view("sidebar", 0.0, 80.0, 140.0);
    let mut main = scroll_view("main", 100.0, 200.0, 300.0);
    main.children
        .push(scroll_view("nested", 10.0, 100.0, 160.0));
    window.children.extend([sidebar, main]);

    let mut input = Interaction::default();
    let frame =
        |input: &Interaction| layout_with_input(&window, Size::new(360.0, 410.0), &mut Text, input);
    let initial = frame(&input);
    for (path, offset) in [(&[0][..], 80.0), (&[1][..], 100.0), (&[1, 0][..], 30.0)] {
        assert!(input.accessibility_scroll_to(&initial, path, Orientation::Vertical, offset));
    }
    assert!(input.scroll_to_top(&window, &frame(&input)));
    let updated = frame(&input);
    for (path, offset) in [(&[0][..], 80.0), (&[1][..], 0.0), (&[1, 0][..], 30.0)] {
        assert_eq!(
            updated.at_path(path).unwrap().scroll.unwrap().offset,
            offset
        );
    }
    assert!(!input.scroll_to_top(&window, &updated));
}

#[test]
fn nested_scroll_views_pass_only_unconsumed_motion_to_the_parent() {
    use xrack_runtime::{Component, ViewportHandle};

    #[derive(Default)]
    struct Listener(Vec<String>);
    impl Component for Listener {
        fn render(&self) -> Node {
            unreachable!("event delivery uses the prepared scene")
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            assert_eq!(event, "scrolled");
            self.0.push(id.into());
            true
        }
    }

    let mut outer = node("ScrollView", "outer");
    let mut inner = node("ScrollView", "inner");
    outer.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    inner.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    for (property, value) in [
        ("width", 100.0),
        ("height", 80.0),
        ("content-width", 140.0),
        ("content-height", 120.0),
    ] {
        length(&mut inner, property, value);
    }
    length(&mut outer, "content-width", 500.0);
    length(&mut outer, "content-height", 400.0);
    outer.children.push(inner);
    let viewport = Size::new(160.0, 120.0);
    let mut input = Interaction::default();
    let render = |input: &Interaction| layout_with_input(&outer, viewport, &mut Text, input);
    let before = render(&input);
    let point = Point { x: 40.0, y: 50.0 };
    input.pointer_move(&outer, &before, point);
    assert!(input.scroll_wheel(&before, Point { x: 55.0, y: 65.0 }, false));
    assert_eq!(input.horizontal_scroll_offsets().get(&[0][..]), Some(&40.0));
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&40.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&15.0));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&25.0));
    let scrolled = render(&input);
    let mut listener = Listener::default();
    for event in input.take_scroll_events(&outer, &scrolled) {
        assert!(event.apply(&mut listener, &outer));
    }
    assert_eq!(listener.0, ["inner", "outer"]);
    input.pointer_move(&outer, &scrolled, point);
    assert!(input.scroll_wheel(&scrolled, Point { x: -50.0, y: -55.0 }, false));
    assert_eq!(input.horizontal_scroll_offsets().get(&[0][..]), Some(&0.0));
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&0.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&5.0));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&10.0));
}

#[test]
fn rtl_scroll_view_bubbles_physical_wheel_remainders_through_an_ltr_child() {
    use xrack_runtime::{LayoutDirection, Orientation};

    let mut outer = node("ScrollView", "outer");
    let mut inner = node("ScrollView", "inner");
    outer
        .properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    inner
        .properties
        .insert("layout-direction", LayoutDirection::LeftToRight.into());
    for (property, value) in [
        ("x", 370.0),
        ("width", 100.0),
        ("height", 80.0),
        ("content-width", 180.0),
    ] {
        length(&mut inner, property, value);
    }
    length(&mut outer, "content-width", 500.0);
    outer.children.push(inner);
    let size = Size::new(160.0, 120.0);
    let render = |input: &Interaction| layout_with_input(&outer, size, &mut Text, input);
    let mut input = Interaction::default();
    let frame = render(&input);
    assert_eq!(frame.children[0].bounds.x, 30.0);
    assert!(input.accessibility_scroll_to(&frame, &[0], Orientation::Horizontal, 35.0));
    let frame = render(&input);
    input.pointer_move(&outer, &frame, Point { x: 90.0, y: 35.0 });
    assert!(input.scroll_x(&frame, -120.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[0][..]), Some(&0.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&85.0));

    let frame = render(&input);
    assert_eq!(frame.children[0].bounds.x, 115.0);
    input.pointer_move(&outer, &frame, Point { x: 135.0, y: 35.0 });
    assert!(input.scroll_x(&frame, 100.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[0][..]), Some(&80.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&65.0));
}

#[test]
fn reversing_a_scroll_view_during_a_thumb_drag_preserves_its_offset() {
    use xrack_runtime::{LayoutDirection, Orientation};

    let mut view = node("ScrollView", "canvas");
    view.properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    length(&mut view, "content-width", 400.0);
    let size = Size::new(200.0, 100.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Horizontal, 30.0));
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let bar = frame.scroll_bars(&view).next().unwrap();
    let start = Point {
        x: bar.thumb.x + 12.0,
        y: bar.thumb.y + 5.0,
    };
    input.pointer_move(&view, &frame, start);
    assert!(matches!(
        input.scroll_bar_press(&view, &frame),
        Some(ScrollBarUpdate::Unchanged)
    ));

    view.properties
        .insert("layout-direction", LayoutDirection::LeftToRight.into());
    let frame = layout_with_input(&view, size, &mut Text, &input);
    input.reconcile(&view, &frame);
    input.pointer_move(
        &view,
        &frame,
        Point {
            x: start.x + 30.0,
            y: start.y,
        },
    );
    assert!(matches!(
        input.scroll_bar_drag(&view, &frame),
        Some(ScrollBarUpdate::Unchanged)
    ));
    input.release_scroll_bar(&view, &frame);
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&30.0));
}

#[test]
fn accessibility_scroll_delivery_does_not_follow_a_replaced_viewport() {
    use xrack_runtime::{Orientation, ViewportHandle};

    let mut view = node("ScrollView", "view");
    view.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    length(&mut view, "content-height", 320.0);
    let viewport = Size::new(160.0, 120.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, viewport, &mut Text, &input);
    assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Vertical, 40.0));
    view.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    let frame = layout_with_input(&view, viewport, &mut Text, &input);
    assert!(input.take_scroll_events(&view, &frame).is_empty());

    assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Vertical, 80.0));
    let frame = layout_with_input(&view, viewport, &mut Text, &input);
    assert_eq!(input.take_scroll_events(&view, &frame).len(), 1);
    assert!(input.take_scroll_events(&view, &frame).is_empty());
}

#[test]
fn swipe_view_yields_to_a_scrollable_page_until_the_next_gesture_at_its_edge() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, Orientation};

    let mut view = node("SwipeView", "pages");
    let mut first = node("SwipePage", "first");
    let mut content = node("ScrollView", "content");
    length(&mut content, "content-width", 340.0);
    first.children.push(content);
    let mut disabled = button("disabled");
    length(&mut disabled, "x", 160.0);
    length(&mut disabled, "y", 45.0);
    boolean(&mut disabled, "enabled", false);
    first.children.push(disabled);
    let mut hidden = node("SwipePage", "hidden");
    boolean(&mut hidden, "visible", false);
    view.children
        .extend([first, hidden, node("SwipePage", "last")]);
    let viewport = Size::new(200.0, 140.0);
    let render = |input: &Interaction| layout_with_input(&view, viewport, &mut Text, input);
    let mut input = Interaction::default();
    let now = Instant::now();
    let start = Point { x: 180.0, y: 55.0 };
    let end = Point { x: 20.0, y: 55.0 };
    let before = render(&input);
    assert!(input.begin_scroll_gesture(&view, &before, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&view, &before, end, now + Duration::from_millis(35)));
    let scrolled = render(&input);
    let offset = scrolled
        .at_path(&[0, 0])
        .unwrap()
        .scroll_on(Orientation::Horizontal)
        .unwrap();
    assert_eq!(offset.offset, offset.max_offset);
    assert!(
        input
            .end_scroll_gesture(&view, &scrolled, end, now + Duration::from_millis(40))
            .control
            .is_none()
    );

    let now = now + Duration::from_millis(80);
    assert!(input.begin_scroll_gesture(&view, &scrolled, start, now, ScrollGestureKind::Touch));
    let release = input.end_scroll_gesture(&view, &scrolled, end, now + Duration::from_millis(40));
    assert!(!release.tap && !release.scrolled);
    assert_eq!(
        release.control,
        Some(ControlInput {
            path: vec![],
            action: ControlAction::Select(2)
        })
    );
    assert!(!input.scroll_motion_active());
}

#[test]
fn swipe_release_distinguishes_flicks_pauses_and_reversals_in_the_local_axis() {
    use std::time::{Duration, Instant};
    use xrack_runtime::ControlAction;

    for (axis, direction, rotation) in [
        (Orientation::Horizontal, LayoutDirection::LeftToRight, 0.0),
        (Orientation::Horizontal, LayoutDirection::RightToLeft, 0.0),
        (Orientation::Vertical, LayoutDirection::RightToLeft, 0.0),
        (Orientation::Horizontal, LayoutDirection::LeftToRight, 90.0),
    ] {
        let mut scene = node("Window", "window");
        let mut view = node("SwipeView", "pages");
        for (property, value) in [
            ("x", 120.0),
            ("y", 120.0),
            ("width", 400.0),
            ("height", 400.0),
        ] {
            length(&mut view, property, value);
        }
        view.properties.extend([
            ("orientation", axis.into()),
            ("layout-direction", direction.into()),
            ("transform-rotation", rotation.into()),
        ]);
        let mut hidden = node("SwipePage", "hidden");
        boolean(&mut hidden, "visible", false);
        view.children.extend([
            node("SwipePage", "one"),
            hidden,
            node("SwipePage", "three"),
            node("SwipePage", "four"),
        ]);
        scene.children.push(view);
        let frame = layout(&scene, Size::new(640.0, 640.0), &mut Text);
        let view = frame.at_path(&[0]).unwrap();
        let start = Point { x: 320.0, y: 320.0 };
        let mirrored = axis == Orientation::Horizontal && direction == LayoutDirection::RightToLeft;
        let point = |along: f32, across: f32| {
            let movement = if mirrored { -along } else { along };
            let local = match axis {
                Orientation::Horizontal => Point {
                    x: start.x - movement,
                    y: start.y - across,
                },
                Orientation::Vertical => Point {
                    x: start.x - across,
                    y: start.y - movement,
                },
            };
            view.map_to_window(local)
        };
        // Points are (along, across, elapsed milliseconds) in the view's local coordinates.
        let gesture = |moves: &[(f32, f32, u64)], release_ms| {
            let mut input = Interaction::default();
            let now = Instant::now();
            assert!(input.begin_scroll_gesture(
                &scene,
                &frame,
                point(0.0, 0.0),
                now,
                ScrollGestureKind::Touch
            ));
            for &(along, across, at) in moves {
                input.move_scroll_gesture(
                    &scene,
                    &frame,
                    point(along, across),
                    now + Duration::from_millis(at),
                );
            }
            let &(along, across, _) = moves.last().unwrap();
            let released = input.end_scroll_gesture(
                &scene,
                &frame,
                point(along, across),
                now + Duration::from_millis(release_ms),
            );
            assert!(!released.tap);
            released.control.map(|control| {
                assert_eq!(control.path, [0]);
                control.action
            })
        };

        let next = Some(ControlAction::Select(2));
        let fast = [(18.0, 0.0, 12), (35.0, 0.0, 24)];
        assert_eq!(
            gesture(&fast, 30),
            next,
            "{axis:?} {direction:?} {rotation}"
        );
        assert_eq!(gesture(&fast, 120), None);
        assert_eq!(gesture(&[(18.0, 0.0, 100), (35.0, 0.0, 200)], 205), None);
        assert_eq!(gesture(&[(35.0, 0.0, 12)], 30), None);
        assert_eq!(gesture(&[(9.0, 0.0, 1), (14.0, 0.0, 2)], 3), None);
        assert_eq!(gesture(&[(18.0, 0.0, 12), (35.0, 19.0, 24)], 30), None);
        assert_eq!(gesture(&[(100.0, 0.0, 18), (66.0, 0.0, 32)], 37), None);
        assert_eq!(gesture(&[(100.0, 0.0, 18), (66.0, 0.0, 32)], 130), next);
        assert_eq!(gesture(&[(180.0, 0.0, 20)], 40), next);
    }
}

#[test]
fn swipe_gesture_cancels_when_a_model_change_replaces_its_destination() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, VecModel};

    let model = VecModel::from(vec![0_i64, 1, 2]);
    let mut view = node("SwipeView", "pages");
    for index in 0..3 {
        let mut page = Node::new("SwipePage", None);
        page.set_row_key(model.row_key(index));
        view.children.push(page);
    }
    let size = Size::new(240.0, 140.0);
    let start = Point { x: 210.0, y: 60.0 };
    let end = Point { x: 25.0, y: 60.0 };
    let now = Instant::now();
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert!(input.begin_scroll_gesture(&view, &frame, start, now, ScrollGestureKind::Touch));
    input.move_scroll_gesture(
        &view,
        &frame,
        Point { x: 170.0, y: 60.0 },
        now + Duration::from_millis(20),
    );

    view.children.swap(1, 2);
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let release = input.end_scroll_gesture(&view, &frame, end, now + Duration::from_millis(80));
    assert!(release.control.is_none());
    assert!(!release.tap);
    assert_eq!(group_index(&view), Some(0));

    let now = now + Duration::from_millis(100);
    assert!(input.begin_scroll_gesture(&view, &frame, start, now, ScrollGestureKind::Touch));
    let release = input.end_scroll_gesture(&view, &frame, end, now + Duration::from_millis(70));
    assert_eq!(release.control.unwrap().action, ControlAction::Select(1));
}

#[test]
fn page_indicator_keeps_logical_marker_identity_in_a_bounded_window() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, Orientation};

    let mut indicator = node("PageIndicator", "pages");
    boolean(&mut indicator, "interactive", true);
    let count = i64::try_from(usize::MAX).unwrap_or(i64::MAX);
    indicator.properties.insert("count", count.into());
    let viewport = Size::new(108.0, 26.0);
    let mut input = Interaction::default();
    let layout = layout(&indicator, viewport, &mut Text);
    let geometry = PageIndicatorGeometry::new(&indicator, layout.bounds).unwrap();
    let markers: Vec<_> = geometry.markers().collect();
    assert_eq!(markers.len(), 6);
    assert!(markers[0].selected);
    for marker in &markers {
        assert_eq!(geometry.marker_at(marker.center), Some(marker.index));
    }
    let wide = PageIndicatorGeometry::new(&indicator, Rect::new(0.0, 0.0, 20_000.0, 30.0)).unwrap();
    assert_eq!(wide.markers().count(), 64);
    let now = Instant::now();
    let boundary = (markers[0].center.x + markers[1].center.x) * 0.5;
    let y = markers[0].center.y;
    assert!(input.begin_scroll_gesture(
        &indicator,
        &layout,
        Point {
            x: boundary - 1.0,
            y
        },
        now,
        ScrollGestureKind::Touch,
    ));
    assert!(
        !input
            .end_scroll_gesture(
                &indicator,
                &layout,
                Point {
                    x: boundary + 1.0,
                    y
                },
                now + Duration::from_millis(25),
            )
            .tap
    );

    input.pointer_move(&indicator, &layout, markers[1].center);
    input.pointer_press();
    indicator
        .properties
        .insert("current-index", (count - 1).into());
    let updated = PageIndicatorGeometry::new(&indicator, layout.bounds).unwrap();
    assert_eq!(
        updated.markers().last().unwrap().index as u64,
        (count - 1) as u64
    );
    assert_ne!(updated.marker_at(markers[1].center), Some(1));
    assert!(input.page_marker_release(&indicator, &layout).is_none());

    indicator
        .properties
        .insert("orientation", Orientation::Vertical.into());
    assert!(
        input
            .page_indicator_key(&indicator, &layout, NavigationKey::Left)
            .is_none()
    );
    assert_eq!(
        input
            .page_indicator_key(&indicator, &layout, NavigationKey::Up)
            .unwrap()
            .action,
        ControlAction::Select((count - 2) as usize)
    );
    assert!(
        input
            .page_indicator_key(&indicator, &layout, NavigationKey::Down)
            .is_none()
    );
    boolean(&mut indicator, "interactive", false);
    input.reconcile(&indicator, &layout);
    assert!(input.focused.is_none());
    indicator.properties.insert("count", (-1_i64).into());
    assert!(PageIndicatorGeometry::new(&indicator, layout.bounds).is_none());
}

#[test]
fn direct_scroll_keeps_the_initial_view_and_passes_residual_motion_to_its_parent() {
    use std::time::{Duration, Instant};
    use xrack_runtime::Orientation;

    let mut outer = node("ScrollView", "outer");
    length(&mut outer, "content-width", 500.0);
    length(&mut outer, "content-height", 480.0);
    let mut inner = node("ScrollView", "inner");
    for (property, value) in [
        ("width", 100.0),
        ("height", 90.0),
        ("content-width", 140.0),
        ("content-height", 130.0),
    ] {
        length(&mut inner, property, value);
    }
    outer.children.push(inner);
    let viewport = Size::new(160.0, 120.0);
    let render = |input: &Interaction| layout_with_input(&outer, viewport, &mut Text, input);
    let mut input = Interaction::default();
    let now = Instant::now();
    let start = Point { x: 55.0, y: 55.0 };
    let end = Point { x: -30.0, y: -30.0 };
    let before = render(&input);
    assert!(input.begin_scroll_gesture(&outer, &before, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&outer, &before, end, now + Duration::from_millis(35)));
    let dragged = render(&input);
    assert_eq!(dragged.children[0].scroll.unwrap().offset, 40.0);
    assert_eq!(
        dragged.children[0]
            .scroll_on(Orientation::Horizontal)
            .unwrap()
            .offset,
        40.0
    );
    let outer_offset = dragged.scroll.unwrap().offset;
    assert!((outer_offset - (85.0 - 8.0 / 2.0_f32.sqrt() - 40.0)).abs() < 0.01);
    let release = input.end_scroll_gesture(&outer, &dragged, end, now + Duration::from_millis(40));
    assert!(!release.tap);
    assert!(input.scroll_motion_active());
    let wake = now + Duration::from_millis(56);
    assert_eq!(input.scroll_motion_deadline(), Some(wake));
    for early in 41..56 {
        assert!(!input.advance_scroll_motion(&outer, &dragged, now + Duration::from_millis(early)));
    }
    assert_eq!(input.scroll_motion_deadline(), Some(wake));
    assert!(input.advance_scroll_motion(&outer, &dragged, wake));
    let advanced = render(&input);
    assert!(advanced.scroll.unwrap().offset > outer_offset);
    assert_eq!(advanced.children[0].scroll.unwrap().offset, 40.0);

    // Touching while coasting arrests motion without activating a control.
    let point = Point { x: 100.0, y: 80.0 };
    assert!(input.begin_scroll_gesture(
        &outer,
        &advanced,
        point,
        now + Duration::from_millis(60),
        ScrollGestureKind::Touch
    ));
    assert!(!input.scroll_motion_active());
    assert!(
        !input
            .end_scroll_gesture(&outer, &advanced, point, now + Duration::from_millis(70))
            .tap
    );
}

#[test]
fn direct_scroll_taps_and_flings_respect_row_identity_and_pause() {
    use std::time::{Duration, Instant};
    use xrack_runtime::VecModel;

    let mut row = node("ListRow", "row");
    let model = VecModel::from(vec![1, 2]);
    row.row_key = model.row_key(0);
    let mut view = node("ScrollView", "view");
    length(&mut view, "content-height", 420.0);
    view.children.push(button("action"));
    row.children.push(view);
    let mut root = node("Window", "root");
    root.children.push(row);
    let viewport = Size::new(160.0, 100.0);
    let render =
        |row: &Node, input: &Interaction| layout_with_input(row, viewport, &mut Text, input);
    let mut input = Interaction::default();
    let now = Instant::now();
    let start = Point { x: 20.0, y: 12.0 };
    let before = render(&root, &input);
    assert!(input.begin_scroll_gesture(&root, &before, start, now, ScrollGestureKind::Touch));
    let nearby = Point { x: 23.0, y: 13.0 };
    assert!(!input.move_scroll_gesture(&root, &before, nearby, now + Duration::from_millis(10)));
    assert!(
        input
            .end_scroll_gesture(&root, &before, nearby, now + Duration::from_millis(20))
            .tap
    );
    assert!(input.begin_scroll_gesture(&root, &before, start, now, ScrollGestureKind::Touch));
    root.children[0].row_key = model.row_key(1);
    assert!(
        !input
            .end_scroll_gesture(&root, &before, nearby, now + Duration::from_millis(20))
            .tap
    );

    let end = Point { x: 20.0, y: -90.0 };
    assert!(input.begin_scroll_gesture(&root, &before, start, now, ScrollGestureKind::Touch));
    root.children[0].row_key = model.row_key(0);
    assert!(!input.move_scroll_gesture(&root, &before, end, now + Duration::from_millis(30)));
    assert!(
        !input
            .end_scroll_gesture(&root, &before, end, now + Duration::from_millis(35))
            .tap
    );
    assert!(!input.scroll_motion_active());

    assert!(input.begin_scroll_gesture(&root, &before, start, now, ScrollGestureKind::MousePan));
    assert!(input.move_scroll_gesture(&root, &before, end, now + Duration::from_millis(30)));
    let dragged = render(&root, &input);
    assert!(
        !input
            .end_scroll_gesture(&root, &dragged, end, now + Duration::from_millis(160))
            .tap
    );
    assert!(!input.scroll_motion_active());

    assert!(input.begin_scroll_gesture(&root, &dragged, start, now, ScrollGestureKind::MousePan));
    assert!(input.move_scroll_gesture(&root, &dragged, end, now + Duration::from_millis(30)));
    let dragged = render(&root, &input);
    input.end_scroll_gesture(&root, &dragged, end, now + Duration::from_millis(35));
    assert!(input.scroll_motion_active());
    for frame in 1..=50 {
        let layout = render(&root, &input);
        input.advance_scroll_motion(&root, &layout, now + Duration::from_millis(35 + frame * 16));
    }
    let settled = render(&root, &input).children[0].children[0]
        .scroll
        .unwrap();
    assert_eq!(settled.offset, settled.max_offset);
    assert!(!input.scroll_motion_active());
}

#[test]
fn dialog_stack_uses_open_order_and_splices_closed_return_targets() {
    let mut root = node("Window", "root");
    root.children.push(button("opener"));
    for (id, child) in [("first", "first-button"), ("second", "second-button")] {
        let mut dialog = node("Dialog", id);
        boolean(&mut dialog, "open", false);
        dialog.children.push(button(child));
        root.children.push(dialog);
    }
    let viewport = Size::new(500.0, 320.0);
    let mut input = Interaction::default();
    input.focus_next(&root, &layout(&root, viewport, &mut Text), false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("opener"));
    boolean(&mut root.children[2], "open", true);
    input.reconcile(&root, &layout(&root, viewport, &mut Text));
    assert_eq!(input.focused.as_ref().unwrap().id, Some("second-button"));
    boolean(&mut root.children[1], "open", true);
    let stacked = layout(&root, viewport, &mut Text);
    input.reconcile(&root, &stacked);
    assert_eq!(
        input.dialog_layers(&stacked).collect::<Vec<_>>(),
        [&[2][..], &[1][..]]
    );
    assert_eq!(input.focused.as_ref().unwrap().id, Some("first-button"));
    boolean(&mut root.children[2], "open", false);
    input.reconcile(&root, &layout(&root, viewport, &mut Text));
    assert_eq!(input.focused.as_ref().unwrap().id, Some("first-button"));
    boolean(&mut root.children[1], "open", false);
    input.reconcile(&root, &layout(&root, viewport, &mut Text));
    assert_eq!(input.focused.as_ref().unwrap().id, Some("opener"));
}

#[test]
fn leading_edge_drawer_reserves_back_only_on_its_physical_edge() {
    use xrack_runtime::DrawerEdge;

    let mut window = node("Window", "root");
    let mut drawer = node("Drawer", "actions");
    length(&mut drawer, "drag-margin", 24.0);
    drawer.properties.insert("edge", DrawerEdge::Left.into());
    window.children.push(drawer);
    let viewport = Size::new(360.0, 660.0);
    let reserves_edge =
        |scene: &Node| layout(scene, viewport, &mut Text).has_leading_edge_drawer(scene);
    assert!(reserves_edge(&window));
    window
        .properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    assert!(!reserves_edge(&window));
    let drawer = &mut window.children[0];
    drawer.properties.insert("edge", DrawerEdge::Right.into());
    assert!(reserves_edge(&window));
    boolean(&mut window.children[0], "enabled", false);
    assert!(!reserves_edge(&window));
}

#[test]
fn drawers_open_from_each_window_edge_and_close_from_the_inner_edge() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{ControlAction, DrawerEdge};

    let viewport = Size::new(360.0, 260.0);
    let now = Instant::now();
    let mut root = node("Window", "root");
    let mut drawer = node("Drawer", "drawer");
    length(&mut drawer, "width", 140.0);
    length(&mut drawer, "height", 110.0);
    length(&mut drawer, "drag-margin", 18.0);
    root.children.push(drawer);
    let p = |x, y| Point { x, y };
    for (edge, start, end, bounds, close_start, close_end) in [
        (
            DrawerEdge::Left,
            p(4.0, 50.0),
            p(85.0, 50.0),
            Rect::new(0.0, 0.0, 140.0, 110.0),
            p(134.0, 50.0),
            p(65.0, 50.0),
        ),
        (
            DrawerEdge::Right,
            p(356.0, 50.0),
            p(275.0, 50.0),
            Rect::new(220.0, 0.0, 140.0, 110.0),
            p(226.0, 50.0),
            p(290.0, 50.0),
        ),
        (
            DrawerEdge::Top,
            p(70.0, 4.0),
            p(70.0, 74.0),
            Rect::new(0.0, 0.0, 140.0, 110.0),
            p(70.0, 104.0),
            p(70.0, 36.0),
        ),
        (
            DrawerEdge::Bottom,
            p(70.0, 256.0),
            p(70.0, 186.0),
            Rect::new(0.0, 150.0, 140.0, 110.0),
            p(70.0, 156.0),
            p(70.0, 220.0),
        ),
    ] {
        root.children[0].properties.insert("edge", edge.into());
        boolean(&mut root.children[0], "open", false);
        let mut input = Interaction::default();
        let closed = layout(&root, viewport, &mut Text);
        assert!(closed.dialog_paths().is_empty());
        assert!(input.begin_scroll_gesture(
            &root,
            &closed,
            start,
            now,
            ScrollGestureKind::MousePan
        ));
        let release =
            input.end_scroll_gesture(&root, &closed, end, now + Duration::from_millis(40));
        assert_eq!(
            release.control,
            Some(ControlInput {
                path: vec![0],
                action: ControlAction::Drawer(true)
            })
        );
        assert!(!release.tap && !input.scroll_motion_active());

        boolean(&mut root.children[0], "open", true);
        let opened = layout(&root, viewport, &mut Text);
        input.reconcile(&root, &opened);
        assert_eq!(opened.children[0].bounds, bounds);
        assert_eq!(opened.dialog_paths(), [vec![0]]);
        assert!(input.begin_scroll_gesture(
            &root,
            &opened,
            close_start,
            now,
            ScrollGestureKind::Touch
        ));
        let release =
            input.end_scroll_gesture(&root, &opened, close_end, now + Duration::from_millis(40));
        assert_eq!(
            release.control,
            Some(ControlInput {
                path: vec![0],
                action: ControlAction::Drawer(false)
            })
        );
    }
}

#[test]
fn vertical_scrolling_from_the_side_of_a_window_does_not_open_a_drawer() {
    use std::time::{Duration, Instant};
    let mut root = node("ScrollView", "view");
    length(&mut root, "content-height", 480.0);
    let mut drawer = node("Drawer", "drawer");
    length(&mut drawer, "drag-margin", 20.0);
    root.children.push(drawer);
    let viewport = Size::new(220.0, 150.0);
    let layout = layout(&root, viewport, &mut Text);
    let mut input = Interaction::default();
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        &root,
        &layout,
        Point { x: 5.0, y: 100.0 },
        now,
        ScrollGestureKind::Touch
    ));
    let release = input.end_scroll_gesture(
        &root,
        &layout,
        Point { x: 10.0, y: 15.0 },
        now + Duration::from_millis(50),
    );
    assert!(release.scrolled);
    assert!(release.control.is_none());
    assert!(
        input
            .scroll_offsets()
            .get(&[][..])
            .is_some_and(|offset| *offset > 70.0)
    );
}

#[test]
fn calendar_paging_matches_recycled_rows_without_a_viewport_handle() {
    use std::rc::Rc;
    use xrack_runtime::{ListMetrics, ListModel, VecModel};

    fn page(model: &Rc<VecModel<i64>>, index: usize) -> Node {
        let mut list = node("ListView", "months");
        list.standard_list_stamp = Some(ListModel::structure_stamp(&**model));
        list.list = Some(ListMetrics {
            row_count: model.len(),
            first_row: index,
        });
        length(&mut list, "row-height", 180.0);
        let mut row = Node::new("ListRow", None);
        row.set_row_key(model.row_key(index));
        let mut grid = node("MonthGrid", "month");
        boolean(&mut grid, "fixed-month", true);
        length(&mut grid, "width", 200.0);
        length(&mut grid, "height", 160.0);
        row.children.push(grid);
        list.children.push(row);
        list
    }

    let model = Rc::new(VecModel::from(vec![1, 2, 3]));
    let size = Size::new(230.0, 180.0);
    let mut input = Interaction::default();
    let january = page(&model, 0);
    let frame = layout_with_input(&january, size, &mut Text, &input);
    let focused = input
        .accessibility_target(&january, &frame, &[0, 0])
        .unwrap();
    input.focus_target(&january, &frame, focused);
    assert!(input.calendar_page_key(&january, &frame, NavigationKey::PageDown));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&180.0));
    let february = page(&model, 1);
    let frame = layout_with_input(&february, size, &mut Text, &input);
    input.reconcile(&february, &frame);
    assert_eq!(input.focused.as_ref().unwrap().row_key, model.row_key(1));
    assert!(input.calendar_page_key(&february, &frame, NavigationKey::PageUp));

    let replacement = page(&Rc::new(VecModel::from(vec![1, 2, 3])), 0);
    let frame = layout_with_input(&replacement, size, &mut Text, &input);
    input.reconcile(&replacement, &frame);
    assert!(
        input
            .focused
            .as_ref()
            .is_none_or(|target| target.element_id() != Some("month"))
    );
}

#[test]
fn retained_selection_follows_its_row_and_clears_when_removed() {
    use xrack_runtime::{FocusHandle, ListMetrics, TextCommand, VecModel};

    let model = VecModel::from(vec![0, 1]);
    let mut list = node("ListView", "list");
    list.list = Some(ListMetrics {
        row_count: 2,
        first_row: 0,
    });
    length(&mut list, "height", 70.0);
    length(&mut list, "row-height", 30.0);
    let selected = FocusHandle::with_text_selection();
    for index in 0..2 {
        let mut row = Node::new("ListRow", None);
        row.set_row_key(model.row_key(index));
        let mut editor = node("LineEdit", "editor");
        editor.focus_handle = Some(if index == 0 {
            selected.clone()
        } else {
            FocusHandle::default()
        });
        editor.properties.insert("text", "same".to_owned().into());
        boolean(&mut editor, "persistent-selection", true);
        length(&mut editor, "width", 120.0);
        length(&mut editor, "height", 28.0);
        row.children.push(editor);
        list.children.push(row);
    }
    let mut toolbar = button("toolbar");
    length(&mut toolbar, "y", 80.0);
    let mut scene = Node::new("Window", None);
    scene.children = vec![list, toolbar];
    let size = Size::new(200.0, 110.0);
    let frame = layout(&scene, size, &mut Text);
    let mut input = Interaction::default();
    for path in [&[0, 0, 0][..], &[1][..]] {
        let target = input.accessibility_target(&scene, &frame, path).unwrap();
        input.focus_target(&scene, &frame, target);
        input.sync_editor(&scene);
        if path.len() == 3 {
            input.apply_text_selection(&scene, TextCommand::SelectAll);
        }
    }
    assert_eq!(selected.selected_text(), "same");
    scene.children[0].children.swap(0, 1);
    let frame = layout(&scene, size, &mut Text);
    input.reconcile(&scene, &frame);
    for (row, expected) in [(0, None), (1, Some(0..4))] {
        let path = [0, row, 0];
        let editor = scene.at_path(&path).unwrap();
        assert_eq!(
            input
                .displayed_editor(editor, &path)
                .map(|edit| edit.selection("same")),
            expected,
        );
    }
    scene.children[0].children.pop();
    if let Some(list) = &mut scene.children[0].list {
        list.row_count = 1;
    }
    let frame = layout(&scene, size, &mut Text);
    input.reconcile(&scene, &frame);
    assert_eq!(selected.selected_text(), "");
}

#[test]
fn text_history_groups_typing_and_survives_focus_changes() {
    use std::time::{Duration, Instant};
    use xrack_runtime::Component;

    struct Fields([String; 2]);
    impl Component for Fields {
        fn render(&self) -> Node {
            let mut root = Node::new("VerticalLayout", None);
            for (index, text) in self.0.iter().enumerate() {
                let mut child = Node::new(if index == 0 { "TextEdit" } else { "LineEdit" }, None);
                child.properties.insert("text", text.clone().into());
                length(&mut child, "height", 40.0);
                root.children.push(child);
            }
            root
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn edit_at(&mut self, _: &Node, path: &[usize], text: String) -> bool {
            self.0[path[0]] = text;
            true
        }
    }

    let mut ui = Fields(["甲".into(), "另一个字段".into()]);
    let mut input = Interaction::default();
    let mut scene = ui.render();
    let geometry = layout(&scene, Size::new(300.0, 120.0), &mut Text);
    let now = Instant::now();
    input.focus_next(&scene, &geometry, false);
    for (i, text) in ["b", "c"].into_iter().enumerate() {
        assert!(input.edit_text(
            &mut ui,
            &scene,
            EditAction::Insert(text),
            EditSource::Keyboard,
            now + Duration::from_millis(i as u64)
        ));
        scene = ui.render();
    }
    assert_eq!(ui.0[0], "甲bc");

    input.focus_next(&scene, &geometry, false);
    assert!(!input.replay_text_edit(&mut ui, &scene, false));
    input.focus_next(&scene, &geometry, false);
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(ui.0[0], "甲");
    assert_eq!(input.editor.as_ref().unwrap().caret(&ui.0[0]), "甲".len());
    scene = ui.render();
    assert!(input.replay_text_edit(&mut ui, &scene, true));
    assert_eq!(ui.0[0], "甲bc");
    scene = ui.render();
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
        EditAction::Insert("中\r\n文"),
        EditSource::Atomic,
        now
    ));
    assert_eq!(ui.0[0], "中\n文");
    scene = ui.render();
    assert!(input.replay_text_edit(&mut ui, &scene, false));
    assert_eq!(
        input.editor.as_ref().unwrap().selection(&ui.0[0]),
        0.."甲bc".len()
    );

    input.focus_next(&ui.render(), &geometry, false);
    ui.0[0] = "外部绑定更新".into();
    scene = ui.render();
    input.focus_next(&scene, &geometry, false);
    assert!(!input.replay_text_edit(&mut ui, &scene, false));
    assert!(!input.replay_text_edit(&mut ui, &scene, true));
    assert_eq!(ui.0[0], "外部绑定更新");
}

#[test]
fn box_padding_spacing_hidden_children_and_resize() {
    let mut root = node("Window", "root");
    let mut column = node("VerticalBox", "column");
    length(&mut column, "padding", 10.0);
    length(&mut column, "spacing", 5.0);
    let mut hidden = node("Rectangle", "hidden");
    boolean(&mut hidden, "visible", false);
    length(&mut hidden, "height", 50.0);
    column.children = vec![
        node("Text", "text"),
        hidden,
        node("Rectangle", "fill"),
        button("button"),
    ];
    root.children.push(column);
    let result = layout(&root, Size::new(100.0, 100.0), &mut Text);
    let children = &result.children[0].children;
    assert_eq!(children[0].bounds, Rect::new(10.0, 10.0, 80.0, 10.0));
    assert!(!children[1].visible);
    assert_eq!(children[2].bounds, Rect::new(10.0, 25.0, 80.0, 40.0));
    assert_eq!(children[3].bounds, Rect::new(10.0, 70.0, 80.0, 20.0));
    let resized = layout(&root, Size::new(100.0, 200.0), &mut Text);
    assert_eq!(resized.children[0].children[2].bounds.height, 140.0);
    assert_eq!(resized.children[0].children[3].bounds.y, 170.0);
}

#[test]
fn passive_editor_text_keeps_alignment_without_measuring_left_aligned_lines() {
    use xrack_runtime::TextHorizontalAlignment::{Center, End, Left, Right, Start};

    let mut cell = node("TableCell", "cell");
    for mode in [Left, Center, Right, Start, End] {
        cell.properties.insert("horizontal-alignment", mode.into());
        for direction in [
            None,
            Some(LayoutDirection::LeftToRight),
            Some(LayoutDirection::RightToLeft),
        ] {
            let alignment = TextAlignment::editor(&cell, direction);
            for text in ["123", "שלום test", "123\u{2029}שלום", "e\u{0301} 👩‍💻"] {
                let visual = VisualTextLine::new(
                    text,
                    text.char_indices().map(|(offset, _)| (offset, 7.32)),
                );
                let expected = alignment.single_line_view_offset(
                    80.0,
                    visual.width(),
                    visual.is_rtl(),
                    None,
                    None,
                );
                let mut measured = false;
                let offset = alignment.passive_line_offset(80.0, text, || {
                    measured = true;
                    visual.width()
                });
                assert_eq!(offset, expected, "{mode:?} {direction:?} {text:?}");
                if mode == Left || mode == Start && direction == Some(LayoutDirection::LeftToRight)
                {
                    assert!(!measured);
                }
            }
        }
    }
}

#[test]
fn linear_layout_redistributes_at_caps_and_shrinks_to_minimums() {
    let mut row = node("HorizontalLayout", "row");
    length(&mut row, "padding-left", 10.0);
    length(&mut row, "padding-right", 20.0);
    length(&mut row, "spacing", 10.0);
    let mut fixed = node("Rectangle", "fixed");
    length(&mut fixed, "width", 50.0);
    let mut capped = node("Rectangle", "capped");
    length(&mut capped, "min-width", 30.0);
    length(&mut capped, "preferred-width", 50.0);
    length(&mut capped, "max-width", 80.0);
    let mut elastic = node("Rectangle", "elastic");
    length(&mut elastic, "min-width", 25.0);
    length(&mut elastic, "preferred-width", 50.0);
    elastic
        .properties
        .insert("horizontal-stretch", Value::Float(2.0));
    row.children = vec![fixed, capped, elastic];
    let widths = |width| {
        let result = layout(&row, Size::new(width, 50.0), &mut Text);
        result
            .children
            .iter()
            .map(|child| child.bounds.width)
            .collect::<Vec<_>>()
    };
    assert_eq!(widths(330.0), [50.0, 80.0, 150.0]);
    assert_eq!(widths(170.0), [50.0, 40.0, 30.0]);
    assert_eq!(widths(130.0), [50.0, 30.0, 25.0]);

    let mut nested = node("HorizontalLayout", "nested");
    let mut tile = node("Rectangle", "tile");
    length(&mut tile, "width", 30.0);
    nested.children.push(tile);
    row.children = vec![nested, node("Rectangle", "sibling")];
    let result = layout(&row, Size::new(160.0, 50.0), &mut Text);
    assert_eq!(result.children[0].bounds.width, 30.0);
    assert_eq!(result.children[1].bounds.width, 90.0);
}

#[test]
fn long_linear_layout_keeps_natural_row_sizes_when_the_tail_grows() {
    let mut column = node("VerticalLayout", "transcript");
    length(&mut column, "padding-top", 11.13);
    let mut history = node("Rectangle", "history");
    length(&mut history, "preferred-height", 212.4);
    column.children = vec![history; 119];
    column.children.push(node("Rectangle", "latest"));
    let reference = Size::new(740.0, 509.0);

    for gap in [0.0, 1.35] {
        length(&mut column, "spacing", gap);
        for alignment in [LayoutAlignment::Start, LayoutAlignment::Stretch] {
            column.properties.insert("alignment", alignment.into());
            for height in [153.6, 192.8, 232.0, 584.8] {
                length(&mut column.children[119], "preferred-height", height);
                let natural = linear::measure(&column, reference, &mut Text).y.preferred;
                let place = |height| {
                    linear::children(
                        &column,
                        linear::Axis::Vertical,
                        Rect::new(0.0, 0.0, reference.width, height),
                        LayoutDirection::LeftToRight,
                        &mut Text,
                    )
                };
                let rows = place(natural);
                assert!(rows[..119].iter().all(|row| row.height == 212.4_f32));
                assert_eq!(rows[119].height, height as f32);
                let tail_end = rows[119].y + rows[119].height;
                assert!((tail_end - natural).abs() <= natural * f32::EPSILON);
                let compressed = place(natural - 24.0);
                assert!(compressed[0].height < rows[0].height);
            }
        }
    }
}

#[test]
fn visual_order_and_axis_alignment_preserve_focus_order() {
    let mut column = node("VerticalLayout", "column");
    length(&mut column, "padding-left", 10.0);
    length(&mut column, "padding-right", 20.0);
    length(&mut column, "padding-top", 5.0);
    length(&mut column, "padding-bottom", 15.0);
    column
        .properties
        .insert("alignment", LayoutAlignment::SpaceEvenly.into());
    column
        .properties
        .insert("cross-axis-alignment", CrossAxisAlignment::End.into());
    let mut first = button("first");
    length(&mut first, "width", 40.0);
    let mut second = button("second");
    length(&mut second, "width", 30.0);
    length(&mut second, "height", 30.0);
    second.properties.insert("layout-order", Value::Int(-1));
    second.properties.insert(
        "cross-axis-self-alignment",
        CrossAxisAlignment::Center.into(),
    );
    let mut hidden = button("hidden");
    boolean(&mut hidden, "visible", false);
    column.children = vec![first, second, hidden];

    let result = layout(&column, Size::new(120.0, 130.0), &mut Text);
    assert_eq!(result.children[0].bounds, Rect::new(60.0, 75.0, 40.0, 20.0));
    assert_eq!(result.children[1].bounds, Rect::new(40.0, 25.0, 30.0, 30.0));
    let mut input = Interaction::default();
    input.focus_next(&column, &result, false);
    assert_eq!(input.focused.unwrap().id, Some("first"));
}

#[test]
fn topmost_disabled_control_blocks_underlying_control() {
    let mut root = node("Window", "root");
    let mut disabled = button("disabled");
    boolean(&mut disabled, "enabled", false);
    root.children = vec![button("under"), disabled];
    let mut input = Interaction::default();
    input.pointer_move(
        &root,
        &layout(&root, Size::new(100.0, 100.0), &mut Text),
        Point { x: 10.0, y: 10.0 },
    );
    assert!(input.hovered.is_none());
    boolean(&mut root.children[1], "visible", false);
    let bounds = layout(&root, Size::new(100.0, 100.0), &mut Text);
    input.pointer_move(&root, &bounds, Point { x: 10.0, y: 10.0 });
    input.pointer_press();
    input.pointer_move(&root, &bounds, Point { x: 90.0, y: 90.0 });
    assert_eq!(input.pointer_release(), None);
    input.pointer_move(&root, &bounds, Point { x: 10.0, y: 10.0 });
    input.pointer_press();
    assert_eq!(input.pointer_release(), Some("under"));
}

#[test]
fn ancestor_clip_prevents_clicking_overflowing_children() {
    let mut root = node("Window", "root");
    let mut panel = node("Rectangle", "panel");
    length(&mut panel, "width", 20.0);
    length(&mut panel, "height", 20.0);
    let mut child = button("child");
    length(&mut child, "x", 30.0);
    panel.children.push(child);
    root.children.push(panel);
    let bounds = layout(&root, Size::new(100.0, 100.0), &mut Text);
    let mut input = Interaction::default();
    input.pointer_move(&root, &bounds, Point { x: 35.0, y: 5.0 });
    assert!(input.hovered.is_none());
}

#[test]
fn keyboard_focus_skips_disabled_and_hidden_controls() {
    let mut root = node("Window", "root");
    let mut disabled = button("disabled");
    boolean(&mut disabled, "enabled", false);
    let mut hidden = button("hidden");
    boolean(&mut hidden, "visible", false);
    root.children = vec![button("first"), disabled, hidden, button("last")];
    let bounds = layout(&root, Size::new(100.0, 100.0), &mut Text);
    let mut input = Interaction::default();
    input.focus_next(&root, &bounds, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("first"));
    input.key_press(ActivationKey::Space);
    assert_eq!(input.key_release(ActivationKey::Enter), None);
    assert_eq!(input.key_release(ActivationKey::Space), Some("first"));
    input.focus_next(&root, &bounds, false);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("last"));
    input.focus_next(&root, &bounds, true);
    assert_eq!(input.focused.as_ref().unwrap().id, Some("first"));
}

#[test]
fn model_change_cancels_capture_without_retargeting_a_click() {
    let mut root = node("Window", "root");
    root.children.push(button("before"));
    let mut input = Interaction::default();
    let bounds = layout(&root, Size::new(100.0, 100.0), &mut Text);
    input.pointer_move(&root, &bounds, Point { x: 10.0, y: 10.0 });
    input.pointer_press();
    root.children[0].id = Some("after");
    input.reconcile(&root, &bounds);
    assert_eq!(input.pointer_release(), None);
    input.focus_next(&root, &bounds, false);
    input.key_press(ActivationKey::Enter);
    boolean(&mut root.children[0], "enabled", false);
    input.reconcile(&root, &layout(&root, Size::new(100.0, 100.0), &mut Text));
    assert_eq!(input.key_release(ActivationKey::Enter), None);
}

#[test]
fn runtime_element_ids_preserve_activation_and_reject_a_replacement() {
    let mut root = node("Window", "root");
    let mut control = button("unused");
    control.set_owned_id(Some(String::from("original").into()));
    root.children.push(control);
    let size = Size::new(100.0, 100.0);
    let point = Point { x: 10.0, y: 10.0 };
    let bounds = layout(&root, size, &mut Text);
    let mut input = Interaction::default();

    input.pointer_move(&root, &bounds, point);
    input.pointer_press_in_scene(&root, &bounds);
    let target = input.pointer_release_target().unwrap();
    assert_eq!(target.element_id(), Some("original"));
    assert_eq!(
        target,
        Target::from_node(vec![0], &button("original"), true, None)
    );

    input.pointer_press_in_scene(&root, &bounds);
    let mut replacement = root.clone();
    replacement.children[0].set_owned_id(Some(String::from("replacement").into()));
    let new_bounds = layout(&replacement, size, &mut Text);
    input.reconcile(&replacement, &new_bounds);
    assert!(input.pointer_release_target().is_none());

    input.focus_next(&root, &bounds, false);
    input.key_press_with_scene(&root, ActivationKey::Space);
    assert!(
        input
            .key_release_with_scene(&replacement, ActivationKey::Space)
            .is_none()
    );
    input.reconcile(&replacement, &new_bounds);
    input.focus_next(&replacement, &new_bounds, false);
    input.key_press_with_scene(&replacement, ActivationKey::Space);
    assert_eq!(
        input
            .key_release_with_scene(&replacement, ActivationKey::Space)
            .unwrap()
            .element_id(),
        Some("replacement")
    );
}

#[test]
fn scroll_capture_does_not_transfer_to_a_different_runtime_view_at_the_same_path() {
    use std::time::{Duration, Instant};

    let mut root = node("Window", "root");
    let mut view = Node::new("ScrollView", None);
    view.set_owned_id(Some("original".into()));
    length(&mut view, "width", 140.0);
    length(&mut view, "height", 120.0);
    length(&mut view, "content-height", 400.0);
    root.children.push(view);
    let size = Size::new(160.0, 140.0);
    let mut input = Interaction::default();
    let start = Point { x: 50.0, y: 80.0 };
    let end = Point { x: 50.0, y: 30.0 };
    let now = Instant::now();
    let bounds = layout(&root, size, &mut Text);
    assert!(input.begin_scroll_gesture(&root, &bounds, start, now, ScrollGestureKind::Touch));

    root.children[0].set_owned_id(Some("replacement".into()));
    let bounds = layout(&root, size, &mut Text);
    assert!(!input.move_scroll_gesture(&root, &bounds, end, now + Duration::from_millis(20)));
    assert!(
        !input
            .end_scroll_gesture(&root, &bounds, end, now + Duration::from_millis(40))
            .scrolled
    );
    assert_eq!(
        input
            .scroll_offsets()
            .get(&[0][..])
            .copied()
            .unwrap_or_default(),
        0.0
    );

    assert!(input.begin_scroll_gesture(
        &root,
        &bounds,
        start,
        now + Duration::from_millis(60),
        ScrollGestureKind::Touch,
    ));
    assert!(input.move_scroll_gesture(&root, &bounds, end, now + Duration::from_millis(80)));
    assert!(
        input
            .scroll_offsets()
            .get(&[0][..])
            .is_some_and(|offset| *offset > 0.0)
    );
}

#[test]
fn deferred_drop_rejects_a_replacement_with_a_different_runtime_id() {
    use xrack_runtime::{Component, DragEvent};

    #[derive(Default)]
    struct Receiver(usize);
    impl Component for Receiver {
        fn render(&self) -> Node {
            Node::new("Window", None)
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn drag_event_at(&mut self, _: &Node, _: &[usize], event: &DragEvent) -> bool {
            if matches!(event, DragEvent::Dropped { .. }) {
                self.0 += 1;
            }
            true
        }
    }

    let mut root = node("Window", "root");
    let mut source = node("DragArea", "source");
    boolean(&mut source, "allow-copy", true);
    let mut target = Node::new("DropArea", None);
    target.set_owned_id(Some("receiver".into()));
    for area in [&mut source, &mut target] {
        length(area, "width", 70.0);
        length(area, "height", 45.0);
    }
    length(&mut target, "x", 90.0);
    root.children.extend([source, target]);

    let frame = layout(&root, Size::new(180.0, 100.0), &mut Text);
    let mut input = Interaction::default();
    input.arm_drag(&root, &frame, Point { x: 25.0, y: 25.0 });
    let end = Point { x: 112.0, y: 24.0 };
    let update = input.drag_motion(&root, &frame, end, None);
    assert!(update.captured);
    let drop = input
        .finish_drag(&root, &frame, end, None)
        .events
        .into_iter()
        .find(|delivery| matches!(delivery.event, DragEvent::Dropped { .. }))
        .unwrap();
    assert_eq!(drop.target.element_id(), Some("receiver"));
    let mut receiver = Receiver::default();
    let old = root.clone();
    root.children[1].set_owned_id(Some("replacement".into()));
    let result = DragDispatch::new(drop.target.clone(), drop.event.clone());
    assert!(!result.apply(&mut receiver, &root));
    assert!(drop.apply(&mut receiver, &old));
    assert_eq!(receiver.0, 1);
}

#[test]
fn removing_an_opted_in_drag_source_cancels_against_the_live_row_model() {
    use std::{rc::Rc, time::Instant};
    use xrack_runtime::{
        DragAction, DragEvent, ListMetrics, ListRowLocator, ListRowRenderer, VecModel,
    };

    let model = Rc::new(VecModel::from(vec![String::from("Pump")]));
    let mut list = node("ListView", "list");
    boolean(&mut list, "drag-auto-scroll", true);
    list.list = Some(ListMetrics {
        row_count: 1,
        first_row: 0,
    });
    list.row_locator = Some(ListRowLocator::new(model.clone()));
    list.list_row_renderer = Some(ListRowRenderer::new(|_| Vec::new()));
    let mut row = node("ListRow", "pump");
    row.row_key = model.row_key(0);
    let mut source = node("DragArea", "source");
    boolean(&mut source, "allow-move", true);
    length(&mut source, "width", 100.0);
    length(&mut source, "height", 26.0);
    row.children.push(source);
    list.children.push(row);

    let frame = layout(&list, Size::new(140.0, 120.0), &mut Text);
    let mut input = Interaction::default();
    let now = Instant::now();
    input.arm_touch_drag(&list, &frame, Point { x: 10.0, y: 10.0 }, now);
    let end = Point { x: 40.0, y: 10.0 };
    assert!(input.drag_motion_at(&list, &frame, end, None, now).captured);

    model.remove(0);
    // The previous prepared frame still contains the source slot.
    let update = input.reconcile_drag(&list, &frame, None);
    assert!(update.captured);
    assert!(matches!(update.events.as_slice(), [event]
        if event.event == DragEvent::Finished(DragAction::None)));
    assert!(input.drag_scroll_deadline(&list, &frame).is_none());
    assert!(
        input
            .finish_drag(&list, &frame, end, None)
            .events
            .is_empty()
    );
}

#[test]
fn paint_edge_trimming_preserves_middle_slots_and_explicit_order() {
    let mut window = Node::new("Window", None);
    window.children = ["prefix", "first", "middle", "last", "suffix"]
        .map(|id| Node::new("Rectangle", Some(id)))
        .into();
    let viewport = Size::new(120.0, 80.0);
    let discard = |child: &Node, _: &LayoutNode| {
        matches!(child.element_id(), Some("prefix" | "middle" | "suffix"))
    };
    let frame = layout(&window, viewport, &mut Text);
    let trimmed = frame
        .children_in_paint_order_trimming_edges(&window, true, discard)
        .map(|(index, _, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(trimmed, [1, 2, 3]);

    window.set_static_paint_order(&[4, 3, 2, 1, 0]);
    let frame = layout(&window, viewport, &mut Text);
    let ordered = frame
        .children_in_paint_order_trimming_edges(&window, true, discard)
        .map(|(index, _, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(ordered, [3, 2, 1]);
    assert_eq!(
        frame
            .children_in_paint_order_trimming_edges(&window, false, discard)
            .map(|(index, _, _)| index)
            .collect::<Vec<_>>(),
        [4, 3, 2, 1, 0]
    );
    assert!(
        frame
            .children_in_paint_order_trimming_edges(&window, true, |_, _| true)
            .next()
            .is_none()
    );
}

#[test]
fn overlapping_scroll_views_route_wheel_input_in_paint_order() {
    let mut window = node("Window", "root");
    for id in ["first", "second"] {
        let mut view = node("ScrollView", id);
        length(&mut view, "width", 120.0);
        length(&mut view, "height", 100.0);
        length(&mut view, "content-height", 300.0);
        window.children.push(view);
    }
    window.children[0].properties.insert("z", 1.0_f64.into());
    window.set_static_paint_order(&[1, 0]);
    let point = Point { x: 20.0, y: 20.0 };
    let viewport = Size::new(160.0, 120.0);
    let frame = layout(&window, viewport, &mut Text);
    assert!(matches!(
        frame.paint_order(),
        Some(&xrack_runtime::ScenePlanSlice::Static(&[1, 0]))
    ));
    let mut input = Interaction::default();
    input.pointer_move(&window, &frame, point);
    assert!(input.scroll_wheel(&frame, Point { x: 0.0, y: 32.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&32.0));
    assert!(!input.scroll_offsets().contains_key(&[1][..]));
    window.children[0].properties.insert("z", 0.0_f64.into());
    assert!(window.static_paint_order().is_none());
    let frame = layout(&window, viewport, &mut Text);
    assert_eq!(frame.child_paint_order().collect::<Vec<_>>(), [0, 1]);
    window.set_natural_paint_order();
    assert!(window.has_natural_paint_order());
    assert!(layout(&window, viewport, &mut Text).paint_order().is_none());
    window.children[0].properties.insert("z", 2.0_f64.into());
    assert!(!window.has_natural_paint_order());
    let changed = layout(&window, viewport, &mut Text);
    assert_eq!(changed.child_paint_order().collect::<Vec<_>>(), [1, 0]);
}

#[test]
fn materialized_rows_keep_their_rounded_ancestor_clip() {
    use std::ops::Range;
    use xrack_runtime::{Component, ListMetrics};

    struct Rows;
    impl Component for Rows {
        fn render(&self) -> Node {
            let mut window = node("Window", "root");
            let mut mask = node("Rectangle", "mask");
            boolean(&mut mask, "clip", true);
            length(&mut mask, "width", 100.0);
            length(&mut mask, "height", 100.0);
            length(&mut mask, "border-radius", 40.0);
            let mut list = node("ListView", "list");
            length(&mut list, "width", 100.0);
            length(&mut list, "height", 100.0);
            length(&mut list, "row-height", 20.0);
            list.list = Some(ListMetrics {
                row_count: 20,
                first_row: 0,
            });
            mask.children.push(list);
            window.children.push(mask);
            window
        }

        fn list_rows(&self, path: &[usize], range: Range<usize>) -> Vec<Node> {
            assert_eq!(path, &[0, 0]);
            range.map(|_| button("delegate")).collect()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let rows = Rows;
    let mut input = Interaction::default();
    let (scene, layout) = prepare_scene_with_input(
        &rows,
        rows.render_shared(),
        Size::new(120.0, 120.0),
        &mut Text,
        &input,
    );
    let first = layout.at_path(&[0, 0, 0, 0]).unwrap();
    let hidden = Point { x: 2.0, y: 2.0 };
    assert!(!first.contains_clip_point(hidden));
    input.pointer_move(&scene, &layout, hidden);
    assert!(input.hovered.is_none());
    let visible = Point { x: 50.0, y: 10.0 };
    assert!(first.contains_clip_point(visible));
    input.pointer_move(&scene, &layout, visible);
    assert_eq!(input.hovered.as_ref().unwrap().id, Some("delegate"));
}

#[test]
fn rotating_a_scroll_view_maps_pan_and_inertia_to_its_local_axis() {
    use std::time::{Duration, Instant};
    use xrack_runtime::ViewportHandle;

    let mut root = node("Window", "root");
    let mut scroller = node("ScrollView", "scroller");
    scroller.viewport_handle = Some(ViewportHandle::with_scroll_notifications());
    for (name, value) in [
        ("x", 150.0),
        ("y", 20.0),
        ("width", 100.0),
        ("height", 100.0),
        ("content-height", 300.0),
        ("transform-origin-x", 0.0),
        ("transform-origin-y", 0.0),
    ] {
        length(&mut scroller, name, value);
    }
    scroller
        .properties
        .insert("transform-rotation", 90.0_f64.into());
    root.children.push(scroller);
    let viewport = Size::new(220.0, 150.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    let start = Instant::now();
    let origin = Point { x: 88.0, y: 70.0 };
    assert!(input.begin_scroll_gesture(&root, &frame, origin, start, ScrollGestureKind::Touch));
    let end = Point { x: 128.0, y: 70.0 };
    assert!(input.move_scroll_gesture(&root, &frame, end, start + Duration::from_millis(20)));
    let offset = input.scroll_offsets().get(&[0][..]).copied().unwrap();
    assert!(offset > 20.0);
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    assert_eq!(input.take_scroll_events(&root, &frame).len(), 1);
    input.end_scroll_gesture(&root, &frame, end, start + Duration::from_millis(21));
    assert!(input.scroll_motion_active());
    assert!(input.advance_scroll_motion(&root, &frame, start + Duration::from_millis(50)));
    assert!(input.scroll_offsets().get(&[0][..]).copied().unwrap() > offset);
    let frame = layout_with_input(&root, viewport, &mut Text, &input);
    assert_eq!(input.take_scroll_events(&root, &frame).len(), 1);
}

#[test]
fn logical_text_alignment_preserves_natural_direction_and_aligns_editor_geometry() {
    use xrack_runtime::{LayoutDirection, TextHorizontalAlignment};

    let measure = |text: &str| text.chars().count() as f32 * 10.0;
    let mut label = node("Label", "label");
    label.properties.insert("text", "abc".to_owned().into());
    let size = Size::new(100.0, 40.0);
    let rtl = Some(LayoutDirection::RightToLeft);
    assert_eq!(
        LabelLayout::in_direction(&label, size, 12.0, rtl, measure).lines[0]
            .origin
            .x,
        70.0
    );
    label
        .properties
        .insert("horizontal-alignment", TextHorizontalAlignment::Left.into());
    assert_eq!(
        LabelLayout::in_direction(&label, size, 12.0, rtl, measure).lines[0]
            .origin
            .x,
        0.0
    );
    label.properties.remove("horizontal-alignment");
    label.properties.insert("text", "שלום".to_owned().into());
    assert_eq!(
        LabelLayout::in_direction(&label, size, 12.0, None, measure).lines[0]
            .origin
            .x,
        60.0
    );

    let editor = node("TextArea", "editor");
    let text = "abc\nx";
    let geometry = TextAreaLayout::new(
        text,
        Rect::new(0.0, 0.0, 100.0, 40.0),
        20.0,
        xrack_runtime::TextWrap::NoWrap,
        measure,
        str::len,
    )
    .aligned(text, TextAlignment::editor(&editor, rtl), measure);
    let mut caret = TextEdit::default();
    caret.place_caret(
        text,
        TextPosition {
            offset: 1,
            trailing: false,
        },
        false,
    );
    assert_eq!(
        geometry.caret_point(text, caret.position(text), measure).x,
        79.0
    );
    assert_eq!(
        geometry
            .hit(text, Point { x: 45.0, y: 6.0 }, Point::default(), measure)
            .offset,
        0
    );
    geometry.move_caret(text, &mut caret, TextAreaMotion::Down, false, measure);
    assert_eq!(caret.caret(text), 4);
    let left = geometry.aligned(text, TextAlignment::editor(&editor, None), measure);
    assert_eq!(left.caret_point(text, caret.position(text), measure).x, 0.0);
}

#[test]
fn rtl_tab_bar_reveals_the_leading_edge_of_an_oversized_tab() {
    use xrack_runtime::LayoutDirection;

    let mut bar = node("TabBar", "bar");
    bar.properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    let mut tab = node("TabButton", "oversized");
    length(&mut tab, "width", 180.0);
    bar.children.push(tab);
    let frame = layout(&bar, Size::new(100.0, 40.0), &mut Text);
    assert_eq!(
        frame.scroll_on(Orientation::Horizontal).unwrap().offset,
        80.0
    );
    assert_eq!(
        frame.child_at(0).unwrap().bounds,
        Rect::new(-80.0, 0.0, 180.0, 40.0)
    );
}

#[test]
fn vertical_touch_on_a_switch_track_scrolls_the_viewport() {
    use std::time::{Duration, Instant};

    let mut view = node("ScrollView", "viewport");
    length(&mut view, "content-height", 300.0);
    let mut toggle = node("Switch", "wifi");
    length(&mut toggle, "width", 100.0);
    length(&mut toggle, "height", 32.0);
    view.children.push(toggle);
    let frame = layout(&view, Size::new(140.0, 100.0), &mut Text);
    let mut input = Interaction::default();
    let start = Point { x: 24.0, y: 16.0 };
    let end = Point { x: 24.0, y: -24.0 };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&view, &frame, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&view, &frame, end, now + Duration::from_millis(30)));
    assert!(!input.has_switch_preview());
    let release = input.end_scroll_gesture(&view, &frame, end, now + Duration::from_millis(60));
    assert!(release.control.is_none() && !release.tap);
    assert!(input.scroll_offsets().values().any(|offset| *offset > 0.0));
}

#[test]
fn rtl_table_passes_unconsumed_horizontal_wheel_motion_to_its_ltr_parent() {
    use xrack_runtime::{LayoutDirection, TableHeader};

    let mut outer = node("ScrollView", "outer");
    length(&mut outer, "content-width", 600.0);
    let mut table = node("TableView", "table");
    length(&mut table, "x", 140.0);
    length(&mut table, "width", 220.0);
    length(&mut table, "height", 110.0);
    table
        .properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    let columns = (0..2)
        .map(|_| {
            let mut column = Node::new("TableColumn", None);
            length(&mut column, "width", 150.0);
            column
        })
        .collect();
    table.table_header = Some(Box::new(TableHeader {
        columns,
        sort: None,
        column_widths: vec![],
    }));
    outer.children.push(table);
    let viewport = Size::new(320.0, 140.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&outer, viewport, &mut Text, &input);
    assert!(input.accessibility_scroll_to(&frame, &[], Orientation::Horizontal, 100.0));
    let frame = layout_with_input(&outer, viewport, &mut Text, &input);
    let table = frame.child_at(0).unwrap().bounds;
    input.pointer_move(
        &outer,
        &frame,
        Point {
            x: table.x + 20.0,
            y: table.y + 12.0,
        },
    );
    assert!(input.scroll_x(&frame, -120.0));
    assert_eq!(input.horizontal_scroll_offsets()[&[0][..]], 80.0);
    assert_eq!(input.horizontal_scroll_offsets()[&[][..]], 60.0);
}

#[test]
fn scroll_artwork_does_not_register_its_shortcuts() {
    let mut window = Node::new("Window", None);
    let mut bar = node("ScrollBar", "bar");
    length(&mut bar, "width", 16.0);
    length(&mut bar, "height", 120.0);
    bar.properties.insert("size", Value::Float(0.25));
    let mut thumb = node("ScrollThumb", "artwork");
    let mut shortcut = node("Shortcut", "decoration-shortcut");
    shortcut
        .properties
        .insert("sequence", "F8".to_owned().into());
    thumb.children.push(shortcut.clone());
    bar.children.push(thumb);
    window.children.push(bar);

    let size = Size::new(200.0, 150.0);
    let frame = layout(&window, size, &mut Text);
    assert!(frame.children[0].children[0].visible);
    let mut input = Interaction::default();
    let key = "F8".parse().unwrap();
    let now = std::time::Instant::now();
    assert_eq!(
        input.shortcut_press(&window, &frame, key, false, now),
        ShortcutOutcome::Unmatched
    );
    window.children.push(shortcut);
    let frame = layout(&window, size, &mut Text);
    assert!(matches!(
        input.shortcut_press(&window, &frame, key, false, now),
        ShortcutOutcome::Activate(_)
    ));
}

#[test]
fn scroll_activity_tracks_each_axis_and_expires_without_a_layout_change() {
    use std::time::{Duration, Instant};
    use xrack_runtime::{GeometryHandle, Orientation, ViewportHandle};

    let mut scene = Node::new("Window", None);
    let viewport = ViewportHandle::default();
    let mut view = node("ScrollView", "view");
    for (name, value) in [
        ("width", 100.0),
        ("height", 70.0),
        ("content-width", 300.0),
        ("content-height", 230.0),
    ] {
        length(&mut view, name, value);
    }
    view.viewport_handle = Some(viewport.clone());
    scene.children.push(view);
    let make_control = |kind, axis: Orientation, x, y| {
        let mut bar = Node::new(kind, None);
        let handle = GeometryHandle::default();
        assert!(!handle.scroll_active());
        bar.geometry_handle = Some(handle.clone());
        bar.scroll_target = Some(viewport.clone());
        bar.properties.insert("orientation", axis.into());
        length(&mut bar, "x", x);
        length(&mut bar, "y", y);
        length(&mut bar, "width", 100.0);
        length(&mut bar, "height", 12.0);
        (bar, handle)
    };
    let (bar, horizontal) = make_control("ScrollBar", Orientation::Horizontal, 0.0, 80.0);
    let (indicator, passive) = make_control("ScrollIndicator", Orientation::Horizontal, 0.0, 100.0);
    let (vertical_bar, vertical) =
        make_control("ScrollIndicator", Orientation::Vertical, 120.0, 0.0);
    scene.children.extend([bar, indicator, vertical_bar]);
    let size = Size::new(240.0, 150.0);
    let mut input = Interaction::default();
    let mut frame = layout_with_input(&scene, size, &mut Text, &input);
    let mut observers = LayoutObservers::default();
    let now = Instant::now();
    observers.sync(&scene, &frame);
    observers.sync_scroll_activity(&scene, &frame, &input, now);
    assert_eq!(observers.scroll_activity_deadline(), None);

    let point = Point { x: 10.0, y: 86.0 };
    input.pointer_move(&scene, &frame, point);
    observers.sync_scroll_activity(&scene, &frame, &input, now);
    assert!(horizontal.scroll_hovered() && horizontal.scroll_active());
    assert!(!passive.scroll_active());
    assert!(input.scroll_bar_press(&scene, &frame).is_some());
    observers.sync_scroll_activity(&scene, &frame, &input, now);
    assert!(horizontal.scroll_pressed());
    input.finish_scroll_bar();
    input.pointer_leave();
    observers.sync_scroll_activity(&scene, &frame, &input, now);
    assert!(!horizontal.scroll_pressed() && !horizontal.scroll_hovered());
    assert!(!horizontal.scroll_active());

    assert!(input.set_viewport_offset(&[0], &frame.children[0], Orientation::Horizontal, 120.0));
    frame = layout_with_input(&scene, size, &mut Text, &input);
    observers.sync(&scene, &frame);
    observers.sync_scroll_activity(&scene, &frame, &input, now);
    assert!(horizontal.scroll_active() && passive.scroll_active());
    assert!(!vertical.scroll_active());
    assert_eq!(
        observers.scroll_activity_deadline(),
        Some(now + Duration::from_millis(180))
    );
    observers.sync_scroll_activity(&scene, &frame, &input, now + Duration::from_millis(179));
    assert!(horizontal.scroll_active());
    observers.sync_scroll_activity(&scene, &frame, &input, now + Duration::from_millis(180));
    assert!(!horizontal.scroll_active() && !passive.scroll_active());
    assert_eq!(observers.scroll_activity_deadline(), None);

    input.pointer_move(&scene, &frame, point);
    assert!(input.scroll_bar_press(&scene, &frame).is_some());
    observers.sync_scroll_activity(&scene, &frame, &input, now + Duration::from_millis(181));
    assert!(horizontal.scroll_pressed());
    boolean(&mut scene.children[0], "visible", false);
    frame = layout_with_input(&scene, size, &mut Text, &input);
    input.reconcile(&scene, &frame);
    observers.sync(&scene, &frame);
    observers.sync_scroll_activity(&scene, &frame, &input, now + Duration::from_millis(182));
    assert!(!horizontal.scroll_active() && !horizontal.scroll_pressed());
    boolean(&mut scene.children[0], "visible", true);
    frame = layout_with_input(&scene, size, &mut Text, &input);
    input.reconcile(&scene, &frame);
    observers.sync(&scene, &frame);
    observers.sync_scroll_activity(&scene, &frame, &input, now + Duration::from_millis(183));
    assert!(
        !horizontal.scroll_pressed(),
        "a cancelled capture cannot reactivate"
    );
}

#[test]
fn relative_scroll_batch_uses_committed_unbound_offsets_and_clamps_each_step() {
    use xrack_runtime::{ScrollAction, ScrollBarStep, ScrollRequest, ViewportHandle};
    let mut view = node("ScrollView", "view");
    let handle = ViewportHandle::default();
    view.viewport_handle = Some(handle.clone());
    length(&mut view, "content-width", 500.0);
    length(&mut view, "content-height", 400.0);
    let mut input = Interaction::default();
    let size = Size::new(100.0, 80.0);
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let actions = [
        ScrollAction::To {
            x: Length(390.0),
            y: Length(5.0),
        },
        ScrollAction::StepBar {
            axis: Orientation::Horizontal,
            step_size: Some(0.1),
            direction: ScrollBarStep::Increase,
        },
        ScrollAction::By {
            dx: Length(-40.0),
            dy: Length(-20.0),
        },
        ScrollAction::StepBar {
            axis: Orientation::Horizontal,
            step_size: None,
            direction: ScrollBarStep::Increase,
        },
        ScrollAction::By {
            dx: Length(-8.0),
            dy: Length(16.0),
        },
    ];
    assert!(input.apply_scroll_requests(
        &view,
        &frame,
        actions.map(|action| ScrollRequest {
            target: handle.clone(),
            action,
        })
    ));
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(
        frame.scroll_on(Orientation::Horizontal).unwrap().offset,
        380.0
    );
    assert_eq!(frame.scroll_on(Orientation::Vertical).unwrap().offset, 16.0);
    assert!(input.take_scroll_events(&view, &frame).is_empty());
}

#[test]
fn reserved_scroll_view_gutters_fit_both_axes_and_mirror_the_vertical_lane() {
    use xrack_runtime::{ScrollBarPolicy, ViewportHandle};
    let mut view = node("ScrollView", "view");
    let handle = ViewportHandle::default();
    view.viewport_handle = Some(handle.clone());
    boolean(&mut view, "reserve-scroll-bar-space", true);
    length(&mut view, "content-width", 176.0);
    length(&mut view, "content-height", 150.0);
    let mut artwork = node("Rectangle", "content");
    length(&mut artwork, "width", 176.0);
    length(&mut artwork, "height", 150.0);
    view.children.push(artwork);

    let size = Size::new(180.0, 108.0);
    let frame = layout(&view, size, &mut Text);
    let body = Rect::new(0.0, 0.0, 170.0, 98.0);
    assert_eq!(frame.scroll_viewport(Orientation::Vertical), body);
    assert_eq!(frame.children[0].clip, body);
    let bars = frame.scroll_bars(&view).collect::<Vec<_>>();
    assert_eq!(bars[0].track, Rect::new(0.0, 98.0, 170.0, 10.0));
    assert_eq!(bars[1].track, Rect::new(170.0, 0.0, 10.0, 98.0));
    assert_eq!((bars[0].page, bars[1].page), (170.0, 98.0));
    assert_eq!(
        (bars[0].scroll.max_offset, bars[1].scroll.max_offset),
        (6.0, 52.0)
    );

    let mut input = Interaction::default();
    input.pointer_move(&view, &frame, Point { x: 175.0, y: 28.0 });
    assert!(input.scroll_bar_press(&view, &frame).is_some());
    input.pointer_move(&view, &frame, Point { x: 175.0, y: 128.0 });
    assert!(matches!(
        input.scroll_bar_drag(&view, &frame),
        Some(ScrollBarUpdate::ViewportMoved)
    ));
    let scrolled = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(scrolled.scroll.unwrap().offset, 52.0);
    assert_eq!(scrolled.children[0].clip, body);

    let mut observers = LayoutObservers::default();
    observers.sync(&view, &frame);
    assert_eq!(
        (handle.visible_width(), handle.visible_height()),
        (Length(170.0), Length(98.0))
    );
    assert_eq!(
        (
            handle.effective_scroll_bar_width(),
            handle.effective_scroll_bar_height()
        ),
        (Length(10.0), Length(10.0))
    );

    view.properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    let rtl = layout(&view, size, &mut Text);
    assert_eq!(rtl.children[0].clip, Rect::new(10.0, 0.0, 170.0, 98.0));
    let bars = rtl.scroll_bars(&view).collect::<Vec<_>>();
    assert_eq!(bars[0].track.x, 10.0);
    assert_eq!(bars[1].track.x, 0.0);
    assert!(bars[0].reversed());

    view.properties.insert(
        "vertical-scroll-bar-policy",
        ScrollBarPolicy::AlwaysOff.into(),
    );
    let off = layout(&view, size, &mut Text);
    assert_eq!(off.scroll_bars(&view).count(), 0);
    assert_eq!(off.scroll_viewport(Orientation::Vertical), off.bounds);
    assert_eq!(off.scroll.unwrap().max_offset, 42.0);
    observers.sync(&view, &off);
    assert_eq!(
        (
            handle.effective_scroll_bar_width(),
            handle.effective_scroll_bar_height()
        ),
        (Length(0.0), Length(0.0))
    );

    view.properties.insert(
        "horizontal-scroll-bar-policy",
        ScrollBarPolicy::AlwaysOn.into(),
    );
    let forced = layout(&view, size, &mut Text);
    let bar = forced.scroll_bars(&view).next().unwrap();
    assert_eq!(bar.track, Rect::new(0.0, 98.0, 180.0, 10.0));
    assert!(!bar.scrollable());
}

#[test]
fn rtl_scroll_view_reveals_the_physical_left_field_in_logical_order() {
    let mut view = node("ScrollView", "view");
    view.properties
        .insert("layout-direction", LayoutDirection::RightToLeft.into());
    length(&mut view, "content-width", 300.0);
    let mut field = node("LineEdit", "field");
    length(&mut field, "x", 12.0);
    length(&mut field, "y", 9.0);
    length(&mut field, "width", 46.0);
    length(&mut field, "height", 24.0);
    view.children.push(field);

    let size = Size::new(100.0, 80.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(
        frame.children[0].clip_rect(frame.children[0].bounds).width,
        0.0
    );
    assert!(input.focus_next(&view, &frame, false));
    let moved = layout_with_input(&view, size, &mut Text, &input);
    input.reconcile(&view, &moved);
    assert_eq!(
        moved.scroll_on(Orientation::Horizontal).unwrap().offset,
        188.0
    );
    assert_eq!(moved.children[0].bounds.x, 0.0);
    assert_eq!(
        input.focused.as_ref().and_then(Target::element_id),
        Some("field")
    );
}

#[test]
fn flickable_reveal_respects_opt_in_and_cannot_focus_a_clipped_subtree() {
    let mut view = node("Flickable", "view");
    length(&mut view, "content-height", 350.0);
    let mut wrapper = node("Rectangle", "wrapper");
    length(&mut wrapper, "width", 80.0);
    length(&mut wrapper, "height", 32.0);
    boolean(&mut wrapper, "clip", true);
    let mut clipped = button("clipped");
    length(&mut clipped, "y", 55.0);
    wrapper.children.push(clipped);
    view.children.push(wrapper);
    let mut field = node("LineEdit", "last");
    length(&mut field, "y", 200.0);
    length(&mut field, "width", 80.0);
    length(&mut field, "height", 24.0);
    view.children.push(field);

    let size = Size::new(140.0, 100.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert!(!input.focus_next(&view, &frame, false));
    assert!(input.focused.is_none());

    boolean(&mut view, "auto-reveal-focus", true);
    let frame = layout_with_input(&view, size, &mut Text, &input);
    assert!(input.focus_next(&view, &frame, false));
    let moved = layout_with_input(&view, size, &mut Text, &input);
    input.reconcile(&view, &moved);
    assert_eq!(moved.scroll.unwrap().offset, 124.0);
    assert_eq!(
        input.focused.as_ref().and_then(Target::element_id),
        Some("last")
    );
}

#[test]
fn positioning_a_child_resolves_pending_offsets_and_the_current_instance() {
    use xrack_runtime::{
        GeometryHandle, ScrollAction, ScrollAlignment, ScrollRequest, ViewportHandle,
    };
    let mut view = node("ScrollView", "view");
    let viewport = ViewportHandle::default();
    view.viewport_handle = Some(viewport.clone());
    length(&mut view, "content-height", 320.0);
    let mut field = button("target");
    length(&mut field, "y", 45.0);
    let geometry = GeometryHandle::default();
    let original = geometry.scroll_target_id();
    field.set_instance_geometry_handle(geometry);
    view.children.push(field.clone());

    let size = Size::new(100.0, 100.0);
    let mut input = Interaction::default();
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let locate = |target| ScrollAction::ToChild {
        target,
        horizontal: ScrollAlignment::Visible,
        vertical: ScrollAlignment::Visible,
    };
    let actions = [
        ScrollAction::To {
            x: Length(0.0),
            y: Length(180.0),
        },
        locate(original),
        ScrollAction::By {
            dx: Length(0.0),
            dy: Length(10.0),
        },
    ];
    assert!(input.apply_scroll_requests(
        &view,
        &frame,
        actions.map(|action| ScrollRequest {
            target: viewport.clone(),
            action,
        })
    ));
    let moved = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(moved.scroll.unwrap().offset, 55.0);

    let replacement_handle = GeometryHandle::default();
    let replacement = replacement_handle.scroll_target_id();
    field.set_instance_geometry_handle(replacement_handle);
    length(&mut field, "y", 125.0);
    view.children[0] = field;
    let frame = layout_with_input(&view, size, &mut Text, &input);
    let center = |id| ScrollRequest {
        target: viewport.clone(),
        action: ScrollAction::ToChild {
            target: id,
            horizontal: ScrollAlignment::Center,
            vertical: ScrollAlignment::Center,
        },
    };
    assert!(!input.apply_scroll_requests(&view, &frame, [center(original)]));
    assert!(input.apply_scroll_requests(&view, &frame, [center(replacement)]));
    let moved = layout_with_input(&view, size, &mut Text, &input);
    assert_eq!(moved.scroll.unwrap().offset, 85.0);
    assert!(input.focused.is_none());
}

#[test]
fn texture_only_source_has_its_own_size_without_consuming_layout_space() {
    let mut root = node("VerticalLayout", "test-layout");
    let mut source = node("Rectangle", "source");
    length(&mut source, "x", 300.0);
    length(&mut source, "y", 220.0);
    length(&mut source, "width", 114.0);
    length(&mut source, "height", 66.0);
    source
        .properties
        .insert("texture-source", Value::String("control-panel".into()));
    source.properties.insert("texture-only", Value::Bool(true));
    let mut child = node("Rectangle", "inside");
    length(&mut child, "x", 6.0);
    length(&mut child, "y", 8.0);
    length(&mut child, "width", 35.0);
    length(&mut child, "height", 20.0);
    source.children.push(child);
    let mut sibling = node("Rectangle", "visible");
    length(&mut sibling, "width", 90.0);
    length(&mut sibling, "height", 30.0);
    root.children.extend([source, sibling]);

    let placed = layout(&root, Size::new(160.0, 64.0), &mut Text);
    let offscreen = &placed.children[0];
    assert_eq!(offscreen.bounds, Rect::new(0.0, 0.0, 114.0, 66.0));
    assert_eq!(offscreen.clip, offscreen.bounds);
    assert_eq!(
        offscreen.children[0].bounds,
        Rect::new(6.0, 8.0, 35.0, 20.0)
    );
    assert_eq!(placed.children[1].bounds.y, 0.0);
}
