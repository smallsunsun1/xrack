use crate::dispatch;
use session::{Commands, Input, QueuedMessage, Snapshot};
use std::{cell::RefCell, rc::Rc};
use tokio::sync::watch;
use workbench_ui::AgentWorkbench;

struct Selection {
    conversation: i64,
    original: QueuedMessage,
    saved: u64,
}

impl Selection {
    fn is_current(&self, snapshot: &Snapshot) -> bool {
        snapshot.view.conversation == self.conversation
            && snapshot
                .view
                .queued
                .iter()
                .any(|item| item == &self.original)
    }
}

#[derive(Clone, Default)]
pub(super) struct QueuedEditor(Rc<RefCell<Option<Selection>>>);

impl QueuedEditor {
    pub(super) fn attach(
        ui: &mut AgentWorkbench,
        commands: Commands,
        output: watch::Receiver<Snapshot>,
    ) -> Self {
        let editor = Self::default();
        let weak = ui.as_weak();
        let selected = editor.clone();
        let snapshots = output.clone();
        ui.on_open_queued_editor(move |id| {
            let Some(mut ui) = weak.upgrade() else { return };
            let snapshot = snapshots.borrow();
            if !ui.get_can_manage_queue() || snapshot.view.conversation != ui.get_conversation_id()
            {
                return;
            }
            let Some(item) = snapshot.view.queued.iter().find(|item| item.id == id) else {
                return;
            };
            let text = item.editable_text();
            ui.set_queue_edit_text(text.unwrap_or(&item.text).into());
            ui.set_queue_edit_original(text.unwrap_or_default().into());
            ui.set_queue_edit_has_attachments(item.has_attachments());
            ui.set_queue_edit_read_only(text.is_none());
            ui.set_queue_edit_stale(false);
            ui.set_queue_edit_error(String::new());
            selected.0.replace(Some(Selection {
                conversation: snapshot.view.conversation,
                original: item.clone(),
                saved: snapshot.view.queue_edit_saved,
            }));
            ui.set_queue_edit_open(true);
            ui.invoke_focus_queued_edit();
        });

        let weak = ui.as_weak();
        let selected = editor.clone();
        let queue_commands = commands.clone();
        ui.on_save_queued_edit(move || {
            let Some(mut ui) = weak.upgrade() else { return };
            if !ui.get_queue_edit_open() || !ui.get_can_save_queued_edit() {
                return;
            }
            let selection = selected.0.borrow();
            let Some(selection) = selection.as_ref() else {
                return;
            };
            if !selection.is_current(&output.borrow()) {
                ui.set_queue_edit_stale(true);
                return;
            }
            let edit = Input::UpdateQueued {
                conversation: selection.conversation,
                original: selection.original.clone(),
                text: ui.get_queue_edit_text(),
            };
            match queue_commands.send(edit) {
                Ok(()) => {
                    ui.set_queue_edit_error(String::new());
                    ui.set_queue_pending(true);
                }
                Err(error) => ui.set_queue_edit_error(error),
            }
        });

        let weak = ui.as_weak();
        ui.on_move_queued(move |id, step| {
            if let Some(mut ui) = weak.upgrade()
                && ui.get_can_manage_queue()
            {
                let conversation = ui.get_conversation_id();
                if dispatch(
                    &mut ui,
                    &commands,
                    Input::MoveQueued {
                        conversation,
                        id,
                        step,
                    },
                ) {
                    ui.set_queue_pending(true);
                }
            }
        });
        editor
    }

    pub(super) fn sync(
        &self,
        ui: &mut AgentWorkbench,
        snapshot: &Snapshot,
        previous: Option<&Snapshot>,
    ) {
        let mut selection = self.0.borrow_mut();
        let Some(edit) = selection.as_ref() else {
            return;
        };
        if !ui.get_queue_edit_open() || snapshot.view.conversation != edit.conversation {
            ui.set_queue_edit_open(false);
            selection.take();
            return;
        }
        if snapshot.view.queue_edit_saved != edit.saved {
            ui.set_queue_edit_open(false);
            ui.invoke_focus_composer();
            selection.take();
            return;
        }
        if !snapshot.view.queue_loading {
            ui.set_queue_edit_stale(!edit.is_current(snapshot));
        }
        if !snapshot.view.queue_edit_error.is_empty()
            && previous.is_none_or(|old| {
                old.view.queue_edit_error != snapshot.view.queue_edit_error
                    || old.view.queue_pending
            })
        {
            ui.set_queue_edit_error(snapshot.view.queue_edit_error.clone());
        }
    }
}
