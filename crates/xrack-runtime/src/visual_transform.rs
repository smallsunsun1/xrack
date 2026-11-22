use crate::Length;
use nalgebra::{Matrix2, Vector2};

/// A position in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2D {
    pub x: Length,
    pub y: Length,
}

impl Point2D {
    pub const fn new(x: Length, y: Length) -> Self {
        Self { x, y }
    }
}

/// A displacement in logical pixels. Translation does not affect it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector2D {
    pub x: Length,
    pub y: Length,
}

impl Vector2D {
    pub const fn new(x: Length, y: Length) -> Self {
        Self { x, y }
    }
}

/// A logical-pixel affine map. Layout resolves it into its typed spaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform2D {
    linear: Matrix2<f64>,
    offset: Vector2<f64>,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    pub const IDENTITY: Self = Self::from_coefficients([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// Columns of the linear map, followed by translation in logical pixels.
    pub const fn from_coefficients([a, b, c, d, x, y]: [f64; 6]) -> Self {
        Self {
            linear: Matrix2::new(a, c, b, d),
            offset: Vector2::new(x, y),
        }
    }

    pub fn coefficients(self) -> [f64; 6] {
        [
            self.linear[(0, 0)],
            self.linear[(1, 0)],
            self.linear[(0, 1)],
            self.linear[(1, 1)],
            self.offset.x,
            self.offset.y,
        ]
    }

    pub const fn translation(x: Length, y: Length) -> Self {
        Self::from_coefficients([1.0, 0.0, 0.0, 1.0, x.0, y.0])
    }

    pub const fn scale(x: f64, y: f64) -> Self {
        Self::from_coefficients([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    pub fn rotation(degrees: f64) -> Self {
        let (sin, cos) = degrees.rem_euclid(360.0).to_radians().sin_cos();
        let snap = |axis: f64| if axis.abs() < 1e-12 { 0.0 } else { axis };
        let (sin, cos) = (snap(sin), snap(cos));
        Self::from_coefficients([cos, sin, -sin, cos, 0.0, 0.0])
    }

    /// Apply this transform, then apply `next`.
    pub fn then(self, next: Self) -> Self {
        if self == Self::IDENTITY {
            return next;
        }
        if next == Self::IDENTITY {
            return self;
        }
        Self {
            linear: next.linear * self.linear,
            offset: next.linear * self.offset + next.offset,
        }
    }

    pub fn around(self, point: Point2D) -> Self {
        if self == Self::IDENTITY {
            return self;
        }
        Self::translation(Length(-point.x.0), Length(-point.y.0))
            .then(self)
            .then(Self::translation(point.x, point.y))
    }

    pub fn map_point(self, point: Point2D) -> Point2D {
        if self == Self::IDENTITY {
            return point;
        }
        let mapped = self.linear * Vector2::new(point.x.0, point.y.0) + self.offset;
        Point2D::new(Length(mapped.x), Length(mapped.y))
    }

    pub fn map_vector(self, vector: Vector2D) -> Vector2D {
        if self.linear == Self::IDENTITY.linear {
            return vector;
        }
        let mapped = self.linear * Vector2::new(vector.x.0, vector.y.0);
        Vector2D::new(Length(mapped.x), Length(mapped.y))
    }
}
