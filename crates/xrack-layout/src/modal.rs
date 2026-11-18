use crate::{LayoutNode, Point, Rect, Size, TextMeasurer, font_size, linear, place_in_direction};
use xrack_runtime::{DrawerEdge, LayoutDirection, Node, is_modal_overlay, is_window_overlay};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OverlayIndex {
    pub open: Vec<Vec<usize>>,
    pub modal: Vec<Vec<usize>>,
    pub drawers: Vec<Vec<usize>>,
}

pub fn dialog_title_height(node: &Node) -> f32 {
    title_height(node, font_size(node))
}

pub(crate) fn title_height(node: &Node, size: f32) -> f32 {
    if is_modal_overlay(node.kind) && node.text("title").is_some_and(|title| !title.is_empty()) {
        size * 1.4 + 20.0
    } else {
        0.0
    }
}

pub(crate) fn bounds(node: &Node, viewport: Rect, text: &mut impl TextMeasurer) -> Rect {
    if node.kind == "Drawer" {
        return drawer_bounds(node, viewport);
    }
    let area = viewport.inset(16.0);
    let width = node
        .relative_length("width", viewport.width)
        .or_else(|| node.relative_length("preferred-width", viewport.width))
        .unwrap_or(440.0)
        .clamp(0.0, area.width);
    let height = node
        .relative_length("height", viewport.height)
        .unwrap_or_else(|| {
            let preferred = linear::measure(node, Size::new(width, viewport.height), text)
                .y
                .preferred;
            let extra = picker_parts(node).map_or(0.0, |(_, footer)| {
                let inner = linear::Padding::read(node).inner_rect(Rect::new(
                    0.0,
                    0.0,
                    width,
                    viewport.height,
                ));
                let (inline, stacked) = picker_actions_size(footer, inner, text);
                if inner.width < inline.width {
                    stacked.height - inline.height
                } else {
                    0.0
                }
            });
            (preferred + extra).max(96.0)
        })
        .clamp(0.0, area.height);
    Rect::new(
        area.x + (area.width - width) * 0.5,
        area.y + (area.height - height) * 0.5,
        width,
        height,
    )
}

pub(crate) fn picker_children(
    node: &Node,
    bounds: Rect,
    direction: LayoutDirection,
    text: &mut impl TextMeasurer,
) -> Option<Vec<Rect>> {
    if node.boolean("__picker-actions", false) {
        let (inline, stacked) = picker_actions_size(node, bounds, text);
        let axis = if bounds.height >= stacked.height && bounds.width < inline.width {
            linear::Axis::Vertical
        } else {
            linear::Axis::Horizontal
        };
        return Some(linear::children(node, axis, bounds, direction, text));
    }

    let (picker, footer) = picker_parts(node)?;
    let inner = linear::Padding::read(node).inner_rect(bounds);
    let available = Size::new(inner.width, inner.height);
    let body = linear::measure(picker, available, text);
    let (inline, stacked) = picker_actions_size(footer, inner, text);
    let gap = linear::spacing(node);
    // Leave room for seven touch-sized day columns, or the picker's natural width.
    let minimum_body_width = body.x.preferred.min(7.0 * 44.0);
    let side_by_side = body.y.preferred + inline.height + gap > inner.height
        && stacked.height <= inner.height
        && inner.width - stacked.width - gap >= minimum_body_width;
    if side_by_side {
        let width = inner.width - stacked.width - gap;
        let actions = Rect::new(
            inner.x + width + gap,
            inner.y + inner.height - stacked.height,
            stacked.width,
            stacked.height,
        );
        return Some(vec![
            place_in_direction(
                Rect::new(inner.x, inner.y, width, inner.height),
                inner,
                direction,
            ),
            place_in_direction(actions, inner, direction),
        ]);
    }

    let footer_height = if inner.width < inline.width && inner.height >= stacked.height + gap {
        stacked.height
    } else {
        inline.height.min(inner.height)
    };
    let body_height = (inner.height - footer_height - gap).max(0.0);
    Some(vec![
        Rect::new(inner.x, inner.y, inner.width, body_height),
        Rect::new(
            inner.x,
            inner.y + inner.height - footer_height,
            inner.width,
            footer_height,
        ),
    ])
}

fn picker_parts(node: &Node) -> Option<(&Node, &Node)> {
    let [picker, footer] = node.children.as_slice() else {
        return None;
    };
    footer
        .boolean("__picker-actions", false)
        .then_some((picker, footer))
}

fn picker_actions_size(footer: &Node, bounds: Rect, text: &mut impl TextMeasurer) -> (Size, Size) {
    let available = Size::new(bounds.width, bounds.height);
    let mut inline = Size::default();
    let mut stacked = Size::default();
    for button in &footer.children {
        let metrics = linear::measure(button, available, text);
        inline.width += metrics.x.preferred;
        inline.height = inline.height.max(metrics.y.preferred);
        stacked.width = stacked.width.max(metrics.x.preferred);
        stacked.height += metrics.y.preferred;
    }
    let gap = linear::spacing(footer);
    inline.width += gap;
    stacked.height += gap;
    (inline, stacked)
}

pub(crate) fn drawer_bounds(node: &Node, viewport: Rect) -> Rect {
    let edge = node.drawer_edge();
    let side = matches!(edge, DrawerEdge::Left | DrawerEdge::Right);
    let width = node
        .relative_length("width", viewport.width)
        .or_else(|| node.relative_length("preferred-width", viewport.width))
        .unwrap_or(if side { 320.0 } else { viewport.width })
        .clamp(0.0, viewport.width);
    let height = node
        .relative_length("height", viewport.height)
        .or_else(|| node.relative_length("preferred-height", viewport.height))
        .unwrap_or(if side { viewport.height } else { 240.0 })
        .clamp(0.0, viewport.height);
    let x = match edge {
        DrawerEdge::Left => 0.0,
        DrawerEdge::Right => viewport.width - width,
        _ => node
            .length("x")
            .unwrap_or(0.0)
            .clamp(0.0, viewport.width - width),
    };
    let y = match edge {
        DrawerEdge::Top => 0.0,
        DrawerEdge::Bottom => viewport.height - height,
        _ => node
            .length("y")
            .unwrap_or(0.0)
            .clamp(0.0, viewport.height - height),
    };
    Rect::new(viewport.x + x, viewport.y + y, width, height)
}

pub(crate) fn drawer_edge_distance(edge: DrawerEdge, bounds: Rect, point: Point) -> f32 {
    match edge {
        DrawerEdge::Left => point.x - bounds.x,
        DrawerEdge::Right => bounds.x + bounds.width - point.x,
        DrawerEdge::Top => point.y - bounds.y,
        DrawerEdge::Bottom => bounds.y + bounds.height - point.y,
    }
}

pub(crate) fn drawer_extent(edge: DrawerEdge, bounds: Rect) -> f32 {
    match edge {
        DrawerEdge::Left | DrawerEdge::Right => bounds.width,
        DrawerEdge::Top | DrawerEdge::Bottom => bounds.height,
    }
}

pub(crate) fn drawer_margin(node: &Node, bounds: Rect) -> f32 {
    node.length("drag-margin")
        .unwrap_or(0.0)
        .clamp(0.0, 64.0)
        .min(drawer_extent(node.drawer_edge(), bounds) * 0.5)
}

/// Visual handle within the swipe-sensitive inner edge of an open drawer.
pub fn drawer_grip(node: &Node, bounds: Rect) -> Option<Rect> {
    if node.kind != "Drawer" {
        return None;
    }
    let margin = drawer_margin(node, bounds);
    if margin < 4.0 {
        return None;
    }
    let side = opposite(node.drawer_edge());
    let horizontal = matches!(side, DrawerEdge::Top | DrawerEdge::Bottom);
    let length = if horizontal {
        bounds.width
    } else {
        bounds.height
    }
    .min(28.0);
    let depth = margin * 0.5;
    let grip = if horizontal {
        let y = if side == DrawerEdge::Top {
            bounds.y + depth
        } else {
            bounds.y + bounds.height - depth
        };
        Rect::new(
            bounds.x + (bounds.width - length) * 0.5,
            y - 1.5,
            length,
            3.0,
        )
    } else {
        let x = if side == DrawerEdge::Left {
            bounds.x + depth
        } else {
            bounds.x + bounds.width - depth
        };
        Rect::new(
            x - 1.5,
            bounds.y + (bounds.height - length) * 0.5,
            3.0,
            length,
        )
    };
    Some(grip)
}

pub(crate) fn opposite(edge: DrawerEdge) -> DrawerEdge {
    match edge {
        DrawerEdge::Left => DrawerEdge::Right,
        DrawerEdge::Right => DrawerEdge::Left,
        DrawerEdge::Top => DrawerEdge::Bottom,
        DrawerEdge::Bottom => DrawerEdge::Top,
    }
}

pub(crate) fn index_overlays(scene: &Node, layout: &mut LayoutNode) {
    fn visit(
        scene: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        overlays: &mut OverlayIndex,
    ) {
        if Node::SCROLL_PARTS.contains(&scene.kind) {
            return;
        }
        if scene.kind == "Drawer"
            && scene.boolean("visible", true)
            && layout.enabled
            && scene
                .length("drag-margin")
                .is_some_and(|margin| margin > 0.0)
        {
            overlays.drawers.push(path.clone());
        }
        if !layout.visible || !scene.group_content_enabled() {
            return;
        }
        if is_window_overlay(scene.kind) {
            overlays.open.push(path.clone());
            if scene.is_modal() {
                overlays.modal.push(path.clone());
            }
        }
        for (index, child, item) in layout.children_in_scene_order(scene) {
            path.push(index);
            visit(child, item, path, overlays);
            path.pop();
        }
    }
    let mut overlays = OverlayIndex::default();
    visit(scene, layout, &mut Vec::new(), &mut overlays);
    layout.overlays =
        (!overlays.open.is_empty() || !overlays.drawers.is_empty()).then(|| Box::new(overlays));
}
