//! Load optimized XR dialect IR and execute its UI on the XRack runtime.
#![deny(clippy::expect_used, clippy::unwrap_used)]
mod action;
mod calendar;
mod evaluate;
mod external_loader;
pub use external_loader::install_external_loader;
mod gesture;
mod group;
mod host;
#[cfg(feature = "live-reload")]
pub mod live;
#[cfg(all(feature = "live-reload", unix))]
mod live_attach;
#[cfg(all(feature = "live-reload", unix))]
mod live_editor;
#[cfg(all(feature = "live-reload", unix))]
mod live_inspect;
#[cfg(all(feature = "live-reload", unix))]
mod live_trace;
mod load;
mod native_dialog;
mod picker;
mod pointer;
mod range;
mod reload;
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
pub use reload::ReloadableInstance;
use xrack_mlir::{BinaryOperator, RowSceneCache, UnaryOperator, VisualCacheGroup, VisualCacheKey};
use xrack_resources::{EmbeddedTranslations, field::BuiltinField};
#[doc(hidden)]
pub use xrack_runtime as __live_runtime;
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

/// Load the interpreter artifact and the resources of a packaged XRack module.
pub fn load_module_package(root: impl AsRef<Path>) -> Result<Program> {
    use xrack_syntax::module_manifest::{MANIFEST_FILE, ModuleManifest};

    let root = root.as_ref();
    let path = ModuleManifest::resolve(root, Path::new(MANIFEST_FILE)).map_err(Error::Invalid)?;
    let manifest = xrack_ir::modules::read_package_manifest(&path).map_err(Error::Invalid)?;
    let mlir = ModuleManifest::resolve(root, &manifest.mlir).map_err(Error::Invalid)?;
    let inputs = manifest
        .inputs
        .iter()
        .map(|path| ModuleManifest::resolve(root, path))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Invalid)?;
    let text = std::fs::read_to_string(&mlir)
        .map_err(|error| Error::Invalid(format!("{}: {error}", mlir.display())))?;
    let module = xrack_mlir::Module::parse(&text).map_err(Error::Invalid)?;
    let (module, translations) =
        xrack_resources::prepare_module(module, &mlir, Some(&inputs), &BTreeMap::new())
            .map_err(|error| Error::Invalid(error.to_string()))?;
    let (program, _) = Program::from_module_with_declared_resources(
        &module,
        &mlir,
        translations.catalog,
        Some(&inputs),
    )?;
    let declared: BTreeSet<_> = manifest.components.iter().map(String::as_str).collect();
    if program.exported_components().collect::<BTreeSet<_>>() != declared {
        return Err(Error::Invalid(
            "module manifest exports do not match the IR".into(),
        ));
    }
    Ok(program)
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
    #[cfg(all(feature = "live-reload", unix))]
    update_plan: Rc<serde_json::Value>,
    components: Rc<BTreeMap<String, Result<Definition>>>,
    exports: Rc<BTreeSet<String>>,
    globals: Rc<BTreeMap<String, bool>>,
    records: Rc<RecordSchemas>,
    record_validator: Rc<RecordRowValidator>,
    translations: Option<Rc<TranslationCatalog>>,
    fonts: Option<LoadedFonts>,
    scene_assets: Rc<xrack_resources::SceneAssets>,
    scene_projections: Rc<xrack_runtime::scene3d::SceneProjectionCache>,
}

impl Program {
    fn prepare_scene(
        &self,
        model: &xrack_runtime::RecordList,
        renderables: bool,
    ) -> std::sync::Arc<xrack_runtime::scene3d::SceneModel> {
        let imported = |key: &str| self.scene_assets.meshes.get(key).cloned();
        if renderables {
            self.scene_projections.record_objects(
                model,
                imported,
                |key| self.scene_assets.textures.get(key).cloned(),
                |key| self.scene_assets.material_shaders.get(key).cloned(),
            )
        } else {
            self.scene_projections.record_cameras(model, imported)
        }
    }

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
        load::program(
            module,
            translations,
            fonts,
            images,
            paths,
            xrack_resources::SceneAssets::default(),
            "",
        )
    }

    /// Load file-backed images and fonts for a prepared module, using the
    /// translation catalog validated before optimization. Returns asset paths
    /// for callers that watch their inputs for changes.
    pub fn from_module_with_file_resources(
        module: &xrack_mlir::Module,
        filename: impl AsRef<Path>,
        translations: Option<EmbeddedTranslations>,
    ) -> Result<(Self, BTreeSet<PathBuf>)> {
        Self::from_module_with_declared_resources(module, filename, translations, None)
    }

    #[doc(hidden)]
    pub fn from_module_with_declared_resources(
        module: &xrack_mlir::Module,
        filename: impl AsRef<Path>,
        translations: Option<EmbeddedTranslations>,
        assets: Option<&[PathBuf]>,
    ) -> Result<(Self, BTreeSet<PathBuf>)> {
        module.verify().map_err(Error::Invalid)?;
        let assets = xrack_resources::resolve_assets(module, filename.as_ref(), assets)
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
            assets.scene,
            &filename.as_ref().to_string_lossy(),
        )?;
        Ok((program, assets.inputs))
    }

    pub fn exported_components(&self) -> impl Iterator<Item = &str> {
        self.exports.iter().map(String::as_str)
    }

    pub fn create(&self, name: &str) -> Result<Instance> {
        install_external_loader();
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
    Point2D,
    Point3D,
    Vector3D,
    Quaternion3D,
    RotationTrack3D,
    Hit3D,
    Hit3DList,
    PhysicsContact3D,
    PhysicsHit3D,
    PhysicsContact3DList,
    PhysicsHit3DList,
    ProjectedPoint3D,
    Instance3D,
    InstanceTable3D,
    WalkResult3D,
    PhysicsWorld3D,
    SceneModel3D,
    Vector2D,
    Transform2D,
    Percent,
    Color,
    Brush,
    String,
    StringList,
    IntList,
    DateList,
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
    SelectionGesture,
    ScrollBarPolicy,
    FlickDirection,
    FlickBoundsBehavior,
    FlickBoundsMovement,
    PopupPlacement,
    LayoutDirection,
    FlexboxLayoutDirection,
    FlexboxLayoutWrap,
    CrossAxisAlignment,
    TextWrap,
    MouseSelectionMode,
    InputValidity,
    TextHorizontalAlignment,
    TextVerticalAlignment,
    TextOverflow,
    ColorScheme,
    SceneToneMap,
    InputType,
    InputPurpose,
    KeyboardCapitalization,
    ReturnKey,
    AccessibleLive,
    DialInputMode,
    Image,
    LogLevel,
    ImageFit,
    ImageTileMode,
    ImageHorizontalAlignment,
    ImageVerticalAlignment,
    ImageRendering,
    ButtonDisplay,
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

#[derive(Clone)]
enum TextPart {
    Static(Rc<str>),
    Dynamic(Expr),
}
type VisualSlot = VisualCacheKey<usize>;

#[derive(Clone)]
enum Expression {
    Literal(Value),
    CachedVisual(VisualSlot, Expr),
    Read(Member),
    Param(usize),
    RowParam(String),
    SplitHandleParam(split::Parameter),
    ListSectionParam(xrack_runtime::ListSectionParameter),
    ComboOptionParam(xrack_runtime::ComboOptionParameter),
    CalendarCellParam(calendar::Parameter),
    Unary(UnaryOperator, Expr),
    Binary(BinaryOperator, Expr, Expr),
    Concat(Vec<TextPart>),
    Select(Expr, Expr, Expr),
    MatchEnum(Expr, Vec<Expr>),
    EnumMatch {
        name: Rc<str>,
        subject: Expr,
        cases: Vec<EnumCase>,
    },
    Cast(Expr),
    Round(Expr),
    MathFunction(xrack_syntax::builtins::MathFunction, Vec<Expr>),
    GeometryFunction(xrack_syntax::builtins::GeometryFunction, Vec<Expr>),
    PhysicsFunction(xrack_syntax::builtins::PhysicsFunction, Vec<Expr>),
    StringFunction(xrack_syntax::builtins::StringFunction, Vec<Expr>),
    ModelProjection {
        model: Expr,
        field: Vec<String>,
        option: Expr,
        filter: bool,
        exact: bool,
    },
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
    TextAccepts(String),
    TextValidity(String),
    GeometryRead(String, Option<xrack_runtime::GeometryMetric>),
    ViewportRead(String, viewport::Metric),
    ListSelectionRead(String),
    ListRowSelected(String, Expr),
    ViewCount {
        owner: String,
        source: bool,
    },
    ViewIndex(String, Expr, xrack_runtime::ViewIndex),
    ViewIndexForKey(String, Expr),
    TreeQuery(String, tree::Query, Expr),
    SelectionKeys(String),
    SplitSave(String),
    LoaderMounted(String, Expr),
    Dynamic(String, Vec<String>, Vec<Expr>),
    LoaderExternalStatus(String, Expr),
    StackRead(String, xrack_mlir::StackRead),
    SplitResizing(String),
    TableRead(String, xrack_mlir::TableRead),
    FocusRead(String, bool),
    ComboPopupRead(String, bool),
    ButtonPressed(String),
    ButtonChecked(String),
    RadioChecked {
        target: String,
        group: String,
    },
    NavigationActive {
        target: String,
        group: String,
    },
    NavigationIndex(String),
    NavigationCount(String),
    ToggleStateRead(Member),
    NumericValueRead(Member),
    ComboStateRead(Member, Value),
    RangePositionRead(Member),
    TextSelectionRead(Member),
    TextCommandAvailable(Member),
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
    IntArray(Vec<Expr>),
    DateArray(Vec<Expr>),
    Hit3DArray(Vec<Expr>),
    PhysicsContact3DArray(Vec<Expr>),
    PhysicsHit3DArray(Vec<Expr>),
    Record {
        name: Rc<str>,
        fields: Vec<(String, Expr)>,
        base: Option<Expr>,
    },
    RecordArray(Rc<str>, Vec<Expr>),
    RecordField(Expr, String),
    BuiltinField(Expr, BuiltinField),
    MakeWalkResult3D([Expr; 2]),
    MakeDate([Expr; 3]),
    DateIsValid(Expr),
    DateWeekday(Expr),
    DateMonths(Expr, Expr),
    DateMonthIndex(Expr, Expr, Expr),
    DateParseIso(Expr, Expr),
    DateIsSelectable(Vec<Expr>),
    DateAddDays(Expr, Expr, Expr),
    DateAddMonths(Expr, Expr, Expr),
    MakeTime([Expr; 3]),
    TimeParseIso(Expr, Expr),
    TimeIsValidIso(Expr),
    TimeFormatIso(Expr, Expr),
    TimeInRange(Expr, Expr, Expr),
    MakeFont([Expr; 4]),
    Call(Member, Vec<Expr>, CallSite),
}

#[derive(Clone)]
enum Statement {
    Local(Expr),
    If {
        condition: Expr,
        yes: Vec<Statement>,
        no: Vec<Statement>,
    },
    For {
        model: Expr,
        indexed: bool,
        body: Vec<Statement>,
    },
    Assign(Member, Expr),
    Evaluate(Expr),
    Call(Member, Vec<Expr>, CallSite),
    Log {
        level: Expr,
        message: Expr,
        file: String,
        span: xrack_syntax::ast::Span,
    },
    Scroll(String, xrack_runtime::ScrollCommandMethod, Vec<Expr>),
    PositionChild {
        view: String,
        target: String,
        alignments: [Expr; 2],
    },
    SelectSource(String, Expr, Option<Expr>),
    SelectKey(String, Expr),
    SelectViewKeys(String, Expr),
    Table(String, table::Command, Vec<Expr>),
    ModelEdit(Expr, xrack_syntax::builtins::ModelEditKind, Vec<Expr>),
    MoveNavigationItem(String, [Expr; 2]),
    Focus(String, xrack_runtime::FocusCommandMethod, Vec<Expr>),
    ButtonClick(String, bool),
    ButtonToggle(String),
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
struct CallSite(#[cfg(all(feature = "live-reload", unix))] Option<Rc<live_inspect::Source>>);

impl CallSite {
    fn from_operation(op: xrack_mlir::OperationRef<'_>) -> Self {
        #[cfg(all(feature = "live-reload", unix))]
        return Self(live_inspect::Source::from_operation(op).map(Rc::new));
        #[cfg(not(all(feature = "live-reload", unix)))]
        {
            let _ = op;
            Self()
        }
    }
}

#[derive(Clone)]
struct Function {
    #[cfg(all(feature = "live-reload", unix))]
    name: String,
    #[cfg(all(feature = "live-reload", unix))]
    location: Option<live_inspect::Source>,
    #[cfg(all(feature = "live-reload", unix))]
    parameter_names: Vec<String>,
    parameters: Vec<ValueType>,
    result: Option<ValueType>,
    body: Vec<Statement>,
    pure: bool,
    cache_default: bool,
    inline_default: bool,
    alias: Option<Member>,
}

struct PropertyDef {
    #[cfg(all(feature = "live-reload", unix))]
    location: Option<live_inspect::Source>,
    #[cfg(all(feature = "live-reload", unix))]
    visibility: String,
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

#[derive(Clone)]
struct EnumCase {
    fields: Vec<String>,
    result: Expr,
}

struct RecordSchema {
    fields: Vec<RecordField>,
    variants: Vec<Vec<RecordField>>,
}

type RecordSchemas = BTreeMap<String, RecordSchema>;

struct Definition {
    #[cfg(all(feature = "live-reload", unix))]
    source: Option<live_inspect::Source>,
    properties: BTreeMap<Member, PropertyDef>,
    callbacks: BTreeMap<Member, Function>,
    callback_defaults: BTreeMap<Vec<Member>, Member>,
    root: Element,
    selectors: Vec<StateSelector>,
    geometry_targets: BTreeSet<String>,
    viewport_targets: BTreeSet<String>,
    source_scroll_targets: BTreeSet<String>,
    key_scroll_targets: BTreeSet<String>,
    focus_targets: BTreeSet<String>,
    button_press_targets: BTreeSet<String>,
    combo_popup_targets: BTreeSet<String>,
    range_position_targets: BTreeSet<String>,
    button_animation_targets: BTreeSet<String>,
    text_selection_targets: BTreeSet<String>,
    text_command_targets: BTreeSet<String>,
    timer_order: Vec<(String, bool)>,
    action_keys: BTreeSet<String>,
    native_dialog_keys: BTreeSet<String>,
    button_groups: BTreeMap<String, group::Members>,
}

struct Element {
    key: String,
    #[cfg(all(feature = "live-reload", unix))]
    source: Option<live_inspect::Source>,
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
    section_header: Option<Box<Element>>,
    combo_option: Option<Box<Element>>,
    clipboard_paste: Option<table_edit::ClipboardPaste>,
    clipboard_text: Option<table::ClipboardText>,
    contains_delegate: bool,
    visible_field: Option<Rc<[String]>>,
    height_field: Option<Rc<[String]>>,
    section_field: Option<Rc<[String]>>,
    typeahead_field: Option<Rc<[String]>>,
    row_key_field: Option<Rc<[String]>>,
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
    sound_effects: Vec<Element>,
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
    loader: Option<(String, usize)>,
    model: Expr,
    item: String,
    index: Option<String>,
    positioned: bool,
    empty: bool,
    static_count: Option<i64>,
    scene_cache: Option<RowSceneCache>,
    uses_row_identity: bool,
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
            Value::StringModel(_) | Value::IntModel(_) | Value::DateModel(_) => {
                evaluate::host_value(value)
            }
            value => evaluate::callback_value(value),
        })
    }

    /// Get the shared hit-list model without replacing its row identities.
    pub fn hit3d_model(&self, name: &str) -> Result<xrack_runtime::VecModel<xrack_runtime::Hit3D>> {
        let member = Member::root(name);
        self.require_public(&member, false)?;
        match self.read(&member)? {
            Value::Hit3DList(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a hit list"
            ))),
        }
    }

    /// Get the shared physics contact model without replacing its row identities.
    pub fn physics_contact3d_model(
        &self,
        name: &str,
    ) -> Result<xrack_runtime::VecModel<xrack_runtime::PhysicsContact3D>> {
        let member = Member::root(name);
        self.require_public(&member, false)?;
        match self.read(&member)? {
            Value::PhysicsContact3DList(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a physics contact list"
            ))),
        }
    }

    pub fn physics_hit3d_model(
        &self,
        name: &str,
    ) -> Result<xrack_runtime::VecModel<xrack_runtime::PhysicsHit3D>> {
        let member = Member::root(name);
        self.require_public(&member, false)?;
        match self.read(&member)? {
            Value::PhysicsHit3DList(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a physics hit list"
            ))),
        }
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

    /// Get the shared integer model, preserving live row identities.
    pub fn int_model(&self, name: &str) -> Result<xrack_runtime::VecModel<i64>> {
        match self.read_public_value(name)? {
            Value::IntModel(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not an int list"
            ))),
        }
    }

    /// Get the shared date model, preserving live row identities.
    pub fn date_model(&self, name: &str) -> Result<xrack_runtime::VecModel<xrack_runtime::Date>> {
        match self.read_public_value(name)? {
            Value::DateModel(model) => Ok(model),
            _ => Err(Error::Evaluation(format!(
                "property {name} is not a date list"
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

    /// Check the schema of a row before a host modifies a writable model.
    pub fn validate_record_row(&self, name: &str, row: &RecordValue) -> Result<()> {
        let member = Member::root(name);
        let property = self.require_public(&member, true)?;
        let ValueType::RecordList(record) = &property.ty else {
            return Err(Error::Evaluation(format!(
                "property {name} is not a record list"
            )));
        };
        self.program().validate_host_value(
            &ValueType::Record(record.clone()),
            &Value::Record(row.clone()),
            format_args!("property {name} row"),
        )
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
