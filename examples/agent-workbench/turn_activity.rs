use session::{ActivityState, Entry, Snapshot};
use std::collections::HashSet;
use workbench_ui::{AgentWorkbench, TurnActivity};

pub(super) fn expanded(ui: &AgentWorkbench) -> HashSet<String> {
    ui.model_turn_activities()
        .snapshot()
        .into_iter()
        .filter(|row| !row.summary.is_empty() && row.expanded)
        .map(|row| row.turn)
        .collect()
}

pub(super) fn sync(ui: &mut AgentWorkbench, snapshot: &Snapshot, retain_expansion: bool) -> bool {
    let open = if retain_expansion {
        expanded(ui)
    } else {
        HashSet::new()
    };
    let mut rows = vec![TurnActivity::default(); snapshot.view.entries.len()];
    for turn in snapshot.view.fork_points.iter() {
        let Some(entries) = snapshot.view.entries.get(turn.entries.clone()) else {
            continue;
        };
        let mut commands = 0;
        let mut plans = 0;
        let mut attention = false;
        let mut header = None;
        for (offset, entry) in entries.iter().enumerate() {
            match entry.as_ref() {
                Entry::Command(command) => {
                    commands += 1;
                    attention |= command.state != ActivityState::Completed;
                }
                Entry::Plan(_) => plans += 1,
                _ => continue,
            }
            let index = turn.entries.start + offset;
            header.get_or_insert(index);
            rows[index] = TurnActivity {
                turn: turn.id.clone(),
                expanded: open.contains(&turn.id),
                ..TurnActivity::default()
            };
        }
        if let Some(index) = header {
            let mut summary = String::from("本轮活动");
            if commands > 0 {
                summary.push_str(&format!(" · {commands} 条命令"));
            }
            if plans > 0 {
                summary.push_str(&format!(" · {plans} 项计划"));
            }
            if attention {
                summary.push_str(" · 有未完成命令");
            }
            rows[index].summary = summary;
            rows[index].attention = attention;
        }
    }
    let model = ui.model_turn_activities();
    let mut changed = false;
    for (index, row) in rows.into_iter().enumerate() {
        if let Some(previous) = model.row(index) {
            if row != previous {
                model.set_row(index, row);
                changed = true;
            }
        } else {
            model.push(row);
            changed = true;
        }
    }
    while model.len() > snapshot.view.entries.len() {
        model.remove(model.len() - 1);
        changed = true;
    }
    changed
}

pub(super) fn restore(ui: &AgentWorkbench, open: &HashSet<String>) {
    let model = ui.model_turn_activities();
    for (index, mut row) in model.snapshot().into_iter().enumerate() {
        let expanded = open.contains(&row.turn);
        if !row.turn.is_empty() && row.expanded != expanded {
            row.expanded = expanded;
            model.set_row(index, row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_references::TranscriptStream;
    use session::{CommandActivity, ForkPoint};
    use std::sync::Arc;
    use std::time::Duration;
    use xrack_runtime::Orientation;
    use xrack_testing::{Size, UiDriver};

    #[test]
    fn completed_turns_fold_independently_and_search_opens_the_matched_group() {
        let mut ui = AgentWorkbench::new();
        let mut markdown = TranscriptStream::default();
        crate::chat_find::attach(&mut ui);
        attach(&mut ui);
        let command = |name: &str, output: &str, state| {
            Arc::new(Entry::Command(CommandActivity {
                command: name.into(),
                state,
                exit_code: None,
                output: output.into(),
                output_truncated: false,
            }))
        };
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.view.entries = vec![
            Arc::new(Entry::User("同一工作区".into())),
            command("check-one", "first", ActivityState::Completed),
            command("check-two", "only in activity", ActivityState::Failed),
            Arc::new(Entry::Assistant {
                text: "保留结果".into(),
                finalized: true,
            }),
            command("running", "still pending", ActivityState::Running),
        ];
        crate::update(&mut ui, &snapshot, None, &mut markdown);
        ui.set_chat_find_open(true);
        ui.set_chat_find_query("only in activity".into());
        ui.invoke_refresh_chat_find();
        assert_eq!(ui.get_expanded_command(), 2);

        let mut completed = snapshot.clone();
        completed.view.fork_points = Arc::new(vec![
            ForkPoint {
                id: "first".into(),
                label: "前一轮".into(),
                prompt: Some(0),
                response: None,
                entries: 0..2,
            },
            ForkPoint {
                id: "second".into(),
                label: "后一轮".into(),
                prompt: None,
                response: Some(3),
                entries: 2..4,
            },
        ]);
        crate::update(&mut ui, &completed, Some(&snapshot), &mut markdown);
        let model = ui.model_turn_activities();
        assert!(model.row(0).unwrap().turn.is_empty());
        assert!(!model.row(1).unwrap().expanded);
        assert!(
            model.row(2).unwrap().expanded,
            "the retained search match must stay visible"
        );
        assert!(model.row(2).unwrap().attention);
        assert!(model.row(3).unwrap().turn.is_empty());
        assert!(model.row(4).unwrap().turn.is_empty());
        assert_eq!(
            ui.model_command_groups().row(1).unwrap(),
            Default::default()
        );
        assert_eq!(ui.get_chat_find_count(), 1);

        ui.invoke_toggle_turn_activity(1);
        assert_eq!(
            expanded(&ui),
            HashSet::from(["first".into(), "second".into()])
        );
        ui.set_chat_find_open(false);
        let mut earlier = completed.clone();
        earlier
            .view
            .entries
            .insert(0, Arc::new(Entry::User("更早的提示".into())));
        for point in Arc::make_mut(&mut earlier.view.fork_points) {
            point.entries.start += 1;
            point.entries.end += 1;
        }
        crate::update(&mut ui, &earlier, Some(&completed), &mut markdown);
        assert!(model.row(2).unwrap().expanded);
        assert!(model.row(3).unwrap().expanded);
        ui.invoke_toggle_turn_activity(2);
        assert!(!model.row(2).unwrap().expanded);
        assert!(model.row(3).unwrap().expanded);
    }

    #[test]
    fn search_scrolls_to_a_line_in_previously_folded_output() {
        let mut ui = AgentWorkbench::new();
        let mut markdown = TranscriptStream::default();
        crate::chat_find::attach(&mut ui);
        let mut snapshot = Snapshot::connecting("/workspace".into());
        snapshot.connected = true;
        snapshot.view.entries = vec![
            Arc::new(Entry::User("检查输出".into())),
            Arc::new(Entry::Command(CommandActivity {
                command: "local-check".into(),
                state: ActivityState::Completed,
                exit_code: Some(0),
                output: format!(
                    "{}target in command output",
                    "an earlier output line\n".repeat(56)
                ),
                output_truncated: false,
            })),
            Arc::new(Entry::Assistant {
                text: "检查完成".into(),
                finalized: true,
            }),
        ];
        snapshot.view.fork_points = Arc::new(vec![ForkPoint {
            id: "turn".into(),
            label: "检查输出".into(),
            prompt: Some(0),
            response: Some(2),
            entries: 0..3,
        }]);
        crate::update(&mut ui, &snapshot, None, &mut markdown);
        assert!(!ui.model_turn_activities().row(1).unwrap().expanded);
        let mut driver = UiDriver::headless(ui, Size::new(780.0, 600.0));
        assert!(driver.menu_shortcut("Primary+F".parse().unwrap()));
        driver.fill_focused("target in command output");
        driver.advance_time(Duration::from_millis(90));
        driver.advance_time(Duration::from_millis(12));
        assert_eq!(driver.focused(), Some("chat-find-input"));
        assert!(
            driver
                .component()
                .model_turn_activities()
                .row(1)
                .unwrap()
                .expanded
        );
        let transcript = driver.path("transcript");
        let offset = driver
            .layout_at(&transcript)
            .unwrap()
            .scroll_on(Orientation::Vertical)
            .unwrap()
            .offset;
        assert!(
            offset > 220.0,
            "expected to reveal the actual command line, offset={offset}"
        );
    }
}

fn set_expanded(ui: &AgentWorkbench, turn: &str, expanded: bool) {
    if turn.is_empty() {
        return;
    }
    let model = ui.model_turn_activities();
    for (index, mut row) in model.snapshot().into_iter().enumerate() {
        if row.turn == turn && row.expanded != expanded {
            row.expanded = expanded;
            model.set_row(index, row);
        }
    }
}

pub(super) fn reveal(ui: &AgentWorkbench, index: usize) -> bool {
    if let Some(row) = ui.model_turn_activities().row(index)
        && !row.turn.is_empty()
        && !row.expanded
    {
        set_expanded(ui, &row.turn, true);
        return true;
    }
    false
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let weak = ui.as_weak();
    ui.on_toggle_turn_activity(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(row) = usize::try_from(index)
            .ok()
            .and_then(|index| ui.model_turn_activities().row(index))
            && !row.summary.is_empty()
        {
            set_expanded(&ui, &row.turn, !row.expanded);
        }
    });
}
