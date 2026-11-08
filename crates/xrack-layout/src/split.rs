use crate::{
    Point, Rect, Size, TextMeasurer,
    linear::{self, Axis},
};
use xrack_runtime::{Node, Orientation};

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
        self.maximum - self.minimum > 0.01
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitGeometry {
    pub orientation: Orientation,
    pub handles: Vec<SplitHandle>,
    panes: Vec<Pane>,
    weight: f64,
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
                    .map(|(mut info, weight_share)| {
                        info.preferred =
                            info.clamp((weight_share * baseline_total / weight) as f32);
                        info
                    }),
                available,
            )
        } else {
            baseline
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
                panes,
                weight,
            },
            rects,
        )
    }

    pub(crate) fn handle(&self, slot: usize) -> Option<&SplitHandle> {
        self.handles.iter().find(|handle| handle.slot == slot)
    }

    pub(crate) fn resize(&self, handle: &SplitHandle, delta: f32) -> Option<Vec<(usize, f32)>> {
        let delta = delta.clamp(handle.minimum, handle.maximum);
        let total: f64 = self.panes.iter().map(|pane| f64::from(pane.length)).sum();
        if !delta.is_finite() || delta.abs() <= 0.01 || total <= 0.0 {
            return None;
        }
        let mass = if self.weight > 0.0 { self.weight } else { 1.0 };
        Some(
            self.panes
                .iter()
                .enumerate()
                .map(|(index, pane)| {
                    let length = match index {
                        i if i == handle.before => pane.length + delta,
                        i if i == handle.before + 1 => pane.length - delta,
                        _ => pane.length,
                    };
                    (
                        pane.slot,
                        (f64::from(length.max(0.0)) * mass / total) as f32,
                    )
                })
                .collect(),
        )
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
