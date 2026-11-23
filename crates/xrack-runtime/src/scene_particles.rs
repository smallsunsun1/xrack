//! Shared emitter projection and time sampling for GPU sprite particles.
use std::sync::Arc;

use crate::scene_particle_motion::ParticleMotion;
use crate::{Color, Image, ImageAsset, ImageFormat, RecordList, Value, VecModel, Vector3D};

use super::scene3d::{MAIN_LAYER, ParticleMeshSurface, Point3D};

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
    start_time: f64,
    rate: f64,
    burst_count: u32,
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
            trail: self.prepare_trail(),
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
pub struct ParticleSample {
    pub count: u32,
    pub burst_count: u32,
    pub burst_age: f32,
    pub interval: f32,
    pub youngest_age: f32,
    pub latest_id: u32,
}

impl ParticleSample {
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
        }
        .prepare(&resolve_surface)
    })
}
