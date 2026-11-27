use super::{Item, ItemPaint};
use xrack_runtime::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragRole {
    Source,
    Target,
}

pub(crate) fn role(node: &Node) -> Option<DragRole> {
    super::item(node.kind).drag_role()
}

pub(super) struct DragArea(pub(super) DragRole);

impl Item for DragArea {
    fn drag_role(&self) -> Option<DragRole> {
        Some(self.0)
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        match self.0 {
            DragRole::Target => ItemPaint::DropArea,
            DragRole::Source => ItemPaint::Default,
        }
    }
}
