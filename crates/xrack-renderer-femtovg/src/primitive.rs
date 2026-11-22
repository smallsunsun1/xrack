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
pub(super) enum Primitive<'a> {
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
        clip: Option<Rect>,
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
    pub(super) fn draw_primitive(&mut self, primitive: Primitive<'_>) -> Result<(), String> {
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
                clip,
            } => {
                if let Some(clip) = clip {
                    self.canvas.save();
                    self.canvas
                        .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
                }
                let result = self
                    .text
                    .fill_text(&mut self.canvas, origin.x, origin.y, text, paint);
                if clip.is_some() {
                    self.canvas.restore();
                }
                result.map_err(|error| format!("text drawing failed: {error}"))?;
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
            } => self.draw_native(bounds, context, source)?,
        }
        Ok(())
    }

    pub(super) fn paint_custom(
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
                            clip: None,
                        })?;
                    }
                    continue;
                }
            };
            display.push(primitive);
        }
        display.submit(self)
    }

    fn draw_native(
        &mut self,
        bounds: Rect,
        context: PaintContext<'_>,
        source: &(impl Fn(GpuPaintContext<'_>) -> Result<(), String> + ?Sized),
    ) -> Result<(), String> {
        // Follow the effective canvas scale for nested transforms as well as HiDPI.
        let scale = canvas_transform(&self.canvas).max_scale();
        let size = [
            (bounds.width * scale).ceil() as u32,
            (bounds.height * scale).ceil() as u32,
        ];
        let limit = self.device.limits().max_texture_dimension_2d;
        crate::validate_extent(size[0], size[1], scale, limit)?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let (texture, reused) = self.transient_textures.acquire(&self.device, size);
        let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack custom draw commands"),
                })
        });
        // Preserve the same transparent starting surface as a fresh texture.
        if reused {
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
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED,
            size[0] as usize,
            size[1] as usize,
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
        let device_rect = bounds.scaled(context.scale);
        let extent = [device_rect.width as u32, device_rect.height as u32];
        let origin = [device_rect.x as u32, device_rect.y as u32];
        if direct
            && bounds.x >= 0.0
            && bounds.y >= 0.0
            && extent[0] > 0
            && extent[1] > 0
            && crate::pixel_aligned(bounds, context.scale)
            && crate::has_device_scale(canvas_transform(&self.canvas), context.scale)
            && let Some(target) = self.inline_target.clone().filter(|target| {
                origin[0].saturating_add(extent[0]) <= target.width()
                    && origin[1].saturating_add(extent[1]) <= target.height()
            })
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
        self.draw_native(bounds, context, &|ctx| record_gpu_pass(ctx, None, source))
    }
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
