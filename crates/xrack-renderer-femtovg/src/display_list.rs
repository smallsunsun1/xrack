use crate::{
    CanvasTransform, Paint, PaintFrame, Palette, Point, Rect, Renderer, canvas_transform, color,
    image::{ImageQuad, QuadBounds},
    layer_compositor::{
        MAX_IMAGE_INSTANCES, MAX_INSTANCES, MIN_IMAGE_INSTANCES, MIN_INSTANCES, large_enough,
    },
    primitive::Primitive,
    rectangle::{RoundedPanel, Solid},
    retained_layers::{WindowLayer, WindowLayerSource, passive_content},
    scroll_images::{MAX_SCROLL_BATCHES, WindowImageBatch},
};
use femtovg::{DrawCommand, FontId, GlyphDrawCommands, ImageId, PositionedGlyph, Quad};
use std::{cell::OnceCell, rc::Rc};
use xrack_layout::{LayoutNode, RoundedClip, Size, TextMeasurer};
use xrack_runtime::{Color as UiColor, Node, Orientation};

const MAX_OCCLUDERS: usize = 4;
const MIN_OCCLUDER_PIXELS: f32 = 32.0 * 32.0;
const DEFERRED_LABEL_PASSES: usize = 2;

/// The canvas state is stable while this list is collected and submitted. The
/// clip is the rectangular extent; any inherited path clip remains on the canvas.
pub(super) struct DisplayList<'a> {
    transform: CanvasTransform,
    clip: Rect,
    scale: f32,
    pub(super) viewport_label_masks: bool,
    first: Option<Primitive<'a>>,
    rest: Vec<Primitive<'a>>,
}

#[derive(Clone, Copy)]
struct LabelBatchContext<'a> {
    label: &'a Rc<crate::text::PreparedLabel>,
    placement: crate::label_glyphs::Placement,
    retain_positions: bool,
    reuse_unicode: bool,
}

struct WindowImages {
    atlas: Option<ImageId>,
    quads: Vec<Quad>,
    transform: CanvasTransform,
    clip: Rect,
    scale: f32,
}

impl WindowImages {
    fn collect(
        &mut self,
        renderer: &mut Renderer,
        node: &Node,
        layout: &LayoutNode,
        depth: usize,
    ) -> Result<bool, String> {
        let in_layout = xrack_layout::is_layout(node.kind);
        for (_, node, layout) in layout.children_in_paint_order(node) {
            if !layout.visible || node.opacity() == 0.0 || node.scene_texture_only() {
                continue;
            }
            let visible = layout.bounds.intersect(layout.clip);
            let outside = visible.width <= 0.0 || visible.height <= 0.0;
            if node.kind != "Image" {
                // Transparent containers contribute their clip to the prepared
                // child layouts. Walking them in paint order preserves overlap.
                if depth == 0
                    || layout.child_clip_shape().is_some()
                    || layout.allows_overflow()
                    || !Renderer::is_unpainted_group(node, layout)
                    || (!outside && !self.collect(renderer, node, layout, depth - 1)?)
                {
                    return Ok(false);
                }
                continue;
            }
            if outside && Renderer::is_plain_image(node, layout) {
                continue;
            }
            let Some(
                item @ Primitive::Image {
                    quad: Some(QuadBounds::Aligned(_)),
                    ..
                },
            ) = renderer.display_image(node, layout, in_layout, self.scale, self.transform)
            else {
                return Ok(false);
            };
            if self.quads.len() == MAX_IMAGE_INSTANCES {
                return Ok(false);
            }
            let Some((image, quad)) =
                renderer.prepare_image_quad(&item, self.clip, self.transform)?
            else {
                return Ok(false);
            };
            if self.atlas.map_or_else(
                || renderer.images.source_atlas(image).is_none(),
                |atlas| atlas != image,
            ) {
                return Ok(false);
            }
            self.atlas = Some(image);
            self.quads.push(quad);
        }
        Ok(true)
    }
}

pub(super) struct PendingSurface<'a> {
    node: &'a Node,
    layout: &'a LayoutNode,
    path: Vec<usize>,
    in_layout: bool,
    mask: Option<usize>,
    live: bool,
    viewport_clip: Option<Rect>,
}

impl PendingSurface<'_> {
    pub(super) fn paint(
        mut self,
        renderer: &mut Renderer,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        let mut paint = |renderer: &mut Renderer| {
            renderer.paint(
                self.node,
                self.layout,
                &mut self.path,
                frame,
                self.in_layout,
            )
        };
        if let Some(clip) = self.viewport_clip {
            renderer.paint_group(clip, 1.0, paint)
        } else {
            paint(renderer)
        }
    }
}

enum PendingLayer<'a> {
    Surface(PendingSurface<'a>),
    ScrollingImages {
        node: &'a Node,
        layout: &'a LayoutNode,
        path: Vec<usize>,
        in_layout: bool,
        clip: Rect,
    },
    Capsule {
        bounds: Rect,
        tint: femtovg::Color,
        clip: Rect,
        clip_y: [f32; 2],
        mask: Option<usize>,
    },
}

#[derive(Clone, Copy, Default)]
struct WindowClip {
    rect: Option<Rect>,
    mask: Option<usize>,
}

#[derive(Default)]
struct WindowLayers<'a> {
    background: UiColor,
    layers: Vec<(WindowLayer, Option<usize>)>,
    pending: Vec<(usize, PendingLayer<'a>)>,
    capsule_layers: usize,
    animating: bool,
    path: Vec<usize>,
    scopes: Vec<Rc<[RoundedClip]>>,
    allow_scroll_batch: bool,
    scroll_batch_count: usize,
}

pub(super) struct WindowComposition<'a> {
    pub(super) layers: Vec<WindowLayer>,
    pub(super) scrolls: Vec<(usize, WindowImageBatch)>,
    pub(super) animating: bool,
    pub(super) live_paints: Vec<PendingSurface<'a>>,
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
        if node.scene_texture_only() {
            return Some(());
        }
        let order = self.layers.len() + self.pending.len();
        if order + self.capsule_layers >= MAX_INSTANCES
            || layout.has_ancestor_clip_chain() != clip.mask.is_some()
        {
            return None;
        }
        if self.allow_scroll_batch
            && clip.mask.is_none()
            && !layout.allows_overflow()
            && layout.child_clip_shape().is_none()
            && (MIN_IMAGE_INSTANCES..=MAX_IMAGE_INSTANCES).contains(&node.children.len())
            && Self::is_unpainted_viewport(node, layout)
        {
            if self.scroll_batch_count == MAX_SCROLL_BATCHES {
                return None;
            }
            let viewport = layout.scroll_viewport(xrack_runtime::Orientation::Vertical);
            let bounds = viewport.intersect(layout.clip);
            let rect = clip.rect.map_or(bounds, |clip| bounds.intersect(clip));
            if !crate::pixel_aligned(rect, frame.scale) {
                return None;
            }
            self.pending.push((
                order,
                PendingLayer::ScrollingImages {
                    node,
                    layout,
                    path: self.path.clone(),
                    in_layout,
                    clip: rect,
                },
            ));
            self.scroll_batch_count += 1;
            return Some(());
        }
        if node.kind == "ProgressIndicator" {
            return self.collect_progress(node, layout, frame, clip);
        }
        if clip.mask.is_none()
            && let Some((bounds, color)) =
                crate::rectangle::Solid::from_node(node, layout, frame.scale)
                    .and_then(crate::rectangle::Solid::plain_layer)
        {
            let bounds = clip.rect.map_or(bounds, |clip| bounds.intersect(clip));
            self.layers.push((WindowLayer::solid(bounds, color), None));
            return Some(());
        }
        let visible = clip
            .rect
            .map_or(layout.clip, |clip| clip.intersect(layout.clip));
        // A pure background matching the clear color does not need an
        // atlas tile or a surface texture. Keep collecting its clipped children.
        let clear_panel = order == 0
            && !node.children.is_empty()
            && node.opacity() == 1.0
            && node.color("background") == Some(self.background)
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
            && crate::rectangle::plain_surface(node, layout);
        if !clear_panel {
            if order + self.capsule_layers + 9 <= MAX_INSTANCES
                && let Some(panel) = RoundedPanel::from_node(node, layout, frame.scale)
                && crate::pixel_aligned(visible, frame.scale)
                && let Some(parts) = renderer.rectangle_batches.window_panel(
                    (&renderer.instance, &renderer.device, &renderer.queue),
                    panel,
                    frame.scale,
                )
            {
                self.layers.extend(parts.map(|mut layer| {
                    if !contains_rect(visible, layer.bounds) {
                        layer.crop(visible);
                    }
                    (layer, clip.mask)
                }));
            } else if let Some(mut layer) =
                renderer.cached_window_layer(node, layout, &self.path, in_layout, frame)
            {
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
                self.pending.push((
                    order,
                    PendingLayer::Surface(PendingSurface {
                        node,
                        layout,
                        path: self.path.clone(),
                        in_layout,
                        mask: clip.mask,
                        live: false,
                        viewport_clip: None,
                    }),
                ));
                if node.children.is_empty() {
                    return Some(());
                }
            } else if !Renderer::is_unpainted_group(node, layout)
                && !Self::is_unpainted_viewport(node, layout)
            {
                return None;
            }
        }

        if layout.allows_overflow() {
            return None;
        }
        // A pixel-aligned scissor can be applied by trimming the child quads.
        let viewport = if node.kind == "ScrollView" {
            layout.scroll_viewport(xrack_runtime::Orientation::Vertical)
        } else {
            layout.bounds
        };
        let content = viewport.intersect(layout.clip);
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

    fn is_unpainted_viewport(node: &Node, layout: &LayoutNode) -> bool {
        matches!(node.kind, "Flickable" | "ScrollView")
            && node.opacity() == 1.0
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
            && node.list.is_none()
            && layout.table.is_none()
            && node.scroll_target.is_none()
            && !node.has_scroll_parts()
            && layout.scroll_bars(node).next().is_none()
            && node.properties.keys().all(|name| {
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
                        | "layout-direction"
                        | "content-width"
                        | "content-height"
                        | "interactive"
                        | "flick-direction"
                        | "mouse-drag-pan-enabled"
                        | "horizontal-scroll-bar-policy"
                        | "vertical-scroll-bar-policy"
                        | "reserve-scroll-bar-space"
                        | "transform-origin-x"
                        | "transform-origin-y"
                        | "transform-scale"
                        | "transform-scale-x"
                        | "transform-scale-y"
                        | "transform-rotation"
                )
            })
    }

    fn collect_progress(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
        clip: WindowClip,
    ) -> Option<()> {
        let bounds = layout.bounds;
        let viewport = clip
            .rect
            .map_or(layout.clip, |clip| layout.clip.intersect(clip));
        let raster = crate::retained_layers::logical_bounds(
            crate::retained_layers::device_bounds(bounds, frame.scale),
            frame.scale,
        );
        let visible = viewport.intersect(raster);
        if node.opacity() != 1.0
            || !node.children.is_empty()
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || layout.allows_overflow()
            || frame.scale < 1.0
            || [bounds.x, bounds.x + bounds.width]
                .into_iter()
                .any(|x| (x * frame.scale).fract() != 0.0)
            || !crate::pixel_aligned(visible, frame.scale)
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
                        | "layout-direction"
                        | "progress"
                        | "minimum"
                        | "maximum"
                        | "animated"
                        | "indeterminate"
                        | "track-color"
                        | "color"
                        | "min-width"
                        | "max-width"
                        | "min-height"
                        | "max-height"
                        | "preferred-width"
                        | "preferred-height"
                        | "horizontal-stretch"
                        | "vertical-stretch"
                )
            })
        {
            return None;
        }
        let content = viewport.intersect(bounds);
        if content.width <= 0.0 || content.height <= 0.0 {
            return Some(());
        }
        // A horizontal cut can affect how the path painter treats a soft
        // vertical scissor. Keep that combination on its general path.
        if content.width != bounds.width
            && [content.y, content.y + content.height]
                .into_iter()
                .any(|y| (y * frame.scale).fract() != 0.0)
        {
            return None;
        }
        let top = bounds.y * frame.scale;
        let bottom = (bounds.y + bounds.height) * frame.scale;
        let cut_top = content.y > bounds.y;
        let cut_bottom = content.y + content.height < bounds.y + bounds.height;
        // Integer cuts are applied to the composed quad. For a remaining
        // soft edge, retain one device pixel of scissor intrusion on the
        // opposite side to keep coverage active in the tile. Deeper cuts
        // share the same tile and only change the subsequent quad crop.
        let clip_y = if cut_top && !cut_bottom && bottom.fract() != 0.0 {
            let top = top.floor() + 1.0;
            [top / frame.scale, (bottom - top) / frame.scale]
        } else if cut_bottom && !cut_top && top.fract() != 0.0 {
            let bottom = bottom.ceil() - 1.0;
            [bounds.y, (bottom - top) / frame.scale]
        } else {
            [bounds.y, bounds.height]
        };
        let state = xrack_runtime::indicator_state(node)?;
        let phase = frame
            .indicator_phase
            .filter(|_| layout.enabled && node.boolean("animated", true));
        let fill =
            xrack_layout::progress_fill_bounds(bounds, layout.layout_direction, state, phase);
        let track_color = color(crate::range::progress_track_color(frame.theme, node));
        let fill_color = color(crate::range::indicator_foreground(
            frame.theme,
            node,
            layout.enabled,
        ));
        for (bounds, tint) in [(bounds, track_color), (fill, fill_color)] {
            if bounds.width <= 0.0 || tint.a == 0.0 {
                continue;
            }
            crate::rectangle::RectangleBatches::capsule_template_width(bounds, frame.scale)?;
            let order = self.layers.len() + self.pending.len();
            if order + self.capsule_layers + 3 > MAX_INSTANCES {
                return None;
            }
            self.pending.push((
                order,
                PendingLayer::Capsule {
                    bounds,
                    tint,
                    clip: visible,
                    clip_y,
                    mask: clip.mask,
                },
            ));
            self.capsule_layers += 2;
        }
        self.animating |= phase.is_some()
            && matches!(state, xrack_runtime::IndicatorState::Indeterminate)
            && fill.width > 0.0
            && fill.height > 0.0
            && fill_color.a > 0.0;
        Some(())
    }
}

fn contains_rect(clip: Rect, bounds: Rect) -> bool {
    bounds.x >= clip.x
        && bounds.y >= clip.y
        && bounds.x + bounds.width <= clip.x + clip.width
        && bounds.y + bounds.height <= clip.y + clip.height
}

fn mark_redundant_layers(
    layers: &mut [(WindowLayer, Option<usize>)],
    background: UiColor,
    scale: f32,
) {
    let mut clear_prefix = (background.0 & 255) == 255;
    let clear = color(background);
    for (layer, _) in layers.iter_mut() {
        if !(layer.bounds.width > 0.0 && layer.bounds.height > 0.0) {
            continue;
        }
        if clear_prefix
            && match &layer.source {
                WindowLayerSource::Solid(fill) => {
                    [fill.r, fill.g, fill.b] == [clear.r, clear.g, clear.b]
                }
                WindowLayerSource::TintedAlpha { tint, .. } => *tint == background.0 >> 8,
                WindowLayerSource::Texture(_) | WindowLayerSource::CoveredSolid { .. } => false,
            }
        {
            layer.bounds.width = 0.0;
        } else {
            clear_prefix = false;
        }
    }

    let mut occluders = [Rect::default(); MAX_OCCLUDERS];
    let mut count = 0;
    let area = |rect: Rect| rect.width * rect.height;
    let min_area = MIN_OCCLUDER_PIXELS / (scale * scale);
    // Keep the layer owners even when their draws are culled so their
    // textures remain ready when the covering rectangle moves away.
    for (layer, scope) in layers.iter_mut().rev() {
        if !(layer.bounds.width > 0.0 && layer.bounds.height > 0.0) {
            continue;
        }
        if occluders[..count]
            .iter()
            .any(|&cover| contains_rect(cover, layer.bounds))
        {
            layer.bounds.width = 0.0;
            continue;
        }
        if scope.is_some()
            || !matches!(&layer.source, WindowLayerSource::Solid(fill) if fill.a == 1.0)
            || area(layer.bounds) < min_area
        {
            continue;
        }
        if count < MAX_OCCLUDERS {
            occluders[count] = layer.bounds;
            count += 1;
        } else if let Some(smallest) = occluders
            .iter_mut()
            .min_by(|a, b| area(**a).total_cmp(&area(**b)))
            && area(layer.bounds) > area(*smallest)
        {
            *smallest = layer.bounds;
        }
    }
}

fn inside_clip_interiors(shapes: &[RoundedClip], bounds: Rect, scale: f32) -> bool {
    // Leave a device pixel around the draw for the clip's soft edge.
    // The two central strips of a rounded rectangle have full coverage.
    let inset = bounds.inset(-1.0 / scale);
    shapes.iter().all(|shape| {
        let [tl, tr, br, bl] = shape.radii;
        let top = tl.max(tr);
        let bottom = bl.max(br);
        let left = tl.max(bl);
        let right = tr.max(br);
        let bounds = shape.bounds;
        let horizontal = Rect::new(
            bounds.x,
            bounds.y + top,
            bounds.width,
            bounds.height - top - bottom,
        );
        let vertical = Rect::new(
            bounds.x + left,
            bounds.y,
            bounds.width - left - right,
            bounds.height,
        );
        contains_rect(horizontal, inset) || contains_rect(vertical, inset)
    })
}

impl<'a> DisplayList<'a> {
    pub(super) fn cached_layers(
        renderer: &mut Renderer,
        root: &'a Node,
        layout: &'a LayoutNode,
        frame: PaintFrame<'_>,
        background: UiColor,
    ) -> Option<WindowComposition<'a>> {
        if layout.allows_overflow() || !Self::plain_window(root) {
            return None;
        }
        let allow_scroll_batch = frame.scale >= 1.0
            && renderer
                .device
                .features()
                .contains(wgpu::Features::IMMEDIATES)
            && renderer.scroll_images.has_layered_viewport(root);
        if !allow_scroll_batch && !large_enough(renderer.opacity_layer_count()) {
            return None;
        }
        let mut collected = WindowLayers {
            background,
            allow_scroll_batch,
            ..WindowLayers::default()
        };
        collected
            .layers
            .reserve(root.children.len().min(MAX_INSTANCES));
        collected.collect_children(renderer, root, layout, frame, WindowClip::default())?;
        let WindowLayers {
            mut layers,
            mut pending,
            scopes,
            animating,
            scroll_batch_count,
            ..
        } = collected;
        if scroll_batch_count == 0 && !large_enough(layers.len()) {
            return None;
        }
        // Observe every candidate on this frame, including when only some
        // of the leaves are ready to capture.
        let stable = pending.iter_mut().fold(true, |stable, (_, layer)| {
            let PendingLayer::Surface(item) = layer else {
                return stable;
            };
            let admitted =
                renderer.admit_window_surface(item.node, item.layout, &item.path, frame.scale);
            item.live = !admitted
                && scroll_batch_count > 0
                && item.mask.is_none()
                && matches!(item.node.kind, "Label" | "Text");
            (admitted || item.live) && stable
        });
        if !stable {
            return None;
        }
        // Admit all sibling viewports together so a newly opened pane does
        // not delay admission of the panes that follow it in paint order.
        let mut image_batches = Vec::with_capacity(scroll_batch_count);
        let mut ready_scrolls = 0;
        for (_, layer) in &pending {
            if let PendingLayer::ScrollingImages {
                node,
                layout,
                path,
                clip,
                ..
            } = layer
            {
                let scale = frame.scale;
                let transform =
                    CanvasTransform::from_coefficients(layout.world_transform().coefficients())
                        .then(xrack_layout::Transform2D::from_coefficients([
                            scale, 0.0, 0.0, scale, 0.0, 0.0,
                        ]));
                let batch = renderer
                    .retained_scroll_image_batch(node, layout, transform, *clip, scale, path)
                    .ok()?;
                ready_scrolls += usize::from(batch.is_some());
                image_batches.push(batch);
            }
        }
        if scroll_batch_count != 0 && ready_scrolls == 0 {
            return None;
        }
        let mut image_batches = image_batches.into_iter();
        let mut extra = 0;
        let mut skipped = 0;
        let mut scrolls = Vec::with_capacity(ready_scrolls);
        let mut scroll_clips = Vec::with_capacity(ready_scrolls);
        let mut changing_surfaces = Vec::new();
        for (order, layer) in pending {
            let index = order + extra - skipped;
            match layer {
                PendingLayer::Surface(item) if item.live => {
                    changing_surfaces.push((index, scrolls.len(), item));
                    skipped += 1;
                }
                PendingLayer::Surface(item) => {
                    let layer = renderer.capture_window_surface(
                        item.node,
                        item.layout,
                        &item.path,
                        item.in_layout,
                        frame,
                    )?;
                    layers.insert(index, (layer, item.mask));
                }
                PendingLayer::ScrollingImages {
                    node,
                    layout,
                    path,
                    in_layout,
                    clip,
                } => {
                    if let Some(batch) = image_batches.next()? {
                        scrolls.push((index, batch));
                        scroll_clips.push(clip);
                    } else {
                        if !node.children.iter().all(passive_content) {
                            return None;
                        }
                        changing_surfaces.push((
                            index,
                            scrolls.len(),
                            PendingSurface {
                                node,
                                layout,
                                path,
                                in_layout,
                                mask: None,
                                live: true,
                                viewport_clip: Some(clip),
                            },
                        ));
                    }
                    skipped += 1;
                }
                PendingLayer::Capsule {
                    bounds,
                    tint,
                    clip,
                    clip_y,
                    mask,
                } => {
                    let parts = renderer.rectangle_batches.window_capsule(
                        (&renderer.instance, &renderer.device, &renderer.queue),
                        bounds,
                        tint,
                        frame.scale,
                        clip_y,
                    )?;
                    layers.splice(
                        index..index,
                        parts.map(|mut layer| {
                            if !contains_rect(clip, layer.bounds) {
                                layer.crop(clip);
                            }
                            (layer, mask)
                        }),
                    );
                    extra += 2;
                }
            }
        }
        let mut live_paints = Vec::new();
        let mut viewport_capture_bytes = 0;
        // Use the normal painter after composition if the surface is disjoint
        // from later native draws. Resolve later surfaces first so a raster
        // inserted here also blocks earlier overlapping surfaces.
        for (index, preceding_scrolls, item) in changing_surfaces.into_iter().rev() {
            let bounds = item
                .viewport_clip
                .unwrap_or_else(|| item.layout.bounds.inset(-edge_reach(frame.scale)));
            if scroll_clips[preceding_scrolls..]
                .iter()
                .any(|clip| overlaps(bounds, *clip))
                || layers[index..]
                    .iter()
                    .any(|(layer, _)| overlaps(bounds, layer.bounds))
            {
                let mask = item.mask;
                let layer = if let Some(clip) = item.viewport_clip {
                    renderer.capture_window_viewport(
                        clip,
                        frame.scale,
                        &mut viewport_capture_bytes,
                        |renderer| item.paint(renderer, frame),
                    )?
                } else {
                    renderer.capture_window_surface(
                        item.node,
                        item.layout,
                        &item.path,
                        item.in_layout,
                        frame,
                    )?
                };
                layers.insert(index, (layer, mask));
                for (boundary, _) in &mut scrolls[preceding_scrolls..] {
                    *boundary += 1;
                }
            } else {
                live_paints.push(item);
            }
        }
        live_paints.reverse();
        if scrolls.is_empty() {
            mark_redundant_layers(&mut layers, background, frame.scale);
        }
        let mut masks = vec![None; scopes.len()];
        for (layer, scope) in &mut layers {
            if let Some(scope) = *scope
                && layer.bounds.width > 0.0
                && layer.bounds.height > 0.0
                && !inside_clip_interiors(&scopes[scope], layer.bounds, frame.scale)
            {
                if masks[scope].is_none() {
                    masks[scope] = Some(renderer.window_clip_mask(&scopes[scope], frame.scale)?);
                }
                layer.sample_mask = true;
            }
        }
        let layers = layers
            .into_iter()
            .map(|(mut layer, scope)| {
                // Keep layers of a clipped scope in the same material. The
                // per-instance flag still skips sampling for interior quads.
                if layer.bounds.width > 0.0 && layer.bounds.height > 0.0 {
                    layer.mask = scope.and_then(|scope| masks[scope].clone());
                }
                layer
            })
            .collect();
        Some(WindowComposition {
            layers,
            scrolls,
            animating,
            live_paints,
        })
    }

    pub(super) fn plain_window(root: &Node) -> bool {
        root.properties.keys().all(|&name| {
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
    }

    pub(super) fn window_opaque_rectangles(
        root: &Node,
        layout: &LayoutNode,
        scale: f32,
        max_instances: usize,
    ) -> Option<Vec<WindowLayer>> {
        if layout.allows_overflow() || !Self::plain_window(root) {
            return None;
        }
        let mut layers = Vec::new();
        collect_window_rectangles(root, layout, layout.clip, scale, max_instances, &mut layers)?;
        if layers.len() < MIN_INSTANCES {
            return None;
        }
        if layers
            .iter()
            .any(|layer| matches!(layer.source, WindowLayerSource::CoveredSolid { .. }))
        {
            // A rectangle on an exact pixel boundary has full coverage.
            // Submit both kinds in one ordered draw when coverage is needed.
            for layer in &mut layers {
                if let WindowLayerSource::Solid(color) = &layer.source {
                    *layer = WindowLayer::covered_solid(layer.bounds, *color, scale);
                }
            }
        }
        Some(layers)
    }

    pub(super) fn window_image_quads(
        renderer: &mut Renderer,
        root: &Node,
        layout: &LayoutNode,
        scale: f32,
    ) -> Result<Option<WindowImageBatch>, String> {
        let (group, layout, scrolling) =
            match (root.children.as_slice(), layout.children.as_slice()) {
                ([group], [item])
                    if item.visible && WindowLayers::is_unpainted_viewport(group, item) =>
                {
                    (group, item, true)
                }
                ([group], [item])
                    if item.visible && Renderer::is_unpainted_container(group, item) =>
                {
                    (group, item, false)
                }
                _ => (root, layout, false),
            };
        if !(MIN_IMAGE_INSTANCES..=MAX_IMAGE_INSTANCES).contains(&group.children.len())
            || scale < 1.0
            || !Self::plain_window(root)
            || layout.allows_overflow()
            || layout.child_clip_shape().is_some()
        {
            return Ok(None);
        }
        // A sole transformed child inherits the window's rectangular clip.
        // A positive axis transform preserves it as the local rectangular scissor.
        let transform = CanvasTransform::from_coefficients(layout.world_transform().coefficients())
            .then(xrack_layout::Transform2D::from_coefficients([
                scale, 0.0, 0.0, scale, 0.0, 0.0,
            ]));
        let viewport = if group.kind == "ScrollView" {
            layout.scroll_viewport(xrack_runtime::Orientation::Vertical)
        } else {
            layout.bounds
        };
        let clip = layout.clip.intersect(viewport);
        if !crate::image::supports_quad_transform(transform)
            || !crate::image::quad_edges_are_pixel_aligned(clip, transform)
        {
            return Ok(None);
        }
        if scrolling
            && let Some(batch) =
                renderer.retained_scroll_image_batch(group, layout, transform, clip, scale, &[0])?
        {
            return Ok(Some(batch));
        }
        let Some([sx, sy]) = transform.axis_scales() else {
            return Ok(None);
        };
        let [tx, ty] = transform.translation();
        let mut images = WindowImages {
            atlas: None,
            quads: Vec::with_capacity(group.children.len()),
            transform,
            clip,
            scale,
        };
        let complete = images.collect(renderer, group, layout, 8)?;
        renderer.images.flush_batch_uploads(&mut renderer.canvas)?;
        let Some(atlas) = images
            .atlas
            .filter(|_| complete && images.quads.len() >= MIN_IMAGE_INSTANCES)
        else {
            return Ok(None);
        };
        for quad in &mut images.quads {
            quad.x0 = quad.x0 * sx + tx;
            quad.x1 = quad.x1 * sx + tx;
            quad.y0 = quad.y0 * sy + ty;
            quad.y1 = quad.y1 * sy + ty;
        }
        Ok(Some(WindowImageBatch::Visible {
            atlas,
            quads: images.quads,
        }))
    }

    pub(super) fn new(renderer: &Renderer, clip: Rect, scale: f32) -> Self {
        Self {
            transform: canvas_transform(&renderer.canvas),
            clip,
            scale,
            viewport_label_masks: false,
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
        // A mask miss can split the run. Keep small lists on the regular
        // batch path, where splitting often costs an extra draw.
        const MIN_CROPPED_RUNS: usize = 16;
        let crop_glyphs = self.rest.len() + 1 >= MIN_CROPPED_RUNS;
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
                    renderer.draw_rectangle_batch(&mut rectangles, self.scale, self.clip)?;
                }
                item @ (Primitive::Image { quad: Some(_), .. } | Primitive::Icon { .. })
                    if image_quads =>
                {
                    let order = batch_order(&item);
                    if items.peek().is_none_or(|next| batch_order(next) != order) {
                        renderer.draw_primitive(item)?;
                        continue;
                    }
                    let Some(first) =
                        renderer.prepare_image_quad(&item, self.clip, self.transform)?
                    else {
                        renderer.draw_primitive(item)?;
                        continue;
                    };
                    let mut quads = Vec::new();
                    while let Some(next) = items.peek() {
                        if batch_order(next) != order {
                            break;
                        }
                        let Some(quad) =
                            renderer.prepare_image_quad(next, self.clip, self.transform)?
                        else {
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
                    renderer.draw_label_batch(
                        item,
                        &mut items,
                        self.transform,
                        self.viewport_label_masks,
                        DEFERRED_LABEL_PASSES,
                    )?;
                }
                item @ Primitive::Text {
                    cell_clip: Some(_), ..
                } if can_batch => {
                    renderer.draw_clipped_text_batch(
                        item,
                        &mut items,
                        self.transform,
                        self.scale,
                        crop_glyphs,
                    )?;
                }
                Primitive::Text {
                    origin,
                    text,
                    paint,
                    cell_clip: None,
                    ..
                } if can_batch => {
                    if !matches!(items.peek(), Some(Primitive::Text { paint: next, cell_clip: None, .. }) if std::ptr::eq(paint, *next))
                    {
                        renderer.draw_primitive(Primitive::Text {
                            origin,
                            text,
                            paint,
                            cell_clip: None,
                            cell_fonts: None,
                        })?;
                        continue;
                    }
                    let mut lines = vec![(origin, text)];
                    while let Some(Primitive::Text {
                        origin,
                        text,
                        paint: next,
                        cell_clip: None,
                        ..
                    }) = items.peek()
                    {
                        if !std::ptr::eq(paint, *next) {
                            break;
                        }
                        lines.push((*origin, *text));
                        items.next();
                    }
                    if !renderer.draw_glyph_batch(&lines, paint, None)? {
                        renderer.draw_text_lines(lines, paint)?;
                    }
                }
                item => renderer.draw_primitive(item)?,
            }
        }
        Ok(())
    }
}

fn collect_window_rectangles(
    parent: &Node,
    layout: &LayoutNode,
    clip: Rect,
    scale: f32,
    max_instances: usize,
    layers: &mut Vec<WindowLayer>,
) -> Option<()> {
    for (_, node, item) in layout.children_in_paint_order(parent) {
        if !item.visible || node.scene_texture_only() {
            continue;
        }
        if let Some(rectangle) = Solid::from_node(node, item, scale) {
            if rectangle.visible() {
                if layers.len() == max_instances
                    || !contains_rect(clip, item.clip.intersect(item.bounds))
                {
                    return None;
                }
                layers.push(rectangle.window_opaque_layer(clip, scale)?);
            }
        } else if Renderer::is_unpainted_group(node, item)
            && !item.allows_overflow()
            && item.child_clip_shape().is_none()
        {
            // Transparent groups add their rectangular clip to child layout.
            // The flattened draw inherits the same window scissor as its parent.
            collect_window_rectangles(node, item, clip, scale, max_instances, layers)?;
        } else {
            return None;
        }
    }
    Some(())
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
            cell_clip: Some(clip),
            ..
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
        if antialiased {
            sampled_bounds(bounds, fringe)
        } else {
            bounds
        }
    })
}

fn sampled_bounds(bounds: Rect, fringe: f32) -> Rect {
    let bounds = bounds.inset(-fringe);
    // The render target has one sample at the center of each pixel.
    // Intersecting fringes only prevent reordering if they reach the same sample.
    let left = (bounds.x - 0.5).ceil();
    let top = (bounds.y - 0.5).ceil();
    let right = (bounds.x + bounds.width - 0.5).floor() + 1.0;
    let bottom = (bounds.y + bounds.height - 0.5).floor() + 1.0;
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn overlaps(first: Rect, second: Rect) -> bool {
    let overlap = first.intersect(second);
    overlap.width > 0.0 && overlap.height > 0.0
}

#[derive(Default)]
struct PaintRegions {
    bounds: Vec<Rect>,
    earlier_bottom: Option<f32>,
}

impl PaintRegions {
    fn add(&mut self, bounds: Rect) {
        const MAX_REGIONS: usize = 32;
        let at_limit = self.bounds.len() == MAX_REGIONS;
        if let Some(last) = self.bounds.last_mut() {
            let bottom = last.y + last.height;
            if at_limit || bounds.y < bottom && last.y < bounds.y + bounds.height {
                *last = crate::shadow::union(*last, bounds);
                return;
            }
            self.earlier_bottom = Some(self.earlier_bottom.map_or(bottom, |y| y.max(bottom)));
        }
        self.bounds.push(bounds);
    }

    fn overlaps(&self, bounds: Rect) -> bool {
        self.bounds.split_last().is_some_and(|(&latest, earlier)| {
            // In paint order a table usually advances one row at a time.
            // Skip older rows when all of them end above this draw.
            overlaps(latest, bounds)
                || (self.earlier_bottom.is_some_and(|y| y > bounds.y)
                    && earlier.iter().any(|&clip| overlaps(clip, bounds)))
        })
    }

    fn clear(&mut self) {
        self.bounds.clear();
        self.earlier_bottom = None;
    }
}

fn reorder_disjoint(items: &mut [Primitive<'_>], transform: CanvasTransform, dpi: f32) {
    let mut start = 0;
    let mut envelopes: [PaintRegions; 6] = Default::default();
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
                .any(|later| later.overlaps(bounds))
        }) {
            if mixed {
                items[start..index].sort_by_key(batch_order);
            }
            start = index;
            envelopes.iter_mut().for_each(PaintRegions::clear);
            highest_order = 0;
            mixed = false;
        }
        if let Some(bounds) = bounds {
            mixed |= order < highest_order;
            highest_order = highest_order.max(order);
            // Keep disjoint rows separate so their combined bounding box does
            // not cover the empty space in the current row.
            envelopes[order].add(bounds);
        } else {
            start = index + 1;
        }
    }
    if mixed {
        items[start..].sort_by_key(batch_order);
    }
}

#[derive(Default)]
struct GlyphBatch {
    font: Option<FontId>,
    positions: Vec<PositionedGlyph>,
    clips: PaintRegions,
    scissors: Vec<ClippedGlyphBand>,
    // Font size and atlas scale are fixed while collecting one batch.
    glyph_extents: [Option<CachedGlyphExtents>; 32],
}

struct ClippedGlyphBand {
    axis: Orientation,
    clip: Rect,
    positions: Vec<PositionedGlyph>,
    runs: u64,
}

impl ClippedGlyphBand {
    fn mask_edges(&self, scale: f32) -> Option<[f32; 4]> {
        let clip = self.clip.scaled(scale);
        let mut edges = [
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        ];
        let axis = match self.axis {
            Orientation::Horizontal => {
                edges[0] = clip.x;
                edges[2] = (self.clip.x + self.clip.width) * scale;
                0
            }
            Orientation::Vertical => {
                edges[1] = clip.y;
                edges[3] = (self.clip.y + self.clip.height) * scale;
                1
            }
        };
        // The other axis already contains the glyph samples. On this
        // axis a scissor ramps over 1/scale device pixels. Rounding the
        // edge is exact when every pixel center is outside that ramp.
        let full_coverage = 0.5 - 0.5 / scale;
        for edge in [axis, axis + 2] {
            let pixel_edge = edges[edge].round();
            if !((edges[edge] - pixel_edge).abs() <= full_coverage) {
                return None;
            }
            edges[edge] = pixel_edge;
        }
        Some(edges)
    }
}

impl GlyphBatch {
    fn clip_band(&mut self, clip: Rect, axis: Orientation, start: usize) {
        let matching = self.scissors.iter().position(|band| {
            band.axis == axis
                && match axis {
                    Orientation::Horizontal => {
                        band.clip.x == clip.x && band.clip.width == clip.width
                    }
                    Orientation::Vertical => {
                        band.clip.y == clip.y && band.clip.height == clip.height
                    }
                }
        });
        let band = if let Some(index) = matching {
            let band = &mut self.scissors[index];
            let bounds = crate::shadow::union(band.clip, clip);
            match axis {
                Orientation::Horizontal => {
                    band.clip.y = bounds.y;
                    band.clip.height = bounds.height;
                }
                Orientation::Vertical => {
                    band.clip.x = bounds.x;
                    band.clip.width = bounds.width;
                }
            }
            band
        } else {
            let index = self.scissors.len();
            self.scissors.push(ClippedGlyphBand {
                axis,
                clip,
                positions: Vec::new(),
                runs: 0,
            });
            &mut self.scissors[index]
        };
        band.positions.extend(self.positions.drain(start..));
        band.runs += 1;
    }
}

#[derive(Clone, Copy)]
struct CachedGlyphExtents {
    font: FontId,
    glyph: u16,
    extents: Option<crate::text::GlyphExtents>,
}

struct LineGlyphBatch {
    font: Option<FontId>,
    positions: Vec<PositionedGlyph>,
    offscreen: Option<OffscreenGlyphs>,
    bottom: f32,
}

impl LineGlyphBatch {
    fn push(&mut self, text: &crate::TextSystem, font: FontId, glyph: PositionedGlyph) -> bool {
        if self.font.is_some_and(|previous| previous != font) {
            return false;
        }
        self.font = Some(font);
        if !self.offscreen.as_mut().is_some_and(|offscreen| {
            offscreen.classify(text, font, &glyph) == GlyphVisibility::RepeatedOffscreen
        }) {
            self.positions.push(glyph);
        }
        true
    }

    fn accept_line(&mut self, top: f32, height: f32, scale: f32) -> bool {
        // The atlas may submit color and alpha glyphs separately.
        if top < self.bottom {
            return false;
        }
        self.bottom = top + height + 1.0 / scale;
        true
    }
}

struct OffscreenGlyphs {
    scale: f32,
    translation_y: f32,
    height: f32,
    size_range: [f32; 2],
    entries: [Option<OffscreenGlyph>; 128],
}

#[derive(Clone, Copy)]
struct OffscreenGlyph {
    font: FontId,
    id: u16,
    vertical_edges: Option<[f32; 2]>,
    empty: bool,
    emitted_phases: u32,
}

#[derive(PartialEq, Eq)]
enum GlyphVisibility {
    PossiblyVisible,
    FirstOffscreen,
    RepeatedOffscreen,
}

impl OffscreenGlyphs {
    fn classify(
        &mut self,
        text: &crate::TextSystem,
        font: FontId,
        glyph: &PositionedGlyph,
    ) -> GlyphVisibility {
        // A quick rejection avoids looking up outlines for visible lines.
        // Actual culling always checks the padded atlas bounds.
        let y = (glyph.y * self.scale).round();
        let target_y = y + self.translation_y;
        // Leave very large coordinates to the atlas, where separate additions
        // can round by more than our one-pixel safety margin.
        if (0.0..=self.height).contains(&target_y)
            || !target_y.is_finite()
            || y.abs().max(self.translation_y.abs()) > 1_048_576.0
        {
            return GlyphVisibility::PossiblyVisible;
        }
        let slot = &mut self.entries[usize::from(glyph.glyph_id) % self.entries.len()];
        if slot.is_none_or(|entry| entry.font != font || entry.id != glyph.glyph_id) {
            let outline = text.outline_glyph(font, glyph.glyph_id);
            *slot = Some(OffscreenGlyph {
                font,
                id: glyph.glyph_id,
                vertical_edges: outline
                    .and_then(|outline| outline.atlas_extents(self.size_range))
                    .map(|extents| {
                        let [_, top, _, bottom] = extents.padded_edges();
                        [top, bottom]
                    }),
                empty: outline.is_some_and(|outline| outline.bounds.is_none()),
                emitted_phases: 0,
            });
        }
        let Some(entry) = slot.as_mut() else {
            return GlyphVisibility::PossiblyVisible;
        };
        if !entry.empty
            && !entry.vertical_edges.is_some_and(|[top, bottom]| {
                y + bottom + self.translation_y < -1.0
                    || y + top + self.translation_y > self.height + 1.0
            })
        {
            return GlyphVisibility::PossiblyVisible;
        }
        // Retain the first offscreen occurrence of an unseen atlas key. Its
        // tile allocation can affect the sampling of visible glyphs.
        let phase = crate::glyph_masks::subpixel_phase((glyph.x * self.scale).fract());
        let mask = 1 << (phase + 10);
        if entry.emitted_phases & mask != 0 {
            return GlyphVisibility::RepeatedOffscreen;
        }
        entry.emitted_phases |= mask;
        GlyphVisibility::FirstOffscreen
    }
}

struct ClippedGlyphLine<'a> {
    origin: Point,
    text: &'a str,
    clip: GlyphClip,
    cell: Option<CellGlyphContext<'a>>,
    label: Option<&'a crate::text::PreparedLabel>,
    shared_scissor: bool,
}

#[derive(Clone, Copy)]
struct GlyphClip {
    logical: Rect,
    pixels: Rect,
    aligned: bool,
}

impl GlyphClip {
    fn project(logical: Rect, transform: CanvasTransform) -> Option<Self> {
        if logical.width <= 0.0 || logical.height <= 0.0 {
            return None;
        }
        // Clipped text batching has already checked for a positive uniform scale.
        let start = transform.map_point(Point {
            x: logical.x,
            y: logical.y,
        });
        let end = transform.map_point(Point {
            x: logical.x + logical.width,
            y: logical.y + logical.height,
        });
        let edges = [start.x, start.y, end.x, end.y];
        if !edges.into_iter().all(f32::is_finite) {
            return None;
        }
        let pixels = if transform.is_identity() {
            logical
        } else {
            Rect::new(start.x, start.y, end.x - start.x, end.y - start.y)
        };
        (pixels.width.is_finite()
            && pixels.height.is_finite()
            && pixels.width > 0.0
            && pixels.height > 0.0)
            .then(|| Self {
                logical,
                pixels,
                aligned: edges.into_iter().all(|edge| edge.fract() == 0.0),
            })
    }
}

#[derive(Clone, Copy)]
struct CellGlyphContext<'a> {
    scale: f32,
    fonts: [usize; 2],
    cell_fonts: Option<&'a crate::text::CellFonts>,
    measurement_paint: &'a OnceCell<Paint>,
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
        viewport_label_masks: bool,
        deferred_passes: usize,
    ) -> Result<(), String> {
        const MAX_DEFERRED_LABELS: usize = 256;
        let Primitive::Label {
            node,
            layout,
            clip,
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
            || !matches!(rest.peek(), Some(Primitive::Label { .. }))
        {
            return self.draw_primitive(first);
        }
        let Some(clip) = GlyphClip::project(*clip, transform) else {
            return self.draw_primitive(first);
        };
        let style = self.text.font_attributes(Some(node));
        let font = self.text.label_font(style, size);
        let cached = |node: &Node, layout: &LayoutNode| {
            let bounds = layout.bounds;
            self.text.cached_prepared_label(
                node,
                Size::new(bounds.width, bounds.height),
                layout.layout_direction_override(),
            )
        };
        let prepare = |node: &Node, layout: &LayoutNode| {
            let bounds = layout.bounds;
            self.text.prepared_label_with_font(
                node,
                Size::new(bounds.width, bounds.height),
                layout.layout_direction_override(),
                font,
            )
        };
        let first_label = cached(node, layout)
            .filter(|label| label.font == font)
            .unwrap_or_else(|| prepare(node, layout));
        let paint = first_label.paint(color(*tint));
        let append = |label: &crate::text::PreparedLabel,
                      bounds: Rect,
                      clip: GlyphClip,
                      dpi: f32,
                      batch: &mut GlyphBatch| {
            // Glyphs from different lines can overlap, while an atlas batch
            // groups drawing by texture.
            let Some((origin, text)) = label.only_line() else {
                return false;
            };
            self.append_clipped_glyphs(
                ClippedGlyphLine {
                    origin: Point {
                        x: bounds.x + origin.x,
                        y: bounds.y + origin.y,
                    },
                    text,
                    clip,
                    cell: None,
                    label: Some(label),
                    shared_scissor: crate::has_device_scale(transform, dpi),
                },
                &paint,
                transform,
                atlas_scale,
                dpi,
                batch,
            )
        };
        let mut batch = GlyphBatch::default();
        if !append(&first_label, layout.bounds, clip, *dpi, &mut batch) {
            return self.draw_primitive(first);
        }
        let mut count = 1;
        let mut deferred = Vec::new();
        let mut mixed_styles = false;
        let mut deferred_clips = PaintRegions::default();
        let fringe = edge_reach(*dpi);
        while let Some(Primitive::Label {
            node,
            layout,
            clip,
            tint: next_tint,
            scale,
        }) = rest.peek()
        {
            let Some(clip) = GlyphClip::project(*clip, transform) else {
                break;
            };
            let bounds = (!deferred.is_empty()).then(|| sampled_bounds(clip.pixels, fringe));
            let cached_label = if *next_tint == *tint {
                cached(node, layout)
            } else {
                None
            };
            let same_style = *next_tint == *tint
                && cached_label.as_ref().map_or_else(
                    || {
                        self.text.font_attributes(Some(node)) == style
                            && self.text.font_size(node) == size
                    },
                    |label| label.font == font,
                );
            if same_style
                && !bounds.is_some_and(|bounds| deferred_clips.overlaps(bounds))
                && append(
                    &cached_label.unwrap_or_else(|| prepare(node, layout)),
                    layout.bounds,
                    clip,
                    *scale,
                    &mut batch,
                )
            {
                rest.next();
                count += 1;
            } else {
                // Keep a clipped label in paint order relative to every later
                // label it can overlap, even if disjoint glyphs can pass it.
                if deferred.len() == MAX_DEFERRED_LABELS {
                    break;
                }
                mixed_styles |= !same_style;
                deferred_clips.add(bounds.unwrap_or_else(|| sampled_bounds(clip.pixels, fringe)));
                deferred.extend(rest.next());
            }
        }
        if count == 1 {
            self.draw_primitive(first)?;
        } else {
            self.draw_positioned_batch(batch, &paint, count, viewport_label_masks.then_some(*dpi))?;
        }
        // Deferred labels retain their order, so a second style can use the
        // same overlap checks. Bound the number of scans for many unique styles.
        if mixed_styles && deferred_passes > 0 && deferred.len() > 1 {
            let mut labels = deferred.into_iter().peekable();
            while let Some(label) = labels.next() {
                self.draw_label_batch(
                    label,
                    &mut labels,
                    transform,
                    viewport_label_masks,
                    deferred_passes - 1,
                )?;
            }
        } else {
            for label in deferred {
                self.draw_primitive(label)?;
            }
        }
        Ok(())
    }

    fn draw_clipped_text_batch<'a>(
        &mut self,
        first: Primitive<'a>,
        rest: &mut std::iter::Peekable<impl Iterator<Item = Primitive<'a>>>,
        transform: CanvasTransform,
        dpi: f32,
        crop_glyphs: bool,
    ) -> Result<(), String> {
        let Primitive::Text {
            origin,
            text,
            paint,
            cell_clip: Some(clip),
            cell_fonts,
        } = first
        else {
            return self.draw_primitive(first);
        };
        let Some([scale, sy]) = transform.axis_scales() else {
            return self.draw_primitive(first);
        };
        let atlas_scale = (scale * 16.0 + 0.5).trunc() / 16.0;
        if cfg!(any(target_os = "android", target_os = "ios"))
            || scale != sy
            || scale < 1.0 / 16.0
            || dpi < 1.0
            || paint.font_size() * atlas_scale > 90.0
            || !paint.text_decoration().is_none()
            || !matches!(rest.peek(), Some(Primitive::Text { paint: next, cell_clip: Some(_), .. }) if std::ptr::eq(paint, *next))
        {
            return self.draw_primitive(first);
        }
        let Some(clip) = GlyphClip::project(clip, transform) else {
            return self.draw_primitive(first);
        };
        let measurement_paint = OnceCell::new();
        let cell = (crate::has_device_scale(transform, dpi)
            && paint.text_align() == femtovg::Align::Left
            && paint.text_baseline() == femtovg::Baseline::Top
            && paint.letter_spacing() == 0.0)
            .then(|| CellGlyphContext {
                scale: crate::cell_glyphs::measurement_scale(dpi),
                fonts: self.text.registered_font_counts(),
                cell_fonts: cell_fonts.filter(|_| self.cell_shaping_scale(transform).is_some()),
                measurement_paint: &measurement_paint,
            })
            .filter(|cell| cell.scale.is_finite() && cell.scale > 0.0);
        let shared_scissor = crate::has_device_scale(transform, dpi);
        let line = ClippedGlyphLine {
            origin,
            text,
            clip,
            cell,
            label: None,
            shared_scissor,
        };
        if crop_glyphs
            && matches!(scale, 1.0 | 2.0 | 4.0)
            && self.can_cache_on_main_canvas()
            && self.draw_clipped_masks(&line, paint, transform, scale, rest)?
        {
            return Ok(());
        }
        let mut batch = GlyphBatch::default();
        if !self.append_clipped_glyphs(line, paint, transform, atlas_scale, dpi, &mut batch) {
            return self.draw_primitive(first);
        }
        let mut count = 1;
        let mut deferred = Vec::new();
        let mut deferred_clips = PaintRegions::default();
        let fringe = edge_reach(dpi);
        while let Some(Primitive::Text {
            origin,
            text,
            paint: next_paint,
            cell_clip: Some(clip),
            ..
        }) = rest.peek()
        {
            if !std::ptr::eq(paint, *next_paint) {
                break;
            }
            let Some(clip) = GlyphClip::project(*clip, transform) else {
                break;
            };
            let bounds = (!deferred.is_empty()).then(|| sampled_bounds(clip.pixels, fringe));
            if !bounds.is_some_and(|bounds| deferred_clips.overlaps(bounds))
                && self.append_clipped_glyphs(
                    ClippedGlyphLine {
                        origin: *origin,
                        text,
                        clip,
                        cell,
                        label: None,
                        shared_scissor,
                    },
                    paint,
                    transform,
                    atlas_scale,
                    dpi,
                    &mut batch,
                )
            {
                rest.next();
                count += 1;
            } else {
                // A clipped run can keep its own scissor without splitting
                // the batch of later glyphs that cannot overlap it.
                if deferred.len() == 256 {
                    break;
                }
                deferred_clips.add(bounds.unwrap_or_else(|| sampled_bounds(clip.pixels, fringe)));
                deferred.extend(rest.next());
            }
        }
        if count == 1 && (!batch.positions.is_empty() || !batch.scissors.is_empty()) {
            self.draw_primitive(first)?;
        } else if count > 1 {
            self.draw_positioned_batch(batch, paint, count, None)?;
        }
        for item in deferred {
            self.draw_primitive(item)?;
        }
        Ok(())
    }

    fn draw_clipped_masks<'a>(
        &mut self,
        first: &ClippedGlyphLine<'_>,
        paint: &Paint,
        transform: CanvasTransform,
        scale: f32,
        rest: &mut std::iter::Peekable<impl Iterator<Item = Primitive<'a>>>,
    ) -> Result<bool, String> {
        let Some(cell) = first.cell else {
            return Ok(false);
        };
        let portable_cell = cell.cell_fonts.is_some();
        let Some(mut masks) = self.glyph_masks.clipped_batch(paint, scale, portable_cell) else {
            return Ok(false);
        };
        let text_system = &self.text;
        let mut regions = PaintRegions::default();
        let mut cropped = 0;
        let mut append = |origin: Point, text: &str, clip: GlyphClip| {
            // A pixel-aligned clip can be applied to the quad without
            // changing the coverage of any sample the original scissor kept.
            if !clip.aligned || text.len() > 128 || regions.overlaps(clip.pixels) {
                return false;
            }
            let start = masks.quads.len();
            let bounds = clip.pixels;
            let edges = [
                bounds.x,
                bounds.y,
                bounds.x + bounds.width,
                bounds.y + bounds.height,
            ];
            masks.clipped = false;
            if !Self::visit_cell_glyphs(text_system, text, origin, paint, cell, |font, id, x, y| {
                masks.append(font, id, [x, y], edges, text_system)
            }) {
                masks.quads.truncate(start);
                return false;
            }
            cropped += u64::from(masks.clipped);
            if masks.quads.len() != start {
                regions.add(clip.pixels);
            }
            true
        };
        if !append(first.origin, first.text, first.clip) {
            return Ok(false);
        }
        let mut count = 1;
        while let Some(Primitive::Text {
            origin,
            text,
            paint: next_paint,
            cell_clip: Some(clip),
            ..
        }) = rest.peek()
        {
            if !std::ptr::eq(paint, *next_paint)
                || !GlyphClip::project(*clip, transform)
                    .is_some_and(|clip| append(*origin, text, clip))
            {
                break;
            }
            rest.next();
            count += 1;
        }
        let (image, quads) = masks.finish();
        if !quads.is_empty() {
            if self.glyph_masks.can_prepare_during_paint(portable_cell)
                && self.glyph_masks.has_pending()
                && !self.glyph_masks.prepare(&mut self.canvas)
            {
                return Err("failed to rasterize table glyph masks".into());
            }
            crate::glyph_masks::draw_mask_quads(&mut self.canvas, image, quads, paint);
            if count > 1 {
                xrack_runtime::update_profile::glyph_batch();
                self.display_batch_stats.glyph_batches += 1;
                self.display_batch_stats.text_runs += count;
                self.display_batch_stats.clipped_text_runs += cropped;
            }
        }
        Ok(true)
    }

    fn visit_cell_glyphs(
        text_system: &crate::TextSystem,
        text: &str,
        origin: Point,
        paint: &Paint,
        cell: CellGlyphContext<'_>,
        visit: impl FnMut(FontId, u16, f32, f32) -> bool,
    ) -> bool {
        let settings = crate::cell_glyphs::Settings {
            font_size: paint.font_size().to_bits(),
            measurement_scale: cell.scale.to_bits(),
            fonts: cell.fonts,
            cell_fonts: cell.cell_fonts.copied(),
        };
        text_system.cell_glyphs.borrow_mut().visit(
            text,
            settings,
            [origin.x, origin.y],
            || {
                // Measure at the origin in shaping units. Placement can
                // share this cache across both scroll axes.
                let paint = cell
                    .measurement_paint
                    .get_or_init(|| paint.clone().with_font_size(paint.font_size() * cell.scale));
                text_system.measure_cell(text, paint, cell.cell_fonts)
            },
            visit,
        )
    }

    fn draw_positioned_batch(
        &mut self,
        mut batch: GlyphBatch,
        paint: &Paint,
        runs: u64,
        viewport_scale: Option<f32>,
    ) -> Result<(), String> {
        let Some(font) = batch.font else {
            return Ok(());
        };
        let clipped_runs = batch.scissors.iter().map(|band| band.runs).sum::<u64>();
        self.draw_glyph_positions(font, batch.positions, paint, runs - clipped_runs, false)?;
        let mask_scale = viewport_scale.filter(|&scale| {
            matches!(scale, 1.0 | 2.0 | 4.0)
                && crate::has_device_scale(canvas_transform(&self.canvas), scale)
        });
        if let Some(scale) = mask_scale {
            self.draw_pixel_clipped_label_bands(font, &mut batch.scissors, paint, scale)?;
        }
        // Accepted cells occupy disjoint pixel regions. Only the axis on
        // which each run already fits can be widened to span other cells.
        for band in batch.scissors {
            let clip = band.clip;
            self.canvas.save();
            self.canvas
                .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
            // A fallback draw warms the stock atlas. The next frame can
            // batch its masks even when this band contains few glyphs.
            let cache_short_run = !cfg!(xrack_femtovg_glyph_samples)
                && mask_scale.is_some_and(|scale| band.mask_edges(scale).is_some());
            let result =
                self.draw_glyph_positions(font, band.positions, paint, band.runs, cache_short_run);
            self.canvas.restore();
            result?;
            if band.runs > 1 {
                self.display_batch_stats.clipped_text_runs += band.runs;
            }
        }
        Ok(())
    }

    fn draw_pixel_clipped_label_bands(
        &mut self,
        font: FontId,
        bands: &mut Vec<ClippedGlyphBand>,
        paint: &Paint,
        scale: f32,
    ) -> Result<(), String> {
        if bands.len() < 2 {
            return Ok(());
        }
        let Some(mut masks) = self.glyph_masks.clipped_batch(paint, scale, false) else {
            return Ok(());
        };
        let mut runs = 0;
        bands.retain(|band| {
            let Some(edges) = band.mask_edges(scale) else {
                return true;
            };
            let start = masks.quads.len();
            if !band.positions.iter().all(|glyph| {
                masks.append(font, glyph.glyph_id, [glyph.x, glyph.y], edges, &self.text)
            }) {
                masks.quads.truncate(start);
                return true;
            }
            runs += band.runs;
            false
        });
        let (image, quads) = masks.finish();
        if quads.is_empty() {
            return Ok(());
        }
        if self.glyph_masks.can_prepare_during_paint(false)
            && self.glyph_masks.has_pending()
            && !self.glyph_masks.prepare(&mut self.canvas)
        {
            return Err("failed to rasterize label glyph masks".into());
        }
        crate::glyph_masks::draw_mask_quads(&mut self.canvas, image, quads, paint);
        if runs > 1 {
            xrack_runtime::update_profile::glyph_batch();
            self.display_batch_stats.glyph_batches += 1;
            self.display_batch_stats.text_runs += runs;
            self.display_batch_stats.clipped_text_runs += runs;
        }
        Ok(())
    }

    fn draw_glyph_positions(
        &mut self,
        font: FontId,
        positions: Vec<PositionedGlyph>,
        paint: &Paint,
        runs: u64,
        cache_short_run: bool,
    ) -> Result<(), String> {
        if positions.is_empty() {
            return Ok(());
        }
        let transform = canvas_transform(&self.canvas);
        let masks = if self.can_cache_on_main_canvas()
            && let Some([scale, sy]) = transform.axis_scales()
            && scale == sy
            && matches!(scale, 1.0 | 2.0 | 4.0)
            && transform.translation() == [0.0, 0.0]
        {
            self.glyph_masks.draw(
                &mut self.canvas,
                &self.text,
                crate::glyph_masks::MaskRun {
                    font,
                    positions: crate::glyph_masks::MaskPositions::Transient(&positions),
                    paint,
                    scale,
                    cache_short_run,
                },
            )
        } else {
            false
        };
        if !masks {
            self.canvas
                .fill_glyph_run(font, &[], positions, paint)
                .map_err(|error| format!("text drawing failed: {error}"))?;
        }
        if runs > 1 {
            xrack_runtime::update_profile::glyph_batch();
            self.display_batch_stats.glyph_batches += 1;
            self.display_batch_stats.text_runs += runs;
        }
        Ok(())
    }

    fn append_clipped_glyphs(
        &self,
        line: ClippedGlyphLine<'_>,
        paint: &Paint,
        transform: CanvasTransform,
        atlas_scale: f32,
        dpi: f32,
        batch: &mut GlyphBatch,
    ) -> bool {
        let GlyphClip {
            logical,
            pixels: clip,
            aligned,
        } = line.clip;
        // At fractional edges the scissor can attenuate samples. Require
        // every sample covered by the padded quads to lie in its interior.
        let interior = if aligned {
            clip
        } else {
            clip.inset(edge_reach(dpi))
        };
        let coverage = if line.shared_scissor && !aligned {
            sampled_bounds(clip, edge_reach(dpi))
        } else {
            clip
        };
        if batch.clips.overlaps(coverage) {
            return false;
        }
        let [tx, ty] = transform.translation();
        let residual = transform.coefficients()[0] / atlas_scale;
        let size_range = crate::text::atlas_size_range(paint.font_size(), atlas_scale);
        let sample_inset = if !aligned && residual == 1.0 && tx.fract() == 0.0 && ty.fract() == 0.0
        {
            // Integer atlas quads only cover pixel centers half a pixel in.
            0.5
        } else {
            0.0
        };
        let mut font = batch.font;
        let start = batch.positions.len();
        let mut scissor_axis = None;
        let fits = (|| {
            let mut add_glyph = |font_id, glyph_id, glyph_x: f32, glyph_y: f32| {
                if font.is_some_and(|id| id != font_id) {
                    return false;
                }
                let index = usize::from(glyph_id) % batch.glyph_extents.len();
                let entry = &mut batch.glyph_extents[index];
                let extents = if let Some(cached) = *entry
                    && cached.font == font_id
                    && cached.glyph == glyph_id
                {
                    cached.extents
                } else {
                    let Some(outline) = self.text.outline_glyph(font_id, glyph_id) else {
                        return false;
                    };
                    let extents = outline.atlas_extents(size_range);
                    *entry = Some(CachedGlyphExtents {
                        font: font_id,
                        glyph: glyph_id,
                        extents,
                    });
                    extents
                };
                let Some(extents) = extents else {
                    return true;
                };
                // Match the atlas's rounded bearings, integer glyph origins,
                // upward-rounded extents and one texel of padding on each side.
                let ox = (glyph_x * atlas_scale).trunc();
                let oy = (glyph_y * atlas_scale).round();
                let x = (ox + extents.min_bearing[0] - 1.0) * residual + tx;
                let y = (oy + extents.min_bearing[1] - 1.0) * residual + ty;
                let right = (ox + extents.max_bearing[0] + extents.size[0] + 1.0) * residual + tx;
                let bottom = (oy + extents.max_bearing[1] + extents.size[1] + 1.0) * residual + ty;
                let fits_x = x + sample_inset >= interior.x
                    && right - sample_inset <= interior.x + interior.width;
                let fits_y = y + sample_inset >= interior.y
                    && bottom - sample_inset <= interior.y + interior.height;
                if !fits_x || !fits_y {
                    // A glyph outside an integer scissor contributes no samples.
                    // Keep a device pixel of clearance for the scissor's fringe.
                    if aligned
                        && (right <= clip.x - 1.0
                            || bottom <= clip.y - 1.0
                            || x >= clip.x + clip.width + 1.0
                            || y >= clip.y + clip.height + 1.0)
                    {
                        return true;
                    }
                    if !line.shared_scissor {
                        return false;
                    }
                    let axis = match (fits_x, fits_y) {
                        (false, true) => Orientation::Horizontal,
                        (true, false) => Orientation::Vertical,
                        _ => return false,
                    };
                    if scissor_axis.is_some_and(|previous| previous != axis) {
                        return false;
                    }
                    scissor_axis = Some(axis);
                }
                font = Some(font_id);
                batch.positions.push(PositionedGlyph {
                    x: glyph_x,
                    y: glyph_y,
                    glyph_id,
                });
                true
            };
            if let Some(mut cell) = line.cell.filter(|_| line.text.len() <= 128) {
                cell.cell_fonts = cell.cell_fonts.filter(|_| line.clip.aligned);
                return Self::visit_cell_glyphs(
                    &self.text,
                    line.text,
                    line.origin,
                    paint,
                    cell,
                    add_glyph,
                );
            }
            if let Some(label) = line.label
                && line.shared_scissor
                && let Some(accepted) = label.visit_glyphs(
                    self.text.context(),
                    line.origin,
                    crate::cell_glyphs::measurement_scale(dpi),
                    &mut add_glyph,
                )
            {
                return accepted;
            }
            self.canvas
                .measure_text(line.origin.x, line.origin.y, line.text, paint)
                .is_ok_and(|metrics| {
                    metrics
                        .glyphs
                        .into_iter()
                        .filter(|glyph| !glyph.c.is_control())
                        .all(|glyph| add_glyph(glyph.font_id, glyph.glyph_id, glyph.x, glyph.y))
                })
        })();
        if fits {
            batch.font = font;
            if batch.positions.len() != start {
                batch.clips.add(coverage);
                if let Some(axis) = scissor_axis {
                    batch.clip_band(logical, axis, start);
                }
            }
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
        if !xrack_layout::uses_label_layout(node) || !Self::plain_label_layout(layout) {
            return None;
        }
        let tint = self
            .text
            .plain_label_tint(node, || Self::plain_label_node(node))?;
        let tint = if layout.enabled {
            tint.unwrap_or(theme.text)
        } else {
            theme.disabled_text
        };
        Some(Primitive::Label {
            node,
            layout,
            clip: layout.clip.intersect(layout.bounds),
            tint,
            scale,
        })
    }

    pub(super) fn is_plain_label(node: &Node, layout: &LayoutNode) -> bool {
        Self::plain_label_layout(layout) && Self::plain_label_node(node)
    }

    fn plain_label_layout(layout: &LayoutNode) -> bool {
        layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
            && !layout.allows_overflow()
    }

    fn plain_label_node(node: &Node) -> bool {
        matches!(node.kind, "Label" | "Text")
            && node.opacity() == 1.0
            && node.children.is_empty()
            && node.list.is_none()
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
        let position = (label.lines().len() >= 16
            && !cfg!(any(target_os = "android", target_os = "ios"))
            && matches!(scale, 1.0 | 2.0 | 4.0)
            && self.display_batching
            && !self.using_capture_canvas()
            && !self.glyph_masks.defer_preparation
            && !self.retained_layers.vectors.opacity_passthrough
            && crate::has_device_scale(canvas_transform(&self.canvas), scale))
        .then(|| crate::label_glyphs::Placement {
            origin: [bounds.x.to_bits(), bounds.y.to_bits()],
            scale: scale.to_bits(),
            target_height: self.canvas.height(),
            registered_fonts: self.text.registered_font_counts(),
        });
        let label_batch = position.map(|placement| {
            let reuse = self
                .text
                .label_glyphs
                .borrow_mut()
                .observe_position(&label, placement);
            LabelBatchContext {
                label: &label,
                placement,
                retain_positions: reuse.positioned,
                reuse_unicode: reuse.unicode,
            }
        });
        if let Some(position) = position {
            let drawn = {
                let mut cache = self.text.label_glyphs.borrow_mut();
                if let Some(batch) = cache.positioned(&label, position) {
                    let masked = if self.can_cache_on_main_canvas()
                        && let [run] = batch.runs.as_slice()
                        && run.glyphs == (0..batch.glyphs.len())
                    {
                        self.glyph_masks.draw(
                            &mut self.canvas,
                            &self.text,
                            crate::glyph_masks::MaskRun {
                                font: run.font,
                                positions: crate::glyph_masks::MaskPositions::Retained(
                                    &batch.glyphs,
                                ),
                                paint: &paint,
                                scale,
                                cache_short_run: false,
                            },
                        )
                    } else {
                        false
                    };
                    if !masked {
                        Self::submit_label_runs(
                            &mut self.canvas,
                            &batch.runs,
                            &batch.glyphs,
                            &paint,
                        )?;
                    }
                    Some(batch.lines)
                } else {
                    None
                }
            };
            if let Some(lines) = drawn {
                self.record_label_batch(lines);
                return Ok(());
            }
        }
        let mut lines = label
            .lines()
            .filter(|(_, text)| !text.is_empty())
            .map(|(origin, text)| {
                (
                    Point {
                        x: bounds.x + origin.x,
                        y: bounds.y + origin.y,
                    },
                    text,
                )
            })
            .peekable();
        let Some(first) = lines.next() else {
            return Ok(());
        };
        let batch = self.display_batching
            && !self.retained_layers.vectors.opacity_passthrough
            && lines.peek().is_some();
        let lines = std::iter::once(first).chain(lines);
        if batch {
            let lines: Vec<_> = lines.collect();
            if self.draw_glyph_batch(&lines, &paint, label_batch)? {
                return Ok(());
            }
            return self.draw_text_lines(lines, &paint);
        }
        self.draw_text_lines(lines, &paint)
    }

    fn draw_text_lines<'a>(
        &mut self,
        lines: impl IntoIterator<Item = (Point, &'a str)>,
        paint: &Paint,
    ) -> Result<(), String> {
        for (origin, text) in lines {
            self.draw_primitive(Primitive::Text {
                origin,
                text,
                paint,
                cell_clip: None,
                cell_fonts: None,
            })?;
        }
        Ok(())
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
                if !item.visible || child.scene_texture_only() {
                    return true;
                }
                let primitive = match child.kind {
                    "Rectangle" if square_batch => {
                        let rect = crate::rectangle::Solid::from_node(child, item, display.scale)
                            .filter(|rect| rect.collect_square(display.clip, display.scale));
                        if rect.is_some_and(|rect| !rect.visible()) {
                            return true;
                        }
                        rect.map(Primitive::Rectangle)
                    }
                    "Image" => {
                        self.display_image(child, item, in_layout, display.scale, display.transform)
                    }
                    "Text" | "Label" => self.display_label(child, item, theme, display.scale),
                    _ => None,
                };
                if let Some(primitive) = primitive {
                    display.push(primitive);
                    return true;
                }
                self.collect_group_primitives(child, item, display, square_batch, theme, depth - 1)
            })
    }

    pub(super) fn is_unpainted_group(node: &Node, layout: &LayoutNode) -> bool {
        layout.local_transform().is_identity() && Self::is_unpainted_container(node, layout)
    }

    fn is_unpainted_container(node: &Node, layout: &LayoutNode) -> bool {
        if !matches!(
            node.kind,
            "Rectangle" | "HorizontalLayout" | "VerticalLayout" | "GridLayout"
        ) || node.opacity() != 1.0
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
                        | "transform-origin-x"
                        | "transform-origin-y"
                        | "transform-scale"
                        | "transform-scale-x"
                        | "transform-scale-y"
                        | "transform-rotation"
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
        transform: CanvasTransform,
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
                transform,
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

    fn draw_glyph_batch(
        &mut self,
        lines: &[(Point, &str)],
        paint: &Paint,
        label: Option<LabelBatchContext<'_>>,
    ) -> Result<bool, String> {
        // Native mobile font fallbacks and decorators are emitted by the full
        // text painter. Only combine lines whose glyphs can use the same font.
        let transform = canvas_transform(&self.canvas);
        let Some([scale, scale_y]) = transform.axis_scales() else {
            return Ok(false);
        };
        if lines.len() < 2
            || cfg!(any(target_os = "android", target_os = "ios"))
            || !paint.text_decoration().is_none()
            || !scale.is_finite()
            || scale <= 0.0
            || scale_y != scale
            || paint.font_size() * scale > 90.0
        {
            return Ok(false);
        }
        let clipped = lines.len() >= 16
            && matches!(scale, 1.0 | 2.0 | 4.0)
            && !self.using_capture_canvas()
            && !self.glyph_masks.defer_preparation;
        if lines.iter().any(|(_, text)| !text.is_ascii()) {
            if let Some(label) = label
                && clipped
                && paint.text_align() == femtovg::Align::Left
                && paint.letter_spacing() == 0.0
            {
                let visible_only = label.label.prefers_visible_glyphs();
                if label.reuse_unicode || visible_only {
                    return self.draw_unicode_label_batch(lines, paint, label, scale, visible_only);
                }
            }
            return Ok(false);
        }
        let new_batch = || LineGlyphBatch {
            font: None,
            positions: Vec::new(),
            bottom: f32::NEG_INFINITY,
            offscreen: clipped.then(|| OffscreenGlyphs {
                scale,
                translation_y: transform.translation()[1],
                height: self.canvas.height() as f32,
                size_range: crate::text::atlas_size_range(paint.font_size(), scale),
                entries: [None; 128],
            }),
        };
        let mut batch = new_batch();
        let mut reused = false;
        if let Some(label) = label
            && clipped
            && paint.text_align() == femtovg::Align::Left
            && paint.text_baseline() == femtovg::Baseline::Top
            && paint.letter_spacing() == 0.0
            && lines
                .iter()
                .all(|(_, text)| text.bytes().all(|byte| (b' '..=b'~').contains(&byte)))
        {
            let measurement_scale = crate::cell_glyphs::measurement_scale(scale);
            let mut cache = self.text.label_glyphs.borrow_mut();
            cache.select(
                label.label.font,
                measurement_scale,
                label.placement.registered_fonts,
            );
            let word_paint = paint
                .clone()
                .with_font_size(paint.font_size() * measurement_scale);
            reused = lines.iter().all(|(origin, text)| {
                cache
                    .place(
                        self.text.context(),
                        &word_paint,
                        text,
                        *origin,
                        measurement_scale,
                        |font, glyph| batch.push(&self.text, font, glyph),
                    )
                    .is_some_and(|[top, height]| batch.accept_line(top, height, scale))
            });
            if !reused {
                batch = new_batch();
            }
        }
        if !reused {
            for (origin, text) in lines {
                let metrics = self
                    .canvas
                    .measure_text(origin.x, origin.y, text, paint)
                    .map_err(|error| format!("text shaping failed: {error}"))?;
                if !batch.accept_line(metrics.y, metrics.height(), scale) {
                    return Ok(false);
                }
                for glyph in metrics
                    .glyphs
                    .into_iter()
                    .filter(|glyph| !glyph.c.is_control())
                {
                    if !batch.push(
                        &self.text,
                        glyph.font_id,
                        PositionedGlyph {
                            x: glyph.x,
                            y: glyph.y,
                            glyph_id: glyph.glyph_id,
                        },
                    ) {
                        return Ok(false);
                    }
                }
            }
        }
        let Some(font) = batch.font else {
            return Ok(false);
        };
        if self
            .canvas
            .font_variation_axes(font)
            .map_or(true, |axes| !axes.is_empty())
        {
            return Ok(false);
        }
        let line_count = lines.len() as u64;
        if let Some(label) = label
            && reused
            && label.retain_positions
            && batch.positions.capacity() <= crate::label_glyphs::Cache::MAX_POSITIONED_GLYPHS
        {
            self.canvas
                .fill_glyph_run(font, &[], batch.positions.iter().cloned(), paint)
                .map_err(|error| format!("text drawing failed: {error}"))?;
            self.text.label_glyphs.borrow_mut().remember_positioned(
                label.label,
                label.placement,
                vec![crate::label_glyphs::FontRun {
                    font,
                    glyphs: 0..batch.positions.len(),
                }],
                batch.positions,
                line_count,
            );
        } else {
            self.canvas
                .fill_glyph_run(font, &[], batch.positions, paint)
                .map_err(|error| format!("text drawing failed: {error}"))?;
        }
        self.record_label_batch(line_count);
        Ok(true)
    }

    fn draw_unicode_label_batch(
        &mut self,
        lines: &[(Point, &str)],
        paint: &Paint,
        label: LabelBatchContext<'_>,
        scale: f32,
        visible_only: bool,
    ) -> Result<bool, String> {
        use crate::glyph_masks::{OutlineBatch, OutlineRuns};
        use crate::label_glyphs::{BatchScratch, FontRun, UnicodeLabel, UnicodePaint};

        // A batch can seed glyphs for the whole document before retention
        // keeps only the visible runs. Bound this staging area independently
        // of the smaller cache for recurring placements.
        const MAX_BATCH_GLYPHS: usize = 64 * 1024;
        let mut offscreen = OffscreenGlyphs {
            scale,
            translation_y: 0.0,
            height: self.canvas.height() as f32,
            size_range: crate::text::atlas_size_range(paint.font_size(), scale),
            entries: [None; 128],
        };
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let mut atlas_phases = crate::label_glyphs::KnownAtlasPhases::<128>::default();
        let BatchScratch {
            mut runs,
            mut visible_runs,
            mut positions,
        } = std::mem::take(&mut self.label_batch_scratch);
        let (mut batch, priming) = {
            let mut cache = self.text.label_glyphs.borrow_mut();
            let mut shaping = UnicodePaint::new(
                &self.text,
                paint,
                scale,
                offscreen.height,
                label.label,
                !visible_only,
            );
            cache.select(
                label.label.font,
                shaping.measurement_scale,
                label.placement.registered_fonts,
            );
            let indexed_words = shaping.select_word_index(&mut cache, label.label);
            let retained = cache.take_unicode(label.label, label.placement, indexed_words);
            #[cfg(not(xrack_femtovg_glyph_samples))]
            let merge_rows = retained.is_none();
            let mut batch = if let Some(batch) = retained {
                batch
            } else {
                let Some(batch) = UnicodeLabel::measure(
                    &mut cache,
                    label.label,
                    label.placement,
                    lines,
                    &shaping,
                )?
                else {
                    return Ok(false);
                };
                batch
            };
            let mut priming = false;
            #[cfg(not(xrack_femtovg_glyph_samples))]
            let mut previous_rows: Option<[f32; 2]> = None;
            for (slot, (origin, value)) in batch.lines.iter_mut().zip(lines) {
                #[cfg(not(xrack_femtovg_glyph_samples))]
                if shaping.seed_offscreen
                    && let Some(seeds) = slot.take_offscreen_seeds(&shaping, origin.y)
                {
                    // Submit missing atlas glyphs in source order. Their ink
                    // remains outside the target at this exact placement.
                    let mut font = None;
                    let mut start = positions.len();
                    for (next_font, glyph) in seeds.positions(origin.y, shaping.measurement_scale) {
                        if offscreen.classify(&self.text, next_font, &glyph)
                            == GlyphVisibility::RepeatedOffscreen
                        {
                            continue;
                        }
                        if font != Some(next_font) {
                            if let Some(font) = font {
                                runs.push(FontRun {
                                    font,
                                    glyphs: start..positions.len(),
                                });
                            }
                            font = Some(next_font);
                            start = positions.len();
                        }
                        positions.push(glyph);
                    }
                    if let Some(font) = font {
                        runs.push(FontRun {
                            font,
                            glyphs: start..positions.len(),
                        });
                        priming = true;
                        previous_rows = None;
                    }
                    if positions.capacity() > MAX_BATCH_GLYPHS
                        || runs.capacity() > crate::label_glyphs::Cache::MAX_POSITIONED_RUNS
                    {
                        return Ok(false);
                    }
                    continue;
                }
                if slot.still_offscreen(&shaping, origin.y) {
                    continue;
                }
                let Some(line) = slot.shape(&mut cache, &shaping, value)? else {
                    return Ok(false);
                };
                let prime_line = shaping.seed_offscreen && !line.atlas_ready_at(origin.x);
                priming |= prime_line;
                let outside = line.offscreen(origin.y, scale, offscreen.height);
                if !prime_line && outside {
                    continue;
                }
                #[cfg(not(xrack_femtovg_glyph_samples))]
                let rows = if merge_rows {
                    line.ink_rows(origin.y, scale)
                } else {
                    None
                };
                #[cfg(not(xrack_femtovg_glyph_samples))]
                let line_start = positions.len();
                let mut glyphs = line
                    .place(*origin, shaping.measurement_scale, |font, glyph, x| {
                        #[cfg(not(xrack_femtovg_glyph_samples))]
                        {
                            // Outside lines only need positions for phases that
                            // still have to seed the stock FemtoVG atlas.
                            !outside
                                || !atlas_phases.contains(font, glyph, x, scale, || {
                                    cache.known_atlas_phases(font, glyph)
                                })
                        }
                        #[cfg(xrack_femtovg_glyph_samples)]
                        {
                            let _ = (font, glyph, x);
                            true
                        }
                    })
                    .peekable();
                // Keep FemtoVG's visual glyph order and per-line font boundaries.
                while let Some(first) = glyphs.next() {
                    let font = first.font;
                    let start = positions.len();
                    let mut visible = false;
                    let mut append = |position: PositionedGlyph| {
                        match offscreen.classify(&self.text, font, &position) {
                            GlyphVisibility::PossiblyVisible => visible = true,
                            GlyphVisibility::FirstOffscreen if prime_line => {}
                            _ => return,
                        }
                        positions.push(position);
                    };
                    append(first.position);
                    while let Some(next) = glyphs.next_if(|next| {
                        #[cfg(not(xrack_femtovg_glyph_samples))]
                        {
                            !next.starts_run
                        }
                        #[cfg(xrack_femtovg_glyph_samples)]
                        {
                            next.font == font
                        }
                    }) {
                        append(next.position);
                    }
                    if start < positions.len() {
                        let glyphs = start..positions.len();
                        if visible && label.retain_positions {
                            visible_runs.push(FontRun {
                                font,
                                glyphs: glyphs.clone(),
                            });
                        }
                        // Stock FemtoVG groups masks by atlas page within a run.
                        // Combining disjoint rows keeps their paint equivalent
                        // and leaves the glyphs in their original atlas order.
                        #[cfg(not(xrack_femtovg_glyph_samples))]
                        let merged = if start == line_start
                            && let Some(([top, bottom], [old_top, old_bottom])) =
                                rows.zip(previous_rows)
                            && (top >= old_bottom || old_top >= bottom)
                            && let Some(previous) = runs.last_mut()
                            && previous.font == font
                            && previous.glyphs.end == start
                        {
                            previous.glyphs.end = glyphs.end;
                            previous_rows = Some([top.min(old_top), bottom.max(old_bottom)]);
                            true
                        } else {
                            previous_rows = rows;
                            false
                        };
                        #[cfg(xrack_femtovg_glyph_samples)]
                        let merged = false;
                        if !merged {
                            runs.push(FontRun { font, glyphs });
                        }
                    }
                    if positions.capacity() > MAX_BATCH_GLYPHS
                        || runs.capacity() > crate::label_glyphs::Cache::MAX_POSITIONED_RUNS
                    {
                        return Ok(false);
                    }
                }
            }
            (batch, priming)
        };
        // Large glyph repertoires populate the atlas as their glyphs become
        // visible. Other documents can seed unseen runs in the original order.
        let moved_vertically = batch.update_placement(label.placement);
        let masked = if !visible_only
            && !cfg!(xrack_femtovg_glyph_samples)
            && moved_vertically
            && batch.allow_outline_masks
            && !priming
            && self.can_cache_on_main_canvas()
        {
            match self.glyph_masks.draw_outline_runs(
                &mut self.canvas,
                &self.text,
                OutlineBatch {
                    runs: &runs,
                    glyphs: &positions,
                    paint,
                    scale,
                    allow_partial: batch.drew_outline_masks,
                },
            )? {
                OutlineRuns::Drawn => {
                    batch.drew_outline_masks = true;
                    true
                }
                OutlineRuns::Fallback => false,
                OutlineRuns::Unsupported => {
                    batch.allow_outline_masks = false;
                    false
                }
            }
        } else {
            false
        };
        if !masked {
            Self::submit_label_runs(&mut self.canvas, &runs, &positions, paint)?;
        }
        let line_count = lines.len() as u64;
        let mut cache = self.text.label_glyphs.borrow_mut();
        if priming {
            cache.remember_atlas_glyphs(&runs, &positions, scale);
        }
        if priming {
            for (line, (origin, _)) in batch.lines.iter().zip(lines) {
                line.remember_atlas_origin(origin.x);
            }
        }
        cache.remember_unicode(batch);
        if label.retain_positions {
            cache.remember_positioned(
                label.label,
                label.placement,
                if priming {
                    std::mem::take(&mut visible_runs)
                } else {
                    std::mem::take(&mut runs)
                },
                std::mem::take(&mut positions),
                line_count,
            );
        }
        drop(cache);
        // Retained batches own the vectors they keep. Recycle the other
        // allocations after recording, including after a native atlas fallback.
        runs.clear();
        visible_runs.clear();
        positions.clear();
        self.label_batch_scratch = BatchScratch {
            runs,
            visible_runs,
            positions,
        };
        self.record_label_batch(line_count);
        Ok(true)
    }

    fn submit_label_runs(
        canvas: &mut crate::Canvas<crate::WGPURenderer>,
        runs: &[crate::label_glyphs::FontRun],
        glyphs: &[PositionedGlyph],
        paint: &Paint,
    ) -> Result<(), String> {
        // New line boundaries can still refer entirely to existing atlas tiles.
        // On a miss, the atlas records nothing and the normal draws seed it.
        #[cfg(xrack_femtovg_glyph_samples)]
        if canvas.fill_cached_glyph_runs(
            runs.iter()
                .map(|run| (run.font, &glyphs[run.glyphs.clone()])),
            paint,
        ) {
            return Ok(());
        }
        for run in runs {
            canvas
                .fill_glyph_run(
                    run.font,
                    &[],
                    glyphs[run.glyphs.clone()].iter().cloned(),
                    paint,
                )
                .map_err(|error| format!("text drawing failed: {error}"))?;
        }
        Ok(())
    }

    fn record_label_batch(&mut self, lines: u64) {
        #[cfg(not(xrack_femtovg_glyph_samples))]
        if lines >= 16 {
            self.glyph_masks.request_atlas_pipeline_warmup();
        }
        xrack_runtime::update_profile::glyph_batch();
        self.display_batch_stats.glyph_batches += 1;
        self.display_batch_stats.text_runs += lines;
    }
}

/// Batches emitted by the renderer's display lists, accumulated since creation.
#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayBatchStats {
    pub image_batches: u64,
    pub images: u64,
    pub glyph_batches: u64,
    pub text_runs: u64,
    pub clipped_text_runs: u64,
}
