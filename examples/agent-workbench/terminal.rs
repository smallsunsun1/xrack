mod actions;
mod process;
mod screen;

use crate::attachments::{Attachments, TEXT_FILE_LIMIT};
use portable_pty::{CommandBuilder, PtySize};
use process::{Input, Session};
use screen::{CURSOR_MARK, Frame};
use session::TerminalOutput;
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};
use workbench_ui::{AgentWorkbench, TerminalTab};
use xrack_runtime::{EventResult, Length};

const PASTE_LIMIT: usize = 16 * 1024;
type Sessions = Rc<RefCell<HashMap<String, Workspace>>>;

struct Tab {
    row: TerminalTab,
    session: Option<Rc<Session>>,
    failure: String,
    action: Option<actions::Action>,
}

#[derive(Default)]
struct Workspace {
    tabs: Vec<Tab>,
    selected: Option<usize>,
    sequence: u64,
}

impl Workspace {
    fn new_tab(&mut self) {
        self.add_tab(None);
    }

    fn add_tab(&mut self, action: Option<actions::Action>) {
        self.sequence += 1;
        self.selected = Some(self.tabs.len());
        self.tabs.push(Tab {
            row: TerminalTab {
                id: self.sequence.to_string(),
                title: action.as_ref().map_or_else(
                    || format!("Shell {}", self.sequence),
                    |action| action.name.clone(),
                ),
            },
            session: None,
            failure: String::new(),
            action,
        });
    }

    fn select(&mut self, id: &str) -> bool {
        if let Some(index) = self.tabs.iter().position(|tab| tab.row.id == id) {
            self.selected = Some(index);
            true
        } else {
            false
        }
    }

    fn close(&mut self, id: &str) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.row.id == id) else {
            return false;
        };
        self.tabs.remove(index);
        self.selected = self.selected.and_then(|selected| {
            if self.tabs.is_empty() {
                None
            } else if selected > index {
                Some(selected - 1)
            } else {
                Some(selected.min(self.tabs.len() - 1))
            }
        });
        true
    }

    fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let selected = self.active().map(|tab| tab.row.id.clone());
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if let Some(id) = selected {
            self.select(&id);
        }
    }

    fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.selected?)
    }
}

fn publish(ui: &mut AgentWorkbench, frame: &Frame) {
    let text = frame.screen.text();
    let cursor = frame.cursor_byte;
    let has_output = cursor.map_or_else(
        || !text.trim().is_empty(),
        |offset| {
            !text[..offset].trim().is_empty()
                || !text[offset + CURSOR_MARK.len_utf8()..].trim().is_empty()
        },
    );
    ui.set_terminal_screen(frame.screen.clone());
    ui.set_terminal_cursor_byte(cursor.map_or(-1, |offset| offset as i64));
    ui.set_terminal_output_available(has_output);
    ui.set_terminal_status(frame.status.clone());
    ui.set_terminal_running(frame.running);
    ui.set_terminal_history(frame.history);
}

fn publish_tabs(ui: &mut AgentWorkbench, workspace: &Workspace) {
    let model = ui.model_terminal_tabs();
    for (index, tab) in workspace.tabs.iter().enumerate() {
        if index == model.len() {
            model.push(tab.row.clone());
        } else if model.row(index).as_ref() != Some(&tab.row) {
            if let Some(previous) = (index + 1..model.len())
                .find(|&slot| model.row(slot).is_some_and(|row| row.id == tab.row.id))
            {
                model.move_row(previous, index);
            } else {
                model.insert(index, tab.row.clone());
            }
        }
    }
    while model.len() > workspace.tabs.len() {
        model.remove(model.len() - 1);
    }
    ui.set_terminal_tab(
        workspace
            .active()
            .map_or_else(String::new, |tab| tab.row.id.clone()),
    );
}

fn size(width: Length, height: Length) -> PtySize {
    if width.0 <= 0.0 || height.0 <= 0.0 {
        return PtySize {
            rows: 12,
            cols: 80,
            ..PtySize::default()
        };
    }
    PtySize {
        rows: ((height.0 - 8.0) / 17.0).clamp(4.0, 80.0) as u16,
        cols: ((width.0 - 18.0) / 8.0).clamp(20.0, 240.0) as u16,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn selected(ui: &AgentWorkbench, workspace: &str, id: &str) -> bool {
    ui.get_terminal_open() && ui.get_workspace() == workspace && ui.get_terminal_tab() == id
}

fn observe(
    ui: &AgentWorkbench,
    workspace: String,
    id: String,
    session: &Rc<Session>,
) -> Result<(), String> {
    let weak = ui.as_weak();
    let alive = Rc::downgrade(session);
    let mut updates = session.output.clone();
    xrack_platform_winit::spawn_local(async move {
        loop {
            let Some(mut ui) = weak.upgrade() else { return };
            if alive.strong_count() == 0 {
                return;
            }
            let frame = updates.borrow_and_update().clone();
            if selected(&ui, &workspace, &id) {
                publish(&mut ui, &frame);
            }
            drop(ui);
            if updates.changed().await.is_err() {
                return;
            }
        }
    })
    .map(drop)
    .map_err(|error| error.to_string())
}

fn start(ui: &AgentWorkbench, workspace: &str, tab: &Tab) -> Result<Rc<Session>, String> {
    let mut command = tab.action.as_ref().map_or_else(
        || {
            let mut shell = CommandBuilder::new_default_prog();
            shell.cwd(Path::new(workspace));
            shell
        },
        actions::Action::command,
    );
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    let requested = size(ui.get_terminal_width(), ui.get_terminal_height());
    let session = Rc::new(Session::start(command, requested)?);
    observe(ui, workspace.to_owned(), tab.row.id.clone(), &session)?;
    Ok(session)
}

fn show_active(ui: &mut AgentWorkbench, workspace: &str, state: &mut Workspace, restart: bool) {
    publish_tabs(ui, state);
    let Some(tab) = state.selected.and_then(|index| state.tabs.get_mut(index)) else {
        publish(ui, &Frame::status(""));
        ui.set_terminal_open(false);
        return;
    };
    ui.set_terminal_input_buffer(String::new());
    if restart {
        tab.session.take();
        tab.failure.clear();
    }
    if tab.session.is_none() && tab.failure.is_empty() {
        match start(ui, workspace, tab) {
            Ok(session) => tab.session = Some(session),
            Err(error) => tab.failure = error,
        }
    }
    if let Some(session) = &tab.session {
        publish(ui, &session.output.borrow());
        // Queue the size even while the Shell is starting.
        let requested = size(ui.get_terminal_width(), ui.get_terminal_height());
        if session.output.has_changed().is_ok() {
            let _ = session.send(Input::Resize(requested));
        }
    } else {
        publish(ui, &Frame::status(&tab.failure));
    }
}

fn active_session(ui: &AgentWorkbench, sessions: &Sessions) -> Option<Rc<Session>> {
    let states = sessions.borrow();
    let tab = states.get(&ui.get_workspace())?.active()?;
    if ui.get_terminal_open() && !ui.get_switching() && tab.row.id == ui.get_terminal_tab() {
        tab.session.clone()
    } else {
        None
    }
}

fn send(ui: &mut AgentWorkbench, terminal: &Session, input: Input) {
    if let Err(error) = terminal.send(input) {
        ui.set_terminal_status(error);
    }
}

fn output_excerpt(
    screen: &str,
    cursor: Option<usize>,
    start: i64,
    end: i64,
    selection: &str,
) -> Result<(String, bool), &'static str> {
    let range = if selection.is_empty() {
        0..screen.len()
    } else {
        let start = usize::try_from(start).map_err(|_| "终端选区无效")?;
        let end = usize::try_from(end).map_err(|_| "终端选区无效")?;
        if screen.get(start..end) != Some(selection) {
            return Err("终端输出已更新，请重新选取");
        }
        start..end
    };
    let mut text = screen[range.clone()].to_owned();
    if let Some(offset) = cursor.and_then(|cursor| cursor.checked_sub(range.start))
        && text
            .get(offset..)
            .is_some_and(|tail| tail.starts_with(CURSOR_MARK))
    {
        text.remove(offset);
    }
    let text = text.trim_matches('\n');
    if text.trim().is_empty() {
        return Err("当前没有可引用的终端输出");
    }
    let truncated = text.len() > TEXT_FILE_LIMIT;
    let from = text.ceil_char_boundary(text.len().saturating_sub(TEXT_FILE_LIMIT));
    Ok((text[from..].to_owned(), truncated))
}

fn attach_output(
    ui: &mut AgentWorkbench,
    sessions: &Sessions,
    attachments: &Attachments,
    start: i64,
    end: i64,
    selection: &str,
) {
    let workspace = ui.get_workspace();
    let states = sessions.borrow();
    let Some(tab) = states.get(&workspace).and_then(Workspace::active) else {
        return;
    };
    if !selected(ui, &workspace, &tab.row.id) || ui.get_switching() {
        return;
    }
    let title = tab.row.title.clone();
    drop(states);
    let screen = ui.get_terminal_screen();
    let cursor = usize::try_from(ui.get_terminal_cursor_byte()).ok();
    match output_excerpt(screen.text(), cursor, start, end, selection) {
        Ok((contents, truncated)) => {
            let output = TerminalOutput {
                title,
                contents,
                selected: !selection.is_empty(),
                history: ui.get_terminal_history(),
                truncated,
            };
            if attachments.add_terminal_output(ui, workspace, output) {
                ui.invoke_focus_composer();
            }
        }
        Err(error) => ui.set_attachment_status(error.into()),
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench, attachments: &Attachments) {
    let sessions = Sessions::default();
    let state = sessions.clone();
    actions::attach(ui, move |ui, action| {
        let workspace = ui.get_workspace();
        ui.set_project_actions_open(false);
        ui.invoke_show_terminal_panel();
        let mut states = state.borrow_mut();
        let tabs = states.entry(workspace.clone()).or_default();
        tabs.add_tab(Some(action));
        show_active(ui, &workspace, tabs, false);
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    let attachments = attachments.clone();
    ui.on_attach_terminal_output(move |start, end, selection| {
        if let Some(mut ui) = weak.upgrade() {
            attach_output(&mut ui, &state, &attachments, start, end, &selection);
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_sync_terminal(move |restart| {
        let Some(mut ui) = weak.upgrade() else { return };
        let workspace = ui.get_workspace();
        if !ui.get_terminal_open() || workspace.is_empty() || (restart && ui.get_switching()) {
            return;
        }
        let mut states = state.borrow_mut();
        let tabs = states.entry(workspace.clone()).or_default();
        if tabs.tabs.is_empty() {
            tabs.new_tab();
        }
        show_active(&mut ui, &workspace, tabs, restart);
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_new_terminal_tab(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let workspace = ui.get_workspace();
        if !ui.get_terminal_open() || workspace.is_empty() || ui.get_switching() {
            return;
        }
        let mut states = state.borrow_mut();
        let tabs = states.entry(workspace.clone()).or_default();
        tabs.new_tab();
        show_active(&mut ui, &workspace, tabs, false);
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_select_terminal_tab(move |id| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_terminal_open() || ui.get_switching() {
            return;
        }
        let workspace = ui.get_workspace();
        if let Some(tabs) = state.borrow_mut().get_mut(&workspace) {
            if tabs.select(&id) {
                show_active(&mut ui, &workspace, tabs, false);
            } else {
                publish_tabs(&mut ui, tabs);
            }
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_cycle_terminal_tab(move |step| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_terminal_open() || ui.get_switching() {
            return;
        }
        let workspace = ui.get_workspace();
        if let Some(tabs) = state.borrow_mut().get_mut(&workspace)
            && let Some(index) = tabs.selected
            && tabs.tabs.len() > 1
        {
            let next = (index as i64 + step.signum()).rem_euclid(tabs.tabs.len() as i64);
            tabs.selected = Some(next as usize);
            show_active(&mut ui, &workspace, tabs, false);
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_close_terminal_tab(move |id| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_terminal_open() || ui.get_switching() {
            return;
        }
        let workspace = ui.get_workspace();
        if let Some(tabs) = state.borrow_mut().get_mut(&workspace)
            && tabs.close(&id)
        {
            show_active(&mut ui, &workspace, tabs, false);
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_reorder_terminal_tabs(move |from, to| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_terminal_open() || ui.get_switching() {
            return;
        }
        if let (Ok(from), Ok(to), Some(tabs)) = (
            usize::try_from(from),
            usize::try_from(to),
            state.borrow_mut().get_mut(&ui.get_workspace()),
        ) {
            tabs.reorder(from, to);
            publish_tabs(&mut ui, tabs);
        }
    });

    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_terminal_resize(move |width, height| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(session) = active_session(&ui, &state)
            && session.output.borrow().running
        {
            send(&mut ui, &session, Input::Resize(size(width, height)));
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_terminal_key(move |event| {
        let Some(mut ui) = weak.upgrade() else {
            return EventResult::Reject;
        };
        let Some(session) = active_session(&ui, &state) else {
            return EventResult::Reject;
        };
        let frame = session.output.borrow();
        if frame.running
            && let Some(input) = process::key_input(&event, frame.keyboard)
        {
            send(&mut ui, &session, input);
            EventResult::Accept
        } else {
            EventResult::Reject
        }
    });
    let state = sessions.clone();
    let weak = ui.as_weak();
    ui.on_terminal_text(move |text| {
        let Some(mut ui) = weak.upgrade() else { return };
        let Some(session) = active_session(&ui, &state) else {
            return;
        };
        let frame = session.output.borrow();
        if !frame.running || text.is_empty() {
            return;
        }
        if text.len() > PASTE_LIMIT {
            ui.set_terminal_status("单次终端粘贴最多 16 KiB".into());
            return;
        }
        let paste = if frame.keyboard.bracketed_paste && text.chars().count() > 1 {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        };
        send(&mut ui, &session, Input::Text(paste.into_bytes()));
    });
    let weak = ui.as_weak();
    ui.on_terminal_scroll(move |pages| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(session) = active_session(&ui, &sessions)
        {
            let rows = i32::from(session.output.borrow().keyboard.rows);
            send(
                &mut ui,
                &session,
                Input::Scroll(pages.signum() as i32 * rows),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::{AttachmentContent, DraftStore, Snapshot};
    use xrack_runtime::StyledText;

    #[test]
    fn excerpts_keep_utf8_and_actual_output_while_removing_only_the_cursor() {
        let text = "中文▏\nlast output▏\n";
        let cursor = text.rfind(CURSOR_MARK);
        assert_eq!(
            output_excerpt(text, cursor, 0, 0, ""),
            Ok(("中文▏\nlast output".into(), false))
        );
        assert_eq!(
            output_excerpt(text, cursor, 0, 6, "中文"),
            Ok(("中文".into(), false))
        );
        assert!(output_excerpt(text, cursor, 0, 6, "更早").is_err());
        assert!(output_excerpt("▏\n", Some(0), 0, 0, "").is_err());
        let long = format!("{}\nfinal", "🦀".repeat(TEXT_FILE_LIMIT));
        let (excerpt, truncated) = output_excerpt(&long, None, 0, 0, "").unwrap();
        assert!(truncated && excerpt.len() <= TEXT_FILE_LIMIT);
        assert!(excerpt.ends_with("\nfinal"));
    }

    #[test]
    fn captures_the_selected_tab_in_a_restartable_draft_and_previews_the_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let workspace = root.to_string_lossy().into_owned();
        let mut snapshot = Snapshot::connecting(workspace.clone());
        snapshot.composer_ready = true;
        let store = DraftStore::new(Some(root.join("profile")));
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(workspace.clone());
        ui.set_terminal_open(true);
        let attachments = crate::attachments::attach(&mut ui);
        let drafts =
            crate::composer_drafts::ComposerDrafts::attach(&mut ui, &attachments, store.clone());
        drafts
            .borrow_mut()
            .show(&mut ui, &attachments, &snapshot, true);

        let sessions = Sessions::default();
        let mut tabs = Workspace::default();
        tabs.new_tab();
        tabs.new_tab();
        publish_tabs(&mut ui, &tabs);
        sessions.borrow_mut().insert(workspace.clone(), tabs);
        ui.set_terminal_screen(StyledText::from_plain_text("check failed 中文\nShell ▏\n"));
        ui.set_terminal_history(true);
        attach_output(&mut ui, &sessions, &attachments, 0, 19, "check failed 中文");
        assert_eq!(ui.model_attachment_rows().row(0).unwrap().name, "Shell 2");
        assert_eq!(ui.model_attachment_rows().row(0).unwrap().kind, "终端选段");
        assert!(ui.model_attachment_rows().row(0).unwrap().has_preview);
        ui.set_terminal_screen(StyledText::from_plain_text("a new run"));
        ui.invoke_save_composer_draft();

        let mut resumed = AgentWorkbench::new();
        let attachments = crate::attachments::attach(&mut resumed);
        let drafts =
            crate::composer_drafts::ComposerDrafts::attach(&mut resumed, &attachments, store);
        drafts
            .borrow_mut()
            .show(&mut resumed, &attachments, &snapshot, true);
        let captured = attachments.prompt(String::new());
        assert!(matches!(
            &captured.attachments[0].content,
            AttachmentContent::Terminal(output)
                if output.title == "Shell 2" && output.contents == "check failed 中文"
                    && output.history && output.selected
        ));
        let row = resumed.model_attachment_rows().row(0).unwrap();
        resumed.invoke_preview_attachment(row.path.clone());
        assert!(resumed.get_attachment_preview_open() && resumed.get_attachment_preview_is_text());
        assert_eq!(
            resumed.get_attachment_preview_text().text(),
            "check failed 中文"
        );
        resumed.invoke_remove_attachment(row.path);
        assert!(attachments.prompt(String::new()).attachments.is_empty());
        Ok(())
    }

    #[test]
    fn tabs_keep_selection_and_row_identity_through_reorder_and_close() {
        let mut ui = AgentWorkbench::new();
        let mut workspace = Workspace::default();
        workspace.new_tab();
        workspace.new_tab();
        workspace.new_tab();
        let model = ui.model_terminal_tabs();
        publish_tabs(&mut ui, &workspace);
        let key = model.row_key(1);
        assert!(workspace.select("2"));
        workspace.reorder(1, 0);
        publish_tabs(&mut ui, &workspace);
        assert_eq!(ui.get_terminal_tab(), "2");
        assert_eq!(model.row_key(0), key);
        assert!(workspace.close("1"));
        publish_tabs(&mut ui, &workspace);
        assert_eq!(ui.get_terminal_tab(), "2");
        assert_eq!(model.row_key(0), key);
        assert!(workspace.close("2"));
        assert_eq!(workspace.active().unwrap().row.id, "3");
        assert!(workspace.close("3"));
        assert!(workspace.active().is_none());
        workspace.new_tab();
        assert_eq!(workspace.active().unwrap().row.id, "4");
    }

    #[cfg(unix)]
    #[test]
    fn independent_pty_tabs_keep_background_output_and_route_only_selected_input()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            thread,
            time::{Duration, Instant},
        };
        let scratch = tempfile::tempdir()?;
        let workspace = scratch
            .path()
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        let mut tabs = Workspace::default();
        for _ in 0..2 {
            tabs.new_tab();
            let mut command = CommandBuilder::new("/bin/sh");
            command.arg("-i");
            command.cwd(&workspace);
            command.env("PS1", "");
            command.env_remove("ENV");
            tabs.tabs.last_mut().unwrap().session =
                Some(Rc::new(Session::start(command, PtySize::default())?));
        }
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(workspace.clone());
        ui.set_terminal_open(true);
        publish_tabs(&mut ui, &tabs);
        assert!(!selected(&ui, &workspace, "1"));
        assert!(selected(&ui, &workspace, "2"));

        let first = tabs.tabs[0].session.clone().unwrap();
        let second = tabs.tabs[1].session.clone().unwrap();
        first.send(Input::Text(b"printf '%s%s\\n' LEFT -RESULT\r".to_vec()))?;
        second.send(Input::Text(b"printf '%s%s\\n' RIGHT -RESULT\r".to_vec()))?;
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let left_frame = first.output.borrow();
            let right_frame = second.output.borrow();
            let left = left_frame.screen.text();
            let right = right_frame.screen.text();
            if left.contains("LEFT-RESULT") && right.contains("RIGHT-RESULT") {
                assert!(!left.contains("RIGHT-RESULT"));
                assert!(!right.contains("LEFT-RESULT"));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "left: {left:?}; right: {right:?}"
            );
            drop(left_frame);
            drop(right_frame);
            thread::sleep(Duration::from_millis(12));
        }
        let sessions = Sessions::default();
        sessions.borrow_mut().insert(workspace.clone(), tabs);
        assert!(Rc::ptr_eq(
            &active_session(&ui, &sessions).unwrap(),
            &second
        ));
        {
            let mut workspaces = sessions.borrow_mut();
            let tabs = workspaces.get_mut(&workspace).unwrap();
            assert!(tabs.select("1"));
            show_active(&mut ui, &workspace, tabs, false);
        }
        assert!(Rc::ptr_eq(&active_session(&ui, &sessions).unwrap(), &first));
        assert!(ui.get_terminal_screen().text().contains("LEFT-RESULT"));
        let closing = first.output.clone();
        {
            let mut workspaces = sessions.borrow_mut();
            let tabs = workspaces.get_mut(&workspace).unwrap();
            assert!(tabs.close("1"));
            show_active(&mut ui, &workspace, tabs, false);
        }
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(6);
        while closing.has_changed().is_ok() {
            assert!(
                Instant::now() < deadline,
                "closed tab should release its PTY"
            );
            thread::sleep(Duration::from_millis(12));
        }
        assert!(Rc::ptr_eq(
            &active_session(&ui, &sessions).unwrap(),
            &second
        ));
        assert!(ui.get_terminal_screen().text().contains("RIGHT-RESULT"));
        ui.set_workspace("/other".into());
        assert!(active_session(&ui, &sessions).is_none());
        assert!(!selected(&ui, &workspace, "1"));
        Ok(())
    }
}
