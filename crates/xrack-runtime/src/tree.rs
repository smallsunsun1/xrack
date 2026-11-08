use crate::{ChangeSignal, Property, RowKey, RowSelection, VecModel, list::MAX_ROWS};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeAction {
    SetExpanded {
        key: i64,
        row: RowKey,
        expanded: bool,
    },
    Selection(TreeSelectionAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSelectionAction {
    Row {
        key: i64,
        row: RowKey,
        operation: RowSelection,
    },
    All,
    Clear,
}

#[doc(hidden)]
pub struct TreeSelectionChange {
    pub current: Option<i64>,
    pub selected_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeItem {
    pub key: i64,
    pub row: RowKey,
    pub model_index: usize,
    pub parent: Option<i64>,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    key: i64,
    row: RowKey,
    model_index: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct Topology {
    model: u64,
    entries: Vec<Entry>,
    index: HashMap<i64, usize>,
    roots: Vec<usize>,
}

impl Topology {
    fn new<T>(model: &VecModel<T>, model_id: u64, roles: impl Fn(&T) -> (i64, i64)) -> Self {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        let mut duplicates = HashSet::new();
        model.visit_rows_untracked(MAX_ROWS, |index, row, item| {
            let (key, parent) = roles(item);
            if key < 0 {
                return;
            }
            if !seen.insert(key) {
                duplicates.insert(key);
            }
            candidates.push((index, row, key, parent));
        });
        candidates.retain(|(_, _, key, _)| !duplicates.contains(key));
        let index: HashMap<_, _> = candidates
            .iter()
            .enumerate()
            .map(|(index, (_, _, key, _))| (*key, index))
            .collect();
        let mut entries: Vec<_> = candidates
            .into_iter()
            .map(|(model_index, row, key, parent)| Entry {
                key,
                row,
                model_index,
                parent: index.get(&parent).copied(),
                children: Vec::new(),
            })
            .collect();

        // Follow parent chains iteratively and break cycles at the smallest key.
        let mut state = vec![0_u8; entries.len()];
        let mut chain = Vec::new();
        for start in 0..entries.len() {
            let mut current = Some(start);
            while let Some(node) = current {
                if state[node] == 2 {
                    break;
                }
                if state[node] == 1 {
                    let cycle = chain
                        .iter()
                        .copied()
                        .rev()
                        .take_while(|&index| index != node);
                    let root = cycle
                        .chain(std::iter::once(node))
                        .min_by_key(|&index| entries[index].key)
                        .unwrap();
                    entries[root].parent = None;
                    break;
                }
                state[node] = 1;
                chain.push(node);
                current = entries[node].parent;
            }
            for node in chain.drain(..) {
                state[node] = 2;
            }
        }
        let mut roots = Vec::new();
        for child in 0..entries.len() {
            if let Some(parent) = entries[child].parent {
                entries[parent].children.push(child);
            } else {
                roots.push(child);
            }
        }
        Self {
            model: model_id,
            entries,
            index,
            roots,
        }
    }

    fn row_key(&self, key: i64) -> Option<RowKey> {
        self.index.get(&key).map(|&index| self.entries[index].row)
    }
}

/// The flattened navigation order. Row delegates are materialized separately by the viewport.
#[derive(Debug, PartialEq, Eq)]
pub struct TreeSnapshot {
    topology: Arc<Topology>,
    visible: Vec<TreeItem>,
    visible_indices: HashMap<i64, usize>,
}

impl TreeSnapshot {
    fn new(topology: Arc<Topology>, expanded: &BTreeMap<i64, RowKey>) -> Self {
        let mut visible = Vec::new();
        let mut pending: Vec<_> = topology
            .roots
            .iter()
            .rev()
            .map(|&index| (index, 0))
            .collect();
        while let Some((index, depth)) = pending.pop() {
            let node = &topology.entries[index];
            let has_children = !node.children.is_empty();
            let expanded = has_children && expanded.get(&node.key) == Some(&node.row);
            visible.push(TreeItem {
                key: node.key,
                row: node.row,
                model_index: node.model_index,
                parent: node.parent.map(|index| topology.entries[index].key),
                depth,
                has_children,
                expanded,
            });
            if expanded {
                pending.extend(node.children.iter().rev().map(|&index| (index, depth + 1)));
            }
        }
        let visible_indices = visible
            .iter()
            .enumerate()
            .map(|(index, item)| (item.key, index))
            .collect();
        Self {
            topology,
            visible,
            visible_indices,
        }
    }

    pub fn rows(&self) -> &[TreeItem] {
        &self.visible
    }

    pub fn visible_index(&self, key: i64) -> Option<usize> {
        self.visible_indices.get(&key).copied()
    }

    #[doc(hidden)]
    pub fn same_model(&self, other: &Self) -> bool {
        self.topology.model == other.topology.model
    }

    pub fn is_descendant(&self, key: i64, ancestor: i64) -> bool {
        let Some(&index) = self.topology.index.get(&key) else {
            return false;
        };
        let mut parent = self.topology.entries[index].parent;
        while let Some(index) = parent {
            let node = &self.topology.entries[index];
            if node.key == ancestor {
                return true;
            }
            parent = node.parent;
        }
        false
    }
}

#[derive(Default)]
struct Selection {
    keys: Arc<BTreeMap<i64, RowKey>>,
    anchor: Option<(i64, RowKey)>,
}

impl Selection {
    fn retain_live(&mut self, topology: &Topology) {
        let live = |key, row| topology.row_key(key) == Some(row);
        if self.keys.iter().any(|(&key, &row)| !live(key, row)) {
            Arc::make_mut(&mut self.keys).retain(|&key, row| live(key, *row));
        }
        self.anchor = self.anchor.filter(|&(key, row)| live(key, row));
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct TreeState {
    topology: Property<Arc<Topology>>,
    roles_changed: RefCell<Option<(u64, Rc<ChangeSignal>)>>,
    snapshot: Property<Arc<TreeSnapshot>>,
    expanded: Property<BTreeMap<i64, RowKey>>,
    selection: RefCell<Selection>,
    selection_changed: ChangeSignal,
}

impl TreeState {
    /// Read topology from a stable, pure projection of a row's key and parent key.
    pub fn snapshot<T>(
        &self,
        model: impl FnOnce() -> Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
    ) -> Arc<TreeSnapshot> {
        self.snapshot.get(|| {
            let topology = self.topology.get(|| {
                let model = model();
                let model_id = model.structure_stamp().0;
                {
                    let mut observation = self.roles_changed.borrow_mut();
                    if let Some((_, signal)) =
                        observation.as_ref().filter(|(id, _)| *id == model_id)
                    {
                        signal.track();
                    } else {
                        let signal = model.observe_projection(MAX_ROWS, roles.clone());
                        signal.track();
                        *observation = Some((model_id, signal));
                    }
                }
                let topology = Arc::new(Topology::new(&model, model_id, roles));
                self.selection.borrow_mut().retain_live(&topology);
                topology
            });
            Arc::new(TreeSnapshot::new(
                topology,
                &self.expanded.get(BTreeMap::new),
            ))
        })
    }

    pub fn set_expanded(&self, tree: &TreeSnapshot, key: i64, row: RowKey, expanded: bool) -> bool {
        if !tree
            .visible_index(key)
            .and_then(|index| tree.rows().get(index))
            .is_some_and(|item| item.row == row && item.has_children && item.expanded != expanded)
        {
            return false;
        }
        let mut state = self.expanded.get(BTreeMap::new);
        state.retain(|key, row| tree.topology.row_key(*key) == Some(*row));
        if expanded {
            state.insert(key, row);
        } else {
            state.remove(&key);
        }
        self.expanded.set_if_changed(state);
        true
    }

    pub fn selection(&self) -> Arc<BTreeMap<i64, RowKey>> {
        self.selection_changed.track();
        self.selection.borrow().keys.clone()
    }

    pub fn select(
        &self,
        tree: &TreeSnapshot,
        current: i64,
        action: TreeSelectionAction,
    ) -> Option<TreeSelectionChange> {
        let mut selection = self.selection.borrow_mut();
        let mut next = selection.keys.clone();
        let current = match action {
            TreeSelectionAction::Row {
                key,
                row,
                operation,
            } => {
                let index = tree.visible_index(key)?;
                if tree.rows()[index].row != row {
                    return None;
                }
                match operation {
                    RowSelection::Replace => {
                        next = Arc::new(BTreeMap::from([(key, row)]));
                        selection.anchor = Some((key, row));
                    }
                    RowSelection::Toggle => {
                        let selected = Arc::make_mut(&mut next);
                        if selected.remove(&key).is_none() {
                            selected.insert(key, row);
                        }
                        selection.anchor = Some((key, row));
                    }
                    RowSelection::Extend | RowSelection::AddRange => {
                        let start = selection
                            .anchor
                            .and_then(|(key, _)| tree.visible_index(key))
                            .or_else(|| tree.visible_index(current))
                            .unwrap_or(index);
                        let anchor = &tree.rows()[start];
                        selection.anchor = Some((anchor.key, anchor.row));
                        let rows = tree.rows()[start.min(index)..=start.max(index)]
                            .iter()
                            .map(|item| (item.key, item.row));
                        if operation == RowSelection::Extend {
                            next = Arc::new(rows.collect());
                        } else {
                            Arc::make_mut(&mut next).extend(rows);
                        }
                    }
                    RowSelection::Move => {}
                }
                (key != current).then_some(key)
            }
            TreeSelectionAction::All => {
                next = Arc::new(
                    tree.rows()
                        .iter()
                        .map(|item| (item.key, item.row))
                        .collect(),
                );
                selection.anchor = None;
                None
            }
            TreeSelectionAction::Clear => {
                next = Arc::new(BTreeMap::new());
                selection.anchor = None;
                None
            }
        };
        let selected_count = (next != selection.keys).then_some(next.len());
        if selected_count.is_some() {
            selection.keys = next;
            self.selection_changed.notify();
        }
        Some(TreeSelectionChange {
            current,
            selected_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn projection_handles_cycles_orphans_and_duplicates_without_rebuilding_on_expand() {
        let model = Rc::new(VecModel::from(vec![
            (9, 8),
            (8, 9),
            (10, 9),
            (3, -1),
            (3, -1),
            (6, 3),
            (7, 7),
        ]));
        let state = TreeState::default();
        let reads = Rc::new(Cell::new(0));
        let role_reads = reads.clone();
        let roles = move |item: &(i64, i64)| {
            role_reads.set(role_reads.get() + 1);
            *item
        };
        let tree = state.snapshot(|| model.clone(), roles.clone());
        assert_eq!(
            tree.rows().iter().map(|row| row.key).collect::<Vec<_>>(),
            [8, 6, 7]
        );
        assert!(tree.is_descendant(10, 8));
        assert!(!tree.is_descendant(8, 9));
        let root = tree.rows()[0];
        assert!(state.set_expanded(&tree, root.key, root.row, true));
        let expanded = state.snapshot(|| model.clone(), roles.clone());
        assert_eq!(reads.get(), 7);
        assert_eq!(
            expanded
                .rows()
                .iter()
                .map(|row| row.key)
                .collect::<Vec<_>>(),
            [8, 9, 6, 7]
        );
        assert!(model.set_row(2, (10, 6)));
        assert_eq!(reads.get(), 9);
        let reparented = state.snapshot(|| model.clone(), roles);
        assert_eq!(reads.get(), 16);
        assert!(!reparented.is_descendant(10, 8));
        assert!(reparented.is_descendant(10, 6));
        assert!(reparented.rows()[0].expanded);
    }

    #[test]
    fn payload_changes_leave_independent_tree_projections_clean() {
        let model = Rc::new(VecModel::from(vec![(1, -1, -1, 0), (2, 1, -1, 0)]));
        let source = Property::default();
        source.set(model.clone());
        let primary = TreeState::default();
        let alternate = TreeState::default();
        let roles = |row: &(i64, i64, i64, i64)| (row.0, row.1);
        let other_roles = |row: &(i64, i64, i64, i64)| (row.0, row.2);
        let primary_tree = || primary.snapshot(|| source.get(|| unreachable!()), roles);
        let alternate_tree = || alternate.snapshot(|| model.clone(), other_roles);
        let first = primary_tree();
        let second = alternate_tree();

        assert!(model.set_row(1, (2, 1, -1, 50)));
        assert!(Arc::ptr_eq(&first, &primary_tree()));
        assert!(Arc::ptr_eq(&second, &alternate_tree()));
        assert!(model.set_row(1, (2, 1, 1, 50)));
        assert!(Arc::ptr_eq(&first, &primary_tree()));
        let reparented = alternate_tree();
        assert!(!Arc::ptr_eq(&second, &reparented));
        assert!(reparented.is_descendant(2, 1));

        source.set(Rc::new(VecModel::from(vec![(3, -1, -1, 0)])));
        let replacement = primary_tree();
        assert!(!replacement.same_model(&first));
        assert_eq!(replacement.rows()[0].key, 3);
        assert!(model.set_row(1, (2, -1, -1, 50)));
        assert!(Arc::ptr_eq(&replacement, &primary_tree()));
        assert!(!alternate_tree().is_descendant(2, 1));
        assert!(model.sort_by(|left, right| right.0.cmp(&left.0)));
        assert_eq!(alternate_tree().rows()[0].key, 2);
        assert!(Arc::ptr_eq(&replacement, &primary_tree()));
    }
}
