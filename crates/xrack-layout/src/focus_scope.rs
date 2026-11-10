use crate::{
    LayoutNode, Target,
    input::{eligible, focus_targets, focusable, moved_row_target, valid},
};
use xrack_runtime::{FocusHandle, Node, is_window_overlay};

#[derive(Clone, Debug, Default)]
pub(crate) struct FocusHistory {
    entries: Vec<(Target, Target)>,
}

pub(crate) fn target_at(scene: &Node, layout: &LayoutNode, path: &[usize]) -> Option<Target> {
    let node = scene.interactive_at_path(path)?;
    let item = layout.at_path(path)?;
    (focusable(node) && eligible(node, item)).then(|| Target {
        text_link: None,
        path: path.to_vec(),
        id: node.id,
        focusable: true,
        row_key: scene.row_key_at(path),
    })
}

pub(crate) fn handle_target(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    handle: &FocusHandle,
) -> Option<Target> {
    fn find(node: &Node, item: &LayoutNode, handle: &FocusHandle, path: &mut Vec<usize>) -> bool {
        if !item.visible || !item.enabled {
            return false;
        }
        if node.focus_handle.as_ref() == Some(handle) {
            return true;
        }
        for (index, (child, item)) in node.children.iter().zip(&item.children).enumerate() {
            path.push(index);
            if find(child, item, handle, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = scope.to_vec();
    find(
        scene.at_path(scope)?,
        layout.at_path(scope)?,
        handle,
        &mut path,
    )
    .then(|| target_at(scene, layout, &path))
    .flatten()
}

impl FocusHistory {
    pub fn remember(&mut self, scene: &Node, layout: &LayoutNode, focused: Option<&Target>) {
        let Some(focused) = focused.filter(|target| valid(target, scene, layout)) else {
            return;
        };
        for depth in 0..focused.path.len() {
            let path = &focused.path[..depth];
            if !scene.at_path(path).is_some_and(|node| {
                node.kind == "FocusScope" && node.boolean("restore-focus", true)
            }) {
                continue;
            }
            let Some(scope) = target_at(scene, layout, path) else {
                continue;
            };
            // Remember a leaf inside this focus scope, not an independently
            // opened popup that happens to be a descendant in the source tree.
            if (depth + 1..=focused.path.len()).any(|depth| {
                scene
                    .at_path(&focused.path[..depth])
                    .is_some_and(|node| is_window_overlay(node.kind))
            }) {
                continue;
            }
            if let Some((_, leaf)) = self.entries.iter_mut().find(|(entry, _)| *entry == scope) {
                *leaf = focused.clone();
            } else {
                self.entries.push((scope, focused.clone()));
            }
        }
    }

    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) {
        let mounted = |target: &Target| {
            scene.row_key_at(&target.path) == target.row_key
                && scene.at_path(&target.path).is_some_and(|node| {
                    node.id == target.id && (focusable(node) || target.text_link_index().is_some())
                })
        };
        self.entries.retain_mut(|(scope, leaf)| {
            if !mounted(scope) {
                let Some(moved) = moved_row_target(scope, scene, layout) else {
                    return false;
                };
                *scope = moved;
            }
            if !scene.at_path(&scope.path).is_some_and(|node| {
                node.kind == "FocusScope" && node.boolean("restore-focus", true)
            }) {
                return false;
            }
            if !mounted(leaf) {
                let Some(moved) = moved_row_target(leaf, scene, layout) else {
                    return false;
                };
                *leaf = moved;
            }
            leaf.path.starts_with(&scope.path)
        });
    }

    pub fn choose(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        scope: Target,
        reverse: bool,
        tab: bool,
    ) -> Target {
        let Some(node) = scene
            .at_path(&scope.path)
            .filter(|node| node.kind == "FocusScope")
        else {
            return scope;
        };
        let candidates = focus_targets(scene, layout, &scope.path);
        let permitted = |target: &Target| !tab || candidates.contains(target);
        if node.boolean("restore-focus", true)
            && let Some((_, leaf)) = self.entries.iter().find(|(entry, _)| *entry == scope)
            && valid(leaf, scene, layout)
            && permitted(leaf)
        {
            return leaf.clone();
        }
        if let Some(handle) = &node.initial_focus
            && let Some(target) = handle_target(scene, layout, &scope.path, handle)
            && target.path.len() > scope.path.len()
        {
            let target = self.choose(scene, layout, target, reverse, tab);
            if permitted(&target) {
                return target;
            }
        }
        let candidate = if reverse {
            candidates.into_iter().next_back()
        } else {
            candidates.into_iter().next()
        };
        if let Some(candidate) = candidate {
            if let Some(child) = self.entered_scope(
                scene,
                layout,
                Some(&scope),
                &candidate,
                scope.path.len() + 1,
                tab,
            ) {
                return self.choose(scene, layout, child, reverse, tab);
            }
            return candidate;
        }
        scope
    }

    pub fn entered_scope(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        previous: Option<&Target>,
        next: &Target,
        boundary: usize,
        tab: bool,
    ) -> Option<Target> {
        (boundary..next.path.len()).find_map(|depth| {
            let path = &next.path[..depth];
            if previous.is_some_and(|previous| previous.path.starts_with(path)) {
                return None;
            }
            let node = scene.at_path(path)?;
            if node.kind != "FocusScope" || tab && !node.boolean("focus-on-tab-navigation", true) {
                return None;
            }
            target_at(scene, layout, path)
        })
    }
}
