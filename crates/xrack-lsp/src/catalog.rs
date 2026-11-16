// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action ActionGroup AnimatedImage ApplicationWindow MediaPlayer BusyIndicator Button ButtonGroup Calendar CheckBox CheckDelegate ColorPickerPopup ComboBox ContextMenuArea DatePickerPopup DelayButton Dial Dialog ControlFrame Pane EmptyState
    ButtonContent SliderHandle RangeLowerHandle RangeUpperHandle ToggleIndicator SwitchThumb SpinUpIndicator SpinDownIndicator DayCell DayOfWeekRow DialogButton DialogButtonBox DoubleSpinBox DragArea Drawer DropArea FileDialog FontPickerPopup Flickable FlexboxLayout FlowLayout FocusScope GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    HoverHandler ItemDelegate ListView Menu MenuBar MenuItem MenuSeparator MessageDialog MonthGrid Page PageFooter PageHeader PageIndicator Path PopupWindow ProgressIndicator RadioButton RadioDelegate
    RadioGroup RangeSlider Rectangle RoundButton Row ScaleRotateGestureHandler ScrollBar ScrollIndicator ScrollView SearchField SecondaryWindow SectionHeader Shortcut Slider SpinBox Spinner SplitView StackPage StackView
    StyledText StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeGestureHandler SwipeLeft SwipePage SwipeRight SwipeView Switch SwitchDelegate Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    SystemTrayIcon TextEdit TextInput TimePicker TimePickerPopup Timer ToolBar ToolButton ToolSeparator Tooltip TouchArea TreeCell TreeDisclosure TreeRow TreeView VerticalBox
    Tumbler VerticalLayout WeekNumberColumn WheelHandler Window WindowMoveArea";

pub(crate) const PROPERTIES: &str = "autoplay duration volume ready ended error muted playback-rate network-bandwidth audio-track subtitle-track subtitle-tracks subtitle-file status seekable seek-start seek-end at-live-edge has-audio has-video audio-tracks media-title subtitle audio-error subtitle-error buffer-progress bitrate accent-color accept-files accept-text accepted-buttons accessible-description accessible-name action alignment anchor-gap animated allow-copy allow-link allow-move auto-complete auto-repeat auto-repeat-delay auto-repeat-interval auto-sort cancel-text blocking cursor-shape section-field section-height sticky-section-headers
    handle-swipe-left handle-swipe-right handle-swipe-up handle-swipe-down
    background bounds-behavior bounds-movement border-bottom-left-radius border-bottom-right-radius border-color border-radius buttons
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state checkable checked clip
    close-on-escape close-on-outside col color color-scheme colorize colspan commands content-height content-padding content-width content-x content-y copyable count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    edit-on-click edit-on-selected-click edit-on-double-click edit-on-key edit-on-tab edit-on-typing
    current-frame current-page current-row current-value date decimal-separator decimals delay depth dim directory display-month dot-size data drag-key drag-margin drag-select drop-action drop-shadow-blur drop-shadow-color drop-shadow-offset-x drop-shadow-offset-y drop-shadow-spread edge edit-text editable
    default-font default-font-family default-font-italic default-font-size default-font-weight enabled end-angle exclusive field flick-direction file-extensions file-name fill filter-field filter-text fit flex-direction flex-wrap frame-count frozen-columns focus focus-on-click focus-on-tab-navigation focus-within follow-current font preview-text group has-focus font-family font-italic font-size font-weight handle-color handle-hit-size handle-size header-height height
    highlighted horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch horizontal-tile-mode icon icon-only icon-size icon-template tooltip
    image-fit image-rendering indentation indeterminate informative-text inner-shadow-blur inner-shadow-color inner-shadow-offset-x inner-shadow-offset-y inner-shadow-spread initial-focus initial-page input-mode input-type interactive interval key keys kind layout-height layout-direction layout-order layout-ready layout-width layout-x layout-y level link-color live loops
    lower-value max-height max-lines max-width maximum maximum-date maximum-length maximum-value minimum-length minimum-value submit-when min-height min-width minimum minimum-date minimum-size modal mode model mouse-drag-pan-enabled movable-columns movable-rows multi-select multi-touch name-filters no-frame open open-side opacity overflow placement playing resize-border-width
    orientation padding padding-bottom padding-left padding-right padding-top parent-key policy position size
    context-menu-enabled password-visible pasteable undoable paused placeholder-text preferred-height preferred-width preserve-scroll-anchor press-and-hold-interval primary progress progress-color read-only rebound-duration resizable resizable-columns resizable-rows restore-focus
    radius repeat role root-key row row-height row-height-field row-header-width row-header-text row-header-field row-key-field row-visible-field rowspan running selected-view-index selection-target sequence sequences shortcut sortable source source-clip-x source-clip-y source-clip-width source-clip-height speed spacing spacing-horizontal spacing-vertical start-angle step
    enabled-role show-alpha show-color-plane show-seconds snap-mode stack-index step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title today track-color transform-origin-x transform-origin-y transform-rotation transform-scale transform-scale-x transform-scale-y transition-duration upper-value use-24-hour-format value value-role
    typeahead-enabled typeahead-field vertical-alignment vertical-scroll-bar-policy vertical-spacing vertical-stretch vertical-tile-mode viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels wheel-enabled width window-bounds-height window-bounds-width window-bounds-x window-bounds-y wrap z";

pub(crate) const EVENTS: &str = "accepted activated activated-ambiguously applied canceled cancelled capture-key-pressed capture-key-released changed check-state-changed clicked close-requested closed collapsed deactivated wheel pointer-event
    column-moved current-cell-changed current-row-changed discarded drag-finished drag-started dropped edited entered exited expanded failed files-dropped files-entered finished focus-changed-event focus-gained focus-lost focus-within-changed help-requested key-pressed key-released leave-requested link-activated month-changed opened position-changed pressed rejected released removed reset resized row-moved search-triggered selected selection-changed
    double-clicked ended flicked moved press-and-hold scroll-event scrolled sort-ascending sort-descending started swiped toggled triggered updated value-selected";

pub(crate) const TYPES: &str = "int float bool string styled-text length percent color brush image [string] WheelEvent CursorShape PointerEvent PointerEventKind PointerButton PointerButtons TouchPoint TouchPoints [TouchPoint]
    ImageFit ImageTileMode ImageHorizontalAlignment ImageVerticalAlignment ImageRendering LayoutAlignment CrossAxisAlignment Orientation FlickDirection FlickBoundsBehavior FlickBoundsMovement LayoutDirection FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy ScrollSnapMode ScrollAlignment TextWrap TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType
    CheckState ColorScheme Date DialogRole DragAction DrawerEdge EventResult FocusReason Font KeyEvent KeyModifiers FileDialogMode LogLevel MessageButtons MessageLevel PopupPlacement SelectionTarget StandardButtonKind SwipeSide Time";

pub(crate) struct BuiltinFunction {
    pub name: &'static str,
    pub parameters: &'static [&'static str],
    pub result: Option<&'static str>,
    pub documentation: &'static str,
}

impl BuiltinFunction {
    pub fn signature(&self) -> String {
        let mut label = format!("{}({})", self.name, self.parameters.join(", "));
        if let Some(result) = self.result {
            label.push_str(" -> ");
            label.push_str(result);
        }
        label
    }
}

pub(crate) fn builtin_function(name: &str) -> Option<&'static BuiltinFunction> {
    FUNCTIONS.iter().find(|function| function.name == name)
}

pub(crate) const FUNCTIONS: &[BuiltinFunction] = &[
    BuiltinFunction {
        name: "@tr",
        parameters: &["id: string literal", "named arguments..."],
        result: Some("string"),
        documentation: "Looks up a Fluent message in the current locale. Supply arguments as `name: value`.",
    },
    BuiltinFunction {
        name: "@image-url",
        parameters: &["path: string literal", "nine-slice(...) (optional)"],
        result: Some("image"),
        documentation: "Loads an image relative to the source file. An optional `nine-slice(...)` argument defines the image's slice margins.",
    },
    BuiltinFunction {
        name: "@markdown",
        parameters: &["source: string literal"],
        result: Some("styled-text"),
        documentation: "Parses Markdown into styled text at compile time.",
    },
    BuiltinFunction {
        name: "@linear-gradient",
        parameters: &["angle: degrees", "color stops..."],
        result: Some("brush"),
        documentation: "Creates a linear gradient. Specify the angle with `deg` and optionally give each color stop a percentage position.",
    },
    BuiltinFunction {
        name: "@radial-gradient",
        parameters: &["shape: circle", "color stops..."],
        result: Some("brush"),
        documentation: "Creates a circular gradient. Each color stop can have a percentage position.",
    },
];

pub(crate) const CHILDREN_DOC: &str = "Places the caller's child elements at this point in a component. Their bindings still use the caller's scope.";

pub(crate) fn row_key_methods(kind: &str) -> &'static [BuiltinFunction] {
    if xrack_ir::semantic::row_key_view(kind) {
        &[
            BuiltinFunction {
                name: "view-index-for-key",
                parameters: &["key: int"],
                result: Some("int"),
                documentation: "Returns the displayed index for a unique row key, or -1 if it is hidden or unavailable. Requires row-key-field.",
            },
            BuiltinFunction {
                name: "select-key",
                parameters: &["key: int"],
                result: None,
                documentation: "Selects a visible record by business key. Requires row-key-field.",
            },
            BuiltinFunction {
                name: "position-view-at-key",
                parameters: &["key: int", "alignment: ScrollAlignment"],
                result: None,
                documentation: "Positions a visible record by business key in the current model. Requires row-key-field.",
            },
        ]
    } else {
        &[]
    }
}

pub(crate) fn row_selection_methods(kind: &str) -> &'static [BuiltinFunction] {
    if !xrack_ir::semantic::row_selection_view(kind) {
        return &[];
    }
    &[
        BuiltinFunction {
            name: "is-row-selected",
            parameters: &["row: int"],
            result: Some("bool"),
            documentation: "Tests selection at a displayed row index, including rows outside the viewport.",
        },
        BuiltinFunction {
            name: "select-row",
            parameters: &["row: int"],
            result: None,
            documentation: "Replaces the selection with the record at this displayed index and moves the cursor.",
        },
        BuiltinFunction {
            name: "toggle-row",
            parameters: &["row: int"],
            result: None,
            documentation: "Toggles the record at this displayed index when multi-select is enabled.",
        },
        BuiltinFunction {
            name: "select-all",
            parameters: &[],
            result: None,
            documentation: "Selects the current filtered projection when multi-select is enabled.",
        },
        BuiltinFunction {
            name: "clear-selection",
            parameters: &[],
            result: None,
            documentation: "Clears the multi-selection and preserves the cursor.",
        },
    ]
}

pub(crate) fn batch_selection_methods(kind: &str) -> &'static [BuiltinFunction] {
    if !xrack_ir::semantic::row_key_view(kind) && kind != "TreeView" {
        return &[];
    }
    &[BuiltinFunction {
        name: "select-keys",
        parameters: &["keys: [int]"],
        result: None,
        documentation: "Atomically replaces the multi-selection by model key, including hidden rows, and preserves the current item. Lists, grids, and tables require row-key-field. Tables require row selection.",
    }]
}

pub(crate) fn tree_selection_methods(kind: &str) -> &'static [BuiltinFunction] {
    if kind != "TreeView" {
        return &[];
    }
    &[
        BuiltinFunction {
            name: "is-key-selected",
            parameters: &["key: int"],
            result: Some("bool"),
            documentation: "Checks whether the record with this key is selected, including retained selections in collapsed or filtered branches.",
        },
        BuiltinFunction {
            name: "select-key",
            parameters: &["key: int"],
            result: None,
            documentation: "Selects the displayed node with this key and replaces any existing multi-selection.",
        },
    ]
}

pub(crate) fn media_methods(kind: &str) -> &'static [BuiltinFunction] {
    match kind {
        "MediaPlayer" => MEDIA_METHODS,
        "SoundEffect" => &[BuiltinFunction {
            name: "play",
            parameters: &[],
            result: None,
            documentation: "Plays the WAV from the start. A call made during preload starts playback when the effect is ready.",
        }],
        _ => &[],
    }
}

const MEDIA_METHODS: &[BuiltinFunction] = &[
    BuiltinFunction {
        name: "go-live",
        parameters: &[],
        result: None,
        documentation: "Jumps to the current HLS live edge and starts playback.",
    },
    BuiltinFunction {
        name: "stop",
        parameters: &[],
        result: None,
        documentation: "Stops playback and rewinds seekable media.",
    },
    BuiltinFunction {
        name: "play",
        parameters: &[],
        result: None,
        documentation: "Starts or resumes playback. After the end, starts again from the beginning.",
    },
    BuiltinFunction {
        name: "pause",
        parameters: &[],
        result: None,
        documentation: "Pauses audio and video at the current playback position.",
    },
    BuiltinFunction {
        name: "seek",
        parameters: &["position: float"],
        result: None,
        documentation: "Seeks to a position in seconds and preserves whether playback was paused.",
    },
];
