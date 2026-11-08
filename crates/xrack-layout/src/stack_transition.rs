use crate::LayoutNode;
use std::{
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_runtime::Node;

const MAX_INTERRUPTED_LAYERS: usize = 32;

struct PageLayer {
    page: Rc<(Node, LayoutNode)>,
    offset: f32,
    visible: Range<f32>,
}

fn visible_slice(offset: f32, slice: Range<f32>) -> Option<Range<f32>> {
    let start = (slice.start + offset).max(0.0);
    let end = (slice.end + offset).min(1.0);
    (start < end).then_some(start..end)
}

impl PageLayer {
    fn capture(node: &Node, layout: &LayoutNode, offset: f32) -> Option<Self> {
        Some(Self {
            visible: visible_slice(offset, 0.0..1.0)?,
            page: Rc::new((node.clone(), layout.clone())),
            offset,
        })
    }

    fn moved(&self, delta: f32) -> Option<(f32, Range<f32>)> {
        Some((
            self.offset + delta,
            visible_slice(delta, self.visible.clone())?,
        ))
    }
}

/// A stack's paint-only motion. Offsets and clip ranges are fractions of its width.
pub struct StackSlide {
    start: Instant,
    duration: Duration,
    direction: f32,
    progress: f32,
    previous: Vec<PageLayer>,
}

impl StackSlide {
    pub fn incoming_offset(&self) -> f32 {
        self.direction * (1.0 - self.progress)
    }

    pub fn outgoing(&self) -> impl Iterator<Item = (&Node, &LayoutNode, f32, Range<f32>)> {
        self.layers().map(|(layer, offset, clip)| {
            let (node, layout) = &*layer.page;
            (node, layout, offset, clip)
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
            slide.progress = 1.0 - remaining.max(0.0).powi(3);
            elapsed < slide.duration
        });
    }

    pub fn update(
        &mut self,
        previous: (&Node, &LayoutNode),
        next: (&Node, &LayoutNode),
        now: Instant,
    ) {
        let mut prior = std::mem::take(&mut self.slides);
        self.collect(previous, next, now, &mut Vec::new(), &mut prior);
    }

    fn collect(
        &mut self,
        (old, old_layout): (&Node, &LayoutNode),
        (new, new_layout): (&Node, &LayoutNode),
        now: Instant,
        path: &mut Vec<usize>,
        prior: &mut BTreeMap<Vec<usize>, StackSlide>,
    ) {
        if !old_layout.visible
            || !new_layout.visible
            || (old.kind, old.id, old.row_key) != (new.kind, new.id, new.row_key)
        {
            return;
        }
        if let ("StackView", Some(old_stack), Some(new_stack)) = (new.kind, &old.stack, &new.stack)
        {
            let existing = prior.remove(path);
            let duration = new.int("transition-duration").unwrap_or(0).max(0) as u64;
            let bounds = new_layout.bounds;
            let viewport = new_layout.clip.intersect(bounds);
            let can_slide = duration > 0
                && old_layout.bounds == bounds
                && old_layout.clip == new_layout.clip
                && viewport.width > 0.0
                && viewport.height > 0.0;
            if old_stack.current() != new_stack.current() {
                if can_slide
                    && let Some(index) = old.stack_page_index()
                    && let Some(page_layout) = old_layout.children.get(index)
                {
                    let backward = new_stack.depth() < old_stack.depth()
                        && old_stack.routes().starts_with(new_stack.routes());
                    let previous = (&old.children[index], page_layout);
                    self.slides.insert(
                        path.clone(),
                        StackSlide {
                            start: now,
                            duration: Duration::from_millis(duration),
                            direction: if backward { -1.0 } else { 1.0 },
                            progress: 0.0,
                            previous: outgoing_pages(previous, existing.as_ref()),
                        },
                    );
                }
                return;
            }
            if can_slide && let Some(slide) = existing {
                self.slides.insert(path.clone(), slide);
            }
        }
        let before = old.children.iter().zip(&old_layout.children);
        let after = new.children.iter().zip(&new_layout.children);
        for (index, (before, after)) in before.zip(after).enumerate() {
            path.push(index);
            self.collect(before, after, now, path, prior);
            path.pop();
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
            })
            .collect();
        layers.extend(PageLayer::capture(node, layout, previous.incoming_offset()));
        if layers.len() <= MAX_INTERRUPTED_LAYERS {
            return layers;
        }
    }
    PageLayer::capture(node, layout, 0.0).into_iter().collect()
}
