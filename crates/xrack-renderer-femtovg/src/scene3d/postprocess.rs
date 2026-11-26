//! Resolve scene-linear HDR after rendering and optional bloom.
use std::{
    collections::VecDeque,
    rc::{Rc, Weak},
};

use crate::{GpuPaintContext, transient_textures::create_render_texture};
use xrack_runtime::{Color, Node, SceneToneMap, Value, scene3d::linear_color};

use super::{
    Attachment, COLOR_FORMAT, HDR_FORMAT, append_floats, attachment_view,
    bloom::{Bloom, BloomSettings},
    buffer,
    color_capture::CaptureViews,
    custom_effect::{EffectList, EffectSurface, Effects},
    depth_of_field::{DepthOfField, FocusDepth, FocusSettings},
    effect_history::Owner,
    fxaa::{Fxaa, FxaaSettings},
    item_textures::ItemTextureView,
    lens_flare::{FlareSettings, LensFlare},
    lut::{LutCache, LutSettings},
    motion::MotionInputs,
    progressive::Accumulator,
    retained_view::{AaSample, DisplayFrame},
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
    flare: Option<FlareSettings>,
    tone_map: SceneToneMap,
    white_point: f32,
    adjustments: Option<[f32; 3]>,
    lut: Option<LutSettings>,
    finish: DisplayFinish,
    fxaa: Option<FxaaSettings>,
    effects: EffectList<'a>,
}

#[derive(Default)]
struct DisplayFinish {
    vignette: [f32; 4],
    radius: f32,
    sharpness: f32,
    dithering: bool,
}

impl DisplayFinish {
    fn from_node(node: &Node) -> Self {
        let unit = |name, default| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
                .clamp(0.0, 1.0)
        };
        let color = node
            .color("scene-vignette-color")
            .unwrap_or(Color(0x000000ff));
        let radius = unit("scene-vignette-radius", 0.55);
        let amount = if radius < 1.0 {
            unit("scene-vignette-strength", 0.0) * (f32::from(color.0 as u8) / 255.0)
        } else {
            0.0
        };
        let vignette = if amount > 0.0 {
            let [r, g, b] = linear_color(color);
            [r, g, b, amount]
        } else {
            [0.0; 4]
        };
        Self {
            vignette,
            radius,
            sharpness: unit("scene-sharpness", 0.0),
            dithering: node.boolean("scene-dithering", false),
        }
    }

    fn active(&self) -> bool {
        self.vignette[3] > 0.0 || self.sharpness > 0.0 || self.dithering
    }
}

impl<'a> PostSettings<'a> {
    pub fn for_retained(node: &'a Node, frame: &DisplayFrame) -> Self {
        let mut settings = Self::from_node(node, false, false, frame.composite_alpha);
        settings.hdr = frame.hdr;
        settings
    }

    pub fn from_node(
        node: &'a Node,
        environment: bool,
        has_geometry: bool,
        composite_alpha: bool,
    ) -> Self {
        let bloom = BloomSettings::from_node(node);
        let flare = FlareSettings::from_node(node);
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
                || flare.is_some()
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
            flare,
            tone_map: curve.unwrap_or_default(),
            white_point: node
                .float("scene-white-point")
                .unwrap_or(11.2)
                .clamp(0.01, 65_000.0),
            adjustments: (adjustments != [1.0; 3]).then_some(adjustments),
            lut: LutSettings::from_node(node),
            finish: DisplayFinish::from_node(node),
            fxaa: FxaaSettings::from_node(node),
            effects,
        }
    }

    pub fn unprocessed(composite_alpha: bool) -> Self {
        Self {
            hdr: false,
            linear_intermediate: false,
            composite_alpha,
            focus: None,
            exposure: 1.0,
            bloom: None,
            flare: None,
            tone_map: SceneToneMap::None,
            white_point: 1.0,
            adjustments: None,
            lut: None,
            finish: DisplayFinish::default(),
            fxaa: None,
            effects: EffectList::default(),
        }
    }

    pub fn custom(node: &'a Node) -> Self {
        Self {
            linear_intermediate: true,
            effects: EffectList::from_node(node),
            ..Self::unprocessed(true)
        }
    }

    pub fn has_effects(&self) -> bool {
        !self.effects.is_empty()
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
        self.converts_color() || self.focus.is_some() || self.fxaa.is_some()
    }

    fn converts_color(&self) -> bool {
        self.uses_linear_intermediate()
            || self.composite_alpha
            || self.adjustments.is_some()
            || self.lut.is_some()
            || self.finish.active()
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
    pub display_resolves: u64,
    pub display_pipeline_builds: u64,
    pub fxaa_passes: u64,
    pub fxaa_pixels: u64,
    pub fxaa_input_allocations: u64,
    pub bloom_passes: u64,
    pub bloom_pixels: u64,
    pub bloom_target_allocations: u64,
    pub lens_flare_passes: u64,
    pub lens_flare_pixels: u64,
    pub lens_flare_highlight_passes: u64,
    pub lens_flare_shared_highlights: u64,
    pub lens_flare_image_uploads: u64,
    pub lens_flare_target_allocations: u64,
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
pub(super) struct DisplayLeases(Vec<Weak<DisplayFrame>>);

impl DisplayLeases {
    pub(super) fn contains(&self, view: &wgpu::TextureView) -> bool {
        self.0.iter().filter_map(Weak::upgrade).any(|frame| {
            frame.color.texture() == view.texture()
                || frame
                    .glow
                    .as_ref()
                    .is_some_and(|glow| glow.texture() == view.texture())
                || frame
                    .flare
                    .as_ref()
                    .is_some_and(|flare| flare.texture() == view.texture())
        })
    }
}

#[derive(Default)]
pub(super) struct PostProcess {
    colors: [VecDeque<Attachment>; 4],
    supersampling: Option<Supersampling>,
    tone_maps: [[Option<ToneMap>; 16]; 5],
    resolve_sampler: Option<wgpu::Sampler>,
    transparent_source: Option<wgpu::TextureView>,
    accumulator: Option<Accumulator>,
    temporal: TemporalFilter,
    bloom: Option<Bloom>,
    lens_flare: Option<LensFlare>,
    fxaa: Fxaa,
    focus: Option<DepthOfField>,
    luts: Option<LutCache>,
    leases: DisplayLeases,
    pub(super) effects: Effects,
}

impl PostProcess {
    pub(super) fn area_filter(&mut self, device: &wgpu::Device) -> &mut Supersampling {
        self.supersampling
            .get_or_insert_with(|| Supersampling::new(device))
    }

    pub fn transparent_source(&mut self, device: &wgpu::Device) -> &wgpu::TextureView {
        // GPU texture initialization guarantees zero for the first read.
        self.transparent_source.get_or_insert_with(|| {
            attachment_view(
                device,
                [1; 2],
                HDR_FORMAT,
                1,
                wgpu::TextureUsages::TEXTURE_BINDING,
                "XRack empty custom pipeline input",
            )
        })
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
        let index = targets
            .iter()
            .position(|target| target.size == size && !self.leases.contains(&target.view));
        let entry = index
            .and_then(|index| targets.remove(index))
            .unwrap_or_else(|| Attachment {
                size,
                view: color_target(
                    device,
                    size,
                    if hdr { HDR_FORMAT } else { COLOR_FORMAT },
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
        (source, raster_size): (&wgpu::TextureView, Option<[u32; 2]>),
        (depth, surface): (Option<&FocusDepth>, Option<&EffectSurface>),
        (settings, sample, motion, owner, retain_display): (
            &PostSettings<'_>,
            Option<&AaSample>,
            Option<&MotionInputs>,
            Option<Owner<'_>>,
            bool,
        ),
    ) -> (PostStats, Option<Rc<DisplayFrame>>) {
        // Custom pipelines may sample sources at different resolutions. Their
        // final pass scales directly into the viewport without the SSAA resolve.
        let raster_size = raster_size.unwrap_or(ctx.size);
        let supersampled = raster_size != ctx.size;
        let linear = settings.uses_linear_intermediate();
        let format = if linear { HDR_FORMAT } else { COLOR_FORMAT };
        let convert = settings.converts_color() || sample.is_some();
        let retain_color = convert || settings.fxaa.is_some();
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
                        (retain_color || supersampled).then_some(format),
                        settings.composite_alpha,
                    ),
                    &self.leases,
                )
        });
        let focus_filtered_pixels = focus.as_ref().map_or(0, |result| result.filtered_pixels);
        let resolved_focus = focus.and_then(|result| result.color);
        let source = resolved_focus.as_ref().unwrap_or(source);
        let reduced = if supersampled {
            let reduced = retain_color.then(|| self.target(ctx.device, ctx.size, linear, false));
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
        if !retain_color {
            return (
                PostStats {
                    focus_filtered_pixels,
                    ..Default::default()
                },
                None,
            );
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
            &self.leases,
        );
        let (source, source_size) = effect_output
            .as_ref()
            .map_or((source, ctx.size), |output| (&output.view, output.size));
        let glow = settings.bloom.and_then(|bloom| {
            self.bloom
                .get_or_insert_with(|| Bloom::new(ctx.device))
                .render(
                    ctx.device,
                    ctx.encoder,
                    source,
                    source_size,
                    bloom.source,
                    (&self.leases, None),
                )
        });
        let mut flare_highlight_passes = 0;
        let mut shared_highlights = 0;
        let flare = settings.flare.as_ref().and_then(|settings_flare| {
            let highlights = if let Some(glow) = glow.as_ref().filter(|_| {
                settings
                    .bloom
                    .is_some_and(|bloom| bloom.source == settings_flare.source)
            }) {
                shared_highlights = 1;
                glow.view.clone()
            } else {
                let output = self
                    .bloom
                    .get_or_insert_with(|| Bloom::new(ctx.device))
                    .render(
                        ctx.device,
                        ctx.encoder,
                        source,
                        source_size,
                        settings_flare.source,
                        (&self.leases, glow.as_ref().map(|glow| &glow.view)),
                    )?;
                flare_highlight_passes = output.passes;
                output.view
            };
            let flare = self
                .lens_flare
                .get_or_insert_with(|| LensFlare::new(ctx.device, queue));
            Some(flare.render(
                ctx.device,
                queue,
                ctx.encoder,
                &highlights,
                ctx.size,
                settings_flare,
                &self.leases,
            ))
        });
        let frame = DisplayFrame {
            color: source.clone(),
            glow: glow.as_ref().map(|glow| glow.view.clone()),
            flare: flare.as_ref().map(|flare| flare.view.clone()),
            hdr: settings.hdr,
            composite_alpha: settings.composite_alpha,
        };
        let mut work = self.render_display(ctx, queue, &frame, settings);
        work.bloom_passes = glow.as_ref().map_or(0, |glow| glow.passes);
        work.bloom_pixels = glow.as_ref().map_or(0, |glow| glow.pixels);
        work.bloom_target_allocations = glow.as_ref().map_or(0, |glow| glow.allocations);
        work.lens_flare_passes = u64::from(flare.is_some());
        work.lens_flare_pixels = flare.as_ref().map_or(0, |flare| flare.pixels);
        work.lens_flare_highlight_passes = flare_highlight_passes;
        work.lens_flare_shared_highlights = shared_highlights;
        work.lens_flare_image_uploads = flare.as_ref().map_or(0, |flare| flare.uploads);
        work.lens_flare_target_allocations = flare.as_ref().map_or(0, |flare| flare.allocations);
        work.focus_filtered_pixels = focus_filtered_pixels;
        work.custom_effect_passes = effect_stats.passes;
        work.custom_effect_pipelines = effect_stats.pipelines;
        work.custom_effect_pixels = effect_stats.pixels;
        work.custom_effect_target_allocations = effect_stats.allocations;
        work.custom_effect_history_allocations = effect_stats.history_allocations;
        work.custom_effect_bytes_allocated = effect_stats.allocated_bytes;
        (work, retain_display.then(|| self.pin(frame)))
    }

    pub fn pin_direct(&mut self, color: &wgpu::TextureView) -> Rc<DisplayFrame> {
        self.pin(DisplayFrame {
            color: color.clone(),
            glow: None,
            flare: None,
            hdr: false,
            composite_alpha: false,
        })
    }

    fn pin(&mut self, frame: DisplayFrame) -> Rc<DisplayFrame> {
        self.leases.0.retain(|lease| lease.strong_count() != 0);
        let frame = Rc::new(frame);
        self.leases.0.push(Rc::downgrade(&frame));
        frame
    }

    pub fn render_display(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        frame: &DisplayFrame,
        settings: &PostSettings<'_>,
    ) -> PostStats {
        let Some(fxaa) = &settings.fxaa else {
            return self.tone_map(ctx, queue, frame, settings, false);
        };
        let packed = ctx.format == COLOR_FORMAT;
        let dithering = packed && settings.finish.dithering;
        let texture = frame.color.texture();
        let raw_ldr = packed
            && !settings.converts_color()
            && texture.format() == wgpu::TextureFormat::Rgba8Unorm
            && [texture.width(), texture.height()] == ctx.size;
        let (work, input, allocated) = if raw_ldr {
            (
                PostStats::default(),
                texture.create_view(&Default::default()),
                false,
            )
        } else {
            let format = if packed && dithering {
                HDR_FORMAT
            } else if packed {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                ctx.format
            };
            let (input, allocated) = self.fxaa.input_target(ctx.device, ctx.size, format);
            let work = self.tone_map(
                &mut GpuPaintContext {
                    encoder: &mut *ctx.encoder,
                    target: &input,
                    format,
                    ..*ctx
                },
                queue,
                frame,
                settings,
                packed,
            );
            (work, input, allocated)
        };
        self.fxaa.resolve(ctx, &input, fxaa, dithering);
        PostStats {
            fxaa_passes: 1,
            fxaa_pixels: ctx.size.into_iter().map(u64::from).product(),
            fxaa_input_allocations: u64::from(allocated),
            ..work
        }
    }

    fn tone_map(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        frame: &DisplayFrame,
        settings: &PostSettings<'_>,
        packed: bool,
    ) -> PostStats {
        let lut = settings.lut.as_ref().and_then(|lut| {
            self.luts
                .get_or_insert_with(|| LutCache::new(ctx.device))
                .bind(ctx.device, queue, lut)
        });
        let mode = usize::from(frame.glow.is_some())
            + 2 * usize::from(lut.is_some())
            + 4 * usize::from(frame.flare.is_some());
        let slot = mode + 8 * usize::from(settings.composite_alpha);
        let format_slot = match (ctx.format, packed) {
            (wgpu::TextureFormat::Rgba8Unorm, true) => 3,
            (HDR_FORMAT, true) => 4,
            (HDR_FORMAT, false) => 1,
            (wgpu::TextureFormat::Rgba32Float, false) => 2,
            _ => 0,
        };
        let pipeline = &mut self.tone_maps[format_slot][slot];
        let display_pipeline_builds = u64::from(pipeline.is_none());
        let tone_map = pipeline.get_or_insert_with(|| {
            ToneMap::new(
                ctx.device,
                ctx.format,
                mode,
                settings.composite_alpha,
                packed,
            )
        });
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
        let mut bytes = Vec::with_capacity(96);
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
        append_floats(&mut bytes, settings.finish.vignette);
        append_floats(
            &mut bytes,
            [
                settings.finish.radius,
                settings.finish.sharpness,
                f32::from(u8::from(settings.finish.dithering)),
                2.0 / (ctx.size[0] as f32).hypot(ctx.size[1] as f32),
            ],
        );
        append_floats(
            &mut bytes,
            [
                settings.flare.as_ref().map_or(0.0, |flare| flare.strength),
                0.0,
                0.0,
                0.0,
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
                resource: wgpu::BindingResource::TextureView(&frame.color),
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
        if let Some(glow) = &frame.glow {
            inputs.extend([
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(glow),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
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
        if let Some(flare) = &frame.flare {
            inputs.push(wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(flare),
            });
        }
        let bindings = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D tone mapping inputs"),
            layout: &tone_map.layout,
            entries: &inputs,
        });
        let mut pass = crate::viewport_gpu_timing::render_pass(
            ctx.encoder,
            &wgpu::RenderPassDescriptor {
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
            },
        );
        pass.set_pipeline(&tone_map.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        PostStats {
            display_resolves: 1,
            display_pipeline_builds,
            lut_resolves: u64::from(lut.is_some()),
            lut_uploads: u64::from(lut.is_some_and(|lut| lut.uploaded)),
            ..Default::default()
        }
    }
}

pub(super) fn color_target(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::TextureView {
    create_render_texture(device, size, format, label).create_view(&wgpu::TextureViewDescriptor {
        format: Some(format),
        ..Default::default()
    })
}

struct ToneMap {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl ToneMap {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        mode: usize,
        composite_alpha: bool,
        packed: bool,
    ) -> Self {
        let linear_output =
            !packed && matches!(format, HDR_FORMAT | wgpu::TextureFormat::Rgba32Float);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D tone mapping"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("display_color.wgsl"),
                    "\n",
                    include_str!("tone_map.wgsl")
                )
                .into(),
            ),
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
                        "fragment_flare",
                        "fragment_bloom_flare",
                        "fragment_flare_lut",
                        "fragment_bloom_flare_lut",
                    ][mode],
                ),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        ("COMPOSITE_ALPHA", f64::from(u8::from(composite_alpha))),
                        ("LINEAR_OUTPUT", f64::from(u8::from(linear_output))),
                        ("PACKED_SIGNAL", f64::from(u8::from(packed))),
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
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
        }
    }
}
