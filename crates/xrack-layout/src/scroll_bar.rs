use crate::{LayoutContext, LayoutNode, Point, Rect, ScrollLayout, TextMeasurer};
use std::collections::BTreeMap;
use xrack_runtime::{LayoutDirection, Node, Orientation, ScrollBarPolicy, ScrollBarRange, Value};

/// An overlay scroll bar whose track and thumb use the viewport's logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarGeometry {
    pub orientation: Orientation,
    pub track: Rect,
    pub thumb: Rect,
    pub scroll: ScrollLayout,
    pub page: f32,
    pub step: f32,
    reversed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttachedScrollBar {
    pub target: Vec<usize>,
    pub geometry: ScrollBarGeometry,
}

/// Resolve handles after laying out both the control and its viewport.
pub(crate) fn attach(scene: &Node, layout: &mut LayoutNode, text: &mut impl TextMeasurer) {
    if !scene.contains_scene_feature(xrack_runtime::SceneFeature::AttachedScrollBar) {
        return;
    }
    fn index<'a>(
        node: &'a Node,
        item: &LayoutNode,
        path: &mut Vec<usize>,
        viewports: &mut BTreeMap<usize, Vec<usize>>,
        controls: &mut Vec<(&'a Node, Vec<usize>)>,
    ) {
        if !item.visible
            || !node.group_content_enabled()
            || !node.contains_scene_feature(xrack_runtime::SceneFeature::ScrollEndpoint)
        {
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
    let window = layout.bounds;
    let offsets = BTreeMap::new();
    for (node, path) in controls {
        let Some((attached, enabled)) = resolve_attachment(node, &path, layout, &viewports) else {
            continue;
        };
        let geometry = attached.geometry;
        let Some(item) = path
            .iter()
            .try_fold(&mut *layout, |item, &index| item.child_at_mut(index))
        else {
            continue;
        };
        item.enabled &= enabled;
        item.details_mut().attached_scroll_bar = Some(Box::new(attached));
        if !node.has_scroll_parts() {
            continue;
        }
        let mut context = LayoutContext {
            defer_styled_text: false,
            text,
            layout_direction: item.direction_override,
            offsets: &offsets,
            input: None,
            path,
            row_key: node.routing_row_key(),
            anchor: None,
            resolved_list_scroll: None,
            resolved_table: None,
            cache_table_sizes: false,
            row_layouts: None,
            ancestor_shapes: item.ancestor_shapes.clone(),
            space: item.space.clone(),
            viewport: window,
        };
        // The two visual parts are placed after resolving the viewport and use
        // exactly the rectangles used for dragging and track paging.
        for (index, part) in node.children.iter().enumerate() {
            let bounds = match part.kind {
                "ScrollTrack" => geometry.track,
                "ScrollThumb" => geometry.thumb,
                _ => continue,
            };
            context.path.push(index);
            item.children[index] = context.place(
                part,
                bounds,
                item.clip.intersect(bounds),
                true,
                item.enabled,
                None,
            );
            context.path.pop();
        }
    }
}

fn resolve_attachment(
    node: &Node,
    path: &[usize],
    layout: &LayoutNode,
    viewports: &BTreeMap<usize, Vec<usize>>,
) -> Option<(AttachedScrollBar, bool)> {
    let target = viewports.get(&node.scroll_target.as_ref()?.identity())?;
    if layout.overlay_index().is_some_and(|overlays| {
        overlays
            .modal
            .iter()
            .any(|modal| path.starts_with(modal) != target.starts_with(modal))
    }) {
        return None;
    }
    let viewport = layout.at_path(target)?;
    let axis = node.orientation();
    let scroll = viewport.scroll_on(axis)?;
    let page = span(viewport.scroll_viewport(axis), axis);
    if page <= 0.0 || !shows_bar(control_policy(node), scroll) {
        return None;
    }
    let item = layout.at_path(path)?;
    let mut geometry = ScrollBarGeometry::new(
        axis,
        item.bounds,
        page,
        scroll,
        minimum_size(node),
        viewport.scroll_axis_reversed(axis),
    )?;
    geometry.step =
        ScrollBarGeometry::viewport_step(page, scroll.max_offset, node.number("step-size"));
    Some((
        AttachedScrollBar {
            target: target.clone(),
            geometry,
        },
        viewport.enabled,
    ))
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

pub(crate) fn standalone_geometry(
    node: &Node,
    bounds: Rect,
    direction: LayoutDirection,
) -> Option<ScrollBarGeometry> {
    let range = ScrollBarRange::from_node(node);
    let scroll = ScrollLayout::new(range.position as f32, range.maximum() as f32);
    if !shows_bar(control_policy(node), scroll) {
        return None;
    }
    let orientation = node.orientation();
    let mut geometry = ScrollBarGeometry::new(
        orientation,
        bounds,
        range.size as f32,
        scroll,
        minimum_size(node),
        orientation == Orientation::Horizontal && direction == LayoutDirection::RightToLeft,
    )?;
    geometry.step = range.step as f32;
    Some(geometry)
}

pub(crate) fn scroll_bar_thickness(bounds: Rect) -> f32 {
    10.0_f32.min(bounds.width * 0.25).min(bounds.height * 0.25)
}

impl LayoutNode {
    pub fn attached_scroll_geometry(&self) -> Option<ScrollBarGeometry> {
        self.attached_scroll_bar().map(|bar| bar.geometry)
    }

    pub fn scroll_control_geometry(&self, node: &Node) -> Option<ScrollBarGeometry> {
        if !self.visible || !matches!(node.kind, "ScrollBar" | "ScrollIndicator") {
            return None;
        }
        if node.scroll_target.is_some() {
            return self.attached_scroll_geometry();
        }
        standalone_geometry(node, self.bounds, self.layout_direction)
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
        if node.kind == "ScrollView" && node.boolean("reserve-scroll-bar-space", false) {
            let side = (self.bounds.width - viewport.width).max(0.0);
            let bottom = (self.bounds.height - viewport.height).max(0.0);
            let mirrored = self.layout_direction == LayoutDirection::RightToLeft;
            let tracks = [
                Rect::new(
                    viewport.x,
                    viewport.y + viewport.height,
                    viewport.width,
                    bottom,
                ),
                Rect::new(
                    if mirrored {
                        self.bounds.x
                    } else {
                        viewport.x + viewport.width
                    },
                    viewport.y,
                    side,
                    viewport.height,
                ),
            ];
            let bars = [Orientation::Horizontal, Orientation::Vertical].map(|axis| {
                let index = usize::from(axis == Orientation::Vertical);
                let track = tracks[index];
                ScrollBarGeometry::new(
                    axis,
                    track,
                    span(viewport, axis),
                    self.scroll_on(axis)?,
                    None,
                    self.scroll_axis_reversed(axis),
                )
            });
            return bars.into_iter().flatten();
        }
        let horizontal_viewport = self.scroll_viewport(Orientation::Horizontal);
        let horizontal_visible = horizontal_viewport.intersect(self.clip).inset(2.0);
        let visible = viewport.intersect(self.clip).inset(2.0);
        let thickness = scroll_bar_thickness(visible);
        let mirrored = self.layout_direction == LayoutDirection::RightToLeft;
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
                        horizontal_visible.x + if mirrored { corner } else { 0.0 },
                        visible.y + visible.height - thickness,
                        horizontal_visible.width - corner,
                        thickness,
                    ),
                    horizontal_viewport.width,
                ),
                Orientation::Vertical => (
                    Rect::new(
                        if mirrored {
                            visible.x
                        } else {
                            visible.x + visible.width - thickness
                        },
                        visible.y,
                        thickness,
                        visible.height - corner,
                    ),
                    viewport.height,
                ),
            };
            ScrollBarGeometry::new(
                orientation,
                track,
                page,
                scroll,
                None,
                self.scroll_axis_reversed(orientation),
            )
        });
        bars.into_iter().flatten()
    }
}

impl ScrollBarGeometry {
    pub const STEP: f32 = 28.0;

    pub(crate) fn viewport_step(page: f32, scrollable: f32, configured: Option<f64>) -> f32 {
        let Some(step) = configured.filter(|step| step.is_finite()) else {
            return Self::STEP;
        };
        let extent = f64::from(page) + f64::from(scrollable);
        (step.clamp(0.0, 1.0) * extent).min(f64::from(f32::MAX)) as f32
    }

    fn new(
        orientation: Orientation,
        track: Rect,
        page: f32,
        scroll: ScrollLayout,
        minimum_size: Option<f32>,
        reversed: bool,
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
            let ratio = scroll.offset / scroll.max_offset;
            (
                thumb_length,
                (length - thumb_length) * if reversed { 1.0 - ratio } else { ratio },
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
            reversed,
        })
    }

    /// The thumb's start and length as fractions of the track, in physical order.
    pub(crate) fn normalized_thumb(self) -> (f64, f64) {
        let length = f64::from(span(self.track, self.orientation));
        let position = f64::from(origin(self.thumb, self.orientation))
            - f64::from(origin(self.track, self.orientation));
        let size = (f64::from(span(self.thumb, self.orientation)) / length).clamp(0.0, 1.0);
        ((position / length).clamp(0.0, 1.0 - size), size)
    }

    #[doc(hidden)]
    pub fn reversed(self) -> bool {
        self.reversed
    }

    pub(crate) fn logical_delta(self, delta: f32) -> f32 {
        if self.reversed { -delta } else { delta }
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
        let fraction = f64::from((relative - grab * thumb) / travel).clamp(0.0, 1.0);
        if self.reversed {
            1.0 - fraction
        } else {
            fraction
        }
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
        (self.scroll.offset + self.logical_delta(delta)).clamp(0.0, self.scroll.max_offset)
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
