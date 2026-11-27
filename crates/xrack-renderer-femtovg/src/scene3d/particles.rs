//! Camera-facing sprites evaluated from emitter uniforms and GPU instance indices.
use std::sync::Arc;
use std::{
    collections::{HashMap, VecDeque},
    ops::Deref,
};

use xrack_runtime::scene_effect::{CapturePipelineState, CaptureRenderables};
use xrack_runtime::{
    Node, Value,
    scene3d::{
        CaptureMaterial, MAX_PARTICLE_EMITTERS, MAX_PARTICLES, MAX_SCENE_LIGHTS, Mesh3D,
        ParticleAffector, ParticleBlend, ParticleEmitter, ParticleFadeEffect, ParticleFollowPlan,
        ParticleMotion, ParticleSample, ParticleSortMode, SceneCamera, TrailTexcoordMode,
    },
};

use super::{
    append_floats, buffer,
    lighting::{LightBuffers, LightSet},
    linear_rgb,
    linked_transparency::{PARTICLE_LIGHT_BINDING, PARTICLE_TRAIL_BINDING},
    particle_affectors::{AffectorBindings, AffectorBuffers},
    particle_follow::{CHILDREN_PER_JOB, FOLLOW_BINDING, FollowGenerator, FollowInputs, JOB_BYTES},
    particle_instances::{ModelInputs, ModelInstanceGenerator},
    particle_meshes::{MeshSurfaceBuffers, SurfaceBinding},
    particle_sort::{ParticleSorter, SORT_BINDING, SortInputs},
    particle_textures::{SpriteBinding, SpriteTextures},
    particle_tracks::{EmissionTrackBuffers, TRACK_BINDING, particles_shader},
    particle_trails::{TrailGenerator, TrailInputs},
    pipeline::ColorOutput,
};

const VIEW_BYTES: usize = 128;
const VIEW_CACHE_CAPACITY: usize = 32;
// Every emitter occupies fourteen aligned vec4 slots in the shader.
const EMITTER_BYTES: usize = 224;
const MOTION_BYTES: usize = 208;
const TRAIL_BYTES: usize = 48;
const POSE_BYTES: usize = 80;
pub(super) const POSE_BINDING: u32 = 9;
const SCENE_DEPTH_BINDING: u32 = 12;
const DEPTH_PROJECTION_BINDING: u32 = 13;
pub(super) const VIEW_BINDING: u32 = 14;

pub(super) struct ParticleDepth {
    pixels: wgpu::Buffer,
    projection: wgpu::Buffer,
}

impl ParticleDepth {
    pub fn parameters(
        camera: SceneCamera,
        [width, height]: [u32; 2],
        clear: f32,
    ) -> Option<[[f32; 4]; 3]> {
        let inverse = camera.inverse_clip_rows()?;
        let row = camera.depth_row();
        let linear_depth: [f32; 4] = std::array::from_fn(|column| {
            (0..4)
                .map(|index| f64::from(row[index]) * f64::from(inverse[index][column]))
                .sum::<f64>() as f32
        });
        if !linear_depth.iter().all(|value| value.is_finite()) {
            return None;
        }
        Some([
            linear_depth,
            inverse[3],
            [
                width as f32,
                clear,
                (width as f32).recip(),
                (height as f32).recip(),
            ],
        ])
    }

    pub fn new(device: &wgpu::Device, pixels: wgpu::Buffer, parameters: [[f32; 4]; 3]) -> Self {
        let mut bytes = Vec::with_capacity(size_of_val(&parameters));
        append_floats(&mut bytes, parameters.into_iter().flatten());
        let projection = buffer(
            device,
            "XRack particle depth projection",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        Self { pixels, projection }
    }
}

fn vertex_entry(
    mesh: bool,
    spatial: bool,
    tinted: bool,
    affected: bool,
    trail: bool,
) -> &'static str {
    const ENTRIES: [[&str; 4]; 3] = [
        [
            "vertex_sprite",
            "vertex_tinted",
            "vertex_affected",
            "vertex_affected_tinted",
        ],
        [
            "vertex_spatial",
            "vertex_spatial_tinted",
            "vertex_spatial_affected",
            "vertex_spatial_affected_tinted",
        ],
        [
            "vertex_mesh",
            "vertex_mesh_tinted",
            "vertex_mesh_affected",
            "vertex_mesh_affected_tinted",
        ],
    ];
    const TRAILS: [[&str; 4]; 3] = [
        [
            "vertex_trail",
            "vertex_trail_tinted",
            "vertex_trail_affected",
            "vertex_trail_affected_tinted",
        ],
        [
            "vertex_spatial_trail",
            "vertex_spatial_trail_tinted",
            "vertex_spatial_trail_affected",
            "vertex_spatial_trail_affected_tinted",
        ],
        [
            "vertex_mesh_trail",
            "vertex_mesh_trail_tinted",
            "vertex_mesh_trail_affected",
            "vertex_mesh_trail_affected_tinted",
        ],
    ];
    let geometry = if mesh { 2 } else { usize::from(spatial) };
    let entries = if trail { &TRAILS } else { &ENTRIES };
    entries[geometry][2 * usize::from(affected) + usize::from(tinted)]
}

pub(super) struct ParticleDraw<'a> {
    emitters: Vec<(DrawEmitter<'a>, ParticleSample, Option<usize>)>,
    followers: Vec<(&'a ParticleEmitter, ParticleFollowPlan, bool)>,
    affectors: &'a [ParticleAffector],
    node: &'a Node,
}

enum DrawEmitter<'a> {
    Source(&'a ParticleEmitter),
    Override(Box<ParticleEmitter>),
}

impl Deref for DrawEmitter<'_> {
    type Target = ParticleEmitter;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Source(emitter) => emitter,
            Self::Override(emitter) => emitter,
        }
    }
}

impl<'a> ParticleDraw<'a> {
    pub fn sample(node: &'a Node, camera: &SceneCamera) -> Option<Self> {
        Self::sample_filtered(node, camera, CaptureRenderables::All, true)
    }

    fn sample_filtered(
        node: &'a Node,
        camera: &SceneCamera,
        renderables: CaptureRenderables,
        reflection: bool,
    ) -> Option<Self> {
        if renderables == CaptureRenderables::None {
            return None;
        }
        let Some(Value::SceneParticleEmitters(emitters)) = node.properties.get("particle-emitters")
        else {
            return None;
        };
        let time = node.number("particle-time").unwrap_or(0.0);
        let mut active = Vec::new();
        let mut followers = Vec::new();
        let mut remaining = MAX_PARTICLES;
        for emitter in emitters.iter().take(MAX_PARTICLE_EMITTERS) {
            if emitter.instances_only && emitter.model.is_none()
                || emitter.layers & camera.layer_mask() == 0
                || reflection && !emitter.casts_reflections
            {
                continue;
            }
            let follow = if emitter.follow_emitter.is_empty() {
                None
            } else {
                let Some(source) = emitter.follow_source(emitters) else {
                    continue;
                };
                Some((
                    source,
                    emitter.plan_follow(source, time, remaining),
                    emitter.follows_surface(),
                ))
            };
            let follows = follow.as_ref().map(|_| followers.len());
            let samples = follow
                .as_ref()
                .into_iter()
                .flat_map(|(_, plan, _)| {
                    plan.groups
                        .iter()
                        .flat_map(|group| group.jobs.iter().map(|job| job.child))
                })
                .chain(emitter.samples(time, remaining * u32::from(follows.is_none())));
            let before = remaining;
            for sample in samples {
                if renderables != CaptureRenderables::All
                    && !emitter.model.as_ref().map_or_else(
                        || renderables.includes(true),
                        |model| {
                            let fading = model_fading(emitter, &sample);
                            model.meshes.iter().any(|mesh| {
                                renderables.includes(false)
                                    && !fading
                                    && (mesh.has_opaque_parts()
                                        || mesh.has_depth_writing_transmission())
                                    || renderables.includes(true)
                                        && (fading || !mesh.blended_parts().is_empty())
                            })
                        },
                    )
                {
                    continue;
                }
                remaining -= sample.count;
                active.push((DrawEmitter::Source(emitter), sample, follows));
            }
            if remaining < before
                && let Some(follow) = follow
            {
                followers.push(follow);
            }
            if remaining == 0 {
                break;
            }
        }
        let affectors = match node.properties.get("particle-affectors") {
            Some(Value::SceneParticleAffectors(affectors)) => affectors.as_ref(),
            _ => &[],
        };
        (!active.is_empty()).then_some(Self {
            emitters: active,
            followers,
            affectors,
            node,
        })
    }

    pub fn sample_with_capture_material(
        node: &'a Node,
        camera: &SceneCamera,
        filter: CaptureRenderables,
        material: Option<&Arc<CaptureMaterial>>,
    ) -> Option<Self> {
        let mut draw = Self::sample_filtered(node, camera, filter, false)?;
        if let Some(material) = material {
            draw.emitters.retain_mut(|(emitter, sample, _)| {
                let Some(model) = &emitter.model else {
                    return true;
                };
                let filter = if model_fading(emitter, sample) {
                    if !filter.includes(true) {
                        return false;
                    }
                    CaptureRenderables::All
                } else {
                    filter
                };
                let meshes: Arc<[_]> = model
                    .meshes
                    .iter()
                    .filter_map(|mesh| material.project(mesh, filter))
                    .collect();
                if meshes.is_empty() {
                    return false;
                }
                let mut projected = (**emitter).clone();
                if let Some(model) = &mut projected.model {
                    model.meshes = meshes;
                }
                *emitter = DrawEmitter::Override(Box::new(projected));
                true
            });
        }
        (!draw.emitters.is_empty()).then_some(draw)
    }

    pub fn has_source_over(&self) -> bool {
        self.emitters.iter().any(|(emitter, _, _)| {
            emitter.model.is_none() && emitter.blend == ParticleBlend::SourceOver
        })
    }

    pub fn has_custom_blend(&self) -> bool {
        self.emitters.iter().any(|(emitter, _, _)| {
            emitter.model.is_none() && emitter.blend != ParticleBlend::SourceOver
        })
    }

    pub fn needs_view(&self, weighted: bool) -> bool {
        self.emitters.iter().any(|(emitter, sample, _)| {
            emitter.model.as_ref().is_none_or(|model| {
                !weighted
                    && emitter.sort == ParticleSortMode::Distance
                    && (model_fading(emitter, sample)
                        || model
                            .meshes
                            .iter()
                            .any(|mesh| !mesh.blended_parts().is_empty()))
            })
        })
    }

    /// Whether a soft sprite is active and whether it uses direct blending.
    pub fn depth_fade(&self, weighted: bool) -> Option<bool> {
        self.emitters
            .iter()
            .filter(|(emitter, _, _)| emitter.model.is_none() && emitter.depth_fade_distance > 0.0)
            .map(|(emitter, _, _)| !weighted || emitter.blend != ParticleBlend::SourceOver)
            .reduce(|direct, next| direct || next)
    }

    pub fn model_meshes(&self) -> impl Iterator<Item = &Mesh3D> {
        self.emitters
            .iter()
            .filter_map(|(emitter, _, _)| emitter.model.as_ref())
            .flat_map(|model| model.meshes.iter())
    }

    pub fn has_model_transparency(&self) -> bool {
        self.emitters.iter().any(|(emitter, sample, _)| {
            emitter.model.as_ref().is_some_and(|model| {
                model_fading(emitter, sample)
                    || model
                        .meshes
                        .iter()
                        .any(|mesh| !mesh.blended_parts().is_empty())
            })
        })
    }

    pub fn model_meshes_with_fading(&self) -> impl Iterator<Item = (&Mesh3D, bool)> {
        self.emitters
            .iter()
            .filter_map(|(emitter, sample, _)| {
                Some((emitter.model.as_ref()?, model_fading(emitter, sample)))
            })
            .flat_map(|(model, fading)| model.meshes.iter().map(move |mesh| (mesh, fading)))
    }

    pub fn model_transmission(&self, renderables: CaptureRenderables) -> Option<(f32, bool, bool)> {
        self.model_meshes_with_fading()
            .filter_map(|(mesh, fading)| {
                super::instances::mesh_transmission(mesh, renderables, fading)
            })
            .reduce(|a, b| (a.0.max(b.0), a.1 || b.1, a.2 || b.2))
    }
}

fn model_fading(emitter: &ParticleEmitter, sample: &ParticleSample) -> bool {
    let (first, last) = if emitter.reversed { (1, 0) } else { (0, 1) };
    emitter.color.0 as u8 != 255
        || emitter.color_variation[3] > 0.0
        || emitter.color_table.is_some()
        || (emitter.fade_effects[first] == ParticleFadeEffect::Opacity
            && emitter.fade[first] > sample.youngest_age)
        || (emitter.fade_effects[last] == ParticleFadeEffect::Opacity
            && emitter.fade[last] > 0.0
            && sample.oldest_age().next_up() >= emitter.shortest_lifetime() - emitter.fade[last])
}

#[derive(Default)]
struct Material {
    sprite: Option<SpriteBinding>,
    color_table: Option<SpriteBinding>,
    lighting: Option<wgpu::Buffer>,
    blend: ParticleBlend,
    sort: ParticleSortMode,
    mesh: Option<SurfaceBinding>,
    track: Option<wgpu::Buffer>,
    follow: Option<wgpu::Buffer>,
    trail_segments: u32,
    prepare_trail: bool,
    repeat_trail: bool,
    model: Option<Arc<[Mesh3D]>>,
    fading: bool,
    casts_shadow: bool,
}

impl Material {
    fn matches(&self, other: &Self) -> bool {
        let identity =
            |binding: &Option<SpriteBinding>| binding.as_ref().map(|texture| texture.identity);
        self.blend == other.blend
            && self.lighting == other.lighting
            && self.sort == other.sort
            && self.trail_segments == other.trail_segments
            && self.prepare_trail == other.prepare_trail
            && self.repeat_trail == other.repeat_trail
            && self.fading == other.fading
            && self.casts_shadow == other.casts_shadow
            && match (&self.model, &other.model) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && self.mesh.as_ref().map(|mesh| mesh.key) == other.mesh.as_ref().map(|mesh| mesh.key)
            && self.track == other.track
            && self.follow == other.follow
            && identity(&self.sprite) == identity(&other.sprite)
            && identity(&self.color_table) == identity(&other.color_table)
    }
}

struct Batch {
    bytes: Vec<u8>,
    motion: Vec<u8>,
    trails: Vec<u8>,
    pose_settings: Vec<u8>,
    material: Material,
    instances: u32,
    slots: usize,
    affected: bool,
    depth_fade: bool,
    shadow_bounds: Option<[[f32; 3]; 2]>,
}

impl Batch {
    fn new(material: Material) -> Self {
        Self {
            bytes: Vec::new(),
            motion: Vec::new(),
            trails: Vec::new(),
            pose_settings: Vec::new(),
            material,
            instances: 0,
            slots: 0,
            affected: false,
            depth_fade: false,
            shadow_bounds: None,
        }
    }

    fn push(&mut self, emitter: &ParticleEmitter, sample: ParticleSample, mask: u32) {
        let offset = sample
            .follow_offset
            .map_or(0, |start| start as i32 - self.instances as i32);
        self.instances += sample.count;
        self.affected |= mask != 0;
        self.depth_fade |= emitter.depth_fade_distance > 0.0;
        let world_oriented = emitter.model.is_none() && !emitter.sprite_billboard;
        if emitter.model.is_some() || world_oriented {
            self.pose_settings.resize(self.slots * POSE_BYTES, 0);
            let shadow = if emitter
                .model
                .as_ref()
                .is_some_and(|model| model.receives_shadow)
            {
                2.0
            } else {
                0.0
            };
            for ([x, y, z], flags) in emitter.rotation.into_iter().zip([0.0, 0.0, shadow, 0.0]) {
                append_floats(&mut self.pose_settings, [x, y, z, flags]);
            }
            append_floats(&mut self.pose_settings, emitter.alignment);
        } else if !self.pose_settings.is_empty() {
            self.pose_settings.resize((self.slots + 1) * POSE_BYTES, 0);
        }
        if let Some(trail) = emitter.trail {
            append_floats(
                &mut self.trails,
                [trail.duration, trail.end_scale, trail.fade, trail.eol_fade],
            );
            let [mode, multiplier] = if self.material.sprite.is_some() {
                [trail.texcoord_mode as u32 as f32, trail.texcoord_multiplier]
            } else {
                [0.0, 1.0]
            };
            append_floats(
                &mut self.trails,
                [trail.length, trail.length_variation, mode, multiplier],
            );
            append_floats(
                &mut self.trails,
                [
                    trail.length_delta_min,
                    trail.scale_multiplier,
                    trail.alpha_retention,
                    0.0,
                ],
            );
        }
        let default_motion = ParticleMotion::default();
        let motion = emitter
            .motion
            .as_ref()
            .or_else(|| sample.birth_frame.map(|_| &default_motion));
        if let Some(motion) = motion {
            self.motion.resize(self.slots * MOTION_BYTES, 0);
            append_floats(&mut self.motion, motion.shape.axes.into_iter().flatten());
            for column in [
                motion.shape.weights,
                motion.axis_speed,
                motion.right_cosine,
                motion.up_variation,
                motion.target,
                motion.target_spread,
            ] {
                append_floats(&mut self.motion, column);
            }
            let mut modes = motion.modes;
            let rows = if let Some(clock) = sample.loop_clock {
                modes[3] = 3;
                clock.words()
            } else if self.material.track.is_some() {
                modes[3] = 2;
                sample.birth_clock.map(|clock| [0, 0, 0, clock.to_bits()])
            } else {
                let placement = sample.birth_frame.or(motion.placement);
                modes[3] = u32::from(placement.is_some());
                placement
                    .unwrap_or_default()
                    .map(|row| row.map(f32::to_bits))
            };
            for value in modes.into_iter().chain(rows.into_iter().flatten()) {
                self.motion.extend_from_slice(&value.to_ne_bytes());
            }
        } else if !self.motion.is_empty() {
            self.motion.resize((self.slots + 1) * MOTION_BYTES, 0);
        }
        self.slots += 1;
        let bytes = &mut self.bytes;
        let [x, y, z] = if sample.birth_frame.is_some() {
            [0.0; 3]
        } else {
            emitter.position
        };
        append_floats(bytes, [x, y, z, 0.5 * emitter.size[0]]);
        let [x, y, z] = emitter.velocity;
        append_floats(bytes, [x, y, z, 0.5 * emitter.size[1]]);
        let [x, y, z] = emitter.velocity_spread;
        append_floats(bytes, [x, y, z, emitter.lifetime as f32]);
        let [x, y, z] = emitter.gravity;
        append_floats(bytes, [x, y, z, emitter.intensity]);
        let [red, green, blue, opacity] =
            emitter.color.0.to_be_bytes().map(|c| f32::from(c) / 255.0);
        // RGB jitter operates on authored sRGB channels. Unvaried RGB can be
        // converted once while preparing the draw instead of in every vertex.
        let [r, g, b] = if emitter.color_variation[..3]
            .iter()
            .any(|&channel| channel > 0.0)
        {
            [red, green, blue]
        } else {
            linear_rgb(emitter.color).map(|c| c as f32 * emitter.intensity)
        };
        append_floats(bytes, [r, g, b, opacity]);
        let [fade_in, fade_out] = emitter.fade;
        append_floats(
            bytes,
            [sample.interval, sample.youngest_age, fade_in, fade_out],
        );
        let flags = u32::from(world_oriented)
            | (u32::from(emitter.unified_color_variation) << 1)
            | ((emitter.fade_effects[0] as u32) << 2)
            | ((emitter.fade_effects[1] as u32) << 4);
        for value in [
            self.instances,
            sample.latest_id,
            flags
                | (u32::from(sample.continuous) << 6)
                | (u32::from(emitter.reversed) << 7)
                | (u32::from(sample.follow_offset.is_some() && emitter.follows_surface()) << 8),
            emitter.seed ^ sample.seed_salt,
        ] {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
        if let Some((texture, sprite)) = self.material.sprite.as_ref().zip(emitter.sprite.as_ref())
        {
            let [width, height] = texture.size.map(|value| value as f32);
            let frames = sprite.frames as f32;
            let cell = frames.recip();
            append_floats(
                bytes,
                [
                    frames,
                    sprite.duration,
                    sprite.duration_variation,
                    sprite.frame_offset as f32,
                ],
            );
            append_floats(
                bytes,
                [cell, 0.5 / width, 0.5 / height, width * cell / height],
            );
            for value in [
                sprite.playback as u32,
                u32::from(sprite.interpolate),
                u32::from(sprite.random_start),
                mask,
            ] {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
        } else {
            bytes.resize(bytes.len() + 44, 0);
            bytes.extend_from_slice(&mask.to_ne_bytes());
        }
        append_floats(
            bytes,
            [
                emitter.lifetime_variation,
                0.5 * emitter.size_variation,
                emitter
                    .end_size_variation
                    .map_or(-1.0, |variation| 0.5 * variation),
                emitter.depth_bias,
            ],
        );
        let billboard = if emitter.model.is_none() && emitter.sprite_billboard {
            emitter.rotation.map(|angles| angles[2])
        } else {
            [0.0; 4]
        };
        append_floats(bytes, billboard);
        append_floats(bytes, emitter.color_variation);
        let [offset_x, offset_y] = emitter.sprite_offset;
        append_floats(
            bytes,
            [
                emitter.depth_fade_distance,
                offset_x,
                offset_y,
                offset as f32,
            ],
        );
    }

    fn upload(mut self, device: &wgpu::Device) -> GpuParticleBatch {
        self.bytes.resize(MAX_PARTICLE_EMITTERS * EMITTER_BYTES, 0);
        GpuParticleBatch {
            uniforms: buffer(
                device,
                "XRack 3D emitter uniforms",
                &self.bytes,
                wgpu::BufferUsages::UNIFORM,
            ),
            motion: (!self.motion.is_empty()).then(|| {
                self.motion.resize(MAX_PARTICLE_EMITTERS * MOTION_BYTES, 0);
                buffer(
                    device,
                    "XRack particle motion uniforms",
                    &self.motion,
                    wgpu::BufferUsages::UNIFORM,
                )
            }),
            trails: (self.material.trail_segments != 0).then(|| {
                self.trails.resize(MAX_PARTICLE_EMITTERS * TRAIL_BYTES, 0);
                for value in [self.material.trail_segments, 0, 0, 0] {
                    self.trails.extend_from_slice(&value.to_ne_bytes());
                }
                buffer(
                    device,
                    "XRack particle trails",
                    &self.trails,
                    wgpu::BufferUsages::UNIFORM,
                )
            }),
            pose_settings: (!self.pose_settings.is_empty()).then(|| {
                self.pose_settings
                    .resize(MAX_PARTICLE_EMITTERS * POSE_BYTES, 0);
                buffer(
                    device,
                    "XRack 3D particle orientation",
                    &self.pose_settings,
                    wgpu::BufferUsages::UNIFORM,
                )
            }),
            material: self.material,
            trail_path: None,
            order: None,
            instances: self.instances,
            affected: self.affected,
            depth_fade: self.depth_fade,
            shadow_bounds: self.shadow_bounds,
        }
    }
}

pub(super) struct GpuParticleBatch {
    uniforms: wgpu::Buffer,
    motion: Option<wgpu::Buffer>,
    trails: Option<wgpu::Buffer>,
    pose_settings: Option<wgpu::Buffer>,
    material: Material,
    trail_path: Option<wgpu::Buffer>,
    order: Option<wgpu::Buffer>,
    pub instances: u32,
    affected: bool,
    depth_fade: bool,
    shadow_bounds: Option<[[f32; 3]; 2]>,
}

impl GpuParticleBatch {
    fn prepare_trail_path(
        &mut self,
        generator: &mut TrailGenerator,
        (device, encoder): (&wgpu::Device, &mut wgpu::CommandEncoder),
        slot: &mut usize,
        affectors: Option<&AffectorBindings>,
    ) {
        let Some(settings) = self.trails.as_ref().filter(|_| self.material.prepare_trail) else {
            return;
        };
        self.trail_path = generator.prepare(
            device,
            encoder,
            *slot,
            TrailInputs {
                emitters: &self.uniforms,
                settings,
                motion: self.motion.as_ref(),
                surface: self.material.mesh.as_ref().map(|surface| &surface.buffer),
                track: self.material.track.as_ref(),
                follow: self.material.follow.as_ref(),
                affectors: affectors.filter(|_| self.affected),
                instances: self.instances,
                segments: self.material.trail_segments,
                world_oriented: self.pose_settings.is_some(),
            },
        );
        *slot += usize::from(self.trail_path.is_some());
    }

    fn prepare_order(
        &mut self,
        sorter: &mut ParticleSorter,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        slot: &mut usize,
        view: Option<&wgpu::Buffer>,
        affectors: Option<&AffectorBindings>,
    ) {
        self.order = sorter.order(
            device,
            encoder,
            *slot,
            SortInputs {
                mode: self.material.sort,
                emitters: &self.uniforms,
                view,
                motion: self.motion.as_ref(),
                surface: self.material.mesh.as_ref().map(|surface| &surface.buffer),
                track: self.material.track.as_ref(),
                follow: self.material.follow.as_ref(),
                affectors: affectors.filter(|_| self.affected),
                count: self.instances,
                model: self.material.model.is_some(),
            },
        );
        *slot += usize::from(self.order.is_some());
    }

    pub fn trail_segments(&self) -> u64 {
        u64::from(self.instances) * u64::from(self.material.trail_segments)
    }
}

#[derive(Default)]
pub(super) struct ParticleBatches {
    pub direct: Vec<GpuParticleBatch>,
    pub weighted: Vec<GpuParticleBatch>,
    pub affectors: Option<AffectorBindings>,
    pub models: Vec<ModelParticleBatch>,
    order_slots: usize,
}

pub(super) struct ModelParticleBatch {
    pub meshes: Arc<[Mesh3D]>,
    pub fading: bool,
    pub instances: wgpu::Buffer,
    pub count: u32,
    pub shadow_bounds: Option<[[f32; 3]; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Composite {
    Direct(ParticleBlend),
    Weighted,
}

impl Composite {
    fn blend_state(self) -> wgpu::BlendState {
        use wgpu::BlendFactor::{Dst, One, OneMinusSrc, OneMinusSrcAlpha, Zero};
        let (src_factor, dst_factor) = match self {
            Self::Direct(ParticleBlend::Additive) => (One, One),
            Self::Direct(ParticleBlend::Screen) => (One, OneMinusSrc),
            Self::Direct(ParticleBlend::Multiply) => (Dst, OneMinusSrcAlpha),
            _ => return wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        };
        wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor,
                dst_factor,
                operation: wgpu::BlendOperation::Add,
            },
            // A multiply filter only modifies existing scene color. Emissive
            // sprites also need coverage when the scene is composed over 2D.
            alpha: if matches!(self, Self::Direct(ParticleBlend::Multiply)) {
                wgpu::BlendComponent {
                    src_factor: Zero,
                    dst_factor: One,
                    operation: wgpu::BlendOperation::Add,
                }
            } else {
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha
            },
        }
    }

    fn fragment(self, textured: bool, trail: bool) -> &'static str {
        if trail {
            return match (self, textured) {
                (Self::Weighted, false) => "fragment_trail_weighted",
                (Self::Weighted, true) => "fragment_trail_texture_weighted",
                (Self::Direct(ParticleBlend::Screen | ParticleBlend::Multiply), false) => {
                    "fragment_trail_bounded"
                }
                (Self::Direct(ParticleBlend::Screen | ParticleBlend::Multiply), true) => {
                    "fragment_trail_texture_bounded"
                }
                (Self::Direct(_), false) => "fragment_trail",
                (Self::Direct(_), true) => "fragment_trail_texture",
            };
        }
        match (self, textured) {
            (Self::Weighted, false) => "fragment_weighted",
            (Self::Weighted, true) => "fragment_texture_weighted",
            (Self::Direct(ParticleBlend::Screen | ParticleBlend::Multiply), false) => {
                "fragment_bounded"
            }
            (Self::Direct(ParticleBlend::Screen | ParticleBlend::Multiply), true) => {
                "fragment_texture_bounded"
            }
            (Self::Direct(_), false) => "fragment_sprite",
            (Self::Direct(_), true) => "fragment_texture",
        }
    }
}

#[derive(Default)]
pub(super) struct ParticlePipelines {
    shaders: [[[[Option<wgpu::ShaderModule>; 6]; 32]; 3]; 4],
    variants: HashMap<
        (
            [bool; 20],
            Composite,
            CapturePipelineState,
            super::wireframe::RasterMode,
            Option<wgpu::TextureFormat>,
        ),
        wgpu::RenderPipeline,
    >,
    sampler: Option<wgpu::Sampler>,
    repeat_sampler: Option<wgpu::Sampler>,
    textures: SpriteTextures,
    mesh_surfaces: MeshSurfaceBuffers,
    tracks: EmissionTrackBuffers,
    affectors: AffectorBuffers,
    model_instances: ModelInstanceGenerator,
    sprite_lights: LightBuffers,
    trail_paths: TrailGenerator,
    follower_origins: FollowGenerator,
    sorter: ParticleSorter,
    views: VecDeque<(Vec<u8>, wgpu::Buffer)>,
    view_uploads: u64,
}

impl ParticlePipelines {
    pub fn view(
        &mut self,
        device: &wgpu::Device,
        camera: SceneCamera,
        weight_scale: f32,
    ) -> wgpu::Buffer {
        let mut header = Vec::with_capacity(VIEW_BYTES);
        append_floats(&mut header, camera.clip_rows().into_iter().flatten());
        append_floats(&mut header, camera.depth_row());
        let [sx, sy, _, _] = camera.clip_lens();
        append_floats(
            &mut header,
            [
                sx,
                sy,
                weight_scale,
                f32::from(u8::from(camera.has_custom_projection())),
            ],
        );
        let lateral = camera.custom_view_clip_rows().map(|rows| {
            [0, 1].map(|axis| [rows[0][axis], rows[1][axis], rows[2][axis], rows[3][axis]])
        });
        append_floats(
            &mut header,
            lateral.unwrap_or([[0.0; 4]; 2]).into_iter().flatten(),
        );
        let entry = self
            .views
            .iter()
            .position(|(key, _)| *key == header)
            .and_then(|index| self.views.remove(index))
            .unwrap_or_else(|| {
                self.view_uploads += 1;
                let view = buffer(
                    device,
                    "XRack particle view",
                    &header,
                    wgpu::BufferUsages::UNIFORM,
                );
                (header, view)
            });
        let view = entry.1.clone();
        self.views.truncate(VIEW_CACHE_CAPACITY - 1);
        self.views.push_front(entry);
        view
    }

    fn prepare_follow(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (slot, source, plan, surface_directions): (
            usize,
            &ParticleEmitter,
            &ParticleFollowPlan,
            bool,
        ),
        affectors: &[&ParticleAffector],
    ) -> Option<wgpu::Buffer> {
        let origins =
            self.follower_origins
                .allocate(device, slot, plan.count, surface_directions)?;
        let mesh = match &source.mesh {
            Some(surface) => Some(self.mesh_surfaces.get(device, surface)?),
            None => None,
        };
        let mut trajectory = source.clone();
        trajectory.model = None;
        trajectory.trail = None;
        trajectory.sprite_billboard = true;
        let mask = (1 << affectors.len()) - 1;
        let fields = (!affectors.is_empty()).then(|| {
            self.affectors
                .get(device, encoder, &mut self.mesh_surfaces, affectors)
        });
        let mut groups = plan.groups.as_slice();
        while let Some(first) = groups.first() {
            let tracks_birth = |sample: &ParticleSample| {
                !source.emission_track.is_empty() && sample.birth_frame.is_none()
            };
            let tracked = tracks_birth(&first.source);
            let count = groups
                .iter()
                .take(MAX_PARTICLE_EMITTERS)
                .take_while(|group| tracks_birth(&group.source) == tracked)
                .count();
            let (chunk, next) = groups.split_at(count);
            let mut batch = Batch::new(Material {
                track: if tracked {
                    Some(self.tracks.get(device, &source.emission_track)?)
                } else {
                    None
                },
                ..Default::default()
            });
            let mut jobs = Vec::with_capacity(
                chunk.iter().map(|group| group.jobs.len()).sum::<usize>() * JOB_BYTES as usize,
            );
            for group in chunk {
                let base = batch.instances;
                batch.push(&trajectory, group.source, mask);
                for job in &group.jobs {
                    let child = job.child;
                    let target = child.follow_offset?;
                    for first in (0..child.count).step_by(CHILDREN_PER_JOB as usize) {
                        let count = (child.count - first).min(CHILDREN_PER_JOB);
                        for value in [base + job.source_index, count, target + first, 0] {
                            jobs.extend_from_slice(&value.to_ne_bytes());
                        }
                        let first_age =
                            job.source_age - (child.count - 1 - first) as f32 * job.source_step;
                        append_floats(&mut jobs, [first_age, job.source_step, 0.0, 0.0]);
                    }
                }
            }
            let batch = batch.upload(device);
            if !self.follower_origins.write(
                device,
                encoder,
                &origins,
                FollowInputs {
                    emitters: &batch.uniforms,
                    motion: batch.motion.as_ref(),
                    track: batch.material.track.as_ref(),
                    surface: mesh.as_ref().map(|surface| &surface.buffer),
                    affectors: fields.as_ref(),
                    jobs: &jobs,
                    surface_directions,
                },
            ) {
                return None;
            }
            groups = next;
        }
        Some(origins)
    }

    pub fn prepare(
        &mut self,
        draw: &ParticleDraw<'_>,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        (layer_mask, view): (u64, Option<&wgpu::Buffer>),
        (weighted, shadows): (bool, bool),
    ) -> ParticleBatches {
        let mut direct: Vec<Batch> = Vec::new();
        let mut oit: Vec<Batch> = Vec::new();
        let mut models: Vec<Batch> = Vec::new();
        let mut prepared_lights = HashMap::new();
        let matches =
            |affector: &ParticleAffector, emitter: &ParticleEmitter, sample: &ParticleSample| {
                affector.matches_emitter(emitter, layer_mask)
                    && affector.position_age[3] <= emitter.maximum_effect_age(sample)
            };
        let selected: Vec<_> = draw
            .affectors
            .iter()
            .filter(|affector| {
                draw.emitters
                    .iter()
                    .any(|(emitter, sample, _)| matches(affector, emitter, sample))
            })
            .collect();
        let follows: Vec<_> = draw
            .followers
            .iter()
            .enumerate()
            .map(|(slot, (source, plan, surface_directions))| {
                let layers = if source.layers & layer_mask != 0 {
                    layer_mask
                } else {
                    source.layers
                };
                let fields: Vec<_> = draw
                    .affectors
                    .iter()
                    .filter(|affector| affector.matches_emitter(source, layers))
                    .collect();
                let origins = self.prepare_follow(
                    device,
                    encoder,
                    (slot, source, plan, *surface_directions),
                    &fields,
                );
                let bounds = if origins.is_some()
                    && shadows
                    && draw.emitters.iter().any(|(emitter, _, index)| {
                        *index == Some(slot)
                            && emitter
                                .model
                                .as_ref()
                                .is_some_and(|model| model.casts_shadow)
                    }) {
                    source.follow_origin_bounds(plan, fields.iter().copied())
                } else {
                    None
                };
                (origins, bounds)
            })
            .collect();
        for (emitter, sample, follower) in &draw.emitters {
            let emitter = emitter.deref();
            let sample = *sample;
            let (follow, origin) = if let Some(index) = follower {
                let (Some(buffer), bounds) = &follows[*index] else {
                    continue;
                };
                (Some(buffer.clone()), *bounds)
            } else {
                (None, None)
            };
            let mesh = if let Some(surface) = &emitter.mesh {
                let Some(binding) = self.mesh_surfaces.get(device, surface) else {
                    continue;
                };
                Some(binding)
            } else {
                None
            };
            let sprite = emitter
                .sprite
                .as_ref()
                .filter(|_| emitter.model.is_none())
                .and_then(|sprite| self.textures.get(device, queue, &sprite.image));
            let prepare_trail = emitter.trail.is_some_and(|trail| {
                trail.length >= 0.0
                    || trail.length_delta_min > 0.0
                    || sprite.is_some()
                        && (trail.texcoord_mode != TrailTexcoordMode::Stretch
                            || trail.texcoord_multiplier != 1.0)
            });
            let repeat_trail = sprite.is_some()
                && emitter.trail.is_some_and(|trail| {
                    matches!(
                        trail.texcoord_mode,
                        TrailTexcoordMode::Absolute | TrailTexcoordMode::Relative
                    ) || trail.texcoord_multiplier != 1.0
                });
            let color_table = emitter
                .color_table
                .as_ref()
                .and_then(|image| self.textures.get(device, queue, image));
            let lighting = emitter.lights.as_ref().map(|lights| {
                let layers = emitter.layers & layer_mask;
                prepared_lights
                    .entry((Arc::as_ptr(lights), layers))
                    .or_insert_with(|| {
                        self.sprite_lights.get(
                            device,
                            LightSet::for_particles(draw.node, lights, layers),
                            [0; MAX_SCENE_LIGHTS],
                        )
                    })
                    .clone()
            });
            let track = if emitter.emission_track.is_empty() || sample.birth_frame.is_some() {
                None
            } else {
                if device.limits().max_storage_buffers_per_shader_stage
                    < 1 + u32::from(mesh.is_some()) + u32::from(follow.is_some())
                {
                    continue;
                }
                let Some(keys) = self.tracks.get(device, &emitter.emission_track) else {
                    continue;
                };
                Some(keys)
            };
            let accumulation =
                weighted && emitter.model.is_none() && emitter.blend == ParticleBlend::SourceOver;
            let fading = emitter.model.is_some() && model_fading(emitter, &sample);
            let sort = if accumulation
                || emitter.model.as_ref().is_some_and(|model| {
                    weighted
                        || (!fading
                            && model
                                .meshes
                                .iter()
                                .all(|mesh| mesh.blended_parts().is_empty()))
                }) {
                ParticleSortMode::None
            } else {
                emitter.sort
            };
            let material = Material {
                sprite,
                track,
                follow,
                color_table,
                lighting,
                blend: emitter.blend,
                sort,
                mesh,
                trail_segments: emitter.trail.map_or(0, |trail| trail.segments),
                prepare_trail,
                repeat_trail,
                model: emitter.model.as_ref().map(|model| model.meshes.clone()),
                fading,
                casts_shadow: shadows
                    && emitter
                        .model
                        .as_ref()
                        .is_some_and(|model| model.casts_shadow),
            };
            let batches = if emitter.model.is_some() {
                &mut models
            } else if accumulation {
                &mut oit
            } else {
                &mut direct
            };
            // Ordinary blending retains emitter order. Weighted blending can
            // combine all emitters with the same texture bindings.
            let index = if accumulation {
                batches.iter().position(|batch| {
                    batch.slots < MAX_PARTICLE_EMITTERS && batch.material.matches(&material)
                })
            } else {
                batches.len().checked_sub(1).filter(|&index| {
                    batches[index].slots < MAX_PARTICLE_EMITTERS
                        && batches[index].material.matches(&material)
                })
            };
            let index = index.unwrap_or_else(|| {
                batches.push(Batch::new(material));
                batches.len() - 1
            });
            let mask = selected
                .iter()
                .enumerate()
                .fold(0, |mask, (index, affector)| {
                    mask | (u32::from(matches(affector, emitter, &sample)) << index)
                });
            let batch = &mut batches[index];
            if batch.material.casts_shadow
                && let Some(bounds) = emitter.shadow_bounds(
                    &sample,
                    origin,
                    selected.iter().enumerate().filter_map(|(index, affector)| {
                        (mask & (1 << index) != 0).then_some(*affector)
                    }),
                )
            {
                batch.shadow_bounds = Some(batch.shadow_bounds.map_or(bounds, |previous| {
                    [
                        std::array::from_fn(|axis| previous[0][axis].min(bounds[0][axis])),
                        std::array::from_fn(|axis| previous[1][axis].max(bounds[1][axis])),
                    ]
                }));
            }
            batch.push(emitter, sample, mask);
        }
        let has_affectors = direct
            .iter()
            .chain(&oit)
            .chain(&models)
            .any(|batch| batch.affected);
        let affectors = has_affectors.then(|| {
            self.affectors
                .get(device, encoder, &mut self.mesh_surfaces, &selected)
        });
        let mut sort_slot = 0;
        let mut computed = Vec::with_capacity(models.len());
        for batch in models {
            let mut batch = batch.upload(device);
            batch.prepare_order(
                &mut self.sorter,
                device,
                encoder,
                &mut sort_slot,
                view,
                affectors.as_ref(),
            );
            let (Some(meshes), Some(settings)) = (batch.material.model, batch.pose_settings) else {
                continue;
            };
            let tint = batch.material.color_table.as_ref().map(|color| {
                (
                    &color.view,
                    particle_sampler(&mut self.sampler, device, false),
                )
            });
            let input = ModelInputs {
                emitters: &batch.uniforms,
                motion: batch.motion.as_ref(),
                surface: batch.material.mesh.as_ref().map(|surface| &surface.buffer),
                track: batch.material.track.as_ref(),
                follow: batch.material.follow.as_ref(),
                affectors: affectors.as_ref().filter(|_| batch.affected),
                settings: &settings,
                order: batch.order.as_ref(),
                tint,
                count: batch.instances,
            };
            if let Some(instances) =
                self.model_instances
                    .write(device, encoder, computed.len(), input)
            {
                computed.push(ModelParticleBatch {
                    meshes,
                    fading: batch.material.fading,
                    instances,
                    count: batch.instances,
                    shadow_bounds: batch.shadow_bounds,
                });
            }
        }
        let mut trail_slot = 0;
        let mut upload = |batch: Batch| {
            let mut batch = batch.upload(device);
            batch.prepare_trail_path(
                &mut self.trail_paths,
                (device, encoder),
                &mut trail_slot,
                affectors.as_ref(),
            );
            batch.prepare_order(
                &mut self.sorter,
                device,
                encoder,
                &mut sort_slot,
                view,
                affectors.as_ref(),
            );
            batch
        };
        ParticleBatches {
            direct: direct.into_iter().map(&mut upload).collect(),
            weighted: oit.into_iter().map(upload).collect(),
            affectors,
            models: computed,
            order_slots: sort_slot,
        }
    }

    pub fn reflection_view(
        &mut self,
        (device, encoder): (&wgpu::Device, &mut wgpu::CommandEncoder),
        batches: &mut ParticleBatches,
        camera: SceneCamera,
        weight_scale: f32,
    ) -> Option<wgpu::Buffer> {
        if batches.direct.is_empty() && batches.weighted.is_empty() {
            return None;
        }
        let view = self.view(device, camera, weight_scale);
        // Keep the age orders prepared for this step live until all faces are drawn.
        let mut slot = batches.order_slots;
        for batch in &mut batches.direct {
            if batch.material.sort == ParticleSortMode::Distance {
                batch.prepare_order(
                    &mut self.sorter,
                    device,
                    encoder,
                    &mut slot,
                    Some(&view),
                    batches.affectors.as_ref(),
                );
            }
        }
        Some(view)
    }

    pub fn sort_dispatches(&self) -> u64 {
        self.sorter.dispatches
    }

    pub fn view_uploads(&self) -> u64 {
        self.view_uploads
    }

    pub fn texture_uploads(&self) -> u64 {
        self.textures.uploads
    }

    pub fn trail_work(&self) -> (u64, u64) {
        (self.trail_paths.dispatches, self.trail_paths.allocations)
    }

    pub fn lighting_uploads(&self) -> u64 {
        self.sprite_lights.uploads
    }

    pub fn mesh_uploads(&self) -> u64 {
        self.mesh_surfaces.uploads
    }

    pub fn track_uploads(&self) -> u64 {
        self.tracks.uploads
    }

    pub fn affector_uploads(&self) -> u64 {
        self.affectors.uploads
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        batches: &[GpuParticleBatch],
        output: ColorOutput,
        (weighted, linked): (
            bool,
            Option<(
                &super::linked_transparency::LinkedFrame<'_>,
                &wgpu::TextureView,
            )>,
        ),
        (view, affectors, scene_depth): (
            &wgpu::Buffer,
            Option<&AffectorBindings>,
            Option<&ParticleDepth>,
        ),
    ) {
        if batches.is_empty() {
            return;
        }
        let wireframe = output.raster == super::wireframe::RasterMode::ShaderLine;
        let linked_samples = linked.map(|_| {
            super::linked_transparency::LinkedSamples::from_multisampling(output.multisampling)
        });
        let shader_slot =
            usize::from(wireframe) + linked_samples.map_or(0, |samples| 2 + 2 * samples.slot());
        for batch in batches {
            let prepared = batch.trail_path.is_some();
            let repeat = prepared && batch.material.repeat_trail;
            let follow = batch.material.follow.as_ref().filter(|_| !prepared);
            let affectors = affectors.filter(|_| !prepared && batch.affected);
            let mesh_targets = AffectorBindings::has_mesh(affectors);
            let tracked = !prepared && batch.material.track.is_some();
            let oriented = batch.pose_settings.is_some();
            let sorted = batch.order.is_some();
            let soft = scene_depth.filter(|_| batch.depth_fade);
            let lighting = batch.material.lighting.as_ref();
            let variant = usize::from(tracked)
                + 2 * usize::from(oriented)
                + 4 * usize::from(sorted)
                + 8 * usize::from(soft.is_some())
                + 16 * usize::from(lighting.is_some());
            let path_variant = usize::from(prepared) + usize::from(repeat);
            let shader = self.shaders[usize::from(follow.is_some()) + 2 * usize::from(mesh_targets)][path_variant][variant][shader_slot].get_or_insert_with(|| {
                let source = particles_shader(tracked, oriented, sorted, follow.is_some(), mesh_targets);
                let source = if prepared {
                    let mut path = source.into_owned();
                    if repeat {
                        path = path.replacen("// XRack repeating ribbon texture", "uv.y = local.y;", 1);
                    }
                    path.push('\n');
                    path.push_str(&include_str!("particle_trail_render.wgsl")
                        .replace("XRACK_TRAIL_BINDING", &PARTICLE_TRAIL_BINDING.to_string())
                        .replace("XRACK_TRAIL_HEADER_ROWS", if oriented { "1u" } else { "0u" }));
                    std::borrow::Cow::Owned(path)
                } else {
                    source
                };
                let source = if lighting.is_some() {
                    let mut lit = source.replacen(
                        "return color; // XRack particle lighting",
                        "return color * particle_irradiance(world);",
                        1,
                    );
                    lit.push_str("\n");
                    lit.push_str(include_str!("light_eval.wgsl"));
                    lit.push_str(&format!("\n@group(0) @binding({PARTICLE_LIGHT_BINDING}) var<uniform> particle_lighting: Lighting;\n"));
                    lit.push_str(
                        r#"
fn particle_irradiance(world: vec3<f32>) -> vec3<f32> {
    var irradiance = particle_lighting.ambient.rgb;
    for (var index = 0u; index < u32(particle_lighting.settings.x); index++) {
        irradiance += incident_light(particle_lighting.lights[index], world).radiance;
    }
    return min(irradiance, vec3<f32>(1000000.0));
}
"#,
                    );
                    std::borrow::Cow::Owned(lit)
                } else {
                    source
                };
                let source = if soft.is_some() {
                    std::borrow::Cow::Owned(source.replacen(
                        "return 1.0; // XRack particle intersection fade",
                        "return sample_intersection(position, distance);",
                        1,
                    ))
                } else {
                    source
                };
                let source = if wireframe {
                    std::borrow::Cow::Owned(super::wireframe::particles(&source))
                } else {
                    source
                };
                let source = if let Some(samples) = linked_samples {
                    std::borrow::Cow::Owned(super::linked_transparency::capture_shader(
                        &source, samples,
                    ))
                } else {
                    source
                };
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("XRack 3D sprites"),
                    source: wgpu::ShaderSource::Wgsl(source),
                })
            });
            let textured = batch.material.sprite.is_some();
            let tinted = batch.material.color_table.is_some();
            let spatial = !prepared && batch.motion.is_some();
            let mesh = !prepared && batch.material.mesh.is_some();
            let trail = batch.trails.is_some();
            let composite = if weighted {
                Composite::Weighted
            } else {
                Composite::Direct(if output.pipeline_state.blend.is_some() {
                    ParticleBlend::SourceOver
                } else {
                    batch.material.blend
                })
            };
            let key = (
                [
                    textured,
                    tinted,
                    spatial,
                    mesh,
                    affectors.is_some(),
                    trail,
                    output.multisampling,
                    output.hdr && !weighted,
                    output.stenciled,
                    output.reverse_depth,
                    linked.is_some(),
                    tracked,
                    oriented,
                    sorted,
                    soft.is_some(),
                    lighting.is_some(),
                    prepared,
                    repeat,
                    follow.is_some(),
                    mesh_targets,
                ],
                composite,
                super::pipeline::fixed_state(output.pipeline_state),
                output.raster,
                if weighted {
                    None
                } else {
                    output.format_override
                },
            );
            let pipeline = self.variants.entry(key).or_insert_with(|| {
                let linked_constants =
                    [("XR_LINKED_REVERSE_DEPTH", f64::from(output.reverse_depth))];
                let ordinary = [Some(wgpu::ColorTargetState {
                    format: output.format(),
                    blend: output.pipeline_state.blend.map_or_else(
                        || Some(composite.blend_state()),
                        super::pipeline::override_blend,
                    ),
                    write_mask: super::pipeline::color_writes(output.pipeline_state),
                })];
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("XRack 3D particle sprites"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some(if prepared {
                            if tinted {
                                "vertex_prepared_trail_tinted"
                            } else {
                                "vertex_prepared_trail"
                            }
                        } else {
                            vertex_entry(mesh, spatial, tinted, affectors.is_some(), trail)
                        }),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &[("TEXTURED", if textured { 1.0 } else { 0.0 })],
                            ..Default::default()
                        },
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(composite.fragment(textured, trail)),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: if linked.is_some() {
                                &linked_constants
                            } else {
                                &[]
                            },
                            ..Default::default()
                        },
                        targets: if weighted {
                            &super::pipeline::WEIGHTED_TARGETS
                        } else {
                            &ordinary
                        },
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        polygon_mode: output.raster.polygon_mode(),
                        cull_mode: output
                            .pipeline_state
                            .cull
                            .and_then(super::pipeline::culled_face),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: output.depth_format(),
                        depth_write_enabled: Some(
                            output.pipeline_state.depth_write.unwrap_or(false),
                        ),
                        depth_compare: Some(super::pipeline::depth_compare(
                            output.pipeline_state,
                            wgpu::CompareFunction::Less,
                            output.reverse_depth,
                        )),
                        stencil: super::pipeline::stencil_state(output.pipeline_state),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: output.samples(),
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                })
            });
            let mut entries = Vec::with_capacity(
                5 + usize::from(spatial)
                    + usize::from(mesh)
                    + usize::from(affectors.is_some())
                    + usize::from(trail)
                    + usize::from(tracked)
                    + usize::from(oriented)
                    + usize::from(sorted)
                    + 2 * usize::from(soft.is_some())
                    + usize::from(lighting.is_some())
                    + usize::from(prepared),
            );
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: batch.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: VIEW_BINDING,
                    resource: view.as_entire_binding(),
                },
            ]);
            if let Some(motion) = batch.motion.as_ref().filter(|_| spatial) {
                entries.push(wgpu::BindGroupEntry {
                    binding: 4,
                    resource: motion.as_entire_binding(),
                });
            }
            if let Some(mesh) = batch.material.mesh.as_ref().filter(|_| mesh) {
                entries.push(wgpu::BindGroupEntry {
                    binding: 5,
                    resource: mesh.buffer.as_entire_binding(),
                });
            }
            if let Some(keys) = batch.material.track.as_ref().filter(|_| tracked) {
                entries.push(wgpu::BindGroupEntry {
                    binding: TRACK_BINDING,
                    resource: keys.as_entire_binding(),
                });
            }
            if let Some(affectors) = affectors {
                entries.extend(affectors.entries());
            }
            if let Some(trails) = &batch.trails {
                entries.push(wgpu::BindGroupEntry {
                    binding: 7,
                    resource: trails.as_entire_binding(),
                });
            }
            if let Some(order) = &batch.order {
                entries.push(wgpu::BindGroupEntry {
                    binding: SORT_BINDING,
                    resource: order.as_entire_binding(),
                });
            }
            if let Some(poses) = &batch.pose_settings {
                entries.push(wgpu::BindGroupEntry {
                    binding: POSE_BINDING,
                    resource: poses.as_entire_binding(),
                });
            }
            if textured || tinted {
                let store = if repeat {
                    &mut self.repeat_sampler
                } else {
                    &mut self.sampler
                };
                let sampler = particle_sampler(store, device, repeat);
                entries.push(wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                });
                if let Some(sprite) = &batch.material.sprite {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&sprite.view),
                    });
                }
                if let Some(color_table) = &batch.material.color_table {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&color_table.view),
                    });
                }
            }
            if let Some((frame, depth)) = linked {
                entries.extend(frame.capture_entries(depth));
            }
            if let Some(depth) = soft {
                entries.extend([
                    wgpu::BindGroupEntry {
                        binding: SCENE_DEPTH_BINDING,
                        resource: depth.pixels.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: DEPTH_PROJECTION_BINDING,
                        resource: depth.projection.as_entire_binding(),
                    },
                ]);
            }
            if let Some(lighting) = lighting {
                entries.push(wgpu::BindGroupEntry {
                    binding: PARTICLE_LIGHT_BINDING,
                    resource: lighting.as_entire_binding(),
                });
            }
            if let Some(follow) = follow {
                entries.push(wgpu::BindGroupEntry {
                    binding: FOLLOW_BINDING,
                    resource: follow.as_entire_binding(),
                });
            }
            if let Some(path) = &batch.trail_path {
                entries.push(wgpu::BindGroupEntry {
                    binding: PARTICLE_TRAIL_BINDING,
                    resource: path.as_entire_binding(),
                });
            }
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack 3D emitter bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            let vertices = if trail {
                2 * (batch.material.trail_segments + 1)
            } else {
                4
            };
            pass.draw(0..vertices, 0..batch.instances);
        }
    }
}

fn particle_sampler<'a>(
    sampler: &'a mut Option<wgpu::Sampler>,
    device: &wgpu::Device,
    repeat_v: bool,
) -> &'a wgpu::Sampler {
    sampler.get_or_insert_with(|| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("XRack particle sheet sampler"),
            address_mode_v: if repeat_v {
                wgpu::AddressMode::Repeat
            } else {
                wgpu::AddressMode::ClampToEdge
            },
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        })
    })
}
