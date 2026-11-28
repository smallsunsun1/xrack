use super::{ArrangeContext, Item, MeasureContext};
use crate::grid::Placement;
use crate::{LayoutNode, Point, Rect, Size, place_in_direction};
use xrack_runtime::{LayoutDirection, Node, SpinAction};

pub struct SpinGeometry {
    pub text: Rect,
    pub up: Rect,
    pub down: Rect,
}

impl SpinGeometry {
    const DEFAULT_WIDTH: f32 = 28.0;

    pub fn new(bounds: Rect) -> Self {
        Self::with_stepper_width(bounds, Self::DEFAULT_WIDTH, LayoutDirection::LeftToRight)
    }

    pub fn for_node(bounds: Rect, node: &Node) -> Self {
        Self::with_layout_direction(bounds, node, node.layout_direction().unwrap_or_default())
    }

    pub fn for_layout(layout: &LayoutNode, node: &Node) -> Self {
        Self::with_layout_direction(layout.bounds, node, layout.layout_direction)
    }

    pub(crate) fn with_layout_direction(
        bounds: Rect,
        node: &Node,
        direction: LayoutDirection,
    ) -> Self {
        Self::with_stepper_width(bounds, Self::preferred_stepper_size(node).width, direction)
    }

    pub(crate) fn preferred_stepper_size(node: &Node) -> Size {
        let mut size = Size::new(Self::DEFAULT_WIDTH, 0.0);
        for kind in ["SpinUpIndicator", "SpinDownIndicator"] {
            if let Some(part) = node.control_part(kind) {
                size.width = size.width.max(part.length("width").unwrap_or(0.0));
                size.height = size.height.max(2.0 * part.length("height").unwrap_or(0.0));
            }
        }
        size
    }

    fn with_stepper_width(bounds: Rect, preferred: f32, direction: LayoutDirection) -> Self {
        let width = preferred.min(bounds.width * 0.5);
        let half_height = bounds.height * 0.5;
        let x = bounds.x + bounds.width - width;
        let mirror = |rect| place_in_direction(rect, bounds, direction);
        Self {
            text: mirror(Rect::new(
                bounds.x,
                bounds.y,
                bounds.width - width,
                bounds.height,
            )),
            up: mirror(Rect::new(x, bounds.y, width, half_height)),
            down: mirror(Rect::new(x, bounds.y + half_height, width, half_height)),
        }
    }

    pub fn action_at(&self, point: Point) -> Option<SpinAction> {
        if self.up.contains(point) {
            Some(SpinAction::Increase)
        } else if self.down.contains(point) {
            Some(SpinAction::Decrease)
        } else {
            None
        }
    }
}

pub(super) struct SpinBox(pub(super) f32);

impl Item for SpinBox {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn focusable(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> super::ItemPaint {
        super::ItemPaint::SpinEntry
    }

    fn event(
        &self,
        item: super::ItemRef<'_, '_>,
        event: super::ItemEvent,
    ) -> Option<super::ItemAction> {
        let super::ItemEvent::HitTarget(point) = event else {
            return None;
        };
        let action = SpinGeometry::with_layout_direction(
            item.local_rect(),
            item.node,
            item.layout.layout_direction,
        )
        .action_at(point)?;
        Some(super::ItemAction::Control(
            xrack_runtime::ControlAction::Spin(action),
        ))
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if cx.node.children.is_empty() {
            return None;
        }
        let geometry = SpinGeometry::with_layout_direction(cx.bounds, cx.node, cx.direction);
        Some(
            cx.node
                .children
                .iter()
                .map(|child| {
                    Placement::plain(match child.kind {
                        "SpinUpIndicator" => geometry.up,
                        "SpinDownIndicator" => geometry.down,
                        _ => cx.layout.absolute_child(child, cx.bounds),
                    })
                })
                .collect(),
        )
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let stepper = SpinGeometry::preferred_stepper_size(cx.node);
        let size = Size::new(
            self.0 + stepper.width,
            (cx.layout.font_size(cx.node) * 1.4 + 10.0)
                .max(34.0)
                .max(stepper.height),
        );
        cx.intrinsic(size)
    }
}
