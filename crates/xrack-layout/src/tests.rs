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
        columns: vec![column; COLUMNS],
        sort: None,
        column_widths: Vec::new(),
    }));
    table.list_row_renderer = Some(ListRowRenderer::with_columns(
        move |range, columns: &TableColumnWindow| {
            project_standard_table_rows(
                &rows,
                range,
                columns,
                COLUMNS,
                |_| None,
                |row, column| format!("{row}:{column}"),
                |_| None,
            )
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
    let narrow = layout_with_input(&table, Size::new(130.0, 180.0), &mut Text, &input);
    assert_eq!(narrow.table.as_ref().unwrap().frozen_boundary(), None);
    assert!(!narrow.table.as_ref().unwrap().is_frozen(1));
    table.properties.insert("frozen-columns", i64::MAX.into());
    let fitted = layout(&table, Size::new(420.0, 180.0), &mut Text);
    assert!(fitted.table.as_ref().unwrap().is_frozen(0));
    assert_eq!(fitted.table.as_ref().unwrap().scrollable_body().width, 0.0);
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
    assert!(frame.horizontal_scroll.is_none());
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
            .children
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
    let point = Point { x: 20.0, y: 20.0 };
    let viewport = Size::new(160.0, 120.0);
    let frame = layout(&window, viewport, &mut Text);
    let mut input = Interaction::default();
    input.pointer_move(&window, &frame, point);
    assert!(input.scroll_wheel(&frame, Point { x: 0.0, y: 32.0 }, false));
    assert_eq!(input.scroll_offsets().get(&[0][..]), Some(&32.0));
    assert!(!input.scroll_offsets().contains_key(&[1][..]));
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
