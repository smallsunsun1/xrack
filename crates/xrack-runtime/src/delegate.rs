use crate::{
    ListModel, Node, Property, RowKey, RowLifetime, TableEditHistory, TableRowPatch, VecModel,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
};

struct Input<Item> {
    key: RowKey,
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
    #[doc(hidden)]
    pub fn with_sample(key: RowKey, index: usize, item: Item) -> Self {
        let input = Self(Rc::new(Input {
            key,
            item: Property::default(),
            index: Property::default(),
        }));
        input.0.index.set(index as i64);
        input.0.item.set(item);
        input
    }

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

impl<Item: Clone + PartialEq> RowContext<Item> {
    fn current_index<T>(&self, model: &VecModel<T>) -> Option<usize> {
        let hint = self.index() as usize;
        if model.row_key(hint) == Some(self.0.key) {
            Some(hint)
        } else {
            model.row_index(self.0.key)
        }
    }

    /// Read a row by identity even if it has moved since the last layout.
    #[doc(hidden)]
    pub fn read_from<T, R>(&self, model: &VecModel<T>, project: impl FnOnce(&T) -> R) -> Option<R> {
        model.project_row(self.current_index(model)?, project)
    }

    #[doc(hidden)]
    pub fn update_in(&self, model: &VecModel<Item>, update: impl FnOnce(&Item) -> Item) {
        self.update_projected_in(model, |row| Some(update(row)), Clone::clone);
    }

    /// Update a typed row and synchronize the delegate's projected input.
    #[doc(hidden)]
    pub fn update_projected_in<T: PartialEq>(
        &self,
        model: &VecModel<T>,
        update: impl FnOnce(&T) -> Option<T>,
        project: impl FnOnce(&T) -> Item,
    ) -> bool {
        self.write_projected_in(
            model,
            |index, key| model.update_row_if_key(index, key, update),
            project,
        )
    }

    #[doc(hidden)]
    pub fn update_in_with_history(
        &self,
        model: &VecModel<Item>,
        history: &TableEditHistory<Item>,
        enabled: bool,
        update: impl FnOnce(&Item) -> Item,
        capture: impl Fn(&Item, &Item, &mut TableRowPatch<Item>),
    ) {
        self.update_projected_in_with_history(
            model,
            history,
            enabled,
            |row| Some(update(row)),
            capture,
            Clone::clone,
        );
    }

    #[doc(hidden)]
    pub fn update_projected_in_with_history<T: Clone + PartialEq>(
        &self,
        model: &VecModel<T>,
        history: &TableEditHistory<T>,
        enabled: bool,
        update: impl FnOnce(&T) -> Option<T>,
        capture: impl Fn(&T, &T, &mut TableRowPatch<T>),
        project: impl FnOnce(&T) -> Item,
    ) -> bool {
        self.write_projected_in(
            model,
            |index, key| {
                if enabled {
                    history.commit_cell(model, index, key, update, capture)
                } else {
                    let changed = model.update_row_if_key(index, key, update);
                    if changed {
                        history.clear();
                    }
                    changed
                }
            },
            project,
        )
    }

    fn write_projected_in<T>(
        &self,
        model: &VecModel<T>,
        write: impl FnOnce(usize, RowKey) -> bool,
        project: impl FnOnce(&T) -> Item,
    ) -> bool {
        let Some(index) = self.current_index(model) else {
            return false;
        };
        if !write(index, self.0.key) {
            return false;
        }
        if let Some(item) = model.project_row(index, project) {
            self.0.item.set_if_changed(item);
        }
        true
    }
}

struct Delegate<Item, State, Ui> {
    input: RowContext<Item>,
    synchronize: Property<()>,
    lifetime: RowLifetime,
    state: State,
    ui: Ui,
}

impl<Item: Clone + PartialEq, State, Ui> Delegate<Item, State, Ui> {
    fn sync(&self, index: usize, item: impl FnOnce() -> Item) {
        self.input.0.index.set_if_changed(index as i64);
        self.synchronize
            .get(|| self.input.0.item.set_if_changed(item()));
    }
}

type DelegateRows<Item, State, Ui> = BTreeMap<RowKey, Rc<Delegate<Item, State, Ui>>>;

pub struct ComponentRepeater<Model: ListModel, State, Ui> {
    model: Property<Rc<Model>>,
    stamp: Cell<Option<(u64, u64)>>,
    rows: RefCell<DelegateRows<Model::Item, State, Ui>>,
    scenes: RefCell<BTreeMap<RowKey, Rc<Property<Node>>>>,
}

impl<Model: ListModel, State, Ui> Default for ComponentRepeater<Model, State, Ui> {
    fn default() -> Self {
        Self {
            model: Property::default(),
            stamp: Cell::default(),
            rows: RefCell::default(),
            scenes: RefCell::default(),
        }
    }
}

impl<Model: ListModel, State: Default, Ui> ComponentRepeater<Model, State, Ui> {
    pub fn resolve_model(&self, model: impl FnOnce() -> Model) -> Rc<Model> {
        self.model.get(|| Rc::new(model()))
    }

    pub fn with_model<R>(
        &self,
        model: impl FnOnce() -> Model,
        read: impl FnOnce(&Model) -> R,
    ) -> R {
        read(&self.resolve_model(model))
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
        self.render_rows(
            model,
            |count| range.start..range.end.min(count),
            None,
            create,
            render,
        )
    }

    pub fn render_indices(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl IntoIterator<Item = usize>,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        self.render_rows(
            model,
            |count| indices.into_iter().filter(move |&index| index < count),
            None,
            create,
            render,
        )
    }

    /// Reuse the rendered scenes for rows that remain in the requested set.
    /// Delegate state itself still follows the model's row lifetime.
    #[doc(hidden)]
    pub fn render_cached_rows(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl IntoIterator<Item = usize>,
        reads_index: bool,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        self.render_rows(
            model,
            |count| indices.into_iter().filter(move |&index| index < count),
            Some(reads_index),
            create,
            render,
        )
    }

    fn render_rows<I: Iterator<Item = usize>>(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl FnOnce(usize) -> I,
        cache_reads_index: Option<bool>,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Node,
    ) -> Vec<Node> {
        self.try_render_rows(
            model,
            indices,
            cache_reads_index,
            create,
            |ui, index, item, state| {
                Ok::<_, std::convert::Infallible>(render(ui, index, item, state))
            },
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Render a row projection, caching only successful scenes when requested.
    #[doc(hidden)]
    pub fn try_render_indices<E>(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl IntoIterator<Item = usize>,
        cache_reads_index: Option<bool>,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Result<Node, E>,
    ) -> Result<Vec<Node>, E> {
        self.try_render_rows(
            model,
            |count| indices.into_iter().filter(move |&index| index < count),
            cache_reads_index,
            create,
            render,
        )
    }

    fn try_render_rows<I: Iterator<Item = usize>, E>(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl FnOnce(usize) -> I,
        cache_reads_index: Option<bool>,
        create: impl Fn(RowContext<Model::Item>) -> Ui,
        render: impl Fn(&Ui, usize, &dyn Fn() -> Model::Item, &State) -> Result<Node, E>,
    ) -> Result<Vec<Node>, E> {
        let model = self.model.get(|| Rc::new(model()));
        let count = model.row_count();
        let stamp = model.structure_stamp();
        if self.stamp.replace(Some(stamp)) != Some(stamp) {
            let mut rows = self.rows.borrow_mut();
            rows.retain(|key, row| row.lifetime.is_live(*key, stamp.0, count));
            self.scenes
                .borrow_mut()
                .retain(|key, _| rows.contains_key(key));
        }
        let mut visible = BTreeSet::new();
        let nodes = indices(count)
            .map(|index| {
                let key = model.row_key(index);
                let row = self
                    .rows
                    .borrow_mut()
                    .entry(key)
                    .or_insert_with(|| {
                        let input = RowContext(Rc::new(Input {
                            key,
                            item: Property::default(),
                            index: Property::default(),
                        }));
                        input.0.index.set_if_changed(index as i64);
                        Rc::new(Delegate {
                            synchronize: Property::default(),
                            lifetime: model.row_lifetime(key),
                            state: State::default(),
                            ui: create(input.clone()),
                            input,
                        })
                    })
                    .clone();
                row.sync(index, || model.item(index));
                let draw = || {
                    let mut node = render(&row.ui, index, &|| row.input.item(), &row.state)?;
                    node.set_row_key(Some(key));
                    Ok(node)
                };
                if let Some(reads_index) = cache_reads_index {
                    visible.insert(key);
                    let scene = self.scenes.borrow_mut().entry(key).or_default().clone();
                    scene.try_get(|| {
                        if reads_index {
                            row.input.index();
                        }
                        draw().map(Node::into_shared)
                    })
                } else {
                    draw()
                }
            })
            .collect();
        self.scenes
            .borrow_mut()
            .retain(|key, _| visible.contains(key));
        nodes
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
        row.sync(index, || model.item(index));
        Some(action(&row.ui))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_row_scenes_can_be_retried_without_recreating_delegate_state() {
        let model = VecModel::from(vec!["item".to_owned()]);
        let repeater = ComponentRepeater::<VecModel<String>, (), ()>::default();
        let creates = Cell::new(0);
        let draws = Cell::new(0);
        let render = |fail| {
            repeater.try_render_indices(
                || model.clone(),
                [0],
                Some(false),
                |_| creates.set(creates.get() + 1),
                |_, _, item, _| {
                    draws.set(draws.get() + 1);
                    if fail {
                        return Err("render error");
                    }
                    let mut node = Node::new("Text", None);
                    node.properties.insert("text", item().into());
                    Ok(node)
                },
            )
        };

        assert_eq!(render(true), Err("render error"));
        let first = render(false).unwrap();
        let reused = render(false).unwrap();
        assert_eq!((creates.get(), draws.get()), (1, 2));
        assert_eq!(first[0].text("text"), Some("item"));
        assert!(first[0].shares_storage_with(&reused[0]));
    }

    #[test]
    fn cached_rows_reuse_only_visible_unchanged_scenes() {
        let model = VecModel::from(vec!["b".to_owned(), "a".to_owned(), "c".to_owned()]);
        let repeater = ComponentRepeater::<VecModel<String>, (), ()>::default();
        let source = Property::default();
        source.set(model.clone());
        let draws = Cell::new(0);
        let render = |range| {
            repeater.render_cached_rows(
                || source.get(|| unreachable!()),
                range,
                true,
                |_| (),
                |_, index, item, _| {
                    draws.set(draws.get() + 1);
                    let mut node = Node::new("Text", None);
                    node.properties
                        .insert("text", format!("{index}:{}", item()).into());
                    node
                },
            )
        };

        let original = render(0..2);
        let overlapping = render(1..3);
        assert_eq!(draws.get(), 3);
        assert!(original[1].shares_storage_with(&overlapping[0]));

        assert!(model.set_row(1, "aa".into()));
        let changed = render(1..3);
        assert_eq!(draws.get(), 4);
        assert!(changed[1].shares_storage_with(&overlapping[1]));
        assert_eq!(changed[0].text("text"), Some("1:aa"));

        assert!(model.sort_by(String::cmp));
        let reordered = render(0..2);
        assert_eq!(reordered[0].row_key, changed[0].row_key);
        assert_eq!(reordered[0].text("text"), Some("0:aa"));
        assert_eq!(reordered[1].text("text"), Some("1:b"));
        assert_eq!(draws.get(), 6);

        source.set(VecModel::from(vec!["replacement".to_owned()]));
        let replaced = render(0..1);
        assert_ne!(replaced[0].row_key, reordered[0].row_key);
        assert_eq!(replaced[0].text("text"), Some("0:replacement"));
        assert_eq!(draws.get(), 7);
    }

    #[test]
    fn captured_context_follows_its_row_and_rejects_a_replacement_model() {
        let model = VecModel::from(vec![String::from("b"), String::from("a")]);
        let repeater = ComponentRepeater::<VecModel<String>, (), RowContext<String>>::default();
        let mut contexts = RefCell::new(Vec::new());
        repeater.render_range(
            || model.clone(),
            0..2,
            |context| context,
            |context, _, _, _| {
                contexts.borrow_mut().push(context.clone());
                Node::new("LineEdit", None)
            },
        );
        let context = contexts.get_mut().remove(0);
        assert!(model.sort_by(|left, right| left.cmp(right)));
        assert_eq!(context.index(), 0);
        assert_eq!(
            context.read_from(&model, String::clone).as_deref(),
            Some("b")
        );
        context.update_in(&model, |_| String::from("changed"));
        assert_eq!(model.row(0).as_deref(), Some("a"));
        assert_eq!(model.row(1).as_deref(), Some("changed"));
        assert_eq!(context.item(), "changed");

        let replacement = VecModel::from(model.snapshot());
        assert_eq!(context.read_from(&replacement, String::clone), None);
        context.update_in(&replacement, |_| String::from("stale"));
        assert_eq!(replacement.row(1).as_deref(), Some("changed"));
    }
}
