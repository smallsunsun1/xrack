include "mlir/IR/OpBase.td"
include "mlir/IR/AttrTypeBase.td"
include "mlir/Interfaces/SideEffectInterfaces.td"

def Xrack_Dialect : Dialect {
  let name = "xr";
  let cppNamespace = "::xrack::dialect";
  let summary = "Typed declarative UI operations for XRack";
  let useDefaultTypePrinterParser = 1;
}

class XR_Type<string name, string spelling> : TypeDef<Xrack_Dialect, name> {
  let mnemonic = spelling;
}
def XR_Int : XR_Type<"Int", "int">;
def XR_Float : XR_Type<"Float", "float">;
def XR_Bool : XR_Type<"Bool", "bool">;
def XR_String : XR_Type<"String", "string">;
def XR_StyledText : XR_Type<"StyledText", "styledtext">;
def XR_StringList : XR_Type<"StringList", "stringlist">;
class XR_NamedType<string name, string spelling> : XR_Type<name, spelling> {
  let parameters = (ins StringRefParameter<"record symbol">:$name);
  let assemblyFormat = "`<` $name `>`";
}
def XR_Record : XR_NamedType<"Record", "record">;
def XR_RecordList : XR_NamedType<"RecordList", "recordlist">;
def XR_Length : XR_Type<"Length", "length">;
def XR_Percent : XR_Type<"Percent", "percent">;
def XR_Color : XR_Type<"Color", "color">;
def XR_Brush : XR_Type<"Brush", "brush">;
def XR_Image : XR_Type<"Image", "image">;
def XR_ImageFit : XR_Type<"ImageFit", "imagefit">;
def XR_ImageTileMode : XR_Type<"ImageTileMode", "imagetilemode">;
def XR_ImageHorizontalAlignment : XR_Type<"ImageHorizontalAlignment", "imagehorizontalalignment">;
def XR_ImageVerticalAlignment : XR_Type<"ImageVerticalAlignment", "imageverticalalignment">;
def XR_ImageRendering : XR_Type<"ImageRendering", "imagerendering">;
def XR_LayoutAlignment : XR_Type<"LayoutAlignment", "layoutalignment">;
def XR_CrossAxisAlignment : XR_Type<"CrossAxisAlignment", "crossaxisalignment">;
def XR_Orientation : XR_Type<"Orientation", "orientation">;
def XR_FlickDirection : XR_Type<"FlickDirection", "flickdirection">;
def XR_FlickBoundsBehavior : XR_Type<"FlickBoundsBehavior", "flickboundsbehavior">;
def XR_FlickBoundsMovement : XR_Type<"FlickBoundsMovement", "flickboundsmovement">;
def XR_FlexboxLayoutDirection : XR_Type<"FlexboxLayoutDirection", "flexboxlayoutdirection">;
def XR_FlexboxLayoutWrap : XR_Type<"FlexboxLayoutWrap", "flexboxlayoutwrap">;
def XR_DialInputMode : XR_Type<"DialInputMode", "dialinputmode">;
def XR_ScrollBarPolicy : XR_Type<"ScrollBarPolicy", "scrollbarpolicy">;
def XR_ScrollSnapMode : XR_Type<"ScrollSnapMode", "scrollsnapmode">;
def XR_ScrollAlignment : XR_Type<"ScrollAlignment", "scrollalignment">;
def XR_TextWrap : XR_Type<"TextWrap", "textwrap">;
def XR_TextHorizontalAlignment : XR_Type<"TextHorizontalAlignment", "texthorizontalalignment">;
def XR_TextVerticalAlignment : XR_Type<"TextVerticalAlignment", "textverticalalignment">;
def XR_TextOverflow : XR_Type<"TextOverflow", "textoverflow">;
def XR_InputType : XR_Type<"InputMode", "inputtype">;
def XR_CursorShape : XR_Type<"CursorShape", "cursorshape">;
def XR_ColorScheme : XR_Type<"ColorScheme", "colorscheme">;
def XR_FileDialogMode : XR_Type<"FileDialogMode", "filedialogmode">;
def XR_MessageLevel : XR_Type<"MessageLevel", "messagelevel">;
def XR_MessageButtons : XR_Type<"MessageButtons", "messagebuttons">;
def XR_CheckState : XR_Type<"CheckState", "checkstate">;
def XR_DialogRole : XR_Type<"DialogRole", "dialogrole">;
def XR_StandardButtonKind : XR_Type<"StandardButtonKind", "standardbuttonkind">;
def XR_SwipeSide : XR_Type<"SwipeSide", "swipeside">;
def XR_DragAction : XR_Type<"DragAction", "dragaction">;
def XR_EventResult : XR_Type<"EventResult", "eventresult">;
def XR_FocusReason : XR_Type<"FocusReason", "focusreason">;
def XR_KeyEvent : XR_Type<"KeyEvent", "keyevent">;
def XR_WheelEvent : XR_Type<"WheelEvent", "wheelevent">;
def XR_PointerEvent : XR_Type<"PointerEvent", "pointerevent">;
def XR_PointerEventKind : XR_Type<"PointerEventKind", "pointereventkind">;
def XR_PointerButton : XR_Type<"PointerButton", "pointerbutton">;
def XR_PointerButtons : XR_Type<"PointerButtons", "pointerbuttons">;
def XR_TouchPoint : XR_Type<"TouchPoint", "touchpoint">;
def XR_TouchPoints : XR_Type<"TouchPoints", "touchpoints">;
def XR_KeyModifiers : XR_Type<"KeyModifiers", "keymodifiers">;
def XR_DrawerEdge : XR_Type<"DrawerEdge", "draweredge">;
def XR_PopupPlacement : XR_Type<"PopupPlacement", "popupplacement">;
def XR_SelectionTarget : XR_Type<"SelectionTarget", "selectiontarget">;
def XR_Date : XR_Type<"Date", "date">;
def XR_Time : XR_Type<"Time", "time">;
def XR_Font : XR_Type<"Font", "font">;
def XR_GradientStop : XR_Type<"GradientStop", "gradientstop">;
def XR_Value : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_String, XR_StyledText, XR_StringList, XR_Record, XR_RecordList, XR_Length, XR_Percent, XR_Color,
                         XR_EventResult, XR_FocusReason, XR_KeyEvent, XR_KeyModifiers, XR_WheelEvent,
                         XR_PointerEvent, XR_PointerEventKind, XR_PointerButton, XR_PointerButtons, XR_TouchPoint, XR_TouchPoints,
                         XR_Brush, XR_Image, XR_ImageFit, XR_ImageTileMode,
                         XR_ImageHorizontalAlignment, XR_ImageVerticalAlignment, XR_ImageRendering, XR_LayoutAlignment, XR_CrossAxisAlignment,
                         XR_Orientation, XR_FlickDirection, XR_FlickBoundsBehavior, XR_FlickBoundsMovement, XR_FlexboxLayoutDirection, XR_FlexboxLayoutWrap, XR_DialInputMode, XR_ScrollBarPolicy, XR_ScrollSnapMode, XR_ScrollAlignment, XR_TextWrap, XR_TextHorizontalAlignment, XR_TextVerticalAlignment, XR_TextOverflow, XR_InputType, XR_CursorShape, XR_ColorScheme, XR_FileDialogMode, XR_MessageLevel, XR_MessageButtons, XR_CheckState, XR_DialogRole, XR_StandardButtonKind, XR_SwipeSide, XR_DragAction, XR_DrawerEdge, XR_PopupPlacement, XR_SelectionTarget, XR_Date, XR_Time, XR_Font]>;
def XR_Stringifiable : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_Date, XR_Time]>;

class XR_Op<string mnemonic, list<Trait> traits = []> : Op<Xrack_Dialect, mnemonic, traits>;

def XR_EndOp : XR_Op<"end", [Terminator]>;

class XR_ScopeOp<string mnemonic, list<Trait> traits = []>
    : XR_Op<mnemonic, !listconcat([SingleBlockImplicitTerminator<"EndOp">, NoRegionArguments], traits)> {
  let regions = (region SizedRegion<1>:$body);
}

def XR_ComponentOp : XR_ScopeOp<"component", [HasParent<"::mlir::ModuleOp">]> {
  let arguments = (ins StrAttr:$sym_name, StrAttr:$exported);
  let hasVerifier = 1;
}
def XR_RecordDeclOp : XR_Op<"record_decl", [HasParent<"::mlir::ModuleOp">]> {
  let arguments = (ins StrAttr:$sym_name, StrAttr:$fields);
}
def XR_FontOp : XR_Op<"font", [HasParent<"::mlir::ModuleOp">]> {
  let arguments = (ins StrAttr:$path);
}
def XR_ElementOp : XR_ScopeOp<"element"> {
  let arguments = (ins StrAttr:$kind, StrAttr:$id, OptionalAttr<StrAttr>:$key,
                       OptionalAttr<StrAttr>:$delegate);
  let hasVerifier = 1;
}
def XR_InstanceOp : XR_ScopeOp<"instance", [HasParent<"ElementOp">]> {
  let arguments = (ins FlatSymbolRefAttr:$component, StrAttr:$id, StrAttr:$key);
  let hasVerifier = 1;
}
def XR_CallbackOp : XR_ScopeOp<"callback", [ParentOneOf<["ComponentOp", "ElementOp", "InstanceOp"]>]> {
  let arguments = (ins StrAttr:$name, StrAttr:$parameters, OptionalAttr<StrAttr>:$pure,
                       OptionalAttr<StrAttr>:$alias_owner, OptionalAttr<StrAttr>:$alias_name,
                       OptionalAttr<StrAttr>:$implementation);
  let hasVerifier = 1;
}
def XR_HandlerOp : XR_ScopeOp<"handler", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$name, StrAttr:$parameters);
  let hasVerifier = 1;
}

def XR_PropertyOp : XR_Op<"property"> {
  let arguments = (ins XR_Value:$initial, StrAttr:$name, StrAttr:$visibility, OptionalAttr<StrAttr>:$alias);
  let hasVerifier = 1;
}
def XR_BindOp : XR_Op<"bind"> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
}
def XR_ActionRefOp : XR_Op<"action_ref", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$owner);
}
def XR_ActionGroupRefOp : XR_Op<"action_group_ref", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$group, StrAttr:$value);
}
def XR_RadioGroupRefOp : XR_Op<"radio_group_ref", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$group);
}
def XR_CheckGroupRefOp : XR_Op<"check_group_ref", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$group, StrAttr:$property);
}
def XR_FocusTargetOp : XR_Op<"focus_target", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$target);
  let hasVerifier = 1;
}
def XR_ScrollTargetOp : XR_Op<"scroll_target", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$target);
}
def XR_SplitFillOp : XR_Op<"split_fill", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$target);
  let hasVerifier = 1;
}
def XR_ShowIfOp : XR_Op<"show_if"> {
  let arguments = (ins XR_Bool:$input, StrAttr:$name);
}
def XR_AnimateOp : XR_Op<"animate", [HasParent<"ElementOp">]> {
  let arguments = (ins XR_Int:$input, Optional<XR_Bool>:$enabled, StrAttr:$name,
                       OptionalAttr<I64Attr>:$delay, OptionalAttr<DenseF64ArrayAttr>:$curve);
  let hasVerifier = 1;
}
def XR_StateSelectOp : XR_Op<"state_select", [ParentOneOf<["ElementOp", "InstanceOp"]>]> {
  let arguments = (ins Variadic<XR_Bool>:$conditions, StrAttr:$owner, ArrayAttr:$names);
  let results = (outs XR_Int:$selected);
  let hasVerifier = 1;
}
def XR_StateSetOp : XR_Op<"state_set", [ParentOneOf<["ElementOp", "InstanceOp"]>]> {
  let arguments = (ins XR_Int:$selector, XR_Value:$input, StrAttr:$name, I64Attr:$case_index);
  let hasVerifier = 1;
}
def XR_InstanceStateOp : XR_Op<"instance_state", [HasParent<"InstanceOp">]> {
  let arguments = (ins XR_Int:$selector, XR_Value:$input, StrAttr:$name, I64Attr:$case_index);
  let hasVerifier = 1;
}
def XR_StateTimingOp : XR_Op<"state_timing", [ParentOneOf<["ElementOp", "InstanceOp"]>]> {
  let arguments = (ins XR_Int:$selector, XR_Int:$input, Optional<XR_Bool>:$enabled,
                       StrAttr:$name, I64Attr:$case_index, StrAttr:$direction,
                       OptionalAttr<I64Attr>:$delay, OptionalAttr<DenseF64ArrayAttr>:$curve);
  let hasVerifier = 1;
}
def XR_RepeatOp : XR_Op<"repeat", [ParentOneOf<["ElementOp", "InstanceOp"]>]> {
  let arguments = (ins AnyTypeOf<[XR_Int, XR_StringList, XR_RecordList, XR_TouchPoints]>:$input, StrAttr:$name,
                       StrAttr:$item, StrAttr:$index);
}
def XR_ControlLinkOp : XR_Op<"control_link", [HasParent<"ElementOp">]> {
  let arguments = (ins AnyTypeOf<[XR_String, XR_Bool, XR_Float, XR_Int, XR_Date, XR_Time,
                                 XR_CheckState, XR_SwipeSide, XR_Color, XR_Length, XR_Font]>:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_InstanceLinkOp : XR_Op<"instance_link", [HasParent<"InstanceOp">]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_ClipboardPasteOp : XR_ScopeOp<"clipboard_paste", [HasParent<"InstanceOp">]> {
  let arguments = (ins StrAttr:$target, StrAttr:$text_parameter);
  let hasVerifier = 1;
}
def XR_RowLinkOp : XR_Op<"row_link", [HasParent<"ElementOp">]> {
  let arguments = (ins AnyTypeOf<[XR_String, XR_Int, XR_Bool, XR_Float, XR_CheckState]>:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_LayoutBindOp : XR_Op<"layout_bind", [HasParent<"InstanceOp">]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_AssignOp : XR_Op<"assign"> {
  let arguments = (ins XR_Value:$input, StrAttr:$name, OptionalAttr<StrAttr>:$owner);
}
def XR_ReturnOp : XR_Op<"return", [ParentOneOf<["CallbackOp", "HandlerOp", "ClipboardPasteOp"]>]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
}
def XR_InvokeOp : XR_Op<"invoke"> {
  let arguments = (ins Variadic<XR_Value>:$args, StrAttr:$owner, StrAttr:$name);
}
def XR_NavigateOp : XR_Op<"navigate"> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_String, XR_StringList]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TimerCommandOp : XR_Op<"timer_command"> {
  let arguments = (ins StrAttr:$owner, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_PopupCommandOp : XR_Op<"popup_command"> {
  let arguments = (ins StrAttr:$owner, StrAttr:$open);
  let hasVerifier = 1;
}
def XR_FileDialogOpenOp : XR_Op<"file_dialog_open"> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_NativeDialogOpenOp : XR_Op<"native_dialog_open"> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_WindowCloseOp : XR_Op<"window_close", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let hasVerifier = 1;
}
def XR_WindowCommandOp : XR_Op<"window_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Optional<XR_Bool>:$enabled, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_CheckGroupCommandOp : XR_Op<"check_group_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins XR_CheckState:$state, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_SecondaryWindowCommandOp : XR_Op<"secondary_window_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_FocusCommandOp : XR_Op<"focus_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$target, StrAttr:$action);
  let hasVerifier = 1;
}
def XR_SwipeCancelOp : XR_Op<"swipe_cancel", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_TextCommandOp : XR_Op<"text_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$offsets, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_ScrollCommandOp : XR_Op<"scroll_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Length, XR_Int, XR_ScrollAlignment]>>:$args,
                       StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_SourceSelectionOp : XR_Op<"source_selection", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableSelectionCommandOp : XR_Op<"table_selection_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableSortCommandOp : XR_Op<"table_sort_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Int, XR_Bool]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableHistoryCommandOp : XR_Op<"table_history_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_SplitRestoreOp : XR_Op<"split_restore", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins XR_String:$state, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_TreeCommandOp : XR_Op<"tree_command", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_CancelFlickOp : XR_Op<"cancel_flick", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_ReturnToBoundsOp : XR_Op<"return_to_bounds", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_MoveColumnOp : XR_Op<"move_column", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins XR_Int:$source, XR_Int:$destination, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_MoveRowOp : XR_Op<"move_row", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins XR_Int:$source, XR_Int:$destination, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_SetRowHeightOp : XR_Op<"set_row_height", [ParentOneOf<["CallbackOp", "HandlerOp"]>]> {
  let arguments = (ins XR_Int:$row, XR_Length:$height, StrAttr:$owner);
  let hasVerifier = 1;
}

class XR_ExpressionOp<string mnemonic, dag operands, TypeConstraint output = XR_Value,
                      list<Trait> traits = []> : XR_Op<mnemonic, traits> {
  // The string carries literal spelling or an operator/property name. SSA
  // values themselves use registered XR types and verified operand arities.
  let arguments = !con(operands, (ins StrAttr:$value));
  let results = (outs output:$result);
}

def XR_LiteralOp : XR_ExpressionOp<"literal", (ins), XR_Value, [Pure]> {
  let hasVerifier = 1;
}
def XR_ImageUrlOp : XR_ExpressionOp<"image_url", (ins), XR_Image, [Pure]>;
def XR_ImageDimensionOp : XR_ExpressionOp<"image_dimension", (ins XR_Image:$input), XR_Int, [Pure]> {
  let hasVerifier = 1;
}
def XR_NineSliceOp : XR_ExpressionOp<"nine_slice", (ins XR_Image:$input,
    XR_Int:$top, XR_Int:$right, XR_Int:$bottom, XR_Int:$left), XR_Image, [Pure]> {
  let hasVerifier = 1;
}
def XR_MarkdownOp : XR_ExpressionOp<"markdown", (ins), XR_StyledText, [Pure]>;
def XR_ArrayOp : XR_ExpressionOp<"array", (ins Variadic<XR_String>:$items), XR_StringList, [Pure]>;
def XR_PointerButtonsOp : XR_ExpressionOp<"pointer_buttons", (ins Variadic<XR_PointerButton>:$items), XR_PointerButtons, [Pure]>;
def XR_RecordArrayOp : XR_ExpressionOp<"record_array", (ins Variadic<XR_Record>:$items), XR_RecordList, [Pure]>;
def XR_MakeRecordOp : XR_ExpressionOp<"make_record", (ins Variadic<XR_Value>:$fields), XR_Record, [Pure]>;
def XR_DefaultRecordOp : XR_ExpressionOp<"default_record", (ins), XR_Record, [Pure]>;
def XR_GetFieldOp : XR_ExpressionOp<"get_field", (ins XR_Record:$input), XR_Value, [Pure]>;
def XR_KeyFieldOp : XR_ExpressionOp<"key_field", (ins AnyTypeOf<[XR_KeyEvent, XR_KeyModifiers]>:$input), XR_Value, [Pure]> {
  let hasVerifier = 1;
}
def XR_WheelFieldOp : XR_ExpressionOp<"wheel_field", (ins XR_WheelEvent:$input), AnyTypeOf<[XR_Length, XR_Float, XR_Bool, XR_KeyModifiers]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_PointerFieldOp : XR_ExpressionOp<"pointer_field", (ins XR_PointerEvent:$input), AnyTypeOf<[XR_Length, XR_Int, XR_Bool, XR_KeyModifiers, XR_PointerEventKind, XR_PointerButton]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_TouchPointFieldOp : XR_ExpressionOp<"touch_point_field", (ins XR_TouchPoint:$input), AnyTypeOf<[XR_Length, XR_Int, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_MakeDateOp : XR_ExpressionOp<"make_date", (ins XR_Int:$year, XR_Int:$month, XR_Int:$day), XR_Date, [Pure]>;
def XR_DateFieldOp : XR_ExpressionOp<"date_field", (ins XR_Date:$input), XR_Int, [Pure]> {
  let hasVerifier = 1;
}
def XR_MakeTimeOp : XR_ExpressionOp<"make_time", (ins XR_Int:$hour, XR_Int:$minute, XR_Int:$second), XR_Time, [Pure]>;
def XR_TimeFieldOp : XR_ExpressionOp<"time_field", (ins XR_Time:$input), XR_Int, [Pure]> {
  let hasVerifier = 1;
}
def XR_MakeFontOp : XR_ExpressionOp<"make_font", (ins XR_String:$family, XR_Length:$size,
    XR_Int:$weight, XR_Bool:$italic), XR_Font, [Pure]>;
def XR_FontFieldOp : XR_ExpressionOp<"font_field", (ins XR_Font:$input),
    AnyTypeOf<[XR_String, XR_Length, XR_Int, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_ReadOp : XR_ExpressionOp<"read", (ins OptionalAttr<StrAttr>:$owner)>;
def XR_TableSelectionReadOp : XR_ExpressionOp<"table_selection_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Int, XR_Bool]>> {
  let hasVerifier = 1;
}
def XR_TableSortReadOp : XR_ExpressionOp<"table_sort_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Int, XR_Bool]>> {
  let hasVerifier = 1;
}
def XR_TableHistoryReadOp : XR_ExpressionOp<"table_history_read", (ins StrAttr:$owner), XR_Bool> {
  let hasVerifier = 1;
}
def XR_SplitStateReadOp : XR_ExpressionOp<"split_state_read", (ins StrAttr:$owner), XR_String>;
def XR_SplitResizingOp : XR_ExpressionOp<"split_resizing", (ins StrAttr:$owner), XR_Bool>;
def XR_TreeQueryOp : XR_ExpressionOp<"tree_query", (ins XR_Int:$key, StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Int]>> {
  let hasVerifier = 1;
}
def XR_ViewIndexOp : XR_ExpressionOp<"view_index", (ins XR_Int:$index, StrAttr:$owner), XR_Int> {
  let hasVerifier = 1;
}
def XR_StackReadOp : XR_ExpressionOp<"stack_read", (ins Optional<XR_String>:$route, StrAttr:$owner), AnyTypeOf<[XR_String, XR_Int, XR_Bool]>> {
  let hasVerifier = 1;
}
def XR_ImagePlaybackReadOp : XR_ExpressionOp<"image_playback_read", (ins StrAttr:$owner), XR_Int> {
  let hasVerifier = 1;
}
def XR_WindowReadOp : XR_ExpressionOp<"window_read", (ins), XR_Bool, [Pure]> {
  let hasVerifier = 1;
}
def XR_CheckGroupReadOp : XR_ExpressionOp<"check_group_read", (ins StrAttr:$owner), XR_CheckState>;
def XR_ActionGroupReadOp : XR_ExpressionOp<"action_group_read", (ins StrAttr:$owner), XR_String>;
def XR_NativeDialogReadOp : XR_ExpressionOp<"native_dialog_read", (ins StrAttr:$owner),
                                           AnyTypeOf<[XR_Bool, XR_String, XR_StringList]>>;
def XR_FocusReadOp : XR_ExpressionOp<"focus_read", (ins StrAttr:$owner), XR_Bool, [Pure]> {
  let hasVerifier = 1;
}
def XR_ControlReadOp : XR_ExpressionOp<"control_read", (ins StrAttr:$owner), XR_Value, [Pure]> {
  let hasVerifier = 1;
}
def XR_GeometryReadOp : XR_ExpressionOp<"geometry_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Length, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_ViewportReadOp : XR_ExpressionOp<"viewport_read", (ins StrAttr:$owner), XR_Length, [Pure]> {
  let hasVerifier = 1;
}
def XR_FlickMotionReadOp : XR_ExpressionOp<"flick_motion_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>>;
def XR_ScrollControlReadOp : XR_ExpressionOp<"scroll_control_read", (ins StrAttr:$owner), XR_Float>;
def XR_ScaleRotateReadOp : XR_ExpressionOp<"scale_rotate_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Float, XR_Length]>>;
def XR_SwipeReadOp : XR_ExpressionOp<"swipe_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>>;
def XR_HoverReadOp : XR_ExpressionOp<"hover_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>> {
  let hasVerifier = 1;
}
def XR_TouchAreaReadOp : XR_ExpressionOp<"touch_area_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Int, XR_Length, XR_TouchPoints]>> {
  let hasVerifier = 1;
}
def XR_ParamOp : XR_ExpressionOp<"param", (ins)>;
def XR_CallOp : XR_ExpressionOp<"call", (ins Variadic<XR_Value>:$args, StrAttr:$owner)>;
def XR_UnaryOp : XR_ExpressionOp<"unary", (ins XR_Value:$input), XR_Value,
                                [AllTypesMatch<["input", "result"]>]>;
def XR_CastOp : XR_ExpressionOp<"cast", (ins XR_Int:$input), XR_Float>;
def XR_ToRatioOp : XR_ExpressionOp<"to_ratio", (ins XR_Percent:$input), XR_Float, [Pure]>;
def XR_StringifyOp : XR_ExpressionOp<"stringify", (ins XR_Stringifiable:$input), XR_String>;
def XR_SolidBrushOp : XR_ExpressionOp<"solid_brush", (ins XR_Color:$color), XR_Brush, [Pure]>;
def XR_GradientStopOp : XR_ExpressionOp<"gradient_stop", (ins XR_Color:$color, XR_Float:$offset),
                                       XR_GradientStop, [Pure]> {
  let hasVerifier = 1;
}
def XR_LinearGradientOp : XR_ExpressionOp<"linear_gradient",
    (ins XR_Float:$angle, Variadic<XR_GradientStop>:$stops), XR_Brush, [Pure]> {
  let hasVerifier = 1;
}
def XR_RadialGradientOp : XR_ExpressionOp<"radial_gradient",
    (ins Variadic<XR_GradientStop>:$stops), XR_Brush, [Pure]> {
  let hasVerifier = 1;
}
def XR_BinaryOp : XR_ExpressionOp<"binary", (ins XR_Value:$lhs, XR_Value:$rhs)>;
def XR_SelectOp : XR_ExpressionOp<"select", (ins XR_Bool:$condition, XR_Value:$on_true, XR_Value:$on_false),
                                 XR_Value, [Pure, AllTypesMatch<["on_true", "on_false", "result"]>]> {
  let hasFolder = 1;
}
