use crate::{PaintRegistration, register_gpu};
use std::{borrow::Cow, cell::RefCell};
use wgpu::util::DeviceExt;

const INTERFACE: &str = r#"
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@group(0) @binding(0) var<uniform> xrack_frame: vec4<f32>;

@vertex fn xrack_vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = array<vec2<f32>, 3>(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0))[index];
    return Vertex(vec4(corner * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0), corner);
}

@fragment fn xrack_fragment(input: Vertex) -> @location(0) vec4<f32> {
    let color = shade(input.uv, xrack_frame);
    return vec4(color.rgb * color.a, color.a);
}
"#;

struct Pipeline {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    result: Result<wgpu::RenderPipeline, String>,
}

/// Register a WGSL function `shade(uv, frame)` for `CustomRender`.
/// UV is in local element coordinates, normalized to 0–1. Frame inputs are
/// target width and height in pixels, scale, and the element revision.
/// Return a straight-alpha RGBA color.
pub fn register_shader_effect(
    name: impl Into<String>,
    wgsl: &str,
) -> Result<PaintRegistration, String> {
    let source = format!("{INTERFACE}\n{wgsl}");
    let cached = RefCell::new(None::<Pipeline>);
    register_gpu(name, move |ctx| {
        let mut cache = cached.borrow_mut();
        if !cache
            .as_ref()
            .is_some_and(|item| item.device == *ctx.device && item.format == ctx.format)
        {
            *cache = Some(Pipeline {
                device: ctx.device.clone(),
                format: ctx.format,
                result: make_pipeline(ctx.device, ctx.format, &source),
            });
        }
        let item = cache
            .as_ref()
            .ok_or("shader pipeline was not initialized")?;
        let pipeline = item.result.as_ref().map_err(Clone::clone)?;
        let params = [
            ctx.size[0] as f32,
            ctx.size[1] as f32,
            ctx.paint.scale,
            ctx.paint.revision as f32,
        ]
        .map(f32::to_ne_bytes)
        .concat();
        let buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("XRack shader parameters"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack shader resources"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack shader effect"),
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
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    source: &str,
) -> Result<wgpu::RenderPipeline, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack shader effect"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack shader effect"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("xrack_vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("xrack_fragment"),
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
    });
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(format!("invalid WGSL shader effect: {error}"));
    }
    Ok(pipeline)
}
