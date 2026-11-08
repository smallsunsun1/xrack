use crate::{LayoutNode, Point, Rect, Size, TextMeasurer, font_size, linear};
use xrack_runtime::{DrawerEdge, Node, is_modal_overlay};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OverlayIndex {
    pub open: Vec<Vec<usize>>,
    pub drawers: Vec<Vec<usize>>,
}

pub fn dialog_title_height(node: &Node) -> f32 {
    if is_modal_overlay(node.kind) && node.text("title").is_some_and(|title| !title.is_empty()) {
        font_size(node) * 1.4 + 20.0
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
            linear::measure(node, Size::new(width, viewport.height), text)
                .y
                .preferred
                .max(96.0)
        })
        .clamp(0.0, area.height);
    Rect::new(
        area.x + (area.width - width) * 0.5,
        area.y + (area.height - height) * 0.5,
        width,
        height,
    )
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

pub(crate) fn index_dialogs(scene: &Node, layout: &mut LayoutNode) {
    fn visit(
        scene: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        overlays: &mut OverlayIndex,
    ) {
        if scene.kind == "Drawer"
            && scene.boolean("visible", true)
            && layout.enabled
            && scene
                .length("drag-margin")
                .is_some_and(|margin| margin > 0.0)
        {
            overlays.drawers.push(path.clone());
        }
        if !layout.visible {
            return;
        }
        if is_modal_overlay(scene.kind) {
            overlays.open.push(path.clone());
        }
        for (index, (child, item)) in scene.children.iter().zip(&layout.children).enumerate() {
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
