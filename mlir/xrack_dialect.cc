#include "mlir/xrack_dialect.h"

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

static bool isExpression(Operation &op) {
  return isa<LiteralOp, ReadOp, StackReadOp, ParamOp, UnaryOp, CastOp,
             StringifyOp, BinaryOp, SelectOp, SolidBrushOp, GradientStopOp,
             LinearGradientOp, RadialGradientOp, ImageUrlOp, ArrayOp, CallOp,
             ToRatioOp, MakeRecordOp, DefaultRecordOp, RecordArrayOp,
             GetFieldOp>(op);
}

static bool isVirtualView(llvm::StringRef kind) {
  return kind == "ListView" || kind == "TableView" || kind == "TreeView";
}

static bool isModalOverlay(llvm::StringRef kind) {
  return kind == "Dialog" || kind == "Drawer";
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

static bool isTypedRowEditor(llvm::StringRef kind) {
  return kind == "SpinBox" || kind == "Slider" || kind == "Dial" ||
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
      auto field = binding.getInput().getDefiningOp<GetFieldOp>();
      if (!field || !isa<IntType>(binding.getInput().getType()))
        return false;
      auto param = field.getInput().getDefiningOp<ParamOp>();
      auto record = dyn_cast<RecordType>(field.getInput().getType());
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
  return isVirtualView(kind) || kind == "Tooltip" || isModalOverlay(kind) ||
         kind == "Menu" || kind == "MenuBar";
}

LogicalResult ComponentOp::verify() {
  if (getExported() != "true" && getExported() != "false")
    return emitOpError("exported must be true or false");
  unsigned roots = 0;
  for (Operation &child : getBody().front()) {
    if (isa<ElementOp>(child))
      ++roots;
    else if (!isa<PropertyOp, CallbackOp, EndOp>(child) && !isExpression(child))
      return child.emitOpError("is not valid in a component body");
  }
  if (roots != 1)
    return emitOpError("requires exactly one root xr.element");
  return success();
}

LogicalResult ElementOp::verify() {
  const bool root = isa<ComponentOp>((*this)->getParentOp());
  if (!root && !isa<ElementOp>((*this)->getParentOp()))
    return emitOpError("requires an xr.component or xr.element parent");
  auto kind = getKind();
  auto parent = dyn_cast<ElementOp>((*this)->getParentOp());
  auto parentKind = parent ? parent.getKind() : llvm::StringRef{};
  auto &body = getBody().front();
  if (kind == "Action") {
    if (!parent || !isa<ComponentOp>(parent->getParentOp()) || getId().empty())
      return emitOpError(
          "Action requires a named declaration at the component root");
    llvm::StringSet<> properties;
    bool handled = false;
    for (Operation &child : body) {
      if (auto bind = dyn_cast<BindOp>(child)) {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if (!((name == "text" || name == "shortcut") &&
              isa<StringType>(type)) &&
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
      } else if (!isa<EndOp>(child) && !isExpression(child)) {
        return child.emitOpError(
            "Action only accepts property bindings and activated()");
      }
    }
    return success();
  }
  auto refs = body.getOps<ActionRefOp>();
  const bool linkedAction = !refs.empty();
  if (linkedAction && ((kind != "Button" && kind != "MenuItem") ||
                       !llvm::hasSingleElement(refs)))
    return emitOpError("requires one Action reference on a Button or MenuItem");
  if (linkedAction)
    for (auto ancestor = *this; ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>())
      if (!ancestor.getBody().empty() &&
          !ancestor.getBody().front().getOps<RepeatOp>().empty())
        return emitOpError(
            "Action presentations require a stable element path");
  if (kind == "Tooltip" && (root || parentKind == "Tooltip"))
    return emitOpError("Tooltip requires a visual owner");
  if (forbiddenInTooltip(kind))
    for (auto ancestor = parent; ancestor;
         ancestor = ancestor->getParentOfType<ElementOp>())
      if (ancestor.getKind() == "Tooltip")
        return emitOpError("Tooltip content cannot be a popup or virtual view");
  if ((kind == "Menu" && parentKind != "MenuBar" && parentKind != "Menu" &&
       parentKind != "ContextMenuArea") ||
      ((kind == "MenuItem" || kind == "MenuSeparator") && parentKind != "Menu"))
    return emitOpError("requires a menu container");
  const bool table = kind == "TableView";
  const bool tree = kind == "TreeView";
  const bool interval = kind == "RangeSlider";
  const bool textCell = kind == "TableCell" || kind == "TreeCell";
  if (kind == "TreeRow" && (parentKind != "TreeView" || !validTreeRoles(*this)))
    return emitOpError("TreeRow requires integer key and parent-key fields of "
                       "a repeated record model");
  if ((kind == "TableRow" || kind == "TableColumn") &&
      parentKind != "TableView")
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
  if ((kind == "RadioButton" &&
       (!parent || parent.getKind() != "RadioGroup")) ||
      (kind == "Tab" && (!parent || parent.getKind() != "TabWidget")) ||
      (kind == "SwipePage" && (!parent || parent.getKind() != "SwipeView")) ||
      (kind == "StackPage" && (!parent || parent.getKind() != "StackView")))
    return emitOpError("requires a matching selection container");
  llvm::StringRef childKind = kind == "RadioGroup"  ? "RadioButton"
                              : kind == "TabWidget" ? "Tab"
                              : kind == "SwipeView" ? "SwipePage"
                              : kind == "StackView" ? "StackPage"
                                                    : "";
  unsigned options = 0;
  bool repeated = false;
  unsigned listChildren = 0;
  unsigned contextMenus = 0;
  unsigned tooltips = 0;
  unsigned tooltipChildren = 0;
  bool tooltipText = false;
  unsigned cellTextBindings = 0;
  unsigned columns = 0;
  std::optional<unsigned> cells;
  llvm::StringSet<> intervalEndpoints;
  ReadOp intervalTarget;
  for (Operation &child : body) {
    if (auto element = dyn_cast<ElementOp>(child);
        element && element.getKind() == "Action")
      continue;
    if (linkedAction && isa<HandlerOp, ControlLinkOp>(child))
      return child.emitOpError(
          "activation and checked state belong to the linked Action");
    if (isa<ElementOp, InstanceOp>(child)) {
      if (kind == "SplitView" && !child.getRegion(0).empty() &&
          !child.getRegion(0).front().getOps<RepeatOp>().empty())
        return child.emitOpError(
            "SplitView panes require stable child positions");
      auto element = dyn_cast<ElementOp>(child);
      auto childType = element ? element.getKind() : llvm::StringRef{};
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
          ++columns;
        } else if (childType == "TableRow" && !cells &&
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
          return child.emitOpError("TableView requires static TableColumn "
                                   "children and one repeated TableRow");
        }
      }
      if (kind == "TableRow" && !child.getRegion(0).empty() &&
          !child.getRegion(0).front().getOps<RepeatOp>().empty())
        return child.emitOpError("table cells cannot be repeated");
      if (kind == "MenuItem" || kind == "MenuSeparator" ||
          (kind == "MenuBar" && childType != "Menu") ||
          (kind == "Menu" && childType != "Menu" && childType != "MenuItem" &&
           childType != "MenuSeparator"))
        return child.emitOpError("invalid menu child");
      if (childType == "Menu")
        ++contextMenus;
      if ((interval || kind == "Dial" || kind == "PageIndicator") &&
          childType != "Tooltip")
        return child.emitOpError() << kind << " only accepts Tooltip children";
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
    if (kind == "TextEdit" || kind == "TextArea")
      if (auto bind = dyn_cast<BindOp>(child);
          bind && bind.getName() == "wrap" &&
          !isa<TextWrapType>(bind.getInput().getType()))
        return child.emitOpError("wrap requires a TextWrap value");
    if (auto bind = dyn_cast<BindOp>(child)) {
      auto name = bind.getName();
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
      if (kind == "StackView" && name == "initial-page") {
        if (!isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("initial-page requires a string route");
        if (failed(verifyStackRoute(&child, *this, bind.getInput())))
          return failure();
      }
      if (kind == "StackView" && name == "transition-duration" &&
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
      if ((kind == "SwipeView" || kind == "SwipePage") &&
          (((name == "enabled" || name == "visible" || name == "interactive") &&
            !isa<BoolType>(bind.getInput().getType())) ||
           (name == "current-index" &&
            !isa<IntType>(bind.getInput().getType())) ||
           (name == "orientation" &&
            !isa<OrientationType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect swipe page or view property type");
      if (name == "vertical-scroll-bar-policy" ||
          name == "horizontal-scroll-bar-policy") {
        const bool scrollable =
            name == "vertical-scroll-bar-policy"
                ? isVirtualView(kind) || kind == "ScrollView"
                : table || kind == "ScrollView";
        if (!scrollable || !isa<ScrollBarPolicyType>(bind.getInput().getType()))
          return child.emitOpError(
              "requires a ScrollBarPolicy on a scrollable axis");
      }
      if ((name == "content-width" || name == "content-height") &&
          (kind != "ScrollView" ||
           !isa<LengthType, PercentType>(bind.getInput().getType())))
        return child.emitOpError("requires a ScrollView content length");
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
            ((name == "font-size" || name == "padding") &&
             !isa<LengthType>(type)) ||
            (name == "color" && !isa<ColorType>(type)) ||
            (name == "background" && !isa<BrushType>(type)))
          return child.emitOpError("incorrect Tooltip property type");
        tooltipText |= name == "text";
      }
      if (kind == "SplitView") {
        auto name = bind.getName();
        auto type = bind.getInput().getType();
        if ((name == "orientation" && !isa<OrientationType>(type)) ||
            (name == "handle-size" && !isa<LengthType>(type)) ||
            (name == "handle-color" && !isa<ColorType>(type)) ||
            (name == "enabled" && !isa<BoolType>(type)))
          return child.emitOpError("incorrect SplitView property type");
      }
      if (table && bind.getName() == "selection-target" &&
          !isa<SelectionTargetType>(bind.getInput().getType()))
        return child.emitOpError(
            "selection-target requires a SelectionTarget value");
      if (textCell && bind.getName() == "text") {
        if (!isa<StringType>(bind.getInput().getType()))
          return child.emitOpError("cell text requires a string value");
        ++cellTextBindings;
      }
      if (((table && (bind.getName() == "resizable-columns" ||
                      bind.getName() == "multi-select")) ||
           (kind == "TableColumn" &&
            (bind.getName() == "resizable" || bind.getName() == "sortable"))) &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError(
            "table interaction flags require a bool value");
      if (tree && bind.getName() == "multi-select" &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("TreeView multi-select requires a bool value");
      if (kind == "DialogButton" && bind.getName() == "role" &&
          !isa<DialogRoleType>(bind.getInput().getType()))
        return child.emitOpError("role requires a DialogRole value");
      if (isModalOverlay(kind) &&
          (bind.getName() == "open" || bind.getName() == "close-on-escape" ||
           bind.getName() == "close-on-outside") &&
          !isa<BoolType>(bind.getInput().getType()))
        return child.emitOpError("dialog state requires a bool value");
      if (kind == "Drawer" &&
          ((name == "edge" &&
            !isa<DrawerEdgeType>(bind.getInput().getType())) ||
           (name == "drag-margin" &&
            !isa<LengthType>(bind.getInput().getType()))))
        return child.emitOpError("incorrect Drawer property type");
    }
    if (textCell) {
      if (!isa<BindOp, RowLinkOp, AnimateOp, ShowIfOp, EndOp>(child) &&
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
    if (isVirtualView(kind) && isa<RepeatOp>(child))
      return child.emitOpError("virtual views cannot be repeated templates");
    if (kind == "SwipeView" && isa<RepeatOp>(child))
      return child.emitOpError("SwipeView cannot be a repeated template");
    if (kind == "StackView" && isa<RepeatOp>(child))
      return child.emitOpError("StackView cannot be a repeated template");
    if (kind == "ListView" && isa<ElementOp, InstanceOp>(child)) {
      auto &body = child.getRegion(0);
      if (body.empty() || body.front().getOps<RepeatOp>().empty())
        return child.emitOpError("ListView requires a repeated element");
      ++listChildren;
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
    if (((kind == "Tab" || kind == "SwipePage" || kind == "StackPage") &&
         isa<ShowIfOp, RepeatOp>(child)) ||
        (kind == "RadioButton" && isa<ShowIfOp>(child)))
      return child.emitOpError("selection options must be static");
    if (kind == "RadioButton" && isa<ElementOp, InstanceOp>(child))
      return child.emitOpError("RadioButton cannot contain child elements");
    if (isa<PropertyOp>(child) && (root || getKey().value_or(getId()).empty()))
      return child.emitOpError("requires a keyed non-root element");
    if (!isa<BindOp, ActionRefOp, AnimateOp, ShowIfOp, RepeatOp, ControlLinkOp,
             RowLinkOp, HandlerOp, ElementOp, InstanceOp, PropertyOp, EndOp>(
            child) &&
        !isExpression(child))
      return child.emitOpError("is not valid in an element body");
  }
  if (repeated && options != 1)
    return emitOpError("a repeated RadioButton must be the only option");
  if (kind == "ListView" && listChildren != 1)
    return emitOpError("ListView requires exactly one for element");
  if (tree && listChildren != 1)
    return emitOpError("TreeView requires exactly one repeated TreeRow");
  if (kind == "StackView" && options == 0)
    return emitOpError("StackView requires at least one named StackPage");
  if (table && (columns == 0 || cells != columns))
    return emitOpError("TableView requires columns and one cell per column in "
                       "the repeated TableRow");
  if (kind == "ContextMenuArea" && contextMenus != 1)
    return emitOpError("ContextMenuArea requires one direct Menu");
  if (tooltips > 1)
    return emitOpError("an element can contain at most one Tooltip");
  if (tooltipChildren > 1 || (tooltipChildren == 1 && tooltipText))
    return emitOpError("Tooltip requires text or a single content child");
  if (cellTextBindings > 1)
    return emitOpError("requires a single cell text binding");
  return success();
}

LogicalResult InstanceOp::verify() {
  if (getKey().empty())
    return emitOpError("requires a nonempty instance key");
  llvm::StringMap<Operation *> bindings;
  bool repeated = false;
  for (Operation &child : getBody().front()) {
    if (isa<BindOp, LayoutBindOp>(child)) {
      auto name = child.getAttrOfType<StringAttr>("name").getValue();
      if (!bindings.try_emplace(name, &child).second)
        return child.emitOpError("duplicate instance binding ") << name;
    } else if (isa<RepeatOp>(child)) {
      auto parent = cast<ElementOp>((*this)->getParentOp());
      if (repeated || parent.getKind() != "ListView" || !getId().empty())
        return child.emitOpError(
            "repeated instances require an anonymous ListView template");
      repeated = true;
    } else if (!isa<EndOp, ShowIfOp>(child) && !isExpression(child)) {
      return child.emitOpError("is not valid in an instance body");
    }
  }
  return success();
}

LogicalResult AnimateOp::verify() {
  auto duration = getInput().getDefiningOp<LiteralOp>();
  int64_t milliseconds;
  if (!duration || duration.getValue().getAsInteger(10, milliseconds) ||
      milliseconds < 0 || milliseconds > 60000)
    return emitOpError("requires a constant duration between 0 and 60000ms");
  return success();
}

LogicalResult ControlLinkOp::verify() {
  auto element = cast<ElementOp>((*this)->getParentOp());
  auto kind = element.getKind();
  auto type = getInput().getType();
  const bool text = (kind == "TextInput" || kind == "LineEdit" ||
                     kind == "TextEdit" || kind == "TextArea") &&
                    getName() == "text" && isa<StringType>(type);
  const bool checked = (kind == "CheckBox" || kind == "Switch" ||
                        kind == "MenuItem" || kind == "Action") &&
                       getName() == "checked" && isa<BoolType>(type);
  const bool opened =
      isModalOverlay(kind) && getName() == "open" && isa<BoolType>(type);
  const bool range =
      (((kind == "Slider" || kind == "Dial") && getName() == "value") ||
       (kind == "RangeSlider" &&
        (getName() == "lower-value" || getName() == "upper-value"))) &&
      isa<FloatType>(type);
  const bool spin =
      kind == "SpinBox" && getName() == "value" && isa<IntType>(type);
  const bool valueChoice = kind == "ComboBox" || kind == "RadioGroup";
  const bool choice =
      ((valueChoice || kind == "TabWidget" || kind == "SwipeView" ||
        kind == "PageIndicator" || kind == "ListView") &&
       getName() == "current-index" && isa<IntType>(type)) ||
      (valueChoice && getName() == "current-value" && isa<StringType>(type)) ||
      (kind == "TreeView" && getName() == "current-key" &&
       isa<IntType>(type)) ||
      (kind == "TableView" && getName() == "current-row" && isa<IntType>(type));
  if ((!text && !checked && !range && !spin && !choice && !opened) ||
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

LogicalResult StackReadOp::verify() {
  auto name = getValue();
  auto type = getResult().getType();
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
  if (!table) {
    row = enclosingTreeRow(cell);
    if (!row || !hasStaticEditorPath(cell, row))
      return {};
  }
  auto field = link.getInput().getDefiningOp<GetFieldOp>();
  Value value = link.getInput();
  if (field)
    value = field.getInput();
  auto item = value.getDefiningOp<ParamOp>();
  const auto kind = cell.getKind();
  const auto type = link.getInput().getType();
  const bool text = kind == (table ? "TableCell" : "TreeCell") &&
                    link.getName() == "text" && isa<StringType>(type);
  const bool spin =
      kind == "SpinBox" && link.getName() == "value" && isa<IntType>(type);
  const bool slider = (kind == "Slider" || kind == "Dial") &&
                      link.getName() == "value" && isa<FloatType>(type);
  const bool toggle = (kind == "CheckBox" || kind == "Switch") &&
                      link.getName() == "checked" && isa<BoolType>(type);
  if ((!text && !spin && !slider && !toggle) || row.getBody().empty() || !item)
    return {};
  auto repeats = row.getBody().front().getOps<RepeatOp>();
  if (!llvm::hasSingleElement(repeats))
    return {};
  auto repeat = *repeats.begin();
  if (repeat.getItem() != item.getValue())
    return {};
  if (field) {
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
        enclosingTreeRow(cell)
            ? "requires a matching field of a writable tree model"
            : "requires the repeated string item or a matching "
              "field of a table model property");
  if (!cell.getId().empty())
    return emitOpError("row editors must be anonymous");
  for (Operation &op : cell.getBody().front()) {
    if (&op == getOperation() || isa<AnimateOp, ShowIfOp, EndOp>(op) ||
        isExpression(op))
      continue;
    if (auto binding = dyn_cast<BindOp>(op);
        binding && binding.getName() != getName())
      continue;
    return emitOpError("row editors require leaf controls with one field link");
  }
  return success();
}

LogicalResult LayoutBindOp::verify() {
  auto *context = getContext();
  auto name = getName();
  Type expected =
      llvm::StringSwitch<Type>(name)
          .Cases({"width", "height", "x", "y", "min-width", "min-height",
                  "max-width", "max-height", "preferred-width",
                  "preferred-height"},
                 LengthType::get(context))
          .Cases({"horizontal-stretch", "vertical-stretch"},
                 FloatType::get(context))
          .Case("visible", BoolType::get(context))
          .Cases({"layout-order", "row", "col", "rowspan", "colspan"},
                 IntType::get(context))
          .Case("cross-axis-self-alignment",
                CrossAxisAlignmentType::get(context))
          .Default({});
  if (!expected)
    return emitOpError("unknown instance layout property ") << name;
  const bool relative = name != "x" && name != "y" &&
                        isa<LengthType>(expected) &&
                        isa<PercentType>(getInput().getType());
  if (getInput().getType() != expected && !relative)
    return emitOpError("incorrect type for instance layout property ") << name;
  return success();
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
    if (kind == "SplitView" &&
        (!args->empty() || handler.getName() != "resized"))
      return handler.emitOpError(
          "SplitView requires resized() without parameters");
    if (kind == "Dialog" &&
        (!args->empty() ||
         (handler.getName() != "accepted" && handler.getName() != "rejected")))
      return handler.emitOpError(
          "dialog requires accepted() or rejected() without parameters");
    if (kind == "Drawer" && (!args->empty() || (handler.getName() != "opened" &&
                                                handler.getName() != "closed")))
      return handler.emitOpError(
          "Drawer requires opened() or closed() without parameters");
    if ((kind == "MenuItem" || kind == "Action") &&
        (!args->empty() || handler.getName() != "activated"))
      return handler.emitOpError("requires activated() without parameters");
    const bool multiline = kind == "TextEdit" || kind == "TextArea";
    const bool table = kind == "TableView";
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
    if (kind == "StackView") {
      auto *page = args->size() == 2 ? args->front().getAsObject() : nullptr;
      auto *depth = args->size() == 2 ? args->back().getAsObject() : nullptr;
      if (handler.getName() != "changed" || !page || !depth ||
          page->getString("ty") != "string" ||
          depth->getString("ty") != "int" || !page->getString("name") ||
          !depth->getString("name"))
        return handler.emitOpError("StackView requires changed(string, int)");
    }
    const bool scalarRange = kind == "Slider" || kind == "Dial";
    const bool indexView = kind == "TabWidget" || kind == "SwipeView" ||
                           kind == "PageIndicator" || kind == "ListView";
    if (scalarRange || kind == "SpinBox" || kind == "ComboBox" || table ||
        kind == "RadioGroup" || indexView || multiline || tree) {
      auto *arg = args->size() == 1 ? args->front().getAsObject() : nullptr;
      auto name = handler.getName();
      auto type = scalarRange ? "float"
                  : (kind == "SpinBox" || indexView || table || tree)
                      ? "int"
                      : "string";
      const bool known =
          scalarRange ? name == "changed" || name == "released"
          : (kind == "SpinBox" || multiline) ? name == "edited"
          : indexView                        ? name == "changed"
          : tree ? name == "changed" || name == "expanded" ||
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
  for (Operation &child : body.front()) {
    if (isa<ReturnOp>(child) && !isa_and_nonnull<EndOp>(child.getNextNode()))
      return child.emitOpError("must be immediately before xr.end");
    if (!isa<AssignOp, InvokeOp, NavigateOp, ReturnOp, EndOp>(child) &&
        !isExpression(child))
      return child.emitOpError("is not valid in a callback or event handler");
  }
  return success();
}

LogicalResult CallbackOp::verify() {
  if (getPure() && *getPure() != "true")
    return emitOpError("pure marker must be true");
  if (getPure())
    for (Operation &child : getBody().front())
      if (isa<AssignOp>(child))
        return child.emitOpError(
            "assignment is not allowed in a pure callback");
  return verifyCallback(*this, getBody(), getParameters());
}

LogicalResult HandlerOp::verify() {
  return verifyCallback(*this, getBody(), getParameters());
}

LogicalResult LiteralOp::verify() {
  auto type = getResult().getType();
  auto spelling = getValue();
  bool valid = false;
  if (isa<IntType>(type)) {
    int64_t value;
    valid = !spelling.getAsInteger(10, value);
  } else if (isa<FloatType, LengthType, PercentType>(type)) {
    double value;
    valid = !spelling.getAsDouble(value) && std::isfinite(value);
  } else if (isa<BoolType>(type)) {
    valid = spelling == "true" || spelling == "false";
  } else if (isa<ImageFitType>(type)) {
    valid = spelling == "fill" || spelling == "contain" ||
            spelling == "cover" || spelling == "preserve";
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
  } else if (isa<DialInputModeType>(type)) {
    valid = spelling == "circular" || spelling == "horizontal" ||
            spelling == "vertical";
  } else if (isa<ScrollBarPolicyType>(type)) {
    valid = spelling == "as-needed" || spelling == "always-on" ||
            spelling == "always-off";
  } else if (isa<TextWrapType>(type)) {
    valid = spelling == "word-wrap" || spelling == "no-wrap" ||
            spelling == "char-wrap";
  } else if (isa<DialogRoleType>(type)) {
    valid = spelling == "accept" || spelling == "reject";
  } else if (isa<DrawerEdgeType>(type)) {
    valid = spelling == "left" || spelling == "right" || spelling == "top" ||
            spelling == "bottom";
  } else if (isa<SelectionTargetType>(type)) {
    valid = spelling == "rows" || spelling == "cells" || spelling == "columns";
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
  double offset;
  if (!literal || literal.getValue().getAsDouble(offset) ||
      !std::isfinite(offset) || offset < 0.0 || offset > 1.0)
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

static bool writableFromParent(llvm::StringRef visibility) {
  return visibility == "in" || visibility == "in-out";
}

static LogicalResult verifyReference(Operation *op, const Owners &owners,
                                     const Components &components,
                                     llvm::StringRef owner,
                                     llvm::StringRef name, Type type,
                                     bool write) {
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
    if (instance && visibility == "private")
      return op->emitOpError("cannot access private property ") << name;
    if (write &&
        (instance ? !writableFromParent(visibility) : visibility == "in"))
      return op->emitOpError("cannot write read-only property ") << name;
    if (property.getInitial().getType() != type)
      return op->emitOpError("property type mismatch for ")
             << owner << "." << name;
    return success();
  }
  return op->emitOpError("unknown property ") << owner << "." << name;
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
  auto expected = declared == "[string]" ? "stringlist" : declared.lower();
  return spelling.consume_front("!xr.") && spelling == expected;
}

static LogicalResult verifyCall(Operation *op, const Owners &owners,
                                const Components &components,
                                llvm::StringRef owner, llvm::StringRef name,
                                Type result) {
  Operation *target = owners.lookup(owner);
  if (auto instance = dyn_cast_or_null<InstanceOp>(target))
    target = components.lookup(instance.getComponent());
  auto component = dyn_cast_or_null<ComponentOp>(target);
  if (!component)
    return op->emitOpError("unknown callback owner ") << owner;

  for (auto callback : component.getBody().front().getOps<CallbackOp>()) {
    if (callback.getName() != name)
      continue;
    auto enclosing = dyn_cast<CallbackOp>(op->getParentOp());
    if (!callback.getPure() && !isa<HandlerOp>(op->getParentOp()) &&
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

static LogicalResult verifyReferences(ComponentOp component,
                                      const Components &components) {
  Owners owners;
  owners.try_emplace("root", component.getOperation());
  auto indexed = component.walk([&](Operation *op) -> WalkResult {
    llvm::StringRef key;
    if (auto element = dyn_cast<ElementOp>(op)) {
      if (isa<ComponentOp>(op->getParentOp()))
        return WalkResult::advance();
      key = element.getKey().value_or(element.getId());
    } else if (auto instance = dyn_cast<InstanceOp>(op)) {
      if (!components.count(instance.getComponent())) {
        op->emitOpError("unknown component ") << instance.getComponent();
        return WalkResult::interrupt();
      }
      key = instance.getKey();
    }
    if (!key.empty() && !owners.try_emplace(key, op).second) {
      op->emitOpError("duplicate property owner ") << key;
      return WalkResult::interrupt();
    }
    return WalkResult::advance();
  });
  if (indexed.wasInterrupted())
    return failure();

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
  }

  auto stackOwner = [&](llvm::StringRef name) -> ElementOp {
    auto *target = owners.lookup(name);
    auto view = dyn_cast_or_null<ElementOp>(target);
    if (target == component.getOperation()) {
      auto roots = component.getBody().front().getOps<ElementOp>();
      if (!roots.empty())
        view = *roots.begin();
    }
    return view && view.getKind() == "StackView" ? view : ElementOp{};
  };
  auto references = component.walk([&](Operation *op) -> WalkResult {
    LogicalResult result = success();
    if (auto read = dyn_cast<ReadOp>(op))
      result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), false);
    else if (auto read = dyn_cast<StackReadOp>(op)) {
      if (!stackOwner(read.getOwner()))
        result = op->emitOpError("requires a StackView owner");
    } else if (auto action = dyn_cast<ActionRefOp>(op)) {
      auto target =
          dyn_cast_or_null<ElementOp>(owners.lookup(action.getOwner()));
      if (!target || target.getKind() != "Action")
        result =
            op->emitOpError("requires an Action owner in the same component");
    } else if (auto assign = dyn_cast<AssignOp>(op))
      result = verifyReference(
          op, owners, components, assign.getOwner().value_or("root"),
          assign.getName(), assign.getInput().getType(), true);
    else if (auto call = dyn_cast<CallOp>(op))
      result = verifyCall(op, owners, components, call.getOwner(),
                          call.getValue(), call.getResult().getType());
    else if (auto invoke = dyn_cast<InvokeOp>(op))
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
    } else if (auto link = dyn_cast<ControlLinkOp>(op)) {
      auto read = link.getInput().getDefiningOp<ReadOp>();
      if (!read)
        return WalkResult::interrupt();
      result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), true);
    } else if (auto link = dyn_cast<RowLinkOp>(op)) {
      auto read = editableRowModel(link);
      if (!read)
        return WalkResult::interrupt();
      result = verifyReference(
          op, owners, components, read.getOwner().value_or("root"),
          read.getValue(), read.getResult().getType(), true);
    } else if (auto bind = dyn_cast<BindOp>(op)) {
      if (auto instance = dyn_cast<InstanceOp>(op->getParentOp()))
        result =
            verifyReference(op, owners, components, instance.getKey(),
                            bind.getName(), bind.getInput().getType(), true);
    }
    return failed(result) ? WalkResult::interrupt() : WalkResult::advance();
  });
  return failure(references.wasInterrupted());
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
    if (!instance.getBody().front().getOps<RepeatOp>().empty()) {
      llvm::DenseSet<Operation *> visited;
      if (containsElement(child, components, visited, isVirtualView)) {
        instance.emitOpError(
            "ListView delegates cannot contain another ListView");
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
      .Cases({"int", "float", "bool", "string", "length", "color", "brush",
              "image", "ImageFit", "LayoutAlignment", "CrossAxisAlignment",
              "Orientation", "DialInputMode", "ScrollBarPolicy", "TextWrap",
              "DialogRole", "DrawerEdge", "SelectionTarget"},
             true)
      .Default(false);
}

static LogicalResult indexRecord(RecordDeclOp record, Records &records) {
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
        !type || !recordScalarType(*type))
      return record.emitOpError(
          "record fields require unique names and scalar or enum types");
    fields.emplace_back(name->str(), type->str());
  }
  if (!records.try_emplace(record.getSymName(), std::move(fields)).second)
    return record.emitOpError("duplicate record ") << record.getSymName();
  return success();
}

static LogicalResult verifyRecords(ModuleOp module, const Records &records) {
  auto verified = module.walk([&](Operation *op) -> WalkResult {
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
    if (auto callback = dyn_cast<CallbackOp>(op)) {
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
          if (recordScalarType(name) || name == "[string]" || name == "percent")
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
  for (Operation &op : module.getBody()->getOperations()) {
    if (auto record = dyn_cast<RecordDeclOp>(op)) {
      if (failed(indexRecord(record, records)))
        return failure();
      continue;
    }
    auto component = dyn_cast<ComponentOp>(op);
    if (!component)
      return op.emitOpError(
          "expected xr.component or xr.record_decl at module scope");
    if (!components.try_emplace(component.getSymName(), component).second)
      return op.emitOpError("duplicate component ") << component.getSymName();
  }
  if (failed(verifyRecords(module, records)))
    return failure();
  for (auto &entry : components)
    if (failed(verifyReferences(entry.getValue(), components)))
      return failure();
  llvm::DenseSet<Operation *> active, done;
  for (auto &entry : components)
    if (failed(verifyInstanceTree(entry.getValue(), components, active, done)))
      return failure();
  return success();
}
} // namespace xrack::dialect
