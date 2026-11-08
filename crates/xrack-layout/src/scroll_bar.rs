use crate::{LayoutNode, Point, Rect, ScrollLayout};
use xrack_runtime::{Node, Orientation, ScrollBarPolicy};

/// An overlay scroll bar whose track and thumb use the viewport's logical pixels.
#[derive(Clone, Copy, Debug)]
pub struct ScrollBarGeometry {
    pub orientation: Orientation,
    pub track: Rect,
    pub thumb: Rect,
    pub scroll: ScrollLayout,
    page: f32,
}

impl LayoutNode {
    /// Bars share the viewport's offsets and do not change content or header layout.
    pub fn scroll_bars(&self, node: &Node) -> impl Iterator<Item = ScrollBarGeometry> {
        if !self.visible
            || self.scroll.is_none() && self.scroll_on(Orientation::Horizontal).is_none()
        {
            return [None; 2].into_iter().flatten();
        }
        let table = self.table.as_deref();
        let viewport = table.map_or(self.bounds, |table| table.body);
        let visible = viewport.intersect(self.clip).inset(2.0);
        let thickness = 10.0_f32
            .min(visible.width * 0.25)
            .min(visible.height * 0.25);
        let axes = [Orientation::Horizontal, Orientation::Vertical];
        let scrolls: [_; 2] = std::array::from_fn(|index| {
            self.scroll_on(axes[index])
                .filter(|scroll| match node.scroll_bar_policy(axes[index]) {
                    ScrollBarPolicy::AsNeeded => scroll.max_offset > 0.0,
                    ScrollBarPolicy::AlwaysOn => true,
                    ScrollBarPolicy::AlwaysOff => false,
                })
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
                        visible.x,
                        visible.y + visible.height - thickness,
                        visible.width - corner,
                        thickness,
                    ),
                    viewport.width,
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
            ScrollBarGeometry::new(orientation, track, page, scroll)
        });
        bars.into_iter().flatten()
    }
}

impl ScrollBarGeometry {
    fn new(orientation: Orientation, track: Rect, page: f32, scroll: ScrollLayout) -> Option<Self> {
        let length = span(track, orientation);
        if length <= 2.0 || page <= 0.0 {
            return None;
        }
        let (length, offset) = if scroll.max_offset > 0.0 {
            let ratio = f64::from(page) / (f64::from(page) + f64::from(scroll.max_offset));
            let thumb_length = (length * ratio as f32)
                .max(20.0_f32.min(length * 0.5))
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

    pub(crate) fn drag(self, point: Point, grab: f32) -> f32 {
        let thumb = span(self.thumb, self.orientation);
        let travel = span(self.track, self.orientation) - thumb;
        let relative = coordinate(point, self.orientation) - origin(self.track, self.orientation);
        ((relative - grab * thumb) / travel).clamp(0.0, 1.0) * self.scroll.max_offset
    }

    pub(crate) fn page_toward(self, point: Point) -> f32 {
        let delta = if coordinate(point, self.orientation) < origin(self.thumb, self.orientation) {
            -self.page
        } else {
            self.page
        };
        (self.scroll.offset + delta).clamp(0.0, self.scroll.max_offset)
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
