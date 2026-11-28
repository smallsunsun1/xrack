use super::{ArrangeContext, HitBehavior, Item, ItemPaint, ItemRef, MeasureContext, SizePolicy};
use crate::{LayoutNode, Point, Rect, Size, grid::Placement};
use std::{collections::BTreeMap, rc::Rc};
use xrack_runtime::{
    CustomItem, CustomItemChange, CustomItemContext, CustomItemId, CustomItemPlacement,
    CustomItemRect, Node, SceneFeature,
};

pub(super) struct Native;

fn rectangle(rect: Rect) -> CustomItemRect {
    CustomItemRect::new(rect.x, rect.y, rect.width, rect.height)
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn size(value: f32) -> f32 {
    finite(value).max(0.0)
}

#[doc(hidden)]
pub fn custom_item_context<'a>(node: &'a Node, layout: &LayoutNode) -> CustomItemContext<'a> {
    let bounds = layout.bounds;
    let clipped = layout.clip_rect(bounds);
    let visible = layout.visible && clipped.width > 0.0 && clipped.height > 0.0;
    CustomItemContext::for_node(
        node,
        CustomItemPlacement {
            bounds: CustomItemRect::new(0.0, 0.0, bounds.width, bounds.height),
            window_bounds: rectangle(layout.window_bounds(bounds)),
            visible_bounds: if visible {
                CustomItemRect::new(
                    clipped.x - bounds.x,
                    clipped.y - bounds.y,
                    clipped.width,
                    clipped.height,
                )
            } else {
                CustomItemRect::default()
            },
            visible,
            enabled: layout.enabled,
        },
    )
}

impl Item for Native {
    fn size_policy(&self, _node: &Node) -> SizePolicy {
        SizePolicy::INTRINSIC
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let measured = cx.node.custom_item().map_or([0.0; 2], |item| {
            item.measure(
                cx.node.custom_item_data(),
                [cx.available.width, cx.available.height],
            )
        });
        cx.intrinsic(Size::new(size(measured[0]), size(measured[1])))
    }

    fn baseline(&self, node: &Node, size: Size, _text: &mut dyn super::LayoutServices) -> f32 {
        node.custom_item().map_or(0.0, |item| {
            item.baseline(node.custom_item_data(), [size.width, size.height])
        })
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        let item = cx
            .node
            .custom_item()
            .filter(|_| !cx.node.children.is_empty())?;
        let bounds = cx.bounds;
        let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        let mut children: Vec<_> = cx
            .node
            .children
            .iter()
            .map(|child| rectangle(cx.layout.absolute_child(child, local)))
            .collect();
        item.arrange(
            CustomItemContext::for_node(
                cx.node,
                CustomItemPlacement {
                    bounds: rectangle(local),
                    window_bounds: rectangle(bounds),
                    visible_bounds: rectangle(local),
                    visible: true,
                    enabled: cx.enabled,
                },
            ),
            &mut children,
        );
        let mut positions: Vec<_> = children
            .into_iter()
            .map(|child| {
                Rect::new(
                    bounds.x + finite(child.x),
                    bounds.y + finite(child.y),
                    size(child.width),
                    size(child.height),
                )
            })
            .collect();
        crate::engine::anchors::apply(
            &cx.node.children,
            bounds,
            &mut positions,
            cx.layout,
            |_| false,
            None,
        );
        Some(positions.into_iter().map(Placement::plain).collect())
    }

    fn reserves_pointer_drag(&self, node: &Node) -> bool {
        node.custom_item()
            .is_some_and(|item| item.reserves_pointer_drag(node.custom_item_data()))
    }

    fn focusable(&self, node: &Node) -> bool {
        node.boolean("focusable", true)
            && node
                .custom_item()
                .is_some_and(|item| item.focusable(node.custom_item_data()))
    }

    fn contains(&self, item: ItemRef<'_, '_>, point: Point) -> bool {
        item.node.custom_item().is_some_and(|definition| {
            definition.contains(
                custom_item_context(item.node, item.layout),
                [point.x, point.y],
            )
        })
    }

    fn hit_behavior(&self, node: &Node) -> HitBehavior {
        if node.custom_item().is_some() {
            HitBehavior::TouchArea
        } else {
            HitBehavior::ChildrenOnly
        }
    }

    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::CustomItem
    }
}

struct Observed {
    definition: Rc<dyn CustomItem>,
    node: Node,
    placement: CustomItemPlacement,
}

impl Observed {
    fn notify(&self, change: CustomItemChange) {
        if let Some(handle) = &self.node.geometry_handle {
            if change == CustomItemChange::Unmounted {
                handle.release_custom_item();
            } else {
                handle.publish_custom_item_placement(self.placement);
            }
        }
        self.definition.changed(
            CustomItemContext::for_node(&self.node, self.placement),
            change,
        );
    }
}

/// The observer runs after layout publication. A retained node snapshot keeps
/// the instance handle live until the unmount notification has been sent.
#[derive(Default)]
pub(crate) struct CustomItemObservers(BTreeMap<CustomItemId, Observed>);

impl CustomItemObservers {
    pub(crate) fn sync(&mut self, scene: &Node, layout: &LayoutNode) {
        fn collect(node: &Node, layout: &LayoutNode, next: &mut BTreeMap<CustomItemId, Observed>) {
            if !node.contains_scene_feature(SceneFeature::CustomItem) {
                return;
            }
            if let Some(definition) = node.custom_item()
                && let Some(handle) = &node.geometry_handle
            {
                let context = custom_item_context(node, layout);
                handle.publish_custom_item_placement(context.placement);
                if let Some(id) = context.state.id {
                    next.insert(
                        id,
                        Observed {
                            definition,
                            node: node.clone(),
                            placement: context.placement,
                        },
                    );
                }
            }
            for (_, child, item) in layout.children_in_scene_order(node) {
                collect(child, item, next);
            }
        }

        let mut next = BTreeMap::new();
        collect(scene, layout, &mut next);
        let mut previous = std::mem::replace(&mut self.0, next);
        for (id, current) in &self.0 {
            match previous.remove(id) {
                Some(old) if Rc::ptr_eq(&old.definition, &current.definition) => {
                    if old.placement != current.placement {
                        current.notify(CustomItemChange::Placement {
                            previous: old.placement,
                        });
                    }
                }
                old => {
                    if let Some(old) = old {
                        old.notify(CustomItemChange::Unmounted);
                    }
                    current.notify(CustomItemChange::Mounted);
                }
            }
        }
        for old in previous.into_values() {
            old.notify(CustomItemChange::Unmounted);
        }
    }
}

impl Drop for CustomItemObservers {
    fn drop(&mut self) {
        for item in self.0.values() {
            item.notify(CustomItemChange::Unmounted);
        }
    }
}
