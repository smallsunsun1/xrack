use super::{
    DeferredLayout, HitBehavior, Item, ItemLayout, ItemPaint, MeasureContext, OverlayLayout,
};
use crate::linear::{Axis, Metrics};
use xrack_runtime::Node;

pub(super) struct Overlay(pub(super) OverlayLayout);

impl Item for Overlay {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Overlay(self.0)
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn hit_behavior(&self, _node: &Node) -> HitBehavior {
        HitBehavior::ChildrenOnly
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Boxed
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        cx.layout
            .measure_line(cx.node, cx.available, Axis::Vertical)
    }
}

pub(super) struct Deferred(pub(super) DeferredLayout);

impl Item for Deferred {
    fn paint(&self, _node: &Node) -> ItemPaint {
        if self.0 == DeferredLayout::Shortcut {
            ItemPaint::Shortcut
        } else {
            ItemPaint::Default
        }
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Deferred(self.0)
    }
}

pub(super) struct Window;

impl Item for Window {
    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::Window
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Window
    }
}
