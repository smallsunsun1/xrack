use crate::{repeat::Model, *};
use std::{cell::Cell, sync::Arc};
use xrack_runtime::{
    ControlAction, RecordValue, RowSelection, TreeAction, TreeCommand, TreeSelectionAction,
    TreeSnapshot,
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
        let error = Cell::new(None);
        let model = || {
            // Read inside the snapshot's reactive computation so replacing
            // the entire source also replaces its topology and row identities.
            let records = records().unwrap_or_else(|failure| {
                error.set(Some(failure));
                source.clone()
            });
            Rc::new(records.model().clone())
        };
        let key = definition.key.clone();
        let parent = definition.parent.clone();
        let roles = move |record: &RecordValue| (integer(record, &key), integer(record, &parent));
        let root = || match self.binding_value(view, "root-key") {
            Ok(Some(Value::Int(key))) => key,
            Ok(_) => -1,
            Err(failure) => {
                error.set(Some(failure));
                -1
            }
        };
        let state = self.repeat_state(view);
        let tree = state.tree_state();
        let snapshot = if view.visible_field.is_some() || definition.filter_field.is_some() {
            tree.snapshot_projected_by(
                model,
                roles,
                root,
                view.visible_field.clone().map(|path| {
                    move |record: &RecordValue| record.field_path(&path) == Some(&Value::Bool(true))
                }),
                definition.filter_field.clone().map(filter_field),
                || match self.binding_value(view, "filter-text") {
                    Ok(Some(Value::String(query))) => query,
                    Ok(_) => String::new(),
                    Err(failure) => {
                        error.set(Some(failure));
                        String::new()
                    }
                },
            )
        } else {
            tree.snapshot_with_root(model, roles, root)
        };
        error.into_inner().map_or(Ok((snapshot, source)), Err)
    }

    pub(crate) fn tree_query(&self, target: &str, query: Query, key: i64) -> Result<Value> {
        let (owner, view) = self.element_owner(target)?;
        let (tree, _) = owner.tree_projection(view)?;
        Ok(match query {
            Query::IsExpanded => Value::Bool(tree.is_expanded(key)),
            Query::Depth => Value::Int(tree.depth(key).map_or(-1, |depth| depth as i64)),
            Query::RowForKey => {
                Value::Int(tree.visible_index(key).map_or(-1, |index| index as i64))
            }
        })
    }

    pub(crate) fn tree_command(&self, target: &str, command: TreeCommand) -> Result<()> {
        let (owner, view) = self.element_owner(target)?;
        let (live, _) = owner.tree_projection(view)?;
        let state = owner.repeat_state(view);
        let Some(change) = state.tree_state().command(&live, command) else {
            return Ok(());
        };
        let current = owner.current_tree_key(view)?;
        let (next, _) = owner.tree_projection(view)?;
        if let Some(key) = next
            .nearest_visible_ancestor(current)
            .filter(|key| *key != current)
        {
            owner.update_tree_key(view, key)?;
        }
        let event = if change.expanded {
            "expanded"
        } else {
            "collapsed"
        };
        owner.handler(view, event, &[Value::Int(change.key)])?;
        Ok(())
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

    fn select_tree(
        &self,
        view: &Element,
        live: &TreeSnapshot,
        multiple: bool,
        action: TreeSelectionAction,
    ) -> Result<bool> {
        let current = self.current_tree_key(view)?;
        if multiple {
            let state = self.repeat_state(view);
            let Some(change) = state.tree_state().select(live, current, action) else {
                return Ok(false);
            };
            if let Some(key) = change.current {
                self.update_tree_key(view, key)?;
            }
            if let Some(count) = change.selected_count {
                self.handler(view, "selection-changed", &[Value::Int(count as i64)])?;
            }
            return Ok(change.current.is_some() || change.selected_count.is_some());
        }
        let TreeSelectionAction::Row { key, row, .. } = action else {
            return Ok(false);
        };
        if current == key
            || !live
                .visible_index(key)
                .and_then(|index| live.rows().get(index))
                .is_some_and(|item| item.row == row)
        {
            return Ok(false);
        }
        self.update_tree_key(view, key)?;
        Ok(true)
    }

    pub(crate) fn select_tree_source(&self, view: &Element, source: usize) -> Result<()> {
        let (tree, _) = self.tree_projection(view)?;
        let Some(item) = tree
            .visible_index_for_source(source)
            .and_then(|index| tree.rows().get(index))
        else {
            return Ok(());
        };
        let multiple = self.table_flag(view, "multi-select", false)?;
        self.select_tree(
            view,
            &tree,
            multiple,
            TreeSelectionAction::Row {
                key: item.key,
                row: item.row,
                operation: RowSelection::Replace,
            },
        )?;
        Ok(())
    }

    pub(crate) fn tree_control(
        &self,
        view: &Element,
        node: &Node,
        action: ControlAction,
    ) -> Result<bool> {
        let Some(snapshot) = &node.tree else {
            return Ok(false);
        };
        let (live, _) = self.tree_projection(view)?;
        if let ControlAction::Tree(TreeAction::SetExpanded { key, row, expanded }) = action {
            let current = self.current_tree_key(view)?;
            let state = self.repeat_state(view);
            if !state.tree_state().set_expanded(&live, key, row, expanded) {
                return Ok(false);
            }
            if !expanded && live.is_descendant(current, key) {
                self.update_tree_key(view, key)?;
            }
            let event = if expanded { "expanded" } else { "collapsed" };
            self.handler(view, event, &[Value::Int(key)])?;
            return Ok(true);
        }
        let multiple = node.tree_selection.is_some();
        let action = match action {
            ControlAction::Select(index) => {
                let Some(item) = snapshot.rows().get(index) else {
                    return Ok(false);
                };
                TreeSelectionAction::Row {
                    key: item.key,
                    row: item.row,
                    operation: RowSelection::Replace,
                }
            }
            ControlAction::Tree(TreeAction::Selection(action)) if multiple => action,
            _ => return Ok(false),
        };
        if multiple && !snapshot.same_model(&live) {
            return Ok(false);
        }
        self.select_tree(view, &live, multiple, action)
    }
}
