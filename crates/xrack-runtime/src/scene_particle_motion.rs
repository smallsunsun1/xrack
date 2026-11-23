//! Precomputed geometry and direction parameters for deterministic particle spawning.
use nalgebra::Vector3;

use crate::{scene_hierarchy::euler_rotation, scene_particles::ParticleEmitterDefinition};

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
enum EmitterShape {
    Point = 0,
    Box = 1,
    Sphere = 2,
    Cylinder = 3,
    Mesh = 4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
enum VelocityMode {
    Vector = 0,
    Cone = 1,
    Target = 2,
    NormalizedVector = 3,
    SurfaceNormal = 4,
}

/// Emitter geometry and direction uniform data. All columns are vec4-aligned.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleMotion {
    pub shape: [[f32; 4]; 3],
    pub surface_weights: [f32; 4],
    pub axis_speed: [f32; 4],
    pub right_cosine: [f32; 4],
    pub up_variation: [f32; 4],
    pub target: [f32; 4],
    pub target_spread: [f32; 4],
    pub modes: [u32; 4],
}

impl ParticleEmitterDefinition {
    pub(crate) fn prepare_motion(&self, velocity: [f64; 3]) -> Option<ParticleMotion> {
        let mut extents = [
            self.emitter_extents.x,
            self.emitter_extents.y,
            self.emitter_extents.z,
        ]
        .map(|value| f64::from(argument(value, 0.0, 1.0e8) as f32));
        let shape = if self.emitter_shape == "mesh" {
            // Scale is already included in the triangle positions and areas.
            extents = [1.0; 3];
            EmitterShape::Mesh
        } else if extents == [0.0; 3] {
            EmitterShape::Point
        } else {
            match self.emitter_shape.as_str() {
                "box" => EmitterShape::Box,
                "sphere" => EmitterShape::Sphere,
                "cylinder" => EmitterShape::Cylinder,
                _ => EmitterShape::Point,
            }
        };
        let motion = match self.velocity_mode.as_str() {
            "cone" => VelocityMode::Cone,
            "target" => VelocityMode::Target,
            "normalized-vector" => VelocityMode::NormalizedVector,
            "surface-normal" if shape == EmitterShape::Mesh => VelocityMode::SurfaceNormal,
            _ => VelocityMode::Vector,
        };
        if shape == EmitterShape::Point && motion == VelocityMode::Vector {
            return None;
        }

        let angles = [self.emitter_yaw, self.emitter_pitch, self.emitter_roll]
            .map(|degrees| finite(degrees).rem_euclid(360.0) as f32);
        let rotation = euler_rotation(angles);
        let (surface, limits, weights) = surface_parameters(shape, extents, !self.emitter_fill);
        let shape_axes = std::array::from_fn(|column| {
            let basis = rotation.matrix().column(column) * extents[column] as f32;
            pack(basis, limits[column])
        });
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
        Some(ParticleMotion {
            shape: shape_axes,
            surface_weights: weights,
            axis_speed: pack(axis, speed as f32),
            right_cosine: pack(right, cone_cosine),
            up_variation: pack(up, variation),
            target: pack(Vector3::from(target), 0.0),
            target_spread: pack(Vector3::from(spread), 0.0),
            modes: [shape as u32, u32::from(surface), motion as u32, 0],
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
    shape: EmitterShape,
    [x, y, z]: [f64; 3],
    surface: bool,
) -> (bool, [f32; 3], [f32; 4]) {
    let defaults = (false, [0.0; 3], [0.0; 4]);
    if !surface {
        return defaults;
    }
    let [yz, xz, xy] = [y * z, x * z, x * y];
    match shape {
        EmitterShape::Box if yz + xz + xy > 0.0 => {
            let area = yz + xz + xy;
            (
                true,
                [(yz / area) as f32, ((yz + xz) / area) as f32, 0.0],
                [0.0; 4],
            )
        }
        EmitterShape::Sphere if yz.max(xz).max(xy) > 0.0 => {
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
        EmitterShape::Cylinder if x + z > 0.0 && (y > 0.0 || xz > 0.0) => {
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
