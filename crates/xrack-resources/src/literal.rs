//! Decode the literal values consumed by both XR backends.
use xrack_runtime::{self as runtime, Color, Length, Percent, Value};
use xrack_syntax::ast::Type;

pub enum Literal {
    Int(i64),
    Bool(bool),
    Float(f64),
    Length(Length),
    Percent(Percent),
    String(String),
    Color(Color),
    Enum {
        value: Value,
        rust_variant: &'static str,
    },
    Default(Value),
}

impl Literal {
    pub fn into_value(self) -> Value {
        match self {
            Self::Int(value) => Value::Int(value),
            Self::Bool(value) => Value::Bool(value),
            Self::Float(value) => Value::Float(value),
            Self::Length(value) => Value::Length(value),
            Self::Percent(value) => Value::Percent(value),
            Self::String(value) => Value::String(value),
            Self::Color(value) => Value::Color(value),
            Self::Enum { value, .. } | Self::Default(value) => value,
        }
    }
}

pub fn decode(ty: &Type, spelling: &str) -> Result<Literal, String> {
    use Literal::{Default, Enum};

    macro_rules! enumeration {
        ($kind:ident, $($name:literal => $variant:ident),+ $(,)?) => {
            match spelling {
                $($name => Enum {
                    value: Value::$kind(runtime::$kind::$variant),
                    rust_variant: stringify!($variant),
                },)+
                _ => return Err(format!("unknown {} value {spelling}", ty.name())),
            }
        };
    }
    let number = || {
        xrack_mlir::parse_float_literal(spelling)
            .ok_or_else(|| format!("invalid {} literal `{spelling}`", ty.name()))
    };
    Ok(match ty {
        Type::Int => Literal::Int(spelling.parse::<i64>().map_err(|error| error.to_string())?),
        Type::Bool => Literal::Bool(
            spelling
                .parse::<bool>()
                .map_err(|error| error.to_string())?,
        ),
        Type::Float => Literal::Float(number()?),
        Type::Length => Literal::Length(Length(number()?)),
        Type::Percent => Literal::Percent(Percent(number()? / 100.0)),
        Type::String => {
            Literal::String(serde_json::from_str(spelling).map_err(|error| error.to_string())?)
        }
        Type::Color => Literal::Color(
            Color::parse_hex_literal(spelling)
                .ok_or_else(|| format!("invalid color literal `{spelling}`"))?,
        ),
        Type::KeyEvent if spelling.is_empty() => {
            Default(Value::KeyEvent(runtime::KeyEvent::default()))
        }
        Type::KeyModifiers if spelling.is_empty() => {
            Default(Value::KeyModifiers(runtime::KeyModifiers::default()))
        }
        Type::WheelEvent if spelling.is_empty() => {
            Default(Value::WheelEvent(runtime::WheelEvent::default()))
        }
        Type::PointerEvent if spelling.is_empty() => {
            Default(Value::PointerEvent(runtime::PointerEvent::default()))
        }
        Type::TouchPoint if spelling.is_empty() => {
            Default(Value::TouchPoint(runtime::TouchPoint::default()))
        }
        Type::TouchPoints if spelling.is_empty() => {
            Default(Value::TouchPoints(runtime::TouchPoints::default()))
        }
        Type::StandardButtonKind => enumeration!(StandardButtonKind,
            "ok" => Ok,
            "open" => Open,
            "save" => Save,
            "save-all" => SaveAll,
            "cancel" => Cancel,
            "close" => Close,
            "apply" => Apply,
            "reset" => Reset,
            "restore-defaults" => RestoreDefaults,
            "help" => Help,
            "yes" => Yes,
            "yes-to-all" => YesToAll,
            "no" => No,
            "no-to-all" => NoToAll,
            "abort" => Abort,
            "retry" => Retry,
            "ignore" => Ignore,
            "discard" => Discard,
        ),
        Type::FileDialogMode => enumeration!(FileDialogMode,
            "open-file" => OpenFile, "open-files" => OpenFiles, "save-file" => SaveFile,
            "select-folder" => SelectFolder, "select-folders" => SelectFolders,
        ),
        Type::MessageLevel => enumeration!(MessageLevel,
            "info" => Info, "warning" => Warning, "error" => Error,
        ),
        Type::MessageButtons => enumeration!(MessageButtons,
            "ok" => Ok, "ok-cancel" => OkCancel, "yes-no" => YesNo, "yes-no-cancel" => YesNoCancel,
        ),
        Type::DrawerEdge => enumeration!(DrawerEdge,
            "left" => Left, "right" => Right, "top" => Top, "bottom" => Bottom
        ),
        Type::DialogRole => enumeration!(DialogRole, "accept" => Accept, "reject" => Reject),
        Type::PointerEventKind => enumeration!(PointerEventKind,
            "down" => Down, "move" => Move, "up" => Up, "cancel" => Cancel),
        Type::PointerButton => enumeration!(PointerButton,
            "primary" => Primary, "secondary" => Secondary, "middle" => Middle),
        Type::CursorShape => enumeration!(CursorShape,
            "auto" => Auto, "default" => Default, "pointer" => Pointer, "text" => Text,
            "crosshair" => Crosshair, "move" => Move, "grab" => Grab, "grabbing" => Grabbing,
            "not-allowed" => NotAllowed, "wait" => Wait, "help" => Help,
            "resize-horizontal" => ResizeHorizontal, "resize-vertical" => ResizeVertical,
            "zoom-in" => ZoomIn, "zoom-out" => ZoomOut),
        Type::EventResult => enumeration!(EventResult, "accept" => Accept, "reject" => Reject),
        Type::FocusReason => enumeration!(FocusReason,
            "programmatic" => Programmatic,
            "tab-navigation" => TabNavigation,
            "pointer-click" => PointerClick,
            "popup-activation" => PopupActivation,
            "window-activation" => WindowActivation,
            "accessibility" => Accessibility,
            "layout-change" => LayoutChange,
            "keyboard-navigation" => KeyboardNavigation,
        ),
        Type::CheckState => enumeration!(CheckState,
            "unchecked" => Unchecked, "partially-checked" => PartiallyChecked, "checked" => Checked),
        Type::LayoutAlignment => enumeration!(LayoutAlignment,
            "stretch" => Stretch, "center" => Center, "start" => Start, "end" => End,
            "space-between" => SpaceBetween, "space-around" => SpaceAround, "space-evenly" => SpaceEvenly),
        Type::Orientation => {
            enumeration!(Orientation, "horizontal" => Horizontal, "vertical" => Vertical)
        }
        Type::PopupPlacement => enumeration!(PopupPlacement,
            "below" => Below, "above" => Above, "left" => Left, "right" => Right, "offset" => Offset),
        Type::FlickDirection => enumeration!(FlickDirection,
            "both" => Both, "horizontal" => Horizontal, "vertical" => Vertical),
        Type::FlickBoundsBehavior => enumeration!(FlickBoundsBehavior,
            "stop-at-bounds" => StopAtBounds, "drag-over-bounds" => DragOverBounds,
            "overshoot-bounds" => OvershootBounds, "drag-and-overshoot-bounds" => DragAndOvershootBounds),
        Type::FlickBoundsMovement => enumeration!(FlickBoundsMovement,
            "follow-bounds-behavior" => FollowBoundsBehavior, "stop-at-bounds" => StopAtBounds),
        Type::ScrollBarPolicy => enumeration!(ScrollBarPolicy,
            "as-needed" => AsNeeded, "always-on" => AlwaysOn, "always-off" => AlwaysOff),
        Type::SelectionTarget => enumeration!(SelectionTarget,
            "rows" => Rows, "cells" => Cells, "columns" => Columns),
        Type::ScrollAlignment => enumeration!(ScrollAlignment,
            "beginning" => Beginning, "center" => Center, "end" => End,
            "visible" => Visible, "contain" => Contain),
        Type::ScrollSnapMode => enumeration!(ScrollSnapMode,
            "none" => None, "always" => Always, "on-release" => OnRelease),
        Type::FlexboxLayoutDirection => enumeration!(FlexboxLayoutDirection,
            "row" => Row, "row-reverse" => RowReverse, "column" => Column, "column-reverse" => ColumnReverse),
        Type::FlexboxLayoutWrap => enumeration!(FlexboxLayoutWrap,
            "wrap" => Wrap, "no-wrap" => NoWrap, "wrap-reverse" => WrapReverse),
        Type::CrossAxisAlignment => enumeration!(CrossAxisAlignment,
            "auto" => Auto, "stretch" => Stretch, "start" => Start, "end" => End, "center" => Center),
        Type::TextWrap => {
            enumeration!(TextWrap, "word-wrap" => WordWrap, "no-wrap" => NoWrap, "char-wrap" => CharWrap)
        }
        Type::TextHorizontalAlignment => enumeration!(TextHorizontalAlignment,
            "start" => Start, "end" => End, "left" => Left, "center" => Center, "right" => Right),
        Type::TextVerticalAlignment => enumeration!(TextVerticalAlignment,
            "top" => Top, "center" => Center, "bottom" => Bottom),
        Type::TextOverflow => enumeration!(TextOverflow, "clip" => Clip, "elide" => Elide),
        Type::ColorScheme => {
            enumeration!(ColorScheme, "system" => System, "light" => Light, "dark" => Dark)
        }
        Type::SwipeSide => {
            enumeration!(SwipeSide, "closed" => Closed, "left" => Left, "right" => Right)
        }
        Type::DragAction => {
            enumeration!(DragAction, "none" => None, "copy" => Copy, "move" => Move, "link" => Link)
        }
        Type::DialInputMode => enumeration!(DialInputMode,
            "circular" => Circular, "horizontal" => Horizontal, "vertical" => Vertical),
        Type::InputType => enumeration!(InputType,
            "text" => Text, "password" => Password, "number" => Number, "decimal" => Decimal, "search" => Search),
        Type::LogLevel => enumeration!(LogLevel,
            "trace" => Trace, "debug" => Debug, "info" => Info, "warn" => Warn, "error" => Error),
        Type::ImageFit => enumeration!(ImageFit,
            "fill" => Fill, "contain" => Contain, "cover" => Cover, "preserve" => Preserve),
        Type::ImageTileMode => enumeration!(ImageTileMode,
            "stretch" => Stretch, "repeat" => Repeat, "round" => Round),
        Type::ImageHorizontalAlignment => enumeration!(ImageHorizontalAlignment,
            "left" => Left, "center" => Center, "right" => Right),
        Type::ImageVerticalAlignment => enumeration!(ImageVerticalAlignment,
            "top" => Top, "center" => Center, "bottom" => Bottom),
        Type::ImageRendering => enumeration!(ImageRendering,
            "smooth" => Smooth, "pixelated" => Pixelated),
        Type::KeyEvent
        | Type::KeyModifiers
        | Type::WheelEvent
        | Type::PointerEvent
        | Type::TouchPoint
        | Type::TouchPoints => {
            return Err(format!("{} requires an empty literal spelling", ty.name()));
        }
        Type::Brush
        | Type::GradientStop
        | Type::Image
        | Type::Date
        | Type::Time
        | Type::Font
        | Type::StringList
        | Type::PointerButtons
        | Type::StyledText
        | Type::Record(_)
        | Type::RecordList(_) => return Err(format!("{} does not have literal syntax", ty.name())),
    })
}
