use crate::{CheckState, Node};
use std::rc::Rc;

/// The compiler's declaration-order option slots for one nonvisual group.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct ButtonGroup {
    pub key: &'static str,
    pub name: String,
    pub paths: &'static [&'static [usize]],
    pub selected: Option<usize>,
    pub enabled: bool,
    pub exclusive: bool,
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonGroupMembership {
    pub group: Rc<ButtonGroup>,
    pub index: usize,
}

pub type RadioMembership = ButtonGroupMembership;

impl ButtonGroupMembership {
    pub fn component_path<'a>(&self, path: &'a [usize]) -> Option<&'a [usize]> {
        path.strip_suffix(*self.group.paths.get(self.index)?)
    }
}

#[doc(hidden)]
pub enum ButtonGroupSelection {
    Index(i64),
    Value(String),
}

pub fn radio_value(node: &Node) -> &str {
    node.text("value")
        .or_else(|| node.text("text"))
        .unwrap_or("")
}

#[doc(hidden)]
pub fn bind_button_group(
    scene: &mut Node,
    key: &'static str,
    name: String,
    paths: &'static [&'static [usize]],
    selection: ButtonGroupSelection,
    enabled: bool,
) {
    let selected = match selection {
        ButtonGroupSelection::Index(index) => usize::try_from(index)
            .ok()
            .filter(|&index| index < paths.len()),
        ButtonGroupSelection::Value(value) => paths.iter().position(|path| {
            scene
                .at_path(path)
                .is_some_and(|node| node.kind == "RadioButton" && radio_value(node) == value)
        }),
    };
    bind_group(
        scene,
        Rc::new(ButtonGroup {
            key,
            name,
            paths,
            selected,
            enabled,
            exclusive: true,
        }),
    );
}

#[doc(hidden)]
pub fn bind_check_group(
    scene: &mut Node,
    key: &'static str,
    name: String,
    paths: &'static [&'static [usize]],
    enabled: bool,
) {
    bind_group(
        scene,
        Rc::new(ButtonGroup {
            key,
            name,
            paths,
            selected: None,
            enabled,
            exclusive: false,
        }),
    );
}

fn bind_group(scene: &mut Node, group: Rc<ButtonGroup>) {
    for (index, path) in group.paths.iter().enumerate() {
        let member = path
            .iter()
            .try_fold(&mut *scene, |node, &index| node.children.get_mut(index));
        let Some(member) = member.filter(|node| {
            if group.exclusive {
                node.kind == "RadioButton"
            } else {
                matches!(node.kind, "CheckBox" | "Switch")
            }
        }) else {
            continue;
        };
        member.button_group = Some(ButtonGroupMembership {
            group: group.clone(),
            index,
        });
        if group.exclusive {
            member
                .properties
                .insert("checked", (group.selected == Some(index)).into());
        }
        if !group.enabled {
            member.properties.insert("enabled", false.into());
        }
    }
}

/// Stop reading members once a mixed result is known. A cached property will
/// subscribe to just the members that established the result.
#[doc(hidden)]
pub fn summarize_checks(checks: &[&dyn Fn() -> CheckState]) -> CheckState {
    let Some((first, rest)) = checks.split_first() else {
        return CheckState::Unchecked;
    };
    let first = first();
    if first == CheckState::PartiallyChecked || rest.iter().any(|read| read() != first) {
        CheckState::PartiallyChecked
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Property;

    #[test]
    fn mixed_check_summary_subscribes_only_to_the_members_that_prove_it() {
        let first = Property::default();
        first.set(CheckState::Checked);
        let second = Property::default();
        second.set(CheckState::Unchecked);
        let third = Property::default();
        third.set(CheckState::Unchecked);
        let summary = Property::default();
        let read = || {
            summary.get(|| {
                summarize_checks(&[
                    &|| first.get(CheckState::default),
                    &|| second.get(CheckState::default),
                    &|| third.get(CheckState::default),
                ])
            })
        };
        assert_eq!(read(), CheckState::PartiallyChecked);
        third.set(CheckState::Checked);
        assert!(!summary.is_dirty());
        second.set(CheckState::Checked);
        assert_eq!(read(), CheckState::Checked);
        third.set(CheckState::Unchecked);
        assert!(summary.is_dirty());
        assert_eq!(read(), CheckState::PartiallyChecked);
    }
}
