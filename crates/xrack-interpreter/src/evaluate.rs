use crate::{
    repeat::{Model, RepeatState, RowScope},
    *,
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Weak,
};
use xrack_runtime::{
    Brush, CallbackSlot, Component, ComponentLifetime, ControlAction, DragEvent, EffectfulCallback,
    EventResult, FocusEvent, GradientStop, ImagePlaybackSlot, InputProperty, InputRoute, Length,
    ListModel, Property, RangeAction, RoutedEvent, StateTiming,
};

pub(crate) type Globals = RefCell<BTreeMap<String, Instance>>;

pub(crate) enum GlobalContext {
    Owner(Rc<Globals>),
    Member(Weak<Globals>),
}

impl GlobalContext {
    pub(crate) fn registry(&self) -> Result<Rc<Globals>> {
        match self {
            Self::Owner(registry) => Ok(registry.clone()),
            Self::Member(registry) => registry
                .upgrade()
                .ok_or_else(|| evaluation("global component tree was released")),
        }
    }
}

pub(crate) struct State {
    pub(crate) binding_lifetime: ComponentLifetime,
    program: Program,
    pub(crate) globals: GlobalContext,
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
    values: BTreeMap<Member, Property<Value>>,
    selectors: Vec<SelectorState>,
    visual_cache: RefCell<BTreeMap<VisualSlot, Rc<Property<Value>>>>,
    action_flags: RefCell<ActionFlagCache>,
    callbacks: BTreeMap<Member, CallbackSlot<CallbackBinding>>,
    controls: BTreeMap<Member, Property<Option<Value>>>,
    pub(crate) layout: viewport::Handles,
    combo_items: RefCell<BTreeMap<String, Rc<Property<xrack_runtime::ComboItems>>>>,
    image_playback: RefCell<BTreeMap<String, Rc<ImagePlaybackSlot>>>,
    media: RefCell<BTreeMap<String, Rc<xrack_runtime::MediaSlot>>>,
    table_columns: OnceCell<Property<xrack_runtime::TableColumnWindow>>,
    scene: Property<Rc<Node>>,
    pub(crate) error: Rc<Cell<Option<Error>>>,
    pub(crate) check_groups: RefCell<BTreeMap<String, Rc<Property<xrack_runtime::CheckState>>>>,
}

#[derive(Default)]
struct SelectorState {
    binding: Property<i64>,
    rendered: Cell<Option<i64>>,
}

struct RenderedSelection<'a> {
    slot: &'a Cell<Option<i64>>,
    previous: Option<i64>,
}

impl Drop for RenderedSelection<'_> {
    fn drop(&mut self) {
        self.slot.set(self.previous);
    }
}

#[derive(Clone, Copy)]
struct TargetRoute<'a> {
    follow_row: bool,
    input: Option<xrack_runtime::InputRoute<'a>>,
}

struct LocatedTarget<'a, 'p> {
    owner: Instance,
    element: &'a Element,
    remaining: &'p [usize],
    visibility_overridden: bool,
}

impl<'a> LocatedTarget<'a, '_> {
    fn exact(self) -> Option<(Instance, &'a Element)> {
        self.remaining
            .is_empty()
            .then_some((self.owner, self.element))
    }
}

type BoundInput = InputProperty<Result<Value>>;
type InstanceInputs = BTreeMap<Vec<Member>, BoundInput>;
type CallbackBinding = Rc<dyn Fn(&[Value]) -> Result<Option<Value>>>;
type InstanceCallbacks = BTreeMap<Vec<Member>, CallbackBinding>;
type ActionFlagCache = BTreeMap<Member, Rc<Property<Option<Value>>>>;

fn evaluation(message: impl Into<String>) -> Error {
    Error::Evaluation(message.into())
}

fn host_callback_arguments(args: &[Value]) -> Vec<Value> {
    args.iter()
        .map(|arg| match arg {
            Value::StringModel(model) => Value::StringList(model.snapshot_untracked()),
            value => value.clone(),
        })
        .collect()
}

impl Program {
    pub(crate) fn allocate(
        &self,
        name: &str,
        parent: Option<Weak<State>>,
        incoming: InstanceInputs,
        callbacks: InstanceCallbacks,
        stack: &mut Vec<String>,
    ) -> Result<Instance> {
        let definition = self.definition(name)?;
        self.allocate_scope(
            name,
            parent,
            (incoming, callbacks),
            &definition.root,
            None,
            stack,
        )
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

    fn allocate_scope(
        &self,
        name: &str,
        parent: Option<Weak<State>>,
        (incoming, mut callbacks): (InstanceInputs, InstanceCallbacks),
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
        for (mut path, input) in incoming {
            if path.len() > 1 {
                forwarded.insert(path, input);
                continue;
            }
            let endpoint = path.pop().expect("verified instance input endpoint");
            if !definition.properties.contains_key(&endpoint) && !controls.contains_key(&endpoint) {
                return Err(Error::Invalid(format!(
                    "unknown input target {}.{}",
                    endpoint.owner, endpoint.name
                )));
            }
            local.insert(endpoint, input);
        }
        let owner = row
            .as_ref()
            .map(|scope| &scope.owner)
            .or(parent.as_ref())
            .and_then(Weak::upgrade);
        let registry = owner
            .as_ref()
            .map_or_else(|| Ok(Rc::default()), |owner| owner.globals.registry())?;
        let globals = if self.globals.contains_key(name) {
            GlobalContext::Member(Rc::downgrade(&registry))
        } else {
            GlobalContext::Owner(registry.clone())
        };
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
        let instance = Instance(
            Rc::new(State {
                binding_lifetime: ComponentLifetime::default(),
                program: self.clone(),
                globals,
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
                    .map(|_| SelectorState::default())
                    .collect(),
                visual_cache: RefCell::default(),
                action_flags: RefCell::default(),
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
                media: RefCell::default(),
                table_columns: OnceCell::new(),
                scene: Property::default(),
                check_groups: RefCell::default(),
                error,
            }),
            None,
        );
        for (path, member) in &definition.callback_defaults {
            if keys.contains(member.owner.as_str()) {
                callbacks.entry(path.clone()).or_insert_with(|| {
                    instance.bind_callback(definition.callbacks[member].clone())
                });
            }
        }
        let mut children = BTreeMap::new();
        stack.push(name.into());
        let result = root.allocate_children(
            self,
            &instance,
            &mut forwarded,
            &mut callbacks,
            &mut children,
            stack,
        );
        stack.pop();
        result?;
        if let Some((path, _)) = forwarded.first_key_value() {
            let member = &path[0];
            return Err(Error::Invalid(format!(
                "unknown input target {}.{}",
                member.owner, member.name
            )));
        }
        assert!(instance.0.children.set(children).is_ok());
        for (mut path, callback) in callbacks {
            let member = path.pop().expect("verified callback endpoint");
            if !path.is_empty() {
                return Err(evaluation(format!(
                    "unknown callback instance {}",
                    path[0].owner
                )));
            }
            let slot = instance.0.callbacks.get(&member).ok_or_else(|| {
                evaluation(format!("unknown callback {}.{}", member.owner, member.name))
            })?;
            slot.set(callback);
        }
        root.bind_viewport_offsets(&instance);
        Ok(instance)
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
        forwarded: &mut InstanceInputs,
        callbacks: &mut InstanceCallbacks,
        children: &mut BTreeMap<String, Instance>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        if let Visual::Instance { component, inputs } = &self.visual {
            let incoming = self.child_inputs(parent, inputs, forwarded);
            let mut handlers: InstanceCallbacks = self
                .instance_handlers
                .iter()
                .map(|(path, function)| (path.clone(), parent.bind_callback(function.clone())))
                .collect();
            for (mut path, callback) in
                callbacks.extract_if(.., |path, _| path.len() > 1 && path[0].owner == self.key)
            {
                path.remove(0);
                handlers.insert(path, callback);
            }
            let child = program.allocate(
                component,
                Some(Rc::downgrade(&parent.0)),
                incoming,
                handlers,
                stack,
            )?;
            children.insert(self.key.clone(), child);
        }
        for element in self.owned_children().filter(|child| child.repeat.is_none()) {
            element.allocate_children(program, parent, forwarded, callbacks, children, stack)?;
        }
        Ok(())
    }

    fn child_inputs(
        &self,
        parent: &Instance,
        inputs: &BTreeMap<String, Input>,
        forwarded: &mut InstanceInputs,
    ) -> InstanceInputs {
        let mut bindings: InstanceInputs = inputs
            .values()
            .filter_map(|input| {
                let source = input.source.as_ref()?;
                Some((input.endpoint.clone(), parent.bind_input(source)))
            })
            .collect();
        for (mut path, input) in forwarded.extract_if(.., |path, _| path[0].owner == self.key) {
            path.remove(0);
            bindings.insert(path, input);
        }
        for input in inputs.values().filter(|input| !input.cases.is_empty()) {
            let key = input.endpoint.clone();
            let base = bindings.remove(&key).unwrap_or_default();
            let cases = input.cases.clone();
            let owner = Rc::downgrade(&parent.0);
            let state = base.with_state(move || {
                let owner = owner.upgrade()?;
                owner.binding_lifetime.track();
                match Instance(owner, None).state_value(&cases) {
                    Ok(value) => value.map(Ok),
                    Err(error) => Some(Err(error)),
                }
            });
            bindings.insert(key, state);
        }
        bindings
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

    fn allocate_controls(&self, controls: &mut BTreeMap<Member, Property<Option<Value>>>) {
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
            (BTreeMap::new(), BTreeMap::new()),
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
        self.0.row.as_ref()?.owner.upgrade().map(Self::from_state)
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

    fn read_row_field_from(
        field: &RowField,
        row: &RowScope,
        model: &Model,
    ) -> Result<Option<Value>> {
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
    }

    fn read_row_field(&self, field: &RowField) -> Result<Option<Value>> {
        self.with_row_model(field, |_, row, model| {
            Self::read_row_field_from(field, row, model)
        })
    }

    fn edit_field_matches(
        field: &RowField,
        model: &Model,
        index: usize,
        key: xrack_runtime::RowKey,
        expected: &Value,
    ) -> bool {
        match model {
            Model::Records(records) => records.model().row_matches(index, key, |record| {
                record.field_path(&field.path) == Some(expected)
            }),
            Model::Strings(strings) if field.path.is_empty() => strings.row_matches(
                index,
                key,
                |text| matches!(expected, Value::String(value) if value == text),
            ),
            _ => false,
        }
    }

    fn write_row_field(
        &self,
        field: &RowField,
        expected: Option<&Value>,
        value: Value,
    ) -> Result<bool> {
        self.prepare_row_field_write(
            field,
            expected,
            value,
            || Ok(true),
            |_, _, _, _, history| history().map(Some),
        )
    }

    pub(crate) fn prepare_row_field_write(
        &self,
        field: &RowField,
        expected: Option<&Value>,
        value: Value,
        available: impl FnOnce() -> Result<bool>,
        prepare: impl FnOnce(
            &Self,
            Option<&Element>,
            &Model,
            &dyn Fn() -> bool,
            &dyn Fn() -> Result<bool>,
        ) -> Result<Option<bool>>,
    ) -> Result<bool> {
        self.with_row_model(field, |owner, row, model| {
            let field_current = || {
                expected.is_none_or(|expected| {
                    model
                        .index_of_source_key(row.context.key(), row.context.index() as usize)
                        .is_some_and(|index| {
                            Self::edit_field_matches(
                                field,
                                model,
                                index,
                                row.context.key(),
                                expected,
                            )
                        })
                })
            };
            let view = owner
                .definition()
                .root
                .editor_view_for_template(&row.root_key);
            let table = view.filter(|view| view.table.is_some());
            let target;
            let (model, state, history) = if let Some(table) = table {
                let record_history = table.table_history_configured();
                let rows = || {
                    owner
                        .editable_table_view_model(table)?
                        .map(|model| model.table_rows())
                        .transpose()
                };
                let prepare = || {
                    prepare(owner, view, model, &field_current, &|| {
                        Ok(record_history && owner.table_history_enabled(table)?)
                    })
                };
                let edit = if let Some(expected) = expected {
                    row.context.try_table_edit_target_matching(
                        rows,
                        |target| {
                            Self::edit_field_matches(
                                field,
                                target.rows.source(),
                                target.index,
                                target.key,
                                expected,
                            )
                        },
                        available,
                        prepare,
                    )?
                } else {
                    row.context.try_table_edit_target_for_field(
                        rows,
                        |target| Self::read_row_field_from(field, row, target.rows.source()),
                        || if available()? { prepare() } else { Ok(None) },
                    )?
                };
                let Some((resolved, history)) = edit else {
                    return Ok(false);
                };
                target = resolved;
                (
                    target.rows.source(),
                    record_history.then(|| owner.repeat_state(table)),
                    history,
                )
            } else {
                if !field_current()
                    || !available()?
                    || !field_current()
                    || prepare(owner, view, model, &field_current, &|| Ok(false))?.is_none()
                {
                    return Ok(false);
                }
                (model, None, false)
            };
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
                BoundInput::bound_optional(move || {
                    let owner = owner.upgrade()?;
                    owner.binding_lifetime.track();
                    Some(Instance(owner, None).evaluate(&expression, &[]))
                })
            }
            InputSource::RowLink(field) => {
                let read_field = field.clone();
                let write_field = field.clone();
                let write_owner = owner.clone();
                BoundInput::linked(
                    move || {
                        let owner = owner.upgrade()?;
                        owner.binding_lifetime.track();
                        Instance(owner, None)
                            .read_row_field(&read_field)
                            .transpose()
                    },
                    move |value| {
                        if let Some(owner) = write_owner.upgrade() {
                            let instance = Instance(owner, None);
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
                    move || {
                        let owner = owner.upgrade()?;
                        owner.binding_lifetime.track();
                        Some(Instance(owner, None).read(&read_target))
                    },
                    move |value| {
                        if let Some(owner) = write_owner.upgrade() {
                            let instance = Instance(owner, None);
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

    pub(crate) fn shared_global(&self, name: &str) -> Result<Instance> {
        let registry = self.0.globals.registry()?;
        if let Some(instance) = registry.borrow().get(name).cloned() {
            return Ok(instance);
        }
        let instance = self.program().allocate(
            name,
            Some(Rc::downgrade(&self.0)),
            BTreeMap::new(),
            BTreeMap::new(),
            &mut Vec::new(),
        )?;
        registry.borrow_mut().insert(name.into(), instance.clone());
        Ok(instance)
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
        if let Some(name) = member.owner.strip_prefix("__global_") {
            return self.shared_global(name)?.read(&Member::root(&member.name));
        }
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
        let initial = match &property.source {
            PropertySource::Initial(initial) => initial,
            PropertySource::Alias(target) => return self.read(target),
        };
        let initial = || self.evaluate(initial, &[]);
        if let Some(input) = self.0.incoming.get(member) {
            input.get_result(initial)
        } else {
            self.0.values[member].try_get(initial)
        }
    }

    fn choose_state(&self, index: usize) -> Result<i64> {
        for (case, condition) in self.definition().selectors[index]
            .conditions
            .iter()
            .enumerate()
        {
            match self.evaluate(condition, &[])? {
                Value::Bool(true) => return Ok(case as i64),
                Value::Bool(false) => {}
                _ => return Err(evaluation("invalid state condition")),
            }
        }
        Ok(-1)
    }

    fn read_state(&self, index: usize) -> Result<i64> {
        self.0.selectors[index]
            .binding
            .try_get(|| self.choose_state(index))
    }

    fn select_state(&self, index: usize) -> Result<i64> {
        if self.definition().selectors[index].persistent {
            self.read_state(index)
        } else {
            self.choose_state(index)
        }
    }

    // Keep the selection stable for this render, even if a binding mutates a predicate input.
    fn render_selection(&self, index: usize) -> Result<RenderedSelection<'_>> {
        Ok(self.scoped_selection(index, self.select_state(index)?))
    }

    fn scoped_selection(&self, index: usize, selected: i64) -> RenderedSelection<'_> {
        let slot = &self.0.selectors[index].rendered;
        RenderedSelection {
            slot,
            previous: slot.replace(Some(selected)),
        }
    }

    fn rendered_state(&self, index: usize) -> Result<i64> {
        self.0.selectors[index]
            .rendered
            .get()
            .map_or_else(|| self.select_state(index), Ok)
    }

    pub(crate) fn state_value(&self, cases: &[StateCase]) -> Result<Option<Value>> {
        self.state_value_with(cases, |selector| {
            if self.0.row.as_ref().is_some_and(|row| row.sampled) {
                // Table projections share a selector across the cells sampled for one row.
                self.read_state(selector)
            } else {
                self.select_state(selector)
            }
        })
    }

    pub(crate) fn input_state_value(&self, cases: &[StateCase]) -> Result<Option<Value>> {
        self.state_value_with(cases, |index| {
            let selector = &self.definition().selectors[index];
            if selector.persistent {
                self.read_state(index)
            } else {
                self.0
                    .binding_lifetime
                    .try_input_state(&selector.owner, || self.choose_state(index))
            }
        })
    }

    fn state_value_with(
        &self,
        cases: &[StateCase],
        select: impl FnOnce(usize) -> Result<i64>,
    ) -> Result<Option<Value>> {
        let Some(first) = cases.first() else {
            return Ok(None);
        };
        let selected = select(first.selector)?;
        cases
            .iter()
            .find(|case| case.index == selected)
            .map(|case| self.evaluate(&case.value, &[]))
            .transpose()
    }

    pub(crate) fn caches_action_flag(
        &self,
        element: &Element,
        name: &str,
        cases: &[StateCase],
    ) -> bool {
        element.action.is_some()
            && matches!(name, "enabled" | "visible")
            && self.definition().selectors[cases[0].selector].persistent
    }

    pub(crate) fn cached_action_flag(
        &self,
        element: &Element,
        name: &str,
        cases: &[StateCase],
    ) -> Result<Option<Value>> {
        let cache = self
            .0
            .action_flags
            .borrow_mut()
            .entry(Member::new(&element.key, name))
            .or_default()
            .clone();
        cache.try_get(|| match self.state_value(cases)? {
            Some(value) => Ok(Some(value)),
            None => self.binding_value(element, name),
        })
    }

    pub(crate) fn write(&self, member: &Member, value: Value) -> Result<()> {
        if let Some(name) = member.owner.strip_prefix("__global_") {
            return self
                .shared_global(name)?
                .write(&Member::root(&member.name), value);
        }
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
        if let PropertySource::Alias(target) = &property.source {
            return self.write(target, value);
        }
        if let Some(input) = self.0.incoming.get(member) {
            input.set_if_changed_by(Ok(value), |left, right| {
                matches!((left, right), (Ok(left), Ok(right)) if left.same_property_value(right))
            });
        } else {
            self.0.values[member].set_if_changed_by(value, Value::same_property_value);
        }
        Ok(())
    }

    pub(crate) fn reset_member(&self, member: &Member) -> Result<()> {
        if let Some(name) = member.owner.strip_prefix("__global_") {
            return self
                .shared_global(name)?
                .reset_member(&Member::root(&member.name));
        }
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
        if let PropertySource::Alias(target) = &self.property(member)?.source {
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
        let records = self.program().records.clone();
        Rc::new(move |args| {
            let Some(owner) = owner.upgrade() else {
                return function
                    .result
                    .as_ref()
                    .map(|ty| load::default_value(&records, ty).map_err(evaluation))
                    .transpose();
            };
            owner.binding_lifetime.track();
            Instance(owner, None).execute(&function, args, None)
        })
    }

    fn callback_definition(&self, member: &Member) -> Result<&Function> {
        self.definition()
            .callbacks
            .get(member)
            .ok_or_else(|| evaluation(format!("unknown callback {}.{}", member.owner, member.name)))
    }

    pub(crate) fn register_callback(&self, member: &Member, callback: Callback) -> Result<()> {
        if self.callback_definition(member)?.pure {
            self.install_callback(
                member,
                Rc::new(move |args| {
                    callback(&host_callback_arguments(args)).map_err(Error::Evaluation)
                }),
            )
        } else {
            self.install_mut_callback(member, move |args| callback(args))
        }
    }

    pub(crate) fn register_mut_callback(
        &self,
        member: &Member,
        callback: impl FnMut(&[Value]) -> std::result::Result<Option<Value>, String> + 'static,
    ) -> Result<()> {
        if self.callback_definition(member)?.pure {
            return Err(evaluation(format!(
                "pure callback {}.{} requires an immutable handler",
                member.owner, member.name
            )));
        }
        self.install_mut_callback(member, callback)
    }

    fn install_mut_callback(
        &self,
        member: &Member,
        callback: impl FnMut(&[Value]) -> std::result::Result<Option<Value>, String> + 'static,
    ) -> Result<()> {
        let handler = EffectfulCallback::new(callback);
        self.install_callback(
            member,
            Rc::new(move |args| {
                handler
                    .try_call(|callback| callback(&host_callback_arguments(args)))
                    .map_err(|error| evaluation(error.to_string()))?
                    .map_err(Error::Evaluation)
            }),
        )
    }

    fn install_callback(&self, member: &Member, callback: CallbackBinding) -> Result<()> {
        if let Some(name) = member.owner.strip_prefix("__global_") {
            return self
                .shared_global(name)?
                .install_callback(&Member::root(&member.name), callback);
        }
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

    pub(crate) fn call(&self, member: &Member, arguments: Vec<Value>) -> Result<Option<Value>> {
        if let Some(name) = member.owner.strip_prefix("__global_") {
            return self
                .shared_global(name)?
                .call(&Member::root(&member.name), arguments);
        }
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
        self.program().validate_callback_arguments(
            &member.name,
            &function.parameters,
            &arguments,
        )?;
        let slot = &self.0.callbacks[member];
        let result = self.execute(function, &arguments, Some(slot))?;
        self.program().validate_callback_result(
            &member.name,
            function.result.as_ref(),
            result.as_ref(),
        )?;
        // A host handler can return its argument. Release the call's copies
        // before deciding whether the returned model is shared.
        drop(arguments);
        Ok(result.map(callback_value))
    }

    fn call_with_operands(
        &self,
        target: &Member,
        operands: &[Expr],
        args: &[Value],
    ) -> Result<Option<Value>> {
        // A later operand can call the host and edit a model read by an earlier operand.
        let arguments = operands
            .iter()
            .map(|operand| self.evaluate(operand, args).map(callback_value))
            .collect::<Result<Vec<_>>>()?;
        self.call(target, arguments)
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
        if slot.is_none()
            && (args.len() != function.parameters.len()
                || !args
                    .iter()
                    .zip(&function.parameters)
                    .all(|(arg, ty)| ty.matches(arg)))
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
                    self.call_with_operands(target, arguments, args)?;
                }
                Statement::Log {
                    level,
                    message,
                    file,
                    span,
                } => {
                    let Value::LogLevel(level) = self.evaluate(level, args)? else {
                        return Err(evaluation("log level must be a LogLevel"));
                    };
                    let Value::String(message) = self.evaluate(message, args)? else {
                        return Err(evaluation("log message must be a string"));
                    };
                    xrack_runtime::log_ui(
                        level,
                        &message,
                        &self.0.name,
                        file,
                        span.line,
                        span.column,
                    );
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
                    let action = match operands {
                        Some([first, second]) => command.action(&[
                            self.int_argument(first, args)?,
                            self.int_argument(second, args)?,
                        ]),
                        None => command.action(&[]),
                    }
                    .ok_or_else(|| evaluation("invalid focus command arguments"))?;
                    self.focus_handle(target)?.request(action);
                }
                Statement::Tree(target, command, key, depth) => {
                    let key = self.int_argument(key, args)?;
                    let depth = depth
                        .as_ref()
                        .map(|depth| self.int_argument(depth, args))
                        .transpose()?;
                    let action = command
                        .action(key, depth)
                        .ok_or_else(|| Error::Invalid("tree command missing depth".into()))?;
                    self.tree_command(target, action)?;
                }
                Statement::Timer(target, command) => self.timer_command(target, *command)?,
                Statement::Media(target, command) => {
                    let command = command.try_evaluate(|position| {
                        let Value::Float(seconds) = self.evaluate(position, args)? else {
                            return Err(evaluation("MediaPlayer seek requires seconds"));
                        };
                        Ok(seconds)
                    })?;
                    self.media_slot(target).command(command);
                }
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
                    self.restore_split_state_by_key(target, &serialized)?;
                }
                Statement::Popup(target, open) => {
                    self.set_overlay(target, *open, None)?;
                }
                Statement::NativeDialogOpen(target) => self.open_native_dialog(target)?,
                Statement::WindowClose => self.0.window.request_close(),
                Statement::SecondaryWindow(target, command) => {
                    self.secondary_window_command(target, *command)?;
                }
                Statement::WindowCommand(method, enabled) => {
                    let enabled = enabled
                        .as_ref()
                        .map(|expr| match self.evaluate(expr, args)? {
                            Value::Bool(value) => Ok(value),
                            _ => Err(evaluation("expected a boolean full-screen argument")),
                        })
                        .transpose()?;
                    let command = method
                        .action(enabled)
                        .ok_or_else(|| evaluation("invalid window command arguments"))?;
                    self.0.window.request(command);
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
        if slot.is_some() {
            // Ordinary calls validate the result with the callback's name and schema.
            return Ok(result);
        }
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

    fn media_slot(&self, key: &str) -> Rc<xrack_runtime::MediaSlot> {
        self.0
            .media
            .borrow_mut()
            .entry(key.into())
            .or_default()
            .clone()
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
        // Table commands sample expressions against the row passed to the command.
        if self.0.row.as_ref().is_some_and(|row| row.sampled) {
            return self.evaluate(value, &[]);
        }
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
        cache.try_get(|| owner.evaluate(value, &[]))
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
            StackRead(target, read) => Ok(crate::stack::read(
                self.stack_snapshot_by_key(target)?.as_ref(),
                read,
            )),
            SplitSave(target) => self.save_split_state_by_key(target).map(Value::String),
            SplitResizing(target) => self.split_resizing(target).map(Value::Bool),
            TreeQuery(target, query, key) => {
                self.tree_query(target, *query, self.int_argument(key, args)?)
            }
            ViewIndex(target, index, direction) => {
                let index = self.int_argument(index, args)?;
                direction
                    .try_query(index, |index, direction| {
                        self.view_index(target, index, direction)
                    })
                    .map(Value::Int)
            }
            TableRead(target, read) => self.table_read(target, *read),
            ScrollControlRead(member) => self.scroll_control_read(member),
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
            MediaRead(member) => self
                .media_slot(&member.owner)
                .read(&member.name)
                .ok_or_else(|| evaluation("invalid MediaPlayer state property")),
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
                .map(callback_parameter)
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
                    (UnaryOperator::Not, Value::Bool(v)) => Ok(Value::Bool(!v)),
                    (UnaryOperator::Negate, Value::Int(v)) => Ok(Value::Int(v.wrapping_neg())),
                    (UnaryOperator::Negate, Value::Float(v)) => Ok(Value::Float(-v)),
                    (UnaryOperator::Negate, Value::Length(Length(v))) => {
                        Ok(Value::Length(Length(-v)))
                    }
                    _ => Err(evaluation("invalid unary operand")),
                }
            }
            Binary(operator, lhs, rhs) => {
                let left = self.evaluate(lhs, args)?;
                if (*operator == BinaryOperator::And && left == Value::Bool(false))
                    || (*operator == BinaryOperator::Or && left == Value::Bool(true))
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
            MathFunction(kind, inputs) => math_function(*kind, &self.arguments(inputs, args)?),
            StringFunction(kind, inputs) => string_function(*kind, &self.arguments(inputs, args)?),
            ModelCount(input) | ModelIsEmpty(input) => {
                let count = match self.evaluate(input, args)? {
                    Value::StringList(rows) => rows.len(),
                    Value::StringModel(rows) => rows.len(),
                    Value::RecordList(rows) => rows.model().len(),
                    _ => return Err(evaluation("model count requires a model")),
                };
                Ok(if matches!(expression.as_ref(), ModelIsEmpty(_)) {
                    Value::Bool(count == 0)
                } else {
                    Value::Int(count as i64)
                })
            }
            ModelAt(model, index, fallback) => {
                let model = self.evaluate(model, args)?;
                let Value::Int(index) = self.evaluate(index, args)? else {
                    return Err(evaluation("model row index requires an int"));
                };
                let row = match model {
                    Value::StringList(rows) => usize::try_from(index)
                        .ok()
                        .and_then(|index| rows.get(index))
                        .cloned()
                        .map(Value::String),
                    Value::StringModel(rows) => rows.row_at(index).map(Value::String),
                    Value::RecordList(rows) => {
                        self.program().record_at(&rows, index)?.map(Value::Record)
                    }
                    _ => return Err(evaluation("model row requires a model")),
                };
                row.map_or_else(|| self.evaluate(fallback, args), Ok)
            }
            ColorFunction(kind, inputs) => color_function(*kind, &self.arguments(inputs, args)?),
            Locale => Ok(Value::String(
                self.0.locale.locale(self.0.program.translations.as_deref()),
            )),
            FormatNumber(input, decimals) => {
                let input = self.evaluate(input, args)?;
                let Value::Int(decimals) = self.evaluate(decimals, args)? else {
                    return Err(evaluation("number format precision requires an int"));
                };
                let locale = &self.0.locale;
                let catalog = self.0.program.translations.as_deref();
                let text = match input {
                    Value::Int(value) => locale.format_number_int(catalog, value, decimals),
                    Value::Float(value) => locale.format_number_float(catalog, value, decimals),
                    _ => return Err(evaluation("number format requires an int or float")),
                };
                Ok(Value::String(text))
            }
            ParseNumber(input, fallback) => {
                let Value::String(input) = self.evaluate(input, args)? else {
                    return Err(evaluation("number parsing requires a string"));
                };
                let Value::Float(fallback) = self.evaluate(fallback, args)? else {
                    return Err(evaluation("number parsing fallback requires a float"));
                };
                Ok(Value::Float(self.0.locale.parse_number(
                    self.0.program.translations.as_deref(),
                    &input,
                    fallback,
                )))
            }
            FormatFixed(input, precision) => {
                Ok(Value::String(match self.evaluate(input, args)? {
                    Value::Int(value) => xrack_runtime::format_fixed_int(value, *precision),
                    Value::Float(value) => xrack_runtime::format_fixed_float(value, *precision),
                    _ => return Err(evaluation("fixed precision requires an int or float")),
                }))
            }
            Round(input) => match self.evaluate(input, args)? {
                Value::Float(value) => Ok(Value::Int(xrack_runtime::round_to_int(value))),
                _ => Err(evaluation("round requires a float operand")),
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
                    let angle_degrees = angle
                        .as_ref()
                        .map(|angle| match self.evaluate(angle, args)? {
                            Value::Float(degrees) => Ok(degrees),
                            _ => Err(evaluation("invalid gradient angle")),
                        })
                        .transpose()?;
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
                    let brush = if let Some(angle_degrees) = angle_degrees {
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
            StateSelect(index) => self.select_state(*index).map(Value::Int),
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
            RecordField(input, name) => {
                let value = match self.evaluate(input, args)? {
                    Value::Record(record) => record.field(name).cloned(),
                    _ => None,
                };
                value.ok_or_else(|| evaluation(format!("unknown record field {name}")))
            }
            BuiltinField(input, field) => field
                .read(self.evaluate(input, args)?)
                .ok_or_else(|| evaluation("invalid built-in field operand")),
            MakeDate([year, month, day]) => Ok(Value::Date(xrack_runtime::Date {
                year: self.int_argument(year, args)?,
                month: self.int_argument(month, args)?,
                day: self.int_argument(day, args)?,
            })),
            DateIsValid(date) => Ok(Value::Bool(self.date_argument(date, args)?.valid())),
            DateWeekday(date) => Ok(Value::Int(
                self.date_argument(date, args)?.weekday().unwrap_or(0),
            )),
            DateAddDays(date, offset, fallback) | DateAddMonths(date, offset, fallback) => {
                let date = self.date_argument(date, args)?;
                let offset = self.int_argument(offset, args)?;
                let shifted = if matches!(expression.as_ref(), DateAddDays(..)) {
                    date.add_days(offset)
                } else {
                    date.add_months_clamped(offset)
                };
                match shifted {
                    Some(date) => Ok(Value::Date(date)),
                    None => self.evaluate(fallback, args),
                }
            }
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
                .call_with_operands(target, arguments, args)?
                .ok_or_else(|| evaluation("expression callback must return a value")),
        }
    }

    fn date_argument(&self, expression: &Expr, args: &[Value]) -> Result<xrack_runtime::Date> {
        match self.evaluate(expression, args)? {
            Value::Date(date) => Ok(date),
            _ => Err(evaluation("date argument requires a Date")),
        }
    }

    fn int_argument(&self, expression: &Expr, args: &[Value]) -> Result<i64> {
        let Value::Int(value) = self.evaluate(expression, args)? else {
            return Err(evaluation("expected integer argument"));
        };
        Ok(value)
    }

    pub(crate) fn render_scene(&self) -> Result<Rc<Node>> {
        self.0.scene.try_get(|| {
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
            .is_some_and(|repeat| !repeat.uses_row_identity || repeat.scene_cache.is_some())
        {
            return self.render_element(template);
        }
        self.0
            .scene
            .try_get(|| self.render_element(template).map(Rc::new))
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
        let items = cache.try_get(|| {
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
        items.populate_scene(node);
        Ok(())
    }

    fn animation(&self, spec: &AnimationSpec) -> Result<Option<Animation>> {
        if let Some(enabled) = &spec.enabled
            && self.evaluate(enabled, &[])? == Value::Bool(false)
        {
            return Ok(None);
        }
        Ok(Some(spec.timing))
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
                let template = self.render_element_body(element, include_children, None)?;
                return Ok(xrack_runtime::static_repeat_scene(
                    template,
                    count,
                    repeat.positioned,
                ));
            }
            let children = self.repeat_state(element).render(self, element, 0..4096)?;
            return Ok(xrack_runtime::repeat_scene(children, repeat.positioned));
        }
        self.render_element_body(element, include_children, None)
    }

    fn render_element_body(
        &self,
        element: &Element,
        include_children: bool,
        deferred_selection: Option<&mut Option<i64>>,
    ) -> Result<Node> {
        let include_plan = include_children || deferred_selection.is_some();
        let is_instance = matches!(element.visual, Visual::Instance { .. });
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
            Visual::Builtin(kind) => {
                let (kind, application_window) = xrack_resources::visual::scene_element(kind);
                let mut node = Node::new(kind, None);
                node.application_window = application_window;
                node
            }
            Visual::Instance { .. } => self.children()[&element.key]
                .render_scene()?
                .as_ref()
                .clone(),
        };
        node.set_owned_id((!element.id.is_empty()).then(|| element.id.clone()));
        let selection = element
            .render_selector
            .map(|index| self.render_selection(index))
            .transpose()?;
        if let Some(deferred) = deferred_selection {
            *deferred = selection
                .as_ref()
                .and_then(|selection| selection.slot.get());
        }
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
        for binding in &element.bindings {
            let name = binding.name;
            if element.state_bindings.contains_key(name) || element.control(name).is_some() {
                continue;
            }
            if node.kind == "Tumbler" && name == "model" {
                if let Some(items) = self.tumbler_items(element)? {
                    node.set_tumbler_items(items);
                }
                continue;
            }
            let value = self.evaluate(&binding.expression, &[])?;
            if is_instance {
                node.set_instance_layout_property(name, scene_value(value));
            } else if node.kind == "ComboBox" && name == "model" {
                let Value::StringModel(model) = value else {
                    return Err(evaluation("expected a string ComboBox model"));
                };
                node.set_string_combo_model(model);
            } else {
                node.properties.insert(name, scene_value(value));
            }
            if binding.paint_only {
                node.mark_paint_only(name);
            }
            if let Some(path) = &binding.vector_path {
                node.set_loaded_vector_path(path.clone());
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
            if let Some(value) = self.control_value(element, name)? {
                node.properties.insert(name, scene_value(value));
            }
        }
        if matches!(element.visual, Visual::Builtin("StandardButton")) {
            node.properties
                .entry("kind")
                .or_insert(xrack_runtime::StandardButtonKind::Ok.into());
        }
        if let Some(combo) = &element.record_combo {
            self.render_combo(element, combo, &mut node)?;
        }
        for (&name, cases) in &element.state_bindings {
            let selected = self.rendered_state(cases[0].selector)?;
            let value = match node.action_state_override(name) {
                Some(value) => Some(value),
                None if self.caches_action_flag(element, name, cases) => {
                    self.cached_action_flag(element, name, cases)?
                }
                None => match cases.iter().find(|case| case.index == selected) {
                    Some(case) => Some(self.evaluate(&case.value, &[])?),
                    None => self.binding_value(element, name)?,
                },
            };
            if let Some(value) = value {
                if is_instance {
                    node.set_instance_layout_property(name, scene_value(value));
                } else {
                    node.properties.insert(name, scene_value(value));
                }
            }
        }
        for (&name, specs) in &element.state_timings {
            let case = self.rendered_state(specs[0].selector)?;
            let timing = |enter| {
                specs
                    .iter()
                    .find(|spec| spec.index == case && spec.enter == enter)
                    .map(|spec| self.animation(&spec.animation))
                    .transpose()
                    .map(Option::flatten)
            };
            node.set_state_timing(
                name,
                StateTiming {
                    case,
                    enter: timing(true)?,
                    exit: timing(false)?,
                },
            );
        }
        if is_instance {
            return Ok(node);
        }
        if let Some(enabled) = action_enabled {
            let enabled = enabled && node.boolean("enabled", true);
            node.properties.insert("enabled", enabled.into());
        }
        if let Some(file) = &element.media_source_file {
            self.media_slot(&element.key).populate(&mut node, file);
        }
        if matches!(element.visual, Visual::Builtin("AnimatedImage")) {
            let state = Rc::downgrade(&self.0);
            let key = element.key.clone();
            self.playback_slot(&key).populate(&mut node, move || {
                let Some(state) = state.upgrade() else {
                    return false;
                };
                let owner = Instance(state, None);
                let Some(element) = owner.definition().root.find_key(&key) else {
                    return false;
                };
                element.handlers.get("finished").is_some_and(|handler| {
                    !handler.body.is_empty()
                        && owner.handle_result(owner.execute(handler, &[], None).map(|_| true))
                })
            });
        }
        for &(name, ref spec) in &element.animations {
            if let Some(animation) = self.animation(spec)? {
                node.set_animation(name, animation);
            }
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
        if include_plan {
            if let Some(grid) = &element.grid_plan {
                node.set_shared_grid(grid.clone());
            }
            if let Some(order) = &element.paint_plan {
                node.set_shared_paint_order(order.clone());
            }
        }
        Ok(node)
    }

    fn render_swipe_pages(&self, element: &Element, node: &mut Node) -> Result<()> {
        let mut selections = Vec::with_capacity(element.children.len());
        for page in &element.children {
            let mut selected = None;
            node.children
                .push(self.render_element_body(page, false, Some(&mut selected))?);
            selections.push(selected);
        }
        node.mount_active_swipe_page(|index, node| {
            let page = &element.children[index];
            let _selection = page
                .render_selector
                .zip(selections[index])
                .map(|(selector, selected)| self.scoped_selection(selector, selected));
            self.render_children(page, node)
        })
        .transpose()?;
        Ok(())
    }

    fn render_children(&self, element: &Element, node: &mut Node) -> Result<()> {
        if matches!(element.visual, Visual::Builtin("RangeSlider")) {
            node.add_range_thumbs();
        }
        if node.kind == "SwipeView" {
            return self.render_swipe_pages(element, node);
        }
        if matches!(node.kind, "RadioGroup" | "TabBar")
            && let Some(model) = self.group_option_model(element)?
        {
            node.snapshot_group_options(&model);
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
            _ => None,
        };
        for (index, child) in element.children.iter().enumerate() {
            let page = matches!(node.kind, "TabWidget" | "StackView");
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

    fn locate<'a, 'p>(
        &self,
        program: &'a Program,
        element: &'a Element,
        scene: &Node,
        path: &'p [usize],
        mut route: TargetRoute<'_>,
    ) -> Result<Option<LocatedTarget<'a, 'p>>> {
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
            let Some(row) =
                self.repeat_state(element)
                    .row(self, repeat, index, key, route.follow_row)?
            else {
                return Ok(None);
            };
            return row.locate(program, element, node, tail, route);
        }
        let visibility_overridden = route
            .input
            .is_some_and(InputRoute::root_visibility_overridden);
        if let Some(input) = route.input {
            let is_instance = matches!(element.visual, Visual::Instance { .. });
            let mut visible = None;
            let flag = |element, name| self.input_flag_value(element, name);
            if !input.try_available(|| -> Result<bool> {
                if path.is_empty()
                    && scene.is_item_delegate()
                    && self.row_template().is_some()
                    && input
                        .event()
                        .activates_item_delegate(element.action.is_some())
                {
                    return self.item_delegate_available(element);
                }
                if scene.input_requires_enabled(path.is_empty())
                    && flag(element, "enabled")? == Some(false)
                {
                    return Ok(false);
                }
                if !self.element_conditions_met(element)? {
                    return Ok(false);
                }
                visible = if visibility_overridden {
                    Some(true)
                } else {
                    flag(element, "visible")?
                };
                if !visible.unwrap_or(true) {
                    return Ok(false);
                }
                if is_instance {
                    // The instance and its component root share the same scene node.
                    // The root owns the control state that governs its descendants.
                    return Ok(true);
                }
                if let Some((&index, tail)) = path.split_first()
                    && (scene.kind != "TabWidget" || !tail.is_empty())
                {
                    let active = match scene.kind {
                        "TabWidget" => usize::try_from(self.current_index(element)?).ok(),
                        "SwipeView" => xrack_runtime::try_resolve_swipe_index(
                            Some(self.current_index(element)?),
                            element
                                .children
                                .iter()
                                .map(|page| self.input_element_available(page, false)),
                        )?,
                        "StackView" => self
                            .stack_snapshot_for(element)?
                            .current()
                            .and_then(|route| scene.stack_route_index(route)),
                        _ => Some(index),
                    };
                    if active != Some(index) {
                        return Ok(false);
                    }
                }
                scene.try_input_scope_available(
                    path.first().and_then(|&index| scene.child_at(index)),
                    || {
                        Ok(input.can_open_drawer(scene, path.is_empty())
                            || self.control_value(element, "open")? == Some(Value::Bool(true)))
                    },
                    || Ok(flag(element, "checkable")?.unwrap_or(false)),
                    || Ok(self.control_value(element, "checked")? != Some(Value::Bool(false))),
                    || {
                        Ok(match self.control_value(element, "open-side")? {
                            Some(Value::SwipeSide(side)) => side,
                            _ => xrack_runtime::SwipeSide::Closed,
                        })
                    },
                )
            })? {
                return Ok(None);
            }
            if element.table.is_some() || element.tree.is_some() {
                input.record_root_visibility_override(&self.0.binding_lifetime);
            }
            route.input =
                Some(input.with_root_visibility_override(visible.is_some() && is_instance));
        }
        if let Visual::Instance { component, .. } = &element.visual {
            let Some(child) = self.children().get(&element.key) else {
                return Ok(None);
            };
            return child.locate(
                program,
                &program.definition(component)?.root,
                scene,
                path,
                route,
            );
        }
        let target = |remaining| LocatedTarget {
            owner: self.clone(),
            element,
            remaining,
            visibility_overridden,
        };
        let Some((&index, tail)) = path.split_first() else {
            return Ok(Some(target(&[])));
        };
        let Some(node) = scene.child_at(index) else {
            return Ok(None);
        };
        if picker::property(element).is_some()
            || matches!(element.visual, Visual::Builtin("TimePicker"))
        {
            return Ok(Some(target(path)));
        }
        if scene.list.is_some() {
            let Some((&0, tail)) = tail.split_first() else {
                return Ok(Some(target(&[])));
            };
            let (Some(template), Some((index, row_key, child))) =
                (element.view_template(), scene.list_event_row(index))
            else {
                return Ok(None);
            };
            let Some(repeat) = &template.repeat else {
                return Ok(None);
            };
            let state = self.repeat_state(template);
            let (model, source) = if route.follow_row {
                (state.row_model(self, repeat)?, Some(index))
            } else if element.tree.is_some() {
                let Some(tree) = &scene.tree else {
                    return Ok(None);
                };
                let (live, model) = self.tree_projection(element)?;
                (model, tree.event_source_index(&live, index, row_key))
            } else {
                let model = self.view_model(element)?;
                let source = xrack_runtime::TableRowPositions::event_source_index_in(
                    model.positions(),
                    scene,
                    index,
                );
                let Some(model) = model.source else {
                    return Ok(None);
                };
                (model, source)
            };
            let Some(source) = source else {
                return Ok(None);
            };
            let Some(row) = state.resolved_row(repeat, &model, source, row_key, route.follow_row)
            else {
                return Ok(None);
            };
            if child.is_item_delegate()
                && template.action.is_some()
                && route
                    .input
                    .is_some_and(|input| input.event().activates_item_delegate(true))
            {
                model.track_item(source);
            }
            return row.locate(program, template, child, tail, route);
        }
        let Some(child) = index
            .checked_sub(element.synthetic_child_count())
            .and_then(|index| element.children.get(index))
        else {
            return Ok(None);
        };
        self.locate(program, child, node, tail, route)
    }

    pub(crate) fn try_target<'a>(
        &'a self,
        scene: &Node,
        path: &[usize],
    ) -> Result<Option<(Self, &'a Element)>> {
        Ok(self
            .locate(
                &self.0.program,
                &self.definition().root,
                scene,
                path,
                TargetRoute {
                    follow_row: false,
                    input: None,
                },
            )?
            .and_then(LocatedTarget::exact))
    }

    pub(crate) fn input_route_target<'a, 'i>(
        &'a self,
        scene: &Node,
        path: &[usize],
        input: InputRoute<'i>,
    ) -> Result<Option<(Self, &'a Element, InputRoute<'i>)>> {
        Ok(self.locate_input(scene, path, input)?.and_then(|target| {
            let input = input.with_root_visibility_override(target.visibility_overridden);
            let (owner, element) = target.exact()?;
            Some((owner, element, input))
        }))
    }

    fn locate_input<'a, 'p>(
        &'a self,
        scene: &Node,
        path: &'p [usize],
        input: InputRoute<'_>,
    ) -> Result<Option<LocatedTarget<'a, 'p>>> {
        self.locate(
            &self.0.program,
            &self.definition().root,
            scene,
            path,
            TargetRoute {
                follow_row: input.event().follows_row(scene, path),
                input: Some(input),
            },
        )
    }

    fn route_typed_input(
        &self,
        scene: &Node,
        path: &[usize],
        input: InputRoute<'_>,
    ) -> Result<bool> {
        let Some((owner, element, input)) = self.input_route_target(scene, path, input)? else {
            return Ok(false);
        };
        if !input.is_current() {
            return Ok(false);
        }
        match input.event() {
            RoutedEvent::Focus(event) => owner.focus_input(element, *event),
            RoutedEvent::Key(phase, event) => owner.handle_input_result(
                element,
                phase.handler(),
                Value::KeyEvent((*event).clone()),
            ),
            RoutedEvent::Wheel(event) => {
                let name = if matches!(element.visual, Visual::Builtin("TouchArea")) {
                    "scroll-event"
                } else {
                    "wheel"
                };
                owner.handle_input_result(element, name, Value::WheelEvent(**event))
            }
            RoutedEvent::Pointer(event) => {
                owner.handler(element, "pointer-event", &[Value::PointerEvent(**event)])
            }
            RoutedEvent::Drag(event) => owner.drag_input(element, event),
            _ => Ok(false),
        }
    }

    fn handle_input_result(&self, element: &Element, event: &str, value: Value) -> Result<bool> {
        let Some(handler) = element.handlers.get(event) else {
            return Ok(false);
        };
        Ok(self.execute(handler, &[value], None)? == Some(Value::EventResult(EventResult::Accept)))
    }

    fn focus_input(&self, element: &Element, event: FocusEvent) -> Result<bool> {
        event.try_deliver(|handler, focused, reason| {
            let reason = Value::FocusReason(reason);
            if matches!(handler, xrack_runtime::FocusHandler::Within) {
                self.handler(element, handler.name(), &[Value::Bool(focused), reason])
            } else {
                self.handler(element, handler.name(), &[reason])
            }
        })
    }

    fn drag_input(&self, element: &Element, event: &DragEvent) -> Result<bool> {
        // A repeated template has no drag route of its own. A component
        // mounted in the row receives events through its own static paths.
        if self.row_template().is_some()
            || !matches!(element.visual, Visual::Builtin(kind) if kind == event.target_kind())
        {
            return Ok(false);
        }
        let (name, arguments) = event.notification();
        if let Some(handler) = element.handlers.get(name) {
            let args = match arguments {
                xrack_runtime::DragArguments::None => vec![],
                xrack_runtime::DragArguments::Action(action) => vec![action.into()],
                xrack_runtime::DragArguments::Data(data, action) => {
                    vec![data.to_owned().into(), action.into()]
                }
                xrack_runtime::DragArguments::Files(paths) => {
                    vec![Value::StringModel(paths.to_vec().into())]
                }
                xrack_runtime::DragArguments::Position(x, y) => vec![x.into(), y.into()],
            };
            self.execute(handler, &args, None)?;
        }
        Ok(true)
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
            Some(element) => self.dispatch_element(element, event, None, None),
            None => Ok(false),
        }
    }

    pub(crate) fn item_delegate_available(&self, element: &Element) -> Result<bool> {
        let mut selection = None;
        let mut flag = |name| -> Result<bool> {
            if let Some(cases) = element.state_bindings.get(name) {
                let selector = cases[0].selector;
                let selected = match selection {
                    Some((previous, selected)) if previous == selector => selected,
                    _ => {
                        let selected = self.choose_state(selector)?;
                        selection = Some((selector, selected));
                        selected
                    }
                };
                if let Some(case) = cases.iter().find(|case| case.index == selected) {
                    return Ok(self.evaluate(&case.value, &[])? == Value::Bool(true));
                }
            }
            Ok(self.binding_value(element, name)? != Some(Value::Bool(false)))
        };
        if !flag("enabled")? {
            return Ok(false);
        }
        for condition in &element.conditions {
            if self.evaluate(condition, &[])? == Value::Bool(false) {
                return Ok(false);
            }
        }
        flag("visible")
    }

    fn dispatch_element(
        &self,
        element: &Element,
        event: &str,
        node: Option<&Node>,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        let Visual::Builtin(kind) = element.visual else {
            return Ok(false);
        };
        if event == "clicked" {
            if let Some(input) = input
                && !input.try_available(|| self.check_group_member_enabled(element))?
            {
                return Ok(false);
            }
            if matches!(kind, "Button" | "ToolButton" | "RoundButton")
                && let Some(action) = &element.action
            {
                return self.activate_action(element, action, input);
            }
            match kind {
                "GroupBox" => return self.toggle_group_box(element, input),
                _ if xrack_runtime::is_toggle(kind) => {
                    return self.dispatch_toggle(element, node, input);
                }
                _ => {}
            }
        }
        if xrack_runtime::supports_untyped_dispatch(kind, event) {
            if let Some(input) = input
                && event == "accepted"
                && matches!(kind, "TextInput" | "LineEdit")
                && element.handlers.contains_key(event)
                && !input.try_available(|| {
                    let text = self.control_value(element, "text")?;
                    let text = match &text {
                        Some(Value::String(text)) => text.as_str(),
                        _ => "",
                    };
                    xrack_runtime::try_text_submission_is_valid(
                        text,
                        self.editor_input_type(element)?,
                        || self.editor_decimal_separator(element),
                        || self.editor_maximum_length(element),
                    )
                })?
            {
                return Ok(false);
            }
            return self.handler(element, event, &[]);
        }
        Ok(false)
    }

    fn dispatch_toggle(
        &self,
        element: &Element,
        node: Option<&Node>,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        use xrack_runtime::CheckState;

        if input.is_some_and(|input| !input.is_current()) {
            return Ok(false);
        }
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
        if input.is_some_and(|input| !input.is_current()) {
            return Ok(false);
        }
        let value = if check_state {
            Value::CheckState(state)
        } else {
            Value::Bool(state == CheckState::Checked)
        };
        let projected = check_state && element.bindings.iter().any(|binding| binding.name == name);
        if !projected {
            let updated = match (row, element.row_links.get(name), input) {
                (Some(node), Some(field), Some(input)) => self.prepare_row_field_write(
                    field,
                    node.properties.get(name),
                    value,
                    || Ok(true),
                    |owner, view, _, field_current, history| {
                        if let Some(tree) = view.filter(|view| view.tree.is_some()) {
                            owner.tree_projection(tree)?;
                        }
                        xrack_runtime::try_prepare_row_toggle_matching(
                            node,
                            input,
                            check_state.then_some(|| {
                                Ok(self.binding_value(element, "tristate")? == Some(true.into()))
                            }),
                            field_current,
                            history,
                        )
                    },
                )?,
                _ => self.set_control_value(element, name, value)?,
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

    fn set_overlay(
        &self,
        key: &str,
        open: bool,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        let (owner, element) = self.overlay_owner(key)?;
        xrack_runtime::try_set_overlay_open(
            open,
            input,
            || Ok(owner.control_value(element, "open")? == Some(Value::Bool(true))),
            |open| {
                owner
                    .set_control_value(element, "open", open.into())
                    .map(|_| ())
            },
            |open| {
                owner
                    .handler(element, if open { "opened" } else { "closed" }, &[])
                    .map(|_| ())
            },
        )
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
        let read_size = || -> Result<f64> {
            Ok(match self.binding_value(element, "size")? {
                Some(Value::Float(size)) => size,
                _ => ScrollBarRange::DEFAULT_SIZE,
            })
        };
        let value = if size {
            ScrollBarRange::new(0.0, read_size()?, 0.0).size
        } else {
            ScrollBarRange::try_position_with(
                || {
                    Ok(match self.control_value(element, "position")? {
                        Some(Value::Float(position)) => position,
                        _ => 0.0,
                    })
                },
                read_size,
            )?
        };
        Ok(Value::Float(value))
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
                .get_result(|| {
                    self.binding_value(element, name)?
                        .map_or_else(|| self.default_control_value(element, name), Ok)
                })
                .map(Some);
        }
        match self.0.controls.get(&member) {
            Some(slot) => slot.try_get(|| {
                let value = self.binding_value(element, name)?;
                if value.is_some()
                    || element
                        .control(name)
                        .is_some_and(|control| control.omit_default)
                {
                    return Ok(value);
                }
                self.default_control_value(element, name).map(Some)
            }),
            None => self.binding_value(element, name),
        }
    }

    pub(crate) fn current_index(&self, element: &Element) -> Result<i64> {
        let name = "current-index";
        let value = self
            .control_value(element, name)?
            .map_or_else(|| self.default_control_value(element, name), Ok)?;
        match value {
            Value::Int(index) => Ok(index),
            _ => Err(evaluation("expected an integer current index")),
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
        if view.table.is_some() {
            return owner.select_table_source(view, source, column);
        }
        let Some(index) = xrack_runtime::try_prepare_source_selection(
            || Ok(owner.view_model(view)?.visual_index(source)),
            || owner.current_index(view),
        )?
        else {
            return Ok(());
        };
        let value = Value::Int(index);
        if owner.set_control_value(view, "current-index", value.clone())? {
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
            input.set_if_changed_by(Ok(model_value(value)), |left, right| {
                matches!((left, right), (Ok(left), Ok(right)) if left.same_property_value(right))
            });
            return Ok(true);
        }
        let Some(slot) = self.0.controls.get(&member) else {
            return Ok(false);
        };
        slot.set_if_changed_by(Some(value), |left, right| {
            matches!((left, right), (Some(left), Some(right)) if left.same_property_value(right))
        });
        Ok(true)
    }

    fn combo_action(
        &self,
        element: &Element,
        node: &Node,
        action: ControlAction,
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        use xrack_runtime::{ComboEditor, ComboUpdate, try_prepare_combo_input_with_live_draft};

        let property = element.selection_property();
        let input = try_prepare_combo_input_with_live_draft(
            node,
            action,
            || self.element_allows_routed_input(element, route),
            || self.table_flag(element, "editable", false),
            || self.combo_model_current(element, node),
            || self.combo_draft_current(element, node, || Ok(true)),
        )?;
        if !route.is_current() {
            return Ok(false);
        }
        let Some(input) = input else {
            return Ok(false);
        };
        let by_value = property == "current-value";
        let editor = match element.control("edit-text") {
            None => ComboEditor::None,
            Some(control) if control.omit_default => ComboEditor::Default,
            Some(_) => ComboEditor::Explicit,
        };
        input.try_apply(editor, by_value, element.record_combo.is_some(), |update| {
            match update {
                ComboUpdate::Select(selection) => {
                    let value = if by_value {
                        selection.value.clone()
                    } else {
                        Value::Int(selection.index as i64)
                    };
                    return self.update_control(element, node, property, value);
                }
                ComboUpdate::ClearIndex => {
                    self.update_control(element, node, property, Value::Int(-1))?;
                }
                ComboUpdate::SetFreeText(text) => {
                    self.update_control(element, node, property, Value::String(text.to_owned()))?;
                }
                ComboUpdate::SetEditText(text) => {
                    self.update_control(
                        element,
                        node,
                        "edit-text",
                        Value::String(text.to_owned()),
                    )?;
                }
                ComboUpdate::ResetEditText => {
                    self.0.controls[&Member::new(&element.key, "edit-text")].reset();
                }
                ComboUpdate::Selected(label) => {
                    self.handler(element, "selected", &[Value::String(label.to_owned())])?;
                }
                ComboUpdate::ValueSelected(value) => {
                    self.handler(element, "value-selected", std::slice::from_ref(value))?;
                }
                ComboUpdate::Accepted(text) => {
                    self.handler(element, "accepted", &[Value::String(text.to_owned())])?;
                }
            }
            Ok(true)
        })
    }

    fn combo_model_current(&self, element: &Element, node: &Node) -> Result<bool> {
        use xrack_runtime::combo_model_is_current;
        if let Some(combo) = &element.record_combo {
            let model = self.record_combo_model(combo)?;
            return Ok(combo_model_is_current(node, model.model()));
        }
        match self.binding_value(element, "model")? {
            Some(Value::StringModel(model)) => Ok(combo_model_is_current(node, &model)),
            None => Ok(true),
            _ => Err(evaluation("expected a string ComboBox model")),
        }
    }

    fn combo_draft_current(
        &self,
        element: &Element,
        node: &Node,
        model_current: impl FnOnce() -> Result<bool>,
    ) -> Result<bool> {
        if element.control("edit-text").is_none() {
            return Ok(false);
        }
        let draft = match self.control_value(element, "edit-text")? {
            Some(Value::String(text)) => Some(text),
            None => None,
            _ => return Err(evaluation("expected ComboBox draft text")),
        };
        node.try_combo_draft_is_current(draft, || {
            if !model_current()? {
                return Ok(false);
            }
            let name = element.selection_property();
            Ok(self
                .control_value(element, name)?
                .is_some_and(|value| node.combo_selection_is_current(name, &value)))
        })
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
        self.render_shared().list_rows_at(path, range)
    }

    fn dispatch(&mut self, id: &str, event: &str) -> bool {
        self.handle_result(self.dispatch_named(id, event))
    }

    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        xrack_runtime::RoutedEvent::Dispatch(event).deliver_input(self, scene, path)
    }

    fn route_input(&mut self, scene: &Node, path: &[usize], input: InputRoute<'_>) -> bool {
        match input.event() {
            RoutedEvent::Dispatch(event) => {
                let Some((instance, element, input)) = self
                    .capture_result(self.input_route_target(scene, path, input))
                    .flatten()
                else {
                    return false;
                };
                input.is_current()
                    && self.handle_result(instance.dispatch_element(
                        element,
                        event,
                        scene.at_path(path),
                        Some(input),
                    ))
            }
            RoutedEvent::Edit(text) => {
                let Some(node) = scene.at_path(path) else {
                    return false;
                };
                self.handle_result((|| {
                    let Some(target) = self.locate_input(scene, path, input)? else {
                        return Ok(false);
                    };
                    if !target.remaining.is_empty() {
                        return target.owner.edit_picker_hex(
                            target.element,
                            target.remaining,
                            node,
                            text,
                            input,
                        );
                    }
                    if !target
                        .owner
                        .live_editor_allows(target.element, node, text, input)?
                    {
                        return Ok(false);
                    }
                    target
                        .owner
                        .edit_control(target.element, node, (*text).into())
                })())
            }
            RoutedEvent::Control(action) => self.route_control(scene, path, *action, input),
            RoutedEvent::Range(action) => self.route_range(scene, path, *action, input),
            RoutedEvent::CommitCell { text, .. } => {
                self.handle_result(self.route_cell_commit(scene, path, text, input))
            }
            RoutedEvent::PasteTable(paste) => {
                self.handle_result(self.route_table_paste(scene, path, *paste, input))
            }
            RoutedEvent::ReplayTable { redo } => self.handle_result((|| {
                let Some(model) = scene.table_edit_history_model(path) else {
                    return Ok(false);
                };
                let Some((owner, view, input)) = self.input_route_target(scene, path, input)?
                else {
                    return Ok(false);
                };
                owner.replay_table_from_view(view, model, *redo, input)
            })()),
            RoutedEvent::ResizeSplit(_) | RoutedEvent::ResizeSplitSizes(_) => {
                self.handle_result(self.resize_split(scene, path, input))
            }
            RoutedEvent::Focus(_)
            | RoutedEvent::Key(..)
            | RoutedEvent::Wheel(_)
            | RoutedEvent::Pointer(_)
            | RoutedEvent::Drag(_) => {
                self.handle_result(self.route_typed_input(scene, path, input))
            }
            RoutedEvent::QueryTable(_) => false,
        }
    }

    fn focus_event_at(&mut self, scene: &Node, path: &[usize], event: FocusEvent) -> bool {
        RoutedEvent::Focus(event).deliver_input(self, scene, path)
    }

    fn key_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        phase: xrack_runtime::KeyPhase,
        event: &xrack_runtime::KeyEvent,
    ) -> EventResult {
        RoutedEvent::Key(phase, event)
            .deliver_input(self, scene, path)
            .into()
    }

    fn pointer_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: &xrack_runtime::PointerEvent,
    ) -> bool {
        RoutedEvent::Pointer(event).deliver_input(self, scene, path)
    }

    fn drag_event_at(&mut self, scene: &Node, path: &[usize], event: &DragEvent) -> bool {
        RoutedEvent::Drag(event).deliver_input(self, scene, path)
    }

    fn wheel_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        event: &xrack_runtime::WheelEvent,
    ) -> EventResult {
        RoutedEvent::Wheel(event)
            .deliver_input(self, scene, path)
            .into()
    }

    fn resize_split_at(&mut self, scene: &Node, path: &[usize], weights: &[(usize, f32)]) -> bool {
        xrack_runtime::RoutedEvent::ResizeSplit(weights).deliver_input(self, scene, path)
    }

    fn resize_split_sizes_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        sizes: &[(usize, f32)],
    ) -> bool {
        xrack_runtime::RoutedEvent::ResizeSplitSizes(sizes).deliver_input(self, scene, path)
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        xrack_runtime::RoutedEvent::Control(action).deliver_input(self, scene, path)
    }

    fn adjust_range_at(&mut self, scene: &Node, path: &[usize], action: RangeAction) -> bool {
        xrack_runtime::RoutedEvent::Range(action).deliver_input(self, scene, path)
    }

    fn copy_table_at(&self, scene: &Node, path: &[usize], columns: &[usize]) -> Option<String> {
        xrack_runtime::TableQuery::Copy(columns)
            .run(self, scene, path)?
            .text()
    }

    fn route_table_query(
        &self,
        scene: &Node,
        path: &[usize],
        input: InputRoute<'_>,
    ) -> Option<xrack_runtime::TableQueryResult> {
        use xrack_runtime::{RoutedEvent, TableQuery, TableQueryResult};
        let RoutedEvent::QueryTable(query) = *input.event() else {
            return None;
        };
        let node = scene.at_path(path)?;
        let (owner, element, input) = self
            .capture_result(self.input_route_target(scene, path, input))
            .flatten()?;
        element.table.as_ref()?;
        match query {
            TableQuery::Copy(columns) => self
                .capture_result(owner.copy_table(element, node, columns, input))
                .flatten()
                .map(TableQueryResult::Text),
            TableQuery::NextEdit(navigation) => self
                .capture_result(owner.table_edit_target(element, node, navigation, input))
                .flatten()
                .map(TableQueryResult::Position),
        }
    }

    fn prepare_table_paste(
        &self,
        scene: &Node,
        path: &[usize],
        row: usize,
        columns: Vec<usize>,
    ) -> Option<xrack_runtime::TablePaste> {
        xrack_runtime::prepare_table_paste(self, || self.as_weak(), scene, path, row, columns)
    }

    fn table_paste_stamp(&self, scene: &Node, path: &[usize]) -> Option<(u64, u64)> {
        self.capture_result(self.capture_table_paste_stamp(scene, path))
            .flatten()
    }

    fn replay_table_paste_at(&self, scene: &Node, path: &[usize], redo: bool) -> bool {
        xrack_runtime::RoutedEvent::ReplayTable { redo }.deliver_input(
            &mut self.clone(),
            scene,
            path,
        )
    }

    fn next_table_edit_cell(
        &self,
        scene: &Node,
        path: &[usize],
        origin: xrack_runtime::TableEditPosition,
        columns: &[usize],
        backward: bool,
    ) -> Option<xrack_runtime::TableEditPosition> {
        xrack_runtime::TableQuery::NextEdit(xrack_runtime::TableEditNavigation {
            origin,
            columns,
            backward,
        })
        .run(self, scene, path)?
        .position()
    }

    fn commit_table_cell(
        &mut self,
        scene: &Node,
        path: &[usize],
        original: &str,
        text: String,
    ) -> bool {
        xrack_runtime::RoutedEvent::CommitCell {
            original,
            text: &text,
        }
        .deliver_input(self, scene, path)
    }

    fn edit_at(&mut self, scene: &Node, path: &[usize], text: String) -> bool {
        xrack_runtime::RoutedEvent::Edit(&text).deliver_input(self, scene, path)
    }
}

impl Instance {
    fn route_cell_commit(
        &self,
        scene: &Node,
        path: &[usize],
        text: &str,
        input: InputRoute<'_>,
    ) -> Result<bool> {
        let Some(node) = scene.at_path(path) else {
            return Ok(false);
        };
        if let [table_path @ .., slot, 0, column] = path
            && let Some(table) = scene.at_path(table_path)
            && table.boolean("__standard-table", false)
        {
            let Some((owner, view, input)) = self.input_route_target(scene, table_path, input)?
            else {
                return Ok(false);
            };
            return owner.commit_projected_cell(view, table, *slot, *column, input);
        }
        let Some((owner, element, input)) = self.input_route_target(scene, path, input)? else {
            return Ok(false);
        };
        let Some(field) = element.row_links.get("text") else {
            return Ok(false);
        };
        owner.prepare_row_field_write(
            field,
            node.properties.get("text"),
            Value::String(text.into()),
            || {
                input.try_available(|| {
                    Ok(!owner.visual_flag(element, "read-only", false)?
                        && owner.visual_flag(element, "editable", true)?)
                })
            },
            |_, _, _, _, history| input.try_prepare(|| Ok(true), || history().map(Some)),
        )
    }

    fn route_range(
        &self,
        scene: &Node,
        path: &[usize],
        action: RangeAction,
        input: InputRoute<'_>,
    ) -> bool {
        let Some(node) = scene.range_target(path) else {
            return false;
        };
        self.handle_result((|| {
            if node.kind == "RangeSlider" {
                let Some((&handle, parent)) = path.split_last() else {
                    return Ok(false);
                };
                let Some(target) = self.locate_input(scene, parent, input)? else {
                    return Ok(false);
                };
                let input = input.with_root_visibility_override(target.visibility_overridden);
                let Some((instance, element)) = target.exact() else {
                    return Ok(false);
                };
                return instance.interval_action(element, node, handle, action, input);
            }
            let Some(target) = self.locate_input(scene, path, input)? else {
                return Ok(false);
            };
            let LocatedTarget {
                owner: instance,
                element,
                remaining,
                visibility_overridden,
            } = target;
            let input = input.with_root_visibility_override(visibility_overridden);
            if !remaining.is_empty() {
                return instance.picker_child_input(
                    element,
                    remaining,
                    node,
                    picker::Input::Range(action, input),
                );
            }
            match node.kind {
                "Slider" | "Dial" => instance.range_action(element, node, action, input),
                "ScrollBar"
                    if node.scroll_target.is_none() && node.boolean("interactive", true) =>
                {
                    instance.scroll_bar_action(element, node, action, input)
                }
                _ => Ok(false),
            }
        })())
    }

    fn route_control(
        &self,
        scene: &Node,
        path: &[usize],
        action: ControlAction,
        input: InputRoute<'_>,
    ) -> bool {
        let Some(node) = scene.at_path(path) else {
            return false;
        };
        let Some(target) = self
            .capture_result(self.locate_input(scene, path, input))
            .flatten()
        else {
            return false;
        };
        let LocatedTarget {
            owner: instance,
            element,
            remaining,
            visibility_overridden,
        } = target;
        let input = input.with_root_visibility_override(visibility_overridden);
        if !input.is_current() {
            return false;
        }
        if !remaining.is_empty() {
            return self.handle_result(instance.picker_child_input(
                element,
                remaining,
                node,
                picker::Input::Control(action, input),
            ));
        }
        if picker::property(element).is_some() {
            return self.handle_result(instance.picker_control(element, node, action, input));
        }
        if let ControlAction::Calendar(action) = action
            && xrack_runtime::is_calendar_control(node.kind)
        {
            return self.handle_result(instance.calendar_action(element, node, action, input));
        }
        if element.tree.is_some() {
            return self.handle_result(instance.tree_control(element, node, input));
        }
        if element.table.is_some() {
            return self.handle_result(instance.table_control(element, node, action, input));
        }
        if let Some(command) = &element.action {
            let expected = match node.kind {
                "MenuItem" => ControlAction::ActivateMenuItem,
                "Shortcut" => ControlAction::ActivateShortcut,
                _ => ControlAction::ActivateAction,
            };
            return action == expected
                && self.handle_result(instance.activate_action(element, command, Some(input)));
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
                    Some(input),
                )
            }
            ControlAction::SwipeDelegate(side) if node.kind == "SwipeDelegate" => (|| {
                if !xrack_runtime::try_prepare_swipe_side(
                    node,
                    side,
                    input,
                    || {
                        Ok(match instance.control_value(element, "open-side")? {
                            Some(Value::SwipeSide(side)) => side,
                            _ => xrack_runtime::SwipeSide::Closed,
                        })
                    },
                    |kind| {
                        let Some(panel) = element.children.iter().find(
                            |child| matches!(child.visual, Visual::Builtin(name) if name == kind),
                        ) else {
                            return Ok(false);
                        };
                        Ok(instance.input_flag_value(panel, "visible")?.unwrap_or(true))
                    },
                )? {
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
                instance.activate_radio_option(element, node, input)
            }
            ControlAction::Select(index)
                if matches!(
                    node.kind,
                    "RadioGroup" | "TabWidget" | "TabBar" | "SwipeView"
                ) =>
            {
                instance.select_group(element, node, index, input)
            }
            ControlAction::Select(index) if node.kind == "PageIndicator" => {
                instance.select_page_indicator(element, index, input)
            }
            ControlAction::ActivateMenuItem if node.kind == "MenuItem" => {
                instance.activate_menu_item(element, input)
            }
            ControlAction::Dialog(_) | ControlAction::DialogCommand(_) if node.kind == "Dialog" => {
                instance.dialog_action(element, action, input)
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
                let selected = xrack_runtime::try_prepare_index_selection(
                    index,
                    || instance.element_allows_routed_input(element, input),
                    |_| {
                        let model = instance.view_model(element)?;
                        Ok(xrack_runtime::ListMetrics::has_row_in_model(
                            node,
                            index,
                            model.stamp(),
                        ))
                    },
                    || instance.current_index(element),
                )?;
                let Some(index) = selected else {
                    return Ok(false);
                };
                if !input.is_current() {
                    return Ok(false);
                }
                let value = Value::Int(index);
                if !instance.update_control(element, node, "current-index", value.clone())? {
                    return Ok(false);
                }
                instance.handler(element, "changed", &[value])?;
                Ok(true)
            })(),
            ControlAction::Select(_) | ControlAction::AcceptComboText
                if node.kind == "ComboBox" =>
            {
                instance.combo_action(element, node, action, input)
            }
            ControlAction::DelayButton(checked) if node.kind == "DelayButton" => (|| {
                let applicable = xrack_runtime::try_delay_button_transition_is_current(
                    node,
                    checked,
                    || instance.element_allows_routed_input(element, input),
                    || {
                        Ok(match instance.binding_value(element, "delay")? {
                            Some(Value::Int(delay)) => Some(delay),
                            _ => None,
                        })
                    },
                    || Ok(instance.control_value(element, "checked")? == Some(Value::Bool(true))),
                )?;
                if !input.is_current()
                    || !applicable
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
                instance.set_overlay(&element.key, open, Some(input))
            }
            ControlAction::SubmitSearch | ControlAction::Select(_) | ControlAction::ClearSearch
                if node.kind == "SearchField" =>
            {
                instance.search_control(element, node, action, input)
            }
            ControlAction::PickTumbler { .. } if node.kind == "Tumbler" => {
                instance.tumbler_action(element, node, action, input)
            }
            ControlAction::Spin(action) if xrack_runtime::is_spin_box(node.kind) => {
                instance.spin_action(element, node, action, input)
            }
            ControlAction::ActivateTextLink(index)
                if node.kind == "StyledText" && element.handlers.contains_key("link-activated") =>
            {
                (|| {
                    let target = node.try_prepare_text_link(index, input, || {
                        if let Some(cases) = element.state_bindings.get("text")
                            && let Some(Value::StyledText(text)) =
                                instance.input_state_value(cases)?
                        {
                            return Ok(text);
                        }
                        Ok(match instance.binding_value(element, "text")? {
                            Some(Value::StyledText(text)) => text,
                            _ => xrack_runtime::StyledText::default(),
                        })
                    })?;
                    let Some(target) = target else {
                        return Ok(false);
                    };
                    instance.handler(element, "link-activated", &[target.to_owned().into()])
                })()
            }
            _ => Ok(false),
        })
    }

    fn editor_input_type(&self, element: &Element) -> Result<xrack_runtime::InputType> {
        Ok(match self.binding_value(element, "input-type")? {
            Some(Value::InputType(mode)) => mode,
            _ => xrack_runtime::InputType::Text,
        })
    }

    fn editor_decimal_separator(&self, element: &Element) -> Result<char> {
        Ok(match self.binding_value(element, "decimal-separator")? {
            Some(Value::String(separator)) if separator == "," => ',',
            _ => '.',
        })
    }

    fn editor_maximum_length(&self, element: &Element) -> Result<Option<i64>> {
        Ok(match self.binding_value(element, "maximum-length")? {
            Some(Value::Int(limit)) => Some(limit),
            _ => None,
        })
    }

    fn live_editor_allows(
        &self,
        element: &Element,
        node: &Node,
        text: &str,
        input: InputRoute<'_>,
    ) -> Result<bool> {
        if node.kind == "ComboBox" {
            return node.try_prepare_combo_edit(
                input,
                || self.visual_flag(element, "editable", false),
                || {
                    self.combo_draft_current(element, node, || {
                        self.combo_model_current(element, node)
                    })
                },
            );
        }
        if !xrack_runtime::is_text_input(node.kind) {
            return Ok(false);
        }
        node.try_prepare_text_edit(
            input,
            || Ok(!self.visual_flag(element, "read-only", false)?),
            || match self.control_value(element, "text")? {
                Some(Value::String(text)) => Ok(text),
                _ => Err(evaluation("expected text editor value")),
            },
            || {
                let mode = if matches!(node.kind, "TextInput" | "LineEdit") {
                    self.editor_input_type(element)?
                } else {
                    xrack_runtime::InputType::Text
                };
                node.try_allows_text_edit_with(
                    text,
                    mode,
                    || self.editor_decimal_separator(element),
                    || self.editor_maximum_length(element),
                )
            },
        )
    }

    fn search_control(
        &self,
        element: &Element,
        node: &Node,
        action: ControlAction,
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let input = xrack_runtime::SearchInput::try_from_control(
            node,
            action,
            || self.element_allows_routed_input(element, route),
            || Ok(!self.table_flag(element, "read-only", false)?),
            || match self.control_value(element, "text")? {
                Some(Value::String(text)) => Ok(text),
                None => Ok(String::new()),
                _ => Err(evaluation("expected search text")),
            },
            || match self.binding_value(element, "suggestions-for")? {
                Some(Value::String(query)) => Ok(Some(query)),
                None => Ok(None),
                _ => Err(evaluation("expected a search suggestion query")),
            },
            |index| match self.binding_value(element, "suggestions")? {
                Some(Value::StringModel(model)) => Ok(model.row(index)),
                Some(Value::StringList(items)) => Ok(items.get(index).cloned()),
                None => Ok(None),
                _ => Err(evaluation("expected a search suggestion list")),
            },
        )?;
        if !route.is_current() {
            return Ok(false);
        }
        match input {
            Some(input) => self.apply_search_input(element, node, input),
            None => Ok(false),
        }
    }

    fn apply_search_input(
        &self,
        element: &Element,
        node: &Node,
        input: xrack_runtime::SearchInput,
    ) -> Result<bool> {
        use xrack_runtime::SearchUpdate;

        input.try_apply(
            || self.table_flag(element, "live", false),
            |update| {
                let (event, text) = match update {
                    SearchUpdate::SetText(text) => {
                        return self.update_control(
                            element,
                            node,
                            "text",
                            Value::String(text.into()),
                        );
                    }
                    SearchUpdate::Edited(text) => ("edited", text),
                    SearchUpdate::Accepted(text) => ("accepted", text),
                    SearchUpdate::Selected(text) => ("selected", text),
                    SearchUpdate::Triggered(text) => ("search-triggered", text),
                };
                self.handler(element, event, &[Value::String(text.into())])?;
                Ok(true)
            },
        )
    }

    fn edit_control(&self, element: &Element, node: &Node, text: String) -> Result<bool> {
        if node.kind == "SearchField" {
            return self.apply_search_input(element, node, xrack_runtime::SearchInput::Edit(text));
        }
        let value = Value::String(text);
        let combo = node.kind == "ComboBox";
        let name = if combo { "edit-text" } else { "text" };
        if !self.update_control(element, node, name, value.clone())? {
            return Ok(false);
        }
        let typed = combo || xrack_runtime::is_multiline_text_input(node.kind);
        let args = std::slice::from_ref(&value);
        self.handler(element, "edited", if typed { args } else { &[] })?;
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
                | (Self::LogLevel, Value::LogLevel(_))
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

/// Match the generated callback ABI, which passes list contents by value.
pub(super) fn callback_value(value: Value) -> Value {
    match value {
        Value::StringModel(model) => Value::StringModel(model.into_snapshot().into()),
        Value::RecordList(list) => {
            let (name, rows) = list.into_snapshot();
            Value::RecordList(RecordList::new(name, rows))
        }
        Value::TouchPoints(points) => Value::TouchPoints(points.into_snapshot().into()),
        value => model_value(value),
    }
}

fn callback_parameter(value: &Value) -> Value {
    // The call site already tracked the live model. A parameter read only
    // copies the sampled values into the expression's own model.
    match value {
        Value::StringModel(model) => Value::StringModel(model.snapshot_untracked().into()),
        Value::RecordList(list) => Value::RecordList(RecordList::new(
            list.name(),
            list.model().snapshot_untracked(),
        )),
        Value::TouchPoints(points) => Value::TouchPoints(points.snapshot_untracked().into()),
        value => model_value(value.clone()),
    }
}

pub(crate) fn host_value(value: Value) -> Value {
    match value {
        Value::StringModel(model) => Value::StringList(model.into_snapshot()),
        value => value,
    }
}

fn scene_value(value: Value) -> Value {
    match value {
        Value::Brush(brush) => Value::from(brush),
        value => host_value(value),
    }
}

fn binary(program: &Program, operator: BinaryOperator, lhs: Value, rhs: Value) -> Result<Value> {
    use BinaryOperator::*;
    use Value::*;
    if matches!(operator, Equal | NotEqual) {
        let equal = match (lhs, rhs) {
            (StringModel(a), StringModel(b)) => a.values_eq_owned(b),
            (TouchPoints(a), TouchPoints(b)) => a.values_eq_owned(b),
            (StringModel(model), StringList(items)) | (StringList(items), StringModel(model)) => {
                model.into_snapshot() == items
            }
            (RecordList(a), RecordList(b)) => a
                .values_eq_owned(b, &program.record_validator)
                .map_err(evaluation)?,
            (lhs, rhs) => lhs.eq_with_image_assets(&rhs),
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
            Divide => xrack_runtime::try_int_divide(a, b)
                .map(Int)
                .map_err(evaluation),
            Remainder => xrack_runtime::try_int_remainder(a, b)
                .map(Int)
                .map_err(evaluation),
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

fn math_function(kind: xrack_syntax::builtins::MathFunction, inputs: &[Value]) -> Result<Value> {
    use xrack_syntax::builtins::MathFunction::*;

    let invalid = || evaluation("invalid math function operands");
    if let [Value::Int(a), rest @ ..] = inputs {
        return Ok(Value::Int(match (kind, rest) {
            (Min, [Value::Int(b)]) => (*a).min(*b),
            (Max, [Value::Int(b)]) => (*a).max(*b),
            (Clamp, [Value::Int(b), Value::Int(c)]) => xrack_runtime::clamp_int(*a, *b, *c),
            (Abs, []) => a.saturating_abs(),
            (Mod, [Value::Int(b)]) => xrack_runtime::modulo_int(*a, *b),
            _ => return Err(invalid()),
        }));
    }
    let float = |value: &Value| match value {
        Value::Float(value) => Ok(*value),
        Value::Length(value) => Ok(value.0),
        Value::Percent(value) => Ok(value.0),
        _ => Err(invalid()),
    };
    let [first, rest @ ..] = inputs else {
        return Err(invalid());
    };
    let a = float(first)?;
    let value = match (kind, rest) {
        (Min, [b]) => xrack_runtime::min_float(a, float(b)?),
        (Max, [b]) => xrack_runtime::max_float(a, float(b)?),
        (Clamp, [b, c]) => xrack_runtime::clamp_float(a, float(b)?, float(c)?),
        (Abs, []) => a.abs(),
        (Mod, [b]) => a.rem_euclid(float(b)?),
        (Sign, []) => xrack_runtime::number_sign(a),
        (Sqrt, []) => a.sqrt(),
        (Pow, [b]) => a.powf(float(b)?),
        (Exp, []) => a.exp(),
        (Ln, []) => a.ln(),
        (Log, [b]) => a.log(float(b)?),
        (Sin, []) => a.sin(),
        (Cos, []) => a.cos(),
        (Tan, []) => a.tan(),
        (Asin, []) => a.asin(),
        (Acos, []) => a.acos(),
        (Atan, []) => a.atan(),
        (Atan2, [b]) => a.atan2(float(b)?),
        (Hypot, [b]) => a.hypot(float(b)?),
        (ToRadians, []) => a.to_radians(),
        (ToDegrees, []) => a.to_degrees(),
        (IsFinite, []) => return Ok(Value::Bool(a.is_finite())),

        (Floor, []) => return Ok(Value::Int(xrack_runtime::floor_to_int(a))),
        (Ceil, []) => return Ok(Value::Int(xrack_runtime::ceil_to_int(a))),
        _ => return Err(invalid()),
    };
    Ok(match first {
        Value::Length(_) => Value::Length(Length(value)),
        Value::Percent(_) => Value::Percent(xrack_runtime::Percent(value)),
        _ => Value::Float(value),
    })
}

fn string_function(
    kind: xrack_syntax::builtins::StringFunction,
    inputs: &[Value],
) -> Result<Value> {
    use xrack_syntax::builtins::StringFunction::*;
    match (kind, inputs) {
        (Trim, [Value::String(text)]) => Ok(Value::String(text.trim().to_owned())),
        (IsEmpty, [Value::String(text)]) => Ok(Value::Bool(text.is_empty())),
        (Contains, [Value::String(text), Value::String(part)]) => {
            Ok(Value::Bool(text.contains(part.as_str())))
        }
        (StartsWith, [Value::String(text), Value::String(prefix)]) => {
            Ok(Value::Bool(text.starts_with(prefix.as_str())))
        }
        (EndsWith, [Value::String(text), Value::String(suffix)]) => {
            Ok(Value::Bool(text.ends_with(suffix.as_str())))
        }
        (ReplaceAll, [Value::String(text), Value::String(from), Value::String(to)]) => {
            Ok(Value::String(text.replace(from.as_str(), to)))
        }
        (Lowercase, [Value::String(text)]) => Ok(Value::String(text.to_lowercase())),
        (Uppercase, [Value::String(text)]) => Ok(Value::String(text.to_uppercase())),
        (IsInt, [Value::String(text)]) => Ok(Value::Bool(xrack_runtime::parse_int(text).is_some())),
        (IsFloat, [Value::String(text)]) => {
            Ok(Value::Bool(xrack_runtime::parse_float(text).is_some()))
        }
        (ToInt, [Value::String(text), Value::Int(fallback)]) => Ok(Value::Int(
            xrack_runtime::parse_int(text).unwrap_or(*fallback),
        )),
        (ToFloat, [Value::String(text), Value::Float(fallback)]) => Ok(Value::Float(
            xrack_runtime::parse_float(text).unwrap_or(*fallback),
        )),
        (CharacterCount, [Value::String(text)]) => {
            Ok(Value::Int(xrack_runtime::character_count(text)))
        }
        _ => Err(evaluation("invalid string function operands")),
    }
}

fn color_function(kind: xrack_syntax::builtins::ColorFunction, inputs: &[Value]) -> Result<Value> {
    use xrack_runtime::Color;
    use xrack_syntax::builtins::ColorFunction::*;
    Ok(Value::Color(match (kind, inputs) {
        (Rgb, [Value::Int(r), Value::Int(g), Value::Int(b)]) => Color::from_rgba(*r, *g, *b, 1.0),
        (Rgba, [Value::Int(r), Value::Int(g), Value::Int(b), Value::Float(a)]) => {
            Color::from_rgba(*r, *g, *b, *a)
        }
        (WithAlpha, [Value::Color(color), Value::Float(alpha)]) => color.with_alpha(*alpha),
        (
            Mix,
            [
                Value::Color(first),
                Value::Color(second),
                Value::Float(amount),
            ],
        ) => first.mix(*second, *amount),
        _ => return Err(evaluation("invalid color function operands")),
    }))
}
