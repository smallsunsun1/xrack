use crate::{ListModel, Node, Property, RowKey, RowLifetime};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
};

struct Input<Item> {
    item: Property<Item>,
    index: Property<i64>,
}

/// Model inputs shared by a delegate's generated bindings.
pub struct RowContext<Item>(Rc<Input<Item>>);

impl<Item> Clone for RowContext<Item> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Item: Clone> RowContext<Item> {
    pub fn item(&self) -> Item {
        self.0
            .item
            .get(|| unreachable!("delegate item is initialized"))
    }

    pub fn index(&self) -> i64 {
        self.0
            .index
            .get(|| unreachable!("delegate index is initialized"))
    }
}

struct Delegate<Item, State, Ui> {
    input: RowContext<Item>,
    synchronize: Property<()>,
    lifetime: RowLifetime,
    state: State,
    ui: Ui,
}

type DelegateRows<Item, State, Ui> = BTreeMap<RowKey, Rc<Delegate<Item, State, Ui>>>;

pub struct ComponentRepeater<Model: ListModel, State, Ui> {
    model: Property<Rc<Model>>,
    stamp: Cell<Option<(u64, u64)>>,
    rows: RefCell<DelegateRows<Model::Item, State, Ui>>,
}

impl<Model: ListModel, State, Ui> Default for ComponentRepeater<Model, State, Ui> {
    fn default() -> Self {
        Self {
            model: Property::default(),
            stamp: Cell::default(),
            rows: RefCell::default(),
        }
    }
}

impl<Model: ListModel, State: Default, Ui> ComponentRepeater<Model, State, Ui> {
    pub fn with_model<R>(
        &self,
        model: impl FnOnce() -> Model,
        read: impl FnOnce(&Model) -> R,
    ) -> R {
        read(&self.model.get(|| Rc::new(model())))
    }

    pub fn row_count(&self, model: impl FnOnce() -> Model) -> usize {
        self.with_model(model, ListModel::row_count)
    }

    pub fn render_range(
        &self,
        model: impl FnOnce() -> Model,
        range: Range<usize>,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        let model = self.model.get(|| Rc::new(model()));
        let count = model.row_count();
        let stamp = model.structure_stamp();
        if self.stamp.replace(Some(stamp)) != Some(stamp) {
            self.rows
                .borrow_mut()
                .retain(|key, row| row.lifetime.is_live(*key, stamp.0, count));
        }
        (range.start..range.end.min(count))
            .map(|index| {
                let key = model.row_key(index);
                let row = self
                    .rows
                    .borrow_mut()
                    .entry(key)
                    .or_insert_with(|| {
                        let input = RowContext(Rc::new(Input {
                            item: Property::default(),
                            index: Property::default(),
                        }));
                        Rc::new(Delegate {
                            synchronize: Property::default(),
                            lifetime: model.row_lifetime(key),
                            state: State::default(),
                            ui: create(input.clone()),
                            input,
                        })
                    })
                    .clone();
                row.input.0.index.set_if_changed(index as i64);
                row.synchronize
                    .get(|| row.input.0.item.set_if_changed(model.item(index)));
                let mut node = render(&row.ui, index, &|| row.input.item(), &row.state);
                node.row_key = Some(key);
                node
            })
            .collect()
    }

    /// Resolve a hit against the current model, so an old viewport cannot edit
    /// the delegate that has since moved into the same slot.
    pub fn with_row<T>(
        &self,
        model: impl FnOnce() -> Model,
        index: usize,
        key: RowKey,
        action: impl FnOnce(&Ui) -> T,
    ) -> Option<T> {
        let model = self.model.get(|| Rc::new(model()));
        if index >= model.row_count() || model.row_key(index) != key {
            return None;
        }
        let row = self.rows.borrow().get(&key).cloned()?;
        Some(action(&row.ui))
    }
}
