//! Behavior of individual scene nodes. Components still own bindings and callbacks.
mod builtin;
pub(crate) use builtin::item;
pub(crate) mod button;
pub(crate) mod calendar;
pub(crate) mod choice;
mod combo_box;
pub(crate) mod container;
mod control_content;
pub(crate) mod delay_button;
pub(crate) mod drag;
pub(crate) mod group_box;
pub(crate) mod menu;
mod overlays;
pub(crate) mod page;
pub(crate) mod page_indicator;
mod parts;
pub(crate) mod path_view;
pub(crate) mod range;
pub(crate) mod search;
pub(crate) mod selection;
mod slider;
pub(crate) mod spin;
pub(crate) mod swipe_delegate;
pub(crate) mod tumbler;
pub(crate) mod views;
use crate::{
    Interaction, LayoutNode, NavigationKey, Point, Rect, Size, TextMeasurer, flow,
    grid::{self, Placement},
    linear::{self, Axis, Metrics},
    transform::TransformSpace,
};
use std::rc::Rc;
use xrack_runtime::{ControlAction, IndicatorState, LayoutDirection, Node, RangeAction};

/// A node and its corresponding layout, borrowed for the current operation.
#[derive(Clone, Copy)]
pub struct ItemRef<'a, 'b> {
    pub node: &'a Node,
    pub layout: &'b LayoutNode,
}

impl<'a, 'b> ItemRef<'a, 'b> {
    /// Unclipped bounds in the item's own logical coordinates.
    pub fn local_rect(self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            self.layout.bounds.width,
            self.layout.bounds.height,
        )
    }

    /// Map a point relative to the item's top-left corner into window coordinates.
    pub fn local_to_window(self, point: Point) -> Point {
        self.layout
            .map_to_window(self.layout.local_to_layout(point))
    }

    /// Map a window point back into item coordinates. Returns `None` when
    /// the mapping is non-invertible or the result is non-finite.
    pub fn window_to_local(self, point: Point) -> Option<Point> {
        self.layout.window_to_local(point)
    }

    /// Map an item-local point into another item's local coordinates.
    /// Both items must belong to the same prepared window frame.
    pub fn map_to_item(self, target: ItemRef<'_, '_>, point: Point) -> Option<Point> {
        target.window_to_local(self.local_to_window(point))
    }

    pub fn paint(self) -> ItemPaint {
        paint(self.node)
    }

    pub(crate) fn event(self, event: ItemEvent) -> Option<ItemAction> {
        item(self.node.kind).event(self, event)
    }
}

/// The painting behavior selected by the item, including for retained rendering.
pub fn paint(node: &Node) -> ItemPaint {
    item(node.kind).paint(node)
}

pub(crate) fn layout(node: &Node) -> ItemLayout {
    item(node.kind).layout()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ItemLayout {
    #[default]
    Absolute,
    Grid,
    Row,
    LineOrFlow(container::ContentLayout),
    Fill {
        padded: bool,
    },
    Page,
    Window,
    Pane,
    Switch,
    Wheel {
        path: bool,
    },
    SwipeDelegate,
    Split,
    TabBar,
    Pages(PageLayout),
    Scroll(ScrollContainer),
    Rows(RowLayout),
    Overlay(OverlayLayout),
    Deferred(DeferredLayout),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayLayout {
    Modal,
    Drawer,
    Popup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeferredLayout {
    Menu,
    Tooltip,
    Shortcut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageLayout {
    Tabs,
    Swipe,
    Stack,
    StackLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollContainer {
    ScrollView,
    Flickable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowLayout {
    List,
    StandardList,
    Grid,
    Tree,
    Table,
    Row,
}

/// How the shared pointer router visits this item and its children.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitBehavior {
    Normal,
    ChildrenOnly,
    Transparent,
    RoundedButton,
    TabHeader,
    ScrollBar,
    SplitHandle,
    BlockBehind,
    SceneTexture,
    WindowMove,
    TouchArea,
    ProxyTarget,
    WheelHandler,
}

/// Drawing requested by an item and executed by the renderer in scene order.
#[derive(Clone, Copy)]
pub enum ItemPaint {
    Default,
    Rectangle,
    Window,
    Pane,
    Container(ContainerPaint),
    Rows,
    TableView,
    TableRow,
    ViewportPart,
    Part(PartPaint),
    Shortcut,
    Media,
    Image { animated: bool },
    Vector,
    Combo,
    SceneView3D,
    CustomRender,
    ColorPlane,
    ColorPreview,
    ToolSeparator,
    SplitHandle,
    Menu,
    MenuBar,
    RadioGroup,
    Tabs { standalone: bool },
    TabButton,
    Calendar { framed: bool },
    Wheel { framed: bool },
    Slider,
    RangeSlider,
    Dial,
    PageIndicator,
    StandardListItem,
    ListRow,
    Toggle(ToggleStyle),
    StyledText,
    Text,
    Label,
    Button(ButtonStyle),
    Entry { multiline: bool },
    SearchEntry,
    SpinEntry,
    Cell { tree: bool },
    GroupHeader { expander: bool },
    Boxed,
    ControlFrame,
    FocusScope,
    ScrollView,
    Flickable,
    ScrollBar { interactive: Option<bool> },
    SwipeView,
    SwipeDelegate,
    DropArea,
    Indicator(IndicatorState),
    Progress(IndicatorState),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContainerPaint {
    Empty,
    Row,
    Column,
    Horizontal,
    Vertical,
    Grid,
    BoxOrFlow,
    Split,
    ButtonContent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PartPaint {
    ProgressTrack,
    ProgressFill,
    ScrollTrack,
    ScrollThumb,
    SwitchThumb,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToggleStyle {
    Check,
    Radio,
    Switch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Push,
    Dialog,
    Tool,
    Round,
    Delay,
}

pub(crate) trait LayoutServices: TextMeasurer {
    fn metrics(&mut self, node: &Node, available: Size, width: Option<f32>) -> Metrics;

    fn preferred_size(&mut self, node: &Node, available: Size, width: Option<f32>) -> Size {
        let metrics = self.metrics(node, available, width);
        Size::new(metrics.x.preferred, metrics.y.preferred)
    }

    fn measure_children(&mut self, content: &Node, available: Size) -> Size;
    fn measure_line(&mut self, node: &Node, available: Size, axis: Axis) -> Metrics;
    fn measure_flow(&mut self, node: &Node, available: Size) -> Metrics;
    fn measure_grid(&mut self, node: &Node, available: Size) -> Metrics;
    fn arrange_grid(
        &mut self,
        node: &Node,
        bounds: Rect,
        direction: LayoutDirection,
    ) -> Vec<Placement>;
    fn absolute_child(&mut self, node: &Node, bounds: Rect) -> Rect;
}

impl<T: TextMeasurer> LayoutServices for T {
    fn metrics(&mut self, node: &Node, available: Size, width: Option<f32>) -> Metrics {
        linear::measure_with_width(node, available, width, self)
    }

    fn measure_children(&mut self, content: &Node, available: Size) -> Size {
        linear::measure_replacement_content(content, available, self)
    }

    fn measure_line(&mut self, node: &Node, available: Size, axis: Axis) -> Metrics {
        linear::measure_axis(node, available, axis, self)
    }

    fn measure_flow(&mut self, node: &Node, available: Size) -> Metrics {
        flow::measure(node, available, self)
    }

    fn measure_grid(&mut self, node: &Node, available: Size) -> Metrics {
        grid::measure(node, available, self)
    }

    fn arrange_grid(
        &mut self,
        node: &Node,
        bounds: Rect,
        direction: LayoutDirection,
    ) -> Vec<Placement> {
        grid::children(node, bounds, direction, self)
    }

    fn absolute_child(&mut self, node: &Node, bounds: Rect) -> Rect {
        crate::absolute_child(self, node, bounds)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SizePolicy {
    pub preferred_width: bool,
    pub preferred_height: bool,
    pub intrinsic: bool,
    pub preferred_minimum: bool,
}

impl SizePolicy {
    pub const FILL: Self = Self {
        preferred_width: false,
        preferred_height: false,
        intrinsic: false,
        preferred_minimum: false,
    };

    pub const PREFERRED: Self = Self {
        preferred_width: true,
        preferred_height: true,
        ..Self::FILL
    };

    pub const INTRINSIC_HEIGHT: Self = Self {
        preferred_height: true,
        intrinsic: true,
        ..Self::FILL
    };

    pub const INTRINSIC: Self = Self {
        preferred_width: true,
        ..Self::INTRINSIC_HEIGHT
    };
}

pub(crate) struct MeasureContext<'a> {
    pub node: &'a Node,
    pub available: Size,
    pub reference: Size,
    pub assigned_width: Option<f32>,
    pub size_policy: SizePolicy,
    pub layout: &'a mut dyn LayoutServices,
}

impl MeasureContext<'_> {
    fn intrinsic(&self, size: Size) -> Metrics {
        linear::intrinsic_metrics(self.node, size, self.size_policy)
    }
}

pub(crate) struct ArrangeContext<'a> {
    pub node: &'a Node,
    pub bounds: Rect,
    pub direction: LayoutDirection,
    pub space: &'a Option<Rc<TransformSpace>>,
    pub enabled: bool,
    pub input: Option<&'a Interaction>,
    pub path: &'a [usize],
    pub switch_position: Option<f32>,
    pub layout: &'a mut dyn LayoutServices,
}

pub(super) enum ControlPartPlacement {
    Plain(Rect),
    Clipped(Rect),
}

impl ArrangeContext<'_> {
    pub(super) fn control_children(
        &mut self,
        mut part: impl FnMut(&Node) -> Option<ControlPartPlacement>,
    ) -> Vec<Placement> {
        self.node
            .children
            .iter()
            .map(|child| match part(child) {
                Some(ControlPartPlacement::Plain(bounds)) => Placement::plain(bounds),
                Some(ControlPartPlacement::Clipped(bounds)) => self.clipped_child(bounds),
                None => Placement::plain(self.layout.absolute_child(child, self.bounds)),
            })
            .collect()
    }

    fn clipped_child(&self, bounds: Rect) -> Placement {
        Placement {
            clip: Some(Rc::new(crate::table::PaneClip {
                bounds,
                space: self.space.clone(),
            })),
            ..Placement::plain(bounds)
        }
    }
}

/// Pointer positions in these events are relative to the item's top-left corner.
pub(crate) enum ItemEvent {
    ChoiceTrigger {
        press: Option<Point>,
        release: Option<Point>,
    },
    ChoiceSelected(usize),
    HitTest(Point),
    HitAfterScroll(Point),
    HitTarget(Point),
    Navigate(NavigationKey),
    NavigateRow(NavigationKey),
    NavigateColumn {
        key: NavigationKey,
        focused_column: Option<usize>,
    },
    SelectRows(crate::TableSelectionKey),
    Activate,
}

#[derive(PartialEq)]
pub(crate) enum ItemAction {
    OpenChoice,
    Control(ControlAction),
    Range(RangeAction),
    Target,
    TableRowGrip(usize),
    TableColumnNavigation {
        next: Option<usize>,
        current_visible: Option<usize>,
    },
    ScrollBar {
        geometry: crate::ScrollBarGeometry,
        offset: f32,
    },
    BlockHit,
    SkipHit,
}

pub(crate) trait Item {
    fn size_policy(&self, _node: &Node) -> SizePolicy {
        SizePolicy::FILL
    }

    fn menu(&self) -> Option<&dyn menu::MenuItem> {
        None
    }

    fn drag_role(&self) -> Option<drag::DragRole> {
        None
    }

    fn reserves_pointer_drag(&self, _node: &Node) -> bool {
        false
    }

    fn reading_order(&self, _node: &Node, _count: usize) -> Option<Vec<usize>> {
        None
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Absolute
    }

    fn focusable(&self, _node: &Node) -> bool {
        false
    }

    fn focus_bounds(&self, item: ItemRef<'_, '_>) -> Option<Rect> {
        item.layout.visible.then_some(item.layout.bounds)
    }

    fn hit_behavior(&self, _node: &Node) -> HitBehavior {
        HitBehavior::Normal
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        cx.intrinsic(Size::default())
    }

    fn arrange(&self, _cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        None
    }

    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::Default
    }

    fn event(&self, _item: ItemRef<'_, '_>, _event: ItemEvent) -> Option<ItemAction> {
        None
    }
}
