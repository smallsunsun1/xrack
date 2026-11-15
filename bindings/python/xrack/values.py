"""Python types accepted by the XRack interpreter."""

from typing import TypeAlias

from ._native import (
    Brush,
    CheckState,
    Color,
    ColorScheme,
    CrossAxisAlignment,
    CursorShape,
    Date,
    DialInputMode,
    DialogRole,
    DragAction,
    DrawerEdge,
    EventResult,
    FileDialogMode,
    FlexboxLayoutDirection,
    FlexboxLayoutWrap,
    FlickBoundsBehavior,
    FlickBoundsMovement,
    FlickDirection,
    FocusReason,
    Font,
    GradientStop,
    Image,
    ImageFit,
    ImageHorizontalAlignment,
    ImageRendering,
    ImageTileMode,
    ImageVerticalAlignment,
    InputType,
    KeyEvent,
    KeyModifiers,
    LayoutAlignment,
    LayoutDirection,
    Length,
    LogLevel,
    MessageButtons,
    MessageLevel,
    Orientation,
    Percent,
    PointerButton,
    PointerButtons,
    PointerEvent,
    PointerEventKind,
    PopupPlacement,
    Record,
    RecordList,
    RecordModel,
    ScrollAlignment,
    ScrollBarPolicy,
    ScrollSnapMode,
    SelectionTarget,
    StandardButtonKind,
    StringModel,
    StyledText,
    SwipeSide,
    TextHorizontalAlignment,
    TextOverflow,
    TextVerticalAlignment,
    TextWrap,
    Time,
    TouchPoint,
    TouchPoints,
    WheelEvent,
)

__all__ = [
    "Brush",
    "CheckState",
    "Color",
    "ColorScheme",
    "CrossAxisAlignment",
    "CursorShape",
    "Date",
    "DialInputMode",
    "DialogRole",
    "DragAction",
    "DrawerEdge",
    "EventResult",
    "FileDialogMode",
    "FlexboxLayoutDirection",
    "FlexboxLayoutWrap",
    "FlickBoundsBehavior",
    "FlickBoundsMovement",
    "FlickDirection",
    "FocusReason",
    "Font",
    "GradientStop",
    "Image",
    "ImageFit",
    "ImageHorizontalAlignment",
    "ImageRendering",
    "ImageTileMode",
    "ImageVerticalAlignment",
    "InputType",
    "KeyEvent",
    "KeyModifiers",
    "LayoutAlignment",
    "LayoutDirection",
    "Length",
    "LogLevel",
    "MessageButtons",
    "MessageLevel",
    "Orientation",
    "Percent",
    "PointerButton",
    "PointerButtons",
    "PointerEvent",
    "PointerEventKind",
    "PopupPlacement",
    "Record",
    "RecordList",
    "RecordModel",
    "ScrollAlignment",
    "ScrollBarPolicy",
    "ScrollSnapMode",
    "SelectionTarget",
    "StandardButtonKind",
    "StringModel",
    "StyledText",
    "SwipeSide",
    "TextHorizontalAlignment",
    "TextOverflow",
    "TextVerticalAlignment",
    "TextWrap",
    "Time",
    "TouchPoint",
    "TouchPoints",
    "WheelEvent",
    "Value",
]

Value: TypeAlias = (
    bool
    | int
    | float
    | str
    | list[str]
    | Brush
    | CheckState
    | Color
    | ColorScheme
    | CrossAxisAlignment
    | CursorShape
    | Date
    | DialInputMode
    | DialogRole
    | DragAction
    | DrawerEdge
    | EventResult
    | FileDialogMode
    | FlexboxLayoutDirection
    | FlexboxLayoutWrap
    | FlickBoundsBehavior
    | FlickBoundsMovement
    | FlickDirection
    | FocusReason
    | Font
    | Image
    | ImageFit
    | ImageHorizontalAlignment
    | ImageRendering
    | ImageTileMode
    | ImageVerticalAlignment
    | InputType
    | KeyEvent
    | KeyModifiers
    | LayoutAlignment
    | LayoutDirection
    | Length
    | LogLevel
    | MessageButtons
    | MessageLevel
    | Orientation
    | Percent
    | PointerButton
    | PointerButtons
    | PointerEvent
    | PointerEventKind
    | PopupPlacement
    | Record
    | RecordList
    | ScrollAlignment
    | ScrollBarPolicy
    | ScrollSnapMode
    | SelectionTarget
    | StandardButtonKind
    | StringModel
    | StyledText
    | SwipeSide
    | TextHorizontalAlignment
    | TextOverflow
    | TextVerticalAlignment
    | TextWrap
    | Time
    | TouchPoint
    | TouchPoints
    | WheelEvent
)
