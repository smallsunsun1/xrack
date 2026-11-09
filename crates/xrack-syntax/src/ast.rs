use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.span.line, self.span.column, self.message
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<Import>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<FontImport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<RecordDef>,
    pub components: Vec<Component>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordDef {
    pub name: String,
    pub exported: bool,
    pub fields: Vec<RecordField>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordField {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Import {
    pub path: String,
    pub names: Vec<ImportName>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontImport {
    pub path: String,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportName {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub base: String,
    pub exported: bool,
    pub members: Vec<Member>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Member {
    Property(Property),
    Callback(Callback),
    Binding(Binding),
    Handler(Handler),
    Element(Element),
    Animation(Animation),
    States(StateBlock),
}

impl Member {
    pub fn span(&self) -> Span {
        match self {
            Self::Property(x) => x.span,
            Self::Callback(x) => x.span,
            Self::Binding(x) => x.span,
            Self::Handler(x) => x.span,
            Self::Element(x) => x.span,
            Self::Animation(x) => x.span,
            Self::States(x) => x.span,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Animation {
    pub property: String,
    pub options: Vec<AnimationOption>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimationOption {
    pub name: String,
    pub value: AnimationValue,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnimationValue {
    Milliseconds(String),
    Expression(Expr),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateBlock {
    pub states: Vec<State>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub name: String,
    pub when: Option<Expr>,
    pub changes: Vec<Binding>,
    pub transitions: Vec<StateTransition>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateTransition {
    pub direction: String,
    pub animations: Vec<Animation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    StyledText,
    StringList,
    Record(Box<str>),
    RecordList(Box<str>),
    Length,
    Percent,
    Color,
    Brush,
    Image,
    ImageFit,
    LayoutAlignment,
    CrossAxisAlignment,
    Orientation,
    FlexboxLayoutDirection,
    FlexboxLayoutWrap,
    DialInputMode,
    ScrollBarPolicy,
    TextWrap,
    TextHorizontalAlignment,
    TextVerticalAlignment,
    TextOverflow,
    InputType,
    ColorScheme,
    FileDialogMode,
    MessageLevel,
    MessageButtons,
    CheckState,
    DialogRole,
    StandardButtonKind,
    SwipeSide,
    DragAction,
    KeyEvent,
    KeyModifiers,
    EventResult,
    FocusReason,
    DrawerEdge,
    PopupPlacement,
    SelectionTarget,
    Date,
    Time,
    /// Internal type for a color and its normalized position in a gradient.
    #[doc(hidden)]
    GradientStop,
}

impl Type {
    pub fn key_field(&self, field: &str) -> Option<Self> {
        match (self, field) {
            (Self::KeyEvent, "key" | "text") => Some(Self::String),
            (Self::KeyEvent, "repeat") => Some(Self::Bool),
            (Self::KeyEvent, "modifiers") => Some(Self::KeyModifiers),
            (Self::KeyModifiers, "control" | "shift" | "alt" | "meta" | "primary") => {
                Some(Self::Bool)
            }
            _ => None,
        }
    }
    /// Interpret a type name from the source grammar, including unresolved records.
    pub fn from_source(spelling: &str) -> Self {
        Self::parse(spelling).unwrap_or_else(|| {
            match spelling.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                Some(name) => Self::RecordList(name.into()),
                None => Self::Record(spelling.into()),
            }
        })
    }

    pub fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::StyledText => "styledtext",
            Self::StringList => "stringlist",
            Self::Record(name) => return Cow::Owned(format!("record<\"{name}\">")),
            Self::RecordList(name) => return Cow::Owned(format!("recordlist<\"{name}\">")),
            Self::Length => "length",
            Self::Percent => "percent",
            Self::Color => "color",
            Self::Brush => "brush",
            Self::Image => "image",
            Self::ImageFit => "imagefit",
            Self::LayoutAlignment => "layoutalignment",
            Self::CrossAxisAlignment => "crossaxisalignment",
            Self::Orientation => "orientation",
            Self::FlexboxLayoutDirection => "flexboxlayoutdirection",
            Self::FlexboxLayoutWrap => "flexboxlayoutwrap",
            Self::DialInputMode => "dialinputmode",
            Self::ScrollBarPolicy => "scrollbarpolicy",
            Self::TextWrap => "textwrap",
            Self::TextHorizontalAlignment => "texthorizontalalignment",
            Self::TextVerticalAlignment => "textverticalalignment",
            Self::TextOverflow => "textoverflow",
            Self::InputType => "inputtype",
            Self::ColorScheme => "colorscheme",
            Self::FileDialogMode => "filedialogmode",
            Self::MessageLevel => "messagelevel",
            Self::MessageButtons => "messagebuttons",
            Self::CheckState => "checkstate",
            Self::DialogRole => "dialogrole",
            Self::StandardButtonKind => "standardbuttonkind",
            Self::SwipeSide => "swipeside",
            Self::DragAction => "dragaction",
            Self::KeyEvent => "keyevent",
            Self::KeyModifiers => "keymodifiers",
            Self::EventResult => "eventresult",
            Self::FocusReason => "focusreason",
            Self::DrawerEdge => "draweredge",
            Self::PopupPlacement => "popupplacement",
            Self::SelectionTarget => "selectiontarget",
            Self::Date => "date",
            Self::Time => "time",
            Self::GradientStop => "gradientstop",
        })
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" => Self::Bool,
            "string" => Self::String,
            "styled-text" | "styledtext" => Self::StyledText,
            "stringlist" | "[string]" => Self::StringList,
            "length" => Self::Length,
            "percent" => Self::Percent,
            "color" => Self::Color,
            "brush" => Self::Brush,
            "image" => Self::Image,
            "imagefit" | "ImageFit" => Self::ImageFit,
            "layoutalignment" | "LayoutAlignment" => Self::LayoutAlignment,
            "crossaxisalignment" | "CrossAxisAlignment" => Self::CrossAxisAlignment,
            "orientation" | "Orientation" => Self::Orientation,
            "flexboxlayoutdirection" | "FlexboxLayoutDirection" => Self::FlexboxLayoutDirection,
            "flexboxlayoutwrap" | "FlexboxLayoutWrap" => Self::FlexboxLayoutWrap,
            "dialinputmode" | "DialInputMode" => Self::DialInputMode,
            "scrollbarpolicy" | "ScrollBarPolicy" => Self::ScrollBarPolicy,
            "textwrap" | "TextWrap" => Self::TextWrap,
            "texthorizontalalignment" | "TextHorizontalAlignment" => Self::TextHorizontalAlignment,
            "textverticalalignment" | "TextVerticalAlignment" => Self::TextVerticalAlignment,
            "textoverflow" | "TextOverflow" => Self::TextOverflow,
            "inputtype" | "InputType" => Self::InputType,
            "colorscheme" | "ColorScheme" => Self::ColorScheme,
            "filedialogmode" | "FileDialogMode" => Self::FileDialogMode,
            "messagelevel" | "MessageLevel" => Self::MessageLevel,
            "messagebuttons" | "MessageButtons" => Self::MessageButtons,
            "checkstate" | "CheckState" => Self::CheckState,
            "dialogrole" | "DialogRole" => Self::DialogRole,
            "standardbuttonkind" | "StandardButtonKind" => Self::StandardButtonKind,
            "swipeside" | "SwipeSide" => Self::SwipeSide,
            "dragaction" | "DragAction" => Self::DragAction,
            "keyevent" | "KeyEvent" => Self::KeyEvent,
            "keymodifiers" | "KeyModifiers" => Self::KeyModifiers,
            "eventresult" | "EventResult" => Self::EventResult,
            "focusreason" | "FocusReason" => Self::FocusReason,
            "draweredge" | "DrawerEdge" => Self::DrawerEdge,
            "popupplacement" | "PopupPlacement" => Self::PopupPlacement,
            "selectiontarget" | "SelectionTarget" => Self::SelectionTarget,
            "date" | "Date" => Self::Date,
            "time" | "Time" => Self::Time,
            "gradientstop" => Self::GradientStop,
            _ => {
                if let Some(name) = s
                    .strip_prefix("record<\"")
                    .and_then(|s| s.strip_suffix("\">"))
                {
                    Self::Record(name.into())
                } else {
                    let name = s.strip_prefix("recordlist<\"")?.strip_suffix("\">")?;
                    Self::RecordList(name.into())
                }
            }
        })
    }

    pub fn list_item(&self) -> Option<Self> {
        match self {
            Self::StringList => Some(Self::String),
            Self::RecordList(name) => Some(Self::Record(name.clone())),
            _ => None,
        }
    }

    pub fn enum_variants(&self) -> &'static [&'static str] {
        match self {
            Self::ImageFit => &["fill", "contain", "cover", "preserve"],
            Self::LayoutAlignment => &[
                "stretch",
                "center",
                "start",
                "end",
                "space-between",
                "space-around",
                "space-evenly",
            ],
            Self::CrossAxisAlignment => &["auto", "stretch", "start", "end", "center"],
            Self::Orientation => &["horizontal", "vertical"],
            Self::FlexboxLayoutDirection => &["row", "row-reverse", "column", "column-reverse"],
            Self::FlexboxLayoutWrap => &["wrap", "no-wrap", "wrap-reverse"],
            Self::DialInputMode => &["circular", "horizontal", "vertical"],
            Self::ScrollBarPolicy => &["as-needed", "always-on", "always-off"],
            Self::TextWrap => &["word-wrap", "no-wrap", "char-wrap"],
            Self::TextHorizontalAlignment => &["start", "end", "left", "center", "right"],
            Self::TextVerticalAlignment => &["top", "center", "bottom"],
            Self::TextOverflow => &["clip", "elide"],
            Self::InputType => &["text", "password", "number", "decimal", "search"],
            Self::ColorScheme => &["system", "light", "dark"],
            Self::FileDialogMode => &["open-file", "open-files", "save-file", "select-folder"],
            Self::MessageLevel => &["info", "warning", "error"],
            Self::MessageButtons => &["ok", "ok-cancel", "yes-no", "yes-no-cancel"],
            Self::CheckState => &["unchecked", "partially-checked", "checked"],
            Self::DialogRole => &["accept", "reject"],
            Self::SwipeSide => &["closed", "left", "right"],
            Self::DragAction => &["none", "copy", "move", "link"],
            Self::EventResult => &["accept", "reject"],
            Self::FocusReason => &[
                "programmatic",
                "tab-navigation",
                "pointer-click",
                "popup-activation",
                "window-activation",
                "accessibility",
                "layout-change",
                "keyboard-navigation",
            ],
            Self::StandardButtonKind => &[
                "ok",
                "open",
                "save",
                "save-all",
                "cancel",
                "close",
                "apply",
                "reset",
                "restore-defaults",
                "help",
                "yes",
                "yes-to-all",
                "no",
                "no-to-all",
                "abort",
                "retry",
                "ignore",
                "discard",
            ],
            Self::DrawerEdge => &["left", "right", "top", "bottom"],
            Self::PopupPlacement => &["below", "above", "left", "right", "offset"],
            Self::SelectionTarget => &["rows", "cells", "columns"],
            _ => &[],
        }
    }
}

impl Serialize for Type {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Record(name) => serializer.serialize_str(name),
            Self::RecordList(name) => serializer.serialize_str(&format!("[{name}]")),
            Self::StyledText => serializer.serialize_str("styled-text"),
            Self::StringList => serializer.serialize_str("[string]"),
            Self::ImageFit => serializer.serialize_str("ImageFit"),
            Self::LayoutAlignment => serializer.serialize_str("LayoutAlignment"),
            Self::CrossAxisAlignment => serializer.serialize_str("CrossAxisAlignment"),
            Self::Orientation => serializer.serialize_str("Orientation"),
            Self::FlexboxLayoutDirection => serializer.serialize_str("FlexboxLayoutDirection"),
            Self::FlexboxLayoutWrap => serializer.serialize_str("FlexboxLayoutWrap"),
            Self::DialInputMode => serializer.serialize_str("DialInputMode"),
            Self::ScrollBarPolicy => serializer.serialize_str("ScrollBarPolicy"),
            Self::TextWrap => serializer.serialize_str("TextWrap"),
            Self::TextHorizontalAlignment => serializer.serialize_str("TextHorizontalAlignment"),
            Self::TextVerticalAlignment => serializer.serialize_str("TextVerticalAlignment"),
            Self::TextOverflow => serializer.serialize_str("TextOverflow"),
            Self::InputType => serializer.serialize_str("InputType"),
            Self::ColorScheme => serializer.serialize_str("ColorScheme"),
            Self::FileDialogMode => serializer.serialize_str("FileDialogMode"),
            Self::MessageLevel => serializer.serialize_str("MessageLevel"),
            Self::MessageButtons => serializer.serialize_str("MessageButtons"),
            Self::CheckState => serializer.serialize_str("CheckState"),
            Self::DialogRole => serializer.serialize_str("DialogRole"),
            Self::StandardButtonKind => serializer.serialize_str("StandardButtonKind"),
            Self::SwipeSide => serializer.serialize_str("SwipeSide"),
            Self::DragAction => serializer.serialize_str("DragAction"),
            Self::KeyEvent => serializer.serialize_str("KeyEvent"),
            Self::KeyModifiers => serializer.serialize_str("KeyModifiers"),
            Self::EventResult => serializer.serialize_str("EventResult"),
            Self::FocusReason => serializer.serialize_str("FocusReason"),
            Self::DrawerEdge => serializer.serialize_str("DrawerEdge"),
            Self::PopupPlacement => serializer.serialize_str("PopupPlacement"),
            Self::SelectionTarget => serializer.serialize_str("SelectionTarget"),
            Self::Date => serializer.serialize_str("Date"),
            Self::Time => serializer.serialize_str("Time"),
            _ => serializer.serialize_str(&self.name()),
        }
    }
}

impl<'de> Deserialize<'de> for Type {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spelling = String::deserialize(deserializer)?;
        Ok(Self::from_source(&spelling))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub visibility: String,
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Callback {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<Type>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pure: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    pub value: Expr,
    pub span: Span,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub two_way: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Handler {
    pub name: String,
    pub parameters: Vec<String>,
    pub statements: Vec<Statement>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Expr>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Statement {
    Assignment(Assignment),
    Call(Call),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub target: String,
    pub op: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Call {
    pub callee: String,
    pub arguments: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Element {
    pub id: Option<String>,
    pub ty: String,
    pub members: Vec<Member>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repeat {
    pub item: String,
    pub index: Option<String>,
    pub model: Expr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExprKind {
    Literal {
        value: String,
        ty: Type,
    },
    ImageUrl(String),
    Markdown(String),
    Array(Vec<Expr>),
    Record {
        name: String,
        fields: Vec<RecordValue>,
    },
    Call(Call),
    LinearGradient {
        angle: String,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        stops: Vec<GradientStop>,
    },
    Reference(String),
    Unary {
        op: String,
        value: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Conditional {
        condition: Box<Expr>,
        yes: Box<Expr>,
        no: Box<Expr>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordValue {
    pub name: String,
    pub value: Expr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GradientStop {
    pub color: Expr,
    pub position: Option<String>,
    pub span: Span,
}
