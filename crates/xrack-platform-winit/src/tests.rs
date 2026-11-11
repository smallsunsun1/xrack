use super::*;
use std::{
    cell::Cell,
    future::{Future, pending, poll_fn},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread,
};
use xrack_runtime::Value;

#[test]
fn native_cursor_resolution_prefers_resize_then_explicit_shapes_then_text() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{HoverHandle, Length};

    struct View(Rc<Node>);
    impl Component for View {
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
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let rect = |kind, x, y, width, height| {
        let mut node = Node::new(kind, None);
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            node.properties.insert(name, Length(value).into());
        }
        node
    };
    let mut scene = Node::new("Window", None);
    let mut outer = rect("HoverHandler", 0.0, 0.0, 280.0, 160.0);
    outer.hover_handle = Some(HoverHandle::default());
    outer
        .properties
        .insert("cursor-shape", CursorShape::Crosshair.into());
    let mut inner = rect("HoverHandler", 12.0, 12.0, 110.0, 44.0);
    inner.hover_handle = Some(HoverHandle::default());
    inner
        .properties
        .insert("cursor-shape", CursorShape::Pointer.into());
    inner.children.push(rect("LineEdit", 0.0, 0.0, 110.0, 44.0));
    outer.children.push(inner);
    let mut touch = rect("TouchArea", 140.0, 12.0, 110.0, 44.0);
    touch
        .properties
        .insert("cursor-shape", CursorShape::Grab.into());
    outer.children.push(touch);
    let mut split = rect("SplitView", 12.0, 82.0, 230.0, 54.0);
    split.children.push(Node::new("Rectangle", None));
    split.children.push(Node::new("Rectangle", None));
    split.add_split_handles();
    outer.children.push(split);
    scene.children.push(outer);
    let mut app = Application::new(
        View(Rc::new(scene)),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let size = Size::new(280.0, 160.0);
    let mut measure = Measure;
    let sync = |app: &mut Application<View, (), _>, measure: &mut Measure, point: Point| {
        let frame = Rc::new(layout(&app.scene, size, measure));
        app.input.pointer_move(&app.scene, &frame, point);
        app.hover_pointer = Some(PhysicalPosition::new(
            f64::from(point.x),
            f64::from(point.y),
        ));
        let events = app
            .hover_observers
            .sync(&app.input, &app.scene, &frame, Some(point));
        app.layout = Some(frame);
        events
    };
    let text = Point { x: 30.0, y: 30.0 };
    assert_eq!(sync(&mut app, &mut measure, text).len(), 2);
    assert_eq!(app.pointer_cursor(), CursorIcon::Pointer);

    Rc::make_mut(&mut app.scene).children[0].children[0]
        .properties
        .insert("cursor-shape", CursorShape::Auto.into());
    assert!(sync(&mut app, &mut measure, text).is_empty());
    assert_eq!(app.pointer_cursor(), CursorIcon::Crosshair);
    Rc::make_mut(&mut app.scene).children[0].children[0]
        .properties
        .insert("blocking", true.into());
    sync(&mut app, &mut measure, text);
    assert_eq!(app.pointer_cursor(), CursorIcon::Text);

    sync(&mut app, &mut measure, Point { x: 175.0, y: 30.0 });
    assert_eq!(app.pointer_cursor(), CursorIcon::Grab);
    let split = [0, 2];
    let handle = app
        .scene
        .at_path(&split)
        .unwrap()
        .children
        .iter()
        .position(|node| node.kind == "SplitHandle")
        .unwrap();
    let bounds = app
        .layout
        .as_ref()
        .unwrap()
        .at_path(&[0, 2, handle])
        .unwrap()
        .bounds;
    let grip = Point {
        x: bounds.x + bounds.width * 0.5,
        y: bounds.y + bounds.height * 0.5,
    };
    sync(&mut app, &mut measure, grip);
    assert_eq!(app.hover_observers.cursor_shape(), CursorShape::Crosshair);
    assert_eq!(app.pointer_cursor(), CursorIcon::ColResize);

    let touch = Point { x: 175.0, y: 30.0 };
    sync(&mut app, &mut measure, touch);
    assert_eq!(app.pointer_cursor(), CursorIcon::Grab);
    let mut dialog = rect("Dialog", 0.0, 0.0, 100.0, 60.0);
    dialog.properties.insert("open", true.into());
    Rc::make_mut(&mut app.scene).children.push(dialog);
    let frame = Rc::new(layout(&app.scene, size, &mut measure));
    app.hover_observers
        .sync(&app.input, &app.scene, &frame, Some(touch));
    app.layout = Some(frame);
    assert_eq!(app.pointer_cursor(), CursorIcon::Default);

    app.input.pointer_leave();
    app.hover_pointer = None;
    app.hover_observers
        .sync(&app.input, &app.scene, app.layout.as_ref().unwrap(), None);
    assert_eq!(app.pointer_cursor(), CursorIcon::Default);
}

#[test]
fn a_denied_control_publishes_guard_side_effects_without_claiming_navigation_success() {
    #[derive(Default)]
    struct View {
        prompt: bool,
        dirty: Cell<bool>,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut scene = Node::new("Window", None);
            scene.properties.insert("prompt", self.prompt.into());
            self.dirty.set(false);
            scene
        }

        fn scene_dirty(&self) -> bool {
            self.dirty.get()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn control_at(&mut self, _: &Node, _: &[usize], action: ControlAction) -> bool {
            assert!(matches!(action, ControlAction::StackBack { .. }));
            self.prompt = true;
            self.dirty.set(true);
            false
        }
    }
    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    assert!(!app.base_scene.boolean("prompt", false));
    app.apply_control(ControlInput {
        path: vec![],
        action: ControlAction::StackBack { revision: 1 },
    });
    assert!(app.base_scene.boolean("prompt", false));
    assert!(!app.component.scene_dirty());
}

#[test]
fn a_deferred_focus_command_is_consumed_with_an_unchanged_scene() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{FocusAction, FocusHandle, Length};

    struct View(Rc<Node>);
    impl Component for View {
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
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let handle = FocusHandle::default();
    let mut scene = Node::new("Window", None);
    let mut target = Node::new("Button", Some("target"));
    target.focus_handle = Some(handle.clone());
    target.properties.insert("width", Length(100.0).into());
    target.properties.insert("height", Length(48.0).into());
    scene.children.push(target);
    let mut app = Application::new(
        View(Rc::new(scene)),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    app.layout = Some(Rc::new(layout(
        &app.scene,
        Size::new(200.0, 100.0),
        &mut Measure,
    )));
    {
        let _focus = app.focus_requests.enter();
        handle.request(FocusAction::Focus);
    }
    assert!(app.input.focused.is_none());
    app.refresh_after_update(false);
    assert_eq!(
        app.input.focused.as_ref().and_then(|target| target.id),
        Some("target")
    );
    assert!(app.focus_requests.is_empty());
}

#[test]
fn a_deferred_scroll_command_is_consumed_with_an_unchanged_scene() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Length, ScrollAction, ViewportHandle};

    struct View(Rc<Node>);
    impl Component for View {
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
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let handle = ViewportHandle::default();
    let mut scene = Node::new("ScrollView", Some("viewport"));
    scene.viewport_handle = Some(handle.clone());
    scene
        .properties
        .insert("content-height", Length(320.0).into());
    let mut app = Application::new(
        View(Rc::new(scene)),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    app.layout = Some(Rc::new(layout(
        &app.scene,
        Size::new(160.0, 100.0),
        &mut Measure,
    )));
    {
        let _scroll = app.scroll_requests.enter();
        handle.request(ScrollAction::To {
            x: Length(0.0),
            y: Length(80.0),
        });
    }
    assert!(app.input.scroll_offsets().is_empty());
    app.refresh_dependencies();
    assert_eq!(app.input.scroll_offsets().get(&[][..]), Some(&80.0));
    assert!(app.scroll_requests.is_empty());
}

#[test]
fn bound_scroll_offsets_invalidate_layout_without_rebuilding_the_scene() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Length, Property, ViewportHandle};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let position = Rc::new(Property::default());
    let reader = position.clone();
    let writer = position.clone();
    let handle = ViewportHandle::default();
    handle.bind_offset(
        Orientation::Vertical,
        move || reader.get(Length::default),
        move |value| writer.set_if_changed(value),
    );
    let mut node = Node::new("ScrollView", Some("viewport"));
    node.viewport_handle = Some(handle);
    node.properties
        .insert("content-height", Length(320.0).into());
    let scene = Rc::new(node);
    let size = Size::new(160.0, 100.0);
    let input = Interaction::default();
    let mut cache = FrameCache::default();
    let prepare = |scene: Rc<Node>| {
        let frame = layout(&scene, size, &mut Measure);
        (scene, frame)
    };
    let (first_scene, first_layout) = cache.prepare(scene.clone(), size, &input, prepare);
    assert_eq!(first_layout.scroll.unwrap().offset, 0.0);
    position.set(Length(-80.0));
    assert!(cache.is_dirty());
    let (next_scene, next_layout) = cache.prepare(scene, size, &input, prepare);
    assert!(Rc::ptr_eq(&first_scene, &next_scene));
    assert_eq!(next_layout.scroll.unwrap().offset, 80.0);
}

#[test]
fn paint_only_updates_replace_the_scene_without_resolving_layout() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Color, Length};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }

    let scene = |color, width| {
        let mut rectangle = Node::new("Rectangle", Some("swatch"));
        rectangle.mark_paint_only_background();
        rectangle
            .properties
            .insert("background", Value::Color(Color(color)));
        rectangle
            .properties
            .insert("width", Value::Length(Length(width)));
        Rc::new(rectangle)
    };
    let viewport = Size::new(160.0, 100.0);
    let input = Interaction::default();
    let resolves = Cell::new(0);
    let prepare = |cache: &mut FrameCache, base, materialize| {
        cache.prepare(base, viewport, &input, |base| {
            resolves.set(resolves.get() + 1);
            let geometry = layout(&base, viewport, &mut Measure);
            let prepared = if materialize {
                Rc::new(base.as_ref().clone())
            } else {
                base
            };
            (prepared, geometry)
        })
    };
    let mut cache = FrameCache::default();
    let (_, original) = prepare(&mut cache, scene(0x112233ff, 40.0), false);
    let recolored = scene(0x445566ff, 40.0);
    let (painted, reused) = prepare(&mut cache, recolored.clone(), false);
    assert!(Rc::ptr_eq(&painted, &recolored));
    assert!(Rc::ptr_eq(&original, &reused));
    assert_eq!(resolves.get(), 1);
    let (_, resized) = prepare(&mut cache, scene(0x445566ff, 80.0), false);
    assert!(!Rc::ptr_eq(&original, &resized));
    assert_eq!(resolves.get(), 2);

    let mut materialized = FrameCache::default();
    let (_, original) = prepare(&mut materialized, scene(0x112233ff, 40.0), true);
    let (_, updated) = prepare(&mut materialized, recolored, true);
    assert!(!Rc::ptr_eq(&original, &updated));
    assert_eq!(resolves.get(), 4);
}

#[test]
fn tumbler_frame_tracks_visible_labels_only() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::{TumblerItems, VecModel};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let model = VecModel::from((0..50_000).map(|row| row.to_string()).collect::<Vec<_>>());
    let mut node = Node::new("Tumbler", None);
    node.tumbler_items = Some(TumblerItems::new(model.clone()));
    let scene = Rc::new(node);
    let input = Interaction::default();
    let viewport = Size::new(140.0, 160.0);
    let mut cache = FrameCache::default();
    let prepare = |cache: &mut FrameCache| {
        cache.prepare(scene.clone(), viewport, &input, |base| {
            let layout = layout_with_input(&base, viewport, &mut Measure, &input);
            (base, layout)
        })
    };
    let (_, first) = prepare(&mut cache);
    assert_eq!(first.tumbler.as_ref().unwrap().rows.len(), 5);
    assert!(model.set_row(1_000, "offscreen".into()));
    let (_, untouched) = prepare(&mut cache);
    assert!(Rc::ptr_eq(&first, &untouched));
    assert!(model.set_row(1, "visible".into()));
    let (_, updated) = prepare(&mut cache);
    assert!(!Rc::ptr_eq(&first, &updated));
    assert!(
        updated
            .tumbler
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label == "visible")
    );
}

#[test]
fn password_clipboard_allows_paste_and_rejects_copy_and_cut_even_when_revealed() {
    use xrack_runtime::InputType;

    let mut field = Node::new("LineEdit", None);
    field
        .properties
        .insert("text", "e\u{301}👩‍💻".to_owned().into());
    let mut input = Interaction::default();
    input.focused = Some(Target {
        text_link: None,
        path: vec![],
        id: None,
        focusable: true,
        row_key: None,
    });
    input.sync_editor(&field);
    input
        .editor
        .as_mut()
        .unwrap()
        .apply(field.text("text").unwrap(), EditAction::SelectAll);
    let (_, normal) = EditContext::request(&input, &field, &ClipboardShortcut::Cut).unwrap();
    field
        .properties
        .insert("input-type", InputType::Password.into());
    assert!(!normal.is_current(&input, &field));
    for revealed in [false, true] {
        field.properties.insert("password-visible", revealed.into());
        assert!(EditContext::request(&input, &field, &ClipboardShortcut::Copy).is_none());
        assert!(EditContext::request(&input, &field, &ClipboardShortcut::Cut).is_none());
        assert!(matches!(
            EditContext::request(&input, &field, &ClipboardShortcut::Paste),
            Some((clipboard::Operation::Read, _))
        ));
    }
}

#[test]
fn accessibility_scrolls_the_requested_container_without_moving_its_parent() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(30.0, 16.0)
        }
    }
    struct View;
    impl Component for View {
        fn render(&self) -> Node {
            let mut outer = Node::new("ScrollView", Some("outer"));
            outer
                .properties
                .insert("content-height", Length(500.0).into());
            let mut inner = Node::new("ScrollView", Some("inner"));
            inner.properties.insert("width", Length(160.0).into());
            inner.properties.insert("height", Length(90.0).into());
            inner
                .properties
                .insert("content-width", Length(400.0).into());
            inner
                .properties
                .insert("content-height", Length(420.0).into());
            inner
                .properties
                .insert("accessible-name", "Workspace".to_owned().into());
            outer.children.push(inner);
            outer
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }
    let mut app = Application::new(
        View,
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(220.0, 120.0);
    let frame = Rc::new(layout(&app.scene, viewport, &mut Measure));
    app.layout = Some(frame.clone());
    let mut projection = Projection::default();
    let update = projection
        .update(&app.scene, &frame, &app.input, 2.0, None)
        .unwrap();
    let (target_node, item) = update
        .nodes
        .iter()
        .find(|(_, item)| item.label() == Some("Workspace"))
        .unwrap();
    assert_eq!(item.role(), accesskit::Role::ScrollView);
    assert_eq!(item.scroll_y_max(), Some(660.0));
    assert_eq!(item.scroll_x_max(), Some(480.0));
    assert!(item.supports_action(AccessibleAction::ScrollDown));
    assert!(!item.supports_action(AccessibleAction::ScrollUp));
    let scaled_offset = accessibility::requested_scroll(
        AccessibleAction::SetScrollOffset,
        Some(&ActionData::SetScrollOffset(accesskit::Point::new(
            120.0, 200.0,
        ))),
        app.scene.at_path(&[0]).unwrap(),
        frame.at_path(&[0]).unwrap(),
        2.0,
    );
    assert_eq!(scaled_offset, [Some(60.0), Some(100.0)]);
    let route = projection.route(*target_node).unwrap();
    app.accessibility_action(
        route.clone(),
        ActionRequest {
            action: AccessibleAction::ScrollDown,
            target_tree: TreeId::ROOT,
            target_node: *target_node,
            data: Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Page)),
        },
    );
    assert_eq!(app.input.scroll_offsets().get(&[0][..]), Some(&90.0));
    assert!(!app.input.scroll_offsets().contains_key(&[][..]));
    app.accessibility_action(
        route,
        ActionRequest {
            action: AccessibleAction::SetScrollOffset,
            target_tree: TreeId::ROOT,
            target_node: *target_node,
            data: Some(ActionData::SetScrollOffset(accesskit::Point::new(
                60.0, 999.0,
            ))),
        },
    );
    assert_eq!(
        app.input.horizontal_scroll_offsets().get(&[0][..]),
        Some(&60.0)
    );
    assert_eq!(app.input.scroll_offsets().get(&[0][..]), Some(&330.0));
    assert!(!app.input.scroll_offsets().contains_key(&[][..]));
}

#[test]
fn accessibility_resizes_the_addressed_splitter_and_rejects_an_old_neighbor() {
    use accesskit::{NodeId, Role};
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View {
        sizes: Option<Arc<[Option<f32>]>>,
        hide_middle: bool,
        edits: usize,
    }

    impl Component for View {
        fn render(&self) -> Node {
            let mut window = Node::new("Window", None);
            let mut view = Node::new("SplitView", Some("work"));
            view.properties.insert("width", Length(300.0).into());
            view.properties.insert("height", Length(100.0).into());
            view.properties
                .insert("accessible-name", "Workspace".to_owned().into());
            view.split_fill_pane = Some("middle");
            view.split_sizes = self.sizes.clone();
            for (id, title, preferred, min, max, visible) in [
                ("left", "Navigation", 100.0, 80.0, 140.0, true),
                ("middle", "Editor", 100.0, 60.0, 180.0, !self.hide_middle),
                ("right", "Inspector", 70.0, 60.0, 180.0, true),
            ] {
                let mut pane = Node::new("Rectangle", Some(id));
                pane.properties
                    .insert("accessible-name", title.to_owned().into());
                pane.properties
                    .insert("preferred-width", Length(preferred).into());
                pane.properties.insert("min-width", Length(min).into());
                pane.properties.insert("max-width", Length(max).into());
                pane.properties.insert("visible", visible.into());
                view.children.push(pane);
            }
            view.add_split_handles();
            window.children.push(view);
            window
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn resize_split_sizes_at(
            &mut self,
            scene: &Node,
            path: &[usize],
            sizes: &[(usize, f32)],
        ) -> bool {
            assert_eq!(path, &[0]);
            let Some(next) = scene.at_path(path).unwrap().resized_split_sizes(sizes) else {
                return false;
            };
            self.sizes = Some(next);
            self.edits += 1;
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(30.0, 16.0)
        }
    }

    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let frame = |app: &mut Application<View, (), _>| {
        app.scene = Rc::new(app.component.render());
        let layout = Rc::new(layout(&app.scene, Size::new(320.0, 140.0), &mut Measure));
        app.layout = Some(layout.clone());
        layout
    };
    let request = |node, action, data| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data,
    };
    let role = if cfg!(target_os = "windows") {
        Role::Slider
    } else {
        Role::Splitter
    };
    let mut projection = Projection::default();
    let layout = frame(&mut app);
    let update = projection
        .update(&app.scene, &layout, &app.input, 2.0, None)
        .unwrap();
    let lookup = |label| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label) && node.role() == Role::Group)
            .unwrap()
    };
    let navigation = lookup("Navigation").0;
    let editor = lookup("Editor").0;
    let inspector = lookup("Inspector").0;
    let (_, split) = lookup("Workspace");
    let [left, first, middle, second, right] = <[NodeId; 5]>::try_from(split.children()).unwrap();
    assert_eq!([left, middle, right], [navigation, editor, inspector]);
    let splitter = update
        .nodes
        .iter()
        .find(|(id, _)| *id == first)
        .unwrap()
        .1
        .clone();
    assert_eq!(splitter.role(), role);
    assert_eq!(splitter.label(), Some("Navigation"));
    assert_eq!(splitter.controls(), &[navigation]);
    assert_eq!(
        splitter.orientation(),
        Some(accesskit::Orientation::Vertical)
    );
    assert_eq!(splitter.numeric_value(), Some(100.0));
    assert_eq!(splitter.min_numeric_value(), Some(80.0));
    assert_eq!(splitter.max_numeric_value(), Some(140.0));
    assert_eq!(splitter.numeric_value_step(), Some(8.0));
    let route = projection.route(first).unwrap();
    app.accessibility_action(route.clone(), request(first, AccessibleAction::Focus, None));
    assert!(app.input.is_focused(&[0, 3]));
    app.accessibility_action(
        route.clone(),
        request(first, AccessibleAction::Increment, None),
    );
    assert_eq!(app.component.sizes.as_deref().unwrap()[0], Some(108.0));

    frame(&mut app);
    app.accessibility_action(
        route.clone(),
        request(
            first,
            AccessibleAction::SetValue,
            Some(ActionData::NumericValue(f64::NAN)),
        ),
    );
    assert_eq!(app.component.edits, 1);
    app.accessibility_action(
        route.clone(),
        request(
            first,
            AccessibleAction::SetValue,
            Some(ActionData::Value("500".into())),
        ),
    );
    assert_eq!(
        app.component.sizes.as_deref().unwrap(),
        &[Some(140.0), Some(74.0), None]
    );
    let layout = frame(&mut app);
    let update = projection
        .update(&app.scene, &layout, &app.input, 2.0, None)
        .unwrap();
    let max = update.nodes.iter().find(|(id, _)| *id == first).unwrap();
    assert_eq!(max.1.numeric_value(), Some(140.0));
    assert!(!max.1.supports_action(AccessibleAction::Increment));
    assert!(max.1.supports_action(AccessibleAction::Decrement));
    assert_eq!(update.focus, first);
    app.accessibility_action(
        route.clone(),
        request(first, AccessibleAction::Increment, None),
    );
    assert_eq!(app.component.edits, 2);
    app.accessibility_action(
        route.clone(),
        request(first, AccessibleAction::Decrement, None),
    );
    assert_eq!(app.component.sizes.as_deref().unwrap()[0], Some(132.0));

    app.component.hide_middle = true;
    let layout = frame(&mut app);
    app.accessibility_action(route, request(first, AccessibleAction::Decrement, None));
    assert_eq!(app.component.edits, 3);
    let update = projection
        .update(&app.scene, &layout, &app.input, 2.0, None)
        .unwrap();
    let splitters: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, item)| item.role() == role)
        .collect();
    assert_eq!(splitters.len(), 1);
    assert_ne!(splitters[0].0, first);
    assert!(projection.route(first).is_none() && projection.route(second).is_none());
}

#[test]
fn accessibility_dispatches_clicks_and_edits_and_rejects_a_stale_path() {
    use accesskit::NodeId;
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{InputType, Length, ScrollSnapMode, SliderRange};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(30.0, 16.0)
        }
    }
    #[derive(Default)]
    struct View {
        clicks: usize,
        text: String,
        slider: f64,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            let mut button = Node::new("Button", Some("save"));
            button.properties.insert("text", "Save".to_owned().into());
            let mut editor = Node::new("LineEdit", Some("editor"));
            editor.properties.insert("y", Length(50.0).into());
            editor
                .properties
                .insert("input-type", InputType::Search.into());
            editor.properties.insert("maximum-length", 3_i64.into());
            editor.properties.insert("text", self.text.clone().into());
            let mut slider = Node::new("Slider", Some("volume"));
            slider.properties.insert("y", Length(90.0).into());
            slider.properties.insert("value", self.slider.into());
            slider.properties.insert("step", 10.0_f64.into());
            slider
                .properties
                .insert("snap-mode", ScrollSnapMode::OnRelease.into());
            root.children = vec![button, editor, slider];
            root
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            assert_eq!((id, event), ("save", "clicked"));
            self.clicks += 1;
            true
        }
        fn adjust_range_at(&mut self, scene: &Node, path: &[usize], action: RangeAction) -> bool {
            assert_eq!(path, &[2]);
            let range =
                SliderRange::from_node(scene.at_path(path).unwrap()).with_value(self.slider);
            let Some(value) = range.apply(action) else {
                return false;
            };
            self.slider = value;
            true
        }
        fn edit_at(&mut self, _: &Node, path: &[usize], value: String) -> bool {
            assert_eq!(path, &[1]);
            self.text = value;
            true
        }
    }
    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(220.0, 120.0);
    let frame = Rc::new(layout(&app.scene, viewport, &mut Measure));
    app.layout = Some(frame.clone());
    let mut projection = Projection::default();
    let update = projection
        .update(&app.scene, &frame, &app.input, 1.0, None)
        .unwrap();
    let id = |role| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == role)
            .unwrap()
            .0
    };
    let click = id(accesskit::Role::Button);
    let edit = id(accesskit::Role::SearchInput);
    let request = |target_node: NodeId, action, data| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node,
        data,
    };
    let slider = id(accesskit::Role::Slider);
    assert!(
        update
            .nodes
            .iter()
            .any(|(id, node)| *id == slider && node.supports_action(AccessibleAction::SetValue))
    );
    app.accessibility_action(
        projection.route(slider).unwrap(),
        request(
            slider,
            AccessibleAction::SetValue,
            Some(ActionData::NumericValue(46.0)),
        ),
    );
    assert_eq!(app.component.slider, 50.0);
    let route = projection.route(click).unwrap();
    app.accessibility_action(route.clone(), request(click, AccessibleAction::Click, None));
    assert_eq!(app.component.clicks, 1);
    app.accessibility_action(
        projection.route(edit).unwrap(),
        request(
            edit,
            AccessibleAction::SetValue,
            Some(ActionData::Value("工作区".into())),
        ),
    );
    assert_eq!(app.component.text, "工作区");
    app.accessibility_action(
        projection.route(edit).unwrap(),
        request(
            edit,
            AccessibleAction::SetValue,
            Some(ActionData::Value("太长的名称".into())),
        ),
    );
    assert_eq!(app.component.text, "工作区");
    assert!(
        app.input
            .editor
            .as_ref()
            .unwrap()
            .selection("工作区")
            .is_empty()
    );
    assert_eq!(
        app.input.focused.as_ref().and_then(|target| target.id),
        Some("editor")
    );

    Rc::make_mut(&mut app.scene).children[0].id = Some("recycled");
    app.layout = Some(Rc::new(layout(&app.scene, viewport, &mut Measure)));
    app.accessibility_action(route, request(click, AccessibleAction::Click, None));
    assert_eq!(app.component.clicks, 1);
}

#[test]
fn native_touch_keeps_range_capture_until_its_own_contact_ends() {
    use xrack_layout::{ColorPlaneGeometry, SliderGeometry, TextMeasurer, layout};
    use xrack_runtime::{Length, RangeAction};

    #[derive(Default)]
    struct View(Vec<RangeAction>);

    impl Component for View {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            let mut plane = Node::new("ColorPlane", Some("plane"));
            plane.properties.insert("width", Length(120.0).into());
            plane.properties.insert("height", Length(80.0).into());
            let mut slider = Node::new("Slider", Some("deferred"));
            slider.properties.insert("y", Length(96.0).into());
            slider.properties.insert("width", Length(120.0).into());
            slider.properties.insert("height", Length(28.0).into());
            slider.properties.insert("live", false.into());
            root.children.extend([plane, slider]);
            root
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            panic!("a captured touch must not dispatch a click");
        }

        fn adjust_range_at(&mut self, _scene: &Node, path: &[usize], action: RangeAction) -> bool {
            assert_eq!(
                path,
                if matches!(action, RangeAction::CommitFraction(_)) {
                    &[1]
                } else {
                    &[0]
                }
            );
            self.0.push(action);
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }

    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let layout = Rc::new(layout(&app.scene, Size::new(180.0, 150.0), &mut Measure));
    let geometry = ColorPlaneGeometry::new(layout.at_path(&[0]).unwrap().bounds);
    let slider = SliderGeometry::new(
        layout.at_path(&[1]).unwrap().bounds,
        Orientation::Horizontal,
    );
    app.layout = Some(layout);
    let start = geometry.point(0.0, 1.0);
    let outside = Point { x: 165.0, y: 110.0 };
    let now = Instant::now();
    let device = DeviceId::dummy();

    app.update_touch(device, 1, TouchPhase::Started, start, now);
    app.update_touch(device, 2, TouchPhase::Started, start, now);
    app.update_touch(device, 2, TouchPhase::Ended, outside, now);
    assert_eq!(app.component.0, [RangeAction::SetPoint { x: 0.0, y: 1.0 }]);
    assert!(app.input.touch_range_captured());
    app.update_touch(device, 1, TouchPhase::Moved, outside, now);
    app.update_touch(device, 1, TouchPhase::Ended, outside, now);
    assert_eq!(
        app.component.0,
        [
            RangeAction::SetPoint { x: 0.0, y: 1.0 },
            RangeAction::SetPoint { x: 1.0, y: 0.0 },
            RangeAction::Release,
        ]
    );
    assert!(!app.input.touch_range_captured());

    app.update_touch(device, 3, TouchPhase::Started, start, now);
    app.update_touch(device, 3, TouchPhase::Cancelled, start, now);
    app.update_touch(device, 3, TouchPhase::Moved, outside, now);
    assert_eq!(app.component.0.len(), 4);
    assert!(!app.input.touch_range_captured());
    assert!(app.input.pressed.is_none());

    app.update_touch(device, 4, TouchPhase::Started, slider.position(0.4), now);
    app.update_touch(device, 4, TouchPhase::Moved, slider.position(0.8), now);
    assert!(app.input.slider_preview(&[1]).is_some());
    assert_eq!(app.component.0.len(), 4);
    app.update_touch(device, 4, TouchPhase::Ended, slider.position(1.0), now);
    assert_eq!(
        app.component.0.last(),
        Some(&RangeAction::CommitFraction(1.0))
    );
    assert!(app.input.slider_preview(&[1]).is_none());
}

#[test]
fn native_split_drag_uses_one_touch_contact_and_cancels_without_a_tap() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Length, SplitResizeHandle};

    #[derive(Default)]
    struct View {
        resizing: SplitResizeHandle,
        updates: Vec<Vec<(usize, f32)>>,
    }

    impl Component for View {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            let mut split = Node::new("SplitView", Some("split"));
            split.properties.insert("width", Length(160.0).into());
            split.properties.insert("height", Length(64.0).into());
            split
                .properties
                .insert("handle-hit-size", Length(24.0).into());
            split.split_resize_handle = Some(self.resizing.clone());
            for id in ["left", "right"] {
                let mut pane = Node::new("Button", Some(id));
                pane.properties
                    .insert("preferred-width", Length(76.0).into());
                split.children.push(pane);
            }
            split.add_split_handles();
            root.children.push(split);
            root
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            panic!("a split drag must not dispatch a pane click");
        }

        fn resize_split_at(&mut self, _: &Node, path: &[usize], weights: &[(usize, f32)]) -> bool {
            assert_eq!(path, &[0]);
            self.updates.push(weights.to_vec());
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }

    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let layout = Rc::new(layout(&app.scene, Size::new(180.0, 100.0), &mut Measure));
    let handle = layout.at_path(&[0, 2]).unwrap().bounds;
    app.layout = Some(layout);
    let start = Point {
        x: handle.x - 5.0,
        y: handle.y + 16.0,
    };
    let moved = Point {
        x: start.x + 15.0,
        ..start
    };
    let device = DeviceId::dummy();
    let now = Instant::now();

    app.update_touch(device, 1, TouchPhase::Started, start, now);
    assert!(app.component.resizing.resizing());
    app.update_touch(device, 2, TouchPhase::Started, start, now);
    app.update_touch(device, 2, TouchPhase::Ended, moved, now);
    assert!(app.component.resizing.resizing());
    assert!(app.component.updates.is_empty());
    app.update_touch(device, 1, TouchPhase::Ended, moved, now);
    assert_eq!(app.component.updates.len(), 1);
    assert!(app.component.updates[0][0].1 > 0.5);
    assert!(!app.component.resizing.resizing());
    assert!(app.scroll_contact.is_none());

    app.update_touch(device, 3, TouchPhase::Started, start, now);
    assert!(app.component.resizing.resizing());
    app.update_touch(device, 3, TouchPhase::Cancelled, moved, now);
    assert_eq!(app.component.updates.len(), 1);
    assert!(!app.component.resizing.resizing());
    assert!(app.input.pressed.is_none());
    assert!(app.input.pointer_position().is_none());
    assert!(app.scroll_contact.is_none());
}

#[test]
fn native_touch_adapter_activates_taps_and_keeps_pans_out_of_click_dispatch() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View(usize);
    impl Component for View {
        fn render(&self) -> Node {
            let mut view = Node::new("ScrollView", None);
            view.properties
                .insert("content-height", Length(480.0).into());
            let mut action = Node::new("Button", Some("action"));
            action.properties.insert("width", Length(90.0).into());
            action.properties.insert("height", Length(40.0).into());
            view.children.push(action);
            view
        }

        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            assert_eq!((id, event), ("action", "clicked"));
            self.0 += 1;
            true
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(180.0, 120.0);
    app.layout = Some(Rc::new(layout_with_input(
        &app.scene,
        viewport,
        &mut Measure,
        &app.input,
    )));
    let start = Point { x: 30.0, y: 20.0 };
    let now = Instant::now();
    let device = DeviceId::dummy();
    app.update_touch(device, 1, TouchPhase::Started, start, now);
    app.update_touch(device, 2, TouchPhase::Started, start, now);
    app.update_touch(
        device,
        2,
        TouchPhase::Ended,
        start,
        now + Duration::from_millis(20),
    );
    assert_eq!(app.component.0, 0);
    app.update_touch(
        device,
        1,
        TouchPhase::Ended,
        start,
        now + Duration::from_millis(25),
    );
    assert_eq!(app.component.0, 1);
    app.update_touch(device, 3, TouchPhase::Started, start, now);
    app.update_touch(
        device,
        3,
        TouchPhase::Moved,
        Point { x: 30.0, y: -90.0 },
        now + Duration::from_millis(30),
    );
    // Even when a drag ends over its initial button it cannot be dispatched as a tap.
    app.update_touch(
        device,
        3,
        TouchPhase::Ended,
        start,
        now + Duration::from_millis(60),
    );
    assert_eq!(app.component.0, 1);
    assert_eq!(app.input.scroll_offsets().get(&[][..]), Some(&0.0));
}

#[test]
fn native_pointer_delivers_release_clicks_double_click_and_pan_cancel_in_order() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View {
        pointer: PointerHandle,
        events: Vec<PointerEvent>,
        clicks: Vec<String>,
    }

    impl Component for View {
        fn render(&self) -> Node {
            let mut scroll = Node::new("ScrollView", None);
            scroll
                .properties
                .insert("content-height", Length(480.0).into());
            let mut area = Node::new("TouchArea", Some("area"));
            area.properties.insert("width", Length(150.0).into());
            area.properties.insert("height", Length(80.0).into());
            area.pointer_handle = Some(self.pointer.clone());
            area.press_and_hold_handler = true;
            area.properties
                .insert("press-and-hold-interval", 120_i64.into());
            scroll.children.push(area);
            scroll
        }

        fn pointer_event_at(&mut self, _: &Node, path: &[usize], event: &PointerEvent) -> bool {
            assert_eq!(path, &[0]);
            assert_eq!(event.button, PointerButton::Primary);
            self.events.push(*event);
            true
        }

        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            if event == "press-and-hold" {
                assert_eq!(id, "area");
                assert!(self.pointer.pressed());
                assert_eq!(self.events.last().unwrap().kind, PointerEventKind::Down);
                self.clicks.push(event.into());
                return true;
            }
            if !matches!(event, "clicked" | "double-clicked") {
                return false;
            }
            assert_eq!(id, "area");
            assert_eq!(self.events.last().unwrap().kind, PointerEventKind::Up);
            assert!(!self.pointer.pressed());
            self.clicks.push(event.into());
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }

    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    app.layout = Some(Rc::new(layout_with_input(
        &app.scene,
        Size::new(180.0, 120.0),
        &mut Measure,
        &app.input,
    )));
    let start = Point { x: 30.0, y: 50.0 };
    let device = DeviceId::dummy();
    let now = Instant::now();
    let deadline = now + Duration::from_millis(120);
    app.update_touch(device, 8, TouchPhase::Started, start, now);
    assert_eq!(app.refresh_deadline(now), Some(deadline));
    app.tick(deadline - Duration::from_millis(1));
    assert!(app.component.clicks.is_empty());
    app.tick(deadline);
    app.tick(deadline + Duration::from_millis(1));
    assert_eq!(app.component.clicks, ["press-and-hold"]);
    assert!(app.component.pointer.pressed());
    app.update_touch(
        device,
        8,
        TouchPhase::Ended,
        start,
        deadline + Duration::from_millis(20),
    );
    assert_eq!(app.component.clicks, ["press-and-hold"]);
    assert_eq!(
        app.component.events.last().unwrap().kind,
        PointerEventKind::Up
    );
    assert!(!app.component.pointer.pressed());
    app.component.clicks.clear();
    app.component.events.clear();

    let mouse = PointerContact::Mouse(PointerButton::Primary);
    app.input
        .pointer_move(&app.scene, app.layout.as_ref().unwrap(), start);
    app.left_pointer(ElementState::Pressed, None);
    app.start_pointer(mouse, start, now);
    app.release_pointer(mouse, start, deadline);
    app.left_pointer(
        ElementState::Released,
        Some(PointerClick {
            touch_device: None,
            point: start,
            time: deadline,
        }),
    );
    assert_eq!(app.component.clicks, ["press-and-hold"]);
    assert_eq!(
        app.component.events.last().unwrap().kind,
        PointerEventKind::Up
    );
    app.component.clicks.clear();
    app.component.events.clear();

    app.update_touch(device, 0, TouchPhase::Started, start, now);
    let capture = app.pointer_capture.as_ref().unwrap();
    assert!(app.input.owns_pointer_press(capture.sequence.target()));
    for enabled in [false, true] {
        Rc::make_mut(&mut app.scene).children[0]
            .properties
            .insert("enabled", enabled.into());
        let layout = Rc::new(layout_with_input(
            &app.scene,
            Size::new(180.0, 120.0),
            &mut Measure,
            &app.input,
        ));
        app.input.reconcile(&app.scene, &layout);
        app.layout = Some(layout);
        app.reconcile_pointer();
    }
    app.update_touch(
        device,
        0,
        TouchPhase::Ended,
        start,
        now + Duration::from_millis(10),
    );
    assert!(app.component.clicks.is_empty());
    assert_eq!(
        app.component
            .events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [PointerEventKind::Down, PointerEventKind::Cancel]
    );
    app.component.events.clear();
    for (id, phase, y, millis) in [
        (1, TouchPhase::Started, 50.0, 0),
        (2, TouchPhase::Started, 50.0, 5),
        (2, TouchPhase::Ended, 50.0, 10),
        (1, TouchPhase::Moved, 49.0, 15),
        (1, TouchPhase::Ended, 49.0, 20),
        (3, TouchPhase::Started, 51.0, 130),
        (3, TouchPhase::Ended, 51.0, 150),
        (4, TouchPhase::Started, 50.0, 180),
        (4, TouchPhase::Ended, 50.0, 200),
        (5, TouchPhase::Started, 50.0, 220),
        (5, TouchPhase::Cancelled, 50.0, 230),
        (6, TouchPhase::Started, 50.0, 245),
        (6, TouchPhase::Ended, 50.0, 255),
        (7, TouchPhase::Started, 50.0, 280),
        (7, TouchPhase::Moved, 10.0, 300),
        (7, TouchPhase::Ended, 8.0, 320),
    ] {
        app.update_touch(
            device,
            id,
            phase,
            Point { y, ..start },
            now + Duration::from_millis(millis),
        );
    }
    let kinds: Vec<_> = app
        .component
        .events
        .iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            PointerEventKind::Down,
            PointerEventKind::Move,
            PointerEventKind::Up,
            PointerEventKind::Down,
            PointerEventKind::Up,
            PointerEventKind::Down,
            PointerEventKind::Up,
            PointerEventKind::Down,
            PointerEventKind::Cancel,
            PointerEventKind::Down,
            PointerEventKind::Up,
            PointerEventKind::Down,
            PointerEventKind::Cancel,
        ]
    );
    assert_eq!(
        app.component.clicks,
        ["clicked", "clicked", "double-clicked", "clicked", "clicked"]
    );
    assert!(app.input.scroll_offsets()[&[][..]] > 0.0);
    assert!(app.pointer_capture.is_none());
    assert!(app.component.events.iter().all(|event| event.is_touch));

    app.component.clicks.clear();
    app.component.events.clear();
    for (x, millis) in [
        (30.0, 400),
        (32.0, 500),
        (30.0, 600),
        (60.0, 700),
        (60.0, 1250),
    ] {
        let point = Point { x, ..start };
        let time = now + Duration::from_millis(millis);
        let mouse = PointerContact::Mouse(PointerButton::Primary);
        app.input
            .pointer_move(&app.scene, app.layout.as_ref().unwrap(), point);
        app.left_pointer(ElementState::Pressed, None);
        app.start_pointer(mouse, point, time);
        app.release_pointer(mouse, point, time);
        app.left_pointer(
            ElementState::Released,
            Some(PointerClick {
                touch_device: None,
                point,
                time,
            }),
        );
    }
    assert!(
        app.component
            .events
            .iter()
            .all(|event| !event.is_touch && event.touch_finger_id == 0)
    );
    assert_eq!(
        app.component.clicks,
        [
            "clicked",
            "clicked",
            "double-clicked",
            "clicked",
            "clicked",
            "clicked"
        ]
    );
}

#[test]
fn native_swipe_routes_mouse_touch_scroll_and_callback_cancellation() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::{Length, SwipeHandle};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    struct View {
        scene: Node,
        handle: SwipeHandle,
        events: Vec<(String, bool, f64)>,
        pointer_events: Vec<PointerEvent>,
        taps: usize,
        cancel_on_move: bool,
    }
    impl Component for View {
        fn render(&self) -> Node {
            self.scene.clone()
        }
        fn pointer_event_at(&mut self, _: &Node, path: &[usize], event: &PointerEvent) -> bool {
            assert_eq!(path, &[0, 0]);
            self.pointer_events.push(*event);
            true
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            if id == "tap" {
                if event != "clicked" {
                    return false;
                }
                assert_eq!(
                    self.pointer_events.last().unwrap().kind,
                    PointerEventKind::Up
                );
                self.taps += 1;
            } else {
                assert_eq!(id, "swipe");
                self.events.push((
                    event.into(),
                    self.handle.swiping(),
                    self.handle.current_x().0 - self.handle.pressed_x().0,
                ));
                if self.cancel_on_move && event == "moved" {
                    self.handle.cancel();
                }
            }
            true
        }
    }
    let handle = SwipeHandle::default();
    let mut view = Node::new("ScrollView", None);
    view.properties
        .insert("content-height", Length(400.0).into());
    view.properties
        .insert("mouse-drag-pan-enabled", true.into());
    let mut swipe = Node::new("SwipeGestureHandler", Some("swipe"));
    swipe.properties.insert("width", Length(180.0).into());
    swipe.properties.insert("height", Length(400.0).into());
    swipe.properties.insert("handle-swipe-right", true.into());
    swipe.swipe_handle = Some(handle.clone());
    let mut area = Node::new("TouchArea", Some("tap"));
    area.properties.insert("width", Length(100.0).into());
    area.properties.insert("height", Length(80.0).into());
    area.pointer_handle = Some(Default::default());
    swipe.children.push(area);
    view.children.push(swipe);
    let mut app = Application::new(
        View {
            scene: view,
            handle,
            events: Vec::new(),
            pointer_events: Vec::new(),
            taps: 0,
            cancel_on_move: false,
        },
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(180.0, 140.0);
    app.layout = Some(Rc::new(layout_with_input(
        &app.scene,
        viewport,
        &mut Measure,
        &app.input,
    )));
    let now = Instant::now();
    let device = DeviceId::dummy();
    let start = Point { x: 36.0, y: 40.0 };
    let moved = Point { x: 76.0, y: 40.0 };
    let released = Point { x: 96.0, y: 40.0 };
    let mouse = PointerContact::Mouse(PointerButton::Primary);
    app.begin_direct_scroll(ScrollContact::MouseControl(start), start, now);
    app.start_pointer(mouse, start, now);
    app.move_direct_scroll(moved, now + Duration::from_millis(20));
    app.move_mouse_pointer(moved);
    app.release_pointer(mouse, released, now + Duration::from_millis(30));
    app.end_direct_scroll(released, now + Duration::from_millis(30));
    assert_eq!(
        app.component.events,
        [
            ("moved".into(), true, 40.0),
            ("moved".into(), true, 60.0),
            ("swiped".into(), false, 60.0),
        ]
    );
    assert_eq!(
        app.component
            .pointer_events
            .iter()
            .map(|event| (event.kind, event.is_touch))
            .collect::<Vec<_>>(),
        [
            (PointerEventKind::Down, false),
            (PointerEventKind::Cancel, false)
        ]
    );
    assert_eq!(app.component.taps, 0);
    app.update_touch(device, 1, TouchPhase::Started, start, now);
    app.update_touch(device, 1, TouchPhase::Ended, start, now);
    assert_eq!(app.component.taps, 1);

    app.component.cancel_on_move = true;
    app.update_touch(device, 2, TouchPhase::Started, start, now);
    app.update_touch(
        device,
        2,
        TouchPhase::Moved,
        moved,
        now + Duration::from_millis(20),
    );
    app.update_touch(
        device,
        2,
        TouchPhase::Ended,
        released,
        now + Duration::from_millis(30),
    );
    assert_eq!(
        &app.component.events[3..],
        &[
            ("moved".into(), true, 40.0),
            ("cancelled".into(), false, 40.0)
        ]
    );
    assert_eq!(app.component.taps, 1);

    app.component.cancel_on_move = false;
    app.update_touch(device, 3, TouchPhase::Started, start, now);
    app.update_touch(
        device,
        3,
        TouchPhase::Moved,
        moved,
        now + Duration::from_millis(20),
    );
    Rc::make_mut(&mut app.scene).children[0]
        .properties
        .insert("enabled", false.into());
    app.update_touch(
        device,
        3,
        TouchPhase::Ended,
        released,
        now + Duration::from_millis(30),
    );
    assert_eq!(
        &app.component.events[5..],
        &[
            ("moved".into(), true, 40.0),
            ("cancelled".into(), false, 40.0)
        ]
    );
    app.update_touch(device, 4, TouchPhase::Started, start, now);
    app.update_touch(device, 4, TouchPhase::Ended, start, now);
    assert_eq!(app.component.taps, 2);

    let vertical_start = Point { x: 50.0, y: 105.0 };
    let vertical_end = Point { x: 50.0, y: 50.0 };
    app.update_touch(device, 5, TouchPhase::Started, vertical_start, now);
    app.update_touch(
        device,
        5,
        TouchPhase::Moved,
        vertical_end,
        now + Duration::from_millis(20),
    );
    app.update_touch(
        device,
        5,
        TouchPhase::Ended,
        vertical_end,
        now + Duration::from_millis(30),
    );
    assert!(app.input.scroll_offsets()[&[][..]] > 40.0);
    assert_eq!(app.component.events.len(), 7);
    assert_eq!(app.component.taps, 2);
}

#[test]
fn native_multi_touch_keeps_contact_ids_and_releases_each_contact_independently() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    struct View {
        area: Node,
        events: Vec<PointerEvent>,
        clicks: usize,
    }
    impl Component for View {
        fn render(&self) -> Node {
            self.area.clone()
        }
        fn pointer_event_at(&mut self, _: &Node, path: &[usize], event: &PointerEvent) -> bool {
            assert!(path.is_empty());
            self.events.push(*event);
            true
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            if event != "clicked" {
                return false;
            }
            assert_eq!(id, "area");
            self.clicks += 1;
            true
        }
    }
    let handle = PointerHandle::default();
    let mut area = Node::new("TouchArea", Some("area"));
    area.pointer_handle = Some(handle.clone());
    area.properties.insert("width", Length(180.0).into());
    area.properties.insert("height", Length(140.0).into());
    area.properties.insert("multi-touch", true.into());
    let mut app = Application::new(
        View {
            area,
            events: Vec::new(),
            clicks: 0,
        },
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    app.layout = Some(Rc::new(layout(
        &app.scene,
        Size::new(180.0, 140.0),
        &mut Measure,
    )));
    let device = DeviceId::dummy();
    let now = Instant::now();
    let first = Point { x: 20.0, y: 30.0 };
    let second = Point { x: 80.0, y: 45.0 };
    app.update_touch(device, 0, TouchPhase::Started, first, now);
    app.update_touch(device, u64::MAX, TouchPhase::Started, second, now);
    assert_eq!(handle.active_touch_count(), 2);
    let ids = [
        app.component.events[0].touch_finger_id,
        app.component.events[1].touch_finger_id,
    ];
    assert!(ids[0] > 0 && ids[1] > 0 && ids[0] != ids[1]);
    app.update_touch(device, 0, TouchPhase::Ended, first, now);
    assert_eq!(app.component.events[2].touch_finger_id, ids[0]);
    assert_eq!(app.component.events[2].kind, PointerEventKind::Up);
    assert!(handle.pressed() && handle.contains_press());
    assert_eq!(handle.active_touch_count(), 1);
    app.update_touch(
        device,
        u64::MAX,
        TouchPhase::Moved,
        Point { x: 95.0, ..second },
        now,
    );
    assert_eq!(app.component.events[3].touch_finger_id, ids[1]);
    assert_eq!(app.component.events[3].x, Length(95.0));
    app.update_touch(device, u64::MAX, TouchPhase::Cancelled, second, now);
    assert_eq!(app.component.events[4].touch_finger_id, ids[1]);
    assert_eq!(app.component.events[4].kind, PointerEventKind::Cancel);
    assert_eq!(handle.active_touch_count(), 0);
    assert!(!handle.pressed());
    assert_eq!(app.component.clicks, 0);

    app.update_touch(device, 0, TouchPhase::Started, first, now);
    app.update_touch(device, 0, TouchPhase::Ended, first, now);
    let reused = app.component.events[5].touch_finger_id;
    assert!(!ids.contains(&reused));
    assert_eq!(app.component.events[6].touch_finger_id, reused);
    assert_eq!(app.component.clicks, 1);

    app.update_touch(device, 3, TouchPhase::Started, first, now);
    app.update_touch(device, 4, TouchPhase::Started, second, now);
    let extra_id = app.component.events.last().unwrap().touch_finger_id;
    Rc::make_mut(&mut app.scene)
        .properties
        .insert("multi-touch", false.into());
    app.reconcile_pointer();
    assert_eq!(handle.active_touch_count(), 1);
    assert_eq!(
        app.component.events.last().unwrap().kind,
        PointerEventKind::Cancel
    );
    assert_eq!(
        app.component.events.last().unwrap().touch_finger_id,
        extra_id
    );
    app.update_touch(device, 4, TouchPhase::Ended, second, now);
    app.update_touch(device, 3, TouchPhase::Ended, first, now);
    assert_eq!(handle.active_touch_count(), 0);
    assert_eq!(app.component.clicks, 1);
}

#[test]
fn native_scale_rotate_arbitrates_touch_and_combines_trackpad_channels() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Length, ScaleRotateHandle, ScaleRotateSample};
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    struct View {
        node: Node,
        events: Vec<(String, bool, ScaleRotateSample)>,
        pointers: Vec<(Vec<usize>, PointerEvent)>,
        taps: usize,
    }
    impl Component for View {
        fn render(&self) -> Node {
            self.node.clone()
        }
        fn pointer_event_at(&mut self, _: &Node, path: &[usize], event: &PointerEvent) -> bool {
            self.pointers.push((path.to_vec(), *event));
            true
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            if id == "tap" {
                if event == "clicked" {
                    self.taps += 1;
                    return true;
                }
                return false;
            }
            if id == "independent" {
                return false;
            }
            assert_eq!(id, "gesture");
            let handle = self.node.children[0].scale_rotate_handle.as_ref().unwrap();
            self.events.push((
                event.to_owned(),
                handle.active(),
                ScaleRotateSample {
                    scale: handle.scale(),
                    rotation: handle.rotation(),
                    center_x: handle.center_x(),
                    center_y: handle.center_y(),
                },
            ));
            true
        }
    }
    let mut area = Node::new("ScaleRotateGestureHandler", Some("gesture"));
    area.scale_rotate_handle = Some(ScaleRotateHandle::default());
    for (name, value) in [
        ("x", 20.0),
        ("y", 10.0),
        ("width", 200.0),
        ("height", 160.0),
    ] {
        area.properties.insert(name, Length(value).into());
    }
    let mut button = Node::new("TouchArea", Some("tap"));
    button.pointer_handle = Some(PointerHandle::default());
    for (name, value) in [("width", 100.0), ("height", 80.0)] {
        button.properties.insert(name, Length(value).into());
    }
    area.children.push(button);
    let mut window = Node::new("Window", None);
    window.children.push(area);
    let mut independent = Node::new("TouchArea", Some("independent"));
    independent.pointer_handle = Some(PointerHandle::default());
    independent.properties.insert("multi-touch", true.into());
    for (name, value) in [("x", 230.0), ("width", 40.0), ("height", 80.0)] {
        independent.properties.insert(name, Length(value).into());
    }
    window.children.push(independent);
    let mut app = Application::new(
        View {
            node: window,
            events: Vec::new(),
            pointers: Vec::new(),
            taps: 0,
        },
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(280.0, 220.0);
    app.layout = Some(Rc::new(layout(&app.scene, viewport, &mut Measure)));
    let now = Instant::now();
    let device = DeviceId::dummy();
    let first = Point { x: 50.0, y: 55.0 };
    let second = Point { x: 90.0, y: 55.0 };
    let moved = Point { x: 50.0, y: 135.0 };
    app.update_touch(device, 1, TouchPhase::Started, first, now);
    let third = Point { x: 245.0, y: 35.0 };
    app.update_touch(device, 9, TouchPhase::Started, third, now);
    app.update_touch(device, 2, TouchPhase::Started, second, now);
    app.update_touch(
        device,
        9,
        TouchPhase::Moved,
        Point { x: 250.0, ..third },
        now,
    );
    app.update_touch(device, 9, TouchPhase::Ended, third, now);
    let pointer_events = &app.component.pointers;
    assert_eq!(
        pointer_events
            .iter()
            .map(|(path, event)| (path.as_slice(), event.kind))
            .collect::<Vec<_>>(),
        [
            (&[0, 0][..], PointerEventKind::Down),
            (&[1][..], PointerEventKind::Down),
            (&[0, 0][..], PointerEventKind::Cancel),
            (&[1][..], PointerEventKind::Move),
            (&[1][..], PointerEventKind::Up),
        ]
    );
    assert_eq!(
        pointer_events[0].1.touch_finger_id,
        pointer_events[2].1.touch_finger_id
    );
    assert_eq!(
        pointer_events[1].1.touch_finger_id,
        pointer_events[4].1.touch_finger_id
    );
    app.update_touch(device, 2, TouchPhase::Moved, moved, now);
    assert_eq!(
        app.component.events[1],
        (
            "updated".into(),
            true,
            ScaleRotateSample {
                scale: 2.0,
                rotation: 90.0,
                center_x: Length(30.0),
                center_y: Length(85.0),
            }
        )
    );
    let lifted = Point { x: 50.0, y: 155.0 };
    app.update_touch(device, 2, TouchPhase::Ended, lifted, now);
    app.update_touch(device, 1, TouchPhase::Ended, first, now);
    assert_eq!(app.component.events[2].0, "updated");
    assert_eq!(app.component.events[2].2.scale, 2.5);
    assert_eq!(
        app.component.events[3],
        ("ended".into(), false, ScaleRotateSample::default())
    );
    assert_eq!(app.component.taps, 0);
    app.update_touch(device, 3, TouchPhase::Started, first, now);
    app.update_touch(device, 3, TouchPhase::Ended, first, now);
    assert_eq!(app.component.taps, 1);

    app.input
        .pointer_move(&app.scene, app.layout.as_ref().unwrap(), first);
    use scale_rotate::TrackpadDelta::{Rotation, Scale};
    app.update_trackpad_gesture(device, TouchPhase::Started, Scale(0.25));
    app.update_trackpad_gesture(device, TouchPhase::Started, Rotation(-30.0));
    app.update_trackpad_gesture(device, TouchPhase::Ended, Scale(0.0));
    assert!(app.scale_rotate.is_active());
    let (_, active, sample) = app.component.events.last().unwrap();
    assert!(*active);
    assert_eq!((sample.scale, sample.rotation), (1.25, 30.0));
    app.update_trackpad_gesture(device, TouchPhase::Cancelled, Rotation(0.0));
    assert_eq!(
        app.component.events.last().unwrap(),
        &("cancelled".into(), false, ScaleRotateSample::default())
    );
    assert!(!app.scale_rotate.is_active());
    app.update_trackpad_gesture(device, TouchPhase::Started, Scale(0.0));
    app.update_trackpad_gesture(device, TouchPhase::Ended, Scale(0.2));
    let events = &app.component.events;
    assert_eq!(events[events.len() - 2].2.scale, 1.2);
    assert_eq!(events.last().unwrap().0, "ended");

    app.update_touch(device, 4, TouchPhase::Started, first, now);
    app.update_touch(device, 5, TouchPhase::Started, second, now);
    Rc::make_mut(&mut app.scene).children[0]
        .properties
        .insert("enabled", false.into());
    app.update_touch(device, 5, TouchPhase::Moved, moved, now);
    assert_eq!(app.component.events.last().unwrap().0, "cancelled");
    app.update_touch(device, 5, TouchPhase::Ended, moved, now);
    app.update_touch(device, 4, TouchPhase::Ended, first, now);
    assert_eq!(app.component.taps, 1);
}

#[test]
fn native_wheel_preserves_another_spin_draft_and_active_ime_composition() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Length, spin_value_after};

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
            if let [slot] = path
                && let ControlAction::Spin(action) = action
                && let Some(value) = scene
                    .at_path(path)
                    .and_then(|node| spin_value_after(node, action))
            {
                self.0.children[*slot]
                    .properties
                    .insert("value", value.into());
                return true;
            }
            false
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    let mut scene = Node::new("Window", None);
    for (id, x) in [("editor", 0.0), ("other", 128.0)] {
        let mut spin = Node::new("SpinBox", Some(id));
        for (name, value) in [("x", x), ("width", 100.0), ("height", 40.0)] {
            spin.properties.insert(name, Length(value).into());
        }
        spin.properties.insert("wheel-enabled", true.into());
        spin.properties.insert("value", 10_i64.into());
        scene.children.push(spin);
    }
    let mut app = Application::new(
        View(scene),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let frame = Rc::new(layout(&app.scene, Size::new(250.0, 100.0), &mut Measure));
    app.layout = Some(frame.clone());
    let target = app
        .input
        .accessibility_target(&app.scene, &frame, &[0])
        .unwrap();
    app.input.focus_target(&app.scene, &frame, target);
    app.input
        .edit_spin(&app.scene.children[0], EditAction::ReplaceAll("37"));
    assert_eq!(app.input.editor_value(&app.scene.children[0]), "37");
    app.input
        .pointer_move(&app.scene, &frame, Point { x: 158.0, y: 18.0 });
    let event = app.wheel_event(Point { x: 0.0, y: -28.0 }, Some(Point { x: 0.0, y: -1.0 }));
    assert!(app.adjust_wheel(event, Instant::now()));
    assert_eq!(app.component.0.children[1].int("value"), Some(11));
    assert_eq!(app.input.editor_value(&app.scene.children[0]), "37");

    app.input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("3".into(), None);
    app.input
        .pointer_move(&app.scene, &frame, Point { x: 40.0, y: 18.0 });
    assert!(!app.adjust_wheel(event, Instant::now()));
    assert_eq!(app.component.0.children[0].int("value"), Some(10));
    assert_eq!(app.input.editor_value(&app.scene.children[0]), "37");
}

#[test]
fn native_scroll_bar_touch_release_snaps_and_cancellation_keeps_the_dragged_position() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::{Length, ScrollSnapMode, ViewportHandle};

    struct View(Node);
    impl Component for View {
        fn render(&self) -> Node {
            self.0.clone()
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    let handle = ViewportHandle::default();
    let mut view = Node::new("ScrollView", Some("view"));
    view.viewport_handle = Some(handle.clone());
    for (name, value) in [
        ("width", 100.0),
        ("height", 100.0),
        ("content-height", 500.0),
    ] {
        view.properties.insert(name, Length(value).into());
    }
    let mut bar = Node::new("ScrollBar", Some("bar"));
    bar.scroll_target = Some(handle);
    bar.properties.insert("x", Length(120.0).into());
    bar.properties.insert("width", Length(12.0).into());
    bar.properties.insert("height", Length(100.0).into());
    bar.properties
        .insert("snap-mode", ScrollSnapMode::OnRelease.into());
    bar.properties.insert("step-size", 0.25_f64.into());
    let mut window = Node::new("Window", None);
    window.children.extend([view, bar]);
    let mut app = Application::new(
        View(window),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let viewport = Size::new(160.0, 120.0);
    let now = Instant::now();
    let device = DeviceId::dummy();
    for (id, fraction, phase, expected) in [
        (1, 0.44, TouchPhase::Ended, 200.0),
        (2, 0.31, TouchPhase::Cancelled, 124.0),
    ] {
        let layout = layout_with_input(&app.scene, viewport, &mut Measure, &app.input);
        let bar = layout.children[1].attached_scroll_geometry().unwrap();
        let start = Point {
            x: bar.thumb.x + 6.0,
            y: bar.thumb.y + bar.thumb.height * 0.5,
        };
        let end = Point {
            x: start.x,
            y: bar.track.y
                + bar.thumb.height * 0.5
                + (bar.track.height - bar.thumb.height) * fraction,
        };
        app.layout = Some(Rc::new(layout));
        app.update_touch(device, id, TouchPhase::Started, start, now);
        app.update_touch(device, id, TouchPhase::Moved, end, now);
        let raw = app.input.scroll_offsets()[&[0][..]];
        assert!((raw - fraction * 400.0).abs() < 0.001);
        app.update_touch(device, id, phase, end, now);
        assert!((app.input.scroll_offsets()[&[0][..]] - expected).abs() < 0.001);
        assert!(!app.input.scroll_bar_captured());
    }
}

#[test]
fn touch_table_range_commits_only_the_owned_contact_on_release() {
    touch_table_drag_commits_only_the_owned_contact(false);
}

#[test]
fn touch_table_reorder_commits_only_the_owned_contact_on_release() {
    touch_table_drag_commits_only_the_owned_contact(true);
}

fn touch_table_drag_commits_only_the_owned_contact(reorder: bool) {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{
        Length, ListMetrics, SelectionTarget, TableHeader, TableSelection, VecModel,
    };

    struct View {
        model: VecModel<i64>,
        selection: TableSelection,
        actions: Vec<ControlAction>,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut window = Node::new("Window", None);
            let mut table = Node::new("TableView", Some("table"));
            table.properties.insert("width", Length(180.0).into());
            table.properties.insert("height", Length(120.0).into());
            table.properties.insert("drag-select", true.into());
            table
                .properties
                .insert("row-header-width", Length(24.0).into());
            table.properties.insert("movable-rows", true.into());
            table.standard_list_stamp = Some((10, 1));
            table.table_header = Some(Box::new(TableHeader {
                columns: vec![Node::new("TableColumn", None)],
                sort: None,
                column_widths: Vec::new(),
            }));
            table.table_selection = Some(Box::new(self.selection.snapshot(
                &self.model,
                None,
                SelectionTarget::Cells,
                1,
                true,
            )));
            table.list = Some(ListMetrics {
                row_count: 2,
                first_row: 0,
            });
            table.children.extend((0..2).map(|index| {
                let mut row = Node::new("ListRow", None);
                row.row_key = self.model.row_key(index);
                row.properties
                    .insert("row-header-text", (index + 1).to_string().into());
                row
            }));
            window.children.push(table);
            window
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn control_at(&mut self, _: &Node, path: &[usize], action: ControlAction) -> bool {
            assert_eq!(path, [0]);
            self.actions.push(action);
            true
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut app = Application::new(
        View {
            model: VecModel::from(vec![0, 1]),
            selection: TableSelection::default(),
            actions: Vec::new(),
        },
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    let frame = layout(&app.scene, Size::new(200.0, 130.0), &mut Measure);
    let table = &frame.children[0];
    let x = if reorder { 12.0 } else { 30.0 };
    let row = app.component.model.row_key(0).unwrap();
    let start = Point {
        x,
        y: table.children[0].bounds.y + 16.0,
    };
    let end = Point {
        x,
        y: table.children[1].bounds.y + 24.0,
    };
    app.layout = Some(Rc::new(frame));
    let device = DeviceId::dummy();
    let now = Instant::now();
    let deadline = now + Duration::from_millis(500);
    if reorder {
        app.update_touch(device, 0, TouchPhase::Started, start, now);
        app.update_touch(
            device,
            0,
            TouchPhase::Ended,
            start,
            now + Duration::from_millis(100),
        );
        assert_eq!(
            app.component.actions.remove(0),
            ControlAction::TableSelection(xrack_runtime::TableSelectionAction::Row {
                index: 0,
                operation: RowSelection::Replace
            },)
        );
    }
    app.update_touch(device, 1, TouchPhase::Started, start, now);
    app.tick(deadline);
    assert!(app.input.touch_table_drag_claimed());
    assert_eq!(app.input.table_row_move_claimed(), reorder);
    app.update_touch(device, 1, TouchPhase::Moved, end, deadline);
    app.update_touch(device, 2, TouchPhase::Ended, end, deadline);
    assert!(app.component.actions.is_empty());
    app.update_touch(device, 1, TouchPhase::Cancelled, end, deadline);
    assert!(!app.input.touch_table_drag_claimed());
    assert!(app.component.actions.is_empty());

    app.update_touch(device, 3, TouchPhase::Started, start, now);
    app.update_touch(device, 3, TouchPhase::Moved, end, deadline);
    assert!(app.input.touch_table_drag_claimed());
    app.update_touch(device, 3, TouchPhase::Ended, end, deadline);
    let expected = if reorder {
        ControlAction::MoveRow {
            from: 0,
            to: 1,
            row,
            stamp: (10, 1),
        }
    } else {
        ControlAction::TableSelection(xrack_runtime::TableSelectionAction::DragRange {
            anchor: (0, 0),
            end: (1, 0),
            additive: false,
        })
    };
    assert_eq!(app.component.actions, [expected]);
    assert!(app.scroll_contact.is_none());

    if reorder {
        let target = app
            .input
            .accessibility_target(&app.scene, app.layout.as_ref().unwrap(), &[0, 0])
            .unwrap();
        let request = |id| ActionRequest {
            action: AccessibleAction::CustomAction,
            target_tree: TreeId::ROOT,
            target_node: accesskit::NodeId(1),
            data: Some(ActionData::CustomAction(id)),
        };
        app.component.actions.clear();
        app.accessibility_action(Route::TableRowHeader(target.clone()), request(0));
        assert!(app.component.actions.is_empty());
        app.accessibility_action(Route::TableRowHeader(target), request(1));
        assert_eq!(app.component.actions, [expected]);
    }
}

#[test]
fn touch_drag_claims_one_contact_and_dispatches_drop_or_cancel() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{DragEvent, Length};

    #[derive(Default)]
    struct View(Vec<DragEvent>);
    impl Component for View {
        fn render(&self) -> Node {
            let mut window = Node::new("Window", None);
            let area = |kind, x| {
                let mut node = Node::new(kind, None);
                for (property, value) in [("x", x), ("width", 80.0), ("height", 60.0)] {
                    node.properties.insert(property, Length(value).into());
                }
                node
            };
            let mut source = area("DragArea", 0.0);
            source.properties.insert("allow-copy", true.into());
            source.properties.insert("data", "touch".to_owned().into());
            window.children.extend([source, area("DropArea", 100.0)]);
            window
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn drag_event_at(&mut self, _: &Node, _: &[usize], event: &DragEvent) -> bool {
            self.0.push(event.clone());
            true
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut app = Application::new(
        View::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut View, _| {},
    );
    app.layout = Some(Rc::new(layout(
        &app.scene,
        Size::new(200.0, 100.0),
        &mut Measure,
    )));
    let start = Point { x: 20.0, y: 22.0 };
    let end = Point { x: 120.0, y: 22.0 };
    let now = Instant::now();
    let device = DeviceId::dummy();
    app.update_touch(device, 1, TouchPhase::Started, start, now);
    app.update_touch(device, 2, TouchPhase::Moved, end, now);
    assert!(app.component.0.is_empty());
    app.update_touch(device, 1, TouchPhase::Moved, end, now);
    app.update_touch(device, 1, TouchPhase::Ended, end, now);
    assert_eq!(
        app.component.0,
        [
            DragEvent::Started,
            DragEvent::Entered {
                data: "touch".into(),
                action: DragAction::Copy
            },
            DragEvent::Moved {
                x: Length(20.0),
                y: Length(22.0)
            },
            DragEvent::Dropped {
                data: "touch".into(),
                action: DragAction::Copy
            },
            DragEvent::Finished(DragAction::Copy),
        ]
    );
    assert!(app.scroll_contact.is_none());
    app.component.0.clear();
    app.update_touch(device, 3, TouchPhase::Started, start, now);
    app.update_touch(device, 3, TouchPhase::Moved, end, now);
    app.update_touch(device, 3, TouchPhase::Cancelled, end, now);
    assert!(
        app.component
            .0
            .ends_with(&[DragEvent::Exited, DragEvent::Finished(DragAction::None)])
    );
    assert!(
        !app.component
            .0
            .iter()
            .any(|event| matches!(event, DragEvent::Dropped { .. }))
    );
}

#[test]
fn clipboard_reply_commits_only_for_the_current_selection() {
    struct Editor(String);
    impl Component for Editor {
        fn render(&self) -> Node {
            let mut node = Node::new("TextEdit", None);
            node.properties.insert("text", self.0.clone().into());
            node
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn edit_at(&mut self, _: &Node, _: &[usize], value: String) -> bool {
            self.0 = value;
            true
        }
    }
    let mut app = Application::new(
        Editor("初始".into()),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_: &mut Editor, _| {},
    );
    app.input.focused = Some(Target {
        text_link: None,
        path: vec![],
        id: None,
        focusable: true,
        row_key: None,
    });
    app.input.sync_editor(&app.scene);
    app.input
        .editor
        .as_mut()
        .unwrap()
        .apply_multiline("初始", EditAction::SelectAll);

    let context = EditContext::capture(&app.input, &app.scene).unwrap();
    assert_eq!(context.selection(), "初始");
    app.pending_clipboard = Some((1, ClipboardContext::Edit(context)));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 1,
        result: Err("test write failure".into()),
    }));
    assert_eq!(app.component.0, "初始");

    app.pending_clipboard = Some((
        5,
        ClipboardContext::Edit(EditContext::capture(&app.input, &app.scene).unwrap()),
    ));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 5,
        result: Ok(ClipboardValue::Text(String::new())),
    }));
    assert_eq!(app.component.0, "初始");

    app.pending_clipboard = Some((
        2,
        ClipboardContext::Edit(EditContext::capture(&app.input, &app.scene).unwrap()),
    ));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 2,
        result: Ok(ClipboardValue::Written),
    }));
    assert_eq!(app.base_scene.text("text"), Some(""));
    app.scene = app.base_scene.clone();
    app.pending_clipboard = Some((
        3,
        ClipboardContext::Edit(EditContext::capture(&app.input, &app.scene).unwrap()),
    ));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 3,
        result: Ok(ClipboardValue::Text("一\r\n二".into())),
    }));
    assert_eq!(app.base_scene.text("text"), Some("一\n二"));
    app.scene = app.base_scene.clone();
    app.pending_clipboard = Some((
        4,
        ClipboardContext::Edit(EditContext::capture(&app.input, &app.scene).unwrap()),
    ));
    app.component.0 = "后台更新".into();
    app.scene = app.component.render_shared();
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 4,
        result: Ok(ClipboardValue::Text("过期粘贴".into())),
    }));
    assert_eq!(app.component.0, "后台更新");
}

#[test]
fn vetoed_close_refreshes_the_scene_and_consumes_synchronous_retries() {
    #[derive(Default)]
    struct Editor {
        close: bool,
        can_close: bool,
        requests: i64,
    }
    impl Component for Editor {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            root.properties.insert("requests", self.requests.into());
            root
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn close_requested(&mut self) -> bool {
            self.requests += 1;
            self.close = true;
            self.can_close
        }

        fn take_window_close_request(&mut self) -> bool {
            std::mem::take(&mut self.close)
        }
    }

    let mut app = Application::new(
        Editor::default(),
        LocalExecutor::new(|| {}),
        Inbox::new(|| {}),
        |_: &mut Editor, _: ()| {},
    );
    assert!(!app.request_close());
    assert_eq!(app.base_scene.int("requests"), Some(1));
    assert!(!app.take_close_request());
    app.component.can_close = true;
    assert!(app.request_close());
    assert_eq!(app.component.requests, 2);
    assert!(!app.take_close_request());
}

#[test]
fn closing_one_window_cancels_its_tasks_and_mailbox_without_affecting_another() {
    let make = || {
        let (notify, events) = mpsc::channel();
        let task_notify = notify.clone();
        let tasks = LocalExecutor::new(move || {
            let _ = task_notify.send(UserEvent::PollTasks);
        });
        let inbox = Inbox::new(move || {
            let _ = notify.send(UserEvent::ApplyMessages);
        });
        let sender = inbox.sender();
        let window = Application::new(TestUi::default(), tasks, inbox, |ui: &mut TestUi, delta| {
            let value = ui.value.clone();
            spawn_local(async move {
                value.set(value.get() + delta);
            })
            .unwrap();
        });
        (window, sender, events)
    };
    let (mut first, first_sender, first_events) = make();
    let (mut second, second_sender, second_events) = make();
    first_sender.try_send(10).unwrap();
    second_sender.try_send(20).unwrap();
    first.user_event(first_events.recv_timeout(Duration::from_secs(1)).unwrap());
    second.user_event(second_events.recv_timeout(Duration::from_secs(1)).unwrap());

    first.close();
    assert!(matches!(
        first_sender.try_send(5),
        Err(std::sync::mpsc::TrySendError::Disconnected(5))
    ));
    first.user_event(first_events.recv_timeout(Duration::from_secs(1)).unwrap());
    second.user_event(second_events.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(first.component.value.get(), 0);
    assert_eq!(second.component.value.get(), 20);
    assert!(
        spawn_local(async {}).is_err(),
        "window executor scope should be restored"
    );
}

#[test]
fn tokio_result_wakes_local_ui_task_and_mailbox() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let (notify, events) = mpsc::channel();
    let notify_tasks = notify.clone();
    let executor = LocalExecutor::new(move || notify_tasks.send(UserEvent::PollTasks).unwrap());
    let _scope = executor.enter();
    let inbox = Inbox::new(move || notify.send(UserEvent::ApplyMessages).unwrap());
    let sender = inbox.sender();
    let mut app = Application::new(
        TestUi::default(),
        executor,
        inbox,
        |ui: &mut TestUi, value| {
            ui.value.set(ui.value.get() + value);
        },
    );
    let (start, wait) = tokio::sync::oneshot::channel();
    let job = runtime.spawn(async move {
        wait.await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        sender.try_send(40).unwrap();
        2
    });
    let value = app.component.value.clone();
    let ui_thread = thread::current().id();
    drop(
        spawn_local(async move {
            let delta = job.await.unwrap();
            assert_eq!(thread::current().id(), ui_thread);
            value.set(value.get() + delta);
        })
        .unwrap(),
    );

    app.process_user_event(events.recv_timeout(Duration::from_secs(5)).unwrap());
    assert_eq!(app.base_scene.int("value"), Some(0));
    start.send(()).unwrap();
    for _ in 0..2 {
        app.process_user_event(events.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    assert_eq!(app.base_scene.int("value"), Some(42));
}

#[test]
fn frame_cache_observes_visible_rows_and_ignores_offscreen_row_updates() {
    use std::ops::Range;
    use xrack_layout::{TextMeasurer, prepare_scene};
    use xrack_runtime::{ListMetrics, Repeater, VecModel};

    struct ListUi {
        model: VecModel<String>,
        repeater: Repeater<VecModel<String>, ()>,
    }

    impl Component for ListUi {
        fn render(&self) -> Node {
            let mut node = Node::new("ListView", None);
            node.list = Some(ListMetrics {
                row_count: self.repeater.row_count(|| self.model.clone()),
                first_row: 0,
            });
            node
        }

        fn list_rows(&self, _: &[usize], range: Range<usize>) -> Vec<Node> {
            self.repeater.render_range(
                || self.model.clone(),
                range,
                |_, item, _| {
                    let mut node = Node::new("Text", None);
                    node.properties.insert("text", Value::String(item()));
                    node
                },
            )
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(10.0, 16.0)
        }
    }

    let ui = ListUi {
        model: VecModel::from(vec!["row".into(); 4]),
        repeater: Repeater::default(),
    };
    let base = ui.render_shared();
    let viewport = Size::new(180.0, 64.0);
    let input = Interaction::default();
    let mut cache = FrameCache::default();
    let mut render = || {
        cache.prepare(base.clone(), viewport, &input, |base| {
            prepare_scene(&ui, base, viewport, &mut Measure, input.scroll_offsets())
        })
    };
    let (before, initial_layout) = render();
    assert_eq!(before.children.len(), 2);
    assert!(ui.model.set_row(3, "offscreen".into()));
    let (unchanged, layout) = render();
    assert!(Rc::ptr_eq(&before, &unchanged));
    assert!(Rc::ptr_eq(&initial_layout, &layout));
    assert!(ui.model.set_row(1, "visible".into()));
    let (updated, layout) = render();
    assert!(!Rc::ptr_eq(&initial_layout, &layout));
    assert_eq!(
        updated.at_path(&[1, 0]).unwrap().text("text"),
        Some("visible")
    );
}

#[test]
fn window_timer_schedule_tracks_the_active_component_layout() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{TimerConfig, TimerSlot};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut app = Application::new(
        TestUi::default(),
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_, _| {},
    );
    let slot = TimerSlot::default();
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    for (visible, time, expected) in [
        (true, 0, Some(at(80))),
        (false, 40, None),
        (true, 200, Some(at(280))),
    ] {
        let mut window = Node::new("Window", None);
        let mut child = Node::new("Rectangle", None);
        child.properties.insert("visible", visible.into());
        child
            .timers
            .push(slot.register(TimerConfig::new(80, true, false).unwrap(), |_| true));
        window.children.push(child);
        app.scene = Rc::new(window);
        app.layout = Some(Rc::new(layout(
            &app.scene,
            Size::new(300.0, 60.0),
            &mut Measure,
        )));
        app.sync_window_effects(at(time));
        assert_eq!(app.timers.deadline(), expected);
    }
    assert!(app.timers.next_due(at(280)).unwrap().fire());
}

#[test]
fn elastic_displacement_invalidates_prepared_geometry_without_offset_changes() {
    use xrack_layout::{ScrollGestureKind, TextMeasurer, layout_with_input};
    use xrack_runtime::{FlickBoundsBehavior, Length, ViewportHandle};
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }
    let mut scene = Node::new("Flickable", None);
    scene.viewport_handle = Some(ViewportHandle::for_flickable(false));
    scene.properties.insert(
        "bounds-behavior",
        FlickBoundsBehavior::DragOverBounds.into(),
    );
    scene
        .properties
        .insert("content-height", Length(260.0).into());
    scene.children.push(Node::new("Rectangle", None));
    let scene = Rc::new(scene);
    let viewport = Size::new(120.0, 100.0);
    let mut input = Interaction::default();
    let mut cache = FrameCache::default();
    let prepare = |cache: &mut FrameCache, input: &Interaction| {
        cache
            .prepare(scene.clone(), viewport, input, |scene| {
                let layout = layout_with_input(&scene, viewport, &mut Measure, input);
                (scene, layout)
            })
            .1
    };
    let before = prepare(&mut cache, &input);
    let now = Instant::now();
    let start = Point { x: 45.0, y: 50.0 };
    let end = Point { x: 45.0, y: 105.0 };
    assert!(input.begin_scroll_gesture(&scene, &before, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &before, end, now + Duration::from_millis(20)));
    let shifted = prepare(&mut cache, &input);
    assert!(shifted.children[0].bounds.y > before.children[0].bounds.y);
    assert_eq!(shifted.scroll, before.scroll);
    input.end_scroll_gesture(&scene, &shifted, end, now + Duration::from_millis(30));
    assert!(input.advance_scroll_motion(&scene, &shifted, now + Duration::from_millis(90)));
    let returning = prepare(&mut cache, &input);
    assert!(returning.children[0].bounds.y < shifted.children[0].bounds.y);
    assert!(returning.children[0].bounds.y > before.children[0].bounds.y);
    assert_eq!(returning.scroll, before.scroll);
}

#[test]
fn swipe_preview_invalidates_the_native_frame_cache_until_canceled() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut row = Node::new("SwipeDelegate", None);
    let mut actions = Node::new("SwipeRight", None);
    actions.properties.insert("width", Length(96.0).into());
    row.children
        .extend([Node::new("SwipeContent", None), actions]);
    let scene = Rc::new(row);
    let mut cache = FrameCache::default();
    let mut input = Interaction::default();
    let viewport = Size::new(300.0, 60.0);
    let mut prepare = |input: &Interaction| {
        cache
            .prepare(scene.clone(), viewport, input, |scene| {
                let layout = layout_with_input(&scene, viewport, &mut Measure, input);
                (scene, layout)
            })
            .1
    };
    let before = prepare(&input);
    let now = Instant::now();
    let start = Point { x: 150.0, y: 28.0 };
    let moved = Point { x: 114.0, ..start };
    assert!(input.begin_scroll_gesture(
        &scene,
        &before,
        start,
        now,
        ScrollGestureKind::MouseControl
    ));
    assert!(input.move_scroll_gesture(&scene, &before, moved, now + Duration::from_millis(16)));
    let preview = prepare(&input);
    assert!(!Rc::ptr_eq(&preview, &before));
    assert_eq!(preview.children[1].clip.width, 36.0);
    input.cancel_scroll_gesture();
    let closed = prepare(&input);
    assert!(!closed.children[1].visible);
}

#[test]
fn horizontal_scroll_invalidates_cached_table_geometry() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::{Length, TableHeader};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            unreachable!("table layout does not measure its header text")
        }
    }

    let mut column = Node::new("TableColumn", None);
    column.properties.insert("width", Length(240.0).into());
    let mut table = Node::new("TableView", None);
    table.table_header = Some(Box::new(TableHeader {
        columns: vec![column],
        sort: None,
        column_widths: Vec::new(),
    }));
    let scene = Rc::new(table);
    let viewport = Size::new(120.0, 100.0);
    let mut cache = FrameCache::default();
    let mut input = Interaction::default();
    let mut prepare = |input: &Interaction| {
        cache
            .prepare(scene.clone(), viewport, input, |scene| {
                let layout = layout_with_input(&scene, viewport, &mut Measure, input);
                (scene, layout)
            })
            .1
    };
    let before = prepare(&input);
    input.pointer_move(&scene, &before, Point { x: 60.0, y: 10.0 });
    assert!(input.scroll_x(&before, 32.0));
    let after = prepare(&input);
    assert!(!Rc::ptr_eq(&before, &after));
    assert_eq!(after.table.as_ref().unwrap().columns[0].x, -32.0);
    assert!(Rc::ptr_eq(&after, &prepare(&input)));
}

#[derive(Default)]
struct TestUi {
    value: Rc<Cell<i64>>,
    renders: Cell<usize>,
}

impl Component for TestUi {
    fn render(&self) -> Node {
        self.renders.set(self.renders.get() + 1);
        let mut node = Node::new("Window", None);
        node.properties
            .insert("value", Value::Int(self.value.get()));
        node
    }

    fn dispatch(&mut self, _: &str, _: &str) -> bool {
        false
    }
}

#[test]
fn local_future_resumes_on_ui_after_worker_wake_and_refreshes_scene() {
    assert!(spawn_local(async {}).is_err());
    let (notify, events) = mpsc::channel();
    let executor = LocalExecutor::new(move || notify.send(UserEvent::PollTasks).unwrap());
    let _scope = executor.enter();
    let mut app = Application::new(
        TestUi::default(),
        executor,
        Inbox::<()>::new(|| {}),
        |_: &mut TestUi, _| {},
    );
    let value = app.component.value.clone();
    let ready = Arc::new(AtomicBool::new(false));
    let is_ready = ready.clone();
    let (waker_tx, waker_rx) = mpsc::channel::<Waker>();
    let ui_thread = thread::current().id();
    let join = spawn_local(async move {
        assert_eq!(thread::current().id(), ui_thread);
        value.set(1);
        poll_fn(|cx| {
            if is_ready.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                waker_tx.send(cx.waker().clone()).unwrap();
                Poll::Pending
            }
        })
        .await;
        assert_eq!(thread::current().id(), ui_thread);
        value.set(2);
        // Reentrant task creation must not borrow the executor's map across poll.
        drop(spawn_local(async {}).unwrap());
        42
    })
    .unwrap();
    let result = Rc::new(Cell::new(None));
    let joined = result.clone();
    drop(
        spawn_local(async move {
            joined.set(Some(join.await.unwrap()));
        })
        .unwrap(),
    );

    assert_eq!(app.base_scene.int("value"), Some(0));
    app.process_user_event(events.try_recv().unwrap());
    assert_eq!(app.base_scene.int("value"), Some(1));
    assert_eq!(result.get(), None);
    assert!(events.try_recv().is_err());
    let waker = waker_rx.try_recv().unwrap();
    thread::spawn(move || {
        ready.store(true, Ordering::Release);
        for _ in 0..100 {
            waker.wake_by_ref();
        }
    })
    .join()
    .unwrap();
    let event = events.try_recv().unwrap();
    assert!(events.try_recv().is_err());
    app.process_user_event(event);
    assert_eq!(app.base_scene.int("value"), Some(2));
    app.process_user_event(events.try_recv().unwrap());
    assert_eq!(result.get(), Some(42));
    assert!(events.try_recv().is_err());
}

#[test]
fn aborted_and_closed_tasks_drop_on_the_ui_thread() {
    struct Guard(Rc<Cell<usize>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let executor = LocalExecutor::new(|| {});
    let _scope = executor.enter();
    let drops = Rc::new(Cell::new(0));
    let task = |guard| async move {
        let _guard: Guard = guard;
        pending::<()>().await;
    };
    let mut aborted = spawn_local(task(Guard(drops.clone()))).unwrap();
    let mut stopped = spawn_local(task(Guard(drops.clone()))).unwrap();
    assert!(executor.poll_ready());
    aborted.abort();
    assert!(executor.poll_ready());
    assert_eq!(drops.get(), 1);
    executor.close();
    assert_eq!(drops.get(), 2);
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        Pin::new(&mut aborted).poll(&mut cx),
        Poll::Ready(Err(TaskCancelled))
    );
    assert_eq!(
        Pin::new(&mut stopped).poll(&mut cx),
        Poll::Ready(Err(TaskCancelled))
    );
    assert!(spawn_local(async {}).is_err());
}

#[test]
fn mailbox_bounds_backlog_and_refreshes_once_per_batch() {
    use mpsc::TrySendError;
    let (notify, events) = mpsc::channel();
    let inbox = Inbox::new(move || notify.send(UserEvent::ApplyMessages).unwrap());
    let sender = inbox.sender();
    let worker = sender.clone();
    let ui_thread = thread::current().id();
    let mut app = Application::new(
        TestUi::default(),
        LocalExecutor::new(|| {}),
        inbox,
        move |ui: &mut TestUi, message| {
            assert_eq!(thread::current().id(), ui_thread);
            assert_eq!(message, ui.value.get() + 1);
            ui.value.set(message);
        },
    );
    thread::spawn(move || {
        for value in 1..=256 {
            worker.try_send(value).unwrap();
        }
        assert_eq!(worker.try_send(257), Err(TrySendError::Full(257)));
    })
    .join()
    .unwrap();
    for batch in 1..=4 {
        let event = events.try_recv().unwrap();
        assert!(events.try_recv().is_err());
        app.process_user_event(event);
        assert_eq!(app.base_scene.int("value"), Some(batch as i64 * 64));
        assert_eq!(app.component.renders.get(), batch + 1);
    }
    assert!(events.try_recv().is_err());
    app.close_async();
    assert_eq!(sender.try_send(257), Err(TrySendError::Disconnected(257)));
}

#[test]
fn image_playback_wakes_the_window_only_at_visible_frame_boundaries() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Image, ImageAnimation, ImageAnimationFrame, ImagePlaybackSlot};

    static ANIMATION: ImageAnimation = ImageAnimation::new(
        [10, 10],
        &[
            ImageAnimationFrame {
                end_ms: 40,
                png: &[],
            },
            ImageAnimationFrame {
                end_ms: 100,
                png: &[],
            },
        ],
        0,
    );
    struct View {
        slot: ImagePlaybackSlot,
        x: f64,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut image = Node::new("AnimatedImage", None);
            image
                .properties
                .insert("source", Image::Animated(&ANIMATION).into());
            image
                .properties
                .insert("x", xrack_runtime::Length(self.x).into());
            self.slot.populate(&mut image, || false);
            let mut window = Node::new("Window", None);
            window.children.push(image);
            window
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut app = Application::new(
        View {
            slot: ImagePlaybackSlot::default(),
            x: 0.0,
        },
        LocalExecutor::new(|| {}),
        Inbox::<()>::new(|| {}),
        |_, _| {},
    );
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let place = |app: &mut Application<View, (), _>| {
        app.scene = app.component.render_shared();
        app.layout = Some(Rc::new(layout(
            &app.scene,
            Size::new(100.0, 80.0),
            &mut Measure,
        )));
    };
    place(&mut app);
    app.sync_window_effects(at(0));
    assert_eq!(app.refresh_deadline(at(0)), Some(at(40)));
    app.tick(at(40));
    assert_eq!(app.component.slot.current_frame(), 1);
    assert_eq!(app.refresh_deadline(at(40)), Some(at(100)));
    assert!(app.scene.same_frame_inputs(&app.base_scene));

    app.component.x = 200.0;
    place(&mut app);
    app.sync_window_effects(at(50));
    assert_eq!(app.refresh_deadline(at(50)), None);
    app.tick(at(200));
    assert_eq!(app.component.slot.current_frame(), 1);

    app.component.x = 0.0;
    place(&mut app);
    app.sync_window_effects(at(200));
    assert_eq!(app.refresh_deadline(at(200)), Some(at(250)));
    app.tick(at(250));
    assert_eq!(app.component.slot.current_frame(), 0);
}
