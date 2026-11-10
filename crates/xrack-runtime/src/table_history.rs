use crate::{RowKey, TablePasteInput, VecModel, table_clipboard::stage_table_text};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    mem::size_of,
};

const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_HISTORY_BATCHES: usize = 64;

type FieldChange<T> = Box<dyn Fn(&mut T, bool) -> bool>;

/// A typed patch for just the fields changed by a paste.
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

/// Paste history belongs to one table instance and follows stable model row keys.
#[doc(hidden)]
pub struct TablePasteHistory<T>(RefCell<History<T>>);

impl<T> Default for TablePasteHistory<T> {
    fn default() -> Self {
        Self(RefCell::default())
    }
}

impl<T: Clone + PartialEq> TablePasteHistory<T> {
    pub fn paste(
        &self,
        model: &VecModel<T>,
        input: TablePasteInput<'_>,
        assign: impl FnMut(usize, usize, &T, &mut T, &str) -> Option<()>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
    ) -> Option<bool> {
        let mut batch = Some(Batch {
            rows: Vec::new(),
            bytes: size_of::<Batch<T>>(),
        });
        let staged = stage_table_text(model, input, assign, |index, before, after| {
            let Some(current) = &mut batch else {
                return;
            };
            let mut patch = TableRowPatch {
                fields: Vec::new(),
                bytes: 0,
                limit: MAX_HISTORY_BYTES - current.bytes,
            };
            capture(before, after, &mut patch);
            if patch.bytes > patch.limit {
                batch = None;
            } else if !patch.fields.is_empty() {
                current.bytes += patch.bytes + 2 * size_of::<HistoryRow<T>>();
                if current.bytes > MAX_HISTORY_BYTES {
                    batch = None;
                } else if let Some(key) = model.row_key(index) {
                    current.rows.push(HistoryRow {
                        key,
                        hint: index,
                        patch,
                    });
                }
            }
        })?;
        if batch.as_ref().is_some_and(|batch| batch.rows.is_empty()) {
            return Some(false);
        }
        let changed = model.replace_range_if_current(input.stamp, input.row, staged);
        if changed {
            let mut history = self.0.borrow_mut();
            history.bind(input.stamp.0);
            if let Some(batch) = batch.filter(|batch| !batch.rows.is_empty()) {
                history.push(batch);
            } else {
                // An unrecordable commit is a boundary for older edits.
                *history = History {
                    model: Some(input.stamp.0),
                    ..History::default()
                };
            }
        }
        Some(changed)
    }

    pub fn replay(&self, model: &VecModel<T>, redo: bool) -> bool {
        let stamp = model.edit_stamp();
        let mut history = self.0.borrow_mut();
        history.bind(stamp.0);
        let index = if redo {
            Some(history.cursor)
        } else {
            history.cursor.checked_sub(1)
        };
        let Some(index) = index.filter(|&index| index < history.batches.len()) else {
            return false;
        };
        let Some(staged) = stage_replay(model, &history.batches[index], redo) else {
            history.remove(index);
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
        assert!(paste(&history, &model, "first", String::len));
        assert!(history.replay(&model, false));
        assert!(!paste(&history, &model, "original", String::len));
        assert!(history.replay(&model, true));
        assert_eq!(model.row(0).as_deref(), Some("first"));
        assert!(history.replay(&model, false));
        assert!(paste(&history, &model, "branch", String::len));
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
}
