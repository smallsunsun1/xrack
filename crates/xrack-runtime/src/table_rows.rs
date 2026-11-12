use crate::{
    ChangeSignal, ListModel, Property, RowKey, RowLifetime, TableSort, VecModel, list::MAX_ROWS,
};
use std::{cell::RefCell, cmp::Ordering, ops::Range, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct TableEditPosition {
    pub row: usize,
    pub column: usize,
    pub key: RowKey,
}

/// Find the next writable cell by projecting model values without creating row delegates.
#[doc(hidden)]
pub fn next_table_edit_cell<T: Clone + PartialEq>(
    rows: &TableRows<VecModel<T>>,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    mut editable: impl FnMut(usize, usize, &T) -> bool,
) -> Option<TableEditPosition> {
    try_next_table_edit_cell(rows, origin, columns, backward, |row, column, record| {
        Ok::<_, std::convert::Infallible>(editable(row, column, record))
    })
    .unwrap_or_else(|error| match error {})
}

/// Find the next writable cell, stopping at the first failed projection.
#[doc(hidden)]
pub fn try_next_table_edit_cell<T: Clone + PartialEq, E>(
    rows: &TableRows<VecModel<T>>,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    mut editable: impl FnMut(usize, usize, &T) -> Result<bool, E>,
) -> Result<Option<TableEditPosition>, E> {
    let count = rows.row_count();
    let origin_row = if origin.row < count && rows.row_key(origin.row) == origin.key {
        Some(origin.row)
    } else {
        rows.index_of_key(origin.key)
    };
    let Some(origin_column) = columns.iter().position(|column| *column == origin.column) else {
        return Ok(None);
    };
    let mut row = if let Some(row) = origin_row {
        row
    } else {
        // A successful edit may filter the record out of the view. Resume at its
        // former position if the same record is still in the source model.
        if rows.source().index_of_key(origin.key).is_none() {
            return Ok(None);
        }
        if backward {
            let Some(previous) = origin.row.min(count).checked_sub(1) else {
                return Ok(None);
            };
            previous
        } else {
            origin.row
        }
    };
    while row < count {
        let Some(source) = rows.positions().source_index(row) else {
            return Ok(None);
        };
        let mut slots = match (Some(row) == origin_row, backward) {
            (true, false) => origin_column + 1..columns.len(),
            (true, true) => 0..origin_column,
            (false, _) => 0..columns.len(),
        };
        let Some(column) = rows
            .source()
            .project_row(source, |record| {
                let mut next = || {
                    if backward {
                        slots.next_back()
                    } else {
                        slots.next()
                    }
                };
                while let Some(slot) = next() {
                    if editable(source, columns[slot], record)? {
                        return Ok(Some(slot));
                    }
                }
                Ok(None)
            })
            .transpose()?
        else {
            return Ok(None);
        };
        if let Some(column) = column {
            return Ok(rows.source().row_key(source).map(|key| TableEditPosition {
                row,
                column: columns[column],
                key,
            }));
        }
        row = if backward {
            let Some(previous) = row.checked_sub(1) else {
                return Ok(None);
            };
            previous
        } else {
            row + 1
        };
    }
    Ok(None)
}

struct Permutation {
    source: Vec<usize>,
    visual: Vec<usize>,
}

impl Permutation {
    fn new(source: Vec<usize>, count: usize) -> Self {
        let mut visual = vec![usize::MAX; count];
        for (index, &row) in source.iter().enumerate() {
            visual[row] = index;
        }
        Self { source, visual }
    }
}

/// Immutable mapping for one revision of a table's visible row order.
#[doc(hidden)]
#[derive(Clone)]
pub struct TableRowPositions {
    stamp: (u64, u64),
    source_stamp: (u64, u64),
    count: usize,
    projected: bool,
    order: Option<Rc<Permutation>>,
}

impl TableRowPositions {
    pub fn row_count(&self) -> usize {
        self.count
    }

    pub fn stamp(&self) -> (u64, u64) {
        self.stamp
    }

    pub fn reordered(&self) -> bool {
        self.order.is_some()
    }

    pub fn projected(&self) -> bool {
        self.projected
    }

    pub fn source_index(&self, visual: usize) -> Option<usize> {
        (visual < self.count).then(|| {
            self.order
                .as_ref()
                .map_or(visual, |order| order.source[visual])
        })
    }

    pub fn visual_index(&self, source: usize) -> Option<usize> {
        match &self.order {
            Some(order) => order
                .visual
                .get(source)
                .copied()
                .filter(|&row| row != usize::MAX),
            None => (source < self.count).then_some(source),
        }
    }
}

#[derive(Default)]
struct State {
    source: Option<(u64, u64)>,
    revision: u64,
    manual: Option<Rc<Permutation>>,
    projected: Option<Rc<Permutation>>,
}

impl State {
    fn reconcile(&mut self, source: (u64, u64)) {
        if self.source != Some(source) {
            self.source = Some(source);
            self.revision += 1;
            self.manual = None;
            self.projected = None;
        }
    }

    fn order(&self) -> Option<&Rc<Permutation>> {
        self.projected.as_ref().or(self.manual.as_ref())
    }

    fn use_projection(&mut self, projection: Option<ProjectedOrder>) {
        let previous = self.order().cloned();
        if projection
            .as_ref()
            .is_some_and(|projection| projection.sorted)
        {
            self.manual = None;
        }
        self.projected = projection.map(|projection| projection.order);
        let same = match (previous.as_ref(), self.order()) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b) || a.source == b.source,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.revision += 1;
        }
    }

    fn positions(&self, stamp: (u64, u64), count: usize) -> TableRowPositions {
        TableRowPositions {
            stamp: (stamp.0, self.revision),
            source_stamp: stamp,
            count: self.order().map_or(count, |order| order.source.len()),
            projected: self.projected.is_some(),
            order: self.order().cloned(),
        }
    }
}

#[derive(Clone)]
struct ProjectedOrder {
    order: Rc<Permutation>,
    sorted: bool,
}

struct ProjectionObservation {
    model: u64,
    column: Option<usize>,
    query: Rc<str>,
    changed: Rc<ChangeSignal>,
}

/// Per-table row order. Source structural changes restore source order.
#[doc(hidden)]
#[derive(Default)]
pub struct TableRowOrder {
    state: RefCell<State>,
    changed: ChangeSignal,
    projection: Property<Option<ProjectedOrder>>,
    projection_source: Property<u64>,
    observation: RefCell<Option<ProjectionObservation>>,
}

impl TableRowOrder {
    /// Keep the predicate fixed for this order's lifetime. It may capture a field path.
    pub fn visible_rows<T: Clone + PartialEq + 'static>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        visible: impl Fn(&T) -> bool + Clone + 'static,
    ) -> TableRows<VecModel<T>> {
        self.project_rows_by(
            source,
            || None,
            |_, _, _| Ordering::Equal,
            None::<fn(&T) -> &str>,
            String::new,
            Some(visible),
        )
    }

    pub fn positions(&self, model: &impl ListModel) -> TableRowPositions {
        self.changed.track();
        let stamp = model.structure_stamp();
        let mut state = self.state.borrow_mut();
        state.reconcile(stamp);
        state.positions(stamp, model.row_count())
    }

    pub fn rows<M: ListModel>(&self, source: Rc<M>) -> TableRows<M> {
        TableRows {
            positions: self.positions(&*source),
            source,
        }
    }

    pub fn sorted_rows<T: Clone + PartialEq + 'static>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        sort: impl FnOnce() -> Option<TableSort>,
        compare: fn(usize, &T, &T) -> Ordering,
    ) -> TableRows<VecModel<T>> {
        self.projected_rows(source, sort, compare, None, String::new)
    }

    pub fn projected_rows<T: Clone + PartialEq + 'static>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        sort: impl FnOnce() -> Option<TableSort>,
        compare: fn(usize, &T, &T) -> Ordering,
        field: Option<fn(&T) -> &str>,
        query: impl FnOnce() -> String,
    ) -> TableRows<VecModel<T>> {
        self.projected_rows_with_visibility(source, sort, compare, field, query, None)
    }

    pub fn projected_rows_with_visibility<T: Clone + PartialEq + 'static>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        sort: impl FnOnce() -> Option<TableSort>,
        compare: fn(usize, &T, &T) -> Ordering,
        field: Option<fn(&T) -> &str>,
        query: impl FnOnce() -> String,
        visible: Option<fn(&T) -> bool>,
    ) -> TableRows<VecModel<T>> {
        self.project_rows_by(source, sort, compare, field, query, visible)
    }

    /// Project the source with immutable field selectors that can capture schema paths.
    #[doc(hidden)]
    pub fn project_rows_by<T: Clone + PartialEq + 'static>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        sort: impl FnOnce() -> Option<TableSort>,
        compare: impl Fn(usize, &T, &T) -> Ordering + Clone + 'static,
        field: Option<impl Fn(&T) -> &str + Clone + 'static>,
        query: impl FnOnce() -> String,
        visible: Option<impl Fn(&T) -> bool + Clone + 'static>,
    ) -> TableRows<VecModel<T>> {
        self.changed.track();
        let source = source();
        let stamp = source.structure_stamp();
        self.projection_source.set_if_changed(stamp.0);
        let projection = self.projection.get(|| {
            self.projection_source.get(|| stamp.0);
            let sort = sort();
            let query: Rc<str> = field
                .as_ref()
                .map_or_else(String::new, |_| query().trim().to_lowercase())
                .into();
            if sort.is_none() && query.is_empty() && visible.is_none() {
                self.observation.borrow_mut().take();
                return None;
            }
            let stamp = source.structure_stamp();
            let column = sort.map(|sort| sort.column);
            let predicate = visible.clone();
            let filter = field.clone();
            let matches = move |row: &T, query: &str| {
                predicate.as_ref().is_none_or(|visible| visible(row))
                    && (query.is_empty()
                        || filter
                            .as_ref()
                            .is_some_and(|field| field(row).to_lowercase().contains(query)))
            };
            {
                let mut observation = self.observation.borrow_mut();
                if observation.as_ref().is_none_or(|observed| {
                    observed.model != stamp.0
                        || observed.column != column
                        || observed.query != query
                }) {
                    let needle = query.clone();
                    let matches = matches.clone();
                    let compare = compare.clone();
                    *observation = Some(ProjectionObservation {
                        model: stamp.0,
                        column,
                        query: query.clone(),
                        changed: source.observe_projection_changes(
                            MAX_ROWS,
                            move |_, before, after| {
                                let field_changed = visible
                                    .as_ref()
                                    .is_some_and(|visible| visible(before) != visible(after))
                                    || !needle.is_empty()
                                        && field
                                            .as_ref()
                                            .is_some_and(|field| field(before) != field(after));
                                let order_changed = column.is_some_and(|column| {
                                    compare(column, before, after) != Ordering::Equal
                                });
                                if !field_changed && !order_changed {
                                    return false;
                                }
                                let visible = matches(after, &needle);
                                field_changed && matches(before, &needle) != visible
                                    || visible && order_changed
                            },
                        ),
                    });
                }
                observation.as_ref().unwrap().changed.track();
            }
            let manual = {
                let state = self.state.borrow();
                if sort.is_none() && state.source == Some(stamp) {
                    state.manual.clone()
                } else {
                    None
                }
            };
            let sorter = sort.map(|sort| {
                move |left: &T, right: &T| {
                    let order = compare(sort.column, left, right);
                    if sort.descending {
                        order.reverse()
                    } else {
                        order
                    }
                }
            });
            let count = source.row_count();
            let order = source.projected_indices_untracked(
                count,
                manual.as_ref().map(|order| order.source.as_slice()),
                |row| matches(row, &query),
                sorter,
            );
            if sort.is_none() && query.is_empty() && order.len() == count {
                return None;
            }
            Some(ProjectedOrder {
                order: Rc::new(Permutation::new(order, count)),
                sorted: sort.is_some(),
            })
        });
        let mut state = self.state.borrow_mut();
        state.reconcile(stamp);
        state.use_projection(projection);
        let positions = state.positions(stamp, source.row_count());
        TableRows { source, positions }
    }

    pub fn move_row(&self, model: &impl ListModel, from: usize, to: usize) -> Option<usize> {
        let count = model.row_count();
        if from == to || from >= count || to >= count {
            return None;
        }
        let mut state = self.state.borrow_mut();
        state.reconcile(model.structure_stamp());
        if state.projected.is_some() {
            return None;
        }
        let mut source = state
            .manual
            .as_ref()
            .map_or_else(|| (0..count).collect(), |order| order.source.clone());
        let moved = source[from];
        if from < to {
            source[from..=to].rotate_left(1);
        } else {
            source[to..=from].rotate_right(1);
        }
        state.manual = Some(Rc::new(Permutation::new(source, count)));
        state.revision += 1;
        drop(state);
        self.changed.notify();
        Some(moved)
    }
}

/// The list interface in visual order, retaining source keys and lifetimes.
#[doc(hidden)]
#[derive(Clone)]
pub struct TableRows<M> {
    source: Rc<M>,
    positions: TableRowPositions,
}

impl<M> TableRows<M> {
    pub fn source(&self) -> &M {
        &self.source
    }

    pub fn positions(&self) -> &TableRowPositions {
        &self.positions
    }
}

impl<M: ListModel> TableRows<M> {
    /// Apply positions to the same source exposed through a different item type.
    #[doc(hidden)]
    pub fn from_positions(source: Rc<M>, positions: TableRowPositions) -> Self {
        debug_assert_eq!(source.structure_stamp(), positions.source_stamp);
        Self { source, positions }
    }
}

impl<M: ListModel> ListModel for TableRows<M> {
    type Item = M::Item;

    fn row_count(&self) -> usize {
        if self.source.structure_stamp() == self.positions.source_stamp {
            self.positions.count
        } else {
            0
        }
    }

    fn item(&self, row: usize) -> Self::Item {
        self.source
            .item(self.positions.source_index(row).expect("row in range"))
    }

    fn row_key(&self, row: usize) -> RowKey {
        self.source
            .row_key(self.positions.source_index(row).expect("row in range"))
    }

    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        self.positions.visual_index(self.source.index_of_key(key)?)
    }

    fn index_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        if self.source.structure_stamp() != self.positions.source_stamp {
            return None;
        }
        self.positions
            .visual_index(self.source.index_of_source_key(key, source_index)?)
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        self.source.row_lifetime(key)
    }

    fn structure_stamp(&self) -> (u64, u64) {
        self.positions.stamp()
    }

    fn source_structure_stamp(&self) -> (u64, u64) {
        self.source.source_structure_stamp()
    }

    fn visit_source_row_keys(&self, visit: impl FnMut(RowKey)) {
        self.source.visit_source_row_keys(visit);
    }

    fn visit_row_keys(&self, range: Range<usize>, mut visit: impl FnMut(RowKey)) {
        if let Some(order) = &self.positions.order {
            for &index in &order.source[range] {
                visit(self.source.row_key(index));
            }
        } else {
            self.source.visit_row_keys(range, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_visibility_composes_with_sort_and_text_without_rescanning_unchanged_membership() {
        thread_local! {
            static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        type Record = (i64, &'static str, bool);
        fn shown(record: &Record) -> bool {
            READS.with(|reads| reads.set(reads.get() + 1));
            record.2
        }
        let model = Rc::new(VecModel::from(vec![
            (30, "keep first", true),
            (10, "keep hidden", false),
            (20, "keep third", true),
            (0, "other", true),
        ]));
        let key = model.row_key(1).unwrap();
        let order = TableRowOrder::default();
        let view = || {
            order.projected_rows_with_visibility(
                || model.clone(),
                || {
                    Some(TableSort {
                        column: 0,
                        descending: false,
                    })
                },
                |_, left, right| left.0.cmp(&right.0),
                Some(|row: &Record| row.1),
                || "keep".into(),
                Some(shown),
            )
        };
        let first = view();
        let stamp = first.structure_stamp();
        assert_eq!(first.row_count(), 2);
        assert_eq!(first.item(0).0, 20);
        assert_eq!(first.index_of_key(key), None);
        assert_eq!(model.row_index(key), Some(1));
        let reads = READS.with(std::cell::Cell::get);
        model.set_row(1, (5, "keep hidden", false));
        assert_eq!(view().structure_stamp(), stamp);
        let after_edit = READS.with(std::cell::Cell::get);
        assert_eq!(after_edit - reads, 3);
        model.set_row(1, (5, "keep hidden", true));
        let restored = view();
        assert_ne!(restored.structure_stamp(), stamp);
        assert_eq!(restored.row_count(), 3);
        assert_eq!(restored.row_key(0), key);
    }

    #[test]
    fn row_visibility_restores_the_manual_order_when_every_record_is_shown_again() {
        let model = Rc::new(VecModel::from(vec![("a", true), ("b", true), ("c", true)]));
        let order = TableRowOrder::default();
        let view = || {
            order.projected_rows_with_visibility(
                || model.clone(),
                || None,
                |_, _, _| Ordering::Equal,
                None,
                String::new,
                Some(|row: &(&str, bool)| row.1),
            )
        };
        assert!(!view().positions().projected());
        assert_eq!(order.move_row(&*model, 2, 0), Some(2));
        model.set_row(0, ("a", false));
        let hidden = view();
        assert!(hidden.positions().projected());
        assert_eq!((hidden.item(0).0, hidden.item(1).0), ("c", "b"));
        assert!(order.move_row(&*model, 1, 0).is_none());
        model.set_row(0, ("a", true));
        let restored = view();
        assert!(!restored.positions().projected());
        assert_eq!(
            (restored.item(0).0, restored.item(1).0, restored.item(2).0),
            ("c", "a", "b")
        );
    }

    #[test]
    fn sorted_projection_tracks_its_column_and_retains_source_identity() {
        thread_local! {
            static COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        let source = Property::default();
        let model = Rc::new(VecModel::from(vec![(30, "a"), (10, "b"), (10, "c")]));
        source.set(model.clone());
        let selection = Property::default();
        selection.set(Some(TableSort {
            column: 0,
            descending: false,
        }));
        let order = TableRowOrder::default();
        let read = || {
            order.sorted_rows(
                || source.get(|| unreachable!()),
                || selection.get(|| None),
                |_, left: &(i32, &'static str), right| {
                    COMPARISONS.with(|count| count.set(count.get() + 1));
                    left.0.cmp(&right.0)
                },
            )
        };
        let initial = read();
        assert_eq!(initial.positions().source_index(0), Some(1));
        assert_eq!(initial.positions().source_index(1), Some(2));
        assert_eq!(initial.row_key(0), model.row_key(1).unwrap());
        assert!(order.move_row(&*model, 0, 1).is_none());
        assert!(model.set_row(1, (10, "updated")));
        let comparisons = COMPARISONS.with(std::cell::Cell::get);
        assert_eq!(initial.structure_stamp(), read().structure_stamp());
        assert_eq!(COMPARISONS.with(std::cell::Cell::get), comparisons);
        assert!(model.set_row(0, (2, "a")));
        let changed = read();
        assert_ne!(changed.structure_stamp(), initial.structure_stamp());
        assert_eq!(changed.positions().source_index(0), Some(0));
        assert_eq!(model.row(0), Some((2, "a")));

        source.set(Rc::new(VecModel::from(vec![(100, "new"), (0, "old")])));
        assert_eq!(read().positions().source_index(0), Some(1));
        assert_eq!(read().row_count(), 2);
        selection.set(None);
        assert_eq!(read().positions().source_index(0), Some(0));
    }

    #[test]
    fn filter_projects_manual_order_and_tracks_only_membership_and_visible_sort_keys() {
        let model = Rc::new(VecModel::from(vec![
            ("Alpha", 30, false),
            ("Beta", 20, false),
            ("ALPHA 2", 10, false),
        ]));
        let order = TableRowOrder::default();
        assert_eq!(order.move_row(&*model, 2, 0), Some(2));
        let query = Property::default();
        query.set(String::from(" alpha "));
        let sort = Property::default();
        let cached = Property::default();
        let read = || {
            cached.get(|| {
                let rows = order.projected_rows(
                    || model.clone(),
                    || sort.get(|| None),
                    |_, a, b| a.1.cmp(&b.1),
                    Some(|row: &(&'static str, i64, bool)| row.0),
                    || query.get(String::new),
                );
                (0..rows.row_count())
                    .map(|index| rows.positions().source_index(index).unwrap())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(read(), [2, 0]);
        assert!(order.move_row(&*model, 0, 1).is_none());
        assert!(model.set_row(0, ("Alpha", 30, true)));
        assert!(!cached.is_dirty());
        sort.set(Some(TableSort {
            column: 0,
            descending: true,
        }));
        assert_eq!(read(), [0, 2]);
        assert!(model.set_row(1, ("Beta", 50, false)));
        assert!(!cached.is_dirty());
        assert!(model.set_row(1, ("Alpha new", 50, false)));
        assert!(cached.is_dirty());
        assert_eq!(read(), [1, 0, 2]);
        assert!(model.set_row(1, ("alpha renamed", 50, false)));
        assert!(!cached.is_dirty());
        assert!(model.set_row(0, ("Removed from view", 30, true)));
        assert_eq!(read(), [1, 2]);
        assert_eq!(model.len(), 3);
    }
}
