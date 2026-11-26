//! Camera-facing sprites evaluated from emitter uniforms and GPU instance indices.
use std::sync::Arc;
use std::{collections::HashMap, ops::Deref};

use xrack_runtime::scene_effect::{CapturePipelineState, CaptureRenderables};
use xrack_runtime::{
    Node, Value,
    scene3d::{
        CaptureMaterial, MAX_PARTICLE_EMITTERS, MAX_PARTICLES, Mesh3D, ParticleAffector,
        ParticleBlend, ParticleEmitter, ParticleMotion, ParticleSample, SceneCamera,
    },
};

use super::{
    append_floats, buffer, linear_rgb,
    particle_affectors::AffectorBuffers,
    particle_instances::{ModelInputs, ModelInstanceGenerator},
    particle_meshes::{MeshSurfaceBuffers, SurfaceBinding},
    particle_textures::{SpriteBinding, SpriteTextures},
    particle_tracks::{EmissionTrackBuffers, TRACK_BINDING, particles_shader},
    pipeline::ColorOutput,
};

const VIEW_BYTES: usize = 128;
// Every emitter occupies twelve aligned vec4 slots in the shader.
const EMITTER_BYTES: usize = 192;
const MOTION_BYTES: usize = 208;
const TRAIL_BYTES: usize = 16;
const MODEL_BYTES: usize = 64;

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
    emitters: Vec<(DrawEmitter<'a>, ParticleSample)>,
    affectors: &'a [ParticleAffector],
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
        Self::sample_filtered(node, camera, CaptureRenderables::All)
    }

    pub fn sample_filtered(
        node: &'a Node,
        camera: &SceneCamera,
        renderables: CaptureRenderables,
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
        let mut remaining = MAX_PARTICLES;
        for emitter in emitters.iter().take(MAX_PARTICLE_EMITTERS) {
            if emitter.layers & camera.layer_mask() == 0 {
                continue;
            }
            for sample in emitter.samples(time, remaining) {
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
                active.push((DrawEmitter::Source(emitter), sample));
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
            affectors,
        })
    }

    pub fn sample_with_capture_material(
        node: &'a Node,
        camera: &SceneCamera,
        filter: CaptureRenderables,
        material: Option<&Arc<CaptureMaterial>>,
    ) -> Option<Self> {
        let mut draw = Self::sample_filtered(node, camera, filter)?;
        if let Some(material) = material {
            draw.emitters.retain_mut(|(emitter, sample)| {
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
        self.emitters.iter().any(|(emitter, _)| {
            emitter.model.is_none() && emitter.blend == ParticleBlend::SourceOver
        })
    }

    pub fn has_custom_blend(&self) -> bool {
        self.emitters.iter().any(|(emitter, _)| {
            emitter.model.is_none() && emitter.blend != ParticleBlend::SourceOver
        })
    }

    pub fn model_meshes(&self) -> impl Iterator<Item = &Mesh3D> {
        self.emitters
            .iter()
            .filter_map(|(emitter, _)| emitter.model.as_ref())
            .flat_map(|model| model.meshes.iter())
    }

    pub fn has_model_transparency(&self) -> bool {
        self.emitters.iter().any(|(emitter, sample)| {
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
            .filter_map(|(emitter, sample)| {
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
    emitter.color.0 as u8 != 255
        || emitter.color_table.is_some()
        || emitter.fade[0] > sample.youngest_age
        || (emitter.fade[1] > 0.0
            && sample.oldest_age().next_up() >= emitter.shortest_lifetime() - emitter.fade[1])
}

struct Material {
    sprite: Option<SpriteBinding>,
    color_table: Option<SpriteBinding>,
    blend: ParticleBlend,
    mesh: Option<SurfaceBinding>,
    track: Option<wgpu::Buffer>,
    trail_segments: u32,
    model: Option<Arc<[Mesh3D]>>,
    fading: bool,
    casts_shadow: bool,
}

impl Material {
    fn matches(&self, other: &Self) -> bool {
        let identity =
            |binding: &Option<SpriteBinding>| binding.as_ref().map(|texture| texture.identity);
        self.blend == other.blend
            && self.trail_segments == other.trail_segments
            && self.fading == other.fading
            && self.casts_shadow == other.casts_shadow
            && match (&self.model, &other.model) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && self.mesh.as_ref().map(|mesh| mesh.key) == other.mesh.as_ref().map(|mesh| mesh.key)
            && self.track == other.track
            && identity(&self.sprite) == identity(&other.sprite)
            && identity(&self.color_table) == identity(&other.color_table)
    }
}

struct Batch {
    bytes: Vec<u8>,
    motion: Vec<u8>,
    trails: Vec<u8>,
    model_settings: Vec<u8>,
    material: Material,
    instances: u32,
    slots: usize,
    affected: bool,
    shadow_bounds: Option<[[f32; 3]; 2]>,
}

impl Batch {
    fn push(&mut self, emitter: &ParticleEmitter, sample: ParticleSample, mask: u32) {
        self.instances += sample.count;
        self.affected |= mask != 0;
        if let Some(model) = &emitter.model {
            let shadow = if model.receives_shadow { 2.0 } else { 0.0 };
            for ([x, y, z], flags) in [
                (model.rotation, 0.0),
                (model.rotation_variation, 0.0),
                (model.angular_velocity, shadow),
                (model.angular_velocity_variation, 0.0),
            ] {
                append_floats(&mut self.model_settings, [x, y, z, flags]);
            }
        }
        if let Some(trail) = emitter.trail {
            append_floats(
                &mut self.trails,
                [trail.duration, trail.end_scale, trail.fade, trail.eol_fade],
            );
        }
        let default_motion = ParticleMotion::default();
        let motion = emitter
            .motion
            .as_ref()
            .or_else(|| sample.birth_frame.map(|_| &default_motion));
        if let Some(motion) = motion {
            self.motion.resize(self.slots * MOTION_BYTES, 0);
            append_floats(&mut self.motion, motion.shape.into_iter().flatten());
            for column in [
                motion.surface_weights,
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
        append_floats(bytes, [x, y, z, 0.0]);
        let [r, g, b] = linear_rgb(emitter.color).map(|c| c as f32 * emitter.intensity);
        let opacity = (emitter.color.0 & 0xff) as f32 / 255.0;
        append_floats(bytes, [r, g, b, opacity]);
        let [fade_in, fade_out] = emitter.fade;
        append_floats(
            bytes,
            [sample.interval, sample.youngest_age, fade_in, fade_out],
        );
        for value in [
            self.instances,
            sample.latest_id,
            0,
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
                0.0,
            ],
        );
        let billboard = if emitter.model.is_none() {
            emitter.billboard_rotation
        } else {
            [0.0; 4]
        };
        append_floats(bytes, billboard);
    }

    fn upload(mut self, device: &wgpu::Device) -> GpuParticleBatch {
        self.bytes
            .resize(VIEW_BYTES + MAX_PARTICLE_EMITTERS * EMITTER_BYTES, 0);
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
            model_settings: (!self.model_settings.is_empty()).then(|| {
                self.model_settings
                    .resize(MAX_PARTICLE_EMITTERS * MODEL_BYTES, 0);
                buffer(
                    device,
                    "XRack model particle rotation",
                    &self.model_settings,
                    wgpu::BufferUsages::UNIFORM,
                )
            }),
            material: self.material,
            instances: self.instances,
            affected: self.affected,
            shadow_bounds: self.shadow_bounds,
        }
    }
}

pub(super) struct GpuParticleBatch {
    uniforms: wgpu::Buffer,
    motion: Option<wgpu::Buffer>,
    trails: Option<wgpu::Buffer>,
    model_settings: Option<wgpu::Buffer>,
    material: Material,
    pub instances: u32,
    affected: bool,
    shadow_bounds: Option<[[f32; 3]; 2]>,
}

impl GpuParticleBatch {
    pub fn trail_segments(&self) -> u64 {
        u64::from(self.instances) * u64::from(self.material.trail_segments)
    }
}

#[derive(Default)]
pub(super) struct ParticleBatches {
    pub direct: Vec<GpuParticleBatch>,
    pub weighted: Vec<GpuParticleBatch>,
    pub affectors: Option<wgpu::Buffer>,
    pub models: Vec<ModelParticleBatch>,
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
    shaders: [[Option<wgpu::ShaderModule>; 6]; 2],
    variants: HashMap<
        (
            [bool; 12],
            Composite,
            CapturePipelineState,
            super::wireframe::RasterMode,
            Option<wgpu::TextureFormat>,
        ),
        wgpu::RenderPipeline,
    >,
    sampler: Option<wgpu::Sampler>,
    textures: SpriteTextures,
    mesh_surfaces: MeshSurfaceBuffers,
    tracks: EmissionTrackBuffers,
    affectors: AffectorBuffers,
    model_instances: ModelInstanceGenerator,
}

impl ParticlePipelines {
    pub fn prepare(
        &mut self,
        draw: &ParticleDraw<'_>,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        camera: SceneCamera,
        weight_scale: f32,
        (weighted, shadows): (bool, bool),
    ) -> ParticleBatches {
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
        let mut direct: Vec<Batch> = Vec::new();
        let mut oit: Vec<Batch> = Vec::new();
        let mut models: Vec<Batch> = Vec::new();
        let matches =
            |affector: &ParticleAffector, emitter: &ParticleEmitter, sample: &ParticleSample| {
                affector.matches_emitter(emitter, camera.layer_mask())
                    // The shader may fuse the multiply-add in the stream age.
                    && affector.position_age[3] <= sample.oldest_age().next_up()
            };
        let selected: Vec<_> = draw
            .affectors
            .iter()
            .filter(|affector| {
                draw.emitters
                    .iter()
                    .any(|(emitter, sample)| matches(affector, emitter, sample))
            })
            .collect();
        for (emitter, sample) in &draw.emitters {
            let emitter = emitter.deref();
            let sample = *sample;
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
            let color_table = emitter
                .color_table
                .as_ref()
                .and_then(|image| self.textures.get(device, queue, image));
            let track = if emitter.emission_track.is_empty() || sample.birth_frame.is_some() {
                None
            } else {
                if device.limits().max_storage_buffers_per_shader_stage
                    < 1 + u32::from(mesh.is_some())
                {
                    continue;
                }
                let Some(keys) = self.tracks.get(device, &emitter.emission_track) else {
                    continue;
                };
                Some(keys)
            };
            let material = Material {
                sprite,
                track,
                color_table,
                blend: emitter.blend,
                mesh,
                trail_segments: emitter.trail.map_or(0, |trail| trail.segments),
                model: emitter.model.as_ref().map(|model| model.meshes.clone()),
                fading: emitter.model.is_some() && model_fading(emitter, &sample),
                casts_shadow: shadows
                    && emitter
                        .model
                        .as_ref()
                        .is_some_and(|model| model.casts_shadow),
            };
            let accumulation =
                weighted && emitter.model.is_none() && emitter.blend == ParticleBlend::SourceOver;
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
                batches.push(Batch {
                    bytes: header.clone(),
                    motion: Vec::new(),
                    trails: Vec::new(),
                    model_settings: Vec::new(),
                    material,
                    instances: 0,
                    slots: 0,
                    affected: false,
                    shadow_bounds: None,
                });
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
        let affectors = has_affectors.then(|| self.affectors.get(device, &selected));
        let mut computed = Vec::with_capacity(models.len());
        for batch in models {
            let batch = batch.upload(device);
            let (Some(meshes), Some(settings)) = (batch.material.model, batch.model_settings)
            else {
                continue;
            };
            let tint = batch
                .material
                .color_table
                .as_ref()
                .map(|color| (&color.view, particle_sampler(&mut self.sampler, device)));
            let input = ModelInputs {
                emitters: &batch.uniforms,
                motion: batch.motion.as_ref(),
                surface: batch.material.mesh.as_ref().map(|surface| &surface.buffer),
                track: batch.material.track.as_ref(),
                affectors: affectors.as_ref().filter(|_| batch.affected),
                settings: &settings,
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
        ParticleBatches {
            direct: direct
                .into_iter()
                .map(|batch| batch.upload(device))
                .collect(),
            weighted: oit.into_iter().map(|batch| batch.upload(device)).collect(),
            affectors,
            models: computed,
        }
    }

    pub fn texture_uploads(&self) -> u64 {
        self.textures.uploads
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
        affectors: Option<&wgpu::Buffer>,
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
            let tracked = batch.material.track.is_some();
            let shader = self.shaders[usize::from(tracked)][shader_slot].get_or_insert_with(|| {
                let source = particles_shader(tracked);
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
            let spatial = batch.motion.is_some();
            let mesh = batch.material.mesh.is_some();
            let trail = batch.trails.is_some();
            let affectors = affectors.filter(|_| batch.affected);
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
                        entry_point: Some(vertex_entry(
                            mesh,
                            spatial,
                            tinted,
                            affectors.is_some(),
                            trail,
                        )),
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
                4 + usize::from(spatial)
                    + usize::from(mesh)
                    + usize::from(affectors.is_some())
                    + usize::from(trail)
                    + usize::from(tracked),
            );
            entries.push(wgpu::BindGroupEntry {
                binding: 0,
                resource: batch.uniforms.as_entire_binding(),
            });
            if let Some(motion) = &batch.motion {
                entries.push(wgpu::BindGroupEntry {
                    binding: 4,
                    resource: motion.as_entire_binding(),
                });
            }
            if let Some(mesh) = &batch.material.mesh {
                entries.push(wgpu::BindGroupEntry {
                    binding: 5,
                    resource: mesh.buffer.as_entire_binding(),
                });
            }
            if let Some(keys) = &batch.material.track {
                entries.push(wgpu::BindGroupEntry {
                    binding: TRACK_BINDING,
                    resource: keys.as_entire_binding(),
                });
            }
            if let Some(affectors) = affectors {
                entries.push(wgpu::BindGroupEntry {
                    binding: 6,
                    resource: affectors.as_entire_binding(),
                });
            }
            if let Some(trails) = &batch.trails {
                entries.push(wgpu::BindGroupEntry {
                    binding: 7,
                    resource: trails.as_entire_binding(),
                });
            }
            if textured || tinted {
                let sampler = particle_sampler(&mut self.sampler, device);
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
) -> &'a wgpu::Sampler {
    sampler.get_or_insert_with(|| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("XRack particle sheet sampler"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        })
    })
}
