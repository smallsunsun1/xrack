use crate::{ChangeSignal, ListModel, Node, Property, RowContext, RowKey, TableColumnWindow};
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
    context: RowContext<Model::Item>,
    state: State,
    scene: Property<Node>,
}

impl<Model: ListModel, State: Default> Row<Model, State> {
    fn new(index: usize, key: RowKey) -> Self {
        Self {
            synchronize: Property::default(),
            model: Property::default(),
            context: RowContext::new(key, index),
            state: State::default(),
            scene: Property::default(),
        }
    }
}

impl<Model: ListModel, State> Row<Model, State> {
    fn sync(&self, model: &Rc<Model>, index: usize) {
        self.context.set_key(model.row_key(index));
        self.model.set_if_changed_by(model.clone(), Rc::ptr_eq);
        self.synchronize.get(|| {
            let model = self
                .model
                .get(|| unreachable!("repeat model is initialized"));
            self.context.set_item(model.item(index));
        });
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
        let index = model.event_row_index(index, key, false)?;
        let row = self
            .rows
            .borrow()
            .get(&index)
            .cloned()
            .unwrap_or_else(|| Rc::new(Row::new(index, key)));
        crate::property::without_read_watches(|| row.sync(&model, index));
        Some(read(&|| row.context.item(), &|| index as i64, &row.state))
    }

    /// Access a materialized row after confirming the displayed key still matches.
    #[doc(hidden)]
    pub fn with_resolved_row<R>(
        &self,
        model: &Rc<Model>,
        index: usize,
        key: RowKey,
        read: impl FnOnce(&State) -> R,
    ) -> Option<R> {
        let index = model.event_row_index(index, key, false)?;
        let row = self.rows.borrow().get(&index).cloned()?;
        crate::property::without_read_watches(|| row.sync(model, index));
        Some(read(&row.state))
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
        self.render_resolved_indices(&resolved, range, render_row)
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
        self.try_render_resolved_indices(resolved, indices, |index, context, state| {
            Ok::<_, std::convert::Infallible>(render_row(index, &|| context.item(), state))
        })
        .unwrap_or_else(|error| match error {})
    }

    /// Cache only successful renders, retaining bindings at positions that remain visible.
    #[doc(hidden)]
    pub fn try_render_resolved_indices<E>(
        &self,
        resolved: &Rc<Model>,
        indices: impl IntoIterator<Item = usize>,
        render_row: impl Fn(usize, &RowContext<Model::Item>, &State) -> Result<Node, E>,
    ) -> Result<Vec<Node>, E> {
        let count = resolved.row_count();
        let indices: Vec<_> = indices.into_iter().filter(|&index| index < count).collect();
        let retained: BTreeSet<_> = indices.iter().copied().collect();
        self.rows
            .borrow_mut()
            .retain(|index, _| retained.contains(index));
        indices
            .into_iter()
            .map(|index| {
                let key = resolved.row_key(index);
                let row = self
                    .rows
                    .borrow_mut()
                    .entry(index)
                    .or_insert_with(|| Rc::new(Row::new(index, key)))
                    .clone();
                row.sync(resolved, index);
                let mut node = row
                    .scene
                    .try_get(|| render_row(index, &row.context, &row.state))?;
                node.set_row_key(Some(key));
                Ok(node)
            })
            .collect()
    }
}
