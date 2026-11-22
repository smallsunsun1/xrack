use crate::{
    CanvasTransform, Paint, PaintFrame, Palette, Point, Rect, Renderer, canvas_transform, color,
    image::{ImageQuad, QuadBounds},
    layer_compositor::{MAX_INSTANCES, large_enough},
    primitive::Primitive,
    retained_layers::CachedLayer,
};
use femtovg::{DrawCommand, FontId, GlyphDrawCommands, ImageId, PositionedGlyph, Quad};
use std::rc::Rc;
use xrack_layout::{LayoutNode, RoundedClip, Size, TextMeasurer};
use xrack_runtime::{Color as UiColor, Node};

/// The canvas state is stable while this list is collected and submitted. The
/// clip is the rectangular extent; any inherited path clip remains on the canvas.
pub(super) struct DisplayList<'a> {
    transform: CanvasTransform,
    clip: Rect,
    scale: f32,
    first: Option<Primitive<'a>>,
    rest: Vec<Primitive<'a>>,
}

struct PendingSurface<'a> {
    order: usize,
    node: &'a Node,
    layout: &'a LayoutNode,
    path: Vec<usize>,
    in_layout: bool,
    mask: Option<usize>,
}

#[derive(Clone, Copy, Default)]
struct WindowClip {
    rect: Option<Rect>,
    mask: Option<usize>,
}

#[derive(Default)]
struct WindowLayers<'a> {
    layers: Vec<(CachedLayer, Option<usize>)>,
    uncached: Vec<PendingSurface<'a>>,
    path: Vec<usize>,
    scopes: Vec<Rc<[RoundedClip]>>,
}

impl<'a> WindowLayers<'a> {
    fn collect_children(
        &mut self,
        renderer: &mut Renderer,
        node: &'a Node,
        layout: &'a LayoutNode,
        frame: PaintFrame<'_>,
        clip: WindowClip,
    ) -> Option<()> {
        let in_layout = xrack_layout::is_layout(node.kind);
        for (index, child, item) in layout.children_in_paint_order(node) {
            if !item.visible
                || child.opacity() == 0.0
                || crate::is_window_overlay(child.kind)
                || Node::PROGRESS_PARTS.contains(&child.kind)
                || Node::SCROLL_PARTS.contains(&child.kind)
            {
                continue;
            }
            self.path.push(index);
            let collected = self.collect(renderer, child, item, frame, clip, in_layout);
            self.path.pop();
            collected?;
        }
        Some(())
    }

    fn collect(
        &mut self,
        renderer: &mut Renderer,
        node: &'a Node,
        layout: &'a LayoutNode,
        frame: PaintFrame<'_>,
        clip: WindowClip,
        in_layout: bool,
    ) -> Option<()> {
        let order = self.layers.len() + self.uncached.len();
        if order == MAX_INSTANCES || layout.has_ancestor_clip_chain() != clip.mask.is_some() {
            return None;
        }
        if let Some(mut layer) =
            renderer.cached_window_layer(node, layout, &self.path, in_layout, frame)
        {
            let visible = clip
                .rect
                .map_or(layout.clip, |clip| clip.intersect(layout.clip));
            if !contains_rect(visible, layer.bounds) {
                if frame.scale < 1.0 || !crate::pixel_aligned(visible, frame.scale) {
                    return None;
                }
                layer.crop(visible);
            }
            let complete = !layer.surface_only || node.children.is_empty();
            self.layers.push((layer, clip.mask));
            if complete {
                return Some(());
            }
        } else if Renderer::can_cache_window_surface(node, layout, frame.scale) {
            if clip
                .rect
                .is_some_and(|clip| !contains_rect(clip, layout.bounds))
            {
                return None;
            }
            self.uncached.push(PendingSurface {
                order,
                node,
                layout,
                path: self.path.clone(),
                in_layout,
                mask: clip.mask,
            });
            if node.children.is_empty() {
                return Some(());
            }
        } else if !Renderer::is_unpainted_group(node, layout) {
            return None;
        }

        if layout.allows_overflow() {
            return None;
        }
        // A pixel-aligned scissor can be applied by trimming the child quads.
        let content = layout.bounds.intersect(layout.clip);
        let rect = clip.rect.map_or(content, |clip| content.intersect(clip));
        if !crate::pixel_aligned(rect, frame.scale) || frame.scale < 1.0 {
            return None;
        }
        let mask = if let Some(shape) = layout.child_clip_shape() {
            if !crate::pixel_aligned(shape.bounds, frame.scale) {
                return None;
            }
            let mut shapes = clip
                .mask
                .map_or_else(Vec::new, |scope| self.scopes[scope].to_vec());
            shapes.push(shape);
            self.scopes.push(shapes.into());
            Some(self.scopes.len() - 1)
        } else {
            clip.mask
        };
        self.collect_children(
            renderer,
            node,
            layout,
            frame,
            WindowClip {
                rect: Some(rect),
                mask,
            },
        )
    }
}

fn contains_rect(clip: Rect, bounds: Rect) -> bool {
    bounds.x >= clip.x
        && bounds.y >= clip.y
        && bounds.x + bounds.width <= clip.x + clip.width
        && bounds.y + bounds.height <= clip.y + clip.height
}

impl<'a> DisplayList<'a> {
    pub(super) fn cached_layers(
        renderer: &mut Renderer,
        root: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
    ) -> Option<Vec<CachedLayer>> {
        if !large_enough(renderer.opacity_layer_count())
            || layout.allows_overflow()
            || !root.properties.keys().all(|&name| {
                matches!(
                    name,
                    "background"
                        | "width"
                        | "height"
                        | "x"
                        | "y"
                        | "opacity"
                        | "enabled"
                        | "visible"
                        | "title"
                        | "default-font"
                        | "default-font-family"
                        | "default-font-size"
                        | "default-font-weight"
                        | "default-font-italic"
                        | "color-scheme"
                        | "accent-color"
                        | "min-width"
                        | "min-height"
                        | "max-width"
                        | "max-height"
                )
            })
        {
            return None;
        }
        let mut collected = WindowLayers::default();
        collected
            .layers
            .reserve(root.children.len().min(MAX_INSTANCES));
        collected.collect_children(renderer, root, layout, frame, WindowClip::default())?;
        let WindowLayers {
            mut layers,
            uncached,
            scopes,
            ..
        } = collected;
        if !large_enough(layers.len()) {
            return None;
        }
        // Observe every candidate on this frame, including when only some
        // of the leaves are ready to capture.
        let stable = uncached.iter().fold(true, |stable, item| {
            renderer.admit_window_surface(item.node, item.layout, &item.path, frame.scale) && stable
        });
        if !stable {
            return None;
        }
        for item in uncached {
            let layer = renderer.capture_window_surface(
                item.node,
                item.layout,
                &item.path,
                item.in_layout,
                frame,
            )?;
            layers.insert(item.order, (layer, item.mask));
        }
        let mut masks = vec![None; scopes.len()];
        layers
            .into_iter()
            .map(|(mut layer, scope)| {
                if let Some(scope) = scope {
                    let mask = &mut masks[scope];
                    if mask.is_none() {
                        *mask = Some(renderer.window_clip_mask(&scopes[scope], frame.scale)?);
                    }
                    layer.mask = mask.clone();
                }
                Some(layer)
            })
            .collect()
    }

    pub(super) fn new(renderer: &Renderer, clip: Rect, scale: f32) -> Self {
        Self {
            transform: canvas_transform(&renderer.canvas),
            clip,
            scale,
            first: None,
            rest: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, item: Primitive<'a>) {
        if self.first.is_none() {
            self.first = Some(item);
        } else {
            self.rest.push(item);
        }
    }

    /// A group that needs its own painter leaves the pending draw order intact.
    pub(super) fn try_append(&mut self, append: impl FnOnce(&mut Self) -> bool) -> bool {
        let had_first = self.first.is_some();
        let len = self.rest.len();
        if append(self) {
            return true;
        }
        self.rest.truncate(len);
        if !had_first {
            self.first = None;
        }
        false
    }

    pub(super) fn submit(&mut self, renderer: &mut Renderer) -> Result<(), String> {
        let Some(first) = self.first.take() else {
            return Ok(());
        };
        debug_assert_eq!(self.transform, canvas_transform(&renderer.canvas));
        if self.rest.is_empty() {
            return renderer.draw_primitive(first);
        }
        let can_batch =
            renderer.display_batching && !renderer.retained_layers.vectors.opacity_passthrough;
        let image_quads = can_batch && crate::image::supports_quad_transform(self.transform);
        let mut previous = batch_order(&first);
        let mixed = image_quads
            && self.rest.len() >= 3
            && self.rest.iter().any(|item| {
                let order = batch_order(item);
                let interleaved = order < previous;
                previous = order;
                interleaved
            });
        let first = if mixed {
            self.rest.insert(0, first);
            reorder_disjoint(&mut self.rest, self.transform, self.scale);
            None
        } else {
            Some(first)
        };
        let mut items = first.into_iter().chain(self.rest.drain(..)).peekable();
        let mut rectangles = Vec::new();

        while let Some(item) = items.next() {
            match item {
                Primitive::Rectangle(rectangle) if can_batch => {
                    let rounded = rectangle.rounded();
                    rectangles.push(rectangle);
                    while let Some(Primitive::Rectangle(next)) = items.peek() {
                        if next.rounded() != rounded {
                            break;
                        }
                        rectangles.push(*next);
                        items.next();
                    }
                    renderer.draw_rectangle_batch(&mut rectangles, self.scale, self.clip);
                }
                item @ (Primitive::Image { quad: Some(_), .. } | Primitive::Icon { .. })
                    if image_quads =>
                {
                    let order = batch_order(&item);
                    if items.peek().is_none_or(|next| batch_order(next) != order) {
                        renderer.draw_primitive(item)?;
                        continue;
                    }
                    let Some(first) = renderer.prepare_image_quad(&item, self.clip)? else {
                        renderer.draw_primitive(item)?;
                        continue;
                    };
                    let mut quads = Vec::new();
                    while let Some(next) = items.peek() {
                        if batch_order(next) != order {
                            break;
                        }
                        let Some(quad) = renderer.prepare_image_quad(next, self.clip)? else {
                            break;
                        };
                        if first.0 != quad.0 {
                            break;
                        }
                        quads.push(quad.1);
                        items.next();
                    }
                    renderer.images.flush_batch_uploads(&mut renderer.canvas)?;
                    if quads.is_empty() {
                        renderer.draw_primitive(item)?;
                    } else {
                        quads.insert(0, first.1);
                        renderer.draw_image_batch(first.0, quads);
                    }
                }
                item @ Primitive::Label { .. }
                    if image_quads && matches!(items.peek(), Some(Primitive::Label { .. })) =>
                {
                    renderer.draw_label_batch(item, &mut items, self.transform)?;
                }
                Primitive::Text {
                    origin,
                    text,
                    paint,
                    clip: None,
                } if can_batch => {
                    if !matches!(items.peek(), Some(Primitive::Text { paint: next, clip: None, .. }) if std::ptr::eq(paint, *next))
                    {
                        renderer.draw_primitive(Primitive::Text {
                            origin,
                            text,
                            paint,
                            clip: None,
                        })?;
                        continue;
                    }
                    let mut lines = vec![(origin, text)];
                    while let Some(Primitive::Text {
                        origin,
                        text,
                        paint: next,
                        clip: None,
                    }) = items.peek()
                    {
                        if !std::ptr::eq(paint, *next) {
                            break;
                        }
                        lines.push((*origin, *text));
                        items.next();
                    }
                    if !renderer.draw_glyph_batch(&lines, paint)? {
                        for (origin, text) in lines {
                            renderer.draw_primitive(Primitive::Text {
                                origin,
                                text,
                                paint,
                                clip: None,
                            })?;
                        }
                    }
                }
                item => renderer.draw_primitive(item)?,
            }
        }
        Ok(())
    }
}

fn batch_order(item: &Primitive<'_>) -> usize {
    match item {
        Primitive::Rectangle(rect) => usize::from(rect.rounded()),
        Primitive::Image {
            quad: Some(QuadBounds::Antialiased(..)),
            ..
        } => 3,
        Primitive::Image { quad: Some(_), .. } | Primitive::Icon { .. } => 2,
        Primitive::Image { .. } => 4,
        _ => 5,
    }
}

// An axis-aligned fill or scissor can reach half of FemtoVG's 1/DPI
// antialiasing fringe outside its bounds. Leave room for mask phase rounding.
fn edge_reach(dpi: f32) -> f32 {
    0.5 / dpi + 1.0 / 64.0
}

fn reorder_bounds(item: &Primitive<'_>, transform: CanvasTransform, fringe: f32) -> Option<Rect> {
    let (bounds, antialiased) = match item {
        Primitive::Rectangle(rect) => (rect.square_bounds()?, rect.uses_mask()),
        Primitive::Image {
            bounds,
            clip,
            colorize: crate::image_colorize::Colorize::Original,
            ..
        } => (clip.map_or(*bounds, |clip| bounds.intersect(clip)), true),
        Primitive::Icon {
            bounds,
            tint: None,
            enabled: true,
            ..
        } => (*bounds, true),
        Primitive::Label { clip, .. }
        | Primitive::Text {
            clip: Some(clip), ..
        } => (*clip, true),
        _ => return None,
    };
    let bounds = transform.map_rect(bounds);
    (bounds.width > 0.0
        && bounds.height > 0.0
        && [bounds.x, bounds.y, bounds.width, bounds.height]
            .into_iter()
            .all(f32::is_finite))
    .then(|| {
        if !antialiased {
            return bounds;
        }
        let bounds = bounds.inset(-fringe);
        // The render target has one sample at the center of each pixel.
        // Intersecting fringes only prevent reordering if they reach the
        // same sample.
        let left = (bounds.x - 0.5).ceil();
        let top = (bounds.y - 0.5).ceil();
        let right = (bounds.x + bounds.width - 0.5).floor() + 1.0;
        let bottom = (bounds.y + bounds.height - 0.5).floor() + 1.0;
        Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    })
}

fn add_region(regions: &mut Vec<Rect>, bounds: Rect) {
    const MAX_REGIONS: usize = 32;
    let at_limit = regions.len() == MAX_REGIONS;
    if let Some(last) = regions.last_mut()
        && (at_limit || bounds.y < last.y + last.height && last.y < bounds.y + bounds.height)
    {
        *last = crate::shadow::union(*last, bounds);
    } else {
        regions.push(bounds);
    }
}

fn overlaps(first: Rect, second: Rect) -> bool {
    let overlap = first.intersect(second);
    overlap.width > 0.0 && overlap.height > 0.0
}

fn reorder_disjoint(items: &mut [Primitive<'_>], transform: CanvasTransform, dpi: f32) {
    let mut start = 0;
    let mut envelopes: [Vec<Rect>; 6] = Default::default();
    let mut highest_order = 0;
    let mut mixed = false;
    let fringe = edge_reach(dpi);
    for index in 0..items.len() {
        let order = batch_order(&items[index]);
        let bounds = reorder_bounds(&items[index], transform, fringe);
        // Moving a draw to an earlier batch must cross only disjoint draws.
        // Unknown paint extents and intersections end the sortable segment.
        if bounds.is_none_or(|bounds| {
            envelopes[order + 1..]
                .iter()
                .flatten()
                .any(|later| overlaps(bounds, *later))
        }) {
            if mixed {
                items[start..index].sort_by_key(batch_order);
            }
            start = index;
            envelopes.iter_mut().for_each(Vec::clear);
            highest_order = 0;
            mixed = false;
        }
        if let Some(bounds) = bounds {
            mixed |= order < highest_order;
            highest_order = highest_order.max(order);
            // Keep disjoint rows separate so their combined bounding box does
            // not cover the empty space in the current row.
            add_region(&mut envelopes[order], bounds);
        } else {
            start = index + 1;
        }
    }
    if mixed {
        items[start..].sort_by_key(batch_order);
    }
}

#[derive(Default)]
struct LabelGlyphBatch {
    font: Option<FontId>,
    positions: Vec<PositionedGlyph>,
    clips: Vec<Rect>,
}

pub(super) fn label_tint(node: &Node, layout: &LayoutNode, theme: Palette) -> UiColor {
    if layout.enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    }
}

impl Renderer {
    fn draw_label_batch<'a>(
        &mut self,
        first: Primitive<'a>,
        rest: &mut std::iter::Peekable<impl Iterator<Item = Primitive<'a>>>,
        transform: CanvasTransform,
    ) -> Result<(), String> {
        const MAX_DEFERRED_LABELS: usize = 256;
        let Primitive::Label {
            node,
            layout,
            tint,
            scale: dpi,
            ..
        } = &first
        else {
            return self.draw_primitive(first);
        };
        let Some([scale, sy]) = transform.axis_scales() else {
            return self.draw_primitive(first);
        };
        // Match FemtoVG's 1/16-step glyph atlas and account for the residual
        // transform when checking the resulting quads in device pixels.
        let atlas_scale = (scale * 16.0 + 0.5).trunc() / 16.0;
        let size = self.text.font_size(node);
        if cfg!(any(target_os = "android", target_os = "ios"))
            || scale != sy
            || scale < 1.0 / 16.0
            || *dpi < 1.0
            || size * atlas_scale > 90.0
        {
            return self.draw_primitive(first);
        }
        let style = self.text.font_attributes(Some(node));
        let bounds = layout.bounds;
        let first_layout = self.text.prepared_label(
            node,
            Size::new(bounds.width, bounds.height),
            layout.layout_direction_override(),
        );
        let paint = first_layout.paint(color(*tint));
        let mut batch = LabelGlyphBatch::default();
        if !self.append_label_glyphs(&first, &paint, transform, atlas_scale, &mut batch) {
            return self.draw_primitive(first);
        }
        let mut count = 1;
        let mut deferred = Vec::new();
        let mut deferred_clips = Vec::new();
        let fringe = edge_reach(*dpi);
        while let Some(
            next @ Primitive::Label {
                node,
                tint: next_tint,
                ..
            },
        ) = rest.peek()
        {
            let Some(bounds) = reorder_bounds(next, transform, fringe) else {
                break;
            };
            if *next_tint == *tint
                && self.text.font_attributes(Some(node)) == style
                && self.text.font_size(node) == size
                && deferred_clips.iter().all(|clip| !overlaps(*clip, bounds))
                && self.append_label_glyphs(next, &paint, transform, atlas_scale, &mut batch)
            {
                rest.next();
                count += 1;
            } else {
                // Keep a clipped label in paint order relative to every later
                // label it can overlap, even if disjoint glyphs can pass it.
                if deferred.len() == MAX_DEFERRED_LABELS {
                    break;
                }
                add_region(&mut deferred_clips, bounds);
                deferred.extend(rest.next());
            }
        }
        if count == 1 {
            self.draw_primitive(first)?;
        } else if let Some(font) = batch.font {
            self.canvas
                .fill_glyph_run(font, &[], batch.positions, &paint)
                .map_err(|error| format!("text drawing failed: {error}"))?;
            xrack_runtime::update_profile::glyph_batch();
            self.display_batch_stats.glyph_batches += 1;
            self.display_batch_stats.text_runs += count;
        }
        for label in deferred {
            self.draw_primitive(label)?;
        }
        Ok(())
    }

    fn append_label_glyphs(
        &self,
        item: &Primitive<'_>,
        paint: &Paint,
        transform: CanvasTransform,
        atlas_scale: f32,
        batch: &mut LabelGlyphBatch,
    ) -> bool {
        let Primitive::Label {
            node,
            layout,
            clip,
            scale: dpi,
            ..
        } = item
        else {
            return false;
        };
        if clip.width <= 0.0 || clip.height <= 0.0 {
            return false;
        }
        let aligned = crate::image::quad_edges_are_pixel_aligned(*clip, transform);
        let clip = transform.map_rect(*clip);
        // At fractional edges the scissor can attenuate samples. Require
        // every sample covered by the padded quads to lie in its interior.
        let interior = if aligned {
            clip
        } else {
            clip.inset(edge_reach(*dpi))
        };
        if batch.clips.iter().any(|previous| overlaps(*previous, clip)) {
            return false;
        }
        let bounds = layout.bounds;
        let label = self.text.prepared_label(
            node,
            Size::new(bounds.width, bounds.height),
            layout.layout_direction_override(),
        );
        // Keep multiline labels on the original path. Glyphs from different
        // lines can overlap, and an atlas batch groups drawing by texture.
        let [line] = label.layout.lines.as_slice() else {
            return false;
        };
        let [tx, ty] = transform.translation();
        let residual = transform.coefficients()[0] / atlas_scale;
        // FemtoVG caches rasters in tenths of a pixel. A previous paint in
        // this size bin may have produced slightly different rounded bounds.
        let size_bin = (paint.font_size() * atlas_scale * 10.0).trunc();
        let small_size = (size_bin / 10.0 - 0.0001).max(0.0);
        let large_size = (size_bin + 1.0) / 10.0 + 0.0001;
        let sample_inset = if !aligned && residual == 1.0 && tx.fract() == 0.0 && ty.fract() == 0.0
        {
            // Integer atlas quads only cover pixel centers half a pixel in.
            0.5
        } else {
            0.0
        };
        let mut font = batch.font;
        let start = batch.positions.len();
        let fits = (|| {
            let Ok(metrics) = self.canvas.measure_text(
                bounds.x + line.origin.x,
                bounds.y + line.origin.y,
                &line.text,
                paint,
            ) else {
                return false;
            };
            for glyph in metrics
                .glyphs
                .into_iter()
                .filter(|glyph| !glyph.c.is_control())
            {
                if font.is_some_and(|id| id != glyph.font_id) {
                    return false;
                }
                let Some(outline) = self.text.outline_glyph(glyph.font_id, glyph.glyph_id) else {
                    return false;
                };
                let Some(ink) = outline.bounds else {
                    continue;
                };
                let units_per_em = f32::from(outline.units_per_em);
                let small = small_size / units_per_em;
                let large = large_size / units_per_em;
                // Match the atlas's rounded bearings, integer glyph origins,
                // upward-rounded extents and one texel of padding on each side.
                let bearing_x = f32::from(ink.x_min);
                let bearing_y = -f32::from(ink.y_max);
                let ox = (glyph.x * atlas_scale).trunc();
                let oy = (glyph.y * atlas_scale).round();
                let x = ox + (bearing_x * small).min(bearing_x * large).round() - 1.0;
                let y = oy + (bearing_y * small).min(bearing_y * large).round() - 1.0;
                let right = ox
                    + (bearing_x * small).max(bearing_x * large).round()
                    + (f32::from(ink.width()) * large).ceil()
                    + 1.0;
                let bottom = oy
                    + (bearing_y * small).max(bearing_y * large).round()
                    + (f32::from(ink.height()) * large).ceil()
                    + 1.0;
                if !(x * residual + tx + sample_inset >= interior.x
                    && y * residual + ty + sample_inset >= interior.y
                    && right * residual + tx - sample_inset <= interior.x + interior.width
                    && bottom * residual + ty - sample_inset <= interior.y + interior.height)
                {
                    return false;
                }
                font = Some(glyph.font_id);
                batch.positions.push(PositionedGlyph {
                    x: glyph.x,
                    y: glyph.y,
                    glyph_id: glyph.glyph_id,
                });
            }
            true
        })();
        if fits {
            batch.font = font;
            add_region(&mut batch.clips, clip);
        } else {
            batch.positions.truncate(start);
        }
        fits
    }

    pub(super) fn display_label<'a>(
        &self,
        node: &'a Node,
        layout: &'a LayoutNode,
        theme: Palette,
        scale: f32,
    ) -> Option<Primitive<'a>> {
        if !xrack_layout::uses_label_layout(node) || !Self::is_plain_label(node, layout) {
            return None;
        }
        Some(Primitive::Label {
            node,
            layout,
            clip: layout.clip.intersect(layout.bounds),
            tint: label_tint(node, layout, theme),
            scale,
        })
    }

    pub(super) fn is_plain_label(node: &Node, layout: &LayoutNode) -> bool {
        matches!(node.kind, "Label" | "Text")
            && node.opacity() == 1.0
            && node.children.is_empty()
            && node.list.is_none()
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
            && !layout.allows_overflow()
            && node.properties.keys().all(|name| {
                matches!(
                    *name,
                    "x" | "y"
                        | "width"
                        | "height"
                        | "z"
                        | "visible"
                        | "enabled"
                        | "opacity"
                        | "text"
                        | "color"
                        | "font-family"
                        | "font-size"
                        | "font-weight"
                        | "font-italic"
                        | "wrap"
                        | "overflow"
                        | "max-lines"
                        | "horizontal-alignment"
                        | "vertical-alignment"
                        | "layout-direction"
                        | "min-width"
                        | "min-height"
                        | "max-width"
                        | "max-height"
                        | "preferred-width"
                        | "preferred-height"
                        | "horizontal-stretch"
                        | "vertical-stretch"
                )
            })
    }

    pub(super) fn paint_label(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        tint: UiColor,
        scale: f32,
    ) -> Result<(), String> {
        let bounds = layout.bounds;
        let label = self.text.prepared_label(
            node,
            Size::new(bounds.width, bounds.height),
            layout.layout_direction_override(),
        );
        let paint = label.paint(color(tint));
        let mut display = DisplayList::new(self, layout.clip.intersect(bounds), scale);
        for line in &label.layout.lines {
            if !line.text.is_empty() {
                display.push(Primitive::Text {
                    origin: Point {
                        x: bounds.x + line.origin.x,
                        y: bounds.y + line.origin.y,
                    },
                    text: &line.text,
                    paint: &paint,
                    clip: None,
                });
            }
        }
        display.submit(self)
    }

    pub(super) fn collect_group_primitives<'a>(
        &self,
        node: &'a Node,
        layout: &'a LayoutNode,
        display: &mut DisplayList<'a>,
        square_batch: bool,
        theme: Palette,
        depth: usize,
    ) -> bool {
        if depth == 0
            || layout.child_clip_shape().is_some()
            || !Self::is_unpainted_group(node, layout)
        {
            return false;
        }

        // The child layout already includes this group's rectangular clip.
        // Preserve child paint order and emit nothing unless the group fits.
        let in_layout = xrack_layout::is_layout(node.kind);
        layout
            .children_in_paint_order(node)
            .all(|(_, child, item)| {
                if !item.visible {
                    return true;
                }
                if square_batch
                    && let Some(rect) =
                        crate::rectangle::Solid::from_node(child, item, display.scale)
                    && rect.collect_square(display.clip, display.scale)
                {
                    if rect.visible() {
                        display.push(Primitive::Rectangle(rect));
                    }
                    return true;
                }
                if let Some(image) =
                    self.display_image(child, item, in_layout, display.scale, display.transform)
                {
                    display.push(image);
                    return true;
                }
                if let Some(label) = self.display_label(child, item, theme, display.scale) {
                    display.push(label);
                    return true;
                }
                self.collect_group_primitives(child, item, display, square_batch, theme, depth - 1)
            })
    }

    fn is_unpainted_group(node: &Node, layout: &LayoutNode) -> bool {
        if !matches!(
            node.kind,
            "Rectangle" | "HorizontalLayout" | "VerticalLayout" | "GridLayout"
        ) || node.opacity() != 1.0
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || layout.scroll.is_some()
            || layout.table.is_some()
            || node.list.is_some()
            || !node.properties.keys().all(|name| {
                matches!(
                    *name,
                    "x" | "y"
                        | "width"
                        | "height"
                        | "z"
                        | "opacity"
                        | "visible"
                        | "enabled"
                        | "clip"
                        | "spacing"
                        | "border-radius"
                        | "border-top-left-radius"
                        | "border-top-right-radius"
                        | "border-bottom-right-radius"
                        | "border-bottom-left-radius"
                        | "padding"
                        | "padding-left"
                        | "padding-top"
                        | "padding-right"
                        | "padding-bottom"
                        | "min-width"
                        | "min-height"
                        | "max-width"
                        | "max-height"
                        | "preferred-width"
                        | "preferred-height"
                        | "horizontal-stretch"
                        | "vertical-stretch"
                        | "horizontal-alignment"
                        | "vertical-alignment"
                        | "layout-direction"
                )
            })
        {
            return false;
        }
        true
    }

    fn prepare_image_quad(
        &mut self,
        item: &Primitive<'_>,
        clip: Rect,
    ) -> Result<Option<(ImageId, Quad)>, String> {
        use xrack_layout::ImagePresentation;
        use xrack_runtime::ImageFit;

        let (bounds, coverage, asset, presentation) = match item {
            Primitive::Image {
                bounds,
                asset,
                presentation,
                colorize: crate::image_colorize::Colorize::Original,
                quad: Some(rect),
                ..
            } => (*bounds, *rect, asset, *presentation),
            Primitive::Icon {
                bounds,
                asset,
                enabled: true,
                tint: None,
                ..
            } => (
                *bounds,
                QuadBounds::Aligned(None),
                *asset,
                ImagePresentation::whole(asset.size(), ImageFit::Contain),
            ),
            _ => return Ok(None),
        };
        self.images.batch_quad(
            &self.device,
            &mut self.canvas,
            ImageQuad {
                asset,
                bounds,
                clip,
                coverage,
                presentation,
            },
        )
    }

    fn draw_image_batch(&mut self, image_id: ImageId, quads: Vec<Quad>) {
        xrack_runtime::update_profile::quad_batch(quads.len());
        self.display_batch_stats.image_batches += 1;
        self.display_batch_stats.images += quads.len() as u64;
        self.canvas.draw_glyph_commands(
            GlyphDrawCommands {
                color_glyphs: vec![DrawCommand { image_id, quads }],
                ..Default::default()
            },
            &Paint::color(femtovg::Color::white()),
        );
    }

    fn draw_glyph_batch(&mut self, lines: &[(Point, &str)], paint: &Paint) -> Result<bool, String> {
        // Native mobile font fallbacks and decorators are emitted by the full
        // text painter. Only combine lines whose glyphs can use the same font.
        let Some([scale, scale_y]) = canvas_transform(&self.canvas).axis_scales() else {
            return Ok(false);
        };
        if lines.len() < 2
            || cfg!(any(target_os = "android", target_os = "ios"))
            || !paint.text_decoration().is_none()
            || !scale.is_finite()
            || scale <= 0.0
            || scale_y != scale
            || paint.font_size() * scale > 90.0
            || lines.iter().any(|(_, text)| !text.is_ascii())
        {
            return Ok(false);
        }
        let mut previous_bottom = f32::NEG_INFINITY;
        let mut font = None;
        let mut positions = Vec::new();
        for (origin, text) in lines {
            let metrics = self
                .canvas
                .measure_text(origin.x, origin.y, text, paint)
                .map_err(|error| format!("text shaping failed: {error}"))?;
            // Preserve overlap order even in fonts with colored ASCII glyphs.
            // The glyph atlas may emit color glyphs after alpha glyphs.
            if metrics.y < previous_bottom {
                return Ok(false);
            }
            previous_bottom = metrics.y + metrics.height() + 1.0 / scale;
            for glyph in metrics
                .glyphs
                .into_iter()
                .filter(|glyph| !glyph.c.is_control())
            {
                if font.is_some_and(|font| font != glyph.font_id) {
                    return Ok(false);
                }
                font = Some(glyph.font_id);
                positions.push(PositionedGlyph {
                    x: glyph.x,
                    y: glyph.y,
                    glyph_id: glyph.glyph_id,
                });
            }
        }
        let Some(font) = font else {
            return Ok(false);
        };
        if self
            .canvas
            .font_variation_axes(font)
            .map_or(true, |axes| !axes.is_empty())
        {
            return Ok(false);
        }
        self.canvas
            .fill_glyph_run(font, &[], positions, paint)
            .map_err(|error| format!("text drawing failed: {error}"))?;
        xrack_runtime::update_profile::glyph_batch();
        self.display_batch_stats.glyph_batches += 1;
        self.display_batch_stats.text_runs += lines.len() as u64;
        Ok(true)
    }
}

/// Batches emitted by the renderer's display lists, accumulated since creation.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayBatchStats {
    pub image_batches: u64,
    pub images: u64,
    pub glyph_batches: u64,
    pub text_runs: u64,
}
