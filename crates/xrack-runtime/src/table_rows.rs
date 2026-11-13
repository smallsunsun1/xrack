use crate::{
    ChangeSignal, InputRoute, ListModel, Node, Property, RowKey, RowLifetime, TableSort, VecModel,
    list::MAX_ROWS,
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
    let stamp = rows.source().contents_stamp();
    try_find_table_edit_cell(
        rows,
        origin,
        columns,
        backward,
        false,
        |row, column, _, record| editable(row, column, record),
        || rows.source().contents_stamp() == stamp,
    )
}

#[doc(hidden)]
pub fn next_table_edit_cell_in_view<M: ListModel>(
    view: &Node,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    rows: impl FnOnce() -> TableRows<M>,
    editable: impl FnMut(usize, usize, RowKey, &M::Item) -> bool,
) -> Option<TableEditPosition> {
    next_table_edit_cell_in_view_if(view, origin, columns, backward, || Some(rows()), editable)
}

#[doc(hidden)]
pub fn next_table_edit_cell_in_view_if<M: ListModel>(
    view: &Node,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    rows: impl FnOnce() -> Option<TableRows<M>>,
    mut editable: impl FnMut(usize, usize, RowKey, &M::Item) -> bool,
) -> Option<TableEditPosition> {
    try_next_table_edit_cell_in_view(
        view,
        origin,
        columns,
        backward,
        || Ok::<_, std::convert::Infallible>(rows()),
        |row, column, key, record| Ok(editable(row, column, key, record)),
    )
    .unwrap_or_else(|error| match error {})
}

/// Search the scene's table without keeping a model borrow across permission callbacks.
#[doc(hidden)]
pub fn try_next_table_edit_cell_in_view<M: ListModel, E>(
    view: &Node,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    rows: impl FnOnce() -> Result<Option<TableRows<M>>, E>,
    editable: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<bool, E>,
) -> Result<Option<TableEditPosition>, E> {
    try_next_table_edit_cell_while_current(
        view,
        crate::TableEditNavigation {
            origin,
            columns,
            backward,
        },
        || true,
        rows,
        editable,
    )
}

#[doc(hidden)]
pub fn next_table_edit_cell_while_current<M: ListModel>(
    view: &Node,
    navigation: crate::TableEditNavigation<'_>,
    current: impl Fn() -> bool,
    rows: impl FnOnce() -> Option<TableRows<M>>,
    mut editable: impl FnMut(usize, usize, RowKey, &M::Item) -> bool,
) -> Option<TableEditPosition> {
    try_next_table_edit_cell_while_current(
        view,
        navigation,
        current,
        || Ok::<_, std::convert::Infallible>(rows()),
        |row, column, key, record| Ok(editable(row, column, key, record)),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_next_table_edit_cell_while_current<M: ListModel, E>(
    view: &Node,
    navigation: crate::TableEditNavigation<'_>,
    available: impl Fn() -> bool,
    rows: impl FnOnce() -> Result<Option<TableRows<M>>, E>,
    editable: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<bool, E>,
) -> Result<Option<TableEditPosition>, E> {
    if !view.boolean("edit-on-tab", false) {
        return Ok(None);
    }
    with_table_rows_in_view(view, available, rows, |rows, current| {
        try_find_table_edit_cell(
            rows,
            navigation.origin,
            navigation.columns,
            navigation.backward,
            view.boolean("__standard-table", false),
            editable,
            current,
        )
    })
}

pub(crate) fn with_table_rows_in_view<M: ListModel, R, E>(
    view: &Node,
    available: impl Fn() -> bool,
    rows: impl FnOnce() -> Result<Option<TableRows<M>>, E>,
    project: impl FnOnce(&TableRows<M>, &dyn Fn() -> bool) -> Result<Option<R>, E>,
) -> Result<Option<R>, E> {
    if !available() {
        return Ok(None);
    }
    crate::property::watch_reads(|reads| {
        let Some(rows) = rows()? else {
            return Ok(None);
        };
        if view.standard_list_stamp != Some(rows.positions().stamp()) {
            return Ok(None);
        }
        let source = rows.source();
        let stamp = source.contents_stamp();
        let current = || available() && !reads.changed() && source.contents_stamp() == stamp;
        if !current() {
            return Ok(None);
        }
        project(&rows, &current)
    })
}

fn try_find_table_edit_cell<M: ListModel, E>(
    rows: &TableRows<M>,
    origin: TableEditPosition,
    columns: &[usize],
    backward: bool,
    shared_permissions: bool,
    mut editable: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<bool, E>,
    current: impl Fn() -> bool,
) -> Result<Option<TableEditPosition>, E> {
    let model = rows.source();
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
        if model.index_of_key(origin.key).is_none() {
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
        let at_origin = Some(row) == origin_row;
        let mut slots = match (at_origin, backward) {
            (true, false) => origin_column + 1..columns.len(),
            (true, true) => 0..origin_column,
            (false, _) => 0..columns.len(),
        };
        if !slots.is_empty() {
            let key = model.row_key(source);
            let record = model.item(source);
            let mut next = || {
                if backward {
                    slots.next_back()
                } else {
                    slots.next()
                }
            };
            while let Some(slot) = next() {
                let column = columns[slot];
                let allowed = editable(source, column, key, &record)?;
                if !current() {
                    return Ok(None);
                }
                if allowed {
                    return Ok(Some(TableEditPosition { row, column, key }));
                }
            }
        }
        // Standard table column permissions are independent of the row.
        if shared_permissions && !at_origin {
            return Ok(None);
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

    /// Resolve a visual index from a scene using this exact row order.
    pub fn event_source_index(&self, stamp: Option<(u64, u64)>, visual: usize) -> Option<usize> {
        if stamp != Some(self.stamp) {
            return None;
        }
        self.source_index(visual)
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

#[derive(Clone, Default)]
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
        self.try_project_rows_by(
            source,
            || Ok::<_, std::convert::Infallible>(sort()),
            compare,
            field,
            || Ok(query()),
            visible,
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Cache a projection only after its sort and filter bindings succeed.
    #[doc(hidden)]
    pub fn try_project_rows_by<T: Clone + PartialEq + 'static, E>(
        &self,
        source: impl FnOnce() -> Rc<VecModel<T>>,
        sort: impl FnOnce() -> Result<Option<TableSort>, E>,
        compare: impl Fn(usize, &T, &T) -> Ordering + Clone + 'static,
        field: Option<impl Fn(&T) -> &str + Clone + 'static>,
        query: impl FnOnce() -> Result<String, E>,
        visible: Option<impl Fn(&T) -> bool + Clone + 'static>,
    ) -> Result<TableRows<VecModel<T>>, E> {
        self.changed.track();
        let source = source();
        let stamp = source.structure_stamp();
        self.projection_source.set_if_changed(stamp.0);
        let projection = self.projection.try_get(|| {
            self.projection_source.get(|| stamp.0);
            let sort = sort()?;
            let query: Rc<str> = if field.is_some() {
                query()?.trim().to_lowercase().into()
            } else {
                Rc::from("")
            };
            if sort.is_none() && query.is_empty() && visible.is_none() {
                self.observation.borrow_mut().take();
                return Ok(None);
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
                return Ok(None);
            }
            Ok(Some(ProjectedOrder {
                order: Rc::new(Permutation::new(order, count)),
                sorted: sort.is_some(),
            }))
        })?;
        let mut state = self.state.borrow_mut();
        state.reconcile(stamp);
        state.use_projection(projection);
        let positions = state.positions(stamp, source.row_count());
        Ok(TableRows { source, positions })
    }

    /// Move a row using one source snapshot and keep the cursor on its source row.
    #[doc(hidden)]
    pub fn try_move_visible_row<M: ListModel, E>(
        &self,
        from: i64,
        to: i64,
        input: Option<InputRoute<'_>>,
        rows: impl FnOnce() -> Result<TableRows<M>, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<(usize, Option<usize>)>, E> {
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
            return Ok(None);
        };
        let prepared = crate::property::watch_reads(|reads| {
            let rows = rows()?;
            let current = usize::try_from(current()?).ok();
            let cursor = current.and_then(|index| rows.positions.source_index(index));
            let valid = !rows.positions.projected()
                && rows.source().structure_stamp() == rows.positions.source_stamp
                && !reads.changed()
                && input.is_none_or(|input| input.is_current());
            Ok(valid.then_some((rows, current, cursor)))
        })?;
        let Some((rows, current, cursor)) = prepared else {
            return Ok(None);
        };
        let source = rows.source();
        let Some(moved) = self.move_row(source, from, to) else {
            return Ok(None);
        };
        let next = cursor
            .and_then(|row| self.positions(source).visual_index(row))
            .filter(|index| Some(*index) != current);
        Ok(Some((moved, next)))
    }

    #[doc(hidden)]
    pub fn move_visible_row<M: ListModel>(
        &self,
        from: i64,
        to: i64,
        input: Option<InputRoute<'_>>,
        rows: impl FnOnce() -> TableRows<M>,
        current: impl FnOnce() -> i64,
    ) -> Option<(usize, Option<usize>)> {
        self.try_move_visible_row(
            from,
            to,
            input,
            || Ok::<_, std::convert::Infallible>(rows()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
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

    /// Commit a sort request and resolve the old cursor in the new projection.
    /// The outer option indicates a sort change. The inner option is a cursor update.
    #[doc(hidden)]
    pub fn try_sort_rows<M: ListModel, E>(
        &self,
        sort: &Property<Option<TableSort>>,
        next: Option<TableSort>,
        input: Option<InputRoute<'_>>,
        sortable: impl FnOnce(usize) -> Result<bool, E>,
        auto_sort: impl FnOnce() -> Result<bool, E>,
        mut rows: impl FnMut() -> Result<TableRows<M>, E>,
        mut current: impl FnMut() -> Result<i64, E>,
    ) -> Result<Option<Option<usize>>, E> {
        let prepared = crate::property::watch_reads(|reads| {
            let previous = sort.get(|| None);
            if previous == next {
                return Ok(None);
            }
            if let Some(next) = next
                && !sortable(next.column)?
            {
                return Ok(None);
            }
            let project = auto_sort()?;
            let cursor = if project {
                let rows = rows()?;
                let index = usize::try_from(current()?).ok();
                index
                    .filter(|&index| index < rows.row_count())
                    .map(|index| rows.row_key(index))
            } else {
                None
            };
            Ok(
                (!reads.changed() && input.is_none_or(|input| input.is_current()))
                    .then_some((previous, project, cursor)),
            )
        })?;
        let Some((previous, project, cursor)) = prepared else {
            return Ok(None);
        };
        if !project {
            sort.set(next);
            return Ok(Some(None));
        }

        // Bindings can read the new sort while computing its row order. Restore
        // the old sort and order if evaluation fails before notifications.
        let mut change = SortChange {
            sort,
            previous,
            next,
            order: self,
            saved_order: Some(self.state.borrow().clone()),
        };
        sort.set(next);
        let prepared = crate::property::watch_reads(|reads| {
            let rows = rows()?;
            let cursor = match cursor.and_then(|key| rows.index_of_key(key)) {
                Some(index) if index as i64 != current()? => Some(index),
                _ => None,
            };
            Ok((!reads.changed()).then_some(cursor))
        })?;
        let Some(cursor) = prepared else {
            return Ok(None);
        };
        if !sort.matches_current(|sort| *sort == next) {
            return Ok(None);
        }
        change.saved_order = None;
        Ok(Some(cursor))
    }

    #[doc(hidden)]
    pub fn sort_rows<M: ListModel>(
        &self,
        sort: &Property<Option<TableSort>>,
        next: Option<TableSort>,
        input: Option<InputRoute<'_>>,
        sortable: impl FnOnce(usize) -> bool,
        auto_sort: impl FnOnce() -> bool,
        mut rows: impl FnMut() -> TableRows<M>,
        mut current: impl FnMut() -> i64,
    ) -> Option<Option<usize>> {
        self.try_sort_rows(
            sort,
            next,
            input,
            |column| Ok::<_, std::convert::Infallible>(sortable(column)),
            || Ok(auto_sort()),
            || Ok(rows()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }
}

struct SortChange<'a> {
    sort: &'a Property<Option<TableSort>>,
    previous: Option<TableSort>,
    next: Option<TableSort>,
    order: &'a TableRowOrder,
    saved_order: Option<State>,
}

impl Drop for SortChange<'_> {
    fn drop(&mut self) {
        let Some(mut saved_order) = self.saved_order.take() else {
            return;
        };
        if self.sort.matches_current(|sort| *sort == self.next) {
            self.sort.set(self.previous);
            let mut state = self.order.state.borrow_mut();
            let changed = state.revision != saved_order.revision;
            if changed {
                // A snapshot taken during evaluation must never match the restored order.
                saved_order.revision = state.revision + 1;
            }
            *state = saved_order;
            drop(state);
            if changed {
                self.order.changed.notify();
            }
        }
    }
}

/// The list interface in visual order, retaining source keys and lifetimes.
#[doc(hidden)]
#[derive(Clone)]
pub struct TableRows<M> {
    source: Rc<M>,
    positions: TableRowPositions,
}

/// The live source row resolved from the scene's table cell.
#[doc(hidden)]
#[derive(Clone)]
pub struct TableCellEditTarget<M> {
    pub rows: TableRows<M>,
    pub index: usize,
    pub key: RowKey,
}

impl<M> TableCellEditTarget<M> {
    pub(crate) fn edit_key(&self) -> ((u64, u64), usize, RowKey) {
        (self.rows.positions().stamp(), self.index, self.key)
    }
}

#[doc(hidden)]
pub fn table_cell_edit_target<M: ListModel + Clone, A>(
    view: &Node,
    slot: usize,
    rows: impl Fn() -> TableRows<M>,
    prepare: impl FnOnce() -> Option<A>,
) -> Option<(TableCellEditTarget<M>, A)> {
    table_cell_edit_target_if(view, slot, || Some(rows()), prepare)
}

#[doc(hidden)]
pub fn table_cell_edit_target_if<M: ListModel + Clone, A>(
    view: &Node,
    slot: usize,
    rows: impl Fn() -> Option<TableRows<M>>,
    prepare: impl FnOnce() -> Option<A>,
) -> Option<(TableCellEditTarget<M>, A)> {
    try_table_cell_edit_target(
        view,
        slot,
        || Ok::<_, std::convert::Infallible>(rows()),
        || Ok(prepare()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Resolve the displayed row before calling user code and recheck it if the model changes.
#[doc(hidden)]
pub fn try_table_cell_edit_target<M: ListModel + Clone, A, E>(
    view: &Node,
    slot: usize,
    rows: impl Fn() -> Result<Option<TableRows<M>>, E>,
    prepare: impl FnOnce() -> Result<Option<A>, E>,
) -> Result<Option<(TableCellEditTarget<M>, A)>, E> {
    let Some(stamp) = view.standard_list_stamp else {
        return Ok(None);
    };
    let Some(visual) = view.list.and_then(|list| list.first_row.checked_add(slot)) else {
        return Ok(None);
    };
    let Some(key) = view.child_at(slot).and_then(|row| row.row_key) else {
        return Ok(None);
    };

    try_prepare_table_edit(
        || {
            let Some(rows) = rows()? else {
                return Ok(None);
            };
            let positions = rows.positions();
            let Some(index) = positions.event_source_index(Some(stamp), visual) else {
                return Ok(None);
            };
            let source = rows.source();
            if index >= source.row_count() || source.row_key(index) != key {
                return Ok(None);
            }
            Ok(Some(TableCellEditTarget { rows, index, key }))
        },
        prepare,
    )
}

pub(crate) fn try_prepare_table_edit<M: Clone, A, E>(
    resolve: impl Fn() -> Result<Option<TableCellEditTarget<M>>, E>,
    prepare: impl FnOnce() -> Result<Option<A>, E>,
) -> Result<Option<(TableCellEditTarget<M>, A)>, E> {
    try_prepare_table_write(resolve, prepare, TableCellEditTarget::edit_key)
}

/// Resolve the model represented by a scene before and after checking history availability.
#[doc(hidden)]
pub fn try_history_replay_source<M: ListModel, E>(
    model: u64,
    source: impl Fn() -> Result<Option<Rc<M>>, E>,
    enabled: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<Rc<M>>, E> {
    try_prepare_history_replay(Some(model), source, enabled)
}

/// Keep a replay on the same model while its availability binding is evaluated.
#[doc(hidden)]
pub fn try_prepare_history_replay<M: ListModel, E>(
    displayed_model: Option<u64>,
    source: impl Fn() -> Result<Option<Rc<M>>, E>,
    enabled: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<Rc<M>>, E> {
    let target = try_prepare_table_write(
        || {
            Ok(source()?.filter(|source| {
                displayed_model.is_none_or(|model| source.structure_stamp().0 == model)
            }))
        },
        || Ok(enabled()?.then_some(())),
        |source| source.structure_stamp().0,
    )?;
    Ok(target.map(|(source, ())| source))
}

pub(crate) fn try_prepare_table_write<T: Clone, A, E, K: PartialEq>(
    resolve: impl Fn() -> Result<Option<T>, E>,
    prepare: impl FnOnce() -> Result<Option<A>, E>,
    key: impl Fn(&T) -> K,
) -> Result<Option<(T, A)>, E> {
    let target = Property::default();
    let read = || target.try_get(&resolve);
    let Some(before) = read()?.as_ref().map(&key) else {
        return Ok(None);
    };
    let Some(prepared) = prepare()? else {
        return Ok(None);
    };
    Ok(read()?
        .filter(|after| key(after) == before)
        .map(|target| (target, prepared)))
}

impl<M> TableRows<M> {
    pub fn source(&self) -> &M {
        &self.source
    }

    #[doc(hidden)]
    pub fn source_model(&self) -> &Rc<M> {
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
    fn failed_cursor_read_restores_the_manual_order_after_projection() {
        let model = Rc::new(VecModel::from(vec![1, 2, 3]));
        let order = TableRowOrder::default();
        let sort = Property::default();
        let next = Some(TableSort {
            column: 0,
            descending: true,
        });
        let rows = || order.sorted_rows(|| model.clone(), || sort.get(|| None), |_, a, b| a.cmp(b));
        assert_eq!(order.move_row(&*model, 1, 0), Some(1));
        let before = rows();
        let key = before.row_key(0);
        let tentative = std::cell::Cell::new(None);

        assert_eq!(
            order.try_sort_rows(
                &sort,
                next,
                None,
                |_| Ok(true),
                || Ok(true),
                || {
                    let rows = rows();
                    if sort.get(|| None) == next {
                        tentative.set(Some(rows.structure_stamp()));
                        assert_eq!(rows.index_of_key(key), Some(1));
                    }
                    Ok(rows)
                },
                || {
                    if sort.get(|| None) == next {
                        Err("cursor failed")
                    } else {
                        Ok(0)
                    }
                },
            ),
            Err("cursor failed")
        );

        assert_eq!(sort.get(|| None), None);
        let restored = rows();
        assert_eq!(restored.index_of_key(key), Some(0));
        assert_ne!(Some(restored.structure_stamp()), tentative.get());
        assert_eq!(
            order.sort_rows(&sort, next, None, |_| true, || true, rows, || 0),
            Some(Some(1))
        );
        assert_eq!(rows().index_of_key(key), Some(1));
    }

    #[test]
    fn failed_sort_binding_keeps_the_last_projection_and_skips_the_filter() {
        let model = Rc::new(VecModel::from(vec!["one".to_owned(), "two".to_owned()]));
        let order = TableRowOrder::default();
        let sort = Property::default();
        let filter_reads = std::cell::Cell::new(0);
        let read = || {
            order.try_project_rows_by(
                || model.clone(),
                || sort.get(|| Ok(None)),
                |_, left, right| left.cmp(right),
                Some(String::as_str),
                || {
                    filter_reads.set(filter_reads.get() + 1);
                    Ok("two".into())
                },
                None::<fn(&String) -> bool>,
            )
        };

        let before = read().unwrap();
        assert_eq!(before.positions().source_index(0), Some(1));
        sort.set(Err("sort failed"));
        assert!(matches!(read(), Err("sort failed")));
        assert_eq!(filter_reads.get(), 1);
        assert_eq!(order.positions(&*model).stamp(), before.positions().stamp());

        sort.set(Ok(Some(TableSort {
            column: 0,
            descending: true,
        })));
        let recovered = read().unwrap();
        assert_eq!(recovered.positions().source_index(0), Some(1));
        assert_eq!(filter_reads.get(), 2);
    }

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
