//! Describe the visual bindings and control storage shared by both backends.
use xrack_mlir::{OperationRef, ValueRef, scene_children};
use xrack_runtime::{MediaCommand, is_text_input, is_toggle};
use xrack_syntax::ast::Type;

fn children<'m, 'n>(
    op: OperationRef<'m>,
    name: &'n str,
) -> impl Iterator<Item = OperationRef<'m>> + use<'m, 'n> {
    op.children().filter(move |child| child.name() == name)
}

/// Runtime media paths use the declaration's file, or the entry file when
/// standalone MLIR has no source locations.
pub fn media_source_file<'a>(op: OperationRef<'a>, entry: &'a str) -> &'a str {
    // MLIR gives an unnamed parser buffer the filename "-".
    op.source_file()
        .filter(|file| !matches!(*file, "" | "-"))
        .unwrap_or(entry)
}

/// A media effect whose seek position has yet to be evaluated.
#[derive(Clone)]
pub enum MediaEffect<Position> {
    Play,
    Pause,
    Stop,
    GoLive,
    Seek(Position),
}

impl<'m> MediaEffect<ValueRef<'m>> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        match (op.attr("method")?, op.operand(0)) {
            ("play", None) => Ok(Self::Play),
            ("pause", None) => Ok(Self::Pause),
            ("stop", None) => Ok(Self::Stop),
            ("go-live", None) => Ok(Self::GoLive),
            ("seek", Some(position)) => Ok(Self::Seek(position)),
            _ => Err("requires play(), pause(), stop(), go-live(), or seek(float)".into()),
        }
    }
}

impl<Position> MediaEffect<Position> {
    pub fn try_map_position<T, E>(
        self,
        convert: impl FnOnce(Position) -> Result<T, E>,
    ) -> Result<MediaEffect<T>, E> {
        Ok(match self {
            Self::Play => MediaEffect::Play,
            Self::Pause => MediaEffect::Pause,
            Self::Stop => MediaEffect::Stop,
            Self::GoLive => MediaEffect::GoLive,
            Self::Seek(position) => MediaEffect::Seek(convert(position)?),
        })
    }

    pub fn try_evaluate<E>(
        &self,
        evaluate: impl FnOnce(&Position) -> Result<f64, E>,
    ) -> Result<MediaCommand, E> {
        Ok(match self {
            Self::Play => MediaCommand::Play,
            Self::Pause => MediaCommand::Pause,
            Self::Stop => MediaCommand::Stop,
            Self::GoLive => MediaCommand::GoLive,
            Self::Seek(position) => MediaCommand::Seek(evaluate(position)?),
        })
    }
}

pub fn table_option_configured(element: OperationRef<'_>, name: &str) -> bool {
    element.children().any(|binding| {
        matches!(binding.name(), "xr.bind" | "xr.state_set")
            && binding.attribute("name") == Some(name)
    })
}

/// Editing history requires a writable string or record model.
pub fn table_history_item(table: OperationRef<'_>) -> Result<Option<Type>, String> {
    let kind = table.attribute("kind");
    if !matches!(kind, Some("TableView" | "StandardTableView"))
        || !(table_option_configured(table, "undoable")
            || table_option_configured(table, "pasteable"))
    {
        return Ok(None);
    }
    let model = if kind == Some("StandardTableView") {
        children(table, "xr.bind").find(|op| op.attribute("name") == Some("model"))
    } else {
        scene_children(table).find_map(|row| children(row, "xr.repeat").next())
    };
    let source = model
        .and_then(|op| op.operand(0))
        .ok_or("missing table model")?;
    Ok(match source.xr_type()? {
        Type::StringList => Some(Type::String),
        Type::RecordList(name) => Some(Type::Record(name)),
        _ => None,
    })
}

/// Visual bindings that can allocate cache slots in their scene scope.
pub fn indexed_visual_bindings(
    element: OperationRef<'_>,
) -> impl Iterator<Item = OperationRef<'_>> {
    element
        .children()
        .filter(move |binding| match binding.name() {
            "xr.show_if" | "xr.layout_bind" => true,
            "xr.bind" if element.name() == "xr.element" => {
                !matches!(binding.attr("name"), Ok("content-x" | "content-y"))
                    && !controlled_properties(element)
                        .any(|(name, _)| binding.attr("name") == Ok(name))
            }
            _ => false,
        })
}

pub fn picker_value(kind: &str) -> Option<(&'static str, Type)> {
    match kind {
        "TimePickerPopup" => Some(("time", Type::Time)),
        "DatePickerPopup" => Some(("date", Type::Date)),
        "ColorPickerPopup" => Some(("color", Type::Color)),
        "FontPickerPopup" => Some(("font", Type::Font)),
        _ => None,
    }
}

/// Return the scene node kind and whether it uses application window layout.
pub fn scene_element(kind: &str) -> (&str, bool) {
    match kind {
        "ApplicationWindow" => ("Window", true),
        "StandardTableView" => ("TableView", false),
        "StandardButton" => ("Button", false),
        "DayCell" | "SplitHandle" => ("Rectangle", false),
        kind if picker_value(kind).is_some() => ("Dialog", false),
        kind => (kind, false),
    }
}

pub fn text_item(element: OperationRef<'_>) -> bool {
    element.attribute("kind").is_some_and(is_text_input)
}

pub fn combo_editor(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("ComboBox")
        && (children(element, "xr.bind")
            .any(|binding| matches!(binding.attr("name"), Ok("editable" | "edit-text")))
            || control_link(element, "edit-text").is_some())
}

pub fn default_combo_edit_text(element: OperationRef<'_>) -> bool {
    control_link(element, "edit-text").is_none()
        && !children(element, "xr.bind").any(|binding| binding.attr("name") == Ok("edit-text"))
}

pub fn check_state_control(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("CheckBox")
        && element.children().any(|binding| {
            matches!(binding.name(), "xr.bind" | "xr.control_link")
                && matches!(binding.attr("name"), Ok("check-state" | "tristate"))
        })
}

pub fn check_state_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("check-state"))
}

pub fn controlled_property(element: OperationRef<'_>) -> Option<(&'static str, Type)> {
    if element.attr("kind") == Ok("ActionGroup") {
        return (!non_exclusive_group(element)).then_some(("current-value", Type::String));
    }
    if children(element, "xr.row_link").next().is_some()
        || action_ref(element).is_some()
        || non_exclusive_button_group(element)
    {
        None
    } else if text_item(element) {
        Some(("text", Type::String))
    } else if element.attr("kind") == Ok("SecondaryWindow") {
        Some(("visible", Type::Bool))
    } else if check_state_control(element) {
        check_state_binding(element)
            .is_none()
            .then_some(("check-state", Type::CheckState))
    } else if element.attr("kind").is_ok_and(is_toggle)
        || matches!(
            element.attr("kind"),
            Ok("MenuItem" | "Action" | "DelayButton" | "GroupBox")
        )
    {
        Some(("checked", Type::Bool))
    } else if matches!(
        element.attr("kind"),
        Ok("Dialog" | "Drawer" | "PopupWindow")
    ) || element.attribute("kind").and_then(picker_value).is_some()
    {
        Some(("open", Type::Bool))
    } else if element.attr("kind") == Ok("TimePicker") {
        Some(("time", Type::Time))
    } else if element.attr("kind") == Ok("Timer") {
        Some(("running", Type::Bool))
    } else if matches!(
        element.attr("kind"),
        Ok("Slider" | "Dial" | "DoubleSpinBox")
    ) {
        Some(("value", Type::Float))
    } else if element.attr("kind") == Ok("ScrollBar")
        && children(element, "xr.scroll_target").next().is_none()
    {
        Some(("position", Type::Float))
    } else if element.attr("kind") == Ok("SpinBox") {
        Some(("value", Type::Int))
    } else if element.attr("kind") == Ok("SwipeDelegate") {
        Some(("open-side", Type::SwipeSide))
    } else if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView")) {
        Some(("current-row", Type::Int))
    } else if element.attr("kind") == Ok("TreeView") {
        Some(("current-key", Type::Int))
    } else if matches!(element.attr("kind"), Ok("PageIndicator" | "Tumbler")) {
        Some(("current-index", Type::Int))
    } else if matches!(element.attr("kind"), Ok("Calendar" | "MonthGrid")) {
        Some(("date", Type::Date))
    } else if matches!(
        element.attr("kind"),
        Ok("ComboBox"
            | "RadioGroup"
            | "ButtonGroup"
            | "TabWidget"
            | "TabBar"
            | "SwipeView"
            | "ListView"
            | "GridView"
            | "StandardListView")
    ) {
        if let Some(binding) = element.children().find(|binding| {
            matches!(binding.name(), "xr.bind" | "xr.control_link")
                && binding.attr("name") == Ok("current-value")
        }) {
            Some((
                "current-value",
                operand_type(binding).expect("verified choice value type"),
            ))
        } else {
            Some(("current-index", Type::Int))
        }
    } else {
        None
    }
}

pub fn controlled_properties(
    element: OperationRef<'_>,
) -> impl Iterator<Item = (&'static str, Type)> {
    let properties = if element.attr("kind") == Ok("RangeSlider") {
        [
            Some(("lower-value", Type::Float)),
            Some(("upper-value", Type::Float)),
        ]
    } else if let Some(value) = element.attribute("kind").and_then(picker_value) {
        [controlled_property(element), Some(value)]
    } else if matches!(element.attr("kind"), Ok("Calendar" | "MonthGrid")) {
        [
            controlled_property(element),
            Some(("display-month", Type::Date)),
        ]
    } else if combo_editor(element) {
        [
            controlled_property(element),
            Some(("edit-text", Type::String)),
        ]
    } else {
        [controlled_property(element), None]
    };
    properties.into_iter().flatten()
}

pub enum ControlDefault<T> {
    Value(T),
    Binding(&'static str, T),
    Control(&'static str),
}

pub enum ControlLiteral {
    TypeDefault,
    Literal(&'static str),
}

/// Describe the initial value of control storage without evaluating its bindings.
pub fn control_default(
    kind: &str,
    property: &str,
    ty: &Type,
) -> Option<ControlDefault<ControlLiteral>> {
    use ControlDefault::{Binding, Control, Value};
    use ControlLiteral::{Literal, TypeDefault};

    Some(match ty {
        Type::Date if property == "display-month" => Control("date"),
        Type::Date | Type::Time | Type::Font => Value(TypeDefault),
        Type::Color => Value(Literal("#ffffffff")),
        Type::CheckState => Value(Literal("unchecked")),
        Type::SwipeSide => Value(Literal("closed")),
        Type::String => Value(Literal("\"\"")),
        Type::Bool => Value(Literal(if kind == "GroupBox" { "true" } else { "false" })),
        Type::Float if property == "position" => Value(Literal("0")),
        Type::Float if property == "upper-value" => Binding("maximum", Literal("100")),
        Type::Float => Binding("minimum", Literal("0")),
        Type::Int
            if matches!(
                kind,
                "RadioGroup"
                    | "ButtonGroup"
                    | "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TreeView"
            ) =>
        {
            Value(Literal("-1"))
        }
        Type::Int => Binding("minimum", Literal("0")),
        _ => return None,
    })
}

pub fn operand_type(binding: OperationRef<'_>) -> Option<Type> {
    binding.operand(0)?.xr_type().ok()
}

pub fn control_link<'a>(element: OperationRef<'a>, property: &str) -> Option<OperationRef<'a>> {
    children(element, "xr.control_link").find(|link| link.attr("name") == Ok(property))
}

pub fn action_ref(element: OperationRef<'_>) -> Option<&str> {
    children(element, "xr.action_ref")
        .next()?
        .attribute("owner")
}

pub fn non_exclusive_button_group(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("ButtonGroup") && non_exclusive_group(element)
}

pub fn non_exclusive_group(element: OperationRef<'_>) -> bool {
    children(element, "xr.bind")
        .find(|bind| bind.attr("name") == Ok("exclusive"))
        .and_then(|bind| bind.operand(0)?.defining_operation())
        .is_some_and(|literal| {
            literal.name() == "xr.literal" && literal.attr("value") == Ok("false")
        })
}
