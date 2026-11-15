use crate::{Interaction, LayoutNode, Target};
use xrack_runtime::{
    Component, FocusEvent, FocusHandle, FocusReason, Node, combo_editable, is_text_input,
};

pub(crate) struct FocusEntry {
    pub target: Target,
    pub handle: FocusHandle,
    pub direct: bool,
}

pub struct FocusDelivery {
    target: Target,
    handle: FocusHandle,
    pub event: FocusEvent,
}

impl FocusDelivery {
    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        let path = &self.target.path;
        scene.row_key_at(path) == self.target.row_key
            && scene
                .at_path(path)
                .is_some_and(|node| node.focus_handle.as_ref() == Some(&self.handle))
            && component.focus_event_at(scene, path, self.event)
    }
}

/// Publish focus to reactive handles before delivering the corresponding events.
#[derive(Default)]
pub struct FocusObservers {
    active: bool,
    entries: Vec<FocusEntry>,
}

impl FocusObservers {
    pub fn sync(
        &mut self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        active: bool,
    ) -> Vec<FocusDelivery> {
        let reason = if self.active != active {
            FocusReason::WindowActivation
        } else {
            input.focus_reason()
        };
        self.active = active;
        let next = if active {
            input.focus_entries(scene, layout)
        } else {
            Vec::new()
        };
        let previous = std::mem::replace(&mut self.entries, next);
        let mut events = Vec::new();
        let mut send = |entry: &FocusEntry, event| {
            events.push(FocusDelivery {
                target: entry.target.clone(),
                handle: entry.handle.clone(),
                event,
            })
        };
        for old in previous.iter().rev() {
            let replacement = self.entries.iter().find(|entry| entry.handle == old.handle);
            if old.direct && !replacement.is_some_and(|entry| entry.direct) {
                old.handle.clear_text_state();
                send(replacement.unwrap_or(old), FocusEvent::Lost(reason));
            }
            if replacement.is_none() {
                old.handle.publish(false, false);
                send(
                    old,
                    FocusEvent::Within {
                        focused: false,
                        reason,
                    },
                );
            }
        }
        for entry in &self.entries {
            let old = previous.iter().find(|old| old.handle == entry.handle);
            entry.handle.publish(entry.direct, true);
            if old.is_none() {
                send(
                    entry,
                    FocusEvent::Within {
                        focused: true,
                        reason,
                    },
                );
            }
            if entry.direct && !old.is_some_and(|old| old.direct) {
                send(entry, FocusEvent::Gained(reason));
            }
        }
        self.sync_text_editor(input, scene);
        events
    }

    #[doc(hidden)]
    pub fn sync_text_editor(&self, input: &Interaction, scene: &Node) {
        let Some(entry) = self.entries.iter().find(|entry| {
            entry.direct
                && (entry.handle.observes_text_selection() || entry.handle.observes_text_commands())
        }) else {
            return;
        };
        let Some(node) = entry
            .target
            .node_in(scene)
            .filter(|node| is_text_input(node.kind) || combo_editable(node))
        else {
            entry.handle.clear_text_state();
            return;
        };
        let text = input.editor_value(node);
        let (caret, range) = input.editor.as_ref().map_or_else(
            || (text.len(), text.len()..text.len()),
            |editor| (editor.caret(&text), editor.selection(&text)),
        );
        if entry.handle.observes_text_commands() {
            entry
                .handle
                .publish_text_commands(input.text_command_availability(
                    scene,
                    &entry.target,
                    node,
                    &text,
                    !range.is_empty(),
                ));
        }
        if entry.handle.observes_text_selection() {
            let selected = if node.is_password() {
                ""
            } else {
                &text[range.clone()]
            };
            entry.handle.publish_text_selection(caret, range, selected);
        }
    }

    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            entry.handle.publish(false, false);
            entry.handle.clear_text_state();
        }
        self.active = false;
    }
}
