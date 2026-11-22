use crate::{attachments::Attachments, review};
use session::{DraftContent, DraftStore, SavedDraft, Snapshot};
use std::{cell::RefCell, rc::Rc};
use workbench_ui::AgentWorkbench;

pub(super) struct ComposerDrafts {
    store: DraftStore,
    current: Option<SavedDraft>,
}

impl ComposerDrafts {
    pub(super) fn attach(
        ui: &mut AgentWorkbench,
        attachments: &Attachments,
        store: DraftStore,
    ) -> Rc<RefCell<Self>> {
        let state = Rc::new(RefCell::new(Self {
            store,
            current: None,
        }));
        let drafts = state.clone();
        let weak = ui.as_weak();
        let attachments = attachments.clone();
        ui.on_save_composer_draft(move || {
            if let Some(mut ui) = weak.upgrade() {
                drafts.borrow_mut().save(&mut ui, &attachments);
            }
        });
        state
    }

    pub(super) fn show(
        &mut self,
        ui: &mut AgentWorkbench,
        attachments: &Attachments,
        snapshot: &Snapshot,
        restore: bool,
    ) {
        if !snapshot.composer_ready || !self.store.enabled() {
            return;
        }
        if snapshot.view.workspace.is_empty() {
            self.current = None;
            ui.set_draft_store_ready(false);
            ui.set_draft_storage_status(String::new());
            return;
        }
        if let Some(current) = &mut self.current
            && current.workspace == snapshot.view.workspace
            && current.project == snapshot.project_workspace()
            && ui.get_draft_store_ready()
            && !restore
        {
            current.key = snapshot.view.draft_key.clone();
            return;
        }
        let mut draft = SavedDraft {
            key: snapshot.view.draft_key.clone(),
            workspace: snapshot.view.workspace.clone(),
            project: snapshot.project_workspace().to_owned(),
            content: DraftContent::default(),
        };
        let mut status = String::new();
        if restore {
            match self.store.records() {
                Ok(records) => {
                    if let Some(record) = records.into_iter().find(|saved| {
                        saved.key == draft.key
                            && saved.workspace == draft.workspace
                            && saved.project == draft.project
                    }) {
                        draft.content = record.content;
                        if capture(ui, attachments).is_empty() {
                            ui.set_draft(draft.content.text);
                            attachments.restore_saved(ui, draft.content.attachments);
                            review::restore_notes(ui, draft.content.review_notes);
                            draft.content = capture(ui, attachments);
                            status = "已恢复草稿".into();
                        }
                    }
                }
                Err(error) => status = format!("读取草稿失败（{error}）"),
            }
        }
        self.current = Some(draft);
        ui.set_draft_storage_status(status);
        ui.set_draft_store_ready(true);
        self.save(ui, attachments);
        ui.invoke_draft_checkpoint();
    }

    pub(super) fn leave(&mut self, ui: &mut AgentWorkbench, attachments: &Attachments) {
        self.save(ui, attachments);
        self.current = None;
        ui.set_draft_store_ready(false);
    }

    pub(super) fn save(&mut self, ui: &mut AgentWorkbench, attachments: &Attachments) {
        let Some(current) = &mut self.current else {
            return;
        };
        let content = capture(ui, attachments);
        if current.content == content {
            return;
        }
        let next = SavedDraft {
            content,
            ..current.clone()
        };
        match self.store.save(next.clone()) {
            Ok(()) => {
                *current = next;
                ui.set_draft_storage_status(String::new());
            }
            Err(error) => ui.set_draft_storage_status(format!("保存草稿失败（{error}）")),
        }
    }
}

fn capture(ui: &AgentWorkbench, attachments: &Attachments) -> DraftContent {
    let prompt = attachments.prompt(ui.get_draft());
    DraftContent {
        text: prompt.text,
        attachments: prompt.attachments,
        review_notes: review::capture_notes(ui),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::{Attachment, AttachmentContent, DraftKey, SavedReviewNote};

    #[test]
    fn a_draft_with_only_review_notes_reopens_with_its_original_references() -> std::io::Result<()>
    {
        let directory = tempfile::tempdir()?;
        let profile = directory.path().canonicalize()?;
        let store = DraftStore::new(Some(profile.clone()));
        let mut snapshot = Snapshot::connecting(profile.to_string_lossy().into_owned());
        snapshot.composer_ready = true;
        snapshot.view.draft_key = DraftKey::Thread("review-thread".into());
        let note = SavedReviewNote {
            id: 2,
            reference: "src/lib.rs · 旧行 10 · 未暂存".into(),
            text: "保留原来的错误路径".into(),
        };
        let mut ui = AgentWorkbench::new();
        let attachments = crate::attachments::attach(&mut ui);
        let drafts = ComposerDrafts::attach(&mut ui, &attachments, store.clone());
        drafts
            .borrow_mut()
            .show(&mut ui, &attachments, &snapshot, true);
        review::restore_notes(&ui, vec![note.clone()]);
        ui.invoke_draft_review_notes_changed();
        ui.invoke_save_composer_draft();
        assert_eq!(store.records()?[0].content.review_notes, [note.clone()]);
        assert!(store.records()?[0].content.text.is_empty());

        let mut restarted = AgentWorkbench::new();
        let attachments = crate::attachments::attach(&mut restarted);
        let drafts = ComposerDrafts::attach(&mut restarted, &attachments, store);
        drafts
            .borrow_mut()
            .show(&mut restarted, &attachments, &snapshot, true);
        assert!(restarted.get_draft().is_empty());
        assert_eq!(restarted.get_draft_storage_status(), "已恢复草稿");
        assert_eq!(review::capture_notes(&restarted), [note]);
        Ok(())
    }

    #[test]
    fn reopening_a_thread_restores_its_draft_and_a_rejected_send_keeps_it() -> std::io::Result<()> {
        let profile = tempfile::tempdir()?;
        let directory = profile.path().canonicalize()?;
        let store = DraftStore::new(Some(directory.clone()));
        let source = directory.join("notes.txt");
        std::fs::write(&source, "new text on disk")?;
        let source = Attachment {
            path: source.to_string_lossy().into_owned(),
            content: AttachmentContent::Text("attached before restart".into()),
        };
        let mut snapshot = Snapshot::connecting(directory.to_string_lossy().into_owned());
        snapshot.composer_ready = true;
        snapshot.view.draft_key = DraftKey::Thread("persisted-thread".into());
        let mut ui = AgentWorkbench::new();
        let attachments = crate::attachments::attach(&mut ui);
        let drafts = ComposerDrafts::attach(&mut ui, &attachments, store.clone());
        drafts
            .borrow_mut()
            .show(&mut ui, &attachments, &snapshot, true);
        ui.set_draft("finish after restarting".into());
        attachments.restore_saved(&mut ui, vec![source.clone()]);
        ui.invoke_save_composer_draft();

        let mut ui = AgentWorkbench::new();
        let attachments = crate::attachments::attach(&mut ui);
        let drafts = ComposerDrafts::attach(&mut ui, &attachments, store.clone());
        drafts
            .borrow_mut()
            .show(&mut ui, &attachments, &snapshot, true);
        assert_eq!(ui.get_draft(), "finish after restarting");
        assert_eq!(ui.get_draft_storage_status(), "已恢复草稿");
        assert_eq!(capture(&ui, &attachments).attachments, [source]);

        ui.set_submitted_draft(ui.get_draft());
        ui.set_sending_prompt(true);
        let mut pending = snapshot.clone();
        pending.view.prompt_pending = true;
        crate::update_followups(&mut ui, &pending, Some(&snapshot));
        assert!(ui.get_sending_prompt());
        crate::update_followups(&mut ui, &snapshot, Some(&pending));
        ui.invoke_save_composer_draft();
        assert_eq!(store.records()?.len(), 1);
        assert_eq!(capture(&ui, &attachments).attachments.len(), 1);

        ui.set_submitted_draft(ui.get_draft());
        ui.set_sending_prompt(true);
        snapshot.view.sent_prompts += 1;
        crate::update_followups(&mut ui, &snapshot, Some(&pending));
        ui.invoke_save_composer_draft();
        assert!(capture(&ui, &attachments).is_empty());
        assert!(store.records()?.is_empty());
        Ok(())
    }
}
