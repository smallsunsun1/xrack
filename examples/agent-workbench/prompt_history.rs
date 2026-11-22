use session::{Entry, Snapshot};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};
use tokio::sync::watch;
use workbench_ui::AgentWorkbench;

struct Submission {
    prompt: String,
    latest_entry: Option<Arc<Entry>>,
}

struct Browse {
    conversation: i64,
    prompts: Vec<String>,
    position: usize,
}

#[derive(Default)]
struct PromptHistory {
    submitted: HashMap<i64, Submission>,
    browsing: Option<Browse>,
}

fn latest_user_message(snapshot: &Snapshot) -> Option<&Arc<Entry>> {
    snapshot
        .view
        .entries
        .iter()
        .rev()
        .find(|entry| matches!(entry.as_ref(), Entry::User(_)))
}

fn available_prompts(snapshot: &Snapshot, submission: Option<&Submission>) -> Vec<String> {
    let latest = latest_user_message(snapshot);
    let recent = submission.filter(|submission| match (&submission.latest_entry, latest) {
        (Some(remembered), Some(latest)) => Arc::ptr_eq(remembered, latest),
        (None, None) => true,
        _ => false,
    });
    let loaded = snapshot
        .view
        .entries
        .iter()
        .rev()
        .filter_map(|entry| match entry.as_ref() {
            Entry::User(message) => Some(message.prompt_text()),
            _ => None,
        });
    let mut seen = HashSet::new();
    recent
        .into_iter()
        .map(|submitted| submitted.prompt.as_str())
        .chain(loaded)
        .filter(|prompt| !prompt.trim().is_empty() && seen.insert(*prompt))
        .map(str::to_owned)
        .collect()
}

impl PromptHistory {
    fn browse(&mut self, ui: &mut AgentWorkbench, snapshot: &Snapshot, step: i64) -> bool {
        let conversation = ui.get_conversation_id();
        if ui.get_switching()
            || snapshot.view.conversation != conversation
            || snapshot.view.workspace != ui.get_workspace()
        {
            return false;
        }
        let draft = ui.get_draft();
        if self.browsing.as_ref().is_some_and(|browse| {
            browse.conversation != conversation || browse.prompts[browse.position] != draft
        }) {
            self.browsing = None;
            ui.set_composer_recalling(false);
        }
        if self.browsing.is_none() {
            if step <= 0 || !draft.is_empty() {
                return false;
            }
            let prompts = available_prompts(snapshot, self.submitted.get(&conversation));
            if prompts.is_empty() {
                return false;
            }
            self.browsing = Some(Browse {
                conversation,
                prompts,
                position: 0,
            });
        } else if step == 0
            || step < 0
                && self
                    .browsing
                    .as_ref()
                    .is_some_and(|browse| browse.position == 0)
        {
            self.browsing = None;
        } else if let Some(browse) = &mut self.browsing {
            browse.position = if step > 0 {
                (browse.position + 1).min(browse.prompts.len() - 1)
            } else {
                browse.position - 1
            };
        }
        let draft = self
            .browsing
            .as_ref()
            .map_or("", |browse| browse.prompts[browse.position].as_str());
        let end = draft.len() as i64;
        ui.set_composer_recalling(self.browsing.is_some());
        ui.set_composer_recall_caret(end);
        ui.set_draft(draft.to_owned());
        ui.invoke_restore_composer_caret(end);
        true
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench, output: watch::Receiver<Snapshot>) {
    let state = Rc::new(RefCell::new(PromptHistory::default()));
    let weak = ui.as_weak();
    let recent = state.clone();
    let submissions = output.clone();
    ui.on_remember_composer_prompt(move |prompt| {
        let Some(ui) = weak.upgrade() else { return };
        let snapshot = submissions.borrow();
        if ui.get_conversation_id() == snapshot.view.conversation
            && ui.get_workspace() == snapshot.view.workspace
            && !prompt.trim().is_empty()
        {
            recent.borrow_mut().submitted.insert(
                snapshot.view.conversation,
                Submission {
                    prompt,
                    latest_entry: latest_user_message(&snapshot).cloned(),
                },
            );
        }
    });

    let weak = ui.as_weak();
    let browsing = state.clone();
    ui.on_recall_composer_prompt(move |step| {
        weak.upgrade().is_some_and(|mut ui| {
            browsing
                .borrow_mut()
                .browse(&mut ui, &output.borrow(), step)
        })
    });

    let weak = ui.as_weak();
    ui.on_end_composer_recall(move || {
        state.borrow_mut().browsing = None;
        if let Some(mut ui) = weak.upgrade() {
            ui.set_composer_recalling(false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recalls_a_confirmed_followup_then_loaded_history_and_protects_new_edits() {
        let mut snapshot = Snapshot::connecting("/workspace".into());
        let first = Arc::new(Entry::User("第一条".into()));
        let latest = Arc::new(Entry::User("最新\n请看这个改动".into()));
        snapshot.view.entries = vec![first, latest];
        let (output, receiver) = watch::channel(snapshot.clone());
        let mut ui = AgentWorkbench::new();
        ui.set_workspace("/workspace".into());
        attach(&mut ui, receiver);
        ui.invoke_remember_composer_prompt("已加入队列".into());

        assert!(ui.invoke_recall_composer_prompt(1));
        assert_eq!(ui.get_draft(), "已加入队列");
        assert!(ui.invoke_recall_composer_prompt(1));
        assert_eq!(ui.get_draft(), "最新\n请看这个改动");
        assert_eq!(
            ui.get_composer_recall_caret() as usize,
            ui.get_draft().len()
        );

        snapshot
            .view
            .entries
            .push(Arc::new(Entry::User("另一条已发送的消息".into())));
        output.send_replace(snapshot);
        assert!(ui.invoke_recall_composer_prompt(1));
        assert_eq!(ui.get_draft(), "第一条");
        ui.set_draft("保留外部写入的草稿".into());
        assert!(!ui.invoke_recall_composer_prompt(1));
        assert_eq!(ui.get_draft(), "保留外部写入的草稿");
        assert!(!ui.get_composer_recalling());
        ui.set_draft(String::new());
        assert!(ui.invoke_recall_composer_prompt(1));
        assert_eq!(ui.get_draft(), "另一条已发送的消息");
        assert!(ui.invoke_recall_composer_prompt(0));
        assert!(ui.get_draft().is_empty());
    }

    #[test]
    fn a_snapshot_from_another_conversation_cannot_supply_a_prompt() {
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.view.conversation = 7;
        snapshot.view.entries = vec![Arc::new(Entry::User("其他会话的内容".into()))];
        let (_output, receiver) = watch::channel(snapshot);
        let mut ui = AgentWorkbench::new();
        ui.set_workspace("/workspace".into());
        ui.set_conversation_id(8);
        attach(&mut ui, receiver);
        assert!(!ui.invoke_recall_composer_prompt(1));
        assert!(ui.get_draft().is_empty());
    }
}
