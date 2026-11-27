use crate::{CheckState, InputRoute, Node};
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
        (0..self.len()).map(|index| match self {
            Self::Static(paths) => paths[index],
            Self::Owned(paths) => paths[index].as_slice(),
        })
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

impl ButtonGroupSelection {
    pub(crate) fn matches(&self, index: usize, value: &str) -> bool {
        match self {
            Self::Index(current) => usize::try_from(*current).ok() == Some(index),
            Self::Value(current) => current == value,
        }
    }

    /// Search one declaration's slots, preserving their position even when
    /// a slot is hidden. Index selection only reads the requested option.
    #[doc(hidden)]
    pub fn try_select_radio_option<E, Label: AsRef<str>>(
        &mut self,
        count: usize,
        mut option: impl FnMut(usize, bool) -> Result<Option<Label>, E>,
    ) -> Result<Option<usize>, E> {
        match self {
            Self::Index(remaining) => {
                let Some(index) = usize::try_from(*remaining).ok() else {
                    return Ok(None);
                };
                if index >= count {
                    *remaining -= count as i64;
                    return Ok(None);
                }
                *remaining = -1;
                Ok(option(index, false)?.map(|_| index))
            }
            Self::Value(value) => {
                for index in 0..count {
                    if option(index, true)?.is_some_and(|label| label.as_ref() == value.as_str()) {
                        return Ok(Some(index));
                    }
                }
                Ok(None)
            }
        }
    }

    #[doc(hidden)]
    pub fn select_radio_option<Label: AsRef<str>>(
        &mut self,
        count: usize,
        mut option: impl FnMut(usize, bool) -> Option<Label>,
    ) -> Option<usize> {
        self.try_select_radio_option(count, |index, label| {
            Ok::<_, std::convert::Infallible>(option(index, label))
        })
        .unwrap_or_else(|never| match never {})
    }
}

#[doc(hidden)]
pub fn toggled_button_group_index(current: i64, option: usize) -> i64 {
    let option = option as i64;
    if current == option { -1 } else { option }
}

#[doc(hidden)]
pub fn toggled_button_group_value(current: &str, option: String) -> Option<String> {
    if option.is_empty() {
        return None;
    }
    Some(if current == option {
        String::new()
    } else {
        option
    })
}

/// Sample the group in scene construction order.
#[doc(hidden)]
pub struct ButtonGroupOptions {
    pub name: String,
    pub selection: Option<ButtonGroupSelection>,
    pub enabled: bool,
}

impl ButtonGroupOptions {
    pub fn try_read<E>(
        name: impl FnOnce() -> Result<String, E>,
        selection: impl FnOnce() -> Result<Option<ButtonGroupSelection>, E>,
        enabled: impl FnOnce() -> Result<bool, E>,
    ) -> Result<Self, E> {
        Ok(Self {
            name: name()?,
            selection: selection()?,
            enabled: enabled()?,
        })
    }

    pub fn read(
        name: impl FnOnce() -> String,
        selection: impl FnOnce() -> Option<ButtonGroupSelection>,
        enabled: impl FnOnce() -> bool,
    ) -> Self {
        Self::try_read(
            || Ok::<_, std::convert::Infallible>(name()),
            || Ok(selection()),
            || Ok(enabled()),
        )
        .unwrap_or_else(|error| match error {})
    }
}

/// Recheck a distributed option without evaluating other members.
#[doc(hidden)]
pub fn try_prepare_button_group_option<E>(
    node: &Node,
    group: &str,
    input: InputRoute<'_>,
    enabled: impl FnOnce() -> Result<bool, E>,
    value: impl FnOnce() -> Result<String, E>,
    current: impl FnOnce() -> Result<ButtonGroupSelection, E>,
) -> Result<Option<(usize, String, bool)>, E> {
    let Some(member) = node
        .button_group
        .as_ref()
        .filter(|member| member.group.exclusive && member.group.key == group)
    else {
        return Ok(None);
    };
    if !input.try_available(enabled)? {
        return Ok(None);
    }
    let value = value()?;
    if !input.is_current() || value != radio_value(node) {
        return Ok(None);
    }
    let current = current()?;
    if matches!(&current, ButtonGroupSelection::Value(_)) && value.is_empty() {
        return Ok(None);
    }
    let unchanged = current.matches(member.index, &value);
    Ok(input
        .is_current()
        .then_some((member.index, value, !unchanged)))
}

#[doc(hidden)]
pub fn try_prepare_radio_selection<E>(
    node: &Node,
    group: &str,
    input: InputRoute<'_>,
    enabled: impl FnOnce() -> Result<bool, E>,
    value: impl FnOnce() -> Result<String, E>,
    current: impl FnOnce() -> Result<ButtonGroupSelection, E>,
) -> Result<Option<(usize, String)>, E> {
    Ok(
        try_prepare_button_group_option(node, group, input, enabled, value, current)?
            .and_then(|(index, value, changed)| changed.then_some((index, value))),
    )
}

#[doc(hidden)]
pub fn prepare_button_group_option(
    node: &Node,
    group: &str,
    input: InputRoute<'_>,
    enabled: impl FnOnce() -> bool,
    value: impl FnOnce() -> String,
    current: impl FnOnce() -> ButtonGroupSelection,
) -> Option<(usize, String, bool)> {
    try_prepare_button_group_option(
        node,
        group,
        input,
        || Ok::<_, std::convert::Infallible>(enabled()),
        || Ok(value()),
        || Ok(current()),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn prepare_radio_selection(
    node: &Node,
    group: &str,
    input: InputRoute<'_>,
    enabled: impl FnOnce() -> bool,
    value: impl FnOnce() -> String,
    current: impl FnOnce() -> ButtonGroupSelection,
) -> Option<(usize, String)> {
    try_prepare_radio_selection(
        node,
        group,
        input,
        || Ok::<_, std::convert::Infallible>(enabled()),
        || Ok(value()),
        || Ok(current()),
    )
    .unwrap_or_else(|error| match error {})
}

pub fn radio_value(node: &Node) -> &str {
    node.text("value")
        .or_else(|| node.text("text"))
        .unwrap_or("")
}

fn push_button(node: &Node) -> bool {
    matches!(node.kind, "Button" | "ToolButton" | "RoundButton")
}

#[doc(hidden)]
pub fn is_exclusive_push_option(node: &Node) -> bool {
    push_button(node)
        && node
            .button_group
            .as_ref()
            .is_some_and(|member| member.group.exclusive)
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
    bind_button_group_options(
        scene,
        key,
        paths,
        ButtonGroupOptions {
            name,
            selection: Some(selection),
            enabled,
        },
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
    bind_button_group_options(
        scene,
        key,
        paths,
        ButtonGroupOptions {
            name,
            selection: None,
            enabled,
        },
    );
}

#[doc(hidden)]
pub fn bind_button_group_options(
    scene: &mut Node,
    key: &'static str,
    paths: &'static [&'static [usize]],
    options: ButtonGroupOptions,
) {
    bind_group(
        scene,
        Cow::Borrowed(key),
        options.name,
        Paths::Static(paths),
        options.selection,
        options.enabled,
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
        Some(ButtonGroupSelection::Value(value)) if !value.is_empty() => {
            paths.iter().position(|path| {
                scene.at_path(path).is_some_and(|node| {
                    (node.kind == "RadioButton" || push_button(node)) && radio_value(node) == value
                })
            })
        }
        Some(ButtonGroupSelection::Value(_)) | None => None,
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
            if push_button(node) {
                true
            } else if group.exclusive {
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
        if push_button(member) {
            member.properties.insert("checkable", true.into());
        }
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
