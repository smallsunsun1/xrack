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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alias: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<CallbackRef>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_handler: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallbackRef {
    pub owner: String,
    pub name: String,
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
    WindowClose {
        window_close: Span,
    },
    WindowCommand(WindowCommand),
    CheckGroupCommand(CheckGroupCommand),
    SecondaryWindowCommand(SecondaryWindowCommand),
    FocusCommand(FocusCommand),
    SwipeCancel {
        swipe_owner: String,
        span: Span,
    },
    TextCommand(TextCommand),
    ScrollCommand(ScrollCommand),
    SourceSelection(SourceSelection),
    TreeCommand(TreeCommand),
    RestoreSplit(RestoreSplit),
    TableSortCommand(TableSortCommand),
    TableSelectionCommand(TableSelectionCommand),
    TableHistoryCommand {
        history_owner: String,
        redo: bool,
        span: Span,
    },
    CancelFlick {
        flick_viewport: String,
        span: Span,
    },
    ReturnToBounds {
        bounds_viewport: String,
        span: Span,
    },
    MoveColumn(MoveTableItem),
    MoveRow(MoveTableItem),
    SetRowHeight(SetRowHeight),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetRowHeight {
    pub row_height_owner: String,
    pub arguments: [Expr; 2],
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoveTableItem {
    pub table_owner: String,
    pub positions: [Expr; 2],
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableSelectionCommand {
    pub selection_owner: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableSortCommand {
    pub sort_owner: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreSplit {
    pub split_owner: String,
    pub state: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeCommand {
    pub tree_owner: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScrollCommand {
    pub viewport: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSelection {
    pub source_view: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FocusCommand {
    pub focus_owner: String,
    pub clear: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextCommand {
    pub owner: String,
    pub method: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecondaryWindowCommand {
    pub secondary_window: String,
    pub command: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowCommand {
    pub window_command: String,
    pub argument: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckGroupCommand {
    pub check_group: String,
    pub state: Expr,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_group: Option<ActionGroupRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button_group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<Property>,
    pub bindings: Vec<Binding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<Animation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub states: Option<StateSelection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_bindings: Vec<StateBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_timings: Vec<StateTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_fill: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callbacks: Vec<Callback>,
    pub handlers: Vec<Callback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard_paste: Option<ClipboardPaste>,
    pub children: Vec<Element>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionGroupRef {
    pub group: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipboardPaste {
    pub target: String,
    pub text_parameter: String,
    pub result: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Animation {
    pub property: String,
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub curve: Option<[f64; 4]>,
    pub enabled: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSelection {
    pub owner: String,
    pub cases: Vec<(String, Expr)>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateBinding {
    pub owner: String,
    pub case: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub instance_input: bool,
    pub binding: Binding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateTiming {
    pub owner: String,
    pub case: i64,
    pub direction: String,
    pub animation: Animation,
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
