use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, VecDeque},
};

use xrack_runtime::{
    Node, Value,
    scene_effect::{
        BufferSampling, BufferSource, CAPTURE_CAMERA_BINDING, CaptureAttachment, Chain, DepthInput,
        Input, MAX_BUFFERS, OutputFormat, SURFACE_BINDING, SURFACE_CAMERA_BINDING, buffer_bindings,
        needs_capture_position, needs_surface, shader, source_is_valid,
    },
    scene3d::SceneCamera,
};

use crate::GpuPaintContext;

use super::{
    Attachment, DEPTH_FORMAT, HDR_FORMAT, append_floats, attachment_view, buffer,
    color_capture::{CaptureProjection, CaptureViews, SharedProjectionBuffers},
    depth_of_field::FocusDepth,
    effect_history::{History, Owner},
    item_textures::ItemTextureView,
    material::{SharedImages, address, filter},
};

#[derive(Clone, Copy)]
struct EffectSettings<'a> {
    source: &'a str,
    strength: f32,
    depth: bool,
    surface: bool,
    scale: f64,
    parameters: [f32; 3],
    format: OutputFormat,
}

impl<'a> EffectSettings<'a> {
    fn from_node(node: &'a Node) -> Option<Self> {
        let source = node.text("scene-effect")?;
        let strength = node
            .float("scene-effect-strength")
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        (source_is_valid(source) && strength > 0.0).then_some(Self {
            source,
            strength,
            depth: node.boolean("scene-effect-depth", false),
            surface: needs_surface(source),
            scale: 1.0,
            parameters: [0.0; 3],
            format: OutputFormat::default(),
        })
    }
}

pub(super) struct EffectList<'a> {
    single: Option<EffectSettings<'a>>,
    chain: Option<&'a Chain>,
}

impl<'a> EffectList<'a> {
    pub fn from_node(node: &'a Node) -> Self {
        if let Some(Value::SceneEffects(chain)) = node.properties.get("scene-effects") {
            return Self {
                single: None,
                chain: Some(chain),
            };
        }
        Self {
            single: EffectSettings::from_node(node),
            chain: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.single.is_none() && self.chain.is_none_or(|chain| chain.passes().is_empty())
    }

    pub fn needs_depth(&self) -> bool {
        self.passes().any(|pass| pass.depth && !pass.surface)
    }

    pub fn needs_surface(&self) -> bool {
        self.passes().any(|pass| pass.surface)
    }

    fn passes(&self) -> impl Iterator<Item = EffectSettings<'a>> {
        self.single.into_iter().chain(
            self.chain
                .into_iter()
                .flat_map(|chain| chain.passes().iter())
                .map(|pass| EffectSettings {
                    source: &pass.source,
                    strength: pass.strength,
                    depth: pass.depth,
                    surface: needs_surface(&pass.source),
                    scale: pass.scale,
                    parameters: pass.parameters,
                    format: pass.format,
                }),
        )
    }
}

const MAX_PIPELINES: usize = 8;
const RETAINED_TARGETS: usize = 4;

pub(super) fn effect_size(view: [u32; 2], scale: f64) -> [u32; 2] {
    view.map(|side| (f64::from(side) * scale).ceil().max(1.0) as u32)
}

pub(super) fn effect_format(format: OutputFormat) -> wgpu::TextureFormat {
    match format {
        OutputFormat::R8 => wgpu::TextureFormat::R8Unorm,
        OutputFormat::R16Float => wgpu::TextureFormat::R16Float,
        OutputFormat::Rg8 => wgpu::TextureFormat::Rg8Unorm,
        OutputFormat::Rg16Float => wgpu::TextureFormat::Rg16Float,
        OutputFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
        OutputFormat::Rgba16Float => HDR_FORMAT,
    }
}

pub(super) fn texture_bytes(size: [u32; 2], format: wgpu::TextureFormat) -> u64 {
    size.map(u64::from).into_iter().product::<u64>()
        * u64::from(format.block_copy_size(None).unwrap_or(8))
}

pub(crate) fn has_history(node: &Node) -> bool {
    matches!(node.properties.get("scene-effects"), Some(Value::SceneEffects(chain))
        if !chain.history_writers().is_empty())
}

#[derive(Default)]
pub(super) struct EffectStats {
    pub passes: u64,
    pub pipelines: u64,
    pub pixels: u64,
    pub allocations: u64,
    pub history_allocations: u64,
    pub allocated_bytes: u64,
}

#[derive(Default)]
pub(super) struct Effects {
    pipelines: VecDeque<Pipeline>,
    outputs: VecDeque<Attachment>,
    samplers: HashMap<BufferSampling, wgpu::Sampler>,
    pub history: History,
    pub images: SharedImages,
    pub projections: SharedProjectionBuffers,
    surface: Option<Attachment>,
    surface_depth: Option<Attachment>,
}

pub(super) struct EffectSurface {
    pub view: wgpu::TextureView,
    pub depth: FocusDepth,
    inverse: wgpu::Buffer,
}

struct Pipeline {
    source: Box<str>,
    depth: DepthInput,
    reverse_depth: bool,
    buffers: usize,
    nearest_buffers: u8,
    format: wgpu::TextureFormat,
    program: Option<Program>,
}

struct Program {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

struct PassInputs<'a> {
    input: &'a wgpu::TextureView,
    original: &'a wgpu::TextureView,
    previous: &'a wgpu::TextureView,
    buffers: &'a [BufferTexture<'a>],
    retained: &'a BTreeMap<usize, (usize, Attachment)>,
    destination: Option<Attachment>,
}

#[derive(Clone, Copy, Default)]
enum BufferEncoding {
    #[default]
    LinearPremultiplied = 0,
    Straight = 1,
    SrgbPremultiplied = 2,
}

struct BufferTexture<'a> {
    view: Option<Cow<'a, wgpu::TextureView>>,
    sampling: BufferSampling,
    encoding: BufferEncoding,
    nearest: bool,
    projection: Option<&'a CaptureProjection>,
}

impl Effects {
    pub fn prepare_surface(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        camera: SceneCamera,
    ) -> Option<EffectSurface> {
        let inverse = camera.inverse_clip_rows()?;
        let view = Attachment::get(&mut self.surface, device, size, HDR_FORMAT, 1, true).clone();
        let depth = Attachment::get(&mut self.surface_depth, device, size, DEPTH_FORMAT, 1, true);
        let depth = FocusDepth {
            view: depth.clone(),
            camera,
            multisampled: false,
        };
        let mut bytes = Vec::with_capacity(80);
        append_floats(&mut bytes, inverse.into_iter().flatten());
        append_floats(&mut bytes, [camera.depth_clear(), 0.0, 0.0, 0.0]);
        let inverse = buffer(
            device,
            "XRack scene effect inverse clip",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        Some(EffectSurface {
            view,
            depth,
            inverse,
        })
    }

    pub fn render(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        original: &wgpu::TextureView,
        geometry: (Option<&FocusDepth>, Option<&EffectSurface>),
        chain: &EffectList<'_>,
        owner: Option<Owner<'_>>,
        (queue, items, captures): (
            &wgpu::Queue,
            &BTreeMap<String, ItemTextureView>,
            Option<&CaptureViews>,
        ),
    ) -> (Option<Attachment>, EffectStats) {
        let mut last = None::<Attachment>;
        let mut stats = EffectStats::default();
        if chain.is_empty() {
            return (last, stats);
        }
        let mut history = owner
            .zip(chain.chain)
            .and_then(|(owner, chain)| self.history.prepare(owner, ctx.size, chain));
        let mut retained = BTreeMap::<usize, (usize, Attachment)>::new();
        let mut target_capacity = RETAINED_TARGETS;
        let pipeline_capacity = chain.chain.map_or(MAX_PIPELINES, |chain| {
            MAX_PIPELINES.max(chain.passes().len())
        });
        for (index, settings) in chain.passes().enumerate() {
            let previous = last.as_ref().map_or(original, |pass| &pass.view);
            let route = chain.chain.and_then(|chain| chain.route(index));
            let lookup = |producer| retained.get(&producer).map(|(_, target)| &target.view);
            let input = match route.map(|route| route.input) {
                None | Some(Input::Previous) => previous,
                Some(Input::Original) => original,
                Some(Input::Named(producer)) => lookup(producer).unwrap_or(original),
            };
            let mut buffers: [BufferTexture<'_>; MAX_BUFFERS] =
                std::array::from_fn(|_| BufferTexture {
                    view: None,
                    sampling: BufferSampling::DEFAULT,
                    encoding: BufferEncoding::default(),
                    nearest: false,
                    projection: None,
                });
            let count = route.map_or(0, |route| {
                for (slot, buffer) in buffers.iter_mut().zip(route.buffers.iter()) {
                    let mut encoding = BufferEncoding::default();
                    let mut nearest = false;
                    let mut projection = None;
                    let view = match buffer.source.as_ref() {
                        None => None,
                        Some(BufferSource::Current(producer)) => {
                            lookup(*producer).map(Cow::Borrowed)
                        }
                        Some(BufferSource::History(producer)) => history
                            .as_ref()
                            .and_then(|history| history.previous(*producer))
                            .map(Cow::Borrowed),
                        Some(BufferSource::Image(image)) => {
                            encoding = BufferEncoding::Straight;
                            Some(Cow::Owned(self.images.borrow_mut().base_view(
                                ctx.device,
                                queue,
                                &image.image,
                                image.color,
                            )))
                        }
                        Some(BufferSource::Capture(read)) => {
                            if read.attachment != CaptureAttachment::Color {
                                encoding = BufferEncoding::Straight;
                            }
                            nearest = read.attachment == CaptureAttachment::Depth;
                            captures
                                .and_then(|captures| captures.get(read))
                                .map(|capture| {
                                    projection = capture.projection.as_deref();
                                    Cow::Borrowed(&capture.view)
                                })
                        }
                        Some(BufferSource::Item(name)) => items.get(name.as_ref()).map(|item| {
                            if !item.linear_premultiplied {
                                encoding = BufferEncoding::SrgbPremultiplied;
                            }
                            Cow::Borrowed(&item.view)
                        }),
                    };
                    *slot = BufferTexture {
                        view,
                        sampling: buffer.sampling,
                        encoding,
                        nearest,
                        projection,
                    };
                }
                route.buffers.len()
            });
            let destination = history.as_ref().and_then(|history| {
                let (attachment, allocated) = history.destination(ctx.device, index)?;
                stats.history_allocations += u64::from(allocated);
                if allocated {
                    stats.allocated_bytes +=
                        texture_bytes(attachment.size, attachment.view.texture().format());
                }
                Some(attachment)
            });
            let save_history = destination.is_some();
            let inputs = PassInputs {
                original,
                input,
                previous,
                buffers: &buffers[..count],
                retained: &retained,
                destination,
            };
            target_capacity = target_capacity.max(retained.len() + 2);
            let rendered = self.render_pass(
                ctx,
                inputs,
                geometry,
                settings,
                pipeline_capacity,
                &mut stats,
            );
            retained.retain(|_, (last_read, _)| *last_read > index);
            if let Some(rendered) = rendered {
                if save_history && let Some(history) = &mut history {
                    history.written(index, &rendered);
                }
                if let Some(route) = route.filter(|route| route.last_read > index) {
                    retained.insert(
                        index,
                        (
                            route.last_read,
                            Attachment {
                                size: rendered.size,
                                view: rendered.view.clone(),
                            },
                        ),
                    );
                }
                last = Some(rendered);
            }
        }
        if let Some(history) = history {
            self.history.record(history);
        }
        self.outputs.truncate(target_capacity);
        (last, stats)
    }

    fn render_pass(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        inputs: PassInputs<'_>,
        (depth, surface): (Option<&FocusDepth>, Option<&EffectSurface>),
        settings: EffectSettings<'_>,
        pipeline_capacity: usize,
        stats: &mut EffectStats,
    ) -> Option<Attachment> {
        let surface = surface.filter(|_| settings.surface);
        let depth = surface
            .map(|surface| &surface.depth)
            .or(depth.filter(|_| settings.depth));
        let kind = match depth {
            Some(_) if surface.is_some() => DepthInput::Surface,
            Some(depth) if depth.multisampled => DepthInput::Multisampled,
            Some(_) => DepthInput::Single,
            None => DepthInput::None,
        };
        let reverse_depth = kind == DepthInput::Multisampled
            && depth.is_some_and(|depth| depth.camera.reverse_depth());
        let count = inputs.buffers.len();
        let nearest_buffers = inputs
            .buffers
            .iter()
            .enumerate()
            .fold(0u8, |bits, (index, input)| {
                bits | (u8::from(input.nearest) << index)
            });
        let format = effect_format(settings.format);
        let cached = self
            .pipelines
            .iter()
            .position(|entry| {
                entry.source.as_ref() == settings.source
                    && entry.depth == kind
                    && entry.reverse_depth == reverse_depth
                    && entry.buffers == count
                    && entry.nearest_buffers == nearest_buffers
                    && entry.format == format
            })
            .and_then(|index| self.pipelines.remove(index));
        stats.pipelines += u64::from(cached.is_none());
        let entry = cached.unwrap_or_else(|| Pipeline {
            source: settings.source.into(),
            depth: kind,
            reverse_depth,
            buffers: count,
            nearest_buffers,
            format,
            program: Program::new(
                ctx.device,
                settings.source,
                (kind, reverse_depth),
                count,
                nearest_buffers,
                format,
            )
            .map_err(|error| tracing::warn!(%error, "XRack disabled an invalid scene effect"))
            .ok(),
        });
        self.pipelines.truncate(pipeline_capacity - 1);
        self.pipelines.push_front(entry);
        let program = self
            .pipelines
            .front()
            .and_then(|entry| entry.program.as_ref())?;
        for sampling in std::iter::once(BufferSampling::DEFAULT)
            .chain(inputs.buffers.iter().map(|buffer| buffer.sampling))
        {
            self.samplers.entry(sampling).or_insert_with(|| {
                let filtering = filter(sampling.filter);
                ctx.device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("XRack scene effect sampler"),
                    address_mode_u: address(sampling.wrap[0]),
                    address_mode_v: address(sampling.wrap[1]),
                    mag_filter: filtering,
                    min_filter: filtering,
                    ..Default::default()
                })
            });
        }
        let sampler = self.samplers.get(&BufferSampling::DEFAULT)?;
        let size = effect_size(ctx.size, settings.scale);
        let history = inputs.destination.is_some();
        let output = inputs.destination.unwrap_or_else(|| {
            let previous = self.outputs.iter().position(|slot| {
                slot.size == size
                    && slot.view.texture().format() == format
                    && &slot.view != inputs.previous
                    && &slot.view != inputs.original
                    && !inputs
                        .retained
                        .values()
                        .any(|(_, active)| active.view == slot.view)
            });
            let cached = previous.and_then(|index| self.outputs.remove(index));
            cached.unwrap_or_else(|| Attachment {
                size,
                view: {
                    stats.allocations += 1;
                    stats.allocated_bytes += texture_bytes(size, format);
                    attachment_view(
                        ctx.device,
                        size,
                        format,
                        1,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        "XRack scene effect output",
                    )
                },
            })
        });
        let mut bytes = Vec::with_capacity(48);
        append_floats(
            &mut bytes,
            [size[0] as f32, size[1] as f32, settings.strength],
        );
        let mask = inputs
            .buffers
            .iter()
            .enumerate()
            .fold(0u32, |mask, (index, view)| {
                mask | (u32::from(view.view.is_some()) << index)
            });
        bytes.extend_from_slice(&mask.to_ne_bytes());
        let [x, y, z] = settings.parameters;
        append_floats(&mut bytes, [x, y, z, 0.0]);
        let encoding = inputs
            .buffers
            .iter()
            .enumerate()
            .fold(0u32, |encoding, (index, buffer)| {
                encoding | ((buffer.encoding as u32) << (2 * index))
            });
        bytes.extend_from_slice(&encoding.to_ne_bytes());
        bytes.extend_from_slice(&[0; 12]);
        let uniform = buffer(
            ctx.device,
            "XRack scene effect frame",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(inputs.input),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(inputs.original),
            },
        ];
        if let Some(depth) = depth {
            entries.push(wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&depth.view),
            });
        }
        if let Some(surface) = surface {
            entries.extend([
                wgpu::BindGroupEntry {
                    binding: SURFACE_BINDING,
                    resource: wgpu::BindingResource::TextureView(&surface.view),
                },
                wgpu::BindGroupEntry {
                    binding: SURFACE_CAMERA_BINDING,
                    resource: surface.inverse.as_entire_binding(),
                },
            ]);
        }
        let projections =
            (nearest_buffers != 0 && needs_capture_position(settings.source)).then(|| {
                self.projections.borrow_mut().get(
                    ctx.device,
                    count,
                    inputs.buffers.iter().map(|buffer| buffer.projection),
                )
            });
        if let Some(projections) = &projections {
            entries.push(wgpu::BindGroupEntry {
                binding: CAPTURE_CAMERA_BINDING,
                resource: projections.as_entire_binding(),
            });
        }
        entries.extend(
            inputs
                .buffers
                .iter()
                .enumerate()
                .flat_map(|(index, buffer)| {
                    let (texture, sampler_binding) = buffer_bindings(index);
                    let configured = self.samplers.get(&buffer.sampling).unwrap_or(sampler);
                    [
                        wgpu::BindGroupEntry {
                            binding: texture,
                            resource: wgpu::BindingResource::TextureView(
                                buffer.view.as_deref().unwrap_or(inputs.original),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: sampler_binding,
                            resource: wgpu::BindingResource::Sampler(configured),
                        },
                    ]
                }),
        );
        let bindings = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack scene effect inputs"),
            layout: &program.layout,
            entries: &entries,
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack scene effect"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&program.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        stats.passes += 1;
        stats.pixels += size.into_iter().map(u64::from).product::<u64>();
        if history {
            Some(output)
        } else {
            let result = output.clone();
            self.outputs.push_front(output);
            Some(result)
        }
    }
}

impl Program {
    fn new(
        device: &wgpu::Device,
        source: &str,
        (depth, reverse_depth): (DepthInput, bool),
        buffers: usize,
        nearest_buffers: u8,
        format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        let binding = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let texture = |sample_type, multisampled| wgpu::BindingType::Texture {
            sample_type,
            multisampled,
            view_dimension: wgpu::TextureViewDimension::D2,
        };
        let mut entries = vec![
            binding(
                0,
                texture(wgpu::TextureSampleType::Float { filterable: true }, false),
            ),
            binding(
                1,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
            binding(
                2,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            binding(
                4,
                texture(wgpu::TextureSampleType::Float { filterable: true }, false),
            ),
        ];
        if depth != DepthInput::None {
            entries.push(binding(
                3,
                texture(
                    wgpu::TextureSampleType::Depth,
                    depth == DepthInput::Multisampled,
                ),
            ));
        }
        if depth == DepthInput::Surface {
            entries.extend([
                binding(
                    SURFACE_BINDING,
                    texture(wgpu::TextureSampleType::Float { filterable: false }, false),
                ),
                binding(
                    SURFACE_CAMERA_BINDING,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ]);
        }
        if nearest_buffers != 0 && needs_capture_position(source) {
            entries.push(binding(
                CAPTURE_CAMERA_BINDING,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ));
        }
        entries.extend((0..buffers).flat_map(|index| {
            let (view, sampler) = buffer_bindings(index);
            [
                binding(
                    view,
                    texture(
                        wgpu::TextureSampleType::Float {
                            filterable: nearest_buffers & (1 << index) == 0,
                        },
                        false,
                    ),
                ),
                binding(
                    sampler,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ]
        }));
        let label = Some("XRack scene effect");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let wgsl = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label,
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(shader(
                source,
                depth,
                buffers,
                nearest_buffers,
            ))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &wgsl,
                entry_point: Some("xr_effect_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &wgsl,
                entry_point: Some("xr_effect_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        (
                            "XR_EFFECT_DATA",
                            f64::from(u8::from(matches!(
                                format,
                                wgpu::TextureFormat::R8Unorm
                                    | wgpu::TextureFormat::R16Float
                                    | wgpu::TextureFormat::Rg8Unorm
                                    | wgpu::TextureFormat::Rg16Float
                            ))),
                        ),
                        ("XR_EFFECT_REVERSE_DEPTH", f64::from(reverse_depth)),
                    ],
                    ..Default::default()
                },
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
            return Err(error.to_string());
        }
        Ok(Self { layout, pipeline })
    }
}
