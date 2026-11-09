use serde::{Deserialize, Serialize};
use xrack_syntax::ast::{Parameter, RecordDef, Span, Type};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Module {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<RecordDef>,
    pub components: Vec<Component>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub exported: bool,
    pub properties: Vec<Property>,
    pub callbacks: Vec<Callback>,
    pub root: Element,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub visibility: String,
    pub init: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Callback {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub body: Vec<Statement>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Expr>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pure: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Statement {
    Assignment(Assignment),
    Invoke(CallbackCall),
    Navigate(Navigation),
    Timer(TimerEffect),
    Popup(PopupEffect),
    NativeDialogOpen(NativeDialogOpen),
    WindowClose { window_close: Span },
    SecondaryWindowCommand(SecondaryWindowCommand),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecondaryWindowCommand {
    pub secondary_window: String,
    pub command: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeDialogOpen {
    #[serde(alias = "file_dialog")]
    pub dialog: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PopupEffect {
    pub owner: String,
    pub open: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimerEffect {
    pub owner: String,
    pub command: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Navigation {
    pub owner: String,
    pub method: String,
    pub page: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallbackCall {
    pub owner: String,
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub target: PropertyRef,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyRef {
    pub owner: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    pub kind: String,
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
    pub bindings: Vec<Binding>,
    pub handlers: Vec<Callback>,
    pub children: Vec<Element>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Expr {
    pub op: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub ty: Type,
    pub args: Vec<Expr>,
    pub span: Span,
}

impl Expr {
    pub(crate) fn operation(op: &str, ty: Type, args: Vec<Self>, span: Span) -> Self {
        Self {
            op: op.into(),
            value: String::new(),
            owner: None,
            ty,
            args,
            span,
        }
    }

    pub fn literal(ty: Type, value: impl Into<String>, span: Span) -> Self {
        Self {
            op: "literal".into(),
            value: value.into(),
            owner: None,
            ty,
            args: vec![],
            span,
        }
    }

    pub fn read(property: PropertyRef, ty: Type, span: Span) -> Self {
        Self {
            op: "read".into(),
            value: property.name,
            owner: Some(property.owner),
            ty,
            args: vec![],
            span,
        }
    }
}
