pub(crate) use xrack_parser::builtins::{
    BuiltinSignature as BuiltinFunction, POSITION_CHILD_METHOD, VIEWPORT_MOVE_METHODS,
    media_methods, navigation_move_methods, scene_view_methods, scroll_bar_methods,
    text_editor_methods,
};

pub(crate) fn viewport_move_methods(kind: &str) -> &'static [BuiltinFunction] {
    if xrack_ir::semantic::viewport_kind(kind) {
        VIEWPORT_MOVE_METHODS
    } else {
        &[]
    }
}

pub(crate) fn child_position_methods(kind: &str) -> &'static [BuiltinFunction] {
    if xrack_ir::semantic::content_viewport(kind) {
        &[POSITION_CHILD_METHOD]
    } else {
        &[]
    }
}

// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "DynamicContainer Action ActionGroup AnimatedImage ApplicationWindow MediaPlayer BusyIndicator Button ButtonGroup Calendar CheckBox CheckDelegate ColorPickerPopup CustomRender CustomItem SceneView3D MeshLoader3D ComboBox ComboContent ComboIndicator ComboOption ComboPopup ContextMenuArea DatePickerPopup DelayButton Dial Dialog ControlFrame Pane EmptyState
    ButtonContent SliderHandle RangeLowerHandle RangeUpperHandle ToggleIndicator SwitchThumb SpinUpIndicator SpinDownIndicator DayCell DayOfWeekRow DialogButton DialogButtonBox DoubleSpinBox DragArea Drawer DropArea Expander FileDialog FontPickerPopup Flickable FlexboxLayout FlowLayout FocusScope GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    HoverHandler ItemDelegate ListView Loader Menu MenuBar MenuItem MenuSeparator MessageDialog MonthGrid Page PageFooter PageHeader PageIndicator Path PathView PathViewOption PopupWindow ProgressFill ProgressIndicator ProgressTrack RadioButton RadioDelegate
    RadioGroup RangeSlider Rectangle RoundButton Row ScaleRotateGestureHandler ScrollBar ScrollIndicator ScrollTrack ScrollThumb ScrollView SearchField SecondaryWindow SectionHeader SelectionStartHandle SelectionEndHandle Shortcut Slider SpinBox Spinner SplitView StackItem StackLayout StackPage StackView
    StyledText StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeGestureHandler SwipeLeft SwipePage SwipeRight SwipeView Switch SwitchDelegate Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    SystemTrayIcon TextEdit TextInput TimePicker TimePickerPopup Timer FrameAnimation ToolBar ToolButton ToolSeparator Tooltip TouchArea TreeCell TreeDisclosure TreeRow TreeView VerticalBox
    Tumbler TumblerOption VerticalLayout WeekNumberColumn WheelHandler Window WindowMoveArea";

pub(crate) const PROPERTIES: &str = "auto-lod lightmap-uv asynchronous autoplay duration volume ready ended error muted playback-rate network-bandwidth audio-track subtitle-track subtitle-tracks subtitle-file status seekable seek-start seek-end at-live-edge has-audio has-video audio-tracks media-title subtitle audio-error subtitle-error buffer-progress bitrate accent-color accept-files accept-text acceptable-input accepted-buttons accessible-description accessible-heading-level accessible-live accessible-name action alignment anchor-gap animated allow-copy allow-link allow-move auto-complete auto-repeat auto-repeat-delay auto-repeat-interval auto-sort cancel-text blocking cursor-shape section-field section-height sticky-section-headers
    anchor-fill anchor-center-in anchor-left anchor-right anchor-top anchor-bottom anchor-horizontal-center anchor-vertical-center anchor-baseline anchor-margin baseline-offset
    handle-swipe-left handle-swipe-right handle-swipe-up handle-swipe-down bake-denoise-passes bake-indirect-factor
    background bounds-behavior bounds-movement border-bottom-left-radius border-bottom-right-radius border-color border-radius buttons cache-rendering-hint
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state colorize-icon checkable checked clip
    close-on-escape close-on-outside col color color-scheme colorize colspan commands content-height content-padding content-width content-x content-y copyable count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    implementation preload-model preload-ready lod-fade-width render-mode render-format render-overrides
    environment-sky
    environment-pitch environment-roll environment-horizon
    skybox-source skybox-sky skybox-yaw skybox-pitch skybox-roll skybox-intensity skybox-blur
    scene-vignette-strength scene-vignette-radius scene-vignette-color scene-sharpness scene-dithering
    fxaa fxaa-edge-threshold fxaa-min-threshold fxaa-subpixel fxaa-strength fxaa-search-steps
    lens-flare lens-flare-strength lens-flare-threshold lens-flare-ghost-count lens-flare-ghost-dispersal lens-flare-halo-width lens-flare-halo-strength lens-flare-distortion lens-flare-rotation lens-flare-aspect-corrected lens-flare-blur-levels lens-flare-color-map lens-flare-dirt-map lens-flare-starburst-map
    edit-on-click edit-on-selected-click edit-on-double-click edit-on-key edit-on-tab edit-on-typing
    current-frame current-page current-row current-text current-value date decimal-separator decimals delay depth dim directory camera auto-camera camera-yaw camera-pitch camera-distance camera-fit camera-fit-margin light-yaw light-elevation lights imported-lights ambient-color ambient-intensity ambient-occlusion ao-strength ao-distance ao-bias scene-background scene-grid scene-grid-axes scene-grid-color scene-grid-height scene-grid-spacing render-stats-revision texture-source texture-only texture-mipmaps texture-input multisampling supersampling progressive-aa temporal-aa temporal-aa-mode temporal-aa-strength screen-space-reflections ssr-strength ssr-distance ssr-thickness ssr-steps ssr-roughness-cutoff ssr-scale screen-space-global-illumination ssgi-strength ssgi-occlusion ssgi-radius ssgi-thickness ssgi-scale ssgi-rays ssgi-steps texture-width texture-height effective-texture-width effective-texture-height render-scale specular-aa order-independent-transparency fog-color fog-near fog-far fog-density particle-emitters particle-instance-models particle-affectors particle-time fog-volumes fog-volume-scattering fog-volume-anisotropy fog-volume-samples fog-volume-density-samples fog-depth-enabled fog-height-enabled fog-transmit-enabled fog-depth-curve fog-height-start fog-height-end fog-height-curve fog-transmit-curve frustum-culling shadows shadow-map-size shadow-map-precision material-debug oit-method scissor-rect depth-prepass debug-wireframe shadow-debug-cascades shadow-debug-light-volumes shadow-debug-casters shadow-debug-receivers shadow-debug-intersections shadow-freeze-camera shadow-bias shadow-cascades shadow-distance shadow-cascade-blend shadow-split-balance environment environment-intensity environment-yaw environment-skybox reflection-probes reflection-faces-per-frame reflection-filter-passes-per-frame reflection-filter-texels-per-frame layer-mask cache-rendering scene-exposure scene-tone-map scene-white-point scene-brightness scene-contrast scene-saturation scene-effect scene-effects scene-captures scene-color-capture scene-depth-capture scene-effect-strength scene-effect-depth scene-effect-history-revision scene-effect-animate scene-lut scene-lut-size scene-lut-strength scene-hdr bloom bloom-strength bloom-threshold bloom-levels depth-of-field dof-blur-radius dof-focus-distance dof-focus-range selected-object physics-world physics-debug physics-debug-body display-month dot-size data renderer revision drag-auto-scroll drag-key drag-margin touch-drag-delay drag-select selection-gesture selection-hold-interval selection-active selection-dragging drop-action drop-shadow-blur drop-shadow-color drop-shadow-offset-x drop-shadow-offset-y drop-shadow-spread edge edit-text editable
    default-font default-font-family default-font-italic default-font-size default-font-weight display enabled end-angle exclusive field flick-deceleration flick-direction dragging flicking moving rebounding horizontal-overshoot vertical-overshoot file-extensions file-name fill filter-field filter-text fit flat flex-direction flex-wrap frame-count frozen-columns focus focus-on-click focus-on-tab-navigation focus-within follow-current font preview-text group has-focus font-family font-italic font-size font-weight handle-color handle-hit-size handle-size header-height height
    highlighted highlighted-index horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch horizontal-tile-mode icon icon-only icon-size icon-template tooltip
    image-fit image-rendering indentation indeterminate informative-text inner-shadow-blur inner-shadow-color inner-shadow-offset-x inner-shadow-offset-y inner-shadow-spread initial-focus initial-page input-mode input-mask input-pattern input-purpose input-type return-key keyboard-navigation keyboard-capitalization auto-correct spell-check interactive interval key keys kind layout-height layout-direction layout-order layout-ready layout-width layout-x layout-y level link-color live loops loops-remaining mouse-selection-mode input-validity overwrite-mode persistent-selection selectable selection-color selected-text-color
    lower-value max-height max-lines max-width maximum maximum-date maximum-length maximum-value minimum-length minimum-value submit-when min-height min-width minimum minimum-date minimum-time maximum-time minimum-size modal mode model mouse-drag-pan-enabled movable-columns movable-rows multi-select multi-touch name-filters no-frame open open-side opacity overflow placement playing popup-open resize-border-width
    orientation path item-width item-height padding padding-bottom padding-left padding-right padding-top parent-key policy position visual-position visual-size size active pressed hovered
    context-menu-enabled password-character password-mask-delay password-visible pasteable undoable paused placeholder-text placeholder-text-color preferred-height preferred-width preserve-scroll-anchor pull-to-refresh refreshing press-and-hold-interval primary progress progress-color read-only rebound-duration resizable resizable-columns resizable-rows restore-focus reorderable tabs-closable closable
    radius repeat role root-key row row-height row-height-field row-header-width row-header-text row-header-field row-key-field row-visible-field rowspan running scroll-indicator selected-view-index active-index selection-target sequence sequences shortcut sortable source source-url source-clip-x source-clip-y source-clip-width source-clip-height speed spacing spacing-horizontal spacing-vertical start-angle step
    enabled-role show-alpha show-color-plane show-seconds snap-mode stack-index step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title today track-color transform transform-origin-x transform-origin-y transform-rotation transform-scale transform-scale-x transform-scale-y transition-duration upper-value use-24-hour-format value value-role
    typeahead-enabled typeahead-field vertical-alignment vertical-scroll-bar-policy auto-reveal-focus reserve-scroll-bar-space effective-scroll-bar-width effective-scroll-bar-height vertical-spacing vertical-stretch vertical-tile-mode viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels disabled-dates disabled-weekdays dismiss-keyboard-on-drag fixed-month highlight-start highlight-end wheel-enabled width window-bounds-height window-bounds-width window-bounds-x window-bounds-y wrap z expanded";

pub(crate) const EVENTS: &str = "mounted loaded accepted activated activated-ambiguously applied canceled cancelled capture-key-pressed capture-key-released changed check-state-changed clicked close-requested closed collapsed deactivated wheel pointer-event
    column-moved current-cell-changed current-row-changed discarded drag-finished drag-started dropped edited editing-finished entered exited expanded failed files-dropped files-entered files-failed finished focus-changed-event focus-gained focus-lost focus-within-changed help-requested key-pressed key-released leave-requested link-activated month-changed opened position-changed pressed rejected released refresh-requested removed reset resized row-moved search-triggered selected selection-changed
    double-clicked ended flicked moved press-and-hold scroll-event scrolled sort-ascending sort-descending started swiped tab-close-requested tab-moved page-moved toggled triggered updated value-selected expanded-changed";

pub(crate) const TYPES: &str = "int float bool string styled-text length point2d point3d vector2d vector3d quaternion3d rotationtrack3d hit3d [hit3d] instance3d instancetable3d texturedata2d texturedata3d meshasset3d meshnode3d bounds3d projectedpoint3d walkresult3d physicsworld3d scenemodel3d physicscontact3d [physicscontact3d] physicshit3d [physicshit3d] transform2d percent color brush image [string] WheelEvent CursorShape PointerEvent PointerEventKind PointerButton PointerButtons TouchPoint TouchPoints [TouchPoint]
    ImageFit ImageTileMode ImageHorizontalAlignment ImageVerticalAlignment ImageRendering ButtonDisplay LayoutAlignment CrossAxisAlignment Orientation FlickDirection FlickBoundsBehavior FlickBoundsMovement LayoutDirection FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy ScrollSnapMode ScrollAlignment TextWrap MouseSelectionMode InputValidity TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType InputPurpose KeyboardCapitalization ReturnKey AccessibleLive
    CheckState ColorScheme SceneToneMap Date DialogRole DragAction DrawerEdge EventResult FocusReason Font KeyEvent KeyModifiers FileDialogMode LogLevel MessageButtons MessageLevel PopupPlacement SelectionTarget SelectionGesture StandardButtonKind SwipeSide Time";

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
        name: "@mesh-url",
        parameters: &[
            "path: string literal",
            "auto-lod (optional)",
            "lightmap-uv (optional)",
        ],
        result: Some("string"),
        documentation: "Loads glTF with materials and animations, imports a Wavefront OBJ with MTL materials, or builds terrain from an 8-bit or 16-bit grayscale PNG. Paths are relative to the source file. Returns a SceneView3D mesh key. The auto-lod option generates simpler variants within a mesh error budget, including sampled skinning and morph positions. The lightmap-uv option unwraps static geometry into an independent UV channel before optional LOD generation.",
    },
    BuiltinFunction {
        name: "@texture-url",
        parameters: &["path: string literal"],
        result: Some("string"),
        documentation: "Prepares a PNG or JPEG for a 3D material or post-effect input, or a KTX/KTX2 cubemap for a shader input. The path is relative to the source file. Materials and effects can share the uploaded texture.",
    },
    BuiltinFunction {
        name: "@shader-url",
        parameters: &["path: string literal"],
        result: Some("string"),
        documentation: "Validates an XRack WGSL post-effect relative to the source file for SceneView3D.scene-effect or a pass in scene-effects. Define scene_effect(uv: vec2<f32>) -> vec4<f32>. Built-in helpers expose color, named buffers, depth, texture sizes and parameters. scene_buffer_color_lod(index, uv, lod) samples a 2D mip level. scene_buffer_cubemap_lod(index, direction, lod) samples a KTX cubemap in a 3D direction. scene_buffer_volume(index, uvw) samples a TextureData3D buffer in normalized coordinates. scene_buffer_color_grad(index, uv, dx, dy) selects the level from UV gradients and supports max-anisotropy from the buffer record. scene_uv_dx() and scene_uv_dy() provide the fullscreen gradients. A capture buffer can read a named scene-captures target with attachment color, depth, normal-roughness, base-color-metalness or emissive-occlusion. Use scene_surface(uv) for the viewport surface and scene_buffer_world_position(index, uv) for a named depth attachment and its capture camera.",
    },
    BuiltinFunction {
        name: "@material-shader-url",
        parameters: &["path: string literal"],
        result: Some("string"),
        documentation: "Validates a WGSL material shader relative to the source file. Use the returned key in a model material or a scene-captures override-material. For scene-captures augment-shader, define scene_capture_augment(input: XrMaterialInput, surface: XrMaterialSurface, index: u32) -> vec4<f32> in a separate WGSL file. capture_augment_parameters() reads the capture record's augment-parameters vector. capture_augment_texture(index, uv), capture_augment_texture_lod(index, uv, lod) and capture_augment_texture_world_position(index, uv) read its augment-textures. Material inputs can use material_texture_lod(index, uv, lod) for 2D textures and material_cubemap_lod(index, direction, lod) for KTX cubemaps. Use material_volume(index, uvw) for TextureData3D. The material_texture_grad and capture_augment_texture_grad helpers accept explicit UV gradients and respect max-anisotropy. UV gradients are available as input.uv0_dx, input.uv0_dy, input.uv1_dx and input.uv1_dy. Optional hooks are scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface, scene_vertex(input: XrVertexInput) -> vec3<f32>, scene_light(input: XrLightInput, response: XrLightResponse) -> XrLightResponse and scene_ambient(input: XrAmbientInput, diffuse: vec3<f32>) -> vec3<f32>. Lighting hooks apply to lit materials. A vertex program uses a positive shader-displacement bound. Both inputs expose instance_data, instance_index and instance_color. XrVertexInput.instance_transform is the instance world matrix. In a fragment hook, material_texture_world_position(index, uv) reconstructs position from a named depth attachment.",
    },
    BuiltinFunction {
        name: "@instance-table-url",
        parameters: &["path: string literal"],
        result: Some("instancetable3d"),
        documentation: "Loads a Qt Quick 3D XML InstanceTable or QtIR .bin file relative to the source. Negative scale can mirror an instance. Bind the result to a model row's instance-table field. The value exposes loaded and count, and the row may limit rendering with instance-count-override.",
    },
    BuiltinFunction {
        name: "@particle-shape-url",
        parameters: &["path: string literal"],
        result: Some("string"),
        documentation: "Loads a QQ3D_SHAPE version 1 CBOR point cloud relative to the source. Bind it to emitter-point-source or attractor-point-source with shape set to points.",
    },
    BuiltinFunction {
        name: "@environment-url",
        parameters: &["path: string literal"],
        result: Some("string"),
        documentation: "Loads a Radiance .hdr environment or a .ktx/.ktx2 color cubemap relative to the source. Bind the filtered resource to environment, skybox-source, or a reflection probe.",
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

pub(crate) fn dynamic_methods(kind: &str) -> &'static [BuiltinFunction] {
    if kind == "DynamicContainer" {
        xrack_parser::builtins::DYNAMIC_CONTAINER_METHODS
    } else {
        &[]
    }
}

pub(crate) fn focus_methods(kind: &str) -> &'static [BuiltinFunction] {
    if !xrack_ir::semantic::focus_property(kind, "has-focus") {
        return &[];
    }
    const METHODS: &[BuiltinFunction] = &[
        BuiltinFunction {
            name: "focus",
            parameters: &[],
            result: None,
            documentation: "Requests keyboard focus. RadioGroup targets its selected available option, or its first available option.",
        },
        BuiltinFunction {
            name: "clear-focus",
            parameters: &[],
            result: None,
            documentation: "Clears keyboard focus held by the receiver. On a RadioGroup, clears focus held by any of its options.",
        },
        BuiltinFunction {
            name: "open-popup",
            parameters: &[],
            result: None,
            documentation: "Requests the window to open this ComboBox popup and focus the control.",
        },
        BuiltinFunction {
            name: "close-popup",
            parameters: &[],
            result: None,
            documentation: "Requests the window to dismiss this ComboBox popup.",
        },
    ];
    if kind == "ComboBox" {
        METHODS
    } else {
        &METHODS[..2]
    }
}

pub(crate) fn timer_methods(kind: &str) -> &'static [BuiltinFunction] {
    if !matches!(kind, "Timer" | "FrameAnimation") {
        return &[];
    }
    &[
        BuiltinFunction {
            name: "start",
            parameters: &[],
            result: None,
            documentation: "Starts the clock if it is stopped.",
        },
        BuiltinFunction {
            name: "stop",
            parameters: &[],
            result: None,
            documentation: "Stops the clock.",
        },
        BuiltinFunction {
            name: "restart",
            parameters: &[],
            result: None,
            documentation: "Restarts the clock from its initial state.",
        },
    ]
}

pub(crate) fn button_methods(kind: &str) -> &'static [BuiltinFunction] {
    if !xrack_parser::builtins::push_button(kind) {
        return &[];
    }
    &[
        BuiltinFunction {
            name: "click",
            parameters: &[],
            result: None,
            documentation: "Synchronously presses, releases, and activates a mounted, enabled button.",
        },
        BuiltinFunction {
            name: "animate-click",
            parameters: &[],
            result: None,
            documentation: "Presses the button and schedules activation after 100 ms. Calling again restarts the release timer.",
        },
        BuiltinFunction {
            name: "toggle",
            parameters: &[],
            result: None,
            documentation: "Changes checked state without activating the command. A repeated button uses the state of its current model row.",
        },
    ]
}
