// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action BusyIndicator Button Calendar CheckBox ComboBox ContextMenuArea DatePickerPopup DelayButton Dial Dialog
    DialogButton DialogButtonBox DoubleSpinBox Drawer FlexboxLayout FlowLayout GridLayout GridView GroupBox HorizontalBox HorizontalLayout Image Label LineEdit
    ListView Menu MenuBar MenuItem MenuSeparator Page PageFooter PageHeader PageIndicator Path PopupWindow ProgressIndicator RadioButton
    RadioGroup RangeSlider Rectangle Row ScrollView SearchField Slider SpinBox Spinner SplitView StackPage StackView
    StandardButton StandardListView StandardTableView SwipeContent SwipeDelegate SwipeLeft SwipePage SwipeRight SwipeView Switch Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    TextEdit TextInput TimePicker TimePickerPopup Timer ToolBar ToolButton Tooltip TouchArea TreeCell TreeRow TreeView VerticalBox
    Tumbler VerticalLayout Window";

pub(crate) const PROPERTIES: &str = "accept-text accessible-description accessible-name alignment anchor-gap animated auto-complete cancel-text
    background border-bottom-left-radius border-bottom-right-radius border-color border-radius
    border-top-left-radius border-top-right-radius border-width can-go-back cell-height cell-width check-state checkable checked
    close-on-escape close-on-outside col color colspan commands content-height content-padding content-width count
    cross-axis-alignment cross-axis-line-alignment cross-axis-self-alignment current-color current-index current-key
    current-page current-row current-value date decimal-separator decimals delay depth dim display-month dot-size drag-margin edge edit-text editable
    enabled end-angle field fill fit flex-direction flex-wrap focus follow-current font-size handle-color handle-size header-height height
    horizontal-alignment horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch icon icon-only icon-size
    image-fit indentation indeterminate initial-page input-mode input-type interactive interval key kind layout-order live
    lower-value max-height max-lines max-width maximum maximum-date maximum-length min-height min-width minimum minimum-date modal model multi-select open open-side overflow placement
    orientation padding padding-bottom padding-left padding-right padding-top parent-key
    password-visible placeholder-text preferred-height preferred-width primary progress progress-color read-only resizable resizable-columns
    repeat role row row-height rowspan running selection-target shortcut sortable source spacing spacing-horizontal spacing-vertical start-angle step
    show-seconds step-size stroke stroke-width suggestions suggestions-for text text-role time timeout title track-color transition-duration upper-value use-24-hour-format value
    vertical-alignment vertical-scroll-bar-policy vertical-spacing vertical-stretch viewbox-height viewbox-width
    triggered-on-start tristate viewbox-x viewbox-y viewport-height visible visible-item-count week-start weekday-labels width wrap";

pub(crate) const EVENTS: &str = "accepted activated applied canceled changed check-state-changed clicked closed collapsed
    current-row-changed discarded edited expanded help-requested month-changed opened rejected released reset resized search-triggered selected selection-changed
    sort-ascending sort-descending toggled triggered";

pub(crate) const TYPES: &str = "int float bool string length percent color brush image [string]
    ImageFit LayoutAlignment CrossAxisAlignment Orientation FlexboxLayoutDirection FlexboxLayoutWrap DialInputMode ScrollBarPolicy TextWrap TextHorizontalAlignment TextVerticalAlignment TextOverflow InputType
    CheckState Date DialogRole DrawerEdge PopupPlacement SelectionTarget StandardButtonKind SwipeSide Time";

pub(crate) const FUNCTIONS: &str = "@image-url @linear-gradient @radial-gradient";
