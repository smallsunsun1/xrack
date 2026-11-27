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
        let same_target = |path| {
            scene.row_key_at(path) == self.target.row_key
                && scene
                    .at_path(path)
                    .is_some_and(|node| node.focus_handle.as_ref() == Some(&self.handle))
        };
        if same_target(path) {
            let delivered = component.focus_event_at(scene, path, self.event);
            if matches!(self.event, FocusEvent::Lost(_)) && !delivered {
                self.handle.discard_editor_changes();
            }
            return delivered;
        }
        if matches!(self.event, FocusEvent::Lost(_)) && self.handle.has_editor_changes() {
            if let Some(moved) = editor_path(scene, &self.handle, &mut Vec::new())
                && same_target(&moved)
                && component.focus_event_at(scene, &moved, self.event)
            {
                return true;
            }
            self.handle.discard_editor_changes();
        }
        false
    }
}

// If a model moved the old editor, match its owning instance before delivering loss.
fn editor_path(node: &Node, handle: &FocusHandle, path: &mut Vec<usize>) -> Option<Vec<usize>> {
    if node.focus_handle.as_ref() == Some(handle) {
        return Some(path.clone());
    }
    node.materialized_children().find_map(|(index, child)| {
        path.push(index);
        let result = editor_path(child, handle, path);
        path.pop();
        result
    })
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
                if let Some((editor, node)) =
                    input.retained_editor_for(scene, &old.target, &old.handle)
                {
                    let text = xrack_runtime::text_input_value(node);
                    let range = editor.selection(text);
                    let selected = if node.is_password() {
                        ""
                    } else {
                        &text[range.clone()]
                    };
                    old.handle
                        .publish_text_selection(editor.caret(text), range, selected);
                    old.handle.publish_text_commands(Default::default());
                } else {
                    old.handle.clear_text_state();
                }
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
                && (entry.handle.observes_text_selection()
                    || entry.handle.observes_text_commands()
                    || entry.handle.has_editor_changes())
        }) else {
            return;
        };
        let Some(node) = entry.target.node_in(scene) else {
            entry.handle.clear_text_state();
            return;
        };
        if !is_text_input(node.kind) && !combo_editable(node) && !node.selectable_styled_text() {
            entry.handle.discard_editor_changes();
            entry.handle.clear_text_state();
            return;
        }
        if !entry.handle.observes_text_commands() && !entry.handle.observes_text_selection() {
            return;
        }
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
            entry.handle.discard_editor_changes();
        }
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::VecModel;

    #[derive(Default)]
    struct Recorder(Vec<Vec<usize>>);

    impl Component for Recorder {
        fn render(&self) -> Node {
            Node::new("Window", None)
        }

        fn dispatch(&mut self, _: &str, _: &str) -> bool {
            false
        }

        fn focus_event_at(&mut self, scene: &Node, path: &[usize], event: FocusEvent) -> bool {
            let Some(editor) = scene.focus_event_target(path, event) else {
                return false;
            };
            if !editor
                .focus_handle
                .as_ref()
                .is_some_and(|handle| handle.take_editor_changes("edited"))
            {
                return false;
            }
            self.0.push(path.to_vec());
            true
        }
    }

    #[test]
    fn editing_loss_follows_the_moved_editor_and_discards_a_removed_editor() {
        let model = VecModel::from(vec![0_i64, 1]);
        let mut scene = Node::new("Window", None);
        let handle = FocusHandle::default();
        for index in 0..2 {
            let mut row = Node::new("ListRow", None);
            row.set_row_key(model.row_key(index));
            let mut editor = Node::new("LineEdit", Some("editor"));
            editor.focus_handle = Some(if index == 0 {
                handle.clone()
            } else {
                FocusHandle::default()
            });
            row.children.push(editor);
            scene.children.push(row);
        }
        let delivery = |scene: &Node, path: Vec<usize>| {
            let target = Target::from_node(
                path.clone(),
                scene.at_path(&path).unwrap(),
                true,
                scene.row_key_at(&path),
            );
            FocusDelivery {
                target,
                handle: handle.clone(),
                event: FocusEvent::Lost(FocusReason::TabNavigation),
            }
        };
        handle.record_editor_edit("", "edited");
        let moved = delivery(&scene, vec![0, 0]);
        scene.children.swap(0, 1);
        let mut ui = Recorder::default();
        assert!(moved.apply(&mut ui, &scene));
        assert_eq!(ui.0, [vec![1, 0]]);

        handle.record_editor_edit("", "edited");
        let removed = delivery(&scene, vec![1, 0]);
        scene.children.pop();
        assert!(!removed.apply(&mut ui, &scene));
        assert!(!handle.has_editor_changes());
        assert_eq!(ui.0.len(), 1);
    }
}
