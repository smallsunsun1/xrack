//! Shared emitter projection and time sampling for GPU sprite particles.

pub(crate) mod affectors;
pub(crate) mod bursts;
pub(crate) mod model_blend;
pub(crate) mod motion;
pub(crate) mod points;
pub(crate) mod surface;
pub(crate) mod track;

use std::sync::Arc;

use self::surface::ParticleSceneShape;

use crate::scene3d::lighting::{SceneLightModel, record_lights};
use crate::scene3d::particles::bursts::{BirthSpan, ParticleBurstSchedule, retain_newest};
use crate::scene3d::particles::motion::ParticleMotion;
use crate::scene3d::particles::track::{
    MAX_EMISSION_TIME, ParticleEmissionAnimation, ParticleEmissionTrack, ParticleLoopClock,
};
use crate::scene3d::projection::SceneProjectionCache;
use crate::{Color, Image, ImageAsset, ImageFormat, Node, RecordList, Value, VecModel, Vector3D};

use super::{MAIN_LAYER, Mesh3D, MeshAsset, ParticleMeshSurface, Point3D};

pub const MAX_PARTICLE_EMITTERS: usize = 16;
pub const MAX_PARTICLES: u32 = 8192;
pub const MAX_PARTICLE_TEXTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PER_EMITTER: u32 = 4096;
const MAX_FOLLOW_SOURCES: u32 = 256;

#[doc(hidden)]
pub struct ParticleEmitterDefinition {
    pub parent: String,
    pub parent_node: String,
    pub emitter_id: String,
    pub follow_emitter: String,
    pub follow_burst_at: String,
    pub particle_group: String,
    pub emission_track: ParticleEmissionTrack,
    pub emission_loop: bool,
    pub position: Point3D,
    pub color: Color,
    pub color_variation: [f64; 4],
    pub unified_color_variation: bool,
    pub lifetime: f64,
    pub reversed: bool,
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
    pub fade_in_effect: String,
    pub fade_out_effect: String,
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
    pub sprite_billboard: bool,
    pub sprite_offset_x: f64,
    pub sprite_offset_y: f64,
    pub color_table: Image,
    pub particle_blend_mode: String,
    pub particle_sort_mode: String,
    pub depth_bias: f64,
    pub depth_fade_distance: f64,
    pub emitter_shape: String,
    pub emitter_mesh: String,
    pub emitter_point_source: String,
    pub emitter_point_randomize: bool,
    pub emitter_point_scale: Vector3D,
    pub emitter_scene_root: String,
    pub emitter_scene_exclude: VecModel<String>,
    pub emitter_scene_center: Point3D,
    pub emitter_scene_extents: Vector3D,
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
    pub velocity_target_normalized: bool,
    pub velocity_target_magnitude: Option<f64>,
    pub velocity_speed_variation: f64,
    pub trail_segments: i64,
    pub trail_duration: f64,
    pub trail_end_scale: f64,
    pub trail_fade: f64,
    pub trail_eol_fade: f64,
    pub trail_length: f64,
    pub trail_length_variation: f64,
    pub trail_length_delta_min: f64,
    pub trail_scale_multiplier: f64,
    pub trail_alpha_fade: f64,
    pub trail_texcoord_mode: String,
    pub trail_texcoord_multiplier: f64,
    pub particle_instances_only: bool,
    pub particle_model: String,
    pub particle_model_scene: Option<Arc<super::SceneModel>>,
    pub particle_rotation: Vector3D,
    pub particle_align_mode: String,
    pub particle_align_target: Point3D,
    pub particle_rotation_variation: Vector3D,
    pub particle_angular_velocity: Vector3D,
    pub particle_angular_velocity_variation: Vector3D,
    pub particle_casts_shadow: bool,
    pub particle_receives_shadow: bool,
    pub particle_casts_reflections: bool,
    pub particle_lights: Option<Arc<SceneLightModel>>,
}

impl Default for ParticleEmitterDefinition {
    fn default() -> Self {
        Self {
            parent: String::new(),
            parent_node: String::new(),
            emitter_id: String::new(),
            follow_emitter: String::new(),
            follow_burst_at: String::new(),
            particle_group: String::new(),
            emission_track: ParticleEmissionTrack::default(),
            emission_loop: false,
            position: Point3D::default(),
            color: Color(0xffffffff),
            color_variation: [0.0; 4],
            unified_color_variation: false,
            lifetime: 1.0,
            lifetime_variation: 0.0,
            reversed: false,
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
            fade_in_effect: String::new(),
            fade_out_effect: String::new(),
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
            sprite_billboard: true,
            sprite_offset_x: 0.0,
            sprite_offset_y: 0.0,
            color_table: Image::default(),
            particle_blend_mode: String::new(),
            particle_sort_mode: String::new(),
            depth_bias: 0.0,
            depth_fade_distance: 0.0,
            emitter_shape: String::new(),
            emitter_mesh: String::new(),
            emitter_point_source: String::new(),
            emitter_point_randomize: false,
            emitter_point_scale: Vector3D::new(1.0, 1.0, 1.0),
            emitter_scene_root: String::new(),
            emitter_scene_exclude: VecModel::default(),
            emitter_scene_center: Point3D::default(),
            emitter_scene_extents: Vector3D::default(),
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
            velocity_target_normalized: true,
            velocity_target_magnitude: None,
            velocity_speed_variation: 0.0,
            trail_segments: 0,
            trail_duration: 0.25,
            trail_end_scale: 0.12,
            trail_fade: 1.0,
            trail_eol_fade: 0.0,
            trail_length: -1.0,
            trail_length_variation: 0.0,
            trail_length_delta_min: 0.0,
            trail_scale_multiplier: 1.0,
            trail_alpha_fade: 0.0,
            trail_texcoord_mode: String::new(),
            trail_texcoord_multiplier: 1.0,
            particle_instances_only: false,
            particle_model: String::new(),
            particle_model_scene: None,
            particle_rotation: Vector3D::default(),
            particle_align_mode: String::new(),
            particle_align_target: Point3D::default(),
            particle_rotation_variation: Vector3D::default(),
            particle_angular_velocity: Vector3D::default(),
            particle_angular_velocity_variation: Vector3D::default(),
            particle_casts_shadow: false,
            particle_receives_shadow: true,
            particle_casts_reflections: false,
            particle_lights: None,
        }
    }
}

/// Numeric values are shared by the particle shaders.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ParticleFadeEffect {
    Opacity = 0,
    Scale = 1,
    None = 2,
}

impl ParticleFadeEffect {
    fn from_name(name: &str) -> Self {
        match name {
            "scale" => Self::Scale,
            "none" => Self::None,
            _ => Self::Opacity,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleEmitter {
    pub position: [f32; 3],
    pub emitter_id: Arc<str>,
    pub follow_emitter: Arc<str>,
    pub burst_on_death: bool,
    pub velocity: [f32; 3],
    pub velocity_spread: [f32; 3],
    pub gravity: [f32; 3],
    pub color: Color,
    pub color_variation: [f32; 4],
    pub unified_color_variation: bool,
    pub intensity: f32,
    pub size: [f32; 2],
    pub size_variation: f32,
    pub end_size_variation: Option<f32>,
    pub fade: [f32; 2],
    pub fade_effects: [ParticleFadeEffect; 2],
    pub lifetime: f64,
    pub reversed: bool,
    pub lifetime_variation: f32,
    /// Rotation, rotation spread, angular velocity and angular velocity spread.
    pub rotation: [[f32; 3]; 4],
    /// World target (xyz) and alignment mode (w).
    pub alignment: [f32; 4],
    pub sprite_billboard: bool,
    pub sprite_offset: [f32; 2],
    pub casts_reflections: bool,
    pub lights: Option<Arc<SceneLightModel>>,
    pub seed: u32,
    pub layers: u64,
    pub sprite: Option<ParticleSprite>,
    pub color_table: Option<Image>,
    pub blend: ParticleBlend,
    pub sort: ParticleSortMode,
    pub depth_bias: f32,
    pub depth_fade_distance: f32,
    pub motion: Option<ParticleMotion>,
    pub emission_track: ParticleEmissionTrack,
    emission_loop: bool,
    pub mesh: Option<Arc<ParticleMeshSurface>>,
    scene_shape: Option<Arc<ParticleSceneShape>>,
    pub trail: Option<ParticleTrail>,
    pub model: Option<ParticleModel>,
    pub instances_only: bool,
    model_shadows: [bool; 2],
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

    pub(super) fn rows(&self, scene: &super::SceneModel) -> Option<[[f32; 4]; 3]> {
        scene.particle_parent_rows(&self.object, &self.node)
    }
}

#[doc(hidden)]
pub enum ParticleModelSource<'a> {
    Asset(&'a str),
    Scene(&'a Arc<super::SceneModel>),
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleModel {
    pub meshes: Arc<[Mesh3D]>,
    pub casts_shadow: bool,
    pub receives_shadow: bool,
}

/// A scene model attached to the instance stream of a named emitter in a view.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleInstanceModel {
    pub(super) emitter: String,
    pub(super) meshes: Arc<[Mesh3D]>,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleTrail {
    pub segments: u32,
    pub duration: f32,
    pub end_scale: f32,
    pub fade: f32,
    pub eol_fade: f32,
    pub length: f32,
    pub length_variation: f32,
    pub length_delta_min: f32,
    pub scale_multiplier: f32,
    pub alpha_retention: f32,
    pub texcoord_mode: TrailTexcoordMode,
    pub texcoord_multiplier: f32,
}

/// Values are shared with the ribbon geometry shader.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TrailTexcoordMode {
    Stretch = 0,
    Fill = 1,
    Relative = 2,
    Absolute = 3,
}

impl TrailTexcoordMode {
    fn from_name(name: &str) -> Self {
        match name {
            "fill" => Self::Fill,
            "relative" => Self::Relative,
            "absolute" => Self::Absolute,
            _ => Self::Stretch,
        }
    }
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
pub enum ParticleSortMode {
    #[default]
    None,
    Newest,
    Oldest,
    Distance,
}

impl ParticleSortMode {
    fn from_name(name: &str) -> Self {
        match name {
            "newest" => Self::Newest,
            "oldest" => Self::Oldest,
            "distance" => Self::Distance,
            _ => Self::None,
        }
    }
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
            length: if self.trail_length.is_finite() && self.trail_length >= 0.0 {
                self.trail_length.min(1e8) as f32
            } else {
                -1.0
            },
            length_variation: if self.trail_length_variation.is_finite() {
                self.trail_length_variation.clamp(0.0, 1e8) as f32
            } else {
                0.0
            },
            length_delta_min: if self.trail_length_delta_min.is_finite() {
                self.trail_length_delta_min.clamp(0.0, 1e8) as f32
            } else {
                0.0
            },
            scale_multiplier: if self.trail_scale_multiplier.is_finite() {
                self.trail_scale_multiplier.clamp(0.0, 2.0) as f32
            } else {
                1.0
            },
            alpha_retention: if self.trail_alpha_fade.is_finite() {
                (1.0 - self.trail_alpha_fade.clamp(0.0, 1.0)) as f32
            } else {
                1.0
            },
            texcoord_mode: TrailTexcoordMode::from_name(&self.trail_texcoord_mode),
            texcoord_multiplier: if self.trail_texcoord_multiplier.is_finite() {
                self.trail_texcoord_multiplier.clamp(-1e6, 1e6) as f32
            } else {
                1.0
            },
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
        resolve_points: impl FnOnce(&str, [f32; 3], Option<u32>) -> Option<Arc<ParticleMeshSurface>>,
        resolve_model: impl FnOnce(ParticleModelSource<'_>) -> Option<Arc<[Mesh3D]>>,
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
        let color_variation = self.color_variation.map(|channel| {
            if channel.is_finite() {
                channel.clamp(0.0, 1.0) as f32
            } else {
                0.0
            }
        });
        if !self.enabled
            || self.layers == 0
            || (self.color.0 & 0xff == 0 && color_variation[3] == 0.0)
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
        } else if self.emitter_shape == "points" {
            Some(resolve_points(
                &self.emitter_point_source,
                [
                    self.emitter_point_scale.x,
                    self.emitter_point_scale.y,
                    self.emitter_point_scale.z,
                ]
                .map(|value| value as f32),
                self.emitter_point_randomize.then_some(self.seed as u32),
            )?)
        } else {
            None
        };
        let scene_shape = if self.emitter_shape == "scene" {
            Some(ParticleSceneShape::new(
                &self.emitter_scene_root,
                self.emitter_scene_exclude.clone(),
                self.emitter_scene_center,
                self.emitter_scene_extents,
                self.emitter_mesh_scale,
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
        let alignment = match self.particle_align_mode.as_str() {
            "start-velocity" => [0.0, 0.0, 0.0, 1.0],
            "velocity" => [0.0, 0.0, 0.0, 3.0],
            "target" => {
                let target = self.particle_align_target;
                let xyz = [target.x, target.y, target.z];
                if !xyz.into_iter().all(|axis| axis.abs() <= 1.0e8) {
                    return None;
                }
                [target.x as f32, target.y as f32, target.z as f32, 2.0]
            }
            _ => [0.0; 4],
        };
        let source = match (&self.particle_model_scene, self.particle_model.as_str()) {
            (Some(scene), _) if !scene.objects().is_empty() => {
                Some(ParticleModelSource::Scene(scene))
            }
            (_, asset) if !asset.is_empty() => Some(ParticleModelSource::Asset(asset)),
            (Some(_), _) => return None,
            (None, _) => None,
        };
        let model = match source {
            Some(source) => Some(ParticleModel {
                meshes: resolve_model(source)?,
                casts_shadow: self.particle_casts_shadow,
                receives_shadow: self.particle_receives_shadow,
            }),
            None => None,
        };
        let mut motion = self
            .prepare_motion(velocity)
            .or_else(|| tracked.then(ParticleMotion::default));
        if let Some(points) = mesh.as_ref().and_then(|surface| surface.point_uniform()) {
            motion.as_mut()?.shape.weights = points;
        }
        Some(ParticleEmitter {
            position: position.map(|value| value as f32),
            emitter_id: self.emitter_id.as_str().into(),
            follow_emitter: self.follow_emitter.as_str().into(),
            burst_on_death: self.follow_burst_at == "death",
            velocity: velocity.map(|value| value as f32),
            velocity_spread: spread.map(|value| value.abs() as f32),
            gravity: gravity.map(|value| value as f32),
            color: self.color,
            color_variation,
            unified_color_variation: self.unified_color_variation,
            intensity: self.intensity.min(64.0) as f32,
            size: [self.size as f32, end_size as f32],
            size_variation: self.size_variation as f32,
            end_size_variation: self.end_size_variation.map(|value| value as f32),
            fade: [self.fade_in as f32, self.fade_out as f32],
            fade_effects: [
                ParticleFadeEffect::from_name(&self.fade_in_effect),
                ParticleFadeEffect::from_name(&self.fade_out_effect),
            ],
            lifetime: self.lifetime,
            reversed: self.reversed,
            lifetime_variation: self.lifetime_variation as f32,
            rotation: [
                rotation,
                rotation_variation,
                angular_velocity,
                angular_velocity_variation,
            ],
            alignment,
            sprite_billboard: self.sprite_billboard,
            sprite_offset: [self.sprite_offset_x, self.sprite_offset_y].map(|offset| {
                if offset.is_finite() {
                    offset.clamp(-1e4, 1e4) as f32
                } else {
                    0.0
                }
            }),
            casts_reflections: self.particle_casts_reflections,
            seed: self.seed as u32,
            layers: self.layers as u64,
            sprite: self.prepare_sprite(),
            motion,
            trail: if model.is_none() {
                self.prepare_trail()
            } else {
                None
            },
            lights: self.particle_lights.filter(|_| model.is_none()),
            model,
            instances_only: self.particle_instances_only,
            model_shadows: [self.particle_casts_shadow, self.particle_receives_shadow],
            mesh,
            scene_shape,
            color_table: particle_png(&self.color_table).map(|_| self.color_table),
            parent: (!tracked)
                .then(|| ParticleParent::new(self.parent, self.parent_node))
                .flatten(),
            group: (!self.particle_group.is_empty()).then(|| self.particle_group.into()),
            emission_track: self.emission_track,
            emission_loop: self.emission_loop,
            blend: ParticleBlend::from_name(&self.particle_blend_mode),
            sort: ParticleSortMode::from_name(&self.particle_sort_mode),
            depth_bias: if self.depth_bias.is_finite() {
                self.depth_bias.clamp(-1e8, 1e8) as f32
            } else {
                0.0
            },
            depth_fade_distance: if self.depth_fade_distance.is_finite() {
                self.depth_fade_distance.clamp(0.0, 1e8) as f32
            } else {
                0.0
            },
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
    pub continuous: bool,
    /// Interval and elapsed times at the current and previous stream ID wraps.
    pub birth_clock: [f32; 3],
    pub loop_clock: Option<ParticleLoopClock>,
    pub birth_frame: Option<[[f32; 4]; 3]>,
    pub follow_offset: Option<u32>,
}

#[doc(hidden)]
pub struct ParticleFollowPlan {
    pub groups: Vec<ParticleFollowGroup>,
    pub count: u32,
}

#[doc(hidden)]
pub struct ParticleFollowGroup {
    pub source: ParticleSample,
    pub jobs: Vec<ParticleFollowJob>,
}

#[doc(hidden)]
pub struct ParticleFollowJob {
    pub source_index: u32,
    pub child: ParticleSample,
    /// Age of the source when the youngest child in this span was born.
    pub source_age: f32,
    pub source_step: f32,
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
    pub fn follows_surface(&self) -> bool {
        !self.follow_emitter.is_empty()
            && self
                .motion
                .as_ref()
                .is_some_and(ParticleMotion::uses_surface_velocity)
    }

    pub fn follow_source<'a>(&self, emitters: &'a [Self]) -> Option<&'a Self> {
        if self.follow_emitter.is_empty() {
            return None;
        }
        let mut sources = emitters
            .iter()
            .filter(|source| source.emitter_id == self.follow_emitter);
        let source = sources.next()?;
        (source.follow_emitter.is_empty() && sources.next().is_none()).then_some(source)
    }

    /// Schedule children on the parent's local timeline. Rebuilding this plan
    /// from the clock also supports seeking without saving particle state.
    pub fn plan_follow(&self, source: &Self, time: f64, remaining: u32) -> ParticleFollowPlan {
        let mut plan = ParticleFollowPlan {
            groups: Vec::new(),
            count: 0,
        };
        let budget = remaining.min(MAX_PER_EMITTER);
        if budget == 0 {
            return plan;
        }
        let lifetime = self.render_lifetime();
        let bursts = self.bursts.events();
        let last_burst = bursts
            .iter()
            .map(|burst| burst.time + burst.duration)
            .fold(0.0, f64::max);
        // Only consider sources old enough to have emitted at least one child.
        let first_burst = bursts.first().map_or(f64::INFINITY, |burst| burst.time)
            + if self.burst_on_death {
                source.shortest_lifetime_seconds()
            } else {
                0.0
            };
        let earliest = self.rate.recip().min(first_burst);
        let delay = self.start_time.max(0.0);
        let offset = delay + earliest;
        let history = (source.longest_lifetime() + last_burst + lifetime - earliest).next_up();
        let mut spans = Vec::new();
        let mut jobs = Vec::new();
        for source_span in source.spans_in_window(time - offset, MAX_FOLLOW_SOURCES, history) {
            let age_at_index = move |reverse| {
                f64::from(reverse).mul_add(source_span.interval, source_span.youngest_age) + offset
            };
            let sample = source.sample_span(source_span);
            let group = plan.groups.len();
            plan.groups.push(ParticleFollowGroup {
                source: sample,
                jobs: Vec::new(),
            });
            for index in 0..sample.count {
                let reverse = sample.count - 1 - index;
                let age = age_at_index(reverse);
                let tick = sample.latest_id.wrapping_sub(reverse);
                let seed = source.seed ^ sample.seed_salt ^ tick.wrapping_mul(0xa24baed5);
                let death = source.lifetime_for_seed(seed);
                let mut add = |mut span: BirthSpan, age_at_birth, follows_path| {
                    span.seed_salt ^= seed;
                    let step = if follows_path { span.interval } else { 0.0 };
                    spans.push(span);
                    jobs.push((group, index, age_at_birth, step));
                };
                let elapsed = age - delay;
                if self.rate > 0.0 {
                    let stop = elapsed.min(death - delay);
                    let lag = elapsed - stop;
                    if let Some(mut span) =
                        BirthSpan::continuous(stop, self.rate, lifetime - lag, budget)
                    {
                        span.youngest_age += lag;
                        let age_at_birth = (age - span.youngest_age).clamp(0.0, death);
                        add(span, age_at_birth, true);
                    }
                }
                let event_age = if self.burst_on_death { death } else { 0.0 };
                for burst in bursts
                    .iter()
                    .take_while(|burst| burst.time <= elapsed - event_age)
                {
                    if let Some(span) =
                        burst.sample(elapsed - event_age, self.seed ^ seed, lifetime, budget)
                    {
                        add(span, event_age, false);
                    }
                }
            }
        }
        retain_newest(&mut [], &mut spans, budget);
        for (span, (group, source_index, age, step)) in spans.into_iter().zip(jobs) {
            if span.count == 0 {
                continue;
            }
            let mut child = self.sample_span(span);
            child.follow_offset = Some(plan.count);
            plan.count += child.count;
            plan.groups[group].jobs.push(ParticleFollowJob {
                source_index,
                child,
                source_age: age as f32,
                source_step: step as f32,
            });
        }
        plan.groups.retain(|group| !group.jobs.is_empty());
        plan
    }

    fn lifetime_for_seed(&self, seed: u32) -> f64 {
        if self.lifetime_variation == 0.0 {
            return self.lifetime;
        }
        // Match signed_random and the lifetime stream in particles.wgsl.
        let input = seed ^ 0xb5297a4d;
        let bits = (input ^ (input >> 16)).wrapping_mul(0x7feb352d);
        let bits = (bits ^ (bits >> 15)).wrapping_mul(0x846ca68b);
        let random = ((bits ^ (bits >> 16)) >> 8) as f32 / 16_777_216.0;
        f64::from(
            (2.0 * random - 1.0)
                .mul_add(self.lifetime_variation, self.lifetime as f32)
                .clamp(1e-6, 1e8),
        )
    }

    fn shortest_lifetime_seconds(&self) -> f64 {
        if self.lifetime_variation == 0.0 {
            self.lifetime
        } else {
            f64::from(self.shortest_lifetime().next_down())
        }
    }

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

    fn trajectory_bounds(
        &self,
        sample: &ParticleSample,
        [youngest, oldest]: [f64; 2],
        follow_origin: Option<[[f64; 3]; 2]>,
    ) -> [[f64; 3]; 2] {
        let target = self
            .motion
            .as_ref()
            .filter(|motion| motion.uses_target_velocity());
        // Target velocities are in world space. Include age zero so the
        // base bounds also contain every possible particle birth position.
        let youngest = if target.is_some() { 0.0 } else { youngest };
        let spawn = self.motion.as_ref().map_or([0.0; 3], |motion| {
            motion.spawn_extent(self.mesh.as_ref().map(|mesh| mesh.radius))
        });
        let frame = sample
            .birth_frame
            .or_else(|| self.motion.as_ref().and_then(|motion| motion.placement));
        let speed = self
            .motion
            .as_ref()
            .and_then(|motion| motion.max_directed_speed(self.velocity, self.velocity_spread));
        // Surface velocities can turn after the affine transform. A sphere
        // in world space covers all outgoing directions and scaled speeds.
        let surface_world_travel = frame
            .filter(|_| {
                self.motion
                    .as_ref()
                    .is_some_and(ParticleMotion::uses_surface_velocity)
            })
            .zip(speed)
            .map(|(rows, speed)| {
                let matrix_norm = rows
                    .iter()
                    .flat_map(|row| &row[..3])
                    .map(|&component| f64::from(component).powi(2))
                    .sum::<f64>()
                    .sqrt();
                oldest * matrix_norm * speed
            });
        let position = if sample.birth_frame.is_some() {
            [0.0; 3]
        } else {
            self.position.map(f64::from)
        };
        let axes: [[f64; 2]; 3] = std::array::from_fn(|axis| {
            let velocity = f64::from(self.velocity[axis]);
            let spread = f64::from(self.velocity_spread[axis]);
            let [slow, fast] = if surface_world_travel.is_some() {
                [0.0; 2]
            } else {
                speed.map_or([velocity - spread, velocity + spread], |v| [-v, v])
            };
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
        if let Some(travel) = surface_world_travel {
            bounds[0] = bounds[0].map(|value| value - travel);
            bounds[1] = bounds[1].map(|value| value + travel);
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
        if let Some(origin) = follow_origin {
            for (side, offsets) in bounds.iter_mut().zip(origin) {
                for (value, offset) in side.iter_mut().zip(offsets) {
                    *value += offset;
                }
            }
        }
        if let Some(motion) = target {
            let travel = motion.target_travel(&bounds, oldest);
            for (axis, distance) in travel.into_iter().enumerate() {
                bounds[0][axis] -= distance;
                bounds[1][axis] += distance;
            }
        }
        bounds
    }

    pub fn follow_origin_bounds<'a>(
        &self,
        plan: &ParticleFollowPlan,
        affectors: impl Iterator<Item = &'a crate::scene3d::particles::affectors::ParticleAffector>
        + Clone,
    ) -> Option<[[f64; 3]; 2]> {
        plan.groups
            .iter()
            .map(|group| {
                let oldest = group
                    .jobs
                    .iter()
                    .map(|job| f64::from(job.source_age.next_up()))
                    .fold(0.0, f64::max);
                let ages = self.effect_age_range([0.0, oldest]);
                let mut bounds = self.trajectory_bounds(&group.source, ages, None);
                for affector in affectors.clone() {
                    affector.expand_particle_bounds(&mut bounds, ages[1]);
                }
                bounds
            })
            .reduce(|a, b| {
                [
                    std::array::from_fn(|axis| a[0][axis].min(b[0][axis])),
                    std::array::from_fn(|axis| a[1][axis].max(b[1][axis])),
                ]
            })
    }

    /// Conservatively fit one emitter without reading instance positions back from the GPU.
    pub fn shadow_bounds<'a>(
        &self,
        sample: &ParticleSample,
        origin: Option<[[f64; 3]; 2]>,
        affectors: impl Iterator<Item = &'a crate::scene3d::particles::affectors::ParticleAffector>,
    ) -> Option<[[f32; 3]; 2]> {
        let model = self.model.as_ref()?;
        if !model.casts_shadow || !model.meshes.iter().any(Mesh3D::has_opaque_parts) {
            return None;
        }
        let youngest = f64::from(sample.youngest_age.next_down()).max(0.0);
        let oldest = f64::from(sample.oldest_age().next_up());
        let ages = self.effect_age_range([youngest, oldest]);
        let mut bounds = self.trajectory_bounds(sample, ages, origin);
        let mut scale_factor = 1.0;
        for affector in affectors {
            affector.expand_particle_bounds(&mut bounds, ages[1]);
            scale_factor *= affector.max_scale(ages[1]);
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

    fn effect_age_range(&self, [youngest, oldest]: [f64; 2]) -> [f64; 2] {
        if !self.reversed {
            return [youngest, oldest];
        }
        let shortest = f64::from(self.shortest_lifetime().next_down());
        let longest = f64::from((self.longest_lifetime() as f32).next_up());
        [(shortest - oldest).max(0.0), (longest - youngest).max(0.0)]
    }

    /// Maximum sampled curve age, including historical ribbon points.
    pub fn maximum_effect_age(&self, sample: &ParticleSample) -> f32 {
        if !self.reversed {
            // The shader may fuse the multiply-add in the stream age.
            return sample.oldest_age().next_up();
        }
        let mut youngest = sample.youngest_age.next_down().max(0.0);
        if let Some(trail) = self.trail {
            // During the end fade, the ribbon head is frozen at the death time.
            youngest = (youngest.min(self.shortest_lifetime()) - trail.duration).max(0.0);
        }
        (self.longest_lifetime() as f32 - youngest)
            .max(0.0)
            .next_up()
            .next_up()
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
        self.spans_in_window(time, remaining.min(MAX_PER_EMITTER), self.render_lifetime())
            .map(|span| self.sample_span(span))
    }

    fn render_lifetime(&self) -> f64 {
        self.longest_lifetime() + self.trail.map_or(0.0, |trail| f64::from(trail.eol_fade))
    }

    fn spans_in_window(
        &self,
        time: f64,
        budget: u32,
        lifetime: f64,
    ) -> impl Iterator<Item = BirthSpan> {
        let elapsed = time - self.start_time;
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
    }

    fn sample_span(&self, span: BirthSpan) -> ParticleSample {
        let mut sample = ParticleSample {
            count: span.count,
            interval: span.interval.min(2.0 * MAX_EMISSION_TIME) as f32,
            youngest_age: span.youngest_age as f32,
            latest_id: span.latest as u32,
            continuous: span.origin.is_none(),
            seed_salt: span.seed_salt,
            birth_clock: [0.0; 3],
            loop_clock: None,
            birth_frame: span.frame,
            follow_offset: None,
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

impl ParticleEmitter {
    fn attach_instance_models(&mut self, mesh_groups: impl Iterator<Item = Arc<[Mesh3D]>>) {
        let mut groups = self
            .model
            .iter()
            .map(|model| model.meshes.clone())
            .chain(mesh_groups);
        let Some(first) = groups.next() else {
            return;
        };
        let count_vertices = |meshes: &[Mesh3D]| {
            meshes
                .iter()
                .map(|mesh| mesh.vertices().len())
                .sum::<usize>()
        };
        let mut vertices = count_vertices(&first);
        let mut combined = None;
        for group in groups {
            let count = count_vertices(&group);
            if vertices + count <= super::MAX_IMPORTED_VERTICES {
                combined
                    .get_or_insert_with(|| first.to_vec())
                    .extend_from_slice(&group);
                vertices += count;
            }
        }
        let meshes = combined.map_or(first, Arc::from);
        let [casts_shadow, receives_shadow] = self.model_shadows;
        self.model = Some(ParticleModel {
            meshes,
            casts_shadow,
            receives_shadow,
        });
        self.trail = None;
        self.lights = None;
    }
}

impl Node {
    pub(crate) fn prepare_particle_attachments(&mut self) {
        let instance_models = match self.properties.remove("particle-instance-models") {
            Some(Value::SceneParticleInstanceModels(models)) => models,
            _ => Arc::from([]),
        };
        let Some(Value::SceneParticleEmitters(emitters)) = self.properties.get("particle-emitters")
        else {
            return;
        };
        if instance_models.is_empty()
            && emitters
                .iter()
                .all(|emitter| emitter.parent.is_none() && emitter.scene_shape.is_none())
        {
            return;
        }
        let scene = match self.properties.get("model") {
            Some(Value::SceneObjects(scene)) => Some(scene.as_ref()),
            _ => None,
        };
        let emitters = emitters
            .iter()
            .filter_map(|emitter| {
                let mut emitter = emitter.clone();
                if !emitter.emitter_id.is_empty()
                    && instance_models
                        .iter()
                        .any(|model| model.emitter.as_str() == emitter.emitter_id.as_ref())
                    && emitters
                        .iter()
                        .filter(|source| source.emitter_id == emitter.emitter_id)
                        .count()
                        == 1
                {
                    let id = emitter.emitter_id.clone();
                    emitter.attach_instance_models(
                        instance_models
                            .iter()
                            .filter(|model| model.emitter.as_str() == id.as_ref())
                            .map(|model| model.meshes.clone()),
                    );
                }

                if let Some(shape) = emitter.scene_shape.as_ref() {
                    let projected = scene?.particle_scene_surface(shape)?;
                    emitter.mesh = Some(projected.mesh);
                    if emitter.parent.is_none() && emitter.emission_track.is_empty() {
                        emitter
                            .motion
                            .get_or_insert_with(ParticleMotion::default)
                            .placement = projected.placement;
                    }
                }
                let Some(parent) = &emitter.parent else {
                    return Some(emitter);
                };
                let Some(rows) = scene.and_then(|scene| parent.rows(scene)) else {
                    return emitter.bursts.has_fixed_placement().then_some(emitter);
                };
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
    resolve_points: impl Fn(&str, [f32; 3], Option<u32>) -> Option<Arc<ParticleMeshSurface>>,
    resolve_model: impl Fn(ParticleModelSource<'_>) -> Option<Arc<[Mesh3D]>>,
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
        let rgba = match row.field("color-variation") {
            Some(Value::Record(variation)) => {
                ["red", "green", "blue", "alpha"].map(|channel| match variation.field(channel) {
                    Some(Value::Float(value)) => *value,
                    _ => 0.0,
                })
            }
            _ => [0.0; 4],
        };
        ParticleEmitterDefinition {
            parent: name("parent"),
            parent_node: name("parent-node"),
            emitter_id: name("emitter-id"),
            follow_emitter: name("follow-emitter"),
            follow_burst_at: name("follow-burst-at"),
            particle_group: name("particle-group"),
            emission_track,
            emission_loop: row.field("emission-loop") == Some(&Value::Bool(true)),
            position: *position,
            color: *color,
            color_variation: rgba,
            unified_color_variation: row.field("unified-color-variation")
                == Some(&Value::Bool(true)),
            size: float("size")?,
            lifetime: float("lifetime")?,
            lifetime_variation: float("lifetime-variation").unwrap_or_default(),
            reversed: row.field("reversed") == Some(&Value::Bool(true)),
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
            fade_in_effect: name("fade-in-effect"),
            fade_out_effect: name("fade-out-effect"),
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
            sprite_billboard: row.field("sprite-billboard") != Some(&Value::Bool(false)),
            sprite_offset_x: float("sprite-offset-x").unwrap_or_default(),
            sprite_offset_y: float("sprite-offset-y").unwrap_or_default(),
            color_table: image("color-table"),
            particle_blend_mode: name("particle-blend-mode"),
            particle_sort_mode: name("particle-sort-mode"),
            depth_bias: float("depth-bias").unwrap_or_default(),
            depth_fade_distance: float("depth-fade-distance").unwrap_or_default(),
            emitter_shape: name("emitter-shape"),
            emitter_mesh: name("emitter-mesh"),
            emitter_point_source: name("emitter-point-source"),
            emitter_point_randomize: row.field("emitter-point-randomize")
                == Some(&Value::Bool(true)),
            emitter_point_scale: match row.field("emitter-point-scale") {
                Some(Value::Vector3D(scale)) => *scale,
                _ => defaults.emitter_point_scale,
            },
            emitter_scene_root: name("emitter-scene-root"),
            emitter_scene_exclude: ParticleSceneShape::record_exclusions(
                row.field("emitter-scene-exclude"),
            ),
            emitter_scene_center: match row.field("emitter-scene-center") {
                Some(Value::Point3D(center)) => *center,
                _ => Point3D::default(),
            },
            emitter_scene_extents: match row.field("emitter-scene-extents") {
                Some(Value::Vector3D(extents)) => *extents,
                _ => Vector3D::default(),
            },
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
            velocity_target_normalized: row.field("velocity-target-normalized")
                != Some(&Value::Bool(false)),
            velocity_target_magnitude: float("velocity-target-magnitude"),
            velocity_speed_variation: float("velocity-speed-variation")
                .unwrap_or(defaults.velocity_speed_variation),
            trail_segments: integer("trail-segments").unwrap_or(defaults.trail_segments),
            trail_duration: float("trail-duration").unwrap_or(defaults.trail_duration),
            trail_end_scale: float("trail-end-scale").unwrap_or(defaults.trail_end_scale),
            trail_fade: float("trail-fade").unwrap_or(defaults.trail_fade),
            trail_eol_fade: float("trail-eol-fade").unwrap_or(defaults.trail_eol_fade),
            trail_length: float("trail-length").unwrap_or(defaults.trail_length),
            trail_length_variation: float("trail-length-variation").unwrap_or_default(),
            trail_length_delta_min: float("trail-length-delta-min").unwrap_or_default(),
            trail_scale_multiplier: float("trail-scale-multiplier").unwrap_or(1.0),
            trail_alpha_fade: float("trail-alpha-fade").unwrap_or_default(),
            trail_texcoord_mode: name("trail-texcoord-mode"),
            trail_texcoord_multiplier: float("trail-texcoord-multiplier").unwrap_or(1.0),
            particle_instances_only: row.field("particle-instances-only")
                == Some(&Value::Bool(true)),
            particle_model: name("particle-model"),
            particle_model_scene: match row.field("particle-model-scene") {
                Some(Value::SceneObjects(scene)) => Some(scene.clone()),
                _ => None,
            },
            particle_rotation: vector("particle-rotation"),
            particle_align_mode: name("particle-align-mode"),
            particle_align_target: match row.field("particle-align-target") {
                Some(Value::Point3D(target)) => *target,
                _ => Point3D::default(),
            },
            particle_rotation_variation: vector("particle-rotation-variation"),
            particle_angular_velocity: vector("particle-angular-velocity"),
            particle_angular_velocity_variation: vector("particle-angular-velocity-variation"),
            particle_casts_shadow: row.field("particle-casts-shadow") == Some(&Value::Bool(true)),
            particle_receives_shadow: row.field("particle-receives-shadow")
                != Some(&Value::Bool(false)),
            particle_casts_reflections: row.field("particle-casts-reflections")
                == Some(&Value::Bool(true)),
            particle_lights: match row.field("particle-lights") {
                Some(Value::RecordList(lights)) if !lights.model().is_empty() => {
                    Some(record_lights(lights))
                }
                _ => None,
            },
        }
        .prepare(&resolve_surface, &resolve_points, &resolve_model)
    })
}
