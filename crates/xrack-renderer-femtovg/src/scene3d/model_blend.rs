//! One GPU invocation per triangle, shared by color, depth, and capture passes.
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use xrack_runtime::scene3d::{Mesh3D, MeshVertex, MeshVertexStreams, ModelBlendPose};

use super::{MeshBuffers, buffer, mesh_buffer};

const TRIANGLES_PER_GROUP: u32 = 64;

struct Output {
    source: Weak<MeshVertexStreams>,
    pose: ModelBlendPose,
    frame: u64,
    mesh: Arc<MeshBuffers>,
}

#[derive(Default)]
pub(super) struct ModelBlendBuffers {
    inputs: HashMap<usize, (Weak<MeshVertexStreams>, MeshBuffers)>,
    outputs: Vec<Output>,
    pipelines: [Option<wgpu::ComputePipeline>; 4],
    pub dispatches: u64,
    pub allocations: u64,
    pub source_uploads: u64,
}

impl ModelBlendBuffers {
    pub fn begin_frame(&mut self, frame: u64) {
        self.inputs
            .retain(|_, (source, _)| source.strong_count() != 0);
        self.outputs.retain(|entry| {
            entry.source.strong_count() != 0 && frame.wrapping_sub(entry.frame) <= 4
        });
    }

    pub fn get(&self, mesh: &Mesh3D) -> Option<&Arc<MeshBuffers>> {
        let Mesh3D::Imported(data) = mesh else {
            return None;
        };
        let pose = mesh.model_blend()?;
        let key = Arc::as_ptr(data.vertex_streams());
        self.outputs
            .iter()
            .find(|entry| entry.source.as_ptr() == key && entry.pose == pose)
            .map(|entry| &entry.mesh)
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: u64,
        mesh: &Mesh3D,
    ) {
        let Mesh3D::Imported(data) = mesh else {
            return;
        };
        let Some(pose) = mesh.model_blend() else {
            return;
        };
        let streams = data.vertex_streams();
        let key = Arc::as_ptr(streams);
        let same = |entry: &Output| entry.source.as_ptr() == key;
        if let Some(cached) = self
            .outputs
            .iter_mut()
            .find(|entry| same(entry) && entry.pose == pose)
        {
            cached.frame = frame;
            return;
        }
        let bytes = u64::from(pose.triangles()) * 3 * MeshVertex::STRIDE as u64;
        let groups = pose.triangles().div_ceil(TRIANGLES_PER_GROUP);
        let tangents = !mesh.tangents().is_empty();
        let limits = device.limits();
        if bytes
            > limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
            || groups > limits.max_compute_workgroups_per_dimension
            || limits.max_compute_workgroup_size_x < TRIANGLES_PER_GROUP
            || limits.max_compute_invocations_per_workgroup < TRIANGLES_PER_GROUP
            || limits.max_storage_buffers_per_shader_stage < if tangents { 4 } else { 2 }
        {
            return;
        }
        let (_, rest) = self.inputs.entry(key as usize).or_insert_with(|| {
            self.source_uploads += 1;
            (
                Arc::downgrade(streams),
                mesh_buffer(device, mesh, true, None),
            )
        });
        let output = if let Some(entry) = self
            .outputs
            .iter_mut()
            .find(|entry| same(entry) && entry.frame != frame)
        {
            entry.frame = frame;
            entry.pose = pose;
            &entry.mesh
        } else {
            let create = |label, size| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            };
            self.allocations += 1;
            self.outputs.push(Output {
                source: Arc::downgrade(streams),
                pose,
                frame,
                mesh: Arc::new(MeshBuffers {
                    vertices: create("XRack model particles posed vertices", bytes),
                    tangents: rest.tangents.as_ref().map(|buffer| {
                        create("XRack model particles posed tangents", buffer.size())
                    }),
                    index: None,
                    skin_weights: None,
                    morph: None,
                    point_rows: None,
                    point_ranges: Vec::new(),
                    line_ranges: Vec::new(),
                    line_vertices: None,
                    binormals: None,
                    lightmap_uv: Default::default(),
                }),
            });
            let Some(entry) = self.outputs.last() else {
                return;
            };
            &entry.mesh
        };
        let settings: Vec<_> = pose
            .uniforms()
            .into_iter()
            .flatten()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let uniform = buffer(
            device,
            "XRack model particle trajectory",
            &settings,
            wgpu::BufferUsages::UNIFORM,
        );
        let affine = pose.has_affine_destination();
        let slot = usize::from(tangents) + 2 * usize::from(affine);
        let pipeline = self.pipelines[slot].get_or_insert_with(|| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack triangle particle shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("model_blend.wgsl").into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("XRack model particles"),
                layout: None,
                module: &shader,
                entry_point: Some(if tangents { "deform_tangent" } else { "deform" }),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("AFFINE_DESTINATION", f64::from(affine))],
                    ..Default::default()
                },
                cache: None,
            })
        });
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: rest.vertices.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.vertices.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ];
        if let Some((source, destination)) = rest.tangents.as_ref().zip(output.tangents.as_ref()) {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: destination.as_entire_binding(),
                },
            ]);
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack triangle particle geometry"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = crate::viewport_gpu_timing::compute_pass(
            encoder,
            &wgpu::ComputePassDescriptor {
                label: Some("XRack compute triangle particle pose"),
                ..Default::default()
            },
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(groups, 1, 1);
        self.dispatches += 1;
    }
}
