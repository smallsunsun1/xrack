use std::collections::VecDeque;

use crate::{GpuPaintContext, transient_textures::create_render_texture};
use xrack_runtime::Node;

use super::{COLOR_FORMAT, HDR_FORMAT, append_floats, buffer, postprocess::RETAINED_VIEW_SIZES};

const OUTPUTS: [wgpu::TextureFormat; 3] =
    [COLOR_FORMAT, HDR_FORMAT, wgpu::TextureFormat::Rgba32Float];

pub(super) struct FxaaSettings {
    thresholds: [f32; 2],
    subpixel: f32,
    strength: f32,
    steps: u8,
}

impl FxaaSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("fxaa", false) {
            return None;
        }
        let finite = |name, default| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
        };
        let strength = finite("fxaa-strength", 1.0).clamp(0.0, 1.0);
        (strength > 0.0).then(|| Self {
            thresholds: [
                finite("fxaa-edge-threshold", 0.125).clamp(0.03125, 0.5),
                finite("fxaa-min-threshold", 0.03125).clamp(1.0 / 255.0, 0.25),
            ],
            subpixel: finite("fxaa-subpixel", 0.75).clamp(0.0, 1.0),
            strength,
            steps: node.int("fxaa-search-steps").unwrap_or(6).clamp(2, 8) as u8,
        })
    }
}

struct Input {
    size: [u32; 2],
    view: wgpu::TextureView,
}

#[derive(Default)]
pub(super) struct Fxaa {
    inputs: VecDeque<Input>,
    shader: Option<wgpu::ShaderModule>,
    pipelines: [Option<wgpu::RenderPipeline>; 3],
    sampler: Option<wgpu::Sampler>,
}

impl Fxaa {
    pub fn input_target(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> (wgpu::TextureView, bool) {
        let index = self
            .inputs
            .iter()
            .position(|input| input.size == size && input.view.texture().format() == format);
        let existing = index.and_then(|index| self.inputs.remove(index));
        let allocated = existing.is_none();
        let input = existing.unwrap_or_else(|| Input {
            size,
            view: create_render_texture(device, size, format, "XRack FXAA color input")
                .create_view(&Default::default()),
        });
        let view = input.view.clone();
        self.inputs.truncate(RETAINED_VIEW_SIZES - 1);
        self.inputs.push_front(input);
        (view, allocated)
    }

    pub fn resolve(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        input: &wgpu::TextureView,
        settings: &FxaaSettings,
        dither: bool,
    ) {
        let shader = self.shader.get_or_insert_with(|| {
            ctx.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("XRack 3D FXAA"),
                    source: wgpu::ShaderSource::Wgsl(
                        concat!(
                            include_str!("display_color.wgsl"),
                            "\n",
                            include_str!("fxaa.wgsl")
                        )
                        .into(),
                    ),
                })
        });
        let slot = OUTPUTS
            .iter()
            .position(|&format| format == ctx.format)
            .unwrap_or(0);
        let pipeline = self.pipelines[slot].get_or_insert_with(|| {
            ctx.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("XRack 3D FXAA"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some("fragment"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &[("DISPLAY_SIGNAL", if slot == 0 { 1.0 } else { 0.0 })],
                            ..Default::default()
                        },
                        targets: &[Some(wgpu::ColorTargetState {
                            format: OUTPUTS[slot],
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        });
        let sampler = self.sampler.get_or_insert_with(|| {
            ctx.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D FXAA sampler"),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        });
        let mut bytes = Vec::with_capacity(32);
        append_floats(
            &mut bytes,
            [
                ctx.size[0] as f32,
                ctx.size[1] as f32,
                f32::from(settings.steps),
                f32::from(u8::from(dither)),
            ],
        );
        append_floats(
            &mut bytes,
            [
                settings.thresholds[0],
                settings.thresholds[1],
                settings.subpixel,
                settings.strength,
            ],
        );
        let uniform = buffer(
            ctx.device,
            "XRack 3D FXAA settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let bindings = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D FXAA input bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            ctx.encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack 3D FXAA"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            },
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
}
