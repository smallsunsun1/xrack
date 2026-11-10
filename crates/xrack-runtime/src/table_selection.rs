use crate::table_clipboard::{MAX_CLIPBOARD_BYTES, MAX_CLIPBOARD_CELLS};
use crate::{ChangeSignal, ListModel, Node, RowKey};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionTarget {
    #[default]
    Rows,
    Cells,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSelection {
    Replace,
    Toggle,
    Extend,
    AddRange,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableSelectionAction {
    Row {
        index: usize,
        operation: RowSelection,
    },
    Cell {
        row: usize,
        column: usize,
        operation: RowSelection,
    },
    Column {
        column: usize,
        operation: RowSelection,
    },
    All,
    Clear,
}

type RowKeys = Arc<BTreeSet<RowKey>>;
type ColumnCells = BTreeMap<usize, RowKeys>;

#[derive(Clone, Debug)]
enum Selected {
    Rows(RowKeys),
    Columns(Arc<BTreeSet<usize>>),
    Cells(Arc<ColumnCells>),
}

impl PartialEq for Selected {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rows(a), Self::Rows(b)) => a == b,
            (Self::Columns(a), Self::Columns(b)) => a == b,
            (Self::Cells(a), Self::Cells(b)) => {
                let mut compared = HashSet::new();
                Arc::ptr_eq(a, b)
                    || a.len() == b.len()
                        && a.iter()
                            .zip(b.iter())
                            .all(|((a_col, a_rows), (b_col, b_rows))| {
                                a_col == b_col
                                    && (!compared
                                        .insert((Arc::as_ptr(a_rows), Arc::as_ptr(b_rows)))
                                        || a_rows == b_rows)
                            })
            }
            _ => false,
        }
    }
}

impl Eq for Selected {}

impl Selected {
    fn empty(target: SelectionTarget) -> Self {
        match target {
            SelectionTarget::Rows => Self::Rows(Arc::default()),
            SelectionTarget::Cells => Self::Cells(Arc::default()),
            SelectionTarget::Columns => Self::Columns(Arc::default()),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Rows(rows) => rows.len(),
            Self::Columns(columns) => columns.len(),
            Self::Cells(cells) => cells.values().map(|rows| rows.len()).sum(),
        }
    }

    fn retain_live(&mut self, model: &impl ListModel) {
        if matches!(self, Self::Columns(_)) || self.len() == 0 {
            return;
        }
        match self {
            Self::Rows(rows) => {
                let mut live = BTreeSet::new();
                model.visit_row_keys(0..model.row_count(), |key| {
                    if rows.contains(&key) {
                        live.insert(key);
                    }
                });
                if live.len() != rows.len() {
                    *rows = Arc::new(live);
                }
            }
            Self::Cells(cells) => {
                let live = row_keys(model, 0..model.row_count());
                // A rectangular range can share a single row set across many columns.
                let mut pruned = HashMap::<_, RowKeys>::new();
                let cells = Arc::make_mut(cells);
                cells.retain(|_, rows| {
                    *rows = pruned
                        .entry(Arc::as_ptr(rows))
                        .or_insert_with(|| {
                            if rows.is_subset(&live) {
                                rows.clone()
                            } else {
                                Arc::new(rows.intersection(&live).copied().collect())
                            }
                        })
                        .clone();
                    !rows.is_empty()
                });
            }
            Self::Columns(_) => {}
        }
    }
}

fn row_keys(model: &impl ListModel, range: Range<usize>) -> BTreeSet<RowKey> {
    let mut keys = BTreeSet::new();
    model.visit_row_keys(range, |key| {
        keys.insert(key);
    });
    keys
}

fn span(a: usize, b: usize) -> Range<usize> {
    a.min(b)..a.max(b) + 1
}

/// Selection in a table snapshot, including units outside the viewport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSelectionSnapshot {
    stamp: (u64, u64),
    target: SelectionTarget,
    column_count: usize,
    order: crate::TableColumnOrder,
    multiple: bool,
    current_column: Option<usize>,
    selected: Selected,
}

impl TableSelectionSnapshot {
    #[doc(hidden)]
    pub fn with_column_order(mut self, order: crate::TableColumnOrder) -> Self {
        self.order = order;
        self
    }

    pub fn target(&self) -> SelectionTarget {
        self.target
    }

    pub fn current_column(&self) -> Option<usize> {
        self.current_column
    }

    pub fn multiple(&self) -> bool {
        self.multiple
    }

    /// Number of selected rows, cells, or columns, depending on the target.
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> &BTreeSet<RowKey> {
        static EMPTY: BTreeSet<RowKey> = BTreeSet::new();
        match &self.selected {
            Selected::Rows(rows) => rows,
            _ => &EMPTY,
        }
    }

    pub fn columns(&self) -> &BTreeSet<usize> {
        static EMPTY: BTreeSet<usize> = BTreeSet::new();
        match &self.selected {
            Selected::Columns(columns) => columns,
            _ => &EMPTY,
        }
    }

    /// Explicit cell selection in a column, without expanding whole-row or whole-column selections.
    pub fn cell_keys(&self, column: usize) -> Option<&BTreeSet<RowKey>> {
        match &self.selected {
            Selected::Cells(cells) => cells.get(&column).map(Arc::as_ref),
            _ => None,
        }
    }

    pub fn contains_cell(&self, row: RowKey, column: usize) -> bool {
        column < self.column_count
            && row.model == self.stamp.0
            && match &self.selected {
                Selected::Rows(rows) => rows.contains(&row),
                Selected::Columns(columns) => columns.contains(&column),
                Selected::Cells(cells) => {
                    cells.get(&column).is_some_and(|rows| rows.contains(&row))
                }
            }
    }

    /// Resolve selected rows in the model's current order with one model pass.
    pub fn indices(&self, model: &impl ListModel) -> Vec<usize> {
        let keys = self.keys();
        if keys.is_empty() || self.stamp.0 != model.structure_stamp().0 {
            return Vec::new();
        }
        let mut indices = Vec::with_capacity(keys.len());
        let mut index = 0;
        model.visit_row_keys(0..model.row_count(), |key| {
            if keys.contains(&key) {
                indices.push(index);
            }
            index += 1;
        });
        indices
    }
}

/// Serialize the selected model rows in the supplied visual column order.
#[doc(hidden)]
pub fn copy_table_text<M: ListModel>(
    model: &M,
    table: &Node,
    displayed_columns: &[usize],
    mut cell: impl FnMut(usize, usize, &M::Item) -> Option<String>,
) -> Option<String> {
    let count = model.row_count();
    let column_count = table.table_header.as_ref()?.columns.len();
    let mut columns: Vec<_> = displayed_columns
        .iter()
        .copied()
        .filter(|&column| column < column_count)
        .collect();
    let mut rows = Vec::new();
    let mut range = 0..count;
    let mut cells = None;
    if let Some(selection) = table.table_selection.as_deref() {
        if selection.stamp.0 != model.structure_stamp().0 {
            return None;
        }
        match &selection.selected {
            Selected::Rows(selected) => {
                if selected.len().checked_mul(columns.len())? > MAX_CLIPBOARD_CELLS {
                    return None;
                }
                let current = table
                    .int("current-row")
                    .and_then(|row| usize::try_from(row).ok());
                if let Some(row) = current.filter(|&row| {
                    selected.len() == 1 && row < count && selected.contains(&model.row_key(row))
                }) {
                    rows.push(row);
                } else {
                    rows = selection.indices(model);
                }
                range = 0..0;
            }
            Selected::Columns(selected) => columns.retain(|column| selected.contains(column)),
            Selected::Cells(selected) => {
                let start = columns
                    .iter()
                    .position(|column| selected.contains_key(column))?;
                let end = columns
                    .iter()
                    .rposition(|column| selected.contains_key(column))?
                    + 1;
                columns = columns[start..end].to_vec();
                let selected_count = columns
                    .iter()
                    .filter_map(|column| selected.get(column))
                    .try_fold(0usize, |count, keys| count.checked_add(keys.len()))?;
                if selected_count > MAX_CLIPBOARD_CELLS {
                    return None;
                }
                let mut seen = HashSet::new();
                let row_sets: Vec<_> = columns
                    .iter()
                    .filter_map(|column| selected.get(column))
                    .filter(|keys| seen.insert(Arc::as_ptr(keys)))
                    .collect();
                let union: HashSet<_> = if row_sets.len() > 1 {
                    row_sets
                        .iter()
                        .flat_map(|rows| rows.iter().copied())
                        .collect()
                } else {
                    HashSet::new()
                };
                let mut index = 0;
                let mut first = count;
                let mut last = 0;
                model.visit_row_keys(0..count, |key| {
                    let included = if let [keys] = row_sets.as_slice() {
                        keys.contains(&key)
                    } else {
                        union.contains(&key)
                    };
                    if included {
                        first = first.min(index);
                        last = index + 1;
                    }
                    index += 1;
                });
                range = first..last;
                cells = Some(selection);
            }
        }
    } else {
        let row = usize::try_from(table.int("current-row")?).ok()?;
        if row >= count {
            return None;
        }
        range = row..row + 1;
    }

    let size = (rows.len() + range.len()).checked_mul(columns.len())?;
    if size == 0 || size > MAX_CLIPBOARD_CELLS {
        return None;
    }
    let mut text = String::new();
    for (line, row) in rows.into_iter().chain(range).enumerate() {
        if line != 0 {
            text.push('\n');
        }
        let line_start = text.len();
        let key = model.row_key(row);
        let item = model.item(row);
        for (slot, &column) in columns.iter().enumerate() {
            if slot != 0 {
                text.push('\t');
            }
            if cells.is_none_or(|selection| selection.contains_cell(key, column))
                && let Some(value) = cell(row, column, &item)
            {
                append_cell(&mut text, &value)?;
            }
            if text.len() > MAX_CLIPBOARD_BYTES {
                return None;
            }
        }
        if text.len() == line_start {
            // A blank final single-column row would otherwise look like a trailing newline.
            if text.len().checked_add(2)? > MAX_CLIPBOARD_BYTES {
                return None;
            }
            text.push_str("\"\"");
        }
    }
    Some(text)
}

fn append_cell(text: &mut String, value: &str) -> Option<()> {
    let quoted = value.contains(['\t', '\n', '\r', '"']);
    let extra = if quoted {
        value.bytes().filter(|&byte| byte == b'"').count() + 2
    } else {
        0
    };
    if text.len().saturating_add(value.len()).saturating_add(extra) > MAX_CLIPBOARD_BYTES {
        return None;
    }
    if quoted {
        text.push('"');
        for (index, part) in value.split('"').enumerate() {
            if index != 0 {
                text.push_str("\"\"");
            }
            text.push_str(part);
        }
        text.push('"');
    } else {
        text.push_str(value);
    }
    Some(())
}

#[doc(hidden)]
pub struct TableSelectionChange {
    pub current: Option<usize>,
    pub column_changed: bool,
    pub selected_count: Option<usize>,
}

#[derive(Default)]
struct Selection {
    stamp: Option<(u64, u64)>,
    target: SelectionTarget,
    multiple: bool,
    selected: Option<Selected>,
    row_anchor: Option<(usize, RowKey)>,
    column_anchor: Option<usize>,
    current_column: Option<usize>,
}

impl Selection {
    fn refresh(
        &mut self,
        model: &impl ListModel,
        stamp: (u64, u64),
        target: SelectionTarget,
        multiple: bool,
    ) {
        if self.target != target || self.multiple != multiple {
            *self = Self {
                target,
                multiple,
                stamp: Some(stamp),
                ..Self::default()
            };
            return;
        }
        if self.stamp == Some(stamp) {
            return;
        }
        if let Some(previous) = self.stamp.replace(stamp) {
            if previous.0 != stamp.0 && target != SelectionTarget::Columns {
                self.selected = Some(Selected::empty(target));
                self.row_anchor = None;
            } else if let Some(selected) = &mut self.selected {
                selected.retain_live(model);
            }
        }
    }

    fn selected(&self, model: &impl ListModel, current: Option<usize>) -> Selected {
        self.selected.clone().unwrap_or_else(|| {
            if self.target == SelectionTarget::Rows {
                Selected::Rows(Arc::new(
                    current
                        .filter(|&index| index < model.row_count())
                        .map(|index| model.row_key(index))
                        .into_iter()
                        .collect(),
                ))
            } else {
                Selected::empty(self.target)
            }
        })
    }

    fn row_range(
        &mut self,
        model: &impl ListModel,
        current: Option<usize>,
        row: usize,
    ) -> Range<usize> {
        let count = model.row_count();
        let anchor = self
            .row_anchor
            .and_then(|(old, key)| {
                if old < count && model.row_key(old) == key {
                    Some(old)
                } else {
                    model.index_of_key(key)
                }
            })
            .or(current.filter(|&index| index < count))
            .unwrap_or(row);
        self.row_anchor = Some((anchor, model.row_key(anchor)));
        span(anchor, row)
    }

    fn column_range<'a>(
        &mut self,
        column: usize,
        view: &'a TableSelectionSnapshot,
    ) -> impl Iterator<Item = usize> + use<'a> {
        let index = |column| view.order.visual_index(view.column_count, column);
        let anchor = self
            .column_anchor
            .or(self.current_column)
            .filter(|&column| column < view.column_count)
            .unwrap_or(column);
        self.column_anchor = Some(anchor);
        let range = span(index(anchor).unwrap_or(0), index(column).unwrap_or(0));
        view.order
            .iter(view.column_count)
            .skip(range.start)
            .take(range.len())
    }
}

fn toggle<T: Ord>(set: &mut BTreeSet<T>, key: T) {
    if !set.remove(&key) {
        set.insert(key);
    }
}

/// Generated per table so gestures and data changes share the same selection.
#[doc(hidden)]
#[derive(Default)]
pub struct TableSelection {
    state: RefCell<Selection>,
    changed: ChangeSignal,
}

impl TableSelection {
    pub fn snapshot(
        &self,
        model: &impl ListModel,
        current: Option<usize>,
        target: SelectionTarget,
        column_count: usize,
        multiple: bool,
    ) -> TableSelectionSnapshot {
        self.changed.track();
        let stamp = model.structure_stamp();
        let mut state = self.state.borrow_mut();
        state.refresh(model, stamp, target, multiple);
        TableSelectionSnapshot {
            stamp,
            target,
            column_count,
            order: crate::TableColumnOrder::default(),
            multiple,
            current_column: state.current_column,
            selected: state.selected(model, current),
        }
    }

    pub fn apply(
        &self,
        model: &impl ListModel,
        view: &TableSelectionSnapshot,
        current: Option<usize>,
        action: TableSelectionAction,
    ) -> Option<TableSelectionChange> {
        let stamp = model.structure_stamp();
        let mut state = self.state.borrow_mut();
        if stamp != view.stamp || state.target != view.target || state.multiple != view.multiple {
            return None;
        }
        let count = model.row_count();
        let column_count = view.column_count;
        state.refresh(model, stamp, view.target, view.multiple);
        let previous = state.selected(model, current);
        let mut next = previous.clone();
        let mut cursor = None;
        let old_column = state.current_column;
        let operation_for = |operation| {
            if view.multiple || operation == RowSelection::Move {
                operation
            } else {
                RowSelection::Replace
            }
        };
        match action {
            TableSelectionAction::Row { index, operation } if index < count => {
                let operation = operation_for(operation);
                cursor = Some(index);
                if let Selected::Rows(rows) = &mut next {
                    let key = model.row_key(index);
                    match operation {
                        RowSelection::Replace => {
                            state.row_anchor = Some((index, key));
                            *rows = Arc::new(BTreeSet::from([key]));
                        }
                        RowSelection::Toggle => {
                            state.row_anchor = Some((index, key));
                            toggle(Arc::make_mut(rows), key);
                        }
                        RowSelection::Extend | RowSelection::AddRange => {
                            let keys = row_keys(model, state.row_range(model, current, index));
                            if operation == RowSelection::Extend {
                                *rows = Arc::new(keys);
                            } else {
                                Arc::make_mut(rows).extend(keys);
                            }
                        }
                        RowSelection::Move => {}
                    }
                } else if operation != RowSelection::Move {
                    return None;
                }
            }
            TableSelectionAction::Column { column, operation } if column < column_count => {
                let operation = operation_for(operation);
                let Selected::Columns(columns) = &mut next else {
                    return None;
                };
                match operation {
                    RowSelection::Replace => {
                        state.column_anchor = Some(column);
                        *columns = Arc::new(BTreeSet::from([column]));
                    }
                    RowSelection::Toggle => {
                        state.column_anchor = Some(column);
                        toggle(Arc::make_mut(columns), column);
                    }
                    RowSelection::Extend | RowSelection::AddRange => {
                        let range = state.column_range(column, view);
                        if operation == RowSelection::Extend {
                            *columns = Arc::new(range.collect());
                        } else {
                            Arc::make_mut(columns).extend(range);
                        }
                    }
                    RowSelection::Move => {}
                }
                state.current_column = Some(column);
            }
            TableSelectionAction::Cell {
                row,
                column,
                operation,
            } if row < count && column < column_count => {
                let operation = operation_for(operation);
                let Selected::Cells(cells) = &mut next else {
                    return None;
                };
                cursor = Some(row);
                match operation {
                    RowSelection::Replace | RowSelection::Toggle => {
                        let key = model.row_key(row);
                        state.row_anchor = Some((row, key));
                        state.column_anchor = Some(column);
                        if operation == RowSelection::Replace {
                            *cells = Arc::new(BTreeMap::from([(
                                column,
                                Arc::new(BTreeSet::from([key])),
                            )]));
                        } else {
                            let cells = Arc::make_mut(cells);
                            let rows = cells.entry(column).or_default();
                            toggle(Arc::make_mut(rows), key);
                            if rows.is_empty() {
                                cells.remove(&column);
                            }
                        }
                    }
                    RowSelection::Extend | RowSelection::AddRange => {
                        let range = state.column_range(column, view);
                        let rows = Arc::new(row_keys(model, state.row_range(model, current, row)));
                        if operation == RowSelection::Extend {
                            *cells = Arc::new(range.map(|column| (column, rows.clone())).collect());
                        } else {
                            let cells = Arc::make_mut(cells);
                            let mut merged = HashMap::<_, RowKeys>::new();
                            for column in range {
                                let keys = cells.entry(column).or_insert_with(|| rows.clone());
                                if !rows.is_subset(keys) {
                                    *keys = merged
                                        .entry(Arc::as_ptr(keys))
                                        .or_insert_with(|| {
                                            Arc::new(keys.union(&rows).copied().collect())
                                        })
                                        .clone();
                                }
                            }
                        }
                    }
                    RowSelection::Move => {}
                }
                state.current_column = Some(column);
            }
            TableSelectionAction::All if view.multiple => {
                let total = match view.target {
                    SelectionTarget::Rows => count,
                    SelectionTarget::Columns => column_count,
                    SelectionTarget::Cells => count.saturating_mul(column_count),
                };
                if previous.len() == total {
                    return Some(TableSelectionChange {
                        current: None,
                        column_changed: false,
                        selected_count: None,
                    });
                }
                next = match view.target {
                    SelectionTarget::Rows => Selected::Rows(Arc::new(row_keys(model, 0..count))),
                    SelectionTarget::Columns => {
                        Selected::Columns(Arc::new((0..column_count).collect()))
                    }
                    SelectionTarget::Cells => {
                        if count == 0 {
                            Selected::empty(view.target)
                        } else {
                            let rows = Arc::new(row_keys(model, 0..count));
                            Selected::Cells(Arc::new(
                                (0..column_count)
                                    .map(|column| (column, rows.clone()))
                                    .collect(),
                            ))
                        }
                    }
                };
            }
            TableSelectionAction::Clear => {
                state.row_anchor = None;
                state.column_anchor = None;
                next = Selected::empty(view.target);
            }
            _ => return None,
        }
        let selected_count = (next != previous).then_some(next.len());
        let column_changed = state.current_column != old_column;
        state.selected = Some(next);
        if selected_count.is_some() || column_changed {
            self.changed.notify();
        }
        Some(TableSelectionChange {
            current: cursor.filter(|index| Some(*index) != current),
            column_changed,
            selected_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_column_copy_is_ordered_and_checks_the_budget_before_projecting() {
        let selection = TableSelection::default();
        let model = 3_i64;
        let mut table = Node::new("TableView", None);
        table.table_header = Some(Box::new(crate::TableHeader {
            columns: (0..3).map(|_| Node::new("TableColumn", None)).collect(),
            sort: None,
            column_widths: Vec::new(),
        }));
        for (column, operation) in [(1, RowSelection::Replace), (2, RowSelection::Toggle)] {
            let view = selection.snapshot(&model, None, SelectionTarget::Columns, 3, true);
            assert!(
                selection
                    .apply(
                        &model,
                        &view,
                        None,
                        TableSelectionAction::Column { column, operation }
                    )
                    .is_some()
            );
        }
        table.table_selection = Some(Box::new(selection.snapshot(
            &model,
            None,
            SelectionTarget::Columns,
            3,
            true,
        )));
        let text = copy_table_text(&model, &table, &[2, 0, 1], |_, column, value| {
            Some(format!("{value}:{column}"))
        });
        assert_eq!(text.as_deref(), Some("0:2\t0:1\n1:2\t1:1\n2:2\t2:1"));
        assert!(
            copy_table_text(&600_000_i64, &table, &[2, 0, 1], |_, _, _| panic!(
                "oversized selection must be rejected before projection"
            ))
            .is_none()
        );
    }

    #[test]
    fn numeric_models_prune_removed_positions_and_reject_stale_actions() {
        let selection = TableSelection::default();
        let view = selection.snapshot(&5_i64, Some(1), SelectionTarget::Rows, 1, true);
        assert_eq!(view.indices(&5_i64), [1]);
        let change = selection
            .apply(
                &5_i64,
                &view,
                Some(1),
                TableSelectionAction::Row {
                    index: 4,
                    operation: RowSelection::Extend,
                },
            )
            .unwrap();
        assert_eq!(change.selected_count, Some(4));
        assert_eq!(change.current, Some(4));
        let stale = selection.snapshot(&5_i64, Some(4), SelectionTarget::Rows, 1, true);
        assert!(
            selection
                .apply(&3_i64, &stale, None, TableSelectionAction::All)
                .is_none()
        );
        assert_eq!(
            selection
                .snapshot(&3_i64, None, SelectionTarget::Rows, 1, true)
                .indices(&3_i64),
            [1, 2]
        );
    }

    #[test]
    fn cell_select_all_shares_row_keys_across_columns() {
        let selection = TableSelection::default();
        let model = crate::VecModel::from((0..10_000).collect::<Vec<_>>());
        let view = selection.snapshot(&model, None, SelectionTarget::Cells, 128, true);
        selection
            .apply(&model, &view, None, TableSelectionAction::All)
            .unwrap();
        let view = selection.snapshot(&model, None, SelectionTarget::Cells, 128, true);
        assert_eq!(view.len(), 1_280_000);
        assert!(std::ptr::eq(
            view.cell_keys(0).unwrap(),
            view.cell_keys(127).unwrap()
        ));
        let removed = model.row_key(0).unwrap();
        model.remove(0);
        let view = selection.snapshot(&model, None, SelectionTarget::Cells, 128, true);
        assert_eq!(view.len(), 1_279_872);
        assert!(!view.contains_cell(removed, 0));
        assert!(std::ptr::eq(
            view.cell_keys(0).unwrap(),
            view.cell_keys(127).unwrap()
        ));
    }

    #[test]
    fn grid_selection_can_be_single_and_mode_changes_reject_old_gestures() {
        let selection = TableSelection::default();
        let view = selection.snapshot(&5_i64, None, SelectionTarget::Cells, 2, false);
        assert!(
            selection
                .apply(&5_i64, &view, None, TableSelectionAction::All)
                .is_none()
        );
        for (row, column, operation) in
            [(0, 0, RowSelection::Replace), (4, 1, RowSelection::Extend)]
        {
            selection
                .apply(
                    &5_i64,
                    &view,
                    None,
                    TableSelectionAction::Cell {
                        row,
                        column,
                        operation,
                    },
                )
                .unwrap();
        }
        let view = selection.snapshot(&5_i64, None, SelectionTarget::Cells, 2, false);
        assert_eq!(view.len(), 1);
        assert!(view.contains_cell(5_i64.row_key(4), 1));
        let columns = selection.snapshot(&5_i64, None, SelectionTarget::Columns, 2, true);
        assert!(columns.is_empty());
        assert!(
            selection
                .apply(&5_i64, &view, None, TableSelectionAction::Clear)
                .is_none()
        );
    }
}
