use crate::{Point, Rect, Size};
use xrack_runtime::{Node, Orientation, page_indicator_count, page_indicator_index};

const MAX_MARKERS: usize = 64;

/// A logical page index and its marker in the current viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMarker {
    pub index: usize,
    pub center: Point,
    pub diameter: f32,
    pub selected: bool,
}

/// A bounded window of markers. Painting and pointer selection use the same slots.
pub struct PageIndicatorGeometry {
    track: Rect,
    orientation: Orientation,
    first: usize,
    len: usize,
    count: usize,
    selected: Option<usize>,
    step: f32,
    diameter: f32,
}

impl PageIndicatorGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Option<Self> {
        let count = page_indicator_count(node);
        if count == 0
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || bounds.width <= 0.0
            || bounds.height <= 0.0
        {
            return None;
        }
        let orientation = node.orientation();
        let (main, cross) = match orientation {
            Orientation::Horizontal => (bounds.width, bounds.height),
            Orientation::Vertical => (bounds.height, bounds.width),
        };
        let (preferred_step, preferred_cross, dot) = sizing(node);
        let capacity = (main / preferred_step).floor().max(1.0) as usize;
        let len = count.min(capacity).min(MAX_MARKERS);
        let step = preferred_step.min(main / len as f32);
        let diameter = dot.min(step).min(cross);
        let selected = page_indicator_index(node);
        let first = selected
            .unwrap_or(0)
            .saturating_sub(len / 2)
            .min(count - len);
        let main = step * len as f32;
        let cross = cross.min(preferred_cross);
        let (width, height) = match orientation {
            Orientation::Horizontal => (main, cross),
            Orientation::Vertical => (cross, main),
        };
        let track = Rect::new(
            bounds.x + (bounds.width - width) * 0.5,
            bounds.y + (bounds.height - height) * 0.5,
            width,
            height,
        );
        Some(Self {
            track,
            orientation,
            first,
            len,
            count,
            selected,
            step,
            diameter,
        })
    }

    pub fn marker_at(&self, point: Point) -> Option<usize> {
        if !self.track.contains(point) {
            return None;
        }
        let distance = match self.orientation {
            Orientation::Horizontal => point.x - self.track.x,
            Orientation::Vertical => point.y - self.track.y,
        };
        Some(self.first + ((distance / self.step) as usize).min(self.len - 1))
    }

    pub fn markers(&self) -> impl Iterator<Item = PageMarker> + '_ {
        (0..self.len).map(|slot| {
            let index = self.first + slot;
            let distance = self.step * (slot as f32 + 0.5);
            let center = match self.orientation {
                Orientation::Horizontal => Point {
                    x: self.track.x + distance,
                    y: self.track.y + self.track.height * 0.5,
                },
                Orientation::Vertical => Point {
                    x: self.track.x + self.track.width * 0.5,
                    y: self.track.y + distance,
                },
            };
            let selected = self.selected == Some(index);
            let edge =
                (slot == 0 && self.first > 0) || (slot + 1 == self.len && index + 1 < self.count);
            let diameter = if edge && !selected && self.len > 1 {
                self.diameter * 0.65
            } else {
                self.diameter
            };
            PageMarker {
                index,
                center,
                diameter,
                selected,
            }
        })
    }

    pub(crate) fn preferred_size(node: &Node) -> Size {
        let (step, cross, _) = sizing(node);
        let main = step * page_indicator_count(node).min(12) as f32;
        match node.orientation() {
            Orientation::Horizontal => Size::new(main, cross),
            Orientation::Vertical => Size::new(cross, main),
        }
    }
}

fn sizing(node: &Node) -> (f32, f32, f32) {
    let dot = node.length("dot-size").unwrap_or(8.0).clamp(2.0, 32.0);
    let spacing = node.length("spacing").unwrap_or(8.0).clamp(0.0, 48.0);
    let (min_step, min_cross) = if node.boolean("interactive", false) {
        (18.0, 22.0)
    } else {
        (6.0, 16.0)
    };
    (
        (dot + spacing).max(min_step),
        (dot + 6.0).max(min_cross),
        dot,
    )
}
