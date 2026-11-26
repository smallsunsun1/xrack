include "mlir/IR/OpBase.td"
include "mlir/IR/AttrTypeBase.td"
include "mlir/IR/SymbolInterfaces.td"
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
def XR_IntList : XR_Type<"IntList", "intlist">;
class XR_NamedType<string name, string spelling> : XR_Type<name, spelling> {
  let parameters = (ins StringRefParameter<"record symbol">:$name);
  let assemblyFormat = "`<` $name `>`";
}
def XR_Record : XR_NamedType<"Record", "record">;
def XR_RecordList : XR_NamedType<"RecordList", "recordlist">;
def XR_Length : XR_Type<"Length", "length">;
def XR_Point2D : XR_Type<"Point2D", "point2d">;
def XR_Point3D : XR_Type<"Point3D", "point3d">;
def XR_Vector3D : XR_Type<"Vector3D", "vector3d">;
def XR_Quaternion3D : XR_Type<"Quaternion3D", "quaternion3d">;
def XR_RotationTrack3D : XR_Type<"RotationTrack3D", "rotationtrack3d">;
def XR_Hit3D : XR_Type<"Hit3D", "hit3d">;
def XR_Instance3D : XR_Type<"Instance3D", "instance3d">;
def XR_Bounds3D : XR_Type<"Bounds3D", "bounds3d">;
def XR_MeshNode3D : XR_Type<"MeshNode3D", "meshnode3d">;
def XR_MeshAsset3D : XR_Type<"MeshAsset3D", "meshasset3d">;
def XR_InstanceTable3D : XR_Type<"InstanceTable3D", "instancetable3d">;
def XR_Hit3DList : XR_Type<"Hit3DList", "hit3dlist">;
def XR_PhysicsContact3D : XR_Type<"PhysicsContact3D", "physicscontact3d">;
def XR_PhysicsHit3D : XR_Type<"PhysicsHit3D", "physicshit3d">;
def XR_PhysicsContact3DList : XR_Type<"PhysicsContact3DList", "physicscontact3dlist">;
def XR_PhysicsHit3DList : XR_Type<"PhysicsHit3DList", "physicshit3dlist">;
def XR_PhysicsWorld3D : XR_Type<"PhysicsWorld3D", "physicsworld3d">;
def XR_SceneModel3D : XR_Type<"SceneModel3D", "scenemodel3d">;
def XR_WalkResult3D : XR_Type<"WalkResult3D", "walkresult3d">;
def XR_ProjectedPoint3D : XR_Type<"ProjectedPoint3D", "projectedpoint3d">;
def XR_Vector2D : XR_Type<"Vector2D", "vector2d">;
def XR_Transform2D : XR_Type<"Transform2D", "transform2d">;
def XR_Percent : XR_Type<"Percent", "percent">;
def XR_Color : XR_Type<"Color", "color">;
def XR_Brush : XR_Type<"Brush", "brush">;
def XR_Image : XR_Type<"Image", "image">;
def XR_LogLevel : XR_Type<"LogLevel", "loglevel">;
def XR_ImageFit : XR_Type<"ImageFit", "imagefit">;
def XR_ImageTileMode : XR_Type<"ImageTileMode", "imagetilemode">;
def XR_ImageHorizontalAlignment : XR_Type<"ImageHorizontalAlignment", "imagehorizontalalignment">;
def XR_ImageVerticalAlignment : XR_Type<"ImageVerticalAlignment", "imageverticalalignment">;
def XR_ImageRendering : XR_Type<"ImageRendering", "imagerendering">;
def XR_ButtonDisplay : XR_Type<"ButtonDisplay", "buttondisplay">;
def XR_LayoutAlignment : XR_Type<"LayoutAlignment", "layoutalignment">;
def XR_CrossAxisAlignment : XR_Type<"CrossAxisAlignment", "crossaxisalignment">;
def XR_Orientation : XR_Type<"Orientation", "orientation">;
def XR_FlickDirection : XR_Type<"FlickDirection", "flickdirection">;
def XR_FlickBoundsBehavior : XR_Type<"FlickBoundsBehavior", "flickboundsbehavior">;
def XR_FlickBoundsMovement : XR_Type<"FlickBoundsMovement", "flickboundsmovement">;
def XR_LayoutDirection : XR_Type<"LayoutDirection", "layoutdirection">;
def XR_FlexboxLayoutDirection : XR_Type<"FlexboxLayoutDirection", "flexboxlayoutdirection">;
def XR_FlexboxLayoutWrap : XR_Type<"FlexboxLayoutWrap", "flexboxlayoutwrap">;
def XR_DialInputMode : XR_Type<"DialInputMode", "dialinputmode">;
def XR_ScrollBarPolicy : XR_Type<"ScrollBarPolicy", "scrollbarpolicy">;
def XR_ScrollSnapMode : XR_Type<"ScrollSnapMode", "scrollsnapmode">;
def XR_ScrollAlignment : XR_Type<"ScrollAlignment", "scrollalignment">;
def XR_TextWrap : XR_Type<"TextWrap", "textwrap">;
def XR_MouseSelectionMode : XR_Type<"MouseSelectionMode", "mouseselectionmode">;
def XR_InputValidity : XR_Type<"InputValidity", "inputvalidity">;
def XR_TextHorizontalAlignment : XR_Type<"TextHorizontalAlignment", "texthorizontalalignment">;
def XR_TextVerticalAlignment : XR_Type<"TextVerticalAlignment", "textverticalalignment">;
def XR_TextOverflow : XR_Type<"TextOverflow", "textoverflow">;
def XR_InputType : XR_Type<"InputMode", "inputtype">;
def XR_InputPurpose : XR_Type<"InputPurpose", "inputpurpose">;
def XR_KeyboardCapitalization : XR_Type<"KeyboardCapitalization", "keyboardcapitalization">;
def XR_ReturnKey : XR_Type<"ReturnKey", "returnkey">;
def XR_AccessibleLive : XR_Type<"AccessibleLive", "accessiblelive">;
def XR_CursorShape : XR_Type<"CursorShape", "cursorshape">;
def XR_ColorScheme : XR_Type<"ColorScheme", "colorscheme">;
def XR_SceneToneMap : XR_Type<"SceneToneMap", "scenetonemap">;
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
def XR_SelectionGesture : XR_Type<"SelectionGesture", "selectiongesture">;
def XR_Date : XR_Type<"Date", "date">;
def XR_DateList : XR_Type<"DateList", "datelist">;
def XR_Time : XR_Type<"Time", "time">;
def XR_Font : XR_Type<"Font", "font">;
def XR_GradientStop : XR_Type<"GradientStop", "gradientstop">;
def XR_Value : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_String, XR_StyledText, XR_IntList, XR_DateList, XR_StringList, XR_Record, XR_RecordList, XR_Length, XR_Point2D, XR_Point3D, XR_Vector3D, XR_Quaternion3D, XR_RotationTrack3D, XR_Hit3D, XR_Instance3D, XR_InstanceTable3D, XR_MeshAsset3D, XR_MeshNode3D, XR_Bounds3D, XR_Hit3DList, XR_PhysicsContact3D, XR_PhysicsHit3D, XR_PhysicsContact3DList, XR_PhysicsHit3DList, XR_ProjectedPoint3D, XR_WalkResult3D, XR_PhysicsWorld3D, XR_SceneModel3D, XR_Vector2D, XR_Transform2D, XR_Percent, XR_Color,
                         XR_EventResult, XR_FocusReason, XR_KeyEvent, XR_KeyModifiers, XR_WheelEvent,
                         XR_PointerEvent, XR_PointerEventKind, XR_PointerButton, XR_PointerButtons, XR_TouchPoint, XR_TouchPoints,
                         XR_Brush, XR_Image, XR_LogLevel, XR_ImageFit, XR_ImageTileMode,
                         XR_ImageHorizontalAlignment, XR_ImageVerticalAlignment, XR_ImageRendering, XR_ButtonDisplay, XR_LayoutAlignment, XR_CrossAxisAlignment,
                         XR_Orientation, XR_FlickDirection, XR_FlickBoundsBehavior, XR_FlickBoundsMovement, XR_LayoutDirection, XR_FlexboxLayoutDirection, XR_FlexboxLayoutWrap, XR_DialInputMode, XR_ScrollBarPolicy, XR_ScrollSnapMode, XR_ScrollAlignment, XR_TextWrap, XR_MouseSelectionMode, XR_InputValidity, XR_TextHorizontalAlignment, XR_TextVerticalAlignment, XR_TextOverflow, XR_InputType, XR_InputPurpose, XR_KeyboardCapitalization, XR_ReturnKey, XR_AccessibleLive, XR_CursorShape, XR_ColorScheme, XR_SceneToneMap, XR_FileDialogMode, XR_MessageLevel, XR_MessageButtons, XR_CheckState, XR_DialogRole, XR_StandardButtonKind, XR_SwipeSide, XR_DragAction, XR_DrawerEdge, XR_PopupPlacement, XR_SelectionTarget, XR_SelectionGesture, XR_Date, XR_Time, XR_Font]>;
def XR_Stringifiable : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_Date, XR_Time]>;

class XR_Op<string mnemonic, list<Trait> traits = []> : Op<Xrack_Dialect, mnemonic, traits>;

def XR_EndOp : XR_Op<"end", [Terminator]>;

class XR_ScopeOp<string mnemonic, list<Trait> traits = []>
    : XR_Op<mnemonic, !listconcat([SingleBlockImplicitTerminator<"EndOp">, NoRegionArguments], traits)> {
  let regions = (region SizedRegion<1>:$body);
}

def XR_ComponentOp : XR_ScopeOp<"component", [HasParent<"::mlir::ModuleOp">, Symbol]> {
  let arguments = (ins StrAttr:$sym_name, StrAttr:$exported, OptionalAttr<StrAttr>:$global);
  let hasVerifier = 1;
}
def XR_RecordDeclOp : XR_Op<"record_decl", [HasParent<"::mlir::ModuleOp">, Symbol]> {
  let arguments = (ins StrAttr:$sym_name, StrAttr:$fields, OptionalAttr<ArrayAttr>:$variants, OptionalAttr<StrAttr>:$variant_payloads);
}
def XR_FontOp : XR_Op<"font", [HasParent<"::mlir::ModuleOp">]> {
  let arguments = (ins StrAttr:$path);
}
def XR_TranslationCatalogOp : XR_Op<"translation_catalog", [HasParent<"::mlir::ModuleOp">]> {
  let arguments = (ins StrAttr:$path);
}
def XR_ElementOp : XR_ScopeOp<"element"> {
  let arguments = (ins StrAttr:$kind, StrAttr:$id, OptionalAttr<StrAttr>:$key,
                       OptionalAttr<StrAttr>:$delegate, OptionalAttr<StrAttr>:$toggle_storage);
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

def XR_IfOp : XR_Op<"if", [SingleBlockImplicitTerminator<"EndOp">, NoRegionArguments,
    ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Bool:$condition);
  let regions = (region SizedRegion<1>:$thenBody, SizedRegion<1>:$elseBody);
  let hasVerifier = 1;
}
def XR_ForOp : XR_ScopeOp<"for", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins AnyTypeOf<[XR_Int, XR_IntList, XR_DateList, XR_StringList,
                                 XR_RecordList, XR_Hit3DList, XR_PhysicsContact3DList, XR_PhysicsHit3DList, XR_TouchPoints]>:$model,
                       StrAttr:$item, StrAttr:$index);
  let hasVerifier = 1;
}

def XR_LocalOp : XR_Op<"local", [AllTypesMatch<["input", "result"]>,
    ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
  let results = (outs XR_Value:$result);
}

def XR_PropertyOp : XR_Op<"property"> {
  let arguments = (ins XR_Value:$initial, StrAttr:$name, StrAttr:$visibility,
                       OptionalAttr<StrAttr>:$alias, OptionalAttr<StrAttr>:$mode);
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
  let arguments = (ins AnyTypeOf<[XR_Int, XR_IntList, XR_DateList, XR_StringList, XR_RecordList, XR_Hit3DList, XR_PhysicsContact3DList, XR_PhysicsHit3DList, XR_TouchPoints]>:$input, StrAttr:$name,
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
def XR_LogOp : XR_Op<"log", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>,
                                MemoryEffects<[MemWrite]>]> {
  let arguments = (ins XR_LogLevel:$level, XR_String:$message);
  let hasVerifier = 1;
}
def XR_EvaluateOp : XR_Op<"evaluate", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Value:$input);
}
def XR_InvokeOp : XR_Op<"invoke"> {
  let arguments = (ins Variadic<XR_Value>:$args, StrAttr:$owner, StrAttr:$name);
}
def XR_NavigateOp : XR_Op<"navigate"> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_String, XR_StringList]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TimerCommandOp : XR_Op<"timer_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_MediaCommandOp : XR_Op<"media_command"> {
  let arguments = (ins Optional<XR_Float>:$position, StrAttr:$owner, StrAttr:$method);
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
def XR_WindowCloseOp : XR_Op<"window_close", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let hasVerifier = 1;
}
def XR_WindowCommandOp : XR_Op<"window_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Optional<XR_Bool>:$enabled, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_CheckGroupCommandOp : XR_Op<"check_group_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_CheckState:$state, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_SecondaryWindowCommandOp : XR_Op<"secondary_window_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$command);
  let hasVerifier = 1;
}
def XR_FocusCommandOp : XR_Op<"focus_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$target, StrAttr:$action);
  let hasVerifier = 1;
}
def XR_ButtonClickOp : XR_Op<"button_click", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner, OptionalAttr<StrAttr>:$animated);
  let hasVerifier = 1;
}
def XR_ButtonToggleOp : XR_Op<"button_toggle", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_SwipeCancelOp : XR_Op<"swipe_cancel", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_TextCommandOp : XR_Op<"text_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Int, XR_String]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_ScrollCommandOp : XR_Op<"scroll_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Length, XR_Int, XR_ScrollAlignment]>>:$args,
                       StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_PositionChildOp : XR_Op<"position_child", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$target,
                       XR_ScrollAlignment:$horizontal, XR_ScrollAlignment:$vertical);
  let hasVerifier = 1;
}
def XR_SourceSelectionOp : XR_Op<"source_selection", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Int, XR_IntList]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableSelectionCommandOp : XR_Op<"table_selection_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableSortCommandOp : XR_Op<"table_sort_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_Int, XR_Bool]>>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_TableHistoryCommandOp : XR_Op<"table_history_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_SplitRestoreOp : XR_Op<"split_restore", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_String:$state, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_TreeCommandOp : XR_Op<"tree_command", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins Variadic<XR_Int>:$args, StrAttr:$owner, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_CancelFlickOp : XR_Op<"cancel_flick", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_ReturnToBoundsOp : XR_Op<"return_to_bounds", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_MoveColumnOp : XR_Op<"move_column", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Int:$source, XR_Int:$destination, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_MoveRowOp : XR_Op<"move_row", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Int:$source, XR_Int:$destination, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_MoveNavigationItemOp : XR_Op<"move_navigation_item", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Int:$source, XR_Int:$destination, StrAttr:$owner);
  let hasVerifier = 1;
}
def XR_SetRowHeightOp : XR_Op<"set_row_height", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
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
def XR_MeshUrlOp : XR_ExpressionOp<"mesh_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_MeshLodUrlOp : XR_ExpressionOp<"mesh_lod_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_MeshLightmapUrlOp : XR_ExpressionOp<"mesh_lightmap_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_MeshLodLightmapUrlOp : XR_ExpressionOp<"mesh_lod_lightmap_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_TextureUrlOp : XR_ExpressionOp<"texture_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_ShaderUrlOp : XR_ExpressionOp<"shader_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_InstanceTableUrlOp : XR_ExpressionOp<"instance_table_url", (ins), XR_InstanceTable3D, [Pure]> {
  let hasVerifier = 1;
}
def XR_MaterialShaderUrlOp : XR_ExpressionOp<"material_shader_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_EnvironmentUrlOp : XR_ExpressionOp<"environment_url", (ins), XR_String, [Pure]> {
  let hasVerifier = 1;
}
def XR_ImageDimensionOp : XR_ExpressionOp<"image_dimension", (ins XR_Image:$input), XR_Int, [Pure]> {
  let hasVerifier = 1;
}
def XR_NineSliceOp : XR_ExpressionOp<"nine_slice", (ins XR_Image:$input,
    XR_Int:$top, XR_Int:$right, XR_Int:$bottom, XR_Int:$left), XR_Image, [Pure]> {
  let hasVerifier = 1;
}
def XR_MarkdownOp : XR_ExpressionOp<"markdown", (ins), XR_StyledText, [Pure]>;
def XR_TranslateOp : XR_ExpressionOp<"translate",
    (ins Variadic<AnyTypeOf<[XR_String, XR_Int, XR_Float]>>:$args, StrArrayAttr:$arg_names), XR_String> {
  let hasVerifier = 1;
}
def XR_LocaleOp : XR_ExpressionOp<"locale", (ins), XR_String, [MemoryEffects<[MemRead]>]>;
def XR_FormatNumberOp : XR_ExpressionOp<"format_number",
    (ins AnyTypeOf<[XR_Int, XR_Float]>:$input, XR_Int:$decimals), XR_String, [MemoryEffects<[MemRead]>]>;
def XR_ParseNumberOp : XR_ExpressionOp<"parse_number",
    (ins XR_String:$input, XR_Float:$fallback), XR_Float, [MemoryEffects<[MemRead]>]>;
def XR_ConcatOp : XR_ExpressionOp<"concat", (ins Variadic<XR_String>:$parts), XR_String, [Pure]>;
def XR_ArrayOp : XR_ExpressionOp<"array", (ins Variadic<XR_String>:$items), XR_StringList, [Pure]>;
def XR_Hit3DArrayOp : XR_ExpressionOp<"hit3d_array", (ins Variadic<XR_Hit3D>:$items), XR_Hit3DList, [Pure]>;
def XR_PhysicsContact3DArrayOp : XR_ExpressionOp<"physics_contact3d_array",
    (ins Variadic<XR_PhysicsContact3D>:$items), XR_PhysicsContact3DList, [Pure]>;
def XR_PhysicsHit3DArrayOp : XR_ExpressionOp<"physics_hit3d_array",
    (ins Variadic<XR_PhysicsHit3D>:$items), XR_PhysicsHit3DList, [Pure]>;
def XR_IntArrayOp : XR_ExpressionOp<"int_array", (ins Variadic<XR_Int>:$items), XR_IntList, [Pure]>;
def XR_DateArrayOp : XR_ExpressionOp<"date_array", (ins Variadic<XR_Date>:$items), XR_DateList, [Pure]>;
def XR_PointerButtonsOp : XR_ExpressionOp<"pointer_buttons", (ins Variadic<XR_PointerButton>:$items), XR_PointerButtons, [Pure]>;
def XR_RecordArrayOp : XR_ExpressionOp<"record_array", (ins Variadic<XR_Record>:$items), XR_RecordList, [Pure]>;
def XR_MakeRecordOp : XR_ExpressionOp<"make_record", (ins Variadic<XR_Value>:$fields), XR_Record, [Pure]>;
def XR_UpdateRecordOp : XR_ExpressionOp<"update_record",
    (ins Variadic<XR_Value>:$overrides, XR_Record:$base, StrArrayAttr:$fields), XR_Record, [Pure]>;
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
def XR_DateIsValidOp : XR_ExpressionOp<"date_is_valid", (ins XR_Date:$input), XR_Bool, [Pure]>;
def XR_DateAddDaysOp : XR_ExpressionOp<"date_add_days",
    (ins XR_Date:$input, XR_Int:$days, XR_Date:$fallback), XR_Date, [Pure]>;
def XR_DateAddMonthsOp : XR_ExpressionOp<"date_add_months",
    (ins XR_Date:$input, XR_Int:$months, XR_Date:$fallback), XR_Date, [Pure]>;
def XR_DateWeekdayOp : XR_ExpressionOp<"date_weekday", (ins XR_Date:$input), XR_Int, [Pure]>;
def XR_DateMonthsOp : XR_ExpressionOp<"date_months", (ins XR_Date:$from, XR_Date:$to), XR_DateList, [Pure]>;
def XR_DateParseIsoOp : XR_ExpressionOp<"date_parse_iso", (ins XR_String:$text, XR_Date:$fallback), XR_Date, [Pure]>;
def XR_DateIsSelectableOp : XR_ExpressionOp<"date_is_selectable",
    (ins XR_Date:$date, XR_Date:$minimum, XR_Date:$maximum, XR_IntList:$disabledWeekdays, XR_DateList:$disabledDates),
    XR_Bool, [MemoryEffects<[MemRead]>]>;
def XR_DateMonthIndexOp : XR_ExpressionOp<"date_month_index",
    (ins XR_Date:$from, XR_Date:$to, XR_Date:$date), XR_Int, [Pure]>;
def XR_MakeTimeOp : XR_ExpressionOp<"make_time", (ins XR_Int:$hour, XR_Int:$minute, XR_Int:$second), XR_Time, [Pure]>;
def XR_TimeParseIsoOp : XR_ExpressionOp<"time_parse_iso", (ins XR_String:$text, XR_Time:$fallback), XR_Time, [Pure]>;
def XR_TimeIsValidIsoOp : XR_ExpressionOp<"time_is_valid_iso", (ins XR_String:$text), XR_Bool, [Pure]>;
def XR_TimeFormatIsoOp : XR_ExpressionOp<"time_format_iso", (ins XR_Time:$time, XR_Bool:$showSeconds), XR_String, [Pure]>;
def XR_TimeInRangeOp : XR_ExpressionOp<"time_in_range", (ins XR_Time:$time, XR_Time:$minimum, XR_Time:$maximum), XR_Bool, [Pure]>;
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
def XR_LoaderExternalStatusOp : XR_ExpressionOp<"loader_external_status", (ins XR_String:$source, StrAttr:$owner), XR_String, [MemoryEffects<[MemRead]>]>;
def XR_DynamicActionOp : XR_ExpressionOp<"dynamic_action", (ins Variadic<XR_Value>:$inputs, StrAttr:$owner), XR_Value, [MemoryEffects<[MemWrite]>]>;
def XR_DynamicReadOp : XR_ExpressionOp<"dynamic_read", (ins Variadic<XR_Value>:$inputs, StrAttr:$owner), XR_Value, [MemoryEffects<[MemRead]>]>;
def XR_LoaderMountedOp : XR_ExpressionOp<"loader_mounted", (ins XR_Int:$selection, StrAttr:$owner), XR_Bool, [MemoryEffects<[MemRead]>]>;
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
def XR_ListSelectionReadOp : XR_ExpressionOp<"list_selection_read", (ins StrAttr:$owner), XR_Int>;
def XR_TreeSelectionKeysOp : XR_ExpressionOp<"tree_selection_keys", (ins StrAttr:$owner), XR_IntList>;
def XR_ListSelectionKeysOp : XR_ExpressionOp<"list_selection_keys", (ins StrAttr:$owner), XR_IntList>;
def XR_ListRowSelectedOp : XR_ExpressionOp<"list_row_selected", (ins XR_Int:$index, StrAttr:$owner), XR_Bool>;
def XR_ViewCountOp : XR_ExpressionOp<"view_count", (ins StrAttr:$owner), XR_Int>;
def XR_ViewIndexOp : XR_ExpressionOp<"view_index", (ins XR_Int:$index, StrAttr:$owner), XR_Int> {
  let hasVerifier = 1;
}
def XR_StackReadOp : XR_ExpressionOp<"stack_read", (ins Optional<XR_String>:$route, StrAttr:$owner), AnyTypeOf<[XR_String, XR_Int, XR_Bool]>> {
  let hasVerifier = 1;
}
def XR_ImagePlaybackReadOp : XR_ExpressionOp<"image_playback_read", (ins StrAttr:$owner), XR_Int> {
  let hasVerifier = 1;
}
def XR_MediaReadOp : XR_ExpressionOp<"media_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Float, XR_Bool, XR_String, XR_Int]>> {
  let hasVerifier = 1;
}
def XR_TextAcceptsOp : XR_ExpressionOp<"text_accepts", (ins StrAttr:$owner), XR_Bool>;
def XR_TextValidityOp : XR_ExpressionOp<"text_validity", (ins StrAttr:$owner), XR_InputValidity>;
def XR_WindowReadOp : XR_ExpressionOp<"window_read", (ins), XR_Bool, [Pure]> {
  let hasVerifier = 1;
}
def XR_CheckGroupReadOp : XR_ExpressionOp<"check_group_read", (ins StrAttr:$owner), XR_CheckState>;
def XR_ActionGroupReadOp : XR_ExpressionOp<"action_group_read", (ins StrAttr:$owner), XR_String>;
def XR_MeshLoaderReadOp : XR_ExpressionOp<"mesh_loader_read", (ins StrAttr:$owner),
    AnyTypeOf<[XR_MeshAsset3D, XR_Bounds3D, XR_String]>, [MemoryEffects<[MemRead]>]>;

def XR_NativeDialogReadOp : XR_ExpressionOp<"native_dialog_read", (ins StrAttr:$owner),
                                           AnyTypeOf<[XR_Bool, XR_String, XR_StringList]>>;
def XR_FocusReadOp : XR_ExpressionOp<"focus_read", (ins StrAttr:$owner), XR_Bool, [Pure]> {
  let hasVerifier = 1;
}
def XR_TextSelectionReadOp : XR_ExpressionOp<"text_selection_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Int, XR_String]>>;
def XR_TextCommandAvailableOp : XR_ExpressionOp<"text_command_available", (ins StrAttr:$owner), XR_Bool>;
def XR_ControlReadOp : XR_ExpressionOp<"control_read", (ins StrAttr:$owner), XR_Value, [Pure]> {
  let hasVerifier = 1;
}
def XR_SceneViewQueryOp : XR_ExpressionOp<"scene_view_query",
    (ins Variadic<AnyTypeOf<[XR_Point2D, XR_Point3D, XR_Vector3D, XR_Float, XR_Int, XR_IntList]>>:$inputs,
         StrAttr:$owner),
    AnyTypeOf<[XR_ProjectedPoint3D, XR_Point3D, XR_String, XR_Hit3D, XR_Hit3DList, XR_IntList]>,
    [MemoryEffects<[MemRead]>]> {
  let hasVerifier = 1;
}
def XR_GeometryReadOp : XR_ExpressionOp<"geometry_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Length, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_ViewportReadOp : XR_ExpressionOp<"viewport_read", (ins StrAttr:$owner), XR_Length, [Pure]> {
  let hasVerifier = 1;
}
def XR_FlickMotionReadOp : XR_ExpressionOp<"flick_motion_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>>;
def XR_ScrollControlReadOp : XR_ExpressionOp<"scroll_control_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Float, XR_Bool]>>;
def XR_ScaleRotateReadOp : XR_ExpressionOp<"scale_rotate_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Float, XR_Length]>>;
def XR_SwipeReadOp : XR_ExpressionOp<"swipe_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>>;
def XR_HoverReadOp : XR_ExpressionOp<"hover_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Length]>> {
  let hasVerifier = 1;
}
def XR_TouchAreaReadOp : XR_ExpressionOp<"touch_area_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Int, XR_Length, XR_TouchPoints]>> {
  let hasVerifier = 1;
}
def XR_ButtonPressedOp : XR_ExpressionOp<"button_pressed", (ins StrAttr:$owner), XR_Bool>;
def XR_ButtonCheckedOp : XR_ExpressionOp<"button_checked", (ins StrAttr:$owner), XR_Bool>;
def XR_RadioCheckedOp : XR_ExpressionOp<"radio_checked", (ins StrAttr:$owner), XR_Bool>;
def XR_NavigationActiveOp : XR_ExpressionOp<"navigation_active", (ins StrAttr:$owner), XR_Bool>;
def XR_NavigationIndexOp : XR_ExpressionOp<"navigation_index", (ins StrAttr:$owner), XR_Int>;
def XR_NavigationCountOp : XR_ExpressionOp<"navigation_count", (ins StrAttr:$owner), XR_Int>;
def XR_ToggleStateReadOp : XR_ExpressionOp<"toggle_state_read", (ins StrAttr:$owner),
    AnyTypeOf<[XR_Bool, XR_CheckState]>>;
def XR_NumericValueReadOp : XR_ExpressionOp<"numeric_value_read", (ins StrAttr:$owner),
    AnyTypeOf<[XR_Int, XR_Float]>>;
def XR_ComboStateReadOp : XR_ExpressionOp<"combo_state_read", (ins StrAttr:$owner),
    AnyTypeOf<[XR_Bool, XR_Int, XR_Float, XR_String, XR_Date, XR_Time]>>;
def XR_RangePositionReadOp : XR_ExpressionOp<"range_position_read", (ins StrAttr:$owner), XR_Float>;
def XR_ComboPopupReadOp : XR_ExpressionOp<"combo_popup_read", (ins StrAttr:$owner), AnyTypeOf<[XR_Bool, XR_Int]>>;
def XR_ParamOp : XR_ExpressionOp<"param", (ins)>;
def XR_LoopParamOp : XR_ExpressionOp<"loop_param", (ins)> {
  let hasVerifier = 1;
}
def XR_CallOp : XR_ExpressionOp<"call", (ins Variadic<XR_Value>:$args, StrAttr:$owner)>;
def XR_UnaryOp : XR_ExpressionOp<"unary", (ins XR_Value:$input), XR_Value,
                                [AllTypesMatch<["input", "result"]>]> {
  let hasVerifier = 1;
}
def XR_CastOp : XR_ExpressionOp<"cast", (ins XR_Int:$input), XR_Float>;
def XR_MathOperand : AnyTypeOf<[XR_Int, XR_Float, XR_Length, XR_Percent]>;
def XR_MathFunctionOp : XR_ExpressionOp<"math_function", (ins Variadic<XR_MathOperand>:$inputs),
    AnyTypeOf<[XR_MathOperand, XR_Bool]>, [Pure]> { let hasVerifier = 1; }
def XR_GeometryValue : AnyTypeOf<[XR_Point2D, XR_Vector2D, XR_Transform2D, XR_Point3D, XR_Vector3D, XR_Quaternion3D, XR_RotationTrack3D, XR_Hit3D, XR_Instance3D, XR_Hit3DList, XR_ProjectedPoint3D, XR_WalkResult3D, XR_Bounds3D, XR_MeshNode3D]>;
def XR_GeometryFunctionOp : XR_ExpressionOp<"geometry_function",
    (ins Variadic<AnyTypeOf<[XR_GeometryValue, XR_Float, XR_Length, XR_Record, XR_RecordList, XR_SceneModel3D, XR_InstanceTable3D, XR_PhysicsWorld3D, XR_MeshAsset3D, XR_String, XR_Int, XR_IntList]>>:$inputs),
    AnyTypeOf<[XR_GeometryValue, XR_SceneModel3D, XR_InstanceTable3D, XR_String, XR_IntList]>,
    [DeclareOpInterfaceMethods<MemoryEffectsOpInterface>]> { let hasVerifier = 1; }
def XR_PhysicsFunctionOp : XR_ExpressionOp<"physics_function",
    (ins Variadic<AnyTypeOf<[XR_PhysicsWorld3D, XR_Point3D, XR_Vector3D, XR_Int, XR_Float, XR_Bool, XR_String, XR_RecordList]>>:$inputs),
    AnyTypeOf<[XR_PhysicsWorld3D, XR_Point3D, XR_Vector3D, XR_Int, XR_Float, XR_Bool, XR_IntList, XR_PhysicsContact3DList, XR_PhysicsHit3D, XR_PhysicsHit3DList]>,
    [DeclareOpInterfaceMethods<MemoryEffectsOpInterface>]> { let hasVerifier = 1; }
def XR_PhysicsContact3DFieldOp : XR_ExpressionOp<"physics_contact3d_field", (ins XR_PhysicsContact3D:$input),
    AnyTypeOf<[XR_Int, XR_Float, XR_Point3D, XR_Vector3D]>, [Pure]> { let hasVerifier = 1; }
def XR_PhysicsHit3DFieldOp : XR_ExpressionOp<"physics_hit3d_field", (ins XR_PhysicsHit3D:$input),
    AnyTypeOf<[XR_Int, XR_Float, XR_Point3D, XR_Vector3D, XR_Bool]>, [Pure]> { let hasVerifier = 1; }
def XR_Point2DFieldOp : XR_ExpressionOp<"point2d_field", (ins XR_Point2D:$input), XR_Length, [Pure]> {
  let hasVerifier = 1;
}
def XR_Point3DFieldOp : XR_ExpressionOp<"point3d_field", (ins XR_Point3D:$input), XR_Float, [Pure]> {
  let hasVerifier = 1;
}
def XR_Vector3DFieldOp : XR_ExpressionOp<"vector3d_field", (ins XR_Vector3D:$input), XR_Float, [Pure]> {
  let hasVerifier = 1;
}
def XR_Quaternion3DFieldOp : XR_ExpressionOp<"quaternion3d_field", (ins XR_Quaternion3D:$input), XR_Float, [Pure]> {
  let hasVerifier = 1;
}
def XR_RotationTrack3DFieldOp : XR_ExpressionOp<"rotation_track3d_field", (ins XR_RotationTrack3D:$input),
    AnyTypeOf<[XR_Int, XR_Bool, XR_Float]>, [Pure]> { let hasVerifier = 1; }
def XR_InstanceTable3DFieldOp : XR_ExpressionOp<"instance_table3d_field", (ins XR_InstanceTable3D:$input),
    AnyTypeOf<[XR_Int, XR_Bool]>, [Pure]> { let hasVerifier = 1; }
def XR_Instance3DFieldOp : XR_ExpressionOp<"instance3d_field", (ins XR_Instance3D:$input),
    AnyTypeOf<[XR_Point3D, XR_Vector3D, XR_Color, XR_Int, XR_Float, XR_Bool, XR_String]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_Hit3DFieldOp : XR_ExpressionOp<"hit3d_field", (ins XR_Hit3D:$input),
    AnyTypeOf<[XR_Point3D, XR_Vector3D, XR_Int, XR_Float, XR_Bool, XR_String]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_WalkResult3DOp : XR_ExpressionOp<"walk_result3d",
    (ins XR_Point3D:$position, XR_Hit3DList:$crossings), XR_WalkResult3D, [Pure]>;
def XR_WalkResult3DFieldOp : XR_ExpressionOp<"walk_result3d_field",
    (ins XR_WalkResult3D:$input), AnyTypeOf<[XR_Point3D, XR_Hit3DList, XR_Hit3D, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_Bounds3DFieldOp : XR_ExpressionOp<"bounds3d_field", (ins XR_Bounds3D:$input),
    AnyTypeOf<[XR_Point3D, XR_Vector3D, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_MeshNode3DFieldOp : XR_ExpressionOp<"mesh_node3d_field", (ins XR_MeshNode3D:$input),
    AnyTypeOf<[XR_Point3D, XR_Vector3D, XR_Bool, XR_String, XR_Int]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_MeshAsset3DFieldOp : XR_ExpressionOp<"mesh_asset3d_field", (ins XR_MeshAsset3D:$input),
    AnyTypeOf<[XR_Bounds3D, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_ProjectedPoint3DFieldOp : XR_ExpressionOp<"projected_point3d_field",
    (ins XR_ProjectedPoint3D:$input), AnyTypeOf<[XR_Point2D, XR_Float, XR_Bool]>, [Pure]> {
  let hasVerifier = 1;
}
def XR_Vector2DFieldOp : XR_ExpressionOp<"vector2d_field", (ins XR_Vector2D:$input), XR_Length, [Pure]> {
  let hasVerifier = 1;
}
def XR_StringFunctionOp : XR_ExpressionOp<"string_function", (ins Variadic<AnyTypeOf<[XR_String, XR_Int, XR_Float]>>:$inputs),
    AnyTypeOf<[XR_String, XR_Bool, XR_Int, XR_Float]>, [Pure]> { let hasVerifier = 1; }
def XR_ColorFunctionOp : XR_ExpressionOp<"color_function",
    (ins Variadic<AnyTypeOf<[XR_Int, XR_Float, XR_Color]>>:$inputs), XR_Color, [Pure]> {
  let hasVerifier = 1;
}
def XR_ModelFilterOp : XR_ExpressionOp<"model_filter", (ins XR_RecordList:$model, XR_String:$query),
    XR_RecordList, [Pure, AllTypesMatch<["model", "result"]>]>;
def XR_ModelLookupOp : XR_ExpressionOp<"model_lookup", (ins XR_RecordList:$model, XR_String:$key),
    XR_RecordList, [Pure, AllTypesMatch<["model", "result"]>]>;
def XR_ModelSortByOp : XR_ExpressionOp<"model_sort_by", (ins XR_RecordList:$model, XR_Bool:$descending),
    XR_RecordList, [Pure, AllTypesMatch<["model", "result"]>]>;
def XR_Model : AnyTypeOf<[XR_IntList, XR_DateList, XR_StringList, XR_RecordList, XR_Hit3DList, XR_PhysicsContact3DList, XR_PhysicsHit3DList]>;
def XR_ModelEditOp : XR_Op<"model_edit", [ParentOneOf<["CallbackOp", "HandlerOp", "IfOp", "ForOp"]>]> {
  let arguments = (ins XR_Model:$model, Variadic<XR_Value>:$args, StrAttr:$method);
  let hasVerifier = 1;
}
def XR_ModelCountOp : XR_ExpressionOp<"model_count", (ins XR_Model:$input), XR_Int, [MemoryEffects<[MemRead]>]>;
def XR_ModelIsEmptyOp : XR_ExpressionOp<"model_is_empty", (ins XR_Model:$input), XR_Bool, [MemoryEffects<[MemRead]>]>;
def XR_ModelAtOp : XR_ExpressionOp<"model_at",
    (ins XR_Model:$model, XR_Int:$index, AnyTypeOf<[XR_Int, XR_String, XR_Date, XR_Record, XR_Hit3D, XR_PhysicsContact3D, XR_PhysicsHit3D]>:$fallback),
    AnyTypeOf<[XR_Int, XR_String, XR_Date, XR_Record, XR_Hit3D, XR_PhysicsContact3D, XR_PhysicsHit3D]>, [MemoryEffects<[MemRead]>, AllTypesMatch<["fallback", "result"]>]> {
  let hasVerifier = 1;
}
def XR_FormatFixedOp : XR_ExpressionOp<"format_fixed",
    (ins AnyTypeOf<[XR_Int, XR_Float]>:$input), XR_String, [Pure]> { let hasVerifier = 1; }
def XR_RoundOp : XR_ExpressionOp<"round", (ins XR_Float:$input), XR_Int, [Pure]>;
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
def XR_BinaryOp : XR_ExpressionOp<"binary", (ins XR_Value:$lhs, XR_Value:$rhs)> {
  let hasVerifier = 1;
}
def XR_EnumConstructOp : XR_ExpressionOp<"enum_construct",
    (ins Variadic<XR_Value>:$payload, I64Attr:$tag, ArrayAttr:$fields), XR_Record, [Pure]>;
def XR_EnumMatchOp : XR_ExpressionOp<"enum_match", (ins XR_Record:$subject), XR_Value,
    [SingleBlockImplicitTerminator<"EndOp">, NoRegionArguments]> {
  let regions = (region SizedRegion<1>:$body);
  let hasVerifier = 1;
}
def XR_EnumCaseOp : XR_ScopeOp<"enum_case", [HasParent<"EnumMatchOp">]> {
  let arguments = (ins StrAttr:$name, I64Attr:$ordinal);
  let hasVerifier = 1;
}
def XR_EnumResultOp : XR_Op<"enum_result", [HasParent<"EnumCaseOp">]> {
  let arguments = (ins XR_Value:$input);
}
def XR_EnumParameterOp : XR_ExpressionOp<"enum_parameter", (ins)>;

def XR_MatchEnumOp : XR_ExpressionOp<"match_enum", (ins XR_Int:$tag, Variadic<XR_Value>:$arms), XR_Value, [Pure]> {
  let hasVerifier = 1;
}
def XR_SelectOp : XR_ExpressionOp<"select", (ins XR_Bool:$condition, XR_Value:$on_true, XR_Value:$on_false),
                                 XR_Value, [Pure, AllTypesMatch<["on_true", "on_false", "result"]>]> {
  let hasFolder = 1;
}
