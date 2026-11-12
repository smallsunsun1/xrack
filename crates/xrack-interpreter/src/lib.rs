//! Load optimized XR dialect IR and execute its UI on the XRack runtime.
mod action;
mod calendar;
mod evaluate;
mod focus;
mod gesture;
mod group;
mod host;
mod load;
mod native_dialog;
mod picker;
mod pointer;
mod range;
mod repeat;
mod secondary_window;
mod split;
mod stack;
mod table;
mod table_edit;
#[cfg(test)]
mod tests;
mod timer;
mod tree;
mod viewport;
mod visual_cache;

use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
use xrack_runtime::{
    Easing, ImageAsset, LoadedFonts, LoadedVectorPath, Node, RecordList, RecordRowValidator,
    RecordValue, TranslationCatalog, Value,
};

pub use load::load_mlir;
use xrack_mlir::{RowSceneCache, VisualCacheGroup, VisualCacheKey};
use xrack_resources::EmbeddedTranslations;
pub use xrack_runtime::Value as HostValue;

/// Load XR dialect IR and resolve its resources relative to the MLIR file.
pub fn load_mlir_file(path: impl AsRef<Path>) -> Result<Program> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)
        .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
    let (module, translations) = xrack_resources::prepare_mlir(&source, path)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Program::from_module_with_file_resources(&module, path, translations.catalog)
        .map(|(program, _)| program)
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Evaluation(String),
}

type Result<T> = std::result::Result<T, Error>;
type StaticCache<T> = Rc<OnceCell<Result<T>>>;

#[derive(Clone)]
pub struct Program {
    components: Rc<BTreeMap<String, Result<Definition>>>,
    exports: Rc<BTreeSet<String>>,
    records: Rc<RecordSchemas>,
    record_validator: Rc<RecordRowValidator>,
    translations: Option<Rc<TranslationCatalog>>,
    fonts: Option<LoadedFonts>,
}

impl Program {
    /// Copy the verified operation graph into definitions that own their data.
    pub fn from_module(module: &xrack_mlir::Module) -> Result<Self> {
        Self::from_module_with_translations(module, None)
    }

    pub fn from_module_with_translations(
        module: &xrack_mlir::Module,
        translations: Option<TranslationCatalog>,
    ) -> Result<Self> {
        Self::from_module_with_resources(module, translations, None)
    }

    pub fn from_module_with_resources(
        module: &xrack_mlir::Module,
        translations: Option<TranslationCatalog>,
        fonts: Option<LoadedFonts>,
    ) -> Result<Self> {
        Self::from_module_with_assets(module, translations, fonts, BTreeMap::new())
    }

    pub fn from_module_with_assets(
        module: &xrack_mlir::Module,
        translations: Option<TranslationCatalog>,
        fonts: Option<LoadedFonts>,
        images: BTreeMap<String, ImageAsset>,
    ) -> Result<Self> {
        let paths = xrack_resources::collect_vector_paths(module).map_err(Error::Invalid)?;
        load::program(module, translations, fonts, images, paths)
    }

    /// Load file-backed images and fonts for a prepared module, using the
    /// translation catalog validated before optimization. Returns asset paths
    /// for callers that watch their inputs for changes.
    pub fn from_module_with_file_resources(
        module: &xrack_mlir::Module,
        filename: impl AsRef<Path>,
        translations: Option<EmbeddedTranslations>,
    ) -> Result<(Self, BTreeSet<PathBuf>)> {
        let assets = xrack_resources::resolve_assets(module, filename.as_ref(), None)
            .map_err(Error::Invalid)?;
        xrack_resources::check_nine_slices(module, &assets.images).map_err(Error::Invalid)?;
        let translations = translations
            .map(|catalog| TranslationCatalog::from_owned(&catalog.default, catalog.locales))
            .transpose()
            .map_err(Error::Invalid)?;
        let fonts = (!assets.fonts.is_empty()).then(|| LoadedFonts::new(assets.fonts));
        let images = assets
            .images
            .into_iter()
            .map(|(url, image)| (url, image.into_image_asset()))
            .collect();
        let program = load::program(module, translations, fonts, images, assets.paths)?;
        Ok((program, assets.inputs))
    }

    pub fn exported_components(&self) -> impl Iterator<Item = &str> {
        self.exports.iter().map(String::as_str)
    }

    pub fn create(&self, name: &str) -> Result<Instance> {
        self.validate_dependencies(name, &mut BTreeSet::new())?;
        self.allocate(name, None, BTreeMap::new(), &mut Vec::new())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValueType {
    EventResult,
    FocusReason,
    DialogRole,
    StandardButtonKind,
    DrawerEdge,
    FileDialogMode,
    MessageLevel,
    MessageButtons,
    KeyEvent,
    WheelEvent,
    PointerEvent,
    PointerEventKind,
    PointerButton,
    PointerButtons,
    CursorShape,
    SwipeSide,
    DragAction,
    TouchPoint,
    TouchPoints,
    KeyModifiers,
    Bool,
    Int,
    Float,
    Length,
    Percent,
    Color,
    Brush,
    String,
    StringList,
    Record(Rc<str>),
    RecordList(Rc<str>),
    Date,
    Time,
    Font,
    CheckState,
    StyledText,
    LayoutAlignment,
    Orientation,
    ScrollSnapMode,
    ScrollAlignment,
    SelectionTarget,
    ScrollBarPolicy,
    FlickDirection,
    FlickBoundsBehavior,
    FlickBoundsMovement,
    PopupPlacement,
    FlexboxLayoutDirection,
    FlexboxLayoutWrap,
    CrossAxisAlignment,
    TextWrap,
    TextHorizontalAlignment,
    TextVerticalAlignment,
    TextOverflow,
    ColorScheme,
    InputType,
    DialInputMode,
    Image,
    ImageFit,
    ImageTileMode,
    ImageHorizontalAlignment,
    ImageVerticalAlignment,
    ImageRendering,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Member {
    owner: String,
    name: String,
}

impl Member {
    fn new(owner: &str, name: &str) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    fn root(name: &str) -> Self {
        Self::new("root", name)
    }
}

type Expr = Rc<Expression>;
type VisualSlot = VisualCacheKey<usize>;

#[derive(Clone)]
enum Expression {
    Literal(Value),
    CachedVisual(VisualSlot, Expr),
    Read(Member),
    Param(usize),
    RowParam(String),
    SplitHandleParam(split::Parameter),
    CalendarCellParam(calendar::Parameter),
    Unary(Unary, Expr),
    Binary(Binary, Expr, Expr),
    Select(Expr, Expr, Expr),
    Cast(Expr),
    Stringify(Expr),
    SolidBrush(Expr),
    ImageDimension(Expr, usize),
    ImagePlaybackRead(Member),
    GeometryRead(String, Option<xrack_runtime::GeometryMetric>),
    ViewportRead(String, viewport::Metric),
    ViewIndex(String, Expr, bool),
    TreeQuery(String, tree::Query, Expr),
    SplitSave(String),
    StackRead(String, stack::Read, Option<Expr>),
    SplitResizing(String),
    TableRead(Member),
    FocusRead(String, bool),
    HoverRead(String, pointer::HoverRead),
    GestureRead(String, gesture::Metric),
    PointerRead(String, pointer::Read),
    PointerButtons(Vec<Expr>),
    ActionGroupRead(String),
    NativeDialogRead(String, native_dialog::Read),
    WindowRead(xrack_runtime::WindowMetric),
    ScrollControlRead(Member),
    PopupOpen(String),
    NineSlice(Expr, xrack_runtime::NineSlice),
    Gradient {
        angle: Option<Expr>,
        stops: Vec<(Expr, Expr)>,
        cache: Option<StaticCache<Value>>,
    },
    StateSelect(usize),
    Translate {
        id: String,
        arguments: Vec<(String, Expr)>,
    },
    Array(Vec<Expr>),
    Record {
        name: Rc<str>,
        fields: Vec<(String, Expr)>,
    },
    RecordArray(Rc<str>, Vec<Expr>),
    Field(Expr, String),
    MakeDate([Expr; 3]),
    MakeTime([Expr; 3]),
    MakeFont([Expr; 4]),
    Call(Member, Vec<Expr>),
}

#[derive(Clone, Copy)]
enum Unary {
    Negate,
    Not,
}

#[derive(Clone, Copy, PartialEq)]
enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Clone)]
enum Statement {
    Assign(Member, Expr),
    Call(Member, Vec<Expr>),
    Scroll(String, viewport::Command, Vec<Expr>),
    SelectSource(String, Expr, Option<Expr>),
    Table(String, table::Command, Vec<Expr>),
    Focus(String, focus::Command, Vec<Expr>),
    Tree(String, tree::Command, Expr, Option<Expr>),
    SplitRestore(String, Expr),
    SwipeCancel(String),
    SetCheckGroup(String, Expr),
    StackNavigate(String, stack::Command, Option<Expr>),
    Timer(String, xrack_runtime::TimerCommand),
    Popup(String, bool),
    NativeDialogOpen(String),
    WindowClose,
    SecondaryWindow(String, secondary_window::Command),
    WindowCommand(xrack_runtime::WindowCommand),
    WindowFullScreen(Expr),
    Return(Expr),
}

#[derive(Clone)]
struct Function {
    parameters: Vec<ValueType>,
    result: Option<ValueType>,
    body: Vec<Statement>,
    pure: bool,
    alias: Option<Member>,
    alias_implementation: bool,
}

struct PropertyDef {
    ty: ValueType,
    initial: Expr,
    public: bool,
    writable: bool,
    alias: Option<Member>,
}

struct RecordField {
    name: String,
    ty: ValueType,
}

type RecordSchemas = BTreeMap<String, Vec<RecordField>>;

struct Definition {
    properties: BTreeMap<Member, PropertyDef>,
    callbacks: BTreeMap<Member, Function>,
    root: Element,
    selectors: Vec<Vec<Expr>>,
    geometry_targets: BTreeSet<String>,
    viewport_targets: BTreeSet<String>,
    source_scroll_targets: BTreeSet<String>,
    focus_targets: BTreeSet<String>,
    timer_order: Vec<String>,
    action_keys: BTreeSet<String>,
    native_dialog_keys: BTreeSet<String>,
    button_groups: BTreeMap<String, group::Members>,
}

struct Element {
    key: String,
    id: Arc<str>,
    repeat: Option<Repeat>,
    delegate: bool,
    record_combo: Option<RecordCombo>,
    standard_list: Option<StandardList>,
    table: Option<table::Definition>,
    tree: Option<tree::Definition>,
    split: Option<split::Definition>,
    stack_routes: Option<Box<[String]>>,
    day_cell: Option<Box<Element>>,
    column_field: Option<Rc<[String]>>,
    clipboard_parser: Option<table_edit::ClipboardParser>,
    clipboard_text: Option<Expr>,
    contains_delegate: bool,
    visible_field: Option<Rc<[String]>>,
    height_field: Option<Rc<[String]>>,
    instance_handlers: BTreeMap<String, Function>,
    visual: Visual,
    grid_plan: Option<Arc<[xrack_runtime::GridCellSlot]>>,
    paint_plan: Option<Arc<[usize]>>,
    static_subtree: Option<StaticCache<Node>>,
    static_hidden: bool,
    bindings: Vec<Binding>,
    controls: Box<[Control]>,
    offsets: Vec<(xrack_runtime::Orientation, viewport::Offset)>,
    scroll_target: Option<String>,
    initial_focus: Option<String>,
    action: Option<String>,
    action_group: Option<action::Membership>,
    button_group: Option<group::Membership>,
    non_exclusive: bool,
    state_bindings: BTreeMap<&'static str, Vec<StateCase>>,
    animations: Vec<(&'static str, AnimationSpec)>,
    state_timings: BTreeMap<&'static str, Vec<StateTimingSpec>>,
    conditions: Vec<Expr>,
    handlers: BTreeMap<String, Function>,
    links: BTreeMap<&'static str, Member>,
    row_links: BTreeMap<&'static str, RowField>,
    children: Vec<Element>,
    timers: Vec<Element>,
    commands: Vec<Element>,
    native_dialogs: Vec<Element>,
    secondary_windows: Vec<Element>,
}

struct Control {
    name: &'static str,
    default: xrack_resources::visual::ControlDefault<Value>,
    omit_default: bool,
}

struct StandardList {
    model: Option<Expr>,
    label: Vec<String>,
}

struct RecordCombo {
    model: Expr,
    label: Vec<String>,
    value: Vec<String>,
    enabled: Option<Vec<String>>,
}

struct Repeat {
    model: Expr,
    item: String,
    index: Option<String>,
    positioned: bool,
    empty: bool,
    static_count: Option<i64>,
    scene_cache: Option<RowSceneCache>,
}

enum Visual {
    Builtin(&'static str),
    Instance {
        component: String,
        inputs: BTreeMap<String, Input>,
    },
}

struct Binding {
    name: &'static str,
    expression: Expr,
    paint_only: bool,
    vector_path: Option<LoadedVectorPath>,
}

struct AnimationSpec {
    duration: Expr,
    enabled: Option<Expr>,
    delay_ms: u64,
    easing: Easing,
}

struct StateTimingSpec {
    selector: Expr,
    index: i64,
    enter: bool,
    animation: AnimationSpec,
}

#[derive(Clone)]
struct StateCase {
    selector: Expr,
    index: i64,
    value: Expr,
}

#[derive(Clone, Default)]
struct Input {
    source: Option<InputSource>,
    cases: Vec<StateCase>,
}

#[derive(Clone)]
enum InputSource {
    Binding(Expr),
    Linked(Member),
    RowLink(RowField),
}

#[derive(Clone)]
struct RowField {
    item: String,
    path: Vec<String>,
}

pub type Callback = Rc<dyn Fn(&[Value]) -> std::result::Result<Option<Value>, String>>;

#[derive(Clone)]
pub struct Instance(Rc<evaluate::State>);

impl Instance {
    pub fn properties(&self) -> impl Iterator<Item = (&str, ValueType, bool)> {
        self.definition()
            .properties
            .iter()
            .filter(|(_, property)| property.public)
            .map(|(member, property)| {
                (member.name.as_str(), property.ty.clone(), property.writable)
            })
    }

    pub fn callbacks(&self) -> impl Iterator<Item = (&str, &[ValueType], Option<ValueType>)> {
        self.definition()
            .callbacks
            .iter()
            .filter(|(member, _)| member.owner == "root")
            .map(|(member, function)| {
                (
                    member.name.as_str(),
                    function.parameters.as_slice(),
                    function.result.clone(),
                )
            })
    }

    pub fn get(&self, name: &str) -> Result<Value> {
        let member = Member::root(name);
        let property = self.require_public(&member, false)?;
        let value = self.read(&member)?;
        self.program().validate_host_value(
            &property.ty,
            &value,
            format_args!("property {name}"),
        )?;
        Ok(evaluate::host_value(value))
    }

    /// Get the shared string model without taking a snapshot or replacing row identities.
    pub fn string_model(&self, name: &str) -> Result<xrack_runtime::VecModel<String>> {
        let member = Member::root(name);
        self.require_public(&member, false)?;
        match self.read(&member)? {
            Value::StringModel(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a string list"
            ))),
        }
    }

    pub fn reset(&self, name: &str) -> Result<()> {
        let member = Member::root(name);
        self.require_public(&member, true)?;
        self.reset_member(&member)
    }

    pub fn set(&self, name: &str, value: Value) -> Result<()> {
        let member = Member::root(name);
        let property = self.require_public(&member, true)?;
        self.program().validate_host_value(
            &property.ty,
            &value,
            format_args!("property {name}"),
        )?;
        self.write(&member, value)
    }

    pub fn on(&self, name: &str, callback: Callback) -> Result<()> {
        self.register_callback(&Member::root(name), callback)
    }

    pub fn invoke(&self, name: &str, arguments: &[Value]) -> Result<Option<Value>> {
        let member = Member::root(name);
        let function = self.definition().callbacks.get(&member);
        if let Some(function) = function {
            self.program()
                .validate_host_arguments(name, &function.parameters, arguments)?;
        }
        let arguments: Vec<_> = arguments
            .iter()
            .cloned()
            .map(evaluate::model_value)
            .collect();
        let result = self.call(&member, &arguments)?;
        let result_type = function.and_then(|function| function.result.as_ref());
        self.program()
            .validate_host_result(name, result_type, result.as_ref())?;
        Ok(result.map(evaluate::host_value))
    }

    pub fn try_render(&self) -> Result<Rc<Node>> {
        self.render_scene()
    }

    /// Get an evaluation error raised from an event handled through Component.
    pub fn take_error(&self) -> Option<Error> {
        self.0.error.take()
    }
}
