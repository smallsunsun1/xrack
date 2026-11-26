//! Depth-aware reflection probe guides drawn over the resolved viewport.
use std::collections::HashMap;

use xrack_runtime::scene3d::{ReflectionProbe, SceneCamera};

use super::{FocusDepth, append_floats, buffer};
use crate::GpuPaintContext;

pub(super) fn visible(probe: &ReflectionProbe, layers: u64) -> bool {
    probe.debug_view && probe.layers & layers != 0
}

#[derive(PartialEq)]
struct ProbeGeometry {
    min: [f32; 3],
    max: [f32; 3],
    position: [f32; 3],
}

#[derive(Default)]
pub(super) struct ProbeDebug {
    shader: Option<wgpu::ShaderModule>,
    pipelines: HashMap<(wgpu::TextureFormat, bool, bool), wgpu::RenderPipeline>,
    geometry: Option<(Vec<ProbeGeometry>, wgpu::Buffer)>,
}

impl ProbeDebug {
    pub fn draw(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        probes: &[ReflectionProbe],
        camera: SceneCamera,
        depth: &FocusDepth,
        jitter: [f32; 2],
        region: super::CaptureRegion,
    ) {
        let geometry: Vec<_> = probes
            .iter()
            .filter(|probe| visible(probe, camera.layer_mask()))
            .map(|probe| ProbeGeometry {
                min: probe.min,
                max: probe.max,
                position: probe.position,
            })
            .collect();
        if geometry.is_empty() {
            return;
        }
        if self
            .geometry
            .as_ref()
            .is_none_or(|(cached, _)| *cached != geometry)
        {
            let mut bytes = Vec::with_capacity(36 * geometry.len());
            for probe in &geometry {
                append_floats(
                    &mut bytes,
                    [probe.min, probe.max, probe.position].into_iter().flatten(),
                );
            }
            self.geometry = Some((
                geometry,
                buffer(
                    ctx.device,
                    "XRack reflection probe guides",
                    &bytes,
                    wgpu::BufferUsages::VERTEX,
                ),
            ));
        }
        let Some((geometry, vertices)) = &self.geometry else {
            return;
        };

        let mut bytes = Vec::with_capacity(80);
        append_floats(
            &mut bytes,
            camera
                .clip_rows()
                .into_iter()
                .flatten()
                .chain(ctx.size.map(|side| 1.0 / side as f32))
                .chain([jitter[0] * 0.5, -jitter[1] * 0.5]),
        );
        let globals = buffer(
            ctx.device,
            "XRack probe guide camera",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let multisampled = depth.multisampled;
        let reverse_depth = depth.camera.reverse_depth();
        let depth_binding = if multisampled { 2 } else { 1 };
        let pipeline = self.pipelines.entry((ctx.format, multisampled, reverse_depth)).or_insert_with(|| {
            let shader = self.shader.get_or_insert_with(|| {
                ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("XRack reflection probe guides"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("probe_debug.wgsl").into()),
                })
            });
            ctx.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack reflection probe guides"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 36,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(if multisampled { "multisampled" } else { "single_sample" }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("REVERSE_DEVICE_DEPTH", f64::from(reverse_depth))],
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: ctx.format,
                        blend: None,
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
            label: Some("XRack reflection probe guide inputs"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: depth_binding,
                    resource: wgpu::BindingResource::TextureView(&depth.view),
                },
            ],
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            ctx.encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack reflection probe guide overlay"),
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
            },
        );
        region.clip(&mut pass);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..30, 0..geometry.len() as u32);
    }
}
