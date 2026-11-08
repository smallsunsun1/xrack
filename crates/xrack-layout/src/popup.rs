use crate::{Rect, Size, TextMeasurer, linear};
use xrack_runtime::{Node, PopupPlacement, Value};

pub(crate) fn bounds(
    node: &Node,
    anchor: Rect,
    viewport: Rect,
    text: &mut impl TextMeasurer,
) -> Rect {
    let area = viewport.inset(8.0);
    let available = Size::new(area.width, area.height);
    let width = node
        .relative_length("width", anchor.width)
        .or_else(|| node.relative_length("preferred-width", anchor.width))
        .unwrap_or_else(|| {
            linear::measure(node, available, text)
                .x
                .preferred
                .max(120.0)
        })
        .clamp(0.0, area.width);
    let height = node
        .relative_length("height", anchor.height)
        .or_else(|| node.relative_length("preferred-height", anchor.height))
        .unwrap_or_else(|| {
            linear::measure_with_width(node, available, Some(width), text)
                .y
                .preferred
                .max(32.0)
        })
        .clamp(0.0, area.height);
    let gap = node.length("anchor-gap").unwrap_or(8.0).clamp(0.0, 64.0);
    let x = anchor.x + node.length("x").unwrap_or(0.0);
    let y = anchor.y + node.length("y").unwrap_or(0.0);
    let placement = match node.properties.get("placement") {
        Some(Value::PopupPlacement(value)) => *value,
        _ => PopupPlacement::Below,
    };
    let below = y + anchor.height + gap;
    let above = y - height - gap;
    let left = x - width - gap;
    let right = x + anchor.width + gap;
    let (x, y) = match placement {
        PopupPlacement::Below => (x, flip(below, above, height, area.y, area.height)),
        PopupPlacement::Above => (x, flip(above, below, height, area.y, area.height)),
        PopupPlacement::Left => (flip(left, right, width, area.x, area.width), y),
        PopupPlacement::Right => (flip(right, left, width, area.x, area.width), y),
        PopupPlacement::Offset => (x, y),
    };
    Rect::new(
        x.clamp(area.x, area.x + (area.width - width)),
        y.clamp(area.y, area.y + (area.height - height)),
        width,
        height,
    )
}

fn flip(preferred: f32, opposite: f32, size: f32, origin: f32, extent: f32) -> f32 {
    let visible = |start: f32| ((start + size).min(origin + extent) - start.max(origin)).max(0.0);
    if visible(opposite) > visible(preferred) {
        opposite
    } else {
        preferred
    }
}
