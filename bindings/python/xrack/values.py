"""Python types accepted by the XRack interpreter."""

from typing import TypeAlias

from ._native import (
    Brush,
    ButtonDisplay,
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
    InputValidity,
    KeyEvent,
    KeyModifiers,
    LayoutAlignment,
    LayoutDirection,
    Length,
    LogLevel,
    MessageButtons,
    MessageLevel,
    MouseSelectionMode,
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
    ReturnKey,
    ScrollAlignment,
    ScrollBarPolicy,
    ScrollSnapMode,
    SelectionGesture,
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
    "ButtonDisplay",
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
    "InputValidity",
    "KeyEvent",
    "KeyModifiers",
    "LayoutAlignment",
    "LayoutDirection",
    "Length",
    "LogLevel",
    "MessageButtons",
    "MessageLevel",
    "MouseSelectionMode",
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
    "ReturnKey",
    "ScrollAlignment",
    "ScrollBarPolicy",
    "ScrollSnapMode",
    "SelectionGesture",
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
    | list[int]
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
    | ButtonDisplay
    | ImageTileMode
    | ImageVerticalAlignment
    | InputType
    | InputValidity
    | KeyEvent
    | KeyModifiers
    | LayoutAlignment
    | LayoutDirection
    | Length
    | LogLevel
    | MessageButtons
    | MessageLevel
    | MouseSelectionMode
    | Orientation
    | Percent
    | PointerButton
    | PointerButtons
    | PointerEvent
    | PointerEventKind
    | PopupPlacement
    | Record
    | RecordList
    | ReturnKey
    | ScrollAlignment
    | ScrollBarPolicy
    | ScrollSnapMode
    | SelectionGesture
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
