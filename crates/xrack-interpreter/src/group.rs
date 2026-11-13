use crate::{Element, Error, Instance, Member, Result, Visual};
use std::{collections::BTreeMap, rc::Rc};
use xrack_runtime::{ButtonGroupSelection, CheckState, ListModel, Node, Value};

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
        self.visual_flag(group, "enabled", true)
    }

    pub(crate) fn check_group_member_enabled(&self, member: &Element) -> Result<bool> {
        let Some(membership) = member
            .button_group
            .as_ref()
            .filter(|group| group.check_property.is_some())
        else {
            return Ok(true);
        };
        let (owner, group) = self.element_owner(&membership.key)?;
        owner.group_enabled(group)
    }

    fn group_selection(&self, group: &Element) -> Result<ButtonGroupSelection> {
        match self.group_control(group, group.selection_property())? {
            Value::Int(index) => Ok(ButtonGroupSelection::Index(index)),
            Value::String(value) => Ok(ButtonGroupSelection::Value(value)),
            _ => Err(Error::Evaluation("invalid button group selection".into())),
        }
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
                Some(self.group_selection(group)?)
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
        property.try_get(|| {
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

    pub(crate) fn activate_radio_option(
        &self,
        element: &Element,
        node: &Node,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(membership) = &element.button_group else {
            return Ok(false);
        };
        let (owner, group) = self.element_owner(&membership.key)?;
        let Some((index, label)) = xrack_runtime::try_prepare_radio_selection(
            node,
            &membership.key,
            input,
            || owner.group_enabled(group),
            || self.live_radio_value(element),
            || owner.group_selection(group),
        )?
        else {
            return Ok(false);
        };
        let property = group.selection_property();
        let next = if property == "current-value" {
            Value::String(label.clone())
        } else {
            Value::Int(index as i64)
        };
        if !owner.set_control_value(group, property, next)? {
            return Ok(false);
        }
        owner.handler(group, "selected", &[Value::String(label)])?;
        Ok(true)
    }

    pub(crate) fn group_option_model(
        &self,
        element: &Element,
    ) -> Result<Option<crate::repeat::Model>> {
        let Some(template) = element.children.first() else {
            return Ok(None);
        };
        let Some(repeat) = template
            .repeat
            .as_ref()
            .filter(|repeat| !repeat.empty && repeat.static_count.is_none())
        else {
            return Ok(None);
        };
        self.repeat_state(template)
            .row_model(self, repeat)
            .map(Some)
    }

    pub(crate) fn select_group(
        &self,
        element: &Element,
        node: &Node,
        index: usize,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let property = element.selection_property();
        let model = std::cell::OnceCell::new();
        let Some(label) = xrack_runtime::try_prepare_group_selection(
            node,
            index,
            input,
            || {
                let source = self.group_option_model(element)?;
                let current = source
                    .as_ref()
                    .is_none_or(|source| node.group_options_are_current(source));
                let _ = model.set(source);
                Ok::<_, Error>(current)
            },
            || self.group_selection(element),
            |index, label| {
                if let Some(source) = model.get().and_then(Option::as_ref) {
                    let Some(key) = node.child_at(index).and_then(|item| item.row_key) else {
                        return Ok(None);
                    };
                    if index >= source.row_count() || source.row_key(index) != key {
                        return Ok(None);
                    }
                    let template = &element.children[0];
                    let repeat = template.repeat.as_ref().expect("repeated group option");
                    let Some(row) = self
                        .repeat_state(template)
                        .row(self, repeat, index, key, false)?
                    else {
                        return Ok(None);
                    };
                    return row.live_group_option(template, label);
                }
                if element
                    .children
                    .first()
                    .is_some_and(|child| child.repeat.is_some())
                {
                    return Ok(xrack_runtime::group_item(node, index).map(|item| {
                        if label {
                            xrack_runtime::radio_value(item).into()
                        } else {
                            String::new()
                        }
                    }));
                }
                let Some(option) = element.children.get(index) else {
                    return Ok(None);
                };
                self.live_group_option(option, label)
            },
        )?
        else {
            return Ok(false);
        };
        let label = Value::String(label);
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

    fn live_group_option(&self, element: &Element, label: bool) -> Result<Option<String>> {
        if !self.item_delegate_available(element)? {
            return Ok(None);
        }
        if label {
            return self.live_radio_value(element).map(Some);
        }
        Ok(Some(String::new()))
    }

    fn live_radio_value(&self, element: &Element) -> Result<String> {
        for name in ["value", "text"] {
            if let Some(Value::String(value)) = self.binding_value(element, name)? {
                return Ok(value);
            }
        }
        Ok(String::new())
    }

    pub(crate) fn select_page_indicator(
        &self,
        element: &Element,
        index: usize,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let selected = xrack_runtime::try_prepare_index_selection(
            index,
            || {
                Ok(self.element_allows_editing(element)?
                    && self.table_flag(element, "interactive", false)?)
            },
            |index| {
                Ok(
                    matches!(self.binding_value(element, "count")?, Some(Value::Int(count)) if index < count),
                )
            },
            || self.current_index(element),
        )?;
        let Some(index) = selected else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        let value = Value::Int(index);
        if !self.set_control_value(element, "current-index", value.clone())? {
            return Ok(false);
        }
        self.handler(element, "changed", &[value])?;
        Ok(true)
    }
}
