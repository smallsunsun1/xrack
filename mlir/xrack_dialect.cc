#include "mlir/xrack_dialect.h"

#include <utility>

#include "mlir/IR/Builders.h"
#include "mlir/IR/DialectImplementation.h"
#include "mlir/IR/OpImplementation.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/ADT/StringMap.h"
#include "llvm/ADT/StringSet.h"
#include "llvm/ADT/StringSwitch.h"
#include "llvm/ADT/TypeSwitch.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/raw_ostream.h"
#include <algorithm>
#include <cmath>
#include <optional>

#include "mlir/XrackDialect.cpp.inc"

#define GET_TYPEDEF_CLASSES
#include "mlir/XrackTypes.cpp.inc"

#define GET_OP_CLASSES
#include "mlir/XrackOps.cpp.inc"

namespace xrack::dialect {
using namespace ::mlir;

void XrackDialect::initialize() {
  addTypes<
#define GET_TYPEDEF_LIST
#include "mlir/XrackTypes.cpp.inc"
      >();
  addOperations<
#define GET_OP_LIST
#include "mlir/XrackOps.cpp.inc"
      >();
}

bool isExpression(Operation &op) {
  return isa<LiteralOp, ReadOp, DynamicActionOp, DynamicReadOp, LoaderMountedOp, LoaderExternalStatusOp, ControlReadOp, GeometryReadOp, StackReadOp, TreeQueryOp, TreeSelectionKeysOp, ListSelectionKeysOp, ViewIndexOp, ViewCountOp, ListSelectionReadOp, TableSortReadOp, TableSelectionReadOp, TableHistoryReadOp, SplitStateReadOp, SplitResizingOp, ImagePlaybackReadOp, MediaReadOp, TextAcceptsOp, TextValidityOp, WindowReadOp, CheckGroupReadOp, ActionGroupReadOp, NativeDialogReadOp, FocusReadOp, TextSelectionReadOp, TextCommandAvailableOp, ViewportReadOp, FlickMotionReadOp, ScrollControlReadOp, ScaleRotateReadOp, SwipeReadOp, HoverReadOp, TouchAreaReadOp, ButtonPressedOp, ButtonCheckedOp, RadioCheckedOp, NavigationActiveOp, NavigationIndexOp, NavigationCountOp, ToggleStateReadOp, NumericValueReadOp, ComboStateReadOp, ComboPopupReadOp, RangePositionReadOp, ParamOp, UnaryOp, CastOp,
             ListRowSelectedOp, StringifyOp, BinaryOp, ConcatOp, SelectOp, SolidBrushOp, GradientStopOp,
             LinearGradientOp, RadialGradientOp, ImageUrlOp, MeshUrlOp, MeshLodUrlOp, MeshLightmapUrlOp, MeshLodLightmapUrlOp, TextureUrlOp, ShaderUrlOp, MaterialShaderUrlOp, EnvironmentUrlOp, ImageDimensionOp, NineSliceOp, MarkdownOp, TranslateOp, LocaleOp, FormatNumberOp, ParseNumberOp, ArrayOp, IntArrayOp, DateArrayOp, Hit3DArrayOp, PointerButtonsOp, CallOp,
             RoundOp, MathFunctionOp, GeometryFunctionOp, Point2DFieldOp, Point3DFieldOp,
             ProjectedPoint3DFieldOp, Hit3DFieldOp, Vector3DFieldOp, Vector2DFieldOp,
             StringFunctionOp, ColorFunctionOp, ModelCountOp, ModelIsEmptyOp, ModelAtOp, FormatFixedOp, ToRatioOp, MakeRecordOp, UpdateRecordOp, DefaultRecordOp, RecordArrayOp,
             GetFieldOp, LoopParamOp, MatchEnumOp, EnumConstructOp, EnumMatchOp, EnumParameterOp, ModelFilterOp, ModelLookupOp, ModelSortByOp, KeyFieldOp, WheelFieldOp, PointerFieldOp, TouchPointFieldOp, MakeDateOp, DateFieldOp,
             DateIsValidOp, DateAddDaysOp, DateAddMonthsOp, DateWeekdayOp, DateMonthsOp, DateMonthIndexOp, DateParseIsoOp, DateIsSelectableOp,
             MakeTimeOp, TimeFieldOp, TimeParseIsoOp, TimeIsValidIsoOp, TimeFormatIsoOp, TimeInRangeOp, MakeFontOp, FontFieldOp>(op);
}

LogicalResult MeshUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty mesh path");
  return success();
}

LogicalResult MeshLodUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty mesh path");
  return success();
}

LogicalResult MeshLightmapUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty mesh path");
  return success();
}

LogicalResult MeshLodLightmapUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty mesh path");
  return success();
}

LogicalResult TextureUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty texture path");
  return success();
}

LogicalResult ShaderUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty shader path");
  return success();
}

LogicalResult MaterialShaderUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty material shader path");
  return success();
}

LogicalResult EnvironmentUrlOp::verify() {
  if (getValue().empty())
    return emitOpError("requires a nonempty environment path");
  return success();
}

LogicalResult TranslateOp::verify() {
  if (getValue().empty() || getArgNames().size() != getArgs().size())
    return emitOpError("requires a message ID and a name for each operand");
  llvm::StringSet<> names;
  for (auto attr : getArgNames()) {
    auto name = cast<StringAttr>(attr).getValue();
    if (name.empty() || !names.insert(name).second)
      return emitOpError("translation argument names must be nonempty and unique");
  }
  return success();
}

static bool geometryProperty(llvm::StringRef name) {
  return llvm::StringSwitch<bool>(name)
      .Cases({"layout-x", "layout-y", "layout-width", "layout-height", "layout-ready",
              "window-bounds-x", "window-bounds-y", "window-bounds-width", "window-bounds-height",
              "available-width", "available-height"}, true)
      .Default(false);
}

static bool isPane(llvm::StringRef kind) {
  return kind == "Pane" || kind == "ControlFrame";
}

static bool isTextSubmissionRule(llvm::StringRef kind, llvm::StringRef name) {
  bool singleLine = kind == "TextInput" || kind == "LineEdit";
  return ((singleLine || kind == "TextEdit" || kind == "TextArea") &&
          (name == "minimum-length" || name == "submit-when")) ||
         (singleLine && (name == "minimum-value" || name == "maximum-value" ||
                         name == "decimals"));
}

static bool isTextEditor(llvm::StringRef kind) {
  return llvm::StringSwitch<bool>(kind)
      .Cases({"TextInput", "LineEdit", "TextEdit", "TextArea", "SearchField", "ComboBox"}, true)
      .Default(false);
}

static bool validTextEditorAppearance(llvm::StringRef kind,
                                      llvm::StringRef name, Type type) {
  if (name == "placeholder-text-color")
    return isTextEditor(kind) && isa<ColorType>(type);
  if (isTextEditor(kind) &&
      llvm::StringSwitch<bool>(name)
          .Cases({"padding", "padding-left", "padding-right", "padding-top",
                  "padding-bottom"}, true)
          .Default(false))
    return isa<LengthType>(type);
  return true;
}

static bool isTextCommandProperty(llvm::StringRef kind, llvm::StringRef name) {
  if (kind == "StyledText")
    return name == "can-copy";
  return isTextEditor(kind) &&
         llvm::StringSwitch<bool>(name)
             .Cases({"can-undo", "can-redo", "can-copy", "can-cut"}, true)
             .Default(false);
}

static bool isTextSelectionProperty(llvm::StringRef kind, llvm::StringRef name) {
  return (isTextEditor(kind) || kind == "StyledText") &&
         llvm::StringSwitch<bool>(name)
             .Cases({"cursor-offset", "selection-start-offset", "selection-end-offset", "selected-text"}, true)
             .Default(false);
}

static bool isWindow(llvm::StringRef kind) {
  return kind == "Window" || kind == "ApplicationWindow";
}

static bool validApplicationWindowBinding(llvm::StringRef kind, llvm::StringRef name, Type type) {
  return kind != "ApplicationWindow" ||
      !llvm::StringSwitch<bool>(name)
          .Cases({"padding", "padding-left", "padding-right", "padding-top", "padding-bottom", "spacing"}, true)
          .Default(false) ||
      isa<LengthType>(type);
}

static bool isPushButton(llvm::StringRef kind) {
  return kind == "Button" || kind == "ToolButton" || kind == "RoundButton";
}

static bool validButtonGeometryBinding(llvm::StringRef kind, llvm::StringRef name, Type type) {
  if (name == "radius")
    return kind == "RoundButton" && isa<LengthType>(type);
  if (kind != "ToolSeparator")
    return true;
  if (name == "orientation")
    return isa<OrientationType>(type);
  if (name == "stroke-width" || name == "padding")
    return isa<LengthType>(type);
  if (name == "color")
    return isa<ColorType>(type);
  return true;
}

static bool validPaneBinding(llvm::StringRef kind, llvm::StringRef name, Type type) {
  if (!isPane(kind))
    return true;
  if (name == "background" || name == "border-color")
    return isa<BrushType, ColorType>(type);
  if (name == "enabled")
    return isa<BoolType>(type);
  return !llvm::StringSwitch<bool>(name)
              .Cases({"content-width", "content-height", "padding", "padding-left", "padding-right",
                      "padding-top", "padding-bottom", "border-width", "border-radius",
                      "border-top-left-radius", "border-top-right-radius",
                      "border-bottom-left-radius", "border-bottom-right-radius"}, true)
              .Default(false) ||
         isa<LengthType>(type);
}

static bool windowStatusProperty(llvm::StringRef name) {
  return llvm::StringSwitch<bool>(name)
      .Cases({"window-active", "minimized", "minimized-known", "maximized", "full-screen"}, true)
      .Default(false);
}

LogicalResult GeometryReadOp::verify() {
  auto type = getResult().getType();
  if (geometryProperty(getValue()) &&
      (getValue() == "layout-ready" ? isa<BoolType>(type) : isa<LengthType>(type)))
    return success();
  return emitOpError("requires a typed layout geometry property");
}

LogicalResult PropertyOp::verify() {
  if (!getAlias())
    return success();
  if (getAlias().value_or("") != "true" ||
      (!getInitial().getDefiningOp<ReadOp>() && !getInitial().getDefiningOp<ControlReadOp>()))
    return emitOpError("property alias requires a writable property reference");
  return success();
}

LogicalResult ControlReadOp::verify() {
  for (Operation *user : getResult().getUsers()) {
    auto alias = dyn_cast<PropertyOp>(user);
    if (!alias || alias.getAlias().value_or("") != "true")
      return emitOpError("is only valid as the target of a property alias");
  }
  return success();
}

LogicalResult ImageDimensionOp::verify() {
  return getValue() == "width" || getValue() == "height"
      ? success() : emitOpError("image dimension must be width or height");
}

LogicalResult NineSliceOp::verify() {
  auto image = getInput().getDefiningOp<ImageUrlOp>();
  if (!image || image.getValue().empty())
    return emitOpError("nine-slice requires a nonempty image URL");
  for (Value edge : {getTop(), getRight(), getBottom(), getLeft()}) {
    auto literal = edge.getDefiningOp<LiteralOp>();
    uint32_t pixels;
    if (!literal || literal.getValue().getAsInteger(10, pixels))
      return emitOpError("nine-slice edges must be nonnegative integer literals");
  }
  return success();
}

LogicalResult KeyFieldOp::verify() {
  auto field = getValue();
  auto input = getInput().getType();
  auto result = getResult().getType();
  bool valid = isa<KeyEventType>(input)
      ? ((field == "key" || field == "text") && isa<StringType>(result)) ||
        (field == "repeat" && isa<BoolType>(result)) ||
        (field == "modifiers" && isa<KeyModifiersType>(result))
      : llvm::StringSwitch<bool>(field).Cases({"control", "shift", "alt", "meta", "primary"}, true).Default(false)
        && isa<BoolType>(result);
  return valid ? success() : emitOpError("incorrect key event field or type");
}

LogicalResult HoverReadOp::verify() {
  auto property = getValue();
  auto type = getResult().getType();
  if ((property == "hovered" && isa<BoolType>(type)) ||
      ((property == "hover-x" || property == "hover-y") && isa<LengthType>(type)))
    return success();
  return emitOpError("incorrect hover metric or type");
}

static Type touchAreaType(MLIRContext *context, StringRef property) {
  if (property == "touch-points")
    return TouchPointsType::get(context);
  if (property == "active-touch-count")
    return IntType::get(context);
  if (property == "pressed" || property == "contains-press" || property == "has-hover")
    return BoolType::get(context);
  if (property == "mouse-x" || property == "mouse-y" || property == "pressed-x" ||
      property == "pressed-y")
    return LengthType::get(context);
  return {};
}

LogicalResult TouchAreaReadOp::verify() {
  if (touchAreaType(getContext(), getValue()) == getResult().getType())
    return success();
  return emitOpError("incorrect TouchArea state property or type");
}

LogicalResult WheelFieldOp::verify() {
  auto field = getValue();
  auto type = getResult().getType();
  if (((field == "x" || field == "y" || field == "delta-x" || field == "delta-y") &&
       isa<LengthType>(type)) ||
      ((field == "lines-x" || field == "lines-y") && isa<FloatType>(type)) ||
      (field == "is-line" && isa<BoolType>(type)) ||
      (field == "modifiers" && isa<KeyModifiersType>(type)))
    return success();
  return emitOpError("incorrect wheel event field or type");
}

LogicalResult PointerFieldOp::verify() {
  auto field = getValue();
  auto type = getResult().getType();
  if (((field == "x" || field == "y") && isa<LengthType>(type)) ||
      ((field == "inside" || field == "is-touch") && isa<BoolType>(type)) ||
      (field == "kind" && isa<PointerEventKindType>(type)) ||
      (field == "button" && isa<PointerButtonType>(type)) ||
      (field == "touch-finger-id" && isa<IntType>(type)) ||
      (field == "modifiers" && isa<KeyModifiersType>(type)))
    return success();
  return emitOpError("incorrect pointer event field or type");
}

LogicalResult TouchPointFieldOp::verify() {
  auto field = getValue();
  auto type = getResult().getType();
  if (((field == "x" || field == "y" || field == "start-x" || field == "start-y") &&
       isa<LengthType>(type)) || (field == "id" && isa<IntType>(type)) ||
      (field == "inside" && isa<BoolType>(type)))
    return success();
  return emitOpError("incorrect touch point field or type");
}

LogicalResult DateFieldOp::verify() {
  auto field = getValue();
  return field == "year" || field == "month" || field == "day"
             ? success()
             : emitOpError("requires year, month, or day");
}

LogicalResult TimeFieldOp::verify() {
  auto field = getValue();
  return field == "hour" || field == "minute" || field == "second"
             ? success()
             : emitOpError("requires hour, minute, or second");
}

LogicalResult FontFieldOp::verify() {
  auto result = getResult().getType();
  bool valid = llvm::StringSwitch<bool>(getValue())
      .Case("family", isa<StringType>(result))
      .Case("size", isa<LengthType>(result))
      .Case("weight", isa<IntType>(result))
      .Case("italic", isa<BoolType>(result))
      .Default(false);
  return valid ? success() : emitOpError("incorrect font field or type");
}

static bool usesListTemplate(llvm::StringRef kind) {
  return kind == "ListView" || kind == "GridView";
}

static bool isSwipeSlot(llvm::StringRef kind) {
  return kind == "SwipeContent" || kind == "SwipeLeft" || kind == "SwipeRight";
}

static bool isTableView(llvm::StringRef kind) {
  return kind == "TableView" || kind == "StandardTableView";
}

static bool isSelectionList(llvm::StringRef kind) {
  return usesListTemplate(kind) || kind == "StandardListView";
}

static bool isRowSelectionView(llvm::StringRef kind) {
  return isSelectionList(kind) || kind == "TreeView";
}

static bool isRowKeyView(llvm::StringRef kind) {
  return isSelectionList(kind) || isTableView(kind);
}

static bool isKeyedRowView(ElementOp view) {
  return view && isRowKeyView(view.getKind()) &&
      llvm::any_of(view.getBody().front().getOps<BindOp>(), [](BindOp binding) {
        return binding.getName() == "row-key-field";
      });
}

static bool isSelectedIndexProperty(llvm::StringRef kind, llvm::StringRef name) {
  return isSelectionList(kind) && name == "selected-view-index";
}

static bool isVirtualView(llvm::StringRef kind) {
  return usesListTemplate(kind) || kind == "StandardListView" ||
         isTableView(kind) || kind == "TreeView";
}

static bool isContentViewport(llvm::StringRef kind) {
  return kind == "ScrollView" || kind == "Flickable";
}

static bool isViewCountProperty(llvm::StringRef kind, llvm::StringRef name) {
  return isVirtualView(kind) && (name == "row-count" || name == "source-row-count");
}

static bool isViewport(llvm::StringRef kind) {
  return isVirtualView(kind) || isContentViewport(kind);
}

static bool isScrollControl(llvm::StringRef kind) {
  return kind == "ScrollBar" || kind == "ScrollIndicator";
}

static bool isScrollThumbMetric(llvm::StringRef name) {
  return name == "visual-position" || name == "visual-size";
}

static bool isScrollStateMetric(llvm::StringRef kind, llvm::StringRef name) {
  return (isScrollControl(kind) && name == "active") ||
         (kind == "ScrollBar" && (name == "pressed" || name == "hovered"));
}

static Type scaleRotateType(MLIRContext *context, llvm::StringRef name) {
  return llvm::StringSwitch<Type>(name)
      .Case("active", BoolType::get(context))
      .Cases({"scale", "rotation"}, FloatType::get(context))
      .Cases({"center-x", "center-y"}, LengthType::get(context))
      .Default({});
}

static Type swipeType(MLIRContext *context, llvm::StringRef name) {
  return llvm::StringSwitch<Type>(name)
      .Case("swiping", BoolType::get(context))
      .Cases({"pressed-x", "pressed-y", "current-x", "current-y"}, LengthType::get(context))
      .Default({});
}

static bool isScrollBarExtent(llvm::StringRef name) {
  return name == "effective-scroll-bar-width" || name == "effective-scroll-bar-height";
}

static bool isViewportProperty(llvm::StringRef name) {
  return name == "content-x" || name == "content-y" ||
         name == "visible-width" || name == "visible-height" || isScrollBarExtent(name);
}

static bool isViewportOffset(llvm::StringRef kind, llvm::StringRef name) {
  return (name == "content-y" && isViewport(kind)) ||
         (name == "content-x" && (isContentViewport(kind) || isTableView(kind)));
}

static bool isStaticViewport(ElementOp target) {
  for (auto current = target; current; current = current->getParentOfType<ElementOp>())
    if (!current.getBody().front().getOps<RepeatOp>().empty())
      return false;
  return true;
}

static ReadOp writableNavigationModelRead(ElementOp target) {
  auto kind = target.getKind();
  auto templateKind = kind == "TabBar" ? "TabButton"
      : kind == "TabWidget" ? "Tab" : kind == "SwipeView" ? "SwipePage" : "";
  if (llvm::StringRef(templateKind).empty())
    return {};
  for (auto item : target.getBody().front().getOps<ElementOp>()) {
    if (item.getKind() != templateKind)
      continue;
    auto repeats = item.getBody().front().getOps<RepeatOp>();
    if (!llvm::hasSingleElement(repeats))
      continue;
    auto source = (*repeats.begin()).getInput();
    if (isa<IntListType, DateListType, StringListType, RecordListType, Hit3DListType>(source.getType()))
      return source.getDefiningOp<ReadOp>();
  }
  return {};
}

static bool isTimePicker(llvm::StringRef kind) {
  return kind == "TimePicker" || kind == "TimePickerPopup";
}

static bool isPickerPopup(llvm::StringRef kind) {
  return kind == "TimePickerPopup" || kind == "DatePickerPopup" ||
         kind == "ColorPickerPopup" || kind == "FontPickerPopup";
}

static bool isWindowOverlay(llvm::StringRef kind) {
  return kind == "Dialog" || kind == "Drawer" || kind == "PopupWindow" || isPickerPopup(kind);
}

static bool isNativeDialog(llvm::StringRef kind) {
  return kind == "FileDialog" || kind == "MessageDialog";
}

static llvm::StringRef comboValueTypeName(Type type) {
  return llvm::TypeSwitch<Type, llvm::StringRef>(type)
      .Case<StringType>([](auto) { return "string"; })
      .Case<IntType>([](auto) { return "int"; })
      .Case<FloatType>([](auto) { return "float"; })
      .Case<BoolType>([](auto) { return "bool"; })
      .Case<DateType>([](auto) { return "Date"; })
      .Case<TimeType>([](auto) { return "Time"; })
      .Default(llvm::StringRef{});
}

static LogicalResult verifyStackRoute(Operation *op, ElementOp view,
                                      Value input,
                                      llvm::StringSet<> *seen = nullptr) {
  auto literal = input.getDefiningOp<LiteralOp>();
  if (!literal)
    return success();
  auto parsed = llvm::json::parse(literal.getValue());
  if (!parsed) {
    llvm::consumeError(parsed.takeError());
    return op->emitOpError("invalid StackPage route literal");
  }
  auto route = parsed->getAsString();
  if (route)
    for (auto page : view.getBody().front().getOps<ElementOp>())
      if (page.getKind() == "StackPage" && page.getId() == *route) {
        if (seen && !seen->insert(*route).second)
          return op->emitOpError("duplicate StackPage route ") << *route;
        return success();
      }
  return op->emitOpError("unknown StackPage route ") << literal.getValue();
}

static bool staticPropertyOwner(Operation *op);

static bool isNavigationGroup(llvm::StringRef kind) {
  return kind == "TabBar" || kind == "TabWidget" || kind == "SwipeView" || kind == "StackLayout";
}

static bool hasNavigationValue(ElementOp element) {
  return element && isNavigationGroup(element.getKind()) && !element.getBody().empty() &&
         llvm::any_of(element.getBody().front(), [](Operation &child) {
           auto name = child.getAttrOfType<StringAttr>("name");
           return isa<BindOp, ControlLinkOp>(child) && name && name.getValue() == "current-value";
         });
}

static ElementOp enclosingVisualTemplate(Operation *op) {
  for (auto element = op->getParentOfType<ElementOp>(); element;
       element = element->getParentOfType<ElementOp>())
    if (element.getKind() == "DayCell" || element.getKind() == "SplitHandle" ||
        element.getKind() == "TreeDisclosure" || element.getKind() == "SectionHeader" ||
        element.getKind() == "ComboOption" || element.getKind() == "TumblerOption" ||
        element.getKind() == "PathViewOption")
      return element;
  return {};
}

static bool isVirtualItemDelegate(ElementOp element) {
  auto view = dyn_cast<ElementOp>(element->getParentOp());
  return element.getDelegate() && view &&
         (view.getKind() == "ListView" || view.getKind() == "GridView") &&
         !element.getBody().front().getOps<RepeatOp>().empty();
}

static bool isVirtualSwitchPart(llvm::StringRef owner, ElementOp part) {
  if (owner != "Switch" ||
      (part.getKind() != "ToggleIndicator" && part.getKind() != "SwitchThumb"))
    return false;
  auto &body = part.getBody().front();
  return part.getId().empty() && body.getOps<ElementOp>().empty() &&
         body.getOps<InstanceOp>().empty() && body.getOps<PropertyOp>().empty() &&
         body.getOps<CallbackOp>().empty() && body.getOps<HandlerOp>().empty() &&
         body.getOps<ControlLinkOp>().empty() && body.getOps<RowLinkOp>().empty();
}

static bool isTypedRowEditor(llvm::StringRef kind) {
  return kind == "SpinBox" || kind == "DoubleSpinBox" || kind == "Slider" || kind == "Dial" ||
         kind == "CheckBox" || kind == "Switch";
}

static ElementOp enclosingTreeRow(ElementOp element) {
  for (auto parent = dyn_cast<ElementOp>(element->getParentOp()); parent;
       parent = dyn_cast<ElementOp>(parent->getParentOp()))
    if (parent.getKind() == "TreeRow")
      return parent;
  return {};
}

static bool hasStaticEditorPath(ElementOp element, ElementOp row) {
  for (; element != row; element = cast<ElementOp>(element->getParentOp())) {
    if (element.getKind() == "Tooltip" || element.getBody().empty() ||
        !element.getBody().front().getOps<RepeatOp>().empty())
      return false;
  }
  return true;
}

static Value recordFieldBase(Value value) {
  while (auto field = value.getDefiningOp<GetFieldOp>())
    value = field.getInput();
  return value;
}

static bool validTreeRoles(ElementOp row) {
  auto &body = row.getBody().front();
  auto repeats = body.getOps<RepeatOp>();
  if (!llvm::hasSingleElement(repeats) || !body.getOps<ShowIfOp>().empty())
    return false;
  auto repeat = *repeats.begin();
  auto model = dyn_cast<RecordListType>(repeat.getInput().getType());
  if (!model)
    return false;
  for (auto name : {"key", "parent-key"}) {
    unsigned count = 0;
    for (auto binding : body.getOps<BindOp>()) {
      if (binding.getName() != name)
        continue;
      ++count;
      auto input = binding.getInput();
      auto base = recordFieldBase(input);
      if (base == input || !isa<IntType>(input.getType()))
        return false;
      auto param = base.getDefiningOp<ParamOp>();
      auto record = dyn_cast<RecordType>(base.getType());
      if (!param || !record || record.getName() != model.getName() ||
          param.getValue() != repeat.getItem())
        return false;
    }
    if (count != 1)
      return false;
  }
  return true;
}

static bool forbiddenInTooltip(llvm::StringRef kind) {
  return isVirtualView(kind) || kind == "Tooltip" || isWindowOverlay(kind) ||
         kind == "Menu" || kind == "MenuBar";
}

LogicalResult ComponentOp::verify() {
  if (getExported() != "true" && getExported() != "false")
    return emitOpError("exported must be true or false");
  if (getGlobal() && *getGlobal() != "true")
    return emitOpError("global must be true when present");
  unsigned roots = 0;
  for (Operation &child : getBody().front()) {
    if (auto root = dyn_cast<ElementOp>(child)) {
      if (getGlobal() &&
          (root.getKind() != "Rectangle" || !llvm::hasSingleElement(root.getBody()) ||
           !llvm::hasSingleElement(root.getBody().front()) ||
           !isa<EndOp>(root.getBody().front().front())))
        return child.emitOpError("global requires an empty Rectangle root");
      ++roots;
    } else if (!isa<PropertyOp, CallbackOp, EndOp>(child) && !isExpression(child))
      return child.emitOpError("is not valid in a component body");
  }
  if (roots != 1)
    return emitOpError("requires exactly one root xr.element");
  return success();
}

static bool focusTargetKind(llvm::StringRef kind);

static bool validCompositionBinding(llvm::StringRef kind, llvm::StringRef name,
                                    Type type) {
  if (name != "z" && name != "opacity" && name != "cache-rendering-hint")
    return true;
  return (name == "cache-rendering-hint" ? isa<BoolType>(type)
                                         : isa<FloatType>(type)) &&
      !llvm::StringSwitch<bool>(kind)
          .Cases({"Action", "ActionGroup", "ButtonGroup", "Timer", "SoundEffect",
                  "FileDialog", "MessageDialog",
                  "SecondaryWindow", "SystemTrayIcon", "Shortcut", "MenuItem",
                  "MenuSeparator", "TableColumn"}, true)
          .Default(false);
}

static bool validTransformBinding(llvm::StringRef kind, llvm::StringRef name,
                                  Type type) {
  if (name != "transform" && !name.starts_with("transform-"))
    return true;
  if (llvm::StringSwitch<bool>(kind)
          .Cases({"Action", "Timer", "FileDialog", "MessageDialog", "Shortcut",
                  "Window", "ApplicationWindow", "Dialog", "Drawer", "PopupWindow", "SecondaryWindow",
                  "TimePickerPopup", "DatePickerPopup", "ColorPickerPopup", "FontPickerPopup",
                  "Menu", "MenuItem", "MenuSeparator", "Tooltip", "Tab", "TabButton",
                  "TableColumn"}, true)
          .Default(false))
    return false;
  if (name == "transform")
    return isa<Transform2DType>(type);
  if (name == "transform-origin-x" || name == "transform-origin-y")
    return isa<LengthType, PercentType>(type);
  return (name == "transform-rotation" || name == "transform-scale" ||
          name == "transform-scale-x" || name == "transform-scale-y") &&
      isa<FloatType>(type);
}

static bool validBoxShadowBinding(llvm::StringRef kind, llvm::StringRef name,
                                  Type type) {
  if (name == "drop-shadow-color" || name == "inner-shadow-color")
    return kind == "Rectangle" && isa<ColorType>(type);
  if (llvm::StringSwitch<bool>(name)
          .Cases({"drop-shadow-blur", "drop-shadow-offset-x", "drop-shadow-offset-y",
                  "drop-shadow-spread", "inner-shadow-blur", "inner-shadow-offset-x",
                  "inner-shadow-offset-y", "inner-shadow-spread"}, true)
          .Default(false))
    return kind == "Rectangle" && isa<LengthType>(type);
  return true;
}

static bool validImageBinding(llvm::StringRef kind, llvm::StringRef name, Type type) {
  bool image = kind == "Image" || kind == "AnimatedImage";
  if (name == "horizontal-tile-mode" || name == "vertical-tile-mode")
    return image && isa<ImageTileModeType>(type);
  if (name == "image-rendering")
    return image && isa<ImageRenderingType>(type);
  if (name == "colorize")
    return image && isa<BrushType>(type);
  if (name == "source-clip-x" || name == "source-clip-y" ||
      name == "source-clip-width" || name == "source-clip-height")
    return image && isa<IntType>(type);
  if (!image)
    return true;
  return (name != "source" || isa<ImageType>(type)) &&
      (name != "image-fit" || isa<ImageFitType>(type)) &&
      (name != "horizontal-alignment" || isa<ImageHorizontalAlignmentType>(type)) &&
      (name != "vertical-alignment" || isa<ImageVerticalAlignmentType>(type));
}

static bool nonExclusiveGroup(ElementOp element) {
  if (!element)
    return false;
  for (auto bind : element.getBody().front().getOps<BindOp>())
    if (bind.getName() == "exclusive")
      if (auto literal = bind.getInput().getDefiningOp<LiteralOp>())
        return isa<BoolType>(literal.getResult().getType()) && literal.getValue() == "false";
  return false;
}

static bool nonExclusiveButtonGroup(ElementOp element) {
  return element && element.getKind() == "ButtonGroup" && nonExclusiveGroup(element);
}

LogicalResult ElementOp::verify() {
  const bool root = isa<ComponentOp>((*this)->getParentOp());
  if (!root && !isa<ElementOp>((*this)->getParentOp()))
    return emitOpError("requires an xr.component or xr.element parent");
  auto kind = getKind();
  if (auto storage = getToggleStorage(); storage && (!isPushButton(kind) || *storage != "true"))
    return emitOpError("toggle storage must be true on a push button");
  auto parent = dyn_cast<ElementOp>((*this)->getParentOp());
  auto parentKind = parent ? parent.getKind() : llvm::StringRef{};
  auto &body = getBody().front();
  if (kind == "StackLayout" && !isStaticViewport(*this))
    return emitOpError("StackLayout in a repeated scope requires a component delegate");
  auto partParents = [](llvm::StringRef part) {
    return llvm::StringSwitch<llvm::StringRef>(part)
        .Case("ButtonContent", "Button, ToolButton, RoundButton, DelayButton or TabButton")
        .Case("EmptyState", "virtual view")
        .Case("SliderHandle", "Slider")
        .Cases({"RangeLowerHandle", "RangeUpperHandle"}, "RangeSlider")
        .Cases({"ProgressTrack", "ProgressFill"}, "ProgressIndicator")
        .Cases({"ScrollTrack", "ScrollThumb"}, "ScrollBar or ScrollIndicator")
        .Case("ToggleIndicator", "CheckBox or RadioButton or Switch")
        .Case("SwitchThumb", "Switch")
        .Cases({"SpinUpIndicator", "SpinDownIndicator"}, "SpinBox or DoubleSpinBox")
        .Cases({"ComboContent", "ComboIndicator", "ComboOption", "ComboPopup"}, "ComboBox")
        .Case("TumblerOption", "Tumbler")
        .Case("PathViewOption", "PathView")
        .Case("TreeDisclosure", "TreeRow")
        .Case("SectionHeader", "ListView or StandardListView")
        .Cases({"SelectionStartHandle", "SelectionEndHandle"}, "TableView or StandardTableView")
        .Default("");
  };
  auto acceptsPart = [](llvm::StringRef control, llvm::StringRef part) {
    if (part == "EmptyState")
      return isVirtualView(control);
    return llvm::StringSwitch<bool>(control)
        .Cases({"Button", "ToolButton", "RoundButton", "DelayButton", "TabButton"}, part == "ButtonContent")
        .Case("Slider", part == "SliderHandle")
        .Case("RangeSlider", part == "RangeLowerHandle" || part == "RangeUpperHandle")
        .Case("ProgressIndicator", part == "ProgressTrack" || part == "ProgressFill")
        .Cases({"ScrollBar", "ScrollIndicator"}, part == "ScrollTrack" || part == "ScrollThumb")
        .Cases({"CheckBox", "RadioButton"}, part == "ToggleIndicator")
        .Case("Switch", part == "ToggleIndicator" || part == "SwitchThumb")
        .Cases({"SpinBox", "DoubleSpinBox"},
               part == "SpinUpIndicator" || part == "SpinDownIndicator")
        .Case("ComboBox", part == "ComboContent" || part == "ComboIndicator" ||
                          part == "ComboOption" || part == "ComboPopup")
        .Case("Tumbler", part == "TumblerOption")
        .Case("PathView", part == "PathViewOption")
        .Case("TreeRow", part == "TreeDisclosure")
        .Cases({"ListView", "StandardListView"}, part == "SectionHeader")
        .Cases({"TableView", "StandardTableView"},
               part == "SelectionStartHandle" || part == "SelectionEndHandle")
        .Default(false);
  };
  if (auto required = partParents(kind);
      !required.empty() &&
      (!acceptsPart(parentKind, kind) || !body.getOps<RepeatOp>().empty() ||
       !body.getOps<ShowIfOp>().empty()))
    return emitOpError() << "requires a static child of " << required;
  if (kind == "SelectionStartHandle" || kind == "SelectionEndHandle") {
    for (auto &child : body) {
      auto name = child.getAttrOfType<StringAttr>("name");
      if (name && isa<BindOp, StateSetOp, AnimateOp>(child) &&
          (name.getValue() == "x" || name.getValue() == "y"))
        return child.emitOpError("selection handle position is assigned by its table");
    }
  }
  if (kind == "ScrollTrack" || kind == "ScrollThumb") {
    for (auto &child : body) {
      auto name = child.getAttrOfType<StringAttr>("name");
      if (name && isa<BindOp, StateSetOp, AnimateOp>(child) &&
          llvm::StringSwitch<bool>(name.getValue())
              .Cases({"x", "y", "width", "height", "min-width", "min-height",
                      "preferred-width", "preferred-height", "max-width", "max-height"}, true)
              .Default(false))
        return child.emitOpError("scroll part bounds are assigned by its parent");
    }
  }
  if ((kind == "ComboOption" || kind == "TumblerOption" || kind == "PathViewOption") && !staticPropertyOwner(getOperation()))
    return emitOpError() << kind << " in a repeated control requires a component delegate";
  if (kind == "ComboPopup") {
    for (auto &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        bool valid = llvm::StringSwitch<bool>(name)
            .Cases({"width", "max-height"}, isa<LengthType, PercentType>(type))
            .Cases({"row-height", "padding", "border-width", "border-radius"},
                   isa<LengthType>(type))
            .Cases({"background", "border-color"}, isa<BrushType, ColorType>(type))
            .Case("scroll-indicator", isa<BoolType>(type))
            .Default(false);
        if (!valid)
          return bind.emitOpError("invalid ComboPopup property or type");
      } else if (!isa<EndOp>(child) && !isExpression(child)) {
        return child.emitOpError("ComboPopup accepts only one-way property bindings");
      }
    }
  }
  llvm::StringSet<> parts;
  for (auto child : body.getOps<ElementOp>()) {
    auto part = child.getKind();
    if (!partParents(part).empty() && !parts.insert(part).second)
      return emitOpError() << "accepts at most one " << part;
  }
  if (kind == "MonthGrid" &&
      llvm::count_if(body.getOps<ElementOp>(), [](ElementOp child) {
        return child.getKind() == "DayCell";
      }) > 1)
    return emitOpError("MonthGrid accepts only one DayCell");
  if (kind == "SplitView" &&
      llvm::count_if(body.getOps<ElementOp>(), [](ElementOp child) {
        return child.getKind() == "SplitHandle";
      }) > 1)
    return emitOpError("SplitView accepts only one SplitHandle");
  if (kind == "SectionHeader" &&
      !llvm::any_of(parent.getBody().front().getOps<BindOp>(),
                    [](BindOp bind) { return bind.getName() == "section-field"; }))
    return emitOpError("SectionHeader requires a list with section-field");
  bool templateRoot = kind == "DayCell" || kind == "SplitHandle" ||
                      kind == "TreeDisclosure" || kind == "SectionHeader" ||
                      kind == "ComboOption" || kind == "TumblerOption" || kind == "PathViewOption";
  auto enclosing = enclosingVisualTemplate(*this);
  auto visualTemplate = templateRoot ? kind : enclosing ? enclosing.getKind() : llvm::StringRef{};
  if (!visualTemplate.empty()) {
    if ((kind == "DayCell" && parentKind != "MonthGrid") ||
        (kind == "SplitHandle" && parentKind != "SplitView"))
      return emitOpError() << kind << " requires a "
                           << (kind == "DayCell" ? "MonthGrid" : "SplitView")
                           << " parent";
    if (!getId().empty())
      return emitOpError() << visualTemplate << " templates are anonymous";
    for (auto &child : body) {
      if (auto element = dyn_cast<ElementOp>(child)) {
        if (llvm::StringSwitch<bool>(element.getKind())
            .Cases({"Rectangle", "Text", "Label", "Image", "Path", "StyledText",
                    "GridLayout", "Row", "FlowLayout", "FlexboxLayout",
                    "VerticalBox", "HorizontalBox", "VerticalLayout", "HorizontalLayout"}, true)
            .Default(false))
          continue;
      }
      if (!isa<BindOp, ShowIfOp, EndOp>(child) && !isExpression(child))
        return child.emitOpError() << visualTemplate << " accepts visual elements and read-only bindings";
      if (templateRoot && isa<ShowIfOp>(child))
        return child.emitOpError() << kind << " cannot be conditional";
      if (templateRoot)
        if (auto bind = dyn_cast<BindOp>(child)) {
          auto type = bind.getInput().getType();
          auto name = bind.getName();
          if (!((name == "background" || name == "border-color") && isa<BrushType, ColorType>(type)) &&
              !((name == "border-width" || name == "border-radius") && isa<LengthType>(type)) &&
              !(name == "opacity" && isa<FloatType>(type)) &&
              !(name == "visible" && isa<BoolType>(type)) &&
              !(kind == "DayCell" && name == "accessible-description" && isa<StringType>(type)) &&
              !(kind == "SectionHeader" && name == "accessible-name" && isa<StringType>(type)))
            return child.emitOpError() << "invalid " << kind << " binding";
        }
    }
  }
  if (auto delegate = getDelegate()) {
    auto base = llvm::StringSwitch<llvm::StringRef>(*delegate)
        .Case("ItemDelegate", "Button").Case("CheckDelegate", "CheckBox")
        .Case("RadioDelegate", "RadioButton").Case("SwitchDelegate", "Switch").Default({});
    if (base.empty() || base != kind)
      return emitOpError("invalid item delegate kind");
  }
  const bool virtualDelegate = isVirtualItemDelegate(*this);
  if (virtualDelegate) {
    auto invalidPart = [&](ElementOp part) {
      return !isVirtualSwitchPart(kind, part);
    };
    if (!getId().empty() || llvm::any_of(body.getOps<ElementOp>(), invalidPart) ||
        !body.getOps<InstanceOp>().empty() || !body.getOps<PropertyOp>().empty() ||
        !body.getOps<CallbackOp>().empty())
      return emitOpError("virtual item delegates require an anonymous template with only supported visual parts");
  }
  if (virtualDelegate && (kind == "CheckBox" || kind == "Switch") &&
      !llvm::hasSingleElement(body.getOps<RowLinkOp>()))
    return emitOpError("virtual toggle delegates require one writable row field");
  if (kind == "ActionGroup") {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError("ActionGroup requires a named declaration at the component root");
    llvm::StringSet<> bindings;
    bool handled = false;
    for (auto &child : body) {
      if (isa<BindOp, ControlLinkOp>(child)) {
        auto name = child.getAttrOfType<StringAttr>("name").getValue();
        auto input = child.getOperand(0);
        if (!bindings.insert(name).second)
          return child.emitOpError("duplicate ActionGroup binding");
        if (name == "current-value" && isa<StringType>(input.getType()) &&
            !nonExclusiveGroup(*this))
          continue;
        if (isa<BindOp>(child) && isa<BoolType>(input.getType()) &&
            (name == "enabled" || (name == "exclusive" && input.getDefiningOp<LiteralOp>())))
          continue;
        return child.emitOpError("invalid ActionGroup property or type");
      }
      if (auto action = dyn_cast<ElementOp>(child); action && action.getKind() == "Action")
        continue;
      if (auto handler = dyn_cast<HandlerOp>(child); handler && handler.getName() == "activated" && !handled) {
        handled = true;
        continue;
      }
      if (!isa<EndOp>(child) && !isExpression(child))
        return child.emitOpError("ActionGroup only accepts bindings, Actions and activated(value)");
    }
    return success();
  }
  if (kind == "ButtonGroup") {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError("ButtonGroup requires a named declaration at the component root");
    llvm::StringSet<> bindings;
    const bool nonExclusive = nonExclusiveButtonGroup(*this);
    for (Operation &child : body) {
      if (isa<BindOp, ControlLinkOp>(child)) {
        auto name = child.getAttrOfType<StringAttr>("name").getValue();
        auto type = child.getOperand(0).getType();
        if (!bindings.insert(name).second ||
            !((name == "current-index" && isa<IntType>(type)) ||
              ((name == "current-value" || name == "accessible-name") && isa<StringType>(type)) ||
              ((name == "enabled" || name == "exclusive") && isa<BoolType>(type))))
          return child.emitOpError("incorrect ButtonGroup property or type");
        if (name == "exclusive" && (!isa<BindOp>(child) || !child.getOperand(0).getDefiningOp<LiteralOp>()))
          return child.emitOpError("ButtonGroup exclusive requires a literal bool");
        if (nonExclusive && (name == "current-index" || name == "current-value"))
          return child.emitOpError("selection properties require an exclusive ButtonGroup");
      } else if (nonExclusive && isa<HandlerOp>(child)) {
        return child.emitOpError("selected requires an exclusive ButtonGroup");
      } else if (!isa<HandlerOp, EndOp>(child) && !isExpression(child)) {
        return child.emitOpError("ButtonGroup only accepts property bindings and selected(value)");
      }
    }
    if (bindings.contains("current-index") && bindings.contains("current-value"))
      return emitOpError("ButtonGroup accepts either current-index or current-value");
    return success();
  }
  if ((kind == "AnimatedImage" || kind == "MediaPlayer") &&
      !isStaticViewport(*this))
    return emitOpError() << kind << " in a repetition requires a component delegate";
  if (kind == "SecondaryWindow") {
    if (!isWindow(parentKind) || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError("SecondaryWindow requires a named declaration at the root Window");
    if (!llvm::hasSingleElement(body.getOps<InstanceOp>()))
      return emitOpError("SecondaryWindow requires one Window component instance");
    bool visible = false;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        if (std::exchange(visible, true) || bind.getName() != "visible" ||
            !isa<BoolType>(bind.getInput().getType()))
          return child.emitOpError("SecondaryWindow requires a single boolean visibility binding");
      } else if (auto link = dyn_cast<ControlLinkOp>(child)) {
        if (std::exchange(visible, true) || link.getName() != "visible")
          return child.emitOpError("SecondaryWindow requires a single visibility link");
      } else if (!isa<InstanceOp, EndOp>(child) && !isExpression(child)) {
        return child.emitOpError("SecondaryWindow only accepts visibility and a Window instance");
      }
    }
    return success();
  }
  if (isNativeDialog(kind)) {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError() << kind << " requires a named static declaration at the component root";
    llvm::StringSet<> properties;
    llvm::StringSet<> handlers;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        bool common = (name == "title" && isa<StringType>(type)) ||
                      (name == "enabled" && isa<BoolType>(type));
        bool specific = kind == "FileDialog"
            ? (name == "mode" && isa<FileDialogModeType>(type)) ||
                  ((name == "directory" || name == "file-name") && isa<StringType>(type)) ||
                  (name == "name-filters" && isa<StringListType>(type))
            : (name == "buttons" && isa<MessageButtonsType>(type)) ||
                  (name == "level" && isa<MessageLevelType>(type)) ||
                  ((name == "text" || name == "informative-text") && isa<StringType>(type));
        if (!common && !specific)
          return child.emitOpError() << "incorrect " << kind << " property or type";
        if (!properties.insert(name).second)
          return child.emitOpError() << "duplicate " << kind << " binding";
      } else if (auto handler = dyn_cast<HandlerOp>(child)) {
        if (!handlers.insert(handler.getName()).second)
          return child.emitOpError() << "duplicate " << kind << " result handler";
      } else if (!isa<EndOp>(child) && !isExpression(child)) {
        return child.emitOpError() << kind << " only accepts bindings and result handlers";
      }
    }
    return success();
  }
  if (kind == "SoundEffect") {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError("SoundEffect requires a named static declaration at the component root");
    llvm::StringSet<> properties;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "source" && isa<StringType>(type)) ||
              (name == "volume" && isa<FloatType>(type)) ||
              (name == "muted" && isa<BoolType>(type)) ||
              (name == "loops" && isa<IntType>(type))))
          return child.emitOpError("incorrect SoundEffect property or type");
        if (!properties.insert(name).second)
          return child.emitOpError("duplicate SoundEffect property");
      } else if (!isa<EndOp>(child) && !isExpression(child)) {
        return child.emitOpError("SoundEffect only accepts one-way property bindings");
      }
    }
    return success();
  }
  if (kind == "Timer") {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError("Timer requires a named static declaration at the component root");
    llvm::StringSet<> properties;
    bool handled = false;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "interval" && isa<IntType>(type)) ||
              ((name == "running" || name == "repeat" || name == "triggered-on-start") && isa<BoolType>(type))))
          return child.emitOpError("incorrect Timer property type or name");
        if (!properties.insert(name).second)
          return child.emitOpError("duplicate Timer property");
      } else if (auto link = dyn_cast<ControlLinkOp>(child)) {
        if (link.getName() != "running" || !properties.insert("running").second)
          return child.emitOpError("Timer requires a single running binding");
      } else if (auto handler = dyn_cast<HandlerOp>(child)) {
        if (handler.getName() != "triggered" || handled)
          return child.emitOpError("Timer accepts one triggered() handler");
        handled = true;
      } else if (!isa<EndOp>(child) && !isExpression(child)) {
        return child.emitOpError("Timer only accepts property bindings and triggered()");
      }
    }
    return success();
  }
  if (kind == "Action") {
    const bool inlineAction = parentKind == "ActionGroup";
    if (!parent || (!inlineAction && !isa<ComponentOp>(parent->getParentOp())) || getId().empty())
      return emitOpError(
          "Action requires a named declaration at the component root or in an ActionGroup");
    auto groupRefs = body.getOps<ActionGroupRefOp>();
    if ((!groupRefs.empty() && !llvm::hasSingleElement(groupRefs)) ||
        (inlineAction && (groupRefs.empty() ||
         (*groupRefs.begin()).getGroup() != parent.getKey().value_or(parent.getId()))))
      return emitOpError("Action requires a single ActionGroup membership");
    llvm::StringSet<> properties;
    bool handled = false;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "text" || name == "shortcut") &&
              isa<StringType>(type)) &&
            !(name == "icon" && isa<ImageType>(type)) &&
            !((name == "enabled" || name == "checked" || name == "checkable") &&
              isa<BoolType>(type)))
          return child.emitOpError("invalid Action property or type");
        if (!properties.insert(name).second)
          return child.emitOpError("duplicate Action property");
      } else if (auto link = dyn_cast<ControlLinkOp>(child)) {
        if (!properties.insert(link.getName()).second)
          return child.emitOpError("duplicate Action property");
      } else if (isa<HandlerOp>(child)) {
        if (handled)
          return child.emitOpError("duplicate Action handler");
        handled = true;
      } else if (!isa<ActionGroupRefOp, EndOp>(child) && !isExpression(child)) {
        return child.emitOpError(
            "Action only accepts property bindings and activated()");
      }
    }
    return success();
  }
  auto refs = body.getOps<ActionRefOp>();
  const bool linkedAction = !refs.empty();
  if (linkedAction &&
      ((!isPushButton(kind) && kind != "MenuItem" && kind != "Shortcut") ||
       !llvm::hasSingleElement(refs)))
    return emitOpError(
        "requires one Action reference on a push button, MenuItem or Shortcut");
  if (linkedAction && !virtualDelegate)
    for (auto ancestor = *this; ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>())
      if (!ancestor.getBody().empty() &&
          !ancestor.getBody().front().getOps<RepeatOp>().empty())
        return emitOpError(
            "Action presentations require a stable element path");

  if (kind == "Shortcut") {
    llvm::StringSet<> bindings;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "sequence" && isa<StringType>(type)) ||
              (name == "sequences" && isa<StringListType>(type)) ||
              ((name == "enabled" || name == "visible" || name == "auto-repeat" ||
                name == "focus-within") && isa<BoolType>(type))))
          return child.emitOpError("incorrect Shortcut property or type");
        if (!bindings.insert(name).second)
          return child.emitOpError("duplicate Shortcut binding");
      } else if (!isa<HandlerOp, ActionRefOp, ShowIfOp, EndOp>(child) &&
                 !isExpression(child)) {
        return child.emitOpError("Shortcut only accepts bindings and activation handlers");
      }
    }
    if (bindings.contains("sequence") && bindings.contains("sequences"))
      return emitOpError("Shortcut requires sequence or sequences");
  }
  if (kind == "Tooltip" && (root || parentKind == "Tooltip"))
    return emitOpError("Tooltip requires a visual owner");
  if (kind == "PopupWindow" && root)
    return emitOpError("PopupWindow requires a visual owner");
  if (kind == "SystemTrayIcon" && !root)
    return emitOpError("SystemTrayIcon requires a top-level component");
  if (kind == "ApplicationWindow" && !root)
    return emitOpError("ApplicationWindow requires a component root");
  const bool pageSlot = kind == "PageHeader" || kind == "PageFooter";
  if (pageSlot && (root || (parentKind != "Page" && parentKind != "ApplicationWindow")))
    return emitOpError() << kind << " requires a direct Page parent or ApplicationWindow parent";
  if (isSwipeSlot(kind) && parentKind != "SwipeDelegate")
    return emitOpError("swipe slots require a direct SwipeDelegate parent");
  if (forbiddenInTooltip(kind))
    for (auto ancestor = parent; ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>())
      if (ancestor.getKind() == "Tooltip")
        return emitOpError("Tooltip content cannot be a popup or virtual view");
  if ((kind == "Menu" && parentKind != "MenuBar" && parentKind != "Menu" &&
       parentKind != "ContextMenuArea" && parentKind != "SystemTrayIcon") ||
      ((kind == "MenuItem" || kind == "MenuSeparator") && parentKind != "Menu"))
    return emitOpError("requires a menu container");
  const bool table = isTableView(kind);
  const bool standardTable = kind == "StandardTableView";
  const bool tree = kind == "TreeView";
  const bool interval = kind == "RangeSlider";
  const bool textCell = kind == "TableCell" || kind == "TreeCell";
  if (kind == "TreeRow" && (parentKind != "TreeView" || !validTreeRoles(*this)))
    return emitOpError("TreeRow requires integer key and parent-key fields of "
                       "a repeated record model");
  if ((kind == "TableRow" && parentKind != "TableView") ||
      (kind == "TableColumn" && !isTableView(parentKind)))
    return emitOpError("requires a TableView parent");
  if (kind == "TableCell" && parentKind != "TableRow")
    return emitOpError("requires a TableRow parent");
  if (kind == "TreeCell" || isTypedRowEditor(kind)) {
    auto row = enclosingTreeRow(*this);
    if ((kind == "TreeCell" && !row) ||
        (row && !hasStaticEditorPath(*this, row)))
      return emitOpError("requires a stable child path within a TreeRow");
    if (isTypedRowEditor(kind) && (parentKind == "TableRow" || row) &&
        !llvm::hasSingleElement(getBody().front().getOps<RowLinkOp>()))
      return emitOpError(
          row ? "tree editors require exactly one writable field link"
              : "table editors require exactly one writable field link");
  }
  if (isVirtualView(kind) || kind == "SwipeView" || kind == "StackView") {
    for (auto ancestor = parent; ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>()) {
      if (!ancestor.getBody().empty() &&
          !ancestor.getBody().front().getOps<RepeatOp>().empty())
        return emitOpError() << (isVirtualView(kind) ? "virtual views" : kind)
                             << " cannot be inside a repeated template";
    }
  }
  auto groupRefs = body.getOps<RadioGroupRefOp>();
  auto checkRefs = body.getOps<CheckGroupRefOp>();
  auto scrollTargets = body.getOps<ScrollTargetOp>();
  const bool scrollControl = isScrollControl(kind);
  const bool attachedScroll = !scrollTargets.empty();
  if (attachedScroll && (!scrollControl || !llvm::hasSingleElement(scrollTargets)))
    return emitOpError("a scroll control accepts a single target viewport");
  const bool pushButton = isPushButton(kind);
  if (!checkRefs.empty() && ((!pushButton && kind != "CheckBox" && kind != "Switch") ||
                            !llvm::hasSingleElement(checkRefs)))
    return emitOpError("requires a single non-exclusive ButtonGroup on a toggle");
  const bool attachedRadio = !groupRefs.empty();
  if (attachedRadio && ((!pushButton && kind != "RadioButton") || parentKind == "RadioGroup" ||
                       !llvm::hasSingleElement(groupRefs) || !checkRefs.empty()))
    return emitOpError("requires a single exclusive ButtonGroup outside RadioGroup");
  if (linkedAction && (attachedRadio || !checkRefs.empty()))
    return emitOpError("an Action presentation cannot also join a ButtonGroup");
  if ((kind == "RadioButton" && !attachedRadio && !virtualDelegate && parentKind != "RadioGroup") ||
      (kind == "Tab" && (!parent || parent.getKind() != "TabWidget")) ||
      (kind == "TabButton" && (!parent || parent.getKind() != "TabBar")) ||
      (kind == "SwipePage" && (!parent || parent.getKind() != "SwipeView")) ||
      (kind == "StackPage" && (!parent || parent.getKind() != "StackView")) ||
      (kind == "StackItem" && (!parent || parent.getKind() != "StackLayout")))
    return emitOpError("requires a matching selection container");
  llvm::StringRef childKind = kind == "RadioGroup"  ? "RadioButton"
                              : kind == "TabWidget" ? "Tab"
                              : kind == "TabBar"   ? "TabButton"
                              : kind == "SwipeView" ? "SwipePage"
                              : kind == "StackView" ? "StackPage"
                                                    : "";
  if (hasNavigationValue(*this)) {
    if (!staticPropertyOwner(getOperation()))
      return emitOpError("navigation by value requires a statically addressed group");
    for (Operation &child : body) {
      if (isa<BindOp, ControlLinkOp>(child) &&
          child.getAttrOfType<StringAttr>("name").getValue() == "current-index")
        return child.emitOpError("selection accepts either current-index or current-value");
      if (auto item = dyn_cast<ElementOp>(child);
          item && (item.getKind() == childKind ||
                   (kind == "StackLayout" && item.getKind() == "StackItem")) &&
          !llvm::any_of(item.getBody().front().getOps<BindOp>(), [](BindOp bind) {
            return bind.getName() == "value";
          }))
        return child.emitOpError("requires value when the navigation group selects by current-value");
    }
  }
  if ((kind == "TabBar" || kind == "TabWidget") &&
      llvm::any_of(body.getOps<BindOp>(), [](BindOp bind) { return bind.getName() == "reorderable"; }) &&
      !writableNavigationModelRead(*this))
    return emitOpError() << "reorderable " << kind
                         << " requires a repeated tab over a writable list property";
  unsigned options = 0;
  bool repeated = false;
  unsigned listChildren = 0;
  unsigned contextMenus = 0;
  unsigned tooltips = 0;
  unsigned tooltipChildren = 0;
  bool tooltipText = false;
  unsigned cellTextBindings = 0;
  unsigned columns = 0;
  unsigned tableModels = 0;
  unsigned columnFields = 0;
  std::optional<unsigned> cells;
  llvm::StringSet<> intervalEndpoints;
  llvm::StringSet<> viewportOffsets;
  llvm::StringSet<> pageSlots;
  llvm::StringSet<> swipeSlots;
  llvm::StringSet<> animations;
  llvm::StringSet<> visualBindings;
  llvm::StringSet<> eventHandlers;
  ReadOp intervalTarget;
  ReadOp comboTextTarget;
  ReadOp calendarTarget;
  ReadOp viewportTarget;
  bool checkboxBoolean = false;
  bool checkboxState = false;
  for (Operation &child : body) {
    if (kind == "SystemTrayIcon") {
      if (isa<ElementOp, InstanceOp>(child)) {
        auto element = dyn_cast<ElementOp>(child);
        if (!element || (element.getKind() != "Menu" && element.getKind() != "Action"))
          return child.emitOpError("SystemTrayIcon accepts one optional static Menu and named Actions");
        if (element.getKind() == "Menu" && !element.getBody().empty() &&
            (!element.getBody().front().getOps<ShowIfOp>().empty() ||
             !element.getBody().front().getOps<RepeatOp>().empty()))
          return child.emitOpError("SystemTrayIcon requires a static Menu");
      }
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "icon" && isa<ImageType>(type)) ||
              ((name == "title" || name == "tooltip") && isa<StringType>(type)) ||
              ((name == "visible" || name == "icon-template") && isa<BoolType>(type))))
          return child.emitOpError("invalid SystemTrayIcon property or type");
      }
    }
    if (auto animation = dyn_cast<AnimateOp>(child)) {
      if (root && isWindow(kind))
        return child.emitOpError("animate a visual child instead of the Window");
      if (animation.getName().empty() || !animations.insert(animation.getName()).second)
        return child.emitOpError("animation requires a nonempty, unique property name");
    }
    if (kind == "CheckBox" && isa<BindOp, ControlLinkOp, RowLinkOp>(child))
      if (auto attr = child.getAttrOfType<StringAttr>("name")) {
        auto name = attr.getValue();
        checkboxBoolean |= name == "checked";
        checkboxState |= name == "check-state" || name == "tristate";
        if (checkboxBoolean && checkboxState)
          return child.emitOpError("CheckBox accepts checked or check-state/tristate");
      }
    if (auto element = dyn_cast<ElementOp>(child);
        element && (element.getKind() == "Action" || element.getKind() == "ActionGroup" || element.getKind() == "ButtonGroup" || element.getKind() == "Timer" || element.getKind() == "SoundEffect" ||
                    isNativeDialog(element.getKind())))
      continue;
    if (linkedAction &&
        (isa<ControlLinkOp>(child) ||
         (isa<HandlerOp>(child) &&
          !llvm::StringSwitch<bool>(cast<HandlerOp>(child).getName())
              .Cases({"press-and-hold", "double-clicked", "pressed", "released", "canceled"}, true)
              .Default(false))))
      return child.emitOpError(
          "activation and checked state belong to the linked Action");
    if (auto slot = dyn_cast<ElementOp>(child);
        slot && ((slot.getKind() == "EmptyState" && isVirtualView(kind)) ||
                 (slot.getKind() == "SectionHeader" && (kind == "ListView" || kind == "StandardListView")) ||
                 (isTableView(kind) && (slot.getKind() == "SelectionStartHandle" ||
                                        slot.getKind() == "SelectionEndHandle"))))
      continue;
    if (isa<ElementOp, InstanceOp>(child)) {
      if (kind == "StandardListView")
        return child.emitOpError("StandardListView cannot contain child elements");
      if (kind == "SplitView" && !child.getRegion(0).empty() &&
          !child.getRegion(0).front().getOps<RepeatOp>().empty())
        return child.emitOpError(
            "SplitView panes require stable child positions");
      auto element = dyn_cast<ElementOp>(child);
      auto childType = element ? element.getKind() : llvm::StringRef{};
      if (kind == "SwipeDelegate") {
        if (!isSwipeSlot(childType) || !element.getBody().front().getOps<RepeatOp>().empty() ||
            !element.getBody().front().getOps<ShowIfOp>().empty())
          return child.emitOpError("SwipeDelegate requires static swipe slots");
        if (!swipeSlots.insert(childType).second)
          return child.emitOpError() << "duplicate " << childType << " slot";
      }
      if (kind == "DialogButtonBox" &&
          ((childType != "Button" && childType != "DialogButton" && childType != "StandardButton") ||
           !element.getBody().front().getOps<RepeatOp>().empty()))
        return child.emitOpError("DialogButtonBox requires direct button children without repetition");
      if (kind == "TimePicker" || isPickerPopup(kind))
        return child.emitOpError() << kind << " uses built-in picker children";
      if (childType == "PageHeader" || childType == "PageFooter" ||
          (kind == "ApplicationWindow" && childType == "MenuBar")) {
        if (!pageSlots.insert(childType).second)
          return child.emitOpError() << kind << " accepts only one " << childType;
        if (!element.getBody().front().getOps<RepeatOp>().empty())
          return child.emitOpError("window and page bands cannot repeat");
      }
      if (childType == "Tooltip")
        ++tooltips;
      if (tree) {
        if (childType != "TreeRow")
          return child.emitOpError("TreeView requires a repeated TreeRow");
        ++listChildren;
      }
      if (kind == "Tooltip")
        ++tooltipChildren;
      if (table) {
        if (childType == "TableColumn") {
          auto &columnBody = element.getBody().front();
          if (standardTable && (!columnBody.getOps<RepeatOp>().empty() ||
                                !columnBody.getOps<ShowIfOp>().empty()))
            return child.emitOpError("StandardTableView requires static columns");
          ++columns;
        } else if (!standardTable && childType == "TableRow" && !cells &&
                   !element.getBody().empty()) {
          auto &body = element.getBody().front();
          cells = llvm::count_if(body, [](Operation &op) {
            return isa<ElementOp, InstanceOp>(op);
          });
          if (!llvm::hasSingleElement(body.getOps<RepeatOp>()) ||
              !body.getOps<ShowIfOp>().empty())
            return child.emitOpError(
                "TableRow requires one repeat and no condition");
        } else {
          if (standardTable)
            return child.emitOpError("StandardTableView accepts static TableColumn children");
          return child.emitOpError("TableView requires static TableColumn "
                                   "children and one repeated TableRow");
        }
      }
      if (kind == "TableRow" && !child.getRegion(0).empty() &&
          !child.getRegion(0).front().getOps<RepeatOp>().empty())
        return child.emitOpError("table cells cannot be repeated");
      if (kind == "TableRow") {
        unsigned parsers = 0;
        child.walk([&](ClipboardPasteOp) { ++parsers; });
        if (parsers > 1)
          return child.emitOpError("each table column accepts only one clipboard-paste handler");
      }
      if (kind == "MenuItem" || kind == "MenuSeparator" ||
          (kind == "MenuBar" && childType != "Menu") ||
          (kind == "Menu" && childType != "Menu" && childType != "MenuItem" &&
           childType != "MenuSeparator"))
        return child.emitOpError("invalid menu child");
      if (childType == "Menu")
        ++contextMenus;
      if (kind == "ToolSeparator")
        return child.emitOpError("ToolSeparator cannot contain child elements");
      if ((interval || kind == "Dial" || kind == "PageIndicator" ||
           kind == "Calendar" || kind == "MonthGrid" || kind == "DayOfWeekRow" ||
           kind == "WeekNumberColumn" || kind == "Tumbler" || kind == "PathView" || kind == "Spinner" ||
           kind == "BusyIndicator") &&
          childType != "Tooltip" && !acceptsPart(kind, childType) &&
          !(kind == "MonthGrid" && childType == "DayCell")) {
        if (interval)
          return child.emitOpError("RangeSlider only accepts handle parts and Tooltip children");
        return child.emitOpError() << kind << " only accepts Tooltip children";
      }
    }
    if (interval) {
      if (auto binding = dyn_cast<BindOp>(child)) {
        auto name = binding.getName();
        auto type = binding.getInput().getType();
        if (((name == "lower-value" || name == "upper-value" ||
              name == "minimum" || name == "maximum" || name == "step") &&
             !isa<FloatType>(type)) ||
            (name == "orientation" && !isa<OrientationType>(type)) ||
            (name == "enabled" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect RangeSlider property type");
        if ((name == "lower-value" || name == "upper-value") &&
            !intervalEndpoints.insert(name).second)
          return child.emitOpError("duplicate RangeSlider endpoint");
      }
      if (auto link = dyn_cast<ControlLinkOp>(child)) {
        if (!intervalEndpoints.insert(link.getName()).second)
          return child.emitOpError("duplicate RangeSlider endpoint");
        if (auto read = link.getInput().getDefiningOp<ReadOp>()) {
          if (intervalTarget && intervalTarget.getValue() == read.getValue() &&
              intervalTarget.getOwner().value_or("root") ==
                  read.getOwner().value_or("root"))
            return child.emitOpError(
                "RangeSlider endpoints require distinct writable properties");
          intervalTarget = read;
        }
      }
    }
    if (auto link = dyn_cast<ControlLinkOp>(child);
        link && isViewportOffset(kind, link.getName())) {
      if (!viewportOffsets.insert(link.getName()).second)
        return child.emitOpError("duplicate viewport offset");
      if (auto read = link.getInput().getDefiningOp<ReadOp>()) {
        if (viewportTarget && viewportTarget.getValue() == read.getValue() &&
            viewportTarget.getOwner().value_or("root") ==
                read.getOwner().value_or("root"))
          return child.emitOpError("viewport axes require distinct writable properties");
        viewportTarget = read;
      }
    }
    if (kind == "Calendar" || kind == "MonthGrid")
      if (auto link = dyn_cast<ControlLinkOp>(child))
        if (auto read = link.getInput().getDefiningOp<ReadOp>()) {
          if (calendarTarget && calendarTarget.getValue() == read.getValue() &&
              calendarTarget.getOwner().value_or("root") ==
                  read.getOwner().value_or("root"))
            return child.emitOpError("Calendar date and display-month require "
                                     "distinct writable properties");
          calendarTarget = read;
        }
    if (kind == "TextEdit" || kind == "TextArea")
      if (auto bind = dyn_cast<BindOp>(child);
          bind && bind.getName() == "wrap" &&
          !isa<TextWrapType>(bind.getInput().getType()))
        return child.emitOpError("wrap requires a TextWrap value");
    if (auto bind = dyn_cast<BindOp>(child)) {
      auto name = bind.getName();
      auto type = bind.getInput().getType();
      if (kind == "CustomRender" &&
          (((name == "renderer" || name == "data") && !isa<StringType>(type)) ||
           (name == "revision" && !isa<IntType>(type))))
        return child.emitOpError("CustomRender requires string renderer and data and integer revision");
      if (kind == "SceneView3D" &&
          (((name == "model" || name == "lights" || name == "reflection-probes" || name == "fog-volumes" || name == "particle-emitters" || name == "particle-affectors" || name == "scene-effects" || name == "scene-captures") && !isa<RecordListType>(type)) ||
           ((name == "selected-object" || name == "environment" || name == "camera" || name == "temporal-aa-mode" || name == "scene-effect" ||
             name == "scene-color-capture" || name == "scene-depth-capture") && !isa<StringType>(type)) ||
           ((name == "shadows" || name == "imported-lights" || name == "environment-skybox" || name == "frustum-culling" ||
             name == "cache-rendering" || name == "multisampling" || name == "specular-aa" || name == "temporal-aa" || name == "order-independent-transparency" ||
             name == "ambient-occlusion" || name == "scene-hdr" || name == "bloom" || name == "screen-space-reflections" || name == "screen-space-global-illumination" ||
             name == "fog-depth-enabled" || name == "fog-height-enabled" ||
             name == "fog-transmit-enabled" || name == "depth-of-field" || name == "scene-effect-depth" || name == "scene-effect-animate") && !isa<BoolType>(type)) ||
           ((name == "shadow-map-size" || name == "shadow-cascades" || name == "bloom-levels" ||
             name == "scene-lut-size" || name == "texture-width" || name == "texture-height" || name == "progressive-aa" || name == "ssr-steps" || name == "ssgi-rays" || name == "ssgi-steps" ||
             name == "fog-volume-samples" || name == "fog-volume-density-samples" || name == "layer-mask" ||
             name == "scene-effect-history-revision") && !isa<IntType>(type)) ||
           (name == "scene-lut" && !isa<ImageType>(type)) ||
           (name == "dof-blur-radius" && !isa<LengthType>(type)) ||
           (name == "scene-tone-map" && !isa<SceneToneMapType>(type)) ||
           ((name == "scene-background" || name == "ambient-color" || name == "fog-color") && !isa<ColorType>(type)) ||
           ((name.starts_with("camera-") || name.starts_with("light-") ||
             name == "ambient-intensity" || name == "shadow-bias" || name == "shadow-distance" ||
             name == "shadow-cascade-blend" || name == "shadow-split-balance" ||
             name == "environment-yaw" || name == "environment-intensity" || name == "scene-exposure" ||
             name == "supersampling" || name == "temporal-aa-strength" || name == "render-scale" ||
             name == "ssr-strength" || name == "ssr-distance" || name == "ssr-thickness" ||
             name == "ssr-roughness-cutoff" || name == "ssr-scale" ||
             name == "ssgi-strength" || name == "ssgi-occlusion" || name == "ssgi-radius" ||
             name == "ssgi-thickness" || name == "ssgi-scale" ||
             name == "scene-white-point" || name == "scene-brightness" ||
             name == "scene-contrast" || name == "scene-saturation" || name == "scene-lut-strength" || name == "scene-effect-strength" ||
             name == "fog-near" || name == "fog-far" || name == "fog-density" ||
             name == "fog-depth-curve" || name == "fog-height-start" || name == "fog-height-end" ||
             name == "fog-height-curve" || name == "fog-transmit-curve" ||
             name == "fog-volume-scattering" || name == "fog-volume-anisotropy" || name == "particle-time" ||
             name == "ao-strength" || name == "ao-distance" || name == "ao-bias" ||
             name == "bloom-strength" || name == "bloom-threshold" ||
             name == "dof-focus-distance" || name == "dof-focus-range") &&
            !isa<FloatType>(type))))
        return child.emitOpError("invalid SceneView3D property type");
      if (name == "highlighted" && (!getDelegate() || !isa<BoolType>(type)))
        return child.emitOpError("highlighted requires a boolean on an item delegate");
      if (virtualDelegate && kind == "RadioButton" && name == "checked")
        return child.emitOpError("virtual RadioDelegate checked state follows current-index");
      bool singleLine = kind == "TextInput" || kind == "LineEdit";
      if ((kind == "RadioButton" || pushButton) &&
          (name == "group" || (name == "value" && !isa<StringType>(type))))
        return child.emitOpError("requires a ButtonGroup reference or a string option value");
      if (pushButton && (attachedRadio || !checkRefs.empty()) && name == "checkable") {
        auto literal = bind.getInput().getDefiningOp<LiteralOp>();
        if (!literal || literal.getValue() != "true")
          return child.emitOpError("ButtonGroup members are checkable");
      }
      if (pushButton && attachedRadio && name == "checked")
        return child.emitOpError("bind the exclusive ButtonGroup selection instead of checked");
      if ((kind == "CheckBox" || kind == "Switch") && name == "group")
        return child.emitOpError("requires a ButtonGroup reference");
      if (name == "context-menu-enabled" &&
          (!isa<BoolType>(type) ||
           !(singleLine || kind == "SearchField" || kind == "TextArea" || kind == "TextEdit" ||
             kind == "ComboBox" || kind == "SpinBox" || kind == "DoubleSpinBox" ||
             kind == "TableCell" || kind == "TreeCell")))
        return child.emitOpError("context-menu-enabled requires a boolean on a text editor");
      if ((name == "has-focus" && focusTargetKind(kind)) ||
          (name == "focus-within" && kind == "FocusScope"))
        return child.emitOpError("focus state is read-only");
      if (name == "pressed" && isPushButton(kind))
        return child.emitOpError("button pressed state is read-only");
      if (isWindow(kind) && windowStatusProperty(name))
        return child.emitOpError("native window state is read-only");
      if (kind == "FocusScope" &&
          (name == "enabled" || name == "focus-on-click" ||
           name == "focus-on-tab-navigation" || name == "restore-focus") &&
          !isa<BoolType>(type))
        return child.emitOpError("FocusScope policy requires a boolean");
      if (name == "focus-on-click" && isPushButton(kind) &&
          !isa<BoolType>(type))
        return child.emitOpError("button focus-on-click requires a boolean");
      if (((name == "auto-repeat" && kind != "Shortcut") || name == "auto-repeat-delay" ||
           name == "auto-repeat-interval") &&
          (!isPushButton(kind) ||
           (name == "auto-repeat" ? !isa<BoolType>(type) : !isa<IntType>(type))))
        return child.emitOpError("auto-repeat requires a boolean and repeat timing requires integer milliseconds on a push button");
      if (kind == "FocusScope" && name == "initial-focus")
        return child.emitOpError("initial-focus requires an xr.focus_target reference");
      if ((kind == "DragArea" &&
           (((name == "data" || name == "drag-key") && !isa<StringType>(type)) ||
            ((name == "enabled" || name == "allow-copy" || name == "allow-move" ||
              name == "allow-link") && !isa<BoolType>(type)))) ||
          (kind == "DropArea" &&
           (((name == "enabled" || name == "accept-files") && !isa<BoolType>(type)) ||
            ((name == "keys" || name == "file-extensions") && !isa<StringListType>(type)) ||
            (name == "drop-action" && !isa<DragActionType>(type)))))
        return child.emitOpError("incorrect drag-and-drop property type");
      if (name == "touch-drag-delay" && (kind != "DragArea" || !isa<IntType>(type)))
        return child.emitOpError("touch-drag-delay requires integer milliseconds on a DragArea");
      if (name == "drag-auto-scroll" && (!isSelectionList(kind) || !isa<BoolType>(type)))
        return child.emitOpError("drag-auto-scroll requires a boolean on a list or grid");
      if ((name == "color-scheme" && (!isWindow(kind) || !isa<ColorSchemeType>(type))) ||
          (name == "accent-color" && (!isWindow(kind) || !isa<ColorType>(type))))
        return child.emitOpError("incorrect window palette property or type");
      if ((name == "no-frame" && (!isWindow(kind) || !isa<BoolType>(type))) ||
          (isWindow(kind) && name == "resizable" && !isa<BoolType>(type)) ||
          (name == "resize-border-width" && (!isWindow(kind) || !isa<LengthType>(type))) ||
          (kind == "WindowMoveArea" && name == "enabled" && !isa<BoolType>(type)))
        return child.emitOpError("incorrect window chrome property or type");
      if (name == "default-font" || name == "default-font-family" || name == "default-font-size" ||
          name == "default-font-weight" || name == "default-font-italic") {
        if (!isWindow(kind) ||
            (name == "default-font" && !isa<FontType>(type)) ||
            (name == "default-font-family" && !isa<StringType>(type)) ||
            (name == "default-font-size" && !isa<LengthType>(type)) ||
            (name == "default-font-weight" && !isa<IntType>(type)) ||
            (name == "default-font-italic" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect window font property or type");
      }
      if ((name == "font" && kind != "FontPickerPopup") || name == "font-family" || name == "font-italic" ||
          name == "font-weight" || name == "font-size") {
        const bool hasFont = singleLine ||
            llvm::StringSwitch<bool>(kind)
                .Cases({"Text", "Label", "StyledText", "TextArea", "TextEdit", "SearchField",
                        "Button", "StandardButton", "ToolButton", "RoundButton", "DelayButton",
                        "DialogButton", "CheckBox", "Switch", "RadioButton",
                        "RadioGroup", "GroupBox", "Expander", "Dialog", "Drawer", "TableCell",
                        "TreeCell", "Tooltip", "SpinBox", "DoubleSpinBox",
                        "ComboBox", "Menu", "MenuItem", "Calendar",
                        "DatePickerPopup", "Tumbler", "PathView"},
                       true)
                .Default(false);
        if (!hasFont || (name == "font" && !isa<FontType>(type)) ||
            (name == "font-family" && !isa<StringType>(type)) ||
            (name == "font-weight" && !isa<IntType>(type)) ||
            (name == "font-size" && !isa<LengthType>(type)) ||
            (name == "font-italic" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect text font property or type");
      }
      const bool textEntry = singleLine || kind == "ComboBox";
      if (name == "input-purpose" && (!textEntry || !isa<InputPurposeType>(type)))
        return child.emitOpError("input-purpose requires an InputPurpose on a single-line text editor");
      if (name == "return-key" && (!textEntry || !isa<ReturnKeyType>(type)))
        return child.emitOpError("return-key requires a return key on a single-line text editor");
      if (name == "keyboard-capitalization" &&
          (!isTextEditor(kind) || !isa<KeyboardCapitalizationType>(type)))
        return child.emitOpError("keyboard-capitalization requires a KeyboardCapitalization on a text editor");
      if ((name == "auto-correct" || name == "spell-check") &&
          (!isTextEditor(kind) || !isa<BoolType>(type)))
        return child.emitOpError("auto-correct and spell-check require booleans on a text editor");
      if (name == "keyboard-navigation" && (!isTextEditor(kind) || !isa<BoolType>(type)))
        return child.emitOpError("keyboard-navigation requires a boolean on a text editor");
      if ((name == "input-type" && (!textEntry || !isa<InputModeType>(type))) ||
          (name == "password-visible" && (!singleLine || !isa<BoolType>(type))) ||
          (name == "password-character" && (!textEntry || !isa<StringType>(type))) ||
          (name == "password-mask-delay" && (!textEntry || !isa<IntType>(type))))
        return child.emitOpError("incorrect password input property or type");
      if (name == "input-mask" && (!textEntry || !isa<StringType>(type)))
        return child.emitOpError("input-mask requires a string on a single-line text editor");
      if (name == "overwrite-mode" && (!isTextEditor(kind) || !isa<BoolType>(type)))
        return child.emitOpError("overwrite-mode requires a boolean on a text editor");
      if (name == "selectable" && (kind != "StyledText" || !isa<BoolType>(type)))
        return child.emitOpError("selectable requires a boolean on StyledText");
      if (name == "persistent-selection" && (!(isTextEditor(kind) || kind == "StyledText") || !isa<BoolType>(type)))
        return child.emitOpError("persistent-selection requires a boolean on a text editor");
      if (name == "mouse-selection-mode" &&
          (!(isTextEditor(kind) || kind == "StyledText") || !isa<MouseSelectionModeType>(type)))
        return child.emitOpError("mouse-selection-mode requires a MouseSelectionMode value on a text editor");
      const bool textSubmission = textEntry || kind == "TextEdit" || kind == "TextArea";
      if (name == "input-pattern" && (!textSubmission || !isa<StringType>(type)))
        return child.emitOpError("input-pattern requires a string on a text editor");
      if ((name == "minimum-length" || name == "maximum-length") &&
          (!textSubmission || !isa<IntType>(type)))
        return child.emitOpError() << name << " requires an integer on a text editor";
      if (name == "submit-when" && (!textSubmission || !isa<BoolType>(type)))
        return child.emitOpError("submit-when requires a boolean on a text editor");
      if ((name == "minimum-value" || name == "maximum-value") &&
          (!textEntry || !isa<FloatType>(type)))
        return child.emitOpError() << name << " requires a float on a single-line text editor";
      if (textEntry && name == "decimals" && !isa<IntType>(type))
        return child.emitOpError("decimals requires an integer");
      if (textSubmission && (name == "acceptable-input" || name == "input-validity"))
        return child.emitOpError() << name << " is read-only";
      if (kind == "ComboBox" && (name == "popup-open" || name == "highlighted-index"))
        return child.emitOpError("ComboBox popup state is read-only");
      if (isTextSelectionProperty(kind, name) || isTextCommandProperty(kind, name))
        return child.emitOpError("text editor state is read-only");
      if (name == "decimal-separator" &&
          ((!textEntry && kind != "DoubleSpinBox") || !isa<StringType>(type)))
        return child.emitOpError("decimal-separator requires a string on a decimal editor");
    }
    if (kind == "ComboBox")
      if (auto link = dyn_cast<ControlLinkOp>(child);
          link &&
          (link.getName() == "edit-text" || link.getName() == "current-value"))
        if (auto read = link.getInput().getDefiningOp<ReadOp>()) {
          if (comboTextTarget &&
              comboTextTarget.getValue() == read.getValue() &&
              comboTextTarget.getOwner().value_or("root") ==
                  read.getOwner().value_or("root"))
            return child.emitOpError("ComboBox edit-text and current-value "
                                     "require distinct writable properties");
          comboTextTarget = read;
        }
    if (auto bind = dyn_cast<BindOp>(child)) {
      auto name = bind.getName();
      if (kind == "ProgressIndicator" || kind == "Spinner" ||
          kind == "BusyIndicator") {
        auto type = bind.getInput().getType();
        if (kind == "ProgressIndicator" && (name == "position" || name == "visual-position"))
          return child.emitOpError("progress positions are read-only");
        if ((name == "progress" &&
             (kind == "BusyIndicator" || !isa<FloatType>(type))) ||
            ((name == "minimum" || name == "maximum") &&
             (kind != "ProgressIndicator" || !isa<FloatType>(type))) ||
            (name == "indeterminate" &&
             (kind == "BusyIndicator" || !isa<BoolType>(type))) ||
            (name == "running" &&
             (kind != "BusyIndicator" || !isa<BoolType>(type))) ||
            ((name == "animated" || name == "enabled") && !isa<BoolType>(type)) ||
            ((name == "track-color" || name == "color") && !isa<ColorType>(type)) ||
            (name == "stroke-width" &&
             (kind == "ProgressIndicator" || !isa<LengthType>(type))))
          return child.emitOpError("incorrect activity indicator property type");
      }
      if ((name == "check-state" || name == "tristate") &&
          (kind != "CheckBox" ||
           (name == "check-state" && !isa<CheckStateType>(bind.getInput().getType())) ||
           (name == "tristate" && !isa<BoolType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect CheckBox property or type");
      if ((name == "accessible-name" || name == "accessible-description") &&
          !isa<StringType>(bind.getInput().getType()))
        return child.emitOpError("accessibility labels require strings");
      if (name == "accessible-live" &&
          ((kind != "Text" && kind != "Label") ||
           !isa<AccessibleLiveType>(bind.getInput().getType())))
        return child.emitOpError("accessible-live requires an AccessibleLive on Text or Label");
      if (name == "accessible-heading-level" &&
          ((kind != "Text" && kind != "Label") || !isa<IntType>(bind.getInput().getType())))
        return child.emitOpError("accessible-heading-level requires an integer on Text or Label");
      if (!validTransformBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect visual transform property or type");
      if (name == "clip" &&
          ((kind != "Rectangle" && kind != "Flickable" && !isPane(kind)) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("clip requires a boolean on Rectangle, Flickable, Pane or ControlFrame");
      if (!validPaneBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect Pane or ControlFrame property type");
      if (!validApplicationWindowBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("ApplicationWindow padding and spacing require lengths");
      if (kind == "MenuBar" && name == "background" &&
          !isa<BrushType, ColorType>(bind.getInput().getType()))
        return child.emitOpError("MenuBar background requires a brush");
      if (!validButtonGeometryBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect RoundButton or ToolSeparator property type");
      if (!validBoxShadowBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect Rectangle shadow property type");
      if (name == "layout-direction" && !isa<LayoutDirectionType>(bind.getInput().getType()))
        return child.emitOpError("layout-direction requires a LayoutDirection value");
      if (!validCompositionBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError(name == "cache-rendering-hint"
                                     ? "cache-rendering-hint requires a boolean on a visual item"
                                     : "z and opacity require a visual item and a float");
      if (!validImageBinding(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect Image property type");
      if (kind == "MediaPlayer") {
        auto type = bind.getInput().getType();
        if (((name == "source" || name == "subtitle-file") && !isa<StringType>(type)) ||
            ((name == "autoplay" || name == "muted") && !isa<BoolType>(type)) ||
            ((name == "volume" || name == "playback-rate") && !isa<FloatType>(type)) ||
            ((name == "loops" || name == "audio-track" || name == "subtitle-track" ||
              name == "network-bandwidth") && !isa<IntType>(type)) ||
            (name == "image-fit" && !isa<ImageFitType>(type)) ||
            llvm::StringSwitch<bool>(name)
                .Cases({"duration", "position", "ready", "ended", "playing", "error", "status", "seekable", "has-audio", "has-video", "audio-tracks", "subtitle-tracks", "media-title", "subtitle", "audio-error", "subtitle-error", "buffer-progress", "bitrate", "live", "seek-start", "seek-end", "at-live-edge"}, true)
                .Default(false))
          return child.emitOpError("incorrect MediaPlayer property or type");
      }
      if (kind == "AnimatedImage" &&
          ((name == "current-frame" || name == "frame-count") ||
           ((name == "playing" || name == "paused") && !isa<BoolType>(bind.getInput().getType())) ||
           (name == "speed" && !isa<FloatType>(bind.getInput().getType())) ||
           (name == "loops" && !isa<IntType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect AnimatedImage playback property or type");
      if (!validTextEditorAppearance(kind, name, bind.getInput().getType()))
        return child.emitOpError("incorrect text editor appearance property or type");
      if (name == "link-color" && (kind != "StyledText" || !isa<ColorType>(bind.getInput().getType())))
        return child.emitOpError("link-color requires StyledText and a color");
      if ((name == "selection-color" || name == "selected-text-color") &&
          (!(isTextEditor(kind) || kind == "StyledText") || !isa<ColorType>(bind.getInput().getType())))
        return child.emitOpError() << name << " requires a color on a text editor or StyledText";
      if (kind == "Text" || kind == "Label" || kind == "StyledText") {
        auto type = bind.getInput().getType();
        if ((name == "text" &&
             !(kind == "StyledText" ? isa<StyledTextType>(type) : isa<StringType>(type))) ||
            (kind == "StyledText" && name == "enabled" && !isa<BoolType>(type)) ||
            (name == "wrap" && !isa<TextWrapType>(type)) ||
            (name == "overflow" && !isa<TextOverflowType>(type)) ||
            (name == "max-lines" && !isa<IntType>(type)) ||
            (name == "horizontal-alignment" && !isa<TextHorizontalAlignmentType>(type)) ||
            (name == "vertical-alignment" && !isa<TextVerticalAlignmentType>(type)))
          return child.emitOpError("incorrect text layout property type");
      }
      if (isTextEditor(kind) && kind != "ComboBox" && name == "horizontal-alignment" &&
          !isa<TextHorizontalAlignmentType>(bind.getInput().getType()))
        return child.emitOpError("horizontal-alignment requires a TextHorizontalAlignment value");
      if (kind == "FlowLayout" || kind == "ToolBar" || kind == "FlexboxLayout") {
        auto type = bind.getInput().getType();
        if ((name == "orientation" && !isa<OrientationType>(type)) ||
            (name == "alignment" && !isa<LayoutAlignmentType>(type)) ||
            (kind == "ToolBar" && name == "wrap" && !isa<BoolType>(type)) ||
            (name == "flex-direction" && !isa<FlexboxLayoutDirectionType>(type)) ||
            (name == "flex-wrap" && !isa<FlexboxLayoutWrapType>(type)) ||
            (name == "cross-axis-line-alignment" && !isa<LayoutAlignmentType>(type)) ||
            (name == "cross-axis-alignment" &&
             !isa<CrossAxisAlignmentType>(type)) ||
            ((name == "spacing" || name == "horizontal-spacing" ||
              name == "spacing-horizontal" || name == "spacing-vertical" ||
              name == "vertical-spacing" || name == "padding" ||
              name == "padding-left" || name == "padding-right" ||
              name == "padding-top" || name == "padding-bottom") &&
             !isa<LengthType>(type)))
          return child.emitOpError()
                 << "incorrect " << kind << " property type";
      }
      if ((isPushButton(kind) || kind == "DelayButton" || kind == "MenuItem") &&
          name == "icon" && !isa<ImageType>(bind.getInput().getType()))
        return child.emitOpError("icon requires an image");
      if (isPushButton(kind) || kind == "DelayButton") {
        auto type = bind.getInput().getType();
        if (((name == "icon-only" || name == "colorize-icon") && !isa<BoolType>(type)) ||
            ((name == "icon-size" || name == "spacing") && !isa<LengthType>(type)) ||
            (name == "display" && !isa<ButtonDisplayType>(type)))
          return child.emitOpError("incorrect button icon property type");
        if (isPushButton(kind) && (name == "checked" || name == "checkable") &&
            !isa<BoolType>(type))
          return child.emitOpError("button check properties require bool");
      }
      if (kind == "Expander") {
        auto type = bind.getInput().getType();
        if ((name == "title" && !isa<StringType>(type)) ||
            (name == "content-padding" && !isa<LengthType>(type)) ||
            ((name == "enabled" || name == "expanded") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect Expander property type");
      }
      if (kind == "GroupBox") {
        auto type = bind.getInput().getType();
        if ((name == "title" && !isa<StringType>(type)) ||
            (name == "content-padding" && !isa<LengthType>(type)) ||
            ((name == "enabled" || name == "checked" || name == "checkable") &&
             !isa<BoolType>(type)))
          return child.emitOpError("incorrect GroupBox property type");
      }
      if (kind == "StandardListView") {
        auto type = bind.getInput().getType();
        if ((name == "model" && !isa<StringListType, RecordListType>(type)) ||
            (name == "text-role" && (!isa<StringType>(type) ||
             !bind.getInput().getDefiningOp<LiteralOp>())) ||
            (name == "follow-current" && !isa<BoolType>(type)) ||
            (name == "row-height" && !isa<LengthType>(type)) ||
            (name == "current-index" && !isa<IntType>(type)))
          return child.emitOpError("incorrect StandardListView property type");
      }
      if (kind == "GridView") {
        auto type = bind.getInput().getType();
        if (((name == "cell-width" || name == "cell-height" || name == "spacing") &&
             !isa<LengthType>(type)) ||
            ((name == "follow-current" || name == "enabled") && !isa<BoolType>(type)) ||
            (name == "current-index" && !isa<IntType>(type)))
          return child.emitOpError("incorrect GridView property type");
      }
      if (kind == "DelayButton") {
        auto type = bind.getInput().getType();
        if ((name == "delay" && !isa<IntType>(type)) ||
            (name == "progress-color" && !isa<ColorType>(type)) ||
            ((name == "enabled" || name == "checked") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect DelayButton property type");
      }
      if (kind == "Tumbler" || kind == "PathView") {
        auto type = bind.getInput().getType();
        if (kind == "PathView" && ((name == "path" && !isa<StringType>(type)) ||
            ((name == "item-width" || name == "item-height") && !isa<LengthType>(type)) ||
            (name == "orientation" && !isa<OrientationType>(type))))
          return child.emitOpError("incorrect PathView path or item geometry type");
        if (name == "dragging" || name == "flicking" || name == "moving")
          return child.emitOpError() << kind << " motion state is read-only";
        if (name == "flick-deceleration" && !isa<FloatType>(type))
          return child.emitOpError() << kind << " flick-deceleration requires float";
        if (name == "current-text" || name == "enabled-role")
          return child.emitOpError() << "cannot bind this " << kind << " property";
        if ((name == "model" && !isa<StringListType, RecordListType>(type)) ||
            ((name == "text-role" || name == "value-role") && !isa<StringType>(type)) ||
            ((name == "current-index" || name == "count" || name == "visible-item-count") && !isa<IntType>(type)) ||
            ((name == "wrap" || name == "enabled") && !isa<BoolType>(type)))
          return child.emitOpError() << "incorrect " << kind << " property type";
        if ((name == "count" || name == "model") &&
            llvm::any_of(body.getOps<BindOp>(), [&](BindOp other) {
              return other.getName() == (name == "model" ? "count" : "model");
            }))
          return child.emitOpError() << kind << " accepts either model or count";
      }
      if (kind == "SearchField") {
        auto type = bind.getInput().getType();
        if (((name == "text" || name == "placeholder-text" ||
              name == "suggestions-for") && !isa<StringType>(type)) ||
            (name == "suggestions" && !isa<StringListType>(type)) ||
            ((name == "live" || name == "read-only" || name == "enabled") &&
             !isa<BoolType>(type)))
          return child.emitOpError("incorrect SearchField property type");
      }
      if (kind == "ComboBox") {
        auto type = bind.getInput().getType();
        if (((name == "editable" || name == "auto-complete") &&
             !isa<BoolType>(type)) ||
            ((name == "edit-text" || name == "placeholder-text") && !isa<StringType>(type)))
          return child.emitOpError("incorrect editable ComboBox property type");
        if (((name == "text-role" || name == "value-role" || name == "enabled-role") && !isa<StringType>(type)) ||
            (name == "model" && !isa<StringListType, RecordListType>(type)))
          return child.emitOpError("incorrect ComboBox property type");
      }
      if (linkedAction) {
        if (name == "checked" || name == "checkable" || name == "shortcut")
          return child.emitOpError("property belongs to the linked Action");
        if ((name == "text" && !isa<StringType>(bind.getInput().getType())) ||
            (name == "enabled" && !isa<BoolType>(bind.getInput().getType())))
          return child.emitOpError(
              "incorrect Action presentation property type");
      }
      if (kind == "MenuItem" && name == "shortcut" &&
          !isa<StringType>(bind.getInput().getType()))
        return child.emitOpError("menu shortcut requires a string");
      if (kind == "StackPage" && name == "visible")
        return child.emitOpError("StackView manages StackPage visibility");
      if (kind == "StackPage" && (name == "active" || name == "stack-index"))
        return child.emitOpError("StackPage state is read-only");
      if (isViewCountProperty(kind, name) || isSelectedIndexProperty(kind, name))
        return child.emitOpError("view counts and selection positions are read-only");
      if (isViewport(kind) && isViewportProperty(name)) {
        if (!isViewportOffset(kind, name))
          return child.emitOpError("viewport geometry is read-only");
        if (!isa<LengthType>(bind.getInput().getType()) || !isStaticViewport(*this))
          return child.emitOpError("requires a length on a statically addressed viewport");
        if (!viewportOffsets.insert(name).second)
          return child.emitOpError("duplicate viewport offset");
      }
      if (isViewport(kind) && name == "enabled" &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("viewport enabled requires a boolean");
      if (name == "mouse-drag-pan-enabled" &&
          (!isViewport(kind) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("mouse drag panning requires a boolean on a viewport");
      if ((name == "pull-to-refresh" || name == "refreshing") &&
          (!isViewport(kind) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("pull-to-refresh properties require booleans on a viewport");
      if (name == "dismiss-keyboard-on-drag" &&
          (!isViewport(kind) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("dismiss-keyboard-on-drag requires a boolean on a viewport");
      if (name == "preserve-scroll-anchor" &&
          (!isVirtualView(kind) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("scroll anchoring requires a boolean on a virtual view");
      if (name == "movable-columns" &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("movable-columns requires a boolean on a table");
      if (name == "movable-rows" &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("movable-rows requires a boolean on a table");
      if (name == "frozen-columns" &&
          (!table || !isa<IntType>(bind.getInput().getType())))
        return child.emitOpError("frozen-columns requires an integer on a table");
      if (name == "row-header-width" &&
          (!table || !isa<LengthType>(bind.getInput().getType())))
        return child.emitOpError("row-header-width requires a length on a table");
      if (name == "row-header-text" &&
          (kind != "TableRow" || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("row-header-text requires a string on a TableRow");
      if (name == "row-header-field" &&
          (!standardTable || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("row-header-field requires a string on a StandardTableView");
      if (name == "resizable-rows" &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("resizable-rows requires a boolean on a table");
      if (name == "copyable" &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("copyable requires a boolean on a table");
      if (kind == "WheelHandler" &&
          ((name == "enabled" && !isa<BoolType>(bind.getInput().getType())) ||
           (name == "orientation" && !isa<OrientationType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect WheelHandler property type");
      if (kind == "HoverHandler") {
        if (name == "hovered" || name == "hover-x" || name == "hover-y")
          return child.emitOpError("hover state is read-only");
        if ((name == "blocking" || name == "enabled") && !isa<BoolType>(bind.getInput().getType()))
          return child.emitOpError("incorrect HoverHandler property type");
      }
      if (kind == "TouchArea" && touchAreaType(getContext(), name))
        return child.emitOpError("TouchArea state is read-only");
      if (name == "press-and-hold-interval" &&
          ((kind != "TouchArea" && !isPushButton(kind)) ||
           !isa<IntType>(bind.getInput().getType())))
        return child.emitOpError("press-and-hold-interval requires an integer on TouchArea or a push button");
      if (name == "accepted-buttons" &&
          (kind != "TouchArea" || !isa<PointerButtonsType>(bind.getInput().getType())))
        return child.emitOpError("accepted-buttons requires PointerButtons on TouchArea");
      if (name == "wheel-enabled" &&
          (!llvm::StringSwitch<bool>(kind)
                .Cases({"Slider", "RangeSlider", "Dial", "SpinBox", "DoubleSpinBox"}, true)
                .Default(false) ||
           !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("wheel-enabled requires a boolean on a numeric control");
      if (name == "multi-touch" &&
          (kind != "TouchArea" || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("multi-touch requires a boolean on TouchArea");
      if (name == "cursor-shape" &&
          ((kind != "HoverHandler" && kind != "TouchArea") ||
           !isa<CursorShapeType>(bind.getInput().getType())))
        return child.emitOpError("cursor-shape requires a CursorShape on HoverHandler or TouchArea");
      if ((name == "pasteable" || name == "undoable") &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError() << name << " requires a boolean on a table view";
      if (geometryProperty(name))
        return child.emitOpError("layout geometry is read-only");
      if (name == "clipboard-text") {
        auto row = dyn_cast<ElementOp>((*this)->getParentOp());
        if (!row || row.getKind() != "TableRow" || !isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("clipboard-text requires a string on a direct table cell");
      }
      if (name == "row-height-field" &&
          ((!table && kind != "ListView" && kind != "StandardListView" && kind != "TreeView") ||
           !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("row-height-field requires a string on a list, tree, or table");
      if ((name == "section-field" || name == "section-height" ||
           name == "sticky-section-headers") &&
          ((kind != "ListView" && kind != "StandardListView") ||
           (name == "section-field" && !isa<StringType>(bind.getInput().getType())) ||
           (name == "section-height" && !isa<LengthType>(bind.getInput().getType())) ||
           (name == "sticky-section-headers" && !isa<BoolType>(bind.getInput().getType()))))
        return child.emitOpError("invalid list section property");
      if (name == "row-key-field" &&
          (!isRowKeyView(kind) || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("row-key-field requires a string on a list, grid, or table");
      if ((name == "typeahead-field" || name == "typeahead-enabled") &&
          (!isSelectionList(kind) ||
           (name == "typeahead-field" && !isa<StringType>(bind.getInput().getType())) ||
           (name == "typeahead-enabled" && !isa<BoolType>(bind.getInput().getType()))))
        return child.emitOpError("invalid list or grid typeahead property");
      if (name == "current-key" && isSelectionList(kind) &&
          !isa<IntType>(bind.getInput().getType()))
        return child.emitOpError("current-key requires an integer");
      if (name == "row-visible-field" &&
          (!isVirtualView(kind) || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("row-visible-field requires a string on a virtual view");
      if ((name == "filter-field" || name == "filter-text") &&
          ((kind != "TreeView" && !standardTable) || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("filters require string properties on a TreeView or StandardTableView");
      if ((kind == "Slider" || kind == "RangeSlider") &&
          ((name == "live" && !isa<BoolType>(bind.getInput().getType())) ||
           (name == "snap-mode" && !isa<ScrollSnapModeType>(bind.getInput().getType()))))
        return child.emitOpError("invalid slider interaction property type");
      if (kind == "StackView" && name == "initial-page") {
        if (!isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("initial-page requires a string route");
        if (failed(verifyStackRoute(&child, *this, bind.getInput())))
          return failure();
      }
      if ((kind == "StackView" || kind == "SwipeView") &&
          name == "transition-duration" &&
          !isa<IntType>(bind.getInput().getType()))
        return child.emitOpError("transition-duration requires an integer");
      if (kind == "StackView" &&
          (name == "current-page" || name == "depth" || name == "can-go-back"))
        return child.emitOpError("StackView state is read-only");
      if (kind == "PageIndicator") {
        auto type = bind.getInput().getType();
        if (((name == "count" || name == "current-index") &&
             !isa<IntType>(type)) ||
            ((name == "interactive" || name == "enabled") &&
             !isa<BoolType>(type)) ||
            ((name == "dot-size" || name == "spacing") &&
             !isa<LengthType>(type)) ||
            ((name == "color" || name == "current-color") &&
             !isa<ColorType>(type)) ||
            (name == "orientation" && !isa<OrientationType>(type)))
          return child.emitOpError("incorrect PageIndicator property type");
      }
      if (kind == "DoubleSpinBox") {
        auto type = bind.getInput().getType();
        if (((name == "value" || name == "minimum" || name == "maximum" ||
              name == "step-size") && !isa<FloatType>(type)) ||
            (name == "decimals" && !isa<IntType>(type)) ||
            ((name == "editable" || name == "read-only" || name == "wrap" ||
              name == "enabled") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect DoubleSpinBox property type");
      }
      if (name == "disabled-weekdays" &&
          (!(kind == "Calendar" || kind == "MonthGrid" || kind == "DatePickerPopup") ||
           !isa<IntListType>(bind.getInput().getType())))
        return child.emitOpError("disabled-weekdays requires [int] on a date selection control");
      if ((name == "highlight-start" || name == "highlight-end") &&
          (!(kind == "Calendar" || kind == "MonthGrid") ||
           !isa<DateType>(bind.getInput().getType())))
        return child.emitOpError("calendar highlight endpoints require a Date on Calendar or MonthGrid");
      if (name == "fixed-month" &&
          (kind != "MonthGrid" || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("fixed-month requires a boolean on MonthGrid");
      if (name == "disabled-dates" &&
          (!(kind == "Calendar" || kind == "MonthGrid" || kind == "DatePickerPopup") ||
           !isa<DateListType>(bind.getInput().getType())))
        return child.emitOpError("disabled-dates requires [Date] on a date selection control");
      if (kind == "Calendar" || kind == "MonthGrid" || kind == "DayOfWeekRow" ||
          kind == "WeekNumberColumn" || kind == "DatePickerPopup") {
        auto type = bind.getInput().getType();
        if (((name == "date" || name == "display-month" ||
              name == "minimum-date" || name == "maximum-date" || name == "today") &&
             !isa<DateType>(type)) ||
            (name == "week-start" && !isa<IntType>(type)) ||
            (name == "enabled" && !isa<BoolType>(type)) ||
            (name == "weekday-labels" && !isa<StringListType>(type)))
          return child.emitOpError("incorrect Calendar property type");
        if (kind == "DatePickerPopup" && name == "display-month")
          return child.emitOpError("DatePickerPopup manages its display month");
        if ((name == "today" && kind != "MonthGrid") ||
            (name == "weekday-labels" && (kind == "MonthGrid" || kind == "WeekNumberColumn")) ||
            (name == "display-month" && kind == "DayOfWeekRow") ||
            ((name == "date" || name == "minimum-date" || name == "maximum-date") &&
             (kind == "DayOfWeekRow" || kind == "WeekNumberColumn")))
          return child.emitOpError("property is unavailable on this calendar control");
      }
      if ((name == "minimum-time" || name == "maximum-time") &&
          (kind != "TimePickerPopup" || !isa<TimeType>(bind.getInput().getType())))
        return child.emitOpError("time bounds require a Time on TimePickerPopup");
      if (isTimePicker(kind)) {
        auto type = bind.getInput().getType();
        if ((name == "time" && !isa<TimeType>(type)) ||
            ((name == "use-24-hour-format" || name == "show-seconds" ||
              name == "enabled") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect time picker property type");
      }
      if (kind == "ColorPickerPopup") {
        auto type = bind.getInput().getType();
        if ((name == "color" && !isa<ColorType>(type)) ||
            ((name == "show-alpha" || name == "show-color-plane" || name == "enabled") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect color picker property type");
      }
      if (kind == "FontPickerPopup") {
        auto type = bind.getInput().getType();
        if ((name == "font" && !isa<FontType>(type)) ||
            (name == "preview-text" && !isa<StringType>(type)) ||
            (name == "enabled" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect font picker property type");
      }
      if ((kind == "TimePicker" || isPickerPopup(kind)) &&
          (name == "accept-text" || name == "cancel-text" || name == "title") &&
          (!isPickerPopup(kind) || !isa<StringType>(bind.getInput().getType())))
        return child.emitOpError("incorrect picker label property type");
      if ((kind == "SwipeView" || kind == "SwipePage") &&
          (((name == "enabled" || name == "visible" || name == "interactive") &&
            !isa<BoolType>(bind.getInput().getType())) ||
           (name == "current-index" &&
            !isa<IntType>(bind.getInput().getType())) ||
           (name == "orientation" &&
            !isa<OrientationType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect swipe page or view property type");
      if (name == "auto-reveal-focus" &&
          (!isContentViewport(kind) || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("auto-reveal-focus requires a boolean on ScrollView or Flickable");
      if (name == "reserve-scroll-bar-space" &&
          (kind != "ScrollView" || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("reserve-scroll-bar-space requires a boolean on ScrollView");
      if (name == "vertical-scroll-bar-policy" ||
          name == "horizontal-scroll-bar-policy") {
        const bool scrollable =
            name == "vertical-scroll-bar-policy"
                ? isVirtualView(kind) || kind == "ScrollView" || kind == "TabBar"
                : table || kind == "ScrollView" || kind == "TabBar";
        if (!scrollable || !isa<ScrollBarPolicyType>(bind.getInput().getType()))
          return child.emitOpError(
              "requires a ScrollBarPolicy on a scrollable axis");
      }
      if ((name == "content-width" || name == "content-height") &&
          !isPane(kind) && (!isContentViewport(kind) ||
           !isa<LengthType, PercentType>(bind.getInput().getType())))
        return child.emitOpError("requires a scrollable content length");
      if (name == "flick-direction" && (kind != "Flickable" || !isa<FlickDirectionType>(bind.getInput().getType())))
        return child.emitOpError("requires a FlickDirection on Flickable");
      if (name == "bounds-behavior" && (kind != "Flickable" || !isa<FlickBoundsBehaviorType>(bind.getInput().getType())))
        return child.emitOpError("requires a FlickBoundsBehavior on Flickable");
      if (name == "bounds-movement" && (kind != "Flickable" || !isa<FlickBoundsMovementType>(bind.getInput().getType())))
        return child.emitOpError("requires a FlickBoundsMovement on Flickable");
      if (name == "rebound-duration" && (kind != "Flickable" || !isa<IntType>(bind.getInput().getType())))
        return child.emitOpError("requires an integer rebound duration on Flickable");
      if (kind == "Flickable" && (name == "dragging" || name == "flicking" || name == "moving" || name == "rebounding" ||
          name == "horizontal-overshoot" || name == "vertical-overshoot"))
        return child.emitOpError("Flickable motion properties are read-only");
      if (kind == "Flickable" && name == "interactive" && !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("Flickable interactive requires a boolean");
      if ((isNavigationGroup(kind) && name == "count") ||
          (kind == "StackLayout" && name == "active-index"))
        return child.emitOpError("navigation counts and active-index are read-only");
      if ((isNavigationGroup(kind) && name == "current-value") ||
          ((kind == "TabButton" || kind == "Tab" || kind == "SwipePage" || kind == "StackItem") && name == "value"))
        if (!isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("navigation values require string");
      if ((name == "tabs-closable" || name == "closable") &&
          (!isa<BoolType>(bind.getInput().getType()) ||
           (name == "tabs-closable" ? kind != "TabBar" && kind != "TabWidget"
                                    : kind != "TabButton" && kind != "Tab")))
        return child.emitOpError("tab close settings require bool on the matching tab or container");
      if ((kind == "TabBar" || kind == "TabWidget") && name == "reorderable" &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("reorderable requires bool");
      if (kind == "TabBar" || kind == "TabButton") {
        auto type = bind.getInput().getType();
        if (((name == "enabled" || name == "visible") && !isa<BoolType>(type)) ||
            (name == "text" && !isa<StringType>(type)) ||
            (name == "current-index" && !isa<IntType>(type)) ||
            (name == "orientation" && !isa<OrientationType>(type)) ||
            (name == "spacing" && !isa<LengthType>(type)))
          return child.emitOpError("incorrect tab bar or button property type");
      }
      if (kind == "Page" || pageSlot) {
        auto type = bind.getInput().getType();
        if ((name == "title" && (pageSlot || !isa<StringType>(type))) ||
            (name == "background" && !isa<BrushType>(type)) ||
            ((name == "enabled" || name == "visible") && !isa<BoolType>(type)) ||
            ((name == "spacing" || name == "padding" ||
              name == "padding-left" || name == "padding-right" ||
              name == "padding-top" || name == "padding-bottom") &&
             !isa<LengthType>(type)))
          return child.emitOpError("incorrect page property type");
      }
      if (kind == "Dial") {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (((name == "value" || name == "minimum" || name == "maximum" ||
              name == "step" || name == "start-angle" || name == "end-angle") &&
             !isa<FloatType>(type)) ||
            ((name == "wrap" || name == "enabled") && !isa<BoolType>(type)) ||
            (name == "input-mode" && !isa<DialInputModeType>(type)))
          return child.emitOpError("incorrect Dial property type");
      }
      if (kind == "Tooltip") {
        const auto name = bind.getName();
        const auto type = bind.getInput().getType();
        if ((name == "text" && !isa<StringType>(type)) ||
            ((name == "delay" || name == "timeout") && !isa<IntType>(type)) ||
            (name == "padding" && !isa<LengthType>(type)) ||
            (name == "color" && !isa<ColorType>(type)) ||
            (name == "background" && !isa<BrushType>(type)))
          return child.emitOpError("incorrect Tooltip property type");
        tooltipText |= name == "text";
      }
      if (kind == "SplitView") {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (name == "resizing")
          return child.emitOpError("SplitView resizing is read-only");
        if ((name == "orientation" && !isa<OrientationType>(type)) ||
            ((name == "handle-size" || name == "handle-hit-size") &&
             !isa<LengthType>(type)) ||
            (name == "handle-color" && !isa<ColorType>(type)) ||
            (name == "enabled" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect SplitView property type");
      }
      if (table && bind.getName() == "selection-target" &&
          !isa<SelectionTargetType>(bind.getInput().getType()))
        return child.emitOpError(
            "selection-target requires a SelectionTarget value");
      if (name == "auto-sort" && (!standardTable || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("auto-sort requires a bool on StandardTableView");
      if (llvm::StringSwitch<bool>(name)
              .Cases({"edit-on-click", "edit-on-selected-click", "edit-on-double-click",
                      "edit-on-key", "edit-on-tab", "edit-on-typing"}, true)
              .Default(false) &&
          (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("edit triggers require a bool on a table view");
      if (kind == "TableColumn" && name == "editable" &&
          (parentKind != "StandardTableView" || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("TableColumn.editable requires a bool on a StandardTableView column");
      if (kind == "TableColumn" && name == "visible" &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("TableColumn.visible requires a bool");
      if (standardTable && name == "model") {
        if (!isa<RecordListType>(bind.getInput().getType()))
          return child.emitOpError("StandardTableView model requires a record list");
        ++tableModels;
      }
      if (standardTable && name == "follow-current" && !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("follow-current requires a bool value");
      if (standardTable && name == "current-row" && !isa<IntType>(bind.getInput().getType()))
        return child.emitOpError("current-row requires an int value");
      if (kind == "TableColumn" && name == "field") {
        if (parentKind != "StandardTableView" ||
            !isa<StringType>(bind.getInput().getType()) ||
            !bind.getInput().getDefiningOp<LiteralOp>())
          return child.emitOpError("field requires a constant string on a StandardTableView column");
        ++columnFields;
      }
      if (textCell && bind.getName() == "text") {
        if (!isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("cell text requires a string value");
        ++cellTextBindings;
      }
      if (table && (name == "can-undo" || name == "can-redo"))
        return child.emitOpError("table history state is read-only");
      if (table && (name == "current-sort-column" || name == "current-sort-descending" ||
                    name == "current-column" || name == "selection-count" || name == "has-selection" ||
                    name == "selection-active" || name == "selection-dragging"))
        return child.emitOpError("table sorting state is read-only");
      if (isRowSelectionView(kind) && (name == "selection-count" || name == "has-selection"))
        return child.emitOpError("list selection state is read-only");
      if ((isRowSelectionView(kind) || table) && name == "selected-keys")
        return child.emitOpError("selected-keys is read-only");
      if (name == "drag-select" && (!table || !isa<BoolType>(bind.getInput().getType())))
        return child.emitOpError("drag-select requires a bool value on a table");
      if (name == "selection-gesture" &&
          (!table || !isa<SelectionGestureType>(bind.getInput().getType())))
        return child.emitOpError("selection-gesture requires a SelectionGesture on a table");
      if (name == "selection-hold-interval" &&
          (!table || !isa<IntType>(bind.getInput().getType())))
        return child.emitOpError("selection-hold-interval requires an int on a table");
      if (((table && (bind.getName() == "resizable-columns" ||
                      bind.getName() == "multi-select")) ||
           (kind == "TableColumn" &&
            (bind.getName() == "resizable" || bind.getName() == "sortable"))) &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError(
            "table interaction flags require a bool value");
      if ((tree || isSelectionList(kind)) && bind.getName() == "multi-select" &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("multi-select requires a bool value");
      if (kind == "TreeView" && name == "root-key" && !isa<IntType>(bind.getInput().getType()))
        return child.emitOpError("TreeView root-key requires an integer");
      if (kind == "DialogButton" && bind.getName() == "role" &&
          !isa<DialogRoleType>(bind.getInput().getType()))
        return child.emitOpError("role requires a DialogRole value");
      if (kind == "StandardButton" && name == "kind" &&
          !isa<StandardButtonKindType>(bind.getInput().getType()))
        return child.emitOpError("kind requires a StandardButtonKind value");
      if ((pushButton || kind == "DelayButton" || kind == "StandardButton" || kind == "DialogButton") &&
          (name == "primary" || name == "flat") &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("button appearance flags require bool values");
      if (kind == "DialogButtonBox" && (name == "enabled" || name == "wrap") &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("button box state requires a bool value");
      if (kind == "SwipeDelegate" &&
          ((name == "open-side" && !isa<SwipeSideType>(bind.getInput().getType())) ||
           (name == "enabled" && !isa<BoolType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect SwipeDelegate property type");
      if (isWindowOverlay(kind) &&
          (bind.getName() == "open" || bind.getName() == "close-on-escape" ||
           bind.getName() == "close-on-outside") &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("dialog state requires a bool value");
      if (kind == "PopupWindow") {
        auto type = bind.getInput().getType();
        if ((name == "placement" && !isa<PopupPlacementType>(type)) ||
            (name == "anchor-gap" && !isa<LengthType>(type)) ||
            ((name == "modal" || name == "focus" || name == "dim" || name == "enabled") && !isa<BoolType>(type)))
          return child.emitOpError("incorrect PopupWindow property type");
      }
      if (kind == "Drawer" &&
          ((name == "edge" &&
            !isa<DrawerEdgeType>(bind.getInput().getType())) ||
           (name == "drag-margin" &&
            !isa<LengthType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect Drawer property type");
    }
    if (textCell) {
      if (!isa<BindOp, RowLinkOp, AnimateOp, StateSelectOp, StateSetOp, StateTimingOp, ShowIfOp, EndOp>(child) &&
          !isExpression(child))
        return child.emitOpError("invalid cell member");
      if (isa<RowLinkOp>(child))
        ++cellTextBindings;
    }
    if (kind == "TableColumn" && !isa<BindOp, EndOp>(child) &&
        !isExpression(child))
      return child.emitOpError("TableColumn only supports property bindings");
    if (kind == "SplitView" && isa<RepeatOp>(child))
      return child.emitOpError("SplitView cannot be a repeated template");
    if (kind == "Tooltip" && isa<RepeatOp>(child))
      return child.emitOpError("Tooltip cannot be a repeated child");
    if (kind == "PopupWindow" && isa<RepeatOp>(child))
      return child.emitOpError("PopupWindow cannot be a repeated template");
    if (pageSlot && isa<RepeatOp>(child))
      return child.emitOpError() << kind << " cannot be repeated";
    if (isVirtualView(kind) && isa<RepeatOp>(child))
      return child.emitOpError("virtual views cannot be repeated templates");
    if (kind == "SwipeView" && isa<RepeatOp>(child))
      return child.emitOpError("SwipeView cannot be a repeated template");
    if (kind == "SwipeDelegate" && isa<RepeatOp>(child))
      return child.emitOpError("SwipeDelegate cannot be a repeated template");
    if (kind == "StackView" && isa<RepeatOp>(child))
      return child.emitOpError("StackView cannot be a repeated template");
    if (usesListTemplate(kind) && isa<ElementOp, InstanceOp>(child)) {
      auto &body = child.getRegion(0);
      if (body.empty() || body.front().getOps<RepeatOp>().empty())
        return child.emitOpError() << kind << " requires a repeated element";
      ++listChildren;
    }
    if (kind == "Loader" && isa<ElementOp, InstanceOp>(child)) {
      auto instance = dyn_cast<InstanceOp>(child);
      if (!instance || instance.getBody().empty() ||
          instance.getBody().front().getOps<RepeatOp>().empty())
        return child.emitOpError("Loader requires a repeated component candidate");
    }
    if (kind == "StackLayout" && isa<ElementOp, InstanceOp>(child)) {
      auto item = dyn_cast<ElementOp>(child);
      auto repeatedItem = !child.getRegion(0).front().getOps<RepeatOp>().empty();
      if ((hasNavigationValue(*this) || repeatedItem) &&
          (!item || item.getKind() != "StackItem"))
        return child.emitOpError("StackLayout requires StackItem children for repetition or current-value");
      ++options;
      repeated |= repeatedItem;
    }
    if (!childKind.empty() && isa<ElementOp, InstanceOp>(child)) {
      auto element = dyn_cast<ElementOp>(child);
      if (!element || element.getKind() != childKind)
        return child.emitOpError("requires a ") << childKind << " child";
      if (kind == "StackView" && element.getId().empty())
        return child.emitOpError("StackPage requires a route ID");
      ++options;
      if (!element.getBody().empty() &&
          !element.getBody().front().getOps<RepeatOp>().empty())
        repeated = true;
    }
    if ((kind == "StackPage" && isa<ShowIfOp, RepeatOp>(child)) ||
        ((kind == "SwipePage" || kind == "Tab") && isa<ShowIfOp>(child)) ||
        (((kind == "RadioButton" && !attachedRadio && parentKind != "RadioGroup") ||
          kind == "TabButton") && isa<ShowIfOp>(child)))
      return child.emitOpError("selection options must be static");
    if ((kind == "RadioButton" || kind == "TabButton") &&
        isa<ElementOp, InstanceOp>(child)) {
      auto element = dyn_cast<ElementOp>(child);
      if (!element || !acceptsPart(kind, element.getKind()))
        return child.emitOpError() << kind << " cannot contain child elements";
    }
    if (isa<PropertyOp, CallbackOp>(child) && (root || getKey().value_or(getId()).empty()))
      return child.emitOpError("requires a keyed non-root element");
    if (kind == "HoverHandler" && isa<RepeatOp>(child))
      return child.emitOpError("repeated hover handlers require a component delegate");
    if (kind == "ScaleRotateGestureHandler" || kind == "SwipeGestureHandler") {
      if (isa<RepeatOp>(child))
        return child.emitOpError("repeated gesture handlers require a component delegate");
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        if (kind == "SwipeGestureHandler" ? swipeType(getContext(), name)
                                            : scaleRotateType(getContext(), name))
          return bind.emitOpError("gesture metrics are read-only");
        if ((name == "enabled" || name.starts_with("handle-swipe-")) && !isa<BoolType>(bind.getInput().getType()))
          return bind.emitOpError("gesture options require a boolean");
      }
    }
    if (scrollControl) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto type = bind.getInput().getType();
        auto name = bind.getName();
        if (isScrollThumbMetric(name) || isScrollStateMetric(kind, name))
          return bind.emitOpError("scroll control metrics are read-only");
        const bool standaloneProperty = name == "position" || name == "size";
        if (name == "target" || (attachedScroll && standaloneProperty))
          return bind.emitOpError("invalid property for the scroll control mode");
        if ((name == "orientation" && !isa<OrientationType>(type)) ||
            (name == "policy" && !isa<ScrollBarPolicyType>(type)) ||
            ((standaloneProperty || name == "minimum-size") && !isa<FloatType>(type)) ||
            (name == "interactive" && (kind != "ScrollBar" || !isa<BoolType>(type))) ||
            (name == "step-size" && (kind != "ScrollBar" || !isa<FloatType>(type))) ||
            (name == "snap-mode" && (kind != "ScrollBar" || !isa<ScrollSnapModeType>(type))))
          return bind.emitOpError("has an invalid scroll control property type");
      }
      auto part = dyn_cast<ElementOp>(child);
      if ((part && !acceptsPart(kind, part.getKind())) || isa<InstanceOp, RepeatOp>(child) ||
          ((attachedScroll || kind == "ScrollIndicator") && isa<HandlerOp, ControlLinkOp>(child)))
        return child.emitOpError("is invalid on a scroll control");
    }
    if (!isa<BindOp, ActionRefOp, RadioGroupRefOp, CheckGroupRefOp, ScrollTargetOp, SplitFillOp, AnimateOp, ShowIfOp, RepeatOp, ControlLinkOp,
             RowLinkOp, HandlerOp, CallbackOp, ElementOp, InstanceOp, PropertyOp, StateSelectOp,
             StateSetOp, StateTimingOp, FocusTargetOp, EndOp>(child) &&
        !isExpression(child))
      return child.emitOpError("is not valid in an element body");
    if (auto name = child.getAttrOfType<StringAttr>("name")) {
      if (isa<BindOp, ControlLinkOp, RowLinkOp>(child) &&
          !visualBindings.insert(name.getValue()).second)
        return child.emitOpError("duplicate visual binding ") << name.getValue();
      if (isa<HandlerOp>(child) && !eventHandlers.insert(name.getValue()).second)
        return child.emitOpError("duplicate event handler ") << name.getValue();
    }
  }
  if (repeated && options != 1 && kind != "RadioGroup")
    return emitOpError() << "a repeated " << childKind
                         << " must be the only option";
  if (usesListTemplate(kind) && listChildren != 1)
    return emitOpError() << kind << " requires exactly one for element";
  if (isSelectionList(kind)) {
    if (visualBindings.contains("row-key-field") && visualBindings.contains("current-index"))
      return emitOpError("current-index cannot be bound when row-key-field is present");
    if (visualBindings.contains("current-key") && !visualBindings.contains("row-key-field"))
      return emitOpError("current-key requires row-key-field");
  }
  if (tree && listChildren != 1)
    return emitOpError("TreeView requires exactly one repeated TreeRow");
  if (kind == "StackView" && options == 0)
    return emitOpError("StackView requires at least one named StackPage");
  if (kind == "SwipeDelegate" && (swipeSlots.size() < 2 || !swipeSlots.contains("SwipeContent")))
    return emitOpError("SwipeDelegate requires SwipeContent and at least one action slot");
  if (standardTable && (columns == 0 || tableModels != 1))
    return emitOpError("StandardTableView requires one record model and at least one TableColumn");
  if (kind == "TableColumn" && parentKind == "StandardTableView" && columnFields != 1)
    return emitOpError("StandardTableView column requires exactly one field");
  if (kind == "TableView" && (columns == 0 || cells != columns))
    return emitOpError("TableView requires columns and one cell per column in "
                       "the repeated TableRow");
  if (kind == "ContextMenuArea" && contextMenus != 1)
    return emitOpError("ContextMenuArea requires one direct Menu");
  if (kind == "SystemTrayIcon" && contextMenus > 1)
    return emitOpError("SystemTrayIcon accepts at most one Menu");
  if (tooltips > 1)
    return emitOpError("an element can contain at most one Tooltip");
  if (tooltipChildren > 1 || (tooltipChildren == 1 && tooltipText))
    return emitOpError("Tooltip requires text or a single content child");
  if (cellTextBindings > 1)
    return emitOpError("requires a single cell text binding");
  return success();
}

static ElementOp enclosingRepeatedElement(InstanceOp instance) {
  for (auto element = instance->getParentOfType<ElementOp>(); element;
       element = element->getParentOfType<ElementOp>())
    if (!element.getBody().front().getOps<RepeatOp>().empty())
      return element;
  return {};
}

static bool supportsRepeatedChildInstances(ElementOp row) {
  return row && llvm::StringSwitch<bool>(row.getKind())
      .Cases({"TableRow", "TreeRow", "TabButton", "SwipePage", "Tab", "StackItem"}, true)
      .Default(false);
}

LogicalResult InstanceOp::verify() {
  if (getKey().empty())
    return emitOpError("requires a nonempty instance key");
  llvm::StringMap<Operation *> bindings;
  llvm::StringSet<> handlers;
  const bool secondary = cast<ElementOp>((*this)->getParentOp()).getKind() == "SecondaryWindow";
  bool repeated = false;
  bool clipboard = false;
  if (auto row = enclosingRepeatedElement(*this);
      row && (!supportsRepeatedChildInstances(row) || !getId().empty() ||
              !getBody().front().getOps<RepeatOp>().empty()))
    return emitOpError("requires an anonymous component with a static path in a TableRow, TreeRow, TabButton, Tab, or SwipePage");
  for (Operation &child : getBody().front()) {
    if (secondary && isa<LayoutBindOp, RepeatOp, ShowIfOp, StateSetOp, StateTimingOp>(child))
      return child.emitOpError("a secondary window instance only accepts input bindings");
    if (isa<BindOp, InstanceLinkOp, LayoutBindOp>(child)) {
      auto name = child.getAttrOfType<StringAttr>("name").getValue();
      if (geometryProperty(name))
        return child.emitOpError("layout geometry is read-only");
      if (!bindings.try_emplace(name, &child).second)
        return child.emitOpError("duplicate instance binding ") << name;
      if (auto bind = dyn_cast<BindOp>(child);
          bind && name == "clipboard-text" &&
          cast<ElementOp>((*this)->getParentOp()).getKind() == "TableRow" &&
          !isa<StringType>(bind.getInput().getType()))
        return child.emitOpError("clipboard-text requires a string on a direct table cell");
    } else if (isa<ClipboardPasteOp>(child)) {
      if (std::exchange(clipboard, true))
        return child.emitOpError("duplicate clipboard-paste handler");
    } else if (auto callback = dyn_cast<CallbackOp>(child)) {
      if (!handlers.insert(callback.getName()).second)
        return child.emitOpError("duplicate instance callback handler ") << callback.getName();
    } else if (auto repeat = dyn_cast<RepeatOp>(child)) {
      auto parent = cast<ElementOp>((*this)->getParentOp());
      if (parent.getKind() == "Loader") {
        if (repeated || !getId().empty() || repeat.getItem() != "_" ||
            !repeat.getIndex().empty() || !isa<IntType>(repeat.getInput().getType()))
          return child.emitOpError(
              "Loader requires an anonymous component and an integer repeat with no named parameters");
      } else if (repeated || !usesListTemplate(parent.getKind()) || !getId().empty()) {
        return child.emitOpError(
            "repeated instances require an anonymous ListView template or an anonymous GridView template");
      }
      repeated = true;
    } else if (!isa<EndOp, ShowIfOp, StateSelectOp, StateSetOp, StateTimingOp, InstanceStateOp>(child) && !isExpression(child)) {
      return child.emitOpError("is not valid in an instance body");
    }
  }
  return success();
}

static RepeatOp instanceRepeat(InstanceOp instance) {
  auto repeats = instance.getBody().front().getOps<RepeatOp>();
  if (llvm::hasSingleElement(repeats))
    return *repeats.begin();
  auto row = enclosingRepeatedElement(instance);
  if (!supportsRepeatedChildInstances(row))
    return {};
  auto rowRepeats = row.getBody().front().getOps<RepeatOp>();
  return llvm::hasSingleElement(rowRepeats) ? *rowRepeats.begin() : RepeatOp{};
}

static ReadOp instanceRowModel(InstanceLinkOp link) {
  auto repeat = instanceRepeat(cast<InstanceOp>(link->getParentOp()));
  if (!repeat)
    return {};
  auto value = recordFieldBase(link.getInput());
  auto item = value.getDefiningOp<ParamOp>();
  if (!item || item.getValue() != repeat.getItem())
    return {};
  if (auto model = dyn_cast<RecordListType>(repeat.getInput().getType())) {
    auto record = dyn_cast<RecordType>(value.getType());
    if (!record || record.getName() != model.getName())
      return {};
  } else if (value != link.getInput() || !isa<StringListType>(repeat.getInput().getType()) ||
             !isa<StringType>(value.getType())) {
    return {};
  }
  return repeat.getInput().getDefiningOp<ReadOp>();
}

LogicalResult InstanceLinkOp::verify() {
  if (getInput().getDefiningOp<ReadOp>() || instanceRowModel(*this))
    return success();
  return emitOpError("requires a property reference or a repeated item of a writable model");
}

static InstanceLinkOp clipboardTarget(ClipboardPasteOp parser) {
  auto instance = cast<InstanceOp>(parser->getParentOp());
  for (auto link : instance.getBody().front().getOps<InstanceLinkOp>())
    if (link.getName() == parser.getTarget())
      return link;
  return {};
}

static ReturnOp clipboardResult(ClipboardPasteOp parser) {
  auto *terminator = parser.getBody().front().getTerminator();
  return terminator ? dyn_cast_or_null<ReturnOp>(terminator->getPrevNode()) : ReturnOp{};
}

LogicalResult ClipboardPasteOp::verify() {
  auto instance = cast<InstanceOp>((*this)->getParentOp());
  auto row = enclosingRepeatedElement(instance);
  auto repeat = instanceRepeat(instance);
  auto link = clipboardTarget(*this);
  if (!row || row.getKind() != "TableRow" || !repeat || !link || !instanceRowModel(link))
    return emitOpError("requires a component input linked to a table model row");
  auto result = clipboardResult(*this);
  if (!result || !isa<RecordType>(result.getInput().getType()))
    return emitOpError("requires a record result");
  auto text = getTextParameter();
  if (text.empty() || text == repeat.getItem() || text == repeat.getIndex())
    return emitOpError("requires a text parameter distinct from the row item and index");
  for (auto &child : getBody().front()) {
    if (auto param = dyn_cast<ParamOp>(child)) {
      auto name = param.getValue();
      auto type = param.getResult().getType();
      auto model = repeat.getInput().getType();
      auto record = dyn_cast<RecordType>(type);
      auto list = dyn_cast<RecordListType>(model);
      const bool itemType = (isa<StringListType>(model) && isa<StringType>(type)) ||
                            (isa<IntListType>(model) && isa<IntType>(type)) ||
                            (isa<Hit3DListType>(model) && isa<Hit3DType>(type)) ||
                            (isa<DateListType>(model) && isa<DateType>(type)) ||
          (list && record && list.getName() == record.getName());
      if (name.empty() || (name == text && !isa<StringType>(type)) ||
          (name != text && name != repeat.getItem() && name != repeat.getIndex()) ||
          (name == repeat.getItem() && !itemType) ||
          (name == repeat.getIndex() && !isa<IntType>(type)))
        return param.emitOpError("incorrect clipboard-paste parameter");
    }
    if (&child != result.getOperation() && !isa<EndOp>(child) && !isExpression(child))
      return child.emitOpError("clipboard-paste requires one pure result expression");
  }
  return success();
}

template <typename Op> static LogicalResult verifyTiming(Op animation) {
  auto duration = animation.getInput().template getDefiningOp<LiteralOp>();
  int64_t milliseconds;
  if (!duration || duration.getValue().getAsInteger(10, milliseconds) ||
      milliseconds < 0 || milliseconds > 60000)
    return animation.emitOpError("requires a constant duration between 0 and 60000ms");
  if (auto delay = animation.getDelayAttr(); delay && (delay.getInt() < 0 || delay.getInt() > 60000))
    return animation.emitOpError("requires a delay between 0 and 60000ms");
  if (auto curve = animation.getCurveAttr()) {
    auto coordinates = curve.asArrayRef();
    if (coordinates.size() != 4 ||
        !llvm::all_of(coordinates, [](double value) { return std::isfinite(value); }) ||
        coordinates[0] < 0 || coordinates[0] > 1 ||
        coordinates[2] < 0 || coordinates[2] > 1)
      return animation.emitOpError("requires four finite curve coordinates with both x coordinates in [0, 1]");
  }
  return success();
}

LogicalResult AnimateOp::verify() {
  auto kind = cast<ElementOp>((*this)->getParentOp()).getKind();
  if (isViewCountProperty(kind, getName()) || isSelectedIndexProperty(kind, getName()))
    return emitOpError("view counts and selection positions are read-only");
  if (isScrollControl(kind) && (isScrollThumbMetric(getName()) || isScrollStateMetric(kind, getName())))
    return emitOpError("scroll control metrics are read-only");
  if (isViewport(kind) && isViewportProperty(getName()))
    return emitOpError("viewport geometry is read-only");
  if (isTextSubmissionRule(kind, getName()) || isTextSelectionProperty(kind, getName()) ||
      isTextCommandProperty(kind, getName()))
    return emitOpError("cannot animate text editor state");
  return verifyTiming(*this);
}

LogicalResult StateSelectOp::verify() {
  auto *parent = (*this)->getParentOp();
  auto key = parent->getAttrOfType<StringAttr>("key");
  auto owner = key ? key.getValue() : parent->getAttrOfType<StringAttr>("id").getValue();
  if (getOwner() != owner ||
      getNames().empty() || getNames().size() != getConditions().size() ||
      !llvm::hasSingleElement(parent->getRegion(0).front().getOps<StateSelectOp>()))
    return emitOpError("requires one ordered state group on its owning element");
  llvm::StringSet<> names;
  for (auto attribute : getNames()) {
    auto name = dyn_cast<StringAttr>(attribute);
    if (!name || name.getValue().empty() || !names.insert(name.getValue()).second)
      return emitOpError("requires nonempty, unique state names");
  }
  return success();
}

static bool stateCase(Operation *use, Value selector, int64_t index) {
  auto state = selector.getDefiningOp<StateSelectOp>();
  if (!state || index < 0 || static_cast<uint64_t>(index) >= state.getNames().size())
    return false;
  for (auto *owner = use->getParentOp(); isa_and_nonnull<ElementOp, InstanceOp>(owner);
       owner = owner->getParentOp()) {
    if (owner == state->getParentOp())
      return true;
    if (!owner->getRegion(0).front().getOps<RepeatOp>().empty())
      return false;
  }
  return false;
}

static bool sameVisualType(Type a, Type b) {
  return a == b ||
      (isa<LengthType, PercentType>(a) && isa<LengthType, PercentType>(b));
}

static LogicalResult verifyInstanceLayout(Operation *op, llvm::StringRef name, Type value);

template <typename Op> static LogicalResult verifyStateValues(Op state) {
  if (!stateCase(state, state.getSelector(), state.getCaseIndexAttr().getInt()) || state.getName().empty())
    return state.emitOpError("requires a property and a state case in scope");
  auto &body = state->getParentOp()->getRegion(0).front();
  for (auto other : body.template getOps<Op>())
    if (other != state && other.getName() == state.getName() &&
        (other.getSelector() != state.getSelector() ||
         other.getCaseIndexAttr() == state.getCaseIndexAttr()))
      return state.emitOpError("requires one state group and one value per case");
  return success();
}

LogicalResult InstanceStateOp::verify() {
  if (failed(verifyStateValues(*this)))
    return failure();
  auto instance = cast<InstanceOp>((*this)->getParentOp());
  for (auto link : instance.getBody().front().getOps<InstanceLinkOp>())
    if (link.getName() == getName())
      return emitOpError("cannot override a two-way component input");
  return success();
}

LogicalResult StateSetOp::verify() {
  if (failed(verifyStateValues(*this)))
    return failure();
  if (isa<InstanceOp>((*this)->getParentOp()))
    return verifyInstanceLayout(*this, getName(), getInput().getType());
  auto element = cast<ElementOp>((*this)->getParentOp());
  if (!validTextEditorAppearance(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect text editor appearance property or type");
  if (getName() == "pressed" && isPushButton(element.getKind()))
    return emitOpError("button pressed state is read-only");
  if (isTextSubmissionRule(element.getKind(), getName()) ||
      isTextSelectionProperty(element.getKind(), getName()) ||
      isTextCommandProperty(element.getKind(), getName()))
    return emitOpError("cannot override text editor state in a visual state");
  if (isScrollControl(element.getKind()) &&
      (isScrollThumbMetric(getName()) || isScrollStateMetric(element.getKind(), getName())))
    return emitOpError("scroll control metrics are read-only");
  if (geometryProperty(getName()))
    return emitOpError("layout geometry is read-only");
  if (isViewCountProperty(element.getKind(), getName()) ||
      isSelectedIndexProperty(element.getKind(), getName()) ||
      ((isRowSelectionView(element.getKind()) || isTableView(element.getKind())) &&
       getName() == "selected-keys") ||
      (isRowSelectionView(element.getKind()) &&
       (getName() == "selection-count" || getName() == "has-selection")))
    return emitOpError("view counts and selection positions are read-only");
  if (isViewport(element.getKind()) && isViewportProperty(getName()))
    return emitOpError("viewport geometry is read-only");
  if (isViewport(element.getKind()) && getName() == "enabled" &&
      !isa<BoolType>(getInput().getType()))
    return emitOpError("viewport enabled requires a boolean");
  if (!validTransformBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect visual transform property or type");
  if (getName() == "clip" &&
      ((element.getKind() != "Rectangle" && element.getKind() != "Flickable" && !isPane(element.getKind())) || !isa<BoolType>(getInput().getType())))
    return emitOpError("clip requires a boolean on Rectangle, Flickable, Pane or ControlFrame");
  if (!validPaneBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect Pane or ControlFrame property type");
  if (!validApplicationWindowBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("ApplicationWindow padding and spacing require lengths");
  if (!validButtonGeometryBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect RoundButton or ToolSeparator property type");
  if (!validBoxShadowBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect Rectangle shadow property type");
  if (getName() == "layout-direction" && !isa<LayoutDirectionType>(getInput().getType()))
    return emitOpError("layout-direction requires a LayoutDirection value");
  if (!validCompositionBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError(getName() == "cache-rendering-hint"
                           ? "cache-rendering-hint requires a boolean on a visual item"
                           : "z and opacity require a visual item and a float");
  if (!validImageBinding(element.getKind(), getName(), getInput().getType()))
    return emitOpError("incorrect Image property type");
  auto &body = element.getBody().front();
  for (auto binding : body.getOps<BindOp>())
    if (binding.getName() == getName() &&
        !sameVisualType(binding.getInput().getType(), getInput().getType()))
      return emitOpError("must preserve the base binding type");
  for (auto &op : body)
    if (isa<ControlLinkOp, RowLinkOp>(op))
      if (auto name = op.getAttrOfType<StringAttr>("name"); name && name.getValue() == getName())
        return emitOpError("cannot override a two-way property");
  for (auto binding : body.getOps<StateSetOp>())
    if (binding != *this && binding.getName() == getName() &&
        !sameVisualType(binding.getInput().getType(), getInput().getType()))
      return emitOpError("state cases must have compatible value types");
  return success();
}

LogicalResult StateTimingOp::verify() {
  if (!stateCase(*this, getSelector(), getCaseIndexAttr().getInt()) ||
      (getDirection() != "in" && getDirection() != "out"))
    return emitOpError("requires a state case and an in or out direction");
  auto *parent = (*this)->getParentOp();
  auto &body = parent->getRegion(0).front();
  auto element = dyn_cast<ElementOp>(parent);
  if ((element && isWindow(element.getKind())) ||
      llvm::any_of(body.getOps<AnimateOp>(), [&](AnimateOp op) { return op.getName() == getName(); }))
    return emitOpError("requires a visual property without a standalone animation");
  const bool base = element
      ? llvm::any_of(body.getOps<BindOp>(), [&](BindOp op) { return op.getName() == getName(); })
      : llvm::any_of(body.getOps<LayoutBindOp>(), [&](LayoutBindOp op) { return op.getName() == getName(); });
  if (!base ||
      !llvm::any_of(body.getOps<StateSetOp>(), [&](StateSetOp op) {
        return op.getName() == getName() && op.getSelector() == getSelector() &&
               op.getCaseIndexAttr().getInt() == getCaseIndexAttr().getInt() &&
               isa<IntType, FloatType, LengthType, PercentType, ColorType, BrushType>(op.getInput().getType());
      }))
    return emitOpError("requires a base binding and an animatable property in the state");
  for (auto timing : body.getOps<StateTimingOp>())
    if (timing != *this && timing.getName() == getName() &&
        timing.getSelector() == getSelector() && timing.getDirection() == getDirection() &&
        timing.getCaseIndexAttr().getInt() == getCaseIndexAttr().getInt())
      return emitOpError("duplicate state transition direction");
  return verifyTiming(*this);
}

LogicalResult ControlLinkOp::verify() {
  auto element = cast<ElementOp>((*this)->getParentOp());
  auto kind = element.getKind();
  auto type = getInput().getType();
  const bool text = (kind == "TextInput" || kind == "LineEdit" ||
                     kind == "SearchField" || kind == "TextEdit" || kind == "TextArea") &&
                    getName() == "text" && isa<StringType>(type);
  const bool expanded = kind == "Expander" && getName() == "expanded" && isa<BoolType>(type);
  const bool checked = (isPushButton(kind) || kind == "CheckBox" || kind == "Switch" ||
                        kind == "MenuItem" || kind == "Action" || kind == "DelayButton" ||
                        kind == "GroupBox") &&
                       getName() == "checked" && isa<BoolType>(type);
  if (checked && isPushButton(kind) && !element.getBody().front().getOps<RadioGroupRefOp>().empty())
    return emitOpError("bind the exclusive ButtonGroup selection instead of checked");
  const bool checkState = kind == "CheckBox" && getName() == "check-state" && isa<CheckStateType>(type);
  if (checkState && getInput().getDefiningOp<CheckGroupReadOp>()) {
    for (auto binding : element.getBody().front().getOps<BindOp>())
      if (binding.getName() == "tristate")
        return emitOpError("an aggregate CheckBox uses the group's three-state value without tristate");
    return success();
  }
  const bool opened =
      isWindowOverlay(kind) && getName() == "open" && isa<BoolType>(type);
  const bool range =
      (((kind == "Slider" || kind == "Dial") && getName() == "value") ||
       (kind == "ScrollBar" && getName() == "position") ||
       (kind == "RangeSlider" &&
        (getName() == "lower-value" || getName() == "upper-value"))) &&
      isa<FloatType>(type);
  const bool spin = getName() == "value" &&
                    ((kind == "SpinBox" && isa<IntType>(type)) ||
                     (kind == "DoubleSpinBox" && isa<FloatType>(type)));
  const bool recordChoice = kind == "ComboBox" || kind == "Tumbler" || kind == "PathView";
  const bool valueChoice = recordChoice || kind == "RadioGroup" || kind == "ButtonGroup" || isNavigationGroup(kind);
  const bool choice =
      ((valueChoice || kind == "TabWidget" || kind == "TabBar" || kind == "SwipeView" ||
        kind == "PageIndicator" || usesListTemplate(kind) || kind == "StandardListView") &&
       getName() == "current-index" && isa<IntType>(type)) ||
      (valueChoice && getName() == "current-value" &&
       (recordChoice ? !comboValueTypeName(type).empty() : isa<StringType>(type))) ||
      (kind == "ActionGroup" && getName() == "current-value" && isa<StringType>(type)) ||
      ((kind == "TreeView" || isSelectionList(kind)) && getName() == "current-key" &&
       isa<IntType>(type)) ||
      (isTableView(kind) && getName() == "current-row" && isa<IntType>(type));
  const bool comboText =
      kind == "ComboBox" && getName() == "edit-text" && isa<StringType>(type);
  const bool calendar =
      (((kind == "Calendar" || kind == "MonthGrid" || kind == "DatePickerPopup") && getName() == "date") ||
       ((kind == "Calendar" || kind == "MonthGrid") && getName() == "display-month")) &&
      isa<DateType>(type);
  const bool time = isTimePicker(kind) && getName() == "time" && isa<TimeType>(type);
  const bool color = kind == "ColorPickerPopup" && getName() == "color" && isa<ColorType>(type);
  const bool font = kind == "FontPickerPopup" && getName() == "font" && isa<FontType>(type);
  const bool swipeSide = kind == "SwipeDelegate" && getName() == "open-side" && isa<SwipeSideType>(type);
  const bool running = kind == "Timer" && getName() == "running" && isa<BoolType>(type);
  const bool visible = kind == "SecondaryWindow" && getName() == "visible" && isa<BoolType>(type);
  const bool viewport = isViewportOffset(kind, getName()) && isa<LengthType>(type) &&
                        isStaticViewport(element);
  if ((!text && !checked && !expanded && !checkState && !range && !spin && !choice && !opened &&
       !comboText && !calendar && !time && !color && !font && !swipeSide && !running && !visible && !viewport) ||
      !getInput().getDefiningOp<ReadOp>())
    return emitOpError(
        "requires a writable property reference for the control");
  return success();
}

LogicalResult NavigateOp::verify() {
  auto *parent = (*this)->getParentOp();
  if (!isa_and_nonnull<HandlerOp, CallbackOp>(parent))
    return emitOpError("requires a handler or callback parent");
  if (auto callback = dyn_cast<CallbackOp>(parent);
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("stack navigation is not allowed in a pure callback");
  auto method = getMethod();
  const bool page = method == "push" || method == "replace" ||
                    method == "pop-to" || method == "reset";
  const bool path = method == "push-pages" || method == "reset-to";
  auto args = getArgs();
  if ((!page && !path && method != "pop") ||
      args.size() != (page || path ? 1 : 0) ||
      (page && !isa<StringType>(args.front().getType())) ||
      (path && !isa<StringListType>(args.front().getType())))
    return emitOpError("invalid StackView navigation method or arguments");
  return success();
}

LogicalResult TimerCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("timer commands are not allowed in a pure callback");
  auto command = getCommand();
  if (command != "start" && command != "stop" && command != "restart")
    return emitOpError("requires start, stop, or restart");
  return success();
}

LogicalResult MediaCommandOp::verify() {
  auto *parent = (*this)->getParentOp();
  if (!isa_and_nonnull<HandlerOp, CallbackOp>(parent))
    return emitOpError("requires a handler or callback parent");
  if (auto callback = dyn_cast<CallbackOp>(parent);
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("media commands require an impure callback");
  auto method = getMethod();
  if ((method == "seek" && getPosition()) ||
      ((method == "play" || method == "pause" || method == "stop" || method == "go-live") && !getPosition()))
    return success();
  return emitOpError("requires play(), pause(), stop(), go-live(), or seek(float)");
}

LogicalResult PopupCommandOp::verify() {
  auto *parent = (*this)->getParentOp();
  if (!isa_and_nonnull<HandlerOp, CallbackOp>(parent))
    return emitOpError("requires a handler or callback parent");
  if (auto callback = dyn_cast<CallbackOp>(parent);
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("popup commands are not allowed in a pure callback");
  if (getOpen() != "true" && getOpen() != "false")
    return emitOpError("requires a boolean open value");
  return success();
}

static LogicalResult verifyNativeDialogOpen(Operation *op) {
  auto *parent = op->getParentOp();
  if (!isa_and_nonnull<HandlerOp, CallbackOp>(parent))
    return op->emitOpError("requires a handler or callback parent");
  if (auto callback = dyn_cast<CallbackOp>(parent);
      callback && callback.getPure().value_or("false") == "true")
    return op->emitOpError("native dialogs cannot be opened in a pure callback");
  return success();
}

LogicalResult FileDialogOpenOp::verify() { return verifyNativeDialogOpen(*this); }
LogicalResult NativeDialogOpenOp::verify() { return verifyNativeDialogOpen(*this); }

LogicalResult FocusTargetOp::verify() {
  auto parent = cast<ElementOp>((*this)->getParentOp());
  if (parent.getKind() != "FocusScope" || getTarget().empty() ||
      !llvm::hasSingleElement(parent.getBody().front().getOps<FocusTargetOp>()))
    return emitOpError("requires one initial-focus reference on a FocusScope");
  return success();
}

LogicalResult SplitFillOp::verify() {
  auto parent = cast<ElementOp>((*this)->getParentOp());
  auto &body = parent.getBody().front();
  if (parent.getKind() != "SplitView" || getTarget().empty() ||
      !llvm::hasSingleElement(body.getOps<SplitFillOp>()))
    return emitOpError("requires one fill-pane reference on a SplitView");
  for (auto child : body.getOps<ElementOp>()) {
    if (child.getId() != getTarget())
      continue;
    auto kind = child.getKind();
    if (kind == "Menu" || kind == "Tooltip" || kind == "Dialog" ||
        kind == "Drawer" || kind == "PopupWindow" || kind == "Shortcut" ||
        isNativeDialog(kind))
      break;
    return success();
  }
  if (llvm::any_of(body.getOps<InstanceOp>(), [&](InstanceOp child) {
        return child.getId() == getTarget();
      }))
    return success();
  return emitOpError("fill-pane requires a named direct pane of this SplitView");
}

LogicalResult FocusCommandOp::verify() {
  if (!llvm::StringSwitch<bool>(getAction())
           .Cases({"focus", "clear", "open-popup", "close-popup"}, true)
           .Default(false))
    return emitOpError("unknown focus action");
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("focus commands are not allowed in pure callbacks");
  return success();
}

LogicalResult ButtonClickOp::verify() {
  if (getAnimated() && getAnimated().value_or("") != "true")
    return emitOpError("animated must be true when present");
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("button activation is not allowed in pure callbacks");
  return success();
}

LogicalResult ButtonToggleOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("button state changes are not allowed in pure callbacks");
  return success();
}

LogicalResult SwipeCancelOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("swipe cancellation is not allowed in pure callbacks");
  return success();
}

LogicalResult CancelFlickOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("cancel-flick is not allowed in pure callbacks");
  return success();
}

LogicalResult ReturnToBoundsOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("return-to-bounds is not allowed in pure callbacks");
  return success();
}

LogicalResult TextCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("text commands are not allowed in pure callbacks");
  auto method = getMethod();
  auto args = getArgs();
  if ((method == "set-selection-offsets" || method == "edit-cell" || method == "remove-text") &&
      args.size() == 2 && isa<IntType>(args[0].getType()) && isa<IntType>(args[1].getType()))
    return success();
  if (method == "replace-text" && args.size() == 3 &&
      isa<IntType>(args[0].getType()) && isa<IntType>(args[1].getType()) &&
      isa<StringType>(args[2].getType()))
    return success();
  if (method == "insert-text" && args.size() == 2 &&
      isa<IntType>(args[0].getType()) && isa<StringType>(args[1].getType()))
    return success();
  if (method == "append-text" && args.size() == 1 && isa<StringType>(args[0].getType()))
    return success();
  if (args.empty() &&
      llvm::StringSwitch<bool>(method)
          .Cases({"select-all", "select-word", "clear-selection", "undo", "redo", "cut", "copy", "paste",
                  "cancel-edit", "clear-text"}, true)
          .Default(false))
    return success();
  return emitOpError("requires set-selection-offsets(int, int), edit-cell(int, int), a valid text edit signature, or a known command with no operands");
}

LogicalResult ScrollCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("scroll commands are not allowed in pure callbacks");
  auto args = getArgs();
  auto method = getMethod();
  if ((method == "increase" || method == "decrease") && args.empty())
    return success();
  if (args.size() == 2) {
    auto first = args[0].getType();
    auto second = args[1].getType();
    if (((method == "scroll-to" || method == "scroll-by") &&
         isa<LengthType>(first) && isa<LengthType>(second)) ||
        ((method == "position-view-at-index" || method == "position-view-at-source-index" || method == "position-view-at-row" ||
          method == "position-view-at-column" || method == "position-view-at-key") && isa<IntType>(first) &&
         isa<ScrollAlignmentType>(second)))
      return success();
  }
  if ((method == "position-view-at-cell" || method == "position-view-at-source-cell") &&
      args.size() == 4 &&
      isa<IntType>(args[0].getType()) && isa<IntType>(args[1].getType()) &&
      isa<ScrollAlignmentType>(args[2].getType()) && isa<ScrollAlignmentType>(args[3].getType()))
    return success();
  return emitOpError("requires increase(), decrease(), scroll-to(length, length), scroll-by(length, length), position-view-at-index(int, ScrollAlignment), "
                     "position-view-at-source-index(int, ScrollAlignment), position-view-at-key(int, ScrollAlignment), "
                     "position-view-at-row(int, ScrollAlignment), position-view-at-column(int, ScrollAlignment), "
                     "position-view-at-cell(int, int, ScrollAlignment, ScrollAlignment), "
                     "or position-view-at-source-cell(int, int, ScrollAlignment, ScrollAlignment)");
}

LogicalResult PositionChildOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("scroll commands are not allowed in pure callbacks");
  return success();
}

LogicalResult TableSelectionCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("table selection commands are not allowed in a pure callback");
  auto arity = llvm::StringSwitch<int>(getMethod())
      .Cases({"select-all", "clear-selection"}, 0)
      .Cases({"select-row", "toggle-row", "select-column"}, 1)
      .Cases({"select-cell", "set-current-cell", "toggle-cell"}, 2)
      .Case("select-range", 4).Default(-1);
  if (arity < 0 || getArgs().size() != static_cast<size_t>(arity))
    return emitOpError("invalid table selection command or arguments");
  return success();
}

LogicalResult SourceSelectionOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("source selection is not allowed in pure callbacks");
  auto method = getMethod();
  auto args = getArgs();
  if (method == "select-keys" && args.size() == 1 && isa<IntListType>(args[0].getType()))
    return success();
  if ((((method == "select-source-index" || method == "select-key") && args.size() == 1) ||
       (method == "select-source-cell" && args.size() == 2)) &&
      llvm::all_of(args, [](Value arg) { return isa<IntType>(arg.getType()); }))
    return success();
  return emitOpError("requires select-source-index(int), select-key(int), select-source-cell(int, int), or select-keys([int])");
}

LogicalResult TableSelectionReadOp::verify() {
  auto type = getResult().getType();
  if (((getValue() == "current-column" || getValue() == "selection-count") && isa<IntType>(type)) ||
      ((getValue() == "has-selection" || getValue() == "selection-active" ||
        getValue() == "selection-dragging") && isa<BoolType>(type)))
    return success();
  return emitOpError("requires a typed table selection property");
}

LogicalResult TableSortCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("table sorting is not allowed in a pure callback");
  auto args = getArgs();
  if ((getMethod() == "clear-sort" && args.empty()) ||
      (getMethod() == "sort-by" && args.size() == 2 &&
       isa<IntType>(args[0].getType()) && isa<BoolType>(args[1].getType())))
    return success();
  return emitOpError("invalid table sort command or arguments");
}

LogicalResult TableSortReadOp::verify() {
  auto type = getResult().getType();
  if ((getValue() == "current-sort-column" && isa<IntType>(type)) ||
      (getValue() == "current-sort-descending" && isa<BoolType>(type)))
    return success();
  return emitOpError("requires a typed table sort property");
}

LogicalResult TableHistoryCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("table history commands are not allowed in a pure callback");
  if (getMethod() != "undo" && getMethod() != "redo")
    return emitOpError("requires undo or redo");
  return success();
}

LogicalResult TableHistoryReadOp::verify() {
  if (getValue() != "can-undo" && getValue() != "can-redo")
    return emitOpError("requires can-undo or can-redo");
  return success();
}

LogicalResult SplitRestoreOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("restoring split state is not allowed in a pure callback");
  return success();
}

LogicalResult TreeCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("tree commands are not allowed in pure callbacks");
  auto arity = llvm::StringSwitch<int>(getMethod())
      .Cases({"expand", "collapse", "toggle-expanded", "collapse-recursively", "expand-to"}, 1)
      .Case("expand-recursively", 2).Default(-1);
  if (arity < 0 || getArgs().size() != static_cast<size_t>(arity))
    return emitOpError("invalid tree command or arguments");
  return success();
}

LogicalResult TreeQueryOp::verify() {
  auto type = getResult().getType();
  auto name = getValue();
  if (((name == "is-expanded" || name == "is-row-selected" ||
        name == "is-key-selected") && isa<BoolType>(type)) ||
      ((name == "depth" || name == "row-for-key") && isa<IntType>(type)))
    return success();
  return emitOpError("requires a typed tree query");
}

LogicalResult ViewIndexOp::verify() {
  if (getValue() == "view-index-for-source" || getValue() == "source-index-for-view" ||
      getValue() == "view-index-for-key")
    return success();
  return emitOpError("requires view-index-for-source or source-index-for-view or view-index-for-key");
}

LogicalResult MoveColumnOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("moving a column is not allowed in a pure callback");
  return success();
}

LogicalResult MoveRowOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("moving a row is not allowed in a pure callback");
  return success();
}

LogicalResult MoveNavigationItemOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("move-item is not allowed in a pure callback");
  return success();
}

LogicalResult SetRowHeightOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("set-row-height is not allowed in a pure callback");
  return success();
}

static bool hasRootWindow(Operation *op) {
  auto component = op->getParentOfType<ComponentOp>();
  if (!component)
    return false;
  auto roots = component.getBody().front().getOps<ElementOp>();
  return llvm::hasSingleElement(roots) && isWindow((*roots.begin()).getKind());
}

LogicalResult WindowReadOp::verify() {
  if (!hasRootWindow(*this) || !windowStatusProperty(getValue()))
    return emitOpError("requires a status property on the root Window");
  return success();
}

LogicalResult WindowCommandOp::verify() {
  if (!hasRootWindow(*this))
    return emitOpError("requires a component with a root Window");
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("window commands are not allowed in a pure callback");
  if (getCommand() == "set-full-screen" && getEnabled())
    return success();
  if (!getEnabled() && llvm::StringSwitch<bool>(getCommand())
      .Cases({"minimize", "maximize", "restore", "toggle-maximized", "toggle-full-screen"}, true)
      .Default(false))
    return success();
  return emitOpError("invalid window command or arguments");
}

LogicalResult CheckGroupCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("group commands are not allowed in a pure callback");
  return success();
}

LogicalResult WindowCloseOp::verify() {
  if (!hasRootWindow(*this))
    return emitOpError("requires a component with a root Window");
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("Window.close is not allowed in a pure callback");
  return success();
}

LogicalResult SecondaryWindowCommandOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("secondary window commands are not allowed in a pure callback");
  if (getCommand() != "show" && getCommand() != "hide" && getCommand() != "close")
    return emitOpError("requires show, hide, or close");
  return success();
}

LogicalResult MediaReadOp::verify() {
  auto name = getValue();
  auto type = getResult().getType();
  if (((name == "position" || name == "duration" || name == "buffer-progress" ||
        name == "seek-start" || name == "seek-end") && isa<FloatType>(type)) ||
      ((name == "ready" || name == "playing" || name == "ended" || name == "seekable" ||
        name == "has-audio" || name == "has-video" || name == "live" || name == "at-live-edge") && isa<BoolType>(type)) ||
      ((name == "audio-tracks" || name == "subtitle-tracks" || name == "bitrate" ||
        name == "loops-remaining") && isa<IntType>(type)) ||
      ((name == "error" || name == "status" || name == "media-title" ||
        name == "subtitle" || name == "audio-error" || name == "subtitle-error") && isa<StringType>(type)))
    return success();
  return emitOpError("incorrect media state property or type");
}

LogicalResult ImagePlaybackReadOp::verify() {
  return getValue() == "current-frame" || getValue() == "frame-count"
      ? success() : emitOpError("requires an AnimatedImage playback state property");
}

LogicalResult FocusReadOp::verify() {
  return getValue() == "has-focus" || getValue() == "focus-within"
      ? success() : emitOpError("requires a focus state property");
}

LogicalResult ViewportReadOp::verify() {
  if (!isViewportProperty(getValue()))
    return emitOpError("requires a viewport geometry property");
  return success();
}

LogicalResult StackReadOp::verify() {
  auto name = getValue();
  auto type = getResult().getType();
  if (auto route = getRoute()) {
    if (!route.getDefiningOp<LiteralOp>())
      return emitOpError("requires a literal StackPage route");
    if ((name == "active" && isa<BoolType>(type)) ||
        (name == "stack-index" && isa<IntType>(type)))
      return success();
    return emitOpError("requires a typed StackPage state property");
  }
  if ((name == "current-page" && isa<StringType>(type)) ||
      (name == "depth" && isa<IntType>(type)) ||
      (name == "can-go-back" && isa<BoolType>(type)))
    return success();
  return emitOpError("requires a typed StackView state property");
}

static ReadOp editableRowModel(RowLinkOp link) {
  auto cell = cast<ElementOp>(link->getParentOp());
  auto row = dyn_cast<ElementOp>(cell->getParentOp());
  const bool table = row && row.getKind() == "TableRow";
  const bool itemDelegate = isVirtualItemDelegate(cell);
  if (itemDelegate) {
    row = cell;
  } else if (!table) {
    row = enclosingTreeRow(cell);
    if (!row || !hasStaticEditorPath(cell, row))
      return {};
  }
  auto value = recordFieldBase(link.getInput());
  auto item = value.getDefiningOp<ParamOp>();
  const auto kind = cell.getKind();
  const auto type = link.getInput().getType();
  const bool text = kind == (table ? "TableCell" : "TreeCell") &&
                    link.getName() == "text" && isa<StringType>(type);
  const bool spin = link.getName() == "value" &&
                    ((kind == "SpinBox" && isa<IntType>(type)) ||
                     (kind == "DoubleSpinBox" && isa<FloatType>(type)));
  const bool slider = (kind == "Slider" || kind == "Dial") &&
                      link.getName() == "value" && isa<FloatType>(type);
  const bool toggle = (kind == "CheckBox" || kind == "Switch") &&
                      link.getName() == "checked" && isa<BoolType>(type);
  const bool checkState = kind == "CheckBox" && link.getName() == "check-state" && isa<CheckStateType>(type);
  if ((!text && !spin && !slider && !toggle && !checkState) || row.getBody().empty() || !item)
    return {};
  auto repeats = row.getBody().front().getOps<RepeatOp>();
  if (!llvm::hasSingleElement(repeats))
    return {};
  auto repeat = *repeats.begin();
  if (repeat.getItem() != item.getValue())
    return {};
  if (value != link.getInput()) {
    auto model = dyn_cast<RecordListType>(repeat.getInput().getType());
    auto itemType = dyn_cast<RecordType>(value.getType());
    if (!model || !itemType || itemType.getName() != model.getName())
      return {};
  } else if (!text || !table ||
             !isa<StringListType>(repeat.getInput().getType())) {
    return {};
  }
  return repeat.getInput().getDefiningOp<ReadOp>();
}

LogicalResult RowLinkOp::verify() {
  auto cell = cast<ElementOp>((*this)->getParentOp());
  if (!editableRowModel(*this))
    return emitOpError(
        isVirtualItemDelegate(cell)
            ? "requires a matching field of a writable list model"
        : enclosingTreeRow(cell)
            ? "requires a matching field of a writable tree model"
            : "requires the repeated string item or a matching "
              "field of a table model property");
  if (!cell.getId().empty())
    return emitOpError("row editors must be anonymous");
  for (Operation &op : cell.getBody().front()) {
    if (&op == getOperation() || isa<AnimateOp, ShowIfOp, EndOp>(op) ||
        isExpression(op))
      continue;
    if (isVirtualItemDelegate(cell)) {
      if (isa<RepeatOp, HandlerOp, StateSelectOp, StateSetOp, StateTimingOp>(op))
        continue;
      if (auto part = dyn_cast<ElementOp>(op);
          part && isVirtualSwitchPart(cell.getKind(), part))
        continue;
    }
    if (auto binding = dyn_cast<BindOp>(op);
        binding && binding.getName() != getName())
      continue;
    return emitOpError("row editors require leaf controls with one field link");
  }
  return success();
}

static LogicalResult verifyInstanceLayout(Operation *op, llvm::StringRef name, Type value) {
  auto *context = op->getContext();
  Type expected =
      llvm::StringSwitch<Type>(name)
          .Cases({"width", "height", "x", "y", "min-width", "min-height",
                  "max-width", "max-height", "preferred-width",
                  "preferred-height", "transform-origin-x", "transform-origin-y"},
                 LengthType::get(context))
          .Cases({"horizontal-stretch", "vertical-stretch", "z", "opacity",
                  "transform-rotation", "transform-scale", "transform-scale-x", "transform-scale-y"},
                 FloatType::get(context))
          .Case("transform", Transform2DType::get(context))
          .Cases({"visible", "cache-rendering-hint"}, BoolType::get(context))
          .Case("layout-direction", LayoutDirectionType::get(context))
          .Cases({"layout-order", "row", "col", "rowspan", "colspan"},
                 IntType::get(context))
          .Case("cross-axis-self-alignment",
                CrossAxisAlignmentType::get(context))
          .Default({});
  if (!expected)
    return op->emitOpError("unknown instance layout property ") << name;
  const bool relative = name != "x" && name != "y" &&
                        isa<LengthType>(expected) &&
                        isa<PercentType>(value);
  if (value != expected && !relative)
    return op->emitOpError("incorrect type for instance layout property ") << name;
  return success();
}

LogicalResult LayoutBindOp::verify() {
  return verifyInstanceLayout(*this, getName(), getInput().getType());
}

static LogicalResult verifyStatementBlock(Region &body, bool allowReturn) {
  for (Operation &child : body.front()) {
    if (isa<ReturnOp>(child) &&
        (!allowReturn || !isa_and_nonnull<EndOp>(child.getNextNode())))
      return child.emitOpError("must be immediately before xr.end in a callback");
    if (!isa<IfOp, ForOp, LocalOp, AssignOp, InvokeOp, EvaluateOp, LogOp, NavigateOp, TimerCommandOp, MediaCommandOp, PopupCommandOp,
             FileDialogOpenOp, NativeDialogOpenOp, WindowCloseOp, WindowCommandOp, CheckGroupCommandOp,
             SecondaryWindowCommandOp, FocusCommandOp, ButtonClickOp, ButtonToggleOp, SwipeCancelOp, TextCommandOp, ScrollCommandOp, PositionChildOp, SourceSelectionOp, TreeCommandOp, TableSortCommandOp, TableSelectionCommandOp, TableHistoryCommandOp, SplitRestoreOp, CancelFlickOp, ReturnToBoundsOp, MoveColumnOp, MoveRowOp, MoveNavigationItemOp, ModelEditOp,
             SetRowHeightOp, ReturnOp, EndOp>(child) &&
        !isExpression(child))
      return child.emitOpError("is not valid in a callback or event handler");
  }
  return success();
}

LogicalResult IfOp::verify() {
  if (failed(verifyStatementBlock(getThenBody(), false)))
    return failure();
  return verifyStatementBlock(getElseBody(), false);
}

LogicalResult ForOp::verify() {
  if (getItem().empty() || getItem() == getIndex())
    return emitOpError("requires a named item and a distinct optional index");
  return verifyStatementBlock(getBody(), false);
}

static LogicalResult verifyCallback(Operation *op, Region &body,
                                    llvm::StringRef parameters) {
  auto parsed = llvm::json::parse(parameters);
  if (!parsed) {
    llvm::consumeError(parsed.takeError());
    return op->emitOpError("parameters must be a JSON array");
  }
  auto *args = parsed->getAsArray();
  if (!args)
    return op->emitOpError("parameters must be a JSON array");
  if (auto handler = dyn_cast<HandlerOp>(op)) {
    auto element = cast<ElementOp>(op->getParentOp());
    auto kind = element.getKind();
    auto results = body.front().getOps<ReturnOp>();
    if (kind == "SystemTrayIcon") {
      if (handler.getName() != "clicked" || !args->empty() || !results.empty())
        return handler.emitOpError("SystemTrayIcon requires clicked() with no parameters");
    } else if (kind == "HoverHandler") {
      auto name = handler.getName();
      if ((name != "entered" && name != "moved" && name != "exited") ||
          !args->empty() || !results.empty())
        return handler.emitOpError("requires a zero-argument hover event");
    } else if (kind == "SwipeGestureHandler") {
      auto name = handler.getName();
      if ((name != "moved" && name != "swiped" && name != "cancelled") ||
          !args->empty() || !results.empty())
        return handler.emitOpError("requires a zero-argument swipe gesture event");
    } else if (kind == "ScaleRotateGestureHandler") {
      auto name = handler.getName();
      if ((name != "started" && name != "updated" && name != "ended" && name != "cancelled") ||
          !args->empty() || !results.empty())
        return handler.emitOpError("requires a zero-argument scale/rotate gesture event");
    } else if (handler.getName() == "current-cell-changed") {
      if (!isTableView(kind) || args->size() != 2 || !results.empty() ||
          !llvm::all_of(*args, [](const llvm::json::Value &value) {
            auto *arg = value.getAsObject();
            return arg && arg->getString("ty") == "int" && arg->getString("name");
          }))
        return handler.emitOpError("requires current-cell-changed(int, int) on a table");
    } else if (handler.getName() == "tab-close-requested") {
      if ((kind != "TabBar" && kind != "TabWidget") || args->size() != 2 || !results.empty())
        return handler.emitOpError("requires tab-close-requested(int, string) on a tab container");
      auto index = (*args)[0].getAsObject();
      auto value = (*args)[1].getAsObject();
      if (!index || index->getString("ty") != "int" || !index->getString("name") ||
          !value || value->getString("ty") != "string" || !value->getString("name"))
        return handler.emitOpError("requires tab-close-requested(int, string) on a tab container");
    } else if (handler.getName() == "tab-moved" || handler.getName() == "page-moved") {
      bool matchingKind = handler.getName() == "tab-moved" ? kind == "TabBar" || kind == "TabWidget"
                                                           : kind == "SwipeView";
      if (!matchingKind || args->size() != 2 || !results.empty() ||
          llvm::any_of(*args, [](const llvm::json::Value &arg) {
            auto *item = arg.getAsObject();
            return !item || item->getString("ty") != "int" || !item->getString("name");
          }))
        return handler.emitOpError("requires tab-moved(int, int) on a tab control or page-moved(int, int) on SwipeView");
    } else if (handler.getName() == "column-moved" || handler.getName() == "row-moved") {
      if (!isTableView(kind) || args->size() != (handler.getName() == "row-moved" ? 3 : 2) || !results.empty() ||
          !llvm::all_of(*args, [](const llvm::json::Value &value) {
            auto *arg = value.getAsObject();
            return arg && arg->getString("ty") == "int" && arg->getString("name");
          }))
        return handler.emitOpError(handler.getName() == "row-moved"
            ? "requires row-moved(int, int, int) on a table"
            : "requires column-moved(int, int) on a table");
    } else if (handler.getName() == "scrolled") {
      if (!isViewport(kind) || kind == "Flickable" || !args->empty() || !results.empty())
        return handler.emitOpError("requires scrolled() on a scrollable element");
    } else if (handler.getName() == "flicked") {
      if (kind != "Flickable" || !args->empty() || !results.empty())
        return handler.emitOpError("requires flicked() on Flickable");
    } else if (handler.getName() == "refresh-requested") {
      if (!isViewport(kind) || !args->empty() || !results.empty())
        return handler.emitOpError("requires refresh-requested() on a viewport");
    } else if (handler.getName() == "close-requested") {
      if (!isWindow(kind) || !isa<ComponentOp>(element->getParentOp()))
        return handler.emitOpError("close-requested requires the component's root Window");
      if (!args->empty() || !llvm::hasSingleElement(results) ||
          !isa<BoolType>((*results.begin()).getInput().getType()))
        return handler.emitOpError("close-requested requires no parameters and a bool result");
    } else if (handler.getName() == "leave-requested") {
      auto *destination = args->size() == 2 ? args->front().getAsObject() : nullptr;
      auto *removing = args->size() == 2 ? args->back().getAsObject() : nullptr;
      if (kind != "StackPage" || !destination || !removing ||
          destination->getString("ty") != "string" || !destination->getString("name") ||
          removing->getString("ty") != "bool" || !removing->getString("name") ||
          !llvm::hasSingleElement(results) || !isa<BoolType>((*results.begin()).getInput().getType()))
        return handler.emitOpError("StackPage requires leave-requested(string, bool) -> bool");
    } else if (llvm::StringSwitch<bool>(handler.getName())
                   .Cases({"capture-key-pressed", "capture-key-released", "key-pressed", "key-released"}, true)
                   .Default(false)) {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (kind != "FocusScope" || !arg || arg->getString("ty") != "KeyEvent" ||
          !arg->getString("name") || !llvm::hasSingleElement(results) ||
          !isa<EventResultType>((*results.begin()).getInput().getType()))
        return handler.emitOpError("FocusScope key handlers require (KeyEvent) -> EventResult");
    } else if (handler.getName() == "pointer-event") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (kind != "TouchArea" || !arg || arg->getString("ty") != "PointerEvent" ||
          !arg->getString("name") || !results.empty())
        return handler.emitOpError("TouchArea requires pointer-event(PointerEvent)");
    } else if (isPushButton(kind) &&
               (handler.getName() == "pressed" || handler.getName() == "released" ||
                handler.getName() == "canceled" || handler.getName() == "toggled")) {
      if (!args->empty() || !results.empty())
        return handler.emitOpError("requires a zero-argument button event");
    } else if (handler.getName() == "press-and-hold") {
      if ((kind != "TouchArea" && !isPushButton(kind)) || !args->empty() || !results.empty())
        return handler.emitOpError("requires press-and-hold() on TouchArea or a push button");
    } else if (handler.getName() == "double-clicked") {
      if ((kind != "TouchArea" && !isPushButton(kind)) || !args->empty() || !results.empty())
        return handler.emitOpError("requires double-clicked() on TouchArea or a push button");
    } else if (handler.getName() == "moved") {
      if (kind != "TouchArea" || !args->empty() || !results.empty())
        return handler.emitOpError("requires a zero-argument TouchArea event");
    } else if (handler.getName() == "wheel" || handler.getName() == "scroll-event") {
      const bool wheel = handler.getName() == "wheel";
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (kind != (wheel ? "WheelHandler" : "TouchArea") || !arg || arg->getString("ty") != "WheelEvent" ||
          !arg->getString("name") || !llvm::hasSingleElement(results) ||
          !isa<EventResultType>((*results.begin()).getInput().getType()))
        return handler.emitOpError(wheel
            ? "WheelHandler requires wheel(WheelEvent) -> EventResult"
            : "TouchArea requires scroll-event(WheelEvent) -> EventResult");
    } else if (!results.empty()) {
      return handler.emitOpError(isWindow(kind)
          ? "only close-requested may return a value on Window"
          : "this event cannot return a value");
    }
    if (llvm::StringSwitch<bool>(handler.getName())
            .Cases({"focus-gained", "focus-lost", "focus-changed-event", "focus-within-changed"}, true)
            .Default(false)) {
      bool within = handler.getName() == "focus-within-changed";
      auto argIs = [&](size_t index, llvm::StringRef type) {
        if (index >= args->size()) return false;
        auto *arg = (*args)[index].getAsObject();
        return arg && arg->getString("name") && arg->getString("ty") == type;
      };
      if (kind != "FocusScope" || args->size() != (within ? 2u : 1u) ||
          !argIs(within ? 1 : 0, "FocusReason") || (within && !argIs(0, "bool")))
        return handler.emitOpError("incorrect FocusScope focus event signature");
    }
    if (kind == "StyledText" || handler.getName() == "link-activated") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (kind != "StyledText" || handler.getName() != "link-activated" || !arg ||
          arg->getString("ty") != "string" || !arg->getString("name"))
        return handler.emitOpError("StyledText requires link-activated(string)");
    }
    if (kind == "AnimatedImage" && (handler.getName() != "finished" || !args->empty()))
      return handler.emitOpError("AnimatedImage requires finished() without parameters");
    if (kind == "Timer" && (handler.getName() != "triggered" || !args->empty()))
      return handler.emitOpError("Timer requires triggered() without parameters");
    if (kind == "SplitView" &&
        (!args->empty() || handler.getName() != "resized"))
      return handler.emitOpError(
          "SplitView requires resized() without parameters");
    if (kind == "Dialog" &&
        (!args->empty() ||
         !llvm::StringSwitch<bool>(handler.getName())
              .Cases({"accepted", "rejected", "applied", "reset", "help-requested", "discarded"}, true)
              .Default(false)))
      return handler.emitOpError(
          "requires a dialog event without parameters");
    if ((kind == "Drawer" || kind == "PopupWindow") && (!args->empty() || (handler.getName() != "opened" &&
                                                handler.getName() != "closed")))
      return handler.emitOpError(
          "Drawer requires opened() or closed() without parameters");
    if ((kind == "MenuItem" || kind == "Action" || kind == "DelayButton") &&
        (!args->empty() || handler.getName() != "activated"))
      return handler.emitOpError("requires activated() without parameters");
    if (kind == "ActionGroup") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (handler.getName() != "activated" || !arg || !arg->getString("name") ||
          arg->getString("ty") != "string" || !results.empty())
        return handler.emitOpError("requires activated(string) on an ActionGroup");
    }
    if (kind == "Shortcut" && (!args->empty() ||
        (handler.getName() != "activated" && handler.getName() != "activated-ambiguously")))
      return handler.emitOpError("Shortcut requires an activation handler without parameters");
    if (kind == "FileDialog") {
      auto name = handler.getName();
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (!(name == "canceled" && args->empty()) &&
          !(arg && arg->getString("name") &&
            ((name == "accepted" && arg->getString("ty") == "[string]") ||
             (name == "failed" && arg->getString("ty") == "string"))))
        return handler.emitOpError("FileDialog requires accepted([string]), canceled() or failed(string)");
    }
    if (kind == "MessageDialog") {
      auto name = handler.getName();
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (!((name == "accepted" || name == "rejected") && args->empty()) &&
          !(arg && arg->getString("name") &&
            ((name == "finished" && arg->getString("ty") == "StandardButtonKind") ||
             (name == "failed" && arg->getString("ty") == "string"))))
        return handler.emitOpError("invalid MessageDialog result handler or signature");
    }
    if (kind == "Expander") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (handler.getName() != "expanded-changed" || !arg ||
          arg->getString("ty") != "bool" || !arg->getString("name"))
        return handler.emitOpError("Expander requires expanded-changed(bool)");
    }
    if (kind == "GroupBox") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (handler.getName() != "toggled" || !arg ||
          arg->getString("ty") != "bool" || !arg->getString("name"))
        return handler.emitOpError("GroupBox requires toggled(bool)");
    }
    if (kind == "CheckBox") {
      auto name = handler.getName();
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (!(name == "toggled" && args->empty()) &&
          !(name == "check-state-changed" && arg &&
            arg->getString("ty") == "CheckState" && arg->getString("name")))
        return handler.emitOpError("CheckBox requires toggled() or check-state-changed(CheckState)");
    }
    const bool multiline = kind == "TextEdit" || kind == "TextArea";
    const bool table = isTableView(kind);
    const bool tree = kind == "TreeView";
    if (kind == "RangeSlider") {
      auto valid = [](const llvm::json::Value &value) {
        auto *arg = value.getAsObject();
        return arg && arg->getString("ty") == "float" && arg->getString("name");
      };
      if ((handler.getName() != "changed" && handler.getName() != "released") ||
          args->size() != 2 || !llvm::all_of(*args, valid))
        return handler.emitOpError("RangeSlider requires two float parameters");
    }
    if (kind == "DynamicContainer" && (handler.getName() != "mounted" || !args->empty()))
      return handler.emitOpError("DynamicContainer requires mounted()");
    if (kind == "Loader" && (handler.getName() != "loaded" || !args->empty()))
      return handler.emitOpError("Loader requires loaded()");
    if (kind == "StackPage" && handler.getName() != "leave-requested" &&
        (!llvm::StringSwitch<bool>(handler.getName())
              .Cases({"activated", "deactivated", "removed"}, true).Default(false) ||
         !args->empty()))
      return handler.emitOpError("invalid StackPage lifecycle event or signature");
    if (kind == "StackView") {
      auto *page = args->size() == 2 ? args->front().getAsObject() : nullptr;
      auto *depth = args->size() == 2 ? args->back().getAsObject() : nullptr;
      if (handler.getName() != "changed" || !page || !depth ||
          page->getString("ty") != "string" ||
          depth->getString("ty") != "int" || !page->getString("name") ||
          !depth->getString("name"))
        return handler.emitOpError("StackView requires changed(string, int)");
    }
    if (kind == "SwipeDelegate") {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (!(handler.getName() == "clicked" && args->empty()) &&
          !(handler.getName() == "changed" && arg && arg->getString("ty") == "SwipeSide" && arg->getString("name")))
        return handler.emitOpError("SwipeDelegate requires clicked() or changed(SwipeSide)");
    }
    if (handler.getName() == "files-failed" && kind != "DropArea")
      return handler.emitOpError("files-failed requires a DropArea");
    if (kind == "DragArea" || kind == "DropArea") {
      auto name = handler.getName();
      auto argIs = [&](size_t index, llvm::StringRef type) {
        if (index >= args->size())
          return false;
        auto *arg = (*args)[index].getAsObject();
        return arg && arg->getString("ty") == type && arg->getString("name");
      };
      bool valid = kind == "DragArea"
          ? (name == "drag-started" && args->empty()) ||
                (name == "drag-finished" && args->size() == 1 && argIs(0, "DragAction"))
          : (name == "exited" && args->empty()) ||
                ((name == "entered" || name == "dropped") && args->size() == 2 &&
                 argIs(0, "string") && argIs(1, "DragAction")) ||
                ((name == "files-entered" || name == "files-dropped") &&
                 args->size() == 1 && argIs(0, "[string]")) ||
                (name == "files-failed" && args->size() == 1 && argIs(0, "string")) ||
                (name == "position-changed" && args->size() == 2 &&
                 argIs(0, "length") && argIs(1, "length"));
      if (!valid)
        return handler.emitOpError("invalid drag-and-drop event or signature");
    }
    if (kind == "TimePicker" || isPickerPopup(kind)) {
      auto name = handler.getName();
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      auto type = llvm::StringSwitch<llvm::StringRef>(kind)
          .Case("DatePickerPopup", "Date")
          .Case("ColorPickerPopup", "color")
          .Case("FontPickerPopup", "Font")
          .Default("Time");
      if (!(isPickerPopup(kind) && name == "canceled" && args->empty()) &&
          !((kind == "TimePicker" ? name == "changed" : name == "accepted") &&
            arg && arg->getString("ty") == type && arg->getString("name")))
        return handler.emitOpError("invalid picker event or signature");
    }
    if (handler.getName() == "editing-finished") {
      auto *text = args->size() == 2 ? (*args)[0].getAsObject() : nullptr;
      auto *acceptable = args->size() == 2 ? (*args)[1].getAsObject() : nullptr;
      if (!(kind == "TextInput" || kind == "LineEdit" || kind == "ComboBox" || multiline) ||
          !text || text->getString("ty") != "string" || !text->getString("name") ||
          !acceptable || acceptable->getString("ty") != "bool" ||
          !acceptable->getString("name"))
        return handler.emitOpError("editing-finished requires (string, bool) on a text editor");
    }
    const bool scalarRange = kind == "Slider" || kind == "Dial" || kind == "ScrollBar";
    const bool calendar = kind == "Calendar" || kind == "MonthGrid";
    const bool search = kind == "SearchField";
    const bool spin = kind == "SpinBox" || kind == "DoubleSpinBox";
    const bool indexView = kind == "TabWidget" || kind == "TabBar" || kind == "SwipeView" ||
                           kind == "PageIndicator" || usesListTemplate(kind) ||
                           kind == "StandardListView" || kind == "Tumbler" || kind == "PathView";
    if (handler.getName() != "scrolled" && handler.getName() != "refresh-requested" &&
        handler.getName() != "editing-finished" &&
        handler.getName() != "column-moved" &&
        !((kind == "Tumbler" || kind == "PathView") && handler.getName() == "value-selected") &&
        handler.getName() != "tab-close-requested" && handler.getName() != "tab-moved" && handler.getName() != "page-moved" && handler.getName() != "row-moved" && handler.getName() != "current-cell-changed" && (scalarRange || spin ||
        (kind == "ComboBox" && handler.getName() != "value-selected") || table ||
        kind == "RadioGroup" || kind == "ButtonGroup" || indexView || multiline || tree || calendar || search)) {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      auto name = handler.getName();
      auto type = calendar ? "Date" : (scalarRange || kind == "DoubleSpinBox") ? "float"
                  : (kind == "SpinBox" || indexView || table || tree)
                      ? "int"
                      : "string";
      const bool known =
          calendar ? name == "changed" || name == "month-changed" ||
                     (kind == "MonthGrid" && name == "activated")
          : search ? name == "accepted" || name == "edited" || name == "selected" || name == "search-triggered"
          : scalarRange ? name == "changed" || (kind != "ScrollBar" && name == "released")
          : (spin || multiline) ? name == "edited"
          : kind == "ComboBox"
              ? name == "selected" || name == "edited" || name == "accepted"
          : indexView ? name == "changed" || (isSelectionList(kind) && name == "selection-changed")
          : tree      ? name == "changed" || name == "expanded" ||
                       name == "collapsed" || name == "selection-changed"
          : table
              ? name == "current-row-changed" || name == "selection-changed" ||
                    name == "sort-ascending" || name == "sort-descending"
              : name == "selected";
      if (!known || !arg || arg->getString("ty") != type ||
          !arg->getString("name"))
        return handler.emitOpError("control event requires one ")
               << type << " parameter";
    }
  }
  return verifyStatementBlock(body, true);
}

static bool canPlanCallbackDefault(CallbackOp callback, bool withParameters) {
  auto component = dyn_cast_or_null<ComponentOp>(callback->getParentOp());
  if (!component || callback.getPure().value_or("") != "true" ||
      (callback.getParameters() != "[]") != withParameters || callback.getAliasOwner())
    return false;
  bool hasReturn = false;
  for (auto &op : callback.getBody().front()) {
    if (auto result = dyn_cast<ReturnOp>(op)) {
      hasReturn = isa<IntType, FloatType, BoolType, StringType, LengthType>(
          result.getInput().getType());
      if (!hasReturn)
        return false;
    } else if (auto read = dyn_cast<ReadOp>(op)) {
      if (read.getOwner().value_or("root") != "root")
        return false;
      bool privateLiteral = llvm::any_of(component.getBody().front().getOps<PropertyOp>(),
          [&](PropertyOp property) {
            return property.getName() == read.getValue() &&
                   property.getVisibility() == "private" &&
                   property.getInitial().getDefiningOp<LiteralOp>();
          });
      if (!privateLiteral)
        return false;
    } else if (!isa<LiteralOp, ParamOp, UnaryOp, BinaryOp, ConcatOp, SelectOp, CastOp, EndOp>(op)) {
      return false;
    }
  }
  return hasReturn;
}

bool isCacheableCallbackDefault(CallbackOp callback) {
  return canPlanCallbackDefault(callback, false);
}

bool isInlineableCallbackDefault(CallbackOp callback) {
  return canPlanCallbackDefault(callback, true);
}

LogicalResult CallbackOp::verify() {
  if ((*this)->hasAttr("xr.inline_callback_default")) {
    auto plan = (*this)->getAttrOfType<StringAttr>("xr.inline_callback_default");
    if (!plan || plan.getValue() != "true" || !isInlineableCallbackDefault(*this))
      return emitOpError("invalid inline default callback plan");
  }
  if ((*this)->hasAttr("xr.cache_callback_default")) {
    auto cache = (*this)->getAttrOfType<StringAttr>("xr.cache_callback_default");
    if (!cache || cache.getValue() != "true" || !isCacheableCallbackDefault(*this))
      return emitOpError("invalid cached default callback plan");
  }
  if (getPure() && *getPure() != "true")
    return emitOpError("pure marker must be true");
  if (getAliasOwner().has_value() != getAliasName().has_value() ||
      (getAliasOwner() && (getAliasOwner()->empty() || getAliasName()->empty() ||
                           isa<InstanceOp>((*this)->getParentOp()))))
    return emitOpError("requires a named alias target on a callback declaration");
  if (getImplementation() && *getImplementation() != "true")
    return emitOpError("implementation marker must be true");
  if (getPure() && (*this)->walk([](AssignOp assign) {
        assign.emitOpError("assignment is not allowed in a pure callback");
        return WalkResult::interrupt();
      }).wasInterrupted())
    return failure();
  return verifyCallback(*this, getBody(), getParameters());
}

LogicalResult MathFunctionOp::verify() {
  auto operation = getValue();
  bool scalar = llvm::StringSwitch<bool>(operation)
                    .Cases({"sign", "sqrt", "pow", "exp", "ln", "log", "sin",
                            "cos", "tan", "asin", "acos", "atan", "atan2",
                            "hypot", "to-radians", "to-degrees", "is-finite"}, true)
                    .Default(false);
  unsigned arity = llvm::StringSwitch<unsigned>(operation)
                       .Case("clamp", 3)
                       .Cases({"min", "max", "mod", "pow", "log", "atan2", "hypot"}, 2)
                       .Cases({"abs", "floor", "ceil"}, 1)
                       .Default(scalar ? 1 : 0);
  if (!arity || getInputs().size() != arity)
    return emitOpError("unknown math function or wrong argument count");
  Type input = getInputs().front().getType();
  Type result = getResult().getType();
  if (!llvm::all_of(getInputs(), [&](Value value) { return value.getType() == input; }))
    return emitOpError("expected matching argument types");
  if (operation == "floor" || operation == "ceil") {
    if (!isa<FloatType>(input) || !isa<IntType>(result))
      return emitOpError("expected float input and int result");
  } else if (scalar) {
    if (!isa<FloatType>(input) ||
        (operation == "is-finite" ? !isa<BoolType>(result) : input != result))
      return emitOpError("invalid scalar math argument or result type");
  } else if (input != result) {
    return emitOpError("expected matching argument and result types");
  }
  return success();
}

void GeometryFunctionOp::getEffects(
    llvm::SmallVectorImpl<MemoryEffects::EffectInstance> &effects) {
  // Scene queries read mutable models, including nested vertex and index
  // tables. An unchanged SSA model handle does not imply unchanged geometry.
  if (llvm::any_of(getInputs(), [](Value input) {
        return isa<RecordListType, IntListType>(input.getType());
      }))
    effects.emplace_back(MemoryEffects::Read::get());
}

LogicalResult GeometryFunctionOp::verify() {
  Type point = Point2DType::get(getContext());
  Type worldPoint = Point3DType::get(getContext());
  Type worldVector = Vector3DType::get(getContext());
  Type hit = Hit3DType::get(getContext());
  Type hitList = Hit3DListType::get(getContext());
  Type projection = ProjectedPoint3DType::get(getContext());
  Type vector = Vector2DType::get(getContext());
  Type transform = Transform2DType::get(getContext());
  Type length = LengthType::get(getContext());
  Type number = FloatType::get(getContext());
  llvm::SmallVector<Type, 2> inputs;
  Type output = transform;
  auto operation = getValue();
  if (operation == "point" || operation == "vector") {
    inputs = {length, length};
    output = operation == "point" ? point : vector;
  } else if (operation == "translation")
    inputs = {length, length};
  else if (operation == "rotation")
    inputs = {number};
  else if (operation == "scale")
    inputs = {number, number};
  else if (operation == "compose")
    inputs = {transform, transform};
  else if (operation == "around")
    inputs = {transform, point};
  else if (operation == "map-point") {
    inputs = {transform, point};
    output = point;
  } else if (operation == "map-vector") {
    inputs = {transform, vector};
    output = vector;
  } else if (operation == "point-3d" || operation == "vector-3d") {
    inputs = {number, number, number};
    output = operation == "point-3d" ? worldPoint : worldVector;
  } else if (operation == "project-3d") {
    inputs = {number, number, number, point, worldPoint};
    output = projection;
  } else if (operation == "unproject-3d") {
    inputs = {number, number, number, point, point, number, worldPoint};
    output = worldPoint;
  } else if (operation == "pick-3d" || operation == "pick-camera-3d" ||
             operation == "pick-in-rect-3d" || operation == "pick-in-rect-camera-3d" ||
             operation == "hit-row-3d" || operation == "hit-row-camera-3d" || operation == "ray-hit-row-3d" ||
             operation == "hit-instance-3d" || operation == "hit-instance-camera-3d" ||
             operation == "ray-hit-instance-3d" ||
             operation == "hit-subset-3d" || operation == "hit-subset-camera-3d" || operation == "ray-hit-subset-3d" ||
             operation == "project-camera-3d" || operation == "unproject-camera-3d" ||
             operation == "map-position-3d" || operation == "map-direction-3d" ||
             operation == "hit-3d" || operation == "hit-camera-3d" || operation == "ray-hit-3d" ||
             operation == "hit-all-3d" || operation == "hit-all-camera-3d" || operation == "ray-hit-all-3d" ||
             operation == "closest-point-3d" || operation == "closest-point-on-3d" ||
             operation == "sphere-cast-3d" || operation == "sphere-overlap-3d" || operation == "sphere-slide-3d" || operation == "sphere-walk-3d") {
    if (getInputs().empty() || !isa<RecordListType>(getInputs().front().getType()))
      return emitOpError("3D query requires a scene object record model");
    auto model = getInputs().front().getType();
    auto string = StringType::get(getContext());
    if (operation == "hit-3d" || operation == "hit-all-3d") {
      inputs = {model, number, number, number, point, point};
      output = operation == "hit-3d" ? hit : hitList;
    } else if (operation == "hit-camera-3d" || operation == "hit-all-camera-3d") {
      inputs = {model, string, point, point};
      output = operation == "hit-camera-3d" ? hit : hitList;
    } else if (operation == "ray-hit-3d" || operation == "ray-hit-all-3d") {
      inputs = {model, worldPoint, worldVector, number};
      output = operation == "ray-hit-3d" ? hit : hitList;
    } else if (operation == "hit-row-3d" || operation == "hit-subset-3d" ||
               operation == "hit-row-camera-3d" || operation == "hit-subset-camera-3d" ||
               operation == "ray-hit-row-3d" || operation == "ray-hit-subset-3d" ||
               operation == "hit-instance-3d" || operation == "hit-instance-camera-3d" ||
               operation == "ray-hit-instance-3d") {
      bool subset = operation.contains("-subset-");
      Type selector = subset ? Type(IntListType::get(getContext())) : Type(IntType::get(getContext()));
      if (operation.starts_with("ray-"))
        inputs = {model, worldPoint, worldVector, number, selector};
      else if (operation.ends_with("-camera-3d"))
        inputs = {model, string, point, point, selector};
      else
        inputs = {model, number, number, number, point, point, selector};
      if (operation.contains("-instance-"))
        inputs.append(2, IntType::get(getContext()));
      output = subset ? hitList : hit;
    } else if (operation == "pick-in-rect-3d" || operation == "pick-in-rect-camera-3d") {
      if (operation == "pick-in-rect-3d")
        inputs = {model, number, number, number, point, point, point};
      else
        inputs = {model, string, point, point, point};
      output = IntListType::get(getContext());
    } else if (operation == "closest-point-3d") {
      inputs = {model, worldPoint, number};
      output = hit;
    } else if (operation == "closest-point-on-3d") {
      inputs = {model, string, worldPoint, number};
      output = hit;
    } else if (operation == "sphere-cast-3d") {
      inputs = {model, worldPoint, worldVector, number, number};
      output = hit;
    } else if (operation == "sphere-overlap-3d") {
      inputs = {model, worldPoint, number};
      output = hitList;
    } else if (operation == "sphere-slide-3d") {
      inputs = {model, worldPoint, worldVector, number};
      output = worldPoint;
    } else if (operation == "sphere-walk-3d") {
      inputs = {model, worldPoint, worldVector, number, number, number, number};
      output = worldPoint;
    } else if (operation == "map-position-3d" || operation == "map-direction-3d") {
      output = operation == "map-position-3d" ? worldPoint : worldVector;
      inputs = {model, string, string, output, output};
    } else if (operation == "project-camera-3d") {
      inputs = {model, string, point, worldPoint};
      output = projection;
    } else if (operation == "unproject-camera-3d") {
      inputs = {model, string, point, point, number, worldPoint};
      output = worldPoint;
    } else {
      output = string;
      if (operation == "pick-camera-3d")
        inputs = {model, string, point, point};
      else
        inputs = {model, number, number, number, point, point};
    }
  } else if (operation != "identity") {
    return emitOpError("unknown geometry function");
  }
  if (getInputs().size() == inputs.size() + 1 &&
      llvm::StringSwitch<bool>(operation)
          .Cases({"pick-3d", "pick-camera-3d", "hit-3d", "hit-camera-3d", "hit-all-3d",
                  "hit-all-camera-3d", "ray-hit-3d", "ray-hit-all-3d",
                  "pick-in-rect-3d", "pick-in-rect-camera-3d",
                  "hit-row-3d", "hit-row-camera-3d", "ray-hit-row-3d",
                  "hit-instance-3d", "hit-instance-camera-3d", "ray-hit-instance-3d",
                  "hit-subset-3d", "hit-subset-camera-3d", "ray-hit-subset-3d",
                  "closest-point-3d", "closest-point-on-3d", "sphere-cast-3d", "sphere-overlap-3d", "sphere-slide-3d", "sphere-walk-3d"}, true)
          .Default(false))
    inputs.push_back(IntType::get(getContext()));
  if (!llvm::equal(getInputs().getTypes(), inputs) || getResult().getType() != output)
    return emitOpError("invalid geometry function signature");
  return success();
}

LogicalResult Point2DFieldOp::verify() {
  if (getValue() != "x" && getValue() != "y")
    return emitOpError("expected a point coordinate x or y");
  return success();
}

LogicalResult Point3DFieldOp::verify() {
  if (getValue() != "x" && getValue() != "y" && getValue() != "z")
    return emitOpError("expected a world coordinate x, y or z");
  return success();
}

LogicalResult Vector3DFieldOp::verify() {
  if (getValue() != "x" && getValue() != "y" && getValue() != "z")
    return emitOpError("expected a vector coordinate x, y or z");
  return success();
}

LogicalResult Hit3DFieldOp::verify() {
  auto type = getResult().getType();
  auto field = getValue();
  if ((field == "valid" && isa<BoolType>(type)) ||
      (field == "object-id" && isa<StringType>(type)) ||
      ((field == "row-index" || field == "mesh-node-index" || field == "instance-index") &&
       isa<IntType>(type)) ||
      ((field == "distance" || field == "u" || field == "v") && isa<FloatType>(type)) ||
      ((field == "position" || field == "local-position") && isa<Point3DType>(type)) ||
      ((field == "normal" || field == "local-normal") && isa<Vector3DType>(type)))
    return success();
  return emitOpError("invalid hit field or result type");
}

LogicalResult ProjectedPoint3DFieldOp::verify() {
  auto type = getResult().getType();
  auto field = getValue();
  if ((field == "point" && isa<Point2DType>(type)) ||
      (field == "depth" && isa<FloatType>(type)) ||
      ((field == "valid" || field == "inside") && isa<BoolType>(type)))
    return success();
  return emitOpError("invalid projected point field or result type");
}

LogicalResult Vector2DFieldOp::verify() {
  if (getValue() != "x" && getValue() != "y")
    return emitOpError("expected a vector coordinate x or y");
  return success();
}

LogicalResult StringFunctionOp::verify() {
  auto operation = getValue();
  unsigned arity = llvm::StringSwitch<unsigned>(operation)
                       .Case("replace-all", 3)
                       .Cases({"contains", "starts-with", "ends-with", "to-int", "to-float"}, 2)
                       .Cases({"trim", "is-empty", "character-count", "lowercase",
                               "uppercase", "is-int", "is-float"}, 1)
                       .Default(0);
  if (!arity || getInputs().size() != arity)
    return emitOpError("unknown string function or wrong argument count");
  auto *context = getContext();
  auto string = StringType::get(context);
  Type result = llvm::StringSwitch<Type>(operation)
                    .Cases({"trim", "replace-all", "lowercase", "uppercase"}, string)
                    .Cases({"character-count", "to-int"}, IntType::get(context))
                    .Case("to-float", FloatType::get(context))
                    .Default(BoolType::get(context));
  if (result != getResult().getType())
    return emitOpError("invalid string function result type");
  for (auto [index, input] : llvm::enumerate(getInputs())) {
    bool fallback = index == 1 && (operation == "to-int" || operation == "to-float");
    if (input.getType() != (fallback ? result : Type(string)))
      return emitOpError("invalid string function operand type");
  }
  return success();
}

LogicalResult ColorFunctionOp::verify() {
  auto *context = getContext();
  auto color = ColorType::get(context);
  auto scalar = FloatType::get(context);
  auto integer = IntType::get(context);
  llvm::SmallVector<Type, 4> expected;
  auto operation = getValue();
  if (operation == "rgb")
    expected = {integer, integer, integer};
  else if (operation == "rgba")
    expected = {integer, integer, integer, scalar};
  else if (operation == "with-alpha")
    expected = {color, scalar};
  else if (operation == "mix")
    expected = {color, color, scalar};
  if (expected.empty() || !llvm::equal(getInputs().getTypes(), expected))
    return emitOpError("unknown color function or invalid operands");
  return success();
}

LogicalResult FormatFixedOp::verify() {
  unsigned precision;
  if (getValue().getAsInteger(10, precision) || precision > 20)
    return emitOpError("fixed precision must be between 0 and 20");
  return success();
}

LogicalResult LogOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure())
    return emitOpError("logging is not allowed in a pure callback");
  return success();
}

LogicalResult HandlerOp::verify() {
  return verifyCallback(*this, getBody(), getParameters());
}

std::optional<double> parseFloatLiteral(llvm::StringRef spelling) {
  double value;
  if (spelling.getAsDouble(value) || !std::isfinite(value))
    return std::nullopt;
  return value;
}

std::optional<llvm::SmallVector<double, 6>>
parseGeometryLiteral(Type type, llvm::StringRef spelling) {
  unsigned size = isa<Transform2DType>(type) ? 6
                  : isa<Point3DType, Vector3DType>(type) ? 3
                  : isa<Point2DType, Vector2DType>(type) ? 2
                                                       : 0;
  if (size == 0)
    return std::nullopt;
  llvm::SmallVector<llvm::StringRef, 6> parts;
  spelling.split(parts, ' ', -1, false);
  if (parts.size() != size)
    return std::nullopt;
  llvm::SmallVector<double, 6> values;
  for (auto part : parts) {
    auto value = parseFloatLiteral(part);
    if (!value)
      return std::nullopt;
    values.push_back(*value);
  }
  return values;
}

LogicalResult LiteralOp::verify() {
  auto type = getResult().getType();
  auto spelling = getValue();
  bool valid = false;
  if (isa<IntType>(type)) {
    int64_t value;
    valid = !spelling.getAsInteger(10, value);
  } else if (isa<FloatType, LengthType, PercentType>(type)) {
    valid = parseFloatLiteral(spelling).has_value();
  } else if (isa<Point2DType, Vector2DType, Transform2DType, Point3DType, Vector3DType>(type)) {
    valid = parseGeometryLiteral(type, spelling).has_value();
  } else if (isa<ProjectedPoint3DType, Hit3DType>(type)) {
    valid = spelling.empty();
  } else if (isa<BoolType>(type)) {
    valid = spelling == "true" || spelling == "false";
  } else if (isa<LogLevelType>(type)) {
    valid = llvm::StringSwitch<bool>(spelling)
                .Cases({"trace", "debug", "info", "warn", "error"}, true)
                .Default(false);
  } else if (isa<ImageFitType>(type)) {
    valid = spelling == "fill" || spelling == "contain" ||
            spelling == "cover" || spelling == "preserve";
  } else if (isa<ImageTileModeType>(type)) {
    valid = spelling == "stretch" || spelling == "repeat" || spelling == "round";
  } else if (isa<ImageHorizontalAlignmentType>(type)) {
    valid = spelling == "left" || spelling == "center" || spelling == "right";
  } else if (isa<ImageVerticalAlignmentType>(type)) {
    valid = spelling == "top" || spelling == "center" || spelling == "bottom";
  } else if (isa<ImageRenderingType>(type)) {
    valid = spelling == "smooth" || spelling == "pixelated";
  } else if (isa<ButtonDisplayType>(type)) {
    valid = spelling == "icon-only" || spelling == "text-only" ||
            spelling == "text-beside-icon" || spelling == "text-under-icon";
  } else if (isa<LayoutAlignmentType>(type)) {
    valid = spelling == "stretch" || spelling == "center" ||
            spelling == "start" || spelling == "end" ||
            spelling == "space-between" || spelling == "space-around" ||
            spelling == "space-evenly";
  } else if (isa<CrossAxisAlignmentType>(type)) {
    valid = spelling == "auto" || spelling == "stretch" ||
            spelling == "start" || spelling == "end" || spelling == "center";
  } else if (isa<OrientationType>(type)) {
    valid = spelling == "horizontal" || spelling == "vertical";
  } else if (isa<FlickDirectionType>(type)) {
    valid = spelling == "both" || spelling == "horizontal" || spelling == "vertical";
  } else if (isa<FlickBoundsBehaviorType>(type)) {
    valid = spelling == "stop-at-bounds" || spelling == "drag-over-bounds" ||
            spelling == "overshoot-bounds" || spelling == "drag-and-overshoot-bounds";
  } else if (isa<FlickBoundsMovementType>(type)) {
    valid = spelling == "follow-bounds-behavior" || spelling == "stop-at-bounds";
  } else if (isa<LayoutDirectionType>(type)) {
    valid = spelling == "left-to-right" || spelling == "right-to-left";
  } else if (isa<FlexboxLayoutDirectionType>(type)) {
    valid = spelling == "row" || spelling == "row-reverse" ||
            spelling == "column" || spelling == "column-reverse";
  } else if (isa<FlexboxLayoutWrapType>(type)) {
    valid = spelling == "wrap" || spelling == "no-wrap" || spelling == "wrap-reverse";
  } else if (isa<DialInputModeType>(type)) {
    valid = spelling == "circular" || spelling == "horizontal" ||
            spelling == "vertical";
  } else if (isa<ScrollBarPolicyType>(type)) {
    valid = spelling == "as-needed" || spelling == "always-on" ||
            spelling == "always-off";
  } else if (isa<ScrollSnapModeType>(type)) {
    valid = spelling == "none" || spelling == "always" || spelling == "on-release";
  } else if (isa<ScrollAlignmentType>(type)) {
    valid = spelling == "beginning" || spelling == "center" ||
            spelling == "end" || spelling == "visible" || spelling == "contain";
  } else if (isa<TextWrapType>(type)) {
    valid = spelling == "word-wrap" || spelling == "no-wrap" ||
            spelling == "char-wrap";
  } else if (isa<MouseSelectionModeType>(type)) {
    valid = spelling == "characters" || spelling == "words" || spelling == "caret";
  } else if (isa<InputValidityType>(type)) {
    valid = spelling == "invalid" || spelling == "intermediate" || spelling == "acceptable";
  } else if (isa<TextHorizontalAlignmentType>(type)) {
    valid = spelling == "start" || spelling == "end" || spelling == "left" ||
            spelling == "center" || spelling == "right";
  } else if (isa<TextVerticalAlignmentType>(type)) {
    valid = spelling == "top" || spelling == "center" || spelling == "bottom";
  } else if (isa<TextOverflowType>(type)) {
    valid = spelling == "clip" || spelling == "elide";
  } else if (isa<CursorShapeType>(type)) {
    valid = llvm::StringSwitch<bool>(spelling)
                .Cases({"auto", "default", "pointer", "text", "crosshair", "move",
                        "grab", "grabbing", "not-allowed", "wait", "help",
                        "resize-horizontal", "resize-vertical", "zoom-in", "zoom-out"}, true)
                .Default(false);
  } else if (isa<InputModeType>(type)) {
    valid = spelling == "text" || spelling == "password" || spelling == "number" ||
            spelling == "decimal" || spelling == "search" || spelling == "email" ||
            spelling == "url" || spelling == "tel";
  } else if (isa<InputPurposeType>(type)) {
    valid = spelling == "auto" || spelling == "username" || spelling == "current-password" ||
            spelling == "new-password" || spelling == "one-time-code";
  } else if (isa<KeyboardCapitalizationType>(type)) {
    valid = spelling == "auto" || spelling == "none" || spelling == "words" ||
            spelling == "sentences" || spelling == "characters";
  } else if (isa<ReturnKeyType>(type)) {
    valid = spelling == "auto" || spelling == "next" || spelling == "done";
  } else if (isa<AccessibleLiveType>(type)) {
    valid = spelling == "off" || spelling == "polite" || spelling == "assertive";
  } else if (isa<ColorSchemeType>(type)) {
    valid = spelling == "system" || spelling == "light" || spelling == "dark";
  } else if (isa<SceneToneMapType>(type)) {
    valid = spelling == "none" || spelling == "linear" || spelling == "reinhard" ||
            spelling == "aces" || spelling == "hejl-dawson" || spelling == "filmic";
  } else if (isa<FileDialogModeType>(type)) {
    valid = spelling == "open-file" || spelling == "open-files" ||
            spelling == "save-file" || spelling == "select-folder" || spelling == "select-folders";
  } else if (isa<MessageLevelType>(type)) {
    valid = spelling == "info" || spelling == "warning" || spelling == "error";
  } else if (isa<MessageButtonsType>(type)) {
    valid = spelling == "ok" || spelling == "ok-cancel" ||
            spelling == "yes-no" || spelling == "yes-no-cancel";
  } else if (isa<CheckStateType>(type)) {
    valid = spelling == "unchecked" || spelling == "partially-checked" ||
            spelling == "checked";
  } else if (isa<DialogRoleType>(type)) {
    valid = spelling == "accept" || spelling == "reject";
  } else if (isa<PointerEventKindType>(type)) {
    valid = spelling == "down" || spelling == "move" || spelling == "up" || spelling == "cancel";
  } else if (isa<PointerButtonType>(type)) {
    valid = spelling == "primary" || spelling == "secondary" || spelling == "middle";
  } else if (isa<KeyEventType, KeyModifiersType, WheelEventType, PointerEventType, TouchPointType, TouchPointsType>(type)) {
    valid = spelling.empty();
  } else if (isa<FocusReasonType>(type)) {
    valid = llvm::StringSwitch<bool>(spelling)
                .Cases({"programmatic", "tab-navigation", "pointer-click", "popup-activation",
                        "window-activation", "accessibility", "layout-change", "keyboard-navigation"}, true)
                .Default(false);
  } else if (isa<EventResultType>(type)) {
    valid = spelling == "accept" || spelling == "reject";
  } else if (isa<DragActionType>(type)) {
    valid = spelling == "none" || spelling == "copy" || spelling == "move" ||
            spelling == "link";
  } else if (isa<SwipeSideType>(type)) {
    valid = spelling == "closed" || spelling == "left" || spelling == "right";
  } else if (isa<StandardButtonKindType>(type)) {
    valid = llvm::StringSwitch<bool>(spelling)
                .Cases({"ok", "open", "save", "save-all", "cancel", "close",
                        "apply", "reset", "restore-defaults", "help", "yes",
                        "yes-to-all", "no", "no-to-all", "abort", "retry",
                        "ignore", "discard"}, true)
                .Default(false);
  } else if (isa<DrawerEdgeType>(type)) {
    valid = spelling == "left" || spelling == "right" || spelling == "top" ||
            spelling == "bottom";
  } else if (isa<PopupPlacementType>(type)) {
    valid = spelling == "below" || spelling == "above" || spelling == "left" ||
            spelling == "right" || spelling == "offset";
  } else if (isa<SelectionTargetType>(type)) {
    valid = spelling == "rows" || spelling == "cells" || spelling == "columns";
  } else if (isa<SelectionGestureType>(type)) {
    valid = spelling == "auto" || spelling == "drag" || spelling == "press-and-hold";
  } else if (isa<StringType>(type)) {
    auto value = llvm::json::parse(spelling);
    if (value)
      valid = value->getAsString().has_value();
    else
      llvm::consumeError(value.takeError());
  } else if (isa<ColorType>(type)) {
    const auto length = spelling.size();
    valid = (length == 4 || length == 5 || length == 7 || length == 9) &&
            spelling.consume_front("#") &&
            spelling.find_first_not_of("0123456789abcdefABCDEF") ==
                llvm::StringRef::npos;
  }
  if (!valid)
    return emitOpError("literal spelling does not match result type ") << type;
  return success();
}

LogicalResult UnaryOp::verify() {
  auto type = getInput().getType();
  auto op = getValue();
  if (op == "!" ? isa<BoolType>(type)
                : (op == "+" || op == "-") && isa<IntType, FloatType, LengthType>(type))
    return success();
  return emitOpError("invalid operand or result type for operator '") << op << "'";
}

LogicalResult BinaryOp::verify() {
  auto left = getLhs().getType(), right = getRhs().getType();
  auto op = getValue();
  Type expected;
  if (op == "==" || op == "!=") {
    if (left == right)
      expected = BoolType::get(getContext());
  } else if (op == "<" || op == "<=" || op == ">" || op == ">=") {
    if (left == right && isa<IntType, FloatType, LengthType, DateType, TimeType>(left))
      expected = BoolType::get(getContext());
  } else if (op == "&&" || op == "||") {
    if (left == right && isa<BoolType>(left))
      expected = left;
  } else if (op == "%") {
    if (left == right && isa<IntType>(left))
      expected = left;
  } else if (op == "+" || op == "-" || op == "*" || op == "/") {
    if (left == right && isa<IntType, FloatType>(left)) {
      expected = left;
    } else if (left == right && isa<LengthType>(left)) {
      if (op == "+" || op == "-")
        expected = left;
      else if (op == "/")
        expected = FloatType::get(getContext());
    } else if ((op == "*" || op == "/") && isa<LengthType>(left) && isa<FloatType>(right)) {
      expected = left;
    } else if (op == "*" && isa<FloatType>(left) && isa<LengthType>(right)) {
      expected = right;
    } else if (op == "+" && left == right && isa<StringType>(left)) {
      expected = left;
    }
  }
  if (expected == getResult().getType())
    return success();
  return emitOpError("invalid operand or result type for operator '") << op << "'";
}

OpFoldResult SelectOp::fold(FoldAdaptor) {
  // The backend evaluates expressions at their use site. Equal arms do not
  // eliminate the need to evaluate a condition that may read or call.
  if (auto constant = getCondition().getDefiningOp<LiteralOp>()) {
    if (constant.getValue() == "true")
      return getOnTrue();
    if (constant.getValue() == "false")
      return getOnFalse();
  }
  return {};
}

static std::optional<double> gradientOffset(GradientStopOp stop) {
  auto literal = stop.getOffset().getDefiningOp<LiteralOp>();
  auto offset = literal ? parseFloatLiteral(literal.getValue()) : std::nullopt;
  if (!offset || *offset < 0.0 || *offset > 1.0)
    return std::nullopt;
  return offset;
}

LogicalResult GradientStopOp::verify() {
  if (!gradientOffset(*this))
    return emitOpError("offset must be a constant between 0 and 1");
  return success();
}

static LogicalResult verifyGradient(Operation *op, ValueRange stops) {
  if (stops.empty())
    return op->emitOpError("requires at least one color stop");
  double previous = 0.0;
  for (Value value : stops) {
    auto stop = value.getDefiningOp<GradientStopOp>();
    auto offset = stop ? gradientOffset(stop) : std::nullopt;
    if (!offset || *offset < previous)
      return op->emitOpError(
          "requires ordered gradient stops with constant offsets");
    previous = *offset;
  }
  return success();
}

LogicalResult LinearGradientOp::verify() {
  return verifyGradient(*this, getStops());
}

LogicalResult RadialGradientOp::verify() {
  return verifyGradient(*this, getStops());
}

using Components = llvm::StringMap<ComponentOp>;
using Owners = llvm::StringMap<Operation *>;

llvm::StringRef scopedOwnerKey(Operation *op) {
  if (auto element = dyn_cast<ElementOp>(op)) {
    if (!isa_and_nonnull<ComponentOp>(op->getParentOp()))
      return element.getKey().value_or(element.getId());
  } else if (auto instance = dyn_cast<InstanceOp>(op)) {
    return instance.getKey();
  }
  return {};
}

static bool staticPropertyOwner(Operation *op) {
  for (; op && !isa<ComponentOp>(op); op = op->getParentOp())
    if (isa<ElementOp, InstanceOp>(op) &&
        !op->getRegion(0).front().getOps<RepeatOp>().empty())
      return false;
  return true;
}

static bool controlStateAvailable(ElementOp target, Operation *use) {
  for (auto owner = target; owner; owner = owner->getParentOfType<ElementOp>()) {
    auto repeats = owner.getBody().front().getOps<RepeatOp>();
    if (repeats.empty())
      continue;
    auto repeat = *repeats.begin();
    auto *scope = owner.getOperation();
    for (auto *site = use; site; site = site->getParentOp())
      if (site->getParentOp() == scope)
        return repeat->isBeforeInBlock(site);
    return false;
  }
  return true;
}

static bool overlayGeometryDomain(Operation *op) {
  for (; op; op = op->getParentOp())
    if (auto element = dyn_cast<ElementOp>(op);
        element && (element.getKind() == "PopupWindow" || element.getKind() == "Tooltip"))
      return true;
  return false;
}

static LogicalResult verifyDirectGeometryFlow(Operation *op, const Owners &owners) {
  auto dimension = [](llvm::StringRef name) {
    return llvm::StringSwitch<bool>(name)
        .Cases({"x", "y", "width", "height", "min-width", "min-height",
                "preferred-width", "preferred-height", "max-width", "max-height",
                "visible", "spacing", "layout-order", "layout-direction", "row", "col", "orientation",
                "content-width", "content-height", "viewport-height", "content-x", "content-y",
                "reserve-scroll-bar-space", "vertical-scroll-bar-policy", "horizontal-scroll-bar-policy"}, true)
        .Default(false) || name.starts_with("padding-") ||
        name.starts_with("transform-") || name == "padding";
  };
  llvm::SmallVector<Value> values;
  if (auto bind = dyn_cast<BindOp>(op); bind && dimension(bind.getName()))
    values.push_back(bind.getInput());
  else if (auto bind = dyn_cast<LayoutBindOp>(op); bind && dimension(bind.getName()))
    values.push_back(bind.getInput());
  else if (auto link = dyn_cast<ControlLinkOp>(op); link && dimension(link.getName()))
    values.push_back(link.getInput());
  else if (auto state = dyn_cast<StateSetOp>(op); state && dimension(state.getName()))
    llvm::append_range(values, state->getOperands());
  else if (auto condition = dyn_cast<ShowIfOp>(op))
    values.push_back(condition.getInput());
  else if (auto repeat = dyn_cast<RepeatOp>(op))
    values.push_back(repeat.getInput());

  llvm::DenseSet<Operation *> seen;
  while (!values.empty()) {
    auto *definition = values.pop_back_val().getDefiningOp();
    if (!definition || !seen.insert(definition).second)
      continue;
    Operation *geometrySource = nullptr;
    if (auto read = dyn_cast<GeometryReadOp>(definition)) {
      geometrySource = owners.lookup(read.getOwner());
    } else if (auto read = dyn_cast<ViewportReadOp>(definition); read && isScrollBarExtent(read.getValue())) {
      geometrySource = owners.lookup(read.getOwner());
    } else if (auto read = dyn_cast<ScrollControlReadOp>(definition)) {
      if (auto bar = dyn_cast_or_null<ElementOp>(owners.lookup(read.getOwner()));
          bar && !bar.getBody().empty()) {
        auto targets = bar.getBody().front().getOps<ScrollTargetOp>();
        if (isScrollThumbMetric(read.getValue()) || isScrollStateMetric(bar.getKind(), read.getValue()))
          geometrySource = bar;
        else if (llvm::hasSingleElement(targets))
          geometrySource = owners.lookup((*targets.begin()).getTarget());
      }
    }
    if (geometrySource && (!overlayGeometryDomain(op) || overlayGeometryDomain(geometrySource)))
      return op->emitOpError("layout feedback from a geometry read");
    llvm::append_range(values, definition->getOperands());
  }
  return success();
}

static bool validNumericControlValue(llvm::StringRef kind, llvm::StringRef name, Type type) {
  if (name == "value")
    return (kind == "SpinBox" && isa<IntType>(type)) ||
        ((kind == "Slider" || kind == "Dial" || kind == "DoubleSpinBox") &&
         isa<FloatType>(type));
  return kind == "RangeSlider" && isa<FloatType>(type) &&
      (name == "lower-value" || name == "upper-value");
}

static bool validControlAlias(ElementOp element, llvm::StringRef name, Type type) {
  if (!element || !staticPropertyOwner(element))
    return false;
  auto kind = element.getKind();
  if (name == "text" && isa<StringType>(type))
    return llvm::StringSwitch<bool>(kind)
        .Cases({"TextInput", "LineEdit", "SearchField", "TextEdit", "TextArea"}, true)
        .Default(false);
  if (name == "checked" && isa<BoolType>(type)) {
    if (kind == "Switch")
      return true;
    return kind == "CheckBox" &&
        llvm::none_of(element.getBody().front(), [](Operation &op) {
          auto name = op.getAttrOfType<StringAttr>("name");
          return isa<BindOp, ControlLinkOp>(op) && name &&
              (name.getValue() == "tristate" || name.getValue() == "check-state");
        });
  }
  return validNumericControlValue(kind, name, type);
}

static bool writableFromParent(llvm::StringRef visibility) {
  return visibility == "in" || visibility == "in-out";
}

static LogicalResult verifyReference(Operation *op, const Owners &owners,
                                     const Components &components,
                                     llvm::StringRef owner,
                                     llvm::StringRef name, Type type,
                                     bool write, bool requireInOut = false) {
  Operation *scope = owners.lookup(owner);
  if (!scope)
    return op->emitOpError("unknown property owner ") << owner;
  auto instance = dyn_cast<InstanceOp>(scope);
  if (instance)
    scope = components.lookup(instance.getComponent());
  for (auto property : scope->getRegion(0).front().getOps<PropertyOp>()) {
    if (property.getName() != name)
      continue;
    auto visibility = property.getVisibility();
    if (requireInOut && visibility != "in-out")
      return op->emitOpError("requires an in-out component property ") << name;
    bool external = instance || owner.starts_with("__global_");
    if (external && visibility == "private")
      return op->emitOpError("cannot access private property ") << name;
    if (write &&
        (external ? !writableFromParent(visibility) : visibility == "in"))
      return op->emitOpError("cannot write read-only property ") << name;
    if (property.getInitial().getType() != type)
      return op->emitOpError("property type mismatch for ")
             << owner << "." << name;
    return success();
  }
  return op->emitOpError("unknown property ") << owner << "." << name;
}

static LogicalResult verifyPropertyLinkCycles(const Owners &owners) {
  llvm::StringMap<llvm::StringMap<Operation *>> links;
  for (const auto &entry : owners) {
    auto index = [&](Block &block) {
      for (auto &op : block) {
        auto alias = dyn_cast<PropertyOp>(op);
        if (isa<InstanceLinkOp, ControlLinkOp>(op) ||
            (alias && alias.getAlias().value_or("") == "true"))
          links[entry.getKey()].try_emplace(
              op.getAttrOfType<StringAttr>("name").getValue(), &op);
      }
    };
    auto &body = entry.getValue()->getRegion(0).front();
    index(body);
    if (isa<ComponentOp>(entry.getValue()))
      for (auto root : body.getOps<ElementOp>())
        index(root.getBody().front());
  }
  llvm::DenseSet<Operation *> checked;
  for (const auto &entry : links)
    for (const auto &start : entry.getValue()) {
      llvm::DenseSet<Operation *> chain;
      for (Operation *link = start.getValue(); link && !checked.contains(link);) {
        if (!chain.insert(link).second)
          return link->emitOpError(isa<InstanceLinkOp>(link)
                                       ? "cyclic instance property link"
                                       : "cyclic property alias");
        auto *reference = link->getOperand(0).getDefiningOp();
        if (!isa_and_nonnull<ReadOp, ControlReadOp>(reference))
          break;
        auto target = reference->getAttrOfType<StringAttr>("owner");
        auto owner = links.find(target ? target.getValue() : "root");
        link = owner == links.end() ? nullptr : owner->getValue().lookup(
            reference->getAttrOfType<StringAttr>("value").getValue());
      }
      for (auto *link : chain)
        checked.insert(link);
    }
  return success();
}

static bool matchesDeclaredType(Type type, llvm::StringRef declared) {
  if (auto record = dyn_cast<RecordType>(type))
    return record.getName() == declared;
  if (auto list = dyn_cast<RecordListType>(type))
    return declared.consume_front("[") && declared.consume_back("]") &&
           list.getName() == declared;
  std::string printed;
  llvm::raw_string_ostream stream(printed);
  type.print(stream);
  auto spelling = llvm::StringRef(stream.str());
  if (declared == "[string]")
    declared = "stringlist";
  else if (declared == "[int]")
    declared = "intlist";
  else if (declared == "[Date]")
    declared = "datelist";
  else if (declared == "[hit3d]" || declared == "[Hit3D]")
    declared = "hit3dlist";
  else if (declared == "styled-text")
    declared = "styledtext";
  return spelling.consume_front("!xr.") && spelling == declared.lower();
}

static Type repeatedItemType(Type model) {
  auto *context = model.getContext();
  if (auto list = dyn_cast<RecordListType>(model))
    return RecordType::get(context, list.getName());
  if (isa<StringListType>(model))
    return StringType::get(context);
  if (isa<IntListType>(model))
    return IntType::get(context);
  if (isa<DateListType>(model))
    return DateType::get(context);
  if (isa<Hit3DListType>(model))
    return Hit3DType::get(context);
  if (isa<TouchPointsType>(model))
    return TouchPointType::get(context);
  return model;
}

LogicalResult ModelEditOp::verify() {
  if (auto callback = (*this)->getParentOfType<CallbackOp>();
      callback && callback.getPure().value_or("false") == "true")
    return emitOpError("model edits are not allowed in a pure callback");
  if (!getModel().getDefiningOp<ReadOp>())
    return emitOpError("model edits require a writable model property");
  auto args = getArgs();
  auto row = repeatedItemType(getModel().getType());
  auto method = getMethod();
  if ((method == "append" && args.size() == 1 && args[0].getType() == row) ||
      ((method == "insert" || method == "set") && args.size() == 2 &&
       isa<IntType>(args[0].getType()) && args[1].getType() == row) ||
      (method == "remove" && args.size() == 1 && isa<IntType>(args[0].getType())) ||
      (method == "move" && args.size() == 2 && isa<IntType>(args[0].getType()) &&
       isa<IntType>(args[1].getType())) ||
      (method == "clear" && args.empty()))
    return success();
  return emitOpError("invalid model edit method or argument types");
}

LogicalResult ModelAtOp::verify() {
  if (getResult().getType() != repeatedItemType(getModel().getType()))
    return emitOpError("model row type must match the fallback and result");
  return success();
}


LogicalResult EnumMatchOp::verify() {
  if (getBody().empty())
    return emitOpError("requires an enum match body");
  uint64_t ordinal = 0;
  llvm::StringSet<> names;
  for (Operation &child : getBody().front()) {
    if (isa<EndOp>(child))
      continue;
    auto arm = dyn_cast<EnumCaseOp>(child);
    if (!arm || arm.getOrdinal() != ordinal++ || arm.getName().empty() ||
        !llvm::all_of(arm.getName(), [](char c) { return llvm::isAlnum(c) || c == '_'; }) ||
        !names.insert(arm.getName()).second)
      return child.emitOpError("expected a unique enum match case in declaration order");
  }
  if (ordinal == 0)
    return emitOpError("requires at least one enum case");
  return success();
}

LogicalResult EnumCaseOp::verify() {
  auto parent = (*this)->getParentOfType<EnumMatchOp>();
  if (!parent || getBody().empty() || getBody().front().empty())
    return emitOpError("requires a body in an enum match");
  auto yield = dyn_cast_or_null<EnumResultOp>(getBody().front().back().getPrevNode());
  if (!yield || yield.getInput().getType() != parent.getResult().getType())
    return emitOpError("requires a final xr.enum_result of the match result type");
  for (Operation &child : getBody().front()) {
    if (&child != yield.getOperation() && !isa<EndOp>(child) && !isExpression(child))
      return child.emitOpError("expected an enum arm expression");
  }
  return success();
}

LogicalResult MatchEnumOp::verify() {
  if (getArms().empty())
    return emitOpError("requires at least one arm");
  for (Value arm : getArms()) {
    if (arm.getType() != getResult().getType())
      return emitOpError("each arm must match the result type");
  }
  return success();
}

LogicalResult LoopParamOp::verify() {
  auto name = getValue();
  for (auto *scope = (*this)->getParentOp(); scope; scope = scope->getParentOp()) {
    if (auto loop = dyn_cast<ForOp>(scope)) {
      Type expected;
      if (name == loop.getItem())
        expected = repeatedItemType(loop.getModel().getType());
      else if (!name.empty() && name == loop.getIndex())
        expected = IntType::get(getContext());
      if (expected) {
        if (getResult().getType() != expected)
          return emitOpError("loop binding type mismatch for ") << name;
        return success();
      }
    }
  }
  return emitOpError("unknown loop binding ") << name;
}

static LogicalResult verifyParameter(ParamOp param) {
  auto name = param.getValue();
  auto type = param.getResult().getType();
  auto checkType = [&](Type expected) -> LogicalResult {
    if (type != expected)
      return param.emitOpError("parameter type mismatch for ") << name;
    return success();
  };
  auto *scope = param->getParentOp();
  while (isa<IfOp, ForOp, EnumMatchOp, EnumCaseOp>(scope))
    scope = scope->getParentOp();
  bool allowsRow = true;
  if (isa<CallbackOp, HandlerOp>(scope)) {
    auto parsed = llvm::json::parse(
        scope->getAttrOfType<StringAttr>("parameters").getValue());
    if (!parsed) {
      llvm::consumeError(parsed.takeError());
      return param.emitOpError("invalid parameter declaration");
    }
    auto *parameters = parsed->getAsArray();
    if (!parameters)
      return param.emitOpError("invalid parameter declaration");
    for (auto &parameter : *parameters) {
      auto *object = parameter.getAsObject();
      if (!object || object->getString("name") != name)
        continue;
      auto declared = object->getString("ty");
      if (!declared || !matchesDeclaredType(type, *declared))
        return param.emitOpError("parameter type mismatch for ") << name;
      return success();
    }
    auto *owner = scope->getParentOp();
    allowsRow = isa<InstanceOp>(owner);
    if (auto element = dyn_cast<ElementOp>(owner)) {
      auto handler = dyn_cast<HandlerOp>(scope);
      allowsRow = handler &&
          (isVirtualItemDelegate(element) ||
           (element.getKind() == "StyledText" &&
            handler.getName() == "link-activated") ||
           (isPushButton(element.getKind()) &&
            (handler.getName() == "clicked" || handler.getName() == "toggled")));
    }
  } else if (auto parser = dyn_cast<ClipboardPasteOp>(scope)) {
    if (name == parser.getTextParameter())
      return checkType(StringType::get(param.getContext()));
  }

  if (allowsRow) {
    for (auto *owner = scope; owner && !isa<ComponentOp>(owner);
         owner = owner->getParentOp()) {
      if (!isa<ElementOp, InstanceOp>(owner))
        continue;
      for (auto repeat : owner->getRegion(0).front().getOps<RepeatOp>()) {
        // The model expression is evaluated in the enclosing row, before this
        // repeat introduces its item and index.
        if (owner == scope && param->isBeforeInBlock(repeat.getOperation()))
          continue;
        if (name == repeat.getItem())
          return checkType(repeatedItemType(repeat.getInput().getType()));
        if (!name.empty() && name == repeat.getIndex())
          return checkType(IntType::get(param.getContext()));
      }
    }
  }

  auto visualTemplate = enclosingVisualTemplate(param);
  auto templateKind = visualTemplate ? visualTemplate.getKind() : llvm::StringRef{};
  auto *context = param.getContext();
  Type expected;
  if (templateKind == "SplitHandle") {
    if (name == "handle-index")
      expected = IntType::get(context);
    else if (name == "handle-orientation")
      expected = OrientationType::get(context);
    else if (name == "handle-hovered" || name == "handle-pressed")
      expected = BoolType::get(context);
  } else if (templateKind == "SectionHeader") {
    if (name == "section-label")
      expected = StringType::get(context);
    else if (name == "section-first-index" || name == "section-count")
      expected = IntType::get(context);
    else if (name == "section-pinned")
      expected = BoolType::get(context);
  } else if (templateKind == "ComboOption" || templateKind == "TumblerOption" || templateKind == "PathViewOption") {
    const bool tumbler = templateKind != "ComboOption";
    if (name == "option-data") {
      expected = tumbler ? Type(IntType::get(context)) : Type(StringType::get(context));
      if (auto combo = dyn_cast<ElementOp>(visualTemplate->getParentOp())) {
        for (auto binding : combo.getBody().front().getOps<BindOp>()) {
          if (binding.getName() == "model") {
            expected = repeatedItemType(binding.getInput().getType());
            break;
          }
        }
      }
    } else if (name == "option-text")
      expected = StringType::get(context);
    else if (name == "option-index")
      expected = IntType::get(context);
    else if (name == "option-enabled" || name == "option-selected" ||
             (!tumbler && name == "option-highlighted"))
      expected = BoolType::get(context);
    else if ((tumbler && name == "option-displacement") ||
             (templateKind == "PathViewOption" && name == "option-progress"))
      expected = FloatType::get(context);
  } else if (templateKind == "DayCell") {
    if (name == "cell-date")
      expected = DateType::get(context);
    else if (name == "cell-index" || name == "cell-weekday" || name == "cell-week" ||
             name == "cell-week-year")
      expected = IntType::get(context);
    else if (name == "cell-selected" || name == "cell-today" || name == "cell-in-month" ||
             name == "cell-enabled" || name == "cell-in-range" ||
             name == "cell-range-start" || name == "cell-range-end")
      expected = BoolType::get(context);
  }
  if (!templateKind.empty()) {
    if (type != expected)
      return param.emitOpError("invalid ") << templateKind << " parameter or type";
    return success();
  }
  return param.emitOpError("unknown parameter ") << name;
}

static LogicalResult verifyCall(Operation *op, const Owners &owners,
                                const Components &components,
                                llvm::StringRef owner, llvm::StringRef name,
                                Type result) {
  Operation *target = owners.lookup(owner);
  if (auto instance = dyn_cast_or_null<InstanceOp>(target))
    target = components.lookup(instance.getComponent());
  if (!isa_and_nonnull<ComponentOp, ElementOp>(target))
    return op->emitOpError("unknown callback owner ") << owner;

  for (auto callback : target->getRegion(0).front().getOps<CallbackOp>()) {
    if (callback.getName() != name)
      continue;
    auto enclosing = op->getParentOfType<CallbackOp>();
    if (!callback.getPure() && !op->getParentOfType<HandlerOp>() &&
        (!enclosing || enclosing.getPure()))
      return op->emitOpError("impure callback in a pure context: ") << name;

    auto parsed = llvm::json::parse(callback.getParameters());
    if (!parsed) {
      llvm::consumeError(parsed.takeError());
      return op->emitOpError("invalid callback signature for ") << name;
    }
    auto *parameters = parsed->getAsArray();
    if (!parameters || parameters->size() != op->getNumOperands())
      return op->emitOpError("incorrect argument count for callback ") << name;
    for (unsigned index = 0; index < parameters->size(); ++index) {
      auto *param = (*parameters)[index].getAsObject();
      auto declared = param ? param->getString("ty") : std::nullopt;
      if (!declared ||
          !matchesDeclaredType(op->getOperand(index).getType(), *declared))
        return op->emitOpError("incorrect argument type for callback ")
               << name << " at argument " << (index + 1);
    }
    if (result) {
      auto returned = dyn_cast_or_null<ReturnOp>(
          callback.getBody().front().getTerminator()->getPrevNode());
      if (!returned || returned.getInput().getType() != result)
        return op->emitOpError("incorrect result type for callback ") << name;
    }
    return success();
  }
  return op->emitOpError("unknown callback ") << owner << "." << name;
}

static CallbackOp lookupCallback(const Owners &owners, const Components &components,
                                 llvm::StringRef owner, llvm::StringRef name) {
  auto *scope = owners.lookup(owner);
  if (auto instance = dyn_cast_or_null<InstanceOp>(scope))
    scope = components.lookup(instance.getComponent());
  if (scope)
    for (auto callback : scope->getRegion(0).front().getOps<CallbackOp>())
      if (callback.getName() == name)
        return callback;
  return {};
}

static Type callbackResult(CallbackOp callback) {
  auto returned = dyn_cast_or_null<ReturnOp>(callback.getBody().front().getTerminator()->getPrevNode());
  return returned ? returned.getInput().getType() : Type{};
}

static LogicalResult verifyCallbackSignature(CallbackOp callback, CallbackOp target) {
  if (!target)
    return callback.emitOpError("unknown callback target");
  if (callback.getPure() != target.getPure())
    return callback.emitOpError("callback purity must match its target");
  auto parse = [](CallbackOp op) -> std::optional<llvm::json::Value> {
    auto json = llvm::json::parse(op.getParameters());
    if (!json) {
      llvm::consumeError(json.takeError());
      return std::nullopt;
    }
    return std::move(*json);
  };
  auto left = parse(callback);
  auto right = parse(target);
  auto *a = left ? left->getAsArray() : nullptr;
  auto *b = right ? right->getAsArray() : nullptr;
  if (!a || !b || a->size() != b->size() || callbackResult(callback) != callbackResult(target))
    return callback.emitOpError("callback signature must match its target");
  for (auto [value, expected] : llvm::zip(*a, *b))
    if (!value.getAsObject() || !expected.getAsObject() ||
        value.getAsObject()->getString("ty") != expected.getAsObject()->getString("ty"))
      return callback.emitOpError("callback parameter type must match its target");
  return success();
}

static LogicalResult verifyCallbackAliasCycles(ComponentOp component, const Owners &owners,
                                               const Components &components) {
  llvm::DenseSet<Operation *> checked;
  for (const auto &entry : owners)
    for (auto start : entry.getValue()->getRegion(0).front().getOps<CallbackOp>()) {
      llvm::DenseSet<Operation *> chain;
      for (auto alias = start; alias && alias.getAliasOwner() &&
           alias->getParentOfType<ComponentOp>() == component &&
           !checked.contains(alias.getOperation());) {
        if (!chain.insert(alias.getOperation()).second)
          return alias.emitOpError("cyclic callback alias");
        alias = lookupCallback(owners, components, *alias.getAliasOwner(),
                               alias.getAliasName().value_or(""));
      }
      for (auto *callback : chain)
        checked.insert(callback);
    }
  return success();
}

static bool focusTargetKind(llvm::StringRef kind) {
  return llvm::StringSwitch<bool>(kind)
      .Cases({"FocusScope", "Loader", "TextInput", "LineEdit", "TextEdit", "TextArea", "SearchField", "StyledText",
              "Button", "ToolButton", "RoundButton", "DelayButton", "DialogButton", "StandardButton",
              "Slider", "Dial", "SpinBox", "DoubleSpinBox", "ComboBox", "Calendar", "MonthGrid", "ScrollBar",
              "CheckBox", "Switch", "RadioButton", "RadioGroup", "Tab", "TabButton", "SwipeView",
              "SwipeDelegate", "StackView", "ListView", "GridView", "StandardListView",
              "TreeView", "TableView", "StandardTableView", "GroupBox", "Expander", "PageIndicator",
              "Tumbler", "PathView"}, true)
      .Default(false);
}

using ComboValueTypes = llvm::DenseMap<Operation *, std::string>;

static LogicalResult verifyReferences(ComponentOp component,
                                      const Components &components,
                                      const ComboValueTypes &comboValues) {
  Owners owners;
  owners.try_emplace("root", component.getOperation());
  auto indexed = component.walk([&](Operation *op) -> WalkResult {
    if (auto instance = dyn_cast<InstanceOp>(op)) {
      if (auto target = components.lookup(instance.getComponent()); !target || target.getGlobal()) {
        op->emitOpError("unknown component ") << instance.getComponent();
        return WalkResult::interrupt();
      }
    }
    llvm::StringRef key = scopedOwnerKey(op);
    if (!key.empty() && !owners.try_emplace(key, op).second) {
      op->emitOpError("duplicate property owner ") << key;
      return WalkResult::interrupt();
    }
    return WalkResult::advance();
  });
  if (indexed.wasInterrupted())
    return failure();

  llvm::SmallVector<std::string> globalKeys;
  for (const auto &entry : components) {
    auto target = entry.getValue();
    if (target.getGlobal() && target != component)
      globalKeys.push_back(("__global_" + entry.getKey()).str());
  }
  for (const auto &key : globalKeys)
    owners.try_emplace(key, components.lookup(llvm::StringRef(key).drop_front(9)));

  for (auto &entry : owners) {
    llvm::StringMap<PropertyOp> properties;
    for (auto property :
         entry.getValue()->getRegion(0).front().getOps<PropertyOp>()) {
      if (!properties.try_emplace(property.getName(), property).second)
        return property.emitOpError("duplicate property ")
               << property.getName();
      auto visibility = property.getVisibility();
      if (visibility != "out" && visibility != "private" &&
          !writableFromParent(visibility))
        return property.emitOpError("invalid property visibility ")
               << visibility;
    }
    llvm::StringSet<> callbacks;
    for (auto callback : entry.getValue()->getRegion(0).front().getOps<CallbackOp>()) {
      if (properties.count(callback.getName()) || !callbacks.insert(callback.getName()).second)
        return callback.emitOpError("duplicate or conflicting callback ") << callback.getName();
    }
  }

  auto elementOwner = [&](llvm::StringRef name) -> ElementOp {
    auto *target = owners.lookup(name);
    auto view = dyn_cast_or_null<ElementOp>(target);
    if (target == component.getOperation()) {
      auto roots = component.getBody().front().getOps<ElementOp>();
      if (!roots.empty())
        view = *roots.begin();
    }
    return view;
  };
  auto stackOwner = [&](llvm::StringRef name) -> ElementOp {
    auto view = elementOwner(name);
    return view && view.getKind() == "StackView" ? view : ElementOp{};
  };
  auto verifyViewport = [&](Operation *op, llvm::StringRef name,
                            bool indexed) -> LogicalResult {
    auto target = elementOwner(name);
    if (!target || !(indexed ? isVirtualView(target.getKind()) : isViewport(target.getKind())))
      return op->emitOpError("requires a scrollable element supporting the operation");
    if (!isStaticViewport(target))
      return op->emitOpError("requires a statically addressed viewport");
    return success();
  };
  llvm::StringMap<llvm::SmallVector<llvm::StringRef>> checkGroupLinks;
  llvm::StringMap<llvm::StringSet<>> actionGroupValues;
  auto references = component.walk([&](Operation *op) -> WalkResult {
    if (failed(verifyDirectGeometryFlow(op, owners)))
      return WalkResult::interrupt();
    LogicalResult result = success();
    if (auto property = dyn_cast<PropertyOp>(op);
        property && property.getAlias().value_or("") == "true") {
      if (!staticPropertyOwner(op->getParentOp()))
        result = op->emitOpError("property aliases require statically addressed endpoints");
      else if (auto read = property.getInitial().getDefiningOp<ReadOp>()) {
        auto owner = read.getOwner().value_or("root");
        result = verifyReference(op, owners, components, owner, read.getValue(),
                                 read.getResult().getType(), true);
        if (succeeded(result) && !staticPropertyOwner(owners.lookup(owner)))
          result = op->emitOpError("property aliases require statically addressed endpoints");
      }
    } else if (auto binding = dyn_cast<BindOp>(op); binding && binding.getName() == "reorderable") {
      auto group = dyn_cast<ElementOp>(op->getParentOp());
      if (!group || (group.getKind() != "TabBar" && group.getKind() != "TabWidget"))
        result = op->emitOpError("reorderable requires a TabBar or TabWidget");
      else if (auto read = writableNavigationModelRead(group))
        result = verifyReference(op, owners, components, read.getOwner().value_or("root"),
                                 read.getValue(), read.getResult().getType(), true);
    } else if (auto param = dyn_cast<ParamOp>(op)) {
      result = verifyParameter(param);
    } else if (auto control = dyn_cast<ControlReadOp>(op)) {
      if (!validControlAlias(elementOwner(control.getOwner()), control.getValue(), control.getResult().getType()))
        result = op->emitOpError("requires a supported writable control property of matching type");
    } else if (auto read = dyn_cast<ReadOp>(op)) {
      auto owner = read.getOwner().value_or("root");
      if (op->getParentOfType<ClipboardPasteOp>() && !staticPropertyOwner(owners.lookup(owner)))
        result = op->emitOpError("clipboard-paste cannot read a row component instance");
      else
        result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), false);
    } else if (isa<DynamicActionOp, DynamicReadOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      if (!target || target.getKind() != "DynamicContainer" || !staticPropertyOwner(target))
        result = op->emitOpError("requires a statically addressed DynamicContainer owner");
      if (isa<DynamicActionOp>(op)) {
        auto callback = op->getParentOfType<CallbackOp>();
        if (!op->getParentOfType<HandlerOp>() && (!callback || callback.getPure()))
          result = op->emitOpError("dynamic component mutations require an effectful callback");
      }
    } else if (auto read = dyn_cast<LoaderExternalStatusOp>(op)) {
      auto loader = elementOwner(read.getOwner());
      if (!loader || loader.getKind() != "Loader" || !staticPropertyOwner(loader))
        result = op->emitOpError("requires a statically addressed Loader owner");
    } else if (auto read = dyn_cast<LoaderMountedOp>(op)) {
      auto loader = elementOwner(read.getOwner());
      if (!loader || loader.getKind() != "Loader" || !staticPropertyOwner(loader))
        result = op->emitOpError("requires a statically addressed Loader owner");
    } else if (auto read = dyn_cast<StackReadOp>(op)) {
      auto view = stackOwner(read.getOwner());
      if (!view)
        result = op->emitOpError("requires a StackView owner");
      else if (auto route = read.getRoute())
        result = verifyStackRoute(op, view, route);
    } else if (isa<TextAcceptsOp, TextValidityOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      auto property = isa<TextValidityOp>(op) ? "input-validity" : "acceptable-input";
      if (!target || !staticPropertyOwner(target) ||
          op->getAttrOfType<StringAttr>("value").getValue() != property ||
          !llvm::StringSwitch<bool>(target.getKind())
               .Cases({"TextInput", "LineEdit", "TextEdit", "TextArea", "ComboBox"}, true)
               .Default(false))
        result = op->emitOpError() << property << " requires a statically addressed text editor";
    } else if (auto read = dyn_cast<MediaReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto name = read.getValue();
      if (!target ||
          !((target.getKind() == "MediaPlayer" && name != "loops-remaining") ||
            (target.getKind() == "SoundEffect" &&
             (name == "ready" || name == "playing" || name == "status" ||
              name == "error" || name == "loops-remaining"))))
        result = op->emitOpError("requires a media owner exposing this state property");
    } else if (auto read = dyn_cast<ImagePlaybackReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || target.getKind() != "AnimatedImage")
        result = op->emitOpError("requires an AnimatedImage owner");
    } else if (auto read = dyn_cast<GeometryReadOp>(op)) {
      auto *owner = owners.lookup(read.getOwner());
      auto element = elementOwner(read.getOwner());
      if (auto instance = dyn_cast_or_null<InstanceOp>(owner)) {
        auto roots = components.lookup(instance.getComponent()).getBody().front().getOps<ElementOp>();
        if (llvm::hasSingleElement(roots))
          element = *roots.begin();
      }
      if (!element || !staticPropertyOwner(owner) ||
          element.getKind() == "Action" || element.getKind() == "ActionGroup" || element.getKind() == "ButtonGroup" || element.getKind() == "Timer" || element.getKind() == "SoundEffect" ||
          element.getKind() == "SecondaryWindow" || element.getKind() == "SystemTrayIcon" || element.getKind() == "TableColumn" ||
          element.getKind() == "ComboPopup" || isNativeDialog(element.getKind()))
        result = op->emitOpError("layout geometry requires a statically addressed visual target");
      else if ((read.getValue() == "available-width" || read.getValue() == "available-height") &&
               !isPane(element.getKind()) && element.getKind() != "ApplicationWindow")
        result = op->emitOpError("available size requires a Pane or ControlFrame or ApplicationWindow");
    } else if (auto read = dyn_cast<ViewportReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (isScrollBarExtent(read.getValue()) && (!target || target.getKind() != "ScrollView"))
        result = op->emitOpError("requires ScrollView for effective scrollbar dimensions");
      else
        result = verifyViewport(op, read.getOwner(), false);
    } else if (auto read = dyn_cast<FlickMotionReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto metric = read.getValue();
      auto type = read.getResult().getType();
      const bool valid = ((metric == "dragging" || metric == "flicking" || metric == "moving" || metric == "rebounding") && isa<BoolType>(type)) ||
                         ((metric == "horizontal-overshoot" || metric == "vertical-overshoot") && isa<LengthType>(type));
      const bool tumbler = target && (target.getKind() == "Tumbler" || target.getKind() == "PathView");
      if (tumbler) {
        if (!staticPropertyOwner(target) ||
            !(metric == "dragging" || metric == "flicking" || metric == "moving") ||
            !isa<BoolType>(type))
          result = op->emitOpError("requires a motion property on a statically addressed ") << target.getKind();
      } else if (!target || target.getKind() != "Flickable" || !isStaticViewport(target) || !valid) {
        result = op->emitOpError("requires a motion property on Flickable");
      }
    } else if (auto command = dyn_cast<CancelFlickOp>(op)) {
      auto target = elementOwner(command.getOwner());
      if (!target || target.getKind() != "Flickable" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed Flickable");
    } else if (auto command = dyn_cast<ReturnToBoundsOp>(op)) {
      auto target = elementOwner(command.getOwner());
      if (!target || target.getKind() != "Flickable" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed Flickable");
    } else if (auto read = dyn_cast<ScrollControlReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto metric = read.getValue();
      auto type = read.getResult().getType();
      if (!target || !isScrollControl(target.getKind()) || !isStaticViewport(target) ||
          !((isa<FloatType>(type) &&
             (metric == "position" || metric == "size" || isScrollThumbMetric(metric))) ||
            (isa<BoolType>(type) && isScrollStateMetric(target.getKind(), metric))))
        result = op->emitOpError("requires a supported typed metric on a scroll control");
    } else if (auto read = dyn_cast<ScaleRotateReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || target.getKind() != "ScaleRotateGestureHandler" || !isStaticViewport(target) ||
          scaleRotateType(op->getContext(), read.getValue()) != read.getResult().getType())
        result = op->emitOpError("requires a typed metric on a statically addressed gesture handler");
    } else if (auto read = dyn_cast<SwipeReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || target.getKind() != "SwipeGestureHandler" || !isStaticViewport(target) ||
          swipeType(op->getContext(), read.getValue()) != read.getResult().getType())
        result = op->emitOpError("requires a typed metric on a statically addressed swipe handler");
    } else if (auto read = dyn_cast<HoverReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || target.getKind() != "HoverHandler" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed HoverHandler");
    } else if (auto read = dyn_cast<TouchAreaReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || target.getKind() != "TouchArea" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed TouchArea");
    } else if (auto command = dyn_cast<SwipeCancelOp>(op)) {
      auto target = elementOwner(command.getOwner());
      if (!target || target.getKind() != "SwipeGestureHandler" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed swipe handler");
    } else if (auto selected = dyn_cast<ListSelectionReadOp>(op)) {
      auto target = elementOwner(selected.getOwner());
      if (!target || !isSelectedIndexProperty(target.getKind(), selected.getValue()) || !isStaticViewport(target))
        result = op->emitOpError("requires selected-view-index on a statically addressed list or grid");
    } else if (auto selected = dyn_cast<ListRowSelectedOp>(op)) {
      auto target = elementOwner(selected.getOwner());
      if (!target || !isSelectionList(target.getKind()) || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed list or grid");
    } else if (auto selected = dyn_cast<ListSelectionKeysOp>(op)) {
      auto target = elementOwner(selected.getOwner());
      if (!isKeyedRowView(target) || !isStaticViewport(target))
        result = op->emitOpError("selected-keys requires a statically addressed view with row-key-field");
    } else if (auto count = dyn_cast<ViewCountOp>(op)) {
      auto target = elementOwner(count.getOwner());
      if (!target || !isViewCountProperty(target.getKind(), count.getValue()))
        result = op->emitOpError("requires a row count property on a virtual view");
      else
        result = verifyViewport(op, count.getOwner(), true);
    } else if (auto lookup = dyn_cast<ViewIndexOp>(op)) {
      if (lookup.getValue() == "view-index-for-key" &&
          !isKeyedRowView(elementOwner(lookup.getOwner())))
        result = op->emitOpError("view-index-for-key requires row-key-field");
      else
        result = verifyViewport(op, lookup.getOwner(), true);
    } else if (auto selection = dyn_cast<SourceSelectionOp>(op)) {
      if (selection.getMethod() == "select-keys" || selection.getMethod() == "select-key") {
        auto target = elementOwner(selection.getOwner());
        if (!target || (target.getKind() != "TreeView" && !isKeyedRowView(target)) ||
            !isStaticViewport(target))
          result = op->emitOpError() << selection.getMethod()
              << " requires a statically addressed TreeView or a view with row-key-field";
      } else if (selection.getMethod() == "select-source-cell") {
        auto target = elementOwner(selection.getOwner());
        if (!target || !isTableView(target.getKind()) || !isStaticViewport(target))
          result = op->emitOpError("requires a statically addressed table");
      } else {
        result = verifyViewport(op, selection.getOwner(), true);
      }
    } else if (auto command = dyn_cast<PositionChildOp>(op)) {
      auto view = elementOwner(command.getOwner());
      auto *target = owners.lookup(command.getTarget());
      if (!view || !isContentViewport(view.getKind()) || !isStaticViewport(view) ||
          !target || !staticPropertyOwner(target) || target == view.getOperation()) {
        result = op->emitOpError("requires a static content viewport and a named descendant");
      } else {
        auto *parent = target;
        for (; parent != view.getOperation(); parent = parent->getParentOp()) {
          if (!isa_and_nonnull<ElementOp, InstanceOp>(parent))
            break;
          if (auto element = dyn_cast<ElementOp>(parent);
              element && (isWindowOverlay(element.getKind()) || element.getKind() == "Menu" ||
                          element.getKind() == "Tooltip" || element.getKind() == "Shortcut"))
            break;
        }
        if (parent != view.getOperation())
          result = op->emitOpError("requires a descendant in the viewport content");
      }
    } else if (auto command = dyn_cast<ScrollCommandOp>(op)) {
      auto method = command.getMethod();
      if (method == "increase" || method == "decrease") {
        auto target = elementOwner(command.getOwner());
        if (!target || target.getKind() != "ScrollBar" || !isStaticViewport(target))
          result = op->emitOpError("requires a statically addressed ScrollBar");
      } else if (method == "position-view-at-key") {
        auto target = elementOwner(command.getOwner());
        if (!isKeyedRowView(target) || !isStaticViewport(target))
          result = op->emitOpError("position-view-at-key requires a statically addressed view with row-key-field");
      } else if (method == "position-view-at-row" || method == "position-view-at-column" ||
          method == "position-view-at-cell" || method == "position-view-at-source-cell") {
        auto target = elementOwner(command.getOwner());
        if (!target || !isTableView(target.getKind()) || !isStaticViewport(target))
          result = op->emitOpError("requires a statically addressed table");
      } else {
        result = verifyViewport(op, command.getOwner(),
                                method == "position-view-at-index" || method == "position-view-at-source-index");
      }
    } else if (isa<TableSortCommandOp, TableSortReadOp, TableSelectionCommandOp, TableSelectionReadOp,
                   TableHistoryCommandOp, TableHistoryReadOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      bool valid = target && isTableView(target.getKind());
      if (auto read = dyn_cast<TableSelectionReadOp>(op); target && read)
        valid |= isRowSelectionView(target.getKind()) &&
                 (read.getValue() == "selection-count" || read.getValue() == "has-selection");
      if (auto command = dyn_cast<TableSelectionCommandOp>(op); target && command) {
        auto name = command.getMethod();
        valid = (valid && name != "toggle-row") ||
                (isRowSelectionView(target.getKind()) &&
                 (name == "select-all" || name == "clear-selection" ||
                  name == "select-row" || name == "toggle-row"));
      }
      if (!valid || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed table or a list, grid, or tree supporting the requested selection operation");
    } else if (isa<SplitRestoreOp, SplitStateReadOp, SplitResizingOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      if (!target || target.getKind() != "SplitView" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed SplitView");
      else if (auto read = dyn_cast<SplitResizingOp>(op); read && read.getValue() != "resizing")
        result = op->emitOpError("requires the resizing property");
    } else if (isa<TreeCommandOp, TreeQueryOp, TreeSelectionKeysOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      if (!target || target.getKind() != "TreeView" || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed TreeView");
    } else if (auto edit = dyn_cast<ModelEditOp>(op)) {
      if (auto read = edit.getModel().getDefiningOp<ReadOp>())
        result = verifyReference(op, owners, components, read.getOwner().value_or("root"),
                                 read.getValue(), read.getResult().getType(), true);
    } else if (auto move = dyn_cast<MoveNavigationItemOp>(op)) {
      auto target = elementOwner(move.getOwner());
      auto read = target && isStaticViewport(target) ? writableNavigationModelRead(target) : ReadOp{};
      if (!read)
        result = op->emitOpError("move-item requires a statically addressed navigation control with a writable list model");
      else
        result = verifyReference(op, owners, components, read.getOwner().value_or("root"),
                                 read.getValue(), read.getResult().getType(), true);
    } else if (isa<MoveColumnOp, MoveRowOp, SetRowHeightOp>(op)) {
      auto target = elementOwner(op->getAttrOfType<StringAttr>("owner").getValue());
      if (!target || !isTableView(target.getKind()) || !isStaticViewport(target))
        result = op->emitOpError("requires a statically addressed table");
    } else if (auto command = dyn_cast<TextCommandAvailableOp>(op)) {
      auto target = elementOwner(command.getOwner());
      if (!target || !staticPropertyOwner(target) ||
          !isTextCommandProperty(target.getKind(), command.getValue()))
        result = op->emitOpError("requires a text command property on a statically addressed editor");
    } else if (auto selection = dyn_cast<TextSelectionReadOp>(op)) {
      auto target = elementOwner(selection.getOwner());
      auto name = selection.getValue();
      auto type = selection.getResult().getType();
      if (!target || !staticPropertyOwner(target) ||
          !isTextSelectionProperty(target.getKind(), name) ||
          (name == "selected-text" ? !isa<StringType>(type) : !isa<IntType>(type)))
        result = op->emitOpError("requires a text selection property on a statically addressed editor with matching type");
    } else if (auto pressed = dyn_cast<ButtonPressedOp>(op)) {
      auto target = elementOwner(pressed.getOwner());
      if (!target || !isPushButton(target.getKind()) || !staticPropertyOwner(target) ||
          pressed.getValue() != "pressed")
        result = op->emitOpError("requires pressed on a statically addressed push button");
    } else if (auto checked = dyn_cast<ButtonCheckedOp>(op)) {
      auto target = elementOwner(checked.getOwner());
      if (!target || checked.getValue() != "checked" ||
          !(isPushButton(target.getKind()) ||
            (target.getKind() == "RadioButton" &&
             !target.getBody().front().getOps<RadioGroupRefOp>().empty())))
        result = op->emitOpError("requires checked on a button or a ButtonGroup radio option");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated button's row scope");
    } else if (auto read = dyn_cast<RadioCheckedOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto choice = elementOwner(read.getValue());
      if (!target || !choice || target.getKind() != "RadioButton" ||
          target->getParentOp() != choice.getOperation() ||
          !(choice.getKind() == "RadioGroup" ||
            ((choice.getKind() == "ListView" || choice.getKind() == "GridView") &&
             isVirtualItemDelegate(target))))
        result = op->emitOpError("requires a radio option and its owning selection control");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated radio option's row scope");
    } else if (auto read = dyn_cast<NavigationCountOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || !isNavigationGroup(target.getKind()) || !staticPropertyOwner(target) || read.getValue() != "count")
        result = op->emitOpError("requires count on a statically addressed navigation group");
    } else if (auto read = dyn_cast<NavigationIndexOp>(op)) {
      auto target = elementOwner(read.getOwner());
      bool activeIndex = target && target.getKind() == "StackLayout" && read.getValue() == "active-index";
      bool valueIndex = read.getValue() == "current-index" && hasNavigationValue(target);
      if (!target || (!activeIndex && !valueIndex) || !staticPropertyOwner(target))
        result = op->emitOpError("requires current-index on a navigation group using current-value or active-index on StackLayout");
    } else if (auto read = dyn_cast<NavigationActiveOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto choice = elementOwner(read.getValue());
      if (!target || !choice || target->getParentOp() != choice.getOperation() ||
          !((target.getKind() == "TabButton" && choice.getKind() == "TabBar") ||
            (target.getKind() == "Tab" && choice.getKind() == "TabWidget") ||
            (target.getKind() == "SwipePage" && choice.getKind() == "SwipeView") ||
            (target.getKind() == "StackItem" && choice.getKind() == "StackLayout")))
        result = op->emitOpError("requires a navigation item and its owning selection control");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated navigation item's row scope");
    } else if (auto read = dyn_cast<ToggleStateReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto name = read.getValue();
      auto type = read.getResult().getType();
      if (!target ||
          !((name == "checked" && isa<BoolType>(type) &&
             (target.getKind() == "CheckBox" || target.getKind() == "Switch")) ||
            (name == "check-state" && isa<CheckStateType>(type) &&
             target.getKind() == "CheckBox")))
        result = op->emitOpError("requires a supported checkbox or switch state property");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated control's row scope");
    } else if (auto read = dyn_cast<NumericValueReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      if (!target || !validNumericControlValue(target.getKind(), read.getValue(),
                                               read.getResult().getType()))
        result = op->emitOpError("requires a supported numeric control value of matching type");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated control's row scope");
    } else if (auto read = dyn_cast<ComboPopupReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto name = read.getValue();
      auto type = read.getResult().getType();
      if (!target || target.getKind() != "ComboBox" || !staticPropertyOwner(target) ||
          !((name == "popup-open" && isa<BoolType>(type)) ||
            (name == "highlighted-index" && isa<IntType>(type))))
        result = op->emitOpError("requires a ComboBox popup state property of matching type");
    } else if (auto read = dyn_cast<ComboStateReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto name = read.getValue();
      auto type = read.getResult().getType();
      auto valueType = target ? comboValues.find(target.getOperation()) : comboValues.end();
      if (!target || !staticPropertyOwner(target) || valueType == comboValues.end() ||
          !(((name == "count" || name == "current-index") && isa<IntType>(type)) ||
            ((name == "current-text" || (name == "edit-text" && target.getKind() == "ComboBox")) &&
             isa<StringType>(type)) ||
            (name == "current-value" && comboValueTypeName(type) == valueType->second)))
        result = op->emitOpError(target && target.getKind() == "PathView"
                                     ? "requires a record PathView state property of matching type"
                                     : target && target.getKind() == "Tumbler"
                                           ? "requires a record Tumbler state property of matching type"
                                           : "requires a ComboBox state property of matching type");
    } else if (auto read = dyn_cast<RangePositionReadOp>(op)) {
      auto target = elementOwner(read.getOwner());
      auto kind = target ? target.getKind() : llvm::StringRef();
      auto name = read.getValue();
      if (!(((kind == "Slider" || kind == "Dial" || kind == "ProgressIndicator") && name == "position") ||
            ((kind == "Slider" || kind == "ProgressIndicator") && name == "visual-position") ||
            (kind == "RangeSlider" &&
             (name == "lower-position" || name == "upper-position" ||
              name == "lower-visual-position" || name == "upper-visual-position"))))
        result = op->emitOpError("requires a supported range position");
      else if (!staticPropertyOwner(target))
        result = op->emitOpError("range positions on repeated controls require a component delegate");
    } else if (auto focus = dyn_cast<FocusReadOp>(op)) {
      auto target = elementOwner(focus.getOwner());
      const bool within = focus.getValue() == "focus-within";
      if (!target || !focusTargetKind(target.getKind()) ||
          (within && target.getKind() != "FocusScope" && target.getKind() != "RadioGroup") ||
          (!within && target.getKind() == "RadioGroup")) {
        result = op->emitOpError("requires a focus state property on its owning element");
      } else {
        for (auto current = target; current; current = current->getParentOfType<ElementOp>())
          if (!current.getBody().front().getOps<RepeatOp>().empty()) {
            result = op->emitOpError("focus bindings on repeated elements require a component delegate");
            break;
          }
      }
    } else if (auto focus = dyn_cast<FocusTargetOp>(op)) {
      auto target = elementOwner(focus.getTarget());
      auto parent = cast<ElementOp>(op->getParentOp());
      if (!target || !focusTargetKind(target.getKind()) || target == parent) {
        result = op->emitOpError("requires a focusable descendant");
      } else {
        while (target && target != parent &&
               target.getBody().front().getOps<RepeatOp>().empty())
          target = target->getParentOfType<ElementOp>();
        if (target != parent)
          result = op->emitOpError("requires a descendant in the same repetition");
      }
    } else if (auto command = dyn_cast<TextCommandOp>(op)) {
      auto target = elementOwner(command.getOwner());
      auto method = command.getMethod();
      bool tableEdit = method == "edit-cell" || method == "cancel-edit";
      if (tableEdit) {
        if (!target || !isTableView(target.getKind()) || !isStaticViewport(target))
          result = op->emitOpError("requires a statically addressed table view");
      } else if (!target || !isStaticViewport(target) ||
                 !(isTextEditor(target.getKind()) ||
                   (target.getKind() == "StyledText" &&
                    llvm::StringSwitch<bool>(method)
                        .Cases({"select-all", "select-word", "clear-selection", "set-selection-offsets", "copy"}, true)
                        .Default(false)))) {
        result = op->emitOpError("requires a statically addressed editor supporting the requested command");
      }
    } else if (auto click = dyn_cast<ButtonClickOp>(op)) {
      auto target = elementOwner(click.getOwner());
      if (!target || !isPushButton(target.getKind()) || !staticPropertyOwner(target))
        result = op->emitOpError("requires a statically addressed push button");
    } else if (auto toggle = dyn_cast<ButtonToggleOp>(op)) {
      auto target = elementOwner(toggle.getOwner());
      if (!target || !isPushButton(target.getKind()) ||
          target.getToggleStorage().value_or("") != "true")
        result = op->emitOpError("requires a push button with toggle storage");
      else if (!controlStateAvailable(target, op))
        result = op->emitOpError("requires the repeated button's row scope");
    } else if (auto focus = dyn_cast<FocusCommandOp>(op)) {
      auto target = elementOwner(focus.getTarget());
      auto action = focus.getAction();
      if (!target || !focusTargetKind(target.getKind())) {
        result = op->emitOpError("requires a focusable target");
      } else if ((action == "open-popup" || action == "close-popup") &&
                 target.getKind() != "ComboBox") {
        result = op->emitOpError("popup commands require a ComboBox target");
      } else {
        for (auto current = target; current; current = current->getParentOfType<ElementOp>())
          if (!current.getBody().front().getOps<RepeatOp>().empty()) {
            result = op->emitOpError("requires a statically addressed focus target");
            break;
          }
      }
    } else if (auto scroll = dyn_cast<ScrollTargetOp>(op)) {
      auto target = elementOwner(scroll.getTarget());
      auto control = cast<ElementOp>(op->getParentOp());
      if (!target || !isViewport(target.getKind()) || !isStaticViewport(target) ||
          !isStaticViewport(control))
        result = op->emitOpError("requires a static viewport and scroll control in the same component");
    } else if (auto group = dyn_cast<RadioGroupRefOp>(op)) {
      auto target = dyn_cast_or_null<ElementOp>(owners.lookup(group.getGroup()));
      if (!target || target.getKind() != "ButtonGroup" || !staticPropertyOwner(op->getParentOp()))
        result = op->emitOpError("requires a static option and a ButtonGroup in the same component");
      else if (nonExclusiveButtonGroup(target))
        result = op->emitOpError("option requires an exclusive ButtonGroup");
    } else if (auto group = dyn_cast<CheckGroupRefOp>(op)) {
      auto member = cast<ElementOp>(op->getParentOp());
      auto target = dyn_cast_or_null<ElementOp>(owners.lookup(group.getGroup()));
      if (!nonExclusiveButtonGroup(target) || !staticPropertyOwner(member)) {
        result = op->emitOpError("requires a static toggle and a non-exclusive ButtonGroup");
      } else {
        bool triState = false;
        for (Operation &child : member.getBody().front()) {
          if (auto binding = dyn_cast<BindOp>(child)) {
            if (binding.getName() == "check-state") {
              op->emitOpError("ButtonGroup members require writable check-state");
              return WalkResult::interrupt();
            }
            triState |= binding.getName() == "tristate";
          } else if (auto link = dyn_cast<ControlLinkOp>(child); link && link.getName() == "check-state") {
            triState = true;
            if (auto read = link.getInput().getDefiningOp<CheckGroupReadOp>())
              checkGroupLinks[group.getGroup()].push_back(read.getOwner());
          }
        }
        if (group.getProperty() != (triState ? "check-state" : "checked"))
          result = op->emitOpError("incorrect check group member property");
      }
    } else if (auto read = dyn_cast<CheckGroupReadOp>(op)) {
      if (read.getValue() != "check-state" || !nonExclusiveButtonGroup(dyn_cast_or_null<ElementOp>(owners.lookup(read.getOwner()))))
        result = op->emitOpError("check-state requires a non-exclusive ButtonGroup in the same component");
    } else if (auto command = dyn_cast<CheckGroupCommandOp>(op)) {
      if (!nonExclusiveButtonGroup(dyn_cast_or_null<ElementOp>(owners.lookup(command.getOwner()))))
        result = op->emitOpError("set-check-state requires a non-exclusive ButtonGroup in the same component");
    } else if (auto member = dyn_cast<ActionGroupRefOp>(op)) {
      auto group = elementOwner(member.getGroup());
      auto action = cast<ElementOp>(op->getParentOp());
      if (!group || group.getKind() != "ActionGroup" || action.getKind() != "Action" || member.getValue().empty())
        result = op->emitOpError("requires an Action, a local ActionGroup and a nonempty value");
      else if (!actionGroupValues[member.getGroup()].insert(member.getValue()).second)
        result = op->emitOpError("duplicate ActionGroup value ") << member.getValue();
      else if (!nonExclusiveGroup(group))
        for (auto &child : action.getBody().front())
          if (isa<BindOp, ControlLinkOp>(child) &&
              child.getAttrOfType<StringAttr>("name").getValue() == "checked")
            result = child.emitOpError("exclusive Action checked state belongs to its ActionGroup");
    } else if (auto read = dyn_cast<NativeDialogReadOp>(op)) {
      auto dialog = elementOwner(read.getOwner());
      auto name = read.getValue();
      auto type = read.getResult().getType();
      if (!dialog || !isNativeDialog(dialog.getKind()) ||
          !((name == "pending" && isa<BoolType>(type)) ||
            (dialog.getKind() == "FileDialog" &&
             ((name == "selected-file" && isa<StringType>(type)) ||
              (name == "selected-files" && isa<StringListType>(type))))))
        result = op->emitOpError("requires a typed native dialog result property");
    } else if (auto read = dyn_cast<ActionGroupReadOp>(op)) {
      auto group = elementOwner(read.getOwner());
      if (!group || group.getKind() != "ActionGroup" || nonExclusiveGroup(group) || read.getValue() != "current-value")
        result = op->emitOpError("current-value requires an exclusive ActionGroup in the same component");
    } else if (auto action = dyn_cast<ActionRefOp>(op)) {
      auto target =
          dyn_cast_or_null<ElementOp>(owners.lookup(action.getOwner()));
      if (!target || target.getKind() != "Action")
        result =
            op->emitOpError("requires an Action owner in the same component");
    } else if (auto callback = dyn_cast<CallbackOp>(op)) {
      if (auto instance = dyn_cast<InstanceOp>(op->getParentOp())) {
        result = verifyCallbackSignature(callback,
            lookupCallback(owners, components, instance.getKey(), callback.getName()));
      } else if (auto owner = callback.getAliasOwner()) {
        auto *target = owners.lookup(*owner);
        if (auto global = dyn_cast_or_null<ComponentOp>(target);
            global && global != component && global.getGlobal())
          result = op->emitOpError("global callbacks can be called but cannot be aliased");
        else if (!staticPropertyOwner(op->getParentOp()) || !staticPropertyOwner(target))
          result = op->emitOpError("callback aliases require statically addressed endpoints");
        else
          result = verifyCallbackSignature(callback,
              lookupCallback(owners, components, *owner, callback.getAliasName().value_or("")));
      }
    } else if (auto assign = dyn_cast<AssignOp>(op))
      result = verifyReference(
          op, owners, components, assign.getOwner().value_or("root"),
          assign.getName(), assign.getInput().getType(), true);
    else if (auto call = dyn_cast<CallOp>(op)) {
      if (op->getParentOfType<ClipboardPasteOp>() && !staticPropertyOwner(owners.lookup(call.getOwner())))
        result = op->emitOpError("clipboard-paste cannot call a row component instance");
      else
        result = verifyCall(op, owners, components, call.getOwner(),
                          call.getValue(), call.getResult().getType());
    } else if (auto invoke = dyn_cast<InvokeOp>(op))
      result = verifyCall(op, owners, components, invoke.getOwner(),
                          invoke.getName(), {});
    else if (auto navigation = dyn_cast<NavigateOp>(op)) {
      auto view = stackOwner(navigation.getOwner());
      if (!view)
        result = op->emitOpError("requires a StackView owner");
      else if (!navigation.getArgs().empty()) {
        auto argument = navigation.getArgs().front();
        if (auto array = argument.getDefiningOp<ArrayOp>()) {
          if (array.getItems().empty()) {
            op->emitOpError("StackView path requires at least one route");
            return WalkResult::interrupt();
          }
          llvm::StringSet<> seen;
          for (auto item : array.getItems())
            if (failed(verifyStackRoute(op, view, item, &seen)))
              return WalkResult::interrupt();
        } else if (isa<StringType>(argument.getType())) {
          result = verifyStackRoute(op, view, argument);
        }
      }
    } else if (auto command = dyn_cast<MediaCommandOp>(op)) {
      auto target = elementOwner(command.getOwner());
      auto method = command.getMethod();
      if (!target || (target.getKind() != "MediaPlayer" &&
          !(target.getKind() == "SoundEffect" && (method == "play" || method == "stop"))))
        result = op->emitOpError("requires a media owner supporting this command");
    } else if (auto command = dyn_cast<TimerCommandOp>(op)) {
      auto timer = dyn_cast_or_null<ElementOp>(owners.lookup(command.getOwner()));
      if (!timer || timer.getKind() != "Timer")
        result = op->emitOpError("requires a Timer owner in the same component");
    } else if (auto command = dyn_cast<PopupCommandOp>(op)) {
      auto popup = dyn_cast_or_null<ElementOp>(owners.lookup(command.getOwner()));
      if (!popup || popup.getKind() != "PopupWindow")
        result = op->emitOpError("requires a PopupWindow owner in the same component");
    } else if (auto open = dyn_cast<FileDialogOpenOp>(op)) {
      auto dialog = dyn_cast_or_null<ElementOp>(owners.lookup(open.getOwner()));
      if (!dialog || dialog.getKind() != "FileDialog")
        result = op->emitOpError("requires a FileDialog owner in the same component");
    } else if (auto open = dyn_cast<NativeDialogOpenOp>(op)) {
      auto dialog = dyn_cast_or_null<ElementOp>(owners.lookup(open.getOwner()));
      if (!dialog || !isNativeDialog(dialog.getKind()))
        result = op->emitOpError("requires a native dialog owner in the same component");
    } else if (auto command = dyn_cast<SecondaryWindowCommandOp>(op)) {
      auto window = dyn_cast_or_null<ElementOp>(owners.lookup(command.getOwner()));
      if (!window || window.getKind() != "SecondaryWindow")
        result = op->emitOpError("requires a SecondaryWindow owner in the same component");
    } else if (auto link = dyn_cast<ControlLinkOp>(op)) {
      if (link.getInput().getDefiningOp<CheckGroupReadOp>())
        return WalkResult::advance();
      auto read = link.getInput().getDefiningOp<ReadOp>();
      if (!read)
        return WalkResult::interrupt();
      result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), true);
    } else if (auto state = dyn_cast<InstanceStateOp>(op)) {
      auto instance = cast<InstanceOp>(op->getParentOp());
      result = verifyReference(op, owners, components, instance.getKey(),
                               state.getName(), state.getInput().getType(), true);
    } else if (auto link = dyn_cast<InstanceLinkOp>(op)) {
      auto instance = cast<InstanceOp>(op->getParentOp());
      result = verifyReference(op, owners, components, instance.getKey(),
                               link.getName(), link.getInput().getType(), true, true);
      if (succeeded(result)) {
        auto read = link.getInput().getDefiningOp<ReadOp>();
        if (!read)
          read = instanceRowModel(link);
        if (!read)
          return WalkResult::interrupt();
        result = verifyReference(op, owners, components, read.getOwner().value_or("root"),
                                 read.getValue(), read.getResult().getType(), true);
      }
    } else if (auto link = dyn_cast<RowLinkOp>(op)) {
      auto read = editableRowModel(link);
      if (!read)
        return WalkResult::interrupt();
      result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), true);
    } else if (auto bind = dyn_cast<BindOp>(op)) {
      auto table = dyn_cast<ElementOp>(op->getParentOp());
      if (table && table.getKind() == "StandardTableView" && bind.getName() == "model" &&
          llvm::any_of(table.getBody().front().getOps<ElementOp>(), [](ElementOp column) {
            return column.getKind() == "TableColumn" &&
                   llvm::any_of(column.getBody().front().getOps<BindOp>(), [](BindOp binding) {
                     return binding.getName() == "editable";
                   });
          })) {
        auto read = bind.getInput().getDefiningOp<ReadOp>();
        if (!read)
          result = op->emitOpError("editable columns require a writable model property");
        else
          result = verifyReference(op, owners, components, read.getOwner().value_or("root"),
                                   read.getValue(), read.getResult().getType(), true);
      } else if (auto instance = dyn_cast<InstanceOp>(op->getParentOp());
          instance && !(bind.getName() == "clipboard-text" &&
              cast<ElementOp>(instance->getParentOp()).getKind() == "TableRow"))
        result =
            verifyReference(op, owners, components, instance.getKey(),
                            bind.getName(), bind.getInput().getType(), true);
    }
    return failed(result) ? WalkResult::interrupt() : WalkResult::advance();
  });
  if (references.wasInterrupted())
    return failure();
  llvm::StringSet<> activeGroups, checkedGroups;
  auto acyclic = [&](auto &&self, llvm::StringRef group) -> bool {
    if (checkedGroups.contains(group))
      return true;
    if (!activeGroups.insert(group).second)
      return false;
    auto next = checkGroupLinks.find(group);
    if (next != checkGroupLinks.end())
      for (auto target : next->second)
        if (!self(self, target))
          return false;
    activeGroups.erase(group);
    checkedGroups.insert(group);
    return true;
  };
  for (const auto &entry : checkGroupLinks)
    if (!acyclic(acyclic, entry.getKey()))
      return component.emitOpError("cyclic check group links");
  if (failed(verifyCallbackAliasCycles(component, owners, components)))
    return failure();
  return verifyPropertyLinkCycles(owners);
}

static bool containsElement(ComponentOp component, const Components &components,
                            llvm::DenseSet<Operation *> &visited,
                            bool (*matchesKind)(llvm::StringRef)) {
  if (!visited.insert(component.getOperation()).second)
    return false;
  return component
      .walk([&](Operation *op) {
        if (auto element = dyn_cast<ElementOp>(op);
            element && matchesKind(element.getKind()))
          return WalkResult::interrupt();
        if (auto instance = dyn_cast<InstanceOp>(op))
          if (auto target = components.lookup(instance.getComponent());
              target &&
              containsElement(target, components, visited, matchesKind))
            return WalkResult::interrupt();
        return WalkResult::advance();
      })
      .wasInterrupted();
}

static LogicalResult verifyInstanceTree(ComponentOp component,
                                        const Components &components,
                                        llvm::DenseSet<Operation *> &active,
                                        llvm::DenseSet<Operation *> &done) {
  if (done.contains(component.getOperation()))
    return success();
  if (!active.insert(component.getOperation()).second)
    return component.emitOpError("cyclic component instantiation");
  auto result = component.walk([&](InstanceOp instance) -> WalkResult {
    auto child = components.lookup(instance.getComponent());
    auto roots = child.getBody().front().getOps<ElementOp>();
    if (llvm::hasSingleElement(roots) && (*roots.begin()).getKind() == "SystemTrayIcon") {
      instance.emitOpError("SystemTrayIcon requires a top-level component");
      return WalkResult::interrupt();
    }
    if (cast<ElementOp>(instance->getParentOp()).getKind() == "SecondaryWindow") {
      if (!llvm::hasSingleElement(roots) || !isWindow((*roots.begin()).getKind())) {
        instance.emitOpError("SecondaryWindow requires a component rooted in Window");
        return WalkResult::interrupt();
      }
    }
    for (auto ancestor = instance->getParentOfType<ElementOp>(); ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>()) {
      if (ancestor.getKind() != "Tooltip")
        continue;
      llvm::DenseSet<Operation *> visited;
      if (containsElement(child, components, visited, forbiddenInTooltip)) {
        instance.emitOpError(
            "Tooltip content cannot be a popup or virtual view");
        return WalkResult::interrupt();
      }
      break;
    }
    if (instanceRepeat(instance) && enclosingRepeatedElement(instance)) {
      llvm::DenseSet<Operation *> visited;
      if (containsElement(child, components, visited, [](llvm::StringRef kind) {
            return isWindow(kind) || kind == "SecondaryWindow";
          })) {
        instance.emitOpError("row component delegates cannot contain a window");
        return WalkResult::interrupt();
      }
    }
    return failed(verifyInstanceTree(child, components, active, done))
               ? WalkResult::interrupt()
               : WalkResult::advance();
  });
  active.erase(component.getOperation());
  done.insert(component.getOperation());
  return failure(result.wasInterrupted());
}

using Fields = llvm::SmallVector<std::pair<std::string, std::string>>;
using Records = llvm::StringMap<Fields>;

struct EnumVariantLayout {
  bool tuple = false;
  Fields fields;
};
using EnumLayouts = llvm::StringMap<llvm::SmallVector<EnumVariantLayout>>;

static bool recordIdentifier(llvm::StringRef name) {
  if (name.empty() || name.starts_with("__") ||
      llvm::StringSwitch<bool>(name)
          .Cases({"_", "self", "Self", "super", "crate", "root"}, true)
          .Default(false))
    return false;
  return (llvm::isAlpha(name.front()) || name.front() == '_') &&
         llvm::all_of(name, [](char c) {
           return llvm::isAlnum(c) || c == '_' || c == '-';
         });
}

static bool recordScalarType(llvm::StringRef type) {
  return llvm::StringSwitch<bool>(type)
      .Cases({"int", "float", "bool", "string", "styled-text", "length", "color", "brush",
              "point2d", "vector2d", "transform2d", "Point2D", "Vector2D", "Transform2D",
              "point3d", "Point3D", "vector3d", "Vector3D", "hit3d", "Hit3D",
              "projectedpoint3d", "ProjectedPoint3D",
              "image", "LogLevel", "ImageFit", "ImageTileMode", "ImageHorizontalAlignment", "ImageVerticalAlignment", "ImageRendering", "ButtonDisplay",
              "LayoutAlignment", "CrossAxisAlignment",
              "Orientation", "FlickDirection", "FlickBoundsBehavior", "FlickBoundsMovement", "LayoutDirection", "FlexboxLayoutDirection", "FlexboxLayoutWrap", "DialInputMode", "ScrollBarPolicy", "ScrollSnapMode", "ScrollAlignment", "TextWrap", "MouseSelectionMode", "InputValidity",
              "TextHorizontalAlignment", "TextVerticalAlignment", "TextOverflow", "InputType", "InputPurpose", "KeyboardCapitalization", "ReturnKey", "AccessibleLive", "CursorShape", "ColorScheme", "SceneToneMap", "FileDialogMode", "MessageLevel", "MessageButtons",
              "DialogRole", "StandardButtonKind", "SwipeSide", "DragAction", "KeyEvent", "KeyModifiers", "WheelEvent", "PointerEvent", "PointerEventKind", "PointerButton", "PointerButtons", "TouchPoint", "EventResult", "FocusReason", "DrawerEdge", "PopupPlacement", "SelectionTarget", "SelectionGesture", "Date", "Time", "Font", "CheckState"},
             true)
      .Default(false);
}

static std::optional<llvm::StringRef> recordListElement(llvm::StringRef type) {
  if (type.consume_front("[") && type.consume_back("]") && recordIdentifier(type))
    return type;
  return std::nullopt;
}

static bool validRecordFieldType(llvm::StringRef type) {
  if (recordScalarType(type) || recordIdentifier(type))
    return true;
  auto element = recordListElement(type);
  return element && (*element == "int" || !recordScalarType(*element));
}

static LogicalResult indexRecord(RecordDeclOp record, Records &records, EnumLayouts &enums) {
  if (!recordIdentifier(record.getSymName()) ||
      recordScalarType(record.getSymName()))
    return record.emitOpError("invalid record name");
  auto parsed = llvm::json::parse(record.getFields());
  if (!parsed) {
    llvm::consumeError(parsed.takeError());
    return record.emitOpError("invalid record fields");
  }
  auto *array = parsed->getAsArray();
  if (!array || array->empty())
    return record.emitOpError("requires at least one record field");
  Fields fields;
  llvm::StringSet<> names;
  for (auto &value : *array) {
    auto *object = value.getAsObject();
    auto name = object ? object->getString("name") : std::nullopt;
    auto type = object ? object->getString("ty") : std::nullopt;
    auto rustName = name.value_or("").str();
    std::replace(rustName.begin(), rustName.end(), '-', '_');
    if (!name || !recordIdentifier(*name) || !names.insert(rustName).second ||
        !type || !validRecordFieldType(*type))
      return record.emitOpError(
          "record fields require unique names and supported types");
    fields.emplace_back(name->str(), type->str());
  }
  if (auto variants = record->getAttrOfType<ArrayAttr>("variants")) {
    if (variants.empty() || fields.size() != 1 ||
        fields.front().first != "discriminant" || fields.front().second != "int")
      return record.emitOpError("an enum requires variants and an integer discriminant");
    llvm::StringSet<> variantNames;
    for (Attribute variant : variants) {
      auto name = dyn_cast<StringAttr>(variant);
      if (!name || !recordIdentifier(name.getValue()))
        return record.emitOpError("invalid enum variant name");
      auto normalized = name.getValue().str();
      std::replace(normalized.begin(), normalized.end(), '-', '_');
      if (!variantNames.insert(normalized).second)
        return record.emitOpError("duplicate enum variant ") << name.getValue();
    }
    llvm::SmallVector<EnumVariantLayout> layouts;
    if (auto payloads = record->getAttrOfType<StringAttr>("variant_payloads")) {
      auto parsed = llvm::json::parse(payloads.getValue());
      if (!parsed) {
        llvm::consumeError(parsed.takeError());
        return record.emitOpError("invalid enum payload schema");
      }
      auto *array = parsed->getAsArray();
      if (!array || array->size() != variants.size())
        return record.emitOpError("enum payloads must match the declared variants");
      for (auto [index, value] : llvm::enumerate(*array)) {
        const auto *variant = value.getAsObject();
        auto name = variant ? variant->getString("name") : std::nullopt;
        auto shape = variant ? variant->getString("shape") : std::nullopt;
        const auto *payload = variant ? variant->getArray("fields") : nullptr;
        if (!name || *name != cast<StringAttr>(variants[index]).getValue() ||
            !shape || !payload ||
            (*shape != "unit" && *shape != "tuple" && *shape != "named") ||
            ((*shape == "unit") != payload->empty()))
          return record.emitOpError("invalid variant payload declaration");
        EnumVariantLayout layout;
        layout.tuple = *shape == "tuple";
        llvm::StringSet<> fieldNames;
        for (auto [fieldIndex, value] : llvm::enumerate(*payload)) {
          const auto *field = value.getAsObject();
          auto fieldName = field ? field->getString("name") : std::nullopt;
          auto type = field ? field->getString("ty") : std::nullopt;
          auto normalized = fieldName.value_or("").str();
          std::replace(normalized.begin(), normalized.end(), '-', '_');
          auto positional = ("_" + llvm::Twine(fieldIndex)).str();
          if (!fieldName || !recordIdentifier(*fieldName) ||
              *fieldName == "discriminant" || !fieldNames.insert(normalized).second ||
              (layout.tuple && *fieldName != positional) ||
              !type || !validRecordFieldType(*type))
            return record.emitOpError("invalid enum payload field");
          layout.fields.emplace_back(fieldName->str(), type->str());
        }
        layouts.push_back(std::move(layout));
      }
    } else {
      layouts.resize(variants.size());
    }
    enums.try_emplace(record.getSymName(), std::move(layouts));
  } else if (record->hasAttr("variant_payloads")) {
    return record.emitOpError("payloads require an enum declaration");
  }
  if (!records.try_emplace(record.getSymName(), std::move(fields)).second)
    return record.emitOpError("duplicate record ") << record.getSymName();
  return success();
}

static LogicalResult verifyRecordLayouts(ModuleOp module, const Records &records,
                                         const EnumLayouts &enums) {
  llvm::StringMap<RecordDeclOp> declarations;
  for (auto record : module.getBody()->getOps<RecordDeclOp>())
    declarations.try_emplace(record.getSymName(), record);
  llvm::StringSet<> active, done;
  auto visit = [&](auto &&visit, RecordDeclOp record) -> LogicalResult {
    auto name = record.getSymName();
    if (done.contains(name))
      return success();
    if (!active.insert(name).second)
      return record.emitOpError("cyclic record layout involving ") << name;
    auto visitFields = [&](const Fields &fields) -> LogicalResult {
      for (const auto &field : fields) {
        llvm::StringRef type = field.second;
        if (recordScalarType(type) || type == "[int]")
          continue;
        auto target = declarations.find(recordListElement(type).value_or(type));
        if (target == declarations.end())
          return record.emitOpError("unknown record type ") << field.second;
        if (failed(visit(visit, target->second)))
          return failure();
      }
      return success();
    };
    if (failed(visitFields(records.find(name)->second)))
      return failure();
    if (auto found = enums.find(name); found != enums.end())
      for (auto &variant : found->second)
        if (failed(visitFields(variant.fields)))
          return failure();
    active.erase(name);
    done.insert(name);
    return success();
  };
  for (auto record : module.getBody()->getOps<RecordDeclOp>())
    if (failed(visit(visit, record)))
      return failure();
  return success();
}

static std::optional<llvm::StringRef> recordFieldType(
    const Records &records, const EnumLayouts &enums, llvm::StringRef record, llvm::StringRef path) {
  while (!path.empty()) {
    if (enums.contains(record))
      return std::nullopt;
    auto schema = records.find(record);
    if (schema == records.end())
      return std::nullopt;
    auto [name, rest] = path.split('.');
    auto field = llvm::find_if(schema->second, [&](const auto &entry) {
      return entry.first == name;
    });
    if (field == schema->second.end())
      return std::nullopt;
    if (name.size() == path.size())
      return field->second;
    record = field->second;
    path = rest;
  }
  return std::nullopt;
}

static LogicalResult verifyStandardTable(ElementOp table, const Records &records, const EnumLayouts &enums) {
  auto &body = table.getBody().front();
  auto bindings = body.getOps<BindOp>();
  auto model = llvm::find_if(bindings, [](BindOp bind) {
    return bind.getName() == "model";
  });
  if (model == bindings.end())
    return table.emitOpError("missing record model");
  auto record = dyn_cast<RecordListType>((*model).getInput().getType());
  if (!record)
    return (*model).emitOpError("requires a record list");
  if (!records.contains(record.getName()))
    return (*model).emitOpError("unknown record type ") << record.getName();
  auto verifyField = [&](BindOp field) -> LogicalResult {
    auto literal = field.getInput().getDefiningOp<LiteralOp>();
    if (!literal)
      return field.emitOpError("field requires a constant string");
    auto parsed = llvm::json::parse(literal.getValue());
    if (!parsed) {
      llvm::consumeError(parsed.takeError());
      return field.emitOpError("field requires a constant string");
    }
    auto name = parsed->getAsString();
    if (!name)
      return field.emitOpError("field requires a constant string");
    auto type = recordFieldType(records, enums, record.getName(), *name);
    if (!type)
      return field.emitOpError("record has no field ") << *name;
    if (!llvm::StringSwitch<bool>(*type)
             .Cases({"string", "int", "float", "bool", "Date", "Time"}, true)
             .Default(false))
      return field.emitOpError("field requires a string, int, float, bool, Date or Time field");
    return success();
  };
  for (auto bind : bindings)
    if (bind.getName() == "row-header-field" && failed(verifyField(bind)))
      return failure();
  for (auto column : body.getOps<ElementOp>()) {
    if (column.getKind() != "TableColumn")
      continue;
    auto columnBindings = column.getBody().front().getOps<BindOp>();
    auto role = llvm::find_if(columnBindings, [](BindOp bind) {
      return bind.getName() == "field";
    });
    if (role == columnBindings.end())
      return column.emitOpError("missing record field");
    if (failed(verifyField(*role)))
      return failure();
  }
  return success();
}

static FailureOr<std::string> comboRole(BindOp binding, llvm::StringRef fallback) {
  if (!binding)
    return fallback.str();
  if (auto literal = binding.getInput().getDefiningOp<LiteralOp>()) {
    auto parsed = llvm::json::parse(literal.getValue());
    if (parsed) {
      if (auto role = parsed->getAsString())
        return role->str();
    } else {
      llvm::consumeError(parsed.takeError());
    }
  }
  binding.emitOpError("ComboBox role requires a constant string");
  return failure();
}

static LogicalResult verifyComboModel(ElementOp combo, const Records &records, const EnumLayouts &enums,
                                      ComboValueTypes &comboValues) {
  auto &body = combo.getBody().front();
  auto kind = combo.getKind();
  BindOp model, textRole, valueRole, enabledRole;
  for (auto bind : body.getOps<BindOp>()) {
    if (bind.getName() == "model") model = bind;
    if (bind.getName() == "text-role") textRole = bind;
    if (bind.getName() == "value-role") valueRole = bind;
    if (bind.getName() == "enabled-role") enabledRole = bind;
  }
  auto record = model ? dyn_cast<RecordListType>(model.getInput().getType()) : RecordListType();
  if (!record && (textRole || valueRole || enabledRole))
    return combo.emitOpError() << kind << " roles require a record model";

  std::string expected = "string";
  if (record) {
    auto text = comboRole(textRole, "text");
    if (failed(text)) return failure();
    auto value = comboRole(valueRole, *text);
    if (failed(value)) return failure();
    if (!records.contains(record.getName()))
      return model.emitOpError("unknown record type ") << record.getName();
    if (recordFieldType(records, enums, record.getName(), *text) != "string")
      return model.emitOpError() << kind << " text-role requires a string field named " << *text;
    if (enabledRole) {
      auto enabled = comboRole(enabledRole, "");
      if (failed(enabled)) return failure();
      if (recordFieldType(records, enums, record.getName(), *enabled) != "bool")
        return enabledRole.emitOpError("ComboBox enabled-role requires a bool field named ") << *enabled;
    }
    auto type = recordFieldType(records, enums, record.getName(), *value);
    if (!type ||
        !llvm::StringSwitch<bool>(*type).Cases({"string", "int", "float", "bool", "Date", "Time"}, true).Default(false))
      return model.emitOpError() << kind << " value-role requires a scalar field named " << *value;
    expected = type->str();
  }

  if (kind == "ComboBox" || record)
    comboValues.try_emplace(combo.getOperation(), expected);
  bool byIndex = false;
  bool byValue = false;
  for (Operation &child : body) {
    if (isa<BindOp, ControlLinkOp>(child)) {
      auto name = child.getAttrOfType<StringAttr>("name").getValue();
      byIndex |= name == "current-index";
      byValue |= name == "current-value";
      if (name == "current-value") {
        if ((kind == "Tumbler" || kind == "PathView") && !record)
          return child.emitOpError() << "current-value requires a record " << kind << " model";
        if (comboValueTypeName(child.getOperand(0).getType()) != expected)
          return child.emitOpError() << kind << " current-value requires " << expected;
      }
    }
    if (auto handler = dyn_cast<HandlerOp>(child); handler && handler.getName() == "value-selected") {
      if (!record)
        return handler.emitOpError() << "value-selected requires a record " << kind << " model";
      auto parameters = llvm::json::parse(handler.getParameters());
      if (!parameters) {
        llvm::consumeError(parameters.takeError());
        return handler.emitOpError("value-selected requires one parameter");
      }
      auto *args = parameters->getAsArray();
      auto *arg = args && args->size() == 1 ? args->front().getAsObject() : nullptr;
      if (!arg || !arg->getString("name") || arg->getString("ty") != expected)
        return handler.emitOpError() << kind << " value-selected requires one " << expected << " parameter";
    }
  }
  if ((kind == "Tumbler" || kind == "PathView") && byIndex && byValue)
    return combo.emitOpError() << kind << " selection accepts either current-index or current-value";
  return success();
}

static LogicalResult verifyViewRecordField(ElementOp element, const Records &records, const EnumLayouts &enums,
                                           llvm::StringRef property, llvm::StringRef expected) {
  auto &body = element.getBody().front();
  BindOp role;
  Value model;
  bool hasFilterText = false;
  for (auto bind : body.getOps<BindOp>()) {
    hasFilterText |= bind.getName() == "filter-text";
    if (bind.getName() == property) {
      if (role)
        return bind.emitOpError("duplicate ") << property;
      role = bind;
    } else if (bind.getName() == "model")
      model = bind.getInput();
  }
  if (!role) {
    if (property == "filter-field" && hasFilterText)
      return element.emitOpError() << element.getKind() << " filter-text requires filter-field";
    return success();
  }
  if (element.getKind() != "StandardListView" && element.getKind() != "StandardTableView") {
    model = Value{};
    for (auto &child : body)
      if (isa<ElementOp, InstanceOp>(child) && !child.getRegion(0).empty())
        for (auto repeat : child.getRegion(0).front().getOps<RepeatOp>())
          model = repeat.getInput();
  }
  auto record = model ? dyn_cast<RecordListType>(model.getType()) : RecordListType();
  if (!record)
    return role.emitOpError() << property << " requires a record model";
  auto literal = role.getInput().getDefiningOp<LiteralOp>();
  if (!literal)
    return role.emitOpError() << property << " requires a constant string";
  auto parsed = llvm::json::parse(literal.getValue());
  if (!parsed) {
    llvm::consumeError(parsed.takeError());
    return role.emitOpError() << property << " requires a constant string";
  }
  auto name = parsed->getAsString();
  if (!name)
    return role.emitOpError() << property << " requires a constant string";
  if (recordFieldType(records, enums, record.getName(), *name) != expected)
    return role.emitOpError() << property << " requires a " << expected << " field named " << *name;
  return success();
}

static LogicalResult verifyEnumOperations(Operation *op,
                                         const EnumLayouts &enums) {
  if (auto make = dyn_cast<EnumConstructOp>(op)) {
    auto type = cast<RecordType>(make.getResult().getType());
    auto layout = enums.find(type.getName());
    if (layout == enums.end() || make.getTag() >= layout->second.size())
      return op->emitOpError("unknown enum variant");
    const auto &variant = layout->second[make.getTag()];
    if (make.getFields().size() != variant.fields.size() ||
        make.getPayload().size() != variant.fields.size())
      return op->emitOpError("wrong enum payload field count");
    llvm::StringSet<> seen;
    for (auto [index, entry] : llvm::enumerate(llvm::zip(make.getFields(), make.getPayload()))) {
      auto name = dyn_cast<StringAttr>(std::get<0>(entry));
      auto field = llvm::find_if(variant.fields, [&](const auto &field) {
        return name && field.first == name.getValue();
      });
      if (field == variant.fields.end() || !seen.insert(name.getValue()).second ||
          (variant.tuple && field->first != variant.fields[index].first) ||
          !matchesDeclaredType(std::get<1>(entry).getType(), field->second))
        return op->emitOpError("invalid enum payload field or type");
    }
  } else if (auto match = dyn_cast<EnumMatchOp>(op)) {
    auto type = cast<RecordType>(match.getSubject().getType());
    auto layout = enums.find(type.getName());
    if (layout == enums.end() ||
        static_cast<size_t>(std::distance(match.getBody().front().getOps<EnumCaseOp>().begin(),
                                         match.getBody().front().getOps<EnumCaseOp>().end())) !=
            layout->second.size())
      return op->emitOpError("match must cover all variants of its enum type");
  } else if (auto legacy = dyn_cast<MatchEnumOp>(op)) {
    Value tag = legacy.getTag();
    if (auto literal = tag.getDefiningOp<LiteralOp>()) {
      unsigned index;
      if (!literal.getValue().getAsInteger(10, index) &&
          index < legacy.getArms().size())
        return success();
    } else if (auto field = tag.getDefiningOp<GetFieldOp>()) {
      auto type = cast<RecordType>(field.getInput().getType());
      auto layout = enums.find(type.getName());
      if (field.getValue() == "discriminant" && layout != enums.end() &&
          layout->second.size() == legacy.getArms().size())
        return success();
    }
    return op->emitOpError("legacy match requires a valid enum discriminant");
  } else if (auto parameter = dyn_cast<EnumParameterOp>(op)) {
    auto [scope, field] = parameter.getValue().rsplit('_');
    unsigned index;
    if (scope.empty() || field.getAsInteger(10, index))
      return op->emitOpError("invalid enum binding identifier");
    for (auto *parent = op->getParentOp(); parent; parent = parent->getParentOp()) {
      auto arm = dyn_cast<EnumCaseOp>(parent);
      if (!arm || arm.getName() != scope)
        continue;
      auto match = arm->getParentOfType<EnumMatchOp>();
      auto type = cast<RecordType>(match.getSubject().getType());
      auto layout = enums.find(type.getName());
      if (layout == enums.end() || arm.getOrdinal() >= layout->second.size())
        break;
      const auto &fields = layout->second[arm.getOrdinal()].fields;
      if (index >= fields.size() ||
          !matchesDeclaredType(parameter.getResult().getType(), fields[index].second))
        return op->emitOpError("invalid enum binding type or field index");
      return success();
    }
    return op->emitOpError("enum binding has no enclosing matching case");
  } else if (auto make = dyn_cast<MakeRecordOp>(op)) {
    auto type = cast<RecordType>(make.getResult().getType());
    if (auto layout = enums.find(type.getName()); layout != enums.end()) {
      auto literal = make.getFields().size() == 1
                         ? make.getFields().front().getDefiningOp<LiteralOp>()
                         : LiteralOp();
      unsigned index;
      if (!literal || literal.getValue().getAsInteger(10, index) ||
          index >= layout->second.size() || !layout->second[index].fields.empty())
        return op->emitOpError("legacy enum constructor requires a valid constant unit tag");
    }
  } else if (auto update = dyn_cast<UpdateRecordOp>(op)) {
    if (enums.contains(cast<RecordType>(update.getResult().getType()).getName()))
      return op->emitOpError("record updates cannot set an enum discriminant");
  }
  return success();
}

static LogicalResult verifySceneRecords(
    Operation *op, Type type, const Records &records, const EnumLayouts &enums,
    llvm::StringRef subject,
    std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>> fields,
    std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>> optionalFields) {
  auto model = dyn_cast<RecordListType>(type);
  if (!model)
    return success();
  for (const auto &[field, expected] : fields) {
    if (recordFieldType(records, enums, model.getName(), field) != expected)
      return op->emitOpError() << subject << " requires a " << expected << " field named " << field;
  }
  for (const auto &[field, expected] : optionalFields)
    if (auto actual = recordFieldType(records, enums, model.getName(), field);
        actual && *actual != expected)
      return op->emitOpError() << subject << " requires a " << expected << " field named " << field;
  return success();
}

static LogicalResult verifySceneMaterial(Operation *op, llvm::StringRef material,
                                         const Records &records, const EnumLayouts &enums) {
  auto style = records.find(material);
    if (style == records.end() || enums.contains(material))
      return op->emitOpError("3D model requires material to be a record");
    for (const auto &[name, expected] :
        std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
          {"enabled", "bool"}, {"vertex-colors", "bool"}, {"base-color", "color"}, {"base-item", "string"}, {"metalness", "float"},
          {"material-shader", "string"}, {"shader-parameters", "vector3d"}, {"shader-displacement", "float"},
          {"roughness", "float"}, {"emissive-color", "color"}, {"emissive-strength", "float"},
          {"alpha-mode", "string"}, {"blend-mode", "string"}, {"depth-draw-mode", "string"}, {"cull-mode", "string"}, {"alpha-cutoff", "float"},
          {"unlit", "bool"}, {"clearcoat", "float"}, {"clearcoat-roughness", "float"},
          {"transmission", "float"}, {"index-of-refraction", "float"},
          {"occlusion-amount", "float"}, {"specular-amount", "float"},
          {"specular-color", "color"}, {"thickness", "float"},
          {"attenuation-color", "color"}, {"attenuation-distance", "float"},
          {"dispersion", "float"}, {"anisotropy-strength", "float"},
          {"anisotropy-rotation", "float"},
          {"fresnel-power", "float"}, {"fresnel-scale", "float"},
          {"fresnel-bias", "float"}, {"fresnel-scale-bias-enabled", "bool"},
          {"clearcoat-fresnel-power", "float"}, {"clearcoat-fresnel-scale", "float"},
          {"clearcoat-fresnel-bias", "float"}, {"clearcoat-fresnel-scale-bias-enabled", "bool"},
          {"normal-strength", "float"}, {"clearcoat-normal-strength", "float"}}) {
      auto field = recordFieldType(records, enums, material, name);
      if (field && *field != expected)
        return op->emitOpError("invalid 3D material field ") << name << ", expected " << expected;
    }
    if (auto inputs = recordFieldType(records, enums, material, "shader-textures")) {
      auto element = recordListElement(*inputs);
      if (!element ||
          (recordFieldType(records, enums, *element, "texture") != "string" &&
           recordFieldType(records, enums, *element, "item") != "string" &&
           recordFieldType(records, enums, *element, "capture") != "string"))
        return op->emitOpError("3D material shader-textures requires a record list with a string field named texture, item or capture");
      for (llvm::StringRef name : {"texture", "item", "capture", "attachment", "filter", "wrap-u", "wrap-v", "color-space"})
        if (auto field = recordFieldType(records, enums, *element, name); field && *field != "string")
          return op->emitOpError("3D material shader texture requires a string field named ") << name;
      if (auto enabled = recordFieldType(records, enums, *element, "enabled"); enabled && *enabled != "bool")
        return op->emitOpError("3D material shader texture requires a bool field named enabled");
    }
    for (llvm::StringRef name : {"base-color-channel", "emissive-channel", "metalness-channel", "roughness-channel", "occlusion-channel", "specular-channel", "specular-color-channel", "transmission-channel", "thickness-channel", "clearcoat-channel", "clearcoat-roughness-channel"}) {
      if (auto field = recordFieldType(records, enums, material, name);
          field && *field != "string")
        return op->emitOpError("invalid 3D material channel ") << name << ", expected string";
    }
    for (llvm::StringRef name : {
          "vertex-metalness-channel", "vertex-roughness-channel",
          "vertex-occlusion-channel", "vertex-specular-channel",
          "vertex-normal-channel", "vertex-clearcoat-channel",
          "vertex-clearcoat-roughness-channel", "vertex-clearcoat-normal-channel",
          "vertex-transmission-channel", "vertex-thickness-channel"}) {
      if (auto field = recordFieldType(records, enums, material, name);
          field && *field != "string")
        return op->emitOpError("invalid 3D vertex mask channel ") << name << ", expected string";
    }
    for (llvm::StringRef name : {"base-color-map", "metallic-roughness-map",
                               "emissive-map", "occlusion-map", "normal-map",
                               "clearcoat-map", "clearcoat-roughness-map",
                               "clearcoat-normal-map", "transmission-map",
                               "specular-map", "specular-color-map", "thickness-map", "anisotropy-map"}) {
    auto texture = recordFieldType(records, enums, material, name);
    if (!texture || *texture == "string")
      continue;
    auto schema = records.find(*texture);
    if (schema == records.end() || enums.contains(*texture))
      return op->emitOpError("3D material map requires a texture key or record");
    for (const auto &[setting, expected] :
         std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
             {"source", "string"}, {"wrap-u", "string"}, {"wrap-v", "string"},
             {"min-filter", "string"}, {"mag-filter", "string"}, {"mip-filter", "string"},
             {"uv-set", "int"}, {"uv-scale-u", "float"}, {"uv-scale-v", "float"},
             {"uv-offset-u", "float"}, {"uv-offset-v", "float"}, {"uv-rotation", "float"}}) {
      auto field = recordFieldType(records, enums, *texture, setting);
      if ((setting == "source" || field) && (!field || *field != expected))
        return op->emitOpError("invalid 3D texture field ") << setting << ", expected " << expected;
    }
  }
  return success();
}

static LogicalResult verifySceneModel(Operation *op, Type type, const Records &records,
                                      const EnumLayouts &enums) {
  if (failed(verifySceneRecords(
      op, type, records, enums, "3D model",
      {{"id", "string"}, {"x", "float"}, {"y", "float"}, {"z", "float"},
       {"scale-x", "float"}, {"scale-y", "float"}, {"scale-z", "float"}, {"yaw", "float"},
       {"pitch", "float"}, {"roll", "float"}, {"color", "color"}},
      {{"mesh", "string"}, {"casts-shadow", "bool"}, {"receives-shadow", "bool"}, {"receives-reflections", "bool"},
       {"casts-reflections", "bool"}, {"parent", "string"}, {"layers", "int"},
       {"visible", "bool"}, {"lod-of", "string"}, {"lod-max-size", "length"}, {"lod-bias", "float"},
       {"camera-lod-bias", "float"}, {"pickable", "bool"}, {"collidable", "bool"}, {"pivot-x", "float"}, {"pivot-y", "float"}, {"pivot-z", "float"},
       {"camera-kind", "string"}, {"camera-fov", "float"}, {"camera-height", "float"},
       {"camera-near", "float"}, {"camera-far", "float"}, {"camera-look-at", "string"},
       {"camera-left", "float"}, {"camera-right", "float"},
       {"camera-bottom", "float"}, {"camera-top", "float"},
       {"material-variant", "string"}, {"instance-of", "string"}, {"instance-template", "bool"},
       {"animation", "string"}, {"animation-time", "float"}, {"animation-loop", "bool"},
       {"animation-to", "string"}, {"animation-to-time", "float"},
       {"animation-to-loop", "bool"}, {"animation-blend", "float"},
       {"lightmap", "image"}, {"lightmap-mode", "string"}, {"lightmap-intensity", "float"},
       {"lightmap-u", "float"}, {"lightmap-v", "float"}, {"lightmap-width", "float"},
       {"lightmap-height", "float"}})))
    return failure();
  if (auto model = dyn_cast<RecordListType>(type))
    if (auto weights = recordFieldType(records, enums, model.getName(), "morph-weights")) {
      auto schema = records.find(*weights);
      if (schema == records.end() || enums.contains(*weights) ||
          !llvm::all_of(schema->second, [](const auto &field) { return field.second == "float"; }))
        return op->emitOpError("3D model requires a morph-weights record with float fields");
    }
  if (auto model = dyn_cast<RecordListType>(type)) {
    if (auto geometry = recordFieldType(records, enums, model.getName(), "geometry")) {
      if (!records.contains(*geometry) || enums.contains(*geometry))
        return op->emitOpError("3D model requires geometry to be a record");
      if (auto vertices = recordFieldType(records, enums, *geometry, "vertices")) {
        if (recordFieldType(records, enums, *geometry, "shape"))
          return op->emitOpError("3D geometry cannot declare both shape and vertices");
        auto vertex = recordListElement(*vertices);
        if (!vertex || !records.contains(*vertex) || enums.contains(*vertex))
          return op->emitOpError("3D geometry requires vertices to be a list of vertex records");
        for (const auto &[name, expected] :
             std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
                 {"position", "point3d"}, {"normal", "vector3d"},
                 {"u", "float"}, {"v", "float"}, {"u1", "float"}, {"v1", "float"}, {"color", "color"}}) {
          auto field = recordFieldType(records, enums, *vertex, name);
          if ((name == "position" || field) && (!field || *field != expected))
            return op->emitOpError("invalid 3D vertex field ") << name << ", expected " << expected;
        }
        if (auto indices = recordFieldType(records, enums, *geometry, "indices"); indices && *indices != "[int]")
          return op->emitOpError("3D geometry indices require [int]");
        if (auto subsets = recordFieldType(records, enums, *geometry, "subsets")) {
          auto subset = recordListElement(*subsets);
          if (!subset || !records.contains(*subset) || enums.contains(*subset))
            return op->emitOpError("3D geometry requires subsets to be a list of subset records");
          for (const auto &[name, expected] :
               std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
                   {"offset", "int"}, {"count", "int"}, {"name", "string"}}) {
            auto field = recordFieldType(records, enums, *subset, name);
            if (!field || *field != expected)
              return op->emitOpError("invalid 3D subset field ") << name << ", expected " << expected;
          }
        }
      } else {
        if (recordFieldType(records, enums, *geometry, "subsets"))
          return op->emitOpError("3D geometry requires vertices when declaring subsets");
        for (const auto &[name, expected] :
             std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
                 {"shape", "string"}, {"width", "float"}, {"height", "float"},
                 {"depth", "float"}, {"radius", "float"}, {"top-radius", "float"},
                 {"tube-radius", "float"}, {"segments", "int"}, {"rings", "int"},
                 {"capped", "bool"}}) {
          auto field = recordFieldType(records, enums, *geometry, name);
          if ((name == "shape" || field) && (!field || *field != expected))
            return op->emitOpError("invalid 3D geometry field ") << name << ", expected " << expected;
        }
      }
    } else if (!recordFieldType(records, enums, model.getName(), "mesh")) {
      return op->emitOpError("3D model requires a string mesh field or a geometry record");
    }
    if (auto material = recordFieldType(records, enums, model.getName(), "material"))
      if (failed(verifySceneMaterial(op, *material, records, enums)))
        return failure();
    if (auto palette = recordFieldType(records, enums, model.getName(), "material-slots")) {
      auto schema = records.find(*palette);
      if (schema == records.end() || enums.contains(*palette))
        return op->emitOpError("3D material-slots requires a record of material bindings");
      for (const auto &[name, material] : schema->second) {
        if (failed(verifySceneMaterial(op, material, records, enums)))
          return failure();
        if (auto slot = recordFieldType(records, enums, material, "slot"); slot && *slot != "string")
          return op->emitOpError("3D material slot requires a string selector for ") << name;
      }
    }
  }
  return success();
}

static LogicalResult verifySceneLights(BindOp lights, const Records &records,
                                       const EnumLayouts &enums) {
  return verifySceneRecords(
      lights, lights.getInput().getType(), records, enums, "3D lights",
      {{"kind", "string"}, {"x", "float"}, {"y", "float"}, {"z", "float"}, {"yaw", "float"},
       {"elevation", "float"}, {"intensity", "float"}, {"color", "color"}, {"range", "float"},
       {"inner-angle", "float"}, {"outer-angle", "float"}},
      {{"casts-shadow", "bool"}, {"parent", "string"}, {"visible", "bool"}, {"layers", "int"}});
}

static LogicalResult verifyParticleEmitters(BindOp emitters, const Records &records,
                                            const EnumLayouts &enums) {
  return verifySceneRecords(
      emitters, emitters.getInput().getType(), records, enums, "3D particle emitters",
      {{"position", "point3d"}, {"color", "color"}, {"size", "float"}, {"lifetime", "float"}},
      {{"end-size", "float"}, {"start-time", "float"}, {"rate", "float"},
       {"burst-count", "int"}, {"velocity", "vector3d"}, {"velocity-spread", "vector3d"},
       {"gravity", "vector3d"}, {"fade-in", "float"}, {"fade-out", "float"},
       {"intensity", "float"}, {"seed", "int"}, {"layers", "int"}, {"enabled", "bool"},
       {"sprite", "image"}, {"sprite-frames", "int"}, {"sprite-frame", "int"},
       {"sprite-duration", "float"}, {"sprite-duration-variation", "float"},
       {"sprite-playback", "string"}, {"sprite-interpolate", "bool"},
       {"sprite-random-start", "bool"}, {"color-table", "image"},
       {"particle-blend-mode", "string"}, {"emitter-shape", "string"},
       {"emitter-mesh", "string"}, {"emitter-mesh-scale", "vector3d"},
       {"emitter-extents", "vector3d"}, {"emitter-fill", "bool"}, {"emitter-yaw", "float"},
       {"emitter-pitch", "float"}, {"emitter-roll", "float"}, {"velocity-mode", "string"},
       {"velocity-cone-angle", "float"}, {"velocity-target", "point3d"},
       {"velocity-target-spread", "vector3d"}, {"velocity-speed-variation", "float"},
       {"trail-segments", "int"}, {"trail-duration", "float"}, {"trail-end-scale", "float"},
       {"trail-fade", "float"}, {"trail-eol-fade", "float"},
       {"particle-model", "string"}, {"particle-rotation", "vector3d"},
       {"particle-rotation-variation", "vector3d"}, {"particle-angular-velocity", "vector3d"},
       {"particle-casts-shadow", "bool"}, {"particle-receives-shadow", "bool"},
       {"particle-casts-reflections", "bool"}});
}

static LogicalResult verifyParticleAffectors(BindOp affectors, const Records &records,
                                             const EnumLayouts &enums) {
  return verifySceneRecords(
      affectors, affectors.getInput().getType(), records, enums, "3D particle affectors",
      {{"affector-kind", "string"}},
      {{"position", "point3d"}, {"position-variation", "vector3d"}, {"direction", "vector3d"},
       {"duration", "float"}, {"duration-variation", "float"}, {"start-age", "float"},
       {"hide-at-end", "bool"}, {"radius", "float"}, {"outer-radius", "float"}, {"strength", "float"},
       {"angular-speed", "float"}, {"seed", "int"}, {"layers", "int"}, {"enabled", "bool"}});
}

static LogicalResult verifyFogVolumes(BindOp volumes, const Records &records,
                                      const EnumLayouts &enums) {
  return verifySceneRecords(
      volumes, volumes.getInput().getType(), records, enums, "3D fog volumes",
      {{"position", "point3d"}, {"size", "vector3d"}, {"density", "float"}, {"color", "color"}},
      {{"fog-shape", "string"}, {"visible", "bool"}, {"layers", "int"},
       {"yaw", "float"}, {"pitch", "float"}, {"roll", "float"},
       {"fog-height-enabled", "bool"}, {"fog-height-start", "float"}, {"fog-height-end", "float"},
       {"fog-noise-amount", "float"}, {"fog-noise-scale", "float"}, {"fog-noise-offset", "vector3d"}});
}

static LogicalResult verifySceneReflections(BindOp probes, const Records &records,
                                            const EnumLayouts &enums) {
  return verifySceneRecords(
      probes, probes.getInput().getType(), records, enums, "3D reflection probes",
      {{"position", "point3d"}, {"box-size", "vector3d"}},
      {{"source", "string"}, {"capture-size", "int"}, {"capture-revision", "int"}, {"refresh-mode", "string"},
       {"time-slicing", "string"},
       {"box-offset", "vector3d"}, {"visible", "bool"}, {"yaw", "float"},
       {"intensity", "float"}, {"parallax-correction", "bool"}, {"layers", "int"}});
}

static LogicalResult verifySceneCaptures(BindOp captures, const Records &records,
                                          const EnumLayouts &enums) {
  auto type = captures.getInput().getType();
  if (failed(verifySceneRecords(
          captures, type, records, enums, "3D scene captures", {{"name", "string"}},
          {{"camera", "string"}, {"layers", "int"}, {"scale", "float"},
           {"background", "color"}, {"enabled", "bool"}, {"color-source", "string"},
           {"color-format", "string"},
           {"depth-source", "string"}, {"depth-clear", "float"}, {"stencil-source", "string"},
           {"stencil-clear", "int"}, {"renderables", "string"}})))
    return failure();
  if (auto list = dyn_cast<RecordListType>(type)) {
    if (auto material = recordFieldType(records, enums, list.getName(), "override-material")) {
      if (records.find(*material) == records.end() || enums.contains(*material))
        return captures.emitOpError("3D scene captures requires a record field named override-material");
      if (failed(verifySceneMaterial(captures, *material, records, enums)))
        return failure();
    }
    if (auto state = recordFieldType(records, enums, list.getName(), "pipeline-state")) {
      if (records.find(*state) == records.end() || enums.contains(*state))
        return captures.emitOpError("3D scene captures requires a record field named pipeline-state");
      for (const auto &[field, expected] :
           std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
               {"enabled", "bool"}, {"depth-test", "bool"}, {"depth-write", "bool"},
               {"depth-compare", "string"}, {"cull-mode", "string"}, {"blend-mode", "string"},
               {"color-write", "bool"}, {"polygon-mode", "string"}})
        if (auto actual = recordFieldType(records, enums, *state, field);
            actual && *actual != expected)
          return captures.emitOpError() << "3D capture pipeline state requires a " << expected
                                        << " field named " << field;
      if (auto stencil = recordFieldType(records, enums, *state, "stencil")) {
        if (records.find(*stencil) == records.end() || enums.contains(*stencil))
          return captures.emitOpError("3D capture pipeline state requires a record field named stencil");
        for (const auto &[field, expected] :
             std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
                 {"enabled", "bool"}, {"reference", "int"}, {"read-mask", "int"},
                 {"write-mask", "int"}, {"compare", "string"}, {"fail", "string"},
                 {"depth-fail", "string"}, {"pass", "string"}})
          if (auto actual = recordFieldType(records, enums, *stencil, field);
              actual && *actual != expected)
            return captures.emitOpError() << "3D capture stencil state requires a " << expected
                                          << " field named " << field;
      }
      if (auto blend = recordFieldType(records, enums, *state, "target-blend")) {
        if (records.find(*blend) == records.end() || enums.contains(*blend))
          return captures.emitOpError("3D capture pipeline state requires a record field named target-blend");
        for (const auto &[field, expected] :
             std::initializer_list<std::pair<llvm::StringRef, llvm::StringRef>>{
                 {"enabled", "bool"}, {"blend-enabled", "bool"}, {"write-mask", "int"},
                 {"constant", "color"}, {"source-color", "string"}, {"destination-color", "string"},
                 {"source-alpha", "string"}, {"destination-alpha", "string"},
                 {"color-operation", "string"}, {"alpha-operation", "string"}})
          if (auto actual = recordFieldType(records, enums, *blend, field);
              actual && *actual != expected)
            return captures.emitOpError() << "3D capture target-blend state requires a "
                                          << expected << " field named " << field;
      }
      for (llvm::StringRef region : {"viewport", "scissor"})
        if (auto rect = recordFieldType(records, enums, *state, region)) {
          if (records.find(*rect) == records.end() || enums.contains(*rect))
            return captures.emitOpError() << "3D capture pipeline state requires a record field named " << region;
          for (llvm::StringRef field : {"x", "y", "width", "height"})
            if (recordFieldType(records, enums, *rect, field) != "float")
              return captures.emitOpError() << "3D capture " << region << " requires a float field named " << field;
          if (auto value = recordFieldType(records, enums, *rect, "enabled"); value && *value != "bool")
            return captures.emitOpError() << "3D capture " << region << " requires a bool field named enabled";
        }
    }
  }
  return success();
}

static LogicalResult verifySceneEffects(BindOp effects, const Records &records,
                                        const EnumLayouts &enums) {
  auto type = effects.getInput().getType();
  if (failed(verifySceneRecords(
      effects, effects.getInput().getType(), records, enums, "3D effect passes",
      {{"shader", "string"}},
      {{"depth", "bool"}, {"strength", "float"}, {"scale", "float"}, {"parameters", "vector3d"},
       {"input", "string"}, {"output", "string"}, {"history", "bool"}, {"output-format", "string"}})))
    return failure();
  if (auto model = dyn_cast<RecordListType>(type))
    if (auto buffers = recordFieldType(records, enums, model.getName(), "buffers")) {
      if (!buffers->consume_front("[") || !buffers->consume_back("]") ||
          (recordFieldType(records, enums, *buffers, "name") != "string" &&
           recordFieldType(records, enums, *buffers, "texture") != "string" &&
           recordFieldType(records, enums, *buffers, "item") != "string" &&
           recordFieldType(records, enums, *buffers, "capture") != "string"))
        return effects.emitOpError("3D effect pass buffers requires a record list with a string field named name, texture, item or capture");
      for (llvm::StringRef name : {"previous-frame", "enabled"})
        if (auto type = recordFieldType(records, enums, *buffers, name); type && *type != "bool")
          return effects.emitOpError("3D effect pass buffer requires a bool field named ") << name;
      for (llvm::StringRef name : {"name", "texture", "item", "capture", "attachment", "filter", "wrap-u", "wrap-v", "color-space"})
        if (auto type = recordFieldType(records, enums, *buffers, name); type && *type != "string")
          return effects.emitOpError("3D effect pass buffer requires a string field named ") << name;
    }
  return success();
}

static LogicalResult verifyRecords(ModuleOp module, const Records &records,
                                   const EnumLayouts &enums,
                                   ComboValueTypes &comboValues) {
  auto verified = module.walk([&](Operation *op) -> WalkResult {
    if (failed(verifyEnumOperations(op, enums)))
      return WalkResult::interrupt();
    if (auto picking = dyn_cast<GeometryFunctionOp>(op);
        picking && (picking.getValue() == "pick-3d" || picking.getValue() == "pick-camera-3d" ||
                    picking.getValue() == "project-camera-3d" ||
                    picking.getValue() == "unproject-camera-3d" ||
                    picking.getValue() == "map-position-3d" || picking.getValue() == "map-direction-3d" ||
                    picking.getValue() == "hit-3d" || picking.getValue() == "hit-camera-3d" ||
                    picking.getValue() == "ray-hit-3d" ||
                    picking.getValue() == "hit-instance-3d" || picking.getValue() == "hit-instance-camera-3d" ||
                    picking.getValue() == "ray-hit-instance-3d" ||
                    picking.getValue() == "hit-row-3d" || picking.getValue() == "hit-row-camera-3d" ||
                    picking.getValue() == "ray-hit-row-3d" || picking.getValue() == "hit-subset-3d" ||
                    picking.getValue() == "hit-subset-camera-3d" || picking.getValue() == "ray-hit-subset-3d" ||
                    picking.getValue() == "pick-in-rect-3d" || picking.getValue() == "pick-in-rect-camera-3d" ||
                    picking.getValue() == "closest-point-3d" || picking.getValue() == "closest-point-on-3d" ||
                    picking.getValue() == "sphere-cast-3d" || picking.getValue() == "sphere-overlap-3d" ||
                    picking.getValue() == "sphere-slide-3d" || picking.getValue() == "sphere-walk-3d" ||
                    picking.getValue() == "hit-all-3d" ||
                    picking.getValue() == "hit-all-camera-3d" ||
                    picking.getValue() == "ray-hit-all-3d") &&
        !picking.getInputs().empty() &&
        failed(verifySceneModel(op, picking.getInputs().front().getType(), records, enums)))
      return WalkResult::interrupt();
    if (auto scene = dyn_cast<ElementOp>(op); scene && scene.getKind() == "SceneView3D")
      for (auto binding : scene.getBody().front().getOps<BindOp>())
        if ((binding.getName() == "model" &&
             failed(verifySceneModel(binding, binding.getInput().getType(), records, enums))) ||
            (binding.getName() == "lights" && failed(verifySceneLights(binding, records, enums))) ||
            (binding.getName() == "particle-emitters" && failed(verifyParticleEmitters(binding, records, enums))) ||
            (binding.getName() == "particle-affectors" && failed(verifyParticleAffectors(binding, records, enums))) ||
            (binding.getName() == "fog-volumes" && failed(verifyFogVolumes(binding, records, enums))) ||
            (binding.getName() == "scene-captures" && failed(verifySceneCaptures(binding, records, enums))) ||
            (binding.getName() == "scene-effects" && failed(verifySceneEffects(binding, records, enums))) ||
            (binding.getName() == "reflection-probes" && failed(verifySceneReflections(binding, records, enums))))
          return WalkResult::interrupt();
    for (auto type : op->getResultTypes()) {
      llvm::StringRef name;
      if (auto record = dyn_cast<RecordType>(type))
        name = record.getName();
      else if (auto list = dyn_cast<RecordListType>(type))
        name = list.getName();
      else
        continue;
      if (!records.contains(name)) {
        op->emitOpError("unknown record type ") << name;
        return WalkResult::interrupt();
      }
    }
    if (auto element = dyn_cast<ElementOp>(op);
        element && (failed(verifyViewRecordField(element, records, enums, "row-height-field", "length")) ||
                    failed(verifyViewRecordField(element, records, enums, "section-field", "string")) ||
                    (isVirtualView(element.getKind()) &&
                     failed(verifyViewRecordField(element, records, enums, "row-visible-field", "bool"))) ||
                    (isRowKeyView(element.getKind()) &&
                     failed(verifyViewRecordField(element, records, enums, "row-key-field", "int"))) ||
                    (isSelectionList(element.getKind()) &&
                     failed(verifyViewRecordField(element, records, enums, "typeahead-field", "string"))) ||
                    ((element.getKind() == "TreeView" || element.getKind() == "StandardTableView") &&
                     failed(verifyViewRecordField(element, records, enums, "filter-field", "string")))))
      return WalkResult::interrupt();
    if (auto element = dyn_cast<ElementOp>(op);
        element && element.getKind() == "StandardTableView" &&
        failed(verifyStandardTable(element, records, enums)))
      return WalkResult::interrupt();
    if (auto element = dyn_cast<ElementOp>(op);
        element && (element.getKind() == "ComboBox" || element.getKind() == "Tumbler" || element.getKind() == "PathView") &&
        failed(verifyComboModel(element, records, enums, comboValues)))
      return WalkResult::interrupt();
    if (auto element = dyn_cast<ElementOp>(op);
        element && element.getKind() == "StandardListView") {
      BindOp model;
      BindOp role;
      for (auto bind : element.getBody().front().getOps<BindOp>()) {
        if (bind.getName() == "model")
          model = bind;
        else if (bind.getName() == "text-role")
          role = bind;
      }
      auto record = model ? dyn_cast<RecordListType>(model.getInput().getType()) : RecordListType();
      if (role && !record) {
        role.emitOpError("StandardListView text-role requires a record model");
        return WalkResult::interrupt();
      }
      if (record) {
        std::string field = "text";
        if (role) {
          auto literal = role.getInput().getDefiningOp<LiteralOp>();
          if (!literal) {
            role.emitOpError("StandardListView text-role requires a constant string");
            return WalkResult::interrupt();
          }
          auto parsed = llvm::json::parse(literal.getValue());
          if (!parsed) {
            llvm::consumeError(parsed.takeError());
            return WalkResult::interrupt();
          }
          auto label = parsed->getAsString();
          if (!label)
            return WalkResult::interrupt();
          field = label->str();
        }
        if (recordFieldType(records, enums, record.getName(), field) != "string") {
          model.emitOpError("StandardListView model requires a string field named ") << field;
          return WalkResult::interrupt();
        }
      }
    } else if (auto parser = dyn_cast<ClipboardPasteOp>(op)) {
      auto result = clipboardResult(parser);
      auto link = clipboardTarget(parser);
      if (!result || !link)
        return WalkResult::interrupt();
      auto record = cast<RecordType>(result.getInput().getType());
      auto schema = records.find(record.getName());
      if (schema == records.end() || schema->second.size() != 2 ||
          !llvm::any_of(schema->second, [](const auto &field) {
            return field.first == "accepted" && field.second == "bool";
          }) ||
          !llvm::any_of(schema->second, [&](const auto &field) {
            return field.first == "value" &&
                matchesDeclaredType(link.getInput().getType(), field.second);
          })) {
        parser.emitOpError("requires a result record with accepted: bool and the linked value type");
        return WalkResult::interrupt();
      }
    } else if (auto callback = dyn_cast<CallbackOp>(op)) {
      auto parsed = llvm::json::parse(callback.getParameters());
      if (!parsed) {
        llvm::consumeError(parsed.takeError());
        return WalkResult::interrupt();
      }
      if (auto *parameters = parsed->getAsArray()) {
        for (auto &value : *parameters) {
          auto *parameter = value.getAsObject();
          auto type = parameter ? parameter->getString("ty") : std::nullopt;
          if (!type)
            continue;
          auto name = *type;
          if (recordScalarType(name) || name == "[int]" || name == "[string]" ||
              name == "[Date]" || name == "[hit3d]" || name == "[Hit3D]" ||
              name == "TouchPoints" || name == "percent")
            continue;
          const bool wellFormed =
              !name.consume_front("[") || name.consume_back("]");
          if (!wellFormed || !records.contains(name)) {
            op->emitOpError("unknown callback parameter type ") << *type;
            return WalkResult::interrupt();
          }
        }
      }
    } else if (auto make = dyn_cast<MakeRecordOp>(op)) {
      const auto &fields =
          records.find(cast<RecordType>(make.getResult().getType()).getName())
              ->second;
      if (fields.size() != make.getFields().size() ||
          !llvm::all_of(llvm::zip(fields, make.getFields()), [](auto pair) {
            return matchesDeclaredType(std::get<1>(pair).getType(),
                                       std::get<0>(pair).second);
          })) {
        op->emitOpError("record operands must match its field types");
        return WalkResult::interrupt();
      }
    } else if (auto update = dyn_cast<UpdateRecordOp>(op)) {
      auto type = cast<RecordType>(update.getResult().getType());
      if (update.getBase().getType() != type ||
          update.getFields().size() != update.getOverrides().size()) {
        op->emitOpError("record update requires a base of its result type and one name per override");
        return WalkResult::interrupt();
      }
      const auto &schema = records.find(type.getName())->second;
      llvm::StringSet<> seen;
      for (auto [field, value] : llvm::zip(update.getFields(), update.getOverrides())) {
        auto name = cast<StringAttr>(field).getValue();
        if (!seen.insert(name).second ||
            !llvm::any_of(schema, [&](const auto &entry) {
              return entry.first == name &&
                     matchesDeclaredType(value.getType(), entry.second);
            })) {
          op->emitOpError("unknown, duplicate or mismatched record update field: ") << name;
          return WalkResult::interrupt();
        }
      }
    } else if (isa<ModelFilterOp, ModelLookupOp, ModelSortByOp>(op)) {
      auto name = cast<RecordListType>(op->getResult(0).getType()).getName();
      auto path = op->getAttrOfType<StringAttr>("value").getValue();
      auto type = recordFieldType(records, enums, name, path);
      if (type != "string" && !(isa<ModelSortByOp>(op) && type == "int")) {
        op->emitOpError("invalid record field for model projection: ") << path;
        return WalkResult::interrupt();
      }
    } else if (auto field = dyn_cast<GetFieldOp>(op)) {
      const auto name = cast<RecordType>(field.getInput().getType()).getName();
      auto schema = records.find(name);
      if (schema == records.end() ||
          !llvm::any_of(schema->second, [&](const auto &entry) {
            return entry.first == field.getValue() &&
                   matchesDeclaredType(field.getResult().getType(),
                                       entry.second);
          })) {
        op->emitOpError("unknown record field or mismatched field type: ")
            << field.getValue();
        return WalkResult::interrupt();
      }
    } else if (auto array = dyn_cast<RecordArrayOp>(op)) {
      auto name = cast<RecordListType>(array.getResult().getType()).getName();
      if (!llvm::all_of(array.getItems(), [&](Value item) {
            return cast<RecordType>(item.getType()).getName() == name;
          })) {
        op->emitOpError("array items must match the record list type");
        return WalkResult::interrupt();
      }
    }
    return WalkResult::advance();
  });
  return failure(verified.wasInterrupted());
}

LogicalResult verifyModule(ModuleOp module) {
  Components components;
  Records records;
  EnumLayouts enums;
  bool hasTranslationCatalog = false;
  for (Operation &op : module.getBody()->getOperations()) {
    if (isa<FontOp>(op))
      continue;
    if (isa<TranslationCatalogOp>(op)) {
      if (hasTranslationCatalog)
        return op.emitOpError("only one translation catalog can be declared");
      hasTranslationCatalog = true;
      continue;
    }
    if (auto record = dyn_cast<RecordDeclOp>(op)) {
      if (failed(indexRecord(record, records, enums)))
        return failure();
      continue;
    }
    auto component = dyn_cast<ComponentOp>(op);
    if (!component)
      return op.emitOpError(
          "expected xr.component, xr.record_decl, xr.font or xr.translation_catalog at module scope");
    if (!components.try_emplace(component.getSymName(), component).second)
      return op.emitOpError("duplicate component ") << component.getSymName();
  }
  ComboValueTypes comboValues;
  if (failed(verifyRecordLayouts(module, records, enums)) ||
      failed(verifyRecords(module, records, enums, comboValues)))
    return failure();
  for (auto &entry : components)
    if (failed(verifyReferences(entry.getValue(), components, comboValues)))
      return failure();
  llvm::DenseSet<Operation *> active, done;
  for (auto &entry : components)
    if (failed(verifyInstanceTree(entry.getValue(), components, active, done)))
      return failure();
  return success();
}
} // namespace xrack::dialect
