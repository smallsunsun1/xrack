use crate::{
    EditAction, Target,
    edit::{AppliedEdit, SelectionState},
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use xrack_runtime::{Node, RowKey};

const EDITORS: usize = 16;
const EDIT_GROUPS: usize = 128;
const EDIT_BYTES: usize = 1024 * 1024;
const GROUP_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditorKey {
    path: Vec<usize>,
    row: Option<(usize, RowKey)>,
}

impl EditorKey {
    pub(crate) fn new(scene: &Node, target: &Target) -> Self {
        let mut path = target.path.clone();
        let row = target.row_key.and_then(|key| {
            (1..=path.len()).rev().find_map(|depth| {
                let node = scene.at_path(&path[..depth])?;
                (node.kind == "ListRow" && node.row_key == Some(key)).then_some((depth - 1, key))
            })
        });
        if let Some((slot, _)) = row {
            path[slot] = 0;
        }
        Self { path, row }
    }
}

/// The source of an edit determines whether it can merge with preceding typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSource {
    Keyboard,
    Atomic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    Insert,
    Backspace,
    Delete,
    Atomic,
}

impl Group {
    pub(crate) fn for_edit(
        source: EditSource,
        action: EditAction<'_>,
        before: SelectionState,
        edit: &AppliedEdit,
    ) -> Self {
        if source == EditSource::Atomic || !before.collapsed() {
            return Self::Atomic;
        }
        match action {
            EditAction::Insert(_) if !edit.inserted.contains('\n') => Self::Insert,
            EditAction::Backspace => Self::Backspace,
            EditAction::Delete => Self::Delete,
            _ => Self::Atomic,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Record {
    start: usize,
    removed: String,
    inserted: String,
    before: SelectionState,
    after: SelectionState,
}

impl Record {
    pub(crate) fn new(
        edit: AppliedEdit,
        before: SelectionState,
        after: SelectionState,
    ) -> (String, Self) {
        (
            edit.text,
            Self {
                start: edit.start,
                removed: edit.removed,
                inserted: edit.inserted,
                before,
                after,
            },
        )
    }

    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }

    fn merge(&mut self, next: &Self, group: Group) -> bool {
        if self.after != next.before {
            return false;
        }
        match group {
            Group::Insert if self.start + self.inserted.len() == next.start => {
                self.inserted.push_str(&next.inserted)
            }
            Group::Delete if self.start == next.start => self.removed.push_str(&next.removed),
            Group::Backspace if next.start + next.removed.len() == self.start => {
                self.removed.insert_str(0, &next.removed);
                self.start = next.start;
            }
            _ => return false,
        }
        self.after = next.after;
        true
    }

    fn apply(&self, current: &str, redo: bool) -> (String, SelectionState) {
        let (remove, insert, selection) = if redo {
            (&self.removed, &self.inserted, self.after)
        } else {
            (&self.inserted, &self.removed, self.before)
        };
        let range = self.start..self.start + remove.len();
        debug_assert_eq!(&current[range.clone()], remove);
        let mut text = current.to_owned();
        text.replace_range(range, insert);
        (text, selection)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextHistory {
    expected: String,
    undo: VecDeque<Record>,
    redo: Vec<Record>,
    group: Option<(Group, Instant)>,
}

impl TextHistory {
    pub(crate) fn break_group(&mut self) {
        self.group = None;
    }
    pub(crate) fn observe(&mut self, current: &str) {
        if self.expected != current {
            self.undo.clear();
            self.redo.clear();
            self.group = None;
            self.expected.clear();
            self.expected.push_str(current);
        }
    }

    pub(crate) fn push(&mut self, next: String, record: Record, group: Group, now: Instant) {
        let merged = group != Group::Atomic
            && self.group.is_some_and(|(previous, when)| {
                previous == group && now.saturating_duration_since(when) <= GROUP_INTERVAL
            })
            && self
                .undo
                .back_mut()
                .is_some_and(|last| last.merge(&record, group));
        if !merged {
            self.undo.push_back(record);
        }
        self.expected = next;
        self.group = Some((group, now));
        self.redo.clear();
        let mut bytes: usize = self.undo.iter().map(Record::bytes).sum();
        while self.undo.len() > EDIT_GROUPS || bytes > EDIT_BYTES {
            bytes -= self.undo.pop_front().unwrap().bytes();
        }
    }

    pub(crate) fn replay(&self, redo: bool) -> Option<(String, SelectionState)> {
        let record = if redo {
            self.redo.last()
        } else {
            self.undo.back()
        }?;
        Some(record.apply(&self.expected, redo))
    }

    pub(crate) fn commit_replay(&mut self, text: String, redo: bool) {
        if redo {
            self.undo.push_back(self.redo.pop().unwrap());
        } else {
            self.redo.push(self.undo.pop_back().unwrap());
        }
        self.expected = text;
        self.group = None;
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EditHistories(VecDeque<(EditorKey, TextHistory)>);

impl EditHistories {
    pub(crate) fn entry(&mut self, key: EditorKey) -> &mut TextHistory {
        let old = self
            .0
            .iter()
            .position(|(stored, _)| *stored == key)
            .and_then(|index| self.0.remove(index));
        if self.0.len() == EDITORS {
            self.0.pop_front();
        }
        self.0
            .push_back(old.unwrap_or_else(|| (key, TextHistory::default())));
        &mut self.0.back_mut().unwrap().1
    }

    pub(crate) fn get_mut(&mut self, key: &EditorKey) -> Option<&mut TextHistory> {
        self.0
            .iter_mut()
            .find(|(stored, _)| stored == key)
            .map(|(_, history)| history)
    }

    pub(crate) fn break_group(&mut self) {
        for (_, history) in &mut self.0 {
            history.break_group();
        }
    }
}
