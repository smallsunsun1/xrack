use crate::{Callback, Expr, Statement};
use std::collections::{BTreeMap, BTreeSet};
use xrack_syntax::ast::{Diagnostic, Span};

#[derive(Default)]
pub(crate) struct OwnershipChecks {
    pub(crate) inputs: BTreeMap<String, Span>,
    pub(crate) links: Vec<Access>,
    entry: Vec<Access>,
    callbacks: BTreeMap<String, Vec<Access>>,
}

#[derive(Clone)]
pub struct Access<Site = Span> {
    pub endpoint: String,
    pub site: Site,
    pub conditional: bool,
}

impl Access<Span> {
    pub(crate) fn new(endpoint: String, span: Span) -> Self {
        Self {
            endpoint,
            site: span,
            conditional: false,
        }
    }
}

impl OwnershipChecks {
    pub(crate) fn callback(
        &mut self,
        endpoint: Option<String>,
        callback: &Callback,
        local: bool,
        path: &impl Fn(&str, &str) -> String,
    ) {
        if callback.pure || (!local && endpoint.is_none()) {
            return;
        }
        let mut accesses = Vec::new();
        statements(&callback.body, false, path, &mut accesses);
        if let Some(result) = &callback.result {
            expression(result, false, path, &mut accesses);
        }
        if local {
            // Direct assignments are checked even in branches that never execute.
            self.entry.extend(
                accesses
                    .iter()
                    .filter(|access| access.endpoint.ends_with("()"))
                    .cloned(),
            );
            self.entry.extend(
                crate::walk_statements(&callback.body).filter_map(|statement| {
                    let Statement::Assignment(assignment) = statement else {
                        return None;
                    };
                    Some(Access::new(
                        path(&assignment.target.owner, &assignment.target.name),
                        assignment.span,
                    ))
                }),
            );
        }
        if let Some(endpoint) = endpoint {
            self.callbacks.insert(endpoint, accesses);
        }
    }

    pub(crate) fn validate(
        &self,
        terminal: impl Fn(String) -> String,
        source_files: &[String],
    ) -> Result<(), Diagnostic> {
        if self.inputs.is_empty() {
            return Ok(());
        }
        for access in self.links.iter().chain(&self.entry) {
            let endpoint = terminal(access.endpoint.clone());
            if let Some(&binding) = self.inputs.get(&endpoint) {
                return Err(Diagnostic {
                    span: access.site,
                    message: format!(
                        "cannot write read-only property `{}`: its storage is supplied by a one-way input at {}; use `<=>` to share writable storage",
                        display_endpoint(&access.endpoint),
                        location(binding, source_files),
                    ),
                });
            }

            if let Some((effect, binding, conditional)) =
                blocked_write(&endpoint, &self.callbacks, &terminal, |target| {
                    self.inputs.get(target).copied()
                })
            {
                let certainty = if conditional { "may write" } else { "writes" };
                return Err(Diagnostic {
                    span: access.site,
                    message: format!(
                        "callback `{}` {certainty} read-only property `{}` at {}; its storage is supplied by a one-way input at {}; use `<=>` to share writable storage",
                        display_endpoint(&access.endpoint),
                        display_endpoint(&effect.endpoint),
                        location(effect.site, source_files),
                        location(binding, source_files),
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Find a possible write to an input supplied by a one-way binding.
pub fn blocked_write<'a, Site: Copy>(
    entry: &str,
    callbacks: &'a BTreeMap<String, Vec<Access<Site>>>,
    terminal: impl Fn(String) -> String,
    binding: impl Fn(&str) -> Option<Site>,
) -> Option<(&'a Access<Site>, Site, bool)> {
    let mut pending: Vec<_> = callbacks
        .get(entry)?
        .iter()
        .map(|effect| (effect, false))
        .collect();
    let mut visited = BTreeSet::new();
    while let Some((effect, conditional)) = pending.pop() {
        let conditional = conditional || effect.conditional;
        let target = terminal(effect.endpoint.clone());
        if let Some(binding) = binding(&target) {
            return Some((effect, binding, conditional));
        }
        if visited.insert(target.clone())
            && let Some(calls) = callbacks.get(&target)
        {
            pending.extend(calls.iter().map(|call| (call, conditional)));
        }
    }
    None
}

fn statements(
    body: &[Statement],
    conditional: bool,
    path: &impl Fn(&str, &str) -> String,
    accesses: &mut Vec<Access>,
) {
    for statement in body {
        match statement {
            Statement::Assignment(assignment) => {
                expression(&assignment.value, conditional, path, accesses);
                accesses.push(Access {
                    endpoint: path(&assignment.target.owner, &assignment.target.name),
                    site: assignment.span,
                    conditional,
                });
            }
            Statement::Invoke(call) => {
                for arg in &call.args {
                    expression(arg, conditional, path, accesses);
                }
                accesses.push(Access {
                    endpoint: format!("{}()", path(&call.owner, &call.name)),
                    site: call.span,
                    conditional,
                });
            }
            Statement::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                expression(condition, conditional, path, accesses);
                match (condition.op.as_str(), condition.value.as_str()) {
                    ("literal", "true") => statements(then_body, conditional, path, accesses),
                    ("literal", "false") => statements(else_body, conditional, path, accesses),
                    _ => {
                        statements(then_body, true, path, accesses);
                        statements(else_body, true, path, accesses);
                    }
                }
            }
            Statement::For { model, body, .. } => {
                expression(model, conditional, path, accesses);
                statements(body, true, path, accesses);
            }
            Statement::Let { value, .. } | Statement::Evaluate { evaluate: value } => {
                expression(value, conditional, path, accesses);
            }
            _ => {}
        }
    }
}

/// For a lazy operand, return whether its evaluation is conditional.
pub fn operand_reachability(
    op: &str,
    operator: &str,
    condition: Option<bool>,
    index: usize,
) -> Option<bool> {
    let selected = match (op, operator, index) {
        ("select", _, 1) | ("binary", "&&", 1) => true,
        ("select", _, 2) | ("binary", "||", 1) => false,
        ("enum_match", _, 1..) => return Some(true),
        _ => return Some(false),
    };
    match condition {
        Some(value) if selected != value => None,
        _ => Some(condition.is_none()),
    }
}

fn expression(
    value: &Expr,
    conditional: bool,
    path: &impl Fn(&str, &str) -> String,
    accesses: &mut Vec<Access>,
) {
    if value.op == "call"
        && let Some(owner) = &value.owner
    {
        accesses.push(Access {
            endpoint: format!("{}()", path(owner, &value.value)),
            site: value.span,
            conditional,
        });
    }
    let condition = value
        .args
        .first()
        .filter(|argument| argument.op == "literal")
        .and_then(|argument| argument.value.parse().ok());
    for (index, argument) in value.args.iter().enumerate() {
        if let Some(branch) = operand_reachability(&value.op, &value.value, condition, index) {
            expression(argument, conditional || branch, path, accesses);
        }
    }
}

fn display_endpoint(endpoint: &str) -> String {
    endpoint.replace("/root.", ".").replace('/', ".")
}

fn location(span: Span, source_files: &[String]) -> String {
    let position = format!("{}:{}", span.line, span.column);
    span.source
        .checked_sub(1)
        .and_then(|source| source_files.get(source))
        .map_or_else(|| position.clone(), |file| format!("{file}:{position}"))
}
