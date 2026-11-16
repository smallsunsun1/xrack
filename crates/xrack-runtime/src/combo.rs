use crate::{ChangeSignal, ControlAction, InputRoute, Node, Property, RowKey, Value, VecModel};
use std::{
    any::Any,
    borrow::Cow,
    cell::{Cell, RefCell},
    fmt,
    ops::Range,
    rc::Rc,
};

#[derive(Clone, Default)]
#[doc(hidden)]
pub struct ComboPopupHandle(Rc<ComboPopupState>);

#[derive(Default)]
struct ComboPopupState {
    open: Property<bool>,
    highlighted: Property<i64>,
}

impl ComboPopupHandle {
    pub fn popup_open(&self) -> bool {
        self.0.open.get(|| false)
    }

    pub fn highlighted_index(&self) -> i64 {
        self.0.highlighted.get(|| -1)
    }

    pub fn publish(&self, open: bool, highlighted: Option<usize>) {
        self.0.open.set_if_changed(open);
        self.0
            .highlighted
            .set_if_changed(highlighted.map_or(-1, |index| index as i64));
    }
}

impl PartialEq for ComboPopupHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ComboPopupHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComboPopupHandle")
            .finish_non_exhaustive()
    }
}

#[doc(hidden)]
pub struct ComboOptionInput {
    pub text: String,
    pub index: i64,
    pub enabled: bool,
    pub selected: bool,
    pub highlighted: bool,
    pub displacement: f64,
    pub progress: f64,
    numeric: bool,
    record_rows: Option<Rc<dyn Rows>>,
}

impl ComboOptionInput {
    pub fn new(node: &Node, index: usize, text: String, selected: bool, highlighted: bool) -> Self {
        Self {
            text,
            index: index as i64,
            enabled: choice_enabled(node, index),
            selected,
            highlighted,
            displacement: 0.0,
            progress: 0.5,
            numeric: false,
            record_rows: model(node).map(|model| model.items.rows.clone()),
        }
    }

    pub(crate) fn for_tumbler(
        index: usize,
        text: String,
        selected: bool,
        enabled: bool,
        displacement: f64,
        numeric: bool,
        records: Option<&ComboItems>,
    ) -> Self {
        Self {
            index: index as i64,
            text,
            selected,
            enabled,
            highlighted: false,
            displacement,
            progress: 0.5,
            numeric,
            record_rows: records.map(|items| items.rows.clone()),
        }
    }

    /// Fetch the row in its original representation and observe this row's changes.
    pub fn record_data<T: Clone + 'static>(&self) -> Option<T> {
        let rows = self.record_rows.as_ref()?;
        let model = rows.source().downcast_ref::<VecModel<T>>()?;
        model.row(self.index as usize)
    }
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub enum ComboOptionParameter {
    Data,
    Text,
    Index,
    Enabled,
    Selected,
    Highlighted,
    Displacement,
    Progress,
}

impl ComboOptionParameter {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "option-data" => Self::Data,
            "option-text" => Self::Text,
            "option-index" => Self::Index,
            "option-enabled" => Self::Enabled,
            "option-selected" => Self::Selected,
            "option-highlighted" => Self::Highlighted,
            "option-displacement" => Self::Displacement,
            "option-progress" => Self::Progress,
            _ => return None,
        })
    }

    pub fn read(self, input: &ComboOptionInput) -> Value {
        match self {
            Self::Data if input.numeric => input.index.into(),
            Self::Data => input
                .record_data::<crate::RecordValue>()
                .map_or_else(|| input.text.clone().into(), Value::Record),
            Self::Text => input.text.clone().into(),
            Self::Index => input.index.into(),
            Self::Enabled => input.enabled.into(),
            Self::Selected => input.selected.into(),
            Self::Highlighted => input.highlighted.into(),
            Self::Displacement => input.displacement.into(),
            Self::Progress => input.progress.into(),
        }
    }
}

type RenderOption = dyn Fn(ComboOptionInput) -> Option<Node>;

struct OptionArtwork {
    render: Box<RenderOption>,
    reads: RefCell<Option<Property<()>>>,
}

/// Produces artwork for a visible choice option.
#[derive(Clone)]
#[doc(hidden)]
pub struct ComboOptionRenderer(Rc<OptionArtwork>);

impl ComboOptionRenderer {
    pub fn new(render: impl Fn(ComboOptionInput) -> Option<Node> + 'static) -> Self {
        Self(Rc::new(OptionArtwork {
            render: Box::new(render),
            reads: RefCell::default(),
        }))
    }

    pub fn render(&self, input: ComboOptionInput) -> Option<Node> {
        (self.0.render)(input)
    }

    pub fn render_visible(
        &self,
        inputs: impl Iterator<Item = ComboOptionInput>,
    ) -> Vec<Option<Node>> {
        let reads = Property::default();
        let mut options = Vec::new();
        reads.get(|| options = inputs.map(self.0.render.as_ref()).collect());
        // Keep the observed dependencies alive between frames.
        self.0.reads.replace(Some(reads));
        options
    }

    pub fn artwork_dirty(&self) -> bool {
        self.0
            .reads
            .borrow()
            .as_ref()
            .is_some_and(Property::is_dirty)
    }
}

impl PartialEq for ComboOptionRenderer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ComboOptionRenderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComboOptionRenderer")
            .finish_non_exhaustive()
    }
}

trait Rows {
    fn source(&self) -> &dyn Any;
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

impl<T: 'static> Rows for RecordRows<T> {
    fn source(&self) -> &dyn Any {
        &self.model
    }

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

/// A typed record projection shared by snapshots of choice controls.
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
        let index = self.selected_index(index, value);
        let selected = index.and_then(|index| Some((index, self.rows.label(index)?)));
        ComboModel {
            items: self.clone(),
            structure,
            revision: self.revision.get(),
            selected,
        }
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn stamp(&self) -> ((u64, u64), u64) {
        self.changes.track();
        (self.rows.structure(), self.revision.get())
    }

    pub(crate) fn same_projection(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.rows, &other.rows)
    }

    pub(crate) fn row_key(&self, index: usize) -> Option<RowKey> {
        self.rows.key(index)
    }

    pub fn selected_index(&self, index: i64, value: Option<&Value>) -> Option<usize> {
        if let Some(value) = value {
            self.changes.track();
            self.rows.structure();
            self.rows.find_value(value)
        } else {
            usize::try_from(index)
                .ok()
                .filter(|&index| index < self.rows.len())
        }
    }

    pub(crate) fn selected_text(&self, index: usize) -> Option<String> {
        self.changes.track();
        self.rows.structure();
        self.rows.label(index)
    }

    pub(crate) fn selected_value(&self, index: usize) -> Option<Value> {
        self.changes.track();
        self.rows.structure();
        self.rows.value(index)
    }

    /// Use the selection already read for the scene when projecting record options.
    #[doc(hidden)]
    pub fn populate_scene(&self, node: &mut Node) {
        let index = node.int("current-index").unwrap_or(0);
        let snapshot = self.snapshot(index, node.properties.get("current-value"));
        node.properties.insert("model", Value::ComboModel(snapshot));
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

fn string_combo_index(model: &VecModel<String>, index: i64, value: Option<&str>) -> Option<usize> {
    if let Some(value) = value {
        model.contents_stamp();
        model.position_untracked(0..model.len(), |item| item == value)
    } else {
        usize::try_from(index)
            .ok()
            .filter(|&index| index < model.len())
    }
}

/// Live model access for declarative reads that do not require a rendered node.
#[doc(hidden)]
pub enum ComboStateModel {
    Strings(VecModel<String>),
    Records(ComboItems),
    Tumbler(ComboItems),
}

impl ComboStateModel {
    fn exact_enabled_selection(&self, text: &str) -> Option<ComboSelection> {
        let (index, value) = match self {
            Self::Strings(model) => {
                model.contents_stamp();
                let index = model.position_untracked(0..model.len(), |label| label == text)?;
                (index, Value::String(text.to_owned()))
            }
            Self::Records(items) => {
                items.stamp();
                let index = items
                    .rows
                    .find_label(0..items.rows.len(), true, &mut |label| label == text)?;
                (index, items.rows.value(index)?)
            }
            Self::Tumbler(_) => return None,
        };
        Some(ComboSelection {
            index,
            value,
            label: text.to_owned(),
        })
    }

    pub fn count(&self) -> usize {
        match self {
            Self::Strings(model) => model.len(),
            Self::Records(model) => model.count(),
            Self::Tumbler(model) => model.wheel_count(),
        }
    }

    pub fn selected_index(&self, index: i64, value: Option<&Value>) -> Option<usize> {
        match self {
            Self::Strings(model) => {
                let value = match value {
                    Some(Value::String(text)) => Some(text.as_str()),
                    None => None,
                    _ => return None,
                };
                string_combo_index(model, index, value)
            }
            Self::Records(model) => model.selected_index(index, value),
            Self::Tumbler(model) => model.wheel_index(index, value),
        }
    }

    pub fn text_at(&self, index: usize) -> Option<String> {
        match self {
            Self::Strings(model) => model.row(index),
            Self::Records(model) | Self::Tumbler(model) => model.selected_text(index),
        }
    }

    pub fn value_at(&self, index: usize) -> Option<Value> {
        match self {
            Self::Strings(model) => model.row(index).map(Value::String),
            Self::Records(model) | Self::Tumbler(model) => model.selected_value(index),
        }
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
    FinishEdit { text: &'a str, accepted: bool },
    Selected(&'a str),
    ValueSelected(&'a Value),
    Accepted(&'a str),
}

impl ComboInput {
    pub fn accepts_unmatched_text(&self) -> bool {
        matches!(
            self,
            Self::Accept {
                selection: None,
                ..
            }
        )
    }

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
                    update(FinishEdit {
                        text: &selection.label,
                        accepted: false,
                    })?;
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
                    update(FinishEdit {
                        text: &text,
                        accepted: true,
                    })?;
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
                    update(FinishEdit {
                        text: &text,
                        accepted: true,
                    })?;
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

fn same_combo_value(left: &Value, right: &Value) -> bool {
    left == right
        || matches!((left, right), (Value::Float(a), Value::Float(b)) if a.is_nan() && b.is_nan())
}

/// Run the accepted handler before checking a fresh model for newly inserted text.
/// The editor can keep its existing free-text state or let the handler supply a selection.
#[doc(hidden)]
pub fn try_resolve_combo_after_accept<E>(
    text: &str,
    mut state: impl FnMut() -> Result<Option<Value>, E>,
    accepted: impl FnOnce() -> Result<(), E>,
    available: impl FnOnce() -> Result<bool, E>,
    model: impl FnOnce() -> Result<ComboStateModel, E>,
) -> Result<Option<ComboSelection>, E> {
    let before = state()?;
    accepted()?;
    let Some(before) = before else {
        return Ok(None);
    };
    crate::property::watch_reads(|reads| {
        if !available()?
            || !state()?.is_some_and(|after| same_combo_value(&before, &after))
            || reads.changed()
        {
            return Ok(None);
        }
        let selection = model()?.exact_enabled_selection(text);
        Ok(selection.filter(|_| !reads.changed()))
    })
}

#[doc(hidden)]
pub fn resolve_combo_after_accept(
    text: &str,
    mut state: impl FnMut() -> Option<Value>,
    accepted: impl FnOnce(),
    available: impl FnOnce() -> bool,
    model: impl FnOnce() -> ComboStateModel,
) -> Option<ComboSelection> {
    try_resolve_combo_after_accept(
        text,
        || Ok::<_, std::convert::Infallible>(state()),
        || Ok(accepted()),
        || Ok(available()),
        || Ok(model()),
    )
    .unwrap_or_else(|error| match error {})
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
    try_prepare_combo_input_with_draft(node, action, available, editable, model_current, || {
        Ok(None)
    })
}

#[doc(hidden)]
pub fn try_prepare_combo_input_with_draft<E>(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> Result<bool, E>,
    editable: impl FnOnce() -> Result<bool, E>,
    model_current: impl FnOnce() -> Result<bool, E>,
    linked_draft: impl FnOnce() -> Result<Option<String>, E>,
) -> Result<Option<ComboInput>, E> {
    try_prepare_combo_input_with_live_draft(
        node,
        action,
        available,
        editable,
        model_current,
        || Ok(linked_draft()?.is_none_or(|draft| draft == crate::text_input_value(node))),
    )
}

#[doc(hidden)]
pub fn try_prepare_combo_input_with_live_draft<E>(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> Result<bool, E>,
    editable: impl FnOnce() -> Result<bool, E>,
    model_current: impl FnOnce() -> Result<bool, E>,
    draft_current: impl FnOnce() -> Result<bool, E>,
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
                if !draft_current()? || reads.changed() {
                    return Ok(None);
                }
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
pub fn prepare_combo_input_with_live_draft(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> bool,
    editable: impl FnOnce() -> bool,
    model_current: impl FnOnce() -> bool,
    draft_current: impl FnOnce() -> bool,
) -> Option<ComboInput> {
    try_prepare_combo_input_with_live_draft(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(editable()),
        || Ok(model_current()),
        || Ok(draft_current()),
    )
    .unwrap_or_else(|error| match error {})
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

#[doc(hidden)]
pub fn prepare_combo_input_with_draft(
    node: &Node,
    action: ControlAction,
    available: impl FnOnce() -> bool,
    editable: impl FnOnce() -> bool,
    model_current: impl FnOnce() -> bool,
    linked_draft: impl FnOnce() -> Option<String>,
) -> Option<ComboInput> {
    try_prepare_combo_input_with_draft(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(editable()),
        || Ok(model_current()),
        || Ok(linked_draft()),
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
    /// Only read the model and selection when the editor is displaying a label.
    #[doc(hidden)]
    pub fn try_combo_draft_is_current<E>(
        &self,
        draft: Option<String>,
        selection: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        let shown = crate::text_input_value(self);
        match draft {
            Some(draft) => Ok(draft == shown),
            None => Ok(shown == combo_text(self) && selection()?),
        }
    }

    #[doc(hidden)]
    pub fn combo_draft_is_current(
        &self,
        draft: Option<String>,
        selection: impl FnOnce() -> bool,
    ) -> bool {
        self.try_combo_draft_is_current(draft, || Ok::<_, std::convert::Infallible>(selection()))
            .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn combo_selection_is_current(&self, property: &str, value: &Value) -> bool {
        self.properties
            .get(property)
            .is_some_and(|shown| same_combo_value(shown, value))
    }

    #[doc(hidden)]
    pub fn try_prepare_combo_edit<E>(
        &self,
        input: InputRoute<'_>,
        editable: impl FnOnce() -> Result<bool, E>,
        draft_current: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        Ok(input.try_available(editable)? && draft_current()? && input.is_current())
    }

    #[doc(hidden)]
    pub fn prepare_combo_edit(
        &self,
        input: InputRoute<'_>,
        editable: impl FnOnce() -> bool,
        draft_current: impl FnOnce() -> bool,
    ) -> bool {
        self.try_prepare_combo_edit(
            input,
            || Ok::<_, std::convert::Infallible>(editable()),
            || Ok(draft_current()),
        )
        .unwrap_or_else(|error| match error {})
    }

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
        node.standard_list_stamp == Some(source.contents_stamp())
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
