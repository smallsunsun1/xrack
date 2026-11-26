use std::collections::VecDeque;

use xrack_runtime::{ImageAsset, Node};

use super::{
    Attachment, HDR_FORMAT, append_floats, attachment_view,
    bloom::HighlightSettings,
    buffer,
    lut::LutCache,
    postprocess::{DisplayLeases, RETAINED_VIEW_SIZES},
};

pub(super) struct FlareSettings {
    pub strength: f32,
    pub source: HighlightSettings,
    optics: [f32; 8],
    maps: [Option<ImageAsset>; 3],
}

impl FlareSettings {
    pub(super) fn strength(node: &Node) -> Option<f32> {
        let strength = node
            .float("lens-flare-strength")
            .filter(|value| value.is_finite())
            .unwrap_or(0.65)
            .clamp(0.0, 4.0);
        (node.boolean("lens-flare", false) && strength > 0.0).then_some(strength)
    }

    pub fn from_node(node: &Node) -> Option<Self> {
        let strength = Self::strength(node)?;
        let finite = |name, default| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
        };
        let ghosts = node.int("lens-flare-ghost-count").unwrap_or(4).clamp(0, 20);
        let halo_width = finite("lens-flare-halo-width", 0.30).clamp(0.0, 1.0);
        let halo_strength = if halo_width > 0.0 {
            finite("lens-flare-halo-strength", 0.65).clamp(0.0, 2.0)
        } else {
            0.0
        };
        if ghosts == 0 && halo_strength == 0.0 {
            return None;
        }
        let (sin, cos) = finite("lens-flare-rotation", 0.0)
            .rem_euclid(360.0)
            .to_radians()
            .sin_cos();
        Some(Self {
            strength,
            source: HighlightSettings {
                threshold: finite("lens-flare-threshold", 1.0).clamp(0.0, 65_000.0),
                levels: node.int("lens-flare-blur-levels").unwrap_or(3).clamp(1, 6) as usize,
            },
            optics: [
                ghosts as f32,
                finite("lens-flare-ghost-dispersal", 0.42).clamp(0.0, 5.0),
                halo_width,
                halo_strength,
                finite("lens-flare-distortion", 1.0).clamp(0.0, 8.0),
                f32::from(u8::from(node.boolean("lens-flare-aspect-corrected", true))),
                sin,
                cos,
            ],
            maps: [
                "lens-flare-color-map",
                "lens-flare-dirt-map",
                "lens-flare-starburst-map",
            ]
            .map(|name| {
                node.image_asset(name).filter(|asset| {
                    asset.pixels().is_some_and(|pixels| {
                        pixels
                            .size
                            .into_iter()
                            .all(|side| (1..=1024).contains(&side))
                    })
                })
            }),
        })
    }
}

pub(super) struct FlareOutput {
    pub view: wgpu::TextureView,
    pub pixels: u64,
    pub allocations: u64,
    pub uploads: u64,
}

pub(super) struct LensFlare {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    images: LutCache,
    targets: VecDeque<Attachment>,
}

impl LensFlare {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D lens flare"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lens_flare.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack 3D lens flare"),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let white = attachment_view(
            device,
            [1, 1],
            wgpu::TextureFormat::Rgba8Unorm,
            1,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            "XRack neutral lens flare map",
        );
        queue.write_texture(
            white.texture().as_image_copy(),
            &[255; 4],
            Default::default(),
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D lens flare sampler"),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            white,
            images: LutCache::new(device),
            targets: VecDeque::new(),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        highlights: &wgpu::TextureView,
        size: [u32; 2],
        settings: &FlareSettings,
        leases: &DisplayLeases,
    ) -> FlareOutput {
        let reduced = size.map(|side| side.div_ceil(4));
        let index = self
            .targets
            .iter()
            .position(|target| target.size == reduced && !leases.contains(&target.view));
        let cached = index.and_then(|index| self.targets.remove(index));
        let allocations = u64::from(cached.is_none());
        let target = cached.unwrap_or_else(|| Attachment {
            size: reduced,
            view: attachment_view(
                device,
                reduced,
                HDR_FORMAT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "XRack quarter-resolution lens flare",
            ),
        });
        let maps = settings.maps.each_ref().map(|map| {
            map.as_ref()
                .and_then(|asset| self.images.image(device, queue, asset))
        });
        let mut bytes = Vec::with_capacity(64);
        append_floats(&mut bytes, settings.optics);
        append_floats(
            &mut bytes,
            [
                reduced[0] as f32,
                reduced[1] as f32,
                size[0] as f32 / size[1] as f32,
                0.0,
            ],
        );
        append_floats(
            &mut bytes,
            std::array::from_fn::<_, 4, _>(|index| {
                f32::from(u8::from(maps.get(index).is_some_and(Option::is_some)))
            }),
        );
        let uniform = buffer(
            device,
            "XRack lens flare settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let view_entry = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let [color, dirt, starburst] = maps
            .each_ref()
            .map(|map| map.as_ref().map_or(&self.white, |image| &image.view));
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D lens flare inputs"),
            layout: &self.layout,
            entries: &[
                view_entry(0, highlights),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                view_entry(3, color),
                view_entry(4, dirt),
                view_entry(5, starburst),
            ],
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack 3D lens flare"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        let view = target.view.clone();
        self.targets.truncate(RETAINED_VIEW_SIZES - 1);
        self.targets.push_front(target);
        FlareOutput {
            view,
            pixels: reduced.into_iter().map(u64::from).product(),
            allocations,
            uploads: maps
                .into_iter()
                .flatten()
                .filter(|image| image.uploaded)
                .count() as u64,
        }
    }
}
