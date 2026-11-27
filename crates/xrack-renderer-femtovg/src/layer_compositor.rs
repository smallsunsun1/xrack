use crate::{
    Rect, Renderer, color,
    image::Images,
    retained_layers::{CachedLayerImage, WindowLayer, WindowLayerSource},
    scroll_images::{ImageMotion, MAX_SCROLL_BATCHES, WindowImageBatch},
};
use femtovg::Quad;
use std::{collections::HashMap, rc::Rc};
use xrack_runtime::Color;

const IMAGE_INSTANCE_STRIDE: usize = 9 * size_of::<f32>();
const LAYER_INSTANCE_STRIDE: usize =
    IMAGE_INSTANCE_STRIDE + size_of::<u32>() + size_of::<[i32; 4]>() + size_of::<[f32; 4]>();
pub(super) const MAX_INSTANCES: usize = 1024;
pub(super) const MAX_IMAGE_INSTANCES: usize = 4096;
pub(super) const MIN_IMAGE_INSTANCES: usize = 64;
pub(super) const IMAGE_MOTION_BYTES: u32 = size_of::<[f32; 4]>() as u32;
pub(super) const MIN_INSTANCES: usize = 8;
const REORDER_WINDOW: usize = 64;

const SHADER: &str = r#"
@group(0) @binding(0) var layer_texture: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;
@group(1) @binding(0) var clip_texture: texture_2d<f32>;
@group(2) @binding(0) var second_clip_texture: texture_2d<f32>;

struct Vertex {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) paint_parameter: f32,
    @location(2) @interpolate(flat) clip_flags: u32,
    @location(3) @interpolate(flat) clip_origin: vec2i,
    @location(4) @interpolate(flat) edge_clip: vec4f,
    @location(5) @interpolate(flat) second_origin: vec2i,
}

fn quad_vertex(
    index: u32, position: vec4f, uv_bounds: vec4f, paint_parameter: f32, clip_flags: u32, clip_origin: vec2i, edge_clip: vec4f, second_origin: vec2i,
) -> Vertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    let p = mix(position.xy, position.zw, corner);
    let uv = mix(uv_bounds.xy, uv_bounds.zw, corner);
    return Vertex(vec4f(p, 0.0, 1.0), uv, paint_parameter, clip_flags, clip_origin, edge_clip, second_origin);
}

@vertex fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) uv_bounds: vec4f,
    @location(2) paint_parameter: f32,
    @location(3) clip_flags: u32,
    @location(4) clip_origin: vec2i,
    @location(5) edge_clip: vec4f,
    @location(6) second_origin: vec2i,
) -> Vertex {
    return quad_vertex(index, position, uv_bounds, paint_parameter, clip_flags, clip_origin, edge_clip, second_origin);
}

@vertex fn image_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) uv_bounds: vec4f,
    @location(2) paint_parameter: f32,
) -> Vertex {
    return quad_vertex(index, position, uv_bounds, paint_parameter, 0u, vec2i(0), vec4f(0.0), vec2i(0));
}

struct SolidVertex {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) color: vec4f,
}

@vertex fn solid_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) color: vec4f,
) -> SolidVertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    return SolidVertex(vec4f(mix(position.xy, position.zw, corner), 0.0, 1.0), color);
}

@fragment fn solid_fragment(input: SolidVertex) -> @location(0) vec4f {
    return vec4f(input.color.rgb * input.color.a, input.color.a);
}

struct CoveredVertex {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) edges: vec4f,
    @location(1) @interpolate(flat) tint: u32,
    @location(2) @interpolate(flat) scale: f32,
}

@vertex fn covered_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) edges: vec4f,
    @location(2) tint: f32,
    @location(3) scale_bits: u32,
) -> CoveredVertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    return CoveredVertex(vec4f(mix(position.xy, position.zw, corner), 0.0, 1.0),
        edges, u32(tint), bitcast<f32>(scale_bits));
}

fn rgb_tint(tint: u32) -> vec3f {
    return vec3f(f32(tint >> 16u), f32((tint >> 8u) & 255u), f32(tint & 255u)) / 255.0;
}

@fragment fn covered_fragment(input: CoveredVertex) -> @location(0) vec4f {
    let axis_distance = min(input.position.xy - input.edges.xy, input.edges.zw - input.position.xy);
    let coverage = clamp(0.5 + min(axis_distance.x, axis_distance.y) * input.scale, 0.0, 1.0);
    // Match the original palette's RGBA8 coverage before premultiplied blending.
    let alpha = floor(255.0 * coverage + 0.5) / 255.0;
    return vec4f(rgb_tint(input.tint) * alpha, alpha);
}

@fragment fn fragment(input: Vertex) -> @location(0) vec4f {
    return textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0) * (input.paint_parameter * rectangular_coverage(input));
}

@fragment fn image_fragment(input: Vertex) -> @location(0) vec4f {
    let sample = textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0);
    return vec4f(sample.rgb * sample.a, sample.a);
}

fn clip_coverage(input: Vertex) -> f32 {
    if (input.clip_flags & 1u) == 0u {
        return 1.0;
    }
    let pixel = vec2i(input.position.xy) - input.clip_origin;
    let size = vec2i(textureDimensions(clip_texture));
    if all(pixel >= vec2i(0)) && all(pixel < size) {
        return textureLoad(clip_texture, pixel, 0).r;
    }
    return 0.0;
}

fn split_clip_coverage(input: Vertex) -> f32 {
    let outer = clip_coverage(input);
    if outer == 0.0 || (input.clip_flags & 4u) == 0u {
        return outer;
    }
    let pixel = vec2i(input.position.xy) - input.second_origin;
    let size = vec2i(textureDimensions(second_clip_texture));
    if all(pixel >= vec2i(0)) && all(pixel < size) {
        return outer * textureLoad(second_clip_texture, pixel, 0).r;
    }
    return 0.0;
}

@fragment fn split_masked_fragment(input: Vertex) -> @location(0) vec4f {
    return textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0)
        * (input.paint_parameter * rectangular_coverage(input) * split_clip_coverage(input));
}

@fragment fn split_masked_alpha_fragment(input: Vertex) -> @location(0) vec4f {
    return tint_alpha(input) * split_clip_coverage(input);
}

fn rectangular_coverage(input: Vertex) -> f32 {
    if (input.clip_flags & 2u) == 0u {
        return 1.0;
    }
    let coverage = clamp(
        vec2f(0.5) + min(input.position.xy - input.edge_clip.xy, input.edge_clip.zw - input.position.xy),
        vec2f(0.0), vec2f(1.0),
    );
    return coverage.x * coverage.y;
}

@fragment fn masked_fragment(input: Vertex) -> @location(0) vec4f {
    return textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0)
        * (input.paint_parameter * rectangular_coverage(input) * clip_coverage(input));
}

fn tint_alpha(input: Vertex) -> vec4f {
    let rgb = rgb_tint(u32(input.paint_parameter));
    let coverage = textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0).a;
    return vec4f(rgb * coverage, coverage);
}

@fragment fn alpha_fragment(input: Vertex) -> @location(0) vec4f {
    return tint_alpha(input);
}

@fragment fn masked_alpha_fragment(input: Vertex) -> @location(0) vec4f {
    return tint_alpha(input) * clip_coverage(input);
}

"#;

const SCROLL_SHADER: &str = r#"
struct ImageMotion {
    offset: vec2f,
    scale: vec2f,
}

var<immediate> motion: ImageMotion;

@vertex fn scroll_image_vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) uv_bounds: vec4f,
    @location(2) paint_parameter: f32,
) -> Vertex {
    let moved = (position + motion.offset.xyxy) * motion.scale.xyxy + vec4f(-1.0, 1.0, -1.0, 1.0);
    return quad_vertex(index, moved, uv_bounds, paint_parameter, 0u, vec2i(0), vec4f(0.0), vec2i(0));
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LayerShader {
    Retained,
    Masked,
    SplitMasked,
    Image,
    ScrollImage,
    Alpha,
    MaskedAlpha,
    SplitMaskedAlpha,
    Solid,
    OpaqueSolid,
    CoveredSolid,
}

impl LayerShader {
    fn for_layer(layer: &WindowLayer) -> Self {
        let split = layer
            .mask
            .as_ref()
            .is_some_and(|clip| clip.second.is_some());
        match &layer.source {
            WindowLayerSource::TintedAlpha { .. } if split => Self::SplitMaskedAlpha,
            WindowLayerSource::TintedAlpha { .. } if layer.mask.is_some() => Self::MaskedAlpha,
            WindowLayerSource::TintedAlpha { .. } => Self::Alpha,
            WindowLayerSource::Solid(_) => Self::Solid,
            WindowLayerSource::CoveredSolid { .. } => Self::CoveredSolid,
            WindowLayerSource::Texture(_) if split => Self::SplitMasked,
            WindowLayerSource::Texture(_) if layer.mask.is_some() => Self::Masked,
            WindowLayerSource::Texture(_) => Self::Retained,
        }
    }

    fn for_batch(batch: &[WindowLayer]) -> Self {
        let flavor = Self::for_layer(&batch[0]);
        if flavor == Self::Solid
            && batch.iter().all(
                |layer| matches!(&layer.source, WindowLayerSource::Solid(color) if color.a == 1.0),
            )
        {
            Self::OpaqueSolid
        } else {
            flavor
        }
    }
}

#[derive(Clone)]
struct InstanceBinding {
    buffer: wgpu::Buffer,
    offset: u64,
}

struct ImageDraw {
    pipeline: wgpu::RenderPipeline,
    binding: wgpu::BindGroup,
    instances: InstanceBinding,
    motion: Option<ImageMotion>,
    count: u32,
    image_count: u64,
    written: usize,
}

impl ImageDraw {
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, target: &wgpu::Texture) {
        pass.set_pipeline(&self.pipeline);
        let [width, height] = [target.width(), target.height()];
        if let Some(motion) = self.motion {
            let x = motion.clip.x.min(width);
            let y = motion.clip.y.min(height);
            let right = motion.clip.x.saturating_add(motion.clip.width).min(width);
            let bottom = motion.clip.y.saturating_add(motion.clip.height).min(height);
            pass.set_scissor_rect(x, y, right - x, bottom - y);
            let [dx, dy] = motion.offset;
            let push = [dx, dy, 2.0 / width as f32, -2.0 / height as f32].map(f32::to_ne_bytes);
            pass.set_immediates(0, push.as_flattened());
        }
        pass.set_vertex_buffer(0, self.instances.buffer.slice(self.instances.offset..));
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..4, 0..self.count);
        if self.motion.is_some() {
            pass.set_scissor_rect(0, 0, width, height);
        }
        xrack_runtime::update_profile::quad_batch_upload(self.written);
        if self.motion.is_some() && self.written == 0 {
            xrack_runtime::update_profile::reused_geometry();
        }
    }
}

enum InstanceStorage {
    Shared {
        belt: wgpu::util::StagingBelt,
        resident: Option<InstanceBinding>,
    },
    Copied(wgpu::Buffer),
}

struct InstanceBuffer<const STRIDE: usize> {
    storage: InstanceStorage,
    contents: Vec<[u8; STRIDE]>,
}

impl<const STRIDE: usize> InstanceBuffer<STRIDE> {
    fn new(device: &wgpu::Device, capacity: usize, label: &'static str) -> Self {
        let storage = if device
            .features()
            .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
        {
            InstanceStorage::Shared {
                belt: wgpu::util::StagingBelt::new_with_buffer_usages(
                    device.clone(),
                    8192,
                    wgpu::BufferUsages::VERTEX,
                ),
                resident: None,
            }
        } else {
            InstanceStorage::Copied(instance_copy_buffer(
                device,
                (capacity * STRIDE) as u64,
                label,
            ))
        };
        Self {
            storage,
            contents: Vec::new(),
        }
    }

    fn discard_upload(&mut self) {
        self.contents.clear();
    }

    fn binding(&self) -> Option<InstanceBinding> {
        match &self.storage {
            InstanceStorage::Shared { resident, .. } => resident.clone(),
            InstanceStorage::Copied(buffer) => Some(InstanceBinding {
                buffer: buffer.clone(),
                offset: 0,
            }),
        }
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        staging: &mut wgpu::util::StagingBelt,
        encoder: &mut wgpu::CommandEncoder,
        instances: impl ExactSizeIterator<Item = [u8; STRIDE]>,
    ) -> Result<Option<(InstanceBinding, usize)>, String> {
        let count = instances.len();
        if count == 0 {
            return Ok(None);
        }
        let bytes = (count as u64).saturating_mul(STRIDE as u64);
        let limit = device.limits().max_buffer_size;
        if bytes > limit {
            return Err("compositor instances exceed the device buffer limit".into());
        }
        if let InstanceStorage::Copied(buffer) = &mut self.storage
            && buffer.size() < bytes
        {
            let capacity = bytes
                .checked_next_power_of_two()
                .unwrap_or(limit)
                .min(limit);
            *buffer = instance_copy_buffer(device, capacity, "XRack compositor instances");
            // A newly allocated buffer has none of the previous prefix.
            self.contents.clear();
        }
        let mut first_changed = None;
        let mut end = 0;
        for (index, instance) in instances.enumerate() {
            match self.contents.get_mut(index) {
                Some(current) if *current == instance => continue,
                Some(current) => *current = instance,
                None => self.contents.push(instance),
            }
            first_changed.get_or_insert(index);
            end = index + 1;
        }
        match &mut self.storage {
            InstanceStorage::Shared { belt, resident } => {
                if first_changed.is_none()
                    && let Some(binding) = resident
                {
                    return Ok(Some((binding.clone(), 0)));
                }
                // Reuse a resident buffer until its vertices change. Its previous
                // uses have been submitted before the next frame, so recall can
                // schedule a remap once those draws complete.
                *resident = None;
                belt.recall();
                // Only the active prefix needs to be in the new resident buffer.
                self.contents.truncate(count);
                let bytes = self.contents.as_flattened();
                let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else {
                    return Ok(None);
                };
                let slice = belt.allocate(size, wgpu::BufferSize::MIN);
                let binding = slice.get_mapped_range_mut().map(|mut mapped| {
                    mapped.copy_from_slice(bytes);
                    InstanceBinding {
                        buffer: slice.buffer().clone(),
                        offset: slice.offset(),
                    }
                });
                belt.finish();
                let binding = binding.map_err(|error| error.to_string())?;
                *resident = Some(binding.clone());
                Ok(Some((binding, bytes.len())))
            }
            InstanceStorage::Copied(buffer) => {
                let mut written = 0;
                if let Some(start) = first_changed {
                    // The buffer retains trailing vertices when the draw count shrinks.
                    let bytes = self.contents[start..end].as_flattened();
                    if let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) {
                        staging
                            .write_buffer(encoder, buffer, (start * STRIDE) as u64, size)
                            .copy_from_slice(bytes);
                        staging.finish_and_recall_on_submit(encoder);
                        written = bytes.len();
                    }
                }
                Ok(Some((
                    InstanceBinding {
                        buffer: buffer.clone(),
                        offset: 0,
                    },
                    written,
                )))
            }
        }
    }
}

fn instance_copy_buffer(device: &wgpu::Device, size: u64, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        mapped_at_creation: false,
    })
}

struct ImageInstances {
    buffer: InstanceBuffer<IMAGE_INSTANCE_STRIDE>,
    geometry: Option<Rc<[Quad]>>,
}

pub(super) struct LayerCompositor {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    scroll_pipeline_layout: Option<wgpu::PipelineLayout>,
    solid_pipeline_layout: wgpu::PipelineLayout,
    mask_layout: wgpu::BindGroupLayout,
    masked_pipeline_layout: wgpu::PipelineLayout,
    split_mask_pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    instances: InstanceBuffer<LAYER_INSTANCE_STRIDE>,
    staging: wgpu::util::StagingBelt,
    image_instances: Option<ImageInstances>,
    scroll_instances: [Option<ImageInstances>; MAX_SCROLL_BATCHES],
    pipelines: HashMap<(wgpu::TextureFormat, LayerShader), wgpu::RenderPipeline>,
}

impl LayerCompositor {
    fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack cached layer texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("XRack cached layer compositor"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let scroll_pipeline_layout =
            device
                .features()
                .contains(wgpu::Features::IMMEDIATES)
                .then(|| {
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("XRack scrolling images"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: IMAGE_MOTION_BYTES,
                    })
                });
        let solid_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack solid layer compositor"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack cached layer clip mask"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let masked_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack masked layer compositor"),
                bind_group_layouts: &[Some(&layout), Some(&mask_layout)],
                immediate_size: 0,
            });
        let split_mask_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack split mask compositor"),
                bind_group_layouts: &[Some(&layout), Some(&mask_layout), Some(&mask_layout)],
                immediate_size: 0,
            });
        let source = if scroll_pipeline_layout.is_some() {
            format!("{SHADER}{SCROLL_SHADER}").into()
        } else {
            SHADER.into()
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack cached layer compositor"),
            source: wgpu::ShaderSource::Wgsl(source),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let instances = InstanceBuffer::new(device, MAX_INSTANCES, "XRack cached layer instances");
        Self {
            layout,
            pipeline_layout,
            scroll_pipeline_layout,
            solid_pipeline_layout,
            mask_layout,
            masked_pipeline_layout,
            split_mask_pipeline_layout,
            shader,
            sampler,
            instances,
            staging: wgpu::util::StagingBelt::new(device.clone(), 8192),
            pipelines: HashMap::new(),
            image_instances: None,
            scroll_instances: std::array::from_fn(|_| None),
        }
    }

    pub(super) fn discard_pending_instances(&mut self) {
        // An aborted frame can drop copies encoded since the last queue submit.
        self.instances.discard_upload();
        for images in self
            .image_instances
            .iter_mut()
            .chain(self.scroll_instances.iter_mut().flatten())
        {
            images.buffer.discard_upload();
            images.geometry = None;
        }
    }
    fn bind_source(&self, device: &wgpu::Device, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack compositor source"),
            layout: &self.layout,
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
        })
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        flavor: LayerShader,
    ) -> &wgpu::RenderPipeline {
        self.pipelines.entry((format, flavor)).or_insert_with(|| {
            let attributes =
                wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32, 3 => Uint32, 4 => Sint32x2, 5 => Float32x4, 6 => Sint32x2];
            let (stride, attributes) = if matches!(flavor, LayerShader::Image | LayerShader::ScrollImage) {
                (IMAGE_INSTANCE_STRIDE, &attributes[..3])
            } else {
                (LAYER_INSTANCE_STRIDE, attributes.as_slice())
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack cached layer compositor"),
                layout: Some(match flavor {
                    LayerShader::Masked | LayerShader::MaskedAlpha => &self.masked_pipeline_layout,
                    LayerShader::SplitMasked | LayerShader::SplitMaskedAlpha => &self.split_mask_pipeline_layout,
                    LayerShader::Solid | LayerShader::OpaqueSolid | LayerShader::CoveredSolid => &self.solid_pipeline_layout,
                    LayerShader::ScrollImage => self.scroll_pipeline_layout.as_ref().unwrap_or(&self.pipeline_layout),
                    _ => &self.pipeline_layout,
                }),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some(match flavor {
                        LayerShader::Solid | LayerShader::OpaqueSolid => "solid_vertex",
                        LayerShader::CoveredSolid => "covered_vertex",
                        LayerShader::Image => "image_vertex",
                        LayerShader::ScrollImage => "scroll_image_vertex",
                        _ => "vertex",
                    }),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(match flavor {
                        LayerShader::Retained => "fragment",
                        LayerShader::Masked => "masked_fragment",
                        LayerShader::SplitMasked => "split_masked_fragment",
                        LayerShader::Image | LayerShader::ScrollImage => "image_fragment",
                        LayerShader::Alpha => "alpha_fragment",
                        LayerShader::MaskedAlpha => "masked_alpha_fragment",
                        LayerShader::SplitMaskedAlpha => "split_masked_alpha_fragment",
                        LayerShader::Solid | LayerShader::OpaqueSolid => "solid_fragment",
                        LayerShader::CoveredSolid => "covered_fragment",
                    }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: (flavor != LayerShader::OpaqueSolid)
                            .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}

impl LayerCompositor {
    fn prepare_image_draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        atlas: &Images,
        images: &WindowImageBatch,
    ) -> Result<Option<ImageDraw>, String> {
        let (source, quads, image_count, motion) = images.parts();
        if quads.is_empty() {
            return Ok(None);
        }
        let Some((source, binding)) = atlas.source_atlas(source) else {
            return Ok(None);
        };
        let binding = binding
            .get_or_init(|| self.bind_source(device, source))
            .clone();
        let shader = if motion.is_some() {
            LayerShader::ScrollImage
        } else {
            LayerShader::Image
        };
        let pipeline = self.pipeline(device, target.format(), shader).clone();
        let (slot, geometry) = match images {
            WindowImageBatch::Scrolling { quads, slot, .. } => {
                (&mut self.scroll_instances[*slot], Some(quads))
            }
            WindowImageBatch::Visible { .. } => (&mut self.image_instances, None),
        };
        let instances = slot.get_or_insert_with(|| ImageInstances {
            buffer: InstanceBuffer::new(
                device,
                MAX_IMAGE_INSTANCES,
                "XRack window image instances",
            ),
            geometry: None,
        });
        let to_x = 2.0 / target.width() as f32;
        let to_y = 2.0 / target.height() as f32;
        let data = quads.iter().map(|quad| {
            let position = if motion.is_some() {
                [quad.x0, quad.y0, quad.x1, quad.y1]
            } else {
                [
                    quad.x0 * to_x - 1.0,
                    1.0 - quad.y0 * to_y,
                    quad.x1 * to_x - 1.0,
                    1.0 - quad.y1 * to_y,
                ]
            };
            let [x0, y0, x1, y1] = position;
            let instance = [x0, y0, x1, y1, quad.s0, quad.t0, quad.s1, quad.t1, 1.0];
            let mut bytes = [0; IMAGE_INSTANCE_STRIDE];
            bytes.copy_from_slice(instance.map(f32::to_ne_bytes).as_flattened());
            bytes
        });
        let resident = if geometry
            .zip(instances.geometry.as_ref())
            .is_some_and(|(current, previous)| Rc::ptr_eq(current, previous))
        {
            instances.buffer.binding()
        } else {
            None
        };
        let (instances, written) = if let Some(resident) = resident {
            (resident, 0)
        } else {
            let Some(upload) = instances
                .buffer
                .upload(device, &mut self.staging, encoder, data)?
            else {
                return Ok(None);
            };
            instances.geometry = geometry.cloned();
            upload
        };
        Ok(Some(ImageDraw {
            binding,
            pipeline,
            instances,
            motion,
            count: quads.len() as u32,
            image_count: image_count as u64,
            written,
        }))
    }
}

impl Renderer {
    pub(super) fn encode_window_image_quads(
        &mut self,
        target: &wgpu::Texture,
        background: Color,
        images: &WindowImageBatch,
    ) -> Result<bool, String> {
        let compositor = self
            .layer_compositor
            .get_or_insert_with(|| LayerCompositor::new(&self.device));
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack window image commands"),
                })
        });
        let Some(image) =
            compositor.prepare_image_draw(&self.device, encoder, target, &self.images, images)?
        else {
            return Ok(false);
        };
        let view = target.create_view(&Default::default());
        {
            let mut pass =
                begin_composition(encoder, &view, background, "XRack composite window images");
            image.draw(&mut pass, target);
        }
        self.display_batch_stats.image_batches += 1;
        self.display_batch_stats.images += image.image_count;
        Ok(true)
    }

    pub(super) fn encode_cached_layers(
        &mut self,
        target: &wgpu::Texture,
        background: Color,
        scale: f32,
        mut layers: Vec<WindowLayer>,
        mut scrolls: Vec<(usize, WindowImageBatch)>,
    ) -> Result<bool, String> {
        let visible = |layer: &WindowLayer| layer.bounds.width > 0.0 && layer.bounds.height > 0.0;
        for (index, _) in &mut scrolls {
            *index = layers[..*index]
                .iter()
                .filter(|layer| visible(layer))
                .count();
        }
        // The caller has retained their textures for reentry into the viewport.
        layers.retain(visible);
        self.pack_composited_opacity_layers(&mut layers);
        // Bound the overlap checks independently of the instance buffer size.
        // Keep any reordering between consecutive scrolling batches.
        let mut start = 0;
        for end in scrolls
            .iter()
            .map(|(index, _)| *index)
            .chain(std::iter::once(layers.len()))
        {
            for window in layers[start..end].chunks_mut(REORDER_WINDOW) {
                if window[1..]
                    .iter()
                    .all(|layer| same_material(&window[0], layer))
                {
                    continue;
                }
                if disjoint(window) {
                    window.sort_by_key(material_key);
                } else {
                    group_disjoint_draws(window);
                }
            }
            start = end;
        }
        let compositor = self
            .layer_compositor
            .get_or_insert_with(|| LayerCompositor::new(&self.device));
        let format = target.format();
        let to_x = |x: f32| 2.0 * (x * scale) / target.width() as f32 - 1.0;
        let to_y = |y: f32| 1.0 - 2.0 * (y * scale) / target.height() as f32;
        let data = layers.iter().map(|layer| {
            let Rect {
                x,
                y,
                width,
                height,
            } = layer.bounds;
            let [outer, inner] = layer.clip_samples;
            let flags = u32::from(outer)
                | (u32::from(layer.edge_clip.is_some()) << 1)
                | (u32::from(inner) << 2);
            let (attributes, paint_parameter, flags) = match &layer.source {
                WindowLayerSource::Texture(_) => (layer.uv, layer.opacity, flags),
                // RGB8 fits exactly in one f32 and stays flat across the quad.
                WindowLayerSource::TintedAlpha { tint, .. } => (layer.uv, *tint as f32, flags),
                WindowLayerSource::Solid(color) => {
                    ([color.r, color.g, color.b, color.a], 0.0, flags)
                }
                WindowLayerSource::CoveredSolid { tint, edges } => {
                    (*edges, *tint as f32, scale.to_bits())
                }
            };
            let [a, b, c, d] = attributes;
            let geometry = [
                to_x(x),
                to_y(y),
                to_x(x + width),
                to_y(y + height),
                a,
                b,
                c,
                d,
                paint_parameter,
            ];
            let origin = layer.mask.as_ref().map_or([0; 2], |clip| clip.origin);
            let mut bytes = [0; LAYER_INSTANCE_STRIDE];
            let (geometry_bytes, clip_bytes) = bytes.split_at_mut(IMAGE_INSTANCE_STRIDE);
            geometry_bytes.copy_from_slice(geometry.map(f32::to_ne_bytes).as_flattened());
            clip_bytes[..4].copy_from_slice(&flags.to_ne_bytes());
            clip_bytes[4..12].copy_from_slice(origin.map(i32::to_ne_bytes).as_flattened());
            if let Some(clip) = layer.edge_clip {
                let clip = [clip.x, clip.y, clip.x + clip.width, clip.y + clip.height]
                    .map(|edge| (edge * scale).to_ne_bytes());
                clip_bytes[12..28].copy_from_slice(clip.as_flattened());
            }
            if let Some(second) = layer.mask.as_ref().and_then(|clip| clip.second.as_ref()) {
                clip_bytes[28..]
                    .copy_from_slice(second.origin.map(i32::to_ne_bytes).as_flattened());
            }
            bytes
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            mip_level_count: Some(1),
            array_layer_count: Some(1),
            ..Default::default()
        });
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack cached layer commands"),
                })
        });
        let instances =
            compositor
                .instances
                .upload(&self.device, &mut compositor.staging, encoder, data)?;
        let mut scrolling = Vec::with_capacity(scrolls.len());
        for (index, images) in scrolls {
            let Some(draw) = compositor.prepare_image_draw(
                &self.device,
                encoder,
                target,
                &self.images,
                &images,
            )?
            else {
                return Ok(false);
            };
            scrolling.push((index, draw));
        }
        let mut pass =
            begin_composition(encoder, &view, background, "XRack composite cached layers");
        let mut index = 0;
        let mut pending_stats = instances.as_ref().map_or(0, |(_, written)| *written);
        let mut start = 0;
        for (end, draw) in scrolling
            .iter()
            .map(|(index, draw)| (*index, Some(draw)))
            .chain(std::iter::once((layers.len(), None)))
        {
            let layers = &layers[start..end];
            start = end;
            if !layers.is_empty()
                && let Some((instances, _)) = &instances
            {
                pass.set_vertex_buffer(0, instances.buffer.slice(instances.offset..));
            }
            let mut current_shader = None;
            for run in layers.chunk_by(same_material) {
                let layer = &run[0];
                // A mixed solid batch keeps its blending pipeline and draw count.
                let flavor = LayerShader::for_batch(run);
                if current_shader != Some(flavor) {
                    pass.set_pipeline(compositor.pipeline(&self.device, format, flavor));
                    current_shader = Some(flavor);
                }
                let end = index + run.len() as u32;
                if let WindowLayerSource::Texture(image)
                | WindowLayerSource::TintedAlpha { image, .. } = &layer.source
                {
                    let bind_group = image
                        .binding
                        .get_or_init(|| compositor.bind_source(&self.device, &image.texture));
                    pass.set_bind_group(0, bind_group, &[]);
                }
                if let Some(clip) = &layer.mask {
                    let masks = [
                        Some(&clip.mask),
                        clip.second.as_ref().map(|second| &second.mask),
                    ];
                    for (index, mask) in masks.into_iter().enumerate() {
                        let Some(mask) = mask else {
                            continue;
                        };
                        let binding = mask.binding.get_or_init(|| {
                            let view = mask.texture.create_view(&Default::default());
                            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("XRack cached layer clip"),
                                layout: &compositor.mask_layout,
                                entries: &[wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(&view),
                                }],
                            })
                        });
                        pass.set_bind_group(index as u32 + 1, binding, &[]);
                    }
                }
                pass.draw(0..4, index..end);
                xrack_runtime::update_profile::quad_batch_upload(std::mem::take(
                    &mut pending_stats,
                ));
                index = end;
            }
            if let Some(draw) = draw {
                draw.draw(&mut pass, target);
                self.display_batch_stats.image_batches += 1;
                self.display_batch_stats.images += draw.image_count;
            }
        }
        Ok(true)
    }
}

fn begin_composition<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    background: Color,
    label: &str,
) -> wgpu::RenderPass<'a> {
    let background = color(background);
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(background.r),
                    g: f64::from(background.g),
                    b: f64::from(background.b),
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}

pub(super) fn large_enough(count: usize) -> bool {
    (MIN_INSTANCES..=MAX_INSTANCES).contains(&count)
}

pub(super) fn rectangle_instance_limit(device: &wgpu::Device) -> usize {
    let by_buffer = device.limits().max_buffer_size / LAYER_INSTANCE_STRIDE as u64;
    usize::try_from(by_buffer.min(u64::from(u32::MAX))).unwrap_or(usize::MAX)
}

pub(super) fn supports_target(target: &wgpu::Texture) -> bool {
    target.sample_count() == 1
        && matches!(
            target.format(),
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        )
}

fn disjoint(layers: &[WindowLayer]) -> bool {
    layers
        .iter()
        .enumerate()
        .all(|(index, layer)| layers[..index].iter().all(|other| !overlaps(layer, other)))
}

fn overlaps(first: &WindowLayer, second: &WindowLayer) -> bool {
    let intersection = first.bounds.intersect(second.bounds);
    intersection.width > 0.0 && intersection.height > 0.0
}

fn material_key(
    layer: &WindowLayer,
) -> (
    Option<*const CachedLayerImage>,
    Option<*const crate::retained_layers::WindowClipMask>,
    Option<*const crate::retained_layers::WindowClipMask>,
) {
    let image = match &layer.source {
        WindowLayerSource::Texture(image) | WindowLayerSource::TintedAlpha { image, .. } => {
            Some(Rc::as_ptr(image))
        }
        WindowLayerSource::Solid(_) | WindowLayerSource::CoveredSolid { .. } => None,
    };
    (
        image,
        layer.mask.as_ref().map(|clip| Rc::as_ptr(&clip.mask)),
        layer
            .mask
            .as_ref()
            .and_then(|clip| clip.second.as_ref())
            .map(|sample| Rc::as_ptr(&sample.mask)),
    )
}

fn same_material(first: &WindowLayer, second: &WindowLayer) -> bool {
    material_key(first) == material_key(second)
        && LayerShader::for_layer(first) == LayerShader::for_layer(second)
}

fn group_disjoint_draws(layers: &mut [WindowLayer]) {
    let mut start = 0;
    while start < layers.len() {
        let mut end = start + 1;
        for index in end..layers.len() {
            // A draw may join the batch only when everything it passes
            // covers different pixels. The order within a batch is retained.
            if same_material(&layers[start], &layers[index])
                && layers[end..index]
                    .iter()
                    .all(|layer| !overlaps(layer, &layers[index]))
            {
                layers[end..=index].rotate_right(1);
                end += 1;
            }
        }
        start = end;
    }
}
