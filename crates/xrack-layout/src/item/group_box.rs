use crate::{
    Rect, Size, TextMeasurer, font_size,
    label::{LabelPlan, TitleLayout},
    linear::Padding,
};
use xrack_runtime::{LayoutDirection, Node};

pub(crate) fn title_height(node: &Node, size: f32) -> f32 {
    if node.kind == "Expander"
        || node.kind == "GroupBox"
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
    header_bounds(node, bounds, title_height(node, size))
}

pub(crate) fn header_bounds(node: &Node, bounds: Rect, height: f32) -> Rect {
    let inner = Padding::read(node).inner_rect(bounds);
    Rect::new(inner.x, inner.y, inner.width, height.min(inner.height))
}

pub(crate) fn title_layout(
    node: &Node,
    available: Size,
    direction: LayoutDirection,
    text: &mut impl TextMeasurer,
) -> Option<TitleLayout> {
    if text.text_scale() <= 1.0 || !matches!(node.kind, "GroupBox" | "Expander") {
        return None;
    }
    let title = node.text("title").filter(|title| !title.is_empty())?;
    let inner = Padding::read(node).inner_size(available);
    let has_body = !node.expander_collapsed() && node.children.iter().any(Node::is_visible);
    let body = if has_body { 44.0 } else { 0.0 };
    let budget = inner.height - body - 4.0;
    let preferred_size = text.font_size(node);
    let standard_size = preferred_size / text.text_scale();
    let size = preferred_size.min((budget / 1.4).max(standard_size));
    let line_height = size * 1.4;
    // Padding can round a previously measured two-line height down by a fraction.
    let max_lines = if budget + 0.01 >= 2.0 * line_height {
        2
    } else {
        1
    };
    let indicator_width = if node.kind == "Expander" {
        23.0
    } else if node.group_checkable() {
        31.0
    } else {
        0.0
    };
    let mut plan = LabelPlan::leading_title(
        node,
        title,
        (inner.width - indicator_width).max(0.0),
        size,
        direction,
        max_lines,
        text,
    );
    let height = (plan.size.height.max(line_height) + 4.0)
        .max(24.0)
        .min(inner.height);
    for line in &mut plan.lines {
        if direction == LayoutDirection::LeftToRight {
            line.origin.x += indicator_width;
        }
        line.origin.y += ((height - plan.size.height) * 0.5).max(0.0);
    }
    Some(TitleLayout {
        height,
        font_size: size,
        lines: plan.lines,
    })
}
