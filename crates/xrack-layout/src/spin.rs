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
