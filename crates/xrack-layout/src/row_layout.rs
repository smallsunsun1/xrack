use crate::{LayoutNode, Rect, Size};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use xrack_runtime::{Node, RowKey};

const MAX_CACHED_ROWS: usize = 512;
const MAX_CACHED_SCROLL_VIEWS: usize = 8;
const MAX_SCROLL_CONTENT_NODES: usize = 4096;
const MAX_LOCAL_SUBTREE_NODES: usize = 128;

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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Slot {
    pub row: RowKey,
    pub column: Option<usize>,
}

pub(super) struct ScrollGeometry {
    pub(super) measured: Option<ScrollMeasurement>,
    pub(super) children: BTreeMap<usize, (RowInputs, LayoutNode)>,
}

pub(super) struct ScrollMeasurement {
    pub(super) bounds: Rect,
    pub(super) mirrored: bool,
    pub(super) viewport: Rect,
    pub(super) extent: Size,
    pub(super) child_rects: Vec<Rect>,
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
    scroll_views: BTreeMap<Vec<usize>, Option<ScrollGeometry>>,
    table_sizes: BTreeMap<Vec<usize>, crate::table::ColumnSizing>,
    local_subtrees: BTreeMap<Vec<usize>, Row>,
}

impl RowLayoutCache {
    pub fn stats(&self) -> RowLayoutStats {
        self.stats
    }

    pub(super) fn begin_frame(&mut self, scene: &Rc<Node>, typography: Option<Rc<()>>) {
        let typography = typography.as_ref().map_or_else(Weak::new, Rc::downgrade);
        if !self.typography.ptr_eq(&typography) {
            self.typography = typography;
            self.local_subtrees.clear();
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
            self.scroll_views.clear();
            for sizing in self.table_sizes.values_mut() {
                sizing.invalidate_scene();
            }
            for row in self.local_subtrees.values_mut() {
                row.used = false;
            }
        }
        for row in self.views.values_mut().flat_map(BTreeMap::values_mut) {
            row.used = false;
        }
    }

    pub(super) fn local_layout(
        &mut self,
        path: &[usize],
        node: &Node,
        inputs: RowInputs,
        kind: LocalSubtree,
    ) -> Option<LayoutNode> {
        let row = self.local_subtrees.get_mut(path)?;
        if row.inputs != inputs || !kind.matches(&row.node, node) {
            return None;
        }
        row.used = true;
        self.stats.subtree_reuses += 1;
        Some(row.layout.clone())
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
        // A whole-content snapshot is useful while scrolling a stable scene.
        // Consecutive scene revisions would discard the snapshot every frame.
        if self.changing_scene {
            return None;
        }
        if self.scroll_views.len() < MAX_CACHED_SCROLL_VIEWS
            && !self.scroll_views.contains_key(path)
        {
            let mut budget = MAX_SCROLL_CONTENT_NODES;
            let cacheable = view
                .children
                .iter()
                .all(|child| scroll_child(child, &mut budget));
            self.scroll_views.insert(
                path.to_vec(),
                cacheable.then(|| ScrollGeometry {
                    measured: None,
                    children: BTreeMap::new(),
                }),
            );
        }
        self.scroll_views.get_mut(path)?.as_mut()
    }

    pub(super) fn get(
        &mut self,
        path: &[usize],
        slot: Slot,
        node: &Node,
        inputs: RowInputs,
    ) -> Option<LayoutNode> {
        let row = self.views.get_mut(path)?.get_mut(&slot)?;
        if row.inputs != inputs || !row.node.same_frame_inputs(node) {
            return None;
        }
        row.used = true;
        self.stats.reused += 1;
        Some(row.layout.clone())
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
        if self.scene_changed {
            self.table_sizes.retain(|_, sizing| sizing.used);
            self.local_subtrees.retain(|_, row| row.used);
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

pub(super) fn place_in_view(layout: &mut LayoutNode, bounds: Rect, clip: Rect) {
    fn translate(layout: &mut LayoutNode, x: f32, y: f32, clip: Rect) {
        layout.bounds.x += x;
        layout.bounds.y += y;
        layout.clip.x += x;
        layout.clip.y += y;
        layout.clip = layout.clip.intersect(clip);
        for child in &mut layout.children {
            translate(child, x, y, clip);
        }
    }
    layout.bounds = bounds;
    layout.clip = clip;
    for child in &mut layout.children {
        translate(child, bounds.x, bounds.y, clip);
    }
}
