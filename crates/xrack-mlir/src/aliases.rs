//! Resolve property and callback storage shared by the two XR dialect backends.
use crate::{Module, OperationRef};
use std::collections::{BTreeMap, BTreeSet};

pub type PropertyEndpoint<'a> = (&'a str, &'a str);

#[derive(Default)]
pub struct AliasGraphs<'a> {
    pub properties: BTreeMap<&'a str, AliasGraph<'a>>,
    pub callbacks: BTreeMap<&'a str, AliasGraph<'a>>,
}

impl<'a> AliasGraphs<'a> {
    /// Index property and callback endpoints once for all components in a module.
    pub fn from_module(module: &'a Module) -> Result<Self, String> {
        let mut aliases = Self::default();
        for op in module.operations().filter(|op| op.name() == "xr.component") {
            let name = op.attr("sym_name")?;
            for (graphs, callbacks) in [
                (&mut aliases.properties, false),
                (&mut aliases.callbacks, true),
            ] {
                let mut graph = AliasGraph::default();
                graph.index(op, callbacks).map_err(|error| {
                    let span = op.span();
                    let source = op.source_file().unwrap_or("<MLIR>");
                    format!("{source}:{}:{}: {error}", span.line, span.column)
                })?;
                graphs.insert(name, graph);
            }
        }
        Ok(aliases)
    }
}

#[derive(Default)]
pub struct AliasGraph<'a> {
    edges: BTreeMap<PropertyEndpoint<'a>, PropertyEndpoint<'a>>,
    instances: BTreeMap<&'a str, &'a str>,
    controls: BTreeSet<PropertyEndpoint<'a>>,
}

fn property_reference(op: OperationRef<'_>) -> Result<PropertyEndpoint<'_>, String> {
    Ok((op.attribute("owner").unwrap_or("root"), op.attr("value")?))
}

fn element_key(op: OperationRef<'_>) -> Result<&str, String> {
    op.attribute("key").map_or_else(|| op.attr("id"), Ok)
}

impl<'a> AliasGraph<'a> {
    pub fn instance(&self, owner: &str) -> Option<&'a str> {
        self.instances.get(owner).copied()
    }

    pub(crate) fn is_control(&self, endpoint: PropertyEndpoint<'_>) -> bool {
        self.controls.contains(&endpoint)
    }

    /// Expand a resolved endpoint through its child components to its storage slot.
    pub(crate) fn storage_path(
        &self,
        graphs: &BTreeMap<&'a str, AliasGraph<'a>>,
        target: PropertyEndpoint<'a>,
    ) -> Result<Vec<PropertyEndpoint<'a>>, String> {
        let Some(child) = self.instance(target.0) else {
            return Ok(vec![target]);
        };
        let mut path = vec![(target.0, "")];
        path.extend(canonical_input(graphs, child, target.1)?);
        Ok(path)
    }

    pub fn index(&mut self, op: OperationRef<'a>, callbacks: bool) -> Result<(), String> {
        let key = match op.name() {
            "xr.component" => "root",
            "xr.element" => element_key(op)?,
            "xr.instance" => {
                let key = element_key(op)?;
                self.instances.insert(key, op.attr("component")?);
                key
            }
            _ => return Ok(()),
        };
        for member in op.children() {
            if callbacks && member.name() == "xr.callback" {
                if let Some(owner) = member.attribute("alias_owner") {
                    self.edges.insert(
                        (key, member.attr("name")?),
                        (owner, member.attr("alias_name")?),
                    );
                }
            } else if !callbacks
                && (matches!(member.name(), "xr.control_link" | "xr.instance_link")
                    || member.name() == "xr.property" && member.attribute("alias") == Some("true"))
            {
                let target = member
                    .operand(0)
                    .and_then(|operand| operand.defining_operation())
                    .ok_or("property link without a reference")?;
                if matches!(target.name(), "xr.read" | "xr.control_read") {
                    let reference = property_reference(target)?;
                    if target.name() == "xr.control_read" {
                        self.controls.insert(reference);
                    }
                    self.edges.insert((key, member.attr("name")?), reference);
                }
            } else {
                self.index(member, callbacks)?;
            }
        }
        Ok(())
    }

    pub fn resolve<'q>(
        &self,
        graphs: &BTreeMap<&'a str, AliasGraph<'a>>,
        mut endpoint: PropertyEndpoint<'q>,
    ) -> Result<PropertyEndpoint<'q>, String>
    where
        'a: 'q,
    {
        let mut seen = BTreeSet::new();
        loop {
            let target = if let Some(&target) = self.edges.get(&endpoint) {
                Some(target)
            } else if let Some(&component) = self.instances.get(endpoint.0) {
                let mut equivalents = self
                    .edges
                    .range((endpoint.0, "")..)
                    .take_while(|((owner, _), _)| *owner == endpoint.0)
                    .peekable();
                let mut target = None;
                if equivalents.peek().is_some() {
                    let storage = canonical_input(graphs, component, endpoint.1)?;
                    for ((_, name), &destination) in equivalents {
                        if canonical_input(graphs, component, name)? == storage {
                            target = Some(destination);
                            break;
                        }
                    }
                }
                target
            } else {
                None
            };
            let Some(target) = target else {
                return Ok(endpoint);
            };
            if !seen.insert(endpoint) {
                return Err(format!(
                    "cyclic property alias at {}.{}",
                    endpoint.0, endpoint.1
                ));
            }
            endpoint = target;
        }
    }
}

pub fn canonical_input<'a, 'q>(
    graphs: &BTreeMap<&'a str, AliasGraph<'a>>,
    component: &'q str,
    property: &'q str,
) -> Result<Vec<PropertyEndpoint<'q>>, String>
where
    'a: 'q,
{
    canonical_input_with_storage(graphs, component, property).map(|(path, _)| path)
}

/// Resolve a public input to its storage, retaining whether it ends at a control.
pub(crate) fn canonical_input_with_storage<'a, 'q>(
    graphs: &BTreeMap<&'a str, AliasGraph<'a>>,
    mut component: &'q str,
    mut property: &'q str,
) -> Result<(Vec<PropertyEndpoint<'q>>, bool), String>
where
    'a: 'q,
{
    let mut path = Vec::new();
    loop {
        let graph = graphs
            .get(component)
            .ok_or("unknown component alias scope")?;
        let (owner, name) = graph.resolve(graphs, ("root", property))?;
        if let Some(&nested) = graph.instances.get(owner) {
            path.push((owner, ""));
            component = nested;
            property = name;
        } else {
            path.push((owner, name));
            return Ok((path, graph.is_control((owner, name))));
        }
    }
}
