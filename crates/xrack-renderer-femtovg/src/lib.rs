//! wgpu-backed 2D renderer. No windowing dependency and no compiler/MLIR dependency.
#[cfg(target_os = "android")]
mod android_emoji;
mod cell_glyphs;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod color_glyphs;
mod controls;
mod display_list;
mod entry;
mod glyph_masks;
mod image;
mod image_colorize;
mod image_coverage;
#[cfg(target_os = "ios")]
mod ios_font;
mod label_glyphs;
mod layer_compositor;
mod nine_slice;
pub mod preview;
mod preview_pointer;
mod primitive;
mod range;
mod rectangle;
mod retained_geometry;
mod retained_layers;
mod scene3d;
pub use scene3d::SceneDrawStats;
mod retained_text;
mod scroll_images;
mod shader_effect;
mod shadow;
mod styled_ink;
mod styled_text;
mod text;
mod theme;
mod transform;
mod transient_textures;
mod vector;
mod vector_masks;
mod viewport_gpu_timing;
mod window_damage;
#[cfg(target_os = "android")]
#[doc(hidden)]
pub use color_glyphs::GlyphBitmap as AndroidGlyphBitmap;
pub use display_list::DisplayBatchStats;
use display_list::DisplayList;
#[doc(hidden)]
pub use entry::{PointerCaret, TouchSelectionHandle};
/// FemtoVG types accepted and returned by this renderer, including `TextSystem`.
pub use femtovg;
use femtovg::{
    Canvas, Color, FillRule, LayerEffects, Paint, Path, Solidity, renderer::WGPURenderer,
};
use primitive::Primitive;
pub use primitive::{
    Geometry, GpuPaintContext, GpuPassContext, PaintContext, PaintRegistration, register_geometry,
    register_gpu, register_gpu_pass,
};
#[doc(hidden)]
pub use retained_layers::{LayerStats, OpacityAtlasStats};
pub use shader_effect::register_shader_effect;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path as FilePath,
    rc::Rc,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
pub use text::TextSystem;
pub use theme::Theme;
use theme::{Palette, PaletteRole as Role};
use transform::{CanvasTransform, canvas_transform, has_device_scale, has_pixel_aligned_scale};
pub use window_damage::{DamageRect, DamageRegions, WindowDamage};
use xrack_layout::{
    ButtonContent, ComboCompletion, ImagePresentation, Interaction, LayoutNode, Point, Rect, Size,
    SpinGeometry, StackTransitions, TextMeasurer, TreeRowGeometry, corner_radii,
};
use xrack_runtime::{
    Brush, Color as UiColor, ColorScheme, Component, GradientStop, Node, Orientation,
    StandardButtonKind, Value, button_text, combo_editable, group_index, is_button,
    is_multiline_text_input, is_spin_box, is_text_input, is_toggle, is_window_overlay,
    spin_editable, spin_text, text_input_value,
};

#[derive(Clone, Copy)]
struct PaintFrame<'a> {
    input: &'a Interaction,
    scale: f32,
    transitions: Option<&'a StackTransitions>,
    indicator_phase: Option<f32>,
    theme: Palette,
    cleared_background: Option<&'a ClearBackground<'a>>,
    /// Complete paint groups to replay, bounded in physical pixels.
    damage: Option<DamageRegions>,
    prepared_layout: Option<&'a Rc<LayoutNode>>,
}

#[derive(Clone, Copy)]
struct ClearBackground<'a> {
    color: UiColor,
    root: &'a Node,
    surface: Option<&'a Node>,
}

impl ClearBackground<'_> {
    fn covers(&self, node: &Node) -> bool {
        std::ptr::eq(self.root, node)
            || self
                .surface
                .is_some_and(|surface| std::ptr::eq(surface, node))
    }
}

fn color(color: UiColor) -> Color {
    Color::rgba(
        (color.0 >> 24) as u8,
        (color.0 >> 16) as u8,
        (color.0 >> 8) as u8,
        color.0 as u8,
    )
}

fn pixel_aligned(bounds: Rect, scale: f32) -> bool {
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(|value| {
            let pixels = value * scale;
            pixels.is_finite() && pixels == pixels.round()
        })
}

/// The fractional sampling position in a device pixel. Integer translations
/// share it, and the quantization error is less than 1/256 of a pixel.
fn pixel_phase(value: f32) -> u8 {
    ((value - value.floor()) * 256.0).round() as u8
}

fn tint(value: UiColor, factor: f32) -> UiColor {
    let channel = |shift: u32| {
        let c = ((value.0 >> shift) & 255_u32) as f32;
        (if factor >= 0.0 {
            c + (255.0 - c) * factor
        } else {
            c * (1.0 + factor)
        }) as u32
    };
    UiColor(channel(24) << 24 | channel(16) << 16 | channel(8) << 8 | (value.0 & 255))
}

fn brush_paint(node: &Node, name: &str, bounds: Rect, shade: f32) -> Option<Paint> {
    brush_paint_with(node, name, bounds, |value| color(tint(value, shade)))
}

fn brush_paint_with(
    node: &Node,
    name: &str,
    bounds: Rect,
    map: impl Fn(UiColor) -> Color,
) -> Option<Paint> {
    match node.properties.get(name)? {
        Value::Color(value) => Some(Paint::color(map(*value))),
        Value::Brush(brush) => Some(paint_brush_with(brush, bounds, map)),
        _ => None,
    }
}

fn paint_brush(brush: &Brush, bounds: Rect, shade: f32) -> Paint {
    paint_brush_with(brush, bounds, |value| color(tint(value, shade)))
}

fn paint_brush_with(brush: &Brush, bounds: Rect, tinted: impl Fn(UiColor) -> Color) -> Paint {
    let stop = |stop: &GradientStop| (stop.offset as f32, tinted(stop.color));
    let cx = bounds.x + bounds.width * 0.5;
    let cy = bounds.y + bounds.height * 0.5;
    match brush {
        Brush::Solid(value) => Paint::color(tinted(*value)),
        Brush::Linear {
            angle_degrees,
            stops: colors,
        } => {
            let degrees = if angle_degrees.is_finite() {
                angle_degrees.rem_euclid(360.0)
            } else {
                0.0
            };
            let (dx, up) = (degrees as f32).to_radians().sin_cos();
            let dy = -up;
            // Project the corners onto the gradient line so the stops span the whole box.
            let half_span = (dx.abs() * bounds.width + dy.abs() * bounds.height) * 0.5;
            Paint::linear_gradient_stops(
                cx - dx * half_span,
                cy - dy * half_span,
                cx + dx * half_span,
                cy + dy * half_span,
                colors.iter().map(stop),
            )
        }
        Brush::Radial { stops: colors } => {
            let radius = bounds.width.hypot(bounds.height) * 0.5;
            Paint::radial_gradient_stops(cx, cy, 0.0, radius, colors.iter().map(stop))
        }
    }
}

// A full opaque Window fill can be the render pass clear.
fn window_clear_background(
    scene: &Node,
    layout: &LayoutNode,
    viewport: Rect,
    scale: f32,
) -> Option<UiColor> {
    scene.color("background").filter(|background| {
        background.0 & 0xff == 0xff
            && scene.kind == "Window"
            && covers_frame(scene, layout, viewport, scale)
    })
}

fn covers_frame(node: &Node, layout: &LayoutNode, viewport: Rect, scale: f32) -> bool {
    node.opacity() == 1.0
        && layout.visible
        && layout.bounds == viewport
        && layout.clip.intersect(layout.bounds) == viewport
        && pixel_aligned(viewport, scale)
        && layout.local_transform().is_identity()
        && layout.table_pane_clip().is_none()
        && corner_radii(node, layout.bounds) == [0.0; 4]
}

fn first_clear_colored_panel<'a>(
    scene: &'a Node,
    layout: &'a LayoutNode,
    background: UiColor,
) -> Option<&'a Node> {
    if scene.application_window
        || scene.is_item_delegate()
        || layout.table.is_some()
        || layout.allows_overflow()
        || !DisplayList::plain_window(scene)
    {
        return None;
    }
    let (_, panel, item) = layout
        .children_in_paint_order(scene)
        .find(|(_, node, item)| item.visible && node.opacity() != 0.0)?;
    // The rounded panel paints directly onto the just-cleared target.
    // Leave its child clip active even when the background fill is redundant.
    (panel.opacity() == 1.0
        && !panel.children.is_empty()
        && panel.color("background") == Some(background)
        && item.child_clip_shape().is_some()
        && item.local_transform().is_identity()
        && item.table_pane_clip().is_none()
        && rectangle::plain_surface(panel, item))
    .then_some(panel)
}

fn table_frame_background<'a>(
    scene: &'a Node,
    layout: &'a LayoutNode,
    viewport: Rect,
    frame: PaintFrame<'_>,
) -> Option<ClearBackground<'a>> {
    // A plain Window contributes only its own background before its first
    // visible child. A full opaque table can supply both background fills.
    let (table, item) = if scene.kind == "Window" {
        let (_, child, item) = layout
            .children_in_paint_order(scene)
            .find(|(_, _, item)| item.visible)?;
        if child.kind != "TableView"
            || scene.application_window
            || scene.is_item_delegate()
            || layout.table.is_some()
            || !DisplayList::plain_window(scene)
            || !covers_frame(scene, layout, viewport, frame.scale)
            || layout.has_ancestor_clip_chain()
        {
            return None;
        }
        (child, item)
    } else {
        (scene, layout)
    };
    if table.kind != "TableView"
        || table.is_item_delegate()
        || item.has_ancestor_clip_chain()
        || !covers_frame(table, item, viewport, frame.scale)
        || shadow::OuterShadow::for_node(
            table,
            item.bounds,
            frame.scale,
            frame.theme.role(Role::Border),
        )
        .is_some()
    {
        return None;
    }
    let background = match table.properties.get("background") {
        Some(Value::Color(value) | Value::Brush(Brush::Solid(value))) => *value,
        None => frame.theme.role(Role::Surface),
        Some(_) => return None,
    };
    (background.0 & 0xff == 0xff).then_some(ClearBackground {
        color: background,
        root: scene,
        surface: (!std::ptr::eq(table, scene)).then_some(table),
    })
}

fn indicator_phase(elapsed: Duration) -> f32 {
    const PERIOD_NS: u128 = 1_400_000_000;
    (elapsed.as_nanos() % PERIOD_NS) as f32 / PERIOD_NS as f32
}

pub struct Renderer {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    canvas: Canvas<WGPURenderer>,
    canvas_font_context: Rc<()>,
    images: image::Images,
    scroll_images: scroll_images::ScrollImages,
    retained_layers: retained_layers::RetainedLayers,
    layer_compositor: Option<layer_compositor::LayerCompositor>,
    cached_layer_compositing: bool,
    rectangle_batches: rectangle::RectangleBatches,
    geometry_cache: retained_geometry::GeometryCache,
    glyph_masks: glyph_masks::GlyphMasks,
    label_batch_scratch: label_glyphs::BatchScratch,
    render_scene: xrack_runtime::SceneIndex,
    scene_generation: u64,
    inspect_scene: bool,
    scene_changes: xrack_runtime::SceneChanges,
    transient_textures: transient_textures::TransientTextures,
    scene3d: scene3d::SceneRenderer,
    viewport_gpu_timing: viewport_gpu_timing::ViewportGpuTimer,
    viewport_textures: scene3d::retained_view::ViewportTextures,
    scene_texture_sizes: scene3d::retained_view::TextureSizeObservations,
    scene_item_textures: scene3d::item_textures::ItemTextures,
    inline_target: Option<wgpu::Texture>,
    window_damage: window_damage::WindowDamageState,
    geometry_row: Option<xrack_runtime::RowKey>,
    effect_mount: Option<xrack_runtime::RowKey>,
    display_batching: bool,
    display_batch_stats: DisplayBatchStats,
    custom_encoder: Option<wgpu::CommandEncoder>,
    vector_paths: vector::Paths,
    row_layouts: xrack_layout::RowLayoutCache,
    clock: Instant,
    needs_redraw: bool,
    painted_combo_completion: Option<ComboCompletion>,
    inspection_outline: Option<Rect>,
    pub text: TextSystem,
    pub theme: Theme,
    system_color_scheme: ColorScheme,
    system_increased_contrast: bool,
    system_switch_labels: bool,
    system_button_shapes: bool,
    system_differentiate_without_color: bool,
}

fn canvas_renderer(
    instance: &wgpu::Instance,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> WGPURenderer {
    #[cfg(xrack_femtovg_glyph_samples)]
    {
        WGPURenderer::new_with_device_cache(instance, device.clone(), queue.clone())
    }
    #[cfg(not(xrack_femtovg_glyph_samples))]
    {
        let _ = instance;
        WGPURenderer::new(device.clone(), queue.clone())
    }
}

/// GPU resources that can back more than one independent renderer.
#[derive(Clone)]
pub struct RenderDevice {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    system_fonts: Arc<OnceLock<fontdb::Database>>,
}

impl RenderDevice {
    pub async fn request(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: surface,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no compatible GPU adapter: {e}"))?;
        let supported_limits = adapter.limits();
        let mut required_limits =
            wgpu::Limits::default().using_resolution(supported_limits.clone());
        // Programmable materials use samplerless loads and share the device's
        // sampled-texture budget with the built-in material and lighting maps.
        required_limits.max_sampled_textures_per_shader_stage = required_limits
            .max_sampled_textures_per_shader_stage
            .max(scene3d::material_texture_limit(
                supported_limits.max_sampled_textures_per_shader_stage,
            ));
        let features = adapter.features();
        let scroll_immediates = features.contains(wgpu::Features::IMMEDIATES)
            && supported_limits.max_immediate_size >= layer_compositor::IMAGE_MOTION_BYTES;
        let required_limits = wgpu::Limits {
            max_immediate_size: if scroll_immediates {
                layer_compositor::IMAGE_MOTION_BYTES
            } else {
                0
            },
            ..required_limits
        };
        // The iOS Simulator advertises one fewer varyings slot than the WebGPU default.
        #[cfg(target_os = "ios")]
        let required_limits = wgpu::Limits {
            max_inter_stage_shader_variables: required_limits
                .max_inter_stage_shader_variables
                .min(supported_limits.max_inter_stage_shader_variables),
            ..required_limits
        };
        let trace = match std::env::var_os("XRACK_GPU_TRACE_DIR") {
            Some(root) => {
                use std::sync::atomic::{AtomicU64, Ordering};
                static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);
                let index = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
                let folder = std::path::PathBuf::from(root)
                    .join(format!("xrack-{}-{index}", std::process::id()));
                std::fs::create_dir_all(&folder).map_err(|error| {
                    format!(
                        "cannot create GPU trace directory {}: {error}",
                        folder.display()
                    )
                })?;
                wgpu::Trace::Directory(folder)
            }
            None => wgpu::Trace::Off,
        };
        let info = adapter.get_info();
        let mut required_features = if info.backend == wgpu::Backend::Metal
            && info.device_type == wgpu::DeviceType::IntegratedGpu
        {
            features & wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
        } else {
            wgpu::Features::empty()
        };
        required_features |=
            features & (wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE);
        // Timestamp queries are only recorded when viewport statistics are enabled.
        required_features |= features & wgpu::Features::TIMESTAMP_QUERY;
        let normalized_targets = wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        if features.contains(normalized_targets) {
            required_features |= normalized_targets;
        }
        if scroll_immediates {
            required_features |= wgpu::Features::IMMEDIATES;
        }
        if std::env::var_os("XRACK_FORCE_SHADER_WIREFRAME").is_none_or(|value| value != "1") {
            required_features |= features & wgpu::Features::POLYGON_MODE_LINE;
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                trace,
                label: Some("XRack"),
                required_features,
                required_limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("GPU device creation failed: {e}"))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            system_fonts: Arc::new(OnceLock::new()),
        })
    }

    pub fn supports_surface(&self, surface: &wgpu::Surface<'_>) -> bool {
        self.adapter.is_surface_supported(surface)
    }
}

impl Renderer {
    pub async fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, String> {
        Self::with_text(instance, surface, TextSystem::system()?).await
    }

    pub async fn with_text(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        text: TextSystem,
    ) -> Result<Self, String> {
        Self::with_device_and_text(RenderDevice::request(instance, surface).await?, text)
    }

    /// Creates a renderer using the system font snapshot of this device.
    pub fn with_device(gpu: RenderDevice) -> Result<Self, String> {
        let database = gpu
            .system_fonts
            .get_or_init(TextSystem::discover_system_fonts);
        let text = TextSystem::from_system_database(database.clone())?;
        Self::with_device_and_text(gpu, text)
    }

    fn with_device_and_text(gpu: RenderDevice, text: TextSystem) -> Result<Self, String> {
        let RenderDevice {
            instance,
            adapter,
            device,
            queue,
            ..
        } = gpu;
        let scene3d = scene3d::SceneRenderer::new(&adapter, device.features());
        let canvas = Canvas::new_with_text_context(
            canvas_renderer(&instance, &device, &queue),
            text.context().clone(),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            canvas,
            canvas_font_context: text.context_key().clone(),
            glyph_masks: Default::default(),
            label_batch_scratch: Default::default(),
            images: image::Images::default(),
            scroll_images: Default::default(),
            retained_layers: Default::default(),
            layer_compositor: None,
            cached_layer_compositing: true,
            rectangle_batches: Default::default(),
            geometry_cache: retained_geometry::GeometryCache::new(
                std::env::var_os("XRACK_RETAINED_GEOMETRY").is_some_and(|value| value == "1"),
            ),
            transient_textures: Default::default(),
            scene3d,
            viewport_gpu_timing: viewport_gpu_timing::ViewportGpuTimer::default(),
            viewport_textures: Default::default(),
            scene_texture_sizes: Default::default(),
            scene_item_textures: Default::default(),
            render_scene: Default::default(),
            scene_generation: 0,
            inspect_scene: false,
            scene_changes: Default::default(),
            inline_target: None,
            window_damage: Default::default(),
            geometry_row: None,
            effect_mount: None,
            display_batching: true,
            display_batch_stats: DisplayBatchStats::default(),
            custom_encoder: None,
            vector_paths: Default::default(),
            row_layouts: Default::default(),
            clock: Instant::now(),
            needs_redraw: false,
            painted_combo_completion: None,
            inspection_outline: None,
            text,
            theme: Theme::default(),
            system_color_scheme: ColorScheme::System,
            system_increased_contrast: false,
            system_switch_labels: false,
            system_button_shapes: false,
            system_differentiate_without_color: false,
        })
    }

    /// Apply the platform's preferred text scale to measurement and drawing.
    /// Returns whether cached layouts need to be prepared again.
    pub fn set_text_scale(&mut self, scale: f32) -> bool {
        if !self.text.set_text_scale(scale) {
            return false;
        }
        self.invalidate_text_layout();
        true
    }

    /// Select heavier faces for the platform's Bold Text setting.
    /// Returns whether cached layouts need to be prepared again.
    pub fn set_system_bold_text(&mut self, enabled: bool) -> bool {
        if !self.text.set_system_bold_text(enabled) {
            return false;
        }
        self.invalidate_text_layout();
        true
    }

    fn invalidate_text_layout(&mut self) {
        self.submit_opacity_atlas();
        self.row_layouts = Default::default();
        self.retained_layers.clear_images(&mut self.canvas);
    }

    pub fn layout(&mut self, scene: &Node, viewport: Size) -> LayoutNode {
        xrack_layout::layout(scene, viewport, &mut self.text)
    }

    pub fn layout_with_input(
        &mut self,
        scene: &Node,
        viewport: Size,
        input: &Interaction,
    ) -> LayoutNode {
        xrack_layout::layout_with_input(scene, viewport, &mut self.text, input)
    }

    pub fn prepare_scene(
        &mut self,
        component: &impl Component,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
    ) -> (Rc<Node>, LayoutNode) {
        xrack_layout::prepare_scene_with_row_cache(
            component,
            scene,
            viewport,
            &mut self.text,
            input,
            &mut self.row_layouts,
        )
    }

    #[doc(hidden)]
    pub fn prepare_scene_recycling(
        &mut self,
        component: &impl Component,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        previous: Option<LayoutNode>,
    ) -> (Rc<Node>, LayoutNode) {
        self.row_layouts.use_recycled_layout(previous);
        self.row_layouts.set_defer_clipped_text(true);
        let output = self.prepare_scene(component, scene, viewport, input);
        self.row_layouts.set_defer_clipped_text(false);
        self.row_layouts.use_recycled_layout(None);
        output
    }

    /// An animated element was visible in the last frame.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    /// Select the direct vector path for benchmarks and pixel comparisons.
    #[doc(hidden)]
    pub fn set_vector_path_cache_enabled(&mut self, enabled: bool) {
        self.vector_paths.set_enabled(enabled);
    }

    /// Select full rich-text painting for pixel comparisons and benchmarks.
    #[doc(hidden)]
    pub fn set_styled_text_culling_enabled(&mut self, enabled: bool) {
        self.text.styled_ink.get_mut().set_enabled(enabled);
    }

    #[doc(hidden)]
    pub fn styled_text_culled_fragments(&self) -> u64 {
        self.text.styled_ink.borrow().culled
    }

    /// Select a fresh rich-text layout for each revision in tests and benchmarks.
    #[doc(hidden)]
    pub fn set_styled_text_incremental_enabled(&mut self, enabled: bool) {
        self.text.set_styled_text_incremental_enabled(enabled);
    }

    /// Enable experimental paragraph shaping for desktop system-font rich text.
    pub fn set_retained_styled_text_enabled(&mut self, enabled: bool) -> bool {
        if !self.text.set_retained_styled_text_enabled(enabled) {
            return false;
        }
        self.invalidate_text_layout();
        true
    }

    #[doc(hidden)]
    pub fn styled_text_reused_fragments(&self) -> u64 {
        self.text.styled_text_reused_fragments()
    }

    /// Reuse device vertex buffers for compatible opaque rectangle batches.
    pub fn set_retained_geometry_enabled(&mut self, enabled: bool) {
        self.geometry_cache.enabled = enabled;
    }

    /// Return reused batches and uploaded vertex bytes.
    pub fn retained_geometry_stats(&self) -> (u64, u64) {
        (self.geometry_cache.reused, self.geometry_cache.uploaded)
    }

    /// Count geometry cache hits in scopes with a stable model row identity.
    pub fn retained_row_geometry_reuses(&self) -> u64 {
        self.geometry_cache.row_reuses
    }

    #[doc(hidden)]
    pub fn transient_target_reuses(&self) -> u64 {
        self.transient_textures.reuses
    }

    /// Cumulative GPU uploads of resolved joint palettes.
    pub fn scene_skin_uploads(&self) -> u64 {
        self.scene3d.skin_uploads()
    }

    /// Number of distinct morph weight sets uploaded by the 3D renderer.
    pub fn scene_morph_uploads(&self) -> u64 {
        self.scene3d.morph_uploads()
    }

    /// Cumulative uploads of imported 3D mesh vertex buffers.
    pub fn scene_mesh_uploads(&self) -> u64 {
        self.scene3d.mesh_uploads()
    }

    /// Cumulative uploads of transformed mesh instances for SceneView3D.
    pub fn scene_instance_uploads(&self) -> u64 {
        self.scene3d.instance_uploads()
    }

    /// Enable collection of viewport work for subsequent frames.
    pub fn set_viewport_statistics_enabled(&mut self, enabled: bool) {
        self.scene3d.viewport_collector.enable(
            enabled,
            self.device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY),
        );
    }

    pub fn viewport_statistics(&self) -> Option<&xrack_runtime::ViewportStatistics> {
        self.scene3d.viewport_collector.statistics()
    }

    /// Consume the latest completed query readback without waiting for GPU work.
    pub fn poll_viewport_gpu_timing(&mut self) -> bool {
        let (frame, sampled) = self.viewport_gpu_timing.poll(&self.device, &self.queue);
        self.needs_redraw |= sampled;
        let reported = frame.is_some_and(|frame| self.scene3d.viewport_collector.report_gpu(frame));
        sampled || reported
    }

    pub fn viewport_gpu_timing_pending(&self) -> bool {
        self.viewport_gpu_timing.pending()
            && (self.viewport_statistics().is_some() || self.render_stats_pending())
    }

    #[doc(hidden)]
    pub fn render_stats_pending(&self) -> bool {
        self.viewport_gpu_timing.sample_pending()
    }

    /// Color submissions and camera-frustum rejections since renderer creation.
    pub fn scene_draw_stats(&self) -> SceneDrawStats {
        self.scene3d.stats
    }

    /// Cumulative cubemap uploads for SceneView3D environment probes.
    pub fn scene_environment_uploads(&self) -> u64 {
        self.scene3d.environment_uploads()
    }

    /// Cumulative depth passes used to update SceneView3D shadow maps.
    pub fn scene_shadow_passes(&self) -> u64 {
        self.scene3d.shadow_passes()
    }

    #[doc(hidden)]
    pub fn set_scene_observation(&mut self, generation: u64, enabled: bool) {
        if self.scene_generation != generation {
            self.viewport_gpu_timing.invalidate();
            self.scene3d.viewport_collector.set_gpu(None);
        }
        self.scene_generation = generation;
        self.inspect_scene = enabled;
    }

    #[doc(hidden)]
    pub fn scene_changes(&self) -> xrack_runtime::SceneChanges {
        self.scene_changes
    }

    /// Enable or disable merging compatible display list entries.
    pub fn set_display_batching_enabled(&mut self, enabled: bool) {
        self.display_batching = enabled;
    }

    #[doc(hidden)]
    pub fn set_cached_layer_compositing_enabled(&mut self, enabled: bool) {
        self.cached_layer_compositing = enabled;
    }

    pub fn display_batch_stats(&self) -> DisplayBatchStats {
        self.display_batch_stats
    }

    /// Compare rectangle fills and batches with the general painter in tests.
    #[doc(hidden)]
    pub fn set_rectangle_optimization_enabled(&mut self, enabled: bool) {
        self.rectangle_batches.disabled = !enabled;
    }

    /// Compare atlas submission strategies in GPU tests.
    #[doc(hidden)]
    pub fn set_atlas_batching_enabled(&mut self, enabled: bool) {
        self.rectangle_batches.eager_atlas_submission = !enabled;
    }

    #[doc(hidden)]
    pub fn set_fractional_rounded_tile_retention_enabled(&mut self, enabled: bool) {
        self.rectangle_batches.reset_fractional_tiles_each_frame = !enabled;
    }

    /// Compare demand-only and batched capsule atlas updates in benchmarks.
    #[doc(hidden)]
    pub fn set_capsule_warmup_enabled(&mut self, enabled: bool) {
        self.rectangle_batches.capsule_warmup_disabled = !enabled;
    }

    #[doc(hidden)]
    pub fn rounded_tile_rasterizations(&self) -> usize {
        self.rectangle_batches.rounded_tile_rasterizations()
    }

    #[doc(hidden)]
    pub fn rectangle_batch_draws(&self) -> u64 {
        self.rectangle_batches.draws
    }

    #[doc(hidden)]
    pub fn rectangle_palette_uploads(&self) -> u64 {
        self.rectangle_batches.palette_uploads
    }

    #[doc(hidden)]
    pub fn rounded_rectangle_batch_draws(&self) -> u64 {
        self.rectangle_batches.rounded_draws
    }

    pub fn take_painted_combo_completion(&mut self) -> Option<ComboCompletion> {
        self.painted_combo_completion.take()
    }

    /// Set the host color scheme used by windows that request `system`.
    pub fn set_system_color_scheme(&mut self, scheme: ColorScheme) -> bool {
        std::mem::replace(&mut self.system_color_scheme, scheme) != scheme
    }

    /// Increase contrast in the default palette according to the host accessibility setting.
    pub fn set_system_increased_contrast(&mut self, increased: bool) -> bool {
        std::mem::replace(&mut self.system_increased_contrast, increased) != increased
    }

    /// Show on/off symbols in the built-in switch track according to the host preference.
    pub fn set_system_switch_labels(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.system_switch_labels, enabled) != enabled
    }

    /// Outline buttons when the host requests visible button shapes.
    pub fn set_system_button_shapes(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.system_button_shapes, enabled) != enabled
    }

    /// Mark selected tabs and choices with shapes when requested by the host.
    pub fn set_system_differentiate_without_color(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.system_differentiate_without_color, enabled) != enabled
    }

    fn palette(&self, scene: &Node) -> Palette {
        Palette::resolve(
            self.theme,
            scene,
            self.system_color_scheme,
            self.system_increased_contrast,
            self.system_switch_labels,
            self.system_button_shapes,
            self.system_differentiate_without_color,
        )
    }

    /// Draw into a caller-owned render attachment, in logical pixels at the given scale.
    pub fn draw(
        &mut self,
        scene: &Node,
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
    ) -> Result<LayoutNode, String> {
        let size = target.size();
        validate_extent(
            size.width,
            size.height,
            scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        let layout = self.layout_with_input(
            scene,
            Size::new(size.width as f32 / scale, size.height as f32 / scale),
            input,
        );
        self.paint_frame(
            scene,
            &layout,
            target,
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions: None,
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            },
        )?;
        Ok(layout)
    }

    /// Draw a scene with the viewport layout used for input and virtual rows.
    pub fn draw_with_layout(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
    ) -> Result<(), String> {
        self.draw_with_transitions(
            scene,
            layout,
            target,
            scale,
            input,
            &StackTransitions::default(),
        )
    }

    pub fn draw_with_transitions(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<(), String> {
        let size = target.size();
        validate_extent(
            size.width,
            size.height,
            scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        self.paint_frame(
            scene,
            layout,
            target,
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions: Some(transitions),
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            },
        )
    }

    /// Draw a window frame, reusing the previous image for bounded repaint work.
    /// The supplied texture may be a different swapchain image on every call.
    pub fn draw_window(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<WindowDamage, String> {
        self.draw_window_with_layout(scene, (layout, None), target, scale, input, transitions)
    }

    /// Reuse the geometry of a prepared layout while the window keeps it alive.
    pub fn draw_window_prepared(
        &mut self,
        scene: &Node,
        layout: &Rc<LayoutNode>,
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<WindowDamage, String> {
        self.draw_window_with_layout(
            scene,
            (layout, Some(layout)),
            target,
            scale,
            input,
            transitions,
        )
    }

    fn draw_window_with_layout(
        &mut self,
        scene: &Node,
        (layout, prepared_layout): (&LayoutNode, Option<&Rc<LayoutNode>>),
        target: &wgpu::Texture,
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<WindowDamage, String> {
        validate_extent(
            target.width(),
            target.height(),
            scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        self.paint_window(
            scene,
            layout,
            target,
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions: Some(transitions),
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
                cleared_background: None,
                damage: None,
                prepared_layout,
            },
        )
    }

    /// Set the development overlay for the next frame in window coordinates.
    #[doc(hidden)]
    pub fn set_inspection_outline(&mut self, bounds: Option<Rect>) {
        self.inspection_outline = bounds;
    }

    fn paint_frame(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        self.paint_frame_with_transfer(scene, layout, target, frame, None)
    }

    fn paint_frame_with_transfer(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
        transfer: Option<wgpu::CommandBuffer>,
    ) -> Result<(), String> {
        self.text.begin_label_frame();
        self.inline_target = (frame.damage.is_none()
            && scene.kind == "Window"
            && scene.opacity() == 1.0
            && !layout.has_ancestor_clip_chain()
            && layout.child_clip_shape().is_none()
            && !frame.transitions.is_some_and(StackTransitions::is_active))
        .then(|| target.clone());
        let result = self.record_frame(scene, layout, target, frame, transfer);
        self.viewport_textures.finish_frame(result.is_ok());
        self.scene3d.finish_frame(result.is_ok());
        self.needs_redraw |= self
            .scene3d
            .viewport_collector
            .finish_samples(result.is_ok());
        self.needs_redraw |= self.scene_texture_sizes.finish_frame(result.is_ok());
        self.inline_target = None;
        if result.is_err() {
            self.viewport_gpu_timing.start_frame(None);
            self.custom_encoder = None;
            self.scene3d.discard_recorded_passes();
            if let Some(compositor) = &mut self.layer_compositor {
                compositor.discard_pending_instances();
            }
            self.retained_layers.reset_canvas();
            self.canvas_font_context = Rc::new(());
        }
        self.transient_textures.finish_frame(&self.queue);
        result
    }

    fn record_frame(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
        transfer: Option<wgpu::CommandBuffer>,
    ) -> Result<(), String> {
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let warmup_start = self
            .glyph_masks
            .atlas_pipeline_warmup_pending()
            .then(Instant::now);
        self.submit_opacity_atlas();
        let inspection_outline = self.inspection_outline.take();
        let PaintFrame {
            input,
            scale,
            theme,
            ..
        } = frame;
        self.text.set_window(scene);
        let canvas_changed = !Rc::ptr_eq(&self.canvas_font_context, self.text.context_key());
        if canvas_changed {
            self.canvas = self
                .create_auxiliary_canvas()
                .map_err(|error| error.to_string())?;
            self.canvas_font_context = self.text.context_key().clone();
            self.text.label_glyphs.get_mut().reset_canvas();
            self.glyph_masks = Default::default();
            self.images = image::Images::default();
            self.scroll_images = Default::default();
            self.retained_layers.reset_canvas();
            self.scene_item_textures.clear();
        }
        self.glyph_masks.begin_frame();
        self.vector_paths.begin_frame();
        self.rectangle_batches.begin_frame(canvas_changed);
        self.geometry_cache.begin_frame();
        if self.geometry_cache.enabled || self.inspect_scene {
            self.scene_changes = self.render_scene.synchronize(scene, self.scene_generation);
        }
        self.transient_textures.begin_frame();
        self.viewport_textures.begin_frame();
        let gpu_supported = self
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        self.scene3d.begin_frame(gpu_supported);
        self.needs_redraw = false;
        self.poll_viewport_gpu_timing();
        self.viewport_gpu_timing
            .start_frame(self.scene3d.viewport_collector.gpu_frame());
        self.custom_encoder = None;
        #[cfg(any(target_os = "android", target_os = "ios"))]
        self.text.begin_color_glyph_frame(canvas_changed);
        self.retained_layers.begin_frame(&mut self.canvas);
        self.retained_layers
            .sync_font_defaults(&mut self.canvas, self.text.default_style());
        self.painted_combo_completion = None;
        let size = target.size();
        let scrim_bounds = Rect::new(
            0.0,
            0.0,
            size.width as f32 / scale,
            size.height as f32 / scale,
        );
        let window_background = window_clear_background(scene, layout, scrim_bounds, scale);
        self.images.begin_frame();
        self.scroll_images.begin_frame();
        let frame = if self.prepare_scene_item_textures(scene, layout, frame)? {
            self.retained_layers
                .invalidate_live_image_layers(&mut self.canvas);
            PaintFrame {
                damage: None,
                ..frame
            }
        } else {
            frame
        };
        let mut live_paints = Vec::new();
        let composed = if self.cached_layer_compositing
            && self.display_batching
            && self.inline_target.is_some()
            && layer_compositor::supports_target(target)
            && let Some(background) = window_background
        {
            if let Some(composition) =
                DisplayList::cached_layers(self, scene, layout, frame, background)
            {
                self.retain_composited_layers(&composition.layers);
                let encoded = self.encode_cached_layers(
                    target,
                    background,
                    scale,
                    composition.layers,
                    composition.scrolls,
                )?;
                self.needs_redraw |= encoded && composition.animating;
                if encoded {
                    live_paints = composition.live_paints;
                }
                encoded
            } else if !self.rectangle_batches.disabled
                && let Some(layers) = DisplayList::window_opaque_rectangles(
                    scene,
                    layout,
                    scale,
                    layer_compositor::rectangle_instance_limit(&self.device),
                )
            {
                self.encode_cached_layers(target, background, scale, layers, Vec::new())?
            } else if let Some(images) =
                DisplayList::window_image_quads(self, scene, layout, scale)?
            {
                self.encode_window_image_quads(target, background, &images)?
            } else {
                false
            }
        } else {
            false
        };
        let mut canvas_started = false;
        if !composed {
            let cleared_background = self
                .display_batching
                .then(|| table_frame_background(scene, layout, scrim_bounds, frame))
                .flatten()
                .or_else(|| {
                    window_background.map(|color| ClearBackground {
                        color,
                        root: scene,
                        surface: frame
                            .damage
                            .is_none()
                            .then(|| first_clear_colored_panel(scene, layout, color))
                            .flatten(),
                    })
                });
            self.start_frame_canvas(size, scale, &mut canvas_started);
            let clear = color(
                cleared_background
                    .as_ref()
                    .map_or(theme.window, |background| background.color),
            );
            if let Some(regions) = frame.damage {
                for &DamageRect {
                    x,
                    y,
                    width,
                    height,
                } in regions.rectangles()
                {
                    self.canvas.clear_rect(x, y, width, height, clear);
                }
            } else {
                self.canvas.clear_rect(0, 0, size.width, size.height, clear);
            }
            if !is_window_overlay(scene.kind) {
                let root_frame = PaintFrame {
                    cleared_background: cleared_background.as_ref(),
                    ..frame
                };
                self.paint(scene, layout, &mut Vec::new(), root_frame, false)?;
            }
        }
        for paint in live_paints {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            paint.paint(self, frame)?;
        }
        for path in input.overlay_layers(layout) {
            let (Some(node), Some(item)) = (scene.at_path(path), layout.at_path(path)) else {
                continue;
            };
            if !node.is_modal() && !input.allows_popup(layout, path) {
                continue;
            }
            self.start_frame_canvas(size, scale, &mut canvas_started);
            if node.boolean("dim", node.is_modal()) {
                let reveal = input.closing_drawer_scrim(scene, layout, path);
                self.paint_overlay_scrim(scrim_bounds, theme, reveal);
            }
            self.paint(node, item, &mut path.to_vec(), frame, false)?;
        }
        if let Some((node, viewport, reveal)) = input.opening_drawer_preview(scene, layout) {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            if node.boolean("dim", node.is_modal()) {
                self.paint_overlay_scrim(scrim_bounds, theme, reveal);
            }
            let (shell, item, displacement) =
                xrack_layout::layout_drawer_opening(node, viewport, reveal, &mut self.text);
            self.canvas.save();
            self.canvas
                .intersect_scissor(viewport.x, viewport.y, viewport.width, viewport.height);
            self.canvas.translate(displacement.x, displacement.y);
            let result = self.paint(
                &shell,
                &item,
                &mut Vec::new(),
                PaintFrame {
                    input: &Interaction::default(),
                    transitions: None,
                    ..frame
                },
                false,
            );
            self.canvas.restore();
            result?;
        }
        if let Some(menu) = input.choice_menu(scene, layout) {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            self.paint_choice_menu(&menu, frame)?;
        }
        for panel in input.menu_panels(scene, layout) {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            controls::menu_panel(
                &mut self.canvas,
                &self.text,
                theme,
                panel,
                &mut self.images,
                scale,
            )?;
        }
        for handle in self.text.touch_selection_handles(scene, layout, input) {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            handle.paint(&mut self.canvas, color(theme.focus));
        }
        if let Some(popup) = input.tooltip_popup(scene, layout) {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            let viewport = Rect::new(
                0.0,
                0.0,
                size.width as f32 / scale,
                size.height as f32 / scale,
            );
            let (surface, overlay) = xrack_layout::layout_tooltip(popup, viewport, &mut self.text);
            self.paint(
                &surface,
                &overlay,
                &mut Vec::new(),
                PaintFrame {
                    input: &Interaction::default(),
                    scale,
                    theme,
                    transitions: None,
                    indicator_phase: frame.indicator_phase,
                    cleared_background: None,
                    damage: None,
                    prepared_layout: None,
                },
                false,
            )?;
        }
        if let Some(bounds) =
            inspection_outline.filter(|rect| rect.width >= 3.0 && rect.height >= 3.0)
        {
            self.start_frame_canvas(size, scale, &mut canvas_started);
            let rect = bounds.inset(1.5);
            let mut outline = Path::new();
            outline.rect(rect.x, rect.y, rect.width, rect.height);
            self.canvas.fill_path(
                &outline,
                &Paint::color(femtovg::Color::rgba(55, 186, 255, 32)),
            );
            let mut stroke = Paint::color(femtovg::Color::rgb(83, 205, 255));
            stroke.set_line_width(3.0);
            self.canvas.stroke_path(&outline, &stroke);
        }
        self.needs_redraw |=
            self.scene3d
                .preload_resources(scene, layout, &self.device, &self.queue);
        self.rectangle_batches.warm_capsules(&self.queue);
        self.rectangle_batches.upload(&mut self.canvas)?;
        #[cfg(not(xrack_femtovg_glyph_samples))]
        if !canvas_changed
            && warmup_start
                .is_some_and(|started| started.elapsed() <= std::time::Duration::from_millis(1))
        {
            // Spend the one-time setup cost in a later short frame, with no
            // extra submission or change to the presented image.
            self.glyph_masks.warm_atlas_pipelines(&mut self.canvas);
        }
        self.viewport_gpu_timing
            .finish(&self.device, &mut self.custom_encoder);
        self.submit_canvas_with_transfer(target, transfer);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        self.text.end_color_glyph_frame(&mut self.canvas);
        self.images.end_frame(&mut self.canvas);
        self.retained_layers
            .finish_frame(&mut self.canvas, &self.render_scene);
        Ok(())
    }

    fn create_auxiliary_canvas(&self) -> Result<Canvas<WGPURenderer>, femtovg::ErrorKind> {
        #[cfg(xrack_femtovg_glyph_samples)]
        let renderer = self.canvas.renderer().new_sibling();
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let renderer = WGPURenderer::new(self.device.clone(), self.queue.clone());
        Canvas::new_with_text_context(renderer, self.text.context().clone())
    }

    fn start_frame_canvas(&mut self, size: wgpu::Extent3d, scale: f32, started: &mut bool) {
        if !*started {
            // A viewport can revisit the same clip after several frames of
            // motion. Retain the masks within the existing byte budget.
            #[cfg(xrack_femtovg_glyph_samples)]
            self.canvas.set_clip_mask_retention_frames(32);
            self.canvas.set_size(size.width, size.height, scale);
            self.canvas.reset();
            self.canvas.scale(scale, scale);
            // Populate the atlas before the first live paint. A frame drawn
            // entirely by the compositor can leave the work queued.
            if self.display_batching && self.glyph_masks.has_pending() {
                self.glyph_masks.prepare(&mut self.canvas);
            }
            *started = true;
        }
    }

    fn submit_canvas(&mut self, target: &wgpu::Texture) {
        self.submit_canvas_with_transfer(target, None);
    }

    fn submit_canvas_with_transfer(
        &mut self,
        target: &wgpu::Texture,
        transfer: Option<wgpu::CommandBuffer>,
    ) {
        #[cfg(xrack_femtovg_glyph_samples)]
        if std::mem::take(&mut self.rectangle_batches.plain_fills_pending) {
            self.canvas.batch_opacity_atlas_fills();
        }
        if self.rectangle_batches.eager_atlas_submission {
            self.submit_opacity_atlas();
        }
        xrack_runtime::update_profile::submit_gpu();
        let rounded = self.rectangle_batches.take_rounded_commands();
        let opacity = self.take_opacity_atlas_commands();
        let commands = self.custom_encoder.take().map(wgpu::CommandEncoder::finish);
        let image_coverage = self.images.encode_coverage(&self.device, &self.queue);
        // Rounded tiles feed the opacity atlas, and both feed the canvas.
        // The window can read the backing image only after the canvas writes it.
        if transfer.is_some() {
            xrack_runtime::update_profile::transfer_window_image();
        }
        self.queue.submit(
            rounded
                .into_iter()
                .chain(opacity)
                .chain(commands)
                .chain(image_coverage)
                .chain(self.canvas.flush_to_output(target))
                .chain(transfer),
        );
    }

    fn paint(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        if node.scene_texture_only() {
            return Ok(());
        }
        self.paint_subtree(node, layout, path, frame, in_layout)
    }

    fn paint_subtree(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let previous = self.geometry_row;
        let previous_mount = self.effect_mount;
        let row = node.routing_row_key().or(previous);
        self.geometry_row = row;
        self.effect_mount = node.dynamic_mount_key().or(previous_mount);
        // The damage plan expands to a viewport's complete paint extent.
        // Replay that viewport with its usual clips and batch boundaries.
        let frame = if frame.damage.is_some() && window_damage::is_paint_viewport(node.kind) {
            PaintFrame {
                damage: None,
                ..frame
            }
        } else {
            frame
        };
        let result = self.paint_with_transform(node, layout, path, frame, in_layout);
        self.geometry_row = previous;
        self.effect_mount = previous_mount;
        result
    }

    fn paint_with_transform(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let local = layout.local_transform();
        let pane = layout.table_pane_clip();
        if local.is_identity() && pane.is_none() {
            return self.paint_in_space(node, layout, path, frame, in_layout);
        }
        self.canvas.save();
        if let Some((bounds, transform)) = pane {
            let corners = transform.map_corners(bounds);
            let mut mask = Path::new();
            mask.move_to(corners[0].x, corners[0].y);
            for point in &corners[1..] {
                mask.line_to(point.x, point.y);
            }
            mask.close();
            self.canvas.clip_path(&mask, FillRule::NonZero);
        }
        if !local.is_identity() {
            let transform = local.then(canvas_transform(&self.canvas));
            transform::set_canvas_transform(&mut self.canvas, transform);
        }
        let result = self.paint_in_space(node, layout, path, frame, in_layout);
        self.canvas.restore();
        result
    }

    fn paint_in_space(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let opacity = node.opacity();
        if !layout.visible || opacity == 0.0 {
            return Ok(());
        }
        if !self.rectangle_batches.disabled
            && node.kind == "Rectangle"
            && has_device_scale(canvas_transform(&self.canvas), frame.scale)
            && let Some(rectangle) = rectangle::Solid::from_node(node, layout, frame.scale)
            && !rectangle.uses_mask()
        {
            if rectangle.visible() {
                self.draw_primitive(Primitive::Rectangle(rectangle))?;
            }
            return Ok(());
        }
        let content = layout.clip.intersect(layout.bounds);
        let overflow = layout.allows_overflow();
        if (content.width <= 0.0 || content.height <= 0.0)
            && !overflow
            && node.can_skip_clipped_layout_group()
        {
            return Ok(());
        }
        let shadow = shadow::OuterShadow::clipped_for_node(
            node,
            layout,
            frame.scale,
            frame.theme.role(Role::Border),
        );
        let retain_opaque_group = self.geometry_cache.enabled
            && !(frame.damage.is_some() && window_damage::can_clip_surface(node, layout, frame))
            && node.kind == "Rectangle"
            && node
                .color("background")
                .is_some_and(|color| color.0 & 0xff == 0xff)
            && !node.children.is_empty()
            && corner_radii(node, layout.bounds) == [0.0; 4]
            && content == layout.bounds;
        if !overflow
            && opacity.is_finite()
            && (opacity < 1.0
                || retain_opaque_group
                || node.boolean("cache-rendering-hint", false)
                || shadow
                    .as_ref()
                    .is_some_and(|(shadow, _)| shadow.is_filtered()))
            && self.paint_retained_opacity(node, layout, path, frame, in_layout, shadow.as_ref())?
        {
            return Ok(());
        }
        if !overflow
            && opacity < 1.0
            && self.paint_transient_opacity(
                node,
                layout,
                path,
                frame,
                in_layout,
                retained_layers::OpacitySource::Transient(shadow.as_ref()),
            )?
        {
            return Ok(());
        }
        let mask = self.prepare_shadow_mask(shadow.as_ref(), layout, frame.scale, false)?;
        let shadow = shadow.as_ref().map(|(source, clip)| shadow::ShadowPaint {
            source,
            clip: *clip,
            mask,
        });
        if overflow {
            // The group must cover descendants outside the box, including their
            // paint-only effects. The enclosing viewport is its hard bound.
            return self.paint_group(layout.clip, opacity, |renderer| {
                if let Some(shadow) = shadow {
                    shadow.draw(&mut renderer.canvas);
                }
                renderer.paint_element(node, layout, path, frame, in_layout)
            });
        }
        let clip = shadow
            .as_ref()
            .map_or(content, |shadow| shadow::union(content, shadow.clip));
        self.paint_group(clip, opacity, |renderer| {
            renderer.paint_element_with_shadow(node, layout, path, frame, in_layout, shadow)
        })
    }

    fn paint_element_with_shadow(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
        shadow: Option<shadow::ShadowPaint<'_>>,
    ) -> Result<(), String> {
        if let Some(shadow) = shadow {
            shadow.draw(&mut self.canvas);
            self.paint_group(layout.clip.intersect(layout.bounds), 1.0, |renderer| {
                renderer.paint_element(node, layout, path, frame, in_layout)
            })
        } else {
            self.paint_element(node, layout, path, frame, in_layout)
        }
    }

    fn paint_overlay_scrim(&mut self, bounds: Rect, theme: Palette, reveal: f32) {
        let mut backdrop = Path::new();
        backdrop.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        let mut tint = color(theme.role(Role::Scrim));
        tint.a *= reveal;
        self.canvas.fill_path(&backdrop, &Paint::color(tint));
    }

    fn paint_group(
        &mut self,
        clip: Rect,
        opacity: f32,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        if opacity == 0.0 || clip.width <= 0.0 || clip.height <= 0.0 {
            return Ok(());
        }
        self.canvas.save();
        let transform = canvas_transform(&self.canvas);
        if transform.axis_scales().is_some() {
            self.canvas
                .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        } else {
            if opacity < 1.0 {
                // Bound the offscreen allocation in device space while keeping
                // the exact rotated clip for the final composite.
                let capture = transform.map_rect(clip).inset(-1.0);
                self.canvas.reset_transform();
                self.canvas
                    .intersect_scissor(capture.x, capture.y, capture.width, capture.height);
                self.canvas.set_transform(&transform::to_femtovg(transform));
            }
            let mut mask = Path::new();
            mask.rect(clip.x, clip.y, clip.width, clip.height);
            self.canvas.clip_path(&mask, FillRule::NonZero);
        }
        let layer = opacity < 1.0;
        let inline_target = layer.then(|| self.inline_target.take());
        let previous_mask_deferral = self.glyph_masks.defer_preparation;
        // An isolated layer may suppress its draws. Pending masks should
        // only become ready after their coverage has actually been recorded.
        self.glyph_masks.defer_preparation |= layer;
        let previous_passthrough = self.retained_layers.vectors.opacity_passthrough;
        if layer
            && !self
                .canvas
                .begin_layer(&LayerEffects::new().with_opacity(opacity))
        {
            // In a pass-through layer FemtoVG multiplies alpha into each draw.
            // A retained mask blends the coverage of several strokes at once.
            self.retained_layers.vectors.opacity_passthrough = true;
        }
        let result = draw(self);
        self.glyph_masks.defer_preparation = previous_mask_deferral;
        self.retained_layers.vectors.opacity_passthrough = previous_passthrough;
        if layer {
            self.canvas.end_layer();
        }
        if let Some(target) = inline_target {
            self.inline_target = target;
        }
        self.canvas.restore();
        result
    }

    fn paint_choice_menu(
        &mut self,
        menu: &xrack_layout::ChoiceMenu<'_>,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let panel = controls::choice_surface(&mut self.canvas, menu, frame.theme);
        let input = Interaction::default();
        let option_frame = PaintFrame {
            input: &input,
            transitions: None,
            ..frame
        };
        let mut artwork = menu.control.combo_option_renderer.as_ref().map(|template| {
            template
                .render_visible((0..menu.rows.len()).filter_map(|offset| menu.option_input(offset)))
                .into_iter()
        });
        self.paint_group(menu.bounds, 1.0, |renderer| {
            if menu.popup_style().is_some() {
                renderer.canvas.clip_path(&panel, FillRule::NonZero);
            }
            for offset in 0..menu.rows.len() {
                let bounds = controls::choice_content_bounds(menu, frame.theme, offset);
                if let Some(node) = artwork.as_mut().and_then(Iterator::next).flatten() {
                    let layout = xrack_layout::layout_choice_option(
                        &node,
                        bounds,
                        menu.layout_direction,
                        menu.row_enabled(offset),
                        &mut renderer.text,
                    );
                    renderer.paint_group(bounds, 1.0, |renderer| {
                        renderer.paint(&node, &layout, &mut Vec::new(), option_frame, false)
                    })?;
                } else {
                    controls::choice_row(
                        &mut renderer.canvas,
                        &renderer.text,
                        frame.theme,
                        menu,
                        offset,
                        bounds,
                    )?;
                }
                if frame.theme.differentiate_without_color {
                    controls::choice_selection_marks(
                        &mut renderer.canvas,
                        frame.theme,
                        menu,
                        offset,
                        bounds,
                    );
                }
            }
            if let Some(indicator) = menu.scroll_indicator() {
                controls::choice_indicator(&mut renderer.canvas, indicator, frame.theme);
            }
            Ok(())
        })?;
        controls::choice_border(&mut self.canvas, menu, frame.theme);
        Ok(())
    }

    fn paint_element(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let content = layout.clip.intersect(layout.bounds);
        if layout.allows_overflow() {
            self.paint_group(content, 1.0, |renderer| {
                renderer.paint_surface(node, layout, path, frame, in_layout)
            })?;
        } else if let Some(damage) = frame.damage
            && window_damage::can_clip_surface(node, layout, frame)
        {
            // Large opaque surfaces no longer expand the damage plan.
            // Keep each fill's original geometry and restrict its writes.
            for rect in damage.rectangles() {
                let clip = rect.as_rect().scaled(1.0 / frame.scale).intersect(content);
                if clip.width <= 0.0 || clip.height <= 0.0 {
                    continue;
                }
                self.canvas.save();
                self.canvas
                    .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
                let result = self.paint_surface(node, layout, path, frame, in_layout);
                self.canvas.restore();
                result?;
            }
        } else {
            self.paint_surface(node, layout, path, frame, in_layout)?;
        }
        if node.kind == "Menu" {
            return Ok(());
        }
        let visible_viewport = layout.scroll_viewport(Orientation::Vertical);
        if node.kind == "ScrollView" && visible_viewport != layout.bounds {
            self.paint_group(visible_viewport.intersect(layout.clip), 1.0, |renderer| {
                renderer.paint_children(node, layout, path, frame)
            })?;
        } else if let Some(close) = layout.tab_close
            && node.kind == "TabButton"
        {
            let content =
                xrack_layout::tab_content_rect(layout.bounds, close).intersect(layout.clip);
            self.paint_group(content, 1.0, |renderer| {
                renderer.paint_children(node, layout, path, frame)
            })?;
        } else if let Some(clip) = layout.child_clip_shape() {
            let mut path_clip = Path::new();
            append_rect(&mut path_clip, clip.bounds, clip.radii);
            self.canvas.save();
            self.canvas.clip_path(&path_clip, FillRule::NonZero);
            let result = self.paint_children(node, layout, path, frame);
            self.canvas.restore();
            result?;
        } else {
            self.paint_children(node, layout, path, frame)?;
        }
        if frame.theme.button_shapes && is_button(node.kind) {
            let bounds = layout.bounds;
            let width = 1.5_f32.min(bounds.width.min(bounds.height) * 0.5);
            let tint = if layout.enabled {
                frame.theme.text
            } else {
                frame.theme.disabled_text
            };
            self.canvas.fill_path(
                &border_path(bounds, corner_radii(node, bounds), width),
                &Paint::color(color(tint)),
            );
        }
        if frame.theme.differentiate_without_color
            && matches!(node.kind, "TabWidget" | "TabBar")
            && let Some(index) = group_index(node)
            && let Some(tab) = node.children.get(index)
            && let Some(page) = layout.child_at(index)
            && let Some(header) = page
                .tab_header
                .or_else(|| (node.kind == "TabBar" && page.visible).then_some(page.bounds))
        {
            let clip = header.intersect(content);
            self.paint_group(clip, tab.opacity(), |renderer| {
                controls::tab_selection_mark(
                    &mut renderer.canvas,
                    header,
                    frame.theme,
                    page.enabled,
                );
                Ok(())
            })?;
        }
        for header in layout.list_section_headers() {
            let clip = content.intersect(header.bounds);
            if clip.width > 0.0 && clip.height > 0.0 {
                self.paint_group(clip, 1.0, |renderer| {
                    if let Some((visual, item)) = header.content() {
                        renderer.paint(
                            visual,
                            item,
                            &mut Vec::new(),
                            PaintFrame {
                                input: &Interaction::default(),
                                transitions: None,
                                ..frame
                            },
                            false,
                        )
                    } else {
                        controls::list_section_header(
                            &mut renderer.canvas,
                            &renderer.text,
                            frame.theme,
                            node,
                            header,
                            layout.layout_direction,
                        )
                    }
                })?;
            }
        }
        if layout
            .table
            .as_ref()
            .is_some_and(|table| table.row_header.width > 0.0)
        {
            let hovered = frame
                .input
                .hovered
                .as_ref()
                .and_then(|target| target.path.strip_prefix(path.as_slice()))
                .and_then(|rest| match rest {
                    [slot] => Some(*slot),
                    _ => None,
                });
            let resize = frame.input.table_resize_row(path);
            let moving = frame.input.table_row_move_source(path, node);
            controls::table_row_headers(
                &mut self.canvas,
                &self.text,
                frame.theme,
                node,
                layout,
                |slot, key| {
                    (
                        hovered == Some(slot) || moving.is_some() && moving == key,
                        resize.is_some() && resize == key,
                    )
                },
            )?;
        }
        if layout.allows_overflow() {
            self.paint_group(content, 1.0, |renderer| {
                renderer.paint_chrome(node, layout, path, frame)
            })
        } else {
            self.paint_chrome(node, layout, path, frame)
        }
    }

    fn paint_surface(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let PaintFrame {
            input,
            scale,
            indicator_phase,
            theme,
            ..
        } = frame;
        let bounds = layout.bounds;
        if node.kind == "ToolSeparator" {
            let padding = node.length("padding").unwrap_or(6.0).max(0.0);
            let stroke = node.length("stroke-width").unwrap_or(1.0).max(0.0);
            let line = match node.orientation() {
                Orientation::Horizontal => {
                    let padding = padding.min(bounds.width * 0.5);
                    let stroke = stroke.min(bounds.height);
                    Rect::new(
                        bounds.x + padding,
                        bounds.y + (bounds.height - stroke) * 0.5,
                        bounds.width - 2.0 * padding,
                        stroke,
                    )
                }
                Orientation::Vertical => {
                    let padding = padding.min(bounds.height * 0.5);
                    let stroke = stroke.min(bounds.width);
                    Rect::new(
                        bounds.x + (bounds.width - stroke) * 0.5,
                        bounds.y + padding,
                        stroke,
                        bounds.height - 2.0 * padding,
                    )
                }
            };
            if line.width > 0.0 && line.height > 0.0 {
                let foreground = node.color("color").unwrap_or_else(|| {
                    if layout.enabled {
                        theme.role(Role::Border)
                    } else {
                        theme.disabled_text
                    }
                });
                let mut path = Path::new();
                path.rect(line.x, line.y, line.width, line.height);
                self.canvas
                    .fill_path(&path, &Paint::color(color(foreground)));
            }
            return Ok(());
        }
        let group_header =
            matches!(node.kind, "GroupBox" | "Expander").then(|| layout.group_header(node));
        if node.kind == "SplitHandle" {
            if node.children.is_empty() {
                controls::split_handle(
                    &mut self.canvas,
                    node,
                    bounds,
                    theme,
                    layout.enabled,
                    input.is_hovered(path) || input.is_focused(path),
                );
            }
            return Ok(());
        }
        if node.kind == "Menu" {
            return controls::menu_header(
                &mut self.canvas,
                &self.text,
                theme,
                node,
                layout,
                input.menu_header_open(path) || input.is_hovered(path) || input.is_focused(path),
            );
        }
        let is_button = is_button(node.kind);
        let item_delegate = node.is_item_delegate();
        let is_cell = matches!(node.kind, "TableCell" | "TreeCell");
        let editable_combo = combo_editable(node);
        let is_input =
            is_text_input(node.kind) || editable_combo || is_cell && input.table_cell_editing(path);
        let boxed = is_input
            || matches!(
                node.kind,
                "SpinBox"
                    | "DoubleSpinBox"
                    | "ComboBox"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "TreeView"
                    | "Dialog"
                    | "PopupWindow"
                    | "GroupBox"
                    | "Expander"
                    | "Pane"
                    | "ControlFrame"
                    | "Drawer"
                    | "MenuBar"
                    | "ToolBar"
                    | "Calendar"
                    | "MonthGrid"
                    | "Tumbler"
            );
        let use_row_surface = item_delegate
            && (!is_button
                || !node.properties.contains_key("primary")
                    && !node.properties.contains_key("flat"));
        let checked = is_button
            && (((node.kind == "DelayButton" || node.boolean("checkable", false))
                && node.boolean("checked", false))
                || (item_delegate && !use_row_surface && node.boolean("highlighted", false)));
        let hovered = (is_button || item_delegate) && input.is_hovered(path);
        let pressed = (is_button || item_delegate)
            && (input.is_pressed(path)
                || node
                    .button_press_handle
                    .as_ref()
                    .is_some_and(xrack_runtime::ButtonPressHandle::is_synthetic_pressed));
        let field = is_input || is_spin_box(node.kind) || node.kind == "ComboBox";
        let mut button_text_color = theme.text;
        let background = if frame
            .cleared_background
            .is_some_and(|background| background.covers(node))
        {
            None
        } else if use_row_surface {
            brush_paint(node, "background", bounds, 0.0).or_else(|| {
                let role = if node.boolean("highlighted", false) || checked {
                    Some(Role::RowSelection)
                } else if layout.enabled && (hovered || pressed) {
                    Some(Role::RowHover)
                } else {
                    None
                };
                role.map(|role| Paint::color(color(theme.role(role))))
            })
        } else if !is_button {
            brush_paint(node, "background", bounds, 0.0).or_else(|| {
                let surface = if field && theme.is_light() {
                    Role::InsetSurface
                } else {
                    Role::Surface
                };
                boxed.then(|| Paint::color(color(theme.role(surface))))
            })
        } else if !layout.enabled {
            let shade = if checked { -0.2 } else { 0.0 };
            (!node.boolean("flat", false) || checked)
                .then(|| Paint::color(color(tint(theme.disabled, shade))))
        } else {
            let shade = if pressed {
                -0.18
            } else if hovered {
                0.12
            } else {
                0.0
            } - if checked { 0.28 } else { 0.0 };
            brush_paint(node, "background", bounds, shade).or_else(|| {
                let kind = node.standard_button_kind();
                let primary = node.boolean("primary", kind.is_none());
                let (base, foreground) = match kind {
                    Some(StandardButtonKind::Discard) => (
                        theme.role(Role::Danger),
                        if theme.is_light() {
                            UiColor(0xffffffff)
                        } else {
                            theme.text
                        },
                    ),
                    _ if !primary => (theme.role(Role::SecondaryButton), theme.text),
                    _ => (theme.button, theme.on_accent(theme.text)),
                };
                let flat = node.boolean(
                    "flat",
                    node.kind == "ToolButton" && !node.boolean("primary", false),
                );
                (!flat || checked || hovered || pressed).then(|| {
                    button_text_color = foreground;
                    Paint::color(color(tint(base, shade)))
                })
            })
        };
        let radii = corner_radii(node, bounds);
        if let Some(background) = background {
            rectangle::fill_surface(&mut self.canvas, bounds, radii, background);
        }
        let image_scale = scale * layout.world_transform().max_scale();
        shadow::draw_inner(&mut self.canvas, node, bounds, radii, image_scale);
        if node.kind == "DelayButton" {
            let (fraction, pending) = input.delay_progress(path).unwrap_or((0.0, false));
            let fraction = if checked { 1.0 } else { fraction };
            if fraction > 0.0 && layout.enabled {
                self.canvas.save();
                self.canvas.intersect_scissor(
                    bounds.x,
                    bounds.y,
                    bounds.width * fraction,
                    bounds.height,
                );
                let mut path = Path::new();
                append_rect(&mut path, bounds, radii);
                let tint = node
                    .color("progress-color")
                    .unwrap_or(theme.role(Role::DelayProgress));
                self.canvas.fill_path(&path, &Paint::color(color(tint)));
                self.canvas.restore();
            }
            self.needs_redraw |= pending && layout.enabled;
        }
        if let Some(size) = layout.dialog_title_font_size() {
            self.canvas.save();
            self.canvas.intersect_scissor(
                bounds.x,
                bounds.y,
                bounds.width,
                layout.dialog_title_height(node),
            );
            let result = controls::draw_title(
                &mut self.canvas,
                &self.text,
                node,
                bounds,
                node.color("color").unwrap_or(theme.text),
                size,
                layout.dialog_title_lines(),
            );
            self.canvas.restore();
            result?;
        }
        if let Some(header) =
            group_header.filter(|header| header.width > 0.0 && header.height > 0.0)
        {
            self.canvas.save();
            self.canvas
                .intersect_scissor(header.x, header.y, header.width, header.height);
            let result = if node.kind == "Expander" {
                controls::expander_header(&mut self.canvas, &self.text, theme, node, header, layout)
            } else if node.group_checkable() {
                entry::toggle(&mut self.canvas, &self.text, theme, node, header, layout)
            } else {
                let foreground = if layout.enabled {
                    node.color("color").unwrap_or(theme.text)
                } else {
                    theme.disabled_text
                };
                if let Some((size, lines)) = layout.group_title() {
                    controls::draw_title(
                        &mut self.canvas,
                        &self.text,
                        node,
                        header,
                        foreground,
                        size,
                        lines,
                    )
                } else {
                    let size = self.text.font_size(node);
                    self.text
                        .fill_text(
                            &mut self.canvas,
                            header.x,
                            header.y + ((header.height - size * 1.4) * 0.5).max(0.0),
                            node.text("title").unwrap_or(""),
                            &self.text.paint_node(node, color(foreground), false),
                        )
                        .map(|_| ())
                        .map_err(|error| format!("group title drawing failed: {error}"))
                }
            };
            self.canvas.restore();
            result?;
        }
        let border = node
            .length("border-width")
            .unwrap_or(
                if node.kind == "ControlFrame"
                    || group_header.is_some()
                    || field && theme.outlines_fields()
                {
                    1.0
                } else {
                    0.0
                },
            )
            .max(0.0)
            .min(bounds.width.min(bounds.height));
        if border > 0.0 {
            let path = border_path(bounds, radii, border);
            let paint = brush_paint(node, "border-color", bounds, 0.0)
                .unwrap_or_else(|| Paint::color(color(theme.role(Role::Border))));
            self.canvas.fill_path(&path, &paint);
        }
        if (is_button
            || boxed
            || is_toggle(node.kind)
            || is_cell
            || matches!(
                node.kind,
                "Slider"
                    | "ColorPlane"
                    | "RadioButton"
                    | "SwipeView"
                    | "PageIndicator"
                    | "FocusScope"
            ))
            && input.is_focused(path)
            && layout.enabled
        {
            let outline = if is_cell && input.table_cell_edit_invalid(node, path) {
                theme.role(Role::Danger)
            } else {
                theme.focus
            };
            let mut paint = Paint::color(color(outline));
            paint.set_line_width(2.0);
            let mut path = Path::new();
            let focus = group_header.unwrap_or(bounds);
            append_rect(
                &mut path,
                focus.inset(2.0),
                corner_radii(node, focus).map(|r| (r - 2.0).max(0.0)),
            );
            self.canvas.stroke_path(&path, &paint);
        }
        if node.kind == "MediaPlayer" {
            if let (Some(media), Some(frame)) = (&node.media, &node.media_frame) {
                self.images.draw_video(
                    &mut self.canvas,
                    media.key(),
                    frame,
                    bounds,
                    ImagePresentation::for_node(node, frame.size(), in_layout),
                )?;
            }
        } else if let Some(texture) = node
            .image_texture_input()
            .and_then(|name| self.scene3d.item_color_texture(name))
        {
            let texture = self.viewport_display_texture(texture);
            let size = [texture.width(), texture.height()];
            image::draw_live_texture(
                &mut self.canvas,
                texture,
                bounds,
                ImagePresentation::for_node(node, size, in_layout),
                image::Colorize::from_value(node.properties.get("colorize")),
            )?;
        } else if matches!(node.kind, "Image" | "AnimatedImage")
            && let Some(image) = node.image_asset("source")
        {
            let image = if node.kind == "AnimatedImage" {
                image.frame(node.int("current-frame").unwrap_or_default().max(0) as usize)
            } else {
                image
            };
            let presentation = ImagePresentation::for_node(node, image.size(), in_layout);
            self.draw_primitive(Primitive::Image {
                bounds,
                clip: None,
                quad: None,
                asset: image,
                presentation,
                colorize: image::Colorize::from_value(node.properties.get("colorize")),
                scale: image_scale,
            })?;
        } else if node.kind == "Path" {
            self.draw_primitive(Primitive::Vector {
                node,
                layout,
                scale,
            })?;
        } else if node.kind == "SceneView3D" {
            self.paint_scene3d(node, layout, path, scale)?;
        } else if node.kind == "CustomRender" {
            self.paint_custom(node, layout, path, scale)?;
        } else if node.kind == "ColorPlane" {
            controls::color_plane(&mut self.canvas, node, bounds);
        } else if node.kind == "ColorPreview" {
            controls::color_preview(
                &mut self.canvas,
                bounds,
                node.color("color").unwrap_or_default(),
            );
        }
        if node.kind == "RadioGroup" {
            let tint = if layout.enabled {
                theme.text
            } else {
                theme.disabled_text
            };
            controls::group_title(
                &mut self.canvas,
                &self.text,
                node,
                bounds,
                tint,
                layout.layout_direction,
            )?;
        } else if matches!(node.kind, "TabWidget" | "TabBar") {
            let selected = group_index(node);
            let standalone = node.kind == "TabBar";
            for (index, tab, page) in layout.children_in_paint_order(node) {
                let Some(header) = page
                    .tab_header
                    .or_else(|| (standalone && page.visible).then_some(page.bounds))
                else {
                    continue;
                };
                let clip = header.intersect(layout.clip).intersect(bounds);
                if clip.width <= 0.0 || clip.height <= 0.0 {
                    continue;
                }
                path.push(index);
                let result = self.paint_group(clip, tab.opacity(), |renderer| {
                    controls::tab(
                        &mut renderer.canvas,
                        &renderer.text,
                        theme,
                        header,
                        tab.text(if standalone { "text" } else { "title" })
                            .unwrap_or(""),
                        tab,
                        controls::TabAppearance {
                            selected: selected == Some(index),
                            enabled: page.enabled,
                            focused: input.is_focused(path),
                            close: page.tab_close,
                            close_hovered: input.tab_close_hovered(path),
                            direction: page.layout_direction,
                        },
                    )
                });
                path.pop();
                result?;
            }
        }
        if let Some(wheel) = &layout.tumbler {
            if node.kind == "Tumbler" || node.combo_option_renderer.is_none() {
                controls::tumbler_background(&mut self.canvas, theme, wheel, layout.enabled);
            }
            let input = Interaction::default();
            let option_frame = PaintFrame {
                input: &input,
                transitions: None,
                ..frame
            };
            for row in &wheel.rows {
                if let Some((content, row_layout)) = row.content() {
                    self.paint(content, row_layout, &mut Vec::new(), option_frame, false)?;
                } else {
                    controls::tumbler_row(
                        &mut self.canvas,
                        &self.text,
                        theme,
                        node,
                        row,
                        layout.enabled,
                    )?;
                }
            }
        } else if matches!(
            node.kind,
            "Calendar" | "MonthGrid" | "DayOfWeekRow" | "WeekNumberColumn"
        ) {
            controls::calendar(
                &mut self.canvas,
                &self.text,
                theme,
                node,
                &xrack_layout::CalendarGeometry::for_layout(node, layout),
                layout.enabled,
            )?;
        } else if node.kind == "Slider" {
            range::slider(
                &mut self.canvas,
                theme,
                node,
                layout,
                input
                    .pressed
                    .as_ref()
                    .is_some_and(|target| target.path == *path)
                    || input.is_scene_control_pressed(path),
                if layout.enabled && !node.boolean("live", true) {
                    input.slider_preview(path)
                } else {
                    None
                },
            );
        } else if node.kind == "Dial" {
            range::dial(
                &mut self.canvas,
                theme,
                node,
                bounds,
                layout.enabled,
                input.is_focused(path) || input.is_pressed(path),
            );
        } else if node.kind == "RangeSlider" {
            let preview = layout
                .enabled
                .then(|| input.interval_preview(node, path))
                .flatten();
            let active = Self::range_thumb_activity(path, input);
            range::interval(&mut self.canvas, theme, node, layout, active, preview);
        } else if node.kind == "PageIndicator" {
            if let Some(geometry) = xrack_layout::PageIndicatorGeometry::with_layout_direction(
                node,
                bounds,
                layout.layout_direction,
            ) {
                controls::page_indicator(
                    &mut self.canvas,
                    theme,
                    node,
                    &geometry,
                    layout.enabled,
                    |index| input.page_marker_hovered(path, index),
                );
            }
        } else if let Some(indicator) = xrack_runtime::indicator_state(node)
            && !node.has_progress_parts()
        {
            let phase =
                indicator_phase.filter(|_| layout.enabled && node.boolean("animated", true));
            self.needs_redraw |=
                range::indicator(&mut self.canvas, theme, node, layout, indicator, phase);
        } else if node.kind == "StandardListItem" {
            controls::row_label(
                &mut self.canvas,
                &self.text,
                node,
                Rect::new(
                    bounds.x + 12.0,
                    bounds.y,
                    (bounds.width - 24.0).max(0.0),
                    bounds.height,
                ),
                node.text("text").unwrap_or("").lines().next().unwrap_or(""),
                if layout.enabled {
                    theme.text
                } else {
                    theme.disabled_text
                },
                if self.text.text_scale() > 1.0 {
                    layout.layout_direction
                } else {
                    xrack_runtime::LayoutDirection::LeftToRight
                },
            )?;
        } else if node.kind == "RadioButton" || is_toggle(node.kind) {
            entry::toggle(&mut self.canvas, &self.text, theme, node, bounds, layout)?;
        } else if node.kind == "ComboBox" && !editable_combo {
            controls::combo(
                &mut self.canvas,
                &self.text,
                theme,
                node,
                layout,
                input.choice_is_open_for(path),
            )?;
        } else if is_input || is_cell || is_spin_box(node.kind) {
            let (area, value) = if editable_combo {
                let geometry = controls::combo(
                    &mut self.canvas,
                    &self.text,
                    theme,
                    node,
                    layout,
                    input.choice_is_open_for(path),
                )?;
                (
                    geometry.entry,
                    std::borrow::Cow::Borrowed(text_input_value(node)),
                )
            } else if node.kind == "SearchField" {
                let geometry = xrack_layout::SearchGeometry::for_layout(layout);
                controls::search_icons(&mut self.canvas, theme, node, &geometry, layout.enabled);
                (
                    geometry.entry,
                    std::borrow::Cow::Borrowed(text_input_value(node)),
                )
            } else if is_spin_box(node.kind) {
                let geometry = SpinGeometry::for_layout(layout, node);
                controls::spin_buttons(
                    &mut self.canvas,
                    &geometry,
                    node,
                    theme,
                    layout.enabled && !node.boolean("read-only", false),
                );
                let value = if input.is_focused(path) {
                    input.editor_value(node)
                } else {
                    std::borrow::Cow::Owned(spin_text(node))
                };
                (geometry.text, value)
            } else if is_cell && is_input {
                (bounds, input.editor_value(node))
            } else {
                (
                    bounds,
                    std::borrow::Cow::Borrowed(node.text("text").unwrap_or("")),
                )
            };
            let editor = if is_input || spin_editable(node) {
                input.displayed_editor(node, path)
            } else {
                None
            };
            let completion = if editable_combo && layout.enabled {
                input.combo_completion(node, path)
            } else {
                None
            };
            let entry = entry::EntryText {
                value: &value,
                editor,
                active: input.editor_active(path),
                scroll: input.text_scroll(path),
                direction: layout.layout_direction_override(),
                completion: completion
                    .as_ref()
                    .map(|candidate| (candidate.suffix(), layout.clip)),
            };
            // Text editing clips to the entry, which must not clip sibling control parts.
            self.canvas.save();
            let drawn = if is_multiline_text_input(node.kind) {
                entry::draw_area(
                    &mut self.canvas,
                    &self.text,
                    theme,
                    node,
                    area,
                    entry,
                    layout.enabled,
                )
                .map(|()| false)
            } else {
                entry::draw(
                    &mut self.canvas,
                    &self.text,
                    theme,
                    node,
                    area,
                    entry,
                    layout.enabled,
                )
            };
            self.canvas.restore();
            if drawn? {
                self.painted_combo_completion = completion;
            }
        } else if node.kind == "StyledText" {
            let focused = input
                .focused
                .as_ref()
                .filter(|target| target.path == *path)
                .and_then(xrack_layout::Target::text_link_index);
            let selection = input.displayed_editor(node, path).map_or(0..0, |editor| {
                editor.selection(node.text("text").unwrap_or_default())
            });
            styled_text::draw(
                &mut self.canvas,
                &self.text,
                node,
                layout,
                frame,
                focused,
                selection,
            )?;
        } else if xrack_layout::uses_label_layout(node) {
            self.paint_label(
                node,
                layout,
                display_list::label_tint(node, layout, theme),
                scale,
            )?;
        } else if node.kind == "Text" || is_button && node.control_part("ButtonContent").is_none() {
            let button = is_button.then(|| ButtonContent::new(node));
            let text = button
                .as_ref()
                .map_or_else(|| button_text(node), |button| button.label);
            let text_color = if !layout.enabled {
                theme.disabled_text
            } else {
                node.color("color").unwrap_or(button_text_color)
            };
            let mut area = button
                .as_ref()
                .map_or(bounds, |button| bounds.inset(button.padding));
            let mut label_font_size = self.text.font_size(node);
            let scaled_button = button.as_ref().filter(|_| self.text.text_scale() > 1.0);
            if let Some(button) = scaled_button {
                (area, label_font_size) = button.scaled_label_metrics(bounds, label_font_size);
            }
            let centered = is_button && !item_delegate;
            let right_aligned = is_button
                && !centered
                && layout.layout_direction == xrack_runtime::LayoutDirection::RightToLeft;
            let mut paint =
                self.text
                    .paint_node_at_size(node, label_font_size, color(text_color), centered);
            if right_aligned {
                paint.set_text_align(femtovg::Align::Right);
            }
            let line_height = label_font_size * 1.4;
            let label_width = button
                .as_ref()
                .map_or(area.width, |button| button.label_width(area, line_height));
            let lines = self.text.lines(text, &paint, label_width);
            let visible_lines = scaled_button.map_or(lines.len(), |button| {
                let height = button.label_height(area, line_height);
                (((height + 0.01) / line_height) as usize).min(lines.len())
            });
            let last = (visible_lines > 0 && visible_lines < lines.len()).then(|| {
                let last = text[lines[visible_lines - 1].clone()].trim_end();
                let preview = format!("{last}…");
                xrack_layout::elide_to_width(&preview, label_width, |line| {
                    self.text.advance_painted(line, &paint)
                })
                .into_owned()
            });
            let shown_line = |index: usize| {
                if index + 1 == visible_lines
                    && let Some(last) = last.as_deref()
                {
                    last
                } else {
                    &text[lines[index].clone()]
                }
            };
            let (label, clip) = if let Some(button) = button.as_ref() {
                let width = if button.icon.is_some() {
                    (0..visible_lines)
                        .map(|index| self.text.advance_painted(shown_line(index), &paint))
                        .fold(0.0_f32, f32::max)
                } else {
                    label_width
                };
                let size = Size::new(width, line_height * visible_lines as f32);
                let clip = scaled_button.is_some() || size.height > area.height;
                let placed =
                    button.place(area, size, line_height, centered, layout.layout_direction);
                if let (Some((image, _)), Some(bounds)) = (button.icon.as_ref(), placed.icon) {
                    let mut display = DisplayList::new(self, layout.clip.intersect(bounds), scale);
                    display.push(Primitive::Icon {
                        bounds,
                        asset: image,
                        enabled: layout.enabled,
                        tint: node
                            .boolean("colorize-icon", false)
                            .then(|| color(text_color)),
                        scale,
                    });
                    display.submit(self)?;
                }
                (placed.label, placed.icon.is_some() || clip)
            } else {
                (bounds, false)
            };
            if clip {
                self.canvas.save();
                self.canvas
                    .intersect_scissor(label.x, label.y, label.width, label.height);
            }
            let x = if centered {
                label.x + label.width * 0.5
            } else if right_aligned {
                label.x + label.width
            } else {
                label.x
            };
            let result = {
                let mut display = DisplayList::new(self, layout.clip.intersect(label), scale);
                for i in 0..visible_lines {
                    let y = label.y + i as f32 * line_height;
                    if y >= label.y + label.height {
                        break;
                    }
                    display.push(Primitive::Text {
                        origin: Point { x, y },
                        text: shown_line(i),
                        paint: &paint,
                        cell_clip: None,
                        cell_fonts: None,
                    });
                }
                display.submit(self)
            };
            if clip {
                self.canvas.restore();
            }
            result?;
        }
        let table = layout.table.as_deref();
        if let (Some(header), Some(table)) = (&node.table_header, table) {
            let grip = input.table_resize_column(path);
            self.draw_table_header(frame, header, table, layout, |column| {
                (
                    input.table_column_hovered(path, column),
                    grip == Some(column),
                    node.table_selection
                        .as_ref()
                        .is_some_and(|view| view.columns().contains(&column)),
                )
            })?;
            if let Some(x) = input.table_column_move_marker(path, node, layout) {
                let marker = Rect::new(x - 1.0, table.header.y, 2.0, table.header.height)
                    .intersect(table.header)
                    .intersect(layout.clip);
                if marker.width > 0.0 && marker.height > 0.0 {
                    let mut path = Path::new();
                    path.rect(marker.x, marker.y, marker.width, marker.height);
                    self.canvas
                        .fill_path(&path, &Paint::color(color(theme.focus)));
                }
            }
        }
        Ok(())
    }

    fn range_thumb_activity(path: &mut Vec<usize>, input: &Interaction) -> [bool; 2] {
        [0, 1].map(|index| {
            path.push(index);
            let active = input.is_focused(path) || input.is_pressed(path);
            path.pop();
            active
        })
    }

    fn paint_custom_range_thumbs(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let active = Self::range_thumb_activity(path, frame.input);
        let order = if active[0] { [1, 0] } else { [0, 1] };
        for index in order {
            let kind = Node::RANGE_HANDLE_PARTS[index];
            if let Some((slot, part, item)) = layout
                .children_in_paint_order(node)
                .find(|(_, child, _)| child.kind == kind)
            {
                path.push(slot);
                let result = self.paint(part, item, path, frame, false);
                path.pop();
                result?;
            } else if let Some(thumb) = layout.child_at(index) {
                let bounds = thumb.bounds;
                range::knob(
                    &mut self.canvas,
                    frame.theme,
                    Point {
                        x: bounds.x + bounds.width * 0.5,
                        y: bounds.y + bounds.height * 0.5,
                    },
                    bounds.width.min(bounds.height) * 0.5,
                    layout.enabled,
                    active[index],
                );
            }
        }
        Ok(())
    }

    fn paint_children(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        if node.kind == "RangeSlider"
            && Node::RANGE_HANDLE_PARTS
                .iter()
                .any(|kind| node.control_part(kind).is_some())
        {
            return self.paint_custom_range_thumbs(node, layout, path, frame);
        }
        if node.has_progress_parts() {
            self.paint_progress_parts(node, layout, path, frame)?;
        }
        let PaintFrame {
            transitions,
            scale,
            theme,
            ..
        } = frame;
        let bounds = layout.bounds;
        let is_layout = xrack_layout::is_layout(node.kind);
        let slide = transitions.and_then(|transitions| transitions.at(path));
        let slide_viewport = bounds.intersect(layout.clip);
        if let Some(slide) = slide {
            let frozen_input = Interaction::default();
            let frozen = PaintFrame {
                input: &frozen_input,
                scale,
                theme,
                transitions: None,
                indicator_phase: None,
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            };
            for (page, offset, visible, cacheable) in slide.outgoing_layers() {
                let clip = slide.slice(bounds, visible).intersect(slide_viewport);
                if clip.width <= 0.0 || clip.height <= 0.0 {
                    continue;
                }
                self.canvas.save();
                self.canvas
                    .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
                let result = self.paint_stack_layer(
                    page,
                    bounds,
                    slide.displacement(bounds, offset),
                    frozen,
                    cacheable,
                );
                self.canvas.restore();
                result?;
            }
        }
        let swipe_offset = frame
            .input
            .swipe_page_motion(path)
            .zip(
                node.swipe_preview
                    .as_ref()
                    .and_then(|preview| preview.page.as_ref()),
            )
            .zip(layout.swipe_preview.as_deref())
            .filter(|(((slot, _, _), (preview_slot, _)), item)| {
                slot == preview_slot && item.visible
            });
        let swipe_offset = if let Some((((_, fraction, orientation), (_, page)), item)) =
            swipe_offset
            && slide.is_none()
        {
            let (x, y) = match orientation {
                xrack_runtime::Orientation::Horizontal => (bounds.width, 0.0),
                xrack_runtime::Orientation::Vertical => (0.0, bounds.height),
            };
            let preview_input = Interaction::default();
            let frozen = PaintFrame {
                input: &preview_input,
                transitions: None,
                indicator_phase: None,
                ..frame
            };
            let fraction_to_page = fraction.signum() - fraction;
            self.canvas.save();
            self.canvas.intersect_scissor(
                slide_viewport.x,
                slide_viewport.y,
                slide_viewport.width,
                slide_viewport.height,
            );
            self.canvas
                .translate(x * fraction_to_page, y * fraction_to_page);
            let result = self.paint(page, item, &mut Vec::new(), frozen, is_layout);
            self.canvas.restore();
            result?;
            Some(Point {
                x: -x * fraction,
                y: -y * fraction,
            })
        } else {
            None
        };
        let thumb = if node.kind == "Switch" {
            layout
                .children_in_paint_order(node)
                .find(|(_, child, _)| child.kind == "SwitchThumb")
        } else {
            None
        };
        let thumb_index = thumb.map(|(index, _, _)| index);
        let offset = slide
            .map(|slide| slide.displacement(bounds, slide.incoming_offset()))
            .or(swipe_offset);
        let cull_clipped_children =
            matches!(node.kind, "VerticalLayout" | "ScrollView" | "Flickable")
                && layout.children.len() >= 32
                && offset.is_none();
        let clipped = |child: &Node, item: &LayoutNode| {
            !item.allows_overflow()
                && is_clipped_out(item.bounds, item.clip)
                && (child.can_skip_clipped_layout_group()
                    // Styled text has no outer shadow. Keep retained layers
                    // on the ordinary painter so they remain tracked.
                    || child.kind == "StyledText"
                        && child.opacity() == 1.0
                        && !child.boolean("cache-rendering-hint", false))
        };
        let children = layout
            .children_in_paint_order_trimming_edges(node, cull_clipped_children, |child, item| {
                !item.visible || clipped(child, item)
            })
            .filter(|(index, _, _)| Some(*index) != thumb_index)
            .chain(thumb);
        // The color-atlas shader does not inherit the canvas's global alpha.
        let transform = canvas_transform(&self.canvas);
        let axis_aligned = offset.is_none()
            && (self.can_cache_on_main_canvas() || self.using_capture_canvas())
            && !self.retained_layers.vectors.opacity_passthrough
            && has_pixel_aligned_scale(transform, scale);
        let plain_canvas =
            axis_aligned && transform.translation() == [0.0, 0.0] && !self.using_capture_canvas();
        let rounded_batch = axis_aligned && !self.rectangle_batches.disabled;
        // Layout keeps the outer clip chain for hit testing. During a
        // rectangular capture it is applied later, on the destination.
        let shaped_clip_on_destination = {
            #[cfg(xrack_femtovg_glyph_samples)]
            {
                rounded_batch
                    && self.using_capture_canvas()
                    && self.direct_rounded_fills_enabled()
                    && !self.canvas.has_shaped_clip()
            }
            #[cfg(not(xrack_femtovg_glyph_samples))]
            {
                false
            }
        };
        let square_batch = plain_canvas && rounded_batch;
        let mut prepare_opacity_for = plain_canvas.then_some((node, layout));
        let batch_images = self.display_batching
            && offset.is_none()
            && self.can_cache_on_main_canvas()
            && !self.retained_layers.vectors.opacity_passthrough
            && image::supports_quad_transform(transform);
        let batch_groups = batch_images && !transitions.is_some_and(StackTransitions::is_active);
        let plain_table_row = self.display_batching
            && plain_canvas
            && node.kind == "TableRow"
            && self.text.text_scale() <= 1.0;
        let batch_table_rows = batch_groups
            && plain_canvas
            && !cfg!(any(target_os = "android", target_os = "ios"))
            && node.kind == "TableView"
            && layout.table.is_some()
            && layout.child_clip_shape().is_none()
            && self.text.text_scale() <= 1.0
            && frame.damage.is_none();
        let resizing_row = batch_table_rows
            .then(|| frame.input.table_resize_row(path))
            .flatten();
        let row_enabled = layout.enabled;
        let cell_paint = std::cell::OnceCell::new();
        let batch_clip = if layout.allows_overflow() {
            layout.clip
        } else {
            layout.clip.intersect(bounds)
        };
        // Keep offscreen cache captures complete during a window repaint.
        let damage = frame.damage.filter(|_| !self.using_capture_canvas());
        let mut display = DisplayList::new(self, batch_clip, scale);
        // Cropped glyph quads can replace a band scissor when its only
        // enclosing scissor is the full device-pixel viewport.
        display.viewport_label_masks = batch_images
            && path.is_empty()
            && node.kind == "Window"
            && DisplayList::plain_window(node)
            && damage.is_none()
            && layout.child_clip_shape().is_none()
            && batch_clip.scaled(scale)
                == Rect::new(
                    0.0,
                    0.0,
                    self.canvas.width() as f32,
                    self.canvas.height() as f32,
                );
        for (i, node, layout) in children {
            // Hidden branches emit no commands and cannot break a batch.
            if !layout.visible
                || node.scene_texture_only()
                || is_window_overlay(node.kind)
                || Node::PROGRESS_PARTS.contains(&node.kind)
                || Node::SCROLL_PARTS.contains(&node.kind)
            {
                continue;
            }
            // An offscreen container between retained items can also be
            // skipped when it cannot change an existing batch boundary.
            if cull_clipped_children && display.is_empty() && clipped(node, layout) {
                continue;
            }
            if node.opacity() != 0.0
                && damage.is_some_and(|rect| {
                    !window_damage::intersects_damage(node, layout, frame, rect)
                })
            {
                // Keep disjoint runs separate when the skipped node was a
                // paint barrier in the full frame.
                display.submit(self)?;
                continue;
            }
            if batch_table_rows
                && layout.enabled == row_enabled
                && (resizing_row.is_none() || resizing_row != node.row_key)
            {
                path.push(i);
                let collected = display.try_append(|display| {
                    self.collect_passive_table_row(node, layout, path, frame, &cell_paint, display)
                });
                path.pop();
                if collected {
                    continue;
                }
            }
            if rounded_batch
                && let Some(rectangle) = rectangle::Solid::from_node_with_capture_clip(
                    node,
                    layout,
                    scale,
                    shaped_clip_on_destination,
                )
                && (rectangle.rounded()
                    || square_batch && rectangle.collect_square(batch_clip, scale))
            {
                if rectangle.visible() {
                    display.push(Primitive::Rectangle(rectangle));
                }
                continue;
            }
            if batch_images
                && let Some(image) = self.display_image(node, layout, is_layout, scale, transform)
            {
                display.push(image);
                continue;
            }
            if batch_images && let Some(label) = self.display_label(node, layout, theme, scale) {
                display.push(label);
                continue;
            }
            if batch_groups
                && display.try_append(|display| {
                    // Bound the repeated work when a deep group eventually
                    // contains an element that needs the general painter.
                    self.collect_group_primitives(node, layout, display, square_batch, theme, 6)
                })
            {
                continue;
            }
            if plain_table_row && layout.enabled == row_enabled {
                path.push(i);
                let collected =
                    self.append_table_cell(node, layout, path, frame, &cell_paint, &mut display);
                path.pop();
                if collected {
                    continue;
                }
            }
            if let Some((parent, parent_layout)) = prepare_opacity_for {
                let opacity = node.opacity();
                if 0.0 < opacity && opacity < 1.0 {
                    self.prepare_opacity_atlas_for_children(parent, parent_layout, frame)?;
                    prepare_opacity_for = None;
                }
            }
            display.submit(self)?;
            path.push(i);
            let result = if let Some(offset) = offset {
                self.canvas.save();
                self.canvas.intersect_scissor(
                    slide_viewport.x,
                    slide_viewport.y,
                    slide_viewport.width,
                    slide_viewport.height,
                );
                self.canvas.translate(offset.x, offset.y);
                let result = self.paint(node, layout, path, frame, is_layout);
                self.canvas.restore();
                result
            } else {
                self.paint(node, layout, path, frame, is_layout)
            };
            path.pop();
            result?;
        }
        display.submit(self)
    }

    fn collect_passive_table_row<'a>(
        &self,
        item: &'a Node,
        layout: &'a LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        cell_paint: &'a std::cell::OnceCell<text::CellPaint>,
        display: &mut DisplayList<'a>,
    ) -> bool {
        // Leave row chrome and custom delegates on the ordinary painter.
        let [row] = item.children.as_slice() else {
            return false;
        };
        let [row_layout] = layout.children.as_slice() else {
            return false;
        };
        if item.kind != "ListRow"
            || row.kind != "TableRow"
            || item.boolean("selected", false)
            || item.boolean("current", false)
            || frame.input.is_hovered(path)
            || !item
                .properties
                .keys()
                .all(|key| matches!(*key, "selected" | "current" | "row-header-text"))
            || !row.properties.keys().all(|key| *key == "row-header-text")
            || !row_layout.visible
            || row_layout.enabled != layout.enabled
            || !layout.local_transform().is_identity()
            || !row_layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || row_layout.table_pane_clip().is_some()
            || layout.has_ancestor_clip_chain()
            || row_layout.has_ancestor_clip_chain()
        {
            return false;
        }
        path.push(0);
        let collected = row_layout
            .children_in_paint_order(row)
            .all(|(index, cell, item)| {
                if !item.visible || item.clip.width <= 0.0 || item.clip.height <= 0.0 {
                    return true;
                }
                if item.enabled != layout.enabled {
                    return false;
                }
                path.push(index);
                let collected =
                    self.append_table_cell(cell, item, path, frame, cell_paint, display);
                path.pop();
                collected
            });
        path.pop();
        collected
    }

    fn append_table_cell<'a>(
        &self,
        node: &'a Node,
        layout: &LayoutNode,
        path: &[usize],
        frame: PaintFrame<'_>,
        row_paint: &'a std::cell::OnceCell<text::CellPaint>,
        display: &mut DisplayList<'a>,
    ) -> bool {
        if node.kind != "TableCell"
            || !node.children.is_empty()
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || layout.has_ancestor_clip_chain()
            || layout.allows_overflow()
            || !node.properties.keys().all(|name| {
                matches!(
                    *name,
                    "text" | "horizontal-alignment" | "enabled" | "editable"
                )
            })
            || frame.input.is_focused(path)
            || frame.input.editor_active(path)
            || frame.input.table_cell_editing(path)
        {
            return false;
        }
        let Some(text) = node.text("text").filter(|text| !text.is_empty()) else {
            return false;
        };
        let area = xrack_layout::text_editor_viewport(node, layout.bounds);
        let clip = area.intersect(layout.clip);
        if !pixel_aligned(area, frame.scale) {
            return false;
        }
        if clip.width <= 0.0 || clip.height <= 0.0 {
            // A viewport can expose cell padding without exposing any text.
            // An empty pixel-aligned scissor emits nothing and leaves the
            // neighboring rows in the same glyph batch.
            return pixel_aligned(layout.clip, frame.scale);
        }
        if !pixel_aligned(clip, frame.scale) {
            return false;
        }
        let text::CellPaint { paint, fonts } = row_paint.get_or_init(|| {
            let size = self.text.entry_font_size(node, layout.bounds);
            let tint = color(frame.theme.editor_foreground(node, false, layout.enabled));
            self.text.cell_paint(node, size, tint)
        });
        let measurement_scale = || {
            // Power-of-two scaling preserves the advances and their sum.
            // Match the glyph batch's size so measurement and drawing can
            // share the shaped run in either FemtoVG build.
            if !cfg!(any(target_os = "android", target_os = "ios"))
                && text.len() <= 128
                && matches!(frame.scale, 2.0 | 4.0)
                && (1.0..=90.0).contains(&(paint.font_size() * frame.scale))
                && paint.letter_spacing() == 0.0
                && crate::has_device_scale(canvas_transform(&self.canvas), frame.scale)
            {
                cell_glyphs::measurement_scale(frame.scale)
            } else {
                1.0
            }
        };
        display.push(Primitive::Text {
            origin: entry::passive_origin(
                &self.text,
                node,
                area,
                text,
                paint,
                layout.layout_direction_override(),
                measurement_scale,
            ),
            text,
            paint,
            cell_clip: Some(clip),
            cell_fonts: Some(fonts),
        });
        true
    }

    fn display_image<'a>(
        &self,
        node: &'a Node,
        layout: &LayoutNode,
        in_layout: bool,
        scale: f32,
        transform: CanvasTransform,
    ) -> Option<Primitive<'a>> {
        let image = image::PlainImage::from_node(node, layout)?;
        let bounds = layout.bounds;
        let visible = bounds.intersect(layout.clip);
        if visible.width <= 0.0 || visible.height <= 0.0 {
            return None;
        }
        let asset = image.asset()?;
        let aligned = image::quad_edges_are_pixel_aligned(bounds, transform);
        let presentation = image.presentation(asset.size(), in_layout);
        let clip = (!aligned || visible != bounds).then_some(visible);
        let quad = if aligned
            && (visible == bounds || image::quad_edges_are_pixel_aligned(visible, transform))
        {
            Some(image::QuadBounds::Aligned(clip))
        } else {
            let visible = presentation.placement(bounds).visible.intersect(visible);
            image::quad_pixel_coverage(visible, transform, scale).map(image::QuadBounds::Snapped)
        };
        let quad = quad.or_else(|| {
            (scale >= 1.0 && !layout.has_ancestor_clip_chain())
                .then_some(image::QuadBounds::Antialiased(visible, scale))
        });
        if quad.is_none() && scale < 1.0 {
            return None;
        }
        Some(Primitive::Image {
            presentation,
            bounds,
            // An unaligned image stays on the original path painter with
            // its element scissor, even when collected alongside other draws.
            clip,
            quad,
            asset,
            colorize: image::Colorize::Original,
            scale: scale * layout.world_transform().max_scale(),
        })
    }

    fn is_plain_image(node: &Node, layout: &LayoutNode) -> bool {
        image::PlainImage::from_node(node, layout).is_some()
    }

    fn draw_rectangle_batch(
        &mut self,
        rectangles: &mut Vec<rectangle::Solid>,
        scale: f32,
        clip: Rect,
    ) -> Result<(), String> {
        if self.draw_retained_geometry(rectangles, scale) {
            rectangles.clear();
            return Ok(());
        }
        if rectangles.first().is_some_and(rectangle::Solid::rounded) {
            self.draw_rounded_batch(rectangles, scale)?;
        } else {
            self.rectangle_batches
                .draw_squares(&mut self.canvas, scale, clip, rectangles);
        }
        Ok(())
    }

    fn paint_progress_parts(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let state =
            xrack_runtime::indicator_state(node).unwrap_or(xrack_runtime::IndicatorState::Hidden);
        let phase = frame
            .indicator_phase
            .filter(|_| layout.enabled && node.boolean("animated", true));
        let fill = xrack_layout::progress_fill_bounds(
            layout.bounds,
            layout.layout_direction,
            state,
            phase,
        );
        for kind in Node::PROGRESS_PARTS {
            let moving = kind == "ProgressFill";
            let bounds = if moving { fill } else { layout.bounds };
            let drawn = if let Some((slot, part, item)) = layout
                .children_in_paint_order(node)
                .find(|(_, part, _)| part.kind == kind)
            {
                let clip = bounds.intersect(layout.clip);
                path.push(slot);
                let result = self.paint_group(clip, 1.0, |renderer| {
                    renderer
                        .canvas
                        .translate(bounds.x - item.bounds.x, bounds.y - item.bounds.y);
                    renderer.paint(part, item, path, frame, false)
                });
                path.pop();
                result?;
                part.is_visible()
                    && part.opacity() > 0.0
                    && bounds.width > 0.0
                    && bounds.height > 0.0
            } else if moving {
                range::progress_fill(&mut self.canvas, frame.theme, node, bounds, layout.enabled)
            } else {
                range::progress_track(&mut self.canvas, frame.theme, node, bounds);
                false
            };
            self.needs_redraw |= moving
                && drawn
                && phase.is_some()
                && state == xrack_runtime::IndicatorState::Indeterminate;
        }
        Ok(())
    }

    fn paint_chrome(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let PaintFrame { input, theme, .. } = frame;
        let bounds = layout.bounds;
        if layout.enabled && node.boolean("pull-to-refresh", false) {
            let refreshing = node.boolean("refreshing", false);
            let progress = if refreshing {
                Some(0.27)
            } else {
                input.pull_to_refresh_progress(node)
            };
            if let Some(progress) = progress.filter(|progress| *progress > 0.0) {
                let viewport = layout.scroll_viewport(Orientation::Vertical);
                let phase = frame
                    .indicator_phase
                    .filter(|_| refreshing && !input.reduce_motion());
                self.paint_group(viewport.intersect(layout.clip), 1.0, |renderer| {
                    range::refresh_indicator(
                        &mut renderer.canvas,
                        theme,
                        viewport,
                        progress,
                        phase,
                    );
                    Ok(())
                })?;
                self.needs_redraw |= phase.is_some();
            }
        }
        if let Some(marker) = input.tab_move_marker(node, layout, path) {
            let mut line = Path::new();
            line.rect(marker.x, marker.y, marker.width, marker.height);
            self.canvas
                .fill_path(&line, &Paint::color(color(theme.focus)));
        }
        if let Some((rect, _)) = layout
            .swipe_delegate
            .as_deref()
            .and_then(xrack_layout::SwipeDelegateGeometry::toggle)
        {
            let mut dots = Path::new();
            let center = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
            for x in [-5.0, 0.0, 5.0] {
                dots.circle(center.0 + x, center.1, 1.3);
            }
            self.canvas.fill_path(
                &dots,
                &Paint::color(color(if layout.enabled {
                    theme.text
                } else {
                    theme.disabled_text
                })),
            );
        }
        let drag_target = node.kind == "DropArea" && input.is_drop_target(node, path);
        let focus_outline = (node.kind == "SwipeDelegate"
            || node.kind == "SplitHandle" && !node.children.is_empty())
            && input.is_focused(path);
        if (focus_outline || drag_target) && layout.enabled {
            let mut outline = Path::new();
            append_rect(&mut outline, bounds.inset(2.0), [0.0; 4]);
            let mut paint = Paint::color(color(theme.focus));
            paint.set_line_width(2.0);
            self.canvas.stroke_path(&outline, &paint);
        }
        if let Some(grip) = xrack_layout::drawer_grip(node, bounds) {
            let mut path = Path::new();
            path.rounded_rect(grip.x, grip.y, grip.width, grip.height, 1.5);
            self.canvas
                .fill_path(&path, &Paint::color(color(theme.role(Role::Grip))));
        }
        if node.kind == "ListRow" {
            let item_delegate = node.children.first().is_some_and(Node::is_item_delegate);
            controls::list_row(
                &mut self.canvas,
                theme,
                bounds,
                layout.layout_direction,
                !item_delegate && node.boolean("selected", false),
                !item_delegate && input.is_hovered(path) && layout.enabled,
                node.boolean("current", false)
                    && path
                        .split_last()
                        .is_some_and(|(_, table)| input.is_focused(table)),
            );
            if let Some((_, table)) = path.split_last()
                && node
                    .row_key
                    .is_some_and(|key| input.table_resize_row(table) == Some(key))
            {
                let grip = Rect::new(bounds.x, bounds.y + bounds.height - 2.0, bounds.width, 2.0)
                    .intersect(layout.clip);
                if grip.width > 0.0 && grip.height > 0.0 {
                    let mut path = Path::new();
                    path.rect(grip.x, grip.y, grip.width, grip.height);
                    self.canvas
                        .fill_path(&path, &Paint::color(color(theme.focus)));
                }
            }
            if node.boolean("tree-has-children", false)
                && node
                    .children
                    .first()
                    .is_none_or(|row| row.control_part("TreeDisclosure").is_none())
                && let Some(tree) = TreeRowGeometry::for_layout(node, layout)
            {
                controls::tree_indicator(
                    &mut self.canvas,
                    theme,
                    tree.indicator,
                    layout.layout_direction,
                    node.boolean("tree-expanded", false),
                    layout.enabled,
                    self.text.text_scale(),
                );
            }
        }
        let cell_selected = node.boolean("table-selected", false);
        let cell_current = node.boolean("table-current", false)
            && path.len() >= 3
            && input.is_focused(&path[..path.len() - 3]);
        if cell_selected || cell_current {
            controls::list_row(
                &mut self.canvas,
                theme,
                bounds,
                layout.layout_direction,
                cell_selected,
                false,
                cell_current,
            );
        }
        if let Some(table) = &layout.table
            && let Some(boundary) = table.frozen_boundary()
        {
            let seam = Rect::new(
                boundary - 1.0,
                table.header.y,
                1.0,
                table.header.height + table.body.height,
            )
            .intersect(layout.clip);
            if seam.width > 0.0 && seam.height > 0.0 {
                rectangle::fill_surface(
                    &mut self.canvas,
                    seam,
                    [0.0; 4],
                    Paint::color(color(theme.role(Role::Divider))),
                );
            }
        }
        if let Some(rect) = input.table_row_move_marker(path, node, layout) {
            let mut marker = Path::new();
            marker.rect(rect.x, rect.y, rect.width, rect.height);
            self.canvas
                .fill_path(&marker, &Paint::color(color(theme.focus)));
        }
        for rect in input.table_drag_preview(path, node, layout) {
            let mut region = Path::new();
            append_rect(&mut region, rect, [0.0; 4]);
            let translucent = UiColor((theme.focus.0 & 0xffff_ff00) | 42);
            self.canvas
                .fill_path(&region, &Paint::color(color(translucent)));
            let mut outline = Path::new();
            append_rect(&mut outline, rect.inset(0.5), [0.0; 4]);
            self.canvas
                .stroke_path(&outline, &Paint::color(color(theme.focus)));
        }
        for (index, rect) in input.table_selection_handle_parts(path, node, layout) {
            if let Some(part) = node.table_selection_part(index) {
                let item = xrack_layout::layout_choice_option(
                    part,
                    rect,
                    layout.layout_direction,
                    layout.enabled,
                    &mut self.text,
                );
                self.paint_group(rect, 1.0, |renderer| {
                    renderer.paint(
                        part,
                        &item,
                        &mut Vec::new(),
                        PaintFrame {
                            input: &Interaction::default(),
                            transitions: None,
                            ..frame
                        },
                        false,
                    )
                })?;
                continue;
            }
            let x = rect.x + rect.width * 0.5;
            let y = rect.y + rect.height * 0.5;
            let radius = 6.0_f32.min(rect.width * 0.5);
            let mut border = Path::new();
            border.circle(x, y, radius);
            self.canvas
                .fill_path(&border, &Paint::color(color(theme.role(Role::Surface))));
            let mut center = Path::new();
            center.circle(x, y, radius * 0.75);
            self.canvas
                .fill_path(&center, &Paint::color(color(theme.focus)));
        }
        let custom_parts = node.has_scroll_parts();
        for bar in layout.scroll_bars(node) {
            let focused = node.kind == "ScrollBar" && input.is_focused(path);
            let indicator = node.kind == "ScrollIndicator"
                || node.kind == "ScrollBar" && !node.boolean("interactive", true);
            for kind in Node::SCROLL_PARTS {
                let replacement = custom_parts
                    .then(|| {
                        layout
                            .children_in_paint_order(node)
                            .find(|(_, child, _)| child.kind == kind)
                    })
                    .flatten();
                if let Some((slot, part, item)) = replacement {
                    path.push(slot);
                    let slot_bounds = if kind == "ScrollThumb" {
                        bar.thumb
                    } else {
                        bar.track
                    };
                    let painted =
                        self.paint_group(slot_bounds.intersect(layout.clip), 1.0, |renderer| {
                            renderer.paint(part, item, path, frame, false)
                        });
                    path.pop();
                    painted?;
                } else if kind == "ScrollThumb" {
                    range::scroll_thumb(
                        &mut self.canvas,
                        theme,
                        bar,
                        layout.enabled,
                        input.scroll_bar_highlighted(path, bar.orientation) || focused,
                    );
                } else if !indicator {
                    range::scroll_track(&mut self.canvas, theme, bar.track);
                }
            }
            if focused && layout.enabled && custom_parts {
                let bounds = bar.track.inset(1.0);
                let mut outline = Path::new();
                append_rect(
                    &mut outline,
                    bounds,
                    [bounds.width.min(bounds.height) * 0.5; 4],
                );
                let mut paint = Paint::color(color(theme.focus));
                paint.set_line_width(2.0);
                self.canvas.stroke_path(&outline, &paint);
            }
        }
        Ok(())
    }

    /// Actual GPU rendering followed by bounded GPU→CPU readback for PNGs/tests.
    pub fn snapshot(
        &mut self,
        scene: &Node,
        width: u32,
        height: u32,
        scale: f32,
        input: &Interaction,
    ) -> Result<Snapshot, String> {
        self.snapshot_with_transitions(
            scene,
            width,
            height,
            scale,
            input,
            &StackTransitions::default(),
        )
    }

    /// Capture a sampled frame, including frozen pages currently sliding out.
    pub fn snapshot_with_transitions(
        &mut self,
        scene: &Node,
        width: u32,
        height: u32,
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<Snapshot, String> {
        self.snapshot_frame(
            scene,
            width,
            height,
            None,
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions: Some(transitions),
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            },
        )
    }

    /// Sample the indicator animation at a specified elapsed time.
    pub fn snapshot_at(
        &mut self,
        scene: &Node,
        width: u32,
        height: u32,
        scale: f32,
        input: &Interaction,
        elapsed: Duration,
    ) -> Result<Snapshot, String> {
        self.snapshot_frame(
            scene,
            width,
            height,
            None,
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions: None,
                indicator_phase: Some(indicator_phase(elapsed)),
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            },
        )
    }

    /// Capture the same prepared scene and layout used for input hit testing.
    pub fn snapshot_prepared(
        &mut self,
        scene: &Node,
        layout: LayoutNode,
        width: u32,
        height: u32,
        scale: f32,
        input: &Interaction,
    ) -> Result<Snapshot, String> {
        self.snapshot_prepared_frame(scene, layout, [width, height], scale, input, None)
    }

    /// Capture a prepared preview frame with its current stack slides.
    pub fn snapshot_prepared_with_transitions(
        &mut self,
        scene: &Node,
        layout: LayoutNode,
        size: [u32; 2],
        scale: f32,
        input: &Interaction,
        transitions: &StackTransitions,
    ) -> Result<Snapshot, String> {
        self.snapshot_prepared_frame(scene, layout, size, scale, input, Some(transitions))
    }

    fn snapshot_prepared_frame(
        &mut self,
        scene: &Node,
        layout: LayoutNode,
        [width, height]: [u32; 2],
        scale: f32,
        input: &Interaction,
        transitions: Option<&StackTransitions>,
    ) -> Result<Snapshot, String> {
        self.snapshot_frame(
            scene,
            width,
            height,
            Some(layout),
            PaintFrame {
                input,
                scale,
                theme: self.palette(scene),
                transitions,
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
                cleared_background: None,
                damage: None,
                prepared_layout: None,
            },
        )
    }

    fn snapshot_frame(
        &mut self,
        scene: &Node,
        width: u32,
        height: u32,
        layout: Option<LayoutNode>,
        frame: PaintFrame<'_>,
    ) -> Result<Snapshot, String> {
        let scale = frame.scale;
        validate_extent(
            width,
            height,
            scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack snapshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let layout = layout.unwrap_or_else(|| {
            self.layout_with_input(
                scene,
                Size::new(width as f32 / scale, height as f32 / scale),
                frame.input,
            )
        });
        self.paint_frame(scene, &layout, &texture, frame)?;
        let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("XRack readback"),
            size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(15)),
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let view = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| e.to_string())?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in view.chunks_exact(stride as usize) {
            rgba.extend_from_slice(&row[..width as usize * 4]);
        }
        drop(view);
        buffer.unmap();
        self.poll_viewport_gpu_timing();
        Ok(Snapshot {
            width,
            height,
            rgba,
            layout,
        })
    }
}

#[inline]
fn is_clipped_out(bounds: Rect, clip: Rect) -> bool {
    // Long viewports commonly contain a run wholly above or below the clip.
    // A separating finite edge gives a zero-height intersection.
    if clip.y.is_finite() && bounds.y + bounds.height <= clip.y
        || bounds.y.is_finite() && clip.y + clip.height <= bounds.y
    {
        return true;
    }
    let intersection = clip.intersect(bounds);
    intersection.width <= 0.0 || intersection.height <= 0.0
}

#[test]
fn clipped_out_matches_rectangular_intersection() {
    let values = [
        f32::NEG_INFINITY,
        -8.0,
        -0.0,
        0.0,
        8.0,
        f32::INFINITY,
        f32::NAN,
    ];
    let rects: Vec<_> = values
        .into_iter()
        .flat_map(|y| values.into_iter().map(move |h| Rect::new(0.0, y, 12.0, h)))
        .collect();
    for bounds in &rects {
        for clip in &rects {
            let intersection = clip.intersect(*bounds);
            assert_eq!(
                is_clipped_out(*bounds, *clip),
                intersection.width <= 0.0 || intersection.height <= 0.0
            );
        }
    }
}

fn border_path(bounds: Rect, radii: [f32; 4], border: f32) -> Path {
    let mut path = Path::new();
    append_rect(&mut path, bounds, radii);
    let inner = bounds.inset(border);
    if inner.width > 0.0 && inner.height > 0.0 {
        append_rect(
            &mut path,
            inner,
            radii.map(|radius| (radius - border).max(0.0)),
        );
        path.solidity(Solidity::Hole);
    }
    path
}

fn append_rect(path: &mut Path, bounds: Rect, [tl, tr, br, bl]: [f32; 4]) {
    path.rounded_rect_varying(
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        tl,
        tr,
        br,
        bl,
    );
}

const MAX_RENDER_PIXELS: u64 = 64_000_000;

fn validate_extent(width: u32, height: u32, scale: f32, limit: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > limit
        || height > limit
        || u64::from(width) * u64::from(height) > MAX_RENDER_PIXELS
        || !scale.is_finite()
        || scale <= 0.0
    {
        return Err("invalid render extent or scale factor".into());
    }
    Ok(())
}

pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub layout: LayoutNode,
}
impl Snapshot {
    pub fn save_png(&self, path: impl AsRef<FilePath>) -> Result<(), String> {
        self.write_png(BufWriter::new(
            File::create(path).map_err(|e| e.to_string())?,
        ))
    }

    pub fn write_png(&self, writer: impl Write) -> Result<(), String> {
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&self.rgba)
            .map_err(|e| e.to_string())
    }
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.rgba[((y * self.width + x) * 4) as usize..][..4]
            .try_into()
            .ok()
    }
}
