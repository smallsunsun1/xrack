"""Python types accepted by the XRack interpreter."""

from typing import TypeAlias

from ._native import (
    AccessibleLive,
    Brush,
    ButtonDisplay,
    CheckState,
    Color,
    ColorScheme,
    SceneToneMap,
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
    Hit3D,
    GradientStop,
    Image,
    ImageFit,
    ImageHorizontalAlignment,
    ImageRendering,
    ImageTileMode,
    ImageVerticalAlignment,
    InputPurpose,
    InputType,
    InputValidity,
    KeyboardCapitalization,
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
    Point2D,
    Point3D,
    ProjectedPoint3D,
    RenderStats3D,
    Instance3D,
    InstanceTable3D,
    TextureData2D,
    MeshAsset3D,
    MeshNode3D,
    Bounds3D,
    WalkResult3D,
    PhysicsWorld3D,
    SceneModel3D,
    PhysicsContact3D,
    PhysicsHit3D,
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
    Transform2D,
    Vector2D,
    Vector3D,
    Quaternion3D,
    RotationTrack3D,
    WheelEvent,
)

__all__ = [
    "AccessibleLive",
    "Brush",
    "ButtonDisplay",
    "CheckState",
    "Color",
    "ColorScheme",
    "SceneToneMap",
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
    "Hit3D",
    "GradientStop",
    "Image",
    "ImageFit",
    "ImageHorizontalAlignment",
    "ImageRendering",
    "ImageTileMode",
    "ImageVerticalAlignment",
    "InputPurpose",
    "InputType",
    "InputValidity",
    "KeyboardCapitalization",
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
    "Point2D",
    "Point3D",
    "ProjectedPoint3D",
    "RenderStats3D",
    "Instance3D",
    "InstanceTable3D",
    "TextureData2D",
    "MeshAsset3D",
    "MeshNode3D",
    "Bounds3D",
    "WalkResult3D",
    "PhysicsWorld3D",
    "SceneModel3D",
    "PhysicsContact3D",
    "PhysicsHit3D",
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
    "Transform2D",
    "Vector2D",
    "Vector3D",
    "Quaternion3D",
    "RotationTrack3D",
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
    | SceneToneMap
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
    | Hit3D
    | Image
    | ImageFit
    | ImageHorizontalAlignment
    | ImageRendering
    | ButtonDisplay
    | ImageTileMode
    | ImageVerticalAlignment
    | InputPurpose
    | InputType
    | InputValidity
    | KeyboardCapitalization
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
    | Point2D
    | Point3D
    | ProjectedPoint3D
    | RenderStats3D
    | Instance3D
    | InstanceTable3D
    | TextureData2D
    | MeshAsset3D
    | MeshNode3D
    | Bounds3D
    | WalkResult3D
    | PhysicsWorld3D
    | SceneModel3D
    | PhysicsContact3D
    | PhysicsHit3D
    | list[PhysicsContact3D]
    | list[PhysicsHit3D]
    | PointerButton
    | PointerButtons
    | PointerEvent
    | PointerEventKind
    | PopupPlacement
    | Record
    | RecordList
    | ReturnKey
    | AccessibleLive
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
    | Transform2D
    | Vector2D
    | Vector3D
    | Quaternion3D
    | RotationTrack3D
    | WheelEvent
)
