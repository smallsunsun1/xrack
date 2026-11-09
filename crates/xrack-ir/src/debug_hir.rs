//! Compatibility view for `--emit hir` and frontend inspection.
use crate::{
    model as ir,
    semantic::{LoweredMember, for_each_component},
};
use xrack_syntax::ast::{Diagnostic, Document};

pub fn analyze(doc: &Document) -> Result<ir::Module, Vec<Diagnostic>> {
    let mut components = Vec::new();
    for_each_component(doc, |syntax, members| {
        let mut properties = Vec::new();
        let mut callbacks = Vec::new();
        let mut elements: Vec<ir::Element> = Vec::new();
        let mut root = None;
        for member in members {
            match member {
                LoweredMember::Property(p) => {
                    if let Some(element) = elements.last_mut() {
                        element.properties.push(p);
                    } else {
                        properties.push(p);
                    }
                }
                LoweredMember::Callback(c) => callbacks.push(c),
                LoweredMember::BeginElement { kind, id, span, .. } => elements.push(ir::Element {
                    kind: kind.into(),
                    id: id.map(str::to_owned),
                    action: None,
                    span,
                    properties: vec![],
                    bindings: vec![],
                    animations: vec![],
                    states: None,
                    state_bindings: vec![],
                    state_timings: vec![],
                    initial_focus: None,
                    handlers: vec![],
                    children: vec![],
                }),
                LoweredMember::Binding(b) | LoweredMember::InstanceLayout(b) => {
                    elements.last_mut().unwrap().bindings.push(b)
                }
                LoweredMember::ActionRef { owner, .. } => {
                    elements.last_mut().unwrap().action = Some(owner)
                }
                LoweredMember::ControlLink(b) | LoweredMember::RowLink(b) => {
                    elements.last_mut().unwrap().bindings.push(b)
                }
                LoweredMember::Animation(animation) => {
                    elements.last_mut().unwrap().animations.push(animation)
                }
                LoweredMember::StateSelection(state) => {
                    elements.last_mut().unwrap().states = Some(state)
                }
                LoweredMember::StateBinding(binding) => {
                    elements.last_mut().unwrap().state_bindings.push(binding)
                }
                LoweredMember::StateTiming(timing) => {
                    elements.last_mut().unwrap().state_timings.push(timing)
                }
                LoweredMember::InitialFocus { target, .. } => {
                    elements.last_mut().unwrap().initial_focus = Some(target)
                }
                LoweredMember::Condition(value) => {
                    elements.last_mut().unwrap().bindings.push(ir::Binding {
                        name: "if".into(),
                        span: value.span,
                        value,
                    })
                }
                LoweredMember::Repeat { model, .. } => {
                    elements.last_mut().unwrap().bindings.push(ir::Binding {
                        name: "for".into(),
                        span: model.span,
                        value: model,
                    })
                }
                LoweredMember::Handler(h) => elements.last_mut().unwrap().handlers.push(h),
                LoweredMember::EndElement => {
                    let child = elements.pop().unwrap();
                    if let Some(parent) = elements.last_mut() {
                        parent.children.push(child);
                    } else {
                        root = Some(child);
                    }
                }
            }
        }
        components.push(ir::Component {
            name: syntax.name.clone(),
            exported: syntax.exported,
            span: syntax.span,
            properties,
            callbacks,
            root: root.expect("validated component has a root element"),
        });
    })?;
    Ok(ir::Module {
        records: doc.records.clone(),
        components,
    })
}
