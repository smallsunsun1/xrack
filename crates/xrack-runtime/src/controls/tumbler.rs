use crate::{
    ComboItems, ComboOptionInput, ControlAction, ListModel, Node, RowKey, Value, VecModel,
    list::MAX_ROWS,
};
use std::fmt;

/// A live model handle. Layout reads only the rows that intersect the wheel.
#[derive(Clone)]
pub struct TumblerItems(Items);

#[derive(Clone)]
enum Items {
    Strings(VecModel<String>),
    Records {
        items: ComboItems,
        structure: (u64, u64),
        revision: u64,
    },
}

impl TumblerItems {
    pub fn new(model: VecModel<String>) -> Self {
        Self(Items::Strings(model))
    }

    fn records(items: ComboItems) -> Self {
        let (structure, revision) = items.stamp();
        Self(Items::Records {
            items,
            structure,
            revision,
        })
    }

    fn count(&self) -> usize {
        match &self.0 {
            Items::Strings(model) => model.len().min(MAX_ROWS),
            Items::Records { items, .. } => items.wheel_count(),
        }
    }

    fn structure_stamp(&self) -> (u64, u64) {
        match &self.0 {
            Items::Strings(model) => model.structure_stamp(),
            Items::Records { structure, .. } => *structure,
        }
    }

    fn row_key(&self, index: usize) -> Option<RowKey> {
        match &self.0 {
            Items::Strings(model) => model.row_key(index),
            Items::Records { items, .. } => items.row_key(index),
        }
    }

    fn label(&self, index: usize) -> Option<String> {
        match &self.0 {
            Items::Strings(model) => model.row(index),
            Items::Records { items, .. } => items.selected_text(index),
        }
    }
}

impl PartialEq for TumblerItems {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Items::Strings(left), Items::Strings(right)) => left == right,
            (
                Items::Records {
                    items: left,
                    structure,
                    revision,
                },
                Items::Records {
                    items: right,
                    structure: other_structure,
                    revision: other_revision,
                },
            ) => {
                left.same_projection(right)
                    && structure == other_structure
                    && revision == other_revision
            }
            _ => false,
        }
    }
}

impl Eq for TumblerItems {}

impl Node {
    #[doc(hidden)]
    pub fn set_tumbler_items(&mut self, items: TumblerItems) {
        self.tumbler_items = Some(items);
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
            || node.int("count").unwrap_or(0).row_count(),
            TumblerItems::count,
        );
        let visible = node.int("visible-item-count").unwrap_or(5).clamp(1, 15) as usize | 1;
        let selected = clamped_index(count, node.int("current-index").unwrap_or(0));
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
            .map_or((0, self.count as u64), TumblerItems::structure_stamp)
    }

    #[doc(hidden)]
    pub fn projection_revision(&self) -> u64 {
        match self.items.map(|items| &items.0) {
            Some(Items::Records { revision, .. }) => *revision,
            _ => 0,
        }
    }

    pub fn row_key(&self, index: usize) -> Option<RowKey> {
        if index >= self.count {
            return None;
        }
        self.items.map_or_else(
            || Some((self.count as i64).row_key(index)),
            |items| items.row_key(index),
        )
    }

    pub fn label(&self, index: usize) -> Option<String> {
        if index >= self.count {
            return None;
        }
        self.items
            .map_or_else(|| Some(index.to_string()), |items| items.label(index))
    }

    #[doc(hidden)]
    pub fn option_input(
        &self,
        index: usize,
        text: String,
        displacement: f64,
        enabled: bool,
    ) -> ComboOptionInput {
        let records = match self.items.map(|items| &items.0) {
            Some(Items::Records { items, .. }) => Some(items),
            _ => None,
        };
        ComboOptionInput::for_tumbler(
            index,
            text,
            self.selected == Some(index),
            enabled,
            displacement,
            self.items.is_none(),
            records,
        )
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

    #[doc(hidden)]
    pub fn prepare_record_selection(
        node: &Node,
        action: ControlAction,
        items: ComboItems,
        current_index: i64,
        current_value: Option<&Value>,
    ) -> Option<(i64, Value)> {
        let ControlAction::PickTumbler { index, row } = action else {
            return None;
        };
        let current_items = TumblerItems::records(items.clone());
        if node.tumbler_items.as_ref() != Some(&current_items)
            || index >= current_items.count()
            || items.row_key(index) != Some(row)
        {
            return None;
        }
        let value = items.selected_value(index)?;
        let changed = current_value.map_or_else(
            || items.wheel_index(current_index, None) != Some(index),
            |current| *current != value,
        );
        changed.then_some((index as i64, value))
    }

    /// Resolve a row from an input scene against the control's current bindings.
    #[doc(hidden)]
    pub fn try_prepare_selection<E>(
        node: &'a Node,
        action: ControlAction,
        available: impl FnOnce() -> Result<bool, E>,
        items: impl FnOnce() -> Result<Option<TumblerItems>, E>,
        count: impl FnOnce() -> Result<i64, E>,
        current: impl FnOnce() -> Result<i64, E>,
    ) -> Result<Option<i64>, E> {
        let ControlAction::PickTumbler { index, row } = action else {
            return Ok(None);
        };
        crate::try_prepare_index_selection(
            index,
            available,
            |_| {
                if items()? != node.tumbler_items {
                    return Ok(false);
                }
                let wheel = Self::from_node(node);
                Ok(wheel.row_key(index) == Some(row)
                    && (wheel.items.is_some() || index < count()?.row_count()))
            },
            current,
        )
    }

    #[doc(hidden)]
    pub fn prepare_selection(
        node: &'a Node,
        action: ControlAction,
        available: impl FnOnce() -> bool,
        items: impl FnOnce() -> Option<TumblerItems>,
        count: impl FnOnce() -> i64,
        current: impl FnOnce() -> i64,
    ) -> Option<i64> {
        Self::try_prepare_selection(
            node,
            action,
            || Ok::<_, std::convert::Infallible>(available()),
            || Ok(items()),
            || Ok(count()),
            || Ok(current()),
        )
        .unwrap_or_else(|error| match error {})
    }
}

fn clamped_index(count: usize, index: i64) -> Option<usize> {
    (count > 0).then(|| index.clamp(0, count as i64 - 1) as usize)
}

impl ComboItems {
    #[doc(hidden)]
    pub fn populate_tumbler(&self, node: &mut Node) {
        let index = self
            .wheel_index(
                node.int("current-index").unwrap_or(0),
                node.properties.get("current-value"),
            )
            .map_or(-1, |index| index as i64);
        node.properties.insert("current-index", Value::Int(index));
        node.set_tumbler_items(TumblerItems::records(self.clone()));
    }

    pub(crate) fn wheel_count(&self) -> usize {
        self.count().min(MAX_ROWS)
    }

    pub(crate) fn wheel_index(&self, index: i64, value: Option<&Value>) -> Option<usize> {
        let count = self.wheel_count();
        if value.is_some() {
            self.selected_index(0, value)
                .filter(|&index| index < count)
                .or_else(|| clamped_index(count, 0))
        } else {
            clamped_index(count, index)
        }
    }
}
