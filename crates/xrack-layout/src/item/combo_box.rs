use super::{
    ArrangeContext, ControlPartPlacement, Item, ItemAction, ItemEvent, ItemPaint, ItemRef,
    MeasureContext,
};
use crate::{ComboGeometry, Rect, grid::Placement};
use xrack_runtime::{ControlAction, combo_editable};

pub(super) struct ComboBox;

impl Item for ComboBox {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

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
        let direction = cx.direction;
        let empty = Rect::new(cx.bounds.x, cx.bounds.y, 0.0, 0.0);
        Some(cx.control_children(|child| {
            Some(match child.kind {
                "ComboIndicator" => ControlPartPlacement::Plain(geometry.indicator_bounds(child)),
                "ComboPopup" => ControlPartPlacement::Plain(empty),
                "ComboContent" => ControlPartPlacement::Clipped(geometry.label_bounds(direction)),
                _ => return None,
            })
        }))
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
