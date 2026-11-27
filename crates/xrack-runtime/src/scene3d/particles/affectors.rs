//! Immutable GPU parameters for timeline-sampled 3D particle affectors.
use std::sync::Arc;

use nalgebra::{Matrix3, Vector3};

use crate::{
    Node, Point3D, RecordList, Value, VecModel, Vector3D,
    scene3d::particles::motion::ParticleShape,
    scene3d::particles::surface::{ParticleMeshSurface, ParticleSceneShape},
    scene3d::particles::{ParticleEmitter, ParticleParent},
};

pub const MAX_PARTICLE_AFFECTORS: usize = 16;
const MAX_PARAMETER: f64 = 1.0e8;

#[doc(hidden)]
pub struct ParticleAffectorDefinition {
    pub affector_kind: String,
    pub parent: String,
    pub parent_node: String,
    pub particle_group: String,
    pub position: Point3D,
    pub position_variation: Vector3D,
    pub attractor_shape: String,
    pub attractor_extents: Vector3D,
    pub attractor_fill: bool,
    pub attractor_yaw: f64,
    pub attractor_pitch: f64,
    pub attractor_roll: f64,
    pub attractor_mesh: String,
    pub attractor_point_source: String,
    pub attractor_point_randomize: bool,
    pub attractor_point_scale: Vector3D,
    pub attractor_scene_root: String,
    pub attractor_scene_exclude: VecModel<String>,
    pub attractor_scene_center: Point3D,
    pub attractor_scene_extents: Vector3D,
    pub attractor_mesh_scale: Vector3D,
    pub attractor_samples: i64,
    pub direction: Option<Vector3D>,
    pub duration: f64,
    pub duration_variation: f64,
    pub start_age: f64,
    pub hide_at_end: bool,
    pub radius: f64,
    pub outer_radius: f64,
    pub strength: f64,
    pub angular_speed: f64,
    pub global_amount: Vector3D,
    pub global_pace: Vector3D,
    pub global_pace_start: Vector3D,
    pub unique_amount: Vector3D,
    pub unique_pace: Vector3D,
    pub unique_amount_variation: f64,
    pub unique_pace_variation: f64,
    pub fade_in_duration: f64,
    pub fade_out_duration: f64,
    pub min_size: f64,
    pub max_size: f64,
    pub scale_mode: String,
    pub scale_easing: String,
    pub seed: i64,
    pub layers: i64,
    pub enabled: bool,
}

impl Default for ParticleAffectorDefinition {
    fn default() -> Self {
        Self {
            affector_kind: String::new(),
            parent: String::new(),
            parent_node: String::new(),
            particle_group: String::new(),
            position: Point3D::default(),
            position_variation: Vector3D::default(),
            attractor_shape: String::new(),
            attractor_extents: Vector3D::new(1.0, 1.0, 1.0),
            attractor_fill: true,
            attractor_yaw: 0.0,
            attractor_pitch: 0.0,
            attractor_roll: 0.0,
            attractor_mesh: String::new(),
            attractor_point_source: String::new(),
            attractor_point_randomize: false,
            attractor_point_scale: Vector3D::new(1.0, 1.0, 1.0),
            attractor_scene_root: String::new(),
            attractor_scene_exclude: VecModel::default(),
            attractor_scene_center: Point3D::default(),
            attractor_scene_extents: Vector3D::default(),
            attractor_mesh_scale: Vector3D::new(1.0, 1.0, 1.0),
            attractor_samples: 0,
            direction: None,
            duration: -1.0,
            duration_variation: 0.0,
            start_age: 0.0,
            hide_at_end: false,
            radius: 0.0,
            outer_radius: 1.0,
            strength: 1.0,
            angular_speed: 10.0,
            global_amount: Vector3D::default(),
            global_pace: Vector3D::default(),
            global_pace_start: Vector3D::default(),
            unique_amount: Vector3D::default(),
            unique_pace: Vector3D::default(),
            unique_amount_variation: 0.0,
            unique_pace_variation: 0.0,
            fade_in_duration: 0.0,
            fade_out_duration: 0.0,
            min_size: 1.0,
            max_size: 1.0,
            scale_mode: String::new(),
            scale_easing: String::new(),
            seed: 0,
            layers: -1,
            enabled: true,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleAffector {
    pub layers: u64,
    pub position_age: [f32; 4],
    pub vector_value: [f32; 4],
    pub time_bounds: [f32; 4],
    pub control: [u32; 4],
    pub wander: [[f32; 4]; 2],
    pub frame: Option<ParticleAffectorFrame>,
    pub shape: ParticleShape,
    pub shape_mesh: Option<Arc<ParticleMeshSurface>>,
    scene_shape: Option<Arc<ParticleSceneShape>>,
    parent: Option<Arc<ParticleParent>>,
    group: Option<Arc<str>>,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleAffectorFrame {
    pub world_to_local: [[f32; 4]; 3],
    pub local_to_world: [[f32; 4]; 3],
}

impl ParticleAffectorFrame {
    fn new(local_to_world: [[f32; 4]; 3]) -> Option<Self> {
        let inverse =
            Matrix3::from_fn(|row, column| f64::from(local_to_world[row][column])).try_inverse()?;
        let origin = Vector3::from(local_to_world.map(|row| f64::from(row[3])));
        let offset = -(inverse * origin);
        let world_to_local = std::array::from_fn(|axis| {
            [
                inverse[(axis, 0)],
                inverse[(axis, 1)],
                inverse[(axis, 2)],
                offset[axis],
            ]
            .map(|value| value as f32)
        });
        world_to_local
            .iter()
            .flatten()
            .all(|value| value.is_finite() && value.abs() <= 1.0e6)
            .then_some(Self {
                world_to_local,
                local_to_world,
            })
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Kind {
    Attractor = 1,
    Repeller = 2,
    Gravity = 3,
    PointRotator = 4,
    Wander = 5,
    Scale = 6,
}

impl ParticleAffector {
    pub fn matches_emitter(&self, emitter: &ParticleEmitter, layers: u64) -> bool {
        self.layers & emitter.layers & layers != 0
            && (self.parent.is_none() || self.frame.is_some())
            && self
                .group
                .as_ref()
                .is_none_or(|group| emitter.group.as_ref() == Some(group))
    }

    /// Only the projected parameters participate in the GPU uniform cache key.
    pub fn parameters_match(&self, other: &Self) -> bool {
        self.position_age == other.position_age
            && self.vector_value == other.vector_value
            && self.time_bounds == other.time_bounds
            && self.control == other.control
            && self.wander == other.wander
            && self.frame == other.frame
            && self.shape == other.shape
            && self.shape_mesh == other.shape_mesh
    }

    pub(crate) fn max_scale(&self, age: f64) -> f64 {
        if self.control[0] == Kind::Scale as u32 && age >= f64::from(self.position_age[3]) {
            f64::from(self.vector_value[1]).max(1.0)
        } else {
            1.0
        }
    }

    pub(crate) fn expand_particle_bounds(&self, [min, max]: &mut [[f64; 3]; 2], age: f64) {
        let age = age - f64::from(self.position_age[3]);
        if age < 0.0 {
            return;
        }
        let position = self.position_age.map(f64::from);
        let vector = self.vector_value.map(f64::from);
        let world = self
            .frame
            .map(|frame| frame.local_to_world.map(|row| row.map(f64::from)));
        let world_point = |axis: usize| {
            world.map_or(position[axis], |rows| {
                (0..3).fold(rows[axis][3], |value, column| {
                    value + rows[axis][column] * position[column]
                })
            })
        };
        let world_radius = |axis: usize| {
            world.map_or(1.0, |rows| {
                rows[axis][..3]
                    .iter()
                    .map(|value| value.powi(2))
                    .sum::<f64>()
                    .sqrt()
            })
        };
        match self.control[0] {
            kind if kind == Kind::Attractor as u32 => {
                let extents = if self.shape.kind == 0 {
                    [vector[0], vector[1], vector[2]]
                } else {
                    self.shape
                        .extent(self.shape_mesh.as_ref().map(|mesh| mesh.radius))
                };
                for axis in 0..3 {
                    let extent = world.map_or(extents[axis], |rows| {
                        (0..3)
                            .map(|column| rows[axis][column].abs() * extents[column])
                            .sum()
                    });
                    let target = world_point(axis);
                    min[axis] = min[axis].min(target - extent);
                    max[axis] = max[axis].max(target + extent);
                }
            }
            kind if kind == Kind::Repeller as u32 => {
                for axis in 0..3 {
                    let offset = vector[3] * world_radius(axis);
                    min[axis] -= offset;
                    max[axis] += offset;
                }
            }
            kind if kind == Kind::Gravity as u32 => {
                let scale = 0.5 * age * age;
                for axis in 0..3 {
                    let acceleration = world.map_or(vector[axis], |rows| {
                        (0..3)
                            .map(|column| rows[axis][column] * vector[column])
                            .sum()
                    });
                    let offset = acceleration * scale;
                    min[axis] += offset.min(0.0);
                    max[axis] += offset.max(0.0);
                }
            }
            kind if kind == Kind::PointRotator as u32 => {
                let inverse = self
                    .frame
                    .map(|frame| frame.world_to_local.map(|row| row.map(f64::from)));
                let radius = (0..3)
                    .map(|axis| {
                        let [a, b] = std::array::from_fn(|side| {
                            inverse.map_or(if side == 0 { min[axis] } else { max[axis] }, |rows| {
                                (0..3).fold(rows[axis][3], |value, col| {
                                    let scale = rows[axis][col];
                                    let bound = if (side == 0) ^ (scale < 0.0) {
                                        min[col]
                                    } else {
                                        max[col]
                                    };
                                    value + scale * bound
                                })
                            })
                        });
                        (a - position[axis])
                            .abs()
                            .max((b - position[axis]).abs())
                            .powi(2)
                    })
                    .sum::<f64>()
                    .sqrt();
                for axis in 0..3 {
                    let pivot = world_point(axis);
                    let extent = radius * world_radius(axis);
                    min[axis] = min[axis].min(pivot - extent);
                    max[axis] = max[axis].max(pivot + extent);
                }
            }
            kind if kind == Kind::Wander as u32 => {
                let [unique, _] = self.wander;
                let extents: [f64; 3] = std::array::from_fn(|axis| {
                    vector[axis].abs()
                        + f64::from(unique[axis].abs()) * (1.0 + f64::from(unique[3]))
                });
                for axis in 0..3 {
                    let extent = world.map_or(extents[axis], |rows| {
                        (0..3)
                            .map(|column| rows[axis][column].abs() * extents[column])
                            .sum()
                    });
                    min[axis] -= extent;
                    max[axis] += extent;
                }
            }
            _ => {}
        }
    }
}

impl ParticleAffectorDefinition {
    pub fn prepare(
        self,
        resolve_surface: impl FnOnce(&str, [f32; 3]) -> Option<Arc<ParticleMeshSurface>>,
        resolve_points: impl FnOnce(&str, [f32; 3], Option<u32>) -> Option<Arc<ParticleMeshSurface>>,
    ) -> Option<ParticleAffector> {
        let kind = match self.affector_kind.as_str() {
            "attractor" => Kind::Attractor,
            "repeller" => Kind::Repeller,
            "gravity" => Kind::Gravity,
            "point-rotator" => Kind::PointRotator,
            "wander" => Kind::Wander,
            "scale" => Kind::Scale,
            _ => return None,
        };
        if !self.enabled
            || self.layers == 0
            || !(0.0..=MAX_PARAMETER).contains(&self.start_age)
            || self.parent.is_empty() && !self.parent_node.is_empty()
        {
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
            wander: [[0.0; 4]; 2],
            frame: None,
            shape: ParticleShape::default(),
            shape_mesh: None,
            scene_shape: None,
            parent: None,
            group: None,
        };
        if matches!(kind, Kind::Attractor | Kind::Repeller | Kind::PointRotator) {
            let position = [self.position.x, self.position.y, self.position.z];
            if !position.into_iter().all(valid) {
                return None;
            }
            result.position_age[..3].copy_from_slice(&position.map(|value| value as f32));
        }
        match kind {
            Kind::Attractor => {
                self.prepare_attractor(&mut result, resolve_surface, resolve_points)?;
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
            Kind::Wander => self.prepare_wander(&mut result)?,
            Kind::Scale => self.prepare_scale(&mut result)?,
        }
        result.parent = ParticleParent::new(self.parent, self.parent_node);
        result.group = (!self.particle_group.is_empty()).then(|| self.particle_group.into());
        Some(result)
    }

    fn prepare_attractor(
        &self,
        result: &mut ParticleAffector,
        resolve_surface: impl FnOnce(&str, [f32; 3]) -> Option<Arc<ParticleMeshSurface>>,
        resolve_points: impl FnOnce(&str, [f32; 3], Option<u32>) -> Option<Arc<ParticleMeshSurface>>,
    ) -> Option<()> {
        if !valid(self.duration) || !valid(self.duration_variation) {
            return None;
        }
        result.time_bounds[0] = self.duration as f32;
        result.time_bounds[1] = self.duration_variation.max(0.0) as f32;
        result.time_bounds[2] = self.attractor_samples.clamp(0, 8192) as f32;
        let angles = [
            self.attractor_yaw,
            self.attractor_pitch,
            self.attractor_roll,
        ];
        let extents = components(self.attractor_extents);
        match self.attractor_shape.as_str() {
            "" => {
                let spread = components(self.position_variation);
                if !spread.into_iter().all(valid) {
                    return None;
                }
                result.vector_value[..3].copy_from_slice(&spread.map(|value| value.abs() as f32));
                return Some(());
            }
            "point" => return Some(()),
            "mesh" => {
                let scale = components(self.attractor_mesh_scale);
                if !scale
                    .iter()
                    .all(|value| (0.0..=MAX_PARAMETER).contains(value))
                {
                    return None;
                }
                result.shape_mesh = Some(resolve_surface(
                    &self.attractor_mesh,
                    scale.map(|value| value as f32),
                )?);
            }
            "points" => {
                result.shape_mesh = Some(resolve_points(
                    &self.attractor_point_source,
                    [
                        self.attractor_point_scale.x,
                        self.attractor_point_scale.y,
                        self.attractor_point_scale.z,
                    ]
                    .map(|value| value as f32),
                    self.attractor_point_randomize.then_some(self.seed as u32),
                )?);
            }
            "scene" => {
                result.scene_shape = Some(ParticleSceneShape::new(
                    &self.attractor_scene_root,
                    self.attractor_scene_exclude.clone(),
                    self.attractor_scene_center,
                    self.attractor_scene_extents,
                    self.attractor_mesh_scale,
                )?);
            }
            "box" | "sphere" | "cylinder" => {
                if !extents
                    .iter()
                    .all(|value| (0.0..=MAX_PARAMETER).contains(value))
                {
                    return None;
                }
            }
            _ => return None,
        }
        result.shape = ParticleShape::new(
            &self.attractor_shape,
            extents,
            angles,
            !self.attractor_fill,
            false,
        );
        Some(())
    }

    fn prepare_wander(&self, result: &mut ParticleAffector) -> Option<()> {
        let [global, global_pace, phase, unique, unique_pace] = [
            self.global_amount,
            self.global_pace,
            self.global_pace_start,
            self.unique_amount,
            self.unique_pace,
        ]
        .map(components);
        if ![global, global_pace, phase, unique, unique_pace]
            .into_iter()
            .flatten()
            .all(valid)
            || ![self.fade_in_duration, self.fade_out_duration]
                .into_iter()
                .all(|v| (0.0..=MAX_PARAMETER).contains(&v))
            || ![self.unique_amount_variation, self.unique_pace_variation]
                .into_iter()
                .all(|v| v.is_finite())
        {
            return None;
        }
        let amplitude = |axis: usize, amounts: [f64; 3], pace: [f64; 3]| {
            if pace[axis] as f32 == 0.0 {
                0.0
            } else {
                amounts[axis] as f32
            }
        };
        let [x, y, z] = std::array::from_fn(|axis| amplitude(axis, global, global_pace));
        let [dx, dy, dz] = std::array::from_fn(|axis| amplitude(axis, unique, unique_pace));
        if [x, y, z, dx, dy, dz].into_iter().all(|v| v == 0.0) {
            return None;
        }
        let cycles = phase
            .map(|angle| (angle.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU) as f32);
        result.position_age[..3].copy_from_slice(&cycles);
        result.vector_value = [x, y, z, self.fade_in_duration as f32];
        let [px, py, pz] = global_pace.map(|value| value as f32);
        result.time_bounds = [px, py, pz, self.fade_out_duration as f32];
        let [ux, uy, uz] = unique_pace.map(|value| value as f32);
        result.wander = [
            [
                dx,
                dy,
                dz,
                self.unique_amount_variation.clamp(0.0, 1.0) as f32,
            ],
            [
                ux,
                uy,
                uz,
                self.unique_pace_variation.clamp(0.0, 1.0) as f32,
            ],
        ];
        Some(())
    }

    fn prepare_scale(&self, result: &mut ParticleAffector) -> Option<()> {
        let period = if self.duration == -1.0 {
            1.0
        } else {
            self.duration
        };
        if !(1e-6..=MAX_PARAMETER).contains(&period)
            || !(0.0..=64.0).contains(&self.min_size)
            || !(self.min_size..=64.0).contains(&self.max_size)
            || self.min_size == 1.0 && self.max_size == 1.0
        {
            return None;
        }
        let wave = match self.scale_mode.as_str() {
            "" | "linear" => 0,
            "triangle" => 1,
            "sine" => 2,
            "abs-sine" => 3,
            "step" => 4,
            "smooth-step" => 5,
            _ => return None,
        };
        let easing = match self.scale_easing.as_str() {
            "" | "linear" => 0,
            "ease-in" => 1,
            "ease-out" => 2,
            "ease-in-out" => 3,
            _ => return None,
        };
        result.vector_value = [
            self.min_size as f32,
            self.max_size as f32,
            period as f32,
            0.0,
        ];
        result.control[2] = wave | (easing << 3);
        Some(())
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

impl Node {
    pub(crate) fn prepare_affector_attachments(&mut self) {
        let Some(Value::SceneParticleAffectors(affectors)) =
            self.properties.get("particle-affectors")
        else {
            return;
        };
        if affectors
            .iter()
            .all(|affector| affector.parent.is_none() && affector.scene_shape.is_none())
        {
            return;
        }
        let scene = match self.properties.get("model") {
            Some(Value::SceneObjects(scene)) => Some(scene.as_ref()),
            _ => None,
        };
        let placed = affectors
            .iter()
            .filter_map(|affector| {
                let mut affector = affector.clone();
                if let Some(shape) = affector.scene_shape.as_ref() {
                    let projected = scene?.particle_scene_surface(shape)?;
                    affector.shape_mesh = Some(projected.mesh);
                    if affector.parent.is_none()
                        && let Some(rows) = projected.placement
                    {
                        affector.frame = Some(ParticleAffectorFrame::new(rows)?);
                    }
                }
                if let Some(parent) = &affector.parent {
                    affector.frame = Some(ParticleAffectorFrame::new(parent.rows(scene?)?)?);
                }
                Some(affector)
            })
            .collect();
        self.properties
            .insert("particle-affectors", Value::SceneParticleAffectors(placed));
    }
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
pub fn record_particle_affectors(
    list: &RecordList,
    resolve_surface: impl Fn(&str, [f32; 3]) -> Option<Arc<ParticleMeshSurface>>,
    resolve_points: impl Fn(&str, [f32; 3], Option<u32>) -> Option<Arc<ParticleMeshSurface>>,
) -> Arc<[ParticleAffector]> {
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
        let name = |field| match row.field(field) {
            Some(Value::String(value)) => value.clone(),
            _ => String::new(),
        };
        ParticleAffectorDefinition {
            affector_kind: kind.to_string(),
            parent: name("parent"),
            parent_node: name("parent-node"),
            particle_group: name("particle-group"),
            position: match row.field("position") {
                Some(Value::Point3D(point)) => *point,
                _ => defaults.position,
            },
            position_variation: vector("position-variation").unwrap_or_default(),
            attractor_shape: name("attractor-shape"),
            attractor_extents: vector("attractor-extents").unwrap_or(defaults.attractor_extents),
            attractor_fill: row.field("attractor-fill") != Some(&Value::Bool(false)),
            attractor_yaw: float("attractor-yaw", 0.0),
            attractor_pitch: float("attractor-pitch", 0.0),
            attractor_roll: float("attractor-roll", 0.0),
            attractor_mesh: name("attractor-mesh"),
            attractor_point_source: name("attractor-point-source"),
            attractor_point_randomize: row.field("attractor-point-randomize")
                == Some(&Value::Bool(true)),
            attractor_point_scale: match row.field("attractor-point-scale") {
                Some(Value::Vector3D(scale)) => *scale,
                _ => defaults.attractor_point_scale,
            },
            attractor_scene_root: name("attractor-scene-root"),
            attractor_scene_exclude: ParticleSceneShape::record_exclusions(
                row.field("attractor-scene-exclude"),
            ),
            attractor_scene_center: match row.field("attractor-scene-center") {
                Some(Value::Point3D(center)) => *center,
                _ => Point3D::default(),
            },
            attractor_scene_extents: match row.field("attractor-scene-extents") {
                Some(Value::Vector3D(extents)) => *extents,
                _ => Vector3D::default(),
            },
            attractor_mesh_scale: vector("attractor-mesh-scale")
                .unwrap_or(defaults.attractor_mesh_scale),
            attractor_samples: match row.field("attractor-samples") {
                Some(Value::Int(value)) => *value,
                _ => 0,
            },
            direction: vector("direction"),
            duration: float("duration", defaults.duration),
            duration_variation: float("duration-variation", defaults.duration_variation),
            start_age: float("start-age", defaults.start_age),
            hide_at_end: row.field("hide-at-end") == Some(&Value::Bool(true)),
            radius: float("radius", defaults.radius),
            outer_radius: float("outer-radius", defaults.outer_radius),
            strength: float("strength", defaults.strength),
            angular_speed: float("angular-speed", defaults.angular_speed),
            global_amount: vector("global-amount").unwrap_or_default(),
            global_pace: vector("global-pace").unwrap_or_default(),
            global_pace_start: vector("global-pace-start").unwrap_or_default(),
            unique_amount: vector("unique-amount").unwrap_or_default(),
            unique_pace: vector("unique-pace").unwrap_or_default(),
            unique_amount_variation: float(
                "unique-amount-variation",
                defaults.unique_amount_variation,
            ),
            unique_pace_variation: float("unique-pace-variation", defaults.unique_pace_variation),
            fade_in_duration: float("fade-in-duration", defaults.fade_in_duration),
            fade_out_duration: float("fade-out-duration", defaults.fade_out_duration),
            min_size: float("min-size", defaults.min_size),
            max_size: float("max-size", defaults.max_size),
            scale_mode: name("scale-mode"),
            scale_easing: name("scale-easing"),
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
        .prepare(&resolve_surface, &resolve_points)
    })
}
