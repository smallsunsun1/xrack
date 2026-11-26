//! Decode the literal values consumed by both XR backends.
use xrack_runtime::{self as runtime, Color, Length, Percent, Value};
use xrack_syntax::ast::Type;

pub enum Literal {
    Int(i64),
    Bool(bool),
    Float(f64),
    Length(Length),
    Point2D(runtime::Point2D),
    Point3D(runtime::Point3D),
    Vector3D(runtime::Vector3D),
    Quaternion3D([f64; 4]),
    RotationTrack3D(Vec<[f64; 5]>),
    Vector2D(runtime::Vector2D),
    Transform2D(runtime::Transform2D),
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
            Self::Point2D(value) => Value::Point2D(value),
            Self::Point3D(value) => Value::Point3D(value),
            Self::Vector3D(value) => Value::Vector3D(value),
            Self::Quaternion3D([x, y, z, w]) => {
                Value::Quaternion3D(runtime::Quaternion3D::new(x, y, z, w))
            }
            Self::RotationTrack3D(keys) => Value::RotationTrack3D(runtime::RotationTrack3D::new(
                keys.into_iter()
                    .map(|[time, x, y, z, w]| (time, runtime::Quaternion3D::new(x, y, z, w)))
                    .collect(),
            )),
            Self::Vector2D(value) => Value::Vector2D(value),
            Self::Transform2D(value) => Value::Transform2D(value),
            Self::Percent(value) => Value::Percent(value),
            Self::String(value) => Value::String(value),
            Self::Color(value) => Value::Color(value),
            Self::Enum { value, .. } | Self::Default(value) => value,
        }
    }
}

fn numbers(spelling: &str, invalid: impl Fn() -> String) -> Result<Vec<f64>, String> {
    spelling
        .split(' ')
        .filter(|part| !part.is_empty())
        .map(|part| xrack_mlir::parse_float_literal(part).ok_or_else(&invalid))
        .collect()
}

fn coordinates<const N: usize>(spelling: &str) -> Result<[f64; N], String> {
    let invalid = || format!("invalid geometry literal `{spelling}`");
    numbers(spelling, invalid)?
        .try_into()
        .map_err(|_| invalid())
}

fn rotation_track(spelling: &str) -> Result<Literal, String> {
    let invalid = || String::from("invalid rotation track literal");
    let components = numbers(spelling, invalid)?;
    let (keys, remainder) = components.as_chunks::<5>();
    if !remainder.is_empty()
        || keys.first().is_some_and(|first| first[0] < 0.0)
        || keys.windows(2).any(|pair| pair[0][0] >= pair[1][0])
    {
        return Err(invalid());
    }
    if keys.is_empty() {
        Ok(Literal::Default(Value::RotationTrack3D(
            runtime::RotationTrack3D::default(),
        )))
    } else {
        Ok(Literal::RotationTrack3D(keys.to_vec()))
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
        Type::Point2D => {
            let [x, y] = coordinates(spelling)?;
            Literal::Point2D(runtime::Point2D::new(Length(x), Length(y)))
        }
        Type::Point3D => {
            let [x, y, z] = coordinates(spelling)?;
            Literal::Point3D(runtime::Point3D::new(x, y, z))
        }
        Type::Vector3D => {
            let [x, y, z] = coordinates(spelling)?;
            Literal::Vector3D(runtime::Vector3D::new(x, y, z))
        }
        Type::Quaternion3D => {
            let [x, y, z, w] = coordinates(spelling)?;
            Literal::Quaternion3D([x, y, z, w])
        }
        Type::PhysicsContact3D => {
            if !spelling.is_empty() {
                return Err("physics contact literals only support the default value".into());
            }
            Default(Value::PhysicsContact3D(runtime::PhysicsContact3D::default()))
        }
        Type::PhysicsHit3D => {
            if !spelling.is_empty() {
                return Err("physics hit literals only support the default value".into());
            }
            Default(Value::PhysicsHit3D(runtime::PhysicsHit3D::default()))
        }
        Type::Hit3D => {
            if !spelling.is_empty() {
                return Err("invalid scene hit literal".into());
            }
            Default(Value::Hit3D(runtime::Hit3D::default()))
        }
        Type::SceneModel3D => {
            if !spelling.is_empty() {
                return Err("invalid scene model literal".into());
            }
            Default(Value::SceneObjects(std::sync::Arc::default()))
        }
        Type::PhysicsWorld3D => {
            if !spelling.is_empty() {
                return Err("invalid physics world literal".into());
            }
            Default(Value::PhysicsWorld3D(runtime::PhysicsWorld3D::default()))
        }
        Type::WalkResult3D => {
            if !spelling.is_empty() {
                return Err("invalid walk result literal".into());
            }
            Default(Value::WalkResult3D(runtime::WalkResult3D::default()))
        }
        Type::RotationTrack3D => rotation_track(spelling)?,
        Type::MeshAsset3D => {
            if !spelling.is_empty() {
                return Err("invalid mesh asset literal".into());
            }
            Default(Value::MeshAsset3D(runtime::MeshAsset3D::default()))
        }
        Type::InstanceTable3D => {
            if !spelling.is_empty() {
                return Err("invalid instance table literal".into());
            }
            Default(Value::InstanceTable3D(runtime::InstanceTable3D::default()))
        }
        Type::Instance3D => {
            if !spelling.is_empty() {
                return Err("invalid 3D instance literal".into());
            }
            Default(Value::Instance3D(runtime::Instance3D::default()))
        }
        Type::ProjectedPoint3D => {
            if !spelling.is_empty() {
                return Err("invalid projected point literal".into());
            }
            Default(Value::ProjectedPoint3D(runtime::ProjectedPoint3D::default()))
        }
        Type::Vector2D => {
            let [x, y] = coordinates(spelling)?;
            Literal::Vector2D(runtime::Vector2D::new(Length(x), Length(y)))
        }
        Type::Transform2D => Literal::Transform2D(runtime::Transform2D::from_coefficients(
            coordinates(spelling)?,
        )),
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
        Type::SelectionGesture => enumeration!(SelectionGesture,
            "auto" => Auto, "drag" => Drag, "press-and-hold" => PressAndHold,
        ),
        Type::SelectionTarget => enumeration!(SelectionTarget,
            "rows" => Rows, "cells" => Cells, "columns" => Columns),
        Type::ScrollAlignment => enumeration!(ScrollAlignment,
            "beginning" => Beginning, "center" => Center, "end" => End,
            "visible" => Visible, "contain" => Contain),
        Type::ScrollSnapMode => enumeration!(ScrollSnapMode,
            "none" => None, "always" => Always, "on-release" => OnRelease),
        Type::LayoutDirection => enumeration!(LayoutDirection,
            "left-to-right" => LeftToRight, "right-to-left" => RightToLeft),
        Type::FlexboxLayoutDirection => enumeration!(FlexboxLayoutDirection,
            "row" => Row, "row-reverse" => RowReverse, "column" => Column, "column-reverse" => ColumnReverse),
        Type::FlexboxLayoutWrap => enumeration!(FlexboxLayoutWrap,
            "wrap" => Wrap, "no-wrap" => NoWrap, "wrap-reverse" => WrapReverse),
        Type::CrossAxisAlignment => enumeration!(CrossAxisAlignment,
            "auto" => Auto, "stretch" => Stretch, "start" => Start, "end" => End, "center" => Center),
        Type::TextWrap => {
            enumeration!(TextWrap, "word-wrap" => WordWrap, "no-wrap" => NoWrap, "char-wrap" => CharWrap)
        }
        Type::InputValidity => enumeration!(InputValidity,
            "invalid" => Invalid, "intermediate" => Intermediate, "acceptable" => Acceptable),
        Type::MouseSelectionMode => enumeration!(MouseSelectionMode,
            "characters" => Characters, "words" => Words, "caret" => Caret),
        Type::TextHorizontalAlignment => enumeration!(TextHorizontalAlignment,
            "start" => Start, "end" => End, "left" => Left, "center" => Center, "right" => Right),
        Type::TextVerticalAlignment => enumeration!(TextVerticalAlignment,
            "top" => Top, "center" => Center, "bottom" => Bottom),
        Type::TextOverflow => enumeration!(TextOverflow, "clip" => Clip, "elide" => Elide),
        Type::ColorScheme => {
            enumeration!(ColorScheme, "system" => System, "light" => Light, "dark" => Dark)
        }
        Type::SceneToneMap => enumeration!(SceneToneMap,
            "none" => None, "linear" => Linear, "reinhard" => Reinhard,
            "aces" => Aces, "hejl-dawson" => HejlDawson, "filmic" => Filmic),
        Type::SwipeSide => {
            enumeration!(SwipeSide, "closed" => Closed, "left" => Left, "right" => Right)
        }
        Type::DragAction => {
            enumeration!(DragAction, "none" => None, "copy" => Copy, "move" => Move, "link" => Link)
        }
        Type::DialInputMode => enumeration!(DialInputMode,
            "circular" => Circular, "horizontal" => Horizontal, "vertical" => Vertical),
        Type::InputType => enumeration!(InputType,
            "text" => Text, "password" => Password, "number" => Number, "decimal" => Decimal,
            "search" => Search, "email" => Email, "url" => Url, "tel" => Tel),
        Type::InputPurpose => enumeration!(InputPurpose,
            "auto" => Auto, "username" => Username, "current-password" => CurrentPassword,
            "new-password" => NewPassword, "one-time-code" => OneTimeCode),
        Type::KeyboardCapitalization => enumeration!(KeyboardCapitalization,
            "auto" => Auto, "none" => None, "words" => Words,
            "sentences" => Sentences, "characters" => Characters),
        Type::ReturnKey => enumeration!(ReturnKey, "auto" => Auto, "next" => Next, "done" => Done),
        Type::AccessibleLive => enumeration!(AccessibleLive,
            "off" => Off, "polite" => Polite, "assertive" => Assertive),
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
        Type::ButtonDisplay => enumeration!(ButtonDisplay,
            "icon-only" => IconOnly, "text-only" => TextOnly, "text-beside-icon" => TextBesideIcon, "text-under-icon" => TextUnderIcon),
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
        | Type::IntList
        | Type::DateList
        | Type::Hit3DList
        | Type::PhysicsContact3DList
        | Type::PhysicsHit3DList
        | Type::PointerButtons
        | Type::StyledText
        | Type::Record(_)
        | Type::RecordList(_) => return Err(format!("{} does not have literal syntax", ty.name())),
    })
}
