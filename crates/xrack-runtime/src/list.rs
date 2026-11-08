use crate::{Node, VecModel};
use std::{ops::Range, rc::Weak};

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
        node.length("row-height").unwrap_or(32.0).clamp(12.0, 512.0)
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
