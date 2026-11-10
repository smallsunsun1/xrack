use crate::{ChangeSignal, Node, RowKey, Value, VecModel};
use std::{borrow::Cow, fmt, ops::Range, rc::Rc};

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

struct RecordRows<T> {
    model: VecModel<T>,
    label: fn(&T) -> &str,
    value: fn(&T) -> Value,
    enabled: fn(&T) -> bool,
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
        self.model.project_row_untracked(index, self.value)
    }
    fn enabled(&self, index: usize) -> bool {
        self.model
            .project_row_untracked(index, self.enabled)
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
}

impl ComboItems {
    pub fn new<T: 'static>(
        model: VecModel<T>,
        label: fn(&T) -> &str,
        value: fn(&T) -> Value,
    ) -> Self {
        Self::with_enabled(model, label, value, |_| true)
    }

    pub fn with_enabled<T: 'static>(
        model: VecModel<T>,
        label: fn(&T) -> &str,
        value: fn(&T) -> Value,
        enabled: fn(&T) -> bool,
    ) -> Self {
        let changes = model.observe_projection_changes(usize::MAX, move |_, before, after| {
            enabled(before) != enabled(after)
                || label(before) != label(after)
                || value(before) != value(after)
        });
        Self {
            rows: Rc::new(RecordRows {
                model,
                label,
                value,
                enabled,
            }),
            changes,
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
            selected,
        }
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ComboModel {
    items: ComboItems,
    structure: (u64, u64),
    selected: Option<(usize, String)>,
}

impl fmt::Debug for ComboModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComboModel")
            .field("structure", &self.structure)
            .field("selected", &self.selected)
            .finish()
    }
}

impl PartialEq for ComboModel {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items.rows, &other.items.rows)
            && self.structure == other.structure
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

pub fn choice_enabled(node: &Node, index: usize) -> bool {
    if node.kind == "SearchField" {
        return crate::search_suggestion(node, index).is_some();
    }
    model(node).map_or_else(
        || index < node.strings("model").len(),
        |model| model.items.rows.enabled(index),
    )
}

#[doc(hidden)]
pub fn combo_model_is_current<T>(node: &Node, source: &VecModel<T>) -> bool {
    model(node).is_some_and(|model| model.structure == source.structure_stamp())
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
