use crate::{repeat::Model, *};
use std::sync::Arc;
use xrack_runtime::{RecordValue, TreeCommand, TreeControlChange, TreeControlEvent, TreeSnapshot};

pub(crate) struct Definition {
    pub(crate) key: Rc<[String]>,
    pub(crate) parent: Rc<[String]>,
    pub(crate) filter_field: Option<Rc<[String]>>,
}

#[derive(Clone, Copy)]
pub(crate) enum Query {
    IsExpanded,
    Depth,
    RowForKey,
}

impl Query {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "is-expanded" => Self::IsExpanded,
            "depth" => Self::Depth,
            "row-for-key" => Self::RowForKey,
            _ => return None,
        })
    }
}

fn integer(row: &RecordValue, path: &[String]) -> i64 {
    match row.field_path(path) {
        Some(Value::Int(value)) => *value,
        _ => -1,
    }
}

fn filter_field(path: Rc<[String]>) -> impl Fn(&RecordValue) -> &str + Clone {
    move |row| match row.field_path(&path) {
        Some(Value::String(text)) => text,
        _ => "",
    }
}

impl Instance {
    pub(crate) fn tree_projection(
        &self,
        view: &Element,
    ) -> Result<(Arc<TreeSnapshot>, xrack_runtime::RecordList)> {
        let definition = view.tree.as_ref().expect("tree definition");
        let template = view
            .view_template()
            .ok_or_else(|| Error::Invalid("missing tree row template".into()))?;
        let repeat = template.repeat.as_ref().expect("tree row repeat");
        let rows = self.repeat_state(template);
        let records = || match rows.row_model(self, repeat)? {
            Model::Records(records) => Ok(records),
            _ => Err(Error::Invalid("tree requires a record model".into())),
        };
        let source = records()?;
        let model = Rc::new(source.model().clone());
        let key = definition.key.clone();
        let parent = definition.parent.clone();
        let roles = move |record: &RecordValue| (integer(record, &key), integer(record, &parent));
        let root = || match self.binding_value(view, "root-key")? {
            Some(Value::Int(key)) => Ok(key),
            _ => Ok(-1),
        };
        let state = self.repeat_state(view);
        let tree = state.tree_state();
        let snapshot = if view.visible_field.is_some() || definition.filter_field.is_some() {
            tree.try_snapshot_projected_by(
                model,
                roles,
                root,
                view.visible_field.clone().map(|path| {
                    move |record: &RecordValue| record.field_path(&path) == Some(&Value::Bool(true))
                }),
                definition.filter_field.clone().map(filter_field),
                || match self.binding_value(view, "filter-text")? {
                    Some(Value::String(query)) => Ok(query),
                    _ => Ok(String::new()),
                },
            )?
        } else {
            tree.try_snapshot_with_root(model, roles, root)?
        };
        Ok((snapshot, source))
    }

    pub(crate) fn tree_query(&self, target: &str, query: Query, key: i64) -> Result<Value> {
        let (owner, view) = self.element_owner(target)?;
        let (tree, _) = owner.tree_projection(view)?;
        Ok(match query {
            Query::IsExpanded => Value::Bool(tree.is_expanded(key)),
            Query::Depth => Value::Int(tree.query_depth(key)),
            Query::RowForKey => Value::Int(tree.query_row(key)),
        })
    }

    fn tree_owner(&self, id: &str) -> Result<(Self, &Element)> {
        let (owner, view) = self.element_owner(id)?;
        if view.tree.is_none() {
            return Err(Error::Invalid(format!("{id} is not a TreeView")));
        }
        Ok((owner, view))
    }

    /// Read the current visible tree and expansion state of a view.
    pub fn tree_snapshot(&self, id: &str) -> Result<Arc<TreeSnapshot>> {
        let (owner, view) = self.tree_owner(id)?;
        owner.tree_projection(view).map(|(tree, _)| tree)
    }

    /// Apply a host expansion command using the same path as a DSL command.
    pub fn command_tree(&mut self, id: &str, command: TreeCommand) -> Result<bool> {
        self.tree_command(id, command)
    }

    pub(crate) fn tree_command(&self, target: &str, command: TreeCommand) -> Result<bool> {
        let (owner, view) = self.tree_owner(target)?;
        let state = owner.repeat_state(view);
        let Some((change, current)) = state.tree_state().try_apply_command(
            command,
            || owner.tree_projection(view).map(|(tree, _)| tree),
            || owner.current_tree_key(view),
        )?
        else {
            return Ok(false);
        };
        owner.notify_tree_change(view, TreeControlChange::Expansion { change, current })
    }

    fn current_tree_key(&self, view: &Element) -> Result<i64> {
        Ok(match self.control_value(view, "current-key")? {
            Some(Value::Int(key)) => key,
            _ => -1,
        })
    }

    fn notify_tree_change(&self, view: &Element, change: TreeControlChange) -> Result<bool> {
        change.try_notify(|event| {
            let (name, argument) = match event {
                TreeControlEvent::CurrentKeyChanged(key) => {
                    self.set_control_value(view, "current-key", Value::Int(key))?;
                    ("changed", key)
                }
                TreeControlEvent::SelectionChanged(count) => ("selection-changed", count as i64),
                TreeControlEvent::Expanded(key) => ("expanded", key),
                TreeControlEvent::Collapsed(key) => ("collapsed", key),
            };
            self.handler(view, name, &[Value::Int(argument)])
                .map(|_| ())
        })
    }

    pub(crate) fn select_tree_source(&self, view: &Element, source: usize) -> Result<()> {
        let state = self.repeat_state(view);
        if let Some(change) = state.tree_state().try_select_source(
            source,
            || self.tree_projection(view).map(|(tree, _)| tree),
            || self.table_flag(view, "multi-select", false),
            || self.current_tree_key(view),
        )? {
            self.notify_tree_change(view, TreeControlChange::Selection(change))?;
        }
        Ok(())
    }

    pub(crate) fn tree_control(
        &self,
        view: &Element,
        node: &Node,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let state = self.repeat_state(view);
        let change = state.tree_state().try_apply_control(
            node,
            input,
            || self.element_allows_editing(view),
            || self.table_flag(view, "multi-select", false),
            || self.tree_projection(view).map(|(live, _)| live),
            || self.current_tree_key(view),
        )?;
        match change {
            Some(change) => self.notify_tree_change(view, change),
            None => Ok(false),
        }
    }
}
