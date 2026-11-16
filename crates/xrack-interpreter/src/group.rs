use crate::{Element, Error, Instance, Member, Repeat, Result, Visual};
use std::{collections::BTreeMap, rc::Rc};
use xrack_runtime::{
    ButtonGroupOptions, ButtonGroupSelection, CheckState, ListModel, Node, RowContext, Value,
};

pub(crate) struct Membership {
    pub(crate) key: String,
    pub(crate) check_property: Option<String>,
}

pub(crate) struct Members {
    paths: Rc<[Vec<usize>]>,
    keys: Vec<String>,
}

impl Element {
    pub(crate) fn is_exclusive_push_option(&self) -> bool {
        matches!(self.visual, Visual::Builtin(kind) if xrack_syntax::builtins::push_button(kind))
            && self
                .button_group
                .as_ref()
                .is_some_and(|group| group.check_property.is_none())
    }

    pub(crate) fn live_option_repeat(&self) -> Option<&Repeat> {
        self.repeat
            .as_ref()
            .filter(|repeat| !repeat.empty && repeat.static_count.is_none())
    }
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
        if let Some(empty) = element.empty_state() {
            path.push(0);
            collect(empty, path, groups);
            path.pop();
            return;
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
    pub(crate) fn radio_choice(&self, group: &Element) -> Result<Option<(usize, usize)>> {
        let cache = self
            .0
            .radio_choices
            .borrow_mut()
            .entry(group.key.clone())
            .or_default()
            .clone();
        cache.try_get(|| {
            let mut selection = self.group_selection(group)?;
            for (index, option) in group.children.iter().enumerate() {
                if matches!(selection, ButtonGroupSelection::Index(index) if index < 0) {
                    break;
                }
                let read = |owner: &Instance, read_label| {
                    if !owner.input_element_available(option, false)? {
                        Ok(None)
                    } else if read_label {
                        owner.group_option_value(option).map(Some)
                    } else {
                        Ok(Some(String::new()))
                    }
                };
                let row = if let Some(repeat) = &option.repeat {
                    if repeat.empty {
                        continue;
                    }
                    let model = self.repeat_state(option).row_model(self, repeat)?;
                    selection.try_select_radio_option(
                        model.row_count().min(4096),
                        |row, label| {
                            let projected =
                                self.sample_group_option(option, repeat, &model, row)?;
                            read(&projected, label)
                        },
                    )?
                } else {
                    selection.try_select_radio_option(1, |_, label| read(self, label))?
                };
                if let Some(row) = row {
                    return Ok(Some((index, row)));
                }
            }
            Ok(None)
        })
    }

    fn sample_group_option(
        &self,
        option: &Element,
        repeat: &Repeat,
        model: &crate::repeat::Model,
        row: usize,
    ) -> Result<Self> {
        let context = RowContext::with_sample(model.row_key(row), row, model.item(row));
        self.allocate_row(
            option,
            crate::repeat::RowScope {
                owner: Rc::downgrade(&self.0),
                root_key: option.key.clone(),
                item: repeat.item.clone(),
                index: repeat.index.clone(),
                context,
                sampled: true,
            },
        )
    }

    pub(crate) fn describe_navigation_page(
        &self,
        page: &Element,
        repeat: &Repeat,
        model: &crate::repeat::Model,
        slot: usize,
    ) -> Result<Node> {
        let sample = self.sample_group_option(page, repeat, model, slot)?;
        let kind = match page.visual {
            Visual::Builtin("Tab") => "Tab",
            Visual::Builtin("StackItem") => "StackItem",
            _ => "SwipePage",
        };
        let mut header = Node::new(kind, None);
        header
            .properties
            .insert("visible", sample.visual_flag(page, "visible", true)?.into());
        header
            .properties
            .insert("enabled", sample.visual_flag(page, "enabled", true)?.into());
        let mut properties = vec![
            "title",
            "value",
            "accessible-name",
            "accessible-description",
            "closable",
        ];
        if kind == "StackItem" {
            properties.extend([
                "width",
                "height",
                "min-width",
                "min-height",
                "preferred-width",
                "preferred-height",
                "max-width",
                "max-height",
            ]);
        }
        for property in properties {
            let value = if let Some(cases) = page.state_bindings.get(property) {
                match sample.state_value(cases)? {
                    Some(value) => Some(value),
                    None => sample.binding_value(page, property)?,
                }
            } else {
                sample.binding_value(page, property)?
            };
            if let Some(value) = value {
                header.properties.insert(property, value);
            }
        }
        Ok(header)
    }

    pub(crate) fn sample_navigation_option(
        &self,
        page: &Element,
        repeat: &Repeat,
        model: &crate::repeat::Model,
        row: usize,
        by_value: bool,
    ) -> Result<Option<String>> {
        let sample = self.sample_group_option(page, repeat, model, row)?;
        sample.live_group_option(page, by_value, sample.input_element_available(page, false)?)
    }

    pub(crate) fn navigation_choice(&self, group: &Element) -> Result<Option<usize>> {
        let cache = self
            .0
            .navigation_choices
            .borrow_mut()
            .entry(group.key.clone())
            .or_default()
            .clone();
        cache.try_get(|| {
            if matches!(group.visual, Visual::Builtin("SwipeView"))
                && !self.visual_flag(group, "visible", true)?
            {
                return Ok(None);
            }
            let layout = matches!(group.visual, Visual::Builtin("StackLayout"));
            if group.selection_property() == "current-value" {
                let ButtonGroupSelection::Value(requested) = self.group_selection(group)? else {
                    return Err(Error::Evaluation("invalid navigation value".into()));
                };
                if let Some(option) = group.children.first()
                    && let Some(repeat) = &option.repeat
                {
                    if repeat.empty {
                        return Ok(None);
                    }
                    let model = self.repeat_state(option).row_model(self, repeat)?;
                    return xrack_runtime::try_resolve_navigation_value(
                        &requested,
                        (0..model.row_count().min(4096)).map(|row| {
                            let sample = self.sample_group_option(option, repeat, &model, row)?;
                            sample.live_group_option(
                                option,
                                true,
                                layout || sample.input_element_available(option, false)?,
                            )
                        }),
                    );
                }
                return xrack_runtime::try_resolve_navigation_value(
                    &requested,
                    group.children.iter().map(|option| {
                        self.live_group_option(
                            option,
                            true,
                            layout || self.input_element_available(option, false)?,
                        )
                    }),
                );
            }
            let requested = self.current_index(group)?;
            if layout || matches!(group.visual, Visual::Builtin("TabWidget")) {
                let Some(index) = usize::try_from(requested).ok() else {
                    return Ok(None);
                };
                if let Some(tab) = group.children.first()
                    && let Some(repeat) = &tab.repeat
                {
                    if repeat.empty {
                        return Ok(None);
                    }
                    let model = self.repeat_state(tab).row_model(self, repeat)?;
                    if index >= model.row_count().min(4096) {
                        return Ok(None);
                    }
                    if layout {
                        return Ok(Some(index));
                    }
                    let sample = self.sample_group_option(tab, repeat, &model, index)?;
                    return Ok(sample.input_element_available(tab, false)?.then_some(index));
                }
                let Some(tab) = group.children.get(index) else {
                    return Ok(None);
                };
                return Ok((layout || self.input_element_available(tab, false)?).then_some(index));
            }
            if let Some(option) = group.children.first()
                && let Some(repeat) = &option.repeat
            {
                if repeat.empty {
                    return Ok(None);
                }
                let model = self.repeat_state(option).row_model(self, repeat)?;
                return xrack_runtime::try_resolve_swipe_index(
                    Some(requested),
                    (0..model.row_count().min(4096)).map(|row| {
                        let sample = self.sample_group_option(option, repeat, &model, row)?;
                        sample.input_element_available(option, false)
                    }),
                );
            }
            xrack_runtime::try_resolve_swipe_index(
                Some(requested),
                group
                    .children
                    .iter()
                    .map(|option| self.input_element_available(option, false)),
            )
        })
    }

    pub(crate) fn group_option_checked(&self, element: &Element) -> Result<bool> {
        let Some(membership) = &element.button_group else {
            return Ok(false);
        };
        let key = &membership.key;
        let (owner, group) = self.element_owner(key)?;
        let definition = owner.definition();
        let members = &definition.button_groups[key];
        match owner.group_selection(group)? {
            ButtonGroupSelection::Index(index) => Ok(usize::try_from(index)
                .ok()
                .and_then(|index| members.keys.get(index))
                == Some(&element.key)),
            ButtonGroupSelection::Value(value) => {
                if value.is_empty() {
                    return Ok(false);
                }
                for (candidate, path) in members.keys.iter().zip(members.paths.iter()) {
                    if let Some(option) = owner.mounted_group_option(&definition.root, path)?
                        && owner.group_option_value(option)? == value
                    {
                        return Ok(candidate == &element.key);
                    }
                    if candidate == &element.key {
                        break;
                    }
                }
                Ok(false)
            }
        }
    }

    fn mounted_group_option<'a>(
        &self,
        mut element: &'a Element,
        path: &[usize],
    ) -> Result<Option<&'a Element>> {
        if !self.element_conditions_met(element)? {
            return Ok(None);
        }
        for &index in path {
            let child = if let Some(empty) = element.empty_state() {
                if self.view_count(&element.key, false)? != 0 {
                    return Ok(None);
                }
                empty
            } else {
                &element.children[index - element.synthetic_child_count()]
            };
            let mounted = match element.visual {
                Visual::Builtin("TabWidget") => self.current_index(element)? == index as i64,
                Visual::Builtin("StackLayout") => self.navigation_choice(element)? == Some(index),
                Visual::Builtin("StackView") => {
                    self.visual_flag(element, "visible", true)?
                        && self.stack_snapshot_for(element)?.current() == Some(child.id.as_ref())
                }
                Visual::Builtin("SwipeView") => {
                    self.visual_flag(element, "visible", true)?
                        && xrack_runtime::try_resolve_swipe_index(
                            Some(self.current_index(element)?),
                            element
                                .children
                                .iter()
                                .map(|page| self.input_element_available(page, false)),
                        )? == Some(index)
                }
                Visual::Builtin(kind) if xrack_runtime::is_window_overlay(kind) => {
                    self.visual_flag(element, "visible", true)?
                        && self.control_value(element, "open")? == Some(Value::Bool(true))
                }
                _ => true,
            };
            if !mounted {
                return Ok(None);
            }
            element = child;
            if !self.element_conditions_met(element)? {
                return Ok(None);
            }
            if matches!(
                element.visual,
                Visual::Builtin("Tab" | "StackPage" | "SwipePage")
            ) && !(self.visual_flag(element, "visible", true)?
                && self.visual_flag(element, "enabled", true)?)
            {
                return Ok(None);
            }
        }
        Ok(Some(element))
    }

    fn group_option_value(&self, element: &Element) -> Result<String> {
        if let Some(Value::String(value)) = self.binding_value(element, "value")? {
            return Ok(value);
        }
        if let Some(cases) = element.state_bindings.get("text") {
            let selected = self.choose_state(cases[0].selector)?;
            if let Some(case) = cases.iter().find(|case| case.index == selected)
                && let Value::String(value) = self.evaluate(&case.value, &[])?
            {
                return Ok(value);
            }
        }
        Ok(match self.binding_value(element, "text")? {
            Some(Value::String(text)) => text,
            _ => String::new(),
        })
    }

    pub(crate) fn toggle_group_option(&self, element: &Element) -> Result<()> {
        let membership = element
            .button_group
            .as_ref()
            .ok_or_else(|| Error::Invalid("missing button membership".into()))?;
        let (owner, group) = self.element_owner(&membership.key)?;
        match owner.group_selection(group)? {
            ButtonGroupSelection::Index(current) => {
                let index = owner
                    .definition()
                    .button_groups
                    .get(&group.key)
                    .and_then(|members| members.keys.iter().position(|key| *key == element.key))
                    .ok_or_else(|| Error::Invalid("missing button group option".into()))?;
                owner.set_control_value(
                    group,
                    "current-index",
                    xrack_runtime::toggled_button_group_index(current, index).into(),
                )?;
            }
            ButtonGroupSelection::Value(current) => {
                if let Some(next) = xrack_runtime::toggled_button_group_value(
                    &current,
                    self.live_radio_value(element)?,
                ) {
                    owner.set_control_value(group, "current-value", next.into())?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn group_member_path(&self, element: &Element) -> Option<&[usize]> {
        let group = self
            .definition()
            .button_groups
            .get(&element.button_group.as_ref()?.key)?;
        let index = group.keys.iter().position(|key| *key == element.key)?;
        Some(&group.paths[index])
    }

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
            let options = ButtonGroupOptions::try_read(
                || {
                    Ok(match self.binding_value(group, "accessible-name")? {
                        Some(Value::String(name)) => name,
                        _ => String::new(),
                    })
                },
                || {
                    (!group.non_exclusive)
                        .then(|| self.group_selection(group))
                        .transpose()
                },
                || self.group_enabled(group),
            )?;
            xrack_runtime::bind_dynamic_button_group(
                scene,
                key.clone(),
                options.name,
                members.paths.clone(),
                options.selection,
                options.enabled,
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
        let Some((index, label, changed)) = xrack_runtime::try_prepare_button_group_option(
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
        let push_button = element.is_exclusive_push_option();
        if changed {
            let property = group.selection_property();
            let next = if property == "current-value" {
                Value::String(label.clone())
            } else {
                Value::Int(index as i64)
            };
            if !owner.set_control_value(group, property, next)? {
                return Ok(false);
            }
            if push_button {
                self.handler(element, "toggled", &[])?;
            }
            owner.handler(group, "selected", &[Value::String(label)])?;
        }
        let click = push_button
            && matches!(
                input.event(),
                xrack_runtime::RoutedEvent::Dispatch("clicked")
            );
        if click {
            self.handler(element, "clicked", &[])?;
        }
        Ok(changed || click)
    }

    pub(crate) fn group_option_model(
        &self,
        element: &Element,
    ) -> Result<Option<Rc<crate::repeat::Model>>> {
        let Some(template) = element.children.first() else {
            return Ok(None);
        };
        let Some(repeat) = template.live_option_repeat() else {
            return Ok(None);
        };
        self.repeat_state(template)
            .row_model(self, repeat)
            .map(Some)
    }

    pub(crate) fn close_tab(
        &self,
        element: &Element,
        node: &Node,
        request: xrack_runtime::TabClose,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(value) = xrack_runtime::try_prepare_tab_close(
            node,
            request,
            input,
            || {
                let model = self.group_option_model(element)?;
                let current = model
                    .as_ref()
                    .is_none_or(|model| node.group_options_are_current(model.as_ref()));
                Ok::<_, Error>(current.then_some(model))
            },
            |model, slot| {
                let (tab, owner) = if let Some(model) = model {
                    let Some(template) = element.children.first() else {
                        return Ok(None);
                    };
                    let Some(repeat) = template.live_option_repeat() else {
                        return Ok(None);
                    };
                    let row = if node.kind == "TabWidget" {
                        self.sample_group_option(template, repeat, model, slot)?
                    } else {
                        let Some(key) = node.child_at(slot).and_then(|item| item.row_key) else {
                            return Ok(None);
                        };
                        let Some(row) = self
                            .repeat_state(template)
                            .resolved_row(repeat, model, slot, key, false)
                        else {
                            return Ok(None);
                        };
                        row
                    };
                    (template, row)
                } else {
                    let Some(tab) = element.children.get(slot) else {
                        return Ok(None);
                    };
                    (tab, self.clone())
                };
                if !(owner.input_element_available(tab, false)?) {
                    return Ok(None);
                }
                let allowed = match owner.input_flag_value(tab, "closable")? {
                    Some(flag) => flag,
                    None => self
                        .input_flag_value(element, "tabs-closable")?
                        .unwrap_or(false),
                };
                if !allowed {
                    return Ok(None);
                }
                Ok(Some(match owner.binding_value(tab, "value")? {
                    Some(Value::String(value)) => value,
                    _ => String::new(),
                }))
            },
        )?
        else {
            return Ok(false);
        };
        self.handler(
            element,
            "tab-close-requested",
            &[Value::Int(request.index as i64), value.into()],
        )
    }

    pub(crate) fn move_tab(
        &self,
        element: &Element,
        node: &Node,
        movement: xrack_runtime::TabMove,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(model) = self.group_option_model(element)? else {
            return Ok(false);
        };
        let Some(template) = element.children.first() else {
            return Ok(false);
        };
        let Some(repeat) = template.live_option_repeat() else {
            return Ok(false);
        };
        if !xrack_runtime::try_prepare_tab_move(
            node,
            movement,
            input,
            &*model,
            || {
                Ok::<_, Error>(
                    self.input_flag_value(element, "reorderable")?
                        .unwrap_or(false),
                )
            },
            |slot| {
                if node.kind == "TabWidget" {
                    return Ok(self
                        .sample_navigation_option(template, repeat, &model, slot, false)?
                        .is_some());
                }
                Ok(self
                    .live_repeated_group_option(
                        template,
                        repeat,
                        &model,
                        slot,
                        node.child_at(slot).and_then(|item| item.row_key),
                        false,
                    )?
                    .is_some())
            },
        )? {
            return Ok(false);
        }
        self.commit_navigation_move(element, &model, movement.from, movement.to, || {
            input.is_current() && movement.matches_model(&*model)
        })
    }

    pub(crate) fn commit_navigation_move(
        &self,
        element: &Element,
        model: &crate::repeat::Model,
        from: usize,
        to: usize,
        valid: impl FnOnce() -> bool,
    ) -> Result<bool> {
        if from == to || from.max(to) >= model.row_count() {
            return Ok(false);
        }
        let remapped = if element.selection_property() == "current-index" {
            let current = self.current_index(element)?;
            let next = xrack_runtime::moved_navigation_index(from, to, current);
            (next != current).then_some(next)
        } else {
            None
        };
        if !valid() || !model.move_row(from, to) {
            return Ok(false);
        }
        if let Some(next) = remapped {
            self.set_control_value(element, "current-index", next.into())?;
        }
        if let Visual::Builtin(kind) = element.visual
            && let Some(event) = xrack_syntax::builtins::navigation_move_event(kind)
        {
            self.handler(
                element,
                event,
                &[Value::Int(from as i64), Value::Int(to as i64)],
            )?;
        }
        Ok(true)
    }

    pub(crate) fn select_group(
        &self,
        element: &Element,
        node: &Node,
        index: usize,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let property = element.selection_property();
        let label = if node.kind == "RadioGroup"
            && element.children.iter().any(|child| child.repeat.is_some())
        {
            self.prepare_radio_group_selection(element, node, index, input)?
        } else {
            xrack_runtime::try_prepare_group_selection_with_model(
                node,
                index,
                input,
                || {
                    let source = self.group_option_model(element)?;
                    let current = source
                        .as_ref()
                        .is_none_or(|source| node.group_options_are_current(source.as_ref()));
                    Ok::<_, Error>(current.then_some(source))
                },
                || self.group_selection(element),
                |source, index, label| {
                    if let Some(source) = source {
                        let template = &element.children[0];
                        let repeat = template.repeat.as_ref().ok_or_else(|| {
                            Error::Invalid("missing repeated group option".into())
                        })?;
                        if matches!(node.kind, "SwipeView" | "TabWidget") {
                            return self
                                .sample_navigation_option(template, repeat, source, index, label);
                        }
                        return self.live_repeated_group_option(
                            template,
                            repeat,
                            source,
                            index,
                            node.child_at(index).and_then(|item| item.row_key),
                            label,
                        );
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
                    self.live_group_option(
                        option,
                        label,
                        self.input_element_available(option, false)?,
                    )
                },
            )?
        };
        let Some(label) = label else {
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

    fn prepare_radio_group_selection(
        &self,
        element: &Element,
        node: &Node,
        index: usize,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<Option<String>> {
        xrack_runtime::try_prepare_group_selection_with_model(
            node,
            index,
            input,
            || {
                let Some((declaration, row)) = node.radio_option_location(index) else {
                    return Ok(None);
                };
                let mut chosen = None;
                for (slot, template) in element.children.iter().enumerate().take(declaration + 1) {
                    if let Some(repeat) = template.live_option_repeat() {
                        let source = self.repeat_state(template).row_model(self, repeat)?;
                        if !node.radio_option_model_is_current(slot, source.as_ref()) {
                            return Ok(None);
                        }
                        if slot == declaration {
                            chosen = Some((repeat, source));
                        }
                    }
                }
                Ok(Some((declaration, row, chosen)))
            },
            || self.group_selection(element),
            |(declaration, row, chosen), slot, label| {
                let Some(template) = element.children.get(*declaration) else {
                    return Ok(None);
                };
                if let Some((repeat, source)) = chosen {
                    return self.live_repeated_group_option(
                        template,
                        repeat,
                        source,
                        *row,
                        node.child_at(slot).and_then(|item| item.row_key),
                        label,
                    );
                }
                self.live_group_option(
                    template,
                    label,
                    self.input_element_available(template, false)?,
                )
            },
        )
    }

    fn live_repeated_group_option(
        &self,
        template: &Element,
        repeat: &Repeat,
        source: &Rc<crate::repeat::Model>,
        row: usize,
        key: Option<xrack_runtime::RowKey>,
        label: bool,
    ) -> Result<Option<String>> {
        let Some(key) = key else {
            return Ok(None);
        };
        let Some(owner) = self
            .repeat_state(template)
            .resolved_row(repeat, source, row, key, false)
        else {
            return Ok(None);
        };
        owner.live_group_option(template, label, owner.item_delegate_available(template)?)
    }

    fn live_group_option(
        &self,
        element: &Element,
        label: bool,
        available: bool,
    ) -> Result<Option<String>> {
        if !available {
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
                Ok(self.element_allows_routed_input(element, input)?
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
