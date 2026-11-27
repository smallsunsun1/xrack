//! Blend stationary subpixel samples in scene-linear space.
use super::HDR_FORMAT;

pub(crate) struct ProgressiveSample {
    pub history: wgpu::TextureView,
    pub index: u8,
    pub offset: [f32; 2],
}

impl ProgressiveSample {
    pub fn new(history: wgpu::TextureView, index: u8, samples: u8) -> Self {
        Self {
            history,
            index,
            offset: sample_offset(index, samples),
        }
    }
}

pub(super) fn sample_offset(index: u8, samples: u8) -> [f32; 2] {
    // Rotated, balanced subpixel grids. Each pair has zero mean so partial
    // results converge around the original camera as well.
    let pattern: &[[f32; 2]] = match samples {
        2 => &[[-0.25, -0.25], [0.25, 0.25]],
        4 => &[
            [-0.375, -0.125],
            [0.375, 0.125],
            [0.125, -0.375],
            [-0.125, 0.375],
        ],
        _ => &[
            [-0.4375, 0.0625],
            [0.4375, -0.0625],
            [0.0625, 0.4375],
            [-0.0625, -0.4375],
            [-0.1875, -0.3125],
            [0.1875, 0.3125],
            [0.3125, -0.1875],
            [-0.3125, 0.1875],
        ],
    };
    pattern[usize::from(index)]
}

pub(super) struct Accumulator {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Accumulator {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack progressive 3D AA"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../frame/pixel_blit.wgsl").into()),
        });
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack progressive 3D AA"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: blend,
                        alpha: blend,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let layout = pipeline.get_bind_group_layout(0);
        Self { pipeline, layout }
    }

    pub fn blend(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sample: &ProgressiveSample,
    ) {
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack progressive 3D AA input"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack progressive 3D AA blend"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sample.history,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if sample.index == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            },
        );
        let weight = f64::from(sample.index + 1).recip();
        pass.set_pipeline(&self.pipeline);
        pass.set_blend_constant(wgpu::Color {
            r: weight,
            g: weight,
            b: weight,
            a: weight,
        });
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
}
