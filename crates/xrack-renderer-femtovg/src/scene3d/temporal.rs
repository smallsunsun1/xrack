//! Scene-linear temporal filtering with raw or motion-reprojected history.
use std::{cell::RefCell, rc::Rc};

use super::{
    Attachment, HDR_FORMAT, attachment_view,
    motion::{MotionHistory, MotionInputs},
    progressive::sample_offset,
};

const USAGE: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::TEXTURE_BINDING);

#[derive(Clone, Copy, PartialEq)]
pub(super) struct TemporalSettings {
    pub strength: f32,
    pub motion_vectors: bool,
}

#[derive(Default)]
pub(super) struct TemporalHistory {
    last: Option<wgpu::TextureView>,
    spare: Option<wgpu::TextureView>,
    index: u8,
    motion: Option<Rc<RefCell<MotionHistory>>>,
}

pub(crate) struct TemporalSample {
    pub previous: Option<wgpu::TextureView>,
    pub current: wgpu::TextureView,
    pub offset: [f32; 2],
    pub(super) motion: Option<Rc<RefCell<MotionHistory>>>,
}

impl TemporalHistory {
    pub fn sample(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        settings: TemporalSettings,
    ) -> TemporalSample {
        let current = self.spare.take().unwrap_or_else(|| {
            attachment_view(
                device,
                size,
                HDR_FORMAT,
                1,
                USAGE,
                "XRack temporal AA raw sample",
            )
        });
        let previous = self.last.replace(current.clone());
        self.spare = previous.clone();
        let offset = if settings.motion_vectors {
            sample_offset(self.index, 8).map(|position| position * 2.0 * settings.strength)
        } else {
            let shift = if self.index % 2 == 1 {
                settings.strength
            } else {
                -settings.strength
            };
            [shift, shift]
        };
        self.index = (self.index + 1) % 8;
        let motion = settings
            .motion_vectors
            .then(|| self.motion.get_or_insert_with(Default::default).clone());
        TemporalSample {
            previous,
            current,
            offset,
            motion,
        }
    }
}

#[derive(Default)]
pub(super) struct TemporalFilter {
    pipelines: [Option<TemporalPipeline>; 3],
    result: Option<Attachment>,
}

impl TemporalFilter {
    pub fn resolve(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        (sample, motion): (&TemporalSample, Option<&MotionInputs>),
        size: [u32; 2],
    ) -> wgpu::TextureView {
        let mode = if motion.is_some() {
            TemporalVariant::Reproject
        } else if sample.motion.is_none() && sample.previous.is_some() {
            TemporalVariant::Blend
        } else {
            TemporalVariant::Initial
        };
        let blend = mode != TemporalVariant::Initial;
        let pipeline = self.pipelines[mode as usize]
            .get_or_insert_with(|| TemporalPipeline::new(device, mode));
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let color = texture(0, source);
        let history = texture(1, sample.previous.as_ref().unwrap_or(source));

        let bindings = if let Some(motion) = motion {
            pipeline.bind(
                device,
                &[
                    color,
                    history,
                    texture(2, &motion.vectors),
                    texture(3, &motion.ids),
                    texture(4, &motion.previous_ids),
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: motion.instances.as_entire_binding(),
                    },
                ],
            )
        } else {
            pipeline.bind(device, &[color, history][..1 + usize::from(blend)])
        };
        let attachment = |view| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        };
        let filtered =
            blend.then(|| Attachment::get(&mut self.result, device, size, HDR_FORMAT, 1, true));
        let colors = [Some(attachment(&sample.current)), filtered.map(attachment)];
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack temporal AA"),
                color_attachments: &colors[..1 + usize::from(blend)],
                ..Default::default()
            },
        );
        pass.set_pipeline(&pipeline.render);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        filtered.unwrap_or(&sample.current).clone()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TemporalVariant {
    Initial,
    Blend,
    Reproject,
}

struct TemporalPipeline {
    render: wgpu::RenderPipeline,
    inputs: wgpu::BindGroupLayout,
}

impl TemporalPipeline {
    fn bind(&self, device: &wgpu::Device, entries: &[wgpu::BindGroupEntry<'_>]) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack temporal AA inputs"),
            layout: &self.inputs,
            entries,
        })
    }

    fn new(device: &wgpu::Device, mode: TemporalVariant) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack temporal AA"),
            source: wgpu::ShaderSource::Wgsl(include_str!("temporal.wgsl").into()),
        });
        let targets = [(); 2].map(|()| {
            Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        });
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack temporal AA"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(match mode {
                    TemporalVariant::Initial => "initial",
                    TemporalVariant::Blend => "blend",
                    TemporalVariant::Reproject => "reproject",
                }),
                compilation_options: Default::default(),
                targets: &targets[..1 + usize::from(mode != TemporalVariant::Initial)],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let inputs = render.get_bind_group_layout(0);
        Self { render, inputs }
    }
}
