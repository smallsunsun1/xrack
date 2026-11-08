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
def XR_LayoutAlignment : XR_Type<"LayoutAlignment", "layoutalignment">;
def XR_CrossAxisAlignment : XR_Type<"CrossAxisAlignment", "crossaxisalignment">;
def XR_Orientation : XR_Type<"Orientation", "orientation">;
def XR_DialInputMode : XR_Type<"DialInputMode", "dialinputmode">;
def XR_ScrollBarPolicy : XR_Type<"ScrollBarPolicy", "scrollbarpolicy">;
def XR_TextWrap : XR_Type<"TextWrap", "textwrap">;
def XR_DialogRole : XR_Type<"DialogRole", "dialogrole">;
def XR_DrawerEdge : XR_Type<"DrawerEdge", "draweredge">;
def XR_SelectionTarget : XR_Type<"SelectionTarget", "selectiontarget">;
def XR_Date : XR_Type<"Date", "date">;
def XR_GradientStop : XR_Type<"GradientStop", "gradientstop">;
def XR_Value : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_String, XR_StringList, XR_Record, XR_RecordList, XR_Length, XR_Percent, XR_Color,
                         XR_Brush, XR_Image, XR_ImageFit, XR_LayoutAlignment, XR_CrossAxisAlignment,
                         XR_Orientation, XR_DialInputMode, XR_ScrollBarPolicy, XR_TextWrap, XR_DialogRole, XR_DrawerEdge, XR_SelectionTarget, XR_Date]>;
def XR_Stringifiable : AnyTypeOf<[XR_Int, XR_Float, XR_Bool, XR_Date]>;

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
def XR_ElementOp : XR_ScopeOp<"element"> {
  let arguments = (ins StrAttr:$kind, StrAttr:$id, OptionalAttr<StrAttr>:$key);
  let hasVerifier = 1;
}
def XR_InstanceOp : XR_ScopeOp<"instance", [HasParent<"ElementOp">]> {
  let arguments = (ins FlatSymbolRefAttr:$component, StrAttr:$id, StrAttr:$key);
  let hasVerifier = 1;
}
def XR_CallbackOp : XR_ScopeOp<"callback", [HasParent<"ComponentOp">]> {
  let arguments = (ins StrAttr:$name, StrAttr:$parameters, OptionalAttr<StrAttr>:$pure);
  let hasVerifier = 1;
}
def XR_HandlerOp : XR_ScopeOp<"handler", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$name, StrAttr:$parameters);
  let hasVerifier = 1;
}

def XR_PropertyOp : XR_Op<"property"> {
  let arguments = (ins XR_Value:$initial, StrAttr:$name, StrAttr:$visibility);
}
def XR_BindOp : XR_Op<"bind"> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
}
def XR_ActionRefOp : XR_Op<"action_ref", [HasParent<"ElementOp">]> {
  let arguments = (ins StrAttr:$owner);
}
def XR_ShowIfOp : XR_Op<"show_if"> {
  let arguments = (ins XR_Bool:$input, StrAttr:$name);
}
def XR_AnimateOp : XR_Op<"animate", [HasParent<"ElementOp">]> {
  let arguments = (ins XR_Int:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_RepeatOp : XR_Op<"repeat", [ParentOneOf<["ElementOp", "InstanceOp"]>]> {
  let arguments = (ins AnyTypeOf<[XR_Int, XR_StringList, XR_RecordList]>:$input, StrAttr:$name,
                       StrAttr:$item, StrAttr:$index);
}
def XR_ControlLinkOp : XR_Op<"control_link", [HasParent<"ElementOp">]> {
  let arguments = (ins AnyTypeOf<[XR_String, XR_Bool, XR_Float, XR_Int, XR_Date]>:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_RowLinkOp : XR_Op<"row_link", [HasParent<"ElementOp">]> {
  let arguments = (ins AnyTypeOf<[XR_String, XR_Int, XR_Bool, XR_Float]>:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_LayoutBindOp : XR_Op<"layout_bind", [HasParent<"InstanceOp">]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
  let hasVerifier = 1;
}
def XR_AssignOp : XR_Op<"assign"> {
  let arguments = (ins XR_Value:$input, StrAttr:$name, OptionalAttr<StrAttr>:$owner);
}
def XR_ReturnOp : XR_Op<"return", [HasParent<"CallbackOp">]> {
  let arguments = (ins XR_Value:$input, StrAttr:$name);
}
def XR_InvokeOp : XR_Op<"invoke"> {
  let arguments = (ins Variadic<XR_Value>:$args, StrAttr:$owner, StrAttr:$name);
}
def XR_NavigateOp : XR_Op<"navigate"> {
  let arguments = (ins Variadic<AnyTypeOf<[XR_String, XR_StringList]>>:$args, StrAttr:$owner, StrAttr:$method);
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
def XR_ArrayOp : XR_ExpressionOp<"array", (ins Variadic<XR_String>:$items), XR_StringList, [Pure]>;
def XR_RecordArrayOp : XR_ExpressionOp<"record_array", (ins Variadic<XR_Record>:$items), XR_RecordList, [Pure]>;
def XR_MakeRecordOp : XR_ExpressionOp<"make_record", (ins Variadic<XR_Value>:$fields), XR_Record, [Pure]>;
def XR_DefaultRecordOp : XR_ExpressionOp<"default_record", (ins), XR_Record, [Pure]>;
def XR_GetFieldOp : XR_ExpressionOp<"get_field", (ins XR_Record:$input), XR_Value, [Pure]>;
def XR_MakeDateOp : XR_ExpressionOp<"make_date", (ins XR_Int:$year, XR_Int:$month, XR_Int:$day), XR_Date, [Pure]>;
def XR_DateFieldOp : XR_ExpressionOp<"date_field", (ins XR_Date:$input), XR_Int, [Pure]> {
  let hasVerifier = 1;
}
def XR_ReadOp : XR_ExpressionOp<"read", (ins OptionalAttr<StrAttr>:$owner)>;
def XR_StackReadOp : XR_ExpressionOp<"stack_read", (ins StrAttr:$owner), AnyTypeOf<[XR_String, XR_Int, XR_Bool]>> {
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
