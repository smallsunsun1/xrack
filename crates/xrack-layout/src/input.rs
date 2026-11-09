use crate::{
    CalendarGeometry, ChoiceKey, ChoiceMenu, ComboCompletion, ComboGeometry, DialGeometry,
    EditAction, EditSource, LayoutNode, MenuEntry, MenuKey, MenuPanel, MenuShortcut,
    PageIndicatorGeometry, Point, ScrollBarGeometry, ScrollGestureEnd, ScrollGestureKind,
    SearchGeometry, ShortcutOutcome, SliderGeometry, SpinGeometry, SplitGeometry, SplitHandle,
    StackTransitions, TextEdit, TreeRowGeometry, TumblerScroll,
    choice::ChoicePopup,
    delay_button::{DelayHold, DelaySource},
    history::{EditHistories, EditorKey, Group, Record, TextHistory},
    menu::MenuPopup,
    modal,
    scroll_gesture::{DragMotion, FlingMotion},
    shortcut::{self, PendingShortcut},
    split::axis_point,
    swipe_delegate::SwipeDrag,
    tooltip::{self, Source as TooltipSource, TooltipState},
    tumbler::{TumblerDrag, TumblerWheel},
    view_cells::ViewCells,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use xrack_runtime::{
    CalendarAction, CalendarState, Component, ControlAction, Date, DialInputMode, DialogRole,
    DrawerEdge, ListMetrics, Node, Orientation, RangeAction, RowKey, RowSelection, SelectionTarget,
    SliderInterval, SliderRange, SpinAction, SpinDecimal, StandardButtonKind, SwipeSide,
    TableSelectionAction, TreeAction, TreeSelectionAction, Tumbler, combo_count, combo_editable,
    combo_index, decimal_separator, delay_millis, dialog_button_action, editable_table_cell,
    group_index, group_item, is_multiline_text_input, is_spin_box, is_text_input, is_toggle,
    is_window_overlay, page_indicator_count, page_indicator_index, resizable_table_column,
    search_suggestion, spin_editable, spin_text, spin_text_is_valid, text_input_value,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: Vec<usize>,
    pub id: Option<&'static str>,
    pub focusable: bool,
    pub row_key: Option<RowKey>,
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

pub struct RangeInput {
    pub path: Vec<usize>,
    pub action: RangeAction,
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
}

impl ControlInput {
    fn with_row_selection(mut self, scene: &Node, operation: RowSelection) -> Self {
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
}

#[derive(Clone, Debug)]
struct SplitDrag {
    target: Target,
    orientation: Orientation,
    after_pane: usize,
    grab_offset: f32,
}

#[derive(Clone, Debug)]
struct TableEdit {
    target: Target,
    original: String,
    draft: String,
    history: TextHistory,
}

#[derive(Clone, Debug)]
struct SpinDraft {
    text: String,
    mode: SpinDraftMode,
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
            Self::Integer(
                node.int("value")
                    .filter(|_| node.boolean("table-editor", false)),
            )
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
}

impl ScrollTarget {
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
            && scene.row_key_at(&self.path) == self.row_key)
            .then_some((node, item))
    }
}

#[derive(Clone, Debug)]
struct ScrollBarPress {
    target: ScrollTarget,
    grab: Option<f32>,
}

#[derive(Clone, Debug)]
struct ScrollRoute {
    scope: Vec<usize>,
    scope_row: Option<RowKey>,
    axes: [Option<ScrollTarget>; 2],
}

#[derive(Clone, Debug)]
struct ScrollGesture {
    route: ScrollRoute,
    motion: DragMotion,
    tap: Option<Hit>,
    popup: (bool, bool),
    drawer: Option<DrawerSwipe>,
    pages: Vec<PageSwipe>,
    tumbler: Option<TumblerDrag>,
    swipe_delegate: Option<SwipeDrag>,
    delay_touch: bool,
}

#[derive(Clone, Debug)]
struct PageSwipe {
    target: ScrollTarget,
    current: usize,
    previous: Option<usize>,
    next: Option<usize>,
    start: Point,
    threshold: f32,
}

impl PageSwipe {
    fn movement(&self, point: Point) -> (f32, f32) {
        let x = self.start.x - point.x;
        let y = self.start.y - point.y;
        match self.target.orientation {
            Orientation::Horizontal => (x, y.abs()),
            Orientation::Vertical => (y, x.abs()),
        }
    }

    fn destination(&self, forward: bool) -> Option<usize> {
        if forward { self.next } else { self.previous }
    }

    fn action(self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<ControlInput> {
        let path = &self.target.path;
        let view = scene.interactive_at_path(path)?;
        let item = layout.at_path(path)?;
        let (distance, across) = self.movement(point);
        let index = self.destination(distance > 0.0)?;
        (distance.abs() >= self.threshold
            && distance.abs() > across
            && view.kind == "SwipeView"
            && view.boolean("interactive", true)
            && view.orientation() == self.target.orientation
            && item.visible
            && item.enabled
            && scene.row_key_at(path) == self.target.row_key
            && group_index(view) == Some(self.current)
            && group_item(view, index).is_some())
        .then_some(ControlInput {
            path: self.target.path,
            action: ControlAction::Select(index),
        })
    }
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

#[derive(Clone, Debug)]
struct DrawerSwipe {
    path: Vec<usize>,
    row_key: Option<RowKey>,
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
}

#[derive(Clone, Debug, Default)]
pub struct Interaction {
    pub hovered: Option<Target>,
    pub focused: Option<Target>,
    pub pressed: Option<Target>,
    drag: Option<crate::drag::DragSession>,
    file_drag: crate::file_drop::FileDrag,
    hovered_scroll_bar: Option<ScrollTarget>,
    scroll_bar_press: Option<ScrollBarPress>,
    scroll_gesture: Option<ScrollGesture>,
    tumbler_wheel: Option<TumblerWheel>,
    scroll_fling: Option<ScrollFling>,
    key_pressed: Option<(ActivationKey, Target)>,
    key_standard_button: Option<StandardButtonKind>,
    delay_hold: Option<DelayHold>,
    delay_pointer_claimed: bool,
    delay_key_claimed: Option<ActivationKey>,
    range_keys: BTreeMap<SliderKey, Target>,
    slider_offset: f64,
    slider_anchor: Option<f64>,
    dial_pointer: Option<Point>,
    spin_pressed: Option<SpinAction>,
    spin_draft: Option<SpinDraft>,
    choice: Option<ChoicePopup>,
    search_query: Option<String>,
    painted_combo_completion: Option<ComboCompletion>,
    dismissed_combo_draft: Option<String>,
    menu: Option<MenuPopup>,
    pending_shortcut: Option<PendingShortcut>,
    pointer: Option<Point>,
    press_origin: Option<Point>,
    tooltip: TooltipState,
    hovered_radio: Option<String>,
    pressed_radio: Option<String>,
    hovered_list_row: Option<(usize, Option<&'static str>)>,
    pressed_list_row: Option<(usize, Option<&'static str>)>,
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
    hovered_table_grip: Option<(usize, f32)>,
    table_resize: Option<TableResize>,
    split_drag: Option<SplitDrag>,
    table_edit: Option<TableEdit>,
    last_table_click: Option<(Target, Instant)>,
    pub editor: Option<TextEdit>,
    history: EditHistories,
    scroll_offsets: BTreeMap<Vec<usize>, f32>,
    horizontal_scroll_offsets: BTreeMap<Vec<usize>, f32>,
    scroll_selections: BTreeMap<Vec<usize>, (Option<usize>, usize)>,
    text_offsets: BTreeMap<Vec<usize>, (Option<RowKey>, Point)>,
    dialogs: Vec<DialogFocus>,
    stack_focus: BTreeMap<StackFocusKey, Target>,
    transitioning_stacks: BTreeSet<Vec<usize>>,
    pointer_on_stack_slide: bool,
    dialog_backdrop: Option<Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Hit {
    None,
    Blocked(Vec<usize>),
    StackSlide,
    Target(Target),
    ControlPart(Target, ControlAction),
    ScrollBar(ScrollTarget),
}

fn focusable(node: &Node) -> bool {
    let kind = node.kind;
    if kind == "GroupBox" {
        return node.group_checkable();
    }
    if kind == "PageIndicator" {
        return node.boolean("interactive", false) && page_indicator_count(node) != 0;
    }
    if kind == "Tumbler" {
        return Tumbler::from_node(node).selected().is_some();
    }
    matches!(
        kind,
        "Button"
            | "ToolButton"
            | "DelayButton"
            | "DialogButton"
            | "Dialog"
            | "Drawer"
            | "PopupWindow"
            | "Slider"
            | "Dial"
            | "RangeThumb"
            | "SpinBox"
            | "DoubleSpinBox"
            | "ComboBox"
            | "Calendar"
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
}

fn interactive(node: &Node) -> bool {
    matches!(node.kind, "TouchArea" | "ListRow" | "RangeSlider") || focusable(node)
}

fn list_row(scene: &Node, target: &Target) -> Option<(usize, Option<&'static str>)> {
    let (&slot, parent) = target.path.split_last()?;
    let list = scene.at_path(parent)?;
    let index = list.list?.first_row + slot;
    (list.children.get(slot)?.kind == "ListRow").then_some((index, list.id))
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

fn tree_cell_row<'a>(scene: &Node, path: &'a [usize]) -> Option<(&'a [usize], usize)> {
    let cell = scene.at_path(path)?;
    if cell.kind != "TreeCell" && !cell.boolean("table-editor", false) {
        return None;
    }
    let mut node = scene;
    for (depth, &index) in path.iter().enumerate() {
        if node.kind == "TreeView" {
            return (path.get(depth + 1) == Some(&0)
                && node.children.get(index)?.kind == "ListRow")
                .then_some((&path[..depth], index));
        }
        node = node.children.get(index)?;
    }
    None
}

pub(crate) fn scroll_selection(node: &Node) -> Option<usize> {
    if node.kind == "TabBar" {
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
    point: Point,
    path: &mut Vec<usize>,
    transitioning_stacks: &BTreeSet<Vec<usize>>,
) -> Hit {
    if node.kind == "Tab"
        && layout
            .tab_header
            .is_some_and(|header| layout.clip.intersect(header).contains(point))
    {
        return if layout.enabled {
            Hit::Target(Target {
                path: path.clone(),
                id: node.id,
                focusable: true,
                row_key: None,
            })
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if !layout.visible || !layout.clip.intersect(layout.bounds).contains(point) {
        return Hit::None;
    }
    if transitioning_stacks.contains(path) {
        return Hit::StackSlide;
    }
    if node.group_checkable() && layout.group_header(node).contains(point) {
        return if layout.enabled {
            Hit::Target(Target {
                path: path.clone(),
                id: node.id,
                focusable: true,
                row_key: None,
            })
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if let Some(action) = layout.tumbler.as_ref().and_then(|wheel| wheel.hit(point)) {
        return if layout.enabled {
            Hit::ControlPart(
                Target {
                    path: path.clone(),
                    id: node.id,
                    focusable: true,
                    row_key: None,
                },
                action,
            )
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "SearchField"
        && let Some(action) = SearchGeometry::new(layout.bounds).action_at(node, point)
    {
        return if layout.enabled {
            Hit::ControlPart(
                Target {
                    path: path.clone(),
                    id: node.id,
                    focusable: true,
                    row_key: None,
                },
                action,
            )
        } else {
            Hit::Blocked(path.clone())
        };
    }
    if node.kind == "Calendar" {
        let action = CalendarGeometry::new(node, layout.bounds).action_at(point);
        return if layout.enabled
            && let Some(action) = action
        {
            Hit::ControlPart(
                Target {
                    path: path.clone(),
                    id: node.id,
                    focusable: true,
                    row_key: None,
                },
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
        let Some(index) = PageIndicatorGeometry::new(node, layout.bounds)
            .and_then(|geometry| geometry.marker_at(point))
        else {
            return Hit::None;
        };
        return if layout.enabled {
            Hit::ControlPart(
                Target {
                    path: path.clone(),
                    id: node.id,
                    focusable: true,
                    row_key: None,
                },
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
            Hit::ControlPart(
                Target {
                    path: path.clone(),
                    id: node.id,
                    focusable: true,
                    row_key: None,
                },
                action,
            )
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
            })
        } else {
            Hit::Blocked(path.clone())
        };
    }
    for (i, (child, child_layout)) in node.children.iter().zip(&layout.children).enumerate().rev() {
        if is_window_overlay(child.kind) {
            continue;
        }
        path.push(i);
        let found = hit(child, child_layout, point, path, transitioning_stacks);
        path.pop();
        if !matches!(found, Hit::None) {
            return found;
        }
    }
    if matches!(node.kind, "Tab" | "GroupBox")
        || is_window_overlay(node.kind)
        || !interactive(node)
        || node.kind == "TreeCell" && !editable_table_cell(node)
    {
        return Hit::None;
    }
    if !layout.enabled {
        return Hit::Blocked(path.clone());
    }
    Hit::Target(Target {
        path: path.clone(),
        id: node.id,
        focusable: focusable(node),
        row_key: None,
    })
}

pub(crate) fn valid(target: &Target, scene: &Node, layout: &LayoutNode) -> bool {
    let Some(node) = scene.at_path(&target.path) else {
        return false;
    };
    let Some(item) = layout.at_path(&target.path) else {
        return false;
    };
    node.id == target.id
        && target.focusable == focusable(node)
        && target.row_key == scene.row_key_at(&target.path)
        && eligible(node, item)
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

fn moved_row_target(target: &Target, scene: &Node, layout: &LayoutNode) -> Option<Target> {
    let key = target.row_key?;
    for depth in (0..target.path.len()).rev() {
        let Some(node) = scene.at_path(&target.path[..depth]) else {
            continue;
        };
        if !matches!(
            node.kind,
            "ListView" | "GridView" | "StandardListView" | "TableView" | "TreeView"
        ) {
            continue;
        }
        let slot = node
            .children
            .iter()
            .position(|row| row.row_key == Some(key))?;
        let mut moved = target.clone();
        moved.path[depth] = slot;
        return valid(&moved, scene, layout).then_some(moved);
    }
    None
}

fn eligible(node: &Node, layout: &LayoutNode) -> bool {
    let hitbox = if node.kind == "Tab" {
        layout.tab_header
    } else if layout.visible && node.kind == "GroupBox" {
        Some(layout.group_header(node))
    } else if layout.visible {
        Some(layout.bounds)
    } else {
        None
    };
    layout.enabled
        && interactive(node)
        && hitbox.is_some_and(|bounds| {
            let clipped = layout.clip.intersect(bounds);
            clipped.width > 0.0 && clipped.height > 0.0
        })
}

fn scroll_target_visible(item: &LayoutNode) -> bool {
    let clip = item.bounds.intersect(item.clip);
    item.visible && item.enabled && clip.width > 0.0 && clip.height > 0.0
}

fn first_editable_tree_cell<'a>(
    node: &'a Node,
    layout: &LayoutNode,
    path: &mut Vec<usize>,
) -> Option<&'a Node> {
    if !layout.enabled || !layout.visible {
        return None;
    }
    if editable_table_cell(node) && eligible(node, layout) {
        return Some(node);
    }
    for (index, (child, item)) in node.children.iter().zip(&layout.children).enumerate() {
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
) -> impl Iterator<Item = (usize, Target)> {
    let scrollable_tabs = node.kind == "TabBar" && scroll_target_visible(layout);
    node.children
        .iter()
        .zip(&layout.children)
        .enumerate()
        .filter_map(move |(index, (child, item))| {
            let usable = if scrollable_tabs {
                item.visible && item.enabled && item.bounds.width > 0.0 && item.bounds.height > 0.0
            } else {
                eligible(child, item)
            };
            if group_item(node, index).is_none() || !usable {
                return None;
            }
            let mut child_path = path.to_vec();
            child_path.push(index);
            Some((
                index,
                Target {
                    path: child_path,
                    id: child.id,
                    focusable: true,
                    row_key: None,
                },
            ))
        })
}

fn group_focus(node: &Node, layout: &LayoutNode, path: &[usize]) -> Option<Target> {
    let selected = group_index(node);
    group_targets(node, layout, path)
        .min_by_key(|(index, _)| Some(*index) != selected)
        .map(|(_, target)| target)
}

fn focus_targets(scene: &Node, layout: &LayoutNode, scope: &[usize]) -> Vec<Target> {
    fn collect(node: &Node, layout: &LayoutNode, path: &mut Vec<usize>, out: &mut Vec<Target>) {
        if !layout.visible || !layout.enabled {
            return;
        }
        let clip = layout.clip.intersect(layout.bounds);
        if clip.width <= 0.0 || clip.height <= 0.0 {
            return;
        }
        if matches!(node.kind, "TabWidget" | "TabBar" | "RadioGroup") {
            if let Some(target) = group_focus(node, layout, path) {
                out.push(target);
            }
            if node.kind != "TabWidget" {
                return;
            }
        } else if !matches!(node.kind, "Tab" | "TreeCell")
            && !is_window_overlay(node.kind)
            && focusable(node)
            && (node.kind != "GroupBox" || eligible(node, layout))
        {
            out.push(Target {
                path: path.clone(),
                id: node.id,
                focusable: true,
                row_key: None,
            });
        }
        let mut visit = |index: usize| {
            if let Some((child, item)) = node.children.get(index).zip(layout.children.get(index))
                && !is_window_overlay(child.kind)
            {
                path.push(index);
                collect(child, item, path, out);
                path.pop();
            }
        };
        if node.kind == "TreeView" {
            if let Some(slot) = current_list_slot(node) {
                visit(slot);
            }
        } else if layout.split.is_some() {
            let count = node.split_pane_count();
            for pane in 0..count {
                visit(pane);
                visit(count + pane);
            }
        } else if !node.boolean("__standard-table", false) {
            for index in crate::child_reading_order(node) {
                visit(index);
            }
        }
    }
    let mut targets = Vec::new();
    if let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) {
        collect(node, item, &mut scope.to_vec(), &mut targets);
        if targets.is_empty() && is_window_overlay(node.kind) && eligible(node, item) {
            targets.push(Target {
                path: scope.to_vec(),
                id: node.id,
                focusable: true,
                row_key: None,
            });
        }
    }
    for target in &mut targets {
        target.row_key = scene.row_key_at(&target.path);
    }
    targets
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
        eligible(node, layout.at_path(path)?).then(|| Target {
            path: path.to_vec(),
            id: node.id,
            focusable: true,
            row_key: scene.row_key_at(path),
        })
    })
}

fn slider_target<'a, 'b>(
    target: &'a Target,
    scene: &'b Node,
    layout: &LayoutNode,
) -> Option<(&'a [usize], &'b Node)> {
    let leaf = scene.at_path(&target.path)?;
    if matches!(leaf.kind, "Slider" | "RangeSlider" | "Dial") {
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
            let sibling = Target {
                id: child.id,
                row_key: scene.row_key_at(&path),
                focusable: true,
                path,
            };
            valid(&sibling, scene, layout).then_some((slot, sibling))
        })
        .collect();
    let current = choices.iter().position(|(slot, _)| *slot == index)?;
    let step = if key == MenuKey::Left {
        choices.len() - 1
    } else {
        1
    };
    Some(choices[(current + step) % choices.len()].1.clone())
}

impl Interaction {
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
        let competing = match self.hit_at(scene, layout, point) {
            Hit::Target(target) => scene.at_path(&target.path).is_some_and(|node| {
                is_text_input(node.kind)
                    || combo_editable(node)
                    || is_spin_box(node.kind)
                    || matches!(
                        node.kind,
                        "Slider"
                            | "Dial"
                            | "RangeSlider"
                            | "SplitHandle"
                            | "TableCell"
                            | "TreeCell"
                            | "Tumbler"
                            | "SwipeDelegate"
                    )
            }),
            Hit::None => false,
            _ => true,
        };
        self.drag = if competing {
            None
        } else {
            crate::drag::DragSession::arm(self.drag_context(scene, layout, point), point)
        };
    }

    pub fn drag_motion(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        requested: Option<xrack_runtime::DragAction>,
    ) -> crate::DragUpdate {
        let Some(mut drag) = self.drag.take() else {
            return crate::DragUpdate::default();
        };
        let active = drag.active();
        let (update, keep) = drag.motion(self.drag_context(scene, layout, point), point, requested);
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
        if self.focused != focus {
            self.cancel_shortcut_sequence();
            self.cancel_delay();
            self.history.break_group();
            self.editor = None;
            self.spin_draft = None;
            self.table_edit = None;
            self.last_table_click = None;
            self.choice = None;
            self.search_query = None;
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

    pub fn menu_panels<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Vec<MenuPanel<'a>> {
        self.menu
            .as_ref()
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
        let path = self
            .menu
            .as_mut()?
            .accessibility_entry(scene, layout, trigger, entry, activate)?;
        self.close_menu(scene, layout);
        Some(ControlInput {
            path,
            action: ControlAction::ActivateMenuItem,
        })
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
    ) {
        if self
            .menu
            .as_mut()
            .and_then(|popup| popup.accessibility_close(scene, layout, trigger, path))
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
        match self.hit_at(scene, layout, point) {
            Hit::Target(target) | Hit::ControlPart(target, _) => Some(target),
            _ => None,
        }
    }

    fn hit_at(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Hit {
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
        );
        if matches!(result, Hit::None) && node.kind == "PopupWindow" && item.bounds.contains(point)
        {
            result = Hit::Blocked(scope.to_vec());
        }
        match &mut result {
            Hit::Target(target) | Hit::ControlPart(target, _) => {
                target.row_key = scene.row_key_at(&target.path)
            }
            Hit::ScrollBar(target) => target.row_key = scene.row_key_at(&target.path),
            _ => {}
        }
        result
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
        let Some(popup) = MenuPopup::open(target.clone(), target.path.clone(), None, scene, layout)
        else {
            return false;
        };
        self.dismiss_choice();
        self.set_focus(Some(target));
        self.cancel_shortcut_sequence();
        self.menu = Some(popup);
        true
    }

    pub fn open_context_menu(&mut self, scene: &Node, layout: &LayoutNode, keyboard: bool) -> bool {
        let target = if keyboard {
            self.focused.clone()
        } else {
            self.pointer
                .and_then(|point| self.target_at(scene, layout, point))
        };
        let Some(target) = target else {
            return false;
        };
        let scope = self.active_dialog(layout).unwrap_or_default();
        if !target.path.starts_with(scope) {
            return false;
        }
        for depth in (scope.len()..=target.path.len()).rev() {
            let path = &target.path[..depth];
            let Some(area) = scene
                .at_path(path)
                .filter(|node| node.kind == "ContextMenuArea")
            else {
                continue;
            };
            let Some(index) = area
                .children
                .iter()
                .position(|child| child.kind == "Menu" && child.is_visible())
            else {
                continue;
            };
            let point = if keyboard {
                let Some(item) = layout.at_path(&target.path) else {
                    return false;
                };
                let anchor = item.bounds.intersect(item.clip);
                Point {
                    x: anchor.x,
                    y: anchor.y + anchor.height,
                }
            } else {
                self.pointer.unwrap()
            };
            let trigger = Target {
                path: path.to_vec(),
                id: area.id,
                focusable: true,
                row_key: scene.row_key_at(path),
            };
            let mut path = path.to_vec();
            path.push(index);
            let Some(popup) = MenuPopup::open(trigger, path, Some(point), scene, layout) else {
                return false;
            };
            self.dismiss_choice();
            self.cancel_press();
            self.cancel_shortcut_sequence();
            self.menu = Some(popup);
            return true;
        }
        false
    }

    pub fn menu_press(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        if !self.menu_open() {
            return false;
        }
        self.cancel_press();
        if let Some(point) = self.pointer {
            if self.menu.as_mut().unwrap().press(scene, layout, point) {
                return true;
            }
            if let Some(header) = self.target_at(scene, layout, point)
                && !self.menu_header_open(&header.path)
                && self.open_menu_bar(scene, layout, header)
            {
                return true;
            }
        }
        self.close_menu(scene, layout);
        true
    }

    pub fn menu_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let path = self.menu.as_mut()?.release(scene, layout, self.pointer?)?;
        self.close_menu(scene, layout);
        Some(ControlInput {
            path,
            action: ControlAction::ActivateMenuItem,
        })
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
        if key == MenuKey::Right && popup.enter_submenu(scene, layout) {
            return None;
        }
        if matches!(key, MenuKey::Left | MenuKey::Right)
            && popup.at_root()
            && popup.bar_header().is_some()
            && let Some(next) = sibling_menu(scene, layout, &popup.trigger, key)
        {
            self.open_menu_bar(scene, layout, next);
            return None;
        }
        let path = popup.key(scene, layout, key)?;
        self.close_menu(scene, layout);
        Some(ControlInput {
            path,
            action: ControlAction::ActivateMenuItem,
        })
    }

    pub fn scroll_offsets(&self) -> &BTreeMap<Vec<usize>, f32> {
        &self.scroll_offsets
    }

    pub fn horizontal_scroll_offsets(&self) -> &BTreeMap<Vec<usize>, f32> {
        &self.horizontal_scroll_offsets
    }

    pub(crate) fn scroll_selection_changed(
        &self,
        node: &Node,
        path: &[usize],
        columns: usize,
    ) -> bool {
        self.scroll_selections
            .get(path)
            .is_some_and(|previous| *previous != (scroll_selection(node), columns))
    }

    pub fn pointer_position(&self) -> Option<Point> {
        self.pointer
    }

    pub fn pointer_press_origin(&self) -> Option<Point> {
        self.press_origin
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
        if !offset.is_finite()
            || self
                .active_dialog(layout)
                .is_some_and(|scope| !path.starts_with(scope))
        {
            return false;
        }
        let Some(scroll) = layout
            .at_path(path)
            .filter(|item| scroll_target_visible(item))
            .and_then(|item| item.scroll_on(orientation))
        else {
            return false;
        };
        self.cancel_scroll_gesture();
        let offsets = match orientation {
            Orientation::Horizontal => &self.horizontal_scroll_offsets,
            Orientation::Vertical => &self.scroll_offsets,
        };
        let previous = offsets
            .get(path)
            .copied()
            .unwrap_or(scroll.offset)
            .clamp(0.0, scroll.max_offset);
        let delta = offset.clamp(0.0, scroll.max_offset) - previous;
        self.consume_scroll(layout, path, path.len(), delta, orientation)
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
        let (node, item) = target.resolve(scene, layout)?;
        item.scroll_bars(node)
            .find(|bar| bar.orientation == target.orientation && bar.scrollable())
    }

    /// Returns Some when the scroll bar captures the press, with whether it scrolled.
    pub fn scroll_bar_press(&mut self, scene: &Node, layout: &LayoutNode) -> Option<bool> {
        if self.menu_open() || self.choice_open() {
            return None;
        }
        let target = self.hovered_scroll_bar.clone()?;
        let point = self.pointer?;
        let bar = self.scroll_bar_for_target(scene, layout, &target)?;
        if !bar.track.contains(point) {
            return None;
        }
        self.cancel_press();
        self.tooltip.dismiss();
        let grab = bar.grab(point);
        let moved =
            grab.is_none() && self.set_scroll_bar_offset(&target, bar, bar.page_toward(point));
        self.scroll_bar_press = Some(ScrollBarPress { target, grab });
        Some(moved)
    }

    /// Continue handling movement after the cursor leaves a captured thumb's track.
    pub fn scroll_bar_drag(&mut self, scene: &Node, layout: &LayoutNode) -> Option<bool> {
        let press = self.scroll_bar_press.as_ref()?;
        let (Some(grab), Some(point)) = (press.grab, self.pointer) else {
            return Some(false);
        };
        let target = press.target.clone();
        let Some(bar) = self.scroll_bar_for_target(scene, layout, &target) else {
            self.scroll_bar_press.as_mut().unwrap().grab = None;
            return Some(false);
        };
        Some(self.set_scroll_bar_offset(&target, bar, bar.drag(point, grab)))
    }

    fn set_scroll_bar_offset(
        &mut self,
        target: &ScrollTarget,
        bar: ScrollBarGeometry,
        offset: f32,
    ) -> bool {
        if !offset.is_finite() {
            return false;
        }
        let offsets = match bar.orientation {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        let current = offsets
            .get(&target.path)
            .copied()
            .unwrap_or(bar.scroll.offset);
        if current == offset {
            return false;
        }
        offsets.insert(target.path.clone(), offset);
        true
    }

    pub fn finish_scroll_bar(&mut self) -> bool {
        self.scroll_bar_press.take().is_some()
    }

    pub fn scroll_bar_highlighted(&self, path: &[usize], orientation: Orientation) -> bool {
        let matches =
            |target: &ScrollTarget| target.path == path && target.orientation == orientation;
        self.hovered_scroll_bar.as_ref().is_some_and(matches)
            || self
                .scroll_bar_press
                .as_ref()
                .is_some_and(|press| press.grab.is_some() && matches(&press.target))
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
        let path = Self::scroll_path(layout, point, scope, orientation)?;
        Some(self.consume_scroll(layout, &path, scope_len, delta, orientation))
    }

    fn scroll_path(
        layout: &LayoutNode,
        point: Point,
        scope: &[usize],
        orientation: Orientation,
    ) -> Option<Vec<usize>> {
        fn target(
            layout: &LayoutNode,
            point: Point,
            path: &mut Vec<usize>,
            orientation: Orientation,
        ) -> bool {
            if !layout.visible
                || !layout.enabled
                || !layout.clip.intersect(layout.bounds).contains(point)
            {
                return false;
            }
            for (index, child) in layout.children.iter().enumerate().rev() {
                path.push(index);
                if target(child, point, path, orientation) {
                    return true;
                }
                path.pop();
            }
            layout
                .scroll_on(orientation)
                .is_some_and(|scroll| scroll.max_offset > 0.0)
        }

        let mut path = scope.to_vec();
        target(layout.at_path(scope)?, point, &mut path, orientation).then_some(path)
    }

    fn consume_scroll(
        &mut self,
        layout: &LayoutNode,
        path: &[usize],
        scope_len: usize,
        delta: f32,
        orientation: Orientation,
    ) -> bool {
        let offsets = match orientation {
            Orientation::Horizontal => &mut self.horizontal_scroll_offsets,
            Orientation::Vertical => &mut self.scroll_offsets,
        };
        let mut delta = f64::from(delta);
        let mut moved = false;
        // Only the portion the inner viewport could not consume reaches its ancestors.
        for depth in (scope_len..=path.len()).rev() {
            let path = &path[..depth];
            let Some(scroll) = layout
                .at_path(path)
                .and_then(|item| item.scroll_on(orientation))
            else {
                continue;
            };
            let current = offsets
                .get(path)
                .copied()
                .unwrap_or(scroll.offset)
                .clamp(0.0, scroll.max_offset);
            let next = (f64::from(current) + delta).clamp(0.0, f64::from(scroll.max_offset)) as f32;
            if next == current {
                continue;
            }
            offsets.insert(path.to_vec(), next);
            moved = true;
            let remaining = delta - (f64::from(next) - f64::from(current));
            delta = if delta > 0.0 {
                remaining.max(0.0)
            } else {
                remaining.min(0.0)
            };
            if delta == 0.0 {
                break;
            }
        }
        moved
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
        let interrupted = self.scroll_fling.take().is_some();
        if !point.x.is_finite() || !point.y.is_finite() {
            return false;
        }
        let popup = (self.menu_open(), self.choice_open());
        let scope = self.pointer_scope(layout, point).to_vec();
        let hit = self.hit_at(scene, layout, point);
        if !popup.0 && !popup.1 && matches!(hit, Hit::StackSlide) {
            return false;
        }
        let tumbler = match &hit {
            Hit::Target(target) | Hit::ControlPart(target, _) if !popup.0 && !popup.1 => scene
                .at_path(&target.path)
                .zip(layout.at_path(&target.path))
                .and_then(|(node, item)| {
                    (node.kind == "Tumbler")
                        .then(|| TumblerDrag::new(target.clone(), node, item, point))
                        .flatten()
                }),
            _ => None,
        };
        if kind == ScrollGestureKind::MouseTumbler && tumbler.is_none() {
            return false;
        }
        let swipe_delegate = match &hit {
            Hit::Target(target)
                if !popup.0 && !popup.1 && kind != ScrollGestureKind::MouseTumbler =>
            {
                scene
                    .at_path(&target.path)
                    .zip(layout.at_path(&target.path))
                    .and_then(|(node, item)| SwipeDrag::new(target.clone(), node, item, point))
            }
            _ => None,
        };
        if kind == ScrollGestureKind::MouseControl && tumbler.is_none() && swipe_delegate.is_none()
        {
            return false;
        }
        let pages = if popup.0 || popup.1 {
            Vec::new()
        } else {
            Self::page_swipes(scene, layout, point, &scope, &hit)
        };
        let drawer = if popup.0 || popup.1 || self.popup_at(layout, point).is_some() {
            None
        } else {
            self.drawer_swipe(scene, layout, point)
        };
        let axes = [Orientation::Horizontal, Orientation::Vertical].map(|orientation| {
            if popup.0 || popup.1 {
                return None;
            }
            let path = Self::scroll_path(layout, point, &scope, orientation)?;
            let row_key = scene.row_key_at(&path);
            Some(ScrollTarget {
                path,
                orientation,
                row_key,
            })
        });
        if kind == ScrollGestureKind::MousePan
            && drawer.is_none()
            && pages.is_empty()
            && tumbler.is_none()
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
        let tap = (kind != ScrollGestureKind::MousePan && !interrupted).then_some(hit);
        self.cancel_press();
        self.pointer_leave();
        let delay_touch = delay_target.is_some_and(|target| {
            self.set_focus(Some(target.clone()));
            self.begin_delay(scene, layout, target, DelaySource::Touch(point), now)
        });
        self.scroll_gesture = Some(ScrollGesture {
            route: ScrollRoute {
                scope_row: scene.row_key_at(&scope),
                scope,
                axes,
            },
            motion: DragMotion::new(point, now),
            tap,
            popup,
            drawer,
            pages,
            tumbler,
            swipe_delegate,
            delay_touch,
        });
        true
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
                    || !item.bounds.intersect(item.clip).contains(point)
                {
                    return None;
                }
                let current = group_index(view)?;
                let previous = swipe_neighbor(view, current, false);
                let next = swipe_neighbor(view, current, true);
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
                    },
                    current,
                    previous,
                    next,
                    start: point,
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
            layout
                .at_path(path)
                .and_then(|item| item.scroll_on(target.orientation))
                .is_some_and(|scroll| {
                    let offset = offsets
                        .get(path)
                        .copied()
                        .unwrap_or(scroll.offset)
                        .clamp(0.0, scroll.max_offset);
                    if forward {
                        offset < scroll.max_offset
                    } else {
                        offset > 0.0
                    }
                })
        })
    }

    fn drawer_swipe(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<DrawerSwipe> {
        let active = self.active_dialog(layout);
        let viewport = layout.bounds;
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
    }

    fn scroll_captured(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        route: &ScrollRoute,
        delta: Point,
    ) -> [bool; 2] {
        if !self.scroll_route_is_current(scene, layout, route) {
            return [false; 2];
        }
        std::array::from_fn(|index| {
            let movement = [delta.x, delta.y][index];
            let Some(target) = &route.axes[index] else {
                return false;
            };
            let path = &target.path;
            if movement == 0.0 || target.resolve(scene, layout).is_none() {
                return false;
            }
            self.consume_scroll(
                layout,
                path,
                route.scope.len(),
                movement,
                target.orientation,
            )
        })
    }

    pub fn move_scroll_gesture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> bool {
        let Some(mut gesture) = self.scroll_gesture.take() else {
            return false;
        };
        if !self.scroll_route_is_current(scene, layout, &gesture.route)
            || gesture.popup != (self.menu_open(), self.choice_open())
        {
            self.cancel_touch_delay();
            return gesture.tumbler.is_some_and(|drag| drag.offset != 0.0)
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
                .is_some_and(|item| item.bounds.intersect(item.clip).contains(point))
            {
                self.cancel_delay();
            }
        }
        let was_dragging = gesture.motion.dragging;
        let scrolled = gesture.motion.update(point, now).is_some_and(|delta| {
            if gesture.delay_touch {
                self.cancel_touch_delay();
            }
            if let Some(drag) = &mut gesture.tumbler {
                if was_dragging || (point.y - drag.start.y).abs() > (point.x - drag.start.x).abs() {
                    return drag.update(scene, layout, point);
                }
                gesture.tumbler = None;
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
                && gesture
                    .swipe_delegate
                    .as_ref()
                    .is_some_and(|swipe| !swipe.accepts_direction(point))
            {
                gesture.swipe_delegate = None;
            }
            if let Some(swipe) = &mut gesture.swipe_delegate {
                return swipe.update(scene, layout, point);
            }
            if !was_dragging {
                let selected = gesture.pages.iter().position(|page| {
                    let (distance, across) = page.movement(point);
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
            if !gesture.pages.is_empty() {
                return false;
            }
            self.scroll_captured(scene, layout, &gesture.route, delta)
                .into_iter()
                .any(|moved| moved)
        });
        self.scroll_gesture = Some(gesture);
        scrolled
    }

    pub fn end_scroll_gesture(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> ScrollGestureEnd {
        if !point.x.is_finite() || !point.y.is_finite() {
            self.cancel_scroll_gesture();
            return ScrollGestureEnd::default();
        }
        let scrolled = self.move_scroll_gesture(scene, layout, point, now);
        let Some(gesture) = self.scroll_gesture.take() else {
            return ScrollGestureEnd {
                scrolled,
                ..ScrollGestureEnd::default()
            };
        };
        let tap = !gesture.delay_touch
            && !gesture.motion.dragging
            && gesture
                .tap
                .is_some_and(|hit| hit == self.hit_at(scene, layout, point));
        if gesture.delay_touch && !gesture.motion.dragging {
            self.cancel_touch_delay();
            return ScrollGestureEnd {
                tap: false,
                scrolled,
                control: None,
            };
        }
        if let Some(drag) = gesture.tumbler {
            let had_preview = drag.offset != 0.0;
            let target = drag.target.clone();
            let control = if gesture.motion.dragging {
                drag.finish(scene, layout)
            } else {
                None
            };
            if control.is_some() {
                self.set_focus(Some(target));
            }
            return ScrollGestureEnd {
                tap,
                scrolled: scrolled || had_preview,
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
            return ScrollGestureEnd {
                tap,
                scrolled,
                control: page.action(scene, layout, point),
            };
        }
        let axes = gesture.route.axes.each_ref().map(|axis| {
            axis.as_ref()
                .is_some_and(|target| target.resolve(scene, layout).is_some())
        });
        self.scroll_fling = gesture.motion.finish(now, axes).map(|motion| ScrollFling {
            route: gesture.route,
            motion,
        });
        ScrollGestureEnd {
            tap,
            scrolled,
            control: None,
        }
    }

    pub fn advance_scroll_motion(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some(mut fling) = self.scroll_fling.take() else {
            return false;
        };
        if !self.scroll_route_is_current(scene, layout, &fling.route)
            || self.menu_open()
            || self.choice_open()
        {
            return false;
        }
        let Some(delta) = fling.motion.step(now) else {
            return false;
        };
        if delta == Point::default() {
            self.scroll_fling = Some(fling);
            return false;
        }
        let moved = self.scroll_captured(scene, layout, &fling.route, delta);
        if fling.motion.finish_step(moved) {
            self.scroll_fling = Some(fling);
        }
        moved.into_iter().any(|moved| moved)
    }

    pub fn scroll_motion_active(&self) -> bool {
        self.scroll_fling.is_some()
    }

    pub fn tumbler_preview(&self) -> Option<(&[usize], f32)> {
        let drag = self.scroll_gesture.as_ref()?.tumbler.as_ref()?;
        (drag.offset != 0.0).then_some((&drag.target.path, drag.offset))
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
        self.scroll_gesture
            .as_ref()
            .and_then(|gesture| gesture.tumbler.as_ref())
            .map_or(0.0, |drag| drag.offset_for(wheel, path))
    }

    pub fn tumbler_scroll(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        delta: f32,
        lines: bool,
    ) -> TumblerScroll {
        let target = self
            .pointer
            .and_then(|point| self.target_at(scene, layout, point));
        if let Some(target) = target
            && let Some(node) = scene
                .at_path(&target.path)
                .filter(|node| node.kind == "Tumbler")
            && let Some(wheel) = layout
                .at_path(&target.path)
                .and_then(|item| item.tumbler.as_ref())
        {
            return crate::tumbler::scroll(
                &mut self.tumbler_wheel,
                &target,
                &Tumbler::from_node(node),
                wheel.center.height,
                delta,
                lines,
            );
        }
        self.tumbler_wheel = None;
        TumblerScroll::Unhandled
    }

    pub fn tumbler_key(
        &self,
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
            .filter(|node| node.kind == "Tumbler")?;
        let wheel = Tumbler::from_node(node);
        let index = match key {
            NavigationKey::Up => wheel.step(-1)?,
            NavigationKey::Down => wheel.step(1)?,
            NavigationKey::Home => 0,
            NavigationKey::End => wheel.count().checked_sub(1)?,
            NavigationKey::PageUp => wheel.step(-(wheel.visible() as i64))?,
            NavigationKey::PageDown => wheel.step(wheel.visible() as i64)?,
            _ => return None,
        };
        Some(ControlInput {
            path: target.path.clone(),
            action: wheel.select(index)?,
        })
    }

    pub fn cancel_scroll_gesture(&mut self) {
        self.scroll_gesture = None;
        self.scroll_fling = None;
        self.cancel_touch_delay();
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
            self.delay_pointer_claimed =
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
            self.delay_key_claimed = Some(key);
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

    pub fn is_text_editor(&self, node: &Node, target: &Target) -> bool {
        is_text_input(node.kind)
            || combo_editable(node)
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
        if previous.is_some_and(|(prior, when)| {
            prior == *target && now.saturating_duration_since(when) <= Duration::from_millis(450)
        }) {
            self.begin_table_edit(scene, layout)
        } else {
            self.last_table_click = Some((target.clone(), now));
            false
        }
    }

    pub fn begin_table_edit(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        if self.table_edit.is_some() {
            return false;
        }
        let Some(focused) = &self.focused else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&focused.path) else {
            return false;
        };
        let mut target = focused.clone();
        if matches!(node.kind, "TableView" | "TreeView") {
            let Some(slot) = current_list_slot(node) else {
                return false;
            };
            let Some(row) = node.children.get(slot).and_then(|row| row.children.first()) else {
                return false;
            };
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
                target.path.push(0);
                row.children.iter().enumerate().find_map(|(column, cell)| {
                    *target.path.last_mut().unwrap() = column;
                    (current_column.is_none_or(|current| current == column)
                        && editable_table_cell(cell)
                        && layout
                            .at_path(&target.path)
                            .is_some_and(|item| eligible(cell, item)))
                    .then_some(cell)
                })
            };
            let Some(cell) = cell else {
                return false;
            };
            target.id = cell.id;
            target.row_key = scene.row_key_at(&target.path);
        }
        let Some(cell) = scene.interactive_at_path(&target.path).filter(|cell| {
            target.row_key.is_some() && editable_table_cell(cell) && valid(&target, scene, layout)
        }) else {
            return false;
        };
        let original = cell.text("text").unwrap_or("").to_owned();
        self.set_focus(Some(target.clone()));
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
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return false;
        }
        let Some(edit) = self.table_edit.take() else {
            return false;
        };
        self.editor = None;
        self.last_table_click = None;
        edit.draft != edit.original
            && scene.row_key_at(&edit.target.path) == edit.target.row_key
            && component.commit_table_cell(scene, &edit.target.path, &edit.original, edit.draft)
    }

    pub fn sync_editor(&mut self, scene: &Node) {
        let target_node = self
            .focused
            .as_ref()
            .and_then(|target| Some((target, scene.at_path(&target.path)?)));
        if let Some((_, node)) = target_node
            .filter(|(target, node)| self.is_text_editor(node, target) || spin_editable(node))
        {
            if self.editor.is_none() {
                self.editor = Some(TextEdit::at_end(&self.editor_value(node)));
            }
            if (is_text_input(node.kind) || combo_editable(node))
                && let Some(target) = &self.focused
                && let Some(history) = self.history.get_mut(&EditorKey::new(scene, target))
            {
                history.observe(text_input_value(node));
            }
        } else {
            self.editor = None;
            self.spin_draft = None;
        }
    }

    pub fn break_text_edit_group(&mut self) {
        self.history.break_group();
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
        let Some(target) = &self.focused else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&target.path).filter(|node| {
            self.is_text_editor(node, target) && scene.row_key_at(&target.path) == target.row_key
        }) else {
            return false;
        };
        if node.boolean("read-only", false)
            && matches!(
                action,
                EditAction::Insert(_)
                    | EditAction::ReplaceAll(_)
                    | EditAction::Backspace
                    | EditAction::Delete
            )
        {
            return false;
        }

        let draft = self
            .table_edit
            .as_mut()
            .filter(|edit| &edit.target == target);
        let text = draft
            .as_ref()
            .map_or_else(|| text_input_value(node), |edit| &edit.draft);
        let editor = self.editor.get_or_insert_with(|| TextEdit::at_end(text));
        let previous = editor.clone();
        let before = editor.selection_state(text);
        let Some(edit) = editor.apply_mode(text, action, is_multiline_text_input(node.kind)) else {
            self.break_text_edit_group();
            return false;
        };
        if edit.text == text {
            self.break_text_edit_group();
            return false;
        }
        if draft.is_none()
            && (!node.allows_text_edit(&edit.text) || !commit(target, edit.text.clone()))
        {
            *editor = previous;
            editor.set_preedit(String::new(), None);
            return false;
        }
        self.painted_combo_completion = None;
        self.dismissed_combo_draft = None;
        let group = Group::for_edit(source, action, before, &edit);
        let after = editor.selection_state(&edit.text);
        let (next, record) = Record::new(edit, before, after);
        if node.kind == "SearchField" {
            self.search_query = (!next.is_empty()).then(|| next.clone());
            self.choice = None;
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
        let Some(target) = &self.focused else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&target.path).filter(|node| {
            self.is_text_editor(node, target)
                && !node.boolean("read-only", false)
                && scene.row_key_at(&target.path) == target.row_key
        }) else {
            return false;
        };
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
        if node.kind == "SearchField" {
            self.search_query = (!next.is_empty()).then(|| next.clone());
            self.choice = None;
        }
        history.commit_replay(next, redo);
        self.painted_combo_completion = None;
        self.dismissed_combo_draft = None;
        true
    }

    pub fn edit_spin(&mut self, node: &Node, action: EditAction<'_>) {
        if !spin_editable(node) {
            return;
        }
        let text = self.editor_value(node).into_owned();
        let edit = self.editor.get_or_insert_with(|| TextEdit::at_end(&text));
        let previous = edit.clone();
        if let Some(next) = edit.apply(&text, action) {
            let mode = self
                .spin_draft
                .as_ref()
                .map_or_else(|| SpinDraftMode::for_node(node), |draft| draft.mode);
            if spin_text_is_valid(&next, mode.separator()) {
                self.spin_draft = Some(SpinDraft { text: next, mode });
            } else {
                *edit = previous;
            }
        }
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

    pub fn spin_press(&mut self, scene: &Node, layout: &LayoutNode) {
        self.spin_pressed = (|| {
            let target = self.pressed.as_ref()?;
            if !valid(target, scene, layout) || !is_spin_box(scene.at_path(&target.path)?.kind) {
                return None;
            }
            SpinGeometry::new(layout.at_path(&target.path)?.bounds).action_at(self.pointer?)
        })();
    }

    pub fn spin_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let pressed = self.spin_pressed.take()?;
        let target = self.pointer_release_target()?;
        if !valid(&target, scene, layout) || !is_spin_box(scene.at_path(&target.path)?.kind) {
            return None;
        }
        let action =
            SpinGeometry::new(layout.at_path(&target.path)?.bounds).action_at(self.pointer?)?;
        (action == pressed).then_some(ControlInput {
            path: target.path,
            action: ControlAction::Spin(action),
        })
    }

    pub fn combo_completion(&self, node: &Node, path: &[usize]) -> Option<ComboCompletion> {
        let target = self
            .focused
            .as_ref()
            .filter(|target| target.path == path && target.id == node.id)?;
        if !combo_editable(node)
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

    fn take_current_combo_completion(&mut self, scene: &Node) -> Option<ComboCompletion> {
        let painted = self.painted_combo_completion.take()?;
        let path = &painted.target.path;
        let node = scene.interactive_at_path(path)?;
        if node.id != painted.target.id || scene.row_key_at(path) != painted.target.row_key {
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
        let Some(popup) = ChoicePopup::open(target.clone(), scene, layout) else {
            return false;
        };
        self.set_focus(Some(target));
        self.cancel_shortcut_sequence();
        self.choice = Some(popup);
        self.pressed = None;
        self.pressed_radio = None;
        self.hovered = None;
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
        if scene
            .at_path(&target.path)
            .is_none_or(|node| node.kind != "ComboBox")
        {
            return false;
        }
        if scene.at_path(&target.path).is_some_and(combo_editable) {
            let Some(item) = layout.at_path(&target.path) else {
                return false;
            };
            let trigger = ComboGeometry::new(item.bounds).trigger;
            if !origin
                .zip(self.pointer)
                .is_some_and(|(start, end)| trigger.contains(start) && trigger.contains(end))
            {
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

    pub fn dismiss_choice(&mut self) {
        self.choice = None;
        self.search_query = None;
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
                    .is_some_and(|item| item.clip.intersect(item.bounds).contains(point));
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
        let last = combo_count(node).checked_sub(1)?;
        let current = combo_index(node);
        let index = match key {
            ChoiceKey::Up => current.map_or(last, |index| index.saturating_sub(1)),
            ChoiceKey::Down => current.map_or(0, |index| (index + 1).min(last)),
            ChoiceKey::Home => 0,
            ChoiceKey::End => last,
            _ => return None,
        };
        (current != Some(index)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Select(index),
        })
    }

    pub fn pointer_move(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if self.pointer != Some(point) {
            self.tooltip.source = TooltipSource::Pointer;
        }
        self.pointer = Some(point);
        let hit = self.hit_at(scene, layout, point);
        let cancelled_delay = self.delay_hold.as_ref().is_some_and(|hold| {
            matches!(hold.source, DelaySource::Pointer)
                && !matches!(&hit, Hit::Target(target) if *target == hold.target)
        });
        if cancelled_delay {
            self.cancel_delay();
        }
        self.pointer_on_stack_slide = matches!(hit, Hit::StackSlide);
        if let Some(popup) = &mut self.menu {
            return popup.hover(scene, layout, point);
        }
        if let Some(popup) = &mut self.choice {
            return popup.hover(scene, layout, point);
        }
        let (next, scroll_bar, control_part) = match hit {
            Hit::Target(target) => (Some(target), None, None),
            Hit::ScrollBar(target) => (None, Some(target), None),
            Hit::ControlPart(target, action) => (Some(target), None, Some(action)),
            _ => (None, None, None),
        };
        let radio = next
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .filter(|node| node.kind == "RadioButton")
            .map(|node| node.text("text").unwrap_or(""));
        let list_row = next.as_ref().and_then(|target| list_row(scene, target));
        let standard_button = next
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .and_then(Node::standard_button_kind);
        let tree_indicator = next.as_ref().is_some_and(|target| {
            scene
                .at_path(&target.path)
                .zip(layout.at_path(&target.path))
                .is_some_and(|(node, item)| {
                    node.boolean("tree-has-children", false)
                        && TreeRowGeometry::new(node, item.bounds)
                            .is_some_and(|tree| tree.indicator.contains(point))
                })
        });
        let body_column = next.as_ref().and_then(|target| {
            let (path, _) = table_hit_row(scene, &target.path)?;
            if scene.at_path(path)?.selection_target() == SelectionTarget::Rows {
                return None;
            }
            layout.at_path(path)?.table.as_ref()?.body_column_at(point)
        });
        let mut table_grip = None;
        let mut table_column = None;
        if let Some(target) = &next
            && let Some(node) = scene.at_path(&target.path)
            && let Some(item) = layout.at_path(&target.path)
            && let Some(table) = &item.table
        {
            if let Some(index) = table.resize_at(node, item.clip, point) {
                let bounds = table.columns[index];
                table_grip = Some((index, bounds.width));
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
            || radio != self.hovered_radio.as_deref()
            || list_row != self.hovered_list_row
            || tree_indicator != self.hovered_tree_indicator
            || table_column != self.hovered_table_column
            || table_grip != self.hovered_table_grip
            || body_column != self.hovered_body_column;
        if radio != self.hovered_radio.as_deref() {
            self.hovered_radio = radio.map(str::to_owned);
        }
        self.hovered = next;
        self.hovered_scroll_bar = scroll_bar;
        self.hovered_control_part = control_part;
        self.hovered_standard_button = standard_button;
        self.hovered_list_row = list_row;
        self.hovered_tree_indicator = tree_indicator;
        self.hovered_table_column = table_column;
        self.hovered_table_grip = table_grip;
        self.hovered_body_column = body_column;
        changed
    }
    pub fn pointer_leave(&mut self) -> bool {
        let cancelled_delay = self
            .delay_hold
            .as_ref()
            .is_some_and(|hold| matches!(hold.source, DelaySource::Pointer));
        if cancelled_delay {
            self.cancel_delay();
        }
        self.pointer = None;
        self.tumbler_wheel = None;
        self.pointer_on_stack_slide = false;
        let tooltip_was_visible = self.tooltip.pointer_left();
        self.hovered_radio = None;
        self.hovered_list_row = None;
        self.hovered_tree_indicator = false;
        self.hovered_table_column = None;
        self.hovered_table_grip = None;
        self.hovered_body_column = None;
        self.hovered_control_part = None;
        self.hovered_standard_button = None;
        let bar = self.hovered_scroll_bar.take().is_some();
        self.hovered.take().is_some() || bar || tooltip_was_visible || cancelled_delay
    }
    pub fn pointer_press(&mut self) {
        self.cancel_shortcut_sequence();
        self.cancel_delay();
        self.delay_pointer_claimed = false;
        if self.pointer_on_stack_slide {
            return;
        }
        self.press_origin = self.pointer;
        self.tooltip.source = TooltipSource::Pointer;
        self.tooltip.dismiss();
        self.pressed = self.hovered.clone();
        self.pressed_radio.clone_from(&self.hovered_radio);
        self.pressed_list_row = self.hovered_list_row;
        self.pressed_tree_indicator = self.hovered_tree_indicator;
        self.pressed_table_column = self.hovered_table_column;
        self.pressed_body_column = self.hovered_body_column;
        self.pressed_control_part = self.hovered_control_part;
        self.pressed_standard_button = self.hovered_standard_button;
        self.table_resize = (|| {
            let (column, width) = self.hovered_table_grip?;
            Some(TableResize {
                target: self.pressed.clone()?,
                column,
                pointer_x: self.pointer?.x,
                width,
            })
        })();
        self.slider_offset = 0.0;
        self.slider_anchor = None;
        self.dial_pointer = None;
        self.spin_pressed = None;
        let focus = self.hovered.clone().and_then(|mut target| {
            if let Some((_, id)) = self.hovered_list_row {
                target.path.pop();
                target.id = id;
                target.focusable = true;
                target.row_key = None;
            }
            target.focusable.then_some(target)
        });
        self.set_focus(focus);
        self.key_pressed = None;
    }

    pub fn split_press(&mut self, scene: &Node, layout: &LayoutNode) {
        self.split_drag = (|| {
            let target = self.pressed.as_ref()?;
            let (_, geometry, handle) = split_target(target, scene, layout)?;
            let orientation = geometry.orientation;
            let position = axis_point(orientation, self.pointer?);
            Some(SplitDrag {
                target: target.clone(),
                orientation,
                after_pane: handle.after_pane(),
                grab_offset: position - handle.center(orientation),
            })
        })();
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
        if geometry.orientation != drag.orientation || handle.after_pane() != drag.after_pane {
            return None;
        }
        let position = axis_point(drag.orientation, self.pointer?) - drag.grab_offset;
        let weights = geometry.resize(handle, position - handle.center(drag.orientation))?;
        Some(SplitResize {
            path: path.to_vec(),
            weights,
        })
    }

    pub fn finish_split_resize(&mut self) -> bool {
        if self.split_drag.is_none() {
            return false;
        }
        self.pointer_release_target();
        true
    }

    pub fn split_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<SplitResize> {
        let (path, geometry, handle) = split_target(self.focused.as_ref()?, scene, layout)?;
        let delta = match (geometry.orientation, key) {
            (Orientation::Horizontal, NavigationKey::Left)
            | (Orientation::Vertical, NavigationKey::Up) => -8.0,
            (Orientation::Horizontal, NavigationKey::Right)
            | (Orientation::Vertical, NavigationKey::Down) => 8.0,
            (_, NavigationKey::Home) => handle.minimum,
            (_, NavigationKey::End) => handle.maximum,
            _ => return None,
        };
        Some(SplitResize {
            path: path.to_vec(),
            weights: geometry.resize(handle, delta)?,
        })
    }

    pub fn slider_press(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        let target = self.pressed.as_ref()?;
        let (path, node) = slider_target(target, scene, layout)?;
        let bounds = layout.at_path(path)?.bounds;
        let point = self.pointer?;
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
        let geometry = SliderGeometry::new(bounds, node.orientation());
        let fraction = geometry.fraction_at(point)?;
        let mut path = path.to_vec();
        let current = if node.kind == "RangeSlider" {
            let [lower, upper] = SliderInterval::from_node(node).fractions();
            let handle = usize::from(if lower == upper {
                fraction >= lower
            } else {
                (fraction - upper).abs() < (fraction - lower).abs()
            });
            path.push(handle);
            self.focus_slider_thumb(scene, &path);
            let current = [lower, upper][handle];
            if lower == upper && geometry.contains_thumb(current, point) {
                self.slider_anchor = Some(fraction);
            }
            current
        } else {
            SliderRange::from_node(node).fraction()
        };
        if geometry.contains_thumb(current, point) {
            self.slider_offset = current - fraction;
            None
        } else {
            Some(RangeInput {
                path,
                action: RangeAction::SetFraction(fraction),
            })
        }
    }

    fn focus_slider_thumb(&mut self, scene: &Node, path: &[usize]) {
        let target = Target {
            path: path.to_vec(),
            id: None,
            focusable: true,
            row_key: scene.row_key_at(path),
        };
        self.set_focus(Some(target.clone()));
        self.pressed = Some(target);
    }

    pub fn slider_drag(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        let target = self.pressed.as_ref()?;
        let (owner, node) = slider_target(target, scene, layout)?;
        let bounds = layout.at_path(owner)?.bounds;
        if node.kind == "Dial" {
            let point = self.pointer?;
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
        let fraction =
            SliderGeometry::new(bounds, node.orientation()).fraction_at(self.pointer?)?;
        let mut path = target.path.clone();
        if let Some(anchor) = self.slider_anchor {
            if fraction == anchor {
                return None;
            }
            path.truncate(owner.len());
            path.push(usize::from(fraction > anchor));
            self.focus_slider_thumb(scene, &path);
            self.slider_anchor = None;
        }
        Some(RangeInput {
            path,
            action: RangeAction::SetFraction(fraction + self.slider_offset),
        })
    }

    pub fn slider_release(&mut self, scene: &Node, layout: &LayoutNode) -> Option<RangeInput> {
        slider_target(self.pressed.as_ref()?, scene, layout)?;
        self.slider_offset = 0.0;
        self.slider_anchor = None;
        self.dial_pointer = None;
        let path = self.pressed.take()?.path;
        Some(RangeInput {
            path,
            action: RangeAction::Release,
        })
    }

    pub fn slider_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: SliderKey,
        pressed: bool,
    ) -> Option<RangeInput> {
        let target = if pressed {
            let target = self.focused.as_ref()?;
            if !valid(target, scene, layout) {
                return None;
            }
            let (_, node) = slider_target(target, scene, layout)?;
            let matches_axis = match key {
                SliderKey::Left | SliderKey::Right => {
                    node.kind == "Dial" || node.orientation() == Orientation::Horizontal
                }
                SliderKey::Up | SliderKey::Down => {
                    node.kind == "Dial" || node.orientation() == Orientation::Vertical
                }
                SliderKey::Home | SliderKey::End => true,
                SliderKey::PageUp | SliderKey::PageDown => false,
            };
            if !matches_axis {
                return None;
            }
            self.range_keys.entry(key).or_insert_with(|| target.clone());
            target.clone()
        } else {
            let target = self.range_keys.remove(&key)?;
            if self.focused.as_ref() != Some(&target) {
                return None;
            }
            slider_target(&target, scene, layout)?;
            return Some(RangeInput {
                path: target.path,
                action: RangeAction::Release,
            });
        };
        let action = match key {
            SliderKey::Left | SliderKey::Down => RangeAction::Decrease,
            SliderKey::Right | SliderKey::Up => RangeAction::Increase,
            SliderKey::Home => RangeAction::Minimum,
            SliderKey::End => RangeAction::Maximum,
            SliderKey::PageUp | SliderKey::PageDown => return None,
        };
        Some(RangeInput {
            path: target.path,
            action,
        })
    }

    pub fn select_group_option(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        if !valid(target, scene, layout) {
            return None;
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
        if !self.hovered_tree_indicator || !valid(target, scene, layout) {
            return None;
        }
        let (&slot, path) = target.path.split_last()?;
        let node = scene.at_path(path)?;
        let index = node.list?.first_row.checked_add(slot)?;
        let item = node.tree.as_ref()?.rows().get(index)?;
        (item.has_children && target.row_key == Some(item.row)).then_some(ControlInput {
            path: path.to_vec(),
            action: ControlAction::Tree(TreeAction::SetExpanded {
                key: item.key,
                row: item.row,
                expanded: !item.expanded,
            }),
        })
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
        let tree = node.tree.as_ref()?;
        let index = tree.visible_index(node.int("current-key")?)?;
        let item = tree.rows().get(index)?;
        if item.has_children && (key == NavigationKey::Right) != item.expanded {
            return self.tree_toggle(scene, layout);
        }
        let next = if key == NavigationKey::Left {
            tree.visible_index(item.parent?)?
        } else {
            index.checked_add(1).filter(|&next| {
                tree.rows()
                    .get(next)
                    .is_some_and(|next| next.parent == Some(item.key))
            })?
        };
        self.reveal_row(&path, node, layout.at_path(&path)?, next);
        self.set_focus(Some(Target {
            path: path.clone(),
            id: node.id,
            focusable: true,
            row_key: scene.row_key_at(&path),
        }));
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
                let column = cell_row
                    .and_then(|_| target.path.last().copied())
                    .or(selection.current_column())
                    .unwrap_or(0);
                if selection.target() == SelectionTarget::Columns {
                    TableSelectionAction::Column { column, operation }
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
                            column,
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
            .or_else(|| cell.and_then(|_| focused.path.last().copied()));
        let last = table.columns.len().checked_sub(1)?;
        let visible = |&column: &usize| table.columns[column].width > 0.0;
        let next = match key {
            NavigationKey::Left => (0..current.unwrap_or(last + 1))
                .rev()
                .find(visible)
                .or(current),
            NavigationKey::Right => (current.map_or(0, |column| column + 1)..=last)
                .find(visible)
                .or(current),
            NavigationKey::Home if view.target() == SelectionTarget::Columns => {
                (0..=last).find(visible)
            }
            NavigationKey::End if view.target() == SelectionTarget::Columns => {
                (0..=last).rev().find(visible)
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
            let column = current.or_else(|| (0..=last).find(visible))?;
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
        let bounds = table.columns[column];
        let scroll = table.horizontal;
        let left = bounds.x - table.body.x + scroll.offset;
        let right = left + bounds.width.min(table.body.width);
        let offset = if left < scroll.offset {
            left
        } else {
            scroll.offset.max(right - table.body.width)
        }
        .clamp(0.0, scroll.max_offset);
        if offset != scroll.offset {
            self.horizontal_scroll_offsets.insert(path.clone(), offset);
        }
        self.set_focus(Some(Target {
            path: path.clone(),
            id: node.id,
            focusable: true,
            row_key: None,
        }));
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
        let offset = scroll
            .revealing(cells.top(row) as f32, cells.height, viewport.height)
            .offset;
        if offset != scroll.offset {
            self.scroll_offsets.insert(path.to_vec(), offset);
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
        let page = ((f64::from(viewport.height) / cells.pitch()).floor() as usize)
            .max(1)
            .saturating_mul(columns);
        let current = ListMetrics::selection(node)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&row| row <= last);
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
            NavigationKey::PageUp => current.map_or(0, |row| row.saturating_sub(page)),
            NavigationKey::PageDown => current.map_or(0, |row| row.saturating_add(page).min(last)),
            _ => return None,
        };
        let action = ControlInput {
            path: path.to_vec(),
            action: ControlAction::Select(index),
        };
        self.reveal_row(&action.path, node, layout, index);
        if focused_cell {
            self.set_focus(Some(Target {
                path: action.path.clone(),
                id: node.id,
                focusable: true,
                row_key: None,
            }));
        }
        Some(action)
    }

    pub fn group_key(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
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
        let choices: Vec<_> = group_targets(group, group_layout, path).collect();
        let position = choices.iter().position(|(index, _)| *index == current)?;
        let len = choices.len();
        let next = match key {
            NavigationKey::Home => 0,
            NavigationKey::End => len - 1,
            NavigationKey::Up if vertical => (position + len - 1) % len,
            NavigationKey::Down if vertical => (position + 1) % len,
            NavigationKey::Left if !vertical => (position + len - 1) % len,
            NavigationKey::Right if !vertical => (position + 1) % len,
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
        if group.kind != "TabBar" {
            return false;
        }
        let orientation = group.orientation();
        let Some(scroll) = item.scroll_on(orientation) else {
            return false;
        };
        let Some(tab) = item.children.get(index) else {
            return false;
        };
        let offset = crate::selection::tab_reveal_offset(
            item.bounds,
            tab.bounds,
            orientation,
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
        let current = group_index(view)?;
        let eligible = |&index: &usize| group_item(view, index).is_some();
        let vertical = view.orientation() == Orientation::Vertical;
        let index = match key {
            NavigationKey::Home => (0..view.children.len()).find(eligible),
            NavigationKey::End => (0..view.children.len()).rev().find(eligible),
            NavigationKey::Left if !vertical => swipe_neighbor(view, current, false),
            NavigationKey::Right if !vertical => swipe_neighbor(view, current, true),
            NavigationKey::Up if vertical => swipe_neighbor(view, current, false),
            NavigationKey::Down if vertical => swipe_neighbor(view, current, true),
            _ => None,
        }?;
        if index == current {
            return None;
        }
        let path = path.to_vec();
        self.set_focus(Some(Target {
            row_key: scene.row_key_at(&path),
            path: path.clone(),
            id: view.id,
            focusable: true,
        }));
        Some(ControlInput {
            path,
            action: ControlAction::Select(index),
        })
    }

    pub fn swipe_delegate_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        let geometry = layout.at_path(&target.path)?.swipe_delegate.as_deref()?;
        let side = match (key, geometry.side) {
            (NavigationKey::Left, SwipeSide::Left) | (NavigationKey::Right, SwipeSide::Right) => {
                SwipeSide::Closed
            }
            (NavigationKey::Left, _) => SwipeSide::Right,
            (NavigationKey::Right, _) => SwipeSide::Left,
            _ => return None,
        };
        self.swipe_delegate_action(scene, layout, target, side)
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
        let index = match key {
            NavigationKey::Home => 0,
            NavigationKey::End => last,
            NavigationKey::Left if !vertical => current.map_or(0, |index| index.saturating_sub(1)),
            NavigationKey::Right if !vertical => current.map_or(0, |index| (index + 1).min(last)),
            NavigationKey::Up if vertical => current.map_or(0, |index| index.saturating_sub(1)),
            NavigationKey::Down if vertical => current.map_or(0, |index| (index + 1).min(last)),
            _ => return None,
        };
        (current != Some(index)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Select(index),
        })
    }

    pub fn calendar_key(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        key: NavigationKey,
    ) -> Option<ControlInput> {
        let target = self.focused.as_ref()?;
        let node = scene.at_path(&target.path)?;
        if node.kind != "Calendar" || !valid(target, scene, layout) {
            return None;
        }
        let calendar = CalendarState::from_node(node);
        let day = |delta| {
            calendar
                .cursor()
                .add_days(delta)
                .unwrap_or(if delta < 0 { Date::MIN } else { Date::MAX })
                .clamp(calendar.minimum, calendar.maximum)
        };
        let action = match key {
            NavigationKey::Left => CalendarAction::Select(day(-1)),
            NavigationKey::Right => CalendarAction::Select(day(1)),
            NavigationKey::Up => CalendarAction::Select(day(-7)),
            NavigationKey::Down => CalendarAction::Select(day(7)),
            NavigationKey::Home => CalendarAction::Select(calendar.month.max(calendar.minimum)),
            NavigationKey::End => {
                CalendarAction::Select(calendar.month.last_of_month().min(calendar.maximum))
            }
            NavigationKey::PageUp => {
                CalendarAction::Browse(calendar.adjacent_month(-1).unwrap_or(calendar.month))
            }
            NavigationKey::PageDown => {
                CalendarAction::Browse(calendar.adjacent_month(1).unwrap_or(calendar.month))
            }
        };
        Some(ControlInput {
            path: target.path.clone(),
            action: ControlAction::Calendar(action),
        })
    }

    pub fn calendar_activate(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
    ) -> Option<ControlInput> {
        let node = scene.at_path(&target.path)?;
        (node.kind == "Calendar" && valid(target, scene, layout)).then(|| ControlInput {
            path: target.path.clone(),
            action: ControlAction::Calendar(CalendarAction::Select(
                CalendarState::from_node(node).cursor(),
            )),
        })
    }

    pub fn pointer_release_target(&mut self) -> Option<Target> {
        let claimed = std::mem::take(&mut self.delay_pointer_claimed);
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
        let resized = self.table_resize.take().is_some();
        let split_resized = self.split_drag.take().is_some();
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

    pub fn table_resize_drag(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let drag = self.table_resize.as_ref()?;
        if !valid(&drag.target, scene, layout) {
            return None;
        }
        let node = scene.at_path(&drag.target.path)?;
        let column = resizable_table_column(node, drag.column)?;
        let table = layout.at_path(&drag.target.path)?.table.as_ref()?;
        let bounds = table.columns[drag.column];
        let (min, max) = crate::table::column_limits(column, table.header.width);
        let width = drag.width + (self.pointer?.x - drag.pointer_x);
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
        let resizing = self.table_resize.is_some();
        if resizing {
            self.pointer_release_target();
        }
        resizing
    }

    pub fn table_resize_cursor(&self) -> bool {
        self.table_resize.is_some() || self.hovered_table_grip.is_some()
    }

    pub fn table_resize_column(&self, path: &[usize]) -> Option<usize> {
        if let Some(drag) = &self.table_resize {
            (drag.target.path == path).then_some(drag.column)
        } else if self.is_hovered(path) {
            self.hovered_table_grip.map(|(column, _)| column)
        } else {
            None
        }
    }
    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) {
        self.reconcile_dialogs(scene, layout);
        self.stack_focus.retain(|key, _| {
            scene
                .at_path(&key.path)
                .is_some_and(|view| view.kind == "StackView")
                && scene.row_key_at(&key.path) == key.row
        });
        if self.scroll_bar_press.as_ref().is_some_and(|press| {
            press.grab.is_some()
                && self
                    .scroll_bar_for_target(scene, layout, &press.target)
                    .is_none()
        }) {
            self.scroll_bar_press.as_mut().unwrap().grab = None;
        }
        if self.split_drag.as_ref().is_some_and(|drag| {
            !split_target(&drag.target, scene, layout).is_some_and(|(_, geometry, handle)| {
                geometry.orientation == drag.orientation && handle.after_pane() == drag.after_pane
            })
        }) {
            self.split_drag = None;
            self.pressed = None;
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
        // Preserve the user's scroll between selection and grid reflow changes.
        self.scroll_selections = self
            .scroll_offsets
            .keys()
            .chain(self.horizontal_scroll_offsets.keys())
            .filter_map(|path| {
                let view = scene.at_path(path).filter(|node| {
                    matches!(node.kind, "TabBar" | "StandardListView" | "GridView")
                        || node.kind == "TableView" && node.boolean("follow-current", false)
                })?;
                let cells = ViewCells::new(view, layout.at_path(path)?.bounds.width);
                Some((path.clone(), (scroll_selection(view), cells.columns)))
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
        if self
            .focused
            .as_ref()
            .is_some_and(|t| !valid(t, scene, layout))
        {
            let current = self.focused.as_ref().unwrap().clone();
            if let Some(next) = self.restore_stack_focus(&current, scene, layout) {
                self.set_focus(Some(next));
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
                let fallback = (0..current.path.len()).rev().find_map(|length| {
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
                        "ListView" | "GridView" | "StandardListView" | "TableView" | "TreeView"
                        | "SwipeView" | "SwipeDelegate" | "StackView" | "GroupBox"
                            if eligible(node, item) =>
                        {
                            Target {
                                path: path.to_vec(),
                                id: node.id,
                                focusable: true,
                                row_key: None,
                            }
                        }
                        "TabWidget" | "TabBar" | "RadioGroup" => group_focus(node, item, path)?,
                        "Dialog" | "Drawer" => dialog_entry_focus(scene, layout, path)?,
                        _ => return None,
                    };
                    target.row_key = scene.row_key_at(&target.path);
                    Some(target)
                });
                self.set_focus(fallback);
            }
            self.key_pressed = None;
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
        if self
            .choice
            .as_mut()
            .is_some_and(|popup| !popup.reconcile(scene, layout))
        {
            self.choice = None;
        }
        self.reconcile_search_choice(scene, layout);
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
            }
        }
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
            .or_else(|| {
                Some(Target {
                    path: path.to_vec(),
                    id: view.id,
                    focusable: true,
                    row_key: row,
                })
            })
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
        self.set_focus(focus);
    }
    /// Move focus, returning whether revealing the focused tab requires layout.
    pub fn focus_next(&mut self, scene: &Node, layout: &LayoutNode, reverse: bool) -> bool {
        self.tooltip.focus_source();
        let scope = self
            .focused_popup(layout)
            .or_else(|| self.active_dialog(layout))
            .unwrap_or_default();
        let mut targets = focus_targets(scene, layout, scope);
        self.key_pressed = None;
        if targets.is_empty() {
            self.set_focus(None);
            return false;
        }
        let current = self.focused.as_ref().and_then(|target| {
            targets
                .iter()
                .position(|entry| entry == target)
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
        let next = targets.swap_remove(index);
        let revealed = next.path.split_last().is_some_and(|(&index, path)| {
            scene
                .at_path(path)
                .zip(layout.at_path(path))
                .is_some_and(|(group, item)| self.reveal_tab(group, item, path, index))
        });
        self.set_focus(Some(next));
        revealed
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
        Some(Target {
            path: path.to_vec(),
            id: node.id,
            focusable: focusable(node),
            row_key: scene.row_key_at(path),
        })
    }

    pub fn accessibility_focus(&mut self, target: Target) {
        if target.focusable {
            self.tooltip.focus_source();
            self.set_focus(Some(target));
        }
    }
    pub fn key_press(&mut self, key: ActivationKey) {
        self.cancel_delay();
        self.delay_key_claimed = None;
        self.key_pressed = self.focused.clone().map(|t| (key, t));
        self.key_standard_button = None;
    }

    pub fn key_press_with_scene(&mut self, scene: &Node, key: ActivationKey) {
        self.key_press(key);
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
        self.key_standard_button = None;
        let claimed = self.delay_key_claimed.take() == Some(key);
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
        (scene
            .at_path(&target.path)
            .and_then(Node::standard_button_kind)
            == kind)
            .then_some(target)
    }
    pub fn cancel_press(&mut self) {
        self.cancel_scroll_gesture();
        self.cancel_delay();
        self.delay_pointer_claimed = false;
        self.delay_key_claimed = None;
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
        self.split_drag = None;
        self.slider_offset = 0.0;
        self.slider_anchor = None;
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
    pub fn is_pressed(&self, path: &[usize]) -> bool {
        (self.pressed.as_ref().is_some_and(|t| t.path == path)
            && self.is_hovered(path)
            && self.pressed_list_row == self.hovered_list_row
            && self.pressed_standard_button == self.hovered_standard_button)
            || self
                .key_pressed
                .as_ref()
                .is_some_and(|(_, t)| t.path == path)
    }
}
