use crate::VisualSlot;
use std::collections::HashMap;
use xrack_mlir::{
    OperationRef, ValueRef, VisualCacheGroup, VisualCacheKey, scene_children, visual_cache_operand,
};
use xrack_resources::visual::indexed_visual_bindings;

type Scope<'m> = Option<&'m str>;
type Key<'m> = VisualCacheKey<ValueRef<'m>>;

#[derive(Default)]
pub(crate) struct VisualBindings<'m> {
    slots: HashMap<(Scope<'m>, Key<'m>), VisualSlot>,
    use_visual_plan: bool,
}

pub(crate) fn repeat_scope(element: OperationRef<'_>) -> &str {
    element
        .attribute("key")
        .or_else(|| element.attribute("id"))
        .unwrap_or_default()
}

impl<'m> VisualBindings<'m> {
    pub(crate) fn new(root: OperationRef<'m>, use_visual_plan: bool) -> Self {
        let mut bindings = Self {
            use_visual_plan,
            ..Self::default()
        };
        bindings.index_scene(root, None);
        bindings.index_component_bindings(root);
        bindings
    }

    pub(crate) fn slot(&self, binding: OperationRef<'m>, scope: Scope<'m>) -> Option<VisualSlot> {
        let key = Key::for_binding(binding, self.use_visual_plan)?;
        self.slots
            .get(&(
                if key.uses_component_scope() {
                    None
                } else {
                    scope
                },
                key,
            ))
            .copied()
    }

    fn index(&mut self, binding: OperationRef<'m>, scope: Scope<'m>) {
        if visual_cache_operand(binding, self.use_visual_plan).is_none() {
            return;
        }
        let Some(key) = Key::for_binding(binding, self.use_visual_plan) else {
            return;
        };
        // Repeat invariants are allocated in the component's state.
        if scope.is_some() && key.uses_component_scope() {
            return;
        }
        let index = self.slots.len();
        self.slots.entry((scope, key)).or_insert_with(|| match key {
            Key::Operand(_) => VisualSlot::Operand(index),
            Key::Planned(group) => VisualSlot::Planned(group),
        });
    }

    fn empty_repeat(&self, element: OperationRef<'m>) -> bool {
        self.use_visual_plan
            && element.children().any(|child| {
                child.name() == "xr.repeat" && child.attribute("xr.empty_repeat") == Some("true")
            })
    }

    fn index_scene(&mut self, element: OperationRef<'m>, scope: Scope<'m>) {
        if self.empty_repeat(element)
            || self.use_visual_plan && element.attribute("xr.static_subtree") == Some("true")
        {
            return;
        }
        let scope = if element.children().any(|child| child.name() == "xr.repeat") {
            Some(repeat_scope(element))
        } else {
            scope
        };
        for binding in indexed_visual_bindings(element) {
            self.index(binding, scope);
        }
        for child in scene_children(element) {
            self.index_scene(child, scope);
        }
    }

    fn index_component_bindings(&mut self, element: OperationRef<'m>) {
        if self.empty_repeat(element) {
            return;
        }
        // Action and timer definitions are outside the ordinary scene slots.
        if matches!(element.attribute("kind"), Some("Action" | "Timer")) {
            self.index_scene(element, None);
        }
        for child in element.children() {
            if matches!(child.name(), "xr.element" | "xr.instance") {
                self.index_component_bindings(child);
            } else if self.use_visual_plan
                && matches!(child.name(), "xr.bind" | "xr.layout_bind" | "xr.show_if")
                && matches!(
                    VisualCacheGroup::for_binding(child),
                    Some(VisualCacheGroup::RepeatInvariant(_))
                )
            {
                self.index(child, None);
            }
        }
    }
}
