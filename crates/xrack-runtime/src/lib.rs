//! Renderer-independent scene snapshots produced by generated components.
mod animation;
mod calendar;
mod controls;
mod delegate;
mod double_spin;
mod list;
mod model;
mod property;
mod range;
mod repeat;
mod split;
mod stack;
mod table_selection;
mod tree;
pub use animation::SceneTransition;
pub use calendar::{CalendarAction, CalendarState, Date};
pub use controls::{
    ActionKey, ControlAction, DialogRole, SpinAction, SpinRange, TableHeader, TableSort,
    button_text, combo_editable, combo_index, group_index, group_item, is_button, is_modal_overlay,
    is_spin_box, page_indicator_count, page_indicator_index, resizable_table_column,
    resolve_swipe_index, search_suggestion, search_suggestions, spin_editable, spin_text,
    spin_value_after, text_input_value,
};
#[doc(hidden)]
pub use delegate::{ComponentRepeater, RowContext};
pub use double_spin::{
    DecimalSpinRange, SpinDecimal, decimal_separator, decimal_spin_value_after, spin_text_is_valid,
};
pub use list::{ListMetrics, RowKey};
#[doc(hidden)]
pub use list::{ListModel, RowLifetime};
pub use model::VecModel;
pub use property::{ChangeSignal, Property};
pub use range::{DialInputMode, RangeAction, SliderInterval, SliderRange, progress_value};
#[doc(hidden)]
pub use repeat::Repeater;
pub use stack::{StackNavigation, StackSnapshot, StackState};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
pub use table_selection::{
    RowSelection, SelectionTarget, TableSelection, TableSelectionAction, TableSelectionSnapshot,
};
pub use tree::{TreeAction, TreeItem, TreeSelectionAction, TreeSnapshot, TreeState};

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

    /// Render the requested model rows of a virtual view in a base scene.
    fn list_rows(&self, _path: &[usize], _rows: std::ops::Range<usize>) -> Vec<Node> {
        Vec::new()
    }

    fn dispatch(&mut self, element_id: &str, event: &str) -> bool;

    /// Commit the complete value of an editable text item.
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

#[doc(hidden)]
pub enum RoutedEvent<'a> {
    Dispatch(&'a str),
    Edit(&'a str),
    CommitCell { original: &'a str, text: &'a str },
    Range(RangeAction),
    Control(ControlAction),
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
    Length(Length),
    Percent(Percent),
    Color(Color),
    Brush(Brush),
    Image(Image),
    ImageFit(ImageFit),
    LayoutAlignment(LayoutAlignment),
    CrossAxisAlignment(CrossAxisAlignment),
    Orientation(Orientation),
    DialInputMode(DialInputMode),
    ScrollBarPolicy(ScrollBarPolicy),
    TextWrap(TextWrap),
    DialogRole(DialogRole),
    DrawerEdge(DrawerEdge),
    SelectionTarget(SelectionTarget),
    Date(Date),
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
value_from!(DialInputMode, DialInputMode);
value_from!(ScrollBarPolicy, ScrollBarPolicy);
value_from!(TextWrap, TextWrap);
value_from!(DialogRole, DialogRole);
value_from!(DrawerEdge, DrawerEdge);
value_from!(SelectionTarget, SelectionTarget);
value_from!(Date, Date);

impl From<Brush> for Value {
    fn from(value: Brush) -> Self {
        match value {
            Brush::Solid(color) => Self::Color(color),
            gradient => Self::Brush(gradient),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: &'static str,
    pub id: Option<&'static str>,
    pub properties: BTreeMap<&'static str, Value>,
    pub children: Vec<Node>,
    pub animations: BTreeMap<&'static str, u64>,
    pub list: Option<ListMetrics>,
    pub table_header: Option<Box<TableHeader>>,
    pub table_selection: Option<Box<TableSelectionSnapshot>>,
    pub tree: Option<Arc<TreeSnapshot>>,
    pub stack: Option<Arc<StackSnapshot>>,
    pub action: Option<ActionKey>,
    /// Selected TreeView business keys and model row identities, including folded descendants.
    pub tree_selection: Option<Arc<BTreeMap<i64, RowKey>>>,
    pub split_weights: Option<Arc<[Option<f32>]>>,
    pub row_key: Option<RowKey>,
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
    pub fn date(&self, name: &str) -> Option<Date> {
        match self.properties.get(name) {
            Some(Value::Date(date)) => Some(*date),
            _ => None,
        }
    }

    pub fn new(kind: &'static str, id: Option<&'static str>) -> Self {
        Self {
            kind,
            id,
            properties: BTreeMap::new(),
            children: Vec::new(),
            animations: BTreeMap::new(),
            list: None,
            table_header: None,
            table_selection: None,
            tree: None,
            stack: None,
            action: None,
            tree_selection: None,
            split_weights: None,
            row_key: None,
        }
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

    pub fn dialog_role(&self) -> DialogRole {
        match self.properties.get("role") {
            Some(Value::DialogRole(role)) => *role,
            _ => DialogRole::default(),
        }
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
            && (!is_modal_overlay(self.kind) || self.boolean("open", false))
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
            if !node.is_visible() || !node.boolean("enabled", true) {
                return None;
            }
            node = node.children.get(*index)?;
        }
        (node.is_visible() && node.boolean("enabled", true)).then_some(node)
    }

    #[doc(hidden)]
    pub fn control_target(&self, path: &[usize], action: ControlAction) -> Option<&Self> {
        if action != ControlAction::Drawer(true) {
            return self.interactive_at_path(path);
        }
        let node = match path.split_last() {
            Some((&index, parent)) => self.interactive_at_path(parent)?.children.get(index)?,
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
