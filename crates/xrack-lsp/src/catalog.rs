// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action ActionGroup AnimatedImage ApplicationWindow BusyIndicator Button ButtonGroup Calendar CheckBox CheckDelegate ColorPickerPopup ComboBox ContextMenuArea DatePickerPopup DelayButton Dial Dialog ControlFrame Pane
    DayCell DayOfWeekRow DialogButton DialogButtonBox DoubleSpinBox DragArea Drawer DropArea FileDialog FontPickerPopup Flickable FlexboxLayout FlowLayout FocusScope GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    HoverHandler ItemDelegate ListView Menu MenuBar MenuItem MenuSeparator MessageDialog MonthGrid Page PageFooter PageHeader PageIndicator Path PopupWindow ProgressIndicator RadioButton RadioDelegate
    RadioGroup RangeSlider Rectangle RoundButton Row ScaleRotateGestureHandler ScrollBar ScrollIndicator ScrollView SearchField SecondaryWindow Shortcut Slider SpinBox Spinner SplitView StackPage StackView
    StyledText StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeGestureHandler SwipeLeft SwipePage SwipeRight SwipeView Switch SwitchDelegate Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    SystemTrayIcon TextEdit TextInput TimePicker TimePickerPopup Timer ToolBar ToolButton ToolSeparator Tooltip TouchArea TreeCell TreeRow TreeView VerticalBox
    Tumbler VerticalLayout WeekNumberColumn WheelHandler Window WindowMoveArea";

pub(crate) const PROPERTIES: &str = "accent-color accept-files accept-text accepted-buttons accessible-description accessible-name action alignment anchor-gap animated allow-copy allow-link allow-move auto-complete auto-repeat auto-sort cancel-text blocking cursor-shape
    handle-swipe-left handle-swipe-right handle-swipe-up handle-swipe-down
    background bounds-behavior bounds-movement border-bottom-left-radius border-bottom-right-radius border-color border-radius buttons
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state checkable checked clip
    close-on-escape close-on-outside col color color-scheme colorize colspan commands content-height content-padding content-width content-x content-y copyable count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    edit-on-click edit-on-selected-click edit-on-double-click edit-on-key edit-on-tab edit-on-typing
    current-frame current-page current-row current-value date decimal-separator decimals delay depth dim directory display-month dot-size data drag-key drag-margin drag-select drop-action drop-shadow-blur drop-shadow-color drop-shadow-offset-x drop-shadow-offset-y drop-shadow-spread edge edit-text editable
    default-font default-font-family default-font-italic default-font-size default-font-weight enabled end-angle exclusive field flick-direction file-extensions file-name fill filter-field filter-text fit flex-direction flex-wrap frame-count frozen-columns focus focus-on-click focus-on-tab-navigation focus-within follow-current font preview-text group has-focus font-family font-italic font-size font-weight handle-color handle-hit-size handle-size header-height height
    highlighted horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch horizontal-tile-mode icon icon-only icon-size icon-template tooltip
    image-fit image-rendering indentation indeterminate informative-text inner-shadow-blur inner-shadow-color inner-shadow-offset-x inner-shadow-offset-y inner-shadow-spread initial-focus initial-page input-mode input-type interactive interval key keys kind layout-height layout-order layout-ready layout-width layout-x layout-y level link-color live loops
    lower-value max-height max-lines max-width maximum maximum-date maximum-length min-height min-width minimum minimum-date minimum-size modal mode model mouse-drag-pan-enabled movable-columns movable-rows multi-select multi-touch name-filters no-frame open open-side opacity overflow placement playing resize-border-width
    orientation padding padding-bottom padding-left padding-right padding-top parent-key policy position size
    context-menu-enabled password-visible pasteable undoable paused placeholder-text preferred-height preferred-width preserve-scroll-anchor press-and-hold-interval primary progress progress-color read-only rebound-duration resizable resizable-columns resizable-rows restore-focus
    radius repeat role root-key row row-height row-height-field row-header-width row-header-text row-header-field row-visible-field rowspan running selection-target sequence sequences shortcut sortable source source-clip-x source-clip-y source-clip-width source-clip-height speed spacing spacing-horizontal spacing-vertical start-angle step
    enabled-role show-alpha show-color-plane show-seconds snap-mode stack-index step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title today track-color transform-origin-x transform-origin-y transform-rotation transform-scale transform-scale-x transform-scale-y transition-duration upper-value use-24-hour-format value value-role
    vertical-alignment vertical-scroll-bar-policy vertical-spacing vertical-stretch vertical-tile-mode viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels wheel-enabled width window-bounds-height window-bounds-width window-bounds-x window-bounds-y wrap z";

pub(crate) const EVENTS: &str = "accepted activated activated-ambiguously applied canceled cancelled capture-key-pressed capture-key-released changed check-state-changed clicked close-requested closed collapsed deactivated wheel pointer-event
    column-moved current-cell-changed current-row-changed discarded drag-finished drag-started dropped edited entered exited expanded failed files-dropped files-entered finished focus-changed-event focus-gained focus-lost focus-within-changed help-requested key-pressed key-released leave-requested link-activated month-changed opened position-changed rejected released removed reset resized row-moved search-triggered selected selection-changed
    double-clicked ended flicked moved press-and-hold scroll-event scrolled sort-ascending sort-descending started swiped toggled triggered updated value-selected";

pub(crate) const TYPES: &str = "int float bool string styled-text length percent color brush image [string] WheelEvent CursorShape PointerEvent PointerEventKind PointerButton PointerButtons TouchPoint TouchPoints [TouchPoint]
    ImageFit ImageTileMode ImageHorizontalAlignment ImageVerticalAlignment ImageRendering LayoutAlignment CrossAxisAlignment Orientation FlickDirection FlickBoundsBehavior FlickBoundsMovement FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy ScrollSnapMode ScrollAlignment TextWrap TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType
    CheckState ColorScheme Date DialogRole DragAction DrawerEdge EventResult FocusReason Font KeyEvent KeyModifiers FileDialogMode MessageButtons MessageLevel PopupPlacement SelectionTarget StandardButtonKind SwipeSide Time";

pub(crate) const FUNCTIONS: &str = "@image-url @markdown @linear-gradient @radial-gradient";
