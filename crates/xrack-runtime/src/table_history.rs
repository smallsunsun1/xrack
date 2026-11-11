use crate::{
    Property, RowKey, TablePasteInput, TableRows, VecModel, table_clipboard::stage_table_text,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    mem::size_of,
};

const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_HISTORY_BATCHES: usize = 64;

type FieldChange<T> = Box<dyn Fn(&mut T, bool) -> bool>;

/// A typed patch for the fields changed by one table edit.
#[doc(hidden)]
pub struct TableRowPatch<T> {
    fields: Vec<FieldChange<T>>,
    bytes: usize,
    limit: usize,
}

impl<T: 'static> TableRowPatch<T> {
    pub fn record<V: Clone + 'static>(
        &mut self,
        before: &V,
        after: &V,
        field: fn(&mut T) -> &mut V,
        same: fn(&V, &V) -> bool,
        payload_bytes: fn(&V) -> usize,
    ) {
        if self.bytes > self.limit || same(before, after) {
            return;
        }
        let overhead = 2 * (size_of::<V>() + size_of::<usize>() + size_of::<FieldChange<T>>());
        self.bytes = self
            .bytes
            .saturating_add(overhead)
            .saturating_add(payload_bytes(before))
            .saturating_add(payload_bytes(after));
        if self.bytes > self.limit {
            return;
        }
        let before = before.clone();
        let after = after.clone();
        self.fields.push(Box::new(move |row, redo| {
            let (expected, replacement) = if redo {
                (&before, &after)
            } else {
                (&after, &before)
            };
            let value = field(row);
            if !same(value, expected) {
                return false;
            }
            *value = replacement.clone();
            true
        }));
    }
}

struct HistoryRow<T> {
    key: RowKey,
    hint: usize,
    patch: TableRowPatch<T>,
}

struct Batch<T> {
    rows: Vec<HistoryRow<T>>,
    bytes: usize,
}

impl<T> Batch<T> {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            bytes: size_of::<Self>(),
        }
    }

    fn capture(
        &mut self,
        key: RowKey,
        index: usize,
        before: &T,
        after: &T,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> bool {
        let mut patch = TableRowPatch {
            fields: Vec::new(),
            bytes: 0,
            limit: MAX_HISTORY_BYTES - self.bytes,
        };
        capture(before, after, &mut patch);
        if patch.bytes > patch.limit {
            return false;
        }
        if !patch.fields.is_empty() {
            let bytes = self.bytes + patch.bytes + 2 * size_of::<HistoryRow<T>>();
            if bytes > MAX_HISTORY_BYTES {
                return false;
            }
            self.bytes = bytes;
            self.rows.push(HistoryRow {
                key,
                hint: index,
                patch,
            });
        }
        true
    }
}

struct History<T> {
    model: Option<u64>,
    batches: VecDeque<Batch<T>>,
    cursor: usize,
    bytes: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            model: None,
            batches: VecDeque::new(),
            cursor: 0,
            bytes: 0,
        }
    }
}

impl<T> History<T> {
    fn next(&self, redo: bool) -> Option<usize> {
        let index = if redo {
            Some(self.cursor)
        } else {
            self.cursor.checked_sub(1)
        };
        index.filter(|&index| index < self.batches.len())
    }

    fn bind(&mut self, model: u64) {
        if self.model != Some(model) {
            *self = Self {
                model: Some(model),
                ..Self::default()
            };
        }
    }

    fn remove(&mut self, index: usize) {
        let batch = self.batches.remove(index).expect("valid history index");
        self.bytes -= batch.bytes;
        if index < self.cursor {
            self.cursor -= 1;
        }
    }

    fn push(&mut self, batch: Batch<T>) {
        while self.batches.len() > self.cursor {
            self.remove(self.batches.len() - 1);
        }
        self.bytes += batch.bytes;
        self.batches.push_back(batch);
        self.cursor += 1;
        while self.batches.len() > MAX_HISTORY_BATCHES || self.bytes > MAX_HISTORY_BYTES {
            self.remove(0);
        }
    }
}

/// Edit history belongs to one table instance and follows stable model row keys.
#[doc(hidden)]
pub struct TableEditHistory<T> {
    history: RefCell<History<T>>,
    availability: [Property<Option<u64>>; 2],
}

#[doc(hidden)]
pub type TablePasteHistory<T> = TableEditHistory<T>;

impl<T> Default for TableEditHistory<T> {
    fn default() -> Self {
        Self {
            history: RefCell::default(),
            availability: Default::default(),
        }
    }
}

impl<T: Clone + PartialEq> TableEditHistory<T> {
    pub fn clear(&self) {
        *self.history.borrow_mut() = History::default();
        for available in &self.availability {
            available.set_if_changed(None);
        }
    }

    /// An entry exists for this model. Replay checks its field values before writing.
    pub fn can_replay(&self, model: &VecModel<T>, redo: bool) -> bool {
        self.availability[usize::from(redo)].get(|| None) == Some(model.edit_stamp().0)
    }

    pub fn commit_cell(
        &self,
        model: &VecModel<T>,
        index: usize,
        key: RowKey,
        update: impl FnOnce(&T) -> Option<T>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> bool {
        let mut batch = Batch::new();
        let mut recorded = false;
        let changed = model.update_row_if_key(index, key, |current| {
            let next = update(current)?;
            recorded = batch.capture(key, index, current, &next, capture);
            (!recorded || !batch.rows.is_empty()).then_some(next)
        });
        if changed {
            self.record_commit(key.model, recorded.then_some(batch));
        }
        changed
    }

    pub fn paste(
        &self,
        model: &VecModel<T>,
        input: TablePasteInput<'_>,
        assign: impl FnMut(usize, usize, &T, &mut T, &str) -> Option<()>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> Option<bool> {
        self.paste_mapped(model, input, Some, assign, capture)
    }

    pub fn paste_in_order(
        &self,
        model: &TableRows<VecModel<T>>,
        input: TablePasteInput<'_>,
        assign: impl FnMut(usize, usize, &T, &mut T, &str) -> Option<()>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> Option<bool> {
        self.paste_mapped(
            model.source(),
            input,
            |row| model.positions().source_index(row),
            assign,
            capture,
        )
    }

    fn paste_mapped(
        &self,
        model: &VecModel<T>,
        input: TablePasteInput<'_>,
        source_index: impl Fn(usize) -> Option<usize>,
        assign: impl FnMut(usize, usize, &T, &mut T, &str) -> Option<()>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> Option<bool> {
        let mut batch = Some(Batch::new());
        let staged = stage_table_text(
            model,
            input,
            source_index,
            assign,
            |index, before, after| {
                if let Some(current) = &mut batch
                    && !model
                        .row_key(index)
                        .is_some_and(|key| current.capture(key, index, before, after, &capture))
                {
                    batch = None;
                }
            },
        )?;
        if batch.as_ref().is_some_and(|batch| batch.rows.is_empty()) {
            return Some(false);
        }
        let changed = model.replace_rows_if_current(input.stamp, staged);
        if changed {
            self.record_commit(input.stamp.0, batch);
        }
        Some(changed)
    }

    fn record_commit(&self, model: u64, batch: Option<Batch<T>>) {
        let mut history = self.history.borrow_mut();
        history.bind(model);
        if let Some(batch) = batch {
            history.push(batch);
        } else {
            // An unrecordable commit is a boundary for older edits.
            *history = History {
                model: Some(model),
                ..History::default()
            };
        }
        self.publish(&history);
    }

    fn publish(&self, history: &History<T>) {
        for (available, redo) in self.availability.iter().zip([false, true]) {
            available.set_if_changed(history.model.filter(|_| history.next(redo).is_some()));
        }
    }

    pub fn replay(&self, model: &VecModel<T>, redo: bool) -> bool {
        let stamp = model.edit_stamp();
        let mut history = self.history.borrow_mut();
        history.bind(stamp.0);
        let Some(index) = history.next(redo) else {
            self.publish(&history);
            return false;
        };
        let Some(staged) = stage_replay(model, &history.batches[index], redo) else {
            history.remove(index);
            self.publish(&history);
            return false;
        };
        if !model.replace_rows_if_current(stamp, staged) {
            return false;
        }
        if redo {
            history.cursor += 1;
        } else {
            history.cursor -= 1;
        }
        self.publish(&history);
        true
    }
}

fn stage_replay<T: Clone>(
    model: &VecModel<T>,
    batch: &Batch<T>,
    redo: bool,
) -> Option<Vec<(usize, T)>> {
    let mut missing = BTreeMap::new();
    let mut positions: Vec<_> = batch
        .rows
        .iter()
        .enumerate()
        .map(|(slot, row)| {
            if model.row_key(row.hint) == Some(row.key) {
                Some(row.hint)
            } else {
                missing.insert(row.key, slot);
                None
            }
        })
        .collect();
    if !missing.is_empty() {
        let mut index = 0;
        model.visit_row_keys(0..model.len(), |key| {
            if let Some(slot) = missing.remove(&key) {
                positions[slot] = Some(index);
            }
            index += 1;
        });
    }
    batch
        .rows
        .iter()
        .zip(positions)
        .map(|(row, index)| {
            let index = index?;
            let mut current = model.row(index)?;
            for field in &row.patch.fields {
                if !field(&mut current, redo) {
                    return None;
                }
            }
            Some((index, current))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paste(
        history: &TablePasteHistory<String>,
        model: &VecModel<String>,
        text: &str,
        bytes: fn(&String) -> usize,
    ) -> bool {
        history
            .paste(
                model,
                TablePasteInput {
                    stamp: model.edit_stamp(),
                    row: 0,
                    columns: &[0],
                    text,
                },
                |_, _, _, next, text| {
                    *next = text.to_owned();
                    Some(())
                },
                |before, after, patch| patch.record(before, after, |row| row, PartialEq::eq, bytes),
            )
            .unwrap()
    }

    #[test]
    fn no_op_preserves_redo_and_a_new_commit_branches_history() {
        let history = TablePasteHistory::default();
        let model = VecModel::from(vec![String::from("original")]);
        let key = model.row_key(0).unwrap();
        let edit = |text: &str| {
            history.commit_cell(
                &model,
                0,
                key,
                |_| Some(text.into()),
                |before, after, patch| {
                    patch.record(before, after, |row| row, PartialEq::eq, String::len)
                },
            )
        };
        assert!(paste(&history, &model, "first", String::len));
        assert!(history.replay(&model, false));
        assert!(!edit("original"));
        assert!(history.replay(&model, true));
        assert_eq!(model.row(0).as_deref(), Some("first"));
        assert!(history.replay(&model, false));
        assert!(edit("branch"));
        assert!(!history.replay(&model, true));
        assert!(history.replay(&model, false));
        assert_eq!(model.row(0).as_deref(), Some("original"));
    }

    #[test]
    fn history_capacity_and_oversized_paste_keep_commits_bounded() {
        let history = TablePasteHistory::default();
        let model = VecModel::from(vec![String::from("0")]);
        for index in 1..=MAX_HISTORY_BATCHES + 2 {
            assert!(paste(&history, &model, &index.to_string(), String::len));
        }
        for _ in 0..MAX_HISTORY_BATCHES {
            assert!(history.replay(&model, false));
        }
        assert_eq!(model.row(0).as_deref(), Some("2"));
        assert!(!history.replay(&model, false));

        assert!(history.replay(&model, true));
        assert!(paste(&history, &model, "too large to retain", |_| {
            MAX_HISTORY_BYTES
        }));
        assert_eq!(model.row(0).as_deref(), Some("too large to retain"));
        assert!(!history.replay(&model, false));
        assert!(!history.replay(&model, true));
    }

    #[test]
    fn history_bindings_only_invalidate_when_their_direction_changes() {
        let history = TableEditHistory::default();
        let model = VecModel::from(vec![String::from("original")]);
        let undo = Property::default();
        let redo = Property::default();
        assert!(!undo.get(|| history.can_replay(&model, false)));
        assert!(!redo.get(|| history.can_replay(&model, true)));
        assert!(paste(&history, &model, "first", String::len));
        assert!(undo.is_dirty());
        assert!(!redo.is_dirty());
        assert!(undo.get(|| history.can_replay(&model, false)));
        assert!(paste(&history, &model, "second", String::len));
        assert!(!undo.is_dirty());
        assert!(history.replay(&model, false));
        assert!(!undo.is_dirty());
        assert!(redo.is_dirty());
        assert!(redo.get(|| history.can_replay(&model, true)));
        assert!(history.replay(&model, false));
        assert!(!redo.is_dirty());
        assert!(!undo.get(|| history.can_replay(&model, false)));
        assert!(model.set_row(0, "conflict".into()));
        assert!(!history.replay(&model, true));
        assert!(redo.get(|| history.can_replay(&model, true)));
        assert!(!history.replay(&model, true));
        assert!(!redo.get(|| history.can_replay(&model, true)));
    }
}
