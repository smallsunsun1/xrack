use session::{Commands, ConversationActivity, Input, Snapshot};
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc, sync::Arc};
use tokio::sync::watch;
use workbench_ui::{AgentWorkbench, ConversationRow, ProjectGroupRow};

#[derive(Default)]
pub(super) struct Sidebar {
    project_keys: HashMap<String, i64>,
    next_key: i64,
}

impl Sidebar {
    fn project_key(&mut self, path: &str) -> i64 {
        *self.project_keys.entry(path.into()).or_insert_with(|| {
            self.next_key += 4;
            self.next_key
        })
    }

    pub(super) fn update(
        &mut self,
        ui: &mut AgentWorkbench,
        snapshot: &Snapshot,
        previous: Option<&Snapshot>,
    ) {
        if !ui.get_project_groups_open() {
            return;
        }
        if previous.is_none_or(|prior| {
            !Arc::ptr_eq(&prior.project_search, &snapshot.project_search)
                || prior.conversations != snapshot.conversations
        }) {
            publish_search(ui, snapshot);
        }
        if previous.is_some_and(|prior| {
            Arc::ptr_eq(&prior.projects, &snapshot.projects)
                && Arc::ptr_eq(&prior.project_histories, &snapshot.project_histories)
                && prior.conversations == snapshot.conversations
                && prior.view.conversation == snapshot.view.conversation
                && prior.connected == snapshot.connected
        }) {
            return;
        }
        let rows = self.rows(snapshot);
        let count = rows.len();
        let model = ui.model_project_groups();
        for (index, row) in rows.into_iter().enumerate() {
            if model
                .row(index)
                .is_some_and(|existing| existing.key == row.key)
            {
                if model.row(index).as_ref() != Some(&row) {
                    model.set_row(index, row);
                }
            } else if let Some(previous) = (index + 1..model.len()).find(|&slot| {
                model
                    .row(slot)
                    .is_some_and(|existing| existing.key == row.key)
            }) {
                model.move_row(previous, index);
                if model.row(index).as_ref() != Some(&row) {
                    model.set_row(index, row);
                }
            } else if index == model.len() {
                model.push(row);
            } else {
                model.insert(index, row);
            }
        }
        while model.len() > count {
            model.remove(model.len() - 1);
        }
    }

    fn rows(&mut self, snapshot: &Snapshot) -> Vec<ProjectGroupRow> {
        let mut projects: HashMap<&str, Vec<_>> = HashMap::new();
        for row in snapshot.conversations.iter().filter(|row| !row.archived) {
            projects.entry(row.project()).or_default().push(row);
        }
        let names: Vec<_> = snapshot
            .projects
            .iter()
            .map(|path| crate::folder_name(path))
            .collect();
        let mut rows = Vec::new();
        for (project, name) in snapshot.projects.iter().zip(&names) {
            let key = self.project_key(project);
            let known = projects.remove(project.as_str()).unwrap_or_default();
            let title = if names.iter().filter(|other| *other == name).count() > 1 {
                let parent = Path::new(project)
                    .parent()
                    .and_then(Path::to_str)
                    .unwrap_or(project);
                format!("{name} · {parent}")
            } else {
                name.clone()
            };
            let grouping = [
                ConversationActivity::NeedsInput,
                ConversationActivity::Running,
                ConversationActivity::Updated,
            ]
            .into_iter()
            .find(|state| known.iter().any(|row| row.activity == *state))
            .map(activity)
            .unwrap_or_default();
            rows.push(ProjectGroupRow {
                key,
                parent: -1,
                title,
                detail: grouping.into(),
                kind: "project".into(),
                project: project.clone(),
                selected: project == snapshot.project_workspace(),
                ..ProjectGroupRow::default()
            });
            for pinned in [true, false] {
                for entry in known.iter().filter(|row| row.pinned == pinned) {
                    let mut detail = Vec::new();
                    if entry.project_workspace.is_some() {
                        detail.push("工作树");
                    }
                    if pinned {
                        detail.push("置顶");
                    }
                    let status = activity(entry.activity);
                    if !status.is_empty() {
                        detail.push(status);
                    }
                    rows.push(ProjectGroupRow {
                        key: entry.id * 2 + 1,
                        parent: key,
                        title: entry.title.clone(),
                        detail: detail.join(" · "),
                        kind: "thread".into(),
                        project: project.clone(),
                        conversation: entry.id,
                        selected: entry.id == snapshot.view.conversation,
                    });
                }
            }
            let (kind, title) = match snapshot.project_histories.get(project) {
                _ if !snapshot.connected => ("status", "Codex 未连接".into()),
                Some(history) if history.loading => ("status", "正在读取会话…".into()),
                Some(history) if !history.status.is_empty() => (
                    if history.more() { "more" } else { "load" },
                    history.status.clone(),
                ),
                Some(history) if history.more() => ("more", "加载更多会话".into()),
                Some(history) if history.loaded && known.is_empty() => {
                    ("status", "暂无会话".into())
                }
                Some(history) if history.loaded => continue,
                _ => ("load", "读取历史会话".into()),
            };
            rows.push(ProjectGroupRow {
                key: key + 2,
                parent: key,
                title,
                kind: kind.into(),
                project: project.clone(),
                ..ProjectGroupRow::default()
            });
        }
        rows
    }
}

fn publish_search(ui: &mut AgentWorkbench, snapshot: &Snapshot) {
    let search = &snapshot.project_search;
    let model = ui.model_project_search_results();
    if ui.get_project_search_query() != search.query
        || ui.get_project_search_result_archived() != search.archived
    {
        model.clear();
    }
    let by_id: HashMap<_, _> = if search.matches.is_empty() {
        HashMap::new()
    } else {
        snapshot
            .conversations
            .iter()
            .map(|row| (row.id, row))
            .collect()
    };
    let rows = search
        .matches
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .filter(|row| row.archived == search.archived);
    let mut count = 0;
    for (index, row) in rows.enumerate() {
        let mut label = crate::folder_name(row.project());
        if snapshot
            .projects
            .iter()
            .filter(|project| crate::folder_name(project) == label)
            .take(2)
            .count()
            > 1
        {
            let parent = Path::new(row.project())
                .parent()
                .and_then(Path::to_str)
                .unwrap_or(row.project());
            label.push_str(" · ");
            label.push_str(parent);
        }
        if row.project_workspace.is_some() {
            label.push_str(" · 工作树");
        }
        let result = ConversationRow {
            id: row.id,
            title: row.title.clone(),
            folder: label,
            activity: row.activity.key().into(),
            shown: true,
            pinned: row.pinned && !row.archived,
            pinned_shown: false,
        };
        if index == model.len() {
            model.push(result);
        } else if model.row(index).as_ref() != Some(&result) {
            model.set_row(index, result);
        }
        count = index + 1;
    }
    while model.len() > count {
        model.remove(model.len() - 1);
    }
    ui.set_project_search_query(search.query.clone());
    ui.set_project_search_result_archived(search.archived);
    ui.set_project_search_loading(search.loading);
    ui.set_project_search_status(search.status.clone());
    ui.set_project_search_more(search.more());
}

fn activity(state: ConversationActivity) -> &'static str {
    match state {
        ConversationActivity::Running => "运行中",
        ConversationActivity::NeedsInput => "待确认",
        ConversationActivity::Updated => "有更新",
        ConversationActivity::Idle => "",
    }
}

pub(super) fn attach(
    ui: &mut AgentWorkbench,
    output: watch::Receiver<Snapshot>,
    commands: Commands,
) -> Rc<RefCell<Sidebar>> {
    let sidebar = Rc::new(RefCell::new(Sidebar::default()));
    let state = sidebar.clone();
    let weak = ui.as_weak();
    ui.on_show_project_groups(move || {
        if let Some(mut ui) = weak.upgrade() {
            state.borrow_mut().update(&mut ui, &output.borrow(), None);
        }
    });
    let weak = ui.as_weak();
    let request = commands.clone();
    ui.on_search_all_projects(move |query, archived, more| {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_project_groups_open()
        {
            crate::dispatch(
                &mut ui,
                &request,
                Input::ProjectSearch {
                    query,
                    archived,
                    more,
                },
            );
        }
    });
    let weak = ui.as_weak();
    let request = commands.clone();
    ui.on_expand_project_group(move |key| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(row) = ui
                .model_project_groups()
                .snapshot()
                .into_iter()
                .find(|row| row.key == key && row.kind == "project")
        {
            crate::dispatch(
                &mut ui,
                &request,
                Input::ProjectHistory {
                    path: row.project,
                    more: false,
                },
            );
        }
    });
    let weak = ui.as_weak();
    ui.on_activate_project_group_row(move |key| {
        let Some(mut ui) = weak.upgrade() else { return };
        let Some(row) = ui
            .model_project_groups()
            .snapshot()
            .into_iter()
            .find(|row| row.key == key)
        else {
            return;
        };
        match row.kind.as_str() {
            "thread" => ui.invoke_select_conversation(row.conversation),
            "load" | "more" => {
                ui.set_project_group_key(-1);
                crate::dispatch(
                    &mut ui,
                    &commands,
                    Input::ProjectHistory {
                        path: row.project,
                        more: row.kind == "more",
                    },
                );
            }
            _ => {}
        }
    });
    sidebar
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::ConversationSummary;

    #[test]
    fn groups_keep_keys_when_recent_project_order_changes_and_put_pins_first() {
        let first = "/repos/demo";
        let second = "/scratch/demo";
        let mut snapshot = Snapshot::connecting(first.into());
        snapshot.connected = true;
        snapshot.projects = Arc::new(vec![first.into(), second.into()]);
        let row = |id, pinned, activity| ConversationSummary {
            id,
            workspace: "/worktrees/demo".into(),
            title: format!("Thread {id}"),
            pinned,
            archived: false,
            activity,
            project_workspace: Some(second.into()),
        };
        snapshot.conversations.extend([
            row(1, false, ConversationActivity::Running),
            row(2, true, ConversationActivity::NeedsInput),
        ]);
        let mut ui = AgentWorkbench::new();
        ui.set_project_groups_open(true);
        let mut sidebar = Sidebar::default();
        sidebar.update(&mut ui, &snapshot, None);
        let model = ui.model_project_groups();
        let index = model
            .snapshot()
            .iter()
            .position(|row| row.project == second && row.kind == "project")
            .unwrap();
        let group_key = model.row_key(index);
        let heading = model.row(index).unwrap();
        assert_eq!(heading.detail, "待确认");
        assert!(heading.title.contains("/scratch"));
        assert_eq!(model.row(index + 1).unwrap().conversation, 2);
        assert_eq!(model.row(index + 1).unwrap().parent, heading.key);

        let mut reordered = snapshot.clone();
        reordered.projects = Arc::new(vec![second.into(), first.into()]);
        sidebar.update(&mut ui, &reordered, Some(&snapshot));
        assert_eq!(model.row_key(0), group_key);
        assert_eq!(model.row(0).unwrap().key, heading.key);
    }
}
