use crate::{
    repeat::{Model, RowScope, ViewModel},
    table, *,
};
use xrack_runtime::{
    Property, RecordValue, RowContext, RowKey, TableCellEditor, TableColumnOrder, TableEditHistory,
    TableEditPosition, TablePaste, TablePasteInput, TableRowPatch, TableRows, VecModel,
};

pub(crate) struct ClipboardParser {
    pub(crate) target: String,
    pub(crate) expression: Expr,
}

enum PasteColumn<'a> {
    Field {
        column: &'a Element,
        path: &'a [String],
    },
    Row {
        field: &'a RowField,
        guards: Vec<&'a Element>,
        parser: Option<&'a Expr>,
    },
}

impl<'a> PasteColumn<'a> {
    fn for_row(cell: &'a Element) -> Option<Self> {
        let mut guards = Vec::new();
        if let Some((field, parser)) = cell.clipboard_binding(&mut guards) {
            return Some(Self::Row {
                field,
                guards,
                parser: Some(parser),
            });
        }
        let field = cell.row_links.values().next()?;
        Some(Self::Row {
            field,
            guards: vec![cell],
            parser: None,
        })
    }

    fn path(&self) -> &[String] {
        match self {
            Self::Field { path, .. } => path,
            Self::Row { field, .. } => &field.path,
        }
    }
}

impl Element {
    fn clipboard_binding<'a>(
        &'a self,
        guards: &mut Vec<&'a Element>,
    ) -> Option<(&'a RowField, &'a Expr)> {
        guards.push(self);
        if let (Some(parser), Visual::Instance { inputs, .. }) =
            (&self.clipboard_parser, &self.visual)
            && let Some(InputSource::RowLink(field)) = inputs
                .get(&parser.target)
                .and_then(|input| input.source.as_ref())
        {
            return Some((field, &parser.expression));
        }
        if let Some(binding) = self
            .children
            .iter()
            .find_map(|child| child.clipboard_binding(guards))
        {
            return Some(binding);
        }
        guards.pop();
        None
    }

    pub(crate) fn table_for_template(&self, template: &str) -> Option<&Self> {
        if self.table.is_some() && self.view_template().is_some_and(|row| row.key == template) {
            return Some(self);
        }
        self.children
            .iter()
            .find_map(|child| child.table_for_template(template))
    }

    fn paste_configured(&self) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.name == "pasteable")
            || self.state_bindings.contains_key("pasteable")
    }
}

struct PendingPaste {
    model_stamp: (u64, u64),
    order_stamp: (u64, u64),
    column_order: TableColumnOrder,
    row: usize,
    columns: Vec<usize>,
}

fn capture(before: &RecordValue, after: &RecordValue, patch: &mut TableRowPatch<RecordValue>) {
    patch.record_changes(before, after);
}

pub(crate) fn capture_string(before: &String, after: &String, patch: &mut TableRowPatch<String>) {
    patch.record(
        before,
        after,
        |value| value,
        |left, right| left == right,
        String::len,
    );
}

fn replace_field(record: &RecordValue, path: &[String], value: Value) -> Option<RecordValue> {
    if path.is_empty() {
        match value {
            Value::Record(value) if value.name() == record.name() => Some(value),
            _ => None,
        }
    } else {
        record.with_field(path, value)
    }
}

fn string_rows(model: &ViewModel) -> Option<TableRows<VecModel<String>>> {
    let Model::Strings(strings) = model.source.as_ref()? else {
        return None;
    };
    Some(TableRows::from_positions(
        Rc::new(strings.clone()),
        model.positions()?.clone(),
    ))
}

fn edit_stamp(model: &ViewModel) -> Option<(u64, u64)> {
    match model.source.as_ref()? {
        Model::Strings(strings) => Some(strings.edit_stamp()),
        Model::Records(records) => Some(records.model().edit_stamp()),
        _ => None,
    }
}

trait EditableRow: Clone + PartialEq + 'static {
    fn row_value(&self) -> Value;
    fn field_value(&self, path: &[String]) -> Option<Value>;
    fn replace_field(&self, path: &[String], value: Value) -> Option<Self>;
    fn capture(before: &Self, after: &Self, patch: &mut TableRowPatch<Self>);
}

impl EditableRow for RecordValue {
    fn row_value(&self) -> Value {
        Value::Record(self.clone())
    }
    fn field_value(&self, path: &[String]) -> Option<Value> {
        self.field_path(path).cloned()
    }
    fn replace_field(&self, path: &[String], value: Value) -> Option<Self> {
        replace_field(self, path, value)
    }
    fn capture(before: &Self, after: &Self, patch: &mut TableRowPatch<Self>) {
        capture(before, after, patch);
    }
}

impl EditableRow for String {
    fn row_value(&self) -> Value {
        Value::String(self.clone())
    }
    fn field_value(&self, path: &[String]) -> Option<Value> {
        path.is_empty().then(|| self.row_value())
    }
    fn replace_field(&self, path: &[String], value: Value) -> Option<Self> {
        match value {
            Value::String(value) if path.is_empty() => Some(value),
            _ => None,
        }
    }
    fn capture(before: &Self, after: &Self, patch: &mut TableRowPatch<Self>) {
        capture_string(before, after, patch);
    }
}

impl Instance {
    fn visual_flag(&self, element: &Element, name: &str, default: bool) -> Result<bool> {
        if let Some(cases) = element.state_bindings.get(name)
            && let Some(Value::Bool(value)) = self.state_value(cases)?
        {
            return Ok(value);
        }
        self.table_flag(element, name, default)
    }

    pub(crate) fn shown_for_table(&self, element: &Element) -> Result<bool> {
        for condition in &element.conditions {
            if self.evaluate(condition, &[])? == Value::Bool(false) {
                return Ok(false);
            }
        }
        self.visual_flag(element, "visible", true)
    }

    fn editable_for_table(&self, element: &Element) -> Result<bool> {
        Ok(self.shown_for_table(element)?
            && self.visual_flag(element, "enabled", true)?
            && !self.visual_flag(element, "read-only", false)?)
    }

    pub(crate) fn table_history_enabled(&self, view: &Element) -> Result<bool> {
        Ok(self.visual_flag(view, "undoable", false)?
            || self.visual_flag(view, "pasteable", false)?)
    }

    pub(crate) fn can_replay_table(&self, view: &Element, redo: bool) -> Result<bool> {
        if !self.table_history_enabled(view)? {
            return Ok(false);
        }
        let model = self.view_model(view)?;
        let state = self.repeat_state(view);
        let table = state.table_state();
        Ok(match &model.source {
            Some(Model::Records(records)) => table.history.can_replay(records.model(), redo),
            Some(Model::Strings(strings)) => table.string_history.can_replay(strings, redo),
            _ => false,
        })
    }

    pub(crate) fn replay_table(&self, view: &Element, redo: bool) -> Result<bool> {
        if !self.table_history_enabled(view)? {
            return Ok(false);
        }
        let model = self.view_model(view)?;
        let state = self.repeat_state(view);
        let table = state.table_state();
        Ok(match &model.source {
            Some(Model::Records(records)) => table.history.replay(records.model(), redo),
            Some(Model::Strings(strings)) => table.string_history.replay(strings, redo),
            _ => false,
        })
    }

    pub(crate) fn commit_projected_cell(
        &self,
        view: &Element,
        table: &Node,
        slot: usize,
        column: usize,
        original: &str,
        text: &str,
    ) -> Result<bool> {
        let Some(definition) = view.table_columns().nth(column) else {
            return Ok(false);
        };
        if !self.table_flag(definition, "editable", false)? {
            return Ok(false);
        }
        let model = self.view_model(view)?;
        if model.stamp() != table.standard_list_stamp {
            return Ok(false);
        }
        let Some(rows) = &model.projected else {
            return Ok(false);
        };
        let Some(path) = &definition.column_field else {
            return Ok(false);
        };
        let Some(source) = table
            .list
            .and_then(|list| list.first_row.checked_add(slot))
            .and_then(|row| model.source_index(row))
        else {
            return Ok(false);
        };
        let Some(key) = table.child_at(slot).and_then(|row| row.row_key) else {
            return Ok(false);
        };
        let update = |record: &RecordValue| {
            let value = record.field_path(path)?;
            if table::field_text(Some(value)) != original {
                return None;
            }
            let value = table::editor(value)?.parse(text)?;
            record.with_field(path, value)
        };
        let state = self.repeat_state(view);
        let history = &state.table_state().history;
        let model = rows.source();
        if self.table_history_enabled(view)? {
            Ok(history.commit_cell(model, source, key, update, capture))
        } else {
            let changed = model.update_row_if_key(source, key, update);
            if changed {
                history.clear();
            }
            Ok(changed)
        }
    }

    pub(crate) fn capture_table_paste(
        &self,
        scene: &Node,
        path: &[usize],
        row: usize,
        columns: Vec<usize>,
    ) -> Result<Option<TablePaste>> {
        let Some(node) = scene
            .interactive_at_path(path)
            .filter(|node| node.boolean("pasteable", false))
        else {
            return Ok(None);
        };
        let Some((owner, view)) = self.try_target(scene, path)? else {
            return Ok(None);
        };
        if view.table.is_none() || !view.paste_configured() {
            return Ok(None);
        }
        let has_editor = if view.table_definition().model.is_some() {
            view.table_columns().any(|column| {
                column
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "editable")
            })
        } else {
            view.view_template().is_some_and(|row| {
                row.children
                    .iter()
                    .any(|cell| PasteColumn::for_row(cell).is_some())
            })
        };
        if !has_editor {
            return Ok(None);
        }
        let model = owner.view_model(view)?;
        let (Some(model_stamp), Some(order_stamp)) = (edit_stamp(&model), model.stamp()) else {
            return Ok(None);
        };
        if node.standard_list_stamp != Some(order_stamp) {
            return Ok(None);
        }
        let paste = PendingPaste {
            model_stamp,
            order_stamp,
            column_order: node.table_column_order.clone(),
            row,
            columns,
        };
        let root = Rc::downgrade(&self.0);
        let scene = scene.clone();
        let path = path.to_vec();
        Ok(Some(Box::new(move |text| {
            let Some(root) = root.upgrade().map(Instance) else {
                return false;
            };
            let Some((owner, view)) = root.target(&scene, &path) else {
                return false;
            };
            root.handle_result(owner.apply_table_paste(view, &paste, text))
        })))
    }

    pub(crate) fn sample_table_row(
        &self,
        template: &Element,
        index: usize,
        key: RowKey,
        item: Value,
    ) -> Result<Self> {
        let repeat = template.repeat.as_ref().expect("table row repeat");
        self.allocate_row(
            template,
            RowScope {
                owner: Rc::downgrade(&self.0),
                root_key: template.key.clone(),
                item: repeat.item.clone(),
                index: repeat.index.clone(),
                context: RowContext::with_sample(key, index, item),
            },
        )
    }

    fn apply_table_paste(&self, view: &Element, paste: &PendingPaste, text: &str) -> Result<bool> {
        let state = self.repeat_state(view);
        let table = state.table_state();
        let target = Property::<Result<Option<ViewModel>>>::default();
        let read_target = || {
            target.get(|| {
                if !self.visual_flag(view, "pasteable", false)?
                    || !self.visual_flag(view, "enabled", true)?
                    || !self.shown_for_table(view)?
                    || table.column_order() != paste.column_order
                {
                    return Ok(None);
                }
                let model = self.view_model(view)?;
                let matches = model.stamp() == Some(paste.order_stamp)
                    && edit_stamp(&model) == Some(paste.model_stamp);
                Ok(matches.then_some(model))
            })
        };
        let Some(model) = read_target()? else {
            return Ok(false);
        };
        let still_current = || {
            if target.is_dirty() {
                read_target().map(|model| model.is_some())
            } else {
                Ok(true)
            }
        };
        let input = TablePasteInput {
            stamp: paste.model_stamp,
            row: paste.row,
            columns: &paste.columns,
            text,
        };
        if let Some(rows) = &model.projected {
            self.paste_rows(view, rows, &table.history, input, still_current)
        } else if let Some(rows) = string_rows(&model) {
            self.paste_rows(view, &rows, &table.string_history, input, still_current)
        } else {
            Ok(false)
        }
    }

    fn paste_rows<T: EditableRow>(
        &self,
        view: &Element,
        rows: &TableRows<VecModel<T>>,
        history: &TableEditHistory<T>,
        input: TablePasteInput<'_>,
        still_current: impl FnMut() -> Result<bool>,
    ) -> Result<bool> {
        let template = view.view_template();
        let definitions: Vec<_> = if view.table_definition().model.is_some() {
            view.table_columns()
                .map(|column| {
                    column
                        .column_field
                        .as_deref()
                        .map(|path| PasteColumn::Field { column, path })
                })
                .collect()
        } else {
            let Some(template) = template else {
                return Ok(false);
            };
            template.children.iter().map(PasteColumn::for_row).collect()
        };
        let mut sample = None::<(usize, Self)>;
        let changed = history.try_paste_in_order(
            rows,
            input,
            still_current,
            |row, column, current, next, text| {
                let Some(definition) = definitions.get(column).and_then(Option::as_ref) else {
                    return Ok(None);
                };
                let value = match definition {
                    PasteColumn::Field { column, path } => {
                        if !self.table_flag(column, "editable", false)? {
                            return Ok(None);
                        }
                        current
                            .field_value(path)
                            .and_then(|value| table::editor(&value))
                            .and_then(|editor| editor.parse(text))
                    }
                    PasteColumn::Row { .. } => {
                        let Some(template) = template else {
                            return Ok(None);
                        };
                        if sample.as_ref().is_none_or(|(index, _)| *index != row) {
                            let Some(key) = rows.source().row_key(row) else {
                                return Ok(None);
                            };
                            sample = Some((
                                row,
                                self.sample_table_row(template, row, key, current.row_value())?,
                            ));
                        }
                        let (_, sample) = sample.as_ref().expect("sampled row");
                        sample.parse_table_row(
                            template,
                            definition,
                            current.field_value(definition.path()),
                            text,
                        )?
                    }
                };
                let Some(updated) =
                    value.and_then(|value| next.replace_field(definition.path(), value))
                else {
                    return Ok(None);
                };
                *next = updated;
                Ok(Some(()))
            },
            T::capture,
        )?;
        Ok(changed.unwrap_or(false))
    }

    fn parse_table_row(
        &self,
        row: &Element,
        column: &PasteColumn<'_>,
        current: Option<Value>,
        text: &str,
    ) -> Result<Option<Value>> {
        let PasteColumn::Row { guards, parser, .. } = column else {
            return Ok(None);
        };
        if !self.shown_for_table(row)? || !self.visual_flag(row, "enabled", true)? {
            return Ok(None);
        }
        for element in guards {
            if !self.editable_for_table(element)? {
                return Ok(None);
            }
        }
        if let Some(parser) = parser {
            return Ok(
                match self.evaluate(parser, &[Value::String(text.into())])? {
                    Value::Record(result)
                        if result.field("accepted") == Some(&Value::Bool(true)) =>
                    {
                        result.field("value").cloned()
                    }
                    _ => None,
                },
            );
        }
        let cell = guards[0];
        if matches!(cell.visual, Visual::Builtin("SpinBox" | "DoubleSpinBox"))
            && !self.visual_flag(cell, "editable", true)?
        {
            return Ok(None);
        }
        let value = match current {
            Some(Value::String(_)) => Some(Value::String(text.to_owned())),
            Some(Value::Bool(_)) => xrack_runtime::table_paste_bool(text).map(Value::Bool),
            Some(Value::CheckState(_)) => xrack_runtime::table_paste_check_state(
                text,
                self.table_flag(cell, "tristate", false)?,
            )
            .map(Value::CheckState),
            Some(Value::Int(_)) => {
                let Some(Value::Int(value)) = TableCellEditor::Int.parse(text) else {
                    return Ok(None);
                };
                let min = match self.binding_value(cell, "minimum")? {
                    Some(Value::Int(value)) => value,
                    _ => 0,
                };
                let max = match self.binding_value(cell, "maximum")? {
                    Some(Value::Int(value)) => value,
                    _ => 100,
                };
                (min..=max.max(min))
                    .contains(&value)
                    .then_some(Value::Int(value))
            }
            Some(Value::Float(_)) => {
                let Some(Value::Float(value)) = TableCellEditor::Float.parse(text) else {
                    return Ok(None);
                };
                let min = match self.binding_value(cell, "minimum")? {
                    Some(Value::Float(value)) => value,
                    _ => 0.0,
                };
                let max = match self.binding_value(cell, "maximum")? {
                    Some(Value::Float(value)) => value,
                    _ => 100.0,
                };
                (min..=max.max(min))
                    .contains(&value)
                    .then_some(Value::Float(value))
            }
            _ => None,
        };
        Ok(value)
    }

    pub(crate) fn table_edit_target(
        &self,
        view: &Element,
        node: &Node,
        origin: TableEditPosition,
        columns: &[usize],
        backward: bool,
    ) -> Result<Option<TableEditPosition>> {
        if !node.boolean("edit-on-tab", false) {
            return Ok(None);
        }
        let model = self.view_model(view)?;
        if model.stamp() != node.standard_list_stamp {
            return Ok(None);
        }
        let Some(header) = &node.table_header else {
            return Ok(None);
        };
        if view.table_definition().model.is_some() {
            let Some(rows) = &model.projected else {
                return Ok(None);
            };
            let editable = |column: usize| {
                header
                    .columns
                    .get(column)
                    .is_some_and(|column| column.boolean("editable", false))
            };
            if !columns.iter().copied().any(editable) {
                return Ok(None);
            }
            return Ok(xrack_runtime::next_table_edit_cell(
                rows,
                origin,
                columns,
                backward,
                |_, column, _| editable(column),
            ));
        }
        let Some(template) = view.view_template() else {
            return Ok(None);
        };
        if let Some(rows) = &model.projected {
            self.next_delegate_cell(template, rows, origin, columns, backward)
        } else if let Some(rows) = string_rows(&model) {
            self.next_delegate_cell(template, &rows, origin, columns, backward)
        } else {
            Ok(None)
        }
    }

    fn next_delegate_cell<T: EditableRow>(
        &self,
        template: &Element,
        rows: &TableRows<VecModel<T>>,
        origin: TableEditPosition,
        columns: &[usize],
        backward: bool,
    ) -> Result<Option<TableEditPosition>> {
        let editable = |column| {
            template.children.get(column).filter(|cell: &&Element| {
                matches!(cell.visual, Visual::Builtin("TableCell"))
                    && cell.row_links.contains_key("text")
            })
        };
        if !columns.iter().any(|&column| editable(column).is_some()) {
            return Ok(None);
        }
        let mut sample = None::<(usize, Self)>;
        xrack_runtime::try_next_table_edit_cell(
            rows,
            origin,
            columns,
            backward,
            |index, column, record| {
                let Some(cell) = editable(column) else {
                    return Ok(false);
                };
                if sample.as_ref().is_none_or(|(row, _)| *row != index) {
                    let Some(key) = rows.source().row_key(index) else {
                        return Ok(false);
                    };
                    sample = Some((
                        index,
                        self.sample_table_row(template, index, key, record.row_value())?,
                    ));
                }
                let (_, sample) = sample.as_ref().expect("sampled row");
                Ok(sample.shown_for_table(template)?
                    && sample.visual_flag(template, "enabled", true)?
                    && sample.editable_for_table(cell)?)
            },
        )
    }
}
