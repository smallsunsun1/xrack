use crate::{
    repeat::{Model, RepeatState, RowScope},
    *,
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Weak,
};
use xrack_runtime::{
    Animation, Brush, CallbackSlot, Component, ControlAction, GradientStop, ImagePlaybackSlot,
    InputProperty, Length, Property, RangeAction, StateTiming,
};

pub(crate) struct State {
    program: Program,
    name: String,
    row: Option<RowScope>,
    repeats: RefCell<BTreeMap<String, Rc<RepeatState>>>,
    splits: RefCell<BTreeMap<String, Rc<split::State>>>,
    stacks: RefCell<BTreeMap<String, Rc<xrack_runtime::StackState>>>,
    timers: BTreeMap<String, xrack_runtime::TimerSlot>,
    focus: BTreeMap<String, xrack_runtime::FocusHandle>,
    hover: RefCell<BTreeMap<String, xrack_runtime::HoverHandle>>,
    gestures: gesture::Handles,
    pointers: RefCell<BTreeMap<String, xrack_runtime::PointerHandle>>,
    pub(crate) actions: BTreeMap<String, xrack_runtime::ActionKey>,
    pub(crate) native_dialogs: BTreeMap<String, native_dialog::State>,
    pub(crate) window: Rc<xrack_runtime::WindowLifecycle>,
    pub(crate) secondary_windows: Option<secondary_window::State>,
    split_input: Cell<Option<xrack_runtime::SplitHandleInput>>,
    calendar_input: Cell<Option<xrack_runtime::CalendarCell>>,
    calendars: RefCell<BTreeMap<String, Rc<calendar::State>>>,
    pickers: RefCell<BTreeMap<String, Rc<picker::State>>>,
    locale: xrack_runtime::LocaleBinding,
    incoming: BTreeMap<Member, BoundInput>,
    children: OnceCell<BTreeMap<String, Instance>>,
    values: BTreeMap<Member, Property<Result<Value>>>,
    selectors: Vec<Property<Result<i64>>>,
    visual_cache: RefCell<BTreeMap<VisualSlot, Rc<Property<Result<Value>>>>>,
    callbacks: BTreeMap<Member, CallbackSlot<CallbackBinding>>,
    controls: BTreeMap<Member, Property<Result<Option<Value>>>>,
    pub(crate) layout: viewport::Handles,
    combo_items: RefCell<BTreeMap<String, Rc<Property<Result<xrack_runtime::ComboItems>>>>>,
    image_playback: RefCell<BTreeMap<String, Rc<ImagePlaybackSlot>>>,
    table_columns: OnceCell<Property<xrack_runtime::TableColumnWindow>>,
    scene: Property<Result<Rc<Node>>>,
    pub(crate) error: Rc<Cell<Option<Error>>>,
    pub(crate) check_groups:
        RefCell<BTreeMap<String, Rc<Property<Result<xrack_runtime::CheckState>>>>>,
}

type BoundInput = InputProperty<Result<Value>>;
type CallbackBinding = Rc<dyn Fn(&[Value]) -> Result<Option<Value>>>;

fn evaluation(message: impl Into<String>) -> Error {
    Error::Evaluation(message.into())
}

impl Program {
    pub(crate) fn allocate(
        &self,
        name: &str,
        parent: Option<Weak<State>>,
        incoming: BTreeMap<String, BoundInput>,
        stack: &mut Vec<String>,
    ) -> Result<Instance> {
        let definition = self.definition(name)?;
        self.allocate_scope(name, parent, incoming, &definition.root, None, stack)
    }

    pub(crate) fn validate_dependencies(
        &self,
        name: &str,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        if !visited.insert(name.to_owned()) {
            return Ok(());
        }
        self.definition(name)?
            .root
            .validate_dependencies(self, visited)
    }

    fn definition(&self, name: &str) -> Result<&Definition> {
        self.components
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("unknown component {name}")))?
            .as_ref()
            .map_err(Clone::clone)
    }

    fn input_endpoint(&self, component: &str, name: &str) -> Result<Vec<String>> {
        let mut path = Vec::new();
        let mut component = component.to_owned();
        let mut name = name.to_owned();
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert((component.clone(), name.clone())) {
                return Err(Error::Invalid(format!(
                    "cyclic input alias {component}.{name}"
                )));
            }
            let definition = self.definition(&component)?;
            let endpoint = definition.alias_endpoint(&name)?;
            let node = definition.root.find_key(&endpoint.owner);
            if definition.properties.contains_key(&endpoint)
                || node.is_some_and(|node| matches!(node.visual, Visual::Builtin(_)))
            {
                path.extend([endpoint.owner, endpoint.name]);
                return Ok(path);
            }
            let Some(Visual::Instance {
                component: child, ..
            }) = node.map(|node| &node.visual)
            else {
                return Err(Error::Invalid(format!(
                    "unknown input alias {}.{}",
                    endpoint.owner, endpoint.name
                )));
            };
            component = child.clone();
            name = endpoint.name;
            path.push(endpoint.owner);
        }
    }

    fn allocate_scope(
        &self,
        name: &str,
        parent: Option<Weak<State>>,
        incoming: BTreeMap<String, BoundInput>,
        root: &Element,
        row: Option<RowScope>,
        stack: &mut Vec<String>,
    ) -> Result<Instance> {
        if stack.iter().any(|entry| entry == name) {
            return Err(Error::Invalid(format!("recursive component {name}")));
        }
        let definition = self.definition(name)?;
        let mut keys = BTreeSet::new();
        root.scope_keys(&mut keys);
        if row.is_none() {
            keys.insert("root");
        }
        let mut controls = BTreeMap::new();
        root.allocate_controls(&mut controls);
        let mut local = BTreeMap::new();
        let mut forwarded = BTreeMap::new();
        for (name, input) in incoming {
            let endpoint = definition.alias_endpoint(&name)?;
            let inputs = if definition.properties.contains_key(&endpoint)
                || controls.contains_key(&endpoint)
            {
                &mut local
            } else {
                &mut forwarded
            };
            if inputs.insert(endpoint, input).is_some() {
                return Err(Error::Invalid(format!(
                    "duplicate input binding for {name}"
                )));
            }
        }
        let owner = row
            .as_ref()
            .map(|scope| &scope.owner)
            .or(parent.as_ref())
            .and_then(Weak::upgrade);
        let locale = owner
            .as_ref()
            .map_or_else(xrack_runtime::LocaleBinding::default, |owner| {
                owner.locale.clone()
            });
        let window = match (&row, &owner) {
            (Some(_), Some(owner)) => owner.window.clone(),
            _ => Rc::default(),
        };
        let error = owner.map_or_else(Rc::default, |owner| owner.error.clone());
        let layout = viewport::Handles::new(definition, &keys);
        let instance = Instance(Rc::new(State {
            program: self.clone(),
            name: name.into(),
            row,
            repeats: RefCell::default(),
            splits: RefCell::default(),
            stacks: RefCell::default(),
            timers: definition
                .timer_order
                .iter()
                .filter(|key| keys.contains(key.as_str()))
                .cloned()
                .map(|key| (key, Default::default()))
                .collect(),
            focus: definition
                .focus_targets
                .iter()
                .filter(|key| keys.contains(key.as_str()))
                .cloned()
                .map(|key| (key, Default::default()))
                .collect(),
            actions: definition
                .action_keys
                .iter()
                .filter(|key| keys.contains(key.as_str()))
                .cloned()
                .map(|key| (key, Default::default()))
                .collect(),
            native_dialogs: definition
                .native_dialog_keys
                .iter()
                .filter(|key| keys.contains(key.as_str()))
                .cloned()
                .map(|key| (key, Default::default()))
                .collect(),
            window,
            secondary_windows: secondary_window::State::new(&root.secondary_windows),
            hover: RefCell::default(),
            gestures: gesture::Handles::default(),
            pointers: RefCell::default(),
            split_input: Cell::default(),
            calendar_input: Cell::default(),
            calendars: RefCell::default(),
            pickers: RefCell::default(),
            locale,
            incoming: local,
            children: OnceCell::new(),
            values: definition
                .properties
                .keys()
                .filter(|member| keys.contains(member.owner.as_str()))
                .cloned()
                .map(|member| (member, Property::default()))
                .collect(),
            selectors: definition
                .selectors
                .iter()
                .map(|_| Property::default())
                .collect(),
            visual_cache: RefCell::default(),
            callbacks: definition
                .callbacks
                .keys()
                .filter(|member| keys.contains(member.owner.as_str()))
                .cloned()
                .map(|member| (member, CallbackSlot::default()))
                .collect(),
            controls,
            layout,
            combo_items: RefCell::default(),
            image_playback: RefCell::default(),
            table_columns: OnceCell::new(),
            scene: Property::default(),
            check_groups: RefCell::default(),
            error,
        }));
        let mut children = BTreeMap::new();
        stack.push(name.into());
        let result = root.allocate_children(self, &instance, &mut forwarded, &mut children, stack);
        stack.pop();
        result?;
        if let Some((member, _)) = forwarded.first_key_value() {
            return Err(Error::Invalid(format!(
                "unknown input target {}.{}",
                member.owner, member.name
            )));
        }
        assert!(instance.0.children.set(children).is_ok());
        for (member, function) in &definition.callbacks {
            if function.alias_implementation && keys.contains(member.owner.as_str()) {
                instance.install_callback(member, instance.bind_callback(function.clone()))?;
            }
        }
        root.bind_viewport_offsets(&instance);
        Ok(instance)
    }
}

impl Definition {
    fn alias_endpoint(&self, name: &str) -> Result<Member> {
        let member = Member::root(name);
        let property = self
            .properties
            .get(&member)
            .ok_or_else(|| Error::Invalid(format!("unknown component input {name}")))?;
        Ok(property.alias.clone().unwrap_or(member))
    }
}

impl Element {
    fn validate_dependencies(
        &self,
        program: &Program,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        if let Visual::Instance { component, .. } = &self.visual {
            program.validate_dependencies(component, visited)?;
        }
        for child in self.owned_children() {
            child.validate_dependencies(program, visited)?;
        }
        Ok(())
    }

    pub(crate) fn owned_children(&self) -> impl Iterator<Item = &Self> {
        self.children
            .iter()
            .chain(&self.timers)
            .chain(&self.commands)
            .chain(&self.native_dialogs)
            .chain(&self.secondary_windows)
            .chain(self.day_cell.as_deref())
    }

    fn scope_keys<'a>(&'a self, keys: &mut BTreeSet<&'a str>) {
        keys.insert(&self.key);
        for child in self.owned_children().filter(|child| child.repeat.is_none()) {
            child.scope_keys(keys);
        }
    }

    fn allocate_children(
        &self,
        program: &Program,
        parent: &Instance,
        forwarded: &mut BTreeMap<Member, BoundInput>,
        children: &mut BTreeMap<String, Instance>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        if let Visual::Instance { component, inputs } = &self.visual {
            let incoming = self.child_inputs(program, parent, component, inputs, forwarded)?;
            let child =
                program.allocate(component, Some(Rc::downgrade(&parent.0)), incoming, stack)?;
            for (name, function) in &self.instance_handlers {
                child.install_callback(
                    &Member::root(name),
                    parent.bind_callback(function.clone()),
                )?;
            }
            children.insert(self.key.clone(), child);
        }
        for element in self.owned_children().filter(|child| child.repeat.is_none()) {
            element.allocate_children(program, parent, forwarded, children, stack)?;
        }
        Ok(())
    }

    fn child_inputs(
        &self,
        program: &Program,
        parent: &Instance,
        component: &str,
        inputs: &BTreeMap<String, Input>,
        forwarded: &mut BTreeMap<Member, BoundInput>,
    ) -> Result<BTreeMap<String, BoundInput>> {
        let mut bindings = BTreeMap::new();
        let endpoint = |name: &str| program.input_endpoint(component, name);
        for (name, input) in inputs {
            if let Some(source) = &input.source
                && bindings
                    .insert(endpoint(name)?, (name.clone(), parent.bind_input(source)))
                    .is_some()
            {
                return Err(Error::Invalid(format!(
                    "duplicate input binding for {name}"
                )));
            }
        }
        let forwards: Vec<_> = forwarded
            .keys()
            .filter(|member| member.owner == self.key)
            .cloned()
            .collect();
        for member in forwards {
            let input = forwarded.remove(&member).expect("forwarded input exists");
            bindings.insert(endpoint(&member.name)?, (member.name, input));
        }
        for (name, input) in inputs.iter().filter(|(_, input)| !input.cases.is_empty()) {
            let key = endpoint(name)?;
            let (_, base) = bindings
                .remove(&key)
                .unwrap_or_else(|| (name.clone(), BoundInput::default()));
            let cases = input.cases.clone();
            let owner = Rc::downgrade(&parent.0);
            let state =
                base.with_state(
                    move || match Instance(owner.upgrade()?).state_value(&cases) {
                        Ok(value) => value.map(Ok),
                        Err(error) => Some(Err(error)),
                    },
                );
            bindings.insert(key, (name.clone(), state));
        }
        Ok(bindings.into_values().collect())
    }

    pub(crate) fn selection_property(&self) -> &'static str {
        if self.links.contains_key("current-value")
            || self
                .bindings
                .iter()
                .any(|binding| binding.name == "current-value")
        {
            "current-value"
        } else {
            "current-index"
        }
    }

    fn has_check_state(&self) -> bool {
        self.row_links.contains_key("check-state")
            || self.links.contains_key("check-state")
            || self
                .bindings
                .iter()
                .any(|binding| matches!(binding.name, "check-state" | "tristate"))
    }

    fn control(&self, name: &str) -> Option<&Control> {
        self.controls.iter().find(|control| control.name == name)
    }

    fn allocate_controls(&self, controls: &mut BTreeMap<Member, Property<Result<Option<Value>>>>) {
        for control in &self.controls {
            controls.insert(Member::new(&self.key, control.name), Property::default());
        }
        for child in self.owned_children().filter(|child| child.repeat.is_none()) {
            child.allocate_controls(controls);
        }
    }
}

impl Instance {
    pub(super) fn program(&self) -> &Program {
        &self.0.program
    }

    pub(crate) fn definition(&self) -> &Definition {
        self.0.program.components[&self.0.name]
            .as_ref()
            .expect("validated instance definition")
    }

    pub(crate) fn allocate_row(&self, template: &Element, row: RowScope) -> Result<Self> {
        self.0.program.allocate_scope(
            &self.0.name,
            None,
            BTreeMap::new(),
            template,
            Some(row),
            &mut Vec::new(),
        )
    }

    pub(crate) fn repeat_state(&self, template: &Element) -> Rc<RepeatState> {
        self.0
            .repeats
            .borrow_mut()
            .entry(template.key.clone())
            .or_default()
            .clone()
    }

    pub(crate) fn row_owner(&self) -> Option<Self> {
        self.0.row.as_ref()?.owner.upgrade().map(Self)
    }

    pub(crate) fn row_template(&self) -> Option<&str> {
        self.0.row.as_ref().map(|row| row.root_key.as_str())
    }

    fn in_row(&self, element: &Element) -> bool {
        self.0
            .row
            .as_ref()
            .is_some_and(|row| row.root_key == element.key)
    }

    fn row_parameter(&self, name: &str) -> Result<Value> {
        if let Some(row) = &self.0.row {
            if row.item == name {
                return Ok(row.context.item());
            }
            if row.index.as_deref() == Some(name) {
                return Ok(Value::Int(row.context.index()));
            }
        }
        self.row_owner()
            .ok_or_else(|| evaluation(format!("unknown row parameter {name}")))?
            .row_parameter(name)
    }

    pub(crate) fn children(&self) -> &BTreeMap<String, Instance> {
        self.0
            .children
            .get()
            .expect("initialized instance children")
    }

    fn with_row_model<R>(
        &self,
        field: &RowField,
        action: impl FnOnce(&Self, &RowScope, &Model) -> Result<R>,
    ) -> Result<R> {
        let row = self
            .0
            .row
            .as_ref()
            .ok_or_else(|| evaluation("row binding has no row scope"))?;
        let owner = self
            .row_owner()
            .ok_or_else(|| evaluation("row binding has no live model owner"))?;
        if row.item != field.item {
            return owner.with_row_model(field, action);
        }
        let template = owner
            .definition()
            .root
            .find_key(&row.root_key)
            .ok_or_else(|| evaluation("row template is missing"))?;
        let repeat = template
            .repeat
            .as_ref()
            .ok_or_else(|| evaluation("row model is missing"))?;
        let model = owner.repeat_state(template).row_model(&owner, repeat)?;
        action(&owner, row, &model)
    }

    fn read_row_field(&self, field: &RowField) -> Result<Option<Value>> {
        self.with_row_model(field, |_, row, model| {
            Ok(match model {
                Model::Records(records) => row
                    .context
                    .read_from(records.model(), |record| {
                        if field.path.is_empty() {
                            Some(Value::Record(record.clone()))
                        } else {
                            record.field_path(&field.path).cloned()
                        }
                    })
                    .flatten(),
                Model::Strings(strings) if field.path.is_empty() => row
                    .context
                    .read_from(strings, |text| Value::String(text.clone())),
                _ => return Err(evaluation("invalid row link")),
            })
        })
    }

    fn write_row_field(
        &self,
        field: &RowField,
        expected: Option<&Value>,
        value: Value,
    ) -> Result<bool> {
        self.with_row_model(field, |owner, row, model| {
            let table = owner.definition().root.table_for_template(&row.root_key);
            let state = table.map(|table| owner.repeat_state(table));
            let history = table
                .map(|table| owner.table_history_enabled(table))
                .transpose()?
                .unwrap_or(false);
            match (model, value) {
                (Model::Strings(model), Value::String(value)) if field.path.is_empty() => {
                    let update = |text: &String| {
                        if expected.is_none_or(
                            |expected| matches!(expected, Value::String(value) if value == text),
                        ) {
                            Some(value)
                        } else {
                            None
                        }
                    };
                    let project = |text: &String| Value::String(text.clone());
                    if let Some(state) = state {
                        Ok(row.context.update_projected_in_with_history(
                            model,
                            &state.table_state().string_history,
                            history,
                            update,
                            table_edit::capture_string,
                            project,
                        ))
                    } else {
                        Ok(row.context.update_projected_in(model, update, project))
                    }
                }
                (Model::Records(records), value) => {
                    let update = |record: &xrack_runtime::RecordValue| {
                        if expected.is_some() && record.field_path(&field.path) != expected {
                            return None;
                        }
                        if field.path.is_empty() {
                            match value {
                                Value::Record(replacement)
                                    if replacement.name() == record.name() =>
                                {
                                    Some(replacement)
                                }
                                _ => None,
                            }
                        } else {
                            record.with_field(&field.path, value)
                        }
                    };
                    let project =
                        |record: &xrack_runtime::RecordValue| Value::Record(record.clone());
                    if let Some(state) = state {
                        Ok(row.context.update_projected_in_with_history(
                            records.model(),
                            &state.table_state().history,
                            history,
                            update,
                            |before, after, patch| patch.record_changes(before, after),
                            project,
                        ))
                    } else {
                        Ok(row
                            .context
                            .update_projected_in(records.model(), update, project))
                    }
                }
                _ => Err(evaluation("invalid row link value")),
            }
        })
    }

    fn bind_input(&self, source: &InputSource) -> BoundInput {
        let owner = Rc::downgrade(&self.0);
        match source {
            InputSource::Binding(expression) => {
                let expression = expression.clone();
                BoundInput::bound(move || {
                    let owner = owner
                        .upgrade()
                        .map(Instance)
                        .ok_or_else(|| evaluation("component parent is no longer available"))?;
                    owner.evaluate(&expression, &[])
                })
            }
            InputSource::RowLink(field) => {
                let read_field = field.clone();
                let write_field = field.clone();
                let write_owner = owner.clone();
                BoundInput::linked(
                    move || {
                        Instance(owner.upgrade()?)
                            .read_row_field(&read_field)
                            .transpose()
                    },
                    move |value| {
                        if let Some(owner) = write_owner.upgrade() {
                            let instance = Instance(owner);
                            if let Err(error) = value.and_then(|value| {
                                instance.write_row_field(&write_field, None, value)
                            }) {
                                instance.0.error.set(Some(error));
                            }
                        }
                    },
                )
            }
            InputSource::Linked(target) => {
                let read_target = target.clone();
                let write_target = target.clone();
                let write_owner = owner.clone();
                BoundInput::linked(
                    move || Some(Instance(owner.upgrade()?).read(&read_target)),
                    move |value| {
                        if let Some(owner) = write_owner.upgrade() {
                            let instance = Instance(owner);
                            if let Err(error) =
                                value.and_then(|value| instance.write(&write_target, value))
                            {
                                instance.0.error.set(Some(error));
                            }
                        }
                    },
                )
            }
        }
    }

    fn property(&self, member: &Member) -> Result<&PropertyDef> {
        self.definition()
            .properties
            .get(member)
            .ok_or_else(|| evaluation(format!("unknown property {}.{}", member.owner, member.name)))
    }

    pub(crate) fn require_public(&self, member: &Member, write: bool) -> Result<&PropertyDef> {
        let property = self.property(member)?;
        if !property.public {
            return Err(evaluation(format!("property {} is private", member.name)));
        }
        if write && !property.writable {
            return Err(evaluation(format!("property {} is read-only", member.name)));
        }
        Ok(property)
    }

    pub(crate) fn read(&self, member: &Member) -> Result<Value> {
        if let Some(child) = self.children().get(&member.owner) {
            let member = Member::root(&member.name);
            child.require_public(&member, false)?;
            return child.read(&member);
        }
        if self.is_check_group(member) {
            return self.check_group_state(&member.owner).map(Value::CheckState);
        }
        if let Some(element) = self.control_element(member) {
            return self
                .control_value(element, &member.name)?
                .map_or_else(|| self.default_control_value(element, &member.name), Ok);
        }
        if !self.0.values.contains_key(member)
            && let Some(owner) = self.row_owner()
        {
            return owner.read(member);
        }
        let property = self.property(member)?;
        if let Some(target) = &property.alias {
            return self.read(target);
        }
        let initial = || self.evaluate(&property.initial, &[]);
        if let Some(input) = self.0.incoming.get(member) {
            input.get(initial)
        } else {
            self.0.values[member].get(initial)
        }
    }

    pub(crate) fn state_value(&self, cases: &[StateCase]) -> Result<Option<Value>> {
        for case in cases {
            if self.evaluate(&case.selector, &[])? == Value::Int(case.index) {
                return self.evaluate(&case.value, &[]).map(Some);
            }
        }
        Ok(None)
    }

    pub(crate) fn write(&self, member: &Member, value: Value) -> Result<()> {
        let value = model_value(value);
        if let Some(child) = self.children().get(&member.owner) {
            let member = Member::root(&member.name);
            child.require_public(&member, true)?;
            return child.write(&member, value);
        }
        if self.is_check_group(member) {
            return self.set_check_group(&member.owner, value);
        }
        if let Some(element) = self.control_element(member) {
            self.set_control_value(element, &member.name, value)?;
            return Ok(());
        }
        if !self.0.values.contains_key(member)
            && let Some(owner) = self.row_owner()
        {
            return owner.write(member, value);
        }
        let property = self.property(member)?;
        if !property.ty.matches(&value) {
            return Err(evaluation(format!(
                "wrong value type for {}.{}: expected {:?}",
                member.owner, member.name, property.ty
            )));
        }
        if let Some(target) = &property.alias {
            return self.write(target, value);
        }
        let equal = |left: &Result<Value>, right: &Result<Value>| matches!((left, right), (Ok(left), Ok(right)) if left.eq_with_image_assets(right));
        if let Some(input) = self.0.incoming.get(member) {
            input.set_if_changed_by(Ok(value), equal);
        } else {
            self.0.values[member].set_if_changed_by(Ok(value), equal);
        }
        Ok(())
    }

    pub(crate) fn reset_member(&self, member: &Member) -> Result<()> {
        if let Some(child) = self.children().get(&member.owner) {
            return child.reset(&member.name);
        }
        if let Some(control) = self.0.controls.get(member) {
            if let Some(input) = self.0.incoming.get(member) {
                input.reset();
            } else {
                control.reset();
            }
            return Ok(());
        }
        if !self.0.values.contains_key(member)
            && let Some(owner) = self.row_owner()
        {
            return owner.reset_member(member);
        }
        if let Some(target) = &self.property(member)?.alias {
            return self.reset_member(target);
        }
        if let Some(input) = self.0.incoming.get(member) {
            input.reset();
        } else {
            self.0.values[member].reset();
        }
        Ok(())
    }

    fn control_element(&self, member: &Member) -> Option<&Element> {
        self.0
            .controls
            .contains_key(member)
            .then(|| self.definition().root.find_key(&member.owner))
            .flatten()
    }

    fn bind_callback(&self, function: Function) -> CallbackBinding {
        let owner = Rc::downgrade(&self.0);
        Rc::new(move |args| {
            let owner = owner
                .upgrade()
                .map(Instance)
                .ok_or_else(|| evaluation("callback owner was released"))?;
            owner.execute(&function, args, None)
        })
    }

    pub(crate) fn register_callback(&self, member: &Member, callback: Callback) -> Result<()> {
        let function = self.definition().callbacks.get(member).ok_or_else(|| {
            evaluation(format!("unknown callback {}.{}", member.owner, member.name))
        })?;
        let parameters = function.parameters.clone();
        let result_type = function.result.clone();
        let name = member.name.clone();
        let program = self.program().clone();
        self.install_callback(
            member,
            Rc::new(move |args| {
                program.validate_host_arguments(&name, &parameters, args)?;
                let host: Vec<_> = args.iter().cloned().map(host_value).collect();
                let result = callback(&host).map_err(Error::Evaluation)?;
                program.validate_host_result(&name, result_type.as_ref(), result.as_ref())?;
                Ok(result.map(model_value))
            }),
        )
    }

    fn install_callback(&self, member: &Member, callback: CallbackBinding) -> Result<()> {
        if let Some(child) = self.children().get(&member.owner) {
            return child.install_callback(&Member::root(&member.name), callback);
        }
        if !self.0.callbacks.contains_key(member)
            && let Some(owner) = self.row_owner()
        {
            return owner.install_callback(member, callback);
        }
        if let Some(target) = self
            .definition()
            .callbacks
            .get(member)
            .and_then(|function| function.alias.as_ref())
        {
            return self.install_callback(target, callback);
        }
        let slot = self.0.callbacks.get(member).ok_or_else(|| {
            evaluation(format!("unknown callback {}.{}", member.owner, member.name))
        })?;
        slot.set(callback);
        Ok(())
    }

    pub(crate) fn call(&self, member: &Member, arguments: &[Value]) -> Result<Option<Value>> {
        if let Some(child) = self.children().get(&member.owner) {
            return child.call(&Member::root(&member.name), arguments);
        }
        if !self.0.callbacks.contains_key(member)
            && let Some(owner) = self.row_owner()
        {
            return owner.call(member, arguments);
        }
        let function = self.definition().callbacks.get(member).ok_or_else(|| {
            evaluation(format!("unknown callback {}.{}", member.owner, member.name))
        })?;
        if let Some(target) = &function.alias {
            return self.call(target, arguments);
        }
        let slot = &self.0.callbacks[member];
        self.execute(function, arguments, Some(slot))
    }

    fn hover_handle(&self, element: &Element) -> xrack_runtime::HoverHandle {
        self.0
            .hover
            .borrow_mut()
            .entry(element.key.clone())
            .or_default()
            .clone()
    }

    fn pointer_handle(&self, element: &Element) -> xrack_runtime::PointerHandle {
        self.0
            .pointers
            .borrow_mut()
            .entry(element.key.clone())
            .or_default()
            .clone()
    }

    fn focus_handle(&self, target: &str) -> Result<xrack_runtime::FocusHandle> {
        if let Some(handle) = self.0.focus.get(target) {
            return Ok(handle.clone());
        }
        self.row_owner()
            .ok_or_else(|| evaluation(format!("unmounted focus target {target}")))?
            .focus_handle(target)
    }

    pub(crate) fn execute(
        &self,
        function: &Function,
        args: &[Value],
        slot: Option<&CallbackSlot<CallbackBinding>>,
    ) -> Result<Option<Value>> {
        if args.len() != function.parameters.len()
            || !args
                .iter()
                .zip(&function.parameters)
                .all(|(arg, ty)| ty.matches(arg))
        {
            return Err(evaluation("callback argument types do not match"));
        }
        if function.pure
            && let Some(slot) = slot
        {
            slot.track();
        }
        let mut fallback = None;
        for statement in &function.body {
            match statement {
                Statement::Assign(target, expression) => {
                    self.write(target, self.evaluate(expression, args)?)?;
                }
                Statement::Call(target, arguments) => {
                    self.call(target, &self.arguments(arguments, args)?)?;
                }
                Statement::Scroll(target, command, arguments) => {
                    self.scroll_command(target, *command, &self.arguments(arguments, args)?)?;
                }
                Statement::SelectSource(target, index, column) => {
                    let index = self.int_argument(index, args)?;
                    let column = column
                        .as_ref()
                        .map(|column| self.int_argument(column, args))
                        .transpose()?;
                    self.select_source(target, index, column)?;
                }
                Statement::Table(target, command, operands) => {
                    self.table_command(target, *command, &self.arguments(operands, args)?)?;
                }
                Statement::Focus(target, command, operands) => {
                    let action = command.action(&self.arguments(operands, args)?)?;
                    self.focus_handle(target)?.request(action);
                }
                Statement::Tree(target, command, key, depth) => {
                    let key = self.int_argument(key, args)?;
                    let depth = depth
                        .as_ref()
                        .map(|depth| self.int_argument(depth, args))
                        .transpose()?;
                    self.tree_command(target, command.action(key, depth)?)?;
                }
                Statement::Timer(target, command) => self.timer_command(target, *command)?,
                Statement::StackNavigate(target, command, argument) => {
                    let argument = argument
                        .as_ref()
                        .map(|value| self.evaluate(value, args))
                        .transpose()?;
                    self.navigate_stack_command(target, *command, argument)?;
                }
                Statement::SetCheckGroup(target, state) => {
                    self.set_check_group(target, self.evaluate(state, args)?)?;
                }
                Statement::SwipeCancel(target) => {
                    let (owner, element) = self.element_owner(target)?;
                    owner.0.gestures.swipe(&element.key).cancel();
                }
                Statement::SplitRestore(target, state) => {
                    let Value::String(serialized) = self.evaluate(state, args)? else {
                        return Err(evaluation("split state must be a string"));
                    };
                    self.restore_split(target, &serialized)?;
                }
                Statement::Popup(target, open) => {
                    self.set_overlay(target, *open)?;
                }
                Statement::NativeDialogOpen(target) => self.open_native_dialog(target)?,
                Statement::WindowClose => self.0.window.request_close(),
                Statement::SecondaryWindow(target, command) => {
                    self.secondary_window_command(target, *command)?;
                }
                Statement::WindowCommand(command) => self.0.window.request(*command),
                Statement::WindowFullScreen(enabled) => {
                    let Value::Bool(enabled) = self.evaluate(enabled, args)? else {
                        return Err(evaluation("expected a boolean full-screen argument"));
                    };
                    self.0
                        .window
                        .request(xrack_runtime::WindowCommand::SetFullScreen(enabled));
                }
                Statement::Return(expression) => {
                    fallback = Some(expression);
                    break;
                }
            }
        }
        let result = match slot.and_then(CallbackSlot::get) {
            Some(callback) => callback(args)?,
            None => fallback
                .map(|expression| self.evaluate(expression, args))
                .transpose()?,
        };
        match (function.result.as_ref(), &result) {
            (None, None) => Ok(result),
            (Some(ty), Some(value)) if ty.matches(value) => Ok(result),
            _ => Err(evaluation("callback return type does not match")),
        }
    }

    fn arguments(&self, expressions: &[Expr], args: &[Value]) -> Result<Vec<Value>> {
        expressions
            .iter()
            .map(|expr| self.evaluate(expr, args))
            .collect()
    }

    fn playback_slot(&self, key: &str) -> Rc<ImagePlaybackSlot> {
        self.0
            .image_playback
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone()
    }

    fn cached_visual(&self, slot: VisualSlot, value: &Expr) -> Result<Value> {
        let mut owner = self.clone();
        if slot.uses_component_scope() {
            // Repeated scopes share invariants with their owning component.
            // Component instances inside a row still have independent state.
            while let Some(parent) = owner.row_owner() {
                owner = parent;
            }
        }
        let cache = owner
            .0
            .visual_cache
            .borrow_mut()
            .entry(slot)
            .or_default()
            .clone();
        cache.get(|| owner.evaluate(value, &[]))
    }

    #[cfg(test)]
    pub(crate) fn visual_cache_state(&self) -> BTreeMap<VisualCacheGroup, bool> {
        self.0
            .visual_cache
            .borrow()
            .iter()
            .filter_map(|(&slot, value)| match slot {
                VisualSlot::Planned(group) => Some((group, value.is_dirty())),
                VisualSlot::Operand(_) => None,
            })
            .collect()
    }

    pub(crate) fn evaluate(&self, expression: &Expr, args: &[Value]) -> Result<Value> {
        use Expression::*;
        match expression.as_ref() {
            Literal(value) => Ok(value.clone()),
            CachedVisual(group, value) => self.cached_visual(*group, value),
            Read(member) => self.read(member),
            GeometryRead(target, metric) => self.geometry_read(target, *metric),
            ViewportRead(target, metric) => Ok(metric.read(&self.viewport(target)?)),
            StackRead(target, read, route) => {
                let route = route
                    .as_ref()
                    .map(|value| self.evaluate(value, args))
                    .transpose()?;
                read.read(self.stack_snapshot(target)?.as_ref(), route)
            }
            SplitSave(target) => self.save_split(target).map(Value::String),
            SplitResizing(target) => self.split_resizing(target).map(Value::Bool),
            TreeQuery(target, query, key) => {
                self.tree_query(target, *query, self.int_argument(key, args)?)
            }
            ViewIndex(target, index, from_source) => {
                let index = self.int_argument(index, args)?;
                Ok(Value::Int(
                    self.view_index(target, index, *from_source)?
                        .map_or(-1, |index| index as i64),
                ))
            }
            TableRead(member) => self.table_read(member),
            ScrollControlRead(member) => self.scroll_control_read(member),
            PopupOpen(key) => Ok(Value::Bool(self.popup_open(key)?)),
            ImageDimension(input, axis) => {
                let source = image_asset(self.evaluate(input, args)?)?;
                Ok(Value::Int(i64::from(source.size()[*axis])))
            }
            NineSlice(input, edges) => {
                let source = image_asset(self.evaluate(input, args)?)?;
                source
                    .with_nine_slice(*edges)
                    .map(Value::ImageAsset)
                    .map_err(|error| evaluation(error.to_string()))
            }
            ImagePlaybackRead(member) => {
                let slot = self.playback_slot(&member.owner);
                let metric = match member.name.as_str() {
                    "current-frame" => slot.current_frame(),
                    "frame-count" => slot.frame_count(),
                    _ => return Err(evaluation("invalid image playback metric")),
                };
                Ok(Value::Int(metric))
            }
            Param(index) => args
                .get(*index)
                .cloned()
                .ok_or_else(|| evaluation("unbound parameter")),
            RowParam(name) => self.row_parameter(name),
            SplitHandleParam(parameter) => self
                .0
                .split_input
                .get()
                .map(|input| parameter.value(input))
                .ok_or_else(|| evaluation("split handle parameter outside a handle")),
            CalendarCellParam(parameter) => self
                .0
                .calendar_input
                .get()
                .map(|cell| parameter.read(cell))
                .ok_or_else(|| evaluation("calendar cell parameter outside a DayCell")),
            Unary(operator, operand) => {
                let value = self.evaluate(operand, args)?;
                match (operator, value) {
                    (crate::Unary::Not, Value::Bool(v)) => Ok(Value::Bool(!v)),
                    (crate::Unary::Negate, Value::Int(v)) => Ok(Value::Int(v.wrapping_neg())),
                    (crate::Unary::Negate, Value::Float(v)) => Ok(Value::Float(-v)),
                    (crate::Unary::Negate, Value::Length(Length(v))) => {
                        Ok(Value::Length(Length(-v)))
                    }
                    _ => Err(evaluation("invalid unary operand")),
                }
            }
            Binary(operator, lhs, rhs) => {
                let left = self.evaluate(lhs, args)?;
                if (*operator == crate::Binary::And && left == Value::Bool(false))
                    || (*operator == crate::Binary::Or && left == Value::Bool(true))
                {
                    return Ok(left);
                }
                binary(self.program(), *operator, left, self.evaluate(rhs, args)?)
            }
            Select(condition, yes, no) => match self.evaluate(condition, args)? {
                Value::Bool(true) => self.evaluate(yes, args),
                Value::Bool(false) => self.evaluate(no, args),
                _ => Err(evaluation("invalid condition")),
            },
            Cast(input) => match self.evaluate(input, args)? {
                Value::Int(v) => Ok(Value::Float(v as f64)),
                Value::Percent(v) => Ok(Value::Float(v.0)),
                _ => Err(evaluation("invalid cast operand")),
            },
            Stringify(input) => Ok(Value::String(match self.evaluate(input, args)? {
                Value::Int(v) => v.to_string(),
                Value::Float(v) => v.to_string(),
                Value::Bool(v) => v.to_string(),
                Value::Date(v) => v.to_string(),
                Value::Time(v) => v.to_string(),
                _ => return Err(evaluation("invalid stringify operand")),
            })),
            SolidBrush(input) => match self.evaluate(input, args)? {
                Value::Color(color) => Ok(Value::Brush(Brush::Solid(color))),
                _ => Err(evaluation("invalid brush operand")),
            },
            Gradient {
                angle,
                stops,
                cache,
            } => {
                let build = || {
                    let stops = stops
                        .iter()
                        .map(|(color, offset)| {
                            let Value::Color(color) = self.evaluate(color, args)? else {
                                return Err(evaluation("invalid gradient stop color"));
                            };
                            let Value::Float(offset) = self.evaluate(offset, args)? else {
                                return Err(evaluation("invalid gradient stop offset"));
                            };
                            Ok(GradientStop { color, offset })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into();
                    let brush = if let Some(angle) = angle {
                        let Value::Float(angle_degrees) = self.evaluate(angle, args)? else {
                            return Err(evaluation("invalid gradient angle"));
                        };
                        Brush::Linear {
                            angle_degrees,
                            stops,
                        }
                    } else {
                        Brush::Radial { stops }
                    };
                    Ok(Value::Brush(brush))
                };
                if let Some(cache) = cache {
                    cache.get_or_init(build).clone()
                } else {
                    build()
                }
            }
            StateSelect(index) => self.0.selectors[*index]
                .get(|| {
                    for (index, condition) in self.definition().selectors[*index].iter().enumerate()
                    {
                        match self.evaluate(condition, &[])? {
                            Value::Bool(true) => return Ok(index as i64),
                            Value::Bool(false) => {}
                            _ => return Err(evaluation("invalid state condition")),
                        }
                    }
                    Ok(-1)
                })
                .map(Value::Int),
            Translate { id, arguments } => {
                use xrack_runtime::TranslationValue;
                let catalog = self
                    .0
                    .program
                    .translations
                    .as_ref()
                    .ok_or_else(|| evaluation("missing translation catalog"))?;
                let args = arguments
                    .iter()
                    .map(|(name, expr)| {
                        let value = match self.evaluate(expr, args)? {
                            Value::Int(value) => TranslationValue::from(value),
                            Value::Float(value) => TranslationValue::from(value),
                            Value::String(value) => TranslationValue::from(value),
                            _ => return Err(evaluation("invalid translation argument")),
                        };
                        Ok((name.as_str(), value))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::String(catalog.translate(&self.0.locale, id, &args)))
            }
            Record { name, fields } => {
                let fields = fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.evaluate(value, args)?)))
                    .collect::<Result<_>>()?;
                Ok(Value::Record(RecordValue::new(name.clone(), fields)))
            }
            RecordArray(name, items) => {
                let rows = self
                    .arguments(items, args)?
                    .into_iter()
                    .map(|row| match row {
                        Value::Record(record) if record.name() == name.as_ref() => Ok(record),
                        _ => Err(evaluation("invalid record list item")),
                    })
                    .collect::<Result<_>>()?;
                Ok(Value::RecordList(xrack_runtime::RecordList::new(
                    name.clone(),
                    rows,
                )))
            }
            ActionGroupRead(target) => self.action_group_value(target),
            NativeDialogRead(target, field) => self.native_dialog_read(target, *field),
            WindowRead(metric) => Ok(self.0.window.get(*metric).into()),
            HoverRead(target, read) => {
                let (owner, element) = self.element_owner(target)?;
                Ok(read.read(&owner.hover_handle(element)))
            }
            GestureRead(target, read) => {
                let (owner, element) = self.element_owner(target)?;
                Ok(owner.0.gestures.read(&element.key, *read))
            }
            PointerRead(target, read) => {
                let (owner, element) = self.element_owner(target)?;
                Ok(read.read(&owner.pointer_handle(element)))
            }
            PointerButtons(items) => {
                let buttons = self
                    .arguments(items, args)?
                    .into_iter()
                    .map(|value| match value {
                        Value::PointerButton(button) => Ok(button),
                        _ => Err(evaluation("expected a pointer button")),
                    })
                    .collect::<Result<xrack_runtime::PointerButtons>>()?;
                Ok(Value::PointerButtons(buttons))
            }
            FocusRead(target, within) => {
                let handle = self.focus_handle(target)?;
                Ok(Value::Bool(if *within {
                    handle.focus_within()
                } else {
                    handle.has_focus()
                }))
            }
            Field(input, name) => {
                let value = match self.evaluate(input, args)? {
                    Value::Record(record) => record.field(name).cloned(),
                    Value::KeyEvent(event) => match name.as_str() {
                        "key" => Some(Value::String(event.key)),
                        "text" => Some(Value::String(event.text)),
                        "repeat" => Some(Value::Bool(event.repeat)),
                        "modifiers" => Some(Value::KeyModifiers(event.modifiers)),
                        _ => None,
                    },
                    Value::PointerEvent(event) => match name.as_str() {
                        "kind" => Some(Value::PointerEventKind(event.kind)),
                        "button" => Some(Value::PointerButton(event.button)),
                        "x" => Some(Value::Length(event.x)),
                        "y" => Some(Value::Length(event.y)),
                        "inside" => Some(Value::Bool(event.inside)),
                        "is-touch" => Some(Value::Bool(event.is_touch)),
                        "touch-finger-id" => Some(Value::Int(event.touch_finger_id)),
                        "modifiers" => Some(Value::KeyModifiers(event.modifiers)),
                        _ => None,
                    },
                    Value::TouchPoint(point) => match name.as_str() {
                        "id" => Some(Value::Int(point.id)),
                        "x" => Some(Value::Length(point.x)),
                        "y" => Some(Value::Length(point.y)),
                        "start-x" => Some(Value::Length(point.start_x)),
                        "start-y" => Some(Value::Length(point.start_y)),
                        "inside" => Some(Value::Bool(point.inside)),
                        _ => None,
                    },
                    Value::WheelEvent(event) => match name.as_str() {
                        "x" => Some(Value::Length(event.x)),
                        "y" => Some(Value::Length(event.y)),
                        "delta-x" => Some(Value::Length(event.delta_x)),
                        "delta-y" => Some(Value::Length(event.delta_y)),
                        "lines-x" => Some(Value::Float(event.lines_x)),
                        "lines-y" => Some(Value::Float(event.lines_y)),
                        "is-line" => Some(Value::Bool(event.is_line)),
                        "modifiers" => Some(Value::KeyModifiers(event.modifiers)),
                        _ => None,
                    },
                    Value::KeyModifiers(modifiers) => match name.as_str() {
                        "control" => Some(Value::Bool(modifiers.control)),
                        "shift" => Some(Value::Bool(modifiers.shift)),
                        "alt" => Some(Value::Bool(modifiers.alt)),
                        "meta" => Some(Value::Bool(modifiers.meta)),
                        "primary" => Some(Value::Bool(modifiers.primary)),
                        _ => None,
                    },
                    Value::Date(date) => match name.as_str() {
                        "year" => Some(Value::Int(date.year)),
                        "month" => Some(Value::Int(date.month)),
                        "day" => Some(Value::Int(date.day)),
                        _ => None,
                    },
                    Value::Font(font) => match name.as_str() {
                        "family" => Some(Value::String(font.family)),
                        "size" => Some(Value::Length(font.size)),
                        "weight" => Some(Value::Int(font.weight)),
                        "italic" => Some(Value::Bool(font.italic)),
                        _ => None,
                    },
                    Value::Time(time) => match name.as_str() {
                        "hour" => Some(Value::Int(time.hour)),
                        "minute" => Some(Value::Int(time.minute)),
                        "second" => Some(Value::Int(time.second)),
                        _ => None,
                    },
                    _ => None,
                };
                value.ok_or_else(|| evaluation(format!("unknown field {name}")))
            }
            MakeDate([year, month, day]) => Ok(Value::Date(xrack_runtime::Date {
                year: self.int_argument(year, args)?,
                month: self.int_argument(month, args)?,
                day: self.int_argument(day, args)?,
            })),
            MakeTime([hour, minute, second]) => Ok(Value::Time(
                xrack_runtime::Time {
                    hour: self.int_argument(hour, args)?,
                    minute: self.int_argument(minute, args)?,
                    second: self.int_argument(second, args)?,
                }
                .normalized(),
            )),
            MakeFont(fields) => {
                let values = self.arguments(fields, args)?;
                let [
                    Value::String(family),
                    Value::Length(size),
                    Value::Int(weight),
                    Value::Bool(italic),
                ] = values.as_slice()
                else {
                    return Err(evaluation("invalid font constructor arguments"));
                };
                Ok(Value::Font(xrack_runtime::Font {
                    family: family.clone(),
                    size: *size,
                    weight: *weight,
                    italic: *italic,
                }))
            }
            Array(items) => Ok(Value::StringModel(xrack_runtime::VecModel::from(
                self.arguments(items, args)?
                    .into_iter()
                    .map(|item| match item {
                        Value::String(text) => Ok(text),
                        _ => Err(evaluation("invalid string list item")),
                    })
                    .collect::<Result<Vec<_>>>()?,
            ))),
            Call(target, arguments) => self
                .call(target, &self.arguments(arguments, args)?)?
                .ok_or_else(|| evaluation("expression callback must return a value")),
        }
    }

    fn int_argument(&self, expression: &Expr, args: &[Value]) -> Result<i64> {
        let Value::Int(value) = self.evaluate(expression, args)? else {
            return Err(evaluation("expected integer argument"));
        };
        Ok(value)
    }

    pub(crate) fn render_scene(&self) -> Result<Rc<Node>> {
        self.0.scene.get(|| {
            let mut node = self.render_element(&self.definition().root)?;
            self.bind_button_groups(&mut node)?;
            node.set_loaded_fonts(self.0.program.fonts.clone());
            Ok(Rc::new(node))
        })
    }

    pub(crate) fn render_template(
        &self,
        template: &Element,
        columns: &xrack_runtime::TableColumnWindow,
    ) -> Result<Node> {
        if matches!(template.visual, Visual::Builtin("TableRow")) {
            self.0
                .table_columns
                .get_or_init(Default::default)
                .set_if_changed(columns.clone());
        }
        if template
            .repeat
            .as_ref()
            .is_some_and(|repeat| repeat.scene_cache.is_some())
        {
            return self.render_element(template);
        }
        self.0
            .scene
            .get(|| self.render_element(template).map(Rc::new))
            .map(|node| node.as_ref().clone())
    }

    fn record_combo_model(&self, combo: &RecordCombo) -> Result<xrack_runtime::RecordList> {
        let Value::RecordList(model) = self.evaluate(&combo.model, &[])? else {
            return Err(evaluation("expected a record ComboBox model"));
        };
        self.program().validate_live_record_list(&model)?;
        Ok(model)
    }

    fn render_combo(&self, element: &Element, combo: &RecordCombo, node: &mut Node) -> Result<()> {
        let cache = self
            .0
            .combo_items
            .borrow_mut()
            .entry(element.key.clone())
            .or_default()
            .clone();
        let items = cache.get(|| {
            let model = self.record_combo_model(combo)?.model().clone();
            let label = combo.label.clone();
            let value = combo.value.clone();
            let enabled = combo.enabled.clone();
            Ok(xrack_runtime::ComboItems::with_enabled(
                model,
                move |record| match record.field_path(&label) {
                    Some(Value::String(text)) => text.as_str(),
                    _ => "",
                },
                move |record| {
                    record
                        .field_path(&value)
                        .cloned()
                        .unwrap_or_else(|| Value::String(String::new()))
                },
                move |record| {
                    enabled
                        .as_ref()
                        .is_none_or(|path| record.field_path(path) == Some(&Value::Bool(true)))
                },
            ))
        })?;
        let index = node.int("current-index").unwrap_or(0);
        let snapshot = items.snapshot(index, node.properties.get("current-value"));
        node.properties.insert("model", Value::ComboModel(snapshot));
        Ok(())
    }

    fn animation(&self, spec: &AnimationSpec) -> Result<Option<Animation>> {
        if let Some(enabled) = &spec.enabled
            && self.evaluate(enabled, &[])? == Value::Bool(false)
        {
            return Ok(None);
        }
        let Value::Int(duration) = self.evaluate(&spec.duration, &[])? else {
            return Err(evaluation("invalid animation duration"));
        };
        let duration =
            u64::try_from(duration).map_err(|_| evaluation("invalid animation duration"))?;
        Ok(Some(Animation::new(duration, spec.delay_ms, spec.easing)))
    }

    pub(crate) fn timer_slot(&self, key: &str) -> Result<&xrack_runtime::TimerSlot> {
        self.0
            .timers
            .get(key)
            .ok_or_else(|| evaluation(format!("unknown timer {key}")))
    }

    pub(crate) fn split_state(&self, key: &str) -> Rc<split::State> {
        self.0
            .splits
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone()
    }

    pub(crate) fn render_split_handle(
        &self,
        template: &Element,
        input: xrack_runtime::SplitHandleInput,
    ) -> Result<Node> {
        let previous = self.0.split_input.replace(Some(input));
        let result = self.render_element(template);
        self.0.split_input.set(previous);
        result
    }

    pub(crate) fn stack_state(&self, element: &Element) -> Rc<xrack_runtime::StackState> {
        self.0
            .stacks
            .borrow_mut()
            .entry(element.key.clone())
            .or_default()
            .clone()
    }

    pub(crate) fn calendar_state(&self, element: &Element) -> Rc<calendar::State> {
        self.0
            .calendars
            .borrow_mut()
            .entry(element.key.clone())
            .or_insert_with(|| {
                Rc::new(match element.visual {
                    Visual::Builtin("MonthGrid") => calendar::State::Cells(Default::default()),
                    Visual::Builtin("Tumbler") => calendar::State::Tumbler(Default::default()),
                    _ => unreachable!("calendar control"),
                })
            })
            .clone()
    }

    pub(crate) fn picker_state(&self, element: &Element) -> Rc<picker::State> {
        self.0
            .pickers
            .borrow_mut()
            .entry(element.key.clone())
            .or_insert_with(|| Rc::new(picker::State::new(element)))
            .clone()
    }

    pub(crate) fn render_calendar_cell(
        &self,
        template: &Element,
        cell: xrack_runtime::CalendarCell,
    ) -> Result<Node> {
        let previous = self.0.calendar_input.replace(Some(cell));
        let result = self.render_element(template);
        self.0.calendar_input.set(previous);
        result
    }

    pub(crate) fn render_element(&self, element: &Element) -> Result<Node> {
        self.render_element_with_content(element, true)
    }

    fn render_element_with_content(
        &self,
        element: &Element,
        include_children: bool,
    ) -> Result<Node> {
        if include_children && let Some(cache) = &element.static_subtree {
            return cache
                .get_or_init(|| {
                    self.render_element_or_repeat(element, true)
                        .map(Node::into_shared)
                })
                .clone();
        }
        self.render_element_or_repeat(element, include_children)
    }

    fn render_element_or_repeat(&self, element: &Element, include_children: bool) -> Result<Node> {
        if let Some(repeat) = &element.repeat
            && !self.in_row(element)
        {
            if repeat.empty {
                return Ok(Node::hidden());
            }
            if let Some(count) = repeat.static_count {
                let template = self.render_element_body(element, include_children)?;
                return Ok(xrack_runtime::static_repeat_scene(
                    template,
                    count,
                    repeat.positioned,
                ));
            }
            let children = self.repeat_state(element).render(self, element, 0..4096)?;
            if children.is_empty() {
                return Ok(Node::hidden());
            }
            let kind = if repeat.positioned {
                "Rectangle"
            } else {
                "VerticalLayout"
            };
            let mut node = Node::new(kind, None);
            node.children = children;
            return Ok(node);
        }
        self.render_element_body(element, include_children)
    }

    fn render_element_body(&self, element: &Element, include_children: bool) -> Result<Node> {
        let picker_value = picker::property(element);
        let hidden = element.conditions.iter().try_fold(
            element.static_hidden,
            |hidden, guard| -> Result<bool> {
                Ok(hidden || self.evaluate(guard, &[])? == Value::Bool(false))
            },
        )?;
        if hidden {
            if picker_value.is_some()
                && let Some(state) = self.0.pickers.borrow().get(&element.key)
            {
                state.close();
            }
            return Ok(Node::hidden());
        }
        let mut node = match element.visual {
            Visual::Builtin("StandardTableView") => Node::new("TableView", None),
            Visual::Builtin("SplitHandle" | "DayCell") => Node::new("Rectangle", None),
            Visual::Builtin(_) if picker_value.is_some() => Node::new("Dialog", None),
            Visual::Builtin("StandardButton") => Node::new("Button", None),
            Visual::Builtin("ApplicationWindow") => {
                let mut node = Node::new("Window", None);
                node.application_window = true;
                node
            }
            Visual::Builtin(kind) => Node::new(kind, None),
            Visual::Instance { .. } => self.children()[&element.key]
                .render_scene()?
                .as_ref()
                .clone(),
        };
        node.set_owned_id((!element.id.is_empty()).then(|| element.id.clone()));
        self.populate_layout_handles(element, &mut node)?;
        match element.visual {
            Visual::Builtin("TouchArea") => {
                node.pointer_handle = Some(self.pointer_handle(element));
                node.scroll_event_handler = element.handlers.contains_key("scroll-event");
                node.press_and_hold_handler = element.handlers.contains_key("press-and-hold");
            }
            Visual::Builtin("HoverHandler") => node.hover_handle = Some(self.hover_handle(element)),
            Visual::Builtin("SwipeGestureHandler") => {
                node.swipe_handle = Some(self.0.gestures.swipe(&element.key));
            }
            Visual::Builtin("ScaleRotateGestureHandler") => {
                node.scale_rotate_handle = Some(self.0.gestures.scale_rotate(&element.key));
            }
            _ => {}
        }
        if let Some(handle) = self.0.focus.get(&element.key) {
            node.focus_handle = Some(handle.clone());
        }
        if let Some(target) = &element.initial_focus {
            node.initial_focus = Some(self.focus_handle(target)?);
        }
        if matches!(element.visual, Visual::Builtin("FocusScope")) {
            node.key_handlers = xrack_runtime::KeyPhase::ALL
                .iter()
                .filter(|phase| element.handlers.contains_key(phase.handler()))
                .fold(0, |mask, phase| mask | phase.mask());
        }
        if element.delegate {
            node.properties.insert("__item-delegate", Value::Bool(true));
        }
        let action_enabled = element
            .action
            .as_deref()
            .map(|action| self.present_action(action, &mut node))
            .transpose()?;
        let mut state_values = BTreeMap::new();
        for (&name, cases) in &element.state_bindings {
            if let Some(value) = self.state_value(cases)? {
                state_values.insert(name, scene_value(value));
            }
        }
        for binding in &element.bindings {
            let name = binding.name;
            if state_values.contains_key(name)
                || picker_value == Some(name)
                || (node.kind == "Tumbler" && name == "model")
            {
                continue;
            }
            let member = Member::new(&element.key, name);
            let initial = || self.evaluate(&binding.expression, &[]);
            let value = if let Some(input) = self.0.incoming.get(&member) {
                input.get(initial)
            } else if let Some(slot) = self.0.controls.get(&member) {
                slot.get(|| initial().map(Some))?.map_or_else(initial, Ok)
            } else {
                initial()
            }?;
            node.properties.insert(name, scene_value(value));
            if binding.paint_only {
                node.mark_paint_only(name);
            }
            if let Some(path) = &binding.vector_path {
                node.set_loaded_vector_path(path.clone());
            }
        }
        node.properties.extend(state_values);
        for (&name, target) in &element.links {
            if picker_value != Some(name) {
                node.properties
                    .insert(name, scene_value(self.read(target)?));
            }
        }
        for (&name, field) in &element.row_links {
            let value = self
                .read_row_field(field)?
                .ok_or_else(|| evaluation("row field no longer exists"))?;
            node.properties.insert(name, scene_value(value));
            if !element.delegate {
                let flag = if name == "text" {
                    "editable"
                } else {
                    "table-editor"
                };
                node.properties.insert(flag, true.into());
            }
        }
        for control in &element.controls {
            let name = control.name;
            if picker_value == Some(name) || node.properties.contains_key(name) {
                continue;
            }
            let value = self.control_value(element, name)?;
            let value = if value.is_none() && !control.omit_default {
                Some(self.default_control_value(element, name)?)
            } else {
                value
            };
            if let Some(value) = value {
                node.properties.insert(name, scene_value(value));
            }
        }
        if let Some(enabled) = action_enabled {
            let enabled = enabled && node.boolean("enabled", true);
            node.properties.insert("enabled", enabled.into());
        }
        if matches!(element.visual, Visual::Builtin("StandardButton")) {
            node.properties
                .entry("kind")
                .or_insert(xrack_runtime::StandardButtonKind::Ok.into());
        }
        if matches!(element.visual, Visual::Builtin("AnimatedImage")) {
            let state = Rc::downgrade(&self.0);
            let key = element.key.clone();
            self.playback_slot(&key).populate(&mut node, move || {
                let Some(state) = state.upgrade() else {
                    return false;
                };
                let owner = Instance(state);
                let Some(element) = owner.definition().root.find_key(&key) else {
                    return false;
                };
                owner.handle_result(owner.handler(element, "finished", &[]))
            });
        }
        if matches!(element.visual, Visual::Builtin("Tumbler")) {
            node.tumbler_items = self.tumbler_items(element)?;
        }
        if let Some(combo) = &element.record_combo {
            self.render_combo(element, combo, &mut node)?;
        }
        for &(name, ref spec) in &element.animations {
            if let Some(animation) = self.animation(spec)? {
                node.animations.insert(name, animation);
            }
        }
        for (&name, specs) in &element.state_timings {
            let Value::Int(case) = self.evaluate(&specs[0].selector, &[])? else {
                return Err(evaluation("invalid state selector"));
            };
            let mut timing = StateTiming {
                case,
                enter: None,
                exit: None,
            };
            for spec in specs.iter().filter(|spec| spec.index == case) {
                if spec.enter {
                    timing.enter = self.animation(&spec.animation)?;
                } else {
                    timing.exit = self.animation(&spec.animation)?;
                }
            }
            node.state_timings
                .get_or_insert_with(Default::default)
                .insert(name, timing);
        }
        for timer in &element.timers {
            node.timers.extend(self.timer_registration(timer)?);
        }
        for dialog in &element.native_dialogs {
            node.native_dialogs
                .extend(self.native_dialog_registration(dialog)?);
        }
        if element.is_list_view() {
            self.render_view(element, &mut node)?;
            return Ok(node);
        }
        if picker_value.is_some() {
            node.properties.entry("open").or_insert(Value::Bool(false));
            self.populate_picker(element, &mut node)?;
            return Ok(node);
        }
        if matches!(element.visual, Visual::Builtin("TimePicker")) {
            let time = node.time("time").unwrap_or_default();
            xrack_runtime::TimePickerOptions::from_node(&node).populate(&mut node, time);
            return Ok(node);
        }
        if xrack_runtime::is_window_overlay(node.kind) {
            node.properties.entry("open").or_insert(Value::Bool(false));
            if !node.is_visible() {
                return Ok(node);
            }
        }
        if node.kind == "TableRow" {
            let columns = self
                .0
                .table_columns
                .get_or_init(Default::default)
                .get(Default::default);
            node.children = columns
                .columns(element.children.len())
                .map(|index| {
                    let cell = &element.children[index];
                    let mut rendered = self.render_element(cell)?;
                    if cell.contains_delegate && rendered.is_visible() {
                        rendered
                            .properties
                            .insert("table-delegate-cell", true.into());
                    }
                    Ok(rendered)
                })
                .collect::<Result<_>>()?;
            node.table_column_window = columns;
            return Ok(node);
        }
        if matches!(element.visual, Visual::Builtin("StackPage")) {
            node.properties
                .insert("__stack-route", element.id.to_string().into());
        }
        if element.stack_routes.is_some() {
            node.stack = Some(self.stack_snapshot_for(element)?);
        }
        let eligible_page = !matches!(node.kind, "Tab" | "SwipePage" | "StackPage")
            || node.boolean("visible", true) && node.boolean("enabled", true);
        if include_children && eligible_page && !element.children.is_empty() {
            node.children.clear();
            self.render_children(element, &mut node)?;
        } else if matches!(element.visual, Visual::Builtin("RangeSlider")) {
            node.add_range_thumbs();
        }
        if let Some(split) = &element.split {
            self.populate_split(element, split, &mut node)?;
        }
        if let Some(template) = &element.day_cell
            && node.is_visible()
        {
            self.populate_calendar_cells(element, template, &mut node)?;
        }
        if include_children {
            if let Some(grid) = &element.grid_plan {
                node.set_shared_grid(grid.clone());
            }
            if let Some(order) = &element.paint_plan {
                node.set_shared_paint_order(order.clone());
            }
        }
        Ok(node)
    }

    fn render_children(&self, element: &Element, node: &mut Node) -> Result<()> {
        if matches!(element.visual, Visual::Builtin("RangeSlider")) {
            node.add_range_thumbs();
        }
        let active = match node.kind {
            "StackView" if node.is_visible() => node
                .stack
                .as_ref()
                .and_then(|stack| stack.current())
                .and_then(|route| {
                    element
                        .children
                        .iter()
                        .position(|page| page.id.as_ref() == route)
                }),
            "TabWidget" => usize::try_from(node.int("current-index").unwrap_or(0)).ok(),
            "SwipeView" => {
                let available = element
                    .children
                    .iter()
                    .map(|page| {
                        Ok(
                            self.binding_value(page, "visible")? != Some(Value::Bool(false))
                                && self.binding_value(page, "enabled")? != Some(Value::Bool(false)),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                xrack_runtime::resolve_swipe_index(node.int("current-index"), available)
                    .filter(|_| node.is_visible())
            }
            _ => None,
        };
        for (index, child) in element.children.iter().enumerate() {
            let page = matches!(node.kind, "TabWidget" | "SwipeView" | "StackView");
            let child_node =
                self.render_element_with_content(child, !page || Some(index) == active)?;
            if matches!(node.kind, "RadioGroup" | "TabBar") && child.repeat.is_some() {
                node.children.extend(child_node.children.iter().cloned());
            } else {
                node.children.push(child_node);
            }
        }
        if node.kind == "RadioGroup" {
            let selected = xrack_runtime::group_index(node);
            for (index, child) in node.children.iter_mut().enumerate() {
                child
                    .properties
                    .insert("checked", (selected == Some(index)).into());
            }
        }
        Ok(())
    }

    fn locate<'a>(
        &self,
        program: &'a Program,
        element: &'a Element,
        scene: &Node,
        path: &[usize],
    ) -> Result<Option<(Self, &'a Element)>> {
        if let Some(repeat) = &element.repeat
            && !self.in_row(element)
        {
            let Some((&index, tail)) = path.split_first() else {
                return Ok(None);
            };
            let Some(node) = scene.child_at(index) else {
                return Ok(None);
            };
            let Some(key) = node.row_key else {
                return Ok(None);
            };
            let Some(row) = self.repeat_state(element).row(self, repeat, index, key)? else {
                return Ok(None);
            };
            return row.locate(program, element, node, tail);
        }
        if let Visual::Instance { component, .. } = &element.visual {
            let Some(child) = self.children().get(&element.key) else {
                return Ok(None);
            };
            return child.locate(program, &program.definition(component)?.root, scene, path);
        }
        let Some((&index, tail)) = path.split_first() else {
            return Ok(Some((self.clone(), element)));
        };
        let Some(node) = scene.child_at(index) else {
            return Ok(None);
        };
        if let Some(list) = scene.list {
            let Some((&0, tail)) = tail.split_first() else {
                return Ok(Some((self.clone(), element)));
            };
            let (Some(template), Some(index), Some(row_key), Some(child)) = (
                element.view_template(),
                list.first_row.checked_add(index),
                node.row_key,
                node.child_at(0),
            ) else {
                return Ok(None);
            };
            let Some(repeat) = &template.repeat else {
                return Ok(None);
            };
            let source = if element.tree.is_some() {
                let Some(tree) = &scene.tree else {
                    return Ok(None);
                };
                let (live, _) = self.tree_projection(element)?;
                tree.event_source_index(&live, index, row_key)
            } else {
                let model = self.view_model(element)?;
                if (element.visible_field.is_some() || element.table.is_some())
                    && scene.standard_list_stamp != model.stamp()
                {
                    return Ok(None);
                }
                model.source_index(index)
            };
            let Some(source) = source else {
                return Ok(None);
            };
            let Some(row) = self
                .repeat_state(template)
                .row(self, repeat, source, row_key)?
            else {
                return Ok(None);
            };
            return row.locate(program, template, child, tail);
        }
        let Some(child) = index
            .checked_sub(element.synthetic_child_count())
            .and_then(|index| element.children.get(index))
        else {
            return Ok(None);
        };
        self.locate(program, child, node, tail)
    }

    pub(crate) fn target<'a>(
        &'a self,
        scene: &Node,
        path: &[usize],
    ) -> Option<(Self, &'a Element)> {
        self.capture_result(self.try_target(scene, path)).flatten()
    }

    pub(crate) fn try_target<'a>(
        &'a self,
        scene: &Node,
        path: &[usize],
    ) -> Result<Option<(Self, &'a Element)>> {
        self.locate(&self.0.program, &self.definition().root, scene, path)
    }

    fn typed_input_result(
        &self,
        owner: &Self,
        element: &Element,
        event: &str,
        value: Value,
    ) -> xrack_runtime::EventResult {
        use xrack_runtime::EventResult;
        let Some(handler) = element.handlers.get(event) else {
            return EventResult::Reject;
        };
        match owner.execute(handler, &[value], None) {
            Ok(Some(Value::EventResult(result))) => result,
            Ok(_) => EventResult::Reject,
            Err(error) => {
                self.0.error.set(Some(error));
                EventResult::Reject
            }
        }
    }

    pub(crate) fn handler(&self, element: &Element, event: &str, args: &[Value]) -> Result<bool> {
        if let Some(function) = element.handlers.get(event) {
            self.execute(function, args, None)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn dispatch_named(&self, id: &str, event: &str) -> Result<bool> {
        if let Some((key, id)) = id.split_once('/') {
            return match self.children().get(key) {
                Some(instance) => instance.dispatch_named(id, event),
                None => Ok(false),
            };
        }
        match self.definition().root.named_dispatch_target(id, event) {
            Some(element) => self.dispatch_element(element, event, None),
            None => Ok(false),
        }
    }

    fn dispatch_element(
        &self,
        element: &Element,
        event: &str,
        node: Option<&Node>,
    ) -> Result<bool> {
        let Visual::Builtin(kind) = element.visual else {
            return Ok(false);
        };
        if event == "clicked" {
            if matches!(kind, "Button" | "ToolButton" | "RoundButton")
                && let Some(action) = &element.action
            {
                return self.activate_action(action);
            }
            match kind {
                "GroupBox" => return self.toggle_group_box(element),
                _ if xrack_runtime::is_toggle(kind) => return self.dispatch_toggle(element, node),
                _ => {}
            }
        }
        if xrack_runtime::supports_untyped_dispatch(kind, event) {
            return self.handler(element, event, &[]);
        }
        Ok(false)
    }

    fn dispatch_toggle(&self, element: &Element, node: Option<&Node>) -> Result<bool> {
        use xrack_runtime::CheckState;

        let check_state = element.has_check_state();
        let name = if check_state {
            "check-state"
        } else {
            "checked"
        };
        let row = node.filter(|_| element.row_links.contains_key(name));
        let current = match row {
            Some(node) => node.check_state(),
            None if check_state => match self.control_value(element, name)? {
                Some(Value::CheckState(state)) => state,
                _ => CheckState::Unchecked,
            },
            None => CheckState::from(self.control_value(element, name)? == Some(Value::Bool(true))),
        };
        let tristate = check_state
            && match row {
                Some(node) => node.boolean("tristate", false),
                None => self.binding_value(element, "tristate")? == Some(Value::Bool(true)),
            };
        let state = current.next(tristate);
        let value = if check_state {
            Value::CheckState(state)
        } else {
            Value::Bool(state == CheckState::Checked)
        };
        let projected = check_state && element.bindings.iter().any(|binding| binding.name == name);
        if !projected {
            let updated = match node {
                Some(node) => self.update_control(element, node, name, value)?,
                None => self.set_control_value(element, name, value)?,
            };
            if !updated {
                return Ok(false);
            }
        }
        self.handler(element, "toggled", &[])?;
        self.handler(element, "check-state-changed", &[Value::CheckState(state)])?;
        Ok(true)
    }

    fn overlay_owner(&self, key: &str) -> Result<(Self, &Element)> {
        let element = self
            .definition()
            .root
            .find_key(key)
            .ok_or_else(|| evaluation("unknown overlay"))?;
        let member = Member::new(key, "open");
        let mut owner = self.clone();
        while !owner.0.controls.contains_key(&member) {
            owner = owner
                .row_owner()
                .ok_or_else(|| evaluation("unmounted overlay target"))?;
        }
        Ok((owner, element))
    }

    fn popup_open(&self, key: &str) -> Result<bool> {
        let (owner, element) = self.overlay_owner(key)?;
        Ok(owner.control_value(element, "open")? == Some(Value::Bool(true)))
    }

    fn set_overlay(&self, key: &str, open: bool) -> Result<bool> {
        let (owner, element) = self.overlay_owner(key)?;
        let current = owner.control_value(element, "open")? == Some(Value::Bool(true));
        if current == open {
            return Ok(false);
        }
        owner.set_control_value(element, "open", open.into())?;
        owner.handler(element, if open { "opened" } else { "closed" }, &[])?;
        Ok(true)
    }

    fn scroll_control_read(&self, member: &Member) -> Result<Value> {
        use xrack_runtime::{Orientation, ScrollBarRange};
        let element = self
            .definition()
            .root
            .find_key(&member.owner)
            .ok_or_else(|| evaluation("unknown scroll control"))?;
        let size = member.name == "size";
        if let Some(target) = &element.scroll_target {
            let handle = self.viewport(target)?;
            let axis = match self.binding_value(element, "orientation")? {
                Some(Value::Orientation(axis)) => axis,
                _ => Orientation::Vertical,
            };
            return Ok(Value::Float(if size {
                handle.scroll_size(axis)
            } else {
                handle.scroll_position(axis)
            }));
        }
        let size_binding = match self.binding_value(element, "size")? {
            Some(Value::Float(size)) => size,
            _ => ScrollBarRange::DEFAULT_SIZE,
        };
        let position = if size {
            0.0
        } else {
            match self.control_value(element, "position")? {
                Some(Value::Float(position)) => position,
                _ => 0.0,
            }
        };
        let range = ScrollBarRange::new(position, size_binding, 0.0);
        Ok(Value::Float(if size { range.size } else { range.position }))
    }

    pub(crate) fn binding_value(&self, element: &Element, name: &str) -> Result<Option<Value>> {
        element
            .bindings
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| self.evaluate(&binding.expression, &[]))
            .transpose()
    }

    pub(crate) fn control_value(&self, element: &Element, name: &str) -> Result<Option<Value>> {
        if let Some(target) = element.links.get(name) {
            return self.read(target).map(Some);
        }
        if let Some(field) = element.row_links.get(name) {
            return self.read_row_field(field);
        }
        let member = Member::new(&element.key, name);
        if let Some(input) = self.0.incoming.get(&member) {
            return input
                .get(|| {
                    self.binding_value(element, name)?
                        .map_or_else(|| self.default_control_value(element, name), Ok)
                })
                .map(Some);
        }
        let initial = || self.binding_value(element, name);
        match self.0.controls.get(&member) {
            Some(slot) => slot.get(initial),
            None => initial(),
        }
    }

    pub(crate) fn default_control_value(&self, element: &Element, name: &str) -> Result<Value> {
        use xrack_resources::visual::ControlDefault;

        let control = element.control(name).ok_or_else(|| {
            evaluation(format!(
                "unsupported control storage {}.{name}",
                element.key
            ))
        })?;
        match &control.default {
            ControlDefault::Value(value) => Ok(value.clone()),
            ControlDefault::Binding(property, fallback) => Ok(self
                .binding_value(element, property)?
                .unwrap_or_else(|| fallback.clone())),
            ControlDefault::Control(property) => self
                .control_value(element, property)?
                .map_or_else(|| self.default_control_value(element, property), Ok),
        }
    }

    fn select_source(&self, target: &str, source: i64, column: Option<i64>) -> Result<()> {
        let Ok(source) = usize::try_from(source) else {
            return Ok(());
        };
        let (owner, view) = self.element_owner(target)?;
        if view.tree.is_some() {
            return owner.select_tree_source(view, source);
        }
        let Some(index) = owner.view_model(view)?.visual_index(source) else {
            return Ok(());
        };
        if view.table.is_some() {
            return owner.select_table_source(view, index, column);
        }
        let value = Value::Int(index as i64);
        let current = owner
            .control_value(view, "current-index")?
            .unwrap_or(Value::Int(-1));
        if current != value && owner.set_control_value(view, "current-index", value.clone())? {
            owner.handler(view, "changed", &[value])?;
        }
        Ok(())
    }

    pub(crate) fn update_control(
        &self,
        element: &Element,
        node: &Node,
        name: &str,
        value: Value,
    ) -> Result<bool> {
        if let Some(field) = element.row_links.get(name) {
            return self.write_row_field(field, node.properties.get(name), value);
        }
        self.set_control_value(element, name, value)
    }

    pub(crate) fn set_control_value(
        &self,
        element: &Element,
        name: &str,
        value: Value,
    ) -> Result<bool> {
        if let Some(target) = element.links.get(name) {
            self.write(target, value)?;
            return Ok(true);
        }
        let member = Member::new(&element.key, name);
        if let Some(input) = self.0.incoming.get(&member) {
            input.set_if_changed(Ok(model_value(value)));
            return Ok(true);
        }
        let Some(slot) = self.0.controls.get(&member) else {
            return Ok(false);
        };
        slot.set_if_changed(Ok(Some(value)));
        Ok(true)
    }

    fn combo_select(
        &self,
        element: &Element,
        node: &Node,
        index: usize,
        accept: bool,
    ) -> Result<bool> {
        if !xrack_runtime::choice_enabled(node, index) {
            return Ok(false);
        }
        let Some(label) = xrack_runtime::combo_label(node, index) else {
            return Ok(false);
        };
        let label = Value::String(label.into_owned());
        let by_value = element.selection_property() == "current-value";
        let chosen = if let Some(combo) = &element.record_combo {
            let model = self.record_combo_model(combo)?;
            if !xrack_runtime::combo_model_is_current(node, model.model()) {
                return Ok(false);
            }
            let Some(value) = xrack_runtime::combo_value(node, index) else {
                return Ok(false);
            };
            value
        } else {
            label.clone()
        };
        let value = if by_value {
            chosen.clone()
        } else {
            Value::Int(index as i64)
        };
        self.update_control(element, node, element.selection_property(), value)?;
        if xrack_runtime::combo_editable(node)
            && let Some(control) = element.control("edit-text")
        {
            if control.omit_default {
                self.0.controls[&Member::new(&element.key, control.name)].reset();
            } else if !accept {
                self.update_control(element, node, "edit-text", label.clone())?;
            }
        }
        if !accept {
            self.handler(element, "selected", &[label])?;
        }
        if element.record_combo.is_some() {
            self.handler(element, "value-selected", &[chosen])?;
        }
        Ok(true)
    }

    fn accept_combo(&self, element: &Element, node: &Node) -> Result<bool> {
        if !xrack_runtime::combo_editable(node) {
            return Ok(false);
        }
        let draft = xrack_runtime::text_input_value(node).to_owned();
        let matching = xrack_runtime::combo_index(node)
            .filter(|&index| {
                xrack_runtime::choice_enabled(node, index)
                    && xrack_runtime::combo_text(node) == draft
            })
            .or_else(|| xrack_runtime::combo_find_enabled_label(node, 0, |label| label == draft));
        if let Some(index) = matching {
            if !self.combo_select(element, node, index, true)? {
                return Ok(false);
            }
        } else if let Some(combo) = &element.record_combo {
            let model = self.record_combo_model(combo)?;
            if !xrack_runtime::combo_model_is_current(node, model.model())
                || xrack_runtime::combo_find_label(node, |label| label == draft).is_some()
            {
                return Ok(false);
            }
            if element.selection_property() == "current-index" {
                self.update_control(element, node, "current-index", Value::Int(-1))?;
            }
        } else {
            let value = if element.selection_property() == "current-value" {
                Value::String(draft.clone())
            } else {
                Value::Int(-1)
            };
            self.update_control(element, node, element.selection_property(), value)?;
        }
        self.handler(element, "accepted", &[Value::String(draft)])?;
        Ok(true)
    }

    /// Retain errors for component methods that cannot return a Result.
    pub(crate) fn capture_result<T>(&self, result: Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.0.error.set(Some(error));
                None
            }
        }
    }

    pub(crate) fn handle_result(&self, result: Result<bool>) -> bool {
        self.capture_result(result).unwrap_or(false)
    }
}

impl Element {
    pub(crate) fn synthetic_child_count(&self) -> usize {
        if matches!(self.visual, Visual::Builtin("RangeSlider")) {
            Node::RANGE_THUMBS
        } else {
            0
        }
    }

    fn named_dispatch_target(&self, id: &str, event: &str) -> Option<&Self> {
        let Visual::Builtin(kind) = self.visual else {
            return None;
        };
        if self.id.as_ref() == id
            && (!id.is_empty()
                || xrack_runtime::supports_untyped_dispatch(kind, event)
                    && self.handlers.contains_key(event))
        {
            return Some(self);
        }
        self.children
            .iter()
            .filter(|child| child.repeat.is_none())
            .find_map(|child| child.named_dispatch_target(id, event))
    }
}

impl Component for Instance {
    fn set_locale_context(&self, context: xrack_runtime::LocaleContext) {
        self.0.locale.set_context(context);
    }

    fn render(&self) -> Node {
        self.render_shared().as_ref().clone()
    }

    fn render_shared(&self) -> Rc<Node> {
        self.capture_result(self.try_render())
            .unwrap_or_else(|| Rc::new(Node::hidden()))
    }

    fn scene_dirty(&self) -> bool {
        self.0.scene.is_dirty()
    }

    fn close_requested(&mut self) -> bool {
        let Some(handler) = self.definition().root.handlers.get("close-requested") else {
            return true;
        };
        let result = self
            .execute(handler, &[], None)
            .and_then(|value| match value {
                Some(Value::Bool(allow)) => Ok(allow),
                _ => Err(evaluation("close-requested must return a boolean")),
            });
        self.handle_result(result)
    }

    fn take_window_close_request(&mut self) -> bool {
        self.0.window.take_close_request()
    }

    fn take_window_commands(&mut self) -> Vec<xrack_runtime::WindowCommand> {
        self.0.window.take_commands()
    }

    fn has_window_commands(&self) -> bool {
        self.0.window.has_commands()
    }

    fn secondary_windows(&self) -> Option<Rc<[xrack_runtime::SecondaryWindowRegistration]>> {
        self.capture_result(self.secondary_window_registrations())
            .flatten()
    }

    fn publish_window_status(&self, status: xrack_runtime::WindowStatus) {
        self.0.window.publish(status);
    }

    fn list_rows(&self, path: &[usize], range: std::ops::Range<usize>) -> Vec<Node> {
        let Some(scene) = self.capture_result(self.try_render()) else {
            return Vec::new();
        };
        let Some((owner, element)) = self.target(&scene, path).filter(|(_, e)| e.is_list_view())
        else {
            return Vec::new();
        };
        self.capture_result(owner.view_rows(
            element,
            range,
            &xrack_runtime::TableColumnWindow::default(),
        ))
        .unwrap_or_default()
    }

    fn dispatch(&mut self, id: &str, event: &str) -> bool {
        self.handle_result(self.dispatch_named(id, event))
    }

    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        let Some(node) = scene.dispatch_target(path, event) else {
            return false;
        };
        let Some((instance, element)) = self.target(scene, path) else {
            return false;
        };
        self.handle_result(instance.dispatch_element(element, event, Some(node)))
    }

    fn focus_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: xrack_runtime::FocusEvent,
    ) -> bool {
        use xrack_runtime::FocusEvent;
        let node = if event.is_loss() {
            scene.at_path(path)
        } else {
            scene.interactive_at_path(path)
        };
        if !node.is_some_and(|node| node.kind == "FocusScope") {
            return false;
        }
        let Some((instance, element)) = self.target(scene, path) else {
            return false;
        };
        self.handle_result((|| {
            let (name, reason) = match event {
                FocusEvent::Gained(reason) => ("focus-gained", reason),
                FocusEvent::Lost(reason) => ("focus-lost", reason),
                FocusEvent::Within { focused, reason } => {
                    return instance.handler(
                        element,
                        "focus-within-changed",
                        &[Value::Bool(focused), Value::FocusReason(reason)],
                    );
                }
            };
            let args = &[Value::FocusReason(reason)];
            let handled = instance.handler(element, name, args)?;
            Ok(instance.handler(element, "focus-changed-event", args)? || handled)
        })())
    }

    fn key_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        phase: xrack_runtime::KeyPhase,
        event: &xrack_runtime::KeyEvent,
    ) -> xrack_runtime::EventResult {
        use xrack_runtime::EventResult;
        if !scene
            .interactive_at_path(path)
            .is_some_and(|node| node.kind == "FocusScope" && node.key_handlers & phase.mask() != 0)
        {
            return EventResult::Reject;
        }
        let Some((instance, element)) = self.target(scene, path) else {
            return EventResult::Reject;
        };
        self.typed_input_result(
            &instance,
            element,
            phase.handler(),
            Value::KeyEvent(event.clone()),
        )
    }

    fn pointer_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: &xrack_runtime::PointerEvent,
    ) -> bool {
        let node = if event.kind == xrack_runtime::PointerEventKind::Cancel {
            scene.at_path(path)
        } else {
            scene.interactive_at_path(path)
        };
        if !node.is_some_and(|node| node.pointer_handle.is_some()) {
            return false;
        }
        let Some((instance, element)) = self.target(scene, path) else {
            return false;
        };
        self.handle_result(instance.handler(
            element,
            "pointer-event",
            &[Value::PointerEvent(*event)],
        ))
    }

    fn drag_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: &xrack_runtime::DragEvent,
    ) -> bool {
        let node = if event.is_terminal() {
            scene.at_path(path)
        } else {
            scene.interactive_at_path(path)
        };
        if !node.is_some_and(|node| node.kind == event.target_kind()) {
            return false;
        }
        let Some((instance, element)) = self.target(scene, path) else {
            return false;
        };
        use xrack_runtime::DragEvent;
        let (name, args) = match event {
            DragEvent::Started => ("drag-started", vec![]),
            DragEvent::Finished(action) => ("drag-finished", vec![(*action).into()]),
            DragEvent::Entered { data, action } => {
                ("entered", vec![data.clone().into(), (*action).into()])
            }
            DragEvent::Dropped { data, action } => {
                ("dropped", vec![data.clone().into(), (*action).into()])
            }
            DragEvent::FilesEntered(paths) => (
                "files-entered",
                vec![model_value(Value::StringList(paths.to_vec()))],
            ),
            DragEvent::FilesDropped(paths) => (
                "files-dropped",
                vec![model_value(Value::StringList(paths.to_vec()))],
            ),
            DragEvent::Moved { x, y } => ("position-changed", vec![(*x).into(), (*y).into()]),
            DragEvent::Exited => ("exited", vec![]),
        };
        self.handle_result(instance.handler(element, name, &args).map(|_| true))
    }

    fn wheel_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: &xrack_runtime::WheelEvent,
    ) -> xrack_runtime::EventResult {
        use xrack_runtime::EventResult;
        let Some(node) = scene
            .interactive_at_path(path)
            .filter(|node| node.handles_wheel())
        else {
            return EventResult::Reject;
        };
        let Some((instance, element)) = self.target(scene, path) else {
            return EventResult::Reject;
        };
        let event_name = if node.kind == "TouchArea" {
            "scroll-event"
        } else {
            "wheel"
        };
        self.typed_input_result(&instance, element, event_name, Value::WheelEvent(*event))
    }

    fn resize_split_at(&mut self, scene: &Node, path: &[usize], weights: &[(usize, f32)]) -> bool {
        self.handle_result(self.resize_split(scene, path, weights, false))
    }

    fn resize_split_sizes_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        sizes: &[(usize, f32)],
    ) -> bool {
        self.handle_result(self.resize_split(scene, path, sizes, true))
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        let Some(node) = scene.control_target(path, action) else {
            return false;
        };
        let Some(target) = self.capture_result(self.try_target(scene, path)) else {
            return false;
        };
        let Some((instance, element)) = target else {
            return self.handle_result(self.picker_child_input(
                scene,
                path,
                node,
                picker::Input::Control(action),
            ));
        };
        if picker::property(element).is_some() {
            return self.handle_result(instance.picker_control(element, node, action));
        }
        if let ControlAction::Calendar(action) = action
            && xrack_runtime::is_calendar_control(node.kind)
        {
            return self.handle_result((|| {
                if node.kind == "MonthGrid" {
                    let current = self.render_scene()?;
                    let valid = |live: &Node| {
                        xrack_runtime::CalendarState::from_node(live)
                            == xrack_runtime::CalendarState::from_node(node)
                            && live.int("week-start") == node.int("week-start")
                    };
                    if let Some(live) = current.interactive_at_path(path) {
                        if !valid(live) {
                            return Ok(false);
                        }
                    } else if scene.row_key_at(path).is_none()
                        || !valid(&instance.render_element(element)?)
                    {
                        return Ok(false);
                    }
                }
                instance.calendar_action(element, node, action)
            })());
        }
        if element.tree.is_some() {
            return self.handle_result(instance.tree_control(element, node, action));
        }
        if element.table.is_some() {
            return self.handle_result(instance.table_control(element, node, action));
        }
        if let Some(command) = &element.action {
            let expected = match node.kind {
                "MenuItem" => ControlAction::ActivateMenuItem,
                "Shortcut" => ControlAction::ActivateShortcut,
                _ => ControlAction::ActivateAction,
            };
            return action == expected && self.handle_result(instance.activate_action(command));
        }
        self.handle_result(match action {
            ControlAction::StackBack { revision }
                if node.kind == "StackView"
                    && node
                        .stack
                        .as_ref()
                        .is_some_and(|stack| stack.revision() == revision) =>
            {
                instance.navigate_stack_view(
                    element,
                    Some(revision),
                    xrack_runtime::StackNavigation::Pop,
                )
            }
            ControlAction::SwipeDelegate(side) if node.kind == "SwipeDelegate" => (|| {
                let Value::SwipeSide(current) = instance
                    .control_value(element, "open-side")?
                    .unwrap_or(Value::SwipeSide(xrack_runtime::SwipeSide::Closed))
                else {
                    return Ok(false);
                };
                if !node.accepts_swipe_side(current, side) {
                    return Ok(false);
                }
                let next = Value::SwipeSide(side);
                if !instance.update_control(element, node, "open-side", next.clone())? {
                    return Ok(false);
                }
                instance.handler(element, "changed", &[next])?;
                Ok(true)
            })(),
            ControlAction::ActivateRadioOption if node.kind == "RadioButton" => {
                instance.activate_radio_option(element, node)
            }
            ControlAction::Select(index)
                if matches!(
                    node.kind,
                    "RadioGroup" | "TabWidget" | "TabBar" | "SwipeView"
                ) =>
            {
                instance.select_group(element, node, index)
            }
            ControlAction::Select(index) if node.kind == "PageIndicator" => {
                instance.select_page_indicator(element, index)
            }
            ControlAction::ActivateMenuItem if node.kind == "MenuItem" => {
                instance.activate_menu_item(element, node)
            }
            ControlAction::Dialog(_) | ControlAction::DialogCommand(_) if node.kind == "Dialog" => {
                instance.dialog_action(element, action)
            }
            ControlAction::ActivateShortcut | ControlAction::AmbiguousShortcut
                if node.kind == "Shortcut" =>
            {
                let event = if action == ControlAction::ActivateShortcut {
                    "activated"
                } else {
                    "activated-ambiguously"
                };
                instance.handler(element, event, &[]).map(|_| true)
            }
            ControlAction::Select(index) if element.is_list_view() => (|| {
                let model = instance.view_model(element)?;
                let value = Value::Int(index as i64);
                if !xrack_runtime::ListMetrics::has_row_in_model(node, index, model.stamp())
                    || instance.control_value(element, "current-index")?.as_ref() == Some(&value)
                    || !instance.update_control(element, node, "current-index", value.clone())?
                {
                    return Ok(false);
                }
                instance.handler(element, "changed", &[value])?;
                Ok(true)
            })(),
            ControlAction::Select(index) if node.kind == "ComboBox" => {
                instance.combo_select(element, node, index, false)
            }
            ControlAction::DelayButton(checked) if node.kind == "DelayButton" => (|| {
                let current =
                    instance.control_value(element, "checked")? == Some(Value::Bool(true));
                if current == checked
                    || !instance.update_control(element, node, "checked", checked.into())?
                {
                    return Ok(false);
                }
                if checked {
                    instance.handler(element, "activated", &[])?;
                }
                Ok(true)
            })(),
            ControlAction::Popup(open) | ControlAction::Drawer(open)
                if matches!(
                    (node.kind, action),
                    ("PopupWindow", ControlAction::Popup(_)) | ("Drawer", ControlAction::Drawer(_))
                ) =>
            {
                instance.set_overlay(&element.key, open)
            }
            ControlAction::SubmitSearch if node.kind == "SearchField" => (|| {
                let text = Value::String(xrack_runtime::text_input_value(node).to_owned());
                instance.handler(element, "accepted", std::slice::from_ref(&text))?;
                instance.handler(element, "search-triggered", &[text])?;
                Ok(true)
            })(),
            ControlAction::Select(index) if node.kind == "SearchField" => (|| {
                let Some(suggestion) = xrack_runtime::search_suggestion(node, index) else {
                    return Ok(false);
                };
                let value = Value::String(suggestion.into());
                let args = std::slice::from_ref(&value);
                if suggestion != xrack_runtime::text_input_value(node) {
                    if !instance.update_control(element, node, "text", value.clone())? {
                        return Ok(false);
                    }
                    instance.handler(element, "edited", args)?;
                }
                instance.handler(element, "selected", args)?;
                instance.handler(element, "search-triggered", args)?;
                Ok(true)
            })(),
            ControlAction::ClearSearch
                if node.kind == "SearchField"
                    && !node.boolean("read-only", false)
                    && !xrack_runtime::text_input_value(node).is_empty() =>
            {
                instance.edit_control(element, node, String::new())
            }
            ControlAction::AcceptComboText if node.kind == "ComboBox" => {
                instance.accept_combo(element, node)
            }
            ControlAction::PickTumbler { .. } if node.kind == "Tumbler" => {
                instance.tumbler_action(element, node, action)
            }
            ControlAction::Spin(action) if xrack_runtime::is_spin_box(node.kind) => {
                instance.spin_action(element, node, action)
            }
            ControlAction::ActivateTextLink(index) if node.kind == "StyledText" => {
                let Some(link) = node
                    .styled_text("text")
                    .and_then(|text| text.links().get(index))
                else {
                    return false;
                };
                instance.handler(
                    element,
                    "link-activated",
                    &[Value::String(link.href.to_string())],
                )
            }
            _ => Ok(false),
        })
    }

    fn adjust_range_at(&mut self, scene: &Node, path: &[usize], action: RangeAction) -> bool {
        let Some(node) = scene.range_target(path) else {
            return false;
        };
        if node.kind == "RangeSlider" {
            let Some((&handle, parent)) = path.split_last() else {
                return false;
            };
            let Some((instance, element)) = self.target(scene, parent) else {
                return false;
            };
            return self.handle_result(instance.interval_action(element, node, handle, action));
        }
        let Some(target) = self.capture_result(self.try_target(scene, path)) else {
            return false;
        };
        let Some((instance, element)) = target else {
            return self.handle_result(self.picker_child_input(
                scene,
                path,
                node,
                picker::Input::Range(action),
            ));
        };
        self.handle_result(match node.kind {
            "Slider" | "Dial" => instance.range_action(element, node, action),
            "ScrollBar" if node.scroll_target.is_none() && node.boolean("interactive", true) => {
                (|| {
                    let Some(value) = xrack_runtime::ScrollBarRange::from_node(node).apply(action)
                    else {
                        return Ok(false);
                    };
                    let value = Value::Float(value);
                    instance.update_control(element, node, "position", value.clone())?;
                    instance.handler(element, "changed", &[value])?;
                    Ok(true)
                })()
            }
            _ => Ok(false),
        })
    }

    fn copy_table_at(&self, scene: &Node, path: &[usize], columns: &[usize]) -> Option<String> {
        let node = scene.interactive_at_path(path)?;
        let (instance, element) = self.target(scene, path)?;
        element.table.as_ref()?;
        self.capture_result(instance.copy_table(element, node, columns))
            .flatten()
    }

    fn prepare_table_paste(
        &self,
        scene: &Node,
        path: &[usize],
        row: usize,
        columns: Vec<usize>,
    ) -> Option<xrack_runtime::TablePaste> {
        self.capture_result(self.capture_table_paste(scene, path, row, columns))
            .flatten()
    }

    fn replay_table_paste_at(&self, scene: &Node, path: &[usize], redo: bool) -> bool {
        let Some(node) = scene.interactive_at_path(path) else {
            return false;
        };
        if !node.table_edit_history_enabled() {
            return false;
        }
        let Some((owner, element)) = self.target(scene, path) else {
            return false;
        };
        self.handle_result(owner.replay_table(element, redo))
    }

    fn next_table_edit_cell(
        &self,
        scene: &Node,
        path: &[usize],
        origin: xrack_runtime::TableEditPosition,
        columns: &[usize],
        backward: bool,
    ) -> Option<xrack_runtime::TableEditPosition> {
        let node = scene.interactive_at_path(path)?;
        let (owner, element) = self.target(scene, path)?;
        element.table.as_ref()?;
        self.capture_result(owner.table_edit_target(element, node, origin, columns, backward))
            .flatten()
    }

    fn commit_table_cell(
        &mut self,
        scene: &Node,
        path: &[usize],
        original: &str,
        text: String,
    ) -> bool {
        let Some(node) = scene
            .interactive_at_path(path)
            .filter(|node| xrack_runtime::editable_table_cell(node))
        else {
            return false;
        };
        if let [table_path @ .., slot, 0, column] = path
            && let Some(table) = scene.at_path(table_path)
            && table.boolean("__standard-table", false)
        {
            let Some((owner, view)) = self.target(scene, table_path) else {
                return false;
            };
            return self.handle_result(
                owner.commit_projected_cell(view, table, *slot, *column, original, &text),
            );
        }
        let Some((owner, element)) = self.target(scene, path) else {
            return false;
        };
        if node.text("text") != Some(original) {
            return false;
        }
        self.handle_result(owner.update_control(element, node, "text", Value::String(text)))
    }

    fn edit_at(&mut self, scene: &Node, path: &[usize], text: String) -> bool {
        let Some(node) = scene.interactive_at_path(path) else {
            return false;
        };
        if !node.accepts_editor_update(&text) {
            return false;
        }
        let Some(target) = self.capture_result(self.try_target(scene, path)) else {
            return false;
        };
        let Some((instance, element)) = target else {
            return self.handle_result(self.picker_child_input(
                scene,
                path,
                node,
                picker::Input::Edit(text),
            ));
        };
        self.handle_result(instance.edit_control(element, node, text))
    }
}

impl Instance {
    fn edit_control(&self, element: &Element, node: &Node, text: String) -> Result<bool> {
        let value = Value::String(text);
        let combo = node.kind == "ComboBox";
        let name = if combo { "edit-text" } else { "text" };
        if !self.update_control(element, node, name, value.clone())? {
            return Ok(false);
        }
        let typed = combo
            || node.kind == "SearchField"
            || xrack_runtime::is_multiline_text_input(node.kind);
        let args = std::slice::from_ref(&value);
        self.handler(element, "edited", if typed { args } else { &[] })?;
        if node.kind == "SearchField"
            && self.binding_value(element, "live")? == Some(Value::Bool(true))
        {
            self.handler(element, "search-triggered", args)?;
        }
        Ok(true)
    }
}

impl ValueType {
    pub(super) fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Record(name), Value::Record(record)) => return name.as_ref() == record.name(),
            (Self::RecordList(name), Value::RecordList(list)) => {
                return name.as_ref() == list.name();
            }
            _ => {}
        }
        matches!(
            (self, value),
            (Self::Bool, Value::Bool(_))
                | (Self::EventResult, Value::EventResult(_))
                | (Self::FocusReason, Value::FocusReason(_))
                | (Self::DialogRole, Value::DialogRole(_))
                | (Self::StandardButtonKind, Value::StandardButtonKind(_))
                | (Self::DrawerEdge, Value::DrawerEdge(_))
                | (Self::FileDialogMode, Value::FileDialogMode(_))
                | (Self::MessageLevel, Value::MessageLevel(_))
                | (Self::MessageButtons, Value::MessageButtons(_))
                | (Self::KeyEvent, Value::KeyEvent(_))
                | (Self::WheelEvent, Value::WheelEvent(_))
                | (Self::PointerEvent, Value::PointerEvent(_))
                | (Self::PointerEventKind, Value::PointerEventKind(_))
                | (Self::PointerButton, Value::PointerButton(_))
                | (Self::PointerButtons, Value::PointerButtons(_))
                | (Self::CursorShape, Value::CursorShape(_))
                | (Self::SwipeSide, Value::SwipeSide(_))
                | (Self::DragAction, Value::DragAction(_))
                | (Self::TouchPoint, Value::TouchPoint(_))
                | (Self::TouchPoints, Value::TouchPoints(_))
                | (Self::KeyModifiers, Value::KeyModifiers(_))
                | (Self::Int, Value::Int(_))
                | (Self::Float, Value::Float(_))
                | (Self::Length, Value::Length(_))
                | (Self::Percent, Value::Percent(_))
                | (Self::Color, Value::Color(_))
                | (Self::Brush, Value::Brush(_))
                | (Self::String, Value::String(_))
                | (
                    Self::StringList,
                    Value::StringList(_) | Value::StringModel(_)
                )
                | (Self::Date, Value::Date(_))
                | (Self::Time, Value::Time(_))
                | (Self::Font, Value::Font(_))
                | (Self::CheckState, Value::CheckState(_))
                | (Self::StyledText, Value::StyledText(_))
                | (Self::LayoutAlignment, Value::LayoutAlignment(_))
                | (Self::Orientation, Value::Orientation(_))
                | (Self::ScrollSnapMode, Value::ScrollSnapMode(_))
                | (Self::ScrollAlignment, Value::ScrollAlignment(_))
                | (Self::SelectionTarget, Value::SelectionTarget(_))
                | (Self::ScrollBarPolicy, Value::ScrollBarPolicy(_))
                | (Self::FlickDirection, Value::FlickDirection(_))
                | (Self::FlickBoundsBehavior, Value::FlickBoundsBehavior(_))
                | (Self::FlickBoundsMovement, Value::FlickBoundsMovement(_))
                | (Self::PopupPlacement, Value::PopupPlacement(_))
                | (
                    Self::FlexboxLayoutDirection,
                    Value::FlexboxLayoutDirection(_)
                )
                | (Self::FlexboxLayoutWrap, Value::FlexboxLayoutWrap(_))
                | (Self::CrossAxisAlignment, Value::CrossAxisAlignment(_))
                | (Self::TextWrap, Value::TextWrap(_))
                | (
                    Self::TextHorizontalAlignment,
                    Value::TextHorizontalAlignment(_)
                )
                | (Self::TextVerticalAlignment, Value::TextVerticalAlignment(_))
                | (Self::TextOverflow, Value::TextOverflow(_))
                | (Self::ColorScheme, Value::ColorScheme(_))
                | (Self::InputType, Value::InputType(_))
                | (Self::DialInputMode, Value::DialInputMode(_))
                | (Self::Image, Value::Image(_) | Value::ImageAsset(_))
                | (Self::ImageFit, Value::ImageFit(_))
                | (Self::ImageTileMode, Value::ImageTileMode(_))
                | (
                    Self::ImageHorizontalAlignment,
                    Value::ImageHorizontalAlignment(_)
                )
                | (
                    Self::ImageVerticalAlignment,
                    Value::ImageVerticalAlignment(_)
                )
                | (Self::ImageRendering, Value::ImageRendering(_))
        )
    }
}

fn image_asset(value: Value) -> Result<ImageAsset> {
    match value {
        Value::Image(image) => Ok(image.into()),
        Value::ImageAsset(image) => Ok(image),
        _ => Err(evaluation("expected an image")),
    }
}

pub(crate) fn model_value(value: Value) -> Value {
    match value {
        Value::StringList(items) => Value::StringModel(xrack_runtime::VecModel::from(items)),
        value => value,
    }
}

pub(crate) fn host_value(value: Value) -> Value {
    match value {
        Value::StringModel(model) => Value::StringList(model.snapshot()),
        value => value,
    }
}

fn scene_value(value: Value) -> Value {
    match value {
        Value::Brush(brush) => Value::from(brush),
        value => host_value(value),
    }
}

fn binary(program: &Program, operator: Binary, lhs: Value, rhs: Value) -> Result<Value> {
    use Binary::*;
    use Value::*;
    if matches!(operator, Equal | NotEqual) {
        let equal = match (&lhs, &rhs) {
            (StringModel(a), StringModel(b)) => a.values_eq(b),
            (TouchPoints(a), TouchPoints(b)) => a.values_eq(b),
            (StringModel(model), StringList(items)) | (StringList(items), StringModel(model)) => {
                model.snapshot() == *items
            }
            (RecordList(a), RecordList(b)) => {
                program.validate_live_record_list(a)?;
                program.validate_live_record_list(b)?;
                a.name() == b.name() && a.model().values_eq(b.model())
            }
            _ => lhs.eq_with_image_assets(&rhs),
        };
        return Ok(Bool(equal == (operator == Equal)));
    }

    let ordered = |ordering: std::cmp::Ordering| match operator {
        Less => Ok(Bool(ordering.is_lt())),
        LessEqual => Ok(Bool(!ordering.is_gt())),
        Greater => Ok(Bool(ordering.is_gt())),
        GreaterEqual => Ok(Bool(!ordering.is_lt())),
        _ => Err(evaluation("invalid ordered operation")),
    };
    macro_rules! numeric {
        ($lhs:expr, $rhs:expr, $wrap:expr) => {
            match operator {
                Add => Ok($wrap($lhs + $rhs)),
                Subtract => Ok($wrap($lhs - $rhs)),
                Multiply => Ok($wrap($lhs * $rhs)),
                Divide => Ok($wrap($lhs / $rhs)),
                Remainder => Ok($wrap($lhs % $rhs)),
                Less => Ok(Bool($lhs < $rhs)),
                LessEqual => Ok(Bool($lhs <= $rhs)),
                Greater => Ok(Bool($lhs > $rhs)),
                GreaterEqual => Ok(Bool($lhs >= $rhs)),
                _ => Err(evaluation("invalid numeric operation")),
            }
        };
    }

    match (lhs, rhs) {
        (Int(a), Int(b)) => match operator {
            Add => Ok(Int(a.wrapping_add(b))),
            Subtract => Ok(Int(a.wrapping_sub(b))),
            Multiply => Ok(Int(a.wrapping_mul(b))),
            Divide => a
                .checked_div(b)
                .map(Int)
                .ok_or_else(|| evaluation("invalid integer division")),
            Remainder => a
                .checked_rem(b)
                .map(Int)
                .ok_or_else(|| evaluation("invalid integer remainder")),
            _ => numeric!(a, b, Int),
        },
        (Float(a), Float(b)) => numeric!(a, b, Float),
        (Length(a), Length(b)) if operator == Divide => Ok(Float(a.0 / b.0)),
        (Length(xr_length), Length(yr_length)) => numeric!(xr_length.0, yr_length.0, |v| Length(
            xrack_runtime::Length(v)
        )),
        (Length(length), Float(factor)) => {
            numeric!(length.0, factor, |v| Length(xrack_runtime::Length(v)))
        }
        (Float(factor), Length(length)) => {
            numeric!(factor, length.0, |v| Length(xrack_runtime::Length(v)))
        }
        (Percent(x), Percent(y)) => numeric!(x.0, y.0, |v| Percent(xrack_runtime::Percent(v))),
        (Date(a), Date(b)) => ordered(a.cmp(&b)),
        (Time(a), Time(b)) => ordered(a.cmp(&b)),
        (Bool(a), Bool(b)) if operator == And => Ok(Bool(a && b)),
        (Bool(a), Bool(b)) if operator == Or => Ok(Bool(a || b)),
        (String(mut a), String(b)) if operator == Add => {
            a.push_str(&b);
            Ok(String(a))
        }
        _ => Err(evaluation("invalid binary operands")),
    }
}
