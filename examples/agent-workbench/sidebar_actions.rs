use session::{Client, Input, Snapshot};
use std::sync::Arc;
use tokio::sync::watch;
use workbench_ui::{AgentWorkbench, SidebarMenuState};

pub(super) fn update(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    if previous.is_none_or(|prior| {
        !Arc::ptr_eq(&prior.conversation_tools, &snapshot.conversation_tools)
            || prior.conversations != snapshot.conversations
    }) {
        let model = ui.model_sidebar_menus();
        for row in &snapshot.conversations {
            let Ok(index) = usize::try_from(row.id) else {
                continue;
            };
            let tools = snapshot.conversation_tools.get(&row.id);
            let menu = SidebarMenuState {
                archived: row.archived,
                pinned: row.pinned,
                rename: tools.is_some_and(|tools| tools.rename),
                archive: tools.is_some_and(|tools| tools.archive),
                pin: tools.is_some_and(|tools| tools.pin) && !row.archived,
            };
            while model.len() < index {
                model.push(SidebarMenuState::default());
            }
            if model.len() == index {
                model.push(menu);
            } else if model.row(index).as_ref() != Some(&menu) {
                model.set_row(index, menu);
            }
        }
    }
    let status = if snapshot.sidebar_status.is_empty() {
        &snapshot.pin_status
    } else {
        &snapshot.sidebar_status
    };
    ui.set_sidebar_action_status(status.clone());
    if !ui.get_sidebar_rename_open() {
        return;
    }
    let id = ui.get_sidebar_rename_id();
    let Some(tools) = snapshot.conversation_tools.get(&id) else {
        ui.set_sidebar_rename_open(false);
        return;
    };
    if ui.get_sidebar_rename_pending() && !tools.rename_pending && tools.rename_error.is_empty() {
        ui.set_sidebar_rename_open(false);
    }
    ui.set_sidebar_rename_pending(tools.rename_pending);
    ui.set_sidebar_rename_error(tools.rename_error.clone());
}

pub(super) fn attach(ui: &mut AgentWorkbench, client: &Client, output: watch::Receiver<Snapshot>) {
    let state = output.clone();
    let commands = client.commands();
    let weak = ui.as_weak();
    ui.on_manage_sidebar_session(move |id, action| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_switching() {
            return;
        }
        let snapshot = state.borrow();
        let Some(row) = snapshot.conversations.iter().find(|row| row.id == id) else {
            return;
        };
        let Some(tools) = snapshot.conversation_tools.get(&id) else {
            return;
        };
        match action.as_str() {
            "rename" if tools.rename => {
                ui.set_sidebar_rename_id(id);
                ui.set_sidebar_rename_draft(row.title.clone());
                ui.set_sidebar_rename_pending(false);
                ui.set_sidebar_rename_error(String::new());
                ui.invoke_show_sidebar_rename();
            }
            "pin" if tools.pin && !row.archived => {
                crate::dispatch(
                    &mut ui,
                    &commands,
                    Input::SetPinned {
                        conversation: id,
                        pinned: !row.pinned,
                    },
                );
            }
            "archive" if tools.archive => {
                crate::dispatch(
                    &mut ui,
                    &commands,
                    Input::SetConversationArchived {
                        conversation: id,
                        archived: !row.archived,
                    },
                );
            }
            _ => ui.set_sidebar_action_status("当前无法操作该会话".into()),
        }
    });
    let commands = client.commands();
    let weak = ui.as_weak();
    ui.on_rename_sidebar_session(move |name| {
        let Some(mut ui) = weak.upgrade() else { return };
        let id = ui.get_sidebar_rename_id();
        if !ui.get_sidebar_rename_open() || ui.get_sidebar_rename_pending() {
            return;
        }
        if !output
            .borrow()
            .conversation_tools
            .get(&id)
            .is_some_and(|tools| tools.rename)
        {
            ui.set_sidebar_rename_error("当前无法重命名，可稍后重试".into());
            return;
        }
        if crate::dispatch(
            &mut ui,
            &commands,
            Input::RenameConversation {
                conversation: id,
                name,
            },
        ) {
            ui.set_sidebar_rename_pending(true);
            ui.set_sidebar_rename_error(String::new());
        }
    });
}
