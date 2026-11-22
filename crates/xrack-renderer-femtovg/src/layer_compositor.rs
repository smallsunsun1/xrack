use crate::{Rect, Renderer, color, retained_layers::CachedLayer};
use std::{collections::HashMap, rc::Rc};
use wgpu::util::DeviceExt;
use xrack_runtime::Color;

const INSTANCE_STRIDE: u64 = 9 * size_of::<f32>() as u64;
pub(super) const MAX_INSTANCES: usize = 64;
const MIN_INSTANCES: usize = 8;

const SHADER: &str = r#"
@group(0) @binding(0) var layer_texture: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;
@group(1) @binding(0) var clip_texture: texture_2d<f32>;
@group(1) @binding(1) var<uniform> clip_origin: vec2i;

struct Vertex {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) opacity: f32,
}

@vertex fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) position: vec4f,
    @location(1) uv_bounds: vec4f,
    @location(2) opacity: f32,
) -> Vertex {
    let corner = vec2f(f32(index & 1u), f32(index >> 1u));
    let p = mix(position.xy, position.zw, corner);
    let uv = mix(uv_bounds.xy, uv_bounds.zw, corner);
    return Vertex(vec4f(p, 0.0, 1.0), uv, opacity);
}

@fragment fn fragment(input: Vertex) -> @location(0) vec4f {
    return textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0) * input.opacity;
}

@fragment fn masked_fragment(input: Vertex) -> @location(0) vec4f {
    let pixel = vec2i(input.position.xy) - clip_origin;
    let size = vec2i(textureDimensions(clip_texture));
    var coverage = 0.0;
    if all(pixel >= vec2i(0)) && all(pixel < size) {
        coverage = textureLoad(clip_texture, pixel, 0).r;
    }
    return textureSampleLevel(layer_texture, layer_sampler, input.uv, 0.0)
        * (input.opacity * coverage);
}
"#;

pub(super) struct LayerCompositor {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    mask_layout: wgpu::BindGroupLayout,
    masked_pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    instances: wgpu::Buffer,
    staging: wgpu::util::StagingBelt,
    pipelines: HashMap<(wgpu::TextureFormat, bool), wgpu::RenderPipeline>,
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
        let mask_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack cached layer clip mask"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let masked_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack masked layer compositor"),
                bind_group_layouts: &[Some(&layout), Some(&mask_layout)],
                immediate_size: 0,
            });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack cached layer compositor"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("XRack cached layer instances"),
            size: MAX_INSTANCES as u64 * INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline_layout,
            mask_layout,
            masked_pipeline_layout,
            shader,
            sampler,
            instances,
            staging: wgpu::util::StagingBelt::new(device.clone(), 8192),
            pipelines: HashMap::new(),
        }
    }
}

impl Renderer {
    pub(super) fn encode_cached_layers(
        &mut self,
        target: &wgpu::Texture,
        background: Color,
        scale: f32,
        mut layers: Vec<CachedLayer>,
    ) {
        // The caller has retained their textures for reentry into the viewport.
        layers.retain(|layer| layer.bounds.width > 0.0 && layer.bounds.height > 0.0);
        if disjoint(&layers) {
            layers.sort_by_key(|layer| {
                (
                    Rc::as_ptr(&layer.image),
                    layer.mask.as_ref().map(Rc::as_ptr),
                )
            });
        } else {
            group_disjoint_draws(&mut layers);
        }
        let compositor = self
            .layer_compositor
            .get_or_insert_with(|| LayerCompositor::new(&self.device));
        let format = target.format();
        for masked in [false, true] {
            if !layers.iter().any(|layer| layer.mask.is_some() == masked) {
                continue;
            }
            compositor
                .pipelines
                .entry((format, masked))
                .or_insert_with(|| {
                    let attributes =
                        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32];
                    self.device
                        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                            label: Some("XRack cached layer compositor"),
                            layout: Some(if masked {
                                &compositor.masked_pipeline_layout
                            } else {
                                &compositor.pipeline_layout
                            }),
                            vertex: wgpu::VertexState {
                                module: &compositor.shader,
                                entry_point: Some("vertex"),
                                compilation_options: Default::default(),
                                buffers: &[Some(wgpu::VertexBufferLayout {
                                    array_stride: INSTANCE_STRIDE,
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
                                module: &compositor.shader,
                                entry_point: Some(if masked {
                                    "masked_fragment"
                                } else {
                                    "fragment"
                                }),
                                compilation_options: Default::default(),
                                targets: &[Some(wgpu::ColorTargetState {
                                    format,
                                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                    write_mask: wgpu::ColorWrites::ALL,
                                })],
                            }),
                            multiview_mask: None,
                            cache: None,
                        })
                });
        }
        let mut data = Vec::with_capacity(layers.len() * INSTANCE_STRIDE as usize);
        let to_x = |x: f32| 2.0 * (x * scale) / target.width() as f32 - 1.0;
        let to_y = |y: f32| 1.0 - 2.0 * (y * scale) / target.height() as f32;
        for layer in &layers {
            let Rect {
                x,
                y,
                width,
                height,
            } = layer.bounds;
            for value in [to_x(x), to_y(y), to_x(x + width), to_y(y + height)]
                .into_iter()
                .chain(layer.uv)
                .chain([layer.opacity])
            {
                data.extend_from_slice(&value.to_ne_bytes());
            }
        }
        let bytes = wgpu::BufferSize::new(data.len() as u64);
        let target = target.create_view(&wgpu::TextureViewDescriptor {
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
        if let Some(bytes) = bytes {
            compositor
                .staging
                .write_buffer(encoder, &compositor.instances, 0, bytes)
                .copy_from_slice(&data);
        }
        let background = color(background);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack composite cached layers"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
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
        });
        pass.set_vertex_buffer(0, compositor.instances.slice(..));
        let mut index = 0;
        let mut current_masked = None;
        for run in layers.chunk_by(same_material) {
            let image = &run[0].image;
            let mask = &run[0].mask;
            let masked = mask.is_some();
            if current_masked != Some(masked) {
                let Some(pipeline) = compositor.pipelines.get(&(format, masked)) else {
                    continue;
                };
                pass.set_pipeline(pipeline);
                current_masked = Some(masked);
            }
            let bind_group = image.binding.get_or_init(|| {
                let view = image.texture.create_view(&Default::default());
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("XRack cached layer source"),
                    layout: &compositor.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&compositor.sampler),
                        },
                    ],
                })
            });
            let end = index + run.len() as u32;
            pass.set_bind_group(0, bind_group, &[]);
            if let Some(mask) = mask {
                let binding = mask.binding.get_or_init(|| {
                    let view = mask.texture.create_view(&Default::default());
                    let origin = mask.origin.map(i32::to_ne_bytes);
                    let coordinates =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("XRack cached clip origin"),
                                contents: origin.as_flattened(),
                                usage: wgpu::BufferUsages::UNIFORM,
                            });
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("XRack cached layer clip"),
                        layout: &compositor.mask_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: coordinates.as_entire_binding(),
                            },
                        ],
                    })
                });
                pass.set_bind_group(1, binding, &[]);
            }
            pass.draw(0..4, index..end);
            xrack_runtime::update_profile::quad_batch(run.len());
            index = end;
        }
        drop(pass);
        if bytes.is_some() {
            compositor.staging.finish_and_recall_on_submit(encoder);
        }
    }
}

pub(super) fn large_enough(count: usize) -> bool {
    (MIN_INSTANCES..=MAX_INSTANCES).contains(&count)
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

fn disjoint(layers: &[CachedLayer]) -> bool {
    layers
        .iter()
        .enumerate()
        .all(|(index, layer)| layers[..index].iter().all(|other| !overlaps(layer, other)))
}

fn overlaps(first: &CachedLayer, second: &CachedLayer) -> bool {
    let intersection = first.bounds.intersect(second.bounds);
    intersection.width > 0.0 && intersection.height > 0.0
}

fn same_material(first: &CachedLayer, second: &CachedLayer) -> bool {
    Rc::ptr_eq(&first.image, &second.image)
        && first.mask.as_ref().map(Rc::as_ptr) == second.mask.as_ref().map(Rc::as_ptr)
}

fn group_disjoint_draws(layers: &mut [CachedLayer]) {
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
