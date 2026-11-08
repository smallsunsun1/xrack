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
    use xrack_runtime::Length;

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
    let edit = id(accesskit::Role::TextInput);
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
