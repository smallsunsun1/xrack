use crate::{LayoutNode, Point, Rect, ScrollLayout};
use std::collections::BTreeMap;
use xrack_runtime::{Node, Orientation, ScrollBarPolicy, ScrollBarRange, Value};

/// An overlay scroll bar whose track and thumb use the viewport's logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarGeometry {
    pub orientation: Orientation,
    pub track: Rect,
    pub thumb: Rect,
    pub scroll: ScrollLayout,
    pub page: f32,
    pub step: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttachedScrollBar {
    pub target: Vec<usize>,
    pub geometry: ScrollBarGeometry,
}

/// Resolve handles after laying out both the control and its viewport.
pub(crate) fn attach(scene: &Node, layout: &mut LayoutNode) {
    fn index<'a>(
        node: &'a Node,
        item: &LayoutNode,
        path: &mut Vec<usize>,
        viewports: &mut BTreeMap<usize, Vec<usize>>,
        controls: &mut Vec<(&'a Node, Vec<usize>)>,
    ) {
        if !item.visible || !node.group_content_enabled() {
            return;
        }
        if let Some(handle) = &node.viewport_handle {
            viewports.insert(handle.identity(), path.clone());
        }
        if node.scroll_target.is_some() {
            controls.push((node, path.clone()));
        }
        for (index_in_parent, child, child_layout) in item.children_in_scene_order(node) {
            path.push(index_in_parent);
            index(child, child_layout, path, viewports, controls);
            path.pop();
        }
    }

    let mut viewports = BTreeMap::new();
    let mut controls = Vec::new();
    index(
        scene,
        layout,
        &mut Vec::new(),
        &mut viewports,
        &mut controls,
    );
    for (node, path) in controls {
        let Some(target) = node
            .scroll_target
            .as_ref()
            .and_then(|handle| viewports.get(&handle.identity()))
        else {
            continue;
        };
        if layout.overlays.as_ref().is_some_and(|overlays| {
            overlays
                .modal
                .iter()
                .any(|modal| path.starts_with(modal) != target.starts_with(modal))
        }) {
            continue;
        }
        let viewport = layout.at_path(target).expect("indexed viewport path");
        let axis = node.orientation();
        let Some(scroll) = viewport.scroll_on(axis) else {
            continue;
        };
        if !shows_bar(control_policy(node), scroll) {
            continue;
        }
        let page = viewport.scroll_viewport(axis);
        let enabled = viewport.enabled;
        let item = path
            .iter()
            .try_fold(&mut *layout, |item, &index| item.child_at_mut(index))
            .expect("indexed scroll control path");
        let page = span(page, axis);
        if page > 0.0
            && let Some(mut geometry) =
                ScrollBarGeometry::new(axis, item.bounds, page, scroll, minimum_size(node))
        {
            if let Some(step) = node.number("step-size") {
                let extent = f64::from(page) + f64::from(scroll.max_offset);
                geometry.step = (step.clamp(0.0, 1.0) * extent).min(f64::from(f32::MAX)) as f32;
            }
            item.enabled &= enabled;
            item.attached_scroll_bar = Some(Box::new(AttachedScrollBar {
                target: target.clone(),
                geometry,
            }));
        }
    }
}

fn control_policy(node: &Node) -> ScrollBarPolicy {
    match node.properties.get("policy") {
        Some(Value::ScrollBarPolicy(policy)) => *policy,
        _ => ScrollBarPolicy::AsNeeded,
    }
}

fn minimum_size(node: &Node) -> Option<f32> {
    node.number("minimum-size")
        .map(|size| size.clamp(0.0, 1.0) as f32)
}

fn shows_bar(policy: ScrollBarPolicy, scroll: ScrollLayout) -> bool {
    match policy {
        ScrollBarPolicy::AsNeeded => scroll.max_offset > 0.0,
        ScrollBarPolicy::AlwaysOn => true,
        ScrollBarPolicy::AlwaysOff => false,
    }
}

impl LayoutNode {
    pub fn attached_scroll_geometry(&self) -> Option<ScrollBarGeometry> {
        self.attached_scroll_bar.as_ref().map(|bar| bar.geometry)
    }

    pub fn scroll_control_geometry(&self, node: &Node) -> Option<ScrollBarGeometry> {
        if !self.visible || !matches!(node.kind, "ScrollBar" | "ScrollIndicator") {
            return None;
        }
        if node.scroll_target.is_some() {
            return self.attached_scroll_geometry();
        }
        let range = ScrollBarRange::from_node(node);
        let scroll = ScrollLayout::new(range.position as f32, range.maximum() as f32);
        if !shows_bar(control_policy(node), scroll) {
            return None;
        }
        let mut geometry = ScrollBarGeometry::new(
            node.orientation(),
            self.bounds,
            range.size as f32,
            scroll,
            minimum_size(node),
        )?;
        geometry.step = range.step as f32;
        Some(geometry)
    }

    /// Bars share the viewport's offsets and do not change content or header layout.
    pub fn scroll_bars(&self, node: &Node) -> impl Iterator<Item = ScrollBarGeometry> {
        if matches!(node.kind, "ScrollBar" | "ScrollIndicator") {
            return [self.scroll_control_geometry(node), None]
                .into_iter()
                .flatten();
        }
        if node.kind == "Flickable"
            || !self.visible
            || self.scroll.is_none() && self.scroll_on(Orientation::Horizontal).is_none()
        {
            return [None; 2].into_iter().flatten();
        }
        let viewport = self.scroll_viewport(Orientation::Vertical);
        let horizontal_viewport = self.scroll_viewport(Orientation::Horizontal);
        let horizontal_visible = horizontal_viewport.intersect(self.clip).inset(2.0);
        let visible = viewport.intersect(self.clip).inset(2.0);
        let thickness = 10.0_f32
            .min(visible.width * 0.25)
            .min(visible.height * 0.25);
        let axes = [Orientation::Horizontal, Orientation::Vertical];
        let scrolls: [_; 2] = std::array::from_fn(|index| {
            self.scroll_on(axes[index])
                .filter(|_| {
                    axes[index] != Orientation::Horizontal || horizontal_visible.width > 0.0
                })
                .filter(|&scroll| shows_bar(node.scroll_bar_policy(axes[index]), scroll))
        });
        let bars: [_; 2] = std::array::from_fn(|index| {
            if thickness < 1.0 {
                return None;
            }
            let orientation = axes[index];
            let scroll = scrolls[index]?;
            let corner = if scrolls[1 - index].is_some() {
                thickness
            } else {
                0.0
            };
            let (track, page) = match orientation {
                Orientation::Horizontal => (
                    Rect::new(
                        horizontal_visible.x,
                        visible.y + visible.height - thickness,
                        horizontal_visible.width - corner,
                        thickness,
                    ),
                    horizontal_viewport.width,
                ),
                Orientation::Vertical => (
                    Rect::new(
                        visible.x + visible.width - thickness,
                        visible.y,
                        thickness,
                        visible.height - corner,
                    ),
                    viewport.height,
                ),
            };
            ScrollBarGeometry::new(orientation, track, page, scroll, None)
        });
        bars.into_iter().flatten()
    }
}

impl ScrollBarGeometry {
    pub const STEP: f32 = 28.0;

    fn new(
        orientation: Orientation,
        track: Rect,
        page: f32,
        scroll: ScrollLayout,
        minimum_size: Option<f32>,
    ) -> Option<Self> {
        let length = span(track, orientation);
        if length <= 2.0 || track.width <= 0.0 || track.height <= 0.0 {
            return None;
        }
        let (length, offset) = if scroll.max_offset > 0.0 {
            let ratio = f64::from(page) / (f64::from(page) + f64::from(scroll.max_offset));
            let minimum = minimum_size.map_or(20.0_f32.min(length * 0.5), |size| size * length);
            let thumb_length = (length * ratio as f32)
                .max(minimum.max(1.0))
                .min(length - 1.0);
            (
                thumb_length,
                (length - thumb_length) * (scroll.offset / scroll.max_offset),
            )
        } else {
            (length, 0.0)
        };
        let thumb = match orientation {
            Orientation::Horizontal => Rect::new(track.x + offset, track.y, length, track.height),
            Orientation::Vertical => Rect::new(track.x, track.y + offset, track.width, length),
        };
        Some(Self {
            orientation,
            track,
            thumb,
            scroll,
            page,
            step: Self::STEP,
        })
    }

    pub fn scrollable(self) -> bool {
        self.scroll.max_offset > 0.0
    }

    pub(crate) fn grab(self, point: Point) -> Option<f32> {
        self.thumb.contains(point).then(|| {
            (coordinate(point, self.orientation) - origin(self.thumb, self.orientation))
                / span(self.thumb, self.orientation)
        })
    }

    pub(crate) fn drag_fraction(self, point: Point, grab: f32) -> f64 {
        let thumb = span(self.thumb, self.orientation);
        let travel = span(self.track, self.orientation) - thumb;
        let relative = coordinate(point, self.orientation) - origin(self.track, self.orientation);
        f64::from((relative - grab * thumb) / travel).clamp(0.0, 1.0)
    }

    pub(crate) fn page_toward(self, point: Point) -> f32 {
        let page = if self.page > 0.0 {
            self.page
        } else {
            self.step
        };
        let delta = if coordinate(point, self.orientation) < origin(self.thumb, self.orientation) {
            -page
        } else {
            page
        };
        (self.scroll.offset + delta).clamp(0.0, self.scroll.max_offset)
    }
}

pub(crate) fn snap_fraction(fraction: f64, step: f64) -> f64 {
    let step = step.clamp(0.0, 1.0);
    if step <= f64::EPSILON || fraction == 0.0 || fraction == 1.0 {
        return fraction;
    }
    let lower = (fraction / step).floor() * step;
    let upper = (lower + step).min(1.0);
    if fraction - lower < upper - fraction {
        lower
    } else {
        upper
    }
}

fn coordinate(point: Point, orientation: Orientation) -> f32 {
    match orientation {
        Orientation::Horizontal => point.x,
        Orientation::Vertical => point.y,
    }
}

fn origin(rect: Rect, orientation: Orientation) -> f32 {
    coordinate(
        Point {
            x: rect.x,
            y: rect.y,
        },
        orientation,
    )
}

fn span(rect: Rect, orientation: Orientation) -> f32 {
    match orientation {
        Orientation::Horizontal => rect.width,
        Orientation::Vertical => rect.height,
    }
}
