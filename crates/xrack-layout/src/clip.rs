use crate::{Affine, Point, Rect, transform::TransformSpace};
use std::sync::Arc;
use xrack_runtime::{Node, is_button, is_pane, is_text_input};

/// A child clip in window coordinates, with corners clockwise from the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedClip {
    pub bounds: Rect,
    pub radii: [f32; 4],
}

impl RoundedClip {
    pub(crate) fn content_box(node: &Node, bounds: Rect) -> Self {
        let border = node
            .length("border-width")
            .unwrap_or(if node.kind == "ControlFrame" {
                1.0
            } else {
                0.0
            })
            .max(0.0);
        let inner = bounds.inset(border);
        let radii = corner_radii(node, bounds).map(|radius| (radius - border).max(0.0));
        Self {
            bounds: inner,
            radii: normalize_radii(inner, radii),
        }
    }

    pub(crate) fn contains(self, point: Point) -> bool {
        if !self.bounds.contains(point) {
            return false;
        }
        let [tl, tr, br, bl] = self.radii;
        let y = point.y - self.bounds.y;
        let left = inset_over(y, y, self.bounds.height, tl, bl);
        let right = inset_over(y, y, self.bounds.height, tr, br);
        point.x >= self.bounds.x + left && point.x <= self.bounds.x + self.bounds.width - right
    }

    fn clip_rect(self, rect: Rect) -> Rect {
        let rect = self.bounds.intersect(rect);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return rect;
        }
        let [tl, tr, br, bl] = self.radii;
        let x = rect.x - self.bounds.x;
        let y = rect.y - self.bounds.y;
        let left = inset_over(y, y + rect.height, self.bounds.height, tl, bl);
        let right = inset_over(y, y + rect.height, self.bounds.height, tr, br);
        let top = inset_over(x, x + rect.width, self.bounds.width, tl, tr);
        let bottom = inset_over(x, x + rect.width, self.bounds.width, bl, br);
        rect.intersect(Rect::new(
            self.bounds.x + left,
            self.bounds.y + top,
            self.bounds.width - left - right,
            self.bounds.height - top - bottom,
        ))
    }
}

// Minimum inset along a span. A span touching the flat part has no inset.
fn inset_over(start: f32, end: f32, extent: f32, before: f32, after: f32) -> f32 {
    let (radius, distance) = if end < before {
        (before, before - end)
    } else if start > extent - after {
        (after, start - (extent - after))
    } else {
        return 0.0;
    };
    radius - (radius * radius - distance * distance).max(0.0).sqrt()
}

#[derive(Debug, PartialEq)]
pub(crate) struct ClipChain {
    pub(crate) shape: RoundedClip,
    pub(crate) parent: Option<Arc<Self>>,
    pub(crate) space: Option<Arc<TransformSpace>>,
}

impl ClipChain {
    pub(crate) fn contains(&self, window: Point) -> bool {
        let inverse = match &self.space {
            Some(space) => space.inverse,
            None => Some(Affine::IDENTITY),
        };
        inverse.is_some_and(|inverse| self.shape.contains(inverse.map_point(window)))
            && self
                .parent
                .as_deref()
                .is_none_or(|parent| parent.contains(window))
    }

    pub(crate) fn clip_rect(&self, rect: Rect, world: Affine, inverse: Affine) -> Rect {
        let rect = match &self.space {
            None if world == Affine::IDENTITY => self.shape.clip_rect(rect),
            Some(space) if space.world == world => self.shape.clip_rect(rect),
            space => {
                let (clip_world, clip_inverse) = space
                    .as_ref()
                    .map_or((Affine::IDENTITY, Some(Affine::IDENTITY)), |space| {
                        (space.world, space.inverse)
                    });
                let Some(clip_inverse) = clip_inverse else {
                    return Rect::default();
                };
                let inside = self
                    .shape
                    .clip_rect(world.then(clip_inverse).map_rect(rect));
                rect.intersect(clip_world.then(inverse).map_rect(inside))
            }
        };
        self.parent
            .as_deref()
            .map_or(rect, |parent| parent.clip_rect(rect, world, inverse))
    }
}

pub fn corner_radii(node: &Node, bounds: Rect) -> [f32; 4] {
    let radius = (node.kind == "RoundButton")
        .then(|| node.length("radius"))
        .flatten()
        .or_else(|| node.length("border-radius"))
        .unwrap_or(if node.kind == "Dialog" {
            12.0
        } else if is_pane(node.kind) {
            8.0
        } else if node.kind == "RoundButton" {
            bounds.width.min(bounds.height) * 0.5
        } else if is_button(node.kind) || is_text_input(node.kind) {
            7.0
        } else {
            0.0
        });
    let radii = [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
    ]
    .map(|name| node.length(name).unwrap_or(radius).max(0.0));
    normalize_radii(bounds, radii)
}

pub fn normalize_radii(bounds: Rect, radii: [f32; 4]) -> [f32; 4] {
    let [tl, tr, br, bl] = radii;
    let scale = [
        (bounds.width, tl + tr),
        (bounds.width, bl + br),
        (bounds.height, tl + bl),
        (bounds.height, tr + br),
    ]
    .into_iter()
    .filter(|(_, sum)| *sum > 0.0)
    .fold(1.0_f32, |scale, (side, sum)| scale.min(side / sum));
    radii.map(|radius| radius * scale)
}
