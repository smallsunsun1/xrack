//! Decode visual operations shared by the code generator and interpreter.
use crate::{
    AliasGraph, AliasGraphs, Module, OperationRef, PropertyEndpoint, ValueRef, canonical_input,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use xrack_ir::semantic::{
    animation_target_error, builtin_event, builtin_property, control_binding_conflict,
    dependency_cycle, event_parameters, event_return_type, is_builtin, is_reserved_identifier,
    qualify_global_dependency, relative_size, rust_name, state_property, visual_state_target,
};
use xrack_syntax::ast::{Parameter, Type};

/// Check element interfaces against the same rules used to lower XR source.
pub(crate) fn verify_element_interfaces(module: &Module) -> Result<(), String> {
    fn verify(op: OperationRef<'_>, root: bool) -> Result<(), String> {
        let mut members = BTreeSet::new();
        for child in op.children() {
            if matches!(child.name(), "xr.property" | "xr.callback") {
                let name = declaration_name(child, "name")?;
                if !members.insert(name) {
                    return Err(at(child, "duplicate member after normalizing its name"));
                }
            }
        }
        if matches!(op.name(), "xr.element" | "xr.instance")
            && let Some(key) = op.attribute("key").or_else(|| op.attribute("id"))
            && !key.is_empty()
            && !identifier(&rust_name(key))
        {
            return Err(at(op, &format!("invalid element key `{key}`")));
        }
        if op.name() == "xr.element" {
            let kind = op.attr("kind")?;
            if !is_builtin(kind) {
                return Err(at(op, &format!("unknown element kind `{kind}`")));
            }
            let source_kind = op.attribute("delegate").unwrap_or(kind);
            let record_combo = crate::RecordComboRef::from_operation(op)?.is_some();
            let mut bindings = BTreeSet::new();
            for child in op.children() {
                match child.name() {
                    "xr.handler" => verify_handler(kind, child)?,
                    "xr.bind" | "xr.control_link" | "xr.row_link" => {
                        bindings.insert(child.attr("name")?);
                        if let Some(message) =
                            control_binding_conflict(kind, |name| bindings.contains(name))
                        {
                            return Err(at(child, &message));
                        }
                        if child.name() == "xr.bind" {
                            verify_binding(source_kind, child, 0, record_combo)?;
                        }
                    }
                    "xr.state_set" => verify_binding(source_kind, child, 1, record_combo)?,
                    "xr.animate" => {
                        let name = child.attr("name")?;
                        if let Some(message) = animation_target_error(kind, name, root) {
                            return Err(at(child, &message));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut repeated = false;
        for child in op.children() {
            match child.name() {
                "xr.element" | "xr.instance" => verify(child, op.name() == "xr.component")?,
                "xr.callback" => {
                    verify_parameters(child)?;
                }
                "xr.repeat" => {
                    if std::mem::replace(&mut repeated, true) {
                        return Err(at(child, "element has more than one repeat"));
                    }
                    verify_repeat(child)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn verify_repeat(op: OperationRef<'_>) -> Result<(), String> {
        let repeat = RepeatRef::from_operation(op).map_err(|error| at(op, &error))?;
        let item = rust_name(repeat.item);
        if !source_identifier(repeat.item) || is_reserved_identifier(&item) {
            return Err(at(
                op,
                &format!("invalid repeat item name `{}`", repeat.item),
            ));
        }
        if let Some(index) = repeat.index {
            let normalized = rust_name(index);
            if !source_identifier(index) || is_reserved_identifier(&normalized) {
                return Err(at(op, &format!("invalid repeat index name `{index}`")));
            }
            if normalized == item {
                return Err(at(op, "repeat item and index must have different names"));
            }
        }
        Ok(())
    }

    fn verify_binding(
        kind: &str,
        op: OperationRef<'_>,
        operand: usize,
        record_combo: bool,
    ) -> Result<(), String> {
        let name = op.attr("name")?;
        // Native verification checks the direct TableRow scope.
        let expected = if op.name() == "xr.state_set" {
            if !visual_state_target(kind) {
                return Err(at(
                    op,
                    "state targets must be visual elements or component instances",
                ));
            }
            state_property(kind, name)
                .ok_or_else(|| at(op, &format!("state cannot override `{kind}.{name}`")))?
        } else if name == "clipboard-text" {
            Type::String
        } else {
            builtin_property(kind, name)
                .ok_or_else(|| at(op, &format!("unknown property `{kind}.{name}`")))?
        };
        let actual = op
            .operand(operand)
            .ok_or_else(|| at(op, "missing property value"))?
            .xr_type()
            .map_err(|error| at(op, &error))?;
        // Record roles and their concrete value types are verified natively.
        let record_role = operand == 0
            && (kind == "ComboBox" && record_combo && name == "current-value"
                || name == "model"
                    && matches!(kind, "ComboBox" | "StandardListView" | "StandardTableView")
                    && matches!(actual, Type::RecordList(_)));
        if actual == expected
            || actual == Type::Percent && relative_size(kind, name, &expected)
            || actual == Type::Color && expected == Type::Brush
            || record_role
        {
            return Ok(());
        }
        Err(at(
            op,
            &format!(
                "property `{kind}.{name}` requires {}, found {}",
                expected.name(),
                actual.name()
            ),
        ))
    }

    fn verify_parameters(op: OperationRef<'_>) -> Result<Vec<Parameter>, String> {
        let params = op.parameters().map_err(|error| at(op, &error))?;
        let mut names = BTreeSet::new();
        for param in &params {
            let name = param.name.as_str();
            if !source_identifier(name) {
                return Err(at(op, &format!("invalid parameter name `{name}`")));
            }
            if !names.insert(name) {
                return Err(at(op, &format!("duplicate parameter `{name}`")));
            }
        }
        Ok(params)
    }

    fn verify_handler(kind: &str, handler: OperationRef<'_>) -> Result<(), String> {
        let name = handler.attr("name")?;
        if !builtin_event(kind, name) {
            return Err(at(handler, &format!("unknown event `{kind}.{name}`")));
        }
        let params = verify_parameters(handler)?;
        if (kind, name) != ("ComboBox", "value-selected") {
            let expected = event_parameters(kind, name, None, handler.span())
                .map_err(|error| at(handler, &error.message))?;
            if !params.iter().map(|param| &param.ty).eq(expected) {
                let types: Vec<_> = expected.iter().map(Type::name).collect();
                return Err(at(
                    handler,
                    &format!("event `{kind}.{name}` requires ({})", types.join(", ")),
                ));
            }
        }
        // The native verifier checks the record projection for value-selected.
        let result = handler
            .children()
            .find(|child| child.name() == "xr.return")
            .and_then(|child| child.operand(0))
            .map(|operand| operand.xr_type())
            .transpose()
            .map_err(|error| at(handler, &error))?;
        let expected = event_return_type(kind, name);
        if result != expected {
            return Err(at(
                handler,
                &format!(
                    "event `{kind}.{name}` requires {}",
                    expected.as_ref().map_or_else(
                        || "no result".into(),
                        |ty| { format!("a {} result", ty.name()) }
                    ),
                ),
            ));
        }
        Ok(())
    }

    let mut symbols = BTreeSet::new();
    for op in module.operations() {
        if matches!(op.name(), "xr.component" | "xr.record_decl")
            && !symbols.insert(declaration_name(op, "sym_name")?)
        {
            return Err(at(op, "duplicate symbol after normalizing its name"));
        }
        if op.name() == "xr.component" {
            verify(op, false)?;
        }
    }
    verify_property_and_callback_dependencies(module)
}

fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn source_identifier(name: &str) -> bool {
    identifier(&rust_name(name))
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

fn declaration_name(op: OperationRef<'_>, attr: &str) -> Result<String, String> {
    let name = op.attr(attr)?;
    let normalized = rust_name(name);
    if !source_identifier(name) || is_reserved_identifier(&normalized) {
        return Err(at(op, &format!("invalid declaration name `{name}`")));
    }
    Ok(normalized)
}

fn at(op: OperationRef<'_>, message: &str) -> String {
    let span = op.span();
    format!(
        "{}:{}:{}: {message}",
        op.source_file().unwrap_or("<MLIR>"),
        span.line,
        span.column
    )
}

type DependencyEdges<'m> = BTreeMap<String, (OperationRef<'m>, Vec<String>)>;

fn collect_dependencies<'m>(
    op: OperationRef<'m>,
    qualify: &impl Fn(&str, &str) -> String,
    seen: &mut HashSet<ValueRef<'m>>,
    dependencies: &mut Vec<String>,
) -> Result<(), String> {
    let owner = op.attribute("owner").unwrap_or("root");
    match op.name() {
        "xr.read" => dependencies.push(qualify(owner, op.attr("value")?)),
        "xr.control_read" => {
            dependencies.push(qualify(owner, &format!("@control:{}", op.attr("value")?)))
        }
        "xr.call" => dependencies.push(qualify(owner, &format!("{}()", op.attr("value")?))),
        "xr.invoke" => dependencies.push(qualify(owner, &format!("{}()", op.attr("name")?))),
        _ => {}
    }
    for operand in (0..op.operand_count()).filter_map(|index| op.operand(index)) {
        if seen.insert(operand)
            && let Some(definition) = operand.defining_operation()
        {
            collect_dependencies(definition, qualify, seen, dependencies)?;
        }
    }
    Ok(())
}

fn member_dependencies(
    member: OperationRef<'_>,
    qualify: &impl Fn(&str, &str) -> String,
) -> Result<Vec<String>, String> {
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();
    if member.name() == "xr.callback" {
        // Only expressions used by statements and the return value are evaluated.
        for statement in member.children().filter(|op| op.result_count() == 0) {
            collect_dependencies(statement, qualify, &mut seen, &mut dependencies)?;
        }
    } else {
        collect_dependencies(member, qualify, &mut seen, &mut dependencies)?;
    }
    Ok(dependencies)
}

fn verify_cycles(edges: &DependencyEdges<'_>, description: &str) -> Result<(), String> {
    if let Some(name) = dependency_cycle(edges) {
        return Err(at(
            edges[name].0,
            &format!("cyclic {description} involving `{name}`"),
        ));
    }
    Ok(())
}

fn verify_global_dependencies<'m>(
    components: &BTreeMap<&'m str, OperationRef<'m>>,
) -> Result<(), String> {
    let mut edges = BTreeMap::new();
    for (&name, &component) in components
        .iter()
        .filter(|(_, op)| op.attribute("global") == Some("true"))
    {
        let qualify = |owner: &str, member: &str| {
            qualify_global_dependency(name, &format!("{owner}.{member}"))
        };
        for member in component.children() {
            let (slot, dependencies) = match member.name() {
                "xr.property" => (
                    qualify("root", member.attr("name")?),
                    member_dependencies(member, &qualify)?,
                ),
                "xr.callback" => {
                    let deps = if let Some(owner) = member.attribute("alias_owner") {
                        vec![qualify(owner, &format!("{}()", member.attr("alias_name")?))]
                    } else {
                        member_dependencies(member, &qualify)?
                    };
                    (
                        qualify("root", &format!("{}()", member.attr("name")?)),
                        deps,
                    )
                }
                _ => continue,
            };
            edges.insert(slot, (member, dependencies));
        }
    }
    verify_cycles(&edges, "global binding or callback call")
}

fn verify_property_and_callback_dependencies(module: &Module) -> Result<(), String> {
    let components = module
        .operations()
        .filter(|op| op.name() == "xr.component")
        .map(|op| Ok((op.attr("sym_name")?, op)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    verify_global_dependencies(&components)?;
    let graph = ComponentDependencies {
        components,
        aliases: AliasGraphs::from_module(module)?,
    };
    for (&name, component) in &graph.components {
        // Validate declarations before passes can erase inactive states or repeats.
        // Both backends use this same canonical input and callback analysis.
        ComponentAnalysis::from_component(
            *component,
            &graph.aliases.properties,
            &graph.aliases.callbacks,
            false,
        )
        .map_err(|error| at(*component, &error))?;
        if component.attribute("global") != Some("true") {
            let mut bindings = ComponentBindingEdges::default();
            graph.collect(name, *component, "", "root", false, &mut bindings)?;
            for (slot, (operation, dependencies)) in bindings.state_inputs {
                // An incoming link and a state in a nested component can reach
                // the same storage even when their instance operations differ.
                if bindings
                    .edges
                    .get(&slot)
                    .is_some_and(|(binding, _)| binding.name() == "xr.instance_link")
                {
                    let name = operation.attr("name")?;
                    return Err(at(
                        operation,
                        &format!("state cannot override two-way input `{name}`"),
                    ));
                }
                bindings
                    .edges
                    .entry(slot)
                    .or_insert((operation, Vec::new()))
                    .1
                    .extend(dependencies);
            }
            verify_cycles(&bindings.edges, "property binding or callback call")?;
        }
    }
    Ok(())
}

fn instance_dependency_slot<'m>(
    prefix: &str,
    owner: &str,
    child: &'m str,
    graphs: &BTreeMap<&'m str, AliasGraph<'m>>,
    name: &'m str,
) -> Result<String, String> {
    let (path, control) = crate::aliases::canonical_input_with_storage(graphs, child, name)?;
    let (endpoint, instances) = path.split_last().ok_or("missing instance endpoint")?;
    let mut slot = format!("{prefix}{owner}/");
    for (child, _) in instances {
        slot.push_str(child);
        slot.push('/');
    }
    let namespace = if control { "@control:" } else { "" };
    Ok(format!("{slot}{}.{namespace}{}", endpoint.0, endpoint.1))
}

struct ComponentDependencies<'m> {
    components: BTreeMap<&'m str, OperationRef<'m>>,
    aliases: AliasGraphs<'m>,
}

#[derive(Default)]
struct ComponentBindingEdges<'m> {
    edges: DependencyEdges<'m>,
    state_inputs: DependencyEdges<'m>,
}

impl<'m> ComponentDependencies<'m> {
    fn collect(
        &self,
        component: &str,
        scope: OperationRef<'m>,
        prefix: &str,
        owner: &str,
        parent_is_table_row: bool,
        bindings: &mut ComponentBindingEdges<'m>,
    ) -> Result<(), String> {
        let properties = &self.aliases.properties[component];
        let callbacks = &self.aliases.callbacks[component];
        let input_component = (scope.name() == "xr.instance")
            .then(|| scope.attr("component"))
            .transpose()?;
        let qualify = |owner: &str, member: &str| {
            if properties.instance(owner).is_some() {
                format!("{prefix}{owner}/root.{member}")
            } else {
                format!("{prefix}{owner}.{member}")
            }
        };
        for member in scope.children() {
            match member.name() {
                "xr.element" | "xr.instance" => {
                    let key = if scope.name() == "xr.component" {
                        "root"
                    } else {
                        member.attribute("key").unwrap_or(member.attr("id")?)
                    };
                    if member.name() == "xr.instance" {
                        let child = member.attr("component")?;
                        let child_prefix = format!("{prefix}{key}/");
                        // Instance inputs replace the base values declared by the child.
                        self.collect(
                            child,
                            self.components[child],
                            &child_prefix,
                            "root",
                            false,
                            bindings,
                        )?;
                    }
                    self.collect(
                        component,
                        member,
                        prefix,
                        key,
                        scope.attribute("kind") == Some("TableRow"),
                        bindings,
                    )?;
                    continue;
                }
                "xr.property" | "xr.callback" | "xr.bind" | "xr.control_link"
                | "xr.instance_link" | "xr.instance_state" => {}
                _ => continue,
            }
            let name = member.attr("name")?;
            // Before anonymous keys are assigned, these elements have no
            // addressable property or callback endpoint.
            if owner.is_empty() {
                continue;
            }
            if parent_is_table_row
                && input_component.is_some()
                && member.name() == "xr.bind"
                && name == "clipboard-text"
            {
                continue;
            }
            let mut dependencies = member_dependencies(member, &qualify)?;
            let slot = match member.name() {
                "xr.callback" => {
                    if let Some(child) = input_component {
                        let slot = instance_dependency_slot(
                            prefix,
                            owner,
                            child,
                            &self.aliases.callbacks,
                            name,
                        )?;
                        bindings
                            .edges
                            .entry(format!("{slot}()"))
                            .or_insert((member, Vec::new()))
                            .1
                            .extend(dependencies);
                        continue;
                    }
                    if let Some(target) = member.attribute("alias_owner") {
                        let target_name = member.attr("alias_name")?;
                        if member.attribute("implementation") == Some("true") {
                            let target = callbacks
                                .resolve(&self.aliases.callbacks, (target, target_name))?;
                            let mut slot = qualify(target.0, target.1);
                            if let Some(child) = callbacks.instance(target.0) {
                                slot = instance_dependency_slot(
                                    prefix,
                                    target.0,
                                    child,
                                    &self.aliases.callbacks,
                                    target.1,
                                )?;
                            }
                            // The target declaration can be visited after its alias.
                            bindings
                                .edges
                                .entry(format!("{slot}()"))
                                .or_insert((member, Vec::new()))
                                .1
                                .append(&mut dependencies);
                        }
                        dependencies = vec![qualify(target, &format!("{target_name}()"))];
                    }
                    let slot = qualify(owner, &format!("{name}()"));
                    bindings
                        .edges
                        .entry(slot)
                        .or_insert((member, Vec::new()))
                        .1
                        .extend(dependencies);
                    continue;
                }
                "xr.property" => qualify(owner, name),
                _ if let Some(child) = input_component => {
                    instance_dependency_slot(prefix, owner, child, &self.aliases.properties, name)?
                }
                _ => qualify(owner, &format!("@control:{name}")),
            };
            if member.name() == "xr.instance_state" {
                bindings
                    .state_inputs
                    .entry(slot)
                    .or_insert((member, Vec::new()))
                    .1
                    .extend(dependencies);
            } else {
                bindings.edges.insert(slot, (member, dependencies));
            }
        }
        Ok(())
    }
}

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

/// Determine whether a repeated scene needs state that follows model row keys.
pub fn repeat_uses_row_identity(template: OperationRef<'_>, use_visual_plan: bool) -> bool {
    if use_visual_plan
        && (template.attribute("xr.static_subtree") == Some("true")
            || template.children().any(|op| {
                op.name() == "xr.repeat" && op.attribute("xr.empty_repeat") == Some("true")
            }))
    {
        return false;
    }
    template.name() == "xr.instance"
        || template.attribute("kind") == Some("GroupBox")
        || scene_children(template).any(|child| repeat_uses_row_identity(child, use_visual_plan))
}

/// Return the ID used by the host view APIs, including the anonymous root.
pub fn host_element_id<'a>(key: &str, id: &'a str) -> Option<&'a str> {
    match (key, id) {
        ("root", "") => Some("root"),
        (_, "") => None,
        _ => Some(id),
    }
}

/// Resolve a SplitView's fill pane against its declared scene slots.
pub fn split_fill_slot(view: OperationRef<'_>) -> Result<Option<usize>, String> {
    let Some(fill) = view.children().find(|op| op.name() == "xr.split_fill") else {
        return Ok(None);
    };
    let target = fill.attr("target")?;
    scene_children(view)
        .position(|pane| pane.attribute("id") == Some(target))
        .map(Some)
        .ok_or_else(|| format!("split fill target {target} must be a direct pane"))
}

/// The expression that supplies clipboard text for a direct TableRow cell.
pub enum TableClipboardRef<'m> {
    Value(OperationRef<'m>),
    Text(Option<OperationRef<'m>>),
}

impl<'m> TableClipboardRef<'m> {
    pub fn from_cell(
        cell: OperationRef<'m>,
        instance_text: Option<OperationRef<'m>>,
    ) -> Option<Self> {
        if cell.name() == "xr.instance" {
            return instance_text.map(Self::Value);
        }
        let binding = |name| {
            cell.children()
                .find(|op| op.name() == "xr.bind" && op.attribute("name") == Some(name))
        };
        if let Some(value) = binding("clipboard-text")
            .or_else(|| cell.children().find(|op| op.name() == "xr.row_link"))
        {
            return Some(Self::Value(value));
        }
        if matches!(cell.attribute("kind"), Some("Text" | "TableCell")) {
            let text = binding("text");
            if text.is_some()
                || cell
                    .children()
                    .any(|op| op.name() == "xr.state_set" && op.attribute("name") == Some("text"))
            {
                return Some(Self::Text(text));
            }
        }
        None
    }
}

/// The model field and optional parser used to paste into a TableRow cell.
pub struct TablePasteRef<'m> {
    pub target: ValueRef<'m>,
    pub parser: Option<ClipboardParserRef<'m>>,
}

pub struct ClipboardParserRef<'m> {
    pub parameter: &'m str,
    pub result: ValueRef<'m>,
    /// Scene child indices from the cell to the instance that declares the parser.
    pub path: Vec<usize>,
}

impl<'m> TablePasteRef<'m> {
    pub fn from_cell(cell: OperationRef<'m>) -> Result<Option<Self>, String> {
        let mut path = Vec::new();
        if let Some((instance, parser)) = Self::find_parser(cell, &mut path) {
            let name = parser.attr("target")?;
            let target = instance
                .children()
                .find(|op| op.name() == "xr.instance_link" && op.attribute("name") == Some(name))
                .and_then(|link| link.operand(0))
                .ok_or("missing clipboard target link")?;
            let result = parser
                .children()
                .find(|op| op.name() == "xr.return")
                .and_then(|op| op.operand(0))
                .ok_or("missing clipboard parser result")?;
            return Ok(Some(Self {
                target,
                parser: Some(ClipboardParserRef {
                    parameter: parser.attr("text_parameter")?,
                    result,
                    path,
                }),
            }));
        }
        cell.children()
            .find(|op| op.name() == "xr.row_link")
            .map(|link| {
                Ok(Self {
                    target: link
                        .operand(0)
                        .ok_or("missing clipboard row link operand")?,
                    parser: None,
                })
            })
            .transpose()
    }

    fn find_parser(
        element: OperationRef<'m>,
        path: &mut Vec<usize>,
    ) -> Option<(OperationRef<'m>, OperationRef<'m>)> {
        if let Some(parser) = element
            .children()
            .find(|op| op.name() == "xr.clipboard_paste")
        {
            return Some((element, parser));
        }
        for (index, child) in scene_children(element).enumerate() {
            path.push(index);
            if let Some(found) = Self::find_parser(child, path) {
                return Some(found);
            }
            path.pop();
        }
        None
    }
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
    let static_gradient = use_visual_plan && GradientRef::static_group(definition).is_some();
    (!static_gradient
        && !matches!(
            definition.name(),
            "xr.literal" | "xr.read" | "xr.image_url" | "xr.param"
        ))
    .then_some(definition)
}

pub struct GradientRef<'m> {
    pub angle: Option<ValueRef<'m>>,
    pub stops: Vec<(ValueRef<'m>, ValueRef<'m>)>,
}

impl<'m> GradientRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        let operand = |operation: OperationRef<'m>, index| {
            operation
                .operand(index)
                .ok_or_else(|| format!("{} missing gradient operand {index}", operation.name()))
        };
        let angle = match op.name() {
            "xr.linear_gradient" => Some(operand(op, 0)?),
            "xr.radial_gradient" => None,
            name => return Err(format!("expected gradient, found {name}")),
        };
        let stops = (usize::from(angle.is_some())..op.operand_count())
            .map(|index| {
                let stop = operand(op, index)?
                    .defining_operation()
                    .filter(|operation| operation.name() == "xr.gradient_stop")
                    .ok_or("expected a gradient stop")?;
                Ok((operand(stop, 0)?, operand(stop, 1)?))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { angle, stops })
    }

    pub fn static_group(op: OperationRef<'_>) -> Option<i64> {
        if matches!(op.name(), "xr.linear_gradient" | "xr.radial_gradient")
            && op.attribute("xr.static_gradient") == Some("true")
        {
            op.integer_attribute("xr.static_gradient_group")
        } else {
            None
        }
    }
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

/// A typed StackView or StackPage query. The verifier requires a literal page route.
#[derive(Clone)]
pub enum StackRead {
    CurrentPage,
    Depth,
    CanGoBack,
    PageActive(String),
    PageIndex(String),
}

impl StackRead {
    pub fn from_operation(op: OperationRef<'_>) -> Result<Self, String> {
        if op.name() != "xr.stack_read" {
            return Err(format!("expected stack query, found {}", op.name()));
        }
        let route = || {
            let route = op
                .operand(0)
                .and_then(ValueRef::defining_operation)
                .ok_or("missing StackPage route")?;
            serde_json::from_str(route.attr("value")?).map_err(|error| error.to_string())
        };
        Ok(match op.attr("value")? {
            "current-page" => Self::CurrentPage,
            "depth" => Self::Depth,
            "can-go-back" => Self::CanGoBack,
            "active" => Self::PageActive(route()?),
            "stack-index" => Self::PageIndex(route()?),
            name => return Err(format!("unknown stack property {name}")),
        })
    }
}

/// State read from a table's selection, sort or edit history.
#[derive(Clone, Copy)]
pub enum TableRead {
    CurrentColumn,
    SelectionCount,
    HasSelection,
    SortColumn,
    SortDescending,
    History { redo: bool },
}

impl TableRead {
    pub fn from_operation(op: OperationRef<'_>) -> Result<Self, String> {
        let name = op.attr("value")?;
        Ok(match (op.name(), name) {
            ("xr.table_selection_read", "current-column") => Self::CurrentColumn,
            ("xr.table_selection_read", "selection-count") => Self::SelectionCount,
            ("xr.table_selection_read", "has-selection") => Self::HasSelection,
            ("xr.table_sort_read", "current-sort-column") => Self::SortColumn,
            ("xr.table_sort_read", "current-sort-descending") => Self::SortDescending,
            ("xr.table_history_read", "can-undo") => Self::History { redo: false },
            ("xr.table_history_read", "can-redo") => Self::History { redo: true },
            (operation, _) => return Err(format!("unknown {operation} property {name}")),
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
