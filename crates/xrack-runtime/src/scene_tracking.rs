use crate::{Node, RowKey};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

/// Identity of one node within a committed scene, independent of its path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SceneNodeId(u64);

impl SceneNodeId {
    pub fn number(self) -> u64 {
        self.0
    }

    fn allocate() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneChanges {
    pub inserted: usize,
    pub removed: usize,
    pub moved: usize,
    pub pooled_rows: usize,
    pub reused_rows: usize,
    pub measure: usize,
    pub transform: usize,
    pub clip: usize,
    pub paint: usize,
    pub input: usize,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
enum Slot {
    Root,
    Child(usize),
    Named(Cow<'static, str>, usize),
    Row(RowKey, usize),
    Mount(RowKey),
}

struct Entry {
    id: SceneNodeId,
    scene: Node,
    ordinal: usize,
    pooled: bool,
}

/// Reconciles committed scene nodes without using a row's position as its identity.
#[derive(Default)]
pub struct SceneIndex {
    entries: BTreeMap<(Option<SceneNodeId>, Slot), Entry>,
    paths: BTreeMap<Vec<usize>, SceneNodeId>,
    scene: Option<Node>,
    generation: u64,
    views: BTreeMap<SceneNodeId, crate::ListRowLocator>,
}

impl SceneIndex {
    pub fn node_at(&self, path: &[usize]) -> Option<SceneNodeId> {
        self.paths.get(path).copied()
    }

    pub fn is_pooled(&self, id: SceneNodeId) -> bool {
        self.entries
            .values()
            .any(|entry| entry.id == id && entry.pooled)
    }

    pub fn synchronize(&mut self, scene: &Node, generation: u64) -> SceneChanges {
        if self.generation == generation
            && self
                .scene
                .as_ref()
                .is_some_and(|old| old.shares_storage_with(scene))
        {
            return SceneChanges::default();
        }
        let mut changes = SceneChanges::default();
        let mut previous = std::mem::take(&mut self.entries);
        if self.generation != generation {
            changes.removed = previous.len();
            previous.clear();
        }
        self.generation = generation;
        self.paths.clear();
        self.views.clear();
        self.visit(
            scene,
            None,
            Slot::Root,
            &mut Vec::new(),
            &mut previous,
            &mut changes,
        );
        // A logical row can still be in the model after its renderer leaves
        // the viewport. Keep a bounded set of detached identities for it and
        // its children. A deleted model key is always retired.
        const MAX_POOLED_NODES: usize = 256;
        let mut kept = BTreeSet::new();
        for ((parent, slot), mut entry) in previous {
            let keep = kept.len() < MAX_POOLED_NODES
                && parent.is_some_and(|parent| {
                    kept.contains(&parent)
                        || match slot {
                            Slot::Row(key, _) => self.views.get(&parent).is_some_and(|locator| {
                                locator
                                    .first_position(&[(key.in_view(0), entry.ordinal)])
                                    .is_some()
                            }),
                            _ => false,
                        }
                });
            if keep {
                if !entry.pooled && matches!(slot, Slot::Row(..)) {
                    changes.pooled_rows += 1;
                }
                entry.pooled = true;
                kept.insert(entry.id);
                self.entries.insert((parent, slot), entry);
            } else {
                changes.removed += 1;
            }
        }
        self.scene = Some(scene.clone());
        changes
    }

    fn visit(
        &mut self,
        node: &Node,
        parent: Option<SceneNodeId>,
        slot: Slot,
        path: &mut Vec<usize>,
        previous: &mut BTreeMap<(Option<SceneNodeId>, Slot), Entry>,
        changes: &mut SceneChanges,
    ) {
        if node.kind == "Empty" {
            return;
        }
        let ordinal = path.last().copied().unwrap_or_default();
        let id = match previous.remove(&(parent, slot.clone())) {
            Some(old) if same_slot(&old.scene, node) => {
                changes.moved += usize::from(ordinal != old.ordinal);
                changes.reused_rows += usize::from(old.pooled && matches!(slot, Slot::Row(..)));
                compare(&old.scene, node, changes);
                old.id
            }
            old => {
                changes.removed += usize::from(old.is_some());
                changes.inserted += 1;
                SceneNodeId::allocate()
            }
        };
        self.paths.insert(path.clone(), id);
        self.entries.insert(
            (parent, slot),
            Entry {
                id,
                scene: node.clone(),
                ordinal,
                pooled: false,
            },
        );
        if let Some(locator) = &node.row_locator {
            self.views.insert(id, locator.clone());
        }
        let mut occurrences = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (index, child) in node.materialized_children() {
            let slot = if let Some(mount) = &child.loader_mount {
                Slot::Mount(mount.key)
            } else if let Some(row) = child.routing_row_key() {
                let occurrence = occurrences.entry(row).or_insert(0);
                let slot = Slot::Row(row, *occurrence);
                *occurrence += 1;
                slot
            } else if let Some(name) = child.element_id_owned() {
                let occurrence = names.entry(name.clone()).or_insert(0);
                let slot = Slot::Named(name, *occurrence);
                *occurrence += 1;
                slot
            } else {
                Slot::Child(index)
            };
            path.push(index);
            self.visit(child, Some(id), slot, path, previous, changes);
            path.pop();
        }
    }
}

fn same_slot(old: &Node, new: &Node) -> bool {
    old.kind == new.kind
        && old.element_id() == new.element_id()
        && old.routing_row_key() == new.routing_row_key()
        && old.loader_mount == new.loader_mount
}

fn compare(old: &Node, new: &Node, changes: &mut SceneChanges) {
    if old.shares_storage_with(new) {
        return;
    }
    let changed = |name| old.properties.get(name) != new.properties.get(name);
    changes.measure += usize::from(!old.same_own_layout_inputs(new, true, true));
    changes.transform += usize::from(
        [
            "x",
            "y",
            "transform-rotation",
            "transform",
            "transform-scale",
            "transform-scale-x",
            "transform-scale-y",
            "transform-origin-x",
            "transform-origin-y",
        ]
        .into_iter()
        .any(changed),
    );
    changes.clip += usize::from(
        [
            "clip",
            "border-width",
            "border-radius",
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ]
        .into_iter()
        .any(changed),
    );
    changes.input += usize::from(
        old.key_handlers != new.key_handlers
            || ["enabled", "visible", "z", "hit-test", "focusable"]
                .into_iter()
                .any(changed),
    );
    changes.paint +=
        usize::from(old.properties != new.properties || old.media_frame != new.media_frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, VecModel};

    #[test]
    fn row_ids_follow_reordering_and_are_replaced_after_removal() {
        let model = VecModel::from(vec!["one", "two"]);
        let mut scene = Node::new("ListView", None);
        for index in 0..2 {
            let mut row = Node::new("ListRow", None);
            row.row_key = model.row_key(index);
            scene.children.push(row);
        }
        let mut index = SceneIndex::default();
        assert_eq!(index.synchronize(&scene, 0).inserted, 3);
        let first = index.node_at(&[0]);
        scene.children.swap(0, 1);
        assert_eq!(index.synchronize(&scene, 0).moved, 2);
        assert_eq!(index.node_at(&[1]), first);
        let temporarily_removed = scene.children.pop().unwrap();
        assert_eq!(index.synchronize(&scene, 0).removed, 1);
        scene.children.push(temporarily_removed);
        assert_eq!(index.synchronize(&scene, 0).inserted, 1);
        assert_ne!(index.node_at(&[1]), first);
    }

    #[test]
    fn marks_a_paint_only_property_without_measurement() {
        let mut node = Node::new("Rectangle", None);
        node.mark_paint_only_background();
        let mut index = SceneIndex::default();
        index.synchronize(&node, 0);
        node.properties
            .insert("background", Color(0xff0000ff).into());
        let changes = index.synchronize(&node, 0);
        assert_eq!(changes.paint, 1);
        assert_eq!(changes.measure, 0);
        assert_eq!(index.synchronize(&node, 0), SceneChanges::default());
    }

    #[test]
    fn names_and_parent_instance_qualify_a_retained_node() {
        let mut view = Node::new("Window", None);
        view.children.push(Node::new("Rectangle", Some("first")));
        view.children.push(Node::new("Rectangle", Some("second")));
        let mut index = SceneIndex::default();
        index.synchronize(&view, 0);
        let id = index.node_at(&[0]);
        view.children.swap(0, 1);
        assert_eq!(index.synchronize(&view, 0).moved, 2);
        assert_eq!(index.node_at(&[1]), id);
        index.synchronize(&view, 1);
        assert_ne!(index.node_at(&[1]), id);
    }

    #[test]
    fn a_virtual_row_stays_identifiable_while_outside_the_viewport() {
        use std::rc::Rc;

        let model = Rc::new(VecModel::from(vec!["one", "two"]));
        let mut view = Node::new("ListView", None);
        let mut row = Node::new("ListRow", None);
        row.row_key = model.row_key(0);
        row.children.push(Node::new("Text", None));
        view.row_locator = Some(crate::ListRowLocator::new(model.clone()));
        view.children.push(row.clone());
        let mut index = SceneIndex::default();
        index.synchronize(&view, 0);
        let id = index.node_at(&[0, 0]);
        view.children.clear();
        assert_eq!(index.synchronize(&view, 0).pooled_rows, 1);
        assert!(index.node_at(&[0, 0]).is_none());
        view.children.push(row);
        assert_eq!(index.synchronize(&view, 0).reused_rows, 1);
        assert_eq!(index.node_at(&[0, 0]), id);
        model.remove(0);
        view.children.clear();
        assert_eq!(index.synchronize(&view, 0).removed, 2);
    }
}
