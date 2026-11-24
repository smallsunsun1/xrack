//! Resolve scene-linear HDR after rendering and optional bloom.
use std::collections::VecDeque;

use crate::GpuPaintContext;
use xrack_runtime::{Node, SceneToneMap, Value};

use super::{
    Attachment, COLOR_FORMAT, HDR_FORMAT, append_floats, attachment_view,
    bloom::{Bloom, BloomSettings},
    buffer,
    color_capture::CaptureViews,
    custom_effect::{EffectList, EffectSurface, Effects},
    depth_of_field::{DepthOfField, FocusDepth, FocusSettings},
    effect_history::Owner,
    item_textures::ItemTextureView,
    lut::{LutCache, LutSettings},
    motion::MotionInputs,
    progressive::Accumulator,
    retained_view::AaSample,
    supersampling::Supersampling,
    temporal::TemporalFilter,
};

pub(super) struct PostSettings<'a> {
    pub hdr: bool,
    linear_intermediate: bool,
    composite_alpha: bool,
    pub focus: Option<FocusSettings>,
    exposure: f32,
    bloom: Option<BloomSettings>,
    tone_map: SceneToneMap,
    white_point: f32,
    adjustments: Option<[f32; 3]>,
    lut: Option<LutSettings>,
    effects: EffectList<'a>,
}

impl<'a> PostSettings<'a> {
    pub fn from_node(
        node: &'a Node,
        environment: bool,
        has_geometry: bool,
        composite_alpha: bool,
    ) -> Self {
        let bloom = BloomSettings::from_node(node);
        let curve = match node.properties.get("scene-tone-map") {
            Some(Value::SceneToneMap(curve)) => Some(*curve),
            _ => None,
        };
        let adjustments = ["scene-brightness", "scene-contrast", "scene-saturation"]
            .map(|name| node.float(name).unwrap_or(1.0).clamp(0.0, 4.0));
        let effects = EffectList::from_node(node);
        Self {
            hdr: environment
                || node.boolean("scene-hdr", false)
                || bloom.is_some()
                || curve.is_some(),
            linear_intermediate: false,
            composite_alpha: composite_alpha || !effects.is_empty(),
            focus: has_geometry
                .then(|| FocusSettings::from_node(node))
                .flatten(),
            exposure: node
                .float("scene-exposure")
                .unwrap_or(1.0)
                .clamp(0.0, 100.0),
            bloom,
            tone_map: curve.unwrap_or_default(),
            white_point: node
                .float("scene-white-point")
                .unwrap_or(11.2)
                .clamp(0.01, 65_000.0),
            adjustments: (adjustments != [1.0; 3]).then_some(adjustments),
            lut: LutSettings::from_node(node),
            effects,
        }
    }

    pub fn require_linear_intermediate(&mut self) {
        self.linear_intermediate = true;
    }

    pub fn uses_linear_intermediate(&self) -> bool {
        self.hdr || self.linear_intermediate || !self.effects.is_empty()
    }

    pub fn needs_effect_depth(&self) -> bool {
        self.effects.needs_depth()
    }

    pub fn needs_effect_surface(&self) -> bool {
        self.effects.needs_surface()
    }

    pub fn needs_target(&self) -> bool {
        self.uses_linear_intermediate()
            || self.focus.is_some()
            || self.composite_alpha
            || self.adjustments.is_some()
            || self.lut.is_some()
    }

    pub fn composites_alpha(&self) -> bool {
        self.composite_alpha
    }

    pub fn has_adjustments(&self) -> bool {
        self.adjustments.is_some()
    }
}

#[derive(Default)]
pub(super) struct PostStats {
    pub bloom_passes: u64,
    pub bloom_pixels: u64,
    pub bloom_target_allocations: u64,
    pub focus_filtered_pixels: u64,
    pub custom_effect_passes: u64,
    pub custom_effect_pipelines: u64,
    pub custom_effect_pixels: u64,
    pub custom_effect_target_allocations: u64,
    pub custom_effect_history_allocations: u64,
    pub custom_effect_bytes_allocated: u64,
    pub lut_resolves: u64,
    pub lut_uploads: u64,
}

pub(super) const RETAINED_VIEW_SIZES: usize = 2;

#[derive(Default)]
pub(super) struct PostProcess {
    colors: [VecDeque<Attachment>; 4],
    supersampling: Option<Supersampling>,
    tone_maps: [Option<ToneMap>; 8],
    resolve_sampler: Option<wgpu::Sampler>,
    accumulator: Option<Accumulator>,
    temporal: TemporalFilter,
    bloom: Option<Bloom>,
    focus: Option<DepthOfField>,
    luts: Option<LutCache>,
    pub(super) effects: Effects,
}

impl PostProcess {
    pub(super) fn area_filter(&mut self, device: &wgpu::Device) -> &mut Supersampling {
        self.supersampling
            .get_or_insert_with(|| Supersampling::new(device))
    }

    pub fn target(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        hdr: bool,
        supersampled: bool,
    ) -> wgpu::TextureView {
        let slot = usize::from(hdr) + 2 * usize::from(supersampled);
        let targets = &mut self.colors[slot];
        let index = targets.iter().position(|target| target.size == size);
        let entry = index
            .and_then(|index| targets.remove(index))
            .unwrap_or_else(|| Attachment {
                size,
                view: attachment_view(
                    device,
                    size,
                    if hdr { HDR_FORMAT } else { COLOR_FORMAT },
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    "XRack 3D postprocessing source",
                ),
            });
        let view = entry.view.clone();
        targets.truncate(RETAINED_VIEW_SIZES - 1);
        targets.push_front(entry);
        view
    }

    pub fn resolve(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        (queue, items, captures): (
            &wgpu::Queue,
            &std::collections::BTreeMap<String, ItemTextureView>,
            Option<&CaptureViews>,
        ),
        (source, raster_size): (&wgpu::TextureView, [u32; 2]),
        (depth, surface): (Option<&FocusDepth>, Option<&EffectSurface>),
        (settings, sample, motion, owner): (
            &PostSettings<'_>,
            Option<&AaSample>,
            Option<&MotionInputs>,
            Option<Owner<'_>>,
        ),
    ) -> PostStats {
        let supersampled = raster_size != ctx.size;
        let linear = settings.uses_linear_intermediate();
        let format = if linear { HDR_FORMAT } else { COLOR_FORMAT };
        let convert = linear
            || settings.composite_alpha
            || settings.adjustments.is_some()
            || settings.lut.is_some()
            || sample.is_some();
        let focus = settings.focus.zip(depth).map(|(focus, depth)| {
            let mut focus_ctx = GpuPaintContext {
                size: raster_size,
                encoder: &mut *ctx.encoder,
                ..*ctx
            };
            self.focus
                .get_or_insert_with(|| DepthOfField::new(ctx.device))
                .render(
                    &mut focus_ctx,
                    source,
                    depth,
                    focus,
                    (
                        (convert || supersampled).then_some(format),
                        settings.composite_alpha,
                    ),
                )
        });
        let focus_filtered_pixels = focus.as_ref().map_or(0, |result| result.filtered_pixels);
        let resolved_focus = focus.and_then(|result| result.color);
        let source = resolved_focus.as_ref().unwrap_or(source);
        let reduced = if supersampled {
            let reduced = convert.then(|| self.target(ctx.device, ctx.size, linear, false));
            self.area_filter(ctx.device).resolve(
                ctx.device,
                ctx.encoder,
                (source, reduced.as_ref().unwrap_or(ctx.target)),
                ctx.size,
                if reduced.is_some() {
                    format
                } else {
                    ctx.format
                },
            );
            reduced
        } else {
            None
        };
        if !convert {
            return PostStats {
                focus_filtered_pixels,
                ..Default::default()
            };
        }
        let source = reduced.as_ref().unwrap_or(source);
        let temporal = match sample {
            Some(AaSample::Temporal(sample)) => Some(self.temporal.resolve(
                ctx.device,
                ctx.encoder,
                source,
                (sample, motion),
                ctx.size,
            )),
            _ => None,
        };
        let source = if let Some(AaSample::Progressive(sample)) = sample {
            self.accumulator
                .get_or_insert_with(|| Accumulator::new(ctx.device))
                .blend(ctx.device, ctx.encoder, source, sample);
            &sample.history
        } else {
            temporal.as_ref().unwrap_or(source)
        };
        let (effect_output, effect_stats) = self.effects.render(
            ctx,
            source,
            (depth, surface),
            &settings.effects,
            owner,
            (queue, items, captures),
        );
        let (source, source_size) = effect_output
            .as_ref()
            .map_or((source, ctx.size), |output| (&output.view, output.size));
        let glow = settings.bloom.and_then(|bloom| {
            self.bloom
                .get_or_insert_with(|| Bloom::new(ctx.device))
                .render(ctx.device, ctx.encoder, source, source_size, bloom)
        });
        let lut = settings.lut.as_ref().and_then(|lut| {
            self.luts
                .get_or_insert_with(|| LutCache::new(ctx.device))
                .bind(ctx.device, queue, lut)
        });
        let mode = usize::from(glow.is_some()) + 2 * usize::from(lut.is_some());
        let slot = mode + 4 * usize::from(settings.composite_alpha);
        let tone_map = self.tone_maps[slot]
            .get_or_insert_with(|| ToneMap::new(ctx.device, mode, settings.composite_alpha));
        let sampler = self.resolve_sampler.get_or_insert_with(|| {
            ctx.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D tone mapping source sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        });
        let strength = settings.bloom.map_or(0.0, |bloom| bloom.strength);
        let curve = if settings.hdr {
            settings.tone_map
        } else {
            SceneToneMap::None
        };
        let mut bytes = Vec::with_capacity(48);
        append_floats(
            &mut bytes,
            [
                settings.exposure,
                strength,
                curve as u8 as f32,
                settings.white_point,
            ],
        );
        let [brightness, contrast, saturation] = settings.adjustments.unwrap_or([1.0; 3]);
        append_floats(
            &mut bytes,
            [
                brightness,
                contrast,
                saturation,
                if settings.adjustments.is_some() {
                    1.0
                } else {
                    0.0
                },
            ],
        );
        let [lut_size, lut_strength] = settings
            .lut
            .as_ref()
            .map_or([0.0; 2], |lut| [lut.size as f32, lut.strength]);
        append_floats(
            &mut bytes,
            [
                lut_size,
                lut_strength,
                ctx.size[0] as f32,
                ctx.size[1] as f32,
            ],
        );
        let uniform = buffer(
            ctx.device,
            "XRack 3D HDR resolve settings",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut inputs = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ];
        if let Some(glow) = &glow {
            inputs.extend([
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(glow.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(glow.sampler),
                },
            ]);
        }
        if let Some(lut) = &lut {
            inputs.extend([
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&lut.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&lut.sampler),
                },
            ]);
        }
        let bindings = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D tone mapping inputs"),
            layout: &tone_map.layout,
            entries: &inputs,
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D HDR resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&tone_map.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        PostStats {
            bloom_passes: glow.as_ref().map_or(0, |glow| glow.passes),
            bloom_pixels: glow.as_ref().map_or(0, |glow| glow.pixels),
            bloom_target_allocations: glow.as_ref().map_or(0, |glow| glow.allocations),
            focus_filtered_pixels,
            custom_effect_passes: effect_stats.passes,
            custom_effect_pipelines: effect_stats.pipelines,
            custom_effect_pixels: effect_stats.pixels,
            custom_effect_target_allocations: effect_stats.allocations,
            custom_effect_history_allocations: effect_stats.history_allocations,
            custom_effect_bytes_allocated: effect_stats.allocated_bytes,
            lut_resolves: u64::from(lut.is_some()),
            lut_uploads: u64::from(lut.is_some_and(|lut| lut.uploaded)),
        }
    }
}

struct ToneMap {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl ToneMap {
    fn new(device: &wgpu::Device, mode: usize, composite_alpha: bool) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D tone mapping"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tone_map.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack 3D tone mapping"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(
                    [
                        "fragment",
                        "fragment_bloom",
                        "fragment_lut",
                        "fragment_bloom_lut",
                    ][mode],
                ),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("COMPOSITE_ALPHA", f64::from(u8::from(composite_alpha)))],
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
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
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
        }
    }
}
