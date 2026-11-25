//! Unit quaternions for authored 3D rotations and interpolation.

use nalgebra::{Matrix3, Quaternion, Rotation3, Unit, UnitQuaternion, Vector3};
use std::sync::Arc;

use crate::{Point3D, VecModel, Vector3D};

/// A snapshot of strictly increasing rotation keys in seconds.
#[derive(Clone, Debug, Default)]
pub struct RotationTrack3D(Arc<[(f64, Quaternion3D)]>);

impl PartialEq for RotationTrack3D {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.as_ref() == other.0.as_ref()
    }
}

impl RotationTrack3D {
    pub fn new(keys: Vec<(f64, Quaternion3D)>) -> Self {
        if keys
            .iter()
            .any(|(time, _)| !time.is_finite() || *time < 0.0)
            || keys.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Self::default();
        }
        Self(keys.into())
    }

    pub(crate) fn from_model<T>(
        model: &VecModel<T>,
        mut project: impl FnMut(&T) -> Option<(f64, Quaternion3D)>,
    ) -> Self {
        let count = model.len();
        let mut keys = Vec::with_capacity(count);
        model.visit_row_data(count, |index, _, row| {
            if keys.len() == index {
                keys.extend(project(row));
            }
        });
        if keys.len() == count {
            Self::new(keys)
        } else {
            Self::default()
        }
    }

    pub fn valid(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn count(&self) -> i64 {
        self.0.len() as i64
    }

    pub fn duration(&self) -> f64 {
        self.0.last().map_or(0.0, |key| key.0)
    }

    /// Clamp to the end keys and interpolate inside a segment on the shortest arc.
    pub fn sample(&self, time: f64) -> Quaternion3D {
        let Some(&(start, first)) = self.0.first() else {
            return Quaternion3D::IDENTITY;
        };
        if time.is_nan() {
            return Quaternion3D::IDENTITY;
        }
        if time <= start {
            return first;
        }
        let right = self.0.partition_point(|(key_time, _)| *key_time < time);
        let Some(&(end, next)) = self.0.get(right) else {
            return self.0[right - 1].1;
        };
        let (begin, previous) = self.0[right - 1];
        previous.slerp(next, (time - begin) / (end - begin))
    }
}

/// A unit rotation in 3D. Components use the (x, y, z, w) convention.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion3D([f64; 4]);

impl Default for Quaternion3D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quaternion3D {
    pub const IDENTITY: Self = Self([0.0, 0.0, 0.0, 1.0]);

    /// Normalize the supplied components. Invalid and zero-length values yield identity.
    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        let values = [x, y, z, w];
        let Some(scale) = normalization_scale(values) else {
            return Self::IDENTITY;
        };
        let [x, y, z, w] = values.map(|value| value / scale);
        Self::from_unit(UnitQuaternion::new_normalize(Quaternion::new(w, x, y, z)))
    }

    /// Construct a rotation from a nonzero axis and an angle in degrees.
    pub fn from_axis_angle(axis: Vector3D, degrees: f64) -> Self {
        let values = [axis.x, axis.y, axis.z];
        let Some(scale) = normalization_scale(values) else {
            return Self::IDENTITY;
        };
        if !degrees.is_finite() {
            return Self::IDENTITY;
        }
        let axis = Unit::new_normalize(Vector3::from(values.map(|value| value / scale)));
        Self::from_unit(UnitQuaternion::from_axis_angle(
            &axis,
            degrees.rem_euclid(360.0).to_radians(),
        ))
    }

    /// Use the scene's X, Y, Z Euler convention, in degrees.
    pub fn from_euler(angles: Vector3D) -> Self {
        let values = [angles.x, angles.y, angles.z];
        if !values.into_iter().all(f64::is_finite) {
            return Self::IDENTITY;
        }
        let [x, y, z] = values.map(|value| value.rem_euclid(360.0).to_radians());
        Self::from_unit(UnitQuaternion::from_euler_angles(x, y, z))
    }

    /// Point local -Z along `forward` and local +Y toward the projected up direction.
    pub fn look_rotation(forward: Vector3D, up: Vector3D) -> Self {
        look_basis(Vector3::new(forward.x, forward.y, forward.z), up, None)
            .map_or(Self::IDENTITY, Self::from_rotation)
    }

    /// Aim local -Z from one point toward another.
    pub fn look_at(from: Point3D, target: Point3D, up: Vector3D) -> Self {
        look_at_basis(from, target, up, None).map_or(Self::IDENTITY, Self::from_rotation)
    }

    fn from_rotation(rotation: Rotation3<f64>) -> Self {
        Self::from_unit(UnitQuaternion::from_rotation_matrix(&rotation))
    }

    /// Apply this rotation, then the next one.
    pub fn then(self, next: Self) -> Self {
        if self == Self::IDENTITY {
            return next;
        }
        if next == Self::IDENTITY {
            return self;
        }
        Self::from_unit(next.unit() * self.unit())
    }

    pub fn inverse(self) -> Self {
        Self::from_unit(self.unit().inverse())
    }

    /// Interpolate along the shortest arc, clamped to the endpoints.
    pub fn slerp(self, next: Self, progress: f64) -> Self {
        if progress <= 0.0 || progress.is_nan() || self == next {
            return self;
        }
        if progress >= 1.0 {
            return next;
        }
        let first = self.unit();
        let mut second = next.unit();
        if first.coords.dot(&second.coords) < 0.0 {
            second = UnitQuaternion::new_unchecked(-second.into_inner());
        }
        Self::from_unit(
            first
                .try_slerp(&second, progress, 1.0e-12)
                .unwrap_or_else(|| first.nlerp(&second, progress)),
        )
    }

    pub fn rotate(self, vector: Vector3D) -> Vector3D {
        let result = self.unit() * Vector3::new(vector.x, vector.y, vector.z);
        Vector3D::new(result.x, result.y, result.z)
    }

    pub const fn components(self) -> [f64; 4] {
        self.0
    }

    pub const fn x(self) -> f64 {
        self.0[0]
    }

    pub const fn y(self) -> f64 {
        self.0[1]
    }

    pub const fn z(self) -> f64 {
        self.0[2]
    }

    pub const fn w(self) -> f64 {
        self.0[3]
    }

    pub(crate) fn rotation_f32(self) -> Rotation3<f32> {
        self.unit().to_rotation_matrix().cast()
    }

    fn unit(self) -> UnitQuaternion<f64> {
        let [x, y, z, w] = self.0;
        UnitQuaternion::new_unchecked(Quaternion::new(w, x, y, z))
    }

    fn from_unit(unit: UnitQuaternion<f64>) -> Self {
        let [x, y, z, w]: [f64; 4] = unit.into_inner().coords.into();
        // Antipodal quaternions describe the same rotation. Store one sign.
        let negative = [w, x, y, z]
            .into_iter()
            .find(|value| *value != 0.0)
            .is_some_and(|value| value < 0.0);
        Self([x, y, z, w].map(|value| {
            if value == 0.0 {
                0.0
            } else if negative {
                -value
            } else {
                value
            }
        }))
    }
}

pub(crate) fn look_at_basis(
    from: Point3D,
    target: Point3D,
    up: Vector3D,
    right_hint: Option<Vector3<f64>>,
) -> Option<Rotation3<f64>> {
    let from = Vector3::new(from.x, from.y, from.z);
    let target = Vector3::new(target.x, target.y, target.z);
    let mut direction = target - from;
    if direction.iter().any(|value| !value.is_finite()) {
        let scale = normalization_scale([from.x, from.y, from.z, target.x, target.y, target.z])?;
        direction = target / scale - from / scale;
    }
    look_basis(direction, up, right_hint)
}

fn normalized(vector: Vector3<f64>) -> Option<Vector3<f64>> {
    let scale = normalization_scale([vector.x, vector.y, vector.z])?;
    Some((vector / scale).normalize())
}

fn look_basis(
    direction: Vector3<f64>,
    up: Vector3D,
    right_hint: Option<Vector3<f64>>,
) -> Option<Rotation3<f64>> {
    const MIN_TANGENT: f64 = 1e-7;
    let forward = normalized(direction)?;
    let up = normalized(Vector3::new(up.x, up.y, up.z)).unwrap_or_else(Vector3::y);
    let right = forward
        .cross(&up)
        .try_normalize(MIN_TANGENT)
        .or_else(|| {
            let hint = right_hint?;
            (hint - forward * forward.dot(&hint)).try_normalize(MIN_TANGENT)
        })
        .unwrap_or_else(|| {
            let axis = forward.abs();
            let tangent = if axis.x <= axis.y && axis.x <= axis.z {
                Vector3::x()
            } else if axis.y <= axis.z {
                Vector3::y()
            } else {
                Vector3::z()
            };
            // The least aligned axis has a nonzero cross product.
            forward.cross(&tangent).normalize()
        });
    let up = right.cross(&forward);
    Some(Rotation3::from_matrix_unchecked(Matrix3::from_columns(&[
        right, up, -forward,
    ])))
}

fn normalization_scale<const N: usize>(values: [f64; N]) -> Option<f64> {
    if !values.into_iter().all(f64::is_finite) {
        return None;
    }
    let scale = values.into_iter().map(f64::abs).fold(0.0, f64::max);
    (scale > 0.0).then_some(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(value: Vector3D, [x, y, z]: [f64; 3]) {
        assert!((value.x - x).abs() < 1.0e-10, "{value:?}");
        assert!((value.y - y).abs() < 1.0e-10, "{value:?}");
        assert!((value.z - z).abs() < 1.0e-10, "{value:?}");
    }

    #[test]
    fn normalizes_extreme_components_and_antipodes() {
        let unit = Quaternion3D::new(1.0, 0.0, 0.0, 1.0);
        assert_eq!(Quaternion3D::new(1.0e300, 0.0, 0.0, 1.0e300), unit);
        assert_eq!(Quaternion3D::new(-1.0e-300, 0.0, 0.0, -1.0e-300), unit);
        assert_eq!(
            Quaternion3D::new(0.0, 0.0, 0.0, 0.0),
            Quaternion3D::IDENTITY
        );
        assert_eq!(
            Quaternion3D::new(f64::NAN, 0.0, 0.0, 1.0),
            Quaternion3D::IDENTITY
        );
    }

    #[test]
    fn composes_in_scene_order_and_interpolates_the_shortest_arc() {
        let x = Quaternion3D::from_axis_angle(Vector3D::new(1.0, 0.0, 0.0), 90.0);
        let y = Quaternion3D::from_axis_angle(Vector3D::new(0.0, 1.0, 0.0), 90.0);
        near(
            x.then(y).rotate(Vector3D::new(0.0, 1.0, 0.0)),
            [1.0, 0.0, 0.0],
        );
        near(
            x.then(y).inverse().rotate(Vector3D::new(1.0, 0.0, 0.0)),
            [0.0, 1.0, 0.0],
        );
        let euler = Quaternion3D::from_euler(Vector3D::new(90.0, 90.0, 0.0));
        near(euler.rotate(Vector3D::new(0.0, 1.0, 0.0)), [1.0, 0.0, 0.0]);

        let axis = Vector3D::new(0.0, 0.0, 1.0);
        let first = Quaternion3D::from_axis_angle(axis, 170.0);
        let last = Quaternion3D::from_axis_angle(axis, -170.0);
        near(
            first.slerp(last, 0.5).rotate(Vector3D::new(1.0, 0.0, 0.0)),
            [-1.0, 0.0, 0.0],
        );
        assert_eq!(first.slerp(last, -1.0), first);
        assert_eq!(first.slerp(last, 2.0), last);
    }

    #[test]
    fn aiming_keeps_forward_and_projected_up_orthogonal_at_the_poles() {
        let eye = Point3D::new(0.0, 8.0, 0.0);
        let north = Vector3D::new(0.0, 0.0, -1.0);
        let up = Vector3D::new(0.0, 1.0, 0.0);
        let forward = Vector3D::new(0.0, 0.0, -1.0);
        let target = Point3D::default();
        let aimed = Quaternion3D::look_at(eye, target, north);
        near(aimed.rotate(forward), [0.0, -1.0, 0.0]);
        near(aimed.rotate(up), [0.0, 0.0, -1.0]);

        let pole = Quaternion3D::look_at(eye, target, up);
        near(pole.rotate(forward), [0.0, -1.0, 0.0]);
        near(pole.rotate(up), [1.0, 0.0, 0.0]);
        let rolled = Quaternion3D::look_rotation(forward, Vector3D::new(1.0, 0.0, 0.0));
        near(rolled.rotate(forward), [0.0, 0.0, -1.0]);
        near(rolled.rotate(up), [1.0, 0.0, 0.0]);
        assert_eq!(
            Quaternion3D::look_rotation(forward, Vector3D::new(0.0, f64::NAN, 0.0)),
            Quaternion3D::IDENTITY
        );
    }

    #[test]
    fn aiming_handles_overflowing_differences_and_degenerate_directions() {
        let up = Vector3D::new(0.0, 1.0, 0.0);
        let maximum = f64::MAX;
        let from = Point3D::new(maximum, 0.0, 0.0);
        assert_eq!(
            Quaternion3D::look_at(from, from, up),
            Quaternion3D::IDENTITY
        );
        assert_eq!(
            Quaternion3D::look_rotation(Vector3D::default(), up),
            Quaternion3D::IDENTITY
        );
        let aimed = Quaternion3D::look_at(from, Point3D::new(-maximum, 0.0, 0.0), up);
        near(
            aimed.rotate(Vector3D::new(0.0, 0.0, -1.0)),
            [-1.0, 0.0, 0.0],
        );
        near(aimed.rotate(up), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn track_clamps_and_uses_the_interval_between_nonuniform_keys() {
        let z = Vector3D::new(0.0, 0.0, 1.0);
        let first = Quaternion3D::from_axis_angle(z, 170.0);
        let middle = Quaternion3D::from_axis_angle(z, -170.0);
        let last = Quaternion3D::from_axis_angle(z, -80.0);
        let track = RotationTrack3D::new(vec![(0.5, first), (1.5, middle), (4.5, last)]);
        assert!(track.valid());
        assert_eq!(track.count(), 3);
        assert_eq!(track.duration(), 4.5);
        assert_eq!(track.sample(f64::NEG_INFINITY), first);
        assert_eq!(track.sample(f64::INFINITY), last);
        assert_eq!(track.sample(f64::NAN), Quaternion3D::IDENTITY);
        let x = Vector3D::new(1.0, 0.0, 0.0);
        near(track.sample(1.0).rotate(x), [-1.0, 0.0, 0.0]);
        assert_eq!(track.sample(1.5), middle);
        assert_eq!(track.sample(3.0), middle.slerp(last, 0.5));
        assert_eq!(RotationTrack3D::new(vec![(2.0, last)]).sample(0.0), last);
    }

    #[test]
    fn malformed_keys_never_leave_a_partially_valid_track() {
        let q = Quaternion3D::IDENTITY;
        for keys in [
            vec![],
            vec![(-0.1, q)],
            vec![(f64::NAN, q)],
            vec![(f64::INFINITY, q)],
            vec![(0.0, q), (0.0, q)],
            vec![(1.0, q), (0.5, q)],
        ] {
            let track = RotationTrack3D::new(keys);
            assert!(!track.valid());
            assert_eq!(track.count(), 0);
            assert_eq!(track.duration(), 0.0);
            assert_eq!(track.sample(1.0), q);
        }
    }

    #[test]
    fn rotation_projection_reuses_each_model_revision_and_preserves_snapshots() {
        use crate::scene_projection::SceneProjectionCache;

        let model = VecModel::from(vec![0.0, 1.0, 2.0]);
        let shared = model.clone();
        let cache = SceneProjectionCache::new();
        let mut projected = 0;
        let mut key = |time: &f64| {
            projected += 1;
            Some((*time, Quaternion3D::IDENTITY))
        };
        let first = cache.rotation_track(&model, &mut key);
        let same = cache.rotation_track(&shared, &mut key);
        assert_eq!(first, same);
        assert!(model.set_row(2, 1.0));
        let repeated = cache.rotation_track(&shared, &mut key);
        assert!(!repeated.valid());
        assert!(model.set_row(2, 3.0));
        let after = cache.rotation_track(&shared, &mut key);
        assert_eq!(projected, 9);
        assert_eq!(first.duration(), 2.0);
        assert_eq!(after.duration(), 3.0);
    }
}
