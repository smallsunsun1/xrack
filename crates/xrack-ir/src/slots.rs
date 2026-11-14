use std::collections::{BTreeMap, BTreeSet};

use xrack_syntax::ast::{Component, Diagnostic, Element, Member, PlacementScope, Span};

use crate::semantic::{is_builtin, layout_item_property};

#[derive(Default)]
pub(crate) struct Names {
    pub root: String,
    pub ids: BTreeMap<String, String>,
}

pub(crate) struct PlacedComponent {
    pub component: Component,
    pub scopes: Vec<Names>,
}

type Result<T> = std::result::Result<T, Diagnostic>;

fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        message: message.into(),
    }
}

fn slot(members: &[Member], found: &mut Option<Span>, conditional_or_repeated: bool) -> Result<()> {
    for member in members {
        match member {
            Member::Children(span) => {
                if conditional_or_repeated {
                    return Err(error(
                        *span,
                        "@children requires an unconditional, non-repeated parent",
                    ));
                }
                if found.replace(*span).is_some() {
                    return Err(error(*span, "a component can declare @children only once"));
                }
            }
            Member::Element(element) => slot(
                &element.members,
                found,
                conditional_or_repeated || element.condition.is_some() || element.repeat.is_some(),
            )?,
            _ => {}
        }
    }
    Ok(())
}

/// Resolve child placement before lowering so both backends see the same tree.
/// Names and source positions remain attached to their original lexical scope.
pub(crate) fn place(components: Vec<Component>) -> Result<Vec<PlacedComponent>> {
    let mut definitions = BTreeMap::new();
    for component in &components {
        let mut marker = None;
        slot(&component.members, &mut marker, false)?;
        definitions.insert(component.name.as_str(), (component, marker.is_some()));
    }
    components
        .iter()
        .map(|component| {
            let mut placement = Placement {
                definitions: &definitions,
                scopes: vec![Names {
                    root: "root".into(),
                    ids: BTreeMap::new(),
                }],
                active: Vec::new(),
            };
            let mut component = component.clone();
            component.members = placement.members(component.members, "root", 0, None)?;
            Ok(PlacedComponent {
                component,
                scopes: placement.scopes,
            })
        })
        .collect()
}

struct Placement<'a> {
    definitions: &'a BTreeMap<&'a str, (&'a Component, bool)>,
    scopes: Vec<Names>,
    active: Vec<String>,
}

impl Placement<'_> {
    fn members(
        &mut self,
        members: Vec<Member>,
        parent: &str,
        scope: usize,
        mut supplied: Option<&mut Vec<Member>>,
    ) -> Result<Vec<Member>> {
        let mut result = Vec::new();
        for member in members {
            match member {
                Member::Children(_) => {
                    if let Some(supplied) = &mut supplied {
                        result.append(supplied);
                    }
                }
                Member::Element(element) => {
                    let element = self.element(
                        element,
                        parent,
                        scope,
                        result.len(),
                        supplied.as_deref_mut(),
                    )?;
                    result.push(Member::Element(element));
                }
                other => result.push(other),
            }
        }
        Ok(result)
    }

    fn element(
        &mut self,
        mut element: Element,
        parent: &str,
        scope: usize,
        index: usize,
        supplied: Option<&mut Vec<Member>>,
    ) -> Result<Element> {
        let key = match &element.id {
            Some(id) if scope == 0 => id.clone(),
            Some(id) => {
                let key = format!("__slot_{scope}_{id}");
                if self.scopes[scope]
                    .ids
                    .insert(id.clone(), key.clone())
                    .is_some()
                {
                    return Err(error(element.span, format!("duplicate element id `{id}`")));
                }
                key
            }
            None => format!("__{parent}_{index}"),
        };
        let Some(&(definition, explicit)) = self.definitions.get(element.ty.as_str()) else {
            if !is_builtin(&element.ty) && supplied.is_some() {
                return Err(error(
                    element.span,
                    format!("unknown element or component `{}`", element.ty),
                ));
            }
            element.members = self.members(element.members, &key, scope, supplied)?;
            if scope != 0 {
                element.placement_scope = Some(PlacementScope {
                    key,
                    lexical_scope: scope,
                    lexical_parent: Some(parent.into()),
                    source_component: None,
                    caller: None,
                    caller_members: BTreeSet::new(),
                });
            }
            return Ok(element);
        };

        if definition.global {
            return Err(error(
                element.span,
                "global declarations cannot be instantiated as elements",
            ));
        }
        let has_content = element
            .members
            .iter()
            .any(|member| matches!(member, Member::Element(_) | Member::Children(_)));
        if !has_content {
            if scope != 0 {
                element.placement_scope = Some(PlacementScope {
                    key,
                    lexical_scope: scope,
                    lexical_parent: Some(parent.into()),
                    source_component: None,
                    caller: None,
                    caller_members: BTreeSet::new(),
                });
            }
            return Ok(element);
        }
        if self.active.contains(&element.ty) {
            return Err(error(
                element.span,
                format!("cyclic component placement involving `{}`", element.ty),
            ));
        }

        let mut content = Vec::new();
        let mut overrides = Vec::new();
        for member in element.members {
            match member {
                Member::Element(_) | Member::Children(_) => content.push(member),
                _ => overrides.push(member),
            }
        }
        // Resolve supplied children in the caller's scope, including any forwarded slot.
        // Use a separate key prefix for anonymous content before giving it a physical parent.
        let content_parent = format!("__content_{key}");
        let mut content = self.members(content, &content_parent, scope, supplied)?;
        for (index, child) in content.iter_mut().enumerate() {
            if let Member::Element(child) = child {
                if let Some(placement) = &mut child.placement_scope {
                    if placement.lexical_parent.as_deref() == Some(&content_parent) {
                        placement.lexical_parent = Some(parent.into());
                    }
                    if let Some((_, caller_parent)) = &mut placement.caller
                        && *caller_parent == content_parent
                    {
                        *caller_parent = parent.into();
                    }
                } else {
                    child.placement_scope = Some(PlacementScope {
                        key: child
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("__{content_parent}_{index}")),
                        lexical_scope: scope,
                        lexical_parent: Some(parent.into()),
                        source_component: None,
                        caller: None,
                        caller_members: BTreeSet::new(),
                    });
                }
            }
        }
        let inner = self.scopes.len();
        self.scopes.push(Names {
            root: key.clone(),
            ids: BTreeMap::new(),
        });
        self.active.push(element.ty.clone());
        let mut members =
            self.members(definition.members.clone(), &key, inner, Some(&mut content))?;
        self.active.pop();
        if !explicit {
            members.append(&mut content);
        }
        let source_component = Some(std::mem::replace(&mut element.ty, definition.base.clone()));
        let caller_members = apply_inputs(&mut members, overrides)?;
        element.members = members;
        element.placement_scope = Some(PlacementScope {
            key,
            lexical_scope: inner,
            lexical_parent: None,
            source_component,
            caller: Some((scope, parent.into())),
            caller_members,
        });
        Ok(element)
    }
}

fn apply_inputs(members: &mut Vec<Member>, overrides: Vec<Member>) -> Result<BTreeSet<String>> {
    let mut caller_members = BTreeSet::new();
    for member in overrides {
        let span = member.span();
        let name = match member {
            Member::Binding(binding) => {
                if let Some(property) = members.iter_mut().find_map(|member| match member {
                    Member::Property(property) if property.name == binding.name => Some(property),
                    _ => None,
                }) {
                    if !matches!(property.visibility.as_str(), "in" | "in-out")
                        || binding.two_way && property.visibility != "in-out"
                    {
                        return Err(error(
                            binding.span,
                            format!("cannot bind component input `{}`", binding.name),
                        ));
                    }
                    property.value = Some(binding.value);
                    property.two_way = binding.two_way;
                } else if !binding.two_way && layout_item_property(&binding.name).is_some() {
                    members.retain(|member| !matches!(member, Member::Binding(existing) if existing.name == binding.name));
                    let name = binding.name.clone();
                    members.push(Member::Binding(binding));
                    if !caller_members.insert(name.clone()) {
                        return Err(error(
                            members.last().unwrap().span(),
                            format!("duplicate input `{name}`"),
                        ));
                    }
                    continue;
                } else {
                    return Err(error(
                        binding.span,
                        format!("unknown component input `{}`", binding.name),
                    ));
                }
                binding.name
            }
            Member::Handler(handler) => {
                if !members.iter().any(|member| matches!(member, Member::Callback(callback) if callback.name == handler.name)) {
                    return Err(error(handler.span, format!("unknown component callback `{}`", handler.name)));
                }
                let name = handler.name.clone();
                members.retain(|member| !matches!(member, Member::Handler(existing) if existing.name == handler.name));
                members.push(Member::Handler(handler));
                name
            }
            _ => {
                return Err(error(
                    member.span(),
                    "slotted component instances accept children, inputs, and callback handlers",
                ));
            }
        };
        if !caller_members.insert(name.clone()) {
            return Err(error(span, format!("duplicate input or handler `{name}`")));
        }
    }
    Ok(caller_members)
}
