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
pub(crate) struct Access {
    pub(crate) endpoint: String,
    pub(crate) span: Span,
    conditional: bool,
}

impl Access {
    pub(crate) fn new(endpoint: String, span: Span) -> Self {
        Self {
            endpoint,
            span,
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
        warnings: &mut Vec<Diagnostic>,
    ) -> Result<(), Diagnostic> {
        if self.inputs.is_empty() {
            return Ok(());
        }
        for access in self.links.iter().chain(&self.entry) {
            let endpoint = terminal(access.endpoint.clone());
            if let Some(&binding) = self.inputs.get(&endpoint) {
                return Err(Diagnostic {
                    span: access.span,
                    message: format!(
                        "cannot write read-only property `{}`: its storage is supplied by a one-way input at {}; use `<=>` to share writable storage",
                        display_endpoint(&access.endpoint),
                        location(binding, source_files),
                    ),
                });
            }

            let Some(calls) = self.callbacks.get(&endpoint) else {
                continue;
            };
            let mut pending: Vec<_> = calls.iter().map(|call| (call, false)).collect();
            let mut visited = BTreeSet::new();
            let mut warning = None;
            while let Some((effect, conditional)) = pending.pop() {
                let conditional = conditional || effect.conditional;
                let target = terminal(effect.endpoint.clone());
                if let Some(&binding) = self.inputs.get(&target) {
                    let certainty = if conditional { "may write" } else { "writes" };
                    let diagnostic = Diagnostic {
                        span: access.span,
                        message: format!(
                            "callback `{}` {certainty} read-only property `{}` at {}; its storage is supplied by a one-way input at {}; use `<=>` to share writable storage",
                            display_endpoint(&access.endpoint),
                            display_endpoint(&effect.endpoint),
                            location(effect.span, source_files),
                            location(binding, source_files),
                        ),
                    };
                    if !conditional {
                        return Err(diagnostic);
                    }
                    warning.get_or_insert(diagnostic);
                } else if visited.insert((target.clone(), conditional))
                    && let Some(calls) = self.callbacks.get(&target)
                {
                    pending.extend(calls.iter().map(|call| (call, conditional)));
                }
            }
            if let Some(warning) = warning {
                warnings.push(warning);
            }
        }
        Ok(())
    }
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
                    span: assignment.span,
                    conditional,
                });
            }
            Statement::Invoke(call) => {
                for arg in &call.args {
                    expression(arg, conditional, path, accesses);
                }
                accesses.push(Access {
                    endpoint: format!("{}()", path(&call.owner, &call.name)),
                    span: call.span,
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
            span: value.span,
            conditional,
        });
    }
    for (index, argument) in value.args.iter().enumerate() {
        let conditional = conditional
            || index > 0 && matches!(value.op.as_str(), "select" | "enum_match")
            || index > 0 && value.op == "binary" && matches!(value.value.as_str(), "&&" | "||");
        expression(argument, conditional, path, accesses);
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
