//! Rust token generation from the typed UI operations exported from MLIR.
mod image;
mod live;
mod scene_plan;
mod styled_text;

pub use live::generate_live;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{format_ident, quote};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
};
use xrack_ir::semantic::{
    day_cell_inputs, is_window, rust_name, section_header_inputs, split_handle_inputs,
    visual_template_inputs,
};
use xrack_mlir::{
    AliasGraph, AliasGraphs, AnimationRef, BinaryOperator, CallbackForward, ComponentAnalysis,
    GradientRef, Module, OperationRef, PropertyEndpoint, RecordComboRef, RecordDeclRef,
    RecordUpdateRef, RepeatRef, RowSceneCache, StackRead, StandardTableRef, StateCaseRef,
    TableClipboardRef, TablePasteRef, TableRead, TranslationRef, TreeViewRef, UnaryOperator,
    ValueRef, VisualCacheGroup, has_button_check_state, repeat_is_positioned, scene_children,
    visual_cache_operand,
};
use xrack_resources::visual::{
    ControlDefault, ControlLiteral, action_ref, check_state_binding, check_state_control,
    combo_editor, control_default, control_link, controlled_properties, controlled_property,
    default_combo_edit_text, indexed_visual_bindings, non_exclusive_button_group,
    non_exclusive_group, operand_type, picker_value, scene_element, table_history_item,
    table_option_configured, text_item,
};
pub use xrack_resources::{EmbeddedImage, EmbeddedPath, EmbeddedTranslations, check_nine_slices};
use xrack_runtime::{is_multiline_text_input, is_toggle};
use xrack_syntax::{
    ast::{Parameter, Type},
    builtins::{ColorFunction, MathFunction, StringFunction, text_submission_rule},
};

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
        Type::IntList => quote!(::xrack_runtime::VecModel<::core::primitive::i64>),
        Type::DateList => quote!(::xrack_runtime::VecModel<::xrack_runtime::Date>),
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
        Type::LogLevel => quote!(::xrack_runtime::LogLevel),
        Type::ImageFit => quote!(::xrack_runtime::ImageFit),
        Type::ImageTileMode => quote!(::xrack_runtime::ImageTileMode),
        Type::ImageHorizontalAlignment => quote!(::xrack_runtime::ImageHorizontalAlignment),
        Type::ImageVerticalAlignment => quote!(::xrack_runtime::ImageVerticalAlignment),
        Type::ImageRendering => quote!(::xrack_runtime::ImageRendering),
        Type::ButtonDisplay => quote!(::xrack_runtime::ButtonDisplay),
        Type::Orientation => quote!(::xrack_runtime::Orientation),
        Type::FlickDirection => quote!(::xrack_runtime::FlickDirection),
        Type::FlickBoundsBehavior => quote!(::xrack_runtime::FlickBoundsBehavior),
        Type::FlickBoundsMovement => quote!(::xrack_runtime::FlickBoundsMovement),
        Type::LayoutDirection => quote!(::xrack_runtime::LayoutDirection),
        Type::FlexboxLayoutDirection => quote!(::xrack_runtime::FlexboxLayoutDirection),
        Type::FlexboxLayoutWrap => quote!(::xrack_runtime::FlexboxLayoutWrap),
        Type::DialInputMode => quote!(::xrack_runtime::DialInputMode),
        Type::ScrollBarPolicy => quote!(::xrack_runtime::ScrollBarPolicy),
        Type::ScrollSnapMode => quote!(::xrack_runtime::ScrollSnapMode),
        Type::ScrollAlignment => quote!(::xrack_runtime::ScrollAlignment),
        Type::TextWrap => quote!(::xrack_runtime::TextWrap),
        Type::MouseSelectionMode => quote!(::xrack_runtime::MouseSelectionMode),
        Type::InputValidity => quote!(::xrack_runtime::InputValidity),
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
        Type::SelectionGesture => quote!(::xrack_runtime::SelectionGesture),
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
        quote!((#value).into_snapshot())
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

fn write_cached_property(field: &Ident, value: &TokenStream, ty: Option<&Type>) -> TokenStream {
    let same = match ty {
        Some(Type::Float) => quote!(::xrack_runtime::same_float_bits),
        Some(Type::Length | Type::Percent) => {
            quote!(|left, right| ::xrack_runtime::same_float_bits(&left.0, &right.0))
        }
        _ => return quote!(self.__state.#field.set_if_changed(#value);),
    };
    quote!(self.__state.#field.set_if_changed_by(#value, #same);)
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
    op.result(0)
        .ok_or("missing XR value")?
        .xr_type()
        .map_err(|error| format!("{} has unsupported {error}", op.name()))
}

fn literal(spelling: &str, ty: Type) -> Result<TokenStream, String> {
    use xrack_resources::literal::Literal as XrLiteral;

    Ok(match xrack_resources::literal::decode(&ty, spelling)? {
        XrLiteral::Int(value) => {
            let number = Literal::i64_suffixed(value);
            quote!(#number)
        }
        XrLiteral::Float(value) => {
            let number = Literal::f64_suffixed(value);
            quote!(#number)
        }
        XrLiteral::Length(value) => {
            let number = Literal::f64_suffixed(value.0);
            quote!(::xrack_runtime::Length(#number))
        }
        XrLiteral::Percent(value) => {
            let number = Literal::f64_suffixed(value.0);
            quote!(::xrack_runtime::Percent(#number))
        }
        XrLiteral::Bool(value) => quote!(#value),
        XrLiteral::String(value) => quote!(::std::string::String::from(#value)),
        XrLiteral::Color(color) => {
            let rgba = Literal::u32_suffixed(color.0);
            quote!(::xrack_runtime::Color(#rgba))
        }
        XrLiteral::Enum { rust_variant, .. } => {
            let variant = format_ident!("{rust_variant}");
            let enum_type = self::ty(&ty);
            quote!(#enum_type::#variant)
        }
        XrLiteral::Default(_) => {
            let ty = self::ty(&ty);
            quote!(<#ty as ::core::default::Default>::default())
        }
    })
}

fn binary_operator(operator: BinaryOperator) -> TokenStream {
    use BinaryOperator::*;
    match operator {
        Add => quote!(+),
        Subtract => quote!(-),
        Multiply => quote!(*),
        Divide => quote!(/),
        Remainder => quote!(%),
        Equal => quote!(==),
        NotEqual => quote!(!=),
        Less => quote!(<),
        LessEqual => quote!(<=),
        Greater => quote!(>),
        GreaterEqual => quote!(>=),
        And => quote!(&&),
        Or => quote!(||),
    }
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

fn parameter_ident(name: &str) -> Ident {
    format_ident!("__xr_param_{}", rust_key(name))
}

fn control_field(key: &str, property: &str) -> Ident {
    format_ident!("__control_{}_{}", rust_key(key), rust_key(property))
}

fn timer_member(key: &str, member: &str) -> Ident {
    format_ident!("__timer_{}_{}", member, rust_key(key))
}

fn media_slot(key: &str) -> Ident {
    format_ident!("__media_{}", rust_key(key))
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

fn table_selection_command_method(key: &str) -> Ident {
    format_ident!("__table_select_command_{}", rust_key(key))
}

fn table_selection_input_method(key: &str) -> Ident {
    format_ident!("__table_select_input_{}", rust_key(key))
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

fn split_save_method(key: &str) -> Ident {
    format_ident!("__split_save_{}", rust_key(key))
}

fn split_restore_method(key: &str) -> Ident {
    format_ident!("__split_restore_{}", rust_key(key))
}

fn tree_state_field(key: &str) -> Ident {
    format_ident!("__tree_state_{}", rust_key(key))
}

fn tree_snapshot_method(key: &str) -> Ident {
    format_ident!("__tree_snapshot_{}", rust_key(key))
}

fn tree_projection_method(key: &str) -> Ident {
    format_ident!("__tree_projection_{}", rust_key(key))
}

fn tree_command_method(key: &str) -> Ident {
    format_ident!("__tree_command_{}", rust_key(key))
}

fn tree_selection_method(key: &str) -> Ident {
    format_ident!("__tree_selection_{}", rust_key(key))
}

fn tree_selection_info_method(key: &str) -> Ident {
    format_ident!("__tree_selection_info_{}", rust_key(key))
}

fn tree_selection_keys_method(key: &str) -> Ident {
    format_ident!("__tree_selection_keys_{}", rust_key(key))
}

fn tree_replace_selection_method(key: &str) -> Ident {
    format_ident!("__tree_replace_selection_{}", rust_key(key))
}

fn tree_input_method(key: &str) -> Ident {
    format_ident!("__tree_input_{}", rust_key(key))
}

fn search_method(key: &str, name: &str) -> Ident {
    format_ident!("__search_{}_{}", name, rust_key(key))
}

fn visual_field(value: usize) -> Ident {
    format_ident!("__visual_{value}")
}

fn dynamic_children(owner: &str) -> Ident {
    format_ident!("__dynamic_children_{}", rust_key(owner))
}

fn external_loader_slot(owner: &str) -> Ident {
    format_ident!("__external_loader_{}", rust_key(owner))
}

fn loader_slot(owner: &str) -> Ident {
    format_ident!("__loader_{}", rust_key(owner))
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

fn combo_items_field(key: &str) -> Ident {
    format_ident!("__combo_items_{}", rust_key(key))
}

fn combo_state_method(key: &str, property: &str) -> Ident {
    format_ident!("__combo_{}_{}", rust_key(key), rust_name(property))
}

fn combo_value_variant(ty: &Type) -> Result<Ident, String> {
    let name = match ty {
        Type::String => "String",
        Type::Int => "Int",
        Type::Float => "Float",
        Type::Bool => "Bool",
        Type::Date => "Date",
        Type::Time => "Time",
        _ => return Err("unsupported ComboBox value-role type".into()),
    };
    Ok(format_ident!("{name}"))
}

fn table_columns(element: OperationRef<'_>) -> impl Iterator<Item = OperationRef<'_>> {
    scene_children(element).filter(|column| column.attr("kind") == Ok("TableColumn"))
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

fn check_group_ref(element: OperationRef<'_>) -> Option<&str> {
    children(element, "xr.check_group_ref")
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

fn action_checked_method(key: &str) -> Ident {
    format_ident!("__action_checked_{}", rust_key(key))
}

fn button_checked_method(key: &str) -> Ident {
    format_ident!("__button_checked_{}", rust_key(key))
}

fn toggle_state_method(key: &str) -> Ident {
    format_ident!("__toggle_state_{}", rust_key(key))
}

fn numeric_value_method(key: &str, property: &str) -> Ident {
    format_ident!("__numeric_{}_{}", rust_key(key), rust_name(property))
}

fn range_position_handle(key: &str) -> Ident {
    format_ident!("__range_position_handle_{}", rust_key(key))
}

fn combo_popup_handle(key: &str) -> Ident {
    format_ident!("__combo_popup_handle_{}", rust_key(key))
}

fn range_position_method(key: &str, property: &str) -> Ident {
    format_ident!("__range_position_{}_{}", rust_key(key), rust_name(property))
}

fn radio_choice_method(key: &str) -> Ident {
    format_ident!("__radio_choice_{}", rust_key(key))
}

fn repeated_navigation_page(
    view: OperationRef<'_>,
) -> Option<(OperationRef<'_>, OperationRef<'_>)> {
    if !matches!(
        view.attribute("kind"),
        Some("TabWidget" | "SwipeView" | "StackLayout")
    ) {
        return None;
    }
    let page = scene_children(view).next()?;
    let repeat = children(page, "xr.repeat").next()?;
    Some((page, repeat))
}

fn navigation_selects_by_value(element: OperationRef<'_>) -> bool {
    element
        .attribute("kind")
        .is_some_and(xrack_syntax::builtins::navigation_group)
        && controlled_property(element).is_some_and(|(name, _)| name == "current-value")
}

fn navigation_choice_method(key: &str) -> Ident {
    format_ident!("__navigation_choice_{}", rust_key(key))
}

fn row_button_binding(key: &str, name: &str) -> Ident {
    let encoded: String = key.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__row_button_{name}_{encoded}")
}

fn action_activate_method(key: &str) -> Ident {
    format_ident!("__activate_action_{}", rust_key(key))
}

fn action_state_flag_method(key: &str, flag: &str) -> Ident {
    format_ident!("__action_state_{}_{}", rust_key(key), rust_name(flag))
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

fn global_accessor(name: &str) -> Ident {
    format_ident!("global_{}", rust_name(name))
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

fn has_editing_finished(element: OperationRef<'_>) -> bool {
    children(element, "xr.handler").any(|handler| handler.attr("name") == Ok("editing-finished"))
}

fn focus_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_focus_{encoded}")
}

fn button_press_handle(owner: &str) -> Ident {
    format_ident!("__button_press_{}", rust_key(owner))
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

fn swipe_preview_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_swipe_preview_{encoded}")
}

fn viewport_handle(owner: &str) -> Ident {
    let encoded: String = owner.bytes().map(|byte| format!("{byte:02x}")).collect();
    format_ident!("__xr_viewport_{encoded}")
}

fn observed_scroll_read(op: OperationRef<'_>) -> bool {
    matches!(
        op.attribute("value"),
        Some("visual-position" | "visual-size" | "active" | "pressed" | "hovered")
    )
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

fn list_key_index_method(key: &str) -> Ident {
    format_ident!("__list_index_for_key_{}", rust_key(key))
}

fn list_select_key_method(key: &str) -> Ident {
    format_ident!("__list_select_key_{}", rust_key(key))
}

fn list_key_locator_method(key: &str) -> Ident {
    format_ident!("__list_key_locator_{}", rust_key(key))
}

fn list_selected_index_method(key: &str) -> Ident {
    format_ident!("__list_selected_index_{}", rust_key(key))
}

fn list_selection_key_method(key: &str) -> Ident {
    format_ident!("__list_selection_key_{}", rust_key(key))
}

fn list_selection_index_method(key: &str) -> Ident {
    format_ident!("__list_selection_index_{}", rust_key(key))
}

fn list_multi_selection(element: OperationRef<'_>) -> bool {
    matches!(
        element.attr("kind"),
        Ok("ListView" | "GridView" | "StandardListView")
    ) && children(element, "xr.bind").any(|binding| binding.attr("name") == Ok("multi-select"))
}

fn list_selection_field(key: &str) -> Ident {
    format_ident!("__list_selection_{}", rust_key(key))
}

fn list_selection_command_method(key: &str) -> Ident {
    format_ident!("__list_selection_command_{}", rust_key(key))
}

fn list_selection_info_method(key: &str) -> Ident {
    format_ident!("__list_selection_info_{}", rust_key(key))
}

fn list_selection_keys_method(key: &str) -> Ident {
    format_ident!("__list_selection_keys_{}", rust_key(key))
}

fn list_replace_selection_method(key: &str) -> Ident {
    format_ident!("__list_replace_selection_{}", rust_key(key))
}

fn view_count_method(key: &str) -> Ident {
    format_ident!("__view_count_{}", rust_key(key))
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

fn section_renderer_field(key: &str) -> Ident {
    format_ident!("__section_renderer_{}", rust_key(key))
}

fn section_header_method(key: &str) -> Ident {
    format_ident!("__section_header_{}", rust_key(key))
}

fn combo_option_method(key: &str) -> Ident {
    format_ident!("__combo_option_{}", rust_key(key))
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

fn child_slots(op: OperationRef<'_>) -> impl Iterator<Item = (usize, OperationRef<'_>)> {
    // The first two RangeSlider children are the generated handles.
    let first = if op.attr("kind") == Ok("RangeSlider") {
        xrack_runtime::Node::RANGE_THUMBS
    } else {
        0
    };
    scene_children(op)
        .enumerate()
        .map(move |(index, child)| (first + index, child))
}

fn empty_state(view: OperationRef<'_>) -> Option<OperationRef<'_>> {
    element_children(view).find(|child| child.attribute("kind") == Some("EmptyState"))
}

fn static_children(op: OperationRef<'_>) -> impl Iterator<Item = (usize, OperationRef<'_>)> {
    let has_empty_state = has_virtual_rows(op) && empty_state(op).is_some();
    child_slots(op).filter_map(move |(index, child)| {
        if has_empty_state {
            (child.attribute("kind") == Some("EmptyState")).then_some((0, child))
        } else {
            children(child, "xr.repeat")
                .next()
                .is_none()
                .then_some((index, child))
        }
    })
}

fn has_repeated_check_state(element: OperationRef<'_>) -> bool {
    element.attr("kind") == Ok("GroupBox") || has_button_check_state(element)
}

type GroupMembers<'a> = BTreeMap<&'a str, Vec<(Vec<usize>, OperationRef<'a>)>>;

fn button_group_members(root: OperationRef<'_>) -> GroupMembers<'_> {
    fn collect<'a>(
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        groups: &mut GroupMembers<'a>,
    ) {
        if let Some(group) = radio_group_ref(element).or_else(|| check_group_ref(element)) {
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
    editors: &mut Vec<(Vec<usize>, OperationRef<'a>, OperationRef<'a>)>,
) {
    for (index, child) in static_children(row) {
        path.push(index);
        if let Some(link) = children(child, "xr.row_link").next() {
            editors.push((path.clone(), child, link));
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

type VisualCacheKey<'a> = xrack_mlir::VisualCacheKey<ValueRef<'a>>;

#[derive(Default)]
struct VisualScope<'a> {
    row_index: Option<usize>,
    combo_option: bool,
    bindings: HashMap<VisualCacheKey<'a>, usize>,
    binding_types: Vec<Type>,
    repeats: BTreeMap<&'a str, RepeatScope<'a>>,
    loaders: BTreeSet<&'a str>,
    external_loaders: BTreeSet<&'a str>,
    row_delegates: BTreeMap<&'a str, usize>,
    row_checks: BTreeMap<&'a str, OperationRef<'a>>,
}

struct RepeatScope<'a> {
    model: Type,
    row: VisualScope<'a>,
    delegates: Vec<(Vec<usize>, OperationRef<'a>)>,
    table: bool,
    uses_row_identity: bool,
    loader: Option<(&'a str, usize)>,
}

impl RepeatScope<'_> {
    #[expect(
        clippy::expect_used,
        reason = "a RepeatScope is constructed with a row index"
    )]
    fn index(&self) -> usize {
        self.row.row_index.expect("repeat row index")
    }

    fn uses_row_identity(&self) -> bool {
        self.uses_row_identity
    }

    fn storage_type(&self, component: &str) -> TokenStream {
        let row_type = self.row.state_name(component);
        let model_type = ty(&self.model);
        if self.uses_row_identity() {
            let types = self.delegates.iter().map(|(_, delegate)| {
                #[expect(clippy::expect_used, reason = "MLIR verification requires a component on each row delegate")]
                let component = ident(delegate.attr("component").expect("verified component"));
                if self.table {
                    quote!(::std::cell::LazyCell<#component, ::std::boxed::Box<dyn FnOnce() -> #component>>)
                } else {
                    quote!(#component)
                }
            });
            quote!(::xrack_runtime::ComponentRepeater<#model_type, #row_type, (#(#types,)*)>)
        } else {
            quote!(::xrack_runtime::Repeater<#model_type, #row_type>)
        }
    }
}

#[derive(Clone, Copy)]
struct ExpressionContext<'a> {
    receiver: &'a TokenStream,
    repeat_parameters: bool,
    hoist_repeat_subexpressions: bool,
    row_toggle_owner: Option<&'a str>,
    row_context: bool,
    combo_option: bool,
}

impl<'a> ExpressionContext<'a> {
    fn new(receiver: &'a TokenStream) -> Self {
        Self {
            receiver,
            repeat_parameters: false,
            hoist_repeat_subexpressions: false,
            row_toggle_owner: None,
            row_context: false,
            combo_option: false,
        }
    }

    fn row_index(self) -> TokenStream {
        if self.row_context {
            quote!(__context.index() as usize)
        } else {
            quote!(__xr_index)
        }
    }
}

#[derive(Clone, Copy)]
enum GroupOptionRead {
    Select(bool),
    Close,
}

struct TableProjection<'context, 'module> {
    expression: ExpressionContext<'context>,
    routed_input: bool,
    selectors: RefCell<BTreeMap<&'module str, OperationRef<'module>>>,
}

impl<'context> TableProjection<'context, '_> {
    fn new(expression: ExpressionContext<'context>) -> Self {
        Self {
            expression,
            routed_input: false,
            selectors: RefCell::default(),
        }
    }

    fn for_input(receiver: &'context TokenStream) -> Self {
        Self {
            routed_input: true,
            ..Self::new(ExpressionContext::new(receiver))
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
        self.receiver_in(&quote!(self))
    }

    fn receiver_in(&self, component: &TokenStream) -> TokenStream {
        if let Some(index) = self.row_index {
            let row = row_local(index);
            quote!(#row)
        } else {
            quote!(#component.__state.__visuals)
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
        for (&key, &element) in &self.row_checks {
            if control_link(element, "checked").is_some() {
                continue;
            }
            let field = control_field(key, "checked");
            fields.extend(quote!(#field: ::xrack_runtime::Property<::core::primitive::bool>,));
        }
        for &owner in &self.external_loaders {
            let field = external_loader_slot(owner);
            fields.extend(quote!(#field: ::xrack_runtime::ExternalLoader,));
        }
        for &owner in &self.loaders {
            let field = loader_slot(owner);
            fields.extend(quote!(#field: ::xrack_runtime::LoaderSlot,));
        }
        for repeat in self.repeats.values() {
            repeat.row.declare(component, declarations);
            if repeat.loader.is_none() {
                let field = repeat_field(repeat.index());
                let storage = repeat.storage_type(component);
                fields.extend(quote!(#field: #storage,));
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
    component_name: &'a str,
    root_key: &'a str,
    alias_graphs: &'a BTreeMap<&'a str, AliasGraph<'a>>,
    alias_graph: &'a AliasGraph<'a>,
    elements: BTreeMap<&'a str, OperationRef<'a>>,
    callback_graphs: &'a BTreeMap<&'a str, AliasGraph<'a>>,
    callback_graph: &'a AliasGraph<'a>,
    instance_handlers: Vec<(OperationRef<'a>, Option<OperationRef<'a>>)>,
    records: &'a BTreeMap<&'a str, Vec<Parameter>>,
    instances: BTreeMap<&'a str, OperationRef<'a>>,
    analysis: ComponentAnalysis<'a>,
    element_properties: Vec<(&'a str, OperationRef<'a>)>,
    unlinked_controls: Vec<(&'a str, &'static str, Type)>,
    record_combos: BTreeMap<&'a str, RecordComboRef<'a>>,
    picker_popups: Vec<(&'a str, Type)>,
    tables: Vec<OperationRef<'a>>,
    standard_tables: BTreeMap<&'a str, StandardTableRef<'a>>,
    row_height_views: BTreeSet<&'a str>,
    sectioned_lists: BTreeSet<&'a str>,
    section_headers: BTreeMap<&'a str, OperationRef<'a>>,
    combo_options: BTreeMap<&'a str, OperationRef<'a>>,
    splits: Vec<&'a str>,
    trees: BTreeMap<&'a str, TreeViewRef<'a>>,
    stacks: BTreeMap<&'a str, OperationRef<'a>>,
    actions: BTreeMap<&'a str, OperationRef<'a>>,
    timers: Vec<OperationRef<'a>>,
    animated_images: Vec<OperationRef<'a>>,
    media_players: Vec<OperationRef<'a>>,
    native_dialogs: Vec<OperationRef<'a>>,
    focus_targets: BTreeSet<&'a str>,
    button_press_targets: BTreeSet<&'a str>,
    button_checked_targets: BTreeSet<&'a str>,
    toggle_state_targets: BTreeSet<&'a str>,
    numeric_read_targets: BTreeSet<(&'a str, &'a str)>,
    combo_state_targets: BTreeSet<(&'a str, &'a str)>,
    combo_popup_targets: BTreeSet<&'a str>,
    range_position_targets: BTreeMap<&'a str, BTreeSet<&'a str>>,
    radio_read_groups: BTreeSet<&'a str>,
    navigation_read_groups: BTreeSet<&'a str>,
    repeated_buttons: BTreeSet<&'a str>,
    button_animation_targets: BTreeSet<&'a str>,
    text_selection_targets: BTreeSet<&'a str>,
    text_command_targets: BTreeSet<&'a str>,
    text_validation_targets: BTreeSet<(&'a str, bool)>,
    pointer_targets: BTreeSet<&'a str>,
    viewport_targets: BTreeSet<&'a str>,
    view_count_targets: BTreeSet<&'a str>,
    list_selection_info_targets: BTreeSet<&'a str>,
    list_selection_command_targets: BTreeSet<&'a str>,
    list_selection_targets: BTreeSet<&'a str>,
    key_scroll_targets: BTreeSet<&'a str>,
    view_empty_state_owners: BTreeMap<&'a str, &'a str>,
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
    entry_file: &'a str,
    with_fonts: bool,
    with_locale: bool,
    globals: &'a [OperationRef<'a>],
    has_translation_catalog: bool,
}

#[derive(Default)]
struct TableClipboardArms {
    copies: TokenStream,
    pastes: TokenStream,
    paste_stamps: TokenStream,
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
}

enum RenderContent {
    Inline,
    Active(TokenStream),
    Deferred(Ident),
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
                if repeat.is_some()
                    && op
                        .attribute("kind")
                        .is_some_and(xrack_syntax::builtins::push_button)
                {
                    self.repeated_buttons.insert(key);
                }
                self.elements.insert(key, op);
                if navigation_selects_by_value(op)
                    || repeated_navigation_page(op).is_some()
                    || op.attribute("kind") == Some("StackLayout")
                {
                    self.navigation_read_groups.insert(key);
                }
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
                if let Some(slot) = empty_state(op) {
                    self.view_empty_state_owners.insert(element_key(slot)?, key);
                    self.view_count_targets.insert(key);
                }
                if row_height_binding(op).is_some() {
                    self.row_height_views.insert(key);
                }
                if children(op, "xr.bind")
                    .any(|binding| binding.attr("name") == Ok("section-field"))
                {
                    self.sectioned_lists.insert(key);
                }
                if let Some(header) = children(op, "xr.element")
                    .find(|child| child.attribute("kind") == Some("SectionHeader"))
                {
                    self.section_headers.insert(key, header);
                }
                if let Some(option) = children(op, "xr.element").find(|child| {
                    matches!(
                        child.attribute("kind"),
                        Some("ComboOption" | "TumblerOption" | "PathViewOption")
                    )
                }) {
                    self.combo_options.insert(key, option);
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
                if has_editing_finished(op)
                    || children(op, "xr.bind")
                        .any(|bind| bind.attr("name") == Ok("persistent-selection"))
                    || children(op, "xr.handler").any(|handler| {
                        handler
                            .attribute("name")
                            .is_some_and(xrack_runtime::is_focus_handler)
                    })
                {
                    self.focus_targets.insert(key);
                }
                if matches!(
                    op.attribute("kind"),
                    Some("Button" | "ToolButton" | "RoundButton")
                ) && children(op, "xr.handler").any(|handler| {
                    matches!(
                        handler.attribute("name"),
                        Some("pressed" | "released" | "canceled")
                    )
                }) {
                    self.button_press_targets.insert(key);
                }
                if let Some(combo) = RecordComboRef::from_operation(op)? {
                    self.record_combos.insert(key, combo);
                }
                if let Some((_, ty)) = op.attribute("kind").and_then(picker_value) {
                    self.picker_popups.push((key, ty));
                }
                if matches!(op.attr("kind"), Ok("TableView" | "StandardTableView")) {
                    self.tables.push(op);
                }
                if op.attr("kind") == Ok("StandardTableView") {
                    self.standard_tables
                        .insert(key, StandardTableRef::from_operation(op)?);
                }
                if op.attr("kind") == Ok("SplitView") {
                    self.splits.push(key);
                }
                if op.attr("kind") == Ok("TreeView") {
                    self.trees.insert(key, TreeViewRef::from_operation(op)?);
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
                if matches!(op.attribute("kind"), Some("MediaPlayer" | "SoundEffect")) {
                    self.media_players.push(op);
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
                    if op.attr("kind") != Ok("Action")
                        && control_link(op, property).is_none()
                        && !(repeat.is_some() && has_repeated_check_state(op))
                    {
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
                "xr.focus_target" => {
                    self.focus_targets.insert(child.attr("target")?);
                }
                "xr.focus_read" => {
                    self.focus_targets.insert(child.attr("owner")?);
                }
                "xr.button_pressed" => {
                    self.button_press_targets.insert(child.attr("owner")?);
                }
                "xr.button_checked" => {
                    self.button_checked_targets.insert(child.attr("owner")?);
                }
                "xr.toggle_state_read" => {
                    self.toggle_state_targets.insert(child.attr("owner")?);
                }
                "xr.numeric_value_read" => {
                    self.numeric_read_targets
                        .insert((child.attr("owner")?, child.attr("value")?));
                }
                "xr.combo_state_read" => {
                    self.combo_state_targets
                        .insert((child.attr("owner")?, child.attr("value")?));
                }
                "xr.combo_popup_read" => {
                    self.combo_popup_targets.insert(child.attr("owner")?);
                }
                "xr.range_position_read" => {
                    self.range_position_targets
                        .entry(child.attr("owner")?)
                        .or_default()
                        .insert(child.attr("value")?);
                }
                "xr.radio_checked" => {
                    self.radio_read_groups.insert(child.attr("value")?);
                }
                "xr.navigation_active" => {
                    self.navigation_read_groups.insert(child.attr("value")?);
                }
                "xr.text_selection_read" | "xr.text_command_available" => {
                    self.index_editor_state(child)?;
                }
                "xr.text_accepts" | "xr.text_validity" => {
                    self.text_validation_targets
                        .insert((child.attr("owner")?, child.name() == "xr.text_validity"));
                }
                "xr.view_count" => {
                    self.view_count_targets.insert(child.attr("owner")?);
                }
                "xr.list_selection_read" => {
                    let owner = child.attr("owner")?;
                    self.list_selection_targets.insert(owner);
                    self.view_count_targets.insert(owner);
                }
                "xr.table_selection_read" | "xr.list_row_selected" => {
                    self.list_selection_info_targets
                        .insert(child.attr("owner")?);
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
                "xr.scroll_control_read" if observed_scroll_read(child) => {
                    self.geometry_targets.insert(child.attr("owner")?);
                }
                "xr.callback" | "xr.handler" | "xr.clipboard_paste" => {
                    for effect in child.children() {
                        match effect.name() {
                            "xr.focus_command" => {
                                self.focus_targets.insert(effect.attr("target")?);
                            }
                            "xr.button_click" => {
                                let owner = effect.attr("owner")?;
                                self.focus_targets.insert(owner);
                                self.button_press_targets.insert(owner);
                                if effect.attribute("animated") == Some("true") {
                                    self.button_animation_targets.insert(owner);
                                }
                            }
                            "xr.focus_read" | "xr.text_command" => {
                                self.focus_targets.insert(effect.attr("owner")?);
                            }
                            "xr.button_pressed" => {
                                self.button_press_targets.insert(effect.attr("owner")?);
                            }
                            "xr.button_checked" => {
                                self.button_checked_targets.insert(effect.attr("owner")?);
                            }
                            "xr.toggle_state_read" => {
                                self.toggle_state_targets.insert(effect.attr("owner")?);
                            }
                            "xr.numeric_value_read" => {
                                self.numeric_read_targets
                                    .insert((effect.attr("owner")?, effect.attr("value")?));
                            }
                            "xr.combo_state_read" => {
                                self.combo_state_targets
                                    .insert((effect.attr("owner")?, effect.attr("value")?));
                            }
                            "xr.combo_popup_read" => {
                                self.combo_popup_targets.insert(effect.attr("owner")?);
                            }
                            "xr.range_position_read" => {
                                self.range_position_targets
                                    .entry(effect.attr("owner")?)
                                    .or_default()
                                    .insert(effect.attr("value")?);
                            }
                            "xr.radio_checked" => {
                                self.radio_read_groups.insert(effect.attr("value")?);
                            }
                            "xr.navigation_active" => {
                                self.navigation_read_groups.insert(effect.attr("value")?);
                            }
                            "xr.text_selection_read" | "xr.text_command_available" => {
                                self.index_editor_state(effect)?;
                            }
                            "xr.text_accepts" | "xr.text_validity" => {
                                self.text_validation_targets.insert((
                                    effect.attr("owner")?,
                                    effect.name() == "xr.text_validity",
                                ));
                            }
                            "xr.viewport_read"
                            | "xr.scroll_command"
                            | "xr.flick_motion_read"
                            | "xr.cancel_flick"
                            | "xr.return_to_bounds" => {
                                let owner = effect.attr("owner")?;
                                let method = effect
                                    .attribute("method")
                                    .and_then(xrack_runtime::ScrollCommandMethod::parse);
                                if method.is_none_or(|method| method.bar_step().is_none()) {
                                    self.viewport_targets.insert(owner);
                                }
                                if effect.name() == "xr.scroll_command"
                                    && let Some(method) = method
                                {
                                    if method.uses_source_row() {
                                        self.source_scroll_targets.insert(owner);
                                    }
                                    if method.uses_business_key() {
                                        self.key_scroll_targets.insert(owner);
                                    }
                                }
                            }
                            "xr.view_count" => {
                                self.view_count_targets.insert(effect.attr("owner")?);
                            }
                            "xr.list_selection_read" => {
                                let owner = effect.attr("owner")?;
                                self.list_selection_targets.insert(owner);
                                self.view_count_targets.insert(owner);
                            }
                            "xr.table_selection_read" | "xr.list_row_selected" => {
                                self.list_selection_info_targets
                                    .insert(effect.attr("owner")?);
                            }
                            "xr.table_selection_command" => {
                                self.list_selection_command_targets
                                    .insert(effect.attr("owner")?);
                            }
                            "xr.geometry_read" => {
                                self.geometry_targets.insert(effect.attr("owner")?);
                            }
                            "xr.position_child" => {
                                self.viewport_targets.insert(effect.attr("owner")?);
                                self.geometry_targets.insert(effect.attr("target")?);
                            }
                            "xr.scroll_control_read" if observed_scroll_read(effect) => {
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

    fn index_editor_state(&mut self, read: OperationRef<'a>) -> Result<(), String> {
        let owner = read.attr("owner")?;
        self.focus_targets.insert(owner);
        if read.name() == "xr.text_selection_read" {
            self.text_selection_targets.insert(owner);
        } else {
            self.text_command_targets.insert(owner);
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
        loader: Option<(&'a str, usize)>,
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
                    loader,
                    table: element.attr("kind") == Ok("TableRow"),
                    uses_row_identity: xrack_mlir::repeat_uses_row_identity(
                        element,
                        self.use_visual_plan,
                    ),
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
        if scope.row_index.is_some() && has_repeated_check_state(element) {
            scope.row_checks.insert(element_key(element)?, element);
        }
        for binding in indexed_visual_bindings(element) {
            self.index_visual_binding(binding, scope)?;
        }
        let loader = (element.attribute("kind") == Some("Loader"))
            .then(|| element_key(element))
            .transpose()?;
        if let Some(owner) = loader {
            scope.loaders.insert(owner);
            if children(element, "xr.bind").any(|bind| bind.attribute("name") == Some("source-url"))
            {
                scope.external_loaders.insert(owner);
            }
        }
        for (index, child) in scene_children(element).enumerate() {
            self.index_visual_bindings(child, scope, next, loader.map(|owner| (owner, index)))?;
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

    #[expect(clippy::expect_used, reason = "the verified binding has one operand")]
    fn visual_cache_key(&self, binding: OperationRef<'a>) -> VisualCacheKey<'a> {
        VisualCacheKey::for_binding(binding, self.use_visual_plan)
            .expect("verified binding operand")
    }

    fn visual_value(
        &self,
        binding: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        self.visual_value_in(binding, scope, &quote!(self))
    }

    fn visual_value_in(
        &self,
        binding: OperationRef<'a>,
        scope: &VisualScope<'_>,
        receiver: &TokenStream,
    ) -> Result<TokenStream, String> {
        let value = self.visual_expr_in(self.operand(binding, 0)?, scope, receiver)?;
        let key = self.visual_cache_key(binding);
        let cache = if key.uses_component_scope() {
            &self.visuals
        } else {
            scope
        };
        if let Some(&id) = cache.bindings.get(&key) {
            let field = visual_field(id);
            let receiver = cache.receiver_in(receiver);
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
        self.visual_expr_in(op, scope, &quote!(self))
    }

    fn visual_expr_in(
        &self,
        op: OperationRef<'a>,
        scope: &VisualScope<'_>,
        receiver: &TokenStream,
    ) -> Result<TokenStream, String> {
        self.expr_in(
            op,
            ExpressionContext {
                receiver,
                repeat_parameters: scope.row_index.is_some(),
                hoist_repeat_subexpressions: scope.row_index.is_some(),
                row_toggle_owner: None,
                row_context: false,
                combo_option: scope.combo_option,
            },
        )
    }

    fn dynamic_expression(
        &self,
        op: OperationRef<'a>,
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        let names: Vec<_> = op.attr("value")?.split(':').collect();
        let [verb, component, members @ ..] = names.as_slice() else {
            return Err("invalid dynamic component operation".into());
        };
        let component_type = ident(component);
        let receiver = context.receiver;
        let slot = dynamic_children(owner(op));
        let children = quote!(#receiver.__state.#slot);
        let arg = |index| self.expr_in(self.operand(op, index)?, context);
        if *verb == "mount-url" {
            let source = arg(0)?;
            let Type::Record(handle) = value_type(op)? else {
                return Err("mount requires a component handle result".into());
            };
            let handle = ident(&handle);
            return Ok(quote!(#handle { id: #children.mount_url(#source) }));
        }
        if matches!(*verb, "mount" | "mount-async") {
            let fields = members.iter().map(|name| property_field("root", name));
            let values = (0..members.len()).map(arg).collect::<Result<Vec<_>, _>>()?;
            let captures: Vec<_> = (0..values.len())
                .map(|i| format_ident!("__xr_initial_{i}"))
                .collect();
            let inputs = format_ident!("__Inputs_{}", rust_name(component));
            let globals = (!self.globals.is_empty()).then(|| quote!(__globals: __xr_globals,));
            let locale = self.with_locale.then(|| quote!(__locale: __xr_locale,));
            let capture_globals = (!self.globals.is_empty())
                .then(|| quote!(let __xr_globals = #receiver.__state.__globals.clone();));
            let capture_locale = self
                .with_locale
                .then(|| quote!(let __xr_locale = #receiver.__state.__locale.clone();));
            let Type::Record(handle) = value_type(op)? else {
                return Err("mount requires a component handle result".into());
            };
            let handle = ident(&handle);
            let create = quote! {
                let __xr_item = #component_type::__new(#inputs {
                    #(#fields: ::core::option::Option::Some({
                        let input = ::xrack_runtime::InputProperty::default();
                        input.set_if_changed_by(#captures, |_, _| false);
                        input
                    }),)*
                    #globals #locale ..::core::default::Default::default()
                });
                let weak = __xr_item.as_weak();
            };
            let mount = if *verb == "mount-async" {
                quote!(#children.prepare(move || {
                    #create
                    ::core::result::Result::Ok(::xrack_runtime::LoadedExternal::new(__xr_item, weak))
                }))
            } else {
                quote!({ #create #children.mount(__xr_item, weak) })
            };
            return Ok(quote!({
                #(let #captures = #values;)*
                #capture_globals
                #capture_locale
                #handle { id: #mount }
            }));
        }
        let handle = arg(0)?;
        let id = quote!((#handle).id);
        Ok(match (*verb, members) {
            ("remove", []) => quote!(#children.remove(#id)),
            ("move", []) => {
                let index = arg(1)?;
                quote!(#children.move_to(#id, #index))
            }
            ("is-mounted", []) => quote!(#children.contains(#id)),
            ("status", []) => quote!(#children.status(#id).to_owned()),
            ("get", [member]) => {
                let getter = getter("root", member, &value_type(op)?);
                let fallback = arg(1)?;
                quote!(#children.get::<#component_type>(#id).map_or_else(|| #fallback, |ui| ui.#getter()))
            }
            ("set", [member]) => {
                let setter = property_method("write", "root", member);
                let value = arg(1)?;
                quote!(#children.get::<#component_type>(#id).is_some_and(|ui| {
                    ui.#setter(#value);
                    true
                }))
            }
            ("call" | "call-value", [member]) => {
                let callback = callback_method("invoke_cb", "root", member);
                let has_value = *verb == "call-value";
                let first = if has_value { 2 } else { 1 };
                let args = (first..op.operand_count())
                    .map(arg)
                    .collect::<Result<Vec<_>, _>>()?;
                if has_value {
                    let fallback = arg(1)?;
                    quote!(#children.get::<#component_type>(#id).map_or_else(|| #fallback, |ui| ui.#callback(#(#args),*)))
                } else {
                    quote!(#children.get::<#component_type>(#id).is_some_and(|ui| {
                        ui.#callback(#(#args),*);
                        true
                    }))
                }
            }
            _ => return Err("invalid dynamic component operation".into()),
        })
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
            let receiver = self.visuals.receiver_in(context.receiver);
            return Ok(quote!(#receiver.#field.get(|| #value)));
        }
        let receiver = context.receiver;
        if op.name() == "xr.state_select" {
            let owner = op.attr("owner")?;
            if self.analysis.persistent_state_selectors.contains_key(owner) {
                let selector = state_selector_name(owner);
                return Ok(quote!(#receiver.#selector()));
            }
            return self.state_choice(op, |condition| self.expr_in(condition, context));
        }
        let value = op.attr("value")?;
        let arg = |i| self.expr_in(self.operand(op, i)?, context);
        Ok(match op.name() {
            "xr.literal" => literal(value, value_type(op)?)?,
            "xr.locale" | "xr.format_number" | "xr.parse_number" => {
                let catalog = if self.has_translation_catalog {
                    quote!(::core::option::Option::Some(__xr_catalog))
                } else {
                    quote!(::core::option::Option::None)
                };
                let binding = quote!(#receiver.__state.__locale);
                let call = match op.name() {
                    "xr.locale" => quote!(#binding.locale(#catalog)),
                    "xr.format_number" => {
                        let (value, decimals) = (arg(0)?, arg(1)?);
                        let method = if value_type(self.operand(op, 0)?)? == Type::Int {
                            format_ident!("format_number_int")
                        } else {
                            format_ident!("format_number_float")
                        };
                        quote!(#binding.#method(#catalog, #value, #decimals))
                    }
                    _ => {
                        let (text, fallback) = (arg(0)?, arg(1)?);
                        quote!(#binding.parse_number(#catalog, &(#text), #fallback))
                    }
                };
                if self.has_translation_catalog {
                    quote!(__XR_TRANSLATIONS.with(|__xr_catalog| #call))
                } else {
                    call
                }
            }
            "xr.translate" => {
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
            "xr.array" | "xr.int_array" | "xr.date_array" | "xr.record_array" => {
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
            "xr.update_record" => {
                let record = ty(&value_type(op)?);
                let update = RecordUpdateRef::from_operation(op)?;
                let fields = update
                    .fields
                    .into_iter()
                    .map(|(name, value)| {
                        let name = ident(name);
                        let value = value
                            .defining_operation()
                            .ok_or("record field has no definition")?;
                        let value = self.expr_in(value, context)?;
                        Ok(quote!(#name: #value))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let base = update
                    .base
                    .defining_operation()
                    .ok_or("record update base has no definition")?;
                let base = self.expr_in(base, context)?;
                quote!(#record { #(#fields,)* ..#base })
            }
            "xr.make_date" => {
                let (year, month, day) = (arg(0)?, arg(1)?, arg(2)?);
                quote!(::xrack_runtime::Date { year: #year, month: #month, day: #day })
            }
            "xr.date_is_valid" => {
                let date = arg(0)?;
                quote!((#date).valid())
            }
            "xr.date_weekday" => {
                let date = arg(0)?;
                quote!((#date).weekday().unwrap_or(0))
            }
            "xr.date_months" | "xr.date_month_index" => {
                let from = arg(0)?;
                let to = arg(1)?;
                if op.name() == "xr.date_months" {
                    quote!((#from).months_through(#to))
                } else {
                    let date = arg(2)?;
                    quote!((#from).month_index(#to, #date))
                }
            }
            "xr.date_parse_iso" => {
                let (text, fallback) = (arg(0)?, arg(1)?);
                quote!(::xrack_runtime::Date::parse_iso(&#text).unwrap_or_else(|| #fallback))
            }
            "xr.date_is_selectable" => {
                let (date, minimum, maximum, weekdays, dates) =
                    (arg(0)?, arg(1)?, arg(2)?, arg(3)?, arg(4)?);
                quote!((#date).is_selectable(#minimum, #maximum, &#weekdays, &#dates))
            }
            "xr.date_add_days" | "xr.date_add_months" => {
                let (date, offset, fallback) = (arg(0)?, arg(1)?, arg(2)?);
                let method = if op.name() == "xr.date_add_days" {
                    quote!(add_days)
                } else {
                    quote!(add_months_clamped)
                };
                quote!((#date).#method(#offset).unwrap_or_else(|| #fallback))
            }
            "xr.time_parse_iso" => {
                let (text, fallback) = (arg(0)?, arg(1)?);
                quote!(::xrack_runtime::Time::parse_iso(&#text).unwrap_or_else(|| #fallback))
            }
            "xr.time_is_valid_iso" => {
                let text = arg(0)?;
                quote!(::xrack_runtime::Time::parse_iso(&#text).is_some())
            }
            "xr.time_format_iso" => {
                let (time, show_seconds) = (arg(0)?, arg(1)?);
                quote!((#time).format_iso(#show_seconds))
            }
            "xr.time_in_range" => {
                let (time, minimum, maximum) = (arg(0)?, arg(1)?, arg(2)?);
                quote!((#time).in_range(#minimum, #maximum))
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
            "xr.get_field" => {
                let record = arg(0)?;
                let field = ident(value);
                quote!((#record).#field)
            }
            "xr.date_field"
            | "xr.time_field"
            | "xr.font_field"
            | "xr.key_field"
            | "xr.wheel_field"
            | "xr.pointer_field"
            | "xr.touch_point_field" => {
                let record = arg(0)?;
                let field =
                    ident(xrack_resources::field::BuiltinField::from_operation(op)?.member());
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
                quote!(#image.clone())
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
                if let Some(global) = owner.strip_prefix("__global_") {
                    let method = global_accessor(global);
                    let getter = getter("root", value, &value_type(op)?);
                    quote!(#receiver.#method().#getter())
                } else if self.instances.contains_key(owner) {
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
            "xr.scroll_control_read" if observed_scroll_read(op) => {
                let handle = geometry_handle(owner(op));
                let getter = format_ident!("scroll_{}", rust_name(value));
                quote!(#receiver.__state.#handle.#getter())
            }
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
            "xr.combo_popup_read" => {
                let handle = combo_popup_handle(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.button_pressed" => {
                let handle = button_press_handle(owner(op));
                quote!(#receiver.__state.#handle.pressed())
            }
            "xr.button_checked" => {
                let key = owner(op);
                let method = button_checked_method(key);
                if self.repeated_buttons.contains(key) {
                    let button = self.elements[key];
                    if let Some(action) = action_ref(button) {
                        let read = action_checked_method(action);
                        quote!(#receiver.#read(::core::option::Option::None))
                    } else if has_button_check_state(button) {
                        let read = row_button_binding(key, "checked");
                        quote!(#read())
                    } else {
                        quote!(false)
                    }
                } else {
                    quote!(#receiver.#method())
                }
            }
            "xr.toggle_state_read" => {
                let key = owner(op);
                let target = self.elements[key];
                let state = if context.row_toggle_owner == Some(key) {
                    quote!(__xr_read_toggle())
                } else if let Some(link) = children(target, "xr.row_link").next() {
                    let field = self.expr_in(self.operand(link, 0)?, context)?;
                    quote!(::xrack_runtime::CheckState::from(#field))
                } else {
                    let method = toggle_state_method(key);
                    quote!(#receiver.#method())
                };
                if value == "checked" {
                    quote!(((#state) == ::xrack_runtime::CheckState::Checked))
                } else {
                    state
                }
            }
            "xr.numeric_value_read" => {
                let key = owner(op);
                let control = self.elements[key];
                if let Some(link) =
                    children(control, "xr.row_link").find(|link| link.attr("name") == Ok(value))
                {
                    self.expr_in(self.operand(link, 0)?, context)?
                } else {
                    let method = numeric_value_method(key, value);
                    quote!(#receiver.#method())
                }
            }
            "xr.combo_state_read" => {
                let method = combo_state_method(owner(op), value);
                quote!(#receiver.#method())
            }
            "xr.range_position_read" => {
                let method = range_position_method(owner(op), value);
                quote!(#receiver.#method())
            }
            "xr.radio_checked" => {
                let group = self.elements[value];
                let method = radio_choice_method(value);
                if group.attr("kind") == Ok("RadioGroup") {
                    let key = owner(op);
                    let declaration = scene_children(group)
                        .position(|child| element_key(child) == Ok(key))
                        .ok_or("unknown radio option")?;
                    let row = if children(self.elements[key], "xr.repeat").next().is_some() {
                        context.row_index()
                    } else {
                        quote!(0usize)
                    };
                    quote!((#receiver.#method() == ::core::option::Option::Some((#declaration, #row))))
                } else {
                    let index = context.row_index();
                    quote!((#receiver.#method() == ::core::option::Option::Some(#index)))
                }
            }
            "xr.navigation_count" => {
                let group = self.elements[owner(op)];
                if let Some(page) = scene_children(group).next()
                    && let Some(repeat) = children(page, "xr.repeat").next()
                {
                    if self.empty_repeat(page) {
                        quote!(0_i64)
                    } else {
                        let model = self.expr_in(self.operand(repeat, 0)?, context)?;
                        let source =
                            if let Some(repeat) = self.visuals.repeats.get(element_key(page)?) {
                                let field = repeat_field(repeat.index());
                                let visuals = self.visuals.receiver_in(context.receiver);
                                quote!(&*#visuals.#field.resolve_model(|| #model))
                            } else {
                                quote!(&(#model))
                            };
                        quote!(::xrack_runtime::ListModel::row_count(#source).min(4096) as ::core::primitive::i64)
                    }
                } else {
                    let count = Literal::i64_suffixed(scene_children(group).count() as i64);
                    quote!(#count)
                }
            }
            "xr.navigation_index" => {
                let method = navigation_choice_method(owner(op));
                quote!((#receiver.#method().map_or(-1_i64, |index| index as ::core::primitive::i64)))
            }
            "xr.navigation_active" => {
                let key = owner(op);
                let item = self.elements[key];
                let group = self.elements[value];
                let method = navigation_choice_method(value);
                let index = if children(item, "xr.repeat").next().is_some() {
                    context.row_index()
                } else {
                    let slot = scene_children(group)
                        .position(|child| element_key(child) == Ok(key))
                        .ok_or("unknown navigation item")?;
                    quote!(#slot)
                };
                quote!((#receiver.#method() == ::core::option::Option::Some(#index)))
            }
            "xr.text_selection_read" | "xr.text_command_available" => {
                let handle = focus_handle(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#handle.#getter())
            }
            "xr.text_accepts" | "xr.text_validity" => {
                let getter = format_ident!(
                    "__xr_{}_{}",
                    op.name().trim_start_matches("xr."),
                    rust_key(owner(op))
                );
                quote!(#receiver.#getter())
            }
            "xr.media_read" => {
                let slot = media_slot(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#slot.#getter())
            }
            "xr.image_playback_read" => {
                let slot = playback_slot(owner(op));
                let getter = ident(value);
                quote!(#receiver.__state.#slot.#getter())
            }
            "xr.window_read" => {
                let metric = xrack_runtime::WindowMetric::parse(value)
                    .ok_or("unknown native window status property")?;
                let metric = format_ident!("{metric:?}");
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
            "xr.dynamic_action" | "xr.dynamic_read" => return self.dynamic_expression(op, context),
            "xr.loader_external_status" => {
                let slot = external_loader_slot(owner(op));
                let source = arg(0)?;
                quote!(#receiver.__state.__visuals.#slot.status(&#source))
            }
            "xr.loader_mounted" => {
                let slot = loader_slot(owner(op));
                let selection = arg(0)?;
                quote!(#receiver.__state.__visuals.#slot.is_mounted(#selection))
            }
            "xr.stack_read" => {
                let snapshot = stack_snapshot_method(owner(op));
                let snapshot = quote!(#receiver.#snapshot());
                match StackRead::from_operation(op)? {
                    StackRead::CurrentPage => quote!(#snapshot.current_page()),
                    StackRead::Depth => quote!(#snapshot.depth() as ::core::primitive::i64),
                    StackRead::CanGoBack => quote!(#snapshot.can_go_back()),
                    StackRead::PageActive(route) => quote!(#snapshot.page_active(#route)),
                    StackRead::PageIndex(route) => quote!(#snapshot.page_index(#route)),
                }
            }
            "xr.table_selection_read" | "xr.table_sort_read" | "xr.table_history_read" => {
                let key = owner(op);
                if self.trees.contains_key(key) {
                    let method = tree_selection_info_method(key);
                    let count = quote!(#receiver.#method(::core::option::Option::None).0);
                    return match TableRead::from_operation(op)? {
                        TableRead::SelectionCount => Ok(count),
                        TableRead::HasSelection => Ok(quote!(#count != 0)),
                        _ => Err("unsupported tree selection query".into()),
                    };
                }
                if self.elements.get(key).is_some_and(|view| {
                    matches!(
                        view.attr("kind"),
                        Ok("ListView" | "GridView" | "StandardListView")
                    )
                }) {
                    let method = list_selection_info_method(key);
                    return match TableRead::from_operation(op)? {
                        TableRead::SelectionCount => Ok(quote!(#receiver.#method(-1).0)),
                        TableRead::HasSelection => Ok(quote!(#receiver.#method(-1).0 != 0)),
                        _ => Err("unsupported list selection query".into()),
                    };
                }
                let snapshot = table_selection_snapshot_method(key);
                let selection = table_selection_field(key);
                let field = table_sort_field(key);
                let sort = quote!(#receiver.__state.#field.get(|| None));
                match TableRead::from_operation(op)? {
                    TableRead::CurrentColumn => {
                        quote!(#receiver.#snapshot().current_column_index())
                    }
                    TableRead::SelectionCount => quote!(#receiver.#snapshot().selection_count()),
                    TableRead::HasSelection => quote!(!#receiver.#snapshot().is_empty()),
                    TableRead::SelectionActive => quote!(
                        #receiver.__state.#selection.dragging() || #receiver.#snapshot().drag_range().is_some()
                    ),
                    TableRead::SelectionDragging => quote!(#receiver.__state.#selection.dragging()),
                    TableRead::SortColumn => {
                        quote!(::xrack_runtime::TableSort::column_index(#sort))
                    }
                    TableRead::SortDescending => {
                        quote!(::xrack_runtime::TableSort::is_descending(#sort))
                    }
                    TableRead::History { redo } => {
                        let table = *self.elements.get(key).ok_or("missing history table")?;
                        if table_history_item(table)?.is_some() {
                            let method = table_history_method(key, "available");
                            quote!(#receiver.#method(#redo))
                        } else {
                            quote!(false)
                        }
                    }
                }
            }
            "xr.list_row_selected" => {
                let method = list_selection_info_method(owner(op));
                let row = arg(0)?;
                quote!(#receiver.#method(#row).1)
            }
            "xr.tree_selection_keys" | "xr.list_selection_keys" => {
                let method = if op.name() == "xr.tree_selection_keys" {
                    tree_selection_keys_method(owner(op))
                } else {
                    list_selection_keys_method(owner(op))
                };
                quote!(#receiver.#method())
            }
            "xr.split_state_read" => {
                let save = split_save_method(owner(op));
                quote!(#receiver.#save())
            }
            "xr.split_resizing" => {
                let state = split_state_field(owner(op));
                quote!(#receiver.__state.#state.handle().resizing())
            }
            "xr.tree_query" => {
                if matches!(value, "is-row-selected" | "is-key-selected") {
                    let method = tree_selection_info_method(owner(op));
                    let lookup = if value == "is-row-selected" {
                        quote!(Row)
                    } else {
                        quote!(Key)
                    };
                    let index = arg(0)?;
                    return Ok(quote!(#receiver.#method(::core::option::Option::Some(
                        ::xrack_runtime::TreeSelectionLookup::#lookup(#index)
                    )).1));
                }
                let snapshot = tree_snapshot_method(owner(op));
                let key = arg(0)?;
                let method = match value {
                    "is-expanded" => quote!(is_expanded),
                    "depth" => quote!(query_depth),
                    "row-for-key" => quote!(query_row),
                    _ => return Err("unknown tree query".into()),
                };
                quote!({
                    let __xr_key = #key;
                    #receiver.#snapshot().#method(__xr_key)
                })
            }
            "xr.list_selection_read" => {
                let method = list_selected_index_method(owner(op));
                quote!(#receiver.#method())
            }
            "xr.view_count" => {
                let method = view_count_method(owner(op));
                let source = value == "source-row-count";
                quote!(#receiver.#method(#source))
            }
            "xr.view_index" => {
                if value == "view-index-for-key" {
                    let method = list_key_index_method(owner(op));
                    let key = arg(0)?;
                    return Ok(quote!(#receiver.#method(#key)));
                }
                let method = view_index_method(owner(op));
                let index = arg(0)?;
                let direction =
                    xrack_runtime::ViewIndex::parse(value).ok_or("unknown view index query")?;
                let direction = format_ident!("{direction:?}");
                quote! {
                    ::xrack_runtime::ViewIndex::#direction.query(#index, |index, direction| #receiver.#method(index, direction))
                }
            }
            "xr.call" => from_host(self.callback_call(op, value, context)?, &value_type(op)?),
            "xr.param" => {
                let name = parameter_ident(value);
                if context.repeat_parameters || context.combo_option && value == "option-data" {
                    quote!(#name())
                } else {
                    from_host(quote!(#name.clone()), &value_type(op)?)
                }
            }
            "xr.cast" => {
                let arg = arg(0)?;
                quote!((#arg as ::core::primitive::f64))
            }
            "xr.math_function" => {
                let kind = MathFunction::parse(value).ok_or("unknown math function")?;
                let input_type = value_type(self.operand(op, 0)?)?;
                let unwrap = matches!(input_type, Type::Length | Type::Percent);
                let operand = |index| -> Result<TokenStream, String> {
                    let operand = arg(index)?;
                    Ok(if unwrap {
                        quote!((#operand).0)
                    } else {
                        operand
                    })
                };
                let a = operand(0)?;
                let expression = match kind {
                    MathFunction::Min => {
                        let b = operand(1)?;
                        if input_type == Type::Int {
                            quote!((#a).min(#b))
                        } else {
                            quote!(::xrack_runtime::min_float(#a, #b))
                        }
                    }
                    MathFunction::Max => {
                        let b = operand(1)?;
                        if input_type == Type::Int {
                            quote!((#a).max(#b))
                        } else {
                            quote!(::xrack_runtime::max_float(#a, #b))
                        }
                    }
                    MathFunction::Clamp => {
                        let (b, c) = (operand(1)?, operand(2)?);
                        if input_type == Type::Int {
                            quote!(::xrack_runtime::clamp_int(#a, #b, #c))
                        } else {
                            quote!(::xrack_runtime::clamp_float(#a, #b, #c))
                        }
                    }
                    MathFunction::Mod => {
                        let b = operand(1)?;
                        if input_type == Type::Int {
                            quote!(::xrack_runtime::modulo_int(#a, #b))
                        } else {
                            quote!((#a).rem_euclid(#b))
                        }
                    }
                    MathFunction::Sign => quote!(::xrack_runtime::number_sign(#a)),
                    MathFunction::Sqrt => quote!((#a).sqrt()),
                    MathFunction::Pow => {
                        let b = operand(1)?;
                        quote!((#a).powf(#b))
                    }
                    MathFunction::Exp => quote!((#a).exp()),
                    MathFunction::Ln => quote!((#a).ln()),
                    MathFunction::Log => {
                        let b = operand(1)?;
                        quote!((#a).log(#b))
                    }
                    MathFunction::Sin => quote!((#a).sin()),
                    MathFunction::Cos => quote!((#a).cos()),
                    MathFunction::Tan => quote!((#a).tan()),
                    MathFunction::Asin => quote!((#a).asin()),
                    MathFunction::Acos => quote!((#a).acos()),
                    MathFunction::Atan => quote!((#a).atan()),
                    MathFunction::Atan2 => {
                        let b = operand(1)?;
                        quote!((#a).atan2(#b))
                    }
                    MathFunction::Hypot => {
                        let b = operand(1)?;
                        quote!((#a).hypot(#b))
                    }
                    MathFunction::ToRadians => quote!((#a).to_radians()),
                    MathFunction::ToDegrees => quote!((#a).to_degrees()),
                    MathFunction::IsFinite => quote!((#a).is_finite()),
                    MathFunction::Abs if input_type == Type::Int => quote!((#a).saturating_abs()),
                    MathFunction::Abs => quote!((#a).abs()),
                    MathFunction::Floor => quote!(::xrack_runtime::floor_to_int(#a)),
                    MathFunction::Ceil => quote!(::xrack_runtime::ceil_to_int(#a)),
                };
                if unwrap {
                    let wrapper = ty(&input_type);
                    quote!(#wrapper(#expression))
                } else {
                    expression
                }
            }
            "xr.string_function" => {
                let a = arg(0)?;
                match StringFunction::parse(value).ok_or("unknown string function")? {
                    StringFunction::Trim => quote!((#a).trim().to_owned()),
                    StringFunction::IsEmpty => quote!((#a).is_empty()),
                    StringFunction::Contains => {
                        let b = arg(1)?;
                        quote!((#a).contains((#b).as_str()))
                    }
                    StringFunction::StartsWith => {
                        let b = arg(1)?;
                        quote!((#a).starts_with((#b).as_str()))
                    }
                    StringFunction::EndsWith => {
                        let b = arg(1)?;
                        quote!((#a).ends_with((#b).as_str()))
                    }
                    StringFunction::ReplaceAll => {
                        let (from, to) = (arg(1)?, arg(2)?);
                        quote!((#a).replace((#from).as_str(), &(#to)))
                    }
                    StringFunction::Lowercase => quote!((#a).to_lowercase()),
                    StringFunction::Uppercase => quote!((#a).to_uppercase()),
                    StringFunction::IsInt => quote!(::xrack_runtime::parse_int(&(#a)).is_some()),
                    StringFunction::IsFloat => {
                        quote!(::xrack_runtime::parse_float(&(#a)).is_some())
                    }
                    StringFunction::ToInt | StringFunction::ToFloat => {
                        let fallback = arg(1)?;
                        let parse = if value == "to-int" {
                            quote!(::xrack_runtime::parse_int)
                        } else {
                            quote!(::xrack_runtime::parse_float)
                        };
                        quote!(#parse(&(#a)).unwrap_or(#fallback))
                    }
                    StringFunction::CharacterCount => {
                        quote!(::xrack_runtime::character_count(&(#a)))
                    }
                }
            }
            "xr.model_count" => {
                let model = arg(0)?;
                quote!((#model).len() as ::core::primitive::i64)
            }
            "xr.model_is_empty" => {
                let model = arg(0)?;
                quote!((#model).is_empty())
            }
            "xr.model_at" => {
                let (model, index, fallback) = (arg(0)?, arg(1)?, arg(2)?);
                quote!((#model).row_at(#index).unwrap_or_else(|| #fallback))
            }
            "xr.color_function" => {
                let a = arg(0)?;
                let b = arg(1)?;
                match ColorFunction::parse(value).ok_or("unknown color function")? {
                    ColorFunction::Rgb | ColorFunction::Rgba => {
                        let c = arg(2)?;
                        let alpha = if value == "rgb" { quote!(1.0) } else { arg(3)? };
                        quote!(::xrack_runtime::Color::from_rgba(#a, #b, #c, #alpha))
                    }
                    ColorFunction::WithAlpha => quote!((#a).with_alpha(#b)),
                    ColorFunction::Mix => {
                        let amount = arg(2)?;
                        quote!((#a).mix(#b, #amount))
                    }
                }
            }
            "xr.format_fixed" => {
                let number = arg(0)?;
                let precision = value
                    .parse::<usize>()
                    .map_err(|_| "invalid fixed precision")?;
                if value_type(self.operand(op, 0)?)? == Type::Int {
                    quote!(::xrack_runtime::format_fixed_int(#number, #precision))
                } else {
                    quote!(::xrack_runtime::format_fixed_float(#number, #precision))
                }
            }
            "xr.round" => {
                let operand = arg(0)?;
                quote!(::xrack_runtime::round_to_int(#operand))
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
            "xr.linear_gradient" | "xr.radial_gradient" => {
                let GradientRef { angle, stops } = GradientRef::from_operation(op)?;
                let operand = |value: ValueRef<'a>| {
                    let expression = value
                        .defining_operation()
                        .ok_or("gradient operand has no definition")?;
                    self.expr_in(expression, context)
                };
                let stops = stops
                    .into_iter()
                    .map(|(color, offset)| {
                        let (color, offset) = (operand(color)?, operand(offset)?);
                        Ok(quote!(::xrack_runtime::GradientStop { color: #color, offset: #offset }))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let brush = if let Some(angle) = angle {
                    let angle = operand(angle)?;
                    quote!(::xrack_runtime::Brush::Linear {
                        angle_degrees: #angle,
                        stops: ::std::sync::Arc::from([#(#stops),*]),
                    })
                } else {
                    quote!(::xrack_runtime::Brush::Radial {
                        stops: ::std::sync::Arc::from([#(#stops),*]),
                    })
                };
                if self.use_visual_plan
                    && let Some(group) = GradientRef::static_group(op)
                {
                    let cache = format_ident!("__XR_GRADIENT_{group}");
                    quote!(#cache.get_or_init(|| #brush).clone())
                } else {
                    brush
                }
            }
            "xr.unary" => {
                let arg = arg(0)?;
                match UnaryOperator::from_operation(op)? {
                    UnaryOperator::Positive => arg,
                    UnaryOperator::Negate if value_type(op)? == Type::Int => {
                        quote!(::core::primitive::i64::wrapping_neg(#arg))
                    }
                    UnaryOperator::Negate if value_type(op)? == Type::Length => {
                        quote!(::xrack_runtime::Length(-(#arg).0))
                    }
                    UnaryOperator::Negate => quote!((-#arg)),
                    UnaryOperator::Not => quote!((!#arg)),
                }
            }
            "xr.binary" if value_type(op)? == Type::String => {
                let (left, right) = (arg(0)?, arg(1)?);
                quote!(::std::format!("{}{}", #left, #right))
            }
            "xr.binary" if value_type(self.operand(op, 0)?)?.list_item().is_some() => {
                let (left, right) = (arg(0)?, arg(1)?);
                let equal = quote!((#left).values_eq_owned(#right));
                match BinaryOperator::from_operation(op)? {
                    BinaryOperator::Equal => equal,
                    BinaryOperator::NotEqual => quote!(!#equal),
                    _ => return Err(format!("unsupported list operator {value}")),
                }
            }
            "xr.binary" if value_type(op)? == Type::Int => {
                let (left, right) = (arg(0)?, arg(1)?);
                match BinaryOperator::from_operation(op)? {
                    BinaryOperator::Add => {
                        quote!(::core::primitive::i64::wrapping_add(#left, #right))
                    }
                    BinaryOperator::Subtract => {
                        quote!(::core::primitive::i64::wrapping_sub(#left, #right))
                    }
                    BinaryOperator::Multiply => {
                        quote!(::core::primitive::i64::wrapping_mul(#left, #right))
                    }
                    BinaryOperator::Divide => {
                        quote!(::xrack_runtime::int_divide(#left, #right))
                    }
                    BinaryOperator::Remainder => {
                        quote!(::xrack_runtime::int_remainder(#left, #right))
                    }
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
                let operator = binary_operator(BinaryOperator::from_operation(op)?);
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
        if let Some(global) = owner.strip_prefix("__global_") {
            let global = global_accessor(global);
            let method = property_method(action, "root", name);
            quote!(#receiver.#global().#method(#args);)
        } else if self.instances.contains_key(owner) {
            let instance = instance_field(owner);
            let method = property_method(action, "root", name);
            quote!(#receiver.__state.#instance.#method(#args);)
        } else {
            let method = property_method(action, owner, name);
            quote!(#receiver.#method(#args);)
        }
    }

    fn forwarded_input(&self, endpoint: PropertyEndpoint<'a>) -> Option<TokenStream> {
        let forward = self.analysis.property_forwards.get(&[endpoint][..])?;
        Some(Self::take_forwarded_inputs(&forward.inputs))
    }

    fn take_forwarded_inputs(names: &[&str]) -> TokenStream {
        let mut fields = names.iter().map(|name| property_field("root", name));
        #[expect(
            clippy::expect_used,
            reason = "a property forward is recorded only for a nonempty alias group"
        )]
        let first = fields.next().expect("a forward has at least one alias");
        quote!(__inputs.#first.take() #(.or_else(|| __inputs.#fields.take()))*)
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
        if let Some(global) = owner.strip_prefix("__global_") {
            let global = global_accessor(global);
            let method = callback_method(action, "root", name);
            quote!(#receiver.#global().#method(#args))
        } else if self.instances.contains_key(owner) {
            let field = instance_field(owner);
            let method = callback_method(action, "root", name);
            quote!(#receiver.__state.#field.#method(#args))
        } else {
            let method = callback_method(action, owner, name);
            quote!(#receiver.#method(#args))
        }
    }

    fn statements(&self, handler: OperationRef<'a>) -> Result<TokenStream, String> {
        self.statements_in(handler, ExpressionContext::new(&quote!(self)))
    }

    fn statements_in(
        &self,
        handler: OperationRef<'a>,
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        let expression = |op| self.expr_in(op, context);
        let mut statements = TokenStream::new();
        for statement in handler.children() {
            match statement.name() {
                "xr.assign" => {
                    let value = expression(self.operand(statement, 0)?)?;
                    statements.extend(self.set_property(
                        owner(statement),
                        statement.attr("name")?,
                        &value,
                    ));
                }
                "xr.evaluate" => {
                    let value = expression(self.operand(statement, 0)?)?;
                    statements.extend(quote!(let _ = #value;));
                }
                "xr.invoke" => {
                    let call = self.callback_call(statement, statement.attr("name")?, context)?;
                    statements.extend(quote!(let _ = #call;));
                }
                "xr.log" => {
                    let level = expression(self.operand(statement, 0)?)?;
                    let message = expression(self.operand(statement, 1)?)?;
                    let component = self.component_name;
                    let file = statement.source_file().unwrap_or("<MLIR>");
                    let xrack_syntax::ast::Span { line, column, .. } = statement.span();
                    statements.extend(quote!(
                        ::xrack_runtime::log_ui(
                            #level,
                            &#message, #component, #file, #line, #column,
                        );
                    ));
                }
                "xr.navigate" => {
                    let method = stack_navigate_method(owner(statement));
                    let operation =
                        xrack_runtime::StackNavigationMethod::parse(statement.attr("method")?)
                            .ok_or("unknown StackView navigation method")?;
                    let variant = format_ident!("{operation:?}");
                    let argument = if operation == xrack_runtime::StackNavigationMethod::Pop {
                        quote!(::xrack_runtime::StackNavigationArgument::None)
                    } else {
                        let value = expression(self.operand(statement, 0)?)?;
                        if operation.uses_path() {
                            quote!(::xrack_runtime::StackNavigationArgument::Path(&(#value).snapshot()))
                        } else {
                            quote!(::xrack_runtime::StackNavigationArgument::Route(&(#value)))
                        }
                    };
                    statements.extend(quote! {
                        if let ::core::option::Option::Some(__xr_navigation) =
                            ::xrack_runtime::StackNavigationMethod::#variant.action(#argument)
                        {
                            let _ = self.#method(::core::option::Option::None, __xr_navigation, ::core::option::Option::None);
                        }
                    });
                }
                "xr.media_command" => {
                    let slot = media_slot(owner(statement));
                    use xrack_resources::visual::MediaEffect;
                    let command = match MediaEffect::from_operation(statement)? {
                        MediaEffect::Play => quote!(::xrack_runtime::MediaCommand::Play),
                        MediaEffect::Pause => quote!(::xrack_runtime::MediaCommand::Pause),
                        MediaEffect::Stop => quote!(::xrack_runtime::MediaCommand::Stop),
                        MediaEffect::GoLive => quote!(::xrack_runtime::MediaCommand::GoLive),
                        MediaEffect::Seek(position) => {
                            let position = position
                                .defining_operation()
                                .ok_or("missing seek operand definition")?;
                            let position = expression(position)?;
                            quote!(::xrack_runtime::MediaCommand::Seek(#position))
                        }
                    };
                    statements.extend(quote!(self.__state.#slot.command(#command);));
                }
                "xr.timer_command" => {
                    let method = timer_member(statement.attr("owner")?, "command");
                    let command = xrack_runtime::TimerCommand::parse(statement.attr("command")?)
                        .ok_or("unknown timer command")?;
                    let command = format_ident!("{command:?}");
                    statements
                        .extend(quote!(self.#method(::xrack_runtime::TimerCommand::#command);));
                }
                "xr.button_click" => {
                    let handle = button_press_handle(statement.attr("owner")?);
                    let method = if statement.attribute("animated") == Some("true") {
                        quote!(try_animate_click)
                    } else {
                        quote!(try_click)
                    };
                    statements.extend(quote! {
                        let _ = self.__state.#handle.#method(
                            || self.__render_shared(),
                            |scene, path, input| ::core::result::Result::Ok::<_, ::core::convert::Infallible>(
                                self.__route_input(scene, path, input),
                            ),
                        );
                    });
                }
                "xr.button_toggle" => {
                    let target = *self
                        .elements
                        .get(statement.attr("owner")?)
                        .ok_or("unknown button toggle target")?;
                    statements.extend(self.toggle_button(target)?);
                }
                "xr.popup_command" => {
                    let method = popup_method(statement.attr("owner")?);
                    let open = statement.attr("open")? == "true";
                    statements.extend(quote!(let _ = self.#method(#open, None);));
                }
                "xr.file_dialog_open" | "xr.native_dialog_open" => {
                    let open = native_dialog_member(statement.attr("owner")?, "open");
                    statements.extend(quote!(self.#open();));
                }
                "xr.window_close" => {
                    statements.extend(quote!(self.__state.__window.request_close();));
                }
                "xr.window_command" => {
                    let method =
                        xrack_runtime::WindowCommandMethod::parse(statement.attr("command")?)
                            .ok_or("unknown native window command")?;
                    let variant = format_ident!("{method:?}");
                    let enabled = if method == xrack_runtime::WindowCommandMethod::SetFullScreen {
                        let enabled = expression(self.operand(statement, 0)?)?;
                        quote!(::core::option::Option::Some(#enabled))
                    } else {
                        quote!(::core::option::Option::None)
                    };
                    statements.extend(quote! {
                        if let ::core::option::Option::Some(__xr_command) =
                            ::xrack_runtime::WindowCommandMethod::#variant.action(#enabled)
                        {
                            self.__state.__window.request(__xr_command);
                        }
                    });
                }
                "xr.check_group_command" => {
                    let set = check_group_method(statement.attr("owner")?, "set");
                    let state = expression(self.operand(statement, 0)?)?;
                    statements.extend(quote!(self.#set(#state);));
                }
                "xr.focus_command" | "xr.text_command" => {
                    let (target, method) = if statement.name() == "xr.focus_command" {
                        (statement.attr("target")?, statement.attr("action")?)
                    } else {
                        (statement.attr("owner")?, statement.attr("method")?)
                    };
                    let handle = focus_handle(target);
                    let command = xrack_runtime::FocusCommandMethod::parse(method)
                        .ok_or("unknown focus command")?;
                    let variant = format_ident!("{command:?}");
                    let mut operands = (0..statement.operand_count())
                        .map(|index| expression(self.operand(statement, index)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    let (action, request) = if command.edits_text() {
                        let text = if command.uses_text_operand() {
                            operands.pop().ok_or("missing text operand")?
                        } else {
                            quote!(::std::string::String::new())
                        };
                        (
                            quote!(::xrack_runtime::FocusCommandMethod::#variant.replacement(&[#(#operands),*], #text)),
                            quote!(request_text_edit),
                        )
                    } else {
                        (
                            quote!(::xrack_runtime::FocusCommandMethod::#variant.action(&[#(#operands),*])),
                            quote!(request),
                        )
                    };
                    statements.extend(quote! {
                        if let ::core::option::Option::Some(__xr_action) = #action {
                            self.__state.#handle.#request(__xr_action);
                        }
                    });
                }
                "xr.swipe_cancel" => {
                    let handle = swipe_handle(statement.attr("owner")?);
                    statements.extend(quote!(self.__state.#handle.cancel();));
                }
                "xr.position_child" => {
                    let viewport = viewport_handle(statement.attr("owner")?);
                    let target = geometry_handle(statement.attr("target")?);
                    let horizontal = expression(self.operand(statement, 0)?)?;
                    let vertical = expression(self.operand(statement, 1)?)?;
                    statements.extend(quote! {
                        self.__state.#viewport.request(::xrack_runtime::ScrollAction::ToChild {
                            target: self.__state.#target.scroll_target_id(),
                            horizontal: #horizontal,
                            vertical: #vertical,
                        });
                    });
                }
                "xr.scroll_command" | "xr.cancel_flick" | "xr.return_to_bounds" => {
                    let owner = statement.attr("owner")?;
                    let method = match statement.name() {
                        "xr.cancel_flick" => "cancel-flick",
                        "xr.return_to_bounds" => "return-to-bounds",
                        _ => statement.attr("method")?,
                    };
                    let method = xrack_runtime::ScrollCommandMethod::parse(method)
                        .ok_or("unknown scroll command")?;
                    if let Some(direction) = method.bar_step() {
                        let bar = *self.elements.get(owner).ok_or("unknown scroll bar")?;
                        statements.extend(self.scroll_bar_step(bar, direction)?);
                        continue;
                    }
                    let handle = viewport_handle(owner);
                    let variant = format_ident!("{method:?}");
                    let operands = (0..statement.operand_count())
                        .map(|index| expression(self.operand(statement, index)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    let resolve_key = if method.uses_source_row() {
                        let source = self.view_source_model(
                            *self.elements.get(owner).ok_or("unknown source view")?,
                        )?;
                        quote!(|index| ::xrack_runtime::ListModel::key_at(&*#source, index))
                    } else {
                        quote!(|_| ::core::option::Option::None)
                    };
                    statements.extend(quote! {
                        if let ::core::option::Option::Some(__xr_action) =
                            ::xrack_runtime::ScrollCommandMethod::#variant
                                .command(&[#(::xrack_runtime::Value::from(#operands)),*])
                                .and_then(|command| command.resolve(#resolve_key))
                        {
                            self.__state.#handle.request(__xr_action);
                        }
                    });
                }
                "xr.source_selection" => {
                    let owner = statement.attr("owner")?;
                    let operation = statement.attr("method")?;
                    let method = source_selection_method(owner);
                    let source = expression(self.operand(statement, 0)?)?;
                    let column = if operation == "select-source-cell" {
                        let column = expression(self.operand(statement, 1)?)?;
                        quote!(Some(#column))
                    } else {
                        quote!(None)
                    };
                    if operation == "select-keys" {
                        let select = if self.trees.contains_key(owner) {
                            tree_replace_selection_method(owner)
                        } else {
                            list_replace_selection_method(owner)
                        };
                        statements.extend(quote!(self.#select(#source);));
                    } else if operation == "select-key" {
                        if self.trees.contains_key(owner) {
                            let select = tree_selection_method(owner);
                            statements.extend(quote! {
                                self.#select(::xrack_runtime::TreeSelectionCommand::Key(#source));
                            });
                        } else {
                            let select = list_select_key_method(owner);
                            statements.extend(quote!(self.#select(#source);));
                        }
                    } else {
                        statements.extend(quote!(self.#method(#source, #column);));
                    }
                }
                "xr.model_edit" => {
                    let method =
                        xrack_syntax::builtins::ModelEditKind::parse(statement.attr("method")?)
                            .ok_or("invalid model edit method")?;
                    let variant = format_ident!(
                        "{}",
                        match method {
                            xrack_syntax::builtins::ModelEditKind::Append => "Append",
                            xrack_syntax::builtins::ModelEditKind::Insert => "Insert",
                            xrack_syntax::builtins::ModelEditKind::Set => "Set",
                            xrack_syntax::builtins::ModelEditKind::Remove => "Remove",
                            xrack_syntax::builtins::ModelEditKind::Move => "Move",
                            xrack_syntax::builtins::ModelEditKind::Clear => "Clear",
                        }
                    );
                    let model = expression(self.operand(statement, 0)?)?;
                    let arguments = (1..statement.operand_count())
                        .map(|index| expression(self.operand(statement, index)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    let edit = if arguments.is_empty() {
                        quote!(::xrack_runtime::ModelEdit::#variant)
                    } else {
                        quote!(::xrack_runtime::ModelEdit::#variant(#(#arguments),*))
                    };
                    statements.extend(quote! {
                        {
                            let model = #model;
                            #edit.apply(&model);
                        }
                    });
                }
                "xr.move_navigation_item" => {
                    let key = statement.attr("owner")?;
                    let group = *self.elements.get(key).ok_or("unknown navigation control")?;
                    let model = self
                        .group_option_model(group, &self.visuals)?
                        .ok_or("missing navigation model for move-item")?;
                    let source = expression(self.operand(statement, 0)?)?;
                    let destination = expression(self.operand(statement, 1)?)?;
                    let commit = self.navigation_model_move(group, quote!(true))?;
                    statements.extend(quote! {
                        if let (::core::result::Result::Ok(from), ::core::result::Result::Ok(to)) =
                            (::core::primitive::usize::try_from(#source), ::core::primitive::usize::try_from(#destination))
                        {
                            let model = #model;
                            let _ = #commit;
                        }
                    });
                }
                "xr.move_column" | "xr.move_row" => {
                    let owner = statement.attr("owner")?;
                    let method = if statement.name() == "xr.move_row" {
                        table_move_row_method(owner)
                    } else {
                        table_move_method(owner)
                    };
                    let from = expression(self.operand(statement, 0)?)?;
                    let to = expression(self.operand(statement, 1)?)?;
                    if statement.name() == "xr.move_row" {
                        statements.extend(quote!(self.#method(#from, #to, None);));
                    } else {
                        statements.extend(quote!(self.#method(#from, #to);));
                    }
                }
                "xr.table_selection_command" => {
                    let owner = owner(statement);
                    let tree = self.trees.contains_key(owner);
                    let list = self.elements.get(owner).is_some_and(|view| {
                        matches!(
                            view.attr("kind"),
                            Ok("ListView" | "GridView" | "StandardListView")
                        )
                    });
                    let method = if tree {
                        tree_selection_method(owner)
                    } else if list {
                        list_selection_command_method(owner)
                    } else {
                        table_selection_method(owner)
                    };
                    let action = if tree {
                        quote!(::xrack_runtime::TreeSelectionCommand::Visual(action))
                    } else if list {
                        quote!(::xrack_runtime::ListSelectionCommand::Visual(action))
                    } else {
                        quote!(action)
                    };
                    let operation = statement.attr("method")?;
                    let selection = xrack_runtime::TableSelectionMethod::parse(operation)
                        .ok_or("unknown table selection command")?;
                    let variant = format_ident!("{selection:?}");
                    let values = (0..statement.operand_count())
                        .map(|index| expression(self.operand(statement, index)?))
                        .collect::<Result<Vec<_>, _>>()?;
                    statements.extend(quote! {
                        if let Some(action) = ::xrack_runtime::TableSelectionMethod::#variant.action(&[#(#values),*]) {
                            let _ = self.#method(#action);
                        }
                    });
                }
                "xr.table_history_command" => {
                    let key = owner(statement);
                    let table = *self.elements.get(key).ok_or("missing history table")?;
                    if table_history_item(table)?.is_some() {
                        let method = table_history_method(key, "replay");
                        let redo = statement.attr("method")? == "redo";
                        statements.extend(quote!(let _ = self.#method(#redo);));
                    }
                }
                "xr.table_sort_command" => {
                    let method = table_sort_method(owner(statement));
                    match statement.attr("method")? {
                        "clear-sort" => {
                            statements.extend(quote!(let _ = self.#method(None, None);))
                        }
                        "sort-by" => {
                            let column = expression(self.operand(statement, 0)?)?;
                            let descending = expression(self.operand(statement, 1)?)?;
                            statements.extend(quote! {
                                {
                                    if let Some(sort) = ::xrack_runtime::TableSort::from_index(#column, #descending) {
                                        let _ = self.#method(Some(sort), None);
                                    }
                                }
                            });
                        }
                        _ => return Err("unknown table sort command".into()),
                    }
                }
                "xr.split_restore" => {
                    let restore = split_restore_method(owner(statement));
                    let state = expression(self.operand(statement, 0)?)?;
                    statements.extend(quote!(let _ = self.#restore(&(#state));));
                }
                "xr.tree_command" => {
                    let method = tree_command_method(owner(statement));
                    let key = expression(self.operand(statement, 0)?)?;
                    let command =
                        xrack_runtime::TreeCommandMethod::parse(statement.attr("method")?)
                            .ok_or("unknown tree command")?;
                    let variant = format_ident!("{command:?}");
                    let depth = if command.requires_depth() {
                        let depth = expression(self.operand(statement, 1)?)?;
                        quote!(::core::option::Option::Some(#depth))
                    } else {
                        quote!(::core::option::Option::None)
                    };
                    statements.extend(quote! {
                        if let ::core::option::Option::Some(action) =
                            ::xrack_runtime::TreeCommandMethod::#variant.action(#key, #depth)
                        {
                            self.#method(action);
                        }
                    });
                }
                "xr.set_row_height" => {
                    let method = table_row_height_method(statement.attr("owner")?);
                    let row = expression(self.operand(statement, 0)?)?;
                    let height = expression(self.operand(statement, 1)?)?;
                    statements.extend(quote!(self.#method(#row, #height);));
                }
                "xr.secondary_window_command" => {
                    let method = secondary_window_member(statement.attr("owner")?, "command");
                    let command =
                        xrack_runtime::SecondaryWindowCommand::parse(statement.attr("command")?)
                            .ok_or("unknown secondary window command")?;
                    let variant = format_ident!("{command:?}");
                    statements.extend(
                        quote!(self.#method(::xrack_runtime::SecondaryWindowCommand::#variant);),
                    );
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
            let name = self.render_element(
                element,
                &VisualScope::default(),
                &mut body,
                next,
                RenderContent::Inline,
            )?;
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
            let positioned = repeat_is_positioned(element);
            if repeat.uses_row_identity() {
                let factory = self.repeat_delegate_factory(repeat, model, None)?;
                let scene = if let Some((owner, candidate)) = repeat.loader {
                    let slot = loader_slot(owner);
                    let storage = repeat.storage_type(&rust_name(self.component_name));
                    quote!({
                        let __xr_slot = &#receiver.#slot;
                        let __xr_mode = #expression;
                        if __xr_mode > 0 {
                            __xr_slot.render::<#storage>(
                                #candidate,
                                __xr_mode == 2,
                                |__xr_repeater| {
                                    __xr_repeater.render_range(
                                        || 1,
                                        0..1,
                                        #factory,
                                        |_, _, _, _, _| ::xrack_runtime::Node::hidden(),
                                    );
                                },
                                |__xr_repeater| {
                                    ::xrack_runtime::loader_candidate_scene(
                                        __xr_repeater.render_range(|| 1, 0..1, #factory, #row)
                                    )
                                },
                            )
                        } else {
                            __xr_slot.clear(#candidate);
                            ::xrack_runtime::Node::hidden()
                        }
                    })
                } else {
                    let rows = quote!(#receiver.#field.render_range(|| #expression, 0..4096, #factory, #row));
                    quote!(::xrack_runtime::repeat_scene(#rows, #positioned))
                };
                out.extend(quote!(let #name = #scene;));
                return Ok(name);
            }
            let render = if positioned {
                quote!(render_positioned)
            } else {
                quote!(render)
            };
            out.extend(quote! {
                let #name = #receiver.#field.#render(|| #expression, #row);
            });
            Ok(name)
        } else {
            self.render_element(element, scope, out, next, RenderContent::Inline)
        }
    }

    fn repeat_delegate_factory(
        &self,
        repeat: &RepeatScope<'_>,
        model: OperationRef<'a>,
        table: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        if repeat.delegates.is_empty() {
            return Ok(quote!(|_| ()));
        }
        let mut instances = TokenStream::new();
        let locale = self
            .with_locale
            .then(|| quote!(__locale: __locale.clone(),));
        let globals = (!self.globals.is_empty()).then(|| quote!(__globals: __globals.clone(),));
        let init_globals = (!self.globals.is_empty())
            .then(|| quote!(let __globals = self.__state.__globals.clone();));
        let capture_globals =
            (!self.globals.is_empty()).then(|| quote!(let __globals = __globals.clone();));
        let init_locale = self
            .with_locale
            .then(|| quote!(let __locale = self.__state.__locale.clone();));
        let capture_locale = self
            .with_locale
            .then(|| quote!(let __locale = __locale.clone();));
        for (_, delegate) in &repeat.delegates {
            let component = delegate.attr("component")?;
            let name = ident(component);
            let inputs = format_ident!("__Inputs_{}", rust_name(component));
            let bindings = self.instance_inputs(*delegate, Some(model), table)?;
            let instance = quote!(#name::__new(#inputs { #bindings #locale #globals ..::core::default::Default::default() }));
            if repeat.table {
                instances.extend(quote!({
                    let __context = __context.clone();
                    let __parent = __parent.clone();
                    #capture_locale
                    #capture_globals
                    ::std::cell::LazyCell::new(
                        ::std::boxed::Box::new(move || #instance) as ::std::boxed::Box<dyn FnOnce() -> #name>
                    )
                },));
            } else {
                instances.extend(quote!(#instance,));
            }
        }
        Ok(quote!(|__context| {
            #init_locale
            #init_globals
            let __parent = ::std::rc::Rc::downgrade(&self.__state);
            (#instances)
        }))
    }

    fn repeat_row(
        &self,
        element: OperationRef<'a>,
        repeat: &RepeatScope<'_>,
        model: OperationRef<'a>,
        next: &mut usize,
    ) -> Result<TokenStream, String> {
        let parameters = RepeatRef::from_operation(model)?;
        let item = parameter_ident(parameters.item);
        let row_index = repeat.uses_row_identity().then(|| quote!(__xr_row_index,));
        let index_binding = parameters.index.map(|index| {
            let index = parameter_ident(index);
            if row_index.is_some() {
                quote!(let #index = __xr_row_index;)
            } else {
                quote!(let #index = || __xr_index as ::core::primitive::i64;)
            }
        });
        let row_local = row_local(repeat.index());
        let delegate = repeat.uses_row_identity().then(|| quote!(__xr_delegates,));
        let track_columns = (repeat.table && !repeat.uses_row_identity())
            .then(|| quote!(__xr_columns_changed.track();));
        let checked = self.row_button_reads(&repeat.row)?;
        let mut body = TokenStream::new();
        let child =
            self.render_element(element, &repeat.row, &mut body, next, RenderContent::Inline)?;
        if parameters.item == xrack_ir::semantic::LOADER_REPEAT_ITEM {
            body.extend(quote! {
                #child.set_loader_mount(__xr_delegates.0.__state.__binding_lifetime.loader_mount().clone());
            });
        }
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
            quote!(|#delegate __xr_index, #item, #row_index #row_local| { #track_columns #index_binding #checked #body #child }),
        )
    }

    fn row_button_reads(&self, scope: &VisualScope<'_>) -> Result<TokenStream, String> {
        let mut reads = TokenStream::new();
        for (&key, &button) in &scope.row_checks {
            if !self.button_checked_targets.contains(key) || action_ref(button).is_some() {
                continue;
            }
            let get = row_button_binding(key, "checked");
            let value = self.control_value_in(button, "checked", Type::Bool, scope)?;
            reads.extend(quote!(let #get = || #value;));
        }
        Ok(reads)
    }

    fn row_button_toggles(&self, scope: &VisualScope<'_>) -> Result<TokenStream, String> {
        let mut toggles = TokenStream::new();
        for (&key, &button) in &scope.row_checks {
            if button.attribute("toggle_storage") != Some("true") || action_ref(button).is_some() {
                continue;
            }
            let toggle = row_button_binding(key, "toggle");
            let current = self.control_value_in(button, "checked", Type::Bool, scope)?;
            let update = self.set_checked(button, scope)?;
            toggles.extend(quote! {
                let #toggle = || {
                    let __xr_checked = !(#current);
                    #update
                };
            });
        }
        Ok(toggles)
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

    fn tree_projection(
        &self,
        element: OperationRef<'a>,
        model: &TokenStream,
    ) -> Result<TokenStream, String> {
        let element_key = element_key(element)?;
        let definition = &self.trees[element_key];
        let key = field_access(definition.key.iter().copied());
        let parent = field_access(definition.parent.iter().copied());
        let state = tree_state_field(element_key);
        let root = self.binding_value_or(element, "root-key", quote!(-1i64))?;
        let record = ident(&definition.record);
        let visible = definition
            .visible_field
            .as_ref()
            .map(|field| {
                let field = field_access(field.split('.'));
                quote!(Some(|item: &#record| item #field))
            })
            .unwrap_or_else(|| quote!(None));
        let field = definition
            .filter_field
            .as_ref()
            .map(|field| {
                let field = field_access(field.split('.'));
                quote!(Some(|item: &#record| (item #field).as_str()))
            })
            .unwrap_or_else(|| quote!(None));
        let query =
            self.binding_value_or(element, "filter-text", quote!(::std::string::String::new()))?;
        Ok(quote! {
                self.__state.#state.project_model(
                    || #model,
                    |item| (item #key, item #parent),
                    || #root,
                    #visible,
                    #field,
                    || #query,
                )
        })
    }

    fn list_row_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        if element.attr("kind") == Ok("StandardTableView") {
            let model = self.table_rows_model(element)?;
            let table = self.standard_table(element)?;
            let count = table.columns.len();
            let header = if let Some(name) = &table.row_header {
                let field = field_access(name.split('.'));
                quote!(::core::option::Option::Some(::xrack_runtime::TableField::cell_text(&item #field)))
            } else {
                quote!(::core::option::Option::None)
            };
            let editors = self.standard_table_editors(element)?;
            let mut column_editors = vec![quote!(None); count];
            for StandardTableEditor {
                column,
                enabled,
                ty: field_type,
                ..
            } in editors
            {
                let field_type = ty(&field_type);
                column_editors[column] = quote!((#enabled).then_some(<#field_type as ::xrack_runtime::TableField>::EDITOR));
            }
            let cells = table
                .columns
                .iter()
                .enumerate()
                .map(|(column, definition)| {
                    let field = field_access(definition.field.split('.'));
                    quote!(#column => ::xrack_runtime::TableField::cell_text(&item #field),)
                });
            let editors = column_editors
                .iter()
                .enumerate()
                .map(|(column, editor)| quote!(#column => #editor,));
            return Ok(quote! {
                ::xrack_runtime::render_standard_table_rows(
                    || #model, __range, __xr_columns, __xr_scene.and_then(|node| node.standard_list_stamp), #count, |item| #header,
                    |item, column| match column { #(#cells)* _ => ::core::unreachable!() },
                    |column| match column { #(#editors)* _ => ::core::option::Option::None },
                )
            });
        } else if element.attr("kind") == Ok("StandardListView") {
            if let Some(binding) = model_binding(element) {
                let (model, rows, stamp) = if has_list_visibility(element) {
                    let visible = visible_list_model(element_key(element)?);
                    (
                        quote!(self.#visible()),
                        quote!(standard_list_rows_in_view),
                        quote!(model.positions().stamp()),
                    )
                } else {
                    (
                        self.visual_value(binding, &self.visuals)?,
                        quote!(standard_list_rows),
                        quote!(::xrack_runtime::ListModel::structure_stamp(&model)),
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
                    let model = #model;
                    if !::xrack_runtime::ListMetrics::matches_scene_order(__xr_scene, Some(#stamp)) {
                        return ::std::vec::Vec::new();
                    }
                    ::xrack_runtime::#rows(&model, __range, #label)
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
            let create = if repeat.uses_row_identity() {
                let table = (element.attr("kind") == Ok("TableView")).then_some(element);
                let factory = self.repeat_delegate_factory(repeat, model, table)?;
                quote!(#factory,)
            } else {
                TokenStream::new()
            };
            let cache = self
                .use_visual_plan
                .then(|| RowSceneCache::from_operation(row))
                .flatten()
                .filter(|_| !repeat.delegates.is_empty());
            let reads_index = cache == Some(RowSceneCache::Indexed);
            let render_resolved = |model: TokenStream, indices: TokenStream| {
                if !repeat.uses_row_identity() {
                    quote!(repeater.render_resolved_indices(#model, #indices, #row_expr))
                } else {
                    let cache = if cache.is_some() {
                        quote!(Some(#reads_index))
                    } else {
                        quote!(None)
                    };
                    quote!(repeater.render_resolved_indices(#model, #indices, #cache, #create #row_expr))
                }
            };
            if element.attr("kind") == Ok("TreeView") {
                let tree =
                    self.tree_projection(element, &quote!(repeater.resolve_model(|| #model_expr)))?;
                let rows = render_resolved(
                    quote!(&__xr_model),
                    quote!(rows.iter().map(|row| row.model_index)),
                );
                return Ok(quote! {{
                        let repeater = &self.__state.__visuals.#field;
                        let (tree, __xr_model) = #tree;
                        if !tree.matches_scene_order(__xr_scene) { return ::std::vec::Vec::new(); }
                        let rows = tree.rows_in_range(__range);
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
                let track_columns = (repeat.table && !repeat.uses_row_identity()).then(
                    || quote!(let __xr_columns_changed = repeater.column_window(__xr_columns);),
                );
                let reordered = render_resolved(
                    quote!(rows.source_model()),
                    quote!(__range.filter_map(|row| positions.source_index(row))),
                );
                let range = render_resolved(quote!(rows.source_model()), quote!(__range));
                return Ok(quote! {{
                        let repeater = &self.__state.__visuals.#field;
                        #track_columns
                        let rows = #rows;
                        let positions = rows.positions();
                        if !::xrack_runtime::ListMetrics::matches_scene_order(__xr_scene, Some(positions.stamp())) {
                            return ::std::vec::Vec::new();
                        }
                        if positions.reordered() {
                            #reordered
                        } else {
                            #range
                        }
                }});
            }
            let render_range = render_resolved(quote!(&model), quote!(__range));
            return Ok(quote! {
                let repeater = &self.__state.__visuals.#field;
                let model = repeater.resolve_model(|| #model_expr);
                if !::xrack_runtime::ListMetrics::matches_scene_order(
                    __xr_scene, Some(::xrack_runtime::ListModel::structure_stamp(&*model)),
                ) {
                    return ::std::vec::Vec::new();
                }
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
        if let Some(slot) = empty_state(element) {
            let mut slot_events = TokenStream::new();
            let mut slot_delegates = TokenStream::new();
            path.push(0);
            self.delegate_event_arms(slot, path, &mut slot_events, &mut slot_delegates)?;
            path.pop();
            let active = quote!(scene.at_path(&[#(#path,)* 0]).is_some_and(|node| node.kind == "EmptyState"));
            out.extend(quote! {
                [#(#path,)* 0, ..] if #active => match path { #slot_events _ => None },
            });
            delegates.extend(quote! {
                [#(#path,)* 0, ..] if #active => match path { #slot_delegates _ => None },
            });
        }
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            let component = ident(element.attr("component")?);
            delegates.extend(quote! {
                [#(#path,)* rest @ ..] => Some(scene.at_path(&[#(#path),*]).map_or_else(
                    ::core::default::Default::default,
                    |child| {
                        let mut ui = #component { __state: self.__state.#field.__state.clone() };
                        visit(&mut ui, child, rest, input)
                    },
                )),
            });
        } else if let Some((page, model)) = repeated_navigation_page(element) {
            if !self.empty_repeat(page) {
                self.navigation_page_delegate_arms(page, model, path, delegates)?;
            }
        } else if element.attr("kind") == Ok("StandardTableView") {
            let editors = self.standard_table_editors(element)?;
            if editors.is_empty() {
                return Ok(());
            }
            let model = self.editable_table_rows_model(element, &quote!(self), path.is_empty())?;
            let record_history = if table_history_item(element)?.is_some() {
                let enabled = table_history_method(element_key(element)?, "enabled");
                quote!(self.#enabled())
            } else {
                quote!(false)
            };
            let writes = editors.iter().map(|StandardTableEditor { column, field, enabled, ty: field_type }| {
                let field_path: Vec<_> = field.split('.').collect();
                let field = field_access(field_path.iter().copied());
                let matches = quote!(|record| ::xrack_runtime::TableField::matches_cell_text(&record #field, original));
                let update = quote!(|record| {
                        let mut updated = record.clone();
                        updated #field = next;
                        Some(updated)
                });
                let commit = self.table_cell_commit(
                    element, quote!(target.rows.source()), field_path, field_type.clone(), update,
                    Some(quote!(record_history)),
                )?;
                let field_type = ty(field_type);
                Ok(quote! {
                    #column => {
                        let Some((target, (next, record_history))) = ::xrack_runtime::table_cell_edit_target_matching(
                            view, *slot, || #model, #matches,
                            || input.available(|| #enabled),
                            || input.prepare(
                                || true,
                                || {
                                    let next = <#field_type as ::xrack_runtime::TableField>::parse_cell(text)?;
                                    Some((next, #record_history))
                                },
                            ),
                        ) else {
                            return Some(false);
                        };
                        if !input.is_current() { return Some(false); }
                        let index = target.index;
                        let key = target.key;
                        #commit
                    },
                })
            }).collect::<Result<TokenStream, String>>()?;
            out.extend(quote! {
                [#(#path,)* slot, 0, column] => {
                    let ::xrack_runtime::RoutedEvent::CommitCell { original, text } = action else { return Some(false); };
                    let view = scene.at_path(&[#(#path),*])?;
                    Some(match *column { #writes _ => false })
                }
            });
        } else if matches!(
            element.attr("kind"),
            Ok("ListView" | "GridView" | "TableView" | "TreeView")
        ) {
            let (row, model) = self.list_template(element)?;
            let repeat = &self.visuals.repeats[element_key(row)?];
            let field = repeat_field(repeat.index());
            let model_expr = self.expr(self.operand(model, 0)?)?;
            let resolved_model = quote!(repeater.resolve_model(|| #model_expr));
            let projected_target = match element.attr("kind")? {
                "TreeView" => {
                    let project = tree_projection_method(element_key(element)?);
                    quote! {
                        let tree = view.tree.as_ref()?;
                        let (live, model) = self.#project();
                        (tree.event_source_index(&live, index, key)?, model)
                    }
                }
                kind if kind == "TableView" || has_list_visibility(element) => {
                    let rows = if kind == "TableView" {
                        self.table_rows_model(element)?
                    } else {
                        let visible = visible_list_model(element_key(element)?);
                        quote!(self.#visible())
                    };
                    quote! {
                        let rows = #rows;
                        (rows.positions().event_source_index(view.standard_list_stamp, index)?, rows.source_model().clone())
                    }
                }
                _ => quote!((index, #resolved_model)),
            };
            if row.attribute("delegate").is_some() {
                return self.item_delegate_events(row, model, &projected_target, path, out);
            }
            for (delegate_slot, (delegate_path, delegate)) in repeat.delegates.iter().enumerate() {
                let row_state = row_local(repeat.index());
                let delegate_slot = Literal::usize_unsuffixed(delegate_slot);
                let component = ident(delegate.attr("component")?);
                let parameters = RepeatRef::from_operation(model)?;
                let item = parameter_ident(parameters.item);
                let bind_index = parameters.index.map(|index| {
                    let index = parameter_ident(index);
                    quote!(let #index = __xr_row_index;)
                });
                let available = self.row_target_available(
                    row,
                    delegate_path,
                    &repeat.row,
                    &quote!(__xr_rest.is_empty()),
                    None,
                )?;
                delegates.extend(quote! {
                    [#(#path,)* slot, 0, #(#delegate_path,)* rest @ ..] => Some((|| {
                        let view = scene.at_path(&[#(#path),*])?;
                        let (index, key, __xr_row_node) = view.list_event_row(*slot)?;
                        let __xr_child = __xr_row_node.at_path(&[#(#delegate_path),*])?;
                        let repeater = &self.__state.__visuals.#field;
                        let (index, model) = if follow_row { (index, #resolved_model) } else { #projected_target };
                        let __xr_visit = visit;
                        let __xr_route = input;
                        let __xr_rest = rest;
                        repeater.with_resolved_event_row_state(
                            &*model, index, key, follow_row,
                            |__xr_delegates, #item, __xr_row_index, #row_state| {
                                #bind_index
                                let mut __xr_visibility_overridden = false;
                                if __xr_route.is_some_and(|__xr_input| !(#available)) {
                                    return ::core::default::Default::default();
                                }
                                let mut __xr_ui = #component { __state: __xr_delegates.#delegate_slot.__state.clone() };
                                __xr_visit(&mut __xr_ui, __xr_child, __xr_rest, __xr_route.map(|input| {
                                    input.with_root_visibility_override(__xr_visibility_overridden)
                                }))
                            },
                        )
                    })().unwrap_or_default()),
                });
            }
            let mut groups = TokenStream::new();
            self.repeated_check_targets(row, &repeat.row, &mut Vec::new(), &mut groups)?;
            let group_route = if groups.is_empty() {
                TokenStream::new()
            } else {
                let route = self.with_check_event_row(
                    row,
                    &self.visuals,
                    repeat,
                    Some(quote!(&*model)),
                    &groups,
                )?;
                quote! {
                    if matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked")) {
                        let __xr_rest = rest;
                        let __xr_row_node = row;
                        let handled = (|| {
                            let repeater = &self.__state.__visuals.#field;
                            let (__xr_source_index, model) = { #projected_target };
                            let __xr_row_key = key;
                            #route.flatten()
                        })();
                        if let Some(handled) = handled { return Some(handled); }
                    }
                }
            };
            let edits = if matches!(element.attr("kind"), Ok("TableView" | "TreeView")) {
                let model_index = if element.attr("kind") == Ok("TreeView") {
                    let snapshot = tree_snapshot_method(element_key(element)?);
                    quote! {
                        let tree = view.tree.as_ref()?;
                        let live = self.#snapshot();
                        let index = tree.event_source_index(&live, index, key)?;
                    }
                } else {
                    let model = self.table_model(element)?;
                    quote! {
                        let rows = #model;
                        let index = rows.positions().event_source_index(view.standard_list_stamp, index)?;
                    }
                };
                let record_history = table_history_item(element)?.is_some();
                let (resolve_model, table_rows) = if element.attr("kind") == Ok("TableView") {
                    let rows =
                        self.editable_table_rows_model(element, &quote!(self), path.is_empty())?;
                    (TokenStream::new(), Some(rows))
                } else {
                    (
                        quote! {
                            let repeater = &self.__state.__visuals.#field;
                            let (index, source) = { #projected_target };
                            if !input.is_current() { return Some(false); }
                            let model = &*source;
                        },
                        None,
                    )
                };
                let history = if record_history {
                    let enabled = table_history_method(element_key(element)?, "enabled");
                    quote!(self.#enabled())
                } else {
                    quote!(false)
                };
                let mut editors = Vec::new();
                row_editor_paths(row, &mut Vec::new(), &mut editors);
                let mut writes = TokenStream::new();
                for (cell_path, editor, link) in editors {
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
                            let ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::Spin(spin)) = action else { return Some(false); };
                            let expected = cell.int("value")?;
                            let Some(next) = ::xrack_runtime::spin_value_after(cell, *spin) else { return Some(false); };
                        },
                        Type::Bool => quote! {
                            let expected = cell.boolean("checked", false);
                            let Some(next) = action.checked_after(expected) else { return Some(false); };
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
                    let matches_field = if field_path.is_empty() {
                        quote!(|record| record == expected)
                    } else {
                        let field = field_access(field_path.iter().copied());
                        quote!(|record| record #field == expected)
                    };
                    let field_current = if table_rows.is_some() {
                        quote!(true)
                    } else {
                        quote!(model.row_matches(index, key, #matches_field))
                    };
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
                    let toggle = matches!(value_type, Type::Bool | Type::CheckState);
                    let check_state = value_type == Type::CheckState;
                    let commit = self.table_cell_commit(
                        element,
                        quote!(model),
                        field_path,
                        value_type,
                        update,
                        record_history.then(|| quote!(record_history)),
                    )?;
                    let text_editor = matches!(editor.attr("kind"), Ok("TableCell" | "TreeCell"));
                    let mut permission = quote!(|| true);
                    let preparation = if text_editor || toggle {
                        let nodes = Self::row_path(row, &cell_path)?;
                        let available = self.row_input_available(
                            &nodes,
                            &repeat.row,
                            text_editor,
                            &quote!(__xr_input),
                        )?;
                        let parameters = RepeatRef::from_operation(model)?;
                        let item = parameter_ident(parameters.item);
                        let bind_index = parameters.index.map(|index| {
                            let index = parameter_ident(index);
                            quote!(let #index = __xr_row_index;)
                        });
                        let field = repeat_field(repeat.index());
                        let row_state = row_local(repeat.index());
                        let source = self.expr(self.operand(model, 0)?)?;
                        let prepare = if toggle {
                            let tristate = if check_state {
                                let value = children(editor, "xr.bind")
                                    .find(|binding| binding.attr("name") == Ok("tristate"))
                                    .map(|binding| self.visual_value(binding, &repeat.row))
                                    .transpose()?
                                    .unwrap_or_else(|| quote!(false));
                                quote!(::core::option::Option::Some(|| #value))
                            } else {
                                quote!(::core::option::Option::<fn() -> bool>::None)
                            };
                            let live_source = if table_rows.is_some() {
                                quote!(let __xr_live_source = __xr_repeater.resolve_model(|| #source);)
                            } else {
                                quote!(let __xr_live_source = model;)
                            };
                            quote! {
                                if !__xr_input.available(|| #available) || !(#field_current) {
                                    return ::core::option::Option::None;
                                }
                                #live_source
                                if !__xr_live_row() {
                                    return ::core::option::Option::None;
                                }
                                ::xrack_runtime::prepare_row_toggle_matching(
                                    __xr_cell, __xr_input, #tristate,
                                    || __xr_live_source.row_matches(index, key, #matches_field),
                                    || #history,
                                )
                            }
                        } else {
                            quote!(__xr_input.prepare(|| (#available) && (#field_current), || ::core::option::Option::Some(())))
                        };
                        let toggle_route = toggle.then(|| {
                            let live_row = if table_rows.is_none() {
                                quote! {
                                    let __xr_row_index = || {
                                        let index = visual_index;
                                        #model_index
                                        ::core::option::Option::Some(index)
                                    };
                                    let __xr_live_row = || __xr_row_index() == ::core::option::Option::Some(index);
                                }
                            } else {
                                quote!(let __xr_live_row = || true;)
                            };
                            quote! {
                                let __xr_cell = cell;
                                #live_row
                            }
                        });
                        let preparation_model = if table_rows.is_some() {
                            quote!(&__xr_repeater.resolve_model(|| #source))
                        } else {
                            quote!(&source)
                        };
                        let prepare_row = quote! {{
                            let __xr_input = input;
                            #toggle_route
                            let __xr_repeater = &self.__state.__visuals.#field;
                            __xr_repeater.with_resolved_row_state(
                                #preparation_model, index, key,
                                |#item, __xr_row_index, #row_state| {
                                    #bind_index
                                    #prepare
                                },
                            ).flatten()
                        }};
                        if text_editor && table_rows.is_some() {
                            permission = quote!(|| (|| { #model_index #prepare_row })().is_some());
                            Some(quote!(input.prepare(|| true, || Some(#history))))
                        } else {
                            Some(prepare_row)
                        }
                    } else if matches!(
                        editor.attr("kind"),
                        Ok("SpinBox" | "DoubleSpinBox" | "Slider" | "Dial")
                    ) {
                        let nodes = Self::row_path(row, &cell_path)?;
                        let view_available = if table_rows.is_some() {
                            quote!(true)
                        } else {
                            let receiver = quote!(self);
                            let projection = TableProjection::for_input(&receiver);
                            self.element_allows_routed_input(element, path.is_empty(), &projection)?
                        };
                        let guard = quote! {
                            if !(#view_available) { return ::core::option::Option::None; }
                            #model_index
                        };
                        Some(self.row_numeric_preparation(
                            &nodes,
                            model,
                            repeat,
                            operand,
                            &guard,
                            &field_current,
                            &history,
                            table_rows.is_none(),
                        )?)
                    } else {
                        None
                    };
                    let resolve_edit = if let Some(rows) = &table_rows {
                        let prepared = preparation
                            .map(|prepare| {
                                if text_editor {
                                    prepare
                                } else {
                                    quote!({ #model_index #prepare })
                                }
                            })
                            .unwrap_or_else(|| quote!(::core::option::Option::Some(#history)));
                        let target = quote!(::xrack_runtime::table_cell_edit_target_matching(
                            view, *slot, || #rows, #matches_field, #permission, || #prepared,
                        ));
                        quote! {
                            let ::core::option::Option::Some((target, record_history)) = #target
                            else {
                                return ::core::option::Option::Some(false);
                            };
                            let index = target.index;
                            let key = target.key;
                            let model = target.rows.source();
                        }
                    } else if let Some(prepare) = preparation {
                        quote! {
                            if (|| #prepare)().is_none() {
                                return ::core::option::Option::Some(false);
                            }
                        }
                    } else {
                        TokenStream::new()
                    };
                    writes.extend(quote!([#(#cell_path),*] => {
                        #prepare
                        #resolve_edit
                        if !input.is_current() { return Some(false); }
                        #commit
                    },));
                }
                (!writes.is_empty()).then(|| {
                    quote! {
                        let visual_index = index;
                        let cell = row.interactive_at_path(rest)?;
                        #resolve_model
                        Some(match rest { #writes _ => false })
                    }
                })
            } else {
                None
            };
            if edits.is_some() || !group_route.is_empty() {
                let edit = edits.unwrap_or_else(|| quote!(Some(false)));
                out.extend(quote! {
                    [#(#path,)* slot, 0, rest @ ..] => Some((|| {
                        let view = scene.at_path(&[#(#path),*])?;
                        let (index, key, row) = view.list_event_row(*slot)?;
                        #group_route
                        #edit
                    })().unwrap_or(false)),
                });
            }
        } else {
            if element.attr("kind") == Ok("SplitView") {
                let key = element_key(element)?;
                let field = split_state_field(key);
                let resized = self.typed_event_values(element, "resized", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let node = scene.at_path(path)?;
                        let changed = self.__state.#field.resize(node, input);
                        if changed { #resized }
                        Some(changed)
                    },
                });
            }
            for (index, child) in child_slots(element) {
                if child.attribute("kind") == Some("EmptyState") {
                    continue;
                }
                path.push(index);
                if let Some(model) = children(child, "xr.repeat").next() {
                    if element.attribute("kind") == Some("Loader") && child.name() == "xr.instance"
                    {
                        self.loader_event_arm(child, model, path, delegates)?;
                    } else {
                        let slot = format_ident!("__xr_slot_{}", path.len());
                        let prefix = path.iter().map(|index| quote!(#index)).collect::<Vec<_>>();
                        self.repeated_check_events(child, &self.visuals, &slot, &prefix, out)?;
                    }
                } else {
                    self.delegate_event_arms(child, path, out, delegates)?;
                }
                path.pop();
            }
        }
        Ok(())
    }

    fn loader_item_accessor(&self) -> Result<TokenStream, String> {
        let mut loaders = BTreeMap::<&str, TokenStream>::new();
        for &owner in &self.visuals.external_loaders {
            let loader = self.elements[owner];
            let id = loader.attr("id")?;
            let Some(id) = xrack_mlir::host_element_id(owner, id) else {
                continue;
            };
            let slot = external_loader_slot(owner);
            loaders.entry(id).or_default().extend(quote! {
                if let Some(item) = self.__state.__visuals.#slot.item::<T>() {
                    return Some(item);
                }
            });
        }
        for repeat in self.visuals.repeats.values() {
            let Some((owner, index)) = repeat.loader else {
                continue;
            };
            let loader = self.elements[owner];
            let id = loader.attr("id")?;
            let Some(id) = xrack_mlir::host_element_id(owner, id) else {
                continue;
            };
            let slot = loader_slot(owner);
            let storage = repeat.storage_type(&rust_name(self.component_name));
            loaders.entry(id).or_default().extend(quote! {
                if let Some(repeater) = self.__state.__visuals.#slot.get::<#storage>(#index) {
                    let key = ::xrack_runtime::ListModel::row_key(&1i64, 0);
                    let item = repeater.with_event_row(|| 1, 0, key, false, |delegates| {
                        self.__state.__visuals.#slot.item(#index, delegates.0.as_weak())?.cast::<T>()
                    }).flatten();
                    if item.is_some() { return item; }
                }
            });
        }
        if loaders.is_empty() {
            return Ok(TokenStream::new());
        }
        let branches = loaders
            .into_iter()
            .map(|(id, body)| quote!(#id => { #body },));
        Ok(quote! {
            pub fn loader_item<T: 'static>(&self, id: &str) -> ::core::option::Option<::xrack_runtime::LoaderItem<T>> {
                match id { #(#branches)* _ => {} }
                None
            }
        })
    }

    fn navigation_page_delegate_arms(
        &self,
        page: OperationRef<'a>,
        model: OperationRef<'a>,
        path: &[usize],
        delegates: &mut TokenStream,
    ) -> Result<(), String> {
        let repeat = &self.visuals.repeats[element_key(page)?];
        let field = repeat_field(repeat.index());
        let state = row_local(repeat.index());
        let parameters = RepeatRef::from_operation(model)?;
        let item = parameter_ident(parameters.item);
        let bind_index = parameters.index.map(|name| {
            let name = parameter_ident(name);
            quote!(let #name = __xr_row_index;)
        });
        let model = self.expr(self.operand(model, 0)?)?;
        for (delegate_slot, (delegate_path, delegate)) in repeat.delegates.iter().enumerate() {
            let delegate_slot = Literal::usize_unsuffixed(delegate_slot);
            let component = ident(delegate.attr("component")?);
            let available = self.row_target_available(
                page,
                delegate_path,
                &repeat.row,
                &quote!(__xr_rest.is_empty()),
                None,
            )?;
            delegates.extend(quote! {
                [#(#path,)* slot, #(#delegate_path,)* rest @ ..] => Some((|| {
                    let view = scene.at_path(&[#(#path),*])?;
                    let __xr_row_node = view.child_at(*slot)?;
                    let key = __xr_row_node.row_key?;
                    let __xr_child = __xr_row_node.at_path(&[#(#delegate_path),*])?;
                    let __xr_rest = rest;
                    self.__state.__visuals.#field.with_event_row_state(
                        || #model, *slot, key, false,
                        |__xr_delegates, #item, __xr_row_index, #state| {
                            #bind_index
                            let mut __xr_visibility_overridden = false;
                            if input.is_some_and(|__xr_input| !(#available)) {
                                return ::core::default::Default::default();
                            }
                            let mut ui = #component { __state: __xr_delegates.#delegate_slot.__state.clone() };
                            visit(&mut ui, __xr_child, rest, input.map(|input| {
                                input.with_root_visibility_override(__xr_visibility_overridden)
                            }))
                        },
                    )
                })().unwrap_or_default()),
            });
        }
        Ok(())
    }

    fn loader_event_arm(
        &self,
        candidate: OperationRef<'a>,
        model: OperationRef<'a>,
        path: &[usize],
        delegates: &mut TokenStream,
    ) -> Result<(), String> {
        let repeat = self
            .visuals
            .repeats
            .get(element_key(candidate)?)
            .ok_or("Loader candidate requires a materialized repeat")?;
        let (owner, candidate_index) = repeat.loader.ok_or("missing Loader owner")?;
        let slot_field = loader_slot(owner);
        let storage = repeat.storage_type(&rust_name(self.component_name));
        let row_state = row_local(repeat.index());
        let model = self.expr(self.operand(model, 0)?)?;
        let component = ident(candidate.attr("component")?);
        let available =
            self.row_target_available(candidate, &[], &repeat.row, &quote!(rest.is_empty()), None)?;
        delegates.extend(quote! {
            [#(#path,)* slot, rest @ ..] => Some((|| {
                let __xr_row_node = scene.at_path(&[#(#path,)* *slot])?;
                let key = __xr_row_node.row_key?;
                let repeater = self.__state.__visuals.#slot_field.get::<#storage>(#candidate_index)?;
                if #model <= 0 {
                    return None;
                }
                repeater.with_event_row_state(|| 1, *slot, key, false, |__xr_delegates, _, _, #row_state| {
                    let mut __xr_visibility_overridden = false;
                    if input.is_some_and(|__xr_input| !(#available)) {
                        return ::core::default::Default::default();
                    }
                    let mut ui = #component { __state: __xr_delegates.0.__state.clone() };
                    if __xr_row_node.loader_mount.as_ref() != Some(ui.__state.__binding_lifetime.loader_mount()) {
                        return ::core::default::Default::default();
                    }
                    visit(&mut ui, __xr_row_node, rest, input.map(|input| {
                        input.with_root_visibility_override(__xr_visibility_overridden)
                    }))
                })
            })().unwrap_or_default()),
        });
        Ok(())
    }

    fn repeated_check_events(
        &self,
        row: OperationRef<'a>,
        scope: &VisualScope<'_>,
        slot: &Ident,
        prefix: &[TokenStream],
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let Some(repeat) = scope.repeats.get(element_key(row)?) else {
            return Ok(());
        };
        if !repeat.uses_row_identity() {
            return Ok(());
        }
        let mut body = TokenStream::new();
        self.repeated_check_targets(row, &repeat.row, &mut Vec::new(), &mut body)?;
        if body.is_empty() {
            return Ok(());
        }
        let route = self.with_check_event_row(row, scope, repeat, None, &body)?;
        out.extend(quote! {
            [#(#prefix,)* #slot, __xr_rest @ ..] => Some((|| {
                if !matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked")) {
                    return None;
                }
                let __xr_container = scene.at_path(&[#(#prefix),*])?;
                let __xr_source_index = *#slot;
                let __xr_row_node = __xr_container.child_at(__xr_source_index)?;
                let __xr_row_key = __xr_row_node.row_key?;
                #route.flatten()
            })().unwrap_or(false)),
        });
        Ok(())
    }

    fn with_check_event_row(
        &self,
        row: OperationRef<'a>,
        scope: &VisualScope<'_>,
        repeat: &RepeatScope<'_>,
        resolved_model: Option<TokenStream>,
        body: &TokenStream,
    ) -> Result<TokenStream, String> {
        let model = children(row, "xr.repeat")
            .next()
            .ok_or("missing repeat model")?;
        let parameters = RepeatRef::from_operation(model)?;
        let item = parameter_ident(parameters.item);
        let bind_index = parameters.index.map(|name| {
            let name = parameter_ident(name);
            quote!(let #name = __xr_row_index;)
        });
        let field = repeat_field(repeat.index());
        let row_state = row_local(repeat.index());
        let receiver = scope.receiver();
        let checked = self.row_button_reads(&repeat.row)?;
        let toggles = self.row_button_toggles(&repeat.row)?;
        let route = quote! {
            #bind_index
            #checked
            #toggles
            let __xr_input = input;
            match __xr_rest { #body _ => None }
        };
        if let Some(model) = resolved_model {
            Ok(quote! {
                #receiver.#field.with_resolved_event_row_state(
                    #model, __xr_source_index, __xr_row_key, false,
                    |_, #item, __xr_row_index, #row_state| { #route },
                )
            })
        } else {
            let source = self.visual_expr(self.operand(model, 0)?, scope)?;
            Ok(quote! {
                #receiver.#field.with_row_state(
                    || #source, __xr_source_index, __xr_row_key,
                    |#item, __xr_row_index, #row_state| { #route },
                )
            })
        }
    }

    fn repeated_check_targets(
        &self,
        row: OperationRef<'a>,
        scope: &VisualScope<'_>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let node = *Self::row_path(row, path)?
            .last()
            .ok_or("missing repeated node")?;
        if has_repeated_check_state(node) {
            let available = self.row_target_available(row, path, scope, &quote!(true), None)?;
            let input = quote!(Some(input));
            let click = if has_button_check_state(node) {
                self.activate_checkable_button(node, scope, &input, &quote!(true))?
            } else {
                self.group_box_toggle(node, scope, &input)?
            };
            out.extend(quote!([#(#path),*] => Some((#available) && #click),));
        }
        for (index, child) in child_slots(node) {
            if let Some(repeat) = scope.repeats.get(element_key(child)?) {
                if repeat.uses_row_identity() {
                    let mut body = TokenStream::new();
                    self.repeated_check_targets(child, &repeat.row, &mut Vec::new(), &mut body)?;
                    if !body.is_empty() {
                        let available = self.row_target_available(
                            row,
                            path,
                            scope,
                            &quote!(false),
                            Some(index),
                        )?;
                        let route = self.with_check_event_row(child, scope, repeat, None, &body)?;
                        out.extend(quote! {
                            [#(#path,)* #index, __xr_slot, __xr_rest @ ..] => Some((#available) && (|| {
                                let __xr_container = __xr_row_node.at_path(&[#(#path,)* #index])?;
                                let __xr_source_index = *__xr_slot;
                                let __xr_row_node = __xr_container.child_at(__xr_source_index)?;
                                let __xr_row_key = __xr_row_node.row_key?;
                                #route.flatten()
                            })().unwrap_or(false)),
                        });
                    }
                }
            } else if children(child, "xr.repeat").next().is_none() {
                path.push(index);
                self.repeated_check_targets(row, scope, path, out)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn item_delegate_events(
        &self,
        row: OperationRef<'a>,
        repeat: OperationRef<'a>,
        projected_target: &TokenStream,
        path: &[usize],
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let parameters = RepeatRef::from_operation(repeat)?;
        let parameter = parameter_ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index = (!index.is_empty()).then(|| {
            let index = parameter_ident(index);
            quote!(let #index = __xr_index as ::core::primitive::i64;)
        });
        let parameters = quote! {
            let #parameter = ::xrack_runtime::ListModel::item(&*__xr_model, __xr_index);
            #index
        };
        let mut field_check = TokenStream::new();
        let has_action = action_ref(row).is_some();
        let click = if let Some(action) = action_ref(row) {
            let activate = action_activate_method(action);
            quote! {
                if !matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked") |
                    ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::ActivateAction)) {
                    return Some(false);
                }
                Some(self.#activate(|| true, Some(input)))
            }
        } else if let Some(link) = children(row, "xr.row_link").next() {
            let operand = self.operand(link, 0)?;
            let field = field_access(self.record_path(operand)?);
            let (expected, next, state, tristate_check) = if link.attr("name")? == "check-state" {
                let scope = &self.visuals.repeats[element_key(row)?].row;
                let tristate = children(row, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("tristate"))
                    .map(|binding| self.visual_value(binding, scope))
                    .transpose()?
                    .unwrap_or_else(|| quote!(false));
                let tristate_check = quote! {
                    && ::xrack_runtime::prepare_row_toggle_matching(
                        node, input, ::core::option::Option::Some(|| #tristate),
                        || __xr_model.row_matches(__xr_index, __xr_key, |record| record #field == node.check_state()),
                        || false,
                    ).is_some()
                };
                (
                    quote!(node.check_state()),
                    quote!(__xr_expected.next(node.boolean("tristate", false))),
                    quote!(__xr_next),
                    tristate_check,
                )
            } else {
                (
                    quote!(node.boolean("checked", false)),
                    quote!({
                        let Some(next) = action.checked_after(__xr_expected) else {
                            return Some(false);
                        };
                        next
                    }),
                    quote!(::xrack_runtime::CheckState::from(__xr_next)),
                    TokenStream::new(),
                )
            };
            field_check = quote! {
                && (!matches!(action, ::xrack_runtime::RoutedEvent::Dispatch("clicked")
                    | ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::SetSwitch(_)))
                    || (__xr_model.row_matches(__xr_index, __xr_key, |record| record #field == #expected)
                        #tristate_check))
            };
            let has_toggled =
                children(row, "xr.handler").any(|handler| handler.attr("name") == Ok("toggled"));
            let key = element_key(row)?;
            let receiver = quote!(self);
            let context = ExpressionContext {
                row_toggle_owner: Some(key),
                ..ExpressionContext::new(&receiver)
            };
            let read = if self.toggle_state_targets.contains(key) {
                let field_slot = repeat_field(self.visuals.repeats[key].index());
                let source = self.expr(self.operand(repeat, 0)?)?;
                quote! {
                    let __xr_read_toggle = || {
                        let model = self.__state.__visuals.#field_slot.resolve_model(|| #source);
                        ::xrack_runtime::ListModel::event_row_index(&*model, __xr_index, __xr_key, true)
                            .map(|index| ::xrack_runtime::CheckState::from(::xrack_runtime::ListModel::item(&*model, index) #field))
                            .unwrap_or_default()
                    };
                }
            } else {
                TokenStream::new()
            };
            let toggled = self.typed_event_values_in(row, "toggled", &[], context)?;
            let changed = self.typed_event_values_in(
                row,
                "check-state-changed",
                &[quote!(__xr_state)],
                context,
            )?;
            quote! {
                let __xr_change = match action {
                    ::xrack_runtime::RoutedEvent::Dispatch("clicked")
                    | ::xrack_runtime::RoutedEvent::Control(::xrack_runtime::ControlAction::SetSwitch(_)) => {
                        let __xr_expected = #expected;
                        let __xr_next = #next;
                        if !__xr_model.update_row_if_key(__xr_index, __xr_key, |record| {
                            if record #field != __xr_expected { return ::core::option::Option::None; }
                            let mut record = record.clone();
                            record #field = __xr_next;
                            ::core::option::Option::Some(record)
                        }) { return ::core::option::Option::Some(false); }
                        ::core::option::Option::Some(#state)
                    }
                    ::xrack_runtime::RoutedEvent::Dispatch("toggled") if #has_toggled =>
                        ::core::option::Option::None,
                    _ => return ::core::option::Option::Some(false),
                };
                #parameters
                #read
                #toggled
                if let ::core::option::Option::Some(__xr_state) = __xr_change {
                    #changed
                }
                ::core::option::Option::Some(true)
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
        let repeat_scope = &self.visuals.repeats[element_key(row)?];
        let field = repeat_field(repeat_scope.index());
        let row_state = row_local(repeat_scope.index());
        let available = self.item_delegate_available(row, &repeat_scope.row)?;
        let live_index = RepeatRef::from_operation(repeat)?.index.map(|name| {
            let index = parameter_ident(name);
            quote!(let #index = __xr_row_index;)
        });
        let track_item = has_action.then(|| {
            quote!(
                ::xrack_runtime::ListModel::track_item(&*__xr_model, __xr_index);
            )
        });
        out.extend(quote! {
            [#(#path,)* slot, 0] => {
                let view = scene.interactive_at_path(&[#(#path),*])?;
                let (index, __xr_key, _) = view.list_event_row(*slot)?;
                let node = view.interactive_at_path(&[*slot, 0])?;
                let repeater = &self.__state.__visuals.#field;
                let (__xr_index, __xr_model) = { #projected_target };
                if !input.is_current() { return Some(false); }
                let Some(__xr_index) = ::xrack_runtime::ListModel::event_row_index(
                    &*__xr_model, __xr_index, __xr_key, false,
                ) else { return Some(false); };
                if action.activates_item_delegate(#has_action)
                    && ::xrack_runtime::prepare_action_view(|| {
                        #track_item
                        let allowed = repeater.with_resolved_row_state(
                            &__xr_model, __xr_index, __xr_key,
                            |#parameter, __xr_row_index, #row_state| {
                                #live_index
                                (#available) #field_check
                            },
                        )?;
                        allowed.then_some(())
                    }).is_none()
                {
                    return ::core::option::Option::Some(false);
                }
                if !input.is_current() { return Some(false); }
                #click
            },
        });
        Ok(())
    }

    fn item_delegate_available(
        &self,
        row: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        self.row_input_available(&[row], scope, false, &quote!(input))
    }

    fn row_path(
        mut row: OperationRef<'a>,
        path: &[usize],
    ) -> Result<Vec<OperationRef<'a>>, String> {
        let mut nodes = vec![row];
        for &index in path {
            row = static_children(row)
                .find(|(slot, _)| *slot == index)
                .map(|(_, child)| child)
                .ok_or("invalid path in a row template")?;
            nodes.push(row);
        }
        Ok(nodes)
    }

    fn row_target_available(
        &self,
        row: OperationRef<'a>,
        path: &[usize],
        scope: &VisualScope<'_>,
        at_target: &TokenStream,
        child_slot: Option<usize>,
    ) -> Result<TokenStream, String> {
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&receiver)
        });
        let mut checks = Vec::new();
        for (depth, node) in Self::row_path(row, path)?.into_iter().enumerate() {
            let visibility = if node.name() == "xr.instance" {
                let visible = self.row_input_flag(
                    node,
                    "visible",
                    quote!(::core::option::Option::None),
                    scope,
                    &projection,
                    true,
                )?;
                quote!({
                    let visible = #visible;
                    __xr_visibility_overridden = visible.is_some();
                    visible.unwrap_or(true)
                })
            } else {
                self.row_input_flag(node, "visible", quote!(true), scope, &projection, false)?
            };
            let enabled =
                self.row_input_flag(node, "enabled", quote!(true), scope, &projection, false)?;
            let guards = self.row_input_conditions(node, scope)?;
            let prefix = &path[..depth];
            let target = depth == path.len();
            let scope_guard = if node.attr("kind") == Ok("GroupBox")
                && let Some(next) = path.get(depth).copied().or(child_slot)
            {
                let checkable = self.row_input_flag(
                    node,
                    "checkable",
                    quote!(false),
                    scope,
                    &projection,
                    false,
                )?;
                let checked = self.control_value_in(node, "checked", Type::Bool, scope)?;
                quote! {
                    && __xr_node.input_scope_available(__xr_node.child_at(#next), || false, || #checkable, || #checked, || ::xrack_runtime::SwipeSide::Closed)
                }
            } else {
                TokenStream::new()
            };
            checks.push(quote! {
                __xr_input.available(|| __xr_row_node.at_path(&[#(#prefix),*]).is_some_and(|__xr_node| {
                    (!__xr_node.input_requires_enabled(#target && (#at_target)) || (#enabled))
                        && #((#guards) &&)* (#visibility) #scope_guard
                }))
            });
        }
        let selectors = self.input_selectors(&projection)?;
        Ok(quote!({ #selectors #((#checks) &&)* true }))
    }

    fn row_input_available(
        &self,
        nodes: &[OperationRef<'a>],
        scope: &VisualScope<'_>,
        edit_text: bool,
        input: &TokenStream,
    ) -> Result<TokenStream, String> {
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&receiver)
        });
        let flag = |node, name, default| {
            self.row_input_flag(node, name, default, scope, &projection, false)
        };
        let mut checks = Vec::new();
        for (index, &node) in nodes.iter().enumerate() {
            let enabled = flag(node, "enabled", quote!(true))?;
            let visible = flag(node, "visible", quote!(true))?;
            let guards = self.row_input_conditions(node, scope)?;
            let group = if index + 1 < nodes.len() && node.attr("kind") == Ok("GroupBox") {
                let checkable = flag(node, "checkable", quote!(false))?;
                let checked = self.control_value_in(node, "checked", Type::Bool, scope)?;
                quote!(&& (!(#checkable) || (#checked)))
            } else {
                TokenStream::new()
            };
            checks.push(
                quote!(#input.available(|| (#enabled) && #((#guards) &&)* (#visible) #group)),
            );
        }
        if edit_text {
            let cell = *nodes.last().ok_or("a row editor path requires a cell")?;
            let editable = flag(cell, "editable", quote!(true))?;
            let read_only = flag(cell, "read-only", quote!(false))?;
            checks.push(quote!(#input.available(|| !(#read_only) && (#editable))));
        }
        let selectors = self.input_selectors(&projection)?;
        Ok(quote!({ #selectors #((#checks) &&)* true }))
    }

    fn row_input_conditions(
        &self,
        node: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<Vec<TokenStream>, String> {
        children(node, "xr.show_if")
            .map(|guard| self.visual_value(guard, scope))
            .collect()
    }

    fn row_input_flag(
        &self,
        node: OperationRef<'a>,
        name: &str,
        default: TokenStream,
        scope: &VisualScope<'_>,
        projection: &TableProjection<'_, 'a>,
        optional: bool,
    ) -> Result<TokenStream, String> {
        let wrap = |value: TokenStream| {
            if optional {
                quote!(::core::option::Option::Some(#value))
            } else {
                value
            }
        };
        let base = node
            .children()
            .find(|binding| {
                matches!(binding.name(), "xr.bind" | "xr.layout_bind")
                    && binding.attribute("name") == Some(name)
            })
            .map(|binding| self.visual_value(binding, scope).map(wrap))
            .transpose()?
            .unwrap_or(default);
        self.row_state_value_mapped(node, name, base, projection, wrap)
    }

    fn input_selectors(&self, projection: &TableProjection<'_, 'a>) -> Result<TokenStream, String> {
        let mut selectors = TokenStream::new();
        for (&owner, &selector) in projection.selectors.borrow().iter() {
            let name = state_selector_name(owner);
            let cache = format_ident!("{name}_cache");
            let value = self.state_choice(selector, |condition| {
                self.expr_in(condition, projection.expression)
            })?;
            selectors.extend(quote! {
                let #cache = ::std::cell::OnceCell::new();
                let #name = || *#cache.get_or_init(|| #value);
            });
        }
        Ok(selectors)
    }

    fn control_value(
        &self,
        element: OperationRef<'a>,
        name: &str,
        ty: Type,
    ) -> Result<TokenStream, String> {
        self.control_value_in(element, name, ty, &self.visuals)
    }

    fn control_value_in(
        &self,
        element: OperationRef<'a>,
        name: &str,
        ty: Type,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let expr = |op| {
            if scope.row_index.is_some() {
                self.visual_expr(op, scope)
            } else {
                self.expr(op)
            }
        };
        if let Some(link) = control_link(element, name) {
            return expr(self.operand(link, 0)?);
        }
        let binding = |property| {
            children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(property))
        };
        let initial = if let Some(binding) = binding(name) {
            expr(self.operand(binding, 0)?)?
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
                    Some(binding) => self.visual_value(binding, scope)?,
                    None => fallback(value)?,
                },
                ControlDefault::Control(property) => {
                    self.control_value_in(element, property, ty, scope)?
                }
            }
        };
        let key = element_key(element)?;
        let field = control_field(key, name);
        let storage = if scope.row_checks.contains_key(key) {
            scope.receiver()
        } else {
            quote!(self.__state)
        };
        Ok(quote!(#storage.#field.get(|| #initial)))
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
            let ty = if viewport_bindings(element).any(|name| name == property) {
                Some(Type::Length)
            } else {
                controlled_properties(element)
                    .find_map(|(name, ty)| (name == property).then_some(ty))
            };
            Ok(write_cached_property(&field, value, ty.as_ref()))
        }
    }

    fn scroll_bar_step(
        &self,
        bar: OperationRef<'a>,
        direction: xrack_runtime::ScrollBarStep,
    ) -> Result<TokenStream, String> {
        let direction = format_ident!("{direction:?}");
        if let Some(target) = children(bar, "xr.scroll_target").next() {
            let viewport = viewport_handle(target.attr("target")?);
            let axis = self.binding_value_or(
                bar,
                "orientation",
                quote!(::xrack_runtime::Orientation::Vertical),
            )?;
            let step = children(bar, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("step-size"))
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .map_or_else(
                    || quote!(::core::option::Option::None),
                    |step| quote!(::core::option::Option::Some(#step)),
                );
            return Ok(quote! {
                self.__state.#viewport.request(::xrack_runtime::ScrollAction::StepBar {
                    axis: #axis, step_size: #step, direction: ::xrack_runtime::ScrollBarStep::#direction,
                });
            });
        }
        let position = self.control_value(bar, "position", Type::Float)?;
        let size = self.binding_value_or(
            bar,
            "size",
            quote!(::xrack_runtime::ScrollBarRange::DEFAULT_SIZE),
        )?;
        let step = self.binding_value_or(
            bar,
            "step-size",
            quote!(::xrack_runtime::ScrollBarRange::DEFAULT_STEP),
        )?;
        let write = self.set_control_property(bar, "position", &quote!(__xr_value))?;
        Ok(quote! {
            if let ::core::option::Option::Some(__xr_value) =
                ::xrack_runtime::ScrollBarRange::new(#position, #size, #step)
                    .apply(::xrack_runtime::ScrollBarStep::#direction.action())
            {
                #write
            }
        })
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
        let minimum =
            self.binding_value_or(element, "minimum-time", quote!(::xrack_runtime::Time::MIN))?;
        let maximum =
            self.binding_value_or(element, "maximum-time", quote!(::xrack_runtime::Time::MAX))?;
        Ok(
            quote!(::xrack_runtime::TimePickerOptions::new(#use_24_hours, #show_seconds).with_bounds(#minimum, #maximum)),
        )
    }

    fn disabled_calendar_weekdays(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        children(element, "xr.bind")
            .find(|binding| binding.attribute("name") == Some("disabled-weekdays"))
            .map(|binding| {
                let model = self.visual_value(binding, &self.visuals)?;
                Ok(quote!(::xrack_runtime::calendar_weekday_mask(&#model)))
            })
            .unwrap_or_else(|| Ok(quote!(0_u8)))
    }

    fn disabled_calendar_dates(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        children(element, "xr.bind")
            .find(|binding| binding.attribute("name") == Some("disabled-dates"))
            .map(|binding| {
                let model = self.visual_value(binding, &self.visuals)?;
                Ok(quote!((#model).into_snapshot()))
            })
            .unwrap_or_else(|| Ok(quote!(::std::vec::Vec::new())))
    }

    fn date_picker_bounds(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let minimum =
            self.binding_value_or(element, "minimum-date", quote!(::xrack_runtime::Date::MIN))?;
        let maximum =
            self.binding_value_or(element, "maximum-date", quote!(::xrack_runtime::Date::MAX))?;
        let weekdays = self.disabled_calendar_weekdays(element)?;
        let dates = self.disabled_calendar_dates(element)?;
        Ok(
            quote!(::xrack_runtime::DateBounds::new(#minimum, #maximum).with_disabled_weekdays(#weekdays).with_disabled_dates(#dates)),
        )
    }

    fn standard_table_model(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        self.visual_value(self.standard_table(element)?.model, scope)
    }

    fn standard_table(&self, element: OperationRef<'a>) -> Result<&StandardTableRef<'a>, String> {
        self.standard_tables
            .get(element_key(element)?)
            .ok_or_else(|| "missing standard table metadata".into())
    }

    fn standard_table_editors(
        &self,
        element: OperationRef<'a>,
    ) -> Result<Vec<StandardTableEditor>, String> {
        let table = self.standard_table(element)?;
        let mut editors = Vec::new();
        for (column, definition) in table.columns.iter().enumerate() {
            let Some(binding) = definition.editable else {
                continue;
            };
            let ty = self.record_field_type(table.record.clone(), &definition.field)?;
            editors.push(StandardTableEditor {
                column,
                field: definition.field.clone(),
                ty,
                enabled: self.visual_value(binding, &self.visuals)?,
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
        history_enabled: Option<TokenStream>,
    ) -> Result<TokenStream, String> {
        let direct = quote!((#model).update_row_if_key(index, key, #update));
        if table_history_item(element)?.is_none() {
            return Ok(direct);
        }
        let key = element_key(element)?;
        let history = table_history_field(key);
        let enabled = history_enabled.unwrap_or_else(|| {
            let method = table_history_method(key, "enabled");
            quote!(self.#method())
        });
        let capture = Self::table_edit_capture(BTreeMap::from([(path, ty)]));
        Ok(quote!(self.__state.#history.commit_cell_if_enabled(
            #model, index, key, #enabled, #update, #capture,
        )))
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
            let rows = self.table_action_rows_model(
                element,
                "edit-on-tab",
                &quote!(&view.table_column_order),
                path.is_empty(),
            )?;
            out.extend(quote! {
                [#(#path),*] => {
                    let view = scene.at_path(path)?;
                    #guard
                    ::xrack_runtime::next_table_edit_cell_while_current(
                        view, navigation, || input.is_current(), || #rows, #projection,
                    )
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
            let editors = self.standard_table_editors(element)?;
            if editors.is_empty() {
                return Ok(None);
            }
            let columns = editors.iter().map(|editor| editor.column);
            let flags = editors.iter().map(|editor| {
                let column = editor.column;
                let enabled = &editor.enabled;
                quote!(#column => #enabled,)
            });
            return Ok(Some((
                quote!(if !columns.iter().any(|column| [#(#columns),*].contains(column)) { return None; }),
                quote!(|_, column, _, _| match column { #(#flags)* _ => false }),
            )));
        }
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let item = parameter_ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = (!index.is_empty()).then(|| {
            let index = parameter_ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        });
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&receiver)
        });
        let mut columns = Vec::new();
        let mut editors = TokenStream::new();
        for (column, cell) in scene_children(row).enumerate() {
            if cell.attr("kind") == Ok("TableCell")
                && children(cell, "xr.row_link").next().is_some()
            {
                columns.push(column);
                let editable = self.row_editable(cell, &projection)?;
                editors.extend(quote!(#column => row_editable() && (#editable),));
            }
        }
        if columns.is_empty() {
            return Ok(None);
        }
        let record_type = value_type(self.operand(repeat, 0)?)?
            .list_item()
            .ok_or("editable table requires a list model")?;
        let record_type = ty(&record_type);
        let visible = self.row_shown(row, &projection)?;
        let enabled = self.row_binding_or(row, "enabled", quote!(true), &projection)?;
        Ok(Some((
            quote! {
                if !columns.iter().any(|column| [#(#columns),*].contains(column)) { return None; }
            },
            self.project_table_rows(
                &projection,
                quote!(__xr_index, column, _key, __xr_record: &#record_type),
                quote! {
                    let #item = || (*__xr_record).clone();
                    #index_binding
                },
                quote!({
                    let row_editable = || (#visible) && (#enabled);
                    match column { #editors _ => false }
                }),
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
            && table_option_configured(element, "copyable")
        {
            let model = self.table_action_rows_model(
                element,
                "copyable",
                &quote!(&node.table_column_order),
                path.is_empty(),
            )?;
            let projection = if element.attr("kind") == Ok("StandardTableView") {
                let table = self.standard_table(element)?;
                let row_type = ident(&table.record);
                let arms = table.columns.iter().enumerate().map(|(column, definition)| {
                    let field = field_access(definition.field.split('.'));
                    quote!(#column => ::core::option::Option::Some(::xrack_runtime::TableField::cell_text(&__xr_row #field)),)
                });
                quote!(|_, column, _, __xr_row: &#row_type| match column {
                    #(#arms)*
                    _ => ::core::option::Option::None,
                })
            } else {
                self.custom_table_projection(element)?
            };
            arms.copies.extend(quote! {
                [#(#path),*] => ::xrack_runtime::copy_table_text_while_current(node, columns, || input.is_current(), || #model, #projection),
            });
        }
        self.table_history_arms(element, path, arms)?;
        for (index, child) in static_children(element) {
            path.push(index);
            self.table_clipboard_arms(child, path, arms)?;
            path.pop();
        }
        Ok(())
    }

    fn table_history_arms(
        &self,
        element: OperationRef<'a>,
        path: &[usize],
        arms: &mut TableClipboardArms,
    ) -> Result<(), String> {
        let Some(item) = table_history_item(element)? else {
            return Ok(());
        };
        let item = ty(&item);
        let key = element_key(element)?;
        let history = table_history_field(key);
        let active = table_history_method(key, "enabled");
        let available = table_history_method(key, "available");
        let replay = table_history_method(key, "replay");
        let replay_from_view = table_history_method(key, "replay_from_view");
        let source = self.view_source_model(element)?;
        let receiver = quote!(self);
        let state = TableProjection::new(ExpressionContext::new(&receiver));
        let flag = |name| self.row_binding_or(element, name, quote!(false), &state);
        let undoable = flag("undoable")?;
        let pasteable = flag("pasteable")?;
        let input_state = TableProjection::for_input(&receiver);
        let editable = self.element_allows_routed_input(element, path.is_empty(), &input_state)?;
        arms.fields
            .extend(quote!(#history: ::xrack_runtime::TableEditHistory<#item>,));
        arms.defaults
            .extend(quote!(#history: ::core::default::Default::default(),));
        arms.replays.extend(quote! {
            [#(#path),*] => self.#replay_from_view(model, redo, route),
        });
        arms.methods.extend(quote! {
            fn #active(&self) -> ::core::primitive::bool {
                #undoable || #pasteable
            }
            fn #available(&self, redo: ::core::primitive::bool) -> ::core::primitive::bool {
                self.#active() && self.__state.#history.can_replay(&#source, redo)
            }
            fn #replay(&self, redo: ::core::primitive::bool) -> ::core::primitive::bool {
                let source = ::xrack_runtime::try_prepare_history_replay(
                    ::core::option::Option::None,
                    || ::core::result::Result::<_, ::core::convert::Infallible>::Ok(
                        ::core::option::Option::Some(#source),
                    ),
                    || ::core::result::Result::Ok(self.#active()),
                ).unwrap_or_else(|error| match error {});
                source.is_some_and(|source| self.__state.#history.replay(&source, redo))
            }

            fn #replay_from_view(
                &self,
                model: ::core::primitive::u64,
                redo: ::core::primitive::bool,
                input: ::xrack_runtime::InputRoute<'_>,
            ) -> ::core::primitive::bool {
                let source = ::xrack_runtime::try_history_replay_source(
                    model,
                    || ::core::result::Result::<_, ::core::convert::Infallible>::Ok(
                        (#editable).then(|| #source),
                    ),
                    || ::core::result::Result::Ok(self.#active()),
                ).unwrap_or_else(|error| match error {});
                input.is_current() && source.is_some_and(|source| self.__state.#history.replay(&source, redo))
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
        let paste_model = self.table_action_rows_model(
            element,
            "pasteable",
            &quote!(column_order),
            path.is_empty(),
        )?;
        arms.methods.extend(quote! {
            fn #method(
                &self,
                order_stamp: (::core::primitive::u64, ::core::primitive::u64),
                column_order: &::xrack_runtime::TableColumnOrder,
                paste: ::xrack_runtime::TablePasteInput<'_>,
                input: ::xrack_runtime::InputRoute<'_>,
            ) -> ::core::primitive::bool {
                ::xrack_runtime::with_table_paste_target(
                    input,
                    || (#paste_model).filter(|rows| {
                        rows.positions().stamp() == order_stamp
                            && rows.source().edit_stamp() == paste.stamp
                    }),
                    |rows, current| {
                        self.__state.#history.paste_in_order_guarded(
                            rows, paste, current, #projection, #capture,
                        ).unwrap_or(false)
                    },
                )
            }
        });
        arms.paste_stamps.extend(quote! {
            [#(#path),*] => {
                let rows = #model;
                let stamp = rows.source().edit_stamp();
                let order_stamp = rows.positions().stamp();
                if node.standard_list_stamp != Some(order_stamp) {
                    return ::core::option::Option::None;
                }
                ::core::option::Option::Some(stamp)
            }
        });
        arms.pastes.extend(quote! {
            [#(#path),*] => node.standard_list_stamp.is_some_and(|stamp| {
                self.#method(stamp, &node.table_column_order, paste, route)
            }),
        });
        Ok(())
    }

    fn custom_table_projection(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let row_type = value_type(self.operand(repeat, 0)?)?
            .list_item()
            .unwrap_or(Type::Int);
        let row_type = ty(&row_type);
        let item = parameter_ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = if index.is_empty() {
            TokenStream::new()
        } else {
            let index = parameter_ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        };
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&receiver)
        });
        let expression = |binding| self.expr_in(self.operand(binding, 0)?, projection.expression);
        let row_visible = self.row_shown(row, &projection)?;
        let mut arms = TokenStream::new();
        for (column, cell) in scene_children(row).enumerate() {
            let instance_text = if cell.name() == "xr.instance" {
                self.analysis
                    .instances
                    .get(element_key(cell)?)
                    .and_then(|bindings| bindings.clipboard_text)
            } else {
                None
            };
            let text = match TableClipboardRef::from_cell(cell, instance_text) {
                Some(TableClipboardRef::Value(binding)) => {
                    let value = expression(binding)?;
                    match value_type(self.operand(binding, 0)?)? {
                        Type::String => value,
                        Type::CheckState => quote!(::std::format!("{:?}", #value)),
                        _ => quote!((#value).to_string()),
                    }
                }
                Some(TableClipboardRef::Text(binding)) => {
                    let base = binding
                        .map(expression)
                        .transpose()?
                        .unwrap_or_else(|| quote!(::std::string::String::new()));
                    self.row_state_value(cell, "text", base, &projection)?
                }
                None => continue,
            };
            let shown = self.row_shown(cell, &projection)?;
            arms.extend(quote!(#column if __xr_row_shown() && (#shown) => ::core::option::Option::Some(#text),));
        }
        if arms.is_empty() {
            return Ok(quote!(|_, _, _, _: &#row_type| ::core::option::Option::None));
        }
        self.project_table_rows(
            &projection,
            quote!(__xr_index, __xr_column, _key, __xr_row: &#row_type),
            quote! {
                let #item = || (*__xr_row).clone();
                #index_binding
            },
            quote! {
                let __xr_row_shown = || #row_visible;
                match __xr_column { #arms _ => ::core::option::Option::None }
            },
        )
    }

    fn row_conditions_met(
        &self,
        element: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let mut guards = children(element, "xr.show_if")
            .map(|guard| self.table_projection_value(guard, projection))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(&owner) = self.view_empty_state_owners.get(element_key(element)?) {
            let count = view_count_method(owner);
            let receiver = projection.expression.receiver;
            guards.push(quote!(#receiver.#count(false) == 0));
        }
        Ok(quote!(#((#guards) &&)* true))
    }

    fn row_shown(
        &self,
        element: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let conditions = self.row_conditions_met(element, projection)?;
        let visible = self.row_binding_or(element, "visible", quote!(true), projection)?;
        Ok(quote!((#conditions) && (#visible)))
    }

    fn element_allows_editing(
        &self,
        element: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let enabled = self.row_binding_or(element, "enabled", quote!(true), projection)?;
        let shown = self.row_shown(element, projection)?;
        Ok(quote!((#enabled) && (#shown)))
    }

    fn element_allows_routed_input(
        &self,
        element: OperationRef<'a>,
        is_root: bool,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        if !is_root {
            return self.element_allows_editing(element, projection);
        }
        let enabled = self.row_binding_or(element, "enabled", quote!(true), projection)?;
        let conditions = self.row_conditions_met(element, projection)?;
        let visible = self.row_binding_or(element, "visible", quote!(true), projection)?;
        Ok(quote!((#enabled) && (#conditions) && input.visible(|| #visible)))
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
                ty: field_type,
                enabled,
            } in &editors
            {
                let path: Vec<_> = field.split('.').collect();
                let access = field_access(path.iter().copied());
                edited_fields.insert(path, field_type.clone());
                let field_type = ty(field_type);
                writes.extend(quote! {
                    #column if (#enabled) && __xr_current_target() => {
                        next #access = <#field_type as ::xrack_runtime::TableField>::parse_cell(text)?;
                        ::core::option::Option::Some(())
                    }
                });
            }
            let projection = quote!(|_, column, _, next, text, __xr_current_target| match column {
                #writes
                _ => ::core::option::Option::None,
            });
            return Ok(Some((projection, Self::table_edit_capture(edited_fields))));
        }
        let (row, repeat) = self.list_template(element)?;
        let parameters = RepeatRef::from_operation(repeat)?;
        let item = parameter_ident(parameters.item);
        let index = parameters.index.unwrap_or_default();
        let index_binding = (!index.is_empty()).then(|| {
            let index = parameter_ident(index);
            quote!(let #index = || __xr_index as ::core::primitive::i64;)
        });
        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&receiver)
        });
        let row_shown = self.row_shown(row, &projection)?;
        let row_enabled = self.row_binding_or(row, "enabled", quote!(true), &projection)?;
        let row_available = quote!((#row_shown) && __xr_current_target()
            && (#row_enabled) && __xr_current_target());
        let mut writes = TokenStream::new();
        let mut edited_fields = BTreeMap::new();
        for (column, cell) in scene_children(row).enumerate() {
            let Some(paste) = TablePasteRef::from_cell(cell)? else {
                continue;
            };
            let field = self.paste_field(paste.target, &mut edited_fields)?;
            if let Some(parser) = paste.parser {
                let result = parser
                    .result
                    .defining_operation()
                    .ok_or("missing clipboard parser definition")?;
                let value = self.expr_in(result, projection.expression)?;
                let text = parameter_ident(parser.parameter);
                let mut guards = vec![self.row_editable(cell, &projection)?];
                let mut node = cell;
                for index in parser.path {
                    node = scene_children(node)
                        .nth(index)
                        .ok_or("invalid clipboard parser path")?;
                    guards.push(self.row_editable(node, &projection)?);
                }
                writes.extend(quote! {
                    #column if (#row_available)
                        #(&& (#guards) && __xr_current_target())* => {
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
            let allowed = self.row_editable(cell, &projection)?;
            let kind = cell.attr("kind")?;
            let editable = if matches!(kind, "SpinBox" | "DoubleSpinBox") {
                self.row_binding_or(cell, "editable", quote!(true), &projection)?
            } else {
                quote!(true)
            };
            let value = match paste.target.xr_type()? {
                Type::String => quote!(__xr_text.to_owned()),
                Type::Bool => quote!(::xrack_runtime::table_paste_bool(__xr_text)?),
                Type::CheckState => {
                    let tristate =
                        self.row_binding_or(cell, "tristate", quote!(false), &projection)?;
                    quote!(::xrack_runtime::table_paste_check_state(__xr_text, #tristate)?)
                }
                ty @ (Type::Int | Type::Float) => {
                    let (default_min, default_max, variant, upper_bound) = if ty == Type::Int {
                        (
                            quote!(0_i64),
                            quote!(100_i64),
                            quote!(Int),
                            quote!(::core::primitive::i64::max),
                        )
                    } else {
                        (
                            quote!(0.0_f64),
                            quote!(100.0_f64),
                            quote!(Float),
                            quote!(::core::primitive::f64::max),
                        )
                    };
                    let min = self.row_binding_or(cell, "minimum", default_min, &projection)?;
                    let max = self.row_binding_or(cell, "maximum", default_max, &projection)?;
                    quote!({
                        let Some(::xrack_runtime::Value::#variant(value)) =
                            ::xrack_runtime::TableCellEditor::#variant.parse(__xr_text)
                        else {
                            return ::core::option::Option::None;
                        };
                        ::xrack_runtime::table_paste_number(
                            ::core::option::Option::Some(value),
                            &mut *__xr_current_target, || #min, || #max, #upper_bound,
                        )?
                    })
                }
                _ => return Err("unsupported table paste field type".into()),
            };
            writes.extend(quote! {
                #column if (#row_available)
                    && (#allowed) && __xr_current_target()
                    && (#editable) && __xr_current_target() => {
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
            quote!(
                __xr_index,
                __xr_column,
                __xr_current,
                __xr_next,
                __xr_text,
                __xr_current_target
            ),
            quote! {
                let #item = || (*__xr_current).clone();
                #index_binding
            },
            quote!(match __xr_column { #writes _ => ::core::option::Option::None }),
        )?;
        Ok(Some((projection, Self::table_edit_capture(edited_fields))))
    }

    fn table_edit_capture(edited_fields: BTreeMap<Vec<&str>, Type>) -> TokenStream {
        let fields = edited_fields.into_iter().map(|(path, value_type)| {
            let (before, after, access) = if !path.is_empty() {
                let field = field_access(path.iter().copied());
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
                Type::String | Type::Font | Type::KeyEvent | Type::StyledText | Type::Brush => {
                    quote!(::xrack_runtime::TableHistoryPayload::history_payload_bytes)
                }
                _ => quote!(|_| 0),
            };
            quote!(patch.record_field(&[#(#path),*], #before, #after, #access, #same, #bytes);)
        });
        quote!(|before, after, patch| { #(#fields)* })
    }

    fn paste_field(
        &self,
        operand: ValueRef<'a>,
        edited: &mut BTreeMap<Vec<&'a str>, Type>,
    ) -> Result<TokenStream, String> {
        let (_, mut path) = xrack_mlir::record_field_path(operand)?;
        let target = if path.is_empty() {
            quote!(*__xr_next)
        } else {
            let field = field_access(path.iter().copied());
            quote!(__xr_next #field)
        };
        self.record_paste_leaves(&mut path, &operand.xr_type()?, edited);
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
        if let Some(flag) = self.action_state_flag(element, property)? {
            let receiver = projection.expression.receiver;
            return Ok(quote!(#receiver.#flag().unwrap_or(#default)));
        }
        let base = element
            .children()
            .find(|binding| {
                matches!(binding.name(), "xr.bind" | "xr.layout_bind")
                    && binding.attr("name") == Ok(property)
            })
            .map(|binding| self.table_projection_value(binding, projection))
            .transpose()?
            .unwrap_or(default);
        self.row_state_value(element, property, base, projection)
    }

    fn action_state_flag(
        &self,
        element: OperationRef<'a>,
        flag: &str,
    ) -> Result<Option<Ident>, String> {
        if !matches!(flag, "enabled" | "visible") || action_ref(element).is_none() {
            return Ok(None);
        }
        let Some(case) =
            children(element, "xr.state_set").find(|case| case.attribute("name") == Some(flag))
        else {
            return Ok(None);
        };
        let selector = self.operand(case, 0)?.attr("owner")?;
        if self
            .analysis
            .persistent_state_selectors
            .contains_key(selector)
        {
            Ok(Some(action_state_flag_method(element_key(element)?, flag)))
        } else {
            Ok(None)
        }
    }

    fn table_projection_value(
        &self,
        binding: OperationRef<'a>,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        if projection.expression.repeat_parameters {
            // Sampled rows supply their own item and index, outside a rendered row's cache.
            self.expr_in(self.operand(binding, 0)?, projection.expression)
        } else {
            self.visual_value_in(binding, &self.visuals, projection.expression.receiver)
        }
    }

    fn row_state_value(
        &self,
        element: OperationRef<'a>,
        property: &str,
        base: TokenStream,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        self.row_state_value_mapped(element, property, base, projection, |value| value)
    }

    fn row_state_value_mapped(
        &self,
        element: OperationRef<'a>,
        property: &str,
        base: TokenStream,
        projection: &TableProjection<'_, 'a>,
        map: impl Fn(TokenStream) -> TokenStream,
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
        let selected = if self.analysis.persistent_state_selectors.contains_key(owner) {
            let receiver = projection.expression.receiver;
            quote!(#receiver.#name())
        } else if projection.routed_input {
            let receiver = projection.expression.receiver;
            let choice = self.state_choice(selector, |condition| {
                self.expr_in(condition, projection.expression)
            })?;
            quote!(#receiver.__state.__binding_lifetime.input_state(#owner, || #choice))
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
            let value = map(self.expr_in(value, projection.expression)?);
            arms.extend(quote!(#index => #value,));
        }
        Ok(quote!(match #selected { #arms _ => #base }))
    }

    fn render_view_empty_state(
        &self,
        view: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
        name: Ident,
        model: Option<&TokenStream>,
    ) -> Result<Ident, String> {
        if xrack_resources::visual::keyed_list(view) {
            let index = list_selection_index_method(element_key(view)?);
            out.extend(quote!(#name.properties.insert("current-index", self.#index().into());));
        }
        if list_multi_selection(view)
            && let Some(model) = model
        {
            let selection = list_selection_field(element_key(view)?);
            out.extend(quote!(self.__state.#selection.populate_list_scene(&mut #name, #model);));
        }
        if self.key_scroll_targets.contains(element_key(view)?) {
            let locator = list_key_locator_method(element_key(view)?);
            out.extend(quote!(#name.row_locator = Some(self.#locator());));
        }
        if let Some(slot) = empty_state(view) {
            let mut content = TokenStream::new();
            let child = self.render(slot, scope, &mut content, next)?;
            out.extend(quote! {
                if #name.list.is_some_and(|list| list.row_count == 0) {
                    #content
                    #name.children.push(#child);
                }
            });
        }
        Ok(name)
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
            self.__state.#selection.populate_scene(&mut #name, #model);
        });
        Ok(())
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

    fn project_list_sections(
        &self,
        element: OperationRef<'a>,
        node: &Ident,
        model: &TokenStream,
    ) -> Result<TokenStream, String> {
        let Some(field) = self.record_field_binding(element, "section-field")? else {
            return Ok(TokenStream::new());
        };
        let field = field_access(field.split('.'));
        let state = format_ident!("__list_sections_{}", rust_key(element_key(element)?));
        let project = quote!(|row| (row #field).as_str());
        let projection = if has_list_visibility(element) {
            quote!(self.__state.#state.snapshot((#model).source(), Some((#model).positions()), #project))
        } else {
            quote!(self.__state.#state.snapshot(#model, None, #project))
        };
        Ok(quote!(#node.list_sections = Some(#projection);))
    }

    fn list_text_search(
        &self,
        element: OperationRef<'a>,
        node: &Ident,
        model: &TokenStream,
    ) -> Result<TokenStream, String> {
        if !matches!(
            element.attr("kind"),
            Ok("ListView" | "GridView" | "StandardListView")
        ) {
            return Ok(TokenStream::new());
        }
        let field = self.record_field_binding(element, "typeahead-field")?;
        if field.is_none()
            && !children(element, "xr.bind")
                .any(|binding| binding.attr("name") == Ok("typeahead-enabled"))
        {
            return Ok(TokenStream::new());
        }
        let standard = element.attr("kind") == Ok("StandardListView");
        let binding = if standard {
            model_binding(element)
        } else {
            Some(self.list_template(element)?.1)
        };
        let Some(binding) = binding else {
            return Ok(TokenStream::new());
        };
        let default_enabled = field.is_some();
        let label = match value_type(self.operand(binding, 0)?)? {
            Type::StringList => quote!(|row| row.as_str()),
            Type::RecordList(_) => {
                let field = if standard {
                    field
                        .or(self.record_field_binding(element, "text-role")?)
                        .or_else(|| Some("text".into()))
                } else {
                    field
                };
                let Some(field) = field else {
                    return Ok(TokenStream::new());
                };
                let field = field_access(field.split('.'));
                quote!(|row| (row #field).as_str())
            }
            _ => return Ok(TokenStream::new()),
        };
        let (source, order) = if has_list_visibility(element) {
            (
                quote!((#model).source().clone()),
                quote!(Some((#model).positions().clone())),
            )
        } else {
            (quote!((*#model).clone()), quote!(None))
        };
        Ok(quote! {
            if #node.boolean("typeahead-enabled", #default_enabled) {
                #node.list_text_search = Some(::xrack_runtime::ListTextSearch::new(#source, #order, #label));
            }
        })
    }

    fn table_model(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let rows = self.table_rows_model(element)?;
        Ok(quote!(&#rows))
    }

    fn table_rows_model(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        self.table_rows_model_in(element, &quote!(self))
    }

    fn table_rows_model_in(
        &self,
        element: OperationRef<'a>,
        receiver: &TokenStream,
    ) -> Result<TokenStream, String> {
        if element.attr("kind") == Ok("StandardTableView")
            || children(element, "xr.bind")
                .any(|binding| binding.attr("name") == Ok("row-visible-field"))
        {
            let rows = table_rows_model_method(element_key(element)?);
            return Ok(quote!(#receiver.#rows()));
        }
        let source = self.repeated_view_source_model(element, receiver)?;
        let order = table_row_order_field(element_key(element)?);
        Ok(quote!(#receiver.__state.#order.rows(#source)))
    }

    fn editable_table_rows_model(
        &self,
        table: OperationRef<'a>,
        receiver: &TokenStream,
        is_root: bool,
    ) -> Result<TokenStream, String> {
        let projection = TableProjection::for_input(receiver);
        let available = if is_root {
            let enabled = self.row_binding_or(table, "enabled", quote!(true), &projection)?;
            let conditions = self.row_conditions_met(table, &projection)?;
            let visible = self.row_binding_or(table, "visible", quote!(true), &projection)?;
            quote!((#enabled) && (#conditions) && #receiver.__state.__binding_lifetime.input_root_visible(|| #visible))
        } else {
            self.element_allows_editing(table, &projection)?
        };
        let rows = self.table_rows_model_in(table, receiver)?;
        Ok(quote!((#available).then(|| #rows)))
    }

    fn table_action_rows_model(
        &self,
        table: OperationRef<'a>,
        flag: &str,
        column_order: &TokenStream,
        is_root: bool,
    ) -> Result<TokenStream, String> {
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let enabled = self.row_binding_or(table, flag, quote!(false), &projection)?;
        let available = self.element_allows_routed_input(table, is_root, &projection)?;
        let order = table_order_field(element_key(table)?);
        let rows = self.table_rows_model(table)?;
        Ok(quote! {
            ((#enabled) && (#available)
                && self.__state.#order.get(::core::default::Default::default).eq(#column_order))
                .then(|| #rows)
        })
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
            self.repeated_view_source_model(element, &quote!(self))
        }
    }

    fn repeated_view_source_model(
        &self,
        element: OperationRef<'a>,
        receiver: &TokenStream,
    ) -> Result<TokenStream, String> {
        let (row, repeat) = self.list_template(element)?;
        let repeater = repeat_field(self.visuals.repeats[element_key(row)?].index());
        let model = self.expr_in(self.operand(repeat, 0)?, ExpressionContext::new(receiver))?;
        Ok(quote!(#receiver.__state.__visuals.#repeater.resolve_model(|| #model)))
    }

    fn view_index_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        if element.attr("kind") == Ok("TreeView") {
            let snapshot = tree_snapshot_method(key);
            return Ok(quote! {
                self.#snapshot().view_index(index, direction)
            });
        }
        if let Some(model) = self.projected_view_model(element)? {
            Ok(quote! {
                (#model).positions().view_index(index, direction)
            })
        } else {
            let model = self.view_source_model(element)?;
            Ok(quote!((index < ::xrack_runtime::ListModel::row_count(&*#model)).then_some(index)))
        }
    }

    fn projected_view_model(
        &self,
        element: OperationRef<'a>,
    ) -> Result<Option<TokenStream>, String> {
        if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView")) {
            Ok(Some(self.table_model(element)?))
        } else if has_list_visibility(element) {
            let model = visible_list_model(element_key(element)?);
            Ok(Some(quote!(&self.#model())))
        } else {
            Ok(None)
        }
    }

    fn view_count_body(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        let model = self.view_source_model(view)?;
        let source = quote!(::xrack_runtime::ListModel::row_count(&*#model));
        let rows = if view.attr("kind") == Ok("TreeView") {
            let snapshot = tree_snapshot_method(element_key(view)?);
            quote!(self.#snapshot().rows().len())
        } else if let Some(model) = self.projected_view_model(view)? {
            quote!((#model).positions().row_count())
        } else {
            source.clone()
        };
        Ok(quote!((if source { #source } else { #rows }) as ::core::primitive::i64))
    }

    fn list_selection_index(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        if xrack_resources::visual::keyed_list(view) {
            let method = list_selection_index_method(element_key(view)?);
            Ok(quote!(self.#method()))
        } else {
            self.control_value(view, "current-index", Type::Int)
        }
    }

    fn list_selection_value(
        &self,
        view: OperationRef<'a>,
        index: &TokenStream,
    ) -> Result<Option<TokenStream>, String> {
        if xrack_resources::visual::keyed_list(view) {
            let method = list_selection_key_method(element_key(view)?);
            Ok(Some(
                quote!(self.#method(#index as ::core::primitive::usize)),
            ))
        } else {
            Ok(None)
        }
    }

    fn list_selection_input(
        &self,
        view: OperationRef<'a>,
        available: &TokenStream,
        current: &TokenStream,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        if !list_multi_selection(view) {
            return Ok(TokenStream::new());
        }
        let state = list_selection_field(element_key(view)?);
        let rows = self.list_selection_rows(view)?;
        let multiple = self.row_binding_or(view, "multi-select", quote!(false), projection)?;
        let notify = self.list_selection_notifier(view)?;
        Ok(quote! {
            if node.table_selection.is_some() || matches!(action, ::xrack_runtime::ControlAction::TableSelection(_)) {
                let __xr_rows = ::std::cell::OnceCell::new();
                let __xr_model = || __xr_rows.get_or_init(|| #rows);
                let Some(__xr_change) = self.__state.#state.apply_control(
                    node, input, || #available, || __xr_model().clone(),
                    || ::xrack_runtime::TableSelectionOptions {
                        target: ::xrack_runtime::SelectionTarget::Rows,
                        multiple: #multiple,
                        order: ::core::default::Default::default(),
                        drag_select: false,
                    },
                    || #current,
                ) else { return false; };
                return __xr_change.notify(false, #notify);
            }
        })
    }

    fn list_selection_rows(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        if has_list_visibility(view) {
            let method = visible_list_model(element_key(view)?);
            Ok(quote!(self.#method()))
        } else {
            let model = self.view_source_model(view)?;
            Ok(quote!(::xrack_runtime::TableRows::from_source(#model)))
        }
    }

    fn list_selection_notifier(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        let selected = if let Some(field) = self.record_field_binding(view, "row-key-field")? {
            let field = field_access(field.split('.'));
            quote!(::xrack_runtime::ListModel::item(__xr_model(), __xr_row) #field)
        } else {
            quote!(__xr_index)
        };
        let set = self.set_control(view, &selected)?;
        let changed = self.typed_event(view, "changed", &quote!(__xr_index))?;
        let selected = self.typed_event(view, "selection-changed", &quote!(__xr_count))?;
        Ok(quote! {
                 |event| match event {
                    ::xrack_runtime::TableSelectionEvent::CurrentRowChanged(__xr_row) => {
                        let __xr_index = __xr_row as ::core::primitive::i64;
                        #set
                        #changed
                    }
                    ::xrack_runtime::TableSelectionEvent::SelectionChanged(count) => {
                        let __xr_count = count as ::core::primitive::i64;
                        #selected
                    }
                    _ => {}
                }
        })
    }

    fn list_selection_methods(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        let key = element_key(view)?;
        let reads = self.list_selection_info_targets.contains(key);
        let multiple_bound = list_multi_selection(view);
        let commands = multiple_bound || self.list_selection_command_targets.contains(key);
        if !reads && !commands {
            return Ok(TokenStream::new());
        }
        let rows = self.list_selection_rows(view)?;
        let current = self.list_selection_index(view)?;
        let multiple = self.binding_value_or(view, "multi-select", quote!(false))?;
        let state = list_selection_field(key);
        let mut methods = TokenStream::new();
        if reads {
            let info = list_selection_info_method(key);
            let state = if multiple_bound {
                quote!(::core::option::Option::Some(&self.__state.#state))
            } else {
                quote!(::core::option::Option::None)
            };
            methods.extend(quote! {
                fn #info(&self, row: ::core::primitive::i64) -> (::core::primitive::i64, ::core::primitive::bool) {
                    ::xrack_runtime::TableSelection::try_list_info(
                        #state,
                        || ::core::result::Result::Ok::<_, ::core::convert::Infallible>(#rows),
                        || ::core::result::Result::Ok(#current),
                        || ::core::result::Result::Ok(#multiple),
                        row,
                    ).unwrap_or_else(|error| match error {})
                }
            });
        }
        if commands {
            let method = list_selection_command_method(key);
            let notify = self.list_selection_notifier(view)?;
            let selection = if multiple_bound {
                quote!(self.__state.#state)
            } else {
                quote!(::xrack_runtime::TableSelection::default())
            };
            let key_source = if let Some(field) =
                self.record_field_binding(view, "row-key-field")?
            {
                let field = field_access(field.split('.'));
                let keys = format_ident!("__list_keys_{}", rust_key(key));
                quote!(|rows, key| self.__state.#keys.find(rows.source(), key, |row| row #field))
            } else {
                quote!(|_, _| ::core::option::Option::None)
            };
            methods.extend(quote! {
                fn #method(&self, command: ::xrack_runtime::ListSelectionCommand) -> ::core::primitive::bool {
                    let __xr_rows = ::std::cell::OnceCell::new();
                    let __xr_model = || __xr_rows.get_or_init(|| #rows);
                    let change = #selection.try_apply_list_command(
                        command,
                        || ::core::result::Result::Ok::<_, ::core::convert::Infallible>(__xr_model().clone()),
                        || ::core::result::Result::Ok(#current),
                        || ::core::result::Result::Ok(#multiple),
                        #key_source,
                    ).unwrap_or_else(|error| match error {});
                    change.is_some_and(|change| change.notify(false, #notify))
                }
            });
        }
        Ok(methods)
    }

    fn source_selection_body(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        if matches!(element.attr("kind"), Ok("TableView" | "StandardTableView")) {
            let select = table_selection_command_method(key);
            return Ok(quote! {
                let column = match column {
                    Some(column) => {
                        let Ok(column) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(column) else { return; };
                        Some(column)
                    }
                    None => None,
                };
                self.#select(::xrack_runtime::TableSelectionCommand::Source { row: source, column });
            });
        }
        if element.attr("kind") == Ok("TreeView") {
            let select = tree_selection_method(key);
            return Ok(
                quote!(self.#select(::xrack_runtime::TreeSelectionCommand::Source(source));),
            );
        }
        if list_multi_selection(element) {
            let select = list_selection_command_method(key);
            return Ok(quote! {
                self.#select(::xrack_runtime::ListSelectionCommand::Source(source));
            });
        }
        let lookup = view_index_method(key);
        let current = self.list_selection_index(element)?;
        let value = quote!(index);
        let (prepare, selected) = match self.list_selection_value(element, &value)? {
            Some(key) => (
                quote!(let Some(selection) = #key else { return; };),
                quote!(selection),
            ),
            None => (TokenStream::new(), value.clone()),
        };
        let set = self.set_control(element, &selected)?;
        let changed = self.typed_event(element, "changed", &value)?;
        Ok(quote! {
            if let Some(index) = ::xrack_runtime::prepare_source_selection(
                || self.#lookup(source, ::xrack_runtime::ViewIndex::FromSource), || #current,
            ) {
                #prepare
                #set
                #changed
            }
        })
    }

    fn render_combo_model(
        &self,
        element: OperationRef<'a>,
        combo: &RecordComboRef<'a>,
        scope: &VisualScope<'_>,
        node: &Ident,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        let items = self.combo_items(element, combo, scope)?;
        let populate = if matches!(element.attribute("kind"), Some("Tumbler" | "PathView")) {
            format_ident!("populate_tumbler")
        } else {
            format_ident!("populate_scene")
        };
        out.extend(quote!((#items).#populate(&mut #node);));
        Ok(())
    }

    fn combo_items(
        &self,
        element: OperationRef<'a>,
        combo: &RecordComboRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let label = field_access(combo.label.split('.'));
        let value = field_access(combo.value.split('.'));
        let key = element_key(element)?;
        let items = combo_items_field(key);
        let model = self.visual_value(combo.model, scope)?;
        let project_label = quote!(|record| record #label.as_str());
        let project_value = quote!(|record| ::xrack_runtime::Value::from(record #value.clone()));
        let items_input = if let Some(enabled) = &combo.enabled {
            let enabled = field_access(enabled.split('.'));
            quote!(::xrack_runtime::ComboItems::with_enabled(#model, #project_label, #project_value, |record| record #enabled))
        } else {
            quote!(::xrack_runtime::ComboItems::new(#model, #project_label, #project_value))
        };
        Ok(quote!(self.__state.#items.get(|| #items_input)))
    }

    fn combo_state_model(
        &self,
        element: OperationRef<'a>,
        record: Option<&RecordComboRef<'a>>,
    ) -> Result<TokenStream, String> {
        let model = if let Some(record) = record {
            let items = self.combo_items(element, record, &self.visuals)?;
            if matches!(element.attribute("kind"), Some("Tumbler" | "PathView")) {
                quote!(::xrack_runtime::ComboStateModel::Tumbler(#items))
            } else {
                quote!(::xrack_runtime::ComboStateModel::Records(#items))
            }
        } else {
            let model = self.binding_value_or(
                element,
                "model",
                quote!(::xrack_runtime::VecModel::<::std::string::String>::default()),
            )?;
            quote!(::xrack_runtime::ComboStateModel::Strings(#model))
        };
        Ok(model)
    }

    fn combo_state_value(
        &self,
        element: OperationRef<'a>,
        property: &str,
    ) -> Result<(Type, TokenStream), String> {
        let key = element_key(element)?;
        let record = self.record_combos.get(key);
        let (stored, stored_type) =
            controlled_property(element).ok_or("missing ComboBox selection")?;
        if property == "current-value" && stored == "current-value" {
            let value = self.control_value(element, stored, stored_type.clone())?;
            return Ok((stored_type, value));
        }
        let draft = if property == "edit-text" && combo_editor(element) {
            if !default_combo_edit_text(element) {
                return Ok((
                    Type::String,
                    self.control_value(element, "edit-text", Type::String)?,
                ));
            }
            let field = control_field(key, "edit-text");
            quote! {
                if let ::core::option::Option::Some(text) = self.__state.#field.explicit_value() {
                    return text;
                }
            }
        } else {
            TokenStream::new()
        };
        let model = self.combo_state_model(element, record)?;
        if property == "count" {
            return Ok((Type::Int, quote!((#model).count() as i64)));
        }
        let value = self.control_value(element, stored, stored_type)?;
        let selection = if stored == "current-value" {
            quote!(__xr_model.selected_index(0, ::core::option::Option::Some(&::xrack_runtime::Value::from(#value))))
        } else {
            quote!(__xr_model.selected_index(#value, ::core::option::Option::None))
        };
        let (ty, result) = match property {
            "current-index" => (
                Type::Int,
                quote!(__xr_index.map_or(-1_i64, |index| index as i64)),
            ),
            "current-value" => {
                let ty = record.map_or(Ok(Type::String), |combo| {
                    self.record_field_type(combo.record.clone(), &combo.value)
                })?;
                let variant = combo_value_variant(&ty)?;
                (
                    ty,
                    quote! {
                        __xr_index.and_then(|index| match __xr_model.value_at(index) {
                            ::core::option::Option::Some(::xrack_runtime::Value::#variant(value)) =>
                                ::core::option::Option::Some(value),
                            _ => ::core::option::Option::None,
                        }).unwrap_or_default()
                    },
                )
            }
            _ => (
                Type::String,
                quote!(
                    __xr_index
                        .and_then(|index| __xr_model.text_at(index))
                        .unwrap_or_default()
                ),
            ),
        };
        Ok((
            ty,
            quote! {
                #draft
                let __xr_model = #model;
                let __xr_index = #selection;
                #result
            },
        ))
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
        let duration = animation.duration_ms;
        let delay = animation.delay_ms;
        let easing = match animation.curve {
            Some([x1, y1, x2, y2]) => {
                quote!(::xrack_runtime::Easing::cubic_bezier(#x1, #y1, #x2, #y2).unwrap())
            }
            None => quote!(::xrack_runtime::Easing::LINEAR),
        };
        let timing = quote!(::xrack_runtime::Animation::new(#duration, #delay, #easing));
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
            if !self.analysis.visual_state_selectors.contains(owner) {
                return Ok(());
            }
            let name = state_selector_name(owner);
            let value = if self.analysis.persistent_state_selectors.contains_key(owner) {
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
        let instance = element.name() == "xr.instance";
        for (&key, cases) in overrides {
            let selected = self.state_selector(cases[0])?;
            let wrap = |value: TokenStream| quote!(Some(::xrack_runtime::Value::from(#value)));
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
            let base_op = if instance {
                "xr.layout_bind"
            } else {
                "xr.bind"
            };
            let base = children(element, base_op)
                .find(|binding| binding.attribute("name") == Some(key))
                .map(|binding| self.visual_value(binding, scope).map(wrap))
                .transpose()?
                .unwrap_or_else(|| quote!(None));
            let value = if let Some(flag) = self.action_state_flag(element, key)? {
                quote!(self.#flag().map(::xrack_runtime::Value::from))
            } else {
                quote!(match #selected { #choices _ => #base })
            };
            let value = if key == "enabled" && action {
                quote!(#node.action_state_override(#key).or_else(|| #value))
            } else {
                value
            };
            let set = if instance {
                quote!(#node.set_instance_layout_property(#key, value);)
            } else {
                quote!(#node.properties.insert(#key, value);)
            };
            out.extend(quote! {
                if let Some(value) = #value {
                    #set
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
                #node.set_state_timing(#key, ::xrack_runtime::StateTiming {
                    case: #selected,
                    enter: match #selected { #enter _ => None },
                    exit: match #selected { #exit _ => None },
                });
            });
        }
        Ok(())
    }

    fn group_option_model(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<Option<TokenStream>, String> {
        let Some(template) = scene_children(element).next() else {
            return Ok(None);
        };
        self.group_template_model(template, scope)
    }

    fn group_template_model(
        &self,
        template: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<Option<TokenStream>, String> {
        let Some(model) = children(template, "xr.repeat").next() else {
            return Ok(None);
        };
        if self.empty_repeat(template) || self.static_subtree(template) {
            return Ok(None);
        }
        let repeat = &scope.repeats[element_key(template)?];
        let field = repeat_field(repeat.index());
        let receiver = scope.receiver();
        let source = self.visual_expr(self.operand(model, 0)?, scope)?;
        Ok(Some(quote!(#receiver.#field.resolve_model(|| #source))))
    }

    fn live_group_option(
        &self,
        option: OperationRef<'a>,
        scope: &VisualScope<'_>,
        available: TokenStream,
        read: GroupOptionRead,
    ) -> Result<TokenStream, String> {
        let label = match read {
            GroupOptionRead::Select(true) => self.live_radio_value(option, scope)?,
            GroupOptionRead::Select(false) => quote!(::std::string::String::new()),
            GroupOptionRead::Close => {
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let closable = self.row_input_flag(
                    option,
                    "closable",
                    quote!(__xr_tabs_closable()),
                    scope,
                    &projection,
                    false,
                )?;
                let value = option
                    .children()
                    .find(|binding| {
                        binding.name() == "xr.bind" && binding.attr("name") == Ok("value")
                    })
                    .map(|binding| self.visual_value(binding, scope))
                    .transpose()?
                    .unwrap_or_else(|| quote!(::std::string::String::new()));
                return Ok(quote! { ((#available) && (#closable)).then(|| #value) });
            }
        };
        Ok(quote! {
            (#available).then(|| if read_label { #label } else { ::std::string::String::new() })
        })
    }

    fn live_radio_value(
        &self,
        option: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        Ok(["value", "text"]
            .into_iter()
            .find_map(|name| {
                children(option, "xr.bind").find(|binding| binding.attr("name") == Ok(name))
            })
            .map(|binding| self.visual_value(binding, scope))
            .transpose()?
            .unwrap_or_else(|| quote!(::std::string::String::new())))
    }

    fn radio_choice(&self, group: OperationRef<'a>) -> Result<TokenStream, String> {
        let (property, ty) = controlled_property(group).ok_or("missing radio selection")?;
        let current = self.control_value(group, property, ty)?;
        let selection = if property == "current-value" {
            quote!(Value)
        } else {
            quote!(Index)
        };
        let mut blocks = TokenStream::new();
        let receiver = quote!(self);
        for (declaration, option) in scene_children(group).enumerate() {
            if self.empty_repeat(option) {
                continue;
            }
            let repeat = children(option, "xr.repeat").next();
            let projection = TableProjection::new(ExpressionContext {
                repeat_parameters: repeat.is_some(),
                ..ExpressionContext::new(&receiver)
            });
            let available = self.element_allows_editing(option, &projection)?;
            let property =
                if children(option, "xr.bind").any(|binding| binding.attr("name") == Ok("value")) {
                    "value"
                } else {
                    "text"
                };
            let label = self.row_binding_or(
                option,
                property,
                quote!(::std::string::String::new()),
                &projection,
            )?;
            let selectors = self.input_selectors(&projection)?;
            let (init, count, parameters) = if let Some(repeat) = repeat {
                self.sampled_group_options(option, repeat)?
            } else {
                (TokenStream::new(), quote!(1usize), TokenStream::new())
            };
            blocks.extend(quote! {
                if matches!(&selection, ::xrack_runtime::ButtonGroupSelection::Index(index) if *index < 0) {
                    return ::core::option::Option::None;
                }
                {
                    #init
                    if let ::core::option::Option::Some(row) = selection.select_radio_option(
                        #count, |__xr_index, read_label| {
                            #parameters
                            #selectors
                            (#available).then(|| {
                                if read_label { #label } else { ::std::string::String::new() }
                            })
                        },
                    ) {
                        return ::core::option::Option::Some((#declaration, row));
                    }
                }
            });
        }
        Ok(quote! {
            let mut selection = ::xrack_runtime::ButtonGroupSelection::#selection(#current);
            #blocks
            ::core::option::Option::None
        })
    }

    fn sampled_group_options(
        &self,
        option: OperationRef<'a>,
        repeat: OperationRef<'a>,
    ) -> Result<(TokenStream, TokenStream, TokenStream), String> {
        let model = match self.group_template_model(option, &self.visuals)? {
            Some(model) => model,
            None => {
                let value = self.expr(self.operand(repeat, 0)?)?;
                quote!(::std::rc::Rc::new(#value))
            }
        };
        let repeat = RepeatRef::from_operation(repeat)?;
        let item = parameter_ident(repeat.item);
        let index = repeat.index.map(|name| {
            let index = parameter_ident(name);
            quote!(let #index = || __xr_index as i64;)
        });
        Ok((
            quote!(let model = #model;),
            quote!(::xrack_runtime::ListModel::row_count(&*model).min(4096)),
            quote! {
                let #item = || ::xrack_runtime::ListModel::item(&*model, __xr_index);
                #index
            },
        ))
    }

    fn navigation_choice(&self, group: OperationRef<'a>) -> Result<TokenStream, String> {
        let by_value = navigation_selects_by_value(group);
        let layout = group.attr("kind") == Ok("StackLayout");
        let exact_index = layout || group.attr("kind") == Ok("TabWidget");
        let (property, ty) = controlled_property(group).ok_or("missing navigation selection")?;
        let current = self.control_value(group, property, ty)?;
        let receiver = quote!(self);
        let resolve = |options: TokenStream| {
            if by_value {
                quote!(::xrack_runtime::resolve_navigation_value(&requested, #options))
            } else {
                quote!(::xrack_runtime::resolve_swipe_index(::core::option::Option::Some(requested), #options))
            }
        };
        if let Some(option) = scene_children(group).next()
            && let Some(repeat) = children(option, "xr.repeat").next()
        {
            if self.empty_repeat(option) {
                return Ok(quote!(::core::option::Option::None));
            }
            let (init, count, parameters) = self.sampled_group_options(option, repeat)?;
            let projection = TableProjection::new(ExpressionContext {
                repeat_parameters: true,
                ..ExpressionContext::new(&receiver)
            });
            let available = if layout {
                quote!(true)
            } else {
                self.element_allows_editing(option, &projection)?
            };
            let selectors = self.input_selectors(&projection)?;
            let option = if by_value {
                let label = self.row_binding_or(
                    option,
                    "value",
                    quote!(::std::string::String::new()),
                    &projection,
                )?;
                quote!((#available).then(|| #label))
            } else {
                available
            };
            if exact_index && !by_value {
                return Ok(quote! {
                    let requested = #current;
                    #init
                    let __xr_index = ::core::primitive::usize::try_from(requested).ok()?;
                    if __xr_index >= #count { return ::core::option::Option::None; }
                    #parameters
                    #selectors
                    (#option).then_some(__xr_index)
                });
            }
            let options = resolve(quote!((0..#count).map(|__xr_index| {
                #parameters
                #selectors
                #option
            })));
            return Ok(quote! {
                let requested = #current;
                #init
                #options
            });
        }
        let projection = TableProjection::new(ExpressionContext::new(&receiver));
        let options = scene_children(group)
            .map(|option| {
                let available = if layout {
                    quote!(true)
                } else {
                    self.element_allows_editing(option, &projection)?
                };
                if by_value {
                    let label = self.live_radio_value(option, &self.visuals)?;
                    Ok(quote!((#available).then(|| #label)))
                } else {
                    Ok(available)
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        let slots = 0..options.len();
        let count = options.len();
        let missing = if by_value {
            quote!(::core::option::Option::None)
        } else {
            quote!(false)
        };
        let eligible = quote!(|slot| match slot { #(#slots => #options,)* _ => #missing });
        if exact_index && !by_value {
            return Ok(
                quote!(::core::primitive::usize::try_from(#current).ok().filter(|index| (#eligible)(*index))),
            );
        }
        let visible = if group.attr("kind") == Ok("SwipeView") {
            self.row_binding_or(group, "visible", quote!(true), &projection)?
        } else {
            quote!(true)
        };
        let selected = resolve(quote!((0..#count).map(#eligible)));
        Ok(quote! {
            if !(#visible) { return ::core::option::Option::None; }
            let requested = #current;
            #selected
        })
    }

    fn virtual_radio_choice(&self, view: OperationRef<'a>) -> Result<TokenStream, String> {
        let index = self.list_selection_index(view)?;
        let current = quote!(::core::primitive::usize::try_from(#index).ok());
        if has_list_visibility(view) {
            let visible = visible_list_model(element_key(view)?);
            Ok(quote!((#current).and_then(|index| self.#visible().positions().source_index(index))))
        } else {
            Ok(current)
        }
    }

    fn group_option_reader(
        &self,
        element: OperationRef<'a>,
        read: GroupOptionRead,
    ) -> Result<TokenStream, String> {
        if let Some(template) = scene_children(element).next()
            && let Some(model) = children(template, "xr.repeat").next()
        {
            if self.empty_repeat(template) || self.static_subtree(template) {
                if matches!(read, GroupOptionRead::Close) {
                    return Ok(quote! {
                        |_, slot, _| ::xrack_runtime::group_item(node, slot)
                            .filter(|item| if item.properties.contains_key("closable") {
                                item.boolean("closable", false)
                            } else {
                                __xr_tabs_closable()
                            })
                            .map(|item| item.text("value").unwrap_or("").to_owned())
                    });
                }
                return Ok(quote! {
                    |_, slot, read_label| ::xrack_runtime::group_item(node, slot).map(|item| {
                        if read_label { ::xrack_runtime::radio_value(item).to_owned() } else { ::std::string::String::new() }
                    })
                });
            }
            if repeated_navigation_page(element).is_some() {
                let describe = self.sample_repeated_navigation_option(template, model, read)?;
                return Ok(quote!(|model, slot, read_label| {
                    let model = &**model;
                    let __xr_index = slot;
                    #describe
                }));
            }
            let option = self.repeated_group_option(template, model, read, &quote!(slot))?;
            return Ok(quote!(|model, slot, read_label| #option));
        }

        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let mut options = TokenStream::new();
        for (slot, child) in static_children(element) {
            let available = self.element_allows_editing(child, &projection)?;
            let option = self.live_group_option(child, &self.visuals, available, read)?;
            options.extend(quote!(#slot => #option,));
        }
        Ok(quote!(|_, slot, read_label| match slot { #options _ => ::core::option::Option::None }))
    }

    fn repeated_group_option(
        &self,
        template: OperationRef<'a>,
        repeat_op: OperationRef<'a>,
        read: GroupOptionRead,
        row: &TokenStream,
    ) -> Result<TokenStream, String> {
        let repeat = &self.visuals.repeats[element_key(template)?];
        let field = repeat_field(repeat.index());
        let row_state = row_local(repeat.index());
        let parameters = RepeatRef::from_operation(repeat_op)?;
        let item = parameter_ident(parameters.item);
        let bind_index = parameters.index.map(|index| {
            let index = parameter_ident(index);
            quote!(let #index = __xr_row_index;)
        });
        let available = self.item_delegate_available(template, &repeat.row)?;
        let option = self.live_group_option(template, &repeat.row, available, read)?;
        Ok(quote! {{
            let key = node.child_at(slot)?.row_key?;
            self.__state.__visuals.#field.with_resolved_row_state(
                model, #row, key,
                |#item, __xr_row_index, #row_state| {
                    let __xr_index = #row;
                    #bind_index
                    #option
                },
            ).flatten()
        }})
    }

    fn radio_group_options(
        &self,
        element: OperationRef<'a>,
    ) -> Result<(TokenStream, TokenStream), String> {
        let mut sampling = TokenStream::new();
        let mut options = TokenStream::new();
        let mut models = Vec::new();
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        for (declaration, child) in scene_children(element).enumerate() {
            let option = if let Some(repeat) = children(child, "xr.repeat").next() {
                let Some(source) = self.group_template_model(child, &self.visuals)? else {
                    continue;
                };
                let sample = format_ident!("__xr_radio_model_{declaration}");
                let model = Literal::usize_unsuffixed(models.len());
                models.push(sample.clone());
                sampling.extend(quote! {
                    let #sample = if #declaration <= __xr_declaration {
                        let model = #source;
                        if !node.radio_option_model_is_current(#declaration, &*model) {
                            return ::core::option::Option::None;
                        }
                        ::core::option::Option::Some(model)
                    } else {
                        ::core::option::Option::None
                    };
                });
                let repeated = self.repeated_group_option(
                    child,
                    repeat,
                    GroupOptionRead::Select(true),
                    &quote!(sample.1),
                )?;
                quote!({
                    let model = sample.2.#model.as_ref()?;
                    #repeated
                })
            } else {
                let available = self.element_allows_editing(child, &projection)?;
                self.live_group_option(
                    child,
                    &self.visuals,
                    available,
                    GroupOptionRead::Select(true),
                )?
            };
            options.extend(quote!(#declaration => #option,));
        }
        Ok((
            quote!({
                let (__xr_declaration, __xr_row) = node.radio_option_location(index)?;
                #sampling
                ::core::option::Option::Some((__xr_declaration, __xr_row, (#(#models,)*)))
            }),
            quote!(|sample, slot, read_label| match sample.0 {
                #options
                _ => ::core::option::Option::None,
            }),
        ))
    }

    fn sample_repeated_navigation_option(
        &self,
        page: OperationRef<'a>,
        repeat: OperationRef<'a>,
        read: GroupOptionRead,
    ) -> Result<TokenStream, String> {
        let (_, _, parameters) = self.sampled_group_options(page, repeat)?;
        let component = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&component)
        });
        let available = self.element_allows_editing(page, &projection)?;
        let (available, read_value) = if matches!(read, GroupOptionRead::Close) {
            let closable =
                self.row_binding_or(page, "closable", quote!(__xr_tabs_closable()), &projection)?;
            (quote!((#available) && (#closable)), quote!(true))
        } else {
            (available, quote!(read_label))
        };
        let value = self.row_binding_or(
            page,
            "value",
            quote!(::std::string::String::new()),
            &projection,
        )?;
        let selectors = self.input_selectors(&projection)?;
        Ok(quote! {
            #parameters
            #selectors
            (#available).then(|| if #read_value { #value } else { ::std::string::String::new() })
        })
    }

    fn describe_repeated_navigation_page(
        &self,
        page: OperationRef<'a>,
        repeat: OperationRef<'a>,
    ) -> Result<TokenStream, String> {
        let (_, _, parameters) = self.sampled_group_options(page, repeat)?;
        let component = quote!(self);
        let projection = TableProjection::new(ExpressionContext {
            repeat_parameters: true,
            ..ExpressionContext::new(&component)
        });
        let visible = self.row_binding_or(page, "visible", quote!(true), &projection)?;
        let enabled = self.row_binding_or(page, "enabled", quote!(true), &projection)?;
        let kind = page.attr("kind")?;
        let mut properties = TokenStream::new();
        for property in [
            "title",
            "value",
            "accessible-name",
            "accessible-description",
        ] {
            let optional = |value| quote!(::core::option::Option::Some(#value));
            let base = children(page, "xr.bind")
                .find(|binding| binding.attribute("name") == Some(property))
                .map(|binding| {
                    self.table_projection_value(binding, &projection)
                        .map(optional)
                })
                .transpose()?
                .unwrap_or_else(|| quote!(::core::option::Option::<::std::string::String>::None));
            let value = self.row_state_value_mapped(page, property, base, &projection, optional)?;
            properties.extend(quote! {
                if let ::core::option::Option::Some(value) = #value {
                    __xr_page.properties.insert(#property, ::xrack_runtime::Value::from(value));
                }
            });
        }
        if kind == "StackItem" {
            for property in [
                "width",
                "height",
                "min-width",
                "min-height",
                "preferred-width",
                "preferred-height",
                "max-width",
                "max-height",
            ] {
                let optional = |value| quote!(::core::option::Option::Some(::xrack_runtime::Value::from(#value)));
                let base = children(page, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok(property))
                    .map(|binding| {
                        self.table_projection_value(binding, &projection)
                            .map(optional)
                    })
                    .transpose()?
                    .unwrap_or_else(|| {
                        quote!(::core::option::Option::<::xrack_runtime::Value>::None)
                    });
                let value =
                    self.row_state_value_mapped(page, property, base, &projection, optional)?;
                properties.extend(quote! {
                    if let ::core::option::Option::Some(value) = #value {
                        __xr_page.properties.insert(#property, value);
                    }
                });
            }
        }
        if kind == "Tab" {
            let optional = |value| quote!(::core::option::Option::Some(#value));
            let base = children(page, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("closable"))
                .map(|binding| {
                    self.table_projection_value(binding, &projection)
                        .map(optional)
                })
                .transpose()?
                .unwrap_or_else(|| quote!(::core::option::Option::<::core::primitive::bool>::None));
            let closable =
                self.row_state_value_mapped(page, "closable", base, &projection, optional)?;
            properties.extend(quote! {
                if let ::core::option::Option::Some(flag) = #closable {
                    __xr_page.properties.insert("closable", ::xrack_runtime::Value::from(flag));
                }
            });
        }
        let selectors = self.input_selectors(&projection)?;
        Ok(quote! {
            #parameters
            #selectors
            let mut __xr_page = ::xrack_runtime::Node::new(#kind, ::core::option::Option::None);
            __xr_page.properties.insert("visible", ::xrack_runtime::Value::from(#visible));
            __xr_page.properties.insert("enabled", ::xrack_runtime::Value::from(#enabled));
            #properties
            __xr_page
        })
    }

    fn append_repeated_navigation_pages(
        &self,
        page: OperationRef<'a>,
        repeat_op: OperationRef<'a>,
        scope: &VisualScope<'_>,
        name: &Ident,
        out: &mut TokenStream,
        next: &mut usize,
    ) -> Result<(), String> {
        let repeat = &scope.repeats[element_key(page)?];
        let field = repeat_field(repeat.index());
        let receiver = scope.receiver();
        let model = self
            .group_template_model(page, scope)?
            .ok_or("missing navigation page model")?;
        let factory = self.repeat_delegate_factory(repeat, repeat_op, None)?;
        let draw = self.repeat_row(page, repeat, repeat_op, next)?;
        let describe = self.describe_repeated_navigation_page(page, repeat_op)?;
        out.extend(quote! {
            #name.append_navigation_pages_with_preview(
                || #model,
                |model, __xr_index| {
                    let model = &**model;
                    #describe
                },
                |model, active| #receiver.#field.render_resolved_indices(&**model, active.into_iter().flatten(), None, #factory, #draw),
            );
        });
        Ok(())
    }

    fn render_element(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
        render_content: RenderContent,
    ) -> Result<Ident, String> {
        let mut guards = children(element, "xr.show_if").peekable();
        if guards.peek().is_none() {
            return self.render_element_body(element, scope, out, next, render_content);
        }
        let close = element
            .attribute("kind")
            .and_then(picker_value)
            .map(|_| {
                let popup = picker_field(element_key(element)?);
                Ok::<_, String>(quote!(self.__state.#popup.close();))
            })
            .transpose()?;
        if self.use_visual_plan
            && guards
                .peek()
                .and_then(|guard| guard.attribute("xr.static_hidden"))
                == Some("true")
        {
            let name = format_ident!("__xr_hidden_{}", *next);
            *next += 1;
            out.extend(quote!(let #name = { #close ::xrack_runtime::Node::hidden() };));
            return Ok(name);
        }
        let guards = guards
            .map(|guard| self.visual_value(guard, scope))
            .collect::<Result<Vec<_>, _>>()?;
        let mut body = TokenStream::new();
        let name = self.render_element_body(element, scope, &mut body, next, render_content)?;
        out.extend(quote! {
            let #name = if #((#guards) &&)* true {
                #body
                #name
            } else {
                #close
                ::xrack_runtime::Node::hidden()
            };
        });
        Ok(name)
    }

    fn render_element_body(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        out: &mut TokenStream,
        next: &mut usize,
        render_content: RenderContent,
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
                #name.set_static_id(#id);
            });
            "xr.layout_bind"
        } else {
            let (kind, application_window) = scene_element(element.attr("kind")?);
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
                    ::xrack_runtime::ListRowRenderer::with_columns_in_scene(move |__range, __xr_columns, __xr_scene| {
                        __xr_weak.upgrade().map_or_else(::std::vec::Vec::new, |__state| {
                            Self { __state }.#method(__range, __xr_columns, __xr_scene)
                        })
                    })
                }).clone());
            });
        }
        if self.section_headers.contains_key(key) {
            let field = section_renderer_field(key);
            let method = section_header_method(key);
            out.extend(quote! {
                #name.list_section_renderer = Some(self.__state.#field.get_or_init(|| {
                    let __xr_weak = ::std::rc::Rc::downgrade(&self.__state);
                    ::xrack_runtime::ListSectionRenderer::new(move |__xr_section| {
                        let __state = __xr_weak.upgrade()?;
                        Some(Self { __state }.#method(__xr_section))
                    })
                }).clone());
            });
        }
        if self.combo_options.contains_key(key) {
            let method = combo_option_method(key);
            out.extend(quote! {
                #name.combo_option_renderer = ::core::option::Option::Some(self.__state.#method.get_or_init(|| {
                    let __xr_weak = ::std::rc::Rc::downgrade(&self.__state);
                    ::xrack_runtime::ComboOptionRenderer::new(move |__xr_option| {
                        let __state = __xr_weak.upgrade()?;
                        ::core::option::Option::Some(Self { __state }.#method(__xr_option))
                    })
                }).clone());
            });
        }
        if self.focus_targets.contains(key) {
            let handle = focus_handle(key);
            out.extend(quote!(#name.focus_handle = Some(self.__state.#handle.clone());));
        }
        if self.combo_popup_targets.contains(key) {
            let handle = combo_popup_handle(key);
            out.extend(quote!(#name.combo_popup_handle = ::core::option::Option::Some(self.__state.#handle.clone());));
        }
        if self.range_position_targets.contains_key(key) {
            let handle = range_position_handle(key);
            out.extend(quote!(#name.range_position_handle = ::core::option::Option::Some(self.__state.#handle.clone());));
        }
        if self.button_press_targets.contains(key) {
            let handle = button_press_handle(key);
            out.extend(quote!(#name.button_press_handle = ::core::option::Option::Some(self.__state.#handle.clone());));
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
            if children(element, "xr.handler")
                .any(|handler| handler.attr("name") == Ok("scroll-event"))
            {
                out.extend(quote!(#name.scroll_event_handler = true;));
            }
        }
        if children(element, "xr.handler")
            .any(|handler| handler.attr("name") == Ok("press-and-hold"))
        {
            out.extend(quote!(#name.press_and_hold_handler = true;));
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
            if element.name() == "xr.instance" {
                out.extend(
                    quote!(#name.set_instance_geometry_handle(self.__state.#handle.clone());),
                );
            } else {
                out.extend(quote!(#name.geometry_handle = Some(self.__state.#handle.clone());));
            }
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
        let record_combo = self.record_combos.get(key);
        for binding in children(element, binding_op) {
            let key = binding.attr("name")?;
            if binding_op == "xr.bind"
                && matches!(
                    key,
                    "content-x"
                        | "content-y"
                        | "row-height-field"
                        | "section-field"
                        | "typeahead-field"
                        | "row-key-field"
                        | "row-visible-field"
                        | "row-header-field"
                        | "clipboard-text"
                )
            {
                continue;
            }
            if element
                .attribute("kind")
                .is_some_and(|kind| text_submission_rule(kind, key))
            {
                continue;
            }
            if state_overrides.contains_key(key) {
                continue;
            }
            if record_combo.is_some()
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
            if key == "model" && matches!(element.attr("kind"), Ok("Tumbler" | "PathView")) {
                out.extend(quote! {
                    #name.set_tumbler_items(::xrack_runtime::TumblerItems::new(#value));
                });
                continue;
            }
            if key == "model" && element.attr("kind") == Ok("ComboBox") {
                out.extend(quote!(#name.set_string_combo_model(#value);));
                continue;
            }
            if binding_op == "xr.layout_bind" {
                out.extend(quote!(#name.set_instance_layout_property(#key, ::xrack_runtime::Value::from(#value));));
            } else {
                out.extend(
                    quote!(#name.properties.insert(#key, ::xrack_runtime::Value::from(#value));),
                );
            }
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
            let value = self.control_value_in(element, property, ty, scope)?;
            out.extend(quote! {
                #name.properties.insert(#property, ::xrack_runtime::Value::from(#value));
            });
        }
        if let Some(combo) = record_combo {
            self.render_combo_model(element, combo, scope, &name, out)?;
        }
        self.render_state_overrides(element, scope, &name, &state_overrides, out)?;
        if action.is_some() {
            out.extend(quote! {
                let #action_enabled = #action_enabled && #name.boolean("enabled", true);
                #name.properties.insert("enabled", ::xrack_runtime::Value::Bool(#action_enabled));
            });
        }
        if element.attribute("kind") == Some("MediaPlayer") {
            let slot = media_slot(key);
            let source = xrack_resources::visual::media_source_file(element, self.entry_file);
            let source = source.as_ref();
            out.extend(quote!(self.__state.#slot.populate(&mut #name, #source);));
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
                    #name.set_animation(#key, timing);
                }
            });
        }
        let kind = element.attribute("kind");
        let source_scroll = self.source_scroll_targets.contains(key);
        let mut effects = TokenStream::new();
        for timer in element_children(element).filter(|child| child.attr("kind") == Ok("Timer")) {
            let register = timer_member(element_key(timer)?, "register");
            effects.extend(quote! { #name.timers.extend(self.#register()); });
        }
        for effect in
            element_children(element).filter(|child| child.attr("kind") == Ok("SoundEffect"))
        {
            let slot = media_slot(element_key(effect)?);
            let file = xrack_resources::visual::media_source_file(effect, self.entry_file);
            let file = file.as_ref();
            let source =
                self.binding_value_or(effect, "source", quote!(::std::string::String::new()))?;
            let volume = self.binding_value_or(effect, "volume", quote!(1.0_f64))?;
            let muted = self.binding_value_or(effect, "muted", quote!(false))?;
            let loops = children(effect, "xr.bind")
                .find(|binding| binding.attribute("name") == Some("loops"))
                .map(|binding| {
                    let value = self.visual_value(binding, &self.visuals)?;
                    Ok::<_, String>(quote!(.with_sound_loops(#value)))
                })
                .transpose()?;
            effects.extend(quote! {
                #name.sound_effects.push(self.__state.#slot.sound_effect(#file, &#source, #volume, #muted)#loops);
            });
        }
        for dialog in element_children(element).filter(|child| native_dialog(*child)) {
            let register = native_dialog_member(element_key(dialog)?, "register");
            effects.extend(quote!(#name.native_dialogs.extend(self.#register());));
        }
        if kind == Some("Expander") {
            out.extend(quote! { if !#name.expander_collapsed() { #effects } });
        } else {
            out.extend(effects);
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
        if matches!(kind, Some("TableView" | "StandardTableView")) {
            let mut parts = TokenStream::new();
            for (index, kind) in xrack_syntax::builtins::TABLE_SELECTION_PARTS
                .iter()
                .enumerate()
            {
                if let Some(part) =
                    element_children(element).find(|child| child.attr("kind") == Ok(*kind))
                {
                    let child = self.render(part, scope, &mut parts, next)?;
                    parts.extend(quote!(#name.set_table_selection_part(#index, #child);));
                }
            }
            if !parts.is_empty() {
                out.extend(quote! {
                    if #name.boolean("drag-select", false) && #name.boolean("multi-select", false) {
                        #parts
                    }
                });
            }
        }
        if has_list_visibility(element) {
            let model = visible_list_model(element_key(element)?);
            let projected = self
                .project_row_heights(element, &name, &quote!(&*__xr_view_model), None)?
                .map(|value| quote!(#name.row_heights = #value;));
            let sections =
                self.project_list_sections(element, &name, &quote!(&*__xr_view_model))?;
            let search = self.list_text_search(element, &name, &quote!(&*__xr_view_model))?;
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
                #sections
                #search
            });
            return self.render_view_empty_state(
                element,
                scope,
                out,
                next,
                name,
                Some(&quote!(&*__xr_view_model)),
            );
        }
        if kind == Some("StandardListView") {
            let model = if let Some(binding) = model_binding(element) {
                let model = self.visual_value(binding, scope)?;
                let projected = self
                    .project_row_heights(element, &name, &quote!(&__xr_model), None)?
                    .map(|value| quote!(#name.row_heights = #value;));
                let sections = self.project_list_sections(element, &name, &quote!(&__xr_model))?;
                let search = self.list_text_search(element, &name, &quote!(&__xr_model))?;
                out.extend(quote! {
                    let __xr_model = #model;
                    #name.list = Some(::xrack_runtime::ListMetrics {
                        row_count: ::xrack_runtime::ListModel::row_count(&__xr_model), first_row: 0,
                    });
                    #name.standard_list_stamp = Some(::xrack_runtime::ListModel::structure_stamp(&__xr_model));
                    #projected
                    #sections
                    #search
                    if #source_scroll || #name.boolean("preserve-scroll-anchor", false) {
                        #name.row_locator = Some(::xrack_runtime::ListRowLocator::new(::std::rc::Rc::new(__xr_model.clone())));
                    }
                });
                Some(quote!(&__xr_model))
            } else {
                out.extend(quote! { #name.list = Some(::xrack_runtime::ListMetrics::default()); });
                None
            };
            return self.render_view_empty_state(element, scope, out, next, name, model.as_ref());
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
            return self.render_view_empty_state(element, scope, out, next, name, None);
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
                let tree = self
                    .tree_projection(element, &quote!(#receiver.#field.resolve_model(|| #model)))?;
                let state = tree_state_field(element_key(element)?);
                let tree_model = quote!(&*__xr_tree_model);
                let projected = self
                    .project_row_heights(element, &name, &tree_model, Some(&quote!(&tree)))?
                    .map(|value| quote!(#name.row_heights = #value;));
                out.extend(quote! {
                    let (tree, __xr_tree_model) = #tree;
                    #projected
                    self.__state.#state.populate_scene(&mut #name, tree);
                });
                return self.render_view_empty_state(element, scope, out, next, name, None);
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
                return self.render_view_empty_state(element, scope, out, next, name, None);
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
            out.extend(self.project_list_sections(element, &name, &quote!(&*__xr_view_model))?);
            out.extend(self.list_text_search(element, &name, &quote!(&*__xr_view_model))?);
            return self.render_view_empty_state(
                element,
                scope,
                out,
                next,
                name,
                Some(&quote!(&*__xr_view_model)),
            );
        }
        let mut content = TokenStream::new();
        if kind == Some("SwipeView") {
            let handle = swipe_preview_handle(key);
            content.extend(quote! { #name.enable_swipe_preview(self.__state.#handle.clone()); });
        }
        if kind == Some("StackView") {
            let snapshot = stack_snapshot_method(element_key(element)?);
            content.extend(quote! {
                let __xr_stack = self.#snapshot();
                #name.stack = Some(__xr_stack.clone());
            });
        }
        let radio_blocks = kind == Some("RadioGroup")
            && scene_children(element).any(|child| children(child, "xr.repeat").next().is_some());
        let mut swipe_mounts = TokenStream::new();
        let mut table_cells = TokenStream::new();
        let mut table_columns = 0usize;
        for (index, child) in scene_children(element).enumerate() {
            if matches!(kind, Some("SwipeView" | "TabWidget" | "StackLayout"))
                && let Some(repeat) = children(child, "xr.repeat").next()
            {
                if !self.empty_repeat(child) {
                    self.append_repeated_navigation_pages(
                        child,
                        repeat,
                        scope,
                        &name,
                        &mut content,
                        next,
                    )?;
                }
                continue;
            }
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
            let repeat_op = children(child, "xr.repeat")
                .next()
                .filter(|_| matches!(kind, Some("RadioGroup" | "TabBar")));
            if let Some(repeat_op) = repeat_op
                && let Some(model) = self.group_template_model(child, scope)?
            {
                let repeat = &scope.repeats[element_key(child)?];
                let field = repeat_field(repeat.index());
                let receiver = scope.receiver();
                let row = self.repeat_row(child, repeat, repeat_op, next)?;
                let rows = if repeat.uses_row_identity() {
                    let factory = self.repeat_delegate_factory(repeat, repeat_op, None)?;
                    quote!(#receiver.#field.render_resolved_indices(&**__xr_group_model, 0..4096, None, #factory, #row))
                } else {
                    quote!(#receiver.#field.render_resolved_indices(__xr_group_model, 0..4096, #row))
                };
                content.extend(quote! {
                    #name.append_group_options(|| #model, |__xr_group_model| #rows);
                });
                continue;
            }
            let child = if kind == Some("TabWidget") {
                let index = index as i64;
                let active = if navigation_selects_by_value(element) {
                    let method = navigation_choice_method(element_key(element)?);
                    let index = index as usize;
                    quote!(self.#method() == ::core::option::Option::Some(#index))
                } else {
                    quote!(#name.int("current-index").unwrap_or(0) == #index)
                };
                self.render_element(
                    child,
                    scope,
                    &mut content,
                    next,
                    RenderContent::Active(active),
                )?
            } else if kind == Some("SwipeView") {
                let mount = format_ident!("{name}_mount_{index}");
                swipe_mounts.extend(quote!(#index => #mount(page),));
                self.render_element(
                    child,
                    scope,
                    &mut content,
                    next,
                    RenderContent::Deferred(mount),
                )?
            } else if kind == Some("StackView") {
                let route = child.attr("id")?;
                let active = quote!(#name.is_visible() && __xr_stack.current() == Some(#route));
                self.render_element(
                    child,
                    scope,
                    &mut content,
                    next,
                    RenderContent::Active(active),
                )?
            } else if child.attribute("kind") == Some("ComboContent") {
                let mut display = TokenStream::new();
                let child = self.render(child, scope, &mut display, next)?;
                content.extend(quote! {
                    let #child = if ::xrack_runtime::combo_editable(&#name) {
                        ::xrack_runtime::Node::hidden()
                    } else {
                        #display
                        #child
                    };
                });
                child
            } else {
                self.render(child, scope, &mut content, next)?
            };
            if repeat_op.is_some() {
                content.extend(quote!(#name.children.extend(#child.children.iter().cloned());));
            } else {
                content.extend(quote!(#name.children.push(#child);));
            }
            if radio_blocks {
                content.extend(quote!(#name.record_radio_option_block();));
            }
        }
        if !swipe_mounts.is_empty() {
            content.extend(quote! {
                #name.mount_swipe_pages(|index, page| match index {
                    #swipe_mounts
                    _ => ::core::unreachable!(),
                });
            });
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
                let binding = parameter_ident(name);
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
        if kind == Some("Loader")
            && let Some(source) = children(element, "xr.bind")
                .find(|bind| bind.attribute("name") == Some("source-url"))
        {
            let source = self.visual_value(source, scope)?;
            let slot = external_loader_slot(element_key(element)?);
            let receiver = scope.receiver();
            content.extend(quote!(#name.children.push(#receiver.#slot.render(&#source));));
        }
        if kind == Some("DynamicContainer") {
            let slot = dynamic_children(element_key(element)?);
            content.extend(quote!(#name.children.extend(self.__state.#slot.render());));
        }
        match render_content {
            RenderContent::Inline if kind.is_some_and(xrack_runtime::is_window_overlay) => {
                out.extend(quote! { if #name.is_visible() { #content } });
            }
            RenderContent::Inline if kind == Some("Expander") => out.extend(quote! {
                if !#name.expander_collapsed() { #content }
            }),
            RenderContent::Inline => out.extend(content),
            RenderContent::Active(active) => out.extend(quote! {
                if #active && #name.boolean("visible", true) && #name.boolean("enabled", true) {
                    #content
                }
            }),
            RenderContent::Deferred(mount) => out.extend(quote! {
                let mut #mount = |#name: &mut ::xrack_runtime::Node| { #content };
            }),
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
            let fill = xrack_mlir::split_fill_slot(element)?
                .map_or_else(|| quote!(None), |slot| quote!(Some(#slot)));
            out.extend(quote!(self.__state.#field.populate(&mut #name, #fill);));
            if let Some(template) = children(element, "xr.element")
                .find(|child| child.attr("kind") == Ok("SplitHandle"))
            {
                let mut decoration = TokenStream::new();
                let template =
                    self.render(template, &VisualScope::default(), &mut decoration, next)?;
                let inputs = split_handle_inputs().map(|(name, _)| {
                    let binding = parameter_ident(name);
                    let field = ident(name.trim_start_matches("handle-"));
                    quote!(let #binding = __xr_handle.#field;)
                });
                out.extend(quote! {
                    self.__state.#field.handle().decorate(&mut #name, |__xr_handle| {
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
            let selection = if non_exclusive_button_group(group) {
                quote!(::core::option::Option::None)
            } else {
                let (property, ty) =
                    controlled_property(group).ok_or("missing ButtonGroup selection")?;
                let current = self.control_value(group, property, ty)?;
                let mode = if property == "current-value" {
                    quote!(Value)
                } else {
                    quote!(Index)
                };
                quote!(::core::option::Option::Some(::xrack_runtime::ButtonGroupSelection::#mode(#current)))
            };
            out.extend(quote! {
                ::xrack_runtime::bind_button_group_options(
                    &mut #node, #key, &[#(#paths),*],
                    ::xrack_runtime::ButtonGroupOptions::read(|| #name, || #selection, || #enabled),
                );
            });
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

    fn range_position(
        &self,
        control: OperationRef<'a>,
        property: &str,
    ) -> Result<TokenStream, String> {
        let key = element_key(control)?;
        let handle = range_position_handle(key);
        if control.attr("kind")? == "ProgressIndicator" {
            let receiver = quote!(self);
            let projection = TableProjection::new(ExpressionContext::new(&receiver));
            let number = |name, fallback| {
                let base = self.binding_value_or(control, name, fallback)?;
                self.row_state_value(control, name, base, &projection)
            };
            let minimum = number("minimum", quote!(0.0_f64))?;
            let maximum = number("maximum", quote!(1.0_f64))?;
            let value = number("progress", quote!(0.0_f64))?;
            let position =
                quote!(::xrack_runtime::ProgressRange::new(#minimum, #maximum, #value).position());
            return Ok(if property == "visual-position" {
                quote!(self.__state.#handle.visual_position(#position, ::xrack_runtime::Orientation::Horizontal))
            } else {
                position
            });
        }
        let interval = control.attr("kind")? == "RangeSlider";
        let min = self.binding_value_or(control, "minimum", quote!(0.0_f64))?;
        let max = self.binding_value_or(control, "maximum", quote!(100.0_f64))?;
        let live = if control.attr("kind")? == "Dial" {
            quote!(true)
        } else {
            self.binding_value_or(control, "live", quote!(true))?
        };
        let step = self.binding_value_or(control, "step", quote!(1.0_f64))?;
        let snap = self.binding_value_or(
            control,
            "snap-mode",
            quote!(::xrack_runtime::ScrollSnapMode::None),
        )?;
        let committed = if interval {
            quote!(0.0_f64)
        } else {
            self.control_value(control, "value", Type::Float)?
        };
        let position = if interval {
            let low = self.control_value(control, "lower-value", Type::Float)?;
            let high = self.control_value(control, "upper-value", Type::Float)?;
            let index = usize::from(property.starts_with("upper"));
            quote! {
                __xr_handle.interval_position(
                    ::xrack_runtime::SliderInterval::from_range(__xr_axis, #low, #high), __xr_live, #index)
            }
        } else {
            quote!(__xr_handle.slider_position(__xr_axis, __xr_live))
        };
        let result = if property.contains("visual") {
            let orientation = self.binding_value_or(
                control,
                "orientation",
                quote!(::xrack_runtime::Orientation::Horizontal),
            )?;
            quote!(__xr_handle.visual_position(#position, #orientation))
        } else {
            position
        };
        Ok(quote! {
            let __xr_handle = &self.__state.#handle;
            let minimum = #min;
            let maximum = #max;
            let __xr_live = #live;
            let (step, snap) = if __xr_live {
                (1.0_f64, ::xrack_runtime::ScrollSnapMode::None)
            } else {
                (#step, #snap)
            };
            let __xr_axis = ::xrack_runtime::SliderRange::new(minimum, maximum, #committed, step, false, snap);
            #result
        })
    }

    fn toggle_check_state(&self, control: OperationRef<'a>) -> Result<TokenStream, String> {
        if let Some(binding) = check_state_binding(control) {
            return self.visual_value(binding, &self.visuals);
        }
        if check_state_control(control) {
            return self.control_value(control, "check-state", Type::CheckState);
        }
        let checked = self.control_value(control, "checked", Type::Bool)?;
        Ok(quote!(::xrack_runtime::CheckState::from(#checked)))
    }

    fn button_checked(&self, button: OperationRef<'a>) -> Result<TokenStream, String> {
        if let Some(action) = action_ref(button) {
            let checked = action_checked_method(action);
            return Ok(quote!(self.#checked(::core::option::Option::None)));
        }
        let Some(group_key) = radio_group_ref(button) else {
            return if controlled_property(button) == Some(("checked", Type::Bool)) {
                self.control_value(button, "checked", Type::Bool)
            } else {
                Ok(quote!(false))
            };
        };
        let group = self.elements[group_key];
        let root = self.elements[self.root_key];
        let groups = button_group_members(root);
        let members = &groups[group_key];
        let target = element_key(button)?;
        let (property, ty) = controlled_property(group).ok_or("missing ButtonGroup selection")?;
        let current = self.control_value(group, property, ty)?;
        if property == "current-index" {
            let index = members
                .iter()
                .position(|(_, member)| element_key(*member) == Ok(target))
                .ok_or("missing ButtonGroup option")? as i64;
            return Ok(quote!((#current) == #index));
        }

        let receiver = quote!(self);
        let projection = TableProjection::new(ExpressionContext::new(&receiver));
        let mut candidates = TokenStream::new();
        for (path, candidate) in members {
            let mounted = self.button_group_candidate_mounted(root, path, &projection)?;
            let property = if children(*candidate, "xr.bind")
                .any(|binding| binding.attr("name") == Ok("value"))
            {
                "value"
            } else {
                "text"
            };
            let value = self.row_binding_or(
                *candidate,
                property,
                quote!(::std::string::String::new()),
                &projection,
            )?;
            let matches_target = element_key(*candidate)? == target;
            candidates.extend(quote! {
                if (#mounted) && (#value) == __xr_current { return #matches_target; }
            });
            if matches_target {
                break;
            }
        }
        let selectors = self.input_selectors(&projection)?;
        Ok(quote! {
            let __xr_current = #current;
            if __xr_current.is_empty() { return false; }
            #selectors
            #candidates
            false
        })
    }

    fn button_group_candidate_mounted(
        &self,
        root: OperationRef<'a>,
        path: &[usize],
        projection: &TableProjection<'_, 'a>,
    ) -> Result<TokenStream, String> {
        let ancestors = Self::row_path(root, path)?;
        let mut checks = Vec::new();
        for (depth, &element) in ancestors.iter().enumerate() {
            let kind = element.attr("kind")?;
            let page = matches!(kind, "Tab" | "StackPage" | "SwipePage" | "StackItem");
            checks.push(if page {
                self.element_allows_editing(element, projection)?
            } else {
                self.row_conditions_met(element, projection)?
            });
            let Some(&slot) = path.get(depth) else {
                continue;
            };
            if kind == "StackView" || kind == "SwipeView" || xrack_runtime::is_window_overlay(kind)
            {
                checks.push(self.row_binding_or(element, "visible", quote!(true), projection)?);
            }
            match kind {
                "StackView" => {
                    let snapshot = stack_snapshot_method(element_key(element)?);
                    let route = ancestors[depth + 1].attr("id")?;
                    checks.push(
                        quote!(self.#snapshot().current() == ::core::option::Option::Some(#route)),
                    );
                }
                "StackLayout" => {
                    let method = navigation_choice_method(element_key(element)?);
                    checks.push(quote!(self.#method() == ::core::option::Option::Some(#slot)));
                }
                "TabWidget" => {
                    let current = self.control_value(element, "current-index", Type::Int)?;
                    let slot = slot as i64;
                    checks.push(quote!((#current) == #slot));
                }
                "SwipeView" => {
                    let current = self.control_value(element, "current-index", Type::Int)?;
                    let pages = static_children(element)
                        .map(|(_, page)| self.element_allows_editing(page, projection))
                        .collect::<Result<Vec<_>, _>>()?;
                    let count = pages.len();
                    let indices = 0..count;
                    checks.push(quote! {
                        ::xrack_runtime::resolve_swipe_index(
                            ::core::option::Option::Some(#current),
                            (0..#count).map(|index| match index { #(#indices => #pages,)* _ => false }),
                        ) == ::core::option::Option::Some(#slot)
                    });
                }
                _ if xrack_runtime::is_window_overlay(kind) => {
                    checks.push(self.control_value(element, "open", Type::Bool)?);
                }
                _ => {}
            }
        }
        Ok(quote!(#((#checks) &&)* true))
    }

    fn toggle_button(&self, button: OperationRef<'a>) -> Result<TokenStream, String> {
        if let Some(action) = action_ref(button) {
            let action = *self.actions.get(action).ok_or("unknown button Action")?;
            if let Some(member) = children(action, "xr.action_group_ref").next() {
                let group = *self
                    .elements
                    .get(member.attr("group")?)
                    .ok_or("unknown ActionGroup")?;
                if !non_exclusive_group(group) {
                    let value = member.attr("value")?;
                    return self
                        .toggle_group_value(group, &quote!(::std::string::String::from(#value)));
                }
            }
            let checked = self.control_value(action, "checked", Type::Bool)?;
            return self.set_control(action, &quote!(!(#checked)));
        }
        if let Some(key) = radio_group_ref(button) {
            let group = *self.elements.get(key).ok_or("unknown ButtonGroup")?;
            let (property, ty) =
                controlled_property(group).ok_or("missing ButtonGroup selection")?;
            if property == "current-value" {
                let value = self.live_radio_value(button, &self.visuals)?;
                return self.toggle_group_value(group, &value);
            }
            let root = *self
                .elements
                .get(self.root_key)
                .ok_or("missing component root")?;
            let groups = button_group_members(root);
            let button_key = element_key(button)?;
            let index = groups
                .get(key)
                .and_then(|members| {
                    members
                        .iter()
                        .position(|(_, member)| element_key(*member) == Ok(button_key))
                })
                .ok_or("missing ButtonGroup option")?;
            let current = self.control_value(group, property, ty)?;
            return self.set_control(
                group,
                &quote!(::xrack_runtime::toggled_button_group_index(#current, #index)),
            );
        }
        let key = element_key(button)?;
        if self.repeated_buttons.contains(key) {
            let toggle = row_button_binding(key, "toggle");
            return Ok(quote!(#toggle();));
        }
        let checked = self.control_value(button, "checked", Type::Bool)?;
        self.set_control(button, &quote!(!(#checked)))
    }

    fn toggle_group_value(
        &self,
        group: OperationRef<'a>,
        value: &TokenStream,
    ) -> Result<TokenStream, String> {
        let current = self.control_value(group, "current-value", Type::String)?;
        let set = self.set_control(group, &quote!(__xr_next))?;
        Ok(quote! {
            if let ::core::option::Option::Some(__xr_next) =
                ::xrack_runtime::toggled_button_group_value(&(#current), #value)
            {
                #set
            }
        })
    }

    fn set_checked(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
    ) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        if scope.row_checks.contains_key(key) && control_link(element, "checked").is_none() {
            let field = control_field(key, "checked");
            let storage = scope.receiver();
            Ok(quote!(#storage.#field.set_if_changed(__xr_checked);))
        } else {
            self.set_control(element, &quote!(__xr_checked))
        }
    }

    fn activate_checkable_button(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        input: &TokenStream,
        available: &TokenStream,
    ) -> Result<TokenStream, String> {
        let (checkable, available) = if let Some(key) = check_group_ref(element) {
            let group = *self.elements.get(key).ok_or("unknown check ButtonGroup")?;
            let receiver = quote!(self);
            let projection = TableProjection::new(ExpressionContext::new(&receiver));
            let enabled = self.row_binding_or(group, "enabled", quote!(true), &projection)?;
            (
                quote!(true),
                quote!((#available) && match __xr_route {
                    ::core::option::Option::Some(input) => input.available(|| #enabled),
                    ::core::option::Option::None => #enabled,
                }),
            )
        } else {
            let checkable = children(element, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("checkable"))
                .map(|binding| self.visual_value(binding, scope))
                .transpose()?
                .unwrap_or_else(|| quote!(false));
            (checkable, available.clone())
        };
        let current = self.control_value_in(element, "checked", Type::Bool, scope)?;
        let update = self.set_checked(element, scope)?;
        let receiver = quote!(self);
        let context = ExpressionContext {
            repeat_parameters: scope.row_index.is_some(),
            ..ExpressionContext::new(&receiver)
        };
        let toggled = self.typed_event_values_in(element, "toggled", &[], context)?;
        let clicked = self.typed_event_values_in(element, "clicked", &[], context)?;
        let has_clicked =
            children(element, "xr.handler").any(|handler| handler.attr("name") == Ok("clicked"));
        Ok(quote!((|| {
            let __xr_route: ::core::option::Option<::xrack_runtime::InputRoute<'_>> = #input;
            let ::core::option::Option::Some(__xr_next) = ::xrack_runtime::prepare_action_activation(
                || #available, || #checkable, || #current,
            ) else { return false; };
            if __xr_route.is_some_and(|input| !input.is_current()) { return false; }
            if let ::core::option::Option::Some(__xr_checked) = __xr_next {
                #update
                #toggled
            }
            #clicked
            __xr_next.is_some() || #has_clicked
        })()))
    }

    fn group_box_toggle(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        input: &TokenStream,
    ) -> Result<TokenStream, String> {
        let checkable = children(element, "xr.bind")
            .find(|binding| binding.attr("name") == Ok("checkable"))
            .map(|binding| self.visual_value(binding, scope))
            .transpose()?
            .unwrap_or_else(|| quote!(false));
        let current = self.control_value_in(element, "checked", Type::Bool, scope)?;
        let update = self.set_checked(element, scope)?;
        let toggled = self.typed_event(element, "toggled", &quote!(__xr_checked))?;
        Ok(quote!({
            if let Some(__xr_checked) = ::xrack_runtime::prepare_group_box_toggle(#input, || #checkable, || #current) {
                #update
                #toggled
                true
            } else {
                false
            }
        }))
    }

    fn expander_change(
        &self,
        element: OperationRef<'a>,
        input: &TokenStream,
        requested: &TokenStream,
    ) -> Result<TokenStream, String> {
        let expanded = self.control_value(element, "expanded", Type::Bool)?;
        let update = self.set_control_property(element, "expanded", &quote!(__xr_expanded))?;
        let changed = self.typed_event(element, "expanded-changed", &quote!(__xr_expanded))?;
        Ok(quote!({
            if let Some(__xr_expanded) = ::xrack_runtime::prepare_expander_change(#input, #requested, || #expanded) {
                #update
                #changed
                true
            } else {
                false
            }
        }))
    }

    fn dispatch_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        local: &mut TokenStream,
        paths: &mut TokenStream,
        handlers: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            let field = instance_field(element_key(element)?);
            paths.extend(quote! {
                [#(#path,)* rest @ ..] => self.__state.#field.__dispatch_path(rest, event, input),
            });
            return Ok(());
        }
        let id = element.attr("id")?;
        let kind = element.attr("kind")?;
        let stores_checked = has_button_check_state(element);
        let mut local_handlers = TokenStream::new();
        for handler in children(element, "xr.handler") {
            let event = handler.attr("name")?;
            if xrack_runtime::supports_untyped_dispatch(kind, event) {
                let statements = self.statements(handler)?;
                handlers.extend(quote!(([#(#path),*], #event) => { #statements true },));
                // The checked-state arm handles named clicks before running their handler.
                if event != "clicked" || id.is_empty() || !stores_checked {
                    local_handlers.extend(
                        quote!((#id, #event) => self.__dispatch_handler(&[#(#path),*], event),),
                    );
                }
            }
        }
        if matches!(
            element.attr("kind"),
            Ok("Button" | "ToolButton" | "RoundButton")
        ) && let Some(action) = action_ref(element)
        {
            let activate = action_activate_method(action);
            let receiver = quote!(self);
            let projection = TableProjection::new(ExpressionContext::new(&receiver));
            let available = self.element_allows_editing(element, &projection)?;
            paths.extend(quote!([#(#path),*] => {
                    if event == "clicked" {
                        self.#activate(|| true, Some(input))
                    } else {
                        self.__dispatch_handler(path, event)
                    }
                },));
            if !id.is_empty() {
                local.extend(quote!((#id, "clicked") => self.#activate(|| #available, None),));
            }
        } else if xrack_syntax::builtins::push_button(kind) && radio_group_ref(element).is_some() {
            paths.extend(quote!([#(#path),*] => self.__dispatch_handler(path, event),));
            if !id.is_empty() {
                local.extend(quote!((#id, "clicked") => {
                    let scene = self.__render_shared();
                    let path = &[#(#path),*];
                    ::xrack_runtime::RoutedEvent::Dispatch("clicked")
                        .with_input(&scene, path, |input| self.__route_input(&scene, path, input))
                        .unwrap_or(false)
                },));
            }
        } else if stores_checked {
            let click = self.activate_checkable_button(
                element,
                &self.visuals,
                &quote!(Some(input)),
                &quote!(true),
            )?;
            paths.extend(quote!([#(#path),*] => {
                if event == "clicked" { #click } else { self.__dispatch_handler(path, event) }
            },));
            if !id.is_empty() {
                let receiver = quote!(self);
                let projection = TableProjection::new(ExpressionContext::new(&receiver));
                let available = self.element_allows_editing(element, &projection)?;
                let click = self.activate_checkable_button(
                    element,
                    &self.visuals,
                    &quote!(None),
                    &available,
                )?;
                local.extend(quote!((#id, "clicked") => #click,));
            }
        } else if element.attr("kind") == Ok("Expander") {
            let routed_click =
                self.expander_change(element, &quote!(Some(input)), &quote!(None))?;
            paths.extend(quote!([#(#path),*] => event == "clicked" && #routed_click,));
            if !id.is_empty() {
                let receiver = quote!(self);
                let projection = TableProjection::new(ExpressionContext::new(&receiver));
                let available = self.element_allows_editing(element, &projection)?;
                let click = self.expander_change(element, &quote!(None), &quote!(None))?;
                local.extend(quote!((#id, "clicked") => (#available) && #click,));
            }
        } else if element.attr("kind") == Ok("GroupBox") {
            let routed_click =
                self.group_box_toggle(element, &self.visuals, &quote!(Some(input)))?;
            paths.extend(quote!([#(#path),*] => event == "clicked" && #routed_click,));
            if !id.is_empty() {
                let click = self.group_box_toggle(element, &self.visuals, &quote!(None))?;
                local.extend(quote!((#id, "clicked") => #click,));
            }
        } else if element.attr("kind").is_ok_and(is_toggle) {
            let group_guard = if let Some(key) = check_group_ref(element) {
                let group = *self.elements.get(key).ok_or("unknown check ButtonGroup")?;
                let receiver = quote!(self);
                let projection = TableProjection::new(ExpressionContext::new(&receiver));
                let enabled = self.row_binding_or(group, "enabled", quote!(true), &projection)?;
                quote!(if !input.available(|| #enabled) { return false; })
            } else {
                TokenStream::new()
            };
            let projection = check_state_binding(element);
            let (current, tristate, value) = if check_state_control(element) {
                let current = self.toggle_check_state(element)?;
                let tristate = children(element, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("tristate"))
                    .map(|binding| self.visual_value(binding, &self.visuals))
                    .transpose()?
                    .unwrap_or_else(|| quote!(false));
                (current, tristate, quote!(__xr_state))
            } else {
                (
                    self.toggle_check_state(element)?,
                    quote!(false),
                    quote!(__xr_state == ::xrack_runtime::CheckState::Checked),
                )
            };
            let update = if projection.is_some() {
                TokenStream::new()
            } else {
                self.set_control(element, &value)?
            };
            let changed = self.typed_event(element, "check-state-changed", &quote!(__xr_state))?;
            let click = |input| {
                quote!({
                    let Some(__xr_state) = ::xrack_runtime::CheckState::prepare_toggle(
                        #input, || #current, || #tristate,
                    ) else { return false; };
                    #update
                    self.__dispatch_handler(&[#(#path),*], "toggled");
                    #changed
                    true
                })
            };
            let routed_click = click(quote!(Some(input)));
            paths.extend(quote! {
                [#(#path),*] => {
                    if event == "clicked" { #group_guard #routed_click } else { self.__dispatch_handler(path, event) }
                },
            });
            if !id.is_empty() {
                let click = click(quote!(None));
                local.extend(quote!((#id, "clicked") => #click,));
            }
        } else if matches!(kind, "TextInput" | "LineEdit") && has_editing_finished(element) {
            let handle = focus_handle(element_key(element)?);
            let current = self.control_value(element, "text", Type::String)?;
            let accepts = self.check_text_submission(element, quote!(&__xr_value))?;
            let finished = self.typed_event_values(
                element,
                "editing-finished",
                &[quote!(__xr_value), quote!(true)],
            )?;
            paths.extend(quote! {
                [#(#path),*] => {
                    if event != "accepted" { return self.__dispatch_handler(path, event); }
                    let __xr_value = #current;
                    if !input.available(|| #accepts) { return false; }
                    let __xr_finished = self.__state.#handle.take_editor_changes(&__xr_value);
                    let __xr_handled = self.__dispatch_handler(path, event);
                    if __xr_finished { #finished }
                    __xr_handled || __xr_finished
                },
            });
        } else if !local_handlers.is_empty() {
            let submission = if matches!(kind, "TextInput" | "LineEdit")
                && children(element, "xr.handler")
                    .any(|handler| handler.attr("name") == Ok("accepted"))
            {
                let accepts = self.text_acceptance_expression(element)?;
                quote! {
                    if event == "accepted" && !input.available(|| #accepts) { return false; }
                }
            } else {
                TokenStream::new()
            };
            paths.extend(
                quote!([#(#path),*] => { #submission self.__dispatch_handler(path, event) },),
            );
        }
        local.extend(local_handlers);
        for (index, child) in static_children(element) {
            path.push(index);
            self.dispatch_arms(child, path, local, paths, handlers)?;
            path.pop();
        }
        Ok(())
    }

    fn search_methods(
        &self,
        element: OperationRef<'a>,
        is_root: bool,
    ) -> Result<TokenStream, String> {
        let key = element_key(element)?;
        let input = search_method(key, "input");
        let control = search_method(key, "control");
        let text = quote!(__xr_text.to_owned());
        let set = self.set_control(element, &text)?;
        let edited = self.typed_event(element, "edited", &text)?;
        let accepted = self.typed_event(element, "accepted", &text)?;
        let selected = self.typed_event(element, "selected", &text)?;
        let search = self.typed_event(element, "search-triggered", &text)?;
        let live = self.binding_value_or(element, "live", quote!(false))?;
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let available = self.element_allows_routed_input(element, is_root, &projection)?;
        let read_only = self.binding_value_or(element, "read-only", quote!(false))?;
        let current = self.control_value(element, "text", Type::String)?;
        let binding =
            |name| children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(name));
        let result_query = match binding("suggestions-for") {
            Some(binding) => {
                let value = self.visual_value(binding, &self.visuals)?;
                quote!(::core::option::Option::Some(#value))
            }
            None => quote!(::core::option::Option::None),
        };
        let suggestion = match binding("suggestions") {
            Some(binding) => {
                let model = self.visual_value(binding, &self.visuals)?;
                quote!((#model).row(index))
            }
            None => quote!(::core::option::Option::None),
        };
        Ok(quote! {
            fn #input(&self, input: ::xrack_runtime::SearchInput) -> ::core::primitive::bool {
                input.apply(|| #live, |update| match update {
                    ::xrack_runtime::SearchUpdate::SetText(__xr_text) => { #set }
                    ::xrack_runtime::SearchUpdate::Edited(__xr_text) => { #edited }
                    ::xrack_runtime::SearchUpdate::Accepted(__xr_text) => { #accepted }
                    ::xrack_runtime::SearchUpdate::Selected(__xr_text) => { #selected }
                    ::xrack_runtime::SearchUpdate::Triggered(__xr_text) => { #search }
                })
            }
            fn #control(&self, node: &::xrack_runtime::Node, action: ::xrack_runtime::ControlAction, input: ::xrack_runtime::InputRoute<'_>) -> ::core::primitive::bool {
                let ::core::option::Option::Some(prepared) = ::xrack_runtime::SearchInput::from_control(
                    node, action, || #available, || !(#read_only), || #current, || #result_query, |index| #suggestion,
                ) else { return false; };
                input.is_current() && self.#input(prepared)
            }
        })
    }

    fn text_input_constraints(
        &self,
        element: OperationRef<'a>,
    ) -> Result<(TokenStream, TokenStream, TokenStream), String> {
        let mode = self.binding_value_or(
            element,
            "input-type",
            quote!(::xrack_runtime::InputType::Text),
        )?;
        let separator = self.binding_value_or(
            element,
            "decimal-separator",
            quote!(::std::string::String::new()),
        )?;
        let separator = quote!(if (#separator) == "," { ',' } else { '.' });
        let limit = self.optional_editor_binding(element, "maximum-length")?;
        Ok((mode, separator, limit))
    }

    fn optional_editor_binding(
        &self,
        element: OperationRef<'a>,
        name: &str,
    ) -> Result<TokenStream, String> {
        let binding = children(element, "xr.bind").find(|binding| binding.attr("name") == Ok(name));
        match binding {
            Some(binding) => {
                let value = self.visual_value(binding, &self.visuals)?;
                Ok(quote!(::core::option::Option::Some(#value)))
            }
            None => Ok(quote!(::core::option::Option::None)),
        }
    }

    fn text_acceptance_expression(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        self.text_validation_expression(element, false)
    }

    fn text_validation_expression(
        &self,
        element: OperationRef<'a>,
        detailed: bool,
    ) -> Result<TokenStream, String> {
        if element.attr("kind")? == "ComboBox" {
            let editable = self.binding_value_or(element, "editable", quote!(false))?;
            let (_, draft) = self.combo_state_value(element, "edit-text")?;
            let validation = self.text_validation_call(element, quote!(&__xr_draft), detailed)?;
            let inactive = if detailed {
                quote!(::xrack_runtime::InputValidity::Invalid)
            } else {
                quote!(false)
            };
            return Ok(quote! {
                if #editable {
                    let __xr_draft: ::std::string::String = (|| { #draft })();
                    #validation
                } else {
                    #inactive
                }
            });
        }
        let text = self.control_value(element, "text", Type::String)?;
        self.text_validation_call(element, quote!(&#text), detailed)
    }

    fn check_text_submission(
        &self,
        element: OperationRef<'a>,
        text: TokenStream,
    ) -> Result<TokenStream, String> {
        self.text_validation_call(element, text, false)
    }

    fn text_validation_call(
        &self,
        element: OperationRef<'a>,
        text: TokenStream,
        detailed: bool,
    ) -> Result<TokenStream, String> {
        let (mode, separator, limit) = self.text_input_constraints(element)?;
        let minimum = self.binding_value_or(element, "minimum-length", quote!(0_i64))?;
        let condition = self.binding_value_or(element, "submit-when", quote!(true))?;
        let floor = self.optional_editor_binding(element, "minimum-value")?;
        let ceiling = self.optional_editor_binding(element, "maximum-value")?;
        let decimals = self.optional_editor_binding(element, "decimals")?;
        let mask =
            self.binding_value_or(element, "input-mask", quote!(::std::string::String::new()))?;
        let pattern = self.binding_value_or(
            element,
            "input-pattern",
            quote!(::std::string::String::new()),
        )?;
        let (function, rules) = if detailed {
            (
                format_ident!("text_input_validity"),
                quote!(|| #mask, || #pattern, || #condition),
            )
        } else {
            (
                format_ident!("text_submission_with_mask"),
                quote!(|| #mask, || ::xrack_runtime::input_pattern_accepts(&#pattern, __xr_text) && (#condition)),
            )
        };
        Ok(quote!({
            let __xr_text = #text;
            ::xrack_runtime::#function(
            __xr_text, #mode, || #separator, || #limit, || #minimum,
            |mode| ::xrack_runtime::NumericTextLimits {
                minimum: #floor,
                maximum: #ceiling,
                decimals: if mode == ::xrack_runtime::InputType::Decimal {
                    #decimals
                } else {
                    ::core::option::Option::None
                },
            },
            #rules,
        )
        }))
    }

    fn live_editor_guard(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let kind = element.attr("kind")?;
        let (mode, separator, limit) = self.text_input_constraints(element)?;
        let mask =
            self.binding_value_or(element, "input-mask", quote!(::std::string::String::new()))?;
        let pattern = self.binding_value_or(
            element,
            "input-pattern",
            quote!(::std::string::String::new()),
        )?;
        let matches = quote! {
            #mask == node.text("input-mask").unwrap_or_default()
                && #pattern == node.text("input-pattern").unwrap_or_default()
                && node.allows_text_edit_with(&text, #mode, || #separator, || #limit)
        };
        let allowed = if kind == "ComboBox" {
            let editable = self.binding_value_or(element, "editable", quote!(false))?;
            let model_current = self.combo_model_current(element)?;
            let draft_current = self.combo_draft_current(element, &model_current)?;
            quote!(node.prepare_combo_edit(input, || #editable, || {
                (#draft_current) && (#matches)
            }))
        } else {
            let read_only = self.binding_value_or(element, "read-only", quote!(false))?;
            let current = self.control_value(element, "text", Type::String)?;
            quote!(node.prepare_text_edit(input, || !(#read_only), || #current, || #matches))
        };
        Ok(quote!(if !(#allowed) { return false; }))
    }

    fn edit_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            return Ok(());
        }
        let track = if has_editing_finished(element) {
            let handle = focus_handle(element_key(element)?);
            quote! {
                self.__state.#handle.record_editor_edit(::xrack_runtime::text_input_value(node), &text);
            }
        } else {
            TokenStream::new()
        };
        if combo_editor(element) {
            let guard = self.live_editor_guard(element)?;
            let set = self.set_control_property(element, "edit-text", &quote!(text.clone()))?;
            let edited = self.typed_event(element, "edited", &quote!(text))?;
            out.extend(quote!([#(#path),*] => { #guard #set #track #edited true },));
        } else if element.attr("kind") == Ok("ColorPickerPopup") {
            let popup = picker_field(element_key(element)?);
            let show_alpha = self.binding_value_or(element, "show-alpha", quote!(false))?;
            out.extend(quote! {
                [#(#path,)* 0, 0, 1] => {
                    if !self.__state.#popup.accepts_input(node, input) { return false; }
                    let show_alpha = #show_alpha;
                    input.is_current() && self.__state.#popup.edit_hex(show_alpha, node, text)
                },
            });
        } else if element.attr("kind") == Ok("SearchField") {
            let guard = self.live_editor_guard(element)?;
            let edit = search_method(element_key(element)?, "input");
            out.extend(
                quote!([#(#path),*] => { #guard self.#edit(::xrack_runtime::SearchInput::Edit(text)) },),
            );
        } else if element.attr("kind").is_ok_and(is_multiline_text_input) || text_item(element) {
            let guard = self.live_editor_guard(element)?;
            let multiline = element.attr("kind").is_ok_and(is_multiline_text_input);
            let finish = has_editing_finished(element);
            let preserve = finish
                || multiline
                    && children(element, "xr.handler")
                        .any(|handler| handler.attr("name") == Ok("edited"));
            let value = if preserve {
                quote!(text.clone())
            } else {
                quote!(text)
            };
            let set = self.set_control(element, &value)?;
            let edited = if multiline {
                self.typed_event(element, "edited", &quote!(text))?
            } else {
                quote! { self.__dispatch_handler(path, "edited"); }
            };
            out.extend(quote!([#(#path),*] => { #guard #set #track #edited true },));
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
        self.typed_event_values_in(
            element,
            event,
            values,
            ExpressionContext::new(&quote!(self)),
        )
    }

    fn typed_event_values_in(
        &self,
        element: OperationRef<'a>,
        event: &str,
        values: &[TokenStream],
        context: ExpressionContext<'_>,
    ) -> Result<TokenStream, String> {
        let Some(handler) =
            children(element, "xr.handler").find(|handler| handler.attr("name") == Ok(event))
        else {
            return Ok(TokenStream::new());
        };
        let parameters = handler.parameters()?;
        if parameters.len() != values.len() {
            return Err(format!("{event} requires {} parameters", values.len()));
        }
        let names = parameters
            .iter()
            .map(|parameter| parameter_ident(&parameter.name));
        let body = self.statements_in(handler, context)?;
        let has_result = children(handler, "xr.return").next().is_some();
        if values.is_empty() || (body.is_empty() && !has_result) {
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
            && children(element, "xr.handler").any(|handler| {
                handler
                    .attribute("name")
                    .is_some_and(xrack_runtime::is_focus_handler)
            })
        {
            let mut handlers = TokenStream::new();
            for handler in xrack_runtime::FocusHandler::ALL {
                let name = handler.name();
                let variant = format_ident!("{handler:?}");
                if !children(element, "xr.handler").any(|op| op.attr("name") == Ok(name)) {
                    handlers.extend(quote!(::xrack_runtime::FocusHandler::#variant => false,));
                    continue;
                }
                let args = if matches!(handler, xrack_runtime::FocusHandler::Within) {
                    vec![quote!(__focused), quote!(__reason)]
                } else {
                    vec![quote!(__reason)]
                };
                let body = self.typed_event_values(element, name, &args)?;
                handlers.extend(quote!(::xrack_runtime::FocusHandler::#variant => { #body true },));
            }
            out.extend(quote! {
                [#(#path),*] => event.deliver(|handler, __focused, __reason| {
                    match handler { #handlers }
                }),
            });
        }
        if has_editing_finished(element) {
            let handle = focus_handle(element_key(element)?);
            let (available, current) = if element.attr("kind") == Ok("ComboBox") {
                let editable = self.binding_value_or(element, "editable", quote!(false))?;
                let (_, draft) = self.combo_state_value(element, "edit-text")?;
                (quote!(#editable), quote!((|| { #draft })()))
            } else {
                (
                    quote!(true),
                    self.control_value(element, "text", Type::String)?,
                )
            };
            let accepts = self.check_text_submission(element, quote!(&__xr_value))?;
            let handler = self.typed_event_values(
                element,
                "editing-finished",
                &[quote!(__xr_value), quote!(__xr_acceptable)],
            )?;
            out.extend(quote! {
                [#(#path),*] => {
                    if !matches!(event, ::xrack_runtime::FocusEvent::Lost(_))
                        || !self.__state.#handle.has_editor_changes()
                    {
                        return false;
                    }
                    if !(#available) {
                        self.__state.#handle.discard_editor_changes();
                        return false;
                    }
                    let __xr_value: ::std::string::String = #current;
                    if !self.__state.#handle.take_editor_changes(&__xr_value) {
                        return false;
                    }
                    let __xr_acceptable = #accepts;
                    #handler
                    true
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
                let params = handler.parameters()?;
                let [parameter] = params.as_slice() else {
                    return Err("key handler requires an event parameter".into());
                };
                let name = parameter_ident(&parameter.name);
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
        availability: &mut TokenStream,
        projection: &TableProjection<'_, 'a>,
    ) -> Result<(), String> {
        let conditions = self.row_conditions_met(element, projection)?;
        if element.name() == "xr.instance" {
            let visible = self.row_binding_or(
                element,
                "visible",
                quote!({
                    inherits_visibility = true;
                    true
                }),
                projection,
            )?;
            availability.extend(quote! {
                [#(#path),*] => {
                    let mut inherits_visibility = false;
                    let shown = (#conditions) && (root_visibility_overridden || (#visible));
                    child_visibility_overridden = !inherits_visibility;
                    shown
                },
            });
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
        let enabled = self.row_binding_or(element, "enabled", quote!(true), projection)?;
        let visible = self.row_binding_or(element, "visible", quote!(true), projection)?;
        let next = quote!(path.get(depth).and_then(|index| node.child_at(*index)));
        let closed = quote!(::xrack_runtime::SwipeSide::Closed);
        let scope = match kind {
            kind if xrack_runtime::is_window_overlay(kind) || picker_value(kind).is_some() => {
                let open = self.control_value(element, "open", Type::Bool)?;
                quote!(&& node.input_scope_available(#next, || input.can_open_drawer(node, depth == path.len()) || (#open), || false, || true, || #closed))
            }
            "Expander" => {
                let expanded = self.control_value(element, "expanded", Type::Bool)?;
                quote!(&& (depth == path.len() || (#expanded)))
            }
            "GroupBox" => {
                let checkable =
                    self.row_binding_or(element, "checkable", quote!(false), projection)?;
                let checked = self.control_value(element, "checked", Type::Bool)?;
                quote!(&& node.input_scope_available(#next, || false, || #checkable, || #checked, || #closed))
            }
            "SwipeDelegate" => {
                let side = self.control_value(element, "open-side", Type::SwipeSide)?;
                quote!(&& node.input_scope_available(#next, || false, || false, || true, || #side))
            }
            "StackLayout" => {
                let method = navigation_choice_method(element_key(element)?);
                quote!(&& (depth == path.len() || self.#method() == path.get(depth).copied()))
            }
            "TabWidget" | "SwipeView"
                if navigation_selects_by_value(element)
                    || repeated_navigation_page(element).is_some() =>
            {
                let method = navigation_choice_method(element_key(element)?);
                let page = if kind == "TabWidget" {
                    quote!(depth + 1 >= path.len())
                } else {
                    quote!(depth == path.len())
                };
                quote!(&& (#page || self.#method() == path.get(depth).copied()))
            }
            "TabWidget" => {
                let index = self.control_value(element, "current-index", Type::Int)?;
                quote!(&& (depth + 1 >= path.len() || ::core::primitive::usize::try_from(#index).ok() == path.get(depth).copied()))
            }
            "SwipeView" => {
                let index = self.control_value(element, "current-index", Type::Int)?;
                let mut pages = TokenStream::new();
                let mut count = 0;
                for (slot, page) in static_children(element) {
                    let available = self.element_allows_editing(page, projection)?;
                    pages.extend(quote!(#slot => #available,));
                    count = slot + 1;
                }
                quote! {
                    && (depth == path.len() || ::xrack_runtime::resolve_swipe_index(
                        ::core::option::Option::Some(#index),
                        (0..#count).map(|slot| match slot { #pages _ => false }),
                    ) == path.get(depth).copied())
                }
            }
            "StackView" => {
                let snapshot = stack_snapshot_method(element_key(element)?);
                quote! {
                    && (depth == path.len() || self.#snapshot().current()
                        .and_then(|route| node.stack_route_index(route)) == path.get(depth).copied())
                }
            }
            _ => TokenStream::new(),
        };
        availability.extend(quote! {
            [#(#path),*] => (!node.input_requires_enabled(depth == path.len()) || (#enabled))
                && (#conditions) && (root_visibility_overridden || (#visible)) #scope,
        });
        for handler in children(element, "xr.handler") {
            let returns_result = match (kind, handler.attr("name")?) {
                ("WheelHandler", "wheel") | ("TouchArea", "scroll-event") => true,
                ("TouchArea", "pointer-event") => false,
                _ => continue,
            };
            let params = handler.parameters()?;
            let [parameter] = params.as_slice() else {
                return Err("pointer input handler requires an event parameter".into());
            };
            let name = parameter_ident(&parameter.name);
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
            self.pointer_input_arms(child, path, wheel, pointer, availability, projection)?;
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
        let kind = element.attr("kind")?;
        if xrack_runtime::is_drag_area(kind) {
            let mut handlers = TokenStream::new();
            for handler in children(element, "xr.handler") {
                let name = handler.attr("name")?;
                let parameters = handler.parameters()?;
                let types = parameters.iter().map(|param| &param.ty).collect::<Vec<_>>();
                let (pattern, values) = match types.as_slice() {
                    [] => (quote!(None), vec![]),
                    [Type::DragAction] => (quote!(Action(__action)), vec![quote!(__action)]),
                    [Type::String, Type::DragAction] => (
                        quote!(Data(__data, __action)),
                        vec![quote!(__data.to_owned()), quote!(__action)],
                    ),
                    [Type::StringList] => (quote!(Files(__paths)), vec![quote!(__paths.to_vec())]),
                    [Type::Length, Type::Length] => {
                        (quote!(Position(__x, __y)), vec![quote!(__x), quote!(__y)])
                    }
                    _ => return Err(format!("invalid drag handler signature: {name}")),
                };
                let body = self.typed_event_values(element, name, &values)?;
                handlers.extend(quote! {
                    (#name, ::xrack_runtime::DragArguments::#pattern) => { #body }
                });
            }
            out.extend(quote! {
                [#(#path),*] => {
                    if event.target_kind() != #kind { return false; }
                    match event.notification() {
                        #handlers
                        _ => {}
                    }
                    true
                },
            });
        }
        for (index, child) in static_children(element) {
            path.push(index);
            self.drag_arms(child, path, out)?;
            path.pop();
        }
        Ok(())
    }

    fn control_view_settings(
        &self,
        element: OperationRef<'a>,
        scope: &VisualScope<'_>,
        properties: &[&str],
    ) -> Result<Vec<TokenStream>, String> {
        children(element, "xr.bind")
            .filter_map(|binding| {
                let name = binding.attribute("name")?;
                properties.contains(&name).then_some((name, binding))
            })
            .map(|(name, binding)| {
                let value = self.visual_value(binding, scope)?;
                Ok(quote! {
                    #name => ::core::option::Option::Some(::xrack_runtime::Value::from(#value)),
                })
            })
            .collect()
    }

    fn row_numeric_preparation(
        &self,
        nodes: &[OperationRef<'a>],
        model: OperationRef<'a>,
        repeat: &RepeatScope<'_>,
        operand: OperationRef<'a>,
        guard: &TokenStream,
        field_current: &TokenStream,
        history: &TokenStream,
        projected_tree: bool,
    ) -> Result<TokenStream, String> {
        let editor = *nodes.last().ok_or("missing numeric row editor")?;
        let available = self.row_input_available(nodes, &repeat.row, false, &quote!(input))?;
        let parameters = RepeatRef::from_operation(model)?;
        let item = parameter_ident(parameters.item);
        let index_binding = parameters.index.map(|name| {
            let index = parameter_ident(name);
            quote!(let #index = __xr_row_index;)
        });
        let repeater = repeat_field(repeat.index());
        let row = row_local(repeat.index());
        let source = self.expr(self.operand(model, 0)?)?;
        let kind = editor.attr("kind")?;
        let range = matches!(kind, "Slider" | "Dial");
        let properties = if range {
            xrack_runtime::range_view_properties(kind)
        } else {
            xrack_runtime::SPIN_VIEW_PROPERTIES
        };
        let settings = self.control_view_settings(editor, &repeat.row, properties)?;
        let (event, prepare) = if range {
            (
                quote!(::xrack_runtime::RoutedEvent::Range(__xr_action)),
                quote! {
                    ::xrack_runtime::prepare_range_view(
                        __xr_node, *__xr_action, __xr_available, __xr_read,
                        || input.prepare(|| true, || ::core::option::Option::Some(#history)),
                    ).flatten()
                },
            )
        } else {
            let value = self.visual_expr(operand, &repeat.row)?;
            (
                quote!(::xrack_runtime::RoutedEvent::Control(
                    ::xrack_runtime::ControlAction::Spin(__xr_action)
                )),
                quote! {
                    ::xrack_runtime::prepare_spin_view(
                        __xr_node, *__xr_action, __xr_available,
                        || ::xrack_runtime::Value::from(#value),
                        __xr_read,
                        || input.prepare(|| true, || ::core::option::Option::Some(#history)),
                    )
                },
            )
        };
        let row_model = if projected_tree {
            quote!(&source)
        } else {
            quote!(&__xr_repeater.resolve_model(|| #source))
        };
        let live_source = if projected_tree {
            quote!(__xr_model)
        } else {
            quote!(&__xr_repeater.resolve_model(|| #source))
        };
        Ok(quote! {{
            let #event = action else {
                return ::core::option::Option::None;
            };
            let __xr_source_index = index;
            let __xr_repeater = &self.__state.__visuals.#repeater;
            let __xr_node = cell;
            let __xr_live_row = || {
                let index = visual_index;
                #guard
                ::core::option::Option::Some(index)
            };
            let __xr_model = #row_model;
            __xr_repeater.with_resolved_row_state(
                __xr_model, index, key,
                |#item, __xr_row_index, #row| {
                    #index_binding
                    let __xr_available = || {
                        if __xr_live_row() != ::core::option::Option::Some(__xr_source_index) {
                            return false;
                        }
                        ::xrack_runtime::ListModel::contents_stamp(&**#live_source);
                        (#available) && (#field_current)
                    };
                    let __xr_read = |__xr_property: &::core::primitive::str| match __xr_property {
                        #(#settings)* _ => ::core::option::Option::None,
                    };
                    #prepare
                },
            ).flatten()
        }})
    }

    fn range_view_preparation(
        &self,
        element: OperationRef<'a>,
        path: &[usize],
        values: TokenStream,
    ) -> Result<TokenStream, String> {
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let available = self.element_allows_routed_input(element, path.is_empty(), &projection)?;
        let settings = self.control_view_settings(
            element,
            &self.visuals,
            xrack_runtime::range_view_properties(element.attr("kind")?),
        )?;
        Ok(quote! {
            ::xrack_runtime::prepare_range_view(
                node, action, || #available,
                |name| match name { #(#settings)* _ => ::core::option::Option::None },
                || #values,
            )
        })
    }

    fn range_with_live_bounds(
        &self,
        element: OperationRef<'a>,
        range: TokenStream,
    ) -> Result<TokenStream, String> {
        let minimum = self.binding_value_or(element, "minimum", quote!(0.0_f64))?;
        let maximum = self.binding_value_or(element, "maximum", quote!(100.0_f64))?;
        Ok(quote!((#range).with_bounds(#minimum, #maximum)))
    }

    fn range_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            return Ok(());
        }
        if matches!(element.attr("kind"), Ok("Slider" | "Dial")) {
            let current = self.control_value(element, "value", Type::Float)?;
            let prepared = self.range_view_preparation(element, path, current.clone())?;
            let set = self.set_control(element, &quote!(value))?;
            let changed = self.typed_event(element, "changed", &quote!(value))?;
            let released = self.typed_event(element, "released", &quote!(value))?;
            let released = if released.is_empty() {
                released
            } else {
                let live_range = self.range_with_live_bounds(element, quote!(range))?;
                quote! {
                    let value = (#live_range).with_value(#current).value();
                    #released
                }
            };
            out.extend(quote! {
                [#(#path),*] => {
                    let ::core::option::Option::Some(current) = #prepared else {
                        return false;
                    };
                    if !input.is_current() { return false; }
                    let range = ::xrack_runtime::SliderRange::from_node(node).with_value(current);
                    action.deliver(
                        range.apply(action),
                        |value| { #set #changed },
                        || { #released },
                    )
                },
            });
        } else if element.attr("kind") == Ok("ScrollBar")
            && children(element, "xr.scroll_target").next().is_none()
        {
            let position = self.control_value(element, "position", Type::Float)?;
            let prepared = self.range_view_preparation(element, path, position)?;
            let set = self.set_control(element, &quote!(value))?;
            let changed = self.typed_event(element, "changed", &quote!(value))?;
            out.extend(quote! {
                [#(#path),*] => {
                    if !node.boolean("interactive", true) { return false; }
                    let ::core::option::Option::Some(position) = #prepared else {
                        return false;
                    };
                    if !input.is_current() { return false; }
                    let range = ::xrack_runtime::ScrollBarRange::from_node(node).with_position(position);
                    let Some(value) = range.apply(action) else { return false; };
                    #set
                    #changed
                    true
                },
            });
        } else if element.attr("kind") == Ok("ColorPickerPopup") {
            let popup = picker_field(element_key(element)?);
            let show_alpha = self.binding_value_or(element, "show-alpha", quote!(false))?;
            let show_plane = self.binding_value_or(element, "show-color-plane", quote!(false))?;
            out.extend(quote! {
                [#(#path,)* 0, 5, ..] => self.__state.#popup.accepts_input(node, input)
                    && (#show_plane) && input.is_current() && self.__state.#popup.adjust_plane(node, action),
                [#(#path,)* 0, ..] => {
                    if !self.__state.#popup.accepts_input(node, input) { return false; }
                    let show_alpha = #show_alpha;
                    input.is_current() && self.__state.#popup.slide(show_alpha, node, action)
                },
            });
        } else if element.attr("kind") == Ok("RangeSlider") {
            let lower = self.control_value(element, "lower-value", Type::Float)?;
            let upper = self.control_value(element, "upper-value", Type::Float)?;
            let prepared = self.range_view_preparation(element, path, quote!([#lower, #upper]))?;
            let set_lower =
                self.set_control_property(element, "lower-value", &quote!(next_lower))?;
            let set_upper =
                self.set_control_property(element, "upper-value", &quote!(next_upper))?;
            let values = [quote!(__xr_lower), quote!(__xr_upper)];
            let handler = |event| {
                let body = self.typed_event_values(element, event, &values)?;
                if body.is_empty() {
                    return Ok::<_, String>(body);
                }
                let live_range = self.range_with_live_bounds(
                    element,
                    quote!(::xrack_runtime::SliderRange::from_node(node)),
                )?;
                Ok(quote! {
                    let [__xr_lower, __xr_upper] =
                        ::xrack_runtime::SliderInterval::from_range(#live_range, #lower, #upper).values();
                    #body
                })
            };
            let changed = handler("changed")?;
            let released = handler("released")?;
            out.extend(quote! {
                [#(#path,)* handle @ (0 | 1)] => {
                    let ::core::option::Option::Some([raw_lower, raw_upper]) = #prepared else {
                        return false;
                    };
                    if !input.is_current() { return false; }
                    let interval = ::xrack_runtime::SliderInterval::from_values(node, raw_lower, raw_upper);
                    action.deliver(
                        interval.changed_values(*handle, action, [raw_lower, raw_upper]),
                        |[next_lower, next_upper]| {
                            if let ::core::option::Option::Some(next_lower) = next_lower { #set_lower }
                            if let ::core::option::Option::Some(next_upper) = next_upper { #set_upper }
                            #changed
                        },
                        || { #released },
                    )
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

    fn combo_actions(
        &self,
        element: OperationRef<'a>,
        is_root: bool,
    ) -> Result<TokenStream, String> {
        let record = self.record_combos.get(element_key(element)?);
        let ty = match record {
            Some(combo) => self.record_field_type(combo.record.clone(), &combo.value)?,
            None => Type::String,
        };
        let variant = combo_value_variant(&ty)?;
        let by_value =
            controlled_property(element).is_some_and(|(name, _)| name == "current-value");
        let chosen = if by_value {
            quote!(__xr_value.clone())
        } else {
            quote!(selection.index as ::core::primitive::i64)
        };
        let set = self.set_control(element, &chosen)?;
        let selected = self.typed_event(element, "selected", &quote!(__xr_label.to_owned()))?;
        let records = record.is_some();
        let mut updates = TokenStream::new();
        if records {
            let value_selected =
                self.typed_event(element, "value-selected", &quote!(__xr_value.clone()))?;
            if !value_selected.is_empty() {
                updates.extend(quote! {
                    ::xrack_runtime::ComboUpdate::ValueSelected(::xrack_runtime::Value::#variant(__xr_value)) => { #value_selected },
                });
            }
        }
        let (finish_state, finish) = if has_editing_finished(element) {
            let handle = focus_handle(element_key(element)?);
            let accepts = self.check_text_submission(element, quote!(__xr_text))?;
            let (_, live_draft) = self.combo_state_value(element, "edit-text")?;
            let handler = self.typed_event_values(
                element,
                "editing-finished",
                &[quote!(__xr_text), quote!(__xr_acceptable)],
            )?;
            updates.extend(quote! {
                ::xrack_runtime::ComboUpdate::FinishEdit { text: __xr_text, accepted } => {
                    if __xr_edit_before.as_deref().is_some_and(|before| {
                        self.__state.#handle.take_editor_changes(before)
                    }) {
                        __xr_finished = Some((__xr_text.to_owned(), accepted || #accepts));
                    }
                },
            });
            (
                quote! {
                    let __xr_edit_before: Option<::std::string::String> =
                        self.__state.#handle.has_editor_changes().then(|| { #live_draft });
                    if !input.is_current() { return false; }
                    let mut __xr_finished = ::core::option::Option::None;
                },
                quote! {
                    if applied && let Some((__xr_text, __xr_acceptable)) = __xr_finished {
                        #handler
                    }
                },
            )
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let available = self.element_allows_routed_input(element, is_root, &projection)?;
        let editable = self.binding_value_or(element, "editable", quote!(false))?;
        let mut adoption = TokenStream::new();
        let editor = if combo_editor(element) {
            let text = quote!(__xr_text.to_owned());
            let accepted = self.typed_event(element, "accepted", &text)?;
            let accepted = if accepted.is_empty() || by_value && !records {
                accepted
            } else {
                adoption = quote!(let __xr_can_adopt = choice.accepts_unmatched_text(););
                let draft = self.combo_explicit_draft(element)?;
                let (property, ty) =
                    controlled_property(element).ok_or("missing ComboBox selection")?;
                let current = self.control_value(element, property, ty)?;
                let live_model = self.combo_state_model(element, record)?;
                quote! {
                    if __xr_can_adopt {
                        let selection = ::xrack_runtime::resolve_combo_after_accept(
                            __xr_text,
                            || ((#draft).as_deref() == Some(__xr_text)).then(|| ::xrack_runtime::Value::from(#current)),
                            || { #accepted },
                            || (#available) && (#editable),
                            || #live_model,
                        );
                        if let Some(selection) = selection
                            && let ::xrack_runtime::Value::#variant(__xr_value) = &selection.value
                        {
                            #set
                        }
                    } else {
                        #accepted
                    }
                }
            };
            let set_text = self.set_control_property(element, "edit-text", &text)?;
            updates.extend(quote! {
                ::xrack_runtime::ComboUpdate::SetEditText(__xr_text) => { #set_text },
                ::xrack_runtime::ComboUpdate::Accepted(__xr_text) => { #accepted },
            });
            if !by_value {
                let clear = self.set_control(element, &quote!(-1_i64))?;
                updates.extend(quote!(::xrack_runtime::ComboUpdate::ClearIndex => { #clear },));
            } else if !records {
                let free_text = self.set_control(element, &text)?;
                updates.extend(
                    quote!(::xrack_runtime::ComboUpdate::SetFreeText(__xr_text) => { #free_text },),
                );
            }
            if default_combo_edit_text(element) {
                let field = control_field(element_key(element)?, "edit-text");
                updates.extend(quote!(::xrack_runtime::ComboUpdate::ResetEditText => self.__state.#field.reset(),));
                quote!(Default)
            } else {
                quote!(Explicit)
            }
        } else {
            quote!(None)
        };
        let model_current = self.combo_model_current(element)?;
        // Model identity was already checked before resolving a submission.
        let draft_current = if combo_editor(element) {
            let current = self.combo_draft_current(element, &quote!(true))?;
            let accepts = self
                .check_text_submission(element, quote!(::xrack_runtime::text_input_value(node)))?;
            quote!((#current) && (#accepts))
        } else {
            quote!(false)
        };
        Ok(quote! {
            let ::core::option::Option::Some(choice) = ::xrack_runtime::prepare_combo_input_with_live_draft(
                node, action, || #available, || #editable, || #model_current, || #draft_current,
            ) else { return false; };
            if !input.is_current() { return false; }
            #finish_state
            #adoption
            let applied = choice.apply(::xrack_runtime::ComboEditor::#editor, #by_value, #records, |update| {
                match update {
                    ::xrack_runtime::ComboUpdate::Select(selection) => {
                        let ::xrack_runtime::Value::#variant(__xr_value) = &selection.value else { return false; };
                        #set
                    }
                    ::xrack_runtime::ComboUpdate::Selected(__xr_label) => { #selected }
                    #updates
                    _ => {}
                }
                true
            });
            #finish
            applied
        })
    }

    fn combo_model_current(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let Some(binding) = model_binding(element) else {
            return Ok(quote!(true));
        };
        let model = self.visual_value(binding, &self.visuals)?;
        Ok(quote!(::xrack_runtime::combo_model_is_current(node, &#model)))
    }

    fn combo_explicit_draft(&self, element: OperationRef<'a>) -> Result<TokenStream, String> {
        let draft = if default_combo_edit_text(element) {
            let field = control_field(element_key(element)?, "edit-text");
            quote!(self.__state.#field.explicit_value())
        } else {
            let text = self.control_value(element, "edit-text", Type::String)?;
            quote!(::core::option::Option::Some(#text))
        };
        Ok(draft)
    }

    fn combo_draft_current(
        &self,
        element: OperationRef<'a>,
        model_current: &TokenStream,
    ) -> Result<TokenStream, String> {
        let draft = self.combo_explicit_draft(element)?;
        let (name, ty) = controlled_property(element).ok_or("missing ComboBox selection")?;
        let selection = self.control_value(element, name, ty)?;
        Ok(quote!(node.combo_draft_is_current(#draft, || {
            (#model_current) && node.combo_selection_is_current(
                #name, &::xrack_runtime::Value::from(#selection),
            )
        })))
    }

    fn navigation_model_move(
        &self,
        group: OperationRef<'a>,
        valid: TokenStream,
    ) -> Result<TokenStream, String> {
        let event = xrack_syntax::builtins::navigation_move_event(group.attr("kind")?)
            .ok_or("missing navigation move event")?;
        let event =
            self.typed_event_values(group, event, &[quote!(from as i64), quote!(to as i64)])?;
        let (current, update) = if controlled_property(group)
            .is_some_and(|(name, _)| name == "current-index")
        {
            let current = self.control_value(group, "current-index", Type::Int)?;
            let set = self.set_control(group, &quote!(__xr_next))?;
            (
                quote! {
                    let __xr_previous = #current;
                    let __xr_next = ::xrack_runtime::moved_navigation_index(from, to, __xr_previous);
                },
                quote! { if __xr_next != __xr_previous { #set } },
            )
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        Ok(quote! {{
            if from == to || from.max(to) >= ::xrack_runtime::ListModel::row_count(&*model) {
                false
            } else {
                #current
                if (#valid) && model.move_row(from, to) {
                    #update
                    #event
                    true
                } else {
                    false
                }
            }
        }})
    }

    fn tab_move_arm(&self, bar: OperationRef<'a>) -> Result<TokenStream, String> {
        let model = self
            .group_option_model(bar, &self.visuals)?
            .ok_or("missing reorderable tab model")?;
        let read = self.group_option_reader(bar, GroupOptionRead::Select(false))?;
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        let enabled = self.row_binding_or(bar, "reorderable", quote!(false), &projection)?;
        let commit = self.navigation_model_move(
            bar,
            quote!(input.is_current() && movement.matches_model(&*model)),
        )?;
        Ok(quote! {
            if let ::xrack_runtime::ControlAction::MoveTab(movement) = action {
                let model = #model;
                if !::xrack_runtime::prepare_tab_move(node, movement, input, &model, || #enabled, #read)
                {
                    return false;
                }
                let (from, to) = (movement.from, movement.to);
                return #commit;
            }
        })
    }

    fn control_arms(
        &self,
        element: OperationRef<'a>,
        path: &mut Vec<usize>,
        out: &mut TokenStream,
    ) -> Result<(), String> {
        if element.name() == "xr.instance" {
            return Ok(());
        }
        match element.attr("kind")? {
            "Expander" => {
                let change =
                    self.expander_change(element, &quote!(Some(input)), &quote!(Some(expanded)))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Expander(expanded) = action else { return false; };
                        #change
                    },
                });
            }
            "Switch" => out.extend(quote! {
                [#(#path),*] => matches!(action, ::xrack_runtime::ControlAction::SetSwitch(_))
                    && self.__dispatch_path(path, "clicked", input),
            }),
            "StyledText"
                if children(element, "xr.handler")
                    .any(|handler| handler.attr("name") == Ok("link-activated")) =>
            {
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let text = self.row_binding_or(
                    element,
                    "text",
                    quote!(::xrack_runtime::StyledText::default()),
                    &projection,
                )?;
                let activated = self.typed_event_values(
                    element,
                    "link-activated",
                    &[quote!(__xr_href.to_owned())],
                )?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::ActivateTextLink(index) = action else { return false; };
                        let Some(__xr_href) = node.prepare_text_link(index, input, || #text) else {
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
                let activate =
                    action_activate_method(action_ref(element).ok_or("missing action reference")?);
                let expected = match kind {
                    "MenuItem" => quote!(ActivateMenuItem),
                    "Shortcut" => quote!(ActivateShortcut),
                    _ => quote!(ActivateAction),
                };
                out.extend(quote! {
                    [#(#path),*] => action == ::xrack_runtime::ControlAction::#expected
                        && self.#activate(|| true, Some(input)),
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
                let toggle = self.set_control(element, &quote!(__xr_next))?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                let checkable = self.binding_value_or(element, "checkable", quote!(false))?;
                let activated = children(element, "xr.handler")
                    .find(|handler| handler.attr("name") == Ok("activated"))
                    .map(|handler| self.statements(handler))
                    .transpose()?
                    .unwrap_or_default();
                out.extend(quote! {
                    [#(#path),*] => {
                        if action != ::xrack_runtime::ControlAction::ActivateMenuItem { return false; }
                        let ::core::option::Option::Some(next) = ::xrack_runtime::prepare_action_activation(
                            || #available, || #checkable, || #checked,
                        ) else { return false; };
                        if !input.is_current() { return false; }
                        if let ::core::option::Option::Some(__xr_next) = next { #toggle }
                        #activated
                        true
                    },
                });
            }
            "Dialog" => {
                let open = self.control_value(element, "open", Type::Bool)?;
                let close = self.set_control(element, &quote!(false))?;
                let mut handlers = TokenStream::new();
                use xrack_runtime::DialogEvent;
                for (event, variant) in [
                    (DialogEvent::Accepted, quote!(Accepted)),
                    (DialogEvent::Rejected, quote!(Rejected)),
                    (DialogEvent::Applied, quote!(Applied)),
                    (DialogEvent::Reset, quote!(Reset)),
                    (DialogEvent::HelpRequested, quote!(HelpRequested)),
                    (DialogEvent::Discarded, quote!(Discarded)),
                ] {
                    let body = self.typed_event_values(element, event.handler(), &[])?;
                    handlers.extend(quote!(::xrack_runtime::DialogEvent::#variant => { #body },));
                }
                out.extend(quote! {
                    [#(#path),*] => ::xrack_runtime::DialogEvent::deliver(
                        action, input, || #open, || { #close }, |event| match event { #handlers },
                    ),
                });
            }
            "Drawer" => {
                let open = self.control_value(element, "open", Type::Bool)?;
                let set = self.set_control(element, &quote!(__xr_open))?;
                let opened = self.typed_event_values(element, "opened", &[])?;
                let closed = self.typed_event_values(element, "closed", &[])?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Drawer(open) = action else { return false; };
                        ::xrack_runtime::set_overlay_open(
                            open, Some(input), || #open, |__xr_open| { #set },
                            |__xr_open| { if __xr_open { #opened } else { #closed } },
                        )
                    },
                });
            }
            "PopupWindow" => {
                let method = popup_method(element_key(element)?);
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Popup(open) = action else { return false; };
                        self.#method(open, Some(input))
                    },
                });
            }
            "SwipeDelegate" => {
                let current = self.control_value(element, "open-side", Type::SwipeSide)?;
                let set = self.set_control(element, &quote!(__xr_side))?;
                let changed = self.typed_event(element, "changed", &quote!(__xr_side))?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let mut panels = TokenStream::new();
                for panel in scene_children(element) {
                    let kind = panel.attr("kind")?;
                    if matches!(kind, "SwipeLeft" | "SwipeRight") {
                        let visible =
                            self.row_binding_or(panel, "visible", quote!(true), &projection)?;
                        panels.extend(quote!(#kind => #visible,));
                    }
                }
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::SwipeDelegate(__xr_side) = action else { return false; };
                        if !::xrack_runtime::prepare_swipe_side(
                            node, __xr_side, input, || #current,
                            |panel| match panel { #panels _ => false },
                        ) {
                            return false;
                        }
                        #set
                        #changed
                        true
                    },
                });
            }
            kind @ ("SpinBox" | "DoubleSpinBox") => {
                let (apply, ty) = if kind == "DoubleSpinBox" {
                    (
                        quote!(::xrack_runtime::decimal_spin_value_after),
                        Type::Float,
                    )
                } else {
                    (quote!(::xrack_runtime::spin_value_after), Type::Int)
                };
                let current = self.control_value(element, "value", ty)?;
                let set = self.set_control(element, &quote!(__xr_value))?;
                let edited = self.typed_event(element, "edited", &quote!(__xr_value))?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                let settings = self.control_view_settings(
                    element,
                    &self.visuals,
                    xrack_runtime::SPIN_VIEW_PROPERTIES,
                )?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Spin(action) = action else { return false; };
                        let Some(__xr_value) = #apply(node, action) else { return false; };
                        if !::xrack_runtime::spin_view_is_current(
                            node, action,
                            || #available,
                            || ::xrack_runtime::Value::from(#current),
                            |name| match name { #(#settings)* _ => ::core::option::Option::None },
                        ) {
                            return false;
                        }
                        if !input.is_current() { return false; }
                        #set
                        #edited
                        true
                    },
                });
            }
            "TreeView" => {
                let method = tree_input_method(element_key(element)?);
                out.extend(quote!([#(#path),*] => self.#method(node, input),));
            }
            "TableView" | "StandardTableView" => {
                let key = element_key(element)?;
                let field = table_sort_field(key);
                let sort_table = table_sort_method(key);
                let widths = table_widths_field(key);
                let order = table_order_field(key);
                let move_column = table_move_method(key);
                let move_row = table_move_row_method(key);
                let heights = table_row_heights_field(key);
                let rows = self.table_rows_model(element)?;
                let select = table_selection_input_method(key);
                let drag_flags = [
                    "resizable-columns",
                    "resizable-rows",
                    "movable-columns",
                    "movable-rows",
                ]
                .into_iter()
                .map(|name| {
                    let value = self.binding_value_or(element, name, quote!(false))?;
                    Ok(quote!(#name => #value,))
                })
                .collect::<Result<TokenStream, String>>()?;
                let resize_columns = table_columns(element)
                    .enumerate()
                    .map(|(index, column)| {
                        let value = self.binding_value_or(column, "resizable", quote!(true))?;
                        Ok(quote!(#index => #value,))
                    })
                    .collect::<Result<TokenStream, String>>()?;
                out.extend(quote! {
                    [#(#path),*] => {
                        if matches!(action, ::xrack_runtime::ControlAction::Select(_) | ::xrack_runtime::ControlAction::TableSelection(_)) {
                            return self.#select(node, action, input);
                        }
                        if !::xrack_runtime::table_drag_available(
                            node, input,
                            |name| match name { #drag_flags _ => false },
                            |column| match column { #resize_columns _ => false },
                        ) { return false; }
                        if let ::xrack_runtime::ControlAction::MoveRow { from, to, .. } = action {
                            return self.#move_row(from as ::core::primitive::i64, to as ::core::primitive::i64, Some(input));
                        }
                        use ::xrack_runtime::TableHeaderAction;
                        let Some((action, rows)) = TableHeaderAction::prepare(
                            node, input, action,
                            || self.__state.#field.get(|| None),
                            || self.__state.#order.get(::core::default::Default::default),
                            || #rows,
                        ) else { return false; };
                        match action {
                            TableHeaderAction::ResizeRow { row, key, height } => {
                                self.__state.#heights.set(&rows, row, Some(key), ::core::primitive::f64::from(height))
                            }
                            TableHeaderAction::MoveColumn { from, to } => {
                                self.#move_column(from as ::core::primitive::i64, to as ::core::primitive::i64)
                            }
                            TableHeaderAction::ResizeColumn { column, width } => {
                                ::xrack_runtime::resize_table_column(&self.__state.#widths, column, width)
                            }
                            TableHeaderAction::Sort(sort) => self.#sort_table(Some(sort), Some(input)),
                        }
                    },
                });
            }
            kind @ ("ListView" | "GridView" | "StandardListView") => {
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
                let current = self.list_selection_index(element)?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                let value = quote!(__xr_index);
                let (prepare, selected) = match self.list_selection_value(element, &value)? {
                    Some(key) => (
                        quote!(let Some(__xr_selection) = input.prepare(|| true, || #key) else { return false; };),
                        quote!(__xr_selection),
                    ),
                    None => (TokenStream::new(), value.clone()),
                };
                let set = self.set_control(element, &selected)?;
                let event = self.typed_event(element, "changed", &value)?;
                let extended =
                    self.list_selection_input(element, &available, &current, &projection)?;
                out.extend(quote! {
                    [#(#path),*] => {
                        #extended
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let ::core::option::Option::Some(__xr_index) = ::xrack_runtime::prepare_index_selection(
                            index, || #available,
                            |_| ::xrack_runtime::ListMetrics::has_row_in_model(node, index, #stamp),
                            || #current,
                        ) else { return false; };
                        #prepare
                        if !input.is_current() { return false; }
                        #set
                        #event
                        true
                    },
                });
            }
            kind @ ("RadioButton" | "Button" | "ToolButton" | "RoundButton")
                if radio_group_ref(element).is_some() =>
            {
                let key = radio_group_ref(element).ok_or("missing ButtonGroup reference")?;
                let group = *self.elements.get(key).ok_or("unknown ButtonGroup")?;
                let (property, ty) =
                    controlled_property(group).ok_or("missing ButtonGroup selection")?;
                let current = self.control_value(group, property, ty)?;
                let receiver = quote!(self);
                let projection = TableProjection::new(ExpressionContext::new(&receiver));
                let enabled = self.row_binding_or(group, "enabled", quote!(true), &projection)?;
                let label = self.live_radio_value(element, &self.visuals)?;
                let selection = if property == "current-value" {
                    quote!(Value)
                } else {
                    quote!(Index)
                };
                let next = if property == "current-value" {
                    quote!(__xr_value.clone())
                } else {
                    quote!(__xr_index as ::core::primitive::i64)
                };
                let set = self.set_control(group, &quote!(__xr_next))?;
                let selected = self.typed_event(group, "selected", &quote!(__xr_value))?;
                let toggled = self.typed_event_values(element, "toggled", &[])?;
                let clicked = self.typed_event_values(element, "clicked", &[])?;
                let push_button = xrack_syntax::builtins::push_button(kind);
                out.extend(quote! {
                    [#(#path),*] => {
                        if action != ::xrack_runtime::ControlAction::ActivateRadioOption { return false; }
                        let ::core::option::Option::Some((__xr_index, __xr_value, __xr_changed)) = ::xrack_runtime::prepare_button_group_option(
                            node, #key, input, || #enabled, || #label,
                            || ::xrack_runtime::ButtonGroupSelection::#selection(#current),
                        ) else { return false; };
                        if __xr_changed {
                            let __xr_next = #next;
                            #set
                            #toggled
                            #selected
                        }
                        let __xr_click = #push_button && matches!(input.event(), ::xrack_runtime::RoutedEvent::Dispatch("clicked"));
                        if __xr_click { #clicked }
                        __xr_changed || __xr_click
                    },
                });
            }
            "RadioGroup" | "TabWidget" | "TabBar" | "SwipeView" => {
                let radio = element.attr("kind")? == "RadioGroup";
                let (model_current, read_option) = if radio
                    && scene_children(element)
                        .any(|child| children(child, "xr.repeat").next().is_some())
                {
                    self.radio_group_options(element)?
                } else {
                    let model = self
                        .group_option_model(element, &self.visuals)?
                        .map_or_else(
                            || quote!(::core::option::Option::Some(())),
                            |model| {
                                quote!({
                                    let model = #model;
                                    node.group_options_are_current(&*model).then_some(model)
                                })
                            },
                        );
                    (
                        model,
                        self.group_option_reader(
                            element,
                            GroupOptionRead::Select(radio || navigation_selects_by_value(element)),
                        )?,
                    )
                };
                let (property, ty) =
                    controlled_property(element).ok_or("missing group selection")?;
                let current = self.control_value(element, property, ty)?;
                let selection = if property == "current-value" {
                    quote!(Value)
                } else {
                    quote!(Index)
                };
                let value = if property == "current-value" {
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
                let move_tab = if matches!(element.attribute("kind"), Some("TabBar" | "TabWidget"))
                    && children(element, "xr.bind")
                        .any(|bind| bind.attribute("name") == Some("reorderable"))
                {
                    self.tab_move_arm(element)?
                } else {
                    TokenStream::new()
                };
                let close_tab = if matches!(element.attribute("kind"), Some("TabBar" | "TabWidget"))
                    && children(element, "xr.handler")
                        .any(|handler| handler.attr("name") == Ok("tab-close-requested"))
                {
                    let read = self.group_option_reader(element, GroupOptionRead::Close)?;
                    let receiver = quote!(self);
                    let projection = TableProjection::for_input(&receiver);
                    let enabled =
                        self.row_binding_or(element, "tabs-closable", quote!(false), &projection)?;
                    let event = self.typed_event_values(
                        element,
                        "tab-close-requested",
                        &[quote!(request.index as i64), quote!(__xr_tab_value)],
                    )?;
                    quote! {
                        if let ::xrack_runtime::ControlAction::CloseTab(request) = action {
                            let __xr_tabs_closable = || #enabled;
                            let ::core::option::Option::Some(__xr_tab_value) =
                                ::xrack_runtime::prepare_tab_close(node, request, input, || #model_current, #read)
                                else { return false; };
                            #event
                            return true;
                        }
                    }
                } else {
                    TokenStream::new()
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        #close_tab
                        #move_tab
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let ::core::option::Option::Some(__xr_label) = ::xrack_runtime::prepare_group_selection_with_model(
                            node, index, input, || #model_current,
                            || ::xrack_runtime::ButtonGroupSelection::#selection(#current), #read_option,
                        ) else { return false; };
                        #set
                        #event
                        true
                    },
                });
            }
            "SearchField" => {
                let control = search_method(element_key(element)?, "control");
                out.extend(quote!([#(#path),*] => self.#control(node, action, input),));
            }
            "DelayButton" => {
                let current = self.control_value(element, "checked", Type::Bool)?;
                let set = self.set_control(element, &quote!(__xr_checked))?;
                let activated = self.typed_event_values(element, "activated", &[])?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                let delay = children(element, "xr.bind")
                    .find(|binding| binding.attr("name") == Ok("delay"))
                    .map(|binding| {
                        let value = self.visual_value(binding, &self.visuals)?;
                        Ok::<_, String>(quote!(::core::option::Option::Some(#value)))
                    })
                    .transpose()?
                    .unwrap_or_else(|| quote!(::core::option::Option::None));
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::DelayButton(__xr_checked) = action else { return false; };
                        if !::xrack_runtime::delay_button_transition_is_current(
                            node, __xr_checked, || #available, || #delay, || #current,
                        ) {
                            return false;
                        }
                        if !input.is_current() { return false; }
                        #set
                        if __xr_checked { #activated }
                        true
                    },
                });
            }
            "Tumbler" | "PathView" => {
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                let changed = self.typed_event(element, "changed", &quote!(__xr_index))?;
                if let Some(combo) = self.record_combos.get(element_key(element)?) {
                    let ty = self.record_field_type(combo.record.clone(), &combo.value)?;
                    let variant = combo_value_variant(&ty)?;
                    let items = self.combo_items(element, combo, &self.visuals)?;
                    let (stored, ty) =
                        controlled_property(element).ok_or("missing Tumbler selection")?;
                    let current = self.control_value(element, stored, ty)?;
                    let (index, value, chosen) = if stored == "current-value" {
                        (
                            quote!(0),
                            quote!(::core::option::Option::Some(&::xrack_runtime::Value::from(#current))),
                            quote!(__xr_value.clone()),
                        )
                    } else {
                        (
                            quote!(#current),
                            quote!(::core::option::Option::None),
                            quote!(__xr_index),
                        )
                    };
                    let set = self.set_control(element, &chosen)?;
                    let selected =
                        self.typed_event(element, "value-selected", &quote!(__xr_value))?;
                    out.extend(quote! {
                        [#(#path),*] => {
                            if !(#available) { return false; }
                            let ::core::option::Option::Some((__xr_index, ::xrack_runtime::Value::#variant(__xr_value))) =
                                ::xrack_runtime::Tumbler::prepare_record_selection(
                                    node, action, #items, #index, #value,
                                )
                            else { return false; };
                            if !input.is_current() { return false; }
                            #set
                            #changed
                            #selected
                            true
                        },
                    });
                } else {
                    let current = self.control_value(element, "current-index", Type::Int)?;
                    let set = self.set_control(element, &quote!(__xr_index))?;
                    let items = if let Some(binding) = children(element, "xr.bind")
                        .find(|binding| binding.attr("name") == Ok("model"))
                    {
                        let model = self.visual_value(binding, &self.visuals)?;
                        quote! {
                            ::core::option::Option::Some(::xrack_runtime::TumblerItems::new(#model))
                        }
                    } else {
                        quote!(::core::option::Option::None)
                    };
                    let count = self.binding_value_or(element, "count", quote!(0))?;
                    out.extend(quote! {
                    [#(#path),*] => {
                        let ::core::option::Option::Some(__xr_index) = ::xrack_runtime::Tumbler::prepare_selection(
                            node, action, || #available, || #items, || #count, || #current,
                        ) else {
                            return false;
                        };
                        if !input.is_current() { return false; }
                        #set
                        #changed
                        true
                    },
                });
                }
            }
            kind @ ("Calendar" | "MonthGrid") => {
                let set_date = self.set_control_property(element, "date", &quote!(__xr_date))?;
                let set_month =
                    self.set_control_property(element, "display-month", &quote!(__xr_month))?;
                let date_changed = self.typed_event(element, "changed", &quote!(__xr_date))?;
                let month_changed =
                    self.typed_event(element, "month-changed", &quote!(__xr_month))?;
                let activated = if kind == "MonthGrid" {
                    self.typed_event(element, "activated", &quote!(__xr_activated))?
                } else {
                    TokenStream::new()
                };
                let minimum = self.binding_value_or(element, "minimum-date", quote!(fallback))?;
                let maximum = self.binding_value_or(element, "maximum-date", quote!(fallback))?;
                let date = self.control_value(element, "date", Type::Date)?;
                let month = self.control_value(element, "display-month", Type::Date)?;
                let week_start = self.binding_value_or(element, "week-start", quote!(1))?;
                let weekdays = self.disabled_calendar_weekdays(element)?;
                let dates = self.disabled_calendar_dates(element)?;
                let fixed_month = self.binding_value_or(element, "fixed-month", quote!(false))?;
                out.extend(quote! {
                    [#(#path),*] => {
                        use ::xrack_runtime::CalendarUpdate;
                        let ::xrack_runtime::ControlAction::Calendar(action) = action else { return false; };
                        if !::xrack_runtime::calendar_input_is_current(node, input, |name, fallback| match name {
                            "minimum-date" => #minimum,
                            "maximum-date" => #maximum,
                            "date" => #date,
                            "display-month" => #month,
                            _ => fallback,
                        }, || #week_start, || #weekdays, || #dates, || #fixed_month) || !input.is_current() {
                            return false;
                        }
                        ::xrack_runtime::apply_calendar_action(node, action, |update| match update {
                            CalendarUpdate::SetDate(__xr_date) => { #set_date }
                            CalendarUpdate::SetMonth(__xr_month) => { #set_month }
                            CalendarUpdate::DateChanged(__xr_date) => { #date_changed }
                            CalendarUpdate::MonthChanged(__xr_month) => { #month_changed }
                            CalendarUpdate::Activated(__xr_activated) => { #activated }
                        })
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
                        let Some(__xr_time) = ::xrack_runtime::TimePickerOptions::select_input(
                            node, *slot, action, input, || #options, || #current,
                        ) else { return false; };
                        #set
                        #changed
                        true
                    },
                });
            }
            kind @ ("TimePickerPopup" | "DatePickerPopup" | "ColorPickerPopup"
            | "FontPickerPopup") => {
                let popup = picker_field(element_key(element)?);
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
                            [#(#path,)* 0, slot] => {
                                self.__state.#popup.edit_input(node, *slot, action, input, || #options)
                            },
                        });
                        quote!(self.__state.#popup.finish_input(node, role, input, || #options))
                    }
                    "DatePickerPopup" => {
                        let bounds = self.date_picker_bounds(element)?;
                        out.extend(quote! {
                            [#(#path,)* 0] => {
                                if !self.__state.#popup.accepts_input(node, input) { return false; }
                                let bounds = #bounds;
                                input.is_current() && self.__state.#popup.edit(bounds, node, action)
                            },
                        });
                        quote!(self.__state.#popup.finish_input(node, role, input, || #bounds))
                    }
                    "FontPickerPopup" => {
                        out.extend(quote! {
                            [#(#path,)* 0, ..] => input.is_current() && self.__state.#popup.edit(node, action),
                        });
                        quote!(self.__state.#popup.finish_with_role(node, role))
                    }
                    _ => {
                        let show_alpha =
                            self.binding_value_or(element, "show-alpha", quote!(false))?;
                        out.extend(quote! {
                            [#(#path,)* 0, ..] => {
                                let ::xrack_runtime::ControlAction::Spin(action) = action else { return false; };
                                if !self.__state.#popup.accepts_input(node, input) { return false; }
                                let show_alpha = #show_alpha;
                                input.is_current() && self.__state.#popup.spin(show_alpha, node, action)
                            },
                        });
                        quote!(self.__state.#popup.finish_input(node, role, input, || #show_alpha))
                    }
                };
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Dialog(role) = action else { return false; };
                        let Some(__xr_value) = (#finish) else { return false; };
                        #close
                        match role {
                            ::xrack_runtime::DialogRole::Accept => { #set #accepted }
                            ::xrack_runtime::DialogRole::Reject => { #canceled }
                        }
                        true
                    },
                });
            }
            "ComboBox" => {
                let body = self.combo_actions(element, path.is_empty())?;
                out.extend(quote!([#(#path),*] => { #body },));
            }
            "PageIndicator" => {
                let value = quote!(__xr_index);
                let set = self.set_control(element, &value)?;
                let changed = self.typed_event(element, "changed", &value)?;
                let current = self.control_value(element, "current-index", Type::Int)?;
                let count = self.binding_value_or(element, "count", quote!(0_i64))?;
                let interactive = self.binding_value_or(element, "interactive", quote!(false))?;
                let receiver = quote!(self);
                let projection = TableProjection::for_input(&receiver);
                let available =
                    self.element_allows_routed_input(element, path.is_empty(), &projection)?;
                out.extend(quote! {
                    [#(#path),*] => {
                        let ::xrack_runtime::ControlAction::Select(index) = action else { return false; };
                        let ::core::option::Option::Some(__xr_index) = ::xrack_runtime::prepare_index_selection(
                            index, || (#available) && (#interactive), |index| index < #count, || #current,
                        ) else { return false; };
                        if !input.is_current() { return false; }
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
                        self.#method(Some(revision), ::xrack_runtime::StackNavigation::Pop, Some(input))
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
        table: Option<OperationRef<'a>>,
    ) -> Result<TokenStream, String> {
        let mut inputs = BTreeMap::new();
        let receiver = quote!(__ui);
        let expression_context = ExpressionContext {
            repeat_parameters: repeat.is_some(),
            row_context: repeat.is_some(),
            ..ExpressionContext::new(&receiver)
        };
        let (context, parameters) = if let Some(repeat) = repeat {
            let repeat = RepeatRef::from_operation(repeat)?;
            let item = parameter_ident(repeat.item);
            let mut parameters = quote!(let #item = || __context.item(););
            if let Some(index) = repeat.index {
                let index = parameter_ident(index);
                parameters.extend(quote!(let #index = || __context.index();));
            }
            (quote!(let __context = __context.clone();), parameters)
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        let instance_bindings = self.analysis.instances.get(element_key(instance)?);
        for input in instance_bindings
            .into_iter()
            .flat_map(|bindings| &bindings.inputs)
        {
            let binding = input.operation;
            let name = binding.attr("name")?;
            let value = if binding.name() == "xr.bind" {
                let expression = self.expr_in(self.operand(binding, 0)?, expression_context)?;
                quote! {
                    ::core::option::Option::Some({
                        let __weak = __parent.clone();
                        #context
                        ::xrack_runtime::InputProperty::bound_optional(move || {
                            let __ui = Self { __state: __weak.upgrade()? };
                            __ui.__state.__binding_lifetime.track();
                            #parameters
                            ::core::option::Option::Some(#expression)
                        })
                    })
                }
            } else {
                let link = self.instance_link_input(self.operand(binding, 0)?, repeat, table)?;
                quote!(::core::option::Option::Some(#link))
            };
            inputs.insert(input.endpoint.clone(), (name, value));
        }
        if repeat.is_none() {
            let key = element_key(instance)?;
            for (path, forward) in &self.analysis.property_forwards {
                if path.len() <= 1 || forward.target.0 != key {
                    continue;
                }
                let forwarded = Self::take_forwarded_inputs(&forward.inputs);
                let endpoint = &path[1..];
                let input = match inputs.remove(endpoint) {
                    Some((_, existing)) => quote!(#forwarded.or_else(|| #existing)),
                    None => forwarded,
                };
                inputs.insert(endpoint.to_vec(), (forward.target.1, input));
            }
        }
        for input in instance_bindings
            .into_iter()
            .flat_map(|bindings| &bindings.state_inputs)
        {
            let name = input.name;
            let endpoint = &input.endpoint;
            let cases = &input.cases;
            let selector = self.operand(cases[0], 0)?;
            let owner = selector.attr("owner")?;
            let selected = if self.analysis.persistent_state_selectors.contains_key(owner) {
                let method = state_selector_name(owner);
                quote!(__ui.#method())
            } else {
                self.state_choice(selector, |condition| {
                    self.expr_in(condition, expression_context)
                })?
            };
            let mut choices = TokenStream::new();
            for &case in cases {
                let case = StateCaseRef::from_operation(case)?;
                let index = case.index;
                let value = case
                    .value
                    .defining_operation()
                    .ok_or("state value has no definition")?;
                let value = self.expr_in(value, expression_context)?;
                choices.extend(quote!(#index => ::core::option::Option::Some(#value),));
            }
            let base = inputs.remove(endpoint).map_or_else(
                || quote!(::core::default::Default::default()),
                |(_, base)| quote!((#base).unwrap_or_default()),
            );
            inputs.insert(
                endpoint.clone(),
                (
                    name,
                    quote! {
                        ::core::option::Option::Some({
                            let __base: ::xrack_runtime::InputProperty<_> = #base;
                            let __weak = __parent.clone();
                            #context
                            __base.with_state(move || {
                                let __ui = Self { __state: __weak.upgrade()? };
                                __ui.__state.__binding_lifetime.track();
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
        let init_handle = self
            .globals
            .iter()
            .any(|op| op.attribute("sym_name") == Some(self.component_name))
            .then(|| quote!(__global_context: None,));
        let name = op.attr("name")?;
        let method = callback_method("callback_body", owner, name);
        let params = op.parameters()?;
        let args: Vec<_> = params.iter().map(|p| parameter_ident(&p.name)).collect();
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
                        __state.__binding_lifetime.track();
                        Self { __state, #init_handle }.#method(#context #(#args),*)
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
        let params = op.parameters()?;
        let args = params.iter().map(|p| parameter_ident(&p.name));
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
                if !binding.is_empty() && !params.iter().any(|p| p.name == binding) {
                    let binding = parameter_ident(binding);
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
        forward: &CallbackForward<'a>,
    ) -> Result<Option<TokenStream>, String> {
        let mut inputs: Vec<_> = forward
            .inputs
            .iter()
            .map(|name| {
                let field = callback_input_field(name);
                quote!(__inputs.#field.take())
            })
            .collect();
        if let Some(((owner, _), callback)) = forward.implementation {
            inputs.push(self.callback_handler_value(owner, callback, None)?);
        }
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
        let mut handlers = BTreeMap::new();
        let instance_bindings = self.analysis.instances.get(owner);
        for handler in instance_bindings
            .into_iter()
            .flat_map(|bindings| &bindings.callbacks)
        {
            let callback = handler.operation;
            let name = callback.attr("name")?;
            handlers.insert(
                handler.endpoint.clone(),
                (name, self.callback_handler_value(owner, callback, repeat)?),
            );
        }
        if repeat.is_none() {
            for (path, forward) in &self.analysis.callback_forwards {
                if path.len() <= 1 || forward.target.0 != owner {
                    continue;
                }
                let Some(input) = self.forwarded_callback(forward)? else {
                    continue;
                };
                let endpoint = &path[1..];
                let input = match handlers.remove(endpoint) {
                    Some((_, previous)) => quote!(#input.or_else(|| #previous)),
                    None => input,
                };
                handlers.insert(endpoint.to_vec(), (forward.target.1, input));
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
        table: Option<OperationRef<'a>>,
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
            let write = if let Some(table) = table {
                let rows = self.editable_table_rows_model(
                    table,
                    &receiver,
                    element_key(table)? == self.root_key,
                )?;
                let (prepare, commit) = if table_history_item(table)?.is_some() {
                    let key = element_key(table)?;
                    let history = table_history_field(key);
                    let enabled = table_history_method(key, "enabled");
                    let mut fields = BTreeMap::new();
                    self.record_paste_leaves(&mut field_path, &value_type(target)?, &mut fields);
                    let patch = Self::table_edit_capture(fields);
                    (
                        quote!(__ui.#enabled()),
                        quote!(__write_row.update_in_with_history(
                            target.rows.source(), &__ui.__state.#history, record_history, #update, #patch,
                        );),
                    )
                } else {
                    (
                        quote!(false),
                        quote!(__write_row.update_in(target.rows.source(), #update);),
                    )
                };
                quote! {
                    let target = __write_row.try_table_edit_target_for_field(
                        || ::core::result::Result::<_, ::core::convert::Infallible>::Ok(#rows),
                        |target| ::core::result::Result::Ok(__write_row.read_from(target.rows.source(), #project)),
                        || ::core::result::Result::Ok(::core::option::Option::Some(#prepare)),
                    ).unwrap_or_else(|error| match error {});
                    if let ::core::option::Option::Some((target, record_history)) = target {
                        #commit
                    }
                }
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
                    __ui.__state.__binding_lifetime.track();
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
        let root_key = self.root_key;
        let mut visual_declarations = TokenStream::new();
        let component_key = rust_name(component_name);
        self.visuals
            .declare(&component_key, &mut visual_declarations);
        let visuals = self.visuals.state_name(&component_key);
        let mut fields = quote! {
            __binding_lifetime: ::xrack_runtime::ComponentLifetime,
            __scene: ::xrack_runtime::Property<::std::rc::Rc<::xrack_runtime::Node>>,
            __visuals: #visuals,
        };
        let mut defaults = quote! {
            __binding_lifetime: ::core::default::Default::default(),
            __scene: ::core::default::Default::default(),
            __visuals: ::core::default::Default::default(),
        };
        for (&owner, element) in &self.elements {
            if element.attribute("kind") == Some("DynamicContainer") {
                let slot = dynamic_children(owner);
                fields.extend(quote!(#slot: ::xrack_runtime::DynamicChildren,));
                defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            }
        }
        let mut methods = self.loader_item_accessor()?;
        let mut viewport_initializers = TokenStream::new();
        for (&owner, &selector) in &self.analysis.persistent_state_selectors {
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
        for (&key, &element) in &self.elements {
            if list_multi_selection(element) {
                let field = list_selection_field(key);
                fields.extend(quote!(#field: ::xrack_runtime::TableSelection,));
                defaults.extend(quote!(#field: ::core::default::Default::default(),));
            }
        }
        for &view in &self.sectioned_lists {
            let field = format_ident!("__list_sections_{}", rust_key(view));
            fields.extend(quote!(#field: ::xrack_runtime::ListSectionProjection,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
        }
        for (&view, &template) in &self.section_headers {
            let field = section_renderer_field(view);
            let method = section_header_method(view);
            fields.extend(
                quote!(#field: ::std::cell::OnceCell<::xrack_runtime::ListSectionRenderer>,),
            );
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            let mut body = TokenStream::new();
            let header = self.render(template, &VisualScope::default(), &mut body, &mut 0)?;
            let inputs = section_header_inputs().map(|(name, _)| {
                let binding = parameter_ident(name);
                if name == "section-label" {
                    quote!(let #binding = __xr_section.label.to_string();)
                } else {
                    let field = ident(name.trim_start_matches("section-"));
                    quote!(let #binding = __xr_section.#field;)
                }
            });
            methods.extend(quote! {
                fn #method(&self, __xr_section: ::xrack_runtime::ListSectionInput) -> ::xrack_runtime::Node {
                    #(#inputs)*
                    #body
                    #header
                }
            });
        }
        for (&combo, &template) in &self.combo_options {
            let method = combo_option_method(combo);
            fields.extend(
                quote!(#method: ::std::cell::OnceCell<::xrack_runtime::ComboOptionRenderer>,),
            );
            defaults.extend(quote!(#method: ::core::default::Default::default(),));
            let mut body = TokenStream::new();
            let scope = VisualScope {
                combo_option: true,
                ..VisualScope::default()
            };
            let node = self.render(template, &scope, &mut body, &mut 0)?;
            let kind = template.attr("kind")?;
            let inputs = visual_template_inputs(kind)
                .ok_or("missing choice option inputs")?
                .into_iter()
                .map(|(name, _)| {
                    let binding = parameter_ident(name);
                    let field = ident(name.trim_start_matches("option-"));
                    if name == "option-text" {
                        quote!(let #binding = __xr_option.text.clone();)
                    } else {
                        quote!(let #binding = __xr_option.#field;)
                    }
                });
            let option_data = parameter_ident("option-data");
            let data = if let Some(combo) = self.record_combos.get(combo) {
                let record = ty(&Type::Record(combo.record.clone()));
                quote! {
                    let __xr_record = ::std::cell::OnceCell::<#record>::new();
                    let #option_data = || __xr_record
                        .get_or_init(|| __xr_option.record_data::<#record>().unwrap_or_default())
                        .clone();
                }
            } else if kind != "ComboOption"
                && !children(self.elements[combo], "xr.bind")
                    .any(|binding| binding.attribute("name") == Some("model"))
            {
                quote!(let #option_data = || __xr_option.index;)
            } else {
                quote!(let #option_data = || __xr_option.text.clone();)
            };
            methods.extend(quote! {
                fn #method(&self, __xr_option: ::xrack_runtime::ComboOptionInput) -> ::xrack_runtime::Node {
                    #(#inputs)*
                    #data
                    #body
                    #node
                }
            });
        }

        for &target in &self.button_press_targets {
            let handle = button_press_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::ButtonPressHandle,));
            let initial = if self.button_animation_targets.contains(target) {
                quote!(::xrack_runtime::ButtonPressHandle::animated())
            } else {
                quote!(::core::default::Default::default())
            };
            defaults.extend(quote!(#handle: #initial,));
        }
        for &target in &self.button_checked_targets {
            if self.repeated_buttons.contains(target) {
                continue;
            }
            let method = button_checked_method(target);
            let checked = self.button_checked(self.elements[target])?;
            methods.extend(quote! {
                fn #method(&self) -> ::core::primitive::bool { #checked }
            });
        }
        for &target in &self.toggle_state_targets {
            let control = self.elements[target];
            if children(control, "xr.row_link").next().is_some() {
                continue;
            }
            let method = toggle_state_method(target);
            let state = self.toggle_check_state(control)?;
            methods.extend(quote! {
                fn #method(&self) -> ::xrack_runtime::CheckState { #state }
            });
        }
        for &(target, property) in &self.numeric_read_targets {
            let control = self.elements[target];
            if children(control, "xr.row_link").any(|link| link.attr("name") == Ok(property)) {
                continue;
            }
            let (_, value_type) = controlled_properties(control)
                .find(|(name, _)| *name == property)
                .ok_or("missing numeric control value")?;
            let method = numeric_value_method(target, property);
            let value = self.control_value(control, property, value_type.clone())?;
            let ty = ty(&value_type);
            methods.extend(quote! {
                fn #method(&self) -> #ty { #value }
            });
        }
        for &(target, property) in &self.combo_state_targets {
            let method = combo_state_method(target, property);
            let (value_type, value) = self.combo_state_value(self.elements[target], property)?;
            let ty = ty(&value_type);
            methods.extend(quote! {
                fn #method(&self) -> #ty { #value }
            });
        }
        for (&target, properties) in &self.range_position_targets {
            let handle = range_position_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::RangePositionHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            for &property in properties {
                let method = range_position_method(target, property);
                let position = self.range_position(self.elements[target], property)?;
                methods.extend(quote! {
                    fn #method(&self) -> ::core::primitive::f64 { #position }
                });
            }
        }
        for &target in &self.combo_popup_targets {
            let handle = combo_popup_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::ComboPopupHandle,));
            defaults.extend(quote!(#handle: ::core::default::Default::default(),));
        }
        for &key in &self.radio_read_groups {
            let group = self.elements[key];
            let method = radio_choice_method(key);
            if group.attr("kind") == Ok("RadioGroup") {
                let selected = self.radio_choice(group)?;
                fields.extend(quote!(#method: ::xrack_runtime::Property<::core::option::Option<(usize, usize)>>,));
                defaults.extend(quote!(#method: ::core::default::Default::default(),));
                methods.extend(quote! {
                    fn #method(&self) -> ::core::option::Option<(usize, usize)> {
                        self.__state.#method.get(|| { #selected })
                    }
                });
            } else {
                let selected = self.virtual_radio_choice(group)?;
                methods.extend(quote! {
                    fn #method(&self) -> ::core::option::Option<usize> { #selected }
                });
            }
        }
        for &key in &self.navigation_read_groups {
            let method = navigation_choice_method(key);
            let selected = self.navigation_choice(self.elements[key])?;
            fields
                .extend(quote!(#method: ::xrack_runtime::Property<::core::option::Option<usize>>,));
            defaults.extend(quote!(#method: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #method(&self) -> ::core::option::Option<usize> {
                    self.__state.#method.get(|| { #selected })
                }
            });
        }
        for &target in &self.focus_targets {
            let handle = focus_handle(target);
            fields.extend(quote!(#handle: ::xrack_runtime::FocusHandle,));
            let selection = self.text_selection_targets.contains(target);
            let commands = self.text_command_targets.contains(target);
            let initial = if commands {
                quote!(::xrack_runtime::FocusHandle::with_text_state(#selection, true))
            } else if selection {
                quote!(::xrack_runtime::FocusHandle::with_text_selection())
            } else {
                quote!(::core::default::Default::default())
            };
            defaults.extend(quote!(#handle: #initial,));
        }
        for (&key, &element) in &self.elements {
            if element.attr("kind") == Ok("SwipeView") {
                let handle = swipe_preview_handle(key);
                fields.extend(quote!(#handle: ::xrack_runtime::SwipePreviewHandle,));
                defaults.extend(quote!(#handle: ::core::default::Default::default(),));
            }
            if element.attr("kind") == Ok("SearchField") {
                methods.extend(self.search_methods(element, key == root_key)?);
            }
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
            if self.elements.get(target).is_some_and(|element| {
                matches!(
                    element.attr("kind"),
                    Ok("Flickable" | "Tumbler" | "PathView")
                )
            }) {
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
                    ::xrack_runtime::ScrollBarRange::position_with(|| #position, || #size)
                }
            });
        }
        let mut input_fields = TokenStream::new();
        if !self.globals.is_empty() {
            let context = if component.attribute("global") == Some("true") {
                quote!(::std::rc::Weak<__XrGlobals>)
            } else {
                quote!(::std::rc::Rc<__XrGlobals>)
            };
            let retain = if component.attribute("global") == Some("true") {
                quote!(::std::rc::Rc::downgrade(&__inputs.__globals))
            } else {
                quote!(__inputs.__globals.clone())
            };
            fields.extend(quote!(__globals: #context,));
            input_fields.extend(quote!(__globals: ::std::rc::Rc<__XrGlobals>,));
            defaults.extend(quote!(__globals: #retain,));
            for &global in self.globals {
                let name = global.attr("sym_name")?;
                let method = global_accessor(name);
                let ty = ident(name);
                let cell = format_ident!("__{}", rust_key(name));
                let inputs = format_ident!("__Inputs_{}", rust_name(name));
                let registry = if component.attribute("global") == Some("true") {
                    quote!(
                        self.__state
                            .__globals
                            .upgrade()
                            .expect("live UI global registry")
                    )
                } else {
                    quote!(self.__state.__globals.clone())
                };
                let locale = self
                    .with_locale
                    .then(|| quote!(__locale: self.__state.__locale.clone(),));
                let public = if global.attribute("exported") == Some("true") {
                    quote!(pub)
                } else {
                    TokenStream::new()
                };
                methods.extend(quote! {
                    #[allow(non_snake_case)]
                    #public fn #method(&self) -> #ty {
                        let registry = #registry;
                        let global = registry.#cell.get_or_init(|| #ty::__new(#inputs {
                            __globals: registry.clone(),
                            #locale
                            ..::core::default::Default::default()
                        }));
                        #ty { __state: global.__state.clone(), __global_context: Some(registry) }
                    }
                });
            }
        }
        if self.with_locale {
            fields.extend(quote!(__locale: ::xrack_runtime::LocaleBinding,));
            defaults.extend(quote!(__locale: __inputs.__locale.clone(),));
            input_fields.extend(quote!(__locale: ::xrack_runtime::LocaleBinding,));
            if component.attribute("global") != Some("true") {
                methods.extend(quote! {
                    pub fn new_with_locale(context: ::xrack_runtime::LocaleContext) -> Self {
                        let component = Self::new();
                        ::xrack_runtime::Component::set_locale_context(&component, context);
                        component
                    }
                });
            }
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
                        write_cached_property(&field, &quote!(value), Some(&value_type)),
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
                    write_cached_property(&field, &quote!(value), Some(&value_type)),
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

        for key in self.record_combos.keys() {
            let items = combo_items_field(key);
            fields.extend(quote! {
                #items: ::xrack_runtime::Property<::xrack_runtime::ComboItems>,
            });
            defaults.extend(quote! {
                #items: ::core::default::Default::default(),
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
        for (&key, &element) in &self.elements {
            for name in ["enabled", "visible"] {
                let Some(method) = self.action_state_flag(element, name)? else {
                    continue;
                };
                let slot = control_field(key, name);
                let cases: Vec<_> = children(element, "xr.state_set")
                    .filter(|case| case.attribute("name") == Some(name))
                    .collect();
                let selected = self.state_selector(cases[0])?;
                let mut choices = TokenStream::new();
                for case in cases {
                    let case = StateCaseRef::from_operation(case)?;
                    let index = case.index;
                    let value = case
                        .value
                        .defining_operation()
                        .ok_or("missing action state value")?;
                    let value = self.visual_expr(value, &self.visuals)?;
                    choices.extend(quote!(#index => ::core::option::Option::Some(#value),));
                }
                let base = children(element, "xr.bind")
                    .find(|binding| binding.attribute("name") == Some(name))
                    .map(|binding| {
                        self.visual_value(binding, &self.visuals)
                            .map(|value| quote!(::core::option::Option::Some(#value)))
                    })
                    .transpose()?
                    .unwrap_or_else(|| quote!(::core::option::Option::None));
                fields.extend(quote! {
                    #slot: ::xrack_runtime::Property<::core::option::Option<::core::primitive::bool>>,
                });
                defaults.extend(quote!(#slot: ::core::default::Default::default(),));
                methods.extend(quote! {
                    fn #method(&self) -> ::core::option::Option<::core::primitive::bool> {
                        self.__state.#slot.get(|| match self.#selected() { #choices _ => #base })
                    }
                });
            }
        }
        for (&key, &action) in &self.actions {
            let identity = action_key_field(key);
            fields.extend(quote!(#identity: ::xrack_runtime::ActionKey,));
            defaults.extend(quote!(#identity: ::core::default::Default::default(),));
            let render = action_render_method(key);
            let read_checked = action_checked_method(key);
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
            let (checked, activation_checked, toggle) = if let Some((name, group, value)) =
                exclusive
            {
                let current = action_group_method(name, "current");
                let select =
                    self.set_control(group, &quote!(::std::string::String::from(#value)))?;
                (
                    quote!(__xr_checkable.unwrap_or_else(|| #checkable) && self.#current() == #value),
                    quote!(false),
                    select,
                )
            } else {
                if control_link(action, "checked").is_none() {
                    let field = control_field(key, "checked");
                    fields.extend(
                        quote!(#field: ::xrack_runtime::Property<::core::primitive::bool>,),
                    );
                    defaults.extend(quote!(#field: ::core::default::Default::default(),));
                }
                let checked = self.control_value(action, "checked", Type::Bool)?;
                let toggle = self.set_control(action, &quote!(__xr_next))?;
                (checked.clone(), checked, toggle)
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
                fn #read_checked(&self, __xr_checkable: ::core::option::Option<bool>) -> bool {
                    #checked
                }
                fn #render(&self, node: &mut ::xrack_runtime::Node) {
                    self.__state.#identity.present_with(
                        node, || #checkable, || #enabled, |checkable| self.#read_checked(::core::option::Option::Some(checkable)),
                    );
                    #labels
                }
                fn #activate(&self, available: impl FnOnce() -> bool, input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> bool {
                    let ::core::option::Option::Some(next) = ::xrack_runtime::prepare_action_activation(
                        || (#enabled) && available(), || #checkable, || #activation_checked,
                    ) else { return false; };
                    if input.is_some_and(|input| !input.is_current()) { return false; }
                    if let ::core::option::Option::Some(__xr_next) = next { #toggle }
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
            let (options, selection, handlers) = if dialog.attr("kind") == Ok("FileDialog") {
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
                let accepted =
                    self.typed_event(dialog, "accepted", &quote!(paths.into_snapshot()))?;
                let canceled = self.typed_event_values(dialog, "canceled", &[])?;
                (
                    quote!(::xrack_runtime::NativeDialogOptions::File(
                        ::xrack_runtime::FileDialogOptions::read_bindings(
                            || #mode, || #title, || #directory, || #file_name, || (#name_filters).snapshot(),
                        ),
                    )),
                    quote!(::core::option::Option::Some(&self.__state.#selection)),
                    quote! {
                        NativeDialogEvent::FilesSelected(paths) => { #accepted }
                        NativeDialogEvent::FileCanceled => { #canceled }
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
                    quote!(::xrack_runtime::NativeDialogOptions::Message(
                        ::xrack_runtime::MessageDialogOptions::read_bindings(
                            || #level, || #buttons, || #title, || #text, || #informative_text,
                        ),
                    )),
                    quote!(::core::option::Option::None),
                    quote! {
                        NativeDialogEvent::MessageAccepted => { #accepted }
                        NativeDialogEvent::MessageRejected => { #rejected }
                        NativeDialogEvent::MessageFinished(__xr_button) => { #finished }
                    },
                )
            };
            let failed = self.typed_event(dialog, "failed", &quote!(message))?;
            fields.extend(quote!(#slot: ::xrack_runtime::NativeDialogSlot,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #open(&self) {
                    self.__state.#slot.open_if(|| #enabled, || #options);
                }
                fn #register(&self) -> ::core::option::Option<::xrack_runtime::NativeDialogRegistration> {
                    let weak = self.as_weak();
                    self.__state.#slot.register(move |id, result| weak.upgrade().is_some_and(|ui| ui.#finish(id, result)))
                }
                fn #finish(&self, id: u64, result: ::xrack_runtime::NativeDialogResult) -> bool {
                    use ::xrack_runtime::NativeDialogEvent;
                    self.__state.#slot.complete(id, result, #selection, |event| match event {
                        #handlers
                        NativeDialogEvent::Failed(message) => { #failed }
                        _ => {}
                    })
                }
            });
        }
        for &(target, detailed) in &self.text_validation_targets {
            let (kind, ty) = if detailed {
                ("text_validity", quote!(::xrack_runtime::InputValidity))
            } else {
                ("text_accepts", quote!(::core::primitive::bool))
            };
            let getter = format_ident!("__xr_{}_{}", kind, rust_key(target));
            let element = self
                .elements
                .get(target)
                .ok_or("unknown text editor owner")?;
            let validation = self.text_validation_expression(*element, detailed)?;
            methods.extend(quote! {
                fn #getter(&self) -> #ty {
                    #validation
                }
            });
        }
        for &media in &self.media_players {
            let slot = media_slot(element_key(media)?);
            fields.extend(quote!(#slot: ::std::rc::Rc<::xrack_runtime::MediaSlot>,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
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
            let set_running = self.set_control(timer, &quote!(__xr_running))?;
            let stop = self.set_control(timer, &quote!(false))?;
            let interval = self.binding_value_or(timer, "interval", quote!(1000_i64))?;
            let repeat = self.binding_value_or(timer, "repeat", quote!(false))?;
            let immediate = self.binding_value_or(timer, "triggered-on-start", quote!(false))?;
            let triggered = self.typed_event_values(timer, "triggered", &[])?;
            fields.extend(quote!(#slot: ::xrack_runtime::TimerSlot,));
            defaults.extend(quote!(#slot: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #configure(&self) -> ::core::option::Option<::xrack_runtime::TimerConfig> {
                    ::xrack_runtime::TimerConfig::read(|| #running, || #interval, || #repeat, || #immediate)
                }
                fn #register(&self) -> ::core::option::Option<::xrack_runtime::TimerRegistration> {
                    let config = self.#configure()?;
                    let weak = self.as_weak();
                    Some(self.__state.#slot.register(config, move |tick| weak.upgrade().is_some_and(|ui| ui.#trigger(tick))))
                }
                fn #trigger(&self, tick: ::xrack_runtime::TimerTick) -> bool {
                    self.__state.#slot.deliver(&tick, || self.#configure(), || { #stop }, || { #triggered })
                }
                fn #command(&self, command: ::xrack_runtime::TimerCommand) {
                    self.__state.#slot.command(command, || #running, |__xr_running| { #set_running });
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
                fn #method(&self, __xr_open: bool, input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> bool {
                    ::xrack_runtime::set_overlay_open(
                        __xr_open, input, || #current, |__xr_open| { #set },
                        |__xr_open| { if __xr_open { #opened } else { #closed } },
                    )
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
            let id = xrack_mlir::host_element_id(key, table.attr("id")?);
            if let Some(id) = id {
                sort_reads.extend(quote!(#id => self.__state.#field.get(|| None),));
                sort_writes.extend(quote!(#id => self.#sort_table(sort, None),));
            }

            let widths = table_widths_field(key);
            let selection = table_selection_field(key);
            let order = table_order_field(key);
            let move_column = table_move_method(key);
            let row_order = table_row_order_field(key);
            let move_row = table_move_row_method(key);
            let heights = table_row_heights_field(key);
            let set_height = table_row_height_method(key);
            let rows = self.table_rows_model(table)?;
            let model = quote!(&#rows);
            let source = self.view_source_model(table)?;
            let current_row = self.control_value(table, "current-row", Type::Int)?;
            let set_current_row = self.set_control(table, &quote!(__xr_next))?;
            let current_row_changed =
                self.typed_event(table, "current-row-changed", &quote!(__xr_next))?;
            let selection_snapshot = table_selection_snapshot_method(key);
            let select_table = table_selection_method(key);
            let select_command = table_selection_command_method(key);
            let select_input = table_selection_input_method(key);
            let notify_selection = format_ident!("__table_notify_selection_{}", rust_key(key));
            let target = self.binding_value_or(
                table,
                "selection-target",
                quote!(::xrack_runtime::SelectionTarget::Rows),
            )?;
            let multiple = self.binding_value_or(table, "multi-select", quote!(false))?;
            let drag_select = self.binding_value_or(table, "drag-select", quote!(false))?;
            let receiver = quote!(self);
            let projection = TableProjection::for_input(&receiver);
            let available =
                self.element_allows_routed_input(table, key == root_key, &projection)?;
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
            let has_cell_handler = !cell_changed.is_empty();
            let cell_changed = has_cell_handler.then(|| {
                quote! {
                    let __xr_column = self.#selection_snapshot().current_column_index();
                    #cell_changed
                }
            });
            if let Some(id) = id {
                selection_reads.extend(quote!(#id => Some(self.#selection_snapshot()),));
                selection_writes.extend(quote!(#id => self.#select_table(action),));
            }
            let column_count = table_columns(table).count();
            methods.extend(quote! {
                fn #selection_snapshot(&self) -> ::xrack_runtime::TableSelectionSnapshot {
                    let model = #model;
                    self.__state.#selection.snapshot_with(
                        model, #column_count, || #current_row, || #target, || #multiple,
                        || self.__state.#order.get(::core::default::Default::default),
                    )
                }
                fn #select_table(&self, action: ::xrack_runtime::TableSelectionAction) -> ::core::primitive::bool {
                    self.#select_command(::xrack_runtime::TableSelectionCommand::Visual(action))
                }
                fn #select_command(&self, command: ::xrack_runtime::TableSelectionCommand) -> ::core::primitive::bool {
                    let Some(change) = self.__state.#selection.apply_command(
                        command, #column_count, || #rows, || #current_row,
                        || ::xrack_runtime::TableSelectionOptions {
                            target: #target,
                            multiple: #multiple,
                            order: self.__state.#order.get(::core::default::Default::default),
                            drag_select: false,
                        },
                    ) else { return false; };
                    self.#notify_selection(change, true)
                }
                fn #select_input(
                    &self, node: &::xrack_runtime::Node, action: ::xrack_runtime::ControlAction, input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::primitive::bool {
                    let ::core::option::Option::Some(change) = self.__state.#selection.apply_control(
                        node, input, || #available, || #rows,
                        || ::xrack_runtime::TableSelectionOptions {
                            target: #target,
                            multiple: #multiple,
                            order: self.__state.#order.get(::core::default::Default::default),
                            drag_select: matches!(action, ::xrack_runtime::ControlAction::TableSelection(
                                ::xrack_runtime::TableSelectionAction::DragRange { .. },
                            )) && (#drag_select),
                        },
                        || #current_row,
                    ) else { return false; };
                    self.#notify_selection(change, node.table_selection.is_some())
                }
                fn #notify_selection(
                    &self, change: ::xrack_runtime::TableSelectionChange, notify_cell: ::core::primitive::bool,
                ) -> ::core::primitive::bool {
                    use ::xrack_runtime::TableSelectionEvent;
                    change.notify(notify_cell && #has_cell_handler, |event| match event {
                        TableSelectionEvent::CurrentRowChanged(index) => {
                            let __xr_next = index as ::core::primitive::i64;
                            #set_current_row
                            #current_row_changed
                        }
                        TableSelectionEvent::CurrentCellChanged => { #cell_changed }
                        TableSelectionEvent::SelectionChanged(count) => { #selected }
                    })
                }
            });

            if let Some(field) = self.record_field_binding(table, "row-key-field")? {
                let field = field_access(field.split('.'));
                let keys = format_ident!("__list_keys_{}", rust_key(key));
                fields.extend(quote!(#keys: ::std::rc::Rc<::xrack_runtime::ListSelectionKeys>,));
                defaults.extend(quote!(#keys: ::core::default::Default::default(),));
                let lookup = list_key_index_method(key);
                let select_key = list_select_key_method(key);
                let selected_keys = list_selection_keys_method(key);
                let replace_keys = list_replace_selection_method(key);
                methods.extend(quote! {
                    fn #lookup(&self, key: ::core::primitive::i64) -> ::core::primitive::i64 {
                        let rows = #rows;
                        self.__state.#keys.find(rows.source(), key, |row| row #field)
                            .and_then(|index| rows.positions().visual_index(index))
                            .map_or(-1, |index| index as ::core::primitive::i64)
                    }
                    fn #select_key(&self, key: ::core::primitive::i64) {
                        let change = self.__state.#selection.try_apply_command_for(
                            #column_count,
                            || ::core::result::Result::Ok::<_, ::core::convert::Infallible>(#rows),
                            || ::core::result::Result::Ok(#current_row),
                            || ::core::result::Result::Ok(::xrack_runtime::TableSelectionOptions {
                                target: #target,
                                multiple: #multiple,
                                order: self.__state.#order.get(::core::default::Default::default),
                                drag_select: false,
                            }),
                            |rows| {
                                let row = self.__state.#keys.find(rows.source(), key, |row| row #field)?;
                                ::core::option::Option::Some(::xrack_runtime::TableSelectionCommand::Source {
                                    row, column: ::core::option::Option::None,
                                })
                            },
                        ).unwrap_or_else(|error| match error {});
                        if let ::core::option::Option::Some(change) = change {
                            self.#notify_selection(change, true);
                        }
                    }
                    fn #selected_keys(&self) -> ::xrack_runtime::VecModel<::core::primitive::i64> {
                        let rows = #rows;
                        let snapshot = self.__state.#selection.snapshot_with(
                            &rows, #column_count, || #current_row, || #target, || #multiple,
                            || self.__state.#order.get(::core::default::Default::default),
                        );
                        self.__state.#keys.keys_for_rows(rows.source(), snapshot.keys(), |row| row #field).into()
                    }
                    fn #replace_keys(&self, keys: ::xrack_runtime::VecModel<::core::primitive::i64>) {
                        let keys = keys.into_snapshot();
                        let change = self.__state.#selection.try_replace_list_keys(
                            || ::core::result::Result::Ok::<_, ::core::convert::Infallible>(#rows),
                            || ::core::result::Result::Ok(#current_row),
                            || ::core::result::Result::Ok((#multiple) && (#target) == ::xrack_runtime::SelectionTarget::Rows),
                            |rows| self.__state.#keys.resolve_keys(rows.source(), &keys, |row| row #field),
                        ).unwrap_or_else(|error| match error {});
                        if let ::core::option::Option::Some(change) = change {
                            self.#notify_selection(change, false);
                        }
                    }
                });
                if self.key_scroll_targets.contains(key) {
                    let locator = list_key_locator_method(key);
                    methods.extend(quote! {
                        fn #locator(&self) -> ::xrack_runtime::ListRowLocator {
                            let rows = ::std::rc::Rc::new(#rows);
                            ::xrack_runtime::ListRowLocator::new(rows.clone()).with_business_keys(
                                self.__state.#keys.clone(),
                                rows.source_model().clone(),
                                ::core::option::Option::Some(rows.positions().clone()),
                                |row| row #field,
                            )
                        }
                    });
                }
            }

            let auto_sort = table.attr("kind") == Ok("StandardTableView")
                && children(table, "xr.bind").any(|bind| bind.attr("name") == Ok("auto-sort"));
            let auto_sort_enabled = self.binding_value_or(table, "auto-sort", quote!(false))?;
            let filter_field = self.record_field_binding(table, "filter-field")?;
            let visible_field = self.record_field_binding(table, "row-visible-field")?;
            let projected = auto_sort || filter_field.is_some() || visible_field.is_some();
            let standard = table.attr("kind") == Ok("StandardTableView");
            if standard || visible_field.is_some() {
                let binding = if standard {
                    self.standard_table(table)?.model
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
                        self.standard_table(table)?.columns.iter()
                            .enumerate()
                            .map(|(index, definition)| {
                                let field = field_access(definition.field.split('.'));
                                quote!(#index => ::xrack_runtime::TableField::compare_cells(&left #field, &right #field),)
                            })
                            .collect::<TokenStream>()
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
            let move_guard = auto_sort.then(|| quote! {
                if #auto_sort_enabled && self.__state.#field.get(|| None).is_some() { return false; }
            });
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
                fn #sort_table(&self, sort: ::core::option::Option<::xrack_runtime::TableSort>, input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> ::core::primitive::bool {
                    let Some(__xr_cursor) = self.__state.#row_order.sort_rows(
                        &self.__state.#field, sort, input,
                        |column| match column { #sortable _ => false },
                        || #auto_sort_enabled, || #rows, || #current_row,
                    ) else { return false; };
                    if let Some(__xr_next) = __xr_cursor.map(|index| index as ::core::primitive::i64) {
                        #set_current_row
                        #current_row_changed
                    }
                    if let Some(__xr_sort) = sort {
                        if __xr_sort.descending { #descending } else { #ascending }
                    }
                    true
                }
                fn #set_height(&self, row: ::core::primitive::i64, height: ::xrack_runtime::Length) -> ::core::primitive::bool {
                    self.__state.#heights.set_row(row, height, || #rows)
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
                fn #move_row(&self, from: ::core::primitive::i64, to: ::core::primitive::i64, input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> ::core::primitive::bool {
                    #move_guard
                    let Some((__xr_row, __xr_cursor)) = self.__state.#row_order.move_visible_row(
                        from, to, input, || #rows, || #current_row,
                    ) else { return false; };
                    if let Some(__xr_next) = __xr_cursor.map(|index| index as ::core::primitive::i64) {
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
            let element = self.elements[key];
            let save = split_save_method(key);
            let restore = split_restore_method(key);
            let panes = xrack_mlir::split_pane_names(element)?;
            let resized = self.typed_event_values(element, "resized", &[])?;
            let fixed_sizes = xrack_mlir::split_fill_slot(element)?.is_some();
            fields.extend(quote!(#field: ::xrack_runtime::SplitState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #save(&self) -> ::std::string::String {
                    self.__state.#field.save(&[#(#panes),*], #fixed_sizes)
                }
                fn #restore(&self, state: &::core::primitive::str) -> ::core::primitive::bool {
                    let Some(changed) = self.__state.#field.restore(&[#(#panes),*], #fixed_sizes, state) else { return false; };
                    if changed { #resized }
                    true
                }
            });
            if let Some(id) = xrack_mlir::host_element_id(key, element.attr("id")?) {
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
        for (&key, definition) in &self.trees {
            let field = tree_state_field(key);
            let element = self.elements[key];
            let repeater = repeat_field(self.visuals.repeats[element_key(definition.row)?].index());
            let model = self.expr(
                definition
                    .repeat
                    .model
                    .defining_operation()
                    .ok_or("missing tree model definition")?,
            )?;
            let tree = self.tree_projection(
                element,
                &quote!(self.__state.__visuals.#repeater.resolve_model(|| #model)),
            )?;
            let snapshot = tree_snapshot_method(key);
            let project = tree_projection_method(key);
            let record = ident(&definition.record);
            let command = tree_command_method(key);
            let select = tree_selection_method(key);
            let selection_info = tree_selection_info_method(key);
            let selection_keys = tree_selection_keys_method(key);
            let replace_selection = tree_replace_selection_method(key);
            let input = tree_input_method(key);
            let notify = format_ident!("__tree_notify_{}", rust_key(key));
            let current = self.control_value(element, "current-key", Type::Int)?;
            let receiver = quote!(self);
            let projection = TableProjection::for_input(&receiver);
            let available =
                self.element_allows_routed_input(element, key == root_key, &projection)?;
            let multiple = self.binding_value_or(element, "multi-select", quote!(false))?;
            let set = self.set_control(element, &quote!(key))?;
            let changed = self.typed_event(element, "changed", &quote!(key))?;
            let expanded = self.typed_event(element, "expanded", &quote!(__xr_event_key))?;
            let collapsed = self.typed_event(element, "collapsed", &quote!(__xr_event_key))?;
            let selected = self.typed_event(
                element,
                "selection-changed",
                &quote!(__xr_count as ::core::primitive::i64),
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::TreeState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #project(&self) -> (
                    ::std::sync::Arc<::xrack_runtime::TreeSnapshot>,
                    ::std::rc::Rc<::xrack_runtime::VecModel<#record>>,
                ) {
                    #tree
                }
                fn #snapshot(&self) -> ::std::sync::Arc<::xrack_runtime::TreeSnapshot> {
                    self.#project().0
                }
                fn #selection_info(&self, query: ::core::option::Option<::xrack_runtime::TreeSelectionLookup>)
                    -> (::core::primitive::i64, ::core::primitive::bool) {
                    let tree = self.#snapshot();
                    self.__state.#field.selection_info(&tree, #multiple, #current, query)
                }
                fn #selection_keys(&self) -> ::xrack_runtime::VecModel<::core::primitive::i64> {
                    let tree = self.#snapshot();
                    let current = (!#multiple).then(|| #current);
                    self.__state.#field.selected_keys(&tree, current).into()
                }
                fn #replace_selection(&self, keys: ::xrack_runtime::VecModel<::core::primitive::i64>) {
                    if let ::core::option::Option::Some(change) = self.__state.#field.replace_selection(
                        &keys.into_snapshot(), || self.#snapshot(), || #multiple,
                    ) {
                        self.#notify(::xrack_runtime::TreeControlChange::Selection(change));
                    }
                }
                fn #select(&self, command: ::xrack_runtime::TreeSelectionCommand) -> bool {
                    let ::core::option::Option::Some(change) = self.__state.#field.apply_selection(
                        command, || self.#snapshot(), || #multiple, || #current,
                    ) else { return false; };
                    self.#notify(::xrack_runtime::TreeControlChange::Selection(change))
                }
                fn #input(&self, node: &::xrack_runtime::Node, input: ::xrack_runtime::InputRoute<'_>) -> bool {
                    match self.__state.#field.apply_control(
                        node, input, || #available, || #multiple, || self.#snapshot(), || #current,
                    ) {
                        ::core::option::Option::Some(change) => self.#notify(change),
                        ::core::option::Option::None => false,
                    }
                }
                fn #notify(&self, change: ::xrack_runtime::TreeControlChange) -> bool {
                    change.notify(|event| match event {
                        ::xrack_runtime::TreeControlEvent::CurrentKeyChanged(key) => { #set #changed }
                        ::xrack_runtime::TreeControlEvent::SelectionChanged(__xr_count) => { #selected }
                        ::xrack_runtime::TreeControlEvent::Expanded(__xr_event_key) => { #expanded }
                        ::xrack_runtime::TreeControlEvent::Collapsed(__xr_event_key) => { #collapsed }
                    })
                }
                fn #command(&self, command: ::xrack_runtime::TreeCommand) -> bool {
                    let Some((change, current)) = self.__state.#field.apply_command(
                        command, || self.#snapshot(), || #current,
                    ) else { return false; };
                    self.#notify(::xrack_runtime::TreeControlChange::Expansion { change, current })
                }
            });
            if let Some(id) = xrack_mlir::host_element_id(key, element.attr("id")?) {
                tree_reads.extend(quote!(#id => Some(self.#snapshot()),));
                tree_writes.extend(quote!(#id => self.#command(command),));
            }
        }
        if !tree_reads.is_empty() {
            methods.extend(quote! {
                pub fn tree_snapshot(&self, id: &str) -> ::core::option::Option<::std::sync::Arc<::xrack_runtime::TreeSnapshot>> {
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
            let initial = children(view, "xr.bind")
                .find(|binding| binding.attr("name") == Ok("initial-page"))
                .map(|binding| self.visual_value(binding, &self.visuals))
                .transpose()?
                .unwrap_or_else(|| quote!(::std::string::String::new()));
            let changed = self.typed_event_values(
                view,
                "changed",
                &[quote!(__xr_current.to_owned()), quote!(__xr_depth)],
            )?;
            fields.extend(quote!(#field: ::xrack_runtime::StackState,));
            defaults.extend(quote!(#field: ::core::default::Default::default(),));
            methods.extend(quote! {
                fn #snapshot(&self) -> ::std::sync::Arc<::xrack_runtime::StackSnapshot> {
                    self.__state.#field.snapshot(&[#(#routes),*], || #initial)
                }
                fn #navigate(&self, expected: ::core::option::Option<::core::primitive::u64>, action: ::xrack_runtime::StackNavigation<'_>, input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> bool {
                    self.__state.#field.navigate_guarded(
                        &[#(#routes),*], || #initial, expected, action,
                        |__xr_route, __xr_destination, __xr_removing| {
                            if input.is_some_and(|input| !input.is_current()) { return false; }
                            let allowed = match __xr_route { #guards _ => true };
                            allowed && input.is_none_or(|input| input.is_current())
                        },
                        |__xr_change| __xr_change.notify(|event| match event {
                            ::xrack_runtime::StackEvent::Deactivated(__xr_route) => match __xr_route { #deactivated _ => {} },
                            ::xrack_runtime::StackEvent::Removed(__xr_route) => match __xr_route { #removed _ => {} },
                            ::xrack_runtime::StackEvent::Activated(__xr_route) => match __xr_route { #activated _ => {} },
                            ::xrack_runtime::StackEvent::Changed { current: __xr_current, depth: __xr_depth } => { #changed }
                        }),
                    )
                }
            });
            if let Some(id) = xrack_mlir::host_element_id(key, view.attr("id")?) {
                stack_reads.extend(quote!(#id => Some(self.#snapshot()),));
                stack_writes.extend(quote!(#id => self.#navigate(None, action, None),));
            }
        }
        if !stack_reads.is_empty() {
            methods.extend(quote! {
                pub fn stack_snapshot(&self, id: &str) -> ::core::option::Option<::std::sync::Arc<::xrack_runtime::StackSnapshot>> {
                    match id { #stack_reads _ => None }
                }
                pub fn navigate_stack(&mut self, id: &str, action: ::xrack_runtime::StackNavigation<'_>) -> bool {
                    match id { #stack_writes _ => false }
                }
            });
        }
        let globals =
            (!self.globals.is_empty()).then(|| quote!(__globals: __inputs.__globals.clone(),));
        let mut instance_routes = TokenStream::new();
        let locale = self
            .with_locale
            .then(|| quote!(__locale: __inputs.__locale.clone(),));
        for (&key, &instance) in &self.instances {
            let field = instance_field(key);
            let component_name = instance.attr("component")?;
            let component = ident(component_name);
            let child_inputs = format_ident!("__Inputs_{}", rust_name(component_name));
            let bindings = self.instance_inputs(instance, None, None)?;
            fields.extend(quote!(#field: ::xrack_runtime::LoaderChild<#component>,));
            defaults.extend(quote! {
                #field: {
                    let __xr_child_inputs = #child_inputs {
                        #bindings
                        #locale
                        #globals
                        ..::core::default::Default::default()
                    };
                    ::xrack_runtime::LoaderChild::new(move || #component::__new(__xr_child_inputs))
                },
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
            let params = callback.parameters()?;
            let types: Vec<_> = params.iter().map(|p| host_ty(&p.ty)).collect();
            let args: Vec<_> = params.iter().map(|p| parameter_ident(&p.name)).collect();
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
                        let callback = ::xrack_runtime::EffectfulCallback::new(callback);
                        move |#(#arguments),*| {
                            callback.try_call(|handler| handler(#(#arguments),*))
                                .unwrap_or_else(|error| ::core::panic!("{}", error))
                        }
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
                let forwarded = self
                    .analysis
                    .callback_forwards
                    .get(&[(owner, name)][..])
                    .map(|forward| self.forwarded_callback(forward))
                    .transpose()?
                    .flatten();
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
                let command = secondary_window_member(key, "command");
                let visible = self.control_value(window, "visible", Type::Bool)?;
                let set = self.set_control_property(window, "visible", &quote!(visible))?;
                fields.extend(quote!(#slot: ::xrack_runtime::SecondaryWindowSlot,));
                defaults.extend(quote!(#slot: ::core::default::Default::default(),));
                methods.extend(quote! {
                    fn #visibility(&self, visible: ::core::primitive::bool) { #set }

                    fn #command(&self, command: ::xrack_runtime::SecondaryWindowCommand) {
                        command.apply(
                            || #visible,
                            |visible| self.#visibility(visible),
                            || self.__state.#child.__state.__window.request_close(),
                        );
                    }

                    fn #register(&self) -> ::core::option::Option<::xrack_runtime::SecondaryWindowRegistration> {
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
                fn secondary_windows(&self) -> ::core::option::Option<::std::rc::Rc<[::xrack_runtime::SecondaryWindowRegistration]>> {
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
        let exclusive_button_paths: Vec<_> = groups
            .values()
            .flatten()
            .filter(|(_, member)| {
                member
                    .attribute("kind")
                    .is_some_and(xrack_syntax::builtins::push_button)
                    && radio_group_ref(*member).is_some()
            })
            .map(|(path, _)| quote!([#(#path),*]))
            .collect();
        let exclusive_button_dispatch = if exclusive_button_paths.is_empty() {
            TokenStream::new()
        } else {
            quote! {
                RoutedEvent::Dispatch("clicked") if matches!(path, #(#exclusive_button_paths)|*) =>
                    scene.at_path(path).is_some_and(|node| {
                        self.__control_path(path, node, ::xrack_runtime::ControlAction::ActivateRadioOption, input)
                    }),
            }
        };
        self.declare_check_groups(&groups, &mut fields, &mut defaults, &mut methods)?;
        let mut render = TokenStream::new();
        let node = self.render(root, &self.visuals, &mut render, &mut 0)?;
        self.bind_button_groups(&groups, &node, &mut render)?;
        if self.with_fonts {
            render.extend(quote! {
                let mut #node = #node;
                #node.set_embedded_fonts(&__XR_FONTS);
            });
        }
        let mut dispatch = TokenStream::new();
        let mut path_dispatch = TokenStream::new();
        let mut dispatch_handlers = TokenStream::new();
        self.dispatch_arms(
            root,
            &mut Vec::new(),
            &mut dispatch,
            &mut path_dispatch,
            &mut dispatch_handlers,
        )?;
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
        let mut input_availability = TokenStream::new();
        let receiver = quote!(self);
        let projection = TableProjection::for_input(&receiver);
        self.pointer_input_arms(
            root,
            &mut Vec::new(),
            &mut wheels,
            &mut pointers,
            &mut input_availability,
            &projection,
        )?;
        let mut drag = TokenStream::new();
        self.drag_arms(root, &mut Vec::new(), &mut drag)?;
        for (&key, &view) in &self.elements {
            if !has_virtual_rows(view) {
                continue;
            }
            if matches!(
                view.attr("kind"),
                Ok("ListView" | "GridView" | "StandardListView")
            ) {
                methods.extend(self.list_selection_methods(view)?);
            }
            if self.view_count_targets.contains(key) {
                let count = view_count_method(key);
                let count_body = self.view_count_body(view)?;
                methods.extend(quote! {
                    fn #count(&self, source: ::core::primitive::bool) -> ::core::primitive::i64 {
                        #count_body
                    }
                });
            }
            if self.list_selection_targets.contains(key) {
                let method = list_selected_index_method(key);
                let index = self.list_selection_index(view)?;
                let count = view_count_method(key);
                let resolve = if xrack_resources::visual::keyed_list(view) {
                    index
                } else {
                    quote!({
                        let index = #index;
                        if index < 0 { return -1; }
                        ::xrack_runtime::ListMetrics::effective_index(index, self.#count(false) as ::core::primitive::usize)
                    })
                };
                methods.extend(quote! {
                    fn #method(&self) -> ::core::primitive::i64 { #resolve }
                });
            }
            let lookup = view_index_method(key);
            let index_body = self.view_index_body(view)?;
            if matches!(
                view.attr("kind"),
                Ok("ListView" | "GridView" | "StandardListView")
            ) && let Some(field) = self.record_field_binding(view, "row-key-field")?
            {
                let field = field_access(field.split('.'));
                let keys = format_ident!("__list_keys_{}", rust_key(key));
                let current_key = self.control_value(view, "current-key", Type::Int)?;
                let (model, source, visible, original) = if has_list_visibility(view) {
                    let projection = visible_list_model(key);
                    (
                        quote!(self.#projection()),
                        quote!(model.source()),
                        quote!(model.positions().visual_index(index)),
                        quote!(model.positions().source_index(index)),
                    )
                } else {
                    (
                        self.view_source_model(view)?,
                        quote!(&*model),
                        quote!(::core::option::Option::Some(index)),
                        quote!(::core::option::Option::Some(index)),
                    )
                };
                let index = list_selection_index_method(key);
                let key_at = list_selection_key_method(key);
                fields.extend(quote!(#keys: ::std::rc::Rc<::xrack_runtime::ListSelectionKeys>,));
                defaults.extend(quote!(#keys: ::core::default::Default::default(),));
                let selected_keys = list_selection_keys_method(key);
                let replace_selection = list_replace_selection_method(key);
                let selection = list_selection_field(key);
                let selection_rows = self.list_selection_rows(view)?;
                let multiple = self.binding_value_or(view, "multi-select", quote!(false))?;
                let sample_rows = quote! {
                    let rows = ::std::cell::OnceCell::new();
                    let __xr_model = || rows.get_or_init(|| #selection_rows);
                    let read = || ::core::result::Result::Ok::<_, ::core::convert::Infallible>(__xr_model().clone());
                    let current = || {
                        let rows = __xr_model();
                        let key = #current_key;
                        ::core::result::Result::Ok(self.__state.#keys.find(rows.source(), key, |row| row #field)
                            .and_then(|index| rows.positions().visual_index(index))
                            .map_or(-1, |index| index as ::core::primitive::i64))
                    };
                };
                let (selection_ref, replace) = if list_multi_selection(view) {
                    let notify = self.list_selection_notifier(view)?;
                    (
                        quote!(::core::option::Option::Some(&self.__state.#selection)),
                        quote! {
                            #sample_rows
                            let keys = keys.into_snapshot();
                            let change = self.__state.#selection.try_replace_list_keys(
                                read, current, || ::core::result::Result::Ok(#multiple),
                                |rows| self.__state.#keys.resolve_keys(rows.source(), &keys, |row| row #field),
                            ).unwrap_or_else(|error| match error {});
                            if let ::core::option::Option::Some(change) = change {
                                change.notify(false, #notify);
                            }
                        },
                    )
                } else {
                    (quote!(::core::option::Option::None), quote!(let _ = keys;))
                };
                methods.extend(quote! {
                    fn #selected_keys(&self) -> ::xrack_runtime::VecModel<::core::primitive::i64> {
                        #sample_rows
                        ::xrack_runtime::TableSelection::try_list_selected_keys(
                            #selection_ref, read, current, || ::core::result::Result::Ok(#multiple),
                            |rows, selection| self.__state.#keys.keys_for_rows(rows.source(), selection, |row| row #field),
                        ).unwrap_or_else(|error| match error {}).into()
                    }
                    fn #replace_selection(&self, keys: ::xrack_runtime::VecModel<::core::primitive::i64>) {
                        #replace
                    }
                });
                let find = list_key_index_method(key);
                let select = list_select_key_method(key);
                let set = self.set_control_property(view, "current-key", &quote!(key))?;
                let changed = self.typed_event(view, "changed", &quote!(index))?;
                let select_body = if list_multi_selection(view) {
                    let method = list_selection_command_method(key);
                    quote!(self.#method(::xrack_runtime::ListSelectionCommand::Key(key));)
                } else {
                    quote! {
                        let index = self.#find(key);
                        if index >= 0 && key != (#current_key) {
                            #set
                            #changed
                        }
                    }
                };
                if self.key_scroll_targets.contains(key) {
                    let locator = list_key_locator_method(key);
                    let (model, source, order) = if has_list_visibility(view) {
                        let projection = visible_list_model(key);
                        (
                            quote!(::std::rc::Rc::new(self.#projection())),
                            quote!(model.source_model().clone()),
                            quote!(Some(model.positions().clone())),
                        )
                    } else {
                        (model.clone(), quote!(model.clone()), quote!(None))
                    };
                    methods.extend(quote! {
                        fn #locator(&self) -> ::xrack_runtime::ListRowLocator {
                            let model = #model;
                            ::xrack_runtime::ListRowLocator::new(model.clone()).with_business_keys(
                                self.__state.#keys.clone(), #source, #order, |row| row #field,
                            )
                        }
                    });
                }
                methods.extend(quote! {
                    fn #find(&self, key: ::core::primitive::i64) -> ::core::primitive::i64 {
                        if key < 0 { return -1; }
                        let model = #model;
                        self.__state.#keys.find(#source, key, |row| row #field)
                            .and_then(|index| #visible)
                            .map_or(-1, |index| index as ::core::primitive::i64)
                    }
                    fn #select(&self, key: ::core::primitive::i64) {
                        #select_body
                    }
                    fn #index(&self) -> ::core::primitive::i64 {
                        self.#find(#current_key)
                    }
                    fn #key_at(&self, index: ::core::primitive::usize) -> ::core::option::Option<::core::primitive::i64> {
                        let model = #model;
                        let index = (#original)?;
                        self.__state.#keys.key_at(#source, index, |row| row #field)
                    }
                });
            }
            let select = source_selection_method(key);
            let selection_body = self.source_selection_body(view)?;
            methods.extend(quote! {
                fn #lookup(
                    &self, index: ::core::primitive::usize, direction: ::xrack_runtime::ViewIndex,
                ) -> ::core::option::Option<::core::primitive::usize> {
                    #index_body
                }

                fn #select(&self, source: ::core::primitive::i64, column: ::core::option::Option<::core::primitive::i64>) {
                    let Ok(source) = <::core::primitive::usize as ::core::convert::TryFrom<_>>::try_from(source)
                    else { return; };
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
                    __xr_scene: ::core::option::Option<&::xrack_runtime::Node>,
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
            paste_stamps,
            replays,
            fields: history_fields,
            defaults: history_defaults,
            methods: history_methods,
        } = clipboard;
        fields.extend(history_fields);
        defaults.extend(history_defaults);
        methods.extend(history_methods);
        let record_editor_visibility = matches!(
            root.attr("kind"),
            Ok("TableView" | "StandardTableView" | "TreeView")
        )
        .then(|| quote!(input.record_root_visibility_override(&self.__state.__binding_lifetime);));
        let mut delegate_events = TokenStream::new();
        let mut delegates = TokenStream::new();
        self.delegate_event_arms(root, &mut Vec::new(), &mut delegate_events, &mut delegates)?;
        let set_locale_context = self.with_locale.then(|| {
            quote! {
                fn set_locale_context(&self, context: ::xrack_runtime::LocaleContext) {
                    self.__state.__locale.set_context(context);
                }
            }
        });
        let global_handle = (component.attribute("global") == Some("true")).then(|| {
            quote! {
                __global_context: ::core::option::Option<::std::rc::Rc<__XrGlobals>>,
            }
        });
        let init_handle = global_handle
            .as_ref()
            .map(|_| quote!(__global_context: None,));
        let weak_handle = global_handle
            .as_ref()
            .map(|_| quote!(__global_context: __state.__globals.upgrade(),));
        let constructor = (component.attribute("global") != Some("true")).then(|| {
            quote! {
                pub fn new() -> Self {
                    Self::__new(::core::default::Default::default())
                }
            }
        });
        let default = constructor.as_ref().map(|_| {
            quote! {
                impl ::core::default::Default for #name {
                    fn default() -> Self { Self::new() }
                }
            }
        });
        Ok(quote! {
            #visual_declarations

            #[allow(non_camel_case_types, dead_code)]
            #visibility struct #name {
                __state: ::std::rc::Rc<#state>,
                #global_handle
            }

            #[allow(non_camel_case_types, non_snake_case)]
            struct #state { #fields }

            #[allow(non_camel_case_types)]
            #[derive(::core::default::Default)]
            struct #inputs { #input_fields }

            #default

            // Expressions that can trap remain lazy even when a pass exposes
            // their constant operands to rustc.
            #[allow(arithmetic_overflow, unconditional_panic)]
            #[allow(dead_code, unused_mut, unused_variables, unused_parens, clippy::clone_on_copy)]
            impl #name {
                #constructor

                pub fn as_weak(&self) -> ::xrack_runtime::WeakComponent<Self> {
                    ::xrack_runtime::WeakComponent::new(
                        &self.__state, &self.__state.__binding_lifetime, |__state| Self { #weak_handle __state }
                    )
                }

                fn __new(mut __inputs: #inputs) -> Self {
                    let __component = Self {
                        #init_handle
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

                fn __copy_table_path(
                    &self,
                    path: &[::core::primitive::usize],
                    node: &::xrack_runtime::Node,
                    columns: &[::core::primitive::usize],
                    input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::option::Option<::std::string::String> {
                    match path { #copies _ => ::core::option::Option::None }
                }

                fn __next_table_edit_cell(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    navigation: ::xrack_runtime::TableEditNavigation<'_>, input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::option::Option<::xrack_runtime::TableEditPosition> {
                    let columns = navigation.columns;
                    match path { #edit_navigation _ => ::core::option::Option::None }
                }

                fn __table_paste_stamp(
                    &self, path: &[::core::primitive::usize],
                    node: &::xrack_runtime::Node,
                ) -> ::core::option::Option<(::core::primitive::u64, ::core::primitive::u64)> {
                    match path { #paste_stamps _ => ::core::option::Option::None }
                }

                fn __apply_table_paste(
                    &self, path: &[::core::primitive::usize], node: &::xrack_runtime::Node,
                    paste: ::xrack_runtime::TablePasteInput<'_>, route: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::primitive::bool {
                    match path { #pastes _ => false }
                }

                fn __replay_table_edit(
                    &self, path: &[::core::primitive::usize],
                    model: ::core::primitive::u64, redo: ::core::primitive::bool,
                    route: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::primitive::bool {
                    match path { #replays _ => false }
                }

                fn __with_delegate<R: ::core::default::Default>(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    follow_row: ::core::primitive::bool,
                    input: ::core::option::Option<::xrack_runtime::InputRoute<'_>>,
                    visit: impl FnOnce(&mut dyn ::xrack_runtime::Component, &::xrack_runtime::Node, &[::core::primitive::usize], ::core::option::Option<::xrack_runtime::InputRoute<'_>>) -> R,
                ) -> ::core::option::Option<R> {
                    match path { #delegates _ => None }
                }

                fn __dispatch_delegate_event(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: &::xrack_runtime::RoutedEvent<'_>,
                    input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::option::Option<::core::primitive::bool> {
                    match path {
                        #delegate_events
                        _ => None,
                    }
                }

                fn __input_path_available(
                    &self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::option::Option<::core::primitive::bool> {
                    let mut child_visibility_overridden = false;
                    if input.checks_availability() {
                        let mut node = scene;
                        for depth in 0..=path.len() {
                            let root_visibility_overridden = depth == 0 && input.root_visibility_overridden();
                            if !input.available(|| match &path[..depth] { #input_availability _ => true }) {
                                return ::core::option::Option::None;
                            }
                            if let ::core::option::Option::Some(&index) = path.get(depth) {
                                node = node.child_at(index)?;
                                if node.kind == "ListRow" {
                                    // The row route validates its own delegate and model.
                                    break;
                                }
                            }
                        }
                    }
                    ::core::option::Option::Some(child_visibility_overridden)
                }

                fn __route_input(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::primitive::bool {
                    let ::core::option::Option::Some(child_visibility_overridden) = self.__input_path_available(scene, path, input) else {
                        return false;
                    };
                    #record_editor_visibility
                    let event = input.event();
                    let follow_row = event.follows_row(scene, path);
                    let child_input = input.with_root_visibility_override(child_visibility_overridden);
                    if let ::core::option::Option::Some(result) = self.__with_delegate(scene, path, follow_row, Some(child_input), |ui, child, rest, input| {
                        ui.route_input(child, rest, input.expect("routed input"))
                    }) {
                        return result;
                    }
                    if !input.is_current() { return false; }
                    use ::xrack_runtime::{EventResult, RoutedEvent};
                    match event {
                        #exclusive_button_dispatch
                        RoutedEvent::Dispatch(name) => self.__dispatch_delegate_event(scene, path, event, input)
                            .unwrap_or_else(|| self.__dispatch_path(path, name, input)),
                        RoutedEvent::Edit(text) => scene.at_path(path).is_some_and(|node| self.__edit_path(path, node, (*text).into(), input)),
                        RoutedEvent::PasteTable(paste) => scene.at_path(path).is_some_and(|node| {
                            self.__apply_table_paste(path, node, *paste, input)
                        }),
                        RoutedEvent::ReplayTable { redo } => scene.table_edit_history_model(path).is_some_and(|model| {
                            self.__replay_table_edit(path, model, *redo, input)
                        }),
                        RoutedEvent::Control(action) => self.__dispatch_delegate_event(scene, path, event, input)
                            .unwrap_or_else(|| scene.at_path(path).is_some_and(|node| self.__control_path(path, node, *action, input))),
                        RoutedEvent::Range(action) => self.__dispatch_delegate_event(scene, path, event, input)
                            .unwrap_or_else(|| scene.range_target(path).is_some_and(|node| self.__range_path(path, node, *action, input))),
                        RoutedEvent::ResizeSplit(_) | RoutedEvent::ResizeSplitSizes(_) | RoutedEvent::CommitCell { .. } => {
                            self.__dispatch_delegate_event(scene, path, event, input).unwrap_or(false)
                        }
                        RoutedEvent::Focus(event) => self.__focus_path(path, *event),
                        RoutedEvent::Key(phase, event) => self.__key_path(path, *phase, event) == EventResult::Accept,
                        RoutedEvent::Wheel(event) => self.__wheel_path(path, event) == EventResult::Accept,
                        RoutedEvent::Pointer(event) => self.__pointer_path(path, event),
                        RoutedEvent::Drag(event) => self.__drag_path(path, event),
                        RoutedEvent::QueryTable(_) => false,
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
                    input: ::xrack_runtime::InputRoute<'_>,
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
                    input: ::xrack_runtime::InputRoute<'_>,
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

                fn __range_path(&self, path: &[::core::primitive::usize], node: &::xrack_runtime::Node, action: ::xrack_runtime::RangeAction, input: ::xrack_runtime::InputRoute<'_>) -> ::core::primitive::bool {
                    match path {
                        #ranges
                        _ => false,
                    }
                }

                fn __edit_path(&self, path: &[::core::primitive::usize], node: &::xrack_runtime::Node, text: ::std::string::String, input: ::xrack_runtime::InputRoute<'_>) -> ::core::primitive::bool {
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

                fn __dispatch_handler(
                    &self,
                    path: &[::core::primitive::usize],
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    match (path, event) {
                        #dispatch_handlers
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
                    self.__render_shared().list_rows_at(path, rows)
                }

                fn copy_table_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    columns: &[::core::primitive::usize],
                ) -> ::core::option::Option<::std::string::String> {
                    ::xrack_runtime::TableQuery::Copy(columns).run(self, scene, path)?.text()
                }

                fn route_table_query(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    input: ::xrack_runtime::InputRoute<'_>,
                ) -> ::core::option::Option<::xrack_runtime::TableQueryResult> {
                    use ::xrack_runtime::{RoutedEvent, TableQuery, TableQueryResult};
                    let RoutedEvent::QueryTable(query) = *input.event() else { return ::core::option::Option::None; };
                    let overridden = self.__input_path_available(scene, path, input)?;
                    let child_input = input.with_root_visibility_override(overridden);
                    if let ::core::option::Option::Some(result) = self.__with_delegate(scene, path, false, Some(child_input), |ui, child, rest, input| {
                        ui.route_table_query(child, rest, input.expect("routed table query"))
                    }) {
                        return result;
                    }
                    match query {
                        TableQuery::Copy(columns) => self.__copy_table_path(path, scene.at_path(path)?, columns, input)
                            .map(TableQueryResult::Text),
                        TableQuery::NextEdit(navigation) => self.__next_table_edit_cell(scene, path, navigation, input)
                            .map(TableQueryResult::Position),
                    }
                }

                fn next_table_edit_cell(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    origin: ::xrack_runtime::TableEditPosition, columns: &[::core::primitive::usize],
                    backward: ::core::primitive::bool,
                ) -> ::core::option::Option<::xrack_runtime::TableEditPosition> {
                    ::xrack_runtime::TableQuery::NextEdit(::xrack_runtime::TableEditNavigation { origin, columns, backward })
                        .run(self, scene, path)?.position()
                }

                fn prepare_table_paste(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    row: ::core::primitive::usize, columns: ::std::vec::Vec<::core::primitive::usize>,
                ) -> ::core::option::Option<::xrack_runtime::TablePaste> {
                    ::xrack_runtime::prepare_table_paste(self, || self.as_weak(), scene, path, row, columns)
                }

                fn table_paste_stamp(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                ) -> ::core::option::Option<(::core::primitive::u64, ::core::primitive::u64)> {
                    let node = scene.interactive_at_path(path)?;
                    if !node.boolean("pasteable", false) {
                        return ::core::option::Option::None;
                    }
                    self.__with_delegate(scene, path, false, None, |ui, child, rest, _| ui.table_paste_stamp(child, rest))
                        .unwrap_or_else(|| self.__table_paste_stamp(path, node))
                }

                fn replay_table_paste_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool,
                ) -> ::core::primitive::bool {
                    self.replay_table_edit_at(scene, path, redo)
                }

                fn replay_table_edit_at(
                    &self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool,
                ) -> ::core::primitive::bool {
                    let mut ui = Self { __state: self.__state.clone(), #init_handle };
                    ::xrack_runtime::RoutedEvent::ReplayTable { redo }.deliver_input(&mut ui, scene, path)
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
                    ::xrack_runtime::RoutedEvent::Range(action).deliver_input(self, scene, path)
                }

                fn control_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    action: ::xrack_runtime::ControlAction,
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Control(action).deliver_input(self, scene, path)
                }

                fn route_input(&mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], input: ::xrack_runtime::InputRoute<'_>) -> ::core::primitive::bool {
                    self.__route_input(scene, path, input)
                }

                fn focus_event_at(&mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], event: ::xrack_runtime::FocusEvent) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Focus(event).deliver_input(self, scene, path)
                }

                fn key_event_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    phase: ::xrack_runtime::KeyPhase,
                    event: &::xrack_runtime::KeyEvent,
                ) -> ::xrack_runtime::EventResult {
                    ::xrack_runtime::RoutedEvent::Key(phase, event).deliver_input(self, scene, path).into()
                }

                fn wheel_event_at(
                    &mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    event: &::xrack_runtime::WheelEvent,
                ) -> ::xrack_runtime::EventResult {
                    ::xrack_runtime::RoutedEvent::Wheel(event).deliver_input(self, scene, path).into()
                }

                fn pointer_event_at(
                    &mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize],
                    event: &::xrack_runtime::PointerEvent,
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Pointer(event).deliver_input(self, scene, path)
                }

                fn drag_event_at(&mut self, scene: &::xrack_runtime::Node, path: &[::core::primitive::usize], event: &::xrack_runtime::DragEvent) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Drag(event).deliver_input(self, scene, path)
                }

                fn resize_split_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    weights: &[(::core::primitive::usize, ::core::primitive::f32)],
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::ResizeSplit(weights).deliver_input(self, scene, path)
                }

                fn resize_split_sizes_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    sizes: &[(::core::primitive::usize, ::core::primitive::f32)],
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::ResizeSplitSizes(sizes).deliver_input(self, scene, path)
                }

                fn edit_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    text: ::std::string::String,
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Edit(&text).deliver_input(self, scene, path)
                }

                fn commit_table_cell(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    original: &::core::primitive::str,
                    text: ::std::string::String,
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::CommitCell { original, text: &text }.deliver_input(self, scene, path)
                }

                fn dispatch_at(
                    &mut self,
                    scene: &::xrack_runtime::Node,
                    path: &[::core::primitive::usize],
                    event: &::core::primitive::str,
                ) -> ::core::primitive::bool {
                    ::xrack_runtime::RoutedEvent::Dispatch(event).deliver_input(self, scene, path)
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
    generate_with_locale(module, images, fonts, paths, None)
}

fn collect_static_groups(
    op: OperationRef<'_>,
    gradients: &mut BTreeSet<i64>,
    scenes: &mut BTreeSet<i64>,
) {
    if let Some(group) = GradientRef::static_group(op) {
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

pub fn generate_with_locale(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
    paths: &BTreeMap<String, EmbeddedPath>,
    translations: Option<&EmbeddedTranslations>,
) -> Result<TokenStream, String> {
    generate_with_resources(module, images, fonts, paths, translations, true, "")
}

pub use generate_with_locale as generate_with_translations;

/// Generate from the files already decoded and checked by the resource resolver.
#[doc(hidden)]
pub fn generate_with_resolved_assets(
    module: &Module,
    assets: &xrack_resources::ResolvedAssets,
    translations: Option<&EmbeddedTranslations>,
) -> Result<TokenStream, String> {
    generate_with_resolved_assets_at(module, assets, translations, "")
}

/// Generate file-backed UI using the entry file as a fallback for media paths
/// when an operation has no declaration location.
#[doc(hidden)]
pub fn generate_with_resolved_assets_at(
    module: &Module,
    assets: &xrack_resources::ResolvedAssets,
    translations: Option<&EmbeddedTranslations>,
    entry_file: &str,
) -> Result<TokenStream, String> {
    generate_with_resources(
        module,
        &assets.images,
        &assets.fonts,
        &assets.paths,
        translations,
        false,
        entry_file,
    )
}

fn uses_locale(op: OperationRef<'_>) -> bool {
    matches!(
        op.name(),
        "xr.locale" | "xr.format_number" | "xr.parse_number"
    ) || op.children().any(uses_locale)
}

fn generate_with_resources(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
    fonts: &[Vec<u8>],
    paths: &BTreeMap<String, EmbeddedPath>,
    translations: Option<&EmbeddedTranslations>,
    validate_asset_data: bool,
    entry_file: &str,
) -> Result<TokenStream, String> {
    module.verify()?;
    xrack_resources::validate_module_resources(
        module,
        xrack_resources::ResourceImages::Embedded(images),
        fonts.iter().map(Vec::as_slice),
        translations.map(EmbeddedTranslations::sources),
        validate_asset_data,
    )?;
    let missing_paths = xrack_resources::collect_missing_vector_paths(module, |commands| {
        paths.contains_key(commands)
    })?;
    let globals: Vec<_> = module
        .operations()
        .filter(|op| op.attribute("global") == Some("true"))
        .collect();
    let with_locale = translations.is_some() || module.operations().any(uses_locale);
    let mut out = quote! {
        const _: () = assert!(
            ::xrack_runtime::CODEGEN_ABI_VERSION == 15,
            "xrackc and xrack-runtime have incompatible code generation interfaces"
        );
    };
    if !globals.is_empty() {
        let mut fields = TokenStream::new();
        for &global in &globals {
            let name = global.attr("sym_name")?;
            let field = format_ident!("__{}", rust_key(name));
            let ty = ident(name);
            fields.extend(quote!(#field: ::std::cell::OnceCell<#ty>,));
        }
        out.extend(quote! {
            #[derive(::core::default::Default)]
            #[allow(non_snake_case)]
            struct __XrGlobals { #fields }
        });
    }
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
    for (index, (commands, path)) in paths.iter().chain(&missing_paths).enumerate() {
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
    let AliasGraphs {
        properties: alias_graphs,
        callbacks: callback_graphs,
    } = AliasGraphs::from_module(module)?;
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
        let root = children(op, "xr.element")
            .next()
            .ok_or("component has no root element")?;
        let mut generator = Generator {
            component_name: op.attr("sym_name")?,
            root_key: element_key(root)?,
            alias_graphs: &alias_graphs,
            alias_graph,
            elements: BTreeMap::new(),
            callback_graphs: &callback_graphs,
            callback_graph,
            instance_handlers: Vec::new(),
            records: &records,
            instances: BTreeMap::new(),
            analysis: ComponentAnalysis::from_component(
                op,
                &alias_graphs,
                &callback_graphs,
                module.has_visual_cache_plan(),
            )?,
            element_properties: Vec::new(),
            unlinked_controls: Vec::new(),
            record_combos: BTreeMap::new(),
            picker_popups: Vec::new(),
            tables: Vec::new(),
            standard_tables: BTreeMap::new(),
            row_height_views: BTreeSet::new(),
            sectioned_lists: BTreeSet::new(),
            section_headers: BTreeMap::new(),
            combo_options: BTreeMap::new(),
            splits: Vec::new(),
            trees: BTreeMap::new(),
            stacks: BTreeMap::new(),
            actions: BTreeMap::new(),
            timers: Vec::new(),
            animated_images: Vec::new(),
            media_players: Vec::new(),
            native_dialogs: Vec::new(),
            focus_targets: BTreeSet::new(),
            button_press_targets: BTreeSet::new(),
            button_checked_targets: BTreeSet::new(),
            toggle_state_targets: BTreeSet::new(),
            numeric_read_targets: BTreeSet::new(),
            combo_state_targets: BTreeSet::new(),
            combo_popup_targets: BTreeSet::new(),
            range_position_targets: BTreeMap::new(),
            radio_read_groups: BTreeSet::new(),
            navigation_read_groups: BTreeSet::new(),
            repeated_buttons: BTreeSet::new(),
            button_animation_targets: BTreeSet::new(),
            text_selection_targets: BTreeSet::new(),
            text_command_targets: BTreeSet::new(),
            text_validation_targets: BTreeSet::new(),
            pointer_targets: BTreeSet::new(),
            viewport_targets: BTreeSet::new(),
            view_count_targets: BTreeSet::new(),
            list_selection_info_targets: BTreeSet::new(),
            list_selection_command_targets: BTreeSet::new(),
            list_selection_targets: BTreeSet::new(),
            key_scroll_targets: BTreeSet::new(),
            view_empty_state_owners: BTreeMap::new(),
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
            entry_file,
            with_fonts: !fonts.is_empty(),
            with_locale,
            globals: &globals,
            has_translation_catalog: translations.is_some(),
        };
        generator.index(op, None)?;
        if let Some(root) = children(op, "xr.element").next() {
            let mut visuals = VisualScope::default();
            generator.index_visual_bindings(root, &mut visuals, &mut 0, None)?;
            for &action in generator.actions.values() {
                generator.index_visual_bindings(action, &mut visuals, &mut 0, None)?;
            }
            for &timer in &generator.timers {
                generator.index_visual_bindings(timer, &mut visuals, &mut 0, None)?;
            }
            for &effect in generator
                .media_players
                .iter()
                .filter(|effect| effect.attr("kind") == Ok("SoundEffect"))
            {
                generator.index_visual_bindings(effect, &mut visuals, &mut 0, None)?;
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
