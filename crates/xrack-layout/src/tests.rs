use super::*;
use xrack_runtime::{CrossAxisAlignment, LayoutAlignment, Length, Node, Value};

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
fn scroll_bar_captures_drag_and_preserves_focus_when_policy_changes() {
    use xrack_runtime::{Orientation, ScrollBarPolicy};

    let mut root = node("Window", "root");
    root.children.push(button("opener"));
    let mut view = node("ScrollView", "view");
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
    assert_eq!(input.scroll_bar_press(&root, &layout), Some(true));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&90.0));
    assert_eq!(input.focused, opener);
    assert!(input.pointer_release_target().is_none());

    let layout = render(&root, &input);
    let thumb = bar(&root, &layout).thumb;
    input.pointer_move(
        &root,
        &layout,
        Point {
            x: thumb.x + 4.0,
            y: thumb.y + 5.0,
        },
    );
    assert_eq!(input.scroll_bar_press(&root, &layout), Some(false));
    input.pointer_move(&root, &layout, Point { x: 0.0, y: 300.0 });
    assert_eq!(input.scroll_bar_drag(&root, &layout), Some(true));
    assert_eq!(input.scroll_offsets().get(&[1][..]), Some(&410.0));
    let layout = render(&root, &input);
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
    assert_eq!(input.scroll_bar_drag(&root, &layout), Some(false));
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
fn nested_scroll_views_pass_only_unconsumed_motion_to_the_parent() {
    let mut outer = node("ScrollView", "outer");
    let mut inner = node("ScrollView", "inner");
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
    input.pointer_move(&outer, &scrolled, point);
    assert!(input.scroll_wheel(&scrolled, Point { x: -50.0, y: -55.0 }, false));
    assert_eq!(input.horizontal_scroll_offsets().get(&[0][..]), Some(&0.0));
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&0.0));
    assert_eq!(input.horizontal_scroll_offsets().get(&[][..]), Some(&5.0));
    assert_eq!(input.scroll_offsets().get(&[][..]), Some(&10.0));
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
    assert!(input.advance_scroll_motion(&outer, &dragged, now + Duration::from_millis(56)));
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
