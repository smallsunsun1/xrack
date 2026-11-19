use crate::{
    CalendarGeometry, ChoiceKey, ChoiceMenu, ColorPlaneGeometry, ComboCompletion, ComboGeometry,
    DialGeometry, EditAction, EditSource, LayoutNode, MenuEntry, MenuKey, MenuPanel, MenuShortcut,
    PageIndicatorGeometry, Point, Rect, ScrollBarGeometry, ScrollGestureEnd, ScrollGestureKind,
    SearchGeometry, ShortcutOutcome, SliderGeometry, SpinGeometry, SplitGeometry, SplitHandle,
    SplitHandleRange, StackTransitions, TextEdit, TextPosition, TreeRowGeometry, TumblerScroll,
    WindowResizeEdge,
    choice::{ChoicePopup, MAX_ROWS as CHOICE_PAGE, Typeahead, matches_prefix, next_choice},
    delay_button::{ButtonHold, ButtonHoldEvent, ButtonHoldTiming, DelayHold, DelaySource},
    history::{EditHistories, EditorKey, Group, Record, TextHistory},
    hover_observers::HoverEntry,
    list_anchor::{ListAnchor, capture_list_anchors},
    menu::{MenuPan, MenuPopup, NativeTextMenu, TextMenu},
    modal,
    overscroll::Overscroll,
    scroll_bar::snap_fraction,
    scroll_gesture::{DRAG_THRESHOLD, DragMotion, FlingMotion, PanDevice, SCROLL_FRAME_INTERVAL},
    shortcut::{self, PendingShortcut},
    split::axis_point,
    swipe_delegate::SwipeDrag,
    swipe_gesture::{SwipeDelivery, SwipePhase, SwipeTarget, has_swipe_directions},
    tab_move::TabDrag,
    tooltip::{self, Source as TooltipSource, TooltipState},
    tumbler::{TumblerDrag, TumblerFlick, TumblerWheel},
    view_cells::ViewCells,
    viewport::{MotionOwners, MotionState, UserScrollChange, record_motion, record_motion_owner},
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use xrack_runtime::{
    ButtonGroupMembership, CalendarAction, Component, ControlAction, Date, DialInputMode,
    DialogRole, DrawerEdge, FocusHandle, LayoutDirection, Length, ListMetrics, Node, Orientation,
    PageIdentity, RangeAction, RowKey, RowSelection, ScrollAction, ScrollAlignment, ScrollBarRange,
    ScrollRequest, ScrollSnapMode, SelectionTarget, SliderInterval, SliderRange, SpinAction,
    SpinDecimal, StandardButtonKind, SwipePreviewHandle, SwipeSide, TableEditPosition,
    TableSelectionAction, TextCommand, TextCommandAvailability, TreeAction, TreeSelectionAction,
    Tumbler, Value, ViewportHandle, ViewportMotion, combo_editable, combo_index, decimal_separator,
    delay_millis, dialog_button_action, editable_table_cell, group_index, group_item,
    is_calendar_control, is_multiline_text_input, is_spin_box, is_text_input, is_toggle,
    is_window_overlay, page_indicator_count, page_indicator_index, page_indicator_step,
    radio_value, resizable_table_column, search_suggestion, spin_editable, spin_text,
    spin_text_is_valid, text_input_value,
};

#[derive(Clone, Debug)]
pub(crate) enum ElementId {
    Static(&'static str),
    Shared(std::sync::Arc<str>),
}

impl ElementId {
    pub(crate) fn of(node: &Node) -> Option<Self> {
        node.id
            .map(Self::Static)
            .or_else(|| node.shared_element_id().map(Self::Shared))
    }

    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Static(id) => id,
            Self::Shared(id) => id,
        }
    }
}

impl PartialEq for ElementId {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ElementId {}

#[derive(Clone, Debug)]
pub struct Target {
    pub path: Vec<usize>,
    /// ID of an element constructed with a static name.
    pub id: Option<&'static str>,
    pub focusable: bool,
    pub row_key: Option<RowKey>,
    pub text_link: Option<TextLinkTarget>,
    pub(crate) tab_close: Option<(xrack_runtime::TabClose, String)>,
    owned_id: Option<std::sync::Arc<str>>,
    radio_option_key: Option<RowKey>,
}

impl PartialEq for Target {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.element_id() == other.element_id()
            && self.focusable == other.focusable
            && self.row_key == other.row_key
            && self.tab_close == other.tab_close
            && self.text_link == other.text_link
            && self.radio_option_key == other.radio_option_key
    }
}

impl Eq for Target {}

/// The focused table and the start of a paste in visual column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablePasteDestination {
    pub path: Vec<usize>,
    pub row: usize,
    pub columns: Vec<usize>,
    focus: Target,
}

/// Retains the document seen when the target was resolved. A replacement
/// document must never turn an in-flight action into activation of a new link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLinkTarget {
    index: usize,
    document: xrack_runtime::StyledText,
}

impl Target {
    /// Capture the source identity of a node at its current scene path.
    pub fn from_node(
        path: Vec<usize>,
        node: &Node,
        focusable: bool,
        row_key: Option<RowKey>,
    ) -> Self {
        let mut target = Self::from_element_id(path, ElementId::of(node), focusable, row_key);
        target.radio_option_key = Self::radio_option_key(node);
        target
    }

    pub(crate) fn from_element_id(
        path: Vec<usize>,
        id: Option<ElementId>,
        focusable: bool,
        row_key: Option<RowKey>,
    ) -> Self {
        let mut target = Self {
            path,
            id: None,
            focusable,
            row_key,
            text_link: None,
            tab_close: None,
            owned_id: None,
            radio_option_key: None,
        };
        target.set_element_id(id);
        target
    }

    fn set_element_id(&mut self, id: Option<ElementId>) {
        (self.id, self.owned_id) = match id {
            Some(ElementId::Static(id)) => (Some(id), None),
            Some(ElementId::Shared(id)) => (None, Some(id)),
            None => (None, None),
        };
    }

    /// The captured source ID, including dynamically loaded elements.
    pub fn element_id(&self) -> Option<&str> {
        self.id.or(self.owned_id.as_deref())
    }

    pub(crate) fn matches_node(&self, node: &Node) -> bool {
        self.element_id() == node.element_id()
            && self.radio_option_key == Self::radio_option_key(node)
    }

    fn radio_option_key(node: &Node) -> Option<RowKey> {
        if node.kind == "RadioButton" {
            node.routing_row_key()
        } else {
            None
        }
    }

    #[doc(hidden)]
    pub fn node_in<'a>(&self, scene: &'a Node) -> Option<&'a Node> {
        let node = scene.at_path(&self.path)?;
        (self.matches_node(node) && self.row_key == scene.row_key_at(&self.path)).then_some(node)
    }

    pub fn text_link_index(&self) -> Option<usize> {
        self.text_link.as_ref().map(|link| link.index)
    }

    pub(crate) fn for_text_link(node: &Node, path: &[usize], index: usize) -> Option<Self> {
        let document = node.styled_text("text")?;
        document.links().get(index)?;
        let mut target = Self::from_node(path.to_vec(), node, true, None);
        target.text_link = Some(TextLinkTarget {
            index,
            document: document.clone(),
        });
        Some(target)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationKey {
    Enter,
    Space,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NavigationKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

pub type SliderKey = NavigationKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableSelectionKey {
    Toggle,
    All,
    Clear,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeInput {
    pub path: Vec<usize>,
    pub action: RangeAction,
}

#[derive(Debug, PartialEq)]
pub enum WheelAdjustment {
    Pending,
    Range(RangeInput),
    Spin(ControlInput),
}

#[derive(Clone, Debug)]
struct WheelSteps {
    path: Vec<usize>,
    row: Option<RowKey>,
    handle: Option<usize>,
    horizontal: bool,
    is_line: bool,
    remainder: f64,
    time: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollBarUpdate {
    Unchanged,
    ViewportMoved,
    Range(RangeInput),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlInput {
    pub path: Vec<usize>,
    pub action: ControlAction,
}

fn dismiss_overlay(node: &Node) -> ControlAction {
    match node.kind {
        "Drawer" => ControlAction::Drawer(false),
        "PopupWindow" => ControlAction::Popup(false),
        _ => ControlAction::Dialog(DialogRole::Reject),
    }
}

fn is_modal_path(layout: &LayoutNode, path: &[usize]) -> bool {
    layout.dialog_paths().iter().any(|modal| modal == path)
}

pub struct SplitResize {
    pub path: Vec<usize>,
    pub weights: Vec<(usize, f32)>,
    pub sizes: Option<Vec<(usize, f32)>>,
}

impl ControlInput {
    #[doc(hidden)]
    pub fn with_row_selection(mut self, scene: &Node, operation: RowSelection) -> Self {
        if let Some(node) = scene.at_path(&self.path)
            && let ControlAction::Select(index) = self.action
        {
            if let Some(action) = tree_row_action(node, index, operation) {
                self.action = action;
            } else if node.table_selection.is_some() {
                self.action =
                    ControlAction::TableSelection(TableSelectionAction::Row { index, operation });
            }
        }
        self
    }
}

fn tree_row_action(node: &Node, index: usize, operation: RowSelection) -> Option<ControlAction> {
    node.tree_selection.as_ref()?;
    let item = node.tree.as_ref()?.rows().get(index)?;
    Some(ControlAction::Tree(TreeAction::Selection(
        TreeSelectionAction::Row {
            key: item.key,
            row: item.row,
            operation,
        },
    )))
}

#[derive(Clone, Debug)]
struct DialogFocus {
    path: Vec<usize>,
    row_key: Option<RowKey>,
    modal: bool,
    return_focus: Option<Target>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StackFocusKey {
    path: Vec<usize>,
    row: Option<RowKey>,
    page: usize,
}

#[derive(Clone, Debug)]
struct TableResize {
    target: Target,
    column: usize,
    pointer_x: f32,
    width: f32,
    mirrored: bool,
}

#[derive(Clone, Debug)]
struct TableRowResize {
    target: Target,
    key: RowKey,
    pointer_y: f32,
    height: f32,
}

#[derive(Clone, Debug)]
struct TableColumnMove {
    target: Target,
    column: usize,
    start: Point,
    order: xrack_runtime::TableColumnOrder,
    mirrored: bool,
    claimed: bool,
    scroll_wake: Option<Instant>,
}

#[derive(Clone, Debug)]
struct TableRowMove {
    table: Target,
    from: usize,
    row: xrack_runtime::RowKey,
    stamp: (u64, u64),
    start: Point,
    claimed: bool,
    touch: bool,
    scroll_wake: Option<Instant>,
}

#[derive(Clone, Debug)]
struct SplitDrag {
    target: Target,
    orientation: Orientation,
    mirrored: bool,
    after_pane: usize,
    grab_offset: f32,
    state: Option<xrack_runtime::SplitResizeHandle>,
}

impl SplitDrag {
    fn matches(&self, geometry: &SplitGeometry, handle: &SplitHandle) -> bool {
        geometry.orientation == self.orientation
            && geometry.mirrored == self.mirrored
            && handle.after_pane() == self.after_pane
    }
}

#[derive(Clone, Debug)]
struct WindowMovePress {
    target: Target,
    origin: Point,
}

#[derive(Clone, Debug)]
struct TableEdit {
    target: Target,
    original: String,
    draft: String,
    history: TextHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableEditCommit {
    Kept,
    Unchanged,
    Changed,
    Rejected,
}

pub struct TableEditTarget {
    table: Vec<usize>,
    viewport: ViewportHandle,
    position: TableEditPosition,
}

impl TableEditTarget {
    pub fn scroll_request(&self) -> ScrollRequest {
        ScrollRequest {
            target: self.viewport.clone(),
            action: ScrollAction::ToCell {
                row: self.position.row as i64,
                column: self.position.column as i64,
                horizontal: ScrollAlignment::Contain,
                vertical: ScrollAlignment::Contain,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct SpinDraft {
    text: String,
    mode: SpinDraftMode,
    history: TextHistory,
}

#[derive(Clone, Copy, Debug)]
enum SpinDraftMode {
    Integer(Option<i64>),
    Decimal {
        original: Option<SpinDecimal>,
        separator: char,
    },
}

impl SpinDraftMode {
    fn for_node(node: &Node) -> Self {
        if node.kind == "DoubleSpinBox" {
            Self::Decimal {
                original: node.number("value").and_then(SpinDecimal::new),
                separator: decimal_separator(node),
            }
        } else {
            Self::Integer(node.int("value"))
        }
    }

    fn separator(self) -> Option<char> {
        match self {
            Self::Decimal { separator, .. } => Some(separator),
            Self::Integer(_) => None,
        }
    }

    fn commit(self, text: &str) -> Option<SpinAction> {
        Some(match self {
            Self::Integer(Some(original)) => SpinAction::CommitIfCurrent {
                original,
                value: text.parse().ok()?,
            },
            Self::Integer(None) => SpinAction::Commit(text.parse().ok()?),
            Self::Decimal {
                original,
                separator,
            } => SpinAction::CommitDecimalIfCurrent {
                original,
                value: SpinDecimal::parse(text, separator)?,
            },
        })
    }

    fn is_current(self, node: &Node) -> bool {
        spin_editable(node)
            && match (self, Self::for_node(node)) {
                (Self::Integer(original), Self::Integer(current)) => {
                    original.is_none() || original == current
                }
                (
                    Self::Decimal {
                        original,
                        separator,
                    },
                    Self::Decimal {
                        original: current,
                        separator: current_separator,
                    },
                ) => original == current && separator == current_separator,
                _ => false,
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScrollTarget {
    path: Vec<usize>,
    orientation: Orientation,
    row_key: Option<RowKey>,
    id: Option<ElementId>,
}

impl ScrollTarget {
    fn matches_node(&self, node: &Node) -> bool {
        self.id.as_ref().map(ElementId::as_str) == node.element_id()
    }

    fn resolve<'a, 'b>(
        &self,
        scene: &'a Node,
        layout: &'b LayoutNode,
    ) -> Option<(&'a Node, &'b LayoutNode)> {
        let item = layout.at_path(&self.path)?;
        let node = scene.interactive_at_path(&self.path)?;
        (item.visible
            && item.enabled
            && item.scroll_on(self.orientation).is_some()
            && self.matches_node(node)
            && scene.row_key_at(&self.path) == self.row_key)
            .then_some((node, item))
    }
}

#[derive(Clone, Debug)]
struct ScrollBarPress {
    target: ScrollTarget,
    source: &'static str,
    viewport: Option<xrack_runtime::ViewportHandle>,
    reversed: bool,
    grab: Option<f32>,
    touch: bool,
    cancelled: bool,
    start: Point,
    last: Point,
    dragged: bool,
}

impl ScrollBarPress {
    fn node<'a>(&self, scene: &'a Node) -> Option<&'a Node> {
        scene.at_path(&self.target.path).filter(|node| {
            node.kind == self.source
                && self.target.matches_node(node)
                && node.scroll_target == self.viewport
        })
    }
}

#[derive(Clone, Debug)]
struct ScrollRoute {
    scope: Vec<usize>,
    scope_row: Option<RowKey>,
    scope_id: Option<ElementId>,
    axes: [Option<ScrollTarget>; 2],
    mouse_viewport: bool,
    pan_device: PanDevice,
}

#[derive(Clone, Debug)]
struct ScrollGesture {
    route: ScrollRoute,
    motion: DragMotion,
    tap: Option<Hit>,
    touch_point: Option<Point>,
    popup: (bool, bool),
    menu_pan: Option<MenuPan>,
    drawer: Option<DrawerSwipe>,
    pages: Vec<PageSwipe>,
    tumbler: Option<TumblerDrag>,
    tab_move: Option<TabDrag>,
    switch: Option<SwitchDrag>,
    swipe_delegate: Option<SwipeDrag>,
    swipe: Option<SwipeRoute>,
    delay_touch: bool,
    table_gesture: Option<PendingTableGesture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ButtonContactSource {
    Pointer,
    Gesture,
    Key(ActivationKey),
}

#[derive(Clone, Debug)]
struct ButtonContact {
    source: ButtonContactSource,
    target: Target,
    handle: xrack_runtime::ButtonPressHandle,
}

#[derive(Clone, Debug)]
struct SwitchDrag {
    target: Target,
    start: Point,
    track: Rect,
    travel: f32,
    direction: LayoutDirection,
    initial: bool,
    position: Option<f32>,
    cancelled: bool,
}

impl SwitchDrag {
    fn new(target: Target, node: &Node, item: &LayoutNode, point: Point) -> Option<Self> {
        if node.kind != "Switch" {
            return None;
        }
        let geometry = crate::ToggleGeometry::new(node, item.bounds, item.layout_direction);
        let start = item.map_from_window(point)?;
        let travel = geometry.switch_travel();
        (geometry.indicator.contains(start) && travel > 0.0).then_some(Self {
            target,
            start,
            track: geometry.indicator,
            travel,
            direction: item.layout_direction,
            initial: node.boolean("checked", false),
            position: None,
            cancelled: false,
        })
    }

    fn matches(
        &self,
        node: &Node,
        bounds: Rect,
        direction: LayoutDirection,
        row: Option<RowKey>,
    ) -> bool {
        if node.kind != "Switch"
            || !self.target.matches_node(node)
            || row != self.target.row_key
            || node.boolean("checked", false) != self.initial
            || direction != self.direction
        {
            return false;
        }
        let geometry = crate::ToggleGeometry::new(node, bounds, direction);
        geometry.indicator == self.track && geometry.switch_travel() == self.travel
    }

    fn is_current(&self, scene: &Node, layout: &LayoutNode) -> bool {
        let path = &self.target.path;
        scene
            .interactive_at_path(path)
            .zip(layout.at_path(path))
            .is_some_and(|(node, item)| {
                valid(&self.target, scene, layout)
                    && self.matches(
                        node,
                        item.bounds,
                        item.layout_direction,
                        scene.row_key_at(path),
                    )
            })
    }

    fn move_thumb(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if self.cancelled || !self.is_current(scene, layout) {
            self.cancelled = true;
            return self.position.take().is_some();
        }
        let local = layout
            .at_path(&self.target.path)
            .and_then(|item| item.map_from_window(point));
        let position = local.map(|local| {
            let sign = if self.direction == LayoutDirection::RightToLeft {
                -1.0
            } else {
                1.0
            };
            let origin = if self.initial { 1.0 } else { 0.0 };
            (origin + sign * (local.x - self.start.x) / self.travel).clamp(0.0, 1.0)
        });
        let changed = self.position != position;
        self.position = position;
        changed
    }

    fn accepts_direction(&self, layout: &LayoutNode, point: Point) -> bool {
        layout
            .at_path(&self.target.path)
            .and_then(|item| item.map_from_window(point))
            .is_some_and(|point| (point.x - self.start.x).abs() > (point.y - self.start.y).abs())
    }

    fn finish(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let position = self.position?;
        let checked = position > 0.5 || position == 0.5 && self.initial;
        (checked != self.initial && self.is_current(scene, layout)).then(|| ControlInput {
            path: self.target.path.clone(),
            action: ControlAction::SetSwitch(checked),
        })
    }
}

#[derive(Clone, Debug)]
struct PendingTableGesture {
    drag: PendingTableAction,
    deadline: Option<Instant>,
}

#[derive(Clone, Debug)]
enum PendingTableAction {
    Select(crate::table_drag_selection::TableDragSelection),
    Move(TableRowMove),
}

#[derive(Clone, Debug)]
enum SwipeRoute {
    Pending(Vec<SwipeTarget>),
    Captured(SwipeTarget),
    Cancelled,
}

impl SwipeRoute {
    fn cancel(self) -> Option<SwipeDelivery> {
        match self {
            Self::Captured(target) => Some(target.delivery(SwipePhase::Cancelled)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct SwipePagePosition {
    index: usize,
    identity: PageIdentity,
}

impl SwipePagePosition {
    fn from_view(view: &Node, index: usize) -> Option<Self> {
        let node = group_item(view, index)?;
        Some(Self {
            index,
            identity: PageIdentity::of(node, index),
        })
    }

    fn matches(&self, view: &Node) -> bool {
        group_item(view, self.index)
            .is_some_and(|node| PageIdentity::of(node, self.index) == self.identity)
    }
}

#[derive(Clone, Debug)]
struct PageSwipe {
    target: ScrollTarget,
    current: SwipePagePosition,
    previous: Option<SwipePagePosition>,
    next: Option<SwipePagePosition>,
    mirrored: bool,
    start: Point,
    bounds: Rect,
    threshold: f32,
}

impl PageSwipe {
    fn along_and_across(&self, vector: Point) -> (f32, f32) {
        match self.target.orientation {
            Orientation::Horizontal => (vector.x, vector.y.abs()),
            Orientation::Vertical => (vector.y, vector.x.abs()),
        }
    }

    fn movement(&self, point: Point) -> (f32, f32) {
        self.along_and_across(Point {
            x: self.start.x - point.x,
            y: self.start.y - point.y,
        })
    }

    fn release_distance(
        &self,
        distance: f32,
        item: &LayoutNode,
        velocity: Option<(Point, Point)>,
    ) -> f32 {
        const MIN_FLICK_SPEED: f32 = 480.0;
        const PROJECTION_SECONDS: f32 = 0.10;

        let magnitude = distance.abs();
        let Some((smoothed, latest)) = velocity else {
            return magnitude;
        };
        let Some(latest) = item.map_delta_from_window(latest) else {
            return magnitude;
        };
        let (along, across) = self.along_and_across(latest);
        if along.abs() < MIN_FLICK_SPEED || along.abs() <= across {
            return magnitude;
        }

        let signed = along * distance.signum();
        let velocity = if signed < 0.0 {
            // A clear reversal takes effect before the smoothed velocity catches up.
            signed
        } else {
            let Some(smoothed) = item.map_delta_from_window(smoothed) else {
                return magnitude;
            };
            let (along, across) = self.along_and_across(smoothed);
            let forward = along * distance.signum();
            if forward < MIN_FLICK_SPEED || forward <= across {
                return magnitude;
            }
            signed.min(forward)
        };
        magnitude + (velocity * PROJECTION_SECONDS).clamp(-self.threshold, self.threshold)
    }

    fn destination(&self, physical_forward: bool) -> Option<&SwipePagePosition> {
        if physical_forward != self.mirrored {
            self.next.as_ref()
        } else {
            self.previous.as_ref()
        }
    }

    fn view<'a, 'b>(
        &self,
        scene: &'a Node,
        layout: &'b LayoutNode,
    ) -> Option<(&'a Node, &'b LayoutNode)> {
        let path = &self.target.path;
        let view = scene.interactive_at_path(path)?;
        let item = layout.at_path(path)?;
        (view.kind == "SwipeView"
            && self.target.matches_node(view)
            && view.boolean("interactive", true)
            && view.orientation() == self.target.orientation
            && self.mirrored == swipe_view_mirrored(view, item)
            && item.bounds == self.bounds
            && item.visible
            && item.enabled
            && scene.row_key_at(path) == self.target.row_key
            && group_index(view) == Some(self.current.index)
            && self.current.matches(view))
        .then_some((view, item))
    }

    fn live_destination(&self, view: &Node, forward: bool) -> Option<&SwipePagePosition> {
        let destination = self.destination(forward)?;
        (destination.matches(view)
            && swipe_neighbor(view, self.current.index, forward != self.mirrored)
                == Some(destination.index))
        .then_some(destination)
    }

    fn action(
        self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        velocity: Option<(Point, Point)>,
    ) -> Option<ControlInput> {
        let (view, item) = self.view(scene, layout)?;
        let (distance, across) = self.movement(item.map_from_window(point)?);
        let index = self.live_destination(view, distance > 0.0)?.index;
        (distance.abs() >= self.threshold * 0.25
            && distance.abs() > across
            && self.release_distance(distance, item, velocity) >= self.threshold)
            .then_some(ControlInput {
                path: self.target.path,
                action: ControlAction::Select(index),
            })
    }
}

#[derive(Debug)]
struct SwipePreviewLease(SwipePreviewHandle);

impl Drop for SwipePreviewLease {
    fn drop(&mut self) {
        self.0.clear();
    }
}

#[derive(Clone, Debug)]
struct SwipePageMotion {
    page: PageSwipe,
    lease: Rc<SwipePreviewLease>,
    fraction: f32,
    duration: Duration,
    settling: Option<(Instant, f32)>,
    wake: Option<Instant>,
}

impl SwipePageMotion {
    fn settle(&mut self, now: Instant) {
        self.settling.get_or_insert((now, self.fraction));
        self.wake.get_or_insert(now + SCROLL_FRAME_INTERVAL);
    }

    fn advance(&mut self, now: Instant) -> bool {
        let Some((start, from)) = self.settling else {
            return false;
        };
        let elapsed = now.saturating_duration_since(start);
        let remaining = (1.0 - elapsed.as_secs_f32() / self.duration.as_secs_f32()).max(0.0);
        self.fraction = from * remaining.powi(3);
        self.wake = (elapsed < self.duration).then(|| now + SCROLL_FRAME_INTERVAL);
        true
    }

    fn is_current(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.page.view(scene, layout).is_some_and(|(view, _)| {
            view.int("transition-duration").unwrap_or_default() > 0
                && view
                    .swipe_preview
                    .as_ref()
                    .is_some_and(|preview| preview.handle == self.lease.0)
                && self
                    .page
                    .live_destination(view, self.fraction > 0.0)
                    .is_some()
        })
    }
}

fn navigation_axis(vertical: bool, direction: LayoutDirection) -> (NavigationKey, NavigationKey) {
    if vertical {
        (NavigationKey::Up, NavigationKey::Down)
    } else if direction == LayoutDirection::RightToLeft {
        (NavigationKey::Right, NavigationKey::Left)
    } else {
        (NavigationKey::Left, NavigationKey::Right)
    }
}

fn swipe_view_mirrored(view: &Node, item: &LayoutNode) -> bool {
    view.orientation() == Orientation::Horizontal
        && item.layout_direction == LayoutDirection::RightToLeft
}

fn swipe_neighbor(view: &Node, current: usize, forward: bool) -> Option<usize> {
    if forward {
        (current + 1..view.children.len()).find(|&index| group_item(view, index).is_some())
    } else {
        (0..current)
            .rev()
            .find(|&index| group_item(view, index).is_some())
    }
}

/// Resolve a physical navigation direction against the currently available pages.
pub fn swipe_view_selection(view: &Node, item: &LayoutNode, key: NavigationKey) -> Option<usize> {
    if view.kind != "SwipeView"
        || !view.boolean("interactive", true)
        || !item.enabled
        || !item.visible
    {
        return None;
    }
    let current = group_index(view)?;
    let eligible = |&index: &usize| group_item(view, index).is_some();
    let index = match (key, view.orientation()) {
        (NavigationKey::Home, _) => (0..view.children.len()).find(eligible),
        (NavigationKey::End, _) => (0..view.children.len()).rev().find(eligible),
        (NavigationKey::Left, Orientation::Horizontal) => {
            swipe_neighbor(view, current, swipe_view_mirrored(view, item))
        }
        (NavigationKey::Right, Orientation::Horizontal) => {
            swipe_neighbor(view, current, !swipe_view_mirrored(view, item))
        }
        (NavigationKey::Up, Orientation::Vertical) => swipe_neighbor(view, current, false),
        (NavigationKey::Down, Orientation::Vertical) => swipe_neighbor(view, current, true),
        _ => None,
    }?;
    (index != current).then_some(index)
}

#[derive(Clone, Debug)]
struct DrawerSwipe {
    path: Vec<usize>,
    row_key: Option<RowKey>,
    id: Option<ElementId>,
    edge: DrawerEdge,
    open: bool,
    start: Point,
    threshold: f32,
}

impl DrawerSwipe {
    fn movement(&self, point: Point) -> (f32, f32) {
        let x = point.x - self.start.x;
        let y = point.y - self.start.y;
        let (inward, across) = match self.edge {
            DrawerEdge::Left => (x, y),
            DrawerEdge::Right => (-x, y),
            DrawerEdge::Top => (y, x),
            DrawerEdge::Bottom => (-y, x),
        };
        (if self.open { -inward } else { inward }, across.abs())
    }

    fn action(self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<ControlInput> {
        let action = ControlAction::Drawer(!self.open);
        let node = scene.control_target(&self.path, action)?;
        let (distance, across) = self.movement(point);
        (distance >= self.threshold
            && distance >= across
            && node.element_id() == self.id.as_ref().map(ElementId::as_str)
            && node.drawer_edge() == self.edge
            && node.boolean("open", false) == self.open
            && scene.row_key_at(&self.path) == self.row_key
            && layout.drawer_paths().contains(&self.path))
        .then_some(ControlInput {
            path: self.path,
            action,
        })
    }
}

#[derive(Clone, Debug)]
struct ScrollFling {
    route: ScrollRoute,
    motion: FlingMotion,
    transformed: bool,
}

enum ScrollInput<'a> {
    Wheel,
    Pan {
        device: PanDevice,
        owners: &'a mut MotionOwners,
        flicking: bool,
        touch_scene: Option<&'a Node>,
    },
}

#[derive(Clone, Copy, Default)]
struct AxisMotion {
    bounded: bool,
    elastic: bool,
    dismiss_keyboard: bool,
}

impl AxisMotion {
    fn changed(self) -> bool {
        self.bounded || self.elastic
    }
}

#[derive(Clone, Debug)]
struct EditorClick {
    target: Target,
    count: u8,
    sample: (Point, Instant),
}

#[derive(Clone, Debug)]
struct TextSelectionScroll {
    target: Target,
    point: Point,
    due: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct Interaction {
    pub hovered: Option<Target>,
    pub focused: Option<Target>,
    focus_reason: xrack_runtime::FocusReason,
    pub pressed: Option<Target>,
    drag: Option<crate::drag::DragSession>,
    file_drag: crate::file_drop::FileDrag,
    hovered_scroll_bar: Option<ScrollTarget>,
    scroll_bar_press: Option<ScrollBarPress>,
    scroll_gesture: Option<ScrollGesture>,
    scroll_keyboard_dismissal: bool,
    swipe_events: Vec<SwipeDelivery>,
    tumbler_wheel: Option<TumblerWheel>,
    tumbler_flick: Option<TumblerFlick>,
    wheel_steps: Option<WheelSteps>,
    scroll_fling: Option<ScrollFling>,
    viewport_motion: MotionState,
    overscroll: Overscroll,
    keyboard_avoidance: Option<(Vec<usize>, f32)>,
    key_pressed: Option<(ActivationKey, Target)>,
    key_standard_button: Option<StandardButtonKind>,
    delay_hold: Option<DelayHold>,
    button_hold: Option<ButtonHold>,
    published_button_presses: Vec<xrack_runtime::ButtonPressHandle>,
    button_contacts: Vec<ButtonContact>,
    button_press_events: Vec<(ButtonContact, &'static str)>,
    pointer_activation_claimed: bool,
    key_activation_claimed: Option<ActivationKey>,
    range_keys: BTreeMap<SliderKey, Target>,
    slider_offset: f64,
    slider_anchor: Option<f64>,
    slider_fraction: Option<f64>,
    range_position_handle: Option<xrack_runtime::RangePositionHandle>,
    dial_pointer: Option<Point>,
    touch_range: bool,
    spin_pressed: Option<SpinAction>,
    spin_draft: Option<SpinDraft>,
    choice: Option<ChoicePopup>,
    search_query: Option<String>,
    choice_typeahead: Typeahead,
    list_typeahead: Typeahead,
    pub(crate) list_composition: Option<(u64, Target)>,
    painted_combo_completion: Option<ComboCompletion>,
    dismissed_combo_draft: Option<String>,
    menu: Option<MenuPopup>,
    touch_selection_handles: Option<Target>,
    pending_shortcut: Option<PendingShortcut>,
    pointer: Option<Point>,
    press_origin: Option<Point>,
    tooltip: TooltipState,
    hovered_radio: Option<(String, String)>,
    pressed_radio: Option<(String, String)>,
    hovered_list_row: Option<(usize, Option<ElementId>)>,
    pressed_list_row: Option<(usize, Option<ElementId>)>,
    hovered_tree_indicator: bool,
    pressed_tree_indicator: bool,
    hovered_table_column: Option<usize>,
    pressed_table_column: Option<usize>,
    hovered_body_column: Option<usize>,
    pressed_body_column: Option<usize>,
    hovered_control_part: Option<ControlAction>,
    pressed_control_part: Option<ControlAction>,
    hovered_standard_button: Option<StandardButtonKind>,
    pressed_standard_button: Option<StandardButtonKind>,
    hovered_table_grip: Option<(usize, f32, bool)>,
    hovered_table_row_grip: Option<(RowKey, f32)>,
    table_resize: Option<TableResize>,
    table_row_resize: Option<TableRowResize>,
    table_column_move: Option<TableColumnMove>,
    table_row_move: Option<TableRowMove>,
    table_drag_selection: Option<crate::table_drag_selection::TableDragSelection>,
    split_drag: Option<SplitDrag>,
    split_hover: Option<(xrack_runtime::SplitResizeHandle, usize)>,
    table_edit: Option<TableEdit>,
    last_table_click: Option<(Target, Instant)>,
    editor_press: Option<EditorClick>,
    last_editor_click: Option<EditorClick>,
    text_selection_scroll: Option<TextSelectionScroll>,
    pub editor: Option<TextEdit>,
    persistent_editor: Option<PersistentEditor>,
    retained_selections: Vec<RetainedSelection>,
    editor_window_inactive: bool,
    history: EditHistories,
    scroll_offsets: BTreeMap<Vec<usize>, f32>,
    horizontal_scroll_offsets: BTreeMap<Vec<usize>, f32>,
    scroll_selections: BTreeMap<Vec<usize>, ScrollSelection>,
    tree_scroll_roots: BTreeMap<Vec<usize>, (i64, Option<RowKey>)>,
    pub(crate) scroll_anchors: BTreeMap<Vec<usize>, ListAnchor>,
    pub(crate) user_scroll_changes: Vec<UserScrollChange>,
    pub(crate) refresh_request: Option<ViewportHandle>,
    text_offsets: BTreeMap<Vec<usize>, (Option<RowKey>, Point)>,
    dialogs: Vec<DialogFocus>,
    stack_focus: BTreeMap<StackFocusKey, Target>,
    swipe_page_motion: Option<SwipePageMotion>,
    reduce_motion: bool,
    swipe_commit: Option<(PageSwipe, f32)>,
    page_focus: crate::focus_scope::PageFocusHistory,
    calendar_page_focus: Option<CalendarPageFocus>,
    scope_focus: crate::focus_scope::FocusHistory,
    transitioning_stacks: BTreeSet<Vec<usize>>,
    pointer_on_stack_slide: bool,
    window_move_press: Option<WindowMovePress>,
    dialog_backdrop: Option<Vec<usize>>,
}

#[derive(Clone, Debug)]
struct CalendarPageFocus {
    from: Target,
    view: Target,
    viewport: Option<ViewportHandle>,
    order: (u64, u64),
    index: usize,
}

#[derive(Clone, Debug)]
struct PersistentEditor {
    target: Target,
    handle: Option<FocusHandle>,
    text: String,
    password: bool,
}

impl PersistentEditor {
    fn new(target: &Target, node: &Node) -> Self {
        let mut target = target.clone();
        target.text_link = None;
        Self {
            target,
            handle: node.focus_handle.clone(),
            text: text_input_value(node).to_owned(),
            password: node.is_password(),
        }
    }

    fn same_target(&self, target: &Target) -> bool {
        self.target.path == target.path
            && self.target.row_key == target.row_key
            && self.target.element_id() == target.element_id()
    }

    fn matches(&self, node: &Node) -> bool {
        node.boolean("persistent-selection", false)
            && (is_text_input(node.kind) || combo_editable(node) || node.selectable_styled_text())
            && node.focus_handle == self.handle
            && node.is_password() == self.password
            && text_input_value(node) == self.text
    }
}

#[derive(Clone, Debug)]
struct RetainedSelection {
    source: PersistentEditor,
    editor: TextEdit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Hit {
    None,
    Blocked(Vec<usize>),
    StackSlide,
    WindowMove(Target),
    Target(Target),
    ControlPart(Target, ControlAction),
    ScrollBar(ScrollTarget),
    TableRowGrip(Target, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitPurpose<'a> {
    Button(xrack_runtime::PointerButton),
    Hover,
    Gesture(&'a str),
}

pub(crate) fn focusable(node: &Node) -> bool {
    let kind = node.kind;
    if matches!(kind, "GroupBox" | "Expander") {
        return kind == "Expander" || node.group_checkable();
    }
    if kind == "PageIndicator" {
        return node.boolean("interactive", false) && page_indicator_count(node) != 0;
    }
    if matches!(kind, "Tumbler" | "PathView") {
        return Tumbler::from_node(node).selected().is_some();
    }
    matches!(
        kind,
        "Button"
            | "FocusScope"
            | "Loader"
            | "ToolButton"
            | "RoundButton"
            | "DelayButton"
            | "DialogButton"
            | "Dialog"
            | "Drawer"
            | "PopupWindow"
            | "Slider"
            | "ColorPlane"
            | "ScrollBar"
            | "Dial"
            | "RangeThumb"
            | "SpinBox"
            | "DoubleSpinBox"
            | "ComboBox"
            | "Calendar"
            | "MonthGrid"
            | "RadioButton"
            | "Tab"
            | "TabButton"
            | "SwipeView"
            | "SwipeDelegate"
            | "StackView"
            | "ListView"
            | "GridView"
            | "StandardListView"
            | "TreeView"
            | "TableView"
            | "TableCell"
            | "TreeCell"
            | "SplitHandle"
            | "Menu"
            | "ContextMenuArea"
    ) || is_toggle(kind)
        || is_text_input(kind)
        || node.selectable_styled_text()
}

fn interactive(node: &Node) -> bool {
    matches!(node.kind, "TouchArea" | "ListRow" | "RangeSlider") || focusable(node)
}

fn list_row(scene: &Node, target: &Target) -> Option<(usize, Option<ElementId>)> {
    let (&slot, parent) = target.path.split_last()?;
    let list = scene.at_path(parent)?;
    let index = list.list?.first_row + slot;
    (list.children.get(slot)?.kind == "ListRow").then(|| (index, ElementId::of(list)))
}

fn table_cell_row<'a>(scene: &Node, path: &'a [usize]) -> Option<(&'a [usize], usize)> {
    let [table @ .., slot, 0, _] = path else {
        return None;
    };
    let cell = scene.at_path(path)?;
    ((cell.kind == "TableCell" || cell.boolean("table-editor", false))
        && scene.at_path(table)?.kind == "TableView")
        .then_some((table, *slot))
}

fn table_cell_target(
    scene: &Node,
    layout: &LayoutNode,
    table: &[usize],
    row: usize,
    column: usize,
) -> Option<Target> {
    let view = scene.interactive_at_path(table)?;
    let list = view.list?;
    if view.kind != "TableView" || row >= list.row_count {
        return None;
    }
    let slot = row.checked_sub(list.first_row)?;
    let mut path = table.to_vec();
    path.extend([slot, 0, column]);
    crate::focus_scope::target_at(scene, layout, &path)
}

fn list_item_delegate_row<'a>(scene: &Node, path: &'a [usize]) -> Option<(&'a [usize], usize)> {
    let [view @ .., slot, 0] = path else {
        return None;
    };
    (scene.at_path(path)?.is_item_delegate()
        && matches!(scene.at_path(view)?.kind, "ListView" | "GridView"))
    .then_some((view, *slot))
}

fn tab_close_identity(scene: &Node, path: &[usize]) -> Option<(xrack_runtime::TabClose, String)> {
    let (&index, parent) = path.split_last()?;
    let request = scene.at_path(parent)?.tab_close(index)?;
    let value = scene.at_path(path)?.text("value").unwrap_or("");
    Some((request, value.to_owned()))
}

fn tree_indicator_hit(node: &Node, item: &LayoutNode, point: Point) -> bool {
    node.boolean("tree-has-children", false)
        && item.contains_clip_point(point)
        && TreeRowGeometry::for_layout(node, item)
            .zip(item.map_from_window(point))
            .is_some_and(|(tree, point)| tree.indicator.contains(point))
}

fn tree_cell_row<'a>(scene: &Node, path: &'a [usize]) -> Option<(&'a [usize], usize)> {
    let cell = scene.at_path(path)?;
    if cell.kind != "TreeCell" && !cell.boolean("table-editor", false) {
        return None;
    }
    (0..path.len()).rev().find_map(|depth| {
        let parent = &path[..depth];
        let tree = scene.at_path(parent)?;
        let slot = path[depth];
        (tree.kind == "TreeView"
            && path.get(depth + 1) == Some(&0)
            && tree.children.get(slot)?.kind == "ListRow")
            .then_some((parent, slot))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScrollSelection {
    TabBar(Option<usize>, Orientation, LayoutDirection),
    List(Option<usize>, usize),
}

impl ScrollSelection {
    fn new(node: &Node, columns: usize, direction: LayoutDirection) -> Self {
        let selection = scroll_selection(node);
        if matches!(node.kind, "TabBar" | "TabWidget") {
            let orientation = node.orientation();
            let direction = if orientation == Orientation::Horizontal {
                direction
            } else {
                LayoutDirection::LeftToRight
            };
            Self::TabBar(selection, orientation, direction)
        } else {
            Self::List(selection, columns)
        }
    }
}

pub(crate) fn scroll_selection(node: &Node) -> Option<usize> {
    if matches!(node.kind, "TabBar" | "TabWidget") {
        group_index(node)
    } else {
        let index = usize::try_from(ListMetrics::selection(node)?).ok()?;
        (index < node.list?.row_count).then_some(index)
    }
}

fn current_list_slot(node: &Node) -> Option<usize> {
    let index = usize::try_from(ListMetrics::selection(node)?).ok()?;
    index.checked_sub(node.list?.first_row)
}

fn table_hit_row<'a>(scene: &Node, path: &'a [usize]) -> Option<(&'a [usize], usize)> {
    if let Some(cell) = table_cell_row(scene, path) {
        return Some(cell);
    }
    let (&slot, table) = path.split_last()?;
    (scene.at_path(path)?.kind == "ListRow" && scene.at_path(table)?.kind == "TableView")
        .then_some((table, slot))
}

fn hit(
    node: &Node,
    layout: &LayoutNode,
    window: Point,
    path: &mut Vec<usize>,
    transitioning_stacks: &BTreeSet<Vec<usize>>,
    purpose: HitPurpose<'_>,
) -> Hit {
    if node.is_control_part() {
        return Hit::None;
    }
    let Some(point) = layout.map_from_window(window) else {
        return Hit::None;
    };
    if node.kind == "Tab"
        && layout
            .tab_header
            .is_some_and(|header| header.contains(point) && layout.contains_clip_point(window))
    {
        return if layout.enabled {
            Hit::Target(Target::from_node(path.clone(), node, true, None))
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if !layout.contains_subtree_point(window) {
        return Hit::None;
    }
    if node.kind == "ScrollIndicator"
        || node.kind == "ScrollBar" && !node.boolean("interactive", true)
    {
        return Hit::None;
    }
    if !layout.bounds.contains(point) {
        return hit_children(node, layout, window, path, transitioning_stacks, purpose);
    }
    if node.kind == "RoundButton"
        && !(crate::RoundedClip {
            bounds: layout.bounds,
            radii: crate::corner_radii(node, layout.bounds),
        })
        .contains(point)
    {
        return Hit::None;
    }
    if transitioning_stacks.contains(path) {
        return Hit::StackSlide;
    }
    if layout.enabled
        && let Some(slot) = layout
            .split
            .as_ref()
            .and_then(|split| split.expanded_hit(point))
        && let Some(child) = layout.children.get(slot)
        && let Some(handle) = node.children.get(slot)
        && eligible(handle, child)
        && child.contains_clip_point(window)
    {
        let mut target = path.clone();
        target.push(slot);
        return Hit::Target(Target::from_node(target, handle, true, None));
    }
    if (node.kind == "Expander" || node.group_checkable())
        && layout.group_header(node).contains(point)
    {
        return if layout.enabled {
            Hit::Target(Target::from_node(path.clone(), node, true, None))
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if let Some(action) = layout.tumbler.as_ref().and_then(|wheel| wheel.hit(point)) {
        return if layout.enabled {
            Hit::ControlPart(Target::from_node(path.clone(), node, true, None), action)
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "SearchField"
        && let Some(action) = SearchGeometry::for_layout(layout).action_at(node, point)
    {
        return if layout.enabled {
            Hit::ControlPart(Target::from_node(path.clone(), node, true, None), action)
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if is_calendar_control(node.kind) {
        let action = CalendarGeometry::for_layout(node, layout).action_at(point);
        return if layout.enabled
            && let Some(action) = action
        {
            Hit::ControlPart(
                Target::from_node(path.clone(), node, true, None),
                ControlAction::Calendar(action),
            )
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "PageIndicator" {
        if !focusable(node) {
            return Hit::None;
        }
        let Some(index) = PageIndicatorGeometry::with_layout_direction(
            node,
            layout.bounds,
            layout.layout_direction,
        )
        .and_then(|geometry| geometry.marker_at(point)) else {
            return Hit::None;
        };
        return if layout.enabled {
            Hit::ControlPart(
                Target::from_node(path.clone(), node, true, None),
                ControlAction::Select(index),
            )
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "Dial" && !DialGeometry::new(layout.bounds, node).contains(point) {
        return Hit::None;
    }
    if let Some((rect, action)) = layout
        .swipe_delegate
        .as_deref()
        .and_then(crate::SwipeDelegateGeometry::toggle)
        && rect.contains(point)
    {
        return if layout.enabled {
            Hit::ControlPart(Target::from_node(path.clone(), node, true, None), action)
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if let Some(bar) = layout
        .scroll_bars(node)
        .find(|bar| bar.scrollable() && bar.track.contains(point))
    {
        return if layout.enabled {
            Hit::ScrollBar(ScrollTarget {
                path: path.clone(),
                orientation: bar.orientation,
                row_key: None,
                id: ElementId::of(node),
            })
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "ScrollBar" {
        return Hit::None;
    }
    if let Some(slot) = layout
        .table
        .as_ref()
        .and_then(|table| table.row_resize_at(node, layout, point))
    {
        return if layout.enabled {
            Hit::TableRowGrip(Target::from_node(path.clone(), node, true, None), slot)
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind != "SplitHandle" {
        let child = hit_children(node, layout, window, path, transitioning_stacks, purpose);
        if !matches!(child, Hit::None) {
            return child;
        }
    }
    if let Some((index, _)) = layout
        .text_link_regions()
        .find(|(_, rect)| rect.contains(point))
        && let Some(target) = Target::for_text_link(node, path, index)
    {
        return if layout.enabled {
            Hit::ControlPart(target, ControlAction::ActivateTextLink(index))
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "WindowMoveArea" && layout.enabled && node.boolean("enabled", true) {
        return Hit::WindowMove(Target::from_node(path.clone(), node, false, None));
    }
    if purpose == HitPurpose::Gesture(node.kind)
        && (node.kind == "WheelHandler"
            || node.hover_handle.is_some()
            || node.scale_rotate_handle.is_some()
            || node.swipe_handle.is_some() && has_swipe_directions(node))
        && layout.enabled
        && node.boolean("enabled", true)
    {
        return Hit::Blocked(path.clone());
    }
    if xrack_runtime::is_pane(node.kind) {
        return Hit::Blocked(path.clone());
    }
    if matches!(node.kind, "Tab" | "GroupBox" | "Expander")
        || is_window_overlay(node.kind)
        || !interactive(node)
        || node.kind == "TouchArea"
            && (!node.boolean("enabled", true)
                || matches!(purpose, HitPurpose::Button(button) if !node.accepts_pointer_button(button)))
        || node.kind == "FocusScope" && !node.boolean("focus-on-click", true)
        || node.kind == "TreeCell" && !editable_table_cell(node)
    {
        return Hit::None;
    }
    if !layout.enabled {
        return Hit::Blocked(path.clone());
    }
    let target = Target::from_node(path.clone(), node, focusable(node), None);
    if is_spin_box(node.kind)
        && let Some(action) = SpinGeometry::for_layout(layout, node).action_at(point)
    {
        return Hit::ControlPart(target, ControlAction::Spin(action));
    }
    Hit::Target(target)
}

fn hit_children(
    node: &Node,
    layout: &LayoutNode,
    point: Point,
    path: &mut Vec<usize>,
    transitioning_stacks: &BTreeSet<Vec<usize>>,
    purpose: HitPurpose<'_>,
) -> Hit {
    for (i, child, child_layout) in layout.children_in_paint_order(node).rev() {
        if is_window_overlay(child.kind) {
            continue;
        }
        path.push(i);
        let found = hit(
            child,
            child_layout,
            point,
            path,
            transitioning_stacks,
            purpose,
        );
        path.pop();
        if !matches!(found, Hit::None) {
            return found;
        }
    }
    Hit::None
}

fn inspect_at(node: &Node, layout: &LayoutNode, point: Point, path: &mut Vec<usize>) -> bool {
    if node.is_control_part() || !layout.contains_subtree_point(point) {
        return false;
    }
    // Virtual rows are not individual instances in the inspected tree.
    if node.list.is_none() {
        for (index, child, item) in layout.children_in_paint_order(node).rev() {
            if is_window_overlay(child.kind) {
                continue;
            }
            path.push(index);
            if inspect_at(child, item, point, path) {
                return true;
            }
            path.pop();
        }
    }
    layout
        .map_from_window(point)
        .is_some_and(|point| layout.bounds.contains(point))
}

impl crate::FileDropScope {
    /// Resolve a text drop with the same occlusion and transforms as a pointer press.
    #[doc(hidden)]
    pub fn text_target(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<Target> {
        let (target, node) = self.text_at(scene, layout, point)?;
        (!node.text_read_only()).then_some(target)
    }

    #[doc(hidden)]
    pub fn text_drag_source(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<Target> {
        let (target, node) = self.text_at(scene, layout, point)?;
        (!node.is_password()).then_some(target)
    }

    fn text_at<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(Target, &'a Node)> {
        if self.blocked || self.text_composing {
            return None;
        }
        let scope = self.scope_at(layout, point);
        let (node, item) = scene.at_path(scope).zip(layout.at_path(scope))?;
        let Hit::Target(mut target) = hit(
            node,
            item,
            point,
            &mut scope.to_vec(),
            &self.transitions,
            HitPurpose::Button(xrack_runtime::PointerButton::Primary),
        ) else {
            return None;
        };
        if !target.focusable
            || self
                .swiping
                .as_ref()
                .is_some_and(|path| target.path.starts_with(path))
        {
            return None;
        }
        target.row_key = scene.row_key_at(&target.path);
        let node = scene.interactive_at_path(&target.path)?;
        if !valid(&target, scene, layout) {
            return None;
        }
        if node.kind == "SearchField" || combo_editable(node) {
            let item = layout.at_path(&target.path)?;
            let part = if node.kind == "SearchField" {
                SearchGeometry::for_layout(item).entry
            } else {
                ComboGeometry::for_control(node, item.bounds, item.layout_direction).entry
            };
            part.contains(item.map_from_window(point)?)
                .then_some((target, node))
        } else {
            is_text_input(node.kind).then_some((target, node))
        }
    }
}

pub(crate) fn valid(target: &Target, scene: &Node, layout: &LayoutNode) -> bool {
    let Some(node) = target.node_in(scene) else {
        return false;
    };
    let Some(item) = layout.at_path(&target.path) else {
        return false;
    };
    if let Some(link) = &target.text_link {
        return item.enabled
            && node.styled_text("text") == Some(&link.document)
            && item
                .text_link_regions()
                .any(|(index, _)| index == link.index);
    }
    target.focusable == focusable(node) && eligible(node, item)
}

fn context_menu_area<'a>(
    scene: &'a Node,
    path: &'a [usize],
    scope: &[usize],
) -> Option<(&'a [usize], &'a Node, usize)> {
    (scope.len()..=path.len()).rev().find_map(|depth| {
        let path = &path[..depth];
        let area = scene
            .interactive_at_path(path)
            .filter(|node| node.kind == "ContextMenuArea")?;
        let menu = area
            .children
            .iter()
            .position(|child| child.kind == "Menu" && child.is_visible())?;
        Some((path, area, menu))
    })
}

fn split_target<'a, 'b>(
    target: &'a Target,
    scene: &Node,
    layout: &'b LayoutNode,
) -> Option<(&'a [usize], &'b SplitGeometry, &'b SplitHandle)> {
    if scene.at_path(&target.path)?.kind != "SplitHandle" || !valid(target, scene, layout) {
        return None;
    }
    let (&slot, path) = target.path.split_last()?;
    let geometry = layout.at_path(path)?.split.as_deref()?;
    Some((path, geometry, geometry.handle(slot)?))
}

fn pending_scroll_offset(
    item: &LayoutNode,
    offsets: &BTreeMap<Vec<usize>, f32>,
    path: &[usize],
    axis: Orientation,
) -> Option<f32> {
    if let Some(renderer) = &item.nested_list {
        item.bound_offset(axis).or_else(|| renderer.offset(axis))
    } else {
        offsets.get(path).copied()
    }
}

fn store_scroll_offset(
    item: &LayoutNode,
    offsets: &mut BTreeMap<Vec<usize>, f32>,
    path: &[usize],
    axis: Orientation,
    offset: f32,
) {
    if let Some(renderer) = &item.nested_list {
        renderer.set_offset(axis, offset);
    } else {
        offsets.insert(path.to_vec(), offset);
    }
}

pub(crate) fn moved_row_target(
    target: &Target,
    scene: &Node,
    layout: &LayoutNode,
) -> Option<Target> {
    let key = target.row_key?;
    if key.has_view_scope() {
        fn find(node: &Node, target: &Target, path: &mut Vec<usize>) -> Option<Target> {
            if matches!(
                node.kind,
                "ListRow" | "TabButton" | "Tab" | "StackItem" | "SwipePage"
            ) && node.routing_row_key() == target.row_key
            {
                let suffix = target.path.get(path.len()..)?;
                let mut moved = target.clone();
                moved.path = path.iter().copied().chain(suffix.iter().copied()).collect();
                return Some(moved);
            }
            if path.len() >= target.path.len() {
                return None;
            }
            node.materialized_children().find_map(|(slot, child)| {
                path.push(slot);
                let found = find(child, target, path);
                path.pop();
                found
            })
        }
        return find(scene, target, &mut Vec::new()).filter(|moved| valid(moved, scene, layout));
    }
    for depth in (0..target.path.len()).rev() {
        let Some(node) = scene.at_path(&target.path[..depth]) else {
            continue;
        };
        if node.kind == "DynamicContainer" {
            let (slot, _) = node
                .materialized_children()
                .find(|(_, child)| child.dynamic_mount_key() == Some(key))?;
            let mut moved = target.clone();
            moved.path[depth] = slot;
            return valid(&moved, scene, layout).then_some(moved);
        }
        if !matches!(
            node.kind,
            "ListView"
                | "GridView"
                | "StandardListView"
                | "TableView"
                | "TreeView"
                | "TabBar"
                | "TabWidget"
                | "StackLayout"
                | "SwipeView"
        ) {
            continue;
        }
        let slot = node
            .children
            .iter()
            .position(|row| row.routing_row_key() == Some(key))?;
        let mut moved = target.clone();
        moved.path[depth] = slot;
        return valid(&moved, scene, layout).then_some(moved);
    }
    None
}

fn focus_hitbox(node: &Node, layout: &LayoutNode) -> Option<Rect> {
    if node.kind == "PathView"
        && layout
            .tumbler
            .as_ref()
            .is_none_or(|view| view.rows.is_empty())
    {
        return None;
    }
    if node.kind == "ScrollBar"
        && (!node.boolean("interactive", true)
            || !layout
                .scroll_control_geometry(node)
                .is_some_and(ScrollBarGeometry::scrollable))
    {
        return None;
    }
    let hitbox = if node.kind == "Tab" {
        layout.tab_header
    } else if layout.visible && matches!(node.kind, "GroupBox" | "Expander") {
        Some(layout.group_header(node))
    } else if layout.visible {
        Some(layout.bounds)
    } else {
        None
    };
    (layout.enabled && interactive(node))
        .then_some(hitbox)
        .flatten()
}

pub(crate) fn eligible(node: &Node, layout: &LayoutNode) -> bool {
    focus_hitbox(node, layout).is_some_and(|bounds| {
        let clipped = layout.clip_rect(bounds);
        clipped.width > 0.0 && clipped.height > 0.0
    })
}

pub(crate) fn focus_reachable(
    target: &Target,
    scene: &Node,
    layout: &LayoutNode,
    allow_reveal: bool,
) -> bool {
    valid(target, scene, layout)
        || allow_reveal
            && target.text_link.is_none()
            && target
                .node_in(scene)
                .zip(layout.at_path(&target.path))
                .is_some_and(|(node, item)| {
                    target.focusable == focusable(node)
                        && focus_hitbox(node, item).is_some_and(|rect| {
                            crate::viewport::plan_focus_scroll(scene, layout, &target.path, rect)
                                .is_some_and(|plan| !plan.is_empty())
                        })
                })
}

fn scroll_target_visible(item: &LayoutNode) -> bool {
    let clip = item.clip_rect(item.bounds);
    item.visible && item.enabled && clip.width > 0.0 && clip.height > 0.0
}

fn primary_vertical_viewport(
    node: &Node,
    item: &LayoutNode,
    path: &mut Vec<usize>,
    transitions: &BTreeSet<Vec<usize>>,
    primary: &mut Option<(f32, Vec<usize>)>,
) {
    if !item.visible || !item.enabled || transitions.contains(path) {
        return;
    }
    if item
        .scroll_on(Orientation::Vertical)
        .is_some_and(|scroll| scroll.max_offset > 0.0)
        && scroll_target_visible(item)
    {
        let bounds = item.visible_window_bounds(item.bounds);
        let area = bounds.width * bounds.height;
        if area > 0.0 && primary.as_ref().is_none_or(|(best, _)| area > *best) {
            *primary = Some((area, path.clone()));
        }
        return;
    }
    for (index, child, child_layout) in item.children_in_scene_order(node) {
        if !is_window_overlay(child.kind) {
            path.push(index);
            primary_vertical_viewport(child, child_layout, path, transitions, primary);
            path.pop();
        }
    }
}

fn first_editable_tree_cell<'a>(
    node: &'a Node,
    layout: &LayoutNode,
    path: &mut Vec<usize>,
) -> Option<&'a Node> {
    if !layout.enabled || !layout.visible || node.list.is_some() {
        return None;
    }
    if editable_table_cell(node) && eligible(node, layout) {
        return Some(node);
    }
    for (index, child, item) in layout.children_in_scene_order(node) {
        path.push(index);
        if let Some(cell) = first_editable_tree_cell(child, item, path) {
            return Some(cell);
        }
        path.pop();
    }
    None
}

fn group_targets(
    node: &Node,
    layout: &LayoutNode,
    path: &[usize],
    include_clipped: bool,
) -> impl Iterator<Item = (usize, Target)> {
    let scrollable_tabs =
        matches!(node.kind, "TabBar" | "TabWidget") && scroll_target_visible(layout);
    node.children
        .iter()
        .zip(&layout.children)
        .enumerate()
        .filter_map(move |(index, (child, item))| {
            let usable = if include_clipped || scrollable_tabs {
                focus_hitbox(child, item).is_some_and(|rect| rect.width > 0.0 && rect.height > 0.0)
            } else {
                eligible(child, item)
            };
            if group_item(node, index).is_none() || !usable {
                return None;
            }
            let mut child_path = path.to_vec();
            child_path.push(index);
            Some((index, Target::from_node(child_path, child, true, None)))
        })
}

fn group_focus(node: &Node, layout: &LayoutNode, path: &[usize]) -> Option<Target> {
    let selected = group_index(node);
    group_targets(node, layout, path, false)
        .min_by_key(|(index, _)| Some(*index) != selected)
        .map(|(_, target)| target)
}

pub(crate) fn radio_group_focus(
    scene: &Node,
    layout: &LayoutNode,
    path: &[usize],
    allow_reveal: bool,
) -> Option<Target> {
    let group = scene
        .at_path(path)
        .filter(|node| node.kind == "RadioGroup")?;
    let item = layout.at_path(path)?;
    group_targets(group, item, path, allow_reveal)
        .filter_map(|(index, mut target)| {
            target.row_key = scene.row_key_at(&target.path);
            focus_reachable(&target, scene, layout, allow_reveal).then_some((index, target))
        })
        .min_by_key(|(index, _)| Some(*index) != group_index(group))
        .map(|(_, target)| target)
}

fn attached_radio<'a>(
    scene: &'a Node,
    path: &'a [usize],
) -> Option<(&'a ButtonGroupMembership, &'a [usize])> {
    let member = scene
        .at_path(path)?
        .button_group
        .as_ref()
        .filter(|member| member.group.exclusive)?;
    Some((member, member.component_path(path)?))
}

fn attached_radio_tab_stop(scene: &Node, path: &[usize], targets: &[Target]) -> Option<usize> {
    targets.iter().position(|target| {
        attached_radio(scene, &target.path).is_some_and(|(member, component)| {
            path.strip_prefix(component)
                .is_some_and(|path| member.group.paths().any(|member| member == path))
        })
    })
}

pub(crate) fn focus_targets(scene: &Node, layout: &LayoutNode, scope: &[usize]) -> Vec<Target> {
    focus_targets_with_reveal(scene, layout, scope, false)
}

pub(crate) fn focus_targets_with_reveal(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    allow_reveal: bool,
) -> Vec<Target> {
    fn collect(
        root: (&Node, &LayoutNode),
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        out: &mut Vec<Target>,
        allow_reveal: bool,
        clipped_candidates: bool,
    ) {
        if !layout.visible
            || !layout.enabled
            || node.kind == "CalendarCells"
            || node.is_control_part()
        {
            return;
        }
        let clip = layout.clip_rect(layout.bounds);
        if !clipped_candidates
            && !layout.allows_overflow()
            && (clip.width <= 0.0 || clip.height <= 0.0)
        {
            return;
        }
        let candidate = |path: &[usize], node: &Node, item: &LayoutNode| {
            let target = Target::from_node(path.to_vec(), node, true, root.0.row_key_at(path));
            if !clipped_candidates {
                return Some(target);
            }
            (eligible(node, item) || focus_reachable(&target, root.0, root.1, true))
                .then_some(target)
        };
        let before_children = out.len();
        if matches!(node.kind, "TabWidget" | "TabBar" | "RadioGroup") {
            if clipped_candidates
                && !(matches!(node.kind, "TabBar" | "TabWidget") && scroll_target_visible(layout))
            {
                let selected = group_index(node);
                if let Some((_, target)) = group_targets(node, layout, path, true)
                    .filter_map(|(index, mut target)| {
                        target.row_key = root.0.row_key_at(&target.path);
                        focus_reachable(&target, root.0, root.1, true).then_some((index, target))
                    })
                    .min_by_key(|(index, _)| Some(*index) != selected)
                {
                    out.push(target);
                }
            } else if let Some(target) = group_focus(node, layout, path) {
                out.push(target);
            }
            if node.kind != "TabWidget" {
                return;
            }
        } else if !matches!(node.kind, "Tab" | "TreeCell" | "FocusScope" | "Loader")
            && !is_window_overlay(node.kind)
            && focusable(node)
            && (!matches!(node.kind, "GroupBox" | "Expander")
                || clipped_candidates
                || eligible(node, layout))
        {
            out.extend(candidate(path, node, layout));
        }
        if node.kind == "SplitHandle" {
            return;
        }
        if node.kind == "StyledText" {
            let indices: BTreeSet<_> = layout.text_link_regions().map(|(index, _)| index).collect();
            out.extend(
                indices
                    .into_iter()
                    .filter_map(|index| Target::for_text_link(node, path, index)),
            );
        }
        let reveal_children =
            clipped_candidates || allow_reveal && crate::viewport::reveals_focus(node, layout);
        let mut visit = |index: usize| {
            if let Some((child, item)) = node.child_at(index).zip(layout.child_at(index))
                && !is_window_overlay(child.kind)
            {
                path.push(index);
                collect(root, child, item, path, out, allow_reveal, reveal_children);
                path.pop();
            }
        };
        if node.kind == "TreeView" {
            if let Some(slot) = current_list_slot(node) {
                visit(slot);
            }
        } else if !node.boolean("__standard-table", false) {
            for index in crate::child_reading_order(node) {
                visit(index);
            }
        }
        if node.kind == "FocusScope"
            && out.len() == before_children
            && node.boolean("focus-on-tab-navigation", true)
        {
            out.extend(candidate(path, node, layout));
        }
    }
    let mut targets = Vec::new();
    if let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) {
        let clipped_candidates = allow_reveal
            && (0..scope.len())
                .rev()
                .map(|depth| &scope[..depth])
                .map_while(|path| scene.at_path(path).zip(layout.at_path(path)))
                .take_while(|(node, _)| !is_window_overlay(node.kind))
                .any(|(node, item)| crate::viewport::reveals_focus(node, item));
        collect(
            (scene, layout),
            node,
            item,
            &mut scope.to_vec(),
            &mut targets,
            allow_reveal,
            clipped_candidates,
        );
        if targets.is_empty() && is_window_overlay(node.kind) && eligible(node, item) {
            targets.push(Target::from_node(scope.to_vec(), node, true, None));
        }
    }
    let mut groups = BTreeMap::new();
    let mut stops = Vec::with_capacity(targets.len());
    for mut target in targets {
        target.row_key = scene.row_key_at(&target.path);
        if let Some((member, component)) = attached_radio(scene, &target.path) {
            let key = (component.to_vec(), member.group.key.clone());
            if let Some(&slot) = groups.get(&key) {
                if member.group.selected == Some(member.index) {
                    stops[slot] = target;
                }
                continue;
            }
            groups.insert(key, stops.len());
        }
        stops.push(target);
    }
    stops
}

fn dialog_entry_focus(scene: &Node, layout: &LayoutNode, path: &[usize]) -> Option<Target> {
    let node = scene.at_path(path)?;
    let targets = focus_targets(scene, layout, path);
    let first = targets.iter().find(|target| {
        scene
            .at_path(&target.path)
            .and_then(Node::standard_button_kind)
            != Some(StandardButtonKind::Discard)
    });
    if node.kind == "Dialog"
        && first
            .and_then(|target| scene.at_path(&target.path))
            .and_then(dialog_button_action)
            .is_some()
        && let Some(primary) = targets.iter().find(|target| {
            scene.at_path(&target.path).is_some_and(|button| {
                button.boolean("primary", false)
                    && dialog_button_action(button)
                        == Some(ControlAction::Dialog(DialogRole::Accept))
            })
        })
    {
        return Some(primary.clone());
    }
    first.cloned().or_else(|| {
        eligible(node, layout.at_path(path)?)
            .then(|| Target::from_node(path.to_vec(), node, true, scene.row_key_at(path)))
    })
}

fn slider_target<'a, 'b>(
    target: &'a Target,
    scene: &'b Node,
    layout: &LayoutNode,
) -> Option<(&'a [usize], &'b Node)> {
    let leaf = scene.at_path(&target.path)?;
    if matches!(leaf.kind, "Slider" | "RangeSlider" | "Dial" | "ColorPlane") {
        return valid(target, scene, layout).then_some((target.path.as_slice(), leaf));
    }
    if leaf.kind != "RangeThumb" || target.row_key != scene.row_key_at(&target.path) {
        return None;
    }
    let (&index, path) = target.path.split_last()?;
    let node = scene.interactive_at_path(path)?;
    (index < 2 && node.kind == "RangeSlider" && eligible(node, layout.at_path(path)?))
        .then_some((path, node))
}

fn sibling_menu(
    scene: &Node,
    layout: &LayoutNode,
    target: &Target,
    key: MenuKey,
) -> Option<Target> {
    let (&index, parent) = target.path.split_last()?;
    let bar = scene
        .at_path(parent)
        .filter(|node| node.kind == "MenuBar")?;
    let choices: Vec<_> = bar
        .children
        .iter()
        .enumerate()
        .filter_map(|(slot, child)| {
            if child.kind != "Menu" {
                return None;
            }
            let mut path = parent.to_vec();
            path.push(slot);
            let row_key = scene.row_key_at(&path);
            let sibling = Target::from_node(path, child, true, row_key);
            valid(&sibling, scene, layout).then_some((slot, sibling))
        })
        .collect();
    let current = choices.iter().position(|(slot, _)| *slot == index)?;
    let rtl = layout.at_path(parent)?.layout_direction == LayoutDirection::RightToLeft;
    let previous = (key == MenuKey::Left) != rtl;
    let step = if previous { choices.len() - 1 } else { 1 };
    Some(choices[(current + step) % choices.len()].1.clone())
}

impl Interaction {
    pub fn begin_pointer_sequence(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        event: xrack_runtime::PointerEvent,
        now: Instant,
    ) -> Option<(crate::PointerSequence, crate::PointerDelivery)> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        let Hit::Target(target) =
            self.hit_at_for(scene, layout, point, HitPurpose::Button(event.button))
        else {
            return None;
        };
        crate::PointerSequence::begin(target, self, scene, layout, point, event, now)
    }

    pub(crate) fn touch_area_hover<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(&'a Node, (Length, Length))> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        let Hit::Target(target) = self.hit_at_for(scene, layout, point, HitPurpose::Hover) else {
            return None;
        };
        let node = scene.interactive_at_path(&target.path)?;
        if node.kind != "TouchArea" {
            return None;
        }
        let (_, x, y) =
            crate::pointer_events::local_position(layout.at_path(&target.path)?, point)?;
        Some((node, (x, y)))
    }

    pub fn direct_pointer_claimed(&self) -> bool {
        self.scroll_table_drag_claimed()
            || self.scroll_gesture.as_ref().is_some_and(|gesture| {
                gesture.motion.dragging
                    && (gesture.route.axes.iter().any(Option::is_some)
                        || gesture.drawer.is_some()
                        || !gesture.pages.is_empty()
                        || gesture.tumbler.is_some()
                        || gesture.switch.is_some()
                        || gesture.swipe_delegate.is_some()
                        || matches!(
                            gesture.swipe,
                            Some(SwipeRoute::Captured(_) | SwipeRoute::Cancelled)
                        ))
            })
    }

    pub fn owns_pointer_press(&self, target: &Target) -> bool {
        self.pressed.as_ref() == Some(target)
            || self.scroll_gesture.as_ref().is_some_and(
                |gesture| matches!(&gesture.tap, Some(Hit::Target(tap)) if tap == target),
            )
    }

    pub fn cancel_pointer_activation(&mut self, target: &Target) {
        if self.pressed.as_ref() == Some(target) {
            self.pressed = None;
            self.press_origin = None;
        }
        if let Some(gesture) = &mut self.scroll_gesture
            && matches!(&gesture.tap, Some(Hit::Target(tap)) if tap == target)
        {
            gesture.tap = None;
        }
    }

    pub(crate) fn hover_entries(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Vec<HoverEntry> {
        if self.menu_open() || self.choice_open() {
            return Vec::new();
        }
        let Some((path, scope_depth)) = self.gesture_hit_path(scene, layout, point, "HoverHandler")
        else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for depth in (scope_depth..=path.len()).rev() {
            let path = &path[..depth];
            let Some((node, item)) = scene.at_path(path).zip(layout.at_path(path)) else {
                continue;
            };
            if !node.boolean("enabled", true) {
                continue;
            }
            let Some((handle, position)) = node
                .hover_handle
                .as_ref()
                .zip(local_handler_position(item, point))
            else {
                continue;
            };
            entries.push(HoverEntry {
                path: path.to_vec(),
                row_key: scene.row_key_at(path),
                handle: handle.clone(),
                position,
                cursor_shape: node.cursor_shape(),
            });
            if node.boolean("blocking", false) {
                break;
            }
        }
        entries
    }

    pub fn wheel_route(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        event: xrack_runtime::WheelEvent,
    ) -> Option<WheelRoute> {
        if self.menu_open()
            || self.choice_open()
            || !(event.delta_x.0.is_finite() && event.delta_y.0.is_finite())
            || event.delta_x.0 == 0.0 && event.delta_y.0 == 0.0
        {
            return None;
        }
        let point = self.pointer?;
        let (path, scope_depth) = self.gesture_hit_path(scene, layout, point, "WheelHandler")?;
        let mut deliveries = Vec::new();
        for depth in (scope_depth..=path.len()).rev() {
            let path = &path[..depth];
            let node = scene.at_path(path)?;
            let item = layout.at_path(path)?;
            if !node.handles_wheel() || !item.enabled || !node.boolean("enabled", true) {
                continue;
            }
            let handles_axis = match node.properties.get("orientation") {
                Some(Value::Orientation(Orientation::Horizontal)) => event.delta_x.0 != 0.0,
                Some(Value::Orientation(Orientation::Vertical)) => event.delta_y.0 != 0.0,
                _ => true,
            };
            if !handles_axis {
                continue;
            }
            if let Some((x, y)) = local_handler_position(item, point) {
                deliveries.push((path.to_vec(), xrack_runtime::WheelEvent { x, y, ..event }));
            }
        }
        (!deliveries.is_empty()).then_some(WheelRoute { deliveries })
    }

    pub fn update_tooltip(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) -> bool {
        let scope = self.active_dialog(layout).unwrap_or_default();
        let key = match self.tooltip.source {
            TooltipSource::Pointer => self.pointer.and_then(|point| {
                tooltip::hovered(scene, layout, self.pointer_scope(layout, point), point)
            }),
            TooltipSource::Focus => self
                .focused
                .as_ref()
                .filter(|target| valid(target, scene, layout))
                .and_then(|target| tooltip::focused(scene, &target.path, scope)),
        };
        let blocked = self.menu_open()
            || self.choice_open()
            || self.pressed.is_some()
            || self.table_drag_selection_claimed()
            || self.dialog_backdrop.is_some();
        self.tooltip.advance(scene, key, self.pointer, blocked, now)
    }

    pub fn tooltip_deadline(&self) -> Option<Instant> {
        self.tooltip.deadline()
    }

    pub fn dismiss_tooltip(&mut self) -> bool {
        self.tooltip.dismiss()
    }

    pub fn tooltip_popup<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
    ) -> Option<crate::TooltipPopup<'a>> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        self.tooltip.popup(
            scene,
            layout,
            self.active_dialog(layout).unwrap_or_default(),
        )
    }

    pub fn overlay_layers<'a>(
        &'a self,
        layout: &'a LayoutNode,
    ) -> impl DoubleEndedIterator<Item = &'a [usize]> + 'a {
        self.dialogs
            .iter()
            .filter(move |dialog| layout.overlay_paths().contains(&dialog.path))
            .map(|dialog| dialog.path.as_slice())
            .chain(
                layout
                    .overlay_paths()
                    .iter()
                    .filter(move |path| !self.dialogs.iter().any(|dialog| &dialog.path == *path))
                    .map(Vec::as_slice),
            )
    }

    pub fn dialog_layers<'a>(
        &'a self,
        layout: &'a LayoutNode,
    ) -> impl DoubleEndedIterator<Item = &'a [usize]> + 'a {
        self.overlay_layers(layout)
            .filter(move |path| is_modal_path(layout, path))
    }

    fn active_dialog<'a>(&'a self, layout: &'a LayoutNode) -> Option<&'a [usize]> {
        self.dialog_layers(layout).next_back()
    }

    pub fn allows_popup(&self, layout: &LayoutNode, path: &[usize]) -> bool {
        self.active_dialog(layout)
            .is_none_or(|modal| path.starts_with(modal))
    }

    fn popups<'a>(&'a self, layout: &'a LayoutNode) -> impl Iterator<Item = &'a [usize]> + 'a {
        self.overlay_layers(layout)
            .rev()
            .filter(move |path| !is_modal_path(layout, path) && self.allows_popup(layout, path))
    }

    fn popup_at<'a>(&'a self, layout: &'a LayoutNode, point: Point) -> Option<&'a [usize]> {
        self.popups(layout).find(|path| {
            layout
                .at_path(path)
                .is_some_and(|item| item.bounds.contains(point))
        })
    }

    fn focused_popup<'a>(&'a self, layout: &'a LayoutNode) -> Option<&'a [usize]> {
        let target = self.focused.as_ref()?;
        self.popups(layout)
            .find(|path| target.path.starts_with(path))
    }

    fn pointer_scope<'a>(&'a self, layout: &'a LayoutNode, point: Point) -> &'a [usize] {
        self.popup_at(layout, point)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default()
    }

    fn drag_context<'a>(
        &'a self,
        scene: &'a Node,
        layout: &'a LayoutNode,
        point: Point,
    ) -> crate::drag::Context<'a> {
        crate::drag::Context {
            scene,
            layout,
            scope: self.pointer_scope(layout, point),
            modal: self.active_dialog(layout),
            transitions: &self.transitioning_stacks,
            blocked: self.menu_open() || self.choice_open(),
        }
    }

    pub fn arm_drag(&mut self, scene: &Node, layout: &LayoutNode, point: Point) {
        self.arm_drag_at(scene, layout, point, None);
    }

    #[doc(hidden)]
    pub fn arm_touch_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) {
        self.arm_drag_at(scene, layout, point, Some(now));
    }

    fn arm_drag_at(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        touch_started: Option<Instant>,
    ) {
        if self.table_resize.is_some()
            || self.table_row_resize.is_some()
            || self.table_column_move.is_some()
            || self.table_row_move.is_some()
            || self.table_drag_selection_claimed()
        {
            self.drag = None;
            return;
        }
        let competing = match self.hit_at(scene, layout, point) {
            Hit::Target(target) => scene.at_path(&target.path).is_some_and(|node| {
                is_text_input(node.kind)
                    || combo_editable(node)
                    || is_spin_box(node.kind)
                    || matches!(
                        node.kind,
                        "Slider"
                            | "ColorPlane"
                            | "Dial"
                            | "RangeSlider"
                            | "SplitHandle"
                            | "TableCell"
                            | "TreeCell"
                            | "Tumbler"
                            | "PathView"
                            | "SwipeDelegate"
                    )
            }),
            Hit::None => false,
            _ => true,
        };
        self.drag = if competing {
            None
        } else {
            crate::drag::DragSession::arm(
                self.drag_context(scene, layout, point),
                point,
                touch_started,
            )
        };
    }

    pub fn drag_motion(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        requested: Option<xrack_runtime::DragAction>,
    ) -> crate::DragUpdate {
        self.drag_motion_at(scene, layout, point, requested, Instant::now())
    }

    #[doc(hidden)]
    pub fn drag_motion_at(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        requested: Option<xrack_runtime::DragAction>,
        now: Instant,
    ) -> crate::DragUpdate {
        let Some(mut drag) = self.drag.take() else {
            return crate::DragUpdate::default();
        };
        let active = drag.active();
        let (update, keep) = drag.motion(
            self.drag_context(scene, layout, point),
            point,
            requested,
            now,
        );
        self.drag = keep.then_some(drag);
        if update.captured && !active {
            self.cancel_press();
            self.pointer_leave();
        }
        update
    }

    pub fn finish_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        requested: Option<xrack_runtime::DragAction>,
    ) -> crate::DragUpdate {
        self.drag
            .take()
            .map_or_else(crate::DragUpdate::default, |drag| {
                drag.finish(self.drag_context(scene, layout, point), point, requested)
            })
    }

    pub fn cancel_drag(&mut self) -> crate::DragUpdate {
        self.drag
            .take()
            .map_or_else(crate::DragUpdate::default, |mut drag| drag.cancel())
    }

    pub fn reconcile_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        requested: Option<xrack_runtime::DragAction>,
    ) -> crate::DragUpdate {
        let Some(mut drag) = self.drag.take() else {
            return crate::DragUpdate::default();
        };
        let point = drag.point;
        let (update, keep) =
            drag.reconcile(self.drag_context(scene, layout, point), point, requested);
        self.drag = keep.then_some(drag);
        update
    }

    pub fn drag_position(&self) -> Option<Point> {
        self.drag.as_ref().map(|drag| drag.point)
    }

    #[doc(hidden)]
    pub fn drag_scroll_deadline(&self, scene: &Node, layout: &LayoutNode) -> Option<Instant> {
        let drag = self.drag.as_ref()?;
        drag.auto_scroll_offset(self.drag_context(scene, layout, drag.point))?;
        drag.scroll_deadline()
    }

    #[doc(hidden)]
    pub fn advance_drag_scroll(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) -> bool {
        let Some(drag) = self.drag.as_ref() else {
            return false;
        };
        if drag.scroll_deadline().is_none_or(|deadline| now < deadline) {
            return false;
        }
        let Some((path, offset)) =
            drag.auto_scroll_offset(self.drag_context(scene, layout, drag.point))
        else {
            return false;
        };
        let path = path.to_vec();
        let scrolled = self.scroll_to_user_offset(layout, &path, Orientation::Vertical, offset);
        if let Some(drag) = &mut self.drag {
            drag.advance_scroll_deadline(now, scrolled);
        }
        scrolled
    }

    pub fn drag_action(&self) -> Option<xrack_runtime::DragAction> {
        self.drag
            .as_ref()
            .and_then(crate::drag::DragSession::action)
    }

    pub fn is_drop_target(&self, node: &Node, path: &[usize]) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.over(node, path))
            || self.file_drag.over(node, path)
    }

    pub fn file_drop_scope(&self, layout: &LayoutNode) -> crate::FileDropScope {
        crate::FileDropScope {
            modal: self.active_dialog(layout).map(<[usize]>::to_vec),
            popups: self.popups(layout).map(<[usize]>::to_vec).collect(),
            transitions: self.transitioning_stacks.clone(),
            blocked: self.menu_open() || self.choice_open() || self.drag_action().is_some(),
            text_composing: self.editor.as_ref().is_some_and(TextEdit::is_composing),
            swiping: self
                .swipe_page_motion
                .as_ref()
                .map(|motion| motion.page.target.path.clone()),
        }
    }

    pub fn file_drag_event(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        input: crate::FileDragInput,
    ) -> crate::DragUpdate {
        let scope = self.file_drop_scope(layout);
        self.file_drag.update(&scope, scene, layout, input)
    }

    pub fn reconcile_file_drag(&mut self, scene: &Node, layout: &LayoutNode) -> crate::DragUpdate {
        if !self.file_drag.has_hover() {
            return crate::DragUpdate::default();
        }
        let scope = self.file_drop_scope(layout);
        self.file_drag.reconcile(&scope, scene, layout)
    }

    pub fn clear_file_drag(&mut self) {
        self.file_drag = crate::file_drop::FileDrag::default();
    }

    pub fn popup_outside_press(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let path = self.popups(layout).next()?;
        let point = self.pointer?;
        let popup = scene.at_path(path)?;
        let (_, anchor) = path.split_last()?;
        let anchor_clicked = self
            .target_at(scene, layout, point)
            .is_some_and(|target| target.path == anchor);
        (!anchor_clicked
            && !layout.at_path(path)?.bounds.contains(point)
            && popup.boolean("close-on-outside", true))
        .then(|| ControlInput {
            path: path.to_vec(),
            action: ControlAction::Popup(false),
        })
    }

    pub fn dialog_button(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        let path = self.active_dialog(layout)?;
        let node = scene.at_path(&target.path)?;
        if !target.path.starts_with(path)
            || scene.at_path(path)?.kind != "Dialog"
            || self
                .popups(layout)
                .any(|popup| target.path.starts_with(popup))
            || !valid(target, scene, layout)
        {
            return None;
        }
        Some(ControlInput {
            path: path.to_vec(),
            action: dialog_button_action(node)?,
        })
    }

    pub fn dialog_accept(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        if self.focused_popup(layout).is_some() {
            return None;
        }
        let path = self.active_dialog(layout)?;
        focus_targets(scene, layout, path)
            .iter()
            .filter_map(|target| {
                let action = self.dialog_button(scene, layout, target)?;
                if action.action != ControlAction::Dialog(DialogRole::Accept) {
                    return None;
                }
                let primary = scene.at_path(&target.path)?.boolean("primary", false);
                Some((!primary, action))
            })
            .min_by_key(|(secondary, _)| *secondary)
            .map(|(_, action)| action)
    }

    pub fn dialog_escape(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let path = self
            .popups(layout)
            .next()
            .or_else(|| self.active_dialog(layout))?;
        let node = scene.at_path(path)?;
        node.boolean("close-on-escape", true).then(|| ControlInput {
            path: path.to_vec(),
            action: dismiss_overlay(node),
        })
    }

    /// Keep focus on the dialog when pressing its backdrop or empty panel area.
    pub fn dialog_pointer_press(&mut self, layout: &LayoutNode) -> bool {
        self.dialog_backdrop = None;
        let Some(path) = self.active_dialog(layout) else {
            return false;
        };
        let Some(point) = self.pointer else {
            return false;
        };
        if self.popup_at(layout, point).is_some() {
            return false;
        }
        let Some(item) = layout.at_path(path) else {
            return false;
        };
        let outside = !item.bounds.contains(point);
        if !outside
            && self
                .hovered
                .as_ref()
                .is_some_and(|target| target.path.starts_with(path))
        {
            return false;
        }
        let backdrop = outside.then(|| path.to_vec());
        self.cancel_press();
        self.dialog_backdrop = backdrop;
        true
    }

    pub fn dialog_pointer_release(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        let pressed = self.dialog_backdrop.take()?;
        let path = self.active_dialog(layout)?;
        let node = scene.at_path(path)?;
        if pressed != path
            || layout.at_path(path)?.bounds.contains(self.pointer?)
            || self.popup_at(layout, self.pointer?).is_some()
            || !node.boolean(
                "close-on-outside",
                matches!(node.kind, "Drawer" | "PopupWindow"),
            )
        {
            return None;
        }
        Some(ControlInput {
            path: pressed,
            action: dismiss_overlay(node),
        })
    }

    fn set_focus(&mut self, focus: Option<Target>) {
        self.set_focus_with_reason(focus, xrack_runtime::FocusReason::KeyboardNavigation);
    }

    #[doc(hidden)]
    pub fn clear_focus(&mut self) {
        self.set_focus(None);
    }

    fn set_focus_with_reason(&mut self, focus: Option<Target>, reason: xrack_runtime::FocusReason) {
        if self.focused != focus {
            self.touch_selection_handles = None;
            self.calendar_page_focus = None;
            self.text_selection_scroll = None;
            self.retain_editor_selection();
            self.editor_press = None;
            self.last_editor_click = None;
            self.focus_reason = reason;
            self.cancel_shortcut_sequence();
            self.cancel_delay();
            self.button_hold = None;
            self.history.break_group();
            self.spin_draft = None;
            self.table_edit = None;
            self.last_table_click = None;
            Self::clear_choice(&mut self.choice);
            self.search_query = None;
            self.choice_typeahead = Typeahead::default();
            self.reset_list_typeahead();
            self.list_composition = None;
            self.painted_combo_completion = None;
            self.dismissed_combo_draft = None;
            self.menu = None;
            self.range_keys.clear();
            self.focused = focus;
        }
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    #[doc(hidden)]
    pub fn set_touch_selection_handles(&mut self, visible: bool) {
        self.touch_selection_handles = self.focused.clone().filter(|_| visible);
    }

    #[doc(hidden)]
    pub fn touch_selection_target(&self) -> Option<&Target> {
        self.touch_selection_handles
            .as_ref()
            .filter(|target| self.focused.as_ref() == Some(*target))
    }

    pub fn dismiss_menu(&mut self) {
        if self.menu.take().is_some() {
            self.cancel_shortcut_sequence();
        }
    }

    pub fn close_menu(&mut self, scene: &Node, layout: &LayoutNode) {
        self.dismiss_menu();
        if let Some(point) = self.pointer {
            self.pointer_move(scene, layout, point);
        }
    }

    pub fn menu_panels<'a>(&'a self, scene: &'a Node, layout: &LayoutNode) -> Vec<MenuPanel<'a>> {
        self.menu
            .as_ref()
            .filter(|menu| self.menu_is_current(menu, scene, layout))
            .map_or_else(Vec::new, |menu| menu.panels(scene, layout))
    }

    pub fn menu_trigger(&self) -> Option<&Target> {
        self.menu.as_ref().map(|popup| &popup.trigger)
    }

    pub fn accessibility_menu_entry(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        trigger: &Target,
        entry: &MenuEntry,
        activate: bool,
    ) -> Option<ControlInput> {
        if !self.reconcile_text_menu(scene, layout) {
            return None;
        }
        let path = self
            .menu
            .as_mut()?
            .accessibility_entry(scene, layout, trigger, entry, activate)?;
        self.finish_menu_action(scene, layout, path)
    }

    pub fn menu_header_open(&self, path: &[usize]) -> bool {
        self.menu.as_ref().and_then(MenuPopup::bar_header) == Some(path)
    }

    pub fn accessibility_close_menu(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        trigger: &Target,
        path: &[usize],
        session: u64,
    ) {
        if self
            .menu
            .as_mut()
            .and_then(|popup| popup.accessibility_close(scene, layout, trigger, path, session))
            == Some(true)
        {
            self.close_menu(scene, layout);
        }
    }

    pub fn menu_shortcut(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: MenuShortcut,
    ) -> Option<ControlInput> {
        if self.shortcuts_blocked() {
            return None;
        }
        if let Some(menu) = self.menu.as_ref().filter(|menu| menu.text.is_some()) {
            return self
                .menu_is_current(menu, scene, layout)
                .then(|| menu.text_shortcut(key))
                .flatten();
        }
        let scope = self.shortcut_scope(layout);
        shortcut::single_action(
            shortcut::bindings(
                scene,
                layout,
                scope,
                self.menu.as_ref(),
                self.focused.as_ref(),
            ),
            key,
        )
    }

    pub fn shortcut_press(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: MenuShortcut,
        repeat: bool,
        now: Instant,
    ) -> ShortcutOutcome {
        let mut pending = self.pending_shortcut.take();
        if key.is_escape() && (self.menu_open() || self.choice_open()) {
            return ShortcutOutcome::Unmatched;
        }
        if self.menu.as_ref().is_some_and(|menu| menu.text.is_some()) {
            if repeat || !self.reconcile_text_menu(scene, layout) {
                return ShortcutOutcome::Consumed;
            }
            return self
                .menu
                .as_ref()
                .and_then(|menu| menu.text_shortcut(key))
                .map_or(ShortcutOutcome::Consumed, ShortcutOutcome::Activate);
        }
        if self.shortcuts_blocked() {
            return ShortcutOutcome::Unmatched;
        }
        let scope = self.shortcut_scope(layout);
        let bindings = shortcut::bindings(
            scene,
            layout,
            scope,
            self.menu.as_ref(),
            self.focused.as_ref(),
        );
        let outcome = shortcut::press(&mut pending, bindings, scope, key, repeat, now);
        self.pending_shortcut = pending;
        outcome
    }

    pub fn cancel_shortcut_sequence(&mut self) {
        self.pending_shortcut = None;
    }

    fn shortcuts_blocked(&self) -> bool {
        self.choice_open()
            || self
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
    }

    fn shortcut_scope<'a>(&'a self, layout: &'a LayoutNode) -> &'a [usize] {
        self.focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default()
    }

    fn target_at(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<Target> {
        self.target_at_for(
            scene,
            layout,
            point,
            HitPurpose::Button(xrack_runtime::PointerButton::Primary),
        )
    }

    pub fn pointer_hover_target(&self, scene: &Node, layout: &LayoutNode) -> Option<Target> {
        self.target_at_for(scene, layout, self.pointer?, HitPurpose::Hover)
    }

    fn target_at_for(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        purpose: HitPurpose<'_>,
    ) -> Option<Target> {
        match self.hit_at_for(scene, layout, point, purpose) {
            Hit::Target(target) | Hit::ControlPart(target, _) | Hit::TableRowGrip(target, _) => {
                Some(target)
            }
            _ => None,
        }
    }

    fn hit_at(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Hit {
        self.hit_at_for(
            scene,
            layout,
            point,
            HitPurpose::Button(xrack_runtime::PointerButton::Primary),
        )
    }

    /// Find the frontmost inspectable element, including disabled and static elements.
    #[doc(hidden)]
    pub fn inspection_path(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<Vec<usize>> {
        let scope = self.pointer_scope(layout, point);
        let (node, item) = scene.at_path(scope).zip(layout.at_path(scope))?;
        let mut path = scope.to_vec();
        inspect_at(node, item, point, &mut path).then_some(path)
    }

    fn hit_at_for(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        purpose: HitPurpose<'_>,
    ) -> Hit {
        let scope = self.pointer_scope(layout, point);
        let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) else {
            return Hit::None;
        };
        let mut result = hit(
            node,
            item,
            point,
            &mut scope.to_vec(),
            &self.transitioning_stacks,
            purpose,
        );
        if let Some(motion) = &self.swipe_page_motion {
            let moving = &motion.page.target.path;
            let hit_path = match &result {
                Hit::Target(target)
                | Hit::ControlPart(target, _)
                | Hit::TableRowGrip(target, _) => Some(target.path.as_slice()),
                Hit::ScrollBar(target) => Some(target.path.as_slice()),
                Hit::Blocked(path) => Some(path.as_slice()),
                _ => None,
            };
            if hit_path.is_some_and(|path| path.starts_with(moving)) {
                return Hit::StackSlide;
            }
        }
        if matches!(result, Hit::None) && node.kind == "PopupWindow" && item.bounds.contains(point)
        {
            result = Hit::Blocked(scope.to_vec());
        }
        match &mut result {
            Hit::Target(target) => {
                target.row_key = scene.row_key_at(&target.path);
                target.tab_close = (|| {
                    let item = layout.at_path(&target.path)?;
                    if !item.tab_close?.contains(item.map_from_window(point)?) {
                        return None;
                    }
                    tab_close_identity(scene, &target.path)
                })();
            }
            Hit::ControlPart(target, _)
            | Hit::TableRowGrip(target, _)
            | Hit::WindowMove(target) => target.row_key = scene.row_key_at(&target.path),
            Hit::ScrollBar(target) => target.row_key = scene.row_key_at(&target.path),
            _ => {}
        }
        result
    }

    pub fn scale_rotate_target_at(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<crate::ScaleRotateTarget> {
        if self.menu_open()
            || self.choice_open()
            || self.scroll_bar_captured()
            || self.drag_position().is_some()
            || self
                .scroll_gesture
                .as_ref()
                .is_some_and(|gesture| gesture.motion.dragging)
        {
            return None;
        }
        let (path, scope_len) =
            self.gesture_hit_path(scene, layout, point, "ScaleRotateGestureHandler")?;
        (scope_len..=path.len()).rev().find_map(|depth| {
            let target = crate::ScaleRotateTarget::new(scene, &path[..depth])?;
            let item = layout.at_path(&target.path)?;
            (item.contains_clip_point(point)
                && item.bounds.contains(item.map_from_window(point)?)
                && target.is_live(scene, layout, self))
            .then_some(target)
        })
    }

    fn gesture_hit_path(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        kind: &str,
    ) -> Option<(Vec<usize>, usize)> {
        let scope = self.pointer_scope(layout, point);
        let path = match hit(
            scene.at_path(scope)?,
            layout.at_path(scope)?,
            point,
            &mut scope.to_vec(),
            &self.transitioning_stacks,
            HitPurpose::Gesture(kind),
        ) {
            Hit::Target(target) | Hit::ControlPart(target, _) => target.path,
            Hit::Blocked(path) => path,
            _ => return None,
        };
        Some((path, scope.len()))
    }

    pub fn window_gestures_allowed(&self, scene: &Node, layout: &LayoutNode) -> bool {
        scene.kind == "Window"
            && scene.is_visible()
            && !self.menu_open()
            && !self.choice_open()
            && self.active_dialog(layout).is_none()
            && self.popups(layout).next().is_none()
            && self.drag_position().is_none()
    }

    /// Hit the opt-in client resize border without covering interactive content.
    pub fn window_resize_edge(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<WindowResizeEdge> {
        if !scene.boolean("no-frame", false)
            || !scene.boolean("resizable", true)
            || !self.window_gestures_allowed(scene, layout)
        {
            return None;
        }
        let point = self.pointer?;
        let edge =
            WindowResizeEdge::at(layout.bounds, point, scene.length("resize-border-width")?)?;
        matches!(
            self.hit_at(scene, layout, point),
            Hit::None | Hit::WindowMove(_)
        )
        .then_some(edge)
    }

    pub fn arm_window_move(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(origin) = self
            .pointer
            .filter(|_| self.window_gestures_allowed(scene, layout))
        else {
            return false;
        };
        let Hit::WindowMove(target) = self.hit_at(scene, layout, origin) else {
            return false;
        };
        self.cancel_press();
        self.window_move_press = Some(WindowMovePress { target, origin });
        true
    }

    pub fn take_window_move(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(press) = &self.window_move_press else {
            return false;
        };
        if !self.window_gestures_allowed(scene, layout)
            || !matches!(self.hit_at(scene, layout, press.origin), Hit::WindowMove(target) if target == press.target)
        {
            self.window_move_press = None;
            return false;
        }
        const THRESHOLD: f32 = 4.0;
        let x = point.x - press.origin.x;
        let y = point.y - press.origin.y;
        if x * x + y * y < THRESHOLD * THRESHOLD {
            return false;
        }
        self.window_move_press = None;
        true
    }

    pub fn cancel_window_move(&mut self) -> bool {
        self.window_move_press.take().is_some()
    }

    /// Synchronize the pointer gate with the presentation state before reconciling input.
    pub fn sync_stack_transitions(&mut self, transitions: &StackTransitions) -> bool {
        if self
            .transitioning_stacks
            .iter()
            .map(Vec::as_slice)
            .eq(transitions.active_paths())
        {
            return false;
        }
        self.transitioning_stacks = transitions.active_paths().map(<[usize]>::to_vec).collect();
        true
    }

    pub fn pointer_on_stack_slide(&self) -> bool {
        self.pointer_on_stack_slide
    }

    pub fn open_menu_bar(&mut self, scene: &Node, layout: &LayoutNode, target: Target) -> bool {
        self.open_menu_bar_with_touch(scene, layout, target, false)
    }

    #[doc(hidden)]
    pub fn open_menu_bar_with_touch(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        touch: bool,
    ) -> bool {
        let Some((_, parent)) = target.path.split_last() else {
            return false;
        };
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !target.path.starts_with(scope)
            || !scene
                .at_path(parent)
                .is_some_and(|bar| bar.kind == "MenuBar")
            || !scene
                .at_path(&target.path)
                .is_some_and(|node| node.kind == "Menu")
        {
            return false;
        }
        let Some(popup) = MenuPopup::open(
            target.clone(),
            target.path.clone(),
            None,
            scene,
            layout,
            None,
            touch,
        ) else {
            return false;
        };
        self.dismiss_choice();
        self.set_focus(Some(target));
        self.cancel_shortcut_sequence();
        self.set_split_hover(None);
        self.menu = Some(popup);
        true
    }

    pub fn open_context_menu(&mut self, scene: &Node, layout: &LayoutNode, keyboard: bool) -> bool {
        let target = if keyboard {
            self.focused.clone()
        } else {
            self.pointer.and_then(|point| {
                self.target_at_for(
                    scene,
                    layout,
                    point,
                    HitPurpose::Button(xrack_runtime::PointerButton::Secondary),
                )
            })
        };
        let Some(target) = target else {
            return false;
        };
        self.open_context_menu_at(
            scene,
            layout,
            target,
            if keyboard { None } else { self.pointer },
            false,
        )
    }

    #[doc(hidden)]
    pub fn touch_context_menu_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<Target> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        let hit = self.target_at(scene, layout, point)?;
        let scope = self.active_dialog(layout).unwrap_or_default();
        let (path, area, _) = context_menu_area(scene, &hit.path, scope)?;
        Some(Target::from_node(
            path.to_vec(),
            area,
            true,
            scene.row_key_at(path),
        ))
    }

    #[doc(hidden)]
    pub fn open_touch_context_menu(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        point: Point,
    ) -> bool {
        self.touch_context_menu_target(scene, layout, point)
            .as_ref()
            == Some(&target)
            && self.open_context_menu_at(scene, layout, target, Some(point), true)
    }

    #[doc(hidden)]
    pub fn touch_text_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<Target> {
        if self.menu_open() {
            return None;
        }
        let target = self.target_at_for(
            scene,
            layout,
            point,
            HitPurpose::Button(xrack_runtime::PointerButton::Primary),
        )?;
        let node = self.context_editor(scene, layout, &target)?;
        if !target.focusable || !(self.is_text_editor(node, &target) || spin_editable(node)) {
            return None;
        }
        if is_spin_box(node.kind) {
            let item = layout.at_path(&target.path)?;
            let point = item.map_from_window(point)?;
            if !SpinGeometry::for_layout(item, node).text.contains(point) {
                return None;
            }
        }
        if self.focused.as_ref() == Some(&target)
            && self.editor.as_ref().is_some_and(TextEdit::is_composing)
        {
            return None;
        }
        Some(target)
    }

    #[doc(hidden)]
    pub fn open_touch_text_menu(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        point: Point,
    ) -> bool {
        self.open_context_menu_at(scene, layout, target, Some(point), true)
    }

    #[doc(hidden)]
    pub fn take_native_text_menu(&mut self) -> Option<NativeTextMenu> {
        self.menu
            .take_if(|menu| menu.touch && menu.text.is_some())
            .map(NativeTextMenu)
    }

    #[doc(hidden)]
    pub fn native_text_menu_is_current(
        &self,
        menu: &NativeTextMenu,
        scene: &Node,
        layout: &LayoutNode,
    ) -> bool {
        self.menu.is_none() && self.menu_is_current(&menu.0, scene, layout)
    }

    #[doc(hidden)]
    pub fn native_text_menu_input(
        &self,
        menu: NativeTextMenu,
        scene: &Node,
        layout: &LayoutNode,
        entry: usize,
    ) -> Option<ControlInput> {
        if !self.native_text_menu_is_current(&menu, scene, layout)
            || !menu.entries().any(|(index, _)| index == entry)
        {
            return None;
        }
        let mut path = menu.0.trigger.path.clone();
        path.push(entry);
        menu.0.input(path)
    }

    fn open_context_menu_at(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        point: Option<Point>,
        touch: bool,
    ) -> bool {
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !target.path.starts_with(scope) {
            return false;
        }
        let context = context_menu_area(scene, &target.path, scope);
        let keyboard = point.is_none();
        let point = if let Some(point) = point {
            point
        } else {
            let Some(item) = layout.at_path(&target.path) else {
                return false;
            };
            let direction = context
                .and_then(|(path, area, index)| {
                    area.child_at(index)
                        .and_then(Node::layout_direction)
                        .or_else(|| layout.at_path(path).map(|area| area.layout_direction))
                })
                .unwrap_or(item.layout_direction);
            let anchor = item.visible_window_bounds(item.bounds);
            Point {
                x: anchor.x
                    + if direction == LayoutDirection::RightToLeft {
                        anchor.width
                    } else {
                        0.0
                    },
                y: anchor.y + anchor.height,
            }
        };
        let popup = if let Some((path, area, index)) = context {
            let trigger = Target::from_node(path.to_vec(), area, true, scene.row_key_at(path));
            let mut path = path.to_vec();
            path.push(index);
            MenuPopup::open(trigger, path, Some(point), scene, layout, None, touch)
        } else {
            let Some(node) = self.context_editor(scene, layout, &target) else {
                return false;
            };
            if self.focused.as_ref() == Some(&target)
                && self.editor.as_ref().is_some_and(TextEdit::is_composing)
            {
                return false;
            }
            if !keyboard {
                self.set_focus_with_reason(
                    Some(target.clone()),
                    xrack_runtime::FocusReason::PointerClick,
                );
            }
            self.sync_editor(scene);
            self.break_text_edit_group();
            let value = self.editor_value(node);
            let Some(editor) = &self.editor else {
                return false;
            };
            let selection = editor.selection_state(&value);
            let commands = self.text_command_availability(
                scene,
                &target,
                node,
                &value,
                !selection.collapsed(),
            );
            let menu = TextMenu::new(node, value.into_owned(), selection, commands);
            MenuPopup::open(
                target.clone(),
                target.path,
                Some(point),
                scene,
                layout,
                Some(menu),
                touch,
            )
        };
        let Some(popup) = popup else {
            return false;
        };
        self.dismiss_choice();
        self.cancel_press();
        self.cancel_shortcut_sequence();
        self.set_split_hover(None);
        self.menu = Some(popup);
        true
    }

    pub fn supports_context_menu(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> bool {
        let scope = self.active_dialog(layout).unwrap_or_default();
        target.path.starts_with(scope)
            && (context_menu_area(scene, &target.path, scope).is_some()
                || self.context_editor(scene, layout, target).is_some())
    }

    fn context_editor<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<&'a Node> {
        scene.interactive_at_path(&target.path).filter(|node| {
            node.boolean("context-menu-enabled", true)
                && (self.is_text_editor(node, target) || spin_editable(node))
                && self.spin_draft.as_ref().is_none_or(|draft| {
                    self.focused.as_ref() != Some(target) || draft.mode.is_current(node)
                })
                && self.table_edit.as_ref().is_none_or(|draft| {
                    &draft.target != target || node.text("text").unwrap_or("") == draft.original
                })
                && valid(target, scene, layout)
        })
    }

    fn editor_history(&self, scene: &Node, target: &Target, node: &Node) -> Option<&TextHistory> {
        if is_spin_box(node.kind) {
            self.spin_draft.as_ref().map(|draft| &draft.history)
        } else if let Some(draft) = self
            .table_edit
            .as_ref()
            .filter(|draft| &draft.target == target)
        {
            Some(&draft.history)
        } else {
            self.history.get(&EditorKey::new(scene, target))
        }
    }

    #[doc(hidden)]
    pub fn text_command_availability(
        &self,
        scene: &Node,
        target: &Target,
        node: &Node,
        value: &str,
        selected: bool,
    ) -> TextCommandAvailability {
        if self.editor.as_ref().is_some_and(TextEdit::is_composing) {
            return TextCommandAvailability::default();
        }
        let writable = !node.text_read_only();
        let copy = selected && !node.is_password();
        let (undo, redo) = self
            .editor_history(scene, target, node)
            .filter(|_| writable)
            .map_or((false, false), |history| history.availability(value));
        TextCommandAvailability {
            undo,
            redo,
            copy,
            cut: copy && writable,
        }
    }

    fn menu_is_current(&self, menu: &MenuPopup, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(text) = &menu.text else {
            return true;
        };
        self.focused.as_ref() == Some(&menu.trigger)
            && menu
                .trigger
                .path
                .starts_with(self.active_dialog(layout).unwrap_or_default())
            && self
                .context_editor(scene, layout, &menu.trigger)
                .is_some_and(|node| {
                    let value = self.editor_value(node);
                    text.kind == node.kind
                        && text.password == node.is_password()
                        && text.writable == !node.text_read_only()
                        && text.value == value
                        && self.editor.as_ref().is_some_and(|editor| {
                            !editor.is_composing()
                                && editor.selection_state(&value) == text.selection
                        })
                        && text.commands
                            == self.text_command_availability(
                                scene,
                                &menu.trigger,
                                node,
                                &value,
                                !text.selection.collapsed(),
                            )
                })
    }

    pub fn reconcile_text_menu(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let current = self
            .menu
            .as_ref()
            .is_none_or(|menu| self.menu_is_current(menu, scene, layout));
        if !current {
            self.close_menu(scene, layout);
        }
        current
    }

    pub fn pointer_context_menu_changes_focus(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.pointer
            .and_then(|point| {
                self.target_at_for(
                    scene,
                    layout,
                    point,
                    HitPurpose::Button(xrack_runtime::PointerButton::Secondary),
                )
            })
            .is_some_and(|target| {
                let scope = self.active_dialog(layout).unwrap_or_default();
                self.focused.as_ref() != Some(&target)
                    && target.path.starts_with(scope)
                    && context_menu_area(scene, &target.path, scope).is_none()
                    && self.context_editor(scene, layout, &target).is_some()
            })
    }

    fn finish_menu_action(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        path: Vec<usize>,
    ) -> Option<ControlInput> {
        let action = self.menu.as_ref()?.input(path)?;
        self.close_menu(scene, layout);
        Some(action)
    }

    pub fn menu_press(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        if !self.menu_open() {
            return false;
        }
        if !self.reconcile_text_menu(scene, layout) {
            return true;
        }
        let touch = self.menu.as_ref().is_some_and(|menu| menu.touch);
        self.cancel_press();
        if let Some(point) = self.pointer {
            if self
                .menu
                .as_mut()
                .is_some_and(|menu| menu.press(scene, layout, point))
            {
                return true;
            }
            if let Some(header) = self.target_at(scene, layout, point)
                && !self.menu_header_open(&header.path)
                && self.open_menu_bar_with_touch(scene, layout, header, touch)
            {
                return true;
            }
        }
        self.close_menu(scene, layout);
        true
    }

    pub fn menu_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        if !self.reconcile_text_menu(scene, layout) {
            return None;
        }
        let path = self.menu.as_mut()?.release(scene, layout, self.pointer?)?;
        self.finish_menu_action(scene, layout, path)
    }

    pub fn menu_scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) -> bool {
        let Some(popup) = &mut self.menu else {
            return false;
        };
        popup.scroll(scene, layout, self.pointer, rows);
        true
    }

    pub fn menu_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: MenuKey,
    ) -> Option<ControlInput> {
        if !self.reconcile_text_menu(scene, layout) {
            return None;
        }
        let Some(popup) = self.menu.as_mut() else {
            let target = self.focused.clone().filter(|target| {
                scene
                    .at_path(&target.path)
                    .is_some_and(|node| node.kind == "Menu")
            })?;
            match key {
                MenuKey::Up | MenuKey::Down | MenuKey::Accept => {
                    self.open_menu_bar(scene, layout, target);
                }
                MenuKey::Left | MenuKey::Right => {
                    if let Some(next) = sibling_menu(scene, layout, &target, key) {
                        self.set_focus(Some(next));
                    }
                }
                _ => {}
            }
            return None;
        };
        if key == MenuKey::Escape && popup.at_root() {
            self.close_menu(scene, layout);
            return None;
        }
        if key == popup.forward_key(scene, layout) && popup.enter_submenu(scene, layout) {
            return None;
        }
        if matches!(key, MenuKey::Left | MenuKey::Right)
            && popup.at_root()
            && popup.bar_header().is_some()
            && let Some(next) = sibling_menu(scene, layout, &popup.trigger, key)
        {
            let touch = popup.touch;
            self.open_menu_bar_with_touch(scene, layout, next, touch);
            return None;
        }
        let path = popup.key(scene, layout, key)?;
        self.finish_menu_action(scene, layout, path)
    }

    pub fn scroll_offsets(&self) -> &BTreeMap<Vec<usize>, f32> {
        &self.scroll_offsets
    }

    pub(crate) fn tree_root_changed(&self, path: &[usize], node: &Node) -> bool {
        node.tree.as_ref().is_some_and(|tree| {
            self.tree_scroll_roots
                .get(path)
                .is_some_and(|root| *root != tree.root_identity())
        })
    }

    pub fn horizontal_scroll_offsets(&self) -> &BTreeMap<Vec<usize>, f32> {
        &self.horizontal_scroll_offsets
    }

    pub(crate) fn scroll_selection_changed(
        &self,
        node: &Node,
        path: &[usize],
        columns: usize,
        direction: LayoutDirection,
    ) -> bool {
        self.scroll_selections
            .get(path)
            .is_some_and(|previous| *previous != ScrollSelection::new(node, columns, direction))
    }

    pub fn pointer_position(&self) -> Option<Point> {
        self.pointer
    }

    /// Return the click count for the live editor that owns the primary press.
    #[doc(hidden)]
    pub fn editor_mouse_press(
        &mut self,
        target: &Target,
        point: Point,
        now: Instant,
        selecting: bool,
    ) -> u8 {
        let previous = self.last_editor_click.take();
        self.editor_press = None;
        if selecting
            || self.pressed.as_ref() != Some(target)
            || self.focused.as_ref() != Some(target)
        {
            return 0;
        }
        let sample = (point, now);
        let count = previous
            .filter(|click| {
                click.count < 3
                    && click.target == *target
                    && crate::pointer_click_follows(click.sample, sample)
            })
            .map_or(1, |click| click.count + 1);
        self.editor_press = Some(EditorClick {
            target: target.clone(),
            count,
            sample,
        });
        count
    }

    #[doc(hidden)]
    pub fn clear_editor_click(&mut self) {
        self.editor_press = None;
        self.last_editor_click = None;
    }

    pub fn pointer_press_origin(&self) -> Option<Point> {
        self.press_origin
    }

    #[doc(hidden)]
    pub fn schedule_text_selection_scroll(
        &mut self,
        target: &Target,
        point: Option<Point>,
        now: Instant,
    ) {
        self.text_selection_scroll = point
            .filter(|_| self.pressed.as_ref() == Some(target) && self.editor_active(&target.path))
            .map(|point| TextSelectionScroll {
                target: target.clone(),
                point,
                due: self
                    .text_selection_scroll
                    .as_ref()
                    .filter(|scroll| &scroll.target == target)
                    .map_or(now + SCROLL_FRAME_INTERVAL, |scroll| scroll.due),
            });
    }

    #[doc(hidden)]
    pub fn text_selection_scroll_deadline(&self) -> Option<Instant> {
        let scroll = self.text_selection_scroll.as_ref()?;
        (self.pressed.as_ref() == Some(&scroll.target) && self.editor_active(&scroll.target.path))
            .then_some(scroll.due)
    }

    #[doc(hidden)]
    pub fn take_text_selection_scroll(&mut self, now: Instant) -> Option<(Point, Duration)> {
        let Some(due) = self.text_selection_scroll_deadline() else {
            self.text_selection_scroll = None;
            return None;
        };
        let elapsed = now.checked_duration_since(due)? + SCROLL_FRAME_INTERVAL;
        let scroll = self.text_selection_scroll.take()?;
        Some((scroll.point, elapsed.min(3 * SCROLL_FRAME_INTERVAL)))
    }

    pub fn text_scroll(&self, path: &[usize]) -> Point {
        self.text_offsets
            .get(path)
            .map_or(Point::default(), |(_, offset)| *offset)
    }

    pub fn set_text_scroll(&mut self, target: &Target, offset: Point) {
        if offset == Point::default() {
            self.text_offsets.remove(&target.path);
        } else {
            self.text_offsets
                .insert(target.path.clone(), (target.row_key, offset));
        }
    }

    pub fn scroll_y(&mut self, layout: &LayoutNode, delta: f32) -> bool {
        self.scroll_axis(layout, delta, Orientation::Vertical)
            .unwrap_or(false)
    }

    pub fn scroll_x(&mut self, layout: &LayoutNode, delta: f32) -> bool {
        self.scroll_axis(layout, delta, Orientation::Horizontal)
            .unwrap_or(false)
    }

    pub fn scroll_wheel(&mut self, layout: &LayoutNode, delta: Point, shift: bool) -> bool {
        if shift
            && delta.x == 0.0
            && let Some(moved) = self.scroll_axis(layout, delta.y, Orientation::Horizontal)
        {
            return moved;
        }
        let x = self.scroll_x(layout, delta.x);
        let y = self.scroll_y(layout, delta.y);
        x || y
    }

    /// Scroll only the named container, without pointer hit testing or ancestor bubbling.
    pub fn accessibility_scroll_to(
        &mut self,
        layout: &LayoutNode,
        path: &[usize],
        orientation: Orientation,
        offset: f32,
    ) -> bool {
        self.scroll_to_user_offset(layout, path, orientation, offset)
    }

    /// Return the primary vertical viewport in the active layer to its start.
    pub fn scroll_to_top(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        if self.menu_open() || self.choice_open() {
            return false;
        }
        let scope = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) else {
            return false;
        };
        if self
            .transitioning_stacks
            .iter()
            .any(|path| scope.starts_with(path))
        {
            return false;
        }
        let mut primary = None;
        primary_vertical_viewport(
            node,
            item,
            &mut scope.to_vec(),
            &self.transitioning_stacks,
            &mut primary,
        );
        primary.is_some_and(|(_, path)| {
            self.scroll_to_user_offset(layout, &path, Orientation::Vertical, 0.0)
        })
    }

    fn scroll_to_user_offset(
        &mut self,
        layout: &LayoutNode,
        path: &[usize],
        orientation: Orientation,
        offset: f32,
    ) -> bool {
        if !offset.is_finite()
            || self
                .active_dialog(layout)
                .is_some_and(|scope| !path.starts_with(scope))
        {
            return false;
        }
        let Some(mut item) = layout
            .at_path(path)
            .filter(|item| scroll_target_visible(item))
        else {
            return false;
        };
        let mut path = path;
        if let Some(bar) = &item.attached_scroll_bar {
            if bar.geometry.orientation != orientation || !self.allows_popup(layout, &bar.target) {
                return false;
            }
            path = &bar.target;
            let Some(viewport) = layout
                .at_path(path)
                .filter(|viewport| viewport.visible && viewport.enabled)
            else {
                return false;
            };
            item = viewport;
        }
        let Some(scroll) = item.scroll_on(orientation) else {
            return false;
        };
        if !self.set_viewport_offset(path, item, orientation, f64::from(offset)) {
            return false;
        }
        UserScrollChange::record(
            &mut self.user_scroll_changes,
            item,
            orientation,
            scroll.offset,
        );
        true
    }

    pub(crate) fn scroll_viewport_by(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
        delta: f64,
    ) -> bool {
        delta.is_finite()
            && delta != 0.0
            && self.update_viewport_offset(path, item, axis, |previous| f64::from(previous) + delta)
    }

    pub(crate) fn set_viewport_offset(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
        offset: f64,
    ) -> bool {
        offset.is_finite() && self.update_viewport_offset(path, item, axis, |_| offset)
    }

    pub(crate) fn current_viewport_offset(
        &self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
    ) -> Option<f32> {
        let scroll = item.scroll_on(axis)?;
        let offsets = match axis {
            Orientation::Horizontal => &self.horizontal_scroll_offsets,
            Orientation::Vertical => &self.scroll_offsets,
        };
        Some(
            item.bound_offset(axis)
                .or_else(|| pending_scroll_offset(item, offsets, path, axis))
                .unwrap_or(scroll.offset)
                .clamp(0.0, scroll.max_offset),
        )
    }

    fn update_viewport_offset(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
        next: impl FnOnce(f32) -> f64,
    ) -> bool {
        let Some(scroll) = item.scroll_on(axis) else {
            return false;
        };
        let had_overscroll = self.overscroll.has_offset();
        self.cancel_scroll_gesture();
        let previous = self
            .current_viewport_offset(path, item, axis)
            .unwrap_or(scroll.offset);
        let offsets = match axis {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        let next = next(previous).clamp(0.0, f64::from(scroll.max_offset)) as f32;
        if next == previous {
            return had_overscroll;
        }
        store_scroll_offset(item, offsets, path, axis, next);
        item.write_bound_offset(axis, next);
        true
    }

    fn scroll_bar_for_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &ScrollTarget,
    ) -> Option<ScrollBarGeometry> {
        let path = &target.path;
        if !path.starts_with(self.active_dialog(layout).unwrap_or_default()) {
            return None;
        }
        let item = layout.at_path(path)?;
        let node = scene.interactive_at_path(path)?;
        if !item.visible
            || !item.enabled
            || !target.matches_node(node)
            || scene.row_key_at(path) != target.row_key
        {
            return None;
        }
        if node.kind == "ScrollBar" && !node.boolean("interactive", true) {
            return None;
        }
        if let Some(bar) = &item.attached_scroll_bar
            && !self.allows_popup(layout, &bar.target)
        {
            return None;
        }
        item.scroll_bars(node)
            .find(|bar| bar.orientation == target.orientation && bar.scrollable())
    }

    /// Returns Some when the scroll bar captures the press.
    pub fn scroll_bar_press(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ScrollBarUpdate> {
        let target = self.hovered_scroll_bar.clone()?;
        self.press_scroll_bar(scene, layout, target, self.pointer?, false)
    }

    fn press_scroll_bar(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: ScrollTarget,
        point: Point,
        touch: bool,
    ) -> Option<ScrollBarUpdate> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        let item = layout.at_path(&target.path)?;
        let start = point;
        let point = item.map_from_window(point)?;
        let bar = self.scroll_bar_for_target(scene, layout, &target)?;
        if !bar.track.contains(point) {
            return None;
        }
        self.cancel_scroll_gesture();
        self.cancel_press();
        self.tooltip.dismiss();
        let node = scene.at_path(&target.path)?;
        if node.kind == "ScrollBar" {
            self.set_focus_with_reason(
                Some(Target::from_node(
                    target.path.clone(),
                    node,
                    true,
                    target.row_key,
                )),
                xrack_runtime::FocusReason::PointerClick,
            );
            self.remember_focus_scope(scene, layout);
        }
        let grab = bar.grab(point);
        let update = if grab.is_none() {
            self.move_scroll_bar(node, layout, &target.path, bar, bar.page_toward(point))
        } else {
            ScrollBarUpdate::Unchanged
        };
        self.scroll_bar_press = Some(ScrollBarPress {
            target,
            source: node.kind,
            viewport: node.scroll_target.clone(),
            reversed: bar.reversed(),
            grab,
            touch,
            cancelled: false,
            start,
            last: start,
            dragged: false,
        });
        Some(update)
    }

    /// Continue handling movement after the cursor leaves a captured thumb's track.
    pub fn scroll_bar_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ScrollBarUpdate> {
        self.update_scroll_bar_capture(scene, layout, self.pointer, false)
    }

    pub fn release_scroll_bar(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ScrollBarUpdate> {
        let point = self
            .pointer
            .or_else(|| self.scroll_bar_press.as_ref().map(|press| press.last));
        self.update_scroll_bar_capture(scene, layout, point, true)
    }

    fn update_scroll_bar_capture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Option<Point>,
        released: bool,
    ) -> Option<ScrollBarUpdate> {
        let mut press = self.scroll_bar_press.take()?;
        let update = point.map_or(ScrollBarUpdate::Unchanged, |point| {
            self.update_scroll_bar_drag(scene, layout, &mut press, point, released)
        });
        if !released {
            self.scroll_bar_press = Some(press);
        }
        Some(update)
    }

    fn update_scroll_bar_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        press: &mut ScrollBarPress,
        point: Point,
        released: bool,
    ) -> ScrollBarUpdate {
        let Some(grab) = press.grab.filter(|_| !press.cancelled) else {
            return ScrollBarUpdate::Unchanged;
        };
        let target = &press.target;
        let Some(node) = press.node(scene) else {
            press.cancelled = true;
            return ScrollBarUpdate::Unchanged;
        };
        let Some(bar) = self
            .scroll_bar_for_target(scene, layout, target)
            .filter(|bar| bar.reversed() == press.reversed)
        else {
            press.cancelled = true;
            return ScrollBarUpdate::Unchanged;
        };
        let mode = match node.properties.get("snap-mode") {
            Some(Value::ScrollSnapMode(mode)) => *mode,
            _ => ScrollSnapMode::None,
        };
        if released && mode == ScrollSnapMode::None {
            return ScrollBarUpdate::Unchanged;
        }
        let Some(item) = layout.at_path(&target.path) else {
            return ScrollBarUpdate::Unchanged;
        };
        let window_point = point;
        let (Some(point), Some(start)) = (
            item.map_from_window(point),
            item.map_from_window(press.start),
        ) else {
            return ScrollBarUpdate::Unchanged;
        };
        let mut fraction = bar.drag_fraction(point, grab);
        if !fraction.is_finite() {
            return ScrollBarUpdate::Unchanged;
        }
        press.last = window_point;
        press.dragged |= fraction != bar.drag_fraction(start, grab);
        if !press.dragged {
            return ScrollBarUpdate::Unchanged;
        }
        if mode == ScrollSnapMode::Always || released && mode == ScrollSnapMode::OnRelease {
            fraction = snap_fraction(fraction, node.number("step-size").unwrap_or(0.0));
        }
        self.move_scroll_bar_fraction(node, layout, &target.path, bar, fraction)
    }

    pub fn scroll_bar_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ScrollBarUpdate> {
        let target = self.focused.as_ref()?;
        let node = scene.interactive_at_path(&target.path)?;
        if node.kind != "ScrollBar"
            || !valid(target, scene, layout)
            || !self.allows_popup(layout, &target.path)
        {
            return None;
        }
        let item = layout.at_path(&target.path)?;
        let bar = item.scroll_control_geometry(node)?;
        let page = if bar.page > 0.0 { bar.page } else { bar.step };
        let offset = match (key, bar.orientation) {
            (NavigationKey::Home, _) => 0.0,
            (NavigationKey::End, _) => bar.scroll.max_offset,
            (NavigationKey::PageUp, _) => bar.scroll.offset - page,
            (NavigationKey::PageDown, _) => bar.scroll.offset + page,
            (NavigationKey::Left, Orientation::Horizontal)
            | (NavigationKey::Up, Orientation::Vertical) => {
                bar.scroll.offset + bar.logical_delta(-bar.step)
            }
            (NavigationKey::Right, Orientation::Horizontal)
            | (NavigationKey::Down, Orientation::Vertical) => {
                bar.scroll.offset + bar.logical_delta(bar.step)
            }
            _ => return None,
        };
        let path = target.path.clone();
        Some(self.move_scroll_bar(node, layout, &path, bar, offset))
    }

    fn move_scroll_bar(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        bar: ScrollBarGeometry,
        offset: f32,
    ) -> ScrollBarUpdate {
        if !bar.scrollable() {
            return ScrollBarUpdate::Unchanged;
        }
        let fraction =
            f64::from(offset.clamp(0.0, bar.scroll.max_offset)) / f64::from(bar.scroll.max_offset);
        self.move_scroll_bar_fraction(node, layout, path, bar, fraction)
    }

    fn move_scroll_bar_fraction(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        bar: ScrollBarGeometry,
        fraction: f64,
    ) -> ScrollBarUpdate {
        if node.kind == "ScrollBar" && node.scroll_target.is_none() {
            let action = RangeAction::SetFraction(fraction);
            if ScrollBarRange::from_node(node).apply(action).is_some() {
                return ScrollBarUpdate::Range(RangeInput {
                    path: path.to_vec(),
                    action,
                });
            }
        } else if self.scroll_to_user_offset(
            layout,
            path,
            bar.orientation,
            (fraction * f64::from(bar.scroll.max_offset)) as f32,
        ) {
            return ScrollBarUpdate::ViewportMoved;
        }
        ScrollBarUpdate::Unchanged
    }

    pub fn finish_scroll_bar(&mut self) -> bool {
        self.scroll_bar_press.take().is_some()
    }

    pub fn scroll_bar_captured(&self) -> bool {
        self.scroll_bar_press.is_some()
    }

    pub fn touch_scroll_bar_press(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<ScrollBarUpdate> {
        let Hit::ScrollBar(target) = self.hit_at(scene, layout, point) else {
            return None;
        };
        let moved = self.press_scroll_bar(scene, layout, target.clone(), point, true)?;
        self.pointer = Some(point);
        self.hovered_scroll_bar = Some(target);
        Some(moved)
    }

    pub fn touch_scroll_bar_drag(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> ScrollBarUpdate {
        self.pointer = Some(point);
        self.scroll_bar_drag(scene, layout)
            .unwrap_or(ScrollBarUpdate::Unchanged)
    }

    pub fn finish_touch_scroll_bar(&mut self) -> bool {
        if !self.finish_scroll_bar() {
            return false;
        }
        self.hovered_scroll_bar = None;
        self.pointer = None;
        true
    }

    pub fn release_touch_scroll_bar(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<ScrollBarUpdate> {
        let update = self.update_scroll_bar_capture(scene, layout, Some(point), true)?;
        self.hovered_scroll_bar = None;
        self.pointer = None;
        Some(update)
    }

    pub(crate) fn scroll_bar_interaction(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        path: &[usize],
        axis: Orientation,
    ) -> (bool, bool) {
        let matches = |target: &ScrollTarget| target.path == path && target.orientation == axis;
        let pressed = self.scroll_bar_press.as_ref().is_some_and(|press| {
            !press.cancelled
                && matches(&press.target)
                && press.node(scene).is_some()
                && self
                    .scroll_bar_for_target(scene, layout, &press.target)
                    .is_some_and(|bar| bar.reversed() == press.reversed)
        });
        let hovered = !self
            .scroll_bar_press
            .as_ref()
            .is_some_and(|press| press.touch)
            && self.hovered_scroll_bar.as_ref().is_some_and(|target| {
                matches(target) && self.scroll_bar_for_target(scene, layout, target).is_some()
            });
        (pressed, hovered)
    }

    pub fn scroll_bar_highlighted(&self, path: &[usize], orientation: Orientation) -> bool {
        let matches =
            |target: &ScrollTarget| target.path == path && target.orientation == orientation;
        self.hovered_scroll_bar.as_ref().is_some_and(matches)
            || self.scroll_bar_press.as_ref().is_some_and(|press| {
                !press.cancelled && press.grab.is_some() && matches(&press.target)
            })
    }

    fn scroll_axis(
        &mut self,
        layout: &LayoutNode,
        delta: f32,
        orientation: Orientation,
    ) -> Option<bool> {
        if self.pointer_on_stack_slide || !delta.is_finite() || delta == 0.0 {
            return None;
        }
        let point = self.pointer?;
        let scope = self.pointer_scope(layout, point);
        let scope_len = scope.len();
        let path = Self::scroll_path(layout, point, scope, |_, item| {
            item.scroll_on(orientation)
                .is_some_and(|scroll| scroll.max_offset > 0.0)
        })?;
        Some(
            self.consume_scroll(
                layout,
                &path,
                scope_len,
                delta,
                orientation,
                ScrollInput::Wheel,
            )
            .changed(),
        )
    }

    fn scroll_path(
        layout: &LayoutNode,
        point: Point,
        scope: &[usize],
        accepts: impl Fn(&[usize], &LayoutNode) -> bool,
    ) -> Option<Vec<usize>> {
        fn target(
            layout: &LayoutNode,
            point: Point,
            path: &mut Vec<usize>,
            accepts: &impl Fn(&[usize], &LayoutNode) -> bool,
        ) -> bool {
            if !layout.enabled || !layout.contains_subtree_point(point) {
                return false;
            }
            for (index, child) in layout
                .child_paint_order()
                .rev()
                .filter_map(|index| layout.child_at(index).map(|child| (index, child)))
            {
                path.push(index);
                if target(child, point, path, accepts) {
                    return true;
                }
                path.pop();
            }
            layout
                .map_from_window(point)
                .is_some_and(|local| layout.tab_viewport().contains(local))
                && accepts(path, layout)
        }

        let mut path = scope.to_vec();
        target(layout.at_path(scope)?, point, &mut path, &accepts).then_some(path)
    }

    fn consume_scroll(
        &mut self,
        layout: &LayoutNode,
        path: &[usize],
        scope_len: usize,
        delta: f32,
        orientation: Orientation,
        mut input: ScrollInput<'_>,
    ) -> AxisMotion {
        let mut result = AxisMotion::default();
        let mut delta = f64::from(delta);
        if let ScrollInput::Pan {
            owners,
            flicking: false,
            ..
        } = &mut input
        {
            let (remaining, owner) = self.overscroll.unwind(orientation, delta);
            delta = remaining;
            if let Some(owner) = owner {
                record_motion_owner(owners, &owner, orientation);
                result.elastic = true;
            }
        }
        let offsets = match orientation {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        let mut elastic_owner = None;
        let mut refresh_owner = None;
        // Only the portion the inner viewport could not consume reaches its ancestors.
        let last_depth = path.len();
        for depth in (scope_len..=last_depth).rev() {
            let path = &path[..depth];
            let Some(item) = layout.at_path(path) else {
                continue;
            };
            if item.tab_strip.is_some() && depth != last_depth {
                continue;
            }
            if let ScrollInput::Pan { device, .. } = &input
                && (!item.visible || !item.enabled || !item.accepts_pan(orientation, *device))
            {
                continue;
            }
            let Some(scroll) = item.scroll_on(orientation) else {
                continue;
            };
            if delta != 0.0
                && let ScrollInput::Pan {
                    touch_scene: Some(scene),
                    ..
                } = &input
                && let Some(node) = scene.at_path(path)
            {
                result.dismiss_keyboard |= node.boolean("dismiss-keyboard-on-drag", false);
            }
            let current = pending_scroll_offset(item, offsets, path, orientation)
                .unwrap_or(scroll.offset)
                .clamp(0.0, scroll.max_offset);
            let sign = if item.scroll_axis_reversed(orientation) {
                -1.0
            } else {
                1.0
            };
            let wanted = f64::from(current) + delta * sign;
            let bounded = wanted.clamp(0.0, f64::from(scroll.max_offset));
            // Bubble any residual in the original physical direction.
            delta = (wanted - bounded) * sign;
            if orientation == Orientation::Vertical
                && delta < 0.0
                && refresh_owner.is_none()
                && let ScrollInput::Pan {
                    touch_scene: Some(scene),
                    ..
                } = &input
                && scene.at_path(path).is_some_and(Node::can_pull_to_refresh)
                && item.viewport_handle.is_some()
            {
                refresh_owner = Some(item);
            }
            if let ScrollInput::Pan { flicking, .. } = &input
                && delta != 0.0
                && scroll.max_offset > 0.0
                && item.viewport_handle.is_some()
                && item
                    .elastic
                    .is_some_and(|options| options.behavior.allows_overshoot(*flicking))
            {
                elastic_owner = Some(item);
            }
            let next = bounded as f32;
            if next != current {
                store_scroll_offset(item, offsets, path, orientation, next);
                UserScrollChange::commit(
                    &mut self.user_scroll_changes,
                    item,
                    orientation,
                    scroll.offset,
                    next,
                );
                if let ScrollInput::Pan { owners, .. } = &mut input {
                    record_motion(owners, item, orientation);
                }
                result.bounded = true;
            }
            if delta == 0.0 {
                break;
            }
        }
        if let Some(item) = refresh_owner
            && delta < 0.0
            && !self.overscroll.is_rebounding(orientation)
            && self
                .overscroll
                .pull_to_refresh(item, delta, self.reduce_motion)
        {
            result.elastic = true;
            if let ScrollInput::Pan { owners, .. } = &mut input {
                record_motion(owners, item, orientation);
            }
        } else if let Some(item) = elastic_owner
            && !self.overscroll.is_rebounding(orientation)
            && self.overscroll.pull(item, orientation, delta)
        {
            result.elastic = true;
            if let ScrollInput::Pan {
                owners,
                flicking: false,
                ..
            } = &mut input
            {
                record_motion(owners, item, orientation);
            }
        }
        result
    }

    /// Arm a left-button pan after ordinary press handling. The press stays
    /// with its original control until movement crosses the drag threshold.
    pub fn arm_mouse_viewport_pan(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> bool {
        if self.press_origin != Some(point)
            || self.drag.is_some()
            || self.split_resize_active()
            || self.scroll_bar_press.is_some()
            || self.table_resize.is_some()
            || self.table_row_resize.is_some()
            || self.table_column_move.is_some()
            || self.table_row_move.is_some()
            || self.pressed_table_column.is_some()
            || self.menu_open()
            || self.choice_open()
        {
            return false;
        }
        let hit = self.hit_at(scene, layout, point);
        let hit_path = match &hit {
            Hit::None => None,
            Hit::Target(target)
                if scene.at_path(&target.path).is_some_and(|node| {
                    matches!(
                        node.kind,
                        "Button"
                            | "ToolButton"
                            | "RoundButton"
                            | "TouchArea"
                            | "ListRow"
                            | "ListView"
                            | "GridView"
                            | "StandardListView"
                            | "TableView"
                            | "TreeView"
                    ) || is_toggle(node.kind)
                }) =>
            {
                Some(target.path.as_slice())
            }
            _ => return false,
        };
        let scope = self.pointer_scope(layout, point).to_vec();
        let orientations = [Orientation::Horizontal, Orientation::Vertical];
        let can_pan = |item: &LayoutNode, axis| {
            item.accepts_pan(axis, PanDevice::LeftMouse)
                && item
                    .scroll_on(axis)
                    .is_some_and(|scroll| scroll.max_offset > 0.0)
        };
        let Some(path) = Self::scroll_path(layout, point, &scope, |path, item| {
            hit_path.is_none_or(|target| target.starts_with(path))
                && orientations.into_iter().any(|axis| can_pan(item, axis))
        }) else {
            return false;
        };
        let axes = orientations.map(|orientation| {
            (scope.len()..=path.len()).rev().find_map(|depth| {
                let path = &path[..depth];
                let item = layout.at_path(path)?;
                let node = scene.at_path(path)?;
                can_pan(item, orientation).then(|| ScrollTarget {
                    path: path.to_vec(),
                    orientation,
                    row_key: scene.row_key_at(path),
                    id: ElementId::of(node),
                })
            })
        });
        self.scroll_gesture = Some(ScrollGesture {
            route: ScrollRoute {
                scope_row: scene.row_key_at(&scope),
                scope_id: scene.at_path(&scope).and_then(ElementId::of),
                scope,
                axes,
                mouse_viewport: true,
                pan_device: PanDevice::LeftMouse,
            },
            motion: DragMotion::new(point, now),
            tap: None,
            touch_point: None,
            popup: (false, false),
            menu_pan: None,
            drawer: None,
            pages: Vec::new(),
            tumbler: None,
            tab_move: None,
            switch: None,
            swipe_delegate: None,
            swipe: None,
            delay_touch: false,
            table_gesture: None,
        });
        true
    }

    pub fn mouse_viewport_pan_claimed(&self) -> bool {
        self.scroll_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.route.mouse_viewport && gesture.motion.dragging)
    }

    /// Capture scroll owners at contact start. Touches become taps only on release.
    pub fn begin_scroll_gesture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
        kind: ScrollGestureKind,
    ) -> bool {
        self.begin_scroll_gesture_with_selection(scene, layout, point, now, kind, false)
    }

    #[doc(hidden)]
    pub fn begin_scroll_gesture_with_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
        kind: ScrollGestureKind,
        additive: bool,
    ) -> bool {
        self.scroll_keyboard_dismissal = false;
        self.swipe_commit = None;
        if !point.x.is_finite() || !point.y.is_finite() {
            return false;
        }
        let hit = self.hit_at(scene, layout, point);
        // Let the time picker's footer receive a tap even while its wheel moves.
        let picker_button = match (&hit, &self.tumbler_flick) {
            (Hit::Target(target), Some(flick)) => self
                .dialog_button(scene, layout, target)
                .is_some_and(|button| self.time_picker_flick(flick, scene, layout, &button.path)),
            _ => false,
        };
        let tumbler_interrupted = !picker_button && self.interrupt_tumbler_flick();
        let interrupted = tumbler_interrupted
            || self.scroll_fling.is_some()
            || self.overscroll.deadline().is_some();
        self.scroll_fling = None;
        if interrupted {
            self.viewport_motion.clear();
        }
        let touch = kind == ScrollGestureKind::Touch;
        if matches!(
            kind,
            ScrollGestureKind::Touch | ScrollGestureKind::MouseControl
        ) && !interrupted
            && self.start_table_selection_handle(scene, layout, point, touch, true)
        {
            return true;
        }
        let popup = (self.menu_open(), self.choice_open());
        let scope = self.pointer_scope(layout, point).to_vec();
        if !popup.0 && !popup.1 && matches!(hit, Hit::StackSlide) {
            return false;
        }
        if kind == ScrollGestureKind::MouseControl
            && !popup.0
            && !popup.1
            && let Hit::Target(target) = &hit
            && split_target(target, scene, layout).is_some()
        {
            return false;
        }
        let tumbler = match &hit {
            Hit::Target(target) | Hit::ControlPart(target, _) if !popup.0 && !popup.1 => scene
                .at_path(&target.path)
                .zip(layout.at_path(&target.path))
                .and_then(|(node, item)| {
                    matches!(node.kind, "Tumbler" | "PathView").then_some(())?;
                    TumblerDrag::new(target.clone(), node, item, item.map_from_window(point)?)
                }),
            _ => None,
        };
        if kind == ScrollGestureKind::MouseTumbler && tumbler.is_none() {
            return false;
        }
        let tab_move = match &hit {
            Hit::Target(target)
                if popup == (false, false)
                    && matches!(
                        kind,
                        ScrollGestureKind::Touch | ScrollGestureKind::MouseControl
                    ) =>
            {
                TabDrag::new(target, scene, layout, point)
            }
            _ => None,
        };
        let switch = match &hit {
            Hit::Target(target)
                if popup == (false, false)
                    && matches!(
                        kind,
                        ScrollGestureKind::Touch | ScrollGestureKind::MouseControl
                    ) =>
            {
                scene
                    .at_path(&target.path)
                    .zip(layout.at_path(&target.path))
                    .and_then(|(node, item)| SwitchDrag::new(target.clone(), node, item, point))
            }
            _ => None,
        };
        let swipe_delegate = match &hit {
            Hit::Target(target)
                if !popup.0 && !popup.1 && kind != ScrollGestureKind::MouseTumbler =>
            {
                (scope.len()..=target.path.len()).rev().find_map(|depth| {
                    let path = &target.path[..depth];
                    let node = scene.interactive_at_path(path)?;
                    let item = layout.at_path(path)?;
                    let ancestor = Target::from_node(
                        path.to_vec(),
                        node,
                        focusable(node),
                        scene.row_key_at(path),
                    );
                    SwipeDrag::new(ancestor, node, item, item.map_from_window(point)?)
                })
            }
            _ => None,
        };
        let table_gesture = match &hit {
            Hit::Target(target)
                if !interrupted
                    && popup == (false, false)
                    && matches!(
                        kind,
                        ScrollGestureKind::Touch | ScrollGestureKind::MouseControl
                    ) =>
            {
                self.table_gesture_at(scene, layout, target, point, touch, now, additive)
            }
            _ => None,
        };
        let swipe = (!popup.0
            && !popup.1
            && tumbler.is_none()
            && tab_move.is_none()
            && swipe_delegate.is_none()
            && table_gesture
                .as_ref()
                .is_none_or(|hold| touch && hold.deadline.is_some())
            && matches!(
                kind,
                ScrollGestureKind::Touch | ScrollGestureKind::MouseControl
            ))
        .then(|| self.swipe_route(scene, layout, point, &hit))
        .flatten();
        if kind == ScrollGestureKind::MouseControl
            && tumbler.is_none()
            && tab_move.is_none()
            && switch.is_none()
            && swipe_delegate.is_none()
            && swipe.is_none()
            && table_gesture.is_none()
        {
            return false;
        }
        let left_mouse_pan =
            kind == ScrollGestureKind::MouseControl && (swipe.is_some() || table_gesture.is_some());
        let pan_device = match kind {
            ScrollGestureKind::Touch => PanDevice::Touch,
            ScrollGestureKind::MouseControl if left_mouse_pan => PanDevice::LeftMouse,
            _ => PanDevice::MiddleMouse,
        };
        let pages = if popup.0 || popup.1 || left_mouse_pan {
            Vec::new()
        } else {
            Self::page_swipes(scene, layout, point, &scope, &hit)
        };
        let drawer =
            if popup.0 || popup.1 || left_mouse_pan || self.popup_at(layout, point).is_some() {
                None
            } else {
                self.drawer_swipe(scene, layout, point)
            };
        let axes = [Orientation::Horizontal, Orientation::Vertical].map(|orientation| {
            if popup.0 || popup.1 {
                return None;
            }
            let path = Self::scroll_path(layout, point, &scope, |path, item| {
                item.accepts_pan(orientation, pan_device)
                    && item.scroll_on(orientation).is_some_and(|scroll| {
                        scroll.max_offset > 0.0
                            || (touch
                                && scene.at_path(path).is_some_and(|node| {
                                    node.boolean("dismiss-keyboard-on-drag", false)
                                        || (orientation == Orientation::Vertical
                                            && node.can_pull_to_refresh())
                                }))
                    })
            })?;
            let row_key = scene.row_key_at(&path);
            let id = ElementId::of(scene.at_path(&path)?);
            Some(ScrollTarget {
                path,
                orientation,
                row_key,
                id,
            })
        });
        if kind == ScrollGestureKind::MousePan
            && drawer.is_none()
            && pages.is_empty()
            && tumbler.is_none()
            && tab_move.is_none()
            && swipe_delegate.is_none()
            && axes.iter().all(Option::is_none)
        {
            return false;
        }
        let delay_target = match &hit {
            Hit::Target(target) if kind == ScrollGestureKind::Touch && !popup.0 && !popup.1 => {
                scene
                    .at_path(&target.path)
                    .filter(|node| node.kind == "DelayButton" && !node.boolean("checked", false))
                    .map(|_| target.clone())
            }
            _ => None,
        };
        let repeat_target = match &hit {
            Hit::Target(target)
                if kind == ScrollGestureKind::Touch && !interrupted && popup == (false, false) =>
            {
                Some(target.clone())
            }
            _ => None,
        };
        let tap = (kind != ScrollGestureKind::MousePan && !interrupted).then_some(hit);
        let overscroll = if drawer.is_none()
            && pages.is_empty()
            && tumbler.is_none()
            && tab_move.is_none()
            && switch.is_none()
            && swipe_delegate.is_none()
            && swipe.is_none()
        {
            self.take_drag_overscroll(layout, &axes, scope.len(), pan_device)
        } else {
            Overscroll::default()
        };
        let picker_flick = if picker_button {
            self.tumbler_flick.take()
        } else {
            None
        };
        self.cancel_press();
        self.tumbler_flick = picker_flick;
        self.overscroll = overscroll;
        self.pointer_leave();
        let delay_touch = delay_target.is_some_and(|target| {
            self.set_focus(Some(target.clone()));
            self.begin_delay(scene, layout, target, DelaySource::Touch(point), now)
        });
        if let Some(target) = repeat_target {
            self.begin_button_hold(scene, layout, target, DelaySource::Touch(point), now);
        }
        if popup == (false, false)
            && let Some(Hit::Target(target)) = &tap
        {
            self.start_button_contact(scene, target.clone(), ButtonContactSource::Gesture);
        }
        let menu_pan = if touch {
            self.menu
                .as_ref()
                .and_then(|menu| menu.pan_at(scene, layout, point))
        } else {
            None
        };
        self.scroll_gesture = Some(ScrollGesture {
            route: ScrollRoute {
                scope_row: scene.row_key_at(&scope),
                scope_id: scene.at_path(&scope).and_then(ElementId::of),
                scope,
                axes,
                mouse_viewport: false,
                pan_device,
            },
            motion: DragMotion::new(point, now),
            tap,
            touch_point: (kind == ScrollGestureKind::Touch).then_some(point),
            popup,
            menu_pan,
            drawer,
            pages,
            tumbler,
            tab_move,
            switch,
            swipe_delegate,
            swipe,
            delay_touch,
            table_gesture,
        });
        true
    }

    fn swipe_route(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        hit: &Hit,
    ) -> Option<SwipeRoute> {
        match hit {
            Hit::Target(target)
                if scene.at_path(&target.path).is_some_and(|node| {
                    is_text_input(node.kind)
                        || is_spin_box(node.kind)
                        || combo_editable(node)
                        || matches!(
                            node.kind,
                            "Slider"
                                | "ColorPlane"
                                | "RangeThumb"
                                | "RangeSlider"
                                | "Dial"
                                | "SplitHandle"
                                | "DelayButton"
                                | "TableCell"
                                | "TreeCell"
                                | "ComboBox"
                        )
                }) =>
            {
                return None;
            }
            Hit::Target(_) | Hit::None | Hit::Blocked(_) => {}
            _ => return None,
        }
        let (path, scope_len) =
            self.gesture_hit_path(scene, layout, point, "SwipeGestureHandler")?;
        let targets = (scope_len..=path.len())
            .rev()
            .filter_map(|depth| SwipeTarget::new(scene, layout, &path[..depth], point))
            .collect::<Vec<_>>();
        if !targets.is_empty()
            && crate::drag::DragSession::arm(self.drag_context(scene, layout, point), point, None)
                .is_some()
        {
            return None;
        }
        (!targets.is_empty()).then_some(SwipeRoute::Pending(targets))
    }

    fn page_swipes(
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        scope: &[usize],
        hit: &Hit,
    ) -> Vec<PageSwipe> {
        let hit_path = match hit {
            Hit::Target(target) => &target.path,
            Hit::Blocked(path) => path,
            _ => return Vec::new(),
        };
        (scope.len()..=hit_path.len())
            .rev()
            .filter_map(|depth| {
                let path = &hit_path[..depth];
                let view = scene.interactive_at_path(path)?;
                let item = layout.at_path(path)?;
                if view.kind != "SwipeView"
                    || !view.boolean("interactive", true)
                    || !item.enabled
                    || !item.visible
                    || !(item.contains_subtree_point(point))
                {
                    return None;
                }
                let current = SwipePagePosition::from_view(view, group_index(view)?)?;
                let neighbor = |forward| {
                    let index = swipe_neighbor(view, current.index, forward)?;
                    SwipePagePosition::from_view(view, index)
                };
                let previous = neighbor(false);
                let next = neighbor(true);
                let orientation = view.orientation();
                let extent = match orientation {
                    Orientation::Horizontal => item.bounds.width,
                    Orientation::Vertical => item.bounds.height,
                };
                (previous.is_some() || next.is_some()).then(|| PageSwipe {
                    target: ScrollTarget {
                        path: path.to_vec(),
                        row_key: scene.row_key_at(path),
                        orientation,
                        id: ElementId::of(view),
                    },
                    current,
                    previous,
                    next,
                    mirrored: swipe_view_mirrored(view, item),
                    start: item.map_from_window(point).unwrap_or(point),
                    bounds: item.bounds,
                    threshold: (extent * 0.15).clamp(24.0, 96.0),
                })
            })
            .collect()
    }

    fn page_scroll_can_move(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        route: &ScrollRoute,
        page: &PageSwipe,
        forward: bool,
    ) -> bool {
        let (axis, offsets) = match page.target.orientation {
            Orientation::Horizontal => (0, &self.horizontal_scroll_offsets),
            Orientation::Vertical => (1, &self.scroll_offsets),
        };
        let Some(target) = &route.axes[axis] else {
            return false;
        };
        if !target.path.starts_with(&page.target.path) || target.resolve(scene, layout).is_none() {
            return false;
        }
        (page.target.path.len() + 1..=target.path.len()).any(|depth| {
            let path = &target.path[..depth];
            layout.at_path(path).is_some_and(|item| {
                item.scroll_on(target.orientation).is_some_and(|scroll| {
                    let offset = pending_scroll_offset(item, offsets, path, target.orientation)
                        .unwrap_or(scroll.offset)
                        .clamp(0.0, scroll.max_offset);
                    if forward != item.scroll_axis_reversed(target.orientation) {
                        offset < scroll.max_offset
                    } else {
                        offset > 0.0
                    }
                })
            })
        })
    }

    fn drawer_swipe(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<DrawerSwipe> {
        let active = self.active_dialog(layout);
        // Use the same window viewport that positions the drawer.
        let viewport = crate::window_safe_area(scene, layout.bounds);
        layout
            .drawer_paths()
            .iter()
            .rev()
            .filter_map(|path| {
                let node = scene.control_target(path, ControlAction::Drawer(true))?;
                let open = node.boolean("open", false);
                let panel = if open {
                    if active != Some(path.as_slice()) {
                        return None;
                    }
                    layout.at_path(path)?.bounds
                } else {
                    if active.is_some() {
                        return None;
                    }
                    modal::drawer_bounds(node, viewport)
                };
                let edge = node.drawer_edge();
                let region = if open { panel } else { viewport };
                let span = modal::drawer_extent(edge, panel);
                let margin = modal::drawer_margin(node, panel);
                let activation_edge = if open { modal::opposite(edge) } else { edge };
                let distance = modal::drawer_edge_distance(activation_edge, region, point);
                (region.contains(point) && distance < margin).then(|| {
                    (
                        distance,
                        DrawerSwipe {
                            path: path.clone(),
                            row_key: scene.row_key_at(path),
                            id: ElementId::of(node),
                            edge,
                            open,
                            start: point,
                            threshold: (span * 0.2).clamp(20.0, 80.0),
                        },
                    )
                })
            })
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, swipe)| swipe)
    }

    fn scroll_route_is_current(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        route: &ScrollRoute,
    ) -> bool {
        (self.active_dialog(layout).unwrap_or_default() == route.scope
            || self.popups(layout).any(|path| path == route.scope))
            && scene.row_key_at(&route.scope) == route.scope_row
            && scene.at_path(&route.scope).and_then(Node::element_id)
                == route.scope_id.as_ref().map(ElementId::as_str)
            && (!route.mouse_viewport
                || route.axes.iter().flatten().any(|target| {
                    target.resolve(scene, layout).is_some_and(|(_, item)| {
                        item.accepts_pan(target.orientation, PanDevice::LeftMouse)
                    })
                }))
    }

    fn scroll_captured(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        route: &ScrollRoute,
        delta: Point,
        flicking: bool,
    ) -> ([AxisMotion; 2], MotionOwners) {
        let mut owners = MotionOwners::new();
        if !self.scroll_route_is_current(scene, layout, route) {
            return ([AxisMotion::default(); 2], owners);
        }
        let moved = std::array::from_fn(|index| {
            let Some(target) = &route.axes[index] else {
                return AxisMotion::default();
            };
            if flicking && self.overscroll.owns_axis(target.orientation) {
                return AxisMotion::default();
            }
            let Some((_, item)) = target.resolve(scene, layout) else {
                return AxisMotion::default();
            };
            let Some(delta) = item.map_delta_from_window(delta) else {
                return AxisMotion::default();
            };
            let movement = [delta.x, delta.y][index];
            if movement == 0.0 {
                return AxisMotion::default();
            }
            let path = &target.path;
            self.consume_scroll(
                layout,
                path,
                route.scope.len(),
                movement,
                target.orientation,
                ScrollInput::Pan {
                    device: route.pan_device,
                    owners: &mut owners,
                    flicking,
                    touch_scene: (!flicking
                        && route.pan_device == PanDevice::Touch
                        && movement.abs() >= [delta.y, delta.x][index].abs())
                    .then_some(scene),
                },
            )
        });
        (moved, owners)
    }

    fn take_drag_overscroll(
        &mut self,
        layout: &LayoutNode,
        axes: &[Option<ScrollTarget>; 2],
        scope_len: usize,
        device: PanDevice,
    ) -> Overscroll {
        let mut overscroll = std::mem::take(&mut self.overscroll);
        overscroll.clear_refresh();
        overscroll.resume_drag(|axis, handle| {
            let Some(target) = &axes[usize::from(axis == Orientation::Vertical)] else {
                return false;
            };
            (scope_len..=target.path.len()).rev().any(|depth| {
                layout.at_path(&target.path[..depth]).is_some_and(|item| {
                    item.visible
                        && item.enabled
                        && item.accepts_pan(axis, device)
                        && item.viewport_handle.as_ref() == Some(handle)
                        && item
                            .elastic
                            .is_some_and(|options| options.behavior.allows_overshoot(false))
                })
            })
        });
        overscroll
    }

    pub fn move_scroll_gesture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> bool {
        self.scroll_keyboard_dismissal = false;
        if !point.x.is_finite() || !point.y.is_finite() {
            return false;
        }
        let claimed = self.advance_table_touch_hold(scene, layout, now);
        let wake = self
            .table_row_move
            .as_mut()
            .filter(|drag| drag.touch)
            .map(|drag| &mut drag.scroll_wake)
            .or_else(|| {
                self.table_drag_selection
                    .as_mut()
                    .filter(|drag| drag.scroll_capture)
                    .map(|drag| &mut drag.scroll_wake)
            });
        if let Some(wake) = wake {
            if self.pointer == Some(point) {
                return claimed;
            }
            self.pointer = Some(point);
            wake.get_or_insert(now + SCROLL_FRAME_INTERVAL);
            return true;
        }
        let Some(mut gesture) = self.scroll_gesture.take() else {
            return false;
        };
        if gesture.touch_point.is_some() {
            gesture.touch_point = Some(point);
        }
        if !self.scroll_route_is_current(scene, layout, &gesture.route)
            || gesture.popup != (self.menu_open(), self.choice_open())
        {
            self.viewport_motion.clear();
            let cleared = self.overscroll.clear() | self.swipe_page_motion.take().is_some();
            self.swipe_events
                .extend(gesture.swipe.take().and_then(SwipeRoute::cancel));
            self.cancel_touch_delay();
            self.cancel_touch_button_hold();
            let tumbler_cleared = gesture.tumbler.is_some_and(|mut drag| drag.cancel());
            return cleared
                || tumbler_cleared
                || (gesture.motion.dragging && gesture.tab_move.is_some())
                || gesture.switch.is_some_and(|drag| drag.position.is_some())
                || gesture
                    .swipe_delegate
                    .is_some_and(|drag| drag.has_preview());
        }
        if gesture.delay_touch
            && let Some(hold) = &mut self.delay_hold
            && matches!(hold.source, DelaySource::Touch(_))
        {
            hold.source = DelaySource::Touch(point);
            if !layout
                .at_path(&hold.target.path)
                .is_some_and(|item| item.contains_subtree_point(point))
            {
                self.cancel_delay();
            }
        }
        if let Some(repeat) = &mut self.button_hold
            && matches!(repeat.source, DelaySource::Touch(_))
        {
            repeat.source = DelaySource::Touch(point);
            if repeat.moved_too_far(point)
                || !layout
                    .at_path(&repeat.target.path)
                    .is_some_and(|item| item.contains_subtree_point(point))
            {
                self.cancel_touch_button_hold();
            }
        }
        let was_dragging = gesture.motion.dragging;
        if gesture.route.mouse_viewport && !was_dragging {
            let delta = gesture.motion.delta_to(point);
            let [x, y] = gesture.route.axes.each_ref().map(|target| {
                let Some(target) = target else {
                    return 0.0;
                };
                let Some((_, item)) = target.resolve(scene, layout) else {
                    return 0.0;
                };
                if !item.accepts_pan(target.orientation, PanDevice::LeftMouse) {
                    return 0.0;
                }
                item.map_delta_from_window(delta)
                    .map_or(0.0, |local| match target.orientation {
                        Orientation::Horizontal => local.x,
                        Orientation::Vertical => local.y,
                    })
            });
            if x.hypot(y) < DRAG_THRESHOLD {
                self.scroll_gesture = Some(gesture);
                return false;
            }
        }
        let elastic_frame = self.overscroll.frame();
        let scrolled = gesture.motion.update(point, now).is_some_and(|delta| {
            if let Some(pan) = &mut gesture.menu_pan {
                return self
                    .menu
                    .as_mut()
                    .is_some_and(|menu| pan.advance(menu, scene, layout, delta.y));
            }
            let table_gesture = gesture.table_gesture.take();
            self.finish_button_contact(ButtonContactSource::Gesture, false);
            self.cancel_touch_button_hold();
            if let Some(hold) = table_gesture.filter(|hold| hold.deadline.is_none())
                && let PendingTableAction::Select(mut drag) = hold.drag
            {
                drag.claimed = true;
                drag.touch = true;
                drag.scroll_capture = true;
                if self.table_drag_route(scene, layout, &drag, point).is_some() {
                    self.cancel_touch_delay();
                    self.overscroll.clear();
                    self.viewport_motion.clear();
                    self.capture_table_selection(drag);
                    self.pointer = Some(point);
                    return self.move_table_drag_selection(layout, now);
                }
            }
            if gesture.route.mouse_viewport && !was_dragging {
                let overscroll = self.take_drag_overscroll(
                    layout,
                    &gesture.route.axes,
                    gesture.route.scope.len(),
                    gesture.route.pan_device,
                );
                self.cancel_press();
                self.overscroll = overscroll;
                self.pointer_leave();
            }
            if gesture.delay_touch {
                self.cancel_touch_delay();
            }
            if let Some(drag) = &mut gesture.tab_move {
                if was_dragging || drag.accepts_direction(scene, layout, point) {
                    return drag.update(scene, layout, point, now);
                }
                gesture.tab_move = None;
            }
            if let Some(drag) = &mut gesture.tumbler {
                if let Some(local) = layout
                    .at_path(&drag.target.path)
                    .and_then(|item| item.map_from_window(point))
                    && (was_dragging || drag.accepts_direction(local))
                {
                    return drag.update(scene, layout, local);
                }
                gesture.tumbler = None;
            }
            if let Some(drag) = &mut gesture.switch {
                if was_dragging || drag.accepts_direction(layout, point) {
                    return drag.move_thumb(scene, layout, point);
                }
                gesture.switch = None;
            }
            if !was_dragging && let Some(swipe) = &gesture.drawer {
                let (distance, across) = swipe.movement(point);
                if distance <= across {
                    gesture.drawer = None;
                }
            }
            if gesture.drawer.is_some() {
                gesture.swipe_delegate = None;
                return false;
            }
            if !was_dragging
                && gesture.swipe_delegate.as_ref().is_some_and(|swipe| {
                    layout
                        .at_path(&swipe.target.path)
                        .and_then(|item| item.map_from_window(point))
                        .is_none_or(|local| !swipe.accepts_direction(local))
                })
            {
                gesture.swipe_delegate = None;
            }
            if let Some(swipe) = &mut gesture.swipe_delegate {
                return layout
                    .at_path(&swipe.target.path)
                    .and_then(|item| item.map_from_window(point))
                    .is_some_and(|local| swipe.update(scene, layout, local));
            }
            if self.move_custom_swipe(scene, layout, &mut gesture.swipe, point) {
                return false;
            }
            if !was_dragging {
                let selected = gesture.pages.iter().position(|page| {
                    let Some(local) = layout
                        .at_path(&page.target.path)
                        .and_then(|item| item.map_from_window(point))
                    else {
                        return false;
                    };
                    let (distance, across) = page.movement(local);
                    let forward = distance > 0.0;
                    distance.abs() > across
                        && page.destination(forward).is_some()
                        && !self.page_scroll_can_move(scene, layout, &gesture.route, page, forward)
                });
                if let Some(index) = selected {
                    gesture.pages.swap(0, index);
                    gesture.pages.truncate(1);
                } else {
                    gesture.pages.clear();
                }
            }
            if let Some(page) = gesture.pages.first() {
                return self.update_swipe_page_motion(page, scene, layout, point);
            }
            let (moved, owners) = self.scroll_captured(scene, layout, &gesture.route, delta, false);
            self.scroll_keyboard_dismissal = moved.iter().any(|axis| axis.dismiss_keyboard);
            if moved.into_iter().any(AxisMotion::changed) {
                self.viewport_motion
                    .update(owners, ViewportMotion::Dragging);
                true
            } else {
                false
            }
        });
        if !self.scroll_table_selection_claimed() {
            self.scroll_gesture = Some(gesture);
        }
        scrolled || elastic_frame != self.overscroll.frame()
    }

    /// Whether the last direct scroll movement reached an opt-in viewport.
    #[doc(hidden)]
    pub fn take_scroll_keyboard_dismissal(&mut self) -> bool {
        std::mem::take(&mut self.scroll_keyboard_dismissal)
    }

    fn update_swipe_page_motion(
        &mut self,
        page: &PageSwipe,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> bool {
        let preview = (|| {
            let (view, item) = page.view(scene, layout)?;
            let duration = Duration::from_millis(view.int("transition-duration")?.max(0) as u64);
            if duration.is_zero() {
                return None;
            }
            let handle = &view.swipe_preview.as_ref()?.handle;
            let (distance, _) = page.movement(item.map_from_window(point)?);
            let extent = match page.target.orientation {
                Orientation::Horizontal => item.bounds.width,
                Orientation::Vertical => item.bounds.height,
            };
            if extent <= 0.0 || distance == 0.0 {
                return None;
            }
            let destination = page.live_destination(view, distance > 0.0)?;
            Some((
                handle,
                destination,
                (distance / extent).clamp(-1.0, 1.0),
                duration,
            ))
        })();
        let Some((handle, destination, fraction, duration)) = preview else {
            return self.swipe_page_motion.take().is_some();
        };
        if self
            .swipe_page_motion
            .as_ref()
            .is_some_and(|motion| motion.lease.0 != *handle)
        {
            self.swipe_page_motion = None;
        }
        let motion = self
            .swipe_page_motion
            .get_or_insert_with(|| SwipePageMotion {
                page: page.clone(),
                lease: Rc::new(SwipePreviewLease(handle.clone())),
                fraction: 0.0,
                duration,
                settling: None,
                wake: None,
            });
        let changed = motion.fraction != fraction || motion.settling.is_some();
        motion.page = page.clone();
        motion.fraction = fraction;
        motion.duration = duration;
        motion.settling = None;
        motion.wake = None;
        handle.request(
            (page.current.index, page.current.identity.clone()),
            (destination.index, destination.identity.clone()),
        );
        changed
    }

    #[doc(hidden)]
    pub fn swipe_page_motion(&self, path: &[usize]) -> Option<(usize, f32, Orientation)> {
        let motion = self.swipe_page_motion.as_ref()?;
        (motion.page.target.path == path).then_some(())?;
        let to = motion.page.destination(motion.fraction > 0.0)?;
        Some((to.index, motion.fraction, motion.page.target.orientation))
    }

    pub(crate) fn swipe_commit_fraction(&self, path: &[usize], old: &Node, new: &Node) -> f32 {
        let Some((page, fraction)) = &self.swipe_commit else {
            return 0.0;
        };
        let Some((slot, _)) = old
            .swipe_preview
            .as_ref()
            .and_then(|preview| preview.page.as_ref())
        else {
            return 0.0;
        };
        if page.target.path != path
            || group_index(old) != Some(page.current.index)
            || !page.current.matches(old)
        {
            return 0.0;
        }
        page.destination(*fraction > 0.0)
            .filter(|to| to.index == *slot && group_index(new) == Some(to.index) && to.matches(new))
            .map_or(0.0, |_| *fraction)
    }

    fn move_custom_swipe(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        swipe: &mut Option<SwipeRoute>,
        point: Point,
    ) -> bool {
        if matches!(swipe, Some(SwipeRoute::Pending(_)))
            && let Some(SwipeRoute::Pending(targets)) = swipe.take()
        {
            *swipe = targets
                .into_iter()
                .find(|target| target.accepts(scene, layout, self, point))
                .map(SwipeRoute::Captured);
        }
        match swipe {
            Some(SwipeRoute::Captured(target))
                if target.is_live(scene, layout, self) && !target.cancel_requested() =>
            {
                self.swipe_events.extend(target.update(point));
                true
            }
            Some(SwipeRoute::Captured(_)) => {
                self.swipe_events.extend(
                    swipe
                        .replace(SwipeRoute::Cancelled)
                        .and_then(SwipeRoute::cancel),
                );
                true
            }
            Some(SwipeRoute::Cancelled) => true,
            _ => false,
        }
    }

    pub fn end_scroll_gesture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> ScrollGestureEnd {
        self.cancel_touch_button_hold();
        if !point.x.is_finite() || !point.y.is_finite() {
            self.cancel_scroll_gesture();
            self.cancel_scroll_table_drag();
            return ScrollGestureEnd::default();
        }
        let scrolled = self.move_scroll_gesture(scene, layout, point, now);
        if self.scroll_table_drag_claimed() {
            let touch = self.touch_table_drag_claimed();
            let control = if self.scroll_table_selection_claimed() {
                self.finish_table_drag_selection(scene, layout)
            } else {
                self.finish_table_row_move(scene, layout)
            };
            if touch {
                self.pointer_leave();
            }
            return ScrollGestureEnd {
                control,
                ..ScrollGestureEnd::default()
            };
        }
        let Some(gesture) = self.scroll_gesture.take() else {
            return ScrollGestureEnd {
                scrolled,
                ..ScrollGestureEnd::default()
            };
        };
        let inside = !gesture.motion.dragging
            && gesture
                .tap
                .is_some_and(|hit| hit == self.hit_at(scene, layout, point));
        self.finish_button_contact(ButtonContactSource::Gesture, inside);
        let tap = inside && !gesture.delay_touch;
        match gesture.swipe {
            Some(SwipeRoute::Captured(target)) => {
                self.swipe_events.push(target.delivery(SwipePhase::Swiped));
                return ScrollGestureEnd::default();
            }
            Some(SwipeRoute::Cancelled) => return ScrollGestureEnd::default(),
            _ => {}
        }
        if gesture.delay_touch && !gesture.motion.dragging {
            self.cancel_touch_delay();
            return ScrollGestureEnd {
                tap: false,
                scrolled,
                control: None,
            };
        }
        if let Some(drag) = gesture.tab_move {
            let control = gesture
                .motion
                .dragging
                .then(|| drag.finish(scene, layout))
                .flatten();
            return ScrollGestureEnd {
                tap,
                scrolled: scrolled || gesture.motion.dragging,
                control,
            };
        }
        if let Some(mut drag) = gesture.tumbler {
            if !gesture.motion.dragging {
                return ScrollGestureEnd {
                    tap,
                    scrolled,
                    control: None,
                };
            }
            let target = drag.target.clone();
            if !self.reduce_motion {
                let velocity = gesture.motion.measured_release_velocity(now);
                match drag.settle(scene, layout, velocity, now) {
                    Ok(flick) => {
                        self.set_focus(Some(target));
                        self.tumbler_flick = Some(flick);
                        return ScrollGestureEnd {
                            scrolled: true,
                            ..ScrollGestureEnd::default()
                        };
                    }
                    Err(fallback) => drag = fallback,
                }
            }
            let control = drag.finish(scene, layout);
            if control.is_some() {
                self.set_focus(Some(target));
            }
            return ScrollGestureEnd {
                tap,
                scrolled: true,
                control,
            };
        }
        if let Some(drag) = gesture.switch {
            let control = drag.finish(scene, layout);
            if control.is_some() {
                self.set_focus(Some(drag.target));
            }
            return ScrollGestureEnd {
                tap,
                scrolled: scrolled || drag.position.is_some(),
                control,
            };
        }
        if let Some(swipe) = gesture.drawer {
            let control = gesture
                .motion
                .dragging
                .then(|| swipe.action(scene, layout, point))
                .flatten();
            return ScrollGestureEnd {
                tap,
                scrolled,
                control,
            };
        }
        if let Some(swipe) = gesture.swipe_delegate {
            let control = swipe.finish(scene, layout);
            if control.is_some() {
                self.set_focus(Some(swipe.target.clone()));
            }
            return ScrollGestureEnd {
                tap,
                scrolled: scrolled || swipe.has_preview(),
                control,
            };
        }
        if gesture.motion.dragging
            && let Some(page) = gesture.pages.into_iter().next()
        {
            let control = page.action(
                scene,
                layout,
                point,
                gesture.motion.measured_release_velocities(now),
            );
            if control.is_some()
                && let Some(motion) = &self.swipe_page_motion
            {
                self.swipe_commit = Some((motion.page.clone(), motion.fraction));
            }
            let cleared = self.settle_swipe_page(now);
            return ScrollGestureEnd {
                tap,
                scrolled: scrolled || cleared,
                control,
            };
        }
        let transformed = gesture.route.axes.iter().flatten().any(|target| {
            layout
                .at_path(&target.path)
                .is_some_and(|item| item.world_transform() != crate::Affine::IDENTITY)
        });
        let mut axes = if transformed {
            [true, true]
        } else {
            gesture.route.axes.each_ref().map(|axis| {
                axis.as_ref()
                    .is_some_and(|target| target.resolve(scene, layout).is_some())
            })
        };
        let elastic_axes = [Orientation::Horizontal, Orientation::Vertical]
            .map(|axis| self.overscroll.owns_axis(axis));
        if gesture.motion.dragging
            && gesture.route.pan_device == PanDevice::Touch
            && let Some(owner) = self.overscroll.armed_refresh()
        {
            self.refresh_request = Some(owner.clone());
        }
        if transformed && elastic_axes.contains(&true) {
            axes = [false; 2];
        } else {
            for (axis, elastic) in axes.iter_mut().zip(elastic_axes) {
                *axis &= !elastic;
            }
        }
        let rebounded = self.overscroll.return_to_bounds(now, None);
        self.scroll_fling = gesture.motion.finish(now, axes).map(|motion| ScrollFling {
            route: gesture.route,
            motion,
            transformed,
        });
        if self.scroll_fling.is_some() {
            if !transformed {
                self.viewport_motion.retain_axes(axes);
            }
            self.viewport_motion.set_phase(ViewportMotion::Flicking);
        } else {
            self.viewport_motion.clear();
        }
        ScrollGestureEnd {
            tap,
            scrolled: scrolled || rebounded,
            control: None,
        }
    }

    pub fn advance_scroll_motion(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let rebounded = self.overscroll.advance(now);
        let tab_scroll = self.advance_tab_drag_scroll(scene, layout, now);
        let swipe_scroll = self
            .swipe_page_motion
            .as_mut()
            .is_some_and(|motion| motion.advance(now));
        if swipe_scroll
            && self
                .swipe_page_motion
                .as_ref()
                .is_some_and(|motion| motion.wake.is_none())
        {
            self.swipe_page_motion = None;
            self.swipe_commit = None;
        }
        self.advance_flick(scene, layout, now) || rebounded || tab_scroll || swipe_scroll
    }

    fn advance_tab_drag_scroll(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) -> bool {
        let Some(drag) = self
            .scroll_gesture
            .as_mut()
            .and_then(|gesture| gesture.tab_move.as_mut())
        else {
            return false;
        };
        if !drag.wake.is_some_and(|deadline| deadline <= now) {
            return false;
        }
        let Some((axis, offset)) = drag.scroll_target(scene, layout) else {
            drag.wake = None;
            return false;
        };
        drag.wake = Some(now + SCROLL_FRAME_INTERVAL);
        let offsets = match axis {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        offsets.insert(drag.path().to_vec(), offset);
        true
    }

    pub fn tab_move_active(&self) -> bool {
        self.scroll_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.motion.dragging && gesture.tab_move.is_some())
    }

    pub fn tab_move_marker(&self, bar: &Node, item: &LayoutNode, path: &[usize]) -> Option<Rect> {
        let gesture = self.scroll_gesture.as_ref()?;
        let drag = gesture.tab_move.as_ref()?;
        (gesture.motion.dragging && drag.path() == path).then_some(())?;
        drag.marker(bar, item)
    }

    fn advance_flick(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) -> bool {
        let Some(mut fling) = self.scroll_fling.take() else {
            return false;
        };
        if !self.scroll_route_is_current(scene, layout, &fling.route)
            || self.menu_open()
            || self.choice_open()
        {
            self.viewport_motion.clear();
            return false;
        }
        let Some(delta) = fling.motion.step(now) else {
            self.viewport_motion.clear();
            return false;
        };
        if delta == Point::default() {
            self.scroll_fling = Some(fling);
            return false;
        }
        let (moved, mut owners) = self.scroll_captured(scene, layout, &fling.route, delta, true);
        self.overscroll.return_to_bounds(now, None);
        let continuing = moved.map(|axis| axis.bounded && !axis.elastic);
        let axes = if fling.transformed {
            [continuing.contains(&true) && !moved.iter().any(|axis| axis.elastic); 2]
        } else {
            continuing
        };
        if fling.motion.finish_step(axes) {
            self.scroll_fling = Some(fling);
            owners.retain_mut(|(_, owner_axes)| {
                for (owned, active) in owner_axes.iter_mut().zip(continuing) {
                    *owned &= active;
                }
                owner_axes.contains(&true)
            });
            self.viewport_motion
                .update(owners, ViewportMotion::Flicking);
        } else {
            self.viewport_motion.clear();
        }
        moved.into_iter().any(AxisMotion::changed)
    }

    fn interrupt_tumbler_flick(&mut self) -> bool {
        self.tumbler_flick
            .as_mut()
            .is_some_and(TumblerFlick::interrupt)
    }

    fn tumbler_flick_is_current(
        &self,
        flick: &TumblerFlick,
        scene: &Node,
        layout: &LayoutNode,
    ) -> bool {
        !self.menu_open()
            && !self.choice_open()
            && self
                .active_dialog(layout)
                .is_none_or(|scope| flick.drag.target.path.starts_with(scope))
            && flick.drag.is_current(scene, layout)
    }

    fn time_picker_flick(
        &self,
        flick: &TumblerFlick,
        scene: &Node,
        layout: &LayoutNode,
        dialog: &[usize],
    ) -> bool {
        flick.active()
            && self.active_dialog(layout) == Some(dialog)
            && flick.drag.target.path.starts_with(dialog)
            && scene
                .at_path(dialog)
                .and_then(Node::picker_session_id)
                .is_some_and(|session| flick.drag.picker_session == Some(session))
            && self.tumbler_flick_is_current(flick, scene, layout)
    }

    /// Commit the visible row before the host confirms the same picker session.
    #[doc(hidden)]
    pub fn settle_time_picker_wheel(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        dialog: &[usize],
    ) -> Option<ControlInput> {
        if !self
            .tumbler_flick
            .as_ref()
            .is_some_and(|flick| self.time_picker_flick(flick, scene, layout, dialog))
        {
            return None;
        }
        self.tumbler_flick.take()?.drag.finish(scene, layout)
    }

    pub fn advance_tumbler_motion(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> ScrollGestureEnd {
        let Some(mut flick) = self.tumbler_flick.take() else {
            return ScrollGestureEnd::default();
        };
        if !flick.active() || !self.tumbler_flick_is_current(&flick, scene, layout) {
            flick.interrupt();
            return ScrollGestureEnd {
                scrolled: true,
                ..ScrollGestureEnd::default()
            };
        }
        if now < flick.wake && !self.reduce_motion {
            self.tumbler_flick = Some(flick);
            return ScrollGestureEnd::default();
        }
        // A preference change settles the last displayed offset immediately.
        let control = if self.reduce_motion || flick.advance(now) {
            flick.drag.finish(scene, layout)
        } else {
            self.tumbler_flick = Some(flick);
            None
        };
        ScrollGestureEnd {
            scrolled: true,
            control,
            tap: false,
        }
    }

    pub fn scroll_motion_active(&self) -> bool {
        self.scroll_fling.is_some()
            || self.overscroll.deadline().is_some()
            || self
                .swipe_page_motion
                .as_ref()
                .is_some_and(|motion| motion.wake.is_some())
            || self
                .tumbler_flick
                .as_ref()
                .is_some_and(TumblerFlick::active)
    }

    pub fn scroll_motion_deadline(&self) -> Option<Instant> {
        self.scroll_fling
            .as_ref()
            .map(|fling| fling.motion.deadline())
            .into_iter()
            .chain(self.overscroll.deadline())
            .chain(
                self.swipe_page_motion
                    .as_ref()
                    .and_then(|motion| motion.wake),
            )
            .chain(self.tumbler_flick.as_ref().map(|flick| flick.wake))
            .chain(
                self.scroll_gesture
                    .as_ref()
                    .and_then(|gesture| gesture.tab_move.as_ref()?.wake),
            )
            .min()
    }

    fn tumbler_drag(&self) -> Option<&TumblerDrag> {
        self.scroll_gesture
            .as_ref()
            .and_then(|gesture| gesture.tumbler.as_ref())
            .or_else(|| {
                self.tumbler_flick
                    .as_ref()
                    .filter(|flick| flick.active())
                    .map(|flick| &flick.drag)
            })
    }

    pub fn tumbler_preview(&self) -> Option<(&[usize], f32)> {
        let drag = self.tumbler_drag()?;
        (drag.offset != 0.0).then_some((&drag.target.path, drag.offset))
    }

    #[doc(hidden)]
    pub fn switch_preview(&self) -> Option<(&[usize], Option<RowKey>, f32)> {
        let drag = self.scroll_gesture.as_ref()?.switch.as_ref()?;
        Some((&drag.target.path, drag.target.row_key, drag.position?))
    }

    pub fn has_switch_preview(&self) -> bool {
        self.switch_preview().is_some()
    }

    pub(crate) fn contains_switch_preview(&self, path: &[usize]) -> bool {
        self.switch_preview()
            .is_some_and(|(target, _, _)| target.starts_with(path))
    }

    pub(crate) fn switch_position(
        &self,
        node: &Node,
        bounds: Rect,
        direction: LayoutDirection,
        path: &[usize],
        row: Option<RowKey>,
    ) -> Option<f32> {
        let drag = self.scroll_gesture.as_ref()?.switch.as_ref()?;
        drag.position
            .filter(|_| drag.target.path == path && drag.matches(node, bounds, direction, row))
    }

    pub fn has_swipe_preview(&self) -> bool {
        self.swipe_preview().is_some()
    }

    pub fn swipe_preview(&self) -> Option<(&[usize], Option<RowKey>, f32)> {
        self.scroll_gesture
            .as_ref()
            .and_then(|gesture| gesture.swipe_delegate.as_ref())
            .and_then(SwipeDrag::preview_state)
    }

    pub(crate) fn swipe_preview_offset(
        &self,
        path: &[usize],
        node: &Node,
        geometry: &crate::SwipeDelegateGeometry,
        row: Option<RowKey>,
    ) -> Option<f32> {
        self.scroll_gesture
            .as_ref()?
            .swipe_delegate
            .as_ref()?
            .preview(path, node, geometry, row)
    }

    pub(crate) fn tumbler_offset(&self, wheel: &Tumbler<'_>, path: &[usize]) -> f32 {
        self.tumbler_drag()
            .map_or(0.0, |drag| drag.offset_for(wheel, path))
    }

    pub fn tumbler_scroll(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        delta: f32,
        lines: bool,
    ) -> TumblerScroll {
        let target = self.pointer_hover_target(scene, layout);
        if let Some(target) = target
            && let Some(node) = scene
                .at_path(&target.path)
                .filter(|node| matches!(node.kind, "Tumbler" | "PathView"))
            && let Some(wheel) = layout
                .at_path(&target.path)
                .and_then(|item| item.tumbler.as_ref())
        {
            if delta.is_finite() && delta != 0.0 {
                self.interrupt_tumbler_flick();
            }
            return crate::tumbler::scroll(
                &mut self.tumbler_wheel,
                &target,
                &Tumbler::from_node(node),
                wheel.step_size(),
                delta,
                lines,
            );
        }
        self.tumbler_wheel = None;
        TumblerScroll::Unhandled
    }

    pub fn reset_wheel_adjustment(&mut self) {
        self.wheel_steps = None;
    }

    pub fn wheel_adjustment(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        event: xrack_runtime::WheelEvent,
        now: Instant,
    ) -> Option<WheelAdjustment> {
        let previous = self.wheel_steps.take();
        let modifiers = event.modifiers;
        if modifiers.shift
            || modifiers.control
            || modifiers.alt
            || modifiers.meta
            || self.menu_open()
            || self.choice_open()
            || self.pressed.is_some()
            || !event.delta_x.0.is_finite()
            || !event.delta_y.0.is_finite()
        {
            return None;
        }
        let point = self.pointer?;
        let target = self.pointer_hover_target(scene, layout)?;
        let (path, node) = slider_target(&target, scene, layout).or_else(|| {
            let node = scene.interactive_at_path(&target.path)?;
            valid(&target, scene, layout).then_some((target.path.as_slice(), node))
        })?;
        if !node.boolean("wheel-enabled", false) {
            return None;
        }
        let horizontal = event.delta_x.0.abs() > event.delta_y.0.abs();
        let units = match (horizontal, event.is_line) {
            (true, true) => event.lines_x,
            (false, true) => -event.lines_y,
            (true, false) => event.delta_x.0 / 28.0,
            (false, false) => -event.delta_y.0 / 28.0,
        };
        if !units.is_finite() || units == 0.0 {
            return None;
        }
        let interval = (node.kind == "RangeSlider").then(|| SliderInterval::from_node(node));
        let handle = if let Some(interval) = interval {
            if target.path.len() > path.len() {
                Some(*target.path.last()?)
            } else {
                let item = layout.at_path(path)?;
                let geometry =
                    SliderGeometry::with_layout_direction(item.bounds, node, item.layout_direction);
                let point = item.map_from_window(point)?;
                Some(geometry.interval_handle_at(point, geometry.fraction_at(point)?, interval))
            }
        } else {
            None
        };
        let action_for = |steps| {
            if is_spin_box(node.kind) {
                let action = self.resolve_spin_action(scene, path, SpinAction::Step(steps))?;
                if node.kind == "DoubleSpinBox" {
                    xrack_runtime::decimal_spin_value_after(node, action)?;
                } else {
                    xrack_runtime::spin_value_after(node, action)?;
                }
                return Some(WheelAdjustment::Spin(ControlInput {
                    path: path.to_vec(),
                    action: ControlAction::Spin(action),
                }));
            }
            let action = RangeAction::Step(steps);
            let mut path = path.to_vec();
            if let Some((interval, handle)) = interval.zip(handle) {
                interval.apply(handle, action)?;
                path.push(handle);
            } else if matches!(node.kind, "Slider" | "Dial") {
                SliderRange::from_node(node).apply(action)?;
            } else {
                return None;
            }
            Some(WheelAdjustment::Range(RangeInput { path, action }))
        };
        // At a boundary the viewport should receive even a sub-step delta.
        action_for(units.signum() as i64)?;
        let row = scene.row_key_at(path);
        let mut total = units;
        if let Some(previous) = previous
            && previous.path == path
            && previous.row == row
            && previous.handle == handle
            && previous.horizontal == horizontal
            && previous.is_line == event.is_line
            && previous.remainder.signum() == units.signum()
            && now.saturating_duration_since(previous.time) <= Duration::from_millis(300)
        {
            total += previous.remainder;
        }
        let steps = total.trunc() as i64;
        let adjustment = if steps == 0 {
            WheelAdjustment::Pending
        } else {
            action_for(steps).unwrap_or(WheelAdjustment::Pending)
        };
        let remainder = total.fract();
        self.wheel_steps = (remainder != 0.0).then(|| WheelSteps {
            path: path.to_vec(),
            row,
            handle,
            horizontal,
            is_line: event.is_line,
            remainder,
            time: now,
        });
        Some(adjustment)
    }

    pub fn tumbler_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        if !valid(target, scene, layout) {
            return None;
        }
        let node = scene
            .at_path(&target.path)
            .filter(|node| matches!(node.kind, "Tumbler" | "PathView"))?;
        let wheel = Tumbler::from_node(node);
        let (axis, reversed) = layout
            .at_path(&target.path)?
            .tumbler
            .as_ref()?
            .navigation_axis();
        let index = match key {
            NavigationKey::Left if axis == Orientation::Horizontal => {
                wheel.step(if reversed { 1 } else { -1 })?
            }
            NavigationKey::Right if axis == Orientation::Horizontal => {
                wheel.step(if reversed { -1 } else { 1 })?
            }
            NavigationKey::Up => wheel.step(-1)?,
            NavigationKey::Down => wheel.step(1)?,
            NavigationKey::Home => 0,
            NavigationKey::End => wheel.count().checked_sub(1)?,
            NavigationKey::PageUp => wheel.step(-(wheel.visible() as i64))?,
            NavigationKey::PageDown => wheel.step(wheel.visible() as i64)?,
            _ => return None,
        };
        let input = ControlInput {
            path: target.path.clone(),
            action: wheel.select(index)?,
        };
        self.interrupt_tumbler_flick();
        Some(input)
    }

    #[doc(hidden)]
    pub fn set_reduce_motion(&mut self, reduced: bool) {
        self.reduce_motion = reduced;
        if reduced
            && self
                .swipe_page_motion
                .as_ref()
                .is_some_and(|motion| motion.settling.is_some())
        {
            self.swipe_page_motion = None;
            self.swipe_commit = None;
        }
    }

    /// Whether the host has requested reduced motion.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// The pull distance as a fraction of the refresh threshold.
    pub fn pull_to_refresh_progress(&self, node: &Node) -> Option<f32> {
        self.overscroll
            .refresh_progress(node.viewport_handle.as_ref()?)
    }

    fn settle_swipe_page(&mut self, now: Instant) -> bool {
        if self.reduce_motion {
            self.swipe_commit = None;
            return self.swipe_page_motion.take().is_some();
        }
        if let Some(motion) = &mut self.swipe_page_motion {
            motion.settle(now);
        }
        false
    }

    pub fn cancel_scroll_gesture(&mut self) {
        self.cancel_scroll_gesture_with_wheel(false);
    }

    /// End pointer capture while navigating or confirming the active picker.
    #[doc(hidden)]
    pub fn cancel_scroll_gesture_for_picker_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        confirming: bool,
    ) {
        let keep_wheel = self
            .tumbler_flick
            .as_ref()
            .zip(self.active_dialog(layout))
            .is_some_and(|(flick, dialog)| {
                self.time_picker_flick(flick, scene, layout, dialog)
                    && (!confirming
                        || self.focused.as_ref().is_some_and(|target| {
                            self.dialog_button(scene, layout, target)
                                .is_some_and(|input| {
                                    input.action == ControlAction::Dialog(DialogRole::Accept)
                                })
                        }))
            });
        self.cancel_scroll_gesture_with_wheel(keep_wheel);
    }

    fn cancel_scroll_gesture_with_wheel(&mut self, keep_wheel: bool) {
        self.scroll_keyboard_dismissal = false;
        self.settle_swipe_page(Instant::now());
        self.finish_button_contact(ButtonContactSource::Gesture, false);
        if let Some(drag) = self
            .scroll_gesture
            .as_mut()
            .and_then(|gesture| gesture.tumbler.as_mut())
        {
            drag.cancel();
        }
        if !keep_wheel && let Some(mut flick) = self.tumbler_flick.take() {
            flick.interrupt();
        }
        self.swipe_events.extend(
            self.scroll_gesture
                .take()
                .and_then(|gesture| gesture.swipe)
                .and_then(SwipeRoute::cancel),
        );
        self.scroll_fling = None;
        self.viewport_motion.clear();
        self.overscroll.clear();
        self.cancel_touch_delay();
        self.cancel_touch_button_hold();
    }

    pub fn overscroll_frame(&self) -> crate::OverscrollFrame {
        self.overscroll.frame()
    }

    pub fn has_overscroll(&self) -> bool {
        self.overscroll.has_offset()
    }

    pub(crate) fn visual_overscroll(&self, handle: &ViewportHandle, axis: Orientation) -> f32 {
        self.overscroll.visual_offset(handle, axis)
    }

    pub(crate) fn return_viewport_to_bounds(&mut self, target: &ViewportHandle) -> bool {
        self.overscroll
            .return_to_bounds(Instant::now(), Some(target))
    }

    pub(crate) fn cancel_viewport_flick(&mut self, target: &ViewportHandle) -> bool {
        if self.viewport_motion.contains(target) && self.scroll_fling.take().is_some() {
            self.viewport_motion.clear();
            true
        } else {
            false
        }
    }

    pub fn has_pending_swipe_events(&self) -> bool {
        !self.swipe_events.is_empty() || self.scroll_gesture.as_ref().is_some_and(|gesture|
            matches!(&gesture.swipe, Some(SwipeRoute::Captured(target)) if target.cancel_requested()))
    }

    pub fn reconcile_swipe(&mut self, scene: &Node, layout: &LayoutNode) {
        let invalid = self.scroll_gesture.as_ref().is_some_and(|gesture| matches!(&gesture.swipe,
            Some(SwipeRoute::Captured(target)) if target.cancel_requested() || !target.is_live(scene, layout, self)));
        if invalid && let Some(gesture) = &mut self.scroll_gesture {
            self.swipe_events.extend(
                gesture
                    .swipe
                    .replace(SwipeRoute::Cancelled)
                    .and_then(SwipeRoute::cancel),
            );
        }
    }

    pub fn take_swipe_events(&mut self) -> Vec<SwipeDelivery> {
        std::mem::take(&mut self.swipe_events)
    }

    fn begin_delay(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        source: DelaySource,
        now: Instant,
    ) -> bool {
        let scope = self.active_dialog(layout).unwrap_or_default();
        let Some(node) = scene.at_path(&target.path).filter(|node| {
            node.kind == "DelayButton"
                && !node.boolean("checked", false)
                && target.path.starts_with(scope)
                && valid(&target, scene, layout)
        }) else {
            return false;
        };
        if self.menu_open() || self.choice_open() {
            return false;
        }
        self.delay_hold = Some(DelayHold::new(
            target,
            source,
            scope.to_vec(),
            Duration::from_millis(delay_millis(node)),
            now,
        ));
        true
    }

    pub fn begin_pointer_delay(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) {
        if let Some(target) = self.pressed.clone() {
            self.pointer_activation_claimed =
                self.begin_delay(scene, layout, target, DelaySource::Pointer, now);
        }
    }

    pub fn begin_key_delay(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: ActivationKey,
        now: Instant,
    ) {
        if let Some((pressed, target)) = self.key_pressed.clone()
            && pressed == key
            && self.begin_delay(scene, layout, target, DelaySource::Key(key), now)
        {
            self.key_activation_claimed = Some(key);
        }
    }

    fn begin_button_hold(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        source: DelaySource,
        now: Instant,
    ) -> bool {
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !target.path.starts_with(scope)
            || !valid(&target, scene, layout)
            || self.menu_open()
            || self.choice_open()
        {
            return false;
        }
        let Some(timing) = target.node_in(scene).and_then(ButtonHoldTiming::from_node) else {
            return false;
        };
        self.button_hold = Some(ButtonHold::new(
            target,
            source,
            scope.to_vec(),
            timing,
            match source {
                DelaySource::Pointer => self.pointer,
                DelaySource::Touch(point) => Some(point),
                _ => None,
            },
            now,
        ));
        true
    }

    pub fn begin_pointer_button_hold(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        self.pressed.clone().is_some_and(|target| {
            self.begin_button_hold(scene, layout, target, DelaySource::Pointer, now)
        })
    }

    pub fn begin_key_button_hold(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: ActivationKey,
        now: Instant,
    ) {
        if let Some((pressed, target)) = self.key_pressed.clone()
            && pressed == key
        {
            self.begin_button_hold(scene, layout, target, DelaySource::Key(key), now);
        }
    }

    pub fn button_hold_deadline(&self) -> Option<Instant> {
        self.button_hold.as_ref().and_then(|hold| hold.deadline)
    }

    pub fn advance_button_hold(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> Option<(Target, ButtonHoldEvent)> {
        let mut repeat = self.button_hold.take()?;
        let pointer_matches =
            |point| self.target_at(scene, layout, point).as_ref() == Some(&repeat.target);
        let pressed = match repeat.source {
            DelaySource::Pointer => {
                self.pressed.as_ref() == Some(&repeat.target)
                    && self.pointer.is_some_and(pointer_matches)
            }
            DelaySource::Key(key) => {
                self.focused.as_ref() == Some(&repeat.target)
                    && self
                        .key_pressed
                        .as_ref()
                        .is_some_and(|(held, target)| *held == key && target == &repeat.target)
            }
            DelaySource::Touch(point) => {
                self.scroll_gesture
                    .as_ref()
                    .is_some_and(|gesture| !gesture.motion.dragging)
                    && pointer_matches(point)
            }
            DelaySource::Accessibility => false,
        };
        if !pressed
            || self.menu_open()
            || self.choice_open()
            || self.active_dialog(layout).unwrap_or_default() != repeat.scope
            || !valid(&repeat.target, scene, layout)
            || repeat
                .target
                .node_in(scene)
                .and_then(ButtonHoldTiming::from_node)
                != Some(repeat.timing)
        {
            return None;
        }
        let event = repeat.sample(now);
        if event.is_some() {
            match repeat.source {
                DelaySource::Pointer => self.pointer_activation_claimed = true,
                DelaySource::Key(key) => self.key_activation_claimed = Some(key),
                DelaySource::Touch(_) => {
                    if let Some(gesture) = &mut self.scroll_gesture {
                        gesture.delay_touch = true;
                    }
                    if repeat.target.focusable
                        && repeat
                            .target
                            .node_in(scene)
                            .is_some_and(|node| node.boolean("focus-on-click", true))
                    {
                        self.set_focus_with_reason(
                            Some(repeat.target.clone()),
                            xrack_runtime::FocusReason::PointerClick,
                        );
                    }
                }
                DelaySource::Accessibility => {}
            }
        }
        let delivery = event.map(|event| (repeat.target.clone(), event));
        self.button_hold = Some(repeat);
        delivery
    }

    fn cancel_touch_button_hold(&mut self) {
        if self
            .button_hold
            .as_ref()
            .is_some_and(|repeat| matches!(repeat.source, DelaySource::Touch(_)))
        {
            self.button_hold = None;
        }
    }

    fn delay_current(&self, hold: &DelayHold, scene: &Node, layout: &LayoutNode) -> bool {
        if self.focused.as_ref() != Some(&hold.target)
            || self.menu_open()
            || self.choice_open()
            || self.active_dialog(layout).unwrap_or_default() != hold.scope
            || !valid(&hold.target, scene, layout)
            || !scene.at_path(&hold.target.path).is_some_and(|node| {
                node.kind == "DelayButton"
                    && !node.boolean("checked", false)
                    && Duration::from_millis(delay_millis(node)) == hold.duration
            })
        {
            return false;
        }
        match hold.source {
            DelaySource::Pointer => {
                self.pressed.as_ref() == Some(&hold.target)
                    && self.pointer.is_some_and(|point| {
                        self.target_at(scene, layout, point).as_ref() == Some(&hold.target)
                    })
            }
            DelaySource::Key(key) => self
                .key_pressed
                .as_ref()
                .is_some_and(|(current, target)| *current == key && target == &hold.target),
            DelaySource::Touch(point) => {
                self.target_at(scene, layout, point).as_ref() == Some(&hold.target)
            }
            DelaySource::Accessibility => true,
        }
    }

    pub fn advance_delay(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> Option<ControlInput> {
        let mut hold = self.delay_hold.take()?;
        if !self.delay_current(&hold, scene, layout) {
            return None;
        }
        if hold.sample(now) && !matches!(hold.source, DelaySource::Accessibility) {
            return Some(ControlInput {
                path: hold.target.path,
                action: ControlAction::DelayButton(true),
            });
        }
        self.delay_hold = Some(hold);
        None
    }

    pub fn accessibility_delay_click(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        now: Instant,
    ) -> Option<ControlInput> {
        let node = scene
            .at_path(&target.path)
            .filter(|node| node.kind == "DelayButton")?;
        if !valid(&target, scene, layout)
            || self.menu_open()
            || self.choice_open()
            || !target
                .path
                .starts_with(self.active_dialog(layout).unwrap_or_default())
        {
            return None;
        }
        self.accessibility_focus(target.clone());
        if node.boolean("checked", false) {
            self.cancel_delay();
            return Some(ControlInput {
                path: target.path,
                action: ControlAction::DelayButton(false),
            });
        }
        if let Some(hold) = self.delay_hold.take()
            && matches!(hold.source, DelaySource::Accessibility)
            && hold.target == target
            && self.delay_current(&hold, scene, layout)
        {
            return (now >= hold.deadline()).then_some(ControlInput {
                path: target.path,
                action: ControlAction::DelayButton(true),
            });
        }
        self.begin_delay(scene, layout, target, DelaySource::Accessibility, now);
        None
    }

    pub fn cancel_delay(&mut self) -> bool {
        self.delay_hold.take().is_some()
    }

    fn cancel_touch_delay(&mut self) {
        if self
            .delay_hold
            .as_ref()
            .is_some_and(|hold| matches!(hold.source, DelaySource::Touch(_)))
        {
            self.cancel_delay();
        }
    }

    pub fn delay_deadline(&self) -> Option<Instant> {
        self.delay_hold
            .as_ref()
            .filter(|hold| hold.pending())
            .map(DelayHold::deadline)
    }

    pub fn delay_progress(&self, path: &[usize]) -> Option<(f32, bool)> {
        let hold = self
            .delay_hold
            .as_ref()
            .filter(|hold| hold.target.path == path)?;
        Some((hold.fraction, hold.pending()))
    }

    pub fn delay_accessibility(&self) -> Option<(&Target, bool)> {
        let hold = self
            .delay_hold
            .as_ref()
            .filter(|hold| matches!(hold.source, DelaySource::Accessibility))?;
        Some((&hold.target, !hold.pending()))
    }

    pub fn editor_value<'a>(&'a self, node: &'a Node) -> Cow<'a, str> {
        if is_spin_box(node.kind) {
            match &self.spin_draft {
                Some(draft) => Cow::Borrowed(&draft.text),
                None => Cow::Owned(spin_text(node)),
            }
        } else if matches!(node.kind, "TableCell" | "TreeCell")
            && let Some(edit) = &self.table_edit
        {
            Cow::Borrowed(&edit.draft)
        } else {
            Cow::Borrowed(text_input_value(node))
        }
    }

    pub fn table_cell_editing(&self, path: &[usize]) -> bool {
        self.table_edit
            .as_ref()
            .is_some_and(|edit| edit.target.path == path)
    }

    pub fn table_cell_edit_invalid(&self, node: &Node, path: &[usize]) -> bool {
        self.table_edit.as_ref().is_some_and(|edit| {
            edit.target.path == path
                && node
                    .table_cell_editor
                    .is_some_and(|editor| !editor.accepts(&edit.draft))
        })
    }

    pub fn is_text_editor(&self, node: &Node, target: &Target) -> bool {
        is_text_input(node.kind)
            || combo_editable(node)
            || node.selectable_styled_text()
            || editable_table_cell(node)
                && self
                    .table_edit
                    .as_ref()
                    .is_some_and(|edit| &edit.target == target)
    }

    pub fn table_cell_click(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Option<&Target>,
        now: Instant,
    ) -> bool {
        let previous = self.last_table_click.take();
        let Some(target) =
            target.filter(|target| scene.at_path(&target.path).is_some_and(editable_table_cell))
        else {
            return false;
        };
        let double_click = previous.is_some_and(|(prior, when)| {
            prior == *target && now.saturating_duration_since(when) <= Duration::from_millis(450)
        });
        let table = table_cell_row(scene, &target.path)
            .and_then(|(path, slot)| scene.at_path(path).map(|table| (table, slot)));
        let double_enabled =
            table.is_none_or(|(table, _)| table.boolean("edit-on-double-click", true));
        let start = double_enabled && double_click
            || table.is_some_and(|(table, slot)| {
                table.boolean("edit-on-click", false)
                    || table.boolean("edit-on-selected-click", false)
                        && table
                            .children
                            .get(slot)
                            .and_then(|row| row.row_key)
                            .is_some_and(|key| {
                                let column = target.path.last().copied().unwrap_or_default();
                                table.table_selection.as_ref().map_or_else(
                                    || current_list_slot(table) == Some(slot),
                                    |selection| selection.contains_cell(key, column),
                                )
                            })
            });
        if start {
            return self.begin_cell_edit(scene, layout, target.clone(), self.focus_reason);
        }
        if double_enabled {
            self.last_table_click = Some((target.clone(), now));
        }
        false
    }

    pub fn begin_table_edit(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        self.begin_focused_cell_edit(scene, layout, "edit-on-key", true)
    }

    /// Start editing the addressed accessible cell using its current row identity.
    pub fn begin_accessible_table_edit(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
    ) -> bool {
        self.table_edit.is_none()
            && self
                .accessibility_target(scene, layout, &target.path)
                .as_ref()
                == Some(&target)
            && self.begin_cell_edit(
                scene,
                layout,
                target,
                xrack_runtime::FocusReason::Accessibility,
            )
    }

    pub fn begin_table_typing(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        self.begin_focused_cell_edit(scene, layout, "edit-on-typing", false)
    }

    fn begin_focused_cell_edit(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        property: &str,
        default: bool,
    ) -> bool {
        let Some(target) = self.focused_edit_target(scene, layout) else {
            return false;
        };
        let enabled = table_cell_row(scene, &target.path)
            .and_then(|(path, _)| scene.at_path(path))
            .map_or(default, |table| table.boolean(property, default));
        enabled
            && self.begin_cell_edit(
                scene,
                layout,
                target,
                xrack_runtime::FocusReason::KeyboardNavigation,
            )
    }

    fn focused_edit_target(&self, scene: &Node, layout: &LayoutNode) -> Option<Target> {
        let focused = self.focused.as_ref()?;
        let node = scene.interactive_at_path(&focused.path)?;
        let mut target = focused.clone();
        if matches!(node.kind, "TableView" | "TreeView") {
            let slot = current_list_slot(node)?;
            let row = node.children.get(slot)?.children.first()?;
            target.path.extend([slot, 0]);
            let cell = if node.kind == "TreeView" {
                layout
                    .at_path(&target.path)
                    .and_then(|item| first_editable_tree_cell(row, item, &mut target.path))
            } else {
                let current_column = node
                    .table_selection
                    .as_ref()
                    .and_then(|view| view.current_column());
                let column_index = target.path.len();
                target.path.push(0);
                row.materialized_children().find_map(|(column, cell)| {
                    target.path[column_index] = column;
                    (current_column.is_none_or(|current| current == column)
                        && editable_table_cell(cell)
                        && layout
                            .at_path(&target.path)
                            .is_some_and(|item| eligible(cell, item)))
                    .then_some(cell)
                })
            };
            let cell = cell?;
            target.set_element_id(ElementId::of(cell));
            target.row_key = scene.row_key_at(&target.path);
        }
        Some(target)
    }

    fn begin_cell_edit(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        reason: xrack_runtime::FocusReason,
    ) -> bool {
        if self
            .table_edit
            .as_ref()
            .is_some_and(|edit| edit.target == target)
        {
            return false;
        }
        let Some(cell) = scene.interactive_at_path(&target.path).filter(|cell| {
            target.row_key.is_some() && editable_table_cell(cell) && valid(&target, scene, layout)
        }) else {
            return false;
        };
        let original = cell.text("text").unwrap_or("").to_owned();
        self.set_focus_with_reason(Some(target.clone()), reason);
        let mut editor = TextEdit::at_end(&original);
        editor.apply(&original, EditAction::SelectAll);
        self.editor = Some(editor);
        self.table_edit = Some(TableEdit {
            target,
            draft: original.clone(),
            original,
            history: TextHistory::default(),
        });
        true
    }

    pub fn cancel_table_edit(&mut self) -> bool {
        if self.table_edit.take().is_none() {
            return false;
        }
        self.editor = None;
        self.last_table_click = None;
        true
    }

    pub fn commit_table_edit(&mut self, component: &mut impl Component, scene: &Node) -> bool {
        self.finish_table_edit(component, scene) == TableEditCommit::Changed
    }

    pub fn finish_table_edit(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
    ) -> TableEditCommit {
        let Some(edit) = self.table_edit.as_ref() else {
            return TableEditCommit::Rejected;
        };
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return TableEditCommit::Kept;
        }
        let current = scene
            .table_cell_commit_target(&edit.target.path, &edit.original)
            .filter(|_| scene.row_key_at(&edit.target.path) == edit.target.row_key);
        let Some(cell) = current else {
            self.cancel_table_edit();
            return TableEditCommit::Rejected;
        };
        let mut unchanged = edit.draft == edit.original;
        if !unchanged
            && let Some(editor) = cell.table_cell_editor
            && editor != xrack_runtime::TableCellEditor::String
        {
            let Some(draft) = editor.parse(&edit.draft) else {
                return TableEditCommit::Kept;
            };
            unchanged = editor.parse(&edit.original) == Some(draft);
        }
        let Some(edit) = self.table_edit.take() else {
            return TableEditCommit::Rejected;
        };
        self.editor = None;
        self.last_table_click = None;
        if unchanged {
            TableEditCommit::Unchanged
        } else if component.commit_table_cell(scene, &edit.target.path, &edit.original, edit.draft)
        {
            TableEditCommit::Changed
        } else {
            TableEditCommit::Rejected
        }
    }

    pub fn tab_edit_origin(&self, scene: &Node) -> Option<TableEditTarget> {
        let edit = self.table_edit.as_ref()?;
        let (table, slot) = table_cell_row(scene, &edit.target.path)?;
        let view = scene.interactive_at_path(table)?;
        if !view.boolean("edit-on-tab", false) || self.focused.as_ref() != Some(&edit.target) {
            return None;
        }
        Some(TableEditTarget {
            table: table.to_vec(),
            viewport: view.viewport_handle.clone()?,
            position: TableEditPosition {
                row: view.list?.first_row.checked_add(slot)?,
                column: *edit.target.path.last()?,
                key: view.children.get(slot)?.row_key?,
            },
        })
    }

    pub fn next_table_edit_target(
        &self,
        component: &impl Component,
        scene: &Node,
        layout: &LayoutNode,
        origin: &TableEditTarget,
        backward: bool,
    ) -> Option<TableEditTarget> {
        let view = scene.interactive_at_path(&origin.table)?;
        if view.viewport_handle.as_ref() != Some(&origin.viewport)
            || !self.allows_popup(layout, &origin.table)
        {
            return None;
        }
        let geometry = layout.at_path(&origin.table)?.table.as_ref()?;
        let columns: Vec<_> = geometry
            .visual_columns()
            .filter(|column| geometry.columns[*column].width > 0.0)
            .collect();
        let position = component.next_table_edit_cell(
            scene,
            &origin.table,
            origin.position,
            &columns,
            backward,
        )?;
        Some(TableEditTarget {
            table: origin.table.clone(),
            viewport: origin.viewport.clone(),
            position,
        })
    }

    pub fn begin_table_edit_target(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &TableEditTarget,
    ) -> bool {
        let Some(view) = scene.interactive_at_path(&target.table) else {
            return false;
        };
        if view.viewport_handle.as_ref() != Some(&target.viewport)
            || !view.boolean("edit-on-tab", false)
            || !self.allows_popup(layout, &target.table)
        {
            return false;
        }
        let position = target.position;
        if view
            .list
            .and_then(|list| position.row.checked_sub(list.first_row))
            .and_then(|slot| view.children.get(slot))
            .and_then(|row| row.row_key)
            != Some(position.key)
        {
            return false;
        }
        let cell = table_cell_target(scene, layout, &target.table, position.row, position.column);
        cell.is_some_and(|cell| {
            self.begin_cell_edit(
                scene,
                layout,
                cell,
                xrack_runtime::FocusReason::KeyboardNavigation,
            )
        })
    }

    pub fn sync_editor(&mut self, scene: &Node) {
        let target_node = self
            .focused
            .as_ref()
            .and_then(|target| Some((target, target.node_in(scene)?)));
        if let Some((target, node)) = target_node
            .filter(|(target, node)| self.is_text_editor(node, target) || spin_editable(node))
        {
            let persistent = node.boolean("persistent-selection", false)
                && (is_text_input(node.kind)
                    || combo_editable(node)
                    || node.selectable_styled_text());
            if persistent {
                if let Some(source) = &mut self.persistent_editor {
                    if !source.matches(node) {
                        self.editor = None;
                        if let Some(handle) = &source.handle {
                            handle.clear_text_state();
                        }
                        *source = PersistentEditor::new(target, node);
                    } else {
                        source.target.path.clone_from(&target.path);
                    }
                } else {
                    if self.editor.is_none()
                        && let Some(index) = self.retained_selections.iter().position(|selection| {
                            selection.source.same_target(target) && selection.source.matches(node)
                        })
                    {
                        self.editor = Some(self.retained_selections.remove(index).editor);
                    }
                    self.persistent_editor = Some(PersistentEditor::new(target, node));
                }
            } else {
                self.persistent_editor = None;
            }
            if self.editor.is_none() {
                self.editor = Some(TextEdit::at_end(&self.editor_value(node)));
            }
            if let Some(editor) = &mut self.editor
                && editor
                    .password_echo
                    .as_ref()
                    .is_some_and(|echo| !echo.matches(node, text_input_value(node)))
            {
                editor.password_echo = None;
            }
            if (is_text_input(node.kind) || combo_editable(node))
                && let Some(target) = &self.focused
                && let Some(history) = self.history.get_mut(&EditorKey::new(scene, target))
            {
                history.observe(text_input_value(node));
            }
        } else {
            self.persistent_editor = None;
            self.editor = None;
            self.spin_draft = None;
        }
        self.reconcile_text_composition(scene);
    }

    fn retain_editor_selection(&mut self) {
        let old = self.editor.take();
        let Some(source) = self.persistent_editor.take() else {
            return;
        };
        let Some(editor) = old else { return };
        let state = editor.selection_state(&source.text);
        if state.collapsed() {
            return;
        }
        let mut saved = TextEdit::default();
        saved.restore_selection(&source.text, state);
        saved.set_horizontal_drag_scroll(editor.horizontal_drag_scroll());
        if let Some(handle) = &source.handle {
            let range = saved.selection(&source.text);
            let text = if source.password {
                ""
            } else {
                &source.text[range.clone()]
            };
            handle.publish_text_selection(saved.caret(&source.text), range, text);
            handle.publish_text_commands(Default::default());
        }
        self.retained_selections.push(RetainedSelection {
            source,
            editor: saved,
        });
    }

    #[doc(hidden)]
    pub fn set_editor_window_active(&mut self, active: bool) {
        self.editor_window_inactive = !active;
        if !active && let Some(editor) = &mut self.editor {
            editor.password_echo = None;
        }
    }

    #[doc(hidden)]
    pub fn password_echo_deadline(&self) -> Option<Instant> {
        Some(self.editor.as_ref()?.password_echo.as_ref()?.deadline)
    }

    #[doc(hidden)]
    pub fn expire_password_echo(&mut self, now: Instant) -> bool {
        if let Some(editor) = &mut self.editor
            && editor
                .password_echo
                .as_ref()
                .is_some_and(|echo| now >= echo.deadline)
        {
            editor.password_echo = None;
            return true;
        }
        false
    }

    #[doc(hidden)]
    pub fn editor_active(&self, path: &[usize]) -> bool {
        !self.editor_window_inactive && self.is_focused(path)
    }

    #[doc(hidden)]
    pub fn displayed_editor(&self, node: &Node, path: &[usize]) -> Option<&TextEdit> {
        if self.is_focused(path) {
            return self.editor.as_ref().filter(|_| {
                !self.editor_window_inactive
                    || self
                        .persistent_editor
                        .as_ref()
                        .is_some_and(|source| source.matches(node))
            });
        }
        self.retained_selections.iter().find_map(|selection| {
            (selection.source.target.path == path && selection.source.matches(node))
                .then_some(&selection.editor)
        })
    }

    pub(crate) fn retained_editor_for<'a>(
        &'a self,
        scene: &'a Node,
        target: &Target,
        handle: &FocusHandle,
    ) -> Option<(&'a TextEdit, &'a Node)> {
        for selection in &self.retained_selections {
            let source = &selection.source;
            if source.handle.as_ref() == Some(handle)
                && source.target.row_key == target.row_key
                && let Some(node) = source
                    .target
                    .node_in(scene)
                    .filter(|node| source.matches(node))
            {
                return Some((&selection.editor, node));
            }
        }
        let source = self.persistent_editor.as_ref()?;
        if !self.editor_window_inactive
            || !source.same_target(target)
            || source.handle.as_ref() != Some(handle)
        {
            return None;
        }
        let node = source
            .target
            .node_in(scene)
            .filter(|node| source.matches(node))?;
        Some((self.editor.as_ref()?, node))
    }

    pub fn break_text_edit_group(&mut self) {
        self.history.break_group();
        if let Some(draft) = &mut self.spin_draft {
            draft.history.break_group();
        }
        if let Some(edit) = &mut self.table_edit {
            edit.history.break_group();
        }
    }

    pub fn edit_text(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        action: EditAction<'_>,
        source: EditSource,
        now: Instant,
    ) -> bool {
        self.apply_text_edit(scene, action, source, now, |target, text| {
            component.edit_at(scene, &target.path, text)
        })
    }

    fn apply_text_edit(
        &mut self,
        scene: &Node,
        action: EditAction<'_>,
        source: EditSource,
        now: Instant,
        commit: impl FnOnce(&Target, String) -> bool,
    ) -> bool {
        let Some(target) = self.focused.clone() else {
            return false;
        };
        self.apply_text_edit_at(scene, &target, action, source, now, commit)
    }

    #[doc(hidden)]
    pub fn apply_text_replacement(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        layout: &LayoutNode,
        handle: &FocusHandle,
        replacement: &xrack_runtime::TextReplacement,
        now: Instant,
    ) -> Option<Target> {
        let modal = self.active_dialog(layout).unwrap_or_default();
        let target = crate::focus_scope::handle_target(scene, layout, modal, handle, true)?;
        let focused = self.focused.as_ref() == Some(&target);
        if focused {
            if self.editor.as_ref().is_some_and(TextEdit::is_composing) {
                return None;
            }
            self.sync_editor(scene);
        }
        let action = EditAction::ReplaceRange {
            start: replacement.start,
            end: replacement.end,
            text: &replacement.text,
        };
        let changed = self.apply_text_edit_at(
            scene,
            &target,
            action,
            EditSource::Atomic,
            now,
            |target, text| component.edit_at(scene, &target.path, text),
        );
        if changed && !focused {
            handle.discard_editor_changes();
        }
        changed.then_some(target)
    }

    fn apply_text_edit_at(
        &mut self,
        scene: &Node,
        target: &Target,
        action: EditAction<'_>,
        source: EditSource,
        now: Instant,
        commit: impl FnOnce(&Target, String) -> bool,
    ) -> bool {
        let Some(node) = scene.interactive_at_path(&target.path).filter(|node| {
            self.is_text_editor(node, target) && scene.row_key_at(&target.path) == target.row_key
        }) else {
            return false;
        };
        if node.text_read_only() && action.changes_text() {
            return false;
        }

        let active = self.focused.as_ref() == Some(target);
        let explicit = matches!(action, EditAction::ReplaceRange { .. });
        let mut inactive = (!active).then(|| {
            self.displayed_editor(node, &target.path)
                .cloned()
                .unwrap_or_else(|| TextEdit::at_end(text_input_value(node)))
        });
        let draft = self
            .table_edit
            .as_mut()
            .filter(|edit| &edit.target == target);
        let text = draft
            .as_ref()
            .map_or_else(|| text_input_value(node), |edit| &edit.draft);
        let editor = inactive
            .as_mut()
            .unwrap_or_else(|| self.editor.get_or_insert_with(|| TextEdit::at_end(text)));
        editor.password_echo = None;
        let previous = editor.clone();
        let before = editor.selection_state(text);
        let mask = node.text("input-mask").filter(|mask| !mask.is_empty());
        let overwrite = node.boolean("overwrite-mode", false)
            && matches!(source, EditSource::Keyboard | EditSource::Ime)
            && matches!(action, EditAction::Insert(_))
            && before.collapsed();
        let Some(mut edit) = editor.apply_mode(
            text,
            action,
            is_multiline_text_input(node.kind),
            overwrite && mask.is_none(),
        ) else {
            if explicit {
                *editor = previous;
            } else if active {
                self.break_text_edit_group();
            }
            return false;
        };
        if let Some(mask) = mask {
            let delete = match action {
                EditAction::Backspace | EditAction::BackspaceWord if before.collapsed() => {
                    Some(xrack_runtime::MaskDelete::Backward)
                }
                EditAction::Delete | EditAction::DeleteWord if before.collapsed() => {
                    Some(xrack_runtime::MaskDelete::Forward)
                }
                _ => None,
            };
            let range = edit.start..edit.start + edit.removed.len();
            let Some(formatted) = xrack_runtime::InputMask::parse(mask).and_then(|mask| {
                if overwrite {
                    mask.overwrite(text, range.start, &edit.inserted)
                } else {
                    mask.edit(text, range, &edit.inserted, delete)
                }
            }) else {
                *editor = previous;
                if !explicit {
                    editor.set_preedit(String::new(), None);
                }
                return false;
            };
            editor.place_caret(
                &formatted.text,
                TextPosition {
                    offset: formatted.caret,
                    trailing: !edit.inserted.is_empty(),
                },
                false,
            );
            edit = edit.replace_text(text, formatted.text);
        }
        if edit.text == text {
            if explicit {
                *editor = previous;
            } else if active {
                self.break_text_edit_group();
            }
            return false;
        }
        if draft.is_none()
            && (!node.allows_text_edit(&edit.text) || !commit(target, edit.text.clone()))
        {
            *editor = previous;
            if !explicit {
                editor.set_preedit(String::new(), None);
            }
            return false;
        }
        let group = Group::for_edit(source, action, before, &edit);
        let after = editor.selection_state(&edit.text);
        if active
            && !self.editor_window_inactive
            && source == EditSource::Keyboard
            && let EditAction::Insert(typed) = action
        {
            editor.password_echo = crate::password::PasswordEcho::new(
                node,
                &edit.text,
                typed,
                editor.caret(&edit.text),
                now,
            );
        }
        let (next, record) = Record::new(edit, before, after);
        if active {
            self.painted_combo_completion = None;
            self.dismissed_combo_draft = None;
            if let Some(source) = &mut self.persistent_editor {
                source.text.clone_from(&next);
            }
            if node.kind == "SearchField" {
                self.search_query = (!next.is_empty()).then(|| next.clone());
                Self::clear_choice(&mut self.choice);
            }
        }
        if let Some(draft) = draft {
            draft.history.observe(&draft.draft);
            draft.draft = next.clone();
            draft.history.push(next, record, group, now);
        } else {
            let history = self.history.entry(EditorKey::new(scene, target));
            history.observe(text);
            history.push(next, record, group, now);
        }
        true
    }

    pub fn clear_search(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        path: &[usize],
        now: Instant,
    ) -> bool {
        if self
            .focused
            .as_ref()
            .is_none_or(|target| target.path != path)
            || scene
                .at_path(path)
                .is_none_or(|node| node.kind != "SearchField")
        {
            return false;
        }
        self.apply_text_edit(
            scene,
            EditAction::ReplaceAll(""),
            EditSource::Atomic,
            now,
            |target, _| component.control_at(scene, &target.path, ControlAction::ClearSearch),
        )
    }

    pub fn select_search_suggestion(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        index: usize,
        now: Instant,
    ) -> bool {
        let Some(target) = self
            .focused
            .as_ref()
            .filter(|target| scene.row_key_at(&target.path) == target.row_key)
        else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&target.path) else {
            return false;
        };
        let Some(value) = search_suggestion(node, index) else {
            return false;
        };
        let action = ControlAction::Select(index);
        let changed = if value == text_input_value(node) {
            component.control_at(scene, &target.path, action)
        } else {
            self.apply_text_edit(
                scene,
                EditAction::ReplaceAll(value),
                EditSource::Atomic,
                now,
                |target, _| component.control_at(scene, &target.path, action),
            )
        };
        self.dismiss_choice();
        changed
    }

    pub fn replay_text_edit(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        redo: bool,
    ) -> bool {
        if self.editor.as_ref().is_some_and(TextEdit::is_composing) {
            return false;
        }
        let Some(target) = &self.focused else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&target.path).filter(|node| {
            (self.is_text_editor(node, target) || spin_editable(node))
                && !node.text_read_only()
                && scene.row_key_at(&target.path) == target.row_key
        }) else {
            return false;
        };
        if is_spin_box(node.kind) {
            let Some(draft) = self
                .spin_draft
                .as_mut()
                .filter(|draft| draft.mode.is_current(node))
            else {
                return false;
            };
            let Some((next, selection)) = draft.history.replay(redo) else {
                return false;
            };
            self.editor
                .get_or_insert_with(|| TextEdit::at_end(&draft.text))
                .restore_selection(&next, selection);
            draft.text = next.clone();
            draft.history.commit_replay(next, redo);
            return true;
        }
        if let Some(edit) = self
            .table_edit
            .as_mut()
            .filter(|edit| &edit.target == target)
        {
            let Some((next, selection)) = edit.history.replay(redo) else {
                return false;
            };
            self.editor
                .get_or_insert_with(|| TextEdit::at_end(&edit.draft))
                .restore_selection(&next, selection);
            edit.draft = next.clone();
            edit.history.commit_replay(next, redo);
            return true;
        }
        let text = text_input_value(node);
        let Some(history) = self.history.get_mut(&EditorKey::new(scene, target)) else {
            return false;
        };
        history.observe(text);
        let Some((next, selection)) = history.replay(redo) else {
            return false;
        };
        if !node.allows_text_edit(&next) || !component.edit_at(scene, &target.path, next.clone()) {
            return false;
        }
        self.editor
            .get_or_insert_with(|| TextEdit::at_end(text))
            .restore_selection(&next, selection);
        if let Some(source) = &mut self.persistent_editor {
            source.text.clone_from(&next);
        }
        if node.kind == "SearchField" {
            self.search_query = (!next.is_empty()).then(|| next.clone());
            Self::clear_choice(&mut self.choice);
        }
        history.commit_replay(next, redo);
        self.painted_combo_completion = None;
        self.dismissed_combo_draft = None;
        true
    }

    pub fn edit_spin(&mut self, node: &Node, action: EditAction<'_>) {
        self.edit_spin_from(node, action, EditSource::Keyboard, Instant::now());
    }

    pub fn edit_spin_from(
        &mut self,
        node: &Node,
        action: EditAction<'_>,
        source: EditSource,
        now: Instant,
    ) {
        if !spin_editable(node) {
            return;
        }
        let text = self.editor_value(node).into_owned();
        let edit = self.editor.get_or_insert_with(|| TextEdit::at_end(&text));
        let previous = edit.clone();
        let Some(change) = edit
            .apply_mode(&text, action, false, false)
            .filter(|change| change.text != text)
        else {
            self.break_text_edit_group();
            return;
        };
        let mode = self
            .spin_draft
            .as_ref()
            .map_or_else(|| SpinDraftMode::for_node(node), |draft| draft.mode);
        if !spin_text_is_valid(&change.text, mode.separator()) {
            *edit = previous;
            return;
        }
        let before = previous.selection_state(&text);
        let group = Group::for_edit(source, action, before, &change);
        let after = edit.selection_state(&change.text);
        let (next, record) = Record::new(change, before, after);
        let draft = self.spin_draft.get_or_insert_with(|| SpinDraft {
            text: text.clone(),
            mode,
            history: TextHistory::default(),
        });
        draft.history.observe(&text);
        draft.text = next.clone();
        draft.history.push(next, record, group, now);
    }

    pub fn discard_spin_draft(&mut self) {
        self.spin_draft = None;
        self.editor = None;
    }

    pub fn commit_spin_draft(&mut self) -> Option<ControlInput> {
        let draft = self.spin_draft.take()?;
        self.editor = None;
        let path = self.focused.as_ref()?.path.clone();
        let action = draft.mode.commit(&draft.text)?;
        Some(ControlInput {
            path,
            action: ControlAction::Spin(action),
        })
    }

    /// Resolve a step from this editor's draft as a single guarded write.
    #[doc(hidden)]
    pub fn resolve_spin_action(
        &self,
        scene: &Node,
        path: &[usize],
        action: SpinAction,
    ) -> Option<SpinAction> {
        let steps = match action {
            SpinAction::Increase => 1,
            SpinAction::Decrease => -1,
            SpinAction::Step(steps) => steps,
            _ => return Some(action),
        };
        let Some(target) = self.focused.as_ref().filter(|target| target.path == path) else {
            return Some(action);
        };
        let node = target.node_in(scene)?;
        if self.editor.as_ref().is_some_and(TextEdit::is_composing) {
            return None;
        }
        let commit = self
            .spin_draft
            .as_ref()
            .filter(|draft| draft.mode.is_current(node))
            .and_then(|draft| draft.mode.commit(&draft.text));
        Some(match commit {
            Some(SpinAction::Commit(draft)) => SpinAction::StepFromIntegerDraft {
                original: None,
                draft,
                steps,
            },
            Some(SpinAction::CommitIfCurrent { original, value }) => {
                SpinAction::StepFromIntegerDraft {
                    original: Some(original),
                    draft: value,
                    steps,
                }
            }
            Some(SpinAction::CommitDecimalIfCurrent { original, value }) => {
                SpinAction::StepFromDecimalDraft {
                    original,
                    draft: value,
                    steps,
                }
            }
            _ => action,
        })
    }

    pub fn spin_press(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        self.spin_pressed = (|| {
            let target = self.pressed.as_ref()?;
            let node = scene.at_path(&target.path)?;
            if !valid(target, scene, layout) || !is_spin_box(node.kind) {
                return None;
            }
            let item = layout.at_path(&target.path)?;
            SpinGeometry::for_layout(item, node).action_at(item.map_from_window(self.pointer?)?)
        })();
        self.spin_pressed.is_some()
    }

    pub fn spin_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let pressed = self.spin_pressed.take()?;
        let target = self.pointer_release_target()?;
        let node = scene.at_path(&target.path)?;
        if !valid(&target, scene, layout) || !is_spin_box(node.kind) {
            return None;
        }
        let item = layout.at_path(&target.path)?;
        let action =
            SpinGeometry::for_layout(item, node).action_at(item.map_from_window(self.pointer?)?)?;
        (action == pressed).then_some(ControlInput {
            path: target.path,
            action: ControlAction::Spin(action),
        })
    }

    pub fn combo_completion(&self, node: &Node, path: &[usize]) -> Option<ComboCompletion> {
        let target = self
            .focused
            .as_ref()
            .filter(|target| target.path == path && target.matches_node(node))?;
        if !combo_editable(node)
            || node.is_password()
            || !node.boolean("auto-complete", false)
            || self.choice_open()
            || self.menu_open()
            || self
                .transitioning_stacks
                .iter()
                .any(|stack| path.starts_with(stack))
        {
            return None;
        }
        let draft = text_input_value(node);
        let editor = self.editor.as_ref()?;
        if !editor.preedit.is_empty()
            || editor.caret(draft) != draft.len()
            || !editor.selection(draft).is_empty()
            || self.dismissed_combo_draft.as_deref() == Some(draft)
        {
            return None;
        }
        let value = crate::choice::completion_value(node, draft)?;
        Some(ComboCompletion {
            target: target.clone(),
            value: value.into_owned(),
            draft_len: draft.len(),
        })
    }

    pub fn set_painted_combo_completion(&mut self, completion: Option<ComboCompletion>) {
        self.painted_combo_completion = completion;
    }

    pub fn painted_combo_completion_for(
        &self,
        node: &Node,
        path: &[usize],
    ) -> Option<&ComboCompletion> {
        let painted = self.painted_combo_completion.as_ref()?;
        (self.combo_completion(node, path).as_ref() == Some(painted)).then_some(painted)
    }

    pub fn combo_completion_accepts(&self, key: NavigationKey) -> bool {
        matches!(key, NavigationKey::Left | NavigationKey::Right)
            && self
                .painted_combo_completion
                .as_ref()
                .is_some_and(|completion| {
                    (key == NavigationKey::Left) == completion.continues_left()
                })
    }

    fn take_current_combo_completion(&mut self, scene: &Node) -> Option<ComboCompletion> {
        let painted = self.painted_combo_completion.take()?;
        let path = &painted.target.path;
        let node = scene.interactive_at_path(path)?;
        if !painted.target.matches_node(node) || scene.row_key_at(path) != painted.target.row_key {
            return None;
        }
        (self.combo_completion(node, path)? == painted).then_some(painted)
    }

    pub fn accept_combo_completion(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        now: Instant,
    ) -> bool {
        let Some(completion) = self.take_current_combo_completion(scene) else {
            return false;
        };
        self.edit_text(
            component,
            scene,
            EditAction::Insert(completion.suffix()),
            EditSource::Atomic,
            now,
        )
    }

    pub fn dismiss_combo_completion(&mut self, scene: &Node) -> bool {
        let Some(completion) = self.take_current_combo_completion(scene) else {
            return false;
        };
        self.dismissed_combo_draft = Some(completion.value[..completion.draft_len].to_owned());
        true
    }

    pub fn open_choice(&mut self, scene: &Node, layout: &LayoutNode, target: Target) -> bool {
        if scene.at_path(&target.path).is_some_and(combo_editable)
            && self
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return false;
        }
        let Some(mut popup) = ChoicePopup::open(target.clone(), scene, layout) else {
            return false;
        };
        self.set_focus(Some(target));
        self.cancel_shortcut_sequence();
        Self::clear_choice(&mut self.choice);
        popup.mounted(scene);
        self.choice = Some(popup);
        self.choice_typeahead = Typeahead::default();
        self.pressed = None;
        self.pressed_radio = None;
        self.hovered = None;
        self.set_split_hover(None);
        self.hovered_radio = None;
        true
    }

    pub fn open_choice_at_pointer(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
    ) -> bool {
        let origin = self.press_origin.take();
        let Some(node) = scene
            .at_path(&target.path)
            .filter(|node| node.kind == "ComboBox")
        else {
            return false;
        };
        if combo_editable(node) {
            let Some(item) = layout.at_path(&target.path) else {
                return false;
            };
            let trigger =
                ComboGeometry::for_control(node, item.bounds, item.layout_direction).trigger;
            if !origin.zip(self.pointer).is_some_and(|(start, end)| {
                [start, end].into_iter().all(|point| {
                    item.map_from_window(point)
                        .is_some_and(|point| trigger.contains(point))
                })
            }) {
                return false;
            }
        }
        self.open_choice(scene, layout, target)
    }

    pub fn choice_open(&self) -> bool {
        self.choice.is_some()
    }

    pub fn choice_is_open_for(&self, path: &[usize]) -> bool {
        self.choice
            .as_ref()
            .is_some_and(|popup| popup.target.path == path)
    }

    #[doc(hidden)]
    pub fn choice_artwork_dirty(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.choice
            .as_ref()
            .is_some_and(|popup| popup.artwork_dirty(scene, layout))
    }

    pub fn dismiss_choice(&mut self) {
        Self::clear_choice(&mut self.choice);
        self.search_query = None;
        self.choice_typeahead = Typeahead::default();
    }

    fn clear_choice(choice: &mut Option<ChoicePopup>) {
        if let Some(popup) = choice.take() {
            popup.closed();
        }
    }

    fn reconcile_search_choice(&mut self, scene: &Node, layout: &LayoutNode) {
        let Some(query) = &self.search_query else {
            return;
        };
        let Some(target) = self
            .focused
            .as_ref()
            .filter(|target| valid(target, scene, layout))
        else {
            self.dismiss_choice();
            return;
        };
        if !scene
            .at_path(&target.path)
            .is_some_and(|node| node.kind == "SearchField" && text_input_value(node) == query)
            || self.menu_open()
            || !target
                .path
                .starts_with(self.active_dialog(layout).unwrap_or_default())
        {
            self.dismiss_choice();
            return;
        }
        if self.choice.is_none() {
            self.choice = ChoicePopup::open(target.clone(), scene, layout);
        }
    }

    pub fn choice_menu<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<ChoiceMenu<'a>> {
        self.choice.as_ref()?.view(scene, layout)
    }

    pub fn choice_press(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(popup) = &mut self.choice else {
            return false;
        };
        if let Some(point) = self.pointer {
            if popup.press(scene, layout, point) {
                return true;
            }
            let within_search = scene
                .at_path(&popup.target.path)
                .is_some_and(|node| node.kind == "SearchField")
                && layout
                    .at_path(&popup.target.path)
                    .is_some_and(|item| item.contains_subtree_point(point));
            self.dismiss_choice();
            if within_search {
                self.pointer_move(scene, layout, point);
                return false;
            }
        } else {
            self.dismiss_choice();
        }
        true
    }

    pub fn choice_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let popup = self.choice.as_mut()?;
        let index = popup.release(scene, layout, self.pointer?)?;
        let path = popup.target.path.clone();
        self.dismiss_choice();
        if scene.at_path(&path).is_some_and(combo_editable) {
            self.editor = None;
        }
        Some(ControlInput {
            path,
            action: ControlAction::Select(index),
        })
    }

    pub fn choice_scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) -> bool {
        if let Some(popup) = &mut self.choice {
            popup.scroll(scene, layout, rows);
            true
        } else {
            false
        }
    }

    pub fn choice_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: ChoiceKey,
    ) -> Option<ControlInput> {
        self.choice_typeahead = Typeahead::default();
        if let Some(popup) = &mut self.choice {
            return match key {
                ChoiceKey::Escape | ChoiceKey::Open => {
                    self.dismiss_choice();
                    None
                }
                ChoiceKey::Accept => {
                    let path = popup.target.path.clone();
                    let action = if let Some(index) = popup.selected(scene, layout) {
                        ControlAction::Select(index)
                    } else if scene
                        .at_path(&path)
                        .is_some_and(|node| node.kind == "SearchField")
                    {
                        ControlAction::SubmitSearch
                    } else {
                        return None;
                    };
                    self.dismiss_choice();
                    if scene.at_path(&path).is_some_and(combo_editable) {
                        self.editor = None;
                    }
                    Some(ControlInput { path, action })
                }
                _ => {
                    popup.move_highlight(scene, layout, key);
                    None
                }
            };
        }
        let target = self.focused.as_ref()?;
        if !valid(target, scene, layout) {
            return None;
        }
        let node = scene.at_path(&target.path)?;
        if node.kind == "SearchField" {
            return match key {
                ChoiceKey::Accept => {
                    let path = target.path.clone();
                    self.dismiss_choice();
                    Some(ControlInput {
                        path,
                        action: ControlAction::SubmitSearch,
                    })
                }
                ChoiceKey::Open | ChoiceKey::Down | ChoiceKey::Up => {
                    let text = text_input_value(node);
                    self.search_query = (!text.is_empty()).then(|| text.to_owned());
                    self.reconcile_search_choice(scene, layout);
                    if let Some(popup) = &mut self.choice {
                        popup.move_highlight(scene, layout, key);
                    }
                    None
                }
                ChoiceKey::Escape => {
                    self.dismiss_choice();
                    None
                }
                _ => None,
            };
        }
        if node.kind != "ComboBox" {
            return None;
        }
        if key == ChoiceKey::Open
            || combo_editable(node) && matches!(key, ChoiceKey::Up | ChoiceKey::Down)
        {
            self.open_choice(scene, layout, target.clone());
            return None;
        }
        if combo_editable(node) {
            return (key == ChoiceKey::Accept
                && self
                    .editor
                    .as_ref()
                    .is_none_or(|edit| edit.preedit.is_empty()))
            .then(|| ControlInput {
                path: target.path.clone(),
                action: ControlAction::AcceptComboText,
            });
        }
        let current = combo_index(node);
        let index = next_choice(node, current, key, CHOICE_PAGE)?;
        (current != Some(index)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Select(index),
        })
    }

    pub fn choice_typeahead(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        text: &str,
        now: Instant,
    ) -> Option<ControlInput> {
        let target = self
            .focused
            .as_ref()
            .filter(|target| valid(target, scene, layout))?;
        let node = scene.at_path(&target.path)?;
        if node.kind != "ComboBox" || combo_editable(node) {
            return None;
        }
        if let Some(popup) = &mut self.choice {
            popup.typeahead(scene, layout, &mut self.choice_typeahead, text, now);
            return None;
        }
        let current = combo_index(node);
        let index = self.choice_typeahead.find(node, text, now, current)?;
        (current != Some(index)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Select(index),
        })
    }

    fn typeahead_view<'a>(&self, scene: &Node, focus: &'a Target) -> &'a [usize] {
        list_item_delegate_row(scene, &focus.path).map_or(focus.path.as_slice(), |(view, _)| view)
    }

    pub fn list_typeahead_focus(&self, scene: &Node) -> Option<&Target> {
        if self.editor.is_some() || self.menu_open() || self.choice_open() {
            return None;
        }
        let focus = self.focused.as_ref()?;
        let view = self.typeahead_view(scene, focus);
        scene.interactive_at_path(view)?.list_text_search.as_ref()?;
        Some(focus)
    }

    pub fn list_typeahead_ime_area(&self, scene: &Node, layout: &LayoutNode) -> Option<Rect> {
        let focus = self
            .list_typeahead_focus(scene)
            .filter(|focus| valid(focus, scene, layout))?;
        let item = layout.at_path(self.typeahead_view(scene, focus))?;
        let bounds = item.visible_window_bounds(item.bounds);
        (bounds.width > 0.0 && bounds.height > 0.0).then(|| {
            let offset = if item.layout_direction == LayoutDirection::RightToLeft {
                (bounds.width - 6.0).max(0.0)
            } else {
                6.0_f32.min(bounds.width)
            };
            Rect::new(bounds.x + offset, bounds.y, 1.0, bounds.height.min(28.0))
        })
    }

    pub fn reset_list_typeahead(&mut self) {
        self.list_typeahead = Typeahead::default();
    }

    pub fn list_typeahead(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        text: &str,
        now: Instant,
    ) -> Option<ControlInput> {
        if text.trim().is_empty() {
            return None;
        }
        let focus = self
            .list_typeahead_focus(scene)
            .filter(|focus| valid(focus, scene, layout))?;
        let path = self.typeahead_view(scene, focus).to_vec();
        let from_delegate = focus.path != path;
        let node = scene.interactive_at_path(&path)?;
        let search = node.list_text_search.as_ref()?;
        let item = layout.at_path(&path)?;
        let current = ListMetrics::selection(node)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|&index| node.list.is_some_and(|list| index < list.row_count));
        let index = self
            .list_typeahead
            .find_with(text, now, current, |start, prefix| {
                search.find(start, |label| matches_prefix(label, prefix))
            })?;
        self.reveal_row(&path, node, item, index);
        if from_delegate {
            let saved = std::mem::take(&mut self.list_typeahead);
            self.set_focus(Some(Target::from_node(
                path.clone(),
                node,
                true,
                scene.row_key_at(&path),
            )));
            self.list_typeahead = saved;
        }
        Some(ControlInput {
            path,
            action: ControlAction::Select(index),
        })
    }

    pub fn pointer_move(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if self.pointer != Some(point) {
            self.tooltip.source = TooltipSource::Pointer;
        }
        if self.editor_press.as_ref().is_some_and(|press| {
            let start = press.sample.0;
            (point.x - start.x).hypot(point.y - start.y)
                > crate::pointer_events::PRESS_AND_HOLD_SLOP
        }) {
            self.editor_press = None;
        }
        if let Some((start, target)) = self.press_origin.zip(self.pressed.as_ref())
            && target.text_link_index().is_some()
            && target
                .node_in(scene)
                .is_some_and(Node::selectable_styled_text)
            && (point.x - start.x).hypot(point.y - start.y)
                > crate::pointer_events::PRESS_AND_HOLD_SLOP
        {
            self.pointer_activation_claimed = true;
        }
        self.pointer = Some(point);
        if self.wheel_steps.as_ref().is_some_and(|steps| {
            self.pointer_hover_target(scene, layout)
                .is_none_or(|target| {
                    !target.path.starts_with(&steps.path) || target.row_key != steps.row
                })
        }) {
            self.reset_wheel_adjustment();
        }
        let hit = self.hit_at(scene, layout, point);
        let cancelled_delay = self.delay_hold.as_ref().is_some_and(|hold| {
            matches!(hold.source, DelaySource::Pointer)
                && !matches!(&hit, Hit::Target(target) if *target == hold.target)
        });
        if cancelled_delay {
            self.cancel_delay();
        }
        if self.button_hold.as_ref().is_some_and(|repeat| {
            matches!(repeat.source, DelaySource::Pointer)
                && (repeat.moved_too_far(point)
                    || !matches!(&hit, Hit::Target(target) if *target == repeat.target))
        }) {
            self.button_hold = None;
        }
        self.pointer_on_stack_slide = matches!(hit, Hit::StackSlide);
        if self.menu.is_some() || self.choice.is_some() {
            self.set_split_hover(None);
        }
        if let Some(popup) = &mut self.menu {
            return popup.hover(scene, layout, point);
        }
        if let Some(popup) = &mut self.choice {
            return popup.hover(scene, layout, point);
        }
        let (next, scroll_bar, control_part, row_grip) = match hit {
            Hit::Target(target) => (Some(target), None, None, None),
            Hit::ScrollBar(target) => (None, Some(target), None, None),
            Hit::ControlPart(target, action) => (Some(target), None, Some(action), None),
            Hit::TableRowGrip(target, slot) => {
                let grip = scene
                    .at_path(&target.path)
                    .zip(layout.at_path(&target.path))
                    .and_then(|(node, item)| {
                        Some((
                            node.children.get(slot)?.row_key?,
                            item.children.get(slot)?.bounds.height,
                        ))
                    });
                (Some(target), None, None, grip)
            }
            _ => (None, None, None, None),
        };
        let radio = next
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .filter(|node| node.kind == "RadioButton")
            .map(|node| (node.text("text").unwrap_or(""), radio_value(node)));
        let previous_radio = self
            .hovered_radio
            .as_ref()
            .map(|(label, value)| (label.as_str(), value.as_str()));
        let list_row = next.as_ref().and_then(|target| list_row(scene, target));
        let standard_button = next
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .and_then(Node::standard_button_kind);
        let tree_indicator = next.as_ref().is_some_and(|target| {
            scene
                .at_path(&target.path)
                .zip(layout.at_path(&target.path))
                .is_some_and(|(node, item)| tree_indicator_hit(node, item, point))
        });
        let body_column = next.as_ref().and_then(|target| {
            let (path, _) = table_hit_row(scene, &target.path)?;
            if scene.at_path(path)?.selection_target() == SelectionTarget::Rows {
                return None;
            }
            let item = layout.at_path(path)?;
            item.table
                .as_ref()?
                .body_column_at(item.map_from_window(point)?)
        });
        let mut table_grip = None;
        let mut table_column = None;
        if let Some(target) = &next
            && let Some(node) = scene.at_path(&target.path)
            && let Some(item) = layout.at_path(&target.path)
            && let Some(table) = &item.table
            && let Some(point) = item.map_from_window(point)
        {
            if let Some(index) = table.resize_at(node, item.clip, point) {
                let bounds = table.columns[index];
                table_grip = Some((index, bounds.width, table.mirrored()));
            } else if let Some(index) = table.column_at(point)
                && (node.selection_target() == SelectionTarget::Columns
                    || node
                        .table_header
                        .as_ref()
                        .is_some_and(|header| header.next_sort(index).is_some()))
            {
                table_column = Some(index);
            }
        }
        let changed = cancelled_delay
            || next != self.hovered
            || scroll_bar != self.hovered_scroll_bar
            || control_part != self.hovered_control_part
            || standard_button != self.hovered_standard_button
            || radio != previous_radio
            || list_row != self.hovered_list_row
            || tree_indicator != self.hovered_tree_indicator
            || table_column != self.hovered_table_column
            || table_grip != self.hovered_table_grip
            || row_grip != self.hovered_table_row_grip
            || body_column != self.hovered_body_column;
        if radio != previous_radio {
            self.hovered_radio = radio.map(|(label, value)| (label.to_owned(), value.to_owned()));
        }
        let split_hover = next.as_ref().and_then(|target| {
            let (path, _, handle) = split_target(target, scene, layout)?;
            if scene.at_path(&target.path)?.children.is_empty() {
                return None;
            }
            let state = scene.at_path(path)?.split_resize_handle.clone()?;
            Some((state, handle.slot))
        });
        self.set_split_hover(split_hover);
        self.hovered = next;
        self.hovered_scroll_bar = scroll_bar;
        self.hovered_control_part = control_part;
        self.hovered_standard_button = standard_button;
        self.hovered_list_row = list_row;
        self.hovered_tree_indicator = tree_indicator;
        self.hovered_table_column = table_column;
        self.hovered_table_grip = table_grip;
        self.hovered_table_row_grip = row_grip;
        self.hovered_body_column = body_column;
        changed
    }
    pub fn pointer_leave(&mut self) -> bool {
        let cancelled_table_selection = self.cancel_table_drag_selection();
        let cancelled_row_move = self.cancel_table_row_move();
        self.cancel_window_move();
        let cancelled_delay = self
            .delay_hold
            .as_ref()
            .is_some_and(|hold| matches!(hold.source, DelaySource::Pointer));
        if cancelled_delay {
            self.cancel_delay();
        }
        let cancelled_repeat = self
            .button_hold
            .as_ref()
            .is_some_and(|repeat| matches!(repeat.source, DelaySource::Pointer));
        if cancelled_repeat {
            self.button_hold = None;
        }
        self.pointer = None;
        self.set_split_hover(None);
        self.tumbler_wheel = None;
        self.reset_wheel_adjustment();
        self.pointer_on_stack_slide = false;
        let tooltip_was_visible = self.tooltip.pointer_left();
        self.hovered_radio = None;
        self.hovered_list_row = None;
        self.hovered_tree_indicator = false;
        self.hovered_table_column = None;
        self.hovered_table_grip = None;
        self.hovered_table_row_grip = None;
        self.hovered_body_column = None;
        self.hovered_control_part = None;
        self.hovered_standard_button = None;
        let bar = self.hovered_scroll_bar.take().is_some();
        self.hovered.take().is_some()
            || bar
            || tooltip_was_visible
            || cancelled_delay
            || cancelled_repeat
            || cancelled_table_selection
            || cancelled_row_move
    }
    pub fn pointer_press(&mut self) {
        self.press(None, None, false);
    }

    pub fn pointer_press_in_scene(&mut self, scene: &Node, layout: &LayoutNode) {
        self.replay_tap_in_scene(scene, layout);
        if let Some(target) = self.pressed.clone() {
            self.start_button_contact(scene, target, ButtonContactSource::Pointer);
        }
    }

    /// Reuse click routing for a gesture that has already delivered its press lifecycle.
    pub fn replay_tap_in_scene(&mut self, scene: &Node, layout: &LayoutNode) {
        self.remember_focus_scope(scene, layout);
        let proxy = self
            .hovered
            .as_ref()
            .filter(|target| {
                scene
                    .at_path(&target.path)
                    .is_some_and(|node| node.kind == "FocusScope")
                    && valid(target, scene, layout)
            })
            .map(|target| {
                self.scope_focus
                    .choose(scene, layout, target.clone(), false, false)
            });

        let preserve_focus = self.hovered.as_ref().is_some_and(|target| {
            target.tab_close.is_some()
                || scene.at_path(&target.path).is_some_and(|node| {
                    matches!(node.kind, "Button" | "ToolButton" | "RoundButton")
                        && !node.boolean("focus-on-click", true)
                })
        });
        self.press(proxy, Some(layout), preserve_focus);
        if self.table_resize.is_none() && self.table_row_resize.is_none() {
            self.table_column_move =
                self.pressed
                    .as_ref()
                    .zip(self.pointer)
                    .and_then(|(target, point)| {
                        Self::table_column_move_at(scene, layout, target, point)
                    });
        }
        if self.table_row_resize.is_none() && self.table_column_move.is_none() {
            self.table_row_move =
                self.pressed
                    .as_ref()
                    .zip(self.pointer)
                    .and_then(|(pressed, point)| {
                        Self::table_row_move_at(scene, layout, pressed, point)
                    });
        }
        self.remember_focus_scope(scene, layout);
    }

    fn press(&mut self, proxy: Option<Target>, layout: Option<&LayoutNode>, preserve_focus: bool) {
        self.text_selection_scroll = None;
        self.editor_press = None;
        self.cancel_shortcut_sequence();
        self.reset_list_typeahead();
        self.reset_wheel_adjustment();
        self.cancel_delay();
        self.button_hold = None;
        self.pointer_activation_claimed = false;
        if self.pointer_on_stack_slide {
            return;
        }
        self.press_origin = self.pointer;
        self.tooltip.source = TooltipSource::Pointer;
        self.tooltip.dismiss();
        self.pressed = self.hovered.clone();
        self.pressed_radio.clone_from(&self.hovered_radio);
        self.pressed_list_row = self.hovered_list_row.clone();
        self.pressed_tree_indicator = self.hovered_tree_indicator;
        self.pressed_table_column = self.hovered_table_column;
        self.table_column_move = None;
        self.table_row_move = None;
        self.table_drag_selection = None;
        self.pressed_body_column = self.hovered_body_column;
        self.pressed_control_part = self.hovered_control_part;
        self.pressed_standard_button = self.hovered_standard_button;
        self.table_resize = (|| {
            let (column, width, mirrored) = self.hovered_table_grip?;
            let target = self.pressed.clone()?;
            let pointer = self.pointer?;
            let point = match layout {
                Some(layout) => layout.at_path(&target.path)?.map_from_window(pointer)?,
                None => pointer,
            };
            Some(TableResize {
                target,
                column,
                pointer_x: point.x,
                width,
                mirrored,
            })
        })();
        self.table_row_resize = (|| {
            let (key, height) = self.hovered_table_row_grip?;
            let target = self.pressed.clone()?;
            let pointer = self.pointer?;
            let point = match layout {
                Some(layout) => layout.at_path(&target.path)?.map_from_window(pointer)?,
                None => pointer,
            };
            Some(TableRowResize {
                target,
                key,
                pointer_y: point.y,
                height,
            })
        })();
        self.slider_offset = 0.0;
        self.slider_anchor = None;
        self.slider_fraction = None;
        self.clear_range_preview();
        self.dial_pointer = None;
        self.spin_pressed = None;
        let focus = self.hovered.clone().and_then(|mut target| {
            if let Some((_, id)) = &self.hovered_list_row {
                target.path.pop();
                target.set_element_id(id.clone());
                target.focusable = true;
                target.row_key = None;
            }
            target.focusable.then_some(target)
        });
        if self.table_row_resize.is_none() && !preserve_focus {
            self.set_focus_with_reason(proxy.or(focus), xrack_runtime::FocusReason::PointerClick);
        }
        self.key_pressed = None;
    }

    pub fn split_press(&mut self, scene: &Node, layout: &LayoutNode) {
        self.clear_split_drag();
        self.split_drag = (|| {
            let target = self.pressed.as_ref()?;
            let (path, geometry, handle) = split_target(target, scene, layout)?;
            let orientation = geometry.orientation;
            let point = layout.at_path(path)?.map_from_window(self.pointer?)?;
            let position = axis_point(orientation, point);
            let state = scene.at_path(path)?.split_resize_handle.clone();
            if let Some(state) = &state {
                state.publish_pressed(Some((handle.slot, handle.after_pane())));
            }
            Some(SplitDrag {
                target: target.clone(),
                orientation,
                mirrored: geometry.mirrored,
                after_pane: handle.after_pane(),
                grab_offset: position - handle.center(orientation),
                state,
            })
        })();
    }

    pub fn split_resize_active(&self) -> bool {
        self.split_drag.is_some()
    }

    fn set_split_hover(&mut self, next: Option<(xrack_runtime::SplitResizeHandle, usize)>) {
        if next == self.split_hover {
            return;
        }
        if let Some((state, _)) = self.split_hover.take()
            && next.as_ref().is_none_or(|(next, _)| *next != state)
        {
            state.publish_hovered(None);
        }
        if let Some((state, slot)) = &next {
            state.publish_hovered(Some(*slot));
        }
        self.split_hover = next;
    }

    pub fn touch_split_press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if self.menu.is_some() || self.choice.is_some() {
            return false;
        }
        let Hit::Target(target) = self.hit_at(scene, layout, point) else {
            return false;
        };
        if split_target(&target, scene, layout).is_none() {
            return false;
        }
        self.pointer_move(scene, layout, point);
        self.pointer_press_in_scene(scene, layout);
        self.split_press(scene, layout);
        self.split_resize_active()
    }

    fn clear_split_drag(&mut self) -> bool {
        let Some(drag) = self.split_drag.take() else {
            return false;
        };
        if let Some(state) = drag.state {
            state.publish_pressed(None);
        }
        true
    }

    pub fn split_orientation(&self, scene: &Node) -> Option<Orientation> {
        if let Some(drag) = &self.split_drag {
            return Some(drag.orientation);
        }
        let handle = scene.at_path(&self.hovered.as_ref()?.path)?;
        (handle.kind == "SplitHandle").then(|| handle.orientation())
    }

    pub fn split_drag(&self, scene: &Node, layout: &LayoutNode) -> Option<SplitResize> {
        let drag = self.split_drag.as_ref()?;
        let (path, geometry, handle) = split_target(&drag.target, scene, layout)?;
        if !drag.matches(geometry, handle) {
            return None;
        }
        let point = layout.at_path(path)?.map_from_window(self.pointer?)?;
        let position = axis_point(drag.orientation, point) - drag.grab_offset;
        geometry.resize(
            path.to_vec(),
            handle,
            geometry.logical_delta(position - handle.center(drag.orientation)),
        )
    }

    pub fn finish_split_resize(&mut self) -> bool {
        if !self.clear_split_drag() {
            return false;
        }
        self.cancel_press();
        true
    }

    pub fn split_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<SplitResize> {
        let (path, geometry, handle) = split_target(self.focused.as_ref()?, scene, layout)?;
        let step = SplitHandleRange::STEP as f32;
        let delta = match (geometry.orientation, key) {
            (Orientation::Horizontal, NavigationKey::Left)
            | (Orientation::Vertical, NavigationKey::Up) => geometry.logical_delta(-step),
            (Orientation::Horizontal, NavigationKey::Right)
            | (Orientation::Vertical, NavigationKey::Down) => geometry.logical_delta(step),
            (_, NavigationKey::Home) => handle.minimum,
            (_, NavigationKey::End) => handle.maximum,
            _ => return None,
        };
        geometry.resize(path.to_vec(), handle, delta)
    }

    /// Set the leading pane size using the same limits and layout mode as a drag.
    pub fn split_set_value(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        value: f64,
    ) -> Option<SplitResize> {
        if !value.is_finite() {
            return None;
        }
        let (path, geometry, handle) = split_target(target, scene, layout)?;
        let range = geometry.handle_range(handle);
        let delta = value.clamp(range.minimum, range.maximum) - range.value;
        geometry.resize(path.to_vec(), handle, delta as f32)
    }

    pub fn range_pointer_press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if self.menu.is_some() || self.choice.is_some() {
            return false;
        }
        let Hit::Target(target) = self.hit_at(scene, layout, point) else {
            return false;
        };
        if slider_target(&target, scene, layout).is_none() {
            return false;
        }
        self.pointer_move(scene, layout, point);
        self.pointer_press_in_scene(scene, layout);
        true
    }

    pub fn touch_range_press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if !self.range_pointer_press(scene, layout, point) {
            return false;
        }
        self.touch_range = true;
        true
    }

    pub fn touch_range_captured(&self) -> bool {
        self.touch_range
    }

    pub fn finish_touch_range(&mut self) {
        if std::mem::take(&mut self.touch_range) {
            self.cancel_press();
            self.pointer_leave();
        }
    }

    pub fn slider_press(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        let target = self.pressed.as_ref()?;
        let (path, node) = slider_target(target, scene, layout)?;
        let item = layout.at_path(path)?;
        let bounds = item.bounds;
        let point = item.map_from_window(self.pointer?)?;
        if node.kind == "ColorPlane" {
            return Some(RangeInput {
                path: target.path.clone(),
                action: ColorPlaneGeometry::new(bounds).action_at(point)?,
            });
        }
        if node.kind == "Dial" {
            self.dial_pointer = Some(point);
            let geometry = DialGeometry::new(bounds, node);
            let current = SliderRange::from_node(node).fraction();
            if node.dial_input_mode() != DialInputMode::Circular
                || geometry.contains_thumb(current, point)
            {
                return None;
            }
            return Some(RangeInput {
                path: path.to_vec(),
                action: RangeAction::SetFraction(geometry.fraction_at(point)?),
            });
        }
        let geometry = SliderGeometry::with_layout_direction(bounds, node, item.layout_direction);
        let fraction = geometry.fraction_at(point)?;
        let mut path = path.to_vec();
        let current = if node.kind == "RangeSlider" {
            let interval = SliderInterval::from_node(node);
            let [lower, upper] = interval.fractions();
            let handle = geometry.interval_handle_at(point, fraction, interval);
            path.push(handle);
            self.focus_slider_thumb(scene, &path);
            let current = [lower, upper][handle];
            if lower == upper && geometry.thumb_bounds(handle, current).contains(point) {
                self.slider_anchor = Some(fraction);
            }
            current
        } else {
            SliderRange::from_node(node).fraction()
        };
        let handle = if node.kind == "RangeSlider" {
            *path.last()?
        } else {
            0
        };
        if geometry.thumb_bounds(handle, current).contains(point) {
            self.slider_offset = current - fraction;
            None
        } else {
            self.slider_pointer_action(node, path, fraction)
        }
    }

    fn focus_slider_thumb(&mut self, scene: &Node, path: &[usize]) {
        let target = Target::from_element_id(path.to_vec(), None, true, scene.row_key_at(path));
        self.set_focus(Some(target.clone()));
        self.pressed = Some(target);
    }

    /// Preview for a range with delayed commit. Used by layout and painting.
    #[doc(hidden)]
    pub fn interval_preview(&self, node: &Node, path: &[usize]) -> Option<(usize, f64)> {
        if node.boolean("live", true) {
            return None;
        }
        let (pressed, fraction) = self.slider_preview_position()?;
        let (&index, parent) = pressed.split_last()?;
        (parent == path && index < Node::RANGE_THUMBS).then_some((index, fraction))
    }

    #[doc(hidden)]
    pub fn slider_preview_position(&self) -> Option<(&[usize], f64)> {
        Some((
            self.pressed.as_ref()?.path.as_slice(),
            self.slider_fraction?,
        ))
    }

    pub fn slider_drag(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        let target = self.pressed.as_ref()?;
        let (owner, node) = slider_target(target, scene, layout)?;
        let item = layout.at_path(owner)?;
        let bounds = item.bounds;
        let point = item.map_from_window(self.pointer?)?;
        if node.kind == "ColorPlane" {
            return Some(RangeInput {
                path: target.path.clone(),
                action: ColorPlaneGeometry::new(bounds).action_at(point)?,
            });
        }
        if node.kind == "Dial" {
            let previous = self.dial_pointer.replace(point)?;
            let delta = DialGeometry::new(bounds, node).drag_delta(
                node.dial_input_mode(),
                previous,
                point,
            )?;
            let current = SliderRange::from_node(node).fraction();
            let next = current + delta;
            let next = if node.boolean("wrap", false) && !(0.0..=1.0).contains(&next) {
                next.rem_euclid(1.0)
            } else {
                next.clamp(0.0, 1.0)
            };
            return (next != current).then(|| RangeInput {
                path: target.path.clone(),
                action: RangeAction::SetFraction(next),
            });
        }
        let geometry = SliderGeometry::with_layout_direction(bounds, node, item.layout_direction);
        let fraction = geometry.fraction_at(point)?;
        let mut path = target.path.clone();
        if let Some(anchor) = self.slider_anchor {
            if fraction == anchor {
                return None;
            }
            if self.touch_range {
                // Do not choose one of two coincident thumbs from a tiny
                // initial touch displacement. Measure along the visual axis.
                let start = item.map_to_window(geometry.position(0.0));
                let end = item.map_to_window(geometry.position(1.0));
                let travel = (end.x - start.x).hypot(end.y - start.y);
                if (fraction - anchor).abs() * f64::from(travel) < f64::from(DRAG_THRESHOLD) {
                    return None;
                }
            }
            path.truncate(owner.len());
            path.push(usize::from(fraction > anchor));
            self.focus_slider_thumb(scene, &path);
            self.slider_anchor = None;
        }
        self.slider_pointer_action(node, path, fraction + self.slider_offset)
    }

    fn slider_pointer_action(
        &mut self,
        node: &Node,
        path: Vec<usize>,
        fraction: f64,
    ) -> Option<RangeInput> {
        let live = node.boolean("live", true);
        if !live || SliderRange::from_node(node).snap_mode() == ScrollSnapMode::OnRelease {
            self.slider_fraction = Some(fraction);
            if self.range_position_handle.as_ref() != node.range_position_handle.as_ref() {
                self.clear_range_preview();
                self.range_position_handle = node.range_position_handle.clone();
            }
            if let Some(handle) = &self.range_position_handle {
                let index = if node.kind == "RangeSlider" {
                    path.last().copied().unwrap_or_default()
                } else {
                    0
                };
                handle.publish_preview(Some((index, fraction)));
            }
        } else {
            self.slider_fraction = None;
            self.clear_range_preview();
        }
        live.then_some(RangeInput {
            path,
            action: RangeAction::SetFraction(fraction),
        })
    }

    pub fn slider_preview(&self, path: &[usize]) -> Option<f64> {
        self.slider_fraction.filter(|_| self.is_pressed(path))
    }

    pub fn has_slider_preview(&self) -> bool {
        self.slider_fraction.is_some() && self.pressed.is_some()
    }

    pub fn cancel_slider_preview(&mut self) -> bool {
        if !self.has_slider_preview() {
            return false;
        }
        self.cancel_press();
        true
    }

    pub fn slider_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        let fraction = self.slider_fraction.take();
        self.clear_range_preview();
        slider_target(self.pressed.as_ref()?, scene, layout)?;
        self.slider_offset = 0.0;
        self.slider_anchor = None;
        self.dial_pointer = None;
        let path = self.pressed.take()?.path;
        Some(RangeInput {
            path,
            action: fraction.map_or(RangeAction::Release, RangeAction::CommitFraction),
        })
    }

    fn clear_range_preview(&mut self) {
        if let Some(handle) = self.range_position_handle.take() {
            handle.publish_preview(None);
        }
    }

    pub fn slider_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: SliderKey,
        pressed: bool,
    ) -> Option<RangeInput> {
        if !pressed {
            let target = self.range_keys.remove(&key)?;
            if self.focused.as_ref() != Some(&target) {
                return None;
            }
            slider_target(&target, scene, layout)?;
            return Some(RangeInput {
                path: target.path,
                action: RangeAction::Release,
            });
        }
        let target = self.focused.as_ref()?;
        if !valid(target, scene, layout) {
            return None;
        }
        let (path, node) = slider_target(target, scene, layout)?;
        let action = if node.kind == "ColorPlane" {
            let saturation = node.number("saturation").unwrap_or_default();
            let brightness = node.number("brightness").unwrap_or_default();
            let step = 0.01;
            let (x, y) = match key {
                SliderKey::Left => (saturation - step, brightness),
                SliderKey::Right => (saturation + step, brightness),
                SliderKey::Up => (saturation, brightness + step),
                SliderKey::Down => (saturation, brightness - step),
                _ => return None,
            };
            RangeAction::SetPoint {
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
            }
        } else {
            let horizontal = node.kind == "Dial" || node.orientation() == Orientation::Horizontal;
            let vertical = node.kind == "Dial" || node.orientation() == Orientation::Vertical;
            let mirrored = node.kind != "Dial"
                && layout.at_path(path)?.layout_direction == LayoutDirection::RightToLeft;
            match key {
                SliderKey::Left if horizontal && mirrored => RangeAction::Increase,
                SliderKey::Right if horizontal && mirrored => RangeAction::Decrease,
                SliderKey::Left if horizontal => RangeAction::Decrease,
                SliderKey::Right if horizontal => RangeAction::Increase,
                SliderKey::Down if vertical => RangeAction::Decrease,
                SliderKey::Up if vertical => RangeAction::Increase,
                SliderKey::Home => RangeAction::Minimum,
                SliderKey::End => RangeAction::Maximum,
                _ => return None,
            }
        };
        self.range_keys.entry(key).or_insert_with(|| target.clone());
        Some(RangeInput {
            path: target.path.clone(),
            action,
        })
    }

    /// Resolve a close affordance against the current scene and clipped header.
    /// Capture the offered tab and model revision for a native close button.
    pub fn accessibility_tab_close_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        path: &[usize],
    ) -> Option<Target> {
        let mut target = self.accessibility_target(scene, layout, path)?;
        target.tab_close = Some(tab_close_identity(scene, path)?);
        self.tab_close_action(scene, layout, &target)?;
        Some(target)
    }

    pub fn tab_close_action(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        if !valid(target, scene, layout)
            || self.menu_open()
            || self.choice_open()
            || self
                .active_dialog(layout)
                .is_some_and(|scope| !target.path.starts_with(scope))
        {
            return None;
        }
        let (&index, parent) = target.path.split_last()?;
        let item = layout.at_path(&target.path)?;
        let rect = item.clip_rect(item.tab_close?);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let request = scene.at_path(parent)?.tab_close(index)?;
        if let Some((captured, value)) = &target.tab_close
            && (*captured != request
                || value != scene.at_path(&target.path)?.text("value").unwrap_or(""))
        {
            return None;
        }
        Some(ControlInput {
            path: parent.to_vec(),
            action: ControlAction::CloseTab(request),
        })
    }

    pub fn focused_tab_close(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        self.tab_close_action(scene, layout, self.focused.as_ref()?)
    }

    pub fn tab_close_hovered(&self, path: &[usize]) -> bool {
        self.hovered
            .as_ref()
            .is_some_and(|target| target.tab_close.is_some() && target.path == path)
    }

    pub fn select_group_option(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        if target.tab_close.is_some() {
            return self.tab_close_action(scene, layout, target);
        }
        if !valid(target, scene, layout) {
            return None;
        }
        if attached_radio(scene, &target.path).is_some()
            && scene.at_path(&target.path)?.kind == "RadioButton"
        {
            return Some(ControlInput {
                path: target.path.clone(),
                action: ControlAction::ActivateRadioOption,
            });
        }
        if let Some((view, slot)) =
            table_cell_row(scene, &target.path).or_else(|| tree_cell_row(scene, &target.path))
        {
            if scene.at_path(&target.path)?.boolean("table-editor", false) {
                return None;
            }
            let index = scene.at_path(view)?.list?.first_row.checked_add(slot)?;
            return Some(ControlInput {
                path: view.to_vec(),
                action: ControlAction::Select(index),
            });
        }
        let (&index, path) = target.path.split_last()?;
        let index = if let Some((index, _)) = list_row(scene, target) {
            index
        } else {
            group_item(scene.at_path(path)?, index)?;
            index
        };
        Some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::Select(index),
        })
    }

    pub fn tree_pointer_action(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        let point = self.pointer?;
        let node = scene.at_path(&target.path)?;
        let item = layout.at_path(&target.path)?;
        if !self.hovered_tree_indicator || !tree_indicator_hit(node, item, point) {
            return None;
        }
        self.tree_row_expansion(scene, layout, target, !node.boolean("tree-expanded", false))
    }

    #[doc(hidden)]
    pub fn tree_row_expansion(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        expanded: bool,
    ) -> Option<ControlInput> {
        if !valid(target, scene, layout) {
            return None;
        }
        let (&slot, path) = target.path.split_last()?;
        let node = scene.at_path(path)?;
        let index = node.list?.first_row.checked_add(slot)?;
        let tree = node.tree.as_ref()?;
        if tree.is_filtering() {
            return None;
        }
        let item = tree.rows().get(index)?;
        (item.has_children
            && item.expanded != expanded
            && node.children.get(slot)?.row_key == Some(item.row))
        .then_some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::Tree(TreeAction::SetExpanded {
                key: item.key,
                row: item.row,
                expanded,
            }),
        })
    }

    pub fn tree_has_focus(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.focused_tree(scene, layout).is_some()
    }

    fn focused_tree<'a>(&'a self, scene: &Node, layout: &LayoutNode) -> Option<&'a [usize]> {
        let target = self.focused.as_ref()?;
        if self.table_edit.is_some()
            || !valid(target, scene, layout)
            || scene.at_path(&target.path)?.boolean("table-editor", false)
        {
            return None;
        }
        let path =
            tree_cell_row(scene, &target.path).map_or(target.path.as_slice(), |(path, _)| path);
        (scene.at_path(path)?.kind == "TreeView").then_some(path)
    }

    pub fn tree_toggle(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let path = self.focused_tree(scene, layout)?;
        let node = scene.at_path(path)?;
        let tree = node.tree.as_ref()?;
        if tree.is_filtering() {
            return None;
        }
        let index = tree.visible_index(node.int("current-key")?)?;
        let item = tree.rows().get(index)?;
        item.has_children.then_some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::Tree(TreeAction::SetExpanded {
                key: item.key,
                row: item.row,
                expanded: !item.expanded,
            }),
        })
    }

    pub fn tree_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        if !matches!(key, NavigationKey::Left | NavigationKey::Right) {
            return None;
        }
        let path = self.focused_tree(scene, layout)?.to_vec();
        let node = scene.at_path(&path)?;
        let view = layout.at_path(&path)?;
        let forward = (key == NavigationKey::Right)
            != (view.layout_direction == LayoutDirection::RightToLeft);
        let tree = node.tree.as_ref()?;
        let index = tree.visible_index(node.int("current-key")?)?;
        let item = tree.rows().get(index)?;
        if !tree.is_filtering() && item.has_children && forward != item.expanded {
            return self.tree_toggle(scene, layout);
        }
        let next = if !forward {
            tree.visible_index(item.parent?)?
        } else {
            index.checked_add(1).filter(|&next| {
                tree.rows()
                    .get(next)
                    .is_some_and(|next| next.parent == Some(item.key))
            })?
        };
        self.reveal_row(&path, node, view, next);
        self.set_focus(Some(Target::from_node(
            path.clone(),
            node,
            true,
            scene.row_key_at(&path),
        )));
        Some(ControlInput {
            path,
            action: ControlAction::Select(next),
        })
    }

    pub fn tree_key_with_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        Some(
            self.tree_key(scene, layout, key)?
                .with_row_selection(scene, operation),
        )
    }

    pub fn select_group_option_with_selection(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        let mut input = self.select_group_option(scene, layout, target)?;
        let node = scene.at_path(&input.path)?;
        let ControlAction::Select(row) = input.action else {
            return Some(input);
        };
        if let Some(selection) = &node.table_selection {
            if self.pointer.is_some_and(|point| {
                layout.at_path(&input.path).is_some_and(|item| {
                    item.table
                        .as_ref()
                        .zip(item.map_from_window(point))
                        .is_some_and(|(table, point)| table.row_header.contains(point))
                })
            }) && let Some(action) =
                self.table_row_header_selection(scene, layout, target, operation)
            {
                return Some(action);
            }
            let column = if table_cell_row(scene, &target.path).is_some() {
                target.path.last().copied()
            } else {
                self.hovered_body_column
            };
            let action = match selection.target() {
                SelectionTarget::Rows => TableSelectionAction::Row {
                    index: row,
                    operation,
                },
                SelectionTarget::Cells => TableSelectionAction::Cell {
                    row,
                    column: column?,
                    operation,
                },
                SelectionTarget::Columns => TableSelectionAction::Column {
                    column: column?,
                    operation,
                },
            };
            input.action = ControlAction::TableSelection(action);
        } else if let Some(action) = tree_row_action(node, row, operation) {
            input.action = action;
        }
        Some(input)
    }

    pub fn list_item_delegate_selection(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        if !valid(target, scene, layout) {
            return None;
        }
        target.row_key?;
        let (path, slot) = list_item_delegate_row(scene, &target.path)?;
        let view = scene.interactive_at_path(path)?;
        let row = view.list?.first_row.checked_add(slot)?;
        (view.table_selection.is_some() || view.int("current-index") != Some(row as i64)).then_some(
            ControlInput {
                path: path.to_vec(),
                action: ControlAction::Select(row),
            },
        )
    }

    pub fn current_item_delegate(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        view: &Target,
    ) -> Option<Target> {
        let node = scene.at_path(&view.path)?;
        if !matches!(node.kind, "ListView" | "GridView") {
            return None;
        }
        let slot = current_list_slot(node)?;
        self.item_delegate_at_slot(scene, layout, &view.path, slot)
    }

    pub fn resolve_item_delegate(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        view: &[usize],
        row: RowKey,
    ) -> Option<Target> {
        let node = scene.at_path(view)?;
        let slot = node
            .children
            .iter()
            .position(|candidate| candidate.routing_row_key() == Some(row))?;
        self.item_delegate_at_slot(scene, layout, view, slot)
    }

    fn item_delegate_at_slot(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        view: &[usize],
        slot: usize,
    ) -> Option<Target> {
        let mut path = view.to_vec();
        path.extend([slot, 0]);
        list_item_delegate_row(scene, &path)?;
        self.accessibility_target(scene, layout, &path)
    }

    pub fn table_row_header_selection(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        let (path, slot) = Self::visible_table_row_header(scene, layout, target)?;
        let table = scene.at_path(path)?;
        let index = table.list?.first_row.checked_add(slot)?;
        let action =
            table
                .table_selection
                .as_ref()
                .map_or(ControlAction::Select(index), |selection| {
                    ControlAction::TableSelection(selection.row_header_action(index, operation))
                });
        Some(ControlInput {
            path: path.to_vec(),
            action,
        })
    }

    fn visible_table_row_header<'a>(
        scene: &Node,
        layout: &LayoutNode,
        target: &'a Target,
    ) -> Option<(&'a [usize], usize)> {
        if scene.at_path(&target.path)?.kind != "ListRow" || !valid(target, scene, layout) {
            return None;
        }
        let (path, slot) = table_hit_row(scene, &target.path)?;
        let item = layout.at_path(path)?;
        let table = item.table.as_ref()?;
        let row = table
            .row_header_cell(item.children.get(slot)?.bounds)
            .intersect(table.row_header);
        let visible = item.clip_rect(row);
        (visible.width > 0.0 && visible.height > 0.0).then_some((path, slot))
    }

    fn table_column_move_at(
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        start: Point,
    ) -> Option<TableColumnMove> {
        let node = target.node_in(scene)?;
        if !node.boolean("movable-columns", false) {
            return None;
        }
        let item = layout.at_path(&target.path)?;
        let table = item.table.as_ref()?;
        let column = table.column_at(item.map_from_window(start)?)?;
        Some(TableColumnMove {
            target: target.clone(),
            column,
            start,
            order: node.table_column_order.clone(),
            mirrored: table.mirrored(),
            claimed: false,
            scroll_wake: None,
        })
    }

    /// Start the ordinary press route only on a movable table header.
    pub fn press_table_reorder(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        if self.menu_open()
            || self.choice_open()
            || self.hovered_table_grip.is_some()
            || self.hovered_table_row_grip.is_some()
        {
            return false;
        }
        let Some((target, point)) = self.hovered.as_ref().zip(self.pointer) else {
            return false;
        };
        if Self::table_column_move_at(scene, layout, target, point).is_none()
            && Self::table_row_move_at(scene, layout, target, point).is_none()
        {
            return false;
        }
        self.pointer_press_in_scene(scene, layout);
        self.table_row_move.is_some() || self.table_column_move.is_some()
    }

    fn table_row_move_at(
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        start: Point,
    ) -> Option<TableRowMove> {
        let (path, slot) = Self::visible_table_row_header(scene, layout, target)?;
        let node = scene.at_path(path)?;
        let item = layout.at_path(path)?;
        if !node.boolean("movable-rows", false)
            || !item
                .table
                .as_ref()?
                .row_header
                .contains(item.map_from_window(start)?)
        {
            return None;
        }
        Some(TableRowMove {
            table: Target::from_node(path.to_vec(), node, true, scene.row_key_at(path)),
            from: node.list?.first_row.checked_add(slot)?,
            row: node.children.get(slot)?.row_key?,
            stamp: node.standard_list_stamp?,
            start,
            claimed: false,
            touch: false,
            scroll_wake: None,
        })
    }

    /// A row header's accessible action can move its record without a pointer.
    pub fn table_row_header_move(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        direction: NavigationKey,
    ) -> Option<ControlInput> {
        let (path, slot) = Self::visible_table_row_header(scene, layout, target)?;
        let node = scene.interactive_at_path(path)?;
        let from = node.list?.first_row.checked_add(slot)?;
        let row = node.children.get(slot)?.row_key?;
        self.adjacent_table_row_move(layout, node, path, from, row, direction)
    }

    pub fn table_row_move_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        direction: NavigationKey,
    ) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        let focus_node = scene.at_path(&focused.path)?;
        if !valid(focused, scene, layout)
            || self.is_text_editor(focus_node, focused)
            || spin_editable(focus_node)
            || self.table_edit.is_some()
        {
            return None;
        }
        let path =
            table_cell_row(scene, &focused.path).map_or(focused.path.as_slice(), |(path, _)| path);
        let node = scene.interactive_at_path(path)?;
        let list = node.list?;
        let from = usize::try_from(node.int("current-row")?).ok()?;
        let row = from
            .checked_sub(list.first_row)
            .and_then(|slot| node.children.get(slot)?.row_key)
            .or_else(|| node.row_locator.as_ref()?.key_at(from))?;
        self.adjacent_table_row_move(layout, node, path, from, row, direction)
    }

    fn adjacent_table_row_move(
        &self,
        layout: &LayoutNode,
        node: &Node,
        path: &[usize],
        from: usize,
        row: RowKey,
        direction: NavigationKey,
    ) -> Option<ControlInput> {
        if node.kind != "TableView"
            || !node.boolean("movable-rows", false)
            || self.menu_open()
            || self.choice_open()
            || self
                .active_dialog(layout)
                .is_some_and(|scope| !path.starts_with(scope))
        {
            return None;
        }
        let to = match direction {
            NavigationKey::Up => from.checked_sub(1)?,
            NavigationKey::Down => from.checked_add(1)?,
            _ => return None,
        };
        let count = node.list?.row_count;
        (from < count && to < count).then_some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::MoveRow {
                from,
                to,
                row,
                stamp: node.standard_list_stamp?,
            },
        })
    }

    /// A cell's component can receive its click after the table updates selection.
    pub fn table_delegate_selection(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        if !valid(target, scene, layout) {
            return None;
        }
        let mut node = scene;
        for (depth, &index) in target.path.iter().enumerate() {
            if node.kind == "TableView" {
                let [slot, 0, column, ..] = &target.path[depth..] else {
                    return None;
                };
                let cell = scene.at_path(&target.path[..depth + 3])?;
                if !cell.boolean("table-delegate-cell", false) {
                    return None;
                }
                let row = node.list?.first_row.checked_add(*slot)?;
                let action = match node.selection_target() {
                    SelectionTarget::Rows => TableSelectionAction::Row {
                        index: row,
                        operation,
                    },
                    SelectionTarget::Cells => TableSelectionAction::Cell {
                        row,
                        column: *column,
                        operation,
                    },
                    SelectionTarget::Columns => TableSelectionAction::Column {
                        column: *column,
                        operation,
                    },
                };
                return Some(ControlInput {
                    path: target.path[..depth].to_vec(),
                    action: ControlAction::TableSelection(action),
                });
            }
            node = node.child_at(index)?;
        }
        None
    }

    /// Resolve a selection shortcut in the current focus scope.
    pub fn selection_key_action(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: &str,
        modifiers: xrack_runtime::KeyModifiers,
    ) -> Option<ControlInput> {
        let additive = modifiers.control || modifiers.meta;
        let key = match key {
            " " | "Space" => TableSelectionKey::Toggle,
            "Escape" => TableSelectionKey::Clear,
            key if key.eq_ignore_ascii_case("a")
                && additive
                && !modifiers.alt
                && !modifiers.shift =>
            {
                TableSelectionKey::All
            }
            _ => return None,
        };
        self.table_selection_key(scene, key).or_else(|| {
            if key == TableSelectionKey::Toggle && !additive {
                return None;
            }
            self.tree_selection_key(scene, layout, key)
                .or_else(|| self.list_selection_key(scene, layout, key))
        })
    }

    fn list_selection_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: TableSelectionKey,
    ) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        if self.editor.is_some()
            || self.menu_open()
            || self.choice_open()
            || !valid(focused, scene, layout)
        {
            return None;
        }
        let path = list_item_delegate_row(scene, &focused.path)
            .map_or(focused.path.as_slice(), |(view, _)| view);
        let node = scene.interactive_at_path(path)?;
        if !matches!(node.kind, "ListView" | "GridView" | "StandardListView") {
            return None;
        }
        let selection = node.table_selection.as_ref()?;
        let count = node.list?.row_count;
        let action = match key {
            TableSelectionKey::All if count > 0 => TableSelectionAction::All,
            TableSelectionKey::Clear if !selection.is_empty() => TableSelectionAction::Clear,
            TableSelectionKey::Toggle => {
                let index = usize::try_from(ListMetrics::selection(node)?).ok()?;
                if index >= count {
                    return None;
                }
                TableSelectionAction::Row {
                    index,
                    operation: RowSelection::Toggle,
                }
            }
            _ => return None,
        };
        Some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::TableSelection(action),
        })
    }

    pub fn tree_selection_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: TableSelectionKey,
    ) -> Option<ControlInput> {
        let path = self.focused_tree(scene, layout)?;
        let node = scene.at_path(path)?;
        let selection = node.tree_selection.as_ref()?;
        let tree = node.tree.as_ref()?;
        let action = match key {
            TableSelectionKey::All if !tree.rows().is_empty() => {
                ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::All))
            }
            TableSelectionKey::Clear if !selection.is_empty() => {
                ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Clear))
            }
            TableSelectionKey::Toggle => {
                let index = tree.visible_index(node.int("current-key")?)?;
                tree_row_action(node, index, RowSelection::Toggle)?
            }
            _ => return None,
        };
        Some(ControlInput {
            path: path.to_vec(),
            action,
        })
    }

    pub fn table_selection_key(
        &self,
        scene: &Node,
        key: TableSelectionKey,
    ) -> Option<ControlInput> {
        if self.editor.is_some() {
            return None;
        }
        let target = self.focused.as_ref()?;
        if key == TableSelectionKey::Toggle
            && scene
                .at_path(&target.path)
                .is_some_and(|node| node.boolean("table-editor", false) && is_toggle(node.kind))
        {
            return None;
        }
        if scene.row_key_at(&target.path) != target.row_key {
            return None;
        }
        let cell_row = table_cell_row(scene, &target.path);
        let path = cell_row.map_or(target.path.as_slice(), |(path, _)| path);
        let node = scene.interactive_at_path(path)?;
        let selection = node.table_selection.as_ref()?;
        node.table_header.as_ref()?;
        let list = node.list?;
        let action = match key {
            TableSelectionKey::All
                if selection.multiple()
                    && (list.row_count > 0 || selection.target() == SelectionTarget::Columns) =>
            {
                TableSelectionAction::All
            }
            TableSelectionKey::Clear if !selection.is_empty() => TableSelectionAction::Clear,
            TableSelectionKey::Toggle => {
                let operation = RowSelection::Toggle;
                let header = node.table_header.as_ref()?;
                let visible =
                    |&column: &usize| header.columns.get(column).is_some_and(Node::is_visible);
                let column = cell_row
                    .and_then(|_| target.path.last().copied())
                    .or(selection.current_column())
                    .filter(visible)
                    .or_else(|| {
                        node.table_column_order
                            .iter(header.columns.len())
                            .find(visible)
                    });
                if selection.target() == SelectionTarget::Columns {
                    TableSelectionAction::Column {
                        column: column?,
                        operation,
                    }
                } else {
                    let row = if let Some((_, slot)) = cell_row {
                        list.first_row.checked_add(slot)?
                    } else {
                        usize::try_from(node.int("current-row")?).ok()?
                    };
                    if row >= list.row_count {
                        return None;
                    }
                    if selection.target() == SelectionTarget::Cells {
                        TableSelectionAction::Cell {
                            row,
                            column: column?,
                            operation,
                        }
                    } else {
                        TableSelectionAction::Row {
                            index: row,
                            operation,
                        }
                    }
                }
            }
            _ => return None,
        };
        Some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::TableSelection(action),
        })
    }

    fn focused_clipboard_table<'a>(
        &'a self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<&'a [usize]> {
        let focused = self.focused.as_ref()?;
        if self.editor.is_some()
            || self.menu_open()
            || self.choice_open()
            || !valid(focused, scene, layout)
        {
            return None;
        }
        let path = (0..=focused.path.len())
            .rev()
            .map(|depth| &focused.path[..depth])
            .find(|path| {
                scene
                    .at_path(path)
                    .is_some_and(|node| node.kind == "TableView")
            })?;
        scene.interactive_at_path(path)?;
        self.allows_popup(layout, path).then_some(path)
    }

    fn table_clipboard_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<(Vec<usize>, Vec<usize>)> {
        let path = self.focused_clipboard_table(scene, layout)?;
        let geometry = layout.at_path(path)?.table.as_ref()?;
        let columns = geometry.visible_columns().collect();
        Some((path.to_vec(), columns))
    }

    pub fn table_edit_history_target<'a>(
        &'a self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<&'a [usize]> {
        let path = self.focused_clipboard_table(scene, layout)?;
        scene
            .at_path(path)?
            .table_edit_history_enabled()
            .then_some(path)
    }

    #[doc(hidden)]
    pub fn table_paste_history_target<'a>(
        &'a self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<&'a [usize]> {
        self.table_edit_history_target(scene, layout)
    }

    pub fn table_copy_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<(Vec<usize>, Vec<usize>)> {
        let (path, columns) = self.table_clipboard_target(scene, layout)?;
        scene
            .at_path(&path)?
            .boolean("copyable", false)
            .then_some((path, columns))
    }

    pub fn table_paste_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<TablePasteDestination> {
        let (path, mut columns) = self.table_clipboard_target(scene, layout)?;
        let table = scene.at_path(&path)?;
        let row = usize::try_from(table.int("current-row")?).ok()?;
        if !table.boolean("pasteable", false) || row >= table.list?.row_count || columns.is_empty()
        {
            return None;
        }
        if let Some(column) = table.table_selection.as_ref().and_then(|selection| {
            (selection.target() != SelectionTarget::Rows)
                .then(|| selection.current_column())
                .flatten()
        }) {
            let start = columns.iter().position(|&candidate| candidate == column)?;
            columns.drain(..start);
        }
        Some(TablePasteDestination {
            path,
            row,
            columns,
            focus: self.focused.clone()?,
        })
    }

    pub fn list_key_with_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        if let Some(input) = self.table_grid_key(scene, layout, key, operation) {
            return Some(input);
        }
        Some(
            self.list_key(scene, layout, key)?
                .with_row_selection(scene, operation),
        )
    }

    fn table_grid_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
        operation: RowSelection,
    ) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        let cell = table_cell_row(scene, &focused.path);
        let path = cell
            .map_or(focused.path.as_slice(), |(path, _)| path)
            .to_vec();
        let node = scene.interactive_at_path(&path)?;
        let view = node.table_selection.as_ref()?;
        if view.target() == SelectionTarget::Rows {
            return None;
        }
        let item = layout.at_path(&path)?;
        let table = item.table.as_ref()?;
        let current = view
            .current_column()
            .or_else(|| cell.and_then(|_| focused.path.last().copied()))
            .filter(|&column| column < table.columns.len());
        let current_visual = current.and_then(|column| table.visual_index(column));
        let visible = |&column: &usize| table.is_column_visible(column);
        let visible_current = current.filter(visible);
        let (previous, following) = navigation_axis(false, item.layout_direction);
        let next = match key {
            key if key == previous => table
                .visual_columns()
                .take(current_visual.unwrap_or(table.columns.len()))
                .rev()
                .find(visible)
                .or(visible_current)
                .or_else(|| table.visible_columns().next()),
            key if key == following => table
                .visual_columns()
                .skip(current_visual.map_or(0, |column| column + 1))
                .find(visible)
                .or(visible_current)
                .or_else(|| table.visible_columns().next_back()),
            NavigationKey::Home if view.target() == SelectionTarget::Columns => {
                table.visible_columns().next()
            }
            NavigationKey::End if view.target() == SelectionTarget::Columns => {
                table.visible_columns().next_back()
            }
            _ => None,
        };
        let action = if let Some(column) = next {
            if view.target() == SelectionTarget::Columns {
                TableSelectionAction::Column { column, operation }
            } else {
                let list = node.list?;
                if list.row_count == 0 {
                    return None;
                }
                let row = ListMetrics::selection(node)
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|&row| row < list.row_count)
                    .unwrap_or(list.first_row);
                self.reveal_row(&path, node, item, row);
                TableSelectionAction::Cell {
                    row,
                    column,
                    operation,
                }
            }
        } else {
            let input = self.list_key(scene, layout, key)?;
            let ControlAction::Select(row) = input.action else {
                return None;
            };
            if view.target() == SelectionTarget::Columns {
                return Some(ControlInput {
                    path,
                    action: ControlAction::TableSelection(TableSelectionAction::Row {
                        index: row,
                        operation: RowSelection::Move,
                    }),
                });
            }
            let column = visible_current.or_else(|| table.visible_columns().next())?;
            TableSelectionAction::Cell {
                row,
                column,
                operation,
            }
        };
        let column = match action {
            TableSelectionAction::Cell { column, .. }
            | TableSelectionAction::Column { column, .. } => column,
            _ => unreachable!(),
        };
        let scroll = table.horizontal;
        let offset = table.reveal_column(column);
        if offset != scroll.offset {
            store_scroll_offset(
                item,
                &mut self.horizontal_scroll_offsets,
                &path,
                Orientation::Horizontal,
                offset,
            );
            UserScrollChange::commit(
                &mut self.user_scroll_changes,
                item,
                Orientation::Horizontal,
                scroll.offset,
                offset,
            );
        }
        self.set_focus(Some(Target::from_node(
            path.clone(),
            node,
            true,
            scene.row_key_at(&path),
        )));
        Some(ControlInput {
            path,
            action: ControlAction::TableSelection(action),
        })
    }

    fn reveal_row(&mut self, path: &[usize], node: &Node, item: &LayoutNode, row: usize) {
        let Some(scroll) = item.scroll else {
            return;
        };
        let viewport = item.table.as_ref().map_or(item.bounds, |table| table.body);
        let cells = ViewCells::new(node, viewport.width);
        let offset = cells.reveal(scroll, row, viewport.height).offset;
        if offset != scroll.offset {
            store_scroll_offset(
                item,
                &mut self.scroll_offsets,
                path,
                Orientation::Vertical,
                offset,
            );
            UserScrollChange::commit(
                &mut self.user_scroll_changes,
                item,
                Orientation::Vertical,
                scroll.offset,
                offset,
            );
        }
    }

    pub fn list_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        let cell_view = table_cell_row(scene, &target.path)
            .or_else(|| tree_cell_row(scene, &target.path))
            .or_else(|| list_item_delegate_row(scene, &target.path))
            .map(|(view, _)| view);
        let focused_cell = cell_view.is_some();
        let path = cell_view.unwrap_or(&target.path);
        let node = scene.at_path(path)?;
        let list = node.list?;
        let last = list.row_count.checked_sub(1)?;
        let layout = layout.at_path(path)?;
        layout.scroll?;
        let viewport = layout
            .table
            .as_ref()
            .map_or(layout.bounds, |table| table.body);
        let cells = ViewCells::new(node, viewport.width);
        let columns = cells.columns;
        let current = ListMetrics::selection(node)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&row| row <= last);
        let key = if node.kind == "GridView"
            && layout.layout_direction == xrack_runtime::LayoutDirection::RightToLeft
        {
            match key {
                NavigationKey::Left => NavigationKey::Right,
                NavigationKey::Right => NavigationKey::Left,
                key => key,
            }
        } else {
            key
        };
        let index = match key {
            NavigationKey::Left if node.kind == "GridView" => current.map_or(0, |index| {
                if index.is_multiple_of(columns) {
                    index
                } else {
                    index - 1
                }
            }),
            NavigationKey::Right if node.kind == "GridView" => current.map_or(0, |index| {
                if (index + 1).is_multiple_of(columns) {
                    index
                } else {
                    (index + 1).min(last)
                }
            }),
            NavigationKey::Up => {
                current.map_or(0, |index| index.checked_sub(columns).unwrap_or(index))
            }
            NavigationKey::Down => current.map_or(0, |index| {
                if index / columns == last / columns {
                    index
                } else {
                    index.saturating_add(columns).min(last)
                }
            }),
            NavigationKey::Home => 0,
            NavigationKey::End => last,
            NavigationKey::PageUp => current.map_or(0, |row| {
                cells.page(row, viewport.height, list.row_count, false)
            }),
            NavigationKey::PageDown => current.map_or(0, |row| {
                cells.page(row, viewport.height, list.row_count, true)
            }),
            _ => return None,
        };
        let action = ControlInput {
            path: path.to_vec(),
            action: ControlAction::Select(index),
        };
        self.reset_list_typeahead();
        self.reveal_row(&action.path, node, layout, index);
        if focused_cell {
            self.set_focus(Some(Target::from_node(
                action.path.clone(),
                node,
                true,
                scene.row_key_at(&action.path),
            )));
        }
        Some(action)
    }

    /// An outer `Some` consumes the shortcut even at the first or last movable tab.
    pub fn tab_move_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<Option<ControlInput>> {
        let focused = self.focused.as_ref()?;
        let (&from, path) = focused.path.split_last()?;
        let bar = scene.at_path(path)?;
        let item = layout.at_path(path)?;
        if !matches!(bar.kind, "TabBar" | "TabWidget")
            || !bar.boolean("reorderable", false)
            || !valid(focused, scene, layout)
        {
            return None;
        }
        let (previous, following) = navigation_axis(
            bar.orientation() == Orientation::Vertical,
            item.layout_direction,
        );
        let available = |slot| group_item(bar, slot).is_some();
        let to = if key == previous {
            (0..from).rev().find(|&slot| available(slot))
        } else if key == following {
            (from + 1..bar.children.len()).find(|&slot| available(slot))
        } else {
            return None;
        };
        Some(to.and_then(|to| {
            Some(ControlInput {
                path: path.to_vec(),
                action: ControlAction::MoveTab(bar.tab_move(from, to)?),
            })
        }))
    }

    pub fn group_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        if let Some((member, component)) = attached_radio(scene, &target.path) {
            let scope = self
                .focused_popup(layout)
                .or_else(|| self.active_dialog(layout))
                .unwrap_or_default();
            let choices: Vec<_> = member
                .group
                .paths()
                .filter_map(|path| {
                    let path = [component, path].concat();
                    if !path.starts_with(scope) {
                        return None;
                    }
                    let node = scene.interactive_at_path(&path)?;
                    let item = layout.at_path(&path)?;
                    let row_key = scene.row_key_at(&path);
                    eligible(node, item).then(|| Target::from_node(path, node, true, row_key))
                })
                .collect();
            let position = choices.iter().position(|choice| choice == target)?;
            let len = choices.len();
            let index = match key {
                NavigationKey::Home => 0,
                NavigationKey::End => len - 1,
                NavigationKey::Up | NavigationKey::Left => (position + len - 1) % len,
                NavigationKey::Down | NavigationKey::Right => (position + 1) % len,
                _ => return None,
            };
            let next = choices[index].clone();
            let action = ControlInput {
                path: next.path.clone(),
                action: ControlAction::ActivateRadioOption,
            };
            self.set_focus(Some(next));
            return Some(action);
        }
        let (&current, path) = target.path.split_last()?;
        let group = scene.at_path(path)?;
        group_item(group, current)?;
        let vertical =
            if group.kind == "RadioGroup" && !group.properties.contains_key("orientation") {
                true
            } else {
                group.orientation() == Orientation::Vertical
            };
        let group_layout = layout.at_path(path)?;
        let choices: Vec<_> = group_targets(group, group_layout, path, false).collect();
        let position = choices.iter().position(|(index, _)| *index == current)?;
        let len = choices.len();
        let (previous, next) = navigation_axis(vertical, group_layout.layout_direction);
        let next = match key {
            NavigationKey::Home => 0,
            NavigationKey::End => len - 1,
            key if key == previous => (position + len - 1) % len,
            key if key == next => (position + 1) % len,
            _ => return None,
        };
        let (index, next) = &choices[next];
        let action = ControlInput {
            path: path.to_vec(),
            action: ControlAction::Select(*index),
        };
        let mut next = next.clone();
        next.row_key = scene.row_key_at(&next.path);
        self.reveal_tab(group, group_layout, &action.path, *index);
        self.set_focus(Some(next));
        Some(action)
    }

    fn reveal_tab(
        &mut self,
        group: &Node,
        item: &LayoutNode,
        path: &[usize],
        index: usize,
    ) -> bool {
        if !matches!(group.kind, "TabBar" | "TabWidget") {
            return false;
        }
        let orientation = group.orientation();
        let Some(scroll) = item.scroll_on(orientation) else {
            return false;
        };
        let Some(header) = item.tab_item_rect(index) else {
            return false;
        };
        let offset = crate::selection::tab_reveal_offset(
            item.tab_viewport(),
            header,
            orientation,
            item.layout_direction,
            scroll.offset,
        )
        .clamp(0.0, scroll.max_offset);
        if offset == scroll.offset {
            return false;
        }
        let offsets = match orientation {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        offsets.insert(path.to_vec(), offset);
        true
    }

    pub fn swipe_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        if !valid(target, scene, layout) {
            return None;
        }
        let (path, view) = (0..=target.path.len()).rev().find_map(|depth| {
            let path = &target.path[..depth];
            let view = scene.interactive_at_path(path)?;
            (view.kind == "SwipeView" && view.boolean("interactive", true)).then_some((path, view))
        })?;
        let index = swipe_view_selection(view, layout.at_path(path)?, key)?;
        Some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::Select(index),
        })
    }

    pub fn swipe_delegate_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !focused.path.starts_with(scope) || !valid(focused, scene, layout) {
            return None;
        }
        let path = (scope.len()..=focused.path.len()).rev().find_map(|depth| {
            let path = &focused.path[..depth];
            layout.at_path(path)?.swipe_delegate.as_deref()?;
            Some(path)
        })?;
        let target = self.accessibility_target(scene, layout, path)?;
        let geometry = layout.at_path(path)?.swipe_delegate.as_deref()?;
        let side = match (key, geometry.side) {
            (NavigationKey::Left, SwipeSide::Left) | (NavigationKey::Right, SwipeSide::Right) => {
                SwipeSide::Closed
            }
            (NavigationKey::Left, _) => SwipeSide::Right,
            (NavigationKey::Right, _) => SwipeSide::Left,
            _ => return None,
        };
        self.swipe_delegate_action(scene, layout, &target, side)
    }

    fn swipe_delegate_action(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        side: SwipeSide,
    ) -> Option<ControlInput> {
        let geometry = layout.at_path(&target.path)?.swipe_delegate.as_deref()?;
        (valid(target, scene, layout) && geometry.side != side && geometry.supports(side)).then(
            || ControlInput {
                path: target.path.clone(),
                action: ControlAction::SwipeDelegate(side),
            },
        )
    }

    pub fn swipe_delegate_accessibility(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        expand: bool,
    ) -> Option<ControlInput> {
        let geometry = layout.at_path(&target.path)?.swipe_delegate.as_deref()?;
        if expand == (geometry.side != SwipeSide::Closed) {
            return None;
        }
        let side = if !expand {
            SwipeSide::Closed
        } else if geometry.supports(SwipeSide::Right) {
            SwipeSide::Right
        } else {
            SwipeSide::Left
        };
        self.swipe_delegate_action(scene, layout, target, side)
    }

    pub fn dismiss_focused_swipe(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        (0..=focused.path.len()).rev().find_map(|depth| {
            let path = &focused.path[..depth];
            let geometry = layout.at_path(path)?.swipe_delegate.as_deref()?;
            if geometry.side == SwipeSide::Closed {
                return None;
            }
            let target = self.accessibility_target(scene, layout, path)?;
            self.swipe_delegate_action(scene, layout, &target, SwipeSide::Closed)
        })
    }

    pub fn stack_back(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let focused = self.focused.as_ref()?;
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !focused.path.starts_with(scope) || !valid(focused, scene, layout) {
            return None;
        }
        (scope.len()..=focused.path.len()).rev().find_map(|depth| {
            let path = &focused.path[..depth];
            let stack = scene.interactive_at_path(path)?.stack.as_ref()?;
            (stack.depth() > 1).then(|| ControlInput {
                path: path.to_vec(),
                action: ControlAction::StackBack {
                    revision: stack.revision(),
                },
            })
        })
    }

    /// Resolve window-level Back when focus is outside a navigation stack.
    /// A focused stack wins; otherwise require one visible innermost candidate.
    pub fn stack_back_in_window(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        if let Some(action) = self.stack_back(scene, layout) {
            return Some(action);
        }
        fn collect(
            node: &Node,
            layout: &LayoutNode,
            path: &mut Vec<usize>,
            candidates: &mut Vec<ControlInput>,
        ) {
            if !layout.visible || !layout.enabled || candidates.len() > 1 {
                return;
            }
            let before = candidates.len();
            // Inspect the prepared scene, including only materialized list rows.
            for (slot, child, item) in layout.children_in_scene_order(node) {
                path.push(slot);
                collect(child, item, path, candidates);
                path.pop();
                if candidates.len() > 1 {
                    return;
                }
            }
            let visible = layout.bounds.intersect(layout.clip);
            if candidates.len() == before
                && visible.width > 0.0
                && visible.height > 0.0
                && let Some(stack) = node.stack.as_ref().filter(|stack| stack.depth() > 1)
            {
                candidates.push(ControlInput {
                    path: path.clone(),
                    action: ControlAction::StackBack {
                        revision: stack.revision(),
                    },
                });
            }
        }
        let scope = self.active_dialog(layout).unwrap_or_default();
        let mut path = scope.to_vec();
        let mut candidates = Vec::new();
        collect(
            scene.at_path(scope)?,
            layout.at_path(scope)?,
            &mut path,
            &mut candidates,
        );
        (candidates.len() == 1).then(|| candidates.swap_remove(0))
    }

    pub fn pointer_release(&mut self) -> Option<&'static str> {
        self.pointer_release_target().and_then(|target| target.id)
    }

    pub fn page_marker_release(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        matches!(self.pressed_control_part, Some(ControlAction::Select(_))).then_some(())?;
        self.control_part_release(scene, layout)
    }

    pub fn control_part_release(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        let action = self.pressed_control_part?;
        let pressed = self.pointer_release_target()?;
        let Hit::ControlPart(target, current) = self.hit_at(scene, layout, self.pointer?) else {
            return None;
        };
        (pressed == target && action == current).then_some(ControlInput {
            path: target.path,
            action,
        })
    }

    pub fn page_marker_hovered(&self, path: &[usize], index: usize) -> bool {
        self.hovered_control_part == Some(ControlAction::Select(index)) && self.is_hovered(path)
    }

    pub fn page_indicator_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        let node = scene.at_path(&target.path)?;
        if node.kind != "PageIndicator" || !valid(target, scene, layout) {
            return None;
        }
        let last = page_indicator_count(node).checked_sub(1)?;
        let current = page_indicator_index(node);
        let vertical = node.orientation() == Orientation::Vertical;
        let direction = layout.at_path(&target.path)?.layout_direction;
        let (previous, next) = navigation_axis(vertical, direction);
        let index = match key {
            NavigationKey::Home => 0,
            NavigationKey::End => last,
            key if key == previous => page_indicator_step(node, false)?,
            key if key == next => page_indicator_step(node, true)?,
            _ => return None,
        };
        (current != Some(index)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Select(index),
        })
    }

    /// Page between the fixed month grids in a virtual list, leaving its date and row selection alone.
    #[doc(hidden)]
    pub fn calendar_page_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> bool {
        let step = match key {
            NavigationKey::PageUp => -1,
            NavigationKey::PageDown => 1,
            _ => return false,
        };
        let Some(from) = self
            .focused
            .as_ref()
            .filter(|target| valid(target, scene, layout))
        else {
            return false;
        };
        if !from
            .node_in(scene)
            .is_some_and(|node| node.kind == "MonthGrid" && node.boolean("fixed-month", false))
        {
            return false;
        }
        let Some((view, node, item, index, order)) = (0..from.path.len()).rev().find_map(|depth| {
            let view = &from.path[..depth];
            let node = scene
                .interactive_at_path(view)
                .filter(|node| node.kind == "ListView")?;
            let item = layout
                .at_path(view)
                .filter(|item| item.enabled && item.scroll.is_some())?;
            let list = node.list?;
            let slot = from.path[depth];
            let row = node
                .child_at(slot)
                .filter(|child| child.kind == "ListRow")?
                .routing_row_key()?;
            let index = list
                .first_row
                .checked_add(slot)
                .filter(|&index| index < list.row_count)?;
            let order = node.standard_list_stamp?;
            (from.row_key == Some(row)).then_some((view, node, item, index, order))
        }) else {
            return false;
        };
        if let Some(index) = index
            .checked_add_signed(step)
            .filter(|&index| node.list.is_some_and(|list| index < list.row_count))
        {
            let page = CalendarPageFocus {
                from: from.clone(),
                view: Target::from_node(view.to_vec(), node, true, scene.row_key_at(view)),
                viewport: node.viewport_handle.clone(),
                order,
                index,
            };
            self.reveal_row(&page.view.path, node, item, index);
            self.calendar_page_focus = Some(page);
        }
        true
    }

    fn finish_calendar_page_focus(&mut self, scene: &Node, layout: &LayoutNode) {
        let Some(page) = self
            .calendar_page_focus
            .take()
            .filter(|page| self.focused.as_ref() == Some(&page.from))
        else {
            return;
        };
        // The view can recycle slots after scrolling. Resolve the destination only
        // against the same model order that supplied the previous focused row.
        let Some(view) = page.view.node_in(scene).filter(|view| {
            view.kind == "ListView"
                && view.viewport_handle == page.viewport
                && view.standard_list_stamp == Some(page.order)
        }) else {
            return;
        };
        let Some(slot) = view
            .list
            .and_then(|list| page.index.checked_sub(list.first_row))
        else {
            return;
        };
        let Some(row) = view
            .child_at(slot)
            .filter(|row| row.kind == "ListRow")
            .and_then(Node::routing_row_key)
        else {
            return;
        };
        let mut path = page.view.path;
        let tail = &page.from.path[path.len() + 1..];
        path.push(slot);
        path.extend_from_slice(tail);
        let Some(target) = self
            .accessibility_target(scene, layout, &path)
            .filter(|target| {
                target.focusable
                    && target.row_key == Some(row)
                    && target.element_id() == page.from.element_id()
                    && target.node_in(scene).is_some_and(|node| {
                        node.kind == "MonthGrid" && node.boolean("fixed-month", false)
                    })
            })
        else {
            return;
        };
        self.set_focus(Some(target));
        self.remember_focus_scope(scene, layout);
    }

    pub fn calendar_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        let node = scene.at_path(&target.path)?;
        if !is_calendar_control(node.kind) || !valid(target, scene, layout) {
            return None;
        }
        let item = layout.at_path(&target.path)?;
        let geometry = CalendarGeometry::for_layout(node, item);
        let calendar = &geometry.state;
        let day = |delta| {
            let date = geometry
                .navigation_origin()?
                .add_days(delta)
                .unwrap_or(if delta < 0 { Date::MIN } else { Date::MAX })
                .clamp(calendar.minimum, calendar.maximum);
            calendar.selectable_from(date, delta > 0)
        };
        let month = |offset| {
            if geometry.has_header() {
                calendar.adjacent_month(offset)
            } else {
                calendar.month.add_months(offset)
            }
            .unwrap_or(calendar.month)
        };
        let right_step = if item.layout_direction == LayoutDirection::RightToLeft {
            -1
        } else {
            1
        };
        let action = match key {
            NavigationKey::Left => CalendarAction::Select(day(-right_step)?),
            NavigationKey::Right => CalendarAction::Select(day(right_step)?),
            NavigationKey::Up => CalendarAction::Select(day(-7)?),
            NavigationKey::Down => CalendarAction::Select(day(7)?),
            NavigationKey::Home | NavigationKey::End => {
                let date = if key == NavigationKey::Home {
                    calendar.month.max(calendar.minimum)
                } else {
                    calendar.month.last_of_month().min(calendar.maximum)
                };
                let date = calendar.selectable_from(date, key == NavigationKey::Home)?;
                if date.first_of_month() != calendar.month {
                    return None;
                }
                CalendarAction::Select(date)
            }
            NavigationKey::PageUp | NavigationKey::PageDown if calendar.fixed_month => return None,
            NavigationKey::PageUp => CalendarAction::Browse(month(-1)),
            NavigationKey::PageDown => CalendarAction::Browse(month(1)),
        };
        Some(ControlInput {
            path: target.path.clone(),
            action: ControlAction::Calendar(action),
        })
    }

    pub fn text_link_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        path: &[usize],
        index: usize,
    ) -> Option<Target> {
        let node = scene.at_path(path)?;
        let mut target = Target::for_text_link(node, path, index)?;
        target.row_key = scene.row_key_at(path);
        self.text_link_activation(scene, layout, &target)
            .map(|_| target)
    }

    pub fn text_link_activation(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        let index = target.text_link_index()?;
        let scope = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout));
        (self.menu.is_none()
            && self.choice.is_none()
            && scope.is_none_or(|scope| target.path.starts_with(scope))
            && valid(target, scene, layout))
        .then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::ActivateTextLink(index),
        })
    }

    pub fn calendar_activate(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        let node = scene.at_path(&target.path)?;
        if !is_calendar_control(node.kind) || !valid(target, scene, layout) {
            return None;
        }
        let geometry = CalendarGeometry::for_layout(node, layout.at_path(&target.path)?);
        let date = geometry.cursor()?;
        geometry.state.allows(date).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Calendar(geometry.day_action(date)),
        })
    }

    pub fn pointer_release_target(&mut self) -> Option<Target> {
        self.text_selection_scroll = None;
        if self.pressed.is_some() {
            self.last_editor_click = self.editor_press.take().filter(|press| {
                self.pressed.as_ref() == Some(&press.target)
                    && self.hovered.as_ref() == Some(&press.target)
                    && !self.pointer_activation_claimed
            });
        }
        self.finish_button_contact(
            ButtonContactSource::Pointer,
            self.pressed.is_some() && self.pressed == self.hovered,
        );
        if self
            .button_hold
            .as_ref()
            .is_some_and(|repeat| matches!(repeat.source, DelaySource::Pointer))
        {
            self.button_hold = None;
        }
        let split_resized = self.clear_split_drag();
        self.table_column_move = None;
        if self.cancel_table_row_move() {
            return None;
        }
        if self.cancel_table_drag_selection() {
            return None;
        }
        let claimed = std::mem::take(&mut self.pointer_activation_claimed);
        if self
            .delay_hold
            .as_ref()
            .is_some_and(|hold| matches!(hold.source, DelaySource::Pointer))
        {
            self.cancel_delay();
        }
        if self.finish_scroll_bar() {
            return None;
        }
        let pressed_radio = self.pressed_radio.take();
        let pressed_list = self.pressed_list_row.take();
        let pressed_column = self.pressed_table_column.take();
        let pressed_body_column = self.pressed_body_column.take();
        let pressed_control_part = self.pressed_control_part.take();
        let pressed_standard_button = self.pressed_standard_button.take();
        let resized = self.table_resize.take().is_some() | self.table_row_resize.take().is_some();
        let pressed = self.pressed.take()?;
        (!claimed
            && !resized
            && !split_resized
            && self.hovered.as_ref() == Some(&pressed)
            && pressed_radio == self.hovered_radio
            && pressed_list == self.hovered_list_row
            && self.pressed_tree_indicator == self.hovered_tree_indicator
            && pressed_column == self.hovered_table_column
            && pressed_body_column == self.hovered_body_column
            && pressed_control_part == self.hovered_control_part
            && pressed_standard_button == self.hovered_standard_button)
            .then_some(pressed)
    }

    pub fn table_sort_release(&mut self) -> Option<ControlInput> {
        let column = self.pressed_table_column?;
        let target = self.pointer_release_target()?;
        Some(ControlInput {
            path: target.path,
            action: ControlAction::SortColumn(column),
        })
    }

    pub fn table_header_release(
        &mut self,
        scene: &Node,
        operation: RowSelection,
        sort: bool,
    ) -> Option<ControlInput> {
        let mut input = self.table_sort_release()?;
        let ControlAction::SortColumn(column) = input.action else {
            return None;
        };
        let table = scene.interactive_at_path(&input.path)?;
        if table.selection_target() == SelectionTarget::Columns && !sort {
            input.action =
                ControlAction::TableSelection(TableSelectionAction::Column { column, operation });
        } else {
            table.table_header.as_ref()?.next_sort(column)?;
        }
        Some(input)
    }

    pub fn table_column_hovered(&self, path: &[usize], column: usize) -> bool {
        self.is_hovered(path) && self.hovered_table_column == Some(column)
    }

    /// Arm a mouse selection after the ordinary click and drag-source routes are resolved.
    pub fn arm_table_drag_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        additive: bool,
    ) -> bool {
        self.table_drag_selection = (|| {
            if self.drag.is_some() || self.table_row_resize.is_some() || self.table_resize.is_some()
            {
                return None;
            }
            self.table_selection_at(
                scene,
                layout,
                self.pressed.as_ref()?,
                self.pointer?,
                additive,
            )
            .filter(|drag| !drag.gesture.uses_hold(false))
        })();
        self.table_drag_selection.is_some()
    }

    /// Press a table cell only if it can arm a drag selection.
    pub fn press_table_drag_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        additive: bool,
    ) -> bool {
        if self.drag.is_some()
            || self.hovered_table_row_grip.is_some()
            || self.hovered_table_grip.is_some()
            || self.menu_open()
            || self.choice_open()
        {
            return false;
        }
        let Some(drag) = self
            .hovered
            .as_ref()
            .zip(self.pointer)
            .and_then(|(target, point)| {
                self.table_selection_at(scene, layout, target, point, additive)
                    .filter(|drag| !drag.gesture.uses_hold(false))
            })
        else {
            return false;
        };
        self.pointer_press_in_scene(scene, layout);
        self.table_drag_selection = Some(drag);
        true
    }

    fn table_selection_at(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        point: Point,
        additive: bool,
    ) -> Option<crate::table_drag_selection::TableDragSelection> {
        if self.is_text_editor(scene.at_path(&target.path)?, target) {
            return None;
        }
        let (path, slot) = table_hit_row(scene, &target.path)?;
        let table = scene.at_path(path)?;
        let item = layout.at_path(path)?;
        let column = item
            .table
            .as_ref()?
            .body_column_at(item.map_from_window(point)?)?;
        crate::table_drag_selection::TableDragSelection::new(
            path.to_vec(),
            table,
            (table.list?.first_row + slot, column),
            point,
            additive,
            scene.row_key_at(path),
            item.scroll_axis_reversed(Orientation::Horizontal),
        )
    }

    fn table_gesture_at(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        point: Point,
        touch: bool,
        now: Instant,
        additive: bool,
    ) -> Option<PendingTableGesture> {
        if touch && let Some(drag) = Self::table_row_move_at(scene, layout, target, point) {
            return Some(PendingTableGesture {
                drag: PendingTableAction::Move(drag),
                deadline: Some(now + Duration::from_millis(500)),
            });
        }
        let select = self.table_selection_at(scene, layout, target, point, additive)?;
        let hold = select.gesture.uses_hold(touch);
        if !touch && !hold {
            return None;
        }
        // The explicit modes still leave embedded editors and controls in charge
        // of their own gestures. Table cells or their row backgrounds can select.
        if select.gesture != xrack_runtime::SelectionGesture::Auto
            && !matches!(
                scene.at_path(&target.path)?.kind,
                "TableCell" | "TableRow" | "ListRow"
            )
        {
            return None;
        }
        let deadline = hold.then(|| now + Duration::from_millis(select.hold_interval));
        Some(PendingTableGesture {
            drag: PendingTableAction::Select(select),
            deadline,
        })
    }

    /// Claim an undragged press when the table's hold deadline expires.
    pub fn advance_table_touch_hold(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some(gesture) = &mut self.scroll_gesture else {
            return false;
        };
        let Some(hold) = gesture
            .table_gesture
            .take_if(|hold| hold.deadline.is_some_and(|deadline| now >= deadline))
        else {
            return false;
        };
        let touch = gesture.touch_point.is_some();
        if self.drag.is_some() || self.menu_open() || self.choice_open() {
            return false;
        }
        match hold.drag {
            PendingTableAction::Select(mut drag) => {
                drag.claimed = true;
                drag.touch = touch;
                drag.scroll_capture = true;
                if self
                    .table_drag_route(scene, layout, &drag, drag.start)
                    .is_none()
                {
                    return false;
                }
                self.cancel_scroll_gesture();
                self.capture_table_selection(drag);
            }
            PendingTableAction::Move(mut drag) => {
                let Hit::Target(target) = self.hit_at(scene, layout, drag.start) else {
                    return false;
                };
                if Self::table_row_move_at(scene, layout, &target, drag.start).is_none_or(
                    |current| {
                        current.table != drag.table
                            || current.row != drag.row
                            || current.stamp != drag.stamp
                    },
                ) {
                    return false;
                }
                drag.claimed = true;
                drag.touch = true;
                self.cancel_scroll_gesture();
                self.pointer = Some(drag.start);
                self.claim_table_selection(drag.table.clone());
                self.table_row_move = Some(drag);
            }
        }
        true
    }

    pub fn table_touch_hold_deadline(&self) -> Option<Instant> {
        self.scroll_gesture
            .as_ref()?
            .table_gesture
            .as_ref()?
            .deadline
    }

    fn capture_table_selection(
        &mut self,
        mut drag: crate::table_drag_selection::TableDragSelection,
    ) {
        drag.begin_presentation();
        self.pointer = Some(drag.start);
        let focus = drag.focus_target();
        self.table_drag_selection = Some(drag);
        self.claim_table_selection(focus);
    }

    fn table_selection_handle_geometry(
        &self,
        path: &[usize],
        node: &Node,
        item: &LayoutNode,
    ) -> Option<[Option<crate::table_drag_selection::TableSelectionHandle>; 2]> {
        if !self.is_focused(path)
            || self.menu_open()
            || self.choice_open()
            || self.table_edit.is_some()
            || self.table_drag_selection_claimed()
        {
            return None;
        }
        crate::table_drag_selection::selection_handles(node, item)
    }

    /// The hit bounds of the visible handles, in the table's layout coordinates.
    pub fn table_selection_handles(
        &self,
        path: &[usize],
        node: &Node,
        item: &LayoutNode,
    ) -> impl Iterator<Item = Rect> {
        self.table_selection_handle_parts(path, node, item)
            .map(|(_, bounds)| bounds)
    }

    /// Keep the start/end slot identity even when one corner is clipped.
    pub fn table_selection_handle_parts(
        &self,
        path: &[usize],
        node: &Node,
        item: &LayoutNode,
    ) -> impl Iterator<Item = (usize, Rect)> {
        let custom =
            node.table_selection_part(0).is_some() || node.table_selection_part(1).is_some();
        let preview = if custom {
            self.table_drag_selection
                .as_ref()
                .filter(|drag| drag.path == path && drag.valid(node, item))
                .zip(self.pointer)
                .and_then(|(drag, point)| drag.preview_handles(node, item, point))
        } else {
            None
        };
        preview
            .or_else(|| self.table_selection_handle_geometry(path, node, item))
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, handle)| Some((index, handle?.bounds)))
    }

    pub fn press_table_selection_handle(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        self.pointer.is_some_and(|point| {
            self.start_table_selection_handle(scene, layout, point, false, false)
        })
    }

    fn start_table_selection_handle(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        touch: bool,
        scroll_capture: bool,
    ) -> bool {
        let drag = (|| {
            let focus = self.focused.as_ref()?;
            let node = scene.interactive_at_path(&focus.path)?;
            let item = layout.at_path(&focus.path)?;
            let local = item.map_from_window(point)?;
            let handle = self
                .table_selection_handle_geometry(&focus.path, node, item)?
                .into_iter()
                .flatten()
                .find(|handle| handle.bounds.contains(local))?;
            match self.hit_at(scene, layout, point) {
                Hit::Target(target)
                | Hit::ControlPart(target, _)
                | Hit::TableRowGrip(target, _)
                    if target.path.starts_with(&focus.path) => {}
                _ => return None,
            }
            let mut drag = crate::table_drag_selection::TableDragSelection::new(
                focus.path.clone(),
                node,
                handle.opposite,
                point,
                false,
                focus.row_key,
                item.scroll_axis_reversed(Orientation::Horizontal),
            )?;
            drag.adjust_range(node.table_selection.as_ref()?.drag_range()?);
            drag.touch = touch;
            drag.scroll_capture = scroll_capture;
            self.table_drag_route(scene, layout, &drag, point)?;
            Some(drag)
        })();
        let Some(drag) = drag else { return false };
        self.cancel_press();
        if touch {
            self.pointer_leave();
        }
        self.capture_table_selection(drag);
        true
    }

    pub fn touch_table_selection_claimed(&self) -> bool {
        self.table_drag_selection
            .as_ref()
            .is_some_and(|drag| drag.claimed && drag.touch)
    }

    pub fn touch_table_drag_claimed(&self) -> bool {
        self.touch_table_selection_claimed()
            || self.table_row_move.as_ref().is_some_and(|drag| drag.touch)
    }

    fn scroll_table_selection_claimed(&self) -> bool {
        self.table_drag_selection
            .as_ref()
            .is_some_and(|drag| drag.claimed && drag.scroll_capture)
    }

    pub fn scroll_table_drag_claimed(&self) -> bool {
        self.scroll_table_selection_claimed()
            || self.table_row_move.as_ref().is_some_and(|drag| drag.touch)
    }

    pub fn cancel_scroll_table_drag(&mut self) -> bool {
        if self.scroll_table_selection_claimed() {
            self.cancel_table_drag_selection()
        } else {
            self.cancel_table_touch_drag()
        }
    }

    pub fn cancel_table_touch_drag(&mut self) -> bool {
        if self.touch_table_selection_claimed() {
            self.cancel_table_drag_selection()
        } else if self.touch_table_drag_claimed() {
            self.cancel_table_row_move()
        } else {
            false
        }
    }

    pub fn table_drag_selection_armed(&self) -> bool {
        self.table_drag_selection.is_some()
    }

    pub fn move_table_drag_selection(&mut self, layout: &LayoutNode, now: Instant) -> bool {
        let Some(drag) = &mut self.table_drag_selection else {
            return false;
        };
        if !drag.claimed
            && let Some(point) = self.pointer
            && let Some(item) = layout.at_path(&drag.path)
            && let Some(delta) = item.map_delta_from_window(Point {
                x: point.x - drag.start.x,
                y: point.y - drag.start.y,
            })
            && delta.x.hypot(delta.y) >= DRAG_THRESHOLD
        {
            drag.claimed = true;
            drag.begin_presentation();
            drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
            let focus = drag.focus_target();
            self.claim_table_selection(focus);
            return true;
        }
        if drag.claimed && self.pointer.is_some_and(|point| point != drag.start) {
            drag.scroll_wake.get_or_insert(now + SCROLL_FRAME_INTERVAL);
        }
        drag.claimed
    }

    fn claim_table_selection(&mut self, focus: Target) {
        self.last_table_click = None;
        self.pressed = None;
        self.set_focus_with_reason(Some(focus), xrack_runtime::FocusReason::PointerClick);
    }

    pub fn table_drag_selection_claimed(&self) -> bool {
        self.table_drag_selection
            .as_ref()
            .is_some_and(|drag| drag.claimed)
    }

    pub fn cancel_table_drag_selection(&mut self) -> bool {
        if let Some(drag) = self.table_drag_selection.take().filter(|drag| drag.claimed) {
            if drag.touch {
                self.pointer = None;
            }
            self.cancel_press();
            true
        } else {
            false
        }
    }

    pub fn finish_table_drag_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        let drag = self.table_drag_selection.take()?;
        let point = self.pointer;
        self.cancel_press();
        let (node, item, point) = self.table_drag_route(scene, layout, &drag, point?)?;
        Some(ControlInput {
            action: ControlAction::TableSelection(drag.action(node, item, point)?),
            path: drag.path,
        })
    }

    fn table_drag_route<'a>(
        &self,
        scene: &'a Node,
        layout: &'a LayoutNode,
        drag: &crate::table_drag_selection::TableDragSelection,
        point: Point,
    ) -> Option<(&'a Node, &'a LayoutNode, Point)> {
        if self.menu_open()
            || self.choice_open()
            || !drag.path.starts_with(self.pointer_scope(layout, point))
            || scene.row_key_at(&drag.path) != drag.parent_row
        {
            return None;
        }
        let node = scene.interactive_at_path(&drag.path)?;
        let item = layout.at_path(&drag.path)?;
        drag.valid(node, item).then_some((node, item, point))
    }

    pub fn table_drag_preview(
        &self,
        path: &[usize],
        node: &Node,
        layout: &LayoutNode,
    ) -> Vec<Rect> {
        match (&self.table_drag_selection, self.pointer) {
            (Some(drag), Some(point)) if drag.path == path && drag.valid(node, layout) => {
                drag.preview(node, layout, point)
            }
            _ => Vec::new(),
        }
    }

    fn table_drag_scroll_delta(&self, scene: &Node, layout: &LayoutNode) -> Option<Point> {
        let drag = self.table_drag_selection.as_ref()?;
        let (_, item, point) = self.table_drag_route(scene, layout, drag, self.pointer?)?;
        let delta = drag.scroll_delta(item, point);
        (delta != Point::default()).then_some(delta)
    }

    pub fn table_drag_selection_deadline(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<Instant> {
        self.table_drag_scroll_delta(scene, layout)?;
        self.table_drag_selection.as_ref()?.scroll_wake
    }

    pub fn advance_table_drag_selection(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some(delta) = self.table_drag_scroll_delta(scene, layout) else {
            return false;
        };
        let Some(drag) = self
            .table_drag_selection
            .as_mut()
            .filter(|drag| drag.scroll_wake.is_some_and(|wake| now >= wake))
        else {
            return false;
        };
        drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
        let path = drag.path.clone();
        let Some(item) = layout.at_path(&path) else {
            return false;
        };
        let mut moved = false;
        for (axis, delta) in [
            (Orientation::Horizontal, delta.x),
            (Orientation::Vertical, delta.y),
        ] {
            if delta != 0.0
                && let Some(scroll) = item.scroll_on(axis)
            {
                moved |= self.scroll_to_user_offset(layout, &path, axis, scroll.offset + delta);
            }
        }
        moved
    }

    pub fn table_column_move_drag(&mut self, layout: &LayoutNode, now: Instant) -> bool {
        let Some(drag) = &mut self.table_column_move else {
            return false;
        };
        if !drag.claimed
            && let Some(point) = self.pointer
            && let Some(item) = layout.at_path(&drag.target.path)
            && let Some(delta) = item.map_delta_from_window(Point {
                x: point.x - drag.start.x,
                y: point.y - drag.start.y,
            })
        {
            drag.claimed = delta.x.abs() >= 8.0 && delta.x.abs() >= delta.y.abs();
            if drag.claimed {
                drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
            }
        }
        drag.claimed
    }

    pub fn table_row_move_drag(&mut self, layout: &LayoutNode, now: Instant) -> bool {
        let Some(drag) = &mut self.table_row_move else {
            return false;
        };
        if !drag.claimed
            && let Some(point) = self.pointer
            && let Some(item) = layout.at_path(&drag.table.path)
            && let Some(delta) = item.map_delta_from_window(Point {
                x: point.x - drag.start.x,
                y: point.y - drag.start.y,
            })
            && delta.y.abs() >= DRAG_THRESHOLD
            && delta.y.abs() >= delta.x.abs()
        {
            drag.claimed = true;
            drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
        }
        drag.claimed
    }

    pub fn table_row_move_claimed(&self) -> bool {
        self.table_row_move
            .as_ref()
            .is_some_and(|drag| drag.claimed)
    }

    pub fn cancel_table_row_move(&mut self) -> bool {
        if let Some(drag) = self.table_row_move.take().filter(|drag| drag.claimed) {
            if drag.touch {
                self.pointer = None;
            }
            self.cancel_press();
            true
        } else {
            false
        }
    }

    fn row_move_position(
        drag: &TableRowMove,
        node: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(Rect, Point)> {
        if !drag.claimed
            || !drag.table.matches_node(node)
            || node.standard_list_stamp != Some(drag.stamp)
            || !node.boolean("movable-rows", false)
            || !layout.visible
            || !layout.enabled
            || !layout.contains_clip_point(point)
        {
            return None;
        }
        let table = layout.table.as_ref()?;
        let local = layout.map_from_window(point)?;
        let visible = table.rows_area().intersect(layout.clip);
        (table.row_header.width > 0.0 && visible.contains(local)).then_some((visible, local))
    }

    fn row_drop(
        drag: &TableRowMove,
        node: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(usize, Rect)> {
        let (visible, point) = Self::row_move_position(drag, node, layout, point)?;
        let list = node.list?;
        let rows = node
            .children
            .iter()
            .zip(&layout.children)
            .enumerate()
            .filter(|(_, (row, item))| {
                row.kind == "ListRow" && item.visible && item.bounds.intersect(visible).height > 0.0
            });
        let mut edge = None;
        for (slot, (_, item)) in rows {
            let row = item.bounds;
            let index = list.first_row + slot;
            if point.y < row.y + row.height * 0.5 {
                edge = Some((index, row.y));
                break;
            }
            edge = Some((index + 1, row.y + row.height));
        }
        let (insertion, y) = edge?;
        let to = insertion.saturating_sub(usize::from(insertion > drag.from));
        (to != drag.from && to < list.row_count).then(|| {
            let y = y.clamp(
                visible.y + 1.0_f32.min(visible.height * 0.5),
                visible.y + visible.height - 1.0_f32.min(visible.height * 0.5),
            );
            (
                to,
                Rect::new(visible.x, y - 1.0, visible.width, 2.0).intersect(visible),
            )
        })
    }

    pub fn table_row_move_marker(
        &self,
        path: &[usize],
        node: &Node,
        layout: &LayoutNode,
    ) -> Option<Rect> {
        let drag = self
            .table_row_move
            .as_ref()
            .filter(|drag| drag.table.path == path)?;
        Self::row_drop(drag, node, layout, self.pointer?).map(|(_, marker)| marker)
    }

    pub fn table_row_move_source(&self, path: &[usize], node: &Node) -> Option<RowKey> {
        let drag = self.table_row_move.as_ref()?;
        (drag.claimed
            && drag.table.path == path
            && node.boolean("movable-rows", false)
            && node.standard_list_stamp == Some(drag.stamp))
        .then_some(drag.row)
    }

    fn table_row_move_scroll_delta(&self, scene: &Node, layout: &LayoutNode) -> Option<f32> {
        let drag = self.table_row_move.as_ref()?;
        let point = self.pointer?;
        let path = &drag.table.path;
        if !path.starts_with(self.pointer_scope(layout, point))
            || scene.row_key_at(path) != drag.table.row_key
        {
            return None;
        }
        let item = layout.at_path(path)?;
        let (visible, point) = Self::row_move_position(drag, scene.at_path(path)?, item, point)?;
        let delta = crate::table_drag_selection::edge_delta(point.y, visible.y, visible.height);
        let scroll = item.scroll?;
        ((scroll.offset + delta).clamp(0.0, scroll.max_offset) != scroll.offset).then_some(delta)
    }

    pub fn table_row_move_deadline(&self, scene: &Node, layout: &LayoutNode) -> Option<Instant> {
        self.table_row_move_scroll_delta(scene, layout)?;
        self.table_row_move.as_ref()?.scroll_wake
    }

    pub fn advance_table_row_move(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some(delta) = self.table_row_move_scroll_delta(scene, layout) else {
            return false;
        };
        let Some(drag) = self
            .table_row_move
            .as_mut()
            .filter(|drag| drag.scroll_wake.is_some_and(|wake| now >= wake))
        else {
            return false;
        };
        drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
        let path = drag.table.path.clone();
        let Some(scroll) = layout.at_path(&path).and_then(|item| item.scroll) else {
            return false;
        };
        self.scroll_to_user_offset(layout, &path, Orientation::Vertical, scroll.offset + delta)
    }

    pub fn finish_table_row_move(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        let drag = self.table_row_move.take()?;
        let point = self.pointer;
        self.cancel_press();
        let point = point?;
        let path = &drag.table.path;
        if !valid(&drag.table, scene, layout)
            || !path.starts_with(self.pointer_scope(layout, point))
        {
            return None;
        }
        let (to, _) = Self::row_drop(&drag, scene.at_path(path)?, layout.at_path(path)?, point)?;
        Some(ControlInput {
            path: drag.table.path,
            action: ControlAction::MoveRow {
                from: drag.from,
                to,
                row: drag.row,
                stamp: drag.stamp,
            },
        })
    }

    pub fn table_column_move_claimed(&self) -> bool {
        self.table_column_move
            .as_ref()
            .is_some_and(|drag| drag.claimed)
    }

    fn column_drop(
        drag: &TableColumnMove,
        node: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(usize, usize, f32)> {
        let (table, point) = Self::column_move_position(drag, node, layout, point)?;
        let from = table.visual_index(drag.column)?;
        let (to, marker) = table.column_drop(from, point)?;
        Some((from, to, marker))
    }

    fn column_move_position<'a>(
        drag: &TableColumnMove,
        node: &Node,
        layout: &'a LayoutNode,
        point: Point,
    ) -> Option<(&'a crate::TableGeometry, Point)> {
        if !drag.claimed
            || !drag.target.matches_node(node)
            || !layout.visible
            || !layout.enabled
            || !node.boolean("movable-columns", false)
            || drag.order != node.table_column_order
            || !layout.contains_clip_point(point)
        {
            return None;
        }
        let table = layout.table.as_ref()?;
        if !table.is_column_visible(drag.column) || drag.mirrored != table.mirrored() {
            return None;
        }
        let local = layout.map_from_window(point)?;
        table.header.contains(local).then_some((table, local))
    }

    fn table_column_move_scroll_delta(&self, scene: &Node, layout: &LayoutNode) -> Option<f32> {
        let drag = self.table_column_move.as_ref()?;
        let point = self.pointer?;
        let path = &drag.target.path;
        if !path.starts_with(self.pointer_scope(layout, point)) {
            return None;
        }
        let item = layout.at_path(path)?;
        let (table, point) =
            Self::column_move_position(drag, drag.target.node_in(scene)?, item, point)?;
        let visible = table.scrollable_header().intersect(item.clip);
        if !visible.contains(point) {
            return None;
        }
        let edge = (visible.width * 0.25).min(20.0);
        if edge <= 0.0 {
            return None;
        }
        let left = (visible.x + edge - point.x).clamp(0.0, edge);
        let right = (point.x - (visible.x + visible.width - edge)).clamp(0.0, edge);
        let delta = table.logical_delta((right - left) / edge * 18.0);
        let scroll = table.horizontal;
        ((scroll.offset + delta).clamp(0.0, scroll.max_offset) != scroll.offset).then_some(delta)
    }

    pub fn table_column_move_deadline(&self, scene: &Node, layout: &LayoutNode) -> Option<Instant> {
        self.table_column_move_scroll_delta(scene, layout)?;
        self.table_column_move.as_ref()?.scroll_wake
    }

    pub fn advance_table_column_move(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some(delta) = self.table_column_move_scroll_delta(scene, layout) else {
            return false;
        };
        let Some(drag) = self
            .table_column_move
            .as_mut()
            .filter(|drag| drag.scroll_wake.is_some_and(|wake| now >= wake))
        else {
            return false;
        };
        drag.scroll_wake = Some(now + SCROLL_FRAME_INTERVAL);
        let path = drag.target.path.clone();
        let Some(item) = layout.at_path(&path) else {
            return false;
        };
        let Some(scroll) = item.scroll_on(Orientation::Horizontal) else {
            return false;
        };
        if !self.set_viewport_offset(
            &path,
            item,
            Orientation::Horizontal,
            f64::from(scroll.offset + delta),
        ) {
            return false;
        }
        UserScrollChange::record(
            &mut self.user_scroll_changes,
            item,
            Orientation::Horizontal,
            scroll.offset,
        );
        true
    }

    pub fn table_column_move_marker(
        &self,
        path: &[usize],
        node: &Node,
        layout: &LayoutNode,
    ) -> Option<f32> {
        let drag = self
            .table_column_move
            .as_ref()
            .filter(|drag| drag.target.path == path)?;
        Self::column_drop(drag, node, layout, self.pointer?).map(|(_, _, marker)| marker)
    }

    pub fn finish_table_column_move(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<ControlInput> {
        let drag = self.table_column_move.take()?;
        let point = self.pointer;
        self.cancel_press();
        let point = point?;
        if !valid(&drag.target, scene, layout)
            || !drag
                .target
                .path
                .starts_with(self.pointer_scope(layout, point))
        {
            return None;
        }
        let node = scene.at_path(&drag.target.path)?;
        let item = layout.at_path(&drag.target.path)?;
        let (from, to, _) = Self::column_drop(&drag, node, item, point)?;
        Some(ControlInput {
            path: drag.target.path,
            action: ControlAction::MoveColumn { from, to },
        })
    }

    pub fn table_resize_drag(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let drag = self.table_resize.as_ref()?;
        if !valid(&drag.target, scene, layout) {
            return None;
        }
        let node = scene.at_path(&drag.target.path)?;
        let column = resizable_table_column(node, drag.column)?;
        let item = layout.at_path(&drag.target.path)?;
        let table = item.table.as_ref()?;
        if !table.is_column_visible(drag.column) || drag.mirrored != table.mirrored() {
            return None;
        }
        let bounds = table.columns[drag.column];
        let (min, max) = crate::table::column_limits(column, table.header.width);
        let point = item.map_from_window(self.pointer?)?;
        let width = drag.width + table.logical_delta(point.x - drag.pointer_x);
        if !width.is_finite() || (width.clamp(min, max) - bounds.width).abs() < 0.5 {
            return None;
        }
        Some(ControlInput {
            path: drag.target.path.clone(),
            action: ControlAction::ResizeColumn {
                column: drag.column,
                width: width.clamp(min, max).round() as u32,
            },
        })
    }

    pub fn finish_table_resize(&mut self) -> bool {
        let resizing = self.table_resize.is_some() || self.table_row_resize.is_some();
        if resizing {
            self.pointer_release_target();
        }
        resizing
    }

    pub fn table_resize_cursor(&self) -> bool {
        self.table_resize.is_some() || self.hovered_table_grip.is_some()
    }

    pub fn table_row_resize_cursor(&self) -> bool {
        self.table_row_resize.is_some() || self.hovered_table_row_grip.is_some()
    }

    pub fn table_resize_row(&self, path: &[usize]) -> Option<RowKey> {
        if let Some(drag) = &self.table_row_resize {
            (drag.target.path == path).then_some(drag.key)
        } else if self.is_hovered(path) {
            self.hovered_table_row_grip.map(|(key, _)| key)
        } else {
            None
        }
    }

    pub fn table_row_resize_drag(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let drag = self.table_row_resize.as_ref()?;
        if !valid(&drag.target, scene, layout) {
            return None;
        }
        let node = scene.interactive_at_path(&drag.target.path)?;
        if !node.boolean("resizable-rows", false) || !self.allows_popup(layout, &drag.target.path) {
            return None;
        }
        let item = layout.at_path(&drag.target.path)?;
        let slot = node
            .children
            .iter()
            .position(|row| row.row_key == Some(drag.key))?;
        let row = node.list?.first_row.checked_add(slot)?;
        let point = item.map_from_window(self.pointer?)?;
        let height = drag.height + point.y - drag.pointer_y;
        if !height.is_finite() {
            return None;
        }
        let height = ListMetrics::clamp_row_height(height).round();
        if (height - item.children.get(slot)?.bounds.height).abs() < 0.5 {
            return None;
        }
        Some(ControlInput {
            path: drag.target.path.clone(),
            action: ControlAction::ResizeRow {
                row,
                key: drag.key,
                height: height as u32,
            },
        })
    }

    pub fn table_resize_column(&self, path: &[usize]) -> Option<usize> {
        if let Some(drag) = &self.table_resize {
            (drag.target.path == path).then_some(drag.column)
        } else if self.is_hovered(path) {
            self.hovered_table_grip.map(|(column, _, _)| column)
        } else {
            None
        }
    }
    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) {
        if self
            .swipe_page_motion
            .as_ref()
            .is_some_and(|motion| !motion.is_current(scene, layout))
        {
            self.swipe_page_motion = None;
        }
        if self.table_drag_selection.as_ref().is_some_and(|drag| {
            drag.claimed
                && self
                    .pointer
                    .and_then(|point| self.table_drag_route(scene, layout, drag, point))
                    .is_none()
        }) {
            self.cancel_table_drag_selection();
        }
        if let Some(drag) = self
            .scroll_gesture
            .as_mut()
            .and_then(|gesture| gesture.tumbler.as_mut())
            && !drag.is_current(scene, layout)
        {
            drag.cancel();
        }
        if self.tumbler_flick.as_ref().is_some_and(|flick| {
            flick.active() && !self.tumbler_flick_is_current(flick, scene, layout)
        }) {
            self.interrupt_tumbler_flick();
        }
        if let Some(drag) = self
            .scroll_gesture
            .as_mut()
            .and_then(|gesture| gesture.switch.as_mut())
            && !drag.is_current(scene, layout)
        {
            drag.position = None;
            drag.cancelled = true;
        }
        self.scope_focus.reconcile(scene, layout);
        self.page_focus.reconcile(scene);
        self.remember_focus_scope(scene, layout);
        self.reconcile_dialogs(scene, layout);
        self.finish_calendar_page_focus(scene, layout);
        if self.overscroll.has_offset() {
            let scope = self.active_dialog(layout).unwrap_or_default();
            self.overscroll.reconcile(layout.at_path(scope));
        }
        let route = self
            .scroll_gesture
            .as_ref()
            .map(|gesture| &gesture.route)
            .or_else(|| self.scroll_fling.as_ref().map(|fling| &fling.route));
        if let Some(route) = route
            && self.scroll_route_is_current(scene, layout, route)
        {
            self.viewport_motion.reconcile(layout, route.pan_device);
        } else {
            self.viewport_motion.clear();
        }
        self.stack_focus.retain(|key, _| {
            scene
                .at_path(&key.path)
                .is_some_and(|view| view.kind == "StackView")
                && scene.row_key_at(&key.path) == key.row
        });
        if self.scroll_bar_press.as_ref().is_some_and(|press| {
            !press.cancelled
                && (press.node(scene).is_none()
                    || self
                        .scroll_bar_for_target(scene, layout, &press.target)
                        .is_none_or(|bar| bar.reversed() != press.reversed))
        }) && let Some(press) = &mut self.scroll_bar_press
        {
            press.cancelled = true;
        }
        if self.split_drag.as_ref().is_some_and(|drag| {
            !split_target(&drag.target, scene, layout).is_some_and(|(path, geometry, handle)| {
                drag.matches(geometry, handle)
                    && scene
                        .at_path(path)
                        .and_then(|node| node.split_resize_handle.as_ref())
                        == drag.state.as_ref()
            })
        }) {
            self.clear_split_drag();
            self.pressed = None;
        }
        let unavailable = |target: &Target, column, mirrored| {
            layout
                .at_path(&target.path)
                .and_then(|item| item.table.as_ref())
                .is_none_or(|table| {
                    !table.is_column_visible(column) || table.mirrored() != mirrored
                })
        };
        if self
            .table_resize
            .as_ref()
            .is_some_and(|drag| unavailable(&drag.target, drag.column, drag.mirrored))
            || self
                .table_column_move
                .as_ref()
                .is_some_and(|drag| unavailable(&drag.target, drag.column, drag.mirrored))
        {
            self.cancel_press();
        }
        self.scroll_offsets.retain(|path, value| {
            if let Some(scroll) = layout.at_path(path).and_then(|node| node.scroll) {
                *value = scroll.offset;
                true
            } else {
                false
            }
        });
        self.horizontal_scroll_offsets.retain(|path, value| {
            if let Some(scroll) = layout
                .at_path(path)
                .and_then(|node| node.scroll_on(Orientation::Horizontal))
                && scroll.max_offset > 0.0
            {
                *value = scroll.offset;
                true
            } else {
                false
            }
        });
        self.tree_scroll_roots = self
            .scroll_offsets
            .keys()
            .filter_map(|path| {
                Some((
                    path.clone(),
                    scene.at_path(path)?.tree.as_ref()?.root_identity(),
                ))
            })
            .collect();
        self.scroll_anchors = capture_list_anchors(scene, layout, &self.scroll_anchors);
        for (path, anchor) in &self.scroll_anchors {
            if anchor.scope.is_none()
                && anchor.offset > 0.0
                && !self.scroll_offsets.contains_key(path)
            {
                self.scroll_offsets.insert(path.clone(), anchor.offset);
            }
        }
        // Preserve the user's scroll between selection and grid reflow changes.
        self.scroll_selections = self
            .scroll_offsets
            .keys()
            .chain(self.horizontal_scroll_offsets.keys())
            .filter_map(|path| {
                let view = scene.at_path(path).filter(|node| {
                    matches!(
                        node.kind,
                        "TabBar" | "TabWidget" | "StandardListView" | "GridView"
                    ) || node.kind == "TableView" && node.boolean("follow-current", false)
                })?;
                let item = layout.at_path(path)?;
                let cells = ViewCells::new(view, item.bounds.width);
                Some((
                    path.clone(),
                    ScrollSelection::new(view, cells.columns, item.layout_direction),
                ))
            })
            .collect();
        if self
            .pressed
            .as_ref()
            .is_some_and(|t| !valid(t, scene, layout))
        {
            self.pressed = None;
            self.table_resize = None;
        }
        if let Some(current) = self
            .focused
            .as_ref()
            .filter(|t| !valid(t, scene, layout))
            .cloned()
        {
            if let Some(next) = self.restore_stack_focus(&current, scene, layout) {
                self.set_focus_with_reason(Some(next), xrack_runtime::FocusReason::LayoutChange);
            } else if let Some(moved) = moved_row_target(&current, scene, layout) {
                if let Some(offset) = self.text_offsets.remove(&current.path) {
                    self.text_offsets.insert(moved.path.clone(), offset);
                }
                if let Some(edit) = &mut self.table_edit
                    && edit.target == current
                {
                    edit.target = moved.clone();
                }
                self.focused = Some(moved);
                self.range_keys.clear();
            } else {
                let scope = self
                    .focused_popup(layout)
                    .or_else(|| self.active_dialog(layout))
                    .unwrap_or_default();
                let page = self.page_focus.restore(&current, scene, layout, scope);
                let fallback = page.or_else(|| {
                    let stops = focus_targets(scene, layout, scope);
                    attached_radio_tab_stop(scene, &current.path, &stops)
                        .map(|index| stops[index].clone())
                        .filter(|target| target.row_key == current.row_key)
                });
                let fallback = fallback.or_else(|| {
                    (0..current.path.len()).rev().find_map(|length| {
                        let path = &current.path[..length];
                        let node = scene.at_path(path)?;
                        if scene
                            .row_key_at(path)
                            .is_some_and(|key| Some(key) != current.row_key)
                        {
                            return None;
                        }
                        let item = layout.at_path(path)?;
                        let mut target = match node.kind {
                            "ListView" | "GridView" | "StandardListView" | "TableView"
                            | "TreeView" | "SwipeView" | "SwipeDelegate" | "StackView"
                            | "GroupBox" | "Expander"
                                if eligible(node, item) =>
                            {
                                Target::from_node(path.to_vec(), node, true, None)
                            }
                            "TabWidget" | "TabBar" | "RadioGroup" => group_focus(node, item, path)?,
                            "Dialog" | "Drawer" => dialog_entry_focus(scene, layout, path)?,
                            "FocusScope" => {
                                let target = crate::focus_scope::target_at(scene, layout, path)?;
                                self.scope_focus.choose(scene, layout, target, false, false)
                            }

                            _ => return None,
                        };
                        target.row_key = scene.row_key_at(&target.path);
                        Some(target)
                    })
                });
                self.set_focus_with_reason(fallback, xrack_runtime::FocusReason::LayoutChange);
            }
            self.key_pressed = None;
            self.remember_focus_scope(scene, layout);
        }
        if self.table_edit.as_ref().is_some_and(|edit| {
            !scene
                .interactive_at_path(&edit.target.path)
                .is_some_and(|node| {
                    editable_table_cell(node) && node.text("text").unwrap_or("") == edit.original
                })
        }) {
            self.cancel_table_edit();
        }
        if let Some(draft) = &self.spin_draft
            && !self
                .focused
                .as_ref()
                .and_then(|target| scene.interactive_at_path(&target.path))
                .is_some_and(|node| draft.mode.is_current(node))
        {
            self.discard_spin_draft();
        }
        self.text_offsets.retain(|path, (key, _)| {
            scene
                .at_path(path)
                .is_some_and(|node| is_multiline_text_input(node.kind))
                && scene.row_key_at(path) == *key
        });
        self.retained_selections.retain_mut(|selection| {
            let source = &mut selection.source;
            if !valid(&source.target, scene, layout)
                && let Some(target) = moved_row_target(&source.target, scene, layout)
            {
                source.target = target;
            }
            let keep = valid(&source.target, scene, layout)
                && source
                    .target
                    .node_in(scene)
                    .is_some_and(|node| source.matches(node));
            if !keep && let Some(handle) = &source.handle {
                handle.clear_text_state();
            }
            keep
        });
        if self
            .choice
            .as_mut()
            .is_some_and(|popup| !popup.reconcile(scene, layout))
        {
            Self::clear_choice(&mut self.choice);
        }
        self.reconcile_search_choice(scene, layout);
        self.reconcile_text_menu(scene, layout);
        if self
            .menu
            .as_mut()
            .is_some_and(|popup| !popup.reconcile(scene, layout))
        {
            self.dismiss_menu();
        }
        if let Some(mut pending) = self.pending_shortcut.take()
            && !self.shortcuts_blocked()
        {
            let scope = self.shortcut_scope(layout);
            let active = shortcut::bindings(
                scene,
                layout,
                scope,
                self.menu.as_ref(),
                self.focused.as_ref(),
            );
            if pending.retain_active(scope, &active) {
                self.pending_shortcut = Some(pending);
            }
        }
        if let Some(point) = self.pointer {
            // Keyboard highlight should survive a layout pass with a stationary cursor.
            if !self.choice_open() && !self.menu_open() {
                self.pointer_move(scene, layout, point);
            } else {
                self.set_split_hover(None);
            }
        }
        if let Some(handle) = &self.range_position_handle
            && self.pressed.as_ref().and_then(|target| {
                let (_, node) = slider_target(target, scene, layout)?;
                node.range_position_handle.as_ref()
            }) != Some(handle)
        {
            self.slider_fraction = None;
            self.clear_range_preview();
        }
        self.publish_button_presses(scene, layout);
    }

    fn restore_stack_focus(
        &mut self,
        outgoing: &Target,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<Target> {
        let scope = self.active_dialog(layout).unwrap_or_default();
        let (path, view) = (scope.len()..outgoing.path.len()).rev().find_map(|depth| {
            let path = &outgoing.path[..depth];
            let view = scene.interactive_at_path(path)?;
            (view.kind == "StackView"
                && path.starts_with(scope)
                && layout
                    .at_path(path)
                    .is_some_and(|item| eligible(view, item)))
            .then_some((path, view))
        })?;
        let from = outgoing.path[path.len()];
        let to = view.stack_page_index()?;
        if from == to {
            return None;
        }
        let row = scene.row_key_at(path);
        let key = |page| StackFocusKey {
            path: path.to_vec(),
            row,
            page,
        };
        self.stack_focus.insert(key(from), outgoing.clone());
        if let Some(target) = self.stack_focus.remove(&key(to))
            && valid(&target, scene, layout)
        {
            return Some(target);
        }
        let mut page = path.to_vec();
        page.push(to);
        focus_targets(scene, layout, &page)
            .into_iter()
            .next()
            .or_else(|| Some(Target::from_node(path.to_vec(), view, true, row)))
    }

    fn reconcile_dialogs(&mut self, scene: &Node, layout: &LayoutNode) {
        let key = |entry: &DialogFocus| (entry.path.clone(), entry.row_key, entry.modal);
        let previous_top = self.dialogs.last().map(key);
        let previous_modal = self.dialogs.iter().rfind(|entry| entry.modal).map(key);
        let mut focus = self.focused.clone();
        let mut index = 0;
        while index < self.dialogs.len() {
            let dialog = &mut self.dialogs[index];
            if layout.overlay_paths().contains(&dialog.path)
                && scene.row_key_at(&dialog.path) == dialog.row_key
            {
                dialog.modal = is_modal_path(layout, &dialog.path);
                index += 1;
                continue;
            }
            let removed = self.dialogs.remove(index);
            let falls_inside = |target: &Option<Target>| {
                target
                    .as_ref()
                    .is_some_and(|target| target.path.starts_with(&removed.path))
            };
            if falls_inside(&focus) {
                focus = removed.return_focus.clone();
            }
            for dialog in &mut self.dialogs {
                if falls_inside(&dialog.return_focus) {
                    dialog.return_focus = removed.return_focus.clone();
                }
            }
        }
        for path in layout.overlay_paths() {
            if self.dialogs.iter().any(|dialog| dialog.path == *path) {
                continue;
            }
            let modal = is_modal_path(layout, path);
            self.dialogs.push(DialogFocus {
                path: path.clone(),
                row_key: scene.row_key_at(path),
                modal,
                return_focus: focus.clone(),
            });
            if modal
                || self.allows_popup(layout, path)
                    && scene
                        .at_path(path)
                        .is_some_and(|node| node.boolean("focus", true))
            {
                focus = dialog_entry_focus(scene, layout, path);
            }
        }
        if previous_top == self.dialogs.last().map(key)
            && previous_modal == self.dialogs.iter().rfind(|entry| entry.modal).map(key)
            && focus == self.focused
        {
            return;
        }
        let active = self.active_dialog(layout);
        let scope = active.unwrap_or_default();
        let focus = focus
            .filter(|target| target.path.starts_with(scope) && valid(target, scene, layout))
            .or_else(|| active.and_then(|path| dialog_entry_focus(scene, layout, path)));
        self.dismiss_choice();
        self.dismiss_menu();
        self.cancel_press();
        self.cancel_shortcut_sequence();
        self.set_focus_with_reason(focus, xrack_runtime::FocusReason::PopupActivation);
    }
    /// Move focus, returning whether revealing the focused tab requires layout.
    pub fn focus_next(&mut self, scene: &Node, layout: &LayoutNode, reverse: bool) -> bool {
        self.remember_focus_scope(scene, layout);
        self.tooltip.focus_source();
        let scope = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        let boundary = scope.len();
        let targets = focus_targets_with_reveal(scene, layout, scope, true);
        self.key_pressed = None;
        if targets.is_empty() {
            self.set_focus_with_reason(None, xrack_runtime::FocusReason::TabNavigation);
            return false;
        }
        let current = self.focused.as_ref().and_then(|target| {
            targets
                .iter()
                .position(|entry| entry == target)
                .or_else(|| attached_radio_tab_stop(scene, &target.path, &targets))
                .or_else(|| {
                    let (&index, path) = target.path.split_last()?;
                    let group = scene.at_path(path).filter(|node| {
                        matches!(node.kind, "TabWidget" | "TabBar" | "RadioGroup")
                    })?;
                    group_item(group, index)?;
                    targets.iter().position(|entry| {
                        entry
                            .path
                            .split_last()
                            .is_some_and(|(_, parent)| parent == path)
                    })
                })
                .or_else(|| {
                    let (view, _) = tree_cell_row(scene, &target.path).or_else(|| {
                        table_cell_row(scene, &target.path).filter(|(table, _)| {
                            scene
                                .at_path(table)
                                .is_some_and(|node| node.boolean("__standard-table", false))
                        })
                    })?;
                    targets.iter().position(|entry| entry.path == view)
                })
        });
        let index = match current {
            Some(i) if reverse => (i + targets.len() - 1) % targets.len(),
            Some(i) => (i + 1) % targets.len(),
            None if reverse => targets.len() - 1,
            None => 0,
        };
        let next = self
            .page_focus
            .tab_entry(
                scene,
                layout,
                self.focused.as_ref(),
                &targets[index],
                boundary,
                &targets,
            )
            .unwrap_or_else(|| targets[index].clone());
        let next = self
            .scope_focus
            .entered_scope(
                scene,
                layout,
                self.focused.as_ref(),
                &next,
                boundary,
                true,
                true,
            )
            .map_or(next, |scope| {
                self.scope_focus.choose(scene, layout, scope, reverse, true)
            });
        let revealed = next.path.split_last().is_some_and(|(&index, path)| {
            scene
                .at_path(path)
                .zip(layout.at_path(path))
                .is_some_and(|(group, item)| self.reveal_tab(group, item, path, index))
        });
        let revealed = self.reveal_focus_target(scene, layout, &next) || revealed;
        self.set_focus_with_reason(Some(next), xrack_runtime::FocusReason::TabNavigation);
        self.remember_focus_scope(scene, layout);
        revealed
    }

    /// The next editable field in the current popup or window's tab order.
    #[doc(hidden)]
    pub fn next_text_field(&self, scene: &Node, layout: &LayoutNode) -> Option<Target> {
        let current = self.focused.as_ref()?;
        let scope = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        let targets = focus_targets_with_reveal(scene, layout, scope, true);
        let start = targets.iter().position(|target| target == current)? + 1;
        targets.into_iter().skip(start).find(|target| {
            target.node_in(scene).is_some_and(|node| {
                !node.text_read_only()
                    && (is_text_input(node.kind) || combo_editable(node) || spin_editable(node))
            })
        })
    }

    /// Move to the next editor and report whether its scroll position changed.
    #[doc(hidden)]
    pub fn focus_next_text_field(&mut self, scene: &Node, layout: &LayoutNode) -> Option<bool> {
        let target = self.next_text_field(scene, layout)?;
        self.remember_focus_scope(scene, layout);
        self.dismiss_choice();
        self.tooltip.focus_source();
        let revealed = self.reveal_focus_target(scene, layout, &target);
        self.set_focus_with_reason(Some(target), xrack_runtime::FocusReason::KeyboardNavigation);
        self.remember_focus_scope(scene, layout);
        Some(revealed)
    }

    /// Resolve an accessibility action through the current modal and layout scope.
    pub fn accessibility_target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        path: &[usize],
    ) -> Option<Target> {
        if self
            .active_dialog(layout)
            .is_some_and(|scope| !path.starts_with(scope))
        {
            return None;
        }
        let node = scene.interactive_at_path(path)?;
        let item = layout.at_path(path)?;
        let scrollable = scroll_target_visible(item)
            && [Orientation::Horizontal, Orientation::Vertical]
                .into_iter()
                .any(|axis| {
                    item.scroll_on(axis)
                        .is_some_and(|scroll| scroll.max_offset > 0.0)
                });
        if !eligible(node, item) && !scrollable {
            return None;
        }
        Some(Target::from_node(
            path.to_vec(),
            node,
            focusable(node),
            scene.row_key_at(path),
        ))
    }

    pub(crate) fn focus_reason(&self) -> xrack_runtime::FocusReason {
        self.focus_reason
    }

    pub(crate) fn focus_entries(
        &self,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Vec<crate::focus_observers::FocusEntry> {
        let Some(focus) = self
            .focused
            .as_ref()
            .filter(|target| valid(target, scene, layout))
        else {
            return Vec::new();
        };
        let boundary = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        if !focus.path.starts_with(boundary) {
            return Vec::new();
        }
        (boundary.len()..=focus.path.len())
            .filter_map(|depth| {
                let path = &focus.path[..depth];
                let node = scene.at_path(path)?;
                Some(crate::focus_observers::FocusEntry {
                    handle: node.focus_handle.clone()?,
                    direct: depth == focus.path.len(),
                    target: Target::from_node(path.to_vec(), node, true, scene.row_key_at(path)),
                })
            })
            .collect()
    }

    pub fn remember_focus_scope(&mut self, scene: &Node, layout: &LayoutNode) {
        self.scope_focus
            .remember(scene, layout, self.focused.as_ref());
        self.page_focus
            .remember(scene, layout, self.focused.as_ref());
    }

    pub fn key_route(&self, scene: &Node, layout: &LayoutNode) -> Option<crate::KeyRoute> {
        if self.menu_open()
            || self
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return None;
        }
        let focus = self
            .focused
            .as_ref()
            .filter(|target| valid(target, scene, layout))?;
        let boundary = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        if !focus.path.starts_with(boundary) {
            return None;
        }
        let scopes = (boundary.len()..=focus.path.len())
            .filter_map(|depth| {
                let path = &focus.path[..depth];
                let node = scene.at_path(path)?;
                if node.kind != "FocusScope" || node.key_handlers == 0 {
                    return None;
                }
                crate::focus_scope::target_at(scene, layout, path)
            })
            .collect();
        crate::KeyRoute::new(focus.clone(), scopes)
    }

    pub fn discard_range_key(&mut self, key: SliderKey) {
        self.range_keys.remove(&key);
    }

    /// A focus request uses the same scene eligibility and modal gate as native input.
    pub fn focus_target(&mut self, scene: &Node, layout: &LayoutNode, target: Target) {
        self.focus_target_with_reason(
            scene,
            layout,
            target,
            xrack_runtime::FocusReason::Accessibility,
        );
    }

    #[doc(hidden)]
    pub fn focus_text_from_touch(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
    ) -> bool {
        self.focus_target_with_reason(
            scene,
            layout,
            target.clone(),
            xrack_runtime::FocusReason::PointerClick,
        );
        self.focused.as_ref() == Some(&target)
    }

    fn focus_target_with_reason(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: Target,
        reason: xrack_runtime::FocusReason,
    ) -> bool {
        let reveal = reason == xrack_runtime::FocusReason::Programmatic;
        let target = if target
            .node_in(scene)
            .is_some_and(|node| node.kind == "RadioGroup")
        {
            let Some(target) = radio_group_focus(scene, layout, &target.path, reveal) else {
                return false;
            };
            target
        } else {
            target
        };
        if !focus_reachable(&target, scene, layout, reveal)
            || self
                .active_dialog(layout)
                .is_some_and(|modal| !target.path.starts_with(modal))
        {
            return false;
        }
        self.remember_focus_scope(scene, layout);
        let target = self
            .scope_focus
            .choose_with_reveal(scene, layout, target, false, false, reveal);
        let scrolled = reveal && self.reveal_focus_target(scene, layout, &target);
        self.tooltip.focus_source();
        self.set_focus_with_reason(Some(target), reason);
        self.remember_focus_scope(scene, layout);
        scrolled
    }

    /// Reveal the active editor before reconciling focus after a viewport change.
    #[doc(hidden)]
    pub fn reveal_focused_editor(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(target) = self.focused_editor_in(scene, layout).cloned() else {
            return false;
        };
        self.reveal_focus_target(scene, layout, &target)
    }

    #[doc(hidden)]
    pub fn keyboard_avoidance(&self) -> Option<(&[usize], f32)> {
        self.keyboard_avoidance
            .as_ref()
            .map(|(path, shift)| (path.as_slice(), *shift))
    }

    /// Update the temporary reveal offset without writing any scroll bindings.
    #[doc(hidden)]
    pub fn avoid_floating_keyboard(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        keyboard: Option<Rect>,
    ) -> bool {
        let next = keyboard.and_then(|keyboard| {
            let target = self.focused_editor_in(scene, layout)?;
            let item = layout.at_path(&target.path)?;
            let field = item.window_bounds(focus_hitbox(target.node_in(scene)?, item)?);
            crate::viewport::plan_keyboard_avoidance(
                scene,
                layout,
                &target.path,
                field,
                keyboard,
                self.keyboard_avoidance(),
            )
        });
        if match (&self.keyboard_avoidance, &next) {
            (Some((path, shift)), Some((next_path, next_shift))) => {
                path == next_path && (shift - next_shift).abs() < 0.01
            }
            (None, None) => true,
            _ => false,
        } {
            return false;
        }
        self.keyboard_avoidance = next;
        true
    }

    fn focused_editor_in(&self, scene: &Node, layout: &LayoutNode) -> Option<&Target> {
        let target = self.focused.as_ref()?;
        let node = target.node_in(scene)?;
        (self.allows_popup(layout, &target.path)
            && (self.is_text_editor(node, target) || spin_editable(node)))
        .then_some(target)
    }

    fn reveal_focus_target(&mut self, scene: &Node, layout: &LayoutNode, target: &Target) -> bool {
        if target.text_link.is_some() {
            return false;
        }
        let Some(rect) = scene
            .at_path(&target.path)
            .zip(layout.at_path(&target.path))
            .and_then(|(node, item)| focus_hitbox(node, item))
        else {
            return false;
        };
        self.reveal_focus_in_viewports(scene, layout, &target.path, rect)
    }

    pub fn apply_focus_requests(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        requests: impl IntoIterator<Item = xrack_runtime::FocusRequest>,
    ) {
        for request in requests {
            if let Some(command) = self.apply_focus_request(scene, layout, request) {
                self.apply_text_selection(scene, command);
            }
        }
    }

    /// Apply a focus command or resolve a text command against the current editor.
    pub fn apply_focus_request(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        request: xrack_runtime::FocusRequest,
    ) -> Option<TextCommand> {
        self.apply_focus_request_with_scroll(scene, layout, request)
            .0
    }

    #[doc(hidden)]
    pub fn apply_focus_request_with_scroll(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        request: xrack_runtime::FocusRequest,
    ) -> (Option<TextCommand>, bool) {
        let mut scrolled = false;
        let command = (|| {
            let modal = self.active_dialog(layout).unwrap_or_default();
            let reveal = matches!(request.action, xrack_runtime::FocusAction::Focus);
            let target =
                crate::focus_scope::handle_target(scene, layout, modal, &request.target, reveal)?;
            match request.action {
                xrack_runtime::FocusAction::Focus => {
                    scrolled = self.focus_target_with_reason(
                        scene,
                        layout,
                        target,
                        xrack_runtime::FocusReason::Programmatic,
                    );
                }
                xrack_runtime::FocusAction::Clear => {
                    let scope = scene.at_path(&target.path).is_some_and(|node| {
                        matches!(node.kind, "FocusScope" | "Loader" | "RadioGroup")
                    });
                    if self.focused.as_ref().is_some_and(|focused| {
                        *focused == target || scope && focused.path.starts_with(&target.path)
                    }) {
                        self.remember_focus_scope(scene, layout);
                        self.set_focus_with_reason(None, xrack_runtime::FocusReason::Programmatic);
                    }
                }
                xrack_runtime::FocusAction::OpenChoice => {
                    if scene
                        .at_path(&target.path)
                        .is_some_and(|node| node.kind == "ComboBox")
                        && !self.choice_is_open_for(&target.path)
                        && self.open_choice(scene, layout, target)
                    {
                        self.dismiss_menu();
                    }
                }
                xrack_runtime::FocusAction::CloseChoice => {
                    if self.choice_is_open_for(&target.path) {
                        self.dismiss_choice();
                    }
                }
                xrack_runtime::FocusAction::Text(command) => {
                    let node = scene.interactive_at_path(&target.path)?;
                    let focused = self.focused.as_ref().is_some_and(|current| {
                        current == &target
                            || node.selectable_styled_text()
                                && current.path == target.path
                                && current.row_key == target.row_key
                    });
                    if !focused
                        || !self.is_text_editor(node, &target)
                        || self.editor.as_ref().is_some_and(TextEdit::is_composing)
                    {
                        return None;
                    }
                    self.sync_editor(scene);
                    return Some(command);
                }
                xrack_runtime::FocusAction::EditTableCell { row, column } => {
                    let row = usize::try_from(row).ok()?;
                    let column = usize::try_from(column).ok()?;
                    let cell = table_cell_target(scene, layout, &target.path, row, column)?;
                    self.begin_cell_edit(
                        scene,
                        layout,
                        cell,
                        xrack_runtime::FocusReason::Programmatic,
                    );
                }
                xrack_runtime::FocusAction::CancelTableEdit => {
                    if self.table_edit.as_ref().is_some_and(|edit| {
                        table_cell_row(scene, &edit.target.path)
                            .is_some_and(|(table, _)| table == target.path)
                    }) {
                        self.cancel_table_edit();
                    }
                }
            }
            None
        })();
        (command, scrolled)
    }

    pub fn apply_text_selection(&mut self, scene: &Node, command: TextCommand) {
        let Some(target) = &self.focused else {
            return;
        };
        let Some(node) = scene.interactive_at_path(&target.path) else {
            return;
        };
        let value = self.editor_value(node);
        let Some(mut editor) = self.editor.clone() else {
            return;
        };
        let (anchor, focus) = match command {
            TextCommand::SelectAll => (
                0,
                TextPosition {
                    offset: value.len(),
                    trailing: false,
                },
            ),
            TextCommand::SelectWord => {
                let word = TextEdit::word_at(&value, editor.position(&value), node.is_password());
                (
                    word.start,
                    TextPosition {
                        offset: word.end,
                        trailing: true,
                    },
                )
            }
            TextCommand::ClearSelection => {
                let caret = editor.position(&value);
                (caret.offset, caret)
            }
            TextCommand::SetSelection { anchor, focus } => {
                let offset = |byte: i64| {
                    usize::try_from(byte.max(0))
                        .unwrap_or(usize::MAX)
                        .min(value.len())
                };
                (
                    offset(anchor),
                    TextPosition {
                        offset: offset(focus),
                        trailing: false,
                    },
                )
            }
            _ => return,
        };
        editor.set_selection(&value, anchor, focus);
        self.editor = Some(editor);
        self.break_text_edit_group();
        self.painted_combo_completion = None;
    }

    pub fn accessibility_focus(&mut self, target: Target) {
        if target.focusable {
            self.tooltip.focus_source();
            self.set_focus_with_reason(Some(target), xrack_runtime::FocusReason::Accessibility);
        }
    }
    pub fn key_press(&mut self, key: ActivationKey) {
        self.cancel_delay();
        self.button_hold = None;
        self.key_activation_claimed = None;
        self.key_pressed = self.focused.clone().map(|t| (key, t));
        self.key_standard_button = None;
    }

    pub fn key_press_with_scene(&mut self, scene: &Node, key: ActivationKey) {
        self.key_press(key);
        if let Some(target) = self.focused.clone() {
            self.start_button_contact(scene, target, ButtonContactSource::Key(key));
        }
        self.key_standard_button = self
            .focused
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .and_then(Node::standard_button_kind);
    }
    pub fn key_release(&mut self, key: ActivationKey) -> Option<&'static str> {
        self.key_release_target(key).and_then(|target| target.id)
    }
    pub fn key_release_target(&mut self, key: ActivationKey) -> Option<Target> {
        if self.key_pressed.as_ref()?.0 != key {
            return None;
        }
        let (_, target) = self.key_pressed.take()?;
        self.finish_button_contact(
            ButtonContactSource::Key(key),
            self.focused.as_ref() == Some(&target),
        );
        if self.button_hold.as_ref().is_some_and(
            |repeat| matches!(repeat.source, DelaySource::Key(pressed) if pressed == key),
        ) {
            self.button_hold = None;
        }
        self.key_standard_button = None;
        let claimed = self.key_activation_claimed.take() == Some(key);
        if self
            .delay_hold
            .as_ref()
            .is_some_and(|hold| matches!(hold.source, DelaySource::Key(pressed) if pressed == key))
        {
            self.cancel_delay();
        }
        (!claimed && self.focused.as_ref() == Some(&target)).then_some(target)
    }

    pub fn key_release_with_scene(&mut self, scene: &Node, key: ActivationKey) -> Option<Target> {
        let kind = self.key_standard_button;
        let target = self.key_release_target(key)?;
        let node = target.node_in(scene)?;
        (node.standard_button_kind() == kind).then_some(target)
    }
    pub fn cancel_press(&mut self) {
        self.text_selection_scroll = None;
        self.editor_press = None;
        self.last_editor_click = None;
        self.button_press_events.extend(
            self.button_contacts
                .drain(..)
                .map(|contact| (contact, "canceled")),
        );
        for handle in self.published_button_presses.drain(..) {
            handle.publish(false);
        }
        if self.touch_table_drag_claimed() {
            self.pointer = None;
        }
        self.cancel_window_move();
        self.cancel_scroll_gesture();
        self.cancel_delay();
        self.button_hold = None;
        self.pointer_activation_claimed = false;
        self.key_activation_claimed = None;
        self.dialog_backdrop = None;
        self.pressed = None;
        self.press_origin = None;
        self.scroll_bar_press = None;
        self.pressed_radio = None;
        self.pressed_list_row = None;
        self.pressed_tree_indicator = false;
        self.pressed_table_column = None;
        self.pressed_body_column = None;
        self.pressed_control_part = None;
        self.pressed_standard_button = None;
        self.table_resize = None;
        self.table_row_resize = None;
        self.table_column_move = None;
        self.table_row_move = None;
        self.table_drag_selection = None;
        self.clear_split_drag();
        self.slider_offset = 0.0;
        self.slider_anchor = None;
        self.slider_fraction = None;
        self.clear_range_preview();
        self.dial_pointer = None;
        self.spin_pressed = None;
        self.key_pressed = None;
        self.range_keys.clear();
    }
    pub fn is_hovered(&self, path: &[usize]) -> bool {
        self.hovered.as_ref().is_some_and(|t| t.path == path)
    }
    pub fn is_focused(&self, path: &[usize]) -> bool {
        self.focused.as_ref().is_some_and(|t| t.path == path)
    }
    /// Publish pressed state only for observed buttons which still own a live input.
    pub fn publish_button_presses(&mut self, scene: &Node, layout: &LayoutNode) {
        let mut next = Vec::new();
        if !self.menu_open() && !self.choice_open() {
            let pointer = self
                .pressed
                .as_ref()
                .filter(|target| self.hovered.as_ref() == Some(*target));
            let key = self
                .key_pressed
                .as_ref()
                .map(|(_, target)| target)
                .filter(|target| self.focused.as_ref() == Some(*target));
            let touch = self.scroll_gesture.as_ref().and_then(|gesture| {
                let point = gesture.touch_point?;
                let Some(Hit::Target(target)) = &gesture.tap else {
                    return None;
                };
                (!gesture.motion.dragging
                    && self.target_at(scene, layout, point).as_ref() == Some(target))
                .then_some(target)
            });
            let scope = self.active_dialog(layout).unwrap_or_default();
            for target in [pointer, key, touch].into_iter().flatten() {
                if target.path.starts_with(scope)
                    && valid(target, scene, layout)
                    && let Some(handle) = target
                        .node_in(scene)
                        .and_then(|node| node.button_press_handle.as_ref())
                    && !next.contains(handle)
                {
                    next.push(handle.clone());
                }
            }
        }
        for old in &self.published_button_presses {
            if !next.contains(old) {
                old.publish(false);
            }
        }
        for handle in &next {
            handle.publish(true);
        }
        self.published_button_presses = next;
    }

    fn start_button_contact(&mut self, scene: &Node, target: Target, source: ButtonContactSource) {
        let Some(handle) = target
            .node_in(scene)
            .and_then(|node| node.button_press_handle.clone())
        else {
            return;
        };
        self.finish_button_contact(source, false);
        if handle.cancel_animation() {
            self.button_press_events.push((
                ButtonContact {
                    source,
                    target: target.clone(),
                    handle: handle.clone(),
                },
                "canceled",
            ));
        }
        self.button_press_events.push((
            ButtonContact {
                source,
                target: target.clone(),
                handle: handle.clone(),
            },
            "pressed",
        ));
        self.button_contacts.push(ButtonContact {
            source,
            target,
            handle,
        });
    }

    fn finish_button_contact(&mut self, source: ButtonContactSource, released: bool) {
        if let Some(index) = self
            .button_contacts
            .iter()
            .position(|contact| contact.source == source)
        {
            self.button_press_events.push((
                self.button_contacts.remove(index),
                if released { "released" } else { "canceled" },
            ));
        }
    }

    /// Publish the reactive state before delivering events to their original buttons.
    pub fn dispatch_button_press_events(
        &mut self,
        component: &mut impl Component,
        scene: &Node,
        layout: &LayoutNode,
    ) -> bool {
        self.publish_button_presses(scene, layout);
        if self.button_contacts.is_empty() && self.button_press_events.is_empty() {
            return false;
        }
        let scope = self.active_dialog(layout).unwrap_or_default().to_vec();
        let popup = self.menu_open() || self.choice_open();
        let focused = self.focused.as_ref();
        let pointer = self.pressed.as_ref();
        let key_pressed = self.key_pressed.as_ref();
        let gesture = self.scroll_gesture.is_some();
        self.button_contacts.retain(|contact| {
            let current = contact.target.path.starts_with(&scope)
                && !popup
                && valid(&contact.target, scene, layout)
                && contact
                    .target
                    .node_in(scene)
                    .and_then(|node| node.button_press_handle.as_ref())
                    == Some(&contact.handle)
                && match contact.source {
                    ButtonContactSource::Pointer => pointer == Some(&contact.target),
                    ButtonContactSource::Gesture => gesture,
                    ButtonContactSource::Key(key) => {
                        focused == Some(&contact.target)
                            && key_pressed.is_some_and(|(pressed, target)| {
                                key == *pressed && target == &contact.target
                            })
                    }
                };
            if !current {
                self.button_press_events.push((
                    ButtonContact {
                        source: contact.source,
                        target: contact.target.clone(),
                        handle: contact.handle.clone(),
                    },
                    "canceled",
                ));
            }
            current
        });
        let mut handled = false;
        for (contact, event) in std::mem::take(&mut self.button_press_events) {
            if contact
                .target
                .node_in(scene)
                .and_then(|node| node.button_press_handle.as_ref())
                == Some(&contact.handle)
            {
                handled |= component.dispatch_at(scene, &contact.target.path, event);
            }
        }
        handled
    }

    pub fn is_pressed(&self, path: &[usize]) -> bool {
        (self.pressed.as_ref().is_some_and(|t| t.path == path)
            && self.is_hovered(path)
            && self.pressed_list_row == self.hovered_list_row
            && self.pressed_standard_button == self.hovered_standard_button)
            || self
                .key_pressed
                .as_ref()
                .is_some_and(|(_, t)| t.path == path)
            || self.button_hold.as_ref().is_some_and(|repeat| {
                matches!(repeat.source, DelaySource::Touch(_)) && repeat.target.path == path
            })
    }
}

/// The inner-to-outer delivery path of one wheel event.
pub struct WheelRoute {
    deliveries: Vec<(Vec<usize>, xrack_runtime::WheelEvent)>,
}

fn local_handler_position(
    item: &LayoutNode,
    point: Point,
) -> Option<(xrack_runtime::Length, xrack_runtime::Length)> {
    let local = item.map_from_window(point)?;
    (item.enabled && item.bounds.contains(local) && item.contains_clip_point(point)).then(|| {
        (
            xrack_runtime::Length(f64::from(local.x - item.bounds.x)),
            xrack_runtime::Length(f64::from(local.y - item.bounds.y)),
        )
    })
}

impl WheelRoute {
    pub fn dispatch(
        self,
        component: &mut impl Component,
        scene: &Node,
    ) -> xrack_runtime::EventResult {
        for (path, event) in self.deliveries {
            if component.wheel_event_at(scene, &path, &event) == xrack_runtime::EventResult::Accept
            {
                return xrack_runtime::EventResult::Accept;
            }
        }
        xrack_runtime::EventResult::Reject
    }
}
