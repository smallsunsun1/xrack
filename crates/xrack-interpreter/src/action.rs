use crate::{Element, Error, Instance, Member, Result};
use xrack_runtime::{ControlAction, DialogCommand, DialogRole, Node, Value};

pub(crate) struct Membership {
    pub(crate) group: String,
    pub(crate) value: String,
}

impl Instance {
    pub(crate) fn action_group_value(&self, key: &str) -> Result<Value> {
        self.read(&Member::new(key, "current-value"))
    }

    fn action_group<'a>(
        &'a self,
        element: &'a Element,
    ) -> Result<Option<(Self, &'a Element, &'a str)>> {
        element
            .action_group
            .as_ref()
            .map(|membership| {
                let (owner, group) = self.element_owner(&membership.group)?;
                Ok((owner, group, membership.value.as_str()))
            })
            .transpose()
    }

    fn action_flag(&self, element: &Element, property: &str, default: bool) -> Result<bool> {
        Ok(match self.binding_value(element, property)? {
            Some(Value::Bool(value)) => value,
            _ => default,
        })
    }

    fn action_enabled(
        &self,
        element: &Element,
        group: Option<&(Self, &Element, &str)>,
    ) -> Result<bool> {
        if let Some((owner, group, _)) = group
            && !owner.action_flag(group, "enabled", true)?
        {
            return Ok(false);
        }
        self.action_flag(element, "enabled", true)
    }

    pub(crate) fn present_action(&self, target: &str, node: &mut Node) -> Result<bool> {
        let (owner, element) = self.element_owner(target)?;
        let identity = owner
            .0
            .actions
            .get(target)
            .ok_or_else(|| Error::Evaluation(format!("unmounted action {target}")))?;
        let group = owner.action_group(element)?;
        let enabled = owner.action_enabled(element, group.as_ref())?;
        let exclusive = group.as_ref().filter(|(_, group, _)| !group.non_exclusive);
        let checkable = owner.action_flag(element, "checkable", exclusive.is_some())?;
        let checked = if let Some((owner, group, value)) = exclusive {
            checkable && owner.action_group_value(&group.key)? == Value::String((*value).into())
        } else {
            owner.control_value(element, "checked")? == Some(Value::Bool(true))
        };
        identity.present(node, enabled, checkable, checked);
        for name in ["text", "shortcut", "icon"] {
            if let Some(value) = owner.binding_value(element, name)? {
                node.properties.insert(name, value);
            }
        }
        Ok(enabled)
    }

    pub(crate) fn activate_action(&self, target: &str) -> Result<bool> {
        let (owner, element) = self.element_owner(target)?;
        let group = owner.action_group(element)?;
        if !owner.action_enabled(element, group.as_ref())? {
            return Ok(false);
        }
        let exclusive = group.as_ref().filter(|(_, group, _)| !group.non_exclusive);
        if owner.action_flag(element, "checkable", exclusive.is_some())? {
            if let Some((owner, group, value)) = exclusive {
                owner.set_control_value(group, "current-value", Value::String((*value).into()))?;
            } else {
                let checked = owner.control_value(element, "checked")? == Some(Value::Bool(true));
                owner.set_control_value(element, "checked", (!checked).into())?;
            }
        }
        owner.handler(element, "activated", &[])?;
        if let Some((owner, group, value)) = group {
            owner.handler(group, "activated", &[Value::String(value.into())])?;
        }
        Ok(true)
    }

    pub(crate) fn activate_menu_item(&self, element: &Element, node: &Node) -> Result<bool> {
        if node.boolean("checkable", false) {
            let checked = self.control_value(element, "checked")? == Some(Value::Bool(true));
            self.set_control_value(element, "checked", (!checked).into())?;
        }
        self.handler(element, "activated", &[])?;
        Ok(true)
    }

    pub(crate) fn toggle_group_box(&self, element: &Element) -> Result<bool> {
        if !self.action_flag(element, "checkable", false)? {
            return Ok(false);
        }
        let checked = self.control_value(element, "checked")? == Some(Value::Bool(false));
        self.set_control_value(element, "checked", checked.into())?;
        self.handler(element, "toggled", &[checked.into()])?;
        Ok(true)
    }

    pub(crate) fn dialog_action(&self, element: &Element, action: ControlAction) -> Result<bool> {
        if self.control_value(element, "open")? != Some(Value::Bool(true)) {
            return Ok(false);
        }
        let (event, close) = match action {
            ControlAction::Dialog(DialogRole::Accept) => ("accepted", true),
            ControlAction::Dialog(DialogRole::Reject) => ("rejected", true),
            ControlAction::DialogCommand(DialogCommand::Apply) => ("applied", false),
            ControlAction::DialogCommand(DialogCommand::Reset) => ("reset", false),
            ControlAction::DialogCommand(DialogCommand::Help) => ("help-requested", false),
            ControlAction::DialogCommand(DialogCommand::Discard) => ("discarded", true),
            _ => return Ok(false),
        };
        if close {
            self.set_control_value(element, "open", false.into())?;
        }
        self.handler(element, event, &[])?;
        Ok(true)
    }
}
