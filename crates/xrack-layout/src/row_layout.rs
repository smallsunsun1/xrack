use crate::{ClipChain, LayoutNode, Rect, Size};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use xrack_runtime::{Node, RowKey, SharedNodeIdentity};

const MAX_CACHED_ROWS: usize = 512;
const MAX_CACHED_SCROLL_VIEWS: usize = 8;
const MAX_SCROLL_CAPTURE_ON_EDIT: usize = 4096;
const MAX_CACHED_SCROLL_SLOTS: usize = MAX_CACHED_SCROLL_VIEWS * MAX_SCROLL_CAPTURE_ON_EDIT;
const MAX_CACHED_SCROLL_NODES: usize = MAX_CACHED_SCROLL_SLOTS;
const MAX_LOCAL_SUBTREE_NODES: usize = 128;
// Preserve the previous worst-case number of cached layout nodes while
// allowing more small subtrees to share the budget.
const MAX_CACHED_LOCAL_NODES: usize = MAX_CACHED_ROWS * MAX_LOCAL_SUBTREE_NODES;
const MAX_CACHED_LAYOUT_NODES: usize = MAX_CACHED_SCROLL_NODES + MAX_CACHED_LOCAL_NODES;
const MAX_ABSOLUTE_LEAVES: usize = 1024;

#[derive(Clone, Copy)]
pub(super) enum LocalSubtree {
    SharedRow,
    AnimatedGroup,
    Container,
}

impl LocalSubtree {
    pub(super) fn for_node(node: &Node) -> Option<Self> {
        if node.row_key.is_some() && node.has_shared_storage() {
            Some(Self::SharedRow)
        } else if !node.children.is_empty()
            && node.is_visible()
            && (node.opacity() < 1.0
                || node.animations.contains_key("x")
                || node.animations.contains_key("y")
                || node.kind == "Rectangle"
                    && node
                        .color("drop-shadow-color")
                        .is_some_and(|color| color.0 & 0xff != 0))
        {
            Some(Self::AnimatedGroup)
        } else if node.is_visible()
            && !node.children.is_empty()
            && matches!(
                node.kind,
                "Rectangle" | "Pane" | "HorizontalLayout" | "VerticalLayout"
            )
        {
            Some(Self::Container)
        } else {
            None
        }
    }

    fn matches(self, saved: &Node, current: &Node) -> bool {
        match self {
            Self::SharedRow => saved.shares_storage_with(current),
            Self::AnimatedGroup | Self::Container => saved.same_local_layout_inputs(current),
        }
    }

    fn minimum_nodes(self) -> usize {
        match self {
            Self::SharedRow => 1,
            Self::AnimatedGroup => 4,
            Self::Container => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RowLayoutStats {
    pub calculated: u64,
    pub reused: u64,
    pub subtree_reuses: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RowInputs {
    pub size: Size,
    pub visible: bool,
    pub enabled: bool,
    pub layout_direction: Option<xrack_runtime::LayoutDirection>,
}

struct Row {
    node: Node,
    inputs: RowInputs,
    layout: LayoutNode,
    used: bool,
}

struct LocalLayout {
    node: Node,
    inputs: RowInputs,
    layout: LayoutNode,
    nodes: usize,
    last_frame: u64,
}

const MAX_INDEXED_CHILDREN: usize = 4096;

// Direct children use their placement slot. Nested paths and sparse
// high-numbered slots use the existing keyed cache.
#[derive(Default)]
struct LocalLayouts {
    direct: Vec<Option<Box<LocalLayout>>>,
    nested: BTreeMap<Vec<usize>, LocalLayout>,
    indexed_entries: usize,
}

impl LocalLayouts {
    fn slot(path: &[usize]) -> Option<usize> {
        match path {
            [index] if *index < MAX_INDEXED_CHILDREN => Some(*index),
            _ => None,
        }
    }

    fn get(&self, path: &[usize]) -> Option<&LocalLayout> {
        if let Some(index) = Self::slot(path) {
            self.direct.get(index)?.as_deref()
        } else {
            self.nested.get(path)
        }
    }

    fn get_mut(&mut self, path: &[usize]) -> Option<&mut LocalLayout> {
        if let Some(index) = Self::slot(path) {
            self.direct.get_mut(index)?.as_deref_mut()
        } else {
            self.nested.get_mut(path)
        }
    }

    fn insert(&mut self, path: &[usize], layout: LocalLayout) -> Option<LocalLayout> {
        let Some(index) = Self::slot(path) else {
            return self.nested.insert(path.to_vec(), layout);
        };
        if self.direct.len() <= index {
            self.direct.resize_with(index + 1, || None);
        }
        let slot = &mut self.direct[index];
        if let Some(old) = slot.as_deref_mut() {
            Some(std::mem::replace(old, layout))
        } else {
            *slot = Some(Box::new(layout));
            self.indexed_entries += 1;
            None
        }
    }

    fn len(&self) -> usize {
        self.indexed_entries + self.nested.len()
    }

    fn descendant_nodes(&self, path: &[usize]) -> usize {
        let direct = if path.is_empty() {
            self.direct.iter().flatten().map(|row| row.nodes).sum()
        } else {
            0
        };
        direct
            + self
                .nested
                .iter()
                .filter(|(key, _)| key.len() > path.len() && key.starts_with(path))
                .map(|(_, row)| row.nodes)
                .sum::<usize>()
    }

    fn evict_descendants(&mut self, path: &[usize], frame: u64) -> (usize, usize) {
        let mut nodes = 0;
        let mut visited = 0;
        let mut remove = |row: &LocalLayout| {
            nodes += row.nodes;
            visited += usize::from(row.last_frame == frame);
        };
        if path.is_empty() {
            for row in self.direct.drain(..).flatten() {
                remove(&row);
            }
            self.indexed_entries = 0;
        }
        self.nested.retain(|key, row| {
            let descendant = key.len() > path.len() && key.starts_with(path);
            if descendant {
                remove(row);
            }
            !descendant
        });
        (nodes, visited)
    }

    fn retain_frame(&mut self, frame: u64) -> usize {
        let mut retained_nodes = 0;
        let mut indexed_entries = 0;
        let mut len = 0;
        for (index, slot) in self.direct.iter_mut().enumerate() {
            if let Some(entry) = slot {
                if entry.last_frame == frame {
                    indexed_entries += 1;
                    retained_nodes += entry.nodes;
                    len = index + 1;
                } else {
                    *slot = None;
                }
            }
        }
        self.indexed_entries = indexed_entries;
        self.direct.truncate(len);
        self.nested.retain(|_, entry| {
            if entry.last_frame == frame {
                retained_nodes += entry.nodes;
                true
            } else {
                false
            }
        });
        retained_nodes
    }
}

#[cfg(test)]
mod local_layout_tests {
    use super::*;

    fn entry(frame: u64, nodes: usize) -> LocalLayout {
        let node = Node::new("Rectangle", None);
        let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
        let layout = LayoutNode::leaf(&node, rect, rect, true, true, None, 12.0);
        LocalLayout {
            node,
            inputs: RowInputs {
                size: Size::new(16.0, 16.0),
                visible: true,
                enabled: true,
                layout_direction: None,
            },
            layout,
            nodes,
            last_frame: frame,
        }
    }

    #[test]
    fn reused_projection_refreshes_descendants_and_keeps_matching_buffers() {
        let mut template = entry(1, 4).layout;
        let mut group = entry(1, 2).layout;
        group.children.push(entry(1, 1).layout);
        template.children.extend([group, entry(1, 1).layout]);

        let mut previous = template.children.clone();
        previous[0].children[0].enabled = false;
        previous[0].children[0].visible = false;
        previous[0].children[0].bounds.x = 100.0;
        let reused_buffer = previous[0].children.as_ptr();
        previous[1].children.push(entry(0, 1).layout);

        let mut old_row = entry(0, 4).layout;
        old_row.children = previous;
        let mut old_group = entry(0, 5).layout;
        old_group.children.push(old_row);
        let mut old_window = entry(0, 6).layout;
        old_window.children.push(old_group);
        let mut cache = RowLayoutCache::default();
        cache.use_recycled_layout(Some(old_window));
        let mut row = entry(1, 4);
        row.layout = template;
        let node = row.node.clone();
        let inputs = row.inputs;
        cache.local_subtrees.insert(&[0, 0], row);
        let (template, children) = cache
            .local_layout(&[0, 0], &node, inputs, LocalSubtree::Container)
            .unwrap();

        let bounds = Rect::new(30.25, 20.5, 16.0, 16.0);
        let clip = Rect::new(32.0, 21.0, 10.0, 11.0);
        let expected = clone_in_view(template, bounds, clip, None);
        let reused = clone_in_view_reusing(template, bounds, clip, None, children);
        assert_eq!(reused, expected);
        assert_eq!(reused.children[0].children.as_ptr(), reused_buffer);
    }

    #[test]
    fn scroll_projection_reuses_owned_descendant_storage() {
        let mut view = Node::new("ScrollView", None);
        view.children = vec![Node::new("Rectangle", None); 2];
        let view = view.into_shared();
        let mut cache = RowLayoutCache::default();
        let mut child = entry(0, 2);
        child.layout.children = vec![entry(0, 1).layout];
        let inputs = child.inputs;
        let bounds = Rect::new(10.25, 20.5, 16.0, 16.0);
        let second = Rect::new(10.25, 40.5, 16.0, 16.0);
        let clip = Rect::new(10.0, 26.0, 20.0, 12.0);
        let expected = [bounds, second].map(|rect| clone_in_view(&child.layout, rect, clip, None));
        let geometry = cache.scroll_geometry(&[], &view).unwrap();
        geometry.store_child(0, inputs, child.layout.clone(), false);
        geometry.store_child(1, inputs, child.layout, false);

        let mut previous = entry(0, 3).layout;
        let mut stale = expected[0].clone();
        stale.children[0].bounds.y = -20.0;
        stale.children[0].enabled = false;
        previous.children = vec![stale, expected[1].clone()];
        let old_buffer = previous.children.as_ptr();
        let old_descendants = previous.children[0].children.as_ptr();
        cache.use_recycled_layout(Some(previous));

        let resized = Rect::new(second.x, second.y, 20.0, second.height);
        let (geometry, reuse) = cache.scroll_geometry_for_projection(&[], &view, 2).unwrap();
        assert!(
            geometry
                .project_children(
                    [bounds, resized].into_iter(),
                    clip,
                    inputs,
                    None,
                    reuse,
                    |_, _, _| {}
                )
                .is_none()
        );
        let (geometry, reuse) = cache.scroll_geometry_for_projection(&[], &view, 2).unwrap();
        let projected = geometry
            .project_children(
                [bounds, second].into_iter(),
                clip,
                inputs,
                None,
                reuse,
                |_, _, _| {},
            )
            .unwrap();
        assert_eq!(projected.as_ptr(), old_buffer);
        assert_eq!(projected[0].children.as_ptr(), old_descendants);
        assert_eq!(projected, expected);
    }

    #[test]
    fn direct_and_nested_paths_keep_separate_cache_entries() {
        let mut layouts = LocalLayouts::default();
        assert!(layouts.insert(&[2], entry(1, 4)).is_none());
        assert!(layouts.insert(&[9], entry(1, 6)).is_none());
        assert!(layouts.insert(&[2, 0], entry(2, 5)).is_none());
        assert!(
            layouts
                .insert(&[MAX_INDEXED_CHILDREN], entry(2, 8))
                .is_none()
        );
        assert_eq!(layouts.insert(&[2], entry(2, 3)).unwrap().nodes, 4);
        assert_eq!(layouts.len(), 4);

        assert_eq!(layouts.retain_frame(2), 16);
        assert_eq!(layouts.len(), 3);
        assert_eq!(layouts.direct.len(), 3);
        assert_eq!(layouts.get(&[2]).unwrap().nodes, 3);
        assert!(layouts.get(&[9]).is_none());
        assert_eq!(layouts.get(&[2, 0]).unwrap().nodes, 5);
        assert_eq!(layouts.get(&[MAX_INDEXED_CHILDREN]).unwrap().nodes, 8);
    }

    #[test]
    fn scroll_views_share_and_release_the_content_node_budget() {
        let view = |nodes: usize| {
            let leaf = Node::new("Label", None).into_shared();
            let mut content = Node::new("VerticalLayout", None);
            content.children = vec![leaf; nodes - 1];
            let mut view = Node::new("ScrollView", None);
            view.children.push(content);
            view.into_shared()
        };
        let first_nodes = MAX_CACHED_SCROLL_NODES + 1;
        let first = view(first_nodes);
        let second = view(MAX_CACHED_LAYOUT_NODES - first_nodes + 1);
        let scene = Rc::new(Node::new("Window", None));
        let typography = Rc::new(());
        let mut cache = RowLayoutCache::default();
        cache.begin_frame(&scene, Some(typography.clone()));
        assert!(cache.scroll_geometry(&[0], &first).is_none());
        cache.finish_frame();
        cache.begin_frame(&scene, Some(typography.clone()));
        for index in 0..(MAX_CACHED_ROWS - 1) {
            cache.local_subtrees.insert(
                &[0, index],
                entry(cache.local_frame, MAX_LOCAL_SUBTREE_NODES),
            );
        }
        cache
            .local_subtrees
            .insert(&[1, 0], entry(cache.local_frame, MAX_LOCAL_SUBTREE_NODES));
        cache.local_nodes = MAX_CACHED_LOCAL_NODES;
        cache.local_visited = MAX_CACHED_ROWS;
        assert!(cache.scroll_geometry(&[0], &first).unwrap().caches_child(0));
        assert!(
            !cache
                .scroll_geometry(&[1], &second)
                .unwrap()
                .caches_child(0)
        );
        assert_eq!(cache.scroll_nodes, first_nodes);
        assert_eq!(cache.local_subtrees.len(), 1);
        assert_eq!(cache.local_nodes, MAX_LOCAL_SUBTREE_NODES);
        assert_eq!(cache.local_visited, 1);
        cache.finish_frame();

        cache.begin_frame(&scene, Some(typography.clone()));
        cache.finish_frame();
        assert_eq!(cache.scroll_nodes, 0);
        cache.begin_frame(&scene, Some(typography.clone()));
        assert!(
            cache
                .scroll_geometry(&[1], &second)
                .unwrap()
                .caches_child(0)
        );
    }

    #[test]
    fn long_scroll_view_keeps_its_unchanged_slots_across_an_edit() {
        let count = MAX_SCROLL_CAPTURE_ON_EDIT + 1;
        let leaf = Node::new("Label", None).into_shared();
        let mut view = Node::new("ScrollView", None);
        view.children = vec![leaf.clone(); count];
        let mut too_many = Node::new("ScrollView", None);
        too_many.children = vec![leaf; MAX_CACHED_SCROLL_SLOTS];
        let scene = Rc::new(Node::new("Window", None));
        let typography = Rc::new(());
        let mut cache = RowLayoutCache::default();
        cache.begin_frame(&scene, Some(typography.clone()));
        let pose = entry(1, 1);
        let geometry = cache.scroll_geometry(&[0], &view).unwrap();
        geometry.store_child(0, pose.inputs, pose.layout.clone(), false);
        geometry.store_child(count - 1, pose.inputs, pose.layout, false);
        geometry.measured = Some(ScrollMeasurement {
            size: Size::new(16.0, 16.0),
            mirrored: false,
            viewport: Rect::new(0.0, 0.0, 16.0, 16.0),
            extent: Size::new(16.0, 32.0),
            child_rects: Rc::from([]),
        });
        assert_eq!(cache.scroll_slots, count);
        assert!(cache.scroll_geometry(&[1], &too_many).is_none());
        cache.finish_frame();

        view.children[count - 1]
            .properties
            .insert("text", "A changed reply".to_owned().into());
        let scene = Rc::new(Node::new("Window", None));
        cache.begin_frame(&scene, Some(typography.clone()));
        let geometry = cache.scroll_geometry(&[0], &view).unwrap();
        assert!(geometry.child(0, pose.inputs).is_some());
        assert!(geometry.child(count - 1, pose.inputs).is_none());
        assert!(geometry.measured.is_none());
        assert_eq!(cache.scroll_nodes, count);
        assert_eq!(cache.scroll_slots, count);
        cache.finish_frame();

        cache.begin_frame(&scene, Some(typography));
        cache.finish_frame();
        assert_eq!(cache.scroll_slots, 0);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct AbsoluteLeafInputs {
    pub content: Rect,
    pub clip: Rect,
    pub enabled: bool,
    pub direction: Option<xrack_runtime::LayoutDirection>,
}

struct AbsoluteLeaf {
    identity: SharedNodeIdentity,
    inputs: AbsoluteLeafInputs,
    layout: LayoutNode,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Slot {
    pub row: RowKey,
    pub column: Option<usize>,
}

pub(super) struct ScrollGeometry {
    pub(super) measured: Option<ScrollMeasurement>,
    children: Vec<Option<Box<ScrollChild>>>,
    cached: usize,
    reserved_children: Vec<usize>,
    reserved_nodes: usize,
}

pub(super) struct ScrollChild {
    inputs: RowInputs,
    pub(super) layout: LayoutNode,
    pub(super) deferred_text: bool,
}

impl ScrollGeometry {
    pub(super) fn caches_child(&self, slot: usize) -> bool {
        self.reserved_children
            .get(slot)
            .is_some_and(|nodes| *nodes != 0)
    }

    fn retain_unchanged_children(&mut self, previous: &Node, view: &Node, spare: usize) -> bool {
        if previous.children.len() != view.children.len()
            || !previous.same_local_layout_inputs_at_node(view)
        {
            return false;
        }
        let limit = scroll_edit_limit(self.children.len());
        let mut changed = Vec::new();
        let mut discarded = 0;
        for (index, (old, new)) in previous.children.iter().zip(&view.children).enumerate() {
            if !old.same_frame_inputs(new) {
                if changed.len() == limit {
                    return false;
                }
                changed.push(index);
                discarded += usize::from(self.children[index].is_some());
            }
        }
        if changed.is_empty() {
            return true;
        }
        if discarded == self.cached {
            return false;
        }
        // Existing local layouts stay valid at a new scroll position, but
        // one changed child can move siblings or resize the content extent.
        self.measured = None;
        let released = changed
            .iter()
            .map(|&index| self.reserved_children[index])
            .sum::<usize>();
        let available = spare + released;
        let mut remaining = available;
        for index in changed {
            self.children[index] = None;
            self.reserved_children[index] =
                reserve_scroll_child(&view.children[index], &mut remaining);
        }
        self.cached -= discarded;
        self.reserved_nodes = self.reserved_nodes - released + (available - remaining);
        true
    }

    pub(super) fn uses_local_budget(&self) -> bool {
        self.reserved_nodes > MAX_CACHED_SCROLL_NODES
    }

    pub(super) fn child(&mut self, slot: usize, inputs: RowInputs) -> Option<&mut ScrollChild> {
        self.children
            .get_mut(slot)?
            .as_deref_mut()
            .filter(|child| child.inputs == inputs)
    }

    pub(super) fn store_child(
        &mut self,
        slot: usize,
        inputs: RowInputs,
        layout: LayoutNode,
        deferred_text: bool,
    ) {
        if let Some(entry) = self.children.get_mut(slot) {
            self.cached += usize::from(entry.is_none());
            *entry = Some(Box::new(ScrollChild {
                inputs,
                layout,
                deferred_text,
            }));
        }
    }

    pub(super) fn project_children(
        &mut self,
        rects: impl ExactSizeIterator<Item = Rect>,
        clip: Rect,
        inputs: RowInputs,
        ancestors: Option<&Rc<ClipChain>>,
        reuse: Option<&mut Vec<LayoutNode>>,
        mut prepare: impl FnMut(usize, Rect, &mut ScrollChild),
    ) -> Option<Vec<LayoutNode>> {
        if self.cached != rects.len() || self.children.len() != rects.len() {
            return None;
        }
        let count = rects.len();
        let mut fresh = Vec::new();
        let projected = reuse.unwrap_or(&mut fresh);
        projected.reserve(count.saturating_sub(projected.len()));
        for (slot, bounds) in rects.enumerate() {
            let inputs = RowInputs {
                size: Size::new(bounds.width, bounds.height),
                ..inputs
            };
            let local = self.child(slot, inputs)?;
            if local.deferred_text {
                prepare(slot, bounds, local);
            }
            if let Some(previous) = projected.get_mut(slot) {
                let children = take_child_storage(previous, local.layout.children.len());
                *previous = clone_in_view_reusing(&local.layout, bounds, clip, ancestors, children);
            } else {
                projected.push(clone_in_view(&local.layout, bounds, clip, ancestors));
            }
        }
        projected.truncate(count);
        // Until every child matches, the old slots still belong to the
        // recycling tree and remain available to ordinary placement.
        Some(std::mem::take(projected))
    }
}

pub(super) struct ScrollMeasurement {
    pub(super) size: Size,
    pub(super) mirrored: bool,
    pub(super) viewport: Rect,
    pub(super) extent: Size,
    pub(super) child_rects: Rc<[Rect]>,
}

struct ScrollViewEntry {
    identity: SharedNodeIdentity,
    source: Option<Node>,
    geometry: Option<ScrollGeometry>,
    checked: bool,
    used: bool,
}

fn scroll_edit_limit(slots: usize) -> usize {
    (slots / 4).min(32)
}

fn has_stable_scroll_children(previous: &Node, view: &Node) -> bool {
    let limit = scroll_edit_limit(view.children.len());
    previous.children.len() == view.children.len()
        && previous.same_local_layout_inputs_at_node(view)
        && previous
            .children
            .iter()
            .zip(&view.children)
            .filter(|(previous, current)| !previous.shares_storage_with(current))
            .take(limit + 1)
            .count()
            <= limit
}

#[derive(Default)]
pub struct RowLayoutCache {
    pub(super) defer_clipped_text: bool,
    scene: Weak<Node>,
    typography: Weak<()>,
    scene_changed: bool,
    changing_scene: bool,
    views: BTreeMap<Vec<usize>, BTreeMap<Slot, Row>>,
    entries: usize,
    stats: RowLayoutStats,
    scroll_views: BTreeMap<Vec<usize>, ScrollViewEntry>,
    scroll_nodes: usize,
    scroll_slots: usize,
    table_sizes: BTreeMap<Vec<usize>, crate::table::ColumnSizing>,
    local_subtrees: LocalLayouts,
    local_nodes: usize,
    local_frame: u64,
    local_visited: usize,
    absolute_leaves: Vec<AbsoluteLeaf>,
    next_absolute_leaf: usize,
    recycled_layout: Option<LayoutNode>,
}

impl RowLayoutCache {
    /// Window preparation can resolve offscreen text for programmatic scroll requests.
    #[doc(hidden)]
    pub fn set_defer_clipped_text(&mut self, defer: bool) {
        self.defer_clipped_text = defer;
    }

    #[doc(hidden)]
    pub fn use_recycled_layout(&mut self, layout: Option<LayoutNode>) {
        self.recycled_layout = layout;
    }

    pub fn stats(&self) -> RowLayoutStats {
        self.stats
    }

    pub(super) fn begin_frame(&mut self, scene: &Rc<Node>, typography: Option<Rc<()>>) {
        self.next_absolute_leaf = 0;
        self.local_frame = self.local_frame.wrapping_add(1);
        self.local_visited = 0;
        let typography = typography.as_ref().map_or_else(Weak::new, Rc::downgrade);
        if !self.typography.ptr_eq(&typography) {
            self.typography = typography;
            self.local_subtrees = LocalLayouts::default();
            self.local_nodes = 0;
            self.absolute_leaves.clear();
            self.views.clear();
            self.entries = 0;
            self.scroll_views.clear();
            self.scroll_nodes = 0;
            self.scroll_slots = 0;
            self.table_sizes.clear();
        }
        let scene = Rc::downgrade(scene);
        let changed = !self.scene.ptr_eq(&scene);
        self.changing_scene = changed && self.scene_changed;
        self.scene_changed = changed;
        if self.scene_changed {
            self.scene = scene;
            // Per-row entries validate the source key and actual layout inputs
            // on each lookup, so an unrelated scene edit need not drop them.
            if self.typography.strong_count() == 0 {
                self.scroll_views.clear();
                self.scroll_nodes = 0;
                self.scroll_slots = 0;
            }
            for sizing in self.table_sizes.values_mut() {
                sizing.invalidate_scene();
            }
        }
        for sizing in self.table_sizes.values_mut() {
            sizing.begin_frame();
        }
        for view in self.scroll_views.values_mut() {
            view.used = false;
        }
        for row in self.views.values_mut().flat_map(BTreeMap::values_mut) {
            row.used = false;
        }
    }

    pub(super) fn absolute_leaf_slot(&mut self) -> Option<usize> {
        let slot = self.next_absolute_leaf;
        if slot >= MAX_ABSOLUTE_LEAVES || self.typography.strong_count() == 0 {
            return None;
        }
        self.next_absolute_leaf += 1;
        Some(slot)
    }

    pub(super) fn absolute_leaf(
        &mut self,
        slot: usize,
        node: &Node,
        inputs: AbsoluteLeafInputs,
    ) -> Option<LayoutNode> {
        let leaf = self.absolute_leaves.get(slot)?;
        // Traversal order only selects the slot. Exact snapshot and parent
        // geometry checks still apply when a sibling enters or moves.
        if leaf.inputs != inputs || !leaf.identity.matches(node) {
            return None;
        }
        self.stats.subtree_reuses += 1;
        let layout = &leaf.layout;
        Some(layout.clone_with_placement(layout.bounds, layout.clip, None, Vec::new()))
    }

    pub(super) fn store_absolute_leaf(
        &mut self,
        slot: usize,
        node: &Node,
        inputs: AbsoluteLeafInputs,
        layout: &LayoutNode,
    ) {
        debug_assert!(layout.children.is_empty());
        let leaf = AbsoluteLeaf {
            identity: node.snapshot_identity(),
            inputs,
            layout: layout.clone(),
        };
        if let Some(previous) = self.absolute_leaves.get_mut(slot) {
            *previous = leaf;
        } else {
            self.absolute_leaves.push(leaf);
        }
    }

    pub(super) fn local_layout(
        &mut self,
        path: &[usize],
        node: &Node,
        inputs: RowInputs,
        kind: LocalSubtree,
    ) -> Option<(&LayoutNode, Option<Vec<LayoutNode>>)> {
        let row = self.local_subtrees.get_mut(path)?;
        if row.inputs != inputs || !kind.matches(&row.node, node) {
            return None;
        }
        self.local_visited += usize::from(row.last_frame != self.local_frame);
        row.last_frame = self.local_frame;
        self.stats.subtree_reuses += 1;
        // Each field is refreshed from the cached template. The old
        // slot provides the child storage when the shape fits.
        let old_children =
            recycled_children(&mut self.recycled_layout, path, row.layout.children.len())
                .map(std::mem::take);
        Some((&row.layout, old_children))
    }

    pub(super) fn table_sizing(
        &mut self,
        path: &[usize],
    ) -> Option<&mut crate::table::ColumnSizing> {
        // Only the immutable base scene populates these entries. Across scene
        // revisions the fit is reusable only for the same column definitions.
        if self.table_sizes.len() >= MAX_CACHED_SCROLL_VIEWS && !self.table_sizes.contains_key(path)
        {
            return None;
        }
        Some(self.table_sizes.entry(path.to_vec()).or_default())
    }

    pub(super) fn reuse_prepared_table(
        &mut self,
        path: &[usize],
        node: &mut Node,
    ) -> Option<usize> {
        self.table_sizes
            .get_mut(path)
            .map(|entry| entry.reuse_prepared_node(node))
    }

    pub(super) fn remember_prepared_table(&mut self, path: &[usize], slot: usize, node: &Node) {
        if let Some(entry) = self.table_sizes.get_mut(path) {
            entry.remember_prepared_node(slot, node);
        }
    }

    pub(super) fn prepared_table_rows(
        &mut self,
        path: &[usize],
        order: &xrack_runtime::TableColumnOrder,
        scope: u64,
        headers: bool,
        first: usize,
    ) -> Option<&mut crate::table::PreparedTableRows> {
        self.table_sizes
            .get_mut(path)
            .map(|entry| entry.prepare_rows(order, scope, headers, first))
    }

    pub(super) fn local_layout_node_count(
        &self,
        path: &[usize],
        node: &Node,
        kind: LocalSubtree,
    ) -> Option<usize> {
        if self.typography.strong_count() == 0 {
            return None;
        }
        let limit = (self.local_node_budget() - self.local_nodes_with(path, 0))
            .min(MAX_LOCAL_SUBTREE_NODES);
        if limit < kind.minimum_nodes() {
            return None;
        }
        let mut remaining = limit;
        if !scroll_child(node, &mut remaining) {
            return None;
        }
        let nodes = limit - remaining;
        (nodes >= kind.minimum_nodes()).then_some(nodes)
    }

    fn local_nodes_with(&self, path: &[usize], nodes: usize) -> usize {
        self.local_nodes - self.local_subtrees.get(path).map_or(0, |entry| entry.nodes) + nodes
    }

    fn local_node_budget(&self) -> usize {
        (MAX_CACHED_LAYOUT_NODES - self.scroll_nodes).min(MAX_CACHED_LOCAL_NODES)
    }

    pub(super) fn store_local_layout(
        &mut self,
        path: &[usize],
        node: &Node,
        inputs: RowInputs,
        layout: &LayoutNode,
        nodes: usize,
    ) {
        // Measuring this subtree may also have filled the cache with children.
        let total = self.local_nodes_with(path, nodes);
        if total > self.local_node_budget() {
            return;
        }
        let previous = self.local_subtrees.insert(
            path,
            LocalLayout {
                node: node.clone(),
                inputs,
                layout: layout.clone(),
                nodes,
                last_frame: self.local_frame,
            },
        );
        self.local_nodes = total;
        self.local_visited +=
            usize::from(previous.is_none_or(|entry| entry.last_frame != self.local_frame));
    }

    pub(super) fn scroll_geometry_for_projection(
        &mut self,
        path: &[usize],
        view: &Node,
        count: usize,
    ) -> Option<(&mut ScrollGeometry, Option<&mut Vec<LayoutNode>>)> {
        if self.recycled_layout.is_none() {
            return Some((self.scroll_geometry(path, view)?, None));
        }
        self.scroll_geometry(path, view)?;
        let reuse = recycled_children(&mut self.recycled_layout, path, count);
        let geometry = self.scroll_views.get_mut(path)?.geometry.as_mut()?;
        Some((geometry, reuse))
    }

    pub(super) fn scroll_geometry(
        &mut self,
        path: &[usize],
        view: &Node,
    ) -> Option<&mut ScrollGeometry> {
        if self.scroll_views.len() < MAX_CACHED_SCROLL_VIEWS
            && !self.scroll_views.contains_key(path)
        {
            self.scroll_views.insert(
                path.to_vec(),
                ScrollViewEntry {
                    identity: view.snapshot_identity(),
                    source: self.changing_scene.then(|| view.clone()),
                    geometry: None,
                    checked: false,
                    used: self.changing_scene,
                },
            );
        }
        let entry = self.scroll_views.get_mut(path)?;
        let mut observed_stable_children = false;
        if !entry.identity.matches(view) {
            observed_stable_children = entry.geometry.is_none()
                && entry
                    .source
                    .as_ref()
                    .is_some_and(|previous| has_stable_scroll_children(previous, view));
            let mut kept = false;
            if let (Some(source), Some(geometry)) = (entry.source.as_ref(), entry.geometry.as_mut())
            {
                let old_nodes = geometry.reserved_nodes;
                let spare = MAX_CACHED_LAYOUT_NODES - self.scroll_nodes - self.local_nodes;
                kept = geometry.retain_unchanged_children(source, view, spare);
                if kept {
                    self.scroll_nodes = self.scroll_nodes - old_nodes + geometry.reserved_nodes;
                }
            }
            if kept {
                entry.source = Some(view.clone());
            } else {
                entry.source = self.changing_scene.then(|| view.clone());
                if let Some(previous) = entry.geometry.take() {
                    self.scroll_nodes -= previous.reserved_nodes;
                    self.scroll_slots -= previous.children.len();
                }
                entry.checked = false;
                // A live view may first appear while its last child is
                // changing. Admit it after observing stable siblings.
                entry.used = self.changing_scene && !observed_stable_children;
            }
            entry.identity = view.snapshot_identity();
        }
        if !entry.used && !entry.checked {
            entry.checked = true;
            if view.children.len() <= MAX_CACHED_SCROLL_SLOTS - self.scroll_slots {
                let available = MAX_CACHED_LAYOUT_NODES - self.scroll_nodes - self.local_nodes;
                let reclaimable = if self.scene_changed {
                    0
                } else {
                    self.local_subtrees.descendant_nodes(path)
                };
                let budget = available + reclaimable;
                let mut remaining = budget;
                let reserved_children: Vec<_> = view
                    .children
                    .iter()
                    .map(|child| reserve_scroll_child(child, &mut remaining))
                    .collect();
                let reserved_nodes = budget - remaining;
                if self.scene_changed
                    && !observed_stable_children
                    && reserved_children
                        .iter()
                        .any(|&nodes| nodes > MAX_SCROLL_CAPTURE_ON_EDIT)
                {
                    // A large, changing subtree can make the whole capture
                    // obsolete. Independent small children can be captured
                    // immediately and retained across edits to their siblings.
                    entry.checked = false;
                    entry.source = Some(view.clone());
                } else {
                    if reserved_nodes > available || reserved_nodes > MAX_CACHED_SCROLL_NODES {
                        let (nodes, visited) = self
                            .local_subtrees
                            .evict_descendants(path, self.local_frame);
                        self.local_nodes -= nodes;
                        self.local_visited -= visited;
                    }
                    self.scroll_nodes += reserved_nodes;
                    self.scroll_slots += view.children.len();
                    entry.source = Some(view.clone());
                    entry.geometry = Some(ScrollGeometry {
                        measured: None,
                        children: std::iter::repeat_with(|| None)
                            .take(view.children.len())
                            .collect(),
                        cached: 0,
                        reserved_children,
                        reserved_nodes,
                    });
                }
            }
        }
        entry.used = true;
        entry.geometry.as_mut()
    }

    pub(super) fn get(
        &mut self,
        path: &[usize],
        slot: Slot,
        node: &Node,
        inputs: RowInputs,
    ) -> Option<&LayoutNode> {
        let row = self.views.get_mut(path)?.get_mut(&slot)?;
        if row.inputs != inputs || !row.node.same_frame_inputs(node) {
            return None;
        }
        row.used = true;
        self.stats.reused += 1;
        Some(&row.layout)
    }

    pub(super) fn insert(
        &mut self,
        path: &[usize],
        slot: Slot,
        node: &Node,
        inputs: RowInputs,
        layout: &LayoutNode,
    ) {
        self.stats.calculated += 1;
        if self.entries == MAX_CACHED_ROWS
            && !self
                .views
                .get(path)
                .is_some_and(|view| view.contains_key(&slot))
        {
            return;
        }
        let view = self.views.entry(path.to_vec()).or_default();
        let previous = view.insert(
            slot,
            Row {
                node: node.clone(),
                inputs,
                layout: layout.clone(),
                used: true,
            },
        );
        self.entries += usize::from(previous.is_none());
    }

    pub(super) fn finish_frame(&mut self) {
        self.absolute_leaves.truncate(self.next_absolute_leaf);
        let scroll_nodes = &mut self.scroll_nodes;
        let scroll_slots = &mut self.scroll_slots;
        self.scroll_views.retain(|_, view| {
            if !view.used
                && let Some(geometry) = &view.geometry
            {
                *scroll_nodes -= geometry.reserved_nodes;
                *scroll_slots -= geometry.children.len();
            }
            view.used
        });
        if self.scene_changed {
            self.table_sizes.retain(|_, sizing| sizing.used);
        }
        // No pruning is needed when every entry was visited or replaced.
        // This is common in frames that only reposition cached subtrees.
        if self.scene_changed && self.local_visited != self.local_subtrees.len() {
            self.local_nodes = self.local_subtrees.retain_frame(self.local_frame);
        }
        for sizing in self.table_sizes.values_mut() {
            sizing.finish_frame();
        }
        self.views.retain(|_, view| {
            let previous = view.len();
            view.retain(|_, row| row.used);
            self.entries -= previous - view.len();
            !view.is_empty()
        });
    }
}

pub(crate) fn cacheable_shared_row(node: &Node) -> bool {
    let mut remaining = MAX_LOCAL_SUBTREE_NODES;
    scroll_child(node, &mut remaining)
}

fn reserve_scroll_child(node: &Node, remaining: &mut usize) -> usize {
    let mut rest = *remaining;
    if scroll_child(node, &mut rest) {
        let reserved = *remaining - rest;
        *remaining = rest;
        reserved
    } else {
        0
    }
}

fn scroll_child(node: &Node, budget: &mut usize) -> bool {
    let Some(left) = budget.checked_sub(1) else {
        return false;
    };
    *budget = left;
    // These layouts use only positions local to the scroll content. Virtual
    // views, embedded viewports and overlays still resolve their own geometry.
    matches!(
        node.kind,
        "Empty"
            | "Rectangle"
            | "Pane"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "Label"
            | "Text"
            | "StyledText"
            | "Image"
            | "Path"
            | "Button"
            | "ButtonContent"
    ) && node.list.is_none()
        && !node.boolean("clip", false)
        && !node.scene_texture_only()
        && node
            .properties
            .keys()
            .all(|name| *name != "transform" && !name.starts_with("transform-"))
        && node
            .children
            .iter()
            .all(|child| scroll_child(child, budget))
}

pub(super) fn cacheable(node: &Node) -> bool {
    !node.scene_texture_only()
        && matches!(
            node.kind,
            "ListRow"
                | "TableCell"
                | "Row"
                | "Column"
                | "HorizontalLayout"
                | "VerticalLayout"
                | "GridLayout"
                | "Rectangle"
                | "Text"
                | "Label"
                | "Image"
                | "Button"
                | "CheckBox"
                | "Switch"
                | "SwitchThumb"
                | "ToggleIndicator"
        )
        && node.list.is_none()
        && !node.boolean("clip", false)
        && node
            .properties
            .keys()
            .all(|name| *name != "transform" && !name.starts_with("transform-"))
        && node.children.iter().all(cacheable)
}

fn recycled_children<'a>(
    layout: &'a mut Option<LayoutNode>,
    path: &[usize],
    count: usize,
) -> Option<&'a mut Vec<LayoutNode>> {
    let slot = path.iter().try_fold(layout.as_mut()?, |parent, &index| {
        parent.child_at_mut(index)
    })?;
    (slot.children.capacity() == count).then_some(&mut slot.children)
}

fn take_child_storage(node: &mut LayoutNode, count: usize) -> Option<Vec<LayoutNode>> {
    (node.children.capacity() == count).then(|| std::mem::take(&mut node.children))
}

pub(super) fn clone_in_view(
    layout: &LayoutNode,
    bounds: Rect,
    clip: Rect,
    ancestors: Option<&Rc<ClipChain>>,
) -> LayoutNode {
    clone_in_view_reusing(layout, bounds, clip, ancestors, None)
}

pub(super) fn clone_in_view_reusing(
    layout: &LayoutNode,
    bounds: Rect,
    clip: Rect,
    ancestors: Option<&Rc<ClipChain>>,
    reuse: Option<Vec<LayoutNode>>,
) -> LayoutNode {
    fn translate_children(
        layout: &LayoutNode,
        x: f32,
        y: f32,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
        reuse: Option<Vec<LayoutNode>>,
    ) -> Vec<LayoutNode> {
        let Some(first) = layout.children.first() else {
            return Vec::new();
        };
        let translate = |rect: Rect| Rect::new(rect.x + x, rect.y + y, rect.width, rect.height);
        // Absolute siblings often inherit exactly the same local clip.
        // Project it once for adjacent siblings at this placement.
        let mut local_clip = first.clip;
        let mut projected_clip = translate(local_clip).intersect(clip);
        let placements = layout.children.iter().map(|child| {
            if child.clip != local_clip {
                local_clip = child.clip;
                projected_clip = translate(local_clip).intersect(clip);
            }
            (child, translate(child.bounds), projected_clip)
        });
        let translate_child = |(child, bounds, child_clip): (&LayoutNode, Rect, Rect)| {
            child.clone_with_placement(
                bounds,
                child_clip,
                ancestors,
                translate_children(child, x, y, clip, ancestors, None),
            )
        };
        match reuse {
            Some(mut children) if children.len() == layout.children.len() => {
                for (target, (template, bounds, clip_at_child)) in
                    children.iter_mut().zip(placements)
                {
                    target.clone_placement_from(template, bounds, clip_at_child, ancestors);
                    if !target.children.is_empty() || !template.children.is_empty() {
                        let reuse = take_child_storage(target, template.children.len());
                        target.children =
                            translate_children(template, x, y, clip, ancestors, reuse);
                    }
                }
                children
            }
            Some(mut children) => {
                children.clear();
                children.extend(placements.map(translate_child));
                children
            }
            None => placements.map(translate_child).collect(),
        }
    }

    layout.clone_with_placement(
        bounds,
        clip,
        ancestors,
        translate_children(layout, bounds.x, bounds.y, clip, ancestors, reuse),
    )
}

pub(super) fn place_in_view(
    layout: &mut LayoutNode,
    bounds: Rect,
    clip: Rect,
    ancestors: Option<&Rc<ClipChain>>,
) {
    fn translate(
        layout: &mut LayoutNode,
        x: f32,
        y: f32,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
    ) {
        layout.bounds.x += x;
        layout.bounds.y += y;
        layout.clip.x += x;
        layout.clip.y += y;
        layout.clip = layout.clip.intersect(clip);
        if let Some(ancestors) = ancestors {
            layout.ancestor_shapes = Some(ancestors.clone());
        }
        for child in &mut layout.children {
            translate(child, x, y, clip, ancestors);
        }
    }
    layout.bounds = bounds;
    layout.clip = clip;
    if let Some(ancestors) = ancestors {
        layout.ancestor_shapes = Some(ancestors.clone());
    }
    for child in &mut layout.children {
        translate(child, bounds.x, bounds.y, clip, ancestors);
    }
}
