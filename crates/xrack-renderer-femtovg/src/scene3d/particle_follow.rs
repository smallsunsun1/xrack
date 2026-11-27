//! GPU birth anchors for child particles sampled on the parent's trajectory.
use super::{
    buffer,
    linked_transparency::PARTICLE_TRAIL_BINDING,
    particle_affectors::AffectorBindings,
    particle_tracks::{TRACK_BINDING, particles_shader},
};

pub(super) const FOLLOW_BINDING: u32 = PARTICLE_TRAIL_BINDING + 1;
pub(super) const JOB_BINDING: u32 = FOLLOW_BINDING + 1;
const WORKGROUP_SIZE: u32 = 64;
pub(super) const CHILDREN_PER_JOB: u32 = 8;
pub(super) const JOB_BYTES: u32 = 32;

pub(super) struct FollowInputs<'a> {
    pub emitters: &'a wgpu::Buffer,
    pub motion: Option<&'a wgpu::Buffer>,
    pub surface: Option<&'a wgpu::Buffer>,
    pub track: Option<&'a wgpu::Buffer>,
    pub affectors: Option<&'a AffectorBindings>,
    pub jobs: &'a [u8],
    pub surface_directions: bool,
}

#[derive(Default)]
pub(super) struct FollowGenerator {
    pipelines: [[Option<wgpu::ComputePipeline>; 16]; 3],
    origins: Vec<wgpu::Buffer>,
}

impl FollowGenerator {
    pub fn allocate(
        &mut self,
        device: &wgpu::Device,
        slot: usize,
        count: u32,
        surface_directions: bool,
    ) -> Option<wgpu::Buffer> {
        let limits = device.limits();
        let stride = if surface_directions { 48 } else { 16 };
        let bytes = stride * u64::from(count.next_power_of_two());
        if bytes > limits.max_storage_buffer_binding_size
            || bytes > limits.max_buffer_size
            || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
            || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
        {
            return None;
        }
        let allocate = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack child particle birth anchors"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        if slot == self.origins.len() {
            self.origins.push(allocate());
        }
        let origins = &mut self.origins[slot];
        if origins.size() < bytes {
            *origins = allocate();
        }
        Some(origins.clone())
    }

    pub fn write(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        origins: &wgpu::Buffer,
        input: FollowInputs<'_>,
    ) -> bool {
        let limits = device.limits();
        if limits.max_storage_buffers_per_shader_stage
            < 2 + u32::from(input.surface.is_some()) + u32::from(input.track.is_some())
            || input.jobs.len() as u64
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
        {
            return false;
        }
        let jobs = buffer(
            device,
            "XRack particle follow jobs",
            input.jobs,
            wgpu::BufferUsages::STORAGE,
        );
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
            + 4 * usize::from(mesh_targets)
            + 8 * usize::from(input.surface_directions);
        let pipeline = self.pipelines[geometry][variant].get_or_insert_with(|| {
            let mut source =
                particles_shader(tracked, false, false, false, mesh_targets).into_owned();
            if input.surface_directions {
                source = source.replacen(
                    "const FOLLOW_SURFACE_SOURCE = false;",
                    "const FOLLOW_SURFACE_SOURCE = true;",
                    1,
                );
            }
            source.push('\n');
            source.push_str(
                &include_str!("particle_follow.wgsl")
                    .replace("XRACK_FOLLOW_BINDING", &FOLLOW_BINDING.to_string())
                    .replace("XRACK_JOB_BINDING", &JOB_BINDING.to_string())
                    .replace(
                        "XRACK_FOLLOW_SOURCE",
                        ["vector_source", "spatial_source", "mesh_source"][geometry],
                    )
                    .replace(
                        "XRACK_FOLLOW_CENTER",
                        if affected {
                            "apply_affectors(source).source.center"
                        } else {
                            "source.center"
                        },
                    ),
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack particle follow sampling"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack compute child particle birth anchors"),
                layout: None,
                module: &shader,
                entry_point: Some("compute_follow"),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let mut entries: Vec<_> = [
            (0, Some(input.emitters)),
            (4, input.motion),
            (5, input.surface),
            (TRACK_BINDING, input.track),
            (FOLLOW_BINDING, Some(origins)),
            (JOB_BINDING, Some(&jobs)),
        ]
        .into_iter()
        .filter_map(|(binding, buffer)| {
            Some(wgpu::BindGroupEntry {
                binding,
                resource: buffer?.as_entire_binding(),
            })
        })
        .collect();
        if let Some(fields) = input.affectors {
            entries.extend(fields.entries());
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack particle follow inputs"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack prepare child particle anchors"),
                ..Default::default()
            },
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            (input.jobs.len() as u32 / JOB_BYTES).div_ceil(WORKGROUP_SIZE),
            1,
            1,
        );
        true
    }
}
