use crate::{ChangeSignal, ListModel, Node, Property, TableColumnWindow};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
};

struct Row<Item, State> {
    synchronize: Property<()>,
    item: Property<Item>,
    state: State,
    scene: Property<Node>,
}

impl<Item, State: Default> Default for Row<Item, State> {
    fn default() -> Self {
        Self {
            synchronize: Property::default(),
            item: Property::default(),
            state: State::default(),
            scene: Property::default(),
        }
    }
}

impl<Item: Clone + PartialEq, State> Row<Item, State> {
    fn render(
        &self,
        index: usize,
        item: impl FnOnce() -> Item,
        render_row: &impl Fn(usize, &dyn Fn() -> Item, &State) -> Node,
    ) -> Node {
        // Reconnect on model replacement, but preserve bindings when the row's
        // value at this position is equal to the value we previously rendered.
        self.synchronize.get(|| self.item.set_if_changed(item()));
        let item = || self.item.get(|| unreachable!("repeat item is initialized"));
        self.scene.get(|| render_row(index, &item, &self.state))
    }
}

/// Scene and binding caches scoped to each position in a generated repeat.
pub struct Repeater<Model: ListModel, State> {
    rows: RefCell<BTreeMap<usize, Row<Model::Item, State>>>,
    model: Property<Rc<Model>>,
    columns: RefCell<TableColumnWindow>,
    columns_changed: ChangeSignal,
}

impl<Model: ListModel, State> Default for Repeater<Model, State> {
    fn default() -> Self {
        Self {
            rows: RefCell::default(),
            model: Property::default(),
            columns: RefCell::default(),
            columns_changed: ChangeSignal::default(),
        }
    }
}

impl<Model: ListModel, State: Default> Repeater<Model, State> {
    #[doc(hidden)]
    pub fn column_window(&self, columns: &TableColumnWindow) -> &ChangeSignal {
        if &*self.columns.borrow() != columns {
            self.columns.replace(columns.clone());
            self.columns_changed.notify();
        }
        &self.columns_changed
    }

    pub fn resolve_model(&self, model: impl FnOnce() -> Model) -> Rc<Model> {
        self.model.get(|| Rc::new(model()))
    }
    pub fn with_model<R>(
        &self,
        model: impl FnOnce() -> Model,
        read: impl FnOnce(&Model) -> R,
    ) -> R {
        read(&self.model.get(|| Rc::new(model())))
    }

    pub fn row_count(&self, model: impl FnOnce() -> Model) -> usize {
        self.model.get(|| Rc::new(model())).row_count()
    }

    pub fn render(
        &self,
        model: impl Fn() -> Model,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        self.render_container("VerticalLayout", model, render_row)
    }

    #[doc(hidden)]
    pub fn render_positioned(
        &self,
        model: impl Fn() -> Model,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        self.render_container("Rectangle", model, render_row)
    }

    fn render_container(
        &self,
        kind: &'static str,
        model: impl Fn() -> Model,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        let children = self.render_range(model, 0..4096, render_row);
        if children.is_empty() {
            return Node::hidden();
        }
        let mut node = Node::new(kind, None);
        node.children = children;
        node
    }

    pub fn render_range(
        &self,
        model: impl Fn() -> Model,
        range: Range<usize>,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        let resolved = self.model.get(|| Rc::new(model()));
        let range = range.start..range.end.min(resolved.row_count());
        let mut rows = self.rows.borrow_mut();
        rows.retain(|index, _| range.contains(index));
        range
            .map(|index| {
                let mut node = rows.entry(index).or_default().render(
                    index,
                    || self.model.get(|| Rc::new(model())).item(index),
                    &render_row,
                );
                node.row_key = Some(resolved.row_key(index));
                node
            })
            .collect()
    }

    pub fn render_indices(
        &self,
        model: impl Fn() -> Model,
        indices: impl IntoIterator<Item = usize>,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        let resolved = self.model.get(|| Rc::new(model()));
        let count = resolved.row_count();
        let indices: Vec<_> = indices.into_iter().filter(|&index| index < count).collect();
        let retained: BTreeSet<_> = indices.iter().copied().collect();
        let mut rows = self.rows.borrow_mut();
        rows.retain(|index, _| retained.contains(index));
        indices
            .into_iter()
            .map(|index| {
                let mut node = rows.entry(index).or_default().render(
                    index,
                    || self.model.get(|| Rc::new(model())).item(index),
                    &render_row,
                );
                node.row_key = Some(resolved.row_key(index));
                node
            })
            .collect()
    }
}
