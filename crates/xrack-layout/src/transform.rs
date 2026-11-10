use crate::{Point, Rect};
use xrack_runtime::{Node, is_window_overlay};

/// Maps pre-transform layout coordinates to another coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub(crate) fn for_node(node: &Node, bounds: Rect) -> Self {
        if is_window_overlay(node.kind)
            || matches!(
                node.kind,
                "Window"
                    | "Menu"
                    | "MenuItem"
                    | "MenuSeparator"
                    | "Tooltip"
                    | "Tab"
                    | "TabButton"
                    | "TableColumn"
            )
        {
            return Self::IDENTITY;
        }
        let angle = node
            .number("transform-rotation")
            .unwrap_or_default()
            .rem_euclid(360.0);
        let uniform = node.number("transform-scale").unwrap_or(1.0);
        let scale = |axis| node.number(axis).unwrap_or(uniform).clamp(-1024.0, 1024.0) as f32;
        let sx = scale("transform-scale-x");
        let sy = scale("transform-scale-y");
        if angle == 0.0 && sx == 1.0 && sy == 1.0 {
            return Self::IDENTITY;
        }
        let (sin, cos) = angle.to_radians().sin_cos();
        let (sin, cos) = (snap_axis(sin), snap_axis(cos));
        let a = cos * sx;
        let b = sin * sx;
        let c = -sin * sy;
        let d = cos * sy;
        let ox = bounds.x
            + node
                .relative_length("transform-origin-x", bounds.width)
                .unwrap_or(bounds.width * 0.5);
        let oy = bounds.y
            + node
                .relative_length("transform-origin-y", bounds.height)
                .unwrap_or(bounds.height * 0.5);
        Self([a, b, c, d, ox - a * ox - c * oy, oy - b * ox - d * oy])
    }

    /// Apply this matrix first and `next` second.
    pub fn then(self, next: Self) -> Self {
        let Self([a, b, c, d, x, y]) = self;
        let Self([e, f, g, h, u, v]) = next;
        Self([
            e * a + g * b,
            f * a + h * b,
            e * c + g * d,
            f * c + h * d,
            e * x + g * y + u,
            f * x + h * y + v,
        ])
    }

    pub fn inverse(self) -> Option<Self> {
        let [a, b, c, d, x, y] = self.0.map(f64::from);
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1e-6 {
            return None;
        }
        let inverse =
            [d, -b, -c, a, c * y - d * x, b * x - a * y].map(|value| (value / det) as f32);
        inverse
            .iter()
            .all(|value| value.is_finite())
            .then_some(Self(inverse))
    }

    pub fn map_point(self, point: Point) -> Point {
        let [a, b, c, d, x, y] = self.0;
        Point {
            x: a * point.x + c * point.y + x,
            y: b * point.x + d * point.y + y,
        }
    }

    pub fn map_vector(self, delta: Point) -> Point {
        let [a, b, c, d, _, _] = self.0;
        Point {
            x: a * delta.x + c * delta.y,
            y: b * delta.x + d * delta.y,
        }
    }

    pub fn max_scale(self) -> f32 {
        let [a, b, c, d, _, _] = self.0;
        a.hypot(b).max(c.hypot(d))
    }

    pub fn map_rect(self, rect: Rect) -> Rect {
        if self == Self::IDENTITY {
            return rect;
        }
        if rect.width <= 0.0 || rect.height <= 0.0 {
            let origin = self.map_point(Point {
                x: rect.x,
                y: rect.y,
            });
            return Rect::new(origin.x, origin.y, 0.0, 0.0);
        }
        let corners = [
            Point {
                x: rect.x,
                y: rect.y,
            },
            Point {
                x: rect.x + rect.width,
                y: rect.y,
            },
            Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            },
            Point {
                x: rect.x,
                y: rect.y + rect.height,
            },
        ]
        .map(|point| self.map_point(point));
        let [first, rest @ ..] = corners;
        let (mut left, mut top, mut right, mut bottom) = (first.x, first.y, first.x, first.y);
        for point in rest {
            left = left.min(point.x);
            top = top.min(point.y);
            right = right.max(point.x);
            bottom = bottom.max(point.y);
        }
        Rect::new(left, top, right - left, bottom - top)
    }
}

fn snap_axis(value: f64) -> f32 {
    if value.abs() < 1e-12 {
        0.0
    } else {
        value as f32
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct TransformSpace {
    pub world: Affine,
    pub inverse: Option<Affine>,
}
