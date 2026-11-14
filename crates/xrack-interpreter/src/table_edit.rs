use crate::{
    repeat::{Model, RowScope, ViewModel},
    table, *,
};
use xrack_runtime::{
    RecordValue, RowContext, RowKey, TableCellEditor, TableColumnOrder, TableEditHistory,
    TableEditPosition, TablePasteInput, TableRowPatch, TableRows, VecModel,
};

pub(crate) struct ClipboardParser {
    pub(crate) path: Vec<usize>,
    pub(crate) expression: Expr,
}

pub(crate) struct ClipboardPaste {
    pub(crate) field: RowField,
    pub(crate) parser: Option<ClipboardParser>,
}

enum PasteColumn<'a> {
    Field {
        column: &'a table::StandardColumn,
    },
    Row {
        field: &'a RowField,
        guards: Vec<&'a Element>,
        parser: Option<&'a Expr>,
    },
}

impl<'a> PasteColumn<'a> {
    fn for_row(cell: &'a Element) -> Option<Self> {
        let paste = cell.clipboard_paste.as_ref()?;
        let mut guards = vec![cell];
        if let Some(parser) = &paste.parser {
            let mut element = cell;
            for &index in &parser.path {
                element = element.children.get(index)?;
                guards.push(element);
            }
        }
        Some(Self::Row {
            field: &paste.field,
            guards,
            parser: paste.parser.as_ref().map(|parser| &parser.expression),
        })
    }

    fn path(&self) -> &[String] {
        match self {
            Self::Field { column } => &column.path,
            Self::Row { field, .. } => &field.path,
        }
    }
}

impl Element {
    pub(crate) fn editor_view_for_template(&self, template: &str) -> Option<&Self> {
        if (self.table.is_some() || self.tree.is_some())
            && self.view_template().is_some_and(|row| row.key == template)
        {
            return Some(self);
        }
        self.children
            .iter()
            .find_map(|child| child.editor_view_for_template(template))
    }

    pub(crate) fn table_history_configured(&self) -> bool {
        self.table.as_ref().is_some_and(|table| table.history)
    }

    fn paste_configured(&self) -> bool {
        self.table.as_ref().is_some_and(|table| table.paste)
    }
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
    let Model::Strings(strings) = model.source.as_deref()? else {
        return None;
    };
    Some(TableRows::from_positions(
        Rc::new(strings.clone()),
        model.positions()?.clone(),
    ))
}

fn edit_stamp(model: &ViewModel) -> Option<(u64, u64)> {
    match model.source.as_deref()? {
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
    pub(crate) fn visual_flag_value(&self, element: &Element, name: &str) -> Result<Option<bool>> {
        self.visual_flag_value_with(element, name, |cases| self.state_value(cases))
    }

    pub(crate) fn input_flag_value(&self, element: &Element, name: &str) -> Result<Option<bool>> {
        self.visual_flag_value_with(element, name, |cases| self.input_state_value(cases))
    }

    fn visual_flag_value_with(
        &self,
        element: &Element,
        name: &str,
        state_value: impl FnOnce(&[StateCase]) -> Result<Option<Value>>,
    ) -> Result<Option<bool>> {
        if let Some(cases) = element.state_bindings.get(name) {
            if self.caches_action_flag(element, name, cases) {
                return Ok(match self.cached_action_flag(element, name, cases)? {
                    Some(Value::Bool(value)) => Some(value),
                    _ => None,
                });
            }
            if let Some(Value::Bool(value)) = state_value(cases)? {
                return Ok(Some(value));
            }
        }
        Ok(match self.binding_value(element, name)? {
            Some(Value::Bool(value)) => Some(value),
            _ => None,
        })
    }

    pub(crate) fn visual_flag(&self, element: &Element, name: &str, default: bool) -> Result<bool> {
        Ok(self.visual_flag_value(element, name)?.unwrap_or(default))
    }

    pub(crate) fn element_conditions_met(&self, element: &Element) -> Result<bool> {
        if element.static_hidden {
            return Ok(false);
        }
        for condition in &element.conditions {
            if self.evaluate(condition, &[])? == Value::Bool(false) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn element_shown(&self, element: &Element) -> Result<bool> {
        Ok(self.element_conditions_met(element)? && self.visual_flag(element, "visible", true)?)
    }

    fn editable_for_table(&self, element: &Element) -> Result<bool> {
        Ok(self.element_shown(element)?
            && self.visual_flag(element, "enabled", true)?
            && !self.visual_flag(element, "read-only", false)?)
    }

    pub(crate) fn element_allows_editing(&self, element: &Element) -> Result<bool> {
        Ok(self.visual_flag(element, "enabled", true)? && self.element_shown(element)?)
    }

    pub(crate) fn element_allows_routed_input(
        &self,
        element: &Element,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        self.input_element_available(element, input.root_visibility_overridden())
    }

    pub(crate) fn input_element_available(
        &self,
        element: &Element,
        visibility_overridden: bool,
    ) -> Result<bool> {
        Ok(self.input_flag_value(element, "enabled")?.unwrap_or(true)
            && self.element_conditions_met(element)?
            && (visibility_overridden
                || self.input_flag_value(element, "visible")?.unwrap_or(true)))
    }

    pub(crate) fn editor_view_available(&self, view: &Element) -> Result<bool> {
        let visible = || Ok(self.input_flag_value(view, "visible")?.unwrap_or(true));
        Ok(self.input_flag_value(view, "enabled")?.unwrap_or(true)
            && self.element_conditions_met(view)?
            && if view.key == self.definition().root.key {
                self.0.binding_lifetime.try_input_root_visible(visible)?
            } else {
                visible()?
            })
    }

    pub(crate) fn editable_table_view_model(&self, view: &Element) -> Result<Option<ViewModel>> {
        if !self.editor_view_available(view)? {
            return Ok(None);
        }
        self.view_model(view).map(Some)
    }

    pub(crate) fn table_action_view_model(
        &self,
        view: &Element,
        flag: &str,
        column_order: &TableColumnOrder,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<Option<ViewModel>> {
        if !self.visual_flag(view, flag, false)?
            || !self.element_allows_routed_input(view, input)?
            || &self.repeat_state(view).table_state().column_order() != column_order
        {
            return Ok(None);
        }
        self.view_model(view).map(Some)
    }

    pub(crate) fn table_history_enabled(&self, view: &Element) -> Result<bool> {
        Ok(self.visual_flag(view, "undoable", false)?
            || self.visual_flag(view, "pasteable", false)?)
    }

    pub(crate) fn can_replay_table(&self, view: &Element, redo: bool) -> Result<bool> {
        if !view.table_history_configured() || !self.table_history_enabled(view)? {
            return Ok(false);
        }
        let model = self.view_source_model(view)?;
        let state = self.repeat_state(view);
        let table = state.table_state();
        Ok(match model.as_deref() {
            Some(Model::Records(records)) => table.history.can_replay(records.model(), redo),
            Some(Model::Strings(strings)) => table.string_history.can_replay(strings, redo),
            _ => false,
        })
    }

    pub(crate) fn replay_table(&self, view: &Element, redo: bool) -> Result<bool> {
        if !view.table_history_configured() {
            return Ok(false);
        }
        let model = xrack_runtime::try_prepare_history_replay(
            None,
            || self.view_source_model(view),
            || self.table_history_enabled(view),
        )?;
        Ok(model.is_some_and(|source| self.replay_table_source(view, &source, redo)))
    }

    pub(crate) fn replay_table_from_view(
        &self,
        view: &Element,
        model: u64,
        redo: bool,
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        if !view.table_history_configured() {
            return Ok(false);
        }
        let source = xrack_runtime::try_history_replay_source(
            model,
            || {
                if !self.element_allows_routed_input(view, route)? {
                    return Ok(None);
                }
                self.view_source_model(view)
            },
            || self.table_history_enabled(view),
        )?;
        Ok(route.is_current()
            && source.is_some_and(|source| self.replay_table_source(view, &source, redo)))
    }

    fn replay_table_source(&self, view: &Element, model: &Model, redo: bool) -> bool {
        let state = self.repeat_state(view);
        let table = state.table_state();
        match model {
            Model::Records(records) => table.history.replay(records.model(), redo),
            Model::Strings(strings) => table.string_history.replay(strings, redo),
            _ => false,
        }
    }

    pub(crate) fn commit_projected_cell(
        &self,
        view: &Element,
        table: &Node,
        slot: usize,
        column: usize,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let xrack_runtime::RoutedEvent::CommitCell { original, text } = *input.event() else {
            return Ok(false);
        };
        let Some(field) = view.table_definition().columns.get(column) else {
            return Ok(false);
        };
        let Some((editor, _)) = &field.editor else {
            return Ok(false);
        };
        let path = &field.path;
        let matches =
            |record: &RecordValue| table::matches_field_text(record.field_path(path), original);
        let Some((target, (value, record_history))) =
            xrack_runtime::try_table_cell_edit_target_matching(
                table,
                slot,
                || {
                    Ok(self
                        .editable_table_view_model(view)?
                        .and_then(|model| model.projected))
                },
                matches,
                || input.try_available(|| Ok(self.standard_column_editor(field)?.is_some())),
                || {
                    input.try_prepare(
                        || Ok(true),
                        || {
                            let Some(value) = editor.parse(text) else {
                                return Ok(None);
                            };
                            Ok(Some((value, self.table_history_enabled(view)?)))
                        },
                    )
                },
            )?
        else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        let update = |record: &RecordValue| record.with_field(path, value);
        let state = self.repeat_state(view);
        Ok(state.table_state().history.commit_cell_if_enabled(
            target.rows.source(),
            target.index,
            target.key,
            record_history,
            update,
            capture,
        ))
    }

    pub(crate) fn capture_table_paste_stamp(
        &self,
        scene: &Node,
        path: &[usize],
    ) -> Result<Option<(u64, u64)>> {
        let Some(node) = scene
            .interactive_at_path(path)
            .filter(|node| node.boolean("pasteable", false))
        else {
            return Ok(None);
        };
        let Some((owner, view)) = self.try_target(scene, path)? else {
            return Ok(None);
        };
        if !view.paste_configured() {
            return Ok(None);
        }
        let has_editor = if view.table_definition().model.is_some() {
            view.table_definition()
                .columns
                .iter()
                .any(|column| column.editor.is_some())
        } else {
            view.view_template().is_some_and(|row| {
                row.children
                    .iter()
                    .any(|cell| cell.clipboard_paste.is_some())
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
        Ok(Some(model_stamp))
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
                sampled: true,
            },
        )
    }

    pub(crate) fn route_table_paste(
        &self,
        scene: &Node,
        path: &[usize],
        paste: TablePasteInput<'_>,
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(node) = scene.at_path(path) else {
            return Ok(false);
        };
        let Some((owner, view, route)) = self.input_route_target(scene, path, route)? else {
            return Ok(false);
        };
        if !view.paste_configured() {
            return Ok(false);
        }
        owner.apply_table_paste(view, node, paste, route)
    }

    fn apply_table_paste(
        &self,
        view: &Element,
        node: &Node,
        paste: TablePasteInput<'_>,
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        let table = state.table_state();
        xrack_runtime::try_with_table_paste_target(
            route,
            || {
                let Some(model) = self.table_action_view_model(
                    view,
                    "pasteable",
                    &node.table_column_order,
                    route,
                )?
                else {
                    return Ok(None);
                };
                let matches = model.stamp() == node.standard_list_stamp
                    && edit_stamp(&model) == Some(paste.stamp);
                Ok(matches.then_some(model))
            },
            |model, still_current| {
                if let Some(rows) = &model.projected {
                    self.paste_rows(view, rows, &table.history, paste, still_current)
                } else if let Some(rows) = string_rows(model) {
                    self.paste_rows(view, &rows, &table.string_history, paste, still_current)
                } else {
                    Ok(false)
                }
            },
        )
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
            view.table_definition()
                .columns
                .iter()
                .map(|column| {
                    column
                        .editor
                        .is_some()
                        .then_some(PasteColumn::Field { column })
                })
                .collect()
        } else {
            let Some(template) = template else {
                return Ok(false);
            };
            template.children.iter().map(PasteColumn::for_row).collect()
        };
        let mut sample = None::<(usize, Self)>;
        let changed = history.try_paste_in_order_guarded(
            rows,
            input,
            still_current,
            |row, column, current, next, text, still_current| {
                let Some(definition) = definitions.get(column).and_then(Option::as_ref) else {
                    return Ok(None);
                };
                let value = match definition {
                    PasteColumn::Field { column } => {
                        let Some(editor) = self.standard_column_editor(column)? else {
                            return Ok(None);
                        };
                        if !still_current()? {
                            return Ok(None);
                        }
                        editor.parse(text)
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
                            still_current,
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
        still_current: &mut dyn FnMut() -> Result<bool>,
    ) -> Result<Option<Value>> {
        let PasteColumn::Row { guards, parser, .. } = column else {
            return Ok(None);
        };
        if !self.element_shown(row)?
            || !still_current()?
            || !self.visual_flag(row, "enabled", true)?
            || !still_current()?
        {
            return Ok(None);
        }
        for element in guards {
            if !self.editable_for_table(element)? || !still_current()? {
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
            && (!self.visual_flag(cell, "editable", true)? || !still_current()?)
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
                xrack_runtime::try_table_paste_number(
                    Some(value),
                    still_current,
                    || {
                        Ok(match self.binding_value(cell, "minimum")? {
                            Some(Value::Int(value)) => value,
                            _ => 0,
                        })
                    },
                    || {
                        Ok(match self.binding_value(cell, "maximum")? {
                            Some(Value::Int(value)) => value,
                            _ => 100,
                        })
                    },
                    i64::max,
                )?
                .map(Value::Int)
            }
            Some(Value::Float(_)) => {
                let Some(Value::Float(value)) = TableCellEditor::Float.parse(text) else {
                    return Ok(None);
                };
                xrack_runtime::try_table_paste_number(
                    Some(value),
                    still_current,
                    || {
                        Ok(match self.binding_value(cell, "minimum")? {
                            Some(Value::Float(value)) => value,
                            _ => 0.0,
                        })
                    },
                    || {
                        Ok(match self.binding_value(cell, "maximum")? {
                            Some(Value::Float(value)) => value,
                            _ => 100.0,
                        })
                    },
                    f64::max,
                )?
                .map(Value::Float)
            }
            _ => None,
        };
        Ok(value)
    }

    pub(crate) fn table_edit_target(
        &self,
        view: &Element,
        node: &Node,
        navigation: xrack_runtime::TableEditNavigation<'_>,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<Option<TableEditPosition>> {
        let columns = navigation.columns;
        let rows = || {
            self.table_action_view_model(view, "edit-on-tab", &node.table_column_order, input)?
                .map(|model| model.table_rows())
                .transpose()
        };
        if view.table_definition().model.is_some() {
            let fields = &view.table_definition().columns;
            let editor = |column: usize| fields.get(column).filter(|field| field.editor.is_some());
            if !columns.iter().any(|&column| editor(column).is_some()) {
                return Ok(None);
            }
            return xrack_runtime::try_next_table_edit_cell_while_current(
                node,
                navigation,
                || input.is_current(),
                rows,
                |_, column, _, _| {
                    editor(column).map_or(Ok(false), |column| {
                        Ok(self.standard_column_editor(column)?.is_some())
                    })
                },
            );
        }
        let Some(template) = view.view_template() else {
            return Ok(None);
        };
        let editor = |column| {
            template.children.get(column).filter(|cell: &&Element| {
                matches!(cell.visual, Visual::Builtin("TableCell"))
                    && cell.row_links.contains_key("text")
            })
        };
        if !columns.iter().any(|&column| editor(column).is_some()) {
            return Ok(None);
        }

        let mut sample = None::<(usize, Self)>;
        xrack_runtime::try_next_table_edit_cell_while_current(
            node,
            navigation,
            || input.is_current(),
            rows,
            |index, column, key, record| {
                let Some(cell) = editor(column) else {
                    return Ok(false);
                };
                if sample.as_ref().is_none_or(|(row, _)| *row != index) {
                    sample = Some((
                        index,
                        self.sample_table_row(template, index, key, record.clone())?,
                    ));
                }
                let (_, sample) = sample.as_ref().expect("sampled row");
                Ok(sample.element_shown(template)?
                    && sample.visual_flag(template, "enabled", true)?
                    && sample.editable_for_table(cell)?)
            },
        )
    }
}
