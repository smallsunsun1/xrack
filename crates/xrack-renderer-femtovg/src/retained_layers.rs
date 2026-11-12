use super::{
    Canvas, Color, LayoutNode, Node, Paint, PaintFrame, Palette, Path, Rect, Renderer,
    WGPURenderer, image,
};
use femtovg::{ImageFlags, ImageId, ImageInfo, PixelFormat, Transform2D};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};

const MAX_LAYERS: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
pub struct LayerStats {
    pub rasterized: u64,
    pub reused: u64,
    pub direct: u64,
}

struct Layer {
    page: Weak<(Node, LayoutNode)>,
    raster: Raster,
    bounds: Rect,
    scale: f32,
    palette: Palette,
}

struct OpacityLayer {
    node: Box<Node>,
    layout: Box<LayoutNode>,
    raster: Raster,
    scale: f32,
    palette: Palette,
    in_layout: bool,
    used: bool,
}

#[derive(Clone, Copy)]
struct Raster {
    image: ImageId,
    bounds: Rect,
    bytes: u64,
}

#[derive(Default)]
struct FontDefaults {
    family: String,
    weight: u16,
    italic: bool,
    size: Option<f32>,
}

pub(super) struct RetainedLayers {
    layers: Vec<Layer>,
    opacity: BTreeMap<Vec<usize>, OpacityLayer>,
    opacity_stats: LayerStats,
    capturing: bool,
    capture: Option<(Canvas<WGPURenderer>, image::Images)>,
    budget: u64,
    stats: LayerStats,
    font: FontDefaults,
}

impl Default for RetainedLayers {
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            opacity: BTreeMap::new(),
            opacity_stats: LayerStats::default(),
            capturing: false,
            capture: None,
            budget: 32 * 1024 * 1024,
            stats: LayerStats::default(),
            font: FontDefaults::default(),
        }
    }
}

impl RetainedLayers {
    pub(super) fn reset_canvas(&mut self) {
        self.layers.clear();
        self.opacity.clear();
        self.capture = None;
    }

    pub(super) fn begin_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.layers.retain(|layer| {
            if layer.page.strong_count() != 0 {
                return true;
            }
            canvas.delete_image(layer.raster.image);
            false
        });
        for layer in self.opacity.values_mut() {
            layer.used = false;
        }
    }

    pub(super) fn finish_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.opacity.retain(|_, layer| {
            if layer.used {
                return true;
            }
            canvas.delete_image(layer.raster.image);
            false
        });
    }

    fn clear_images(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        for layer in self.layers.drain(..) {
            canvas.delete_image(layer.raster.image);
        }
        for layer in std::mem::take(&mut self.opacity).into_values() {
            canvas.delete_image(layer.raster.image);
        }
    }

    pub(super) fn sync_font_defaults(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        defaults: (&str, u16, bool, Option<f32>),
    ) {
        let (family, weight, italic, size) = defaults;
        let last = &mut self.font;
        if last.family == family
            && last.weight == weight
            && last.italic == italic
            && last.size == size
        {
            return;
        }
        last.family.clear();
        last.family.push_str(family);
        last.weight = weight;
        last.italic = italic;
        last.size = size;
        self.clear_images(canvas);
    }
}

impl Renderer {
    #[doc(hidden)]
    pub fn stack_layer_stats(&self) -> LayerStats {
        self.retained_layers.stats
    }

    #[doc(hidden)]
    pub fn set_layer_budget(&mut self, bytes: u64) {
        self.retained_layers.budget = bytes;
        self.retained_layers.clear_images(&mut self.canvas);
        if bytes == 0 {
            self.retained_layers.capture = None;
        }
    }

    #[doc(hidden)]
    pub fn opacity_layer_stats(&self) -> LayerStats {
        self.retained_layers.opacity_stats
    }

    fn can_retain_layer(&self, scale: f32) -> bool {
        !self.retained_layers.capturing
            && self.retained_layers.budget > 0
            && self.canvas.transform().0 == Transform2D::scaling(scale, scale).0
    }

    pub(super) fn paint_stack_layer(
        &mut self,
        page: &Rc<(Node, LayoutNode)>,
        bounds: Rect,
        offset: f32,
        frame: PaintFrame<'_>,
        cacheable: bool,
    ) -> Result<(), String> {
        let layer = if cacheable && self.can_retain_layer(frame.scale) {
            self.stack_layer_image(page, bounds, frame)?
        } else {
            None
        };
        self.canvas.translate(offset, 0.0);
        if let Some(raster) = layer {
            self.composite_layer(raster, 1.0);
            Ok(())
        } else {
            self.retained_layers.stats.direct += 1;
            self.paint(&page.0, &page.1, &mut Vec::new(), frame, false)
        }
    }

    pub(super) fn paint_retained_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<bool, String> {
        if !self.can_retain_layer(frame.scale) || node.children.is_empty() {
            return Ok(false);
        }
        let clip = layout.clip.intersect(layout.bounds);
        let raster = if let Some(layer) = self.retained_layers.opacity.get_mut(path)
            && layer.scale == frame.scale
            && layer.palette == frame.theme
            && layer.in_layout == in_layout
            && *layer.layout == *layout
            && layer.node.same_paint_except_opacity(node)
        {
            layer.used = true;
            self.retained_layers.opacity_stats.reused += 1;
            layer.raster
        } else {
            if !passive_content(node) {
                return Ok(false);
            }
            if let Some(layer) = self.retained_layers.opacity.remove(path) {
                self.canvas.delete_image(layer.raster.image);
            }
            let Some(raster) = self.capture_layer(clip, frame.scale, |renderer| {
                renderer.paint_group(clip, 1.0, |renderer| {
                    renderer.paint_element(node, layout, path, frame, in_layout)
                })
            })?
            else {
                self.retained_layers.opacity_stats.direct += 1;
                return Ok(false);
            };
            self.retained_layers.opacity.insert(
                path.clone(),
                OpacityLayer {
                    node: Box::new(node.clone()),
                    layout: Box::new(layout.clone()),
                    raster,
                    scale: frame.scale,
                    palette: frame.theme,
                    in_layout,
                    used: true,
                },
            );
            self.retained_layers.opacity_stats.rasterized += 1;
            raster
        };
        self.paint_group(clip, 1.0, |renderer| {
            renderer.composite_layer(raster, node.opacity());
            Ok(())
        })?;
        Ok(true)
    }

    fn composite_layer(&mut self, raster: Raster, opacity: f32) {
        let bounds = raster.bounds;
        let mut shape = Path::new();
        shape.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        self.canvas.fill_path(
            &shape,
            &Paint::image(
                raster.image,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                0.0,
                opacity,
            ),
        );
    }

    fn stack_layer_image(
        &mut self,
        page: &Rc<(Node, LayoutNode)>,
        bounds: Rect,
        frame: PaintFrame<'_>,
    ) -> Result<Option<Raster>, String> {
        let page_ref = Rc::downgrade(page);
        if let Some(layer) = self.retained_layers.layers.iter().find(|layer| {
            layer.page.ptr_eq(&page_ref)
                && layer.bounds == bounds
                && layer.scale == frame.scale
                && layer.palette == frame.theme
        }) {
            self.retained_layers.stats.reused += 1;
            return Ok(Some(layer.raster));
        }
        let Some(raster) = self.capture_layer(bounds, frame.scale, |renderer| {
            renderer.paint(&page.0, &page.1, &mut Vec::new(), frame, false)
        })?
        else {
            return Ok(None);
        };
        self.retained_layers.layers.push(Layer {
            page: page_ref,
            raster,
            bounds,
            scale: frame.scale,
            palette: frame.theme,
        });
        self.retained_layers.stats.rasterized += 1;
        Ok(Some(raster))
    }

    fn capture_layer(
        &mut self,
        bounds: Rect,
        scale: f32,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<Raster>, String> {
        if bounds.width <= 0.0
            || bounds.height <= 0.0
            || self.retained_layers.layers.len() + self.retained_layers.opacity.len() >= MAX_LAYERS
        {
            return Ok(None);
        }
        let left = (bounds.x * scale).floor();
        let top = (bounds.y * scale).floor();
        let width = ((bounds.x + bounds.width) * scale).ceil() - left;
        let height = ((bounds.y + bounds.height) * scale).ceil() - top;
        let limit = self.device.limits().max_texture_dimension_2d as f32;
        if !(width > 0.0 && width <= limit && height > 0.0 && height <= limit) {
            return Ok(None);
        }
        let bounds = Rect::new(left / scale, top / scale, width / scale, height / scale);
        let width = width as u32;
        let height = height as u32;
        let bytes = u64::from(width) * u64::from(height) * 4;
        let used: u64 = self
            .retained_layers
            .layers
            .iter()
            .map(|layer| layer.raster.bytes)
            .chain(
                self.retained_layers
                    .opacity
                    .values()
                    .map(|layer| layer.raster.bytes),
            )
            .sum();
        if bytes > self.retained_layers.budget.saturating_sub(used) {
            return Ok(None);
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack retained layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        if self.retained_layers.capture.is_none() {
            let Ok(canvas) = Canvas::new_with_text_context(
                WGPURenderer::new(self.device.clone(), self.queue.clone()),
                self.text.context().clone(),
            ) else {
                return Ok(None);
            };
            self.retained_layers.capture = Some((canvas, Default::default()));
        }
        let mut capture = self.retained_layers.capture.take().unwrap();
        self.retained_layers.capturing = true;
        std::mem::swap(&mut self.canvas, &mut capture.0);
        std::mem::swap(&mut self.images, &mut capture.1);
        self.canvas.set_size(width, height, scale);
        self.canvas.reset();
        self.canvas
            .clear_rect(0, 0, width, height, Color::rgba(0, 0, 0, 0));
        self.canvas.scale(scale, scale);
        self.canvas.translate(-bounds.x, -bounds.y);
        self.images.begin_frame();
        let result = draw(self);
        self.queue.submit(self.canvas.flush_to_output(&texture));
        self.images.end_frame(&mut self.canvas);
        std::mem::swap(&mut self.canvas, &mut capture.0);
        std::mem::swap(&mut self.images, &mut capture.1);
        self.retained_layers.capture = Some(capture);
        self.retained_layers.capturing = false;
        result?;
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED,
            width as usize,
            height as usize,
            PixelFormat::Rgba8,
        );
        let Ok(image) = self.canvas.create_image_from_native_texture(texture, info) else {
            return Ok(None);
        };
        Ok(Some(Raster {
            image,
            bounds,
            bytes,
        }))
    }
}

fn passive_content(node: &Node) -> bool {
    matches!(
        node.kind,
        "Rectangle"
            | "Text"
            | "Image"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "Row"
            | "Column"
            | "GridLayout"
    ) && node.children.iter().all(passive_content)
}
