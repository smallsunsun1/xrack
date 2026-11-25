use crate::{ClipChain, LayoutNode, Rect, Size};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use xrack_runtime::{LayoutDirection, Node, RowKey, SharedNodeIdentity};

const MAX_CACHED_ROWS: usize = 512;
const MAX_CACHED_SCROLL_VIEWS: usize = 8;
const MAX_SCROLL_CONTENT_NODES: usize = 4096;
const MAX_LOCAL_SUBTREE_NODES: usize = 128;
// Preserve the previous worst-case number of cached layout nodes while
// allowing more small subtrees to share the budget.
const MAX_CACHED_LOCAL_NODES: usize = MAX_CACHED_ROWS * MAX_LOCAL_SUBTREE_NODES;
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

        let bounds = Rect::new(30.25, 20.5, 16.0, 16.0);
        let clip = Rect::new(32.0, 21.0, 10.0, 11.0);
        let expected = clone_in_view(&template, bounds, clip, None);
        let reused = clone_in_view_reusing(&template, bounds, clip, None, Some(previous));
        assert_eq!(reused, expected);
        assert_eq!(reused.children[0].children.as_ptr(), reused_buffer);
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
    children: Vec<Option<Box<(RowInputs, LayoutNode)>>>,
    cached: usize,
    pub(super) cacheable_children: Vec<bool>,
}

impl ScrollGeometry {
    pub(super) fn child(&self, slot: usize, inputs: RowInputs) -> Option<&LayoutNode> {
        let (last, layout) = self.children.get(slot)?.as_deref()?;
        (*last == inputs).then_some(layout)
    }

    pub(super) fn store_child(&mut self, slot: usize, inputs: RowInputs, layout: LayoutNode) {
        if let Some(entry) = self.children.get_mut(slot) {
            self.cached += usize::from(entry.is_none());
            *entry = Some(Box::new((inputs, layout)));
        }
    }

    pub(super) fn project_children(
        &self,
        rects: impl ExactSizeIterator<Item = Rect>,
        clip: Rect,
        visible: bool,
        enabled: bool,
        layout_direction: Option<LayoutDirection>,
        ancestors: Option<&Rc<ClipChain>>,
    ) -> Option<Vec<LayoutNode>> {
        if self.cached != rects.len() || self.children.len() != rects.len() {
            return None;
        }
        let mut projected = Vec::with_capacity(rects.len());
        for (slot, bounds) in rects.enumerate() {
            let inputs = RowInputs {
                size: Size::new(bounds.width, bounds.height),
                visible,
                enabled,
                layout_direction,
            };
            let local = self.child(slot, inputs)?;
            projected.push(clone_in_view(local, bounds, clip, ancestors));
        }
        Some(projected)
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

#[derive(Default)]
pub struct RowLayoutCache {
    scene: Weak<Node>,
    typography: Weak<()>,
    scene_changed: bool,
    changing_scene: bool,
    views: BTreeMap<Vec<usize>, BTreeMap<Slot, Row>>,
    entries: usize,
    stats: RowLayoutStats,
    scroll_views: BTreeMap<Vec<usize>, ScrollViewEntry>,
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
        let old_children = if let [index] = path {
            self.recycled_layout.as_mut().and_then(|recycled| {
                let slot = recycled.children.get_mut(*index)?;
                (slot.children.capacity() == row.layout.children.len())
                    .then(|| std::mem::take(&mut slot.children))
            })
        } else {
            None
        };
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
        let limit =
            (MAX_CACHED_LOCAL_NODES - self.local_nodes_with(path, 0)).min(MAX_LOCAL_SUBTREE_NODES);
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
        if total > MAX_CACHED_LOCAL_NODES {
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
                    source: None,
                    geometry: None,
                    checked: false,
                    used: self.changing_scene,
                },
            );
        }
        let entry = self.scroll_views.get_mut(path)?;
        if !entry.identity.matches(view) {
            if entry
                .source
                .as_ref()
                .is_some_and(|source| source.same_local_layout_inputs(view))
            {
                entry.source = Some(view.clone());
            } else {
                entry.source = None;
                entry.geometry = None;
                entry.checked = false;
                // Defer new captures while this view keeps changing. Other
                // views can keep their cached measurements across scene edits.
                entry.used = self.changing_scene;
            }
            entry.identity = view.snapshot_identity();
        }
        if !entry.used && !entry.checked {
            entry.checked = true;
            if view.children.len() <= MAX_SCROLL_CONTENT_NODES {
                let mut budget = MAX_SCROLL_CONTENT_NODES;
                entry.source = Some(view.clone());
                entry.geometry = Some(ScrollGeometry {
                    measured: None,
                    children: std::iter::repeat_with(|| None)
                        .take(view.children.len())
                        .collect(),
                    cached: 0,
                    cacheable_children: view
                        .children
                        .iter()
                        .map(|child| scroll_child(child, &mut budget))
                        .collect(),
                });
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
        self.scroll_views.retain(|_, view| view.used);
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
                        let reuse = (target.children.capacity() == template.children.len())
                            .then(|| std::mem::take(&mut target.children));
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
