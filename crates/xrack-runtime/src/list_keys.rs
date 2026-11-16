//! Index a list's business keys while its model maintains stable row identities.
use crate::{RowKey, VecModel, list::MAX_ROWS, model::RowObserver, property::ChangeSignal};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet, hash_map::Entry},
    rc::Rc,
};

/// Resolves the unique, nonnegative business keys configured on a list view.
///
/// The key projection stays fixed for this view's lifetime.
#[derive(Default)]
#[doc(hidden)]
pub struct ListSelectionKeys {
    index: Rc<RefCell<Index>>,
    observer: RefCell<Option<Box<dyn Any>>>,
}

struct Row {
    key: i64,
    position: Option<usize>,
}

#[derive(Default)]
struct Index {
    stamp: Option<(u64, u64)>,
    rows: HashMap<u64, Row>,
    unique: HashMap<i64, u64>,
    duplicates: HashMap<i64, HashSet<u64>>,
    signals: HashMap<i64, ChangeSignal>,
    row_signals: HashMap<u64, ChangeSignal>,
    next_prune: usize,
    next_row_prune: usize,
}

impl Index {
    fn add(&mut self, key: i64, row: u64) {
        if key < 0 {
            return;
        }
        if let Some(rows) = self.duplicates.get_mut(&key) {
            rows.insert(row);
        } else {
            match self.unique.entry(key) {
                Entry::Vacant(slot) => {
                    slot.insert(row);
                }
                Entry::Occupied(previous) => {
                    self.duplicates.insert(key, [previous.remove(), row].into());
                }
            }
        }
    }

    fn remove(&mut self, key: i64, row: u64) {
        if key < 0 {
            return;
        }
        if let Some(rows) = self.duplicates.get_mut(&key) {
            rows.remove(&row);
            if rows.len() == 1 {
                let remaining = *rows.iter().next().unwrap();
                self.duplicates.remove(&key);
                self.unique.insert(key, remaining);
            }
        } else {
            self.unique.remove(&key);
        }
    }

    fn update(&mut self, row_id: u64, key: i64) {
        let Some(row) = self.rows.get_mut(&row_id) else {
            return;
        };
        let previous = row.key;
        if previous == key {
            return;
        }
        row.key = key;
        if let Some(signal) = self.row_signals.get(&row_id) {
            signal.notify();
        }
        let previous_owner = self.unique.get(&previous).copied();
        let new_owner = self.unique.get(&key).copied();
        self.remove(previous, row_id);
        self.add(key, row_id);
        for (key, before) in [(previous, previous_owner), (key, new_owner)] {
            if self.unique.get(&key).copied() != before
                && let Some(signal) = self.signals.get(&key)
            {
                signal.notify();
            }
        }
    }

    fn track(&mut self, key: i64) {
        if !ChangeSignal::is_tracking() {
            return;
        }
        if !self.signals.contains_key(&key) && self.signals.len() >= self.next_prune {
            self.signals.retain(|_, signal| signal.has_observers());
            self.next_prune = (self.signals.len() * 2).max(64);
        }
        self.signals.entry(key).or_default().track();
    }

    fn track_row(&mut self, row: u64) {
        if !ChangeSignal::is_tracking() {
            return;
        }
        if !self.row_signals.contains_key(&row) && self.row_signals.len() >= self.next_row_prune {
            self.row_signals.retain(|_, signal| signal.has_observers());
            self.next_row_prune = (self.row_signals.len() * 2).max(64);
        }
        self.row_signals.entry(row).or_default().track();
    }
}

impl ListSelectionKeys {
    fn synchronize<T: 'static>(&self, model: &VecModel<T>, key_of: impl Fn(&T) -> i64 + 'static) {
        let stamp = model.structure_stamp();
        let mut index = self.index.borrow_mut();
        if index.stamp == Some(stamp) {
            return;
        }
        let replaced = index.stamp.map(|stamp| stamp.0) != Some(stamp.0);
        if replaced {
            // A locator from an older frame can still use this cache. Invalidate
            // readers before switching the row observer to another model.
            for (_, signal) in index.signals.drain() {
                signal.notify();
            }
            for (_, signal) in index.row_signals.drain() {
                signal.notify();
            }
            index.rows.clear();
            index.unique.clear();
            index.duplicates.clear();
        }
        index.stamp = Some(stamp);
        for row in index.rows.values_mut() {
            row.position = None;
        }
        model.visit_rows_untracked(MAX_ROWS, |position, id, record| {
            match index.rows.entry(id.row) {
                Entry::Occupied(mut slot) => slot.get_mut().position = Some(position),
                Entry::Vacant(slot) => {
                    let key = key_of(record);
                    slot.insert(Row {
                        key,
                        position: Some(position),
                    });
                    index.add(key, id.row);
                }
            }
        });
        let mut removed = Vec::new();
        index.rows.retain(|&id, row| {
            if row.position.is_none() {
                removed.push((row.key, id));
                false
            } else {
                true
            }
        });
        for (key, id) in removed {
            index.remove(key, id);
            index.row_signals.remove(&id);
        }

        if replaced {
            let index = Rc::downgrade(&self.index);
            let observer: Rc<RowObserver<T>> = Rc::new(move |id, value| {
                if let Some(index) = index.upgrade()
                    && let Some(value) = value
                {
                    let mut index = index.borrow_mut();
                    // Inserts and rows outside the view's maximum range will
                    // be handled by the next structural reconciliation.
                    if index.stamp.is_some_and(|stamp| stamp.0 == id.model)
                        && index.rows.contains_key(&id.row)
                    {
                        index.update(id.row, key_of(value));
                    }
                }
            });
            model.observe_rows(&observer);
            self.observer.replace(Some(Box::new(observer)));
        }
    }

    pub fn find<T: 'static>(
        &self,
        model: &VecModel<T>,
        key: i64,
        key_of: impl Fn(&T) -> i64 + 'static,
    ) -> Option<usize> {
        if key < 0 {
            return None;
        }
        self.synchronize(model, key_of);
        let mut index = self.index.borrow_mut();
        index.track(key);
        index.rows.get(index.unique.get(&key)?)?.position
    }

    pub fn key_at<T: 'static>(
        &self,
        model: &VecModel<T>,
        source: usize,
        key_of: impl Fn(&T) -> i64 + 'static,
    ) -> Option<i64> {
        if source >= MAX_ROWS {
            return None;
        }
        let id = model.row_key(source)?.row;
        self.synchronize(model, key_of);
        let mut index = self.index.borrow_mut();
        let key = index.rows.get(&id)?.key;
        if key < 0 {
            return None;
        }
        index.track(key);
        (index.unique.get(&key) == Some(&id)).then_some(key)
    }

    /// Resolve a batch against the full source, including filtered rows.
    #[doc(hidden)]
    pub fn resolve_keys<T: 'static>(
        &self,
        model: &VecModel<T>,
        keys: &[i64],
        key_of: impl Fn(&T) -> i64 + 'static,
    ) -> Option<BTreeSet<RowKey>> {
        if keys.is_empty() {
            return Some(BTreeSet::new());
        }
        self.synchronize(model, key_of);
        let mut index = self.index.borrow_mut();
        keys.iter()
            .map(|&key| {
                index.track(key);
                let position = index.rows.get(index.unique.get(&key)?)?.position?;
                model.row_key(position)
            })
            .collect()
    }

    /// Export only unambiguous keys and observe the selected records' key roles.
    #[doc(hidden)]
    pub fn keys_for_rows<T: 'static>(
        &self,
        model: &VecModel<T>,
        rows: &BTreeSet<RowKey>,
        key_of: impl Fn(&T) -> i64 + 'static,
    ) -> Vec<i64> {
        if rows.is_empty() {
            return Vec::new();
        }
        self.synchronize(model, key_of);
        let mut index = self.index.borrow_mut();
        let mut keys = rows
            .iter()
            .filter_map(|row| {
                if index.stamp.is_none_or(|(model, _)| model != row.model) {
                    return None;
                }
                let key = index.rows.get(&row.row)?.key;
                index.track_row(row.row);
                if key < 0 {
                    return None;
                }
                index.track(key);
                (index.unique.get(&key) == Some(&row.row)).then_some(key)
            })
            .collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Property;
    use std::cell::Cell;

    #[test]
    fn selected_key_snapshots_track_key_edits_and_uniqueness_without_tracking_payload() {
        let model = VecModel::from(vec![(1_i64, false), (2, false), (3, false)]);
        let keys = ListSelectionKeys::default();
        let selection = keys.resolve_keys(&model, &[2, 1, 2], |row| row.0).unwrap();
        let snapshot = Property::default();
        let read = || snapshot.get(|| keys.keys_for_rows(&model, &selection, |row| row.0));
        assert_eq!(read(), [1, 2]);

        assert!(model.set_row(0, (1, true)));
        assert!(!snapshot.is_dirty());
        assert!(model.set_row(0, (-1, true)));
        assert!(snapshot.is_dirty());
        assert_eq!(read(), [2]);
        assert!(model.set_row(0, (4, true)));
        assert!(snapshot.is_dirty());
        assert_eq!(read(), [2, 4]);

        assert!(model.set_row(2, (2, false)));
        assert_eq!(read(), [4]);
        assert!(keys.resolve_keys(&model, &[4, 2], |row| row.0).is_none());
        assert!(model.set_row(2, (3, false)));
        assert_eq!(read(), [2, 4]);
    }

    #[test]
    fn large_models_only_project_changed_or_new_records() {
        const ROWS: i64 = 50_000;
        let model = VecModel::from((0..ROWS).map(|id| (id, false)).collect::<Vec<_>>());
        let keys = ListSelectionKeys::default();
        let reads = Rc::new(Cell::new(0));
        let project = {
            let reads = reads.clone();
            move |row: &(i64, bool)| {
                reads.set(reads.get() + 1);
                row.0
            }
        };
        let chosen = Property::default();
        let read = || chosen.get(|| keys.find(&model, 100, project.clone()));

        assert_eq!(keys.find(&model, -1, project.clone()), None);
        assert_eq!(reads.get(), 0);
        assert_eq!(read(), Some(100));
        assert_eq!(reads.get(), ROWS);
        assert!(model.set_row(10, (10, true)));
        assert!(!chosen.is_dirty());
        assert_eq!(read(), Some(100));
        assert_eq!(reads.get(), ROWS + 1);

        assert!(model.set_row(10, (ROWS, true)));
        assert!(!chosen.is_dirty());
        assert_eq!(keys.find(&model, ROWS, project.clone()), Some(10));
        assert_eq!(reads.get(), ROWS + 2);

        assert!(model.sort_by(|left, right| right.0.cmp(&left.0)));
        assert!(chosen.is_dirty());
        assert_eq!(read(), Some((ROWS - 100) as usize));
        assert_eq!(reads.get(), ROWS + 2);

        assert!(model.insert(0, (ROWS + 1, false)));
        assert_eq!(read(), Some((ROWS - 99) as usize));
        assert_eq!(reads.get(), ROWS + 3);
        assert_eq!(model.remove(0), Some((ROWS + 1, false)));
        assert_eq!(read(), Some((ROWS - 100) as usize));
        assert_eq!(reads.get(), ROWS + 3);
    }

    #[test]
    fn duplicate_keys_invalidate_only_when_the_unique_match_changes() {
        let model = VecModel::from(vec![1_i64, 2, 3]);
        let keys = ListSelectionKeys::default();
        let first = Property::default();
        let second = Property::default();
        let lookup_first = || first.get(|| keys.find(&model, 1, |key| *key));
        let lookup_second = || second.get(|| keys.find(&model, 2, |key| *key));
        assert_eq!(lookup_first(), Some(0));
        assert_eq!(lookup_second(), Some(1));

        assert!(model.set_row(2, 1));
        assert!(first.is_dirty());
        assert!(!second.is_dirty());
        assert_eq!(lookup_first(), None);
        assert_eq!(keys.key_at(&model, 0, |key| *key), None);

        assert!(model.set_row(1, 1));
        assert!(!first.is_dirty());
        assert!(second.is_dirty());
        assert_eq!(lookup_second(), None);

        assert!(model.set_row(2, 2));
        assert!(!first.is_dirty());
        assert!(second.is_dirty());
        assert_eq!(lookup_second(), Some(2));

        assert_eq!(model.remove(1), Some(1));
        assert_eq!(lookup_first(), Some(0));
        assert_eq!(lookup_second(), Some(1));
        assert_eq!(keys.key_at(&model, 0, |key| *key), Some(1));
    }

    #[test]
    fn switching_models_releases_the_old_observer_and_invalidates_its_readers() {
        let first_model = VecModel::from(vec![0_i64, 1, 2]);
        let second_model = VecModel::from(vec![2_i64, 1, 0]);
        let keys = ListSelectionKeys::default();
        let selected = Property::default();
        let first_reads = Rc::new(Cell::new(0));
        let project = {
            let first_reads = first_reads.clone();
            move |key: &i64| {
                first_reads.set(first_reads.get() + 1);
                *key
            }
        };
        let read = || selected.get(|| keys.find(&first_model, 0, project.clone()));

        assert_eq!(read(), Some(0));
        assert_eq!(keys.find(&second_model, 0, |key| *key), Some(2));
        assert!(selected.is_dirty());
        assert!(first_model.set_row(0, 3));
        assert_eq!(first_reads.get(), 3);

        assert_eq!(read(), None);
        assert!(first_model.set_row(1, 0));
        assert!(selected.is_dirty());
        assert_eq!(read(), Some(1));
    }
}
