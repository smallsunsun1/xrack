use crate::{Node, VecModel};
use std::{
    collections::HashMap,
    fmt,
    ops::Range,
    rc::{Rc, Weak},
};

pub(crate) const MAX_ROWS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey {
    pub(crate) model: u64,
    pub(crate) row: u64,
}

pub struct RowLifetime(pub(crate) Option<Weak<()>>);

impl RowLifetime {
    pub(crate) fn is_live(&self, key: RowKey, model: u64, count: usize) -> bool {
        key.model == model
            && match &self.0 {
                Some(lifetime) => lifetime.strong_count() != 0,
                None => key.row < count as u64,
            }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListMetrics {
    pub row_count: usize,
    pub first_row: usize,
}

impl ListMetrics {
    pub fn row_height(node: &Node) -> f32 {
        Self::clamp_row_height(node.length("row-height").unwrap_or(32.0))
    }

    pub fn clamp_row_height(height: f32) -> f32 {
        height.clamp(12.0, 512.0)
    }

    pub fn row_height_at(node: &Node, index: usize) -> f32 {
        node.row_heights
            .as_ref()
            .map_or_else(|| Self::row_height(node), |heights| heights.height(index))
    }

    pub fn selection(node: &Node) -> Option<i64> {
        if let Some(tree) = &node.tree {
            return tree
                .visible_index(node.int("current-key")?)
                .map(|index| index as i64);
        }
        node.int(if node.kind == "TableView" {
            "current-row"
        } else {
            "current-index"
        })
    }
}

/// Resolves stable model row identities without constructing their delegates.
#[derive(Clone)]
#[doc(hidden)]
pub struct ListRowLocator(Rc<dyn RowPositions>);

trait RowPositions {
    fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)>;
}

impl<M: ListModel> RowPositions for M {
    fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)> {
        let &(first, hint) = candidates.first()?;
        let count = self.row_count();
        if count == 0 || self.row_key(0).model != first.model {
            return None;
        }
        if hint < count && self.row_key(hint) == first {
            return Some((0, hint));
        }
        // Nearby insertions and removals are common, including in deep lists.
        let nearby = hint.saturating_sub(4)..count.min(hint.saturating_add(5));
        if let Some(index) = nearby
            .into_iter()
            .find(|&index| self.row_key(index) == first)
        {
            return Some((0, index));
        }

        // Resolve all fallback rows in one pass when a whole page was removed.
        let priorities: HashMap<_, _> = candidates
            .iter()
            .enumerate()
            .map(|(priority, &(key, _))| (key, priority))
            .collect();
        let mut best = None;
        let mut index = 0;
        self.visit_row_keys(0..count, |key| {
            if let Some(&priority) = priorities.get(&key)
                && best.is_none_or(|(previous, _)| priority < previous)
            {
                best = Some((priority, index));
            }
            index += 1;
        });
        best
    }
}

impl ListRowLocator {
    pub fn new(model: Rc<impl ListModel + 'static>) -> Self {
        Self(model)
    }

    pub fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)> {
        self.0.first_position(candidates)
    }
}

impl PartialEq for ListRowLocator {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ListRowLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListRowLocator")
            .finish_non_exhaustive()
    }
}

#[doc(hidden)]
pub fn standard_list_rows<T: Clone + PartialEq>(
    model: &VecModel<T>,
    range: Range<usize>,
    mut label: impl FnMut(T) -> String,
) -> Vec<Node> {
    range
        .filter_map(|index| {
            let value = model.row(index)?;
            let key = model.row_key(index)?;
            let mut item = Node::new("StandardListItem", None);
            item.row_key = Some(key);
            item.properties.insert("text", label(value).into());
            Some(item)
        })
        .collect()
}

#[doc(hidden)]
pub fn standard_table_rows<T, const COLUMNS: usize>(
    model: &VecModel<T>,
    range: Range<usize>,
    project: impl Fn(&T) -> [String; COLUMNS],
) -> Vec<Node> {
    range
        .filter_map(|index| {
            let key = model.row_key(index)?;
            let cells = model.project_row(index, &project)?;
            let mut row = Node::new("TableRow", None);
            row.row_key = Some(key);
            row.children.extend(cells.map(|text| {
                let mut cell = Node::new("TableCell", None);
                cell.properties.insert("text", text.into());
                cell
            }));
            Some(row)
        })
        .collect()
}

pub trait ListModel {
    type Item: Clone + PartialEq;

    fn row_count(&self) -> usize;
    fn item(&self, row: usize) -> Self::Item;
    fn row_key(&self, row: usize) -> RowKey;
    fn visit_items(&self, mut visit: impl FnMut(usize, RowKey, &Self::Item)) {
        for index in 0..self.row_count() {
            visit(index, self.row_key(index), &self.item(index));
        }
    }
    fn visit_row_keys(&self, range: Range<usize>, mut visit: impl FnMut(RowKey)) {
        for index in range {
            visit(self.row_key(index));
        }
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        (0..self.row_count()).find(|&index| self.row_key(index) == key)
    }
    fn row_lifetime(&self, key: RowKey) -> RowLifetime;
    fn structure_stamp(&self) -> (u64, u64);
}

impl ListModel for i64 {
    type Item = i64;

    fn row_count(&self) -> usize {
        (*self).clamp(0, MAX_ROWS as i64) as usize
    }

    fn item(&self, row: usize) -> Self::Item {
        row as i64
    }

    fn row_key(&self, row: usize) -> RowKey {
        RowKey {
            model: 0,
            row: row as u64,
        }
    }
    fn row_lifetime(&self, _: RowKey) -> RowLifetime {
        RowLifetime(None)
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        let index = usize::try_from(key.row).ok()?;
        (key.model == 0 && index < self.row_count()).then_some(index)
    }
    fn structure_stamp(&self) -> (u64, u64) {
        (0, self.row_count() as u64)
    }
}

impl<T: Clone + PartialEq> ListModel for VecModel<T> {
    type Item = T;

    fn row_count(&self) -> usize {
        self.len().min(MAX_ROWS)
    }
    fn visit_items(&self, visit: impl FnMut(usize, RowKey, &Self::Item)) {
        self.visit_row_data(self.row_count(), visit);
    }

    fn item(&self, row: usize) -> Self::Item {
        self.row(row).expect("row is inside the model range")
    }

    fn row_key(&self, row: usize) -> RowKey {
        self.row_key(row).expect("row is inside the model range")
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        self.row_lifetime(key)
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        self.row_index(key).filter(|&index| index < MAX_ROWS)
    }
    fn visit_row_keys(&self, range: Range<usize>, visit: impl FnMut(RowKey)) {
        self.visit_row_keys(range, visit);
    }
    fn structure_stamp(&self) -> (u64, u64) {
        self.structure_stamp()
    }
}
