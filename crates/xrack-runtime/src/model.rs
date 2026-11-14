use crate::{ChangeSignal, RowKey, RowLifetime};
use std::{
    cell::{Cell, RefCell},
    cmp::Ordering as Comparison,
    collections::BTreeMap,
    fmt,
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
            self.signals.retain(|_, signal| signal.has_observers());
            self.next_prune = (self.signals.len() * 2).max(64);
        }
        self.signals.entry(index).or_default().track();
    }
}

type ProjectionDiff<T> = dyn Fn(usize, &T, &T) -> bool;
pub(crate) type RowObserver<T> = dyn Fn(RowKey, Option<&T>);

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
    row_observers: RefCell<Vec<Weak<RowObserver<T>>>>,
}

/// A shared UI-thread model with separately tracked row and length reads.
/// Cloning the handle shares its data and subscriptions.
pub struct VecModel<T>(Rc<ModelData<T>>);

impl<T> fmt::Debug for VecModel<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VecModel")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}

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
            row_observers: RefCell::default(),
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
    pub(crate) fn is_shared(&self) -> bool {
        Rc::strong_count(&self.0) > 1
    }

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
            scope: 0,
        })
    }

    /// Find a record's current index by its stable identity.
    pub fn row_index(&self, key: RowKey) -> Option<usize> {
        self.0.structure.track();
        self.row_index_untracked(key)
    }

    pub(crate) fn row_index_untracked(&self, key: RowKey) -> Option<usize> {
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
                scope: 0,
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
                    scope: 0,
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

    /// Check an editor's field against the identified row without watching other fields.
    #[doc(hidden)]
    pub fn row_matches(&self, index: usize, key: RowKey, matches: impl FnOnce(&T) -> bool) -> bool {
        key.model == self.0.id
            && self
                .0
                .rows
                .borrow()
                .get(index)
                .is_some_and(|entry| entry.id == key.row && matches(&entry.value))
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

    /// Observe row contents by stable key. Observers run while the model may be borrowed.
    pub(crate) fn observe_rows(&self, observer: &Rc<RowObserver<T>>) {
        let mut observers = self.0.row_observers.borrow_mut();
        observers.retain(|observer| observer.strong_count() != 0);
        observers.push(Rc::downgrade(observer));
    }

    fn row_changed(&self, id: u64, value: Option<&T>) {
        let key = RowKey {
            model: self.0.id,
            row: id,
            scope: 0,
        };
        self.0.row_observers.borrow_mut().retain(|observer| {
            if let Some(observer) = observer.upgrade() {
                observer(key, value);
                true
            } else {
                false
            }
        });
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

    pub(crate) fn contents_stamp(&self) -> (u64, u64) {
        self.0.values.track();
        self.edit_stamp()
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
            let entry = self.entry(value);
            self.row_changed(entry.id, Some(&entry.value));
            rows.push(entry);
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
            let entry = self.entry(value);
            self.row_changed(entry.id, Some(&entry.value));
            rows.insert(index, entry);
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
        self.row_changed(removed.id, None);
        self.0.lifetimes.borrow_mut().remove(&removed.id);
        self.structure_changed(index, true);
        Some(removed.value)
    }

    pub(crate) fn sorted_indices_untracked(
        &self,
        limit: usize,
        compare: impl FnMut(&T, &T) -> Comparison,
    ) -> Vec<usize> {
        self.projected_indices_untracked(limit, None, |_| true, Some(compare))
    }

    pub(crate) fn projected_indices_untracked(
        &self,
        limit: usize,
        base_order: Option<&[usize]>,
        include: impl Fn(&T) -> bool,
        compare: Option<impl FnMut(&T, &T) -> Comparison>,
    ) -> Vec<usize> {
        let rows = self.0.rows.borrow();
        let mut order: Vec<_> = (0..rows.len().min(limit))
            .map(|index| base_order.map_or(index, |order| order[index]))
            .filter(|&index| include(&rows[index].value))
            .collect();
        if let Some(mut compare) = compare {
            order.sort_by(|&a, &b| compare(&rows[a].value, &rows[b].value));
        }
        order
    }

    /// Stably reorder rows, retaining their keys and delegate lifetimes.
    pub fn sort_by(&self, compare: impl FnMut(&T, &T) -> Comparison) -> bool {
        let order = self.sorted_indices_untracked(usize::MAX, compare);
        let Some(first_changed) = order.iter().enumerate().position(|(new, old)| new != *old)
        else {
            return false;
        };
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
        // Notify readers before discarding signals for indices whose rows have shifted.
        let shifted = self.0.row_signals.borrow_mut().signals.split_off(&index);
        for signal in shifted.into_values() {
            signal.notify();
        }
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

    /// Resolve a DSL model index with the same dependency tracking as a row read.
    #[doc(hidden)]
    pub fn row_at(&self, index: i64) -> Option<T> {
        self.row(usize::try_from(index).ok()?)
    }

    pub fn snapshot(&self) -> Vec<T> {
        self.0.values.track();
        self.snapshot_untracked()
    }

    /// Copy rows that the caller has already sampled from the live model.
    #[doc(hidden)]
    pub fn snapshot_untracked(&self) -> Vec<T> {
        self.0
            .rows
            .borrow()
            .iter()
            .map(|entry| entry.value.clone())
            .collect()
    }

    /// Transfer an owned model's rows at a by-value boundary. A shared model
    /// still needs a tracked read because its other owners can edit it.
    #[doc(hidden)]
    pub fn into_snapshot(self) -> Vec<T> {
        match Rc::try_unwrap(self.0) {
            Ok(data) => data
                .rows
                .into_inner()
                .into_iter()
                .map(|entry| entry.value)
                .collect(),
            Err(data) => Self(data).snapshot(),
        }
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
            self.swap_row(index, current, &mut value)
        };
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
            changed |= self.swap_row(*index, &mut rows[*index], value);
        }
        drop(rows);
        if changed {
            self.values_changed();
        }
        changed
    }

    fn swap_row(&self, index: usize, entry: &mut Entry<T>, next: &mut T) -> bool {
        if &entry.value == next {
            return false;
        }
        self.0.projections.borrow_mut().retain(|projection| {
            let Some(signal) = projection.signal.upgrade() else {
                return false;
            };
            if index < projection.limit && (projection.changed)(index, &entry.value, next) {
                signal.notify();
            }
            true
        });
        std::mem::swap(&mut entry.value, next);
        self.row_changed(entry.id, Some(&entry.value));
        if let Some(signal) = self.0.row_signals.borrow().signals.get(&index) {
            signal.notify();
        }
        true
    }

    /// Compare the current values, independently of handle identity.
    pub fn values_eq(&self, other: &Self) -> bool {
        self.compare_values(other, |model| model.0.values.track())
    }

    /// Compare expression operands, tracking only models that can outlive
    /// the comparison and be changed by another owner.
    #[doc(hidden)]
    pub fn values_eq_owned(self, other: Self) -> bool {
        self.compare_values(&other, |model| {
            if model.is_shared() {
                model.0.values.track();
            }
        })
    }

    fn compare_values(&self, other: &Self, track: impl Fn(&Self)) -> bool {
        if self == other {
            return true;
        }
        track(self);
        track(other);
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
    fn watched_model_rows_survive_pruning_and_detect_shifts() {
        use crate::property::watch_reads;

        let model = VecModel::from((0..128).collect::<Vec<_>>());
        watch_reads(|reads| {
            for index in 0..100 {
                assert_eq!(model.row(index), Some(index));
            }
            assert!(model.set_row(120, 0));
            assert!(!reads.changed());
            assert!(model.set_row(0, 120));
            assert!(reads.changed());
        });
        watch_reads(|reads| {
            assert_eq!(model.row(1), Some(1));
            assert_eq!(model.remove(0), Some(120));
            assert!(reads.changed());
        });
    }

    #[test]
    fn consuming_snapshot_tracks_only_models_with_a_live_owner() {
        let temporary = Property::default();
        assert_eq!(
            temporary.get(|| VecModel::from(vec![1]).into_snapshot()),
            [1]
        );
        assert!(!temporary.is_dirty());

        let shared = VecModel::from(vec![2]);
        let snapshot = Property::default();
        let read = || snapshot.get(|| shared.clone().into_snapshot());
        assert_eq!(read(), [2]);
        assert!(!snapshot.is_dirty());
        assert!(shared.set_row(0, 3));
        assert!(snapshot.is_dirty());
        assert_eq!(read(), [3]);
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
