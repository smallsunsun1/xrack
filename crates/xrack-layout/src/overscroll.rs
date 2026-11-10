use crate::{LayoutNode, scroll_gesture::SCROLL_FRAME_INTERVAL};
use std::time::{Duration, Instant};
use xrack_runtime::{
    FlickBoundsBehavior, FlickBoundsMovement, Node, Orientation, Value, ViewportHandle,
};

const AXES: [Orientation; 2] = [Orientation::Horizontal, Orientation::Vertical];

/// Visual displacement only. Logical scroll ranges stay in ScrollLayout.
pub type OverscrollFrame = [Option<(ViewportHandle, f32)>; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ElasticOptions {
    pub behavior: FlickBoundsBehavior,
    move_contents: bool,
    duration: Duration,
}

impl ElasticOptions {
    pub fn from_node(node: &Node) -> Option<Self> {
        if node.kind != "Flickable" {
            return None;
        }
        let Some(Value::FlickBoundsBehavior(behavior)) = node.properties.get("bounds-behavior")
        else {
            return None;
        };
        if *behavior == FlickBoundsBehavior::StopAtBounds {
            return None;
        }
        let move_contents = !matches!(
            node.properties.get("bounds-movement"),
            Some(Value::FlickBoundsMovement(
                FlickBoundsMovement::StopAtBounds
            ))
        );
        let duration = Duration::from_millis(
            node.int("rebound-duration").unwrap_or(180).clamp(0, 2000) as u64,
        );
        Some(Self {
            behavior: *behavior,
            move_contents,
            duration,
        })
    }

    pub fn moves_contents(self) -> bool {
        self.move_contents
    }
}

#[derive(Clone, Debug)]
struct Rebound {
    start: Instant,
    next: Instant,
    initial: f32,
}

#[derive(Clone, Debug)]
struct ElasticAxis {
    owner: ViewportHandle,
    raw: f64,
    value: f32,
    limit: f64,
    options: ElasticOptions,
    rebound: Option<Rebound>,
}

impl ElasticAxis {
    fn resample(&mut self, axis: Orientation) {
        // A continuous resistance curve with a finite displacement limit.
        self.value = (self.limit * (self.raw / (2.0 * self.limit + self.raw.abs()))) as f32;
        self.owner.publish_overshoot(axis, self.value, false);
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Overscroll {
    axes: [Option<ElasticAxis>; 2],
}

fn index(axis: Orientation) -> usize {
    usize::from(axis == Orientation::Vertical)
}

impl Overscroll {
    pub fn has_offset(&self) -> bool {
        self.axes.iter().any(Option::is_some)
    }
    pub fn frame(&self) -> OverscrollFrame {
        self.axes.each_ref().map(|lane| {
            lane.as_ref()
                .filter(|lane| lane.options.move_contents)
                .map(|lane| (lane.owner.clone(), lane.value))
        })
    }

    pub fn visual_offset(&self, handle: &ViewportHandle, axis: Orientation) -> f32 {
        self.axes[index(axis)]
            .as_ref()
            .filter(|lane| &lane.owner == handle)
            .map_or(0.0, |lane| lane.value)
    }

    pub fn owns_axis(&self, axis: Orientation) -> bool {
        self.axes[index(axis)].is_some()
    }

    pub fn is_rebounding(&self, axis: Orientation) -> bool {
        self.axes[index(axis)]
            .as_ref()
            .is_some_and(|lane| lane.rebound.is_some())
    }

    pub fn resume_drag(&mut self, mut accepts: impl FnMut(Orientation, &ViewportHandle) -> bool) {
        for (axis, slot) in AXES.into_iter().zip(&mut self.axes) {
            let Some(lane) = slot else {
                continue;
            };
            if !accepts(axis, &lane.owner) {
                lane.owner.publish_overshoot(axis, 0.0, false);
                *slot = None;
            } else if lane.rebound.take().is_some() {
                // Invert the resistance curve at the current animation sample.
                let value = f64::from(lane.value);
                lane.raw = 2.0 * lane.limit * value / (lane.limit - value.abs());
                lane.owner.publish_overshoot(axis, lane.value, false);
            }
        }
    }

    pub fn unwind(&mut self, axis: Orientation, delta: f64) -> (f64, Option<ViewportHandle>) {
        let slot = &mut self.axes[index(axis)];
        let Some(lane) = slot.as_mut().filter(|lane| {
            lane.rebound.is_none() && delta != 0.0 && lane.raw.signum() != delta.signum()
        }) else {
            return (delta, None);
        };
        let consumed = delta.signum() * delta.abs().min(lane.raw.abs());
        let owner = lane.owner.clone();
        lane.raw += consumed;
        if lane.raw == 0.0 {
            owner.publish_overshoot(axis, 0.0, false);
            *slot = None;
        } else {
            lane.resample(axis);
        }
        (delta - consumed, Some(owner))
    }

    pub fn pull(&mut self, item: &LayoutNode, axis: Orientation, delta: f64) -> bool {
        let (Some(owner), Some(options)) = (&item.viewport_handle, item.elastic) else {
            return false;
        };
        let bounds = item.scroll_viewport(axis);
        let page = if axis == Orientation::Horizontal {
            bounds.width
        } else {
            bounds.height
        };
        let limit = (f64::from(page) * 0.33).min(96.0);
        if !delta.is_finite() || delta == 0.0 || limit <= 0.0 {
            return false;
        }
        let slot = &mut self.axes[index(axis)];
        if slot.as_ref().is_some_and(|lane| &lane.owner != owner) {
            slot.take()
                .unwrap()
                .owner
                .publish_overshoot(axis, 0.0, false);
        }
        let lane = slot.get_or_insert_with(|| ElasticAxis {
            owner: owner.clone(),
            raw: 0.0,
            value: 0.0,
            limit,
            options,
            rebound: None,
        });
        lane.limit = limit;
        lane.options = options;
        lane.raw = (lane.raw + delta).clamp(-limit * 32.0, limit * 32.0);
        lane.resample(axis);
        true
    }

    pub fn return_to_bounds(&mut self, now: Instant, target: Option<&ViewportHandle>) -> bool {
        let mut changed = false;
        for (axis, slot) in AXES.into_iter().zip(&mut self.axes) {
            let Some(lane) = slot.as_mut().filter(|lane| {
                lane.rebound.is_none() && target.is_none_or(|target| target == &lane.owner)
            }) else {
                continue;
            };
            changed = true;
            if lane.options.duration.is_zero() {
                lane.owner.publish_overshoot(axis, 0.0, false);
                *slot = None;
            } else {
                lane.rebound = Some(Rebound {
                    start: now,
                    next: now + SCROLL_FRAME_INTERVAL,
                    initial: lane.value,
                });
                lane.owner.publish_overshoot(axis, lane.value, true);
            }
        }
        changed
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.axes
            .iter()
            .flatten()
            .filter_map(|lane| lane.rebound.as_ref().map(|rebound| rebound.next))
            .min()
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for (axis, slot) in AXES.into_iter().zip(&mut self.axes) {
            let Some(lane) = slot else {
                continue;
            };
            let Some(rebound) = lane.rebound.as_mut().filter(|rebound| now >= rebound.next) else {
                continue;
            };
            changed = true;
            let elapsed = now.saturating_duration_since(rebound.start);
            if elapsed >= lane.options.duration {
                lane.owner.publish_overshoot(axis, 0.0, false);
                *slot = None;
            } else {
                let remaining = 1.0 - elapsed.as_secs_f32() / lane.options.duration.as_secs_f32();
                lane.value = rebound.initial * remaining.powi(3);
                rebound.next =
                    (now + SCROLL_FRAME_INTERVAL).min(rebound.start + lane.options.duration);
                lane.owner.publish_overshoot(axis, lane.value, true);
            }
        }
        changed
    }

    pub fn reconcile(&mut self, layout: Option<&LayoutNode>) -> bool {
        fn current(
            item: &LayoutNode,
            lane: &ElasticAxis,
            axis: Orientation,
        ) -> Option<ElasticOptions> {
            if !item.visible || !item.enabled {
                return None;
            }
            if item.viewport_handle.as_ref() == Some(&lane.owner) {
                return item.elastic.filter(|options| {
                    (lane.rebound.is_some() || options.behavior.allows_overshoot(false))
                        && item.pan_axes[index(axis)]
                        && item.scroll_on(axis).is_some_and(|scroll| {
                            scroll.max_offset > 0.0
                                && if lane.value < 0.0 {
                                    scroll.offset == 0.0
                                } else {
                                    scroll.offset == scroll.max_offset
                                }
                        })
                });
            }
            item.children
                .iter()
                .find_map(|child| current(child, lane, axis))
        }
        let mut changed = false;
        for (axis, slot) in AXES.into_iter().zip(&mut self.axes) {
            let Some(lane) = slot.as_mut() else {
                continue;
            };
            if let Some(options) = layout.and_then(|item| current(item, lane, axis)) {
                lane.options.move_contents = options.move_contents;
            } else {
                slot.take()
                    .unwrap()
                    .owner
                    .publish_overshoot(axis, 0.0, false);
                changed = true;
            }
        }
        changed
    }

    pub fn clear(&mut self) -> bool {
        self.reconcile(None)
    }
}
