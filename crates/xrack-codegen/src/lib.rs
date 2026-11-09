//! Rust token generation from the typed UI operations exported from MLIR.
mod scene_plan;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};
pub use scene_plan::EmbeddedPath;
use std::collections::{BTreeMap, HashMap};
use xrack_ir::semantic::rust_name;
use xrack_mlir::{Module, OperationRef, ValueRef};
use xrack_runtime::{ImageFormat, is_multiline_text_input, is_text_input, is_toggle};
use xrack_syntax::ast::{Parameter, Type};

fn ident(s: &str) -> Ident {
    let s = rust_name(s);
    if [
        "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
        "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "static", "struct", "trait", "true", "type", "unsafe",
        "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override",
        "priv", "try", "typeof", "unsized", "virtual", "yield",
    ]
    .contains(&s.as_str())
    {
        Ident::new_raw(&s, Span::call_site())
    } else {
        Ident::new(&s, Span::call_site())
    }
}

fn ty(t: &Type) -> TokenStream {
    match t {
        Type::Int => quote!(::core::primitive::i64),
        Type::Float => quote!(::core::primitive::f64),
        Type::Bool => quote!(::core::primitive::bool),
        Type::String => quote!(::std::string::String),
        Type::StringList => quote!(::xrack_runtime::VecModel<::std::string::String>),
        Type::Record(name) => {
            let name = ident(name);
            quote!(#name)
        }
        Type::RecordList(name) => {
            let name = ident(name);
            quote!(::xrack_runtime::VecModel<#name>)
        }
        Type::Length => quote!(::xrack_runtime::Length),
        Type::Percent => quote!(::xrack_runtime::Percent),
        Type::Color => quote!(::xrack_runtime::Color),
        Type::Brush => quote!(::xrack_runtime::Brush),
        Type::Image => quote!(::xrack_runtime::Image),
        Type::ImageFit => quote!(::xrack_runtime::ImageFit),
        Type::Orientation => quote!(::xrack_runtime::Orientation),
        Type::FlexboxLayoutDirection => quote!(::xrack_runtime::FlexboxLayoutDirection),
        Type::FlexboxLayoutWrap => quote!(::xrack_runtime::FlexboxLayoutWrap),
        Type::DialInputMode => quote!(::xrack_runtime::DialInputMode),
        Type::ScrollBarPolicy => quote!(::xrack_runtime::ScrollBarPolicy),
        Type::TextWrap => quote!(::xrack_runtime::TextWrap),
        Type::TextHorizontalAlignment => quote!(::xrack_runtime::TextHorizontalAlignment),
        Type::TextVerticalAlignment => quote!(::xrack_runtime::TextVerticalAlignment),
        Type::TextOverflow => quote!(::xrack_runtime::TextOverflow),
        Type::InputType => quote!(::xrack_runtime::InputType),
        Type::ColorScheme => quote!(::xrack_runtime::ColorScheme),
        Type::FileDialogMode => quote!(::xrack_runtime::FileDialogMode),
        Type::MessageLevel => quote!(::xrack_runtime::MessageLevel),
        Type::MessageButtons => quote!(::xrack_runtime::MessageButtons),
        Type::CheckState => quote!(::xrack_runtime::CheckState),
        Type::DialogRole => quote!(::xrack_runtime::DialogRole),
        Type::StandardButtonKind => quote!(::xrack_runtime::StandardButtonKind),
        Type::SwipeSide => quote!(::xrack_runtime::SwipeSide),
        Type::DragAction => quote!(::xrack_runtime::DragAction),
        Type::DrawerEdge => quote!(::xrack_runtime::DrawerEdge),
        Type::PopupPlacement => quote!(::xrack_runtime::PopupPlacement),
        Type::SelectionTarget => quote!(::xrack_runtime::SelectionTarget),
        Type::Date => quote!(::xrack_runtime::Date),
        Type::Time => quote!(::xrack_runtime::Time),
        Type::LayoutAlignment => quote!(::xrack_runtime::LayoutAlignment),
        Type::CrossAxisAlignment => quote!(::xrack_runtime::CrossAxisAlignment),
        Type::GradientStop => quote!(::xrack_runtime::GradientStop),
    }
}

fn host_ty(t: &Type) -> TokenStream {
    if let Some(item) = t.list_item() {
        let item = ty(&item);
        quote!(::std::vec::Vec<#item>)
    } else {
        ty(t)
    }
}

fn to_host(value: TokenStream, t: &Type) -> TokenStream {
    if t.list_item().is_some() {
        quote!((#value).snapshot())
    } else {
        value
    }
}

fn from_host(value: TokenStream, t: &Type) -> TokenStream {
    if t.list_item().is_some() {
        quote!(::xrack_runtime::VecModel::from(#value))
    } else {
        value
    }
}

fn color(s: &str) -> u32 {
    let digits = &s[1..];
    let mut s = if digits.len() <= 4 {
        digits.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        digits.into()
    };
    if s.len() == 6 {
        s.push_str("ff");
    }
    u32::from_str_radix(&s, 16).expect("validated color")
}

fn children<'m, 'n>(
    op: OperationRef<'m>,
    name: &'n str,
) -> impl Iterator<Item = OperationRef<'m>> + use<'m, 'n> {
    op.children().filter(move |child| child.name() == name)
}

fn value_type(op: OperationRef<'_>) -> Result<Type, String> {
    if op.result_count() != 1 {
        return Err(format!("{} must produce one XR value", op.name()));
    }
    let spelling = op.result(0).expect("one result").type_name();
    spelling
        .strip_prefix("!xr.")
        .and_then(Type::parse)
        .ok_or_else(|| format!("{} has unsupported result type {spelling}", op.name()))
}

fn literal(value: &str, ty: Type) -> Result<TokenStream, String> {
    Ok(match ty {
        Type::Int => {
            let number = value.parse::<i64>().map_err(|e| e.to_string())?;
            let number = Literal::i64_suffixed(number);
            quote!(#number)
        }
        Type::Float | Type::Length | Type::Percent => {
            let number = value.parse::<f64>().map_err(|e| e.to_string())?;
            let number = Literal::f64_suffixed(number);
            if ty == Type::Length {
                quote!(::xrack_runtime::Length(#number))
            } else if ty == Type::Percent {
                quote!(::xrack_runtime::Percent(#number / 100.0))
            } else {
                quote!(#number)
            }
        }
        Type::Bool => {
            let value = value.parse::<bool>().map_err(|e| e.to_string())?;
            quote!(#value)
        }
        Type::String => {
            let value: String = serde_json::from_str(value).map_err(|e| e.to_string())?;
            quote!(::std::string::String::from(#value))
        }
        Type::Color => {
            let rgba = Literal::u32_suffixed(color(value));
            quote!(::xrack_runtime::Color(#rgba))
        }
        Type::ImageFit
        | Type::LayoutAlignment
        | Type::CrossAxisAlignment
        | Type::Orientation
        | Type::FlexboxLayoutDirection
        | Type::FlexboxLayoutWrap
        | Type::DialInputMode
        | Type::ScrollBarPolicy
        | Type::TextWrap
        | Type::TextHorizontalAlignment
        | Type::TextVerticalAlignment
        | Type::TextOverflow
        | Type::InputType
        | Type::ColorScheme
        | Type::FileDialogMode
        | Type::MessageLevel
        | Type::MessageButtons
        | Type::CheckState
        | Type::DialogRole
        | Type::StandardButtonKind
        | Type::SwipeSide
        | Type::DragAction
        | Type::DrawerEdge
        | Type::PopupPlacement
        | Type::SelectionTarget => {
            if !ty.enum_variants().contains(&value) {
                return Err(format!("unknown {} value {value}", ty.name()));
            }
            let variant = value
                .split('-')
                .map(|word| word[..1].to_ascii_uppercase() + &word[1..])
                .collect::<String>();
            let variant = format_ident!("{variant}");
            let enum_type = self::ty(&ty);
            quote!(#enum_type::#variant)
        }
        Type::Brush
        | Type::GradientStop
        | Type::Image
        | Type::Date
        | Type::Time
        | Type::StringList
        | Type::Record(_)
        | Type::RecordList(_) => {
            return Err(format!("{} does not have literal syntax", ty.name()));
        }
    })
}

fn binary_operator(value: &str) -> Result<TokenStream, String> {
    Ok(match value {
        "+" => quote!(+),
        "-" => quote!(-),
        "*" => quote!(*),
        "/" => quote!(/),
        "%" => quote!(%),
        "==" => quote!(==),
        "!=" => quote!(!=),
        "<" => quote!(<),
        "<=" => quote!(<=),
        ">" => quote!(>),
        ">=" => quote!(>=),
        "&&" => quote!(&&),
        "||" => quote!(||),
        _ => return Err(format!("unsupported binary operator {value}")),
    })
}

fn owner<'m>(op: OperationRef<'m>) -> &'m str {
    op.attribute("owner").unwrap_or("root")
}

fn element_key<'m>(op: OperationRef<'m>) -> Result<&'m str, String> {
    match op.attribute("key") {
        Some(key) => Ok(key),
        None => op.attr("id"),
    }
}

fn rust_key(key: &str) -> String {
    // Preserve the distinction between element IDs like left-side and left_side.
    key.replace('_', "__").replace('-', "_h")
}

fn control_field(key: &str, property: &str) -> Ident {
    format_ident!("__control_{}_{}", rust_key(key), rust_key(property))
}

fn timer_member(key: &str, member: &str) -> Ident {
    format_ident!("__timer_{}_{}", member, rust_key(key))
}

fn native_dialog_member(key: &str, member: &str) -> Ident {
    format_ident!("__native_dialog_{}_{}", member, rust_key(key))
}

fn native_dialog(element: OperationRef<'_>) -> bool {
    matches!(element.attr("kind"), Ok("FileDialog" | "MessageDialog"))
}

fn secondary_window_member(key: &str, member: &str) -> Ident {
    format_ident!("__secondary_window_{}_{}", member, rust_key(key))
}

fn popup_method(key: &str) -> Ident {
    format_ident!("__popup_{}", rust_key(key))
}

fn picker_field(key: &str) -> Ident {
    format_ident!("__picker_{}", rust_key(key))
}

fn picker_value(kind: &str) -> Option<(&'static str, Type)> {
    match kind {
        "TimePickerPopup" => Some(("time", Type::Time)),
        "DatePickerPopup" => Some(("date", Type::Date)),
        "ColorPickerPopup" => Some(("color", Type::Color)),
        _ => None,
    }
}

fn table_sort_field(key: &str) -> Ident {
    format_ident!("__table_sort_{}", rust_key(key))
}

fn table_widths_field(key: &str) -> Ident {
    format_ident!("__table_widths_{}", rust_key(key))
}

fn table_selection_field(key: &str) -> Ident {
    format_ident!("__table_selection_{}", rust_key(key))
}

fn split_weights_field(key: &str) -> Ident {
    format_ident!("__split_weights_{}", rust_key(key))
}

fn tree_state_field(key: &str) -> Ident {
    format_ident!("__tree_state_{}", rust_key(key))
}

fn visual_field(value: usize) -> Ident {
    format_ident!("__visual_{value}")
}

fn repeat_field(index: usize) -> Ident {
    format_ident!("__repeat_{index}")
}

fn row_local(index: usize) -> Ident {
    format_ident!("__xr_row_{index}")
}

fn text_item(element: OperationRef<'_>) -> bool {
    element.attribute("kind").is_some_and(is_text_input)
}

fn combo_editor(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("ComboBox")
        && (children(element, "xr.bind")
            .any(|binding| matches!(binding.attr("name"), Ok("editable" | "edit-text")))
            || control_link(element, "edit-text").is_some())
}

fn default_combo_edit_text(element: OperationRef<'_>) -> bool {
    control_link(element, "edit-text").is_none()
        && !children(element, "xr.bind").any(|binding| binding.attr("name") == Ok("edit-text"))
}

fn check_state_control(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("CheckBox")
        && element.children().any(|binding| {
            matches!(binding.name(), "xr.bind" | "xr.control_link")
                && matches!(binding.attr("name"), Ok("check-state" | "tristate"))
        })
}

fn check_state_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("check-state"))
}

fn controlled_property(element: OperationRef<'_>) -> Option<(&'static str, Type)> {
    if children(element, "xr.row_link").next().is_some() || action_ref(element).is_some() {
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
    } else if element.attr("kind") == Ok("Calendar") {
        Some(("date", Type::Date))
    } else if matches!(
        element.attr("kind"),
        Ok("ComboBox"
            | "RadioGroup"
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

fn controlled_properties(element: OperationRef<'_>) -> impl Iterator<Item = (&'static str, Type)> {
    let properties = if element.attr("kind") == Ok("RangeSlider") {
        [
            Some(("lower-value", Type::Float)),
            Some(("upper-value", Type::Float)),
        ]
    } else if let Some(value) = element.attribute("kind").and_then(picker_value) {
        [controlled_property(element), Some(value)]
    } else if element.attr("kind") == Ok("Calendar") {
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

fn model_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("model"))
}

fn operand_type(binding: OperationRef<'_>) -> Option<Type> {
    let name = binding.operand(0)?.type_name();
    Type::parse(name.strip_prefix("!xr.")?)
}

fn record_combo_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    (element.attr("kind") == Ok("ComboBox"))
        .then(|| model_binding(element))
        .flatten()
        .filter(|binding| matches!(operand_type(*binding), Some(Type::RecordList(_))))
}

fn combo_field(key: &str, name: &str) -> Ident {
    format_ident!("__combo_{}_{}", name, rust_key(key))
}

fn table_columns(element: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    scene_children(element).filter(|column| column.attr("kind") == Ok("TableColumn"))
}

fn control_link<'a>(element: OperationRef<'a>, property: &str) -> Option<OperationRef<'a>> {
    children(element, "xr.control_link").find(|link| link.attr("name") == Ok(property))
}

fn typed_events(element: OperationRef<'_>) -> bool {
    matches!(
        element.attr("kind"),
        Ok("Slider"
            | "Dial"
            | "RangeSlider"
            | "SpinBox"
            | "DoubleSpinBox"
            | "SearchField"
            | "ComboBox"
            | "RadioGroup"
            | "TabWidget"
            | "TabBar"
            | "SwipeView"
            | "SwipeDelegate"
            | "StackView"
            | "PageIndicator"
            | "Tumbler"
            | "Calendar"
            | "TimePicker"
            | "TimePickerPopup"
            | "DatePickerPopup"
            | "ColorPickerPopup"
            | "ListView"
            | "GridView"
            | "StandardListView"
            | "TableView"
            | "StandardTableView"
            | "SplitView"
            | "TreeView"
            | "Dialog"
            | "Drawer"
            | "PopupWindow"
            | "MenuItem"
            | "Shortcut"
            | "DelayButton"
            | "DragArea"
            | "DropArea"
            | "GroupBox")
    ) || element.attr("kind").is_ok_and(is_multiline_text_input)
}

fn instance_field(key: &str) -> Ident {
    format_ident!("__instance_{}", rust_key(key))
}

fn action_ref(element: OperationRef<'_>) -> Option<&str> {
    children(element, "xr.action_ref")
        .next()?
        .attribute("owner")
}

fn action_key_field(key: &str) -> Ident {
    format_ident!("__action_key_{}", rust_key(key))
}

fn action_render_method(key: &str) -> Ident {
    format_ident!("__render_action_{}", rust_key(key))
}

fn action_activate_method(key: &str) -> Ident {
    format_ident!("__activate_action_{}", rust_key(key))
}

fn stack_field(key: &str) -> Ident {
    format_ident!("__stack_{}", rust_key(key))
}

fn stack_snapshot_method(key: &str) -> Ident {
    format_ident!("__stack_snapshot_{}", rust_key(key))
}

fn stack_navigate_method(key: &str) -> Ident {
    format_ident!("__stack_navigate_{}", rust_key(key))
}

fn property_field(owner: &str, name: &str) -> Ident {
    format_ident!(
        "__p_{}_{}_{}",
        owner.len(),
        rust_key(owner),
        rust_name(name)
    )
}

fn getter(owner: &str, name: &str, value_type: &Type) -> Ident {
    if owner == "root" {
        let prefix = if value_type.list_item().is_some() {
            "model"
        } else {
            "get"
        };
        format_ident!("{prefix}_{}", rust_name(name))
    } else {
        format_ident!(
            "__get_{}_{}_{}",
            owner.len(),
            rust_key(owner),
            rust_name(name)
        )
    }
}

fn element_children(op: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    op.children()
        .filter(|op| matches!(op.name(), "xr.element" | "xr.instance"))
}

fn scene_children(op: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    element_children(op).filter(|op| {
        !matches!(op.attr("kind"), Ok("Action" | "Timer" | "SecondaryWindow"))
            && !native_dialog(*op)
    })
}

fn static_children(op: OperationRef<'_>) -> impl Iterator<Item = (usize, OperationRef<'_>)> {
    // The first two RangeSlider children are the generated handles.
    let first = if op.attr("kind") == Ok("RangeSlider") {
        2
    } else {
        0
    };
    scene_children(op)
        .enumerate()
        .map(move |(index, child)| (first + index, child))
        .filter(|(_, child)| children(*child, "xr.repeat").next().is_none())
}

fn row_editor_paths<'a>(
    row: OperationRef<'a>,
    path: &mut Vec<usize>,
    editors: &mut Vec<(Vec<usize>, OperationRef<'a>)>,
) {
    for (index, child) in static_children(row) {
        path.push(index);
        if let Some(link) = children(child, "xr.row_link").next() {
            editors.push((path.clone(), link));
        } else {
            row_editor_paths(child, path, editors);
        }
        path.pop();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum VisualCacheKey<'a> {
    Operand(ValueRef<'a>),
    Group(&'a str),
    RepeatInvariant(&'a str),
    RepeatSubexpression(&'a str),
}

#[derive(Default)]
struct VisualScope<'a> {
    row_index: Option<usize>,
    bindings: HashMap<VisualCacheKey<'a>, usize>,
    binding_types: Vec<Type>,
    repeats: BTreeMap<&'a str, RepeatScope<'a>>,
}

struct RepeatScope<'a> {
    model: Type,
    row: VisualScope<'a>,
    component: Option<&'a str>,
}

impl RepeatScope<'_> {
    fn index(&self) -> usize {
        self.row.row_index.expect("repeat row index")
    }
}

#[derive(Clone, Copy)]
struct ExpressionContext<'a> {
    receiver: &'a TokenStream,
    repeat_parameters: bool,
    hoist_repeat_subexpressions: bool,
}

impl<'a> ExpressionContext<'a> {
    fn new(receiver: &'a TokenStream) -> Self {
        Self {
            receiver,
            repeat_parameters: false,
            hoist_repeat_subexpressions: false,
        }
    }
}

impl<'a> VisualScope<'a> {
    fn index_value(
        &mut self,
        key: VisualCacheKey<'a>,
        definition: OperationRef<'a>,
    ) -> Result<(), String> {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.bindings.entry(key) {
            entry.insert(self.binding_types.len());
            self.binding_types.push(value_type(definition)?);
        }
        Ok(())
    }

    fn receiver(&self) -> TokenStream {
        if let Some(index) = self.row_index {
            let row = row_local(index);
            quote!(#row)
        } else {
            quote!(self.__state.__visuals)
        }
    }

    fn state_name(&self, component: &str) -> Ident {
        if let Some(index) = self.row_index {
            format_ident!("__RepeatRow_{component}_{index}")
        } else {
            format_ident!("__Visuals_{component}")
        }
    }

    fn declare(&self, component: &str, declarations: &mut TokenStream) {
        let name = self.state_name(component);
        let mut fields = TokenStream::new();
        for (id, value_type) in self.binding_types.iter().enumerate() {
            let field = visual_field(id);
            let rust_type = ty(value_type);
            fields.extend(quote!(#field: ::xrack_runtime::Property<#rust_type>,));
        }
        for repeat in self.repeats.values() {
            let row = &repeat.row;
            row.declare(component, declarations);
            let field = repeat_field(repeat.index());
            let row_type = row.state_name(component);
            let model_type = ty(&repeat.model);
            if let Some(component) = repeat.component {
                let component = ident(component);
                fields.extend(quote!(#field: ::xrack_runtime::ComponentRepeater<#model_type, #row_type, #component>,));
            } else {
                fields.extend(quote!(#field: ::xrack_runtime::Repeater<#model_type, #row_type>,));
            }
        }
        declarations.extend(quote! {
            #[allow(non_camel_case_types)]
            #[derive(::core::default::Default)]
            struct #name { #fields }
        });
    }
}

struct Generator<'a> {
    records: &'a BTreeMap<&'a str, Vec<Parameter>>,
    instances: BTreeMap<&'a str, OperationRef<'a>>,
    element_properties: Vec<(&'a str, OperationRef<'a>)>,
    unlinked_controls: Vec<(&'a str, &'static str, Type)>,
    record_combos: Vec<&'a str>,
    picker_popups: Vec<(&'a str, Type)>,
    tables: Vec<&'a str>,
    splits: Vec<&'a str>,
    trees: Vec<&'a str>,
    stacks: BTreeMap<&'a str, OperationRef<'a>>,
    actions: BTreeMap<&'a str, OperationRef<'a>>,
    timers: Vec<OperationRef<'a>>,
    native_dialogs: Vec<OperationRef<'a>>,
    secondary_windows: Vec<OperationRef<'a>>,
    popups: Vec<OperationRef<'a>>,
    visuals: VisualScope<'a>,
    repeat_invariants: Vec<OperationRef<'a>>,
    repeat_subexpressions: Vec<(&'a str, OperationRef<'a>)>,
    use_visual_plan: bool,
    images: &'a BTreeMap<&'a str, Ident>,
    paths: &'a BTreeMap<&'a str, Ident>,
    with_fonts: bool,
}

impl<'a> Generator<'a> {
    fn index(&mut self, op: OperationRef<'a>) -> Result<(), String> {
        match op.name() {
            "xr.instance" if children(op, "xr.repeat").next().is_none() => {
                self.instances.insert(element_key(op)?, op);
            }
            "xr.element" => {
                let key = element_key(op)?;
                if record_combo_binding(op).is_some() {
                    self.record_combos.push(key);
                }
                if let Some((_, ty)) = op.attribute("kind").and_then(picker_value) {
                    self.picker_popups.push((key, ty));
                }
                if matches!(op.attr("kind"), Ok("TableView" | "StandardTableView")) {
                    self.tables.push(key);
                }
                if op.attr("kind") == Ok("SplitView") {
                    self.splits.push(key);
                }
                if op.attr("kind") == Ok("TreeView") {
                    self.trees.push(key);
                }
                if op.attr("kind") == Ok("StackView") {
                    self.stacks.insert(key, op);
                }
                if op.attr("kind") == Ok("Action") {
                    self.actions.insert(key, op);
                }
                if op.attr("kind") == Ok("Timer") {
                    self.timers.push(op);
                }
                if native_dialog(op) {
                    self.native_dialogs.push(op);
                }
                if op.attr("kind") == Ok("SecondaryWindow") {
                    self.secondary_windows.push(op);
                }
                if op.attr("kind") == Ok("PopupWindow") {
                    self.popups.push(op);
                }
                for (property, ty) in controlled_properties(op) {
                    if control_link(op, property).is_none() {
                        self.unlinked_controls.push((key, property, ty));
                    }
                }
                self.element_properties
                    .extend(children(op, "xr.property").map(|p| (key, p)));
            }
            _ => {}
        }
        for child in op.children() {
            match child.name() {
                "xr.element" | "xr.instance" => self.index(child)?,
                "xr.bind" | "xr.layout_bind" | "xr.show_if"
                    if self.use_visual_plan
                        && child.attribute("xr.repeat_invariant_group").is_some() =>
                {
                    self.repeat_invariants.push(child);
                }
                _ if self.use_visual_plan => {
                    if let Some(group) = child.attribute("xr.repeat_subexpression_group") {
                        self.repeat_subexpressions.push((group, child));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn operand(&self, op: OperationRef<'a>, index: usize) -> Result<OperationRef<'a>, String> {
        let value = op
            .operand(index)
            .ok_or_else(|| format!("{} missing operand {index}", op.name()))?;
        value
            .defining_operation()
            .ok_or_else(|| format!("{} operand {index} is a block argument", op.name()))
    }

    fn index_visual_bindings(
        &self,
        element: OperationRef<'a>,
        scope: &mut VisualScope<'a>,
        next: &mut usize,
    ) -> Result<(), String> {
        if self.static_subtree(element) {
            return Ok(());
        }
        if let Some(repeat) = children(element, "xr.repeat").next() {
            let model = value_type(self.operand(repeat, 0)?)?;
            if model != Type::Int && model.list_item().is_none() {
                return Err("repeat requires a supported model".into());
            }
            let key = element_key(element)?;
            let mut row = VisualScope {
                row_index: Some(*next),
                ..VisualScope::default()
            };
            *next += 1;
            self.index_visual_element(element, &mut row, next)?;
            let component = (element.name() == "xr.instance")
                .then(|| element.attr("component"))
                .transpose()?;
            scope.repeats.insert(
                key,
                RepeatScope {
                    model,
                    row,
                    component,
                },
            );
        } else {
            self.index_visual_element(element, scope, next)?;
        }
        Ok(())
    }

    fn index_visual_element(
        &self,
        element: OperationRef<'a>,
        scope: &mut VisualScope<'a>,
        next: &mut usize,
    ) -> Result<(), String> {
        for binding in element.children() {
            match binding.name() {
                "xr.show_if" | "xr.layout_bind" => {}
                "xr.bind" if element.name() == "xr.element" => {
                    if controlled_properties(element)
                        .any(|(name, _)| binding.attr("name") == Ok(name))
                    {
                        continue;
                    }
                }
                _ => continue,
            }
            self.index_visual_binding(binding, scope)?;
        }
        for child in scene_children(element) {
            self.index_visual_bindings(child, scope, next)?;
        }
        Ok(())
    }

    fn index_visual_binding(
        &self,
        binding: OperationRef<'a>,
        scope: &mut VisualScope<'a>,
    ) -> Result<(), String> {
        let definition = self.operand(binding, 0)?;
        if !self.static_gradient(definition)
            && !matches!(
                definition.name(),
                "xr.literal" | "xr.read" | "xr.image_url" | "xr.param"
            )
        {
            let key = self.visual_cache_key(binding);
            if matches!(key, VisualCacheKey::RepeatInvariant(_)) && scope.row_index.is_some() {
                return Ok(());
            }
            scope.index_value(key, definition)?;
        }
        Ok(())
    }

    fn visual_cache_key(&self, binding: OperationRef<'a>) -> VisualCacheKey<'a> {
        if self.use_visual_plan {
            if let Some(group) = binding.attribute("xr.repeat_invariant_group") {
                return VisualCacheKey::RepeatInvariant(group);
            }
            if let Some(group) = binding.attribute("xr.visual_cache_group") {
                return VisualCacheKey::Group(group);
            }
        }
        VisualCacheKey::Operand(binding.operand(0).expect("verified binding operand"))
    }

    fn visual_value(
        &self,
        binding: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let value = self.visual_expr(self.operand(binding, 0)?, scope)?;
        let key = self.visual_cache_key(binding);
        let cache = if matches!(key, VisualCacheKey::RepeatInvariant(_)) {
            &self.visuals
        } else {
            scope
        };
        if let Some(&id) = cache.bindings.get(&key) {
            let field = visual_field(id);
            let receiver = cache.receiver();
            Ok(quote!(#receiver.#field.get(|| #value)))
        } else {
            Ok(value)
        }
    }

    // Emit at the use site so bindings stay lazy and reads in each assignment
    // observe the mutations made by preceding statements.
    fn expr(&self, op: OperationRef<'a>) -> Result<TokenStream, String> {
        self.expr_in(op, ExpressionContext::new(&quote!(self)))
    }

    fn visual_expr(
        &self,
        op: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        self.expr_in(
            op,
            ExpressionContext {
                receiver: &quote!(self),
                repeat_parameters: scope.row_index.is_some(),
                hoist_repeat_subexpressions: scope.row_index.is_some(),
            },
        )
    }

    fn expr_in(
        &self,
        op: OperationRef<'a>,
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        if self.use_visual_plan
            && context.hoist_repeat_subexpressions
            && let Some(group) = op.attribute("xr.repeat_subexpression_group")
            && let Some(&id) = self
                .visuals
                .bindings
                .get(&VisualCacheKey::RepeatSubexpression(group))
        {
            let value = self.expr_in(
                op,
                ExpressionContext {
                    hoist_repeat_subexpressions: false,
                    ..context
                },
            )?;
            let field = visual_field(id);
            let receiver = self.visuals.receiver();
            return Ok(quote!(#receiver.#field.get(|| #value)));
        }
        let receiver = context.receiver;
        let value = op.attr("value")?;
        let arg = |i| self.expr_in(self.operand(op, i)?, context);
        Ok(match op.name() {
            "xr.literal" => literal(value, value_type(op)?)?,
            "xr.array" | "xr.record_array" => {
                let items = (0..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                quote!(::xrack_runtime::VecModel::from(::std::vec![#(#items),*]))
            }
            "xr.default_record" => {
                let record = ty(&value_type(op)?);
                quote!(<#record as ::core::default::Default>::default())
            }
            "xr.make_record" => {
                let Type::Record(name) = value_type(op)? else {
                    return Err("expected a record type".into());
                };
                let fields = self
                    .records
                    .get(name.as_ref())
                    .ok_or_else(|| format!("unknown record `{name}`"))?;
                let name = ident(&name);
                let values = (0..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                let names = fields.iter().map(|field| ident(&field.name));
                quote!(#name { #(#names: #values),* })
            }
            "xr.make_date" => {
                let (year, month, day) = (arg(0)?, arg(1)?, arg(2)?);
                quote!(::xrack_runtime::Date { year: #year, month: #month, day: #day })
            }
            "xr.make_time" => {
                let (hour, minute, second) = (arg(0)?, arg(1)?, arg(2)?);
                quote!((::xrack_runtime::Time { hour: #hour, minute: #minute, second: #second }).normalized())
            }
            "xr.get_field" | "xr.date_field" | "xr.time_field" => {
                let record = arg(0)?;
                let field = ident(value);
                quote!((#record).#field)
            }
            "xr.image_url" if value.is_empty() => quote!(::xrack_runtime::Image::Empty),
            "xr.image_url" => {
                let image = self
                    .images
                    .get(value)
                    .ok_or_else(|| format!("unresolved image {value:?}"))?;
                quote!(#image)
            }
            "xr.read" => {
                let owner = owner(op);
                if self.instances.contains_key(owner) {
                    let field = instance_field(owner);
                    let getter = getter("root", value, &value_type(op)?);
                    quote!(#receiver.__state.#field.#getter())
                } else {
                    let getter = getter(owner, value, &value_type(op)?);
                    quote!(#receiver.#getter())
                }
            }
            "xr.stack_read" => {
                let snapshot = stack_snapshot_method(owner(op));
                let snapshot = quote!(#receiver.#snapshot());
                match value {
                    "current-page" => quote!(#snapshot.current().unwrap_or("").to_owned()),
                    "depth" => quote!(#snapshot.depth() as ::core::primitive::i64),
                    "can-go-back" => quote!(#snapshot.depth() > 1),
                    _ => return Err("unknown StackView state property".into()),
                }
            }
            "xr.call" => from_host(self.callback_call(op, value, context)?, &value_type(op)?),
            "xr.param" => {
                let name = ident(value);
                if context.repeat_parameters {
                    quote!(#name())
                } else {
                    from_host(quote!(#name.clone()), &value_type(op)?)
                }
            }
            "xr.cast" => {
                let arg = arg(0)?;
                quote!((#arg as ::core::primitive::f64))
            }
            "xr.to_ratio" => {
                let value = arg(0)?;
                quote!((#value).0)
            }
            "xr.stringify" => {
                let arg = arg(0)?;
                quote!(::std::format!("{}", #arg))
            }
            "xr.solid_brush" => {
                let color = arg(0)?;
                quote!(::xrack_runtime::Brush::Solid(#color))
            }
            "xr.gradient_stop" => {
                let (color, offset) = (arg(0)?, arg(1)?);
                quote!(::xrack_runtime::GradientStop { color: #color, offset: #offset })
            }
            "xr.linear_gradient" | "xr.radial_gradient" => {
                let linear = op.name() == "xr.linear_gradient";
                let stops = (usize::from(linear)..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                let brush = if linear {
                    let angle = arg(0)?;
                    quote!(::xrack_runtime::Brush::Linear {
                        angle_degrees: #angle,
                        stops: ::std::sync::Arc::from([#(#stops),*]),
                    })
                } else {
                    quote!(::xrack_runtime::Brush::Radial {
                        stops: ::std::sync::Arc::from([#(#stops),*]),
                    })
                };
                if self.static_gradient(op) {
                    quote!({
                        static BRUSH: ::std::sync::OnceLock<::xrack_runtime::Brush> =
                            ::std::sync::OnceLock::new();
                        BRUSH.get_or_init(|| #brush).clone()
                    })
                } else {
                    brush
                }
            }
            "xr.unary" => {
                let arg = arg(0)?;
                match value {
                    "+" => arg,
                    "-" if value_type(op)? == Type::Length => {
                        quote!(::xrack_runtime::Length(-(#arg).0))
                    }
                    "-" => quote!((-#arg)),
                    "!" => quote!((!#arg)),
                    _ => return Err(format!("unsupported unary operator {value}")),
                }
            }
            "xr.binary" if value_type(op)? == Type::String => {
                let (left, right) = (arg(0)?, arg(1)?);
                quote!(::std::format!("{}{}", #left, #right))
            }
            "xr.binary" if value_type(self.operand(op, 0)?)?.list_item().is_some() => {
                let (left, right) = (arg(0)?, arg(1)?);
                let equal = quote!((#left).values_eq(&(#right)));
                match value {
                    "==" => equal,
                    "!=" => quote!(!#equal),
                    _ => return Err(format!("unsupported list operator {value}")),
                }
            }
            "xr.binary" => {
                let scalar = |index| {
                    let definition = self.operand(op, index)?;
                    let expression = self.expr_in(definition, context)?;
                    Ok::<_, String>(if value_type(definition)? == Type::Length {
                        quote!((#expression).0)
                    } else {
                        expression
                    })
                };
                let (left, right) = (scalar(0)?, scalar(1)?);
                let operator = binary_operator(value)?;
                let expression = quote!((#left #operator #right));
                if value_type(op)? == Type::Length {
                    quote!(::xrack_runtime::Length(#expression))
                } else {
                    expression
                }
            }
            "xr.select" => {
                let (condition, yes, no) = (arg(0)?, arg(1)?, arg(2)?);
                quote!((if #condition { #yes } else { #no }))
            }
            _ => return Err(format!("unsupported expression {}", op.name())),
        })
    }

    fn set_property(&self, owner: &str, name: &str, value: &TokenStream) -> TokenStream {
        if self.instances.contains_key(owner) {
            let instance = instance_field(owner);
            let field = property_field("root", name);
            quote!(self.__state.#instance.__state.#field.set_if_changed(#value);)
        } else {
            let field = property_field(owner, name);
            quote!(self.__state.#field.set_if_changed(#value);)
        }
    }

    fn callback_call(
        &self,
        op: OperationRef<'a>,
        name: &str,
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        let receiver = context.receiver;
        let method = format_ident!("__invoke_{}", rust_name(name));
        let args = (0..op.operand_count())
            .map(|index| {
                let operand = self.operand(op, index)?;
                Ok(to_host(
                    self.expr_in(operand, context)?,
                    &value_type(operand)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let owner = owner(op);
        if self.instances.contains_key(owner) {
            let instance = instance_field(owner);
            Ok(quote!(#receiver.__state.#instance.#method(#(#args),*)))
        } else {
            Ok(quote!(#receiver.#method(#(#args),*)))
        }
    }

    fn statements(&self, handler: OperationRef<'a>) -> Result<TokenStream, String> {
        let mut statements = TokenStream::new();
        for statement in handler.children() {
            match statement.name() {
                "xr.assign" => {
                    let value = self.expr(self.operand(statement, 0)?)?;
                    statements.extend(self.set_property(
                        owner(statement),
                        statement.attr("name")?,
                        &value,
                    ));
                }
                "xr.invoke" => {
                    let call = self.callback_call(
                        statement,
                        statement.attr("name")?,
                        ExpressionContext::new(&quote!(self)),
                    )?;
                    statements.extend(quote!(let _ = #call;));
                }
                "xr.navigate" => {
                    let method = stack_navigate_method(owner(statement));
                    let operation = statement.attr("method")?;
                    let action = if operation == "pop" {
                        quote!(::xrack_runtime::StackNavigation::Pop)
                    } else {
                        let variant = match operation {
                            "push" => format_ident!("Push"),
                            "replace" => format_ident!("Replace"),
                            "pop-to" => format_ident!("PopTo"),
                            "reset" => format_ident!("Reset"),
                            "push-pages" => format_ident!("PushPages"),
                            "reset-to" => format_ident!("ResetTo"),
                            _ => return Err("unknown StackView navigation method".into()),
                        };
                        let argument = self.expr(self.operand(statement, 0)?)?;
                        let argument = if matches!(operation, "push-pages" | "reset-to") {
                            quote!((#argument).snapshot())
                        } else {
                            argument
                        };
                        quote!(::xrack_runtime::StackNavigation::#variant(&(#argument)))
                    };
                    statements.extend(quote!(let _ = self.#method(None, #action);));
                }
                "xr.timer_command" => {
                    let method = timer_member(statement.attr("owner")?, "command");
                    let command = match statement.attr("command")? {
                        "start" => quote!(Start),
                        "stop" => quote!(Stop),
                        "restart" => quote!(Restart),
                        _ => return Err("unknown timer command".into()),
                    };
                    statements
                        .extend(quote!(self.#method(::xrack_runtime::TimerCommand::#command);));
                }
                "xr.popup_command" => {
                    let method = popup_method(statement.attr("owner")?);
                    let open = statement.attr("open")? == "true";
                    statements.extend(quote!(let _ = self.#method(#open);));
                }
                "xr.file_dialog_open" | "xr.native_dialog_open" => {
                    let open = native_dialog_member(statement.attr("owner")?, "open");
                    statements.extend(quote!(self.#open();));
                }
                "xr.window_close" => {
                    statements.extend(quote!(self.__state.__window_close.set(true);));
                }
                "xr.secondary_window_command" => {
                    let owner = statement.attr("owner")?;
                    match statement.attr("command")? {
                        "show" | "hide" => {
                            let method = secondary_window_member(owner, "visible");
                            let visible = statement.attr("command")? == "show";
                            statements.extend(quote!(self.#method(#visible);));
                        }
                        "close" => {
                            let method = secondary_window_member(owner, "close");
                            statements.extend(quote!(self.#method();));
                        }
                        _ => return Err("unknown secondary window command".into()),
                    }
                }
                _ => {}
            }
        }
        Ok(statements)
    }

    fn static_subtree(&self, element: OperationRef<'a>) -> bool {
        self.use_visual_plan && element.attribute("xr.static_subtree") == Some("true")
    }

    fn static_gradient(&self, expression: OperationRef<'a>) -> bool {
        self.use_visual_plan && expression.attribute("xr.static_gradient") == Some("true")
    }

    fn render(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
    ) -> Result<Ident, String> {
        if self.static_subtree(element) {
            let mut body = TokenStream::new();
            let name =
                self.render_element(element, &VisualScope::default(), &mut body, next, None)?;
            out.extend(quote! {
                let #name = {
                    ::std::thread_local! {
                        static CACHE: ::std::cell::OnceCell<::xrack_runtime::Node> =
                            const { ::std::cell::OnceCell::new() };
                    }
                    CACHE.with(|cache| cache.get_or_init(|| { #body #name.into_shared() }).clone())
                };
            });
            return Ok(name);
        }
        if let Some(model) = children(element, "xr.repeat").next() {
            let key = element_key(element)?;
            let repeat = &scope.repeats[key];
            let receiver = scope.receiver();
            let field = repeat_field(repeat.index());
            let expression = self.visual_expr(self.operand(model, 0)?, scope)?;
            let name = format_ident!("__xr_repeat_{}", *next);
            *next += 1;
            let row = self.repeat_row(element, repeat, model, next)?;
            out.extend(quote! {
                let #name = #receiver.#field.render(|| #expression, #row);
            });
            Ok(name)
        } else if let Some(guard) = children(element, "xr.show_if").next() {
            let close = if element.attribute("kind").and_then(picker_value).is_some() {
                let popup = picker_field(element_key(element)?);
                quote!(self.__state.#popup.close();)
            } else {
                TokenStream::new()
            };
            if self.use_visual_plan && guard.attribute("xr.static_hidden") == Some("true") {
                let name = format_ident!("__xr_hidden_{}", *next);
                *next += 1;
                out.extend(quote! {
                    let #name = { #close ::xrack_runtime::Node::hidden() };
                });
                return Ok(name);
            }
            let condition = self.visual_value(guard, scope)?;
            let mut body = TokenStream::new();
            let name = self.render_element(element, scope, &mut body, next, None)?;
            out.extend(quote! {
                let #name = if #condition {
                    #body
                    #name
                } else {
                    #close
                    ::xrack_runtime::Node::hidden()
                };
            });
            Ok(name)
        } else {
            self.render_element(element, scope, out, next, None)
        }
    }

    fn repeat_row(
        &self,
        element: OperationRef<'a>,
        repeat: &RepeatScope<'_>,
        model: OperationRef<'a>,
        next: &mut usize,
    ) -> Result<TokenStream, String> {
        let item = ident(model.attr("item")?);
        let index = model.attr("index")?;
        let index_binding = if index.is_empty() {
            TokenStream::new()
        } else {
            let index = ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        };
        let row_local = row_local(repeat.index());
        let delegate = repeat.component.map(|_| quote!(__xr_delegate,));
        let mut body = TokenStream::new();
        let child = self.render_element(element, &repeat.row, &mut body, next, None)?;
        Ok(quote!(|#delegate __xr_index, #item, #row_local| { #index_binding #body #child }))
    }

    fn list_template<'b>(
        &self,
        list: OperationRef<'b>,
    ) -> Result<(OperationRef<'b>, OperationRef<'b>), String> {
        let row = scene_children(list)
            .find(|element| children(*element, "xr.repeat").next().is_some())
            .ok_or("virtual view needs a repeated template")?;
        let model = children(row, "xr.repeat")
            .next()
            .ok_or("virtual view needs a model")?;
        Ok((row, model))
    }

    fn tree_snapshot(
        &self,
        element: OperationRef<'a>,
        row: OperationRef<'a>,
        repeater: TokenStream,
        model: &TokenStream,
    ) -> Result<TokenStream, String> {
        let role = |name: &str| -> Result<Ident, String> {
            let binding = children(row, "xr.bind")
                .find(|bind| bind.attr("name") == Ok(name))
                .ok_or_else(|| format!("TreeRow is missing `{name}`"))?;
            Ok(ident(self.operand(binding, 0)?.attr("value")?))
        };
        let key = role("key")?;
        let parent = role("parent-key")?;
        let state = tree_state_field(element_key(element)?);
        Ok(
            quote!(self.__state.#state.snapshot(|| #repeater.resolve_model(|| #model), |item| (item.#key, item.#parent))),
        )
    }

    fn list_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            out.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#field.__list_rows(rest, __range),),
            );
        } else if element.attr("kind") == Ok("StandardTableView") {
            let model = self.standard_table_model(element, &self.visuals)?;
            let fields = table_columns(element)
                .map(|column| {
                    let binding = children(column, "xr.bind")
                        .find(|binding| binding.attr("name") == Ok("field"))
                        .ok_or("standard table column requires a field")?;
                    let name: String =
                        serde_json::from_str(self.operand(binding, 0)?.attr("value")?)
                            .map_err(|error| error.to_string())?;
                    Ok(ident(&name))
                })
                .collect::<Result<Vec<_>, String>>()?;
            out.extend(quote! {
                [#(#path),*] => ::xrack_runtime::standard_table_rows(&#model, __range, |item| [#(item.#fields.to_string()),*]),
            });
        } else if element.attr("kind") == Ok("StandardListView") {
            if let Some(binding) = model_binding(element) {
                let model = self.visual_value(binding, &self.visuals)?;
                let label = if matches!(value_type(self.operand(binding, 0)?)?, Type::RecordList(_))
                {
                    let field = children(element, "xr.bind")
                        .find(|binding| binding.attr("name") == Ok("text-role"))
                        .map(|binding| -> Result<String, String> {
                            let value = self.operand(binding, 0)?.attr("value")?;
                            serde_json::from_str(value).map_err(|error| error.to_string())
                        })
                        .transpose()?
                        .unwrap_or_else(|| "text".into());
                    let field = ident(&field);
                    quote!(|__xr_item| __xr_item.#field)
                } else {
                    quote!(|__xr_item| __xr_item)
                };
                out.extend(quote! {
                    [#(#path),*] => ::xrack_runtime::standard_list_rows(&#model, __range, #label),
                });
            }
        } else if matches!(
            element.attr("kind"),
            Ok("ListView" | "GridView" | "TableView" | "TreeView")
        ) {
            let (row, model) = self.list_template(element)?;
            let repeat = &self.visuals.repeats[element_key(row)?];
            let field = repeat_field(repeat.index());
            let model_expr = self.expr(self.operand(model, 0)?)?;
            let row_expr = self.repeat_row(row, repeat, model, &mut 0)?;
            if element.attr("kind") == Ok("TreeView") {
                let tree = self.tree_snapshot(
                    element,
                    row,
                    quote!(self.__state.__visuals.#field),
                    &model_expr,
                )?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let tree = #tree;
                        let rows = tree.rows().get(__range).unwrap_or_default();
                        self.__state.__visuals.#field.render_indices(|| #model_expr, rows.iter().map(|row| row.model_index), #row_expr)
                    },
                });
                return Ok(());
            }
            let create = if let Some(component) = repeat.component {
                let name = ident(component);
                let inputs = format_ident!("__Inputs_{}", rust_name(component));
                let bindings = self.instance_inputs(row, Some(model))?;
                quote!(|__context| {
                    let __parent = ::std::rc::Rc::downgrade(&self.__state);
                    #name::__new(#inputs { #bindings ..::core::default::Default::default() })
                },)
            } else {
                TokenStream::new()
            };
            out.extend(quote! {
                [#(#path),*] => self.__state.__visuals.#field.render_range(|| #model_expr, __range, #create #row_expr),
            });
        } else if children(element, "xr.repeat").next().is_none() {
            for (index, child) in scene_children(element).enumerate() {
                path.push(index);
                self.list_arms(child, path, out)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn delegate_event_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            out.extend(quote! {
                [#(#path,)* rest @ ..] => self.__state.#field.__route_delegate(scene.at_path(&[#(#path),*])?, rest, action),
            });
        } else if matches!(
            element.attr("kind"),
            Ok("ListView" | "GridView" | "TableView" | "TreeView")
        ) {
            let (row, model) = self.list_template(element)?;
            let repeat = &self.visuals.repeats[element_key(row)?];
            if let Some(component) = repeat.component {
                let field = repeat_field(repeat.index());
                let component = ident(component);
                let model_expr = self.expr(self.operand(model, 0)?)?;
                out.extend(quote! {
                    [#(#path,)* slot, 0, rest @ ..] => {
                        let list = scene.at_path(&[#(#path),*])?;
                        let row = list.children.get(*slot)?;
                        let index = list.list?.first_row.checked_add(*slot)?;
                        let child = row.children.first()?;
                        let result = self.__state.__visuals.#field.with_row(|| #model_expr, index, row.row_key?, |ui| {
                            let mut ui = #component { __state: ui.__state.clone() };
                            action.apply(&mut ui, child, rest)
                        });
                        Some(result.unwrap_or(false))
                    },
                });
            } else if matches!(element.attr("kind"), Ok("TableView" | "TreeView")) {
                let mut editors = Vec::new();
                row_editor_paths(row, &mut Vec::new(), &mut editors);
                let mut writes = TokenStream::new();
                for (cell_path, link) in editors {
                    let operand = self.operand(link, 0)?;
                    let value_type = value_type(operand)?;
                    let prepare = match value_type {
                        Type::String => quote! {
                            let ::xrack_runtime::RoutedEvent::CommitCell { original, text } = action else { return Some(false); };
                            if !::xrack_runtime::editable_table_cell(cell) { return Some(false); }
                            let expected = *original;
                            let next = *text;
                        },
                        Type::Int => quote! {
                            let ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::Spin(action)) = action else { return Some(false); };
                            let expected = cell.int("value")?;
                            let Some(next) = ::xrack_runtime::spin_value_after(cell, *action) else { return Some(false); };
                        },
                        Type::Bool => quote! {
                            let ::xrack_runtime::RoutedEvent::Dispatch("clicked") = action else { return Some(false); };
                            let expected = cell.boolean("checked", false);
                            let next = !expected;
                        },
                        Type::CheckState => quote! {
                            let ::xrack_runtime::RoutedEvent::Dispatch("clicked") = action else { return Some(false); };
                            let expected = cell.check_state();
                            let next = expected.next(cell.boolean("tristate", false));
                        },
                        Type::Float => quote! {
                            let expected = cell.number("value")?;
                            let next = match action {
                                ::xrack_runtime::RoutedEvent::Range(action) if cell.kind != "DoubleSpinBox" =>
                                    ::xrack_runtime::SliderRange::from_node(cell).apply(*action),
                                ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::Spin(action)) =>
                                    ::xrack_runtime::decimal_spin_value_after(cell, *action),
                                _ => None,
                            };
                            let Some(next) = next else { return Some(false); };
                        },
                        _ => return Err("unsupported row editor field type".into()),
                    };
                    let commit = if operand.name() == "xr.get_field" {
                        let field = ident(operand.attr("value")?);
                        let value = if value_type == Type::String {
                            quote!(next.into())
                        } else {
                            quote!(next)
                        };
                        quote! {
                            model.update_row_if_key(index, key, |record| {
                                if record.#field != expected { return None; }
                                let mut updated = record.clone();
                                updated.#field = #value;
                                Some(updated)
                            })
                        }
                    } else {
                        quote!(model.set_row_if_current(index, key, &expected.into(), next.into()))
                    };
                    writes.extend(quote!([#(#cell_path),*] => { #prepare #commit },));
                }
                if !writes.is_empty() {
                    let model_expr = self.expr(self.operand(model, 0)?)?;
                    let model_index = if element.attr("kind") == Ok("TreeView") {
                        quote! {
                            let item = view.tree.as_ref()?.rows().get(index)?;
                            if item.row != key { return Some(false); }
                            let index = item.model_index;
                        }
                    } else {
                        TokenStream::new()
                    };
                    out.extend(quote! {
                        [#(#path,)* slot, 0, rest @ ..] => {
                            let view = scene.at_path(&[#(#path),*])?;
                            let row = view.children.get(*slot)?;
                            let index = view.list?.first_row.checked_add(*slot)?;
                            let cell = row.children.first()?.interactive_at_path(rest)?;
                            let key = row.row_key?;
                            #model_index
                            let model = #model_expr;
                            Some(match rest { #writes _ => false })
                        },
                    });
                }
            }
        } else {
            if element.attr("kind") == Ok("SplitView") {
                let field = split_weights_field(element_key(element)?);
                let resized = children(element, "xr.handler")
                    .find(|handler| handler.attr("name") == Ok("resized"))
                    .map(|handler| self.statements(handler))
                    .transpose()?
                    .unwrap_or_default();
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::RoutedEvent::ResizeSplit(weights) = action else { return None; };
                        let node = scene.interactive_at_path(path)?;
                        let Some(next) = node.resized_split_weights(weights) else { return Some(false); };
                        if self.__state.#field.get(|| None).as_deref() == Some(next.as_ref()) { return Some(false); }
                        self.__state.#field.set(Some(next));
                        #resized
                        Some(true)
                    },
                });
            }
            for (index, child) in static_children(element) {
                path.push(index);
                self.delegate_event_arms(child, path, out)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn control_value(
        &self,
        element: OperationRef<'a>,
        name: &str,
        ty: Type,
    ) -> Result<TokenStream, String> {
        if let Some(link) = control_link(element, name) {
            return self.expr(self.operand(link, 0)?);
        }
        let initial = match children(element, "xr.bind")
            .find(|binding| binding.attr("name") == Ok(name))
        {
            Some(binding) => self.expr(self.operand(binding, 0)?)?,
            None if ty == Type::Date && name == "display-month" => {
                self.control_value(element, "date", Type::Date)?
            }
            None if ty == Type::Date => quote!(::xrack_runtime::Date::default()),
            None if ty == Type::Time => quote!(::xrack_runtime::Time::default()),
            None if ty == Type::Color => quote!(::xrack_runtime::Color(0xffffffff)),
            None if ty == Type::SwipeSide => quote!(::xrack_runtime::SwipeSide::Closed),
            None if ty == Type::CheckState => quote!(::xrack_runtime::CheckState::Unchecked),
            None if ty == Type::Bool && element.attr("kind") == Ok("GroupBox") => quote!(true),
            None if ty == Type::Bool => quote!(false),
            None if ty == Type::Float => {
                let bound = if name == "upper-value" {
                    "maximum"
                } else {
                    "minimum"
                };
                match children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(bound))
                {
                    Some(binding) => self.visual_value(binding, &self.visuals)?,
                    None if name == "upper-value" => quote!(100.0_f64),
                    None => quote!(0.0_f64),
                }
            }
            None if ty == Type::Int
                && matches!(
                    element.attr("kind"),
                    Ok("RadioGroup"
                        | "ListView"
                        | "GridView"
                        | "StandardListView"
                        | "TableView"
                        | "StandardTableView"
                        | "TreeView")
                ) =>
            {
                quote!(-1_i64)
            }
            None if ty == Type::Int => {
                match children(element, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("minimum"))
                {
                    Some(binding) => self.visual_value(binding, &self.visuals)?,
                    None => quote!(0_i64),
                }
            }
            None => quote!(::std::string::String::new()),
        };
        let field = control_field(element_key(element)?, name);
        Ok(quote!(self.__state.#field.get(|| #initial)))
    }

    fn set_control(
        &self,
        element: OperationRef<'a>,
        value: &TokenStream,
    ) -> Result<TokenStream, String> {
        let (property, _) = controlled_property(element).ok_or("missing controlled property")?;
        self.set_control_property(element, property, value)
    }

    fn set_control_property(
        &self,
        element: OperationRef<'a>,
        property: &str,
        value: &TokenStream,
    ) -> Result<TokenStream, String> {
        if let Some(link) = control_link(element, property) {
            let read = self.operand(link, 0)?;
            Ok(self.set_property(owner(read), read.attr("value")?, value))
        } else {
            let field = control_field(element_key(element)?, property);
            Ok(quote!(self.__state.#field.set_if_changed(#value);))
        }
    }

    fn binding_value_or(
        &self,
        element: OperationRef<'a>,
        name: &str,
        fallback: TokenStream,
    ) -> Result<TokenStream, String> {
        Ok(children(element, "xr.bind")
            .find(|binding| binding.attr("name") == Ok(name))
            .map(|binding| self.visual_value(binding, &self.visuals))
            .transpose()?
            .unwrap_or(fallback))
    }

    fn time_picker_options(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let use_24_hours = self.binding_value_or(element, "use-24-hour-format", quote!(true))?;
        let show_seconds = self.binding_value_or(element, "show-seconds", quote!(false))?;
        Ok(quote!(::xrack_runtime::TimePickerOptions::new(#use_24_hours, #show_seconds)))
    }

    fn date_picker_bounds(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let minimum =
            self.binding_value_or(element, "minimum-date", quote!(::xrack_runtime::Date::MIN))?;
        let maximum =
            self.binding_value_or(element, "maximum-date", quote!(::xrack_runtime::Date::MAX))?;
        Ok(quote!(::xrack_runtime::DateBounds::new(#minimum, #maximum)))
    }

    fn standard_table_model(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        self.visual_value(
            model_binding(element).ok_or("standard table requires a model")?,
            scope,
        )
    }

    fn render_table_header(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
        name: &Ident,
        model: &TokenStream,
    ) -> Result<(), String> {
        let columns = table_columns(element)
            .map(|column| self.render(column, scope, out, next))
            .collect::<Result<Vec<_>, _>>()?;
        let key = element_key(element)?;
        let sort = table_sort_field(key);
        let widths = table_widths_field(key);
        let selection = table_selection_field(key);
        out.extend(quote! {
            #name.table_header = Some(::std::boxed::Box::new(::xrack_runtime::TableHeader {
                columns: ::std::vec![#(#columns),*],
                sort: self.__state.#sort.get(|| None),
                column_widths: self.__state.#widths.get(::std::vec::Vec::new),
            }));
            let target = #name.selection_target();
            let multiple = #name.boolean("multi-select", false);
            if multiple || target != ::xrack_runtime::SelectionTarget::Rows {
                let current = #name.int("current-row").and_then(|value| <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(value).ok());
                #name.table_selection = Some(::std::boxed::Box::new(
                    self.__state.#selection.snapshot(#model, current, target, #name.table_header.as_ref().map_or(0, |header| header.columns.len()), multiple)
                ));
            }
        });
        Ok(())
    }

    fn combo_roles(
        &self,
        element: OperationRef<'a>,
        binding: OperationRef<'a>,
    ) -> Result<(String, String, Type), String> {
        let Some(Type::RecordList(record)) = operand_type(binding) else {
            return Err("expected a record ComboBox model".into());
        };
        let role = |name, default: &str| -> Result<String, String> {
            if let Some(role) =
                children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(name))
            {
                serde_json::from_str(self.operand(role, 0)?.attr("value")?)
                    .map_err(|error| error.to_string())
            } else {
                Ok(default.into())
            }
        };
        let text = role("text-role", "text")?;
        let value = role("value-role", &text)?;
        let ty = self.records[record.as_ref()]
            .iter()
            .find(|field| field.name == value)
            .ok_or("missing verified ComboBox value field")?
            .ty
            .clone();
        Ok((text, value, ty))
    }

    fn render_combo_model(
        &self,
        element: OperationRef<'a>,
        binding: OperationRef<'a>,
        scope: &VisualScope<'_>,
        node: &Ident,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let (text, value, _) = self.combo_roles(element, binding)?;
        let label = ident(&text);
        let value = ident(&value);
        let key = element_key(element)?;
        let items = combo_field(key, "items");
        let snapshot = combo_field(key, "snapshot");
        let model = self.visual_value(binding, scope)?;
        let (property, ty) = controlled_property(element).ok_or("missing ComboBox selection")?;
        let selection = self.control_value(element, property, ty)?;
        let selection = if property == "current-value" {
            quote!(__xr_items.snapshot(0, Some(&::xrack_runtime::Value::from(#selection))))
        } else {
            quote!(__xr_items.snapshot(#selection, None))
        };
        out.extend(quote! {
            let __xr_combo = self.__state.#snapshot.get(|| {
                let __xr_items = self.__state.#items.get(|| ::xrack_runtime::ComboItems::new(
                    #model,
                    |record| record.#label.as_str(),
                    |record| ::xrack_runtime::Value::from(record.#value.clone()),
                ));
                #selection
            });
            #node.properties.insert("model", ::xrack_runtime::Value::ComboModel(__xr_combo));
        });
        Ok(())
    }

    fn render_element(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
        include_children: Option<TokenStream>,
    ) -> Result<Ident, String> {
        let name = format_ident!("__xr_node_{}", *next);
        *next += 1;
        let id = element.attr("id")?;
        let id = if id.is_empty() {
            quote!(::core::option::Option::None)
        } else {
            quote!(::core::option::Option::Some(#id))
        };
        let binding_op = if element.name() == "xr.instance" {
            let ui = if children(element, "xr.repeat").next().is_some() {
                quote!(__xr_delegate)
            } else {
                let instance = instance_field(element_key(element)?);
                quote!(self.__state.#instance)
            };
            out.extend(quote! {
                let mut #name = #ui.render();
                #name.id = #id;
            });
            "xr.layout_bind"
        } else {
            let kind = element.attr("kind")?;
            let kind = match kind {
                kind if picker_value(kind).is_some() => "Dialog",
                "StandardTableView" => "TableView",
                "StandardButton" => "Button",
                kind => kind,
            };
            out.extend(quote!(let mut #name = ::xrack_runtime::Node::new(#kind, #id);));
            if self.with_fonts && element.attr("id")? == "root" {
                out.extend(quote!(#name.fonts = ::core::option::Option::Some(&__XR_FONTS);));
            }
            "xr.bind"
        };
        let action = action_ref(element);
        if let Some(action) = action {
            let render = action_render_method(action);
            out.extend(quote!(self.#render(&mut #name);));
        }
        for binding in children(element, binding_op) {
            let key = binding.attr("name")?;
            if record_combo_binding(element).is_some()
                && matches!(key, "model" | "text-role" | "value-role")
            {
                continue;
            }
            if element.attr("kind") == Ok("StandardListView")
                && matches!(key, "model" | "text-role")
            {
                continue;
            }
            if (element.attr("kind") == Ok("StandardTableView") && key == "model")
                || (element.attr("kind") == Ok("TableColumn") && key == "field")
            {
                continue;
            }
            if controlled_properties(element).any(|(property, _)| key == property) {
                continue;
            }
            let mut value = self.visual_value(binding, scope)?;
            if key == "model" && element.attr("kind") == Ok("Tumbler") {
                out.extend(quote! {
                    #name.tumbler_items = Some(::xrack_runtime::TumblerItems::new(#value));
                });
                continue;
            }
            if key == "enabled" && action.is_some() {
                value = quote!(#name.boolean("enabled", true) && #value);
            }
            out.extend(quote! {
                #name.properties.insert(#key, ::xrack_runtime::Value::from(#value));
            });
            if self.use_visual_plan && binding.attribute("xr.update_effect") == Some("paint") {
                out.extend(quote!(#name.mark_paint_only_background();));
            }
            if self.use_visual_plan && binding.attribute("xr.precompiled_path") == Some("true") {
                let commands: String =
                    serde_json::from_str(self.operand(binding, 0)?.attr("value")?)
                        .map_err(|error| error.to_string())?;
                if let Some(path) = self.paths.get(commands.as_str()) {
                    out.extend(quote!(#name.set_precompiled_path(&#path);));
                }
            }
        }
        if let Some(link) = children(element, "xr.row_link").next() {
            let property = link.attr("name")?;
            let flag = if property == "text" {
                "editable"
            } else {
                "table-editor"
            };
            let value = self.visual_value(link, scope)?;
            out.extend(quote! {
                #name.properties.insert(#property, ::xrack_runtime::Value::from(#value));
                #name.properties.insert(#flag, ::xrack_runtime::Value::Bool(true));
            });
        }
        let popup_value = element.attribute("kind").and_then(picker_value);
        for (property, ty) in controlled_properties(element) {
            if popup_value
                .as_ref()
                .is_some_and(|(name, _)| property == *name)
            {
                continue;
            }
            if property == "edit-text" && default_combo_edit_text(element) {
                let field = control_field(element_key(element)?, property);
                out.extend(quote! {
                    if let Some(value) = self.__state.#field.explicit_value() {
                        #name.properties.insert("edit-text", ::xrack_runtime::Value::String(value));
                    }
                });
                continue;
            }
            let value = self.control_value(element, property, ty)?;
            out.extend(quote! {
                #name.properties.insert(#property, ::xrack_runtime::Value::from(#value));
            });
        }
        if let Some(binding) = record_combo_binding(element) {
            self.render_combo_model(element, binding, scope, &name, out)?;
        }
        for animation in children(element, "xr.animate") {
            let key = animation.attr("name")?;
            let duration = self.visual_expr(self.operand(animation, 0)?, scope)?;
            out.extend(quote! {
                #name.animations.insert(#key, #duration as ::core::primitive::u64);
            });
        }
        let kind = element.attribute("kind");
        for timer in element_children(element).filter(|child| child.attr("kind") == Ok("Timer")) {
            let register = timer_member(element_key(timer)?, "register");
            out.extend(quote! { #name.timers.extend(self.#register()); });
        }
        for dialog in element_children(element).filter(|child| native_dialog(*child)) {
            let register = native_dialog_member(element_key(dialog)?, "register");
            out.extend(quote!(#name.native_dialogs.extend(self.#register());));
        }
        if kind == Some("StandardButton") {
            out.extend(quote! {
                #name.properties.entry("kind").or_insert(::xrack_runtime::StandardButtonKind::Ok.into());
            });
        }
        if let Some((property, ty)) = popup_value {
            let popup = picker_field(element_key(element)?);
            let value = self.control_value(element, property, ty)?;
            out.extend(quote! { self.__state.#popup.populate(&mut #name, || #value); });
            return Ok(name);
        }
        if kind == Some("TimePicker") {
            out.extend(quote! {
                let time = #name.time("time").unwrap_or_default();
                ::xrack_runtime::TimePickerOptions::from_node(&#name).populate(&mut #name, time);
            });
            return Ok(name);
        }
        if kind == Some("RangeSlider") {
            out.extend(quote! {
                #name.children.push(::xrack_runtime::Node::new("RangeThumb", None));
                #name.children.push(::xrack_runtime::Node::new("RangeThumb", None));
            });
        }
        if kind == Some("StandardListView") {
            if let Some(binding) = model_binding(element) {
                let model = self.visual_value(binding, scope)?;
                out.extend(quote! {
                    let __xr_model = #model;
                    #name.list = Some(::xrack_runtime::ListMetrics {
                        row_count: ::xrack_runtime::ListModel::row_count(&__xr_model), first_row: 0,
                    });
                    #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(&__xr_model));
                });
            } else {
                out.extend(quote! { #name.list = Some(::xrack_runtime::ListMetrics::default()); });
            }
            return Ok(name);
        }
        if kind == Some("StandardTableView") {
            let model = self.standard_table_model(element, scope)?;
            out.extend(quote! {
                let __xr_table_model = #model;
                #name.list = Some(::xrack_runtime::ListMetrics {
                    row_count: ::xrack_runtime::ListModel::row_count(&__xr_table_model), first_row: 0,
                });
                #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(&__xr_table_model));
                #name.properties.insert("__standard-table", ::xrack_runtime::Value::Bool(true));
                #name.properties.entry("follow-current").or_insert(::xrack_runtime::Value::Bool(true));
            });
            self.render_table_header(element, scope, out, next, &name, &quote!(&__xr_table_model))?;
            return Ok(name);
        }
        if matches!(
            kind,
            Some("ListView" | "GridView" | "TableView" | "TreeView")
        ) {
            let (row, model) = self.list_template(element)?;
            let repeat = &scope.repeats[element_key(row)?];
            let receiver = scope.receiver();
            let field = repeat_field(repeat.index());
            let model = self.visual_expr(self.operand(model, 0)?, scope)?;
            if kind == Some("TreeView") {
                let tree = self.tree_snapshot(element, row, quote!(#receiver.#field), &model)?;
                let state = tree_state_field(element_key(element)?);
                out.extend(quote! {
                    let tree = #tree;
                    #name.list = Some(::xrack_runtime::ListMetrics { row_count: tree.rows().len(), first_row: 0 });
                    if #name.boolean("multi-select", false) {
                        #name.tree_selection = Some(self.__state.#state.selection());
                    }
                    #name.tree = Some(tree);
                });
                return Ok(name);
            }
            let count = if kind == Some("GridView") {
                quote! {
                    #receiver.#field.with_model(|| #model, |model| {
                        #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(model));
                        ::xrack_runtime::ListModel::row_count(model)
                    })
                }
            } else {
                quote!(#receiver.#field.row_count(|| #model))
            };
            out.extend(quote! {
                #name.list = Some(::xrack_runtime::ListMetrics { row_count: #count, first_row: 0 });
            });
            if kind == Some("TableView") {
                let model = quote!(&*#receiver.#field.resolve_model(|| #model));
                self.render_table_header(element, scope, out, next, &name, &model)?;
            }
            return Ok(name);
        }
        let mut content = TokenStream::new();
        if kind == Some("StackView") {
            let snapshot = stack_snapshot_method(element_key(element)?);
            content.extend(quote! {
                let __xr_stack = self.#snapshot();
                #name.stack = Some(__xr_stack.clone());
            });
        }
        if kind == Some("SwipeView") {
            // Page eligibility is cheap to read even when its body is not rendered.
            let mut available = Vec::new();
            for page in scene_children(element) {
                let flag = |name| -> Result<TokenStream, String> {
                    Ok(children(page, "xr.bind")
                        .find(|binding| binding.attr("name") == Ok(name))
                        .map(|binding| self.visual_value(binding, scope))
                        .transpose()?
                        .unwrap_or_else(|| quote!(true)))
                };
                let visible = flag("visible")?;
                let enabled = flag("enabled")?;
                available.push(quote!((#visible) && (#enabled)));
            }
            content.extend(quote! {
                let __xr_swipe_active =
                    ::xrack_runtime::resolve_swipe_index(#name.int("current-index"), [#(#available),*]);
            });
        }
        for (index, child) in scene_children(element).enumerate() {
            let repeated_option = matches!(kind, Some("RadioGroup" | "TabBar"))
                && children(child, "xr.repeat").next().is_some();
            let child = if kind == Some("TabWidget") {
                let index = index as i64;
                let active = quote!(#name.int("current-index").unwrap_or(0) == #index);
                self.render_element(child, scope, &mut content, next, Some(active))?
            } else if kind == Some("SwipeView") {
                let active = quote!(#name.is_visible() && __xr_swipe_active == Some(#index));
                self.render_element(child, scope, &mut content, next, Some(active))?
            } else if kind == Some("StackView") {
                let route = child.attr("id")?;
                let active = quote!(#name.is_visible() && __xr_stack.current() == Some(#route));
                self.render_element(child, scope, &mut content, next, Some(active))?
            } else {
                self.render(child, scope, &mut content, next)?
            };
            if repeated_option {
                content.extend(quote!(#name.children.extend(#child.children.iter().cloned());));
            } else {
                content.extend(quote!(#name.children.push(#child);));
            }
        }
        if matches!(kind, Some("Dialog" | "Drawer" | "PopupWindow")) {
            out.extend(quote! { if #name.is_visible() { #content } });
        } else if let Some(active) = include_children {
            out.extend(quote! {
                if #active && #name.boolean("visible", true) && #name.boolean("enabled", true) {
                    #content
                }
            });
        } else {
            out.extend(content);
        }
        if kind == Some("RadioGroup") {
            out.extend(quote! {
                let __xr_selected = ::xrack_runtime::group_index(&#name);
                for (__xr_index, __xr_child) in #name.children.iter_mut().enumerate() {
                    __xr_child.properties.insert("checked", ::xrack_runtime::Value::Bool(__xr_selected == Some(__xr_index)));
                }
            });
        }
        if kind == Some("SplitView") {
            let field = split_weights_field(element_key(element)?);
            out.extend(quote! {
                #name.split_weights = self.__state.#field.get(|| None);
                #name.add_split_handles();
            });
        }
        if self.use_visual_plan && element.attribute("xr.static_grid") == Some("true") {
            out.extend(scene_plan::static_grid(element, &name)?);
        }
        Ok(name)
    }

    fn dispatch_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        local: &mut TokenStream,
        paths: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            paths.extend(quote! {
                [#(#path,)* rest @ ..] => self.__state.#field.__dispatch_path(rest, event),
            });
            return Ok(());
        }
        let id = element.attr("id")?;
        let dispatchable = |handler: OperationRef<'_>| {
            (!typed_events(element)
                && !matches!(
                    handler.attr("name"),
                    Ok("check-state-changed" | "close-requested")
                ))
                || (element.attr("kind") == Ok("SwipeDelegate")
                    && handler.attr("name") == Ok("clicked"))
        };
        if matches!(element.attr("kind"), Ok("Button" | "ToolButton"))
            && let Some(action) = action_ref(element)
        {
            let activate = action_activate_method(action);
            paths.extend(quote!([#(#path),*] => event == "clicked" && self.#activate(),));
            if !id.is_empty() {
                local.extend(quote!((#id, "clicked") => self.#activate(),));
            }
        } else if element.attr("kind") == Ok("GroupBox") {
            let checkable = children(element, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("checkable"))
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .unwrap_or_else(|| quote!(false));
            let current = self.control_value(element, "checked", Type::Bool)?;
            let update = self.set_control(element, &quote!(__xr_checked))?;
            let toggled = self.typed_event(element, "toggled", &quote!(__xr_checked))?;
            let click = quote!({
                if !#checkable { return false; }
                let __xr_checked = !(#current);
                #update
                #toggled
                true
            });
            paths.extend(quote!([#(#path),*] => event == "clicked" && #click,));
            if !id.is_empty() {
                local.extend(quote!((#id, "clicked") => #click,));
            }
        } else if element.attr("kind").is_ok_and(is_toggle) {
            let projection = check_state_binding(element);
            let (next, value) = if check_state_control(element) {
                let current = if let Some(binding) = projection {
                    self.visual_value(binding, &self.visuals)?
                } else {
                    self.control_value(element, "check-state", Type::CheckState)?
                };
                let tristate = children(element, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("tristate"))
                    .map(|binding| self.visual_value(binding, &self.visuals))
                    .transpose()?
                    .unwrap_or_else(|| quote!(false));
                (quote!((#current).next(#tristate)), quote!(__xr_state))
            } else {
                let checked = self.control_value(element, "checked", Type::Bool)?;
                (
                    quote!(::xrack_runtime::CheckState::from(!(#checked))),
                    quote!(__xr_state == ::xrack_runtime::CheckState::Checked),
                )
            };
            let update = if projection.is_some() {
                TokenStream::new()
            } else {
                self.set_control(element, &value)?
            };
            let changed = self.typed_event(element, "check-state-changed", &quote!(__xr_state))?;
            let click = quote!({
                let __xr_state = #next;
                #update
                self.__dispatch_local(#id, "toggled");
                #changed
                true
            });
            paths.extend(quote! {
                [#(#path),*] => {
                    if event == "clicked" { #click } else { self.__dispatch_local(#id, event) }
                },
            });
            if !id.is_empty() {
                local.extend(quote!((#id, "clicked") => #click,));
            }
        } else if children(element, "xr.handler").any(dispatchable) {
            paths.extend(quote!([#(#path),*] => self.__dispatch_local(#id, event),));
        }
        for handler in children(element, "xr.handler").filter(|handler| dispatchable(*handler)) {
            let event = handler.attr("name")?;
            let statements = self.statements(handler)?;
            local.extend(quote!((#id, #event) => { #statements true },));
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.dispatch_arms(child, path, local, paths)?;
            path.pop();
        }
        Ok(())
    }

    fn edit_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__edit_path(rest, text),),
            );
            return Ok(());
        }
        if combo_editor(element) {
            let set = self.set_control_property(element, "edit-text", &quote!(text.clone()))?;
            let edited = self.typed_event(element, "edited", &quote!(text))?;
            out.extend(quote!([#(#path),*] => { #set #edited true },));
        } else if element.attr("kind") == Ok("SearchField") {
            let set = self.set_control(element, &quote!(text.clone()))?;
            let edited = self.typed_event(element, "edited", &quote!(text.clone()))?;
            let search = self.typed_event(element, "search-triggered", &quote!(text))?;
            let live = children(element, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("live"))
                .map(|binding| self.expr(self.operand(binding, 0)?))
                .transpose()?
                .unwrap_or_else(|| quote!(false));
            out.extend(quote!([#(#path),*] => { #set #edited if #live { #search } true },));
        } else if element.attr("kind").is_ok_and(is_multiline_text_input) {
            let has_handler =
                children(element, "xr.handler").any(|handler| handler.attr("name") == Ok("edited"));
            let value = if has_handler {
                quote!(text.clone())
            } else {
                quote!(text)
            };
            let set = self.set_control(element, &value)?;
            let edited = self.typed_event(element, "edited", &quote!(text))?;
            out.extend(quote!([#(#path),*] => { #set #edited true },));
        } else if text_item(element) {
            let set = self.set_control(element, &quote!(text))?;
            let id = element.attr("id")?;
            out.extend(quote! {
                [#(#path),*] => { #set self.__dispatch_local(#id, "edited"); true },
            });
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.edit_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn typed_event(
        &self,
        element: OperationRef<'a>,
        event: &str,
        value: &TokenStream,
    ) -> Result<TokenStream, String> {
        self.typed_event_values(element, event, std::slice::from_ref(value))
    }

    fn typed_event_values(
        &self,
        element: OperationRef<'a>,
        event: &str,
        values: &[TokenStream],
    ) -> Result<TokenStream, String> {
        let Some(handler) =
            children(element, "xr.handler").find(|handler| handler.attr("name") == Ok(event))
        else {
            return Ok(TokenStream::new());
        };
        let parameters: Vec<Parameter> =
            serde_json::from_str(handler.attr("parameters")?).map_err(|err| err.to_string())?;
        if parameters.len() != values.len() {
            return Err(format!("{event} requires {} parameters", values.len()));
        }
        let names = parameters.iter().map(|parameter| ident(&parameter.name));
        let body = self.statements(handler)?;
        if values.is_empty() {
            return Ok(body);
        }
        Ok(quote!(let (#(#names,)*) = (#(#values,)*); #body))
    }

    fn drag_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__drag_path(rest, event),),
            );
            return Ok(());
        }
        match element.attr("kind")? {
            "DragArea" => {
                let started = self.typed_event_values(element, "drag-started", &[])?;
                let finished =
                    self.typed_event_values(element, "drag-finished", &[quote!(*__action)])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        match event {
                            ::xrack_runtime::DragEvent::Started => { #started }
                            ::xrack_runtime::DragEvent::Finished(__action) => { #finished }
                            _ => return false,
                        }
                        true
                    },
                });
            }
            "DropArea" => {
                let args = &[quote!(__data.clone()), quote!(*__action)];
                let entered = self.typed_event_values(element, "entered", args)?;
                let dropped = self.typed_event_values(element, "dropped", args)?;
                let files = &[quote!(::xrack_runtime::VecModel::from(
                    __paths.as_ref().to_vec()
                ))];
                let files_entered = self.typed_event_values(element, "files-entered", files)?;
                let files_dropped = self.typed_event_values(element, "files-dropped", files)?;
                let moved = self.typed_event_values(
                    element,
                    "position-changed",
                    &[quote!(*__x), quote!(*__y)],
                )?;
                let exited = self.typed_event_values(element, "exited", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        match event {
                            ::xrack_runtime::DragEvent::Entered { data: __data, action: __action } => { #entered }
                            ::xrack_runtime::DragEvent::Dropped { data: __data, action: __action } => { #dropped }
                            ::xrack_runtime::DragEvent::FilesEntered(__paths) => { #files_entered }
                            ::xrack_runtime::DragEvent::FilesDropped(__paths) => { #files_dropped }
                            ::xrack_runtime::DragEvent::Moved { x: __x, y: __y } => { #moved }
                            ::xrack_runtime::DragEvent::Exited => { #exited }
                            _ => return false,
                        }
                        true
                    },
                });
            }
            _ => {}
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.drag_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn range_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(quote!([#(#path,)* rest @ ..] => self.__state.#instance.__range_path(rest, node, action),));
            return Ok(());
        }
        if matches!(element.attr("kind"), Ok("Slider" | "Dial")) {
            let set = self.set_control(element, &quote!(value))?;
            let changed = self.typed_event(element, "changed", &quote!(value))?;
            let released = self.typed_event(element, "released", &quote!(value))?;
            out.extend(quote! {
                [#(#path),*] => {
                    let range = ::xrack_runtime::SliderRange::from_node(node);
                    if action == ::xrack_runtime::RangeAction::Release {
                        let value = range.value();
                        #released
                    } else {
                        let Some(value) = range.apply(action) else { return false; };
                        #set
                        #changed
                    }
                    true
                },
            });
        } else if element.attr("kind") == Ok("ColorPickerPopup") {
            let popup = picker_field(element_key(element)?);
            let open = self.control_value(element, "open", Type::Bool)?;
            let show_alpha = self.binding_value_or(element, "show-alpha", quote!(false))?;
            out.extend(quote! {
                [#(#path,)* 0, ..] => (#open) && self.__state.#popup.slide(#show_alpha, node, action),
            });
        } else if element.attr("kind") == Ok("RangeSlider") {
            let lower = self.control_value(element, "lower-value", Type::Float)?;
            let upper = self.control_value(element, "upper-value", Type::Float)?;
            let set_lower =
                self.set_control_property(element, "lower-value", &quote!(next_lower))?;
            let set_upper =
                self.set_control_property(element, "upper-value", &quote!(next_upper))?;
            let values = [quote!(__xr_lower), quote!(__xr_upper)];
            let changed = self.typed_event_values(element, "changed", &values)?;
            let released = self.typed_event_values(element, "released", &values)?;
            out.extend(quote! {
                [#(#path,)* handle @ (0 | 1)] => {
                    let raw_lower = #lower;
                    let raw_upper = #upper;
                    let interval = ::xrack_runtime::SliderInterval::from_values(node, raw_lower, raw_upper);
                    if action == ::xrack_runtime::RangeAction::Release {
                        let [__xr_lower, __xr_upper] = interval.values();
                        #released
                    } else {
                        let Some([next_lower, next_upper]) = interval.apply(*handle, action) else { return false; };
                        if next_lower != raw_lower { #set_lower }
                        if next_upper != raw_upper { #set_upper }
                        let [__xr_lower, __xr_upper] =
                            ::xrack_runtime::SliderInterval::from_values(node, #lower, #upper).values();
                        #changed
                    }
                    true
                },
            });
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.range_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn record_combo_actions(
        &self,
        element: OperationRef<'a>,
        binding: OperationRef<'a>,
    ) -> Result<TokenStream, String> {
        let (_, _, ty) = self.combo_roles(element, binding)?;
        let variant = match ty {
            Type::String => quote!(String),
            Type::Int => quote!(Int),
            Type::Float => quote!(Float),
            Type::Bool => quote!(Bool),
            Type::Date => quote!(Date),
            Type::Time => quote!(Time),
            _ => return Err("unsupported ComboBox value-role type".into()),
        };
        let by_value =
            controlled_property(element).is_some_and(|(name, _)| name == "current-value");
        let selection = if by_value {
            quote!(__xr_value.clone())
        } else {
            quote!(__xr_index as ::core::primitive::i64)
        };
        let set = self.set_control(element, &selection)?;
        let select_value = quote! {
            let Some(::xrack_runtime::Value::#variant(__xr_value)) = ::xrack_runtime::combo_value(node, __xr_index) else { return false; };
            #set
        };
        let selected = self.typed_event(element, "selected", &quote!(__xr_label))?;
        let value_selected = self.typed_event(element, "value-selected", &quote!(__xr_value))?;
        let (select_text, accept) = if combo_editor(element) {
            let field = control_field(element_key(element)?, "edit-text");
            let default = default_combo_edit_text(element);
            let select_text = if default {
                quote!(self.__state.#field.reset();)
            } else {
                self.set_control_property(element, "edit-text", &quote!(__xr_label.clone()))?
            };
            let reset = default.then(|| quote!(self.__state.#field.reset();));
            let keep_draft =
                default.then(|| quote!(self.__state.#field.set_if_changed(__xr_draft.clone());));
            let clear = if by_value {
                TokenStream::new()
            } else {
                self.set_control(element, &quote!(-1_i64))?
            };
            let accepted = self.typed_event(element, "accepted", &quote!(__xr_draft))?;
            (
                quote!(if ::xrack_runtime::combo_editable(node) { #select_text }),
                quote! {
                    if action == ::xrack_runtime::ControlAction::AcceptComboText {
                        if !::xrack_runtime::combo_editable(node) { return false; }
                        let __xr_draft = ::xrack_runtime::text_input_value(node).to_owned();
                        let matching = ::xrack_runtime::combo_index(node)
                            .filter(|_| ::xrack_runtime::combo_text(node) == __xr_draft)
                            .or_else(|| ::xrack_runtime::combo_find_label(node, |label| label == __xr_draft));
                        if let Some(__xr_index) = matching {
                            #select_value
                            #reset
                            #value_selected
                        } else {
                            #clear
                            #keep_draft
                        }
                        #accepted
                        return true;
                    }
                },
            )
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        Ok(quote! {
            #accept
            let ::xrack_runtime::ControlAction::Select(__xr_index) = action else { return false; };
            let Some(__xr_label) = ::xrack_runtime::combo_label(node, __xr_index) else { return false; };
            let __xr_label = __xr_label.into_owned();
            #select_value
            #select_text
            #selected
            #value_selected
            true
        })
    }

    fn control_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(quote!([#(#path,)* rest @ ..] => self.__state.#instance.__control_path(rest, node, action),));
            return Ok(());
        }
        match element.attr("kind")? {
            kind @ ("MenuItem" | "Button" | "ToolButton" | "Shortcut")
                if action_ref(element).is_some() =>
            {
                let activate = action_activate_method(action_ref(element).unwrap());
                let expected = match kind {
                    "MenuItem" => quote!(ActivateMenuItem),
                    "Shortcut" => quote!(ActivateShortcut),
                    _ => quote!(ActivateAction),
                };
                out.extend(quote! {
                    [#(#path),*] => action == ::xrack_runtime::ControlAction::#expected
                        && self.#activate(),
                });
            }
            "Shortcut" => {
                let activated = self.typed_event_values(element, "activated", &[])?;
                let ambiguous = self.typed_event_values(element, "activated-ambiguously", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        match action {
                            ::xrack_runtime::ControlAction::ActivateShortcut => { #activated }
                            ::xrack_runtime::ControlAction::AmbiguousShortcut => { #ambiguous }
                            _ => return false,
                        }
                        true
                    },
                });
            }
            "MenuItem" => {
                let checked = self.control_value(element, "checked", Type::Bool)?;
                let toggle = self.set_control(element, &quote!(!(#checked)))?;
                let activated = children(element, "xr.handler")
                    .find(|handler| handler.attr("name") == Ok("activated"))
                    .map(|handler| self.statements(handler))
                    .transpose()?
                    .unwrap_or_default();
                out.extend(quote! {
                    [#(#path),*] => {
                        if action != ::xrack_runtime::ControlAction::ActivateMenuItem { return false; }
                        if node.boolean("checkable", false) { #toggle }
                        #activated
                        true
                    },
                });
            }
            "Dialog" => {
                let open = self.control_value(element, "open", Type::Bool)?;
                let close = self.set_control(element, &quote!(false))?;
                let handler = |event| {
                    children(element, "xr.handler")
                        .find(|handler| handler.attr("name") == Ok(event))
                        .map(|handler| self.statements(handler))
                        .transpose()
                        .map(|body| body.unwrap_or_default())
                };
                let accepted = handler("accepted")?;
                let rejected = handler("rejected")?;
                let applied = handler("applied")?;
                let reset = handler("reset")?;
                let help = handler("help-requested")?;
                let discarded = handler("discarded")?;
                out.extend(quote! {
                    [#(#path),*] => {
                        use ::xrack_runtime::{ControlAction, DialogCommand, DialogRole};
                        if !#open { return false; }
                        match action {
                            ControlAction::Dialog(DialogRole::Accept) => { #close #accepted }
                            ControlAction::Dialog(DialogRole::Reject) => { #close #rejected }
                            ControlAction::DialogCommand(DialogCommand::Apply) => { #applied }
                            ControlAction::DialogCommand(DialogCommand::Reset) => { #reset }
                            ControlAction::DialogCommand(DialogCommand::Help) => { #help }
                            ControlAction::DialogCommand(DialogCommand::Discard) => { #close #discarded }
                            _ => return false,
                        }
                        true
                    },
                });
            }
            "Drawer" => {
                let open = self.control_value(element, "open", Type::Bool)?;
                let set = self.set_control(element, &quote!(__xr_open))?;
                let opened = self.typed_event_values(element, "opened", &[])?;
                let closed = self.typed_event_values(element, "closed", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Drawer(__xr_open) = action else { return false; };
                        if #open == __xr_open { return false; }
                        #set
                        if __xr_open { #opened } else { #closed }
                        true
                    },
                });
            }
            "PopupWindow" => {
                let method = popup_method(element_key(element)?);
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Popup(open) = action else { return false; };
                        self.#method(open)
                    },
                });
            }
            "SwipeDelegate" => {
                let current = self.control_value(element, "open-side", Type::SwipeSide)?;
                let set = self.set_control(element, &quote!(__xr_side))?;
                let changed = self.typed_event(element, "changed", &quote!(__xr_side))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::SwipeDelegate(__xr_side) = action else { return false; };
                        let current = #current;
                        if node.swipe_side() != current || __xr_side == current
                            || (__xr_side != ::xrack_runtime::SwipeSide::Closed && node.swipe_panel(__xr_side).is_none()) {
                            return false;
                        }
                        #set
                        #changed
                        true
                    },
                });
            }
            kind @ ("SpinBox" | "DoubleSpinBox") => {
                let (apply, guard) = if kind == "DoubleSpinBox" {
                    let current = self.control_value(element, "value", Type::Float)?;
                    (
                        quote!(::xrack_runtime::decimal_spin_value_after),
                        quote! {
                            if let ::xrack_runtime::SpinAction::CommitDecimalIfCurrent { original, .. } = action
                                && original != ::xrack_runtime::SpinDecimal::new(#current) {
                                return false;
                            }
                        },
                    )
                } else {
                    (
                        quote!(::xrack_runtime::spin_value_after),
                        TokenStream::new(),
                    )
                };
                let set = self.set_control(element, &quote!(__xr_value))?;
                let edited = self.typed_event(element, "edited", &quote!(__xr_value))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Spin(action) = action else { return false; };
                        #guard
                        let Some(__xr_value) = #apply(node, action) else { return false; };
                        #set
                        #edited
                        true
                    },
                });
            }
            "TreeView" => {
                let (row, repeat) = self.list_template(element)?;
                let repeater = repeat_field(self.visuals.repeats[element_key(row)?].index());
                let model = self.expr(self.operand(repeat, 0)?)?;
                let tree = self.tree_snapshot(
                    element,
                    row,
                    quote!(self.__state.__visuals.#repeater),
                    &model,
                )?;
                let state = tree_state_field(element_key(element)?);
                let current = self.control_value(element, "current-key", Type::Int)?;
                let key = quote!(key);
                let set = self.set_control(element, &key)?;
                let changed = self.typed_event(element, "changed", &key)?;
                let expanded_event = self.typed_event(element, "expanded", &key)?;
                let collapsed_event = self.typed_event(element, "collapsed", &key)?;
                let selected = self.typed_event(
                    element,
                    "selection-changed",
                    &quote!(count as ::core::primitive::i64),
                )?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let Some(snapshot) = &node.tree else { return false; };
                        let live = #tree;
                        let current = #current;
                        if let ::xrack_runtime::ControlAction::Tree(::xrack_runtime::TreeAction::SetExpanded { key, row, expanded }) = action {
                            if !self.__state.#state.set_expanded(&live, key, row, expanded) { return false; }
                            if expanded {
                                #expanded_event
                            } else {
                                if live.is_descendant(current, key) {
                                    #set
                                    #changed
                                }
                                #collapsed_event
                            }
                            return true;
                        }
                        let multiple = node.tree_selection.is_some();
                        let action = match action {
                            ::xrack_runtime::ControlAction::Select(index) => {
                                let Some(item) = snapshot.rows().get(index) else { return false; };
                                ::xrack_runtime::TreeSelectionAction::Row {
                                    key: item.key, row: item.row, operation: ::xrack_runtime::RowSelection::Replace,
                                }
                            }
                            ::xrack_runtime::ControlAction::Tree(::xrack_runtime::TreeAction::Selection(action)) if multiple => action,
                            _ => return false,
                        };
                        if multiple {
                            if !snapshot.same_model(&live) { return false; }
                            let Some(change) = self.__state.#state.select(&live, current, action) else { return false; };
                            if let Some(key) = change.current { #set #changed }
                            if let Some(count) = change.selected_count { #selected }
                            return change.current.is_some() || change.selected_count.is_some();
                        }
                        let ::xrack_runtime::TreeSelectionAction::Row { key, row, .. } = action else { return false; };
                        if current == key
                            || !live.visible_index(key).and_then(|index| live.rows().get(index)).is_some_and(|item| item.row == row)
                        { return false; }
                        #set
                        #changed
                        true
                    },
                });
            }
            kind @ ("ListView" | "GridView" | "StandardListView" | "TableView"
            | "StandardTableView") => {
                let source_guard = if matches!(kind, "StandardListView" | "StandardTableView")
                    && let Some(binding) = model_binding(element)
                {
                    let model = self.visual_value(binding, &self.visuals)?;
                    quote! {
                        if node.standard_list_stamp != Some(::xrack_runtime::ListModel::structure_stamp(&#model)) {
                            return false;
                        }
                    }
                } else if kind == "GridView" {
                    let (row, repeat) = self.list_template(element)?;
                    let field = repeat_field(self.visuals.repeats[element_key(row)?].index());
                    let model = self.expr(self.operand(repeat, 0)?)?;
                    quote! {
                        let stamp = self.__state.__visuals.#field.with_model(
                            || #model,
                            ::xrack_runtime::ListModel::structure_stamp,
                        );
                        if node.standard_list_stamp != Some(stamp) { return false; }
                    }
                } else {
                    TokenStream::new()
                };
                let table = matches!(kind, "TableView" | "StandardTableView");
                let value = quote!(index as ::core::primitive::i64);
                let set = self.set_control(element, &value)?;
                let event = self.typed_event(
                    element,
                    if table {
                        "current-row-changed"
                    } else {
                        "changed"
                    },
                    &value,
                )?;
                let header_actions = if table {
                    let field = table_sort_field(element_key(element)?);
                    let widths = table_widths_field(element_key(element)?);
                    let selection = table_selection_field(element_key(element)?);
                    let model = if kind == "StandardTableView" {
                        let model = self.standard_table_model(element, &self.visuals)?;
                        quote!(&#model)
                    } else {
                        let (row, repeat) = self.list_template(element)?;
                        let repeater =
                            repeat_field(self.visuals.repeats[element_key(row)?].index());
                        let model = self.expr(self.operand(repeat, 0)?)?;
                        quote!(&*self.__state.__visuals.#repeater.resolve_model(|| #model))
                    };
                    let selected = self.typed_event(
                        element,
                        "selection-changed",
                        &quote!(count as ::core::primitive::i64),
                    )?;
                    let ascending = self.typed_event(element, "sort-ascending", &value)?;
                    let descending = self.typed_event(element, "sort-descending", &value)?;
                    quote! {
                        if let ::xrack_runtime::ControlAction::ResizeColumn { column, width } = action {
                            if ::xrack_runtime::resizable_table_column(node, column).is_none() { return false; }
                            let mut widths = self.__state.#widths.get(::std::vec::Vec::new);
                            if widths.get(column) == Some(&Some(width)) { return false; }
                            if widths.len() <= column { widths.resize(column + 1, None); }
                            widths[column] = Some(width);
                            self.__state.#widths.set(widths);
                            return true;
                        }
                        if let ::xrack_runtime::ControlAction::SortColumn(index) = action {
                            let Some(sort) = node.table_header.as_ref().and_then(|header| header.next_sort(index)) else { return false; };
                            self.__state.#field.set_if_changed(Some(sort));
                            if sort.descending { #descending } else { #ascending }
                            return true;
                        }
                        if let Some(view) = &node.table_selection {
                            let action = match action {
                                ::xrack_runtime::ControlAction::Select(index) => ::xrack_runtime::TableSelectionAction::Row {
                                    index, operation: ::xrack_runtime::RowSelection::Replace,
                                },
                                ::xrack_runtime::ControlAction::TableSelection(action) => action,
                                _ => return false,
                            };
                            let current = node.int("current-row").and_then(|value| <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(value).ok());
                            let Some(change) = self.__state.#selection.apply(#model, view, current, action) else { return false; };
                            if let Some(index) = change.current { #set #event }
                            if let Some(count) = change.selected_count { #selected }
                            return change.current.is_some() || change.column_changed || change.selected_count.is_some();
                        }
                    }
                } else {
                    TokenStream::new()
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        #source_guard
                        #header_actions
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        if !node.list.is_some_and(|list| index < list.row_count)
                            || ::xrack_runtime::ListMetrics::selection(node) == Some(#value) { return false; }
                        #set
                        #event
                        true
                    },
                });
            }
            "RadioGroup" | "TabWidget" | "TabBar" | "SwipeView" => {
                let radio = element.attr("kind")? == "RadioGroup";
                let mode = controlled_property(element).map(|(property, _)| property);
                let value = if mode == Some("current-value") {
                    quote!(__xr_label.clone())
                } else {
                    quote!(index as ::core::primitive::i64)
                };
                let set = self.set_control(element, &value)?;
                let event = if radio {
                    self.typed_event(element, "selected", &quote!(__xr_label))?
                } else {
                    self.typed_event(element, "changed", &quote!(index as ::core::primitive::i64))?
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let Some(__xr_item) = ::xrack_runtime::group_item(node, index) else { return false; };
                        if ::xrack_runtime::group_index(node) == Some(index) { return false; }
                        let __xr_label = __xr_item.text("text").unwrap_or("").to_owned();
                        #set
                        #event
                        true
                    },
                });
            }
            "SearchField" => {
                let set = self.set_control(element, &quote!(__xr_text.clone()))?;
                let accepted = self.typed_event(element, "accepted", &quote!(__xr_text.clone()))?;
                let edited = self.typed_event(element, "edited", &quote!(__xr_text.clone()))?;
                let selected = self.typed_event(element, "selected", &quote!(__xr_text.clone()))?;
                let search = self.typed_event(element, "search-triggered", &quote!(__xr_text))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        use ::xrack_runtime::ControlAction;
                        let current = ::xrack_runtime::text_input_value(node);
                        let __xr_text = match action {
                            ControlAction::SubmitSearch => {
                                let __xr_text = current.to_owned();
                                #accepted
                                __xr_text
                            }
                            ControlAction::Select(index) => {
                                let Some(value) = ::xrack_runtime::search_suggestion(node, index) else { return false; };
                                let __xr_text = value.to_owned();
                                if value != current { #set #edited }
                                #selected
                                __xr_text
                            }
                            ControlAction::ClearSearch if !node.boolean("read-only", false) && !current.is_empty() =>
                                return self.__edit_path(path, ::std::string::String::new()),
                            _ => return false,
                        };
                        #search
                        true
                    },
                });
            }
            "DelayButton" => {
                let current = self.control_value(element, "checked", Type::Bool)?;
                let set = self.set_control(element, &quote!(__xr_checked))?;
                let activated = self.typed_event_values(element, "activated", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::DelayButton(__xr_checked) = action else { return false; };
                        if #current == __xr_checked { return false; }
                        #set
                        if __xr_checked { #activated }
                        true
                    },
                });
            }
            "Tumbler" => {
                let set = self.set_control(element, &quote!(__xr_index))?;
                let changed = self.typed_event(element, "changed", &quote!(__xr_index))?;
                let source_guard = if let Some(binding) =
                    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("model"))
                {
                    let model = self.visual_value(binding, &self.visuals)?;
                    quote! {
                        if node.tumbler_items.as_ref() != Some(&::xrack_runtime::TumblerItems::new(#model)) {
                            return false;
                        }
                    }
                } else {
                    TokenStream::new()
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::PickTumbler { index, row } = action else { return false; };
                        #source_guard
                        let wheel = ::xrack_runtime::Tumbler::from_node(node);
                        let __xr_index = index as ::core::primitive::i64;
                        if wheel.row_key(index) != Some(row) || node.int("current-index") == Some(__xr_index) {
                            return false;
                        }
                        #set
                        #changed
                        true
                    },
                });
            }
            "Calendar" => {
                let set_date = self.set_control_property(element, "date", &quote!(__xr_date))?;
                let set_month =
                    self.set_control_property(element, "display-month", &quote!(__xr_month))?;
                let date_changed = self.typed_event(element, "changed", &quote!(__xr_date))?;
                let month_changed =
                    self.typed_event(element, "month-changed", &quote!(__xr_month))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        use ::xrack_runtime::CalendarAction;
                        let ::xrack_runtime::ControlAction::Calendar(action) = action else { return false; };
                        let calendar = ::xrack_runtime::CalendarState::from_node(node);
                        let (__xr_date, __xr_month) = match action {
                            CalendarAction::Select(date) if calendar.allows(date) => (Some(date), date.first_of_month()),
                            CalendarAction::Browse(month) if calendar.allows_month(month) => (None, month),
                            _ => return false,
                        };
                        let date_changed = __xr_date.is_some_and(|date| node.date("date") != Some(date));
                        let month_changed = __xr_month != calendar.month;
                        if let Some(__xr_date) = __xr_date.filter(|_| date_changed) { #set_date }
                        if month_changed { #set_month }
                        if let Some(__xr_date) = __xr_date.filter(|_| date_changed) { #date_changed }
                        if month_changed { #month_changed }
                        date_changed || month_changed
                    },
                });
            }
            "TimePicker" => {
                let current = self.control_value(element, "time", Type::Time)?;
                let options = self.time_picker_options(element)?;
                let set = self.set_control(element, &quote!(__xr_time))?;
                let changed = self.typed_event(element, "changed", &quote!(__xr_time))?;
                out.extend(quote! {
                    [#(#path,)* slot] => {
                        let Some(__xr_time) = (#options).select(#current, *slot, node, action) else { return false; };
                        #set
                        #changed
                        true
                    },
                });
            }
            kind @ ("TimePickerPopup" | "DatePickerPopup" | "ColorPickerPopup") => {
                let popup = picker_field(element_key(element)?);
                let open = self.control_value(element, "open", Type::Bool)?;
                let close = self.set_control_property(element, "open", &quote!(false))?;
                let (property, _) = picker_value(kind).ok_or("missing picker value")?;
                let set = self.set_control_property(element, property, &quote!(__xr_value))?;
                let accepted = self.typed_event(element, "accepted", &quote!(__xr_value))?;
                let canceled = self.typed_event_values(element, "canceled", &[])?;
                let finish = match kind {
                    "TimePickerPopup" => {
                        let options = self.time_picker_options(element)?;
                        out.extend(quote! {
                            [#(#path,)* 0, slot] => (#open) && self.__state.#popup.edit(#options, *slot, node, action),
                        });
                        quote!(self.__state.#popup.finish(node))
                    }
                    "DatePickerPopup" => {
                        let bounds = self.date_picker_bounds(element)?;
                        out.extend(quote! {
                            [#(#path,)* 0] => (#open) && self.__state.#popup.edit(#bounds, node, action),
                        });
                        quote!(self.__state.#popup.finish(#bounds, node))
                    }
                    _ => {
                        let show_alpha =
                            self.binding_value_or(element, "show-alpha", quote!(false))?;
                        out.extend(quote! {
                            [#(#path,)* 0, ..] => {
                                let ::xrack_runtime::ControlAction::Spin(action) = action else { return false; };
                                (#open) && self.__state.#popup.spin(#show_alpha, node, action)
                            },
                        });
                        quote!(self.__state.#popup.finish(node))
                    }
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Dialog(role) = action else { return false; };
                        if !(#open) { return false; }
                        let Some(__xr_value) = #finish else { return false; };
                        #close
                        match role {
                            ::xrack_runtime::DialogRole::Accept => { #set #accepted }
                            ::xrack_runtime::DialogRole::Reject => { #canceled }
                        }
                        true
                    },
                });
            }
            "ComboBox" if record_combo_binding(element).is_some() => {
                let binding = record_combo_binding(element).expect("record combo binding");
                let body = self.record_combo_actions(element, binding)?;
                out.extend(quote!([#(#path),*] => { #body },));
            }
            "ComboBox" => {
                let mode = controlled_property(element).map(|(property, _)| property);
                let value = if mode == Some("current-value") {
                    quote!(__xr_choice.clone())
                } else {
                    quote!(index as ::core::primitive::i64)
                };
                let set = self.set_control(element, &value)?;
                let selected =
                    self.typed_event(element, "selected", &quote!(__xr_choice.clone()))?;
                let (select_text, accept_text) = if combo_editor(element) {
                    let default = default_combo_edit_text(element);
                    let field = control_field(element_key(element)?, "edit-text");
                    let select_text = if default {
                        quote!(self.__state.#field.reset();)
                    } else {
                        self.set_control_property(
                            element,
                            "edit-text",
                            &quote!(__xr_choice.clone()),
                        )?
                    };
                    let keep_custom_text = if default {
                        quote! {
                            if __xr_match.is_some() {
                                self.__state.#field.reset();
                            } else {
                                self.__state.#field.set_if_changed(__xr_text.clone());
                            }
                        }
                    } else {
                        TokenStream::new()
                    };
                    let accepted =
                        self.typed_event(element, "accepted", &quote!(__xr_text.clone()))?;
                    let value = if mode == Some("current-value") {
                        quote!(__xr_text.clone())
                    } else {
                        quote!(__xr_match.map_or(-1, |index| index as ::core::primitive::i64))
                    };
                    let accept_selection = self.set_control(element, &value)?;
                    (
                        quote! { if ::xrack_runtime::combo_editable(node) { #select_text } },
                        quote! {
                            if action == ::xrack_runtime::ControlAction::AcceptComboText {
                                if !::xrack_runtime::combo_editable(node) { return false; }
                                let __xr_text = ::xrack_runtime::text_input_value(node).to_owned();
                                let __xr_match = node.strings("model").iter().position(|item| *item == __xr_text);
                                #accept_selection
                                #keep_custom_text
                                #accepted
                                return true;
                            }
                        },
                    )
                } else {
                    (TokenStream::new(), TokenStream::new())
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        #accept_text
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let Some(__xr_choice) = node.strings("model").get(index) else { return false; };
                        #set
                        #select_text
                        #selected
                        true
                    },
                });
            }
            "PageIndicator" => {
                let value = quote!(__xr_index);
                let set = self.set_control(element, &value)?;
                let changed = self.typed_event(element, "changed", &value)?;
                let current = self.control_value(element, "current-index", Type::Int)?;
                let count = children(element, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("count"))
                    .map(|binding| self.visual_value(binding, &self.visuals))
                    .transpose()?
                    .unwrap_or_else(|| quote!(0_i64));
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let Ok(__xr_index) = ::core::primitive::i64::try_from(index) else { return false; };
                        if __xr_index >= #count || __xr_index == #current { return false; }
                        #set
                        #changed
                        true
                    },
                });
            }
            "StackView" => {
                let method = stack_navigate_method(element_key(element)?);
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::StackBack { revision } = action else { return false; };
                        if !node.stack.as_ref().is_some_and(|stack| stack.revision() == revision) {
                            return false;
                        }
                        self.#method(Some(revision), ::xrack_runtime::StackNavigation::Pop)
                    },
                });
            }
            _ => {}
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.control_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn instance_inputs(
        &self,
        instance: OperationRef<'a>,
        repeat: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let mut inputs = TokenStream::new();
        let (context, parameters) = if let Some(repeat) = repeat {
            let item = ident(repeat.attr("item")?);
            let mut parameters = quote!(let #item = || __context.item(););
            let index = repeat.attr("index")?;
            if !index.is_empty() {
                let index = ident(index);
                parameters.extend(quote!(let #index = || __context.index();));
            }
            (quote!(let __context = __context.clone();), parameters)
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        for binding in children(instance, "xr.bind") {
            let field = property_field("root", binding.attr("name")?);
            let expression = self.expr_in(
                self.operand(binding, 0)?,
                ExpressionContext {
                    receiver: &quote!(__ui),
                    repeat_parameters: repeat.is_some(),
                    hoist_repeat_subexpressions: false,
                },
            )?;
            inputs.extend(quote! {
                #field: ::core::option::Option::Some({
                    let __weak = __parent.clone();
                    #context
                    ::std::boxed::Box::new(move || {
                        let __ui = Self {
                            __state: __weak.upgrade().expect("parent of a bound component"),
                        };
                        #parameters
                        #expression
                    })
                }),
            });
        }
        Ok(inputs)
    }

    fn component(&self, component: OperationRef<'a>) -> Result<TokenStream, String> {
        if component.name() != "xr.component" {
            return Err(format!(
                "unexpected top-level operation {}",
                component.name()
            ));
        }
        let component_name = component.attr("sym_name")?;
        let name = ident(component_name);
        let state = format_ident!("__State_{}", rust_name(component_name));
        let inputs = format_ident!("__Inputs_{}", rust_name(component_name));
        let visibility = if component.attr("exported")? == "true" {
            quote!(pub)
        } else {
            TokenStream::new()
        };
        let root = children(component, "xr.element")
            .next()
            .ok_or("component has no root element")?;
        let mut visual_declarations = TokenStream::new();
        let component_key = rust_name(component_name);
        self.visuals
            .declare(&component_key, &mut visual_declarations);
        let visuals = self.visuals.state_name(&component_key);
        let mut fields = quote! {
            __scene: ::xrack_runtime::Property<::std::rc::Rc<::xrack_runtime::Node>>,
            __visuals: #visuals,
        };
        let mut defaults = quote! {
            __scene: ::core::default::Default::default(),
            __visuals: ::core::default::Default::default(),
        };
        let mut methods = TokenStream::new();

        let mut input_fields = TokenStream::new();
        let properties = children(component, "xr.property")
            .map(|p| ("root", p))
            .chain(self.element_properties.iter().copied());
        for (owner, property) in properties {
            let property_name = property.attr("name")?;
            let field = property_field(owner, property_name);
            let init = self.operand(property, 0)?;
            let value_type = value_type(init)?;
            let getter = getter(owner, property_name, &value_type);
            let visibility = property.attr("visibility")?;
            let getter_visibility = if owner == "root" && visibility != "private" {
                quote!(pub)
            } else {
                TokenStream::new()
            };
            let rust_type = ty(&value_type);
            let host_type = host_ty(&value_type);
            let expression = self.expr(init)?;
            fields.extend(quote!(#field: ::xrack_runtime::Property<#rust_type>,));
            methods.extend(quote! {
                #getter_visibility fn #getter(&self) -> #rust_type {
                    self.__state.#field.get(|| #expression)
                }
            });
            if owner == "root" && visibility != "private" && value_type.list_item().is_some() {
                let snapshot = format_ident!("get_{}", rust_name(property_name));
                methods.extend(quote! {
                    pub fn #snapshot(&self) -> #host_type { self.#getter().snapshot() }
                });
            }
            if owner == "root" && matches!(visibility, "in" | "in-out") {
                let setter = format_ident!("set_{}", rust_name(property_name));
                let reset = format_ident!("reset_{}", rust_name(property_name));
                input_fields.extend(quote! {
                    #field: ::core::option::Option<::std::boxed::Box<dyn ::core::ops::Fn() -> #rust_type>>,
                });
                defaults.extend(quote!(#field: ::xrack_runtime::Property::new(__inputs.#field),));
                let input = from_host(quote!(value), &value_type);
                methods.extend(quote! {
                    pub fn #setter(&mut self, value: #host_type) {
                        self.__state.#field.set_if_changed(#input);
                    }
                    pub fn #reset(&mut self) {
                        self.__state.#field.reset();
                    }
                });
                if value_type.list_item().is_some() {
                    let bind_model = format_ident!("bind_model_{}", rust_name(property_name));
                    methods.extend(quote! {
                        pub fn #bind_model(&mut self, model: #rust_type) {
                            self.__state.#field.set_if_changed(model);
                        }
                    });
                }
            } else {
                defaults.extend(quote!(#field: ::core::default::Default::default(),));
            }
        }

        for key in &self.record_combos {
            let items = combo_field(key, "items");
            let snapshot = combo_field(key, "snapshot");
            fields.extend(quote! {
                #items: ::xrack_runtime::Property<::xrack_runtime::ComboItems>,
                #snapshot: ::xrack_runtime::Property<::xrack_runtime::ComboModel>,
            });
            defaults.extend(quote! {
                #items: ::core::default::Default::default(),
                #snapshot: ::core::default::Default::default(),
            });
        }
        for (key, property, value_type) in &self.unlinked_controls {
            let field = control_field(key, property);
            let rust_type = ty(value_type);
            fields.extend(quote!(#field: ::xrack_runtime::Property<#rust_type>,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        for (&key, &action) in &self.actions {
            let identity = action_key_field(key);
            fields.extend(quote!(#identity: ::xrack_runtime::ActionKey,));
            defaults.extend(quote!(#identity: ::core::default::Default::default(),));
            let render = action_render_method(key);
            let activate = action_activate_method(key);
            let binding = |name| -> Result<Option<TokenStream>, String> {
                children(action, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok(name))
                    .map(|binding| self.visual_value(binding, &self.visuals))
                    .transpose()
            };
            let enabled = binding("enabled")?.unwrap_or_else(|| quote!(true));
            let checkable = binding("checkable")?.unwrap_or_else(|| quote!(false));
            let checked = self.control_value(action, "checked", Type::Bool)?;
            let toggle = self.set_control(action, &quote!(!(#checked)))?;
            let activated = self.typed_event_values(action, "activated", &[])?;
            let mut labels = TokenStream::new();
            for name in ["text", "shortcut", "icon"] {
                if let Some(value) = binding(name)? {
                    labels.extend(quote!(node.properties.insert(#name, ::xrack_runtime::Value::from(#value));));
                }
            }
            methods.extend(quote! {
                fn #render(&self, node: &mut ::xrack_runtime::Node) {
                    node.action = Some(self.__state.#identity);
                    node.properties.insert("enabled", ::xrack_runtime::Value::Bool(#enabled));
                    node.properties.insert("checkable", ::xrack_runtime::Value::Bool(#checkable));
                    node.properties.insert("checked", ::xrack_runtime::Value::Bool(#checked));
                    #labels
                }
                fn #activate(&self) -> bool {
                    if !(#enabled) { return false; }
                    if #checkable { #toggle }
                    #activated
                    true
                }
            });
        }
        for &dialog in &self.native_dialogs {
            let key = element_key(dialog)?;
            let slot = native_dialog_member(key, "slot");
            let register = native_dialog_member(key, "register");
            let open = native_dialog_member(key, "open");
            let finish = native_dialog_member(key, "finish");
            let enabled = self.binding_value_or(dialog, "enabled", quote!(true))?;
            let string =
                |name| self.binding_value_or(dialog, name, quote!(::std::string::String::new()));
            let title = string("title")?;
            let (options, handlers) = if dialog.attr("kind") == Ok("FileDialog") {
                let mode = self.binding_value_or(
                    dialog,
                    "mode",
                    quote!(::xrack_runtime::FileDialogMode::OpenFile),
                )?;
                let directory = string("directory")?;
                let file_name = string("file-name")?;
                let name_filters = self.binding_value_or(
                    dialog,
                    "name-filters",
                    quote!(::xrack_runtime::VecModel::<::std::string::String>::default()),
                )?;
                let accepted = self.typed_event(
                    dialog,
                    "accepted",
                    &quote!(::xrack_runtime::VecModel::from(paths)),
                )?;
                let canceled = self.typed_event_values(dialog, "canceled", &[])?;
                (
                    quote!(::xrack_runtime::NativeDialogOptions::File(::xrack_runtime::FileDialogOptions {
                        mode: #mode,
                        title: #title,
                        directory: #directory,
                        file_name: #file_name,
                        name_filters: (#name_filters).snapshot(),
                    })),
                    quote! {
                        ::xrack_runtime::NativeDialogResult::Files(paths) => { #accepted }
                        ::xrack_runtime::NativeDialogResult::Canceled => { #canceled }
                    },
                )
            } else {
                let level = self.binding_value_or(
                    dialog,
                    "level",
                    quote!(::xrack_runtime::MessageLevel::Info),
                )?;
                let buttons = self.binding_value_or(
                    dialog,
                    "buttons",
                    quote!(::xrack_runtime::MessageButtons::Ok),
                )?;
                let text = string("text")?;
                let informative_text = string("informative-text")?;
                let accepted = self.typed_event_values(dialog, "accepted", &[])?;
                let rejected = self.typed_event_values(dialog, "rejected", &[])?;
                let finished = self.typed_event(dialog, "finished", &quote!(__xr_button))?;
                (
                    quote!(::xrack_runtime::NativeDialogOptions::Message(::xrack_runtime::MessageDialogOptions {
                        level: #level,
                        buttons: #buttons,
                        title: #title,
                        text: #text,
                        informative_text: #informative_text,
                    })),
                    quote! {
                        ::xrack_runtime::NativeDialogResult::Button(__xr_button) => {
                            if ::core::matches!(__xr_button, ::xrack_runtime::StandardButtonKind::Ok | ::xrack_runtime::StandardButtonKind::Yes) {
                                #accepted
                            } else {
                                #rejected
                            }
                            #finished
                        }
                    },
                )
            };
            let failed = self.typed_event(dialog, "failed", &quote!(message))?;
            fields.extend(quote!(#slot: ::xrack_runtime::NativeDialogSlot,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #open(&self) {
                    if #enabled {
                        self.__state.#slot.open(|| #options);
                    }
                }
                fn #register(&self) -> Option<::xrack_runtime::NativeDialogRegistration> {
                    let weak = self.as_weak();
                    self.__state.#slot.register(move |id, result| weak.upgrade().is_some_and(|ui| ui.#finish(id, result)))
                }
                fn #finish(&self, id: u64, result: ::xrack_runtime::NativeDialogResult) -> bool {
                    if !self.__state.#slot.finish(id) { return false; }
                    match result {
                        #handlers
                        ::xrack_runtime::NativeDialogResult::Failed(message) => { #failed }
                        ::xrack_runtime::NativeDialogResult::Abandoned => {}
                        _ => return false,
                    }
                    true
                }
            });
        }
        for &timer in &self.timers {
            let key = element_key(timer)?;
            let slot = timer_member(key, "slot");
            let configure = timer_member(key, "config");
            let register = timer_member(key, "register");
            let command = timer_member(key, "command");
            let trigger = timer_member(key, "trigger");
            let running = self.control_value(timer, "running", Type::Bool)?;
            let start = self.set_control(timer, &quote!(true))?;
            let stop = self.set_control(timer, &quote!(false))?;
            let interval = self.binding_value_or(timer, "interval", quote!(1000_i64))?;
            let repeat = self.binding_value_or(timer, "repeat", quote!(false))?;
            let immediate = self.binding_value_or(timer, "triggered-on-start", quote!(false))?;
            let triggered = self.typed_event_values(timer, "triggered", &[])?;
            fields.extend(quote!(#slot: ::xrack_runtime::TimerSlot,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #configure(&self) -> Option<::xrack_runtime::TimerConfig> {
                    if !(#running) { return None; }
                    ::xrack_runtime::TimerConfig::new(#interval, #repeat, #immediate)
                }
                fn #register(&self) -> Option<::xrack_runtime::TimerRegistration> {
                    let config = self.#configure()?;
                    let weak = self.as_weak();
                    Some(self.__state.#slot.register(config, move |tick| weak.upgrade().is_some_and(|ui| ui.#trigger(tick))))
                }
                fn #trigger(&self, tick: ::xrack_runtime::TimerTick) -> bool {
                    if self.__state.#slot.revision() != tick.revision || self.#configure() != Some(tick.config) {
                        return false;
                    }
                    if tick.completed { #stop }
                    #triggered
                    true
                }
                fn #command(&self, command: ::xrack_runtime::TimerCommand) {
                    use ::xrack_runtime::TimerCommand;
                    match command {
                        TimerCommand::Start if #running => {}
                        TimerCommand::Start | TimerCommand::Restart => {
                            self.__state.#slot.restart();
                            #start
                        }
                        TimerCommand::Stop => { #stop }
                    }
                }
            });
        }
        for &popup in &self.popups {
            let method = popup_method(element_key(popup)?);
            let current = self.control_value(popup, "open", Type::Bool)?;
            let set = self.set_control(popup, &quote!(__xr_open))?;
            let opened = self.typed_event_values(popup, "opened", &[])?;
            let closed = self.typed_event_values(popup, "closed", &[])?;
            methods.extend(quote! {
                fn #method(&self, __xr_open: bool) -> bool {
                    if __xr_open == #current { return false; }
                    #set
                    if __xr_open { #opened } else { #closed }
                    true
                }
            });
        }
        for key in &self.tables {
            let field = table_sort_field(key);
            let widths = table_widths_field(key);
            let selection = table_selection_field(key);
            fields.extend(quote!(#field: ::xrack_runtime::Property<::core::option::Option<::xrack_runtime::TableSort>>,));
            fields.extend(quote!(#widths: ::xrack_runtime::Property<::std::vec::Vec<::core::option::Option<::core::primitive::u32>>>,));
            fields.extend(quote!(#selection: ::xrack_runtime::TableSelection,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            defaults.extend(quote!(#widths: ::core::default::Default::default(),));
            defaults.extend(quote!(#selection: ::core::default::Default::default(),));
        }
        for (key, ty) in &self.picker_popups {
            let field = picker_field(key);
            let state = match ty {
                Type::Date => quote!(::xrack_runtime::DatePopupState),
                Type::Time => quote!(::xrack_runtime::TimePopupState),
                Type::Color => quote!(::xrack_runtime::ColorPopupState),
                _ => return Err("unsupported picker state".into()),
            };
            fields.extend(quote!(#field: #state,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        for key in &self.splits {
            let field = split_weights_field(key);
            fields.extend(quote!(#field: ::xrack_runtime::Property<::core::option::Option<::std::sync::Arc<[::core::option::Option<::core::primitive::f32>]>>>,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        for key in &self.trees {
            let field = tree_state_field(key);
            fields.extend(quote!(#field: ::xrack_runtime::TreeState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        let mut stack_reads = TokenStream::new();
        let mut stack_writes = TokenStream::new();
        for (&key, &view) in &self.stacks {
            let field = stack_field(key);
            let snapshot = stack_snapshot_method(key);
            let navigate = stack_navigate_method(key);
            let routes = scene_children(view)
                .map(|page| page.attr("id"))
                .collect::<Result<Vec<_>, _>>()?;
            let initial = children(view, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("initial-page"))
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .unwrap_or_else(|| quote!(::std::string::String::new()));
            let changed = self.typed_event_values(
                view,
                "changed",
                &[
                    quote!(__xr_snapshot.current().unwrap_or("").to_owned()),
                    quote!(__xr_snapshot.depth() as ::core::primitive::i64),
                ],
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::StackState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #snapshot(&self) -> ::std::sync::Arc<::xrack_runtime::StackSnapshot> {
                    self.__state.#field.snapshot(&[#(#routes),*], || #initial)
                }
                fn #navigate(&self, expected: Option<::core::primitive::u64>, action: ::xrack_runtime::StackNavigation<'_>) -> bool {
                    self.__state.#field.navigate(
                        &[#(#routes),*], || #initial, expected, action, |__xr_snapshot| { #changed },
                    )
                }
            });
            let id = view.attr("id")?;
            let id = if id.is_empty() && key == "root" {
                "root"
            } else {
                id
            };
            if !id.is_empty() {
                stack_reads.extend(quote!(#id => Some(self.#snapshot()),));
                stack_writes.extend(quote!(#id => self.#navigate(None, action),));
            }
        }
        if !stack_reads.is_empty() {
            methods.extend(quote! {
                pub fn stack_snapshot(&self, id: &str) -> Option<::std::sync::Arc<::xrack_runtime::StackSnapshot>> {
                    match id { #stack_reads _ => None }
                }
                pub fn navigate_stack(&mut self, id: &str, action: ::xrack_runtime::StackNavigation<'_>) -> bool {
                    match id { #stack_writes _ => false }
                }
            });
        }
        let mut instance_routes = TokenStream::new();
        for (&key, &instance) in &self.instances {
            let field = instance_field(key);
            let component_name = instance.attr("component")?;
            let component = ident(component_name);
            let child_inputs = format_ident!("__Inputs_{}", rust_name(component_name));
            let bindings = self.instance_inputs(instance, None)?;
            fields.extend(quote!(#field: #component,));
            defaults.extend(quote! {
                #field: #component::__new(#child_inputs {
                    #bindings
                    ..::core::default::Default::default()
                }),
            });
            instance_routes.extend(quote!(#key => self.__state.#field.__dispatch(id, event),));
        }

        for callback in children(component, "xr.callback") {
            let callback_name = rust_name(callback.attr("name")?);
            let field = format_ident!("__cb_{callback_name}");
            let on = format_ident!("on_{callback_name}");
            let invoke = format_ident!("invoke_{callback_name}");
            let invoke_internal = format_ident!("__invoke_{callback_name}");
            let params: Vec<Parameter> =
                serde_json::from_str(callback.attr("parameters")?).map_err(|e| e.to_string())?;
            let types: Vec<_> = params.iter().map(|p| host_ty(&p.ty)).collect();
            let args: Vec<_> = params.iter().map(|p| ident(&p.name)).collect();
            let statements = self.statements(callback)?;
            let (track, notify) = if callback.attr("pure") == Ok("true") {
                let signal = format_ident!("__cb_signal_{callback_name}");
                fields.extend(quote!(#signal: ::xrack_runtime::ChangeSignal,));
                defaults.extend(quote!(#signal: ::core::default::Default::default(),));
                (
                    quote!(self.__state.#signal.track();),
                    quote!(self.__state.#signal.notify();),
                )
            } else {
                (TokenStream::new(), TokenStream::new())
            };
            let (call_trait, host) = if callback.attr("pure") == Ok("true") {
                (
                    quote!(::core::ops::Fn),
                    quote!(&*self.__state.#field.borrow()),
                )
            } else {
                (
                    quote!(::core::ops::FnMut),
                    quote!(&mut *self.__state.#field.borrow_mut()),
                )
            };
            let (returns, fallback) = match children(callback, "xr.return").next() {
                Some(result) => {
                    let value = self.operand(result, 0)?;
                    let value_type = value_type(value)?;
                    let rust_type = host_ty(&value_type);
                    (
                        quote!(-> #rust_type),
                        to_host(self.expr(value)?, &value_type),
                    )
                }
                None => (TokenStream::new(), TokenStream::new()),
            };
            fields.extend(quote! {
                #field: ::core::cell::RefCell<::core::option::Option<::std::boxed::Box<dyn #call_trait(#(#types),*) #returns>>>,
            });
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                pub fn #on(&mut self, callback: impl #call_trait(#(#types),*) #returns + 'static) {
                    self.__state.#field.replace(::core::option::Option::Some(::std::boxed::Box::new(callback)));
                    #notify
                }
                pub fn #invoke(&mut self #(, #args: #types)*) #returns {
                    self.#invoke_internal(#(#args),*)
                }
                fn #invoke_internal(&self #(, #args: #types)*) #returns {
                    #track
                    #statements
                    if let ::core::option::Option::Some(__callback) = #host {
                        __callback(#(#args),*)
                    } else {
                        #fallback
                    }
                }
            });
        }

        let secondary_windows = if self.secondary_windows.is_empty() {
            TokenStream::new()
        } else {
            let mut registrations = TokenStream::new();
            for &window in &self.secondary_windows {
                let key = element_key(window)?;
                let child = children(window, "xr.instance")
                    .next()
                    .ok_or("missing secondary window component")?;
                let child = instance_field(element_key(child)?);
                let slot = secondary_window_member(key, "slot");
                let register = secondary_window_member(key, "register");
                let visibility = secondary_window_member(key, "visible");
                let close = secondary_window_member(key, "close");
                let visible = self.control_value(window, "visible", Type::Bool)?;
                let set = self.set_control_property(window, "visible", &quote!(visible))?;
                fields.extend(quote!(#slot: ::xrack_runtime::SecondaryWindowSlot,));
                defaults.extend(quote!(#slot: ::core::default::Default::default(),));
                methods.extend(quote! {
                    fn #visibility(&self, visible: ::core::primitive::bool) { #set }

                    fn #close(&self) {
                        if #visible { self.__state.#child.__state.__window_close.set(true); }
                    }

                    fn #register(&self) -> Option<::xrack_runtime::SecondaryWindowRegistration> {
                        if !(#visible) { return None; }
                        let child = self.__state.#child.as_weak();
                        let owner = self.as_weak();
                        Some(self.__state.#slot.register(
                            move || child.upgrade().map(|child| ::std::boxed::Box::new(child) as ::std::boxed::Box<dyn ::xrack_runtime::Component>),
                            move || { if let Some(owner) = owner.upgrade() { owner.#visibility(false); } },
                        ))
                    }
                });
                registrations.extend(quote!(windows.extend(self.#register());));
            }
            fields.extend(quote!(__secondary_windows: ::xrack_runtime::Property<::std::rc::Rc<[::xrack_runtime::SecondaryWindowRegistration]>>,));
            defaults.extend(quote!(__secondary_windows: ::core::default::Default::default(),));
            quote! {
                fn secondary_windows(&self) -> Option<::std::rc::Rc<[::xrack_runtime::SecondaryWindowRegistration]>> {
                    Some(self.__state.__secondary_windows.get(|| {
                        let mut windows = ::std::vec::Vec::new();
                        #registrations
                        windows.into()
                    }))
                }
            }
        };

        let window_lifecycle = if root.attr("kind") == Ok("Window") {
            fields.extend(quote!(__window_close: ::core::cell::Cell<::core::primitive::bool>,));
            defaults.extend(quote!(__window_close: ::core::default::Default::default(),));
            let close = if let Some(handler) = children(root, "xr.handler")
                .find(|handler| handler.attr("name") == Ok("close-requested"))
            {
                let statements = self.statements(handler)?;
                let result = children(handler, "xr.return")
                    .next()
                    .ok_or("missing close-requested result")?;
                let value = self.expr(self.operand(result, 0)?)?;
                quote!(#statements #value)
            } else {
                quote!(true)
            };
            quote! {
                #[allow(unused_parens)]
                fn close_requested(&mut self) -> ::core::primitive::bool {
                    #close
                }

                fn take_window_close_request(&mut self) -> ::core::primitive::bool {
                    self.__state.__window_close.replace(false)
                }
            }
        } else {
            TokenStream::new()
        };

        let mut render = TokenStream::new();
        let node = self.render(root, &self.visuals, &mut render, &mut 0)?;
        let mut dispatch = TokenStream::new();
        let mut path_dispatch = TokenStream::new();
        self.dispatch_arms(root, &mut Vec::new(), &mut dispatch, &mut path_dispatch)?;
        let mut edits = TokenStream::new();
        self.edit_arms(root, &mut Vec::new(), &mut edits)?;
        let mut ranges = TokenStream::new();
        self.range_arms(root, &mut Vec::new(), &mut ranges)?;
        let mut controls = TokenStream::new();
        self.control_arms(root, &mut Vec::new(), &mut controls)?;
        let mut drag = TokenStream::new();
        self.drag_arms(root, &mut Vec::new(), &mut drag)?;
        let mut lists = TokenStream::new();
        self.list_arms(root, &mut Vec::new(), &mut lists)?;
        let mut delegate_events = TokenStream::new();
        self.delegate_event_arms(root, &mut Vec::new(), &mut delegate_events)?;
        Ok(quote! {
            #visual_declarations

            #[allow(non_camel_case_types, dead_code)]
            #visibility struct #name {
                __state: ::std::rc::Rc<#state>,
            }

            #[allow(non_camel_case_types, non_snake_case)]
            struct #state { #fields }

            #[allow(non_camel_case_types)]
            #[derive(::core::default::Default)]
            struct #inputs { #input_fields }

            impl ::core::default::Default for #name {
                fn default() -> Self { Self::new() }
            }

            #[allow(dead_code, unused_mut, unused_variables, unused_parens, clippy::clone_on_copy)]
            impl #name {
                pub fn new() -> Self {
                    Self::__new(::core::default::Default::default())
                }

                pub fn as_weak(&self) -> ::xrack_runtime::WeakComponent<Self> {
                    ::xrack_runtime::WeakComponent::new(&self.__state, |__state| Self { __state })
                }

                fn __new(__inputs: #inputs) -> Self {
                    Self {
                        __state: ::std::rc::Rc::new_cyclic(
                            |__parent: &::std::rc::Weak<#state>| #state { #defaults },
                        ),
                    }
                }

                #methods

                pub fn render(&self) -> ::xrack_runtime::Node {
                    self.__render_shared().as_ref().clone()
                }

                fn __render_shared(&self) -> ::std::rc::Rc<::xrack_runtime::Node> {
                    self.__state.__scene.get(|| ::std::rc::Rc::new(self.__render_scene()))
                }

                fn __render_scene(&self) -> ::xrack_runtime::Node {
                    #render
                    #node
                }

                fn __list_rows(
                    &self,
                    __path: &[::core::primitive::usize],
                    __range: ::core::ops::Range<::core::primitive::usize>,
                ) -> ::std::vec::Vec<::xrack_runtime::Node> {
                    match __path {
                        #lists
                        _ => ::std::vec::Vec::new(),
                    }
                }

                fn __route_delegate(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: &::xrack_runtime::RoutedEvent<'_>,
                ) -> ::core::option::Option<::core::primitive::bool> {
                    match path {
                        #delegate_events
                        _ => None,
                    }
                }

                pub fn dispatch(
                    &mut self,
                    element_id: &::core::primitive::str,
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    self.__dispatch(element_id, event)
                }

                fn __dispatch(
                    &self,
                    element_id: &::core::primitive::str,
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    if let ::core::option::Option::Some((key, id)) = element_id.split_once('/') {
                        match key {
                            #instance_routes
                            _ => false,
                        }
                    } else {
                        self.__dispatch_local(element_id, event)
                    }
                }

                fn __dispatch_path(
                    &self,
                    path: &[::core::primitive::usize],
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    match path {
                        #path_dispatch
                        _ => false,
                    }
                }

                fn __control_path(
                    &self,
                    path: &[::core::primitive::usize],
                    node: &::xrack_runtime::Node,
                    action: ::xrack_runtime::ControlAction,
                ) -> ::core::primitive::bool {
                    match path {
                        #controls
                        _ => false,
                    }
                }

                fn __drag_path(&self, path: &[::core::primitive::usize], event: &::xrack_runtime::DragEvent) -> ::core::primitive::bool {
                    match path { #drag _ => false }
                }

                fn __range_path(&self, path: &[::core::primitive::usize], node: &::xrack_runtime::Node, action: ::xrack_runtime::RangeAction) -> ::core::primitive::bool {
                    match path {
                        #ranges
                        _ => false,
                    }
                }

                fn __edit_path(&self, path: &[::core::primitive::usize], text: ::std::string::String) -> ::core::primitive::bool {
                    match path {
                        #edits
                        _ => false,
                    }
                }

                fn __dispatch_local(
                    &self,
                    element_id: &::core::primitive::str,
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    match (element_id, event) {
                        #dispatch
                        _ => false,
                    }
                }
            }

            impl ::xrack_runtime::Component for #name {
                #window_lifecycle
                #secondary_windows

                fn render(&self) -> ::xrack_runtime::Node {
                    #name::render(self)
                }

                fn render_shared(&self) -> ::std::rc::Rc<::xrack_runtime::Node> {
                    self.__render_shared()
                }

                fn scene_dirty(&self) -> ::core::primitive::bool {
                    self.__state.__scene.is_dirty()
                }

                fn list_rows(
                    &self,
                    path: &[::core::primitive::usize],
                    rows: ::core::ops::Range<::core::primitive::usize>,
                ) -> ::std::vec::Vec<::xrack_runtime::Node> {
                    self.__list_rows(path, rows)
                }

                fn dispatch(
                    &mut self,
                    id: &::core::primitive::str,
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    #name::dispatch(self, id, event)
                }

                fn adjust_range_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: ::xrack_runtime::RangeAction,
                ) -> ::core::primitive::bool {
                    let ::core::option::Option::Some(target) = scene.interactive_at_path(path) else {
                        return false;
                    };
                    let node = match target.kind {
                        "Slider" | "Dial" => target,
                        "RangeThumb" => {
                            let Some((_, parent)) = path.split_last() else { return false; };
                            let Some(node) = scene.at_path(parent).filter(|node| node.kind == "RangeSlider") else { return false; };
                            node
                        }
                        _ => return false,
                    };
                    if let Some(result) = self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Range(action)) {
                        return result;
                    }
                    self.__range_path(path, node, action)
                }

                fn control_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: ::xrack_runtime::ControlAction,
                ) -> ::core::primitive::bool {
                    scene.control_target(path, action).is_some_and(|node| {
                        self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Control(action))
                            .unwrap_or_else(|| self.__control_path(path, node, action))
                    })
                }

                fn drag_event_at(&mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], event: &::xrack_runtime::DragEvent) -> ::core::primitive::bool {
                    let target = if event.is_terminal() { scene.at_path(path) } else { scene.interactive_at_path(path) };
                    target.is_some_and(|node| node.kind == event.target_kind()) && self
                        .__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Drag(event))
                        .unwrap_or_else(|| self.__drag_path(path, event))
                }

                fn resize_split_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    weights: &[(::core::primitive::usize, ::core::primitive::f32)],
                ) -> ::core::primitive::bool {
                    scene.interactive_at_path(path).is_some_and(|node| {
                        node.kind == "SplitView" && self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::ResizeSplit(weights)).unwrap_or(false)
                    })
                }

                fn edit_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    text: ::std::string::String,
                ) -> ::core::primitive::bool {
                    if scene.interactive_at_path(path).is_some_and(|node| {
                        (::xrack_runtime::is_text_input(node.kind) || ::xrack_runtime::combo_editable(node))
                            && !node.boolean("read-only", false)
                            && ::xrack_runtime::text_input_value(node) != text
                    }) {
                        self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Edit(&text))
                            .unwrap_or_else(|| self.__edit_path(path, text))
                    } else {
                        false
                    }
                }

                fn commit_table_cell(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    original: &::core::primitive::str,
                    text: ::std::string::String,
                ) -> ::core::primitive::bool {
                    scene.interactive_at_path(path).is_some_and(|node| {
                        ::xrack_runtime::editable_table_cell(node) && self.__route_delegate(
                            scene, path, &::xrack_runtime::RoutedEvent::CommitCell { original, text: &text },
                        ).unwrap_or(false)
                    })
                }

                fn dispatch_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    scene.interactive_at_path(path).is_some_and(|node| {
                        event != "accepted" || !::xrack_runtime::is_text_input(node.kind) || node.accepts_text_submission()
                    }) && self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Dispatch(event))
                        .unwrap_or_else(|| self.__dispatch_path(path, event))
                }
            }
        })
    }
}

/// Generate Rust tokens directly from verified MLIR operations and their SSA uses.
pub fn generate(module: &Module) -> Result<TokenStream, String> {
    generate_with_images(module, &BTreeMap::new())
}

pub struct EmbeddedImage {
    pub format: ImageFormat,
    pub size: [u32; 2],
    pub bytes: Vec<u8>,
}

pub fn generate_with_images(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
) -> Result<TokenStream, String> {
    generate_with_assets(module, images, &[])
}

pub fn generate_with_assets(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
) -> Result<TokenStream, String> {
    generate_with_resource_plans(module, images, fonts, &BTreeMap::new())
}

pub fn generate_with_resource_plans(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
    paths: &BTreeMap<String, EmbeddedPath>,
) -> Result<TokenStream, String> {
    let mut out = quote! {
        const _: () = assert!(
            ::xrack_runtime::CODEGEN_ABI_VERSION == 1,
            "xrackc and xrack-runtime have incompatible code generation interfaces"
        );
    };
    if !fonts.is_empty() {
        let bytes = fonts.iter().map(|font| Literal::byte_string(font));
        out.extend(quote! {
            static __XR_FONTS: ::xrack_runtime::EmbeddedFonts =
                ::xrack_runtime::EmbeddedFonts::new(&[#(#bytes),*]);
        });
    } else if let Some(font) = module.operations().find(|op| op.name() == "xr.font") {
        return Err(format!("unresolved font {:?}", font.attr("path")?));
    }
    let mut image_names = BTreeMap::new();
    for (index, (path, image)) in images.iter().enumerate() {
        let name = format_ident!("__XR_IMAGE_{index}");
        let variant = match image.format {
            ImageFormat::Png => quote!(Png),
            ImageFormat::Svg => quote!(Svg),
        };
        let [width, height] = image.size;
        let bytes = Literal::byte_string(&image.bytes);
        out.extend(quote! {
            static #name: ::xrack_runtime::Image = ::xrack_runtime::Image::Embedded {
                format: ::xrack_runtime::ImageFormat::#variant,
                bytes: #bytes,
                size: [#width, #height],
            };
        });
        image_names.insert(path.as_str(), name);
    }
    let mut path_names = BTreeMap::new();
    for (index, (commands, path)) in paths.iter().enumerate() {
        let name = format_ident!("__XR_PATH_{index}");
        out.extend(scene_plan::path_declaration(&name, commands, path));
        path_names.insert(commands.as_str(), name);
    }
    let mut records = BTreeMap::new();
    for op in module
        .operations()
        .filter(|op| op.name() == "xr.record_decl")
    {
        let name = op.attr("sym_name")?;
        let fields: Vec<Parameter> =
            serde_json::from_str(op.attr("fields")?).map_err(|err| err.to_string())?;
        let rust_name = ident(name);
        let names = fields.iter().map(|field| ident(&field.name));
        let types = fields.iter().map(|field| ty(&field.ty));
        out.extend(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone, Debug, Default, PartialEq)]
            pub struct #rust_name { #(pub #names: #types),* }
        });
        records.insert(name, fields);
    }
    for op in module
        .operations()
        .filter(|op| !matches!(op.name(), "xr.record_decl" | "xr.font"))
    {
        let mut generator = Generator {
            records: &records,
            instances: BTreeMap::new(),
            element_properties: Vec::new(),
            unlinked_controls: Vec::new(),
            record_combos: Vec::new(),
            picker_popups: Vec::new(),
            tables: Vec::new(),
            splits: Vec::new(),
            trees: Vec::new(),
            stacks: BTreeMap::new(),
            actions: BTreeMap::new(),
            timers: Vec::new(),
            native_dialogs: Vec::new(),
            secondary_windows: Vec::new(),
            popups: Vec::new(),
            visuals: VisualScope::default(),
            repeat_invariants: Vec::new(),
            repeat_subexpressions: Vec::new(),
            use_visual_plan: module.has_visual_cache_plan(),
            images: &image_names,
            paths: &path_names,
            with_fonts: !fonts.is_empty(),
        };
        generator.index(op)?;
        if let Some(root) = children(op, "xr.element").next() {
            let mut visuals = VisualScope::default();
            generator.index_visual_bindings(root, &mut visuals, &mut 0)?;
            for &action in generator.actions.values() {
                generator.index_visual_bindings(action, &mut visuals, &mut 0)?;
            }
            for &timer in &generator.timers {
                generator.index_visual_bindings(timer, &mut visuals, &mut 0)?;
            }
            for &binding in &generator.repeat_invariants {
                generator.index_visual_binding(binding, &mut visuals)?;
            }
            for &(group, definition) in &generator.repeat_subexpressions {
                visuals.index_value(VisualCacheKey::RepeatSubexpression(group), definition)?;
            }
            generator.visuals = visuals;
        }
        out.extend(generator.component(op)?);
    }
    Ok(out)
}
