use super::{
    ArrangeContext, ContainerPaint, Item, ItemLayout, ItemPaint, ItemRef, MeasureContext,
    PageLayout, ScrollContainer,
};
use crate::{
    Size, dimension, flow,
    grid::Placement,
    linear::{Axis, AxisInfo, Metrics, Padding},
    participates_in_flow,
};
use xrack_runtime::{LayoutDirection, Node, Orientation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentLayout {
    Line(Axis),
    Flow,
    Flex,
    DialogButtons,
    ToolBar,
    RadioGroup,
}

#[derive(Clone, Copy)]
enum Algorithm {
    Line(Axis),
    Flow,
}

impl ContentLayout {
    fn algorithm(self, node: &Node) -> Algorithm {
        match self {
            Self::Line(axis) => Algorithm::Line(axis),
            Self::Flow | Self::Flex => Algorithm::Flow,
            Self::DialogButtons if node.boolean("wrap", true) => Algorithm::Flow,
            Self::DialogButtons => Algorithm::Line(Axis::Horizontal),
            Self::ToolBar if node.boolean("wrap", false) => Algorithm::Flow,
            Self::ToolBar => Algorithm::Line(match node.orientation() {
                Orientation::Horizontal => Axis::Horizontal,
                Orientation::Vertical => Axis::Vertical,
            }),
            Self::RadioGroup
                if node.properties.contains_key("orientation")
                    && node.orientation() == Orientation::Horizontal =>
            {
                Algorithm::Line(Axis::Horizontal)
            }
            Self::RadioGroup => Algorithm::Line(Axis::Vertical),
        }
    }

    pub(crate) fn wraps(self, node: &Node) -> bool {
        matches!(self.algorithm(node), Algorithm::Flow)
    }
}

pub(super) struct LineOrFlow {
    pub(super) content: ContentLayout,
    pub(super) paint: ItemPaint,
}

impl LineOrFlow {
    pub(super) const fn new(content: ContentLayout, paint: ItemPaint) -> Self {
        Self { content, paint }
    }
}

impl super::menu::MenuItem for LineOrFlow {
    fn role(&self) -> super::menu::MenuRole {
        super::menu::MenuRole::Bar
    }
}

impl Item for LineOrFlow {
    fn focus_bounds(&self, item: super::ItemRef<'_, '_>) -> Option<crate::Rect> {
        if !item.layout.visible {
            return None;
        }
        Some(if matches!(self.paint, ItemPaint::GroupHeader { .. }) {
            item.layout.group_header(item.node)
        } else {
            item.layout.bounds
        })
    }

    fn reading_order(&self, node: &Node, _count: usize) -> Option<Vec<usize>> {
        if !matches!(
            self.content,
            ContentLayout::DialogButtons | ContentLayout::Flex
        ) {
            return None;
        }
        let mut slots = (0..node.children.len()).collect::<Vec<_>>();
        slots.sort_by_key(|&slot| crate::linear::item_order(node, &node.children[slot]));
        Some(slots)
    }

    fn menu(&self) -> Option<&dyn super::menu::MenuItem> {
        matches!(self.paint, ItemPaint::MenuBar).then_some(self)
    }

    fn hit_behavior(&self, _node: &Node) -> super::HitBehavior {
        if matches!(self.paint, ItemPaint::GroupHeader { .. }) {
            super::HitBehavior::ChildrenOnly
        } else {
            super::HitBehavior::Normal
        }
    }

    fn event(&self, item: ItemRef<'_, '_>, event: super::ItemEvent) -> Option<super::ItemAction> {
        let super::ItemEvent::HitTest(point) = event else {
            return None;
        };
        let ItemPaint::GroupHeader { expander } = self.paint else {
            return None;
        };
        ((expander || item.node.group_checkable())
            && item
                .layout
                .group_header(item.node)
                .contains(item.layout.local_to_layout(point)))
        .then_some(super::ItemAction::Target)
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::LineOrFlow(self.content)
    }

    fn focusable(&self, node: &Node) -> bool {
        matches!(self.paint, ItemPaint::GroupHeader { expander: true })
            || matches!(self.paint, ItemPaint::GroupHeader { expander: false })
                && node.group_checkable()
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        match self.content.algorithm(cx.node) {
            Algorithm::Line(axis) => cx.layout.measure_line(cx.node, cx.available, axis),
            Algorithm::Flow => cx.layout.measure_flow(cx.node, cx.available),
        }
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        self.paint
    }
}

pub(super) struct Grid;

impl Item for Grid {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Grid
    }

    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::Container(ContainerPaint::Grid)
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        cx.layout.measure_grid(cx.node, cx.available)
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if cx.node.list.is_some() {
            return None;
        }
        Some(cx.layout.arrange_grid(cx.node, cx.bounds, cx.direction))
    }
}

pub(super) struct Row;

impl Item for Row {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Row
    }

    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::Container(ContainerPaint::Row)
    }
}

pub(super) struct Stack;

impl Item for Stack {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Pages(PageLayout::StackLayout)
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let padding = Padding::read(cx.node);
        let inner = padding.inner_size(cx.available);
        let mut x = AxisInfo::flexible(0.0);
        let mut y = AxisInfo::flexible(0.0);
        for child in cx.node.children.iter().filter(|child| child.is_visible()) {
            let measured = cx.layout.metrics(child, inner, None);
            x.min = x.min.max(measured.x.min);
            y.min = y.min.max(measured.y.min);
            x.preferred = x.preferred.max(measured.x.preferred);
            y.preferred = y.preferred.max(measured.y.preferred);
        }
        let padding = padding.size();
        x.min += padding.width;
        y.min += padding.height;
        x.preferred += padding.width;
        y.preferred += padding.height;
        Metrics { x, y }
    }
}

pub(super) struct Loader(pub(super) bool);

impl Item for Loader {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Fill { padded: self.0 }
    }

    fn focusable(&self, _node: &Node) -> bool {
        self.0
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let padding = Padding::read(cx.node);
        let child = cx.node.children.iter().find(|child| child.is_visible());
        let preferred = child.map_or(Size::default(), |child| {
            cx.layout
                .preferred_size(child, padding.inner_size(cx.available), None)
        });
        let padding = padding.size();
        cx.intrinsic(Size::new(
            preferred.width + padding.width,
            preferred.height + padding.height,
        ))
    }
}

pub(super) struct ScrollView(pub(super) ScrollContainer);

impl Item for ScrollView {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Scroll(self.0)
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        match self.0 {
            ScrollContainer::ScrollView => ItemPaint::ScrollView,
            ScrollContainer::Flickable => ItemPaint::Flickable,
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let size = if self.0 == ScrollContainer::ScrollView
            && cx.node.boolean("__picker-viewport", false)
        {
            cx.node.children.first().map_or(Size::default(), |child| {
                cx.layout.preferred_size(child, cx.available, None)
            })
        } else {
            Size::default()
        };
        cx.intrinsic(size)
    }
}

pub(super) struct Pane(pub(super) ItemPaint);

impl Item for Pane {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Pane
    }

    fn hit_behavior(&self, _node: &Node) -> super::HitBehavior {
        super::HitBehavior::BlockBehind
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        self.0
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let node = cx.node;
        let padding = Padding::read(node);
        let mut inner = padding.inner_size(cx.available);
        let content_width = dimension(node, "content-width");
        let content_height = dimension(node, "content-height");
        if cx.assigned_width.is_none()
            && node.relative_length("width", cx.reference.width).is_none()
            && let Some(width) = content_width
        {
            inner.width = inner.width.min(width);
        }
        let mut natural = Size::default();
        for child in node
            .children
            .iter()
            .filter(|child| participates_in_flow(child))
        {
            let size = cx.layout.preferred_size(child, inner, None);
            natural.width = natural
                .width
                .max(child.length("x").unwrap_or(0.0) + size.width);
            natural.height = natural
                .height
                .max(child.length("y").unwrap_or(0.0) + size.height);
        }
        let padding = padding.size();
        cx.intrinsic(Size::new(
            content_width.unwrap_or(natural.width) + padding.width,
            content_height.unwrap_or(natural.height) + padding.height,
        ))
    }
}

pub(super) struct ToolSeparator;

impl Item for ToolSeparator {
    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let padding = dimension(cx.node, "padding").unwrap_or(6.0);
        let thickness = dimension(cx.node, "stroke-width").unwrap_or(1.0);
        let long = AxisInfo::flexible(28.0_f32.max(2.0 * padding + 1.0));
        let short = AxisInfo::fixed(12.0_f32.max(thickness));
        match cx.node.orientation() {
            Orientation::Horizontal => Metrics { x: long, y: short },
            Orientation::Vertical => Metrics { x: short, y: long },
        }
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::ToolSeparator
    }
}

/// Arrange a container's ordinary content after the host has reserved chrome and overlays.
pub(crate) fn arrange_content(
    layout: ItemLayout,
    node: &Node,
    content: crate::Rect,
    direction: LayoutDirection,
    text: &mut impl crate::TextMeasurer,
) -> Option<Vec<Placement>> {
    let algorithm = match layout {
        ItemLayout::LineOrFlow(content) => content.algorithm(node),
        ItemLayout::Overlay(_) => Algorithm::Line(Axis::Vertical),
        _ => return None,
    };
    let bounds = match algorithm {
        Algorithm::Flow => flow::children(node, content, direction, text),
        Algorithm::Line(axis) => crate::linear::children(node, axis, content, direction, text),
    };
    Some(bounds.into_iter().map(Placement::plain).collect())
}
