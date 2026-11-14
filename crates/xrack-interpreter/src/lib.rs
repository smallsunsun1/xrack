//! Load optimized XR dialect IR and execute its UI on the XRack runtime.
mod action;
mod calendar;
mod evaluate;
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
    Animation, Component, ImageAsset, LoadedFonts, LoadedVectorPath, LocaleContext, Node,
    RecordList, RecordRowValidator, RecordValue, TouchPoints, TranslationCatalog, Value,
};

pub use load::load_mlir;
use xrack_mlir::{BinaryOperator, RowSceneCache, UnaryOperator, VisualCacheGroup, VisualCacheKey};
use xrack_resources::{EmbeddedTranslations, field::BuiltinField};
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
    globals: Rc<BTreeMap<String, bool>>,
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
        module.verify().map_err(Error::Invalid)?;
        xrack_resources::validate_module_resources(
            module,
            xrack_resources::ResourceImages::Loaded(&images),
            fonts
                .as_ref()
                .map_or(&[][..], LoadedFonts::sources)
                .iter()
                .map(|font| font.as_slice()),
            translations.as_ref().map(TranslationCatalog::sources),
            true,
        )
        .map_err(Error::Invalid)?;
        let paths = xrack_resources::collect_vector_paths(module).map_err(Error::Invalid)?;
        load::program(module, translations, fonts, images, paths, "")
    }

    /// Load file-backed images and fonts for a prepared module, using the
    /// translation catalog validated before optimization. Returns asset paths
    /// for callers that watch their inputs for changes.
    pub fn from_module_with_file_resources(
        module: &xrack_mlir::Module,
        filename: impl AsRef<Path>,
        translations: Option<EmbeddedTranslations>,
    ) -> Result<(Self, BTreeSet<PathBuf>)> {
        module.verify().map_err(Error::Invalid)?;
        let assets = xrack_resources::resolve_assets(module, filename.as_ref(), None)
            .map_err(Error::Invalid)?;
        xrack_resources::validate_module_resources(
            module,
            xrack_resources::ResourceImages::Embedded(&assets.images),
            assets.fonts.iter().map(Vec::as_slice),
            translations.as_ref().map(EmbeddedTranslations::sources),
            false,
        )
        .map_err(Error::Invalid)?;
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
        let program = load::program(
            module,
            translations,
            fonts,
            images,
            assets.paths,
            &filename.as_ref().to_string_lossy(),
        )?;
        Ok((program, assets.inputs))
    }

    pub fn exported_components(&self) -> impl Iterator<Item = &str> {
        self.exports.iter().map(String::as_str)
    }

    pub fn create(&self, name: &str) -> Result<Instance> {
        if self.globals.contains_key(name) {
            return Err(Error::Invalid(
                "global belongs to a component instance".into(),
            ));
        }
        self.validate_dependencies(name, &mut BTreeSet::new())?;
        self.allocate(
            name,
            None,
            BTreeMap::new(),
            BTreeMap::new(),
            &mut Vec::new(),
        )
    }

    /// Create a component whose translations share the supplied locale context.
    pub fn create_with_locale(&self, name: &str, context: LocaleContext) -> Result<Instance> {
        let instance = self.create(name)?;
        instance.set_locale_context(context);
        Ok(instance)
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
    LogLevel,
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

    fn path(endpoints: Vec<xrack_mlir::PropertyEndpoint<'_>>) -> Vec<Self> {
        endpoints
            .into_iter()
            .map(|(owner, name)| Self::new(owner, name))
            .collect()
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
    Unary(UnaryOperator, Expr),
    Binary(BinaryOperator, Expr, Expr),
    Select(Expr, Expr, Expr),
    Cast(Expr),
    Round(Expr),
    MathFunction(xrack_syntax::builtins::MathFunction, Vec<Expr>),
    StringFunction(xrack_syntax::builtins::StringFunction, Vec<Expr>),
    ModelCount(Expr),
    ModelIsEmpty(Expr),
    ModelAt(Expr, Expr, Expr),
    ColorFunction(xrack_syntax::builtins::ColorFunction, Vec<Expr>),
    Locale,
    FormatNumber(Expr, Expr),
    ParseNumber(Expr, Expr),
    FormatFixed(Expr, usize),
    Stringify(Expr),
    SolidBrush(Expr),
    ImageDimension(Expr, usize),
    ImagePlaybackRead(Member),
    MediaRead(Member),
    GeometryRead(String, Option<xrack_runtime::GeometryMetric>),
    ViewportRead(String, viewport::Metric),
    ViewIndex(String, Expr, xrack_runtime::ViewIndex),
    TreeQuery(String, tree::Query, Expr),
    SplitSave(String),
    StackRead(String, xrack_mlir::StackRead),
    SplitResizing(String),
    TableRead(String, xrack_mlir::TableRead),
    FocusRead(String, bool),
    HoverRead(String, pointer::HoverRead),
    GestureRead(String, gesture::Metric),
    PointerRead(String, pointer::Read),
    PointerButtons(Vec<Expr>),
    ActionGroupRead(String),
    NativeDialogRead(String, native_dialog::Read),
    WindowRead(xrack_runtime::WindowMetric),
    ScrollControlRead(Member),
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
    RecordField(Expr, String),
    BuiltinField(Expr, BuiltinField),
    MakeDate([Expr; 3]),
    DateIsValid(Expr),
    DateWeekday(Expr),
    DateAddDays(Expr, Expr, Expr),
    DateAddMonths(Expr, Expr, Expr),
    MakeTime([Expr; 3]),
    MakeFont([Expr; 4]),
    Call(Member, Vec<Expr>),
}

#[derive(Clone)]
enum Statement {
    Assign(Member, Expr),
    Call(Member, Vec<Expr>),
    Log {
        level: Expr,
        message: Expr,
        file: String,
        span: xrack_syntax::ast::Span,
    },
    Scroll(String, xrack_runtime::ScrollCommandMethod, Vec<Expr>),
    SelectSource(String, Expr, Option<Expr>),
    Table(String, table::Command, Vec<Expr>),
    Focus(String, xrack_runtime::FocusCommandMethod, Option<[Expr; 2]>),
    Tree(String, xrack_runtime::TreeCommandMethod, Expr, Option<Expr>),
    SplitRestore(String, Expr),
    SwipeCancel(String),
    SetCheckGroup(String, Expr),
    StackNavigate(String, xrack_runtime::StackNavigationMethod, Option<Expr>),
    Timer(String, xrack_runtime::TimerCommand),
    Media(String, xrack_resources::visual::MediaEffect<Expr>),
    Popup(String, bool),
    NativeDialogOpen(String),
    WindowClose,
    SecondaryWindow(String, xrack_runtime::SecondaryWindowCommand),
    WindowCommand(xrack_runtime::WindowCommandMethod, Option<Expr>),
    Return(Expr),
}

#[derive(Clone)]
struct Function {
    parameters: Vec<ValueType>,
    result: Option<ValueType>,
    body: Vec<Statement>,
    pure: bool,
    alias: Option<Member>,
}

struct PropertyDef {
    ty: ValueType,
    source: PropertySource,
    public: bool,
    writable: bool,
}

enum PropertySource {
    Initial(Expr),
    Alias(Member),
}

struct RecordField {
    name: String,
    ty: ValueType,
}

type RecordSchemas = BTreeMap<String, Vec<RecordField>>;

struct Definition {
    properties: BTreeMap<Member, PropertyDef>,
    callbacks: BTreeMap<Member, Function>,
    callback_defaults: BTreeMap<Vec<Member>, Member>,
    root: Element,
    selectors: Vec<StateSelector>,
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
    media_source_file: Option<String>,
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
    clipboard_paste: Option<table_edit::ClipboardPaste>,
    clipboard_text: Option<table::ClipboardText>,
    contains_delegate: bool,
    visible_field: Option<Rc<[String]>>,
    height_field: Option<Rc<[String]>>,
    instance_handlers: BTreeMap<Vec<Member>, Function>,
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
    render_selector: Option<usize>,
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
    timing: Animation,
    enabled: Option<Expr>,
}

struct StateSelector {
    owner: String,
    conditions: Vec<Expr>,
    persistent: bool,
}

struct StateTimingSpec {
    selector: usize,
    index: i64,
    enter: bool,
    animation: AnimationSpec,
}

#[derive(Clone)]
struct StateCase {
    selector: usize,
    index: i64,
    value: Expr,
}

#[derive(Clone)]
struct Input {
    endpoint: Vec<Member>,
    source: Option<InputSource>,
    cases: Vec<StateCase>,
}

impl Input {
    fn new(endpoint: Vec<xrack_mlir::PropertyEndpoint<'_>>) -> Self {
        Self {
            endpoint: Member::path(endpoint),
            source: None,
            cases: Vec::new(),
        }
    }
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
pub struct Instance(Rc<evaluate::State>, Option<Rc<evaluate::Globals>>);

impl Instance {
    /// Capture this instance in a callback without keeping the component alive.
    pub(crate) fn from_state(state: Rc<evaluate::State>) -> Self {
        Self(state, None)
    }

    pub fn as_weak(&self) -> xrack_runtime::WeakComponent<Self> {
        xrack_runtime::WeakComponent::new(&self.0, &self.0.binding_lifetime, |state| {
            let registry = match &state.globals {
                evaluate::GlobalContext::Member(registry) => registry.upgrade(),
                evaluate::GlobalContext::Owner(_) => None,
            };
            Self(state, registry)
        })
    }

    /// Get an exported global in this component tree.
    pub fn global(&self, name: &str) -> Result<Instance> {
        if self.program().globals.get(name) != Some(&true) {
            return Err(Error::Invalid(format!("unknown exported global {name}")));
        }
        let mut global = self.shared_global(name)?;
        global.1 = Some(self.0.globals.registry()?);
        Ok(global)
    }

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

    fn read_public_value(&self, name: &str) -> Result<Value> {
        let member = Member::root(name);
        let property = self.require_public(&member, false)?;
        let value = self.read(&member)?;
        self.program()
            .observe_host_value(&property.ty, &value, format_args!("property {name}"))?;
        Ok(value)
    }

    /// Read a public property. Record and touch lists retain their model handles.
    pub fn get(&self, name: &str) -> Result<Value> {
        self.read_public_value(name).map(evaluate::host_value)
    }

    /// Read a public property by value, as generated Rust getters do.
    ///
    /// Lists are detached from the live model. Reading a snapshot in a binding
    /// tracks row edits, insertions, removals and reordering.
    pub fn get_snapshot(&self, name: &str) -> Result<Value> {
        let value = self.read_public_value(name)?;
        Ok(match value {
            Value::StringModel(_) => evaluate::host_value(value),
            value => evaluate::callback_value(value),
        })
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

    /// Get the shared record model, validating its current rows against the declared schema.
    pub fn record_model(&self, name: &str) -> Result<RecordList> {
        let Value::RecordList(model) = self.read_public_value(name)? else {
            return Err(Error::Evaluation(format!(
                "property {name} is not a record list"
            )));
        };
        Ok(model)
    }

    /// Get the shared read-only touch model without copying its rows.
    pub fn touch_points_model(&self, name: &str) -> Result<TouchPoints> {
        let member = Member::root(name);
        self.require_public(&member, false)?;
        match self.read(&member)? {
            Value::TouchPoints(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a touch-point list"
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

    /// Install a handler with mutable captured state for a non-pure callback.
    ///
    /// Calls to the same handler cannot reenter it. A handler may replace itself
    /// before invoking the callback again.
    pub fn on_mut(
        &self,
        name: &str,
        callback: impl FnMut(&[Value]) -> std::result::Result<Option<Value>, String> + 'static,
    ) -> Result<()> {
        self.register_mut_callback(&Member::root(name), callback)
    }

    pub fn invoke(&self, name: &str, arguments: &[Value]) -> Result<Option<Value>> {
        let member = Member::root(name);
        let arguments: Vec<_> = arguments
            .iter()
            .cloned()
            .map(evaluate::callback_value)
            .collect();
        Ok(self.call(&member, arguments)?.map(evaluate::host_value))
    }

    pub fn try_render(&self) -> Result<Rc<Node>> {
        self.render_scene()
    }

    /// Get an evaluation error raised from an event handled through Component.
    pub fn take_error(&self) -> Option<Error> {
        self.0.error.take()
    }
}
