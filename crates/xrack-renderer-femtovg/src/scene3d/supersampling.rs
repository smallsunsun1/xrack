//! Offscreen supersampling and a linear-light area resolve.
use std::collections::HashMap;

use xrack_runtime::Node;

use super::{COLOR_FORMAT, HDR_FORMAT, buffer};

/// Limit the additional raster area, including at high device pixel ratios.
const EXTRA_PIXELS: f64 = 4_194_304.0;

/// The composited scene texture, before the optional supersampling resolve.
pub(crate) fn texture_extent(node: &Node, nominal: [u32; 2], limit: u32) -> [u32; 2] {
    let scale = node.number("render-scale").unwrap_or(1.0).clamp(0.125, 1.0);
    let requested = [node.int("texture-width"), node.int("texture-height")];
    if scale == 1.0
        && requested
            .iter()
            .all(|side| side.is_none_or(|side| side <= 0))
    {
        return nominal;
    }
    let extent: [f64; 2] = std::array::from_fn(|axis| {
        let side = requested[axis].filter(|&side| side > 0).map_or_else(
            || (f64::from(nominal[axis]) * scale).ceil(),
            |side| side as f64,
        );
        side.clamp(1.0, f64::from(limit))
    });
    let nominal_pixels = nominal.map(f64::from).into_iter().product::<f64>();
    let area = (nominal_pixels + EXTRA_PIXELS).min(crate::MAX_RENDER_PIXELS as f64);
    let scale = (area / (extent[0] * extent[1])).sqrt().min(1.0);
    extent.map(|side| (side * scale).floor().max(1.0) as u32)
}

pub(super) fn extent(device: &wgpu::Device, size: [u32; 2], requested: f32) -> [u32; 2] {
    if !requested.is_finite() || requested <= 1.0 {
        return size;
    }
    let [width, height] = size.map(f64::from);
    let pixels = width * height;
    let area_limit =
        (((pixels + EXTRA_PIXELS).min(crate::MAX_RENDER_PIXELS as f64)) / pixels).sqrt();
    let device_limit = f64::from(device.limits().max_texture_dimension_2d) / width.max(height);
    let scale = f64::from(requested)
        .min(2.0)
        .min(area_limit)
        .min(device_limit);
    size.map(|side| (f64::from(side) * scale.max(1.0)).floor() as u32)
}

pub(super) struct Supersampling {
    shader: wgpu::ShaderModule,
    pipelines: [Option<wgpu::RenderPipeline>; 3],
    capture_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl Supersampling {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            shader: device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("XRack 3D supersampling"),
                source: wgpu::ShaderSource::Wgsl(include_str!("supersampling.wgsl").into()),
            }),
            pipelines: [None, None, None],
            capture_pipelines: HashMap::new(),
        }
    }

    pub fn resolve(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (source, target): (&wgpu::TextureView, &wgpu::TextureView),
        size: [u32; 2],
    ) {
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&size[0].to_ne_bytes());
        bytes[4..].copy_from_slice(&size[1].to_ne_bytes());
        let dimensions = buffer(
            device,
            "XRack 3D supersampling output dimensions",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let pipeline = self.pipeline(device, target.texture().format(), false);
        Self::draw(
            device,
            encoder,
            (source, target),
            pipeline,
            Some(&dimensions),
        );
    }

    pub fn decode_item(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        views: (&wgpu::TextureView, &wgpu::TextureView),
    ) {
        let pipeline = self.pipeline(device, COLOR_FORMAT, true);
        Self::draw(device, encoder, views, pipeline, None);
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        decode_item: bool,
    ) -> &wgpu::RenderPipeline {
        let create = || {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack 3D supersampling resolve"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(if decode_item {
                        "decode_item"
                    } else {
                        "fragment"
                    }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
        };
        let index = match (decode_item, format) {
            (true, _) => 2,
            (false, COLOR_FORMAT) => 0,
            (false, HDR_FORMAT) => 1,
            (false, format) => return self.capture_pipelines.entry(format).or_insert_with(create),
        };
        self.pipelines[index].get_or_insert_with(create)
    }

    fn draw(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (source, target): (&wgpu::TextureView, &wgpu::TextureView),
        pipeline: &wgpu::RenderPipeline,
        dimensions: Option<&wgpu::Buffer>,
    ) {
        let source = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        };
        let mut inputs = [source.clone(), source];
        let count = if let Some(dimensions) = dimensions {
            inputs[1] = wgpu::BindGroupEntry {
                binding: 1,
                resource: dimensions.as_entire_binding(),
            };
            2
        } else {
            1
        };
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D area filter input"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &inputs[..count],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D supersampling resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
}
