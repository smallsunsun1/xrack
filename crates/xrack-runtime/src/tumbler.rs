use crate::{ControlAction, ListModel, Node, RowKey, VecModel, list::MAX_ROWS};
use std::fmt;

/// A live model handle. Layout reads only the rows that intersect the wheel.
#[derive(Clone, PartialEq, Eq)]
pub struct TumblerItems(VecModel<String>);

impl TumblerItems {
    pub fn new(model: VecModel<String>) -> Self {
        Self(model)
    }
}

impl fmt::Debug for TumblerItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TumblerItems").finish_non_exhaustive()
    }
}

pub struct Tumbler<'a> {
    items: Option<&'a TumblerItems>,
    count: usize,
    selected: Option<usize>,
    visible: usize,
    wrap: bool,
}

impl<'a> Tumbler<'a> {
    pub fn from_node(node: &'a Node) -> Self {
        let items = node.tumbler_items.as_ref();
        let count = items.map_or_else(
            || node.int("count").unwrap_or(0).clamp(0, MAX_ROWS as i64) as usize,
            |items| items.0.len().min(MAX_ROWS),
        );
        let visible = node.int("visible-item-count").unwrap_or(5).clamp(1, 15) as usize | 1;
        let selected = (count > 0).then(|| {
            node.int("current-index")
                .unwrap_or(0)
                .clamp(0, count as i64 - 1) as usize
        });
        let wrap = count > 1 && node.boolean("wrap", count > visible);
        Self {
            items,
            count,
            selected,
            visible,
            wrap,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn visible(&self) -> usize {
        self.visible
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    pub fn structure_stamp(&self) -> (u64, u64) {
        self.items
            .map_or((0, self.count as u64), |items| items.0.structure_stamp())
    }

    pub fn row_key(&self, index: usize) -> Option<RowKey> {
        if index >= self.count {
            return None;
        }
        self.items.map_or_else(
            || Some((self.count as i64).row_key(index)),
            |items| items.0.row_key(index),
        )
    }

    pub fn label(&self, index: usize) -> Option<String> {
        if index >= self.count {
            return None;
        }
        self.items
            .map_or_else(|| Some(index.to_string()), |items| items.0.row(index))
    }

    pub fn index_at(&self, index: i64) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        if self.wrap {
            Some(index.rem_euclid(self.count as i64) as usize)
        } else {
            usize::try_from(index)
                .ok()
                .filter(|&index| index < self.count)
        }
    }

    pub fn step(&self, delta: i64) -> Option<usize> {
        let index = (self.selected? as i64).saturating_add(delta);
        self.index_at(if self.wrap {
            index
        } else {
            index.clamp(0, self.count as i64 - 1)
        })
    }

    pub fn select(&self, index: usize) -> Option<ControlAction> {
        Some(ControlAction::PickTumbler {
            index,
            row: self.row_key(index)?,
        })
    }
}
