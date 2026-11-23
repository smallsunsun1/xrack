use crate::{
    CanvasTransform, Renderer,
    image::{ImageQuad, QuadBounds},
    layer_compositor::{MAX_IMAGE_INSTANCES, MIN_IMAGE_INSTANCES},
    window_damage::DamageRect,
};
use femtovg::{ImageId, Quad};
use std::rc::Rc;
use xrack_layout::{ImagePresentation, LayoutNode, Rect};
use xrack_runtime::{Node, SharedNodeIdentity};

const MAX_PLACEMENT_DEPTH: usize = 8;
pub(super) const MAX_SCROLL_BATCHES: usize = 8;

pub(super) enum WindowImageBatch {
    Visible {
        atlas: ImageId,
        quads: Vec<Quad>,
    },
    Scrolling {
        atlas: ImageId,
        quads: Rc<[Quad]>,
        slot: usize,
        image_count: usize,
        motion: ImageMotion,
    },
}

#[derive(Clone, Copy)]
pub(super) struct ImageMotion {
    pub(super) offset: [f32; 2],
    pub(super) clip: DamageRect,
}

impl WindowImageBatch {
    pub(super) fn parts(&self) -> (ImageId, &[Quad], usize, Option<ImageMotion>) {
        match self {
            Self::Visible { atlas, quads } => (*atlas, quads, quads.len(), None),
            Self::Scrolling {
                atlas,
                quads,
                image_count,
                motion,
                ..
            } => (*atlas, quads, *image_count, Some(*motion)),
        }
    }
}

struct Placement {
    depth: usize,
    index: usize,
    bounds: Rect,
    painted: bool,
}

#[derive(Default)]
pub(super) struct ScrollImages {
    slots: Vec<ScrollSlot>,
    frame: u64,
    layered_candidate: Option<(SharedNodeIdentity, bool)>,
}

struct ScrollSlot {
    path: Vec<usize>,
    last_frame: u64,
    admission: Option<Admission>,
    retained: Option<RetainedImages>,
}

impl ScrollImages {
    pub(super) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    fn slot(&mut self, path: &[usize]) -> Option<usize> {
        if let Some(index) = self.slots.iter().position(|slot| slot.path == path) {
            self.slots[index].last_frame = self.frame;
            return Some(index);
        }

        let slot = ScrollSlot {
            path: path.to_vec(),
            last_frame: self.frame,
            admission: None,
            retained: None,
        };
        if self.slots.len() < MAX_SCROLL_BATCHES {
            let index = self.slots.len();
            self.slots.push(slot);
            return Some(index);
        }

        // Draws planned for this frame still refer to their slot's GPU
        // buffer. Reuse only a slot that is not part of the current plan.
        let (index, oldest) = self
            .slots
            .iter_mut()
            .enumerate()
            .filter(|(_, slot)| slot.last_frame != self.frame)
            .min_by_key(|(_, slot)| slot.last_frame)?;
        *oldest = slot;
        Some(index)
    }

    pub(super) fn has_layered_viewport(&mut self, root: &Node) -> bool {
        fn contains_viewport(node: &Node, depth: usize) -> bool {
            match node.kind {
                "Flickable" | "ScrollView" => {
                    (MIN_IMAGE_INSTANCES..=MAX_IMAGE_INSTANCES).contains(&node.children.len())
                }
                "Rectangle" | "HorizontalLayout" | "VerticalLayout" | "GridLayout" if depth > 0 => {
                    node.children
                        .iter()
                        .any(|child| contains_viewport(child, depth - 1))
                }
                _ => false,
            }
        }

        if let Some((identity, candidate)) = &self.layered_candidate
            && identity.matches(root)
        {
            return *candidate;
        }
        // A sole viewport still uses the direct window image path.
        let candidate = !matches!(root.children.as_slice(), [node] if matches!(node.kind, "ScrollView" | "Flickable"))
            && root
                .children
                .iter()
                .any(|child| contains_viewport(child, MAX_PLACEMENT_DEPTH));
        self.layered_candidate = Some((root.snapshot_identity(), candidate));
        candidate
    }
}

struct Admission {
    view: SharedNodeIdentity,
    extent: [f32; 2],
    scale: f32,
    rejected: bool,
}

struct RetainedImages {
    view: SharedNodeIdentity,
    scale: f32,
    atlas_key: Rc<()>,
    quads: Rc<[Quad]>,
    image_count: usize,
    placements: Vec<Placement>,
}

fn fills_visible_part(layout: &LayoutNode, clip: Rect) -> bool {
    let expected = layout.bounds.intersect(clip);
    let actual = layout.bounds.intersect(layout.clip);
    actual == expected
        || (expected.width <= 0.0 || expected.height <= 0.0)
            && (actual.width <= 0.0 || actual.height <= 0.0)
}

impl RetainedImages {
    fn offset(&self, view: &Node, layout: &LayoutNode, clip: Rect, scale: f32) -> Option<[f32; 2]> {
        if !self.view.matches(view) || self.scale != scale {
            return None;
        }
        let first = self.placements.first()?;
        let anchor = layout.child_at(first.index)?.bounds;
        let [dx, dy] = [anchor.x - first.bounds.x, anchor.y - first.bounds.y];
        let offset = [dx * scale, dy * scale];
        if offset
            .into_iter()
            .any(|p| !p.is_finite() || p.fract() != 0.0)
        {
            return None;
        }
        // Layout can be supplied by the caller. Check placement and viewport
        // clipping before using the geometry recorded for this Node snapshot.
        // Capture stores placements in depth-first order. Reuse the most
        // recent parent at each depth while checking the supplied layout.
        let mut parents = [layout; MAX_PLACEMENT_DEPTH + 1];
        for entry in &self.placements {
            let item = parents[entry.depth].child_at(entry.index)?;
            let bounds = Rect::new(
                entry.bounds.x + dx,
                entry.bounds.y + dy,
                entry.bounds.width,
                entry.bounds.height,
            );
            if !item.visible
                || item.bounds != bounds
                || !item.local_transform().is_identity()
                || item.has_ancestor_clip_chain()
                || item.allows_overflow()
                || item.table_pane_clip().is_some()
                || item.child_clip_shape().is_some()
                || (entry.painted && !fills_visible_part(item, clip))
            {
                return None;
            }
            parents[entry.depth + 1] = item;
        }
        Some(offset)
    }
}

#[derive(Default)]
struct Capture {
    atlas: Option<ImageId>,
    quads: Vec<Quad>,
    image_count: usize,
    placements: Vec<Placement>,
}

impl Capture {
    fn collect(
        &mut self,
        renderer: &mut Renderer,
        node: &Node,
        layout: &LayoutNode,
        transform: CanvasTransform,
        clip: Rect,
        depth: usize,
    ) -> Result<bool, String> {
        let in_layout = xrack_layout::is_layout(node.kind);
        if depth == MAX_PLACEMENT_DEPTH || layout.children.len() != node.children.len() {
            return Ok(false);
        }
        for (index, child, item) in layout.children_in_paint_order(node) {
            if !item.visible
                || item.has_ancestor_clip_chain()
                || item.child_clip_shape().is_some()
                || self.placements.len() == MAX_IMAGE_INSTANCES
            {
                return Ok(false);
            }
            let image = child.kind == "Image";
            let background = child.color("background").filter(|_| {
                crate::rectangle::plain_surface(child, item)
                    && child.opacity() == 1.0
                    && item.local_transform().is_identity()
                    && item.table_pane_clip().is_none()
                    && xrack_layout::corner_radii(child, item.bounds) == [0.0; 4]
            });
            if !image && background.is_none() && !Renderer::is_unpainted_group(child, item) {
                return Ok(false);
            }
            let paint_background = background.filter(|fill| fill.0 & 255 != 0);
            self.placements.push(Placement {
                depth,
                index,
                bounds: item.bounds,
                painted: image || paint_background.is_some(),
            });
            let complete = if image {
                self.image(renderer, child, item, in_layout, transform, clip)?
            } else {
                if item.allows_overflow() {
                    return Ok(false);
                }
                if let Some(tint) = paint_background {
                    if !fills_visible_part(item, clip)
                        || !crate::image::quad_edges_are_pixel_aligned(item.bounds, transform)
                    {
                        return Ok(false);
                    }
                    let Some((atlas, quad)) = renderer.images.batch_solid_quad(
                        &renderer.device,
                        &mut renderer.canvas,
                        item.bounds,
                        tint,
                    )?
                    else {
                        return Ok(false);
                    };
                    if !self.push(renderer, atlas, quad) {
                        return Ok(false);
                    }
                }
                self.collect(renderer, child, item, transform, clip, depth + 1)?
            };
            if !complete {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn image(
        &mut self,
        renderer: &mut Renderer,
        node: &Node,
        layout: &LayoutNode,
        in_layout: bool,
        transform: CanvasTransform,
        clip: Rect,
    ) -> Result<bool, String> {
        if !Renderer::is_plain_image(node, layout)
            || !fills_visible_part(layout, clip)
            || !crate::image::quad_edges_are_pixel_aligned(layout.bounds, transform)
        {
            return Ok(false);
        }
        let Some(asset) = node.image_asset("source") else {
            return Ok(false);
        };
        let presentation = ImagePresentation::for_whole_node(node, asset.size(), in_layout);
        let Some((image, quad)) = renderer.images.batch_quad(
            &renderer.device,
            &mut renderer.canvas,
            ImageQuad {
                asset: &asset,
                bounds: layout.bounds,
                clip: layout.bounds,
                coverage: QuadBounds::Aligned(None),
                presentation,
                transform,
            },
        )?
        else {
            return Ok(false);
        };
        let added = self.push(renderer, image, quad);
        self.image_count += usize::from(added);
        Ok(added)
    }

    fn push(&mut self, renderer: &Renderer, image: ImageId, quad: Quad) -> bool {
        if self.quads.len() == MAX_IMAGE_INSTANCES
            || self.atlas.map_or_else(
                || renderer.images.source_atlas(image).is_none(),
                |atlas| atlas != image,
            )
        {
            return false;
        }
        self.atlas = Some(image);
        self.quads.push(quad);
        true
    }
}

impl Renderer {
    pub(super) fn retained_scroll_image_batch(
        &mut self,
        view: &Node,
        layout: &LayoutNode,
        transform: CanvasTransform,
        clip: Rect,
        scale: f32,
        path: &[usize],
    ) -> Result<Option<WindowImageBatch>, String> {
        if !self.device.features().contains(wgpu::Features::IMMEDIATES)
            || !crate::has_device_scale(transform, scale)
            || clip.width <= 0.0
            || clip.height <= 0.0
        {
            return Ok(None);
        }
        let Some(slot) = self.scroll_images.slot(path) else {
            return Ok(None);
        };
        let x = (clip.x * scale).round();
        let y = (clip.y * scale).round();
        let right = ((clip.x + clip.width) * scale).round();
        let bottom = ((clip.y + clip.height) * scale).round();
        let clip_rect = DamageRect {
            x: x as u32,
            y: y as u32,
            width: (right.max(0.0) - x.max(0.0)) as u32,
            height: (bottom.max(0.0) - y.max(0.0)) as u32,
        };
        if let Some(cached) = &self.scroll_images.slots[slot].retained
            && let Some(offset) = cached.offset(view, layout, clip, scale)
            && let Some(atlas) = self.images.retain_source_atlas(&cached.atlas_key)
        {
            return Ok(Some(WindowImageBatch::Scrolling {
                atlas,
                quads: cached.quads.clone(),
                slot,
                image_count: cached.image_count,
                motion: ImageMotion {
                    offset,
                    clip: clip_rect,
                },
            }));
        }
        let extent = [layout.bounds.width, layout.bounds.height];
        let admission = &mut self.scroll_images.slots[slot].admission;
        if admission.as_ref().is_none_or(|last| {
            !last.view.matches(view) || last.extent != extent || last.scale != scale
        }) {
            // Let one frame use the visible batch first. A view that keeps
            // replacing image sources should never upload the offscreen tiles.
            *admission = Some(Admission {
                view: view.snapshot_identity(),
                extent,
                scale,
                rejected: false,
            });
            return Ok(None);
        }
        if admission.as_ref().is_some_and(|state| state.rejected) {
            return Ok(None);
        }
        let mut capture = Capture::default();
        let complete = capture.collect(self, view, layout, transform, clip, 0)?;
        self.images.flush_batch_uploads(&mut self.canvas)?;
        let Some(atlas) = capture
            .atlas
            .filter(|_| complete && capture.image_count >= MIN_IMAGE_INSTANCES)
        else {
            if capture.quads.len() >= MIN_IMAGE_INSTANCES
                && let Some(admission) = &mut self.scroll_images.slots[slot].admission
            {
                // Bound the work of scanning a stable view that cannot fit
                // into one atlas or has an incompatible item late in the batch.
                admission.rejected = true;
            }
            return Ok(None);
        };
        let Some(atlas_key) = self.images.source_atlas_key(atlas) else {
            return Ok(None);
        };
        for quad in &mut capture.quads {
            quad.x0 *= scale;
            quad.y0 *= scale;
            quad.x1 *= scale;
            quad.y1 *= scale;
        }
        let quads: Rc<[_]> = capture.quads.into();
        self.scroll_images.slots[slot].retained = Some(RetainedImages {
            view: view.snapshot_identity(),
            scale,
            atlas_key,
            quads: quads.clone(),
            image_count: capture.image_count,
            placements: capture.placements,
        });
        Ok(Some(WindowImageBatch::Scrolling {
            atlas,
            quads,
            slot,
            image_count: capture.image_count,
            motion: ImageMotion {
                offset: [0.0; 2],
                clip: clip_rect,
            },
        }))
    }
}
