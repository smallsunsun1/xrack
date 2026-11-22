use runfiles::{Runfiles, rlocation};
use std::{
    cell::RefCell,
    io,
    rc::Rc,
    time::{Duration, Instant},
};
use workbench_ui::{
    ActivityRow, AgentWorkbench, AttachmentRow, ChatFindDocument, CommandGroup, ConversationRow,
    ConversationTab, GenerationChoice, PaletteItem, ProjectGroupRow, QueuedRow, ReviewFile,
    ReviewLine, ReviewMatchRow, ReviewNoteRow, ReviewPair, SidebarMenuState, TerminalTab,
    TranscriptEntry, TranscriptSection, TurnActivity, WorkspaceFile, WorkspacePreviewTab,
};
use xrack_interpreter::Program;
use xrack_layout::{
    ChoiceKey, EditAction, FocusObservers, Interaction, NavigationKey, Point, ScrollGestureKind,
    ScrollLayout, TreeRowGeometry,
};
use xrack_runtime::{
    Color, ColorScheme, Component, ControlAction, EventResult, FileDialogMode, Image, KeyEvent,
    KeyModifiers, KeyPhase, Length, LocaleContext, NativeDialogOptions, NativeDialogQueue,
    NativeDialogResult, Node, Orientation, RecordList, RecordValue, StyledLink, StyledText, Value,
    combo_text,
};
use xrack_testing::{ActivationKey, Size, UiDriver};

fn workbench_program() -> Result<Program, Box<dyn std::error::Error>> {
    let runfiles = Runfiles::create()?;
    let source = std::env::var("XRACK_WORKBENCH_SOURCE")?;
    let path = rlocation!(runfiles, source)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "workbench.xr runfile"))?;
    let filename = path.to_str().ok_or("workbench.xr path must be UTF-8")?;
    let source = std::fs::read_to_string(&path)?;
    let mut preview = xrack_compiler::prepare_preview(&source, filename, Default::default())?;
    let translations = preview.take_translation_catalog();
    let (program, _) =
        Program::from_module_with_file_resources(&preview.module, &path, translations)?;
    Ok(program)
}

fn exercise_language_switch<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("input");
    ui.fill_focused("Keep this draft");
    assert_eq!(ui.node("send-button").text("text"), Some("发送"));
    assert_eq!(ui.node("status-line").text("text"), Some("等待回复…"));
    assert_eq!(ui.node("toggle-sessions").text("text"), Some("会话"));
    ui.click("open-appearance");
    assert_eq!(ui.node("appearance-dialog").text("title"), Some("外观"));
    ui.click("language-english");
    assert_eq!(
        ui.node("appearance-dialog").text("title"),
        Some("Appearance")
    );
    assert_eq!(ui.node("appearance-dark").text("text"), Some("Dark"));
    ui.click("close-appearance");
    assert_eq!(ui.node("send-button").text("text"), Some("Send"));
    assert_eq!(
        ui.node("status-line").text("text"),
        Some("Waiting for a reply…")
    );
    assert_eq!(
        ui.node("toggle-sessions").text("text"),
        Some("Conversations")
    );
    assert_eq!(ui.node("input").text("text"), Some("Keep this draft"));
    ui.click("open-appearance");
    ui.click("language-chinese");
    assert_eq!(ui.node("appearance-dark").text("text"), Some("深色"));
    ui.click("close-appearance");
    assert_eq!(ui.node("input").text("text"), Some("Keep this draft"));
}

#[test]
fn switches_language_across_imported_components_without_losing_the_draft()
-> Result<(), Box<dyn std::error::Error>> {
    let locale = LocaleContext::default();
    let mut generated = AgentWorkbench::new_with_locale(locale.clone());
    generated.set_status_kind("awaiting-reply".into());
    generated.on_select_language(move |language| locale.set_locale(&language).unwrap());
    exercise_language_switch(generated);

    let locale = LocaleContext::default();
    let instance = workbench_program()?.create_with_locale("AgentWorkbench", locale.clone())?;
    instance.set("status-kind", "awaiting-reply".to_owned().into())?;
    instance.on_mut("select-language", move |args| {
        let [Value::String(language)] = args else {
            return Err("expected a language identifier".into());
        };
        locale.set_locale(language)?;
        Ok(None)
    })?;
    exercise_language_switch(instance);
    Ok(())
}

fn exercise_composer_checkpoints<C: Component>(
    component: C,
    checkpoints: &RefCell<usize>,
    change_attachments: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(940.0, 640.0));
    ui.click("input");
    ui.fill_focused("先写一半");
    ui.advance_time(Duration::from_millis(100));
    assert_eq!(*checkpoints.borrow(), 0);
    ui.fill_focused("写完再发送");
    ui.advance_time(Duration::from_millis(251));
    assert_eq!(*checkpoints.borrow(), 1);
    ui.advance_time(Duration::from_millis(500));
    assert_eq!(*checkpoints.borrow(), 1);
    ui.update(change_attachments);
    ui.advance_time(Duration::from_millis(251));
    assert_eq!(*checkpoints.borrow(), 2);
    ui.fill_focused("退出后继续");
    ui.update(|window| assert!(window.close_requested()));
    assert_eq!(*checkpoints.borrow(), 3);
}

#[test]
fn composer_checkpoints_text_and_attachment_edits_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let checkpoints = Rc::new(RefCell::new(0));
    let mut window = AgentWorkbench::new();
    window.set_draft_store_ready(true);
    let calls = checkpoints.clone();
    window.on_save_composer_draft(move || *calls.borrow_mut() += 1);
    exercise_composer_checkpoints(window, &checkpoints, |window| {
        window.invoke_draft_files_changed()
    });
    *checkpoints.borrow_mut() = 0;
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("draft-store-ready", true.into())?;
    let calls = checkpoints.clone();
    instance.on_mut("save-composer-draft", move |_| {
        *calls.borrow_mut() += 1;
        Ok(None)
    })?;
    exercise_composer_checkpoints(instance, &checkpoints, |window| {
        window.invoke("draft-files-changed", &[]).unwrap();
    });
    Ok(())
}

fn exercise_terminal_panel<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("open-terminal");
    assert!(ui.node("terminal-panel").is_visible());
    assert_eq!(ui.focused(), Some("terminal-input"));
    ui.fill_focused("pwd");
    assert_eq!(ui.node("terminal-input").text("text"), Some(""));
    let scope = ui.path("terminal-key-scope");
    let scene = ui.scene().clone();
    ui.update(|view| {
        assert_eq!(
            view.key_event_at(
                &scene,
                &scope,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: "Enter".into(),
                    ..KeyEvent::default()
                }
            ),
            EventResult::Accept
        );
    });
    ui.advance_time(Duration::from_millis(81));
    assert!(ui.menu_shortcut("Primary+J".parse().unwrap()));
    assert!(!ui.node("terminal-panel").is_visible());
    assert_eq!(ui.focused(), Some("input"));
    assert!(ui.menu_shortcut("Primary+J".parse().unwrap()));
    ui.click("restart-terminal");
    assert_eq!(ui.focused(), Some("terminal-input"));
    assert_eq!(
        calls.borrow().as_slice(),
        ["open", "text:pwd", "key:Enter", "resize", "open", "restart"]
    );
}

#[test]
fn terminal_panel_routes_input_focus_and_resize_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_terminal_running(true);
    window.set_terminal_tab("1".into());
    let output = calls.clone();
    window.on_sync_terminal(move |restart| {
        output
            .borrow_mut()
            .push(if restart { "restart" } else { "open" }.into())
    });
    let output = calls.clone();
    window.on_terminal_text(move |text| output.borrow_mut().push(format!("text:{text}")));
    let output = calls.clone();
    window.on_terminal_key(move |event| {
        output.borrow_mut().push(format!("key:{}", event.key));
        EventResult::Accept
    });
    let output = calls.clone();
    window.on_terminal_resize(move |width, height| {
        assert!(width.0 > 300.0 && height.0 > 100.0);
        output.borrow_mut().push("resize".into());
    });
    exercise_terminal_panel(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("terminal-running", true.into())?;
    instance.set("terminal-tab", "1".to_owned().into())?;
    let output = calls.clone();
    instance.on_mut("sync-terminal", move |args| {
        let [Value::Bool(restart)] = args else {
            return Err("终端参数无效".into());
        };
        output
            .borrow_mut()
            .push(if *restart { "restart" } else { "open" }.into());
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("terminal-text", move |args| {
        let [Value::String(text)] = args else {
            return Err("终端文本无效".into());
        };
        output.borrow_mut().push(format!("text:{text}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("terminal-key", move |args| {
        let [Value::KeyEvent(event)] = args else {
            return Err("终端键值无效".into());
        };
        output.borrow_mut().push(format!("key:{}", event.key));
        Ok(Some(Value::EventResult(EventResult::Accept)))
    })?;
    let output = calls.clone();
    instance.on_mut("terminal-resize", move |args| {
        let [Value::Length(width), Value::Length(height)] = args else {
            return Err("终端尺寸无效".into());
        };
        assert!(width.0 > 300.0 && height.0 > 100.0);
        output.borrow_mut().push("resize".into());
        Ok(None)
    })?;
    exercise_terminal_panel(instance, &calls);
    Ok(())
}

fn exercise_project_actions<C: Component>(
    component: C,
    publish_catalog: impl FnOnce(&mut C),
    calls: &RefCell<Vec<String>>,
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("project-actions-button");
    assert!(ui.node("project-actions-dialog").boolean("open", false));
    assert!(!ui.can_hit("run-project-action"));
    assert_eq!(calls.borrow().as_slice(), ["load"]);
    ui.update(publish_catalog);
    ui.click("project-action-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert_eq!(
        ui.node("project-action-command-preview").text("text"),
        Some("bazel test //tests:expressions_test")
    );
    ui.click("run-project-action");
    assert!(!ui.node("project-actions-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("terminal-input"));
    assert_eq!(
        calls.borrow().as_slice(),
        ["load", "selected:check", "run:check"]
    );
}

#[test]
fn project_actions_preview_before_run_and_focus_the_terminal_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    const COMMAND: &str = "bazel test //tests:expressions_test";
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_terminal_open(true);
    let output = calls.clone();
    window.on_load_project_actions(move || output.borrow_mut().push("load".into()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_select_project_action(move || {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("selected:{}", ui.get_project_action_id()));
        ui.set_project_action_command(COMMAND.into());
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_run_project_action(move || {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("run:{}", ui.get_project_action_id()));
        ui.set_project_actions_open(false);
        ui.invoke_show_terminal_panel();
    });
    exercise_project_actions(
        window,
        |ui| {
            for (id, name) in [("build", "构建"), ("check", "测试")] {
                ui.model_project_actions().push(GenerationChoice {
                    key: id.into(),
                    label: name.into(),
                });
            }
            ui.set_project_action_id("build".into());
        },
        &calls,
    );
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("terminal-open", true.into())?;
    let output = calls.clone();
    instance.on_mut("load-project-actions", move |_| {
        output.borrow_mut().push("load".into());
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("select-project-action", move |_| {
        let ui = weak.upgrade().unwrap();
        let Value::String(id) = ui
            .get("project-action-id")
            .map_err(|error| error.to_string())?
        else {
            return Err("操作 ID 无效".into());
        };
        output.borrow_mut().push(format!("selected:{id}"));
        ui.set("project-action-command", COMMAND.to_owned().into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("run-project-action", move |_| {
        let ui = weak.upgrade().unwrap();
        let Value::String(id) = ui
            .get("project-action-id")
            .map_err(|error| error.to_string())?
        else {
            return Err("操作 ID 无效".into());
        };
        output.borrow_mut().push(format!("run:{id}"));
        ui.set("project-actions-open", false.into())
            .map_err(|error| error.to_string())?;
        ui.invoke("show-terminal-panel", &[])
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_project_actions(
        instance,
        |ui| {
            let choices = [("build", "构建"), ("check", "测试")].map(|(id, name)| {
                RecordValue::new(
                    "GenerationChoice",
                    [
                        ("key".into(), id.to_owned().into()),
                        ("label".into(), name.to_owned().into()),
                    ]
                    .into(),
                )
            });
            ui.set(
                "project-actions",
                Value::RecordList(RecordList::new("GenerationChoice", choices.to_vec())),
            )
            .unwrap();
            ui.set("project-action-id", "build".to_owned().into())
                .unwrap();
        },
        &calls,
    );
    Ok(())
}

fn exercise_terminal_quote<C: Component>(component: C, hits: &RefCell<Vec<(i64, i64, String)>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert_eq!(ui.node("quote-terminal").text("text"), Some("引用输出"));
    ui.click("quote-terminal");
    assert!(hits.borrow()[0].2.is_empty());
    ui.click("terminal-document");
    select_all_context_text(&mut ui);
    assert_eq!(ui.node("quote-terminal").text("text"), Some("引用选段"));
    ui.click("quote-terminal");
    let output = "构建失败\nfailed";
    assert_eq!(hits.borrow()[1], (0, output.len() as i64, output.into()));
}

#[test]
fn terminal_quote_routes_visible_page_and_utf8_selection_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let hits = Rc::new(RefCell::new(Vec::new()));
    let screen = StyledText::from_plain_text("构建失败\nfailed");
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_terminal_open(true);
    window.set_terminal_screen(screen.clone());
    window.set_terminal_output_available(true);
    let output = hits.clone();
    window.on_attach_terminal_output(move |start, end, text| {
        output.borrow_mut().push((start, end, text));
    });
    exercise_terminal_quote(window, &hits);
    hits.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("terminal-open", true.into())?;
    instance.set("terminal-output-available", true.into())?;
    instance.set("terminal-screen", screen.into())?;
    let output = hits.clone();
    instance.on_mut("attach-terminal-output", move |args| {
        let [Value::Int(start), Value::Int(end), Value::String(text)] = args else {
            return Err("终端选区参数无效".into());
        };
        output.borrow_mut().push((*start, *end, text.clone()));
        Ok(None)
    })?;
    exercise_terminal_quote(instance, &hits);
    Ok(())
}

fn exercise_terminal_tabs<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    let bar = ui.path("terminal-tabs");
    let tab = |index| [bar.as_slice(), &[index]].concat();
    ui.click("new-terminal");
    assert_eq!(ui.focused(), Some("terminal-input"));
    assert_eq!(ui.node("terminal-tabs").text("current-value"), Some("3"));
    assert!(ui.menu_shortcut("Primary+Alt+[".parse().unwrap()));
    assert_eq!(ui.node("terminal-tabs").text("current-value"), Some("2"));
    assert_eq!(ui.focused(), Some("terminal-input"));
    ui.click_at_path(&tab(0));
    assert_eq!(ui.node("terminal-tabs").text("current-value"), Some("1"));

    let center = |slot| {
        let item = ui.layout_at(&tab(slot)).unwrap();
        let rect = item.tab_header.unwrap_or(item.bounds);
        Point {
            x: rect.x + rect.width * 0.5,
            y: rect.y + rect.height * 0.5,
        }
    };
    let (start, end) = (center(0), center(2));
    let mut input = Interaction::default();
    let time = Instant::now();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        start,
        time,
        ScrollGestureKind::MouseControl
    ));
    assert!(input.move_scroll_gesture(
        ui.scene(),
        ui.layout(),
        end,
        time + Duration::from_millis(20)
    ));
    let move_tab = input
        .end_scroll_gesture(
            ui.scene(),
            ui.layout(),
            end,
            time + Duration::from_millis(40),
        )
        .control
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, move_tab.action)));
    assert_eq!(
        ui.scene().at_path(&tab(2)).unwrap().text("value"),
        Some("1")
    );
    assert_eq!(ui.node("terminal-tabs").text("current-value"), Some("1"));

    let close = input
        .accessibility_tab_close_target(ui.scene(), ui.layout(), &tab(1))
        .unwrap();
    let close = input
        .tab_close_action(ui.scene(), ui.layout(), &close)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, close.action)));
    assert_eq!(ui.node("terminal-tabs").children.len(), 2);
    assert_eq!(ui.node("terminal-tabs").text("current-value"), Some("1"));
    assert_eq!(
        calls.borrow().as_slice(),
        ["new", "cycle:-1", "select:1", "move:0:2", "close:3"]
    );
}

#[test]
fn terminal_tab_controls_route_creation_selection_reordering_and_close_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let tab = |id: &str| TerminalTab {
        id: id.into(),
        title: format!("Shell {id}"),
    };
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_terminal_open(true);
    window.set_terminal_running(true);
    window.set_terminal_tab("1".into());
    for id in ["1", "2"] {
        window.model_terminal_tabs().push(tab(id));
    }
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_new_terminal_tab(move || {
        let mut ui = weak.upgrade().unwrap();
        ui.model_terminal_tabs().push(tab("3"));
        ui.set_terminal_tab("3".into());
        output.borrow_mut().push("new".into());
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_cycle_terminal_tab(move |step| {
        weak.upgrade().unwrap().set_terminal_tab("2".into());
        output.borrow_mut().push(format!("cycle:{step}"));
    });
    let output = calls.clone();
    window.on_select_terminal_tab(move |id| output.borrow_mut().push(format!("select:{id}")));
    let output = calls.clone();
    window.on_reorder_terminal_tabs(move |from, to| {
        output.borrow_mut().push(format!("move:{from}:{to}"))
    });
    let output = calls.clone();
    let model = window.model_terminal_tabs();
    window.on_close_terminal_tab(move |id| {
        assert_eq!(model.remove(1).unwrap().id, id);
        output.borrow_mut().push(format!("close:{id}"));
    });
    exercise_terminal_tabs(window, &calls);
    calls.borrow_mut().clear();

    let record = |id: &str| {
        RecordValue::new(
            "TerminalTab",
            [
                ("id".into(), id.to_owned().into()),
                ("title".into(), format!("Shell {id}").into()),
            ]
            .into(),
        )
    };
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("terminal-open", true.into())?;
    instance.set("terminal-running", true.into())?;
    instance.set("terminal-tab", "1".to_owned().into())?;
    let rows = RecordList::new("TerminalTab", vec![record("1"), record("2")]);
    instance.set("terminal-tabs", Value::RecordList(rows.clone()))?;
    let output = calls.clone();
    let model = rows.clone();
    let weak = instance.as_weak();
    instance.on_mut("new-terminal-tab", move |_| {
        model.model().push(record("3"));
        weak.upgrade()
            .unwrap()
            .set("terminal-tab", "3".to_owned().into())
            .map_err(|error| error.to_string())?;
        output.borrow_mut().push("new".into());
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("cycle-terminal-tab", move |args| {
        let [Value::Int(step)] = args else {
            return Err("切换方向无效".into());
        };
        weak.upgrade()
            .unwrap()
            .set("terminal-tab", "2".to_owned().into())
            .map_err(|error| error.to_string())?;
        output.borrow_mut().push(format!("cycle:{step}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("select-terminal-tab", move |args| {
        let [Value::String(id)] = args else {
            return Err("标签 ID 无效".into());
        };
        output.borrow_mut().push(format!("select:{id}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("reorder-terminal-tabs", move |args| {
        let [Value::Int(from), Value::Int(to)] = args else {
            return Err("标签位置无效".into());
        };
        output.borrow_mut().push(format!("move:{from}:{to}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("close-terminal-tab", move |args| {
        let [Value::String(id)] = args else {
            return Err("标签 ID 无效".into());
        };
        rows.model().remove(1);
        output.borrow_mut().push(format!("close:{id}"));
        Ok(None)
    })?;
    exercise_terminal_tabs(instance, &calls);
    Ok(())
}

fn exercise_notification_settings<C: Component>(component: C, calls: &RefCell<Vec<&str>>) {
    let mut ui = UiDriver::headless(component, Size::new(980.0, 690.0));
    ui.click("activity-toggle");
    ui.click("open-notification-settings-button");
    assert!(
        ui.node("notification-settings-dialog")
            .boolean("open", false)
    );
    ui.click("notice-background");
    ui.click("question-notice-option");
    ui.click("request-notification-permission");
    ui.click("close-notification-settings");
    assert!(
        !ui.node("notification-settings-dialog")
            .boolean("open", false)
    );
    assert_eq!(
        calls.borrow().as_slice(),
        ["refresh", "save", "save", "request"]
    );
}

#[test]
fn notification_settings_routes_preferences_and_explicit_authorization_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_notifications_supported(true);
    let output = calls.clone();
    window.on_refresh_notification_access(move || output.borrow_mut().push("refresh"));
    let output = calls.clone();
    window.on_save_notification_settings(move || output.borrow_mut().push("save"));
    let output = calls.clone();
    window.on_request_notification_access(move || output.borrow_mut().push("request"));
    exercise_notification_settings(window, &calls);
    calls.borrow_mut().clear();
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("notifications-supported", true.into())?;
    for (callback, label) in [
        ("refresh-notification-access", "refresh"),
        ("save-notification-settings", "save"),
        ("request-notification-access", "request"),
    ] {
        let output = calls.clone();
        instance.on_mut(callback, move |_| {
            output.borrow_mut().push(label);
            Ok(None)
        })?;
    }
    exercise_notification_settings(instance, &calls);
    Ok(())
}

fn exercise_activity_view<C: Component>(component: C, events: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(840.0, 520.0));
    assert!(ui.menu_shortcut("Primary+Alt+U".parse().unwrap()));
    assert!(ui.node("activity-panel").is_visible());
    assert_eq!(ui.focused(), Some("activity-filter-choice"));
    assert!(ui.can_hit("mark-activity-read-button"));
    let panel = ui.bounds("activity-panel");
    let mark = ui.bounds("mark-activity-read-button");
    assert!(mark.y + mark.height <= panel.y + panel.height);
    ui.click("activity-filter-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    let list = ui.node("activity-list");
    assert_eq!(list.children.len(), 1);
    let key = list.children[0].row_key.unwrap();
    let action = ui
        .find_row_path("activity-list", key, Some("activity-row-action"))
        .unwrap();
    assert_eq!(
        ui.scene().at_path(&action).unwrap().text("text"),
        Some("执行测试")
    );
    ui.click_at_path(&action);
    assert!(ui.menu_shortcut("Primary+Alt+A".parse().unwrap()));
    ui.click("mark-activity-read-button");
    assert_eq!(ui.focused(), Some("activity-filter-choice"));
    assert!(ui.menu_shortcut("Shift+Escape".parse().unwrap()));
    assert_eq!(
        events.borrow().as_slice(),
        ["open:21", "next", "read", "read"]
    );
    assert!(ui.menu_shortcut("Escape".parse().unwrap()));
    assert!(!ui.node("activity-panel").is_visible());
    assert_eq!(ui.focused(), Some("activity-toggle"));
}

#[test]
fn activity_view_filters_cross_project_rows_and_routes_its_shortcuts_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let rows = [
        (20i64, "确认要求", "docs", "needs-input"),
        (22, "实现完成", "api", "updated"),
        (21, "执行测试", "xrack", "running"),
    ];
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_sessions_open(false);
    window.set_activity_waiting(1);
    window.set_activity_updated(1);
    window.set_activity_running(1);
    window.set_activity_can_jump(true);
    for (id, title, project, activity) in rows {
        window.model_activity_rows().push(ActivityRow {
            id,
            title: title.into(),
            project: project.into(),
            activity: activity.into(),
        });
    }
    let selected = events.clone();
    let marked = events.clone();
    let next = events.clone();
    window.on_select_conversation(move |id| selected.borrow_mut().push(format!("open:{id}")));
    window.on_mark_activity_read(move || marked.borrow_mut().push("read".into()));
    window.on_open_next_attention(move || next.borrow_mut().push("next".into()));
    exercise_activity_view(window, &events);
    events.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("sessions-open", false.into())?;
    for property in ["activity-waiting", "activity-updated", "activity-running"] {
        instance.set(property, 1i64.into())?;
    }
    instance.set("activity-can-jump", true.into())?;
    let model = RecordList::new(
        "ActivityRow",
        rows.into_iter()
            .map(|(id, title, project, activity)| {
                RecordValue::new(
                    "ActivityRow",
                    [
                        ("id".into(), id.into()),
                        ("title".into(), title.to_owned().into()),
                        ("project".into(), project.to_owned().into()),
                        ("activity".into(), activity.to_owned().into()),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    instance.set("activity-rows", Value::RecordList(model))?;
    let selected = events.clone();
    instance.on_mut("select-conversation", move |args| {
        let [Value::Int(id)] = args else {
            return Err("会话 ID 无效".into());
        };
        selected.borrow_mut().push(format!("open:{id}"));
        Ok(None)
    })?;
    for (callback, name) in [
        ("mark-activity-read", "read"),
        ("open-next-attention", "next"),
    ] {
        let output = events.clone();
        instance.on_mut(callback, move |_| {
            output.borrow_mut().push(name.into());
            Ok(None)
        })?;
    }
    exercise_activity_view(instance, &events);
    Ok(())
}

fn exercise_conversation_navigation<C: Component>(
    component: C,
    calls: &RefCell<Vec<bool>>,
    switching: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(980.0, 690.0));
    ui.click("navigation-back-button");
    assert!(ui.menu_shortcut("Primary+[".parse().unwrap()));
    assert!(ui.menu_shortcut("MouseBack".parse().unwrap()));
    ui.click("navigation-forward-button");
    assert!(ui.menu_shortcut("Primary+]".parse().unwrap()));
    assert!(ui.menu_shortcut("MouseForward".parse().unwrap()));
    assert_eq!(
        calls.borrow().as_slice(),
        [false, false, false, true, true, true]
    );
    ui.update(switching);
    assert!(!ui.can_hit("navigation-back-button"));
    assert!(!ui.menu_shortcut("MouseForward".parse().unwrap()));
    assert_eq!(calls.borrow().len(), 6);
}

#[test]
fn conversation_navigation_routes_buttons_brackets_and_mouse_shortcuts_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_navigation_back(true);
    window.set_navigation_forward(true);
    let output = calls.clone();
    window.on_navigate_conversation(move |forward| output.borrow_mut().push(forward));
    exercise_conversation_navigation(window, &calls, |view| view.set_switching(true));
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("navigation-back", true.into())?;
    instance.set("navigation-forward", true.into())?;
    let output = calls.clone();
    instance.on_mut("navigate-conversation", move |args| {
        let [Value::Bool(forward)] = args else {
            return Err("会话方向无效".into());
        };
        output.borrow_mut().push(*forward);
        Ok(None)
    })?;
    exercise_conversation_navigation(instance, &calls, |view| {
        view.set("switching", true.into()).unwrap()
    });
    Ok(())
}

fn exercise_prompt_recall<C: Component>(component: C, attach_file: impl FnOnce(&mut C)) {
    let mut ui = UiDriver::headless(component, Size::new(980.0, 690.0));
    ui.click("input");
    let key = |ui: &mut UiDriver<C>, key: &str| {
        let scene = ui.scene().clone();
        let layout = ui.layout().clone();
        let route = ui.input().key_route(&scene, &layout).unwrap();
        let mut result = None;
        ui.update(|view| {
            result = route.dispatch(
                view,
                &scene,
                &layout,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: key.into(),
                    ..KeyEvent::default()
                },
            );
        });
        FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
        ui.update(|_| {});
        result
    };
    let observe = |ui: &UiDriver<C>, expected: &str| {
        assert_eq!(ui.node("input").text("text"), Some(expected));
        assert_eq!(ui.focused(), Some("input"));
    };
    assert_eq!(key(&mut ui, "ArrowDown"), Some(EventResult::Reject));
    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Accept));
    observe(&ui, "最近一条\n还有说明");
    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Accept));
    observe(&ui, "更早的提示 🧩");
    assert_eq!(key(&mut ui, "ArrowDown"), Some(EventResult::Accept));
    observe(&ui, "最近一条\n还有说明");
    assert_eq!(key(&mut ui, "ArrowDown"), Some(EventResult::Accept));
    observe(&ui, "");

    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Accept));
    assert_eq!(key(&mut ui, "Escape"), Some(EventResult::Accept));
    observe(&ui, "");
    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Accept));
    ui.fill_focused("自行修改后的草稿");
    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Reject));
    observe(&ui, "自行修改后的草稿");
    ui.fill_focused("");
    ui.update(attach_file);
    assert_eq!(key(&mut ui, "ArrowUp"), Some(EventResult::Reject));
    observe(&ui, "");
}

#[test]
fn composer_history_arrows_preserve_focus_and_stop_browsing_after_editing()
-> Result<(), Box<dyn std::error::Error>> {
    fn recall(position: &mut Option<i64>, step: i64) -> Option<&'static str> {
        *position = match (*position, step) {
            (None, 1) => Some(0),
            (Some(0), -1) | (Some(_), 0) => None,
            (Some(index), step) => Some((index + step).clamp(0, 1)),
            _ => return None,
        };
        Some(match position {
            Some(0) => "最近一条\n还有说明",
            Some(_) => "更早的提示 🧩",
            None => "",
        })
    }

    let mut window = AgentWorkbench::new();
    let position = Rc::new(RefCell::new(None));
    let weak = window.as_weak();
    let history = position.clone();
    window.on_recall_composer_prompt(move |step| {
        let Some(text) = recall(&mut history.borrow_mut(), step) else {
            return false;
        };
        let mut view = weak.upgrade().unwrap();
        view.set_composer_recalling(!text.is_empty());
        view.set_composer_recall_caret(text.len() as i64);
        view.set_draft(text.into());
        view.invoke_restore_composer_caret(text.len() as i64);
        true
    });
    let weak = window.as_weak();
    window.on_end_composer_recall(move || {
        position.replace(None);
        weak.upgrade().unwrap().set_composer_recalling(false);
    });
    exercise_prompt_recall(window, |view| {
        view.model_attachment_rows().push(AttachmentRow {
            path: "/workspace/readme.md".into(),
            name: "readme.md".into(),
            kind: "文本".into(),
            preview: Image::Empty,
            has_preview: false,
        });
    });

    let instance = workbench_program()?.create("AgentWorkbench")?;
    let position = Rc::new(RefCell::new(None));
    let weak = instance.as_weak();
    let history = position.clone();
    instance.on_mut("recall-composer-prompt", move |args| {
        let [Value::Int(step)] = args else {
            return Err("历史方向无效".into());
        };
        let Some(text) = recall(&mut history.borrow_mut(), *step) else {
            return Ok(Some(false.into()));
        };
        let view = weak.upgrade().unwrap();
        view.set("composer-recalling", (!text.is_empty()).into())
            .unwrap();
        view.set("composer-recall-caret", (text.len() as i64).into())
            .unwrap();
        view.set("draft", text.to_owned().into()).unwrap();
        view.invoke("restore-composer-caret", &[(text.len() as i64).into()])
            .unwrap();
        Ok(Some(true.into()))
    })?;
    let weak = instance.as_weak();
    instance.on_mut("end-composer-recall", move |_| {
        position.replace(None);
        weak.upgrade()
            .unwrap()
            .set("composer-recalling", false.into())
            .unwrap();
        Ok(None)
    })?;
    exercise_prompt_recall(instance, |view| {
        let file = RecordValue::new(
            "AttachmentRow",
            [
                ("path".into(), "/workspace/readme.md".to_owned().into()),
                ("name".into(), "readme.md".to_owned().into()),
                ("kind".into(), "文本".to_owned().into()),
                ("preview".into(), Value::Image(Image::Empty)),
                ("has-preview".into(), false.into()),
            ]
            .into(),
        );
        view.set(
            "attachment-rows",
            Value::RecordList(RecordList::new("AttachmentRow", vec![file])),
        )
        .unwrap();
    });
    Ok(())
}
fn exercise_composer_settings<C: Component>(
    component: C,
    events: &RefCell<Vec<String>>,
    set_busy: impl Fn(&mut C),
    set_pending: impl Fn(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(980.0, 690.0));
    ui.click("input");
    let enter = |ui: &mut UiDriver<C>, modifiers: KeyModifiers, repeat| {
        let scene = ui.scene().clone();
        let layout = ui.layout().clone();
        let route = ui.input().key_route(&scene, &layout).unwrap();
        let mut outcome = None;
        ui.update(|component| {
            outcome = route.dispatch(
                component,
                &scene,
                &layout,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: "Enter".into(),
                    modifiers,
                    repeat,
                    ..KeyEvent::default()
                },
            );
        });
        outcome
    };
    let plain = KeyModifiers::default();
    let primary = KeyModifiers {
        primary: true,
        control: !cfg!(target_os = "macos"),
        meta: cfg!(target_os = "macos"),
        ..plain
    };
    let shift = KeyModifiers {
        shift: true,
        ..plain
    };
    let alternate = KeyModifiers {
        shift: true,
        ..primary
    };
    let accepted = Some(EventResult::Accept);
    let rejected = Some(EventResult::Reject);
    ui.fill_focused("改动说明");
    assert_eq!(enter(&mut ui, plain, false), rejected);
    assert!(events.borrow().is_empty());
    assert_eq!(enter(&mut ui, primary, false), accepted);
    assert_eq!(enter(&mut ui, primary, true), accepted);
    assert_eq!(events.borrow().as_slice(), ["send:改动说明"]);

    let select_enter = |ui: &mut UiDriver<C>, id| {
        ui.click("open-composer-settings");
        assert!(ui.node("composer-settings-dialog").boolean("open", false));
        ui.click(id);
        ui.click("close-composer-settings");
        assert_eq!(ui.focused(), Some("input"));
    };
    select_enter(&mut ui, "enter-single-line");
    ui.fill_focused("需要多行\n第二行");
    assert_eq!(enter(&mut ui, plain, false), rejected);
    assert_eq!(enter(&mut ui, primary, false), accepted);
    ui.fill_focused("单行");
    assert_eq!(enter(&mut ui, plain, false), accepted);

    select_enter(&mut ui, "enter-always");
    ui.fill_focused("第一行\n第二行");
    assert_eq!(enter(&mut ui, shift, false), rejected);
    assert!(ui.edit_focused(EditAction::Insert("\n")));
    assert_eq!(ui.node("input").text("text"), Some("第一行\n第二行\n"));
    ui.fill_focused("第一行\n第二行");
    assert_eq!(enter(&mut ui, plain, false), accepted);

    let mut composing = Interaction::default();
    composing.focus_target(ui.scene(), ui.layout(), ui.input().focused.clone().unwrap());
    composing.sync_editor(ui.scene());
    assert!(composing.key_route(ui.scene(), ui.layout()).is_some());
    composing
        .editor
        .as_mut()
        .unwrap()
        .set_preedit("拼音".into(), None);
    assert!(composing.key_route(ui.scene(), ui.layout()).is_none());

    ui.update(set_busy);
    ui.click("open-composer-settings");
    ui.click("followup-steer");
    ui.click("close-composer-settings");
    assert_eq!(ui.node("send-button").text("text"), Some("追加"));
    assert_eq!(ui.node("steer-button").text("text"), Some("排队"));
    ui.fill_focused("补充说明");
    assert_eq!(enter(&mut ui, plain, false), accepted);
    assert_eq!(enter(&mut ui, alternate, false), accepted);
    ui.click("steer-button");
    ui.update(set_pending);
    ui.click("input");
    assert_eq!(enter(&mut ui, primary, false), accepted);
    assert_eq!(
        events.borrow().as_slice(),
        [
            "send:改动说明",
            "save",
            "send:需要多行\n第二行",
            "send:单行",
            "save",
            "send:第一行\n第二行",
            "save",
            "steer:补充说明",
            "queue:补充说明",
            "queue:补充说明",
        ]
    );
}

#[test]
fn composer_send_preferences_work_with_real_focus_routes_in_aot_and_mlir()
-> Result<(), Box<dyn std::error::Error>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    let sent = events.clone();
    let followup = events.clone();
    let saved = events.clone();
    window.on_send(move |text| sent.borrow_mut().push(format!("send:{text}")));
    window.on_followup(move |text, steer| {
        followup
            .borrow_mut()
            .push(format!("{}:{text}", if steer { "steer" } else { "queue" }));
    });
    window.on_save_composer_settings(move || saved.borrow_mut().push("save".into()));
    exercise_composer_settings(
        window,
        &events,
        |view| {
            view.set_active_turn_id("turn-a".into());
            view.set_busy(true);
        },
        |view| view.set_followup_pending(true),
    );
    events.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    let sent = events.clone();
    let followup = events.clone();
    let saved = events.clone();
    instance.on_mut("send", move |args| {
        let [Value::String(text)] = args else {
            return Err("消息类型无效".into());
        };
        sent.borrow_mut().push(format!("send:{text}"));
        Ok(None)
    })?;
    instance.on_mut("followup", move |args| {
        let [Value::String(text), Value::Bool(steer)] = args else {
            return Err("追加参数无效".into());
        };
        followup
            .borrow_mut()
            .push(format!("{}:{text}", if *steer { "steer" } else { "queue" }));
        Ok(None)
    })?;
    instance.on_mut("save-composer-settings", move |_| {
        saved.borrow_mut().push("save".into());
        Ok(None)
    })?;
    exercise_composer_settings(
        instance,
        &events,
        |view| {
            view.set("active-turn-id", "turn-a".to_owned().into())
                .unwrap();
            view.set("busy", true.into()).unwrap();
        },
        |view| view.set("followup-pending", true.into()).unwrap(),
    );
    Ok(())
}

fn exercise_chat_find<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(940.0, 640.0));
    let offset = |ui: &UiDriver<C>| {
        ui.layout_at(&ui.path("transcript"))
            .unwrap()
            .scroll_on(Orientation::Vertical)
            .unwrap()
            .offset
    };
    assert!(ui.menu_shortcut("Primary+F".parse().unwrap()));
    assert_eq!(ui.focused(), Some("chat-find-input"));
    ui.fill_focused("BAZEL");
    ui.advance_time(Duration::from_millis(80));
    ui.advance_time(Duration::from_millis(8));
    let first = offset(&ui);
    assert!(
        first > 220.0,
        "revealed message start instead of the matched line ({first})"
    );
    assert_eq!(ui.focused(), Some("chat-find-input"));
    assert!(
        visible_path(ui.scene(), &|node| {
            node.styled_text("text").is_some_and(|text| {
                text.runs()
                    .iter()
                    .any(|run| run.style.underline && run.style.color == Some(Color(0xb13e11ff)))
            })
        })
        .is_some()
    );

    assert!(ui.menu_shortcut("Primary+G".parse().unwrap()));
    ui.advance_time(Duration::from_millis(8));
    let next = offset(&ui);
    assert!(
        next > first + 200.0,
        "next match should reveal the other row: {first}, {next}"
    );
    assert_eq!(ui.focused(), Some("chat-find-input"));
    ui.click("chat-find-previous");
    ui.advance_time(Duration::from_millis(8));
    assert!((offset(&ui) - first).abs() <= 1.0);
    assert_eq!(ui.focused(), Some("chat-find-input"));

    let scene = ui.scene().clone();
    let layout = ui.layout().clone();
    let route = ui.input().key_route(&scene, &layout).unwrap();
    ui.update(|view| {
        assert_eq!(
            route.dispatch(
                view,
                &scene,
                &layout,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: "Escape".into(),
                    ..KeyEvent::default()
                }
            ),
            Some(EventResult::Accept)
        )
    });
    assert!(!ui.node("chat-find-bar").is_visible());
    assert_eq!(ui.focused(), Some("input"));
}

#[test]
fn chat_find_reveals_the_shaped_line_and_preserves_the_search_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = [
        StyledText::from_plain_text(format!(
            "{}bazel test\n{}",
            "Some test output.\n".repeat(42),
            "More details.\n".repeat(53)
        )),
        StyledText::from_plain_text("Build with Bazel."),
    ];
    let mut window = AgentWorkbench::new();
    for document in &documents {
        window.model_entries().push(TranscriptEntry {
            kind: "assistant".into(),
            heading: "Codex".into(),
            status: String::new(),
            status_tone: String::new(),
            body: document.clone(),
        });
    }
    let weak = window.as_weak();
    let docs = documents.clone();
    window.on_refresh_chat_find(move || {
        let mut view = weak.upgrade().unwrap();
        let query = view.get_chat_find_query();
        if query.is_empty() {
            return;
        }
        let ranges = transcript_search::matches(docs[0].text(), &query);
        let model = view.model_chat_find_documents();
        model.clear();
        model.push(ChatFindDocument {
            body: transcript_search::highlight(&docs[0], &ranges, ranges.first(), false),
        });
        view.set_chat_find_result_query(query);
        view.set_chat_find_count(2);
        view.set_chat_find_position(1);
        view.invoke_schedule_chat_find_reveal();
    });
    let weak = window.as_weak();
    let offsets = documents
        .clone()
        .map(|doc| transcript_search::matches(doc.text(), "BAZEL")[0].start as i64);
    window.on_move_chat_find(move |step| {
        let mut view = weak.upgrade().unwrap();
        view.set_chat_find_position(if step > 0 { 2 } else { 1 });
        view.invoke_schedule_chat_find_reveal();
    });
    let weak = window.as_weak();
    window.on_reveal_chat_find(move || {
        let view = weak.upgrade().unwrap();
        let row = view.get_chat_find_position() as usize - 1;
        assert!(transcript_search::reveal(&view, row, offsets[row] as usize));
    });
    exercise_chat_find(window);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    let model = RecordList::new(
        "TranscriptEntry",
        documents
            .iter()
            .map(|body| {
                RecordValue::new(
                    "TranscriptEntry",
                    [
                        ("kind".into(), "assistant".to_owned().into()),
                        ("heading".into(), "Codex".to_owned().into()),
                        ("status".into(), String::new().into()),
                        ("status-tone".into(), String::new().into()),
                        ("body".into(), Value::StyledText(body.clone())),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    instance.set("entries", Value::RecordList(model))?;
    let hits = transcript_search::matches(documents[0].text(), "BAZEL");
    let highlighted = transcript_search::highlight(&documents[0], &hits, hits.first(), false);
    let decorations = RecordList::new(
        "ChatFindDocument",
        vec![RecordValue::new(
            "ChatFindDocument",
            [("body".into(), Value::StyledText(highlighted))].into(),
        )],
    );
    let weak = instance.as_weak();
    instance.on_mut("refresh-chat-find", move |_| {
        let view = weak.upgrade().unwrap();
        let query = view.get("chat-find-query").unwrap();
        if query != "BAZEL".to_owned().into() {
            return Ok(None);
        }
        for (key, value) in [
            ("chat-find-result-query", query),
            ("chat-find-count", 2i64.into()),
            ("chat-find-position", 1i64.into()),
        ] {
            view.set(key, value).unwrap();
        }
        view.set(
            "chat-find-documents",
            Value::RecordList(decorations.clone()),
        )
        .unwrap();
        view.invoke("schedule-chat-find-reveal", &[]).unwrap();
        Ok(None)
    })?;
    let weak = instance.as_weak();
    instance.on_mut("move-chat-find", move |args| {
        let [Value::Int(step)] = args else {
            return Err("无效的查找方向".into());
        };
        let view = weak.upgrade().unwrap();
        view.set(
            "chat-find-position",
            (if *step > 0 { 2i64 } else { 1i64 }).into(),
        )
        .unwrap();
        view.invoke("schedule-chat-find-reveal", &[]).unwrap();
        Ok(None)
    })?;
    let weak = instance.as_weak();
    instance.on_mut("reveal-chat-find", move |_| {
        let view = weak.upgrade().unwrap();
        let Value::Int(position) = view.get("chat-find-position").unwrap() else {
            return Err("无效的查找位置".into());
        };
        let row = position as usize - 1;
        assert!(transcript_search::reveal(&view, row, offsets[row] as usize));
        Ok(None)
    })?;
    exercise_chat_find(instance);
    Ok(())
}

fn exercise_pinned_sessions<C: Component>(
    component: C,
    events: &RefCell<Vec<String>>,
    apply_pin: impl Fn(&mut C, bool),
    archive: impl Fn(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(880.0, 620.0));
    assert!(!ui.node("pinned-session-list").is_visible());
    ui.click("conversation-menu-trigger");
    ui.click("pin-session");
    assert_eq!(events.borrow().as_slice(), ["pin:true"]);
    ui.update(|view| apply_pin(view, true));
    ui.click("conversation-menu-trigger");
    assert_eq!(ui.node("pin-session").text("text"), Some("取消置顶"));
    ui.escape();
    assert!(ui.node("pinned-session-list").is_visible());
    let row = ui.node("pinned-session-list").children[0].row_key.unwrap();
    assert!(ui.find_row_path("session-list", row, None).is_err());
    let pinned = ui
        .find_row_path("pinned-session-list", row, Some("conversation-action"))
        .unwrap();
    ui.click_at_path(&pinned);
    assert!(ui.menu_shortcut("Primary+Alt+P".parse().unwrap()));
    assert_eq!(
        events.borrow().as_slice(),
        ["pin:true", "open:17", "pin:false"]
    );
    ui.update(|view| apply_pin(view, false));
    assert!(!ui.node("pinned-session-list").is_visible());
    assert!(ui.find_row_path("session-list", row, None).is_ok());

    ui.update(archive);
    ui.click("conversation-menu-trigger");
    assert!(!ui.node("pin-session").is_visible());
    ui.escape();
    assert!(!ui.menu_shortcut("Primary+Alt+P".parse().unwrap()));
}

#[test]
fn pins_move_between_sidebar_groups_and_can_be_toggled_with_the_shortcut()
-> Result<(), Box<dyn std::error::Error>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let row = |pinned: bool| ConversationRow {
        id: 17,
        title: "桌面构建".into(),
        folder: String::new(),
        activity: String::new(),
        shown: !pinned,
        pinned,
        pinned_shown: pinned,
    };
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_conversation_id(17);
    window.set_connected(true);
    window.set_renamable(true);
    window.model_conversations().push(row(false));
    let pin = events.clone();
    let open = events.clone();
    window.on_set_session_pinned(move |state| pin.borrow_mut().push(format!("pin:{state}")));
    window.on_select_conversation(move |id| open.borrow_mut().push(format!("open:{id}")));
    exercise_pinned_sessions(
        window,
        &events,
        |view, pinned| {
            view.set_conversation_pinned(pinned);
            view.set_pinned_count(i64::from(pinned));
            assert!(view.model_conversations().set_row(0, row(pinned)));
        },
        |view| view.set_conversation_archived(true),
    );
    events.borrow_mut().clear();

    let record = |pinned: bool| {
        RecordValue::new(
            "ConversationRow",
            [
                ("id".into(), 17i64.into()),
                ("title".into(), "桌面构建".to_owned().into()),
                ("folder".into(), String::new().into()),
                ("activity".into(), String::new().into()),
                ("shown".into(), (!pinned).into()),
                ("pinned".into(), pinned.into()),
                ("pinned-shown".into(), pinned.into()),
            ]
            .into(),
        )
    };
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("conversation-id", 17i64.into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    let rows = RecordList::new("ConversationRow", vec![record(false)]);
    instance.set("conversations", Value::RecordList(rows.clone()))?;
    let pin = events.clone();
    let open = events.clone();
    instance.on_mut("set-session-pinned", move |args| {
        let [Value::Bool(state)] = args else {
            return Err("无效的置顶状态".into());
        };
        pin.borrow_mut().push(format!("pin:{state}"));
        Ok(None)
    })?;
    instance.on_mut("select-conversation", move |args| {
        let [Value::Int(id)] = args else {
            return Err("无效的会话 ID".into());
        };
        open.borrow_mut().push(format!("open:{id}"));
        Ok(None)
    })?;
    exercise_pinned_sessions(
        instance,
        &events,
        |view, pinned| {
            view.set("conversation-pinned", pinned.into()).unwrap();
            view.set("pinned-count", i64::from(pinned).into()).unwrap();
            assert!(rows.model().set_row(0, record(pinned)));
        },
        |view| view.set("conversation-archived", true.into()).unwrap(),
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum TranscriptUpdate {
    Append,
    Stream,
    Prepend,
    Switch,
    Restore(Length),
}

fn exercise_transcript_scrolling<C: Component>(
    component: C,
    update: impl Fn(&mut C, TranscriptUpdate),
) {
    let mut ui = UiDriver::headless(component, Size::new(880.0, 620.0));
    let scroll = |ui: &UiDriver<C>| -> ScrollLayout {
        ui.layout_at(&ui.path("transcript"))
            .unwrap()
            .scroll_on(Orientation::Vertical)
            .unwrap()
    };
    let near = |actual: f32, expected: f32, stage: &str| {
        assert!(
            (actual - expected).abs() <= 1.0,
            "{stage}: {actual} vs {expected}"
        );
    };
    let tick = |ui: &mut UiDriver<C>| ui.advance_time(Duration::from_millis(8));
    let append = |ui: &mut UiDriver<C>| {
        ui.update(|view| update(view, TranscriptUpdate::Append));
        tick(ui);
    };

    append(&mut ui);
    let bottom = scroll(&ui);
    assert!(bottom.max_offset > 300.0);
    near(bottom.offset, bottom.max_offset, "initial load");
    assert!(!ui.node("latest-message").is_visible());

    ui.click("input");
    ui.fill_focused("保留草稿和焦点");
    assert!(ui.scroll("transcript", Point { x: 0.0, y: -220.0 }));
    let reading = scroll(&ui);
    assert!(reading.offset < bottom.offset);
    assert!(ui.node("latest-message").is_visible());
    ui.update(|view| update(view, TranscriptUpdate::Stream));
    tick(&mut ui);
    let after = scroll(&ui);
    near(after.offset, reading.offset, "while reading");
    assert!(after.max_offset > reading.max_offset);

    ui.click("latest-message");
    tick(&mut ui);
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(ui.node("input").text("text"), Some("保留草稿和焦点"));
    near(scroll(&ui).offset, scroll(&ui).max_offset, "jump button");
    assert!(!ui.node("latest-message").is_visible());
    append(&mut ui);
    near(
        scroll(&ui).offset,
        scroll(&ui).max_offset,
        "after appending again",
    );

    assert!(ui.scroll("transcript", Point { x: 0.0, y: -340.0 }));
    let before = scroll(&ui);
    ui.update(|view| update(view, TranscriptUpdate::Prepend));
    tick(&mut ui);
    let after = scroll(&ui);
    assert!(after.max_offset > before.max_offset);
    near(
        after.offset - before.offset,
        after.max_offset - before.max_offset,
        "prepended history",
    );
    assert!(ui.node("latest-message").is_visible());

    ui.tab_with_direction(true);
    assert_eq!(ui.focused(), Some("latest-message"));
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert!(ui.activate(ActivationKey::Enter));
    tick(&mut ui);
    assert_eq!(ui.focused(), Some("input"));
    near(scroll(&ui).offset, scroll(&ui).max_offset, "keyboard jump");

    assert!(ui.scroll("transcript", Point { x: 0.0, y: -220.0 }));
    assert!(ui.scroll(
        "transcript",
        Point {
            x: 0.0,
            y: 10_000.0
        }
    ));
    append(&mut ui);
    near(
        scroll(&ui).offset,
        scroll(&ui).max_offset,
        "resumed by wheel",
    );
    assert!(!ui.node("latest-message").is_visible());

    assert!(ui.scroll("transcript", Point { x: 0.0, y: -220.0 }));
    let previous_position = scroll(&ui).offset;
    ui.update(|view| update(view, TranscriptUpdate::Switch));
    tick(&mut ui);
    near(
        scroll(&ui).offset,
        scroll(&ui).max_offset,
        "other conversation",
    );
    assert!(!ui.node("latest-message").is_visible());
    ui.update(|view| {
        update(
            view,
            TranscriptUpdate::Restore(Length(-f64::from(previous_position))),
        )
    });
    tick(&mut ui);
    near(
        scroll(&ui).offset,
        previous_position,
        "return to the previous reading position",
    );
    assert!(ui.node("latest-message").is_visible());
}

#[test]
fn transcript_follows_the_tail_and_preserves_a_reading_position_on_prepend()
-> Result<(), Box<dyn std::error::Error>> {
    let row = |text: &str, lines: usize| TranscriptEntry {
        kind: "assistant".into(),
        heading: "Codex".into(),
        status: String::new(),
        status_tone: String::new(),
        body: StyledText::from_plain_text(format!("{text}\n").repeat(lines)),
    };
    let mut window = AgentWorkbench::new();
    window.set_earlier_turns(true);
    for index in 0..14 {
        window
            .model_entries()
            .push(row("已有消息", (index % 3) + 1));
    }
    exercise_transcript_scrolling(window, |view, change| match change {
        TranscriptUpdate::Append => {
            view.model_entries().push(row("更新的消息", 3));
            view.invoke_transcript_updated();
        }
        TranscriptUpdate::Stream => {
            let entries = view.model_entries();
            assert!(entries.set_row(entries.len() - 1, row("本轮流式输出", 9)));
            view.invoke_transcript_updated();
        }
        TranscriptUpdate::Prepend => {
            view.invoke_preserve_transcript_anchor();
            view.model_entries().insert(0, row("历史消息", 6));
            view.set_earlier_turns(false);
        }
        TranscriptUpdate::Switch => view.invoke_reveal_latest(),
        TranscriptUpdate::Restore(offset) => view.invoke_restore_reading_position(offset),
    });

    let record = |row: TranscriptEntry| {
        RecordValue::new(
            "TranscriptEntry",
            [
                ("kind".into(), row.kind.into()),
                ("heading".into(), row.heading.into()),
                ("status".into(), row.status.into()),
                ("status-tone".into(), row.status_tone.into()),
                ("body".into(), Value::StyledText(row.body)),
            ]
            .into(),
        )
    };
    let instance = workbench_program()?.create("AgentWorkbench")?;
    let entries = RecordList::new(
        "TranscriptEntry",
        (0..14)
            .map(|index| record(row("已有消息", (index % 3) + 1)))
            .collect(),
    );
    instance.set("entries", Value::RecordList(entries.clone()))?;
    instance.set("earlier-turns", true.into())?;
    exercise_transcript_scrolling(instance, |view, change| match change {
        TranscriptUpdate::Append => {
            entries.model().push(record(row("更新的消息", 3)));
            view.invoke("transcript-updated", &[]).unwrap();
        }
        TranscriptUpdate::Stream => {
            let entries = entries.model();
            assert!(entries.set_row(entries.len() - 1, record(row("本轮流式输出", 9))));
            view.invoke("transcript-updated", &[]).unwrap();
        }
        TranscriptUpdate::Prepend => {
            view.invoke("preserve-transcript-anchor", &[]).unwrap();
            entries.model().insert(0, record(row("历史消息", 6)));
            view.set("earlier-turns", false.into()).unwrap();
        }
        TranscriptUpdate::Switch => {
            view.invoke("reveal-latest", &[]).unwrap();
        }
        TranscriptUpdate::Restore(offset) => {
            view.invoke("restore-reading-position", &[offset.into()])
                .unwrap();
        }
    });
    Ok(())
}

fn exercise_appearance<C: Component>(component: C, change_system: impl Fn(&mut C, ColorScheme)) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("input");
    ui.fill_focused("正在编辑的草稿");
    let assert_colors = |ui: &UiDriver<C>, dark: bool| {
        let (scheme, rail, composer) = if dark {
            (ColorScheme::Dark, Color(0x20252fff), Color(0x232934ff))
        } else {
            (ColorScheme::Light, Color(0xecf0f8ff), Color(0xffffffff))
        };
        assert_eq!(ui.scene().color_scheme(), Some(scheme));
        assert_eq!(ui.node("rail").color("background"), Some(rail));
        assert_eq!(
            ui.node("composer-panel").color("background"),
            Some(composer)
        );
        assert_eq!(ui.node("input").color("background"), Some(Color(0)));
        assert_eq!(ui.node("input").text("text"), Some("正在编辑的草稿"));
    };
    assert_colors(&ui, false);
    ui.update(|view| change_system(view, ColorScheme::Dark));
    assert_colors(&ui, true);

    ui.click("open-appearance");
    assert!(ui.node("appearance-dialog").boolean("open", false));
    ui.click("appearance-light");
    assert_colors(&ui, false);
    ui.update(|view| change_system(view, ColorScheme::Dark));
    assert_colors(&ui, false);
    assert!(ui.group_key(NavigationKey::Right));
    assert_colors(&ui, true);
    ui.update(|view| change_system(view, ColorScheme::Light));
    assert_colors(&ui, true);
    ui.click("appearance-system");
    assert_colors(&ui, false);
    ui.click("close-appearance");
    assert!(!ui.node("appearance-dialog").boolean("open", false));
    ui.update(|view| change_system(view, ColorScheme::Dark));
    assert_colors(&ui, true);
}

#[test]
fn appearance_picker_switches_shared_colors_without_recreating_the_draft()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_appearance(AgentWorkbench::new(), |view, scheme| {
        view.invoke_system_appearance_changed(scheme);
    });
    let instance = workbench_program()?.create("AgentWorkbench")?;
    exercise_appearance(instance, |view, scheme| {
        view.invoke("system-appearance-changed", &[scheme.into()])
            .unwrap();
    });
    Ok(())
}

fn exercise<C: Component>(component: C, set_busy: impl Fn(&mut C, bool)) {
    let viewport = Size::new(700.0, 500.0);
    let mut ui = UiDriver::headless(component, viewport);
    let composer = ui.bounds("input");
    assert!(composer.x >= 188.0 && composer.x + composer.width <= viewport.width);
    assert!(composer.y + composer.height < viewport.height);
    assert!(!ui.can_hit("send-button"));
    assert!(!ui.node("stop").is_visible());
    ui.click("refresh-sessions");
    ui.click("more-sessions");
    ui.click("overview");
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(
        ui.node("input").text("text"),
        Some("介绍这个项目的结构和运行入口。")
    );
    let row = ui.node("session-list").children[0].row_key.unwrap();
    let button = ui
        .find_row_path("session-list", row, Some("conversation-action"))
        .unwrap();
    ui.click_at_path(&button);
    ui.click("open-folder");
    let scene = ui.component().render_shared();
    let mut queue = NativeDialogQueue::default();
    queue.sync(&scene.native_dialogs.iter().collect::<Vec<_>>());
    let request = queue.begin_next().expect("folder selection should open");
    assert!(
        matches!(request.options.as_ref(), NativeDialogOptions::File(options)
        if options.mode == FileDialogMode::SelectFolder)
    );
    ui.update(|_| {
        assert!(queue.complete(request.id, NativeDialogResult::Files(vec!["/next".into()])))
    });
    ui.click("input");
    ui.fill_focused("解释这个布局");
    assert!(ui.can_hit("send-button"));
    ui.click("send-button");
    ui.update(|component| set_busy(component, true));
    assert!(!ui.can_hit("send-button"));
    assert!(ui.can_hit("stop"));
    ui.click("stop");
    assert!(!ui.node("review-panel").is_visible());
    ui.click("toggle-sessions");
    assert!(!ui.node("rail").is_visible());
    ui.click("open-review");
    assert!(ui.node("review-panel").is_visible());
    assert!(ui.bounds("review-panel").width >= viewport.width - 1.0);
    let row = ui.node("review-files").children[0].row_key.unwrap();
    let button = ui
        .find_row_path("review-files", row, Some("review-file-action"))
        .unwrap();
    ui.click_at_path(&button);
    ui.click("refresh-review-button");
    ui.click("close-review");
    assert!(!ui.node("review-panel").is_visible());
    ui.click("toggle-sessions");
    assert!(ui.node("rail").is_visible());
}

fn exercise_responsive_inspector<C: Component>(
    component: C,
    set_viewport_width: impl Fn(&mut C, Length),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 680.0));
    ui.click("input");
    ui.fill_focused("审阅完成后继续写");
    ui.click("open-files");
    assert!(ui.node("content").is_visible());
    assert!(ui.node("rail").is_visible());
    assert!(ui.can_hit("workspace-tree"));
    assert!(ui.bounds("content").width >= 360.0);

    ui.resize(Size::new(740.0, 560.0));
    ui.update(|ui| set_viewport_width(ui, Length(740.0)));
    assert!(!ui.node("content").is_visible());
    assert!(!ui.node("rail").is_visible());
    assert_eq!(ui.bounds("review-panel").width, 740.0);
    assert!(
        ui.layout_at(&ui.path("input"))
            .is_none_or(|layout| !layout.visible)
    );
    assert!(ui.can_hit("workspace-tree"));

    ui.click("workspace-changes-tab");
    assert_eq!(ui.focused(), Some("close-review"));
    assert_eq!(ui.node("close-review").text("text"), Some("返回对话"));
    assert!(ui.menu_shortcut("Escape".parse().unwrap()));
    assert!(ui.node("rail").is_visible());
    assert!(ui.node("content").is_visible());
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(ui.node("input").text("text"), Some("审阅完成后继续写"));

    assert!(ui.scroll("transcript", Point { x: 0.0, y: 125.0 }));
    let offset = |ui: &UiDriver<C>| {
        ui.layout_at(&ui.path("transcript"))
            .unwrap()
            .scroll_on(Orientation::Vertical)
            .unwrap()
            .offset
    };
    let reading = offset(&ui);
    assert!(reading > 0.0);
    assert!(ui.menu_shortcut("Primary+Shift+E".parse().unwrap()));
    assert_eq!(ui.focused(), Some("workspace-tree"));
    assert!(ui.node("workspace-files-panel").is_visible());
    ui.click("close-review");
    assert!((offset(&ui) - reading).abs() < 1.0);
    assert!(ui.menu_shortcut("Primary+Shift+E".parse().unwrap()));
    ui.resize(Size::new(1120.0, 680.0));
    ui.update(|ui| set_viewport_width(ui, Length(1120.0)));
    assert!(ui.node("rail").is_visible());
    assert!(ui.node("content").is_visible());
    assert!(ui.node("workspace-files-panel").is_visible());
    assert_eq!(ui.focused(), Some("workspace-tree"));
    assert_eq!(ui.node("input").text("text"), Some("审阅完成后继续写"));
}

#[test]
fn narrow_window_inspection_preserves_draft_and_focus_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let body = StyledText::from_plain_text(&"等待审阅结果\n".repeat(48));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_connected(true);
    window.model_entries().push(TranscriptEntry {
        kind: "assistant".into(),
        heading: "Codex".into(),
        status: String::new(),
        status_tone: String::new(),
        body: body.clone(),
    });
    exercise_responsive_inspector(window, AgentWorkbench::set_viewport_width);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            vec![RecordValue::new(
                "TranscriptEntry",
                [
                    ("kind".into(), "assistant".to_owned().into()),
                    ("heading".into(), "Codex".to_owned().into()),
                    ("status".into(), String::new().into()),
                    ("status-tone".into(), String::new().into()),
                    ("body".into(), body.into()),
                ]
                .into(),
            )],
        )),
    )?;
    exercise_responsive_inspector(instance, |ui, width| {
        ui.set("viewport-width", width.into()).unwrap();
    });
    Ok(())
}

#[test]
fn aot_and_interpreter_send_and_stop_the_same_draft() -> Result<(), Box<dyn std::error::Error>> {
    let expected = [
        "list:false",
        "list:true",
        "9",
        "/next",
        "解释这个布局",
        "interrupt",
        "refresh",
        "src/main.rs",
        "refresh",
    ];
    let aot = Rc::new(RefCell::new(Vec::<String>::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_history_more(true);
    window.set_viewport_width(Length(700.0));
    let pending = aot.clone();
    window.on_load_session_history(move |more| pending.borrow_mut().push(format!("list:{more}")));
    window.model_conversations().push(ConversationRow {
        id: 9,
        title: "Test session".into(),
        folder: "workspace".into(),
        activity: String::new(),
        pinned: false,
        pinned_shown: false,
        shown: true,
    });
    window.model_review_files().push(ReviewFile {
        path: "src/main.rs".into(),
        status: "M".into(),
    });
    let pending = aot.clone();
    window.on_refresh_review(move || pending.borrow_mut().push("refresh".into()));
    let pending = aot.clone();
    window.on_select_review_file(move |path| pending.borrow_mut().push(path));
    let pending = aot.clone();
    window.on_select_conversation(move |id| pending.borrow_mut().push(id.to_string()));
    let pending = aot.clone();
    window.on_select_workspace(move |path| pending.borrow_mut().push(path));
    let pending = aot.clone();
    window.on_send(move |draft| pending.borrow_mut().push(draft));
    let pending = aot.clone();
    window.on_interrupt(move || pending.borrow_mut().push("interrupt".into()));
    exercise(window, AgentWorkbench::set_busy);
    assert_eq!(aot.borrow().as_slice(), expected);

    let program = workbench_program()?;
    let instance = program.create("AgentWorkbench")?;
    let interpreted = Rc::new(RefCell::new(Vec::<String>::new()));
    instance.set("workspace", Value::String("/workspace".into()))?;
    instance.set("connected", Value::Bool(true))?;
    instance.set("history-more", Value::Bool(true))?;
    instance.set("viewport-width", Length(700.0).into())?;
    let pending = interpreted.clone();
    instance.on_mut("load-session-history", move |args| {
        if let [Value::Bool(more)] = args {
            pending.borrow_mut().push(format!("list:{more}"));
        }
        Ok(None)
    })?;
    instance.set(
        "review-files",
        Value::RecordList(RecordList::new(
            "ReviewFile",
            vec![RecordValue::new(
                "ReviewFile",
                [
                    ("path".into(), "src/main.rs".to_owned().into()),
                    ("status".into(), "M".to_owned().into()),
                ]
                .into(),
            )],
        )),
    )?;
    let pending = interpreted.clone();
    instance.on_mut("refresh-review", move |_| {
        pending.borrow_mut().push("refresh".into());
        Ok(None)
    })?;
    let pending = interpreted.clone();
    instance.on_mut("select-review-file", move |args| {
        if let [Value::String(path)] = args {
            pending.borrow_mut().push(path.clone());
        }
        Ok(None)
    })?;
    instance.set(
        "conversations",
        Value::RecordList(RecordList::new(
            "ConversationRow",
            vec![RecordValue::new(
                "ConversationRow",
                [
                    ("id".into(), 9i64.into()),
                    ("title".into(), "Test session".to_owned().into()),
                    ("folder".into(), "workspace".to_owned().into()),
                    ("activity".into(), String::new().into()),
                    ("shown".into(), true.into()),
                    ("pinned".into(), false.into()),
                    ("pinned-shown".into(), false.into()),
                ]
                .into(),
            )],
        )),
    )?;
    let pending = interpreted.clone();
    instance.on_mut("select-conversation", move |args| {
        if let [Value::Int(id)] = args {
            pending.borrow_mut().push(id.to_string());
        }
        Ok(None)
    })?;
    let pending = interpreted.clone();
    instance.on_mut("select-workspace", move |args| {
        if let [Value::String(path)] = args {
            pending.borrow_mut().push(path.clone());
        }
        Ok(None)
    })?;
    let pending = interpreted.clone();
    instance.on_mut("send", move |args| {
        if let [Value::String(draft)] = args {
            pending.borrow_mut().push(draft.clone());
        }
        Ok(None)
    })?;
    let pending = interpreted.clone();
    instance.on_mut("interrupt", move |_| {
        pending.borrow_mut().push("interrupt".into());
        Ok(None)
    })?;
    exercise(instance, |component, busy| {
        component.set("busy", Value::Bool(busy)).unwrap();
    });
    assert_eq!(interpreted.borrow().as_slice(), expected);
    Ok(())
}

fn visible_path(node: &Node, matches: &impl Fn(&Node) -> bool) -> Option<Vec<usize>> {
    if !node.is_visible() {
        return None;
    }
    if matches(node) {
        return Some(Vec::new());
    }
    for (index, child) in node.children.iter().enumerate() {
        if let Some(path) = visible_path(child, matches) {
            return Some(std::iter::once(index).chain(path).collect());
        }
    }
    None
}

fn has_visible_output(scene: &Node, text: &str) -> bool {
    visible_path(scene, &|node| {
        node.styled_text("text")
            .is_some_and(|body| body.text() == text)
    })
    .is_some()
}

fn exercise_git_commit<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    publish: impl Fn(&mut C, &str),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert!(!ui.can_hit("open-git-commit-button"));
    ui.update(|view| publish(view, "staged"));
    ui.click("open-git-commit-button");
    assert!(ui.node("git-commit-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("git-commit-message-input"));
    assert!(
        visible_path(ui.scene(), &|node| node.text("text")
            == Some("M    src/main.rs"))
        .is_some()
    );
    assert!(!ui.can_hit("confirm-git-commit-button"));
    ui.fill_focused("调整计数器\n\n保留工作区草稿");
    ui.click("confirm-git-commit-button");
    assert!(!ui.can_hit("confirm-git-commit-button"));
    ui.update(|view| publish(view, "stale"));
    assert!(ui.node("git-commit-error-label").is_visible());
    assert_eq!(
        ui.node("git-commit-message-input").text("text"),
        Some("调整计数器\n\n保留工作区草稿")
    );
    assert!(!ui.can_hit("confirm-git-commit-button"));
    ui.click("refresh-git-commit");
    ui.click("confirm-git-commit-button");
    ui.update(|view| publish(view, "done"));
    assert!(ui.node("git-commit-success-label").is_visible());
    assert!(!ui.node("confirm-git-commit-button").is_visible());
    ui.click("cancel-git-commit");
    assert!(!ui.node("git-commit-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "open",
            "调整计数器\n\n保留工作区草稿",
            "reload",
            "调整计数器\n\n保留工作区草稿"
        ]
    );
}

#[test]
fn staged_commit_dialog_preserves_the_message_on_reload_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_git_commit(move || {
        output.borrow_mut().push("open".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_git_commit_open(true);
        ui.set_git_commit_ready(true);
        ui.set_git_commit_branch("main".into());
        ui.set_git_commit_files(vec!["M    src/main.rs".into()]);
        ui.invoke_focus_git_commit_message();
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_git_commit(move || {
        let mut ui = weak.upgrade().unwrap();
        output.borrow_mut().push(ui.get_git_commit_message());
        ui.set_git_commit_pending(true);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_reload_git_commit(move || {
        output.borrow_mut().push("reload".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_git_commit_ready(true);
        ui.set_git_commit_error(String::new());
    });
    exercise_git_commit(window, &calls, |ui, state| match state {
        "staged" => ui.set_review_staged_count(1),
        "stale" => {
            ui.set_git_commit_pending(false);
            ui.set_git_commit_ready(false);
            ui.set_git_commit_error("暂存区已变动".into());
        }
        "done" => {
            ui.set_git_commit_pending(false);
            ui.set_git_commit_done(true);
            ui.set_git_commit_result("f7142edea25d".into());
        }
        _ => unreachable!(),
    });
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-git-commit", move |_| {
        output.borrow_mut().push("open".into());
        let ui = weak.upgrade().unwrap();
        for (key, value) in [
            ("git-commit-open", true.into()),
            ("git-commit-ready", true.into()),
            ("git-commit-branch", "main".to_owned().into()),
            (
                "git-commit-files",
                vec!["M    src/main.rs".to_owned()].into(),
            ),
        ] {
            ui.set(key, value).map_err(|error| error.to_string())?;
        }
        ui.invoke("focus-git-commit-message", &[])
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-git-commit", move |_| {
        let ui = weak.upgrade().unwrap();
        if let Value::String(message) = ui
            .get("git-commit-message")
            .map_err(|error| error.to_string())?
        {
            output.borrow_mut().push(message);
        }
        ui.set("git-commit-pending", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("reload-git-commit", move |_| {
        output.borrow_mut().push("reload".into());
        let ui = weak.upgrade().unwrap();
        ui.set("git-commit-ready", true.into())
            .map_err(|error| error.to_string())?;
        ui.set("git-commit-error", String::new().into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_git_commit(instance, &calls, |ui, state| match state {
        "staged" => ui.set("review-staged-count", 1_i64.into()).unwrap(),
        "stale" => {
            ui.set("git-commit-pending", false.into()).unwrap();
            ui.set("git-commit-ready", false.into()).unwrap();
            ui.set("git-commit-error", "暂存区已变动".to_owned().into())
                .unwrap();
        }
        "done" => {
            ui.set("git-commit-pending", false.into()).unwrap();
            ui.set("git-commit-done", true.into()).unwrap();
            ui.set("git-commit-result", "f7142edea25d".to_owned().into())
                .unwrap();
        }
        _ => unreachable!(),
    });
    Ok(())
}

fn exercise_git_push<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    finish: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("open-git-push-button");
    assert!(ui.node("git-push-dialog").boolean("open", false));
    assert_eq!(ui.node("git-push-target-input").text("text"), Some("main"));
    ui.click("git-push-remote-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("git-push-target-input");
    ui.fill_focused("");
    assert!(!ui.can_hit("confirm-git-push-button"));
    ui.fill_focused("release/workbench");
    ui.click("git-push-track-option");
    ui.click("confirm-git-push-button");
    assert!(!ui.can_hit("confirm-git-push-button"));
    ui.update(finish);
    assert!(ui.node("git-push-success-label").is_visible());
    assert!(!ui.node("confirm-git-push-button").is_visible());
    ui.click("cancel-git-push");
    assert!(!ui.node("git-push-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "open",
            "remote:backup",
            "push:backup:release/workbench:true"
        ]
    );
}

#[test]
fn git_push_requires_an_explicit_target_and_preserves_the_selected_remote_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_git_push(move || {
        output.borrow_mut().push("open".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_git_push_open(true);
        ui.set_git_push_commit("858df179e613".into());
        ui.set_git_push_source("main".into());
        ui.set_git_push_remotes(vec!["origin".into(), "backup".into()]);
        ui.set_git_push_remote("origin".into());
        ui.set_git_push_address("ssh://git.example.invalid/origin".into());
        ui.set_git_push_target("main".into());
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_select_git_push_remote(move || {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("remote:{}", ui.get_git_push_remote()));
        ui.set_git_push_address("ssh://git.example.invalid/backup".into());
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_git_push(move || {
        let mut ui = weak.upgrade().unwrap();
        output.borrow_mut().push(format!(
            "push:{}:{}:{}",
            ui.get_git_push_remote(),
            ui.get_git_push_target(),
            ui.get_git_push_track()
        ));
        ui.set_git_push_pending(true);
    });
    exercise_git_push(window, &calls, |ui| {
        ui.set_git_push_pending(false);
        ui.set_git_push_done(true);
    });
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-git-push", move |_| {
        output.borrow_mut().push("open".into());
        let ui = weak.upgrade().unwrap();
        for (name, value) in [
            ("git-push-open", true.into()),
            ("git-push-commit", "858df179e613".to_owned().into()),
            ("git-push-source", "main".to_owned().into()),
            (
                "git-push-remotes",
                vec!["origin".to_owned(), "backup".to_owned()].into(),
            ),
            ("git-push-remote", "origin".to_owned().into()),
            (
                "git-push-address",
                "ssh://git.example.invalid/origin".to_owned().into(),
            ),
            ("git-push-target", "main".to_owned().into()),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("select-git-push-remote", move |_| {
        let ui = weak.upgrade().unwrap();
        if let Value::String(remote) = ui
            .get("git-push-remote")
            .map_err(|error| error.to_string())?
        {
            output.borrow_mut().push(format!("remote:{remote}"));
        }
        ui.set(
            "git-push-address",
            "ssh://git.example.invalid/backup".to_owned().into(),
        )
        .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-git-push", move |_| {
        let ui = weak.upgrade().unwrap();
        let remote = ui
            .get("git-push-remote")
            .map_err(|error| error.to_string())?;
        let target = ui
            .get("git-push-target")
            .map_err(|error| error.to_string())?;
        let track = ui
            .get("git-push-track")
            .map_err(|error| error.to_string())?;
        if let (Value::String(remote), Value::String(target), Value::Bool(track)) =
            (remote, target, track)
        {
            output
                .borrow_mut()
                .push(format!("push:{remote}:{target}:{track}"));
        }
        ui.set("git-push-pending", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_git_push(instance, &calls, |ui| {
        ui.set("git-push-pending", false.into()).unwrap();
        ui.set("git-push-done", true.into()).unwrap();
    });
    Ok(())
}

fn exercise_git_pr<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    finish: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("create-pr-after-push");
    assert!(ui.node("git-pr-dialog").boolean("open", false));
    assert_eq!(ui.node("git-pr-base-input").text("text"), Some("main"));
    ui.click("git-pr-base-input");
    ui.fill_focused("release/main");
    ui.click("git-pr-title-input");
    ui.fill_focused("Improve focus");
    ui.click("git-pr-body-input");
    ui.fill_focused("## Changes\n\nKeep the current selection.");
    ui.click("git-pr-draft-option");
    ui.click("confirm-git-pr-button");
    assert!(!ui.can_hit("confirm-git-pr-button"));
    ui.update(finish);
    assert_eq!(
        ui.node("git-pr-result-url").text("text"),
        Some("https://github.com/team/ui/pull/84")
    );
    ui.click("copy-git-pr-button");
    ui.click("cancel-git-pr");
    assert!(!ui.node("git-pr-dialog").boolean("open", false));
    ui.click("open-git-pr-button");
    assert!(ui.node("git-pr-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "open:true",
            "team/ui:release/main:Improve focus:true:## Changes\n\nKeep the current selection.",
            "copy",
            "open:false"
        ]
    );
}

#[test]
fn pull_request_form_routes_the_pushed_source_draft_and_link_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_git_push_open(true);
    window.set_git_push_done(true);
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_git_pr(move |pushed| {
        output.borrow_mut().push(format!("open:{pushed}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_git_pr_source("github.com/contributor/ui:topic/ui · 858df179e613".into());
        ui.set_git_pr_repositories(vec!["team/ui".into()]);
        ui.set_git_pr_repo("team/ui".into());
        ui.set_git_pr_branch("main".into());
        ui.set_git_pr_url(String::new());
        ui.set_git_pr_open(true);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_git_pr(move || {
        let mut ui = weak.upgrade().unwrap();
        output.borrow_mut().push(format!(
            "{}:{}:{}:{}:{}",
            ui.get_git_pr_repo(),
            ui.get_git_pr_branch(),
            ui.get_git_pr_title(),
            ui.get_git_pr_draft(),
            ui.get_git_pr_body()
        ));
        ui.set_git_pr_pending(true);
    });
    let output = calls.clone();
    window.on_copy_git_pr_link(move || output.borrow_mut().push("copy".into()));
    exercise_git_pr(window, &calls, |ui| {
        ui.set_git_pr_pending(false);
        ui.set_git_pr_url("https://github.com/team/ui/pull/84".into());
    });
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    instance.set("git-push-open", true.into())?;
    instance.set("git-push-done", true.into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-git-pr", move |args| {
        let [Value::Bool(pushed)] = args else {
            return Err("invalid PR source".into());
        };
        output.borrow_mut().push(format!("open:{pushed}"));
        let ui = weak.upgrade().unwrap();
        for (name, value) in [
            (
                "git-pr-source",
                "github.com/contributor/ui:topic/ui · 858df179e613"
                    .to_owned()
                    .into(),
            ),
            ("git-pr-repositories", vec!["team/ui".to_owned()].into()),
            ("git-pr-repo", "team/ui".to_owned().into()),
            ("git-pr-branch", "main".to_owned().into()),
            ("git-pr-url", String::new().into()),
            ("git-pr-open", true.into()),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-git-pr", move |_| {
        let ui = weak.upgrade().unwrap();
        let values = [
            "git-pr-repo",
            "git-pr-branch",
            "git-pr-title",
            "git-pr-draft",
            "git-pr-body",
        ]
        .map(|name| ui.get(name).unwrap());
        if let [
            Value::String(repo),
            Value::String(branch),
            Value::String(title),
            Value::Bool(draft),
            Value::String(body),
        ] = &values
        {
            output
                .borrow_mut()
                .push(format!("{repo}:{branch}:{title}:{draft}:{body}"));
        }
        ui.set("git-pr-pending", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("copy-git-pr-link", move |_| {
        output.borrow_mut().push("copy".into());
        Ok(None)
    })?;
    exercise_git_pr(instance, &calls, |ui| {
        ui.set("git-pr-pending", false.into()).unwrap();
        ui.set(
            "git-pr-url",
            "https://github.com/team/ui/pull/84".to_owned().into(),
        )
        .unwrap();
    });
    Ok(())
}

fn exercise_git_branch_picker<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    set_narrow_width: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("git-branch-pill");
    assert!(ui.node("git-branch-dialog").boolean("open", false));
    let occupied = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("occupied")
    })
    .unwrap();
    assert!(
        !ui.scene()
            .at_path(&occupied)
            .unwrap()
            .boolean("enabled", true)
    );
    ui.click("git-branch-query-input");
    ui.fill_focused("feature");
    let next = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("feature/ui")
    })
    .unwrap();
    ui.click_at_path(&next);
    ui.click("confirm-git-branch-button");
    assert!(ui.node("git-branch-complete").is_visible());
    ui.click("cancel-git-branch");
    assert_eq!(ui.node("git-branch-pill").text("text"), Some("feature/ui"));
    ui.click("git-branch-pill");
    ui.click("git-branch-mode-create");
    ui.click("git-branch-name-input");
    ui.fill_focused("codex/worktree-task");
    ui.click("confirm-git-branch-button");
    ui.click("cancel-git-branch");
    assert_eq!(
        ui.node("git-branch-pill").text("text"),
        Some("codex/worktree-task")
    );
    ui.resize(Size::new(680.0, 620.0));
    ui.update(set_narrow_width);
    assert!(!ui.node("git-branch-pill").is_visible());
    assert!(ui.bounds("header-workspace").width >= 106.0);
    ui.click("conversation-menu-trigger");
    ui.click("git-branches-menu");
    assert!(ui.node("git-branch-dialog").boolean("open", false));
    ui.click("cancel-git-branch");
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "open",
            "filter:feature",
            "switch:feature/ui",
            "open",
            "create:codex/worktree-task",
            "open",
        ]
    );
}

#[test]
fn branch_picker_routes_switch_and_create_and_marks_occupied_rows_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let branches = [
        ("main", "current"),
        ("feature/ui", ""),
        ("occupied", "occupied"),
    ];
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_git_checkout_label("main".into());
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_git_branches(move || {
        output.borrow_mut().push("open".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_git_branch_open(true);
        ui.set_git_branch_ready(true);
        ui.set_git_branch_done(false);
        ui.set_git_branch_mode("switch".into());
        ui.set_git_branch_choices(
            branches
                .iter()
                .map(|(name, state)| workbench_ui::BranchChoice {
                    name: (*name).into(),
                    state: (*state).into(),
                })
                .collect(),
        );
    });
    let output = calls.clone();
    window.on_filter_git_branches(move |query| output.borrow_mut().push(format!("filter:{query}")));
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_git_branch(move || {
        let mut ui = weak.upgrade().unwrap();
        let mode = ui.get_git_branch_mode();
        let name = if mode == "create" {
            ui.get_git_branch_new_name()
        } else {
            ui.get_git_branch_target()
        };
        output.borrow_mut().push(format!("{mode}:{name}"));
        ui.set_git_checkout_label(name);
        ui.set_git_branch_done(true);
    });
    exercise_git_branch_picker(window, &calls, |view| {
        view.set_viewport_width(Length(680.0))
    });
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("git-checkout-label", "main".to_owned().into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-git-branches", move |_| {
        output.borrow_mut().push("open".into());
        let ui = weak.upgrade().unwrap();
        let records = branches
            .iter()
            .map(|(name, state)| {
                RecordValue::new(
                    "BranchChoice",
                    [
                        ("name".into(), (*name).to_owned().into()),
                        ("state".into(), (*state).to_owned().into()),
                    ]
                    .into(),
                )
            })
            .collect();
        for (name, value) in [
            ("git-branch-open", true.into()),
            ("git-branch-ready", true.into()),
            ("git-branch-done", false.into()),
            ("git-branch-mode", "switch".to_owned().into()),
            (
                "git-branch-choices",
                Value::RecordList(RecordList::new("BranchChoice", records)),
            ),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("filter-git-branches", move |args| {
        if let [Value::String(query)] = args {
            output.borrow_mut().push(format!("filter:{query}"));
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-git-branch", move |_| {
        let ui = weak.upgrade().unwrap();
        let Value::String(mode) = ui
            .get("git-branch-mode")
            .map_err(|error| error.to_string())?
        else {
            return Err("invalid branch action".into());
        };
        let field = if mode == "create" {
            "git-branch-new-name"
        } else {
            "git-branch-target"
        };
        let Value::String(name) = ui.get(field).map_err(|error| error.to_string())? else {
            return Err("invalid branch name".into());
        };
        output.borrow_mut().push(format!("{mode}:{name}"));
        ui.set("git-checkout-label", name.into())
            .map_err(|error| error.to_string())?;
        ui.set("git-branch-done", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_git_branch_picker(instance, &calls, |view| {
        view.set("viewport-width", Length(680.0).into()).unwrap();
    });
    Ok(())
}

fn exercise_worktree_start<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("composer-worktree-location");
    assert!(ui.node("worktree-start-dialog").boolean("open", false));
    assert!(
        ui.node("worktree-include-status")
            .text("text")
            .is_some_and(|text| text.contains("2"))
    );
    assert!(ui.node("worktree-include-status").is_visible());
    assert!(ui.can_hit("worktree-start-copy-option"));
    ui.click("worktree-start-branch-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert!(!ui.can_hit("worktree-start-copy-option"));
    ui.click("use-worktree-composer");
    assert!(!ui.node("worktree-start-dialog").boolean("open", false));
    assert!(
        ui.node("composer-worktree-location")
            .text("text")
            .is_some_and(|location| location.contains("feature/ui"))
    );
    assert_eq!(
        calls.borrow().as_slice(),
        ["open", "base:feature/ui", "configure:feature/ui"]
    );
    ui.click("input");
    ui.fill_focused("在隔离工作树里检查控件");
    ui.click("send-button");
    assert!(!ui.can_hit("send-button"));
    assert_eq!(
        ui.node("input").text("text"),
        Some("在隔离工作树里检查控件")
    );
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "open",
            "base:feature/ui",
            "configure:feature/ui",
            "send:feature/ui:在隔离工作树里检查控件"
        ]
    );
}

#[test]
fn new_conversation_selects_a_worktree_before_sending_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_worktree_available(true);
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_worktree_start(move || {
        output.borrow_mut().push("open".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_worktree_start_open(true);
        ui.set_worktree_start_choices(vec!["HEAD".into(), "feature/ui".into()]);
        ui.set_worktree_start_base("HEAD".into());
        ui.set_worktree_start_commit("84df174a2a61".into());
        ui.set_worktree_start_can_copy(true);
        ui.set_worktree_start_include_count(2);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_select_worktree_start_base(move || {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("base:{}", ui.get_worktree_start_base()));
        ui.set_worktree_start_can_copy(false);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_configure_worktree_start(move |isolated| {
        assert!(isolated);
        let mut ui = weak.upgrade().unwrap();
        let base = ui.get_worktree_start_base();
        output.borrow_mut().push(format!("configure:{base}"));
        ui.set_worktree_start_label(base);
        ui.set_worktree_start_open(false);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_send(move |prompt| {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("send:{}:{prompt}", ui.get_worktree_start_label()));
        ui.set_worktree_preparing(true);
    });
    exercise_worktree_start(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("worktree-available", true.into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-worktree-start", move |_| {
        output.borrow_mut().push("open".into());
        let ui = weak.upgrade().unwrap();
        for (name, value) in [
            ("worktree-start-open", true.into()),
            (
                "worktree-start-choices",
                vec!["HEAD".to_owned(), "feature/ui".to_owned()].into(),
            ),
            ("worktree-start-base", "HEAD".to_owned().into()),
            ("worktree-start-commit", "84df174a2a61".to_owned().into()),
            ("worktree-start-can-copy", true.into()),
            ("worktree-start-include-count", 2_i64.into()),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("select-worktree-start-base", move |_| {
        let ui = weak.upgrade().unwrap();
        if let Value::String(base) = ui
            .get("worktree-start-base")
            .map_err(|error| error.to_string())?
        {
            output.borrow_mut().push(format!("base:{base}"));
        }
        ui.set("worktree-start-can-copy", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("configure-worktree-start", move |args| {
        assert_eq!(args, &[Value::Bool(true)]);
        let ui = weak.upgrade().unwrap();
        let base = ui
            .get("worktree-start-base")
            .map_err(|error| error.to_string())?;
        if let Value::String(base) = &base {
            output.borrow_mut().push(format!("configure:{base}"));
        }
        ui.set("worktree-start-label", base)
            .map_err(|error| error.to_string())?;
        ui.set("worktree-start-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("send", move |args| {
        let ui = weak.upgrade().unwrap();
        let base = ui
            .get("worktree-start-label")
            .map_err(|error| error.to_string())?;
        if let ([Value::String(prompt)], Value::String(base)) = (args, base) {
            output.borrow_mut().push(format!("send:{base}:{prompt}"));
        }
        ui.set("worktree-preparing", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_worktree_start(instance, &calls);
    Ok(())
}

fn exercise_handoff<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    retry: impl FnOnce(&mut C),
    finish: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("conversation-menu-trigger");
    ui.click("handoff-session");
    assert_eq!(
        ui.node("handoff-target-label").text("text"),
        Some("/workspace/project")
    );
    ui.click("handoff-copy-option");
    ui.click("confirm-handoff-button");
    assert!(!ui.can_hit("confirm-handoff-button"));
    ui.update(retry);
    assert!(ui.node("handoff-error-label").is_visible());
    assert!(!ui.can_hit("handoff-copy-option"));
    ui.click("confirm-handoff-button");
    ui.update(finish);
    assert!(!ui.node("handoff-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        ["open", "confirm:true", "confirm:true"]
    );
}

#[test]
fn handoff_dialog_routes_copy_confirmation_and_retry_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/worktree".into());
    window.set_connected(true);
    window.set_renamable(true);
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_open_handoff(move || {
        output.borrow_mut().push("open".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_handoff_destination("/workspace/project".into());
        ui.set_handoff_to_local(true);
        ui.set_handoff_can_copy(true);
        ui.set_handoff_preview_ready(true);
        ui.set_handoff_open(true);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_handoff(move || {
        let mut ui = weak.upgrade().unwrap();
        output
            .borrow_mut()
            .push(format!("confirm:{}", ui.get_handoff_copy()));
        ui.set_handoff_pending(true);
    });
    exercise_handoff(
        window,
        &calls,
        |ui| {
            ui.set_handoff_error("暂时无法确认目录更新".into());
            ui.set_handoff_prepared(true);
            ui.set_handoff_pending(false);
        },
        |ui| {
            ui.set_handoff_pending(false);
            ui.set_handoff_open(false);
        },
    );
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/worktree".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("open-handoff", move |_| {
        output.borrow_mut().push("open".into());
        let ui = weak.upgrade().unwrap();
        for (name, value) in [
            (
                "handoff-destination",
                "/workspace/project".to_owned().into(),
            ),
            ("handoff-to-local", true.into()),
            ("handoff-can-copy", true.into()),
            ("handoff-preview-ready", true.into()),
            ("handoff-open", true.into()),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-handoff", move |_| {
        let ui = weak.upgrade().unwrap();
        if let Value::Bool(copy) = ui.get("handoff-copy").map_err(|error| error.to_string())? {
            output.borrow_mut().push(format!("confirm:{copy}"));
        }
        ui.set("handoff-pending", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_handoff(
        instance,
        &calls,
        |ui| {
            ui.set("handoff-error", "暂时无法确认目录更新".to_owned().into())
                .unwrap();
            ui.set("handoff-prepared", true.into()).unwrap();
            ui.set("handoff-pending", false.into()).unwrap();
        },
        |ui| {
            ui.set("handoff-pending", false.into()).unwrap();
            ui.set("handoff-open", false.into()).unwrap();
        },
    );
    Ok(())
}

fn exercise_git_comparison<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("compare-review");
    assert_eq!(
        ui.node("review-scope-dialog").text("title"),
        Some("比较 Git 版本")
    );
    assert!(!ui.can_hit("submit-code-review"));
    ui.click("review-branch-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("submit-code-review");
    assert_eq!(ui.node("review-title").text("text"), Some("分支对比"));
    assert!(!ui.node("git-change-filters").is_visible());
    ui.click("inspect-diff-source");
    ui.click("workspace-review-button");
    assert!(ui.node("git-change-filters").is_visible());
    ui.click("compare-review");
    ui.click("review-scope-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("review-revision");
    ui.fill_focused("HEAD~1");
    ui.click("submit-code-review");
    assert_eq!(ui.node("review-title").text("text"), Some("提交改动"));
    assert_eq!(
        calls.borrow().as_slice(),
        ["branch:main", "source", "worktree", "commit:HEAD~1"]
    );
}

#[test]
fn git_comparison_switches_branch_commit_and_historical_source_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    const TREE: &str = "0123456789012345678901234567890123456789";
    let branches = [("", "选择分支"), ("main", "main")];
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_review_selection("src/main.rs".into());
    for (key, label) in branches {
        window.model_review_branch_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_start_review_comparison(move |mode, value| {
        output.borrow_mut().push(format!("{mode}:{value}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_review_comparison_mode(mode);
        ui.set_review_comparison_target(value);
        ui.set_review_comparison_source_tree(TREE.into());
        ui.set_review_scope_open(false);
    });
    let output = calls.clone();
    window.on_open_comparison_source(move || output.borrow_mut().push("source".into()));
    let output = calls.clone();
    window.on_select_review_filter(move || output.borrow_mut().push("worktree".into()));
    exercise_git_comparison(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    instance.set("review-selection", "src/main.rs".to_owned().into())?;
    instance.set(
        "review-branch-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            branches
                .iter()
                .map(|&(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("start-review-comparison", move |args| {
        let [Value::String(mode), Value::String(value)] = args else {
            return Err("比较参数无效".into());
        };
        output.borrow_mut().push(format!("{mode}:{value}"));
        let ui = weak.upgrade().unwrap();
        for (property, value) in [
            ("review-comparison-mode", mode.clone().into()),
            ("review-comparison-target", value.clone().into()),
            ("review-comparison-source-tree", TREE.to_owned().into()),
            ("review-scope-open", false.into()),
        ] {
            ui.set(property, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    for (callback, label) in [
        ("open-comparison-source", "source"),
        ("select-review-filter", "worktree"),
    ] {
        let output = calls.clone();
        instance.on_mut(callback, move |_| {
            output.borrow_mut().push(label.into());
            Ok(None)
        })?;
    }
    exercise_git_comparison(instance, &calls);
    Ok(())
}

fn exercise_index_review<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    set_busy: impl Fn(&mut C, bool),
    show_turn: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert_eq!(
        ui.node("git-change-filters").text("current-value"),
        Some("all")
    );
    ui.click("review-filter-unstaged");
    assert_eq!(
        ui.node("git-change-filters").text("current-value"),
        Some("unstaged")
    );
    assert!(!ui.node("unstage-review-file").is_visible());
    ui.click("stage-review-file");

    ui.click("review-filter-staged");
    assert_eq!(
        ui.node("git-change-filters").text("current-value"),
        Some("staged")
    );
    assert!(!ui.node("stage-review-file").is_visible());
    ui.update(|ui| set_busy(ui, true));
    assert!(!ui.can_hit("unstage-review-file"));
    ui.update(|ui| set_busy(ui, false));
    ui.click("unstage-review-file");
    assert_eq!(
        calls.borrow().as_slice(),
        ["filter:unstaged", "stage", "filter:staged", "unstage"]
    );
    ui.click("review-filter-unstaged");
    assert!(ui.group_key(NavigationKey::Right));
    assert_eq!(
        calls.borrow().last().map(String::as_str),
        Some("filter:staged")
    );

    ui.update(show_turn);
    assert!(!ui.node("git-change-filters").is_visible());
    assert!(!ui.node("unstage-review-file").is_visible());
}

#[test]
fn changes_panel_filters_git_sides_and_routes_file_actions_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_review_selection("src/main.rs".into());
    window.set_review_can_stage(true);
    window.set_review_can_unstage(true);
    let filter = calls.clone();
    let weak = window.as_weak();
    window.on_select_review_filter(move || {
        let value = weak.upgrade().unwrap().get_review_filter();
        filter.borrow_mut().push(format!("filter:{value}"));
    });
    let actions = calls.clone();
    window.on_update_review_index(move |stage| {
        actions
            .borrow_mut()
            .push(if stage { "stage" } else { "unstage" }.into());
    });
    exercise_index_review(
        window,
        &calls,
        AgentWorkbench::set_review_index_busy,
        |ui| ui.set_review_turn(0),
    );
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    instance.set("review-selection", "src/main.rs".to_owned().into())?;
    instance.set("review-can-stage", true.into())?;
    instance.set("review-can-unstage", true.into())?;
    let filter = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("select-review-filter", move |_| {
        let value = weak
            .upgrade()
            .unwrap()
            .get("review-filter")
            .map_err(|error| error.to_string())?;
        let Value::String(value) = value else {
            return Err("Git 筛选类型无效".into());
        };
        filter.borrow_mut().push(format!("filter:{value}"));
        Ok(None)
    })?;
    let actions = calls.clone();
    instance.on_mut("update-review-index", move |args| {
        let [Value::Bool(stage)] = args else {
            return Err("Git 操作类型无效".into());
        };
        actions
            .borrow_mut()
            .push(if *stage { "stage" } else { "unstage" }.into());
        Ok(None)
    })?;
    exercise_index_review(
        instance,
        &calls,
        |ui, busy| {
            ui.set("review-index-busy", busy.into()).unwrap();
        },
        |ui| {
            ui.set("review-turn", 0i64.into()).unwrap();
        },
    );
    Ok(())
}

fn exercise_conversation_tabs<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(680.0, 560.0));
    let bar = ui.path("open-conversation-tabs");
    let tab = |index| [bar.as_slice(), &[index]].concat();
    let mut input = Interaction::default();
    let first = input
        .accessibility_target(ui.scene(), ui.layout(), &tab(0))
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), first);
    let select = input
        .group_key(ui.scene(), ui.layout(), NavigationKey::Right)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, select.action)));
    assert_eq!(
        ui.node("open-conversation-tabs").text("current-value"),
        Some("second")
    );
    assert!(ui.menu_shortcut("Ctrl+Tab".parse().unwrap()));
    assert!(ui.menu_shortcut("Ctrl+Shift+Tab".parse().unwrap()));

    let reorder = ui.node("open-conversation-tabs").tab_move(1, 0).unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, ControlAction::MoveTab(reorder))));
    assert_eq!(
        ui.node("open-conversation-tabs").children[0].text("value"),
        Some("second")
    );
    let close = input
        .accessibility_tab_close_target(ui.scene(), ui.layout(), &tab(1))
        .unwrap();
    let close = input
        .tab_close_action(ui.scene(), ui.layout(), &close)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, close.action)));
    assert!(ui.menu_shortcut("Primary+W".parse().unwrap()));
    assert!(ui.menu_shortcut("Primary+Shift+T".parse().unwrap()));
    assert!(ui.can_hit("new-conversation-tab"));
    ui.click("new-conversation-tab");
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "select:second",
            "cycle:1",
            "cycle:-1",
            "move:1:0",
            "close:first",
            "close:second",
            "reopen",
            "new"
        ]
    );
    assert_eq!(ui.node("input").text("text"), Some("仍保留的草稿"));
}

#[test]
fn conversation_tab_controls_route_mouse_keyboard_and_close_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let tabs = [
        ("first", "构建检查", "/alpha", ""),
        ("second", "文档任务", "/beta", "running"),
    ];
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/alpha".into());
    window.set_connected(true);
    window.set_draft("仍保留的草稿".into());
    window.set_conversation_tab("first".into());
    window.set_can_reopen_conversation_tab(true);
    for (id, title, workspace, activity) in tabs {
        window.model_conversation_tabs().push(ConversationTab {
            id: id.into(),
            title: title.into(),
            workspace: workspace.into(),
            activity: activity.into(),
            archived: false,
        });
    }
    let report = calls.clone();
    window.on_select_conversation_tab(move |id| report.borrow_mut().push(format!("select:{id}")));
    let report = calls.clone();
    window.on_cycle_conversation_tab(move |step| report.borrow_mut().push(format!("cycle:{step}")));
    let report = calls.clone();
    window.on_reorder_conversation_tabs(move |from, to| {
        report.borrow_mut().push(format!("move:{from}:{to}"))
    });
    let report = calls.clone();
    window.on_close_conversation_tab(move |id| report.borrow_mut().push(format!("close:{id}")));
    let report = calls.clone();
    window.on_reopen_conversation_tab(move || report.borrow_mut().push("reopen".into()));
    let report = calls.clone();
    window.on_new_conversation(move || report.borrow_mut().push("new".into()));
    exercise_conversation_tabs(window, &calls);
    calls.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    view.set("workspace", "/alpha".to_owned().into())?;
    view.set("connected", true.into())?;
    view.set("draft", "仍保留的草稿".to_owned().into())?;
    view.set("conversation-tab", "first".to_owned().into())?;
    view.set("can-reopen-conversation-tab", true.into())?;
    view.set(
        "conversation-tabs",
        Value::RecordList(RecordList::new(
            "ConversationTab",
            tabs.into_iter()
                .map(|(id, title, workspace, activity)| {
                    RecordValue::new(
                        "ConversationTab",
                        [
                            ("id".into(), id.to_owned().into()),
                            ("title".into(), title.to_owned().into()),
                            ("workspace".into(), workspace.to_owned().into()),
                            ("activity".into(), activity.to_owned().into()),
                            ("archived".into(), false.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    for (event, label) in [
        ("select-conversation-tab", "select"),
        ("cycle-conversation-tab", "cycle"),
        ("reorder-conversation-tabs", "move"),
        ("close-conversation-tab", "close"),
        ("reopen-conversation-tab", "reopen"),
        ("new-conversation", "new"),
    ] {
        let report = calls.clone();
        view.on_mut(event, move |args| {
            let call = match args {
                [Value::String(id)] => format!("{label}:{id}"),
                [Value::Int(step)] => format!("{label}:{step}"),
                [Value::Int(from), Value::Int(to)] => format!("{label}:{from}:{to}"),
                [] => label.into(),
                _ => return Err("unexpected conversation tab arguments".into()),
            };
            report.borrow_mut().push(call);
            Ok(None)
        })?;
    }
    exercise_conversation_tabs(view, &calls);
    Ok(())
}

fn exercise_preview_tabs<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    let bar = ui.path("workspace-preview-tabs");
    let tab = |index| [bar.as_slice(), &[index]].concat();
    let mut input = Interaction::default();
    let first = input
        .accessibility_target(ui.scene(), ui.layout(), &tab(0))
        .unwrap();
    input.focus_target(ui.scene(), ui.layout(), first);
    let select = input
        .group_key(ui.scene(), ui.layout(), NavigationKey::Right)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, select.action)));
    assert_eq!(
        ui.node("workspace-preview-tabs").text("current-value"),
        Some("second")
    );
    ui.advance_time(Duration::from_millis(2));
    let document = ui.path("workspace-text-preview");
    let view = ui
        .layout_at(&document)
        .unwrap()
        .scroll_on(Orientation::Vertical)
        .unwrap();
    assert_eq!(view.offset, 128.0);

    let center = |slot| {
        let item = ui.layout_at(&tab(slot)).unwrap();
        let rect = item.tab_header.unwrap_or(item.bounds);
        Point {
            x: rect.x + rect.width * 0.5,
            y: rect.y + rect.height * 0.5,
        }
    };
    let (first_point, last_point) = (center(1), center(2));
    let now = Instant::now();
    assert!(input.begin_scroll_gesture(
        ui.scene(),
        ui.layout(),
        first_point,
        now,
        ScrollGestureKind::MouseControl
    ));
    assert!(input.move_scroll_gesture(
        ui.scene(),
        ui.layout(),
        last_point,
        now + Duration::from_millis(20)
    ));
    let release = input.end_scroll_gesture(
        ui.scene(),
        ui.layout(),
        last_point,
        now + Duration::from_millis(40),
    );
    let reorder = release.control.unwrap();
    assert!(matches!(reorder.action, ControlAction::MoveTab(_)));
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, reorder.action)));
    assert_eq!(
        ui.node("workspace-preview-tabs").children[2].text("value"),
        Some("second")
    );

    let close = input
        .accessibility_tab_close_target(ui.scene(), ui.layout(), &tab(1))
        .unwrap();
    let close = input
        .tab_close_action(ui.scene(), ui.layout(), &close)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &bar, close.action)));
    assert_eq!(
        calls.borrow().as_slice(),
        ["select:second", "move:1:2", "close:third"]
    );
    assert_eq!(
        ui.node("workspace-preview-tabs").text("current-value"),
        Some("second")
    );
}

#[test]
fn slash_commands_select_and_dismiss_with_the_interpreter() -> Result<(), Box<dyn std::error::Error>>
{
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    let weak = instance.as_weak();
    instance.on_mut("filter-slash-commands", move |args| {
        let [Value::String(text)] = args else {
            return Err("expected draft".into());
        };
        let ui = weak.upgrade().unwrap();
        let rows = [("/model", "模型"), ("/plan", "计划")]
            .into_iter()
            .filter(|(title, _)| title.starts_with(text.as_str()) && text.starts_with('/'))
            .map(|(title, detail)| {
                RecordValue::new(
                    "PaletteItem",
                    [
                        ("key".into(), title.to_owned().into()),
                        ("title".into(), title.to_owned().into()),
                        ("detail".into(), detail.to_owned().into()),
                        ("enabled".into(), true.into()),
                    ]
                    .into(),
                )
            })
            .collect::<Vec<_>>();
        ui.set("slash-command-draft", text.clone().into()).unwrap();
        ui.set("slash-command-caret", (text.len() as i64).into())
            .unwrap();
        ui.set("slash-command-index", 0i64.into()).unwrap();
        ui.set("slash-commands-open", (!rows.is_empty()).into())
            .unwrap();
        ui.set(
            "slash-command-items",
            Value::RecordList(RecordList::new("PaletteItem", rows)),
        )
        .unwrap();
        Ok(None)
    })?;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let selected = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("activate-slash-command", move |args| {
        let [Value::Int(index)] = args else {
            return Err("expected index".into());
        };
        selected.borrow_mut().push(*index);
        let ui = weak.upgrade().unwrap();
        ui.set("slash-commands-open", false.into()).unwrap();
        ui.set("draft", String::new().into()).unwrap();
        Ok(None)
    })?;
    let mut ui = UiDriver::headless(instance, Size::new(680.0, 620.0));
    let key = |ui: &mut UiDriver<xrack_interpreter::Instance>, key: &str| {
        let scope = ui.path("composer-key-scope");
        let scene = ui.scene().clone();
        ui.update(|view| {
            assert_eq!(
                view.key_event_at(
                    &scene,
                    &scope,
                    KeyPhase::CapturePressed,
                    &KeyEvent {
                        key: key.into(),
                        ..KeyEvent::default()
                    },
                ),
                EventResult::Accept,
            )
        });
    };
    ui.click("input");
    ui.fill_focused("/m");
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    assert!(ui.node("slash-commands").is_visible());
    key(&mut ui, "Escape");
    assert!(!ui.node("slash-commands").is_visible());
    assert_eq!(ui.node("input").text("text"), Some("/m"));
    ui.fill_focused("/");
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    key(&mut ui, "ArrowDown");
    key(&mut ui, "Enter");
    assert_eq!(calls.borrow().as_slice(), [1]);
    assert_eq!(ui.node("input").text("text"), Some(""));
    assert_eq!(ui.focused(), Some("input"));
    Ok(())
}

fn exercise_palette<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("open-palette-button");
    assert!(ui.node("workspace-palette").boolean("open", false));
    assert_eq!(ui.focused(), Some("palette-query-field"));
    ui.fill_focused("文件");
    assert!(!ui.node("palette-results").boolean("enabled", true));
    ui.advance_time(Duration::from_millis(80));
    assert!(ui.node("palette-results").boolean("enabled", false));
    let scope = ui.path("palette-key-scope");
    let scene = ui.scene().clone();
    ui.update(|view| {
        assert_eq!(
            view.key_event_at(
                &scene,
                &scope,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: "ArrowDown".into(),
                    ..KeyEvent::default()
                }
            ),
            EventResult::Accept,
        )
    });
    let search = ui.path("palette-query-field");
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.dispatch_at(&scene, &search, "accepted")));
    assert!(!ui.node("workspace-palette").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        ["begin", "search:文件", "activate:1"]
    );
    assert_eq!(ui.node("input").text("text"), Some("之前的草稿"));
}

#[test]
fn command_palette_validates_queries_and_routes_arrow_and_enter_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_draft("之前的草稿".into());
    for (key, title) in [
        ("quick-open", "快速打开文件"),
        ("workspace-files", "显示文件树"),
    ] {
        window.model_palette_items().push(PaletteItem {
            key: key.into(),
            title: title.into(),
            detail: String::new(),
            enabled: true,
        });
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    window.on_begin_palette_search(move || output.borrow_mut().push("begin".into()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_search_palette(move || {
        let mut ui = weak.upgrade().unwrap();
        let query = ui.get_palette_query();
        output.borrow_mut().push(format!("search:{query}"));
        ui.set_palette_result_query(query);
        ui.set_palette_index(0);
        ui.set_palette_ready(true);
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_activate_palette_item(move |index| {
        output.borrow_mut().push(format!("activate:{index}"));
        weak.upgrade().unwrap().set_palette_open(false);
    });
    exercise_palette(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("draft", "之前的草稿".to_owned().into())?;
    instance.set(
        "palette-items",
        Value::RecordList(RecordList::new(
            "PaletteItem",
            [
                ("quick-open", "快速打开文件"),
                ("workspace-files", "显示文件树"),
            ]
            .into_iter()
            .map(|(key, title)| {
                RecordValue::new(
                    "PaletteItem",
                    [
                        ("key".into(), key.to_owned().into()),
                        ("title".into(), title.to_owned().into()),
                        ("detail".into(), String::new().into()),
                        ("enabled".into(), true.into()),
                    ]
                    .into(),
                )
            })
            .collect(),
        )),
    )?;
    let output = calls.clone();
    instance.on_mut("begin-palette-search", move |_| {
        output.borrow_mut().push("begin".into());
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("search-palette", move |_| {
        let ui = weak.upgrade().unwrap();
        let query = ui.get("palette-query").map_err(|error| error.to_string())?;
        let Value::String(query) = query else {
            return Err("搜索内容无效".into());
        };
        output.borrow_mut().push(format!("search:{query}"));
        ui.set("palette-result-query", query.into())
            .map_err(|error| error.to_string())?;
        ui.set("palette-index", 0i64.into())
            .map_err(|error| error.to_string())?;
        ui.set("palette-ready", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("activate-palette-item", move |args| {
        let [Value::Int(index)] = args else {
            return Err("结果索引无效".into());
        };
        output.borrow_mut().push(format!("activate:{index}"));
        weak.upgrade()
            .unwrap()
            .set("palette-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_palette(instance, &calls);
    Ok(())
}

fn tab_scroll_document() -> StyledText {
    StyledText::from_plain_text("preview line\n".repeat(90))
}

fn exercise_mention<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    skill: bool,
    set_enter_mode: impl Fn(&mut C, &str),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.update(|view| set_enter_mode(view, "enter"));
    ui.click("input");
    let query = if skill {
        "请看 $counter"
    } else {
        "请看 @counter"
    };
    ui.fill_focused(query);
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    ui.advance_time(Duration::from_millis(80));
    let list = ui.node("file-mention-list");
    assert!(list.is_visible());
    assert_eq!(
        list.text("accessible-name"),
        Some(if skill {
            "可添加到草稿的技能"
        } else {
            "可附加的工作区文件"
        })
    );
    assert_eq!(ui.focused(), Some("input"));

    let key = |ui: &mut UiDriver<C>, key: &str| {
        let scene = ui.scene().clone();
        let layout = ui.layout().clone();
        let route = ui.input().key_route(&scene, &layout).unwrap();
        let mut outcome = None;
        ui.update(|view| {
            outcome = route.dispatch(
                view,
                &scene,
                &layout,
                KeyPhase::CapturePressed,
                &KeyEvent {
                    key: key.into(),
                    ..KeyEvent::default()
                },
            );
        });
        outcome
    };
    assert_eq!(key(&mut ui, "ArrowDown"), Some(EventResult::Accept));
    assert_eq!(key(&mut ui, "Enter"), Some(EventResult::Accept));
    assert!(ui.find_path("file-mention-list").is_err());
    assert_eq!(ui.node("input").text("text"), Some("请看 "));
    assert_eq!(ui.focused(), Some("input"));

    ui.fill_focused(query);
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    ui.advance_time(Duration::from_millis(80));
    assert_eq!(key(&mut ui, "Escape"), Some(EventResult::Accept));
    assert!(ui.find_path("file-mention-list").is_err());
    ui.update(|view| set_enter_mode(view, "modified"));
    assert_eq!(key(&mut ui, "Enter"), Some(EventResult::Reject));
    assert_eq!(ui.node("input").text("text"), Some(query));

    ui.fill_focused(if skill {
        "请看 $counter-ui"
    } else {
        "请看 @counter.xr"
    });
    FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
    ui.advance_time(Duration::from_millis(80));
    let row = ui.node("file-mention-list").children[0].row_key.unwrap();
    let choice = ui
        .find_row_path("file-mention-list", row, Some("palette-result-action"))
        .unwrap();
    ui.click_at_path(&choice);
    assert_eq!(ui.focused(), Some("input"));
    assert!(ui.find_path("file-mention-list").is_err());
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "search:15",
            "attach:1",
            "search:15",
            "search:18",
            "attach:0"
        ]
    );
}

#[test]
fn inline_mentions_preserve_composer_focus_and_cancel_without_sending()
-> Result<(), Box<dyn std::error::Error>> {
    let program = workbench_program()?;
    for skill in [false, true] {
        exercise_mention_backends(&program, skill)?;
    }
    Ok(())
}

fn exercise_mention_backends(
    program: &Program,
    skill: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = if skill {
        [
            (".agents/skills/counter-ui/SKILL.md", "$counter-ui"),
            (".agents/skills/counter-tests/SKILL.md", "$counter-tests"),
        ]
    } else {
        [
            ("examples/counter/counter.xr", "counter.xr"),
            ("examples/counter/main.rs", "main.rs"),
        ]
    };
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_skill_mention(skill);
    for (path, title) in candidates {
        window.model_file_mention_items().push(PaletteItem {
            key: path.into(),
            title: title.into(),
            detail: "examples/counter".into(),
            enabled: true,
        });
    }
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_search_file_mention(move |caret, selected| {
        assert!(!selected);
        let mut ui = weak.upgrade().unwrap();
        output.borrow_mut().push(format!("search:{caret}"));
        ui.set_file_mention_draft(ui.get_draft());
        ui.set_file_mention_caret(caret);
        ui.set_file_mention_index(0);
        ui.set_file_mention_open(true);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_attach_file_mention(move |index| {
        output.borrow_mut().push(format!("attach:{index}"));
        let mut ui = weak.upgrade().unwrap();
        ui.invoke_dismiss_file_mention();
        ui.set_draft("请看 ".into());
        ui.invoke_restore_composer_caret(7);
    });
    let output = calls.clone();
    window.on_send(move |_| output.borrow_mut().push("unexpected send".into()));
    exercise_mention(window, &calls, skill, |view, mode| {
        view.set_composer_enter_mode(mode.into())
    });
    calls.borrow_mut().clear();

    let instance = program.create("AgentWorkbench")?;
    instance.set("skill-mention", skill.into())?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set(
        "file-mention-items",
        Value::RecordList(RecordList::new(
            "PaletteItem",
            candidates
                .into_iter()
                .map(|(path, title)| {
                    RecordValue::new(
                        "PaletteItem",
                        [
                            ("key".into(), path.to_owned().into()),
                            ("title".into(), title.to_owned().into()),
                            ("detail".into(), "examples/counter".to_owned().into()),
                            ("enabled".into(), true.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("search-file-mention", move |args| {
        let [Value::Int(caret), Value::Bool(false)] = args else {
            return Err("未取得输入框的光标位置".into());
        };
        let ui = weak.upgrade().unwrap();
        output.borrow_mut().push(format!("search:{caret}"));
        let draft = ui.get("draft").map_err(|error| error.to_string())?;
        for (property, value) in [
            ("file-mention-draft", draft),
            ("file-mention-caret", (*caret).into()),
            ("file-mention-index", 0i64.into()),
            ("file-mention-open", true.into()),
        ] {
            ui.set(property, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("attach-file-mention", move |args| {
        let [Value::Int(index)] = args else {
            return Err("文件索引无效".into());
        };
        output.borrow_mut().push(format!("attach:{index}"));
        let ui = weak.upgrade().unwrap();
        ui.invoke("dismiss-file-mention", &[])
            .map_err(|error| error.to_string())?;
        ui.set("draft", "请看 ".to_owned().into())
            .map_err(|error| error.to_string())?;
        ui.invoke("restore-composer-caret", &[7i64.into()])
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    instance.set("connected", true.into())?;
    let output = calls.clone();
    instance.on_mut("send", move |_| {
        output.borrow_mut().push("unexpected send".into());
        Ok(None)
    })?;
    exercise_mention(instance, &calls, skill, |view, mode| {
        view.set("composer-enter-mode", mode.to_owned().into())
            .unwrap();
    });
    Ok(())
}

fn select_all_context_text<C: Component>(ui: &mut UiDriver<C>) {
    assert!(ui.keyboard_context_menu());
    assert!(ui.click_menu_row("Select All"));
    let _ = FocusObservers::default().sync(ui.input(), ui.scene(), ui.layout(), true);
    ui.update(|_| {});
}

fn exercise_file_editor<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("attach-current-workspace-file");
    ui.click("start-file-edit");
    ui.advance_time(Duration::from_millis(2));
    assert_eq!(ui.focused(), Some("workspace-file-editor"));
    assert!(!ui.can_hit("save-file-edit"));
    ui.fill_focused("let value = 2;\n");
    select_all_context_text(&mut ui);
    assert!(ui.can_hit("attach-editor-excerpt"));
    ui.click("attach-editor-excerpt");
    assert!(ui.can_hit("save-file-edit"));
    ui.click("save-file-edit");
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "file",
            "edit:let value = 2;\n",
            "excerpt:0:15:let value = 2;\n",
            "save"
        ]
    );
    assert!(ui.node("workspace-file-editor").boolean("read-only", false));
    assert!(!ui.can_hit("save-file-edit"));

    ui.update(|view| assert!(!view.close_requested()));
    assert!(
        ui.node("workspace-window-confirm-dialog")
            .boolean("open", false)
    );
    assert!(!ui.can_hit("discard-and-close-workbench"));
    ui.click("cancel-workspace-window-close");
    assert!(
        !ui.node("workspace-window-confirm-dialog")
            .boolean("open", false)
    );
}

#[test]
fn workspace_file_editor_focuses_edits_and_blocks_close_while_saving_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_files_active(true);
    window.set_workspace_file_path("src/lib.rs".into());
    window.set_workspace_file_kind("text".into());
    window.set_workspace_preview_tab("source".into());
    window
        .model_workspace_preview_tabs()
        .push(WorkspacePreviewTab {
            id: "source".into(),
            title: "lib.rs".into(),
            path: "src/lib.rs".into(),
            dirty: false,
        });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    window.on_attach_workspace_file(move || output.borrow_mut().push("file".into()));
    let output = events.clone();
    window.on_attach_workspace_editor_excerpt(move |start, end, text| {
        output
            .borrow_mut()
            .push(format!("excerpt:{start}:{end}:{text}"));
    });
    let weak = window.as_weak();
    window.on_start_workspace_file_edit(move || {
        let mut ui = weak.upgrade().unwrap();
        ui.set_workspace_file_edit_text("let value = 1;\n".into());
        ui.set_workspace_file_editing(true);
        ui.set_workspace_file_edit_focus(true);
    });
    let output = events.clone();
    let weak = window.as_weak();
    window.on_change_workspace_file_edit(move |text| {
        output.borrow_mut().push(format!("edit:{text}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_workspace_file_edit_dirty(true);
        ui.set_workspace_edits_dirty(true);
    });
    let output = events.clone();
    let weak = window.as_weak();
    window.on_save_workspace_file_edit(move || {
        output.borrow_mut().push("save".into());
        let mut ui = weak.upgrade().unwrap();
        ui.set_workspace_file_edit_saving(true);
        ui.set_workspace_edits_saving(true);
    });
    exercise_file_editor(window, &events);
    events.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    for (key, value) in [
        ("workspace", "/workspace".to_owned().into()),
        ("review-open", true.into()),
        ("files-active", true.into()),
        ("workspace-file-path", "src/lib.rs".to_owned().into()),
        ("workspace-file-kind", "text".to_owned().into()),
        ("workspace-preview-tab", "source".to_owned().into()),
        (
            "workspace-preview-tabs",
            Value::RecordList(RecordList::new(
                "WorkspacePreviewTab",
                vec![RecordValue::new(
                    "WorkspacePreviewTab",
                    [
                        ("id".into(), "source".to_owned().into()),
                        ("title".into(), "lib.rs".to_owned().into()),
                        ("path".into(), "src/lib.rs".to_owned().into()),
                        ("dirty".into(), false.into()),
                    ]
                    .into(),
                )],
            )),
        ),
    ] {
        instance.set(key, value)?;
    }
    let output = events.clone();
    instance.on_mut("attach-workspace-file", move |_| {
        output.borrow_mut().push("file".into());
        Ok(None)
    })?;
    let output = events.clone();
    instance.on_mut("attach-workspace-editor-excerpt", move |args| {
        let [Value::Int(start), Value::Int(end), Value::String(text)] = args else {
            return Err("选段参数无效".into());
        };
        output
            .borrow_mut()
            .push(format!("excerpt:{start}:{end}:{text}"));
        Ok(None)
    })?;
    let weak = instance.as_weak();
    instance.on_mut("start-workspace-file-edit", move |_| {
        let ui = weak.upgrade().unwrap();
        ui.set(
            "workspace-file-edit-text",
            "let value = 1;\n".to_owned().into(),
        )
        .map_err(|error| error.to_string())?;
        ui.set("workspace-file-editing", true.into())
            .map_err(|error| error.to_string())?;
        ui.set("workspace-file-edit-focus", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = events.clone();
    let weak = instance.as_weak();
    instance.on_mut("change-workspace-file-edit", move |args| {
        let [Value::String(text)] = args else {
            return Err("编辑内容无效".into());
        };
        output.borrow_mut().push(format!("edit:{text}"));
        let ui = weak.upgrade().unwrap();
        ui.set("workspace-file-edit-dirty", true.into())
            .map_err(|error| error.to_string())?;
        ui.set("workspace-edits-dirty", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = events.clone();
    let weak = instance.as_weak();
    instance.on_mut("save-workspace-file-edit", move |_| {
        output.borrow_mut().push("save".into());
        let ui = weak.upgrade().unwrap();
        ui.set("workspace-file-edit-saving", true.into())
            .map_err(|error| error.to_string())?;
        ui.set("workspace-edits-saving", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_file_editor(instance, &events);
    Ok(())
}

#[test]
fn file_preview_tabs_route_keyboard_selection_drag_reorder_and_close_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let nodes = [
        ("first", "main.rs", "src/main.rs"),
        ("second", "lib.rs", "src/lib.rs"),
        ("third", "README.md", "README.md"),
    ];
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_files_active(true);
    window.set_workspace_preview_tab("first".into());
    for (id, title, path) in nodes {
        window
            .model_workspace_preview_tabs()
            .push(WorkspacePreviewTab {
                id: id.into(),
                title: title.into(),
                path: path.into(),
                dirty: false,
            });
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let report = events.clone();
    let weak = window.as_weak();
    window.on_select_workspace_preview_tab(move |id| {
        report.borrow_mut().push(format!("select:{id}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_workspace_file_path("src/lib.rs".into());
        ui.set_workspace_file_text(tab_scroll_document());
        ui.set_workspace_file_kind("text".into());
        ui.set_workspace_file_restore_y(Length(128.0));
        ui.set_workspace_file_scroll_pending(true);
    });
    let report = events.clone();
    window.on_reorder_workspace_preview_tabs(move |from, to| {
        report.borrow_mut().push(format!("move:{from}:{to}"))
    });
    let report = events.clone();
    window
        .on_close_workspace_preview_tab(move |id| report.borrow_mut().push(format!("close:{id}")));
    exercise_preview_tabs(window, &events);
    events.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    instance.set("files-active", true.into())?;
    instance.set("workspace-preview-tab", "first".to_owned().into())?;
    instance.set(
        "workspace-preview-tabs",
        Value::RecordList(RecordList::new(
            "WorkspacePreviewTab",
            nodes
                .into_iter()
                .map(|(id, title, path)| {
                    RecordValue::new(
                        "WorkspacePreviewTab",
                        [
                            ("id".into(), id.to_owned().into()),
                            ("title".into(), title.to_owned().into()),
                            ("path".into(), path.to_owned().into()),
                            ("dirty".into(), false.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    for (event, name) in [
        ("select-workspace-preview-tab", "select"),
        ("close-workspace-preview-tab", "close"),
    ] {
        let output = events.clone();
        let weak = instance.as_weak();
        instance.on_mut(event, move |args| {
            let [Value::String(id)] = args else {
                return Err("文件标签标识无效".into());
            };
            output.borrow_mut().push(format!("{name}:{id}"));
            if name == "select" {
                let ui = weak.upgrade().unwrap();
                for (key, value) in [
                    ("workspace-file-path", "src/lib.rs".to_owned().into()),
                    (
                        "workspace-file-text",
                        Value::StyledText(tab_scroll_document()),
                    ),
                    ("workspace-file-kind", "text".to_owned().into()),
                    ("workspace-file-restore-y", Value::Length(Length(128.0))),
                    ("workspace-file-scroll-pending", true.into()),
                ] {
                    ui.set(key, value).map_err(|error| error.to_string())?;
                }
            }
            Ok(None)
        })?;
    }
    let output = events.clone();
    instance.on_mut("reorder-workspace-preview-tabs", move |args| {
        let [Value::Int(from), Value::Int(to)] = args else {
            return Err("文件标签索引无效".into());
        };
        output.borrow_mut().push(format!("move:{from}:{to}"));
        Ok(None)
    })?;
    exercise_preview_tabs(instance, &events);
    Ok(())
}

fn exercise_turn_activity<C: Component>(component: C, calls: &RefCell<Vec<i64>>) {
    let mut ui = UiDriver::headless(component, Size::new(680.0, 560.0));
    let command = |scene: &Node| {
        visible_path(scene, &|node| {
            node.kind == "Button" && node.text("text") == Some("local-check")
        })
    };
    let summary = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("本轮活动 · 1 条命令")
    })
    .unwrap();
    assert!(command(ui.scene()).is_none());
    assert!(has_visible_output(ui.scene(), "检查已完成"));
    ui.click_at_path(&summary);
    assert!(command(ui.scene()).is_some());
    assert!(has_visible_output(ui.scene(), "检查已完成"));
    ui.click_at_path(&summary);
    assert!(command(ui.scene()).is_none());
    assert_eq!(calls.borrow().as_slice(), [1, 1]);
}

#[test]
fn completed_activity_can_be_expanded_with_the_answer_visible_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let entries = [
        ("user", "你", "请检查构建"),
        ("command", "local-check", "命令输出"),
        ("assistant", "Codex", "检查已完成"),
    ];
    let activity = TurnActivity {
        turn: "turn".into(),
        summary: "本轮活动 · 1 条命令".into(),
        expanded: false,
        attention: false,
    };
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    for (kind, heading, body) in entries {
        window.model_entries().push(TranscriptEntry {
            kind: kind.into(),
            heading: heading.into(),
            status: "已完成".into(),
            status_tone: String::new(),
            body: StyledText::from_plain_text(body),
        });
    }
    let rows = [
        TurnActivity::default(),
        activity.clone(),
        TurnActivity::default(),
    ];
    for row in rows.clone() {
        window.model_turn_activities().push(row);
    }
    let weak = window.as_weak();
    let reported = calls.clone();
    window.on_toggle_turn_activity(move |index| {
        reported.borrow_mut().push(index);
        let model = weak.upgrade().unwrap().model_turn_activities();
        let mut row = model.row(index as usize).unwrap();
        row.expanded = !row.expanded;
        model.set_row(index as usize, row);
    });
    exercise_turn_activity(window, &calls);
    calls.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    view.set("workspace", "/workspace".to_owned().into())?;
    view.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            entries
                .into_iter()
                .map(|(kind, heading, body)| {
                    RecordValue::new(
                        "TranscriptEntry",
                        [
                            ("kind".into(), kind.to_owned().into()),
                            ("heading".into(), heading.to_owned().into()),
                            ("status".into(), "已完成".to_owned().into()),
                            ("status-tone".into(), String::new().into()),
                            (
                                "body".into(),
                                Value::StyledText(StyledText::from_plain_text(body)),
                            ),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let record = |row: TurnActivity| {
        RecordValue::new(
            "TurnActivity",
            [
                ("turn".into(), row.turn.into()),
                ("summary".into(), row.summary.into()),
                ("expanded".into(), row.expanded.into()),
                ("attention".into(), row.attention.into()),
            ]
            .into(),
        )
    };
    let activity_rows = RecordList::new("TurnActivity", rows.into_iter().map(record).collect());
    view.set("turn-activities", Value::RecordList(activity_rows.clone()))?;
    let reported = calls.clone();
    let mut activity = activity;
    view.on_mut("toggle-turn-activity", move |args| {
        let [Value::Int(index)] = args else {
            return Err("expected activity index".into());
        };
        reported.borrow_mut().push(*index);
        activity.expanded = !activity.expanded;
        assert!(
            activity_rows
                .model()
                .set_row(*index as usize, record(activity.clone()))
        );
        Ok(None)
    })?;
    exercise_turn_activity(view, &calls);
    Ok(())
}

fn exercise_code_section<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    source_code: &str,
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    let copy = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("复制代码")
    })
    .expect("code copy button");
    ui.click_at_path(&copy);
    let link = visible_path(ui.scene(), &|node| {
        node.kind == "StyledText" && node.text("text") == Some("源码")
    })
    .expect("prose containing the workspace link");
    assert!(
        ui.layout_at(&link)
            .unwrap()
            .text_link_regions()
            .next()
            .is_some()
    );
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &link, ControlAction::ActivateTextLink(0))));
    let target = source_code.find("let π").unwrap();
    ui.update(|view| assert!(transcript_search::reveal_section(view, 0, 1, target)));
    let transcript = ui.path("transcript");
    let offset = ui
        .layout_at(&transcript)
        .unwrap()
        .scroll_on(Orientation::Vertical)
        .unwrap()
        .offset;
    assert!(
        offset > 250.0,
        "expected to reveal the matched line in the code block, offset={offset}"
    );
    assert_eq!(
        calls.borrow().as_slice(),
        [
            format!("0:8:{source_code}"),
            "workspace-file:src/lib.rs:7".into()
        ]
    );
}

#[test]
fn fenced_code_buttons_copy_exact_text_and_keep_prose_links_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let source_code = format!("{}let π = 7;\n", "// waiting\n".repeat(78));
    let source = format!("[源码](workspace-file:src/lib.rs:7)\n\n```rust\n{source_code}```");
    let body = StyledText::from_markdown(&source)?;
    let sections = transcript_search::fenced_sections(&source, &body);
    assert_eq!(sections.len(), 2);
    let mut window = AgentWorkbench::new();
    window.model_entries().push(TranscriptEntry {
        kind: "assistant".into(),
        heading: "Codex".into(),
        status: "已完成".into(),
        status_tone: String::new(),
        body: body.clone(),
    });
    window.set_transcript_section_owners(vec!["|reply|".into()]);
    for section in &sections {
        window.model_transcript_sections().push(TranscriptSection {
            owner: "|reply|".into(),
            start: section.start as i64,
            language: section.language.clone(),
            annotated: section.body.clone(),
            body: section.body.clone(),
        });
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let copied = calls.clone();
    window.on_copy_code_section(move |index, start, code| {
        copied
            .borrow_mut()
            .push(format!("{index}:{start}:{}", code.text()));
    });
    let opened = calls.clone();
    window.on_open_workspace_reference(move |href| opened.borrow_mut().push(href));
    exercise_code_section(window, &calls, &source_code);
    calls.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    let row = RecordValue::new(
        "TranscriptEntry",
        [
            ("kind".into(), "assistant".to_owned().into()),
            ("heading".into(), "Codex".to_owned().into()),
            ("status".into(), "已完成".to_owned().into()),
            ("status-tone".into(), String::new().into()),
            ("body".into(), Value::StyledText(body)),
        ]
        .into(),
    );
    view.set(
        "entries",
        Value::RecordList(RecordList::new("TranscriptEntry", vec![row])),
    )?;
    view.set(
        "transcript-section-owners",
        Value::StringList(vec!["|reply|".into()]),
    )?;
    view.set(
        "transcript-sections",
        Value::RecordList(RecordList::new(
            "TranscriptSection",
            sections
                .into_iter()
                .map(|section| {
                    RecordValue::new(
                        "TranscriptSection",
                        [
                            ("owner".into(), "|reply|".to_owned().into()),
                            ("start".into(), (section.start as i64).into()),
                            ("language".into(), section.language.into()),
                            ("annotated".into(), Value::StyledText(section.body.clone())),
                            ("body".into(), Value::StyledText(section.body)),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let copied = calls.clone();
    view.on_mut("copy-code-section", move |args| {
        let [
            Value::Int(index),
            Value::Int(start),
            Value::StyledText(code),
        ] = args
        else {
            return Err("expected code section".into());
        };
        copied
            .borrow_mut()
            .push(format!("{index}:{start}:{}", code.text()));
        Ok(None)
    })?;
    let opened = calls.clone();
    view.on_mut("open-workspace-reference", move |args| {
        let [Value::String(href)] = args else {
            return Err("expected workspace reference".into());
        };
        opened.borrow_mut().push(href.clone());
        Ok(None)
    })?;
    exercise_code_section(view, &calls, &source_code);
    Ok(())
}

fn reference_document(label: &str, path: &str) -> StyledText {
    StyledText::from_markdown(&format!("[{label}](workspace-file:{path})")).unwrap()
}

fn exercise_transcript_references<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    replace_first: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    let reference_path = |scene: &Node, label| {
        visible_path(scene, &|node| {
            node.kind == "StyledText" && node.text("text") == Some(label)
        })
        .expect("transcript reference")
    };
    let first = reference_path(ui.scene(), "计数器实现");
    let (_, rect) = ui
        .layout_at(&first)
        .unwrap()
        .text_link_regions()
        .next()
        .unwrap();
    let mut input = Interaction::default();
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        },
    );
    input.pointer_press_in_scene(ui.scene(), ui.layout());
    let click = input.control_part_release(ui.scene(), ui.layout()).unwrap();
    assert_eq!(click.path, first);
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &first, click.action)));
    assert!(ui.node("workspace-files-panel").is_visible());

    let second = reference_path(ui.scene(), "布局实现");
    input.focused = input.text_link_target(ui.scene(), ui.layout(), &second, 0);
    assert!(input.focused.is_some());
    input.key_press_with_scene(ui.scene(), ActivationKey::Enter);
    let target = input
        .key_release_with_scene(ui.scene(), ActivationKey::Enter)
        .unwrap();
    let enter = input
        .text_link_activation(ui.scene(), ui.layout(), &target)
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &enter.path, enter.action)));

    let old_path = reference_path(ui.scene(), "计数器实现");
    let old_scene = ui.scene().clone();
    ui.update(replace_first);
    ui.update(|view| {
        assert!(!view.control_at(&old_scene, &old_path, ControlAction::ActivateTextLink(0)))
    });
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "workspace-file:src/count.rs#L1531",
            "workspace-file:src/layout.rs:12"
        ]
    );
}

#[test]
fn transcript_references_follow_their_own_row_and_reject_a_stale_document()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = [
        reference_document("计数器实现", "src/count.rs#L1531"),
        reference_document("布局实现", "src/layout.rs:12"),
    ];
    let updated = reference_document("计数器实现", "src/replacement.rs:2");
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    let row = |body| TranscriptEntry {
        kind: "assistant".into(),
        heading: "Codex".into(),
        status: String::new(),
        status_tone: String::new(),
        body,
    };
    for document in documents.clone() {
        window.model_entries().push(row(document));
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let reported = events.clone();
    let weak = window.as_weak();
    window.on_open_workspace_reference(move |href| {
        reported.borrow_mut().push(href);
        weak.upgrade().unwrap().invoke_show_workspace_files();
    });
    let updated_aot = updated.clone();
    exercise_transcript_references(window, &events, |ui| {
        assert!(ui.model_entries().set_row(0, row(updated_aot)));
    });
    events.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    let record = |body| {
        RecordValue::new(
            "TranscriptEntry",
            [
                ("kind".into(), "assistant".to_owned().into()),
                ("heading".into(), "Codex".to_owned().into()),
                ("status".into(), String::new().into()),
                ("status-tone".into(), String::new().into()),
                ("body".into(), Value::StyledText(body)),
            ]
            .into(),
        )
    };
    let rows = RecordList::new(
        "TranscriptEntry",
        documents.into_iter().map(record).collect(),
    );
    instance.set("entries", Value::RecordList(rows.clone()))?;
    let reported = events.clone();
    let weak = instance.as_weak();
    instance.on_mut("open-workspace-reference", move |values| {
        let [Value::String(href)] = values else {
            return Err("引用无效".into());
        };
        reported.borrow_mut().push(href.clone());
        let ui = weak.upgrade().unwrap();
        ui.set("review-open", true.into())
            .map_err(|error| error.to_string())?;
        ui.set("files-active", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_transcript_references(instance, &events, |_| {
        assert!(rows.model().set_row(0, record(updated)));
    });
    Ok(())
}

fn exercise_external_open<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    finish: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("external-open-project");
    assert_eq!(
        ui.node("external-open-location").text("text"),
        Some("/workspace")
    );
    assert!(ui.node("external-open-dialog").boolean("open", false));
    ui.click("external-cursor");
    ui.click("external-remember-project");
    ui.click("confirm-external-open-button");
    assert!(!ui.can_hit("confirm-external-open-button"));
    ui.update(finish);
    ui.click("cancel-external-open");
    assert!(!ui.node("external-open-dialog").boolean("open", false));
    assert_eq!(calls.borrow().as_slice(), ["project", "cursor:true"]);
}

#[test]
fn external_open_routes_destination_and_project_preference_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_present_external(move |origin, path, line, column| {
        assert!(path.is_empty() && line == 0 && column == 0);
        output.borrow_mut().push(origin);
        let mut ui = weak.upgrade().unwrap();
        ui.set_external_open_path("/workspace".into());
        ui.set_external_can_remember(true);
        ui.set_external_open(true);
    });
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_confirm_external_open(move || {
        let mut ui = weak.upgrade().unwrap();
        output.borrow_mut().push(format!(
            "{}:{}",
            ui.get_external_destination(),
            ui.get_external_remember()
        ));
        ui.set_external_open_pending(true);
    });
    exercise_external_open(window, &calls, |view| view.set_external_open_pending(false));
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("present-external", move |args| {
        let [
            Value::String(origin),
            Value::String(path),
            Value::Int(0),
            Value::Int(0),
        ] = args
        else {
            return Err("invalid open target".into());
        };
        assert!(path.is_empty());
        output.borrow_mut().push(origin.clone());
        let ui = weak.upgrade().unwrap();
        for (name, value) in [
            ("external-open-path", "/workspace".to_owned().into()),
            ("external-can-remember", true.into()),
            ("external-open", true.into()),
        ] {
            ui.set(name, value).map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("confirm-external-open", move |_| {
        let ui = weak.upgrade().unwrap();
        let destination = ui
            .get("external-destination")
            .map_err(|error| error.to_string())?;
        let remember = ui
            .get("external-remember")
            .map_err(|error| error.to_string())?;
        if let (Value::String(destination), Value::Bool(remember)) = (destination, remember) {
            output
                .borrow_mut()
                .push(format!("{destination}:{remember}"));
        }
        ui.set("external-open-pending", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_external_open(instance, &calls, |view| {
        view.set("external-open-pending", false.into()).unwrap()
    });
    Ok(())
}

fn exercise_workspace_files<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("open-files");
    assert!(ui.node("workspace-files-panel").is_visible());
    assert!(!ui.node("workspace-changes-panel").is_visible());
    let path = ui.path("workspace-tree");
    let mut row = path.clone();
    row.push(0);
    let geometry = TreeRowGeometry::new(
        ui.scene().at_path(&row).unwrap(),
        ui.layout_at(&row).unwrap().bounds,
    )
    .unwrap();
    let mut input = Interaction::default();
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: geometry.indicator.x + geometry.indicator.width * 0.5,
            y: geometry.indicator.y + geometry.indicator.height * 0.5,
        },
    );
    let expand = input
        .tree_pointer_action(ui.scene(), ui.layout(), input.hovered.as_ref().unwrap())
        .expect("directory disclosure should be clickable");
    assert_eq!(expand.path, path);
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &path, expand.action)));
    let label = visible_path(ui.scene(), &|node| {
        node.kind == "TreeCell" && node.text("text") == Some("main.rs")
    })
    .expect("expanded directory should show the file name");
    let bounds = ui.layout_at(&label).expect("file name layout").bounds;
    assert!(bounds.width >= 20.0 && bounds.height >= 18.0, "{bounds:?}");
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &path, ControlAction::Select(1))));
    assert!(has_visible_output(ui.scene(), "   1  fn main() {}"));
    ui.click("workspace-file-preview-document");
    select_all_context_text(&mut ui);
    assert!(ui.can_hit("attach-preview-excerpt"));
    ui.click("attach-preview-excerpt");
    ui.click("external-open-file");
    ui.click("workspace-changes-tab");
    assert!(ui.node("workspace-changes-panel").is_visible());
    assert!(!ui.node("workspace-files-panel").is_visible());
    ui.click("external-open-diff");
    ui.click("inspect-diff-source");
    assert!(ui.node("workspace-files-panel").is_visible());
    assert!(has_visible_output(ui.scene(), "   1  fn main() {}"));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "load",
            "expand:1",
            "open:2",
            "preview:0:18:   1  fn main() {}",
            "external:file",
            "changes",
            "external:diff",
            "load",
            "diff:src/main.rs"
        ]
    );
}

#[test]
fn workspace_file_tree_and_diff_source_use_the_same_preview_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let nodes = [
        (1, -1, "src", "src", "directory"),
        (2, 1, "src/main.rs", "main.rs", "file"),
    ];
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_selection("src/main.rs".into());
    for (id, parent_id, path, name, kind) in nodes {
        window.model_workspace_files().push(WorkspaceFile {
            id,
            parent_id,
            path: path.into(),
            name: name.into(),
            kind: kind.into(),
        });
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    window.on_load_workspace_files(move || output.borrow_mut().push("load".into()));
    let output = calls.clone();
    window.on_expand_workspace_directory(move |key| {
        output.borrow_mut().push(format!("expand:{key}"))
    });
    let output = calls.clone();
    window.on_refresh_review(move || output.borrow_mut().push("changes".into()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_open_workspace_selection(move |key| {
        output.borrow_mut().push(format!("open:{key}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_workspace_file_path("src/main.rs".into());
        ui.set_workspace_file_kind("text".into());
        ui.set_workspace_file_text(StyledText::from_plain_text("   1  fn main() {}"));
    });
    let output = calls.clone();
    window.on_preview_diff_source(move |path| output.borrow_mut().push(format!("diff:{path}")));
    let output = calls.clone();
    window.on_attach_workspace_preview_excerpt(move |start, end, text| {
        output
            .borrow_mut()
            .push(format!("preview:{start}:{end}:{text}"));
    });
    let output = calls.clone();
    window.on_open_preview_external(move || output.borrow_mut().push("external:file".into()));
    let output = calls.clone();
    window.on_open_diff_external(move || output.borrow_mut().push("external:diff".into()));
    exercise_workspace_files(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-selection", "src/main.rs".to_owned().into())?;
    instance.set(
        "workspace-files",
        Value::RecordList(RecordList::new(
            "WorkspaceFile",
            nodes
                .into_iter()
                .map(|(id, parent, path, name, kind)| {
                    RecordValue::new(
                        "WorkspaceFile",
                        [
                            ("id".into(), id.into()),
                            ("parent-id".into(), parent.into()),
                            ("path".into(), path.to_owned().into()),
                            ("name".into(), name.to_owned().into()),
                            ("kind".into(), kind.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    for (callback, label) in [
        ("load-workspace-files", "load"),
        ("refresh-review", "changes"),
        ("open-preview-external", "external:file"),
        ("open-diff-external", "external:diff"),
    ] {
        let output = calls.clone();
        instance.on_mut(callback, move |_| {
            output.borrow_mut().push(label.into());
            Ok(None)
        })?;
    }
    let output = calls.clone();
    instance.on_mut("expand-workspace-directory", move |args| {
        let [Value::Int(key)] = args else {
            return Err("目录标识无效".into());
        };
        output.borrow_mut().push(format!("expand:{key}"));
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("open-workspace-selection", move |args| {
        let [Value::Int(key)] = args else {
            return Err("文件标识无效".into());
        };
        output.borrow_mut().push(format!("open:{key}"));
        let instance = weak.upgrade().unwrap();
        instance
            .set("workspace-file-path", "src/main.rs".to_owned().into())
            .map_err(|error| error.to_string())?;
        instance
            .set("workspace-file-kind", "text".to_owned().into())
            .map_err(|error| error.to_string())?;
        instance
            .set(
                "workspace-file-text",
                Value::StyledText(StyledText::from_plain_text("   1  fn main() {}")),
            )
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("preview-diff-source", move |args| {
        let [Value::String(path)] = args else {
            return Err("文件路径无效".into());
        };
        output.borrow_mut().push(format!("diff:{path}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("attach-workspace-preview-excerpt", move |args| {
        let [Value::Int(start), Value::Int(end), Value::String(text)] = args else {
            return Err("选段参数无效".into());
        };
        output
            .borrow_mut()
            .push(format!("preview:{start}:{end}:{text}"));
        Ok(None)
    })?;
    exercise_workspace_files(instance, &calls);
    Ok(())
}

fn exercise_sidebar_session_menu<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(960.0, 680.0));
    assert!(ui.context_menu("conversation-menu-area"));
    assert!(ui.click_menu_row("重命名"));
    assert!(ui.node("sidebar-rename-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("sidebar-rename-field"));
    ui.fill_focused("新的后台会话名称");
    ui.click("save-sidebar-rename");
    assert!(!ui.node("sidebar-rename-dialog").boolean("open", false));
    assert!(ui.context_menu("conversation-menu-area"));
    assert!(ui.click_menu_row("置顶"));
    assert!(ui.context_menu("conversation-menu-area"));
    assert!(ui.click_menu_row("归档会话"));
    assert_eq!(ui.node("input").text("text"), Some("保留的草稿"));
    assert_eq!(
        calls.borrow().as_slice(),
        ["rename:1", "save:新的后台会话名称", "pin:1", "archive:1"]
    );
}

#[test]
fn sidebar_context_menu_targets_row_without_switching_the_composer_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace/xrack".into());
    window.set_connected(true);
    window.set_draft("保留的草稿".into());
    window.model_conversations().push(ConversationRow {
        id: 1,
        title: "另一个会话".into(),
        shown: true,
        ..ConversationRow::default()
    });
    window
        .model_sidebar_menus()
        .push(SidebarMenuState::default());
    window.model_sidebar_menus().push(SidebarMenuState {
        rename: true,
        archive: true,
        pin: true,
        ..SidebarMenuState::default()
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_manage_sidebar_session(move |id, action| {
        output.borrow_mut().push(format!("{action}:{id}"));
        if action == "rename" {
            let mut ui = weak.upgrade().unwrap();
            ui.set_sidebar_rename_draft("另一个会话".into());
            ui.invoke_show_sidebar_rename();
        }
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_rename_sidebar_session(move |name| {
        output.borrow_mut().push(format!("save:{name}"));
        weak.upgrade().unwrap().set_sidebar_rename_open(false);
    });
    window.on_select_conversation(|_| panic!("菜单不应切换会话"));
    exercise_sidebar_session_menu(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace/xrack".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("draft", "保留的草稿".to_owned().into())?;
    instance.set(
        "conversations",
        Value::RecordList(RecordList::new(
            "ConversationRow",
            vec![RecordValue::new(
                "ConversationRow",
                [
                    ("id".into(), 1i64.into()),
                    ("title".into(), "另一个会话".to_owned().into()),
                    ("folder".into(), String::new().into()),
                    ("activity".into(), String::new().into()),
                    ("shown".into(), true.into()),
                    ("pinned".into(), false.into()),
                    ("pinned-shown".into(), false.into()),
                ]
                .into(),
            )],
        )),
    )?;
    let menu = |enabled| {
        RecordValue::new(
            "SidebarMenuState",
            [
                ("pinned".into(), false.into()),
                ("archived".into(), false.into()),
                ("rename".into(), Value::Bool(enabled)),
                ("pin".into(), Value::Bool(enabled)),
                ("archive".into(), Value::Bool(enabled)),
            ]
            .into(),
        )
    };
    instance.set(
        "sidebar-menus",
        Value::RecordList(RecordList::new(
            "SidebarMenuState",
            vec![menu(false), menu(true)],
        )),
    )?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("manage-sidebar-session", move |args| {
        let [Value::Int(id), Value::String(action)] = args else {
            return Err("会话菜单参数无效".into());
        };
        output.borrow_mut().push(format!("{action}:{id}"));
        if action == "rename" {
            let ui = weak.upgrade().unwrap();
            ui.set("sidebar-rename-draft", "另一个会话".to_owned().into())
                .map_err(|error| error.to_string())?;
            ui.invoke("show-sidebar-rename", &[])
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("rename-sidebar-session", move |args| {
        let [Value::String(name)] = args else {
            return Err("会话名称无效".into());
        };
        output.borrow_mut().push(format!("save:{name}"));
        weak.upgrade()
            .unwrap()
            .set("sidebar-rename-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    instance.on_mut("select-conversation", |_| panic!("菜单不应切换会话"))?;
    exercise_sidebar_session_menu(instance, &calls);
    Ok(())
}

fn exercise_project_search<C: Component>(
    component: C,
    ready: impl FnOnce(&mut C),
    calls: &RefCell<Vec<String>>,
) {
    let mut ui = UiDriver::headless(component, Size::new(960.0, 680.0));
    ui.click("project-search-field");
    ui.fill_focused("  build  ");
    ui.advance_time(Duration::from_millis(179));
    assert!(calls.borrow().is_empty());
    ui.advance_time(Duration::from_millis(2));
    assert_eq!(calls.borrow().as_slice(), ["search:build:false:false"]);
    assert!(!ui.node("project-search-results").is_visible());
    ui.update(ready);
    let list = ui.node("project-search-results");
    assert!(list.is_visible());
    let key = list.children[0].row_key.unwrap();
    let row = ui
        .find_row_path("project-search-results", key, Some("conversation-action"))
        .unwrap();
    ui.click_at_path(&row);
    ui.click("more-project-search");
    ui.click("project-search-archived-button");
    assert!(!ui.node("project-search-results").is_visible());
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "search:build:false:false",
            "choose:25",
            "search:build:false:true",
            "search:build:true:false"
        ]
    );
    ui.click("project-search-field");
    ui.fill_focused("");
    assert!(ui.node("project-group-tree").is_visible());
}

#[test]
fn grouped_sidebar_search_debounces_and_routes_archive_and_pagination_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_connected(true);
    window.set_workspace("/workspace".into());
    window.set_project_groups_open(true);
    let output = calls.clone();
    window.on_search_all_projects(move |query, archived, more| {
        output
            .borrow_mut()
            .push(format!("search:{query}:{archived}:{more}"));
    });
    let output = calls.clone();
    window.on_select_conversation(move |id| output.borrow_mut().push(format!("choose:{id}")));
    exercise_project_search(
        window,
        |ui| {
            ui.set_project_search_query("build".into());
            ui.set_project_search_more(true);
            ui.model_project_search_results().push(ConversationRow {
                id: 25,
                title: "Build workflow".into(),
                folder: "other · 工作树".into(),
                shown: true,
                ..ConversationRow::default()
            });
        },
        &calls,
    );
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("connected", true.into())?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("project-groups-open", true.into())?;
    let output = calls.clone();
    instance.on_mut("search-all-projects", move |args| {
        let [
            Value::String(query),
            Value::Bool(archived),
            Value::Bool(more),
        ] = args
        else {
            return Err("搜索参数无效".into());
        };
        output
            .borrow_mut()
            .push(format!("search:{query}:{archived}:{more}"));
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("select-conversation", move |args| {
        let [Value::Int(id)] = args else {
            return Err("会话 ID 无效".into());
        };
        output.borrow_mut().push(format!("choose:{id}"));
        Ok(None)
    })?;
    exercise_project_search(
        instance,
        |ui| {
            ui.set("project-search-query", "build".to_owned().into())
                .unwrap();
            ui.set("project-search-more", true.into()).unwrap();
            ui.set(
                "project-search-results",
                Value::RecordList(RecordList::new(
                    "ConversationRow",
                    vec![RecordValue::new(
                        "ConversationRow",
                        [
                            ("id".into(), 25i64.into()),
                            ("title".into(), "Build workflow".to_owned().into()),
                            ("folder".into(), "other · 工作树".to_owned().into()),
                            ("activity".into(), String::new().into()),
                            ("shown".into(), true.into()),
                            ("pinned".into(), false.into()),
                            ("pinned-shown".into(), false.into()),
                        ]
                        .into(),
                    )],
                )),
            )
            .unwrap();
        },
        &calls,
    );
    Ok(())
}

fn exercise_project_groups<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(960.0, 680.0));
    ui.click("project-groups-toggle");
    assert!(ui.node("project-group-tree").is_visible());
    assert_eq!(ui.focused(), Some("project-group-tree"));
    assert_eq!(ui.node("project-group-tree").children.len(), 2);
    let key = ui.node("project-group-tree").children[1].row_key.unwrap();
    let row = ui.find_row_path("project-group-tree", key, None).unwrap();
    let geometry = TreeRowGeometry::new(
        ui.scene().at_path(&row).unwrap(),
        ui.layout_at(&row).unwrap().bounds,
    )
    .unwrap();
    let mut input = Interaction::default();
    input.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: geometry.indicator.x + geometry.indicator.width * 0.5,
            y: geometry.indicator.y + geometry.indicator.height * 0.5,
        },
    );
    let expand = input
        .tree_pointer_action(ui.scene(), ui.layout(), input.hovered.as_ref().unwrap())
        .unwrap();
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &expand.path, expand.action)));
    assert_eq!(ui.node("project-group-tree").children.len(), 3);
    let menu = visible_path(ui.scene(), &|node| {
        node.text("accessible-name") == Some("操作会话 构建脚本检查")
    })
    .unwrap();
    ui.click_at_path(&menu);
    assert!(ui.node("sidebar-session-dialog").boolean("open", false));
    ui.click("sidebar-menu-pin");
    let tree = ui.path("project-group-tree");
    let scene = ui.scene().clone();
    ui.update(|view| assert!(view.control_at(&scene, &tree, ControlAction::Select(2))));
    assert_eq!(
        calls.borrow().as_slice(),
        ["show", "expand:8", "manage:42:pin", "activate:85"]
    );
    ui.click("project-groups-toggle");
    assert!(!ui.node("project-group-tree").is_visible());
    assert!(ui.can_hit("session-search"));
}

#[test]
fn grouped_sidebar_expands_project_history_and_activates_the_chosen_row_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let projects = [("/workspace/first", "First"), ("/workspace/other", "Other")];
    let rows = [
        ProjectGroupRow {
            key: 4,
            parent: -1,
            title: "First".into(),
            kind: "project".into(),
            ..ProjectGroupRow::default()
        },
        ProjectGroupRow {
            key: 8,
            parent: -1,
            title: "Other".into(),
            kind: "project".into(),
            ..ProjectGroupRow::default()
        },
        ProjectGroupRow {
            key: 85,
            parent: 8,
            title: "构建脚本检查".into(),
            kind: "thread".into(),
            conversation: 42,
            ..ProjectGroupRow::default()
        },
    ];
    let mut window = AgentWorkbench::new();
    window.set_workspace(projects[0].0.into());
    window.set_connected(true);
    for (key, label) in projects {
        window.model_project_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    for row in &rows {
        window.model_project_groups().push(row.clone());
    }
    for id in 0..=42 {
        window.model_sidebar_menus().push(SidebarMenuState {
            pin: id == 42,
            ..SidebarMenuState::default()
        });
    }
    let output = calls.clone();
    window.on_manage_sidebar_session(move |id, action| {
        output.borrow_mut().push(format!("manage:{id}:{action}"))
    });
    let output = calls.clone();
    window.on_show_project_groups(move || output.borrow_mut().push("show".into()));
    let output = calls.clone();
    window.on_expand_project_group(move |key| output.borrow_mut().push(format!("expand:{key}")));
    let output = calls.clone();
    window.on_activate_project_group_row(move |key| {
        output.borrow_mut().push(format!("activate:{key}"))
    });
    exercise_project_groups(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("connected", true.into())?;
    instance.set("workspace", projects[0].0.to_owned().into())?;
    instance.set(
        "project-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            projects
                .map(|(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .to_vec(),
        )),
    )?;
    instance.set(
        "project-groups",
        Value::RecordList(RecordList::new(
            "ProjectGroupRow",
            rows.iter()
                .map(|row| {
                    RecordValue::new(
                        "ProjectGroupRow",
                        [
                            ("key".into(), row.key.into()),
                            ("parent".into(), row.parent.into()),
                            ("title".into(), row.title.clone().into()),
                            ("detail".into(), row.detail.clone().into()),
                            ("kind".into(), row.kind.clone().into()),
                            ("project".into(), row.project.clone().into()),
                            ("conversation".into(), row.conversation.into()),
                            ("selected".into(), row.selected.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = calls.clone();
    instance.on_mut("show-project-groups", move |_| {
        output.borrow_mut().push("show".into());
        Ok(None)
    })?;
    for (callback, action) in [
        ("expand-project-group", "expand"),
        ("activate-project-group-row", "activate"),
    ] {
        let output = calls.clone();
        instance.on_mut(callback, move |args| {
            let [Value::Int(key)] = args else {
                return Err("项目行编号无效".into());
            };
            output.borrow_mut().push(format!("{action}:{key}"));
            Ok(None)
        })?;
    }
    instance.set(
        "sidebar-menus",
        Value::RecordList(RecordList::new(
            "SidebarMenuState",
            (0..=42)
                .map(|id| {
                    RecordValue::new(
                        "SidebarMenuState",
                        [
                            ("pinned".into(), false.into()),
                            ("archived".into(), false.into()),
                            ("pin".into(), (id == 42).into()),
                            ("rename".into(), false.into()),
                            ("archive".into(), false.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = calls.clone();
    instance.on_mut("manage-sidebar-session", move |args| {
        let [Value::Int(id), Value::String(action)] = args else {
            return Err("会话菜单参数无效".into());
        };
        output.borrow_mut().push(format!("manage:{id}:{action}"));
        Ok(None)
    })?;
    exercise_project_groups(instance, &calls);
    Ok(())
}

fn exercise_project_switch<C: Component>(component: C, selected: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(720.0, 570.0));
    assert!(ui.can_hit("reset"));
    assert!(
        visible_path(ui.scene(), &|node| {
            node.text("accessible-name") == Some("检查布局 · 运行中")
        })
        .is_some()
    );
    assert_eq!(combo_text(ui.node("project-choice")), "XRack");
    ui.click("project-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert_eq!(selected.borrow().as_slice(), ["/workspace/tools"]);
    assert_eq!(combo_text(ui.node("project-choice")), "Tools");
    assert!(!ui.can_hit("project-choice"));
    assert!(!ui.can_hit("open-folder"));
}

#[test]
fn project_switcher_works_during_a_turn_and_waits_for_send_ack_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let choices = [("/workspace/xrack", "XRack"), ("/workspace/tools", "Tools")];
    let mut window = AgentWorkbench::new();
    window.set_workspace(choices[0].0.into());
    window.set_busy(true);
    window.model_conversations().push(ConversationRow {
        id: 7,
        title: "检查布局".into(),
        folder: String::new(),
        activity: "running".into(),
        pinned: false,
        pinned_shown: false,
        shown: true,
    });
    window.set_project_selection(choices[0].0.into());
    for (key, label) in choices {
        window.model_project_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    let selected = Rc::new(RefCell::new(Vec::new()));
    let output = selected.clone();
    let weak = window.as_weak();
    window.on_select_workspace(move |path| {
        output.borrow_mut().push(path);
        weak.upgrade().unwrap().set_sending_prompt(true);
    });
    exercise_project_switch(window, &selected);
    selected.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", choices[0].0.to_owned().into())?;
    instance.set("busy", true.into())?;
    instance.set(
        "conversations",
        Value::RecordList(RecordList::new(
            "ConversationRow",
            vec![RecordValue::new(
                "ConversationRow",
                [
                    ("id".into(), 7i64.into()),
                    ("title".into(), "检查布局".to_owned().into()),
                    ("folder".into(), String::new().into()),
                    ("activity".into(), "running".to_owned().into()),
                    ("shown".into(), true.into()),
                    ("pinned".into(), false.into()),
                    ("pinned-shown".into(), false.into()),
                ]
                .into(),
            )],
        )),
    )?;
    instance.set("project-selection", choices[0].0.to_owned().into())?;
    instance.set(
        "project-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            choices
                .into_iter()
                .map(|(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = selected.clone();
    let weak = instance.as_weak();
    instance.on_mut("select-workspace", move |args| {
        let [Value::String(path)] = args else {
            return Err("项目路径无效".into());
        };
        output.borrow_mut().push(path.clone());
        weak.upgrade()
            .unwrap()
            .set("sending-prompt", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_project_switch(instance, &selected);
    Ok(())
}

fn exercise_command_group<C: Component>(
    component: C,
    reveal_match: impl FnOnce(&mut C),
    mark_finished: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1000.0, 700.0));
    let button = |caption: &'static str| {
        move |node: &Node| node.kind == "Button" && node.text("text") == Some(caption)
    };
    let summary = |ui: &UiDriver<C>| {
        visible_path(ui.scene(), &|node| {
            node.kind == "Button"
                && node
                    .text("text")
                    .is_some_and(|text| text.replace(['\u{2068}', '\u{2069}'], "") == "2 条命令")
        })
        .unwrap()
    };
    let name = |ui: &UiDriver<C>, path: &[usize]| {
        ui.scene()
            .at_path(path)
            .unwrap()
            .text("accessible-name")
            .unwrap()
            .replace(['\u{2068}', '\u{2069}'], "")
    };
    assert_eq!(name(&ui, &summary(&ui)), "展开 2 条命令 · 1 进行中");
    assert!(visible_path(ui.scene(), &button("bazel test")).is_none());
    assert!(has_visible_output(ui.scene(), "接着说明测试结果"));
    let group = summary(&ui);
    ui.click_at_path(&group);
    let test = visible_path(ui.scene(), &button("bazel test")).unwrap();
    ui.click_at_path(&test);
    assert!(has_visible_output(ui.scene(), "详细日志"));
    ui.update(mark_finished);
    assert!(name(&ui, &summary(&ui)).contains("1 失败"));
    assert!(has_visible_output(ui.scene(), "详细日志"));
    let group = summary(&ui);
    ui.click_at_path(&group);
    assert!(!has_visible_output(ui.scene(), "详细日志"));
    assert!(ui.activate(ActivationKey::Space));
    assert!(visible_path(ui.scene(), &button("bazel test")).is_some());
    assert!(ui.activate(ActivationKey::Space));
    assert!(visible_path(ui.scene(), &button("bazel test")).is_none());

    ui.update(reveal_match);
    assert_eq!(name(&ui, &summary(&ui)), "折叠 2 条命令 · 1 失败");
    assert!(has_visible_output(ui.scene(), "详细日志"));
}

#[test]
fn adjacent_commands_fold_and_search_reveals_the_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let entries = [
        ("command", "bazel build", "已完成", "done", "构建通过"),
        ("command", "bazel test", "进行中", "running", "详细日志"),
        ("assistant", "Codex", "", "", "接着说明测试结果"),
    ];
    let group = CommandGroup {
        first: 0,
        last: 1,
        status: "1 进行中".into(),
        status_tone: "running".into(),
    };
    let marker = CommandGroup {
        first: 0,
        last: 1,
        ..CommandGroup::default()
    };
    let failed = CommandGroup {
        status: "1 失败".into(),
        status_tone: "error".into(),
        ..group.clone()
    };
    let records = |groups: &[CommandGroup]| {
        Value::RecordList(RecordList::new(
            "CommandGroup",
            groups
                .iter()
                .map(|group| {
                    RecordValue::new(
                        "CommandGroup",
                        [
                            ("first".into(), group.first.into()),
                            ("last".into(), group.last.into()),
                            ("status".into(), group.status.clone().into()),
                            ("status-tone".into(), group.status_tone.clone().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        ))
    };
    let window = AgentWorkbench::new();
    for (kind, heading, status, tone, body) in entries {
        window.model_entries().push(TranscriptEntry {
            kind: kind.into(),
            heading: heading.into(),
            status: status.into(),
            status_tone: tone.into(),
            body: StyledText::from_plain_text(body),
        });
    }
    window.model_command_groups().push(group.clone());
    window.model_command_groups().push(marker.clone());
    exercise_command_group(
        window,
        |ui| ui.set_expanded_command(1),
        |ui| {
            ui.model_command_groups().set_row(0, failed.clone());
        },
    );

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            entries
                .iter()
                .map(|&(kind, heading, status, tone, body)| {
                    RecordValue::new(
                        "TranscriptEntry",
                        [
                            ("kind".into(), kind.to_owned().into()),
                            ("heading".into(), heading.to_owned().into()),
                            ("status".into(), status.to_owned().into()),
                            ("status-tone".into(), tone.to_owned().into()),
                            ("body".into(), StyledText::from_plain_text(body).into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    instance.set("command-groups", records(&[group, marker.clone()]))?;
    exercise_command_group(
        instance,
        |ui| {
            ui.set("expanded-command", 1i64.into()).unwrap();
        },
        |ui| {
            ui.set("command-groups", records(&[failed, marker]))
                .unwrap();
        },
    );
    Ok(())
}

fn exercise_activity<C: Component>(component: C, finish_command: impl FnOnce(&mut C)) {
    let mut ui = UiDriver::headless(component, Size::new(930.0, 580.0));
    assert!(ui.can_hit("earlier-messages"));
    ui.click("earlier-messages");
    let disclosure =
        |node: &Node| node.kind == "Button" && node.text("text") == Some("ls examples");
    let button = visible_path(ui.scene(), &disclosure).expect("command button");
    assert!(
        ui.scene()
            .at_path(&button)
            .unwrap()
            .text("accessible-name")
            .unwrap()
            .contains("进行中")
    );
    assert!(!has_visible_output(ui.scene(), "initial"));
    ui.click_at_path(&button);
    assert!(has_visible_output(ui.scene(), "initial"));
    ui.update(finish_command);
    assert!(has_visible_output(ui.scene(), "final output"));
    let button = visible_path(ui.scene(), &disclosure).expect("command button");
    assert!(
        ui.scene()
            .at_path(&button)
            .unwrap()
            .text("accessible-name")
            .unwrap()
            .contains("已完成")
    );
    assert!(ui.activate(ActivationKey::Space));
    assert!(!has_visible_output(ui.scene(), "final output"));

    let changes = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("本轮改动")
    })
    .expect("turn review button");
    ui.click_at_path(&changes);
    assert!(ui.node("review-panel").is_visible());
    assert_eq!(ui.node("review-title").text("text"), Some("本轮改动"));
    assert!(!ui.node("refresh-review-button").is_visible());
    ui.click("workspace-review-button");
    assert_eq!(ui.node("review-title").text("text"), Some("工作区改动"));
    assert!(ui.node("refresh-review-button").is_visible());
}

#[test]
fn activity_details_keep_focus_and_expansion_during_streaming()
-> Result<(), Box<dyn std::error::Error>> {
    let row = |status: &str, status_tone: &str, output: &str| TranscriptEntry {
        kind: "command".into(),
        heading: "ls examples".into(),
        status: status.into(),
        status_tone: status_tone.into(),
        body: StyledText::from_plain_text(output),
    };
    let mut window = AgentWorkbench::new();
    window.set_connected(true);
    window.set_earlier_turns(true);
    let earlier = Rc::new(RefCell::new(0));
    let calls = earlier.clone();
    window.on_load_earlier_turns(move || *calls.borrow_mut() += 1);
    let changes = || TranscriptEntry {
        kind: "changes".into(),
        heading: "本轮改动".into(),
        status: "2 个文件 · +5 −1".into(),
        status_tone: String::new(),
        body: StyledText::from_plain_text(""),
    };
    window
        .model_entries()
        .push(row("进行中", "running", "initial"));
    window.model_entries().push(changes());
    exercise_activity(window, |ui| {
        assert!(
            ui.model_entries()
                .set_row(0, row("已完成", "done", "final output"))
        );
    });
    assert_eq!(*earlier.borrow(), 1);

    let program = workbench_program()?;
    let instance = program.create("AgentWorkbench")?;
    instance.set("connected", Value::Bool(true))?;
    instance.set("earlier-turns", Value::Bool(true))?;
    let earlier = Rc::new(RefCell::new(0));
    let calls = earlier.clone();
    instance.on_mut("load-earlier-turns", move |_| {
        *calls.borrow_mut() += 1;
        Ok(None)
    })?;
    let record = |row: TranscriptEntry| {
        RecordValue::new(
            "TranscriptEntry",
            [
                ("kind".into(), row.kind.into()),
                ("heading".into(), row.heading.into()),
                ("status".into(), row.status.into()),
                ("status-tone".into(), row.status_tone.into()),
                ("body".into(), Value::StyledText(row.body)),
            ]
            .into(),
        )
    };
    let model = RecordList::new(
        "TranscriptEntry",
        vec![
            record(row("进行中", "running", "initial")),
            record(changes()),
        ],
    );
    instance.set("entries", Value::RecordList(model.clone()))?;
    exercise_activity(instance, |_| {
        assert!(
            model
                .model()
                .set_row(0, record(row("已完成", "done", "final output")))
        );
    });
    assert_eq!(*earlier.borrow(), 1);
    Ok(())
}

fn exercise_split_review<C: Component>(component: C, hits: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(760.0, 600.0));
    assert!(ui.node("review-line-list").is_visible());
    ui.click("split-patch");
    assert!(ui.node("review-pair-list").is_visible());
    let panel_width = ui.bounds("review-panel").width;
    assert!(panel_width >= 759.0);
    let mut gutter_positions = Vec::new();
    for name in ["批注旧行 5", "批注新行 8"] {
        let position = visible_path(ui.scene(), &|node| {
            node.kind == "Button" && node.text("accessible-name") == Some(name)
        })
        .unwrap();
        gutter_positions.push(ui.layout_at(&position).unwrap().bounds.x);
        ui.click_at_path(&position);
    }
    assert!((gutter_positions[1] - gutter_positions[0] - panel_width / 2.0).abs() <= 2.0);
    let hunk = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("暂存这块")
    })
    .unwrap();
    ui.click_at_path(&hunk);
    assert_eq!(
        hits.borrow().as_slice(),
        ["line:1:true", "line:1:false", "hunk:0"]
    );
    assert!(ui.menu_shortcut("Primary+F".parse().unwrap()));
    assert_eq!(ui.focused(), Some("review-find-input"));
    assert!(ui.node("review-find-bar").is_visible());
    assert!(
        visible_path(ui.scene(), &|node| {
            node.kind == "StyledText" && node.text("text") == Some("仅换行位置")
        })
        .is_some()
    );
    assert!(ui.menu_shortcut("Primary+G".parse().unwrap()));
    assert_eq!(hits.borrow().last().map(String::as_str), Some("find:1"));
    ui.click("raw-patch");
    assert!(ui.node("patch-scroll").is_visible());
    assert_eq!(ui.node("review-diff").text("text"), Some("仅换行位置"));
    ui.click("raw-patch");
    assert!(ui.node("review-pair-list").is_visible());
    ui.click("split-patch");
    assert!(ui.node("review-line-list").is_visible());
    ui.click("close-review-find-button");
    assert!(!ui.node("review-find-bar").is_visible());
    for index in 0..2 {
        if index == 1 {
            ui.click("split-patch");
            assert!(ui.node("review-pair-list").is_visible());
        }
        let edit = visible_path(ui.scene(), &|node| {
            node.kind == "Button" && node.text("accessible-name") == Some("编辑工作区新行 8")
        })
        .unwrap();
        ui.click_at_path(&edit);
        assert!(ui.node("review-line-edit-dialog").boolean("open", false));
        assert_eq!(ui.focused(), Some("review-line-edit-input"));
        assert!(!ui.can_hit("save-review-line-edit-button"));
        ui.fill_focused("修改后的行\n新增一行");
        if index == 0 {
            ui.click("save-review-line-edit-button");
        } else {
            assert!(ui.menu_shortcut("Primary+S".parse().unwrap()));
        }
        assert!(!ui.node("review-line-edit-dialog").boolean("open", false));
        assert_eq!(hits.borrow()[4 + index * 2], "edit:1");
        assert_eq!(hits.borrow()[5 + index * 2], "save:修改后的行\n新增一行");
    }
    ui.click("revert-review-file");
    assert!(ui.node("review-revert-dialog").boolean("open", false));
    assert_eq!(
        ui.node("review-revert-dialog").text("title"),
        Some("回退文件的未暂存改动")
    );
    assert_eq!(
        hits.borrow().last().map(String::as_str),
        Some("preview:file")
    );
    ui.click("cancel-review-revert");
    assert!(!ui.node("review-revert-dialog").boolean("open", false));
    assert_eq!(
        hits.borrow().last().map(String::as_str),
        Some("preview:file")
    );
    let revert = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("回退这块")
    })
    .unwrap();
    ui.click_at_path(&revert);
    assert_eq!(
        ui.node("review-revert-dialog").text("title"),
        Some("回退差异块")
    );
    assert!(ui.node("review-revert-preview-scroll").is_visible());
    assert_eq!(
        hits.borrow().last().map(String::as_str),
        Some("preview:hunk:0")
    );
    ui.click("confirm-review-revert-button");
    assert!(!ui.node("review-revert-dialog").boolean("open", false));
    assert_eq!(hits.borrow().last().map(String::as_str), Some("confirm"));
}

#[test]
fn parallel_review_uses_the_correct_side_and_original_hunk_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    const PREVIEW: &str = "@@ -5 +8 @@\n-old line\n+uncommitted line\n";
    let header = ReviewLine {
        id: 0,
        kind: "hunk".into(),
        content: "@@ -5 +8 @@".into(),
        section: "未暂存".into(),
        ..ReviewLine::default()
    };
    let line = ReviewLine {
        id: 1,
        old_line: 5,
        new_line: 8,
        kind: "context".into(),
        content: "仅换行位置".into(),
        section: "未暂存".into(),
    };
    let pairs = [
        ReviewPair {
            id: 0,
            full: true,
            left: header.clone(),
            right: ReviewLine {
                id: -1,
                ..ReviewLine::default()
            },
        },
        ReviewPair {
            id: 1,
            full: false,
            left: line.clone(),
            right: line.clone(),
        },
    ];
    let hits = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_review_selection("src/file.rs".into());
    window.set_viewport_width(Length(760.0));
    window.set_review_can_stage_hunks(true);
    let matched = StyledText::from_markdown("仅**换行**位置")?;
    window.set_review_find_query("换行".into());
    window.set_review_find_result_query("换行".into());
    window.set_review_find_count(1);
    window.set_review_find_position(1);
    window.set_review_find_raw_body(matched.clone());
    window
        .model_review_match_rows()
        .push(ReviewMatchRow::default());
    window.model_review_match_rows().push(ReviewMatchRow {
        marked: true,
        current: true,
        body: matched.clone(),
    });
    for row in [&header, &line] {
        window.model_review_lines().push(row.clone());
    }
    for pair in pairs.clone() {
        window.model_review_pairs().push(pair);
    }
    let weak = window.as_weak();
    window.on_toggle_review_split(move || {
        let mut ui = weak.upgrade().unwrap();
        ui.set_review_split(!ui.get_review_split());
    });
    let report = hits.clone();
    window
        .on_select_review_side(move |id, old| report.borrow_mut().push(format!("line:{id}:{old}")));
    let report = hits.clone();
    window.on_update_review_hunk(move |id| report.borrow_mut().push(format!("hunk:{id}")));
    let report = hits.clone();
    window.on_move_review_find(move |delta| report.borrow_mut().push(format!("find:{delta}")));
    let weak = window.as_weak();
    let report = hits.clone();
    window.on_edit_review_line(move |id| {
        report.borrow_mut().push(format!("edit:{id}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_review_line_edit_location("src/file.rs · 新行 8".into());
        ui.set_review_line_edit_original("仅换行位置".into());
        ui.set_review_line_edit_text("仅换行位置".into());
        ui.set_review_line_edit_ready(true);
        ui.set_review_line_edit_open(true);
        ui.invoke_focus_review_line_edit();
    });
    let weak = window.as_weak();
    let report = hits.clone();
    window.on_save_review_line_edit(move || {
        let mut ui = weak.upgrade().unwrap();
        report
            .borrow_mut()
            .push(format!("save:{}", ui.get_review_line_edit_text()));
        ui.set_review_line_edit_open(false);
    });
    let present = |ui: &mut AgentWorkbench, file| {
        ui.set_review_revert_file(file);
        ui.set_review_revert_path("src/file.rs".into());
        ui.set_review_revert_preview(StyledText::from_plain_text(PREVIEW));
        ui.set_review_revert_open(true);
    };
    let weak = window.as_weak();
    let report = hits.clone();
    window.on_propose_review_file_revert(move || {
        report.borrow_mut().push("preview:file".into());
        present(&mut weak.upgrade().unwrap(), true);
    });
    let weak = window.as_weak();
    let report = hits.clone();
    window.on_propose_review_hunk_revert(move |id| {
        report.borrow_mut().push(format!("preview:hunk:{id}"));
        present(&mut weak.upgrade().unwrap(), false);
    });
    let weak = window.as_weak();
    let report = hits.clone();
    window.on_confirm_review_revert(move || {
        report.borrow_mut().push("confirm".into());
        weak.upgrade().unwrap().set_review_revert_open(false);
    });
    exercise_split_review(window, &hits);
    hits.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    for (key, value) in [
        ("workspace", "/workspace".to_owned().into()),
        ("review-open", true.into()),
        ("review-selection", "src/file.rs".to_owned().into()),
        ("viewport-width", Length(760.0).into()),
        ("review-can-stage-hunks", true.into()),
        ("review-find-query", "换行".to_owned().into()),
        ("review-find-result-query", "换行".to_owned().into()),
        ("review-find-position", 1i64.into()),
        ("review-find-count", 1i64.into()),
        ("review-find-raw-body", Value::StyledText(matched.clone())),
    ] {
        view.set(key, value)?;
    }
    view.set(
        "review-match-rows",
        Value::RecordList(RecordList::new(
            "ReviewMatchRow",
            [(false, StyledText::default()), (true, matched)]
                .into_iter()
                .map(|(marked, body)| {
                    RecordValue::new(
                        "ReviewMatchRow",
                        [
                            ("marked".into(), marked.into()),
                            ("current".into(), marked.into()),
                            ("body".into(), Value::StyledText(body)),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let record = |row: ReviewLine| {
        RecordValue::new(
            "ReviewLine",
            [
                ("id".into(), row.id.into()),
                ("old-line".into(), row.old_line.into()),
                ("new-line".into(), row.new_line.into()),
                ("kind".into(), row.kind.into()),
                ("content".into(), row.content.into()),
                ("section".into(), row.section.into()),
            ]
            .into(),
        )
    };
    view.set(
        "review-lines",
        Value::RecordList(RecordList::new(
            "ReviewLine",
            vec![record(header), record(line)],
        )),
    )?;
    view.set(
        "review-pairs",
        Value::RecordList(RecordList::new(
            "ReviewPair",
            pairs
                .into_iter()
                .map(|pair| {
                    RecordValue::new(
                        "ReviewPair",
                        [
                            ("id".into(), pair.id.into()),
                            ("full".into(), pair.full.into()),
                            ("left".into(), Value::Record(record(pair.left))),
                            ("right".into(), Value::Record(record(pair.right))),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let weak = view.as_weak();
    view.on_mut("toggle-review-split", move |_| {
        let ui = weak.upgrade().unwrap();
        let is_split = ui.get("review-split").unwrap() == Value::Bool(true);
        ui.set("review-split", (!is_split).into())
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    let report = hits.clone();
    view.on_mut("select-review-side", move |args| {
        let [Value::Int(id), Value::Bool(old)] = args else {
            return Err("expected line id and side".into());
        };
        report.borrow_mut().push(format!("line:{id}:{old}"));
        Ok(None)
    })?;
    let report = hits.clone();
    view.on_mut("update-review-hunk", move |args| {
        let [Value::Int(id)] = args else {
            return Err("expected hunk id".into());
        };
        report.borrow_mut().push(format!("hunk:{id}"));
        Ok(None)
    })?;
    let report = hits.clone();
    view.on_mut("move-review-find", move |args| {
        let [Value::Int(delta)] = args else {
            return Err("expected find direction".into());
        };
        report.borrow_mut().push(format!("find:{delta}"));
        Ok(None)
    })?;
    let weak = view.as_weak();
    let report = hits.clone();
    view.on_mut("edit-review-line", move |args| {
        let [Value::Int(id)] = args else {
            return Err("expected line id".into());
        };
        report.borrow_mut().push(format!("edit:{id}"));
        let ui = weak.upgrade().unwrap();
        for (key, value) in [
            (
                "review-line-edit-location",
                "src/file.rs · 新行 8".to_owned().into(),
            ),
            ("review-line-edit-original", "仅换行位置".to_owned().into()),
            ("review-line-edit-text", "仅换行位置".to_owned().into()),
            ("review-line-edit-ready", true.into()),
            ("review-line-edit-open", true.into()),
        ] {
            ui.set(key, value).map_err(|e| e.to_string())?;
        }
        ui.invoke("focus-review-line-edit", &[])
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    let weak = view.as_weak();
    let report = hits.clone();
    view.on_mut("save-review-line-edit", move |_| {
        let ui = weak.upgrade().unwrap();
        let Value::String(text) = ui.get("review-line-edit-text").map_err(|e| e.to_string())?
        else {
            return Err("expected review draft".into());
        };
        report.borrow_mut().push(format!("save:{text}"));
        ui.set("review-line-edit-open", false.into())
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    for (event, file) in [
        ("propose-review-file-revert", true),
        ("propose-review-hunk-revert", false),
    ] {
        let weak = view.as_weak();
        let report = hits.clone();
        view.on_mut(event, move |args| {
            let label = match (file, args) {
                (true, []) => "preview:file".into(),
                (false, [Value::Int(id)]) => format!("preview:hunk:{id}"),
                _ => return Err("expected a file or hunk revert".into()),
            };
            report.borrow_mut().push(label);
            let ui = weak.upgrade().unwrap();
            for (key, value) in [
                ("review-revert-file", file.into()),
                ("review-revert-path", "src/file.rs".to_owned().into()),
                (
                    "review-revert-preview",
                    Value::StyledText(StyledText::from_plain_text(PREVIEW)),
                ),
                ("review-revert-open", true.into()),
            ] {
                ui.set(key, value).map_err(|error| error.to_string())?;
            }
            Ok(None)
        })?;
    }
    let weak = view.as_weak();
    let report = hits.clone();
    view.on_mut("confirm-review-revert", move |_| {
        report.borrow_mut().push("confirm".into());
        weak.upgrade()
            .unwrap()
            .set("review-revert-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_split_review(view, &hits);
    Ok(())
}

fn exercise_hunk_actions<C: Component>(component: C, hits: &RefCell<Vec<i64>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    for (index, label) in ["暂存这块", "取消暂存这块"].into_iter().enumerate() {
        let key = ui.node("review-line-list").children[index].row_key.unwrap();
        let path = ui
            .find_row_path("review-line-list", key, Some("hunk-index-action"))
            .unwrap();
        assert_eq!(ui.scene().at_path(&path).unwrap().text("text"), Some(label));
        ui.click_at_path(&path);
    }
    assert!(ui.activate(ActivationKey::Space));
    assert_eq!(hits.borrow().as_slice(), [11, 22, 22]);
}

#[test]
fn hunk_headers_route_only_the_clicked_side_and_row_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let hits = Rc::new(RefCell::new(Vec::new()));
    let rows = [(11i64, "未暂存"), (22i64, "已暂存")];
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_review_selection("main.rs".into());
    window.set_review_can_stage_hunks(true);
    window.set_review_can_unstage_hunks(true);
    for (id, section) in rows {
        window.model_review_lines().push(ReviewLine {
            id,
            kind: "hunk".into(),
            content: "@@ -20,2 +20,2 @@".into(),
            section: section.into(),
            ..ReviewLine::default()
        });
    }
    let output = hits.clone();
    window.on_update_review_hunk(move |row| output.borrow_mut().push(row));
    exercise_hunk_actions(window, &hits);
    hits.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("review-open", true.into())?;
    instance.set("review-selection", "main.rs".to_owned().into())?;
    instance.set("review-can-stage-hunks", true.into())?;
    instance.set("review-can-unstage-hunks", true.into())?;
    instance.set(
        "review-lines",
        Value::RecordList(RecordList::new(
            "ReviewLine",
            rows.iter()
                .map(|&(id, section)| {
                    RecordValue::new(
                        "ReviewLine",
                        [
                            ("id".into(), id.into()),
                            ("old-line".into(), 0i64.into()),
                            ("new-line".into(), 0i64.into()),
                            ("kind".into(), "hunk".to_owned().into()),
                            ("content".into(), "@@ -20,2 +20,2 @@".to_owned().into()),
                            ("section".into(), section.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = hits.clone();
    instance.on_mut("update-review-hunk", move |args| {
        let [Value::Int(row)] = args else {
            return Err("差异块编号无效".into());
        };
        output.borrow_mut().push(*row);
        Ok(None)
    })?;
    exercise_hunk_actions(instance, &hits);
    Ok(())
}

fn exercise_staged_notes<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(680.0, 540.0));
    ui.click("show-staged-review-notes");
    assert!(ui.node("staged-review-notes-dialog").boolean("open", false));
    let list = "staged-review-notes-list";
    let first = ui.node(list).children[0].row_key.unwrap();
    let edit = ui
        .find_row_path(list, first, Some("edit-staged-note"))
        .unwrap();
    ui.click_at_path(&edit);
    ui.advance_time(Duration::from_millis(2));
    assert!(!ui.node("staged-review-notes-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("review-note-input"));
    ui.fill_focused("修改后的旧行批注");
    assert_eq!(
        ui.node("stage-review-note-button").text("text"),
        Some("保存批注")
    );
    ui.click("stage-review-note-button");
    ui.click("show-staged-review-notes");
    let second = ui.node(list).children[1].row_key.unwrap();
    let remove = ui
        .find_row_path(list, second, Some("remove-staged-note"))
        .unwrap();
    ui.click_at_path(&remove);
    assert_eq!(ui.node(list).children.len(), 1);
    ui.click("apply-staged-review-notes-button");
    assert!(!ui.node("staged-review-notes-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        ["edit:7", "stage:修改后的旧行批注", "remove:8", "apply"]
    );
}

#[test]
fn staged_review_note_controls_route_edit_remove_and_apply_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let fixtures = [
        (7i64, "src/one.rs · 旧行 5 · 未暂存", "检查错误路径"),
        (8i64, "src/two.rs · 新行 8 · 已暂存", "补充测试"),
    ];
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_review_open(true);
    window.set_viewport_width(Length(680.0));
    for (id, reference, text) in fixtures {
        window.model_review_notes().push(ReviewNoteRow {
            id,
            reference: reference.into(),
            text: text.into(),
        });
    }
    let weak = window.as_weak();
    let report = calls.clone();
    window.on_edit_staged_review_note(move |id| {
        report.borrow_mut().push(format!("edit:{id}"));
        let mut ui = weak.upgrade().unwrap();
        ui.set_review_notes_open(false);
        ui.set_review_note_reference(fixtures[0].1.into());
        ui.set_review_note_text(fixtures[0].2.into());
        ui.set_review_note_queued_id(id);
        ui.invoke_focus_review_note();
    });
    let weak = window.as_weak();
    let report = calls.clone();
    window.on_stage_review_note(move || {
        let mut ui = weak.upgrade().unwrap();
        report
            .borrow_mut()
            .push(format!("stage:{}", ui.get_review_note_text()));
        ui.set_review_note_reference(String::new());
        ui.set_review_note_text(String::new());
        ui.set_review_note_queued_id(-1);
    });
    let report = calls.clone();
    let model = window.model_review_notes();
    window.on_remove_staged_review_note(move |id| {
        report.borrow_mut().push(format!("remove:{id}"));
        model.remove(1);
    });
    let weak = window.as_weak();
    let report = calls.clone();
    window.on_apply_staged_review_notes(move || {
        report.borrow_mut().push("apply".into());
        weak.upgrade().unwrap().set_review_notes_open(false);
    });
    exercise_staged_notes(window, &calls);
    calls.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    view.set("workspace", "/workspace".to_owned().into())?;
    view.set("review-open", true.into())?;
    view.set("viewport-width", Length(680.0).into())?;
    let rows = RecordList::new(
        "ReviewNoteRow",
        fixtures
            .into_iter()
            .map(|(id, reference, text)| {
                RecordValue::new(
                    "ReviewNoteRow",
                    [
                        ("id".into(), id.into()),
                        ("reference".into(), reference.to_owned().into()),
                        ("text".into(), text.to_owned().into()),
                    ]
                    .into(),
                )
            })
            .collect(),
    );
    view.set("review-notes", Value::RecordList(rows.clone()))?;
    let weak = view.as_weak();
    let report = calls.clone();
    view.on_mut("edit-staged-review-note", move |args| {
        let [Value::Int(id)] = args else {
            return Err("expected note id".into());
        };
        report.borrow_mut().push(format!("edit:{id}"));
        let ui = weak.upgrade().unwrap();
        for (key, value) in [
            ("review-notes-open", false.into()),
            ("review-note-reference", fixtures[0].1.to_owned().into()),
            ("review-note-text", fixtures[0].2.to_owned().into()),
            ("review-note-queued-id", (*id).into()),
        ] {
            ui.set(key, value).map_err(|e| e.to_string())?;
        }
        ui.invoke("focus-review-note", &[])
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    let weak = view.as_weak();
    let report = calls.clone();
    view.on_mut("stage-review-note", move |_| {
        let ui = weak.upgrade().unwrap();
        let Value::String(text) = ui.get("review-note-text").map_err(|e| e.to_string())? else {
            return Err("expected note text".into());
        };
        report.borrow_mut().push(format!("stage:{text}"));
        for (key, value) in [
            ("review-note-reference", String::new().into()),
            ("review-note-text", String::new().into()),
            ("review-note-queued-id", (-1i64).into()),
        ] {
            ui.set(key, value).map_err(|e| e.to_string())?;
        }
        Ok(None)
    })?;
    let report = calls.clone();
    view.on_mut("remove-staged-review-note", move |args| {
        let [Value::Int(id)] = args else {
            return Err("expected note id".into());
        };
        report.borrow_mut().push(format!("remove:{id}"));
        rows.model().remove(1);
        Ok(None)
    })?;
    let weak = view.as_weak();
    let report = calls.clone();
    view.on_mut("apply-staged-review-notes", move |_| {
        report.borrow_mut().push("apply".into());
        weak.upgrade()
            .unwrap()
            .set("review-notes-open", false.into())
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    exercise_staged_notes(view, &calls);
    Ok(())
}

fn exercise_line_note<C: Component>(component: C) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert!(!ui.node("review-note-panel").is_visible());
    ui.click("raw-patch");
    assert!(!ui.node("review-line-list").is_visible());
    assert!(ui.node("patch-scroll").is_visible());
    ui.click("raw-patch");
    let row = ui.node("review-line-list").children[0].row_key.unwrap();
    let gutter = ui
        .find_row_path("review-line-list", row, Some("line-gutter"))
        .unwrap();
    assert_eq!(
        ui.scene().at_path(&gutter).unwrap().text("accessible-name"),
        Some("批注新行 42")
    );
    ui.click_at_path(&gutter);
    ui.advance_time(Duration::from_millis(2));
    assert!(ui.node("review-note-panel").is_visible());
    assert_eq!(ui.focused(), Some("review-note-input"));
    ui.fill_focused("请补上 Linux 运行说明");
    ui.click("add-review-note-button");
    assert!(!ui.node("review-note-panel").is_visible());
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(ui.node("input").text("text"), Some("请补上 Linux 运行说明"));
}

#[test]
fn numbered_diff_routes_the_note_editor_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    const REFERENCE: &str = "docs/development.md · 新行 42 · 本轮改动";
    let mut window = AgentWorkbench::new();
    window.set_review_open(true);
    window.set_review_selection("docs/development.md".into());
    window.model_review_lines().push(ReviewLine {
        id: 7,
        old_line: 0,
        new_line: 42,
        kind: "add".into(),
        content: "桌面计数器".into(),
        section: String::new(),
    });
    let weak = window.as_weak();
    window.on_select_review_line(move |id| {
        assert_eq!(id, 7);
        let mut window = weak.upgrade().unwrap();
        window.set_review_note_reference(REFERENCE.into());
    });
    let weak = window.as_weak();
    window.on_add_review_note(move || {
        let mut window = weak.upgrade().unwrap();
        window.set_draft(window.get_review_note_text());
        window.set_review_note_reference(String::new());
    });
    exercise_line_note(window);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("review-open", Value::Bool(true))?;
    instance.set("review-selection", "docs/development.md".to_owned().into())?;
    instance.set(
        "review-lines",
        Value::RecordList(RecordList::new(
            "ReviewLine",
            vec![RecordValue::new(
                "ReviewLine",
                [
                    ("id".into(), 7i64.into()),
                    ("old-line".into(), 0i64.into()),
                    ("new-line".into(), 42i64.into()),
                    ("kind".into(), "add".to_owned().into()),
                    ("content".into(), "桌面计数器".to_owned().into()),
                    ("section".into(), String::new().into()),
                ]
                .into(),
            )],
        )),
    )?;
    let weak = instance.as_weak();
    instance.on_mut("select-review-line", move |args| {
        assert!(matches!(args, [Value::Int(7)]));
        weak.upgrade()
            .unwrap()
            .set("review-note-reference", REFERENCE.to_owned().into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let weak = instance.as_weak();
    instance.on_mut("add-review-note", move |_| {
        let window = weak.upgrade().unwrap();
        let note = window
            .get("review-note-text")
            .map_err(|error| error.to_string())?;
        window
            .set("draft", note)
            .map_err(|error| error.to_string())?;
        window
            .set("review-note-reference", String::new().into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_line_note(instance);
    Ok(())
}

fn exercise_search_and_rename<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("session-search");
    ui.fill_focused("Ba");
    ui.advance_time(Duration::from_millis(120));
    assert!(calls.borrow().is_empty());
    ui.fill_focused("Bazel");
    ui.advance_time(Duration::from_millis(100));
    assert!(calls.borrow().is_empty());
    ui.advance_time(Duration::from_millis(81));
    assert_eq!(calls.borrow().as_slice(), ["搜索 Bazel"]);
    assert!(ui.node("session-list").is_visible());
    assert_eq!(ui.node("session-list").children.len(), 1);

    ui.click("conversation-menu-trigger");
    ui.click("rename-session");
    assert!(ui.node("rename-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("rename-field"));
    ui.fill_focused("Bazel 构建笔记");
    ui.click("save-rename");
    assert_eq!(
        calls.borrow().as_slice(),
        ["搜索 Bazel", "重命名 Bazel 构建笔记"]
    );
    ui.click("cancel-rename");
    ui.click("session-search");
    ui.fill_focused("");
    ui.advance_time(Duration::from_millis(181));
    assert_eq!(calls.borrow().last().map(String::as_str), Some("搜索 "));
    assert_eq!(ui.node("session-list").children.len(), 2);

    ui.click("archived-sessions");
    assert!(ui.can_hit("active-sessions"));
    ui.click("active-sessions");
    assert!(ui.can_hit("archived-sessions"));
    ui.click("input");
    ui.fill_focused("原有草稿");
    assert!(ui.can_hit("send-button"));
    ui.click("conversation-menu-trigger");
    assert_eq!(ui.node("archive-session").text("text"), Some("归档会话"));
    ui.click("archive-session");
    assert!(!ui.can_hit("send-button"));
    assert_eq!(ui.node("input").text("text"), Some("原有草稿"));
    ui.click("conversation-menu-trigger");
    assert_eq!(ui.node("archive-session").text("text"), Some("恢复会话"));
    ui.click("archive-session");
    assert!(ui.can_hit("send-button"));
    assert!(calls.borrow().ends_with(&[
        "查看归档 true".into(),
        "查看归档 false".into(),
        "归档 true".into(),
        "归档 false".into(),
    ]));
}

#[test]
fn search_debounce_and_rename_dialog_match_in_aot_and_interpreter()
-> Result<(), Box<dyn std::error::Error>> {
    let rows = || {
        [
            ConversationRow {
                id: 1,
                title: "Bazel cache".into(),
                folder: "workspace".into(),
                activity: String::new(),
                pinned: false,
                pinned_shown: false,
                shown: true,
            },
            ConversationRow {
                id: 2,
                title: "Renderer".into(),
                folder: "workspace".into(),
                activity: String::new(),
                pinned: false,
                pinned_shown: false,
                shown: true,
            },
        ]
    };
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_conversation_title("Bazel cache".into());
    for row in rows() {
        window.model_conversations().push(row);
    }
    let events = calls.clone();
    let weak = window.as_weak();
    window.on_search_sessions(move |query| {
        events.borrow_mut().push(format!("搜索 {query}"));
        let cleared = query.is_empty();
        let mut window = weak.upgrade().unwrap();
        window.set_history_query(query);
        let model = window.model_conversations();
        let mut row = model.row(1).unwrap();
        row.shown = cleared;
        model.set_row(1, row);
    });
    let events = calls.clone();
    window.on_rename_conversation(move |name| events.borrow_mut().push(format!("重命名 {name}")));
    let events = calls.clone();
    let weak = window.as_weak();
    window.on_view_archived_sessions(move |archived| {
        events.borrow_mut().push(format!("查看归档 {archived}"));
        weak.upgrade().unwrap().set_archive_view(archived);
    });
    let events = calls.clone();
    let weak = window.as_weak();
    window.on_set_session_archived(move |archived| {
        events.borrow_mut().push(format!("归档 {archived}"));
        let mut window = weak.upgrade().unwrap();
        window.set_conversation_archived(archived);
        window.set_archive_view(archived);
    });
    exercise_search_and_rename(window, &calls);

    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    instance.set("conversation-title", "Bazel cache".to_owned().into())?;
    let record = |row: ConversationRow| {
        RecordValue::new(
            "ConversationRow",
            [
                ("id".into(), row.id.into()),
                ("title".into(), row.title.into()),
                ("folder".into(), row.folder.into()),
                ("activity".into(), row.activity.into()),
                ("shown".into(), row.shown.into()),
                ("pinned".into(), row.pinned.into()),
                ("pinned-shown".into(), row.pinned_shown.into()),
            ]
            .into(),
        )
    };
    let model = RecordList::new("ConversationRow", rows().into_iter().map(record).collect());
    instance.set("conversations", Value::RecordList(model.clone()))?;
    let events = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("search-sessions", move |args| {
        if let [Value::String(query)] = args {
            events.borrow_mut().push(format!("搜索 {query}"));
            weak.upgrade()
                .unwrap()
                .set("history-query", query.clone().into())
                .map_err(|error| error.to_string())?;
            let mut hidden = rows()[1].clone();
            hidden.shown = query.is_empty();
            model.model().set_row(1, record(hidden));
        }
        Ok(None)
    })?;
    let events = calls.clone();
    instance.on_mut("rename-conversation", move |args| {
        if let [Value::String(name)] = args {
            events.borrow_mut().push(format!("重命名 {name}"));
        }
        Ok(None)
    })?;
    let events = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("view-archived-sessions", move |args| {
        if let [Value::Bool(archived)] = args {
            events.borrow_mut().push(format!("查看归档 {archived}"));
            weak.upgrade()
                .unwrap()
                .set("archive-view", (*archived).into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let events = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("set-session-archived", move |args| {
        if let [Value::Bool(archived)] = args {
            events.borrow_mut().push(format!("归档 {archived}"));
            let window = weak.upgrade().unwrap();
            window
                .set("conversation-archived", (*archived).into())
                .map_err(|error| error.to_string())?;
            window
                .set("archive-view", (*archived).into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    exercise_search_and_rename(instance, &calls);
    Ok(())
}

fn exercise_generation_choices<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    set_busy: impl Fn(&mut C, bool),
) {
    let mut ui = UiDriver::headless(component, Size::new(680.0, 620.0));
    ui.click("input");
    ui.fill_focused("验证模型选择");
    let composer_before = ui.bounds("composer-panel");
    assert!(ui.menu_shortcut("Ctrl+Shift+M".parse().unwrap()));
    assert!(ui.node("generation-picker").boolean("open", false));
    assert_eq!(ui.focused(), Some("model-choice"));
    assert_eq!(ui.bounds("composer-panel"), composer_before);
    assert!(!ui.can_hit("effort-choice"));
    ui.click("model-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert!(ui.can_hit("effort-choice"));
    ui.click("effort-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    assert!(
        ui.node("generation-picker-trigger")
            .text("accessible-name")
            .is_some_and(|label| label.ends_with("Sample Model · 高"))
    );
    ui.escape();
    assert!(!ui.node("generation-picker").boolean("open", false));
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(ui.node("input").text("text"), Some("验证模型选择"));

    ui.update(|view| set_busy(view, true));
    ui.click("generation-picker-trigger");
    assert!(!ui.can_hit("model-choice"));
    assert!(!ui.can_hit("effort-choice"));
    ui.escape();
    ui.update(|view| set_busy(view, false));
    let send = ui.bounds("send-button");
    assert!(send.x + send.width <= composer_before.x + composer_before.width);
    ui.click("send-button");
    assert_eq!(
        calls.borrow().as_slice(),
        ["model:sample", "effort:high", "send:验证模型选择"]
    );
}

#[test]
fn generation_selectors_send_model_and_effort_through_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let models = [("", "会话默认"), ("sample", "Sample Model")];
    let efforts = [("medium", "中"), ("high", "高")];
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_viewport_width(Length(680.0));
    for (key, label) in models {
        window.model_model_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    for (key, label) in efforts {
        window.model_effort_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    window.set_generation_effort("medium".into());
    let events = calls.clone();
    window.on_choose_model(move |value| events.borrow_mut().push(format!("model:{value}")));
    let events = calls.clone();
    window.on_choose_effort(move |value| events.borrow_mut().push(format!("effort:{value}")));
    let events = calls.clone();
    window.on_send(move |value| events.borrow_mut().push(format!("send:{value}")));
    exercise_generation_choices(window, &calls, AgentWorkbench::set_busy);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    instance.set("viewport-width", Length(680.0).into())?;
    instance.set("generation-effort", "medium".to_owned().into())?;
    for (property, rows) in [("model-choices", models), ("effort-choices", efforts)] {
        instance.set(
            property,
            Value::RecordList(RecordList::new(
                "GenerationChoice",
                rows.into_iter()
                    .map(|(key, label)| {
                        RecordValue::new(
                            "GenerationChoice",
                            [
                                ("key".into(), key.to_owned().into()),
                                ("label".into(), label.to_owned().into()),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            )),
        )?;
    }
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    for (event, kind) in [
        ("choose-model", "model"),
        ("choose-effort", "effort"),
        ("send", "send"),
    ] {
        let calls = calls.clone();
        instance.on_mut(event, move |args| {
            if let [Value::String(value)] = args {
                calls.borrow_mut().push(format!("{kind}:{value}"));
            }
            Ok(None)
        })?;
    }
    exercise_generation_choices(instance, &calls, |view, busy| {
        view.set("busy", busy.into()).unwrap()
    });
    Ok(())
}

fn exercise_plan_workflow<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    show_question: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("generation-picker-trigger");
    ui.click("plan-mode-choice");
    assert_eq!(ui.node("plan-mode-choice").text("text"), Some("计划"));
    assert_eq!(
        ui.node("generation-picker-trigger").text("text"),
        Some("计划 · 会话默认")
    );
    ui.escape();
    let apply = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("采用方案")
    })
    .expect("proposal action");
    ui.click_at_path(&apply);
    assert_eq!(
        ui.node("generation-picker-trigger").text("text"),
        Some("会话默认")
    );
    assert_eq!(ui.node("input").text("text"), Some("方案已放入草稿"));
    assert_eq!(calls.borrow().as_slice(), ["mode:true", "plan:0"]);

    ui.update(show_question);
    assert!(ui.node("plan-question").boolean("open", false));
    assert!(!ui.can_hit("confirm-question"));
    ui.click("defer-question");
    assert!(!ui.node("plan-question").boolean("open", false));
    assert!(ui.can_hit("reset"));
    ui.click("resume-question");
    assert!(ui.node("plan-question").boolean("open", false));
    ui.click("question-option");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("confirm-question");
    assert_eq!(
        calls.borrow().last().map(String::as_str),
        Some("answer:8:1:counter:false")
    );
}

#[test]
fn plan_mode_proposal_and_question_actions_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_plan_available(true);
    window.model_entries().push(TranscriptEntry {
        kind: "proposal".into(),
        heading: "建议方案".into(),
        status: "已完成".into(),
        status_tone: String::new(),
        body: StyledText::from_plain_text("核对计数器示例。"),
    });
    let events = calls.clone();
    let weak = window.as_weak();
    window.on_choose_plan_mode(move |plan| {
        events.borrow_mut().push(format!("mode:{plan}"));
        weak.upgrade().unwrap().set_plan_mode(plan);
    });
    let events = calls.clone();
    let weak = window.as_weak();
    window.on_use_plan(move |index| {
        events.borrow_mut().push(format!("plan:{index}"));
        let mut window = weak.upgrade().unwrap();
        window.set_plan_mode(false);
        window.set_draft("方案已放入草稿".into());
    });
    let events = calls.clone();
    window.on_answer_question(move |token, index, answer, skip| {
        events
            .borrow_mut()
            .push(format!("answer:{token}:{index}:{answer}:{skip}"));
    });
    exercise_plan_workflow(window, &calls, |window| {
        window.set_question_token(8);
        window.set_question_index(1);
        window.set_question_heading("范围 · 2 / 2".into());
        window.set_question_text("选哪个示例？".into());
        for (key, label) in [("", "请选择"), ("counter", "计数器")] {
            window.model_question_options().push(GenerationChoice {
                key: key.into(),
                label: label.into(),
            });
        }
        window.set_question_open(true);
    });

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("plan-available", true.into())?;
    instance.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            vec![RecordValue::new(
                "TranscriptEntry",
                [
                    ("kind".into(), "proposal".to_owned().into()),
                    ("heading".into(), "建议方案".to_owned().into()),
                    ("status".into(), "已完成".to_owned().into()),
                    ("status-tone".into(), String::new().into()),
                    (
                        "body".into(),
                        Value::StyledText(StyledText::from_plain_text("核对计数器示例。")),
                    ),
                ]
                .into(),
            )],
        )),
    )?;
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let events = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("choose-plan-mode", move |args| {
        if let [Value::Bool(plan)] = args {
            events.borrow_mut().push(format!("mode:{plan}"));
            weak.upgrade()
                .unwrap()
                .set("plan-mode", (*plan).into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let events = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("use-plan", move |args| {
        if let [Value::Int(index)] = args {
            events.borrow_mut().push(format!("plan:{index}"));
            let window = weak.upgrade().unwrap();
            window
                .set("plan-mode", false.into())
                .map_err(|error| error.to_string())?;
            window
                .set("draft", "方案已放入草稿".to_owned().into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let events = calls.clone();
    instance.on_mut("answer-question", move |args| {
        if let [
            Value::Int(token),
            Value::Int(index),
            Value::String(answer),
            Value::Bool(skip),
        ] = args
        {
            events
                .borrow_mut()
                .push(format!("answer:{token}:{index}:{answer}:{skip}"));
        }
        Ok(None)
    })?;
    exercise_plan_workflow(instance, &calls, |window| {
        window.set("question-token", 8i64.into()).unwrap();
        window.set("question-index", 1i64.into()).unwrap();
        window
            .set("question-heading", "范围 · 2 / 2".to_owned().into())
            .unwrap();
        window
            .set("question-text", "选哪个示例？".to_owned().into())
            .unwrap();
        window
            .set(
                "question-options",
                Value::RecordList(RecordList::new(
                    "GenerationChoice",
                    [("", "请选择"), ("counter", "计数器")]
                        .into_iter()
                        .map(|(key, label)| {
                            RecordValue::new(
                                "GenerationChoice",
                                [
                                    ("key".into(), key.to_owned().into()),
                                    ("label".into(), label.to_owned().into()),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                )),
            )
            .unwrap();
        window.set("question-open", true.into()).unwrap();
    });
    Ok(())
}

fn exercise_queue_editor<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    mark_stale: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("input");
    ui.fill_focused("独立的草稿");
    let row_action = |ui: &UiDriver<C>, index: usize, id: &str| {
        let key = ui.node("queued-list").children[index].row_key.unwrap();
        ui.find_row_path("queued-list", key, Some(id)).unwrap()
    };
    let first_up = row_action(&ui, 0usize, "move-queue-up");
    assert!(
        !ui.scene()
            .at_path(&first_up)
            .unwrap()
            .boolean("enabled", true)
    );
    let second_up = row_action(&ui, 1usize, "move-queue-up");
    ui.click_at_path(&second_up);
    let edit = row_action(&ui, 0usize, "edit-queue-row");
    ui.click_at_path(&edit);
    assert!(ui.node("queued-editor").boolean("open", false));
    assert_eq!(ui.focused(), Some("queued-edit-input"));
    assert!(!ui.can_hit("save-queued-edit-button"));
    ui.fill_focused("  修改正文\n    保留缩进");
    ui.click("save-queued-edit-button");
    ui.update(mark_stale);
    assert!(ui.node("queued-edit-warning").is_visible());
    assert!(!ui.can_hit("save-queued-edit-button"));
    assert_eq!(
        ui.node("queued-edit-input").text("text"),
        Some("  修改正文\n    保留缩进")
    );
    ui.click("close-queued-edit");
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(ui.node("input").text("text"), Some("独立的草稿"));
    let preview = row_action(&ui, 1usize, "edit-queue-row");
    ui.click_at_path(&preview);
    assert_eq!(ui.node("queued-editor").text("title"), Some("预览待发消息"));
    assert!(ui.node("queued-edit-input").boolean("read-only", false));
    assert!(!ui.node("save-queued-edit-button").is_visible());
    ui.click("close-queued-edit");
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "move:b:-1",
            "open:a",
            "save:  修改正文\n    保留缩进",
            "open:b"
        ]
    );
}

#[test]
fn queued_editor_uses_stable_rows_and_preserves_drafts_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    for (id, text, editable) in [("a", "第一项", true), ("b", "第二项", false)] {
        window.model_queued_messages().push(QueuedRow {
            id: id.into(),
            text: text.into(),
            editable,
        });
    }
    let output = calls.clone();
    window.on_move_queued(move |id, step| output.borrow_mut().push(format!("move:{id}:{step}")));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_open_queued_editor(move |id| {
        output.borrow_mut().push(format!("open:{id}"));
        let mut ui = weak.upgrade().unwrap();
        let text = if id == "a" { "第一项" } else { "第二项" };
        ui.set_queue_edit_original(text.into());
        ui.set_queue_edit_text(text.into());
        ui.set_queue_edit_read_only(id != "a");
        ui.set_queue_edit_stale(false);
        ui.set_queue_edit_open(true);
        ui.invoke_focus_queued_edit();
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_save_queued_edit(move || {
        output.borrow_mut().push(format!(
            "save:{}",
            weak.upgrade().unwrap().get_queue_edit_text()
        ));
    });
    exercise_queue_editor(window, &calls, |view| view.set_queue_edit_stale(true));
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set(
        "queued-messages",
        Value::RecordList(RecordList::new(
            "QueuedRow",
            [("a", "第一项", true), ("b", "第二项", false)]
                .into_iter()
                .map(|(id, text, editable)| {
                    RecordValue::new(
                        "QueuedRow",
                        [
                            ("id".into(), id.to_owned().into()),
                            ("text".into(), text.to_owned().into()),
                            ("editable".into(), editable.into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = calls.clone();
    instance.on_mut("move-queued", move |args| {
        if let [Value::String(id), Value::Int(step)] = args {
            output.borrow_mut().push(format!("move:{id}:{step}"));
        }
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("open-queued-editor", move |args| {
        if let [Value::String(id)] = args {
            output.borrow_mut().push(format!("open:{id}"));
            let ui = weak.upgrade().unwrap();
            let text = if id == "a" { "第一项" } else { "第二项" }.to_owned();
            for (property, value) in [
                ("queue-edit-original", text.clone().into()),
                ("queue-edit-text", text.into()),
                ("queue-edit-read-only", (id != "a").into()),
                ("queue-edit-stale", false.into()),
                ("queue-edit-open", true.into()),
            ] {
                ui.set(property, value).map_err(|error| error.to_string())?;
            }
            ui.invoke("focus-queued-edit", &[])
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("save-queued-edit", move |_| {
        if let Value::String(text) = weak.upgrade().unwrap().get("queue-edit-text").unwrap() {
            output.borrow_mut().push(format!("save:{text}"));
        }
        Ok(None)
    })?;
    exercise_queue_editor(instance, &calls, |view| {
        view.set("queue-edit-stale", true.into()).unwrap()
    });
    Ok(())
}

fn exercise_followups<C: Component>(
    component: C,
    calls: &RefCell<Vec<String>>,
    finish: impl FnOnce(&mut C),
    cold: impl FnOnce(&mut C),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert_eq!(ui.node("send-button").text("text"), Some("排队"));
    ui.click("input");
    ui.fill_focused("当前草稿");
    ui.click("steer-button");
    ui.click("send-button");
    let key = ui.node("queued-list").children[0].row_key.unwrap();
    let remove = ui
        .find_row_path("queued-list", key, Some("remove-queue-row"))
        .unwrap();
    ui.click_at_path(&remove);
    ui.update(finish);
    let key = ui.node("queued-list").children[0].row_key.unwrap();
    let start = ui
        .find_row_path("queued-list", key, Some("start-queue-row"))
        .unwrap();
    ui.click_at_path(&start);
    ui.update(cold);
    assert!(!ui.can_hit("send-button"));
    ui.click("resume-queue-button");
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "true:当前草稿",
            "false:当前草稿",
            "remove:q",
            "start:q",
            "resume",
        ]
    );
}

#[test]
fn followup_controls_preserve_the_same_actions_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_busy(true);
    window.set_active_turn_id("a".into());
    window.set_queue_resumed(true);
    window.model_queued_messages().push(QueuedRow {
        id: "q".into(),
        text: "待发送".into(),
        editable: true,
    });
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    window.on_followup(move |text, steer| output.borrow_mut().push(format!("{steer}:{text}")));
    let output = calls.clone();
    window.on_remove_queued(move |id| output.borrow_mut().push(format!("remove:{id}")));
    let output = calls.clone();
    window.on_start_queued(move |id| output.borrow_mut().push(format!("start:{id}")));
    let output = calls.clone();
    window.on_resume_queue(move || output.borrow_mut().push("resume".into()));
    exercise_followups(
        window,
        &calls,
        |window| window.set_busy(false),
        |window| window.set_queue_resumed(false),
    );

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("busy", true.into())?;
    instance.set("active-turn-id", "a".to_owned().into())?;
    instance.set("queue-resumed", true.into())?;
    instance.set(
        "queued-messages",
        Value::RecordList(RecordList::new(
            "QueuedRow",
            vec![RecordValue::new(
                "QueuedRow",
                [
                    ("id".into(), "q".to_owned().into()),
                    ("text".into(), "待发送".to_owned().into()),
                    ("editable".into(), true.into()),
                ]
                .into(),
            )],
        )),
    )?;
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    instance.on_mut("followup", move |args| {
        if let [Value::String(text), Value::Bool(steer)] = args {
            output.borrow_mut().push(format!("{steer}:{text}"));
        }
        Ok(None)
    })?;
    for (event, kind) in [("remove-queued", "remove"), ("start-queued", "start")] {
        let output = calls.clone();
        instance.on_mut(event, move |args| {
            if let [Value::String(id)] = args {
                output.borrow_mut().push(format!("{kind}:{id}"));
            }
            Ok(None)
        })?;
    }
    let output = calls.clone();
    instance.on_mut("resume-queue", move |_| {
        output.borrow_mut().push("resume".into());
        Ok(None)
    })?;
    exercise_followups(
        instance,
        &calls,
        |window| window.set("busy", false.into()).unwrap(),
        |window| window.set("queue-resumed", false.into()).unwrap(),
    );
    Ok(())
}

fn exercise_attachment_picker<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert!(!ui.can_hit("send-button"));
    ui.click("add-attachment");
    let scene = ui.component().render_shared();
    let mut queue = NativeDialogQueue::default();
    queue.sync(&scene.native_dialogs.iter().collect::<Vec<_>>());
    let request = queue.begin_next().expect("attachment picker");
    assert!(
        matches!(request.options.as_ref(), NativeDialogOptions::File(options)
        if options.mode == FileDialogMode::OpenFiles)
    );
    ui.update(|_| {
        assert!(queue.complete(
            request.id,
            NativeDialogResult::Files(vec![
                "/workspace/screenshot.png".into(),
                "/workspace/notes.md".into(),
            ])
        ));
    });

    let key = ui.node("attachment-list").children[0].row_key.unwrap();
    let preview = ui
        .find_row_path("attachment-list", key, Some("preview-file-action"))
        .unwrap();
    ui.click_at_path(&preview);
    assert!(ui.node("attachment-preview-dialog").boolean("open", false));
    ui.click("close-attachment-preview");
    assert!(ui.can_hit("send-button"));
    ui.click("send-button");
    let remove = ui
        .find_row_path("attachment-list", key, Some("remove-file-action"))
        .unwrap();
    ui.click_at_path(&remove);
    assert!(!ui.can_hit("send-button"));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "attach",
            "preview:/workspace/screenshot.png",
            "send:",
            "remove:/workspace/screenshot.png"
        ]
    );
}

#[test]
fn attachments_can_be_selected_previewed_and_sent_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_attach_files(move |paths| {
        assert_eq!(
            paths.as_slice(),
            ["/workspace/screenshot.png", "/workspace/notes.md"]
        );
        output.borrow_mut().push("attach".into());
        weak.upgrade()
            .unwrap()
            .model_attachment_rows()
            .push(AttachmentRow {
                path: paths[0].clone(),
                name: "screenshot.png".into(),
                kind: "图片".into(),
                preview: Image::Empty,
                has_preview: true,
            });
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_preview_attachment(move |path| {
        output.borrow_mut().push(format!("preview:{path}"));
        weak.upgrade().unwrap().set_attachment_preview_open(true);
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_remove_attachment(move |path| {
        output.borrow_mut().push(format!("remove:{path}"));
        weak.upgrade().unwrap().model_attachment_rows().clear();
    });
    let output = calls.clone();
    window.on_send(move |text| output.borrow_mut().push(format!("send:{text}")));
    exercise_attachment_picker(window, &calls);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    let model = RecordList::new("AttachmentRow", Vec::new());
    instance.set("attachment-rows", Value::RecordList(model.clone()))?;
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    let rows = model.clone();
    instance.on_mut("attach-files", move |args| {
        if let [Value::StringList(paths)] = args {
            assert_eq!(
                paths.as_slice(),
                ["/workspace/screenshot.png", "/workspace/notes.md"]
            );
            output.borrow_mut().push("attach".into());
            rows.model().push(RecordValue::new(
                "AttachmentRow",
                [
                    ("path".into(), paths[0].clone().into()),
                    ("name".into(), "screenshot.png".to_owned().into()),
                    ("kind".into(), "图片".to_owned().into()),
                    ("preview".into(), Value::Image(Image::Empty)),
                    ("has-preview".into(), true.into()),
                ]
                .into(),
            ));
        }
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("preview-attachment", move |args| {
        if let [Value::String(path)] = args {
            output.borrow_mut().push(format!("preview:{path}"));
            weak.upgrade()
                .unwrap()
                .set("attachment-preview-open", true.into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("remove-attachment", move |args| {
        if let [Value::String(path)] = args {
            output.borrow_mut().push(format!("remove:{path}"));
            model.model().clear();
        }
        Ok(None)
    })?;
    let output = calls.clone();
    instance.on_mut("send", move |args| {
        if let [Value::String(text)] = args {
            output.borrow_mut().push(format!("send:{text}"));
        }
        Ok(None)
    })?;
    exercise_attachment_picker(instance, &calls);
    Ok(())
}

fn exercise_context_dialog<C: Component>(
    component: C,
    set_busy: impl FnOnce(&mut C, bool),
    calls: &RefCell<usize>,
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    assert_eq!(ui.node("context-button").text("text"), Some("上下文 20%"));
    ui.click("context-button");
    assert!(ui.node("context-dialog").boolean("open", false));
    assert_eq!(ui.node("context-meter").number("progress"), Some(0.2));
    assert!(!ui.can_hit("compact-context-button"));
    ui.update(|window| set_busy(window, false));
    assert!(ui.can_hit("compact-context-button"));
    ui.click("compact-context-button");
    assert_eq!(*calls.borrow(), 1);
    assert!(!ui.node("context-dialog").boolean("open", false));
}

#[test]
fn context_meter_and_compaction_button_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Rc::new(RefCell::new(0));
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_busy(true);
    window.set_context_known(true);
    window.set_context_window_known(true);
    window.set_context_fraction(0.2);
    window.set_context_button_text("上下文 20%".into());
    let weak = window.as_weak();
    let output = calls.clone();
    window.on_compact_context(move || {
        *output.borrow_mut() += 1;
        weak.upgrade().unwrap().set_context_open(false);
    });
    exercise_context_dialog(window, AgentWorkbench::set_busy, &calls);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    instance.set("busy", true.into())?;
    instance.set("context-known", true.into())?;
    instance.set("context-window-known", true.into())?;
    instance.set("context-fraction", 0.2f64.into())?;
    instance.set("context-button-text", "上下文 20%".to_owned().into())?;
    let calls = Rc::new(RefCell::new(0));
    let weak = instance.as_weak();
    let output = calls.clone();
    instance.on_mut("compact-context", move |_| {
        *output.borrow_mut() += 1;
        weak.upgrade()
            .unwrap()
            .set("context-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_context_dialog(
        instance,
        |window, busy| window.set("busy", busy.into()).unwrap(),
        &calls,
    );
    Ok(())
}

fn exercise_conversation_header<C: Component>(
    component: C,
    set_width: impl Fn(&mut C, Length),
    set_busy: impl Fn(&mut C, bool),
) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    for width in [1120.0, 680.0] {
        ui.resize(Size::new(width, 620.0));
        ui.update(|component| set_width(component, Length(f64::from(width))));
        let title = ui.bounds("header-title");
        let workspace = ui.bounds("header-workspace");
        let tools = ui.bounds("workspace-tools");
        assert!(title.width >= 128.0, "{title:?}");
        assert!(workspace.width >= 106.0, "{workspace:?}");
        assert!(title.x + title.width <= tools.x);
        assert!(tools.x + tools.width <= width);
        for id in ["open-files", "open-review", "open-terminal"] {
            assert!(ui.can_hit(id), "{id} at {width}");
        }

        ui.click("conversation-menu-trigger");
        assert!(ui.node("conversation-menu").boolean("open", false));
        if width == 680.0 {
            assert!(ui.can_hit("external-open-project-menu"));
        }
        assert_eq!(ui.focused(), Some("rename-session"));
        ui.tab();
        assert_eq!(ui.focused(), Some("pin-session"));
        ui.escape();
        assert!(!ui.node("conversation-menu").boolean("open", false));
        assert_eq!(ui.focused(), Some("conversation-menu-trigger"));

        assert!(ui.activate(ActivationKey::Enter));
        assert!(ui.can_hit("archive-session"));
        ui.click("input");
        assert!(!ui.node("conversation-menu").boolean("open", false));
        assert_eq!(ui.focused(), Some("input"));

        let composer = ui.bounds("composer-panel");
        let send = ui.bounds("send-button");
        assert!(send.x + send.width <= composer.x + composer.width);
        assert!(ui.can_hit("send-button"));
        ui.update(|component| set_busy(component, true));
        for id in ["stop", "steer-button", "send-button"] {
            let button = ui.bounds(id);
            assert!(
                button.x + button.width <= composer.x + composer.width,
                "{id}: {button:?}"
            );
            assert!(ui.can_hit(id), "{id} at {width}");
        }
        ui.update(|component| set_busy(component, false));
    }
}

#[test]
fn conversation_header_keeps_title_tools_and_keyboard_menu_usable_at_minimum_width()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = "/workspace/projects/compiler-experiment/a-very-long-checkout-directory";
    let title = "验证窗口缩放时长会话标题与工作目录的排版";
    let status = "正在运行任务并等待最后几项操作完成";
    let mut window = AgentWorkbench::new();
    window.set_workspace(workspace.into());
    window.set_conversation_title(title.into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_navigation_back(true);
    window.set_navigation_forward(true);
    window.set_status_kind(String::new());
    window.set_status_detail(status.into());
    window.set_draft("保留草稿".into());
    window.set_active_turn_id("turn".into());
    exercise_conversation_header(
        window,
        AgentWorkbench::set_viewport_width,
        AgentWorkbench::set_busy,
    );

    let view = workbench_program()?.create("AgentWorkbench")?;
    view.set("workspace", workspace.to_owned().into())?;
    view.set("conversation-title", title.to_owned().into())?;
    view.set("status-kind", String::new().into())?;
    view.set("status-detail", status.to_owned().into())?;
    view.set("draft", "保留草稿".to_owned().into())?;
    view.set("active-turn-id", "turn".to_owned().into())?;
    for flag in [
        "connected",
        "renamable",
        "navigation-back",
        "navigation-forward",
    ] {
        view.set(flag, true.into())?;
    }
    exercise_conversation_header(
        view,
        |view, width| view.set("viewport-width", width.into()).unwrap(),
        |view, busy| view.set("busy", busy.into()).unwrap(),
    );
    Ok(())
}

fn exercise_historical_prompt<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(680.0, 490.0));
    let edit = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("accessible-name") == Some("编辑这条历史提示")
    })
    .expect("historical prompt action");
    ui.click_at_path(&edit);
    assert!(ui.node("historical-prompt-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("historical-prompt-input"));
    let dialog = ui.bounds("historical-prompt-dialog");
    let action = ui.bounds("fork-historical-prompt-button");
    assert!(
        action.y + action.height <= dialog.y + dialog.height - 6.0,
        "fork action is clipped: {action:?} in {dialog:?}"
    );
    assert!(dialog.y >= 0.0 && dialog.y + dialog.height <= 490.0);
    assert_eq!(
        ui.node("historical-prompt-input").text("text"),
        Some("原始历史提示")
    );
    ui.fill_focused("改写后加入当前草稿");
    ui.click("append-historical-prompt");
    assert!(!ui.node("historical-prompt-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("input"));
    assert_eq!(
        ui.node("input").text("text"),
        Some("已有草稿\n\n改写后加入当前草稿")
    );
    assert!(calls.borrow().is_empty());
    ui.click_at_path(&edit);
    ui.fill_focused("分支的新提示");
    ui.click("fork-historical-prompt-button");
    assert_eq!(calls.borrow().as_slice(), ["turn-a:分支的新提示"]);
    assert!(!ui.can_hit("append-historical-prompt"));
    assert!(!ui.can_hit("cancel-historical-prompt"));
    assert_eq!(
        ui.node("input").text("text"),
        Some("已有草稿\n\n改写后加入当前草稿")
    );
}

#[test]
fn editing_a_historical_prompt_keeps_the_current_draft_and_routes_a_cutoff_fork_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_draft("已有草稿".into());
    let body = StyledText::from_plain_text("原始历史提示\n附件 · sample.png");
    window.model_entries().push(TranscriptEntry {
        kind: "user".into(),
        heading: "你".into(),
        status: "可编辑".into(),
        status_tone: String::new(),
        body: body.clone(),
    });
    let weak = window.as_weak();
    window.on_edit_historical_prompt(move |index| {
        assert_eq!(index, 0);
        weak.upgrade().unwrap().invoke_show_historical_prompt(
            "原始历史提示".into(),
            "turn-a".into(),
            true,
        );
    });
    let calls = Rc::new(RefCell::new(Vec::new()));
    let forked = calls.clone();
    let weak = window.as_weak();
    window.on_fork_historical_prompt(move |turn, text| {
        forked.borrow_mut().push(format!("{turn}:{text}"));
        weak.upgrade().unwrap().set_fork_pending(true);
    });
    exercise_historical_prompt(window, &calls);
    calls.borrow_mut().clear();

    let view = workbench_program()?.create("AgentWorkbench")?;
    view.set("workspace", "/workspace".to_owned().into())?;
    view.set("connected", true.into())?;
    view.set("renamable", true.into())?;
    view.set("draft", "已有草稿".to_owned().into())?;
    let row = RecordValue::new(
        "TranscriptEntry",
        [
            ("kind".into(), "user".to_owned().into()),
            ("heading".into(), "你".to_owned().into()),
            ("status".into(), "可编辑".to_owned().into()),
            ("status-tone".into(), String::new().into()),
            ("body".into(), Value::StyledText(body)),
        ]
        .into(),
    );
    view.set(
        "entries",
        Value::RecordList(RecordList::new("TranscriptEntry", vec![row])),
    )?;
    let weak = view.as_weak();
    view.on_mut("edit-historical-prompt", move |args| {
        assert!(matches!(args, [Value::Int(0)]));
        weak.upgrade()
            .unwrap()
            .invoke(
                "show-historical-prompt",
                &[
                    "原始历史提示".to_owned().into(),
                    "turn-a".to_owned().into(),
                    true.into(),
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let forked = calls.clone();
    let weak = view.as_weak();
    view.on_mut("fork-historical-prompt", move |args| {
        let [Value::String(turn), Value::String(text)] = args else {
            return Err("expected fork anchor and edited text".into());
        };
        forked.borrow_mut().push(format!("{turn}:{text}"));
        weak.upgrade()
            .unwrap()
            .set("fork-pending", true.into())
            .map_err(|e| e.to_string())?;
        Ok(None)
    })?;
    exercise_historical_prompt(view, &calls);
    Ok(())
}

fn exercise_fork_dialog<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("conversation-menu-trigger");
    ui.click("fork-session");
    assert!(ui.node("fork-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("fork-name-field"));
    ui.fill_focused("键盘检查分支");
    ui.click("fork-turn-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("choose-worktree");
    ui.click("copy-worktree-changes");
    ui.click("submit-fork");
    assert_eq!(calls.borrow().as_slice(), ["键盘检查分支:first:true:true"]);
    assert!(!ui.can_hit("submit-fork"));
}

fn exercise_message_actions<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    let action = |ui: &UiDriver<C>, text: &str| {
        visible_path(ui.scene(), &|node| {
            node.kind == "Button" && node.text("text") == Some(text)
        })
        .unwrap()
    };
    ui.click_at_path(&action(&ui, "复制"));
    assert_eq!(calls.borrow().as_slice(), ["copy:1"]);

    ui.click_at_path(&action(&ui, "引用"));
    assert!(ui.node("message-quote-dialog").boolean("open", false));
    assert!(!ui.can_hit("quote-selection"));
    ui.click("message-quote-text");
    select_all_context_text(&mut ui);
    assert!(ui.can_hit("quote-selection"));
    ui.click("quote-selection");
    assert_eq!(calls.borrow().as_slice(), ["copy:1", "quote:第一份回复"]);
    ui.escape();
    ui.click_at_path(&action(&ui, "引用"));
    assert!(!ui.can_hit("quote-selection"));
    ui.escape();

    ui.click_at_path(&action(&ui, "从此分支"));
    assert!(ui.node("fork-dialog").boolean("open", false));
    assert_eq!(ui.focused(), Some("fork-name-field"));
    ui.click("submit-fork");
    assert_eq!(
        calls.borrow().as_slice(),
        ["copy:1", "quote:第一份回复", "fork:first"]
    );
}

#[test]
fn completed_message_actions_target_their_own_response_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let replies = [
        ("流式回复", ""),
        ("第一份回复", "已完成"),
        ("第二份回复", "已完成"),
    ];
    let cutoffs = vec![String::new(), "first".into(), "last".into()];
    let choices = [
        ("", "完整会话"),
        ("last", "最近一轮"),
        ("first", "较早一轮"),
    ];

    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_message_forks(cutoffs.clone());
    for (text, status) in replies {
        window.model_entries().push(TranscriptEntry {
            kind: "assistant".into(),
            heading: "Codex".into(),
            status: status.into(),
            status_tone: String::new(),
            body: StyledText::from_plain_text(text),
        });
    }
    for (key, label) in choices {
        window.model_fork_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let copy = calls.clone();
    window.on_copy_message(move |index| copy.borrow_mut().push(format!("copy:{index}")));
    let quote = calls.clone();
    window.on_quote_message(move |text| quote.borrow_mut().push(format!("quote:{text}")));
    let fork = calls.clone();
    window.on_create_fork(move |_, cutoff, _, _| fork.borrow_mut().push(format!("fork:{cutoff}")));
    exercise_message_actions(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    instance.set("message-forks", Value::StringList(cutoffs))?;
    instance.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            replies
                .map(|(text, status)| {
                    RecordValue::new(
                        "TranscriptEntry",
                        [
                            ("kind".into(), "assistant".to_owned().into()),
                            ("heading".into(), "Codex".to_owned().into()),
                            ("status".into(), status.to_owned().into()),
                            ("status-tone".into(), String::new().into()),
                            (
                                "body".into(),
                                Value::StyledText(StyledText::from_plain_text(text)),
                            ),
                        ]
                        .into(),
                    )
                })
                .to_vec(),
        )),
    )?;
    instance.set(
        "fork-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            choices
                .map(|(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .to_vec(),
        )),
    )?;
    let copy = calls.clone();
    instance.on_mut("copy-message", move |args| {
        let [Value::Int(index)] = args else {
            return Err("expected row index".into());
        };
        copy.borrow_mut().push(format!("copy:{index}"));
        Ok(None)
    })?;
    let quote = calls.clone();
    instance.on_mut("quote-message", move |args| {
        let [Value::String(text)] = args else {
            return Err("expected selected text".into());
        };
        quote.borrow_mut().push(format!("quote:{text}"));
        Ok(None)
    })?;
    let fork = calls.clone();
    instance.on_mut("create-fork", move |args| {
        let [_, Value::String(cutoff), _, _] = args else {
            return Err("expected cutoff".into());
        };
        fork.borrow_mut().push(format!("fork:{cutoff}"));
        Ok(None)
    })?;
    exercise_message_actions(instance, &calls);
    Ok(())
}

fn exercise_review_scope<C: Component>(component: C, calls: &RefCell<Vec<String>>) {
    let mut ui = UiDriver::headless(component, Size::new(1120.0, 760.0));
    ui.click("conversation-menu-trigger");
    ui.click("header-code-review");
    assert!(ui.node("review-scope-dialog").boolean("open", false));
    assert!(ui.can_hit("submit-code-review"));
    ui.click("review-scope-choice");
    ui.choice_key(ChoiceKey::Home);
    ui.choice_key(ChoiceKey::Down);
    ui.choice_key(ChoiceKey::Accept);
    assert!(ui.node("review-branch-choice").is_visible());
    assert!(!ui.can_hit("submit-code-review"));
    ui.click("review-branch-choice");
    ui.choice_key(ChoiceKey::End);
    ui.choice_key(ChoiceKey::Accept);
    ui.click("submit-code-review");
    assert_eq!(calls.borrow().as_slice(), ["load", "branch:main:"]);
    assert_eq!(ui.node("input").text("text"), Some("继续保留的草稿"));

    let action = visible_path(ui.scene(), &|node| {
        node.kind == "Button" && node.text("text") == Some("查看报告")
    })
    .expect("report action");
    ui.click_at_path(&action);
    assert!(ui.node("review-report-dialog").boolean("open", false));
    let path = ui.path("review-report-text");
    let (_, link) = ui
        .layout_at(&path)
        .unwrap()
        .text_link_regions()
        .next()
        .expect("report source link");
    let mut pointer = Interaction::default();
    pointer.pointer_move(
        ui.scene(),
        ui.layout(),
        Point {
            x: link.x + link.width * 0.5,
            y: link.y + link.height * 0.5,
        },
    );
    pointer.pointer_press_in_scene(ui.scene(), ui.layout());
    let activation = pointer
        .control_part_release(ui.scene(), ui.layout())
        .unwrap();
    assert_eq!(activation.path, path);
    let scene = ui.scene().clone();
    ui.update(|component| assert!(component.control_at(&scene, &path, activation.action)));
    assert!(ui.node("review-source-scroll").is_visible());
    assert!(!ui.node("review-report-scroll").is_visible());
    ui.click("back-to-review-report");
    assert!(ui.node("review-report-scroll").is_visible());
    ui.click("close-review-report");
    assert!(!ui.node("review-report-dialog").boolean("open", false));
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "load",
            "branch:main:",
            "report:0",
            "review-source:2:2:src.rs"
        ],
    );
}

#[test]
fn native_review_scope_and_source_navigation_match_in_both_backends()
-> Result<(), Box<dyn std::error::Error>> {
    let choices = [("", "选择分支"), ("main", "main")];
    let label = "src.rs:2-2";
    let document = StyledText::from_parts(
        label,
        vec![],
        vec![StyledLink {
            start: 0,
            end: label.len(),
            href: "review-source:2:2:src.rs".into(),
        }],
    )?;
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_draft("继续保留的草稿".into());
    window.set_review_report_document(document.clone());
    window.model_entries().push(TranscriptEntry {
        kind: "code-review".into(),
        heading: "代码审阅".into(),
        status: "1 处可定位意见".into(),
        status_tone: String::new(),
        body: StyledText::from_plain_text("恢复时同步计数器标签。"),
    });
    for (key, label) in choices {
        window.model_review_branch_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    window.on_load_review_branches(move || output.borrow_mut().push("load".into()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_start_code_review(move |mode, branch, revision| {
        output
            .borrow_mut()
            .push(format!("{mode}:{branch}:{revision}"));
        weak.upgrade().unwrap().set_review_scope_open(false);
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_show_review_report(move |index| {
        output.borrow_mut().push(format!("report:{index}"));
        weak.upgrade().unwrap().set_review_report_open(true);
    });
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_open_review_source(move |href| {
        output.borrow_mut().push(href);
        weak.upgrade().unwrap().set_review_source_open(true);
    });
    exercise_review_scope(window, &calls);
    calls.borrow_mut().clear();

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("draft", "继续保留的草稿".to_owned().into())?;
    instance.set("review-report-document", Value::StyledText(document))?;
    instance.set(
        "entries",
        Value::RecordList(RecordList::new(
            "TranscriptEntry",
            vec![RecordValue::new(
                "TranscriptEntry",
                [
                    ("kind".into(), "code-review".to_owned().into()),
                    ("heading".into(), "代码审阅".to_owned().into()),
                    ("status".into(), "1 处可定位意见".to_owned().into()),
                    ("status-tone".into(), String::new().into()),
                    (
                        "body".into(),
                        Value::StyledText(StyledText::from_plain_text("恢复时同步计数器标签。")),
                    ),
                ]
                .into(),
            )],
        )),
    )?;
    instance.set(
        "review-branch-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            choices
                .into_iter()
                .map(|(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let output = calls.clone();
    instance.on_mut("load-review-branches", move |_| {
        output.borrow_mut().push("load".into());
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("start-code-review", move |args| {
        let [
            Value::String(mode),
            Value::String(branch),
            Value::String(revision),
        ] = args
        else {
            return Err("审阅范围参数无效".into());
        };
        output
            .borrow_mut()
            .push(format!("{mode}:{branch}:{revision}"));
        weak.upgrade()
            .unwrap()
            .set("review-scope-open", false.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("show-review-report", move |args| {
        let [Value::Int(index)] = args else {
            return Err("审阅报告序号无效".into());
        };
        output.borrow_mut().push(format!("report:{index}"));
        weak.upgrade()
            .unwrap()
            .set("review-report-open", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("open-review-source", move |args| {
        let [Value::String(href)] = args else {
            return Err("审阅位置无效".into());
        };
        output.borrow_mut().push(href.clone());
        weak.upgrade()
            .unwrap()
            .set("review-source-open", true.into())
            .map_err(|error| error.to_string())?;
        Ok(None)
    })?;
    exercise_review_scope(instance, &calls);
    Ok(())
}

#[test]
fn fork_dialog_submits_the_selected_turn_in_both_backends() -> Result<(), Box<dyn std::error::Error>>
{
    let choices = [
        ("", "完整会话"),
        ("last", "最近一轮 · 运行示例"),
        ("first", "向前 1 轮 · 核对焦点"),
    ];
    let mut window = AgentWorkbench::new();
    window.set_workspace("/workspace".into());
    window.set_connected(true);
    window.set_renamable(true);
    window.set_worktree_available(true);
    window.set_conversation_title("桌面示例".into());
    for (key, label) in choices {
        window.model_fork_choices().push(GenerationChoice {
            key: key.into(),
            label: label.into(),
        });
    }
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    let weak = window.as_weak();
    window.on_create_fork(move |title, turn, worktree, copy_changes| {
        output
            .borrow_mut()
            .push(format!("{title}:{turn}:{worktree}:{copy_changes}"));
        weak.upgrade().unwrap().set_fork_pending(true);
    });
    exercise_fork_dialog(window, &calls);

    let instance = workbench_program()?.create("AgentWorkbench")?;
    instance.set("workspace", "/workspace".to_owned().into())?;
    instance.set("connected", true.into())?;
    instance.set("renamable", true.into())?;
    instance.set("worktree-available", true.into())?;
    instance.set("conversation-title", "桌面示例".to_owned().into())?;
    instance.set(
        "fork-choices",
        Value::RecordList(RecordList::new(
            "GenerationChoice",
            choices
                .into_iter()
                .map(|(key, label)| {
                    RecordValue::new(
                        "GenerationChoice",
                        [
                            ("key".into(), key.to_owned().into()),
                            ("label".into(), label.to_owned().into()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        )),
    )?;
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let output = calls.clone();
    let weak = instance.as_weak();
    instance.on_mut("create-fork", move |args| {
        if let [
            Value::String(title),
            Value::String(turn),
            Value::Bool(worktree),
            Value::Bool(copy_changes),
        ] = args
        {
            output
                .borrow_mut()
                .push(format!("{title}:{turn}:{worktree}:{copy_changes}"));
            weak.upgrade()
                .unwrap()
                .set("fork-pending", true.into())
                .map_err(|error| error.to_string())?;
        }
        Ok(None)
    })?;
    exercise_fork_dialog(instance, &calls);
    Ok(())
}
