//! Weighted blended transparency with per-sample composition when MSAA is active.
use super::{DEPTH_FORMAT, attachment_view, pipeline::ColorOutput};

pub(super) const ACCUMULATION: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(super) const REVEALAGE: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

#[derive(Default)]
pub(super) struct Transparency {
    modes: [Option<Mode>; 2],
}

struct Mode {
    layout: wgpu::BindGroupLayout,
    pipelines: [Option<wgpu::RenderPipeline>; 2],
    layers: Option<Layers>,
}

struct Layers {
    size: [u32; 2],
    depth: wgpu::TextureView,
    sampled_depth: Option<wgpu::TextureView>,
    colors: [wgpu::TextureView; 2],
    inputs: [Option<wgpu::BindGroup>; 2],
    multisampled_color: [Option<wgpu::TextureView>; 2],
}

pub(super) struct TransparencyFrame<'a> {
    pub depth: &'a wgpu::TextureView,
    pub multisampled_color: Option<&'a wgpu::TextureView>,
    colors: &'a [wgpu::TextureView; 2],
    inputs: &'a wgpu::BindGroup,
    composite: &'a wgpu::RenderPipeline,
}

impl Transparency {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        output: ColorOutput,
        retain_depth: bool,
    ) -> TransparencyFrame<'_> {
        let mode = self.modes[usize::from(output.multisampling)]
            .get_or_insert_with(|| Mode::new(device, output.multisampling));
        let hdr = usize::from(output.hdr);
        let pipeline = mode.pipelines[hdr]
            .get_or_insert_with(|| composite_pipeline(device, &mode.layout, output));
        if mode
            .layers
            .as_ref()
            .is_some_and(|layers| layers.size != size)
        {
            mode.layers = None;
        }
        let layers = mode.layers.get_or_insert_with(|| {
            let samples = output.samples();
            let colors = [ACCUMULATION, REVEALAGE].map(|format| {
                attachment_view(
                    device,
                    size,
                    format,
                    samples,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack 3D transparency layers",
                )
            });
            Layers {
                size,
                depth: attachment_view(
                    device,
                    size,
                    DEPTH_FORMAT,
                    samples,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                    "XRack 3D depth shared with transparency",
                ),
                sampled_depth: None,
                colors,
                inputs: [None, None],
                multisampled_color: [None, None],
            }
        });
        let multisampled_color = output.multisampling.then(|| {
            &*layers.multisampled_color[hdr].get_or_insert_with(|| {
                attachment_view(
                    device,
                    size,
                    output.format(),
                    output.samples(),
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack 3D multisampled color before transparency composite",
                )
            })
        });
        let input_slot = if output.multisampling { hdr } else { 0 };
        let inputs = layers.inputs[input_slot].get_or_insert_with(|| {
            let first = first_binding(output.multisampling);
            let entries: Vec<_> = layers
                .colors
                .iter()
                .chain(multisampled_color)
                .enumerate()
                .map(|(index, view)| wgpu::BindGroupEntry {
                    binding: first + index as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack 3D transparency composite inputs"),
                layout: &mode.layout,
                entries: &entries,
            })
        });
        let depth = if retain_depth {
            layers.sampled_depth.get_or_insert_with(|| {
                attachment_view(
                    device,
                    size,
                    DEPTH_FORMAT,
                    output.samples(),
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack 3D depth shared with transparency and depth of field",
                )
            })
        } else {
            &layers.depth
        };
        TransparencyFrame {
            depth,
            multisampled_color,
            colors: &layers.colors,
            inputs,
            composite: pipeline,
        }
    }
}

impl Mode {
    fn new(device: &wgpu::Device, multisampled: bool) -> Self {
        let first = first_binding(multisampled);
        let entries: Vec<_> = (first..first + if multisampled { 3 } else { 2 })
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack 3D transparency composite layout"),
            entries: &entries,
        });
        Self {
            layout,
            pipelines: [None, None],
            layers: None,
        }
    }
}

fn first_binding(multisampled: bool) -> u32 {
    2 * u32::from(multisampled)
}

impl TransparencyFrame<'_> {
    pub fn accumulate<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
        retain_depth: bool,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D transparent accumulation"),
            color_attachments: &[0, 1].map(|index| {
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.colors[index],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if index == 0 {
                            wgpu::Color::TRANSPARENT
                        } else {
                            wgpu::Color::WHITE
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })
            }),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: if retain_depth {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        })
    }

    pub fn composite(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D transparency composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if self.multisampled_color.is_some() {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(self.composite);
        pass.set_bind_group(0, self.inputs, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn composite_pipeline(
    device: &wgpu::Device,
    inputs: &wgpu::BindGroupLayout,
    output: ColorOutput,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("XRack 3D transparency composite pipeline layout"),
        bind_group_layouts: &[Some(inputs)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack 3D transparency composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("transparency.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack 3D transparency composite"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(if output.multisampling {
                "fragment_multisampled"
            } else {
                "fragment"
            }),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: output.format(),
                blend: (!output.multisampling)
                    .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
