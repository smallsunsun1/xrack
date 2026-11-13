//! Decode visual operations shared by the code generator and interpreter.
use crate::{AliasGraph, OperationRef, PropertyEndpoint, ValueRef, canonical_input};
use std::collections::{BTreeMap, BTreeSet};

/// Direct children that occupy a scene slot, in declaration order.
pub fn scene_children(op: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    op.children().filter(|child| {
        matches!(child.name(), "xr.element" | "xr.instance")
            && !matches!(
                child.attribute("kind"),
                Some(
                    "Action"
                        | "ActionGroup"
                        | "ButtonGroup"
                        | "Timer"
                        | "SecondaryWindow"
                        | "DayCell"
                        | "SplitHandle"
                        | "FileDialog"
                        | "MessageDialog"
                )
            )
    })
}

/// Repeats with an explicit position use an overlay instead of a vertical layout.
pub fn repeat_is_positioned(element: OperationRef<'_>) -> bool {
    element.children().any(|op| {
        matches!(op.name(), "xr.bind" | "xr.layout_bind")
            && matches!(op.attribute("name"), Some("x" | "y"))
    })
}

/// An expression cache planned by the native XR passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisualCacheGroup {
    Binding(u32),
    RepeatInvariant(u32),
    RepeatSubexpression(u32),
}

impl VisualCacheGroup {
    pub fn for_binding(op: OperationRef<'_>) -> Option<Self> {
        op.attribute("xr.repeat_invariant_group")
            .and_then(|group| group.parse().ok())
            .map(Self::RepeatInvariant)
            .or_else(|| {
                op.attribute("xr.visual_cache_group")
                    .and_then(|group| group.parse().ok())
                    .map(Self::Binding)
            })
    }

    pub fn for_subexpression(op: OperationRef<'_>) -> Option<Self> {
        op.attribute("xr.repeat_subexpression_group")
            .and_then(|group| group.parse().ok())
            .map(Self::RepeatSubexpression)
    }
}

/// A visual cache keyed by a pass group or the original SSA operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisualCacheKey<Operand> {
    Operand(Operand),
    Planned(VisualCacheGroup),
}

impl<Operand> VisualCacheKey<Operand> {
    pub fn uses_component_scope(&self) -> bool {
        matches!(
            self,
            Self::Planned(
                VisualCacheGroup::RepeatInvariant(_) | VisualCacheGroup::RepeatSubexpression(_)
            )
        )
    }
}

impl<'m> VisualCacheKey<ValueRef<'m>> {
    pub fn for_binding(binding: OperationRef<'m>, use_visual_plan: bool) -> Option<Self> {
        if use_visual_plan && let Some(group) = VisualCacheGroup::for_binding(binding) {
            return Some(Self::Planned(group));
        }
        binding.operand(0).map(Self::Operand)
    }
}

/// Literals, plain reads and row parameters already have direct access paths.
pub fn visual_cache_operand(
    binding: OperationRef<'_>,
    use_visual_plan: bool,
) -> Option<OperationRef<'_>> {
    let definition = binding.operand(0)?.defining_operation()?;
    let static_gradient = use_visual_plan
        && matches!(
            definition.name(),
            "xr.linear_gradient" | "xr.radial_gradient"
        )
        && definition.attribute("xr.static_gradient") == Some("true");
    (!static_gradient
        && !matches!(
            definition.name(),
            "xr.literal" | "xr.read" | "xr.image_url" | "xr.param"
        ))
    .then_some(definition)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSceneCache {
    Keyed,
    Indexed,
}

impl RowSceneCache {
    pub fn from_operation(op: OperationRef<'_>) -> Option<Self> {
        match op.attribute("xr.row_scene_cache")? {
            "keyed" => Some(Self::Keyed),
            "indexed" => Some(Self::Indexed),
            _ => None,
        }
    }
}

pub struct AnimationRef<'m> {
    pub duration_ms: u64,
    pub enabled: Option<ValueRef<'m>>,
    pub delay_ms: u64,
    pub curve: Option<[f64; 4]>,
}

impl<'m> AnimationRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        let offset = match op.name() {
            "xr.animate" => 0,
            "xr.state_timing" => 1,
            name => return Err(format!("expected animation timing, found {name}")),
        };
        let duration_ms = op
            .operand(offset)
            .and_then(|value| value.defining_operation())
            .filter(|definition| definition.name() == "xr.literal")
            .and_then(|literal| literal.attribute("value"))
            .and_then(|spelling| spelling.parse::<i64>().ok())
            .and_then(|duration| u64::try_from(duration).ok())
            .filter(|&duration| duration <= 60_000)
            .ok_or("animation requires a constant duration between 0 and 60000ms")?;
        Ok(Self {
            duration_ms,
            enabled: op.operand(offset + 1),
            delay_ms: u64::try_from(op.integer_attribute("delay").unwrap_or_default())
                .map_err(|_| "negative animation delay")?,
            curve: op.f64_array_attribute("curve"),
        })
    }
}

/// State selectors, instance bindings and callback aliases shared by both backends.
#[derive(Default)]
pub struct ComponentAnalysis<'m> {
    pub visual_state_selectors: BTreeSet<&'m str>,
    pub persistent_state_selectors: BTreeMap<&'m str, OperationRef<'m>>,
    pub instances: BTreeMap<&'m str, InstanceBindings<'m>>,
    pub property_forwards: BTreeMap<Vec<PropertyEndpoint<'m>>, PropertyForward<'m>>,
    pub callback_forwards: BTreeMap<Vec<PropertyEndpoint<'m>>, CallbackForward<'m>>,
}

/// Public input aliases that refer to the same storage slot.
pub struct PropertyForward<'m> {
    pub target: PropertyEndpoint<'m>,
    pub inputs: Vec<&'m str>,
}

/// Aliases sharing one callback slot, with a callable endpoint in the current scope.
pub struct CallbackForward<'m> {
    pub target: PropertyEndpoint<'m>,
    /// Root callbacks that can supply an override, in declaration order.
    pub inputs: Vec<&'m str>,
    /// The alias body used when there is no incoming override.
    pub implementation: Option<(PropertyEndpoint<'m>, OperationRef<'m>)>,
}

#[derive(Default)]
pub struct InstanceBindings<'m> {
    pub inputs: Vec<InstanceBinding<'m>>,
    pub state_inputs: Vec<InstanceStateInput<'m>>,
    pub callbacks: Vec<InstanceBinding<'m>>,
    pub clipboard_text: Option<OperationRef<'m>>,
}

pub struct InstanceBinding<'m> {
    pub endpoint: Vec<PropertyEndpoint<'m>>,
    pub operation: OperationRef<'m>,
}

pub struct InstanceStateInput<'m> {
    pub name: &'m str,
    pub endpoint: Vec<PropertyEndpoint<'m>>,
    pub cases: Vec<OperationRef<'m>>,
}

impl<'m> ComponentAnalysis<'m> {
    pub fn from_component(
        component: OperationRef<'m>,
        property_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
        callback_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
        use_visual_plan: bool,
    ) -> Result<Self, String> {
        let mut analysis = Self {
            property_forwards: property_forwards(component, property_aliases)?,
            callback_forwards: callback_forwards(component, callback_aliases, use_visual_plan)?,
            ..Self::default()
        };
        analysis.index(
            component,
            property_aliases,
            callback_aliases,
            false,
            use_visual_plan,
            false,
        )?;
        Ok(analysis)
    }

    fn index(
        &mut self,
        element: OperationRef<'m>,
        property_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
        callback_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
        repeated: bool,
        use_visual_plan: bool,
        parent_is_table_row: bool,
    ) -> Result<(), String> {
        let repeat = element.children().find(|op| op.name() == "xr.repeat");
        if use_visual_plan
            && repeat.is_some_and(|op| op.attribute("xr.empty_repeat") == Some("true"))
        {
            return Ok(());
        }
        let repeated = repeated || repeat.is_some();
        if element.name() == "xr.instance" {
            let bindings = instance_bindings(
                element,
                property_aliases,
                callback_aliases,
                parent_is_table_row,
            )?;
            if !bindings.inputs.is_empty()
                || !bindings.state_inputs.is_empty()
                || !bindings.callbacks.is_empty()
                || bindings.clipboard_text.is_some()
            {
                self.instances.insert(element.attr("key")?, bindings);
            }
        }
        let action_presentation = element.children().any(|op| op.name() == "xr.action_ref");
        for op in element.children() {
            if !repeated
                && op.name() == "xr.state_select"
                && op.result(0).is_some_and(|value| {
                    value.uses().any(|(user, index)| {
                        index != 0
                            || !matches!(
                                user.name(),
                                "xr.state_set" | "xr.instance_state" | "xr.state_timing"
                            )
                    })
                })
            {
                self.persistent_state_selectors
                    .insert(op.attr("owner")?, op);
            }
            let visual = matches!(op.name(), "xr.state_set" | "xr.state_timing");
            let persistent = !repeated
                && (op.name() == "xr.instance_state"
                    || op.name() == "xr.state_set"
                        && (action_presentation
                            && matches!(op.attribute("name"), Some("enabled" | "visible"))
                            || matches!(
                                element.attribute("kind"),
                                Some("TableView" | "StandardTableView")
                            ) && matches!(
                                op.attribute("name"),
                                Some("enabled" | "visible" | "pasteable" | "undoable")
                            )));
            if visual || persistent {
                let selector = op
                    .operand(0)
                    .and_then(ValueRef::defining_operation)
                    .ok_or("state case missing selector definition")?;
                let owner = selector.attr("owner")?;
                if visual {
                    self.visual_state_selectors.insert(owner);
                }
                if persistent {
                    self.persistent_state_selectors.insert(owner, selector);
                }
            } else if matches!(op.name(), "xr.element" | "xr.instance") {
                self.index(
                    op,
                    property_aliases,
                    callback_aliases,
                    repeated,
                    use_visual_plan,
                    element.attribute("kind") == Some("TableRow"),
                )?;
            }
        }
        Ok(())
    }
}

fn property_forwards<'m>(
    component: OperationRef<'m>,
    graphs: &BTreeMap<&'m str, AliasGraph<'m>>,
) -> Result<BTreeMap<Vec<PropertyEndpoint<'m>>, PropertyForward<'m>>, String> {
    let graph = graphs
        .get(component.attr("sym_name")?)
        .ok_or("missing property alias graph")?;
    let mut forwards = BTreeMap::new();
    for op in component.children().filter(|op| {
        op.name() == "xr.property"
            && op.attribute("alias") == Some("true")
            && matches!(op.attribute("visibility"), Some("in" | "in-out"))
    }) {
        let name = op.attr("name")?;
        let target = graph.resolve(graphs, ("root", name))?;
        forwards
            .entry(graph.storage_path(graphs, target)?)
            .or_insert_with(|| PropertyForward {
                target,
                inputs: Vec::new(),
            })
            .inputs
            .push(name);
    }
    Ok(forwards)
}

fn callback_forwards<'m>(
    component: OperationRef<'m>,
    graphs: &BTreeMap<&'m str, AliasGraph<'m>>,
    use_visual_plan: bool,
) -> Result<BTreeMap<Vec<PropertyEndpoint<'m>>, CallbackForward<'m>>, String> {
    let graph = graphs
        .get(component.attr("sym_name")?)
        .ok_or("missing callback alias graph")?;
    let mut forwards = BTreeMap::new();
    let mut scopes = vec![("root", component)];
    while let Some((owner, scope)) = scopes.pop() {
        for op in scope.children() {
            if op.name() == "xr.element" {
                if !use_visual_plan
                    || !op.children().any(|child| {
                        child.name() == "xr.repeat"
                            && child.attribute("xr.empty_repeat") == Some("true")
                    })
                {
                    let key = op.attribute("key").map_or_else(|| op.attr("id"), Ok)?;
                    scopes.push((key, op));
                }
                continue;
            }
            let implemented = op.attribute("implementation") == Some("true");
            if op.name() != "xr.callback"
                || op.attribute("alias_owner").is_none()
                || (owner != "root" && !implemented)
            {
                continue;
            }
            let name = op.attr("name")?;
            let target = graph.resolve(graphs, (owner, name))?;
            let path = graph.storage_path(graphs, target)?;
            let forward = forwards.entry(path).or_insert_with(|| CallbackForward {
                target,
                inputs: Vec::new(),
                implementation: None,
            });
            if owner == "root" {
                forward.inputs.push(name);
            }
            if implemented
                && forward
                    .implementation
                    .replace(((owner, name), op))
                    .is_some()
            {
                return Err("multiple alias handlers target the same callback".into());
            }
        }
    }
    Ok(forwards)
}

fn instance_bindings<'m>(
    instance: OperationRef<'m>,
    property_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
    callback_aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
    parent_is_table_row: bool,
) -> Result<InstanceBindings<'m>, String> {
    let component = instance.attr("component")?;
    let clipboard_binding = |op: &OperationRef<'m>| {
        parent_is_table_row
            && op.name() == "xr.bind"
            && op.attribute("name") == Some("clipboard-text")
    };
    let clipboard_text = instance.children().find(clipboard_binding);
    let inputs = canonical_bindings(
        component,
        property_aliases,
        instance.children().filter(|op| {
            matches!(op.name(), "xr.bind" | "xr.instance_link") && !clipboard_binding(op)
        }),
        |name| format!("instance binds multiple inputs to the same property at {name}"),
    )?;
    let callbacks = canonical_bindings(
        component,
        callback_aliases,
        instance.children().filter(|op| op.name() == "xr.callback"),
        |_| "multiple instance handlers target the same callback".into(),
    )?;
    let state_inputs = instance_state_inputs(instance, property_aliases, &inputs)?;
    Ok(InstanceBindings {
        inputs,
        state_inputs,
        callbacks,
        clipboard_text,
    })
}

fn canonical_bindings<'m>(
    component: &'m str,
    aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
    operations: impl Iterator<Item = OperationRef<'m>>,
    duplicate: impl Fn(&str) -> String,
) -> Result<Vec<InstanceBinding<'m>>, String> {
    let mut targets = BTreeSet::new();
    operations
        .map(|operation| {
            let name = operation.attr("name")?;
            let endpoint = canonical_input(aliases, component, name)?;
            if !targets.insert(endpoint.clone()) {
                return Err(duplicate(name));
            }
            Ok(InstanceBinding {
                endpoint,
                operation,
            })
        })
        .collect()
}

fn instance_state_inputs<'m>(
    instance: OperationRef<'m>,
    aliases: &BTreeMap<&'m str, AliasGraph<'m>>,
    bindings: &[InstanceBinding<'m>],
) -> Result<Vec<InstanceStateInput<'m>>, String> {
    let mut inputs = BTreeMap::<_, Vec<_>>::new();
    for op in instance
        .children()
        .filter(|op| op.name() == "xr.instance_state")
    {
        inputs.entry(op.attr("name")?).or_default().push(op);
    }
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let component = instance.attr("component")?;
    let linked = bindings
        .iter()
        .filter(|binding| binding.operation.name() == "xr.instance_link")
        .map(|binding| &binding.endpoint)
        .collect::<BTreeSet<_>>();
    let mut targets = BTreeSet::new();
    inputs
        .into_iter()
        .map(|(name, cases)| {
            let endpoint = canonical_input(aliases, component, name)?;
            if linked.contains(&endpoint) {
                return Err(format!("state cannot override two-way input `{name}`"));
            }
            if !targets.insert(endpoint.clone()) {
                return Err(format!(
                    "states target multiple aliases of the same component input at `{name}`"
                ));
            }
            Ok(InstanceStateInput {
                name,
                endpoint,
                cases,
            })
        })
        .collect()
}

pub struct StateCaseRef<'m> {
    pub selector: ValueRef<'m>,
    pub value: ValueRef<'m>,
    pub index: i64,
}

impl<'m> StateCaseRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if !matches!(
            op.name(),
            "xr.state_set" | "xr.instance_state" | "xr.state_timing"
        ) {
            return Err(format!("expected state case, found {}", op.name()));
        }
        Ok(Self {
            selector: op.operand(0).ok_or("state case missing selector")?,
            value: op.operand(1).ok_or("state case missing value")?,
            index: op
                .integer_attribute("case_index")
                .ok_or("state case missing an index")?,
        })
    }
}

pub struct TranslationRef<'m> {
    pub id: &'m str,
    pub arguments: Vec<(&'m str, ValueRef<'m>)>,
}

impl<'m> TranslationRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if op.name() != "xr.translate" {
            return Err(format!("expected translation, found {}", op.name()));
        }
        let names = op
            .string_array_attribute("arg_names")
            .ok_or("invalid translation argument names")?;
        if names.len() != op.operand_count() {
            return Err("translation argument count does not match its names".into());
        }
        let arguments = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, op.operand(index).expect("checked argument count")))
            .collect();
        Ok(Self {
            id: op.attr("value")?,
            arguments,
        })
    }
}

pub struct RepeatRef<'m> {
    pub model: ValueRef<'m>,
    pub item: &'m str,
    pub index: Option<&'m str>,
}

impl<'m> RepeatRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if op.name() != "xr.repeat" {
            return Err(format!("expected repeat, found {}", op.name()));
        }
        let index = op.attr("index")?;
        Ok(Self {
            model: op.operand(0).ok_or("repeat missing model")?,
            item: op.attr("item")?,
            index: (!index.is_empty()).then_some(index),
        })
    }
}

pub fn nine_slice_edges(op: OperationRef<'_>) -> Result<[u32; 4], String> {
    let edge = |index| {
        op.operand(index)
            .and_then(|value| value.defining_operation())
            .and_then(|literal| literal.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| "expected a nonnegative nine-slice edge".to_owned())
    };
    Ok([edge(1)?, edge(2)?, edge(3)?, edge(4)?])
}
