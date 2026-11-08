//! wgpu-backed 2D renderer. No windowing dependency and no compiler/MLIR dependency.
mod controls;
mod entry;
mod image;
mod range;
mod text;
mod vector;
use femtovg::{Canvas, Color, Paint, Path, Solidity, renderer::WGPURenderer};
use std::{
    fs::File,
    io::BufWriter,
    path::Path as FilePath,
    rc::Rc,
    time::{Duration, Instant},
};
pub use text::TextSystem;
use xrack_layout::{
    ButtonContent, ComboCompletion, Interaction, LayoutNode, Rect, Size, SpinGeometry,
    StackTransitions, TreeRowGeometry, dialog_title_height, font_size,
};
use xrack_runtime::{
    Brush, Color as UiColor, Component, GradientStop, ImageFit, Node, StandardButtonKind, Value,
    button_text, combo_editable, group_index, is_button, is_multiline_text_input, is_spin_box,
    is_text_input, is_toggle, is_window_overlay, spin_editable, spin_text, text_input_value,
};

#[derive(Clone, Copy)]
pub struct Theme {
    pub window: UiColor,
    pub text: UiColor,
    pub button: UiColor,
    pub disabled: UiColor,
    pub disabled_text: UiColor,
    pub focus: UiColor,
}

#[derive(Clone, Copy)]
struct PaintFrame<'a> {
    input: &'a Interaction,
    scale: f32,
    transitions: Option<&'a StackTransitions>,
    indicator_phase: Option<f32>,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            window: UiColor(0x18202bff),
            text: UiColor(0xf2f5faff),
            button: UiColor(0x536dfeff),
            disabled: UiColor(0x3a4350ff),
            disabled_text: UiColor(0x85909fff),
            focus: UiColor(0xadc2ffff),
        }
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
    let tinted = |value| color(tint(value, shade));
    let stop = |stop: &GradientStop| (stop.offset as f32, tinted(stop.color));
    let cx = bounds.x + bounds.width * 0.5;
    let cy = bounds.y + bounds.height * 0.5;
    Some(match node.properties.get(name)? {
        Value::Color(value) | Value::Brush(Brush::Solid(value)) => Paint::color(tinted(*value)),
        Value::Brush(Brush::Linear {
            angle_degrees,
            stops: colors,
        }) => {
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
        Value::Brush(Brush::Radial { stops: colors }) => {
            let radius = bounds.width.hypot(bounds.height) * 0.5;
            Paint::radial_gradient_stops(cx, cy, 0.0, radius, colors.iter().map(stop))
        }
        _ => return None,
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
    images: image::Images,
    clock: Instant,
    needs_redraw: bool,
    painted_combo_completion: Option<ComboCompletion>,
    pub text: TextSystem,
    pub theme: Theme,
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
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: surface,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no compatible GPU adapter: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("XRack"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("GPU device creation failed: {e}"))?;
        let canvas = Canvas::new_with_text_context(
            WGPURenderer::new(device.clone(), queue.clone()),
            text.context.clone(),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            canvas,
            images: image::Images::default(),
            clock: Instant::now(),
            needs_redraw: false,
            painted_combo_completion: None,
            text,
            theme: Theme::default(),
        })
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
        xrack_layout::prepare_scene_with_input(component, scene, viewport, &mut self.text, input)
    }

    /// An animated element was visible in the last frame.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn take_painted_combo_completion(&mut self) -> Option<ComboCompletion> {
        self.painted_combo_completion.take()
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
                transitions: Some(transitions),
                indicator_phase: Some(indicator_phase(self.clock.elapsed())),
            },
        )
    }

    fn paint_frame(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let PaintFrame { input, scale, .. } = frame;
        self.needs_redraw = false;
        self.painted_combo_completion = None;
        let size = target.size();
        self.canvas.set_size(size.width, size.height, scale);
        self.canvas.reset();
        self.canvas
            .clear_rect(0, 0, size.width, size.height, color(self.theme.window));
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
                    .fill_path(&backdrop, &Paint::color(Color::rgba(8, 13, 21, 168)));
            }
            self.paint(node, item, &mut path.to_vec(), frame, false)?;
        }
        if let Some(menu) = input.choice_menu(scene, layout) {
            controls::menu(&mut self.canvas, &self.text, self.theme, menu)?;
        }
        for panel in input.menu_panels(scene, layout) {
            controls::menu_panel(
                &mut self.canvas,
                &self.text,
                self.theme,
                panel,
                &mut self.images,
                scale,
            )?;
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
                    transitions: None,
                    indicator_phase: frame.indicator_phase,
                },
                false,
            )?;
        }
        self.queue.submit(self.canvas.flush_to_output(target));
        self.images.end_frame(&mut self.canvas);
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
        let bounds = layout.bounds;
        let clip = layout.clip.intersect(bounds);
        if !layout.visible || clip.width <= 0.0 || clip.height <= 0.0 {
            return Ok(());
        }
        self.canvas.save();
        self.canvas
            .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        let result = self.paint_element(node, layout, path, frame, in_layout);
        self.canvas.restore();
        result
    }

    fn paint_element(
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
            transitions,
            indicator_phase,
        } = frame;
        let bounds = layout.bounds;
        let group_header =
            (node.kind == "GroupBox").then(|| xrack_layout::group_header(node, bounds));
        if node.kind == "SplitHandle" {
            controls::split_handle(
                &mut self.canvas,
                node,
                bounds,
                self.theme,
                layout.enabled,
                input.is_hovered(path) || input.is_focused(path),
            );
            return Ok(());
        }
        if node.kind == "Menu" {
            return controls::menu_header(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                bounds,
                layout.enabled,
                input.menu_header_open(path) || input.is_hovered(path) || input.is_focused(path),
            );
        }
        let is_button = is_button(node.kind);
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
                    | "Drawer"
                    | "MenuBar"
                    | "ToolBar"
                    | "Calendar"
                    | "Tumbler"
            );
        let checked = is_button
            && (node.kind == "DelayButton" || node.boolean("checkable", false))
            && node.boolean("checked", false);
        let background = if !is_button {
            brush_paint(node, "background", bounds, 0.0)
                .or_else(|| boxed.then(|| Paint::color(color(UiColor(0x27354dff)))))
        } else if !layout.enabled {
            let shade = if checked { -0.2 } else { 0.0 };
            Some(Paint::color(color(tint(self.theme.disabled, shade))))
        } else {
            let shade = if input.is_pressed(path) {
                -0.18
            } else if input.is_hovered(path) {
                0.12
            } else {
                0.0
            } - if checked { 0.28 } else { 0.0 };
            brush_paint(node, "background", bounds, shade).or_else(|| {
                let base = match node.standard_button_kind() {
                    Some(StandardButtonKind::Discard) => UiColor(0xa63843ff),
                    Some(_) if !node.boolean("primary", false) => UiColor(0x384966ff),
                    _ => self.theme.button,
                };
                (node.kind != "ToolButton"
                    || checked
                    || input.is_hovered(path)
                    || input.is_pressed(path))
                .then(|| Paint::color(color(tint(base, shade))))
            })
        };
        let radii = corner_radii(node, bounds);
        if let Some(background) = background {
            let mut path = Path::new();
            append_rect(&mut path, bounds, radii);
            self.canvas.fill_path(&path, &background);
        }
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
                let tint = node.color("progress-color").unwrap_or(UiColor(0x366483ff));
                self.canvas.fill_path(&path, &Paint::color(color(tint)));
                self.canvas.restore();
            }
            self.needs_redraw |= pending && layout.enabled;
        }
        if dialog_title_height(node) > 0.0 {
            let paint = self.text.paint(
                font_size(node),
                color(node.color("color").unwrap_or(self.theme.text)),
                false,
            );
            self.canvas
                .fill_text(
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
            let result = if node.group_checkable() {
                entry::toggle(
                    &mut self.canvas,
                    &self.text,
                    self.theme,
                    node,
                    header,
                    layout.enabled,
                )
            } else {
                let size = font_size(node);
                let foreground = if layout.enabled {
                    node.color("color").unwrap_or(self.theme.text)
                } else {
                    self.theme.disabled_text
                };
                self.canvas
                    .fill_text(
                        header.x,
                        header.y + ((header.height - size * 1.4) * 0.5).max(0.0),
                        node.text("title").unwrap_or(""),
                        &self.text.paint(size, color(foreground), false),
                    )
                    .map(|_| ())
                    .map_err(|error| format!("group title drawing failed: {error}"))
            };
            self.canvas.restore();
            result?;
        }
        let border = node
            .length("border-width")
            .unwrap_or(if group_header.is_some() { 1.0 } else { 0.0 })
            .max(0.0)
            .min(bounds.width.min(bounds.height));
        if border > 0.0 {
            let mut path = Path::new();
            append_rect(&mut path, bounds, radii);
            let inner = bounds.inset(border);
            if inner.width > 0.0 && inner.height > 0.0 {
                append_rect(&mut path, inner, radii.map(|r| (r - border).max(0.0)));
                path.solidity(Solidity::Hole);
            }
            let paint = brush_paint(node, "border-color", bounds, 0.0)
                .unwrap_or_else(|| Paint::color(color(UiColor(0x75859aff))));
            self.canvas.fill_path(&path, &paint);
        }
        if (is_button
            || boxed
            || is_toggle(node.kind)
            || is_cell
            || matches!(
                node.kind,
                "Slider" | "RadioButton" | "SwipeView" | "PageIndicator"
            ))
            && input.is_focused(path)
            && layout.enabled
        {
            let mut paint = Paint::color(color(self.theme.focus));
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
        if node.kind == "Image"
            && let Some(image) = node.image("source")
        {
            let fit = node.image_fit("image-fit").unwrap_or(if in_layout {
                ImageFit::Contain
            } else {
                ImageFit::Fill
            });
            self.images
                .draw(&mut self.canvas, image, bounds, fit, scale)?;
        } else if node.kind == "Path" {
            vector::draw_path(&mut self.canvas, node, bounds)?;
        }
        if node.kind == "RadioGroup" {
            controls::group_title(&mut self.canvas, &self.text, self.theme, node, bounds)?;
        } else if matches!(node.kind, "TabWidget" | "TabBar") {
            let selected = group_index(node);
            let standalone = node.kind == "TabBar";
            for (index, (tab, page)) in node.children.iter().zip(&layout.children).enumerate() {
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
                let result = controls::tab(
                    &mut self.canvas,
                    &self.text,
                    self.theme,
                    header,
                    tab.text(if standalone { "text" } else { "title" })
                        .unwrap_or(""),
                    controls::TabAppearance {
                        selected: selected == Some(index),
                        enabled: page.enabled,
                        focused: input.is_focused(path),
                    },
                );
                path.pop();
                result?;
            }
        }
        if let Some(wheel) = &layout.tumbler {
            controls::tumbler(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                wheel,
                layout.enabled,
            )?;
        } else if node.kind == "Calendar" {
            controls::calendar(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                &xrack_layout::CalendarGeometry::new(node, bounds),
                layout.enabled,
            )?;
        } else if node.kind == "Slider" {
            range::slider(
                &mut self.canvas,
                self.theme,
                node,
                bounds,
                layout.enabled,
                input
                    .pressed
                    .as_ref()
                    .is_some_and(|target| target.path == *path),
            );
        } else if node.kind == "Dial" {
            range::dial(
                &mut self.canvas,
                self.theme,
                node,
                bounds,
                layout.enabled,
                input.is_focused(path) || input.is_pressed(path),
            );
        } else if node.kind == "RangeSlider" {
            let active = [0, 1].map(|index| {
                path.push(index);
                let active = input.is_focused(path) || input.is_pressed(path);
                path.pop();
                active
            });
            range::interval(
                &mut self.canvas,
                self.theme,
                node,
                bounds,
                layout.enabled,
                active,
            );
        } else if node.kind == "PageIndicator" {
            if let Some(geometry) = xrack_layout::PageIndicatorGeometry::new(node, bounds) {
                controls::page_indicator(
                    &mut self.canvas,
                    self.theme,
                    node,
                    &geometry,
                    layout.enabled,
                    |index| input.page_marker_hovered(path, index),
                );
            }
        } else if let Some(indicator) = xrack_runtime::indicator_state(node) {
            let phase =
                indicator_phase.filter(|_| layout.enabled && node.boolean("animated", true));
            self.needs_redraw |= range::indicator(
                &mut self.canvas,
                self.theme,
                node,
                bounds,
                indicator,
                phase,
                layout.enabled,
            );
        } else if node.kind == "StandardListItem" {
            controls::label(
                &mut self.canvas,
                &self.text,
                Rect::new(
                    bounds.x + 12.0,
                    bounds.y,
                    (bounds.width - 24.0).max(0.0),
                    bounds.height,
                ),
                node.text("text").unwrap_or("").lines().next().unwrap_or(""),
                font_size(node),
                if layout.enabled {
                    self.theme.text
                } else {
                    self.theme.disabled_text
                },
            )?;
        } else if node.kind == "RadioButton" {
            controls::radio(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                bounds,
                layout.enabled,
            )?;
        } else if is_toggle(node.kind) {
            entry::toggle(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                bounds,
                layout.enabled,
            )?;
        } else if node.kind == "ComboBox" && !editable_combo {
            controls::combo(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                bounds,
                layout.enabled,
                input.choice_is_open_for(path),
            )?;
        } else if is_input || is_cell || is_spin_box(node.kind) {
            let (area, value) = if editable_combo {
                controls::combo(
                    &mut self.canvas,
                    &self.text,
                    self.theme,
                    node,
                    bounds,
                    layout.enabled,
                    input.choice_is_open_for(path),
                )?;
                (
                    xrack_layout::ComboGeometry::new(bounds).entry,
                    std::borrow::Cow::Borrowed(text_input_value(node)),
                )
            } else if node.kind == "SearchField" {
                let geometry = xrack_layout::SearchGeometry::new(bounds);
                controls::search_icons(
                    &mut self.canvas,
                    self.theme,
                    node,
                    &geometry,
                    layout.enabled,
                );
                (
                    geometry.entry,
                    std::borrow::Cow::Borrowed(text_input_value(node)),
                )
            } else if is_spin_box(node.kind) {
                let geometry = SpinGeometry::new(bounds);
                controls::spin_buttons(
                    &mut self.canvas,
                    &geometry,
                    self.theme,
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
            let editor = input
                .editor
                .as_ref()
                .filter(|_| input.is_focused(path) && (is_input || spin_editable(node)));
            let completion = if editable_combo && layout.enabled {
                input.combo_completion(node, path)
            } else {
                None
            };
            let entry = entry::EntryText {
                value: &value,
                editor,
                scroll: input.text_scroll(path),
                completion: completion
                    .as_ref()
                    .map(|candidate| (candidate.suffix(), layout.clip)),
            };
            if is_multiline_text_input(node.kind) {
                entry::draw_area(
                    &mut self.canvas,
                    &self.text,
                    self.theme,
                    node,
                    area,
                    entry,
                    layout.enabled,
                )?;
            } else if entry::draw(
                &mut self.canvas,
                &self.text,
                self.theme,
                node,
                area,
                entry,
                layout.enabled,
            )? {
                self.painted_combo_completion = completion;
            }
        } else if xrack_layout::uses_label_layout(node) {
            let tint = if layout.enabled {
                node.color("color").unwrap_or(self.theme.text)
            } else {
                self.theme.disabled_text
            };
            let paint = self.text.paint(font_size(node), color(tint), false);
            let label = self
                .text
                .label_layout(node, Size::new(bounds.width, bounds.height));
            for line in label.lines {
                if !line.text.is_empty() {
                    self.canvas
                        .fill_text(
                            bounds.x + line.origin.x,
                            bounds.y + line.origin.y,
                            &*line.text,
                            &paint,
                        )
                        .map_err(|error| format!("text drawing failed: {error}"))?;
                }
            }
        } else if node.kind == "Text" || is_button {
            let button = is_button.then(|| ButtonContent::new(node));
            let text = button
                .as_ref()
                .map_or_else(|| button_text(node), |button| button.label);
            let text_color = if !layout.enabled {
                self.theme.disabled_text
            } else {
                node.color("color").unwrap_or(self.theme.text)
            };
            let area = button
                .as_ref()
                .map_or(bounds, |button| bounds.inset(button.padding));
            let icon = button
                .as_ref()
                .and_then(|button| button.icon)
                .map(|(image, size)| (image, size.min(area.width).min(area.height)))
                .filter(|&(_, size)| size > 0.0);
            let gap = button
                .as_ref()
                .filter(|_| icon.is_some())
                .map_or(0.0, ButtonContent::gap);
            let icon_width = icon.map_or(0.0, |(_, size)| size + gap);
            let label_width = (area.width - icon_width).max(0.0);
            let paint = self
                .text
                .paint(font_size(node), color(text_color), is_button);
            let lines = self.text.lines(text, &paint, label_width);
            let line_height = font_size(node) * 1.4;
            let y = if is_button {
                bounds.y + ((bounds.height - line_height * lines.len() as f32) * 0.5).max(0.0)
            } else {
                bounds.y
            };
            let mut label_left = area.x;
            let x = if let Some((image, size)) = icon {
                let label = lines
                    .iter()
                    .map(|range| self.text.advance(&text[range.clone()], font_size(node)))
                    .fold(0.0_f32, f32::max)
                    .min(label_width);
                let left = area.x + (area.width - icon_width - label).max(0.0) * 0.5;
                let rect = Rect::new(left, area.y + (area.height - size) * 0.5, size, size);
                self.images
                    .draw_icon(&mut self.canvas, image, rect, scale, layout.enabled)?;
                label_left = left + icon_width;
                label_left + label * 0.5
            } else if is_button {
                area.x + area.width * 0.5
            } else {
                area.x
            };
            if icon.is_some() {
                self.canvas.save();
                self.canvas.intersect_scissor(
                    label_left,
                    bounds.y,
                    (area.x + area.width - label_left).max(0.0),
                    bounds.height,
                );
            }
            let result = (|| {
                for (i, range) in lines.iter().enumerate() {
                    let y = y + i as f32 * line_height;
                    if y >= bounds.y + bounds.height {
                        break;
                    }
                    self.canvas
                        .fill_text(x, y, &text[range.clone()], &paint)
                        .map_err(|e| format!("text drawing failed: {e}"))?;
                }
                Ok::<(), String>(())
            })();
            if icon.is_some() {
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
                self.theme,
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
        }
        let is_layout = xrack_layout::is_layout(node.kind);
        let slide = transitions.and_then(|transitions| transitions.at(path));
        let slide_viewport = bounds.intersect(layout.clip);
        if let Some(slide) = slide {
            let frozen_input = Interaction::default();
            let frozen = PaintFrame {
                input: &frozen_input,
                scale,
                transitions: None,
                indicator_phase: None,
            };
            for (page, page_layout, offset, visible) in slide.outgoing() {
                self.canvas.save();
                self.canvas.intersect_scissor(
                    slide_viewport.x + visible.start * slide_viewport.width,
                    slide_viewport.y,
                    (visible.end - visible.start) * slide_viewport.width,
                    slide_viewport.height,
                );
                self.canvas.translate(offset * slide_viewport.width, 0.0);
                let result = self.paint(page, page_layout, &mut Vec::new(), frozen, false);
                self.canvas.restore();
                result?;
            }
        }
        for (i, (node, layout)) in node.children.iter().zip(&layout.children).enumerate() {
            if is_window_overlay(node.kind) {
                continue;
            }
            path.push(i);
            let result = if let Some(slide) = slide {
                self.canvas.save();
                self.canvas
                    .translate(slide.incoming_offset() * slide_viewport.width, 0.0);
                let result = self.paint(node, layout, path, frame, is_layout);
                self.canvas.restore();
                result
            } else {
                self.paint(node, layout, path, frame, is_layout)
            };
            path.pop();
            result?;
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
                    self.theme.text
                } else {
                    self.theme.disabled_text
                })),
            );
        }
        if node.kind == "SwipeDelegate" && input.is_focused(path) && layout.enabled {
            let mut outline = Path::new();
            append_rect(&mut outline, bounds.inset(2.0), [0.0; 4]);
            let mut paint = Paint::color(color(self.theme.focus));
            paint.set_line_width(2.0);
            self.canvas.stroke_path(&outline, &paint);
        }
        if let Some(grip) = xrack_layout::drawer_grip(node, bounds) {
            let mut path = Path::new();
            path.rounded_rect(grip.x, grip.y, grip.width, grip.height, 1.5);
            self.canvas
                .fill_path(&path, &Paint::color(Color::rgba(155, 180, 227, 160)));
        }
        if node.kind == "ListRow" {
            controls::list_row(
                &mut self.canvas,
                bounds,
                node.boolean("selected", false),
                input.is_hovered(path) && layout.enabled,
                node.boolean("current", false)
                    && path
                        .split_last()
                        .is_some_and(|(_, table)| input.is_focused(table)),
            );
            if node.boolean("tree-has-children", false)
                && let Some(tree) = TreeRowGeometry::new(node, bounds)
            {
                controls::tree_indicator(
                    &mut self.canvas,
                    tree.indicator,
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
            controls::list_row(&mut self.canvas, bounds, cell_selected, false, cell_current);
        }
        for bar in layout.scroll_bars(node) {
            range::scroll_bar(
                &mut self.canvas,
                self.theme,
                bar,
                layout.enabled,
                input.scroll_bar_highlighted(path, bar.orientation),
            );
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
            PaintFrame {
                input,
                scale,
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
            PaintFrame {
                input,
                scale,
                transitions: None,
                indicator_phase: Some(indicator_phase(elapsed)),
            },
        )
    }

    fn snapshot_frame(
        &mut self,
        scene: &Node,
        width: u32,
        height: u32,
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
        let layout = self.layout_with_input(
            scene,
            Size::new(width as f32 / scale, height as f32 / scale),
            frame.input,
        );
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

fn corner_radii(node: &Node, bounds: Rect) -> [f32; 4] {
    let radius = node
        .length("border-radius")
        .unwrap_or(if node.kind == "Dialog" {
            12.0
        } else if is_button(node.kind) || is_text_input(node.kind) {
            7.0
        } else {
            0.0
        });
    let radii = [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
    ]
    .map(|name| node.length(name).unwrap_or(radius).max(0.0));
    let [tl, tr, br, bl] = radii;
    let scale = [
        (bounds.width, tl + tr),
        (bounds.width, bl + br),
        (bounds.height, tl + bl),
        (bounds.height, tr + br),
    ]
    .into_iter()
    .filter(|(_, sum)| *sum > 0.0)
    .fold(1.0_f32, |scale, (side, sum)| scale.min(side / sum));
    radii.map(|radius| radius * scale)
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
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(path).map_err(|e| e.to_string())?),
            self.width,
            self.height,
        );
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
