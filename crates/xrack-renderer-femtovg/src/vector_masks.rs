use crate::{
    Renderer, color, pixel_aligned,
    retained_layers::{LayerStats, Raster},
    vector::{Geometry, GeometryKey, Source},
};
use femtovg::{
    Canvas, Color, DrawCommand, GlyphDrawCommands, ImageFlags, Paint, Path, Quad,
    renderer::WGPURenderer,
};
use std::collections::{HashMap, HashSet, hash_map::RandomState};
use xrack_layout::{LayoutNode, Rect};
use xrack_runtime::{Brush, Node, Value};

const MAX_MASKS: usize = 24;
const MAX_CANDIDATES: usize = 128;
const MAX_CAPTURES_PER_FRAME: usize = 4;
const MAX_EDGE_PX: f32 = 256.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    geometry: GeometryKey,
    scale: u32,
    alpha: [u32; 2],
}

#[derive(Clone, Copy)]
enum Plane {
    Fill,
    Stroke,
}

impl Plane {
    fn index(self) -> usize {
        match self {
            Self::Fill => 0,
            Self::Stroke => 1,
        }
    }
}

struct Entry {
    source: Source,
    rasters: [Option<Raster>; 2],
    used: bool,
}

#[derive(Clone, Copy)]
enum MaskPaint<'a> {
    Solid(Color),
    Gradient { brush: &'a Brush, alpha: f32 },
}

impl MaskPaint<'_> {
    fn alpha(self) -> f32 {
        match self {
            Self::Solid(tint) => tint.a,
            Self::Gradient { alpha, .. } => alpha,
        }
    }
}

/// Pixel-aligned vector paths can share their coverage across colors and
/// across instances when source alpha is uniform. Keep the fill and stroke
/// separate so they composite in the same order as direct draws.
#[derive(Default)]
pub(super) struct Masks {
    hash: RandomState,
    entries: HashMap<Key, Entry>,
    seen: HashSet<Key>,
    previous: HashSet<Key>,
    captures: usize,
    disabled: bool,
    pub(super) opacity_passthrough: bool,
    pub(super) stats: LayerStats,
}

impl Masks {
    pub(super) fn begin_frame(&mut self) {
        std::mem::swap(&mut self.seen, &mut self.previous);
        self.seen.clear();
        self.captures = 0;
        for entry in self.entries.values_mut() {
            entry.used = false;
        }
    }

    pub(super) fn finish_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.entries.retain(|_, entry| {
            if entry.used {
                return true;
            }
            for raster in entry.rasters.iter().flatten() {
                canvas.delete_image(raster.image);
            }
            false
        });
    }

    pub(super) fn clear(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        for entry in self.entries.drain().map(|(_, entry)| entry) {
            for raster in entry.rasters.into_iter().flatten() {
                canvas.delete_image(raster.image);
            }
        }
        self.seen.clear();
        self.previous.clear();
    }

    pub(super) fn reset_canvas(&mut self) {
        self.entries.clear();
        self.seen.clear();
        self.previous.clear();
    }

    pub(super) fn rasters(&self) -> impl Iterator<Item = Raster> + '_ {
        self.entries
            .values()
            .flat_map(|entry| entry.rasters)
            .flatten()
    }

    fn repeated(&mut self, key: Key) -> bool {
        let repeated = self.seen.contains(&key) || self.previous.contains(&key);
        if self.seen.len() < MAX_CANDIDATES {
            self.seen.insert(key);
        }
        repeated
    }
}

impl Renderer {
    /// Turn off coverage reuse to compare against direct vector drawing.
    #[doc(hidden)]
    pub fn set_vector_mask_cache_enabled(&mut self, enabled: bool) {
        if self.retained_layers.vectors.disabled == enabled {
            self.retained_layers.vectors.clear(&mut self.canvas);
            self.retained_layers.vectors.disabled = !enabled;
        }
    }

    #[doc(hidden)]
    pub fn vector_mask_stats(&self) -> LayerStats {
        self.retained_layers.vectors.stats
    }

    pub(super) fn paint_vector(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        scale: f32,
    ) -> Result<(), String> {
        let Some(geometry) = Geometry::for_node(node) else {
            return Ok(());
        };
        if !self.paint_vector_masks(&geometry, layout, scale)? {
            self.retained_layers.vectors.stats.direct += 1;
            self.vector_paths
                .draw(&mut self.canvas, &geometry, layout.bounds)?;
        }
        Ok(())
    }

    fn paint_vector_masks(
        &mut self,
        geometry: &Geometry<'_>,
        layout: &LayoutNode,
        scale: f32,
    ) -> Result<bool, String> {
        let bounds = layout.bounds;
        if self.vector_paths.disabled
            || self.retained_layers.vectors.disabled
            || self.retained_layers.vectors.opacity_passthrough
            || !geometry.warrants_mask()
            || !self.can_retain_layer(scale)
            || layout.has_ancestor_clip_chain()
            || !mask_bounds(bounds, scale)
            || !pixel_aligned(bounds.intersect(layout.clip), scale)
        {
            return Ok(false);
        }
        let node = geometry.node;
        let Some(fill) = mask_paint(node, "fill") else {
            return Ok(false);
        };
        let stroke = if geometry.stroke > 0.0 {
            let Some(stroke) = mask_paint(node, "stroke") else {
                return Ok(false);
            };
            stroke
        } else {
            None
        };
        if fill.is_none() && stroke.is_none() {
            return Ok(false);
        }

        let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        let cache = &mut self.retained_layers.vectors;
        let key = Key {
            geometry: geometry.key(&cache.hash, local),
            scale: scale.to_bits(),
            alpha: [fill, stroke].map(|paint| paint.map_or(0, |paint| paint.alpha().to_bits())),
        };
        if let Some(entry) = cache.entries.get_mut(&key) {
            if !entry.source.matches(geometry) {
                return Ok(false);
            }
            entry.used = true;
        } else if !cache.repeated(key) || cache.entries.len() >= MAX_MASKS {
            return Ok(false);
        }

        // Prepare both planes before touching the destination. A failed
        // allocation can then fall through to the complete direct draw.
        let mut draw = [None, None];
        for (plane, paint) in [(Plane::Fill, fill), (Plane::Stroke, stroke)] {
            let Some(paint) = paint else { continue };
            let Some(raster) = self.vector_mask(key, geometry, local, scale, plane)? else {
                return Ok(false);
            };
            draw[plane.index()] = Some((raster, paint));
        }
        let mut rect = Path::new();
        rect.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        for (raster, paint) in draw.into_iter().flatten() {
            match paint {
                MaskPaint::Solid(mut tint) => {
                    // The raster already includes the source alpha.
                    tint.a = 1.0;
                    let mut paint = Paint::image_tint(
                        raster.image,
                        bounds.x,
                        bounds.y,
                        bounds.width,
                        bounds.height,
                        0.0,
                        tint,
                    );
                    paint.set_anti_alias(false);
                    self.canvas.fill_path(&rect, &paint);
                }
                MaskPaint::Gradient { brush, .. } => {
                    paint_gradient_mask(&mut self.canvas, raster, bounds, brush);
                }
            }
        }
        Ok(true)
    }

    fn vector_mask(
        &mut self,
        key: Key,
        geometry: &Geometry<'_>,
        bounds: Rect,
        scale: f32,
        plane: Plane,
    ) -> Result<Option<Raster>, String> {
        let cache = &mut self.retained_layers.vectors;
        if let Some(raster) = cache
            .entries
            .get(&key)
            .and_then(|entry| entry.rasters[plane.index()])
        {
            cache.stats.reused += 1;
            return Ok(Some(raster));
        }
        if cache.captures >= MAX_CAPTURES_PER_FRAME || cache.rasters().count() >= MAX_MASKS {
            return Ok(None);
        }
        let Some(path) = geometry.build(bounds)? else {
            return Ok(None);
        };
        cache.captures += 1;
        let alpha = f32::from_bits(key.alpha[plane.index()]);
        let Some(raster) = self.capture_layer(bounds, scale, ImageFlags::NEAREST, |renderer| {
            let mut paint = Paint::color(Color::rgbaf(1.0, 1.0, 1.0, alpha));
            match plane {
                Plane::Fill => renderer.canvas.fill_path(&path, &paint),
                Plane::Stroke => {
                    paint.set_line_width(geometry.stroke);
                    renderer.canvas.stroke_path(&path, &paint);
                }
            }
            Ok(())
        })?
        else {
            return Ok(None);
        };
        let cache = &mut self.retained_layers.vectors;
        let entry = cache.entries.entry(key).or_insert_with(|| Entry {
            source: geometry.source(),
            rasters: [None, None],
            used: true,
        });
        entry.rasters[plane.index()] = Some(raster);
        cache.stats.rasterized += 1;
        Ok(Some(raster))
    }
}

fn mask_bounds(bounds: Rect, scale: f32) -> bool {
    pixel_aligned(bounds, scale)
        && [bounds.width, bounds.height].into_iter().all(|size| {
            let pixels = size * scale;
            pixels > 0.0 && pixels <= MAX_EDGE_PX
        })
}

fn mask_paint<'a>(node: &'a Node, property: &str) -> Option<Option<MaskPaint<'a>>> {
    match node.properties.get(property) {
        Some(Value::Color(tint) | Value::Brush(Brush::Solid(tint))) => {
            let tint = color(*tint);
            Some((tint.a > 0.0).then_some(MaskPaint::Solid(tint)))
        }
        Some(Value::Brush(brush @ (Brush::Linear { stops, .. } | Brush::Radial { stops }))) => {
            let alpha = color(stops.first()?.color).a;
            // An overlap's accumulated coverage varies with its source alpha.
            // A varying-alpha gradient cannot use the same coverage at each pixel.
            stops
                .iter()
                .all(|stop| color(stop.color).a == alpha)
                .then_some((alpha > 0.0).then_some(MaskPaint::Gradient { brush, alpha }))
        }
        _ => Some(None),
    }
}

fn paint_gradient_mask(
    canvas: &mut Canvas<WGPURenderer>,
    raster: Raster,
    bounds: Rect,
    brush: &Brush,
) {
    let paint = crate::paint_brush_with(brush, bounds, |value| {
        let mut tint = color(value);
        tint.a = 1.0;
        tint
    });
    // FemtoVG's alpha-mask shader multiplies the current paint by the red
    // channel of a separate texture. The retained white raster is premultiplied.
    let commands = GlyphDrawCommands {
        alpha_glyphs: vec![DrawCommand {
            image_id: raster.image,
            quads: vec![Quad {
                x0: bounds.x,
                y0: bounds.y,
                x1: bounds.x + bounds.width,
                y1: bounds.y + bounds.height,
                s0: 0.0,
                t0: 0.0,
                s1: 1.0,
                t1: 1.0,
            }],
        }],
        color_glyphs: Vec::new(),
    };
    canvas.draw_glyph_commands(commands, &paint);
}
