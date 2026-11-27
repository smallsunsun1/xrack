use crate::{Point, Rect};
use nalgebra::{Matrix2, Vector2};
use std::marker::PhantomData;
use xrack_runtime::{Length, Node, Point2D, Value, is_window_overlay};

/// The logical coordinates in which a node's layout geometry was calculated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSpace;

/// Window-relative logical pixels, after applying the visual transforms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSpace;

/// A compact affine map between coordinate spaces. Points are column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2D<From, To> {
    linear: Matrix2<f32>,
    translation: Vector2<f32>,
    spaces: PhantomData<fn(From) -> To>,
}

impl<From, To> Transform2D<From, To> {
    /// A mapping between spaces whose coordinates coincide.
    pub const IDENTITY: Self = Self::from_coefficients([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// Columns of the linear part, followed by the translation vector.
    pub const fn from_coefficients([a, b, c, d, x, y]: [f32; 6]) -> Self {
        Self {
            linear: Matrix2::new(a, c, b, d),
            translation: Vector2::new(x, y),
            spaces: PhantomData,
        }
    }

    pub fn coefficients(&self) -> [f32; 6] {
        [
            self.linear[(0, 0)],
            self.linear[(1, 0)],
            self.linear[(0, 1)],
            self.linear[(1, 1)],
            self.translation.x,
            self.translation.y,
        ]
    }

    pub fn is_identity(&self) -> bool {
        self.same_coefficients(&Self::IDENTITY)
    }

    pub fn axis_scales(&self) -> Option<[f32; 2]> {
        (self.linear[(0, 1)] == 0.0 && self.linear[(1, 0)] == 0.0)
            .then_some([self.linear[(0, 0)], self.linear[(1, 1)]])
    }

    pub fn translation(&self) -> [f32; 2] {
        [self.translation.x, self.translation.y]
    }

    pub(crate) fn same_coefficients<Src, Dst>(&self, other: &Transform2D<Src, Dst>) -> bool {
        self.linear == other.linear && self.translation == other.translation
    }

    /// Apply this map first and `next` second.
    pub fn then<Next>(self, next: Transform2D<To, Next>) -> Transform2D<From, Next> {
        Transform2D {
            linear: next.linear * self.linear,
            translation: next.linear * self.translation + next.translation,
            spaces: PhantomData,
        }
    }

    pub fn inverse(&self) -> Option<Transform2D<To, From>> {
        let linear = self.linear.cast::<f64>();
        let det = linear.determinant();
        if !det.is_finite() || det.abs() < 1e-6 {
            return None;
        }
        let linear = linear.try_inverse()?;
        let translation = -(linear * self.translation.cast::<f64>());
        let inverse = Transform2D {
            linear: linear.cast(),
            translation: translation.cast(),
            spaces: PhantomData,
        };
        inverse
            .coefficients()
            .iter()
            .all(|value| value.is_finite())
            .then_some(inverse)
    }

    pub fn map_point(&self, point: Point) -> Point {
        let position = self.linear * Vector2::new(point.x, point.y) + self.translation;
        Point {
            x: position.x,
            y: position.y,
        }
    }

    pub fn map_vector(&self, delta: Point) -> Point {
        let delta = self.linear * Vector2::new(delta.x, delta.y);
        Point {
            x: delta.x,
            y: delta.y,
        }
    }

    /// Maximum stretch in any direction, including under shear.
    pub fn max_scale(&self) -> f32 {
        if let Some([x, y]) = self.axis_scales() {
            return x.abs().max(y.abs());
        }
        let linear = self.linear.cast::<f64>();
        let gram = linear.transpose() * linear;
        let (xx, xy, yy) = (gram[(0, 0)], gram[(0, 1)], gram[(1, 1)]);
        // Largest eigenvalue of the symmetric 2×2 Gram matrix.
        (0.5 * (xx + yy + (xx - yy).hypot(2.0 * xy))).sqrt() as f32
    }

    /// The transformed corners, clockwise from the original top left.
    pub fn map_corners(&self, rect: Rect) -> [Point; 4] {
        let (left, top) = (rect.x, rect.y);
        let (right, bottom) = (left + rect.width, top + rect.height);
        [
            Point { x: left, y: top },
            Point { x: right, y: top },
            Point {
                x: right,
                y: bottom,
            },
            Point { x: left, y: bottom },
        ]
        .map(|point| self.map_point(point))
    }

    /// The axis-aligned bounds of the transformed rectangle.
    pub fn map_rect(&self, rect: Rect) -> Rect {
        if self.is_identity() {
            return rect;
        }
        if rect.width <= 0.0 || rect.height <= 0.0 {
            let origin = self.map_point(Point {
                x: rect.x,
                y: rect.y,
            });
            return Rect::new(origin.x, origin.y, 0.0, 0.0);
        }
        if self.axis_scales().is_some() {
            let start = Point {
                x: rect.x,
                y: rect.y,
            };
            let end = Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            };
            if [start.x, start.y, end.x, end.y]
                .into_iter()
                .all(f32::is_finite)
            {
                // Each axis is independent, including with a negative scale.
                let start = self.map_point(start);
                let end = self.map_point(end);
                let left = start.x.min(end.x);
                let top = start.y.min(end.y);
                return Rect::new(
                    left,
                    top,
                    start.x.max(end.x) - left,
                    start.y.max(end.y) - top,
                );
            }
        }
        let [first, rest @ ..] = self.map_corners(rect);
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

impl Transform2D<LayoutSpace, LayoutSpace> {
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
        // One ordered lookup handles the common case without any transforms.
        if node
            .properties
            .range("transform"..)
            .next()
            .is_none_or(|(name, _)| !name.starts_with("transform"))
        {
            return Self::IDENTITY;
        }
        let explicit = match node.properties.get("transform") {
            Some(Value::Transform2D(value)) => {
                let top_left =
                    Point2D::new(Length(f64::from(bounds.x)), Length(f64::from(bounds.y)));
                Self::from_coefficients(value.around(top_left).coefficients().map(|x| x as f32))
            }
            _ => Self::IDENTITY,
        };
        let angle = node
            .number("transform-rotation")
            .unwrap_or_default()
            .rem_euclid(360.0);
        let uniform = node.number("transform-scale").unwrap_or(1.0);
        let scale = |axis| node.number(axis).unwrap_or(uniform).clamp(-1024.0, 1024.0) as f32;
        let sx = scale("transform-scale-x");
        let sy = scale("transform-scale-y");
        if angle == 0.0 && sx == 1.0 && sy == 1.0 {
            return explicit;
        }
        let (sin, cos) = angle.to_radians().sin_cos();
        let (sin, cos) = (snap_axis(sin), snap_axis(cos));
        let linear = Matrix2::new(cos * sx, -sin * sy, sin * sx, cos * sy);
        let origin = Vector2::new(
            bounds.x
                + node
                    .relative_length("transform-origin-x", bounds.width)
                    .unwrap_or(bounds.width * 0.5),
            bounds.y
                + node
                    .relative_length("transform-origin-y", bounds.height)
                    .unwrap_or(bounds.height * 0.5),
        );
        let legacy = Self {
            linear,
            translation: origin - linear * origin,
            spaces: PhantomData,
        };
        if explicit.is_identity() {
            legacy
        } else {
            legacy.then(explicit)
        }
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
    pub world: Transform2D<LayoutSpace, WindowSpace>,
    pub inverse: Option<Transform2D<WindowSpace, LayoutSpace>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_in_application_order_and_preserves_signed_scale() {
        let local = Transform2D::<LayoutSpace, LayoutSpace>::from_coefficients([
            2.0, 0.0, 0.0, -3.0, 0.0, 0.0,
        ]);
        let parent = Transform2D::<LayoutSpace, WindowSpace>::from_coefficients([
            1.0, 0.0, 0.0, 1.0, 10.0, 20.0,
        ]);
        let world = local.then(parent);
        let point = Point { x: 1.0, y: 2.0 };
        assert_eq!(world.coefficients(), [2.0, 0.0, 0.0, -3.0, 10.0, 20.0]);
        assert_eq!(world.map_point(point), Point { x: 12.0, y: 14.0 });
        assert_eq!(world.map_vector(point), Point { x: 2.0, y: -6.0 });
        let inverse: Transform2D<WindowSpace, LayoutSpace> = world.inverse().unwrap();
        let roundtrip = inverse.map_point(world.map_point(point));
        assert!((roundtrip.x - point.x).abs() < 1e-6);
        assert!((roundtrip.y - point.y).abs() < 1e-6);
        assert_eq!(
            world.map_rect(Rect::new(1.0, 2.0, 2.0, 3.0)),
            Rect::new(12.0, 5.0, 4.0, 9.0)
        );
    }

    #[test]
    fn inverse_rejects_degenerate_near_singular_and_nonfinite_maps() {
        for scale in [0.0, 0.0005, f32::NAN, f32::INFINITY] {
            let map = Transform2D::<LayoutSpace, WindowSpace>::from_coefficients([
                scale, 0.0, 0.0, scale, 0.0, 0.0,
            ]);
            assert!(map.inverse().is_none());
        }
        let invalid_translation = Transform2D::<LayoutSpace, WindowSpace>::from_coefficients([
            1.0,
            0.0,
            0.0,
            1.0,
            f32::INFINITY,
            0.0,
        ]);
        assert!(invalid_translation.inverse().is_none());
    }

    #[test]
    fn maximum_stretch_accounts_for_nonuniform_scale_after_rotation() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let rotation = Transform2D::<LayoutSpace, LayoutSpace>::from_coefficients([
            half, half, -half, half, 0.0, 0.0,
        ]);
        let scale = Transform2D::<LayoutSpace, WindowSpace>::from_coefficients([
            2.0, 0.0, 0.0, 1.0, 0.0, 0.0,
        ]);
        assert!((rotation.then(scale).max_scale() - 2.0).abs() < 1e-6);
    }
}
