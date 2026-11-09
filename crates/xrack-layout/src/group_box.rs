use crate::{Rect, font_size, linear::Padding};
use xrack_runtime::Node;

pub(crate) fn title_height(node: &Node, size: f32) -> f32 {
    if node.kind == "GroupBox"
        && (node.group_checkable() || node.text("title").is_some_and(|title| !title.is_empty()))
    {
        (size * 1.4 + 4.0).max(24.0)
    } else {
        0.0
    }
}

/// The header remains enabled when the content is disabled.
pub fn group_header(node: &Node, bounds: Rect) -> Rect {
    header_with_font_size(node, bounds, font_size(node))
}

pub(crate) fn header_with_font_size(node: &Node, bounds: Rect, size: f32) -> Rect {
    let inner = Padding::read(node).inner_rect(bounds);
    Rect::new(
        inner.x,
        inner.y,
        inner.width,
        title_height(node, size).min(inner.height),
    )
}
