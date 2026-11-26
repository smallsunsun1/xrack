//! Weighted blended transparency with per-sample composition when MSAA is active.
use std::collections::HashMap;

use super::{
    DEPTH_FORMAT, attachment_view,
    linked_transparency::{LinkedFrame, LinkedTransparency},
    pipeline::ColorOutput,
};

pub(super) const ACCUMULATION: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(super) const REVEALAGE: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

#[derive(Default)]
pub(super) struct Transparency {
    modes: [Option<Mode>; 2],
    linked: LinkedTransparency,
}

struct Mode {
    layout: wgpu::BindGroupLayout,
    pipelines: [Option<wgpu::RenderPipeline>; 2],
    capture_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    layers: Option<Layers>,
}

struct Layers {
    size: [u32; 2],
    depth: Option<wgpu::TextureView>,
    sampled_depth: Option<wgpu::TextureView>,
    colors: [wgpu::TextureView; 2],
    inputs: [Option<wgpu::BindGroup>; 2],
    multisampled_color: [Option<wgpu::TextureView>; 2],
}

pub(super) struct TransparencyFrame<'a> {
    pub depth: &'a wgpu::TextureView,
    pub multisampled_color: Option<&'a wgpu::TextureView>,
    pub linked: Option<LinkedFrame<'a>>,
    colors: &'a [wgpu::TextureView; 2],
    inputs: &'a wgpu::BindGroup,
    composite: &'a wgpu::RenderPipeline,
}

impl Transparency {
    pub fn prepare<'a>(
        &'a mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        output: ColorOutput,
        (retain_depth, linked_region): (bool, Option<[u32; 4]>),
        depth_override: Option<&'a wgpu::TextureView>,
    ) -> TransparencyFrame<'a> {
        let mode = self.modes[usize::from(output.multisampling)]
            .get_or_insert_with(|| Mode::new(device, output.multisampling));
        let hdr = usize::from(output.hdr);
        let create = || {
            composite_pipeline(
                device,
                &mode.layout,
                output.format(),
                output.multisampling,
                false,
            )
        };
        let pipeline = if let Some(format) = output.format_override {
            mode.capture_pipelines.entry(format).or_insert_with(create)
        } else {
            mode.pipelines[hdr].get_or_insert_with(create)
        };
        if mode
            .layers
            .as_ref()
            .is_some_and(|layers| layers.size != size)
        {
            mode.layers = None;
            self.linked.invalidate_bindings(output.multisampling);
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
                depth: None,
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
        let linked = linked_region.and_then(|region| {
            self.linked.prepare(
                device,
                (size, region),
                output.format(),
                &layers.colors,
                multisampled_color,
            )
        });
        let depth = if let Some(depth) = depth_override {
            depth
        } else if retain_depth || linked.is_some() {
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
            layers.depth.get_or_insert_with(|| {
                attachment_view(
                    device,
                    size,
                    DEPTH_FORMAT,
                    output.samples(),
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                    "XRack 3D depth shared with transparency",
                )
            })
        };
        TransparencyFrame {
            depth,
            multisampled_color,
            linked,
            colors: &layers.colors,
            inputs,
            composite: pipeline,
        }
    }
}

impl Mode {
    fn new(device: &wgpu::Device, multisampled: bool) -> Self {
        let entries: Vec<_> = composite_textures(multisampled).collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack 3D transparency composite layout"),
            entries: &entries,
        });
        Self {
            layout,
            pipelines: [None, None],
            capture_pipelines: HashMap::new(),
            layers: None,
        }
    }
}

pub(super) fn composite_textures(
    multisampled: bool,
) -> impl Iterator<Item = wgpu::BindGroupLayoutEntry> {
    let first = first_binding(multisampled);
    (first..first + if multisampled { 3 } else { 2 }).map(move |binding| {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        }
    })
}

pub(super) fn first_binding(multisampled: bool) -> u32 {
    2 * u32::from(multisampled)
}

impl TransparencyFrame<'_> {
    pub fn accumulate<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
        retain_depth: bool,
        stencil_ops: Option<wgpu::Operations<u32>>,
    ) -> wgpu::RenderPass<'a> {
        crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
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
                    depth_ops: self.linked.is_none().then_some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: if retain_depth {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops,
                }),
                ..Default::default()
            },
        )
    }

    pub fn composite(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
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
            },
        );
        if let Some(linked) = &self.linked {
            linked.resolve(&mut pass);
        } else {
            pass.set_pipeline(self.composite);
            pass.set_bind_group(0, self.inputs, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub(super) fn composite_pipeline(
    device: &wgpu::Device,
    inputs: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    multisampling: bool,
    linked: bool,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("XRack 3D transparency composite pipeline layout"),
        bind_group_layouts: &[Some(inputs)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack 3D transparency composite"),
        source: wgpu::ShaderSource::Wgsl(if linked {
            concat!(
                include_str!("transparency.wgsl"),
                "\n",
                include_str!("linked_resolve.wgsl")
            )
            .into()
        } else {
            include_str!("transparency.wgsl").into()
        }),
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
            entry_point: Some(match (linked, multisampling) {
                (true, true) => "fragment_linked_multisampled",
                (true, false) => "fragment_linked",
                (false, true) => "fragment_multisampled",
                (false, false) => "fragment",
            }),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: (!multisampling).then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
