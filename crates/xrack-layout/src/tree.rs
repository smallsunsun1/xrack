use crate::Rect;
use xrack_runtime::Node;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeRowGeometry {
    pub indicator: Rect,
    pub content: Rect,
}

impl TreeRowGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Option<Self> {
        let depth = node.int("tree-depth")?.max(0) as f32;
        let indentation = node
            .length("tree-indentation")
            .unwrap_or(16.0)
            .clamp(0.0, 64.0);
        let indicator_width = 22.0_f32.min(bounds.width);
        let indent =
            (4.0 + depth * indentation).min((bounds.width - indicator_width - 16.0).max(0.0));
        let indicator = Rect::new(bounds.x + indent, bounds.y, indicator_width, bounds.height);
        let content = Rect::new(
            bounds.x + indent + indicator_width,
            bounds.y,
            (bounds.width - indent - indicator_width).max(0.0),
            bounds.height,
        );
        Some(Self { indicator, content })
    }
}
