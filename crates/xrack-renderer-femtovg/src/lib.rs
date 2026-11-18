//! wgpu-backed 2D renderer. No windowing dependency and no compiler/MLIR dependency.
mod controls;
mod entry;
mod image;
mod image_colorize;
#[cfg(target_os = "ios")]
mod ios_font;
mod nine_slice;
pub mod preview;
mod preview_pointer;
mod range;
mod retained_layers;
mod shadow;
mod styled_text;
mod text;
mod theme;
mod vector;
#[doc(hidden)]
pub use entry::{PointerCaret, TouchSelectionHandle};
use femtovg::{
    Canvas, Color, FillRule, LayerEffects, Paint, Path, Solidity, Transform2D,
    renderer::WGPURenderer,
};
#[doc(hidden)]
pub use retained_layers::LayerStats;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path as FilePath,
    rc::Rc,
    time::{Duration, Instant},
};
pub use text::TextSystem;
pub use theme::Theme;
use theme::{Palette, PaletteRole as Role};
use xrack_layout::{
    Affine, ButtonContent, ComboCompletion, ImagePresentation, Interaction, LayoutNode, Point,
    Rect, Size, SpinGeometry, StackTransitions, TextMeasurer, TreeRowGeometry, corner_radii,
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
}

fn color(color: UiColor) -> Color {
    Color::rgba(
        (color.0 >> 24) as u8,
        (color.0 >> 16) as u8,
        (color.0 >> 8) as u8,
        color.0 as u8,
    )
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
    retained_layers: retained_layers::RetainedLayers,
    row_layouts: xrack_layout::RowLayoutCache,
    clock: Instant,
    needs_redraw: bool,
    painted_combo_completion: Option<ComboCompletion>,
    inspection_outline: Option<Rect>,
    pub text: TextSystem,
    pub theme: Theme,
    system_color_scheme: ColorScheme,
}

/// GPU resources that can back more than one independent renderer.
#[derive(Clone)]
pub struct RenderDevice {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
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
        let required_limits = wgpu::Limits::default().using_resolution(supported_limits.clone());
        // The iOS Simulator advertises one fewer varyings slot than the WebGPU default.
        #[cfg(target_os = "ios")]
        let required_limits = wgpu::Limits {
            max_inter_stage_shader_variables: required_limits
                .max_inter_stage_shader_variables
                .min(supported_limits.max_inter_stage_shader_variables),
            ..required_limits
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("XRack"),
                required_features: wgpu::Features::empty(),
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

    pub fn with_device(gpu: RenderDevice) -> Result<Self, String> {
        Self::with_device_and_text(gpu, TextSystem::system()?)
    }

    fn with_device_and_text(gpu: RenderDevice, text: TextSystem) -> Result<Self, String> {
        let RenderDevice {
            instance,
            adapter,
            device,
            queue,
        } = gpu;
        let canvas = Canvas::new_with_text_context(
            WGPURenderer::new(device.clone(), queue.clone()),
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
            images: image::Images::default(),
            retained_layers: Default::default(),
            row_layouts: Default::default(),
            clock: Instant::now(),
            needs_redraw: false,
            painted_combo_completion: None,
            inspection_outline: None,
            text,
            theme: Theme::default(),
            system_color_scheme: ColorScheme::System,
        })
    }

    /// Apply the platform's preferred text scale to measurement and drawing.
    /// Returns whether cached layouts need to be prepared again.
    pub fn set_text_scale(&mut self, scale: f32) -> bool {
        if !self.text.set_text_scale(scale) {
            return false;
        }
        self.row_layouts = Default::default();
        self.retained_layers.clear_images(&mut self.canvas);
        true
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

    /// An animated element was visible in the last frame.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn take_painted_combo_completion(&mut self) -> Option<ComboCompletion> {
        self.painted_combo_completion.take()
    }

    /// Set the host color scheme used by windows that request `system`.
    pub fn set_system_color_scheme(&mut self, scheme: ColorScheme) -> bool {
        std::mem::replace(&mut self.system_color_scheme, scheme) != scheme
    }

    fn palette(&self, scene: &Node) -> Palette {
        Palette::resolve(self.theme, scene, self.system_color_scheme)
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
        let inspection_outline = self.inspection_outline.take();
        let PaintFrame {
            input,
            scale,
            theme,
            ..
        } = frame;
        self.text.set_window(scene);
        if !Rc::ptr_eq(&self.canvas_font_context, self.text.context_key()) {
            self.canvas = Canvas::new_with_text_context(
                WGPURenderer::new(self.device.clone(), self.queue.clone()),
                self.text.context().clone(),
            )
            .map_err(|error| error.to_string())?;
            self.canvas_font_context = self.text.context_key().clone();
            self.images = image::Images::default();
            self.retained_layers.reset_canvas();
        }
        self.retained_layers.begin_frame(&mut self.canvas);
        self.retained_layers
            .sync_font_defaults(&mut self.canvas, self.text.default_style());
        self.needs_redraw = false;
        self.painted_combo_completion = None;
        let size = target.size();
        self.canvas.set_size(size.width, size.height, scale);
        self.canvas.reset();
        self.canvas
            .clear_rect(0, 0, size.width, size.height, color(theme.window));
        self.canvas.scale(scale, scale);
        self.images.begin_frame();
        if !is_window_overlay(scene.kind) {
            self.paint(scene, layout, &mut Vec::new(), frame, false)?;
        }
        for path in input.overlay_layers(layout) {
            let (Some(node), Some(item)) = (scene.at_path(path), layout.at_path(path)) else {
                continue;
            };
            if !node.is_modal() && !input.allows_popup(layout, path) {
                continue;
            }
            if node.boolean("dim", node.is_modal()) {
                let mut backdrop = Path::new();
                backdrop.rect(
                    0.0,
                    0.0,
                    size.width as f32 / scale,
                    size.height as f32 / scale,
                );
                self.canvas
                    .fill_path(&backdrop, &Paint::color(color(theme.role(Role::Scrim))));
            }
            self.paint(node, item, &mut path.to_vec(), frame, false)?;
        }
        if let Some(menu) = input.choice_menu(scene, layout) {
            self.paint_choice_menu(&menu, frame)?;
        }
        for panel in input.menu_panels(scene, layout) {
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
            handle.paint(&mut self.canvas, color(theme.focus));
        }
        if let Some(popup) = input.tooltip_popup(scene, layout) {
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
                },
                false,
            )?;
        }
        if let Some(bounds) =
            inspection_outline.filter(|rect| rect.width >= 3.0 && rect.height >= 3.0)
        {
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
        self.queue.submit(self.canvas.flush_to_output(target));
        self.images.end_frame(&mut self.canvas);
        self.retained_layers.finish_frame(&mut self.canvas);
        Ok(())
    }

    fn paint(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<(), String> {
        let local = layout.local_transform();
        let pane = layout.table_pane_clip();
        if local == Affine::IDENTITY && pane.is_none() {
            return self.paint_in_space(node, layout, path, frame, in_layout);
        }
        self.canvas.save();
        if let Some((bounds, transform)) = pane {
            let corners = [
                Point {
                    x: bounds.x,
                    y: bounds.y,
                },
                Point {
                    x: bounds.x + bounds.width,
                    y: bounds.y,
                },
                Point {
                    x: bounds.x + bounds.width,
                    y: bounds.y + bounds.height,
                },
                Point {
                    x: bounds.x,
                    y: bounds.y + bounds.height,
                },
            ]
            .map(|point| transform.map_point(point));
            let mut mask = Path::new();
            mask.move_to(corners[0].x, corners[0].y);
            for point in &corners[1..] {
                mask.line_to(point.x, point.y);
            }
            mask.close();
            self.canvas.clip_path(&mask, FillRule::NonZero);
        }
        if local != Affine::IDENTITY {
            self.canvas.set_transform(&Transform2D(local.0));
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
        let content = layout.clip.intersect(layout.bounds);
        let shadow = shadow::OuterShadow::for_node(
            node,
            layout.bounds,
            frame.scale * layout.world_transform().max_scale(),
            frame.theme.role(Role::Border),
        )
        .and_then(|shadow| {
            let effect = layout.clip.intersect(shadow.paint_bounds());
            shadow::nonempty(effect).then_some((shadow, effect))
        });
        if layout.allows_overflow() {
            // The group must cover descendants outside the box, including their
            // paint-only effects. The enclosing viewport is its hard bound.
            return self.paint_group(layout.clip, opacity, |renderer| {
                if let Some((shadow, effect)) = shadow {
                    shadow.draw(&mut renderer.canvas, effect);
                }
                renderer.paint_element(node, layout, path, frame, in_layout)
            });
        }
        if let Some((shadow, effect)) = shadow {
            return self.paint_group(shadow::union(content, effect), opacity, |renderer| {
                shadow.draw(&mut renderer.canvas, effect);
                renderer.paint_group(content, 1.0, |renderer| {
                    renderer.paint_element(node, layout, path, frame, in_layout)
                })
            });
        }
        if opacity < 1.0 && self.paint_retained_opacity(node, layout, path, frame, in_layout)? {
            return Ok(());
        }
        self.paint_group(content, opacity, |renderer| {
            renderer.paint_element(node, layout, path, frame, in_layout)
        })
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
        let transform = self.canvas.transform();
        let [_, b, c, _, _, _] = transform.0;
        if b == 0.0 && c == 0.0 {
            self.canvas
                .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        } else {
            if opacity < 1.0 {
                // Bound the offscreen allocation in device space while keeping
                // the exact rotated clip for the final composite.
                let capture = Affine(transform.0).map_rect(clip).inset(-1.0);
                self.canvas.reset_transform();
                self.canvas
                    .intersect_scissor(capture.x, capture.y, capture.width, capture.height);
                self.canvas.set_transform(&transform);
            }
            let mut mask = Path::new();
            mask.rect(clip.x, clip.y, clip.width, clip.height);
            self.canvas.clip_path(&mask, FillRule::NonZero);
        }
        let layer = opacity < 1.0;
        if layer {
            // FemtoVG supplies a pass-through fallback when it cannot reserve a layer.
            let _captured = self
                .canvas
                .begin_layer(&LayerEffects::new().with_opacity(opacity));
        }
        let result = draw(self);
        if layer {
            self.canvas.end_layer();
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
                if let Some(node) = artwork.as_mut().and_then(Iterator::next).flatten() {
                    let bounds = menu.row_bounds(offset);
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
                    continue;
                }
                controls::choice_row(
                    &mut renderer.canvas,
                    &renderer.text,
                    frame.theme,
                    menu,
                    offset,
                )?;
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
        let background = if use_row_surface {
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
            let mut path = Path::new();
            append_rect(&mut path, bounds, radii);
            self.canvas.fill_path(&path, &background);
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
        if layout.dialog_title_height(node) > 0.0 {
            let paint = self.text.paint_node(
                node,
                color(node.color("color").unwrap_or(theme.text)),
                false,
            );
            self.text
                .fill_text(
                    &mut self.canvas,
                    bounds.x + 16.0,
                    bounds.y + 12.0,
                    node.text("title").unwrap_or(""),
                    &paint,
                )
                .map_err(|error| format!("dialog title drawing failed: {error}"))?;
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
                let size = self.text.font_size(node);
                let foreground = if layout.enabled {
                    node.color("color").unwrap_or(theme.text)
                } else {
                    theme.disabled_text
                };
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
            };
            self.canvas.restore();
            result?;
        }
        let border = node
            .length("border-width")
            .unwrap_or(
                if node.kind == "ControlFrame"
                    || group_header.is_some()
                    || field && theme.is_light()
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
        } else if matches!(node.kind, "Image" | "AnimatedImage")
            && let Some(image) = node.image_asset("source")
        {
            let image = if node.kind == "AnimatedImage" {
                image.frame(node.int("current-frame").unwrap_or_default().max(0) as usize)
            } else {
                image
            };
            let presentation = ImagePresentation::for_node(node, image.size(), in_layout);
            self.images.draw(
                &mut self.canvas,
                &image,
                bounds,
                image_scale,
                presentation,
                image::Colorize::from_value(node.properties.get("colorize")),
            )?;
        } else if node.kind == "Path" {
            vector::draw_path(&mut self.canvas, node, bounds)?;
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
                    .is_some_and(|target| target.path == *path),
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
            controls::label(
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
                self.text.font_size(node),
                if layout.enabled {
                    theme.text
                } else {
                    theme.disabled_text
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
                theme,
                focused,
                selection,
            )?;
        } else if xrack_layout::uses_label_layout(node) {
            let tint = if layout.enabled {
                node.color("color").unwrap_or(theme.text)
            } else {
                theme.disabled_text
            };
            let paint = self.text.paint_node(node, color(tint), false);
            let label = self.text.label_layout(
                node,
                Size::new(bounds.width, bounds.height),
                layout.layout_direction_override(),
            );
            for line in label.lines {
                if !line.text.is_empty() {
                    self.text
                        .fill_text(
                            &mut self.canvas,
                            bounds.x + line.origin.x,
                            bounds.y + line.origin.y,
                            &*line.text,
                            &paint,
                        )
                        .map_err(|error| format!("text drawing failed: {error}"))?;
                }
            }
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
            let area = button
                .as_ref()
                .map_or(bounds, |button| bounds.inset(button.padding));
            let centered = is_button && !item_delegate;
            let right_aligned = is_button
                && !centered
                && layout.layout_direction == xrack_runtime::LayoutDirection::RightToLeft;
            let mut paint = self.text.paint_node(node, color(text_color), centered);
            if right_aligned {
                paint.set_text_align(femtovg::Align::Right);
            }
            let line_height = self.text.font_size(node) * 1.4;
            let label_width = button
                .as_ref()
                .map_or(area.width, |button| button.label_width(area, line_height));
            let lines = self.text.lines(text, &paint, label_width);
            let (label, clip) = if let Some(button) = button {
                let width = if button.icon.is_some() {
                    lines
                        .iter()
                        .map(|range| self.text.advance_painted(&text[range.clone()], &paint))
                        .fold(0.0_f32, f32::max)
                } else {
                    label_width
                };
                let size = Size::new(width, line_height * lines.len() as f32);
                let placed =
                    button.place(area, size, line_height, centered, layout.layout_direction);
                if let (Some((image, _)), Some(bounds)) = (button.icon.as_ref(), placed.icon) {
                    self.images.draw_icon(
                        &mut self.canvas,
                        image,
                        bounds,
                        scale,
                        layout.enabled,
                        node.boolean("colorize-icon", false)
                            .then(|| color(text_color)),
                    )?;
                }
                (
                    placed.label,
                    placed.icon.is_some() || size.height > area.height,
                )
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
            let result = (|| {
                for (i, range) in lines.iter().enumerate() {
                    let y = label.y + i as f32 * line_height;
                    if y >= label.y + label.height {
                        break;
                    }
                    self.text
                        .fill_text(&mut self.canvas, x, y, &text[range.clone()], &paint)
                        .map_err(|e| format!("text drawing failed: {e}"))?;
                }
                Ok::<(), String>(())
            })();
            if clip {
                self.canvas.restore();
            }
            result?;
        }
        let table = layout.table.as_deref();
        if let (Some(header), Some(table)) = (&node.table_header, table) {
            let grip = input.table_resize_column(path);
            controls::table_header(
                &mut self.canvas,
                &self.text,
                theme,
                header,
                table,
                layout.clip.intersect(bounds),
                |column| {
                    (
                        input.table_column_hovered(path, column),
                        grip == Some(column),
                        node.table_selection
                            .as_ref()
                            .is_some_and(|view| view.columns().contains(&column)),
                    )
                },
            )?;
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
        let children = layout
            .children_in_paint_order(node)
            .filter(|(index, _, _)| Some(*index) != thumb_index)
            .chain(thumb);
        for (i, node, layout) in children {
            if is_window_overlay(node.kind)
                || Node::PROGRESS_PARTS.contains(&node.kind)
                || Node::SCROLL_PARTS.contains(&node.kind)
            {
                continue;
            }
            path.push(i);
            let offset = slide
                .map(|slide| slide.displacement(bounds, slide.incoming_offset()))
                .or(swipe_offset);
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
                let mut path = Path::new();
                path.rect(seam.x, seam.y, seam.width, seam.height);
                self.canvas
                    .fill_path(&path, &Paint::color(color(theme.role(Role::Divider))));
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
        Ok(Snapshot {
            width,
            height,
            rgba,
            layout,
        })
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

fn validate_extent(width: u32, height: u32, scale: f32, limit: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > limit
        || height > limit
        || width as u64 * height as u64 > 64_000_000
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
