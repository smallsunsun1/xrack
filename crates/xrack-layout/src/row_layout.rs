use crate::{LayoutNode, Rect, Size};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use xrack_runtime::{Node, RowKey};

const MAX_CACHED_ROWS: usize = 512;

#[derive(Clone, Copy, Debug, Default)]
pub struct RowLayoutStats {
    pub calculated: u64,
    pub reused: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RowInputs {
    pub size: Size,
    pub visible: bool,
    pub enabled: bool,
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

#[derive(Default)]
pub struct RowLayoutCache {
    scene: Weak<Node>,
    views: BTreeMap<Vec<usize>, BTreeMap<Slot, Row>>,
    entries: usize,
    stats: RowLayoutStats,
}

impl RowLayoutCache {
    pub fn stats(&self) -> RowLayoutStats {
        self.stats
    }

    pub(super) fn begin_frame(&mut self, scene: &Rc<Node>) {
        let scene = Rc::downgrade(scene);
        if !self.scene.ptr_eq(&scene) {
            self.scene = scene;
            self.views.clear();
            self.entries = 0;
        }
        for row in self.views.values_mut().flat_map(BTreeMap::values_mut) {
            row.used = false;
        }
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
        self.views.retain(|_, view| {
            let previous = view.len();
            view.retain(|_, row| row.used);
            self.entries -= previous - view.len();
            !view.is_empty()
        });
    }
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
    ) && node.list.is_none()
        && !node.boolean("clip", false)
        && node
            .properties
            .keys()
            .all(|name| !name.starts_with("transform-"))
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
