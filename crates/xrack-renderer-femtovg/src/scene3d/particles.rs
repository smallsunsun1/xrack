//! Camera-facing sprites evaluated from emitter uniforms and GPU instance indices.
use std::collections::HashMap;

use xrack_runtime::{
    Node, Value,
    scene3d::{
        MAX_PARTICLE_EMITTERS, MAX_PARTICLES, ParticleAffector, ParticleBlend, ParticleEmitter,
        ParticleSample, SceneCamera,
    },
};

use super::{
    DEPTH_FORMAT, append_floats, buffer, linear_rgb,
    particle_affectors::AffectorBuffers,
    particle_meshes::{MeshSurfaceBuffers, SurfaceBinding},
    particle_textures::{SpriteBinding, SpriteTextures},
    pipeline::ColorOutput,
};

const VIEW_BYTES: usize = 96;
// Every emitter occupies ten aligned vec4 slots in the shader.
const EMITTER_BYTES: usize = 160;
const MOTION_BYTES: usize = 160;
const TRAIL_BYTES: usize = 16;

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
    emitters: Vec<(&'a ParticleEmitter, ParticleSample)>,
    affectors: &'a [ParticleAffector],
}

impl<'a> ParticleDraw<'a> {
    pub fn sample(node: &'a Node, camera: &SceneCamera) -> Option<Self> {
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
            let Some(sample) = emitter.sample(time, remaining) else {
                continue;
            };
            remaining -= sample.count;
            active.push((emitter, sample));
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

    pub fn has_source_over(&self) -> bool {
        self.emitters
            .iter()
            .any(|(emitter, _)| emitter.blend == ParticleBlend::SourceOver)
    }

    pub fn has_custom_blend(&self) -> bool {
        self.emitters
            .iter()
            .any(|(emitter, _)| emitter.blend != ParticleBlend::SourceOver)
    }
}

struct Material {
    sprite: Option<SpriteBinding>,
    color_table: Option<SpriteBinding>,
    blend: ParticleBlend,
    mesh: Option<SurfaceBinding>,
    trail_segments: u32,
}

impl Material {
    fn matches(&self, other: &Self) -> bool {
        let identity =
            |binding: &Option<SpriteBinding>| binding.as_ref().map(|texture| texture.identity);
        self.blend == other.blend
            && self.trail_segments == other.trail_segments
            && self.mesh.as_ref().map(|mesh| mesh.key) == other.mesh.as_ref().map(|mesh| mesh.key)
            && identity(&self.sprite) == identity(&other.sprite)
            && identity(&self.color_table) == identity(&other.color_table)
    }
}

struct Batch {
    bytes: Vec<u8>,
    motion: Vec<u8>,
    trails: Vec<u8>,
    material: Material,
    instances: u32,
    emitters: usize,
    affected: bool,
}

impl Batch {
    fn push(&mut self, emitter: &ParticleEmitter, sample: ParticleSample, mask: u32) {
        self.instances += sample.count;
        self.affected |= mask != 0;
        if let Some(trail) = emitter.trail {
            append_floats(
                &mut self.trails,
                [trail.duration, trail.end_scale, trail.fade, trail.eol_fade],
            );
        }
        if let Some(motion) = &emitter.motion {
            self.motion.resize(self.emitters * MOTION_BYTES, 0);
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
            for value in motion.modes {
                self.motion.extend_from_slice(&value.to_ne_bytes());
            }
        } else if !self.motion.is_empty() {
            self.motion.resize((self.emitters + 1) * MOTION_BYTES, 0);
        }
        self.emitters += 1;
        let bytes = &mut self.bytes;
        let [x, y, z] = emitter.position;
        append_floats(bytes, [x, y, z, 0.5 * emitter.size[0]]);
        let [x, y, z] = emitter.velocity;
        append_floats(bytes, [x, y, z, 0.5 * emitter.size[1]]);
        let [x, y, z] = emitter.velocity_spread;
        append_floats(bytes, [x, y, z, emitter.lifetime as f32]);
        let [x, y, z] = emitter.gravity;
        append_floats(bytes, [x, y, z, sample.burst_age]);
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
            sample.burst_count,
            emitter.seed,
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
            material: self.material,
            instances: self.instances,
            affected: self.affected,
        }
    }
}

pub(super) struct GpuParticleBatch {
    uniforms: wgpu::Buffer,
    motion: Option<wgpu::Buffer>,
    trails: Option<wgpu::Buffer>,
    material: Material,
    pub instances: u32,
    affected: bool,
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
            alpha: wgpu::BlendComponent {
                src_factor: Zero,
                dst_factor: One,
                operation: wgpu::BlendOperation::Add,
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
    shader: Option<wgpu::ShaderModule>,
    variants: HashMap<([bool; 8], Composite), wgpu::RenderPipeline>,
    sampler: Option<wgpu::Sampler>,
    textures: SpriteTextures,
    mesh_surfaces: MeshSurfaceBuffers,
    affectors: AffectorBuffers,
}

impl ParticlePipelines {
    pub fn prepare(
        &mut self,
        draw: ParticleDraw<'_>,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        camera: SceneCamera,
        weight_scale: f32,
        weighted: bool,
    ) -> ParticleBatches {
        let mut header = Vec::with_capacity(VIEW_BYTES);
        append_floats(&mut header, camera.clip_rows().into_iter().flatten());
        append_floats(&mut header, camera.depth_row());
        let [sx, sy, _, _] = camera.clip_lens();
        append_floats(&mut header, [sx, sy, weight_scale, 0.0]);
        let mut direct: Vec<Batch> = Vec::new();
        let mut oit: Vec<Batch> = Vec::new();
        let matches =
            |affector: &ParticleAffector, emitter: &ParticleEmitter, sample: &ParticleSample| {
                affector.layers & emitter.layers & camera.layer_mask() != 0
                    // The shader may fuse the multiply-add in the stream age.
                    && affector.position_age[3] <= sample.oldest_age().next_up()
            };
        let selected: Vec<_> = draw
            .affectors
            .iter()
            .copied()
            .filter(|affector| {
                draw.emitters
                    .iter()
                    .any(|(emitter, sample)| matches(affector, emitter, sample))
            })
            .collect();
        for (emitter, sample) in draw.emitters {
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
                .and_then(|sprite| self.textures.get(device, queue, &sprite.image));
            let color_table = emitter
                .color_table
                .as_ref()
                .and_then(|image| self.textures.get(device, queue, image));
            let material = Material {
                sprite,
                color_table,
                blend: emitter.blend,
                mesh,
                trail_segments: emitter.trail.map_or(0, |trail| trail.segments),
            };
            let accumulation = weighted && emitter.blend == ParticleBlend::SourceOver;
            let batches = if accumulation { &mut oit } else { &mut direct };
            // Ordinary blending retains emitter order. Weighted blending can
            // combine all emitters with the same texture bindings.
            let index = if accumulation {
                batches
                    .iter()
                    .position(|batch| batch.material.matches(&material))
            } else {
                batches
                    .len()
                    .checked_sub(1)
                    .filter(|&index| batches[index].material.matches(&material))
            };
            let index = index.unwrap_or_else(|| {
                batches.push(Batch {
                    bytes: header.clone(),
                    motion: Vec::new(),
                    trails: Vec::new(),
                    material,
                    instances: 0,
                    emitters: 0,
                    affected: false,
                });
                batches.len() - 1
            });
            let mask = selected
                .iter()
                .enumerate()
                .fold(0, |mask, (index, affector)| {
                    mask | (u32::from(matches(affector, emitter, &sample)) << index)
                });
            batches[index].push(emitter, sample, mask);
        }
        let has_affectors = direct.iter().chain(&oit).any(|batch| batch.affected);
        ParticleBatches {
            direct: direct
                .into_iter()
                .map(|batch| batch.upload(device))
                .collect(),
            weighted: oit.into_iter().map(|batch| batch.upload(device)).collect(),
            affectors: has_affectors.then(|| self.affectors.get(device, selected)),
        }
    }

    pub fn texture_uploads(&self) -> u64 {
        self.textures.uploads
    }

    pub fn mesh_uploads(&self) -> u64 {
        self.mesh_surfaces.uploads
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
        weighted: bool,
        affectors: Option<&wgpu::Buffer>,
    ) {
        if batches.is_empty() {
            return;
        }
        let shader = self.shader.get_or_insert_with(|| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack 3D sprites"),
                source: wgpu::ShaderSource::Wgsl(include_str!("particles.wgsl").into()),
            })
        });
        for batch in batches {
            let textured = batch.material.sprite.is_some();
            let tinted = batch.material.color_table.is_some();
            let spatial = batch.motion.is_some();
            let mesh = batch.material.mesh.is_some();
            let trail = batch.trails.is_some();
            let affectors = affectors.filter(|_| batch.affected);
            let composite = if weighted {
                Composite::Weighted
            } else {
                Composite::Direct(batch.material.blend)
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
                ],
                composite,
            );
            let pipeline = self.variants.entry(key).or_insert_with(|| {
                let ordinary = [Some(wgpu::ColorTargetState {
                    format: output.format(),
                    blend: Some(composite.blend_state()),
                    write_mask: wgpu::ColorWrites::ALL,
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
                        compilation_options: Default::default(),
                        targets: if weighted {
                            &super::pipeline::WEIGHTED_TARGETS
                        } else {
                            &ordinary
                        },
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
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
                    + usize::from(trail),
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
                let sampler = self.sampler.get_or_insert_with(|| {
                    device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some("XRack particle sheet sampler"),
                        min_filter: wgpu::FilterMode::Linear,
                        mag_filter: wgpu::FilterMode::Linear,
                        ..Default::default()
                    })
                });
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
