use crate::folder_name;
use session::{ConversationActivity, Snapshot};
use std::collections::HashSet;
use workbench_ui::{ActivityRow, AgentWorkbench};

const STATUSES: [ConversationActivity; 3] = [
    ConversationActivity::NeedsInput,
    ConversationActivity::Updated,
    ConversationActivity::Running,
];

pub(super) fn next_attention(snapshot: &Snapshot) -> Option<i64> {
    let start = snapshot
        .conversations
        .iter()
        .position(|row| row.id == snapshot.view.conversation)
        .map_or(0, |index| index + 1);
    let (before, after) = snapshot.conversations.split_at(start);
    after
        .iter()
        .chain(before)
        .find(|row| {
            row.id != snapshot.view.conversation
                && !row.archived
                && row.activity == ConversationActivity::NeedsInput
        })
        .map(|row| row.id)
}

pub(super) fn publish(ui: &mut AgentWorkbench, snapshot: &Snapshot, previous: Option<&Snapshot>) {
    ui.set_activity_can_jump(next_attention(snapshot).is_some());
    if previous.is_some_and(|old| old.conversations == snapshot.conversations) {
        return;
    }
    let mut counts = [0; STATUSES.len()];
    let mut desired = Vec::new();
    for (index, activity) in STATUSES.into_iter().enumerate() {
        for row in snapshot
            .conversations
            .iter()
            .filter(|row| !row.archived && row.activity == activity)
        {
            counts[index] += 1;
            let project = folder_name(row.project());
            let location = if row.project_workspace.is_some() {
                format!("{project} · 工作树")
            } else {
                project
            };
            desired.push(ActivityRow {
                id: row.id,
                title: row.title.clone(),
                project: location,
                activity: activity.key().into(),
            });
        }
    }
    ui.set_activity_waiting(counts[0]);
    ui.set_activity_updated(counts[1]);
    ui.set_activity_running(counts[2]);

    let model = ui.model_activity_rows();
    let mut current = model.snapshot();
    let retained: HashSet<_> = desired.iter().map(|row| row.id).collect();
    for index in (0..current.len()).rev() {
        if !retained.contains(&current[index].id) {
            current.remove(index);
            model.remove(index);
        }
    }
    for (index, row) in desired.into_iter().enumerate() {
        match current[index..]
            .iter()
            .position(|previous| previous.id == row.id)
        {
            Some(position) => {
                let position = index + position;
                if position != index {
                    let previous = current.remove(position);
                    current.insert(index, previous);
                    model.move_row(position, index);
                }
                let previous = &current[index];
                if previous.title != row.title
                    || previous.project != row.project
                    || previous.activity != row.activity
                {
                    model.set_row(index, row);
                }
            }
            None => {
                current.insert(index, row.clone());
                model.insert(index, row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::ConversationSummary;

    #[test]
    fn activity_is_cross_project_and_status_changes_preserve_row_identity() {
        let mut snapshot = Snapshot::connecting("/workspace/active".into());
        let row = |id, name: &str, workspace: &str, activity| ConversationSummary {
            id,
            workspace: workspace.into(),
            title: name.into(),
            archived: false,
            pinned: false,
            project_workspace: None,
            activity,
        };
        snapshot.conversations.extend([
            row(
                1,
                "构建工具",
                "/workspace/tools",
                ConversationActivity::Running,
            ),
            row(
                2,
                "校对示例",
                "/workspace/examples",
                ConversationActivity::NeedsInput,
            ),
            row(
                3,
                "验证组件",
                "/workspace/components",
                ConversationActivity::Updated,
            ),
        ]);
        let mut ui = AgentWorkbench::new();
        publish(&mut ui, &snapshot, None);
        let model = ui.model_activity_rows();
        let order = || {
            model
                .snapshot()
                .into_iter()
                .map(|row| row.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(), [2, 3, 1]);
        assert_eq!(model.row(0).unwrap().project, "examples");
        assert_eq!(next_attention(&snapshot), Some(2));
        let running_key = model.row_key(2);

        let before = snapshot.clone();
        snapshot.conversations[1].activity = ConversationActivity::NeedsInput;
        snapshot.conversations[2].activity = ConversationActivity::Running;
        snapshot.conversations[3].archived = true;
        publish(&mut ui, &snapshot, Some(&before));
        assert_eq!(order(), [1, 2]);
        assert_eq!(model.row_key(0), running_key);
        assert_eq!(ui.get_activity_running(), 1);
        assert_eq!(ui.get_activity_updated(), 0);
        let before = snapshot.clone();
        snapshot.view.conversation = 1;
        publish(&mut ui, &snapshot, Some(&before));
        assert!(!ui.get_activity_can_jump());
        assert_eq!(order(), [1, 2]);

        snapshot.conversations[2].activity = ConversationActivity::NeedsInput;
        snapshot.conversations[3].archived = false;
        snapshot.conversations[3].activity = ConversationActivity::NeedsInput;
        snapshot.view.conversation = 2;
        assert_eq!(next_attention(&snapshot), Some(3));
        snapshot.view.conversation = 3;
        assert_eq!(next_attention(&snapshot), Some(1));
    }
}
