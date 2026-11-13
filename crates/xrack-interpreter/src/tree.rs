use crate::{repeat::Model, *};
use std::sync::Arc;
use xrack_runtime::{
    RecordValue, TreeCommand, TreeControlChange, TreeSelectionChange, TreeSnapshot,
};

pub(crate) struct Definition {
    pub(crate) key: Rc<[String]>,
    pub(crate) parent: Rc<[String]>,
    pub(crate) filter_field: Option<Rc<[String]>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Expand,
    Collapse,
    Toggle,
    ExpandRecursively,
    CollapseRecursively,
    ExpandTo,
}

impl Command {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "expand" => Self::Expand,
            "collapse" => Self::Collapse,
            "toggle-expanded" => Self::Toggle,
            "expand-recursively" => Self::ExpandRecursively,
            "collapse-recursively" => Self::CollapseRecursively,
            "expand-to" => Self::ExpandTo,
            _ => return None,
        })
    }

    pub(crate) fn action(self, key: i64, depth: Option<i64>) -> Result<TreeCommand> {
        Ok(match self {
            Self::Expand => TreeCommand::Expand(key),
            Self::Collapse => TreeCommand::Collapse(key),
            Self::Toggle => TreeCommand::ToggleExpanded(key),
            Self::ExpandRecursively => TreeCommand::ExpandRecursively {
                key,
                depth: depth.ok_or_else(|| Error::Invalid("tree command missing depth".into()))?,
            },
            Self::CollapseRecursively => TreeCommand::CollapseRecursively(key),
            Self::ExpandTo => TreeCommand::ExpandTo(key),
        })
    }
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
        if let Some(key) = current {
            owner.update_tree_key(view, key)?;
        }
        let event = if change.expanded {
            "expanded"
        } else {
            "collapsed"
        };
        owner.handler(view, event, &[Value::Int(change.key)])?;
        Ok(true)
    }

    fn current_tree_key(&self, view: &Element) -> Result<i64> {
        Ok(match self.control_value(view, "current-key")? {
            Some(Value::Int(key)) => key,
            _ => -1,
        })
    }

    fn update_tree_key(&self, view: &Element, key: i64) -> Result<()> {
        self.set_control_value(view, "current-key", Value::Int(key))?;
        self.handler(view, "changed", &[Value::Int(key)])?;
        Ok(())
    }

    fn notify_tree_selection(&self, view: &Element, change: TreeSelectionChange) -> Result<bool> {
        if let Some(key) = change.current {
            self.update_tree_key(view, key)?;
        }
        if let Some(count) = change.selected_count {
            self.handler(view, "selection-changed", &[Value::Int(count as i64)])?;
        }
        Ok(change.current.is_some() || change.selected_count.is_some())
    }

    pub(crate) fn select_tree_source(&self, view: &Element, source: usize) -> Result<()> {
        let state = self.repeat_state(view);
        if let Some(change) = state.tree_state().try_select_source(
            source,
            || self.tree_projection(view).map(|(tree, _)| tree),
            || self.table_flag(view, "multi-select", false),
            || self.current_tree_key(view),
        )? {
            self.notify_tree_selection(view, change)?;
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
        match state.tree_state().try_apply_control(
            node,
            input,
            || self.element_allows_editing(view),
            || self.table_flag(view, "multi-select", false),
            || self.tree_projection(view).map(|(live, _)| live),
            || self.current_tree_key(view),
        )? {
            Some(TreeControlChange::Selection(change)) => self.notify_tree_selection(view, change),
            Some(TreeControlChange::Expansion { change, current }) => {
                if let Some(key) = current {
                    self.update_tree_key(view, key)?;
                }
                let event = if change.expanded {
                    "expanded"
                } else {
                    "collapsed"
                };
                self.handler(view, event, &[Value::Int(change.key)])?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}
