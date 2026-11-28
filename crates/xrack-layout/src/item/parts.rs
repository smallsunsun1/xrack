use super::{ArrangeContext, Item, ItemPaint, MeasureContext};
use crate::{Size, grid::Placement, progress_fill_bounds, table::PaneClip};
use std::rc::Rc;
use xrack_runtime::{IndicatorState, Node, indicator_state};

pub(super) fn indicator_paint(node: &Node) -> ItemPaint {
    if node.has_progress_parts() {
        ItemPaint::Default
    } else {
        indicator_state(node).map_or(ItemPaint::Default, ItemPaint::Indicator)
    }
}

pub(super) struct Progress;

impl Item for Progress {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn paint(&self, node: &Node) -> ItemPaint {
        ItemPaint::Progress(indicator_state(node).unwrap_or(IndicatorState::Hidden))
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        cx.intrinsic(Size::new(160.0, 10.0))
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if !cx.node.has_progress_parts() {
            return None;
        }
        let fill = progress_fill_bounds(
            cx.bounds,
            cx.direction,
            indicator_state(cx.node).unwrap_or(IndicatorState::Hidden),
            None,
        );
        Some(
            cx.node
                .children
                .iter()
                .map(|child| {
                    Placement::plain(match child.kind {
                        "ProgressTrack" => cx.bounds,
                        "ProgressFill" => fill,
                        _ => cx.layout.absolute_child(child, cx.bounds),
                    })
                })
                .collect(),
        )
    }
}

pub(super) struct CalendarCells;

impl Item for CalendarCells {
    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        let cells = (0..cx.node.children.len()).map(|slot| {
            let bounds = crate::calendar::month_cell(cx.bounds, slot, cx.direction);
            Placement {
                bounds,
                clip: Some(Rc::new(PaneClip {
                    bounds,
                    space: cx.space.clone(),
                })),
                row_children: None,
            }
        });
        Some(cells.collect())
    }
}

pub(super) struct SplitHandle;

impl Item for SplitHandle {
    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn hit_behavior(&self, _node: &Node) -> super::HitBehavior {
        super::HitBehavior::SplitHandle
    }

    fn focusable(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::SplitHandle
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        Some(
            cx.node
                .children
                .iter()
                .map(|_| Placement::plain(cx.bounds))
                .collect(),
        )
    }
}
