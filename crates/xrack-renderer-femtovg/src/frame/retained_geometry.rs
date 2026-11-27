use crate::{Paint, Path, Renderer, canvas_transform, has_device_scale, rectangle::Solid};
use femtovg::{ImageFlags, ImageInfo, PixelFormat};
use std::collections::HashMap;
use wgpu::util::DeviceExt;
use xrack_runtime::RowKey;

const SHADER: &str = r#"
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}
@vertex fn vertex(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> Vertex {
    return Vertex(vec4(position, 0.0, 1.0), color);
}
@fragment fn fragment(vertex: Vertex) -> @location(0) vec4<f32> {
    return vertex.color;
}
"#;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Key {
    row: Option<RowKey>,
    extent: [u32; 2],
    rectangles: Vec<[u32; 5]>,
}

struct Mesh {
    vertices: wgpu::Buffer,
    last_frame: u64,
}

#[derive(Default)]
pub(crate) struct GeometryCache {
    pub(crate) enabled: bool,
    meshes: HashMap<Key, Mesh>,
    frame: u64,
    pipeline: Option<wgpu::RenderPipeline>,
    pub(crate) reused: u64,
    pub(crate) row_reuses: u64,
    pub(crate) uploaded: u64,
}

impl GeometryCache {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Default::default()
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.meshes.retain(|key, mesh| {
            let age = self.frame.wrapping_sub(mesh.last_frame);
            age <= if key.row.is_some() { 120 } else { 2 }
        });
    }

    fn buffer(&mut self, device: &wgpu::Device, key: Key) -> wgpu::Buffer {
        if let Some(mesh) = self.meshes.get_mut(&key) {
            mesh.last_frame = self.frame;
            self.reused += 1;
            if key.row.is_some() {
                self.row_reuses += 1;
            }
            xrack_runtime::update_profile::reused_geometry();
            return mesh.vertices.clone();
        }
        let mut bytes = Vec::with_capacity(key.rectangles.len() * 6 * 24);
        for &[x, y, width, height, rgba] in &key.rectangles {
            let to_x = |x: u32| 2.0 * x as f32 / key.extent[0] as f32 - 1.0;
            let to_y = |y: u32| 1.0 - 2.0 * y as f32 / key.extent[1] as f32;
            let [x0, x1] = [to_x(x), to_x(x + width)];
            let [y0, y1] = [to_y(y), to_y(y + height)];
            let [r, g, b, a] = rgba.to_be_bytes().map(|byte| f32::from(byte) / 255.0);
            for [x, y] in [[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]] {
                for value in [x, y, r, g, b, a] {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
        }
        self.uploaded += bytes.len() as u64;
        xrack_runtime::update_profile::geometry_upload(bytes.len());
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("XRack retained rectangle vertices"),
            contents: &bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.meshes.insert(
            key,
            Mesh {
                vertices: vertices.clone(),
                last_frame: self.frame,
            },
        );
        vertices
    }
}

impl Renderer {
    pub(crate) fn draw_retained_geometry(&mut self, rectangles: &[Solid], scale: f32) -> bool {
        if !self.geometry_cache.enabled
            || rectangles.len() < 8
            || rectangles.len() > 4096
            || self.using_capture_canvas()
            || self.retained_layers.vectors.opacity_passthrough
            || !has_device_scale(canvas_transform(&self.canvas), scale)
        {
            return false;
        }
        let Some(rectangles) = rectangles
            .iter()
            .map(Solid::opaque_rect)
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let Some(&(first, _)) = rectangles.first() else {
            return false;
        };
        let bounds = rectangles
            .iter()
            .map(|(rect, _)| *rect)
            .fold(first, crate::shadow::union);
        let extent = [
            (bounds.width * scale).round() as u32,
            (bounds.height * scale).round() as u32,
        ];
        if extent.contains(&0) || extent.iter().any(|&size| size > 512) {
            return false;
        }
        let key = Key {
            row: self.geometry_row,
            extent,
            rectangles: rectangles
                .iter()
                .map(|(rect, color)| {
                    [
                        ((rect.x - bounds.x) * scale).round() as u32,
                        ((rect.y - bounds.y) * scale).round() as u32,
                        (rect.width * scale).round() as u32,
                        (rect.height * scale).round() as u32,
                        *color,
                    ]
                })
                .collect(),
        };
        if self.geometry_cache.meshes.len() >= 64 && !self.geometry_cache.meshes.contains_key(&key)
        {
            let oldest = self
                .geometry_cache
                .meshes
                .iter()
                .max_by_key(|(_, mesh)| self.geometry_cache.frame.wrapping_sub(mesh.last_frame))
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.geometry_cache.meshes.remove(&oldest);
            }
        }
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let (target, _) = self.transient_textures.acquire(&self.device, extent);
        let view = target.create_view(&Default::default());
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED | ImageFlags::NEAREST,
            extent[0] as usize,
            extent[1] as usize,
            PixelFormat::Rgba8,
        );
        let Ok(image) = self.canvas.create_image_from_native_texture(target, info) else {
            return false;
        };
        let vertices = self.geometry_cache.buffer(&self.device, key);
        let pipeline = self
            .geometry_cache
            .pipeline
            .get_or_insert_with(|| pipeline(&self.device, format));
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack geometry layers"),
                })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("XRack retained geometry"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..rectangles.len() as u32 * 6, 0..1);
        }
        let mut path = Path::new();
        path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        let mut paint = Paint::image(
            image,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            0.0,
            1.0,
        );
        paint.set_anti_alias(false);
        self.canvas.fill_path(&path, &paint);
        self.canvas.delete_image(image);
        true
    }
}

fn pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack rectangle shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("XRack retained rectangles"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}
