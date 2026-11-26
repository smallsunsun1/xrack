use crate::semantic::{
    LOADER_ASYNC, LOADER_EXTERNAL_STATUS_CALL, LOADER_MOUNTED_CALL, LOADER_REPEAT_ITEM,
    LOADER_SELECTED, LOADER_SOURCE, check_name, is_builtin,
};
use std::collections::BTreeSet;
use xrack_syntax::ast::{
    Binding, Call, Component, Diagnostic, Expr, ExprKind, Member, Property, Repeat, Span, Type,
};

pub(crate) fn elaborate(component: &mut Component) -> Result<(), Diagnostic> {
    elaborate_members(&component.base, &mut component.members, component.span)
}

fn elaborate_members(kind: &str, members: &mut Vec<Member>, span: Span) -> Result<(), Diagnostic> {
    if kind == "Loader" {
        elaborate_loader(members, span)?;
    }
    for member in members {
        if let Member::Element(element) = member {
            elaborate_members(&element.ty, &mut element.members, element.span)?;
        }
    }
    Ok(())
}

fn elaborate_loader(members: &mut Vec<Member>, span: Span) -> Result<(), Diagnostic> {
    let mut active = None;
    let mut source = None;
    let mut source_url = None;
    let mut asynchronous = None;
    let mut ordinary_members = Vec::new();
    for member in std::mem::take(members) {
        match member {
            Member::Binding(binding)
                if matches!(
                    binding.name.as_str(),
                    "active" | "source" | "asynchronous" | "source-url"
                ) =>
            {
                let slot = match binding.name.as_str() {
                    "active" => &mut active,
                    "source" => &mut source,
                    "source-url" => &mut source_url,
                    _ => &mut asynchronous,
                };
                if binding.two_way || slot.is_some() {
                    return Err(error(
                        binding.span,
                        format!("Loader {} accepts one one-way binding", binding.name),
                    ));
                }
                *slot = Some(binding);
            }
            Member::Binding(binding) if binding.name == "status" => {
                return Err(error(binding.span, "Loader status is read-only"));
            }
            Member::Property(property)
                if matches!(
                    property.name.as_str(),
                    "status" | LOADER_SELECTED | LOADER_SOURCE | LOADER_ASYNC
                ) =>
            {
                return Err(error(property.span, "property name is reserved by Loader"));
            }
            Member::Children(span)
            | Member::SlotMarker { span, .. }
            | Member::SlotFill { span, .. } => {
                return Err(error(
                    span,
                    "Loader requires explicitly declared component candidates",
                ));
            }
            other => ordinary_members.push(other),
        }
    }

    let count = ordinary_members
        .iter()
        .filter(|member| matches!(member, Member::Element(_)))
        .count();
    if let Some(url) = source_url {
        if source.is_some() || count != 0 {
            return Err(error(
                url.span,
                "Loader source-url requires a Loader without declared candidates",
            ));
        }
        if let Some(binding) = asynchronous {
            return Err(error(
                binding.span,
                "Loader source-url always loads asynchronously",
            ));
        }
        let span = url.span;
        let url = if let Some(active) = active {
            select(active.value, url.value, string("", span), span)
        } else {
            url.value
        };
        ordinary_members.push(property(LOADER_SOURCE, Type::String, "private", url));
        ordinary_members.push(Member::Binding(Binding {
            name: "source-url".into(),
            value: read(LOADER_SOURCE, span),
            span,
            two_way: false,
        }));
        ordinary_members.push(property(
            "status",
            Type::String,
            "out",
            Expr {
                kind: ExprKind::Call(Call {
                    callee: LOADER_EXTERNAL_STATUS_CALL.into(),
                    arguments: vec![read(LOADER_SOURCE, span)],
                    span,
                }),
                span,
            },
        ));
        *members = ordinary_members;
        return Ok(());
    }

    let mut ids = BTreeSet::new();
    let mut names = Vec::new();
    for member in &mut ordinary_members {
        let Member::Element(candidate) = member else {
            continue;
        };
        let span = candidate.span;
        if is_builtin(&candidate.ty) || candidate.repeat.is_some() || candidate.condition.is_some()
        {
            return Err(error(
                span,
                "Loader candidates must be declared component instances without if or for",
            ));
        }
        let id = candidate.id.take();
        if let Some(id) = &id {
            check_name(id, span)?;
            if !ids.insert(id.clone()) {
                return Err(error(span, format!("duplicate Loader candidate `{id}`")));
            }
        } else if source.is_some() || count > 1 {
            return Err(error(
                span,
                "Loader candidates must be named when using source or multiple candidates",
            ));
        }

        let index = names.len();
        names.push(id);
        // Loader repeat modes: hidden (0), synchronous (1), after a frame (2).
        let mode = if asynchronous.is_some() {
            select(read(LOADER_ASYNC, span), int(2, span), int(1, span), span)
        } else {
            int(1, span)
        };
        candidate.repeat = Some(Repeat {
            item: LOADER_REPEAT_ITEM.into(),
            index: None,
            model: select(
                binary("==", read(LOADER_SELECTED, span), int(index, span), span),
                mode,
                int(0, span),
                span,
            ),
            item_span: Span::default(),
            index_span: Span::default(),
        });
    }

    let empty = literal(Type::Int, "-1".into(), span);
    let invalid = literal(Type::Int, "-2".into(), span);
    let selected = if let Some(source) = source {
        let span = source.value.span;
        ordinary_members.push(property(
            LOADER_SOURCE,
            Type::String,
            "private",
            source.value,
        ));
        names.into_iter().enumerate().rev().fold(
            select(
                binary("==", read(LOADER_SOURCE, span), string("", span), span),
                empty.clone(),
                invalid.clone(),
                span,
            ),
            |next, (index, name)| {
                let Some(name) = name else { return next };
                select(
                    binary("==", read(LOADER_SOURCE, span), string(&name, span), span),
                    int(index, span),
                    next,
                    span,
                )
            },
        )
    } else if count == 0 {
        empty.clone()
    } else {
        int(0, span)
    };
    let selected = if let Some(active) = active {
        select(active.value, selected, empty, span)
    } else {
        selected
    };
    ordinary_members.push(property(LOADER_SELECTED, Type::Int, "private", selected));
    let ready = string("ready", span);
    let valid = if let Some(asynchronous) = asynchronous {
        ordinary_members.push(property(
            LOADER_ASYNC,
            Type::Bool,
            "private",
            asynchronous.value,
        ));
        let mounted = Expr {
            kind: ExprKind::Call(Call {
                callee: LOADER_MOUNTED_CALL.into(),
                arguments: vec![read(LOADER_SELECTED, span)],
                span,
            }),
            span,
        };
        select(
            read(LOADER_ASYNC, span),
            select(mounted, ready.clone(), string("loading", span), span),
            ready,
            span,
        )
    } else {
        ready
    };
    let status = select(
        binary(">=", read(LOADER_SELECTED, span), int(0, span), span),
        valid,
        select(
            binary("==", read(LOADER_SELECTED, span), invalid, span),
            string("error", span),
            string("null", span),
            span,
        ),
        span,
    );
    ordinary_members.push(property("status", Type::String, "out", status));
    *members = ordinary_members;
    Ok(())
}

fn property(name: &str, ty: Type, visibility: &str, value: Expr) -> Member {
    let span = value.span;
    Member::Property(Property {
        mode: xrack_syntax::ast::PropertyMode::Formula,
        name: name.into(),
        ty,
        type_span: span,
        visibility: visibility.into(),
        value: Some(value),
        two_way: false,
        span,
    })
}

fn read(name: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Reference(format!("self.{name}")),
        span,
    }
}

fn select(condition: Expr, yes: Expr, no: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Conditional {
            condition: Box::new(condition),
            yes: Box::new(yes),
            no: Box::new(no),
        },
        span,
    }
}

fn int(value: usize, span: Span) -> Expr {
    literal(Type::Int, value.to_string(), span)
}

fn string(value: &str, span: Span) -> Expr {
    literal(Type::String, format!("\"{value}\""), span)
}

fn literal(ty: Type, value: String, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Literal { value, ty },
        span,
    }
}

fn binary(op: &str, left: Expr, right: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary {
            op: op.into(),
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    }
}

fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        span,
        message: message.into(),
    }
}
