use super::container::{ContentLayout, LineOrFlow};
use super::{
    ArrangeContext, ButtonStyle, ContainerPaint, DeferredLayout, HitBehavior, Item, ItemAction,
    ItemEvent, ItemLayout, ItemPaint, ItemRef, MeasureContext, OverlayLayout, PageLayout,
    PartPaint, RowLayout, ScrollContainer, ToggleStyle, container, overlays, views,
};
use crate::{
    ImageRegion, NavigationKey, Size, ToggleGeometry,
    grid::Placement,
    linear::{Axis, Padding},
};
use xrack_runtime::{Node, Orientation, is_multiline_text_input, is_text_input};

struct PreferredSize(Size, ItemPaint);

impl Item for PreferredSize {
    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        cx.intrinsic(self.0)
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        self.1
    }
}

struct ScrollBar {
    interactive: bool,
}

impl Item for ScrollBar {
    fn focus_bounds(&self, item: super::ItemRef<'_, '_>) -> Option<crate::Rect> {
        let layout = item.layout;
        if !self.interactive
            || !item.node.boolean("interactive", true)
            || !layout
                .scroll_control_geometry(item.node)
                .is_some_and(crate::ScrollBarGeometry::scrollable)
        {
            return None;
        }
        layout.visible.then_some(layout.bounds)
    }

    fn paint(&self, node: &Node) -> ItemPaint {
        ItemPaint::ScrollBar {
            interactive: self.interactive.then(|| node.boolean("interactive", true)),
        }
    }

    fn hit_behavior(&self, node: &Node) -> HitBehavior {
        if self.interactive && node.boolean("interactive", true) {
            HitBehavior::ScrollBar
        } else {
            HitBehavior::Transparent
        }
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        let ItemEvent::Navigate(key) = event else {
            return None;
        };
        if !self.interactive {
            return None;
        }
        let geometry = item.layout.scroll_control_geometry(item.node)?;
        let page = if geometry.page > 0.0 {
            geometry.page
        } else {
            geometry.step
        };
        let offset = match (key, geometry.orientation) {
            (NavigationKey::Home, _) => 0.0,
            (NavigationKey::End, _) => geometry.scroll.max_offset,
            (NavigationKey::PageUp, _) => geometry.scroll.offset - page,
            (NavigationKey::PageDown, _) => geometry.scroll.offset + page,
            (NavigationKey::Left, Orientation::Horizontal)
            | (NavigationKey::Up, Orientation::Vertical) => {
                geometry.scroll.offset + geometry.logical_delta(-geometry.step)
            }
            (NavigationKey::Right, Orientation::Horizontal)
            | (NavigationKey::Down, Orientation::Vertical) => {
                geometry.scroll.offset + geometry.logical_delta(geometry.step)
            }
            _ => return None,
        };
        Some(ItemAction::ScrollBar { geometry, offset })
    }

    fn focusable(&self, _node: &Node) -> bool {
        self.interactive
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = match cx.node.orientation() {
            Orientation::Horizontal => Size::new(160.0, 12.0),
            Orientation::Vertical => Size::new(12.0, 160.0),
        };
        cx.intrinsic(size)
    }
}

struct Text(ItemPaint);

impl Item for Text {
    fn baseline(&self, node: &Node, size: Size, text: &mut dyn super::LayoutServices) -> f32 {
        let font_size = text.font_size(node);
        let ascent = text.font_ascent(node, font_size);
        if let Some(layout) = text.shared_styled_text_layout(node, size) {
            return layout
                .fragments
                .iter_range(0..layout.fragments.len())
                .next()
                .map_or(0.0, |fragment| fragment.bounds.y + ascent);
        }
        if !crate::uses_label_layout(node) {
            return ascent;
        }
        crate::LabelLayout::with_font_size(node, size, font_size, |line| {
            text.measure_node(node, line, f32::MAX).width
        })
        .lines
        .first()
        .map_or(0.0, |line| line.origin.y + ascent)
    }

    fn reuses_placement(&self) -> bool {
        matches!(self.0, ItemPaint::Text | ItemPaint::Label)
    }

    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy {
            preferred_minimum: true,
            ..super::SizePolicy::INTRINSIC_HEIGHT
        }
    }

    fn focusable(&self, node: &Node) -> bool {
        node.selectable_styled_text()
    }

    fn paint(&self, node: &xrack_runtime::Node) -> ItemPaint {
        if matches!(self.0, ItemPaint::Text) && crate::uses_label_layout(node) {
            ItemPaint::Label
        } else {
            self.0
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = if crate::uses_label_layout(cx.node) {
            cx.layout.measure_label(cx.node, cx.available.width)
        } else if cx.node.kind == "Text" {
            cx.layout.measure_node(
                cx.node,
                cx.node.text("text").unwrap_or(""),
                cx.available.width,
            )
        } else {
            Size::default()
        };
        cx.intrinsic(size)
    }
}

pub(super) struct TextInput;

impl Item for TextInput {
    fn baseline(&self, node: &Node, size: Size, text: &mut dyn super::LayoutServices) -> f32 {
        let font_size = text.font_size(node);
        let area = Padding::for_editor(node).inner_rect(crate::Rect::new(
            0.0,
            0.0,
            size.width,
            size.height,
        ));
        let line = font_size * 1.4;
        let top = if is_multiline_text_input(node.kind) {
            area.y
        } else {
            area.y + (area.height - line).max(0.0) * 0.5
        };
        top + text.font_ascent(node, font_size)
    }

    fn size_policy(&self, node: &xrack_runtime::Node) -> super::SizePolicy {
        if is_multiline_text_input(node.kind) {
            super::SizePolicy::FILL
        } else {
            super::SizePolicy::INTRINSIC_HEIGHT
        }
    }

    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Entry {
            multiline: is_multiline_text_input(node.kind),
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let padding = Padding::for_editor(cx.node).size();
        let width = if matches!(cx.node.kind, "TextInput" | "LineEdit") {
            168.0
        } else {
            228.0
        };
        let line = cx.layout.font_size(cx.node) * 1.4;
        let height = if is_multiline_text_input(cx.node.kind) {
            4.0 * line + padding.height
        } else {
            (line + padding.height).max(34.0)
        };
        cx.intrinsic(Size::new(width + padding.width, height))
    }
}

struct Toggle {
    decoration: f32,
    style: ToggleStyle,
}

impl Item for Toggle {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn layout(&self) -> ItemLayout {
        if self.style == ToggleStyle::Switch {
            ItemLayout::Switch
        } else {
            ItemLayout::Absolute
        }
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if cx.node.control_part("ToggleIndicator").is_none()
            && cx.node.control_part("SwitchThumb").is_none()
        {
            return None;
        }
        let geometry = ToggleGeometry::new(cx.node, cx.bounds, cx.direction);
        let checked = cx.node.boolean("checked", false);
        Some(
            cx.node
                .children
                .iter()
                .map(|child| {
                    Placement::plain(match child.kind {
                        "ToggleIndicator" => geometry.indicator,
                        "SwitchThumb" => geometry.switch_thumb(checked, cx.switch_position),
                        _ => cx.layout.absolute_child(child, cx.bounds),
                    })
                })
                .collect(),
        )
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Toggle(self.style)
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let indicator = crate::toggle_indicator_size(cx.node);
        let decoration = indicator.map_or(self.decoration, |size| {
            size.width
                + if cx.node.is_item_delegate() {
                    34.0
                } else {
                    13.0
                }
        });
        let label = cx.layout.measure_node(
            cx.node,
            cx.node.text("text").unwrap_or(""),
            (cx.available.width - decoration).max(0.0),
        );
        let size = Size::new(
            label.width + decoration,
            label
                .height
                .max(indicator.map_or(26.0, |size| size.height.max(26.0))),
        );
        cx.intrinsic(size)
    }
}

struct Image {
    animated: bool,
}

impl Item for Image {
    fn reuses_placement(&self) -> bool {
        !self.animated
    }

    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Image {
            animated: self.animated,
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let source = cx.node.image_size("source");
        let [width, height] = ImageRegion::for_node(cx.node, source).size();
        if width == 0 || height == 0 {
            return cx.intrinsic(Size::default());
        }
        let (width, height) = (width as f32, height as f32);
        let scale = cx
            .node
            .relative_length("width", cx.reference.width)
            .map(|size| size.max(0.0) / width)
            .or_else(|| {
                cx.node
                    .relative_length("height", cx.reference.height)
                    .map(|size| size.max(0.0) / height)
            })
            .unwrap_or(1.0);
        cx.intrinsic(Size::new(width * scale, height * scale))
    }
}

struct SwipeDelegate;

impl Item for SwipeDelegate {
    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn layout(&self) -> super::ItemLayout {
        super::ItemLayout::SwipeDelegate
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::SwipeDelegate
    }

    fn event(&self, item: ItemRef<'_, '_>, event: super::ItemEvent) -> Option<super::ItemAction> {
        let super::ItemEvent::HitTest(point) = event else {
            return None;
        };
        let (rect, action) = item.layout.swipe_delegate.as_deref()?.toggle()?;
        rect.contains(item.layout.local_to_layout(point))
            .then_some(super::ItemAction::Control(action))
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = cx
            .node
            .children
            .iter()
            .find(|child| child.kind == "SwipeContent")
            .map_or(Size::default(), |content| {
                cx.layout.preferred_size(content, cx.available, None)
            });
        cx.intrinsic(size)
    }
}

struct FocusItem(ItemPaint, HitBehavior);

impl Item for FocusItem {
    fn focus_bounds(&self, item: super::ItemRef<'_, '_>) -> Option<crate::Rect> {
        if self.1 == HitBehavior::TabHeader {
            item.layout.tab_header
        } else {
            item.layout.visible.then_some(item.layout.bounds)
        }
    }

    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        matches!(self.0, ItemPaint::Cell { .. })
    }

    fn hit_behavior(&self, node: &Node) -> HitBehavior {
        match self.0 {
            ItemPaint::FocusScope if !node.boolean("focus-on-click", true) => {
                HitBehavior::ChildrenOnly
            }
            ItemPaint::Cell { tree: true } if !xrack_runtime::editable_table_cell(node) => {
                HitBehavior::ChildrenOnly
            }
            _ => self.1,
        }
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        self.0
    }
}

struct Indicator;

impl Item for Indicator {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        cx.intrinsic(Size::new(36.0, 36.0))
    }

    fn paint(&self, node: &xrack_runtime::Node) -> ItemPaint {
        super::parts::indicator_paint(node)
    }
}

struct PointerItem(HitBehavior);

impl Item for PointerItem {
    fn hit_behavior(&self, _node: &Node) -> HitBehavior {
        self.0
    }
}

struct DefaultItem;
impl Item for DefaultItem {}

struct Draw(ItemPaint);

impl Item for Draw {
    fn reuses_placement(&self) -> bool {
        matches!(self.0, ItemPaint::Rectangle)
    }

    fn hit_behavior(&self, _node: &Node) -> HitBehavior {
        if matches!(self.0, ItemPaint::SceneView3D) {
            HitBehavior::SceneTexture
        } else {
            HitBehavior::Normal
        }
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        self.0
    }
}

pub(crate) fn item(kind: &str) -> &'static dyn Item {
    match kind {
        "ComboBox" => &super::combo_box::ComboBox,
        "SearchField" => &super::search::SearchField,
        "SceneView3D" => &Draw(ItemPaint::SceneView3D),
        "CustomRender" => &Draw(ItemPaint::CustomRender),
        "CustomItem" => &super::custom_item::Native,
        "Path" => &Draw(ItemPaint::Vector),
        "ColorPlane" => &super::slider::ColorPlane,
        "ColorPreview" => &Draw(ItemPaint::ColorPreview),
        "RadioGroup" => {
            &const { LineOrFlow::new(ContentLayout::RadioGroup, ItemPaint::RadioGroup) }
        }
        "GridLayout" => &container::Grid,
        "StackLayout" => &container::Stack,
        "Loader" => &container::Loader(true),
        "LoaderCandidate" => &container::Loader(false),
        "Pane" => &container::Pane(ItemPaint::Pane),
        "ControlFrame" => &container::Pane(ItemPaint::ControlFrame),
        "ScrollView" => &container::ScrollView(ScrollContainer::ScrollView),
        "Flickable" => &container::ScrollView(ScrollContainer::Flickable),
        "ToolSeparator" => &container::ToolSeparator,
        "GroupBox" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Vertical),
                    ItemPaint::GroupHeader { expander: false },
                )
            }
        }
        "Expander" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Vertical),
                    ItemPaint::GroupHeader { expander: true },
                )
            }
        }
        "Window" => &overlays::Window,
        "Dialog" => &overlays::Overlay(OverlayLayout::Modal),
        "Drawer" => &overlays::Overlay(OverlayLayout::Drawer),
        "PopupWindow" => &overlays::Overlay(OverlayLayout::Popup),
        "Tooltip" => &overlays::Deferred(DeferredLayout::Tooltip),
        "Shortcut" => &overlays::Deferred(DeferredLayout::Shortcut),
        "ToolBar" => &const { LineOrFlow::new(ContentLayout::ToolBar, ItemPaint::Boxed) },
        "MenuBar" => {
            &const { LineOrFlow::new(ContentLayout::Line(Axis::Horizontal), ItemPaint::MenuBar) }
        }
        "HorizontalLayout" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Horizontal),
                    ItemPaint::Container(ContainerPaint::Horizontal),
                )
            }
        }
        "VerticalLayout" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Vertical),
                    ItemPaint::Container(ContainerPaint::Vertical),
                )
            }
        }
        "HorizontalBox" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Horizontal),
                    ItemPaint::Container(ContainerPaint::BoxOrFlow),
                )
            }
        }
        "VerticalBox" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Line(Axis::Vertical),
                    ItemPaint::Container(ContainerPaint::BoxOrFlow),
                )
            }
        }
        "FlowLayout" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Flow,
                    ItemPaint::Container(ContainerPaint::BoxOrFlow),
                )
            }
        }
        "FlexboxLayout" => {
            &const {
                LineOrFlow::new(
                    ContentLayout::Flex,
                    ItemPaint::Container(ContainerPaint::BoxOrFlow),
                )
            }
        }
        "DialogButtonBox" => {
            &const { LineOrFlow::new(ContentLayout::DialogButtons, ItemPaint::Default) }
        }
        "DynamicContainer" | "PageHeader" | "PageFooter" => {
            &const { LineOrFlow::new(ContentLayout::Line(Axis::Vertical), ItemPaint::Default) }
        }
        "TimePicker" | "SwipeContent" | "SwipeLeft" | "SwipeRight" => {
            &const { LineOrFlow::new(ContentLayout::Line(Axis::Horizontal), ItemPaint::Default) }
        }
        "FocusScope" => &FocusItem(ItemPaint::FocusScope, HitBehavior::Normal),
        "RangeThumb" | "ContextMenuArea" => &FocusItem(ItemPaint::Default, HitBehavior::Normal),
        "Tab" => &FocusItem(ItemPaint::Default, HitBehavior::TabHeader),
        "WindowMoveArea" => &PointerItem(HitBehavior::WindowMove),
        "TouchArea" => &PointerItem(HitBehavior::TouchArea),
        "WheelHandler" => &PointerItem(HitBehavior::WheelHandler),
        "TabButton" => &FocusItem(ItemPaint::TabButton, HitBehavior::Normal),
        "TableCell" => &FocusItem(ItemPaint::Cell { tree: false }, HitBehavior::Normal),
        "TreeCell" => &FocusItem(ItemPaint::Cell { tree: true }, HitBehavior::Normal),
        "Rectangle" => &Draw(ItemPaint::Rectangle),
        "TableRow" => &views::TableRow,
        "TableColumn" | "TreeRow" | "ItemDelegate" | "ScrollBarTrack" | "ScrollBarThumb" => {
            &Draw(ItemPaint::ViewportPart)
        }
        "Empty" => &Draw(ItemPaint::Container(ContainerPaint::Empty)),
        "Row" => &container::Row,
        "Column" => &Draw(ItemPaint::Container(ContainerPaint::Column)),
        "ButtonContent" => &Draw(ItemPaint::Container(ContainerPaint::ButtonContent)),
        "ProgressTrack" => &Draw(ItemPaint::Part(PartPaint::ProgressTrack)),
        "ProgressFill" => &Draw(ItemPaint::Part(PartPaint::ProgressFill)),
        "ScrollTrack" => &Draw(ItemPaint::Part(PartPaint::ScrollTrack)),
        "ScrollThumb" => &Draw(ItemPaint::Part(PartPaint::ScrollThumb)),
        "SwitchThumb" => &Draw(ItemPaint::Part(PartPaint::SwitchThumb)),
        "DragArea" => &super::drag::DragArea(super::drag::DragRole::Source),
        "DropArea" => &super::drag::DragArea(super::drag::DragRole::Target),
        "StandardListItem" => &Draw(ItemPaint::StandardListItem),
        "CalendarCells" => &super::parts::CalendarCells,
        "SplitHandle" => &super::parts::SplitHandle,
        "ProgressIndicator" => &super::parts::Progress,
        "Slider" => &super::slider::Slider { interval: false },
        "RangeSlider" => &super::slider::Slider { interval: true },
        "Dial" => &super::slider::Dial,
        "Text" => &Text(ItemPaint::Text),
        "Label" => &Text(ItemPaint::Label),
        "StyledText" => &Text(ItemPaint::StyledText),
        "Button" => &const { super::button::Button(Size::new(64.0, 40.0), ButtonStyle::Push) },
        "DialogButton" => {
            &const { super::button::Button(Size::new(64.0, 40.0), ButtonStyle::Dialog) }
        }
        "DelayButton" => {
            &const { super::button::Button(Size::new(64.0, 40.0), ButtonStyle::Delay) }
        }
        "ToolButton" => &const { super::button::Button(Size::new(34.0, 34.0), ButtonStyle::Tool) },
        "RoundButton" => {
            &const { super::button::Button(Size::new(40.0, 40.0), ButtonStyle::Round) }
        }
        "CheckBox" => &Toggle {
            decoration: 30.0,
            style: ToggleStyle::Check,
        },
        "RadioButton" => &Toggle {
            decoration: 30.0,
            style: ToggleStyle::Radio,
        },
        "Switch" => &Toggle {
            decoration: 50.0,
            style: ToggleStyle::Switch,
        },
        "SpinBox" => &super::spin::SpinBox(88.0),
        "DoubleSpinBox" => &super::spin::SpinBox(120.0),
        "ScrollBar" => &ScrollBar { interactive: true },
        "ScrollIndicator" => &ScrollBar { interactive: false },
        "PathView" => &super::tumbler::TumblerItem(true),
        "SwipeView" => &views::PageView(PageLayout::Swipe),
        "StackView" => &views::PageView(PageLayout::Stack),
        "ListView" => &views::Rows(RowLayout::List),
        "StandardListView" => &views::Rows(RowLayout::StandardList),
        "TreeView" => &views::Rows(RowLayout::Tree),
        "GridView" => &views::Rows(RowLayout::Grid),
        "TableView" | "StandardTableView" => &views::Rows(RowLayout::Table),
        "ListRow" => &views::Rows(RowLayout::Row),
        "MediaPlayer" => &const { PreferredSize(Size::new(320.0, 180.0), ItemPaint::Media) },
        "SplitView" => &views::SplitView,
        "BusyIndicator" | "Spinner" => &Indicator,
        "Calendar" | "MonthGrid" | "DayOfWeekRow" | "WeekNumberColumn" => {
            &super::calendar::Calendar
        }
        "Tumbler" => &super::tumbler::TumblerItem(false),
        "TabBar" => &super::selection::Tabs { standalone: true },
        "TabWidget" => &super::selection::Tabs { standalone: false },
        "Page" => &super::page::Page,
        "PageIndicator" => &super::page_indicator::PageIndicator,
        "SwipeDelegate" => &SwipeDelegate,
        "Menu" => &super::menu::Menu,
        "MenuItem" => &super::menu::MenuEntry(super::menu::MenuRole::Action),
        "MenuSeparator" => &super::menu::MenuEntry(super::menu::MenuRole::Separator),
        "Image" => &Image { animated: false },
        "AnimatedImage" => &Image { animated: true },
        kind if is_text_input(kind) => &TextInput,
        _ => &DefaultItem,
    }
}
