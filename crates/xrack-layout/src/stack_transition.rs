use crate::{Interaction, LayoutNode, Point, Rect};
use std::{
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_runtime::{LayoutDirection, Node, Orientation, PageIdentity, group_index};

const MAX_INTERRUPTED_LAYERS: usize = 32;

struct PageLayer {
    page: Rc<(Node, LayoutNode)>,
    offset: f32,
    visible: Range<f32>,
    first_frame_direct: bool,
}

fn visible_slice(offset: f32, slice: Range<f32>) -> Option<Range<f32>> {
    let start = (slice.start + offset).max(0.0);
    let end = (slice.end + offset).min(1.0);
    (start < end).then_some(start..end)
}

impl PageLayer {
    fn capture(
        node: &Node,
        layout: &LayoutNode,
        offset: f32,
        first_frame_direct: bool,
    ) -> Option<Self> {
        Some(Self {
            visible: visible_slice(offset, 0.0..1.0)?,
            page: Rc::new((node.clone(), layout.clone())),
            offset,
            first_frame_direct,
        })
    }

    fn moved(&self, delta: f32) -> Option<(f32, Range<f32>)> {
        Some((
            self.offset + delta,
            visible_slice(delta, self.visible.clone())?,
        ))
    }
}

/// A stack's paint-only motion. Offsets and clip ranges are fractions of the moving axis.
pub struct StackSlide {
    start: Instant,
    duration: Duration,
    direction: f32,
    orientation: Orientation,
    progress: f32,
    initial_progress: f32,
    previous: Vec<PageLayer>,
}

impl StackSlide {
    pub fn incoming_offset(&self) -> f32 {
        self.direction * (1.0 - self.progress)
    }

    #[doc(hidden)]
    pub fn displacement(&self, bounds: Rect, offset: f32) -> Point {
        match self.orientation {
            Orientation::Horizontal => Point {
                x: bounds.width * offset,
                y: 0.0,
            },
            Orientation::Vertical => Point {
                x: 0.0,
                y: bounds.height * offset,
            },
        }
    }

    #[doc(hidden)]
    pub fn slice(&self, bounds: Rect, range: Range<f32>) -> Rect {
        let start = self.displacement(bounds, range.start);
        let end = self.displacement(bounds, range.end);
        match self.orientation {
            Orientation::Horizontal => {
                Rect::new(bounds.x + start.x, bounds.y, end.x - start.x, bounds.height)
            }
            Orientation::Vertical => {
                Rect::new(bounds.x, bounds.y + start.y, bounds.width, end.y - start.y)
            }
        }
    }

    pub fn outgoing(&self) -> impl Iterator<Item = (&Node, &LayoutNode, f32, Range<f32>)> {
        self.outgoing_layers().map(|(page, offset, clip, _)| {
            let (node, layout) = &**page;
            (node, layout, offset, clip)
        })
    }

    /// The same snapshot is shared by successive frames and interrupted slides.
    #[doc(hidden)]
    pub fn outgoing_layers(
        &self,
    ) -> impl Iterator<Item = (&Rc<(Node, LayoutNode)>, f32, Range<f32>, bool)> {
        self.layers().map(|(layer, offset, clip)| {
            let cacheable = !(layer.first_frame_direct && self.progress == 0.0);
            (&layer.page, offset, clip, cacheable)
        })
    }

    fn layers(&self) -> impl Iterator<Item = (&PageLayer, f32, Range<f32>)> {
        let delta = -self.direction * self.progress;
        self.previous.iter().filter_map(move |layer| {
            let (offset, clip) = layer.moved(delta)?;
            Some((layer, offset, clip))
        })
    }
}

/// Keeps outgoing pages separate from the active scene, focus order, and modal stack.
#[derive(Default)]
pub struct StackTransitions {
    slides: BTreeMap<Vec<usize>, StackSlide>,
}

impl StackTransitions {
    pub fn active_paths(&self) -> impl Iterator<Item = &[usize]> {
        self.slides.keys().map(Vec::as_slice)
    }

    pub fn is_active(&self) -> bool {
        !self.slides.is_empty()
    }

    pub fn at(&self, path: &[usize]) -> Option<&StackSlide> {
        self.slides.get(path)
    }

    /// Sample immediately before painting. A replacement transition starts from that sample.
    pub fn advance(&mut self, now: Instant) {
        self.slides.retain(|_, slide| {
            let elapsed = now.saturating_duration_since(slide.start);
            let remaining = 1.0 - elapsed.as_secs_f32() / slide.duration.as_secs_f32();
            slide.progress = 1.0 - (1.0 - slide.initial_progress) * remaining.max(0.0).powi(3);
            elapsed < slide.duration
        });
    }

    pub fn update(
        &mut self,
        previous: (&Node, &LayoutNode),
        next: (&Node, &LayoutNode),
        now: Instant,
    ) {
        self.update_with_input(previous, next, now, None);
    }

    #[doc(hidden)]
    pub fn update_with_input(
        &mut self,
        previous: (&Node, &LayoutNode),
        next: (&Node, &LayoutNode),
        now: Instant,
        input: Option<&Interaction>,
    ) {
        let mut prior = std::mem::take(&mut self.slides);
        self.collect(previous, next, (now, input), &mut Vec::new(), &mut prior);
    }

    fn collect(
        &mut self,
        (old, old_layout): (&Node, &LayoutNode),
        (new, new_layout): (&Node, &LayoutNode),
        (now, input): (Instant, Option<&Interaction>),
        path: &mut Vec<usize>,
        prior: &mut BTreeMap<Vec<usize>, StackSlide>,
    ) {
        if !old_layout.visible
            || !new_layout.visible
            || (old.kind, old.element_id(), old.row_key)
                != (new.kind, new.element_id(), new.row_key)
        {
            return;
        }
        if let Some(change) = PageChange::between(old, new, new_layout) {
            let existing = prior.remove(path);
            let duration = new.int("transition-duration").unwrap_or(0).max(0) as u64;
            let bounds = new_layout.bounds;
            let viewport = new_layout.clip.intersect(bounds);
            let can_slide = duration > 0
                && old_layout.bounds == bounds
                && old_layout.clip == new_layout.clip
                && (new.kind != "SwipeView"
                    || (old.orientation(), old_layout.layout_direction)
                        == (new.orientation(), new_layout.layout_direction))
                && viewport.width > 0.0
                && viewport.height > 0.0;
            if change.changed {
                if can_slide
                    && let Some(direction) = change.direction
                    && let Some(index) = change.from
                    && let Some(previous) = old.child_at(index).zip(old_layout.child_at(index))
                {
                    let offset =
                        input.map_or(0.0, |input| input.swipe_commit_fraction(path, old, new));
                    let progress = (offset * direction).max(0.0);
                    let duration = Duration::from_millis(duration).mul_f32(1.0 - progress);
                    if !duration.is_zero() {
                        self.slides.insert(
                            path.clone(),
                            StackSlide {
                                start: now,
                                duration,
                                direction,
                                orientation: change.orientation,
                                progress,
                                initial_progress: progress,
                                previous: outgoing_pages(previous, existing.as_ref()),
                            },
                        );
                    }
                }
                return;
            }
            if can_slide && let Some(slide) = existing {
                self.slides.insert(path.clone(), slide);
            }
        }
        for (index, child, layout) in new_layout.children_in_scene_order(new) {
            let Some(before) = old.child_at(index).zip(old_layout.child_at(index)) else {
                continue;
            };
            path.push(index);
            self.collect(before, (child, layout), (now, input), path, prior);
            path.pop();
        }
    }
}

struct PageChange {
    from: Option<usize>,
    changed: bool,
    direction: Option<f32>,
    orientation: Orientation,
}

impl PageChange {
    fn between(old: &Node, new: &Node, layout: &LayoutNode) -> Option<Self> {
        match new.kind {
            "StackView" => {
                let (before, after) = old.stack.as_ref().zip(new.stack.as_ref())?;
                let backward =
                    after.depth() < before.depth() && before.routes().starts_with(after.routes());
                Some(Self {
                    from: old.stack_page_index(),
                    changed: before.current() != after.current(),
                    direction: Some(if backward { -1.0 } else { 1.0 }),
                    orientation: Orientation::Horizontal,
                })
            }
            "SwipeView" => {
                let from = group_index(old);
                let to = group_index(new);
                let identity = |node: &Node, slot: Option<usize>| {
                    let slot = slot?;
                    Some(PageIdentity::of(node.child_at(slot)?, slot))
                };
                let orientation = new.orientation();
                let mirrored = orientation == Orientation::Horizontal
                    && layout.layout_direction == LayoutDirection::RightToLeft;
                Some(Self {
                    from,
                    changed: identity(old, from) != identity(new, to),
                    direction: from.zip(to).and_then(|(from, to)| {
                        (from != to).then_some(if (from < to) != mirrored { 1.0 } else { -1.0 })
                    }),
                    orientation,
                })
            }
            _ => None,
        }
    }
}

fn outgoing_pages(
    (node, layout): (&Node, &LayoutNode),
    previous: Option<&StackSlide>,
) -> Vec<PageLayer> {
    if let Some(previous) = previous {
        let mut layers: Vec<_> = previous
            .layers()
            .map(|(layer, offset, visible)| PageLayer {
                page: layer.page.clone(),
                offset,
                visible,
                first_frame_direct: layer.first_frame_direct && previous.progress == 0.0,
            })
            .collect();
        // The incoming page was drawn directly in the presented frame.
        // Keep its subpixel coverage when an interruption replays that frame.
        layers.extend(PageLayer::capture(
            node,
            layout,
            previous.incoming_offset(),
            true,
        ));
        if layers.len() <= MAX_INTERRUPTED_LAYERS {
            return layers;
        }
    }
    PageLayer::capture(node, layout, 0.0, false)
        .into_iter()
        .collect()
}
