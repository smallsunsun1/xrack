use crate::{
    Canvas, Color, ImagePresentation, LayoutNode, Node, Paint, Path, Point, Rect, Renderer,
    WGPURenderer, canvas_transform, color, has_pixel_aligned_scale, image_colorize, rectangle,
};
use femtovg::{ImageFlags, ImageInfo, PixelFormat};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use xrack_runtime::{Color as UiColor, ImageAsset, ImageFit};
use xrack_runtime::{Value, ViewportRenderWork, scene_effect::SceneScissor, scene3d::SceneCamera};

pub(crate) fn scene_view_content(
    node: &Node,
    bounds: Rect,
) -> (Option<SceneCamera>, Rect, Option<[f64; 4]>) {
    let camera = SceneCamera::for_view(node, [f64::from(bounds.width), f64::from(bounds.height)]);
    let content = camera.map_or(bounds, |camera| {
        let [x, y, width, height] = camera.content_rect();
        Rect::new(
            bounds.x + x as f32,
            bounds.y + y as f32,
            width as f32,
            height as f32,
        )
    });
    let scissor = SceneScissor::for_view(node)
        .map(|scissor| {
            scissor.relative_to(camera.map_or(
                [0.0, 0.0, f64::from(bounds.width), f64::from(bounds.height)],
                |camera| camera.content_rect(),
            ))
        })
        .filter(|bounds| *bounds != [0.0, 0.0, 1.0, 1.0]);
    (camera, content, scissor)
}

/// Geometry in the custom element's local logical coordinates.
pub enum Geometry {
    Rectangle {
        bounds: Rect,
        color: UiColor,
    },
    Polyline {
        points: Vec<Point>,
        width: f32,
        color: UiColor,
    },
    Polygon {
        points: Vec<Point>,
        color: UiColor,
    },
    Image {
        bounds: Rect,
        image: ImageAsset,
    },
    Text {
        origin: Point,
        text: String,
        size: f32,
        color: UiColor,
    },
}

/// Data resolved from the current scene and layout before invoking a painter.
#[derive(Clone, Copy)]
pub struct PaintContext<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub data: &'a str,
    pub revision: i64,
}

/// Record commands to the element's transparent, premultiplied RGBA target.
pub struct GpuPaintContext<'a> {
    pub paint: PaintContext<'a>,
    pub device: &'a wgpu::Device,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub target: &'a wgpu::TextureView,
    pub size: [u32; 2],
    pub format: wgpu::TextureFormat,
}

/// An ordered GPU pass with viewport and scissor already restricted to the element.
pub struct GpuPassContext<'a, 'pass> {
    pub paint: PaintContext<'a>,
    pub device: &'a wgpu::Device,
    pub pass: &'a mut wgpu::RenderPass<'pass>,
    pub format: wgpu::TextureFormat,
    pub size: [u32; 2],
    pub inline: bool,
}

type GeometrySource = dyn Fn(PaintContext<'_>) -> Vec<Geometry>;
type GpuSource = dyn Fn(GpuPaintContext<'_>) -> Result<(), String>;
type PassSource = dyn Fn(GpuPassContext<'_, '_>) -> Result<(), String>;

enum Source {
    Geometry(Box<GeometrySource>),
    Gpu(Box<GpuSource>),
    Pass(Box<PassSource>),
}

thread_local! {
    static SOURCES: RefCell<BTreeMap<String, Weak<Source>>> = const { RefCell::new(BTreeMap::new()) };
}

/// Keep this handle alive on the UI thread while the renderer is registered.
#[must_use]
pub struct PaintRegistration {
    name: String,
    source: Rc<Source>,
}

impl Drop for PaintRegistration {
    fn drop(&mut self) {
        let _ = SOURCES.try_with(|sources| {
            let mut sources = sources.borrow_mut();
            if sources
                .get(&self.name)
                .is_some_and(|entry| entry.ptr_eq(&Rc::downgrade(&self.source)))
            {
                sources.remove(&self.name);
            }
        });
    }
}

/// Register the source of geometry for a `CustomRender` element on this UI thread.
pub fn register_geometry(
    name: impl Into<String>,
    source: impl Fn(PaintContext<'_>) -> Vec<Geometry> + 'static,
) -> Result<PaintRegistration, String> {
    register(name.into(), Source::Geometry(Box::new(source)))
}

/// Register a wgpu painter for a `CustomRender` element on this UI thread.
pub fn register_gpu(
    name: impl Into<String>,
    source: impl Fn(GpuPaintContext<'_>) -> Result<(), String> + 'static,
) -> Result<PaintRegistration, String> {
    register(name.into(), Source::Gpu(Box::new(source)))
}

/// Record directly into the frame target when the element has an aligned,
/// rectangular viewport. Other placements use a transparent intermediate.
/// Pipelines must use premultiplied alpha blending. Leave viewport and scissor
/// under the renderer's control.
pub fn register_gpu_pass(
    name: impl Into<String>,
    source: impl Fn(GpuPassContext<'_, '_>) -> Result<(), String> + 'static,
) -> Result<PaintRegistration, String> {
    register(name.into(), Source::Pass(Box::new(source)))
}

fn register(name: String, source: Source) -> Result<PaintRegistration, String> {
    if name.trim().is_empty() {
        return Err("a custom renderer needs a name".into());
    }
    SOURCES.with(|sources| {
        let mut sources = sources.borrow_mut();
        if sources.get(&name).and_then(Weak::upgrade).is_some() {
            return Err(format!("custom renderer {name:?} is already registered"));
        }
        let source = Rc::new(source);
        sources.insert(name.clone(), Rc::downgrade(&source));
        Ok(PaintRegistration { name, source })
    })
}

/// Drawable content in paint order within the current canvas transform and clip.
pub(crate) enum Primitive<'a> {
    Rectangle(rectangle::Solid),
    FillRect(Rect, Color),
    Shape {
        points: &'a [Point],
        stroke_width: Option<f32>,
        color: Color,
    },
    Image {
        bounds: Rect,
        clip: Option<Rect>,
        quad: Option<crate::image::QuadBounds>,
        asset: ImageAsset,
        presentation: ImagePresentation,
        colorize: image_colorize::Colorize<'a>,
        scale: f32,
    },
    Icon {
        bounds: Rect,
        asset: &'a ImageAsset,
        enabled: bool,
        tint: Option<Color>,
        scale: f32,
    },
    Vector {
        node: &'a Node,
        layout: &'a LayoutNode,
        scale: f32,
    },
    Text {
        origin: Point,
        text: &'a str,
        paint: &'a Paint,
        /// Passive table cells use the window's default entry text style.
        cell_clip: Option<Rect>,
        cell_fonts: Option<&'a crate::text::CellFonts>,
    },
    Label {
        node: &'a Node,
        layout: &'a LayoutNode,
        clip: Rect,
        tint: UiColor,
        scale: f32,
    },
    Native {
        bounds: Rect,
        context: PaintContext<'a>,
        source: &'a GpuSource,
    },
}

impl Renderer {
    pub(crate) fn draw_primitive(&mut self, primitive: Primitive<'_>) -> Result<(), String> {
        xrack_runtime::update_profile::draw_primitive();
        match primitive {
            Primitive::Rectangle(rectangle) => rectangle.draw(&mut self.canvas),
            Primitive::FillRect(bounds, color) => {
                fill_rect(&mut self.canvas, bounds, &Paint::color(color))
            }
            Primitive::Shape {
                points,
                stroke_width,
                color,
            } => {
                let Some((first, rest)) = points.split_first() else {
                    return Ok(());
                };
                if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                    return Ok(());
                }
                let mut path = Path::new();
                path.move_to(first.x, first.y);
                for point in rest {
                    path.line_to(point.x, point.y);
                }
                let mut paint = Paint::color(color);
                if let Some(width) = stroke_width {
                    if width.is_finite() && width > 0.0 && points.len() >= 2 {
                        paint.set_line_width(width);
                        self.canvas.stroke_path(&path, &paint);
                    }
                } else if points.len() >= 3 {
                    path.close();
                    self.canvas.fill_path(&path, &paint);
                }
            }
            Primitive::Image {
                bounds,
                clip,
                asset,
                presentation,
                colorize,
                scale,
                ..
            } => {
                if let Some(clip) = clip {
                    self.canvas.save();
                    self.canvas
                        .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
                }
                let result = self.images.draw(
                    &mut self.canvas,
                    &asset,
                    bounds,
                    scale,
                    presentation,
                    colorize,
                );
                if clip.is_some() {
                    self.canvas.restore();
                }
                result?;
            }
            Primitive::Icon {
                bounds,
                asset,
                enabled,
                tint,
                scale,
            } => {
                self.images
                    .draw_icon(&mut self.canvas, asset, bounds, scale, enabled, tint)?;
            }
            Primitive::Vector {
                node,
                layout,
                scale,
            } => self.paint_vector(node, layout, scale)?,
            Primitive::Text {
                origin,
                text,
                paint,
                cell_clip,
                cell_fonts,
            } => {
                if let (Some(clip), Some(fonts)) = (cell_clip, cell_fonts)
                    && self.draw_shaped_cell(origin, text, paint, clip, fonts)?
                {
                    return Ok(());
                }
                if let Some(clip) = cell_clip {
                    self.canvas.save();
                    self.canvas
                        .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
                }
                let result = self
                    .text
                    .fill_text(&mut self.canvas, origin.x, origin.y, text, paint);
                if cell_clip.is_some() {
                    self.canvas.restore();
                }
                let metrics = result.map_err(|error| format!("text drawing failed: {error}"))?;
                if !cfg!(any(target_os = "android", target_os = "ios"))
                    && let Some(clip) = cell_clip
                    && text.len() <= 128
                    && paint.text_align() == femtovg::Align::Left
                    && paint.text_baseline() == femtovg::Baseline::Top
                    && paint.letter_spacing() == 0.0
                    && self.display_batching
                    && self.can_cache_on_main_canvas()
                    && !self.retained_layers.vectors.opacity_passthrough
                    && paint.text_decoration().is_none()
                {
                    let transform = canvas_transform(&self.canvas);
                    if let Some([scale, sy]) = transform.axis_scales()
                        && scale == sy
                        && matches!(scale, 1.0 | 2.0 | 4.0)
                        && transform.translation() == [0.0, 0.0]
                        && crate::pixel_aligned(clip, scale)
                    {
                        self.glyph_masks.prime_fallback(
                            &mut self.canvas,
                            &self.text,
                            &metrics,
                            paint,
                            scale,
                        );
                    }
                }
            }
            Primitive::Label {
                node,
                layout,
                clip,
                tint,
                scale,
            } => {
                self.paint_group(clip, 1.0, |renderer| {
                    renderer.paint_label(node, layout, tint, scale)
                })?;
            }
            Primitive::Native {
                bounds,
                context,
                source,
            } => self.draw_native(bounds, context, wgpu::TextureFormat::Rgba8Unorm, source)?,
        }
        Ok(())
    }

    pub(crate) fn paint_custom(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        scale: f32,
    ) -> Result<(), String> {
        let bounds = item.bounds;
        let Some(name) = node.text("renderer") else {
            return Ok(());
        };
        let source = SOURCES.with(|sources| sources.borrow().get(name).and_then(Weak::upgrade));
        let Some(source) = source else {
            return Ok(());
        };
        if !usable_rect(bounds) {
            return Ok(());
        }
        let context = PaintContext {
            bounds: Rect::new(0.0, 0.0, bounds.width, bounds.height),
            scale,
            data: node.text("data").unwrap_or_default(),
            revision: node.int("revision").unwrap_or_default(),
        };
        match source.as_ref() {
            Source::Geometry(source) => {
                let geometry = source(context);
                self.paint_group(bounds, 1.0, |renderer| {
                    renderer.canvas.translate(bounds.x, bounds.y);
                    renderer.draw_geometry(&geometry, context)
                })
            }
            Source::Gpu(source) => self.draw_primitive(Primitive::Native {
                bounds,
                context,
                source: source.as_ref(),
            }),
            Source::Pass(source) => {
                let direct = path.len() == 1
                    && node.opacity() == 1.0
                    && !item.has_ancestor_clip_chain()
                    && item.clip.intersect(bounds) == bounds;
                self.draw_custom_pass(bounds, context, direct, source.as_ref())
            }
        }
        .map_err(|error| format!("custom renderer {name:?}: {error}"))
    }

    pub(crate) fn paint_native_item(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        scale: f32,
    ) -> Result<(), String> {
        use xrack_runtime::CustomItemDrawing;

        let bounds = layout.bounds;
        let Some(definition) = node.custom_item().filter(|_| usable_rect(bounds)) else {
            return Ok(());
        };
        let state = xrack_layout::item::custom_item_context(node, layout);
        let mut drawings = Vec::new();
        definition.paint(state, &mut drawings);
        let point = |[x, y]: [f32; 2]| Point { x, y };
        let geometry: Vec<_> = drawings
            .into_iter()
            .map(|drawing| match drawing {
                CustomItemDrawing::Fill { bounds, color } => Geometry::Rectangle {
                    bounds: Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
                    color,
                },
                CustomItemDrawing::Polyline {
                    points,
                    width,
                    color,
                } => Geometry::Polyline {
                    points: points.into_iter().map(point).collect(),
                    width,
                    color,
                },
                CustomItemDrawing::Polygon { points, color } => Geometry::Polygon {
                    points: points.into_iter().map(point).collect(),
                    color,
                },
                CustomItemDrawing::Image { bounds, image } => Geometry::Image {
                    bounds: Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
                    image,
                },
                CustomItemDrawing::Text {
                    origin,
                    text,
                    size,
                    color,
                } => Geometry::Text {
                    origin: point(origin),
                    text,
                    size,
                    color,
                },
            })
            .collect();
        let context = PaintContext {
            bounds: Rect::new(0.0, 0.0, bounds.width, bounds.height),
            scale,
            data: state.state.data,
            revision: state.state.revision,
        };
        self.paint_group(bounds, 1.0, |renderer| {
            renderer.canvas.translate(bounds.x, bounds.y);
            renderer.draw_geometry(&geometry, context)
        })
    }

    fn draw_geometry(
        &mut self,
        geometry: &[Geometry],
        context: PaintContext<'_>,
    ) -> Result<(), String> {
        let scale = context.scale;
        let mut display = crate::DisplayList::new(self, context.bounds, scale);
        let may_batch = !self.rectangle_batches.disabled
            && !self.retained_layers.vectors.opacity_passthrough
            && !self.using_capture_canvas()
            && has_pixel_aligned_scale(canvas_transform(&self.canvas), scale);
        for geometry in geometry {
            if let Geometry::Rectangle { bounds, color } = geometry
                && may_batch
                && let Some(rectangle) =
                    rectangle::Solid::from_rect(*bounds, context.bounds, *color, scale)
            {
                display.push(Primitive::Rectangle(rectangle));
                continue;
            }
            let primitive = match geometry {
                Geometry::Rectangle {
                    bounds,
                    color: tint,
                } => Primitive::FillRect(*bounds, color(*tint)),
                Geometry::Polyline {
                    points,
                    width,
                    color: tint,
                } => Primitive::Shape {
                    points,
                    stroke_width: Some(*width),
                    color: color(*tint),
                },
                Geometry::Polygon {
                    points,
                    color: tint,
                } => Primitive::Shape {
                    points,
                    stroke_width: None,
                    color: color(*tint),
                },
                Geometry::Image { bounds, image } if usable_rect(*bounds) => Primitive::Image {
                    quad: None,
                    bounds: *bounds,
                    clip: None,
                    asset: image.clone(),
                    presentation: ImagePresentation::whole(image.size(), ImageFit::Fill),
                    colorize: image_colorize::Colorize::Original,
                    scale,
                },
                Geometry::Image { .. } => continue,
                Geometry::Text {
                    origin,
                    text,
                    size,
                    color: tint,
                } => {
                    if size.is_finite()
                        && *size > 0.0
                        && origin.x.is_finite()
                        && origin.y.is_finite()
                    {
                        let paint = self.text.paint(*size, color(*tint), false);
                        display.submit(self)?;
                        self.draw_primitive(Primitive::Text {
                            origin: *origin,
                            text,
                            paint: &paint,
                            cell_clip: None,
                            cell_fonts: None,
                        })?;
                    }
                    continue;
                }
            };
            display.push(primitive);
        }
        display.submit(self)
    }

    pub(crate) fn paint_scene3d(
        &mut self,
        node: &Node,
        item: &LayoutNode,
        path: &[usize],
        scale: f32,
    ) -> Result<(), String> {
        use crate::scene3d::retained_view::{Sampling, ViewSize, viewport_format};

        let bounds = item.bounds;
        if !usable_rect(bounds) {
            self.scene_texture_sizes.record(node, [0; 2], false);
            return Ok(());
        }
        let (camera, content, scissor) = scene_view_content(node, bounds);
        let background = crate::scene3d::background(node);
        let alpha = background.0.to_be_bytes()[3];
        if content != bounds && alpha == 255 {
            fill_rect(&mut self.canvas, bounds, &Paint::color(color(background)));
        } else if content != bounds && alpha != 0 {
            let right = content.x + content.width;
            let bottom = content.y + content.height;
            let bars = [
                Rect {
                    width: content.x - bounds.x,
                    ..bounds
                },
                Rect {
                    x: right,
                    width: bounds.x + bounds.width - right,
                    ..bounds
                },
                Rect {
                    x: content.x,
                    width: content.width,
                    height: content.y - bounds.y,
                    ..bounds
                },
                Rect {
                    x: content.x,
                    y: bottom,
                    width: content.width,
                    height: bounds.y + bounds.height - bottom,
                },
            ];
            let paint = Paint::color(color(background));
            for bar in bars.into_iter().filter(|bar| usable_rect(*bar)) {
                fill_rect(&mut self.canvas, bar, &paint);
            }
        }
        if !usable_rect(content) {
            self.scene_texture_sizes.record(node, [0; 2], false);
            return Ok(());
        }
        if let Some(texture) = self.scene_item_textures.viewport_output(node, path) {
            let texture = self.viewport_display_texture(texture);
            self.composite_native_texture(content, texture)?;
            self.scene3d.stats.viewport_texture_composites += 1;
            return Ok(());
        }
        let (nominal, pixel_scale) = self.native_texture_extent(content);
        let texture_size = crate::scene3d::texture_extent(
            node,
            nominal,
            self.device.limits().max_texture_dimension_2d,
        );
        let context = PaintContext {
            bounds: content,
            scale,
            data: "",
            revision: 0,
        };
        let scope = [self.geometry_row, self.effect_mount];
        let effect_owner = crate::scene3d::effect_history::Owner {
            path,
            generation: self.scene_generation,
            scope,
            id: node.element_id(),
            logical_size: [content.width, content.height],
            revision: node.int("scene-effect-history-revision").unwrap_or(0),
        };
        let format = viewport_format(&self.device, node);
        if format == crate::scene3d::COLOR_FORMAT
            && node.scene_view_render_mode() != xrack_runtime::SceneViewRenderMode::Offscreen
            && path.len() == 1
            && node.opacity() == 1.0
            && !item.has_ancestor_clip_chain()
            && item.clip.intersect(content) == content
            && texture_size == nominal
            && let Some(camera) = camera
            && let Some((target, origin, extent)) = self.inline_surface(content, scale)
            && let Some(geometry) =
                crate::scene3d::prepare_inline_geometry(node, &camera, target.format())
        {
            let recording = self.scene3d.begin_viewport_recording(node);
            self.scene3d.visit_viewport(effect_owner, node, false);
            self.viewport_textures
                .clear_disabled_temporal((path, self.scene_generation, scope), Sampling::default());
            fill_rect(&mut self.canvas, content, &Paint::color(color(background)));
            self.submit_opacity_atlas();
            self.rectangle_batches.upload(&mut self.canvas)?;
            self.submit_canvas(&target);
            let view = target.create_view(&Default::default());
            let _gpu_scope = recording.as_ref().and_then(|recording| {
                self.viewport_gpu_timing.enter(
                    &self.device,
                    effect_owner.path,
                    effect_owner.id,
                    &recording.samples,
                )
            });
            let encoder = native_encoder(&mut self.custom_encoder, &self.device);
            self.scene3d.draw_inline(
                node,
                GpuPaintContext {
                    paint: context,
                    device: &self.device,
                    encoder,
                    target: &view,
                    size: extent,
                    format: target.format(),
                },
                &self.queue,
                effect_owner,
                camera,
                (origin, &geometry, scissor),
            );
            self.scene3d.finish_viewport_recording(
                recording,
                effect_owner,
                ViewportRenderWork::Inline,
                extent,
            );
            self.scene_texture_sizes.record(node, [0; 2], false);
            return Ok(());
        }
        let size = ViewSize {
            pixels: texture_size,
            logical: [content.width, content.height],
            scissor,
            format,
        };
        if let Some(output) = self.render_scene_output(
            node,
            context,
            camera,
            effect_owner,
            size,
            pixel_scale,
            false,
        )? {
            self.scene_texture_sizes.record(
                node,
                [output.texture.width(), output.texture.height()],
                false,
            );
            let texture = self.viewport_display_texture(output.texture);
            self.composite_native_texture(content, texture)?;
            self.scene3d.stats.viewport_texture_composites += 1;
        }
        Ok(())
    }

    pub(crate) fn render_scene_output(
        &mut self,
        node: &Node,
        context: PaintContext<'_>,
        camera: Option<SceneCamera>,
        effect_owner: crate::scene3d::effect_history::Owner<'_>,
        size: crate::scene3d::retained_view::ViewSize,
        pixel_scale: f32,
        export: bool,
    ) -> Result<Option<crate::scene3d::retained_view::ViewOutput>, String> {
        use crate::scene3d::retained_view::{Sampling, ViewInputs, ViewOutput, ViewTexture};

        let [width, height] = size.pixels;
        crate::validate_extent(
            width,
            height,
            pixel_scale,
            self.device.limits().max_texture_dimension_2d,
        )?;
        let recording = self.scene3d.begin_viewport_recording(node);
        let _gpu_scope = recording.as_ref().and_then(|recording| {
            self.viewport_gpu_timing.enter(
                &self.device,
                effect_owner.path,
                effect_owner.id,
                &recording.samples,
            )
        });
        let feedback = crate::scene3d::has_effect_history(node);
        let animate_feedback = feedback && node.boolean("scene-effect-animate", false);
        self.needs_redraw |= animate_feedback;
        let internal_passes = !node.scene_internal_passes_disabled();
        let reflections = if internal_passes
            && crate::scene3d::has_scene_reflections(node)
            && let Some(camera) = camera
        {
            let encoder = native_encoder(&mut self.custom_encoder, &self.device);
            self.scene3d.capture_reflections(
                node,
                (&self.device, &self.queue, encoder),
                (effect_owner, size.pixels),
                camera,
                context,
            )
        } else {
            crate::scene3d::ReflectionFrame::default()
        };
        self.needs_redraw |= reflections.needs_redraw;
        let cubes = reflections.cubes;
        self.scene3d.visit_viewport(effect_owner, node, feedback);
        let sampling = camera
            .filter(|_| internal_passes)
            .map_or_else(Sampling::default, |_| Sampling::from_node(node))
            .with_feedback(feedback, animate_feedback);
        let owner = (
            effect_owner.path,
            effect_owner.generation,
            effect_owner.scope,
        );
        self.viewport_textures
            .clear_disabled_temporal(owner, sampling);
        let cached = if export || sampling.is_active() || node.boolean("cache-rendering", false) {
            let automatic_camera = if node.boolean("auto-camera", false)
                && node.text("camera").is_none_or(str::is_empty)
                && let Some(Value::SceneObjects(model)) = node.properties.get("model")
            {
                model.automatic_camera_source()
            } else {
                None
            };
            let target = self.viewport_textures.get(
                &self.device,
                node,
                owner,
                size,
                ViewInputs {
                    reflections: std::array::from_fn(|index| {
                        cubes
                            .get(index)
                            .and_then(Option::as_ref)
                            .map_or(0, |cube| cube.revision)
                    }),
                    items: self.scene3d.item_texture_revision(node),
                    automatic_camera,
                },
                sampling,
            );
            self.scene3d.stats.viewport_cache_allocations = self.viewport_textures.allocations;
            target
        } else {
            None
        };
        let (output, sample) = match cached {
            Some(ViewTexture::Ready(output)) => {
                self.scene3d.stats.viewport_cache_hits =
                    self.scene3d.stats.viewport_cache_hits.saturating_add(1);
                self.scene3d.finish_viewport_recording(
                    recording,
                    effect_owner,
                    ViewportRenderWork::Retained,
                    size.pixels,
                );
                return Ok(Some(output));
            }
            Some(ViewTexture::Reprocess(output, frame)) => {
                let view = output.attachment();
                let encoder = native_encoder(&mut self.custom_encoder, &self.device);
                self.scene3d.reprocess_display(
                    node,
                    &mut GpuPaintContext {
                        paint: context,
                        device: &self.device,
                        encoder,
                        target: &view,
                        size: size.pixels,
                        format: size.format,
                    },
                    &self.queue,
                    &frame,
                );
                self.scene3d.finish_viewport_recording(
                    recording,
                    effect_owner,
                    ViewportRenderWork::DisplayOnly,
                    size.pixels,
                );
                return Ok(Some(output));
            }
            Some(ViewTexture::Render {
                output,
                sample,
                needs_redraw,
            }) => {
                self.needs_redraw |= needs_redraw;
                (output, sample)
            }
            None if export => return Ok(None),
            None => {
                let (texture, _) = self.transient_textures.acquire_render_view(
                    &self.device,
                    size.pixels,
                    size.format,
                );
                (
                    ViewOutput {
                        texture,
                        revision: 0,
                        readers: std::sync::Weak::new(),
                    },
                    None,
                )
            }
        };
        let view = output.attachment();
        let encoder = native_encoder(&mut self.custom_encoder, &self.device);
        let frame = self.scene3d.draw(
            node,
            GpuPaintContext {
                paint: context,
                device: &self.device,
                encoder,
                target: &view,
                size: size.pixels,
                format: size.format,
            },
            &self.queue,
            effect_owner,
            camera,
            (&cubes, sample.as_ref(), reflections.captures),
            (
                output.revision != 0 && sampling.retains_display(node),
                size.scissor,
            ),
        );
        if let Some(frame) = frame {
            self.viewport_textures.keep_display_frame(&output, frame);
        }
        self.scene3d.finish_viewport_recording(
            recording,
            effect_owner,
            ViewportRenderWork::Offscreen,
            size.pixels,
        );
        Ok(Some(output))
    }

    pub(crate) fn viewport_display_texture(&mut self, source: wgpu::Texture) -> wgpu::Texture {
        if source.format() == wgpu::TextureFormat::Rgba8Unorm {
            return source;
        }
        let (output, refresh) = self
            .viewport_textures
            .display_target(&self.device, &source)
            .unwrap_or_else(|| {
                let size = [source.width(), source.height()];
                let (texture, _) = self.transient_textures.acquire(&self.device, size);
                (texture, true)
            });
        if refresh {
            let encoder = native_encoder(&mut self.custom_encoder, &self.device);
            let target = output.create_view(&Default::default());
            self.scene3d.resolve_viewport_display(
                &self.device,
                encoder,
                (&source.create_view(&Default::default()), &target),
            );
        }
        output
    }

    fn draw_native(
        &mut self,
        bounds: Rect,
        context: PaintContext<'_>,
        format: wgpu::TextureFormat,
        source: &(impl Fn(GpuPaintContext<'_>) -> Result<(), String> + ?Sized),
    ) -> Result<(), String> {
        let extent = self.native_texture_extent(bounds);
        self.draw_native_at_size(bounds, context, format, extent, true, source)
    }

    fn native_texture_extent(&self, bounds: Rect) -> ([u32; 2], f32) {
        // Follow the effective canvas scale for nested transforms as well as HiDPI.
        let scale = canvas_transform(&self.canvas).max_scale();
        (
            [
                (bounds.width * scale).ceil() as u32,
                (bounds.height * scale).ceil() as u32,
            ],
            scale,
        )
    }

    fn draw_native_at_size(
        &mut self,
        bounds: Rect,
        context: PaintContext<'_>,
        format: wgpu::TextureFormat,
        (size, scale): ([u32; 2], f32),
        clear_reused: bool,
        source: &(impl Fn(GpuPaintContext<'_>) -> Result<(), String> + ?Sized),
    ) -> Result<(), String> {
        let limit = self.device.limits().max_texture_dimension_2d;
        crate::validate_extent(size[0], size[1], scale, limit)?;
        let (texture, reused) =
            self.transient_textures
                .acquire_render_view(&self.device, size, format);
        self.draw_native_texture(
            bounds,
            context,
            format,
            texture,
            clear_reused && reused,
            source,
        )
    }

    fn draw_native_texture(
        &mut self,
        bounds: Rect,
        context: PaintContext<'_>,
        format: wgpu::TextureFormat,
        texture: wgpu::Texture,
        clear: bool,
        source: &(impl Fn(GpuPaintContext<'_>) -> Result<(), String> + ?Sized),
    ) -> Result<(), String> {
        let size = [texture.width(), texture.height()];
        let target = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        let encoder = native_encoder(&mut self.custom_encoder, &self.device);
        // Preserve the same transparent starting surface as a fresh texture.
        if clear {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("XRack clear recycled custom target"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        source(GpuPaintContext {
            paint: context,
            device: &self.device,
            encoder,
            target: &target,
            size,
            format,
        })?;
        self.composite_native_texture(bounds, texture)
    }

    fn composite_native_texture(
        &mut self,
        bounds: Rect,
        texture: wgpu::Texture,
    ) -> Result<(), String> {
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED,
            texture.width() as usize,
            texture.height() as usize,
            PixelFormat::Rgba8,
        );
        let image = self
            .canvas
            .create_image_from_native_texture(texture, info)
            .map_err(|error| format!("could not import the custom texture: {error}"))?;
        let paint = Paint::image(
            image,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            0.0,
            1.0,
        );
        fill_rect(&mut self.canvas, bounds, &paint);
        self.canvas.delete_image(image);
        Ok(())
    }

    fn draw_custom_pass(
        &mut self,
        bounds: Rect,
        context: PaintContext<'_>,
        direct: bool,
        source: &PassSource,
    ) -> Result<(), String> {
        if direct && let Some((target, origin, extent)) = self.inline_surface(bounds, context.scale)
        {
            self.submit_opacity_atlas();
            self.rectangle_batches.upload(&mut self.canvas)?;
            self.submit_canvas(&target);
            let view = target.create_view(&Default::default());
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack inline custom pass"),
                });
            record_gpu_pass(
                GpuPaintContext {
                    paint: context,
                    device: &self.device,
                    encoder: &mut encoder,
                    target: &view,
                    size: extent,
                    format: target.format(),
                },
                Some(origin),
                source,
            )?;
            xrack_runtime::update_profile::submit_gpu();
            self.queue.submit([encoder.finish()]);
            return Ok(());
        }
        self.draw_native(bounds, context, wgpu::TextureFormat::Rgba8Unorm, &|ctx| {
            record_gpu_pass(ctx, None, source)
        })
    }

    fn inline_surface(
        &self,
        bounds: Rect,
        scale: f32,
    ) -> Option<(wgpu::Texture, [u32; 2], [u32; 2])> {
        let target = self.inline_target.as_ref()?;
        let device_rect = bounds.scaled(scale);
        let extent = [device_rect.width as u32, device_rect.height as u32];
        let origin = [device_rect.x as u32, device_rect.y as u32];
        (bounds.x >= 0.0
            && bounds.y >= 0.0
            && extent[0] > 0
            && extent[1] > 0
            && crate::pixel_aligned(bounds, scale)
            && crate::has_device_scale(canvas_transform(&self.canvas), scale)
            && origin[0].saturating_add(extent[0]) <= target.width()
            && origin[1].saturating_add(extent[1]) <= target.height())
        .then(|| (target.clone(), origin, extent))
    }
}

fn native_encoder<'a>(
    encoder: &'a mut Option<wgpu::CommandEncoder>,
    device: &wgpu::Device,
) -> &'a mut wgpu::CommandEncoder {
    encoder.get_or_insert_with(|| {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("XRack custom draw commands"),
        })
    })
}

fn record_gpu_pass(
    ctx: GpuPaintContext<'_>,
    origin: Option<[u32; 2]>,
    source: &PassSource,
) -> Result<(), String> {
    let inline = origin.is_some();
    let [x, y] = origin.unwrap_or_default();
    let [width, height] = ctx.size;
    let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("XRack custom GPU pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: ctx.target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if inline {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
    pass.set_scissor_rect(x, y, width, height);
    source(GpuPassContext {
        paint: ctx.paint,
        device: ctx.device,
        pass: &mut pass,
        format: ctx.format,
        size: ctx.size,
        inline,
    })
}

fn usable_rect(bounds: Rect) -> bool {
    bounds.width > 0.0
        && bounds.height > 0.0
        && [bounds.x, bounds.y, bounds.width, bounds.height]
            .into_iter()
            .all(f32::is_finite)
}

fn fill_rect(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, paint: &Paint) {
    if usable_rect(bounds) {
        let mut path = Path::new();
        path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        canvas.fill_path(&path, paint);
    }
}

impl Renderer {
    pub(crate) fn cell_shaping_scale(&self, transform: crate::CanvasTransform) -> Option<f32> {
        if cfg!(any(
            xrack_femtovg_glyph_samples,
            target_os = "android",
            target_os = "ios"
        )) || !self.display_batching
            || !self.can_cache_on_main_canvas()
            || self.glyph_masks.defer_preparation
            || self.retained_layers.vectors.opacity_passthrough
        {
            return None;
        }
        let [scale, sy] = transform.axis_scales()?;
        (scale == sy && matches!(scale, 1.0 | 2.0 | 4.0) && transform.translation() == [0.0, 0.0])
            .then_some(scale)
    }

    fn draw_shaped_cell(
        &mut self,
        origin: Point,
        text: &str,
        paint: &Paint,
        clip: Rect,
        fonts: &crate::text::CellFonts,
    ) -> Result<bool, String> {
        if text.len() > 128
            || paint.text_align() != femtovg::Align::Left
            || paint.text_baseline() != femtovg::Baseline::Top
            || paint.letter_spacing() != 0.0
            || !paint.text_decoration().is_none()
        {
            return Ok(false);
        }
        let transform = canvas_transform(&self.canvas);
        let Some(scale) = self.cell_shaping_scale(transform) else {
            return Ok(false);
        };
        if !crate::pixel_aligned(clip, scale) {
            return Ok(false);
        }
        let measurement_scale = crate::cell_glyphs::measurement_scale(scale);
        let settings = crate::cell_glyphs::Settings {
            font_size: paint.font_size().to_bits(),
            measurement_scale: measurement_scale.to_bits(),
            fonts: self.text.registered_font_counts(),
            cell_fonts: Some(*fonts),
        };
        let positions = {
            let mut cache = self.text.cell_glyphs.borrow_mut();
            let Some(run) = cache.portable_run(text, settings, || {
                self.text
                    .shape_cell(text, paint.font_size() * measurement_scale, fonts)
            }) else {
                return Ok(false);
            };
            crate::cell_glyphs::placed_glyphs(
                run.glyphs.iter().copied(),
                run.baseline,
                [origin.x, origin.y],
                measurement_scale,
            )
            .collect::<Vec<_>>()
        };
        self.canvas.save();
        self.canvas
            .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        // Preserve consecutive font runs and the original cell scissor.
        let result = positions.chunk_by(|a, b| a.0 == b.0).try_for_each(|run| {
            self.canvas.fill_glyph_run(
                run[0].0,
                &[],
                run.iter()
                    .map(|&(_, glyph_id, x, y)| femtovg::PositionedGlyph { x, y, glyph_id }),
                paint,
            )
        });
        self.canvas.restore();
        result.map_err(|error| format!("text drawing failed: {error}"))?;
        self.glyph_masks.prime_positioned(
            &mut self.canvas,
            &self.text,
            positions
                .iter()
                .map(|&(font, id, x, y)| (font, id, x, y, true)),
            paint,
            scale,
        );
        Ok(true)
    }
}
