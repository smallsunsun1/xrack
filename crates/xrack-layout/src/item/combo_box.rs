use super::{ArrangeContext, Item, ItemAction, ItemEvent, ItemPaint, ItemRef, MeasureContext};
use crate::{ComboGeometry, Rect, grid::Placement};
use std::rc::Rc;
use xrack_runtime::{ControlAction, combo_editable};

pub(super) struct ComboBox;

impl Item for ComboBox {
    fn reserves_pointer_drag(&self, node: &xrack_runtime::Node) -> bool {
        combo_editable(node)
    }

    fn focusable(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = ComboGeometry::preferred_size(cx.node, cx.available, cx.layout);
        cx.intrinsic(size)
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if cx.node.children.is_empty() {
            return None;
        }
        let geometry = ComboGeometry::for_control(cx.node, cx.bounds, cx.direction);
        Some(
            cx.node
                .children
                .iter()
                .map(|child| match child.kind {
                    "ComboIndicator" => Placement::plain(geometry.indicator_bounds(child)),
                    "ComboPopup" => Placement::plain(Rect::new(cx.bounds.x, cx.bounds.y, 0.0, 0.0)),
                    "ComboContent" => {
                        let bounds = geometry.label_bounds(cx.direction);
                        Placement {
                            bounds,
                            clip: Some(Rc::new(crate::table::PaneClip {
                                bounds,
                                space: cx.space.clone(),
                            })),
                            row_children: None,
                        }
                    }
                    _ => Placement::plain(cx.layout.absolute_child(child, cx.bounds)),
                })
                .collect(),
        )
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Combo
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        match event {
            ItemEvent::ChoiceTrigger { press, release } => {
                if combo_editable(item.node) {
                    let geometry = ComboGeometry::for_control(
                        item.node,
                        item.local_rect(),
                        item.layout.layout_direction,
                    );
                    let (press, release) = press.zip(release)?;
                    if !geometry.trigger.contains(press) || !geometry.trigger.contains(release) {
                        return None;
                    }
                }
                Some(ItemAction::OpenChoice)
            }
            ItemEvent::ChoiceSelected(index) => {
                Some(ItemAction::Control(ControlAction::Select(index)))
            }
            _ => None,
        }
    }
}
