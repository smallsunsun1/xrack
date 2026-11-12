use crate::{Element, Error, Instance, Member, Result, Visual};
use std::{collections::BTreeMap, rc::Rc};
use xrack_runtime::{ButtonGroupSelection, CheckState, Node, Value};

pub(crate) struct Membership {
    pub(crate) key: String,
    pub(crate) check_property: Option<String>,
}

pub(crate) struct Members {
    paths: Rc<[Vec<usize>]>,
    keys: Vec<String>,
}

pub(crate) fn members(root: &Element) -> BTreeMap<String, Members> {
    type Paths = BTreeMap<String, Vec<(Vec<usize>, String)>>;
    fn collect(element: &Element, path: &mut Vec<usize>, groups: &mut Paths) {
        if let Some(group) = &element.button_group {
            groups
                .entry(group.key.clone())
                .or_default()
                .push((path.clone(), element.key.clone()));
        }
        for (index, child) in element
            .children
            .iter()
            .enumerate()
            .filter(|(_, child)| child.repeat.is_none())
        {
            path.push(index + element.synthetic_child_count());
            collect(child, path, groups);
            path.pop();
        }
    }

    let mut groups = BTreeMap::new();
    collect(root, &mut Vec::new(), &mut groups);
    groups
        .into_iter()
        .map(|(key, members)| {
            let (paths, keys): (Vec<_>, _) = members.into_iter().unzip();
            (
                key,
                Members {
                    paths: paths.into(),
                    keys,
                },
            )
        })
        .collect()
}

impl Instance {
    fn group_control(&self, group: &Element, name: &str) -> Result<Value> {
        self.control_value(group, name)?
            .map_or_else(|| self.default_control_value(group, name), Ok)
    }

    fn group_enabled(&self, group: &Element) -> Result<bool> {
        Ok(self.binding_value(group, "enabled")? != Some(Value::Bool(false)))
    }

    pub(crate) fn bind_button_groups(&self, scene: &mut Node) -> Result<()> {
        let definition = self.definition();
        for (key, members) in &definition.button_groups {
            let group = definition
                .root
                .find_key(key)
                .ok_or_else(|| Error::Invalid(format!("unknown button group {key}")))?;
            let name = match self.binding_value(group, "accessible-name")? {
                Some(Value::String(name)) => name,
                _ => String::new(),
            };
            let selection = if group.non_exclusive {
                None
            } else {
                Some(
                    match self.group_control(group, group.selection_property())? {
                        Value::Int(index) => ButtonGroupSelection::Index(index),
                        Value::String(value) => ButtonGroupSelection::Value(value),
                        _ => {
                            return Err(Error::Evaluation("invalid button group selection".into()));
                        }
                    },
                )
            };
            xrack_runtime::bind_dynamic_button_group(
                scene,
                key.clone(),
                name,
                members.paths.clone(),
                selection,
                self.group_enabled(group)?,
            );
        }
        Ok(())
    }

    pub(crate) fn is_check_group(&self, member: &Member) -> bool {
        member.name == "check-state"
            && self
                .definition()
                .root
                .find_key(&member.owner)
                .is_some_and(|element| {
                    element.non_exclusive
                        && matches!(element.visual, Visual::Builtin("ButtonGroup"))
                })
    }

    fn check_members(&self, key: &str) -> impl Iterator<Item = (&Element, &str)> {
        let definition = self.definition();
        definition
            .button_groups
            .get(key)
            .into_iter()
            .flat_map(|members| &members.keys)
            .filter_map(|member| {
                let element = definition.root.find_key(member)?;
                Some((
                    element,
                    element.button_group.as_ref()?.check_property.as_deref()?,
                ))
            })
    }

    pub(crate) fn check_group_state(&self, key: &str) -> Result<CheckState> {
        let (owner, _) = self.element_owner(key)?;
        let property = owner
            .0
            .check_groups
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone();
        property.get(|| {
            xrack_runtime::try_summarize_checks(owner.check_members(key).map(
                |(element, property)| match owner.group_control(element, property)? {
                    Value::CheckState(state) => Ok(state),
                    Value::Bool(checked) => Ok(checked.into()),
                    _ => Err(Error::Evaluation("invalid check group member state".into())),
                },
            ))
        })
    }

    pub(crate) fn set_check_group(&self, key: &str, value: Value) -> Result<()> {
        let Value::CheckState(state) = value else {
            return Err(Error::Evaluation("expected check state".into()));
        };
        if state != CheckState::PartiallyChecked {
            let (owner, _) = self.element_owner(key)?;
            for (element, property) in owner.check_members(key) {
                let value = if property == "checked" {
                    Value::Bool(state == CheckState::Checked)
                } else {
                    Value::CheckState(state)
                };
                owner.set_control_value(element, property, value)?;
            }
        }
        Ok(())
    }

    pub(crate) fn activate_radio_option(&self, element: &Element, node: &Node) -> Result<bool> {
        let Some(group) = &element.button_group else {
            return Ok(false);
        };
        let Some(member) = node
            .button_group
            .as_ref()
            .filter(|member| member.group.key == group.key)
        else {
            return Ok(false);
        };
        let (owner, group) = self.element_owner(&group.key)?;
        if !owner.group_enabled(group)? {
            return Ok(false);
        }
        let property = group.selection_property();
        let label = xrack_runtime::radio_value(node).to_owned();
        let next = if property == "current-value" {
            Value::String(label.clone())
        } else {
            Value::Int(member.index as i64)
        };
        if owner.group_control(group, property)? == next {
            return Ok(false);
        }
        owner.set_control_value(group, property, next)?;
        owner.handler(group, "selected", &[Value::String(label)])?;
        Ok(true)
    }

    pub(crate) fn select_group(
        &self,
        element: &Element,
        node: &Node,
        index: usize,
    ) -> Result<bool> {
        let Some(item) = xrack_runtime::group_item(node, index) else {
            return Ok(false);
        };
        if xrack_runtime::group_index(node) == Some(index) {
            return Ok(false);
        }
        let property = element.selection_property();
        let label = Value::String(xrack_runtime::radio_value(item).into());
        let index = Value::Int(index as i64);
        self.set_control_value(
            element,
            property,
            if property == "current-value" {
                label.clone()
            } else {
                index.clone()
            },
        )?;
        if node.kind == "RadioGroup" {
            self.handler(element, "selected", &[label])?;
        } else {
            self.handler(element, "changed", &[index])?;
        }
        Ok(true)
    }

    pub(crate) fn select_page_indicator(&self, element: &Element, index: usize) -> Result<bool> {
        let Ok(index) = i64::try_from(index) else {
            return Ok(false);
        };
        let count = self
            .binding_value(element, "count")?
            .unwrap_or(Value::Int(0));
        let value = Value::Int(index);
        if !matches!(count, Value::Int(count) if index < count)
            || self.group_control(element, "current-index")? == value
        {
            return Ok(false);
        }
        self.set_control_value(element, "current-index", value.clone())?;
        self.handler(element, "changed", &[value])?;
        Ok(true)
    }
}
