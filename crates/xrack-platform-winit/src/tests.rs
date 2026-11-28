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

mod keyboard_touch;

impl<C: Component> Application<C> {
    pub(super) fn set_test_layout(&mut self, layout: Rc<LayoutNode>) {
        let viewport = Size::new(layout.bounds.width, layout.bounds.height);
        let frame = FrameCache::default().prepare(
            self.frame.scene().clone(),
            viewport,
            &self.input,
            |scene| (scene, Rc::unwrap_or_clone(layout)),
        );
        self.frame.replace(frame);
    }

    pub(super) fn set_test_scene(&mut self, scene: Rc<Node>) {
        let layout = self.frame.layout().cloned();
        self.frame = WindowFrame::Unprepared(scene);
        if let Some(layout) = layout {
            self.set_test_layout(layout);
        }
    }

    fn edit_test_scene(&mut self, edit: impl FnOnce(&mut Node)) {
        let mut scene = self.frame.scene().clone();
        edit(Rc::make_mut(&mut scene));
        self.set_test_scene(scene);
    }
}

#[test]
fn reports_initial_system_scheme_and_skips_duplicate_notifications() {
    #[derive(Default)]
    struct View(Vec<ColorScheme>);

    impl Component for View {
        fn render(&self) -> Node {
            Node::new("Window", None)
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    let mut application = Application::new(View::default(), LocalExecutor::new(|| {}));
    application.system_color_scheme_handler = Some(Box::new(|view, scheme| view.0.push(scheme)));
    assert!(application.publish_system_color_scheme(ColorScheme::Dark));
    assert!(!application.publish_system_color_scheme(ColorScheme::Dark));
    assert!(application.publish_system_color_scheme(ColorScheme::Light));
    assert_eq!(
        application.component.0,
        [ColorScheme::Dark, ColorScheme::Light]
    );
}

#[test]
fn character_space_activates_controls_like_named_space() {
    assert_eq!(
        activation_key_value(&Key::Character(" ".into())),
        Some(ActivationKey::Space)
    );
    assert_eq!(
        activation_key_value(&Key::Named(NamedKey::Space)),
        Some(ActivationKey::Space)
    );
    assert_eq!(activation_key_value(&Key::Character("x".into())), None);
}

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
    let mut app = Application::new(View(Rc::new(scene)), LocalExecutor::new(|| {}));
    let size = Size::new(280.0, 160.0);
    let mut measure = Measure;
    let sync = |app: &mut Application<View>, measure: &mut Measure, point: Point| {
        let frame = Rc::new(layout(app.frame.scene(), size, measure));
        app.input.pointer_move(app.frame.scene(), &frame, point);
        app.hover_pointer = Some(PhysicalPosition::new(
            f64::from(point.x),
            f64::from(point.y),
        ));
        let events = app
            .hover_observers
            .sync(&app.input, app.frame.scene(), &frame, Some(point));
        app.set_test_layout(frame);
        events
    };
    let text = Point { x: 30.0, y: 30.0 };
    assert_eq!(sync(&mut app, &mut measure, text).len(), 2);
    assert_eq!(app.pointer_cursor(), CursorIcon::Pointer);

    app.edit_test_scene(|scene| {
        scene.children[0].children[0]
            .properties
            .insert("cursor-shape", CursorShape::Auto.into());
    });
    assert!(sync(&mut app, &mut measure, text).is_empty());
    assert_eq!(app.pointer_cursor(), CursorIcon::Crosshair);
    app.edit_test_scene(|scene| {
        scene.children[0].children[0]
            .properties
            .insert("blocking", true.into());
    });
    sync(&mut app, &mut measure, text);
    assert_eq!(app.pointer_cursor(), CursorIcon::Text);

    sync(&mut app, &mut measure, Point { x: 175.0, y: 30.0 });
    assert_eq!(app.pointer_cursor(), CursorIcon::Grab);
    let split = [0, 2];
    let handle = app
        .frame
        .scene()
        .at_path(&split)
        .unwrap()
        .children
        .iter()
        .position(|node| node.kind == "SplitHandle")
        .unwrap();
    let bounds = app
        .frame
        .layout()
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
    app.edit_test_scene(|scene| {
        scene.children.push(dialog);
    });
    let frame = Rc::new(layout(app.frame.scene(), size, &mut measure));
    app.hover_observers
        .sync(&app.input, app.frame.scene(), &frame, Some(touch));
    app.set_test_layout(frame);
    assert_eq!(app.pointer_cursor(), CursorIcon::Default);

    app.input.pointer_leave();
    app.hover_pointer = None;
    app.hover_observers.sync(
        &app.input,
        app.frame.scene(),
        app.frame.layout().unwrap(),
        None,
    );
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
    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
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
    let mut app = Application::new(View(Rc::new(scene)), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout(
        app.frame.scene(),
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
fn native_text_replacements_keep_focus_and_leave_ime_preedit_intact() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{FocusCommandMethod, FocusHandle, Length};

    struct View {
        handle: FocusHandle,
        text: String,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut input = Node::new("LineEdit", Some("editor"));
            input.focus_handle = Some(self.handle.clone());
            input.properties.insert("width", Length(180.0).into());
            input.properties.insert("height", Length(34.0).into());
            input.properties.insert("text", self.text.clone().into());
            input
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn edit_at(&mut self, _: &Node, path: &[usize], text: String) -> bool {
            assert!(path.is_empty());
            self.text = text;
            true
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let handle = FocusHandle::default();
    let mut app = Application::new(
        View {
            handle: handle.clone(),
            text: "ab".into(),
        },
        LocalExecutor::new(|| {}),
    );
    let frame = Rc::new(layout(
        app.frame.scene(),
        Size::new(200.0, 90.0),
        &mut Measure,
    ));
    app.set_test_layout(frame.clone());
    {
        let _focus = app.focus_requests.enter();
        handle.request_text_edit(
            FocusCommandMethod::AppendText
                .replacement(&[], "c".into())
                .unwrap(),
        );
        handle.request_text_edit(
            FocusCommandMethod::ReplaceText
                .replacement(&[1, 3], "界".into())
                .unwrap(),
        );
    }
    app.refresh_after_update(false);
    assert_eq!(app.component.text, "a界");
    assert!(app.input.focused.is_none());
    app.input.accessibility_focus(
        app.input
            .accessibility_target(app.frame.scene(), &frame, &[])
            .unwrap(),
    );
    app.input.sync_editor(app.frame.scene());
    app.input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("输入中".into(), None);
    {
        let _focus = app.focus_requests.enter();
        handle.request_text_edit(
            FocusCommandMethod::ClearText
                .replacement(&[], String::new())
                .unwrap(),
        );
    }
    app.apply_focus_and_text_requests();
    assert_eq!(app.component.text, "a界");
    assert_eq!(app.input.editor.as_ref().unwrap().preedit, "输入中");
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
    let mut app = Application::new(View(Rc::new(scene)), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout(
        app.frame.scene(),
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
    let (first_scene, first_layout) = cache
        .prepare(scene.clone(), size, &input, prepare)
        .into_parts();
    assert_eq!(first_layout.scroll.unwrap().offset, 0.0);
    position.set(Length(-80.0));
    assert!(cache.is_dirty());
    let (next_scene, next_layout) = cache.prepare(scene, size, &input, prepare).into_parts();
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
    let (_, original) = prepare(&mut cache, scene(0x112233ff, 40.0), false).into_parts();
    let recolored = scene(0x445566ff, 40.0);
    let paint = xrack_runtime::update_profile::measure(|| {
        let (painted, reused) = prepare(&mut cache, recolored.clone(), false).into_parts();
        assert!(Rc::ptr_eq(&painted, &recolored));
        assert!(Rc::ptr_eq(&original, &reused));
    })
    .unwrap();
    assert_eq!((paint.layout_runs, paint.layout_reuses), (0, 1));
    assert_eq!(resolves.get(), 1);
    let resize = xrack_runtime::update_profile::measure(|| {
        let (_, resized) = prepare(&mut cache, scene(0x445566ff, 80.0), false).into_parts();
        assert!(!Rc::ptr_eq(&original, &resized));
    })
    .unwrap();
    assert_eq!((resize.layout_runs, resize.layout_reuses), (1, 0));
    assert_eq!(resolves.get(), 2);

    let mut materialized = FrameCache::default();
    let (_, original) = prepare(&mut materialized, scene(0x112233ff, 40.0), true).into_parts();
    let (_, updated) = prepare(&mut materialized, recolored, true).into_parts();
    assert!(!Rc::ptr_eq(&original, &updated));
    assert_eq!(resolves.get(), 4);
}

#[test]
fn text_color_marker_preserves_layout_inputs_but_not_text_edits() {
    use xrack_runtime::Color;

    let mut original = Node::new("Text", None);
    original
        .properties
        .insert("text", "before".to_owned().into());
    original
        .properties
        .insert("color", Color(0x112233ff).into());
    original.mark_paint_only("color");

    let mut updated = original.clone();
    updated.properties.insert("color", Color(0x445566ff).into());
    assert!(original.same_frame_inputs(&updated));

    updated.properties.insert("text", "after".to_owned().into());
    assert!(!original.same_frame_inputs(&updated));
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
    let (_, first) = prepare(&mut cache).into_parts();
    assert_eq!(first.tumbler.as_ref().unwrap().rows.len(), 5);
    assert!(model.set_row(1_000, "offscreen".into()));
    let (_, untouched) = prepare(&mut cache).into_parts();
    assert!(Rc::ptr_eq(&first, &untouched));
    assert!(model.set_row(1, "visible".into()));
    let (_, updated) = prepare(&mut cache).into_parts();
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
    input.focused = Some(Target::from_node(vec![], &field, true, None));
    input.sync_editor(&field);
    input
        .editor
        .as_mut()
        .unwrap()
        .apply(field.text("text").unwrap(), EditAction::SelectAll);
    let (_, normal) = clipboard::edit_request(&input, &field, &ClipboardShortcut::Cut).unwrap();
    field
        .properties
        .insert("input-type", InputType::Password.into());
    assert!(!normal.is_current(&input, &field));
    for revealed in [false, true] {
        field.properties.insert("password-visible", revealed.into());
        assert!(clipboard::edit_request(&input, &field, &ClipboardShortcut::Copy).is_none());
        assert!(clipboard::edit_request(&input, &field, &ClipboardShortcut::Cut).is_none());
        assert!(matches!(
            clipboard::edit_request(&input, &field, &ClipboardShortcut::Paste),
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
    let mut app = Application::new(View, LocalExecutor::new(|| {}));
    let viewport = Size::new(220.0, 120.0);
    let frame = Rc::new(layout(app.frame.scene(), viewport, &mut Measure));
    app.set_test_layout(frame.clone());
    let mut projection = Projection::default();
    let update = projection
        .update(app.frame.scene(), &frame, &app.input, 2.0, None)
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
        app.frame.scene().at_path(&[0]).unwrap(),
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
fn modified_tab_runs_shortcut_and_plain_tab_moves_focus() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View(usize);

    impl Component for View {
        fn render(&self) -> Node {
            let button = |id, x| {
                let mut node = Node::new("Button", Some(id));
                node.properties.insert("x", Length(x).into());
                node.properties.insert("width", Length(70.0).into());
                node.properties.insert("height", Length(30.0).into());
                node
            };
            let mut window = Node::new("Window", None);
            let mut shortcut = Node::new("Shortcut", Some("next-tab"));
            shortcut
                .properties
                .insert("sequence", "Ctrl+Tab".to_owned().into());
            window
                .children
                .extend([button("first", 0.0), button("second", 80.0), shortcut]);
            window
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn control_at(&mut self, _: &Node, path: &[usize], action: ControlAction) -> bool {
            assert_eq!(path, [2]);
            assert_eq!(action, ControlAction::ActivateShortcut);
            self.0 += 1;
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(32.0, 16.0)
        }
    }
    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let layout = Rc::new(layout(
        app.frame.scene(),
        Size::new(230.0, 90.0),
        &mut Measure,
    ));
    app.set_test_layout(layout.clone());
    let first = app
        .input
        .accessibility_target(app.frame.scene(), &layout, &[0])
        .unwrap();
    app.input.focus_target(app.frame.scene(), &layout, first);
    let event = KeyEvent {
        logical_key: Key::Named(NamedKey::Tab),
        unmodified_key: Key::Named(NamedKey::Tab),
        text: None,
        state: ElementState::Pressed,
        repeat: false,
    };
    app.modifiers = ModifiersState::CONTROL;
    assert!(app.default_key_input(&event, false));
    assert_eq!(app.component.0, 1);
    assert_eq!(
        app.input
            .focused
            .as_ref()
            .and_then(|focus| focus.element_id()),
        Some("first")
    );
    app.modifiers = ModifiersState::empty();
    assert!(app.default_key_input(&event, false));
    assert_eq!(app.component.0, 1);
    assert_eq!(
        app.input
            .focused
            .as_ref()
            .and_then(|focus| focus.element_id()),
        Some("second")
    );
}

#[test]
fn native_edit_menu_consumes_one_escape_press_and_its_repeats_until_release() {
    use EditMenuEscapeKey::{Consume, Continue, Dismiss};

    let mut event = KeyEvent {
        logical_key: Key::Named(NamedKey::Escape),
        unmodified_key: Key::Named(NamedKey::Escape),
        text: None,
        state: ElementState::Pressed,
        repeat: false,
    };
    let mut held = false;
    assert_eq!(
        edit_menu_escape_key(&event, true, &mut held, Some(41)),
        Continue
    );
    assert!(!held);
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(41)),
        Dismiss(41)
    );
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, None),
        Consume
    );
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(42)),
        Consume
    );

    event.state = ElementState::Released;
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(42)),
        Consume
    );
    assert!(!held);
    event.state = ElementState::Pressed;
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, None),
        Continue
    );
    event.repeat = true;
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(42)),
        Consume
    );
    event.state = ElementState::Released;
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(42)),
        Consume
    );
    event.state = ElementState::Pressed;
    event.repeat = false;
    assert_eq!(
        edit_menu_escape_key(&event, false, &mut held, Some(42)),
        Dismiss(42)
    );
}

#[test]
fn accessibility_escape_respects_topmost_overlay_policy_and_restores_focus() {
    use xrack_layout::TextMeasurer;
    use xrack_runtime::{DialogRole, Length};

    #[derive(Default)]
    struct OverlayDemo {
        outer: bool,
        inner: bool,
        inner_locked: bool,
        rejected: usize,
    }

    impl Component for OverlayDemo {
        fn render(&self) -> Node {
            fn button(id: &'static str) -> Node {
                let mut button = Node::new("Button", Some(id));
                button.properties.insert("width", Length(100.0).into());
                button.properties.insert("height", Length(40.0).into());
                button
            }

            let mut window = Node::new("Window", None);
            window.children.push(button("opener"));
            for (id, open, close_on_escape) in [
                ("outer", self.outer, true),
                ("inner", self.inner, !self.inner_locked),
            ] {
                let mut dialog = Node::new("Dialog", Some(id));
                dialog.properties.insert("open", open.into());
                dialog.properties.insert("width", Length(220.0).into());
                dialog.properties.insert("height", Length(130.0).into());
                dialog
                    .properties
                    .insert("close-on-escape", close_on_escape.into());
                dialog.children.push(button("confirm"));
                window.children.push(dialog);
            }
            window
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn control_at(&mut self, _: &Node, path: &[usize], action: ControlAction) -> bool {
            assert_eq!(action, ControlAction::Dialog(DialogRole::Reject));
            match path {
                [1] => self.outer = false,
                [2] => self.inner = false,
                _ => panic!("Unexpected overlay path: {path:?}"),
            }
            self.rejected += 1;
            true
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(30.0, 16.0)
        }
    }

    let mut app = Application::new(OverlayDemo::default(), LocalExecutor::new(|| {}));
    let prepare = |app: &mut Application<OverlayDemo>| {
        app.set_test_scene(Rc::new(app.component.render()));
        let layout = Rc::new(xrack_layout::layout(
            app.frame.scene(),
            Size::new(320.0, 250.0),
            &mut Measure,
        ));
        app.input.reconcile(app.frame.scene(), &layout);
        app.set_test_layout(layout);
    };
    prepare(&mut app);
    app.input.focus_next(
        app.frame.scene(),
        app.frame.layout().map(Rc::as_ref).unwrap(),
        false,
    );
    assert!(app.input.is_focused(&[0]));
    assert!(app.accessibility_escape().is_none());

    app.component.outer = true;
    prepare(&mut app);
    assert!(app.input.is_focused(&[1, 0]));
    let outer = app.accessibility_escape().unwrap();
    let menu = app.accessibility_escape_with_native_menu(Some(41)).unwrap();
    assert_eq!(menu, AccessibilityEscape::NativeTextMenu(41));
    assert_eq!(
        app.accessibility_escape_with_native_menu(Some(42)),
        Some(AccessibilityEscape::NativeTextMenu(42))
    );
    app.perform_accessibility_escape(menu);
    assert_eq!(
        app.component.rejected, 0,
        "a stale native request cannot close the dialog"
    );
    assert_eq!(app.accessibility_escape(), Some(outer.clone()));
    app.component.inner = true;
    app.component.inner_locked = true;
    prepare(&mut app);
    assert!(app.input.is_focused(&[2, 0]));
    assert!(app.accessibility_escape().is_none());
    app.perform_accessibility_escape(outer.clone());
    assert_eq!(app.component.rejected, 0);

    app.component.inner_locked = false;
    prepare(&mut app);
    let inner = app.accessibility_escape().unwrap();
    app.perform_accessibility_escape(inner.clone());
    assert!(app.component.outer && !app.component.inner);
    assert_eq!(app.component.rejected, 1);
    prepare(&mut app);
    assert!(app.input.is_focused(&[1, 0]));
    app.perform_accessibility_escape(inner);
    assert_eq!(app.component.rejected, 1);

    app.perform_accessibility_escape(outer);
    assert_eq!(app.component.rejected, 2);
    prepare(&mut app);
    assert!(app.input.is_focused(&[0]));
    assert!(app.accessibility_escape().is_none());
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
        rtl: bool,
        edits: usize,
    }

    impl Component for View {
        fn render(&self) -> Node {
            let mut window = Node::new("Window", None);
            if self.rtl {
                window.properties.insert(
                    "layout-direction",
                    xrack_runtime::LayoutDirection::RightToLeft.into(),
                );
            }
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

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let frame = |app: &mut Application<View>| {
        app.set_test_scene(Rc::new(app.component.render()));
        let layout = Rc::new(layout(
            app.frame.scene(),
            Size::new(320.0, 140.0),
            &mut Measure,
        ));
        app.set_test_layout(layout.clone());
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
        .update(app.frame.scene(), &layout, &app.input, 2.0, None)
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

    app.component.rtl = true;
    let layout = frame(&mut app);
    let mirrored = projection
        .update(app.frame.scene(), &layout, &app.input, 2.0, None)
        .unwrap();
    let (_, mirror_handle) = mirrored.nodes.iter().find(|(id, _)| *id == first).unwrap();
    assert_eq!(mirror_handle.numeric_value(), Some(108.0));
    assert_eq!(mirror_handle.controls(), &[navigation]);
    assert!(mirror_handle.bounds().unwrap().x0 > splitter.bounds().unwrap().x0);
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
        .update(app.frame.scene(), &layout, &app.input, 2.0, None)
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
        .update(app.frame.scene(), &layout, &app.input, 2.0, None)
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
    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let viewport = Size::new(220.0, 120.0);
    let frame = Rc::new(layout(app.frame.scene(), viewport, &mut Measure));
    app.set_test_layout(frame.clone());
    let mut projection = Projection::default();
    let update = projection
        .update(app.frame.scene(), &frame, &app.input, 1.0, None)
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

    app.edit_test_scene(|scene| {
        scene.children[0].id = Some("recycled");
    });
    app.set_test_layout(Rc::new(layout(app.frame.scene(), viewport, &mut Measure)));
    app.accessibility_action(route, request(click, AccessibleAction::Click, None));
    assert_eq!(app.component.clicks, 1);
}

#[test]
fn touch_text_hold_uses_the_event_loop_deadline_and_yields_to_queued_motion() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    struct Editor;
    impl Component for Editor {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            let mut editor = Node::new("TextEdit", Some("entry"));
            editor.properties.insert("width", Length(140.0).into());
            editor.properties.insert("height", Length(80.0).into());
            root.children.push(editor);
            root
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

    let mut app = Application::new(Editor, LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout(
        app.frame.scene(),
        Size::new(220.0, 120.0),
        &mut Measure,
    )));
    let origin = Point { x: 20.0, y: 20.0 };
    let device = DeviceId::dummy();
    let now = Instant::now();
    app.update_touch(device, 1, TouchPhase::Started, origin, now);
    let deadline = app.touch_hold.as_ref().unwrap().deadline.unwrap();
    app.tick(deadline - std::time::Duration::from_millis(1));
    assert!(app.touch_hold.is_some());
    app.tick(deadline);
    assert!(app.touch_hold.is_none());

    app.update_touch(device, 2, TouchPhase::Started, origin, deadline);
    assert!(app.touch_hold.is_some());
    app.update_touch(
        device,
        2,
        TouchPhase::Moved,
        Point { x: 20.0, y: 38.0 },
        deadline + std::time::Duration::from_millis(600),
    );
    assert!(app.touch_hold.is_none());
    assert!(app.scroll_contact.is_some());
}

#[test]
fn touch_context_menu_owns_the_hold_and_scrolls_its_own_rows() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View {
        taps: usize,
        selected: Option<String>,
    }
    impl Component for View {
        fn render(&self) -> Node {
            let mut area = Node::new("ContextMenuArea", Some("area"));
            let mut button = Node::new("Button", Some("row"));
            button.properties.insert("width", Length(180.0).into());
            button.properties.insert("height", Length(164.0).into());
            area.children.push(button);

            let mut menu = Node::new("Menu", Some("menu"));
            for index in 0..8 {
                let mut item = Node::new("MenuItem", None);
                item.properties
                    .insert("text", format!("Option {index}").into());
                item.properties
                    .insert("enabled", (!(2..=4).contains(&index)).into());
                menu.children.push(item);
            }
            area.children.push(menu);
            area
        }

        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            let tapped = (id, event) == ("row", "clicked");
            self.taps += usize::from(tapped);
            tapped
        }

        fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
            assert_eq!(action, ControlAction::ActivateMenuItem);
            self.selected = scene
                .at_path(path)
                .and_then(|node| node.text("text"))
                .map(str::to_owned);
            self.selected.is_some()
        }
    }
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let size = Size::new(220.0, 170.0);
    app.set_test_layout(Rc::new(layout(app.frame.scene(), size, &mut Measure)));
    let device = DeviceId::dummy();
    let origin = Point { x: 20.0, y: 32.0 };
    let before_hold = Duration::from_millis(499);
    let hold_time = Duration::from_millis(500);
    let now = Instant::now();

    app.update_touch(device, 1, TouchPhase::Started, origin, now);
    app.tick(now + before_hold);
    assert!(!app.input.menu_open());
    app.update_touch(device, 1, TouchPhase::Ended, origin, now + before_hold);
    assert_eq!(app.component.taps, 1);

    let now = now + hold_time;
    app.update_touch(device, 2, TouchPhase::Started, origin, now);
    let moved = Point {
        y: origin.y + 20.0,
        ..origin
    };
    app.update_touch(device, 2, TouchPhase::Moved, moved, now + hold_time);
    app.tick(now + hold_time);
    assert!(!app.input.menu_open());
    app.update_touch(device, 2, TouchPhase::Ended, origin, now + hold_time);
    assert_eq!(app.component.taps, 1);

    let now = now + hold_time;
    app.update_touch(device, 3, TouchPhase::Started, origin, now);
    app.edit_test_scene(|scene| {
        scene.id = Some("replacement");
    });
    app.tick(now + hold_time);
    assert!(!app.input.menu_open());
    app.update_touch(device, 3, TouchPhase::Cancelled, origin, now + hold_time);

    let now = now + hold_time;
    app.update_touch(device, 4, TouchPhase::Started, origin, now);
    app.tick(now + hold_time);
    assert!(app.input.menu_open());
    app.update_touch(device, 4, TouchPhase::Ended, origin, now + hold_time);
    assert_eq!(app.component.taps, 1);
    assert!(app.component.selected.is_none());

    let menu_top = |app: &Application<View>| {
        let panels = app
            .input
            .menu_panels(app.frame.scene(), app.frame.layout().unwrap());
        let panel = &panels[0];
        (panel.bounds, panel.rows[0].label.to_owned())
    };
    let (_, first) = menu_top(&app);
    assert_eq!(first, "Option 0");
    let mut now = now + hold_time;
    for (id, expected) in [(5, "Option 2"), (6, "Option 4")] {
        let (bounds, _) = menu_top(&app);
        let pan = Point {
            x: bounds.x + 12.0,
            y: bounds.y + bounds.height - 12.0,
        };
        let end = Point {
            y: pan.y - 100.0,
            ..pan
        };
        app.update_touch(device, id, TouchPhase::Started, pan, now);
        app.update_touch(
            device,
            id,
            TouchPhase::Moved,
            end,
            now + Duration::from_millis(20),
        );
        app.update_touch(
            device,
            id,
            TouchPhase::Ended,
            end,
            now + Duration::from_millis(30),
        );
        now += Duration::from_millis(40);
        app.tick(now);
        assert_eq!(menu_top(&app).1, expected);
        assert!(app.component.selected.is_none());
    }

    let (bounds, _) = menu_top(&app);
    let item = Point {
        x: bounds.x + 12.0,
        y: bounds.y + 58.0,
    };
    app.update_touch(device, 7, TouchPhase::Started, item, now);
    app.update_touch(
        device,
        7,
        TouchPhase::Ended,
        item,
        now + Duration::from_millis(20),
    );
    assert!(!app.input.menu_open());
    assert_eq!(app.component.selected.as_deref(), Some("Option 5"));
    assert_eq!(app.component.taps, 1);
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

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let layout = Rc::new(layout(
        app.frame.scene(),
        Size::new(180.0, 150.0),
        &mut Measure,
    ));
    let geometry = ColorPlaneGeometry::new(layout.at_path(&[0]).unwrap().bounds);
    let slider = SliderGeometry::new(
        layout.at_path(&[1]).unwrap().bounds,
        Orientation::Horizontal,
    );
    app.set_test_layout(layout);
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

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let layout = Rc::new(layout(
        app.frame.scene(),
        Size::new(180.0, 100.0),
        &mut Measure,
    ));
    let handle = layout.at_path(&[0, 2]).unwrap().bounds;
    app.set_test_layout(layout);
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
    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    let viewport = Size::new(180.0, 120.0);
    app.set_test_layout(Rc::new(layout_with_input(
        app.frame.scene(),
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
fn native_buttons_pair_mouse_and_touch_separately_after_both_clicks() {
    use xrack_layout::{TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    #[derive(Default)]
    struct View {
        events: Vec<String>,
    }

    impl Component for View {
        fn render(&self) -> Node {
            let mut root = Node::new("Window", None);
            let mut button = Node::new("Button", Some("button"));
            button.properties.insert("width", Length(120.0).into());
            button.properties.insert("height", Length(72.0).into());
            root.children.push(button);
            root
        }

        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            if id == "button" && matches!(event, "clicked" | "double-clicked") {
                self.events.push(event.to_owned());
                true
            } else {
                false
            }
        }
    }

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::new(12.0, 16.0)
        }
    }

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout_with_input(
        app.frame.scene(),
        Size::new(180.0, 120.0),
        &mut Measure,
        &app.input,
    )));
    let point = Point { x: 30.0, y: 40.0 };
    let start = Instant::now();
    let mouse = |app: &mut Application<View>, time| {
        app.input
            .pointer_move(app.frame.scene(), app.frame.layout().unwrap(), point);
        app.left_pointer(ElementState::Pressed, None);
        app.left_pointer(
            ElementState::Released,
            Some(PointerClick {
                touch_device: None,
                point,
                time,
            }),
        );
    };
    let touch = |app: &mut Application<View>, id, time| {
        let device = DeviceId::dummy();
        app.update_touch(device, id, TouchPhase::Started, point, time);
        app.update_touch(
            device,
            id,
            TouchPhase::Ended,
            point,
            time + Duration::from_millis(10),
        );
    };
    mouse(&mut app, start);
    mouse(&mut app, start + Duration::from_millis(100));
    assert_eq!(
        app.component.events,
        ["clicked", "clicked", "double-clicked"]
    );

    app.component.events.clear();
    touch(&mut app, 1, start + Duration::from_millis(200));
    mouse(&mut app, start + Duration::from_millis(250));
    touch(&mut app, 2, start + Duration::from_millis(300));
    touch(&mut app, 3, start + Duration::from_millis(380));
    assert_eq!(
        app.component.events,
        ["clicked", "clicked", "clicked", "clicked", "double-clicked"]
    );
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

    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout_with_input(
        app.frame.scene(),
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
        .pointer_move(app.frame.scene(), app.frame.layout().unwrap(), start);
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
        app.edit_test_scene(|scene| {
            scene.children[0]
                .properties
                .insert("enabled", enabled.into());
        });
        let layout = Rc::new(layout_with_input(
            app.frame.scene(),
            Size::new(180.0, 120.0),
            &mut Measure,
            &app.input,
        ));
        app.input.reconcile(app.frame.scene(), &layout);
        app.set_test_layout(layout);
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
            .pointer_move(app.frame.scene(), app.frame.layout().unwrap(), point);
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
    );
    let viewport = Size::new(180.0, 140.0);
    app.set_test_layout(Rc::new(layout_with_input(
        app.frame.scene(),
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
    app.edit_test_scene(|scene| {
        scene.children[0].properties.insert("enabled", false.into());
    });
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
    );
    app.set_test_layout(Rc::new(layout(
        app.frame.scene(),
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
    app.edit_test_scene(|scene| {
        scene.properties.insert("multi-touch", false.into());
    });
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
    );
    let viewport = Size::new(280.0, 220.0);
    app.set_test_layout(Rc::new(layout(app.frame.scene(), viewport, &mut Measure)));
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
        .pointer_move(app.frame.scene(), app.frame.layout().unwrap(), first);
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
    app.edit_test_scene(|scene| {
        scene.children[0].properties.insert("enabled", false.into());
    });
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
    let mut app = Application::new(View(scene), LocalExecutor::new(|| {}));
    let frame = Rc::new(layout(
        app.frame.scene(),
        Size::new(250.0, 100.0),
        &mut Measure,
    ));
    app.set_test_layout(frame.clone());
    let target = app
        .input
        .accessibility_target(app.frame.scene(), &frame, &[0])
        .unwrap();
    app.input.focus_target(app.frame.scene(), &frame, target);
    app.input
        .edit_spin(&app.frame.scene().children[0], EditAction::ReplaceAll("37"));
    assert_eq!(app.input.editor_value(&app.frame.scene().children[0]), "37");
    app.input
        .pointer_move(app.frame.scene(), &frame, Point { x: 158.0, y: 18.0 });
    let event = app.wheel_event(Point { x: 0.0, y: -28.0 }, Some(Point { x: 0.0, y: -1.0 }));
    assert!(app.adjust_wheel(event, Instant::now()));
    assert_eq!(app.component.0.children[1].int("value"), Some(11));
    assert_eq!(app.input.editor_value(&app.frame.scene().children[0]), "37");

    app.input
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("3".into(), None);
    app.input
        .pointer_move(app.frame.scene(), &frame, Point { x: 40.0, y: 18.0 });
    assert!(!app.adjust_wheel(event, Instant::now()));
    assert_eq!(app.component.0.children[0].int("value"), Some(10));
    assert_eq!(app.input.editor_value(&app.frame.scene().children[0]), "37");
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
    let mut app = Application::new(View(window), LocalExecutor::new(|| {}));
    let viewport = Size::new(160.0, 120.0);
    let now = Instant::now();
    let device = DeviceId::dummy();
    for (id, fraction, phase, expected) in [
        (1, 0.44, TouchPhase::Ended, 200.0),
        (2, 0.31, TouchPhase::Cancelled, 124.0),
    ] {
        let layout = layout_with_input(app.frame.scene(), viewport, &mut Measure, &app.input);
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
        app.set_test_layout(Rc::new(layout));
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
    );
    let frame = layout(app.frame.scene(), Size::new(200.0, 130.0), &mut Measure);
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
    app.set_test_layout(Rc::new(frame));
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
            .accessibility_target(app.frame.scene(), app.frame.layout().unwrap(), &[0, 0])
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
    let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout(
        app.frame.scene(),
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
fn replacing_text_stops_an_active_touch_selection_scroll() {
    struct Notes(&'static str);
    impl Component for Notes {
        fn render(&self) -> Node {
            let mut node = Node::new("TextArea", Some("notes"));
            node.properties.insert("text", self.0.to_owned().into());
            node
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
    }

    for dragging_handle in [false, true] {
        let mut app = Application::new(Notes("original notes"), LocalExecutor::new(|| {}));
        let target = Target::from_node(vec![], app.frame.scene(), true, None);
        app.input.focused = Some(target.clone());
        app.input.sync_editor(app.frame.scene());
        app.input.set_touch_selection_handles(true);
        let contact = PointerContact::Touch(DeviceId::dummy(), 1);
        let origin = Point::default();
        let value = app.component.0.to_owned();
        if dragging_handle {
            app.selection_handle_drag = Some(SelectionHandleDrag {
                contact,
                target: target.clone(),
                origin,
                finger_offset: origin,
                value,
            });
        } else {
            app.touch_hold = Some(TouchHold {
                contact,
                origin,
                deadline: None,
                kind: TouchHoldKind::SelectingText {
                    target: target.clone(),
                    value,
                },
            });
        }
        let now = Instant::now();
        app.input.schedule_text_selection_scroll(
            &target,
            Some(Point { x: 20.0, y: 70.0 }),
            Some(origin),
            now,
        );
        assert!(app.input.text_selection_scroll_deadline().is_some());
        app.refresh_deadline(now);
        assert!(app.input.text_selection_scroll_deadline().is_some());

        app.component.0 = "replacement notes";
        app.set_test_scene(Rc::new(app.component.render()));
        app.refresh_deadline(now);
        assert_eq!(app.input.focused.as_ref(), Some(&target));
        assert!(app.input.text_selection_scroll_deadline().is_none());
    }
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
    let mut app = Application::new(Editor("初始".into()), LocalExecutor::new(|| {}));
    app.focus_active = true;
    app.input.focused = Some(Target::from_node(vec![], app.frame.scene(), true, None));
    app.input.sync_editor(app.frame.scene());
    app.input
        .editor
        .as_mut()
        .unwrap()
        .apply_multiline("初始", EditAction::SelectAll);

    let context = xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap();
    assert_eq!(context.selection(), "初始");
    app.pending_clipboard = Some(PendingClipboard::new(1, ClipboardContext::Cut(context)));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 1,
        result: Err("test write failure".into()),
    }));
    assert_eq!(app.component.0, "初始");

    app.pending_clipboard = Some(PendingClipboard::new(
        5,
        ClipboardContext::Paste(
            xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap(),
        ),
    ));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 5,
        result: Ok(ClipboardValue::Text(String::new())),
    }));
    assert_eq!(app.component.0, "初始");

    app.pending_clipboard = Some(PendingClipboard::new(
        2,
        ClipboardContext::Cut(
            xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap(),
        ),
    ));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 2,
        result: Ok(ClipboardValue::Written),
    }));
    assert_eq!(app.base_scene.text("text"), Some(""));
    app.set_test_scene(app.base_scene.clone());
    app.pending_clipboard = Some(PendingClipboard::new(
        3,
        ClipboardContext::Paste(
            xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap(),
        ),
    ));
    app.accept_clipboard_reply(
        ClipboardReply {
            id: 3,
            result: Ok(ClipboardValue::Text("一\r\n二".into())),
        },
        false,
    );
    assert_eq!(app.base_scene.text("text"), Some(""));
    app.finish_deferred_paste();
    assert_eq!(app.base_scene.text("text"), Some("一\n二"));
    assert!(app.pending_clipboard.is_none());
    app.set_test_scene(app.base_scene.clone());
    app.pending_clipboard = Some(PendingClipboard::new(
        4,
        ClipboardContext::Paste(
            xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap(),
        ),
    ));
    app.accept_clipboard_reply(
        ClipboardReply {
            id: 4,
            result: Ok(ClipboardValue::Text("过期粘贴".into())),
        },
        false,
    );
    app.component.0 = "后台更新".into();
    app.set_test_scene(app.component.render_shared());
    app.finish_deferred_paste();
    assert_eq!(app.component.0, "后台更新");
    assert!(app.pending_clipboard.is_none());

    app.pending_clipboard = Some(PendingClipboard::new(
        6,
        ClipboardContext::Paste(
            xrack_layout::TextEditContext::capture(&app.input, app.frame.scene()).unwrap(),
        ),
    ));
    app.accept_clipboard_reply(
        ClipboardReply {
            id: 6,
            result: Ok(ClipboardValue::Text("不跨后台的粘贴".into())),
        },
        false,
    );
    app.suspend();
    app.focus_active = true;
    app.finish_deferred_paste();
    assert_eq!(app.component.0, "后台更新");
    assert!(app.pending_clipboard.is_none());
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

    let mut app = Application::new(Editor::default(), LocalExecutor::new(|| {}));
    assert!(!app.request_close());
    assert_eq!(app.base_scene.int("requests"), Some(1));
    assert!(!app.take_close_request());
    app.component.can_close = true;
    assert!(app.request_close());
    assert_eq!(app.component.requests, 2);
    assert!(!app.take_close_request());
}

#[test]
fn closing_one_window_cancels_its_tasks_without_affecting_another() {
    let make = |delta| {
        let (notify, events) = mpsc::channel();
        let tasks = LocalExecutor::new(move || {
            let _ = notify.send(UserEvent::PollTasks);
        });
        let window = Application::new(TestUi::default(), tasks.clone());
        let value = window.component.value.clone();
        {
            let _scope = tasks.enter();
            spawn_local(async move { value.set(value.get() + delta) }).unwrap();
        }
        (window, events)
    };
    let (mut first, first_events) = make(10);
    let (mut second, second_events) = make(20);
    first.close();
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
fn tokio_result_wakes_local_ui_task() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let (notify, events) = mpsc::channel();
    let executor = LocalExecutor::new(move || notify.send(UserEvent::PollTasks).unwrap());
    let _scope = executor.enter();
    let mut app = Application::new(TestUi::default(), executor);
    let (start, wait) = tokio::sync::oneshot::channel();
    let job = runtime.spawn(async move {
        wait.await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        42
    });
    let value = app.component.value.clone();
    let ui_thread = thread::current().id();
    spawn_local(async move {
        let delta = job.await.unwrap();
        assert_eq!(thread::current().id(), ui_thread);
        value.set(value.get() + delta);
    })
    .unwrap();

    app.process_user_event(events.recv_timeout(Duration::from_secs(5)).unwrap());
    assert_eq!(app.base_scene.int("value"), Some(0));
    start.send(()).unwrap();
    app.process_user_event(events.recv_timeout(Duration::from_secs(5)).unwrap());
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
    let (before, initial_layout) = render().into_parts();
    assert_eq!(before.children.len(), 2);
    assert!(ui.model.set_row(3, "offscreen".into()));
    let (unchanged, layout) = render().into_parts();
    assert!(Rc::ptr_eq(&before, &unchanged));
    assert!(Rc::ptr_eq(&initial_layout, &layout));
    assert!(ui.model.set_row(1, "visible".into()));
    let (updated, layout) = render().into_parts();
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
    let mut app = Application::new(TestUi::default(), LocalExecutor::new(|| {}));
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
        app.set_test_scene(Rc::new(window));
        app.set_test_layout(Rc::new(layout(
            app.frame.scene(),
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
            .into_parts()
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
fn reduced_motion_finishes_running_transitions_and_restores_future_animation() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{Animation, Easing};

    struct View(Rc<Node>);
    impl Component for View {
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
    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let make_scene = |index: i64, progress: f64| {
        let mut scene = Node::new("Window", None);
        let mut badge = Node::new("ProgressIndicator", Some("progress"));
        badge.properties.insert("progress", progress.into());
        badge.set_animation("progress", Animation::new(800, 0, Easing::LINEAR));
        let mut pages = Node::new("SwipeView", Some("pages"));
        pages.properties.insert("current-index", index.into());
        pages
            .properties
            .insert("transition-duration", 800_i64.into());
        pages.children.extend([
            Node::new("SwipePage", Some("first")),
            Node::new("SwipePage", Some("second")),
        ]);
        scene.children.extend([badge, pages]);
        Rc::new(scene)
    };
    let size = Size::new(240.0, 180.0);
    let initial = make_scene(0, 0.0);
    let mut app = Application::new(View(initial.clone()), LocalExecutor::new(|| {}));
    app.set_test_layout(Rc::new(layout(&initial, size, &mut Measure)));
    let update = |app: &mut Application<View>, index, progress| {
        let scene = make_scene(index, progress);
        let next_layout = Rc::new(layout(&scene, size, &mut Measure));
        app.component.0 = scene.clone();
        assert!(app.sync_base_scene());
        app.update_page_transitions(&scene, &next_layout, Instant::now());
        app.set_test_scene(scene);
        app.set_test_layout(next_layout);
    };

    update(&mut app, 1, 0.5);
    assert!(app.transition.running(app.transition_epoch.elapsed()));
    assert!(app.stack_transitions.is_active());
    let layout = app.frame.layout().unwrap();
    let pages = layout.at_path(&[1]).unwrap().bounds;
    app.input.pointer_move(
        app.frame.scene(),
        layout,
        Point {
            x: pages.x + 4.0,
            y: pages.y + 4.0,
        },
    );
    assert!(app.input.pointer_on_stack_slide());
    app.animation_wake = Some(Instant::now());

    assert!(app.set_reduce_motion(true));
    assert!(!app.input.pointer_on_stack_slide());
    assert!(app.animation_wake.is_none());
    assert!(!app.transition.running(app.transition_epoch.elapsed()));
    assert!(!app.stack_transitions.is_active());
    let frame = app
        .transition
        .frame(&app.base_scene, app.transition_epoch.elapsed());
    assert_eq!(frame.children[0].float("progress"), Some(0.5));

    update(&mut app, 0, 1.0);
    assert!(!app.transition.running(app.transition_epoch.elapsed()));
    assert!(!app.stack_transitions.is_active());
    assert!(!app.set_reduce_motion(true));

    assert!(app.set_reduce_motion(false));
    assert!(!app.transition.running(app.transition_epoch.elapsed()));
    assert!(!app.stack_transitions.is_active());
    update(&mut app, 1, 0.25);
    assert!(app.transition.running(app.transition_epoch.elapsed()));
    assert!(app.stack_transitions.is_active());
}

#[test]
fn reduced_motion_preserves_direct_page_drag_and_skips_settling() {
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::SwipePreviewHandle;

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }
    let mut scene = Node::new("SwipeView", None);
    scene.properties.insert("current-index", 0_i64.into());
    scene
        .properties
        .insert("transition-duration", 300_i64.into());
    scene.enable_swipe_preview(SwipePreviewHandle::default());
    scene.children.extend([
        Node::new("SwipePage", Some("first")),
        Node::new("SwipePage", Some("second")),
    ]);
    let layout = layout(&scene, Size::new(240.0, 180.0), &mut Measure);
    let start = Point { x: 160.0, y: 90.0 };
    let stopped = Point { x: 143.0, ..start };
    let swiped = Point { x: 64.0, ..start };
    let mut input = Interaction::default();
    let mut now = Instant::now();
    let pause = Duration::from_millis(300);
    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &layout, stopped, now + pause));
    assert!(input.swipe_page_motion(&[]).is_some());
    now += 2 * pause;
    assert!(
        input
            .end_scroll_gesture(&scene, &layout, stopped, now)
            .control
            .is_none()
    );
    assert!(input.swipe_page_motion(&[]).is_some());
    input.set_reduce_motion(true);
    assert!(input.swipe_page_motion(&[]).is_none());

    assert!(input.begin_scroll_gesture(&scene, &layout, start, now, ScrollGestureKind::Touch));
    assert!(input.move_scroll_gesture(&scene, &layout, swiped, now + pause));
    assert!(input.swipe_page_motion(&[]).is_some());
    let end = input.end_scroll_gesture(&scene, &layout, swiped, now + 2 * pause);
    assert_eq!(end.control.unwrap().action, ControlAction::Select(1));
    assert!(end.scrolled);
    assert!(input.swipe_page_motion(&[]).is_none());
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
            .into_parts()
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
fn range_preview_updates_cached_thumb_geometry_and_cancellation_restores_it() {
    use xrack_layout::{SliderGeometry, TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }

    let mut node = Node::new("RangeSlider", None);
    node.properties.insert("live", false.into());
    node.properties.insert("lower-value", 20.0_f64.into());
    node.properties.insert("upper-value", 80.0_f64.into());
    node.add_range_thumbs();
    let mut art = Node::new("RangeLowerHandle", None);
    art.properties.insert("width", Length(40.0).into());
    node.children.push(art);
    let scene = Rc::new(node);
    let mut input = Interaction::default();
    let mut cache = FrameCache::default();
    let viewport = Size::new(220.0, 40.0);
    let prepare = |cache: &mut FrameCache, input: &Interaction| {
        cache
            .prepare(scene.clone(), viewport, input, |scene| {
                let layout = layout_with_input(&scene, viewport, &mut Measure, input);
                (scene, layout)
            })
            .into_parts()
            .1
    };
    let before = prepare(&mut cache, &input);
    let track = SliderGeometry::for_node(before.bounds, &scene);
    input.pointer_move(&scene, &before, track.position(0.2));
    input.pointer_press_in_scene(&scene, &before);
    assert!(input.slider_press(&scene, &before).is_none());
    input.pointer_move(&scene, &before, track.position(0.5));
    assert!(input.slider_drag(&scene, &before).is_none());
    assert!(cache.slider_preview_changed(&input));
    let preview = prepare(&mut cache, &input);
    assert!(preview.children[0].bounds.x > before.children[0].bounds.x);
    assert_eq!(preview.children[2].bounds, preview.children[0].bounds);
    assert!(!cache.slider_preview_changed(&input));

    assert!(input.cancel_slider_preview());
    assert!(cache.slider_preview_changed(&input));
    let restored = prepare(&mut cache, &input);
    assert_eq!(restored.children[0].bounds, before.children[0].bounds);
    assert_eq!(restored.children[2].bounds, before.children[2].bounds);
}

#[test]
fn switch_drag_updates_cached_thumb_geometry_until_canceled() {
    use xrack_layout::{TextMeasurer, ToggleGeometry, layout_with_input};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }

    let mut node = Node::new("Switch", None);
    node.properties.insert("checked", true.into());
    let scene = Rc::new(node);
    let viewport = Size::new(160.0, 44.0);
    let mut cache = FrameCache::default();
    let mut prepare = |input: &Interaction| {
        cache
            .prepare(scene.clone(), viewport, input, |scene| {
                let layout = layout_with_input(&scene, viewport, &mut Measure, input);
                (scene, layout)
            })
            .into_parts()
            .1
    };
    let mut input = Interaction::default();
    let before = prepare(&input);
    let geometry = ToggleGeometry::new(&scene, before.bounds, before.layout_direction);
    let thumb = geometry.switch_thumb(true, None);
    let start = Point {
        x: thumb.x + thumb.width / 2.0,
        y: thumb.y + thumb.height / 2.0,
    };
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(&scene, &before, start, now, ScrollGestureKind::Touch));
    for (distance, expected) in [(9.0, 0.5), (18.0, 0.0)] {
        let point = Point {
            x: start.x - distance,
            ..start
        };
        assert!(input.move_scroll_gesture(&scene, &before, point, now + Duration::from_millis(40)));
        let preview = prepare(&input);
        assert_eq!(preview.switch_position, Some(expected));
        assert!(Rc::ptr_eq(&preview, &prepare(&input)));
    }
    input.cancel_scroll_gesture();
    assert_eq!(prepare(&input).switch_position, None);
    assert!(scene.boolean("checked", false));
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
            .into_parts()
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
    let mut app = Application::new(TestUi::default(), executor);
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
    );
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let place = |app: &mut Application<View>| {
        app.set_test_scene(app.component.render_shared());
        app.set_test_layout(Rc::new(layout(
            app.frame.scene(),
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
    assert!(app.frame.scene().same_frame_inputs(&app.base_scene));

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

#[test]
fn unconsumed_keys_continue_after_a_native_suggestion_region() {
    struct Editor(String);
    impl Component for Editor {
        fn render(&self) -> Node {
            let mut input = Node::new("LineEdit", Some("editor"));
            input.properties.insert("text", self.0.clone().into());
            input
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn edit_at(&mut self, _: &Node, _: &[usize], text: String) -> bool {
            self.0 = text;
            true
        }
    }

    for (value, preedit, cursor, expected) in [("O", "O", 1, "O!"), ("旧🚰", "新词", 3, "新!词")]
    {
        let mut app = Application::new(Editor(value.into()), LocalExecutor::new(|| {}));
        app.input.focused = Some(Target::from_node(Vec::new(), app.frame.scene(), true, None));
        app.input.sync_editor(app.frame.scene());
        let editor = app.input.editor.as_mut().unwrap();
        editor.set_selection(
            value,
            0,
            xrack_layout::TextPosition {
                offset: value.len(),
                trailing: false,
            },
        );
        editor.set_preedit(preedit.into(), Some((cursor, cursor)));
        app.commit_text_preedit();
        assert!(app.input.editor.as_ref().unwrap().preedit.is_empty());
        assert_eq!(app.component.0, preedit);
        // Publish the updated model without creating a native renderer in this test.
        app.set_test_scene(app.component.render_shared());
        app.base_scene = app.frame.scene().clone();
        app.edit_text(EditAction::Insert("!"));
        assert_eq!(app.component.0, expected);
    }
}

#[test]
fn selecting_committed_ime_text_preserves_real_composition_and_avoids_model_writes() {
    struct Editor {
        value: String,
        writes: usize,
    }
    impl Component for Editor {
        fn render(&self) -> Node {
            let mut node = Node::new("LineEdit", Some("editor"));
            node.properties.insert("text", self.value.clone().into());
            node
        }
        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }
        fn edit_at(&mut self, _: &Node, _: &[usize], text: String) -> bool {
            self.writes += 1;
            self.value = text;
            true
        }
    }
    for (value, preedit, finished) in [
        ("#4080C0FF", "#4080C0FF", true),
        ("你好🚰", "你好🚰", true),
        ("旧", "新词", false),
    ] {
        let mut app = Application::new(
            Editor {
                value: value.into(),
                writes: 0,
            },
            LocalExecutor::new(|| {}),
        );
        app.input.focused = Some(Target::from_node(Vec::new(), app.frame.scene(), true, None));
        app.input.sync_editor(app.frame.scene());
        let editor = app.input.editor.as_mut().unwrap();
        editor.set_selection(
            value,
            0,
            xrack_layout::TextPosition {
                offset: value.len(),
                trailing: false,
            },
        );
        editor.set_preedit(preedit.into(), Some((preedit.len(), preedit.len())));
        assert_eq!(app.finish_committed_suggestion(), finished);
        assert_eq!(app.component.value, value);
        assert_eq!(app.component.writes, 0);
        assert_eq!(
            app.input.editor.as_ref().unwrap().preedit.is_empty(),
            finished
        );
    }
}
