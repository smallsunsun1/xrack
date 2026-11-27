//! Precomputed geometry and direction parameters for deterministic particle spawning.
use nalgebra::Vector3;

use crate::{scene3d::hierarchy::euler_rotation, scene3d::particles::ParticleEmitterDefinition};

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
enum ShapeKind {
    Point = 0,
    Box = 1,
    Sphere = 2,
    Cylinder = 3,
    Mesh = 4,
    Points = 5,
}

/// Shape axes, area weights and sampling mode shared by emitters and attractors.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParticleShape {
    pub axes: [[f32; 4]; 3],
    pub weights: [f32; 4],
    pub kind: u32,
    pub surface: bool,
}

impl ParticleShape {
    pub(crate) fn new(
        name: &str,
        extents: [f64; 3],
        angles: [f64; 3],
        surface: bool,
        needs_normal: bool,
    ) -> Self {
        let mut extents = extents.map(|value| f64::from(argument(value, 0.0, 1.0e8) as f32));
        let mut kind = match name {
            "mesh" | "scene" => {
                extents = [1.0; 3];
                ShapeKind::Mesh
            }
            "points" => {
                extents = [1.0; 3];
                ShapeKind::Points
            }
            "box" if extents != [0.0; 3] => ShapeKind::Box,
            "sphere" if extents != [0.0; 3] => ShapeKind::Sphere,
            "cylinder" if extents != [0.0; 3] => ShapeKind::Cylinder,
            _ => ShapeKind::Point,
        };
        if kind == ShapeKind::Point && !needs_normal {
            return Self::default();
        }
        let (surface, limits, weights) = surface_parameters(kind, extents, surface || needs_normal);
        if needs_normal && !surface && !matches!(kind, ShapeKind::Mesh | ShapeKind::Points) {
            // A shape with no surface uses the emitter's forward axis.
            kind = ShapeKind::Point;
            extents = [1.0; 3];
        }
        let angles = angles.map(|degrees| finite(degrees).rem_euclid(360.0) as f32);
        let rotation = euler_rotation(angles);
        Self {
            axes: std::array::from_fn(|column| {
                let basis = rotation.matrix().column(column) * extents[column] as f32;
                pack(basis, limits[column])
            }),
            weights,
            kind: kind as u32,
            surface,
        }
    }

    pub(crate) fn extent(&self, mesh_radius: Option<f64>) -> [f64; 3] {
        if let Some(radius) = mesh_radius {
            return [radius; 3];
        }
        if self.kind == ShapeKind::Point as u32 {
            return [0.0; 3];
        }
        std::array::from_fn(|axis| {
            self.axes
                .iter()
                .map(|column| f64::from(column[axis].abs()))
                .sum()
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
enum VelocityMode {
    Vector = 0,
    Cone = 1,
    Target = 2,
    NormalizedVector = 3,
    SurfaceNormal = 4,
    SurfaceReflected = 5,
}

/// Emitter geometry and direction uniform data. All columns are vec4-aligned.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleMotion {
    pub shape: ParticleShape,
    pub axis_speed: [f32; 4],
    pub right_cosine: [f32; 4],
    pub up_variation: [f32; 4],
    pub target: [f32; 4],
    pub target_spread: [f32; 4],
    pub modes: [u32; 4],
    pub placement: Option<[[f32; 4]; 3]>,
}

impl ParticleMotion {
    pub(crate) fn spawn_extent(&self, mesh_radius: Option<f64>) -> [f64; 3] {
        self.shape.extent(mesh_radius)
    }

    pub(crate) fn uses_surface_velocity(&self) -> bool {
        self.modes[2] == VelocityMode::SurfaceNormal as u32
            || self.modes[2] == VelocityMode::SurfaceReflected as u32
    }

    pub(super) fn uses_target_velocity(&self) -> bool {
        self.modes[2] == VelocityMode::Target as u32
    }

    pub(super) fn target_travel(&self, birth_bounds: &[[f64; 3]; 2], age: f64) -> [f64; 3] {
        let magnitude = f64::from(self.axis_speed[3].abs()) + f64::from(self.up_variation[3]);
        if self.target[3] != 0.0 {
            return [age * magnitude; 3];
        }
        std::array::from_fn(|axis| {
            let destination = f64::from(self.target[axis]);
            let spread = f64::from(self.target_spread[axis]);
            let distance = (destination - birth_bounds[0][axis])
                .abs()
                .max((destination - birth_bounds[1][axis]).abs())
                + spread;
            age * magnitude * distance
        })
    }

    pub(crate) fn max_directed_speed(&self, velocity: [f32; 3], spread: [f32; 3]) -> Option<f64> {
        if self.uses_target_velocity() {
            return Some(0.0); // World-space travel is accounted for after placement.
        }
        if self.modes[2] == VelocityMode::Vector as u32 {
            return None;
        }
        let nominal = if self.modes[2] == VelocityMode::SurfaceReflected as u32 {
            velocity
                .into_iter()
                .zip(spread)
                .map(|(axis, variation)| (f64::from(axis.abs()) + f64::from(variation)).powi(2))
                .sum::<f64>()
                .sqrt()
        } else {
            f64::from(self.axis_speed[3])
        };
        Some(nominal + f64::from(self.up_variation[3]))
    }
}

impl ParticleEmitterDefinition {
    pub(crate) fn prepare_motion(&self, velocity: [f64; 3]) -> Option<ParticleMotion> {
        let motion = match self.velocity_mode.as_str() {
            "cone" => VelocityMode::Cone,
            "target" => VelocityMode::Target,
            "normalized-vector" => VelocityMode::NormalizedVector,
            "surface-normal" => VelocityMode::SurfaceNormal,
            "surface-reflected" => VelocityMode::SurfaceReflected,
            _ => VelocityMode::Vector,
        };
        let shape = ParticleShape::new(
            &self.emitter_shape,
            [
                self.emitter_extents.x,
                self.emitter_extents.y,
                self.emitter_extents.z,
            ],
            [self.emitter_yaw, self.emitter_pitch, self.emitter_roll],
            !self.emitter_fill,
            matches!(
                motion,
                VelocityMode::SurfaceNormal | VelocityMode::SurfaceReflected
            ),
        );
        if shape.kind == ShapeKind::Point as u32 && motion == VelocityMode::Vector {
            return None;
        }
        let velocity = Vector3::from(velocity);
        let speed = velocity.norm();
        let axis: Vector3<f32> = if speed > 0.0 {
            (velocity / speed).cast()
        } else {
            Vector3::y()
        };
        let reference = if axis.y.abs() < 0.99 {
            Vector3::y()
        } else {
            Vector3::x()
        };
        let right = reference.cross(&axis).normalize();
        let up = axis.cross(&right);
        let cone_cosine = argument(self.velocity_cone_angle, 0.0, 180.0)
            .to_radians()
            .cos() as f32;
        let target = [
            self.velocity_target.x,
            self.velocity_target.y,
            self.velocity_target.z,
        ]
        .map(|value| argument(value, -1.0e8, 1.0e8) as f32);
        let spread = [
            self.velocity_target_spread.x,
            self.velocity_target_spread.y,
            self.velocity_target_spread.z,
        ]
        .map(|value| argument(value.abs(), 0.0, 1.0e8) as f32);
        let variation = argument(self.velocity_speed_variation, 0.0, 1.0e8) as f32;
        let magnitude = if motion == VelocityMode::Target {
            self.velocity_target_magnitude
                .map_or(speed, |value| argument(value, -1.0e8, 1.0e8))
        } else {
            speed
        };
        Some(ParticleMotion {
            shape,
            axis_speed: pack(axis, magnitude as f32),
            right_cosine: pack(right, cone_cosine),
            up_variation: pack(up, variation),
            target: pack(
                Vector3::from(target),
                u32::from(self.velocity_target_normalized) as f32,
            ),
            target_spread: pack(Vector3::from(spread), 0.0),
            modes: [shape.kind, u32::from(shape.surface), motion as u32, 0],
            placement: None,
        })
    }
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn argument(value: f64, min: f64, max: f64) -> f64 {
    finite(value).clamp(min, max)
}

fn pack(vector: Vector3<f32>, w: f32) -> [f32; 4] {
    [vector.x, vector.y, vector.z, w]
}

fn surface_parameters(
    shape: ShapeKind,
    [x, y, z]: [f64; 3],
    surface: bool,
) -> (bool, [f32; 3], [f32; 4]) {
    let defaults = (false, [0.0; 3], [0.0; 4]);
    if !surface {
        return defaults;
    }
    let [yz, xz, xy] = [y * z, x * z, x * y];
    match shape {
        ShapeKind::Box if yz + xz + xy > 0.0 => {
            let area = yz + xz + xy;
            (
                true,
                [(yz / area) as f32, ((yz + xz) / area) as f32, 0.0],
                [0.0; 4],
            )
        }
        ShapeKind::Sphere if yz.max(xz).max(xy) > 0.0 => {
            let area = yz.max(xz).max(xy);
            // The surface-area Jacobian of the sphere-to-ellipsoid transform.
            (
                true,
                [0.0; 3],
                [
                    (yz / area) as f32,
                    (xz / area) as f32,
                    (xy / area) as f32,
                    0.0,
                ],
            )
        }
        ShapeKind::Cylinder if x + z > 0.0 && (y > 0.0 || xz > 0.0) => {
            let radius = x.max(z);
            let pi = std::f64::consts::PI;
            let eccentricity = ((x - z) / (x + z)).powi(2);
            // Ramanujan's perimeter approximation. Circumference samples
            // are subsequently weighted by the exact local arc speed.
            let perimeter = pi
                * (x + z)
                * (1.0 + 3.0 * eccentricity / (10.0 + (4.0 - 3.0 * eccentricity).sqrt()));
            let caps = pi * xz;
            let cap_probability = caps / (caps + y * perimeter);
            (
                true,
                [0.0, 0.0, cap_probability as f32],
                [(x / radius) as f32, (z / radius) as f32, 0.0, 0.0],
            )
        }
        _ => defaults,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vector3D;

    #[test]
    fn target_travel_accounts_for_world_birth_bounds_and_signed_variation() {
        let mut definition = ParticleEmitterDefinition {
            velocity_mode: "target".into(),
            velocity_target: crate::Point3D::new(5.0, 0.0, 0.0),
            velocity_target_spread: Vector3D::new(0.0, 1.0, 0.0),
            velocity_target_magnitude: Some(-2.0),
            velocity_speed_variation: 0.5,
            ..Default::default()
        };
        let world_births = [[-2.0, -1.0, 0.0], [1.0, 2.0, 0.0]];
        let motion = definition.prepare_motion([0.0; 3]).unwrap();
        assert_eq!(motion.target_travel(&world_births, 0.5), [1.25; 3]);
        definition.velocity_target_normalized = false;
        let motion = definition.prepare_motion([0.0; 3]).unwrap();
        assert_eq!(motion.target_travel(&world_births, 0.5), [8.75, 3.75, 0.0]);
        definition.velocity_target_magnitude = None;
        let motion = definition.prepare_motion([3.0, 4.0, 0.0]).unwrap();
        assert_eq!(motion.axis_speed[3], 5.0);
    }

    #[test]
    fn a_shape_smaller_than_the_gpu_can_represent_becomes_a_point() {
        let definition = ParticleEmitterDefinition {
            emitter_shape: "cylinder".into(),
            emitter_extents: Vector3D::new(1e-230, 1e-200, 0.0),
            emitter_fill: false,
            ..Default::default()
        };
        assert!(definition.prepare_motion([0.0; 3]).is_none());
    }
}
