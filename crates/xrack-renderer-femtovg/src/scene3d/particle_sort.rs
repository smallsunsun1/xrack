//! GPU particle order for ordinary transparency.
use std::{collections::HashMap, num::NonZeroU64};

use xrack_runtime::scene3d::ParticleSortMode;

use super::{
    particle_affectors::AffectorBindings,
    particle_follow::FOLLOW_BINDING,
    particle_tracks::{TRACK_BINDING, particles_shader},
    particles::VIEW_BINDING,
};

pub(super) const SORT_BINDING: u32 = 11;
const TILE_WIDTH: u32 = 128;
const WORKGROUP_SIZE: u32 = 64;
const KEY_BYTES: u64 = 8;

pub(super) struct SortInputs<'a> {
    pub mode: ParticleSortMode,
    pub emitters: &'a wgpu::Buffer,
    pub view: Option<&'a wgpu::Buffer>,
    pub motion: Option<&'a wgpu::Buffer>,
    pub surface: Option<&'a wgpu::Buffer>,
    pub track: Option<&'a wgpu::Buffer>,
    pub follow: Option<&'a wgpu::Buffer>,
    pub affectors: Option<&'a AffectorBindings>,
    pub count: u32,
    pub model: bool,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum KeySource {
    Newest,
    Oldest,
    Depth {
        geometry: &'static str,
        tracked: bool,
        affected: bool,
        followed: bool,
        mesh_targets: bool,
    },
}

impl KeySource {
    fn shader(self) -> String {
        let tracked = matches!(self, Self::Depth { tracked: true, .. });
        let mut source = particles_shader(
            tracked,
            false,
            false,
            matches!(self, Self::Depth { followed: true, .. }),
            matches!(
                self,
                Self::Depth {
                    mesh_targets: true,
                    ..
                }
            ),
        )
        .into_owned();
        source.push_str(include_str!("particle_sort_tile.wgsl"));
        source.push_str("\nfn particle_order_key(index: u32) -> f32 {\n");
        match self {
            Self::Newest => {
                source.push_str("return particle_age(emitters[particle_slot(index)], index);");
            }
            Self::Oldest => {
                source.push_str("return -particle_age(emitters[particle_slot(index)], index);");
            }
            Self::Depth {
                geometry, affected, ..
            } => {
                source.push_str("let birth = ");
                source.push_str(geometry);
                source.push_str("(index);\nlet head_age = min(birth.particle.seconds, birth.particle.emitter.spread_lifetime.w);\nlet sample = trajectory(birth, head_age);\n");
                if affected {
                    source.push_str("let center = apply_affectors(sample).source.center;\n");
                } else {
                    source.push_str("let center = sample.center;\n");
                }
                source.push_str("return -(dot(view.view_depth, vec4<f32>(center, 1.0)) + birth.particle.emitter.birth_variation.w);");
            }
        }
        source.push_str("\n}\n");
        source
    }
}

#[derive(Default)]
pub(super) struct ParticleSorter {
    buffers: Vec<[wgpu::Buffer; 2]>,
    tiles: HashMap<KeySource, wgpu::ComputePipeline>,
    merge_shader: Option<wgpu::ShaderModule>,
    merges: HashMap<u32, wgpu::ComputePipeline>,
    pub dispatches: u64,
}

impl ParticleSorter {
    pub fn order(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        input: SortInputs<'_>,
    ) -> Option<wgpu::Buffer> {
        let source = match input.mode {
            ParticleSortMode::None => return None,
            ParticleSortMode::Newest => KeySource::Newest,
            ParticleSortMode::Oldest => KeySource::Oldest,
            ParticleSortMode::Distance => KeySource::Depth {
                geometry: if input.surface.is_some() {
                    "mesh_source"
                } else if input.motion.is_some() {
                    "spatial_source"
                } else {
                    "vector_source"
                },
                tracked: input.track.is_some(),
                affected: input.affectors.is_some(),
                followed: input.follow.is_some(),
                mesh_targets: AffectorBindings::has_mesh(input.affectors),
            },
        };
        let view = match source {
            KeySource::Depth { .. } => Some(input.view?),
            _ => None,
        };
        let capacity = KEY_BYTES * u64::from(input.count.next_power_of_two());
        let limits = device.limits();
        if input.count <= 1
            || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
            || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
            || limits.max_compute_workgroup_storage_size < TILE_WIDTH * KEY_BYTES as u32
            || limits.max_storage_buffers_per_shader_stage
                < 2.max(
                    1 + u32::from(input.model)
                        + u32::from(input.surface.is_some())
                        + u32::from(input.track.is_some())
                        + u32::from(input.follow.is_some()),
                )
            || capacity > limits.max_storage_buffer_binding_size
            || capacity > limits.max_buffer_size
        {
            return None;
        }
        let allocate = || {
            std::array::from_fn(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("XRack particle sort keys"),
                    size: capacity,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
        };
        if slot == self.buffers.len() {
            self.buffers.push(allocate());
        }
        let buffers = &mut self.buffers[slot];
        if buffers[0].size() < capacity {
            *buffers = allocate();
        }
        let binding = |buffer| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(KEY_BYTES * u64::from(input.count)),
            })
        };
        let tile = self.tiles.entry(source).or_insert_with(|| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack particle sort tiles"),
                source: wgpu::ShaderSource::Wgsl(source.shader().into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack particle sort tiles"),
                layout: None,
                module: &shader,
                entry_point: Some("sort_tile"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.emitters.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: SORT_BINDING,
                resource: binding(&buffers[0]),
            },
        ];
        if let Some(view) = view {
            entries.push(wgpu::BindGroupEntry {
                binding: VIEW_BINDING,
                resource: view.as_entire_binding(),
            });
            if let Some(fields) = input.affectors {
                entries.extend(fields.entries());
            }
            for (binding, buffer) in [
                (4, input.motion),
                (5, input.surface),
                (TRACK_BINDING, input.track),
                (FOLLOW_BINDING, input.follow),
            ] {
                if let Some(buffer) = buffer {
                    entries.push(wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    });
                }
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack particle sort inputs"),
            layout: &tile.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack order transparent particles"),
                ..Default::default()
            },
        );
        pass.set_pipeline(tile);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(input.count.div_ceil(TILE_WIDTH), 1, 1);
        self.dispatches = self.dispatches.saturating_add(1);

        let mut current = 0;
        let mut width = TILE_WIDTH;
        while width < input.count {
            let shader = self.merge_shader.get_or_insert_with(|| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("XRack particle sort merge"),
                    source: wgpu::ShaderSource::Wgsl(
                        include_str!("particle_sort_merge.wgsl").into(),
                    ),
                })
            });
            let merge = self.merges.entry(width).or_insert_with(|| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("XRack merge particle sort runs"),
                    layout: None,
                    module: shader,
                    entry_point: Some("merge_runs"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("RUN_WIDTH", f64::from(width))],
                        ..Default::default()
                    },
                    cache: None,
                })
            });
            let output = current ^ 1;
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack particle sort runs"),
                layout: &merge.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: binding(&buffers[current]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: binding(&buffers[output]),
                    },
                ],
            });
            pass.set_pipeline(merge);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(input.count.div_ceil(WORKGROUP_SIZE), 1, 1);
            self.dispatches = self.dispatches.saturating_add(1);
            current = output;
            width *= 2;
        }
        Some(buffers[current].clone())
    }
}
