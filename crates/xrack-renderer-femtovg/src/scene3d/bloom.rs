//! A linear HDR bloom pyramid. The source has already resolved depth, MSAA and transparency.
use std::collections::VecDeque;

use xrack_runtime::{Node, Value};

use super::{
    HDR_FORMAT, append_floats, attachment_view, buffer,
    postprocess::{DisplayLeases, RETAINED_VIEW_SIZES},
};

const MAX_LEVELS: i64 = 6;

#[derive(Clone, Copy)]
pub(super) struct BloomSettings {
    pub strength: f32,
    pub source: HighlightSettings,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct HighlightSettings {
    pub threshold: f32,
    pub levels: usize,
}

impl BloomSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        if !node.boolean("bloom", false) {
            return None;
        }
        let strength = node.float("bloom-strength").unwrap_or(0.45).clamp(0.0, 4.0);
        let threshold = node.float("bloom-threshold").unwrap_or(1.0).max(0.0);
        let levels = match node.properties.get("bloom-levels") {
            Some(Value::Int(levels)) => *levels,
            _ => 5,
        };
        (strength > 0.0).then_some(Self {
            strength,
            source: HighlightSettings {
                threshold,
                levels: levels.clamp(1, MAX_LEVELS) as usize,
            },
        })
    }
}

pub(super) struct BloomOutput {
    pub view: wgpu::TextureView,
    pub passes: u64,
    pub pixels: u64,
    pub allocations: u64,
}

struct Level {
    size: [u32; 2],
    view: wgpu::TextureView,
    reduce_input: wgpu::BindGroup,
    expand_input: wgpu::BindGroup,
}

struct Pyramid {
    size: [u32; 2],
    levels: Vec<Level>,
    source_binding: Option<(wgpu::TextureView, f32, wgpu::BindGroup)>,
}

pub(super) struct Bloom {
    pyramids: VecDeque<Pyramid>,
    sampler: wgpu::Sampler,
    extract: Filter,
    reduce: Filter,
    expand: Filter,
}

impl Bloom {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        Self {
            pyramids: VecDeque::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D bloom sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            extract: Filter::new(device, &shader, "extract", false),
            reduce: Filter::new(device, &shader, "reduce", false),
            expand: Filter::new(device, &shader, "expand", true),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        size: [u32; 2],
        settings: HighlightSettings,
        (leases, protected): (&DisplayLeases, Option<&wgpu::TextureView>),
    ) -> Option<BloomOutput> {
        let index = self.pyramids.iter().position(|entry| {
            entry.size == size
                && entry.levels.first().is_none_or(|first| {
                    !leases.contains(&first.view) && protected != Some(&first.view)
                })
        });
        let cached = index
            .and_then(|index| self.pyramids.remove(index))
            .unwrap_or_else(|| Pyramid {
                size,
                levels: Vec::new(),
                source_binding: None,
            });
        self.pyramids.truncate(RETAINED_VIEW_SIZES - 1);
        self.pyramids.push_front(cached);
        let pyramid = self.pyramids.front_mut()?;
        let original_levels = pyramid.levels.len();
        while pyramid.levels.len() < settings.levels {
            let previous = pyramid.levels.last().map_or(size, |level| level.size);
            if !pyramid.levels.is_empty() && previous == [1, 1] {
                break;
            }
            let size = previous.map(|side| side.div_ceil(2));
            let view = attachment_view(
                device,
                size,
                HDR_FORMAT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "XRack 3D bloom level",
            );
            pyramid.levels.push(Level {
                size,
                reduce_input: self.reduce.bind(device, &view, &self.sampler, None),
                expand_input: self.expand.bind(device, &view, &self.sampler, None),
                view,
            });
        }
        if pyramid
            .source_binding
            .as_ref()
            .is_none_or(|(view, threshold, _)| view != source || *threshold != settings.threshold)
        {
            let mut bytes = Vec::with_capacity(16);
            append_floats(&mut bytes, [settings.threshold, 0.0, 0.0, 0.0]);
            let threshold = buffer(
                device,
                "XRack 3D bloom threshold",
                &bytes,
                wgpu::BufferUsages::UNIFORM,
            );
            let inputs = self
                .extract
                .bind(device, source, &self.sampler, Some(&threshold));
            pyramid.source_binding = Some((source.clone(), settings.threshold, inputs));
        }
        let (_, _, inputs) = pyramid.source_binding.as_ref()?;
        let active = &pyramid.levels[..pyramid.levels.len().min(settings.levels)];
        let first = active.first()?;
        self.extract.draw(encoder, &first.view, inputs);
        for levels in active.windows(2) {
            self.reduce
                .draw(encoder, &levels[1].view, &levels[0].reduce_input);
        }
        for levels in active.windows(2).rev() {
            self.expand
                .draw(encoder, &levels[0].view, &levels[1].expand_input);
        }
        let area = |level: &Level| level.size.into_iter().map(u64::from).product::<u64>();
        Some(BloomOutput {
            view: first.view.clone(),
            passes: (2 * active.len() - 1) as u64,
            allocations: (pyramid.levels.len() - original_levels) as u64,
            pixels: active.iter().map(area).sum::<u64>()
                + active.iter().rev().skip(1).map(area).sum::<u64>(),
        })
    }
}

struct Filter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    expand: bool,
}

impl Filter {
    fn new(device: &wgpu::Device, shader: &wgpu::ShaderModule, entry: &str, expand: bool) -> Self {
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack 3D bloom filter"),
            layout: None,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: expand.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
            expand,
        }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        threshold: Option<&wgpu::Buffer>,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ];
        if let Some(threshold) = threshold {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: threshold.as_entire_binding(),
            });
        }
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D bloom inputs"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        input: &wgpu::BindGroup,
    ) {
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack 3D bloom filter"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if self.expand {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            },
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }
}
