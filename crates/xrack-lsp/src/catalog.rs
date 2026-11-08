// Enumerate candidate spellings. The compiler decides whether each is available in context.
pub(crate) const ELEMENTS: &str = "Action Button Calendar CheckBox ComboBox ContextMenuArea Dial Dialog
    DialogButton DoubleSpinBox Drawer FlowLayout GridLayout HorizontalBox HorizontalLayout Image LineEdit
    ListView Menu MenuBar MenuItem MenuSeparator PageIndicator Path ProgressIndicator RadioButton
    RadioGroup RangeSlider Rectangle Row ScrollView Slider SpinBox SplitView StackPage StackView
    SwipePage SwipeView Switch Tab TabBar TabButton TabWidget TableCell TableColumn TableRow TableView Text TextArea
    TextEdit TextInput ToolBar ToolButton Tooltip TouchArea TreeCell TreeRow TreeView VerticalBox
    VerticalLayout Window";

pub(crate) const PROPERTIES: &str = "accessible-description accessible-name alignment auto-complete
    background border-bottom-left-radius border-bottom-right-radius border-color border-radius
    border-top-left-radius border-top-right-radius border-width can-go-back checkable checked
    close-on-escape close-on-outside col color colspan commands content-height content-width count
    cross-axis-alignment cross-axis-self-alignment current-color current-index current-key
    current-page current-row current-value date decimal-separator decimals delay depth display-month dot-size drag-margin edge edit-text editable
    enabled end-angle fill fit font-size handle-color handle-size header-height height
    horizontal-scroll-bar-policy horizontal-spacing horizontal-stretch icon icon-only icon-size
    image-fit indentation indeterminate initial-page input-mode interactive key layout-order
    lower-value max-height max-width maximum maximum-date min-height min-width minimum minimum-date model multi-select open
    orientation padding padding-bottom padding-left padding-right padding-top parent-key
    placeholder-text preferred-height preferred-width progress read-only resizable resizable-columns
    role row row-height rowspan selection-target shortcut sortable source spacing start-angle step
    step-size stroke stroke-width text timeout title transition-duration upper-value value
    vertical-scroll-bar-policy vertical-spacing vertical-stretch viewbox-height viewbox-width
    viewbox-x viewbox-y viewport-height visible week-start weekday-labels width wrap";

pub(crate) const EVENTS: &str = "accepted activated changed clicked closed collapsed
    current-row-changed edited expanded month-changed opened rejected released resized selected selection-changed
    sort-ascending sort-descending toggled";

pub(crate) const TYPES: &str = "int float bool string length percent color brush image [string]
    ImageFit LayoutAlignment CrossAxisAlignment Orientation DialInputMode ScrollBarPolicy TextWrap
    Date DialogRole DrawerEdge SelectionTarget";

pub(crate) const FUNCTIONS: &str = "@image-url @linear-gradient @radial-gradient";
