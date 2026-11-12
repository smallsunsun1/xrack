use crate::{
    repeat::{Model, RepeatState, ViewModel},
    *,
};
use std::{cmp::Ordering, ops::Range};
use xrack_runtime::{
    ControlAction, ListMetrics, ListModel, Property, RecordValue, RowHeightOverrides, RowSelection,
    SelectionTarget, TableCellEditor, TableColumnOrder, TableColumnWindow, TableHeader, TableRows,
    TableSelection, TableSelectionAction, TableSelectionSnapshot, TableSort, VecModel,
};

pub(crate) struct Definition {
    pub(crate) model: Option<Expr>,
    pub(crate) filter_field: Option<Rc<[String]>>,
    pub(crate) row_header: Option<Rc<[String]>>,
}

#[derive(Default)]
pub(crate) struct State {
    sort: Property<Option<TableSort>>,
    widths: Property<Vec<Option<u32>>>,
    columns: Property<TableColumnOrder>,
    heights: RowHeightOverrides,
    selection: TableSelection,
    pub(crate) history: xrack_runtime::TableEditHistory<RecordValue>,
    pub(crate) string_history: xrack_runtime::TableEditHistory<String>,
}

impl State {
    pub(crate) fn column_order(&self) -> TableColumnOrder {
        self.columns.get(Default::default)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Command {
    SetHeight,
    Undo,
    Redo,
    MoveRow,
    MoveColumn,
    Sort,
    ClearSort,
    SelectAll,
    ClearSelection,
    SelectRow,
    SelectColumn,
    SelectCell,
    MoveCell,
    ToggleCell,
    SelectRange,
}

impl Command {
    pub(crate) fn parse(method: &str) -> Option<Self> {
        Some(match method {
            "xr.set_row_height" => Self::SetHeight,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "xr.move_row" => Self::MoveRow,
            "xr.move_column" => Self::MoveColumn,
            "sort-by" => Self::Sort,
            "clear-sort" => Self::ClearSort,
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
}

impl Element {
    pub(crate) fn table_columns(&self) -> impl Iterator<Item = &Element> {
        self.children
            .iter()
            .filter(|column| matches!(column.visual, Visual::Builtin("TableColumn")))
    }

    pub(crate) fn table_definition(&self) -> &Definition {
        self.table.as_ref().expect("table definition")
    }
}

pub(crate) fn field_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Int(value)) => value.to_string(),
        Some(Value::Float(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Time(value)) => value.to_string(),
        Some(Value::Date(value)) => value.to_string(),
        Some(Value::CheckState(value)) => format!("{value:?}"),
        _ => String::new(),
    }
}

fn compare_fields(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (left, right) {
        (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
        (Some(Value::Int(a)), Some(Value::Int(b))) => a.cmp(b),
        (Some(Value::Float(a)), Some(Value::Float(b))) => a.total_cmp(b),
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a.cmp(b),
        (Some(Value::Date(a)), Some(Value::Date(b))) => a.cmp(b),
        (Some(Value::Time(a)), Some(Value::Time(b))) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

fn filter_text(path: Rc<[String]>) -> impl Fn(&RecordValue) -> &str + Clone {
    move |row| match row.field_path(&path) {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    }
}

pub(crate) fn editor(value: &Value) -> Option<TableCellEditor> {
    Some(match value {
        Value::String(_) => TableCellEditor::String,
        Value::Int(_) => TableCellEditor::Int,
        Value::Float(_) => TableCellEditor::Float,
        Value::Bool(_) => TableCellEditor::Bool,
        Value::Date(_) => TableCellEditor::Date,
        Value::Time(_) => TableCellEditor::Time,
        _ => return None,
    })
}

fn index(value: i64) -> Option<usize> {
    usize::try_from(value).ok()
}

impl Instance {
    pub(crate) fn project_table(
        &self,
        view: &Element,
        state: &RepeatState,
        model: &VecModel<RecordValue>,
    ) -> Result<TableRows<VecModel<RecordValue>>> {
        let definition = view.table_definition();
        if definition.model.is_none() && view.visible_field.is_none() {
            return Ok(state.row_order.rows(Rc::new(model.clone())));
        }
        let columns: Rc<[Option<Rc<[String]>>]> = view
            .table_columns()
            .map(|column| column.column_field.clone())
            .collect();
        let error = std::cell::Cell::new(None);
        let rows = state.row_order.project_rows_by(
            || Rc::new(model.clone()),
            || match self.table_flag(view, "auto-sort", false) {
                Ok(true) if definition.model.is_some() => state.table_state().sort.get(|| None),
                Ok(_) => None,
                Err(failure) => {
                    error.set(Some(failure));
                    None
                }
            },
            move |column, left, right| {
                let Some(Some(path)) = columns.get(column) else {
                    return Ordering::Equal;
                };
                compare_fields(left.field_path(path), right.field_path(path))
            },
            definition.filter_field.clone().map(filter_text),
            || match self.binding_value(view, "filter-text") {
                Ok(Some(Value::String(text))) => text,
                Ok(_) => String::new(),
                Err(failure) => {
                    error.set(Some(failure));
                    String::new()
                }
            },
            view.visible_field.clone().map(|path| {
                move |row: &RecordValue| row.field_path(&path) == Some(&Value::Bool(true))
            }),
        );
        error.into_inner().map_or(Ok(rows), Err)
    }

    pub(crate) fn render_table(
        &self,
        view: &Element,
        model: &ViewModel,
        state: &RepeatState,
        node: &mut Node,
    ) -> Result<()> {
        let rows = model.table_rows()?;
        if rows.positions().projected() {
            node.properties.insert("movable-rows", false.into());
        }
        if view.table_definition().model.is_some() {
            node.properties.insert("__standard-table", true.into());
            node.properties
                .entry("follow-current")
                .or_insert(true.into());
        }
        node.table_header = Some(Box::new(TableHeader {
            columns: view
                .table_columns()
                .map(|column| self.render_element(column))
                .collect::<Result<_>>()?,
            sort: state.table_state().sort.get(|| None),
            column_widths: state.table_state().widths.get(Vec::new),
        }));
        node.table_column_order = state.table_state().columns.get(Default::default);
        node.row_heights = state.table_state().heights.snapshot_over(
            &rows,
            ListMetrics::row_height(node),
            node.row_heights.take(),
        );
        let snapshot = self.table_snapshot(view, &rows, state.table_state())?;
        if snapshot.multiple()
            || snapshot.target() != SelectionTarget::Rows
            || state.table_state().selection.has_explicit_selection()
        {
            node.table_selection = Some(Box::new(snapshot));
        }
        Ok(())
    }

    pub(crate) fn standard_table_rows(
        &self,
        view: &Element,
        model: &ViewModel,
        range: Range<usize>,
        window: &TableColumnWindow,
    ) -> Result<Vec<Node>> {
        let rows = model
            .projected
            .as_ref()
            .ok_or_else(|| Error::Invalid("standard table needs records".into()))?;
        let columns: Vec<_> = view
            .table_columns()
            .map(|column| {
                let field = column
                    .column_field
                    .as_deref()
                    .ok_or_else(|| Error::Invalid("missing column field".into()))?;
                Ok((field, self.table_flag(column, "editable", false)?))
            })
            .collect::<Result<_>>()?;
        let header = view.table_definition().row_header.as_deref();
        // Each column has a verified field type, so a single record provides its editor kind.
        let first = rows.source().row(0);
        Ok(xrack_runtime::project_standard_table_rows(
            rows,
            range,
            window,
            columns.len(),
            |record| header.map(|path| field_text(record.field_path(path))),
            |record, column| field_text(record.field_path(columns[column].0)),
            |column| {
                let (path, editable) = columns[column];
                editable
                    .then(|| editor(first.as_ref()?.field_path(path)?))
                    .flatten()
            },
        ))
    }

    pub(crate) fn table_flag(&self, element: &Element, name: &str, default: bool) -> Result<bool> {
        Ok(match self.binding_value(element, name)? {
            Some(Value::Bool(value)) => value,
            _ => default,
        })
    }

    fn current_row(&self, view: &Element) -> Result<i64> {
        Ok(match self.control_value(view, "current-row")? {
            Some(Value::Int(value)) => value,
            _ => -1,
        })
    }

    fn update_current_row(&self, view: &Element, row: usize) -> Result<()> {
        let value = Value::Int(row as i64);
        self.set_control_value(view, "current-row", value.clone())?;
        self.handler(view, "current-row-changed", &[value])?;
        Ok(())
    }

    fn table_snapshot(
        &self,
        view: &Element,
        model: &TableRows<Model>,
        state: &State,
    ) -> Result<TableSelectionSnapshot> {
        let target = match self.binding_value(view, "selection-target")? {
            Some(Value::SelectionTarget(target)) => target,
            _ => SelectionTarget::Rows,
        };
        Ok(state
            .selection
            .snapshot(
                model,
                index(self.current_row(view)?),
                target,
                view.table_columns().count(),
                self.table_flag(view, "multi-select", false)?,
            )
            .with_column_order(state.columns.get(Default::default)))
    }

    fn select_table(&self, view: &Element, action: TableSelectionAction) -> Result<bool> {
        let rows = self.view_model(view)?.table_rows()?;
        let state = self.repeat_state(view);
        let snapshot = self.table_snapshot(view, &rows, state.table_state())?;
        let current = index(self.current_row(view)?);
        let Some(change) = state
            .table_state()
            .selection
            .apply(&rows, &snapshot, current, action)
        else {
            return Ok(false);
        };
        if let Some(current) = change.current {
            self.update_current_row(view, current)?;
        }
        if change.current.is_some() || change.column_changed {
            let column = self
                .table_snapshot(
                    view,
                    &self.view_model(view)?.table_rows()?,
                    state.table_state(),
                )?
                .current_column()
                .map_or(-1, |column| column as i64);
            self.handler(
                view,
                "current-cell-changed",
                &[Value::Int(self.current_row(view)?), Value::Int(column)],
            )?;
        }
        if let Some(count) = change.selected_count {
            self.handler(view, "selection-changed", &[Value::Int(count as i64)])?;
        }
        Ok(change.current.is_some() || change.column_changed || change.selected_count.is_some())
    }

    pub(crate) fn select_table_source(
        &self,
        view: &Element,
        row: usize,
        column: Option<i64>,
    ) -> Result<()> {
        let action = if let Some(column) = column {
            let Some(column) = index(column) else {
                return Ok(());
            };
            TableSelectionAction::Cell {
                row,
                column,
                operation: RowSelection::Replace,
            }
        } else {
            let rows = self.view_model(view)?.table_rows()?;
            let state = self.repeat_state(view);
            let snapshot = self.table_snapshot(view, &rows, state.table_state())?;
            let operation = match snapshot.target() {
                SelectionTarget::Rows => RowSelection::Replace,
                SelectionTarget::Cells if snapshot.multiple() => RowSelection::Replace,
                _ => RowSelection::Move,
            };
            TableSelectionAction::Row {
                index: row,
                operation,
            }
        };
        self.select_table(view, action)?;
        Ok(())
    }

    fn sort_table(&self, view: &Element, sort: Option<TableSort>) -> Result<bool> {
        let state = self.repeat_state(view);
        if state.table_state().sort.get(|| None) == sort {
            return Ok(false);
        }
        if let Some(sort) = sort {
            let Some(column) = view.table_columns().nth(sort.column) else {
                return Ok(false);
            };
            if !self.table_flag(column, "sortable", true)? {
                return Ok(false);
            }
        }
        let cursor = if view.table_definition().model.is_some()
            && self.table_flag(view, "auto-sort", false)?
        {
            let rows = self.view_model(view)?.table_rows()?;
            index(self.current_row(view)?)
                .filter(|&row| row < rows.row_count())
                .map(|row| rows.row_key(row))
        } else {
            None
        };
        state.table_state().sort.set(sort);
        if let Some(cursor) = cursor
            && let Some(row) = self.view_model(view)?.table_rows()?.index_of_key(cursor)
            && self.current_row(view)? != row as i64
        {
            self.update_current_row(view, row)?;
        }
        if let Some(sort) = sort {
            let event = if sort.descending {
                "sort-descending"
            } else {
                "sort-ascending"
            };
            self.handler(view, event, &[Value::Int(sort.column as i64)])?;
        }
        Ok(true)
    }

    fn move_table_column(&self, view: &Element, from: usize, to: usize) -> Result<bool> {
        let state = self.repeat_state(view);
        let order = state.table_state().columns.get(Default::default);
        let Some((column, order)) = order.moved(view.table_columns().count(), from, to) else {
            return Ok(false);
        };
        state.table_state().columns.set(order);
        self.handler(
            view,
            "column-moved",
            &[Value::Int(column as i64), Value::Int(to as i64)],
        )?;
        Ok(true)
    }

    fn move_table_row(&self, view: &Element, from: usize, to: usize) -> Result<bool> {
        let state = self.repeat_state(view);
        let before = self.view_model(view)?;
        let Some(source) = before.source.as_ref() else {
            return Ok(false);
        };
        if view.table_definition().model.is_some()
            && self.table_flag(view, "auto-sort", false)?
            && state.table_state().sort.get(|| None).is_some()
        {
            return Ok(false);
        }
        let current = self.current_row(view)?;
        let current_source = index(current).and_then(|row| before.source_index(row));
        let Some(moved) = state.row_order.move_row(source, from, to) else {
            return Ok(false);
        };
        if let Some(index) =
            current_source.and_then(|row| state.row_order.positions(source).visual_index(row))
            && index as i64 != current
        {
            self.update_current_row(view, index)?;
        }
        self.handler(
            view,
            "row-moved",
            &[
                Value::Int(moved as i64),
                Value::Int(from as i64),
                Value::Int(to as i64),
            ],
        )?;
        Ok(true)
    }

    pub(crate) fn table_read(&self, member: &Member) -> Result<Value> {
        let (owner, view) = self.element_owner(&member.owner)?;
        let state = owner.repeat_state(view);
        let sort = || state.table_state().sort.get(|| None);
        match member.name.as_str() {
            "can-undo" | "can-redo" => Ok(Value::Bool(
                owner.can_replay_table(view, member.name == "can-redo")?,
            )),
            "current-sort-column" => Ok(Value::Int(sort().map_or(-1, |sort| sort.column as i64))),
            "current-sort-descending" => {
                Ok(Value::Bool(sort().is_some_and(|sort| sort.descending)))
            }
            name => {
                let model = owner.view_model(view)?.table_rows()?;
                let snapshot = owner.table_snapshot(view, &model, state.table_state())?;
                Ok(match name {
                    "current-column" => {
                        Value::Int(snapshot.current_column().map_or(-1, |column| column as i64))
                    }
                    "selection-count" => Value::Int(snapshot.len() as i64),
                    "has-selection" => Value::Bool(!snapshot.is_empty()),
                    _ => return Err(Error::Invalid(format!("unknown table metric {name}"))),
                })
            }
        }
    }

    pub(crate) fn table_command(
        &self,
        target: &str,
        command: Command,
        args: &[Value],
    ) -> Result<()> {
        let (owner, view) = self.element_owner(target)?;
        use Command::*;
        use TableSelectionAction as Action;
        use Value::Int;
        let index = |value: i64| usize::try_from(value).unwrap_or(usize::MAX);
        let action = match (command, args) {
            (Undo | Redo, []) => {
                owner.replay_table(view, matches!(command, Redo))?;
                return Ok(());
            }
            (SetHeight, [Int(row), Value::Length(height)]) => {
                let rows = owner.view_model(view)?.table_rows()?;
                owner.repeat_state(view).table_state().heights.set(
                    &rows,
                    index(*row),
                    None,
                    height.0,
                );
                return Ok(());
            }
            (MoveRow, [Int(from), Int(to)]) => {
                owner.move_table_row(view, index(*from), index(*to))?;
                return Ok(());
            }
            (MoveColumn, [Int(from), Int(to)]) => {
                owner.move_table_column(view, index(*from), index(*to))?;
                return Ok(());
            }
            (Sort, [Int(column), Value::Bool(descending)]) => {
                owner.sort_table(
                    view,
                    Some(TableSort {
                        column: index(*column),
                        descending: *descending,
                    }),
                )?;
                return Ok(());
            }
            (ClearSort, []) => {
                owner.sort_table(view, None)?;
                return Ok(());
            }
            (SelectAll, []) => Action::All,
            (ClearSelection, []) => Action::Clear,
            (SelectRow, [Int(row)]) => Action::Row {
                index: index(*row),
                operation: RowSelection::Replace,
            },
            (SelectColumn, [Int(column)]) => Action::Column {
                column: index(*column),
                operation: RowSelection::Replace,
            },
            (SelectCell | MoveCell | ToggleCell, [Int(row), Int(column)]) => Action::Cell {
                row: index(*row),
                column: index(*column),
                operation: match command {
                    MoveCell => RowSelection::Move,
                    ToggleCell => RowSelection::Toggle,
                    _ => RowSelection::Replace,
                },
            },
            (SelectRange, [Int(row), Int(column), Int(end_row), Int(end_column)]) => {
                Action::DragRange {
                    anchor: (index(*row), index(*column)),
                    end: (index(*end_row), index(*end_column)),
                    additive: false,
                }
            }
            _ => return Err(Error::Invalid("invalid table command operands".into())),
        };
        if args
            .iter()
            .any(|arg| matches!(arg, Int(value) if *value < 0))
        {
            return Ok(());
        }
        owner.select_table(view, action)?;
        Ok(())
    }

    pub(crate) fn copy_table(
        &self,
        view: &Element,
        node: &Node,
        columns: &[usize],
    ) -> Result<Option<String>> {
        if !node.boolean("copyable", false) {
            return Ok(None);
        }
        let model = self.view_model(view)?;
        let rows = model.table_rows()?;
        if view.table_definition().model.is_some() {
            let paths: Vec<_> = view
                .table_columns()
                .map(|column| column.column_field.as_deref())
                .collect();
            return Ok(xrack_runtime::copy_table_text_in_order(
                &rows,
                node,
                columns,
                |_, column, item| {
                    let Value::Record(record) = item else {
                        return None;
                    };
                    let path = paths.get(column).copied().flatten()?;
                    Some(field_text(record.field_path(path)))
                },
            ));
        }
        let template = view
            .view_template()
            .ok_or_else(|| Error::Invalid("missing table row template".into()))?;
        let source_model = model.source.as_ref().expect("table rows have a source");
        let mut sampled = None::<(usize, Instance)>;
        xrack_runtime::try_copy_table_text_in_order(&rows, node, columns, |source, column, item| {
            let Some(cell) = template.children.get(column) else {
                return Ok(None);
            };
            if sampled.as_ref().is_none_or(|(index, _)| *index != source) {
                sampled = Some((
                    source,
                    self.sample_table_row(
                        template,
                        source,
                        source_model.row_key(source),
                        item.clone(),
                    )?,
                ));
            }
            let row = &sampled.as_ref().expect("sampled row").1;
            if !row.shown_for_table(template)? || !row.shown_for_table(cell)? {
                return Ok(None);
            }
            if let Some(expression) = &cell.clipboard_text {
                let value = row.evaluate(expression, &[])?;
                return Ok(Some(field_text(Some(&value))));
            }
            if let Some(field) = cell.row_links.values().next() {
                let value = match item {
                    Value::Record(record) if !field.path.is_empty() => {
                        record.field_path(&field.path)
                    }
                    _ if field.path.is_empty() => Some(item),
                    _ => None,
                };
                return Ok(value.map(|value| field_text(Some(value))));
            }
            if !matches!(cell.visual, Visual::Builtin("Text" | "TableCell")) {
                return Ok(None);
            }
            if let Some(cases) = cell.state_bindings.get("text")
                && let Some(value) = row.state_value(cases)?
            {
                return Ok(Some(field_text(Some(&value))));
            }
            Ok(row
                .binding_value(cell, "text")?
                .map(|value| field_text(Some(&value))))
        })
    }

    pub(crate) fn table_control(
        &self,
        view: &Element,
        node: &Node,
        action: ControlAction,
    ) -> Result<bool> {
        let model = self.view_model(view)?;
        if node.standard_list_stamp != model.stamp() {
            return Ok(false);
        }
        let state = self.repeat_state(view);
        match action {
            ControlAction::ResizeRow { row, key, height }
                if node.boolean("resizable-rows", false) =>
            {
                Ok(state.table_state().heights.set(
                    &model.table_rows()?,
                    row,
                    Some(key),
                    f64::from(height),
                ))
            }
            ControlAction::ResizeColumn { column, width }
                if xrack_runtime::resizable_table_column(node, column).is_some() =>
            {
                let mut widths = state.table_state().widths.get(Vec::new);
                if widths.get(column) == Some(&Some(width)) {
                    return Ok(false);
                }
                widths.resize(widths.len().max(column + 1), None);
                widths[column] = Some(width);
                state.table_state().widths.set(widths);
                Ok(true)
            }
            ControlAction::MoveColumn { from, to }
                if node.boolean("movable-columns", false)
                    && node.table_column_order
                        == state.table_state().columns.get(Default::default) =>
            {
                self.move_table_column(view, from, to)
            }
            ControlAction::MoveRow {
                from,
                to,
                row,
                stamp,
            } if node.boolean("movable-rows", false) && model.stamp() == Some(stamp) => {
                let rows = model.table_rows()?;
                if from >= rows.row_count() || rows.row_key(from) != row {
                    return Ok(false);
                }
                self.move_table_row(view, from, to)
            }
            ControlAction::SortColumn(column) => {
                let Some(header) = &node.table_header else {
                    return Ok(false);
                };
                if header.sort != state.table_state().sort.get(|| None) {
                    return Ok(false);
                }
                let Some(sort) = header.next_sort(column) else {
                    return Ok(false);
                };
                self.sort_table(view, Some(sort))
            }
            ControlAction::Select(index) if node.table_selection.is_none() => {
                if index >= model.row_count() || ListMetrics::selection(node) == Some(index as i64)
                {
                    return Ok(false);
                }
                self.update_current_row(view, index)?;
                Ok(true)
            }
            ControlAction::Select(_) | ControlAction::TableSelection(_) => {
                let order = state.table_state().columns.get(Default::default);
                let Some(selection) = TableSelectionAction::from_control(node, &order, action)
                else {
                    return Ok(false);
                };
                self.select_table(view, selection)
            }
            _ => Ok(false),
        }
    }
}
