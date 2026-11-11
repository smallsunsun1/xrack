use crate::{DecimalSpinRange, Node, SpinDecimal};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Identity shared by all presentations of one component instance's Action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionKey(u64);

impl Default for ActionKey {
    fn default() -> Self {
        static NEXT_ACTION: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ACTION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("Action keys exhausted");
        Self(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpinAction {
    Increase,
    Decrease,
    Step(i64),
    Minimum,
    Maximum,
    Commit(i64),
    CommitIfCurrent {
        original: i64,
        value: i64,
    },
    CommitDecimal(SpinDecimal),
    CommitDecimalIfCurrent {
        original: Option<SpinDecimal>,
        value: SpinDecimal,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlAction {
    Spin(SpinAction),
    Calendar(crate::CalendarAction),
    Select(usize),
    ActivateRadioOption,
    PickTumbler {
        index: usize,
        row: crate::RowKey,
    },
    TableSelection(crate::TableSelectionAction),
    Tree(crate::TreeAction),
    Dialog(DialogRole),
    DialogCommand(DialogCommand),
    Drawer(bool),
    Popup(bool),
    SwipeDelegate(SwipeSide),
    DelayButton(bool),
    StackBack {
        revision: u64,
    },
    ActivateMenuItem,
    TextCommand(crate::TextCommand),
    ActivateTextLink(usize),
    ActivateAction,
    ActivateShortcut,
    AmbiguousShortcut,
    AcceptComboText,
    SubmitSearch,
    ClearSearch,
    SortColumn(usize),
    ResizeColumn {
        column: usize,
        width: u32,
    },
    ResizeRow {
        row: usize,
        key: crate::RowKey,
        height: u32,
    },
    MoveColumn {
        from: usize,
        to: usize,
    },
    MoveRow {
        from: usize,
        to: usize,
        row: crate::RowKey,
        stamp: (u64, u64),
    },
}

pub fn is_modal_overlay(kind: &str) -> bool {
    matches!(kind, "Dialog" | "Drawer")
}

pub fn is_window_overlay(kind: &str) -> bool {
    is_modal_overlay(kind) || kind == "PopupWindow"
}

#[doc(hidden)]
pub fn is_pane(kind: &str) -> bool {
    matches!(kind, "Pane" | "ControlFrame")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopupPlacement {
    #[default]
    Below,
    Above,
    Left,
    Right,
    Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSort {
    pub column: usize,
    pub descending: bool,
}

/// Visual positions map to the declaration indices used by cells and model actions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableColumnOrder(Option<Arc<ColumnPermutation>>);

#[derive(Debug, PartialEq, Eq)]
struct ColumnPermutation {
    columns: Vec<usize>,
    positions: Vec<usize>,
}

impl TableColumnOrder {
    pub fn as_slice(&self, count: usize) -> Option<&[usize]> {
        self.0
            .as_deref()
            .filter(|order| order.columns.len() == count)
            .map(|order| order.columns.as_slice())
    }

    pub fn iter(&self, count: usize) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
        let order = self.as_slice(count);
        (0..count).map(move |visual| order.map_or(visual, |order| order[visual]))
    }

    pub fn visual_index(&self, count: usize, column: usize) -> Option<usize> {
        if let Some(order) = self
            .0
            .as_deref()
            .filter(|order| order.columns.len() == count)
        {
            order.positions.get(column).copied()
        } else {
            (column < count).then_some(column)
        }
    }

    #[doc(hidden)]
    pub fn moved(&self, count: usize, from: usize, to: usize) -> Option<(usize, Self)> {
        if from >= count || to >= count || from == to {
            return None;
        }
        let mut order: Vec<_> = self.iter(count).collect();
        let column = order.remove(from);
        order.insert(to, column);
        let reordered = order.iter().copied().ne(0..count);
        Some((
            column,
            Self(reordered.then(|| {
                let mut positions = vec![0; count];
                for (visual, &logical) in order.iter().enumerate() {
                    positions[logical] = visual;
                }
                Arc::new(ColumnPermutation {
                    columns: order,
                    positions,
                })
            })),
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableHeader {
    pub columns: Vec<Node>,
    pub sort: Option<TableSort>,
    pub column_widths: Vec<Option<u32>>,
}

pub fn resizable_table_column(table: &Node, column: usize) -> Option<&Node> {
    let definition = table.table_header.as_ref()?.columns.get(column)?;
    (table.boolean("resizable-columns", false) && definition.boolean("resizable", true))
        .then_some(definition)
}

impl TableHeader {
    pub fn next_sort(&self, column: usize) -> Option<TableSort> {
        let definition = self.columns.get(column)?;
        definition.boolean("sortable", true).then_some(TableSort {
            column,
            descending: self
                .sort
                .is_some_and(|sort| sort.column == column && !sort.descending),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogRole {
    #[default]
    Accept,
    Reject,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwipeSide {
    #[default]
    Closed,
    Left,
    Right,
}

impl SwipeSide {
    pub fn panel(self) -> Option<&'static str> {
        match self {
            Self::Closed => None,
            Self::Left => Some("SwipeLeft"),
            Self::Right => Some("SwipeRight"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogCommand {
    Apply,
    Reset,
    Help,
    Discard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StandardButtonKind {
    #[default]
    Ok,
    Open,
    Save,
    SaveAll,
    Cancel,
    Close,
    Apply,
    Reset,
    RestoreDefaults,
    Help,
    Yes,
    YesToAll,
    No,
    NoToAll,
    Abort,
    Retry,
    Ignore,
    Discard,
}

impl StandardButtonKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Open => "Open",
            Self::Save => "Save",
            Self::SaveAll => "Save All",
            Self::Cancel => "Cancel",
            Self::Close => "Close",
            Self::Apply => "Apply",
            Self::Reset => "Reset",
            Self::RestoreDefaults => "Restore Defaults",
            Self::Help => "Help",
            Self::Yes => "Yes",
            Self::YesToAll => "Yes to All",
            Self::No => "No",
            Self::NoToAll => "No to All",
            Self::Abort => "Abort",
            Self::Retry => "Retry",
            Self::Ignore => "Ignore",
            Self::Discard => "Discard",
        }
    }

    fn dialog_action(self) -> ControlAction {
        match self {
            Self::Ok
            | Self::Open
            | Self::Save
            | Self::SaveAll
            | Self::Yes
            | Self::YesToAll
            | Self::Retry
            | Self::Ignore => ControlAction::Dialog(DialogRole::Accept),
            Self::Cancel | Self::Close | Self::No | Self::NoToAll | Self::Abort => {
                ControlAction::Dialog(DialogRole::Reject)
            }
            Self::Apply => ControlAction::DialogCommand(DialogCommand::Apply),
            Self::Reset | Self::RestoreDefaults => {
                ControlAction::DialogCommand(DialogCommand::Reset)
            }
            Self::Help => ControlAction::DialogCommand(DialogCommand::Help),
            Self::Discard => ControlAction::DialogCommand(DialogCommand::Discard),
        }
    }
}

pub fn dialog_button_action(node: &Node) -> Option<ControlAction> {
    if node.kind == "DialogButton" {
        return Some(ControlAction::Dialog(node.dialog_role()));
    }
    node.standard_button_kind()
        .map(StandardButtonKind::dialog_action)
}

pub fn is_button(kind: &str) -> bool {
    matches!(
        kind,
        "Button" | "ToolButton" | "RoundButton" | "DialogButton" | "DelayButton"
    )
}

pub fn delay_millis(node: &Node) -> u64 {
    node.int("delay").unwrap_or(3_000).clamp(0, 60_000) as u64
}

pub fn button_text(node: &Node) -> &str {
    node.text("text").unwrap_or_else(|| {
        if let Some(kind) = node.standard_button_kind() {
            return kind.label();
        }
        match (node.kind, node.dialog_role()) {
            ("DialogButton", DialogRole::Accept) => "OK",
            ("DialogButton", DialogRole::Reject) => "Cancel",
            _ => "",
        }
    })
}

pub struct SpinRange {
    min: i64,
    max: i64,
    value: i64,
    original: Option<i64>,
    step: i64,
    wrap: bool,
}

impl SpinRange {
    pub fn from_node(node: &Node) -> Self {
        let min = node.int("minimum").unwrap_or(0);
        let max = node.int("maximum").unwrap_or(100).max(min);
        let original = node.int("value");
        Self {
            min,
            max,
            value: original.unwrap_or(min).clamp(min, max),
            original,
            step: node.int("step-size").unwrap_or(1).max(0),
            wrap: node.boolean("wrap", false),
        }
    }

    pub fn value(&self) -> i64 {
        self.value
    }

    pub fn apply(&self, action: SpinAction) -> Option<i64> {
        let next = match action {
            SpinAction::Increase
                if self.wrap
                    && self
                        .value
                        .checked_add(self.step)
                        .is_none_or(|value| value > self.max) =>
            {
                self.min
            }
            SpinAction::Decrease
                if self.wrap
                    && self
                        .value
                        .checked_sub(self.step)
                        .is_none_or(|value| value < self.min) =>
            {
                self.max
            }
            SpinAction::Increase => self.value.saturating_add(self.step),
            SpinAction::Decrease => self.value.saturating_sub(self.step),
            SpinAction::Step(steps) => {
                let next = i128::from(self.value) + i128::from(self.step) * i128::from(steps);
                let min = i128::from(self.min);
                let max = i128::from(self.max);
                let step = i128::from(self.step);
                let value = i128::from(self.value);
                if self.wrap && next > max {
                    let remaining = i128::from(steps) - (max - value) / step - 1;
                    (min + remaining % ((max - min) / step + 1) * step) as i64
                } else if self.wrap && next < min {
                    let remaining = -i128::from(steps) - (value - min) / step - 1;
                    (max - remaining % ((max - min) / step + 1) * step) as i64
                } else {
                    next.clamp(min, max) as i64
                }
            }
            SpinAction::Minimum => self.min,
            SpinAction::Maximum => self.max,
            SpinAction::Commit(value) => value,
            SpinAction::CommitIfCurrent { original, value } if self.original == Some(original) => {
                value
            }
            SpinAction::CommitIfCurrent { .. } => return None,
            SpinAction::CommitDecimal(_) | SpinAction::CommitDecimalIfCurrent { .. } => {
                return None;
            }
        }
        .clamp(self.min, self.max);
        (next != self.value).then_some(next)
    }
}

pub fn spin_value_after(node: &Node, action: SpinAction) -> Option<i64> {
    if node.boolean("read-only", false)
        || matches!(
            action,
            SpinAction::Commit(_) | SpinAction::CommitIfCurrent { .. }
        ) && !spin_editable(node)
    {
        return None;
    }
    SpinRange::from_node(node).apply(action)
}

pub fn is_spin_box(kind: &str) -> bool {
    matches!(kind, "SpinBox" | "DoubleSpinBox")
}

pub fn spin_editable(node: &Node) -> bool {
    is_spin_box(node.kind) && node.boolean("editable", true) && !node.boolean("read-only", false)
}

pub fn spin_text(node: &Node) -> String {
    if node.kind == "DoubleSpinBox" {
        DecimalSpinRange::from_node(node).display()
    } else {
        SpinRange::from_node(node).value().to_string()
    }
}

pub fn combo_editable(node: &Node) -> bool {
    node.kind == "ComboBox" && node.boolean("editable", false)
}

pub fn text_input_value(node: &Node) -> &str {
    if combo_editable(node) {
        node.text("edit-text")
            .unwrap_or_else(|| crate::combo_text(node))
    } else {
        node.text("text").unwrap_or("")
    }
}

/// Suggestions can be tagged with the query that produced them.
pub fn search_suggestions(node: &Node) -> &[String] {
    let text = text_input_value(node);
    if node.kind != "SearchField"
        || node.boolean("read-only", false)
        || text.is_empty()
        || node
            .text("suggestions-for")
            .is_some_and(|query| query != text)
    {
        &[]
    } else {
        node.strings("suggestions")
    }
}

pub fn search_suggestion(node: &Node, index: usize) -> Option<&str> {
    let value = search_suggestions(node).get(index)?;
    (!value.is_empty() && !value.chars().any(char::is_control)).then_some(value.as_str())
}

pub fn page_indicator_count(node: &Node) -> usize {
    usize::try_from(node.int("count").unwrap_or(0).max(0)).unwrap_or(usize::MAX)
}

pub fn page_indicator_index(node: &Node) -> Option<usize> {
    let index = usize::try_from(node.int("current-index").unwrap_or(0)).ok()?;
    (index < page_indicator_count(node)).then_some(index)
}

/// Resolve an eligible option without giving each option independent selection state.
pub fn group_item(node: &Node, index: usize) -> Option<&Node> {
    let expected = match node.kind {
        "RadioGroup" => "RadioButton",
        "TabWidget" => "Tab",
        "TabBar" => "TabButton",
        "SwipeView" => "SwipePage",
        _ => return None,
    };
    node.children.get(index).filter(|item| {
        item.kind == expected && item.boolean("visible", true) && item.boolean("enabled", true)
    })
}

pub fn group_index(node: &Node) -> Option<usize> {
    if matches!(node.kind, "SwipeView" | "TabBar") {
        return resolve_swipe_index(
            node.int("current-index"),
            (0..node.children.len()).map(|index| group_item(node, index).is_some()),
        );
    }
    if node.kind == "RadioGroup"
        && let Some(value) = node.text("current-value")
    {
        return node.children.iter().enumerate().find_map(|(index, _)| {
            group_item(node, index)
                .filter(|item| crate::radio_value(item) == value)
                .map(|_| index)
        });
    }
    let default = if node.kind == "RadioGroup" { -1 } else { 0 };
    let index = usize::try_from(node.int("current-index").unwrap_or(default)).ok()?;
    group_item(node, index).map(|_| index)
}

/// Preserve declared page indexes while falling back to the first available page.
#[doc(hidden)]
pub fn resolve_swipe_index(
    requested: Option<i64>,
    available: impl IntoIterator<Item = bool>,
) -> Option<usize> {
    let requested = requested.and_then(|index| usize::try_from(index).ok());
    let mut first = None;
    for (index, available) in available.into_iter().enumerate() {
        if available {
            if requested == Some(index) {
                return Some(index);
            }
            first.get_or_insert(index);
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_steps_preserve_precision_and_handle_overflow() {
        let mut node = Node::new("SpinBox", None);
        node.properties.insert("value", (i64::MAX - 4).into());
        node.properties.insert("maximum", i64::MAX.into());
        node.properties.insert("step-size", 3_i64.into());
        assert_eq!(
            SpinRange::from_node(&node).apply(SpinAction::Increase),
            Some(i64::MAX - 1)
        );
        node.properties.insert("value", (i64::MAX - 1).into());
        assert_eq!(
            SpinRange::from_node(&node).apply(SpinAction::Increase),
            Some(i64::MAX)
        );
        node.properties.insert("wrap", true.into());
        assert_eq!(
            SpinRange::from_node(&node).apply(SpinAction::Increase),
            Some(0)
        );
    }
}
