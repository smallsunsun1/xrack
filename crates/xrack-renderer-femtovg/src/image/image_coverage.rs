use crate::{CanvasTransform, canvas_transform};
use femtovg::renderer::WGPURenderer;
use femtovg::{Atlas, Canvas, ImageFlags, ImageId, ImageInfo, PixelFormat, Quad};
use std::rc::Rc;
use xrack_layout::Rect;

const EDGE: usize = 1024;
const MAX_TILES: usize = 1024;
const INSTANCE_BYTES: u64 = 68;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// Each instance samples one original PNG and writes straight RGBA with the
// clipped rectangle's coverage in alpha. The display list composites
// the result at device-pixel centers using the existing color-texture batch.
const SHADER: &str = r#"
override edge: f32;
@group(0) @binding(0) var images: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;

struct Vertex {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) origin: vec2f,
    @location(1) @interpolate(flat) paint: vec4f,
    @location(2) @interpolate(flat) uv: vec4f,
    @location(3) @interpolate(flat) source: vec4f,
    @location(4) @interpolate(flat) dpi: f32,
}

@vertex fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) tile: vec4f,
    @location(1) paint: vec4f,
    @location(2) uv: vec4f,
    @location(3) source: vec4f,
    @location(4) dpi: f32,
) -> Vertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    let pixel = tile.xy + corner * tile.zw;
    let ndc = pixel * vec2f(2.0, -2.0) / edge + vec2f(-1.0, 1.0);
    return Vertex(vec4f(ndc, 0.0, 1.0), tile.xy, paint, uv, source, dpi);
}

@fragment fn fragment(input: Vertex) -> @location(0) vec4f {
    let p = input.position.xy - input.origin;
    // FemtoVG convex fills extend a half-fringe to either side. At a
    // rectangle corner the miter interpolates the nearer edge's coverage.
    let fill = min(p - input.paint.xy, input.paint.zw - p);
    let aa = clamp(0.5 + min(fill.x, fill.y) * input.dpi, 0.0, 1.0);
    let uv = clamp(input.uv.xy + p * input.uv.zw, input.source.xy, input.source.zw);
    let rgba = textureSampleLevel(images, linear, uv, 0.0);
    return vec4f(rgba.rgb, rgba.a * aa);
}
"#;

pub(crate) struct Coverage {
    raster: Rect,
    paint: Rect,
    pattern: Rect,
    dpi: f32,
}

impl Coverage {
    pub(crate) fn new(
        transform: CanvasTransform,
        paint: Rect,
        clip: Rect,
        pattern: Rect,
        display_clip: Rect,
        dpi: f32,
    ) -> Option<Self> {
        let paint = transform.map_rect(paint);
        let clip = transform.map_rect(clip);
        let pattern = transform.map_rect(pattern);
        // FemtoVG's upright-rectangle fill clips the geometry and adds
        // antialiasing once around the intersection.
        let visible = paint.intersect(clip);
        let x = visible.x.floor();
        let y = visible.y.floor();
        let width = (visible.x + visible.width).ceil() - x;
        let height = (visible.y + visible.height).ceil() - y;
        let raster = Rect::new(x, y, width, height);
        // The active parent scissor remains on the canvas. Keep its soft
        // edges outside the tile so their coverage is never applied twice.
        let interior = transform
            .map_rect(display_clip)
            .inset(0.5 / dpi + 1.0 / 64.0);
        if !(dpi >= 1.0
            && pattern.width > 0.0
            && pattern.height > 0.0
            && visible.width > 1.0
            && visible.height > 1.0
            && (1.0..=64.0).contains(&width)
            && (1.0..=64.0).contains(&height)
            && raster == raster.intersect(interior))
        {
            return None;
        }
        Some(Self {
            raster,
            paint: visible,
            pattern,
            dpi,
        })
    }
}

pub(crate) struct CoverageAtlas {
    pub(crate) image: ImageId,
    view: wgpu::TextureView,
    packing: Atlas,
    bytes: Vec<u8>,
    rasterized: Vec<u8>,
    rasterized_source: Option<Rc<()>>,
    changed: Vec<u8>,
    vertices: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    source: Option<(wgpu::Texture, wgpu::BindGroup)>,
    idle_frames: u8,
}

impl CoverageAtlas {
    pub(crate) fn new(device: &wgpu::Device, canvas: &mut Canvas<WGPURenderer>) -> Option<Self> {
        if device.limits().max_texture_dimension_2d < EDGE as u32 {
            return None;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack antialiased image atlas"),
            size: wgpu::Extent3d {
                width: EDGE as u32,
                height: EDGE as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        // FemtoVG samples native textures as float RGBA. The float atlas
        // avoids an intermediate 8-bit quantization before compositing.
        let image = canvas
            .create_image_from_native_texture(
                texture,
                ImageInfo::new(ImageFlags::NEAREST, EDGE, EDGE, PixelFormat::Rgba8),
            )
            .ok()?;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("XRack antialiased image instances"),
            size: INSTANCE_BYTES * MAX_TILES as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Some(Self {
            image,
            view,
            packing: Atlas::new(EDGE, EDGE),
            bytes: Vec::new(),
            rasterized: Vec::new(),
            rasterized_source: None,
            changed: Vec::new(),
            vertices,
            pipeline: pipeline(device),
            sampler,
            source: None,
            idle_frames: 0,
        })
    }

    pub(crate) fn begin_frame(&mut self) {
        self.packing.reset(EDGE, EDGE);
        self.bytes.clear();
    }

    pub(crate) fn idle(&mut self) -> bool {
        if self.bytes.is_empty() {
            self.idle_frames += 1;
        } else {
            self.idle_frames = 0;
        }
        // Keep the pass warm when movement alternates between aligned and
        // antialiased edges.
        self.idle_frames >= 3
    }

    pub(crate) fn push(
        &mut self,
        canvas: &Canvas<WGPURenderer>,
        coverage: Coverage,
        source: [usize; 2],
        size: [u32; 2],
        source_edge: usize,
    ) -> Option<Quad> {
        if self.bytes.len() >= INSTANCE_BYTES as usize * MAX_TILES {
            return None;
        }
        let Coverage {
            raster,
            paint,
            pattern,
            dpi,
        } = coverage;
        let bounds = canvas_transform(canvas).inverse()?.map_rect(raster);
        let (x, y) = self
            .packing
            .add_rect(raster.width as usize, raster.height as usize)?;
        let [source_x, source_y] = source.map(|n| n as f32 / source_edge as f32);
        let [uv_width, uv_height] = size.map(|n| n as f32 / source_edge as f32);
        let du = uv_width / pattern.width;
        let dv = uv_height / pattern.height;
        let relative = |rect: Rect| {
            let dx = rect.x - raster.x;
            let dy = rect.y - raster.y;
            [dx, dy, dx + rect.width, dy + rect.height]
        };
        let tile = [x as f32, y as f32, raster.width, raster.height];
        let uv = [
            source_x + (raster.x - pattern.x) * du,
            source_y + (raster.y - pattern.y) * dv,
            du,
            dv,
        ];
        let source = [
            source_x,
            source_y,
            source_x + uv_width,
            source_y + uv_height,
        ];
        for value in tile
            .into_iter()
            .chain(relative(paint))
            .chain(uv)
            .chain(source)
            .chain([dpi])
        {
            self.bytes.extend_from_slice(&value.to_ne_bytes());
        }
        let [s, t] = [x as f32 / EDGE as f32, y as f32 / EDGE as f32];
        Some(Quad {
            x0: bounds.x,
            y0: bounds.y,
            x1: bounds.x + bounds.width,
            y1: bounds.y + bounds.height,
            s0: s,
            t0: t,
            s1: s + raster.width / EDGE as f32,
            t1: t + raster.height / EDGE as f32,
        })
    }

    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Texture,
        source_key: &Rc<()>,
    ) -> Option<wgpu::CommandBuffer> {
        if self.bytes.is_empty() {
            return None;
        }
        // The source page changes identity when it replaces a tile in place.
        // Identical UVs can then refer to different source pixels.
        let same_source = self
            .rasterized_source
            .as_ref()
            .is_some_and(|previous| Rc::ptr_eq(previous, source_key));
        if same_source && self.rasterized.starts_with(&self.bytes) {
            return None;
        }
        let mut load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut instances = self.bytes.as_slice();
        if same_source {
            // The command contains the output tile rectangle and every sampling
            // parameter. A match identifies an unchanged, disjoint tile, even
            // when the caller moves it by a whole device pixel.
            let stride = INSTANCE_BYTES as usize;
            let matches = |(index, tile): (usize, &[u8])| {
                let start = index * stride;
                self.rasterized.get(start..start + stride) == Some(tile)
            };
            let mut tiles = self.bytes.chunks_exact(stride).enumerate();
            if let Some(first) = tiles.position(matches) {
                self.changed.clear();
                self.changed
                    .extend_from_slice(&self.bytes[..first * stride]);
                for (index, tile) in tiles {
                    if !matches((index, tile)) {
                        self.changed.extend_from_slice(tile);
                    }
                }
                load = wgpu::LoadOp::Load;
                instances = &self.changed;
            }
        }
        let bind_group = match &mut self.source {
            Some((texture, bind_group)) if texture == source => bind_group,
            slot => {
                let view = source.create_view(&Default::default());
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("XRack image atlas source"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                &slot.insert((source.clone(), group)).1
            }
        };
        queue.write_buffer(&self.vertices, 0, instances);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("XRack rasterize antialiased image tiles"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("XRack rasterize antialiased image tiles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..4, 0..(instances.len() as u64 / INSTANCE_BYTES) as u32);
        }
        self.rasterized.clear();
        self.rasterized.extend_from_slice(&self.bytes);
        self.rasterized_source = Some(source_key.clone());
        Some(encoder.finish())
    }
}

fn pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack antialiased image shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let attributes = wgpu::vertex_attr_array![
        0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack antialiased image pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("edge", EDGE as f64)],
                ..Default::default()
            },
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}
