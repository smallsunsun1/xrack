use crate::{
    DecimalSpinRange, InputRoute, ListModel, Node, Property, RoutedEvent, RowKey, SpinDecimal,
    TableRows, Value,
};
use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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

/// Resolve a toggle or explicit expansion without acting on an old input route.
#[doc(hidden)]
pub fn try_prepare_expander_change<E>(
    input: Option<InputRoute<'_>>,
    requested: Option<bool>,
    expanded: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<bool>, E> {
    if input.is_some_and(|input| !input.is_current()) {
        return Ok(None);
    }
    crate::property::watch_reads(|reads| {
        let current = expanded()?;
        let next = requested.unwrap_or(!current);
        Ok(
            (next != current && !reads.changed() && input.is_none_or(InputRoute::is_current))
                .then_some(next),
        )
    })
}

#[doc(hidden)]
pub fn prepare_expander_change(
    input: Option<InputRoute<'_>>,
    requested: Option<bool>,
    expanded: impl FnOnce() -> bool,
) -> Option<bool> {
    try_prepare_expander_change(input, requested, || {
        Ok::<_, std::convert::Infallible>(expanded())
    })
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

impl crate::CheckState {
    /// Compute a toggle while its state and settings still describe the same input.
    #[doc(hidden)]
    pub fn try_prepare_toggle<E>(
        input: Option<InputRoute<'_>>,
        current: impl FnOnce() -> Result<Self, E>,
        tristate: impl FnOnce() -> Result<bool, E>,
    ) -> Result<Option<Self>, E> {
        let route_current = || input.is_none_or(InputRoute::is_current);
        if !route_current() {
            return Ok(None);
        }
        crate::property::watch_reads(|reads| {
            let current = current()?;
            if reads.changed() || !route_current() {
                return Ok(None);
            }
            let next = if let Some(crate::RoutedEvent::Control(ControlAction::SetSwitch(checked))) =
                input.map(InputRoute::event)
            {
                let desired = Self::from(*checked);
                (desired != current).then_some(desired)
            } else {
                Some(current.next(tristate()?))
            };
            Ok(next.filter(|_| !reads.changed() && route_current()))
        })
    }

    #[doc(hidden)]
    pub fn prepare_toggle(
        input: Option<InputRoute<'_>>,
        current: impl FnOnce() -> Self,
        tristate: impl FnOnce() -> bool,
    ) -> Option<Self> {
        Self::try_prepare_toggle(
            input,
            || Ok::<_, std::convert::Infallible>(current()),
            || Ok(tristate()),
        )
        .unwrap_or_else(|error| match error {})
    }
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
    #[expect(
        clippy::expect_used,
        reason = "action IDs must remain unique across all action instances"
    )]
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
    #[doc(hidden)]
    StepFromIntegerDraft {
        original: Option<i64>,
        draft: i64,
        steps: i64,
    },
    #[doc(hidden)]
    StepFromDecimalDraft {
        original: Option<SpinDecimal>,
        draft: SpinDecimal,
        steps: i64,
    },
}

impl SpinAction {
    /// Read the live control value when applying an action based on an earlier draft.
    #[doc(hidden)]
    pub fn try_accepts_current<E>(
        self,
        current: impl FnOnce() -> Result<Value, E>,
    ) -> Result<bool, E> {
        Ok(match self {
            Self::CommitIfCurrent { original, .. }
            | Self::StepFromIntegerDraft {
                original: Some(original),
                ..
            } => {
                matches!(current()?, Value::Int(value) if value == original)
            }
            Self::CommitDecimalIfCurrent { original, .. }
            | Self::StepFromDecimalDraft { original, .. } => {
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
    SetSwitch(bool),
    Expander(bool),
    Calendar(crate::CalendarAction),
    Select(usize),
    ActivateRadioOption,
    MoveTab(TabMove),
    CloseTab(TabClose),
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
    #[doc(hidden)]
    ValidateTextSubmission,
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
    crate::property::watch_reads(|reads| {
        if !input.is_current()
            || !node.accepts_swipe_side(current()?, side)
            || reads.changed()
            || !input.is_current()
        {
            return Ok(false);
        }
        if let Some(panel) = side.panel()
            && !panel_visible(panel)?
        {
            return Ok(false);
        }
        Ok(!reads.changed() && input.is_current())
    })
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
    let route_current = || input.is_none_or(InputRoute::is_current);
    let should_set = crate::property::watch_reads(|reads| {
        Ok(route_current() && current()? != open && !reads.changed() && route_current())
    })?;
    if !should_set {
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
            SpinAction::StepFromIntegerDraft {
                original,
                draft,
                steps,
            } if original.is_none() || original == self.original => {
                let value = draft.clamp(self.min, self.max);
                let range = Self { value, ..*self };
                range.apply(SpinAction::Step(steps)).unwrap_or(value)
            }
            SpinAction::CommitDecimal(_)
            | SpinAction::CommitDecimalIfCurrent { .. }
            | SpinAction::StepFromIntegerDraft { .. }
            | SpinAction::StepFromDecimalDraft { .. } => {
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
            SpinAction::Commit(_)
                | SpinAction::CommitIfCurrent { .. }
                | SpinAction::StepFromIntegerDraft { .. }
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
            | SpinAction::StepFromIntegerDraft { .. }
            | SpinAction::StepFromDecimalDraft { .. }
    );
    let step = matches!(
        action,
        SpinAction::Increase
            | SpinAction::Decrease
            | SpinAction::Step(_)
            | SpinAction::StepFromIntegerDraft { .. }
            | SpinAction::StepFromDecimalDraft { .. }
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

/// Select the adjacent page in logical order. An invalid selection starts at the first page.
#[doc(hidden)]
pub fn page_indicator_step(node: &Node, forward: bool) -> Option<usize> {
    if !node.boolean("interactive", false) || !node.boolean("enabled", true) {
        return None;
    }
    let last = page_indicator_count(node).checked_sub(1)?;
    let current = page_indicator_index(node);
    let next = current.map_or(0, |index| {
        if forward {
            (index + 1).min(last)
        } else {
            index.saturating_sub(1)
        }
    });
    (current != Some(next)).then_some(next)
}

/// Resolve an eligible option without giving each option independent selection state.
fn navigation_item_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "TabWidget" => Some("Tab"),
        "TabBar" => Some("TabButton"),
        "SwipeView" => Some("SwipePage"),
        _ => None,
    }
}

fn group_item_available(item: &Node, expected: &str) -> bool {
    item.kind == expected && item.boolean("visible", true) && item.boolean("enabled", true)
}

pub fn group_item(node: &Node, index: usize) -> Option<&Node> {
    let expected = if node.kind == "RadioGroup" {
        "RadioButton"
    } else {
        navigation_item_kind(node.kind)?
    };
    node.children
        .get(index)
        .filter(|item| group_item_available(item, expected))
}

fn navigation_index(
    kind: &str,
    requested: Option<&str>,
    index: Option<i64>,
    items: &[Node],
) -> Option<usize> {
    if kind == "StackLayout" {
        return match requested {
            Some(value) => {
                resolve_navigation_value(value, items.iter().map(|item| item.text("value")))
            }
            None => usize::try_from(index.unwrap_or(0))
                .ok()
                .filter(|index| *index < items.len()),
        };
    }
    let expected = navigation_item_kind(kind)?;
    let eligible = |item: &Node| group_item_available(item, expected);
    if let Some(value) = requested {
        return resolve_navigation_value(
            value,
            items
                .iter()
                .map(|item| eligible(item).then(|| item.text("value")).flatten()),
        );
    }
    if kind == "TabWidget" {
        let slot = usize::try_from(index.unwrap_or(0)).ok()?;
        return items.get(slot).filter(|item| eligible(item)).map(|_| slot);
    }
    resolve_swipe_index(index, items.iter().map(eligible))
}

#[doc(hidden)]
pub fn tab_is_closable(group: &Node, item: &Node) -> bool {
    matches!(
        (group.kind, item.kind),
        ("TabBar", "TabButton") | ("TabWidget", "Tab")
    ) && item.boolean("closable", group.boolean("tabs-closable", false))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabClose {
    pub index: usize,
    row: Option<crate::RowKey>,
    stamp: Option<(u64, u64)>,
}

impl Node {
    #[doc(hidden)]
    pub fn tab_close(&self, index: usize) -> Option<TabClose> {
        let item = group_item(self, index)?;
        tab_is_closable(self, item).then_some(TabClose {
            index,
            row: item.row_key,
            stamp: self.standard_list_stamp,
        })
    }
}

/// Validate an offered close target against the current model and its live bindings.
#[doc(hidden)]
pub fn try_prepare_tab_close<M, E>(
    node: &Node,
    request: TabClose,
    route: crate::InputRoute<'_>,
    model: impl FnOnce() -> Result<Option<M>, E>,
    read: impl FnOnce(&M, usize) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    if node.tab_close(request.index) != Some(request) {
        return Ok(None);
    }
    let expected = node
        .child_at(request.index)
        .and_then(|item| item.text("value"))
        .unwrap_or("");
    let mut label = None;
    let sample = || {
        if let Some(model) = model()? {
            label = read(&model, request.index)?.filter(|current| current == expected);
        }
        Ok(label.is_some())
    };
    let accepted = if route.checks_availability() {
        route.try_available(sample)?
    } else {
        sample()?
    };
    Ok(label.filter(|_| accepted && route.is_current()))
}

#[doc(hidden)]
pub fn prepare_tab_close<M>(
    node: &Node,
    request: TabClose,
    route: crate::InputRoute<'_>,
    model: impl FnOnce() -> Option<M>,
    read: impl FnOnce(&M, usize, bool) -> Option<String>,
) -> Option<String> {
    try_prepare_tab_close(
        node,
        request,
        route,
        || Ok::<_, std::convert::Infallible>(model()),
        |model, slot| Ok(read(model, slot, true)),
    )
    .unwrap_or_else(|error| match error {})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabMove {
    pub from: usize,
    pub to: usize,
    source: crate::RowKey,
    destination: crate::RowKey,
    stamp: (u64, u64),
}

#[doc(hidden)]
pub fn moved_navigation_index(from: usize, to: usize, index: i64) -> i64 {
    let Ok(slot) = usize::try_from(index) else {
        return index;
    };
    if slot == from {
        to as i64
    } else if from < slot && slot <= to {
        index - 1
    } else if to <= slot && slot < from {
        index + 1
    } else {
        index
    }
}

impl TabMove {
    pub fn moved_index(self, index: i64) -> i64 {
        moved_navigation_index(self.from, self.to, index)
    }

    #[doc(hidden)]
    pub fn matches_model(self, model: &impl ListModel) -> bool {
        self.stamp == model.contents_stamp()
            && model.key_at(self.from) == Some(self.source)
            && model.key_at(self.to) == Some(self.destination)
    }
}

impl Node {
    #[doc(hidden)]
    pub fn tab_move(&self, from: usize, to: usize) -> Option<TabMove> {
        if !matches!(self.kind, "TabBar" | "TabWidget")
            || !self.boolean("reorderable", false)
            || from == to
        {
            return None;
        }
        Some(TabMove {
            from,
            to,
            source: group_item(self, from)?.row_key?,
            destination: group_item(self, to)?.row_key?,
            stamp: self.standard_list_stamp?,
        })
    }
}

#[doc(hidden)]
pub fn try_prepare_tab_move<E>(
    node: &Node,
    movement: TabMove,
    input: crate::InputRoute<'_>,
    model: &impl ListModel,
    enabled: impl FnOnce() -> Result<bool, E>,
    mut available: impl FnMut(usize) -> Result<bool, E>,
) -> Result<bool, E> {
    if node.tab_move(movement.from, movement.to) != Some(movement) {
        return Ok(false);
    }
    Ok(input.try_available(|| {
        Ok(enabled()?
            && movement.matches_model(model)
            && available(movement.from)?
            && available(movement.to)?)
    })? && input.is_current()
        && movement.matches_model(model))
}

#[doc(hidden)]
pub fn prepare_tab_move<M: ListModel>(
    node: &Node,
    movement: TabMove,
    input: crate::InputRoute<'_>,
    model: &Rc<M>,
    enabled: impl FnOnce() -> bool,
    mut option: impl FnMut(&Rc<M>, usize, bool) -> Option<String>,
) -> bool {
    try_prepare_tab_move(
        node,
        movement,
        input,
        model.as_ref(),
        || Ok::<_, std::convert::Infallible>(enabled()),
        |index| Ok(option(model, index, false).is_some()),
    )
    .unwrap_or_else(|error| match error {})
}

/// Resolve a selectable option that is distinct from the scene's selected option.
#[doc(hidden)]
pub fn group_selection_item(node: &Node, index: usize) -> Option<&Node> {
    let item = group_item(node, index)?;
    (group_index(node) != Some(index)).then_some(item)
}

/// The flattened range and model stamp for a single declaration in a RadioGroup.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RadioOptionBlock {
    end: usize,
    stamp: Option<(u64, u64)>,
}

impl Node {
    #[doc(hidden)]
    pub fn record_radio_option_block(&mut self) {
        self.push_radio_option_block(None);
    }

    fn push_radio_option_block(&mut self, stamp: Option<(u64, u64)>) {
        let end = self.children.len();
        let blocks = self.radio_option_blocks.get_or_insert_with(Rc::default);
        let start = blocks.last().map_or(0, |block| block.end);
        let scope = blocks.len() as u64 + 1;
        Rc::make_mut(blocks).push(RadioOptionBlock { end, stamp });
        for (row, option) in self.children[start..end].iter_mut().enumerate() {
            if option.kind == "RadioButton" {
                // Keep both anonymous fixed options and repeat rows identifiable after a reflow.
                option.row_key.get_or_insert(crate::RowKey {
                    model: 0,
                    row: row as u64,
                    scope: 0,
                });
                option.row_scope = scope;
            }
        }
    }

    /// Resolve the flattened scene position to a declaration and its local row.
    #[doc(hidden)]
    pub fn radio_option_location(&self, slot: usize) -> Option<(usize, usize)> {
        if slot >= self.children.len() {
            return None;
        }
        let Some(blocks) = &self.radio_option_blocks else {
            return Some((slot, 0));
        };
        let declaration = blocks.partition_point(|block| block.end <= slot);
        blocks.get(declaration)?;
        let start = declaration
            .checked_sub(1)
            .map_or(0, |previous| blocks[previous].end);
        Some((declaration, slot - start))
    }

    #[doc(hidden)]
    pub fn radio_option_model_is_current(
        &self,
        declaration: usize,
        model: &impl ListModel,
    ) -> bool {
        self.radio_option_blocks
            .as_ref()
            .and_then(|blocks| blocks.get(declaration))
            .and_then(|block| block.stamp)
            == Some(model.contents_stamp())
    }
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
    try_prepare_group_selection_with_model(
        node,
        index,
        route,
        || Ok((!route.checks_availability() || model_current()?).then_some(())),
        current,
        |_, slot, label| option(slot, label),
    )
}

/// Validate the model once and use that sample for every option read.
#[doc(hidden)]
pub fn try_prepare_group_selection_with_model<M, E>(
    node: &Node,
    index: usize,
    route: crate::InputRoute<'_>,
    model: impl FnOnce() -> Result<Option<M>, E>,
    current: impl FnOnce() -> Result<crate::ButtonGroupSelection, E>,
    mut option: impl FnMut(&M, usize, bool) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    let Some(shown) = group_item(node, index) else {
        return Ok(None);
    };
    let by_navigation_value = matches!(node.kind, "TabBar" | "TabWidget" | "SwipeView")
        && node.text("current-value").is_some();
    let read_value = node.kind == "RadioGroup" || by_navigation_value;
    let mut sample = None;
    if !route.checks_availability() {
        sample = model()?;
    } else if !route.try_available(|| {
        sample = model()?;
        Ok(sample.is_some())
    })? {
        return Ok(None);
    }
    let Some(sample) = sample else {
        return Ok(None);
    };
    let mut option = |slot, label| option(&sample, slot, label);
    let Some(label) = option(index, read_value)? else {
        return Ok(None);
    };
    if !route.is_current() || (read_value && label != crate::radio_value(shown)) {
        return Ok(None);
    }
    if by_navigation_value {
        // A value addresses the first eligible option carrying it.
        for preceding in 0..index {
            if option(preceding, true)?.as_ref() == Some(&label) || !route.is_current() {
                return Ok(None);
            }
        }
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
pub fn prepare_group_selection_with_model<M>(
    node: &Node,
    index: usize,
    route: crate::InputRoute<'_>,
    model: impl FnOnce() -> Option<M>,
    current: impl FnOnce() -> crate::ButtonGroupSelection,
    mut option: impl FnMut(&M, usize, bool) -> Option<String>,
) -> Option<String> {
    try_prepare_group_selection_with_model(
        node,
        index,
        route,
        || Ok::<_, std::convert::Infallible>(model()),
        || Ok(current()),
        |model, slot, label| Ok(option(model, slot, label)),
    )
    .unwrap_or_else(|error| match error {})
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[doc(hidden)]
pub enum PageIdentity {
    Row(RowKey),
    Named(Arc<str>),
    Slot(usize, &'static str),
}

impl PageIdentity {
    pub fn of(page: &Node, slot: usize) -> Self {
        if let Some(key) = page.routing_row_key() {
            Self::Row(key)
        } else if let Some(id) = page.shared_element_id() {
            Self::Named(id)
        } else if let Some(id) = page.id {
            Self::Named(Arc::from(id))
        } else {
            Self::Slot(slot, page.kind)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SwipePreviewRequest {
    from: (usize, PageIdentity),
    to: (usize, PageIdentity),
}

/// The host requests one neighboring page for painting during a SwipeView drag.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct SwipePreviewHandle(Rc<Property<Option<SwipePreviewRequest>>>);

impl std::fmt::Debug for SwipePreviewHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SwipePreviewHandle")
            .field(&Rc::as_ptr(&self.0))
            .finish()
    }
}

impl PartialEq for SwipePreviewHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl SwipePreviewHandle {
    pub fn request(&self, from: (usize, PageIdentity), to: (usize, PageIdentity)) {
        self.0
            .set_if_changed(Some(SwipePreviewRequest { from, to }));
    }

    pub fn clear(&self) {
        self.0.set_if_changed(None);
    }

    fn slot(&self, active: Option<usize>, pages: &[Node]) -> Option<usize> {
        let request = self.0.get(|| None)?;
        let available = |(slot, id): &(usize, PageIdentity)| {
            pages.get(*slot).is_some_and(|page| {
                group_item_available(page, "SwipePage") && PageIdentity::of(page, *slot) == *id
            })
        };
        (active == Some(request.from.0)
            && request.from.0 != request.to.0
            && available(&request.from)
            && available(&request.to))
        .then_some(request.to.0)
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SwipePreview {
    pub handle: SwipePreviewHandle,
    pub page: Option<(usize, Node)>,
}

impl Node {
    #[doc(hidden)]
    pub fn enable_swipe_preview(&mut self, handle: SwipePreviewHandle) {
        self.swipe_preview = Some(Box::new(SwipePreview { handle, page: None }));
    }
}

impl Node {
    #[doc(hidden)]
    pub fn try_append_group_options<M: ListModel, E>(
        &mut self,
        source: impl FnOnce() -> Result<Rc<M>, E>,
        render: impl FnOnce(&Rc<M>) -> Result<Vec<Node>, E>,
    ) -> Result<(), E> {
        let radio = self.kind == "RadioGroup";
        let mut stamp = None;
        let options = crate::try_render_rows_from_source(
            || {
                let model = source()?;
                if radio {
                    stamp = Some(model.contents_stamp());
                } else {
                    self.snapshot_group_options(&*model);
                }
                Ok(model)
            },
            |model| render(&model),
        )?;
        self.children.extend(options);
        if radio {
            self.push_radio_option_block(stamp);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn append_group_options<M: ListModel>(
        &mut self,
        source: impl FnOnce() -> Rc<M>,
        render: impl FnOnce(&Rc<M>) -> Vec<Node>,
    ) {
        self.try_append_group_options(
            || Ok::<_, std::convert::Infallible>(source()),
            |model| Ok(render(model)),
        )
        .unwrap_or_else(|error| match error {});
    }

    #[doc(hidden)]
    pub fn try_append_navigation_pages<M: ListModel, E>(
        &mut self,
        source: impl FnOnce() -> Result<Rc<M>, E>,
        describe: impl FnMut(&Rc<M>, usize) -> Result<Node, E>,
        mount: impl FnOnce(&Rc<M>, Option<usize>) -> Result<Vec<Node>, E>,
    ) -> Result<(), E> {
        self.try_append_navigation_pages_with_preview(source, describe, |model, [active, _]| {
            mount(model, active)
        })
    }

    #[doc(hidden)]
    pub fn append_navigation_pages<M: ListModel>(
        &mut self,
        source: impl FnOnce() -> Rc<M>,
        mut describe: impl FnMut(&Rc<M>, usize) -> Node,
        mount: impl FnOnce(&Rc<M>, Option<usize>) -> Vec<Node>,
    ) {
        self.try_append_navigation_pages(
            || Ok::<_, std::convert::Infallible>(source()),
            |model, slot| Ok(describe(model, slot)),
            |model, slot| Ok(mount(model, slot)),
        )
        .unwrap_or_else(|error| match error {});
    }

    /// Project navigation metadata for every page and mount the selected row and any paint-only swipe preview.
    #[doc(hidden)]
    pub fn try_append_navigation_pages_with_preview<M: ListModel, E>(
        &mut self,
        source: impl FnOnce() -> Result<Rc<M>, E>,
        mut describe: impl FnMut(&Rc<M>, usize) -> Result<Node, E>,
        mount: impl FnOnce(&Rc<M>, [Option<usize>; 2]) -> Result<Vec<Node>, E>,
    ) -> Result<(), E> {
        let requested = self.text("current-value").map(str::to_owned);
        let index = self.int("current-index");
        let visible = self.is_visible();
        let kind = self.kind;
        let mut stamp = None;
        let preview_handle = self
            .swipe_preview
            .as_ref()
            .map(|preview| preview.handle.clone());
        let mut painted = None;
        let pages = crate::try_render_rows_from_source(
            || {
                let model = source()?;
                stamp = Some(model.contents_stamp());
                Ok(model)
            },
            |model| {
                let stamp = model.contents_stamp();
                let mut pages = crate::list::try_render_stable_rows(
                    model.as_ref(),
                    model.structure_stamp(),
                    0..model.row_count().min(4096),
                    |slot| {
                        let mut page = describe(&model, slot)?;
                        page.set_row_key(Some(model.row_key(slot)));
                        Ok(page)
                    },
                )?;
                let active = visible
                    .then(|| navigation_index(kind, requested.as_deref(), index, &pages))
                    .flatten();
                let preview_slot = preview_handle
                    .as_ref()
                    .and_then(|handle| handle.slot(active, &pages));
                let mut mounted = mount(&model, [active, preview_slot])?.into_iter();
                if model.contents_stamp() != stamp {
                    return Ok(Vec::new());
                }
                if let Some(slot) = active {
                    let Some(page) = mounted.next() else {
                        return Ok(Vec::new());
                    };
                    if page.row_key != pages[slot].row_key {
                        return Ok(Vec::new());
                    }
                    pages[slot] = page;
                }
                if let Some(slot) = preview_slot
                    && let Some(page) = mounted.next()
                    && page.row_key == pages[slot].row_key
                {
                    painted = Some((slot, page));
                }
                Ok(pages)
            },
        )?;
        if let Some(preview) = &mut self.swipe_preview {
            preview.page = if pages.is_empty() { None } else { painted };
        }
        self.standard_list_stamp = stamp;
        self.children.extend(pages);
        Ok(())
    }

    #[doc(hidden)]
    pub fn append_navigation_pages_with_preview<M: ListModel>(
        &mut self,
        source: impl FnOnce() -> Rc<M>,
        mut describe: impl FnMut(&Rc<M>, usize) -> Node,
        mount: impl FnOnce(&Rc<M>, [Option<usize>; 2]) -> Vec<Node>,
    ) {
        self.try_append_navigation_pages_with_preview(
            || Ok::<_, std::convert::Infallible>(source()),
            |model, slot| Ok(describe(model, slot)),
            |model, slot| Ok(mount(model, slot)),
        )
        .unwrap_or_else(|error| match error {})
    }

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
    if node.kind == "StackLayout" || navigation_item_kind(node.kind).is_some() {
        return navigation_index(
            node.kind,
            node.text("current-value"),
            node.int("current-index"),
            &node.children,
        );
    }
    if node.kind == "RadioGroup" {
        let mut selection = node.text("current-value").map_or_else(
            || crate::ButtonGroupSelection::Index(node.int("current-index").unwrap_or(-1)),
            |value| crate::ButtonGroupSelection::Value(value.to_owned()),
        );
        return selection.select_radio_option(node.children.len(), |index, label| {
            group_item(node, index).map(|item| if label { crate::radio_value(item) } else { "" })
        });
    }
    let index = usize::try_from(node.int("current-index").unwrap_or(0)).ok()?;
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

    /// Populate the adjacent page outside the live scene while a swipe is in progress.
    #[doc(hidden)]
    pub fn try_mount_swipe_pages<E>(
        &mut self,
        mut mount: impl FnMut(usize, &mut Node) -> Result<(), E>,
    ) -> Result<(), E> {
        self.mount_active_swipe_page(&mut mount).transpose()?;
        let active = self.is_visible().then(|| group_index(self)).flatten();
        let mut painted = None;
        if let Some(slot) = self
            .swipe_preview
            .as_ref()
            .and_then(|preview| preview.handle.slot(active, &self.children))
            && let Some(mut page) = self.children.get(slot).cloned()
        {
            mount(slot, &mut page)?;
            painted = Some((slot, page));
        }
        if let Some(preview) = &mut self.swipe_preview {
            preview.page = painted;
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn mount_swipe_pages(&mut self, mut mount: impl FnMut(usize, &mut Node)) {
        self.try_mount_swipe_pages(|slot, node| {
            mount(slot, node);
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap_or_else(|error| match error {});
    }
}

/// Find the first eligible option addressed by a navigation value.
#[doc(hidden)]
pub fn resolve_navigation_value<V: AsRef<str>>(
    requested: &str,
    options: impl IntoIterator<Item = Option<V>>,
) -> Option<usize> {
    try_resolve_navigation_value(
        requested,
        options.into_iter().map(Ok::<_, std::convert::Infallible>),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_resolve_navigation_value<V: AsRef<str>, E>(
    requested: &str,
    options: impl IntoIterator<Item = Result<Option<V>, E>>,
) -> Result<Option<usize>, E> {
    for (index, option) in options.into_iter().enumerate() {
        if option?.is_some_and(|value| value.as_ref() == requested) {
            return Ok(Some(index));
        }
    }
    Ok(None)
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
