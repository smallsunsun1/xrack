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
fn accessibility_dispatches_clicks_and_edits_and_rejects_a_stale_path() {
    use accesskit::NodeId;
    use xrack_layout::{TextMeasurer, layout};
    use xrack_runtime::{InputType, Length};

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
            root.children = vec![button, editor];
            root
        }
        fn dispatch(&mut self, id: &str, event: &str) -> bool {
            assert_eq!((id, event), ("save", "clicked"));
            self.clicks += 1;
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
    app.pending_clipboard = Some((1, context));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 1,
        result: Err("test write failure".into()),
    }));
    assert_eq!(app.component.0, "初始");

    app.pending_clipboard = Some((2, EditContext::capture(&app.input, &app.scene).unwrap()));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 2,
        result: Ok(ClipboardValue::Written),
    }));
    assert_eq!(app.base_scene.text("text"), Some(""));
    app.scene = app.base_scene.clone();
    app.pending_clipboard = Some((3, EditContext::capture(&app.input, &app.scene).unwrap()));
    app.process_user_event(UserEvent::Clipboard(ClipboardReply {
        id: 3,
        result: Ok(ClipboardValue::Text("一\r\n二".into())),
    }));
    assert_eq!(app.base_scene.text("text"), Some("一\n二"));
    app.scene = app.base_scene.clone();
    app.pending_clipboard = Some((4, EditContext::capture(&app.input, &app.scene).unwrap()));
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
