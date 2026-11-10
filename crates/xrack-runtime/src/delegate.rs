use crate::{ListModel, Node, Property, RowKey, RowLifetime, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
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
    fn current_index(&self, model: &VecModel<Item>) -> Option<usize> {
        let hint = self.index() as usize;
        if model.row_key(hint) == Some(self.0.key) {
            Some(hint)
        } else {
            model.row_index(self.0.key)
        }
    }

    /// Read a row by identity even if it has moved since the last layout.
    #[doc(hidden)]
    pub fn read_from<R>(
        &self,
        model: &VecModel<Item>,
        project: impl FnOnce(&Item) -> R,
    ) -> Option<R> {
        model.project_row(self.current_index(model)?, project)
    }

    #[doc(hidden)]
    pub fn update_in(&self, model: &VecModel<Item>, update: impl FnOnce(&Item) -> Item) {
        let Some(index) = self.current_index(model) else {
            return;
        };
        if model.update_row_if_key(index, self.0.key, |row| Some(update(row))) {
            self.0.item.set_if_changed(model.item(index));
        }
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
            create,
            render,
        )
    }

    fn render_rows<I: Iterator<Item = usize>>(
        &self,
        model: impl FnOnce() -> Model,
        indices: impl FnOnce(usize) -> I,
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
        indices(count)
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
        row.sync(index, || model.item(index));
        Some(action(&row.ui))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
