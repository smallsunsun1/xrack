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

#[derive(Clone, Copy, PartialEq)]
pub(super) struct AbsoluteLeafInputs {
    pub content: Rect,
    pub clip: Rect,
    pub enabled: bool,
    pub direction: Option<xrack_runtime::LayoutDirection>,
}

struct AbsoluteLeaf {
    node: Node,
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
    local_subtrees: BTreeMap<Vec<usize>, Row>,
    absolute_leaves: Vec<AbsoluteLeaf>,
    next_absolute_leaf: usize,
}

impl RowLayoutCache {
    pub fn stats(&self) -> RowLayoutStats {
        self.stats
    }

    pub(super) fn begin_frame(&mut self, scene: &Rc<Node>, typography: Option<Rc<()>>) {
        self.next_absolute_leaf = 0;
        let typography = typography.as_ref().map_or_else(Weak::new, Rc::downgrade);
        if !self.typography.ptr_eq(&typography) {
            self.typography = typography;
            self.local_subtrees.clear();
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
            for row in self.local_subtrees.values_mut() {
                row.used = false;
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
        if leaf.inputs != inputs || !leaf.node.shares_storage_with(node) {
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
            node: node.clone(),
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
    ) -> Option<&LayoutNode> {
        let row = self.local_subtrees.get_mut(path)?;
        if row.inputs != inputs || !kind.matches(&row.node, node) {
            return None;
        }
        row.used = true;
        self.stats.subtree_reuses += 1;
        Some(&row.layout)
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

    pub(super) fn can_store_local_layout(
        &self,
        path: &[usize],
        node: &Node,
        kind: LocalSubtree,
    ) -> bool {
        let mut remaining = MAX_LOCAL_SUBTREE_NODES;
        self.typography.strong_count() != 0
            && (self.local_subtrees.len() < MAX_CACHED_ROWS
                || self.local_subtrees.contains_key(path))
            && scroll_child(node, &mut remaining)
            && MAX_LOCAL_SUBTREE_NODES - remaining >= kind.minimum_nodes()
    }

    pub(super) fn store_local_layout(
        &mut self,
        path: &[usize],
        node: &Node,
        inputs: RowInputs,
        layout: &LayoutNode,
    ) {
        self.local_subtrees.insert(
            path.to_vec(),
            Row {
                node: node.clone(),
                inputs,
                layout: layout.clone(),
                used: true,
            },
        );
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
            self.local_subtrees.retain(|_, row| row.used);
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
    matches!(
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
    ) && node.list.is_none()
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
    fn translate_children(
        layout: &LayoutNode,
        x: f32,
        y: f32,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
    ) -> Vec<LayoutNode> {
        let Some(first) = layout.children.first() else {
            return Vec::new();
        };
        let translate = |rect: Rect| Rect::new(rect.x + x, rect.y + y, rect.width, rect.height);
        // Absolute siblings often inherit exactly the same local clip.
        // Project it once for adjacent siblings at this placement.
        let mut local_clip = first.clip;
        let mut projected_clip = translate(local_clip).intersect(clip);
        layout
            .children
            .iter()
            .map(|child| {
                if child.clip != local_clip {
                    local_clip = child.clip;
                    projected_clip = translate(local_clip).intersect(clip);
                }
                child.clone_with_placement(
                    translate(child.bounds),
                    projected_clip,
                    ancestors,
                    translate_children(child, x, y, clip, ancestors),
                )
            })
            .collect()
    }

    layout.clone_with_placement(
        bounds,
        clip,
        ancestors,
        translate_children(layout, bounds.x, bounds.y, clip, ancestors),
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
