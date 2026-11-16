use crate::{
    LayoutNode, Target,
    input::{
        eligible, focus_reachable, focus_targets, focus_targets_with_reveal, focusable,
        moved_row_target, radio_group_focus, valid,
    },
};
use std::collections::{BTreeMap, BTreeSet};
use xrack_runtime::{FocusHandle, Node, PageIdentity, group_index, is_window_overlay};

#[derive(Clone, Debug, Default)]
pub(crate) struct FocusHistory {
    entries: Vec<(Target, Target)>,
}

fn remembers_page_focus(view: &Node) -> bool {
    matches!(view.kind, "StackLayout" | "SwipeView" | "TabWidget")
}

fn is_page_content(view: &Node, depth: usize, target: &Target) -> bool {
    // A Tab is also its title-bar focus target.
    target.path.len() > depth + 1 || view.kind != "TabWidget"
}

#[derive(Clone, Debug)]
struct ViewedPages {
    owner: Target,
    last_page: PageIdentity,
    pages: BTreeMap<PageIdentity, Target>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PageFocusHistory {
    views: BTreeMap<Vec<usize>, ViewedPages>,
}

impl PageFocusHistory {
    pub fn remember(&mut self, scene: &Node, layout: &LayoutNode, focus: Option<&Target>) {
        let Some(focus) = focus.filter(|focus| valid(focus, scene, layout)) else {
            return;
        };
        if scene
            .at_path(&focus.path)
            .is_some_and(|node| is_window_overlay(node.kind))
        {
            return;
        }
        for depth in (0..focus.path.len()).rev() {
            let path = &focus.path[..depth];
            let Some(view) = scene.at_path(path) else {
                break;
            };
            if is_window_overlay(view.kind) {
                break;
            }
            let slot = focus.path[depth];
            if !remembers_page_focus(view)
                || !is_page_content(view, depth, focus)
                || group_index(view) != Some(slot)
            {
                continue;
            }
            let Some(page) = view.child_at(slot) else {
                continue;
            };
            let id = PageIdentity::of(page, slot);
            let owner = Target::from_node(path.to_vec(), view, false, scene.row_key_at(path));
            let history = self
                .views
                .entry(path.to_vec())
                .or_insert_with(|| ViewedPages {
                    owner: owner.clone(),
                    last_page: id.clone(),
                    pages: BTreeMap::new(),
                });
            if history.owner != owner {
                history.owner = owner;
                history.pages.clear();
            } else if history.last_page != id {
                // Model pages still have their row metadata while their content is unmounted.
                let live: BTreeSet<_> = view
                    .children
                    .iter()
                    .enumerate()
                    .map(|(slot, page)| PageIdentity::of(page, slot))
                    .collect();
                history.pages.retain(|page, _| live.contains(page));
            }
            history.last_page = id.clone();
            history.pages.insert(id, focus.clone());
        }
    }

    pub fn reconcile(&mut self, scene: &Node) {
        self.views.retain(|_, history| {
            history
                .owner
                .node_in(scene)
                .is_some_and(remembers_page_focus)
        });
    }

    pub fn restore(
        &self,
        outgoing: &Target,
        scene: &Node,
        layout: &LayoutNode,
        boundary: &[usize],
    ) -> Option<Target> {
        if !outgoing.path.starts_with(boundary) {
            return None;
        }
        for depth in (boundary.len()..outgoing.path.len()).rev() {
            let path = &outgoing.path[..depth];
            let Some(history) = self.views.get(path) else {
                continue;
            };
            if history.pages.get(&history.last_page) != Some(outgoing) {
                continue;
            }
            let Some(view) = scene.interactive_at_path(path) else {
                continue;
            };
            let Some(slot) = group_index(view) else {
                continue;
            };
            let Some(page) = view.child_at(slot) else {
                continue;
            };
            let id = PageIdentity::of(page, slot);
            if id == history.last_page {
                continue;
            }
            let mut page_path = path.to_vec();
            page_path.push(slot);
            return Self::remembered(history, &id, &page_path, scene, layout)
                .or_else(|| focus_targets(scene, layout, &page_path).into_iter().next());
        }
        None
    }

    pub fn tab_entry(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        previous: Option<&Target>,
        candidate: &Target,
        boundary: usize,
        stops: &[Target],
    ) -> Option<Target> {
        (boundary..candidate.path.len()).find_map(|depth| {
            let path = &candidate.path[..depth];
            let history = self.views.get(path)?;
            let view = history.owner.node_in(scene)?;
            let slot = candidate.path[depth];
            let page_path = &candidate.path[..=depth];
            if !is_page_content(view, depth, candidate)
                || previous.is_some_and(|p| {
                    p.path.starts_with(page_path) && is_page_content(view, depth, p)
                })
                || group_index(view) != Some(slot)
            {
                return None;
            }
            let id = PageIdentity::of(view.child_at(slot)?, slot);
            Self::remembered(history, &id, page_path, scene, layout)
                .filter(|target| stops.contains(target))
        })
    }

    fn remembered(
        history: &ViewedPages,
        id: &PageIdentity,
        page: &[usize],
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<Target> {
        let (&slot, owner) = page.split_last()?;
        let mut target = history.pages.get(id)?.clone();
        target.path[owner.len()] = slot;
        if valid(&target, scene, layout) {
            return Some(target);
        }
        moved_row_target(&target, scene, layout).filter(|target| target.path.starts_with(page))
    }
}

pub(crate) fn target_at(scene: &Node, layout: &LayoutNode, path: &[usize]) -> Option<Target> {
    target_at_with_reveal(scene, layout, path, false)
}

fn target_at_with_reveal(
    scene: &Node,
    layout: &LayoutNode,
    path: &[usize],
    reveal: bool,
) -> Option<Target> {
    let node = scene.interactive_at_path(path)?;
    let item = layout.at_path(path)?;
    let direct = focusable(node);
    let target = Target::from_node(path.to_vec(), node, direct, scene.row_key_at(path));
    let available = if node.kind == "RadioGroup" {
        let clip = item.clip_rect(item.bounds);
        item.visible
            && item.enabled
            && (clip.width > 0.0 && clip.height > 0.0
                || reveal && radio_group_focus(scene, layout, path, true).is_some())
    } else {
        direct && (eligible(node, item) || focus_reachable(&target, scene, layout, reveal))
    };
    available.then_some(target)
}

pub(crate) fn handle_target(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    handle: &FocusHandle,
    reveal: bool,
) -> Option<Target> {
    fn find(node: &Node, item: &LayoutNode, handle: &FocusHandle, path: &mut Vec<usize>) -> bool {
        if !item.visible || !item.enabled {
            return false;
        }
        if node.focus_handle.as_ref() == Some(handle) {
            return true;
        }
        for (index, child, item) in item.children_in_scene_order(node) {
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
    .then(|| target_at_with_reveal(scene, layout, &path, reveal))
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
                matches!(node.kind, "FocusScope" | "Loader") && node.boolean("restore-focus", true)
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
            target
                .node_in(scene)
                .is_some_and(|node| focusable(node) || target.text_link_index().is_some())
        };
        self.entries.retain_mut(|(scope, leaf)| {
            if !mounted(scope) {
                let Some(moved) = moved_row_target(scope, scene, layout) else {
                    return false;
                };
                *scope = moved;
            }
            if !scene.at_path(&scope.path).is_some_and(|node| {
                matches!(node.kind, "FocusScope" | "Loader") && node.boolean("restore-focus", true)
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
        self.choose_with_reveal(scene, layout, scope, reverse, tab, tab)
    }

    pub fn choose_with_reveal(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        scope: Target,
        reverse: bool,
        tab: bool,
        reveal: bool,
    ) -> Target {
        let Some(node) = scene
            .at_path(&scope.path)
            .filter(|node| matches!(node.kind, "FocusScope" | "Loader"))
        else {
            return scope;
        };
        let candidates = focus_targets_with_reveal(scene, layout, &scope.path, reveal);
        let permitted = |target: &Target| !tab || candidates.contains(target);
        if node.boolean("restore-focus", true)
            && let Some((_, leaf)) = self.entries.iter().find(|(entry, _)| *entry == scope)
            && focus_reachable(leaf, scene, layout, reveal)
            && permitted(leaf)
        {
            return leaf.clone();
        }
        if let Some(handle) = &node.initial_focus
            && let Some(target) = handle_target(scene, layout, &scope.path, handle, reveal)
            && target.path.len() > scope.path.len()
        {
            let target = match scene.at_path(&target.path) {
                Some(group) if group.kind == "RadioGroup" => {
                    radio_group_focus(scene, layout, &target.path, reveal)
                }
                _ => Some(self.choose_with_reveal(scene, layout, target, reverse, tab, reveal)),
            };
            if let Some(target) = target.filter(permitted) {
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
                reveal,
            ) {
                return self.choose_with_reveal(scene, layout, child, reverse, tab, reveal);
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
        reveal: bool,
    ) -> Option<Target> {
        (boundary..next.path.len()).find_map(|depth| {
            let path = &next.path[..depth];
            if previous.is_some_and(|previous| previous.path.starts_with(path)) {
                return None;
            }
            let node = scene.at_path(path)?;
            if !matches!(node.kind, "FocusScope" | "Loader")
                || tab && !node.boolean("focus-on-tab-navigation", true)
            {
                return None;
            }
            target_at_with_reveal(scene, layout, path, reveal)
        })
    }
}
