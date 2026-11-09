//! Renderer-independent scene snapshots produced by generated components.

/// Compatibility level expected by the Rust code emitted by xrackc.
#[doc(hidden)]
pub const CODEGEN_ABI_VERSION: u32 = 1;

mod animation;
mod calendar;
mod child_window;
mod color_picker;
mod combo;
mod controls;
mod delegate;
mod double_spin;
mod drag;
mod list;
mod model;
mod native_dialog;
mod picker;
mod property;
mod range;
mod repeat;
mod scene_plan;
mod split;
mod stack;
mod table_selection;
mod text_input;
mod time_picker;
mod timer;
mod tree;
mod tumbler;
pub use animation::SceneTransition;
pub use calendar::{CalendarAction, CalendarState, Date};
#[doc(hidden)]
pub use calendar::{DateBounds, DatePopupState};
#[doc(hidden)]
pub use child_window::{SecondaryWindowRegistration, SecondaryWindowSlot};
#[doc(hidden)]
pub use color_picker::ColorPopupState;
pub use combo::{
    ChoiceIdentity, choice_identity, combo_count, combo_find_label, combo_index, combo_label,
    combo_labels, combo_row_index, combo_row_key, combo_text,
};
#[doc(hidden)]
pub use combo::{ComboItems, ComboModel, combo_value};
pub use controls::{
    ActionKey, ControlAction, DialogCommand, DialogRole, PopupPlacement, SpinAction, SpinRange,
    StandardButtonKind, SwipeSide, TableHeader, TableSort, button_text, combo_editable,
    delay_millis, dialog_button_action, group_index, group_item, is_button, is_modal_overlay,
    is_spin_box, is_window_overlay, page_indicator_count, page_indicator_index,
    resizable_table_column, resolve_swipe_index, search_suggestion, search_suggestions,
    spin_editable, spin_text, spin_value_after, text_input_value,
};
#[doc(hidden)]
pub use delegate::{ComponentRepeater, RowContext};
pub use double_spin::{
    DecimalSpinRange, SpinDecimal, decimal_separator, decimal_spin_value_after, spin_text_is_valid,
};
pub use drag::{DragAction, DragEvent, is_drag_area};
pub use list::{ListMetrics, RowKey};
#[doc(hidden)]
pub use list::{ListModel, RowLifetime, standard_list_rows, standard_table_rows};
pub use model::VecModel;
pub use native_dialog::{
    FileDialogMode, FileDialogOptions, MessageButtons, MessageDialogOptions, MessageLevel,
};
#[doc(hidden)]
pub use native_dialog::{
    NativeDialogOptions, NativeDialogPresentation, NativeDialogQueue, NativeDialogRegistration,
    NativeDialogResult, NativeDialogSlot,
};
pub use property::{ChangeSignal, Property};
pub use range::{
    DialInputMode, IndicatorState, RangeAction, SliderInterval, SliderRange, indicator_state,
    progress_value,
};
#[doc(hidden)]
pub use repeat::Repeater;
#[doc(hidden)]
pub use scene_plan::{
    GridCellPosition, GridCellSlot, GridPlanner, GridTrackInput, StaticVectorPath, VectorSegment,
};
pub use stack::{StackNavigation, StackSnapshot, StackState};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
pub use table_selection::{
    RowSelection, SelectionTarget, TableSelection, TableSelectionAction, TableSelectionSnapshot,
};
pub use text_input::InputType;
pub use time_picker::Time;
#[doc(hidden)]
pub use time_picker::{TimePickerOptions, TimePopupState};
#[doc(hidden)]
pub use timer::{
    TimerCommand, TimerConfig, TimerDelivery, TimerQueue, TimerRegistration, TimerSlot, TimerTick,
};
pub use tree::{TreeAction, TreeItem, TreeSelectionAction, TreeSnapshot, TreeState};
pub use tumbler::{Tumbler, TumblerItems};

/// A UI-thread handle that can be captured by callbacks without keeping a component alive.
pub struct WeakComponent<C>(Rc<dyn Fn() -> Option<C>>);

impl<C> Clone for WeakComponent<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<C: 'static> WeakComponent<C> {
    #[doc(hidden)]
    pub fn new<S: 'static>(state: &Rc<S>, wrap: fn(Rc<S>) -> C) -> Self {
        let state = Rc::downgrade(state);
        Self(Rc::new(move || state.upgrade().map(wrap)))
    }

    pub fn upgrade(&self) -> Option<C> {
        (self.0)()
    }
}

/// The backend-independent interface implemented by generated UI components.
pub trait Component {
    fn render(&self) -> Node;

    /// Return the current scene. Reactive components may reuse a clean snapshot.
    fn render_shared(&self) -> Rc<Node> {
        Rc::new(self.render())
    }

    /// Report invalidation caused by a shared reactive dependency.
    fn scene_dirty(&self) -> bool {
        false
    }

    /// Decide whether the host may close this window.
    fn close_requested(&mut self) -> bool {
        true
    }

    /// Consume a close command emitted by the component.
    fn take_window_close_request(&mut self) -> bool {
        false
    }

    /// Native child windows currently requested by this component.
    #[doc(hidden)]
    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        None
    }

    /// Render the requested model rows of a virtual view in a base scene.
    fn list_rows(&self, _path: &[usize], _rows: std::ops::Range<usize>) -> Vec<Node> {
        Vec::new()
    }

    fn dispatch(&mut self, element_id: &str, event: &str) -> bool;

    /// Commit the complete value of an editable text item after input validation.
    fn edit_at(&mut self, _scene: &Node, _path: &[usize], _text: String) -> bool {
        false
    }

    /// Commit a TableCell or TreeCell draft only if its model row and original value still match.
    fn commit_table_cell(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _original: &str,
        _text: String,
    ) -> bool {
        false
    }

    /// Apply a range control action through the same path used by pointer and keyboard input.
    fn adjust_range_at(&mut self, _scene: &Node, _path: &[usize], _action: RangeAction) -> bool {
        false
    }

    fn control_at(&mut self, _scene: &Node, _path: &[usize], _action: ControlAction) -> bool {
        false
    }

    #[doc(hidden)]
    fn drag_event_at(&mut self, _scene: &Node, _path: &[usize], _event: &DragEvent) -> bool {
        false
    }

    fn resize_split_at(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _weights: &[(usize, f32)],
    ) -> bool {
        false
    }

    /// Dispatch to the node hit by the backend. IDs may repeat in component instances.
    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        scene
            .at_path(path)
            .and_then(|node| node.id)
            .is_some_and(|id| self.dispatch(id, event))
    }
}

impl<T: Component + ?Sized> Component for Box<T> {
    fn render(&self) -> Node {
        (**self).render()
    }
    fn drag_event_at(&mut self, scene: &Node, path: &[usize], event: &DragEvent) -> bool {
        (**self).drag_event_at(scene, path, event)
    }
    fn render_shared(&self) -> Rc<Node> {
        (**self).render_shared()
    }
    fn scene_dirty(&self) -> bool {
        (**self).scene_dirty()
    }
    fn close_requested(&mut self) -> bool {
        (**self).close_requested()
    }
    fn take_window_close_request(&mut self) -> bool {
        (**self).take_window_close_request()
    }
    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        (**self).secondary_windows()
    }

    fn list_rows(&self, path: &[usize], rows: std::ops::Range<usize>) -> Vec<Node> {
        (**self).list_rows(path, rows)
    }

    fn dispatch(&mut self, element_id: &str, event: &str) -> bool {
        (**self).dispatch(element_id, event)
    }

    fn edit_at(&mut self, scene: &Node, path: &[usize], text: String) -> bool {
        (**self).edit_at(scene, path, text)
    }

    fn commit_table_cell(
        &mut self,
        scene: &Node,
        path: &[usize],
        original: &str,
        text: String,
    ) -> bool {
        (**self).commit_table_cell(scene, path, original, text)
    }

    fn adjust_range_at(&mut self, scene: &Node, path: &[usize], action: RangeAction) -> bool {
        (**self).adjust_range_at(scene, path, action)
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        (**self).control_at(scene, path, action)
    }

    fn resize_split_at(&mut self, scene: &Node, path: &[usize], weights: &[(usize, f32)]) -> bool {
        (**self).resize_split_at(scene, path, weights)
    }

    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        (**self).dispatch_at(scene, path, event)
    }
}

#[doc(hidden)]
pub enum RoutedEvent<'a> {
    Dispatch(&'a str),
    Edit(&'a str),
    CommitCell { original: &'a str, text: &'a str },
    Range(RangeAction),
    Control(ControlAction),
    Drag(&'a DragEvent),
    ResizeSplit(&'a [(usize, f32)]),
}

impl RoutedEvent<'_> {
    pub fn apply(&self, ui: &mut impl Component, scene: &Node, path: &[usize]) -> bool {
        match self {
            Self::Dispatch(event) => ui.dispatch_at(scene, path, event),
            Self::Edit(text) => ui.edit_at(scene, path, (*text).into()),
            Self::CommitCell { original, text } => {
                ui.commit_table_cell(scene, path, original, (*text).into())
            }
            Self::Range(action) => ui.adjust_range_at(scene, path, *action),
            Self::Control(action) => ui.control_at(scene, path, *action),
            Self::Drag(event) => ui.drag_event_at(scene, path, event),
            Self::ResizeSplit(weights) => ui.resize_split_at(scene, path, weights),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Length(pub f64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Percent(pub f64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Svg,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Image {
    #[default]
    Empty,
    Embedded {
        format: ImageFormat,
        bytes: &'static [u8],
        size: [u32; 2],
    },
}

impl Image {
    pub fn size(self) -> [u32; 2] {
        match self {
            Self::Empty => [0, 0],
            Self::Embedded { size, .. } => size,
        }
    }
}

/// Immutable font data shared by scenes from the same generated UI library.
pub struct EmbeddedFonts(&'static [&'static [u8]]);

impl EmbeddedFonts {
    pub const fn new(fonts: &'static [&'static [u8]]) -> Self {
        Self(fonts)
    }

    pub fn sources(&self) -> &'static [&'static [u8]] {
        self.0
    }
}

impl PartialEq for EmbeddedFonts {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl std::fmt::Debug for EmbeddedFonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedFonts")
            .field("count", &self.0.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    Fill,
    #[default]
    Contain,
    Cover,
    Preserve,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutAlignment {
    #[default]
    Stretch,
    Center,
    Start,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossAxisAlignment {
    #[default]
    Auto,
    Stretch,
    Start,
    End,
    Center,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexboxLayoutDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexboxLayoutWrap {
    #[default]
    Wrap,
    NoWrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawerEdge {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBarPolicy {
    #[default]
    AsNeeded,
    AlwaysOn,
    AlwaysOff,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrap {
    #[default]
    WordWrap,
    NoWrap,
    CharWrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextHorizontalAlignment {
    #[default]
    Start,
    End,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextVerticalAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Elide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    #[default]
    Unchecked,
    PartiallyChecked,
    Checked,
}

impl From<bool> for CheckState {
    fn from(checked: bool) -> Self {
        if checked {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }
}

impl CheckState {
    pub fn next(self, tristate: bool) -> Self {
        match self {
            Self::Checked => Self::Unchecked,
            Self::Unchecked if tristate => Self::PartiallyChecked,
            _ => Self::Checked,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Position along the gradient in the range 0.0 to 1.0.
    pub offset: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    Linear {
        angle_degrees: f64,
        stops: Arc<[GradientStop]>,
    },
    Radial {
        stops: Arc<[GradientStop]>,
    },
}

impl Default for Brush {
    fn default() -> Self {
        Self::Solid(Color::default())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    StringList(Vec<String>),
    ComboModel(ComboModel),
    Length(Length),
    Percent(Percent),
    Color(Color),
    Brush(Brush),
    Image(Image),
    ImageFit(ImageFit),
    LayoutAlignment(LayoutAlignment),
    CrossAxisAlignment(CrossAxisAlignment),
    Orientation(Orientation),
    FlexboxLayoutDirection(FlexboxLayoutDirection),
    FlexboxLayoutWrap(FlexboxLayoutWrap),
    DialInputMode(DialInputMode),
    ScrollBarPolicy(ScrollBarPolicy),
    TextWrap(TextWrap),
    TextHorizontalAlignment(TextHorizontalAlignment),
    TextVerticalAlignment(TextVerticalAlignment),
    TextOverflow(TextOverflow),
    InputType(InputType),
    ColorScheme(ColorScheme),
    FileDialogMode(FileDialogMode),
    MessageLevel(MessageLevel),
    MessageButtons(MessageButtons),
    CheckState(CheckState),
    DialogRole(DialogRole),
    StandardButtonKind(StandardButtonKind),
    SwipeSide(SwipeSide),
    DragAction(DragAction),
    DrawerEdge(DrawerEdge),
    PopupPlacement(PopupPlacement),
    SelectionTarget(SelectionTarget),
    Date(Date),
    Time(Time),
}

macro_rules! value_from {
    ($ty:ty, $variant:ident) => {
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Self::$variant(value)
            }
        }
    };
}
value_from!(i64, Int);
value_from!(f64, Float);
value_from!(bool, Bool);
value_from!(String, String);
value_from!(Vec<String>, StringList);

impl From<VecModel<String>> for Value {
    fn from(model: VecModel<String>) -> Self {
        Self::StringList(model.snapshot())
    }
}
value_from!(Length, Length);
value_from!(Percent, Percent);
value_from!(Color, Color);
value_from!(Image, Image);
value_from!(ImageFit, ImageFit);
value_from!(LayoutAlignment, LayoutAlignment);
value_from!(CrossAxisAlignment, CrossAxisAlignment);
value_from!(Orientation, Orientation);
value_from!(FlexboxLayoutDirection, FlexboxLayoutDirection);
value_from!(FlexboxLayoutWrap, FlexboxLayoutWrap);
value_from!(DialInputMode, DialInputMode);
value_from!(ScrollBarPolicy, ScrollBarPolicy);
value_from!(TextWrap, TextWrap);
value_from!(TextHorizontalAlignment, TextHorizontalAlignment);
value_from!(TextVerticalAlignment, TextVerticalAlignment);
value_from!(TextOverflow, TextOverflow);
value_from!(InputType, InputType);
value_from!(ColorScheme, ColorScheme);
value_from!(FileDialogMode, FileDialogMode);
value_from!(MessageLevel, MessageLevel);
value_from!(MessageButtons, MessageButtons);
value_from!(CheckState, CheckState);
value_from!(DialogRole, DialogRole);
value_from!(StandardButtonKind, StandardButtonKind);
value_from!(SwipeSide, SwipeSide);
value_from!(DragAction, DragAction);
value_from!(DrawerEdge, DrawerEdge);
value_from!(PopupPlacement, PopupPlacement);
value_from!(SelectionTarget, SelectionTarget);
value_from!(Date, Date);
value_from!(Time, Time);

impl From<Brush> for Value {
    fn from(value: Brush) -> Self {
        match value {
            Brush::Solid(color) => Self::Color(color),
            gradient => Self::Brush(gradient),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node(NodeStorage);

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // Keep ordinary dynamic nodes inline.
enum NodeStorage {
    Inline(NodeData),
    Shared(Arc<NodeData>),
}

/// Scene data shared by clones of a node. Mutating a node detaches its data.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct NodeData {
    pub kind: &'static str,
    pub id: Option<&'static str>,
    pub properties: BTreeMap<&'static str, Value>,
    #[doc(hidden)]
    pub fonts: Option<&'static EmbeddedFonts>,
    pub tumbler_items: Option<TumblerItems>,
    pub children: Vec<Node>,
    #[doc(hidden)]
    pub timers: Vec<TimerRegistration>,
    #[doc(hidden)]
    pub native_dialogs: Vec<NativeDialogRegistration>,
    pub animations: BTreeMap<&'static str, u64>,
    pub list: Option<ListMetrics>,
    /// Selection snapshot for model-backed views. Retains the generated field name.
    #[doc(hidden)]
    pub standard_list_stamp: Option<(u64, u64)>,
    pub table_header: Option<Box<TableHeader>>,
    pub table_selection: Option<Box<TableSelectionSnapshot>>,
    pub tree: Option<Arc<TreeSnapshot>>,
    pub stack: Option<Arc<StackSnapshot>>,
    pub action: Option<ActionKey>,
    /// Selected TreeView business keys and model row identities, including folded descendants.
    pub tree_selection: Option<Arc<BTreeMap<i64, RowKey>>>,
    pub split_weights: Option<Arc<[Option<f32>]>>,
    pub row_key: Option<RowKey>,
    paint_only_background: bool,
    precompiled_path: Option<&'static StaticVectorPath>,
    static_grid: Option<&'static [GridCellSlot]>,
}

impl std::ops::Deref for Node {
    type Target = NodeData;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            NodeStorage::Inline(data) => data,
            NodeStorage::Shared(data) => data,
        }
    }
}

impl std::ops::DerefMut for Node {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let data = match &mut self.0 {
            NodeStorage::Inline(data) => data,
            NodeStorage::Shared(data) => Arc::make_mut(data),
        };
        // Public edits can change grid tracks, visibility or child order.
        data.static_grid = None;
        data
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl NodeData {
    fn same_structure_and_state(&self, other: &Self) -> bool {
        macro_rules! compare {
            ($($field:ident),+ $(,)?) => {{
                // An exhaustive pattern makes new scene fields opt in to comparison.
                let Self { properties: _, children: _, paint_only_background: _, $($field: _,)+ } = self;
                $(self.$field == other.$field)&&+
            }};
        }
        compare!(
            kind,
            id,
            fonts,
            tumbler_items,
            timers,
            native_dialogs,
            animations,
            list,
            standard_list_stamp,
            table_header,
            table_selection,
            tree,
            stack,
            action,
            tree_selection,
            split_weights,
            row_key,
            precompiled_path,
            static_grid,
        )
    }
}

pub fn is_text_input(kind: &str) -> bool {
    matches!(kind, "TextInput" | "LineEdit" | "SearchField") || is_multiline_text_input(kind)
}

pub fn editable_table_cell(node: &Node) -> bool {
    matches!(node.kind, "TableCell" | "TreeCell")
        && node.boolean("editable", false)
        && !node.boolean("read-only", false)
}

pub fn is_multiline_text_input(kind: &str) -> bool {
    matches!(kind, "TextEdit" | "TextArea")
}

pub fn is_toggle(kind: &str) -> bool {
    matches!(kind, "CheckBox" | "Switch")
}

impl Node {
    #[doc(hidden)]
    pub fn enables_children(&self) -> bool {
        is_drag_area(self.kind) || self.boolean("enabled", true)
    }
    pub fn check_state(&self) -> CheckState {
        match self.properties.get("check-state") {
            Some(Value::CheckState(state)) if self.kind == "CheckBox" => *state,
            _ => self.boolean("checked", false).into(),
        }
    }

    pub fn date(&self, name: &str) -> Option<Date> {
        match self.properties.get(name) {
            Some(Value::Date(date)) => Some(*date),
            _ => None,
        }
    }

    pub fn time(&self, name: &str) -> Option<Time> {
        match self.properties.get(name) {
            Some(Value::Time(time)) => Some(*time),
            _ => None,
        }
    }

    pub fn new(kind: &'static str, id: Option<&'static str>) -> Self {
        Self(NodeStorage::Inline(NodeData {
            kind,
            id,
            properties: BTreeMap::new(),
            fonts: None,
            tumbler_items: None,
            children: Vec::new(),
            timers: Vec::new(),
            native_dialogs: Vec::new(),
            animations: BTreeMap::new(),
            list: None,
            standard_list_stamp: None,
            table_header: None,
            table_selection: None,
            tree: None,
            stack: None,
            action: None,
            tree_selection: None,
            split_weights: None,
            row_key: None,
            paint_only_background: false,
            precompiled_path: None,
            static_grid: None,
        }))
    }

    /// Freeze a static subtree so subsequent snapshot clones share its storage.
    #[doc(hidden)]
    pub fn into_shared(self) -> Self {
        match self.0 {
            NodeStorage::Inline(mut data) => {
                data.children = data.children.into_iter().map(Self::into_shared).collect();
                Self(NodeStorage::Shared(Arc::new(data)))
            }
            NodeStorage::Shared(data) => Self(NodeStorage::Shared(data)),
        }
    }

    pub fn shares_storage_with(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || matches!((&self.0, &other.0), (NodeStorage::Shared(a), NodeStorage::Shared(b)) if Arc::ptr_eq(a, b))
    }

    #[doc(hidden)]
    pub fn set_precompiled_path(&mut self, path: &'static StaticVectorPath) {
        self.precompiled_path = Some(path);
    }

    #[doc(hidden)]
    pub fn precompiled_path(&self, commands: &str) -> Option<&'static StaticVectorPath> {
        self.precompiled_path
            .filter(|path| path.commands == commands)
    }

    #[doc(hidden)]
    pub fn set_static_grid(&mut self, cells: &'static [GridCellSlot]) {
        self.static_grid = Some(cells);
    }

    #[doc(hidden)]
    pub fn static_grid(&self) -> Option<&'static [GridCellSlot]> {
        self.static_grid
    }

    /// Mark a generated Rectangle's background as independent of layout and input.
    #[doc(hidden)]
    pub fn mark_paint_only_background(&mut self) {
        if self.kind == "Rectangle" {
            self.paint_only_background = true;
        }
    }

    /// Compare frame inputs, allowing only compiler-proven paint values to differ.
    #[doc(hidden)]
    pub fn same_frame_inputs(&self, other: &Self) -> bool {
        if self.shares_storage_with(other) {
            return true;
        }
        let ignore_background = self.kind == "Rectangle"
            && other.kind == "Rectangle"
            && self.paint_only_background
            && other.paint_only_background;
        let relevant = |(name, _): &(&&str, &Value)| !ignore_background || **name != "background";
        self.same_structure_and_state(other)
            && self
                .properties
                .iter()
                .filter(relevant)
                .eq(other.properties.iter().filter(relevant))
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(child, other)| child.same_frame_inputs(other))
    }
    /// An empty slot keeps the generated event paths stable across conditional renders.
    pub fn hidden() -> Self {
        let mut node = Self::new("Empty", None);
        node.properties.insert("visible", Value::Bool(false));
        node
    }
    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id == Some(id) {
            Some(self)
        } else {
            self.children.iter().find_map(|n| n.find(id))
        }
    }

    pub fn length(&self, name: &str) -> Option<f32> {
        match self.properties.get(name) {
            Some(Value::Length(Length(value))) if value.is_finite() => {
                Some(value.clamp(-1_000_000.0, 1_000_000.0) as f32)
            }
            _ => None,
        }
    }

    pub fn relative_length(&self, name: &str, reference: f32) -> Option<f32> {
        match self.properties.get(name) {
            Some(Value::Percent(Percent(factor))) if factor.is_finite() => {
                Some((factor * f64::from(reference)).clamp(-1_000_000.0, 1_000_000.0) as f32)
            }
            _ => self.length(name),
        }
    }

    pub fn boolean(&self, name: &str, default: bool) -> bool {
        match self.properties.get(name) {
            Some(Value::Bool(value)) => *value,
            _ => default,
        }
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.properties.get(name) {
            Some(Value::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn strings(&self, name: &str) -> &[String] {
        match self.properties.get(name) {
            Some(Value::StringList(values)) => values,
            _ => &[],
        }
    }

    pub fn float(&self, name: &str) -> Option<f32> {
        self.number(name)
            .map(|value| value.clamp(-1_000_000.0, 1_000_000.0) as f32)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        match self.properties.get(name) {
            Some(Value::Float(value)) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    pub fn orientation(&self) -> Orientation {
        match self.properties.get("orientation") {
            Some(Value::Orientation(value)) => *value,
            _ => Orientation::Horizontal,
        }
    }

    pub fn dial_input_mode(&self) -> DialInputMode {
        match self.properties.get("input-mode") {
            Some(Value::DialInputMode(value)) => *value,
            _ => DialInputMode::Circular,
        }
    }

    pub fn scroll_bar_policy(&self, orientation: Orientation) -> ScrollBarPolicy {
        let property = match orientation {
            Orientation::Horizontal => "horizontal-scroll-bar-policy",
            Orientation::Vertical => "vertical-scroll-bar-policy",
        };
        match self.properties.get(property) {
            Some(Value::ScrollBarPolicy(policy)) => *policy,
            _ => ScrollBarPolicy::AsNeeded,
        }
    }

    pub fn text_wrap(&self) -> TextWrap {
        match self.properties.get("wrap") {
            Some(Value::TextWrap(wrap)) => *wrap,
            _ => TextWrap::default(),
        }
    }

    pub fn is_password(&self) -> bool {
        self.input_type() == InputType::Password
    }

    pub fn password_masked(&self) -> bool {
        self.is_password() && !self.boolean("password-visible", false)
    }

    pub fn dialog_role(&self) -> DialogRole {
        match self.properties.get("role") {
            Some(Value::DialogRole(role)) => *role,
            _ => DialogRole::default(),
        }
    }

    pub fn standard_button_kind(&self) -> Option<StandardButtonKind> {
        match (self.kind, self.properties.get("kind")) {
            ("Button", Some(Value::StandardButtonKind(kind))) => Some(*kind),
            _ => None,
        }
    }

    pub fn swipe_side(&self) -> SwipeSide {
        match self.properties.get("open-side") {
            Some(Value::SwipeSide(side)) => *side,
            _ => SwipeSide::Closed,
        }
    }

    pub fn swipe_panel(&self, side: SwipeSide) -> Option<&Node> {
        let kind = side.panel()?;
        self.children
            .iter()
            .find(|child| child.kind == kind && child.is_visible())
    }

    pub fn drawer_edge(&self) -> DrawerEdge {
        match self.properties.get("edge") {
            Some(Value::DrawerEdge(edge)) => *edge,
            _ => DrawerEdge::Left,
        }
    }

    pub fn selection_target(&self) -> SelectionTarget {
        match self.properties.get("selection-target") {
            Some(Value::SelectionTarget(target)) => *target,
            _ => SelectionTarget::Rows,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.boolean("visible", true)
            && (!is_window_overlay(self.kind) || self.boolean("open", false))
    }

    pub fn is_modal(&self) -> bool {
        is_modal_overlay(self.kind) || self.kind == "PopupWindow" && self.boolean("modal", false)
    }

    pub fn stack_page_index(&self) -> Option<usize> {
        let page = self.stack.as_ref()?.current()?;
        self.children.iter().position(|node| node.id == Some(page))
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.properties.get(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn image(&self, name: &str) -> Option<Image> {
        match self.properties.get(name) {
            Some(Value::Image(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn image_fit(&self, name: &str) -> Option<ImageFit> {
        match self.properties.get(name) {
            Some(Value::ImageFit(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        match self.properties.get(name) {
            Some(Value::Color(value) | Value::Brush(Brush::Solid(value))) => Some(*value),
            _ => None,
        }
    }

    pub fn color_scheme(&self) -> Option<ColorScheme> {
        match self.properties.get("color-scheme") {
            Some(Value::ColorScheme(scheme)) => Some(*scheme),
            _ => None,
        }
    }

    pub fn at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut node = self;
        for &index in path {
            node = node.children.get(index)?;
        }
        Some(node)
    }

    pub fn interactive_at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut node = self;
        for index in path {
            if !node.is_visible() || !node.enables_children() || !node.group_content_enabled() {
                return None;
            }
            let child = node.children.get(*index)?;
            if node.kind == "SwipeDelegate"
                && matches!(child.kind, "SwipeLeft" | "SwipeRight")
                && node.swipe_side().panel() != Some(child.kind)
            {
                return None;
            }
            node = child;
        }
        (node.is_visible() && node.boolean("enabled", true)).then_some(node)
    }

    pub fn group_checkable(&self) -> bool {
        self.kind == "GroupBox" && self.boolean("checkable", false)
    }

    pub fn group_content_enabled(&self) -> bool {
        !self.group_checkable() || self.boolean("checked", true)
    }

    #[doc(hidden)]
    pub fn control_target(&self, path: &[usize], action: ControlAction) -> Option<&Self> {
        if action != ControlAction::Drawer(true) {
            return self.interactive_at_path(path);
        }
        let node = match path.split_last() {
            Some((&index, parent)) => {
                let parent = self.interactive_at_path(parent)?;
                if !parent.group_content_enabled() {
                    return None;
                }
                parent.children.get(index)?
            }
            None => self,
        };
        (node.kind == "Drawer" && node.boolean("visible", true) && node.boolean("enabled", true))
            .then_some(node)
    }

    /// Identity of the virtual row containing this scene path.
    pub fn row_key_at(&self, path: &[usize]) -> Option<RowKey> {
        let mut node = self;
        let mut key = None;
        for &index in path {
            node = node.children.get(index)?;
            if node.kind == "ListRow" {
                key = node.row_key;
            }
        }
        key
    }
}
