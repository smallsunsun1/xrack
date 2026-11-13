use crate::{ChangeSignal, ControlAction, Node, RowKey, Value, VecModel};
use std::{borrow::Cow, cell::Cell, fmt, ops::Range, rc::Rc};

trait Rows {
    fn structure(&self) -> (u64, u64);
    fn len(&self) -> usize;
    fn label(&self, index: usize) -> Option<String>;
    fn value(&self, index: usize) -> Option<Value>;
    fn enabled(&self, index: usize) -> bool;
    fn key(&self, index: usize) -> Option<RowKey>;
    fn index(&self, key: RowKey) -> Option<usize>;
    fn find_label(
        &self,
        range: Range<usize>,
        enabled_only: bool,
        predicate: &mut dyn FnMut(&str) -> bool,
    ) -> Option<usize>;
    fn find_value(&self, value: &Value) -> Option<usize>;
}

type LabelProjection<T> = dyn Fn(&T) -> &str;
type ValueProjection<T> = dyn Fn(&T) -> Value;
type EnabledProjection<T> = dyn Fn(&T) -> bool;

struct RecordRows<T> {
    model: VecModel<T>,
    label: Rc<LabelProjection<T>>,
    value: Rc<ValueProjection<T>>,
    enabled: Rc<EnabledProjection<T>>,
}

impl<T> Rows for RecordRows<T> {
    fn structure(&self) -> (u64, u64) {
        self.model.structure_stamp()
    }
    fn len(&self) -> usize {
        self.model.len()
    }
    fn label(&self, index: usize) -> Option<String> {
        self.model
            .project_row_untracked(index, |row| (self.label)(row).to_owned())
    }
    fn value(&self, index: usize) -> Option<Value> {
        self.model
            .project_row_untracked(index, |row| (self.value)(row))
    }
    fn enabled(&self, index: usize) -> bool {
        self.model
            .project_row_untracked(index, |row| (self.enabled)(row))
            .unwrap_or(false)
    }
    fn key(&self, index: usize) -> Option<RowKey> {
        self.model.row_key(index)
    }
    fn index(&self, key: RowKey) -> Option<usize> {
        self.model.row_index(key)
    }
    fn find_label(
        &self,
        range: Range<usize>,
        enabled_only: bool,
        predicate: &mut dyn FnMut(&str) -> bool,
    ) -> Option<usize> {
        self.model.position_untracked(range, |row| {
            (!enabled_only || (self.enabled)(row)) && predicate((self.label)(row))
        })
    }
    fn find_value(&self, value: &Value) -> Option<usize> {
        self.model
            .position_untracked(0..self.model.len(), |row| (self.value)(row) == *value)
    }
}

/// A typed record projection shared by snapshots of a ComboBox.
#[doc(hidden)]
#[derive(Clone)]
pub struct ComboItems {
    rows: Rc<dyn Rows>,
    changes: Rc<ChangeSignal>,
    revision: Rc<Cell<u64>>,
}

impl ComboItems {
    pub fn new<T: 'static>(
        model: VecModel<T>,
        label: impl Fn(&T) -> &str + 'static,
        value: impl Fn(&T) -> Value + 'static,
    ) -> Self {
        Self::with_enabled(model, label, value, |_| true)
    }

    pub fn with_enabled<T: 'static>(
        model: VecModel<T>,
        label: impl Fn(&T) -> &str + 'static,
        value: impl Fn(&T) -> Value + 'static,
        enabled: impl Fn(&T) -> bool + 'static,
    ) -> Self {
        let label: Rc<LabelProjection<T>> = Rc::new(label);
        let value: Rc<ValueProjection<T>> = Rc::new(value);
        let enabled: Rc<EnabledProjection<T>> = Rc::new(enabled);
        let revision = Rc::new(Cell::new(0_u64));
        let changes = model.observe_projection_changes(usize::MAX, {
            let label = label.clone();
            let value = value.clone();
            let enabled = enabled.clone();
            let revision = revision.clone();
            move |_, before, after| {
                let changed = enabled(before) != enabled(after)
                    || label(before) != label(after)
                    || value(before) != value(after);
                if changed {
                    revision.set(revision.get().wrapping_add(1));
                }
                changed
            }
        });
        Self {
            rows: Rc::new(RecordRows {
                model,
                label,
                value,
                enabled,
            }),
            changes,
            revision,
        }
    }

    pub fn snapshot(&self, index: i64, value: Option<&Value>) -> ComboModel {
        self.changes.track();
        let structure = self.rows.structure();
        let index = match value {
            Some(value) => self.rows.find_value(value),
            None => usize::try_from(index).ok(),
        };
        let selected = index.and_then(|index| Some((index, self.rows.label(index)?)));
        ComboModel {
            items: self.clone(),
            structure,
            revision: self.revision.get(),
            selected,
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ComboModel {
    items: ComboItems,
    structure: (u64, u64),
    revision: u64,
    selected: Option<(usize, String)>,
}

impl fmt::Debug for ComboModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComboModel")
            .field("structure", &self.structure)
            .field("revision", &self.revision)
            .field("selected", &self.selected)
            .finish()
    }
}

impl PartialEq for ComboModel {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items.rows, &other.items.rows)
            && self.structure == other.structure
            && self.revision == other.revision
            && self.selected == other.selected
    }
}

fn model(node: &Node) -> Option<&ComboModel> {
    match node.properties.get("model") {
        Some(Value::ComboModel(model)) => Some(model),
        _ => None,
    }
}

pub fn combo_index(node: &Node) -> Option<usize> {
    if let Some(model) = model(node) {
        return model.selected.as_ref().map(|(index, _)| *index);
    }
    let model = node.strings("model");
    if let Some(value) = node.text("current-value") {
        model.iter().position(|item| item == value)
    } else {
        let index = usize::try_from(node.int("current-index").unwrap_or(0)).ok()?;
        (index < model.len()).then_some(index)
    }
}

pub fn combo_text(node: &Node) -> &str {
    if let Some(model) = model(node) {
        return model.selected.as_ref().map_or("", |(_, label)| label);
    }
    combo_index(node).map_or("", |index| node.strings("model")[index].as_str())
}

pub fn combo_count(node: &Node) -> usize {
    model(node).map_or_else(
        || node.strings("model").len(),
        |model| model.items.rows.len(),
    )
}

pub fn combo_label(node: &Node, index: usize) -> Option<Cow<'_, str>> {
    match model(node) {
        Some(model) => model.items.rows.label(index).map(Cow::Owned),
        None => node
            .strings("model")
            .get(index)
            .map(|label| Cow::Borrowed(label.as_str())),
    }
}

pub fn combo_labels(node: &Node, range: Range<usize>) -> Cow<'_, [String]> {
    match model(node) {
        Some(model) => Cow::Owned(
            range
                .filter_map(|index| model.items.rows.label(index))
                .collect(),
        ),
        None => Cow::Borrowed(&node.strings("model")[range]),
    }
}

pub fn combo_find_label(node: &Node, mut predicate: impl FnMut(&str) -> bool) -> Option<usize> {
    match model(node) {
        Some(model) => {
            model
                .items
                .rows
                .find_label(0..model.items.rows.len(), false, &mut predicate)
        }
        None => node
            .strings("model")
            .iter()
            .position(|text| predicate(text)),
    }
}

/// Search enabled rows from a model index, wrapping to the beginning once.
pub fn combo_find_enabled_label(
    node: &Node,
    start: usize,
    mut predicate: impl FnMut(&str) -> bool,
) -> Option<usize> {
    let len = combo_count(node);
    let start = start.min(len);
    [start..len, 0..start].into_iter().find_map(|range| {
        if let Some(model) = model(node) {
            model.items.rows.find_label(range, true, &mut predicate)
        } else {
            let start = range.start;
            node.strings("model")[range]
                .iter()
                .position(|text| predicate(text))
                .map(|offset| start + offset)
        }
    })
}

#[doc(hidden)]
pub struct ComboSubmission {
    pub text: String,
    pub index: Option<usize>,
}

/// Interpret the text in an editable ComboBox snapshot. A disabled record cannot be
/// submitted as free text. Keep the selected record when its label still matches.
#[doc(hidden)]
pub fn combo_submission(node: &Node) -> Option<ComboSubmission> {
    if !crate::combo_editable(node) {
        return None;
    }

    let draft = crate::text_input_value(node);
    let index = if model(node).is_some() {
        let index = combo_index(node)
            .filter(|&index| choice_enabled(node, index) && combo_text(node) == draft)
            .or_else(|| combo_find_enabled_label(node, 0, |label| label == draft));
        if index.is_none() && combo_find_label(node, |label| label == draft).is_some() {
            return None;
        }
        index
    } else {
        node.strings("model").iter().position(|item| item == draft)
    };
    Some(ComboSubmission {
        text: draft.to_owned(),
        index,
    })
}

#[doc(hidden)]
pub struct ComboSelection {
    pub index: usize,
    pub label: String,
    pub value: Value,
}

#[doc(hidden)]
pub enum ComboInput {
    Select {
        selection: ComboSelection,
        editable: bool,
    },
    Accept {
        text: String,
        selection: Option<ComboSelection>,
    },
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub enum ComboEditor {
    None,
    Default,
    Explicit,
}

#[doc(hidden)]
pub enum ComboUpdate<'a> {
    Select(&'a ComboSelection),
    ClearIndex,
    SetFreeText(&'a str),
    SetEditText(&'a str),
    ResetEditText,
    Selected(&'a str),
    ValueSelected(&'a Value),
    Accepted(&'a str),
}

impl ComboInput {
    /// Commit selection and edit text before publishing callbacks.
    pub fn try_apply<E>(
        self,
        editor: ComboEditor,
        by_value: bool,
        records: bool,
        mut update: impl FnMut(ComboUpdate<'_>) -> Result<bool, E>,
    ) -> Result<bool, E> {
        use ComboUpdate::*;

        match self {
            Self::Select {
                selection,
                editable,
            } => {
                if !update(Select(&selection))? {
                    return Ok(false);
                }
                if editable {
                    match editor {
                        ComboEditor::None => {}
                        ComboEditor::Default => {
                            update(ResetEditText)?;
                        }
                        ComboEditor::Explicit => {
                            update(SetEditText(&selection.label))?;
                        }
                    }
                }
                update(Selected(&selection.label))?;
                if records {
                    update(ValueSelected(&selection.value))?;
                }
            }
            Self::Accept { text, selection } => {
                if matches!(editor, ComboEditor::None) {
                    return Ok(false);
                }
                if let Some(selection) = selection {
                    if !update(Select(&selection))? {
                        return Ok(false);
                    }
                    if matches!(editor, ComboEditor::Default) {
                        update(ResetEditText)?;
                    }
                    if records {
                        update(ValueSelected(&selection.value))?;
                    }
                } else {
                    if !by_value {
                        update(ClearIndex)?;
                    } else if !records {
                        update(SetFreeText(&text))?;
                    }
                    if matches!(editor, ComboEditor::Default) {
                        update(SetEditText(&text))?;
                    }
                }
                update(Accepted(&text))?;
            }
        }
        Ok(true)
    }

    pub fn apply(
        self,
        editor: ComboEditor,
        by_value: bool,
        records: bool,
        mut update: impl FnMut(ComboUpdate<'_>) -> bool,
    ) -> bool {
        self.try_apply(editor, by_value, records, |event| {
            Ok::<_, std::convert::Infallible>(update(event))
        })
        .unwrap_or_else(|error| match error {})
    }
}

fn combo_selection(node: &Node, index: usize) -> Option<ComboSelection> {
    if !choice_enabled(node, index) {
        return None;
    }
    let label = combo_label(node, index)?.into_owned();
    let value = if model(node).is_some() {
        combo_value(node, index)?
    } else {
        Value::String(label.clone())
    };
    Some(ComboSelection {
        index,
        label,
        value,
    })
}

/// Check live bindings before resolving the choice or draft from the input scene.
#[doc(hidden)]
pub fn try_prepare_combo_input<E>(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> Result<bool, E>,
    editable: impl FnOnce() -> Result<bool, E>,
    model_current: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<ComboInput>, E> {
    if !matches!(
        action,
        ControlAction::Select(_) | ControlAction::AcceptComboText
    ) {
        return Ok(None);
    }
    crate::property::watch_reads(|reads| {
        if !available()? || reads.changed() {
            return Ok(None);
        }
        let editable = editable()?;
        if (action == ControlAction::AcceptComboText && !editable)
            || reads.changed()
            || !model_current()?
            || reads.changed()
        {
            return Ok(None);
        }
        Ok(match action {
            ControlAction::Select(index) => {
                combo_selection(node, index).map(|selection| ComboInput::Select {
                    selection,
                    editable,
                })
            }
            ControlAction::AcceptComboText => {
                let Some(submission) = combo_submission(node) else {
                    return Ok(None);
                };
                let selection = if let Some(index) = submission.index {
                    let Some(selection) = combo_selection(node, index) else {
                        return Ok(None);
                    };
                    Some(selection)
                } else {
                    None
                };
                Some(ComboInput::Accept {
                    text: submission.text,
                    selection,
                })
            }
            _ => None,
        })
    })
}

#[doc(hidden)]
pub fn prepare_combo_input(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> bool,
    editable: impl FnOnce() -> bool,
    model_current: impl FnOnce() -> bool,
) -> Option<ComboInput> {
    try_prepare_combo_input(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(editable()),
        || Ok(model_current()),
    )
    .unwrap_or_else(|error| match error {})
}

pub fn choice_enabled(node: &Node, index: usize) -> bool {
    if node.kind == "SearchField" {
        return crate::search_suggestion(node, index).is_some();
    }
    model(node).map_or_else(
        || index < node.strings("model").len(),
        |model| model.items.rows.enabled(index),
    )
}

impl Node {
    #[doc(hidden)]
    pub fn set_string_combo_model(&mut self, source: VecModel<String>) {
        self.standard_list_stamp = Some(source.edit_stamp());
        self.properties.insert("model", source.into());
    }
}

#[doc(hidden)]
pub fn combo_model_is_current<T>(node: &Node, source: &VecModel<T>) -> bool {
    if let Some(model) = model(node) {
        model.items.changes.track();
        model.structure == source.structure_stamp() && model.revision == model.items.revision.get()
    } else {
        node.standard_list_stamp == Some(source.edit_stamp())
    }
}

#[doc(hidden)]
pub fn combo_value(node: &Node, index: usize) -> Option<Value> {
    model(node)?.items.rows.value(index)
}

pub fn combo_row_key(node: &Node, index: usize) -> Option<RowKey> {
    model(node)?.items.rows.key(index)
}

pub fn combo_row_index(node: &Node, key: RowKey) -> Option<usize> {
    model(node)?.items.rows.index(key)
}

/// A pressed or projected option must still identify the same record and business value.
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceIdentity {
    pub label: String,
    pub record: Option<(RowKey, Value)>,
}

pub fn choice_identity(node: &Node, index: usize) -> Option<ChoiceIdentity> {
    if node.kind == "SearchField" {
        return Some(ChoiceIdentity {
            label: crate::search_suggestion(node, index)?.to_owned(),
            record: None,
        });
    }
    let label = combo_label(node, index)?.into_owned();
    let record = match model(node) {
        Some(model) => Some((model.items.rows.key(index)?, model.items.rows.value(index)?)),
        None => None,
    };
    Some(ChoiceIdentity { label, record })
}
