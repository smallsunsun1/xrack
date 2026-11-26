//! Shared emitter projection and time sampling for GPU sprite particles.
use std::sync::Arc;

use crate::scene_particle_bursts::{BirthSpan, ParticleBurstSchedule, retain_newest};
use crate::scene_particle_motion::ParticleMotion;
use crate::scene_particle_track::{
    MAX_EMISSION_TIME, ParticleEmissionAnimation, ParticleEmissionTrack, ParticleLoopClock,
};
use crate::scene_projection::SceneProjectionCache;
use crate::{Color, Image, ImageAsset, ImageFormat, Node, RecordList, Value, VecModel, Vector3D};

use super::scene3d::{MAIN_LAYER, Mesh3D, MeshAsset, ParticleMeshSurface, Point3D};

pub const MAX_PARTICLE_EMITTERS: usize = 16;
pub const MAX_PARTICLES: u32 = 8192;
pub const MAX_PARTICLE_TEXTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PER_EMITTER: u32 = 4096;

#[doc(hidden)]
pub struct ParticleEmitterDefinition {
    pub parent: String,
    pub parent_node: String,
    pub particle_group: String,
    pub emission_track: ParticleEmissionTrack,
    pub emission_loop: bool,
    pub position: Point3D,
    pub color: Color,
    pub lifetime: f64,
    pub lifetime_variation: f64,
    pub size: f64,
    pub size_variation: f64,
    pub end_size: Option<f64>,
    pub end_size_variation: Option<f64>,
    pub start_time: f64,
    pub rate: f64,
    pub bursts: ParticleBurstSchedule,
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
    pub particle_angular_velocity_variation: Vector3D,
    pub particle_casts_shadow: bool,
    pub particle_receives_shadow: bool,
    pub particle_casts_reflections: bool,
}

impl Default for ParticleEmitterDefinition {
    fn default() -> Self {
        Self {
            parent: String::new(),
            parent_node: String::new(),
            particle_group: String::new(),
            emission_track: ParticleEmissionTrack::default(),
            emission_loop: false,
            position: Point3D::default(),
            color: Color(0xffffffff),
            lifetime: 1.0,
            lifetime_variation: 0.0,
            size: 0.1,
            size_variation: 0.0,
            end_size: None,
            end_size_variation: None,
            start_time: 0.0,
            rate: 0.0,
            bursts: ParticleBurstSchedule::default(),
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
            particle_angular_velocity_variation: Vector3D::default(),
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
    pub size_variation: f32,
    pub end_size_variation: Option<f32>,
    pub fade: [f32; 2],
    pub lifetime: f64,
    pub lifetime_variation: f32,
    /// Rotation, rotation spread, angular velocity and angular velocity spread around the view axis.
    pub billboard_rotation: [f32; 4],
    pub seed: u32,
    pub layers: u64,
    pub sprite: Option<ParticleSprite>,
    pub color_table: Option<Image>,
    pub blend: ParticleBlend,
    pub motion: Option<ParticleMotion>,
    pub emission_track: ParticleEmissionTrack,
    emission_loop: bool,
    pub mesh: Option<Arc<ParticleMeshSurface>>,
    pub trail: Option<ParticleTrail>,
    pub model: Option<ParticleModel>,
    parent: Option<Arc<ParticleParent>>,
    pub(super) group: Option<Arc<str>>,
    start_time: f64,
    rate: f64,
    bursts: ParticleBurstSchedule,
}

#[derive(Debug, PartialEq)]
pub(super) struct ParticleParent {
    object: String,
    node: String,
}

impl ParticleParent {
    pub(super) fn new(object: String, node: String) -> Option<Arc<Self>> {
        (!object.is_empty()).then(|| Arc::new(Self { object, node }))
    }

    pub(super) fn rows(&self, scene: &super::scene3d::SceneModel) -> Option<[[f32; 4]; 3]> {
        scene.particle_parent_rows(&self.object, &self.node)
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleModel {
    pub meshes: Arc<[Mesh3D]>,
    pub rotation: [f32; 3],
    pub rotation_variation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angular_velocity_variation: [f32; 3],
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
            0.0
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
        let tracked = !self.emission_track.is_empty();
        if !tracked && self.parent.is_empty() && !self.parent_node.is_empty() {
            return None;
        }
        let velocity = [self.velocity.x, self.velocity.y, self.velocity.z];
        let spread = [
            self.velocity_spread.x,
            self.velocity_spread.y,
            self.velocity_spread.z,
        ];
        let gravity = [self.gravity.x, self.gravity.y, self.gravity.z];
        let end_size = self.end_size.unwrap_or(self.size);
        let end_spread = self.end_size_variation.unwrap_or(self.size_variation);
        if !self.enabled
            || self.layers == 0
            || self.color.0 & 0xff == 0
            || !self.start_time.is_finite()
            || !(0.000_001..=1.0e8).contains(&self.lifetime)
            || !(0.0..=1.0e8).contains(&self.size)
            || !(0.0..=1.0e8).contains(&end_size)
            || (self.size + self.size_variation).max(end_size + end_spread) == 0.0
            || ![
                self.rate,
                self.fade_in,
                self.fade_out,
                self.intensity,
                self.lifetime_variation,
                self.size_variation,
                end_spread,
            ]
            .into_iter()
            .all(|value| (0.0..=1.0e8).contains(&value))
            || self.intensity == 0.0
            || self.rate == 0.0 && self.bursts.is_empty()
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
        let angles = |value: Vector3D| {
            let values = [value.x, value.y, value.z];
            values
                .iter()
                .all(|value| (-1.0e8..=1.0e8).contains(value))
                .then(|| values.map(|value| value as f32))
        };
        let rotation = angles(self.particle_rotation)?;
        let rotation_variation = angles(self.particle_rotation_variation)?.map(f32::abs);
        let angular_velocity = angles(self.particle_angular_velocity)?;
        let angular_velocity_variation =
            angles(self.particle_angular_velocity_variation)?.map(f32::abs);
        let model = if self.particle_model.is_empty() {
            None
        } else {
            Some(ParticleModel {
                meshes: resolve_model(&self.particle_model)?,
                rotation,
                rotation_variation,
                angular_velocity,
                angular_velocity_variation,
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
            size_variation: self.size_variation as f32,
            end_size_variation: self.end_size_variation.map(|value| value as f32),
            fade: [self.fade_in as f32, self.fade_out as f32],
            lifetime: self.lifetime,
            lifetime_variation: self.lifetime_variation as f32,
            billboard_rotation: [
                rotation[2],
                rotation_variation[2],
                angular_velocity[2],
                angular_velocity_variation[2],
            ],
            seed: self.seed as u32,
            layers: self.layers as u64,
            sprite: self.prepare_sprite(),
            motion: self
                .prepare_motion(velocity)
                .or_else(|| tracked.then(ParticleMotion::default)),
            trail: if model.is_none() {
                self.prepare_trail()
            } else {
                None
            },
            model,
            mesh,
            color_table: particle_png(&self.color_table).map(|_| self.color_table),
            parent: (!tracked)
                .then(|| ParticleParent::new(self.parent, self.parent_node))
                .flatten(),
            group: (!self.particle_group.is_empty()).then(|| self.particle_group.into()),
            emission_track: self.emission_track,
            emission_loop: self.emission_loop,
            blend: ParticleBlend::from_name(&self.particle_blend_mode),
            start_time: self.start_time,
            rate: self.rate,
            bursts: self.bursts,
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ParticleSample {
    pub count: u32,
    pub seed_salt: u32,
    pub interval: f32,
    pub youngest_age: f32,
    pub latest_id: u32,
    /// Interval and elapsed times at the current and previous stream ID wraps.
    pub birth_clock: [f32; 3],
    pub loop_clock: Option<ParticleLoopClock>,
    pub birth_frame: Option<[[f32; 4]; 3]>,
}

impl ParticleSample {
    fn birth_time_range(&self) -> [f32; 2] {
        let [interval, epoch, previous] = self.birth_clock;
        let first = self.latest_id.wrapping_sub(self.count - 1);
        let start = if first > self.latest_id {
            previous
        } else {
            epoch
        };
        [
            (first as f32).mul_add(interval, start).next_down().max(0.0),
            (self.latest_id as f32).mul_add(interval, epoch).next_up(),
        ]
    }

    pub fn oldest_age(&self) -> f32 {
        self.youngest_age + (self.count - 1) as f32 * self.interval
    }
}

impl ParticleEmitter {
    fn birth_position_bounds(&self, sample: &ParticleSample) -> Option<[[f64; 3]; 2]> {
        let track = &self.emission_track;
        let Some(clock) = sample.loop_clock else {
            return track.position_bounds(sample.birth_time_range());
        };
        let first = sample.latest_id.wrapping_sub(sample.count - 1);
        let span = f64::from(sample.count - 1) * f64::from(sample.interval.next_up());
        if first > sample.latest_id || span >= f64::from(clock.duration.next_down()) {
            return track.position_bounds([0.0, clock.duration]);
        }

        let union = |a: [[f64; 3]; 2], b: [[f64; 3]; 2]| {
            [
                std::array::from_fn(|axis| a[0][axis].min(b[0][axis])),
                std::array::from_fn(|axis| a[1][axis].max(b[1][axis])),
            ]
        };
        let start = clock.time(first, sample.latest_id);
        let end = clock.time(sample.latest_id, sample.latest_id);
        let earliest = start.next_down().max(0.0);
        let latest = end.next_up().min(clock.duration);
        if start <= end {
            track.position_bounds([earliest, latest])
        } else {
            // The live births span the end and the start of one cycle.
            Some(union(
                track.position_bounds([earliest, clock.duration])?,
                track.position_bounds([0.0, latest])?,
            ))
        }
    }

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
        let youngest = f64::from(sample.youngest_age.next_down()).max(0.0);
        let oldest = f64::from(sample.oldest_age().next_up());
        let spawn = self.motion.as_ref().map_or([0.0; 3], |motion| {
            motion.spawn_extent(self.mesh.as_ref().map(|mesh| mesh.radius))
        });
        let speed = self
            .motion
            .as_ref()
            .and_then(ParticleMotion::max_directed_speed);
        let position = if sample.birth_frame.is_some() {
            [0.0; 3]
        } else {
            self.position.map(f64::from)
        };
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
        let frame = sample
            .birth_frame
            .or_else(|| self.motion.as_ref().and_then(|motion| motion.placement));
        if let Some(rows) = frame {
            let local = bounds;
            bounds = std::array::from_fn(|side| {
                std::array::from_fn(|axis| {
                    let [x, y, z, offset] = rows[axis].map(f64::from);
                    [x, y, z]
                        .into_iter()
                        .enumerate()
                        .fold(offset, |value, (column, scale)| {
                            value + scale * local[side ^ usize::from(scale < 0.0)][column]
                        })
                })
            });
        }
        if sample.birth_frame.is_none()
            && let Some(path) = self.birth_position_bounds(sample)
        {
            // A unit quaternion keeps all local positions inside this sphere.
            let extent = (0..3)
                .map(|i| bounds[0][i].abs().max(bounds[1][i].abs()).powi(2))
                .sum::<f64>()
                .sqrt();
            bounds = [
                path[0].map(|value| value - extent),
                path[1].map(|value| value + extent),
            ];
        }
        let mut scale_factor = 1.0;
        for affector in affectors {
            affector.expand_particle_bounds(&mut bounds, oldest);
            scale_factor *= affector.max_scale(oldest);
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
        let end_spread = self.end_size_variation.unwrap_or(self.size_variation);
        let largest = (self.size[0] + self.size_variation)
            .max(self.size[1] + end_spread)
            .min(1e8);
        let scale = f64::from(largest) * scale_factor.min(4096.0);
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

    pub fn shortest_lifetime(&self) -> f32 {
        (self.lifetime as f32 - self.lifetime_variation).max(1e-6)
    }

    fn longest_lifetime(&self) -> f64 {
        if self.lifetime_variation == 0.0 {
            self.lifetime
        } else {
            f64::from(
                (self.lifetime as f32 + self.lifetime_variation)
                    .min(1e8)
                    .next_up(),
            )
        }
    }

    pub fn samples(&self, time: f64, remaining: u32) -> impl Iterator<Item = ParticleSample> {
        let budget = remaining.min(MAX_PER_EMITTER);
        let elapsed = time - self.start_time;
        let lifetime =
            self.longest_lifetime() + self.trail.map_or(0.0, |trail| f64::from(trail.eol_fade));
        let available = budget > 0 && elapsed.is_finite() && elapsed >= 0.0;
        let mounted = self.parent.is_none()
            || self
                .motion
                .as_ref()
                .is_some_and(|motion| motion.placement.is_some());
        let stream = (available && mounted)
            .then(|| BirthSpan::continuous(elapsed, self.rate, lifetime, budget))
            .flatten();
        let mut leading = [stream, None];
        let mut bursts = Vec::new();
        if available && !self.bursts.is_empty() {
            let mut active = self
                .bursts
                .events()
                .iter()
                .take_while(|event| event.time <= elapsed)
                .filter(|event| mounted || event.frame.is_some())
                .filter_map(|event| event.sample(elapsed, self.seed, lifetime, budget));
            leading[1] = active.next();
            bursts.extend(active);
            retain_newest(&mut leading, &mut bursts, budget);
        }
        leading
            .into_iter()
            .flatten()
            .chain(bursts)
            .filter(|span| span.count > 0)
            .map(move |span| self.sample_span(span))
    }

    fn sample_span(&self, span: BirthSpan) -> ParticleSample {
        let mut sample = ParticleSample {
            count: span.count,
            interval: span.interval.min(2.0 * MAX_EMISSION_TIME) as f32,
            youngest_age: span.youngest_age as f32,
            latest_id: span.latest as u32,
            seed_salt: span.seed_salt,
            birth_clock: [0.0; 3],
            loop_clock: None,
            birth_frame: span.frame,
        };
        if self.emission_track.is_empty() || span.frame.is_some() {
            return sample;
        }
        let duration = self.emission_track.duration();
        if self.emission_loop && duration > 0.0 {
            sample.loop_clock = Some(if let Some(origin) = span.origin {
                ParticleLoopClock::for_burst(origin, span.interval, duration)
            } else {
                ParticleLoopClock::new(self.rate, span.latest, duration)
            });
        } else {
            let wrap = span.latest & !u64::from(u32::MAX);
            let origin = span.origin.unwrap_or_default();
            let time_at =
                |tick| (origin + tick as f64 * span.interval).min(MAX_EMISSION_TIME) as f32;
            sample.birth_clock = [
                span.interval.min(MAX_EMISSION_TIME) as f32,
                time_at(wrap),
                time_at(wrap.saturating_sub(1_u64 << 32)),
            ];
        }
        sample
    }
}

impl Node {
    pub(crate) fn prepare_particle_attachments(&mut self) {
        let Some(Value::SceneParticleEmitters(emitters)) = self.properties.get("particle-emitters")
        else {
            return;
        };
        if emitters.iter().all(|emitter| emitter.parent.is_none()) {
            return;
        }
        let scene = match self.properties.get("model") {
            Some(Value::SceneObjects(scene)) => Some(scene.as_ref()),
            _ => None,
        };
        let emitters = emitters
            .iter()
            .filter_map(|emitter| {
                let Some(parent) = &emitter.parent else {
                    return Some(emitter.clone());
                };
                let Some(rows) = scene.and_then(|scene| parent.rows(scene)) else {
                    return emitter
                        .bursts
                        .has_fixed_placement()
                        .then(|| emitter.clone());
                };
                let mut emitter = emitter.clone();
                emitter
                    .motion
                    .get_or_insert_with(ParticleMotion::default)
                    .placement = Some(rows);
                Some(emitter)
            })
            .collect();
        self.properties
            .insert("particle-emitters", Value::SceneParticleEmitters(emitters));
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
    cache: &SceneProjectionCache,
    resolve_asset: impl Fn(&str) -> Option<MeshAsset>,
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
        let mut emission_track = match row.field("emission-track") {
            Some(Value::RecordList(keys)) => cache.record_emission_track(keys)?,
            None => ParticleEmissionTrack::default(),
            _ => return None,
        };
        if emission_track.is_empty()
            && let Some(Value::Record(animation)) = row.field("emission-animation")
        {
            let asset = match animation.field("mesh-asset") {
                Some(Value::MeshAsset3D(asset)) => asset.asset().cloned(),
                _ => None,
            }
            .or_else(|| match animation.field("mesh") {
                Some(Value::String(key)) => resolve_asset(key),
                _ => None,
            })?;
            emission_track = cache.particle_animation_track(
                &asset,
                ParticleEmissionAnimation::from_record(animation)?,
            )?;
        }
        ParticleEmitterDefinition {
            parent: name("parent"),
            parent_node: name("parent-node"),
            particle_group: name("particle-group"),
            emission_track,
            emission_loop: row.field("emission-loop") == Some(&Value::Bool(true)),
            position: *position,
            color: *color,
            size: float("size")?,
            lifetime: float("lifetime")?,
            lifetime_variation: float("lifetime-variation").unwrap_or_default(),
            size_variation: float("size-variation").unwrap_or_default(),
            end_size: float("end-size"),
            end_size_variation: float("end-size-variation"),
            start_time: float("start-time").unwrap_or(defaults.start_time),
            rate: float("rate").unwrap_or(defaults.rate),
            bursts: match row.field("bursts") {
                Some(Value::RecordList(events)) => cache.record_particle_bursts(events)?,
                None => ParticleBurstSchedule::default(),
                _ => return None,
            },
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
            particle_angular_velocity_variation: vector("particle-angular-velocity-variation"),
            particle_casts_shadow: row.field("particle-casts-shadow") == Some(&Value::Bool(true)),
            particle_receives_shadow: row.field("particle-receives-shadow")
                != Some(&Value::Bool(false)),
            particle_casts_reflections: row.field("particle-casts-reflections")
                == Some(&Value::Bool(true)),
        }
        .prepare(&resolve_surface, &resolve_model)
    })
}
