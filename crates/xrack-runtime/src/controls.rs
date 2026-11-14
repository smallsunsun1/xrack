use crate::{
    DecimalSpinRange, InputRoute, ListModel, Node, Property, RoutedEvent, RowKey, SpinDecimal,
    TableRows, Value,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Identity shared by all presentations of one component instance's Action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionKey(u64);

impl ActionKey {
    #[doc(hidden)]
    pub fn present_with(
        self,
        node: &mut Node,
        checkable: impl FnOnce() -> bool,
        enabled: impl FnOnce() -> bool,
        checked: impl FnOnce(bool) -> bool,
    ) {
        self.try_present_with(
            node,
            || Ok::<_, std::convert::Infallible>(checkable()),
            || Ok(enabled()),
            |checkable| Ok(checked(checkable)),
        )
        .unwrap_or_else(|error| match error {});
    }

    /// Resolve presentation bindings in their render order.
    #[doc(hidden)]
    pub fn try_present_with<E>(
        self,
        node: &mut Node,
        checkable: impl FnOnce() -> Result<bool, E>,
        enabled: impl FnOnce() -> Result<bool, E>,
        checked: impl FnOnce(bool) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let checkable = checkable()?;
        let enabled = enabled()?;
        let checked = checked(checkable)?;
        self.present(node, enabled, checkable, checked);
        Ok(enabled)
    }

    #[doc(hidden)]
    pub fn present(self, node: &mut crate::Node, enabled: bool, checkable: bool, checked: bool) {
        node.action = Some(self);
        node.properties.insert("enabled", enabled.into());
        node.properties.insert("checkable", checkable.into());
        node.properties.insert("checked", checked.into());
    }
}

/// Return a pending checked update only while the activation bindings remain current.
#[doc(hidden)]
pub fn try_prepare_action_activation<E>(
    enabled: impl FnOnce() -> Result<bool, E>,
    checkable: impl FnOnce() -> Result<bool, E>,
    checked: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<Option<bool>>, E> {
    crate::property::watch_reads(|reads| {
        if !enabled()? || reads.changed() {
            return Ok(None);
        }
        let checkable = checkable()?;
        if reads.changed() {
            return Ok(None);
        }
        let next = if checkable { Some(!checked()?) } else { None };
        Ok((!reads.changed()).then_some(next))
    })
}

#[doc(hidden)]
pub fn prepare_action_activation(
    enabled: impl FnOnce() -> bool,
    checkable: impl FnOnce() -> bool,
    checked: impl FnOnce() -> bool,
) -> Option<Option<bool>> {
    try_prepare_action_activation(
        || Ok::<_, std::convert::Infallible>(enabled()),
        || Ok(checkable()),
        || Ok(checked()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Read the next expansion state while the group and its input route remain current.
#[doc(hidden)]
pub fn try_prepare_group_box_toggle<E>(
    input: Option<InputRoute<'_>>,
    checkable: impl FnOnce() -> Result<bool, E>,
    checked: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<bool>, E> {
    if input.is_some_and(|input| !input.is_current()) {
        return Ok(None);
    }
    Ok(
        try_prepare_action_activation(|| Ok(true), checkable, checked)?
            .flatten()
            .filter(|_| input.is_none_or(InputRoute::is_current)),
    )
}

#[doc(hidden)]
pub fn prepare_group_box_toggle(
    input: Option<InputRoute<'_>>,
    checkable: impl FnOnce() -> bool,
    checked: impl FnOnce() -> bool,
) -> Option<bool> {
    try_prepare_group_box_toggle(
        input,
        || Ok::<_, std::convert::Infallible>(checkable()),
        || Ok(checked()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Resolve an activation target without allowing bindings to invalidate it mid-read.
#[doc(hidden)]
pub fn try_prepare_action_view<T, E>(
    read: impl FnOnce() -> Result<Option<T>, E>,
) -> Result<Option<T>, E> {
    crate::property::watch_reads(|reads| Ok(read()?.filter(|_| !reads.changed())))
}

#[doc(hidden)]
pub fn prepare_action_view<T>(read: impl FnOnce() -> Option<T>) -> Option<T> {
    try_prepare_action_view(|| Ok::<_, std::convert::Infallible>(read()))
        .unwrap_or_else(|error| match error {})
}

/// Sample toggle settings before history, with the same input guard as the write.
#[doc(hidden)]
pub fn try_prepare_row_toggle<E>(
    node: &Node,
    input: crate::InputRoute<'_>,
    tristate: Option<impl FnOnce() -> Result<bool, E>>,
    history: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<bool>, E> {
    try_prepare_row_toggle_matching(node, input, tristate, || true, history)
}

/// Revalidate the displayed field after reading settings and before recording history.
#[doc(hidden)]
pub fn try_prepare_row_toggle_matching<E>(
    node: &Node,
    input: crate::InputRoute<'_>,
    tristate: Option<impl FnOnce() -> Result<bool, E>>,
    field_current: impl FnOnce() -> bool,
    history: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<bool>, E> {
    input.try_prepare(
        || match tristate {
            Some(read) => Ok(read()? == node.boolean("tristate", false)),
            None => Ok(true),
        },
        || {
            if !field_current() {
                return Ok(None);
            }
            history().map(Some)
        },
    )
}

#[doc(hidden)]
pub fn prepare_row_toggle(
    node: &Node,
    input: crate::InputRoute<'_>,
    tristate: Option<impl FnOnce() -> bool>,
    history: impl FnOnce() -> bool,
) -> Option<bool> {
    prepare_row_toggle_matching(node, input, tristate, || true, history)
}

#[doc(hidden)]
pub fn prepare_row_toggle_matching(
    node: &Node,
    input: crate::InputRoute<'_>,
    tristate: Option<impl FnOnce() -> bool>,
    field_current: impl FnOnce() -> bool,
    history: impl FnOnce() -> bool,
) -> Option<bool> {
    try_prepare_row_toggle_matching(
        node,
        input,
        tristate.map(|read| move || Ok::<_, std::convert::Infallible>(read())),
        field_current,
        || Ok(history()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Resolve an index only while the control and its options remain current.
#[doc(hidden)]
pub fn try_prepare_index_selection<E>(
    index: usize,
    available: impl FnOnce() -> Result<bool, E>,
    contains: impl FnOnce(i64) -> Result<bool, E>,
    current: impl FnOnce() -> Result<i64, E>,
) -> Result<Option<i64>, E> {
    let Ok(index) = i64::try_from(index) else {
        return Ok(None);
    };
    crate::property::watch_reads(|reads| {
        if !available()? || reads.changed() || !contains(index)? || reads.changed() {
            return Ok(None);
        }
        Ok((current()? != index && !reads.changed()).then_some(index))
    })
}

#[doc(hidden)]
pub fn prepare_index_selection(
    index: usize,
    available: impl FnOnce() -> bool,
    contains: impl FnOnce(i64) -> bool,
    current: impl FnOnce() -> i64,
) -> Option<i64> {
    try_prepare_index_selection(
        index,
        || Ok::<_, std::convert::Infallible>(available()),
        |index| Ok(contains(index)),
        || Ok(current()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Resolve a source row and discard the selection if its projection changes during the cursor read.
#[doc(hidden)]
pub fn try_prepare_source_selection<E>(
    resolve: impl FnOnce() -> Result<Option<usize>, E>,
    current: impl FnOnce() -> Result<i64, E>,
) -> Result<Option<i64>, E> {
    try_prepare_action_view(|| {
        let Some(index) = resolve()?.and_then(|index| i64::try_from(index).ok()) else {
            return Ok(None);
        };
        Ok((current()? != index).then_some(index))
    })
}

#[doc(hidden)]
pub fn prepare_source_selection(
    resolve: impl FnOnce() -> Option<usize>,
    current: impl FnOnce() -> i64,
) -> Option<i64> {
    try_prepare_source_selection(
        || Ok::<_, std::convert::Infallible>(resolve()),
        || Ok(current()),
    )
    .unwrap_or_else(|error| match error {})
}

impl Node {
    /// A disabled Action forces its presentation's state-controlled enabled value.
    #[doc(hidden)]
    pub fn action_state_override(&self, property: &str) -> Option<crate::Value> {
        (property == "enabled" && self.action.is_some() && !self.boolean("enabled", true))
            .then_some(crate::Value::Bool(false))
    }
}

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

impl SpinAction {
    /// Read the live control value only for a commit that carries an original value.
    #[doc(hidden)]
    pub fn try_accepts_current<E>(
        self,
        current: impl FnOnce() -> Result<Value, E>,
    ) -> Result<bool, E> {
        Ok(match self {
            Self::CommitIfCurrent { original, .. } => {
                matches!(current()?, Value::Int(value) if value == original)
            }
            Self::CommitDecimalIfCurrent { original, .. } => {
                matches!(current()?, Value::Float(value) if SpinDecimal::new(value) == original)
            }
            _ => true,
        })
    }

    #[doc(hidden)]
    pub fn accepts_current(self, current: impl FnOnce() -> Value) -> bool {
        self.try_accepts_current(|| Ok::<_, std::convert::Infallible>(current()))
            .unwrap_or_else(|error| match error {})
    }
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

/// A swipe may reveal a panel only if it was visible in the displayed scene
/// and is still visible after reading the live open side.
#[doc(hidden)]
pub fn try_prepare_swipe_side<E>(
    node: &Node,
    side: SwipeSide,
    input: InputRoute<'_>,
    current: impl FnOnce() -> Result<SwipeSide, E>,
    panel_visible: impl FnOnce(&str) -> Result<bool, E>,
) -> Result<bool, E> {
    if !input.is_current() || !node.accepts_swipe_side(current()?, side) || !input.is_current() {
        return Ok(false);
    }
    if let Some(panel) = side.panel()
        && !panel_visible(panel)?
    {
        return Ok(false);
    }
    Ok(input.is_current())
}

#[doc(hidden)]
pub fn prepare_swipe_side(
    node: &Node,
    side: SwipeSide,
    input: InputRoute<'_>,
    current: impl FnOnce() -> SwipeSide,
    panel_visible: impl FnOnce(&str) -> bool,
) -> bool {
    try_prepare_swipe_side(
        node,
        side,
        input,
        || Ok::<_, std::convert::Infallible>(current()),
        |panel| Ok(panel_visible(panel)),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_set_overlay_open<E>(
    open: bool,
    input: Option<InputRoute<'_>>,
    current: impl FnOnce() -> Result<bool, E>,
    set_open: impl FnOnce(bool) -> Result<(), E>,
    notify: impl FnOnce(bool) -> Result<(), E>,
) -> Result<bool, E> {
    if current()? == open || input.is_some_and(|input| !input.is_current()) {
        return Ok(false);
    }
    set_open(open)?;
    notify(open)?;
    Ok(true)
}

#[doc(hidden)]
pub fn set_overlay_open(
    open: bool,
    input: Option<InputRoute<'_>>,
    current: impl FnOnce() -> bool,
    set_open: impl FnOnce(bool),
    notify: impl FnOnce(bool),
) -> bool {
    try_set_overlay_open(
        open,
        input,
        || Ok::<_, std::convert::Infallible>(current()),
        |open| {
            set_open(open);
            Ok(())
        },
        |open| {
            notify(open);
            Ok(())
        },
    )
    .unwrap_or_else(|error| match error {})
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

impl TableSort {
    #[doc(hidden)]
    pub fn from_index(column: i64, descending: bool) -> Option<Self> {
        Some(Self {
            column: usize::try_from(column).ok()?,
            descending,
        })
    }

    #[doc(hidden)]
    pub fn column_index(sort: Option<Self>) -> i64 {
        sort.map_or(-1, |sort| sort.column as i64)
    }

    #[doc(hidden)]
    pub fn is_descending(sort: Option<Self>) -> bool {
        sort.is_some_and(|sort| sort.descending)
    }
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

#[doc(hidden)]
pub fn resize_table_column(widths: &Property<Vec<Option<u32>>>, column: usize, width: u32) -> bool {
    let mut next = widths.get(Vec::new);
    if next.get(column) == Some(&Some(width)) {
        return false;
    }
    if next.len() <= column {
        next.resize(column + 1, None);
    }
    next[column] = Some(width);
    widths.set(next);
    true
}

/// Validate a drag against both the originating scene and the current bindings.
#[doc(hidden)]
pub fn try_table_drag_available<E>(
    table: &Node,
    input: InputRoute<'_>,
    flag: impl FnOnce(&str) -> Result<bool, E>,
    column_resizable: impl FnOnce(usize) -> Result<bool, E>,
) -> Result<bool, E> {
    let (property, column) = match *input.event() {
        RoutedEvent::Control(ControlAction::ResizeColumn { column, .. }) => {
            if resizable_table_column(table, column).is_none() {
                return Ok(false);
            }
            ("resizable-columns", Some(column))
        }
        RoutedEvent::Control(ControlAction::ResizeRow { .. }) => ("resizable-rows", None),
        RoutedEvent::Control(ControlAction::MoveColumn { .. }) => ("movable-columns", None),
        RoutedEvent::Control(ControlAction::MoveRow { stamp, .. }) => {
            if table.standard_list_stamp != Some(stamp) {
                return Ok(false);
            }
            ("movable-rows", None)
        }
        _ => return Ok(true),
    };
    if !table.boolean(property, false) {
        return Ok(false);
    }
    input.try_available(|| {
        if !flag(property)? {
            return Ok(false);
        }
        column.map_or(Ok(true), column_resizable)
    })
}

#[doc(hidden)]
pub fn table_drag_available(
    table: &Node,
    input: InputRoute<'_>,
    flag: impl FnOnce(&str) -> bool,
    column_resizable: impl FnOnce(usize) -> bool,
) -> bool {
    try_table_drag_available(
        table,
        input,
        |name| Ok::<_, std::convert::Infallible>(flag(name)),
        |column| Ok(column_resizable(column)),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub enum TableHeaderAction {
    Sort(TableSort),
    MoveColumn {
        from: usize,
        to: usize,
    },
    ResizeColumn {
        column: usize,
        width: u32,
    },
    ResizeRow {
        row: usize,
        key: RowKey,
        height: u32,
    },
}

impl TableHeaderAction {
    /// Reject stale header actions before reading the row projection.
    pub fn try_prepare<M, E>(
        node: &Node,
        input: InputRoute<'_>,
        action: ControlAction,
        sort: impl FnOnce() -> Option<TableSort>,
        order: impl FnOnce() -> TableColumnOrder,
        rows: impl FnOnce() -> Result<TableRows<M>, E>,
    ) -> Result<Option<(Self, TableRows<M>)>, E> {
        let action = match action {
            ControlAction::SortColumn(column) => {
                let Some(sort) = node
                    .table_header
                    .as_ref()
                    .and_then(|header| header.next_sort_if_current(column, sort()))
                else {
                    return Ok(None);
                };
                Self::Sort(sort)
            }
            ControlAction::MoveColumn { from, to } => {
                if !node.table_column_move_is_current(from, to, &order()) {
                    return Ok(None);
                }
                Self::MoveColumn { from, to }
            }
            ControlAction::ResizeColumn { column, width } => Self::ResizeColumn { column, width },
            ControlAction::ResizeRow { row, key, height } => Self::ResizeRow { row, key, height },
            _ => return Ok(None),
        };
        let rows = rows()?;
        Ok(
            (input.is_current() && node.standard_list_stamp == Some(rows.positions().stamp()))
                .then_some((action, rows)),
        )
    }

    pub fn prepare<M>(
        node: &Node,
        input: InputRoute<'_>,
        action: ControlAction,
        sort: impl FnOnce() -> Option<TableSort>,
        order: impl FnOnce() -> TableColumnOrder,
        rows: impl FnOnce() -> TableRows<M>,
    ) -> Option<(Self, TableRows<M>)> {
        Self::try_prepare(node, input, action, sort, order, || {
            Ok::<_, std::convert::Infallible>(rows())
        })
        .unwrap_or_else(|error| match error {})
    }
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

    /// Resolve a click using the sort indicator the user actually saw.
    #[doc(hidden)]
    pub fn next_sort_if_current(
        &self,
        column: usize,
        current: Option<TableSort>,
    ) -> Option<TableSort> {
        if self.sort != current {
            return None;
        }
        self.next_sort(column)
    }
}

impl Node {
    /// A column move uses visual positions from the originating scene.
    #[doc(hidden)]
    pub fn table_column_move_is_current(
        &self,
        from: usize,
        to: usize,
        current: &TableColumnOrder,
    ) -> bool {
        self.table_column_order == *current
            && from != to
            && self
                .table_header
                .as_ref()
                .is_some_and(|header| from < header.columns.len() && to < header.columns.len())
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

#[doc(hidden)]
pub enum DialogEvent {
    Accepted,
    Rejected,
    Applied,
    Reset,
    HelpRequested,
    Discarded,
}

impl DialogEvent {
    pub fn handler(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Reset => "reset",
            Self::HelpRequested => "help-requested",
            Self::Discarded => "discarded",
        }
    }

    pub fn try_deliver<E>(
        action: ControlAction,
        input: InputRoute<'_>,
        open: impl FnOnce() -> Result<bool, E>,
        close: impl FnOnce() -> Result<(), E>,
        notify: impl FnOnce(Self) -> Result<(), E>,
    ) -> Result<bool, E> {
        if !open()? || !input.is_current() {
            return Ok(false);
        }
        let event = match action {
            ControlAction::Dialog(DialogRole::Accept) => Self::Accepted,
            ControlAction::Dialog(DialogRole::Reject) => Self::Rejected,
            ControlAction::DialogCommand(DialogCommand::Apply) => Self::Applied,
            ControlAction::DialogCommand(DialogCommand::Reset) => Self::Reset,
            ControlAction::DialogCommand(DialogCommand::Help) => Self::HelpRequested,
            ControlAction::DialogCommand(DialogCommand::Discard) => Self::Discarded,
            _ => return Ok(false),
        };
        if matches!(event, Self::Accepted | Self::Rejected | Self::Discarded) {
            close()?;
        }
        notify(event)?;
        Ok(true)
    }

    pub fn deliver(
        action: ControlAction,
        input: InputRoute<'_>,
        open: impl FnOnce() -> bool,
        close: impl FnOnce(),
        notify: impl FnOnce(Self),
    ) -> bool {
        Self::try_deliver(
            action,
            input,
            || Ok::<_, std::convert::Infallible>(open()),
            || {
                close();
                Ok(())
            },
            |event| {
                notify(event);
                Ok(())
            },
        )
        .unwrap_or_else(|error| match error {})
    }
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
    #[doc(hidden)]
    pub fn accepts_message(self) -> bool {
        matches!(self, Self::Ok | Self::Yes)
    }

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
    normalized_delay_millis(node.int("delay"))
}

fn normalized_delay_millis(delay: Option<i64>) -> u64 {
    delay.unwrap_or(3_000).clamp(0, 60_000) as u64
}

/// Confirm that a pending hold or reset still applies to the live button.
#[doc(hidden)]
pub fn try_delay_button_transition_is_current<E>(
    node: &Node,
    checked: bool,
    available: impl FnOnce() -> Result<bool, E>,
    delay: impl FnOnce() -> Result<Option<i64>, E>,
    current: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    crate::property::watch_reads(|reads| {
        if !available()? || reads.changed() {
            return Ok(false);
        }
        if checked && (normalized_delay_millis(delay()?) != delay_millis(node) || reads.changed()) {
            return Ok(false);
        }
        Ok(current()? != checked && !reads.changed())
    })
}

#[doc(hidden)]
pub fn delay_button_transition_is_current(
    node: &Node,
    checked: bool,
    available: impl FnOnce() -> bool,
    delay: impl FnOnce() -> Option<i64>,
    current: impl FnOnce() -> bool,
) -> bool {
    try_delay_button_transition_is_current(
        node,
        checked,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(delay()),
        || Ok(current()),
    )
    .unwrap_or_else(|error| match error {})
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

#[doc(hidden)]
pub const SPIN_VIEW_PROPERTIES: &[&str] = &[
    "read-only",
    "editable",
    "minimum",
    "maximum",
    "decimals",
    "decimal-separator",
    "step-size",
    "wrap",
];

/// Check that the options used to interpret an input are still current.
#[doc(hidden)]
pub fn try_spin_view_is_current<E>(
    node: &Node,
    action: SpinAction,
    available: impl FnOnce() -> Result<bool, E>,
    current: impl FnOnce() -> Result<Value, E>,
    read: impl FnMut(&str) -> Result<Option<Value>, E>,
) -> Result<bool, E> {
    try_prepare_spin_view(node, action, available, current, read, || Ok(Some(())))
        .map(|prepared| prepared.is_some())
}

#[doc(hidden)]
pub fn try_prepare_spin_view<T, E>(
    node: &Node,
    action: SpinAction,
    available: impl FnOnce() -> Result<bool, E>,
    current: impl FnOnce() -> Result<Value, E>,
    mut read: impl FnMut(&str) -> Result<Option<Value>, E>,
    prepare: impl FnOnce() -> Result<Option<T>, E>,
) -> Result<Option<T>, E> {
    let decimal = node.kind == "DoubleSpinBox";
    let commit = matches!(
        action,
        SpinAction::Commit(_)
            | SpinAction::CommitIfCurrent { .. }
            | SpinAction::CommitDecimal(_)
            | SpinAction::CommitDecimalIfCurrent { .. }
    );
    let step = matches!(
        action,
        SpinAction::Increase | SpinAction::Decrease | SpinAction::Step(_)
    );
    crate::property::watch_reads(|reads| {
        if !available()? || reads.changed() {
            return Ok(None);
        }
        for &name in SPIN_VIEW_PROPERTIES {
            let used = match name {
                "editable" => commit,
                "decimals" => decimal,
                "decimal-separator" => decimal && commit,
                "step-size" | "wrap" => step,
                _ => true,
            };
            if used && (read(name)?.as_ref() != node.properties.get(name) || reads.changed()) {
                return Ok(None);
            }
        }
        if !action.try_accepts_current(current)? || reads.changed() {
            return Ok(None);
        }
        Ok(prepare()?.filter(|_| !reads.changed()))
    })
}

#[doc(hidden)]
pub fn spin_view_is_current(
    node: &Node,
    action: SpinAction,
    available: impl FnOnce() -> bool,
    current: impl FnOnce() -> Value,
    mut read: impl FnMut(&str) -> Option<Value>,
) -> bool {
    try_spin_view_is_current(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(current()),
        |name| Ok(read(name)),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn prepare_spin_view<T>(
    node: &Node,
    action: SpinAction,
    available: impl FnOnce() -> bool,
    current: impl FnOnce() -> Value,
    mut read: impl FnMut(&str) -> Option<Value>,
    prepare: impl FnOnce() -> Option<T>,
) -> Option<T> {
    try_prepare_spin_view(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        || Ok(current()),
        |name| Ok(read(name)),
        || Ok(prepare()),
    )
    .unwrap_or_else(|error| match error {})
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

/// Resolve a selectable option that is distinct from the scene's selected option.
#[doc(hidden)]
pub fn group_selection_item(node: &Node, index: usize) -> Option<&Node> {
    let item = group_item(node, index)?;
    (group_index(node) != Some(index)).then_some(item)
}

/// Validate the displayed option against the live group before committing a selection.
#[doc(hidden)]
pub fn try_prepare_group_selection<E>(
    node: &Node,
    index: usize,
    route: crate::InputRoute<'_>,
    model_current: impl FnOnce() -> Result<bool, E>,
    current: impl FnOnce() -> Result<crate::ButtonGroupSelection, E>,
    mut option: impl FnMut(usize, bool) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    let Some(shown) = group_item(node, index) else {
        return Ok(None);
    };
    let radio = node.kind == "RadioGroup";
    if !route.try_available(model_current)? {
        return Ok(None);
    }
    let Some(label) = option(index, radio)? else {
        return Ok(None);
    };
    if !route.is_current() || (radio && label != crate::radio_value(shown)) {
        return Ok(None);
    }
    let current = current()?;
    if !route.is_current() {
        return Ok(None);
    }
    let unchanged = match current {
        crate::ButtonGroupSelection::Value(value) => value == label,
        crate::ButtonGroupSelection::Index(current) => {
            let current = usize::try_from(current).ok();
            if current == Some(index) {
                true
            } else if matches!(node.kind, "TabBar" | "SwipeView") {
                let mut selected = false;
                let valid_current = if let Some(current) = current {
                    option(current, false)?.is_some()
                } else {
                    false
                };
                if !valid_current && route.is_current() {
                    selected = true;
                    for preceding in 0..index {
                        if option(preceding, false)?.is_some() || !route.is_current() {
                            selected = false;
                            break;
                        }
                    }
                }
                selected
            } else {
                false
            }
        }
    };
    Ok((!unchanged && route.is_current()).then_some(label))
}

#[doc(hidden)]
pub fn prepare_group_selection(
    node: &Node,
    index: usize,
    route: crate::InputRoute<'_>,
    model_current: impl FnOnce() -> bool,
    current: impl FnOnce() -> crate::ButtonGroupSelection,
    mut option: impl FnMut(usize, bool) -> Option<String>,
) -> Option<String> {
    try_prepare_group_selection(
        node,
        index,
        route,
        || Ok::<_, std::convert::Infallible>(model_current()),
        || Ok(current()),
        |index, label| Ok(option(index, label)),
    )
    .unwrap_or_else(|error| match error {})
}

impl Node {
    #[doc(hidden)]
    pub fn snapshot_group_options(&mut self, model: &impl ListModel) {
        self.standard_list_stamp = Some(model.contents_stamp());
    }

    #[doc(hidden)]
    pub fn group_options_are_current(&self, model: &impl ListModel) -> bool {
        self.standard_list_stamp == Some(model.contents_stamp())
    }
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

impl Node {
    /// Mount the same SwipePage that layout will select from the rendered page properties.
    #[doc(hidden)]
    pub fn mount_active_swipe_page<T>(
        &mut self,
        mount: impl FnOnce(usize, &mut Node) -> T,
    ) -> Option<T> {
        if self.kind != "SwipeView" || !self.is_visible() {
            return None;
        }
        let index = group_index(self)?;
        Some(mount(index, &mut self.children[index]))
    }
}

/// Preserve declared page indexes while falling back to the first available page.
#[doc(hidden)]
pub fn resolve_swipe_index(
    requested: Option<i64>,
    available: impl IntoIterator<Item = bool>,
) -> Option<usize> {
    try_resolve_swipe_index(
        requested,
        available.into_iter().map(Ok::<_, std::convert::Infallible>),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_resolve_swipe_index<E>(
    requested: Option<i64>,
    available: impl IntoIterator<Item = Result<bool, E>>,
) -> Result<Option<usize>, E> {
    let requested = requested.and_then(|index| usize::try_from(index).ok());
    let mut first = None;
    for (index, available) in available.into_iter().enumerate() {
        if available? {
            if requested == Some(index) {
                return Ok(Some(index));
            }
            first.get_or_insert(index);
        }
    }
    Ok(first)
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
