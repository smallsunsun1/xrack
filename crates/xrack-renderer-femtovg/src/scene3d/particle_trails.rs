//! Build camera-independent ribbon paths before the first view consumes them.
use std::num::NonZeroU64;

use super::{
    linked_transparency::PARTICLE_TRAIL_BINDING,
    particle_affectors::AffectorBindings,
    particle_follow::FOLLOW_BINDING,
    particle_tracks::{TRACK_BINDING, particles_shader},
};

const POINT_BYTES: u64 = 32;
const WORKGROUP_SIZE: u32 = 64;

pub(super) struct TrailInputs<'a> {
    pub emitters: &'a wgpu::Buffer,
    pub settings: &'a wgpu::Buffer,
    pub motion: Option<&'a wgpu::Buffer>,
    pub surface: Option<&'a wgpu::Buffer>,
    pub track: Option<&'a wgpu::Buffer>,
    pub follow: Option<&'a wgpu::Buffer>,
    pub affectors: Option<&'a AffectorBindings>,
    pub instances: u32,
    pub segments: u32,
    pub world_oriented: bool,
}

#[derive(Default)]
pub(super) struct TrailGenerator {
    pipelines: [[Option<wgpu::ComputePipeline>; 32]; 3],
    buffers: Vec<wgpu::Buffer>,
    pub dispatches: u64,
    pub allocations: u64,
}

impl TrailGenerator {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        input: TrailInputs<'_>,
    ) -> Option<wgpu::Buffer> {
        let heading_rows = u32::from(input.world_oriented);
        let stride = u64::from(input.segments + 1 + heading_rows) * POINT_BYTES;
        let bytes = stride * u64::from(input.instances);
        let limits = device.limits();
        if bytes == 0
            || bytes > limits.max_buffer_size
            || bytes > limits.max_storage_buffer_binding_size
            || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
            || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
            || limits.max_storage_buffers_per_shader_stage
                < 1 + u32::from(input.surface.is_some())
                    + u32::from(input.track.is_some())
                    + u32::from(input.follow.is_some())
        {
            return None;
        }
        let allocate = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack particle ribbon path"),
                size: (stride * u64::from(input.instances.next_power_of_two())).min(
                    limits
                        .max_buffer_size
                        .min(limits.max_storage_buffer_binding_size)
                        & !3,
                ),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        if slot == self.buffers.len() {
            self.buffers.push(allocate());
            self.allocations += 1;
        }
        let path = &mut self.buffers[slot];
        if path.size() < bytes {
            *path = allocate();
            self.allocations += 1;
        }
        let geometry = if input.surface.is_some() {
            2
        } else {
            usize::from(input.motion.is_some())
        };
        let tracked = input.track.is_some();
        let affected = input.affectors.is_some();
        let mesh_targets = AffectorBindings::has_mesh(input.affectors);
        let variant = usize::from(tracked)
            + 2 * usize::from(affected)
            + 4 * heading_rows as usize
            + 8 * usize::from(input.follow.is_some())
            + 16 * usize::from(mesh_targets);
        let pipeline = self.pipelines[geometry][variant].get_or_insert_with(|| {
            let mut source =
                particles_shader(tracked, false, false, input.follow.is_some(), mesh_targets)
                    .into_owned();
            source.push('\n');
            source.push_str(
                &include_str!("particle_trail_compute.wgsl")
                    .replace("XRACK_TRAIL_BINDING", &PARTICLE_TRAIL_BINDING.to_string())
                    .replace("XRACK_TRAIL_HEADER_ROWS", &format!("{heading_rows}u"))
                    .replace(
                        "XRACK_TRAIL_BIRTH",
                        ["vector_source", "spatial_source", "mesh_source"][geometry],
                    )
                    .replace(
                        "XRACK_TRAIL_AFFECTED",
                        if affected { "true" } else { "false" },
                    )
                    .replace(
                        "XRACK_TRAIL_SAMPLE",
                        if affected {
                            "apply_affectors(trajectory(birth, age))"
                        } else {
                            "Displaced(trajectory(birth, age), true)"
                        },
                    ),
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack particle ribbon sampling"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack sample and trim ribbon paths"),
                layout: None,
                module: &shader,
                entry_point: Some("compute_trail"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let mut bindings = Vec::with_capacity(7);
        for (binding, buffer) in [
            (0, Some(input.emitters)),
            (7, Some(input.settings)),
            (4, input.motion),
            (5, input.surface),
            (TRACK_BINDING, input.track),
            (FOLLOW_BINDING, input.follow),
        ] {
            if let Some(buffer) = buffer {
                bindings.push(wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                });
            }
        }
        bindings.push(wgpu::BindGroupEntry {
            binding: PARTICLE_TRAIL_BINDING,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: path,
                offset: 0,
                size: NonZeroU64::new(bytes),
            }),
        });
        if let Some(fields) = input.affectors {
            bindings.extend(fields.entries());
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack ribbon path inputs"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &bindings,
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack prepare particle ribbons"),
                ..Default::default()
            },
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(input.instances.div_ceil(WORKGROUP_SIZE), 1, 1);
        self.dispatches += 1;
        Some(path.clone())
    }
}
