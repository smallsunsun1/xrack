use crate::{Module, ffi, module::string_ref};
use std::{borrow::Cow, ptr};
use xrack_ir::{
    self as ir,
    semantic::{LoweredMember, for_each_component},
};
use xrack_syntax::ast::{Component, Diagnostic, Document, Span, Type};

type BuildResult<T> = Result<T, Diagnostic>;

struct Builder {
    context: ffi::MlirContext,
    blocks: Vec<(ffi::MlirBlock, Span)>,
    filename: String,
}

impl Builder {
    fn location(&self, span: Span) -> ffi::MlirLocation {
        let coordinate = |value: usize| u32::try_from(value.max(1)).unwrap_or(u32::MAX);
        // The context interns the filename. MLIR borrows it only during the call.
        unsafe {
            ffi::mlirLocationFileLineColGet(
                self.context,
                string_ref(&self.filename),
                coordinate(span.line),
                coordinate(span.column),
            )
        }
    }

    fn result_type(&self, ty: &Type, span: Span) -> BuildResult<ffi::MlirType> {
        let (mnemonic, record) = match ty {
            Type::Record(record) => (Cow::Borrowed("record"), record.as_ref()),
            Type::RecordList(record) => (Cow::Borrowed("recordlist"), record.as_ref()),
            _ => (ty.name(), ""),
        };
        let native = unsafe {
            ffi::xrackMlirGetType(self.context, string_ref(&mnemonic), string_ref(record))
        };
        if native.ptr.is_null() {
            return Err(Diagnostic {
                span,
                message: format!("unknown XR type {mnemonic}"),
            });
        }
        Ok(native)
    }

    fn attr(&self, name: &str, value: ffi::MlirAttribute) -> ffi::MlirNamedAttribute {
        unsafe {
            ffi::mlirNamedAttributeGet(
                ffi::mlirIdentifierGet(self.context, string_ref(name)),
                value,
            )
        }
    }

    fn strings(&self, pairs: &[(&str, &str)]) -> Vec<ffi::MlirNamedAttribute> {
        pairs
            .iter()
            .map(|&(name, value)| {
                self.attr(name, unsafe {
                    ffi::mlirStringAttrGet(self.context, string_ref(value))
                })
            })
            .collect()
    }

    fn emit(
        &self,
        op: &str,
        span: Span,
        operands: &[ffi::MlirValue],
        results: &[ffi::MlirType],
        attrs: &[ffi::MlirNamedAttribute],
    ) -> BuildResult<ffi::MlirOperation> {
        let &(block, _) = self.blocks.last().expect("active lowering block");
        // The generated builder inserts the operation into the current block.
        // All handles belong to the same module context and slices are borrowed for this call.
        let native = unsafe {
            ffi::xrackMlirBuildOp(
                block,
                self.location(span),
                string_ref(op),
                results.len() as isize,
                results.as_ptr(),
                operands.len() as isize,
                operands.as_ptr(),
                attrs.len() as isize,
                attrs.as_ptr(),
            )
        };
        if native.ptr.is_null() {
            return Err(Diagnostic {
                span,
                message: format!("unknown XR operation xr.{op}"),
            });
        }
        Ok(native)
    }

    fn open(&mut self, op: &str, span: Span, attrs: &[ffi::MlirNamedAttribute]) -> BuildResult<()> {
        let scope = self.emit(op, span, &[], &[], attrs)?;
        // ODS creates a single region for component, element, instance, callback and handler.
        // Transfer ownership of the new block to that region immediately.
        let body = unsafe {
            let body = ffi::mlirBlockCreate(0, ptr::null(), ptr::null());
            ffi::mlirRegionAppendOwnedBlock(ffi::mlirOperationGetRegion(scope, 0), body);
            body
        };
        self.blocks.push((body, span));
        Ok(())
    }

    fn close(&mut self) -> BuildResult<()> {
        let &(_, span) = self.blocks.last().expect("active lowering block");
        self.emit("end", span, &[], &[], &[])?;
        self.blocks.pop();
        Ok(())
    }

    fn operands(&self, args: &[ir::Expr]) -> BuildResult<Vec<ffi::MlirValue>> {
        args.iter().map(|arg| self.expr(arg)).collect()
    }

    fn expr(&self, expr: &ir::Expr) -> BuildResult<ffi::MlirValue> {
        let operands = self.operands(&expr.args)?;
        let ty = self.result_type(&expr.ty, expr.span)?;
        let mut attrs = self.strings(&[("value", &expr.value)]);
        if let Some(owner) = &expr.owner {
            attrs.extend(self.strings(&[("owner", owner)]));
        }
        let op = self.emit(&expr.op, expr.span, &operands, &[ty], &attrs)?;
        // All XR expressions in the semantic stream produce exactly one result.
        Ok(unsafe { ffi::mlirOperationGetResult(op, 0) })
    }

    fn consume(
        &self,
        op: &str,
        name: &str,
        expr: &ir::Expr,
        extra: &[(&str, &str)],
        span: Span,
    ) -> BuildResult<()> {
        let input = self.expr(expr)?;
        let mut attrs = self.strings(&[("name", name)]);
        attrs.extend(self.strings(extra));
        self.emit(op, span, &[input], &[], &attrs)?;
        Ok(())
    }

    fn callback(&mut self, op: &str, callback: &ir::Callback) -> BuildResult<()> {
        let parameters = serde_json::to_string(&callback.parameters).unwrap();
        let mut attrs = self.strings(&[("name", &callback.name), ("parameters", &parameters)]);
        if callback.pure {
            attrs.extend(self.strings(&[("pure", "true")]));
        }
        self.open(op, callback.span, &attrs)?;
        for statement in &callback.body {
            match statement {
                ir::Statement::Assignment(s) => self.consume(
                    "assign",
                    &s.target.name,
                    &s.value,
                    &[("owner", &s.target.owner)],
                    s.span,
                )?,
                ir::Statement::Invoke(call) => {
                    let operands = self.operands(&call.args)?;
                    let attrs = self.strings(&[("owner", &call.owner), ("name", &call.name)]);
                    self.emit("invoke", call.span, &operands, &[], &attrs)?;
                }
                ir::Statement::Navigate(navigation) => {
                    let operands = self.operands(navigation.page.as_slice())?;
                    let attrs = self
                        .strings(&[("owner", &navigation.owner), ("method", &navigation.method)]);
                    self.emit("navigate", navigation.span, &operands, &[], &attrs)?;
                }
            }
        }
        if let Some(result) = &callback.result {
            self.consume("return", "result", result, &[], result.span)?;
        }
        self.close()
    }

    fn component(
        &mut self,
        component: &Component,
        members: Vec<LoweredMember<'_>>,
        filename: &str,
    ) -> BuildResult<()> {
        self.filename = component.filename.as_deref().unwrap_or(filename).into();
        let exported = if component.exported { "true" } else { "false" };
        let attrs = self.strings(&[("sym_name", &component.name), ("exported", exported)]);
        self.open("component", component.span, &attrs)?;
        for member in members {
            match member {
                LoweredMember::Property(p) => self.consume(
                    "property",
                    &p.name,
                    &p.init,
                    &[("visibility", &p.visibility)],
                    p.span,
                )?,
                LoweredMember::Callback(cb) => self.callback("callback", &cb)?,
                LoweredMember::BeginElement {
                    kind,
                    id,
                    key,
                    instance,
                    span,
                } => {
                    let mut attrs = self.strings(&[("id", id.unwrap_or_default()), ("key", &key)]);
                    let op = if instance {
                        let target = unsafe {
                            ffi::mlirFlatSymbolRefAttrGet(self.context, string_ref(kind))
                        };
                        attrs.push(self.attr("component", target));
                        "instance"
                    } else {
                        attrs.extend(self.strings(&[("kind", kind)]));
                        "element"
                    };
                    self.open(op, span, &attrs)?;
                }
                LoweredMember::Binding(b) => {
                    self.consume("bind", &b.name, &b.value, &[], b.span)?
                }
                LoweredMember::ActionRef { owner, span } => {
                    self.emit(
                        "action_ref",
                        span,
                        &[],
                        &[],
                        &self.strings(&[("owner", &owner)]),
                    )?;
                }
                LoweredMember::InstanceLayout(b) => {
                    self.consume("layout_bind", &b.name, &b.value, &[], b.span)?
                }
                LoweredMember::ControlLink(b) => {
                    self.consume("control_link", &b.name, &b.value, &[], b.span)?
                }
                LoweredMember::RowLink(b) => {
                    self.consume("row_link", &b.name, &b.value, &[], b.span)?
                }
                LoweredMember::Animation(b) => {
                    self.consume("animate", &b.name, &b.value, &[], b.span)?
                }
                LoweredMember::Condition(value) => {
                    self.consume("show_if", "visible", &value, &[], value.span)?
                }
                LoweredMember::Repeat { model, item, index } => self.consume(
                    "repeat",
                    "model",
                    &model,
                    &[
                        ("item", &item),
                        ("index", index.as_deref().unwrap_or_default()),
                    ],
                    model.span,
                )?,
                LoweredMember::Handler(h) => self.callback("handler", &h)?,
                LoweredMember::EndElement => self.close()?,
            }
        }
        self.close()
    }
}

/// Build an in-memory XR module before native verification or optimization.
pub fn lower_module(doc: &Document, filename: &str) -> Result<Module, Vec<Diagnostic>> {
    let mut module = Module::empty();
    let (context, body) = module.builder_handles();
    let mut builder = Builder {
        context,
        blocks: vec![(body, Span::default())],
        filename: filename.into(),
    };
    for record in &doc.records {
        let fields = record
            .fields
            .iter()
            .map(|field| xrack_syntax::ast::Parameter {
                name: field.name.clone(),
                ty: field.ty.clone(),
            })
            .collect::<Vec<_>>();
        let fields = serde_json::to_string(&fields).unwrap();
        let attrs = builder.strings(&[("sym_name", &record.name), ("fields", &fields)]);
        builder
            .emit("record_decl", record.span, &[], &[], &attrs)
            .map_err(|error| vec![error])?;
    }
    let mut built = Ok(());
    for_each_component(doc, |component, members| {
        if built.is_ok() {
            built = builder.component(component, members, filename);
        }
    })?;
    built.map_err(|error| vec![error])?;
    Ok(module)
}

/// Print the unoptimized native module for inspection.
pub fn lower(doc: &Document, filename: &str) -> Result<String, Vec<Diagnostic>> {
    lower_module(doc, filename).map(|module| module.to_mlir())
}
