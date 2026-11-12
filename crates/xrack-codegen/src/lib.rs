//! Rust token generation from the typed UI operations exported from MLIR.
mod image;
mod scene_plan;
mod styled_text;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
};
use xrack_ir::semantic::{day_cell_inputs, is_window, rust_name, split_handle_inputs};
use xrack_mlir::{
    AliasGraph, AnimationRef, Module, OperationRef, PropertyEndpoint, RecordDeclRef, RepeatRef,
    RowSceneCache, StateCaseRef, TranslationRef, ValueRef, VisualCacheGroup, canonical_input,
    repeat_is_positioned, scene_children, visual_cache_operand,
};
use xrack_resources::visual::{
    ControlDefault, ControlLiteral, action_ref, check_state_binding, check_state_control,
    combo_editor, control_default, control_link, controlled_properties, controlled_property,
    default_combo_edit_text, indexed_visual_bindings, non_exclusive_button_group,
    non_exclusive_group, operand_type, picker_value, text_item,
};
pub use xrack_resources::{EmbeddedImage, EmbeddedPath, EmbeddedTranslations, check_nine_slices};
use xrack_runtime::{is_multiline_text_input, is_toggle};
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

fn field_access<'a>(path: impl IntoIterator<Item = &'a str>) -> TokenStream {
    let fields = path.into_iter().map(ident);
    quote!(#(.#fields)*)
}

fn ty(t: &Type) -> TokenStream {
    match t {
        Type::Int => quote!(::core::primitive::i64),
        Type::Float => quote!(::core::primitive::f64),
        Type::Bool => quote!(::core::primitive::bool),
        Type::String => quote!(::std::string::String),
        Type::StyledText => quote!(::xrack_runtime::StyledText),
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
        Type::ImageTileMode => quote!(::xrack_runtime::ImageTileMode),
        Type::ImageHorizontalAlignment => quote!(::xrack_runtime::ImageHorizontalAlignment),
        Type::ImageVerticalAlignment => quote!(::xrack_runtime::ImageVerticalAlignment),
        Type::ImageRendering => quote!(::xrack_runtime::ImageRendering),
        Type::Orientation => quote!(::xrack_runtime::Orientation),
        Type::FlickDirection => quote!(::xrack_runtime::FlickDirection),
        Type::FlickBoundsBehavior => quote!(::xrack_runtime::FlickBoundsBehavior),
        Type::FlickBoundsMovement => quote!(::xrack_runtime::FlickBoundsMovement),
        Type::FlexboxLayoutDirection => quote!(::xrack_runtime::FlexboxLayoutDirection),
        Type::FlexboxLayoutWrap => quote!(::xrack_runtime::FlexboxLayoutWrap),
        Type::DialInputMode => quote!(::xrack_runtime::DialInputMode),
        Type::ScrollBarPolicy => quote!(::xrack_runtime::ScrollBarPolicy),
        Type::ScrollSnapMode => quote!(::xrack_runtime::ScrollSnapMode),
        Type::ScrollAlignment => quote!(::xrack_runtime::ScrollAlignment),
        Type::TextWrap => quote!(::xrack_runtime::TextWrap),
        Type::TextHorizontalAlignment => quote!(::xrack_runtime::TextHorizontalAlignment),
        Type::TextVerticalAlignment => quote!(::xrack_runtime::TextVerticalAlignment),
        Type::TextOverflow => quote!(::xrack_runtime::TextOverflow),
        Type::InputType => quote!(::xrack_runtime::InputType),
        Type::CursorShape => quote!(::xrack_runtime::CursorShape),
        Type::ColorScheme => quote!(::xrack_runtime::ColorScheme),
        Type::FileDialogMode => quote!(::xrack_runtime::FileDialogMode),
        Type::MessageLevel => quote!(::xrack_runtime::MessageLevel),
        Type::MessageButtons => quote!(::xrack_runtime::MessageButtons),
        Type::CheckState => quote!(::xrack_runtime::CheckState),
        Type::DialogRole => quote!(::xrack_runtime::DialogRole),
        Type::StandardButtonKind => quote!(::xrack_runtime::StandardButtonKind),
        Type::SwipeSide => quote!(::xrack_runtime::SwipeSide),
        Type::DragAction => quote!(::xrack_runtime::DragAction),
        Type::KeyEvent => quote!(::xrack_runtime::KeyEvent),
        Type::WheelEvent => quote!(::xrack_runtime::WheelEvent),
        Type::PointerEvent => quote!(::xrack_runtime::PointerEvent),
        Type::PointerEventKind => quote!(::xrack_runtime::PointerEventKind),
        Type::PointerButton => quote!(::xrack_runtime::PointerButton),
        Type::PointerButtons => quote!(::xrack_runtime::PointerButtons),
        Type::TouchPoint => quote!(::xrack_runtime::TouchPoint),
        Type::TouchPoints => quote!(::xrack_runtime::TouchPoints),
        Type::KeyModifiers => quote!(::xrack_runtime::KeyModifiers),
        Type::EventResult => quote!(::xrack_runtime::EventResult),
        Type::FocusReason => quote!(::xrack_runtime::FocusReason),
        Type::DrawerEdge => quote!(::xrack_runtime::DrawerEdge),
        Type::PopupPlacement => quote!(::xrack_runtime::PopupPlacement),
        Type::SelectionTarget => quote!(::xrack_runtime::SelectionTarget),
        Type::Date => quote!(::xrack_runtime::Date),
        Type::Time => quote!(::xrack_runtime::Time),
        Type::Font => quote!(::xrack_runtime::Font),
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
    if *t == Type::TouchPoints {
        quote!(::xrack_runtime::TouchPoints::from(#value))
    } else if t.list_item().is_some() {
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
            let number = xrack_mlir::parse_float_literal(value)
                .ok_or_else(|| format!("invalid {} literal `{value}`", ty.name()))?;
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
        | Type::ImageTileMode
        | Type::ImageHorizontalAlignment
        | Type::ImageVerticalAlignment
        | Type::ImageRendering
        | Type::LayoutAlignment
        | Type::CrossAxisAlignment
        | Type::Orientation
        | Type::FlickDirection
        | Type::FlickBoundsBehavior
        | Type::FlickBoundsMovement
        | Type::FlexboxLayoutDirection
        | Type::FlexboxLayoutWrap
        | Type::DialInputMode
        | Type::ScrollBarPolicy
        | Type::ScrollSnapMode
        | Type::ScrollAlignment
        | Type::TextWrap
        | Type::TextHorizontalAlignment
        | Type::TextVerticalAlignment
        | Type::TextOverflow
        | Type::InputType
        | Type::CursorShape
        | Type::PointerEventKind
        | Type::PointerButton
        | Type::ColorScheme
        | Type::FileDialogMode
        | Type::MessageLevel
        | Type::MessageButtons
        | Type::CheckState
        | Type::DialogRole
        | Type::StandardButtonKind
        | Type::SwipeSide
        | Type::DragAction
        | Type::EventResult
        | Type::FocusReason
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
        Type::KeyEvent
        | Type::KeyModifiers
        | Type::WheelEvent
        | Type::PointerEvent
        | Type::TouchPoint
        | Type::TouchPoints => {
            let ty = self::ty(&ty);
            quote!(<#ty as ::core::default::Default>::default())
        }
        Type::Brush
        | Type::GradientStop
        | Type::Image
        | Type::Date
        | Type::Time
        | Type::Font
        | Type::StringList
        | Type::PointerButtons
        | Type::StyledText
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

fn playback_slot(key: &str) -> Ident {
    format_ident!("__image_playback_{}", rust_key(key))
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

fn table_selection_method(key: &str) -> Ident {
    format_ident!("__table_select_{}", rust_key(key))
}

fn table_selection_snapshot_method(key: &str) -> Ident {
    format_ident!("__table_selection_snapshot_{}", rust_key(key))
}

fn table_sort_method(key: &str) -> Ident {
    format_ident!("__table_sort_by_{}", rust_key(key))
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

fn table_order_field(key: &str) -> Ident {
    format_ident!("__table_order_{}", rust_key(key))
}

fn table_move_method(key: &str) -> Ident {
    format_ident!("__table_move_column_{}", rust_key(key))
}

fn table_rows_model_method(key: &str) -> Ident {
    format_ident!("__table_rows_model_{}", rust_key(key))
}

fn table_row_order_field(key: &str) -> Ident {
    format_ident!("__table_row_order_{}", rust_key(key))
}

fn table_move_row_method(key: &str) -> Ident {
    format_ident!("__table_move_row_{}", rust_key(key))
}

fn table_row_heights_field(key: &str) -> Ident {
    format_ident!("__table_row_heights_{}", rust_key(key))
}

fn table_row_height_method(key: &str) -> Ident {
    format_ident!("__table_set_row_height_{}", rust_key(key))
}

fn row_height_projection_field(key: &str) -> Ident {
    format_ident!("__row_height_projection_{}", rust_key(key))
}

fn split_state_field(key: &str) -> Ident {
    format_ident!("__split_state_{}", rust_key(key))
}

fn split_resize_handle_field(key: &str) -> Ident {
    format_ident!("__split_resize_handle_{}", rust_key(key))
}

fn split_save_method(key: &str) -> Ident {
    format_ident!("__split_save_{}", rust_key(key))
}

fn split_restore_method(key: &str) -> Ident {
    format_ident!("__split_restore_{}", rust_key(key))
}

fn split_update_method(key: &str) -> Ident {
    format_ident!("__split_update_{}", rust_key(key))
}

fn tree_state_field(key: &str) -> Ident {
    format_ident!("__tree_state_{}", rust_key(key))
}

fn tree_snapshot_method(key: &str) -> Ident {
    format_ident!("__tree_snapshot_{}", rust_key(key))
}

fn tree_command_method(key: &str) -> Ident {
    format_ident!("__tree_command_{}", rust_key(key))
}

fn tree_selection_method(key: &str) -> Ident {
    format_ident!("__tree_selection_{}", rust_key(key))
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

fn model_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("model"))
}

fn row_height_binding(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("row-height-field"))
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

fn table_option_configured(element: OperationRef<'_>, name: &str) -> bool {
    element.children().any(|binding| {
        matches!(binding.name(), "xr.bind" | "xr.state_set") && binding.attr("name") == Ok(name)
    })
}

fn table_history_configured(element: OperationRef<'_>) -> bool {
    matches!(element.attr("kind"), Ok("TableView" | "StandardTableView"))
        && (table_option_configured(element, "undoable")
            || table_option_configured(element, "pasteable"))
}

fn table_history_field(key: &str) -> Ident {
    format_ident!("__xr_table_history_{}", rust_key(key))
}

fn table_history_method(key: &str, action: &str) -> Ident {
    format_ident!("__xr_table_history_{}_{}", action, rust_key(key))
}

fn viewport_bindings(element: OperationRef<'_>) -> impl Iterator<Item = &'static str> {
    ["content-x", "content-y"]
        .into_iter()
        .filter(move |property| {
            control_link(element, property).is_some()
                || children(element, "xr.bind").any(|binding| binding.attr("name") == Ok(*property))
        })
}

fn focus_handler(name: &str) -> bool {
    matches!(
        name,
        "focus-gained" | "focus-lost" | "focus-changed-event" | "focus-within-changed"
    )
}

fn key_phase(name: &str) -> Option<Ident> {
    let phase = xrack_runtime::KeyPhase::ALL
        .into_iter()
        .find(|phase| phase.handler() == name)?;
    Some(format_ident!("{}", format!("{phase:?}")))
}

fn instance_field(key: &str) -> Ident {
    format_ident!("__instance_{}", rust_key(key))
}

fn radio_group_ref(element: OperationRef<'_>) -> Option<&str> {
    children(element, "xr.radio_group_ref")
        .next()?
        .attribute("group")
}

fn action_group_method(group: &str, operation: &str) -> Ident {
    format_ident!("__xr_action_group_{}_{}", rust_key(group), operation)
}

fn check_group_method(group: &str, operation: &str) -> Ident {
    format_ident!("__xr_check_group_{}_{}", rust_key(group), operation)
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

fn property_method(action: &str, owner: &str, name: &str) -> Ident {
    format_ident!(
        "__xr_{}_{}_{}_{}",
        action,
        owner.len(),
        rust_key(owner),
        rust_name(name)
    )
}

fn callback_method(action: &str, owner: &str, name: &str) -> Ident {
    let owner: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_{}_{}_{}", action, owner, rust_name(name))
}

fn callback_input_field(name: &str) -> Ident {
    format_ident!("__cb_{}", rust_name(name))
}

fn state_selector_name(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_state_{encoded}")
}

fn focus_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_focus_{encoded}")
}

fn scale_rotate_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_scale_rotate_{encoded}")
}

fn swipe_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_swipe_{encoded}")
}

fn hover_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_hover_{encoded}")
}

fn pointer_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_pointer_{encoded}")
}

fn viewport_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_viewport_{encoded}")
}

fn scroll_control_getter(owner: &str, property: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_scroll_{encoded}_{property}")
}

fn geometry_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_geometry_{encoded}")
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

fn has_virtual_rows(element: OperationRef<'_>) -> bool {
    matches!(
        element.attribute("kind"),
        Some(
            "ListView"
                | "GridView"
                | "StandardListView"
                | "TableView"
                | "StandardTableView"
                | "TreeView"
        )
    )
}

fn has_list_visibility(element: OperationRef<'_>) -> bool {
    matches!(
        element.attribute("kind"),
        Some("ListView" | "GridView" | "StandardListView")
    ) && children(element, "xr.bind").any(|binding| binding.attr("name") == Ok("row-visible-field"))
}

fn visible_list_model(key: &str) -> Ident {
    format_ident!("__visible_list_model_{}", rust_key(key))
}

fn view_index_method(key: &str) -> Ident {
    format_ident!("__view_index_{}", rust_key(key))
}

fn source_selection_method(key: &str) -> Ident {
    format_ident!("__select_source_{}", rust_key(key))
}

fn list_renderer_field(key: &str) -> Ident {
    format_ident!("__list_renderer_{}", rust_key(key))
}

fn list_rows_method(key: &str) -> Ident {
    format_ident!("__list_rows_{}", rust_key(key))
}

fn day_cell_template(element: OperationRef<'_>) -> Option<OperationRef<'_>> {
    children(element, "xr.element").find(|child| child.attr("kind") == Ok("DayCell"))
}

fn calendar_cells_field(key: &str) -> Ident {
    format_ident!("__calendar_cells_{}", rust_key(key))
}

fn static_children(op: OperationRef<'_>) -> impl Iterator<Item = (usize, OperationRef<'_>)> {
    // The first two RangeSlider children are the generated handles.
    let first = if op.attr("kind") == Ok("RangeSlider") {
        xrack_runtime::Node::RANGE_THUMBS
    } else {
        0
    };
    scene_children(op)
        .enumerate()
        .map(move |(index, child)| (first + index, child))
        .filter(|(_, child)| children(*child, "xr.repeat").next().is_none())
}

type GroupMembers<'a> = BTreeMap<&'a str, Vec<(Vec<usize>, OperationRef<'a>)>>;

fn button_group_members(root: OperationRef<'_>) -> GroupMembers<'_> {
    fn collect<'a>(
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        groups: &mut GroupMembers<'a>,
    ) {
        if let Some(group) = radio_group_ref(element).or_else(|| {
            children(element, "xr.check_group_ref")
                .next()?
                .attribute("group")
        }) {
            groups
                .entry(group)
                .or_default()
                .push((path.clone(), element));
        }
        for (index, child) in static_children(element) {
            path.push(index);
            collect(child, path, groups);
            path.pop();
        }
    }
    let mut groups = BTreeMap::new();
    collect(root, &mut Vec::new(), &mut groups);
    groups
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

fn row_delegate_paths<'a>(
    node: OperationRef<'a>,
    path: &mut Vec<usize>,
    delegates: &mut Vec<(Vec<usize>, OperationRef<'a>)>,
) {
    if node.name() == "xr.instance" {
        delegates.push((path.clone(), node));
        return;
    }
    for (index, child) in static_children(node) {
        path.push(index);
        row_delegate_paths(child, path, delegates);
        path.pop();
    }
}

fn cell_clipboard_parser(
    cell: OperationRef<'_>,
) -> Option<(OperationRef<'_>, Vec<OperationRef<'_>>)> {
    if let Some(parser) = children(cell, "xr.clipboard_paste").next() {
        return Some((parser, vec![cell]));
    }
    scene_children(cell).find_map(|child| {
        let (parser, mut path) = cell_clipboard_parser(child)?;
        path.push(cell);
        Some((parser, path))
    })
}

type VisualCacheKey<'a> = xrack_mlir::VisualCacheKey<ValueRef<'a>>;

#[derive(Default)]
struct VisualScope<'a> {
    row_index: Option<usize>,
    bindings: HashMap<VisualCacheKey<'a>, usize>,
    binding_types: Vec<Type>,
    repeats: BTreeMap<&'a str, RepeatScope<'a>>,
    row_delegates: BTreeMap<&'a str, usize>,
}

struct RepeatScope<'a> {
    model: Type,
    row: VisualScope<'a>,
    delegates: Vec<(Vec<usize>, OperationRef<'a>)>,
    table: bool,
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

struct TableProjection<'context, 'module> {
    expression: ExpressionContext<'context>,
    selectors: RefCell<BTreeMap<&'module str, OperationRef<'module>>>,
}

impl<'context> TableProjection<'context, '_> {
    fn new(expression: ExpressionContext<'context>) -> Self {
        Self {
            expression,
            selectors: RefCell::default(),
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
            if !repeat.delegates.is_empty() {
                let types = repeat.delegates.iter().map(|(_, delegate)| {
                    let component = ident(delegate.attr("component").expect("verified component"));
                    if repeat.table {
                        quote!(::std::cell::LazyCell<#component, ::std::boxed::Box<dyn FnOnce() -> #component>>)
                    } else {
                        quote!(#component)
                    }
                });
                fields.extend(quote!(#field: ::xrack_runtime::ComponentRepeater<#model_type, #row_type, (#(#types,)*)>,));
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
    alias_graphs: &'a BTreeMap<&'a str, AliasGraph<'a>>,
    alias_graph: &'a AliasGraph<'a>,
    forwarded_inputs: BTreeMap<PropertyEndpoint<'a>, Vec<Ident>>,
    elements: BTreeMap<&'a str, OperationRef<'a>>,
    callback_graphs: &'a BTreeMap<&'a str, AliasGraph<'a>>,
    callback_graph: &'a AliasGraph<'a>,
    callback_forwards: BTreeMap<PropertyEndpoint<'a>, Vec<(&'a str, OperationRef<'a>)>>,
    instance_handlers: Vec<(OperationRef<'a>, Option<OperationRef<'a>>)>,
    records: &'a BTreeMap<&'a str, Vec<Parameter>>,
    instances: BTreeMap<&'a str, OperationRef<'a>>,
    persistent_state_selectors: BTreeMap<&'a str, OperationRef<'a>>,
    visual_state_selectors: BTreeSet<&'a str>,
    element_properties: Vec<(&'a str, OperationRef<'a>)>,
    unlinked_controls: Vec<(&'a str, &'static str, Type)>,
    record_combos: Vec<&'a str>,
    picker_popups: Vec<(&'a str, Type)>,
    tables: Vec<OperationRef<'a>>,
    row_height_views: BTreeSet<&'a str>,
    splits: Vec<&'a str>,
    trees: Vec<&'a str>,
    stacks: BTreeMap<&'a str, OperationRef<'a>>,
    actions: BTreeMap<&'a str, OperationRef<'a>>,
    timers: Vec<OperationRef<'a>>,
    animated_images: Vec<OperationRef<'a>>,
    native_dialogs: Vec<OperationRef<'a>>,
    focus_targets: BTreeSet<&'a str>,
    pointer_targets: BTreeSet<&'a str>,
    viewport_targets: BTreeSet<&'a str>,
    source_scroll_targets: BTreeSet<&'a str>,
    geometry_targets: BTreeSet<&'a str>,
    bound_viewports: Vec<OperationRef<'a>>,
    scroll_observers: BTreeSet<&'a str>,
    secondary_windows: Vec<OperationRef<'a>>,
    popups: Vec<OperationRef<'a>>,
    visuals: VisualScope<'a>,
    repeat_invariants: Vec<OperationRef<'a>>,
    repeat_subexpressions: Vec<(VisualCacheGroup, OperationRef<'a>)>,
    use_visual_plan: bool,
    images: &'a BTreeMap<&'a str, Ident>,
    markdown: &'a BTreeMap<&'a str, Ident>,
    paths: &'a BTreeMap<&'a str, Ident>,
    with_fonts: bool,
    with_translations: bool,
}

#[derive(Default)]
struct TableClipboardArms {
    copies: TokenStream,
    pastes: TokenStream,
    replays: TokenStream,
    fields: TokenStream,
    defaults: TokenStream,
    methods: TokenStream,
}

struct StandardTableEditor {
    column: usize,
    field: String,
    ty: Type,
    enabled: TokenStream,
    kind: Ident,
}

impl<'a> Generator<'a> {
    fn declared_callbacks(&self, component: OperationRef<'a>) -> Vec<(&'a str, OperationRef<'a>)> {
        children(component, "xr.callback")
            .map(|callback| ("root", callback))
            .chain(self.elements.iter().flat_map(|(&key, &element)| {
                children(element, "xr.callback").map(move |callback| (key, callback))
            }))
            .collect()
    }

    fn index(
        &mut self,
        op: OperationRef<'a>,
        enclosing_repeat: Option<OperationRef<'a>>,
    ) -> Result<(), String> {
        if self.empty_repeat(op) {
            return Ok(());
        }
        let repeat = children(op, "xr.repeat").next().or(enclosing_repeat);
        match op.name() {
            "xr.instance" => {
                if repeat.is_none() {
                    self.instances.insert(element_key(op)?, op);
                }
                if children(op, "xr.callback").next().is_some() {
                    self.instance_handlers.push((op, repeat));
                }
            }
            "xr.element" => {
                let key = element_key(op)?;
                self.elements.insert(key, op);
                if repeat.is_none() && op.attr("kind") == Ok("TouchArea") {
                    self.pointer_targets.insert(key);
                }
                if matches!(op.attr("kind"), Ok("TableView" | "StandardTableView"))
                    && table_option_configured(op, "edit-on-tab")
                {
                    self.viewport_targets.insert(key);
                }
                if op.attr("kind") == Ok("Flickable")
                    && children(op, "xr.bind")
                        .any(|bind| bind.attr("name") == Ok("bounds-behavior"))
                {
                    self.viewport_targets.insert(key);
                }
                if row_height_binding(op).is_some() {
                    self.row_height_views.insert(key);
                }
                if viewport_bindings(op).next().is_some() {
                    self.viewport_targets.insert(key);
                    self.bound_viewports.push(op);
                    for property in viewport_bindings(op) {
                        if control_link(op, property).is_none() {
                            self.unlinked_controls.push((key, property, Type::Length));
                        }
                    }
                }
                if children(op, "xr.handler")
                    .any(|handler| matches!(handler.attr("name"), Ok("scrolled" | "flicked")))
                {
                    self.viewport_targets.insert(key);
                    self.scroll_observers.insert(key);
                }
                if children(op, "xr.handler")
                    .any(|handler| handler.attribute("name").is_some_and(focus_handler))
                {
                    self.focus_targets.insert(key);
                }
                if record_combo_binding(op).is_some() {
                    self.record_combos.push(key);
                }
                if let Some((_, ty)) = op.attribute("kind").and_then(picker_value) {
                    self.picker_popups.push((key, ty));
                }
                if matches!(op.attr("kind"), Ok("TableView" | "StandardTableView")) {
                    self.tables.push(op);
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
                if op.attr("kind") == Ok("AnimatedImage") {
                    self.animated_images.push(op);
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
                    if op.attr("kind") != Ok("Action") && control_link(op, property).is_none() {
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
                "xr.element" | "xr.instance" => self.index(child, repeat)?,
                "xr.instance_state" if repeat.is_none() => {
                    let selector = self.operand(child, 0)?;
                    self.persistent_state_selectors
                        .insert(selector.attr("owner")?, selector);
                }
                "xr.state_set" | "xr.state_timing" => {
                    let selector = self.operand(child, 0)?;
                    let owner = selector.attr("owner")?;
                    self.visual_state_selectors.insert(owner);
                    if repeat.is_none()
                        && matches!(
                            op.attribute("kind"),
                            Some("TableView" | "StandardTableView")
                        )
                        && child.name() == "xr.state_set"
                        && matches!(
                            child.attribute("name"),
                            Some("enabled" | "visible" | "pasteable" | "undoable")
                        )
                    {
                        self.persistent_state_selectors.insert(owner, selector);
                    }
                }
                "xr.focus_target" => {
                    self.focus_targets.insert(child.attr("target")?);
                }
                "xr.focus_read" => {
                    self.focus_targets.insert(child.attr("owner")?);
                }
                "xr.viewport_read" | "xr.flick_motion_read" => {
                    self.viewport_targets.insert(child.attr("owner")?);
                }
                "xr.scroll_target" => {
                    self.viewport_targets.insert(child.attr("target")?);
                }
                "xr.geometry_read" => {
                    self.geometry_targets.insert(child.attr("owner")?);
                }
                "xr.callback" | "xr.handler" | "xr.clipboard_paste" => {
                    for effect in child.children() {
                        match effect.name() {
                            "xr.focus_command" => {
                                self.focus_targets.insert(effect.attr("target")?);
                            }
                            "xr.focus_read" | "xr.text_command" => {
                                self.focus_targets.insert(effect.attr("owner")?);
                            }
                            "xr.viewport_read"
                            | "xr.scroll_command"
                            | "xr.flick_motion_read"
                            | "xr.cancel_flick"
                            | "xr.return_to_bounds" => {
                                let owner = effect.attr("owner")?;
                                self.viewport_targets.insert(owner);
                                if effect.name() == "xr.scroll_command"
                                    && matches!(
                                        effect.attr("method"),
                                        Ok("position-view-at-source-index"
                                            | "position-view-at-source-cell")
                                    )
                                {
                                    self.source_scroll_targets.insert(owner);
                                }
                            }
                            "xr.geometry_read" => {
                                self.geometry_targets.insert(effect.attr("owner")?);
                            }
                            _ => {}
                        }
                    }
                }
                "xr.bind" | "xr.layout_bind" | "xr.show_if"
                    if self.use_visual_plan
                        && matches!(
                            VisualCacheGroup::for_binding(child),
                            Some(VisualCacheGroup::RepeatInvariant(_))
                        ) =>
                {
                    self.repeat_invariants.push(child);
                }
                _ if self.use_visual_plan => {
                    if let Some(group) = VisualCacheGroup::for_subexpression(child) {
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
        if self.static_subtree(element) || self.empty_repeat(element) {
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
            let mut delegates = Vec::new();
            row_delegate_paths(element, &mut Vec::new(), &mut delegates);
            for (slot, (_, delegate)) in delegates.iter().enumerate() {
                row.row_delegates.insert(element_key(*delegate)?, slot);
            }
            scope.repeats.insert(
                key,
                RepeatScope {
                    model,
                    row,
                    delegates,
                    table: element.attr("kind") == Ok("TableRow"),
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
        for binding in indexed_visual_bindings(element) {
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
        if let Some(definition) = visual_cache_operand(binding, self.use_visual_plan) {
            let key = self.visual_cache_key(binding);
            if !key.uses_component_scope() || scope.row_index.is_none() {
                scope.index_value(key, definition)?;
            }
        }
        Ok(())
    }

    fn visual_cache_key(&self, binding: OperationRef<'a>) -> VisualCacheKey<'a> {
        VisualCacheKey::for_binding(binding, self.use_visual_plan)
            .expect("verified binding operand")
    }

    fn visual_value(
        &self,
        binding: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let value = self.visual_expr(self.operand(binding, 0)?, scope)?;
        let key = self.visual_cache_key(binding);
        let cache = if key.uses_component_scope() {
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
            && let Some(group) = VisualCacheGroup::for_subexpression(op)
            && let Some(&id) = self.visuals.bindings.get(&VisualCacheKey::Planned(group))
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
            "xr.translate" => {
                if !self.with_translations {
                    return Err("@tr requires a translation catalog".into());
                }
                let translation = TranslationRef::from_operation(op)?;
                let id = translation.id;
                let (names, values): (Vec<_>, Vec<_>) = translation
                    .arguments
                    .into_iter()
                    .map(|(name, value)| {
                        let value = value
                            .defining_operation()
                            .ok_or("translation argument has no definition")?;
                        Ok((name, self.expr_in(value, context)?))
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into_iter()
                    .unzip();
                quote!(__XR_TRANSLATIONS.with(|catalog| catalog.translate(
                    &#receiver.__state.__locale, #id,
                    &[#((#names, ::xrack_runtime::TranslationValue::from(#values))),*],
                )))
            }
            "xr.array" | "xr.record_array" => {
                let items = (0..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                quote!(::xrack_runtime::VecModel::from(::std::vec![#(#items),*]))
            }
            "xr.pointer_buttons" => {
                let items = (0..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                quote!(::xrack_runtime::PointerButtons::from([#(#items),*]))
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
            "xr.make_font" => {
                let (family, size, weight, italic) = (arg(0)?, arg(1)?, arg(2)?, arg(3)?);
                quote!(::xrack_runtime::Font { family: #family, size: #size, weight: #weight, italic: #italic })
            }
            "xr.image_dimension" => {
                let image = arg(0)?;
                let axis = match value {
                    "width" => 0usize,
                    "height" => 1usize,
                    _ => return Err("invalid image dimension".into()),
                };
                quote!(::core::primitive::i64::from((#image).size()[#axis]))
            }
            "xr.get_field"
            | "xr.date_field"
            | "xr.time_field"
            | "xr.font_field"
            | "xr.key_field"
            | "xr.wheel_field"
            | "xr.pointer_field"
            | "xr.touch_point_field" => {
                let record = arg(0)?;
                let field = ident(value);
                quote!((#record).#field)
            }
            "xr.markdown" => {
                let name = self
                    .markdown
                    .get(value)
                    .ok_or_else(|| "missing compiled Markdown literal".to_string())?;
                quote!(#name.clone())
            }
            "xr.image_url" if value.is_empty() => quote!(::xrack_runtime::Image::Empty),
            "xr.image_url" => {
                let image = self
                    .images
                    .get(value)
                    .ok_or_else(|| format!("unresolved image {value:?}"))?;
                quote!(#image)
            }
            "xr.nine_slice" => {
                let source = arg(0)?;
                let xrack_runtime::NineSlice {
                    top,
                    right,
                    bottom,
                    left,
                } = xrack_resources::nine_slice_edges(op)?;
                quote! {
                    #source.with_nine_slice(::xrack_runtime::NineSlice {
                        top: #top, right: #right, bottom: #bottom, left: #left,
                    }).expect("verified image slice")
                }
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
            "xr.control_read" => {
                let element = *self
                    .elements
                    .get(owner(op))
                    .ok_or("unknown alias control")?;
                self.control_value(element, value, value_type(op)?)?
            }
            "xr.action_group_read" => {
                let getter = action_group_method(owner(op), "current");
                quote!(#receiver.#getter())
            }
            "xr.native_dialog_read" => match value {
                "pending" => {
                    let slot = native_dialog_member(owner(op), "slot");
                    quote!(#receiver.__state.#slot.pending())
                }
                "selected-file" | "selected-files" => {
                    let selection = native_dialog_member(owner(op), "selection");
                    let getter = ident(value);
                    quote!(#receiver.__state.#selection.#getter())
                }
                _ => return Err("unknown native dialog result property".into()),
            },
            "xr.scroll_control_read" => {
                let getter = scroll_control_getter(owner(op), value);
                quote!(#receiver.#getter())
            }
            "xr.scale_rotate_read" => {
                let handle = scale_rotate_handle(owner(op));
                let metric = match value {
                    "active" => quote!(active),
                    "scale" => quote!(scale),
                    "rotation" => quote!(rotation),
                    "center-x" => quote!(center_x),
                    "center-y" => quote!(center_y),
                    _ => return Err("unknown scale/rotate metric".into()),
                };
                quote!(#receiver.__state.#handle.#metric())
            }
            "xr.swipe_read" => {
                let handle = swipe_handle(owner(op));
                let metric = match value {
                    "swiping" => quote!(swiping),
                    "pressed-x" => quote!(pressed_x),
                    "pressed-y" => quote!(pressed_y),
                    "current-x" => quote!(current_x),
                    "current-y" => quote!(current_y),
                    _ => return Err("unknown swipe metric".into()),
                };
                quote!(#receiver.__state.#handle.#metric())
            }
            "xr.hover_read" => {
                let handle = hover_handle(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.touch_area_read" => {
                let handle = pointer_handle(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.focus_read" => {
                let handle = focus_handle(owner(op));
                let getter = match value {
                    "has-focus" => quote!(has_focus),
                    "focus-within" => quote!(focus_within),
                    _ => return Err("unknown focus state property".into()),
                };
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.image_playback_read" => {
                let slot = playback_slot(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#slot.#getter())
            }
            "xr.window_read" => {
                let metric = match value {
                    "window-active" => quote!(Active),
                    "minimized" => quote!(Minimized),
                    "minimized-known" => quote!(MinimizedKnown),
                    "maximized" => quote!(Maximized),
                    "full-screen" => quote!(FullScreen),
                    _ => return Err("unknown native window status property".into()),
                };
                quote!(#receiver.__state.__window.get(::xrack_runtime::WindowMetric::#metric))
            }
            "xr.check_group_read" => {
                let read = check_group_method(owner(op), "state");
                quote!(#receiver.#read())
            }
            "xr.viewport_read" | "xr.flick_motion_read" => {
                let handle = viewport_handle(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.geometry_read" => {
                let handle = geometry_handle(owner(op));
                if value == "layout-ready" {
                    quote!(#receiver.__state.#handle.ready())
                } else {
                    let metric = xrack_runtime::GeometryMetric::from_name(value)
                        .ok_or("unknown geometry metric")?;
                    let variant = format_ident!("{}", format!("{metric:?}"));
                    quote!(#receiver.__state.#handle.read(::xrack_runtime::GeometryMetric::#variant))
                }
            }
            "xr.stack_read" => {
                let snapshot = stack_snapshot_method(owner(op));
                let snapshot = quote!(#receiver.#snapshot());
                match value {
                    "current-page" => quote!(#snapshot.current().unwrap_or("").to_owned()),
                    "depth" => quote!(#snapshot.depth() as ::core::primitive::i64),
                    "can-go-back" => quote!(#snapshot.depth() > 1),
                    "active" | "stack-index" => {
                        let route = self.operand(op, 0)?;
                        let route: String = serde_json::from_str(route.attr("value")?)
                            .map_err(|error| error.to_string())?;
                        if value == "active" {
                            quote!(#snapshot.current() == ::core::option::Option::Some(#route))
                        } else {
                            quote!(#snapshot.index_of(#route).map_or(-1i64, |index| index as ::core::primitive::i64))
                        }
                    }
                    _ => return Err("unknown StackView state property".into()),
                }
            }
            "xr.table_selection_read" => {
                let snapshot = table_selection_snapshot_method(owner(op));
                match value {
                    "current-column" => {
                        quote!(#receiver.#snapshot().current_column().map_or(-1i64, |column| column as ::core::primitive::i64))
                    }
                    "selection-count" => {
                        quote!(#receiver.#snapshot().len() as ::core::primitive::i64)
                    }
                    "has-selection" => quote!(!#receiver.#snapshot().is_empty()),
                    _ => return Err("unknown table selection property".into()),
                }
            }
            "xr.table_sort_read" => {
                let field = table_sort_field(owner(op));
                let sort = quote!(#receiver.__state.#field.get(|| None));
                match value {
                    "current-sort-column" => {
                        quote!(#sort.map_or(-1i64, |sort| sort.column as ::core::primitive::i64))
                    }
                    "current-sort-descending" => quote!(#sort.is_some_and(|sort| sort.descending)),
                    _ => return Err("unknown table sort property".into()),
                }
            }
            "xr.table_history_read" => {
                let key = owner(op);
                let table = *self.elements.get(key).ok_or("missing history table")?;
                if self.table_history_item(table)?.is_some() {
                    let method = table_history_method(key, "available");
                    let redo = value == "can-redo";
                    quote!(#receiver.#method(#redo))
                } else {
                    quote!(false)
                }
            }
            "xr.split_state_read" => {
                let save = split_save_method(owner(op));
                quote!(#receiver.#save())
            }
            "xr.split_resizing" => {
                let handle = split_resize_handle_field(owner(op));
                quote!(#receiver.__state.#handle.resizing())
            }
            "xr.tree_query" => {
                let snapshot = tree_snapshot_method(owner(op));
                let key = arg(0)?;
                match value {
                    "is-expanded" => quote!(#receiver.#snapshot().is_expanded(#key)),
                    "depth" => {
                        quote!(#receiver.#snapshot().depth(#key).map_or(-1i64, |index| index as ::core::primitive::i64))
                    }
                    "row-for-key" => {
                        quote!(#receiver.#snapshot().visible_index(#key).map_or(-1i64, |index| index as ::core::primitive::i64))
                    }
                    _ => return Err("unknown tree query".into()),
                }
            }
            "xr.view_index" => {
                let method = view_index_method(owner(op));
                let index = arg(0)?;
                let from_source = value == "view-index-for-source";
                quote! {
                    <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(#index)
                        .ok().and_then(|index| #receiver.#method(index, #from_source))
                        .map_or(-1i64, |index| index as ::core::primitive::i64)
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
                if self.static_gradient(op)
                    && let Some(group) = op.integer_attribute("xr.static_gradient_group")
                {
                    let cache = format_ident!("__XR_GRADIENT_{group}");
                    quote!(#cache.get_or_init(|| #brush).clone())
                } else {
                    brush
                }
            }
            "xr.unary" => {
                let arg = arg(0)?;
                match value {
                    "+" => arg,
                    "-" if value_type(op)? == Type::Int => {
                        quote!(::core::primitive::i64::wrapping_neg(#arg))
                    }
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
            "xr.binary" if value_type(op)? == Type::Int => {
                let (left, right) = (arg(0)?, arg(1)?);
                match value {
                    "+" => quote!(::core::primitive::i64::wrapping_add(#left, #right)),
                    "-" => quote!(::core::primitive::i64::wrapping_sub(#left, #right)),
                    "*" => quote!(::core::primitive::i64::wrapping_mul(#left, #right)),
                    "/" => quote!(::core::primitive::i64::checked_div(#left, #right)
                        .expect("invalid integer division")),
                    "%" => quote!(::core::primitive::i64::checked_rem(#left, #right)
                        .expect("invalid integer remainder")),
                    _ => return Err(format!("unsupported integer operator {value}")),
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
        self.set_property_in(&quote!(self), owner, name, value)
    }

    fn set_property_in(
        &self,
        receiver: &TokenStream,
        owner: &str,
        name: &str,
        value: &TokenStream,
    ) -> TokenStream {
        self.property_call("write", receiver, owner, name, value)
    }

    fn property_call(
        &self,
        action: &str,
        receiver: &TokenStream,
        owner: &str,
        name: &str,
        args: &TokenStream,
    ) -> TokenStream {
        if self.instances.contains_key(owner) {
            let instance = instance_field(owner);
            let method = property_method(action, "root", name);
            quote!(#receiver.__state.#instance.#method(#args);)
        } else {
            let method = property_method(action, owner, name);
            quote!(#receiver.#method(#args);)
        }
    }

    fn forwarded_input(&self, endpoint: PropertyEndpoint<'a>) -> Option<TokenStream> {
        let (first, rest) = self.forwarded_inputs.get(&endpoint)?.split_first()?;
        Some(quote!(__inputs.#first.take() #(.or(__inputs.#rest.take()))*))
    }

    fn callback_call(
        &self,
        op: OperationRef<'a>,
        name: &str,
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        let receiver = context.receiver;
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
        Ok(self.callable_method("invoke_cb", receiver, owner, name, &quote!(#(#args),*)))
    }

    fn callable_method(
        &self,
        action: &str,
        receiver: &TokenStream,
        owner: &str,
        name: &str,
        args: &TokenStream,
    ) -> TokenStream {
        if self.instances.contains_key(owner) {
            let field = instance_field(owner);
            let method = callback_method(action, "root", name);
            quote!(#receiver.__state.#field.#method(#args))
        } else {
            let method = callback_method(action, owner, name);
            quote!(#receiver.#method(#args))
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
                    statements.extend(quote!(self.__state.__window.request_close();));
                }
                "xr.window_command" => {
                    let command = match statement.attr("command")? {
                        "minimize" => quote!(Minimize),
                        "maximize" => quote!(Maximize),
                        "restore" => quote!(Restore),
                        "toggle-maximized" => quote!(ToggleMaximized),
                        "toggle-full-screen" => quote!(ToggleFullScreen),
                        "set-full-screen" => {
                            let enabled = self.expr(self.operand(statement, 0)?)?;
                            quote!(SetFullScreen(#enabled))
                        }
                        _ => return Err("unknown native window command".into()),
                    };
                    statements.extend(quote! {
                        let __xr_command = ::xrack_runtime::WindowCommand::#command;
                        self.__state.__window.request(__xr_command);
                    });
                }
                "xr.check_group_command" => {
                    let set = check_group_method(statement.attr("owner")?, "set");
                    let state = self.expr(self.operand(statement, 0)?)?;
                    statements.extend(quote!(self.#set(#state);));
                }
                "xr.focus_command" => {
                    let handle = focus_handle(statement.attr("target")?);
                    let action = if statement.attr("action")? == "clear" {
                        quote!(Clear)
                    } else {
                        quote!(Focus)
                    };
                    statements.extend(quote!(self.__state.#handle.request(::xrack_runtime::FocusAction::#action);));
                }
                "xr.swipe_cancel" => {
                    let handle = swipe_handle(statement.attr("owner")?);
                    statements.extend(quote!(self.__state.#handle.cancel();));
                }
                "xr.cancel_flick" | "xr.return_to_bounds" => {
                    let handle = viewport_handle(statement.attr("owner")?);
                    let action = if statement.name() == "xr.cancel_flick" {
                        quote!(CancelFlick)
                    } else {
                        quote!(ReturnToBounds)
                    };
                    statements.extend(quote!(self.__state.#handle.request(::xrack_runtime::ScrollAction::#action);));
                }
                "xr.text_command" => {
                    let handle = focus_handle(statement.attr("owner")?);
                    let method = statement.attr("method")?;
                    if matches!(method, "edit-cell" | "cancel-edit") {
                        let action = if method == "edit-cell" {
                            let row = self.expr(self.operand(statement, 0)?)?;
                            let column = self.expr(self.operand(statement, 1)?)?;
                            quote!(EditTableCell { row: #row, column: #column })
                        } else {
                            quote!(CancelTableEdit)
                        };
                        statements.extend(quote!(self.__state.#handle.request(::xrack_runtime::FocusAction::#action);));
                        continue;
                    }
                    let command = match method {
                        "select-all" => quote!(SelectAll),
                        "clear-selection" => quote!(ClearSelection),
                        "undo" => quote!(Undo),
                        "redo" => quote!(Redo),
                        "cut" => quote!(Cut),
                        "copy" => quote!(Copy),
                        "paste" => quote!(Paste),
                        "set-selection-offsets" => {
                            let anchor = self.expr(self.operand(statement, 0)?)?;
                            let focus = self.expr(self.operand(statement, 1)?)?;
                            quote!(SetSelection { anchor: #anchor, focus: #focus })
                        }
                        _ => return Err("unknown text command".into()),
                    };
                    statements.extend(quote! {
                        self.__state.#handle.request(::xrack_runtime::FocusAction::Text(
                            ::xrack_runtime::TextCommand::#command,
                        ));
                    });
                }
                "xr.scroll_command" => {
                    let owner = statement.attr("owner")?;
                    let handle = viewport_handle(owner);
                    let first = self.expr(self.operand(statement, 0)?)?;
                    let second = self.expr(self.operand(statement, 1)?)?;
                    let method = statement.attr("method")?;
                    if matches!(
                        method,
                        "position-view-at-source-index" | "position-view-at-source-cell"
                    ) {
                        let source = self.view_source_model(
                            *self.elements.get(owner).ok_or("unknown source view")?,
                        )?;
                        let (arguments, action) = if method == "position-view-at-source-cell" {
                            let horizontal = self.expr(self.operand(statement, 2)?)?;
                            let vertical = self.expr(self.operand(statement, 3)?)?;
                            (
                                quote! {
                                    let __xr_column = #second;
                                    let __xr_horizontal = #horizontal;
                                    let __xr_vertical = #vertical;
                                },
                                quote!(::xrack_runtime::ScrollAction::ToCellAtRowKey {
                                    key: __xr_key,
                                    source_index: __xr_index,
                                    column: __xr_column,
                                    horizontal: __xr_horizontal,
                                    vertical: __xr_vertical,
                                }),
                            )
                        } else {
                            (
                                quote!(let __xr_alignment = #second;),
                                quote!(::xrack_runtime::ScrollAction::ToRowKey {
                                    key: __xr_key,
                                    source_index: __xr_index,
                                    alignment: __xr_alignment,
                                }),
                            )
                        };
                        statements.extend(quote! {{
                            let __xr_source = #first;
                            #arguments
                            if let Ok(__xr_index) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(__xr_source) {
                                let __xr_model = #source;
                                if __xr_index < ::xrack_runtime::ListModel::row_count(&*__xr_model) {
                                    let __xr_key = ::xrack_runtime::ListModel::row_key(&*__xr_model, __xr_index);
                                    self.__state.#handle.request(#action);
                                }
                            }
                        }});
                        continue;
                    }
                    let action = match method {
                        "scroll-to" => {
                            quote!(::xrack_runtime::ScrollAction::To { x: #first, y: #second })
                        }
                        "position-view-at-index" | "position-view-at-row" => {
                            quote!(::xrack_runtime::ScrollAction::ToIndex {
                                index: #first, alignment: #second
                            })
                        }
                        "position-view-at-column" => {
                            quote!(::xrack_runtime::ScrollAction::ToColumn {
                                column: #first, alignment: #second
                            })
                        }
                        "position-view-at-cell" => {
                            let horizontal = self.expr(self.operand(statement, 2)?)?;
                            let vertical = self.expr(self.operand(statement, 3)?)?;
                            quote!(::xrack_runtime::ScrollAction::ToCell {
                                row: #first, column: #second, horizontal: #horizontal, vertical: #vertical
                            })
                        }
                        _ => return Err("unknown scroll command".into()),
                    };
                    statements.extend(quote!(self.__state.#handle.request(#action);));
                }
                "xr.source_selection" => {
                    let method = source_selection_method(statement.attr("owner")?);
                    let source = self.expr(self.operand(statement, 0)?)?;
                    let column = if statement.attr("method")? == "select-source-cell" {
                        let column = self.expr(self.operand(statement, 1)?)?;
                        quote!(Some(#column))
                    } else {
                        quote!(None)
                    };
                    statements.extend(quote!(self.#method(#source, #column);));
                }
                "xr.move_column" | "xr.move_row" => {
                    let owner = statement.attr("owner")?;
                    let method = if statement.name() == "xr.move_row" {
                        table_move_row_method(owner)
                    } else {
                        table_move_method(owner)
                    };
                    let from = self.expr(self.operand(statement, 0)?)?;
                    let to = self.expr(self.operand(statement, 1)?)?;
                    statements.extend(quote!(self.#method(#from, #to);));
                }
                "xr.table_selection_command" => {
                    let method = table_selection_method(owner(statement));
                    let operation = statement.attr("method")?;
                    let names = (0..statement.operand_count())
                        .map(|index| format_ident!("__xr_arg_{index}"))
                        .collect::<Vec<_>>();
                    let values = (0..statement.operand_count())
                        .map(|index| self.expr(self.operand(statement, index)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    let action = match operation {
                        "select-all" => quote!(All),
                        "clear-selection" => quote!(Clear),
                        "select-row" => {
                            let row = &names[0];
                            quote!(Row { index: #row, operation: ::xrack_runtime::RowSelection::Replace })
                        }
                        "select-column" => {
                            let column = &names[0];
                            quote!(Column { column: #column, operation: ::xrack_runtime::RowSelection::Replace })
                        }
                        "select-cell" | "set-current-cell" | "toggle-cell" => {
                            let row = &names[0];
                            let column = &names[1];
                            let selection = match operation {
                                "set-current-cell" => quote!(Move),
                                "toggle-cell" => quote!(Toggle),
                                _ => quote!(Replace),
                            };
                            quote!(Cell { row: #row, column: #column, operation: ::xrack_runtime::RowSelection::#selection })
                        }
                        "select-range" => {
                            let (a, b, c, d) = (&names[0], &names[1], &names[2], &names[3]);
                            quote!(DragRange { anchor: (#a, #b), end: (#c, #d), additive: false })
                        }
                        _ => return Err("unknown table selection command".into()),
                    };
                    let apply = quote!(let _ = self.#method(::xrack_runtime::TableSelectionAction::#action););
                    if names.is_empty() {
                        statements.extend(apply);
                    } else {
                        statements.extend(quote! {
                            if let [#(Ok(#names)),*] = [#(#values),*].map(|value| <::core::primitive::usize as ::core::convert::TryFrom<::core::primitive::i64>>::try_from(value)) {
                                #apply
                            }
                        });
                    }
                }
                "xr.table_history_command" => {
                    let key = owner(statement);
                    let table = *self.elements.get(key).ok_or("missing history table")?;
                    if self.table_history_item(table)?.is_some() {
                        let method = table_history_method(key, "replay");
                        let redo = statement.attr("method")? == "redo";
                        statements.extend(quote!(let _ = self.#method(#redo);));
                    }
                }
                "xr.table_sort_command" => {
                    let method = table_sort_method(owner(statement));
                    match statement.attr("method")? {
                        "clear-sort" => statements.extend(quote!(let _ = self.#method(None);)),
                        "sort-by" => {
                            let column = self.expr(self.operand(statement, 0)?)?;
                            let descending = self.expr(self.operand(statement, 1)?)?;
                            statements.extend(quote! {
                                {
                                    let __xr_column = #column;
                                    let __xr_descending = #descending;
                                    if let Ok(column) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(__xr_column) {
                                        let _ = self.#method(Some(::xrack_runtime::TableSort { column, descending: __xr_descending }));
                                    }
                                }
                            });
                        }
                        _ => return Err("unknown table sort command".into()),
                    }
                }
                "xr.split_restore" => {
                    let restore = split_restore_method(owner(statement));
                    let state = self.expr(self.operand(statement, 0)?)?;
                    statements.extend(quote!(let _ = self.#restore(&(#state));));
                }
                "xr.tree_command" => {
                    let method = tree_command_method(owner(statement));
                    let key = self.expr(self.operand(statement, 0)?)?;
                    let action = match statement.attr("method")? {
                        "expand" => quote!(Expand(#key)),
                        "collapse" => quote!(Collapse(#key)),
                        "toggle-expanded" => quote!(ToggleExpanded(#key)),
                        "collapse-recursively" => quote!(CollapseRecursively(#key)),
                        "expand-to" => quote!(ExpandTo(#key)),
                        "expand-recursively" => {
                            let depth = self.expr(self.operand(statement, 1)?)?;
                            quote!(ExpandRecursively { key: #key, depth: #depth })
                        }
                        _ => return Err("unknown tree command".into()),
                    };
                    statements.extend(quote!(self.#method(::xrack_runtime::TreeCommand::#action);));
                }
                "xr.set_row_height" => {
                    let method = table_row_height_method(statement.attr("owner")?);
                    let row = self.expr(self.operand(statement, 0)?)?;
                    let height = self.expr(self.operand(statement, 1)?)?;
                    statements.extend(quote!(self.#method(#row, #height);));
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
        self.use_visual_plan
            && element.name() == "xr.element"
            && element.attribute("xr.static_subtree") == Some("true")
    }

    fn empty_repeat(&self, element: OperationRef<'a>) -> bool {
        self.use_visual_plan
            && children(element, "xr.repeat")
                .next()
                .is_some_and(|repeat| repeat.attribute("xr.empty_repeat") == Some("true"))
    }

    fn static_gradient(&self, expression: OperationRef<'a>) -> bool {
        self.use_visual_plan
            && matches!(
                expression.name(),
                "xr.linear_gradient" | "xr.radial_gradient"
            )
            && expression.attribute("xr.static_gradient") == Some("true")
    }

    fn render(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
    ) -> Result<Ident, String> {
        if self.empty_repeat(element) {
            let name = format_ident!("__xr_empty_repeat_{}", *next);
            *next += 1;
            out.extend(quote!(let #name = ::xrack_runtime::Node::hidden();));
            return Ok(name);
        }
        if self.static_subtree(element) {
            let mut body = TokenStream::new();
            let name =
                self.render_element(element, &VisualScope::default(), &mut body, next, None)?;
            if let Some(rows) = element.integer_attribute("xr.static_repeat_count") {
                let positioned = repeat_is_positioned(element);
                body.extend(quote! {
                    let #name = ::xrack_runtime::static_repeat_scene(#name, #rows, #positioned);
                });
            }
            if let Some(group) = element.integer_attribute("xr.static_subtree_group") {
                let cache = format_ident!("__XR_STATIC_SCENE_{group}");
                out.extend(quote! {
                    let #name = #cache.with(|cache| {
                        cache.get_or_init(|| { #body #name.into_shared() }).clone()
                    });
                });
                return Ok(name);
            }
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
            let render = if repeat_is_positioned(element) {
                quote!(render_positioned)
            } else {
                quote!(render)
            };
            out.extend(quote! {
                let #name = #receiver.#field.#render(|| #expression, #row);
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
        let parameters = RepeatRef::from_operation(model)?;
        let item = ident(parameters.item);
        let index_binding = parameters.index.map(|index| {
            let index = ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        });
        let row_local = row_local(repeat.index());
        let delegate = (!repeat.delegates.is_empty()).then(|| quote!(__xr_delegates,));
        let track_columns = (repeat.table && repeat.delegates.is_empty())
            .then(|| quote!(__xr_columns_changed.track();));
        let mut body = TokenStream::new();
        let child = self.render_element(element, &repeat.row, &mut body, next, None)?;
        if element.attr("kind") == Ok("TableRow") {
            let cells: BTreeSet<_> = repeat
                .delegates
                .iter()
                .filter_map(|(path, _)| path.first())
                .copied()
                .collect();
            for column in cells {
                body.extend(quote! {
                    if let Some(__xr_cell) = #child.child_at_mut(#column)
                        && __xr_cell.is_visible()
                    {
                        __xr_cell.properties.insert("table-delegate-cell", true.into());
                    }
                });
            }
        }
        Ok(
            quote!(|#delegate __xr_index, #item, #row_local| { #track_columns #index_binding #body #child }),
        )
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

    fn record_path(&self, op: OperationRef<'a>) -> Result<Vec<&'a str>, String> {
        let value = op.result(0).ok_or("record field access missing a result")?;
        xrack_mlir::record_field_path(value).map(|(_, path)| path)
    }

    fn tree_snapshot(
        &self,
        element: OperationRef<'a>,
        row: OperationRef<'a>,
        repeater: TokenStream,
        model: &TokenStream,
    ) -> Result<TokenStream, String> {
        let role = |name: &str| -> Result<TokenStream, String> {
            let binding = children(row, "xr.bind")
                .find(|bind| bind.attr("name") == Ok(name))
                .ok_or_else(|| format!("TreeRow is missing `{name}`"))?;
            Ok(field_access(self.record_path(self.operand(binding, 0)?)?))
        };
        let key = role("key")?;
        let parent = role("parent-key")?;
        let state = tree_state_field(element_key(element)?);
        let root = self.binding_value_or(element, "root-key", quote!(-1i64))?;
        let filter = self.record_field_binding(element, "filter-field")?;
        if let Some(visible) = self.record_field_binding(element, "row-visible-field")? {
            let (_, repeat) = self.list_template(element)?;
            let Some(Type::RecordList(record)) = operand_type(repeat) else {
                return Err("tree row visibility requires a record list".into());
            };
            let record = ident(&record);
            let visible = field_access(visible.split('.'));
            let field = filter
                .map(|field| {
                    let field = field_access(field.split('.'));
                    quote!(Some(|item: &#record| (item #field).as_str()))
                })
                .unwrap_or_else(|| quote!(None));
            let query = self.binding_value_or(
                element,
                "filter-text",
                quote!(::std::string::String::new()),
            )?;
            return Ok(quote! {
                self.__state.#state.snapshot_projected(
                    || #repeater.resolve_model(|| #model),
                    |item| (item #key, item #parent),
                    || #root,
                    Some(|item: &#record| item #visible),
                    #field,
                    || #query,
                )
            });
        }
        if let Some(field) = filter {
            let field = field_access(field.split('.'));
            let query = self.binding_value_or(
                element,
                "filter-text",
                quote!(::std::string::String::new()),
            )?;
            return Ok(quote! {
                self.__state.#state.snapshot_filtered(
                    || #repeater.resolve_model(|| #model),
                    |item| (item #key, item #parent),
                    || #root,
                    || #query,
                    |item| (item #field).as_str(),
                )
            });
        }
        Ok(
            quote!(self.__state.#state.snapshot_with_root(|| #repeater.resolve_model(|| #model), |item| (item #key, item #parent), || #root)),
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
        } else if has_virtual_rows(element) {
            let method = list_rows_method(element_key(element)?);
            out.extend(quote!([#(#path),*] => self.#method(__range, &::core::default::Default::default()),));
        } else if children(element, "xr.repeat").next().is_none() {
            for (index, child) in static_children(element) {
                path.push(index);
                self.list_arms(child, path, out)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn list_row_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        if element.attr("kind") == Ok("StandardTableView") {
            let model = self.table_model(element)?;
            let fields = self.standard_table_fields(element)?;
            let count = fields.len();
            let header = if let Some(binding) = children(element, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("row-header-field"))
            {
                let name: String = serde_json::from_str(self.operand(binding, 0)?.attr("value")?)
                    .map_err(|error| error.to_string())?;
                let field = field_access(name.split('.'));
                quote!(::core::option::Option::Some(item #field.to_string()))
            } else {
                quote!(::core::option::Option::None)
            };
            let editors = self.standard_table_editors(element)?;
            let mut column_editors = vec![quote!(None); count];
            for StandardTableEditor {
                column,
                enabled,
                kind,
                ..
            } in editors
            {
                column_editors[column] =
                    quote!((#enabled).then_some(::xrack_runtime::TableCellEditor::#kind));
            }
            let cells = fields
                .iter()
                .enumerate()
                .map(|(column, field)| quote!(#column => item #field.to_string(),));
            let editors = column_editors
                .iter()
                .enumerate()
                .map(|(column, editor)| quote!(#column => #editor,));
            return Ok(quote! {
                ::xrack_runtime::project_standard_table_rows(
                    #model, __range, __xr_columns, #count, |item| #header,
                    |item, column| match column { #(#cells)* _ => ::core::unreachable!() },
                    |column| match column { #(#editors)* _ => ::core::option::Option::None },
                )
            });
        } else if element.attr("kind") == Ok("StandardListView") {
            if let Some(binding) = model_binding(element) {
                let (model, rows) = if has_list_visibility(element) {
                    let visible = visible_list_model(element_key(element)?);
                    (quote!(self.#visible()), quote!(standard_list_rows_in_view))
                } else {
                    (
                        self.visual_value(binding, &self.visuals)?,
                        quote!(standard_list_rows),
                    )
                };
                let label = if matches!(value_type(self.operand(binding, 0)?)?, Type::RecordList(_))
                {
                    let field = self
                        .record_field_binding(element, "text-role")?
                        .unwrap_or_else(|| "text".into());
                    let field = field_access(field.split('.'));
                    quote!(|__xr_item| __xr_item #field)
                } else {
                    quote!(|__xr_item| __xr_item)
                };
                return Ok(quote! {
                    ::xrack_runtime::#rows(&#model, __range, #label)
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
            let create = if repeat.delegates.is_empty() {
                TokenStream::new()
            } else {
                let mut instances = TokenStream::new();
                let locale = self
                    .with_translations
                    .then(|| quote!(__locale: __locale.clone(),));
                let init_locale = self
                    .with_translations
                    .then(|| quote!(let __locale = self.__state.__locale.clone();));
                let capture_locale = self
                    .with_translations
                    .then(|| quote!(let __locale = __locale.clone();));
                for (_, delegate) in &repeat.delegates {
                    let component = delegate.attr("component")?;
                    let name = ident(component);
                    let inputs = format_ident!("__Inputs_{}", rust_name(component));
                    let table = table_history_configured(element).then_some(element);
                    let bindings = self.instance_inputs(*delegate, Some(model), table)?;
                    let instance = quote!(#name::__new(#inputs { #bindings #locale ..::core::default::Default::default() }));
                    if repeat.table {
                        instances.extend(quote!({
                            let __context = __context.clone();
                            let __parent = __parent.clone();
                            #capture_locale
                            ::std::cell::LazyCell::new(
                                ::std::boxed::Box::new(move || #instance) as ::std::boxed::Box<dyn FnOnce() -> #name>
                            )
                        },));
                    } else {
                        instances.extend(quote!(#instance,));
                    }
                }
                quote!(|__context| {
                    #init_locale
                    let __parent = ::std::rc::Rc::downgrade(&self.__state);
                    (#instances)
                },)
            };
            let cache = self
                .use_visual_plan
                .then(|| RowSceneCache::from_operation(row))
                .flatten()
                .filter(|_| !repeat.delegates.is_empty());
            let reads_index = cache == Some(RowSceneCache::Indexed);
            let render_indices = |indices: TokenStream| {
                if cache.is_some() {
                    quote!(repeater.render_cached_rows(|| #model_expr, #indices, #reads_index, #create #row_expr))
                } else {
                    quote!(repeater.render_indices(|| #model_expr, #indices, #create #row_expr))
                }
            };
            let render_range = if cache.is_some() {
                render_indices(quote!(__range))
            } else {
                quote!(repeater.render_range(|| #model_expr, __range, #create #row_expr))
            };
            if element.attr("kind") == Ok("TreeView") {
                let tree = self.tree_snapshot(
                    element,
                    row,
                    quote!(self.__state.__visuals.#field),
                    &model_expr,
                )?;
                let rows = render_indices(quote!(rows.iter().map(|row| row.model_index)));
                return Ok(quote! {{
                        let repeater = &self.__state.__visuals.#field;
                        let tree = #tree;
                        let rows = tree.rows().get(__range).unwrap_or_default();
                        #rows
                }});
            }
            if element.attr("kind") == Ok("TableView") || has_list_visibility(element) {
                let rows = if element.attr("kind") == Ok("TableView") {
                    self.table_model(element)?
                } else {
                    let visible = visible_list_model(element_key(element)?);
                    quote!(&self.#visible())
                };
                let track_columns = (repeat.table && repeat.delegates.is_empty()).then(
                    || quote!(let __xr_columns_changed = repeater.column_window(__xr_columns);),
                );
                let reordered = render_indices(quote!(
                    __range.filter_map(|row| positions.source_index(row))
                ));
                let range = render_range;
                return Ok(quote! {{
                        let repeater = &self.__state.__visuals.#field;
                        #track_columns
                        let rows = #rows;
                        let positions = rows.positions();
                        if positions.reordered() {
                            #reordered
                        } else {
                            #range
                        }
                }});
            }
            return Ok(quote! {
                let repeater = &self.__state.__visuals.#field;
                #render_range
            });
        }
        Ok(quote!(::std::vec::Vec::new()))
    }

    fn delegate_event_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
        delegates: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            let component = ident(element.attr("component")?);
            delegates.extend(quote! {
                [#(#path,)* rest @ ..] => Some(scene.at_path(&[#(#path),*]).map_or_else(
                    ::core::default::Default::default,
                    |child| {
                        let mut ui = #component { __state: self.__state.#field.__state.clone() };
                        visit(&mut ui, child, rest)
                    },
                )),
            });
        } else if element.attr("kind") == Ok("StandardTableView") {
            let editors = self.standard_table_editors(element)?;
            if editors.is_empty() {
                return Ok(());
            }
            let model = self.table_model(element)?;
            let writes = editors.iter().map(|StandardTableEditor { column, field, enabled, kind, ty }| {
                let field_path: Vec<_> = field.split('.').collect();
                let field = field_access(field_path.iter().copied());
                let update = quote!(|record| {
                        if record #field.to_string() != *original { return None; }
                        let mut updated = record.clone();
                        updated #field = next;
                        Some(updated)
                });
                let commit = self.table_cell_commit(element, quote!(rows.source()), field_path, ty.clone(), update)?;
                Ok(quote! {
                    #column if #enabled => {
                        let ::xrack_runtime::Value::#kind(next) = ::xrack_runtime::TableCellEditor::#kind.parse(text)? else {
                            return Some(false);
                        };
                        #commit
                    },
                })
            }).collect::<Result<TokenStream, String>>()?;
            out.extend(quote! {
                [#(#path,)* slot, 0, column] => {
                    let ::xrack_runtime::RoutedEvent::CommitCell { original, text } = action else { return Some(false); };
                    let view = scene.at_path(&[#(#path),*])?;
                    let row = view.children.get(*slot)?;
                    let key = row.row_key?;
                    let index = view.list?.first_row.checked_add(*slot)?;
                    let rows = #model;
                    if view.standard_list_stamp != Some(rows.positions().stamp()) { return Some(false); }
                    let index = rows.positions().source_index(index)?;
                    Some(match *column { #writes _ => false })
                }
            });
        } else if matches!(
            element.attr("kind"),
            Ok("ListView" | "GridView" | "TableView" | "TreeView")
        ) {
            let (row, model) = self.list_template(element)?;
            if row.attribute("delegate").is_some() {
                return self.item_delegate_events(element, row, model, path, out);
            }
            let repeat = &self.visuals.repeats[element_key(row)?];
            let model_index = if element.attr("kind") == Ok("TreeView") {
                let snapshot = tree_snapshot_method(element_key(element)?);
                quote! {
                    let tree = view.tree.as_ref()?;
                    let live = self.#snapshot();
                    let index = tree.event_source_index(&live, index, key)?;
                }
            } else if element.attr("kind") == Ok("TableView") {
                let model = self.table_model(element)?;
                quote! {
                    let rows = #model;
                    if view.standard_list_stamp != Some(rows.positions().stamp()) { return None; }
                    let index = rows.positions().source_index(index)?;
                }
            } else {
                self.list_source_index(element, &format_ident!("index"))?
            };
            for (delegate_slot, (delegate_path, delegate)) in repeat.delegates.iter().enumerate() {
                let field = repeat_field(repeat.index());
                let delegate_slot = Literal::usize_unsuffixed(delegate_slot);
                let component = ident(delegate.attr("component")?);
                let model_expr = self.expr(self.operand(model, 0)?)?;
                delegates.extend(quote! {
                    [#(#path,)* slot, 0, #(#delegate_path,)* rest @ ..] => Some((|| {
                        let view = scene.at_path(&[#(#path),*])?;
                        let row = view.children.get(*slot)?;
                        let index = view.list?.first_row.checked_add(*slot)?;
                        let key = row.row_key?;
                        let child = row.children.first()?.at_path(&[#(#delegate_path),*])?;
                        #model_index
                        self.__state.__visuals.#field.with_row(|| #model_expr, index, key, |delegates| {
                            let mut ui = #component { __state: delegates.#delegate_slot.__state.clone() };
                            visit(&mut ui, child, rest)
                        })
                    })().unwrap_or_default()),
                });
            }
            if matches!(element.attr("kind"), Ok("TableView" | "TreeView")) {
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
                    let field_path = self.record_path(operand)?;
                    let update = if !field_path.is_empty() {
                        let field = field_access(field_path.iter().copied());
                        let value = if value_type == Type::String {
                            quote!(next.into())
                        } else {
                            quote!(next)
                        };
                        quote! {
                            |record| {
                                if record #field != expected { return None; }
                                let mut updated = record.clone();
                                updated #field = #value;
                                Some(updated)
                            }
                        }
                    } else {
                        quote!(|record| (record == expected).then(|| next.into()))
                    };
                    let commit = self.table_cell_commit(
                        element,
                        quote!(model),
                        field_path,
                        value_type,
                        update,
                    )?;
                    writes.extend(quote!([#(#cell_path),*] => { #prepare #commit },));
                }
                if !writes.is_empty() {
                    let model_expr = if element.attr("kind") == Ok("TableView") {
                        quote!(rows.source())
                    } else {
                        let model = self.expr(self.operand(model, 0)?)?;
                        quote!(&#model)
                    };
                    out.extend(quote! {
                        [#(#path,)* slot, 0, rest @ ..] => Some((|| {
                            let view = scene.at_path(&[#(#path),*])?;
                            let row = view.children.get(*slot)?;
                            let index = view.list?.first_row.checked_add(*slot)?;
                            let cell = row.children.first()?.interactive_at_path(rest)?;
                            let key = row.row_key?;
                            #model_index
                            let model = #model_expr;
                            Some(match rest { #writes _ => false })
                        })().unwrap_or(false)),
                    });
                }
            }
        } else {
            if element.attr("kind") == Ok("SplitView") {
                let key = element_key(element)?;
                let field = split_state_field(key);
                let update = split_update_method(key);
                let (event, snapshot, validate) =
                    if children(element, "xr.split_fill").next().is_some() {
                        (
                            quote!(ResizeSplitSizes),
                            quote!(split_sizes),
                            quote!(resized_split_sizes),
                        )
                    } else {
                        (
                            quote!(ResizeSplit),
                            quote!(split_weights),
                            quote!(resized_split_weights),
                        )
                    };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::RoutedEvent::#event(values) = action else { return None; };
                        let node = scene.interactive_at_path(path)?;
                        if self.__state.#field.get(|| None).as_deref() != node.#snapshot.as_deref() {
                            return Some(false);
                        }
                        let Some(next) = node.#validate(values) else { return Some(false); };
                        Some(self.#update(Some(next)))
                    },
                });
            }
            for (index, child) in static_children(element) {
                path.push(index);
                self.delegate_event_arms(child, path, out, delegates)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn list_source_index(
        &self,
        element: OperationRef<'a>,
        index: &Ident,
    ) -> Result<TokenStream, String> {
        if !has_list_visibility(element) {
            return Ok(TokenStream::new());
        }
        let model = visible_list_model(element_key(element)?);
        Ok(quote! {
            let __xr_view_rows = self.#model();
            if view.standard_list_stamp != Some(__xr_view_rows.positions().stamp()) { return None; }
            let #index = __xr_view_rows.positions().source_index(#index)?;
        })
    }

    fn item_delegate_events(
        &self,
        element: OperationRef<'a>,
        row: OperationRef<'a>,
        repeat: OperationRef<'a>,
        path: &[usize],
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let parameters = RepeatRef::from_operation(repeat)?;
        let parameter = ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index = (!index.is_empty()).then(|| {
            let index = ident(index);
            quote!(let #index = __xr_index as ::core::primitive::i64;)
        });
        let parameters = quote! {
            let #parameter = ::xrack_runtime::ListModel::item(&*__xr_model, __xr_index);
            #index
        };
        let click = if let Some(action) = action_ref(row) {
            let activate = action_activate_method(action);
            quote! {
                if !matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked") |
                    ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::ActivateAction)) {
                    return Some(false);
                }
                Some(self.#activate())
            }
        } else if let Some(link) = children(row, "xr.row_link").next() {
            let operand = self.operand(link, 0)?;
            let field = field_access(self.record_path(operand)?);
            let (expected, next, changed) = if link.attr("name")? == "check-state" {
                let changed = self.typed_event(row, "check-state-changed", &quote!(__xr_next))?;
                (
                    quote!(node.check_state()),
                    quote!(__xr_expected.next(node.boolean("tristate", false))),
                    changed,
                )
            } else {
                (
                    quote!(node.boolean("checked", false)),
                    quote!(!__xr_expected),
                    TokenStream::new(),
                )
            };
            let toggled = self.typed_event_values(row, "toggled", &[])?;
            quote! {
                if !matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked")) { return Some(false); }
                let __xr_expected = #expected;
                let __xr_next = #next;
                if !__xr_model.update_row_if_key(__xr_index, __xr_key, |record| {
                    if record #field != __xr_expected { return None; }
                    let mut record = record.clone();
                    record #field = __xr_next;
                    Some(record)
                }) { return Some(false); }
                #parameters
                #toggled
                #changed
                Some(true)
            }
        } else if let Some(handler) =
            children(row, "xr.handler").find(|handler| handler.attr("name") == Ok("clicked"))
        {
            let statements = self.statements(handler)?;
            quote! {
                if !matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked")) { return Some(false); }
                #parameters
                #statements
                Some(true)
            }
        } else {
            return Ok(());
        };
        let field = repeat_field(self.visuals.repeats[element_key(row)?].index());
        let model = self.expr(self.operand(repeat, 0)?)?;
        let source_index = self.list_source_index(element, &format_ident!("__xr_index"))?;
        out.extend(quote! {
            [#(#path,)* slot, 0] => {
                let view = scene.interactive_at_path(&[#(#path),*])?;
                let __xr_index = view.list?.first_row.checked_add(*slot)?;
                #source_index
                let __xr_key = view.children.get(*slot)?.row_key?;
                let node = view.interactive_at_path(&[*slot, 0])?;
                let __xr_model = self.__state.__visuals.#field.resolve_model(|| #model);
                if __xr_index >= ::xrack_runtime::ListModel::row_count(&*__xr_model)
                    || ::xrack_runtime::ListModel::row_key(&*__xr_model, __xr_index) != __xr_key {
                    return Some(false);
                }
                #click
            },
        });
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
        let binding = |property| {
            children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(property))
        };
        let initial = if let Some(binding) = binding(name) {
            self.expr(self.operand(binding, 0)?)?
        } else {
            let kind = element.attr("kind")?;
            let default = control_default(kind, name, &ty)
                .ok_or_else(|| format!("unsupported control default {kind}.{name}"))?;
            let fallback = |value| match value {
                ControlLiteral::TypeDefault => Ok(quote!(::core::default::Default::default())),
                ControlLiteral::Literal(value) => literal(value, ty.clone()),
            };
            match default {
                ControlDefault::Value(value) => fallback(value)?,
                ControlDefault::Binding(property, value) => match binding(property) {
                    Some(binding) => self.visual_value(binding, &self.visuals)?,
                    None => fallback(value)?,
                },
                ControlDefault::Control(property) => self.control_value(element, property, ty)?,
            }
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
            if read.name() == "xr.check_group_read" {
                let set = check_group_method(owner(read), "set");
                Ok(quote!(self.#set(#value);))
            } else {
                Ok(self.set_property(owner(read), read.attr("value")?, value))
            }
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

    fn standard_table_fields(&self, element: OperationRef<'a>) -> Result<Vec<TokenStream>, String> {
        table_columns(element)
            .map(|column| {
                let binding = children(column, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("field"))
                    .ok_or("standard table column requires a field")?;
                let name: String = serde_json::from_str(self.operand(binding, 0)?.attr("value")?)
                    .map_err(|error| error.to_string())?;
                Ok(field_access(name.split('.')))
            })
            .collect()
    }

    fn standard_table_editors(
        &self,
        element: OperationRef<'a>,
    ) -> Result<Vec<StandardTableEditor>, String> {
        let model = model_binding(element).ok_or("standard table requires a model")?;
        let Type::RecordList(record) = value_type(self.operand(model, 0)?)? else {
            return Err("standard table requires a record model".into());
        };
        let mut editors = Vec::new();
        for (column, element) in table_columns(element).enumerate() {
            let Some(binding) =
                children(element, "xr.bind").find(|binding| binding.attr("name") == Ok("editable"))
            else {
                continue;
            };
            let name = self
                .record_field_binding(element, "field")?
                .ok_or("standard table column requires a field")?;
            let ty = self.record_field_type(record.clone(), &name)?;
            let kind = match ty {
                Type::String => "String",
                Type::Int => "Int",
                Type::Float => "Float",
                Type::Bool => "Bool",
                Type::Date => "Date",
                Type::Time => "Time",
                _ => return Err("unsupported editable table field type".into()),
            };
            editors.push(StandardTableEditor {
                column,
                field: name,
                ty,
                enabled: self.visual_value(binding, &self.visuals)?,
                kind: ident(kind),
            });
        }
        Ok(editors)
    }

    fn table_cell_commit(
        &self,
        element: OperationRef<'a>,
        model: TokenStream,
        path: Vec<&str>,
        ty: Type,
        update: TokenStream,
    ) -> Result<TokenStream, String> {
        let direct = quote!((#model).update_row_if_key(index, key, #update));
        if !table_history_configured(element) {
            return Ok(direct);
        }
        let history = table_history_field(element_key(element)?);
        let enabled = table_history_method(element_key(element)?, "enabled");
        let capture = Self::table_edit_capture(BTreeMap::from([(path, ty)]));
        Ok(quote! {
            if self.#enabled() {
                self.__state.#history.commit_cell(#model, index, key, #update, #capture)
            } else {
                let changed = #direct;
                if changed { self.__state.#history.clear(); }
                changed
            }
        })
    }

    fn table_edit_navigation_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            return Ok(());
        }
        if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView"))
            && table_option_configured(element, "edit-on-tab")
            && let Some((guard, projection)) = self.table_edit_navigation_projection(element)?
        {
            let rows = self.table_model(element)?;
            out.extend(quote! {
                [#(#path),*] => {
                    let view = scene.interactive_at_path(path)?;
                    if !view.boolean("edit-on-tab", false) { return None; }
                    #guard
                    let rows = #rows;
                    if view.standard_list_stamp != Some(rows.positions().stamp()) { return None; }
                    ::xrack_runtime::next_table_edit_cell(rows, origin, columns, backward, #projection)
                },
            });
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.table_edit_navigation_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn table_edit_navigation_projection(
        &self,
        element: OperationRef<'a>,
    ) -> Result<Option<(TokenStream, TokenStream)>, String> {
        if element.attr("kind") == Ok("StandardTableView") {
            return Ok(Some((
                quote! {
                    let header = view.table_header.as_ref()?;
                    let editable = |column: ::core::primitive::usize| {
                        header.columns.get(column).is_some_and(|column| column.boolean("editable", false))
                    };
                    if !columns.iter().copied().any(editable) { return None; }
                },
                quote!(|_, column, _| editable(column)),
            )));
        }
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let item = ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = (!index.is_empty()).then(|| {
            let index = ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        });
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            receiver: &receiver,
            repeat_parameters: true,
            hoist_repeat_subexpressions: false,
        });
        let mut columns = Vec::new();
        let mut editors = TokenStream::new();
        for (column, cell) in scene_children(row).enumerate() {
            if cell.attr("kind") == Ok("TableCell")
                && children(cell, "xr.row_link").next().is_some()
            {
                columns.push(column);
                let editable = self.row_editable(cell, &projection)?;
                editors.extend(quote!(#column => #editable,));
            }
        }
        if columns.is_empty() {
            return Ok(None);
        }
        let visible = self.row_shown(row, &projection)?;
        let enabled = self.row_binding_or(row, "enabled", quote!(true), &projection)?;
        Ok(Some((
            quote! {
                if !columns.iter().any(|column| [#(#columns),*].contains(column)) { return None; }
            },
            self.project_table_rows(
                &projection,
                quote!(__xr_index, column, __xr_record),
                quote! {
                    let #item = || (*__xr_record).clone();
                    #index_binding
                },
                quote!((#visible) && (#enabled) && match column { #editors _ => false }),
            )?,
        )))
    }

    fn table_clipboard_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        arms: &mut TableClipboardArms,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            return Ok(());
        }
        if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView"))
            && children(element, "xr.bind").any(|binding| binding.attr("name") == Ok("copyable"))
        {
            let model = self.table_model(element)?;
            let projection = if element.attr("kind") == Ok("StandardTableView") {
                let fields = self.standard_table_fields(element)?;
                let arms = fields.iter().enumerate().map(|(column, field)| {
                    quote!(#column => ::core::option::Option::Some(__xr_row #field.to_string()),)
                });
                quote!(|_, column, __xr_row| match column {
                    #(#arms)*
                    _ => ::core::option::Option::None,
                })
            } else {
                self.custom_table_projection(element)?
            };
            arms.copies.extend(quote! {
                [#(#path),*] => ::xrack_runtime::copy_table_text_in_order(#model, node, columns, #projection),
            });
        }
        if table_history_configured(element) {
            self.table_history_arms(element, path, arms)?;
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.table_clipboard_arms(child, path, arms)?;
            path.pop();
        }
        Ok(())
    }

    fn table_history_item(&self, element: OperationRef<'a>) -> Result<Option<Type>, String> {
        if !table_history_configured(element) {
            return Ok(None);
        }
        let model_source = if element.attr("kind") == Ok("StandardTableView") {
            model_binding(element).ok_or("standard table requires a model")?
        } else {
            self.list_template(element)?.1
        };
        Ok(value_type(self.operand(model_source, 0)?)?.list_item())
    }

    fn table_history_arms(
        &self,
        element: OperationRef<'a>,
        path: &[usize],
        arms: &mut TableClipboardArms,
    ) -> Result<(), String> {
        let Some(item) = self.table_history_item(element)? else {
            return Ok(());
        };
        let item = ty(&item);
        let key = element_key(element)?;
        let history = table_history_field(key);
        let active = table_history_method(key, "enabled");
        let available = table_history_method(key, "available");
        let replay = table_history_method(key, "replay");
        let source = self.view_source_model(element)?;
        let receiver = quote!(self);
        let state = TableProjection::new(ExpressionContext::new(&receiver));
        let flag = |name| self.row_binding_or(element, name, quote!(false), &state);
        let undoable = flag("undoable")?;
        let pasteable = flag("pasteable")?;
        arms.fields
            .extend(quote!(#history: ::xrack_runtime::TableEditHistory<#item>,));
        arms.defaults
            .extend(quote!(#history: ::core::default::Default::default(),));
        arms.replays.extend(quote! {
            [#(#path),*] => self.#replay(redo),
        });
        arms.methods.extend(quote! {
            fn #active(&self) -> ::core::primitive::bool {
                #undoable || #pasteable
            }
            fn #available(&self, redo: ::core::primitive::bool) -> ::core::primitive::bool {
                self.#active() && self.__state.#history.can_replay(&#source, redo)
            }
            fn #replay(&self, redo: ::core::primitive::bool) -> ::core::primitive::bool {
                self.#active() && self.__state.#history.replay(&#source, redo)
            }
        });
        if !table_option_configured(element, "pasteable") {
            return Ok(());
        }
        let Some((projection, capture)) = self.table_paste_projection(element)? else {
            return Ok(());
        };
        let method = format_ident!("__xr_table_paste_{}", rust_key(key));
        let model = self.table_rows_model(element)?;
        let column_order = table_order_field(key);
        let enabled = self.row_binding_or(element, "enabled", quote!(true), &state)?;
        let shown = self.row_shown(element, &state)?;
        arms.methods.extend(quote! {
                fn #method(
                    &self,
                    order_stamp: (::core::primitive::u64, ::core::primitive::u64),
                    column_order: &::xrack_runtime::TableColumnOrder,
                    input: ::xrack_runtime::TablePasteInput<'_>,
                ) -> ::core::primitive::bool {
                    let target = ::xrack_runtime::Property::default();
                    let read_target = || target.get(|| {
                        if !(#pasteable && #enabled && #shown)
                            || self.__state.#column_order.get(::core::default::Default::default) != *column_order
                        {
                            return ::core::option::Option::None;
                        }
                        let rows = #model;
                        let matches = rows.positions().stamp() == order_stamp
                            && rows.source().edit_stamp() == input.stamp;
                        matches.then_some(rows)
                    });
                    let ::core::option::Option::Some(rows) = read_target() else {
                        return false;
                    };
                    self.__state.#history.paste_in_order_if(
                        &rows, input, || !target.is_dirty() || read_target().is_some(),
                        #projection, #capture,
                    ).unwrap_or(false)
                }
            });
        arms.pastes.extend(quote! {
                [#(#path),*] => {
                    let rows = #model;
                    let stamp = rows.source().edit_stamp();
                    let order_stamp = rows.positions().stamp();
                    if node.standard_list_stamp != Some(order_stamp) {
                        return ::core::option::Option::None;
                    }
                    let column_order = node.table_column_order.clone();
                    let weak = self.as_weak();
                    ::core::option::Option::Some(::std::boxed::Box::new(move |text| {
                        let input = ::xrack_runtime::TablePasteInput { stamp, row, columns: &columns, text };
                        weak.upgrade().is_some_and(|ui| ui.#method(order_stamp, &column_order, input))
                    }))
                }
            });
        Ok(())
    }

    fn custom_table_projection(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let item = ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = if index.is_empty() {
            TokenStream::new()
        } else {
            let index = ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        };
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            receiver: &receiver,
            repeat_parameters: true,
            hoist_repeat_subexpressions: false,
        });
        let expression = |binding| self.expr_in(self.operand(binding, 0)?, projection.expression);
        let row_visible = self.row_shown(row, &projection)?;
        let mut arms = TokenStream::new();
        for (column, cell) in scene_children(row).enumerate() {
            let bindings = || children(cell, "xr.bind");
            let projected = bindings()
                .find(|binding| binding.attr("name") == Ok("clipboard-text"))
                .or_else(|| children(cell, "xr.row_link").next())
                .or_else(|| {
                    matches!(cell.attr("kind"), Ok("Text" | "TableCell"))
                        .then(|| bindings().find(|binding| binding.attr("name") == Ok("text")))
                        .flatten()
                });
            let text = if let Some(binding) = projected {
                let value = expression(binding)?;
                if binding.name() == "xr.bind" && binding.attr("name") == Ok("text") {
                    self.row_state_value(cell, "text", value, &projection)?
                } else {
                    match value_type(self.operand(binding, 0)?)? {
                        Type::String => value,
                        Type::CheckState => quote!(::std::format!("{:?}", #value)),
                        _ => quote!((#value).to_string()),
                    }
                }
            } else if matches!(cell.attr("kind"), Ok("Text" | "TableCell"))
                && children(cell, "xr.state_set").any(|case| case.attr("name") == Ok("text"))
            {
                self.row_state_value(
                    cell,
                    "text",
                    quote!(::std::string::String::new()),
                    &projection,
                )?
            } else {
                continue;
            };
            let shown = self.row_shown(cell, &projection)?;
            arms.extend(quote!(#column if #shown => ::core::option::Option::Some(#text),));
        }
        self.project_table_rows(
            &projection,
            quote!(__xr_index, __xr_column, __xr_row),
            quote! {
                let #item = || (*__xr_row).clone();
                #index_binding
            },
            quote! {
                if #row_visible {
                    match __xr_column { #arms _ => ::core::option::Option::None }
                } else {
                    ::core::option::Option::None
                }
            },
        )
    }

    fn row_shown(
        &self,
        element: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let guards = children(element, "xr.show_if")
            .map(|guard| self.expr_in(self.operand(guard, 0)?, projection.expression))
            .collect::<Result<Vec<_>, _>>()?;
        let visible = self.row_binding_or(element, "visible", quote!(true), projection)?;
        Ok(quote!(#((#guards) &&)* (#visible)))
    }

    fn table_paste_projection(
        &self,
        element: OperationRef<'a>,
    ) -> Result<Option<(TokenStream, TokenStream)>, String> {
        if element.attr("kind") == Ok("StandardTableView") {
            let editors = self.standard_table_editors(element)?;
            if editors.is_empty() {
                return Ok(None);
            }
            let mut edited_fields = BTreeMap::new();
            let mut writes = TokenStream::new();
            for StandardTableEditor {
                column,
                field,
                ty,
                enabled,
                kind,
            } in &editors
            {
                let path: Vec<_> = field.split('.').collect();
                let access = field_access(path.iter().copied());
                edited_fields.insert(path, ty.clone());
                writes.extend(quote! {
                    #column if #enabled => {
                        let ::xrack_runtime::Value::#kind(value) =
                            ::xrack_runtime::TableCellEditor::#kind.parse(text)?
                        else {
                            return ::core::option::Option::None;
                        };
                        next #access = value;
                        ::core::option::Option::Some(())
                    }
                });
            }
            let projection = quote!(|_, column, _, next, text| match column {
                #writes
                _ => ::core::option::Option::None,
            });
            return Ok(Some((projection, Self::table_edit_capture(edited_fields))));
        }
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let item = ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = (!index.is_empty()).then(|| {
            let index = ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        });
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            receiver: &receiver,
            repeat_parameters: true,
            hoist_repeat_subexpressions: false,
        });
        let row_shown = self.row_shown(row, &projection)?;
        let row_enabled = self.row_binding_or(row, "enabled", quote!(true), &projection)?;
        let mut writes = TokenStream::new();
        let mut edited_fields = BTreeMap::new();
        for (column, cell) in scene_children(row).enumerate() {
            if let Some((parser, path)) = cell_clipboard_parser(cell) {
                let target = parser.attr("target")?;
                let link = children(path[0], "xr.instance_link")
                    .find(|link| link.attribute("name") == Some(target))
                    .ok_or("missing clipboard target link")?;
                let field = self.paste_field(self.operand(link, 0)?, &mut edited_fields)?;
                let result = children(parser, "xr.return")
                    .next()
                    .ok_or("missing clipboard parser result")?;
                let value = self.expr_in(self.operand(result, 0)?, projection.expression)?;
                let text = ident(parser.attr("text_parameter")?);
                let guards = path
                    .into_iter()
                    .rev()
                    .map(|node| self.row_editable(node, &projection))
                    .collect::<Result<Vec<_>, _>>()?;
                writes.extend(quote! {
                    #column if true #(&& (#guards))* => {
                        let #text = || __xr_text.to_owned();
                        let __xr_parsed = #value;
                        if !__xr_parsed.accepted {
                            return ::core::option::Option::None;
                        }
                        #field = __xr_parsed.value;
                        ::core::option::Option::Some(())
                    }
                });
                continue;
            }
            let Some(link) = children(cell, "xr.row_link").next() else {
                continue;
            };
            let operand = self.operand(link, 0)?;
            let field = self.paste_field(operand, &mut edited_fields)?;
            let allowed = self.row_editable(cell, &projection)?;
            let kind = cell.attr("kind")?;
            let editable = if matches!(kind, "SpinBox" | "DoubleSpinBox") {
                self.row_binding_or(cell, "editable", quote!(true), &projection)?
            } else {
                quote!(true)
            };
            let value = match value_type(operand)? {
                Type::String => quote!(__xr_text.to_owned()),
                Type::Bool => quote!(::xrack_runtime::table_paste_bool(__xr_text)?),
                Type::CheckState => {
                    let tristate =
                        self.row_binding_or(cell, "tristate", quote!(false), &projection)?;
                    quote!(::xrack_runtime::table_paste_check_state(__xr_text, #tristate)?)
                }
                ty @ (Type::Int | Type::Float) => {
                    let (default_min, default_max, variant) = if ty == Type::Int {
                        (quote!(0_i64), quote!(100_i64), quote!(Int))
                    } else {
                        (quote!(0.0_f64), quote!(100.0_f64), quote!(Float))
                    };
                    let min = self.row_binding_or(cell, "minimum", default_min, &projection)?;
                    let max = self.row_binding_or(cell, "maximum", default_max, &projection)?;
                    quote!({
                        let Some(::xrack_runtime::Value::#variant(value)) =
                            ::xrack_runtime::TableCellEditor::#variant.parse(__xr_text)
                        else {
                            return ::core::option::Option::None;
                        };
                        let minimum = #min;
                        let maximum = (#max).max(minimum);
                        if !(minimum..=maximum).contains(&value) {
                            return ::core::option::Option::None;
                        }
                        value
                    })
                }
                _ => return Err("unsupported table paste field type".into()),
            };
            writes.extend(quote! {
                #column if (#allowed) && (#editable) => {
                    #field = #value;
                    ::core::option::Option::Some(())
                }
            });
        }
        if writes.is_empty() {
            return Ok(None);
        }
        let projection = self.project_table_rows(
            &projection,
            quote!(__xr_index, __xr_column, __xr_current, __xr_next, __xr_text),
            quote! {
                let #item = || (*__xr_current).clone();
                #index_binding
            },
            quote! {
                if !(#row_shown) || !(#row_enabled) {
                    return ::core::option::Option::None;
                }
                match __xr_column { #writes _ => ::core::option::Option::None }
            },
        )?;
        Ok(Some((projection, Self::table_edit_capture(edited_fields))))
    }

    fn table_edit_capture(edited_fields: BTreeMap<Vec<&str>, Type>) -> TokenStream {
        let fields = edited_fields.into_iter().map(|(path, value_type)| {
            let (before, after, access) = if !path.is_empty() {
                let field = field_access(path);
                (
                    quote!(&before #field),
                    quote!(&after #field),
                    quote!(|row| &mut row #field),
                )
            } else {
                (quote!(before), quote!(after), quote!(|row| row))
            };
            let same = match value_type {
                Type::Float => quote!(|a, b| a == b || a.to_bits() == b.to_bits()),
                Type::Length => quote!(|a, b| a == b || a.0.to_bits() == b.0.to_bits()),
                _ => quote!(|a, b| a == b),
            };
            let bytes = match value_type {
                Type::String => quote!(|value| value.len()),
                Type::KeyEvent => quote!(|value| value.key.len().saturating_add(value.text.len())),
                Type::StyledText => quote!(|value| {
                    value.links().iter().fold(
                        value
                            .text()
                            .len()
                            .saturating_add(::core::mem::size_of_val(value.runs()))
                            .saturating_add(::core::mem::size_of_val(value.links())),
                        |size, link| size.saturating_add(link.href.len()),
                    )
                }),
                Type::Brush => quote!(|value| match value {
                    ::xrack_runtime::Brush::Solid(_) => 0,
                    ::xrack_runtime::Brush::Linear { stops, .. }
                    | ::xrack_runtime::Brush::Radial { stops, .. } =>
                        ::core::mem::size_of_val(stops.as_ref()),
                }),
                _ => quote!(|_| 0),
            };
            quote!(patch.record(#before, #after, #access, #same, #bytes);)
        });
        quote!(|before, after, patch| { #(#fields)* })
    }

    fn paste_field(
        &self,
        operand: OperationRef<'a>,
        edited: &mut BTreeMap<Vec<&'a str>, Type>,
    ) -> Result<TokenStream, String> {
        let mut path = self.record_path(operand)?;
        let target = if path.is_empty() {
            quote!(*__xr_next)
        } else {
            let field = field_access(path.iter().copied());
            quote!(__xr_next #field)
        };
        self.record_paste_leaves(&mut path, &value_type(operand)?, edited);
        Ok(target)
    }

    fn record_paste_leaves(
        &self,
        path: &mut Vec<&'a str>,
        ty: &Type,
        edited: &mut BTreeMap<Vec<&'a str>, Type>,
    ) {
        if let Type::Record(name) = ty {
            for field in &self.records[name.as_ref()] {
                path.push(&field.name);
                self.record_paste_leaves(path, &field.ty, edited);
                path.pop();
            }
        } else {
            edited.insert(path.clone(), ty.clone());
        }
    }

    /// Share lazy state selectors across the cells sampled from one row.
    fn project_table_rows(
        &self,
        projection: &TableProjection<'_, 'a>,
        arguments: TokenStream,
        bindings: TokenStream,
        body: TokenStream,
    ) -> Result<TokenStream, String> {
        let selectors = projection.selectors.borrow();
        if selectors.is_empty() {
            return Ok(quote!(|#arguments| { #bindings #body }));
        }
        let mut caches = TokenStream::new();
        let mut resets = TokenStream::new();
        let mut reads = TokenStream::new();
        for (&owner, &selector) in selectors.iter() {
            let name = state_selector_name(owner);
            let cache = format_ident!("{name}_cache");
            let value = self.state_choice(selector, |condition| {
                self.expr_in(condition, projection.expression)
            })?;
            caches.extend(quote! {
                let mut #cache =
                    ::xrack_runtime::Property::<::core::primitive::i64>::default();
            });
            resets.extend(quote!(#cache = ::core::default::Default::default();));
            reads.extend(quote!(let #name = || #cache.get(|| #value);));
        }
        Ok(quote!({
            let mut __xr_sampled_row = ::core::option::Option::None;
            #caches
            move |#arguments| {
                if __xr_sampled_row != ::core::option::Option::Some(__xr_index) {
                    __xr_sampled_row = ::core::option::Option::Some(__xr_index);
                    #resets
                }
                #bindings
                #reads
                #body
            }
        }))
    }

    fn row_editable(
        &self,
        element: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let shown = self.row_shown(element, projection)?;
        let enabled = self.row_binding_or(element, "enabled", quote!(true), projection)?;
        let read_only = self.row_binding_or(element, "read-only", quote!(false), projection)?;
        Ok(quote!((#shown) && (#enabled) && !(#read_only)))
    }

    fn row_binding_or(
        &self,
        element: OperationRef<'a>,
        property: &str,
        default: TokenStream,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let base = element
            .children()
            .find(|binding| {
                matches!(binding.name(), "xr.bind" | "xr.layout_bind")
                    && binding.attr("name") == Ok(property)
            })
            .map(|binding| self.expr_in(self.operand(binding, 0)?, projection.expression))
            .transpose()?
            .unwrap_or(default);
        self.row_state_value(element, property, base, projection)
    }

    fn row_state_value(
        &self,
        element: OperationRef<'a>,
        property: &str,
        base: TokenStream,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let cases: Vec<_> = children(element, "xr.state_set")
            .filter(|case| case.attr("name") == Ok(property))
            .collect();
        let Some(first) = cases.first() else {
            return Ok(base);
        };
        let selector = self.operand(*first, 0)?;
        let owner = selector.attr("owner")?;
        let name = state_selector_name(owner);
        let selected = if self.persistent_state_selectors.contains_key(owner) {
            let receiver = projection.expression.receiver;
            quote!(#receiver.#name())
        } else if projection.expression.repeat_parameters {
            projection.selectors.borrow_mut().insert(owner, selector);
            quote!(#name())
        } else {
            self.state_choice(selector, |condition| {
                self.expr_in(condition, projection.expression)
            })?
        };
        let mut arms = TokenStream::new();
        for case in cases {
            let case = StateCaseRef::from_operation(case)?;
            let index = case.index;
            let value = case
                .value
                .defining_operation()
                .ok_or("state value has no definition")?;
            let value = self.expr_in(value, projection.expression)?;
            arms.extend(quote!(#index => #value,));
        }
        Ok(quote!(match #selected { #arms _ => #base }))
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
        let order = table_order_field(key);
        let heights = table_row_heights_field(key);
        let selection = table_selection_field(key);
        let snapshot = table_selection_snapshot_method(key);
        let base = quote!(::xrack_runtime::ListMetrics::row_height(&#name));
        let geometry =
            if let Some(projected) = self.project_row_heights(element, name, model, None)? {
                quote!(self.__state.#heights.snapshot_over(#model, #base, #projected))
            } else {
                quote!(self.__state.#heights.snapshot(#model, #base))
            };
        out.extend(quote! {
            #name.table_header = Some(::std::boxed::Box::new(::xrack_runtime::TableHeader {
                columns: ::std::vec![#(#columns),*],
                sort: self.__state.#sort.get(|| None),
                column_widths: self.__state.#widths.get(::std::vec::Vec::new),
            }));
            #name.table_column_order = self.__state.#order.get(::core::default::Default::default);
            #name.row_heights = #geometry;
            let target = #name.selection_target();
            let multiple = #name.boolean("multi-select", false);
            let snapshot = self.#snapshot();
            if multiple || target != ::xrack_runtime::SelectionTarget::Rows || self.__state.#selection.has_explicit_selection() {
                #name.table_selection = Some(::std::boxed::Box::new(snapshot));
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
        let text = self
            .record_field_binding(element, "text-role")?
            .unwrap_or_else(|| "text".into());
        let value = self
            .record_field_binding(element, "value-role")?
            .unwrap_or_else(|| text.clone());
        let ty = self.record_field_type(record, &value)?;
        Ok((text, value, ty))
    }

    fn record_field_type(&self, record: Box<str>, path: &str) -> Result<Type, String> {
        let mut ty = Type::Record(record);
        for name in path.split('.') {
            let Type::Record(record) = ty else {
                return Err(format!("missing verified record field {path}"));
            };
            ty = self.records[record.as_ref()]
                .iter()
                .find(|field| field.name == name)
                .ok_or_else(|| format!("missing verified record field {path}"))?
                .ty
                .clone();
        }
        Ok(ty)
    }

    fn record_field_binding(
        &self,
        element: OperationRef<'a>,
        name: &str,
    ) -> Result<Option<String>, String> {
        xrack_mlir::record_role(element, name)
    }

    fn project_row_heights(
        &self,
        element: OperationRef<'a>,
        node: &Ident,
        model: &TokenStream,
        tree: Option<&TokenStream>,
    ) -> Result<Option<TokenStream>, String> {
        let Some(field) = self.record_field_binding(element, "row-height-field")? else {
            return Ok(None);
        };
        let field = field_access(field.split('.'));
        let projection = row_height_projection_field(element_key(element)?);
        let base = quote!(::xrack_runtime::ListMetrics::row_height(&#node));
        let value = if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView"))
            || has_list_visibility(element)
        {
            quote!(self.__state.#projection.table_snapshot((#model).source(), #base, (#model).positions(), |row| row #field))
        } else if let Some(tree) = tree {
            quote!(self.__state.#projection.tree_snapshot(#model, #base, #tree, |row| row #field))
        } else {
            quote!(self.__state.#projection.snapshot(#model, #base, |row| row #field))
        };
        Ok(Some(value))
    }

    fn table_model(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let rows = self.table_rows_model(element)?;
        Ok(quote!(&#rows))
    }

    fn table_rows_model(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        if element.attr("kind") == Ok("StandardTableView")
            || children(element, "xr.bind")
                .any(|binding| binding.attr("name") == Ok("row-visible-field"))
        {
            let rows = table_rows_model_method(element_key(element)?);
            return Ok(quote!(self.#rows()));
        }
        let source = self.view_source_model(element)?;
        let order = table_row_order_field(element_key(element)?);
        Ok(quote!(self.__state.#order.rows(#source)))
    }

    fn view_source_model(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        if element.attr("kind") == Ok("StandardTableView") {
            let model = self.standard_table_model(element, &self.visuals)?;
            Ok(quote!(::std::rc::Rc::new(#model)))
        } else if element.attr("kind") == Ok("StandardListView") {
            let model = model_binding(element)
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .unwrap_or_else(|| quote!(<::xrack_runtime::VecModel<::std::string::String> as ::core::default::Default>::default()));
            Ok(quote!(::std::rc::Rc::new(#model)))
        } else {
            let (row, repeat) = self.list_template(element)?;
            let repeater = repeat_field(self.visuals.repeats[element_key(row)?].index());
            let model = self.expr(self.operand(repeat, 0)?)?;
            Ok(quote!(self.__state.__visuals.#repeater.resolve_model(|| #model)))
        }
    }

    fn view_index_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        if element.attr("kind") == Ok("TreeView") {
            let snapshot = tree_snapshot_method(key);
            return Ok(quote! {
                let tree = self.#snapshot();
                if from_source { tree.visible_index_for_source(index) }
                else { tree.rows().get(index).map(|item| item.model_index) }
            });
        }
        let model = if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView")) {
            Some(self.table_model(element)?)
        } else if has_list_visibility(element) {
            let model = visible_list_model(key);
            Some(quote!(&self.#model()))
        } else {
            None
        };
        if let Some(model) = model {
            Ok(quote! {
                let rows = #model;
                if from_source { rows.positions().visual_index(index) }
                else { rows.positions().source_index(index) }
            })
        } else {
            let model = self.view_source_model(element)?;
            Ok(quote!((index < ::xrack_runtime::ListModel::row_count(&*#model)).then_some(index)))
        }
    }

    fn source_selection_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView")) {
            let select = table_selection_method(key);
            let snapshot = table_selection_snapshot_method(key);
            return Ok(quote! {
                let action = if let Some(column) = column {
                    let Ok(column) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(column) else { return; };
                    ::xrack_runtime::TableSelectionAction::Cell {
                        row: index, column, operation: ::xrack_runtime::RowSelection::Replace,
                    }
                } else {
                    let selection = self.#snapshot();
                    let operation = match selection.target() {
                        ::xrack_runtime::SelectionTarget::Rows => ::xrack_runtime::RowSelection::Replace,
                        ::xrack_runtime::SelectionTarget::Cells if selection.multiple() => ::xrack_runtime::RowSelection::Replace,
                        _ => ::xrack_runtime::RowSelection::Move,
                    };
                    ::xrack_runtime::TableSelectionAction::Row { index, operation }
                };
                self.#select(action);
            });
        }
        if element.attr("kind") == Ok("TreeView") {
            let snapshot = tree_snapshot_method(key);
            let select = tree_selection_method(key);
            let multiple = self.binding_value_or(element, "multi-select", quote!(false))?;
            return Ok(quote! {
                let tree = self.#snapshot();
                let Some(item) = tree.visible_index_for_source(source).and_then(|index| tree.rows().get(index))
                else { return; };
                let action = ::xrack_runtime::TreeSelectionAction::Row {
                    key: item.key, row: item.row, operation: ::xrack_runtime::RowSelection::Replace,
                };
                self.#select(&tree, #multiple, action);
            });
        }
        let current = self.control_value(element, "current-index", Type::Int)?;
        let value = quote!(index as ::core::primitive::i64);
        let set = self.set_control(element, &value)?;
        let changed = self.typed_event(element, "changed", &value)?;
        Ok(quote! {
            if #current != #value {
                #set
                #changed
            }
        })
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
        let label = field_access(text.split('.'));
        let value = field_access(value.split('.'));
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
        let project_label = quote!(|record| record #label.as_str());
        let project_value = quote!(|record| ::xrack_runtime::Value::from(record #value.clone()));
        let items_input = if let Some(enabled) =
            self.record_field_binding(element, "enabled-role")?
        {
            let enabled = field_access(enabled.split('.'));
            quote!(::xrack_runtime::ComboItems::with_enabled(#model, #project_label, #project_value, |record| record #enabled))
        } else {
            quote!(::xrack_runtime::ComboItems::new(#model, #project_label, #project_value))
        };
        out.extend(quote! {
            let __xr_combo = self.__state.#snapshot.get(|| {
                let __xr_items = self.__state.#items.get(|| #items_input);
                #selection
            });
            #node.properties.insert("model", ::xrack_runtime::Value::ComboModel(__xr_combo));
        });
        Ok(())
    }

    fn animation_timing(
        &self,
        op: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let animation = AnimationRef::from_operation(op)?;
        let expression = |value: ValueRef<'a>| {
            let value = value
                .defining_operation()
                .ok_or("animation operand has no definition")?;
            self.visual_expr(value, scope)
        };
        let duration = expression(animation.duration)?;
        let delay = animation.delay_ms;
        let easing = match animation.curve {
            Some([x1, y1, x2, y2]) => {
                quote!(::xrack_runtime::Easing::cubic_bezier(#x1, #y1, #x2, #y2).unwrap())
            }
            None => quote!(::xrack_runtime::Easing::LINEAR),
        };
        let timing = quote!(::xrack_runtime::Animation::new(#duration as ::core::primitive::u64, #delay, #easing));
        if let Some(enabled) = animation.enabled {
            let enabled = expression(enabled)?;
            Ok(quote!(if #enabled { Some(#timing) } else { None }))
        } else {
            Ok(quote!(Some(#timing)))
        }
    }

    fn state_selector(&self, op: OperationRef<'a>) -> Result<Ident, String> {
        let selector = StateCaseRef::from_operation(op)?
            .selector
            .defining_operation()
            .ok_or("state selector has no definition")?;
        Ok(state_selector_name(selector.attr("owner")?))
    }

    fn render_state_selection(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if let Some(selector) = children(element, "xr.state_select").next() {
            let owner = selector.attr("owner")?;
            if !self.visual_state_selectors.contains(owner) {
                return Ok(());
            }
            let name = state_selector_name(owner);
            let value = if self.persistent_state_selectors.contains_key(owner) {
                quote!(self.#name())
            } else {
                self.state_choice(selector, |condition| self.visual_expr(condition, scope))?
            };
            out.extend(quote!(let #name = #value;));
        }
        Ok(())
    }

    fn state_choice(
        &self,
        selector: OperationRef<'a>,
        mut expression: impl FnMut(OperationRef<'a>) -> Result<TokenStream, String>,
    ) -> Result<TokenStream, String> {
        let mut conditions = Vec::new();
        for index in 0.. {
            let Some(value) = selector.operand(index) else {
                break;
            };
            let definition = value
                .defining_operation()
                .ok_or("state condition has no definition")?;
            conditions.push(expression(definition)?);
        }
        let mut value = quote!(-1i64);
        for (index, condition) in conditions.into_iter().enumerate().rev() {
            let index = index as i64;
            value = quote!(if #condition { #index } else { #value });
        }
        Ok(value)
    }

    fn render_state_overrides(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        node: &Ident,
        overrides: &BTreeMap<&str, Vec<OperationRef<'a>>>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let action = action_ref(element).is_some();
        for (&key, cases) in overrides {
            let selected = self.state_selector(cases[0])?;
            let wrap = |value: TokenStream| {
                if key == "enabled" && action {
                    quote!(Some(::xrack_runtime::Value::from(#node.boolean("enabled", true) && (#value))))
                } else {
                    quote!(Some(::xrack_runtime::Value::from(#value)))
                }
            };
            let mut choices = TokenStream::new();
            for &case in cases {
                let case = StateCaseRef::from_operation(case)?;
                let index = case.index;
                let value = case
                    .value
                    .defining_operation()
                    .ok_or("state value has no definition")?;
                let value = wrap(self.visual_expr(value, scope)?);
                choices.extend(quote!(#index => #value,));
            }
            let base_op = if element.name() == "xr.instance" {
                "xr.layout_bind"
            } else {
                "xr.bind"
            };
            let base = children(element, base_op)
                .find(|binding| binding.attribute("name") == Some(key))
                .map(|binding| self.visual_value(binding, scope).map(wrap))
                .transpose()?
                .unwrap_or_else(|| quote!(None));
            out.extend(quote! {
                if let Some(value) = match #selected { #choices _ => #base } {
                    #node.properties.insert(#key, value);
                }
            });
        }
        let mut timings = BTreeMap::<_, Vec<_>>::new();
        for timing in children(element, "xr.state_timing") {
            timings
                .entry(timing.attr("name")?)
                .or_default()
                .push(timing);
        }
        for (key, timings) in timings {
            let selected = self.state_selector(timings[0])?;
            let mut enter = TokenStream::new();
            let mut exit = TokenStream::new();
            for timing in timings {
                let index = StateCaseRef::from_operation(timing)?.index;
                let value = self.animation_timing(timing, scope)?;
                let cases = if timing.attr("direction")? == "in" {
                    &mut enter
                } else {
                    &mut exit
                };
                cases.extend(quote!(#index => #value,));
            }
            out.extend(quote! {
                #node.state_timings.get_or_insert_with(Default::default).insert(#key, ::xrack_runtime::StateTiming {
                    case: #selected,
                    enter: match #selected { #enter _ => None },
                    exit: match #selected { #exit _ => None },
                });
            });
        }
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
            let key = element_key(element)?;
            let ui = if let Some(&slot) = scope.row_delegates.get(key) {
                let slot = Literal::usize_unsuffixed(slot);
                quote!(__xr_delegates.#slot)
            } else {
                let instance = instance_field(key);
                quote!(self.__state.#instance)
            };
            out.extend(quote! {
                let mut #name = #ui.render();
                #name.id = #id;
            });
            "xr.layout_bind"
        } else {
            let kind = element.attr("kind")?;
            let application_window = kind == "ApplicationWindow";
            let kind = match kind {
                kind if picker_value(kind).is_some() => "Dialog",
                "ApplicationWindow" => "Window",
                "StandardTableView" => "TableView",
                "StandardButton" => "Button",
                "DayCell" | "SplitHandle" => "Rectangle",
                kind => kind,
            };
            out.extend(quote!(let mut #name = ::xrack_runtime::Node::new(#kind, #id);));
            if application_window {
                out.extend(quote!(#name.application_window = true;));
            }
            if kind == "StackPage" {
                let route = element.attr("id")?;
                out.extend(
                    quote!(#name.properties.insert("__stack-route", #route.to_owned().into());),
                );
            }
            if element.attribute("delegate").is_some() {
                out.extend(quote!(#name.properties.insert("__item-delegate", ::xrack_runtime::Value::Bool(true));));
            }
            if self.with_fonts && element.attr("id")? == "root" {
                out.extend(quote!(#name.fonts = ::core::option::Option::Some(&__XR_FONTS);));
            }
            "xr.bind"
        };
        self.render_state_selection(element, scope, out)?;
        if element.attribute("kind") == Some("FocusScope") {
            for handler in children(element, "xr.handler") {
                if let Some(phase) = key_phase(handler.attr("name")?) {
                    out.extend(
                        quote!(#name.key_handlers |= ::xrack_runtime::KeyPhase::#phase.mask();),
                    );
                }
            }
        }
        let key = element_key(element)?;
        if has_virtual_rows(element) {
            let field = list_renderer_field(key);
            let method = list_rows_method(key);
            out.extend(quote! {
                #name.list_row_renderer = Some(self.__state.#field.get_or_init(|| {
                    let __xr_weak = ::std::rc::Rc::downgrade(&self.__state);
                    ::xrack_runtime::ListRowRenderer::with_columns(move |__range, __xr_columns| {
                        __xr_weak.upgrade().map_or_else(::std::vec::Vec::new, |__state| {
                            Self { __state }.#method(__range, __xr_columns)
                        })
                    })
                }).clone());
            });
        }
        if self.focus_targets.contains(key) {
            let handle = focus_handle(key);
            out.extend(quote!(#name.focus_handle = Some(self.__state.#handle.clone());));
        }
        if element.attr("kind") == Ok("ScaleRotateGestureHandler") {
            let handle = scale_rotate_handle(key);
            out.extend(quote!(#name.scale_rotate_handle = Some(self.__state.#handle.clone());));
        }
        if element.attr("kind") == Ok("SwipeGestureHandler") {
            let handle = swipe_handle(key);
            out.extend(quote!(#name.swipe_handle = Some(self.__state.#handle.clone());));
        }
        if element.attr("kind") == Ok("HoverHandler") {
            let handle = hover_handle(key);
            out.extend(quote!(#name.hover_handle = Some(self.__state.#handle.clone());));
        }
        if self.pointer_targets.contains(key) {
            let handle = pointer_handle(key);
            out.extend(quote!(#name.pointer_handle = Some(self.__state.#handle.clone());));
            for handler in children(element, "xr.handler") {
                match handler.attr("name")? {
                    "scroll-event" => out.extend(quote!(#name.scroll_event_handler = true;)),
                    "press-and-hold" => out.extend(quote!(#name.press_and_hold_handler = true;)),
                    _ => {}
                }
            }
        }
        if self.viewport_targets.contains(key) {
            let handle = viewport_handle(key);
            out.extend(quote!(#name.viewport_handle = Some(self.__state.#handle.clone());));
        }
        if let Some(target) = children(element, "xr.scroll_target").next() {
            let handle = viewport_handle(target.attr("target")?);
            out.extend(quote!(#name.scroll_target = Some(self.__state.#handle.clone());));
        }
        if self.geometry_targets.contains(key) {
            let handle = geometry_handle(key);
            let field = if element.name() == "xr.instance" {
                quote!(instance_geometry_handle)
            } else {
                quote!(geometry_handle)
            };
            out.extend(quote!(#name.#field = Some(self.__state.#handle.clone());));
        }
        if let Some(target) = children(element, "xr.focus_target").next() {
            let handle = focus_handle(target.attr("target")?);
            out.extend(quote!(#name.initial_focus = Some(self.__state.#handle.clone());));
        }
        let mut state_overrides = BTreeMap::<_, Vec<_>>::new();
        for binding in children(element, "xr.state_set") {
            state_overrides
                .entry(binding.attr("name")?)
                .or_default()
                .push(binding);
        }
        let action = action_ref(element);
        let action_enabled = format_ident!("{name}_action_enabled");
        if let Some(action) = action {
            let render = action_render_method(action);
            out.extend(quote! {
                self.#render(&mut #name);
                let #action_enabled = #name.boolean("enabled", true);
            });
        }
        for binding in children(element, binding_op) {
            let key = binding.attr("name")?;
            if binding_op == "xr.bind"
                && matches!(
                    key,
                    "content-x"
                        | "content-y"
                        | "row-height-field"
                        | "row-visible-field"
                        | "row-header-field"
                        | "clipboard-text"
                )
            {
                continue;
            }
            if state_overrides.contains_key(key) {
                continue;
            }
            if record_combo_binding(element).is_some()
                && matches!(key, "model" | "text-role" | "value-role" | "enabled-role")
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
            let value = self.visual_value(binding, scope)?;
            if key == "model" && element.attr("kind") == Ok("Tumbler") {
                out.extend(quote! {
                    #name.tumbler_items = Some(::xrack_runtime::TumblerItems::new(#value));
                });
                continue;
            }
            out.extend(quote! {
                #name.properties.insert(#key, ::xrack_runtime::Value::from(#value));
            });
            if self.use_visual_plan && binding.attribute("xr.update_effect") == Some("paint") {
                out.extend(quote!(#name.mark_paint_only(#key);));
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
            out.extend(
                quote!(#name.properties.insert(#property, ::xrack_runtime::Value::from(#value));),
            );
            if element.attribute("delegate").is_none() {
                out.extend(
                    quote!(#name.properties.insert(#flag, ::xrack_runtime::Value::Bool(true));),
                );
            }
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
        self.render_state_overrides(element, scope, &name, &state_overrides, out)?;
        if action.is_some() {
            out.extend(quote! {
                let #action_enabled = #action_enabled && #name.boolean("enabled", true);
                #name.properties.insert("enabled", ::xrack_runtime::Value::Bool(#action_enabled));
            });
        }
        if element.attribute("kind") == Some("AnimatedImage") {
            let slot = playback_slot(key);
            let finished = format_ident!("{}_finished", slot);
            out.extend(quote! {
                let weak = self.as_weak();
                self.__state.#slot.populate(&mut #name, move || weak.upgrade().is_some_and(|ui| ui.#finished()));
            });
        }
        for animation in children(element, "xr.animate") {
            let key = animation.attr("name")?;
            let timing = self.animation_timing(animation, scope)?;
            out.extend(quote! {
                if let Some(timing) = #timing {
                    #name.animations.insert(#key, timing);
                }
            });
        }
        let kind = element.attribute("kind");
        let source_scroll = self.source_scroll_targets.contains(key);
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
            let populate = if element.attr("kind") == Ok("ColorPickerPopup") {
                format_ident!("populate_editor")
            } else {
                format_ident!("populate")
            };
            out.extend(quote! { self.__state.#popup.#populate(&mut #name, || #value); });
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
                #name.add_range_thumbs();
            });
        }
        if has_list_visibility(element) {
            let model = visible_list_model(element_key(element)?);
            let projected = self
                .project_row_heights(element, &name, &quote!(&*__xr_view_model), None)?
                .map(|value| quote!(#name.row_heights = #value;));
            out.extend(quote! {
                let __xr_view_model = ::std::rc::Rc::new(self.#model());
                #name.list = Some(::xrack_runtime::ListMetrics {
                    row_count: ::xrack_runtime::ListModel::row_count(&*__xr_view_model), first_row: 0,
                });
                #name.standard_list_stamp = Some(__xr_view_model.positions().stamp());
                if #source_scroll || #name.boolean("preserve-scroll-anchor", false) {
                    #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(__xr_view_model.clone()));
                }
                #projected
            });
            return Ok(name);
        }
        if kind == Some("StandardListView") {
            if let Some(binding) = model_binding(element) {
                let model = self.visual_value(binding, scope)?;
                let projected = self
                    .project_row_heights(element, &name, &quote!(&__xr_model), None)?
                    .map(|value| quote!(#name.row_heights = #value;));
                out.extend(quote! {
                    let __xr_model = #model;
                    #name.list = Some(::xrack_runtime::ListMetrics {
                        row_count: ::xrack_runtime::ListModel::row_count(&__xr_model), first_row: 0,
                    });
                    #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(&__xr_model));
                    #projected
                    if #source_scroll || #name.boolean("preserve-scroll-anchor", false) {
                        #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(::std::rc::Rc::new(__xr_model)));
                    }
                });
            } else {
                out.extend(quote! { #name.list = Some(::xrack_runtime::ListMetrics::default()); });
            }
            return Ok(name);
        }
        if kind == Some("StandardTableView") {
            let key = element_key(element)?;
            let rows = table_rows_model_method(key);
            out.extend(quote! {
                let __xr_table_model = ::std::rc::Rc::new(self.#rows());
                if __xr_table_model.positions().projected() {
                    #name.properties.insert("movable-rows", ::xrack_runtime::Value::Bool(false));
                }
                #name.list = Some(::xrack_runtime::ListMetrics {
                    row_count: ::xrack_runtime::ListModel::row_count(&*__xr_table_model), first_row: 0,
                });
                #name.standard_list_stamp = Some(__xr_table_model.positions().stamp());
                if #source_scroll || #name.boolean("preserve-scroll-anchor", false) || #name.boolean("movable-rows", false) {
                    #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(__xr_table_model.clone()));
                }
                #name.properties.insert("__standard-table", ::xrack_runtime::Value::Bool(true));
                #name.properties.entry("follow-current").or_insert(::xrack_runtime::Value::Bool(true));
            });
            self.render_table_header(
                element,
                scope,
                out,
                next,
                &name,
                &quote!(&*__xr_table_model),
            )?;
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
                let tree_model = quote!(&*#receiver.#field.resolve_model(|| #model));
                let projected = self
                    .project_row_heights(element, &name, &tree_model, Some(&quote!(&tree)))?
                    .map(|value| quote!(#name.row_heights = #value;));
                out.extend(quote! {
                    let tree = #tree;
                    #name.list = Some(::xrack_runtime::ListMetrics { row_count: tree.rows().len(), first_row: 0 });
                    if #name.boolean("multi-select", false) {
                        #name.tree_selection = Some(self.__state.#state.selection());
                    }
                    #projected
                    #name.tree = Some(tree);
                });
                return Ok(name);
            }
            if kind == Some("TableView") {
                let rows = self.table_rows_model(element)?;
                out.extend(quote! {
                    let __xr_view_model = ::std::rc::Rc::new(#rows);
                    if __xr_view_model.positions().projected() {
                        #name.properties.insert("movable-rows", ::xrack_runtime::Value::Bool(false));
                    }
                    #name.list = Some(::xrack_runtime::ListMetrics {
                        row_count: ::xrack_runtime::ListModel::row_count(&*__xr_view_model), first_row: 0,
                    });
                    #name.standard_list_stamp = Some(__xr_view_model.positions().stamp());
                    if #source_scroll || #name.boolean("preserve-scroll-anchor", false) || #name.boolean("movable-rows", false) {
                        #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(__xr_view_model.clone()));
                    }
                });
                self.render_table_header(
                    element,
                    scope,
                    out,
                    next,
                    &name,
                    &quote!(&*__xr_view_model),
                )?;
                return Ok(name);
            }
            out.extend(quote! {
                let __xr_view_model = #receiver.#field.resolve_model(|| #model);
                #name.list = Some(::xrack_runtime::ListMetrics {
                    row_count: ::xrack_runtime::ListModel::row_count(&*__xr_view_model), first_row: 0,
                });
                #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(&*__xr_view_model));
                if #source_scroll || #name.boolean("preserve-scroll-anchor", false) {
                    #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(__xr_view_model.clone()));
                }
            });
            if let Some(projected) =
                self.project_row_heights(element, &name, &quote!(&*__xr_view_model), None)?
            {
                out.extend(quote!(#name.row_heights = #projected;));
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
        let mut table_cells = TokenStream::new();
        let mut table_columns = 0usize;
        for (index, child) in scene_children(element).enumerate() {
            if kind == Some("TableRow") {
                let mut cell = TokenStream::new();
                let child = self.render(child, scope, &mut cell, next)?;
                table_cells.extend(quote! {
                    #index => {
                        #cell
                        #name.children.push(#child);
                    },
                });
                table_columns = index + 1;
                continue;
            }
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
        if kind == Some("TableRow") {
            content.extend(quote! {
                #name.table_column_window = __xr_columns.clone();
                for __xr_column in __xr_columns.columns(#table_columns) {
                    match __xr_column {
                        #table_cells
                        _ => ::core::unreachable!(),
                    }
                }
            });
        }
        if let Some(template) = day_cell_template(element) {
            let field = calendar_cells_field(element_key(element)?);
            let mut cell = TokenStream::new();
            let template = self.render(template, &VisualScope::default(), &mut cell, next)?;
            let inputs = day_cell_inputs().map(|(name, _)| {
                let binding = ident(name);
                let field = ident(name.trim_start_matches("cell-"));
                quote!(let #binding = __xr_cell.#field;)
            });
            content.extend(quote! {
                if #name.is_visible() {
                    let __xr_cells = self.__state.#field.render(&#name, |__xr_cell| {
                        #(#inputs)*
                        #cell
                        #template
                    });
                    #name.children.push(__xr_cells);
                }
            });
        }
        if kind.is_some_and(xrack_runtime::is_window_overlay) {
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
            let key = element_key(element)?;
            let field = split_state_field(key);
            let handle = split_resize_handle_field(key);
            if let Some(fill) = children(element, "xr.split_fill").next() {
                let target = fill.attr("target")?;
                out.extend(quote! {
                    #name.split_fill_pane = Some(#target);
                    #name.split_sizes = self.__state.#field.get(|| None);
                });
            } else {
                out.extend(quote!(#name.split_weights = self.__state.#field.get(|| None);));
            }
            out.extend(quote! {
                #name.split_resize_handle = Some(self.__state.#handle.clone());
                #name.add_split_handles();
            });
            if let Some(template) = children(element, "xr.element")
                .find(|child| child.attr("kind") == Ok("SplitHandle"))
            {
                let mut decoration = TokenStream::new();
                let template =
                    self.render(template, &VisualScope::default(), &mut decoration, next)?;
                let inputs = split_handle_inputs().map(|(name, _)| {
                    let binding = ident(name);
                    let field = ident(name.trim_start_matches("handle-"));
                    quote!(let #binding = __xr_handle.#field;)
                });
                out.extend(quote! {
                    self.__state.#handle.decorate(&mut #name, |__xr_handle| {
                        #(#inputs)*
                        #decoration
                        #template
                    });
                });
            }
        }
        if self.use_visual_plan {
            let plan = xrack_resources::ScenePlan::from_operation(element)?;
            if let Some(grid) = plan.grid {
                out.extend(scene_plan::grid_declaration(&name, grid));
            }
            if let Some(order) = plan.paint_order {
                if order.is_empty() {
                    out.extend(quote!(#name.set_natural_paint_order();));
                } else {
                    out.extend(quote!(#name.set_static_paint_order(&[#(#order),*]);));
                }
            }
        }
        Ok(name)
    }

    fn bind_button_groups(
        &self,
        groups: &GroupMembers<'a>,
        node: &Ident,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        for (&key, members) in groups {
            let group = *self.elements.get(key).ok_or("unknown ButtonGroup")?;
            let name = self.binding_value_or(
                group,
                "accessible-name",
                quote!(::std::string::String::new()),
            )?;
            let enabled = self.binding_value_or(group, "enabled", quote!(true))?;
            let paths = members.iter().map(|(path, _)| quote!(&[#(#path),*]));
            if non_exclusive_button_group(group) {
                out.extend(quote!(::xrack_runtime::bind_check_group(&mut #node, #key, #name, &[#(#paths),*], #enabled);));
            } else {
                let (property, ty) =
                    controlled_property(group).ok_or("missing ButtonGroup selection")?;
                let current = self.control_value(group, property, ty)?;
                let mode = if property == "current-value" {
                    quote!(Value)
                } else {
                    quote!(Index)
                };
                out.extend(quote! {
                    ::xrack_runtime::bind_button_group(
                        &mut #node, #key, #name, &[#(#paths),*],
                        ::xrack_runtime::ButtonGroupSelection::#mode(#current), #enabled,
                    );
                });
            }
        }
        Ok(())
    }

    fn declare_check_groups(
        &self,
        groups: &GroupMembers<'a>,
        fields: &mut TokenStream,
        defaults: &mut TokenStream,
        methods: &mut TokenStream,
    ) -> Result<(), String> {
        for (&key, _) in self
            .elements
            .iter()
            .filter(|(_, group)| non_exclusive_button_group(**group))
        {
            let mut values = Vec::new();
            let mut writes = TokenStream::new();
            for (_, member) in groups.get(key).into_iter().flatten() {
                let (property, ty) =
                    controlled_property(*member).ok_or("missing check group member property")?;
                let checked = ty == Type::Bool;
                let current = self.control_value(*member, property, ty)?;
                let read = if checked {
                    quote!(::xrack_runtime::CheckState::from(#current))
                } else {
                    current
                };
                let value = if checked {
                    quote!(state == ::xrack_runtime::CheckState::Checked)
                } else {
                    quote!(state)
                };
                values.push(read);
                writes.extend(self.set_control_property(*member, property, &value)?);
            }
            let read = check_group_method(key, "state");
            let set = check_group_method(key, "set");
            fields.extend(quote!(#read: ::xrack_runtime::Property<::xrack_runtime::CheckState>,));
            defaults.extend(quote!(#read: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #read(&self) -> ::xrack_runtime::CheckState {
                    self.__state.#read.get(|| ::xrack_runtime::summarize_checks(&[#(&|| #values),*]))
                }

                fn #set(&self, state: ::xrack_runtime::CheckState) {
                    if state == ::xrack_runtime::CheckState::PartiallyChecked { return; }
                    #writes
                }
            });
        }
        Ok(())
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
        let kind = element.attr("kind")?;
        let dispatchable = |handler: OperationRef<'_>| {
            handler
                .attr("name")
                .is_ok_and(|event| xrack_runtime::supports_untyped_dispatch(kind, event))
        };
        if matches!(
            element.attr("kind"),
            Ok("Button" | "ToolButton" | "RoundButton")
        ) && let Some(action) = action_ref(element)
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
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__edit_path(rest, node, text),),
            );
            return Ok(());
        }
        if combo_editor(element) {
            let set = self.set_control_property(element, "edit-text", &quote!(text.clone()))?;
            let edited = self.typed_event(element, "edited", &quote!(text))?;
            out.extend(quote!([#(#path),*] => { #set #edited true },));
        } else if element.attr("kind") == Ok("ColorPickerPopup") {
            let popup = picker_field(element_key(element)?);
            let open = self.control_value(element, "open", Type::Bool)?;
            let show_alpha = self.binding_value_or(element, "show-alpha", quote!(false))?;
            out.extend(quote! {
                [#(#path,)* 0, 0, 1] => (#open) && self.__state.#popup.edit_hex(#show_alpha, node, text),
            });
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

    fn focus_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__focus_path(rest, event),),
            );
            return Ok(());
        }
        if element.attr("kind")? == "FocusScope"
            && children(element, "xr.handler")
                .any(|handler| handler.attribute("name").is_some_and(focus_handler))
        {
            let reason = &[quote!(__reason)];
            let gained = self.typed_event_values(element, "focus-gained", reason)?;
            let lost = self.typed_event_values(element, "focus-lost", reason)?;
            let changed = self.typed_event_values(element, "focus-changed-event", reason)?;
            let within = self.typed_event_values(
                element,
                "focus-within-changed",
                &[quote!(__focused), quote!(__reason)],
            )?;
            let handles_gain = !gained.is_empty() || !changed.is_empty();
            let handles_loss = !lost.is_empty() || !changed.is_empty();
            let handles_within = !within.is_empty();
            out.extend(quote! {
                [#(#path),*] => match event {
                    ::xrack_runtime::FocusEvent::Gained(__reason) => { #gained #changed #handles_gain }
                    ::xrack_runtime::FocusEvent::Lost(__reason) => { #lost #changed #handles_loss }
                    ::xrack_runtime::FocusEvent::Within { focused: __focused, reason: __reason } => { #within #handles_within }
                },
            });
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.focus_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn key_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            out.extend(quote!([#(#path,)* rest @ ..] => self.__state.#instance.__key_path(rest, phase, event),));
            return Ok(());
        }
        if element.attr("kind")? == "FocusScope" {
            let mut phases = TokenStream::new();
            for handler in children(element, "xr.handler") {
                let Some(phase) = key_phase(handler.attr("name")?) else {
                    continue;
                };
                let params: Vec<Parameter> = serde_json::from_str(handler.attr("parameters")?)
                    .map_err(|error| error.to_string())?;
                let [parameter] = params.as_slice() else {
                    return Err("key handler requires an event parameter".into());
                };
                let name = ident(&parameter.name);
                let body = self.statements(handler)?;
                let result = children(handler, "xr.return")
                    .next()
                    .ok_or("key handler requires a result")?;
                let result = self.expr(self.operand(result, 0)?)?;
                phases.extend(quote! {
                    ::xrack_runtime::KeyPhase::#phase => {
                        let #name = event.clone();
                        #body
                        #result
                    },
                });
            }
            if !phases.is_empty() {
                out.extend(quote! {
                    [#(#path),*] => match phase { #phases _ => ::xrack_runtime::EventResult::Reject },
                });
            }
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.key_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn pointer_input_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        wheel: &mut TokenStream,
        pointer: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let instance = instance_field(element_key(element)?);
            wheel.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__wheel_path(rest, event),),
            );
            pointer.extend(
                quote!([#(#path,)* rest @ ..] => self.__state.#instance.__pointer_path(rest, event),),
            );
            return Ok(());
        }
        let kind = element.attr("kind")?;
        for handler in children(element, "xr.handler") {
            let returns_result = match (kind, handler.attr("name")?) {
                ("WheelHandler", "wheel") | ("TouchArea", "scroll-event") => true,
                ("TouchArea", "pointer-event") => false,
                _ => continue,
            };
            let params: Vec<Parameter> = serde_json::from_str(handler.attr("parameters")?)
                .map_err(|error| error.to_string())?;
            let [parameter] = params.as_slice() else {
                return Err("pointer input handler requires an event parameter".into());
            };
            let name = ident(&parameter.name);
            let body = self.statements(handler)?;
            if returns_result {
                let result = children(handler, "xr.return")
                    .next()
                    .ok_or("wheel handler requires a result")?;
                let result = self.expr(self.operand(result, 0)?)?;
                wheel.extend(quote! { [#(#path),*] => { let #name = *event; #body #result }, });
            } else {
                pointer.extend(quote! { [#(#path),*] => { let #name = *event; #body true }, });
            }
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.pointer_input_arms(child, path, wheel, pointer)?;
            path.pop();
        }
        Ok(())
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
            let current = self.control_value(element, "value", Type::Float)?;
            let set = self.set_control(element, &quote!(value))?;
            let changed = self.typed_event(element, "changed", &quote!(value))?;
            let released = self.typed_event(element, "released", &quote!(value))?;
            out.extend(quote! {
                [#(#path),*] => {
                    let range = ::xrack_runtime::SliderRange::from_node(node).with_value(#current);
                    if let Some(value) = range.apply(action) {
                        #set
                        #changed
                    } else if !action.is_release() {
                        return false;
                    }
                    if action.is_release() {
                        let value = range.with_value(#current).value();
                        #released
                    }
                    true
                },
            });
        } else if element.attr("kind") == Ok("ScrollBar")
            && children(element, "xr.scroll_target").next().is_none()
        {
            let set = self.set_control(element, &quote!(value))?;
            let changed = self.typed_event(element, "changed", &quote!(value))?;
            out.extend(quote! {
                [#(#path),*] => {
                    if !node.boolean("interactive", true) { return false; }
                    let range = ::xrack_runtime::ScrollBarRange::from_node(node);
                    let Some(value) = range.apply(action) else { return false; };
                    #set
                    #changed
                    true
                },
            });
        } else if element.attr("kind") == Ok("ColorPickerPopup") {
            let popup = picker_field(element_key(element)?);
            let open = self.control_value(element, "open", Type::Bool)?;
            let show_alpha = self.binding_value_or(element, "show-alpha", quote!(false))?;
            let show_plane = self.binding_value_or(element, "show-color-plane", quote!(false))?;
            out.extend(quote! {
                [#(#path,)* 0, 5, ..] => (#open) && (#show_plane) && self.__state.#popup.adjust_plane(node, action),
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
                    if let Some([next_lower, next_upper]) = interval.apply(*handle, action) {
                        if next_lower != raw_lower { #set_lower }
                        if next_upper != raw_upper { #set_upper }
                        let [__xr_lower, __xr_upper] =
                            ::xrack_runtime::SliderInterval::from_values(node, #lower, #upper).values();
                        #changed
                    } else if !action.is_release() {
                        return false;
                    }
                    if action.is_release() {
                        let [__xr_lower, __xr_upper] =
                            ::xrack_runtime::SliderInterval::from_values(node, #lower, #upper).values();
                        #released
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
            if !::xrack_runtime::choice_enabled(node, __xr_index) { return false; }
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
                            .filter(|index| ::xrack_runtime::choice_enabled(node, *index) && ::xrack_runtime::combo_text(node) == __xr_draft)
                            .or_else(|| ::xrack_runtime::combo_find_enabled_label(node, 0, |label| label == __xr_draft));
                        if let Some(__xr_index) = matching {
                            #select_value
                            #reset
                            #value_selected
                        } else {
                            if ::xrack_runtime::combo_find_label(node, |label| label == __xr_draft).is_some() { return false; }
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
        let model = self.visual_value(binding, &self.visuals)?;
        Ok(quote! {
            if !::xrack_runtime::combo_model_is_current(node, &#model) { return false; }
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
            "StyledText"
                if children(element, "xr.handler")
                    .any(|handler| handler.attr("name") == Ok("link-activated")) =>
            {
                let activated = self.typed_event_values(
                    element,
                    "link-activated",
                    &[quote!(link.href.to_string())],
                )?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::ActivateTextLink(index) = action else { return false; };
                        let Some(link) = node.styled_text("text").and_then(|text| text.links().get(index)) else {
                            return false;
                        };
                        #activated
                        true
                    }
                });
            }

            kind @ ("MenuItem" | "Button" | "ToolButton" | "RoundButton" | "Shortcut")
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
                        if !node.accepts_swipe_side(current, __xr_side) {
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
                let key = element_key(element)?;
                let tree = tree_snapshot_method(key);
                let state = tree_state_field(key);
                let select = tree_selection_method(key);
                let current = self.control_value(element, "current-key", Type::Int)?;
                let key = quote!(key);
                let set = self.set_control(element, &key)?;
                let changed = self.typed_event(element, "changed", &key)?;
                let expanded_event = self.typed_event(element, "expanded", &key)?;
                let collapsed_event = self.typed_event(element, "collapsed", &key)?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let Some(snapshot) = &node.tree else { return false; };
                        let live = self.#tree();
                        if let ::xrack_runtime::ControlAction::Tree(::xrack_runtime::TreeAction::SetExpanded { key, row, expanded }) = action {
                            let current = #current;
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
                        if multiple && !snapshot.same_model(&live) { return false; }
                        self.#select(&live, multiple, action)
                    },
                });
            }
            kind @ ("ListView" | "GridView" | "StandardListView" | "TableView"
            | "StandardTableView") => {
                let table = matches!(kind, "TableView" | "StandardTableView");
                let value = quote!(index as ::core::primitive::i64);
                let selection_guard = if table {
                    quote! {
                        if !node.list.is_some_and(|list| index < list.row_count)
                            || ::xrack_runtime::ListMetrics::selection(node) == Some(#value) { return false; }
                    }
                } else {
                    let stamp = if has_list_visibility(element) {
                        let model = visible_list_model(element_key(element)?);
                        quote!(Some(self.#model().positions().stamp()))
                    } else if kind == "StandardListView" {
                        if let Some(binding) = model_binding(element) {
                            let model = self.visual_value(binding, &self.visuals)?;
                            quote!(Some(::xrack_runtime::ListModel::structure_stamp(&#model)))
                        } else {
                            quote!(None)
                        }
                    } else {
                        let (row, repeat) = self.list_template(element)?;
                        let field = repeat_field(self.visuals.repeats[element_key(row)?].index());
                        let model = self.expr(self.operand(repeat, 0)?)?;
                        quote!(Some(self.__state.__visuals.#field.with_model(
                            || #model,
                            ::xrack_runtime::ListModel::structure_stamp,
                        )))
                    };
                    let current = self.control_value(element, "current-index", Type::Int)?;
                    quote! {
                        if !::xrack_runtime::ListMetrics::has_row_in_model(node, index, #stamp)
                            || #current == #value { return false; }
                    }
                };
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
                    let sort_table = table_sort_method(element_key(element)?);
                    let widths = table_widths_field(element_key(element)?);
                    let order = table_order_field(element_key(element)?);
                    let move_column = table_move_method(element_key(element)?);
                    let move_row = table_move_row_method(element_key(element)?);
                    let heights = table_row_heights_field(element_key(element)?);
                    let model = self.table_model(element)?;
                    let select_table = table_selection_method(element_key(element)?);
                    quote! {
                        let model = #model;
                        if node.standard_list_stamp != Some(model.positions().stamp()) { return false; }
                        if let ::xrack_runtime::ControlAction::ResizeRow { row, key, height } = action {
                            return node.boolean("resizable-rows", false)
                                && self.__state.#heights.set(model, row, Some(key), ::core::primitive::f64::from(height));
                        }
                        if let ::xrack_runtime::ControlAction::MoveColumn { from, to } = action {
                            return node.boolean("movable-columns", false)
                                && node.table_column_order == self.__state.#order.get(::core::default::Default::default)
                                && self.#move_column(from as ::core::primitive::i64, to as ::core::primitive::i64);
                        }
                        if let ::xrack_runtime::ControlAction::MoveRow { from, to, row, stamp } = action {
                            return node.boolean("movable-rows", false)
                                && model.positions().stamp() == stamp
                                && from < ::xrack_runtime::ListModel::row_count(model)
                                && ::xrack_runtime::ListModel::row_key(model, from) == row
                                && self.#move_row(from as ::core::primitive::i64, to as ::core::primitive::i64);
                        }
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
                            let Some(header) = node.table_header.as_ref() else { return false; };
                            if header.sort != self.__state.#field.get(|| None) { return false; }
                            let Some(sort) = header.next_sort(index) else { return false; };
                            return self.#sort_table(Some(sort));
                        }
                        if node.table_selection.is_some() {
                            let order = self.__state.#order.get(::core::default::Default::default);
                            let Some(selection) = ::xrack_runtime::TableSelectionAction::from_control(node, &order, action) else {
                                return false;
                            };
                            return self.#select_table(selection);
                        }
                    }
                } else {
                    TokenStream::new()
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        #header_actions
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        #selection_guard
                        #set
                        #event
                        true
                    },
                });
            }
            "RadioButton" => {
                let Some(key) = radio_group_ref(element) else {
                    return Ok(());
                };
                let group = *self.elements.get(key).ok_or("unknown ButtonGroup")?;
                let (property, ty) =
                    controlled_property(group).ok_or("missing ButtonGroup selection")?;
                let current = self.control_value(group, property, ty)?;
                let enabled = self.binding_value_or(group, "enabled", quote!(true))?;
                let next = if property == "current-value" {
                    quote!(__xr_value.clone())
                } else {
                    quote!(__xr_member.index as ::core::primitive::i64)
                };
                let set = self.set_control(group, &quote!(__xr_next))?;
                let selected = self.typed_event(group, "selected", &quote!(__xr_value))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        if action != ::xrack_runtime::ControlAction::ActivateRadioOption || !(#enabled) { return false; }
                        let Some(__xr_member) = node.button_group.as_ref().filter(|member| member.group.key == #key) else { return false; };
                        let __xr_value = ::xrack_runtime::radio_value(node).to_owned();
                        let __xr_next = #next;
                        if (#current) == __xr_next { return false; }
                        #set
                        #selected
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
                let label = if radio {
                    quote!(::xrack_runtime::radio_value(__xr_item).to_owned())
                } else {
                    quote!(__xr_item.text("text").unwrap_or("").to_owned())
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let Some(__xr_item) = ::xrack_runtime::group_item(node, index) else { return false; };
                        if ::xrack_runtime::group_index(node) == Some(index) { return false; }
                        let __xr_label = #label;
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
                                return self.__edit_path(path, node, ::std::string::String::new()),
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
            kind @ ("Calendar" | "MonthGrid") => {
                let set_date = self.set_control_property(element, "date", &quote!(__xr_date))?;
                let set_month =
                    self.set_control_property(element, "display-month", &quote!(__xr_month))?;
                let date_changed = self.typed_event(element, "changed", &quote!(__xr_date))?;
                let month_changed =
                    self.typed_event(element, "month-changed", &quote!(__xr_month))?;
                let (source_guard, week_start, activated) = if kind == "MonthGrid" {
                    (
                        quote! {
                            let live_scene = ::xrack_runtime::Component::render_shared(self);
                            let Some(live) = live_scene.interactive_at_path(path) else { return false; };
                            if ::xrack_runtime::CalendarState::from_node(live) != calendar
                                || live.int("week-start") != node.int("week-start")
                            { return false; }
                        },
                        quote!(Some(node.int("week-start").unwrap_or(1))),
                        self.typed_event(element, "activated", &quote!(__xr_activated))?,
                    )
                } else {
                    (TokenStream::new(), quote!(None), TokenStream::new())
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        use ::xrack_runtime::CalendarAction;
                        let ::xrack_runtime::ControlAction::Calendar(action) = action else { return false; };
                        let calendar = ::xrack_runtime::CalendarState::from_node(node);
                        #source_guard
                        let Some((__xr_date, __xr_month)) = calendar.apply(action, #week_start) else {
                            return false;
                        };
                        let date_changed = __xr_date.is_some_and(|date| node.date("date") != Some(date));
                        let month_changed = __xr_month != calendar.month;
                        if let Some(__xr_date) = __xr_date.filter(|_| date_changed) { #set_date }
                        if month_changed { #set_month }
                        if let Some(__xr_date) = __xr_date.filter(|_| date_changed) { #date_changed }
                        if month_changed { #month_changed }
                        if let CalendarAction::Activate(__xr_activated) = action {
                            #activated
                            return true;
                        }
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
            kind @ ("TimePickerPopup" | "DatePickerPopup" | "ColorPickerPopup"
            | "FontPickerPopup") => {
                let popup = picker_field(element_key(element)?);
                let open = self.control_value(element, "open", Type::Bool)?;
                let close = self.set_control_property(element, "open", &quote!(false))?;
                let (property, _) = picker_value(kind).ok_or("missing picker value")?;
                let assignment = if kind == "FontPickerPopup" {
                    quote!(__xr_value.clone())
                } else {
                    quote!(__xr_value)
                };
                let set = self.set_control_property(element, property, &assignment)?;
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
                    "FontPickerPopup" => {
                        out.extend(quote! {
                            [#(#path,)* 0, ..] => (#open) && self.__state.#popup.edit(node, action),
                        });
                        quote!(self.__state.#popup.finish(node))
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
                        quote!(self.__state.#popup.finish_editor(#show_alpha, node, role))
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
        history_table: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let mut inputs = BTreeMap::new();
        let mut links = BTreeSet::new();
        let component = instance.attr("component")?;
        let (context, parameters) = if let Some(repeat) = repeat {
            let repeat = RepeatRef::from_operation(repeat)?;
            let item = ident(repeat.item);
            let mut parameters = quote!(let #item = || __context.item(););
            if let Some(index) = repeat.index {
                let index = ident(index);
                parameters.extend(quote!(let #index = || __context.index();));
            }
            (quote!(let __context = __context.clone();), parameters)
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        for binding in children(instance, "xr.bind") {
            let name = binding.attr("name")?;
            if name == "clipboard-text"
                && self.elements.values().any(|element| {
                    element.attr("kind") == Ok("TableRow")
                        && children(*element, "xr.instance")
                            .any(|child| child.attribute("key") == instance.attribute("key"))
                })
            {
                continue;
            }
            let endpoint = canonical_input(self.alias_graphs, component, name)?;
            if inputs.contains_key(&endpoint) {
                return Err(format!(
                    "instance binds multiple inputs to the same property at {name}"
                ));
            }
            let expression = self.expr_in(
                self.operand(binding, 0)?,
                ExpressionContext {
                    receiver: &quote!(__ui),
                    repeat_parameters: repeat.is_some(),
                    hoist_repeat_subexpressions: false,
                },
            )?;
            inputs.insert(
                endpoint,
                (
                    name,
                    quote! {
                        ::core::option::Option::Some({
                            let __weak = __parent.clone();
                            #context
                            ::xrack_runtime::InputProperty::bound(move || {
                                let __ui = Self {
                                    __state: __weak.upgrade().expect("parent of a bound component"),
                                };
                                #parameters
                                #expression
                            })
                        })
                    },
                ),
            );
        }
        for binding in children(instance, "xr.instance_link") {
            let name = binding.attr("name")?;
            let endpoint = canonical_input(self.alias_graphs, component, name)?;
            if inputs.contains_key(&endpoint) {
                return Err(format!(
                    "instance binds multiple inputs to the same property at {name}"
                ));
            }
            let link =
                self.instance_link_input(self.operand(binding, 0)?, repeat, history_table)?;
            links.insert(endpoint.clone());
            inputs.insert(
                endpoint,
                (name, quote!(::core::option::Option::Some(#link))),
            );
        }
        if repeat.is_none() {
            let key = element_key(instance)?;
            for &(owner, name) in self.forwarded_inputs.keys() {
                if owner != key {
                    continue;
                }
                let forwarded = self
                    .forwarded_input((owner, name))
                    .expect("registered alias input");
                let endpoint = canonical_input(self.alias_graphs, component, name)?;
                let input = inputs.remove(&endpoint).map_or_else(
                    || forwarded.clone(),
                    |(_, existing)| quote!(#forwarded.or_else(|| #existing)),
                );
                inputs.insert(endpoint, (name, input));
            }
        }
        let receiver = quote!(__ui);
        let expression_context = ExpressionContext {
            receiver: &receiver,
            repeat_parameters: repeat.is_some(),
            hoist_repeat_subexpressions: false,
        };
        let mut state_inputs = BTreeMap::<_, Vec<_>>::new();
        for binding in children(instance, "xr.instance_state") {
            state_inputs
                .entry(binding.attr("name")?)
                .or_default()
                .push(binding);
        }
        let mut state_endpoints = BTreeSet::new();
        for (name, cases) in state_inputs {
            let endpoint = canonical_input(self.alias_graphs, component, name)?;
            if links.contains(&endpoint) {
                return Err(format!("state cannot override two-way input `{name}`"));
            }
            if !state_endpoints.insert(endpoint.clone()) {
                return Err(format!(
                    "states target multiple aliases of the same component input at `{name}`"
                ));
            }
            let selector = self.operand(cases[0], 0)?;
            let owner = selector.attr("owner")?;
            let selected = if self.persistent_state_selectors.contains_key(owner) {
                let method = state_selector_name(owner);
                quote!(__ui.#method())
            } else {
                self.state_choice(selector, |condition| {
                    self.expr_in(condition, expression_context)
                })?
            };
            let mut choices = TokenStream::new();
            for case in cases {
                let case = StateCaseRef::from_operation(case)?;
                let index = case.index;
                let value = case
                    .value
                    .defining_operation()
                    .ok_or("state value has no definition")?;
                let value = self.expr_in(value, expression_context)?;
                choices.extend(quote!(#index => ::core::option::Option::Some(#value),));
            }
            let base = inputs.remove(&endpoint).map_or_else(
                || quote!(::core::default::Default::default()),
                |(_, base)| quote!((#base).unwrap_or_default()),
            );
            inputs.insert(
                endpoint,
                (
                    name,
                    quote! {
                        ::core::option::Option::Some({
                            let __base: ::xrack_runtime::InputProperty<_> = #base;
                            let __weak = __parent.clone();
                            #context
                            __base.with_state(move || {
                                let __ui = Self { __state: __weak.upgrade()? };
                                #parameters
                                match #selected { #choices _ => ::core::option::Option::None }
                            })
                        })
                    },
                ),
            );
        }
        let mut inputs: TokenStream = inputs
            .into_values()
            .map(|(name, value)| {
                let field = property_field("root", name);
                quote!(#field: #value,)
            })
            .collect();
        inputs.extend(self.instance_callback_inputs(instance, repeat)?);
        Ok(inputs)
    }

    fn callback_parameters(&self, op: OperationRef<'a>) -> Result<Vec<Parameter>, String> {
        serde_json::from_str(op.attr("parameters")?).map_err(|error| error.to_string())
    }

    fn callback_returns(&self, op: OperationRef<'a>) -> Result<TokenStream, String> {
        children(op, "xr.return")
            .next()
            .map(|result| {
                let ty = host_ty(&value_type(self.operand(result, 0)?)?);
                Ok(quote!(-> #ty))
            })
            .transpose()
            .map(|tokens| tokens.unwrap_or_default())
    }

    fn callback_fallback(&self, op: OperationRef<'a>) -> Result<TokenStream, String> {
        children(op, "xr.return")
            .next()
            .map(|result| {
                let value = self.operand(result, 0)?;
                Ok(to_host(self.expr(value)?, &value_type(value)?))
            })
            .transpose()
            .map(|tokens| tokens.unwrap_or_default())
    }

    fn callback_trait(op: OperationRef<'a>) -> TokenStream {
        if op.attribute("pure") == Some("true") {
            quote!(::core::ops::Fn)
        } else {
            quote!(::core::ops::FnMut)
        }
    }

    fn callback_handler_value(
        &self,
        owner: &str,
        op: OperationRef<'a>,
        repeat: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let name = op.attr("name")?;
        let method = callback_method("callback_body", owner, name);
        let params = self.callback_parameters(op)?;
        let args: Vec<_> = params.iter().map(|p| ident(&p.name)).collect();
        let types = params.iter().map(|p| host_ty(&p.ty));
        let returns = self.callback_returns(op)?;
        let (capture, context) = if repeat.is_some() {
            (
                quote!(let __context = __context.clone();),
                quote!(&__context,),
            )
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        Ok(quote!({
            let __weak = __parent.clone();
            #capture
            let __handler: ::std::rc::Rc<dyn ::core::ops::Fn(#(#types),*) #returns> =
                ::std::rc::Rc::new(move |#(#args),*| {
                    if let ::core::option::Option::Some(__state) = __weak.upgrade() {
                        Self { __state }.#method(#context #(#args),*)
                    } else {
                        ::core::default::Default::default()
                    }
                });
            ::core::option::Option::Some(__handler)
        }))
    }

    fn callback_body(
        &self,
        owner: &str,
        op: OperationRef<'a>,
        repeat: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let method = callback_method("callback_body", owner, op.attr("name")?);
        let params = self.callback_parameters(op)?;
        let args = params.iter().map(|p| ident(&p.name));
        let types = params.iter().map(|p| host_ty(&p.ty));
        let returns = self.callback_returns(op)?;
        let body = self.statements(op)?;
        let fallback = self.callback_fallback(op)?;
        let mut bindings = TokenStream::new();
        let mut context = TokenStream::new();
        if let Some(repeat) = repeat {
            let model = value_type(self.operand(repeat, 0)?)?;
            let item_ty = ty(&model.list_item().unwrap_or(Type::Int));
            context.extend(quote!(, __context: &::xrack_runtime::RowContext<#item_ty>));
            for (name, access) in [("item", quote!(item)), ("index", quote!(index))] {
                let binding = repeat.attr(name)?;
                if !binding.is_empty()
                    && !params
                        .iter()
                        .any(|p| rust_name(&p.name) == rust_name(binding))
                {
                    let binding = ident(binding);
                    bindings.extend(quote!(let #binding = __context.#access();));
                }
            }
        }
        Ok(quote! {
            fn #method(&self #context #(, #args: #types)*) #returns {
                #bindings
                #body
                #fallback
            }
        })
    }

    fn forwarded_callback(
        &self,
        endpoint: PropertyEndpoint<'a>,
    ) -> Result<Option<TokenStream>, String> {
        let Some(aliases) = self.callback_forwards.get(&endpoint) else {
            return Ok(None);
        };
        let mut inputs = Vec::new();
        let mut default = None;
        for &(owner, callback) in aliases {
            if owner == "root" {
                let field = callback_input_field(callback.attr("name")?);
                inputs.push(quote!(__inputs.#field.take()));
            }
            if callback.attribute("implementation") == Some("true") {
                if default.is_some() {
                    return Err("multiple alias handlers target the same callback".into());
                }
                default = Some(self.callback_handler_value(owner, callback, None)?);
            }
        }
        inputs.extend(default);
        let Some((first, rest)) = inputs.split_first() else {
            return Ok(None);
        };
        Ok(Some(quote!(#first #(.or_else(|| #rest))*)))
    }

    fn instance_callback_inputs(
        &self,
        instance: OperationRef<'a>,
        repeat: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let owner = element_key(instance)?;
        let component = instance.attr("component")?;
        let mut handlers = BTreeMap::new();
        for callback in children(instance, "xr.callback") {
            let name = callback.attr("name")?;
            let endpoint = canonical_input(self.callback_graphs, component, name)?;
            if handlers
                .insert(
                    endpoint,
                    (name, self.callback_handler_value(owner, callback, repeat)?),
                )
                .is_some()
            {
                return Err("multiple instance handlers target the same callback".into());
            }
        }
        if repeat.is_none() {
            for &(target, name) in self
                .callback_forwards
                .keys()
                .filter(|(target, _)| *target == owner)
            {
                let Some(input) = self.forwarded_callback((target, name))? else {
                    continue;
                };
                let endpoint = canonical_input(self.callback_graphs, component, name)?;
                let input = match handlers.remove(&endpoint) {
                    Some((_, previous)) => quote!(#input.or_else(|| #previous)),
                    None => input,
                };
                handlers.insert(endpoint, (name, input));
            }
        }
        Ok(handlers
            .into_values()
            .map(|(name, input)| {
                let field = callback_input_field(name);
                quote!(#field: #input,)
            })
            .collect())
    }

    fn instance_link_input(
        &self,
        target: OperationRef<'a>,
        repeat: Option<OperationRef<'a>>,
        history_table: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let receiver = quote!(__ui);
        let context = ExpressionContext::new(&receiver);
        let (capture, read, write) = if target.name() == "xr.read" {
            let read = self.expr_in(target, context)?;
            let write = self.set_property_in(
                &receiver,
                owner(target),
                target.attr("value")?,
                &quote!(__value),
            );
            (
                TokenStream::new(),
                quote!(::core::option::Option::Some(#read)),
                write,
            )
        } else {
            let repeat = repeat.ok_or("model row link requires a repeated component")?;
            let model = self.expr_in(self.operand(repeat, 0)?, context)?;
            let capture =
                quote!(let __read_row = __context.clone(); let __write_row = __context.clone(););
            let mut field_path = self.record_path(target)?;
            let (project, update) = if !field_path.is_empty() {
                let field = field_access(field_path.iter().copied());
                (
                    quote!(|row| row #field.clone()),
                    quote!(|row| {
                        let mut row = row.clone();
                        row #field = __value;
                        row
                    }),
                )
            } else {
                (quote!(|row| row.clone()), quote!(|_| __value))
            };
            let write = if let Some(table) = history_table {
                let history = table_history_field(element_key(table)?);
                let enabled = table_history_method(element_key(table)?, "enabled");
                let mut fields = BTreeMap::new();
                self.record_paste_leaves(&mut field_path, &value_type(target)?, &mut fields);
                let patch = Self::table_edit_capture(fields);
                quote!(__write_row.update_in_with_history(
                    &#model, &__ui.__state.#history, __ui.#enabled(), #update, #patch,
                );)
            } else {
                quote!(__write_row.update_in(&#model, #update);)
            };
            (
                capture,
                quote!(__read_row.read_from(&#model, #project)),
                write,
            )
        };
        Ok(quote!({
            let __read_parent = __parent.clone();
            let __write_parent = __parent.clone();
            #capture
            ::xrack_runtime::InputProperty::linked(
                move || {
                    let __ui = Self { __state: __read_parent.upgrade()? };
                    #read
                },
                move |__value| {
                    if let ::core::option::Option::Some(__state) = __write_parent.upgrade() {
                        let __ui = Self { __state };
                        #write
                    }
                },
            )
        }))
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
        let mut viewport_initializers = TokenStream::new();
        for (&owner, &selector) in &self.persistent_state_selectors {
            let field = state_selector_name(owner);
            let value = self.state_choice(selector, |condition| self.expr(condition))?;
            fields.extend(quote!(#field: ::xrack_runtime::Property<::core::primitive::i64>,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #field(&self) -> ::core::primitive::i64 {
                    self.__state.#field.get(|| #value)
                }
            });
        }
        for &view in &self.row_height_views {
            let field = row_height_projection_field(view);
            fields.extend(quote!(#field: ::xrack_runtime::RowHeightProjection,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }

        for &target in &self.focus_targets {
            let handle = focus_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::FocusHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
        }
        for (&key, &element) in &self.elements {
            if element.attr("kind") == Ok("ScaleRotateGestureHandler") {
                let handle = scale_rotate_handle(key);
                fields.extend(quote!(#handle: ::xrack_runtime::ScaleRotateHandle,));
                defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            }
            if element.attr("kind") == Ok("SwipeGestureHandler") {
                let handle = swipe_handle(key);
                fields.extend(quote!(#handle: ::xrack_runtime::SwipeHandle,));
                defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            }
            if element.attr("kind") == Ok("HoverHandler") {
                let handle = hover_handle(key);
                fields.extend(quote!(#handle: ::xrack_runtime::HoverHandle,));
                defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            }
        }
        for &key in &self.pointer_targets {
            let handle = pointer_handle(key);
            fields.extend(quote!(#handle: ::xrack_runtime::PointerHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
        }
        for &target in &self.viewport_targets {
            let handle = viewport_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::ViewportHandle,));
            let notify = self.scroll_observers.contains(target);
            if self
                .elements
                .get(target)
                .is_some_and(|element| element.attr("kind") == Ok("Flickable"))
            {
                defaults.extend(
                    quote!(#handle: ::xrack_runtime::ViewportHandle::for_flickable(#notify),),
                );
            } else if notify {
                defaults.extend(
                    quote!(#handle: ::xrack_runtime::ViewportHandle::with_scroll_notifications(),),
                );
            } else {
                defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            }
        }
        for &target in &self.geometry_targets {
            let handle = geometry_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::GeometryHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
        }
        for &element in &self.bound_viewports {
            let owner = element_key(element)?;
            let handle = viewport_handle(owner);
            for property in viewport_bindings(element) {
                let field = control_field(owner, property);
                let read = format_ident!("{}_read", field);
                let write = format_ident!("{}_write", field);
                let axis = if property == "content-x" {
                    quote!(Horizontal)
                } else {
                    quote!(Vertical)
                };
                let value = self.control_value(element, property, Type::Length)?;
                let update = self.set_control_property(element, property, &quote!(value))?;
                methods.extend(quote! {
                    fn #read(&self) -> ::xrack_runtime::Length { #value }
                    fn #write(&self, value: ::xrack_runtime::Length) { #update }
                });
                viewport_initializers.extend(quote! {
                    {
                        let reader = __component.as_weak();
                        let writer = reader.clone();
                        __component.__state.#handle.bind_offset(
                            ::xrack_runtime::Orientation::#axis,
                            move || reader.upgrade().map(|ui| ui.#read()).unwrap_or_default(),
                            move |value| {
                                if let Some(ui) = writer.upgrade() { ui.#write(value); }
                            },
                        );
                    }
                });
            }
        }

        for (&key, &element) in &self.elements {
            if !matches!(element.attr("kind"), Ok("ScrollBar" | "ScrollIndicator")) {
                continue;
            }
            let size_getter = scroll_control_getter(key, "size");
            let position_getter = scroll_control_getter(key, "position");
            if let Some(target) = children(element, "xr.scroll_target").next() {
                let viewport = viewport_handle(target.attr("target")?);
                let orientation = self.binding_value_or(
                    element,
                    "orientation",
                    quote!(::xrack_runtime::Orientation::Vertical),
                )?;
                methods.extend(quote! {
                    fn #size_getter(&self) -> ::core::primitive::f64 {
                        self.__state.#viewport.scroll_size(#orientation)
                    }
                    fn #position_getter(&self) -> ::core::primitive::f64 {
                        self.__state.#viewport.scroll_position(#orientation)
                    }
                });
                continue;
            }
            let size = self.binding_value_or(
                element,
                "size",
                quote!(::xrack_runtime::ScrollBarRange::DEFAULT_SIZE),
            )?;
            let position = if element.attr("kind") == Ok("ScrollBar") {
                self.control_value(element, "position", Type::Float)?
            } else {
                self.binding_value_or(element, "position", quote!(0.0_f64))?
            };
            methods.extend(quote! {
                fn #size_getter(&self) -> ::core::primitive::f64 {
                    ::xrack_runtime::ScrollBarRange::new(0.0, #size, 0.0).size
                }
                fn #position_getter(&self) -> ::core::primitive::f64 {
                    ::xrack_runtime::ScrollBarRange::new(#position, self.#size_getter(), 0.0).position
                }
            });
        }
        let mut input_fields = TokenStream::new();
        if self.with_translations {
            fields.extend(quote!(__locale: ::xrack_runtime::LocaleBinding,));
            defaults.extend(quote!(__locale: __inputs.__locale.clone(),));
            input_fields.extend(quote!(__locale: ::xrack_runtime::LocaleBinding,));
            methods.extend(quote! {
                pub fn new_with_locale(context: ::xrack_runtime::LocaleContext) -> Self {
                    let component = Self::new();
                    ::xrack_runtime::Component::set_locale_context(&component, context);
                    component
                }
            });
        }
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
            let writable = owner == "root" && matches!(visibility, "in" | "in-out");
            let write_method = property_method("write", owner, property_name);
            let reset_method = property_method("reset", owner, property_name);
            let (read, write, clear) = if property.attribute("alias") == Some("true") {
                let (target, name) = self
                    .alias_graph
                    .resolve(self.alias_graphs, (owner, property_name))?;
                let (write, clear) = if self
                    .unlinked_controls
                    .iter()
                    .any(|&(key, prop, _)| (key, prop) == (target, name))
                {
                    let field = control_field(target, name);
                    (
                        quote!(self.__state.#field.set_if_changed(value);),
                        quote!(self.__state.#field.reset();),
                    )
                } else {
                    (
                        self.set_property(target, name, &quote!(value)),
                        self.property_call(
                            "reset",
                            &quote!(self),
                            target,
                            name,
                            &TokenStream::new(),
                        ),
                    )
                };
                (expression, write, clear)
            } else {
                let incoming = self.forwarded_input((owner, property_name));
                if writable || incoming.is_some() {
                    fields.extend(quote!(#field: ::xrack_runtime::InputProperty<#rust_type>,));
                    let incoming = match (writable, incoming) {
                        (true, Some(aliases)) => quote!(__inputs.#field.take().or(#aliases)),
                        (true, None) => quote!(__inputs.#field.take()),
                        (false, Some(aliases)) => aliases,
                        (false, None) => unreachable!(),
                    };
                    defaults.extend(quote!(#field: (#incoming).unwrap_or_default(),));
                } else {
                    fields.extend(quote!(#field: ::xrack_runtime::Property<#rust_type>,));
                    defaults.extend(quote!(#field: ::core::default::Default::default(),));
                }
                (
                    quote!(self.__state.#field.get(|| #expression)),
                    quote!(self.__state.#field.set_if_changed(value);),
                    quote!(self.__state.#field.reset();),
                )
            };
            methods.extend(quote! {
                #getter_visibility fn #getter(&self) -> #rust_type { #read }
                fn #write_method(&self, value: #rust_type) { #write }
                fn #reset_method(&self) { #clear }
            });
            if owner == "root" && visibility != "private" && value_type.list_item().is_some() {
                let snapshot = format_ident!("get_{}", rust_name(property_name));
                methods.extend(quote! {
                    pub fn #snapshot(&self) -> #host_type { self.#getter().snapshot() }
                });
            }
            if writable {
                let setter = format_ident!("set_{}", rust_name(property_name));
                let reset = format_ident!("reset_{}", rust_name(property_name));
                input_fields.extend(quote! {
                    #field: ::core::option::Option<::xrack_runtime::InputProperty<#rust_type>>,
                });
                let input = from_host(quote!(value), &value_type);
                methods.extend(quote! {
                    pub fn #setter(&mut self, value: #host_type) {
                        self.#write_method(#input);
                    }
                    pub fn #reset(&mut self) {
                        self.#reset_method();
                    }
                });
                if value_type.list_item().is_some() {
                    let bind_model = format_ident!("bind_model_{}", rust_name(property_name));
                    methods.extend(quote! {
                        pub fn #bind_model(&mut self, model: #rust_type) {
                            self.#write_method(model);
                        }
                    });
                }
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
            if let Some(input) = self.forwarded_input((key, property)) {
                fields.extend(quote!(#field: ::xrack_runtime::InputProperty<#rust_type>,));
                defaults.extend(quote!(#field: (#input).unwrap_or_default(),));
            } else {
                fields.extend(quote!(#field: ::xrack_runtime::Property<#rust_type>,));
                defaults.extend(quote!(#field: ::core::default::Default::default(),));
            }
        }
        for (&key, &group) in &self.elements {
            if day_cell_template(group).is_some() {
                let field = calendar_cells_field(key);
                fields.extend(quote!(#field: ::xrack_runtime::CalendarCells,));
                defaults.extend(quote!(#field: ::core::default::Default::default(),));
            }
            if group.attr("kind") != Ok("ActionGroup") {
                continue;
            }
            if !non_exclusive_group(group) {
                let current = action_group_method(key, "current");
                let value = self.control_value(group, "current-value", Type::String)?;
                methods.extend(quote! {
                    fn #current(&self) -> ::std::string::String { #value }
                });
            }
            let trigger = action_group_method(key, "activated");
            let activated = self.typed_event_values(group, "activated", &[quote!(__xr_value)])?;
            methods.extend(quote! {
                fn #trigger(&self, __xr_value: ::std::string::String) { #activated }
            });
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
            let member = children(action, "xr.action_group_ref").next();
            let group = member
                .map(|member| -> Result<_, String> {
                    let name = member.attr("group")?;
                    let group = *self.elements.get(name).ok_or("unknown ActionGroup")?;
                    Ok((name, group, member.attr("value")?))
                })
                .transpose()?;
            let exclusive = group.filter(|(_, group, _)| !non_exclusive_group(*group));
            let enabled = binding("enabled")?.unwrap_or_else(|| quote!(true));
            let enabled = if let Some((_, group, _)) = group {
                let group_enabled = self.binding_value_or(group, "enabled", quote!(true))?;
                quote!((#group_enabled) && (#enabled))
            } else {
                enabled
            };
            let default_checkable = exclusive.is_some();
            let checkable = binding("checkable")?.unwrap_or_else(|| quote!(#default_checkable));
            let (checked, toggle) = if let Some((name, group, value)) = exclusive {
                let current = action_group_method(name, "current");
                let select =
                    self.set_control(group, &quote!(::std::string::String::from(#value)))?;
                (quote!(__xr_checkable && self.#current() == #value), select)
            } else {
                if control_link(action, "checked").is_none() {
                    let field = control_field(key, "checked");
                    fields.extend(
                        quote!(#field: ::xrack_runtime::Property<::core::primitive::bool>,),
                    );
                    defaults.extend(quote!(#field: ::core::default::Default::default(),));
                }
                let checked = self.control_value(action, "checked", Type::Bool)?;
                let toggle = self.set_control(action, &quote!(!(#checked)))?;
                (checked, toggle)
            };
            let activated = self.typed_event_values(action, "activated", &[])?;
            let group_activated = group.map(|(name, _, value)| {
                let trigger = action_group_method(name, "activated");
                quote!(self.#trigger(::std::string::String::from(#value));)
            });
            let mut labels = TokenStream::new();
            for name in ["text", "shortcut", "icon"] {
                if let Some(value) = binding(name)? {
                    labels.extend(quote!(node.properties.insert(#name, ::xrack_runtime::Value::from(#value));));
                }
            }
            methods.extend(quote! {
                fn #render(&self, node: &mut ::xrack_runtime::Node) {
                    let __xr_checkable = #checkable;
                    self.__state.#identity.present(node, #enabled, __xr_checkable, #checked);
                    #labels
                }
                fn #activate(&self) -> bool {
                    if !(#enabled) { return false; }
                    if #checkable { #toggle }
                    #activated
                    #group_activated
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
                let selection = native_dialog_member(key, "selection");
                fields.extend(quote!(#selection: ::xrack_runtime::FileDialogSelection,));
                defaults.extend(quote!(#selection: ::core::default::Default::default(),));
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
                        ::xrack_runtime::NativeDialogResult::Files(paths) => {
                            self.__state.#selection.commit(&paths);
                            #accepted
                        }
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
                            if __xr_button.accepts_message() {
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
        for &image in &self.animated_images {
            let slot = playback_slot(element_key(image)?);
            let finished = format_ident!("{}_finished", slot);
            let handler = self.typed_event_values(image, "finished", &[])?;
            let handled = !handler.is_empty();
            fields.extend(quote!(#slot: ::xrack_runtime::ImagePlaybackSlot,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #finished(&self) -> bool {
                    #handler
                    #handled
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
        let mut selection_reads = TokenStream::new();
        let mut selection_writes = TokenStream::new();
        let mut sort_reads = TokenStream::new();
        let mut sort_writes = TokenStream::new();
        for &table in &self.tables {
            let key = element_key(table)?;
            let field = table_sort_field(key);
            let sort_table = table_sort_method(key);
            let column_value = quote!(__xr_sort.column as ::core::primitive::i64);
            let ascending = self.typed_event(table, "sort-ascending", &column_value)?;
            let descending = self.typed_event(table, "sort-descending", &column_value)?;
            let sortable = table_columns(table)
                .enumerate()
                .map(|(index, column)| {
                    let allowed = self.binding_value_or(column, "sortable", quote!(true))?;
                    Ok(quote!(#index => #allowed,))
                })
                .collect::<Result<TokenStream, String>>()?;
            let id = table.attr("id")?;
            let id = if id.is_empty() && key == "root" {
                "root"
            } else {
                id
            };
            if !id.is_empty() {
                sort_reads.extend(quote!(#id => self.__state.#field.get(|| None),));
                sort_writes.extend(quote!(#id => self.#sort_table(sort),));
            }

            let widths = table_widths_field(key);
            let selection = table_selection_field(key);
            let order = table_order_field(key);
            let move_column = table_move_method(key);
            let row_order = table_row_order_field(key);
            let move_row = table_move_row_method(key);
            let heights = table_row_heights_field(key);
            let set_height = table_row_height_method(key);
            let model = self.table_model(table)?;
            let source = self.view_source_model(table)?;
            let current_row = self.control_value(table, "current-row", Type::Int)?;
            let set_current_row = self.set_control(table, &quote!(__xr_next))?;
            let current_row_changed =
                self.typed_event(table, "current-row-changed", &quote!(__xr_next))?;
            let selection_snapshot = table_selection_snapshot_method(key);
            let select_table = table_selection_method(key);
            let target = self.binding_value_or(
                table,
                "selection-target",
                quote!(::xrack_runtime::SelectionTarget::Rows),
            )?;
            let multiple = self.binding_value_or(table, "multi-select", quote!(false))?;
            let selected = self.typed_event(
                table,
                "selection-changed",
                &quote!(count as ::core::primitive::i64),
            )?;
            let cell_changed = self.typed_event_values(
                table,
                "current-cell-changed",
                &[current_row.clone(), quote!(__xr_column)],
            )?;
            if !id.is_empty() {
                selection_reads.extend(quote!(#id => Some(self.#selection_snapshot()),));
                selection_writes.extend(quote!(#id => self.#select_table(action),));
            }
            let column_count = table_columns(table).count();
            methods.extend(quote! {
                fn #selection_snapshot(&self) -> ::xrack_runtime::TableSelectionSnapshot {
                    let model = #model;
                    let current = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(#current_row).ok();
                    self.__state.#selection.snapshot(model, current, #target, #column_count, #multiple)
                        .with_column_order(self.__state.#order.get(::core::default::Default::default))
                }
                fn #select_table(&self, action: ::xrack_runtime::TableSelectionAction) -> ::core::primitive::bool {
                    let model = #model;
                    let view = self.#selection_snapshot();
                    let current = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(#current_row).ok();
                    let Some(change) = self.__state.#selection.apply(model, &view, current, action) else { return false; };
                    if let Some(index) = change.current {
                        let __xr_next = index as ::core::primitive::i64;
                        #set_current_row
                        #current_row_changed
                    }
                    if change.current.is_some() || change.column_changed {
                        let __xr_column = self.#selection_snapshot().current_column().map_or(-1i64, |column| column as ::core::primitive::i64);
                        #cell_changed
                    }
                    if let Some(count) = change.selected_count { #selected }
                    change.current.is_some() || change.column_changed || change.selected_count.is_some()
                }
            });

            let auto_sort = table.attr("kind") == Ok("StandardTableView")
                && children(table, "xr.bind").any(|bind| bind.attr("name") == Ok("auto-sort"));
            let auto_sort_enabled = self.binding_value_or(table, "auto-sort", quote!(false))?;
            let filter_field = self.record_field_binding(table, "filter-field")?;
            let visible_field = self.record_field_binding(table, "row-visible-field")?;
            let projected = auto_sort || filter_field.is_some() || visible_field.is_some();
            let standard = table.attr("kind") == Ok("StandardTableView");
            if standard || visible_field.is_some() {
                let binding = if standard {
                    model_binding(table).ok_or("standard table requires a model")?
                } else {
                    self.list_template(table)?.1
                };
                let Some(Type::RecordList(record)) = operand_type(binding) else {
                    return Err("table row projection requires a record list".into());
                };
                let row_type = ident(&record);
                let rows = table_rows_model_method(key);
                let body = if projected {
                    let compare = if auto_sort {
                        table_columns(table)
                            .enumerate()
                            .map(|(index, column)| {
                                let name = self
                                    .record_field_binding(column, "field")?
                                    .ok_or("standard table column requires a field")?;
                                let field = field_access(name.split('.'));
                                let ty = self.record_field_type(record.clone(), &name)?;
                                let compare = if ty == Type::Float {
                                    quote!((left #field).total_cmp(&(right #field)))
                                } else {
                                    quote!((left #field).cmp(&(right #field)))
                                };
                                Ok(quote!(#index => #compare,))
                            })
                            .collect::<Result<TokenStream, String>>()?
                    } else {
                        TokenStream::new()
                    };
                    let sort = quote!(|| if #auto_sort_enabled { self.__state.#field.get(|| None) } else { None });
                    let compare = quote!(|column, left, right| match column { #compare _ => ::core::cmp::Ordering::Equal });
                    if let Some(visible) = visible_field {
                        let visible = field_access(visible.split('.'));
                        let filter = filter_field
                            .as_ref()
                            .map(|filter| {
                                let filter = field_access(filter.split('.'));
                                quote!(Some(|item: &#row_type| (item #filter).as_str()))
                            })
                            .unwrap_or_else(|| quote!(None));
                        let query = self.binding_value_or(
                            table,
                            "filter-text",
                            quote!(::std::string::String::new()),
                        )?;
                        quote! {
                            self.__state.#row_order.projected_rows_with_visibility(
                                || #source, #sort, #compare, #filter, || #query, Some(|item: &#row_type| item #visible),
                            )
                        }
                    } else if let Some(filter) = filter_field {
                        let filter = field_access(filter.split('.'));
                        let query = self.binding_value_or(
                            table,
                            "filter-text",
                            quote!(::std::string::String::new()),
                        )?;
                        quote! {
                            self.__state.#row_order.projected_rows(
                                || #source, #sort, #compare, Some(|item| (item #filter).as_str()), || #query,
                            )
                        }
                    } else {
                        quote!(self.__state.#row_order.sorted_rows(|| #source, #sort, #compare))
                    }
                } else {
                    quote!(self.__state.#row_order.rows(#source))
                };
                methods.extend(quote! {
                    fn #rows(&self) -> ::xrack_runtime::TableRows<::xrack_runtime::VecModel<#row_type>> {
                        #body
                    }
                });
            }
            let (capture_current, restore_current, move_guard) = if auto_sort {
                (
                    quote! {
                        let __xr_cursor = if #auto_sort_enabled {
                            let model = #model;
                            let index = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(#current_row).ok();
                            index.filter(|index| *index < ::xrack_runtime::ListModel::row_count(model))
                                .map(|index| ::xrack_runtime::ListModel::row_key(model, index))
                        } else { None };
                    },
                    quote! {
                        if let Some(key) = __xr_cursor
                            && let Some(index) = ::xrack_runtime::ListModel::index_of_key(#model, key)
                            && index as ::core::primitive::i64 != #current_row
                        {
                            let __xr_next = index as ::core::primitive::i64;
                            #set_current_row
                            #current_row_changed
                        }
                    },
                    quote! {
                        if #auto_sort_enabled && self.__state.#field.get(|| None).is_some() { return false; }
                    },
                )
            } else {
                (TokenStream::new(), TokenStream::new(), TokenStream::new())
            };
            let row_positions = if projected {
                quote!((#model).positions().clone())
            } else {
                quote!(self.__state.#row_order.positions(&*__xr_source))
            };

            let row_moved = self.typed_event_values(
                table,
                "row-moved",
                &[
                    quote!(__xr_row as ::core::primitive::i64),
                    quote!(from),
                    quote!(to),
                ],
            )?;
            let count = table_columns(table).count();
            let moved = self.typed_event_values(
                table,
                "column-moved",
                &[
                    quote!(__xr_column as ::core::primitive::i64),
                    quote!(__xr_visual as ::core::primitive::i64),
                ],
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::Property<::core::option::Option<::xrack_runtime::TableSort>>,));
            fields.extend(quote!(#widths: ::xrack_runtime::Property<::std::vec::Vec<::core::option::Option<::core::primitive::u32>>>,));
            fields.extend(quote!(#selection: ::xrack_runtime::TableSelection,));
            fields.extend(quote!(#heights: ::xrack_runtime::RowHeightOverrides,));
            fields.extend(quote!(#row_order: ::xrack_runtime::TableRowOrder,));
            fields.extend(
                quote!(#order: ::xrack_runtime::Property<::xrack_runtime::TableColumnOrder>,),
            );
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            defaults.extend(quote!(#widths: ::core::default::Default::default(),));
            defaults.extend(quote!(#selection: ::core::default::Default::default(),));
            defaults.extend(quote!(#order: ::core::default::Default::default(),));
            defaults.extend(quote!(#heights: ::core::default::Default::default(),));
            defaults.extend(quote!(#row_order: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #sort_table(&self, sort: ::core::option::Option<::xrack_runtime::TableSort>) -> ::core::primitive::bool {
                    if self.__state.#field.get(|| None) == sort { return false; }
                    if let Some(candidate) = sort {
                        let allowed = match candidate.column { #sortable _ => false };
                        if !allowed { return false; }
                    }
                    #capture_current
                    self.__state.#field.set(sort);
                    #restore_current
                    if let Some(__xr_sort) = sort {
                        if __xr_sort.descending { #descending } else { #ascending }
                    }
                    true
                }
                fn #set_height(&self, row: ::core::primitive::i64, height: ::xrack_runtime::Length) -> ::core::primitive::bool {
                    let Ok(index) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(row)
                    else { return false; };
                    self.__state.#heights.set(#model, index, None, height.0)
                }
                fn #move_column(&self, from: ::core::primitive::i64, to: ::core::primitive::i64) -> ::core::primitive::bool {
                    let (Ok(__xr_source), Ok(__xr_visual)) = (
                        <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(from),
                        <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(to),
                    ) else { return false; };
                    let __xr_current = self.__state.#order.get(::core::default::Default::default);
                    let Some((__xr_column, __xr_next)) = __xr_current.moved(#count, __xr_source, __xr_visual)
                    else { return false; };
                    self.__state.#order.set(__xr_next);
                    #moved
                    true
                }
                fn #move_row(&self, from: ::core::primitive::i64, to: ::core::primitive::i64) -> ::core::primitive::bool {
                    #move_guard
                    let (Ok(__xr_from), Ok(__xr_to)) = (
                        <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(from),
                        <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(to),
                    ) else { return false; };
                    let __xr_source = #source;
                    let __xr_before = #row_positions;
                    let __xr_current = #current_row;
                    let __xr_current_source = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(__xr_current)
                        .ok().and_then(|index| __xr_before.source_index(index));
                    let Some(__xr_row) = self.__state.#row_order.move_row(&*__xr_source, __xr_from, __xr_to)
                    else { return false; };
                    let __xr_after = #row_positions;
                    if let Some(__xr_next) = __xr_current_source.and_then(|index| __xr_after.visual_index(index))
                        .map(|index| index as ::core::primitive::i64).filter(|index| *index != __xr_current)
                    {
                        #set_current_row
                        #current_row_changed
                    }
                    #row_moved
                    true
                }
            });
        }
        if !selection_reads.is_empty() {
            methods.extend(quote! {
                pub fn table_selection(&self, id: &::core::primitive::str) -> ::core::option::Option<::xrack_runtime::TableSelectionSnapshot> {
                    match id { #selection_reads _ => None }
                }
                pub fn select_table(&mut self, id: &::core::primitive::str, action: ::xrack_runtime::TableSelectionAction) -> ::core::primitive::bool {
                    match id { #selection_writes _ => false }
                }
            });
        }
        if !sort_reads.is_empty() {
            methods.extend(quote! {
                pub fn table_sort(&self, id: &::core::primitive::str) -> ::core::option::Option<::xrack_runtime::TableSort> {
                    match id { #sort_reads _ => None }
                }
                pub fn set_table_sort(&mut self, id: &::core::primitive::str, sort: ::core::option::Option<::xrack_runtime::TableSort>) -> ::core::primitive::bool {
                    match id { #sort_writes _ => false }
                }
            });
        }
        for (key, ty) in &self.picker_popups {
            let field = picker_field(key);
            let state = match ty {
                Type::Date => quote!(::xrack_runtime::DatePopupState),
                Type::Time => quote!(::xrack_runtime::TimePopupState),
                Type::Color => quote!(::xrack_runtime::ColorPopupState),
                Type::Font => quote!(::xrack_runtime::FontPopupState),
                _ => return Err("unsupported picker state".into()),
            };
            fields.extend(quote!(#field: #state,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        let mut split_saves = TokenStream::new();
        let mut split_restores = TokenStream::new();
        for key in &self.splits {
            let field = split_state_field(key);
            let handle = split_resize_handle_field(key);
            let element = self.elements[key];
            let save = split_save_method(key);
            let restore = split_restore_method(key);
            let update = split_update_method(key);
            let panes = scene_children(element)
                .enumerate()
                .map(|(index, pane)| {
                    let id = pane.attr("id")?;
                    Ok(if id.is_empty() {
                        format!("@{index}")
                    } else {
                        id.to_owned()
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let resized = self.typed_event_values(element, "resized", &[])?;
            let (encode, decode) = if children(element, "xr.split_fill").next().is_some() {
                (quote!(encode_split_sizes), quote!(decode_split_sizes))
            } else {
                (quote!(encode_split_state), quote!(decode_split_state))
            };
            let weights = quote!(
                ::core::option::Option<
                    ::std::sync::Arc<[::core::option::Option<::core::primitive::f32>]>,
                >
            );
            fields.extend(quote!(#field: ::xrack_runtime::Property<#weights>,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            fields.extend(quote!(#handle: ::xrack_runtime::SplitResizeHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #save(&self) -> ::std::string::String {
                    let weights = self.__state.#field.get(|| None);
                    ::xrack_runtime::#encode(&[#(#panes),*], weights.as_deref())
                }
                fn #restore(&self, state: &::core::primitive::str) -> ::core::primitive::bool {
                    let Some(weights) = ::xrack_runtime::#decode(&[#(#panes),*], state) else { return false; };
                    let next = weights.iter().any(::core::option::Option::is_some).then_some(weights);
                    self.#update(next);
                    true
                }
                fn #update(&self, next: #weights) -> ::core::primitive::bool {
                    if self.__state.#field.get(|| None) == next {
                        return false;
                    }
                    self.__state.#field.set(next);
                    #resized
                    true
                }
            });
            let id = element.attr("id")?;
            let id = if id.is_empty() && *key == "root" {
                "root"
            } else {
                id
            };
            if !id.is_empty() {
                split_saves.extend(quote!(#id => Some(self.#save()),));
                split_restores.extend(quote!(#id => self.#restore(state),));
            }
        }
        if !split_saves.is_empty() {
            methods.extend(quote! {
                pub fn save_split_state(&self, id: &::core::primitive::str) -> ::core::option::Option<::std::string::String> {
                    match id { #split_saves _ => None }
                }
                pub fn restore_split_state(&mut self, id: &::core::primitive::str, state: &::core::primitive::str) -> ::core::primitive::bool {
                    match id { #split_restores _ => false }
                }
            });
        }
        let mut tree_reads = TokenStream::new();
        let mut tree_writes = TokenStream::new();
        for key in &self.trees {
            let field = tree_state_field(key);
            let element = self.elements[key];
            let (row, repeat) = self.list_template(element)?;
            let repeater = repeat_field(self.visuals.repeats[element_key(row)?].index());
            let model = self.expr(self.operand(repeat, 0)?)?;
            let tree = self.tree_snapshot(
                element,
                row,
                quote!(self.__state.__visuals.#repeater),
                &model,
            )?;
            let snapshot = tree_snapshot_method(key);
            let command = tree_command_method(key);
            let select = tree_selection_method(key);
            let current = self.control_value(element, "current-key", Type::Int)?;
            let set = self.set_control(element, &quote!(key))?;
            let changed = self.typed_event(element, "changed", &quote!(key))?;
            let expanded = self.typed_event(element, "expanded", &quote!(__xr_change.key))?;
            let collapsed = self.typed_event(element, "collapsed", &quote!(__xr_change.key))?;
            let selected = self.typed_event(
                element,
                "selection-changed",
                &quote!(count as ::core::primitive::i64),
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::TreeState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #snapshot(&self) -> ::std::sync::Arc<::xrack_runtime::TreeSnapshot> {
                    #tree
                }
                fn #select(
                    &self,
                    live: &::xrack_runtime::TreeSnapshot,
                    multiple: bool,
                    action: ::xrack_runtime::TreeSelectionAction,
                ) -> bool {
                    let current = #current;
                    if multiple {
                        let Some(change) = self.__state.#field.select(live, current, action) else { return false; };
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
                }
                fn #command(&self, command: ::xrack_runtime::TreeCommand) -> bool {
                    let live = self.#snapshot();
                    let Some(__xr_change) = self.__state.#field.command(&live, command) else { return false; };
                    let current = #current;
                    if let Some(key) = self.#snapshot().nearest_visible_ancestor(current).filter(|key| *key != current) {
                        #set
                        #changed
                    }
                    if __xr_change.expanded { #expanded } else { #collapsed }
                    true
                }
            });
            let id = element.attr("id")?;
            let id = if id.is_empty() && *key == "root" {
                "root"
            } else {
                id
            };
            if !id.is_empty() {
                tree_reads.extend(quote!(#id => Some(self.#snapshot()),));
                tree_writes.extend(quote!(#id => self.#command(command),));
            }
        }
        if !tree_reads.is_empty() {
            methods.extend(quote! {
                pub fn tree_snapshot(&self, id: &str) -> Option<::std::sync::Arc<::xrack_runtime::TreeSnapshot>> {
                    match id { #tree_reads _ => None }
                }
                pub fn command_tree(&mut self, id: &str, command: ::xrack_runtime::TreeCommand) -> bool {
                    match id { #tree_writes _ => false }
                }
            });
        }
        let mut stack_reads = TokenStream::new();
        let mut stack_writes = TokenStream::new();
        for (&key, &view) in &self.stacks {
            let field = stack_field(key);
            let snapshot = stack_snapshot_method(key);
            let navigate = stack_navigate_method(key);
            let mut routes = Vec::new();
            let mut guards = TokenStream::new();
            let mut activated = TokenStream::new();
            let mut deactivated = TokenStream::new();
            let mut removed = TokenStream::new();
            for page in scene_children(view) {
                let route = page.attr("id")?;
                routes.push(route);
                if let Some(handler) = children(page, "xr.handler")
                    .find(|handler| handler.attr("name") == Ok("leave-requested"))
                {
                    let body = self.typed_event_values(
                        page,
                        "leave-requested",
                        &[quote!(__xr_destination.to_owned()), quote!(__xr_removing)],
                    )?;
                    let result = children(handler, "xr.return")
                        .next()
                        .ok_or("missing leave-requested result")?;
                    let result = self.expr(self.operand(result, 0)?)?;
                    guards.extend(quote!(#route => { #body #result },));
                }
                for (event, arms) in [
                    ("activated", &mut activated),
                    ("deactivated", &mut deactivated),
                    ("removed", &mut removed),
                ] {
                    let body = self.typed_event_values(page, event, &[])?;
                    if !body.is_empty() {
                        arms.extend(quote!(#route => { #body },));
                    }
                }
            }
            let mut lifecycle = TokenStream::new();
            if !deactivated.is_empty() {
                lifecycle.extend(quote! {
                    if let Some(__xr_route) = __xr_change.deactivated() {
                        match __xr_route { #deactivated _ => {} }
                    }
                });
            }
            if !removed.is_empty() {
                lifecycle.extend(quote! {
                    for __xr_route in __xr_change.removed() {
                        match __xr_route { #removed _ => {} }
                    }
                });
            }
            if !activated.is_empty() {
                lifecycle.extend(quote! {
                    if let Some(__xr_route) = __xr_change.activated() {
                        match __xr_route { #activated _ => {} }
                    }
                });
            }
            let initial = children(view, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("initial-page"))
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .unwrap_or_else(|| quote!(::std::string::String::new()));
            let changed = self.typed_event_values(
                view,
                "changed",
                &[
                    quote!(__xr_change.after.current().unwrap_or("").to_owned()),
                    quote!(__xr_change.after.depth() as ::core::primitive::i64),
                ],
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::StackState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #snapshot(&self) -> ::std::sync::Arc<::xrack_runtime::StackSnapshot> {
                    self.__state.#field.snapshot(&[#(#routes),*], || #initial)
                }
                fn #navigate(&self, expected: Option<::core::primitive::u64>, action: ::xrack_runtime::StackNavigation<'_>) -> bool {
                    self.__state.#field.navigate_guarded(
                        &[#(#routes),*], || #initial, expected, action,
                        |__xr_route, __xr_destination, __xr_removing| {
                            match __xr_route { #guards _ => true }
                        },
                        |__xr_change| { #lifecycle #changed },
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
        let locale = self
            .with_translations
            .then(|| quote!(__locale: __inputs.__locale.clone(),));
        for (&key, &instance) in &self.instances {
            let field = instance_field(key);
            let component_name = instance.attr("component")?;
            let component = ident(component_name);
            let child_inputs = format_ident!("__Inputs_{}", rust_name(component_name));
            let bindings = self.instance_inputs(instance, None, None)?;
            fields.extend(quote!(#field: #component,));
            defaults.extend(quote! {
                #field: #component::__new(#child_inputs {
                    #bindings
                    #locale
                    ..::core::default::Default::default()
                }),
            });
            instance_routes.extend(quote!(#key => self.__state.#field.__dispatch(id, event),));
        }

        for &(instance, repeat) in &self.instance_handlers {
            let owner = element_key(instance)?;
            for callback in children(instance, "xr.callback") {
                methods.extend(self.callback_body(owner, callback, repeat)?);
            }
        }
        for (owner, callback) in self.declared_callbacks(component) {
            let name = callback.attr("name")?;
            let callback_name = rust_name(name);
            let input_field = callback_input_field(name);
            let field = callback_method("callback_slot", owner, name);
            let setter = callback_method("set_cb", owner, name);
            let on = format_ident!("on_{callback_name}");
            let invoke = format_ident!("invoke_{callback_name}");
            let invoke_internal = callback_method("invoke_cb", owner, name);
            let params = self.callback_parameters(callback)?;
            let types: Vec<_> = params.iter().map(|p| host_ty(&p.ty)).collect();
            let args: Vec<_> = params.iter().map(|p| ident(&p.name)).collect();
            let returns = self.callback_returns(callback)?;
            let call_trait = Self::callback_trait(callback);
            let pure = callback.attribute("pure") == Some("true");
            let handle = quote!(::std::rc::Rc<dyn ::core::ops::Fn(#(#types),*) #returns>);
            if owner == "root" {
                let installed = if pure {
                    quote!(callback)
                } else {
                    let arguments: Vec<_> = (0..params.len())
                        .map(|index| format_ident!("__xr_arg_{index}"))
                        .collect();
                    quote!({
                        let callback = ::core::cell::RefCell::new(callback);
                        move |#(#arguments),*| (callback.borrow_mut())(#(#arguments),*)
                    })
                };
                input_fields.extend(quote!(#input_field: ::core::option::Option<#handle>,));
                methods.extend(quote! {
                    pub fn #on(&mut self, callback: impl #call_trait(#(#types),*) #returns + 'static) {
                        self.#setter(::std::rc::Rc::new(#installed));
                    }
                    pub fn #invoke(&mut self #(, #args: #types)*) #returns {
                        self.#invoke_internal(#(#args),*)
                    }
                });
            }
            let (invoke_body, install) = if callback.attribute("alias_owner").is_some() {
                if callback.attribute("implementation") == Some("true") {
                    methods.extend(self.callback_body(owner, callback, None)?);
                }
                let (target, name) = self
                    .callback_graph
                    .resolve(self.callback_graphs, (owner, name))?;
                (
                    self.callable_method(
                        "invoke_cb",
                        &quote!(self),
                        target,
                        name,
                        &quote!(#(#args),*),
                    ),
                    self.callable_method("set_cb", &quote!(self), target, name, &quote!(callback)),
                )
            } else {
                let forwarded = self.forwarded_callback((owner, name))?;
                let incoming = match (owner == "root", forwarded) {
                    (true, Some(forwarded)) => {
                        quote!(__inputs.#input_field.take().or_else(|| #forwarded))
                    }
                    (true, None) => quote!(__inputs.#input_field.take()),
                    (false, Some(forwarded)) => forwarded,
                    (false, None) => quote!(::core::option::Option::None),
                };
                fields.extend(quote!(#field: ::xrack_runtime::CallbackSlot<#handle>,));
                defaults.extend(quote!(#field: ::xrack_runtime::CallbackSlot::new(#incoming),));
                let track = pure.then(|| quote!(self.__state.#field.track();));
                let statements = self.statements(callback)?;
                let fallback = self.callback_fallback(callback)?;
                (
                    quote! {
                        #track
                        #statements
                        if let ::core::option::Option::Some(__callback) = self.__state.#field.get() {
                            __callback(#(#args),*)
                        } else { #fallback }
                    },
                    quote!(self.__state.#field.set(callback);),
                )
            };
            methods.extend(quote! {
                fn #setter(&self, callback: #handle) { #install }
                fn #invoke_internal(&self #(, #args: #types)*) #returns { #invoke_body }
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
                        if #visible { self.__state.#child.__state.__window.request_close(); }
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

        let window_lifecycle = if root.attr("kind").is_ok_and(is_window) {
            fields.extend(quote! {
                __window: ::xrack_runtime::WindowLifecycle,
            });
            defaults.extend(quote! {
                __window: ::core::default::Default::default(),
            });
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
                    self.__state.__window.take_close_request()
                }

                fn take_window_commands(&mut self) -> ::std::vec::Vec<::xrack_runtime::WindowCommand> {
                    self.__state.__window.take_commands()
                }

                fn has_window_commands(&self) -> ::core::primitive::bool {
                    self.__state.__window.has_commands()
                }

                fn publish_window_status(&self, status: ::xrack_runtime::WindowStatus) {
                    self.__state.__window.publish(status);
                }
            }
        } else {
            TokenStream::new()
        };

        let groups = button_group_members(root);
        self.declare_check_groups(&groups, &mut fields, &mut defaults, &mut methods)?;
        let mut render = TokenStream::new();
        let node = self.render(root, &self.visuals, &mut render, &mut 0)?;
        self.bind_button_groups(&groups, &node, &mut render)?;
        let mut dispatch = TokenStream::new();
        let mut path_dispatch = TokenStream::new();
        self.dispatch_arms(root, &mut Vec::new(), &mut dispatch, &mut path_dispatch)?;
        let mut edits = TokenStream::new();
        self.edit_arms(root, &mut Vec::new(), &mut edits)?;
        let mut ranges = TokenStream::new();
        self.range_arms(root, &mut Vec::new(), &mut ranges)?;
        let mut controls = TokenStream::new();
        self.control_arms(root, &mut Vec::new(), &mut controls)?;
        let mut focus = TokenStream::new();
        self.focus_arms(root, &mut Vec::new(), &mut focus)?;
        let mut keys = TokenStream::new();
        self.key_arms(root, &mut Vec::new(), &mut keys)?;
        let mut wheels = TokenStream::new();
        let mut pointers = TokenStream::new();
        self.pointer_input_arms(root, &mut Vec::new(), &mut wheels, &mut pointers)?;
        let mut drag = TokenStream::new();
        self.drag_arms(root, &mut Vec::new(), &mut drag)?;
        let mut lists = TokenStream::new();
        self.list_arms(root, &mut Vec::new(), &mut lists)?;
        for (&key, &view) in &self.elements {
            if !has_virtual_rows(view) {
                continue;
            }
            let lookup = view_index_method(key);
            let index_body = self.view_index_body(view)?;
            let select = source_selection_method(key);
            let selection_body = self.source_selection_body(view)?;
            let map_source = if view.attr("kind") == Ok("TreeView") {
                TokenStream::new()
            } else {
                quote!(let Some(index) = self.#lookup(source, true) else { return; };)
            };
            methods.extend(quote! {
                fn #lookup(
                    &self, index: ::core::primitive::usize, from_source: ::core::primitive::bool,
                ) -> ::core::option::Option<::core::primitive::usize> {
                    #index_body
                }

                fn #select(&self, source: ::core::primitive::i64, column: ::core::option::Option<::core::primitive::i64>) {
                    let Ok(source) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(source)
                    else { return; };
                    #map_source
                    #selection_body
                }
            });
            if has_list_visibility(view) {
                let (binding, source) = if view.attr("kind") == Ok("StandardListView") {
                    let binding = model_binding(view).ok_or("list visibility requires a model")?;
                    let model = self.visual_value(binding, &self.visuals)?;
                    (binding, quote!(::std::rc::Rc::new(#model)))
                } else {
                    let (row, repeat) = self.list_template(view)?;
                    let field = repeat_field(self.visuals.repeats[element_key(row)?].index());
                    let model = self.expr(self.operand(repeat, 0)?)?;
                    (
                        repeat,
                        quote!(self.__state.__visuals.#field.resolve_model(|| #model)),
                    )
                };
                let Some(Type::RecordList(record)) = operand_type(binding) else {
                    return Err("list visibility requires a record list".into());
                };
                let record = ident(&record);
                let path = self
                    .record_field_binding(view, "row-visible-field")?
                    .ok_or("missing list visibility field")?;
                let visible = field_access(path.split('.'));
                let model = visible_list_model(key);
                let order = format_ident!("__list_visibility_order_{}", rust_key(key));
                fields.extend(quote!(#order: ::xrack_runtime::TableRowOrder,));
                defaults.extend(quote!(#order: ::core::default::Default::default(),));
                methods.extend(quote! {
                    fn #model(&self) -> ::xrack_runtime::TableRows<::xrack_runtime::VecModel<#record>> {
                        self.__state.#order.visible_rows(|| #source, |item: &#record| item #visible)
                    }
                });
            }
            let field = list_renderer_field(key);
            let method = list_rows_method(key);
            let body = self.list_row_body(view)?;
            fields.extend(quote!(#field: ::std::cell::OnceCell<::xrack_runtime::ListRowRenderer>,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #method(
                    &self,
                    __range: ::core::ops::Range<::core::primitive::usize>,
                    __xr_columns: &::xrack_runtime::TableColumnWindow,
                ) -> ::std::vec::Vec<::xrack_runtime::Node> {
                    #body
                }
            });
        }
        let mut edit_navigation = TokenStream::new();
        self.table_edit_navigation_arms(root, &mut Vec::new(), &mut edit_navigation)?;
        let mut clipboard = TableClipboardArms::default();
        self.table_clipboard_arms(root, &mut Vec::new(), &mut clipboard)?;
        let TableClipboardArms {
            copies,
            pastes,
            replays,
            fields: history_fields,
            defaults: history_defaults,
            methods: history_methods,
        } = clipboard;
        fields.extend(history_fields);
        defaults.extend(history_defaults);
        methods.extend(history_methods);
        let mut delegate_events = TokenStream::new();
        let mut delegates = TokenStream::new();
        self.delegate_event_arms(root, &mut Vec::new(), &mut delegate_events, &mut delegates)?;
        let set_locale_context = self.with_translations.then(|| {
            quote! {
                fn set_locale_context(&self, context: ::xrack_runtime::LocaleContext) {
                    self.__state.__locale.set_context(context);
                }
            }
        });
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

            // Expressions that can trap remain lazy even when a pass exposes
            // their constant operands to rustc.
            #[allow(arithmetic_overflow, unconditional_panic)]
            #[allow(dead_code, unused_mut, unused_variables, unused_parens, clippy::clone_on_copy)]
            impl #name {
                pub fn new() -> Self {
                    Self::__new(::core::default::Default::default())
                }

                pub fn as_weak(&self) -> ::xrack_runtime::WeakComponent<Self> {
                    ::xrack_runtime::WeakComponent::new(&self.__state, |__state| Self { __state })
                }

                fn __new(mut __inputs: #inputs) -> Self {
                    let __component = Self {
                        __state: ::std::rc::Rc::new_cyclic(
                            |__parent: &::std::rc::Weak<#state>| #state { #defaults },
                        ),
                    };
                    #viewport_initializers
                    __component
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

                fn __copy_table_path(
                    &self,
                    path: &[::core::primitive::usize],
                    node: &::xrack_runtime::Node,
                    columns: &[::core::primitive::usize],
                ) -> ::core::option::Option<::std::string::String> {
                    match path { #copies _ => ::core::option::Option::None }
                }

                fn __next_table_edit_cell(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    origin: ::xrack_runtime::TableEditPosition, columns: &[::core::primitive::usize],
                    backward: ::core::primitive::bool,
                ) -> ::core::option::Option<::xrack_runtime::TableEditPosition> {
                    match path { #edit_navigation _ => ::core::option::Option::None }
                }

                fn __prepare_table_paste(
                    &self, path: &[::core::primitive::usize],
                    node: &::xrack_runtime::Node,
                    row: ::core::primitive::usize, columns: ::std::vec::Vec<::core::primitive::usize>,
                ) -> ::core::option::Option<::xrack_runtime::TablePaste> {
                    match path { #pastes _ => ::core::option::Option::None }
                }

                fn __replay_table_edit(&self, path: &[::core::primitive::usize], redo: ::core::primitive::bool) -> ::core::primitive::bool {
                    match path { #replays _ => false }
                }

                fn __with_delegate<R: ::core::default::Default>(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    visit: impl FnOnce(&mut dyn ::xrack_runtime::Component, &::xrack_runtime::Node, &[::core::primitive::usize]) -> R,
                ) -> ::core::option::Option<R> {
                    match path { #delegates _ => None }
                }

                fn __route_delegate(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: &::xrack_runtime::RoutedEvent<'_>,
                ) -> ::core::option::Option<::core::primitive::bool> {
                    if let Some(result) = self.__with_delegate(scene, path, |ui, child, rest| action.apply(ui, child, rest)) {
                        return Some(result);
                    }
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

                fn __focus_path(&self, path: &[::core::primitive::usize], event: ::xrack_runtime::FocusEvent) -> ::core::primitive::bool {
                    match path { #focus _ => false }
                }

                fn __key_path(&self, path: &[::core::primitive::usize], phase: ::xrack_runtime::KeyPhase, event: &::xrack_runtime::KeyEvent) -> ::xrack_runtime::EventResult {
                    match path { #keys _ => ::xrack_runtime::EventResult::Reject }
                }

                fn __wheel_path(&self, path: &[::core::primitive::usize], event: &::xrack_runtime::WheelEvent) -> ::xrack_runtime::EventResult {
                    match path { #wheels _ => ::xrack_runtime::EventResult::Reject }
                }

                fn __pointer_path(&self, path: &[::core::primitive::usize], event: &::xrack_runtime::PointerEvent) -> ::core::primitive::bool {
                    match path { #pointers _ => false }
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

                fn __edit_path(&self, path: &[::core::primitive::usize], node: &::xrack_runtime::Node, text: ::std::string::String) -> ::core::primitive::bool {
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
                #set_locale_context
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

                fn copy_table_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    columns: &[::core::primitive::usize],
                ) -> ::core::option::Option<::std::string::String> {
                    let node = scene.interactive_at_path(path)?;
                    if !node.boolean("copyable", false) { return ::core::option::Option::None; }
                    self.__with_delegate(scene, path, |ui, child, rest| ui.copy_table_at(child, rest, columns))
                        .unwrap_or_else(|| self.__copy_table_path(path, node, columns))
                }

                fn next_table_edit_cell(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    origin: ::xrack_runtime::TableEditPosition, columns: &[::core::primitive::usize],
                    backward: ::core::primitive::bool,
                ) -> ::core::option::Option<::xrack_runtime::TableEditPosition> {
                    self.__with_delegate(scene, path, |ui, child, rest| ui.next_table_edit_cell(child, rest, origin, columns, backward))
                        .unwrap_or_else(|| self.__next_table_edit_cell(scene, path, origin, columns, backward))
                }

                fn prepare_table_paste(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    row: ::core::primitive::usize, columns: ::std::vec::Vec<::core::primitive::usize>,
                ) -> ::core::option::Option<::xrack_runtime::TablePaste> {
                    let node = scene.interactive_at_path(path)?;
                    if !node.boolean("pasteable", false) {
                        return ::core::option::Option::None;
                    }
                    match self.__with_delegate(scene, path, |ui, child, rest| {
                        ui.prepare_table_paste(child, rest, row, columns.clone())
                    }) {
                        Some(result) => {
                            let apply = result?;
                            let weak = self.as_weak();
                            let snapshot = scene.clone();
                            let path = path.to_vec();
                            Some(::std::boxed::Box::new(move |text| {
                                weak.upgrade().is_some_and(|ui| {
                                    ui.__with_delegate(&snapshot, &path, |_, _, _| true) == Some(true)
                                        && apply(text)
                                })
                            }))
                        }
                        None => self.__prepare_table_paste(path, node, row, columns),
                    }
                }

                fn replay_table_paste_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool,
                ) -> ::core::primitive::bool {
                    self.replay_table_edit_at(scene, path, redo)
                }

                fn replay_table_edit_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool,
                ) -> ::core::primitive::bool {
                    scene.interactive_at_path(path).is_some_and(::xrack_runtime::Node::table_edit_history_enabled)
                        && self.__with_delegate(scene, path, |ui, child, rest| ui.replay_table_edit_at(child, rest, redo))
                            .unwrap_or_else(|| self.__replay_table_edit(path, redo))
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
                    let ::core::option::Option::Some(node) = scene.range_target(path) else {
                        return false;
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

                fn focus_event_at(&mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], event: ::xrack_runtime::FocusEvent) -> ::core::primitive::bool {
                    let target = if event.is_loss() { scene.at_path(path) } else { scene.interactive_at_path(path) };
                    target.is_some_and(|node| node.kind == "FocusScope") && self
                        .__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Focus(event))
                        .unwrap_or_else(|| self.__focus_path(path, event))
                }

                fn key_event_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    phase: ::xrack_runtime::KeyPhase,
                    event: &::xrack_runtime::KeyEvent,
                ) -> ::xrack_runtime::EventResult {
                    if !scene.interactive_at_path(path).is_some_and(|node| {
                        node.kind == "FocusScope" && node.key_handlers & phase.mask() != 0
                    }) {
                        return ::xrack_runtime::EventResult::Reject;
                    }
                    if let Some(accepted) = self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Key(phase, event)) {
                        if accepted { ::xrack_runtime::EventResult::Accept } else { ::xrack_runtime::EventResult::Reject }
                    } else {
                        self.__key_path(path, phase, event)
                    }
                }

                fn wheel_event_at(
                    &mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    event: &::xrack_runtime::WheelEvent,
                ) -> ::xrack_runtime::EventResult {
                    use ::xrack_runtime::EventResult;
                    if !scene.interactive_at_path(path).is_some_and(|node| node.handles_wheel() && node.boolean("enabled", true)) {
                        return EventResult::Reject;
                    }
                    if let Some(accepted) = self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Wheel(event)) {
                        return if accepted { EventResult::Accept } else { EventResult::Reject };
                    }
                    self.__wheel_path(path, event)
                }

                fn pointer_event_at(
                    &mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    event: &::xrack_runtime::PointerEvent,
                ) -> ::core::primitive::bool {
                    let node = if event.kind == ::xrack_runtime::PointerEventKind::Cancel {
                        scene.at_path(path)
                    } else {
                        scene.interactive_at_path(path)
                    };
                    node.is_some_and(|node| node.pointer_handle.is_some())
                        && self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Pointer(event))
                            .unwrap_or_else(|| self.__pointer_path(path, event))
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

                fn resize_split_sizes_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    sizes: &[(::core::primitive::usize, ::core::primitive::f32)],
                ) -> ::core::primitive::bool {
                    scene.interactive_at_path(path).is_some_and(|node| {
                        node.kind == "SplitView" && self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::ResizeSplitSizes(sizes)).unwrap_or(false)
                    })
                }

                fn edit_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    text: ::std::string::String,
                ) -> ::core::primitive::bool {
                    let Some(node) = scene.interactive_at_path(path) else { return false; };
                    if !node.accepts_editor_update(&text) {
                        return false;
                    }
                    self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Edit(&text))
                        .unwrap_or_else(|| self.__edit_path(path, node, text))
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
                    scene.dispatch_target(path, event).is_some()
                        && self.__route_delegate(scene, path, &::xrack_runtime::RoutedEvent::Dispatch(event))
                        .unwrap_or_else(|| self.__dispatch_path(path, event))
                }
            }
        })
    }
}

/// Instantiate a generated component by its source name for the editor preview host.
pub fn preview_factory<'a>(names: impl Iterator<Item = &'a str>) -> TokenStream {
    let arms = names.map(|name| {
        let component = ident(name);
        quote!(#name => ::core::option::Option::Some(::std::boxed::Box::new(#component::new())),)
    });
    quote! {
        pub(crate) fn __xrack_preview_component(
            name: &str,
        ) -> ::core::option::Option<::std::boxed::Box<dyn ::xrack_runtime::Component>> {
            match name {
                #(#arms)*
                _ => ::core::option::Option::None,
            }
        }
    }
}

/// Generate Rust tokens directly from verified MLIR operations and their SSA uses.
pub fn generate(module: &Module) -> Result<TokenStream, String> {
    generate_with_images(module, &BTreeMap::new())
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

pub fn check_markdown(module: &Module) -> Result<(), String> {
    xrack_resources::collect_markdown(module).map(|_| ())
}

pub fn generate_with_resource_plans(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
    paths: &BTreeMap<String, EmbeddedPath>,
) -> Result<TokenStream, String> {
    generate_with_translations(module, images, fonts, paths, None)
}

fn collect_static_groups(
    op: OperationRef<'_>,
    gradients: &mut BTreeSet<i64>,
    scenes: &mut BTreeSet<i64>,
) {
    if matches!(op.name(), "xr.linear_gradient" | "xr.radial_gradient")
        && op.attribute("xr.static_gradient") == Some("true")
        && let Some(group) = op.integer_attribute("xr.static_gradient_group")
    {
        gradients.insert(group);
    }
    if op.name() == "xr.element"
        && op.attribute("xr.static_subtree") == Some("true")
        && let Some(group) = op.integer_attribute("xr.static_subtree_group")
    {
        scenes.insert(group);
    }
    for child in op.children() {
        collect_static_groups(child, gradients, scenes);
    }
}

pub fn generate_with_translations(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
    paths: &BTreeMap<String, EmbeddedPath>,
    translations: Option<&EmbeddedTranslations>,
) -> Result<TokenStream, String> {
    check_nine_slices(module, images)?;
    let mut out = quote! {
        const _: () = assert!(
            ::xrack_runtime::CODEGEN_ABI_VERSION == 1,
            "xrackc and xrack-runtime have incompatible code generation interfaces"
        );
    };
    if module.has_visual_cache_plan() {
        let mut gradients = BTreeSet::new();
        let mut scenes = BTreeSet::new();
        for op in module.operations() {
            collect_static_groups(op, &mut gradients, &mut scenes);
        }
        for group in gradients {
            let name = format_ident!("__XR_GRADIENT_{group}");
            out.extend(quote! {
                static #name: ::std::sync::OnceLock<::xrack_runtime::Brush> =
                    ::std::sync::OnceLock::new();
            });
        }
        for group in scenes {
            let name = format_ident!("__XR_STATIC_SCENE_{group}");
            out.extend(quote! {
                ::std::thread_local! {
                    static #name: ::std::cell::OnceCell<::xrack_runtime::Node> =
                        const { ::std::cell::OnceCell::new() };
                }
            });
        }
    }
    if let Some(translations) = translations {
        let default = &translations.default;
        let entries = translations
            .locales
            .iter()
            .map(|(locale, text)| quote!((#locale, #text)));
        out.extend(quote! {
            ::std::thread_local! {
                static __XR_TRANSLATIONS: ::xrack_runtime::TranslationCatalog =
                    ::xrack_runtime::TranslationCatalog::new(#default, &[#(#entries),*]);
            }
        });
    }
    if !fonts.is_empty() {
        let bytes = fonts.iter().map(|font| Literal::byte_string(font));
        out.extend(quote! {
            static __XR_FONTS: ::xrack_runtime::EmbeddedFonts =
                ::xrack_runtime::EmbeddedFonts::new(&[#(#bytes),*]);
        });
    } else if let Some(font) = module.operations().find(|op| op.name() == "xr.font") {
        return Err(format!("unresolved font {:?}", font.attr("path")?));
    }
    let mut markdown = BTreeMap::new();
    for (index, (source, text)) in xrack_resources::collect_markdown(module)?
        .into_iter()
        .enumerate()
    {
        let name = format_ident!("__XR_MARKDOWN_{index}");
        out.extend(styled_text::declaration(&name, &text));
        markdown.insert(source, name);
    }
    let mut image_names = BTreeMap::new();
    for (index, (path, image)) in images.iter().enumerate() {
        let name = format_ident!("__XR_IMAGE_{index}");
        out.extend(image::declaration(&name, image));
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
        let RecordDeclRef { name, fields } = RecordDeclRef::from_operation(op)?;
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
    let mut alias_graphs = BTreeMap::new();
    let mut callback_graphs = BTreeMap::new();
    for op in module.operations().filter(|op| op.name() == "xr.component") {
        let mut graph = AliasGraph::default();
        graph.index(op, false)?;
        alias_graphs.insert(op.attr("sym_name")?, graph);
        let mut graph = AliasGraph::default();
        graph.index(op, true)?;
        callback_graphs.insert(op.attr("sym_name")?, graph);
    }
    for op in module.operations().filter(|op| {
        !matches!(
            op.name(),
            "xr.record_decl" | "xr.font" | "xr.translation_catalog"
        )
    }) {
        let alias_graph = alias_graphs
            .get(op.attr("sym_name")?)
            .ok_or("missing component alias graph")?;
        let callback_graph = callback_graphs
            .get(op.attr("sym_name")?)
            .ok_or("missing callback alias graph")?;
        let mut generator = Generator {
            alias_graphs: &alias_graphs,
            alias_graph,
            forwarded_inputs: BTreeMap::new(),
            elements: BTreeMap::new(),
            callback_graphs: &callback_graphs,
            callback_graph,
            callback_forwards: BTreeMap::new(),
            instance_handlers: Vec::new(),
            records: &records,
            instances: BTreeMap::new(),
            persistent_state_selectors: BTreeMap::new(),
            visual_state_selectors: BTreeSet::new(),
            element_properties: Vec::new(),
            unlinked_controls: Vec::new(),
            record_combos: Vec::new(),
            picker_popups: Vec::new(),
            tables: Vec::new(),
            row_height_views: BTreeSet::new(),
            splits: Vec::new(),
            trees: Vec::new(),
            stacks: BTreeMap::new(),
            actions: BTreeMap::new(),
            timers: Vec::new(),
            animated_images: Vec::new(),
            native_dialogs: Vec::new(),
            focus_targets: BTreeSet::new(),
            pointer_targets: BTreeSet::new(),
            viewport_targets: BTreeSet::new(),
            source_scroll_targets: BTreeSet::new(),
            geometry_targets: BTreeSet::new(),
            bound_viewports: Vec::new(),
            scroll_observers: BTreeSet::new(),
            secondary_windows: Vec::new(),
            popups: Vec::new(),
            visuals: VisualScope::default(),
            repeat_invariants: Vec::new(),
            repeat_subexpressions: Vec::new(),
            use_visual_plan: module.has_visual_cache_plan(),
            images: &image_names,
            markdown: &markdown,
            paths: &path_names,
            with_fonts: !fonts.is_empty(),
            with_translations: translations.is_some(),
        };
        generator.index(op, None)?;
        let mut alias_defaults = BTreeSet::new();
        for (owner, callback) in generator.declared_callbacks(op) {
            if callback.attribute("alias_owner").is_some()
                && (owner == "root" || callback.attribute("implementation") == Some("true"))
            {
                let endpoint =
                    callback_graph.resolve(&callback_graphs, (owner, callback.attr("name")?))?;
                if callback.attribute("implementation") == Some("true") {
                    let mut destination = vec![endpoint];
                    if let Some(child) = callback_graph.instance(endpoint.0) {
                        destination[0].1 = "";
                        destination.extend(canonical_input(&callback_graphs, child, endpoint.1)?);
                    }
                    if !alias_defaults.insert(destination) {
                        return Err("multiple alias handlers target the same callback".into());
                    }
                }
                generator
                    .callback_forwards
                    .entry(endpoint)
                    .or_default()
                    .push((owner, callback));
            }
        }
        for property in children(op, "xr.property").filter(|property| {
            property.attribute("alias") == Some("true")
                && matches!(property.attribute("visibility"), Some("in" | "in-out"))
        }) {
            let name = property.attr("name")?;
            let target = alias_graph.resolve(&alias_graphs, ("root", name))?;
            generator
                .forwarded_inputs
                .entry(target)
                .or_default()
                .push(property_field("root", name));
        }
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
                visuals.index_value(VisualCacheKey::Planned(group), definition)?;
            }
            generator.visuals = visuals;
        }
        out.extend(generator.component(op)?);
    }
    Ok(out)
}
