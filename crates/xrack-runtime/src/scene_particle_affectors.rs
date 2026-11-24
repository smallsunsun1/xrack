//! Immutable GPU parameters for timeline-sampled 3D particle affectors.
use std::sync::Arc;

use nalgebra::Vector3;

use crate::{Point3D, RecordList, Value, VecModel, Vector3D};

pub const MAX_PARTICLE_AFFECTORS: usize = 16;
const MAX_PARAMETER: f64 = 1.0e8;

#[doc(hidden)]
pub struct ParticleAffectorDefinition {
    pub affector_kind: String,
    pub position: Point3D,
    pub position_variation: Vector3D,
    pub direction: Option<Vector3D>,
    pub duration: f64,
    pub duration_variation: f64,
    pub start_age: f64,
    pub hide_at_end: bool,
    pub radius: f64,
    pub outer_radius: f64,
    pub strength: f64,
    pub angular_speed: f64,
    pub seed: i64,
    pub layers: i64,
    pub enabled: bool,
}

impl Default for ParticleAffectorDefinition {
    fn default() -> Self {
        Self {
            affector_kind: String::new(),
            position: Point3D::default(),
            position_variation: Vector3D::default(),
            direction: None,
            duration: -1.0,
            duration_variation: 0.0,
            start_age: 0.0,
            hide_at_end: false,
            radius: 0.0,
            outer_radius: 1.0,
            strength: 1.0,
            angular_speed: 10.0,
            seed: 0,
            layers: -1,
            enabled: true,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleAffector {
    pub layers: u64,
    pub position_age: [f32; 4],
    pub vector_value: [f32; 4],
    pub time_bounds: [f32; 4],
    pub control: [u32; 4],
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Kind {
    Attractor = 1,
    Repeller = 2,
    Gravity = 3,
    PointRotator = 4,
}

impl ParticleAffector {
    pub(crate) fn expand_particle_bounds(&self, [min, max]: &mut [[f64; 3]; 2], age: f64) {
        let age = age - f64::from(self.position_age[3]);
        if age < 0.0 {
            return;
        }
        let position = self.position_age.map(f64::from);
        let vector = self.vector_value.map(f64::from);
        match self.control[0] {
            kind if kind == Kind::Attractor as u32 => {
                for axis in 0..3 {
                    min[axis] = min[axis].min(position[axis] - vector[axis]);
                    max[axis] = max[axis].max(position[axis] + vector[axis]);
                }
            }
            kind if kind == Kind::Repeller as u32 => {
                for axis in 0..3 {
                    min[axis] -= vector[3];
                    max[axis] += vector[3];
                }
            }
            kind if kind == Kind::Gravity as u32 => {
                let scale = 0.5 * age * age;
                for axis in 0..3 {
                    let offset = vector[axis] * scale;
                    min[axis] += offset.min(0.0);
                    max[axis] += offset.max(0.0);
                }
            }
            kind if kind == Kind::PointRotator as u32 => {
                let radius = (0..3)
                    .map(|axis| {
                        (min[axis] - position[axis])
                            .abs()
                            .max((max[axis] - position[axis]).abs())
                            .powi(2)
                    })
                    .sum::<f64>()
                    .sqrt();
                for axis in 0..3 {
                    min[axis] = position[axis] - radius;
                    max[axis] = position[axis] + radius;
                }
            }
            _ => {}
        }
    }
}

impl ParticleAffectorDefinition {
    pub fn prepare(self) -> Option<ParticleAffector> {
        let kind = match self.affector_kind.as_str() {
            "attractor" => Kind::Attractor,
            "repeller" => Kind::Repeller,
            "gravity" => Kind::Gravity,
            "point-rotator" => Kind::PointRotator,
            _ => return None,
        };
        if !self.enabled || self.layers == 0 || !(0.0..=MAX_PARAMETER).contains(&self.start_age) {
            return None;
        }
        let mut result = ParticleAffector {
            layers: self.layers as u64,
            position_age: [0.0, 0.0, 0.0, self.start_age as f32],
            vector_value: [0.0; 4],
            time_bounds: [0.0; 4],
            control: [
                kind as u32,
                self.seed as u32,
                u32::from(self.hide_at_end),
                0,
            ],
        };
        if !matches!(kind, Kind::Gravity) {
            let position = [self.position.x, self.position.y, self.position.z];
            if !position.into_iter().all(valid) {
                return None;
            }
            result.position_age[..3].copy_from_slice(&position.map(|value| value as f32));
        }
        match kind {
            Kind::Attractor => {
                let spread = components(self.position_variation);
                if !spread.into_iter().all(valid)
                    || !valid(self.duration)
                    || !valid(self.duration_variation)
                {
                    return None;
                }
                result.vector_value[..3].copy_from_slice(&spread.map(|value| value.abs() as f32));
                result.time_bounds[0] = self.duration as f32;
                result.time_bounds[1] = self.duration_variation.max(0.0) as f32;
            }
            Kind::Repeller => {
                if !valid(self.radius)
                    || !valid(self.outer_radius)
                    || !valid(self.strength)
                    || self.strength <= 0.0
                {
                    return None;
                }
                let inner = self.radius.max(0.0);
                let outer = inner.max(self.outer_radius);
                if outer == 0.0 {
                    return None;
                }
                result.time_bounds[2] = inner as f32;
                result.time_bounds[3] = outer as f32;
                result.vector_value[3] = self.strength as f32;
            }
            Kind::Gravity => {
                if !valid(self.strength) || self.strength == 0.0 {
                    return None;
                }
                let acceleration = self.unit_direction(-1.0)? * self.strength;
                result.vector_value[..3].copy_from_slice(&[
                    acceleration.x as f32,
                    acceleration.y as f32,
                    acceleration.z as f32,
                ]);
            }
            Kind::PointRotator => {
                if !valid(self.angular_speed) || self.angular_speed == 0.0 {
                    return None;
                }
                let axis = self.unit_direction(1.0)?;
                result.vector_value = [
                    axis.x as f32,
                    axis.y as f32,
                    axis.z as f32,
                    self.angular_speed as f32,
                ];
            }
        }
        Some(result)
    }

    fn unit_direction(&self, default_y: f64) -> Option<Vector3<f64>> {
        let direction = components(
            self.direction
                .unwrap_or_else(|| Vector3D::new(0.0, default_y, 0.0)),
        );
        if !direction.into_iter().all(valid) {
            return None;
        }
        let vector = Vector3::from(direction);
        let length = vector.norm();
        (length > 0.0).then(|| vector / length)
    }
}

fn components(vector: Vector3D) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn valid(value: f64) -> bool {
    (-MAX_PARAMETER..=MAX_PARAMETER).contains(&value)
}

#[doc(hidden)]
pub fn model_particle_affectors<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<ParticleAffector>,
) -> Arc<[ParticleAffector]> {
    (0..model.len())
        .filter_map(|index| project(model.row(index)?))
        .take(MAX_PARTICLE_AFFECTORS)
        .collect()
}

#[doc(hidden)]
pub fn record_particle_affectors(list: &RecordList) -> Arc<[ParticleAffector]> {
    model_particle_affectors(list.model(), |row| {
        let Value::String(kind) = row.field("affector-kind")? else {
            return None;
        };
        let vector = |name| match row.field(name) {
            Some(Value::Vector3D(value)) => Some(*value),
            _ => None,
        };
        let float = |name, default| match row.field(name) {
            Some(Value::Float(value)) => *value,
            _ => default,
        };
        let defaults = ParticleAffectorDefinition::default();
        ParticleAffectorDefinition {
            affector_kind: kind.to_string(),
            position: match row.field("position") {
                Some(Value::Point3D(point)) => *point,
                _ => defaults.position,
            },
            position_variation: vector("position-variation").unwrap_or_default(),
            direction: vector("direction"),
            duration: float("duration", defaults.duration),
            duration_variation: float("duration-variation", defaults.duration_variation),
            start_age: float("start-age", defaults.start_age),
            hide_at_end: row.field("hide-at-end") == Some(&Value::Bool(true)),
            radius: float("radius", defaults.radius),
            outer_radius: float("outer-radius", defaults.outer_radius),
            strength: float("strength", defaults.strength),
            angular_speed: float("angular-speed", defaults.angular_speed),
            seed: match row.field("seed") {
                Some(Value::Int(value)) => *value,
                _ => defaults.seed,
            },
            layers: match row.field("layers") {
                Some(Value::Int(value)) => *value,
                _ => defaults.layers,
            },
            enabled: row.field("enabled") != Some(&Value::Bool(false)),
        }
        .prepare()
    })
}
