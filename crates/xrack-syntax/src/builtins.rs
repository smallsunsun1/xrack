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
    "Vector2D",
    "Transform2D",
    "Scene3D",
];
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[derive(Clone, Copy)]
pub enum FunctionKind {
    Round,
    Math(MathFunction),
    Geometry(GeometryFunction),
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
    Pick3D => "pick-3d",
    PickCamera3D => "pick-camera-3d",
    Point3D => "point-3d",
    Vector3D => "vector-3d",
    Hit3D => "hit-3d",
    HitCamera3D => "hit-camera-3d",
    RayHit3D => "ray-hit-3d",
    HitAll3D => "hit-all-3d",
    HitAllCamera3D => "hit-all-camera-3d",
    RayHitAll3D => "ray-hit-all-3d",
    SphereCast3D => "sphere-cast-3d",
    SphereOverlap3D => "sphere-overlap-3d",
    SphereSlide3D => "sphere-slide-3d",
    SphereWalk3D => "sphere-walk-3d",
    Project3D => "project-3d",
    ProjectCamera3D => "project-camera-3d",
    Unproject3D => "unproject-3d",
    UnprojectCamera3D => "unproject-camera-3d",
});

impl GeometryFunction {
    pub const fn supports_layer_mask(self) -> bool {
        matches!(
            self,
            Self::Pick3D
                | Self::PickCamera3D
                | Self::Hit3D
                | Self::HitCamera3D
                | Self::HitAll3D
                | Self::HitAllCamera3D
                | Self::RayHit3D
                | Self::RayHitAll3D
                | Self::SphereCast3D
                | Self::SphereOverlap3D
                | Self::SphereSlide3D
                | Self::SphereWalk3D
        )
    }

    pub const fn result(self) -> Type {
        match self {
            Self::Point | Self::MapPoint => Type::Point2D,
            Self::Vector | Self::MapVector => Type::Vector2D,
            Self::Pick3D | Self::PickCamera3D => Type::String,
            Self::Point3D
            | Self::Unproject3D
            | Self::UnprojectCamera3D
            | Self::SphereSlide3D
            | Self::SphereWalk3D => Type::Point3D,
            Self::Project3D | Self::ProjectCamera3D => Type::ProjectedPoint3D,
            Self::Vector3D => Type::Vector3D,
            Self::Hit3D | Self::HitCamera3D | Self::RayHit3D | Self::SphereCast3D => Type::Hit3D,
            Self::HitAll3D | Self::HitAllCamera3D | Self::RayHitAll3D | Self::SphereOverlap3D => {
                Type::Hit3DList
            }
            _ => Type::Transform2D,
        }
    }
    pub const fn uses_scene_model(self) -> bool {
        matches!(
            self,
            Self::Pick3D
                | Self::PickCamera3D
                | Self::ProjectCamera3D
                | Self::UnprojectCamera3D
                | Self::Hit3D
                | Self::HitCamera3D
                | Self::RayHit3D
                | Self::HitAll3D
                | Self::HitAllCamera3D
                | Self::RayHitAll3D
                | Self::SphereCast3D
                | Self::SphereOverlap3D
                | Self::SphereSlide3D
                | Self::SphereWalk3D
        )
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
}

impl Parameter {
    pub const fn new(label: &'static str) -> Self {
        Self {
            label,
            expected: None,
        }
    }

    pub const fn with_type(label: &'static str, ty: &'static crate::ast::Type) -> Self {
        Self {
            label,
            expected: Some(ty),
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
        self.parameters.len()
            - usize::from(
                matches!(self.kind, FunctionKind::Geometry(kind) if kind.supports_layer_mask()),
            )
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
    ("mesh", Type::String),
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

pub const SCENE_OBJECT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("animation", Type::String),
    ("animation-time", Type::Float),
    ("animation-loop", Type::Bool),
    ("animation-to", Type::String),
    ("animation-to-time", Type::Float),
    ("animation-to-loop", Type::Bool),
    ("animation-blend", Type::Float),
    ("parent", Type::String),
    ("visible", Type::Bool),
    ("lod-of", Type::String),
    ("lod-max-size", Type::Length),
    ("lod-bias", Type::Float),
    ("camera-lod-bias", Type::Float),
    ("pickable", Type::Bool),
    ("collidable", Type::Bool),
    ("pivot-x", Type::Float),
    ("pivot-y", Type::Float),
    ("pivot-z", Type::Float),
    ("camera-kind", Type::String),
    ("camera-fov", Type::Float),
    ("camera-height", Type::Float),
    ("camera-look-at", Type::String),
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
    ("lightmap-intensity", Type::Float),
    ("lightmap-u", Type::Float),
    ("lightmap-v", Type::Float),
    ("lightmap-width", Type::Float),
    ("lightmap-height", Type::Float),
];

pub const SCENE_PARTICLE_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("color", Type::Color),
    ("size", Type::Float),
    ("lifetime", Type::Float),
];

pub const SCENE_PARTICLE_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("end-size", Type::Float),
    ("start-time", Type::Float),
    ("rate", Type::Float),
    ("burst-count", Type::Int),
    ("velocity", Type::Vector3D),
    ("velocity-spread", Type::Vector3D),
    ("gravity", Type::Vector3D),
    ("fade-in", Type::Float),
    ("fade-out", Type::Float),
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
    ("color-table", Type::Image),
    ("particle-blend-mode", Type::String),
    ("emitter-shape", Type::String),
    ("emitter-mesh", Type::String),
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
    ("velocity-speed-variation", Type::Float),
    ("trail-segments", Type::Int),
    ("trail-duration", Type::Float),
    ("trail-end-scale", Type::Float),
    ("trail-fade", Type::Float),
    ("trail-eol-fade", Type::Float),
];

pub const SCENE_PARTICLE_AFFECTOR_FIELDS: &[(&str, Type)] = &[("affector-kind", Type::String)];
pub const SCENE_PARTICLE_AFFECTOR_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("position", Type::Point3D),
    ("position-variation", Type::Vector3D),
    ("direction", Type::Vector3D),
    ("duration", Type::Float),
    ("duration-variation", Type::Float),
    ("start-age", Type::Float),
    ("hide-at-end", Type::Bool),
    ("radius", Type::Float),
    ("outer-radius", Type::Float),
    ("strength", Type::Float),
    ("angular-speed", Type::Float),
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
];

pub const PARTICLE_EMITTER_SHAPES: &[(&str, &str)] = &[
    ("point", "Emit from the origin"),
    ("mesh", "Emit from the surface of a glTF mesh"),
    ("box", "Emit from a box volume or its faces"),
    ("sphere", "Emit from an ellipsoid volume or its surface"),
    (
        "cylinder",
        "Emit from a Y-axis cylinder with an elliptical cross section",
    ),
];

pub const PARTICLE_VELOCITY_MODES: &[(&str, &str)] = &[
    (
        "surface-normal",
        "Emit along the sampled mesh triangle normal",
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
        "Aim from each birth position toward a world-space target",
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
    ("intensity", Type::Float),
    ("parallax-correction", Type::Bool),
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
pub const MESH_SOURCE_EXTENSIONS: &[&str] = &["glb", "gltf", HEIGHTFIELD_EXTENSION];

pub const MESH_URL_OPTIONS: &[(&str, &str)] = &[
    ("auto-lod", "Generate mesh levels of detail"),
    (
        "lightmap-uv",
        "Generate independent nonoverlapping UV charts for lightmap baking",
    ),
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

pub const SCENE_CAMERA_KINDS: &[(&str, &str)] = &[
    (
        "perspective",
        "Perspective camera with a vertical field of view",
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
pub const SCENE_LIGHT_OPTIONAL_FIELDS: &[(&str, Type)] = &[
    ("layers", Type::Int),
    ("casts-shadow", Type::Bool),
    ("parent", Type::String),
    ("visible", Type::Bool),
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
        GeometryFunction::Point => ("Point2D", "new", "point2d"),
        GeometryFunction::Point3D => ("Point3D", "new", "point3d"),
        GeometryFunction::Vector3D => ("Vector3D", "new", "vector3d"),
        GeometryFunction::Hit3D => ("Scene3D", "hit", "hit3d"),
        GeometryFunction::HitCamera3D => ("Scene3D", "hit-camera", "hit3d"),
        GeometryFunction::RayHit3D => ("Scene3D", "ray-hit", "hit3d"),
        GeometryFunction::HitAll3D => ("Scene3D", "hit-all", "[hit3d]"),
        GeometryFunction::HitAllCamera3D => ("Scene3D", "hit-all-camera", "[hit3d]"),
        GeometryFunction::RayHitAll3D => ("Scene3D", "ray-hit-all", "[hit3d]"),
        GeometryFunction::SphereCast3D => ("Scene3D", "sphere-cast", "hit3d"),
        GeometryFunction::SphereOverlap3D => ("Scene3D", "sphere-overlap", "[hit3d]"),
        GeometryFunction::SphereSlide3D => ("Scene3D", "sphere-slide", "point3d"),
        GeometryFunction::SphereWalk3D => ("Scene3D", "sphere-walk", "point3d"),
        GeometryFunction::Vector => ("Vector2D", "new", "vector2d"),
        GeometryFunction::MapPoint => ("Transform2D", kind.name(), "point2d"),
        GeometryFunction::MapVector => ("Transform2D", kind.name(), "vector2d"),
        GeometryFunction::Pick3D => ("Scene3D", "pick", "string"),
        GeometryFunction::PickCamera3D => ("Scene3D", "pick-camera", "string"),
        GeometryFunction::Project3D => ("Scene3D", "project", "projectedpoint3d"),
        GeometryFunction::ProjectCamera3D => ("Scene3D", "project-camera", "projectedpoint3d"),
        GeometryFunction::Unproject3D => ("Scene3D", "unproject", "point3d"),
        GeometryFunction::UnprojectCamera3D => ("Scene3D", "unproject-camera", "point3d"),
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

const WORLD_POINT: Parameter = Parameter::with_type("world: point3d", &Type::Point3D);
const POINTER: Parameter = Parameter::with_type("pointer: point2d", &Type::Point2D);
const VIEWPORT: Parameter = Parameter::with_type("viewport: point2d", &Type::Point2D);
const VIEW_DEPTH: Parameter = Parameter::with_type("depth: float", &Type::Float);
const WORLD_FALLBACK: Parameter = Parameter::with_type("fallback: point3d", &Type::Point3D);
const LAYER_MASK: Parameter = Parameter::with_type("layer-mask?: int", &Type::Int);
const ORBIT_VIEW: [Parameter; 4] = [
    Parameter::with_type("yaw: float", &Type::Float),
    Parameter::with_type("pitch: float", &Type::Float),
    Parameter::with_type("distance: float", &Type::Float),
    VIEWPORT,
];
const NAMED_VIEW: [Parameter; 3] = [
    Parameter::new("objects: [record]"),
    Parameter::with_type("camera: string", &Type::String),
    VIEWPORT,
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

pub const FUNCTIONS: &[Function] = &[
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
        "Returns the nearest accepted surface of each pickable scene row along the pointer ray, ordered nearest first.",
    ),
    geometry(
        GeometryFunction::HitAllCamera3D,
        CAMERA_HIT,
        "Returns front-to-back hits under a pointer using a named camera. Geometry remains subject to the camera clip planes.",
    ),
    geometry(
        GeometryFunction::RayHitAll3D,
        WORLD_RAY,
        "Returns front-to-back hits along a world-space ray, including objects behind other surfaces. Use Model.at or iterate the result as [hit3d].",
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
