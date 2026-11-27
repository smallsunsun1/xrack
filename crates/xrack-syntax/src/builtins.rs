//! Shared names and signatures for the XR UI standard library.

pub fn push_button(kind: &str) -> bool {
    matches!(kind, "Button" | "ToolButton" | "RoundButton")
}

pub fn button_group_member(kind: &str) -> bool {
    push_button(kind) || matches!(kind, "RadioButton" | "CheckBox" | "Switch")
}

/// Canonical spellings of built-in elements, including the base of each delegate.
pub fn builtin_element(name: &str) -> Option<&'static str> {
    macro_rules! elements {
        ($($element:literal),+ $(,)?) => {
            match name {
                $($element => Some($element),)+
                _ => None,
            }
        };
    }
    elements!(
        "Action",
        "ActionGroup",
        "AnimatedImage",
        "ApplicationWindow",
        "BusyIndicator",
        "Button",
        "ButtonContent",
        "ButtonGroup",
        "Calendar",
        "CheckBox",
        "ColorPickerPopup",
        "CustomRender",
        "SceneView3D",
        "MeshLoader3D",
        "ComboBox",
        "ComboContent",
        "ComboIndicator",
        "ComboOption",
        "ComboPopup",
        "ContextMenuArea",
        "ControlFrame",
        "DatePickerPopup",
        "DayCell",
        "DayOfWeekRow",
        "DelayButton",
        "Dial",
        "Dialog",
        "DialogButton",
        "DialogButtonBox",
        "DoubleSpinBox",
        "DragArea",
        "Drawer",
        "DropArea",
        "FileDialog",
        "FlexboxLayout",
        "EmptyState",
        "Expander",
        "Flickable",
        "FlowLayout",
        "FocusScope",
        "FontPickerPopup",
        "GridLayout",
        "GridView",
        "GroupBox",
        "HorizontalBox",
        "HorizontalLayout",
        "HoverHandler",
        "Image",
        "Label",
        "LineEdit",
        "ListView",
        "Loader",
        "MediaPlayer",
        "SoundEffect",
        "Menu",
        "MenuBar",
        "MenuItem",
        "MenuSeparator",
        "MessageDialog",
        "MonthGrid",
        "Page",
        "PageFooter",
        "PageHeader",
        "PageIndicator",
        "Pane",
        "Path",
        "PathView",
        "PathViewOption",
        "PopupWindow",
        "ProgressFill",
        "ProgressIndicator",
        "ProgressTrack",
        "RadioButton",
        "RadioGroup",
        "RangeLowerHandle",
        "RangeSlider",
        "RangeUpperHandle",
        "Rectangle",
        "RoundButton",
        "Row",
        "ScaleRotateGestureHandler",
        "ScrollBar",
        "ScrollIndicator",
        "ScrollTrack",
        "ScrollThumb",
        "ScrollView",
        "SearchField",
        "SelectionEndHandle",
        "SelectionStartHandle",
        "SecondaryWindow",
        "SectionHeader",
        "Shortcut",
        "Slider",
        "SliderHandle",
        "SpinBox",
        "SpinDownIndicator",
        "SpinUpIndicator",
        "Spinner",
        "SplitHandle",
        "SplitView",
        "StackItem",
        "StackLayout",
        "StackPage",
        "StackView",
        "StandardButton",
        "StandardListView",
        "StandardTableView",
        "StyledText",
        "SwipeContent",
        "SwipeDelegate",
        "SwipeGestureHandler",
        "SwipeLeft",
        "SwipePage",
        "SwipeRight",
        "SwipeView",
        "Switch",
        "SwitchThumb",
        "SystemTrayIcon",
        "Tab",
        "TabBar",
        "TabButton",
        "TabWidget",
        "TableCell",
        "TableColumn",
        "TableRow",
        "TableView",
        "Text",
        "TextArea",
        "TextEdit",
        "TextInput",
        "TimePicker",
        "TimePickerPopup",
        "Timer",
        "FrameAnimation",
        "ToggleIndicator",
        "ToolBar",
        "ToolButton",
        "ToolSeparator",
        "Tooltip",
        "TouchArea",
        "TreeCell",
        "TreeDisclosure",
        "TreeRow",
        "TreeView",
        "Tumbler",
        "TumblerOption",
        "VerticalBox",
        "DynamicContainer",
        "VerticalLayout",
        "WeekNumberColumn",
        "WheelHandler",
        "Window",
        "WindowMoveArea",
    )
}

/// Logical start and end artwork slots for a table's selection range.
pub const TABLE_SELECTION_PARTS: [&str; 2] = ["SelectionStartHandle", "SelectionEndHandle"];

pub fn navigation_group(kind: &str) -> bool {
    matches!(kind, "TabBar" | "TabWidget" | "SwipeView" | "StackLayout")
}

pub fn navigation_option(kind: &str) -> bool {
    matches!(kind, "TabButton" | "Tab" | "SwipePage" | "StackItem")
}

pub struct BuiltinSignature {
    pub name: &'static str,
    pub parameters: &'static [&'static str],
    pub result: Option<&'static str>,
    pub documentation: &'static str,
}

impl BuiltinSignature {
    pub fn signature(&self) -> String {
        let mut label = format!("{}({})", self.name, self.parameters.join(", "));
        if let Some(result) = self.result {
            label.push_str(" -> ");
            label.push_str(result);
        }
        label
    }
}

pub struct SceneViewMethod {
    pub signature: BuiltinSignature,
    pub inputs: &'static [Type],
    pub output: Type,
}

const fn scene_view_method(
    name: &'static str,
    parameters: &'static [&'static str],
    inputs: &'static [Type],
    result: &'static str,
    output: Type,
    documentation: &'static str,
) -> SceneViewMethod {
    SceneViewMethod {
        signature: BuiltinSignature {
            name,
            parameters,
            result: Some(result),
            documentation,
        },
        inputs,
        output,
    }
}

pub fn scene_view_reads_item_sources(method: &str) -> bool {
    matches!(method, "hit-item" | "ray-hit-item")
}

pub const SCENE_VIEW_METHODS: &[SceneViewMethod] = &[
    scene_view_method(
        "render-stats",
        &[],
        &[],
        "renderstats3d",
        Type::RenderStats3D,
        "Returns the latest render sample requested by this viewport's render-stats-revision.",
    ),
    scene_view_method(
        "has-active-camera",
        &[],
        &[],
        "bool",
        Type::Bool,
        "Returns true when this laid-out viewport has a resolved camera, including its orbit camera.",
    ),
    scene_view_method(
        "active-camera",
        &[],
        &[],
        "string",
        Type::String,
        "Returns the resolved camera selector. Unnamed, orbit and unavailable cameras return an empty string.",
    ),
    scene_view_method(
        "active-camera-row",
        &[],
        &[],
        "int",
        Type::Int,
        "Returns the resolved camera's row in the viewport's prepared scene, or -1 for orbit or unavailable cameras.",
    ),
    scene_view_method(
        "active-camera-node",
        &[],
        &[],
        "int",
        Type::Int,
        "Returns the glTF source-node index of the resolved camera, or -1 when there is no glTF source node.",
    ),
    scene_view_method(
        "project",
        &["world: point3d"],
        &[Type::Point3D],
        "projectedpoint3d",
        Type::ProjectedPoint3D,
        "Projects a world point into this view's local logical pixels.",
    ),
    scene_view_method(
        "unproject",
        &["point: point2d", "depth: float", "fallback: point3d"],
        &[Type::Point2D, Type::Float, Type::Point3D],
        "point3d",
        Type::Point3D,
        "Maps a local point at a positive camera-space depth back to the scene.",
    ),
    scene_view_method(
        "pick",
        &["point: point2d"],
        &[Type::Point2D],
        "string",
        Type::String,
        "Returns the closest visible pickable object's ID.",
    ),
    scene_view_method(
        "hit",
        &["point: point2d"],
        &[Type::Point2D],
        "hit3d",
        Type::Hit3D,
        "Returns the closest geometric hit using this view's camera.",
    ),
    scene_view_method(
        "hit-item",
        &["point: point2d"],
        &[Type::Point2D],
        "hit3d",
        Type::Hit3D,
        "Picks a live 2D texture source and returns its local logical-pixel position.",
    ),
    scene_view_method(
        "hit-all",
        &["point: point2d"],
        &[Type::Point2D],
        "[hit3d]",
        Type::Hit3DList,
        "Returns intersections in nearest-first order.",
    ),
    scene_view_method(
        "hit-row",
        &["point: point2d", "row: int"],
        &[Type::Point2D, Type::Int],
        "hit3d",
        Type::Hit3D,
        "Returns the nearest intersection on the given scene record row.",
    ),
    scene_view_method(
        "hit-subset",
        &["point: point2d", "rows: [int]"],
        &[Type::Point2D, Type::IntList],
        "[hit3d]",
        Type::Hit3DList,
        "Returns intersections on the selected scene record rows.",
    ),
    scene_view_method(
        "hit-instance",
        &[
            "point: point2d",
            "row: int",
            "mesh-node: int",
            "instance: int",
        ],
        &[Type::Point2D, Type::Int, Type::Int, Type::Int],
        "hit3d",
        Type::Hit3D,
        "Tests an individual scene instance addressed by its hit indices.",
    ),
    scene_view_method(
        "ray-hit",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
        ],
        &[Type::Point3D, Type::Vector3D, Type::Float],
        "hit3d",
        Type::Hit3D,
        "Traces a world-space ray using the view's layers and selected mesh detail.",
    ),
    scene_view_method(
        "ray-hit-item",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
        ],
        &[Type::Point3D, Type::Vector3D, Type::Float],
        "hit3d",
        Type::Hit3D,
        "Traces a world-space ray to a live 2D texture source and returns its local logical-pixel position.",
    ),
    scene_view_method(
        "ray-hit-all",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
        ],
        &[Type::Point3D, Type::Vector3D, Type::Float],
        "[hit3d]",
        Type::Hit3DList,
        "Returns the view's world-ray intersections in nearest-first order.",
    ),
    scene_view_method(
        "ray-hit-row",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
            "row: int",
        ],
        &[Type::Point3D, Type::Vector3D, Type::Float, Type::Int],
        "hit3d",
        Type::Hit3D,
        "Tests a single authored row using the view's selected mesh detail.",
    ),
    scene_view_method(
        "ray-hit-subset",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
            "rows: [int]",
        ],
        &[Type::Point3D, Type::Vector3D, Type::Float, Type::IntList],
        "[hit3d]",
        Type::Hit3DList,
        "Traces a world-space ray against a list of authored rows.",
    ),
    scene_view_method(
        "ray-hit-instance",
        &[
            "origin: point3d",
            "direction: vector3d",
            "max-distance: float",
            "row: int",
            "mesh-node: int",
            "instance: int",
        ],
        &[
            Type::Point3D,
            Type::Vector3D,
            Type::Float,
            Type::Int,
            Type::Int,
            Type::Int,
        ],
        "hit3d",
        Type::Hit3D,
        "Traces a world-space ray against an addressed instance in this view.",
    ),
    scene_view_method(
        "closest-point",
        &["origin: point3d", "radius: float"],
        &[Type::Point3D, Type::Float],
        "hit3d",
        Type::Hit3D,
        "Finds the nearest pickable surface within a world-space radius using this view's scene selection.",
    ),
    scene_view_method(
        "closest-point-row",
        &["origin: point3d", "radius: float", "row: int"],
        &[Type::Point3D, Type::Float, Type::Int],
        "hit3d",
        Type::Hit3D,
        "Finds the nearest surface on the given scene row within a world-space radius.",
    ),
    scene_view_method(
        "pick-in-rect",
        &["start: point2d", "end: point2d"],
        &[Type::Point2D, Type::Point2D],
        "[int]",
        Type::IntList,
        "Returns scene record rows intersecting a rectangle in local logical pixels.",
    ),
];

pub const SCENE_TEXTURE_SIZE_PROPERTIES: [&str; 2] =
    ["effective-texture-width", "effective-texture-height"];

pub fn scene_texture_size_axis(name: &str) -> Option<usize> {
    SCENE_TEXTURE_SIZE_PROPERTIES
        .iter()
        .position(|&axis| axis == name)
}

pub const SCENE_TOUCH_METHOD: BuiltinSignature = BuiltinSignature {
    name: "set-touchpoint",
    parameters: &[
        "source: string",
        "position: point2d",
        "id: int",
        "pressed: bool",
    ],
    result: None,
    documentation: "Sends a programmatic touch in source-panel logical pixels to a TouchArea, push button, CheckBox, RadioButton, Switch, Slider, RangeSlider, or Dial in an attached texture source.",
};

pub fn scene_view_methods(kind: &str) -> impl Iterator<Item = &'static BuiltinSignature> {
    (kind == "SceneView3D")
        .then_some(SCENE_VIEW_METHODS)
        .into_iter()
        .flatten()
        .map(|method| &method.signature)
        .chain((kind == "SceneView3D").then_some(&SCENE_TOUCH_METHOD))
}

pub const DYNAMIC_CONTAINER_METHODS: &[BuiltinSignature] = &[
    BuiltinSignature {
        name: "mount",
        parameters: &["component initializer"],
        result: Some("component handle"),
        documentation: "Creates a typed component and appends it to this container.",
    },
    BuiltinSignature {
        name: "mount-async",
        parameters: &["component initializer"],
        result: Some("component handle"),
        documentation: "Queues component construction and returns a pending handle.",
    },
    BuiltinSignature {
        name: "mount-url",
        parameters: &["source: string"],
        result: Some("ExternalComponentHandle"),
        documentation: "Loads a component file or URL into this container.",
    },
    BuiltinSignature {
        name: "status",
        parameters: &["handle"],
        result: Some("string"),
        documentation: "Returns null, loading, ready, or error for this handle.",
    },
    BuiltinSignature {
        name: "is-mounted",
        parameters: &["handle"],
        result: Some("bool"),
        documentation: "Checks whether the original component is ready in this container.",
    },
    BuiltinSignature {
        name: "get",
        parameters: &["handle", "property: string", "fallback"],
        result: Some("property type"),
        documentation: "Reads a public property while the handle is mounted.",
    },
    BuiltinSignature {
        name: "set",
        parameters: &["handle", "property: string", "value"],
        result: Some("bool"),
        documentation: "Writes a public input on the mounted component.",
    },
    BuiltinSignature {
        name: "call",
        parameters: &["handle", "callback: string", "arguments..."],
        result: Some("callback result"),
        documentation: "Invokes the mounted component; a returning callback also takes a fallback before its arguments.",
    },
    BuiltinSignature {
        name: "remove",
        parameters: &["handle"],
        result: Some("bool"),
        documentation: "Unmounts the child and invalidates its handle.",
    },
    BuiltinSignature {
        name: "move",
        parameters: &["handle", "index: int"],
        result: Some("bool"),
        documentation: "Moves the child to a zero-based index and retains its state.",
    },
];

/// Public commands accepted by each media object.
pub fn scroll_bar_methods(kind: &str) -> &'static [BuiltinSignature] {
    if kind != "ScrollBar" {
        return &[];
    }
    &[
        BuiltinSignature {
            name: "increase",
            parameters: &[],
            result: None,
            documentation: "Moves toward the logical end by one configured step.",
        },
        BuiltinSignature {
            name: "decrease",
            parameters: &[],
            result: None,
            documentation: "Moves toward the logical start by one configured step.",
        },
    ]
}

pub const VIEWPORT_MOVE_METHODS: &[BuiltinSignature] = &[
    BuiltinSignature {
        name: "scroll-to",
        parameters: &["x: length", "y: length"],
        result: None,
        documentation: "Moves to a logical position on each axis after layout.",
    },
    BuiltinSignature {
        name: "scroll-by",
        parameters: &["dx: length", "dy: length"],
        result: None,
        documentation: "Moves by a signed logical distance on each axis. Multiple requests apply in order after layout.",
    },
];

pub const MOVE_NAVIGATION_ITEM: BuiltinSignature = BuiltinSignature {
    name: "move-item",
    parameters: &["from: int", "to: int"],
    result: None,
    documentation: "Moves the original row in the writable navigation model. Indices are zero-based. Selection follows the moved records.",
};

pub fn navigation_move_methods(kind: &str) -> &'static [BuiltinSignature] {
    if navigation_move_event(kind).is_some() {
        &[MOVE_NAVIGATION_ITEM]
    } else {
        &[]
    }
}

pub fn navigation_move_event(kind: &str) -> Option<&'static str> {
    match kind {
        "TabBar" | "TabWidget" => Some("tab-moved"),
        "SwipeView" => Some("page-moved"),
        _ => None,
    }
}

pub const POSITION_CHILD_METHOD: BuiltinSignature = BuiltinSignature {
    name: "position-view-at-child",
    parameters: &[
        "child: element",
        "horizontal: ScrollAlignment",
        "vertical: ScrollAlignment",
    ],
    result: None,
    documentation: "Positions the view at a named descendant after layout. The two alignments use logical directions.",
};

pub fn media_methods(kind: &str) -> &'static [BuiltinSignature] {
    match kind {
        "MediaPlayer" => MEDIA_METHODS,
        "SoundEffect" => &[
            BuiltinSignature {
                name: "play",
                parameters: &[],
                result: None,
                documentation: "Plays the WAV from the start for the configured number of loops, starting when preload is ready.",
            },
            BuiltinSignature {
                name: "stop",
                parameters: &[],
                result: None,
                documentation: "Stops the current playback or cancels a play requested during preload.",
            },
        ],
        _ => &[],
    }
}

const MEDIA_METHODS: &[BuiltinSignature] = &[
    BuiltinSignature {
        name: "go-live",
        parameters: &[],
        result: None,
        documentation: "Jumps to the current HLS live edge and starts playback.",
    },
    BuiltinSignature {
        name: "stop",
        parameters: &[],
        result: None,
        documentation: "Stops playback and rewinds seekable media.",
    },
    BuiltinSignature {
        name: "play",
        parameters: &[],
        result: None,
        documentation: "Starts or resumes playback. After the end, starts again from the beginning.",
    },
    BuiltinSignature {
        name: "pause",
        parameters: &[],
        result: None,
        documentation: "Pauses audio and video at the current playback position.",
    },
    BuiltinSignature {
        name: "seek",
        parameters: &["position: float"],
        result: None,
        documentation: "Seeks to a position in seconds and preserves whether playback was paused.",
    },
];

pub const TEXT_SELECTION_PROPERTIES: &[&str] = &[
    "cursor-offset",
    "selection-start-offset",
    "selection-end-offset",
    "selected-text",
];

pub const VIEW_COUNT_PROPERTIES: &[&str] = &["row-count", "source-row-count"];

pub const TEXT_COMMAND_PROPERTIES: &[&str] = &["can-undo", "can-redo", "can-cut", "can-copy"];

pub fn text_editor(kind: &str) -> bool {
    text_submission_editor(kind) || kind == "SearchField"
}

pub fn text_selection_kind(kind: &str) -> bool {
    text_editor(kind) || kind == "StyledText"
}

pub fn text_editor_methods(kind: &str) -> &'static [BuiltinSignature] {
    const METHODS: &[BuiltinSignature] = &[
        BuiltinSignature {
            name: "select-all",
            parameters: &[],
            result: None,
            documentation: "Selects all text in the focused editor.",
        },
        BuiltinSignature {
            name: "select-word",
            parameters: &[],
            result: None,
            documentation: "Selects the Unicode word segment at the caret. Password fields select the entire value.",
        },
        BuiltinSignature {
            name: "clear-selection",
            parameters: &[],
            result: None,
            documentation: "Collapses the selection at the caret.",
        },
        BuiltinSignature {
            name: "set-selection-offsets",
            parameters: &["anchor: int", "focus: int"],
            result: None,
            documentation: "Sets the anchor and caret in UTF-8 bytes, clamped to grapheme boundaries.",
        },
        BuiltinSignature {
            name: "copy",
            parameters: &[],
            result: None,
            documentation: "Copies selected text to the clipboard. Password fields do not copy text.",
        },
        BuiltinSignature {
            name: "undo",
            parameters: &[],
            result: None,
            documentation: "Undoes the last committed text edit.",
        },
        BuiltinSignature {
            name: "redo",
            parameters: &[],
            result: None,
            documentation: "Restores the last undone text edit.",
        },
        BuiltinSignature {
            name: "cut",
            parameters: &[],
            result: None,
            documentation: "Copies the selection to the clipboard and removes it from a writable field.",
        },
        BuiltinSignature {
            name: "paste",
            parameters: &[],
            result: None,
            documentation: "Inserts clipboard text, replacing any selection.",
        },
        BuiltinSignature {
            name: "insert-text",
            parameters: &["offset: int", "text: string"],
            result: None,
            documentation: "Queues an undoable insertion at a UTF-8 byte offset.",
        },
        BuiltinSignature {
            name: "replace-text",
            parameters: &["start: int", "end: int", "text: string"],
            result: None,
            documentation: "Queues an undoable replacement between two UTF-8 byte offsets.",
        },
        BuiltinSignature {
            name: "remove-text",
            parameters: &["start: int", "end: int"],
            result: None,
            documentation: "Queues an undoable removal between two UTF-8 byte offsets.",
        },
        BuiltinSignature {
            name: "append-text",
            parameters: &["text: string"],
            result: None,
            documentation: "Queues an undoable insertion at the end of the editor's current text.",
        },
        BuiltinSignature {
            name: "clear-text",
            parameters: &[],
            result: None,
            documentation: "Queues an undoable removal of all text.",
        },
    ];
    if kind == "StyledText" {
        &METHODS[..5]
    } else if text_editor(kind) {
        METHODS
    } else {
        &[]
    }
}

/// Bindings used by a text editor's final submission status.
pub const TEXT_SUBMISSION_INPUTS: &[&str] = &[
    "text",
    "input-type",
    "decimal-separator",
    "maximum-length",
    "input-mask",
    "input-pattern",
    "minimum-length",
    "minimum-value",
    "maximum-value",
    "decimals",
    "submit-when",
];

pub fn text_submission_editor(kind: &str) -> bool {
    text_submission_single_line(kind) || matches!(kind, "TextEdit" | "TextArea")
}

pub fn text_submission_single_line(kind: &str) -> bool {
    matches!(kind, "TextInput" | "LineEdit" | "ComboBox")
}

pub fn editing_finished_editor(kind: &str) -> bool {
    text_submission_editor(kind)
}

pub fn text_mask_editor(kind: &str) -> bool {
    text_submission_single_line(kind)
}

/// Rules that are evaluated when checking the final value rather than rendering an editor.
pub fn text_submission_rule(kind: &str, name: &str) -> bool {
    text_submission_editor(kind) && matches!(name, "minimum-length" | "submit-when")
        || text_submission_single_line(kind)
            && matches!(name, "minimum-value" | "maximum-value" | "decimals")
}

pub const NAMESPACES: &[&str] = &[
    "Math",
    "Strings",
    "Model",
    "Colors",
    "Dates",
    "Times",
    "I18n",
    "Log",
    "Point2D",
    "Point3D",
    "Vector3D",
    "Quaternion3D",
    "Vector2D",
    "Transform2D",
    "TextureData2D",
    "Scene3D",
    "Physics3D",
];
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[derive(Clone, Copy)]
pub enum FunctionKind {
    Round,
    Math(MathFunction),
    Geometry(GeometryFunction),
    Physics(PhysicsFunction),
    String(StringFunction),
    ModelCount,
    ModelIsEmpty,
    ModelAt,
    ModelFilter,
    ModelLookup,
    ModelSortBy,
    ModelEdit(ModelEditKind),
    Color(ColorFunction),
    DateIsValid,
    DateAddDays,
    DateAddMonths,
    DateWeekday,
    DateMonths,
    DateMonthIndex,
    DateParseIso,
    DateIsSelectable,
    TimeParseIso,
    TimeIsValidIso,
    TimeFormatIso,
    TimeInRange,
    Locale,
    FormatNumber,
    ParseNumber,
    Format,
    Log,
    LogAt(&'static str),
}

macro_rules! operations {
    ($name:ident { $($variant:ident => $spelling:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $spelling),+ }
            }

            pub fn parse(name: &str) -> Option<Self> {
                match name { $($spelling => Some(Self::$variant)),+, _ => None }
            }
        }
    };
}

operations!(ModelEditKind {
    Append => "append",
    Insert => "insert",
    Set => "set",
    Remove => "remove",
    Move => "move",
    Clear => "clear",
});

operations!(GeometryFunction {
    Point => "point",
    Vector => "vector",
    Identity => "identity",
    Translation => "translation",
    Rotation => "rotation",
    Scale => "scale",
    Compose => "compose",
    Around => "around",
    MapPoint => "map-point",
    MapVector => "map-vector",
    Prepare3D => "prepare-3d",
    Combine3D => "combine-3d",
    Bounds3D => "bounds-3d",
    GeometryStatus3D => "geometry-status-3d",
    GeometryError3D => "geometry-error-3d",
    MeshNode3D => "mesh-node-3d",
    SceneNode3D => "scene-node-3d",
    MeshNodeIndices3D => "mesh-node-indices-3d",
    MeshMaterialSlots3D => "mesh-material-slots-3d",
    MeshMaterialVariants3D => "mesh-material-variants-3d",
    MeshAnimationClips3D => "mesh-animation-clips-3d",
    MeshAnimationDuration3D => "mesh-animation-duration-3d",
    PrepareInstanceTable3D => "prepare-instance-table-3d",
    TextureRgba8 => "texture-rgba8",
    RandomInstanceTable3D => "random-instance-table-3d",
    TableInstance3D => "table-instance-3d",
    Pick3D => "pick-3d",
    PickCamera3D => "pick-camera-3d",
    PickInRect3D => "pick-in-rect-3d",
    PickInRectCamera3D => "pick-in-rect-camera-3d",
    Point3D => "point-3d",
    Vector3D => "vector-3d",
    QuaternionIdentity => "quaternion-identity",
    QuaternionNew => "quaternion-new",
    QuaternionAxisAngle => "quaternion-axis-angle",
    QuaternionEuler => "quaternion-euler",
    QuaternionLookRotation => "quaternion-look-rotation",
    QuaternionLookAt => "quaternion-look-at",
    QuaternionCompose => "quaternion-compose",
    QuaternionInverse => "quaternion-inverse",
    QuaternionSlerp => "quaternion-slerp",
    QuaternionRotate => "quaternion-rotate",
    QuaternionTrack => "quaternion-track",
    QuaternionTrackKeys => "quaternion-track-keys",
    QuaternionSample => "quaternion-sample",
    Hit3D => "hit-3d",
    HitCamera3D => "hit-camera-3d",
    RayHit3D => "ray-hit-3d",
    HitAll3D => "hit-all-3d",
    HitAllCamera3D => "hit-all-camera-3d",
    RayHitAll3D => "ray-hit-all-3d",
    HitRow3D => "hit-row-3d",
    HitRowCamera3D => "hit-row-camera-3d",
    RayHitRow3D => "ray-hit-row-3d",
    HitInstance3D => "hit-instance-3d",
    HitInstanceCamera3D => "hit-instance-camera-3d",
    RayHitInstance3D => "ray-hit-instance-3d",
    HitSubset3D => "hit-subset-3d",
    HitSubsetCamera3D => "hit-subset-camera-3d",
    RayHitSubset3D => "ray-hit-subset-3d",
    ClosestPoint3D => "closest-point-3d",
    ClosestPointOn3D => "closest-point-on-3d",
    SphereCast3D => "sphere-cast-3d",
    CapsuleCast3D => "capsule-cast-3d",
    SphereOverlap3D => "sphere-overlap-3d",
    CapsuleOverlap3D => "capsule-overlap-3d",
    CapsuleTriggerOverlap3D => "capsule-trigger-overlap-3d",
    CapsuleTriggerCrossings3D => "capsule-trigger-crossings-3d",
    CapsuleWalkResult3D => "capsule-walk-result-3d",
    CapsuleWalkWithContacts3D => "capsule-walk-with-contacts-3d",
    SphereSlide3D => "sphere-slide-3d",
    CapsuleSlide3D => "capsule-slide-3d",
    SphereWalk3D => "sphere-walk-3d",
    CapsuleWalk3D => "capsule-walk-3d",
    Project3D => "project-3d",
    ProjectCamera3D => "project-camera-3d",
    Unproject3D => "unproject-3d",
    UnprojectCamera3D => "unproject-camera-3d",
    MapPosition3D => "map-position-3d",
    MapDirection3D => "map-direction-3d",
    SceneInstance3D => "scene-instance-3d",
    SceneInstanceCamera3D => "scene-instance-camera-3d",
    SceneInstanceOrbit3D => "scene-instance-orbit-3d",
});

impl GeometryFunction {
    pub const fn result(self) -> Type {
        match self {
            Self::Point | Self::MapPoint => Type::Point2D,
            Self::Vector | Self::MapVector => Type::Vector2D,
            Self::Prepare3D | Self::Combine3D => Type::SceneModel3D,
            Self::Bounds3D => Type::Bounds3D,
            Self::GeometryStatus3D | Self::GeometryError3D => Type::String,
            Self::MeshNode3D | Self::SceneNode3D => Type::MeshNode3D,
            Self::MeshNodeIndices3D => Type::IntList,
            Self::MeshMaterialSlots3D
            | Self::MeshMaterialVariants3D
            | Self::MeshAnimationClips3D => Type::StringList,
            Self::MeshAnimationDuration3D => Type::Float,
            Self::PrepareInstanceTable3D | Self::RandomInstanceTable3D => Type::InstanceTable3D,
            Self::TextureRgba8 => Type::TextureData2D,
            Self::TableInstance3D
            | Self::SceneInstance3D
            | Self::SceneInstanceCamera3D
            | Self::SceneInstanceOrbit3D => Type::Instance3D,
            Self::Pick3D | Self::PickCamera3D => Type::String,
            Self::PickInRect3D | Self::PickInRectCamera3D => Type::IntList,
            Self::Point3D
            | Self::Unproject3D
            | Self::UnprojectCamera3D
            | Self::MapPosition3D
            | Self::SphereSlide3D
            | Self::SphereWalk3D
            | Self::CapsuleSlide3D
            | Self::CapsuleWalk3D => Type::Point3D,
            Self::Project3D | Self::ProjectCamera3D => Type::ProjectedPoint3D,
            Self::CapsuleWalkResult3D | Self::CapsuleWalkWithContacts3D => Type::WalkResult3D,
            Self::Vector3D | Self::MapDirection3D => Type::Vector3D,
            Self::QuaternionRotate => Type::Vector3D,
            Self::QuaternionIdentity
            | Self::QuaternionNew
            | Self::QuaternionAxisAngle
            | Self::QuaternionEuler
            | Self::QuaternionLookRotation
            | Self::QuaternionLookAt
            | Self::QuaternionCompose
            | Self::QuaternionInverse
            | Self::QuaternionSlerp
            | Self::QuaternionSample => Type::Quaternion3D,
            Self::QuaternionTrack | Self::QuaternionTrackKeys => Type::RotationTrack3D,
            Self::Hit3D
            | Self::HitCamera3D
            | Self::RayHit3D
            | Self::HitRow3D
            | Self::HitRowCamera3D
            | Self::RayHitRow3D
            | Self::HitInstance3D
            | Self::HitInstanceCamera3D
            | Self::RayHitInstance3D
            | Self::ClosestPoint3D
            | Self::ClosestPointOn3D
            | Self::SphereCast3D
            | Self::CapsuleCast3D => Type::Hit3D,
            Self::HitAll3D
            | Self::HitAllCamera3D
            | Self::RayHitAll3D
            | Self::SphereOverlap3D
            | Self::CapsuleOverlap3D
            | Self::CapsuleTriggerOverlap3D
            | Self::CapsuleTriggerCrossings3D
            | Self::HitSubset3D
            | Self::HitSubsetCamera3D
            | Self::RayHitSubset3D => Type::Hit3DList,
            _ => Type::Transform2D,
        }
    }
    pub const fn uses_mesh_asset(self) -> bool {
        matches!(
            self,
            Self::MeshNode3D
                | Self::MeshNodeIndices3D
                | Self::MeshMaterialSlots3D
                | Self::MeshMaterialVariants3D
                | Self::MeshAnimationClips3D
                | Self::MeshAnimationDuration3D
        )
    }

    pub const fn uses_scene_model(self) -> bool {
        matches!(
            self,
            Self::Prepare3D
                | Self::Bounds3D
                | Self::GeometryStatus3D
                | Self::GeometryError3D
                | Self::SceneNode3D
                | Self::Pick3D
                | Self::PickCamera3D
                | Self::PickInRect3D
                | Self::PickInRectCamera3D
                | Self::ProjectCamera3D
                | Self::UnprojectCamera3D
                | Self::MapPosition3D
                | Self::MapDirection3D
                | Self::SceneInstance3D
                | Self::SceneInstanceCamera3D
                | Self::SceneInstanceOrbit3D
                | Self::Hit3D
                | Self::HitCamera3D
                | Self::RayHit3D
                | Self::HitAll3D
                | Self::HitAllCamera3D
                | Self::RayHitAll3D
                | Self::HitRow3D
                | Self::HitRowCamera3D
                | Self::RayHitRow3D
                | Self::HitInstance3D
                | Self::HitInstanceCamera3D
                | Self::RayHitInstance3D
                | Self::HitSubset3D
                | Self::HitSubsetCamera3D
                | Self::RayHitSubset3D
                | Self::ClosestPoint3D
                | Self::ClosestPointOn3D
                | Self::SphereCast3D
                | Self::SphereOverlap3D
                | Self::SphereSlide3D
                | Self::SphereWalk3D
                | Self::CapsuleCast3D
                | Self::CapsuleOverlap3D
                | Self::CapsuleTriggerOverlap3D
                | Self::CapsuleTriggerCrossings3D
                | Self::CapsuleWalkResult3D
                | Self::CapsuleWalkWithContacts3D
                | Self::CapsuleSlide3D
                | Self::CapsuleWalk3D
        )
    }
}

operations!(PhysicsFunction {
    New => "new",
    AddBox => "add-box",
    AddSphere => "add-sphere",
    AddKinematicBox => "add-kinematic-box",
    AddBoxTrigger => "add-box-trigger",
    AddBody => "add-body",
    AddKinematicBody => "add-kinematic-body",
    AddTrigger => "add-trigger",
    SyncBodies => "sync-bodies",
    BodyById => "body-by-id",
    SyncScene => "sync-scene",
    JointById => "joint-by-id",
    SetCollisionFilter => "set-collision-filter",
    SetContactReports => "set-contact-reports",
    SetKinematicContactReports => "set-kinematic-contact-reports",
    ReportStaticKinematicCollisions => "report-static-kinematic-collisions",
    ReportKinematicKinematicCollisions => "report-kinematic-kinematic-collisions",
    Contacts => "contacts",
    SetContactEvents => "set-contact-events",
    ContactBodies => "contact-bodies",
    ContactEntered => "contact-entered",
    ContactExited => "contact-exited",
    AddFixedJoint => "add-fixed-joint",
    AddSphericalJoint => "add-spherical-joint",
    AddRevoluteJoint => "add-revolute-joint",
    AddPrismaticJoint => "add-prismatic-joint",
    AddDistanceJoint => "add-distance-joint",
    SetJointLimits => "set-joint-limits",
    SetJointMotor => "set-joint-motor",
    JointPosition => "joint-position",
    RemoveJoint => "remove-joint",
    TriggerCount => "trigger-count",
    TriggerOverlaps => "trigger-overlaps",
    TriggerEntered => "trigger-entered",
    TriggerExited => "trigger-exited",
    SetKinematicTarget => "set-kinematic-target",
    SetKinematicTargetWithPivot => "set-kinematic-target-with-pivot",
    SetKinematic => "set-kinematic",
    IsKinematic => "is-kinematic",
    Step => "step",
    Advance => "advance",
    AdvanceRealtime => "advance-realtime",
    SetRunning => "set-running",
    SetTimeScale => "set-time-scale",
    SetPoseInterpolation => "set-pose-interpolation",
    IsRunning => "is-running",
    TimeScale => "time-scale",
    SimulationTime => "simulation-time",
    DroppedTime => "dropped-time",
    InterpolationAlpha => "interpolation-alpha",
    InterpolatedPosition => "interpolated-position",
    InterpolatedRotation => "interpolated-rotation",
    Position => "position",
    Rotation => "rotation",
    Velocity => "velocity",
    ApplyImpulse => "apply-impulse",
    ApplyForce => "apply-force",
    ApplyForceAt => "apply-force-at",
    ApplyTorque => "apply-torque",
    SetContinuousLoad => "set-continuous-load",
    ClearForces => "clear-forces",
    ApplyImpulseAt => "apply-impulse-at",
    ApplyTorqueImpulse => "apply-torque-impulse",
    SetLinearVelocity => "set-linear-velocity",
    SetAngularVelocity => "set-angular-velocity",
    ResetBody => "reset-body",
    SetGravityScale => "set-gravity-scale",
    SetDamping => "set-damping",
    SetMass => "set-mass",
    SetDensity => "set-density",
    UseDefaultDensity => "use-default-density",
    SetDefaultDensity => "set-default-density",
    SetMassProperties => "set-mass-properties",
    SetInertiaMatrix => "set-inertia-matrix",
    Mass => "mass",
    DefaultDensity => "default-density",
    CenterOfMass => "center-of-mass",
    LocalCenterOfMass => "local-center-of-mass",
    PrincipalInertia => "principal-inertia",
    InertiaColumn => "inertia-column",
    SetMaterial => "set-material",
    StaticFriction => "static-friction",
    DynamicFriction => "dynamic-friction",
    Restitution => "restitution",
    SetAxisLocks => "set-axis-locks",
    SetCcdSubsteps => "set-ccd-substeps",
    CcdSubsteps => "ccd-substeps",
    SetBodyCcd => "set-body-ccd",
    BodyCcd => "body-ccd",
    SetSoftCcd => "set-soft-ccd",
    SoftCcd => "soft-ccd",
    SetLengthUnit => "set-length-unit",
    LengthUnit => "length-unit",
    SetGravity => "set-gravity",
    Gravity => "gravity",
    UseCharacterWorldGravity => "use-character-world-gravity",
    CharacterGravity => "character-gravity",
    AngularVelocity => "angular-velocity",
    IsSleeping => "is-sleeping",
    AddCharacter => "add-character",
    SetCharacterMovement => "set-character-movement",
    SetCharacterGravity => "set-character-gravity",
    SetCharacterMidAirControl => "set-character-mid-air-control",
    SetCharacterOptions => "set-character-options",
    JumpCharacter => "jump-character",
    TeleportCharacter => "teleport-character",
    CharacterGrounded => "character-grounded",
    CharacterHitWall => "character-hit-wall",
    CharacterHitCeiling => "character-hit-ceiling",
    CharacterCollisions => "character-collisions",
    CharacterSupport => "character-support",
    SetCharacterHitReports => "set-character-hit-reports",
    CharacterHits => "character-hits",
    RayCast => "ray-cast",
    RayCastAll => "ray-cast-all",
    ShapeCast => "shape-cast",
    ShapeCastAll => "shape-cast-all",
    ShapeOverlap => "shape-overlap",
    Remove => "remove",
    Clear => "clear",
});

impl PhysicsFunction {
    pub const fn result(self) -> Type {
        match self {
            Self::New => Type::PhysicsWorld3D,
            Self::AddBox
            | Self::AddSphere
            | Self::AddKinematicBox
            | Self::AddBoxTrigger
            | Self::AddBody
            | Self::AddKinematicBody
            | Self::AddTrigger
            | Self::BodyById
            | Self::JointById
            | Self::AddFixedJoint
            | Self::AddSphericalJoint
            | Self::AddRevoluteJoint
            | Self::AddPrismaticJoint
            | Self::AddDistanceJoint
            | Self::AddCharacter
            | Self::CharacterCollisions
            | Self::CharacterSupport
            | Self::TriggerCount
            | Self::Advance
            | Self::AdvanceRealtime
            | Self::CcdSubsteps => Type::Int,
            Self::JointPosition
            | Self::StaticFriction
            | Self::DynamicFriction
            | Self::Restitution
            | Self::Mass
            | Self::DefaultDensity
            | Self::TimeScale
            | Self::SimulationTime
            | Self::DroppedTime
            | Self::InterpolationAlpha
            | Self::SoftCcd
            | Self::LengthUnit => Type::Float,
            Self::Contacts | Self::CharacterHits => Type::PhysicsContact3DList,
            Self::RayCast | Self::ShapeCast => Type::PhysicsHit3D,
            Self::RayCastAll | Self::ShapeCastAll => Type::PhysicsHit3DList,
            Self::TriggerEntered
            | Self::TriggerExited
            | Self::ShapeOverlap
            | Self::ContactBodies
            | Self::ContactEntered
            | Self::ContactExited => Type::IntList,
            Self::Position
            | Self::CenterOfMass
            | Self::LocalCenterOfMass
            | Self::InterpolatedPosition => Type::Point3D,
            Self::Rotation
            | Self::Velocity
            | Self::AngularVelocity
            | Self::PrincipalInertia
            | Self::InertiaColumn
            | Self::InterpolatedRotation
            | Self::Gravity
            | Self::CharacterGravity => Type::Vector3D,
            Self::TriggerOverlaps
            | Self::SyncBodies
            | Self::SyncScene
            | Self::SetCollisionFilter
            | Self::SetContactReports
            | Self::SetContactEvents
            | Self::SetKinematicContactReports
            | Self::ReportStaticKinematicCollisions
            | Self::ReportKinematicKinematicCollisions
            | Self::SetCharacterHitReports
            | Self::SetJointLimits
            | Self::SetJointMotor
            | Self::RemoveJoint
            | Self::SetKinematicTarget
            | Self::SetKinematicTargetWithPivot
            | Self::SetKinematic
            | Self::IsKinematic
            | Self::Step
            | Self::SetRunning
            | Self::SetTimeScale
            | Self::SetPoseInterpolation
            | Self::IsRunning
            | Self::ApplyImpulse
            | Self::ApplyForce
            | Self::ApplyForceAt
            | Self::ApplyTorque
            | Self::SetContinuousLoad
            | Self::ClearForces
            | Self::ApplyImpulseAt
            | Self::ApplyTorqueImpulse
            | Self::SetLinearVelocity
            | Self::SetAngularVelocity
            | Self::ResetBody
            | Self::SetGravityScale
            | Self::SetDamping
            | Self::SetMass
            | Self::SetDensity
            | Self::UseDefaultDensity
            | Self::SetDefaultDensity
            | Self::SetMassProperties
            | Self::SetInertiaMatrix
            | Self::SetMaterial
            | Self::SetAxisLocks
            | Self::SetCcdSubsteps
            | Self::SetBodyCcd
            | Self::SetSoftCcd
            | Self::SetLengthUnit
            | Self::SetGravity
            | Self::UseCharacterWorldGravity
            | Self::BodyCcd
            | Self::IsSleeping
            | Self::SetCharacterMovement
            | Self::SetCharacterGravity
            | Self::SetCharacterMidAirControl
            | Self::SetCharacterOptions
            | Self::JumpCharacter
            | Self::TeleportCharacter
            | Self::CharacterGrounded
            | Self::CharacterHitWall
            | Self::CharacterHitCeiling
            | Self::Remove
            | Self::Clear => Type::Bool,
        }
    }

    pub const fn writes(self) -> bool {
        matches!(
            self,
            Self::AddBox
                | Self::AddSphere
                | Self::AddKinematicBox
                | Self::AddBoxTrigger
                | Self::AddBody
                | Self::AddKinematicBody
                | Self::AddTrigger
                | Self::SyncBodies
                | Self::SyncScene
                | Self::SetCollisionFilter
                | Self::SetContactReports
                | Self::SetContactEvents
                | Self::SetKinematicContactReports
                | Self::SetCharacterHitReports
                | Self::AddFixedJoint
                | Self::AddSphericalJoint
                | Self::AddRevoluteJoint
                | Self::AddPrismaticJoint
                | Self::AddDistanceJoint
                | Self::SetJointLimits
                | Self::SetJointMotor
                | Self::RemoveJoint
                | Self::SetKinematicTarget
                | Self::SetKinematicTargetWithPivot
                | Self::SetKinematic
                | Self::Step
                | Self::Advance
                | Self::AdvanceRealtime
                | Self::SetRunning
                | Self::SetTimeScale
                | Self::SetPoseInterpolation
                | Self::ApplyImpulse
                | Self::AddCharacter
                | Self::SetCharacterMovement
                | Self::SetCharacterGravity
                | Self::SetCharacterMidAirControl
                | Self::SetCharacterOptions
                | Self::JumpCharacter
                | Self::TeleportCharacter
                | Self::ApplyForce
                | Self::ApplyForceAt
                | Self::ApplyTorque
                | Self::SetContinuousLoad
                | Self::ClearForces
                | Self::ApplyImpulseAt
                | Self::ApplyTorqueImpulse
                | Self::SetLinearVelocity
                | Self::SetAngularVelocity
                | Self::ResetBody
                | Self::SetGravityScale
                | Self::SetDamping
                | Self::SetMass
                | Self::SetDensity
                | Self::UseDefaultDensity
                | Self::SetDefaultDensity
                | Self::SetMassProperties
                | Self::SetInertiaMatrix
                | Self::SetMaterial
                | Self::SetAxisLocks
                | Self::SetCcdSubsteps
                | Self::SetBodyCcd
                | Self::SetSoftCcd
                | Self::SetLengthUnit
                | Self::SetGravity
                | Self::UseCharacterWorldGravity
                | Self::Remove
                | Self::Clear
        )
    }

    pub const fn uses_shape_list(self) -> bool {
        matches!(
            self,
            Self::AddBody
                | Self::AddKinematicBody
                | Self::AddTrigger
                | Self::ShapeCast
                | Self::ShapeCastAll
                | Self::ShapeOverlap
        )
    }

    pub const fn query_filter_offset(self) -> Option<usize> {
        match self {
            Self::RayCast | Self::RayCastAll | Self::ShapeOverlap => Some(4),
            Self::ShapeCast | Self::ShapeCastAll => Some(6),
            _ => None,
        }
    }
}

operations!(MathFunction {
    Min => "min", Max => "max", Clamp => "clamp", Abs => "abs",
    Floor => "floor", Ceil => "ceil", Mod => "mod",
    Sign => "sign", Sqrt => "sqrt", Pow => "pow", Exp => "exp", Ln => "ln", Log => "log",
    Sin => "sin", Cos => "cos", Tan => "tan", Asin => "asin", Acos => "acos",
    Atan => "atan", Atan2 => "atan2", Hypot => "hypot",
    ToRadians => "to-radians", ToDegrees => "to-degrees",
    IsFinite => "is-finite",
});

operations!(StringFunction {
    Trim => "trim", IsEmpty => "is-empty", Contains => "contains",
    StartsWith => "starts-with", EndsWith => "ends-with", CharacterCount => "character-count",
    ReplaceAll => "replace-all", Lowercase => "lowercase", Uppercase => "uppercase",
    IsInt => "is-int", IsFloat => "is-float", ToInt => "to-int", ToFloat => "to-float",
});

operations!(ColorFunction {
    Rgb => "rgb", Rgba => "rgba", WithAlpha => "with-alpha", Mix => "mix",
});

impl StringFunction {
    pub const fn result(self) -> crate::ast::Type {
        use crate::ast::Type;
        match self {
            Self::Trim | Self::ReplaceAll | Self::Lowercase | Self::Uppercase => Type::String,
            Self::CharacterCount | Self::ToInt => Type::Int,
            Self::ToFloat => Type::Float,
            _ => Type::Bool,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Parameter {
    pub label: &'static str,
    pub expected: Option<&'static crate::ast::Type>,
    optional: bool,
}

impl Parameter {
    pub const fn new(label: &'static str) -> Self {
        Self {
            label,
            expected: None,
            optional: false,
        }
    }

    pub const fn with_type(label: &'static str, ty: &'static crate::ast::Type) -> Self {
        Self {
            label,
            expected: Some(ty),
            optional: false,
        }
    }

    pub const fn optional(label: &'static str, ty: &'static crate::ast::Type) -> Self {
        Self {
            optional: true,
            ..Self::with_type(label, ty)
        }
    }
}

pub struct Function {
    pub namespace: &'static str,
    pub member: &'static str,
    pub kind: FunctionKind,
    pub parameters: &'static [Parameter],
    pub result: Option<&'static str>,
    pub documentation: &'static str,
}

impl Function {
    pub fn minimum_arguments(&self) -> usize {
        self.parameters
            .iter()
            .take_while(|arg| !arg.optional)
            .count()
    }

    pub fn signature(&self) -> String {
        let mut signature = format!(
            "{}.{}({})",
            self.namespace,
            self.member,
            self.parameters
                .iter()
                .map(|parameter| parameter.label)
                .collect::<Vec<_>>()
                .join(", ")
        );
        if let Some(result) = self.result {
            signature.push_str(" -> ");
            signature.push_str(result);
        }
        signature
    }
}

const LOG_MESSAGE: &[Parameter] = &[Parameter::with_type(
    "message: string",
    &crate::ast::Type::String,
)];

const fn log_at(member: &'static str, documentation: &'static str) -> Function {
    Function {
        namespace: "Log",
        member,
        kind: FunctionKind::LogAt(member),
        parameters: LOG_MESSAGE,
        result: None,
        documentation,
    }
}

const fn math(
    kind: MathFunction,
    labels: &'static [Parameter],
    result: &'static str,
    docs: &'static str,
) -> Function {
    Function {
        namespace: "Math",
        member: kind.name(),
        kind: FunctionKind::Math(kind),
        parameters: labels,
        result: Some(result),
        documentation: docs,
    }
}

use crate::ast::Type;

const DATE: Parameter = Parameter::with_type("date: Date", &Type::Date);
const DATE_FALLBACK: Parameter = Parameter::with_type("fallback: Date", &Type::Date);
const TEXT: Parameter = Parameter::with_type("text: string", &Type::String);
const NUMBER: Parameter = Parameter::with_type("value: float", &Type::Float);
const MODEL: Parameter = Parameter::new("model: [int] | [string] | [Date] | [Record] | [hit3d]");
const WRITABLE_MODEL: Parameter = Parameter::new("model: writable [Row]");
const MODEL_ROW: Parameter = Parameter::new("value: Row");
const MODEL_INDEX: Parameter = Parameter::with_type("index: int", &Type::Int);

const fn model_edit(
    edit: ModelEditKind,
    parameters: &'static [Parameter],
    documentation: &'static str,
) -> Function {
    Function {
        namespace: "Model",
        member: edit.name(),
        kind: FunctionKind::ModelEdit(edit),
        parameters,
        result: None,
        documentation,
    }
}
const OPACITY: Parameter = Parameter::with_type("alpha: float", &Type::Float);
const RGB: &[Parameter] = &[
    Parameter::with_type("red: int", &Type::Int),
    Parameter::with_type("green: int", &Type::Int),
    Parameter::with_type("blue: int", &Type::Int),
];
const RGBA: &[Parameter] = &[RGB[0], RGB[1], RGB[2], OPACITY];
const COLOR: Parameter = Parameter::with_type("color: color", &Type::Color);

const fn color(
    kind: ColorFunction,
    parameters: &'static [Parameter],
    docs: &'static str,
) -> Function {
    Function {
        namespace: "Colors",
        member: kind.name(),
        kind: FunctionKind::Color(kind),
        parameters,
        result: Some("color"),
        documentation: docs,
    }
}

const fn string(
    kind: StringFunction,
    parameters: &'static [Parameter],
    result: &'static str,
    docs: &'static str,
) -> Function {
    Function {
        namespace: "Strings",
        member: kind.name(),
        kind: FunctionKind::String(kind),
        parameters,
        result: Some(result),
        documentation: docs,
    }
}

const XY: &[Parameter] = &[
    Parameter::with_type("x: length", &Type::Length),
    Parameter::with_type("y: length", &Type::Length),
];
const TRANSFORM: Parameter = Parameter::with_type("transform: transform2d", &Type::Transform2D);
const POINT: Parameter = Parameter::with_type("point: point2d", &Type::Point2D);
const VECTOR: Parameter = Parameter::with_type("vector: vector2d", &Type::Vector2D);

pub const SCENE_OBJECT_FIELDS: &[(&str, Type)] = &[
    ("id", Type::String),
    ("x", Type::Float),
    ("y", Type::Float),
    ("z", Type::Float),
    ("scale-x", Type::Float),
    ("scale-y", Type::Float),
    ("scale-z", Type::Float),
    ("yaw", Type::Float),
    ("pitch", Type::Float),
    ("roll", Type::Float),
    ("color", Type::Color),
];

/// Map properties that accept RGBA channel selectors. The boolean allows RGB.
pub const SCENE_MATERIAL_CHANNEL_FIELDS: &[(&str, bool)] = &[
    ("base-color-channel", true),
    ("emissive-channel", true),
    ("metalness-channel", false),
    ("roughness-channel", false),
    ("glossiness-channel", false),
    ("occlusion-channel", false),
    ("specular-channel", false),
    ("specular-color-channel", true),
    ("transmission-channel", false),
    ("thickness-channel", false),
    ("clearcoat-channel", false),
    ("clearcoat-roughness-channel", false),
];

pub const SCALAR_TEXTURE_CHANNELS: &[(&str, &str)] = &[
    ("r", "Read the red channel"),
    ("g", "Read the green channel"),
    ("b", "Read the blue channel"),
    ("a", "Read the alpha channel"),
];

pub const COLOR_TEXTURE_CHANNELS: &[(&str, &str)] = &[
    ("rgb", "Use the texture color and alpha"),
    SCALAR_TEXTURE_CHANNELS[0],
    SCALAR_TEXTURE_CHANNELS[1],
    SCALAR_TEXTURE_CHANNELS[2],
    SCALAR_TEXTURE_CHANNELS[3],
];

pub const SCENE_VERTEX_MASK_FIELDS: &[&str] = &[
    "vertex-metalness-channel",
    "vertex-roughness-channel",
    "vertex-occlusion-channel",
    "vertex-specular-channel",
    "vertex-normal-channel",
    "vertex-clearcoat-channel",
    "vertex-clearcoat-roughness-channel",
    "vertex-clearcoat-normal-channel",
    "vertex-transmission-channel",
    "vertex-thickness-channel",
];

pub const SCENE_VERTEX_MASK_CHANNELS: &[(&str, &str)] = &[
    ("none", "Use the material factor without a vertex mask"),
    SCALAR_TEXTURE_CHANNELS[0],
    SCALAR_TEXTURE_CHANNELS[1],
    SCALAR_TEXTURE_CHANNELS[2],
    SCALAR_TEXTURE_CHANNELS[3],
];

pub const SCENE_MATERIAL_FIELDS: &[(&str, Type)] = &[
    ("material-shader", Type::String),
    ("shader-parameters", Type::Vector3D),
    ("shader-displacement", Type::Float),
    ("material-workflow", Type::String),
    ("glossiness", Type::Float),
    ("normal-strength", Type::Float),
    ("clearcoat-normal-strength", Type::Float),
    ("enabled", Type::Bool),
    ("vertex-colors", Type::Bool),
    ("base-color", Type::Color),
    ("metalness", Type::Float),
    ("anisotropy-strength", Type::Float),
    ("anisotropy-rotation", Type::Float),
    ("roughness", Type::Float),
    ("emissive-color", Type::Color),
    ("emissive-strength", Type::Float),
    ("alpha-mode", Type::String),
    ("blend-mode", Type::String),
    ("depth-draw-mode", Type::String),
    ("cull-mode", Type::String),
    ("alpha-cutoff", Type::Float),
    ("unlit", Type::Bool),
    ("clearcoat", Type::Float),
    ("clearcoat-roughness", Type::Float),
    ("transmission", Type::Float),
    ("index-of-refraction", Type::Float),
    ("occlusion-amount", Type::Float),
    ("specular-amount", Type::Float),
    ("specular-color", Type::Color),
    ("thickness", Type::Float),
    ("attenuation-color", Type::Color),
    ("attenuation-distance", Type::Float),
    ("dispersion", Type::Float),
    ("fresnel-power", Type::Float),
    ("fresnel-scale", Type::Float),
    ("fresnel-bias", Type::Float),
    ("fresnel-scale-bias-enabled", Type::Bool),
    ("clearcoat-fresnel-power", Type::Float),
    ("clearcoat-fresnel-scale", Type::Float),
    ("clearcoat-fresnel-bias", Type::Float),
    ("clearcoat-fresnel-scale-bias-enabled", Type::Bool),
];

pub const SCENE_MATERIAL_MAP_FIELDS: &[&str] = &[
    "base-color-map",
    "metallic-roughness-map",
    "glossiness-map",
    "emissive-map",
    "occlusion-map",
    "normal-map",
    "clearcoat-map",
    "clearcoat-roughness-map",
    "clearcoat-normal-map",
    "transmission-map",
    "specular-map",
    "specular-color-map",
    "thickness-map",
    "anisotropy-map",
];

pub const SCENE_TEXTURE_FIELDS: &[(&str, Type)] = &[
    ("source", Type::String),
    ("data", Type::TextureData2D),
    ("wrap-u", Type::String),
    ("wrap-v", Type::String),
    ("min-filter", Type::String),
    ("mag-filter", Type::String),
    ("mip-filter", Type::String),
    ("max-anisotropy", Type::Int),
    ("uv-set", Type::Int),
    ("uv-scale-u", Type::Float),
    ("uv-scale-v", Type::Float),
    ("uv-pivot-u", Type::Float),
    ("uv-pivot-v", Type::Float),
    ("uv-offset-u", Type::Float),
    ("uv-offset-v", Type::Float),
    ("uv-rotation", Type::Float),
    ("uv-flip-u", Type::Bool),
    ("uv-flip-v", Type::Bool),
];

pub const SCENE_SKY_FIELDS: &[(&str, Type)] = &[
    ("enabled", Type::Bool),
    ("sky-horizon-color", Type::Color),
    ("sky-top-color", Type::Color),
    ("sky-curve", Type::Float),
    ("sky-energy", Type::Float),
    ("ground-horizon-color", Type::Color),
    ("ground-bottom-color", Type::Color),
    ("ground-curve", Type::Float),
    ("ground-energy", Type::Float),
    ("sun-color", Type::Color),
    ("sun-curve", Type::Float),
    ("sun-energy", Type::Float),
    ("sun-latitude", Type::Float),
    ("sun-longitude", Type::Float),
    ("sun-angle-min", Type::Float),
    ("sun-angle-max", Type::Float),
    ("resolution", Type::Int),
];

pub const SCENE_TEXTURE_WRAP: &[(&str, &str)] = &[
    ("repeat", "Tile at the UV boundary"),
    ("clamp", "Extend the edge texels"),
    ("mirror", "Mirror every other tile"),
];

pub const SCENE_TEXTURE_FILTER: &[(&str, &str)] = &[
    ("nearest", "Select the nearest texel"),
    ("linear", "Interpolate between neighboring texels"),
];

pub const SCENE_TEXTURE_MIPS: &[(&str, &str)] = &[
    ("none", "Use only the source level"),
    ("nearest", "Select the nearest mip level"),
    ("linear", "Interpolate between mip levels"),
];

pub const SCENE_MATERIAL_BLEND_MODES: &[(&str, &str)] = &[
    ("source-over", "Composite with source alpha"),
    ("screen", "Lighten using inverted multiplication"),
    ("multiply", "Darken by multiplying colors"),
];

pub const SCENE_DEPTH_DRAW_MODES: &[(&str, &str)] = &[
    (
        "opaque-only",
        "Write depth for opaque and alpha-cutout meshes",
    ),
    ("always", "Write depth for every surviving color fragment"),
    ("never", "Keep depth testing and disable depth writes"),
    (
        "opaque-prepass",
        "Write depth for opaque texels in a dedicated pass",
    ),
];

pub const SCENE_CULL_MODES: &[(&str, &str)] = &[
    ("back", "Discard back-facing triangles"),
    ("front", "Discard front-facing triangles"),
    ("none", "Render and pick both sides"),
];

pub const SCENE_MATERIAL_WORKFLOWS: &[(&str, &str)] = &[
    (
        "metallic-roughness",
        "Principled metallic and roughness factors",
    ),
    (
        "specular-glossiness",
        "Independent diffuse albedo, specular color, and glossiness",
    ),
];

pub const SCENE_ALPHA_MODES: &[(&str, &str)] = &[
    ("opaque", "Opaque surface with depth writes"),
    ("mask", "Cut out surfaces below alpha-cutoff"),
    ("blend", "Composite partially transparent surfaces"),
];

pub const SCENE_PHYSICS_BODY_KINDS: &[(&str, &str)] = &[
    ("fixed", "Stationary rigid body"),
    ("dynamic", "Rigid body moved by the solver"),
    ("kinematic", "Rigid body moved by authored targets"),
    ("trigger", "Nonblocking overlap sensor"),
];

pub const SCENE_PHYSICS_BODY_FIELDS: &[(&str, Type)] = &[
    ("id", Type::String),
    ("body-kind", Type::String),
    ("position", Type::Point3D),
];

pub const SCENE_PHYSICS_BODY_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("rotation", Type::Vector3D),
    ("mass", Type::Float),
    ("friction", Type::Float),
    ("restitution", Type::Float),
];

pub const SCENE_PHYSICS_JOINT_KINDS: &[(&str, &str)] = &[
    ("fixed", "Lock both local joint frames together"),
    ("spherical", "Join anchors and allow free rotation"),
    ("revolute", "Rotate around the local X axis"),
    ("prismatic", "Slide along the local X axis"),
    ("distance", "Constrain the distance between local anchors"),
];

pub const SCENE_PHYSICS_JOINT_FIELDS: &[(&str, Type)] = &[
    ("id", Type::String),
    ("joint-kind", Type::String),
    ("first", Type::String),
    ("second", Type::String),
    ("first-anchor", Type::Point3D),
    ("second-anchor", Type::Point3D),
];

pub const SCENE_PHYSICS_JOINT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("first-rotation", Type::Vector3D),
    ("second-rotation", Type::Vector3D),
    ("min", Type::Float),
    ("max", Type::Float),
];

pub const SCENE_ANIMATION_LAYER_FIELDS: &[(&str, Type)] = &[
    ("clip", Type::String),
    ("time", Type::Float),
    ("looping", Type::Bool),
    ("weight", Type::Float),
    ("root-node", Type::String),
    ("mode", Type::String),
];
pub const SCENE_ANIMATION_LAYER_MODES: &[&str] = &["override", "additive"];

pub const SCENE_ANIMATION_NODE_OVERRIDE_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("translation", Type::Vector3D),
    ("rotation", Type::Quaternion3D),
    ("scale", Type::Vector3D),
    ("weight", Type::Float),
];

pub const SCENE_COLLISION_SHAPE_FIELDS: &[(&str, Type)] = &[
    ("shape", Type::String),
    ("mesh", Type::String),
    ("position", Type::Point3D),
    ("size", Type::Vector3D),
    ("yaw", Type::Float),
    ("pitch", Type::Float),
    ("roll", Type::Float),
];

pub const SCENE_COLLISION_SHAPES: &[(&str, &str)] = &[
    ("box", "Centered box collider"),
    ("sphere", "Centered sphere collider"),
    ("cylinder", "Capped cylinder collider centered along Y"),
    (
        "cone",
        "Capped cone collider with its base at the local origin",
    ),
    ("capsule", "Centered capsule along the local Y axis"),
    ("mesh", "Use the triangle surfaces of a mesh resource"),
];

pub const SCENE_INSTANCE_DATA_FIELDS: [&str; 4] = ["x", "y", "z", "w"];

pub const INSTANCE_TABLE_ROW_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("scale", Type::Vector3D),
    ("rotation", Type::Vector3D),
    ("orientation", Type::Quaternion3D),
    ("color", Type::Color),
    ("data-x", Type::Float),
    ("data-y", Type::Float),
    ("data-z", Type::Float),
    ("data-w", Type::Float),
];

pub const SCENE_RANDOM_INSTANCE_FIELDS: &[(&str, Type)] = &[
    ("count", Type::Int),
    ("enabled", Type::Bool),
    ("seed", Type::Int),
    ("position-min", Type::Vector3D),
    ("position-max", Type::Vector3D),
    ("rotation-min", Type::Vector3D),
    ("rotation-max", Type::Vector3D),
    ("scale-min", Type::Vector3D),
    ("scale-max", Type::Vector3D),
    ("proportional-scale", Type::Bool),
    ("color-min", Type::Color),
    ("color-max", Type::Color),
    ("color-model", Type::String),
    ("proportional-color", Type::Bool),
    ("hue-min", Type::Float),
    ("hue-max", Type::Float),
    ("grid-spacing", Type::Vector3D),
];

pub const SCENE_RANDOM_COLOR_MODELS: &[(&str, &str)] = &[
    ("rgb", "Sample sRGB channels (default)"),
    ("hsv", "Sample hue, saturation and value"),
    ("hsl", "Sample hue, saturation and lightness"),
];

pub const ROTATION_KEY_FIELDS: &[(&str, Type)] =
    &[("time", Type::Float), ("orientation", Type::Quaternion3D)];

pub const SCENE_OBJECT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("model-blend-mode", Type::String),
    ("model-blend-order", Type::String),
    ("model-blend-end-node", Type::String),
    ("model-blend-activation-node", Type::String),
    ("model-blend-activation-speed", Type::Float),
    ("model-blend-time", Type::Float),
    ("model-blend-duration", Type::Float),
    ("model-blend-stagger", Type::Float),
    ("model-blend-arrival", Type::Float),
    ("model-blend-origin", Type::Point3D),
    ("model-blend-offset", Type::Vector3D),
    ("model-blend-orientation", Type::Quaternion3D),
    ("model-blend-scale", Type::Vector3D),
    ("model-blend-velocity", Type::Vector3D),
    ("model-blend-spread", Type::Vector3D),
    ("model-blend-gravity", Type::Vector3D),
    ("model-blend-spin", Type::Vector3D),
    ("model-blend-axis", Type::Vector3D),
    ("model-blend-seed", Type::Int),
    ("orientation", Type::Quaternion3D),
    ("depth-bias", Type::Float),
    ("motion-vector-enabled", Type::Bool),
    ("motion-vector-scale", Type::Float),
    ("instancing-lod-min", Type::Float),
    ("instancing-lod-max", Type::Float),
    ("instancing-lod-factor", Type::Float),
    ("physics-body", Type::String),
    ("mesh", Type::String),
    ("mesh-asset", Type::MeshAsset3D),
    ("material-variant", Type::String),
    ("instance-of", Type::String),
    ("instance-root", Type::String),
    ("instance-table", Type::InstanceTable3D),
    ("instance-count-override", Type::Int),
    ("instance-has-transparency", Type::Bool),
    ("instance-depth-sorting", Type::Bool),
    ("instance-shadow-bounds-min", Type::Vector3D),
    ("instance-shadow-bounds-max", Type::Vector3D),
    ("instance-template", Type::Bool),
    ("animation", Type::String),
    ("animation-time", Type::Float),
    ("animation-loop", Type::Bool),
    ("animation-to", Type::String),
    ("animation-to-time", Type::Float),
    ("animation-to-loop", Type::Bool),
    ("animation-blend", Type::Float),
    ("parent", Type::String),
    ("parent-node", Type::String),
    ("visible", Type::Bool),
    ("lod-of", Type::String),
    ("lod-max-size", Type::Length),
    ("lod-bias", Type::Float),
    ("camera-lod-bias", Type::Float),
    ("pickable", Type::Bool),
    ("collidable", Type::Bool),
    ("trigger", Type::Bool),
    ("pivot-x", Type::Float),
    ("pivot-y", Type::Float),
    ("pivot-z", Type::Float),
    ("camera-kind", Type::String),
    ("camera-fov", Type::Float),
    ("camera-fov-axis", Type::String),
    ("camera-height", Type::Float),
    ("camera-width", Type::Float),
    ("camera-look-at", Type::String),
    ("look-at", Type::String),
    ("look-at-up", Type::Vector3D),
    ("camera-look-at-point", Type::Point3D),
    ("camera-up", Type::Vector3D),
    ("camera-clip-normal", Type::Vector3D),
    ("camera-clip-space", Type::String),
    ("camera-reverse-depth", Type::Bool),
    ("camera-near", Type::Float),
    ("camera-far", Type::Float),
    ("camera-left", Type::Float),
    ("camera-right", Type::Float),
    ("camera-bottom", Type::Float),
    ("camera-top", Type::Float),
    ("casts-shadow", Type::Bool),
    ("receives-shadow", Type::Bool),
    ("receives-reflections", Type::Bool),
    ("casts-reflections", Type::Bool),
    ("layers", Type::Int),
    ("lightmap", Type::Image),
    ("lightmap-mode", Type::String),
    ("bake-role", Type::String),
    ("lightmap-size", Type::Int),
    ("lightmap-texels-per-unit", Type::Float),
    ("lightmap-intensity", Type::Float),
    ("lightmap-u", Type::Float),
    ("lightmap-v", Type::Float),
    ("lightmap-width", Type::Float),
    ("lightmap-height", Type::Float),
];

pub const MODEL_BLEND_MODES: &[(&str, &str)] = &[
    (
        "explode",
        "Release the model's triangles along particle trajectories",
    ),
    (
        "construct",
        "Assemble the model from triangles emitted at a local point",
    ),
    (
        "transfer",
        "Release and reassemble the model at a destination transform",
    ),
];

pub const MODEL_BLEND_ORDERS: &[(&str, &str)] = &[
    ("sequential", "Release triangles in mesh order"),
    ("random", "Use a reproducible per-triangle release time"),
    (
        "activation",
        "Sweep the release plane along the activation axis",
    ),
];

pub const SCENE_PARTICLE_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("color", Type::Color),
    ("size", Type::Float),
    ("lifetime", Type::Float),
];

pub const SCENE_CAPTURE_FIELDS: &[(&str, Type)] = &[("name", Type::String)];
pub const SCENE_CAPTURE_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("camera", Type::String),
    ("color-source", Type::String),
    ("color-format", Type::String),
    ("mipmaps", Type::Bool),
    ("material-data-format", Type::String),
    ("material-data-1-format", Type::String),
    ("material-data-2-format", Type::String),
    ("augment-shader", Type::String),
    ("augment-parameters", Type::Vector3D),
    ("combine-attachments", Type::Bool),
    ("depth-source", Type::String),
    ("depth-clear", Type::Float),
    ("stencil-clear", Type::Int),
    ("stencil-source", Type::String),
    ("renderables", Type::String),
    ("layers", Type::Int),
    ("scale", Type::Float),
    ("background", Type::Color),
    ("enabled", Type::Bool),
];

pub const SCENE_SCISSOR_FIELDS: &[(&str, Type)] = &[
    ("x", Type::Length),
    ("y", Type::Length),
    ("width", Type::Length),
    ("height", Type::Length),
    ("enabled", Type::Bool),
];

pub const SCENE_CAPTURE_RECT_FIELDS: &[&str] = &["x", "y", "width", "height"];

pub const SCENE_CAPTURE_PIPELINE_FIELDS: &[(&str, Type)] = &[
    ("enabled", Type::Bool),
    ("depth-test", Type::Bool),
    ("depth-write", Type::Bool),
    ("depth-compare", Type::String),
    ("color-write", Type::Bool),
    ("polygon-mode", Type::String),
    ("cull-mode", Type::String),
    ("blend-mode", Type::String),
];

pub const SCENE_CAPTURE_POLYGON_MODES: &[(&str, &str)] = &[
    ("fill", "Rasterize the complete triangle surface"),
    ("line", "Rasterize triangle edges as a wireframe"),
];

pub const SCENE_CAPTURE_STENCIL_FIELDS: &[(&str, Type)] = &[
    ("enabled", Type::Bool),
    ("reference", Type::Int),
    ("read-mask", Type::Int),
    ("write-mask", Type::Int),
    ("compare", Type::String),
    ("fail", Type::String),
    ("depth-fail", Type::String),
    ("pass", Type::String),
];

pub const SCENE_CAPTURE_STENCIL_OPS: &[(&str, &str)] = &[
    ("keep", "Preserve the existing stencil value"),
    ("zero", "Write zero"),
    ("replace", "Write the stencil reference"),
    ("invert", "Invert each selected bit"),
    ("increment-clamp", "Increment up to 255"),
    ("decrement-clamp", "Decrement down to zero"),
    ("increment-wrap", "Increment, wrapping from 255 to zero"),
    ("decrement-wrap", "Decrement, wrapping from zero to 255"),
];

pub const SCENE_CAPTURE_DEPTH_COMPARES: &[(&str, &str)] = &[
    ("material", "Use the material's depth comparison"),
    ("never", "Reject every fragment"),
    ("less", "Accept fragments nearer than the stored depth"),
    ("less-equal", "Accept nearer fragments and equal depth"),
    ("equal", "Accept fragments at the stored depth"),
    ("not-equal", "Reject fragments at the stored depth"),
    ("greater-equal", "Accept farther fragments and equal depth"),
    ("greater", "Accept fragments farther than the stored depth"),
    ("always", "Accept every fragment"),
];

pub const SCENE_CAPTURE_BLENDS: &[(&str, &str)] = &[
    ("material", "Use the material's blend mode"),
    ("replace", "Write incoming color directly"),
    (
        "source-over",
        "Composite premultiplied color over the existing color",
    ),
    ("additive", "Add incoming color to the existing color"),
    ("multiply", "Multiply existing color"),
    ("screen", "Screen blend with existing color"),
];

pub const SCENE_CAPTURE_TARGET_BLEND_FIELDS: &[(&str, Type)] = &[
    ("enabled", Type::Bool),
    ("blend-enabled", Type::Bool),
    ("write-mask", Type::Int),
    ("constant", Type::Color),
    ("source-color", Type::String),
    ("destination-color", Type::String),
    ("color-operation", Type::String),
    ("source-alpha", Type::String),
    ("destination-alpha", Type::String),
    ("alpha-operation", Type::String),
];

pub const SCENE_CAPTURE_BLEND_FACTORS: &[(&str, &str)] = &[
    ("zero", "Multiply by zero"),
    ("one", "Multiply by one"),
    ("src", "Multiply by the fragment channels"),
    (
        "one-minus-src",
        "Multiply by one minus the fragment channels",
    ),
    ("src-alpha", "Multiply by the fragment alpha"),
    (
        "one-minus-src-alpha",
        "Multiply by one minus the fragment alpha",
    ),
    ("dst", "Multiply by the attachment channels"),
    (
        "one-minus-dst",
        "Multiply by one minus the attachment channels",
    ),
    ("dst-alpha", "Multiply by the attachment alpha"),
    (
        "one-minus-dst-alpha",
        "Multiply by one minus the attachment alpha",
    ),
    (
        "src-alpha-saturated",
        "Multiply RGB by min(source alpha, 1 - destination alpha)",
    ),
    ("constant", "Multiply by the blend constant"),
    (
        "one-minus-constant",
        "Multiply by one minus the blend constant",
    ),
];

pub const SCENE_CAPTURE_BLEND_OPERATIONS: &[(&str, &str)] = &[
    ("add", "Add the scaled fragment and attachment values"),
    (
        "subtract",
        "Subtract the scaled attachment from the scaled fragment",
    ),
    (
        "reverse-subtract",
        "Subtract the scaled fragment from the scaled attachment",
    ),
    ("min", "Keep the lower unscaled channel value"),
    ("max", "Keep the higher unscaled channel value"),
];

pub const SCENE_CAPTURE_RENDERABLES: &[(&str, &str)] = &[
    ("all", "Render opaque and transparent geometry"),
    ("opaque", "Render opaque and alpha-masked geometry"),
    (
        "transparent",
        "Render blended geometry and translucent particles",
    ),
    (
        "none",
        "Compose capture inputs over the background without geometry",
    ),
];

pub const SCENE_EFFECT_FIELDS: &[(&str, Type)] = &[("shader", Type::String)];

pub const SCENE_EFFECT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("strength", Type::Float),
    ("depth", Type::Bool),
    ("scale", Type::Float),
    ("parameters", Type::Vector3D),
    ("output", Type::String),
    ("input", Type::String),
    ("history", Type::Bool),
    ("output-format", Type::String),
];

pub const SCENE_EFFECT_FORMATS: &[(&str, &str)] = &[
    (
        "rgba32f",
        "Four 32-bit float channels for high-precision linear color and alpha",
    ),
    (
        "rgba16f",
        "Four 16-bit float channels for HDR color and alpha",
    ),
    (
        "rgba8",
        "Four normalized 8-bit channels for linear color and alpha",
    ),
    ("rg32f", "Two 32-bit float data channels"),
    ("rg16f", "Two 16-bit float data channels"),
    ("rg8", "Two normalized 8-bit data channels"),
    ("r32f", "One 32-bit float data channel"),
    ("r16f", "One 16-bit float data channel"),
    ("r16", "One normalized 16-bit data channel"),
    ("r8", "One normalized 8-bit data channel"),
];

pub const SCENE_CAPTURE_ATTACHMENTS: &[(&str, &str)] = &[
    ("color", "Scene-linear HDR color"),
    ("depth", "Raw camera depth, sampled at the nearest pixel"),
    (
        "normal-roughness",
        "World-space normal and material roughness",
    ),
    (
        "base-color-metalness",
        "Linear base color and material metalness",
    ),
    (
        "emissive-occlusion",
        "Linear HDR emission and material occlusion",
    ),
    (
        "material-data",
        "First programmable material data attachment",
    ),
    (
        "material-data-1",
        "Second programmable material data attachment",
    ),
    (
        "material-data-2",
        "Third programmable material data attachment",
    ),
];

pub const SCENE_EFFECT_COLOR_SPACES: &[(&str, &str)] = &[
    ("srgb", "Decode a color texture from sRGB to linear RGB"),
    ("linear", "Read texture channels as linear data"),
];

pub const SCENE_PARTICLE_EMISSION_ANIMATION_FIELDS: &[(&str, Type)] = &[
    ("mesh", Type::String),
    ("mesh-asset", Type::MeshAsset3D),
    ("node", Type::String),
    ("clip", Type::String),
    ("sample-rate", Type::Float),
    ("position", Type::Point3D),
    ("orientation", Type::Quaternion3D),
    ("scale", Type::Vector3D),
];

pub const SCENE_PARTICLE_EMISSION_KEY_FIELDS: &[(&str, Type)] = &[
    ("time", Type::Float),
    ("position", Type::Point3D),
    ("orientation", Type::Quaternion3D),
];

pub const SCENE_PARTICLE_BURST_FIELDS: &[(&str, Type)] =
    &[("time", Type::Float), ("count", Type::Int)];

pub const SCENE_PARTICLE_BURST_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("duration", Type::Float),
    ("count-variation", Type::Int),
    ("enabled", Type::Bool),
    ("position", Type::Point3D),
    ("orientation", Type::Quaternion3D),
    ("seed", Type::Int),
];

pub const SCENE_PARTICLE_COLOR_VARIATION_FIELDS: &[(&str, Type)] = &[
    ("red", Type::Float),
    ("green", Type::Float),
    ("blue", Type::Float),
    ("alpha", Type::Float),
];

pub const SCENE_PARTICLE_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("emitter-id", Type::String),
    ("follow-emitter", Type::String),
    ("follow-burst-at", Type::String),
    ("emission-loop", Type::Bool),
    ("particle-group", Type::String),
    ("parent", Type::String),
    ("parent-node", Type::String),
    ("lifetime-variation", Type::Float),
    ("reversed", Type::Bool),
    ("size-variation", Type::Float),
    ("end-size-variation", Type::Float),
    ("end-size", Type::Float),
    ("start-time", Type::Float),
    ("rate", Type::Float),
    ("velocity", Type::Vector3D),
    ("velocity-spread", Type::Vector3D),
    ("gravity", Type::Vector3D),
    ("fade-in", Type::Float),
    ("fade-out", Type::Float),
    ("fade-in-effect", Type::String),
    ("fade-out-effect", Type::String),
    ("unified-color-variation", Type::Bool),
    ("intensity", Type::Float),
    ("seed", Type::Int),
    ("layers", Type::Int),
    ("enabled", Type::Bool),
    ("sprite", Type::Image),
    ("sprite-frames", Type::Int),
    ("sprite-frame", Type::Int),
    ("sprite-duration", Type::Float),
    ("sprite-duration-variation", Type::Float),
    ("sprite-playback", Type::String),
    ("sprite-interpolate", Type::Bool),
    ("sprite-random-start", Type::Bool),
    ("sprite-billboard", Type::Bool),
    ("sprite-offset-x", Type::Float),
    ("sprite-offset-y", Type::Float),
    ("color-table", Type::Image),
    ("particle-blend-mode", Type::String),
    ("particle-sort-mode", Type::String),
    ("depth-bias", Type::Float),
    ("depth-fade-distance", Type::Float),
    ("emitter-shape", Type::String),
    ("emitter-mesh", Type::String),
    ("emitter-point-source", Type::String),
    ("emitter-point-randomize", Type::Bool),
    ("emitter-point-scale", Type::Vector3D),
    ("emitter-scene-root", Type::String),
    ("emitter-scene-exclude", Type::StringList),
    ("emitter-scene-center", Type::Point3D),
    ("emitter-scene-extents", Type::Vector3D),
    ("emitter-mesh-scale", Type::Vector3D),
    ("emitter-extents", Type::Vector3D),
    ("emitter-fill", Type::Bool),
    ("emitter-yaw", Type::Float),
    ("emitter-pitch", Type::Float),
    ("emitter-roll", Type::Float),
    ("velocity-mode", Type::String),
    ("velocity-cone-angle", Type::Float),
    ("velocity-target", Type::Point3D),
    ("velocity-target-spread", Type::Vector3D),
    ("velocity-target-normalized", Type::Bool),
    ("velocity-target-magnitude", Type::Float),
    ("velocity-speed-variation", Type::Float),
    ("trail-segments", Type::Int),
    ("trail-duration", Type::Float),
    ("trail-end-scale", Type::Float),
    ("trail-fade", Type::Float),
    ("trail-eol-fade", Type::Float),
    ("trail-length", Type::Float),
    ("trail-length-variation", Type::Float),
    ("trail-length-delta-min", Type::Float),
    ("trail-scale-multiplier", Type::Float),
    ("trail-alpha-fade", Type::Float),
    ("trail-texcoord-mode", Type::String),
    ("trail-texcoord-multiplier", Type::Float),
    ("particle-instances-only", Type::Bool),
    ("particle-model", Type::String),
    ("particle-model-scene", Type::SceneModel3D),
    ("particle-align-mode", Type::String),
    ("particle-align-target", Type::Point3D),
    ("particle-rotation", Type::Vector3D),
    ("particle-rotation-variation", Type::Vector3D),
    ("particle-angular-velocity", Type::Vector3D),
    ("particle-angular-velocity-variation", Type::Vector3D),
    ("particle-casts-shadow", Type::Bool),
    ("particle-receives-shadow", Type::Bool),
    ("particle-casts-reflections", Type::Bool),
];

pub const SCENE_PARTICLE_AFFECTOR_FIELDS: &[(&str, Type)] = &[("affector-kind", Type::String)];
pub const SCENE_PARTICLE_INSTANCE_MODEL_FIELDS: &[(&str, Type)] =
    &[("emitter", Type::String), ("model", Type::SceneModel3D)];

pub const SCENE_PARTICLE_AFFECTOR_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("parent", Type::String),
    ("parent-node", Type::String),
    ("particle-group", Type::String),
    ("position", Type::Point3D),
    ("position-variation", Type::Vector3D),
    ("attractor-shape", Type::String),
    ("attractor-extents", Type::Vector3D),
    ("attractor-fill", Type::Bool),
    ("attractor-yaw", Type::Float),
    ("attractor-pitch", Type::Float),
    ("attractor-roll", Type::Float),
    ("attractor-mesh", Type::String),
    ("attractor-point-source", Type::String),
    ("attractor-point-randomize", Type::Bool),
    ("attractor-point-scale", Type::Vector3D),
    ("attractor-scene-root", Type::String),
    ("attractor-scene-exclude", Type::StringList),
    ("attractor-scene-center", Type::Point3D),
    ("attractor-scene-extents", Type::Vector3D),
    ("attractor-mesh-scale", Type::Vector3D),
    ("attractor-samples", Type::Int),
    ("direction", Type::Vector3D),
    ("duration", Type::Float),
    ("duration-variation", Type::Float),
    ("start-age", Type::Float),
    ("hide-at-end", Type::Bool),
    ("radius", Type::Float),
    ("outer-radius", Type::Float),
    ("strength", Type::Float),
    ("angular-speed", Type::Float),
    ("global-amount", Type::Vector3D),
    ("global-pace", Type::Vector3D),
    ("global-pace-start", Type::Vector3D),
    ("unique-amount", Type::Vector3D),
    ("unique-pace", Type::Vector3D),
    ("unique-amount-variation", Type::Float),
    ("unique-pace-variation", Type::Float),
    ("fade-in-duration", Type::Float),
    ("fade-out-duration", Type::Float),
    ("min-size", Type::Float),
    ("max-size", Type::Float),
    ("scale-mode", Type::String),
    ("scale-easing", Type::String),
    ("seed", Type::Int),
    ("layers", Type::Int),
    ("enabled", Type::Bool),
];
pub const PARTICLE_AFFECTOR_KINDS: &[(&str, &str)] = &[
    (
        "attractor",
        "Pull particles toward a destination over a fixed duration",
    ),
    (
        "repeller",
        "Displace particles outward within a falloff region",
    ),
    ("gravity", "Add constant world-space acceleration"),
    (
        "point-rotator",
        "Rotate particles around a shared world-space pivot",
    ),
    (
        "wander",
        "Offset each particle with shared and seeded local waves",
    ),
    (
        "scale",
        "Multiply particle size by a repeating age-based curve",
    ),
];

pub const PARTICLE_SCALE_MODES: &[(&str, &str)] = &[
    ("linear", "Ramp from minimum to maximum each cycle"),
    ("triangle", "Ramp up and down each cycle"),
    ("sine", "Smooth sine wave across the size range"),
    ("abs-sine", "Two smooth positive pulses per cycle"),
    (
        "step",
        "Switch from minimum to maximum halfway through the cycle",
    ),
    (
        "smooth-step",
        "Smoothly interpolate between minimum and maximum",
    ),
];

pub const PARTICLE_SCALE_EASINGS: &[(&str, &str)] = &[
    ("linear", "Constant speed"),
    ("ease-in", "Cubic acceleration"),
    ("ease-out", "Cubic deceleration"),
    ("ease-in-out", "Cubic acceleration and deceleration"),
];

pub const PARTICLE_EMITTER_SHAPES: &[(&str, &str)] = &[
    (
        "points",
        "Emit from positions authored in a CBOR point cloud",
    ),
    ("point", "Emit from the origin"),
    ("mesh", "Emit from the surface of a glTF mesh"),
    (
        "scene",
        "Emit from the visible mesh surfaces in a scene subtree",
    ),
    ("box", "Emit from a box volume or its faces"),
    ("sphere", "Emit from an ellipsoid volume or its surface"),
    (
        "cylinder",
        "Emit from a Y-axis cylinder with an elliptical cross section",
    ),
];

pub const PARTICLE_ATTRACTOR_SHAPES: &[(&str, &str)] = &[
    (
        "points",
        "Attract to positions authored in a CBOR point cloud",
    ),
    (
        "scene",
        "Attract to visible mesh surfaces in a scene subtree",
    ),
    ("point", "Attract to a single point"),
    ("mesh", "Attract to points on a glTF mesh surface"),
    ("box", "Attract to points within a box or on its faces"),
    (
        "sphere",
        "Attract to points within an ellipsoid or on its surface",
    ),
    (
        "cylinder",
        "Attract to points within a cylinder or on its surface",
    ),
];

pub const PARTICLE_SORT_MODES: &[(&str, &str)] = &[
    ("none", "Use the regular particle draw order"),
    ("newest", "Draw the newest particles first"),
    ("oldest", "Draw the oldest particles first"),
    (
        "distance",
        "Draw particles from farthest to nearest in camera depth",
    ),
];

pub const FOLLOW_BURST_EVENTS: &[(&str, &str)] = &[
    (
        "birth",
        "Schedule bursts at the source particle's birth position",
    ),
    (
        "death",
        "Schedule bursts at the source particle's final position",
    ),
];

pub const TRAIL_TEXCOORD_MODES: &[(&str, &str)] = &[
    ("stretch", "Stretch once over the sampled time interval"),
    (
        "fill",
        "Map one texture over the visible world-space trail length",
    ),
    ("relative", "Repeat by world distance from the trail head"),
    ("absolute", "Repeat by world distance traveled since birth"),
];

pub const PARTICLE_FADE_EFFECTS: &[(&str, &str)] = &[
    (
        "opacity",
        "Ramp the particle's opacity across the fade duration",
    ),
    ("scale", "Ramp the particle's size across the fade duration"),
    (
        "none",
        "Keep the particle's opacity and size during the fade interval",
    ),
];

pub const PARTICLE_ALIGNMENT_MODES: &[(&str, &str)] = &[
    (
        "none",
        "Use the particle's authored rotation in world space",
    ),
    (
        "start-velocity",
        "Align local +Z with the particle's initial velocity",
    ),
    (
        "velocity",
        "Align local +Z with the particle's current trajectory, including affectors",
    ),
    (
        "target",
        "Align local +Z with the world-space particle-align-target",
    ),
];

pub const PARTICLE_VELOCITY_MODES: &[(&str, &str)] = &[
    (
        "surface-normal",
        "Emit along the sampled normal. A follower without its own surface uses the source particle's normal.",
    ),
    (
        "surface-reflected",
        "Reflect around the sampled world-space normal. A follower can use the source birth direction at its own speed.",
    ),
    (
        "vector",
        "Apply component-wise variation to the velocity vector",
    ),
    (
        "normalized-vector",
        "Apply direction variation while preserving the nominal speed",
    ),
    (
        "cone",
        "Sample directions within a solid angle around the velocity",
    ),
    (
        "target",
        "Aim at a world-space target with optional distance scaling and signed magnitude",
    ),
];

pub const PARTICLE_BLEND_MODES: &[(&str, &str)] = &[
    (
        "source-over",
        "Composite premultiplied color over the destination",
    ),
    ("additive", "Add emitted light to the destination"),
    ("screen", "Lighten the destination using screen blending"),
    ("multiply", "Darken the destination with a color filter"),
];

pub const PARTICLE_SPRITE_PLAYBACK: &[(&str, &str)] = &[
    ("loop", "Play frames from left to right and loop"),
    ("reverse", "Play frames from right to left and loop"),
    ("ping-pong", "Reverse each time the sequence reaches an end"),
    (
        "ping-pong-reverse",
        "Play a ping-pong sequence starting from the last frame",
    ),
    ("once", "Play once and hold the last frame"),
    (
        "single-frame",
        "Hold the initial frame for the particle lifetime",
    ),
];

pub const SCENE_FOG_VOLUME_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("size", Type::Vector3D),
    ("density", Type::Float),
    ("color", Type::Color),
];

pub const SCENE_FOG_VOLUME_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("fog-shape", Type::String),
    ("visible", Type::Bool),
    ("layers", Type::Int),
    ("yaw", Type::Float),
    ("pitch", Type::Float),
    ("roll", Type::Float),
    ("fog-height-enabled", Type::Bool),
    ("fog-height-start", Type::Float),
    ("fog-height-end", Type::Float),
    ("fog-noise-amount", Type::Float),
    ("fog-noise-scale", Type::Float),
    ("fog-noise-offset", Type::Vector3D),
];

pub const FOG_VOLUME_SHAPES: &[(&str, &str)] = &[
    ("box", "Box, optionally rotated by yaw, pitch and roll"),
    ("sphere", "Sphere or ellipsoid with the specified size"),
];

pub const SCENE_REFLECTION_FIELDS: &[(&str, Type)] =
    &[("position", Type::Point3D), ("box-size", Type::Vector3D)];

pub const SCENE_REFLECTION_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("source", Type::String),
    ("capture-size", Type::Int),
    ("capture-revision", Type::Int),
    ("refresh-mode", Type::String),
    ("time-slicing", Type::String),
    ("layers", Type::Int),
    ("box-offset", Type::Vector3D),
    ("visible", Type::Bool),
    ("yaw", Type::Float),
    ("pitch", Type::Float),
    ("roll", Type::Float),
    ("intensity", Type::Float),
    ("parallax-correction", Type::Bool),
    ("debug-view", Type::Bool),
];

pub const REFLECTION_REFRESH_MODES: &[(&str, &str)] = &[
    (
        "first-frame",
        "Capture once, then refresh when capture-revision changes",
    ),
    ("on-change", "Refresh after a change to the captured scene"),
    ("every-frame", "Capture each time the viewport renders"),
];

pub const HEIGHTFIELD_EXTENSION: &str = "png";
pub const MESH_LOADER_INPUTS: &[(&str, Type)] = &[
    ("source", Type::String),
    ("active", Type::Bool),
    ("revision", Type::Int),
    ("auto-lod", Type::Bool),
    ("lightmap-uv", Type::Bool),
];

pub const MESH_LOADER_OUTPUTS: &[(&str, Type)] = &[
    ("asset", Type::MeshAsset3D),
    ("bounds", Type::Bounds3D),
    ("status", Type::String),
    ("error", Type::String),
];

pub fn mesh_loader_output_type(kind: &str, property: &str) -> Option<Type> {
    if kind != "MeshLoader3D" {
        return None;
    }
    MESH_LOADER_OUTPUTS
        .iter()
        .find(|(name, _)| *name == property)
        .map(|(_, ty)| ty.clone())
}

pub const OBJ_EXTENSION: &str = "obj";
pub const MESH_SOURCE_EXTENSIONS: &[&str] = &["glb", "gltf", OBJ_EXTENSION, HEIGHTFIELD_EXTENSION];

pub const MESH_URL_OPTIONS: &[(&str, &str)] = &[
    ("auto-lod", "Generate mesh levels of detail"),
    (
        "lightmap-uv",
        "Generate independent nonoverlapping UV charts for lightmap baking",
    ),
];

pub const SCENE_BAKE_ROLES: &[(&str, &str)] = &[
    (
        "auto",
        "Contribute and receive an atlas region when eligible (default)",
    ),
    (
        "receiver",
        "Require an atlas region and contribute to baking",
    ),
    (
        "contributor",
        "Contribute shadows and bounced light without an atlas region",
    ),
    ("excluded", "Keep the model outside the offline bake"),
];

pub const LIGHTMAP_MODES: &[(&str, &str)] = &[
    (
        "indirect",
        "Replace ambient diffuse lighting and keep real-time direct lighting",
    ),
    (
        "diffuse",
        "Replace all diffuse lighting and keep view-dependent specular",
    ),
    (
        "mixed",
        "Combine a mixed bake with the lights still evaluated in real time",
    ),
];

pub const SCENE_RENDER_MODES: &[(&str, &str)] = &[
    (
        "offscreen",
        "Render into a texture and compose it in paint order.",
    ),
    (
        "inline",
        "Draw eligible viewports directly into the window in paint order.",
    ),
    (
        "underlay",
        "Draw a window's direct child viewport behind ordinary window content.",
    ),
    (
        "overlay",
        "Draw a window's direct child viewport above ordinary window content.",
    ),
];

pub const SCENE_OIT_METHODS: &[(&str, &str)] = &[
    ("sorted", "Sort transparent submeshes by camera depth."),
    (
        "weighted",
        "Approximate order-independent blending with weighted accumulation.",
    ),
    (
        "linked-list",
        "Sort visible fragments per pixel, with weighted fallback when the fragment budget is exceeded.",
    ),
];

pub const SCENE_MATERIAL_DEBUG_MODES: &[(&str, &str)] = &[
    ("none", "Show regular material shading"),
    ("base-color", "Show resolved base color without lighting"),
    ("roughness", "Show sampled material roughness"),
    ("metalness", "Show sampled material metalness"),
    ("emission", "Show emitted material radiance"),
    (
        "ambient-occlusion",
        "Show combined material and screen-space visibility",
    ),
    ("normals", "Encode the resolved world-space normal as RGB"),
    ("tangents", "Encode the world-space material tangent as RGB"),
    (
        "binormals",
        "Encode the world-space material binormal as RGB",
    ),
    ("f0", "Show reflectance at perpendicular incidence"),
    ("diffuse", "Show only the diffuse lighting contribution"),
    ("specular", "Show only the specular lighting contribution"),
    ("shadow-occlusion", "Show direct shadow visibility"),
];

pub const SCENE_SHADOW_PRECISIONS: &[(&str, &str)] = &[
    ("depth32", "Use 32-bit floating-point shadow depth"),
    ("depth16", "Use 16-bit normalized shadow depth"),
];

pub const SCENE_RENDER_FORMATS: &[(&str, &str)] = &[
    (
        "rgba8",
        "8-bit sRGB viewport output for ordinary composition",
    ),
    (
        "rgba16f",
        "16-bit float linear viewport output for materials and effects",
    ),
    (
        "rgba32f",
        "32-bit float linear viewport output on GPUs with float32 filtering",
    ),
];

pub const SCENE_RENDER_OVERRIDES: &[(&str, &str)] = &[
    ("none", "Render the default 3D viewport passes."),
    (
        "disable-internal-passes",
        "Produce the viewport from explicit scene captures and WGSL effects.",
    ),
];

pub const TEMPORAL_AA_MODES: &[(&str, &str)] = &[
    ("blend", "Blend the current and previous raw frames"),
    (
        "motion-vectors",
        "Reproject visible surfaces with motion and depth",
    ),
];

pub const REFLECTION_TIME_SLICING: &[(&str, &str)] = &[
    (
        "none",
        "Capture and prefilter the whole cubemap in one frame",
    ),
    (
        "individual-faces",
        "Capture one face per frame, then prefilter one face per frame",
    ),
    (
        "all-faces-at-once",
        "Capture all faces, then prefilter one roughness mip per frame",
    ),
];

pub const SCENE_MESH_VERTEX_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("normal", Type::Vector3D),
    ("tangent", Type::Vector3D),
    ("tangent-sign", Type::Float),
    ("u", Type::Float),
    ("v", Type::Float),
    ("u1", Type::Float),
    ("v1", Type::Float),
    ("color", Type::Color),
];

pub const SCENE_SKIN_WEIGHT_FIELDS: &[(&str, Type)] = &[
    ("vertex", Type::Int),
    ("joint", Type::Int),
    ("weight", Type::Float),
];

pub const SCENE_MORPH_DELTA_FIELDS: &[(&str, Type)] =
    &[("target", Type::String), ("vertex", Type::Int)];

pub const SCENE_MORPH_DELTA_CHANNELS: &[(&str, Type)] = &[
    ("position", Type::Vector3D),
    ("normal", Type::Vector3D),
    ("tangent", Type::Vector3D),
    ("binormal", Type::Vector3D),
    ("u", Type::Float),
    ("v", Type::Float),
    ("u1", Type::Float),
    ("v1", Type::Float),
    ("color-r", Type::Float),
    ("color-g", Type::Float),
    ("color-b", Type::Float),
    ("color-a", Type::Float),
];

pub const SCENE_MESH_SUBSET_FIELDS: &[(&str, Type)] = &[
    ("offset", Type::Int),
    ("count", Type::Int),
    ("name", Type::String),
];

pub const SCENE_PARAMETRIC_FIELDS: &[(&str, Type)] = &[
    ("shape", Type::String),
    ("width", Type::Float),
    ("height", Type::Float),
    ("depth", Type::Float),
    ("radius", Type::Float),
    ("top-radius", Type::Float),
    ("tube-radius", Type::Float),
    ("segments", Type::Int),
    ("rings", Type::Int),
    ("capped", Type::Bool),
    ("text", Type::String),
    ("font", Type::Font),
    ("text-scale", Type::Float),
    ("asynchronous", Type::Bool),
];

pub const SCENE_PARAMETRIC_SHAPES: &[(&str, &str)] = &[
    ("plane", "Subdivided XY plane"),
    ("box", "Box with six material slots"),
    (
        "sphere",
        "UV sphere with configurable radius and tessellation",
    ),
    ("cylinder", "Cylinder or frustum with optional end caps"),
    ("cone", "Cone with configurable top radius"),
    ("capsule", "Y-aligned capsule with hemispherical ends"),
    (
        "torus",
        "Ring in the XY plane with customizable tube radius",
    ),
    ("text", "Extruded text shaped with the selected font"),
];

pub const SCENE_PRIMITIVE_MESHES: &[(&str, &str)] = &[
    ("primitive:box", "Unit box centered at its origin"),
    ("primitive:sphere", "Unit-radius sphere"),
    (
        "primitive:rectangle",
        "Two-sided unit rectangle in the XY plane",
    ),
    ("primitive:cylinder", "Capped cylinder centered along Y"),
    ("primitive:cone", "Capped cone from Y = 0 to Y = 1"),
];

pub fn scene_primitive_slots(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "primitive:box" | "primitive:sphere" => Some(&[""]),
        "primitive:rectangle" => Some(&["surface"]),
        "primitive:cylinder" => Some(&["side", "top", "bottom"]),
        "primitive:cone" => Some(&["side", "base"]),
        _ => None,
    }
}

pub const SCENE_CAMERA_KINDS: &[(&str, &str)] = &[
    ("custom", "Use the camera-projection matrix"),
    (
        "perspective",
        "Perspective camera with an angular field of view",
    ),
    (
        "orthographic",
        "Orthographic camera with a fixed world-space height",
    ),
    (
        "frustum",
        "Perspective camera with explicit near-plane bounds",
    ),
];

pub const SCENE_CAMERA_CLIP_SPACES: &[(&str, &str)] = &[
    ("webgpu", "Clip depth from zero to one"),
    ("opengl", "Clip depth from minus one to one"),
];

pub const SCENE_CAMERA_FOV_AXES: &[(&str, &str)] = &[
    (
        "vertical",
        "Hold the vertical field of view fixed as the viewport resizes",
    ),
    (
        "horizontal",
        "Hold the horizontal field of view fixed as the viewport resizes",
    ),
];

pub const SCENE_LIGHT_KINDS: &[(&str, &str)] = &[
    ("directional", "Illuminate uniformly with a parallel light"),
    (
        "point",
        "Illuminate from a position with distance attenuation",
    ),
    (
        "spot",
        "Illuminate through a cone with distance attenuation",
    ),
];

pub const SCENE_LIGHT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("layers", Type::Int),
    ("casts-shadow", Type::Bool),
    ("shadow-filter", Type::String),
    ("shadow-strength", Type::Float),
    ("shadow-radius", Type::Float),
    ("shadow-world-radius", Type::Float),
    ("shadow-bias", Type::Float),
    ("shadow-map-far", Type::Float),
    ("shadow-map-size", Type::Int),
    ("shadow-map-precision", Type::String),
    ("shadow-cascades", Type::Int),
    ("shadow-split-1", Type::Float),
    ("shadow-split-2", Type::Float),
    ("shadow-split-3", Type::Float),
    ("shadow-split-balance", Type::Float),
    ("shadow-cascade-blend", Type::Float),
    ("shadow-lock-texels", Type::Bool),
    ("bake-mode", Type::String),
    ("scope", Type::String),
    ("parent", Type::String),
    ("visible", Type::Bool),
];

pub const SCENE_LIGHT_BAKE_MODES: &[(&str, &str)] = &[
    (
        "disabled",
        "Keep the light entirely real-time in a mixed bake (default)",
    ),
    (
        "indirect",
        "Bake bounced light and keep the direct light real-time",
    ),
    ("all", "Bake direct diffuse light and bounces"),
];

pub const SCENE_SHADOW_FILTERS: &[(&str, &str)] = &[
    ("hard", "Compare one shadow texel"),
    (
        "bilinear",
        "Blend the four neighboring depth comparisons (default)",
    ),
    ("pcf4", "Soften the shadow with four filtered taps"),
    ("pcf8", "Soften the shadow with eight filtered taps"),
    ("pcf16", "Soften the shadow with sixteen filtered taps"),
    ("pcf32", "Soften the shadow with thirty-two disk samples"),
    ("pcf64", "Soften the shadow with sixty-four disk samples"),
];

pub const SCENE_LIGHT_FIELDS: &[(&str, Type)] = &[
    ("kind", Type::String),
    ("x", Type::Float),
    ("y", Type::Float),
    ("z", Type::Float),
    ("yaw", Type::Float),
    ("elevation", Type::Float),
    ("intensity", Type::Float),
    ("color", Type::Color),
    ("range", Type::Float),
    ("inner-angle", Type::Float),
    ("outer-angle", Type::Float),
];

const fn geometry(
    kind: GeometryFunction,
    parameters: &'static [Parameter],
    documentation: &'static str,
) -> Function {
    let (namespace, member, result) = match kind {
        GeometryFunction::Prepare3D => ("Scene3D", "prepare", "scenemodel3d"),
        GeometryFunction::Combine3D => ("Scene3D", "combine", "scenemodel3d"),
        GeometryFunction::Bounds3D => ("Scene3D", "bounds", "bounds3d"),
        GeometryFunction::GeometryStatus3D => ("Scene3D", "geometry-status", "string"),
        GeometryFunction::GeometryError3D => ("Scene3D", "geometry-error", "string"),
        GeometryFunction::MeshNode3D => ("Scene3D", "mesh-node", "meshnode3d"),
        GeometryFunction::SceneNode3D => ("Scene3D", "scene-node", "meshnode3d"),
        GeometryFunction::MeshNodeIndices3D => ("Scene3D", "mesh-node-indices", "[int]"),
        GeometryFunction::MeshMaterialSlots3D => ("Scene3D", "mesh-material-slots", "[string]"),
        GeometryFunction::MeshMaterialVariants3D => {
            ("Scene3D", "mesh-material-variants", "[string]")
        }
        GeometryFunction::MeshAnimationClips3D => ("Scene3D", "mesh-animation-clips", "[string]"),
        GeometryFunction::MeshAnimationDuration3D => {
            ("Scene3D", "mesh-animation-duration", "float")
        }
        GeometryFunction::TextureRgba8 => ("TextureData2D", "rgba8", "texturedata2d"),
        GeometryFunction::PrepareInstanceTable3D => {
            ("Scene3D", "instance-table", "instancetable3d")
        }
        GeometryFunction::RandomInstanceTable3D => {
            ("Scene3D", "random-instance-table", "instancetable3d")
        }
        GeometryFunction::TableInstance3D => ("Scene3D", "table-instance", "instance3d"),
        GeometryFunction::Point => ("Point2D", "new", "point2d"),
        GeometryFunction::Point3D => ("Point3D", "new", "point3d"),
        GeometryFunction::Vector3D => ("Vector3D", "new", "vector3d"),
        GeometryFunction::QuaternionIdentity => ("Quaternion3D", "identity", "quaternion3d"),
        GeometryFunction::QuaternionNew => ("Quaternion3D", "new", "quaternion3d"),
        GeometryFunction::QuaternionAxisAngle => {
            ("Quaternion3D", "from-axis-angle", "quaternion3d")
        }
        GeometryFunction::QuaternionEuler => ("Quaternion3D", "from-euler", "quaternion3d"),
        GeometryFunction::QuaternionLookRotation => {
            ("Quaternion3D", "look-rotation", "quaternion3d")
        }
        GeometryFunction::QuaternionLookAt => ("Quaternion3D", "look-at", "quaternion3d"),
        GeometryFunction::QuaternionCompose => ("Quaternion3D", "compose", "quaternion3d"),
        GeometryFunction::QuaternionInverse => ("Quaternion3D", "inverse", "quaternion3d"),
        GeometryFunction::QuaternionSlerp => ("Quaternion3D", "slerp", "quaternion3d"),
        GeometryFunction::QuaternionRotate => ("Quaternion3D", "rotate", "vector3d"),
        GeometryFunction::QuaternionTrack => ("Quaternion3D", "track", "rotationtrack3d"),
        GeometryFunction::QuaternionSample => ("Quaternion3D", "sample", "quaternion3d"),
        GeometryFunction::Hit3D => ("Scene3D", "hit", "hit3d"),
        GeometryFunction::HitCamera3D => ("Scene3D", "hit-camera", "hit3d"),
        GeometryFunction::RayHit3D => ("Scene3D", "ray-hit", "hit3d"),
        GeometryFunction::HitAll3D => ("Scene3D", "hit-all", "[hit3d]"),
        GeometryFunction::HitAllCamera3D => ("Scene3D", "hit-all-camera", "[hit3d]"),
        GeometryFunction::RayHitAll3D => ("Scene3D", "ray-hit-all", "[hit3d]"),
        GeometryFunction::HitRow3D => ("Scene3D", "hit-row", "hit3d"),
        GeometryFunction::HitRowCamera3D => ("Scene3D", "hit-row-camera", "hit3d"),
        GeometryFunction::RayHitRow3D => ("Scene3D", "ray-hit-row", "hit3d"),
        GeometryFunction::HitInstance3D => ("Scene3D", "hit-instance", "hit3d"),
        GeometryFunction::HitInstanceCamera3D => ("Scene3D", "hit-instance-camera", "hit3d"),
        GeometryFunction::RayHitInstance3D => ("Scene3D", "ray-hit-instance", "hit3d"),
        GeometryFunction::HitSubset3D => ("Scene3D", "hit-subset", "[hit3d]"),
        GeometryFunction::HitSubsetCamera3D => ("Scene3D", "hit-subset-camera", "[hit3d]"),
        GeometryFunction::RayHitSubset3D => ("Scene3D", "ray-hit-subset", "[hit3d]"),
        GeometryFunction::ClosestPoint3D => ("Scene3D", "closest-point", "hit3d"),
        GeometryFunction::ClosestPointOn3D => ("Scene3D", "closest-point-on", "hit3d"),
        GeometryFunction::SphereCast3D => ("Scene3D", "sphere-cast", "hit3d"),
        GeometryFunction::CapsuleCast3D => ("Scene3D", "capsule-cast", "hit3d"),
        GeometryFunction::SphereOverlap3D => ("Scene3D", "sphere-overlap", "[hit3d]"),
        GeometryFunction::CapsuleOverlap3D => ("Scene3D", "capsule-overlap", "[hit3d]"),
        GeometryFunction::CapsuleTriggerOverlap3D => {
            ("Scene3D", "capsule-trigger-overlap", "[hit3d]")
        }
        GeometryFunction::CapsuleTriggerCrossings3D => {
            ("Scene3D", "capsule-trigger-crossings", "[hit3d]")
        }
        GeometryFunction::CapsuleWalkResult3D => ("Scene3D", "capsule-walk-result", "walkresult3d"),
        GeometryFunction::CapsuleWalkWithContacts3D => {
            ("Scene3D", "capsule-walk-with-contacts", "walkresult3d")
        }
        GeometryFunction::SphereSlide3D => ("Scene3D", "sphere-slide", "point3d"),
        GeometryFunction::CapsuleSlide3D => ("Scene3D", "capsule-slide", "point3d"),
        GeometryFunction::SphereWalk3D => ("Scene3D", "sphere-walk", "point3d"),
        GeometryFunction::CapsuleWalk3D => ("Scene3D", "capsule-walk", "point3d"),
        GeometryFunction::Vector => ("Vector2D", "new", "vector2d"),
        GeometryFunction::MapPoint => ("Transform2D", kind.name(), "point2d"),
        GeometryFunction::MapVector => ("Transform2D", kind.name(), "vector2d"),
        GeometryFunction::Pick3D => ("Scene3D", "pick", "string"),
        GeometryFunction::PickCamera3D => ("Scene3D", "pick-camera", "string"),
        GeometryFunction::PickInRect3D => ("Scene3D", "pick-in-rect", "[int]"),
        GeometryFunction::PickInRectCamera3D => ("Scene3D", "pick-in-rect-camera", "[int]"),
        GeometryFunction::Project3D => ("Scene3D", "project", "projectedpoint3d"),
        GeometryFunction::ProjectCamera3D => ("Scene3D", "project-camera", "projectedpoint3d"),
        GeometryFunction::Unproject3D => ("Scene3D", "unproject", "point3d"),
        GeometryFunction::UnprojectCamera3D => ("Scene3D", "unproject-camera", "point3d"),
        GeometryFunction::MapPosition3D => ("Scene3D", "map-position", "point3d"),
        GeometryFunction::MapDirection3D => ("Scene3D", "map-direction", "vector3d"),
        GeometryFunction::SceneInstance3D => ("Scene3D", "instance", "instance3d"),
        GeometryFunction::SceneInstanceCamera3D => ("Scene3D", "instance-camera", "instance3d"),
        GeometryFunction::SceneInstanceOrbit3D => ("Scene3D", "instance-orbit", "instance3d"),
        _ => ("Transform2D", kind.name(), "transform2d"),
    };
    Function {
        namespace,
        member,
        kind: FunctionKind::Geometry(kind),
        parameters,
        result: Some(result),
        documentation,
    }
}

const PHYSICS_WORLD: Parameter =
    Parameter::with_type("world: physicsworld3d", &Type::PhysicsWorld3D);
const PHYSICS_DIRECTION: Parameter = Parameter::with_type("direction: vector3d", &Type::Vector3D);
const PHYSICS_REACH: Parameter = Parameter::with_type("max-distance: float", &Type::Float);
const PHYSICS_QUERY_MASK: Parameter = Parameter::optional("group-mask?: int", &Type::Int);
const PHYSICS_QUERY_IGNORE: Parameter = Parameter::optional("ignore-body?: int", &Type::Int);
const PHYSICS_QUERY_SENSORS: Parameter = Parameter::optional("include-sensors?: bool", &Type::Bool);
const PHYSICS_RAY: &[Parameter] = &[
    PHYSICS_WORLD,
    Parameter::with_type("origin: point3d", &Type::Point3D),
    PHYSICS_DIRECTION,
    PHYSICS_REACH,
    PHYSICS_QUERY_MASK,
    PHYSICS_QUERY_IGNORE,
    PHYSICS_QUERY_SENSORS,
];
const PHYSICS_SHAPE_SWEEP: &[Parameter] = &[
    PHYSICS_WORLD,
    PHYSICS_CENTER,
    PHYSICS_ROTATION,
    PHYSICS_SHAPES,
    PHYSICS_DIRECTION,
    PHYSICS_REACH,
    PHYSICS_QUERY_MASK,
    PHYSICS_QUERY_IGNORE,
    PHYSICS_QUERY_SENSORS,
];
const PHYSICS_SHAPE_OVERLAP: &[Parameter] = &[
    PHYSICS_WORLD,
    PHYSICS_CENTER,
    PHYSICS_ROTATION,
    PHYSICS_SHAPES,
    PHYSICS_QUERY_MASK,
    PHYSICS_QUERY_IGNORE,
    PHYSICS_QUERY_SENSORS,
];
const PHYSICS_MAX_STEPS: Parameter = Parameter::with_type("max-steps: int", &Type::Int);
const PHYSICS_BODY: Parameter = Parameter::with_type("body: int", &Type::Int);
const PHYSICS_TRIGGER: Parameter = Parameter::with_type("trigger: int", &Type::Int);
const PHYSICS_JOINT: Parameter = Parameter::with_type("joint: int", &Type::Int);
const PHYSICS_FIRST: Parameter = Parameter::with_type("first: int", &Type::Int);
const PHYSICS_SECOND: Parameter = Parameter::with_type("second: int", &Type::Int);
const JOINT_ANCHOR: Parameter = Parameter::with_type("anchor: point3d", &Type::Point3D);
const JOINT_MIN: Parameter = Parameter::with_type("min: float", &Type::Float);
const JOINT_MAX: Parameter = Parameter::with_type("max: float", &Type::Float);
const JOINT_ANCHORED: &[Parameter] = &[PHYSICS_WORLD, PHYSICS_FIRST, PHYSICS_SECOND, JOINT_ANCHOR];
const JOINT_AXIAL: &[Parameter] = &[
    PHYSICS_WORLD,
    PHYSICS_FIRST,
    PHYSICS_SECOND,
    JOINT_ANCHOR,
    Parameter::with_type("axis: vector3d", &Type::Vector3D),
    JOINT_MIN,
    JOINT_MAX,
];
const PHYSICS_CENTER: Parameter = Parameter::with_type("center: point3d", &Type::Point3D);
const PHYSICS_MASS: Parameter = Parameter::with_type("mass: float", &Type::Float);
const PHYSICS_DENSITY: Parameter = Parameter::with_type("density: float", &Type::Float);
const PHYSICS_LOCAL_CENTER: Parameter =
    Parameter::with_type("local-center: point3d", &Type::Point3D);
const PHYSICS_SIZE: Parameter = Parameter::with_type("size: vector3d", &Type::Vector3D);
const PHYSICS_ROTATION: Parameter = Parameter::with_type("rotation: vector3d", &Type::Vector3D);
const PHYSICS_SHAPES: Parameter = Parameter::new("shapes: [record]");
const PHYSICS_FORCE: Parameter = Parameter::with_type("force: vector3d", &Type::Vector3D);
const PHYSICS_TORQUE: Parameter = Parameter::with_type("torque: vector3d", &Type::Vector3D);
const PHYSICS_IMPULSE: Parameter = Parameter::with_type("impulse: vector3d", &Type::Vector3D);
const PHYSICS_POINT: Parameter = Parameter::with_type("point: point3d", &Type::Point3D);
const PHYSICS_VELOCITY: Parameter = Parameter::with_type("velocity: vector3d", &Type::Vector3D);
const PHYSICS_FRICTION: Parameter = Parameter::with_type("friction: float", &Type::Float);
const PHYSICS_RESTITUTION: Parameter = Parameter::with_type("restitution: float", &Type::Float);

const fn physics(
    kind: PhysicsFunction,
    parameters: &'static [Parameter],
    documentation: &'static str,
) -> Function {
    let result = match kind {
        PhysicsFunction::New => "physicsworld3d",
        PhysicsFunction::AddBox
        | PhysicsFunction::AddSphere
        | PhysicsFunction::AddKinematicBox
        | PhysicsFunction::AddBoxTrigger
        | PhysicsFunction::AddBody
        | PhysicsFunction::AddKinematicBody
        | PhysicsFunction::AddTrigger
        | PhysicsFunction::BodyById
        | PhysicsFunction::JointById
        | PhysicsFunction::AddFixedJoint
        | PhysicsFunction::AddSphericalJoint
        | PhysicsFunction::AddRevoluteJoint
        | PhysicsFunction::AddPrismaticJoint
        | PhysicsFunction::AddDistanceJoint
        | PhysicsFunction::AddCharacter
        | PhysicsFunction::CharacterCollisions
        | PhysicsFunction::CharacterSupport
        | PhysicsFunction::TriggerCount
        | PhysicsFunction::Advance
        | PhysicsFunction::AdvanceRealtime
        | PhysicsFunction::CcdSubsteps => "int",
        PhysicsFunction::JointPosition
        | PhysicsFunction::StaticFriction
        | PhysicsFunction::DynamicFriction
        | PhysicsFunction::Restitution
        | PhysicsFunction::Mass
        | PhysicsFunction::DefaultDensity
        | PhysicsFunction::TimeScale
        | PhysicsFunction::SimulationTime
        | PhysicsFunction::DroppedTime
        | PhysicsFunction::InterpolationAlpha
        | PhysicsFunction::SoftCcd
        | PhysicsFunction::LengthUnit => "float",
        PhysicsFunction::Contacts | PhysicsFunction::CharacterHits => "[physicscontact3d]",
        PhysicsFunction::RayCast | PhysicsFunction::ShapeCast => "physicshit3d",
        PhysicsFunction::RayCastAll | PhysicsFunction::ShapeCastAll => "[physicshit3d]",
        PhysicsFunction::TriggerEntered
        | PhysicsFunction::TriggerExited
        | PhysicsFunction::ShapeOverlap
        | PhysicsFunction::ContactBodies
        | PhysicsFunction::ContactEntered
        | PhysicsFunction::ContactExited => "[int]",
        PhysicsFunction::Position
        | PhysicsFunction::CenterOfMass
        | PhysicsFunction::LocalCenterOfMass
        | PhysicsFunction::InterpolatedPosition => "point3d",
        PhysicsFunction::Rotation
        | PhysicsFunction::Velocity
        | PhysicsFunction::AngularVelocity
        | PhysicsFunction::PrincipalInertia
        | PhysicsFunction::InertiaColumn
        | PhysicsFunction::InterpolatedRotation
        | PhysicsFunction::Gravity
        | PhysicsFunction::CharacterGravity => "vector3d",
        _ => "bool",
    };
    Function {
        namespace: "Physics3D",
        member: kind.name(),
        kind: FunctionKind::Physics(kind),
        parameters,
        result: Some(result),
        documentation,
    }
}

const WORLD_POINT: Parameter = Parameter::with_type("world: point3d", &Type::Point3D);
const POINTER: Parameter = Parameter::with_type("pointer: point2d", &Type::Point2D);
const VIEWPORT: Parameter = Parameter::with_type("viewport: point2d", &Type::Point2D);
const VIEW_DEPTH: Parameter = Parameter::with_type("depth: float", &Type::Float);
const WORLD_FALLBACK: Parameter = Parameter::with_type("fallback: point3d", &Type::Point3D);
const LAYER_MASK: Parameter = Parameter::optional("layer-mask?: int", &Type::Int);
const ORBIT_VIEW: [Parameter; 4] = [
    Parameter::with_type("yaw: float", &Type::Float),
    Parameter::with_type("pitch: float", &Type::Float),
    Parameter::with_type("distance: float", &Type::Float),
    VIEWPORT,
];
const NAMED_VIEW: [Parameter; 3] = [
    Parameter::new("objects: [record] | scenemodel3d"),
    Parameter::with_type("camera: string", &Type::String),
    VIEWPORT,
];

const SCENE_ENDPOINTS: [Parameter; 3] = [
    NAMED_VIEW[0],
    Parameter::with_type("from: string", &Type::String),
    Parameter::with_type("to: string", &Type::String),
];
const XYZ: &[Parameter] = &[
    Parameter::with_type("x: float", &Type::Float),
    Parameter::with_type("y: float", &Type::Float),
    Parameter::with_type("z: float", &Type::Float),
];

const WORLD_RAY: &[Parameter] = &[
    NAMED_VIEW[0],
    Parameter::with_type("origin: point3d", &Type::Point3D),
    Parameter::with_type("direction: vector3d", &Type::Vector3D),
    Parameter::with_type("max-distance: float", &Type::Float),
    LAYER_MASK,
];
const SPHERE_RADIUS: Parameter = Parameter::with_type("radius: float", &Type::Float);
const WORLD_CLOSEST_POINT: &[Parameter] = &[WORLD_RAY[0], WORLD_RAY[1], SPHERE_RADIUS, LAYER_MASK];
const WORLD_CLOSEST_POINT_ON: &[Parameter] = &[
    WORLD_RAY[0],
    Parameter::with_type("object-id: string", &Type::String),
    WORLD_RAY[1],
    SPHERE_RADIUS,
    LAYER_MASK,
];
const WORLD_SPHERE_CAST: &[Parameter] = &[
    WORLD_RAY[0],
    WORLD_RAY[1],
    WORLD_RAY[2],
    WORLD_RAY[3],
    SPHERE_RADIUS,
    LAYER_MASK,
];
const WORLD_SPHERE_OVERLAP: &[Parameter] = &[
    WORLD_RAY[0],
    Parameter::with_type("center: point3d", &Type::Point3D),
    SPHERE_RADIUS,
    LAYER_MASK,
];
const WORLD_SPHERE_SLIDE: &[Parameter] = &[
    WORLD_RAY[0],
    WORLD_RAY[1],
    Parameter::with_type("displacement: vector3d", &Type::Vector3D),
    SPHERE_RADIUS,
    LAYER_MASK,
];
const WORLD_SPHERE_WALK: &[Parameter] = &[
    WORLD_SPHERE_SLIDE[0],
    WORLD_SPHERE_SLIDE[1],
    WORLD_SPHERE_SLIDE[2],
    SPHERE_RADIUS,
    Parameter::with_type("step-height: float", &Type::Float),
    Parameter::with_type("snap-distance: float", &Type::Float),
    Parameter::with_type("max-slope: float", &Type::Float),
    LAYER_MASK,
];
const WORLD_CAPSULE_CAST: &[Parameter] = &[
    WORLD_RAY[0],
    WORLD_RAY[1],
    WORLD_RAY[2],
    WORLD_RAY[3],
    SPHERE_RADIUS,
    Parameter::with_type("segment-height: float", &Type::Float),
    LAYER_MASK,
];
const WORLD_CAPSULE_OVERLAP: &[Parameter] = &[
    WORLD_RAY[0],
    Parameter::with_type("center: point3d", &Type::Point3D),
    SPHERE_RADIUS,
    Parameter::with_type("segment-height: float", &Type::Float),
    LAYER_MASK,
];
const WORLD_TRIGGER_PATH: &[Parameter] = &[
    WORLD_RAY[0],
    Parameter::with_type("from: point3d", &Type::Point3D),
    Parameter::with_type("to: point3d", &Type::Point3D),
    SPHERE_RADIUS,
    Parameter::with_type("segment-height: float", &Type::Float),
    LAYER_MASK,
];
const WORLD_CAPSULE_SLIDE: &[Parameter] = &[
    WORLD_RAY[0],
    WORLD_RAY[1],
    Parameter::with_type("displacement: vector3d", &Type::Vector3D),
    SPHERE_RADIUS,
    Parameter::with_type("segment-height: float", &Type::Float),
    LAYER_MASK,
];
const WORLD_CAPSULE_WALK: &[Parameter] = &[
    WORLD_CAPSULE_SLIDE[0],
    WORLD_CAPSULE_SLIDE[1],
    WORLD_CAPSULE_SLIDE[2],
    SPHERE_RADIUS,
    Parameter::with_type("segment-height: float", &Type::Float),
    Parameter::with_type("step-height: float", &Type::Float),
    Parameter::with_type("snap-distance: float", &Type::Float),
    Parameter::with_type("max-slope: float", &Type::Float),
    LAYER_MASK,
];
const ORBIT_HIT: &[Parameter] = &[
    NAMED_VIEW[0],
    ORBIT_VIEW[0],
    ORBIT_VIEW[1],
    ORBIT_VIEW[2],
    VIEWPORT,
    POINTER,
    LAYER_MASK,
];
const CAMERA_HIT: &[Parameter] = &[NAMED_VIEW[0], NAMED_VIEW[1], VIEWPORT, POINTER, LAYER_MASK];
const HIT_ROW: Parameter = Parameter::with_type("row: int", &Type::Int);
const HIT_ROWS: Parameter = Parameter::with_type("rows: [int]", &Type::IntList);
const HIT_INSTANCE: &[Parameter] = &[
    Parameter::with_type("row-index: int", &Type::Int),
    Parameter::with_type("mesh-node-index: int", &Type::Int),
    Parameter::with_type("instance-index: int", &Type::Int),
];
const INSTANCE_QUERY: [Parameter; 4] = [
    NAMED_VIEW[0],
    HIT_INSTANCE[0],
    HIT_INSTANCE[1],
    HIT_INSTANCE[2],
];
const INSTANCE_CAMERA_QUERY: [Parameter; 6] = [
    NAMED_VIEW[0],
    NAMED_VIEW[1],
    VIEWPORT,
    HIT_INSTANCE[0],
    HIT_INSTANCE[1],
    HIT_INSTANCE[2],
];
const INSTANCE_ORBIT_QUERY: [Parameter; 8] = [
    NAMED_VIEW[0],
    ORBIT_VIEW[0],
    ORBIT_VIEW[1],
    ORBIT_VIEW[2],
    VIEWPORT,
    HIT_INSTANCE[0],
    HIT_INSTANCE[1],
    HIT_INSTANCE[2],
];
const fn restrict_pick<const N: usize>(
    parameters: &[Parameter],
    selectors: &[Parameter],
) -> [Parameter; N] {
    let mut result = [LAYER_MASK; N];
    let mut index = 0;
    while index + 1 < parameters.len() {
        result[index] = parameters[index];
        index += 1;
    }
    let mut selector = 0;
    while selector < selectors.len() {
        result[index] = selectors[selector];
        index += 1;
        selector += 1;
    }
    result
}
const RECT_START: Parameter = Parameter::with_type("start: point2d", &Type::Point2D);
const RECT_END: Parameter = Parameter::with_type("end: point2d", &Type::Point2D);
const ORBIT_RECT: &[Parameter] = &[
    NAMED_VIEW[0],
    ORBIT_VIEW[0],
    ORBIT_VIEW[1],
    ORBIT_VIEW[2],
    VIEWPORT,
    RECT_START,
    RECT_END,
    LAYER_MASK,
];
const CAMERA_RECT: &[Parameter] = &[
    NAMED_VIEW[0],
    NAMED_VIEW[1],
    VIEWPORT,
    RECT_START,
    RECT_END,
    LAYER_MASK,
];

pub const FUNCTIONS: &[Function] = &[
    physics(
        PhysicsFunction::New,
        &[
            Parameter::with_type("gravity: vector3d", &Type::Vector3D),
            Parameter::with_type("timestep: float", &Type::Float),
        ],
        "Create an independent physics world with a fixed timestep in seconds.",
    ),
    physics(
        PhysicsFunction::AddBox,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_SIZE,
            PHYSICS_MASS,
            PHYSICS_FRICTION,
            PHYSICS_RESTITUTION,
        ],
        "Insert a box with full extents. A zero mass creates a fixed body, and 0 is an invalid handle.",
    ),
    physics(
        PhysicsFunction::AddSphere,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            Parameter::with_type("radius: float", &Type::Float),
            PHYSICS_MASS,
            PHYSICS_FRICTION,
            PHYSICS_RESTITUTION,
        ],
        "Insert a sphere. A zero mass creates a fixed body, and 0 is an invalid handle.",
    ),
    physics(
        PhysicsFunction::AddKinematicBox,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_SIZE,
            PHYSICS_ROTATION,
            PHYSICS_FRICTION,
            PHYSICS_RESTITUTION,
        ],
        "Insert a script-driven box. Rotation contains yaw, pitch and roll in degrees.",
    ),
    physics(
        PhysicsFunction::SetKinematic,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Take over a dynamic body or release a kinematic body with its committed linear and angular velocities.",
    ),
    physics(
        PhysicsFunction::IsKinematic,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Check whether the body's pose is driven kinematically.",
    ),
    physics(
        PhysicsFunction::SetKinematicTargetWithPivot,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("position: point3d", &Type::Point3D),
            PHYSICS_ROTATION,
            Parameter::with_type("pivot: vector3d", &Type::Vector3D),
        ],
        "Queue a kinematic pose that rotates about a body-local pivot.",
    ),
    physics(
        PhysicsFunction::SetKinematicTarget,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            PHYSICS_CENTER,
            PHYSICS_ROTATION,
        ],
        "Queue the world pose for the next step. The last target wins if set more than once.",
    ),
    physics(
        PhysicsFunction::AddBoxTrigger,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_SIZE,
            PHYSICS_ROTATION,
        ],
        "Insert a fixed box sensor with full extents. It reports overlap without applying contact forces.",
    ),
    physics(
        PhysicsFunction::AddBody,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_ROTATION,
            PHYSICS_SHAPES,
            PHYSICS_MASS,
            PHYSICS_FRICTION,
            PHYSICS_RESTITUTION,
        ],
        "Create a rigid body from local shape records. A mass of zero makes it fixed. A plane must be the only shape and use zero mass.",
    ),
    physics(
        PhysicsFunction::AddKinematicBody,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_ROTATION,
            PHYSICS_SHAPES,
            PHYSICS_FRICTION,
            PHYSICS_RESTITUTION,
        ],
        "Create a kinematic body from local convex shapes. Move it with set-kinematic-target.",
    ),
    physics(
        PhysicsFunction::AddTrigger,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            PHYSICS_ROTATION,
            PHYSICS_SHAPES,
        ],
        "Create a nonblocking trigger from local convex shapes. Reports are shared by all of its parts.",
    ),
    physics(
        PhysicsFunction::SyncBodies,
        &[PHYSICS_WORLD, Parameter::new("bodies: [record]")],
        "Synchronize fixed, dynamic, kinematic and trigger bodies by ID. Unchanged rows keep their simulation state.",
    ),
    physics(
        PhysicsFunction::BodyById,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("id: string", &Type::String),
        ],
        "Return the handle of a declared body by ID, or zero if it is absent.",
    ),
    physics(
        PhysicsFunction::SyncScene,
        &[
            PHYSICS_WORLD,
            Parameter::new("bodies: [record]"),
            Parameter::new("joints: [record]"),
        ],
        "Atomically synchronize named bodies and joints. Joint anchors and rotations are local to each endpoint. Unchanged constraints keep their handles and motors.",
    ),
    physics(
        PhysicsFunction::JointById,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("id: string", &Type::String),
        ],
        "Return the handle of a declared joint by ID, or zero if it is absent.",
    ),
    physics(
        PhysicsFunction::SetCollisionFilter,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("group: int", &Type::Int),
            Parameter::with_type("ignore-groups: int", &Type::Int),
        ],
        "Set the body's group index (0–31) and the bitmask of groups it ignores on subsequent steps.",
    ),
    physics(
        PhysicsFunction::SetKinematicContactReports,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("static-kinematic: bool", &Type::Bool),
            Parameter::with_type("kinematic-kinematic: bool", &Type::Bool),
        ],
        "Select geometric contact reporting for static–kinematic and kinematic–kinematic pairs.",
    ),
    physics(
        PhysicsFunction::ReportStaticKinematicCollisions,
        &[PHYSICS_WORLD],
        "Read whether the world reports static–kinematic contacts.",
    ),
    physics(
        PhysicsFunction::ReportKinematicKinematicCollisions,
        &[PHYSICS_WORLD],
        "Read whether the world reports kinematic–kinematic contacts.",
    ),
    physics(
        PhysicsFunction::SetContactReports,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("send: bool", &Type::Bool),
            Parameter::with_type("receive: bool", &Type::Bool),
        ],
        "Enable sending or receiving solved contact points for this solid body on subsequent steps.",
    ),
    physics(
        PhysicsFunction::Contacts,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the contact points and normal impulses received during the latest completed step.",
    ),
    physics(
        PhysicsFunction::SetContactEvents,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Track solid-body contact transitions at fixed step boundaries, independently of impulse reporting.",
    ),
    physics(
        PhysicsFunction::ContactBodies,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the solid bodies touching this body after the last completed substep.",
    ),
    physics(
        PhysicsFunction::ContactEntered,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the solid bodies that began touching this body during the latest completed step batch.",
    ),
    physics(
        PhysicsFunction::ContactExited,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the solid bodies that separated from this body during the latest completed step batch.",
    ),
    physics(
        PhysicsFunction::AddFixedJoint,
        JOINT_ANCHORED,
        "Join two bodies at a world-space anchor and keep their initial relative pose.",
    ),
    physics(
        PhysicsFunction::AddSphericalJoint,
        JOINT_ANCHORED,
        "Keep the bodies joined at the world-space anchor and allow free rotation.",
    ),
    physics(
        PhysicsFunction::AddRevoluteJoint,
        JOINT_AXIAL,
        "Create a hinge around the world-space axis. Limits are relative degrees from −180 to 180.",
    ),
    physics(
        PhysicsFunction::AddPrismaticJoint,
        JOINT_AXIAL,
        "Allow translation along the world-space axis within the relative min and max travel.",
    ),
    physics(
        PhysicsFunction::AddDistanceJoint,
        &[
            PHYSICS_WORLD,
            PHYSICS_FIRST,
            PHYSICS_SECOND,
            Parameter::with_type("first-anchor: point3d", &Type::Point3D),
            Parameter::with_type("second-anchor: point3d", &Type::Point3D),
            JOINT_MIN,
            JOINT_MAX,
        ],
        "Keep the distance between two world-space anchors in the specified nonnegative range.",
    ),
    physics(
        PhysicsFunction::SetJointLimits,
        &[PHYSICS_WORLD, PHYSICS_JOINT, JOINT_MIN, JOINT_MAX],
        "Update the bounds of a revolute, prismatic, or distance joint on the next step.",
    ),
    physics(
        PhysicsFunction::SetJointMotor,
        &[
            PHYSICS_WORLD,
            PHYSICS_JOINT,
            Parameter::with_type("velocity: float", &Type::Float),
            Parameter::with_type("max-force: float", &Type::Float),
        ],
        "Drive a hinge in degrees per second or a slider in world units per second. Zero maximum force disables the drive.",
    ),
    physics(
        PhysicsFunction::JointPosition,
        &[PHYSICS_WORLD, PHYSICS_JOINT],
        "Read the relative hinge angle in degrees, slider travel, or distance between anchors.",
    ),
    physics(
        PhysicsFunction::RemoveJoint,
        &[PHYSICS_WORLD, PHYSICS_JOINT],
        "Remove a joint while keeping the connected bodies.",
    ),
    physics(
        PhysicsFunction::TriggerCount,
        &[PHYSICS_WORLD, PHYSICS_TRIGGER],
        "Count the solid bodies currently overlapping a trigger.",
    ),
    physics(
        PhysicsFunction::TriggerOverlaps,
        &[PHYSICS_WORLD, PHYSICS_TRIGGER, PHYSICS_BODY],
        "Test whether a particular solid body currently overlaps the trigger.",
    ),
    physics(
        PhysicsFunction::TriggerEntered,
        &[PHYSICS_WORLD, PHYSICS_TRIGGER],
        "Return the handles of bodies that entered a trigger during the latest completed step.",
    ),
    physics(
        PhysicsFunction::TriggerExited,
        &[PHYSICS_WORLD, PHYSICS_TRIGGER],
        "Return the handles of bodies that exited a trigger during the latest completed step.",
    ),
    physics(
        PhysicsFunction::Advance,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("elapsed: float", &Type::Float),
            PHYSICS_MAX_STEPS,
        ],
        "Consume elapsed seconds in fixed steps and return the number of steps executed.",
    ),
    physics(
        PhysicsFunction::AdvanceRealtime,
        &[PHYSICS_WORLD, PHYSICS_MAX_STEPS],
        "Advance the world by monotonic time elapsed since the previous call.",
    ),
    physics(
        PhysicsFunction::SetRunning,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("running: bool", &Type::Bool),
        ],
        "Pause or resume elapsed-time advancement of the world.",
    ),
    physics(
        PhysicsFunction::SetTimeScale,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("scale: float", &Type::Float),
        ],
        "Multiply elapsed time by the given nonnegative scale before scheduling fixed steps.",
    ),
    physics(
        PhysicsFunction::IsRunning,
        &[PHYSICS_WORLD],
        "Read whether elapsed time is advancing the world.",
    ),
    physics(
        PhysicsFunction::TimeScale,
        &[PHYSICS_WORLD],
        "Read the clock's elapsed-time multiplier.",
    ),
    physics(
        PhysicsFunction::SimulationTime,
        &[PHYSICS_WORLD],
        "Read the committed simulation time in seconds.",
    ),
    physics(
        PhysicsFunction::DroppedTime,
        &[PHYSICS_WORLD],
        "Read the accumulated simulation time discarded by the fixed-step cap.",
    ),
    physics(
        PhysicsFunction::InterpolationAlpha,
        &[PHYSICS_WORLD],
        "Read the fraction used to interpolate between the two most recent fixed-step poses.",
    ),
    physics(
        PhysicsFunction::SetPoseInterpolation,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Enable or disable retaining the previous step's pose for this body.",
    ),
    physics(
        PhysicsFunction::InterpolatedPosition,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the render position of a body using its enabled pose interpolation.",
    ),
    physics(
        PhysicsFunction::InterpolatedRotation,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the interpolated yaw, pitch and roll in degrees.",
    ),
    physics(
        PhysicsFunction::Step,
        &[PHYSICS_WORLD],
        "Advance the world by its fixed timestep.",
    ),
    physics(
        PhysicsFunction::Position,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read a body's world position and observe later simulation steps.",
    ),
    physics(
        PhysicsFunction::Rotation,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read yaw, pitch and roll in degrees in SceneView3D field order.",
    ),
    physics(
        PhysicsFunction::Velocity,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the body's linear velocity in world units per second.",
    ),
    physics(
        PhysicsFunction::ApplyImpulse,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_IMPULSE],
        "Apply an impulse at the center of mass of a dynamic body.",
    ),
    physics(
        PhysicsFunction::ApplyForce,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_FORCE],
        "Apply a force at a dynamic body's center of mass for the next step.",
    ),
    physics(
        PhysicsFunction::ApplyForceAt,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_FORCE, PHYSICS_POINT],
        "Apply a force at a world-space point for the next step, including torque about the center of mass.",
    ),
    physics(
        PhysicsFunction::ApplyTorque,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_TORQUE],
        "Apply a world-space torque for the next step.",
    ),
    physics(
        PhysicsFunction::SetContinuousLoad,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_FORCE, PHYSICS_TORQUE],
        "Replace the persistent world-space force and torque used in subsequent steps.",
    ),
    physics(
        PhysicsFunction::ClearForces,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Clear both queued and continuous forces and torques.",
    ),
    physics(
        PhysicsFunction::ApplyImpulseAt,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_IMPULSE, PHYSICS_POINT],
        "Apply an immediate impulse at a world-space point.",
    ),
    physics(
        PhysicsFunction::ApplyTorqueImpulse,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_IMPULSE],
        "Apply an immediate angular impulse about the world XYZ axes.",
    ),
    physics(
        PhysicsFunction::SetLinearVelocity,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_VELOCITY],
        "Replace the linear velocity in world units per second.",
    ),
    physics(
        PhysicsFunction::SetAngularVelocity,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_VELOCITY],
        "Replace the angular velocity about the world XYZ axes in radians per second.",
    ),
    physics(
        PhysicsFunction::AngularVelocity,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the world XYZ angular velocity in radians per second.",
    ),
    physics(
        PhysicsFunction::ResetBody,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            PHYSICS_CENTER,
            PHYSICS_ROTATION,
        ],
        "Move a dynamic body directly to a pose and clear its velocities, forces and torques.",
    ),
    physics(
        PhysicsFunction::SetGravityScale,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("scale: float", &Type::Float),
        ],
        "Set a body's gravity multiplier. Zero disables gravity and negative values reverse it.",
    ),
    physics(
        PhysicsFunction::SetMass,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_MASS],
        "Assign a total mass and derive inertia from the shapes assuming uniform density.",
    ),
    physics(
        PhysicsFunction::SetDensity,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_DENSITY],
        "Derive mass and inertia at the given density, in mass per cubic world unit.",
    ),
    physics(
        PhysicsFunction::UseDefaultDensity,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Use the world density and follow future changes to that density.",
    ),
    physics(
        PhysicsFunction::SetDefaultDensity,
        &[PHYSICS_WORLD, PHYSICS_DENSITY],
        "Change the world density and recompute it for bodies that follow the default.",
    ),
    physics(
        PhysicsFunction::SetMassProperties,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            PHYSICS_MASS,
            PHYSICS_LOCAL_CENTER,
            Parameter::with_type("principal-inertia: vector3d", &Type::Vector3D),
            Parameter::with_type("principal-rotation: vector3d", &Type::Vector3D),
        ],
        "Set local center of mass, principal moments, and principal-axis yaw, pitch and roll in degrees.",
    ),
    physics(
        PhysicsFunction::SetInertiaMatrix,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            PHYSICS_MASS,
            PHYSICS_LOCAL_CENTER,
            Parameter::with_type("column-x: vector3d", &Type::Vector3D),
            Parameter::with_type("column-y: vector3d", &Type::Vector3D),
            Parameter::with_type("column-z: vector3d", &Type::Vector3D),
        ],
        "Set the positive definite body-local inertia matrix about the center of mass.",
    ),
    physics(
        PhysicsFunction::Mass,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the current total mass of a dynamic body.",
    ),
    physics(
        PhysicsFunction::DefaultDensity,
        &[PHYSICS_WORLD],
        "Read the current default density of the world.",
    ),
    physics(
        PhysicsFunction::CenterOfMass,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the current center of mass in world coordinates.",
    ),
    physics(
        PhysicsFunction::LocalCenterOfMass,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the local center of mass of a dynamic body.",
    ),
    physics(
        PhysicsFunction::PrincipalInertia,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the moments of inertia about the body's principal axes.",
    ),
    physics(
        PhysicsFunction::InertiaColumn,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("column: int", &Type::Int),
        ],
        "Read a zero-based column of the body-local inertia matrix.",
    ),
    physics(
        PhysicsFunction::SetMaterial,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("static-friction: float", &Type::Float),
            Parameter::with_type("dynamic-friction: float", &Type::Float),
            PHYSICS_RESTITUTION,
        ],
        "Set the static friction, dynamic friction, and restitution of a solid body's colliders.",
    ),
    physics(
        PhysicsFunction::StaticFriction,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the friction coefficient when the contact is sticking.",
    ),
    physics(
        PhysicsFunction::DynamicFriction,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the friction coefficient when the contact slides.",
    ),
    physics(
        PhysicsFunction::Restitution,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the body's coefficient of restitution.",
    ),
    physics(
        PhysicsFunction::SetDamping,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("linear: float", &Type::Float),
            Parameter::with_type("angular: float", &Type::Float),
        ],
        "Set nonnegative linear and angular damping for this body.",
    ),
    physics(
        PhysicsFunction::SetCcdSubsteps,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("maximum: int", &Type::Int),
        ],
        "Set the world CCD substep budget from zero to 64. Zero disables CCD sweeps.",
    ),
    physics(
        PhysicsFunction::CcdSubsteps,
        &[PHYSICS_WORLD],
        "Read the world's CCD substep budget.",
    ),
    physics(
        PhysicsFunction::SetBodyCcd,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Select whether this solid body uses full CCD, including sweeps against dynamic targets.",
    ),
    physics(
        PhysicsFunction::BodyCcd,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the body's configured full CCD mode.",
    ),
    physics(
        PhysicsFunction::SetSoftCcd,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("distance: float", &Type::Float),
        ],
        "Set the speculative-contact prediction distance in world units.",
    ),
    physics(
        PhysicsFunction::SoftCcd,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the speculative-contact prediction distance.",
    ),
    physics(
        PhysicsFunction::SetLengthUnit,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("length: float", &Type::Float),
        ],
        "Set the scene length scale used by the solver's positional and velocity tolerances.",
    ),
    physics(
        PhysicsFunction::LengthUnit,
        &[PHYSICS_WORLD],
        "Read the solver's scene length scale.",
    ),
    physics(
        PhysicsFunction::SetGravity,
        &[
            PHYSICS_WORLD,
            Parameter::with_type("gravity: vector3d", &Type::Vector3D),
        ],
        "Set world gravity and update characters that follow it.",
    ),
    physics(
        PhysicsFunction::Gravity,
        &[PHYSICS_WORLD],
        "Read world gravity in world units per second squared.",
    ),
    physics(
        PhysicsFunction::UseCharacterWorldGravity,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Make a character follow world gravity and its future updates.",
    ),
    physics(
        PhysicsFunction::CharacterGravity,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the effective gravity of a character.",
    ),
    physics(
        PhysicsFunction::SetAxisLocks,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("linear-mask: int", &Type::Int),
            Parameter::with_type("angular-mask: int", &Type::Int),
        ],
        "Lock world translation and rotation axes. Each mask uses X=1, Y=2, Z=4.",
    ),
    physics(
        PhysicsFunction::IsSleeping,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Test whether a dynamic body is sleeping.",
    ),
    physics(
        PhysicsFunction::AddCharacter,
        &[
            PHYSICS_WORLD,
            PHYSICS_CENTER,
            Parameter::with_type("radius: float", &Type::Float),
            Parameter::with_type("segment-height: float", &Type::Float),
        ],
        "Create a capsule character with the world's gravity. Segment height excludes the two hemispheres.",
    ),
    physics(
        PhysicsFunction::SetCharacterMovement,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_VELOCITY],
        "Set the persistent desired velocity of the character in world units per second.",
    ),
    physics(
        PhysicsFunction::SetCharacterGravity,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("gravity: vector3d", &Type::Vector3D),
        ],
        "Replace the character's acceleration due to gravity. Zero also clears accumulated falling speed.",
    ),
    physics(
        PhysicsFunction::SetCharacterMidAirControl,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Choose whether to accept new movement input while airborne. This is enabled by default.",
    ),
    physics(
        PhysicsFunction::SetCharacterOptions,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("step-height: float", &Type::Float),
            Parameter::with_type("snap-distance: float", &Type::Float),
            Parameter::with_type("max-slope: float", &Type::Float),
            Parameter::with_type("push-mass: float", &Type::Float),
        ],
        "Set stair height, ground snap distance, maximum climb angle in degrees and effective pushing mass.",
    ),
    physics(
        PhysicsFunction::JumpCharacter,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("speed: float", &Type::Float),
        ],
        "Start a jump along world Y when the character is grounded.",
    ),
    physics(
        PhysicsFunction::TeleportCharacter,
        &[PHYSICS_WORLD, PHYSICS_BODY, PHYSICS_CENTER],
        "Move the character directly to a center and clear falling speed and contact state.",
    ),
    physics(
        PhysicsFunction::CharacterGrounded,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Test whether the character found walkable support.",
    ),
    physics(
        PhysicsFunction::CharacterHitWall,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Test whether the character's last movement hit a side obstacle.",
    ),
    physics(
        PhysicsFunction::CharacterHitCeiling,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Test whether the character's last movement hit a surface overhead.",
    ),
    physics(
        PhysicsFunction::CharacterCollisions,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the last move's collision flags. Side=1, above=2 and ground=4.",
    ),
    physics(
        PhysicsFunction::CharacterSupport,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the body handle supporting the character, or zero when unsupported.",
    ),
    physics(
        PhysicsFunction::SetCharacterHitReports,
        &[
            PHYSICS_WORLD,
            PHYSICS_BODY,
            Parameter::with_type("enabled: bool", &Type::Bool),
        ],
        "Enable shape-hit snapshots for a character. Reporting is disabled initially.",
    ),
    physics(
        PhysicsFunction::RayCast,
        PHYSICS_RAY,
        "Find the nearest collider surface in world space. Body zero means no hit.",
    ),
    physics(
        PhysicsFunction::RayCastAll,
        PHYSICS_RAY,
        "Find one collider surface per body in world space, sorted by distance and body handle.",
    ),
    physics(
        PhysicsFunction::ShapeCast,
        PHYSICS_SHAPE_SWEEP,
        "Sweep a convex or compound shape at constant rotation and return its nearest contact.",
    ),
    physics(
        PhysicsFunction::ShapeCastAll,
        PHYSICS_SHAPE_SWEEP,
        "Sweep a convex or compound shape and return one contact per body, ordered by travel distance.",
    ),
    physics(
        PhysicsFunction::ShapeOverlap,
        PHYSICS_SHAPE_OVERLAP,
        "Return the sorted handles of bodies that intersect a convex or compound shape.",
    ),
    physics(
        PhysicsFunction::CharacterHits,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Read the last move's world-space shape hits and net push impulses in sweep order.",
    ),
    physics(
        PhysicsFunction::Remove,
        &[PHYSICS_WORLD, PHYSICS_BODY],
        "Remove a body and retire its handle.",
    ),
    physics(
        PhysicsFunction::Clear,
        &[PHYSICS_WORLD],
        "Remove every body and keep the world's gravity and timestep.",
    ),
    geometry(
        GeometryFunction::Point3D,
        XYZ,
        "Creates a point in 3D scene coordinates.",
    ),
    geometry(
        GeometryFunction::Vector3D,
        XYZ,
        "Creates a displacement or direction in 3D scene coordinates.",
    ),
    geometry(
        GeometryFunction::QuaternionIdentity,
        &[],
        "Returns the identity 3D rotation.",
    ),
    geometry(
        GeometryFunction::QuaternionNew,
        &[
            Parameter::with_type("x: float", &Type::Float),
            Parameter::with_type("y: float", &Type::Float),
            Parameter::with_type("z: float", &Type::Float),
            Parameter::with_type("w: float", &Type::Float),
        ],
        "Normalizes the (x, y, z, w) components. A zero or non-finite value returns identity.",
    ),
    geometry(
        GeometryFunction::QuaternionAxisAngle,
        &[
            Parameter::with_type("axis: vector3d", &Type::Vector3D),
            Parameter::with_type("degrees: float", &Type::Float),
        ],
        "Creates a rotation about an axis by an angle in degrees.",
    ),
    geometry(
        GeometryFunction::QuaternionEuler,
        &[Parameter::with_type("angles: vector3d", &Type::Vector3D)],
        "Creates a unit rotation using the scene Euler convention in degrees.",
    ),
    geometry(
        GeometryFunction::QuaternionLookRotation,
        &[
            Parameter::with_type("forward: vector3d", &Type::Vector3D),
            Parameter::optional("up?: vector3d", &Type::Vector3D),
        ],
        "Aim local -Z along forward and local +Y toward the projected up vector. Up defaults to +Y. A zero forward direction returns identity.",
    ),
    geometry(
        GeometryFunction::QuaternionLookAt,
        &[
            Parameter::with_type("from: point3d", &Type::Point3D),
            Parameter::with_type("target: point3d", &Type::Point3D),
            Parameter::optional("up?: vector3d", &Type::Vector3D),
        ],
        "Aim local -Z from a point toward a target. The points and up vector use the same coordinate space. Coincident points return identity.",
    ),
    geometry(
        GeometryFunction::QuaternionCompose,
        &[
            Parameter::with_type("first: quaternion3d", &Type::Quaternion3D),
            Parameter::with_type("next: quaternion3d", &Type::Quaternion3D),
        ],
        "Applies the first rotation, then the next rotation.",
    ),
    geometry(
        GeometryFunction::QuaternionInverse,
        &[Parameter::with_type(
            "rotation: quaternion3d",
            &Type::Quaternion3D,
        )],
        "Returns the inverse of a unit rotation.",
    ),
    geometry(
        GeometryFunction::QuaternionSlerp,
        &[
            Parameter::with_type("from: quaternion3d", &Type::Quaternion3D),
            Parameter::with_type("to: quaternion3d", &Type::Quaternion3D),
            Parameter::with_type("progress: float", &Type::Float),
        ],
        "Interpolates along the shortest rotational arc. Progress is clamped to [0, 1].",
    ),
    geometry(
        GeometryFunction::QuaternionRotate,
        &[
            Parameter::with_type("rotation: quaternion3d", &Type::Quaternion3D),
            Parameter::with_type("vector: vector3d", &Type::Vector3D),
        ],
        "Rotates a 3D vector using the unit quaternion.",
    ),
    geometry(
        GeometryFunction::QuaternionTrack,
        &[Parameter::new("keys: [record]")],
        "Prepares a rotation track from time (float seconds) and orientation (quaternion3d) fields. Times are finite, nonnegative and strictly increasing.",
    ),
    geometry(
        GeometryFunction::QuaternionSample,
        &[
            Parameter::with_type("track: rotationtrack3d", &Type::RotationTrack3D),
            Parameter::with_type("time: float", &Type::Float),
        ],
        "Samples the shortest arc between keys in seconds. Values outside the interval use the nearest key. An invalid track or NaN time returns identity.",
    ),
    geometry(
        GeometryFunction::MeshNode3D,
        &[
            Parameter::new("asset: meshasset3d | string"),
            Parameter::new("selector: string | int"),
        ],
        "Looks up a glTF node by its unique name, absolute path, or original numeric index. The origin and axes use the asset's initial coordinate frame. Missing or ambiguous selectors return valid false.",
    ),
    geometry(
        GeometryFunction::MeshNodeIndices3D,
        &[
            Parameter::new("asset: meshasset3d | string"),
            Parameter::optional("kind?: string", &Type::String),
        ],
        "Lists original glTF node indices in traversal order. Filter by all, mesh, camera, or light. Pass an index to Scene3D.mesh-node.",
    ),
    geometry(
        GeometryFunction::MeshMaterialSlots3D,
        &[
            Parameter::new("asset: meshasset3d | string"),
            Parameter::optional("variant?: string", &Type::String),
        ],
        "Lists material slot selectors for the selected asset layout. An empty or missing variant selector uses the original layout.",
    ),
    geometry(
        GeometryFunction::MeshMaterialVariants3D,
        &[Parameter::new("asset: meshasset3d | string")],
        "Lists selectable material variants in source order, using a numeric selector for a duplicate or reserved name.",
    ),
    geometry(
        GeometryFunction::MeshAnimationClips3D,
        &[Parameter::new("asset: meshasset3d | string")],
        "Lists animation clip selectors in source order, using original numeric indices for missing or ambiguous names.",
    ),
    geometry(
        GeometryFunction::MeshAnimationDuration3D,
        &[
            Parameter::new("asset: meshasset3d | string"),
            Parameter::with_type("clip: string", &Type::String),
        ],
        "Returns the duration of the selected animation in seconds. Missing assets and clips return zero.",
    ),
    geometry(
        GeometryFunction::SceneNode3D,
        &[
            NAMED_VIEW[0],
            Parameter::with_type("object: string", &Type::String),
            Parameter::new("selector: string | int"),
        ],
        "Looks up a glTF node on a uniquely named, visible scene object. The origin and axes use the current world pose, including animation and resolved physics. Use a node name, absolute path, or original index.",
    ),
    geometry(
        GeometryFunction::Bounds3D,
        &[NAMED_VIEW[0], LAYER_MASK],
        "Returns world-space bounds for the current prepared scene pose, before camera-dependent culling or LOD. The default layer mask is 1. Empty results have valid false.",
    ),
    geometry(
        GeometryFunction::GeometryStatus3D,
        &[
            Parameter::with_type("model: scenemodel3d", &Type::SceneModel3D),
            Parameter::with_type("object: string", &Type::String),
        ],
        "Reads the geometry status for a unique object ID in the prepared scene: empty, loading, ready or error.",
    ),
    geometry(
        GeometryFunction::GeometryError3D,
        &[
            Parameter::with_type("model: scenemodel3d", &Type::SceneModel3D),
            Parameter::with_type("object: string", &Type::String),
        ],
        "Returns the error for a failed text geometry request. Returns an empty string in other states.",
    ),
    geometry(
        GeometryFunction::Prepare3D,
        &[
            Parameter::new("objects: [record]"),
            Parameter::optional("world?: physicsworld3d", &Type::PhysicsWorld3D),
        ],
        "Prepares an immutable scene for views and spatial queries. An optional physics world resolves physics-body links at the current interpolated pose.",
    ),
    geometry(
        GeometryFunction::Combine3D,
        &[
            Parameter::with_type("shared: scenemodel3d", &Type::SceneModel3D),
            Parameter::with_type("local: scenemodel3d", &Type::SceneModel3D),
        ],
        "Combines two prepared scenes in shared-then-local row order. The views and spatial queries share the result, and each input keeps its own resolved hierarchy.",
    ),
    geometry(
        GeometryFunction::TextureRgba8,
        &[
            Parameter::with_type("width: int", &Type::Int),
            Parameter::with_type("height: int", &Type::Int),
            Parameter::with_type("pixels: [int]", &Type::IntList),
        ],
        "Prepares top-to-bottom RGBA8 pixels as a texture snapshot. Pass exactly width × height × 4 bytes, each in 0..255, with dimensions in 1..2048. Invalid data returns an invalid texture with width and height 0. Pixel model edits produce a new snapshot. Use it directly as a material map or as the data field of an input record.",
    ),
    geometry(
        GeometryFunction::PrepareInstanceTable3D,
        &[Parameter::new("rows: [record]")],
        "Creates an immutable instancetable3d from model rows. Fields are optional: position (point3d), scale and rotation (vector3d), color, and data-x through data-w (float). Bind the table to one or more scene model rows. Rotation uses degrees with X pitch, Y yaw and Z roll.",
    ),
    geometry(
        GeometryFunction::RandomInstanceTable3D,
        &[Parameter::new("settings: record")],
        "Creates a seeded instancetable3d from the same settings as random-instances. Count must be an int. Bounds, grid spacing, tint and custom channels are sampled by index. A disabled generator returns an unloaded table.",
    ),
    geometry(
        GeometryFunction::Hit3D,
        ORBIT_HIT,
        "Returns the nearest visible surface along the pointer ray from the orbit camera. Read valid before using object-id, world and local position, normals, distance, or UV.",
    ),
    geometry(
        GeometryFunction::HitCamera3D,
        CAMERA_HIT,
        "Returns the nearest visible surface under a local logical-pixel point using the named camera and its clip planes.",
    ),
    geometry(
        GeometryFunction::RayHit3D,
        WORLD_RAY,
        "Traces a world-space ray with a nonzero direction up to max-distance world units. Returns a hit3d value with valid false when the ray misses.",
    ),
    geometry(
        GeometryFunction::HitAll3D,
        ORBIT_HIT,
        "Returns front-to-back hits along the pointer ray, preserving each imported glTF instance.",
    ),
    geometry(
        GeometryFunction::HitAllCamera3D,
        CAMERA_HIT,
        "Returns front-to-back hits under a pointer using a named camera. Instances keep their distinct addresses and respect camera clipping.",
    ),
    geometry(
        GeometryFunction::RayHitAll3D,
        WORLD_RAY,
        "Returns front-to-back hits along a world-space ray, including distinct instances behind other surfaces.",
    ),
    geometry(
        GeometryFunction::HitRow3D,
        &restrict_pick::<8>(ORBIT_HIT, &[HIT_ROW]),
        "Returns the hit on a single scene row at a local logical-pixel point using the orbit camera. Rows outside the model or excluded by picking and clip planes return a miss.",
    ),
    geometry(
        GeometryFunction::HitRowCamera3D,
        &restrict_pick::<6>(CAMERA_HIT, &[HIT_ROW]),
        "Returns the hit on a single scene row under a pointer using the named camera.",
    ),
    geometry(
        GeometryFunction::RayHitRow3D,
        &restrict_pick::<6>(WORLD_RAY, &[HIT_ROW]),
        "Traces a world ray against the selected scene row, up to the given maximum distance.",
    ),
    geometry(
        GeometryFunction::HitInstance3D,
        &restrict_pick::<10>(ORBIT_HIT, HIT_INSTANCE),
        "Picks one glTF instance under a pointer using the orbit camera. Pass row-index, mesh-node-index and instance-index from a hit3d value.",
    ),
    geometry(
        GeometryFunction::HitInstanceCamera3D,
        &restrict_pick::<8>(CAMERA_HIT, HIT_INSTANCE),
        "Picks one glTF instance under a pointer using a named camera and its clip planes. All three address indices are zero-based.",
    ),
    geometry(
        GeometryFunction::RayHitInstance3D,
        &restrict_pick::<8>(WORLD_RAY, HIT_INSTANCE),
        "Traces a world ray against the glTF instance identified by row, source node and instance indices. Other geometry is skipped.",
    ),
    geometry(
        GeometryFunction::HitSubset3D,
        &restrict_pick::<8>(ORBIT_HIT, &[HIT_ROWS]),
        "Returns front-to-back pointer hits for the selected scene rows. The rows can come from pick-in-rect.",
    ),
    geometry(
        GeometryFunction::HitSubsetCamera3D,
        &restrict_pick::<6>(CAMERA_HIT, &[HIT_ROWS]),
        "Returns front-to-back pointer hits for the selected rows using the named camera.",
    ),
    geometry(
        GeometryFunction::RayHitSubset3D,
        &restrict_pick::<6>(WORLD_RAY, &[HIT_ROWS]),
        "Returns front-to-back hits along a world ray, restricted to the selected rows.",
    ),
    geometry(
        GeometryFunction::PickInRect3D,
        ORBIT_RECT,
        "Returns the zero-based scene row indices of pickable models intersecting a logical-pixel view rectangle. Uses the orbit camera and includes obscured models.",
    ),
    geometry(
        GeometryFunction::PickInRectCamera3D,
        CAMERA_RECT,
        "Returns scene row indices selected by a logical-pixel rectangle with the named camera. Respects camera clipping, letterboxing, and the layer mask.",
    ),
    geometry(
        GeometryFunction::ClosestPoint3D,
        WORLD_CLOSEST_POINT,
        "Finds the nearest point on a pickable posed triangle within a world-space radius. Read valid before using position, distance, object-id, normals or UV.",
    ),
    geometry(
        GeometryFunction::ClosestPointOn3D,
        WORLD_CLOSEST_POINT_ON,
        "Finds the nearest surface on pickable models with the specified object ID, within a world-space radius.",
    ),
    geometry(
        GeometryFunction::SphereCast3D,
        WORLD_SPHERE_CAST,
        "Sweeps a world-space sphere along a nonzero direction and returns the first contact. Distance is center travel; position is the contact on the collider.",
    ),
    geometry(
        GeometryFunction::SphereOverlap3D,
        WORLD_SPHERE_OVERLAP,
        "Returns one contact for each authored collider overlapping a world-space sphere, in model row order.",
    ),
    geometry(
        GeometryFunction::SphereSlide3D,
        WORLD_SPHERE_SLIDE,
        "Moves a world-space sphere, clips remaining displacement against blocking contacts, and returns its new center.",
    ),
    geometry(
        GeometryFunction::SphereWalk3D,
        WORLD_SPHERE_WALK,
        "Resolves a world-space sphere while walking. Limits uphill slopes in degrees, steps when grounded, and follows nearby walkable ground.",
    ),
    geometry(
        GeometryFunction::CapsuleCast3D,
        WORLD_CAPSULE_CAST,
        "Sweeps a upright world-space capsule along a nonzero direction and returns the first contact. Distance is center travel; position is the contact on the collider.",
    ),
    geometry(
        GeometryFunction::CapsuleOverlap3D,
        WORLD_CAPSULE_OVERLAP,
        "Returns one contact for each authored collider overlapping a upright world-space capsule, in model row order.",
    ),
    geometry(
        GeometryFunction::CapsuleTriggerOverlap3D,
        WORLD_CAPSULE_OVERLAP,
        "Returns non-blocking trigger volumes overlapping a vertical capsule.",
    ),
    geometry(
        GeometryFunction::CapsuleTriggerCrossings3D,
        WORLD_TRIGGER_PATH,
        "Reports both entries and exits along a straight capsule movement in travel order. Use each hit's entering field to distinguish the events.",
    ),
    geometry(
        GeometryFunction::CapsuleWalkResult3D,
        WORLD_CAPSULE_WALK,
        "Resolves one capsule walk. Returns the final position, grounded state, wall and ceiling flags, and ordered trigger crossings.",
    ),
    geometry(
        GeometryFunction::CapsuleWalkWithContacts3D,
        WORLD_CAPSULE_WALK,
        "Resolves one capsule walk with ordered blocking hits in contacts and the final ground contact in support. A stationary query can identify the supporting body.",
    ),
    geometry(
        GeometryFunction::CapsuleSlide3D,
        WORLD_CAPSULE_SLIDE,
        "Moves a upright world-space capsule, clips remaining displacement against blocking contacts, and returns its new center.",
    ),
    geometry(
        GeometryFunction::CapsuleWalk3D,
        WORLD_CAPSULE_WALK,
        "Resolves a upright world-space capsule while walking. Limits uphill slopes in degrees, steps when grounded, and follows nearby walkable ground.",
    ),
    geometry(
        GeometryFunction::Project3D,
        &[
            ORBIT_VIEW[0],
            ORBIT_VIEW[1],
            ORBIT_VIEW[2],
            ORBIT_VIEW[3],
            WORLD_POINT,
        ],
        "Maps a world point into logical pixels using the orbit camera. The result has point, depth, valid, and inside fields.",
    ),
    geometry(
        GeometryFunction::ProjectCamera3D,
        &[NAMED_VIEW[0], NAMED_VIEW[1], NAMED_VIEW[2], WORLD_POINT],
        "Maps a world point into logical pixels using a named scene camera. The result has point, depth, valid, and inside fields.",
    ),
    geometry(
        GeometryFunction::Unproject3D,
        &[
            ORBIT_VIEW[0],
            ORBIT_VIEW[1],
            ORBIT_VIEW[2],
            ORBIT_VIEW[3],
            POINTER,
            VIEW_DEPTH,
            WORLD_FALLBACK,
        ],
        "Maps a logical-pixel point at a positive forward-axis depth into world space. Returns fallback for an invalid viewport, point, or depth.",
    ),
    geometry(
        GeometryFunction::UnprojectCamera3D,
        &[
            NAMED_VIEW[0],
            NAMED_VIEW[1],
            NAMED_VIEW[2],
            POINTER,
            VIEW_DEPTH,
            WORLD_FALLBACK,
        ],
        "Maps a logical-pixel point at a positive forward-axis depth into world space using a named camera. Returns fallback if the camera or input is invalid.",
    ),
    geometry(
        GeometryFunction::MapPosition3D,
        &[
            SCENE_ENDPOINTS[0],
            SCENE_ENDPOINTS[1],
            SCENE_ENDPOINTS[2],
            Parameter::with_type("position: point3d", &Type::Point3D),
            WORLD_FALLBACK,
        ],
        "Converts a position between scene node IDs, including inherited scale and pivots. An empty ID denotes world space. Invalid inputs return fallback.",
    ),
    geometry(
        GeometryFunction::MapDirection3D,
        &[
            SCENE_ENDPOINTS[0],
            SCENE_ENDPOINTS[1],
            SCENE_ENDPOINTS[2],
            Parameter::with_type("direction: vector3d", &Type::Vector3D),
            Parameter::with_type("fallback: vector3d", &Type::Vector3D),
        ],
        "Converts a direction between scene node IDs using inherited rotations and preserving length. An empty ID denotes world space. Invalid inputs return fallback.",
    ),
    geometry(
        GeometryFunction::TableInstance3D,
        &[
            Parameter::with_type("table: instancetable3d", &Type::InstanceTable3D),
            Parameter::with_type("index: int", &Type::Int),
        ],
        "Reads a zero-based source row in table coordinates. Returns origin, affine axes, color and data-x through data-w. An unloaded table or an index outside the table returns valid: false.",
    ),
    geometry(
        GeometryFunction::SceneInstance3D,
        &INSTANCE_QUERY,
        "Reads the resolved primary instance by the three source indices from a hit. Use -1 for a missing mesh node or instance-table index. Returns an immutable instance3d with valid, color, origin, world axes and data-x through data-w.",
    ),
    geometry(
        GeometryFunction::SceneInstanceCamera3D,
        &INSTANCE_CAMERA_QUERY,
        "Reads an instance using the LOD selected by a named camera, regardless of viewport culling. An invalid camera or address yields valid: false.",
    ),
    geometry(
        GeometryFunction::SceneInstanceOrbit3D,
        &INSTANCE_ORBIT_QUERY,
        "Reads an instance using the LOD selected by the orbit camera and viewport size.",
    ),
    geometry(
        GeometryFunction::Pick3D,
        ORBIT_HIT,
        "Uses the SceneView3D camera and local logical coordinates to return the nearest visible object ID, or an empty string. Parent transforms, pivots, and inherited visibility are shared with drawing.",
    ),
    geometry(
        GeometryFunction::PickCamera3D,
        CAMERA_HIT,
        "Picks the nearest visible object inside the named camera's clip planes. The camera ID, hierarchy and projection are the same as SceneView3D.camera.",
    ),
    geometry(
        GeometryFunction::Point,
        XY,
        "Creates a point in logical pixels.",
    ),
    geometry(
        GeometryFunction::Vector,
        XY,
        "Creates a displacement in logical pixels.",
    ),
    geometry(
        GeometryFunction::Identity,
        &[],
        "Returns the identity transform.",
    ),
    geometry(
        GeometryFunction::Translation,
        XY,
        "Moves points by a logical-pixel offset.",
    ),
    geometry(
        GeometryFunction::Rotation,
        &[Parameter::with_type("degrees: float", &Type::Float)],
        "Rotates clockwise by the given angle in degrees.",
    ),
    geometry(
        GeometryFunction::Scale,
        &[
            Parameter::with_type("x: float", &Type::Float),
            Parameter::with_type("y: float", &Type::Float),
        ],
        "Scales the two axes independently.",
    ),
    geometry(
        GeometryFunction::Compose,
        &[
            Parameter::with_type("first: transform2d", &Type::Transform2D),
            Parameter::with_type("second: transform2d", &Type::Transform2D),
        ],
        "Applies the first transform and then the second.",
    ),
    geometry(
        GeometryFunction::Around,
        &[TRANSFORM, POINT],
        "Applies a transform around a point in local coordinates.",
    ),
    geometry(
        GeometryFunction::MapPoint,
        &[TRANSFORM, POINT],
        "Maps a point including translation.",
    ),
    geometry(
        GeometryFunction::MapVector,
        &[TRANSFORM, VECTOR],
        "Maps a displacement by the linear part.",
    ),
    math(
        MathFunction::Min,
        &[Parameter::new("left: T"), Parameter::new("right: T")],
        "T",
        "Returns the smaller value. T can be int, float, length or percent. Int and float may be mixed.",
    ),
    math(
        MathFunction::Max,
        &[Parameter::new("left: T"), Parameter::new("right: T")],
        "T",
        "Returns the larger value. T can be int, float, length or percent. Int and float may be mixed.",
    ),
    math(
        MathFunction::Clamp,
        &[
            Parameter::new("value: T"),
            Parameter::new("min: T"),
            Parameter::new("max: T"),
        ],
        "T",
        "Limits a value to the interval formed by the bounds. Reversed bounds are swapped. Supports int, float, length and percent.",
    ),
    math(
        MathFunction::Abs,
        &[Parameter::new("value: T")],
        "T",
        "Returns the magnitude, preserving the unit. The most negative integer saturates to the largest integer.",
    ),
    math(
        MathFunction::Floor,
        &[Parameter::new("value: int | float")],
        "int",
        "Rounds down to an integer. Values outside the integer range saturate.",
    ),
    math(
        MathFunction::Ceil,
        &[Parameter::new("value: int | float")],
        "int",
        "Rounds up to an integer. Values outside the integer range saturate.",
    ),
    math(
        MathFunction::Mod,
        &[Parameter::new("value: T"), Parameter::new("divisor: T")],
        "T",
        "Returns the Euclidean remainder. T can be int, float, length or percent. For a zero divisor, returns 0 for integers and NaN otherwise.",
    ),
    math(
        MathFunction::Sign,
        &[NUMBER],
        "float",
        "Returns -1 for negative values, 1 for positive values, and 0 for either signed zero.",
    ),
    math(
        MathFunction::Sqrt,
        &[NUMBER],
        "float",
        "Returns the square root.",
    ),
    math(
        MathFunction::Pow,
        &[
            Parameter::with_type("base: float", &Type::Float),
            Parameter::with_type("exponent: float", &Type::Float),
        ],
        "float",
        "Raises the base to the exponent.",
    ),
    math(
        MathFunction::Exp,
        &[NUMBER],
        "float",
        "Raises e to the given power.",
    ),
    math(
        MathFunction::Ln,
        &[NUMBER],
        "float",
        "Returns the natural logarithm.",
    ),
    math(
        MathFunction::Log,
        &[NUMBER, Parameter::with_type("base: float", &Type::Float)],
        "float",
        "Returns the logarithm in the given base.",
    ),
    math(
        MathFunction::Sin,
        &[Parameter::with_type("radians: float", &Type::Float)],
        "float",
        "Returns the sine of an angle in radians.",
    ),
    math(
        MathFunction::Cos,
        &[Parameter::with_type("radians: float", &Type::Float)],
        "float",
        "Returns the cosine of an angle in radians.",
    ),
    math(
        MathFunction::Tan,
        &[Parameter::with_type("radians: float", &Type::Float)],
        "float",
        "Returns the tangent of an angle in radians.",
    ),
    math(
        MathFunction::Asin,
        &[NUMBER],
        "float",
        "Returns the inverse sine in radians.",
    ),
    math(
        MathFunction::Acos,
        &[NUMBER],
        "float",
        "Returns the inverse cosine in radians.",
    ),
    math(
        MathFunction::Atan,
        &[NUMBER],
        "float",
        "Returns the inverse tangent in radians.",
    ),
    math(
        MathFunction::Atan2,
        &[
            Parameter::with_type("y: float", &Type::Float),
            Parameter::with_type("x: float", &Type::Float),
        ],
        "float",
        "Returns the angle from the positive x axis to (x, y), in radians.",
    ),
    math(
        MathFunction::Hypot,
        &[
            Parameter::with_type("x: float", &Type::Float),
            Parameter::with_type("y: float", &Type::Float),
        ],
        "float",
        "Returns the length of the hypotenuse.",
    ),
    math(
        MathFunction::ToRadians,
        &[Parameter::with_type("degrees: float", &Type::Float)],
        "float",
        "Converts degrees to radians.",
    ),
    math(
        MathFunction::ToDegrees,
        &[Parameter::with_type("radians: float", &Type::Float)],
        "float",
        "Converts radians to degrees.",
    ),
    math(
        MathFunction::IsFinite,
        &[NUMBER],
        "bool",
        "Returns true for finite numbers, excluding infinities and NaN.",
    ),
    string(
        StringFunction::Trim,
        &[TEXT],
        "string",
        "Removes leading and trailing Unicode whitespace.",
    ),
    string(
        StringFunction::IsEmpty,
        &[TEXT],
        "bool",
        "Returns true if the string contains no characters.",
    ),
    string(
        StringFunction::Contains,
        &[
            TEXT,
            Parameter::with_type("substring: string", &crate::ast::Type::String),
        ],
        "bool",
        "Checks for a case-sensitive substring.",
    ),
    string(
        StringFunction::StartsWith,
        &[
            TEXT,
            Parameter::with_type("prefix: string", &crate::ast::Type::String),
        ],
        "bool",
        "Checks for a case-sensitive prefix.",
    ),
    string(
        StringFunction::CharacterCount,
        &[TEXT],
        "int",
        "Counts Unicode grapheme clusters, including combined emoji and letters with combining marks.",
    ),
    string(
        StringFunction::EndsWith,
        &[TEXT, Parameter::with_type("suffix: string", &Type::String)],
        "bool",
        "Checks for a case-sensitive suffix.",
    ),
    string(
        StringFunction::ReplaceAll,
        &[
            TEXT,
            Parameter::with_type("from: string", &Type::String),
            Parameter::with_type("to: string", &Type::String),
        ],
        "string",
        "Replaces every non-overlapping occurrence of from with to.",
    ),
    string(
        StringFunction::Lowercase,
        &[TEXT],
        "string",
        "Converts to Unicode lowercase.",
    ),
    string(
        StringFunction::Uppercase,
        &[TEXT],
        "string",
        "Converts to Unicode uppercase.",
    ),
    string(
        StringFunction::IsInt,
        &[TEXT],
        "bool",
        "Checks whether the trimmed string is an integer within the XR int range.",
    ),
    string(
        StringFunction::IsFloat,
        &[TEXT],
        "bool",
        "Checks whether the trimmed string is a finite number using a dot as decimal separator.",
    ),
    string(
        StringFunction::ToInt,
        &[TEXT, Parameter::with_type("fallback: int", &Type::Int)],
        "int",
        "Parses a signed integer or returns the provided fallback.",
    ),
    string(
        StringFunction::ToFloat,
        &[TEXT, Parameter::with_type("fallback: float", &Type::Float)],
        "float",
        "Parses a finite number using a dot as decimal separator or returns the provided fallback.",
    ),
    Function {
        namespace: "Model",
        member: "count",
        kind: FunctionKind::ModelCount,
        parameters: &[MODEL],
        result: Some("int"),
        documentation: "Returns the number of rows. Bindings update when rows are inserted or removed.",
    },
    Function {
        namespace: "Model",
        member: "is-empty",
        kind: FunctionKind::ModelIsEmpty,
        parameters: &[MODEL],
        result: Some("bool"),
        documentation: "Returns true when the model has no rows.",
    },
    Function {
        namespace: "Model",
        member: "at",
        kind: FunctionKind::ModelAt,
        parameters: &[
            MODEL,
            Parameter::with_type("index: int", &Type::Int),
            Parameter::new("fallback: Row"),
        ],
        result: Some("Row"),
        documentation: "Reads a zero-based row. Evaluates the fallback only if the row does not exist.",
    },
    Function {
        namespace: "Model",
        member: "filter",
        kind: FunctionKind::ModelFilter,
        parameters: &[
            MODEL,
            Parameter::with_type("field: string", &Type::String),
            TEXT,
        ],
        result: Some("[Row]"),
        documentation: "Projects a record model using a case-insensitive text search on a constant string field path. Preserves row keys.",
    },
    Function {
        namespace: "Model",
        member: "lookup",
        kind: FunctionKind::ModelLookup,
        parameters: &[
            MODEL,
            Parameter::with_type("field: string", &Type::String),
            Parameter::with_type("key: string", &Type::String),
        ],
        result: Some("[Row]"),
        documentation: "Looks up an exact string key on a constant field path. Preserves source order and row keys.",
    },
    Function {
        namespace: "Model",
        member: "sort-by",
        kind: FunctionKind::ModelSortBy,
        parameters: &[
            MODEL,
            Parameter::with_type("field: string", &Type::String),
            Parameter::with_type("descending: bool", &Type::Bool),
        ],
        result: Some("[Row]"),
        documentation: "Returns a read-only sorted projection of a record model by a constant int or string field path. Preserves row keys.",
    },
    model_edit(
        ModelEditKind::Append,
        &[WRITABLE_MODEL, MODEL_ROW],
        "Appends one typed row to a writable model. Existing rows keep their identities.",
    ),
    model_edit(
        ModelEditKind::Insert,
        &[WRITABLE_MODEL, MODEL_INDEX, MODEL_ROW],
        "Inserts a typed row before a zero-based index. The model length is a valid insertion index.",
    ),
    model_edit(
        ModelEditKind::Set,
        &[WRITABLE_MODEL, MODEL_INDEX, MODEL_ROW],
        "Replaces one row's data and keeps its identity.",
    ),
    model_edit(
        ModelEditKind::Remove,
        &[WRITABLE_MODEL, MODEL_INDEX],
        "Removes one row at a zero-based index. Other rows keep their identities.",
    ),
    model_edit(
        ModelEditKind::Move,
        &[
            WRITABLE_MODEL,
            Parameter::with_type("from: int", &Type::Int),
            Parameter::with_type("to: int", &Type::Int),
        ],
        "Moves one row to its final index while preserving its identity.",
    ),
    model_edit(
        ModelEditKind::Clear,
        &[WRITABLE_MODEL],
        "Removes all rows from the existing model in one operation.",
    ),
    color(
        ColorFunction::Rgb,
        RGB,
        "Creates an opaque color. Channels are clamped to 0–255.",
    ),
    color(
        ColorFunction::Rgba,
        RGBA,
        "Creates a color with a clamped opacity between 0 and 1.",
    ),
    color(
        ColorFunction::WithAlpha,
        &[COLOR, OPACITY],
        "Returns the color with its opacity replaced.",
    ),
    color(
        ColorFunction::Mix,
        &[
            COLOR,
            Parameter::with_type("other: color", &Type::Color),
            Parameter::with_type("amount: float", &Type::Float),
        ],
        "Interpolates RGBA channels from color to other. Amount is clamped to 0–1.",
    ),
    Function {
        namespace: "Dates",
        member: "is-valid",
        kind: FunctionKind::DateIsValid,
        parameters: &[DATE],
        result: Some("bool"),
        documentation: "Checks whether a date exists in the Gregorian calendar, with years from 1 to 9999.",
    },
    Function {
        namespace: "Dates",
        member: "add-days",
        kind: FunctionKind::DateAddDays,
        parameters: &[
            DATE,
            Parameter::with_type("days: int", &Type::Int),
            DATE_FALLBACK,
        ],
        result: Some("Date"),
        documentation: "Adds signed calendar days. Evaluates the fallback only when the input is invalid or the result is outside the supported years.",
    },
    Function {
        namespace: "Dates",
        member: "add-months",
        kind: FunctionKind::DateAddMonths,
        parameters: &[
            DATE,
            Parameter::with_type("months: int", &Type::Int),
            DATE_FALLBACK,
        ],
        result: Some("Date"),
        documentation: "Adds signed calendar months. Clamps the day to the end of the target month. Evaluates the fallback only when the input is invalid or the result is outside the supported years.",
    },
    Function {
        namespace: "Dates",
        member: "weekday",
        kind: FunctionKind::DateWeekday,
        parameters: &[DATE],
        result: Some("int"),
        documentation: "Returns the ISO weekday, Monday (1) through Sunday (7), or 0 for an invalid date.",
    },
    Function {
        namespace: "Dates",
        member: "parse-iso",
        kind: FunctionKind::DateParseIso,
        parameters: &[
            Parameter::with_type("text: string", &Type::String),
            DATE_FALLBACK,
        ],
        result: Some("Date"),
        documentation: "Parses an ASCII YYYY-MM-DD Gregorian date. Evaluates the fallback only for invalid or incomplete input.",
    },
    Function {
        namespace: "Dates",
        member: "is-selectable",
        kind: FunctionKind::DateIsSelectable,
        parameters: &[
            DATE,
            Parameter::with_type("minimum: Date", &Type::Date),
            Parameter::with_type("maximum: Date", &Type::Date),
            Parameter::with_type("disabled-weekdays: [int]", &Type::IntList),
            Parameter::with_type("disabled-dates: [Date]", &Type::DateList),
        ],
        result: Some("bool"),
        documentation: "Tests the date against the same bounds, disabled weekdays, and disabled dates used by Calendar and DatePickerPopup.",
    },
    Function {
        namespace: "Dates",
        member: "months",
        kind: FunctionKind::DateMonths,
        parameters: &[
            Parameter::with_type("from: Date", &Type::Date),
            Parameter::with_type("to: Date", &Type::Date),
        ],
        result: Some("[Date]"),
        documentation: "Returns the first day of each month intersecting the inclusive date range. Invalid or reversed ranges return an empty model.",
    },
    Function {
        namespace: "Dates",
        member: "month-index",
        kind: FunctionKind::DateMonthIndex,
        parameters: &[
            Parameter::with_type("from: Date", &Type::Date),
            Parameter::with_type("to: Date", &Type::Date),
            DATE,
        ],
        result: Some("int"),
        documentation: "Returns the zero-based index of the date's month in Dates.months(from, to), or -1 when the month or date is invalid.",
    },
    Function {
        namespace: "Times",
        member: "parse-iso",
        kind: FunctionKind::TimeParseIso,
        parameters: &[
            Parameter::with_type("text: string", &Type::String),
            Parameter::with_type("fallback: Time", &Type::Time),
        ],
        result: Some("Time"),
        documentation: "Parses ASCII HH:mm or HH:mm:ss. Omitted seconds are zero. Invalid input evaluates the fallback.",
    },
    Function {
        namespace: "Times",
        member: "is-valid-iso",
        kind: FunctionKind::TimeIsValidIso,
        parameters: &[Parameter::with_type("text: string", &Type::String)],
        result: Some("bool"),
        documentation: "Checks that an ASCII HH:mm or HH:mm:ss string represents a time of day.",
    },
    Function {
        namespace: "Times",
        member: "format-iso",
        kind: FunctionKind::TimeFormatIso,
        parameters: &[
            Parameter::with_type("time: Time", &Type::Time),
            Parameter::with_type("show-seconds: bool", &Type::Bool),
        ],
        result: Some("string"),
        documentation: "Formats a time as zero-padded HH:mm or HH:mm:ss in 24-hour notation.",
    },
    Function {
        namespace: "Times",
        member: "in-range",
        kind: FunctionKind::TimeInRange,
        parameters: &[
            Parameter::with_type("time: Time", &Type::Time),
            Parameter::with_type("minimum: Time", &Type::Time),
            Parameter::with_type("maximum: Time", &Type::Time),
        ],
        result: Some("bool"),
        documentation: "Checks the inclusive time range used by TimePickerPopup. Reversed ranges contain no times.",
    },
    Function {
        namespace: "I18n",
        member: "locale",
        kind: FunctionKind::Locale,
        parameters: &[],
        result: Some("string"),
        documentation: "Returns the negotiated catalog locale. Without a catalog, returns the requested locale or und. Updates when the UI language changes.",
    },
    Function {
        namespace: "I18n",
        member: "format-number",
        kind: FunctionKind::FormatNumber,
        parameters: &[
            Parameter::new("value: int | float"),
            Parameter::with_type("decimals: int", &Type::Int),
        ],
        result: Some("string"),
        documentation: "Formats a number with localized digits and grouping. The decimal count is clamped to 0–20. Uses the requested UI locale, or the catalog default if none was requested.",
    },
    Function {
        namespace: "I18n",
        member: "parse-number",
        kind: FunctionKind::ParseNumber,
        parameters: &[TEXT, Parameter::with_type("fallback: float", &Type::Float)],
        result: Some("float"),
        documentation: "Parses a localized decimal number, with or without locale-specific grouping. Returns the fallback for invalid or non-finite input. Uses the requested UI locale, or the catalog default if none was requested.",
    },
    Function {
        namespace: "Math",
        member: "round",
        kind: FunctionKind::Round,
        parameters: &[Parameter::new("value: int | float")],
        result: Some("int"),
        documentation: "Rounds to the nearest integer, with ties away from zero. Values outside the integer range saturate.",
    },
    Function {
        namespace: "Strings",
        member: "format",
        kind: FunctionKind::Format,
        parameters: &[
            Parameter::new("template: string literal"),
            Parameter::new("values..."),
        ],
        result: Some("string"),
        documentation: "Formats values into sequential `{}` placeholders. Use `{{` and `}}` for literal braces. Use `{:.2}` for two decimal places. The compiler checks the template and argument count.",
    },
    Function {
        namespace: "Log",
        member: "write",
        kind: FunctionKind::Log,
        parameters: &[
            Parameter::with_type("level: LogLevel", &crate::ast::Type::LogLevel),
            Parameter::with_type("message: string", &crate::ast::Type::String),
        ],
        result: None,
        documentation: "Emits a structured `xrack::ui` event at the specified level. Use it as a statement in an effectful handler or callback.",
    },
    log_at(
        "trace",
        "Emits a trace-level UI event. Shorthand for `Log.write(LogLevel.trace, message)`.",
    ),
    log_at(
        "debug",
        "Emits a debug-level UI event. Shorthand for `Log.write(LogLevel.debug, message)`.",
    ),
    log_at(
        "info",
        "Emits an info-level UI event. Shorthand for `Log.write(LogLevel.info, message)`.",
    ),
    log_at(
        "warn",
        "Emits a warning-level UI event. Shorthand for `Log.write(LogLevel.warn, message)`.",
    ),
    log_at(
        "error",
        "Emits an error-level UI event. Shorthand for `Log.write(LogLevel.error, message)`.",
    ),
];

pub fn function(namespace: &str, member: &str) -> Option<&'static Function> {
    FUNCTIONS
        .iter()
        .find(|function| function.namespace == namespace && function.member == member)
}

pub struct Constant {
    pub namespace: &'static str,
    pub member: &'static str,
    pub value: f64,
    pub documentation: &'static str,
}

pub const CONSTANTS: &[Constant] = &[
    Constant {
        namespace: "Math",
        member: "pi",
        value: std::f64::consts::PI,
        documentation: "The ratio of a circle's circumference to its diameter.",
    },
    Constant {
        namespace: "Math",
        member: "tau",
        value: std::f64::consts::TAU,
        documentation: "The number of radians in a full turn.",
    },
    Constant {
        namespace: "Math",
        member: "e",
        value: std::f64::consts::E,
        documentation: "The base of the natural logarithm.",
    },
];

pub fn constant(namespace: &str, member: &str) -> Option<&'static Constant> {
    CONSTANTS
        .iter()
        .find(|constant| constant.namespace == namespace && constant.member == member)
}
