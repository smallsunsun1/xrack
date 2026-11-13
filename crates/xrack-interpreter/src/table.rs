use crate::{
    repeat::{Model, RepeatState, ViewModel},
    *,
};
use std::{cmp::Ordering, ops::Range};
use xrack_runtime::{
    ControlAction, ListMetrics, ListModel, Property, RecordValue, RowHeightOverrides, RowSelection,
    SelectionTarget, TableCellEditor, TableColumnOrder, TableColumnWindow, TableHeader, TableRows,
    TableSelection, TableSelectionAction, TableSelectionChange, TableSelectionCommand,
    TableSelectionOptions, TableSelectionSnapshot, TableSort, VecModel,
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
    fn table_owner(&self, id: &str) -> Result<(Self, &Element)> {
        let (owner, view) = self.element_owner(id)?;
        if view.table.is_none() {
            return Err(Error::Invalid(format!("{id} is not a table")));
        }
        Ok((owner, view))
    }

    /// Read the current selection of a table in this component.
    pub fn table_selection(&self, id: &str) -> Result<TableSelectionSnapshot> {
        let (owner, view) = self.table_owner(id)?;
        let rows = owner.view_model(view)?.table_rows()?;
        let state = owner.repeat_state(view);
        owner.table_snapshot(view, &rows, state.table_state())
    }

    /// Apply a host selection command using the same path as a DSL command.
    pub fn select_table(&mut self, id: &str, action: TableSelectionAction) -> Result<bool> {
        let (owner, view) = self.table_owner(id)?;
        owner.apply_table_selection(view, TableSelectionCommand::Visual(action))
    }

    /// Read the active sort indicator of a table in this component.
    pub fn table_sort(&self, id: &str) -> Result<Option<TableSort>> {
        let (owner, view) = self.table_owner(id)?;
        Ok(owner.repeat_state(view).table_state().sort.get(|| None))
    }

    /// Set or clear a table sort using the same path as a DSL command.
    pub fn set_table_sort(&mut self, id: &str, sort: Option<TableSort>) -> Result<bool> {
        let (owner, view) = self.table_owner(id)?;
        owner.sort_table(view, sort, None)
    }

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
        state.row_order.try_project_rows_by(
            || Rc::new(model.clone()),
            || {
                if definition.model.is_some() && self.table_flag(view, "auto-sort", false)? {
                    Ok(state.table_state().sort.get(|| None))
                } else {
                    Ok(None)
                }
            },
            move |column, left, right| {
                let Some(Some(path)) = columns.get(column) else {
                    return Ordering::Equal;
                };
                compare_fields(left.field_path(path), right.field_path(path))
            },
            definition.filter_field.clone().map(filter_text),
            || match self.binding_value(view, "filter-text")? {
                Some(Value::String(text)) => Ok(text),
                _ => Ok(String::new()),
            },
            view.visible_field.clone().map(|path| {
                move |row: &RecordValue| row.field_path(&path) == Some(&Value::Bool(true))
            }),
        )
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

    fn column_editor(&self, record: &str, path: &[String]) -> Result<TableCellEditor> {
        let mut ty = ValueType::Record(Rc::from(record));
        for name in path {
            let ValueType::Record(record) = &ty else {
                return Err(Error::Invalid(format!(
                    "invalid table field {}",
                    path.join(".")
                )));
            };
            ty = self
                .program()
                .records
                .get(record.as_ref())
                .and_then(|fields| fields.iter().find(|field| field.name == *name))
                .ok_or_else(|| Error::Invalid(format!("unknown table field {}", path.join("."))))?
                .ty
                .clone();
        }
        match ty {
            ValueType::String => Ok(TableCellEditor::String),
            ValueType::Int => Ok(TableCellEditor::Int),
            ValueType::Float => Ok(TableCellEditor::Float),
            ValueType::Bool => Ok(TableCellEditor::Bool),
            ValueType::Date => Ok(TableCellEditor::Date),
            ValueType::Time => Ok(TableCellEditor::Time),
            _ => Err(Error::Invalid(format!(
                "unsupported editable table field {}",
                path.join(".")
            ))),
        }
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
        let Some(Model::Records(records)) = &model.source else {
            return Err(Error::Invalid("standard table needs records".into()));
        };
        let columns = view
            .table_columns()
            .map(|column| {
                let path = column
                    .column_field
                    .as_deref()
                    .ok_or_else(|| Error::Invalid("missing column field".into()))?;
                Ok((column, path))
            })
            .collect::<Result<Vec<_>>>()?;
        let header = view.table_definition().row_header.as_deref();
        xrack_runtime::try_project_standard_table_rows(
            rows,
            range,
            window,
            columns.len(),
            |record| header.map(|path| field_text(record.field_path(path))),
            |record, column| field_text(record.field_path(columns[column].1)),
            |index| {
                let (column, path) = columns[index];
                if !self.table_flag(column, "editable", false)? {
                    return Ok(None);
                }
                self.column_editor(records.name(), path).map(Some)
            },
        )
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

    fn table_selection_target(&self, view: &Element) -> Result<SelectionTarget> {
        Ok(match self.binding_value(view, "selection-target")? {
            Some(Value::SelectionTarget(target)) => target,
            _ => SelectionTarget::Rows,
        })
    }

    fn table_snapshot(
        &self,
        view: &Element,
        model: &TableRows<Model>,
        state: &State,
    ) -> Result<TableSelectionSnapshot> {
        let target = self.table_selection_target(view)?;
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

    fn apply_table_selection(
        &self,
        view: &Element,
        command: TableSelectionCommand,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        let state = state.table_state();
        let Some(change) = state.selection.try_apply_command(
            command,
            view.table_columns().count(),
            || self.view_model(view)?.table_rows(),
            || self.current_row(view),
            || {
                Ok(TableSelectionOptions {
                    target: self.table_selection_target(view)?,
                    multiple: self.table_flag(view, "multi-select", false)?,
                    order: state.column_order(),
                    drag_select: false,
                })
            },
        )?
        else {
            return Ok(false);
        };
        self.notify_table_selection(view, state, change, true)
    }

    fn notify_table_selection(
        &self,
        view: &Element,
        state: &State,
        change: TableSelectionChange,
        notify_cell: bool,
    ) -> Result<bool> {
        if let Some(current) = change.current {
            self.update_current_row(view, current)?;
        }
        if notify_cell && (change.current.is_some() || change.column_changed) {
            let column = self
                .table_snapshot(view, &self.view_model(view)?.table_rows()?, state)?
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
        let column = match column {
            Some(column) => {
                let Some(column) = index(column) else {
                    return Ok(());
                };
                Some(column)
            }
            None => None,
        };
        self.apply_table_selection(view, TableSelectionCommand::Source { row, column })?;
        Ok(())
    }

    fn sort_table(
        &self,
        view: &Element,
        sort: Option<TableSort>,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        let Some(cursor) = state.row_order.try_sort_rows(
            &state.table_state().sort,
            sort,
            input,
            |column| match view.table_columns().nth(column) {
                Some(column) => self.table_flag(column, "sortable", true),
                None => Ok(false),
            },
            || {
                Ok(view.table_definition().model.is_some()
                    && self.table_flag(view, "auto-sort", false)?)
            },
            || self.view_model(view)?.table_rows(),
            || self.current_row(view),
        )?
        else {
            return Ok(false);
        };
        if let Some(row) = cursor {
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

    fn move_table_row(
        &self,
        view: &Element,
        from: i64,
        to: i64,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        if view.table_definition().model.is_some()
            && self.table_flag(view, "auto-sort", false)?
            && state.table_state().sort.get(|| None).is_some()
        {
            return Ok(false);
        }
        let Some((moved, cursor)) = state.row_order.try_move_visible_row(
            from,
            to,
            input,
            || self.view_model(view)?.table_rows(),
            || self.current_row(view),
        )?
        else {
            return Ok(false);
        };
        if let Some(row) = cursor {
            self.update_current_row(view, row)?;
        }
        self.handler(
            view,
            "row-moved",
            &[Value::Int(moved as i64), Value::Int(from), Value::Int(to)],
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
                owner.move_table_row(view, *from, *to, None)?;
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
                    None,
                )?;
                return Ok(());
            }
            (ClearSort, []) => {
                owner.sort_table(view, None, None)?;
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
        owner.apply_table_selection(view, TableSelectionCommand::Visual(action))?;
        Ok(())
    }

    pub(crate) fn copy_table(
        &self,
        view: &Element,
        node: &Node,
        columns: &[usize],
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<Option<String>> {
        if !node.boolean("copyable", false) {
            return Ok(None);
        }
        let rows = || {
            self.table_action_view_model(view, "copyable", &node.table_column_order)?
                .map(|model| model.table_rows())
                .transpose()
        };
        if view.table_definition().model.is_some() {
            let paths: Vec<_> = view
                .table_columns()
                .map(|column| column.column_field.as_deref())
                .collect();
            return xrack_runtime::try_copy_table_text_while_current(
                node,
                columns,
                || input.is_current(),
                rows,
                |_, column, _, item| {
                    let Value::Record(record) = item else {
                        return Ok(None);
                    };
                    Ok(paths
                        .get(column)
                        .copied()
                        .flatten()
                        .map(|path| field_text(record.field_path(path))))
                },
            );
        }
        let template = view
            .view_template()
            .ok_or_else(|| Error::Invalid("missing table row template".into()))?;
        let mut sampled = None::<(usize, Instance)>;
        xrack_runtime::try_copy_table_text_while_current(
            node,
            columns,
            || input.is_current(),
            rows,
            |source, column, key, item| {
                let Some(cell) = template.children.get(column) else {
                    return Ok(None);
                };
                if sampled.as_ref().is_none_or(|(index, _)| *index != source) {
                    sampled = Some((
                        source,
                        self.sample_table_row(template, source, key, item.clone())?,
                    ));
                }
                let row = &sampled.as_ref().expect("sampled row").1;
                if !row.element_shown(template)? || !row.element_shown(cell)? {
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
            },
        )
    }

    pub(crate) fn table_control(
        &self,
        view: &Element,
        node: &Node,
        action: ControlAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        if matches!(
            action,
            ControlAction::Select(_) | ControlAction::TableSelection(_)
        ) {
            let state = state.table_state();
            let change = state.selection.try_apply_control(
                node,
                input,
                || self.element_allows_editing(view),
                || self.view_model(view)?.table_rows(),
                || {
                    Ok(TableSelectionOptions {
                        target: self.table_selection_target(view)?,
                        multiple: self.table_flag(view, "multi-select", false)?,
                        order: state.column_order(),
                        drag_select: matches!(
                            action,
                            ControlAction::TableSelection(TableSelectionAction::DragRange { .. })
                        ) && self.table_flag(view, "drag-select", false)?,
                    })
                },
                || self.current_row(view),
            )?;
            return match change {
                Some(change) => {
                    self.notify_table_selection(view, state, change, node.table_selection.is_some())
                }
                None => Ok(false),
            };
        }
        if !xrack_runtime::try_table_drag_available(
            node,
            input,
            |name| self.table_flag(view, name, false),
            |column| match view.table_columns().nth(column) {
                Some(column) => self.table_flag(column, "resizable", true),
                None => Ok(false),
            },
        )? {
            return Ok(false);
        }
        let model = self.view_model(view)?;
        if !input.is_current() || node.standard_list_stamp != model.stamp() {
            return Ok(false);
        }
        match action {
            ControlAction::ResizeRow { row, key, height } => Ok(state.table_state().heights.set(
                &model.table_rows()?,
                row,
                Some(key),
                f64::from(height),
            )),
            ControlAction::ResizeColumn { column, width } => {
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
                if node.table_column_order == state.table_state().columns.get(Default::default) =>
            {
                self.move_table_column(view, from, to)
            }
            ControlAction::MoveRow {
                from,
                to,
                row,
                stamp,
            } if model.stamp() == Some(stamp) => {
                let rows = model.table_rows()?;
                if from >= rows.row_count() || rows.row_key(from) != row {
                    return Ok(false);
                }
                self.move_table_row(view, from as i64, to as i64, Some(input))
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
                self.sort_table(view, Some(sort), Some(input))
            }
            _ => Ok(false),
        }
    }
}
