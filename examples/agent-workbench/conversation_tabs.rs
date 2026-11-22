use session::{Commands, Input, Snapshot};
use std::{cell::RefCell, rc::Rc};
use workbench_ui::{AgentWorkbench, ConversationTab};

#[derive(Default)]
pub(super) struct ConversationTabs {
    open: Vec<i64>,
    closed: Vec<i64>,
    current: Option<i64>,
    closing: Option<(i64, i64)>,
    initial: Option<i64>,
    restored: bool,
    storage_ready: bool,
    persisted: Option<(Vec<i64>, Option<i64>)>,
    commands: Option<Commands>,
}

impl ConversationTabs {
    pub(super) fn sync(&mut self, ui: &mut AgentWorkbench, snapshot: &Snapshot) {
        let summaries = &snapshot.conversations;
        let exists = |id: &i64| summaries.iter().any(|summary| summary.id == *id);
        let id = snapshot.view.conversation;
        let current = (!snapshot.view.workspace.is_empty() && exists(&id)).then_some(id);
        let initial = *self.initial.get_or_insert(id);
        let restored_now = !self.restored && snapshot.restored_tabs.is_some();
        if restored_now && let Some(restored) = &snapshot.restored_tabs {
            self.restored = true;
            let previous = std::mem::replace(&mut self.open, restored.as_ref().clone());
            for open in previous {
                if (open != initial || current == Some(initial)) && !self.open.contains(&open) {
                    self.open.push(open);
                }
            }
        }
        if let Some((old, next)) = self.closing.take()
            && current == Some(next)
            && let Some(index) = self.open.iter().position(|entry| *entry == old)
        {
            self.remove(ui, index);
        }
        self.open.retain(exists);
        self.closed.retain(exists);
        if let Some(id) = current {
            if !self.open.contains(&id) {
                self.open.push(id);
            }
            self.closed.retain(|closed| *closed != id);
        }
        self.current = current;

        let model = ui.model_conversation_tabs();
        for (index, id) in self.open.iter().enumerate() {
            let Some(summary) = summaries.iter().find(|row| row.id == *id) else {
                continue;
            };
            let row = ConversationTab {
                id: id.to_string(),
                title: summary.title.clone(),
                workspace: summary.workspace.clone(),
                activity: summary.activity.key().into(),
                archived: summary.archived,
            };
            if model
                .row(index)
                .is_none_or(|existing| existing.id != row.id)
            {
                if let Some(old) = (index + 1..model.len())
                    .find(|old| model.row(*old).is_some_and(|old| old.id == row.id))
                {
                    model.move_row(old, index);
                } else {
                    model.insert(index, row.clone());
                }
            }
            if model.row(index).is_some_and(|existing| existing != row) {
                model.set_row(index, row);
            }
        }
        while model.len() > self.open.len() {
            model.remove(model.len() - 1);
        }
        self.publish_selection(ui);
        ui.set_can_reopen_conversation_tab(!self.closed.is_empty());
        self.storage_ready =
            snapshot.connected && snapshot.composer_ready && !snapshot.tabs_restoring;
        if restored_now {
            // Keep unresolved saved tabs on disk until the user changes the set.
            self.persisted = Some((self.open.clone(), self.current));
        } else {
            self.persist();
        }
    }

    fn persist(&mut self) {
        let Some(commands) = self.commands.as_ref().filter(|_| self.storage_ready) else {
            return;
        };
        if self
            .persisted
            .as_ref()
            .is_some_and(|(open, active)| open == &self.open && active == &self.current)
        {
            return;
        }
        let open = self.open.clone();
        if commands
            .send(Input::SaveOpenTabs {
                open: open.clone(),
                active: self.current,
            })
            .is_ok()
        {
            self.persisted = Some((open, self.current));
        }
    }

    fn publish_selection(&self, ui: &mut AgentWorkbench) {
        ui.set_conversation_tab(self.current.map(|id| id.to_string()).unwrap_or_default());
    }

    fn request_selection(&self, ui: &mut AgentWorkbench, id: i64) -> bool {
        if self.current == Some(id) || !crate::can_navigate(ui) {
            return false;
        }
        ui.invoke_select_conversation(id);
        ui.get_switching()
    }

    fn remove(&mut self, ui: &mut AgentWorkbench, index: usize) {
        self.closed.push(self.open.remove(index));
        ui.model_conversation_tabs().remove(index);
        ui.set_can_reopen_conversation_tab(true);
    }

    fn close(&mut self, ui: &mut AgentWorkbench, id: i64) {
        if !crate::can_navigate(ui) || self.open.len() < 2 {
            return;
        }
        let Some(index) = self.open.iter().position(|open| *open == id) else {
            return;
        };
        if self.current != Some(id) {
            self.remove(ui, index);
            self.persist();
            return;
        }
        let next = *self
            .open
            .get(index + 1)
            .unwrap_or_else(|| &self.open[index - 1]);
        if self.request_selection(ui, next) {
            self.closing = Some((id, next));
        }
    }

    fn cycle(&self, ui: &mut AgentWorkbench, delta: i64) {
        if let Some(index) = self
            .open
            .iter()
            .position(|open| Some(*open) == self.current)
        {
            let next = (index as i64 + delta.signum()).rem_euclid(self.open.len() as i64);
            self.request_selection(ui, self.open[next as usize]);
        }
    }

    fn reorder(&mut self, from: usize, to: usize) {
        if from < self.open.len() && to < self.open.len() {
            let value = self.open.remove(from);
            self.open.insert(to, value);
            self.persist();
        }
    }
}

pub(super) fn attach(
    ui: &mut AgentWorkbench,
    commands: Option<Commands>,
) -> Rc<RefCell<ConversationTabs>> {
    let tabs = Rc::new(RefCell::new(ConversationTabs {
        commands,
        ..ConversationTabs::default()
    }));

    let weak = ui.as_weak();
    let state = tabs.clone();
    ui.on_select_conversation_tab(move |value| {
        if let Some(mut ui) = weak.upgrade() {
            let state = state.borrow();
            if let Ok(id) = value.parse::<i64>()
                && state.open.contains(&id)
            {
                state.request_selection(&mut ui, id);
            }
            state.publish_selection(&mut ui);
        }
    });

    let weak = ui.as_weak();
    let state = tabs.clone();
    ui.on_close_conversation_tab(move |value| {
        if let Some(mut ui) = weak.upgrade()
            && let Ok(id) = value.parse()
        {
            state.borrow_mut().close(&mut ui, id);
        }
    });

    let state = tabs.clone();
    ui.on_reorder_conversation_tabs(move |from, to| {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) {
            state.borrow_mut().reorder(from, to);
        }
    });

    let weak = ui.as_weak();
    let state = tabs.clone();
    ui.on_cycle_conversation_tab(move |delta| {
        if let Some(mut ui) = weak.upgrade() {
            state.borrow().cycle(&mut ui, delta);
        }
    });

    let weak = ui.as_weak();
    let state = tabs.clone();
    ui.on_reopen_conversation_tab(move || {
        if let Some(mut ui) = weak.upgrade() {
            let state = state.borrow();
            if let Some(id) = state.closed.last() {
                state.request_selection(&mut ui, *id);
            }
        }
    });

    tabs
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::{ConversationActivity, ConversationSummary};
    use std::sync::Arc;

    #[test]
    fn restored_order_replaces_the_startup_placeholder_and_is_applied_once() {
        let mut ui = AgentWorkbench::new();
        let tabs = attach(&mut ui, None);
        let mut snapshot = Snapshot::connecting("/alpha".into());
        snapshot.tabs_restoring = true;
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        assert_eq!(ui.get_conversation_tab(), "0");
        for (id, workspace) in [(1, "/alpha"), (2, "/beta")] {
            snapshot.conversations.push(ConversationSummary {
                id,
                title: workspace.into(),
                workspace: workspace.into(),
                archived: false,
                pinned: false,
                project_workspace: None,
                activity: ConversationActivity::Idle,
            });
        }
        snapshot.view.conversation = 2;
        snapshot.view.workspace = "/beta".into();
        snapshot.tabs_restoring = false;
        snapshot.restored_tabs = Some(Arc::new(vec![2, 1]));
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        let model = ui.model_conversation_tabs();
        assert_eq!(ui.get_conversation_tab(), "2");
        assert_eq!(model.len(), 2);
        assert_eq!(model.row(0).unwrap().id, "2");
        assert_eq!(model.row(1).unwrap().id, "1");
        assert!(model.move_row(1, 0));
        ui.invoke_reorder_conversation_tabs(1, 0);
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        assert_eq!(model.row(0).unwrap().id, "1");
        assert_eq!(ui.get_conversation_tab(), "2");
    }

    #[test]
    fn cross_project_tabs_keep_identity_and_close_only_after_the_neighbor_opens() {
        let mut ui = AgentWorkbench::new();
        let mut snapshot = Snapshot::connecting("/alpha".into());
        snapshot.connected = true;
        snapshot.conversations.push(ConversationSummary {
            id: 1,
            workspace: "/beta".into(),
            title: "Beta".into(),
            archived: false,
            pinned: false,
            project_workspace: None,
            activity: ConversationActivity::Running,
        });
        let tabs = attach(&mut ui, None);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let selected = requests.clone();
        let weak = ui.as_weak();
        ui.on_select_conversation(move |id| {
            selected.borrow_mut().push(id);
            weak.upgrade().unwrap().set_switching(true);
        });
        let model = ui.model_conversation_tabs();
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        let alpha = model.row_key(0).unwrap();
        assert_eq!(ui.get_conversation_tab(), "0");

        snapshot.view.conversation = 1;
        snapshot.view.workspace = "/beta".into();
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        let beta = model.row_key(1).unwrap();
        assert_eq!(model.row(1).unwrap().workspace, "/beta");
        assert_eq!(model.row(1).unwrap().activity, "running");
        assert_eq!(ui.get_conversation_tab(), "1");

        ui.invoke_cycle_conversation_tab(1);
        assert_eq!(requests.borrow().as_slice(), [0]);
        assert_eq!(ui.get_conversation_tab(), "1");
        let mut alpha_view = snapshot.clone();
        alpha_view.view.conversation = 0;
        alpha_view.view.workspace = "/alpha".into();
        ui.set_switching(false);
        tabs.borrow_mut().sync(&mut ui, &alpha_view);
        assert_eq!(ui.get_conversation_tab(), "0");

        // An acknowledged but rejected switch must leave the tab open.
        ui.invoke_close_conversation_tab("0".into());
        assert_eq!(requests.borrow().as_slice(), [0, 1]);
        assert_eq!(model.len(), 2);
        ui.set_switching(false);
        tabs.borrow_mut().sync(&mut ui, &alpha_view);
        assert_eq!(model.row_key(0), Some(alpha));
        assert!(!ui.get_can_reopen_conversation_tab());

        ui.invoke_close_conversation_tab("0".into());
        assert_eq!(requests.borrow().as_slice(), [0, 1, 1]);
        ui.set_switching(false);
        tabs.borrow_mut().sync(&mut ui, &snapshot);
        assert_eq!(model.len(), 1);
        assert_eq!(model.row_key(0), Some(beta));
        assert!(ui.get_can_reopen_conversation_tab());

        ui.invoke_reopen_conversation_tab();
        assert_eq!(requests.borrow().as_slice(), [0, 1, 1, 0]);
        ui.set_switching(false);
        tabs.borrow_mut().sync(&mut ui, &alpha_view);
        assert_eq!(
            model
                .snapshot()
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "0"]
        );
        assert!(!ui.get_can_reopen_conversation_tab());
        assert!(model.move_row(1, 0));
        ui.invoke_reorder_conversation_tabs(1, 0);
        tabs.borrow_mut().sync(&mut ui, &alpha_view);
        assert_eq!(model.row(0).unwrap().id, "0");
        assert_eq!(model.row_key(1), Some(beta));

        ui.invoke_close_conversation_tab("1".into());
        assert_eq!(model.len(), 1);
        assert_eq!(requests.borrow().as_slice(), [0, 1, 1, 0]);
        assert_eq!(ui.get_conversation_tab(), "0");
    }
}
