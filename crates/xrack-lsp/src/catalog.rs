// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action AnimatedImage BusyIndicator Button ButtonGroup Calendar CheckBox ColorPickerPopup ComboBox ContextMenuArea DatePickerPopup DelayButton Dial Dialog
    DialogButton DialogButtonBox DoubleSpinBox DragArea Drawer DropArea FileDialog FlexboxLayout FlowLayout FocusScope GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    ListView Menu MenuBar MenuItem MenuSeparator MessageDialog Page PageFooter PageHeader PageIndicator Path PopupWindow ProgressIndicator RadioButton
    RadioGroup RangeSlider Rectangle Row ScrollView SearchField SecondaryWindow Shortcut Slider SpinBox Spinner SplitView StackPage StackView
    StyledText StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeLeft SwipePage SwipeRight SwipeView Switch Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    TextEdit TextInput TimePicker TimePickerPopup Timer ToolBar ToolButton Tooltip TouchArea TreeCell TreeRow TreeView VerticalBox
    Tumbler VerticalLayout Window WindowMoveArea";

pub(crate) const PROPERTIES: &str = "accent-color accept-files accept-text accessible-description accessible-name action alignment anchor-gap animated allow-copy allow-link allow-move auto-complete auto-repeat cancel-text
    background border-bottom-left-radius border-bottom-right-radius border-color border-radius buttons
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state checkable checked clip
    close-on-escape close-on-outside col color color-scheme colorize colspan commands content-height content-padding content-width content-x content-y copyable count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    current-frame current-page current-row current-value date decimal-separator decimals delay depth dim directory display-month dot-size data drag-key drag-margin drop-action drop-shadow-blur drop-shadow-color drop-shadow-offset-x drop-shadow-offset-y drop-shadow-spread edge edit-text editable
    default-font-family default-font-italic default-font-size default-font-weight enabled end-angle exclusive field file-extensions file-name fill fit flex-direction flex-wrap frame-count frozen-columns focus focus-on-click focus-on-tab-navigation focus-within follow-current group has-focus font-family font-italic font-size font-weight handle-color handle-size header-height height
    horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch horizontal-tile-mode icon icon-only icon-size
    image-fit image-rendering indentation indeterminate informative-text inner-shadow-blur inner-shadow-color inner-shadow-offset-x inner-shadow-offset-y inner-shadow-spread initial-focus initial-page input-mode input-type interactive interval key keys kind layout-height layout-order layout-ready layout-width layout-x layout-y level link-color live loops
    lower-value max-height max-lines max-width maximum maximum-date maximum-length min-height min-width minimum minimum-date modal mode model mouse-drag-pan-enabled movable-columns multi-select name-filters no-frame open open-side opacity overflow placement playing resize-border-width
    orientation padding padding-bottom padding-left padding-right padding-top parent-key
    context-menu-enabled password-visible pasteable paused placeholder-text preferred-height preferred-width preserve-scroll-anchor primary progress progress-color read-only resizable resizable-columns resizable-rows restore-focus
    repeat role row row-height row-height-field rowspan running selection-target sequence sequences shortcut sortable source source-clip-x source-clip-y source-clip-width source-clip-height speed spacing spacing-horizontal spacing-vertical start-angle step
    enabled-role show-alpha show-seconds step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title track-color transform-origin-x transform-origin-y transform-rotation transform-scale transform-scale-x transform-scale-y transition-duration upper-value use-24-hour-format value value-role
    vertical-alignment vertical-scroll-bar-policy vertical-spacing vertical-stretch vertical-tile-mode viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels width window-bounds-height window-bounds-width window-bounds-x window-bounds-y wrap z";

pub(crate) const EVENTS: &str = "accepted activated activated-ambiguously applied canceled capture-key-pressed capture-key-released changed check-state-changed clicked close-requested closed collapsed
    column-moved current-row-changed discarded drag-finished drag-started dropped edited entered exited expanded failed files-dropped files-entered finished focus-changed-event focus-gained focus-lost focus-within-changed help-requested key-pressed key-released link-activated month-changed opened position-changed rejected released reset resized search-triggered selected selection-changed
    scrolled sort-ascending sort-descending toggled triggered value-selected";

pub(crate) const TYPES: &str = "int float bool string styled-text length percent color brush image [string]
    ImageFit ImageTileMode ImageHorizontalAlignment ImageVerticalAlignment ImageRendering LayoutAlignment CrossAxisAlignment Orientation FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy ScrollAlignment TextWrap TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType
    CheckState ColorScheme Date DialogRole DragAction DrawerEdge EventResult FocusReason KeyEvent KeyModifiers FileDialogMode MessageButtons MessageLevel PopupPlacement SelectionTarget StandardButtonKind SwipeSide Time";

pub(crate) const FUNCTIONS: &str = "@image-url @markdown @linear-gradient @radial-gradient";
