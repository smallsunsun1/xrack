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

/// Bindings used by a text editor's final submission status.
pub const TEXT_SUBMISSION_INPUTS: &[&str] = &[
    "text",
    "input-type",
    "decimal-separator",
    "maximum-length",
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

/// Rules that are evaluated when checking the final value rather than rendering an editor.
pub fn text_submission_rule(kind: &str, name: &str) -> bool {
    text_submission_editor(kind) && matches!(name, "minimum-length" | "submit-when")
        || text_submission_single_line(kind)
            && matches!(name, "minimum-value" | "maximum-value" | "decimals")
}

pub const NAMESPACES: &[&str] = &["Math", "Strings", "Model", "Colors", "Dates", "I18n", "Log"];
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

#[derive(Clone, Copy)]
pub enum FunctionKind {
    Round,
    Math(MathFunction),
    String(StringFunction),
    ModelCount,
    ModelIsEmpty,
    ModelAt,
    ModelEdit(ModelEditKind),
    Color(ColorFunction),
    DateIsValid,
    DateAddDays,
    DateAddMonths,
    DateWeekday,
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
const MODEL: Parameter = Parameter::new("model: [int] | [string] | [Record]");
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

pub const FUNCTIONS: &[Function] = &[
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
