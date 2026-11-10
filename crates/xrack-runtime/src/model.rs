use crate::{ChangeSignal, RowKey, RowLifetime};
use std::{
    cell::{Cell, RefCell},
    cmp::Ordering as Comparison,
    collections::BTreeMap,
    ops::Range,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_MODEL: AtomicU64 = AtomicU64::new(1);

struct Entry<T> {
    id: u64,
    value: T,
}

#[derive(Default)]
struct RowSignals {
    signals: BTreeMap<usize, ChangeSignal>,
    next_prune: usize,
}

impl RowSignals {
    fn track(&mut self, index: usize) {
        if !self.signals.contains_key(&index) && self.signals.len() >= self.next_prune {
            self.signals.retain(|_, signal| signal.has_dependents());
            self.next_prune = (self.signals.len() * 2).max(64);
        }
        self.signals.entry(index).or_default().track();
    }
}

type ProjectionDiff<T> = dyn Fn(usize, &T, &T) -> bool;

struct RowProjection<T> {
    signal: Weak<ChangeSignal>,
    limit: usize,
    changed: Box<ProjectionDiff<T>>,
}

struct ModelData<T> {
    id: u64,
    rows: RefCell<Vec<Entry<T>>>,
    next_row: Cell<u64>,
    revision: Cell<u64>,
    edit_revision: Cell<u64>,
    lifetimes: RefCell<BTreeMap<u64, Rc<()>>>,
    count: ChangeSignal,
    structure: ChangeSignal,
    values: ChangeSignal,
    row_signals: RefCell<RowSignals>,
    projections: RefCell<Vec<RowProjection<T>>>,
}

/// A shared UI-thread model with separately tracked row and length reads.
/// Cloning the handle shares its data and subscriptions.
pub struct VecModel<T>(Rc<ModelData<T>>);

impl<T> Clone for VecModel<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> From<Vec<T>> for VecModel<T> {
    fn from(rows: Vec<T>) -> Self {
        let id = NEXT_MODEL
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("model identity exhausted");
        let next_row = rows.len() as u64;
        Self(Rc::new(ModelData {
            id,
            rows: RefCell::new(
                rows.into_iter()
                    .enumerate()
                    .map(|(id, value)| Entry {
                        id: id as u64,
                        value,
                    })
                    .collect(),
            ),
            next_row: Cell::new(next_row),
            revision: Cell::default(),
            edit_revision: Cell::default(),
            lifetimes: RefCell::default(),
            count: ChangeSignal::default(),
            structure: ChangeSignal::default(),
            values: ChangeSignal::default(),
            row_signals: RefCell::default(),
            projections: RefCell::default(),
        }))
    }
}

impl<T> Default for VecModel<T> {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}

impl<T> PartialEq for VecModel<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for VecModel<T> {}

impl<T> VecModel<T> {
    fn entry(&self, value: T) -> Entry<T> {
        let id = self.0.next_row.get();
        self.0
            .next_row
            .set(id.checked_add(1).expect("model row identity exhausted"));
        Entry { id, value }
    }

    /// A stable identity that survives edits, inserts, removes of other rows, and sorting.
    pub fn row_key(&self, index: usize) -> Option<RowKey> {
        self.0.structure.track();
        self.0.rows.borrow().get(index).map(|entry| RowKey {
            model: self.0.id,
            row: entry.id,
        })
    }

    /// Find a record's current index by its stable identity.
    pub fn row_index(&self, key: RowKey) -> Option<usize> {
        self.0.structure.track();
        if key.model != self.0.id {
            return None;
        }
        self.0
            .rows
            .borrow()
            .iter()
            .position(|entry| entry.id == key.row)
    }

    pub(crate) fn visit_row_keys(&self, range: Range<usize>, mut visit: impl FnMut(RowKey)) {
        self.0.structure.track();
        for entry in &self.0.rows.borrow()[range] {
            visit(RowKey {
                model: self.0.id,
                row: entry.id,
            });
        }
    }

    pub(crate) fn visit_row_data(&self, count: usize, visit: impl FnMut(usize, RowKey, &T)) {
        self.0.values.track();
        self.visit_rows_untracked(count, visit);
    }

    /// Borrow the rows after the caller has established the appropriate dependencies.
    pub(crate) fn visit_rows_untracked(
        &self,
        count: usize,
        mut visit: impl FnMut(usize, RowKey, &T),
    ) {
        for (index, entry) in self.0.rows.borrow().iter().take(count).enumerate() {
            visit(
                index,
                RowKey {
                    model: self.0.id,
                    row: entry.id,
                },
                &entry.value,
            );
        }
    }

    /// Locate a row after the caller has established the appropriate dependencies.
    pub(crate) fn position_untracked(
        &self,
        range: std::ops::Range<usize>,
        mut predicate: impl FnMut(&T) -> bool,
    ) -> Option<usize> {
        let start = range.start;
        self.0
            .rows
            .borrow()
            .get(range)?
            .iter()
            .position(|entry| predicate(&entry.value))
            .map(|offset| start + offset)
    }

    pub(crate) fn project_row_untracked<R>(
        &self,
        index: usize,
        project: impl FnOnce(&T) -> R,
    ) -> Option<R> {
        self.0
            .rows
            .borrow()
            .get(index)
            .map(|entry| project(&entry.value))
    }

    /// Subscribe to changes in the projected value of existing rows. The caller also tracks
    /// structure changes and keeps the returned signal alive for as long as it uses the projection.
    pub(crate) fn observe_projection<P: PartialEq>(
        &self,
        limit: usize,
        project: impl Fn(&T) -> P + 'static,
    ) -> Rc<ChangeSignal> {
        self.observe_projection_changes(limit, move |_, before, after| {
            project(before) != project(after)
        })
    }

    pub(crate) fn observe_projection_changes(
        &self,
        limit: usize,
        changed: impl Fn(usize, &T, &T) -> bool + 'static,
    ) -> Rc<ChangeSignal> {
        let signal = Rc::new(ChangeSignal::default());
        let mut projections = self.0.projections.borrow_mut();
        projections.retain(|projection| projection.signal.strong_count() != 0);
        projections.push(RowProjection {
            signal: Rc::downgrade(&signal),
            limit,
            changed: Box::new(changed),
        });
        signal
    }

    pub(crate) fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        let mut lifetimes = self.0.lifetimes.borrow_mut();
        RowLifetime(Some(Rc::downgrade(lifetimes.entry(key.row).or_default())))
    }

    pub(crate) fn structure_stamp(&self) -> (u64, u64) {
        self.0.structure.track();
        (self.0.id, self.0.revision.get())
    }

    /// Identify the exact model contents and order seen by an asynchronous edit.
    #[doc(hidden)]
    pub fn edit_stamp(&self) -> (u64, u64) {
        (self.0.id, self.0.edit_revision.get())
    }

    pub fn len(&self) -> usize {
        self.0.count.track();
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&self, value: T) {
        let index = {
            let mut rows = self.0.rows.borrow_mut();
            let index = rows.len();
            rows.push(self.entry(value));
            index
        };
        self.structure_changed(index, true);
    }

    /// Insert at a model index. An index beyond the end leaves the model unchanged.
    pub fn insert(&self, index: usize, value: T) -> bool {
        {
            let mut rows = self.0.rows.borrow_mut();
            if index > rows.len() {
                return false;
            }
            rows.insert(index, self.entry(value));
        }
        self.structure_changed(index, true);
        true
    }

    pub fn remove(&self, index: usize) -> Option<T> {
        let removed = {
            let mut rows = self.0.rows.borrow_mut();
            if index >= rows.len() {
                return None;
            }
            rows.remove(index)
        };
        self.0.lifetimes.borrow_mut().remove(&removed.id);
        self.structure_changed(index, true);
        Some(removed.value)
    }

    /// Stably reorder rows, retaining their keys and delegate lifetimes.
    pub fn sort_by(&self, mut compare: impl FnMut(&T, &T) -> Comparison) -> bool {
        let rows = self.0.rows.borrow();
        let mut order: Vec<_> = (0..rows.len()).collect();
        order.sort_by(|&a, &b| compare(&rows[a].value, &rows[b].value));
        let Some(first_changed) = order.iter().enumerate().position(|(new, old)| new != *old)
        else {
            return false;
        };
        drop(rows);

        {
            let mut rows = self.0.rows.borrow_mut();
            let mut source: Vec<_> = rows.drain(first_changed..).map(Some).collect();
            rows.extend(order.into_iter().skip(first_changed).map(|index| {
                source[index - first_changed]
                    .take()
                    .expect("sort permutation contains each row once")
            }));
        }
        self.structure_changed(first_changed, false);
        true
    }

    fn structure_changed(&self, index: usize, count_changed: bool) {
        self.0.revision.set(self.0.revision.get().wrapping_add(1));
        // Dropping the old signals invalidates reads at the shifted indices.
        let shifted = self.0.row_signals.borrow_mut().signals.split_off(&index);
        drop(shifted);
        if count_changed {
            self.0.count.notify();
        }
        self.0.structure.notify();
        self.values_changed();
    }

    fn values_changed(&self) {
        self.0
            .edit_revision
            .set(self.0.edit_revision.get().wrapping_add(1));
        self.0.values.notify();
    }
}

impl<T> VecModel<T> {
    pub(crate) fn project_row<R>(&self, index: usize, project: impl FnOnce(&T) -> R) -> Option<R> {
        let rows = self.0.rows.borrow();
        if let Some(entry) = rows.get(index) {
            if ChangeSignal::is_tracking() {
                self.0.row_signals.borrow_mut().track(index);
            }
            Some(project(&entry.value))
        } else {
            self.0.count.track();
            None
        }
    }
}

impl<T: Clone> VecModel<T> {
    pub fn row(&self, index: usize) -> Option<T> {
        self.project_row(index, T::clone)
    }

    pub fn snapshot(&self) -> Vec<T> {
        self.0.values.track();
        self.0
            .rows
            .borrow()
            .iter()
            .map(|entry| entry.value.clone())
            .collect()
    }
}

impl<T: PartialEq> VecModel<T> {
    /// Commit a value against the row and value observed by an editor.
    pub fn set_row_if_current(&self, index: usize, key: RowKey, expected: &T, value: T) -> bool {
        self.update_row_if_key(index, key, |current| (current == expected).then_some(value))
    }

    /// Update an identified record from its latest value. Returning None declines the edit.
    pub fn update_row_if_key(
        &self,
        index: usize,
        key: RowKey,
        update: impl FnOnce(&T) -> Option<T>,
    ) -> bool {
        let next = {
            let rows = self.0.rows.borrow();
            let Some(entry) = rows
                .get(index)
                .filter(|entry| key.model == self.0.id && entry.id == key.row)
            else {
                return false;
            };
            update(&entry.value)
        };
        next.is_some_and(|value| self.set_row(index, value))
    }

    /// Replace one row. Returns false for an invalid index or equal value.
    pub fn set_row(&self, index: usize, value: T) -> bool {
        let mut value = value;
        let changed = {
            let mut rows = self.0.rows.borrow_mut();
            let Some(current) = rows.get_mut(index) else {
                return false;
            };
            self.swap_row(index, &mut current.value, &mut value)
        };
        if changed {
            self.values_changed();
        }
        changed
    }

    pub(crate) fn replace_range_if_current(
        &self,
        stamp: (u64, u64),
        start: usize,
        mut values: Vec<T>,
    ) -> bool {
        if self.edit_stamp() != stamp {
            return false;
        }
        let Some(end) = start.checked_add(values.len()) else {
            return false;
        };
        let mut rows = self.0.rows.borrow_mut();
        let Some(entries) = rows.get_mut(start..end) else {
            return false;
        };
        let mut changed = false;
        for (offset, (entry, value)) in entries.iter_mut().zip(&mut values).enumerate() {
            changed |= self.swap_row(start + offset, &mut entry.value, value);
        }
        drop(rows);
        if changed {
            self.values_changed();
        }
        changed
    }

    pub(crate) fn replace_rows_if_current(
        &self,
        stamp: (u64, u64),
        mut values: Vec<(usize, T)>,
    ) -> bool {
        if self.edit_stamp() != stamp {
            return false;
        }
        let mut rows = self.0.rows.borrow_mut();
        if values.iter().any(|(index, _)| *index >= rows.len()) {
            return false;
        }
        let mut changed = false;
        for (index, value) in &mut values {
            changed |= self.swap_row(*index, &mut rows[*index].value, value);
        }
        drop(rows);
        if changed {
            self.values_changed();
        }
        changed
    }

    fn swap_row(&self, index: usize, current: &mut T, next: &mut T) -> bool {
        if current == next {
            return false;
        }
        self.0.projections.borrow_mut().retain(|projection| {
            let Some(signal) = projection.signal.upgrade() else {
                return false;
            };
            if index < projection.limit && (projection.changed)(index, current, next) {
                signal.notify();
            }
            true
        });
        std::mem::swap(current, next);
        if let Some(signal) = self.0.row_signals.borrow().signals.get(&index) {
            signal.notify();
        }
        true
    }

    /// Compare the current values, independently of handle identity.
    pub fn values_eq(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        self.0.values.track();
        other.0.values.track();
        self.0.rows.borrow().iter().map(|row| &row.value).eq(other
            .0
            .rows
            .borrow()
            .iter()
            .map(|row| &row.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Property;
    use std::cell::Cell;

    #[test]
    fn sorting_retains_stable_keys_and_invalidates_only_affected_reads() {
        let model = VecModel::from(vec![(0, 'a'), (2, 'b'), (1, 'c'), (2, 'd')]);
        let keys: Vec<_> = (0..4).map(|index| model.row_key(index).unwrap()).collect();
        let lifetime = model.row_lifetime(keys[2]);
        let len = Property::default();
        let first = Property::default();
        let second = Property::default();
        let count_reads = Cell::new(0);
        let prefix_reads = Cell::new(0);
        let count = || {
            len.get(|| {
                count_reads.set(count_reads.get() + 1);
                model.len()
            })
        };
        let prefix = || {
            first.get(|| {
                prefix_reads.set(prefix_reads.get() + 1);
                model.row(0)
            })
        };
        let row = || second.get(|| model.row(1));
        assert_eq!(
            (count(), prefix(), row()),
            (4, Some((0, 'a')), Some((2, 'b')))
        );
        assert!(model.sort_by(|a, b| a.0.cmp(&b.0)));
        assert_eq!(
            (count(), prefix(), row()),
            (4, Some((0, 'a')), Some((1, 'c')))
        );
        assert_eq!((count_reads.get(), prefix_reads.get()), (1, 1));
        assert_eq!(model.snapshot(), [(0, 'a'), (1, 'c'), (2, 'b'), (2, 'd')]);
        assert_eq!(model.row_key(1), Some(keys[2]));
        assert_eq!(model.row_key(2), Some(keys[1]));
        assert_eq!(model.row_key(3), Some(keys[3]));
        assert!(lifetime.is_live(keys[2], model.structure_stamp().0, 4));
        assert!(!model.sort_by(|a, b| a.0.cmp(&b.0)));
    }

    #[test]
    fn model_tracks_rows_length_and_snapshots_independently() {
        let model = VecModel::from(vec![10, 20, 30]);
        let count = Property::default();
        let second = Property::default();
        let missing = Property::default();
        let snapshot = Property::default();
        let reads = Cell::new(0);
        let len = || {
            count.get(|| {
                reads.set(reads.get() + 1);
                model.len()
            })
        };
        let row = || second.get(|| model.row(1));
        let tail = || missing.get(|| model.row(3));
        let values = || snapshot.get(|| model.snapshot());

        assert_eq!((len(), row(), tail()), (3, Some(20), None));
        assert_eq!(values(), [10, 20, 30]);
        assert!(model.set_row(0, 11));
        assert_eq!((len(), row()), (3, Some(20)));
        assert_eq!(reads.get(), 1);
        assert_eq!(values(), [11, 20, 30]);
        assert!(!model.set_row(0, 11));
        assert!(!model.set_row(9, 11));
        model.push(40);
        assert_eq!((len(), row(), tail()), (4, Some(20), Some(40)));
        assert!(model.insert(0, 5));
        assert_eq!((len(), row(), tail()), (5, Some(11), Some(30)));
        assert_eq!(model.remove(0), Some(5));
        assert_eq!((len(), row(), tail()), (4, Some(20), Some(40)));
    }
}
