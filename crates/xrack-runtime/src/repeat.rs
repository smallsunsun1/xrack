use crate::{ChangeSignal, ListModel, Node, Property, RowKey, TableColumnWindow};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
};

/// Build a scene for a compiler-proven static integer repeat.
#[doc(hidden)]
pub fn static_repeat_scene(template: Node, count: i64, positioned: bool) -> Node {
    let template = template.into_shared();
    let children = (0..count.row_count())
        .map(|index| {
            let mut row = template.clone();
            row.set_row_key(Some(count.row_key(index)));
            row
        })
        .collect();
    repeat_scene(children, positioned).into_shared()
}

/// Wrap the materialized rows of an ordinary repeat in their scene container.
#[doc(hidden)]
pub fn repeat_scene(children: Vec<Node>, positioned: bool) -> Node {
    if children.is_empty() {
        return Node::hidden();
    }
    let kind = if positioned {
        "Rectangle"
    } else {
        "VerticalLayout"
    };
    let mut scene = Node::new(kind, None);
    scene.children = children;
    scene
}

struct Row<Model: ListModel, State> {
    synchronize: Property<()>,
    model: Property<Rc<Model>>,
    item: Property<Model::Item>,
    state: State,
    scene: Property<Node>,
}

impl<Model: ListModel, State: Default> Default for Row<Model, State> {
    fn default() -> Self {
        Self {
            synchronize: Property::default(),
            model: Property::default(),
            item: Property::default(),
            state: State::default(),
            scene: Property::default(),
        }
    }
}

impl<Model: ListModel, State> Row<Model, State> {
    fn sync(&self, model: &Rc<Model>, index: usize) {
        self.model.set_if_changed_by(model.clone(), Rc::ptr_eq);
        self.synchronize.get(|| {
            let model = self
                .model
                .get(|| unreachable!("repeat model is initialized"));
            self.item.set_if_changed(model.item(index));
        });
    }

    fn item(&self) -> Model::Item {
        self.item.get(|| unreachable!("repeat item is initialized"))
    }

    fn render(
        &self,
        index: usize,
        model: &Rc<Model>,
        render_row: &impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        // Reconnect on model replacement, but preserve bindings when the row's
        // value at this position is equal to the value we previously rendered.
        self.sync(model, index);
        let item = || self.item();
        self.scene.get(|| render_row(index, &item, &self.state))
    }
}

/// Scene and binding caches scoped to each position in a generated repeat.
pub struct Repeater<Model: ListModel, State> {
    rows: RefCell<BTreeMap<usize, Rc<Row<Model, State>>>>,
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

    #[doc(hidden)]
    pub fn with_row_state<R>(
        &self,
        model: impl FnOnce() -> Model,
        index: usize,
        key: RowKey,
        read: impl FnOnce(&dyn Fn() -> Model::Item, &dyn Fn() -> i64, &State) -> R,
    ) -> Option<R> {
        let model = self.resolve_model(model);
        if index >= model.row_count() || model.row_key(index) != key {
            return None;
        }
        let row = self.rows.borrow().get(&index).cloned().unwrap_or_default();
        row.sync(&model, index);
        Some(read(&|| row.item(), &|| index as i64, &row.state))
    }

    pub fn row_count(&self, model: impl FnOnce() -> Model) -> usize {
        self.model.get(|| Rc::new(model())).row_count()
    }

    pub fn render(
        &self,
        model: impl Fn() -> Model,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        repeat_scene(self.render_range(model, 0..4096, render_row), false)
    }

    #[doc(hidden)]
    pub fn render_positioned(
        &self,
        model: impl Fn() -> Model,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Node {
        repeat_scene(self.render_range(model, 0..4096, render_row), true)
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
                let mut node = rows
                    .entry(index)
                    .or_default()
                    .render(index, &resolved, &render_row);
                node.set_row_key(Some(resolved.row_key(index)));
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
        self.render_resolved_indices(&resolved, indices, render_row)
    }

    #[doc(hidden)]
    pub fn render_resolved_indices(
        &self,
        resolved: &Rc<Model>,
        indices: impl IntoIterator<Item = usize>,
        render_row: impl Fn(usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        let count = resolved.row_count();
        let indices: Vec<_> = indices.into_iter().filter(|&index| index < count).collect();
        let retained: BTreeSet<_> = indices.iter().copied().collect();
        let mut rows = self.rows.borrow_mut();
        rows.retain(|index, _| retained.contains(index));
        indices
            .into_iter()
            .map(|index| {
                let mut node = rows
                    .entry(index)
                    .or_default()
                    .render(index, resolved, &render_row);
                node.set_row_key(Some(resolved.row_key(index)));
                node
            })
            .collect()
    }
}
