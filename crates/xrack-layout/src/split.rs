use crate::{
    LayoutNode, Point, Rect, Size, SplitResize, TextMeasurer,
    linear::{self, Axis},
};
use xrack_runtime::{Node, Orientation};

const RESIZE_EPSILON: f32 = 0.01;

/// The leading pane's size and reachable range in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitHandleRange {
    pub orientation: Orientation,
    pub leading_pane: usize,
    pub trailing_pane: usize,
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
}

impl SplitHandleRange {
    pub const STEP: f64 = 8.0;

    pub fn can_increase(self) -> bool {
        self.maximum - self.value > f64::from(RESIZE_EPSILON)
    }

    pub fn can_decrease(self) -> bool {
        self.value - self.minimum > f64::from(RESIZE_EPSILON)
    }
}

impl LayoutNode {
    pub fn split_handle_range(&self, path: &[usize]) -> Option<SplitHandleRange> {
        let (&slot, parent) = path.split_last()?;
        let split = self.at_path(parent)?.split.as_deref()?;
        Some(split.handle_range(split.handle(slot)?))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Pane {
    slot: usize,
    length: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitHandle {
    pub slot: usize,
    pub bounds: Rect,
    before: usize,
    after: usize,
    pub(crate) minimum: f32,
    pub(crate) maximum: f32,
}

impl SplitHandle {
    pub(crate) fn adjustable(&self) -> bool {
        self.maximum - self.minimum > RESIZE_EPSILON
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitGeometry {
    pub orientation: Orientation,
    pub handles: Vec<SplitHandle>,
    hit_radius: Option<f32>,
    panes: Vec<Pane>,
    weight: f64,
    sized: bool,
    sides_constrained: bool,
}

pub(crate) fn axis_point(orientation: Orientation, point: Point) -> f32 {
    match orientation {
        Orientation::Horizontal => point.x,
        Orientation::Vertical => point.y,
    }
}

impl SplitGeometry {
    pub(crate) fn new(
        node: &Node,
        bounds: Rect,
        text: &mut impl TextMeasurer,
    ) -> (Self, Vec<Rect>) {
        let orientation = node.orientation();
        let (axis, cross) = match orientation {
            Orientation::Horizontal => (Axis::Horizontal, bounds.height),
            Orientation::Vertical => (Axis::Vertical, bounds.width),
        };
        let size = Size::new(bounds.width, bounds.height);
        let total = axis.extent(size);
        let visible: Vec<_> = node.split_panes().collect();
        let gaps = visible.len().saturating_sub(1) as f32;
        let gap = node
            .length("handle-size")
            .unwrap_or(8.0)
            .clamp(1.0, 48.0)
            .min(total / gaps.max(1.0));
        let hit_size = node
            .length("handle-hit-size")
            .unwrap_or(gap)
            .clamp(gap, total);
        let available = (total - gaps * gap).max(0.0);
        let mut constraints: Vec<_> = visible
            .iter()
            .map(|(_, pane)| linear::measure(pane, size, text).along(axis))
            .collect();
        let min_total: f64 = constraints.iter().map(|info| f64::from(info.min)).sum();
        if min_total > f64::from(available) {
            let scale = f64::from(available) / min_total;
            for info in &mut constraints {
                info.min = (f64::from(info.min) * scale) as f32;
                info.preferred = info.min;
            }
        }
        let (lengths, weight, sides_constrained) = if node.has_split_fill() && !visible.is_empty() {
            let target = node.split_fill_slot();
            let fill = visible
                .iter()
                .position(|(slot, _)| Some(*slot) == target)
                .unwrap_or(visible.len() - 1);
            let fill_info = constraints[fill];
            let fixed: Vec<_> = visible
                .iter()
                .zip(&constraints)
                .enumerate()
                .filter(|(index, _)| *index != fill)
                .map(|(_, ((slot, _), info))| {
                    let mut info = *info;
                    if let Some(saved) = node
                        .split_sizes
                        .as_ref()
                        .and_then(|sizes| sizes.get(*slot))
                        .copied()
                        .flatten()
                    {
                        info.preferred = info.clamp(saved);
                    }
                    info
                })
                .collect();
            let preferred: f64 = fixed.iter().map(|info| f64::from(info.preferred)).sum();
            let fill_length = fill_info.clamp((f64::from(available) - preferred) as f32);
            let side_space = (available - fill_length).max(0.0);
            let constrained = (preferred - f64::from(side_space)).abs() > 0.01;
            let mut lengths = linear::fit_axis(fixed, side_space);
            lengths.insert(fill, fill_length);
            (lengths, 1.0, constrained)
        } else {
            let baseline = linear::fit_axis(constraints.iter().copied(), available);
            let baseline_total: f64 = baseline.iter().map(|length| f64::from(*length)).sum();
            let weights: Vec<_> = visible
                .iter()
                .zip(&baseline)
                .map(|((slot, _), length)| {
                    node.split_weights
                        .as_deref()
                        .and_then(|saved| saved.get(*slot))
                        .copied()
                        .flatten()
                        .map_or_else(
                            || {
                                if baseline_total > 0.0 {
                                    f64::from(*length) / baseline_total
                                } else {
                                    0.0
                                }
                            },
                            f64::from,
                        )
                })
                .collect();
            let weight: f64 = weights.iter().sum();
            let lengths = if node.split_weights.is_some() && weight > 0.0 {
                linear::fit_axis(
                    constraints
                        .iter()
                        .copied()
                        .zip(weights)
                        .map(|(mut info, share)| {
                            info.preferred = info.clamp((share * baseline_total / weight) as f32);
                            info
                        }),
                    available,
                )
            } else {
                baseline
            };
            (lengths, weight, false)
        };
        let panes: Vec<_> = visible
            .iter()
            .zip(lengths)
            .map(|((slot, _), length)| Pane {
                slot: *slot,
                length,
            })
            .collect();
        let mut handles = Vec::new();
        let mut rects = vec![Rect::new(bounds.x, bounds.y, 0.0, 0.0); node.children.len()];
        let mut cursor = 0.0;
        let first_handle = node.split_pane_count();
        for (index, pane) in panes.iter().enumerate() {
            rects[pane.slot] = axis.rect(bounds, cursor, 0.0, pane.length, cross);
            cursor += pane.length;
            if let Some(next) = panes.get(index + 1) {
                let a = constraints[index];
                let b = constraints[index + 1];
                let slot = first_handle + pane.slot;
                if let Some(handle_rect) = rects.get_mut(slot) {
                    *handle_rect = axis.rect(bounds, cursor, 0.0, gap, cross);
                    handles.push(SplitHandle {
                        slot,
                        bounds: *handle_rect,
                        before: index,
                        after: next.slot,
                        minimum: (a.min - pane.length).max(next.length - b.max).min(0.0),
                        maximum: (a.max - pane.length).min(next.length - b.min).max(0.0),
                    });
                }
                cursor += gap;
            }
        }
        (
            Self {
                orientation,
                handles,
                hit_radius: (hit_size > gap).then_some(hit_size * 0.5),
                panes,
                weight,
                sized: node.has_split_fill(),
                sides_constrained,
            },
            rects,
        )
    }

    pub(crate) fn handle(&self, slot: usize) -> Option<&SplitHandle> {
        self.handles.iter().find(|handle| handle.slot == slot)
    }

    pub(crate) fn handle_range(&self, handle: &SplitHandle) -> SplitHandleRange {
        let before = &self.panes[handle.before];
        let value = f64::from(before.length);
        SplitHandleRange {
            orientation: self.orientation,
            leading_pane: before.slot,
            trailing_pane: handle.after,
            value,
            minimum: value + f64::from(handle.minimum),
            maximum: value + f64::from(handle.maximum),
        }
    }

    pub(crate) fn expanded_hit(&self, point: Point) -> Option<usize> {
        let radius = self.hit_radius?;
        let position = axis_point(self.orientation, point);
        let distance = |handle: &SplitHandle| (position - handle.center(self.orientation)).abs();
        self.handles
            .iter()
            .filter(|handle| handle.adjustable() && distance(handle) <= radius)
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .map(|handle| handle.slot)
    }

    pub(crate) fn resize(
        &self,
        path: Vec<usize>,
        handle: &SplitHandle,
        delta: f32,
    ) -> Option<SplitResize> {
        let delta = delta.clamp(handle.minimum, handle.maximum);
        let total: f64 = self.panes.iter().map(|pane| f64::from(pane.length)).sum();
        if !delta.is_finite() || delta.abs() <= RESIZE_EPSILON || total <= 0.0 {
            return None;
        }
        let mass = if self.weight > 0.0 { self.weight } else { 1.0 };
        let lengths = || {
            self.panes.iter().enumerate().map(|(index, pane)| {
                let length = match index {
                    i if i == handle.before => pane.length + delta,
                    i if i == handle.before + 1 => pane.length - delta,
                    _ => pane.length,
                };
                (pane.slot, length.max(0.0))
            })
        };
        Some(SplitResize {
            path,
            weights: lengths()
                .map(|(slot, length)| (slot, (f64::from(length) * mass / total) as f32))
                .collect(),
            sizes: self.sized.then(|| {
                if self.sides_constrained {
                    // Preserve the visible result of the drag even when the old
                    // preferred widths could not fit the window.
                    lengths().collect()
                } else {
                    lengths().skip(handle.before).take(2).collect()
                }
            }),
        })
    }
}

impl SplitHandle {
    pub(crate) fn after_pane(&self) -> usize {
        self.after
    }

    pub(crate) fn center(&self, orientation: Orientation) -> f32 {
        axis_point(
            orientation,
            Point {
                x: self.bounds.x + self.bounds.width * 0.5,
                y: self.bounds.y + self.bounds.height * 0.5,
            },
        )
    }
}
