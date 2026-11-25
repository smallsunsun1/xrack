//! Collider lines drawn over the viewport's resolved color.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
};

use xrack_runtime::{PhysicsDebugVertex, scene3d::SceneCamera};

use super::{append_floats, buffer};
use crate::GpuPaintContext;

struct CachedVertices {
    source: Weak<[PhysicsDebugVertex]>,
    buffer: wgpu::Buffer,
}

#[derive(Default)]
pub(super) struct PhysicsDebugRenderer {
    shader: Option<wgpu::ShaderModule>,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    vertices: VecDeque<CachedVertices>,
}

impl PhysicsDebugRenderer {
    pub(super) fn draw(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        camera: SceneCamera,
        lines: &Arc<[PhysicsDebugVertex]>,
    ) -> Option<(u32, bool)> {
        let count = u32::try_from(lines.len()).ok()?;
        const STRIDE: u64 = 16;
        if count == 0 || u64::from(count) * STRIDE > ctx.device.limits().max_buffer_size {
            return None;
        }
        let source = Arc::downgrade(lines);
        let cached = self
            .vertices
            .iter()
            .position(|item| item.source.ptr_eq(&source))
            .and_then(|index| self.vertices.remove(index));
        let uploaded = cached.is_none();
        self.vertices.retain(|item| item.source.strong_count() != 0);
        self.vertices.push_front(cached.unwrap_or_else(|| {
            let mut bytes = Vec::with_capacity(lines.len() * STRIDE as usize);
            for vertex in lines.iter() {
                append_floats(&mut bytes, vertex.position);
                bytes.extend_from_slice(&vertex.color);
            }
            CachedVertices {
                source,
                buffer: buffer(
                    ctx.device,
                    "XRack physics debug vertices",
                    &bytes,
                    wgpu::BufferUsages::VERTEX,
                ),
            }
        }));
        self.vertices.truncate(8);
        let vertices = &self.vertices.front()?.buffer;

        let mut bytes = Vec::with_capacity(64);
        append_floats(&mut bytes, camera.clip_rows().into_iter().flatten());
        let matrix = buffer(
            ctx.device,
            "XRack physics debug camera",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let pipeline = self.pipelines.entry(ctx.format).or_insert_with(|| {
            let shader = self.shader.get_or_insert_with(|| {
                ctx.device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("XRack physics debug lines"),
                        source: wgpu::ShaderSource::Wgsl(include_str!("physics_debug.wgsl").into()),
                    })
            });
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("XRack physics debug lines"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vertex"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: STRIDE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fragment"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: ctx.format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::LineList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        });
        let bindings = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack physics debug camera binding"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: matrix.as_entire_binding(),
            }],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack physics debug overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..count, 0..1);
        Some((count / 2, uploaded))
    }
}
