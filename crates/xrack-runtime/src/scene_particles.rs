//! Shared emitter projection and time sampling for GPU sprite particles.
use std::sync::Arc;

use crate::scene_particle_motion::ParticleMotion;
use crate::{Color, Image, ImageAsset, ImageFormat, RecordList, Value, VecModel, Vector3D};

use super::scene3d::{MAIN_LAYER, Mesh3D, ParticleMeshSurface, Point3D};

pub const MAX_PARTICLE_EMITTERS: usize = 16;
pub const MAX_PARTICLES: u32 = 8192;
pub const MAX_PARTICLE_TEXTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PER_EMITTER: u32 = 4096;

#[doc(hidden)]
pub struct ParticleEmitterDefinition {
    pub position: Point3D,
    pub color: Color,
    pub lifetime: f64,
    pub size: f64,
    pub end_size: Option<f64>,
    pub start_time: f64,
    pub rate: f64,
    pub burst_count: i64,
    pub velocity: Vector3D,
    pub velocity_spread: Vector3D,
    pub gravity: Vector3D,
    pub fade_in: f64,
    pub fade_out: f64,
    pub intensity: f64,
    pub seed: i64,
    pub layers: i64,
    pub enabled: bool,
    pub sprite: Image,
    pub sprite_frames: i64,
    pub sprite_frame: Option<i64>,
    pub sprite_duration: f64,
    pub sprite_duration_variation: f64,
    pub sprite_playback: String,
    pub sprite_interpolate: bool,
    pub sprite_random_start: bool,
    pub color_table: Image,
    pub particle_blend_mode: String,
    pub emitter_shape: String,
    pub emitter_mesh: String,
    pub emitter_mesh_scale: Vector3D,
    pub emitter_extents: Vector3D,
    pub emitter_fill: bool,
    pub emitter_yaw: f64,
    pub emitter_pitch: f64,
    pub emitter_roll: f64,
    pub velocity_mode: String,
    pub velocity_cone_angle: f64,
    pub velocity_target: Point3D,
    pub velocity_target_spread: Vector3D,
    pub velocity_speed_variation: f64,
    pub trail_segments: i64,
    pub trail_duration: f64,
    pub trail_end_scale: f64,
    pub trail_fade: f64,
    pub trail_eol_fade: f64,
    pub particle_model: String,
    pub particle_rotation: Vector3D,
    pub particle_rotation_variation: Vector3D,
    pub particle_angular_velocity: Vector3D,
    pub particle_casts_shadow: bool,
    pub particle_receives_shadow: bool,
    pub particle_casts_reflections: bool,
}

impl Default for ParticleEmitterDefinition {
    fn default() -> Self {
        Self {
            position: Point3D::default(),
            color: Color(0xffffffff),
            lifetime: 1.0,
            size: 0.1,
            end_size: None,
            start_time: 0.0,
            rate: 0.0,
            burst_count: 0,
            velocity: Vector3D::default(),
            velocity_spread: Vector3D::default(),
            gravity: Vector3D::default(),
            fade_in: 0.0,
            fade_out: 0.25,
            intensity: 1.0,
            seed: 0,
            layers: MAIN_LAYER as i64,
            enabled: true,
            sprite: Image::default(),
            sprite_frames: 1,
            sprite_frame: None,
            sprite_duration: 0.0,
            sprite_duration_variation: 0.0,
            sprite_playback: String::new(),
            sprite_interpolate: true,
            sprite_random_start: false,
            color_table: Image::default(),
            particle_blend_mode: String::new(),
            emitter_shape: String::new(),
            emitter_mesh: String::new(),
            emitter_mesh_scale: Vector3D::new(1.0, 1.0, 1.0),
            emitter_extents: Vector3D::default(),
            emitter_fill: true,
            emitter_yaw: 0.0,
            emitter_pitch: 0.0,
            emitter_roll: 0.0,
            velocity_mode: String::new(),
            velocity_cone_angle: 0.0,
            velocity_target: Point3D::default(),
            velocity_target_spread: Vector3D::default(),
            velocity_speed_variation: 0.0,
            trail_segments: 0,
            trail_duration: 0.25,
            trail_end_scale: 0.12,
            trail_fade: 1.0,
            trail_eol_fade: 0.0,
            particle_model: String::new(),
            particle_rotation: Vector3D::default(),
            particle_rotation_variation: Vector3D::default(),
            particle_angular_velocity: Vector3D::default(),
            particle_casts_shadow: false,
            particle_receives_shadow: true,
            particle_casts_reflections: false,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEmitter {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub velocity_spread: [f32; 3],
    pub gravity: [f32; 3],
    pub color: Color,
    pub intensity: f32,
    pub size: [f32; 2],
    pub fade: [f32; 2],
    pub lifetime: f64,
    pub seed: u32,
    pub layers: u64,
    pub sprite: Option<ParticleSprite>,
    pub color_table: Option<Image>,
    pub blend: ParticleBlend,
    pub motion: Option<ParticleMotion>,
    pub mesh: Option<Arc<ParticleMeshSurface>>,
    pub trail: Option<ParticleTrail>,
    pub model: Option<ParticleModel>,
    start_time: f64,
    rate: f64,
    burst_count: u32,
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleModel {
    pub meshes: Arc<[Mesh3D]>,
    pub rotation: [f32; 3],
    pub rotation_variation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub casts_shadow: bool,
    pub receives_shadow: bool,
    pub casts_reflections: bool,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleTrail {
    pub segments: u32,
    pub duration: f32,
    pub end_scale: f32,
    pub fade: f32,
    pub eol_fade: f32,
}

/// A validated horizontal sprite sheet and its playback parameters.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleSprite {
    pub image: Image,
    pub frames: u32,
    pub frame_offset: u32,
    pub duration: f32,
    pub duration_variation: f32,
    pub playback: SpritePlayback,
    pub interpolate: bool,
    pub random_start: bool,
}

/// Values are shared with the particle vertex shader.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SpritePlayback {
    Loop = 0,
    Reverse = 1,
    PingPong = 2,
    PingPongReverse = 3,
    Once = 4,
    SingleFrame = 5,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ParticleBlend {
    #[default]
    SourceOver,
    Additive,
    Screen,
    Multiply,
}

impl ParticleBlend {
    fn from_name(name: &str) -> Self {
        match name {
            "additive" => Self::Additive,
            "screen" => Self::Screen,
            "multiply" => Self::Multiply,
            _ => Self::SourceOver,
        }
    }
}

fn particle_png(image: &Image) -> Option<[u32; 2]> {
    let asset = ImageAsset::from(image.clone());
    let pixels = asset.pixels()?;
    let [width, height] = pixels.size;
    (pixels.format == ImageFormat::Png
        && pixels.edges.is_none()
        && width > 0
        && height > 0
        && u64::from(width) * u64::from(height) <= MAX_PARTICLE_TEXTURE_BYTES / 4)
        .then_some(pixels.size)
}

impl SpritePlayback {
    fn from_name(name: &str) -> Self {
        match name {
            "reverse" => Self::Reverse,
            "ping-pong" => Self::PingPong,
            "ping-pong-reverse" => Self::PingPongReverse,
            "once" => Self::Once,
            "single-frame" => Self::SingleFrame,
            _ => Self::Loop,
        }
    }

    fn reversed(self) -> bool {
        matches!(self, Self::Reverse | Self::PingPongReverse)
    }
}

impl ParticleEmitterDefinition {
    fn prepare_trail(&self) -> Option<ParticleTrail> {
        if self.trail_segments <= 0
            || !(0.000_001..=1.0e8).contains(&self.trail_duration)
            || ![self.trail_end_scale, self.trail_fade, self.trail_eol_fade]
                .into_iter()
                .all(f64::is_finite)
        {
            return None;
        }
        Some(ParticleTrail {
            segments: self.trail_segments.min(16) as u32,
            duration: self.trail_duration as f32,
            end_scale: self.trail_end_scale.clamp(0.0, 2.0) as f32,
            fade: self.trail_fade.clamp(0.0, 1.0) as f32,
            eol_fade: self.trail_eol_fade.clamp(0.0, 1.0e8) as f32,
        })
    }

    fn prepare_sprite(&self) -> Option<ParticleSprite> {
        let [width, _] = particle_png(&self.sprite)?;
        let frames = u32::try_from(self.sprite_frames).ok()?;
        if !((1..=256).contains(&frames) && width.is_multiple_of(frames)) {
            return None;
        }
        let playback = SpritePlayback::from_name(&self.sprite_playback);
        let frame_offset = self.sprite_frame.map_or(0, |frame| {
            let index = frame.clamp(0, i64::from(frames - 1)) as u32;
            if playback.reversed() {
                frames - 1 - index
            } else {
                index
            }
        });
        let duration = if (0.000_001..=1.0e8).contains(&self.sprite_duration) {
            self.sprite_duration
        } else {
            self.lifetime
        };
        let variation = self.sprite_duration_variation;
        let duration_variation = if variation.is_finite() {
            variation.clamp(0.0, 1.0e8)
        } else {
            0.0
        };
        Some(ParticleSprite {
            image: self.sprite.clone(),
            frames,
            frame_offset,
            duration: duration as f32,
            duration_variation: duration_variation as f32,
            playback,
            interpolate: self.sprite_interpolate,
            random_start: self.sprite_random_start,
        })
    }

    pub fn prepare(
        self,
        resolve_surface: impl FnOnce(&str, [f32; 3]) -> Option<Arc<ParticleMeshSurface>>,
        resolve_model: impl FnOnce(&str) -> Option<Arc<[Mesh3D]>>,
    ) -> Option<ParticleEmitter> {
        let position = [self.position.x, self.position.y, self.position.z];
        let velocity = [self.velocity.x, self.velocity.y, self.velocity.z];
        let spread = [
            self.velocity_spread.x,
            self.velocity_spread.y,
            self.velocity_spread.z,
        ];
        let gravity = [self.gravity.x, self.gravity.y, self.gravity.z];
        let end_size = self.end_size.unwrap_or(self.size);
        if !self.enabled
            || self.layers == 0
            || self.color.0 & 0xff == 0
            || !self.start_time.is_finite()
            || !(0.000_001..=1.0e8).contains(&self.lifetime)
            || !(0.0..=1.0e8).contains(&self.size)
            || !(0.0..=1.0e8).contains(&end_size)
            || self.size.max(end_size) == 0.0
            || ![self.rate, self.fade_in, self.fade_out, self.intensity]
                .into_iter()
                .all(|value| (0.0..=1.0e8).contains(&value))
            || self.intensity == 0.0
            || self.rate == 0.0 && self.burst_count <= 0
            || ![position, velocity, spread, gravity]
                .into_iter()
                .flatten()
                .all(|value| value.abs() <= 1.0e8)
        {
            return None;
        }
        let mesh = if self.emitter_shape == "mesh" {
            let scale = [
                self.emitter_mesh_scale.x,
                self.emitter_mesh_scale.y,
                self.emitter_mesh_scale.z,
            ];
            if !scale.iter().all(|value| (0.0..=1.0e8).contains(value)) {
                return None;
            }
            Some(resolve_surface(
                &self.emitter_mesh,
                scale.map(|value| value as f32),
            )?)
        } else {
            None
        };
        let model = if self.particle_model.is_empty() {
            None
        } else {
            let angles = |value: Vector3D| {
                let values = [value.x, value.y, value.z];
                values
                    .iter()
                    .all(|value| (-1.0e8..=1.0e8).contains(value))
                    .then(|| values.map(|value| value as f32))
            };
            Some(ParticleModel {
                meshes: resolve_model(&self.particle_model)?,
                rotation: angles(self.particle_rotation)?,
                rotation_variation: angles(self.particle_rotation_variation)?.map(f32::abs),
                angular_velocity: angles(self.particle_angular_velocity)?,
                casts_shadow: self.particle_casts_shadow,
                receives_shadow: self.particle_receives_shadow,
                casts_reflections: self.particle_casts_reflections,
            })
        };
        Some(ParticleEmitter {
            position: position.map(|value| value as f32),
            velocity: velocity.map(|value| value as f32),
            velocity_spread: spread.map(|value| value.abs() as f32),
            gravity: gravity.map(|value| value as f32),
            color: self.color,
            intensity: self.intensity.min(64.0) as f32,
            size: [self.size as f32, end_size as f32],
            fade: [self.fade_in as f32, self.fade_out as f32],
            lifetime: self.lifetime,
            seed: self.seed as u32,
            layers: self.layers as u64,
            sprite: self.prepare_sprite(),
            motion: self.prepare_motion(velocity),
            trail: if model.is_none() {
                self.prepare_trail()
            } else {
                None
            },
            model,
            mesh,
            color_table: particle_png(&self.color_table).map(|_| self.color_table),
            blend: ParticleBlend::from_name(&self.particle_blend_mode),
            start_time: self.start_time,
            rate: self.rate,
            burst_count: self.burst_count.clamp(0, i64::from(MAX_PER_EMITTER)) as u32,
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ParticleSample {
    pub count: u32,
    pub burst_count: u32,
    pub burst_age: f32,
    pub interval: f32,
    pub youngest_age: f32,
    pub latest_id: u32,
}

impl ParticleSample {
    pub fn youngest_age(&self) -> f32 {
        if self.count == self.burst_count {
            self.burst_age
        } else {
            self.youngest_age
        }
    }

    pub fn oldest_age(&self) -> f32 {
        let emitted = self.count - self.burst_count;
        let oldest_stream = if emitted == 0 {
            0.0
        } else {
            self.youngest_age + (emitted - 1) as f32 * self.interval
        };
        self.burst_age.max(oldest_stream)
    }
}

impl ParticleEmitter {
    /// Conservatively fit one emitter without reading instance positions back from the GPU.
    pub fn shadow_bounds<'a>(
        &self,
        sample: &ParticleSample,
        affectors: impl Iterator<Item = &'a crate::scene_particle_affectors::ParticleAffector>,
    ) -> Option<[[f32; 3]; 2]> {
        let model = self.model.as_ref()?;
        if !model.casts_shadow || !model.meshes.iter().any(Mesh3D::has_opaque_parts) {
            return None;
        }
        let youngest = f64::from(sample.youngest_age().next_down()).max(0.0);
        let oldest = f64::from(sample.oldest_age().next_up());
        let spawn = self.motion.as_ref().map_or([0.0; 3], |motion| {
            motion.spawn_extent(self.mesh.as_ref().map(|mesh| mesh.radius))
        });
        let speed = self
            .motion
            .as_ref()
            .and_then(ParticleMotion::max_directed_speed);
        let position = self.position.map(f64::from);
        let axes: [[f64; 2]; 3] = std::array::from_fn(|axis| {
            let velocity = f64::from(self.velocity[axis]);
            let spread = f64::from(self.velocity_spread[axis]);
            let [slow, fast] = speed.map_or([velocity - spread, velocity + spread], |v| [-v, v]);
            let acceleration = f64::from(self.gravity[axis]);
            let extreme = |v: f64, select: fn(f64, f64) -> f64| {
                let offset = |age: f64| age * (v + 0.5 * age * acceleration);
                let turn = if acceleration != 0.0 {
                    (-v / acceleration).clamp(youngest, oldest)
                } else {
                    youngest
                };
                select(select(offset(youngest), offset(oldest)), offset(turn))
            };
            [
                position[axis] + extreme(slow, f64::min) - spawn[axis],
                position[axis] + extreme(fast, f64::max) + spawn[axis],
            ]
        });
        let mut bounds = [axes.map(|axis| axis[0]), axes.map(|axis| axis[1])];
        for affector in affectors {
            affector.expand_particle_bounds(&mut bounds, oldest);
        }
        let extent = model
            .meshes
            .iter()
            .map(|mesh| {
                let (min, max) = mesh.bounds();
                (0..3)
                    .map(|axis| f64::from(min[axis].abs().max(max[axis].abs())).powi(2))
                    .sum::<f64>()
            })
            .fold(0.0, f64::max)
            .sqrt();
        let scale = f64::from(self.size[0].max(self.size[1]));
        let result = std::array::from_fn(|side| {
            std::array::from_fn(|axis| {
                let value = bounds[side][axis];
                let margin = extent * scale + value.abs() * f64::from(32.0 * f32::EPSILON);
                if side == 0 {
                    ((value - margin) as f32).next_down()
                } else {
                    ((value + margin) as f32).next_up()
                }
            })
        });
        result
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(result)
    }

    pub fn sample(&self, time: f64, remaining: u32) -> Option<ParticleSample> {
        let elapsed = time - self.start_time;
        if !elapsed.is_finite() || elapsed < 0.0 {
            return None;
        }
        let budget = remaining.min(MAX_PER_EMITTER);
        let lifetime = self.lifetime + self.trail.map_or(0.0, |trail| f64::from(trail.eol_fade));
        let mut interval = 0.0;
        let mut youngest_age = 0.0;
        let mut latest_id = 0;
        let mut stream = 0;
        let ticks = elapsed * self.rate;
        // Beyond this range f64 can no longer distinguish adjacent emissions.
        if (1.0..(1_u64 << 52) as f64).contains(&ticks) {
            let latest = ticks.floor();
            interval = self.rate.recip();
            youngest_age = (ticks - latest) * interval;
            if youngest_age < lifetime {
                latest_id = latest as u64 as u32;
                let alive = ((lifetime - youngest_age) * self.rate).ceil();
                stream = (alive.min(latest) as u32).min(budget);
            }
        }
        // Prefer more recent emissions when the budget is saturated.
        let burst = if elapsed < lifetime {
            self.burst_count.min(budget - stream)
        } else {
            0
        };
        (burst + stream != 0).then_some(ParticleSample {
            count: burst + stream,
            burst_count: burst,
            burst_age: if burst > 0 { elapsed as f32 } else { 0.0 },
            interval: interval.min(lifetime) as f32,
            youngest_age: youngest_age as f32,
            latest_id,
        })
    }
}

#[doc(hidden)]
pub fn model_particles<T: Clone>(
    list: &VecModel<T>,
    project: impl Fn(T) -> Option<ParticleEmitter>,
) -> Arc<[ParticleEmitter]> {
    (0..list.len())
        .filter_map(|index| project(list.row(index)?))
        .take(MAX_PARTICLE_EMITTERS)
        .collect()
}

#[doc(hidden)]
pub fn record_particles(
    list: &RecordList,
    resolve_surface: impl Fn(&str, [f32; 3]) -> Option<Arc<ParticleMeshSurface>>,
    resolve_model: impl Fn(&str) -> Option<Arc<[Mesh3D]>>,
) -> Arc<[ParticleEmitter]> {
    model_particles(list.model(), |row| {
        let Value::Point3D(position) = row.field("position")? else {
            return None;
        };
        let Value::Color(color) = row.field("color")? else {
            return None;
        };
        let float = |name| match row.field(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        let integer = |name| match row.field(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        };
        let vector = |name| match row.field(name) {
            Some(Value::Vector3D(value)) => *value,
            _ => Vector3D::default(),
        };
        let image = |name| match row.field(name) {
            Some(Value::Image(image) | Value::ImageAsset(ImageAsset::Embedded(image))) => {
                image.clone()
            }
            Some(Value::ImageAsset(ImageAsset::Loaded(image))) => Image::Loaded(image.clone()),
            _ => Image::default(),
        };
        let defaults = ParticleEmitterDefinition::default();
        let name = |field| match row.field(field) {
            Some(Value::String(value)) => value.to_string(),
            _ => String::new(),
        };
        ParticleEmitterDefinition {
            position: *position,
            color: *color,
            size: float("size")?,
            lifetime: float("lifetime")?,
            end_size: float("end-size"),
            start_time: float("start-time").unwrap_or(defaults.start_time),
            rate: float("rate").unwrap_or(defaults.rate),
            burst_count: integer("burst-count").unwrap_or(defaults.burst_count),
            velocity: vector("velocity"),
            velocity_spread: vector("velocity-spread"),
            gravity: vector("gravity"),
            fade_in: float("fade-in").unwrap_or(defaults.fade_in),
            fade_out: float("fade-out").unwrap_or(defaults.fade_out),
            intensity: float("intensity").unwrap_or(defaults.intensity),
            seed: integer("seed").unwrap_or(defaults.seed),
            layers: integer("layers").unwrap_or(defaults.layers),
            enabled: row.field("enabled") != Some(&Value::Bool(false)),
            sprite: image("sprite"),
            sprite_frames: integer("sprite-frames").unwrap_or(defaults.sprite_frames),
            sprite_frame: integer("sprite-frame"),
            sprite_duration: float("sprite-duration").unwrap_or(defaults.sprite_duration),
            sprite_duration_variation: float("sprite-duration-variation")
                .unwrap_or(defaults.sprite_duration_variation),
            sprite_playback: name("sprite-playback"),
            sprite_interpolate: row.field("sprite-interpolate") != Some(&Value::Bool(false)),
            sprite_random_start: row.field("sprite-random-start") == Some(&Value::Bool(true)),
            color_table: image("color-table"),
            particle_blend_mode: name("particle-blend-mode"),
            emitter_shape: name("emitter-shape"),
            emitter_mesh: name("emitter-mesh"),
            emitter_mesh_scale: match row.field("emitter-mesh-scale") {
                Some(Value::Vector3D(scale)) => *scale,
                _ => defaults.emitter_mesh_scale,
            },
            emitter_extents: vector("emitter-extents"),
            emitter_fill: row.field("emitter-fill") != Some(&Value::Bool(false)),
            emitter_yaw: float("emitter-yaw").unwrap_or(defaults.emitter_yaw),
            emitter_pitch: float("emitter-pitch").unwrap_or(defaults.emitter_pitch),
            emitter_roll: float("emitter-roll").unwrap_or(defaults.emitter_roll),
            velocity_mode: name("velocity-mode"),
            velocity_cone_angle: float("velocity-cone-angle")
                .unwrap_or(defaults.velocity_cone_angle),
            velocity_target: match row.field("velocity-target") {
                Some(Value::Point3D(value)) => *value,
                _ => defaults.velocity_target,
            },
            velocity_target_spread: vector("velocity-target-spread"),
            velocity_speed_variation: float("velocity-speed-variation")
                .unwrap_or(defaults.velocity_speed_variation),
            trail_segments: integer("trail-segments").unwrap_or(defaults.trail_segments),
            trail_duration: float("trail-duration").unwrap_or(defaults.trail_duration),
            trail_end_scale: float("trail-end-scale").unwrap_or(defaults.trail_end_scale),
            trail_fade: float("trail-fade").unwrap_or(defaults.trail_fade),
            trail_eol_fade: float("trail-eol-fade").unwrap_or(defaults.trail_eol_fade),
            particle_model: name("particle-model"),
            particle_rotation: vector("particle-rotation"),
            particle_rotation_variation: vector("particle-rotation-variation"),
            particle_angular_velocity: vector("particle-angular-velocity"),
            particle_casts_shadow: row.field("particle-casts-shadow") == Some(&Value::Bool(true)),
            particle_receives_shadow: row.field("particle-receives-shadow")
                != Some(&Value::Bool(false)),
            particle_casts_reflections: row.field("particle-casts-reflections")
                == Some(&Value::Bool(true)),
        }
        .prepare(&resolve_surface, &resolve_model)
    })
}
