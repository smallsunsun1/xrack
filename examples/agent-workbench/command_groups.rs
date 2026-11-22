use session::{ActivityState, Entry};
use std::sync::Arc;
use workbench_ui::{AgentWorkbench, CommandGroup};

pub(super) fn state(entry: &Entry) -> Option<ActivityState> {
    match entry {
        Entry::Command(command) => Some(command.state),
        _ => None,
    }
}

#[derive(Default)]
struct Status {
    active: usize,
    failed: usize,
    incomplete: usize,
}

impl Status {
    fn add(&mut self, state: ActivityState) {
        match state {
            ActivityState::Completed => {}
            ActivityState::Pending | ActivityState::Running => self.active += 1,
            ActivityState::Failed | ActivityState::Declined => self.failed += 1,
            ActivityState::Interrupted | ActivityState::Unfinished => self.incomplete += 1,
        }
    }

    fn label(&self) -> (String, String) {
        if self.active == 0 && self.failed == 0 && self.incomplete == 0 {
            return ("全部完成".into(), "done".into());
        }
        let mut parts = Vec::new();
        for (count, label) in [
            (self.failed, "失败"),
            (self.active, "进行中"),
            (self.incomplete, "未完成"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {label}"));
            }
        }
        let tone = if self.failed > 0 {
            "error"
        } else if self.active > 0 {
            "running"
        } else {
            ""
        };
        (parts.join(" · "), tone.into())
    }
}

/// Keep transcript indices unchanged for search, review links and paging.
/// Only the first row of a group needs the summary string.
pub(super) fn sync(ui: &AgentWorkbench, entries: &[Arc<Entry>]) {
    let model = ui.model_command_groups();
    let activities = ui.model_turn_activities();
    let owner = |index| {
        activities
            .row(index)
            .map_or_else(String::new, |row| row.turn)
    };
    let write = |index, group: CommandGroup| {
        if index == model.len() {
            model.push(group);
        } else if model.row(index).as_ref() != Some(&group) {
            model.set_row(index, group);
        }
    };
    let mut index = 0;
    while index < entries.len() {
        let first = index;
        let turn = owner(first);
        let mut status = Status::default();
        while let Some(state) = entries.get(index).and_then(|entry| state(entry))
            && (index == first || owner(index) == turn)
        {
            status.add(state);
            index += 1;
        }
        if index - first < 2 {
            write(first, CommandGroup::default());
            index = first + 1;
            continue;
        }
        let (status, status_tone) = status.label();
        let members = CommandGroup {
            first: first as i64,
            last: index as i64 - 1,
            ..CommandGroup::default()
        };
        write(
            first,
            CommandGroup {
                status,
                status_tone,
                ..members.clone()
            },
        );
        for member in first + 1..index {
            write(member, members.clone());
        }
    }
    while model.len() > entries.len() {
        model.remove(model.len() - 1);
    }
}
