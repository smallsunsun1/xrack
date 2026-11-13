use crate::{
    ChangeSignal, ControlAction, Node, Property, RowKey, RowSelection, VecModel, list::MAX_ROWS,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    rc::Rc,
    sync::{Arc, Weak},
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

/// Commands address the model's stable non-negative business keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeCommand {
    Expand(i64),
    Collapse(i64),
    ToggleExpanded(i64),
    ExpandRecursively { key: i64, depth: i64 },
    CollapseRecursively(i64),
    ExpandTo(i64),
}

#[doc(hidden)]
pub struct TreeExpansionChange {
    pub key: i64,
    pub expanded: bool,
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

#[doc(hidden)]
pub enum TreeControlChange {
    Selection(TreeSelectionChange),
    Expansion {
        change: TreeExpansionChange,
        current: Option<i64>,
    },
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
    depth: usize,
    preorder: usize,
    subtree_end: usize,
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
                depth: 0,
                preorder: 0,
                subtree_end: 0,
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
        let mut preorder = 0;
        let mut pending: Vec<_> = roots.iter().rev().map(|&index| (index, false)).collect();
        while let Some((index, exiting)) = pending.pop() {
            if exiting {
                entries[index].subtree_end = preorder;
                continue;
            }
            entries[index].preorder = preorder;
            entries[index].depth = entries[index]
                .parent
                .map_or(0, |parent| entries[parent].depth + 1);
            preorder += 1;
            pending.push((index, true));
            pending.extend(
                entries[index]
                    .children
                    .iter()
                    .rev()
                    .map(|&child| (child, false)),
            );
        }
        Self {
            model: model_id,
            entries,
            index,
            roots,
        }
    }

    fn contains_descendant(&self, root: usize, index: usize) -> bool {
        let root = &self.entries[root];
        let entry = &self.entries[index];
        root.preorder < entry.preorder && entry.preorder < root.subtree_end
    }

    fn roots_for(&self, root_key: i64, root: Option<usize>) -> &[usize] {
        if root_key == -1 {
            &self.roots
        } else {
            root.map_or(&[], |index| self.entries[index].children.as_slice())
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
    expanded: Arc<BTreeMap<i64, RowKey>>,
    root_key: i64,
    root: Option<usize>,
    available: Option<Arc<[bool]>>,
    filtering: bool,
    visible: Vec<TreeItem>,
    visible_indices: HashMap<i64, usize>,
}

impl TreeSnapshot {
    fn new(
        topology: Arc<Topology>,
        expanded: Arc<BTreeMap<i64, RowKey>>,
        root_key: i64,
        available: Option<Arc<[bool]>>,
        included: Option<Vec<bool>>,
    ) -> Self {
        let root = topology.index.get(&root_key).copied();
        let roots = topology.roots_for(root_key, root);
        let is_included = |index: usize| {
            available.as_ref().is_none_or(|rows| rows[index])
                && included.as_ref().is_none_or(|rows| rows[index])
        };
        let filtering = included.is_some();
        let mut visible = Vec::new();
        let mut pending: Vec<_> = roots
            .iter()
            .rev()
            .copied()
            .filter(|&index| is_included(index))
            .map(|index| (index, 0))
            .collect();
        while let Some((index, depth)) = pending.pop() {
            let node = &topology.entries[index];
            let has_children = node.children.iter().copied().any(is_included);
            let expanded =
                has_children && (filtering || expanded.get(&node.key) == Some(&node.row));
            visible.push(TreeItem {
                key: node.key,
                row: node.row,
                model_index: node.model_index,
                parent: node
                    .parent
                    .filter(|parent| Some(*parent) != root)
                    .map(|index| topology.entries[index].key),
                depth,
                has_children,
                expanded,
            });
            if expanded {
                pending.extend(
                    node.children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|&index| is_included(index))
                        .map(|index| (index, depth + 1)),
                );
            }
        }
        let visible_indices = visible
            .iter()
            .enumerate()
            .map(|(index, item)| (item.key, index))
            .collect();
        Self {
            topology,
            expanded,
            root_key,
            root,
            available,
            filtering,
            visible,
            visible_indices,
        }
    }

    pub fn rows(&self) -> &[TreeItem] {
        &self.visible
    }

    #[doc(hidden)]
    pub fn rows_in_range(&self, range: Range<usize>) -> &[TreeItem] {
        self.visible
            .get(range.start..range.end.min(self.visible.len()))
            .unwrap_or_default()
    }

    pub fn visible_index(&self, key: i64) -> Option<usize> {
        self.visible_indices.get(&key).copied()
    }

    pub fn visible_index_for_source(&self, source: usize) -> Option<usize> {
        self.visible_index(self.entry_for_source(source)?.key)
    }

    /// Resolve a rendered row only if it still appears in the live tree.
    #[doc(hidden)]
    pub fn event_source_index(&self, live: &Self, index: usize, row: RowKey) -> Option<usize> {
        let item = self.rows().get(index).filter(|item| item.row == row)?;
        let current = live.rows().get(live.visible_index(item.key)?)?;
        (current.row == row && current.model_index == item.model_index).then_some(item.model_index)
    }

    fn entry_for_source(&self, source: usize) -> Option<&Entry> {
        let index = self
            .topology
            .entries
            .binary_search_by_key(&source, |entry| entry.model_index)
            .ok()?;
        self.topology.entries.get(index)
    }

    pub(crate) fn position_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        if key.model != self.topology.model {
            return None;
        }
        if let Some(entry) = self.entry_for_source(source_index)
            && entry.row == key
        {
            return self.visible_index(entry.key);
        }
        self.visible.iter().position(|item| item.row == key)
    }

    pub fn root_key(&self) -> i64 {
        self.root_key
    }

    #[doc(hidden)]
    pub fn root_identity(&self) -> (i64, Option<RowKey>) {
        (
            self.root_key,
            self.root.map(|index| self.topology.entries[index].row),
        )
    }

    fn structural_index(&self, key: i64) -> Option<usize> {
        let index = *self.topology.index.get(&key)?;
        (self.root_key == -1
            || self
                .root
                .is_some_and(|root| self.topology.contains_descendant(root, index)))
        .then_some(index)
    }

    fn entry_index(&self, key: i64) -> Option<usize> {
        self.structural_index(key)
            .filter(|&index| self.is_available(index))
    }

    fn is_available(&self, index: usize) -> bool {
        self.available.as_ref().is_none_or(|rows| rows[index])
    }

    fn has_available_children(&self, index: usize) -> bool {
        self.topology.entries[index]
            .children
            .iter()
            .any(|&child| self.is_available(child))
    }

    fn root_entries(&self) -> &[usize] {
        self.topology.roots_for(self.root_key, self.root)
    }

    pub fn is_filtering(&self) -> bool {
        self.filtering
    }

    pub fn is_expanded(&self, key: i64) -> bool {
        if self.filtering {
            return self
                .visible_index(key)
                .is_some_and(|index| self.visible[index].expanded);
        }
        self.entry_index(key).is_some_and(|index| {
            let entry = &self.topology.entries[index];
            self.has_available_children(index) && self.expanded.get(&key) == Some(&entry.row)
        })
    }

    pub fn depth(&self, key: i64) -> Option<usize> {
        let index = self.entry_index(key)?;
        let base = self
            .root
            .map_or(0, |root| self.topology.entries[root].depth + 1);
        Some(self.topology.entries[index].depth - base)
    }

    #[doc(hidden)]
    pub fn query_depth(&self, key: i64) -> i64 {
        self.depth(key).map_or(-1, |depth| depth as i64)
    }

    #[doc(hidden)]
    pub fn query_row(&self, key: i64) -> i64 {
        self.visible_index(key).map_or(-1, |index| index as i64)
    }

    pub fn nearest_visible_ancestor(&self, key: i64) -> Option<i64> {
        let mut index = self.structural_index(key)?;
        loop {
            let entry = &self.topology.entries[index];
            if self.visible_index(entry.key).is_some() {
                return Some(entry.key);
            }
            index = entry.parent.filter(|index| Some(*index) != self.root)?;
        }
    }

    fn nearest_visible_after_expansion(
        &self,
        key: i64,
        expanded: &BTreeMap<i64, RowKey>,
    ) -> Option<i64> {
        let mut index = self.structural_index(key)?;
        let mut path = vec![index];
        while let Some(parent) = self.topology.entries[index]
            .parent
            .filter(|parent| Some(*parent) != self.root)
        {
            path.push(parent);
            index = parent;
        }
        let mut visible = None;
        for index in path.into_iter().rev() {
            if !self.is_available(index) {
                break;
            }
            let entry = &self.topology.entries[index];
            visible = Some(entry.key);
            if expanded.get(&entry.key) != Some(&entry.row) {
                break;
            }
        }
        visible
    }

    #[doc(hidden)]
    pub fn same_model(&self, other: &Self) -> bool {
        self.topology.model == other.topology.model
    }

    fn same_visible_rows(&self, other: &Self) -> bool {
        let identity = |item: &TreeItem| (item.key, item.row);
        std::ptr::eq(self, other)
            || self
                .rows()
                .iter()
                .map(identity)
                .eq(other.rows().iter().map(identity))
    }

    pub fn is_descendant(&self, key: i64, ancestor: i64) -> bool {
        let Some(index) = self.entry_index(key) else {
            return false;
        };
        let Some(ancestor) = self.entry_index(ancestor) else {
            return false;
        };
        self.topology.contains_descendant(ancestor, index)
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

struct FilterObservation {
    topology: Weak<Topology>,
    available: Option<Arc<[bool]>>,
    root: i64,
    query: Rc<str>,
    changed: Rc<ChangeSignal>,
}

#[doc(hidden)]
#[derive(Default)]
pub struct TreeState {
    source: Property<u64>,
    topology: Property<Arc<Topology>>,
    roles_changed: RefCell<Option<(u64, Rc<ChangeSignal>)>>,
    visibility_changed: RefCell<Option<Rc<ChangeSignal>>>,
    availability: Property<Arc<[bool]>>,
    filter_observation: RefCell<Option<FilterObservation>>,
    snapshot: Property<Arc<TreeSnapshot>>,
    expanded: Property<Arc<BTreeMap<i64, RowKey>>>,
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
        self.snapshot_with_root(model, roles, || -1)
    }

    pub fn snapshot_with_root<T>(
        &self,
        model: impl FnOnce() -> Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> i64,
    ) -> Arc<TreeSnapshot> {
        self.try_snapshot_with_root(model(), roles, || Ok::<_, std::convert::Infallible>(root()))
            .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_snapshot_with_root<T, E>(
        &self,
        model: Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Arc<TreeSnapshot>, E> {
        self.source.set_if_changed(model.edit_stamp().0);
        self.snapshot.try_get(|| {
            self.track_source();
            Ok(self.build_snapshot(self.topology(&model, roles), root()?, None, None))
        })
    }

    pub fn snapshot_filtered<T: 'static>(
        &self,
        model: impl FnOnce() -> Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> i64,
        query: impl FnOnce() -> String,
        field: fn(&T) -> &str,
    ) -> Arc<TreeSnapshot> {
        self.snapshot_projected(model, roles, root, None, Some(field), query)
    }

    pub fn snapshot_projected<T: 'static>(
        &self,
        model: impl FnOnce() -> Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> i64,
        visible: Option<fn(&T) -> bool>,
        field: Option<fn(&T) -> &str>,
        query: impl FnOnce() -> String,
    ) -> Arc<TreeSnapshot> {
        self.snapshot_projected_by(model, roles, root, visible, field, query)
    }

    /// Project using field selectors that may capture immutable schema paths.
    #[doc(hidden)]
    pub fn snapshot_projected_by<T: 'static>(
        &self,
        model: impl FnOnce() -> Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> i64,
        visible: Option<impl Fn(&T) -> bool + Clone + 'static>,
        field: Option<impl Fn(&T) -> &str + Clone + 'static>,
        query: impl FnOnce() -> String,
    ) -> Arc<TreeSnapshot> {
        self.try_snapshot_projected_by(
            model(),
            roles,
            || Ok::<_, std::convert::Infallible>(root()),
            visible,
            field,
            || Ok(query()),
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Resolve fallible view bindings without caching a substitute projection.
    #[doc(hidden)]
    pub fn try_snapshot_projected_by<T: 'static, E>(
        &self,
        model: Rc<VecModel<T>>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
        root: impl FnOnce() -> Result<i64, E>,
        visible: Option<impl Fn(&T) -> bool + Clone + 'static>,
        field: Option<impl Fn(&T) -> &str + Clone + 'static>,
        query: impl FnOnce() -> Result<String, E>,
    ) -> Result<Arc<TreeSnapshot>, E> {
        self.source.set_if_changed(model.edit_stamp().0);
        self.snapshot.try_get(|| {
            self.track_source();
            let root = root()?;
            let topology = self.topology(&model, roles.clone());
            let available = visible.map(|visible| {
                self.availability.get(|| {
                    let topology = self.topology(&model, roles);
                    let mut shown = vec![false; topology.entries.len()];
                    let mut pending = topology.roots.clone();
                    while let Some(index) = pending.pop() {
                        let entry = &topology.entries[index];
                        if model
                            .project_row_untracked(entry.model_index, &visible)
                            .unwrap_or(false)
                        {
                            shown[index] = true;
                            pending.extend_from_slice(&entry.children);
                        }
                    }
                    let shown: Arc<[bool]> = shown.into();
                    let observed = Arc::downgrade(&topology);
                    let reachable = shown.clone();
                    let signal = model.observe_projection_changes(
                        MAX_ROWS,
                        move |model_index, before, after| {
                            if visible(before) == visible(after) {
                                return false;
                            }
                            let Some(topology) = observed.upgrade() else {
                                return false;
                            };
                            topology
                                .entries
                                .binary_search_by_key(&model_index, |entry| entry.model_index)
                                .is_ok_and(|index| {
                                    topology.entries[index]
                                        .parent
                                        .is_none_or(|parent| reachable[parent])
                                })
                        },
                    );
                    signal.track();
                    *self.visibility_changed.borrow_mut() = Some(signal);
                    shown
                })
            });
            let query: Rc<str> = if field.is_some() {
                query()?.trim().to_lowercase().into()
            } else {
                Rc::from("")
            };
            let Some(field) = field.filter(|_| !query.is_empty()) else {
                self.filter_observation.borrow_mut().take();
                return Ok(self.build_snapshot(topology, root, available, None));
            };
            let branch = topology.index.get(&root).copied();
            let matches = |row: &T| field(row).to_lowercase().contains(query.as_ref());
            {
                let mut observation = self.filter_observation.borrow_mut();
                let topology_ref = Arc::downgrade(&topology);
                if observation.as_ref().is_none_or(|current| {
                    !Weak::ptr_eq(&current.topology, &topology_ref)
                        || match (&current.available, &available) {
                            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                            (None, None) => false,
                            _ => true,
                        }
                        || current.root != root
                        || current.query != query
                }) {
                    let needle = query.clone();
                    let observed = topology_ref.clone();
                    let shown = available.clone();
                    let field = field.clone();
                    *observation = Some(FilterObservation {
                        topology: topology_ref,
                        available: available.clone(),
                        root,
                        query: query.clone(),
                        changed: model.observe_projection_changes(
                            MAX_ROWS,
                            move |model_index, before, after| {
                                let Some(topology) = observed.upgrade() else {
                                    return false;
                                };
                                let Ok(index) = topology
                                    .entries
                                    .binary_search_by_key(&model_index, |entry| entry.model_index)
                                else {
                                    return false;
                                };
                                let in_scope = root == -1
                                    || branch.is_some_and(|branch| {
                                        topology.contains_descendant(branch, index)
                                    });
                                let matches =
                                    |row: &T| field(row).to_lowercase().contains(needle.as_ref());
                                in_scope
                                    && shown.as_ref().is_none_or(|rows| rows[index])
                                    && matches(before) != matches(after)
                            },
                        ),
                    });
                }
                observation.as_ref().unwrap().changed.track();
            }
            let mut included = vec![false; topology.entries.len()];
            let mut pending = topology.roots_for(root, branch).to_vec();
            while let Some(index) = pending.pop() {
                if available.as_ref().is_some_and(|rows| !rows[index]) {
                    continue;
                }
                let entry = &topology.entries[index];
                pending.extend_from_slice(&entry.children);
                if !model
                    .project_row_untracked(entry.model_index, matches)
                    .unwrap_or(false)
                {
                    continue;
                }
                let mut ancestor = Some(index);
                while let Some(index) = ancestor {
                    if Some(index) == branch || included[index] {
                        break;
                    }
                    included[index] = true;
                    ancestor = topology.entries[index].parent;
                }
            }
            Ok(self.build_snapshot(topology, root, available, Some(included)))
        })
    }

    fn track_source(&self) {
        self.source
            .get(|| unreachable!("tree source is initialized"));
    }

    fn topology<T>(
        &self,
        model: &VecModel<T>,
        roles: impl Fn(&T) -> (i64, i64) + Clone + 'static,
    ) -> Arc<Topology> {
        self.topology.get(|| {
            self.track_source();
            let model_id = model.structure_stamp().0;
            {
                let mut observation = self.roles_changed.borrow_mut();
                if let Some((_, signal)) = observation.as_ref().filter(|(id, _)| *id == model_id) {
                    signal.track();
                } else {
                    let signal = model.observe_projection(MAX_ROWS, roles.clone());
                    signal.track();
                    *observation = Some((model_id, signal));
                }
            }
            let topology = Arc::new(Topology::new(model, model_id, roles));
            self.selection.borrow_mut().retain_live(&topology);
            topology
        })
    }

    fn build_snapshot(
        &self,
        topology: Arc<Topology>,
        root: i64,
        available: Option<Arc<[bool]>>,
        included: Option<Vec<bool>>,
    ) -> Arc<TreeSnapshot> {
        Arc::new(TreeSnapshot::new(
            topology,
            self.expanded.get(|| Arc::new(BTreeMap::new())),
            root,
            available,
            included,
        ))
    }

    pub fn set_expanded(&self, tree: &TreeSnapshot, key: i64, row: RowKey, expanded: bool) -> bool {
        if !tree
            .visible_index(key)
            .and_then(|index| tree.rows().get(index))
            .is_some_and(|item| item.row == row && item.has_children && item.expanded != expanded)
        {
            return false;
        }
        let command = if expanded {
            TreeCommand::Expand(key)
        } else {
            TreeCommand::Collapse(key)
        };
        self.command(tree, command).is_some()
    }

    pub fn command(
        &self,
        tree: &TreeSnapshot,
        command: TreeCommand,
    ) -> Option<TreeExpansionChange> {
        let (expanded, change) = self.prepare_command(tree, command)?;
        self.expanded.set(expanded);
        Some(change)
    }

    /// Resolve the command and cursor against the same live tree before committing.
    #[doc(hidden)]
    pub fn try_apply_command<E>(
        &self,
        command: TreeCommand,
        live: impl FnOnce() -> Result<Arc<TreeSnapshot>, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<(TreeExpansionChange, Option<i64>)>, E> {
        let prepared = crate::try_prepare_action_view(|| {
            let tree = live()?;
            let Some((expanded, change)) = self.prepare_command(&tree, command) else {
                return Ok(None);
            };
            let current = current()?;
            let next = tree
                .nearest_visible_after_expansion(current, &expanded)
                .filter(|key| *key != current);
            Ok(Some((expanded, change, next)))
        })?;
        Ok(prepared.map(|(expanded, change, next)| {
            self.expanded.set(expanded);
            (change, next)
        }))
    }

    #[doc(hidden)]
    pub fn apply_command(
        &self,
        command: TreeCommand,
        live: impl FnOnce() -> Arc<TreeSnapshot>,
        current: impl FnOnce() -> i64,
    ) -> Option<(TreeExpansionChange, Option<i64>)> {
        self.try_apply_command(
            command,
            || Ok::<_, std::convert::Infallible>(live()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }

    fn prepare_command(
        &self,
        tree: &TreeSnapshot,
        command: TreeCommand,
    ) -> Option<(Arc<BTreeMap<i64, RowKey>>, TreeExpansionChange)> {
        if tree.is_filtering()
            || !self
                .topology
                .matches_current(|current| Arc::ptr_eq(current, &tree.topology))
            || tree.available.as_ref().is_some_and(|shown| {
                !self
                    .availability
                    .matches_current(|current| Arc::ptr_eq(current, shown))
            })
        {
            return None;
        }
        let topology = &tree.topology;
        let mut state = self.expanded.get(|| Arc::new(BTreeMap::new()));
        let expanded = Arc::make_mut(&mut state);
        expanded.retain(|key, row| topology.row_key(*key) == Some(*row));
        let (key, expand, pending, depth_limit) = match command {
            TreeCommand::Expand(key)
            | TreeCommand::Collapse(key)
            | TreeCommand::ToggleExpanded(key) => {
                let index = tree.entry_index(key)?;
                let entry = &topology.entries[index];
                let expand = match command {
                    TreeCommand::ToggleExpanded(_) => expanded.get(&key) != Some(&entry.row),
                    TreeCommand::Expand(_) => true,
                    _ => false,
                };
                (key, expand, vec![index], Some(1_usize))
            }
            TreeCommand::ExpandRecursively { key, depth } => {
                if depth < -1 || depth == 0 {
                    return None;
                }
                let roots = if key == -1 {
                    tree.root_entries().to_vec()
                } else {
                    vec![tree.entry_index(key)?]
                };
                (key, true, roots, usize::try_from(depth).ok())
            }
            TreeCommand::CollapseRecursively(key) => {
                let roots = if key == -1 {
                    tree.root_entries().to_vec()
                } else {
                    vec![tree.entry_index(key)?]
                };
                (key, false, roots, None)
            }
            TreeCommand::ExpandTo(key) => {
                let mut index = tree.entry_index(key)?;
                let mut ancestors = Vec::new();
                while let Some(parent) = topology.entries[index]
                    .parent
                    .filter(|parent| Some(*parent) != tree.root)
                {
                    ancestors.push(parent);
                    index = parent;
                }
                (key, true, ancestors, Some(1))
            }
        };
        let mut pending: Vec<_> = pending.into_iter().map(|index| (index, 0_usize)).collect();
        let mut changed = false;
        while let Some((index, depth)) = pending.pop() {
            let entry = &topology.entries[index];
            if !tree.is_available(index) || !tree.has_available_children(index) {
                continue;
            }
            if expand {
                changed |= expanded.insert(entry.key, entry.row) != Some(entry.row);
            } else {
                changed |= expanded.remove(&entry.key).is_some();
            }
            if depth_limit.is_none_or(|limit| depth + 1 < limit) {
                pending.extend(entry.children.iter().map(|&child| (child, depth + 1)));
            }
        }
        if !changed {
            return None;
        }
        Some((
            state,
            TreeExpansionChange {
                key,
                expanded: expand,
            },
        ))
    }

    pub fn selection(&self) -> Arc<BTreeMap<i64, RowKey>> {
        self.selection_changed.track();
        self.selection.borrow().keys.clone()
    }

    /// Resolve a scene's positional input to stable rows before changing tree state.
    #[doc(hidden)]
    pub fn try_apply_control<E>(
        &self,
        node: &Node,
        input: crate::InputRoute<'_>,
        available: impl FnOnce() -> Result<bool, E>,
        multiple: impl FnOnce() -> Result<bool, E>,
        live: impl FnOnce() -> Result<Arc<TreeSnapshot>, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<TreeControlChange>, E> {
        let crate::RoutedEvent::Control(action) = *input.event() else {
            return Ok(None);
        };
        let Some(snapshot) = &node.tree else {
            return Ok(None);
        };
        let snapshot_multiple = node.tree_selection.is_some();
        let action = match action {
            ControlAction::Select(index) => {
                let Some(item) = snapshot.rows().get(index) else {
                    return Ok(None);
                };
                TreeAction::Selection(TreeSelectionAction::Row {
                    key: item.key,
                    row: item.row,
                    operation: RowSelection::Replace,
                })
            }
            ControlAction::Tree(action @ TreeAction::SetExpanded { .. }) => action,
            ControlAction::Tree(action @ TreeAction::Selection(_)) if snapshot_multiple => action,
            _ => return Ok(None),
        };
        let prepared = crate::property::watch_reads(|reads| {
            if !available()? || reads.changed() {
                return Ok(None);
            }
            if matches!(action, TreeAction::Selection(_))
                && (multiple()? != snapshot_multiple || reads.changed())
            {
                return Ok(None);
            }
            let live = live()?;
            if reads.changed() {
                return Ok(None);
            }
            if let TreeAction::Selection(selection) = action
                && snapshot_multiple
            {
                let needs_order = matches!(
                    selection,
                    TreeSelectionAction::Row {
                        operation: RowSelection::Extend | RowSelection::AddRange,
                        ..
                    }
                );
                if !snapshot.same_model(&live)
                    || (needs_order && !snapshot.same_visible_rows(&live))
                {
                    return Ok(None);
                }
            }
            let current = current()?;
            Ok((input.is_current() && !reads.changed()).then_some((live, current)))
        })?;
        let Some((live, current)) = prepared else {
            return Ok(None);
        };
        Ok(match action {
            TreeAction::Selection(selection) => self
                .select_in_mode(&live, current, snapshot_multiple, selection)
                .map(TreeControlChange::Selection),
            TreeAction::SetExpanded { key, row, expanded } => self
                .set_expanded(&live, key, row, expanded)
                .then(|| TreeControlChange::Expansion {
                    change: TreeExpansionChange { key, expanded },
                    current: (!expanded && live.is_descendant(current, key)).then_some(key),
                }),
        })
    }

    #[doc(hidden)]
    pub fn apply_control(
        &self,
        node: &Node,
        input: crate::InputRoute<'_>,
        available: impl FnOnce() -> bool,
        multiple: impl FnOnce() -> bool,
        live: impl FnOnce() -> Arc<TreeSnapshot>,
        current: impl FnOnce() -> i64,
    ) -> Option<TreeControlChange> {
        self.try_apply_control(
            node,
            input,
            || Ok::<_, std::convert::Infallible>(available()),
            || Ok(multiple()),
            || Ok(live()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Resolve a source index against the same live tree used to update selection.
    #[doc(hidden)]
    pub fn try_select_source<E>(
        &self,
        source: usize,
        live: impl FnOnce() -> Result<Arc<TreeSnapshot>, E>,
        multiple: impl FnOnce() -> Result<bool, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<TreeSelectionChange>, E> {
        let prepared = crate::try_prepare_action_view(|| {
            let tree = live()?;
            let Some(item) = tree
                .visible_index_for_source(source)
                .and_then(|index| tree.rows().get(index))
            else {
                return Ok(None);
            };
            let action = TreeSelectionAction::Row {
                key: item.key,
                row: item.row,
                operation: RowSelection::Replace,
            };
            let multiple = multiple()?;
            let current = current()?;
            Ok(Some((tree, multiple, current, action)))
        })?;
        Ok(prepared.and_then(|(tree, multiple, current, action)| {
            self.select_in_mode(&tree, current, multiple, action)
        }))
    }

    #[doc(hidden)]
    pub fn select_source(
        &self,
        source: usize,
        live: impl FnOnce() -> Arc<TreeSnapshot>,
        multiple: impl FnOnce() -> bool,
        current: impl FnOnce() -> i64,
    ) -> Option<TreeSelectionChange> {
        self.try_select_source(
            source,
            || Ok::<_, std::convert::Infallible>(live()),
            || Ok(multiple()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn select_in_mode(
        &self,
        tree: &TreeSnapshot,
        current: i64,
        multiple: bool,
        action: TreeSelectionAction,
    ) -> Option<TreeSelectionChange> {
        if !self
            .snapshot
            .matches_current(|live| std::ptr::eq(live.as_ref(), tree))
        {
            return None;
        }
        if multiple {
            return self.select(tree, current, action);
        }
        let TreeSelectionAction::Row { key, row, .. } = action else {
            return None;
        };
        let item = tree.rows().get(tree.visible_index(key)?)?;
        (item.row == row && key != current).then_some(TreeSelectionChange {
            current: Some(key),
            selected_count: None,
        })
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
    fn recursive_commands_clear_hidden_descendants_and_preserve_depth_limits() {
        let model = Rc::new(VecModel::from(vec![
            (1, -1),
            (2, 1),
            (3, 2),
            (4, 1),
            (9, -1),
            (10, 9),
        ]));
        let state = TreeState::default();
        let tree = || state.snapshot(|| model.clone(), |row| *row);
        let initial = tree();
        assert_eq!(initial.depth(3), Some(2));
        assert_eq!(initial.depth(99), None);
        assert!(
            state
                .command(
                    &initial,
                    TreeCommand::ExpandRecursively { key: 1, depth: 1 }
                )
                .is_some()
        );
        assert!(tree().is_expanded(1));
        assert!(!tree().is_expanded(2));
        assert_eq!(tree().visible_index(3), None);

        assert!(state.command(&tree(), TreeCommand::ExpandTo(3)).is_some());
        assert_eq!(tree().visible_index(3), Some(2));
        assert!(state.command(&tree(), TreeCommand::Collapse(1)).is_some());
        assert!(tree().is_expanded(2));
        assert_eq!(tree().nearest_visible_ancestor(3), Some(1));
        assert!(
            state
                .command(&tree(), TreeCommand::CollapseRecursively(1))
                .is_some()
        );
        assert!(!tree().is_expanded(2));
        assert!(state.command(&tree(), TreeCommand::Expand(1)).is_some());
        assert_eq!(tree().visible_index(3), None);

        let before = tree();
        for command in [
            TreeCommand::Expand(99),
            TreeCommand::ExpandRecursively { key: 1, depth: 0 },
            TreeCommand::ExpandRecursively { key: 1, depth: -2 },
        ] {
            assert!(state.command(&before, command).is_none());
        }
        assert!(Arc::ptr_eq(&before, &tree()));
        assert!(state.command(&tree(), TreeCommand::ExpandTo(3)).is_some());
        let prior_model = tree();
        assert_eq!(model.remove(1), Some((2, 1)));
        model.push((2, 1));
        assert!(
            state
                .command(&prior_model, TreeCommand::CollapseRecursively(-1))
                .is_none()
        );
        assert!(!tree().is_expanded(2));
        assert_eq!(tree().visible_index(3), None);
    }

    #[test]
    fn changing_the_view_root_reuses_topology_and_limits_expansion_to_the_branch() {
        let model = Rc::new(VecModel::from(vec![
            (1, -1),
            (10, 1),
            (100, 10),
            (103, 100),
            (2, -1),
            (20, 2),
        ]));
        let state = TreeState::default();
        let requested = Property::default();
        let reads = Rc::new(Cell::new(0));
        let count = reads.clone();
        let roles = move |row: &(i64, i64)| {
            count.set(count.get() + 1);
            *row
        };
        let tree =
            || state.snapshot_with_root(|| model.clone(), roles.clone(), || requested.get(|| -1));
        assert!(
            state
                .command(
                    &tree(),
                    TreeCommand::ExpandRecursively { key: -1, depth: -1 }
                )
                .is_some()
        );
        let global = tree();
        assert_eq!(global.rows().len(), 6);

        requested.set(10);
        let source = tree();
        assert!(Arc::ptr_eq(&global.topology, &source.topology));
        assert_eq!(
            source.rows().iter().map(|row| row.key).collect::<Vec<_>>(),
            [100, 103]
        );
        assert_eq!(source.rows()[0].parent, None);
        assert_eq!(source.depth(100), Some(0));
        assert_eq!(source.depth(103), Some(1));
        assert_eq!(source.depth(10), None);
        assert_eq!(source.depth(20), None);
        assert!(!source.is_descendant(103, 10));
        assert!(state.command(&source, TreeCommand::ExpandTo(20)).is_none());
        assert!(
            state
                .command(&source, TreeCommand::CollapseRecursively(-1))
                .is_some()
        );

        requested.set(2);
        assert_eq!(tree().rows()[0].key, 20);
        assert_eq!(tree().rows()[0].depth, 0);
        requested.set(-1);
        let restored = tree();
        assert!(restored.is_expanded(1));
        assert!(restored.is_expanded(10));
        assert!(!restored.is_expanded(100));
        assert!(restored.is_expanded(2));
        assert_eq!(reads.get(), 6);

        requested.set(999);
        let missing = tree();
        assert!(missing.rows().is_empty());
        assert!(
            state
                .command(&missing, TreeCommand::CollapseRecursively(-1))
                .is_none()
        );
        assert_eq!(missing.depth(100), None);
        assert_eq!(missing.root_key(), 999);
    }

    #[test]
    fn filtering_uses_live_matches_and_preserves_the_saved_expansion() {
        type Row = (i64, i64, &'static str);
        let source = Property::default();
        let model = Rc::new(VecModel::from(vec![
            (1, -1, "project"),
            (2, 1, "src"),
            (3, 2, "Window.RS"),
            (4, 1, "notes"),
            (5, -1, "window outside"),
        ]));
        source.set(model.clone());
        let query = Property::default();
        query.set(" WINDOW ".to_owned());
        let state = TreeState::default();
        let read = || {
            state.snapshot_filtered(
                || source.get(|| unreachable!()),
                |row: &Row| (row.0, row.1),
                || 1,
                || query.get(String::new),
                |row| row.2,
            )
        };
        let filtered = read();
        assert!(filtered.is_filtering());
        assert_eq!(
            filtered
                .rows()
                .iter()
                .map(|row| row.key)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert!(filtered.is_expanded(2));
        assert!(state.command(&filtered, TreeCommand::Collapse(2)).is_none());
        assert!(model.set_row(3, (4, 1, "README")));
        assert!(model.set_row(4, (5, -1, "outside rename")));
        assert!(Arc::ptr_eq(&filtered, &read()));
        assert!(model.set_row(3, (4, 1, "window docs")));
        assert_eq!(read().visible_index(4), Some(2));

        query.set(String::new());
        let original = read();
        assert!(!original.is_filtering());
        assert!(!original.is_expanded(2));
        assert_eq!(original.visible_index(3), None);

        query.set("window".to_owned());
        let before = read();
        source.set(Rc::new(VecModel::from(vec![
            (1, -1, "new project"),
            (9, 1, "window"),
        ])));
        let replaced = read();
        assert!(!before.same_model(&replaced));
        assert_eq!(replaced.rows()[0].key, 9);
        assert_eq!(replaced.rows().len(), 1);
    }

    #[test]
    fn deep_ancestor_expansion_uses_the_topology_without_recursive_calls() {
        let model = Rc::new(VecModel::from(
            (0..4096).map(|key| (key, key - 1)).collect::<Vec<_>>(),
        ));
        let state = TreeState::default();
        let initial = state.snapshot(|| model.clone(), |row| *row);
        let topology = initial.topology.clone();
        assert!(
            state
                .command(&initial, TreeCommand::ExpandTo(4095))
                .is_some()
        );
        let expanded = state.snapshot(|| model.clone(), |row| *row);
        assert!(Arc::ptr_eq(&topology, &expanded.topology));
        assert_eq!(expanded.visible_index(4095), Some(4095));
        assert_eq!(expanded.depth(4095), Some(4095));
        assert!(
            state
                .command(&expanded, TreeCommand::CollapseRecursively(-1))
                .is_some()
        );
        let collapsed = state.snapshot(|| model.clone(), |row| *row);
        assert_eq!(collapsed.rows().len(), 1);
        assert!(!collapsed.is_expanded(4094));
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

    #[test]
    fn visibility_masks_branches_without_rebuilding_topology_or_losing_expansion() {
        type Record = (i64, i64, bool, &'static str);
        let model = Rc::new(VecModel::from(vec![
            (1, -1, true, "root"),
            (2, 1, true, "branch"),
            (3, 2, true, "leaf"),
            (4, -1, true, "other"),
        ]));
        let root = Property::default();
        let query = Property::default();
        let state = TreeState::default();
        let snapshot = || {
            state.snapshot_projected(
                || model.clone(),
                |row| (row.0, row.1),
                || root.get(|| -1),
                Some(|row: &Record| row.2),
                Some(|row: &Record| row.3),
                || query.get(String::new),
            )
        };
        let keys =
            |tree: &TreeSnapshot| tree.rows().iter().map(|item| item.key).collect::<Vec<_>>();
        let first = snapshot();
        assert!(state.command(&first, TreeCommand::ExpandTo(3)).is_some());
        let expanded = snapshot();
        assert_eq!(keys(&expanded), [1, 2, 3, 4]);
        assert!(Arc::ptr_eq(
            first.available.as_ref().unwrap(),
            expanded.available.as_ref().unwrap()
        ));
        let leaf = model.row_key(2).unwrap();
        state
            .select(
                &expanded,
                -1,
                TreeSelectionAction::Row {
                    key: 3,
                    row: leaf,
                    operation: RowSelection::Replace,
                },
            )
            .unwrap();

        model.set_row(1, (2, 1, false, "branch"));
        assert!(state.command(&expanded, TreeCommand::Collapse(2)).is_none());
        let hidden = snapshot();
        assert!(Arc::ptr_eq(&expanded.topology, &hidden.topology));
        assert_eq!(keys(&hidden), [1, 4]);
        assert_eq!(hidden.depth(3), None);
        assert_eq!(hidden.nearest_visible_ancestor(3), Some(1));
        assert!(!hidden.is_expanded(1));
        assert!(!hidden.is_filtering());
        assert!(
            state
                .command(&hidden, TreeCommand::CollapseRecursively(-1))
                .is_none()
        );
        assert_eq!(state.selection().get(&3), Some(&leaf));
        model.set_row(2, (3, 2, false, "leaf"));
        assert!(Arc::ptr_eq(&hidden, &snapshot()));

        query.set("needle".into());
        let filtered = snapshot();
        assert!(filtered.rows().is_empty());
        model.set_row(2, (3, 2, true, "needle"));
        assert!(Arc::ptr_eq(&filtered, &snapshot()));
        model.set_row(1, (2, 1, true, "branch"));
        assert_eq!(keys(&snapshot()), [1, 2, 3]);
        query.set(String::new());
        assert_eq!(keys(&snapshot()), [1, 2, 3, 4]);

        root.set(2);
        assert_eq!(keys(&snapshot()), [3]);
        model.set_row(0, (1, -1, false, "root"));
        assert!(snapshot().rows().is_empty());
        model.set_row(0, (1, -1, true, "root"));
        assert_eq!(keys(&snapshot()), [3]);
    }
}
