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
    pub(crate) fn tree_projection(&self, view: &Element) -> Result<(Arc<TreeSnapshot>, Rc<Model>)> {
        let definition = view.tree.as_ref().expect("tree definition");
        let template = view
            .view_template()
            .ok_or_else(|| Error::Invalid("missing tree row template".into()))?;
        let repeat = template.repeat.as_ref().expect("tree row repeat");
        let rows = self.repeat_state(template);
        let source = || {
            let model = rows.row_model(self, repeat)?;
            if !matches!(model.as_ref(), Model::Records(_)) {
                return Err(Error::Invalid("tree requires a record model".into()));
            }
            Ok(model)
        };
        let key = definition.key.clone();
        let parent = definition.parent.clone();
        let roles = move |record: &RecordValue| (integer(record, &key), integer(record, &parent));
        let root = || match self.binding_value(view, "root-key")? {
            Some(Value::Int(key)) => Ok(key),
            _ => Ok(-1),
        };
        let state = self.repeat_state(view);
        let tree = state.tree_state();
        tree.try_project_model(
            source,
            |model| match model.as_ref() {
                Model::Records(records) => records.model(),
                _ => unreachable!("verified tree model"),
            },
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
        )
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

    /// Read the current visible tree and expansion state of a view.
    pub fn tree_snapshot(&self, id: &str) -> Result<Arc<TreeSnapshot>> {
        let view = self.host_element(id, |view| view.tree.is_some())?;
        self.tree_projection(view).map(|(tree, _)| tree)
    }

    /// Apply a host expansion command using the same path as a DSL command.
    pub fn command_tree(&mut self, id: &str, command: TreeCommand) -> Result<bool> {
        let view = self.host_element(id, |view| view.tree.is_some())?;
        self.tree_command_for(view, command)
    }

    pub(crate) fn tree_command(&self, target: &str, command: TreeCommand) -> Result<bool> {
        let (owner, view) = self.element_owner(target)?;
        owner.tree_command_for(view, command)
    }

    fn tree_command_for(&self, view: &Element, command: TreeCommand) -> Result<bool> {
        let state = self.repeat_state(view);
        let Some((change, current)) = state.tree_state().try_apply_command(
            command,
            || self.tree_projection(view).map(|(tree, _)| tree),
            || self.current_tree_key(view),
        )?
        else {
            return Ok(false);
        };
        self.notify_tree_change(view, TreeControlChange::Expansion { change, current })
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
            || self.element_allows_routed_input(view, input),
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
