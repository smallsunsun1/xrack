//! ANTLR Rust contexts mapped directly into the owned XRack syntax tree.
use crate::generated::{xracklexer::XrackLexer, xrackparser::*};
use antlr4rust::{
    InputStream,
    common_token_stream::CommonTokenStream,
    error_listener::ErrorListener,
    errors::ANTLRError,
    parser::Parser,
    parser_rule_context::ParserRuleContext,
    recognizer::Recognizer,
    token::Token,
    token_factory::TokenFactory,
    tree::{ParseTree, Tree},
};
use std::{cell::RefCell, rc::Rc};
use xrack_syntax::ast::*;

#[derive(Clone, Default)]
struct Errors(Rc<RefCell<Vec<Diagnostic>>>);

impl<'input, R: Recognizer<'input>> ErrorListener<'input, R> for Errors {
    fn syntax_error(
        &self,
        _: &R,
        _: Option<&<R::TF as TokenFactory<'input>>::Inner>,
        line: isize,
        column: isize,
        message: &str,
        _: Option<&ANTLRError>,
    ) {
        self.0.borrow_mut().push(Diagnostic {
            span: Span {
                line: line as usize,
                column: (column + 1) as usize,
            },
            message: message.to_owned(),
        });
    }
}

pub fn parse(source: &str) -> Result<Document, Vec<Diagnostic>> {
    // A UTF-8 BOM does not count towards source columns.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let errors = Errors::default();
    let mut lexer = XrackLexer::new(InputStream::new(source));
    lexer.remove_error_listeners();
    lexer.add_error_listener(Box::new(errors.clone()));
    let mut parser = XrackParser::new(CommonTokenStream::new(lexer));
    parser.remove_error_listeners();
    parser.add_error_listener(Box::new(errors.clone()));

    let tree = parser.document();
    let diagnostics = errors.0.take();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let tree = tree.map_err(|error| {
        vec![Diagnostic {
            span: Span { line: 1, column: 1 },
            message: error.to_string(),
        }]
    })?;
    // A recovered tree may lack required children. Only map a clean parse.
    Ok(document(&tree))
}

fn span<'input>(ctx: &impl ParserRuleContext<'input>) -> Span {
    let token = ctx.start();
    Span {
        line: token.get_line() as usize,
        column: (token.get_column() + 1) as usize,
    }
}

fn child_text<'input>(ctx: &impl ParserRuleContext<'input>, index: usize) -> String {
    ctx.get_child(index).expect("grammar child").get_text()
}

fn starts_with<'input>(ctx: &impl ParserRuleContext<'input>, text: &str) -> bool {
    child_text(ctx, 0) == text
}

fn document(ctx: &DocumentContextAll<'_>) -> Document {
    Document {
        imports: ctx
            .importDecl_all()
            .iter()
            .map(|import| Import {
                path: import.STRING().expect("import path").get_text(),
                names: import
                    .importName_all()
                    .iter()
                    .map(|item| ImportName {
                        name: item.identifier(0).expect("import name").get_text(),
                        alias: item.identifier(1).map(|id| id.get_text()),
                    })
                    .collect(),
                span: span(&**import),
            })
            .collect(),
        fonts: ctx
            .fontImport_all()
            .iter()
            .map(|font| FontImport {
                path: font.STRING().expect("font path").get_text(),
                span: span(&**font),
                filename: None,
            })
            .collect(),
        records: ctx
            .recordDecl_all()
            .iter()
            .map(|record| RecordDef {
                name: record.identifier().expect("record name").get_text(),
                exported: starts_with(&**record, "export"),
                fields: record
                    .recordField_all()
                    .iter()
                    .map(|field| RecordField {
                        name: field.identifier().expect("field name").get_text(),
                        ty: Type::from_source(&field.typeName().expect("field type").get_text()),
                        span: span(&**field),
                    })
                    .collect(),
                span: span(&**record),
            })
            .collect(),
        components: ctx
            .component_all()
            .iter()
            .map(|component| Component {
                name: component.identifier(0).expect("component name").get_text(),
                base: component.identifier(1).expect("component base").get_text(),
                exported: starts_with(&**component, "export"),
                members: members(component.member_all()),
                span: span(&**component),
                filename: None,
            })
            .collect(),
    }
}

fn members(contexts: Vec<Rc<MemberContextAll<'_>>>) -> Vec<Member> {
    contexts.iter().map(|ctx| member(ctx)).collect()
}

fn member(ctx: &MemberContextAll<'_>) -> Member {
    if let Some(property) = ctx.property() {
        Member::Property(Property {
            name: property.identifier().expect("property name").get_text(),
            ty: Type::from_source(&property.typeName().expect("property type").get_text()),
            visibility: property
                .visibility()
                .map_or_else(|| "private".into(), |v| v.get_text()),
            value: property.expression().as_deref().map(expression),
            span: span(&*property),
        })
    } else if let Some(callback) = ctx.callback() {
        Member::Callback(Callback {
            name: callback.identifier().expect("callback name").get_text(),
            parameters: callback
                .parameter_all()
                .iter()
                .map(|parameter| Parameter {
                    name: parameter
                        .identifier()
                        .map_or_else(String::new, |id| id.get_text()),
                    ty: Type::from_source(
                        &parameter.typeName().expect("parameter type").get_text(),
                    ),
                })
                .collect(),
            span: span(&*callback),
            return_type: callback
                .typeName()
                .map(|ty| Type::from_source(&ty.get_text())),
            pure: starts_with(&*callback, "pure"),
        })
    } else if let Some(binding) = ctx.binding() {
        let reference = binding.reference();
        let value = match &reference {
            Some(reference) => reference_expr(reference),
            None => expression(&binding.expression().expect("binding expression")),
        };
        Member::Binding(Binding {
            name: binding.identifier().expect("binding name").get_text(),
            value,
            span: span(&*binding),
            two_way: reference.is_some(),
        })
    } else if let Some(handler) = ctx.handler() {
        Member::Handler(Handler {
            name: handler.identifier(0).expect("handler name").get_text(),
            parameters: handler
                .identifier_all()
                .into_iter()
                .skip(1)
                .map(|id| id.get_text())
                .collect(),
            statements: handler
                .statement_all()
                .iter()
                .map(|ctx| statement(ctx))
                .collect(),
            span: span(&*handler),
            result: handler.expression().as_deref().map(expression),
        })
    } else if let Some(element) = ctx.element() {
        let first = element
            .identifier(0)
            .expect("element kind or id")
            .get_text();
        let (id, ty) = match element.identifier(1) {
            Some(kind) => (Some(first), kind.get_text()),
            None => (None, first),
        };
        Member::Element(Element {
            id,
            ty,
            members: members(element.member_all()),
            span: span(&*element),
            condition: element.conditionClause().map(|condition| {
                expression(&condition.expression().expect("condition expression"))
            }),
            repeat: element.repeatClause().map(|repeat| Repeat {
                item: repeat.identifier(0).expect("repeat item").get_text(),
                index: repeat.identifier(1).map(|id| id.get_text()),
                model: expression(&repeat.expression().expect("repeat model")),
            }),
        })
    } else {
        let animation = ctx.animation().expect("member alternative");
        Member::Animation(Animation {
            property: animation
                .identifier()
                .expect("animated property")
                .get_text(),
            duration_ms: animation.NUMBER().expect("animation duration").get_text(),
            span: span(&*animation),
        })
    }
}

fn statement(ctx: &StatementContextAll<'_>) -> Statement {
    if let Some(invocation) = ctx.call() {
        Statement::Call(call(&invocation))
    } else {
        Statement::Assignment(Assignment {
            target: ctx.reference().expect("assignment target").get_text(),
            op: ctx.assignmentOp().expect("assignment operator").get_text(),
            value: expression(&ctx.expression().expect("assignment value")),
            span: span(ctx),
        })
    }
}

fn call(ctx: &CallContextAll<'_>) -> Call {
    Call {
        callee: ctx.reference().expect("callee").get_text(),
        arguments: ctx
            .expression_all()
            .iter()
            .map(|arg| expression(arg))
            .collect(),
        span: span(ctx),
    }
}

fn reference_expr(ctx: &ReferenceContextAll<'_>) -> Expr {
    Expr {
        kind: ExprKind::Reference(ctx.get_text()),
        span: span(ctx),
    }
}

fn expression(ctx: &ExpressionContextAll<'_>) -> Expr {
    let ctx = ctx.conditional().expect("conditional expression");
    let condition = logical_or(&ctx.logicalOr().expect("logical expression"));
    let Some(yes) = ctx.expression(0) else {
        return condition;
    };
    let no = ctx.expression(1).expect("conditional alternative");
    Expr {
        kind: ExprKind::Conditional {
            condition: Box::new(condition),
            yes: Box::new(expression(&yes)),
            no: Box::new(expression(&no)),
        },
        span: span(&*ctx),
    }
}

fn binary_chain<'input>(
    ctx: &impl ParserRuleContext<'input>,
    mut operands: impl Iterator<Item = Expr>,
) -> Expr {
    let first = operands.next().expect("left operand");
    operands.enumerate().fold(first, |left, (i, right)| Expr {
        kind: ExprKind::Binary {
            op: child_text(ctx, 2 * i + 1),
            left: Box::new(left),
            right: Box::new(right),
        },
        span: span(ctx),
    })
}

fn logical_or(ctx: &LogicalOrContextAll<'_>) -> Expr {
    binary_chain(
        ctx,
        ctx.logicalAnd_all().iter().map(|child| logical_and(child)),
    )
}
fn logical_and(ctx: &LogicalAndContextAll<'_>) -> Expr {
    binary_chain(ctx, ctx.equality_all().iter().map(|child| equality(child)))
}
fn equality(ctx: &EqualityContextAll<'_>) -> Expr {
    binary_chain(
        ctx,
        ctx.comparison_all().iter().map(|child| comparison(child)),
    )
}
fn comparison(ctx: &ComparisonContextAll<'_>) -> Expr {
    binary_chain(ctx, ctx.additive_all().iter().map(|child| additive(child)))
}
fn additive(ctx: &AdditiveContextAll<'_>) -> Expr {
    binary_chain(
        ctx,
        ctx.multiplicative_all()
            .iter()
            .map(|child| multiplicative(child)),
    )
}
fn multiplicative(ctx: &MultiplicativeContextAll<'_>) -> Expr {
    binary_chain(ctx, ctx.unary_all().iter().map(|child| unary(child)))
}

fn unary(ctx: &UnaryContextAll<'_>) -> Expr {
    if let Some(primary) = ctx.primary() {
        return primary_expr(&primary);
    }
    Expr {
        kind: ExprKind::Unary {
            op: child_text(ctx, 0),
            value: Box::new(unary(&ctx.unary().expect("unary operand"))),
        },
        span: span(ctx),
    }
}

fn primary_expr(ctx: &PrimaryContextAll<'_>) -> Expr {
    if let Some(inner) = ctx.expression() {
        return expression(&inner);
    }
    if let Some(reference) = ctx.reference() {
        return reference_expr(&reference);
    }
    if let Some(gradient) = ctx.gradient() {
        return gradient_expr(&gradient);
    }
    let kind = if let Some(invocation) = ctx.call() {
        ExprKind::Call(call(&invocation))
    } else if let Some(url) = ctx.imageUrl() {
        ExprKind::ImageUrl(url.STRING().expect("image URL").get_text())
    } else if let Some(array) = ctx.arrayLiteral() {
        ExprKind::Array(
            array
                .expression_all()
                .iter()
                .map(|item| expression(item))
                .collect(),
        )
    } else if let Some(record) = ctx.recordLiteral() {
        ExprKind::Record {
            name: record.identifier().expect("record type").get_text(),
            fields: record
                .recordValue_all()
                .iter()
                .map(|value| RecordValue {
                    name: value.identifier().expect("record field").get_text(),
                    value: expression(&value.expression().expect("record field value")),
                })
                .collect(),
        }
    } else {
        let value = child_text(ctx, 0);
        let ty = if ctx.STRING().is_some() {
            Type::String
        } else if ctx.COLOR().is_some() {
            Type::Color
        } else if ctx.UNIT().is_some() {
            Type::Length
        } else if ctx.get_child(1).is_some() {
            Type::Percent
        } else if ctx.NUMBER().is_none() {
            Type::Bool
        } else if value.contains(['.', 'e', 'E']) {
            Type::Float
        } else {
            Type::Int
        };
        ExprKind::Literal { value, ty }
    };
    Expr {
        kind,
        span: span(ctx),
    }
}

fn gradient_expr(ctx: &GradientContextAll<'_>) -> Expr {
    let stops = ctx
        .gradientStop_all()
        .iter()
        .map(|stop| GradientStop {
            color: expression(&stop.expression().expect("gradient color")),
            position: stop.NUMBER().map(|position| position.get_text()),
            span: span(&**stop),
        })
        .collect();
    let kind = match ctx.gradientAngle() {
        Some(angle) => ExprKind::LinearGradient {
            angle: angle
                .get_text()
                .strip_suffix("deg")
                .expect("degree suffix")
                .into(),
            stops,
        },
        None => ExprKind::RadialGradient { stops },
    };
    Expr {
        kind,
        span: span(ctx),
    }
}
