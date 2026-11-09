// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action BusyIndicator Button Calendar CheckBox ColorPickerPopup ComboBox ContextMenuArea DatePickerPopup DelayButton Dial Dialog
    DialogButton DialogButtonBox DoubleSpinBox DragArea Drawer DropArea FileDialog FlexboxLayout FlowLayout GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    ListView Menu MenuBar MenuItem MenuSeparator MessageDialog Page PageFooter PageHeader PageIndicator Path PopupWindow ProgressIndicator RadioButton
    RadioGroup RangeSlider Rectangle Row ScrollView SearchField SecondaryWindow Shortcut Slider SpinBox Spinner SplitView StackPage StackView
    StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeLeft SwipePage SwipeRight SwipeView Switch Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    TextEdit TextInput TimePicker TimePickerPopup Timer ToolBar ToolButton Tooltip TouchArea TreeCell TreeRow TreeView VerticalBox
    Tumbler VerticalLayout Window";

pub(crate) const PROPERTIES: &str = "accent-color accept-files accept-text accessible-description accessible-name action alignment anchor-gap animated allow-copy allow-link allow-move auto-complete auto-repeat cancel-text
    background border-bottom-left-radius border-bottom-right-radius border-color border-radius buttons
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state checkable checked
    close-on-escape close-on-outside col color color-scheme colspan commands content-height content-padding content-width count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    current-page current-row current-value date decimal-separator decimals delay depth dim directory display-month dot-size data drag-key drag-margin drop-action edge edit-text editable
    default-font-family default-font-italic default-font-size default-font-weight enabled end-angle field file-extensions file-name fill fit flex-direction flex-wrap focus focus-within follow-current font-family font-italic font-size font-weight handle-color handle-size header-height height
    horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch icon icon-only icon-size
    image-fit indentation indeterminate informative-text initial-page input-mode input-type interactive interval key keys kind layout-order level live
    lower-value max-height max-lines max-width maximum maximum-date maximum-length min-height min-width minimum minimum-date modal mode model multi-select name-filters open open-side overflow placement
    orientation padding padding-bottom padding-left padding-right padding-top parent-key
    password-visible placeholder-text preferred-height preferred-width primary progress progress-color read-only resizable resizable-columns
    repeat role row row-height rowspan running selection-target sequence sequences shortcut sortable source spacing spacing-horizontal spacing-vertical start-angle step
    show-alpha show-seconds step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title track-color transition-duration upper-value use-24-hour-format value value-role
    vertical-alignment vertical-scroll-bar-policy vertical-spacing vertical-stretch viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels width wrap";

pub(crate) const EVENTS: &str = "accepted activated activated-ambiguously applied canceled changed check-state-changed clicked close-requested closed collapsed
    current-row-changed discarded drag-finished drag-started dropped edited entered exited expanded failed files-dropped files-entered finished help-requested month-changed opened position-changed rejected released reset resized search-triggered selected selection-changed
    sort-ascending sort-descending toggled triggered value-selected";

pub(crate) const TYPES: &str = "int float bool string length percent color brush image [string]
    ImageFit LayoutAlignment CrossAxisAlignment Orientation FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy TextWrap TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType
    CheckState ColorScheme Date DialogRole DragAction DrawerEdge FileDialogMode MessageButtons MessageLevel PopupPlacement SelectionTarget StandardButtonKind SwipeSide Time";

pub(crate) const FUNCTIONS: &str = "@image-url @linear-gradient @radial-gradient";
