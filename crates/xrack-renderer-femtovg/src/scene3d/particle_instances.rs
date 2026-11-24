//! GPU generation of the regular mesh instance stream from particle ages.
use std::num::NonZeroU64;

use super::INSTANCE_STRIDE;

pub(super) struct ModelInputs<'a> {
    pub emitters: &'a wgpu::Buffer,
    pub motion: Option<&'a wgpu::Buffer>,
    pub surface: Option<&'a wgpu::Buffer>,
    pub affectors: Option<&'a wgpu::Buffer>,
    pub settings: &'a wgpu::Buffer,
    pub tint: Option<(&'a wgpu::TextureView, &'a wgpu::Sampler)>,
    pub count: u32,
}

#[derive(Default)]
pub(super) struct ModelInstanceGenerator {
    shader: Option<wgpu::ShaderModule>,
    pipelines: [Option<wgpu::ComputePipeline>; 12],
    instances: Vec<wgpu::Buffer>,
}

impl ModelInstanceGenerator {
    pub fn write(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        input: ModelInputs<'_>,
    ) -> Option<wgpu::Buffer> {
        let storage = INSTANCE_STRIDE * u64::from(input.count);
        let limits = device.limits();
        if limits.max_compute_invocations_per_workgroup < 64
            || limits.max_compute_workgroup_size_x < 64
            || limits.max_storage_buffers_per_shader_stage < 1 + u32::from(input.surface.is_some())
            || storage > limits.max_storage_buffer_binding_size
            || storage > limits.max_buffer_size
        {
            return None;
        }
        let make_buffer = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack GPU particle instances"),
                size: INSTANCE_STRIDE * u64::from(input.count.next_power_of_two()),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            })
        };
        // Each prepared viewport records its dispatches and draws in order
        // before another viewport reuses the same allocation slots.
        if slot == self.instances.len() {
            self.instances.push(make_buffer());
        }
        let allocation = &mut self.instances[slot];
        if allocation.size() < storage {
            *allocation = make_buffer();
        }
        let shader = self.shader.get_or_insert_with(|| {
            let source = concat!(
                include_str!("particles.wgsl"),
                "\n",
                include_str!("particle_instances.wgsl")
            );
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack model particle compute"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        });
        let geometry = if input.surface.is_some() {
            2
        } else {
            usize::from(input.motion.is_some())
        };
        let variant = 4 * geometry
            + 2 * usize::from(input.affectors.is_some())
            + usize::from(input.tint.is_some());
        const ENTRIES: [&str; 12] = [
            "compute_vector",
            "compute_vector_tinted",
            "compute_vector_affected",
            "compute_vector_affected_tinted",
            "compute_spatial",
            "compute_spatial_tinted",
            "compute_spatial_affected",
            "compute_spatial_affected_tinted",
            "compute_mesh",
            "compute_mesh_tinted",
            "compute_mesh_affected",
            "compute_mesh_affected_tinted",
        ];
        let pipeline = self.pipelines[variant].get_or_insert_with(|| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack model particle transforms"),
                layout: None,
                module: shader,
                entry_point: Some(ENTRIES[variant]),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let mut entries = Vec::with_capacity(8);
        entries.extend([
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.emitters.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: allocation,
                    offset: 0,
                    size: NonZeroU64::new(storage),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: input.settings.as_entire_binding(),
            },
        ]);
        for (binding, buffer) in [(4, input.motion), (5, input.surface), (6, input.affectors)] {
            if let Some(buffer) = buffer {
                entries.push(wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                });
            }
        }
        if let Some((view, sampler)) = input.tint {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(view),
                },
            ]);
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack model particle inputs"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("XRack update model particle instances"),
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(input.count.div_ceil(64), 1, 1);
        Some(allocation.clone())
    }
}
