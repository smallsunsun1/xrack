use std::borrow::Cow;
use xrack_syntax::ast::{Document, Member, RecordDef, RecordField, Type};

pub(crate) fn handle_type(component: &str) -> String {
    format!("{component}Handle")
}

pub fn declare_handles(document: &Document) -> Cow<'_, Document> {
    fn has_container(members: &[Member]) -> bool {
        members.iter().any(|member| match member {
            Member::Element(element) => {
                element.ty == "DynamicContainer" || has_container(&element.members)
            }
            _ => false,
        })
    }
    if !document
        .components
        .iter()
        .any(|component| component.base == "DynamicContainer" || has_container(&component.members))
    {
        return Cow::Borrowed(document);
    }

    let mut document = document.clone();
    document.records.extend(
        document
            .components
            .iter()
            .filter(|c| !c.global)
            .map(|component| RecordDef {
                name: handle_type(&component.name),
                exported: component.exported,
                variants: Vec::new(),
                fields: vec![RecordField {
                    name: "id".into(),
                    ty: Type::Int,
                    type_span: component.span,
                    span: component.span,
                }],
                span: component.span,
            }),
    );
    document.records.push(RecordDef {
        name: "ExternalComponentHandle".into(),
        exported: true,
        variants: Vec::new(),
        fields: vec![RecordField {
            name: "id".into(),
            ty: Type::Int,
            type_span: Default::default(),
            span: Default::default(),
        }],
        span: Default::default(),
    });
    Cow::Owned(document)
}
