//! Compatibility view for `--emit hir` and frontend inspection.
use crate::{
    model as ir,
    semantic::{LoweredMember, for_each_component},
};
use xrack_syntax::ast::{Diagnostic, Document};

#[expect(
    clippy::expect_used,
    reason = "lowering emits element members between matching BeginElement and EndElement records"
)]
fn current_element(elements: &mut [ir::Element]) -> &mut ir::Element {
    elements.last_mut().expect("active lowered element")
}

pub fn analyze(doc: &Document) -> Result<ir::Module, Vec<Diagnostic>> {
    let doc = crate::declare_handles(doc);
    let doc = doc.as_ref();
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
                LoweredMember::Callback(c) => {
                    if let Some(element) = elements.last_mut() {
                        element.callbacks.push(c);
                    } else {
                        callbacks.push(c);
                    }
                }
                LoweredMember::BeginElement {
                    kind,
                    delegate,
                    id,
                    span,
                    ..
                } => elements.push(ir::Element {
                    kind: delegate.unwrap_or(kind).into(),
                    id: id.map(str::to_owned),
                    action: None,
                    action_group: None,
                    button_group: None,
                    span,
                    properties: vec![],
                    bindings: vec![],
                    animations: vec![],
                    states: None,
                    state_bindings: vec![],
                    state_timings: vec![],
                    initial_focus: None,
                    scroll_target: None,
                    split_fill: None,
                    callbacks: vec![],
                    handlers: vec![],
                    clipboard_paste: None,
                    children: vec![],
                }),
                LoweredMember::Binding(b) | LoweredMember::InstanceLayout(b) => {
                    current_element(&mut elements).bindings.push(b)
                }
                LoweredMember::ActionRef { owner, .. } => {
                    current_element(&mut elements).action = Some(owner)
                }
                LoweredMember::ActionGroupRef { membership, .. } => {
                    current_element(&mut elements).action_group = Some(membership);
                }
                LoweredMember::ScrollTarget { target, .. } => {
                    current_element(&mut elements).scroll_target = Some(target)
                }
                LoweredMember::SplitFill { target, .. } => {
                    current_element(&mut elements).split_fill = Some(target);
                }
                LoweredMember::RadioGroupRef { group, .. }
                | LoweredMember::CheckGroupRef { group, .. } => {
                    current_element(&mut elements).button_group = Some(group)
                }
                LoweredMember::ControlLink(b)
                | LoweredMember::InstanceLink(b)
                | LoweredMember::RowLink(b) => current_element(&mut elements).bindings.push(b),
                LoweredMember::Animation(animation) => {
                    current_element(&mut elements).animations.push(animation)
                }
                LoweredMember::StateSelection(state) => {
                    current_element(&mut elements).states = Some(state)
                }
                LoweredMember::StateBinding(binding) => {
                    current_element(&mut elements).state_bindings.push(binding)
                }
                LoweredMember::StateTiming(timing) => {
                    current_element(&mut elements).state_timings.push(timing)
                }
                LoweredMember::InitialFocus { target, .. } => {
                    current_element(&mut elements).initial_focus = Some(target)
                }
                LoweredMember::Condition(value) => {
                    current_element(&mut elements).bindings.push(ir::Binding {
                        name: "if".into(),
                        span: value.span,
                        value,
                    })
                }
                LoweredMember::Repeat { model, .. } => {
                    current_element(&mut elements).bindings.push(ir::Binding {
                        name: "for".into(),
                        span: model.span,
                        value: model,
                    })
                }
                LoweredMember::Handler(h) | LoweredMember::InstanceHandler(h) => {
                    current_element(&mut elements).handlers.push(h)
                }
                LoweredMember::ClipboardPaste(paste) => {
                    current_element(&mut elements).clipboard_paste = Some(paste)
                }
                LoweredMember::EndElement => {
                    #[expect(
                        clippy::expect_used,
                        reason = "each EndElement is paired with an earlier BeginElement"
                    )]
                    let child = elements.pop().expect("active lowered element");
                    if let Some(parent) = elements.last_mut() {
                        parent.children.push(child);
                    } else {
                        root = Some(child);
                    }
                }
            }
        }
        #[expect(
            clippy::expect_used,
            reason = "successful component lowering emits one root element"
        )]
        let root = root.expect("validated component has a root element");
        components.push(ir::Component {
            name: syntax.name.clone(),
            exported: syntax.exported,
            span: syntax.span,
            properties,
            callbacks,
            root,
        });
    })?;
    Ok(ir::Module {
        records: doc.records.clone(),
        components,
    })
}
