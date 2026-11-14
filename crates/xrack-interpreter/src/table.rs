use crate::{
    repeat::{Model, RepeatState, ViewModel},
    *,
};
use std::{cmp::Ordering, ops::Range};
use xrack_mlir::TableRead;
use xrack_runtime::{
    ControlAction, ListMetrics, Property, RecordValue, RowHeightOverrides, SelectionTarget,
    TableCellEditor, TableColumnOrder, TableColumnWindow, TableField, TableHeader,
    TableHeaderAction, TableRows, TableSelection, TableSelectionAction, TableSelectionChange,
    TableSelectionCommand, TableSelectionMethod, TableSelectionOptions, TableSelectionSnapshot,
    TableSort, VecModel,
};

pub(crate) struct Definition {
    pub(crate) model: Option<Expr>,
    pub(crate) columns: Rc<[StandardColumn]>,
    pub(crate) history: bool,
    pub(crate) paste: bool,
    pub(crate) filter_field: Option<Rc<[String]>>,
    pub(crate) row_header: Option<Rc<[String]>>,
}

pub(crate) struct StandardColumn {
    pub(crate) path: Box<[String]>,
    pub(crate) editor: Option<(TableCellEditor, Expr)>,
}

pub(crate) enum ClipboardText {
    Expression(Expr),
    Text(Option<Expr>),
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
    Select(TableSelectionMethod),
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
            _ => Self::Select(TableSelectionMethod::parse(method)?),
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
        Some(Value::String(value)) => value.cell_text(),
        Some(Value::Int(value)) => value.cell_text(),
        Some(Value::Float(value)) => value.cell_text(),
        Some(Value::Bool(value)) => value.cell_text(),
        Some(Value::Time(value)) => value.cell_text(),
        Some(Value::Date(value)) => value.cell_text(),
        Some(Value::CheckState(value)) => format!("{value:?}"),
        _ => String::new(),
    }
}

pub(crate) fn matches_field_text(value: Option<&Value>, text: &str) -> bool {
    match value {
        Some(Value::String(value)) => value.matches_cell_text(text),
        Some(Value::Int(value)) => value.matches_cell_text(text),
        Some(Value::Float(value)) => value.matches_cell_text(text),
        Some(Value::Bool(value)) => value.matches_cell_text(text),
        Some(Value::Time(value)) => value.matches_cell_text(text),
        Some(Value::Date(value)) => value.matches_cell_text(text),
        _ => false,
    }
}

fn compare_fields(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (left, right) {
        (Some(Value::String(a)), Some(Value::String(b))) => a.compare_cells(b),
        (Some(Value::Int(a)), Some(Value::Int(b))) => a.compare_cells(b),
        (Some(Value::Float(a)), Some(Value::Float(b))) => a.compare_cells(b),
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a.compare_cells(b),
        (Some(Value::Date(a)), Some(Value::Date(b))) => a.compare_cells(b),
        (Some(Value::Time(a)), Some(Value::Time(b))) => a.compare_cells(b),
        _ => Ordering::Equal,
    }
}

fn filter_text(path: Rc<[String]>) -> impl Fn(&RecordValue) -> &str + Clone {
    move |row| match row.field_path(&path) {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    }
}

fn index(value: i64) -> Option<usize> {
    usize::try_from(value).ok()
}

impl Instance {
    /// Read the current selection of a table in this component.
    pub fn table_selection(&self, id: &str) -> Result<TableSelectionSnapshot> {
        let view = self.host_element(id, |view| view.table.is_some())?;
        let rows = self.view_model(view)?.table_rows()?;
        let state = self.repeat_state(view);
        self.table_snapshot(view, &rows, state.table_state())
    }

    /// Apply a host selection command using the same path as a DSL command.
    pub fn select_table(&mut self, id: &str, action: TableSelectionAction) -> Result<bool> {
        let view = self.host_element(id, |view| view.table.is_some())?;
        self.apply_table_selection(view, TableSelectionCommand::Visual(action))
    }

    /// Read the active sort indicator of a table in this component.
    pub fn table_sort(&self, id: &str) -> Result<Option<TableSort>> {
        let view = self.host_element(id, |view| view.table.is_some())?;
        Ok(self.repeat_state(view).table_state().sort.get(|| None))
    }

    /// Set or clear a table sort using the same path as a DSL command.
    pub fn set_table_sort(&mut self, id: &str, sort: Option<TableSort>) -> Result<bool> {
        let view = self.host_element(id, |view| view.table.is_some())?;
        self.sort_table(view, sort, None)
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
        let columns = definition.columns.clone();
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
                let Some(column) = columns.get(column) else {
                    return Ordering::Equal;
                };
                compare_fields(
                    left.field_path(&column.path),
                    right.field_path(&column.path),
                )
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
        state.table_state().selection.populate_scene(node, &rows);
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
        let definition = view.table_definition();
        let columns = &definition.columns;
        let header = definition.row_header.as_deref();
        xrack_runtime::try_project_standard_table_rows(
            rows,
            range,
            window,
            columns.len(),
            |record| header.map(|path| field_text(record.field_path(path))),
            |record, column| field_text(record.field_path(&columns[column].path)),
            |index| self.standard_column_editor(&columns[index]),
        )
    }

    pub(crate) fn standard_column_editor(
        &self,
        column: &StandardColumn,
    ) -> Result<Option<TableCellEditor>> {
        let Some((editor, enabled)) = &column.editor else {
            return Ok(None);
        };
        Ok((self.evaluate(enabled, &[])? == Value::Bool(true)).then_some(*editor))
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
        state.selection.try_snapshot_with(
            model,
            view.table_columns().count(),
            || self.current_row(view),
            || self.table_selection_target(view),
            || self.table_flag(view, "multi-select", false),
            || state.column_order(),
        )
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
        use xrack_runtime::TableSelectionEvent;

        let notify_cell = notify_cell && view.handlers.contains_key("current-cell-changed");
        change.try_notify(notify_cell, |event| match event {
            TableSelectionEvent::CurrentRowChanged(row) => self.update_current_row(view, row),
            TableSelectionEvent::CurrentCellChanged => {
                let column = self
                    .table_snapshot(view, &self.view_model(view)?.table_rows()?, state)?
                    .current_column_index();
                self.handler(
                    view,
                    "current-cell-changed",
                    &[Value::Int(self.current_row(view)?), Value::Int(column)],
                )
                .map(|_| ())
            }
            TableSelectionEvent::SelectionChanged(count) => self
                .handler(view, "selection-changed", &[Value::Int(count as i64)])
                .map(|_| ()),
        })
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

    pub(crate) fn table_read(&self, key: &str, read: TableRead) -> Result<Value> {
        let (owner, view) = self.element_owner(key)?;
        let state = owner.repeat_state(view);
        let sort = || state.table_state().sort.get(|| None);
        let snapshot = || {
            let model = owner.view_model(view)?.table_rows()?;
            owner.table_snapshot(view, &model, state.table_state())
        };
        Ok(match read {
            TableRead::History { redo } => owner.can_replay_table(view, redo)?.into(),
            TableRead::SortColumn => TableSort::column_index(sort()).into(),
            TableRead::SortDescending => TableSort::is_descending(sort()).into(),
            TableRead::CurrentColumn => snapshot()?.current_column_index().into(),
            TableRead::SelectionCount => snapshot()?.selection_count().into(),
            TableRead::HasSelection => (!snapshot()?.is_empty()).into(),
        })
    }

    pub(crate) fn table_command(
        &self,
        target: &str,
        command: Command,
        args: &[Value],
    ) -> Result<()> {
        let (owner, view) = self.element_owner(target)?;
        use Command::*;
        use Value::Int;
        match (command, args) {
            (Undo | Redo, []) => {
                owner.replay_table(view, matches!(command, Redo))?;
                return Ok(());
            }
            (SetHeight, [Int(row), Value::Length(height)]) => {
                owner.repeat_state(view).table_state().heights.try_set_row(
                    *row,
                    *height,
                    || owner.view_model(view)?.table_rows(),
                )?;
                return Ok(());
            }
            (MoveRow, [Int(from), Int(to)]) => {
                owner.move_table_row(view, *from, *to, None)?;
                return Ok(());
            }
            (MoveColumn, [Int(from), Int(to)]) => {
                if let (Some(from), Some(to)) = (index(*from), index(*to)) {
                    owner.move_table_column(view, from, to)?;
                }
                return Ok(());
            }
            (Sort, [Int(column), Value::Bool(descending)]) => {
                if let Some(sort) = TableSort::from_index(*column, *descending) {
                    owner.sort_table(view, Some(sort), None)?;
                }
                return Ok(());
            }
            (ClearSort, []) => {
                owner.sort_table(view, None, None)?;
                return Ok(());
            }
            (Select(method), args) => {
                let indices = args
                    .iter()
                    .map(|arg| match arg {
                        Int(value) => Ok(*value),
                        _ => Err(Error::Invalid("invalid table selection operand".into())),
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(action) = method.action(&indices) {
                    owner.apply_table_selection(view, TableSelectionCommand::Visual(action))?;
                }
            }
            _ => return Err(Error::Invalid("invalid table command operands".into())),
        }
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
            self.table_action_view_model(view, "copyable", &node.table_column_order, input)?
                .map(|model| model.table_rows())
                .transpose()
        };
        if view.table_definition().model.is_some() {
            let fields = &view.table_definition().columns;
            return xrack_runtime::try_copy_table_text_while_current(
                node,
                columns,
                || input.is_current(),
                rows,
                |_, column, _, item| {
                    let Value::Record(record) = item else {
                        return Ok(None);
                    };
                    Ok(fields
                        .get(column)
                        .map(|field| field_text(record.field_path(&field.path))))
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
                let Some(text) = &cell.clipboard_text else {
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
                let value = match text {
                    ClipboardText::Expression(expression) => Some(row.evaluate(expression, &[])?),
                    ClipboardText::Text(base) => {
                        if let Some(cases) = cell.state_bindings.get("text")
                            && let Some(value) = row.state_value(cases)?
                        {
                            Some(value)
                        } else {
                            base.as_ref()
                                .map(|base| row.evaluate(base, &[]))
                                .transpose()?
                        }
                    }
                };
                Ok(Some(field_text(value.as_ref())))
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
                || self.element_allows_routed_input(view, input),
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
        if let ControlAction::MoveRow { from, to, .. } = action {
            return self.move_table_row(view, from as i64, to as i64, Some(input));
        }
        let state = state.table_state();
        let Some((action, rows)) = TableHeaderAction::try_prepare(
            node,
            input,
            action,
            || state.sort.get(|| None),
            || state.column_order(),
            || self.view_model(view)?.table_rows(),
        )?
        else {
            return Ok(false);
        };
        match action {
            TableHeaderAction::ResizeRow { row, key, height } => {
                Ok(state.heights.set(&rows, row, Some(key), f64::from(height)))
            }
            TableHeaderAction::ResizeColumn { column, width } => Ok(
                xrack_runtime::resize_table_column(&state.widths, column, width),
            ),
            TableHeaderAction::MoveColumn { from, to } => self.move_table_column(view, from, to),
            TableHeaderAction::Sort(sort) => self.sort_table(view, Some(sort), Some(input)),
        }
    }
}
