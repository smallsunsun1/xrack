use crate::{CheckState, Node};
use std::{borrow::Cow, rc::Rc};

/// The compiler's declaration-order option slots for one nonvisual group.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct ButtonGroup {
    pub key: Cow<'static, str>,
    pub name: String,
    paths: Paths,
    pub selected: Option<usize>,
    pub enabled: bool,
    pub exclusive: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Paths {
    Static(&'static [&'static [usize]]),
    Owned(Rc<[Vec<usize>]>),
}

impl Paths {
    fn len(&self) -> usize {
        match self {
            Self::Static(paths) => paths.len(),
            Self::Owned(paths) => paths.len(),
        }
    }

    fn get(&self, index: usize) -> Option<&[usize]> {
        match self {
            Self::Static(paths) => paths.get(index).copied(),
            Self::Owned(paths) => paths.get(index).map(Vec::as_slice),
        }
    }

    fn iter(&self) -> impl ExactSizeIterator<Item = &[usize]> {
        (0..self.len()).map(|index| self.get(index).expect("group path index"))
    }
}

impl ButtonGroup {
    pub fn paths(&self) -> impl ExactSizeIterator<Item = &[usize]> {
        self.paths.iter()
    }
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
        path.strip_suffix(self.group.paths.get(self.index)?)
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
    bind_group(
        scene,
        Cow::Borrowed(key),
        name,
        Paths::Static(paths),
        Some(selection),
        enabled,
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
        Cow::Borrowed(key),
        name,
        Paths::Static(paths),
        None,
        enabled,
    );
}

#[doc(hidden)]
pub fn bind_dynamic_button_group(
    scene: &mut Node,
    key: String,
    name: String,
    paths: Rc<[Vec<usize>]>,
    selection: Option<ButtonGroupSelection>,
    enabled: bool,
) {
    bind_group(
        scene,
        Cow::Owned(key),
        name,
        Paths::Owned(paths),
        selection,
        enabled,
    );
}

fn bind_group(
    scene: &mut Node,
    key: Cow<'static, str>,
    name: String,
    paths: Paths,
    selection: Option<ButtonGroupSelection>,
    enabled: bool,
) {
    let exclusive = selection.is_some();
    let selected = match selection {
        Some(ButtonGroupSelection::Index(index)) => usize::try_from(index)
            .ok()
            .filter(|&index| index < paths.len()),
        Some(ButtonGroupSelection::Value(value)) => paths.iter().position(|path| {
            scene
                .at_path(path)
                .is_some_and(|node| node.kind == "RadioButton" && radio_value(node) == value)
        }),
        None => None,
    };
    let group = Rc::new(ButtonGroup {
        key,
        name,
        paths,
        selected,
        enabled,
        exclusive,
    });
    for (index, path) in group.paths.iter().enumerate() {
        let member = path
            .iter()
            .try_fold(&mut *scene, |node, &index| node.child_at_mut(index));
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
    try_summarize_checks(
        checks
            .iter()
            .map(|read| Ok::<_, std::convert::Infallible>(read())),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_summarize_checks<E>(
    checks: impl IntoIterator<Item = Result<CheckState, E>>,
) -> Result<CheckState, E> {
    let mut checks = checks.into_iter();
    let Some(first) = checks.next().transpose()? else {
        return Ok(CheckState::Unchecked);
    };
    if first == CheckState::PartiallyChecked {
        return Ok(first);
    }
    for check in checks {
        if check? != first {
            return Ok(CheckState::PartiallyChecked);
        }
    }
    Ok(first)
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
