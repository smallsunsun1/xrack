use crate::table_clipboard::{MAX_CLIPBOARD_BYTES, MAX_CLIPBOARD_CELLS};
use crate::{
    ChangeSignal, ControlAction, KeyModifiers, ListModel, Node, RowKey, TableColumnOrder,
    TableRows, table_rows::with_table_rows_in_view,
};
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

impl RowSelection {
    #[doc(hidden)]
    pub fn from_modifiers(modifiers: KeyModifiers, keyboard: bool) -> Self {
        match (modifiers.shift, modifiers.control || modifiers.meta) {
            (true, true) => Self::AddRange,
            (true, false) => Self::Extend,
            (false, true) if keyboard => Self::Move,
            (false, true) => Self::Toggle,
            (false, false) => Self::Replace,
        }
    }
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
    DragRange {
        anchor: (usize, usize),
        end: (usize, usize),
        additive: bool,
    },
    All,
    Clear,
}

impl TableSelectionAction {
    /// Resolve a selection against the table column order seen by the input event.
    #[doc(hidden)]
    pub fn from_control(
        table: &Node,
        live_order: &TableColumnOrder,
        action: ControlAction,
    ) -> Option<Self> {
        if table.table_selection.is_none() || &table.table_column_order != live_order {
            return None;
        }
        let selection = match action {
            ControlAction::Select(index) => Self::Row {
                index,
                operation: RowSelection::Replace,
            },
            ControlAction::TableSelection(selection) => selection,
            _ => return None,
        };
        (!matches!(selection, Self::DragRange { .. }) || table.boolean("drag-select", false))
            .then_some(selection)
    }
}

/// Selection commands with signed positions from XR dialect operands.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum TableSelectionMethod {
    SelectAll,
    ClearSelection,
    SelectRow,
    SelectColumn,
    SelectCell,
    MoveCell,
    ToggleCell,
    SelectRange,
}

impl TableSelectionMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "select-all" => Self::SelectAll,
            "clear-selection" => Self::ClearSelection,
            "select-row" => Self::SelectRow,
            "select-column" => Self::SelectColumn,
            "select-cell" => Self::SelectCell,
            "set-current-cell" => Self::MoveCell,
            "toggle-cell" => Self::ToggleCell,
            "select-range" => Self::SelectRange,
            _ => return None,
        })
    }

    pub fn action(self, args: &[i64]) -> Option<TableSelectionAction> {
        use TableSelectionAction as Action;
        let index = |value| usize::try_from(value).ok();
        Some(match (self, args) {
            (Self::SelectAll, []) => Action::All,
            (Self::ClearSelection, []) => Action::Clear,
            (Self::SelectRow, [row]) => Action::Row {
                index: index(*row)?,
                operation: RowSelection::Replace,
            },
            (Self::SelectColumn, [column]) => Action::Column {
                column: index(*column)?,
                operation: RowSelection::Replace,
            },
            (Self::SelectCell | Self::MoveCell | Self::ToggleCell, [row, column]) => Action::Cell {
                row: index(*row)?,
                column: index(*column)?,
                operation: match self {
                    Self::MoveCell => RowSelection::Move,
                    Self::ToggleCell => RowSelection::Toggle,
                    _ => RowSelection::Replace,
                },
            },
            (Self::SelectRange, [row, column, end_row, end_column]) => Action::DragRange {
                anchor: (index(*row)?, index(*column)?),
                end: (index(*end_row)?, index(*end_column)?),
                additive: false,
            },
            _ => return None,
        })
    }
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
                model.visit_source_row_keys(|key| {
                    if rows.contains(&key) {
                        live.insert(key);
                    }
                });
                if live.len() != rows.len() {
                    *rows = Arc::new(live);
                }
            }
            Self::Cells(cells) => {
                let mut live = BTreeSet::new();
                model.visit_source_row_keys(|key| {
                    live.insert(key);
                });
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
    drag_range: Option<CommittedRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommittedRange {
    endpoints: [(usize, usize); 2],
    order: crate::TableColumnOrder,
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

    #[doc(hidden)]
    pub fn structure_stamp(&self) -> (u64, u64) {
        self.stamp
    }

    /// Endpoints of the last replacement drag, in row and logical column indices.
    pub fn drag_range(&self) -> Option<[(usize, usize); 2]> {
        let range = self.drag_range.as_ref()?;
        (range.order == self.order).then_some(range.endpoints)
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

    pub fn row_header_selected(&self, row: RowKey) -> bool {
        match &self.selected {
            Selected::Rows(rows) => rows.contains(&row),
            Selected::Cells(cells) => whole_row_selected(cells, self.column_count, row),
            Selected::Columns(_) => false,
        }
    }

    pub fn row_header_action(&self, index: usize, operation: RowSelection) -> TableSelectionAction {
        match self.target {
            SelectionTarget::Cells if !self.multiple => {
                let column = self
                    .current_column
                    .filter(|column| *column < self.column_count)
                    .or_else(|| self.order.iter(self.column_count).next())
                    .unwrap_or_default();
                TableSelectionAction::Cell {
                    row: index,
                    column,
                    operation,
                }
            }
            SelectionTarget::Columns => TableSelectionAction::Row {
                index,
                operation: RowSelection::Move,
            },
            _ => TableSelectionAction::Row { index, operation },
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
pub fn copy_table_text_in_order<M: ListModel>(
    rows: &crate::TableRows<M>,
    table: &Node,
    columns: &[usize],
    mut cell: impl FnMut(usize, usize, &M::Item) -> Option<String>,
) -> Option<String> {
    try_copy_table_text_in_order(rows, table, columns, |row, column, item| {
        Ok::<_, std::convert::Infallible>(cell(row, column, item))
    })
    .unwrap_or_else(|error| match error {})
}

/// Serialize a selection, stopping before evaluating any cells after an error.
#[doc(hidden)]
pub fn try_copy_table_text_in_order<M: ListModel, E>(
    rows: &TableRows<M>,
    table: &Node,
    columns: &[usize],
    mut cell: impl FnMut(usize, usize, &M::Item) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    if table.standard_list_stamp != Some(rows.positions().stamp()) {
        return Ok(None);
    }
    let source = rows.source();
    let stamp = source.contents_stamp();
    try_copy_table_rows(
        rows,
        table,
        columns,
        || source.contents_stamp() == stamp,
        |row, column, _, item| cell(row, column, item),
    )
}

#[doc(hidden)]
pub fn copy_table_text_in_view<M: ListModel>(
    table: &Node,
    columns: &[usize],
    rows: impl FnOnce() -> TableRows<M>,
    cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Option<String>,
) -> Option<String> {
    copy_table_text_in_view_if(table, columns, || Some(rows()), cell)
}

#[doc(hidden)]
pub fn copy_table_text_in_view_if<M: ListModel>(
    table: &Node,
    columns: &[usize],
    rows: impl FnOnce() -> Option<TableRows<M>>,
    mut cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Option<String>,
) -> Option<String> {
    try_copy_table_text_in_view(
        table,
        columns,
        || Ok::<_, std::convert::Infallible>(rows()),
        |row, column, key, item| Ok(cell(row, column, key, item)),
    )
    .unwrap_or_else(|error| match error {})
}

/// Copy cells only while the model and the dependencies read by the projection stay current.
#[doc(hidden)]
pub fn try_copy_table_text_in_view<M: ListModel, E>(
    table: &Node,
    columns: &[usize],
    rows: impl FnOnce() -> Result<Option<TableRows<M>>, E>,
    cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    try_copy_table_text_while_current(table, columns, || true, rows, cell)
}

#[doc(hidden)]
pub fn copy_table_text_while_current<M: ListModel>(
    table: &Node,
    columns: &[usize],
    available: impl Fn() -> bool,
    rows: impl FnOnce() -> Option<TableRows<M>>,
    mut cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Option<String>,
) -> Option<String> {
    try_copy_table_text_while_current(
        table,
        columns,
        available,
        || Ok::<_, std::convert::Infallible>(rows()),
        |row, column, key, item| Ok(cell(row, column, key, item)),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_copy_table_text_while_current<M: ListModel, E>(
    table: &Node,
    columns: &[usize],
    available: impl Fn() -> bool,
    rows: impl FnOnce() -> Result<Option<TableRows<M>>, E>,
    cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    with_table_rows_in_view(table, available, rows, |rows, current| {
        try_copy_table_rows(rows, table, columns, current, cell)
    })
}

fn try_copy_table_rows<M: ListModel, E>(
    rows: &TableRows<M>,
    table: &Node,
    columns: &[usize],
    current: impl Fn() -> bool,
    mut cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    try_copy_table_text(rows, table, columns, current, |row, column, key, item| {
        let Some(source) = rows.positions().source_index(row) else {
            return Ok(None);
        };
        cell(source, column, key, item)
    })
}

#[doc(hidden)]
pub fn copy_table_text<M: ListModel>(
    model: &M,
    table: &Node,
    displayed_columns: &[usize],
    mut cell: impl FnMut(usize, usize, &M::Item) -> Option<String>,
) -> Option<String> {
    let stamp = model.contents_stamp();
    try_copy_table_text(
        model,
        table,
        displayed_columns,
        || model.contents_stamp() == stamp,
        |row, column, _, item| Ok::<_, std::convert::Infallible>(cell(row, column, item)),
    )
    .unwrap_or_else(|error| match error {})
}

struct ClipboardSelection<'a> {
    rows: Vec<usize>,
    range: Range<usize>,
    columns: Vec<usize>,
    cells: Option<&'a TableSelectionSnapshot>,
}

fn clipboard_selection<'a>(
    model: &impl ListModel,
    table: &'a Node,
    displayed_columns: &[usize],
) -> Option<ClipboardSelection<'a>> {
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

    let size = rows
        .len()
        .checked_add(range.len())?
        .checked_mul(columns.len())?;
    if size == 0 || size > MAX_CLIPBOARD_CELLS {
        return None;
    }
    Some(ClipboardSelection {
        rows,
        range,
        columns,
        cells,
    })
}

fn try_copy_table_text<M: ListModel, E>(
    model: &M,
    table: &Node,
    displayed_columns: &[usize],
    current: impl Fn() -> bool,
    mut cell: impl FnMut(usize, usize, RowKey, &M::Item) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    let Some(ClipboardSelection {
        rows,
        range,
        columns,
        cells,
    }) = clipboard_selection(model, table, displayed_columns)
    else {
        return Ok(None);
    };
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
            if cells.is_none_or(|selection| selection.contains_cell(key, column)) {
                let value = cell(row, column, key, &item)?;
                if !current() {
                    return Ok(None);
                }
                if let Some(value) = value {
                    let Some(()) = append_cell(&mut text, &value) else {
                        return Ok(None);
                    };
                }
            }
            if text.len() > MAX_CLIPBOARD_BYTES {
                return Ok(None);
            }
        }
        if text.len() == line_start {
            // A blank final single-column row would otherwise look like a trailing newline.
            if text.len().saturating_add(2) > MAX_CLIPBOARD_BYTES {
                return Ok(None);
            }
            text.push_str("\"\"");
        }
    }
    Ok(Some(text))
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

#[doc(hidden)]
pub enum TableSelectionEvent {
    CurrentRowChanged(usize),
    CurrentCellChanged,
    SelectionChanged(usize),
}

impl TableSelectionChange {
    /// Publish the cursor before resolving the cell notification. The row
    /// handler can change bindings that the cell handler will read.
    pub fn try_notify<E>(
        self,
        notify_cell: bool,
        mut notify: impl FnMut(TableSelectionEvent) -> Result<(), E>,
    ) -> Result<bool, E> {
        if let Some(row) = self.current {
            notify(TableSelectionEvent::CurrentRowChanged(row))?;
        }
        let cursor_changed = self.current.is_some() || self.column_changed;
        if notify_cell && cursor_changed {
            notify(TableSelectionEvent::CurrentCellChanged)?;
        }
        if let Some(count) = self.selected_count {
            notify(TableSelectionEvent::SelectionChanged(count))?;
        }
        Ok(cursor_changed || self.selected_count.is_some())
    }

    pub fn notify(self, notify_cell: bool, mut notify: impl FnMut(TableSelectionEvent)) -> bool {
        self.try_notify(notify_cell, |event| {
            notify(event);
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap_or_else(|error| match error {})
    }
}

#[doc(hidden)]
pub struct TableSelectionOptions {
    pub target: SelectionTarget,
    pub multiple: bool,
    pub order: TableColumnOrder,
    pub drag_select: bool,
}

/// Address a table selection by its displayed position or source row.
#[doc(hidden)]
pub enum TableSelectionCommand {
    Visual(TableSelectionAction),
    Source { row: usize, column: Option<usize> },
}

#[derive(Default)]
struct Selection {
    stamp: Option<(u64, u64)>,
    source_stamp: Option<(u64, u64)>,
    target: SelectionTarget,
    multiple: bool,
    selected: Option<Selected>,
    row_anchor: Option<(usize, RowKey)>,
    column_anchor: Option<usize>,
    current_column: Option<usize>,
    drag_range: Option<CommittedRange>,
}

impl Selection {
    fn refresh(
        &mut self,
        model: &impl ListModel,
        stamp: (u64, u64),
        target: SelectionTarget,
        multiple: bool,
    ) {
        let source = model.source_structure_stamp();
        if self.target != target || self.multiple != multiple {
            *self = Self {
                target,
                multiple,
                stamp: Some(stamp),
                source_stamp: Some(source),
                ..Self::default()
            };
            return;
        }
        if self.stamp == Some(stamp) {
            return;
        }
        self.drag_range = None;
        let source_changed = self.source_stamp.replace(source) != Some(source);
        if let Some(previous) = self.stamp.replace(stamp) {
            if previous.0 != stamp.0 && target != SelectionTarget::Columns {
                self.selected = Some(Selected::empty(target));
                self.row_anchor = None;
            } else if source_changed && let Some(selected) = &mut self.selected {
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
        let anchor = self
            .column_anchor
            .or(self.current_column)
            .filter(|&column| column < view.column_count)
            .unwrap_or(column);
        self.column_anchor = Some(anchor);
        column_span(view, anchor, column)
    }
}

fn column_span(
    view: &TableSelectionSnapshot,
    anchor: usize,
    column: usize,
) -> impl Iterator<Item = usize> + use<'_> {
    let index = |column| view.order.visual_index(view.column_count, column);
    let range = span(index(anchor).unwrap_or(0), index(column).unwrap_or(0));
    view.order
        .iter(view.column_count)
        .skip(range.start)
        .take(range.len())
}

fn select_cells(
    cells: &mut Arc<ColumnCells>,
    columns: impl Iterator<Item = usize>,
    rows: RowKeys,
    additive: bool,
) {
    if !additive {
        *cells = Arc::new(columns.map(|column| (column, rows.clone())).collect());
        return;
    }
    let cells = Arc::make_mut(cells);
    let mut merged = HashMap::<_, RowKeys>::new();
    for column in columns {
        let keys = cells.entry(column).or_insert_with(|| rows.clone());
        if !rows.is_subset(keys) {
            *keys = merged
                .entry(Arc::as_ptr(keys))
                .or_insert_with(|| Arc::new(keys.union(&rows).copied().collect()))
                .clone();
        }
    }
}

fn whole_row_selected(cells: &ColumnCells, column_count: usize, row: RowKey) -> bool {
    column_count > 0
        && cells.len() == column_count
        && cells.values().all(|keys| keys.contains(&row))
}

fn remove_row_from_cells(cells: &mut Arc<ColumnCells>, row: RowKey) {
    let mut trimmed = HashMap::new();
    Arc::make_mut(cells).retain(|_, keys| {
        *keys = trimmed
            .entry(Arc::as_ptr(keys))
            .or_insert_with(|| {
                let mut remaining = (**keys).clone();
                remaining.remove(&row);
                Arc::new(remaining)
            })
            .clone();
        !keys.is_empty()
    });
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
    pub fn has_explicit_selection(&self) -> bool {
        self.changed.track();
        self.state.borrow().selected.is_some()
    }

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
            drag_range: state.drag_range.clone(),
        }
    }

    /// Read the live cursor and selection bindings in the same order for both backends.
    #[doc(hidden)]
    pub fn try_snapshot_with<E>(
        &self,
        model: &impl ListModel,
        column_count: usize,
        current: impl FnOnce() -> Result<i64, E>,
        target: impl FnOnce() -> Result<SelectionTarget, E>,
        multiple: impl FnOnce() -> Result<bool, E>,
        order: impl FnOnce() -> TableColumnOrder,
    ) -> Result<TableSelectionSnapshot, E> {
        let current = usize::try_from(current()?).ok();
        Ok(self
            .snapshot(model, current, target()?, column_count, multiple()?)
            .with_column_order(order()))
    }

    #[doc(hidden)]
    pub fn snapshot_with(
        &self,
        model: &impl ListModel,
        column_count: usize,
        current: impl FnOnce() -> i64,
        target: impl FnOnce() -> SelectionTarget,
        multiple: impl FnOnce() -> bool,
        order: impl FnOnce() -> TableColumnOrder,
    ) -> TableSelectionSnapshot {
        self.try_snapshot_with(
            model,
            column_count,
            || Ok::<_, std::convert::Infallible>(current()),
            || Ok(target()),
            || Ok(multiple()),
            order,
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Build the rendered selection from the same row projection and bindings as the scene.
    #[doc(hidden)]
    pub fn populate_scene(&self, table: &mut Node, model: &impl ListModel) {
        let current = table
            .int("current-row")
            .and_then(|row| usize::try_from(row).ok());
        let target = table.selection_target();
        let columns = table
            .table_header
            .as_ref()
            .map_or(0, |header| header.columns.len());
        let multiple = table.boolean("multi-select", false);
        let snapshot = self
            .snapshot(model, current, target, columns, multiple)
            .with_column_order(table.table_column_order.clone());
        table.table_selection =
            (multiple || target != SelectionTarget::Rows || self.has_explicit_selection())
                .then(|| Box::new(snapshot));
    }

    /// Apply a scene's selection only while its row order and selection mode still apply.
    #[doc(hidden)]
    pub fn try_apply_control<M: ListModel, E>(
        &self,
        table: &Node,
        input: crate::InputRoute<'_>,
        available: impl FnOnce() -> Result<bool, E>,
        rows: impl FnOnce() -> Result<M, E>,
        options: impl FnOnce() -> Result<TableSelectionOptions, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<TableSelectionChange>, E> {
        let crate::RoutedEvent::Control(action) = *input.event() else {
            return Ok(None);
        };
        let prepared = crate::property::watch_reads(|reads| {
            if !available()? || reads.changed() {
                return Ok(None);
            }
            let rows = rows()?;
            if table.standard_list_stamp != Some(rows.structure_stamp()) || reads.changed() {
                return Ok(None);
            }
            let row_count = rows.row_count();
            let options = options()?;
            if options.target != table.selection_target()
                || options.multiple != table.boolean("multi-select", false)
                || reads.changed()
            {
                return Ok(None);
            }
            let selection = if table.table_selection.is_some() {
                let Some(selection) =
                    TableSelectionAction::from_control(table, &options.order, action)
                else {
                    return Ok(None);
                };
                if matches!(selection, TableSelectionAction::DragRange { .. })
                    && !options.drag_select
                {
                    return Ok(None);
                }
                Some(selection)
            } else {
                if !matches!(action, ControlAction::Select(index) if index < row_count)
                    || self.has_explicit_selection()
                {
                    return Ok(None);
                }
                None
            };
            let current = usize::try_from(current()?).ok();
            Ok((input.is_current() && !reads.changed())
                .then_some((rows, options, current, selection)))
        })?;
        let Some((rows, options, current, selection)) = prepared else {
            return Ok(None);
        };
        if let Some(selection) = selection {
            let column_count = table
                .table_header
                .as_ref()
                .map_or(0, |header| header.columns.len());
            let view = self
                .snapshot(
                    &rows,
                    current,
                    options.target,
                    column_count,
                    options.multiple,
                )
                .with_column_order(options.order);
            return Ok(self.apply(&rows, &view, current, selection));
        }
        Ok(match action {
            ControlAction::Select(index) if Some(index) != current => Some(TableSelectionChange {
                current: Some(index),
                column_changed: false,
                selected_count: None,
            }),
            _ => None,
        })
    }

    #[doc(hidden)]
    pub fn apply_control<M: ListModel>(
        &self,
        table: &Node,
        input: crate::InputRoute<'_>,
        available: impl FnOnce() -> bool,
        rows: impl FnOnce() -> M,
        options: impl FnOnce() -> TableSelectionOptions,
        current: impl FnOnce() -> i64,
    ) -> Option<TableSelectionChange> {
        self.try_apply_control(
            table,
            input,
            || Ok::<_, std::convert::Infallible>(available()),
            || Ok(rows()),
            || Ok(options()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Apply a command using one live model and cursor snapshot.
    #[doc(hidden)]
    pub fn try_apply_command<M: ListModel, E>(
        &self,
        command: TableSelectionCommand,
        column_count: usize,
        rows: impl FnOnce() -> Result<TableRows<M>, E>,
        current: impl FnOnce() -> Result<i64, E>,
        options: impl FnOnce() -> Result<TableSelectionOptions, E>,
    ) -> Result<Option<TableSelectionChange>, E> {
        let prepared = crate::property::watch_reads(|reads| {
            let rows = rows()?;
            let command = match command {
                TableSelectionCommand::Source { row, column } => {
                    let Some(row) = rows.positions().visual_index(row) else {
                        return Ok(None);
                    };
                    TableSelectionCommand::Source { row, column }
                }
                command => command,
            };
            let current = usize::try_from(current()?).ok();
            let options = options()?;
            Ok((!reads.changed()).then_some((rows, current, options, command)))
        })?;
        let Some((rows, current, options, command)) = prepared else {
            return Ok(None);
        };
        let action = match command {
            TableSelectionCommand::Visual(action) => action,
            TableSelectionCommand::Source {
                row,
                column: Some(column),
            } => TableSelectionAction::Cell {
                row,
                column,
                operation: RowSelection::Replace,
            },
            TableSelectionCommand::Source { row, column: None } => {
                let operation = match options.target {
                    SelectionTarget::Rows => RowSelection::Replace,
                    SelectionTarget::Cells if options.multiple => RowSelection::Replace,
                    _ => RowSelection::Move,
                };
                TableSelectionAction::Row {
                    index: row,
                    operation,
                }
            }
        };
        let view = self
            .snapshot(
                &rows,
                current,
                options.target,
                column_count,
                options.multiple,
            )
            .with_column_order(options.order);
        Ok(self.apply(&rows, &view, current, action))
    }

    #[doc(hidden)]
    pub fn apply_command<M: ListModel>(
        &self,
        command: TableSelectionCommand,
        column_count: usize,
        rows: impl FnOnce() -> TableRows<M>,
        current: impl FnOnce() -> i64,
        options: impl FnOnce() -> TableSelectionOptions,
    ) -> Option<TableSelectionChange> {
        self.try_apply_command(
            command,
            column_count,
            || Ok::<_, std::convert::Infallible>(rows()),
            || Ok(current()),
            || Ok(options()),
        )
        .unwrap_or_else(|error| match error {})
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
        let mut drag_range = None;
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
                } else if let Selected::Cells(cells) = &mut next
                    && view.multiple
                    && column_count > 0
                {
                    match operation {
                        RowSelection::Replace | RowSelection::Toggle => {
                            let key = model.row_key(index);
                            state.row_anchor = Some((index, key));
                            state.column_anchor = state
                                .current_column
                                .or_else(|| view.order.iter(column_count).next());
                            state.current_column = state.column_anchor;
                            if operation == RowSelection::Toggle
                                && whole_row_selected(cells, column_count, key)
                            {
                                remove_row_from_cells(cells, key);
                            } else {
                                select_cells(
                                    cells,
                                    0..column_count,
                                    Arc::new(BTreeSet::from([key])),
                                    operation == RowSelection::Toggle,
                                );
                            }
                        }
                        RowSelection::Extend | RowSelection::AddRange => {
                            let rows =
                                Arc::new(row_keys(model, state.row_range(model, current, index)));
                            select_cells(
                                cells,
                                0..column_count,
                                rows,
                                operation == RowSelection::AddRange,
                            );
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
                        select_cells(cells, range, rows, operation == RowSelection::AddRange);
                    }
                    RowSelection::Move => {}
                }
                state.current_column = Some(column);
            }
            TableSelectionAction::DragRange {
                anchor: (first_row, first_column),
                end: (row, column),
                additive,
            } if view.multiple
                && first_row < count
                && row < count
                && first_column < column_count
                && column < column_count =>
            {
                drag_range = (!additive).then(|| CommittedRange {
                    endpoints: [(first_row, first_column), (row, column)],
                    order: view.order.clone(),
                });
                match &mut next {
                    Selected::Rows(rows) => {
                        let range = row_keys(model, span(first_row, row));
                        if additive {
                            Arc::make_mut(rows).extend(range);
                        } else {
                            *rows = Arc::new(range);
                        }
                    }
                    Selected::Columns(columns) => {
                        let range = column_span(view, first_column, column);
                        if additive {
                            Arc::make_mut(columns).extend(range);
                        } else {
                            *columns = Arc::new(range.collect());
                        }
                    }
                    Selected::Cells(cells) => select_cells(
                        cells,
                        column_span(view, first_column, column),
                        Arc::new(row_keys(model, span(first_row, row))),
                        additive,
                    ),
                }
                if view.target != SelectionTarget::Columns {
                    state.row_anchor = Some((first_row, model.row_key(first_row)));
                    cursor = Some(row);
                }
                if view.target != SelectionTarget::Rows {
                    state.column_anchor = Some(first_column);
                    state.current_column = Some(column);
                }
            }
            TableSelectionAction::All if view.multiple => {
                let total = match view.target {
                    SelectionTarget::Rows => count,
                    SelectionTarget::Columns => column_count,
                    SelectionTarget::Cells => count.saturating_mul(column_count),
                };
                if previous.len() == total {
                    if state.drag_range.take().is_some() {
                        self.changed.notify();
                    }
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
        let range_changed = state.drag_range != drag_range;
        state.drag_range = drag_range;
        state.selected = Some(next);
        if selected_count.is_some() || column_changed || range_changed {
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
    fn drag_ranges_use_the_explicit_anchor_in_each_selection_mode() {
        let selection = TableSelection::default();
        let model = 6_i64;
        let rows = selection.snapshot(&model, Some(0), SelectionTarget::Rows, 3, true);
        let action = TableSelectionAction::DragRange {
            anchor: (4, 2),
            end: (1, 0),
            additive: false,
        };
        let change = selection.apply(&model, &rows, Some(0), action).unwrap();
        assert_eq!(change.current, Some(1));
        assert_eq!(change.selected_count, Some(4));
        assert_eq!(
            selection
                .snapshot(&model, Some(1), SelectionTarget::Rows, 3, true)
                .indices(&model),
            [1, 2, 3, 4],
        );

        let (_, order) = crate::TableColumnOrder::default().moved(3, 2, 0).unwrap();
        let columns = selection
            .snapshot(&model, Some(1), SelectionTarget::Columns, 3, true)
            .with_column_order(order.clone());
        let change = selection.apply(&model, &columns, Some(1), action).unwrap();
        assert_eq!(change.current, None);
        assert_eq!(change.selected_count, Some(2));
        let view = selection.snapshot(&model, Some(1), SelectionTarget::Columns, 3, true);
        assert_eq!(view.columns().iter().copied().collect::<Vec<_>>(), [0, 2]);
        assert_eq!(view.current_column(), Some(0));
        assert_eq!(view.drag_range(), None);
        assert_eq!(
            view.with_column_order(order).drag_range(),
            Some([(4, 2), (1, 0)])
        );
        let single = selection.snapshot(&model, Some(1), SelectionTarget::Columns, 3, false);
        assert!(selection.apply(&model, &single, Some(1), action).is_none());
    }

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
