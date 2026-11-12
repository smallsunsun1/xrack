#include "mlir/xrack_passes.h"

#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/StringSet.h"
#include "llvm/ADT/StringSwitch.h"
#include <algorithm>
#include <cmath>
#include <numeric>
#include <optional>

namespace xrack::dialect {
#define GEN_PASS_DEF_XRPLANUPDATEEFFECTS
#define GEN_PASS_DEF_XRSPECIALIZEEMPTYREPEATS
#define GEN_PASS_DEF_XRPLANFIXEDTEXTUPDATES
#define GEN_PASS_DEF_XRPLANFIXEDIMAGEUPDATES
#define GEN_PASS_DEF_XRPLANSTATICZORDER
#define GEN_PASS_DEF_XRPLANREPEATROWS
#define GEN_PASS_DEF_XRCACHESTATICSUBTREES
#define GEN_PASS_DEF_XRSPECIALIZESTATICREPEATS
#define GEN_PASS_DEF_XRSHARESTATICSUBTREES
#define GEN_PASS_DEF_XRPRECOMPILESTATICPATHS
#define GEN_PASS_DEF_XRPLANSTATICGRID
#define GEN_PASS_DEF_XRCACHESTATICGRADIENTS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
void clearMarker(Operation *op, StringAttr name) {
  if (op->getDiscardableAttr(name))
    op->removeDiscardableAttr(name);
}

bool passiveElement(llvm::StringRef kind) {
  return llvm::StringSwitch<bool>(kind)
      .Cases({"Rectangle", "Text", "Label", "StyledText", "Image", "Path", "HorizontalBox",
              "VerticalBox", "HorizontalLayout", "VerticalLayout", "GridLayout",
              "FlowLayout", "FlexboxLayout", "Row"},
             true)
      .Default(false);
}

/// Shared analysis of scene properties and row structure. The planning passes
/// only attach discardable hints, so each can preserve these facts.
struct SceneAnalysis {
  llvm::DenseSet<Operation *> paintOnly;
  llvm::DenseMap<Operation *, llvm::SmallVector<int64_t>> zOrders;
  llvm::DenseMap<Operation *, bool> repeatRows;

  explicit SceneAnalysis(Operation *module) {
    module->walk([&](BindOp binding) {
      auto element = dyn_cast_or_null<ElementOp>(binding->getParentOp());
      if (!element)
        return;
      auto kind = element.getKind();
      auto name = binding.getName();
      if ((kind == "Rectangle" && name == "background") ||
          (kind == "Text" && name == "color"))
        paintOnly.insert(binding);
    });
    module->walk([&](ElementOp element) {
      if (auto order = staticZOrder(element))
        zOrders.try_emplace(element, std::move(*order));
      auto kind = element.getKind();
      if (kind != "ListView" && kind != "GridView" && kind != "TableView" &&
          kind != "TreeView")
        return;
      for (auto &child : element.getBody().front())
        if (isa<ElementOp, InstanceOp>(child))
          if (auto index = cacheableRow(&child))
            repeatRows.try_emplace(&child, *index);
    });
  }

private:
  static std::optional<llvm::SmallVector<int64_t>> staticZOrder(ElementOp parent) {
    if (!passiveElement(parent.getKind()) && parent.getKind() != "Window")
      return {};
    llvm::SmallVector<double> z;
    for (auto &op : parent.getBody().front()) {
      if (isa<InstanceOp>(op))
        return {};
      auto child = dyn_cast<ElementOp>(op);
      if (!child)
        continue;
      if (!passiveElement(child.getKind()) && child.getKind() != "Button")
        return {};
      double value = 0;
      for (auto &member : child.getBody().front()) {
        if (isa<RepeatOp, ShowIfOp>(member))
          return {};
        if (auto binding = dyn_cast<BindOp>(member);
            binding && binding.getName() == "z") {
          auto literal = binding.getInput().getDefiningOp<LiteralOp>();
          if (!literal || literal.getValue().getAsDouble(value) ||
              !std::isfinite(value))
            return {};
        }
        if (isa<StateSetOp, AnimateOp>(member) &&
            member.getAttrOfType<StringAttr>("name").getValue() == "z")
          return {};
      }
      z.push_back(value);
    }
    if (z.size() < 2)
      return {};
    if (std::is_sorted(z.begin(), z.end()))
      return llvm::SmallVector<int64_t>{};
    llvm::SmallVector<int64_t> order(z.size());
    std::iota(order.begin(), order.end(), 0);
    std::stable_sort(order.begin(), order.end(),
                     [&](int64_t a, int64_t b) { return z[a] < z[b]; });
    return order;
  }

  static std::optional<bool> cacheableRow(Operation *row) {
    auto repeats = row->getRegion(0).front().getOps<RepeatOp>();
    if (repeats.empty())
      return std::nullopt;
    auto repeat = *repeats.begin();
    bool hasInstance = false;
    bool usesIndex = false;
    auto result = row->walk([&](Operation *op) -> WalkResult {
      if (isa<InstanceOp>(op))
        hasInstance = true;
      if (auto element = dyn_cast<ElementOp>(op);
          element && !passiveElement(element.getKind()) &&
          !llvm::StringSwitch<bool>(element.getKind())
               .Cases({"TableRow", "TableCell", "TreeRow", "TreeCell"}, true)
               .Default(false))
        return WalkResult::interrupt();
      if (isa<CallOp, AnimateOp, ShowIfOp, StateSelectOp>(op) ||
          (isa<RepeatOp>(op) && op != repeat.getOperation()))
        return WalkResult::interrupt();
      if (auto param = dyn_cast<ParamOp>(op);
          param && !repeat.getIndex().empty() &&
          param.getValue() == repeat.getIndex())
        usesIndex = true;
      return WalkResult::advance();
    });
    if (!hasInstance || result.wasInterrupted())
      return std::nullopt;
    return usesIndex;
  }
};

bool emptyModel(Value model) {
  if (auto array = model.getDefiningOp<ArrayOp>())
    return array.getItems().empty();
  if (auto array = model.getDefiningOp<RecordArrayOp>())
    return array.getItems().empty();
  auto literal = model.getDefiningOp<LiteralOp>();
  int64_t count;
  return literal && isa<IntType>(model.getType()) &&
         !literal.getValue().getAsInteger(10, count) && count <= 0;
}

bool ordinaryPassiveRepeat(ElementOp element) {
  auto parent = dyn_cast_or_null<ElementOp>(element->getParentOp());
  if (!parent ||
      !llvm::StringSwitch<bool>(parent.getKind())
           .Cases({"Window", "ApplicationWindow", "Rectangle", "Pane", "ControlFrame",
                   "HorizontalLayout", "VerticalLayout", "HorizontalBox", "VerticalBox",
                   "GridLayout", "FlowLayout", "FlexboxLayout"}, true)
           .Default(false))
    return false;

  // The subtree must have no external addressing or state registration.
  // Keep the operations in the IR for validation and the interpreter.
  auto valid = element->walk([&](Operation *op) -> WalkResult {
    if (auto child = dyn_cast<ElementOp>(op)) {
      if (!child.getId().empty() || child.getDelegateAttr() ||
          !(passiveElement(child.getKind()) || child.getKind() == "Label"))
        return WalkResult::interrupt();
    } else if (!isa<RepeatOp, BindOp, ShowIfOp, EndOp>(op) &&
               op->getNumResults() != 1) {
      return WalkResult::interrupt();
    }
    return WalkResult::advance();
  });
  return !valid.wasInterrupted();
}

struct SpecializeEmptyRepeats
    : impl::XrSpecializeEmptyRepeatsBase<SpecializeEmptyRepeats> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.empty_repeat");
    auto marker = StringAttr::get(&getContext(), "true");
    getOperation()->walk([&](RepeatOp repeat) {
      clearMarker(repeat, name);
      auto element = dyn_cast_or_null<ElementOp>(repeat->getParentOp());
      if (element && emptyModel(repeat.getInput()) && ordinaryPassiveRepeat(element))
        repeat->setDiscardableAttr(name, marker);
    });
  }
};

struct PlanUpdateEffects : impl::XrPlanUpdateEffectsBase<PlanUpdateEffects> {
  void runOnOperation() override {
    const auto &analysis = getAnalysis<SceneAnalysis>();
    auto name = StringAttr::get(&getContext(), "xr.update_effect");
    auto paint = StringAttr::get(&getContext(), "paint");
    getOperation()->walk([&](BindOp binding) {
      clearMarker(binding, name);
      if (analysis.paintOnly.contains(binding))
        binding->setDiscardableAttr(name, paint);
    });
    markAnalysesPreserved<SceneAnalysis>();
  }
};

bool fixedVisualBox(ElementOp element) {
  auto parent = dyn_cast_or_null<ElementOp>(element->getParentOp());
  if (!parent ||
      !llvm::StringSwitch<bool>(parent.getKind())
           .Cases({"Window", "ApplicationWindow", "Rectangle", "Pane", "ControlFrame",
                   "HorizontalLayout", "VerticalLayout", "HorizontalBox", "VerticalBox",
                   "GridLayout", "FlexboxLayout", "FlowLayout", "Row"}, true)
           .Default(false))
    return false;

  bool width = false, height = false;
  auto dimension = [](llvm::StringRef name) {
    return name == "width" || name == "height";
  };
  for (auto &op : element.getBody().front()) {
    if (auto binding = dyn_cast<BindOp>(op); binding && dimension(binding.getName())) {
      auto value = binding.getInput();
      if (!isa<LengthType>(value.getType()) || !value.getDefiningOp<LiteralOp>())
        return false;
      (binding.getName() == "width" ? width : height) = true;
    }
    if (isa<StateSetOp, StateTimingOp, AnimateOp, ControlLinkOp>(op)) {
      auto name = op.getAttrOfType<StringAttr>("name").getValue();
      if (dimension(name))
        return false;
    }
  }
  return width && height;
}

struct PlanFixedTextUpdates
    : impl::XrPlanFixedTextUpdatesBase<PlanFixedTextUpdates> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.update_effect");
    auto paint = StringAttr::get(&getContext(), "paint");
    getOperation()->walk([&](ElementOp element) {
      auto kind = element.getKind();
      if (kind != "Text" && kind != "Label")
        return;
      for (auto binding : element.getBody().front().getOps<BindOp>()) {
        if (binding.getName() != "text")
          continue;
        clearMarker(binding, name);
        if (!binding.getInput().getDefiningOp<LiteralOp>() && fixedVisualBox(element))
          binding->setDiscardableAttr(name, paint);
      }
    });
  }
};

struct PlanFixedImageUpdates
    : impl::XrPlanFixedImageUpdatesBase<PlanFixedImageUpdates> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.update_effect");
    auto paint = StringAttr::get(&getContext(), "paint");
    getOperation()->walk([&](BindOp binding) {
      auto element = dyn_cast_or_null<ElementOp>(binding->getParentOp());
      if (!element || element.getKind() != "Image" || binding.getName() != "source")
        return;
      clearMarker(binding, name);
      if (!binding.getInput().getDefiningOp<ImageUrlOp>() && fixedVisualBox(element))
        binding->setDiscardableAttr(name, paint);
    });
  }
};

struct PlanStaticZOrder : impl::XrPlanStaticZOrderBase<PlanStaticZOrder> {
  void runOnOperation() override {
    const auto &analysis = getAnalysis<SceneAnalysis>();
    auto name = StringAttr::get(&getContext(), "xr.static_z_order");
    getOperation()->walk([&](ElementOp element) {
      clearMarker(element, name);
      if (auto order = analysis.zOrders.find(element);
          order != analysis.zOrders.end())
        element->setDiscardableAttr(
            name, DenseI64ArrayAttr::get(&getContext(), order->second));
    });
    markAnalysesPreserved<SceneAnalysis>();
  }
};

struct PlanRepeatRows : impl::XrPlanRepeatRowsBase<PlanRepeatRows> {
  void runOnOperation() override {
    const auto &analysis = getAnalysis<SceneAnalysis>();
    auto name = StringAttr::get(&getContext(), "xr.row_scene_cache");
    getOperation()->walk([&](Operation *row) {
      if (!isa<ElementOp, InstanceOp>(row))
        return;
      clearMarker(row, name);
      if (auto index = analysis.repeatRows.find(row);
          index != analysis.repeatRows.end())
        row->setDiscardableAttr(
            name, StringAttr::get(&getContext(),
                                  index->second ? "indexed" : "keyed"));
    });
    markAnalysesPreserved<SceneAnalysis>();
  }
};

struct PrecompileStaticPaths
    : impl::XrPrecompileStaticPathsBase<PrecompileStaticPaths> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.precompiled_path");
    auto marker = StringAttr::get(&getContext(), "true");
    getOperation()->walk([&](BindOp binding) {
      clearMarker(binding, name);
      auto element = dyn_cast_or_null<ElementOp>(binding->getParentOp());
      if (element && element.getKind() == "Path" &&
          binding.getName() == "commands" &&
          isa<StringType>(binding.getInput().getType()) &&
          binding.getInput().getDefiningOp<LiteralOp>())
        binding->setDiscardableAttr(name, marker);
    });
  }
};

bool gridTrack(llvm::StringRef name) {
  return name == "row" || name == "col" || name == "rowspan" ||
         name == "colspan" || name == "visible";
}

bool fixedGridChild(ElementOp element, bool allowRow) {
  auto kind = element.getKind();
  if ((kind == "Row" && !allowRow) ||
      (!passiveElement(kind) && kind != "Button" && kind != "CheckBox" &&
       kind != "TextInput" && kind != "LineEdit" && kind != "Slider"))
    return false;
  for (auto &op : element.getBody().front()) {
    if (isa<ShowIfOp, RepeatOp>(op))
      return false;
    if (auto binding = dyn_cast<BindOp>(op);
        binding && gridTrack(binding.getName())) {
      auto value = binding.getInput();
      if (!value.getDefiningOp<LiteralOp>() ||
          (binding.getName() == "visible" ? !isa<BoolType>(value.getType())
                                          : !isa<IntType>(value.getType())))
        return false;
    }
    if (auto animation = dyn_cast<AnimateOp>(op);
        animation && gridTrack(animation.getName()))
      return false;
    if (auto binding = dyn_cast<StateSetOp>(op);
        binding && gridTrack(binding.getName()))
      return false;
    if (kind == "Row" && isa<ElementOp, InstanceOp>(op)) {
      auto child = dyn_cast<ElementOp>(op);
      if (!child || !fixedGridChild(child, false))
        return false;
    }
  }
  return true;
}

struct PlanStaticGrid : impl::XrPlanStaticGridBase<PlanStaticGrid> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.static_grid");
    auto marker = StringAttr::get(&getContext(), "true");
    getOperation()->walk([&](ElementOp element) {
      clearMarker(element, name);
      if (element.getKind() != "GridLayout")
        return;
      auto &body = element.getBody().front();
      auto children = body.getOps<ElementOp>();
      if (!children.empty() && body.getOps<InstanceOp>().empty() &&
          llvm::all_of(children, [](ElementOp child) {
            return fixedGridChild(child, true);
          }))
        element->setDiscardableAttr(name, marker);
    });
  }
};

bool staticValue(Value value, llvm::DenseMap<Value, bool> &known) {
  if (auto found = known.find(value); found != known.end())
    return found->second;
  auto *definition = value.getDefiningOp();
  bool result =
      isa_and_nonnull<LiteralOp, ImageUrlOp, ImageDimensionOp, NineSliceOp,
                      MarkdownOp, SolidBrushOp, GradientStopOp, LinearGradientOp,
                      RadialGradientOp, MakeFontOp, MakeDateOp, MakeTimeOp,
                      MakeRecordOp>(definition) &&
      llvm::all_of(definition->getOperands(),
                   [&](Value operand) { return staticValue(operand, known); });
  known.try_emplace(value, result);
  return result;
}

class StaticSignatures {
public:
  explicit StaticSignatures(MLIRContext *context)
      : context(context), indexType(IntegerType::get(context, 64)) {}

  unsigned value(Value input) {
    if (auto found = values.find(input); found != values.end())
      return found->second;
    auto *op = input.getDefiningOp();
    llvm::SmallVector<Attribute> key{
        StringAttr::get(context, op->getName().getStringRef()),
        TypeAttr::get(input.getType()), op->getAttrDictionary()};
    for (auto operand : op->getOperands())
      key.push_back(IntegerAttr::get(indexType, value(operand)));
    auto id = intern(key);
    values.try_emplace(input, id);
    return id;
  }

  unsigned subtree(ElementOp element) {
    if (!element.getId().empty() || element.getDelegateAttr())
      return 0;
    llvm::SmallVector<Attribute> key{
        StringAttr::get(context, "xr.element"), element.getKindAttr()};
    for (auto name : {"xr.static_grid", "xr.static_z_order"})
      if (auto attribute = element->getDiscardableAttr(name))
        key.push_back(attribute);
    if (auto count = element->getDiscardableAttr("xr.static_repeat_count"))
      key.push_back(ArrayAttr::get(context, {count}));
    for (auto &op : element.getBody().front()) {
      if (auto bind = dyn_cast<BindOp>(op)) {
        key.push_back(bind.getNameAttr());
        key.push_back(IntegerAttr::get(indexType, value(bind.getInput())));
      } else if (auto child = dyn_cast<ElementOp>(op)) {
        auto id = subtree(child);
        if (!id)
          return 0;
        key.push_back(IntegerAttr::get(indexType, id));
      }
    }
    return intern(key);
  }

private:
  unsigned intern(llvm::ArrayRef<Attribute> key) {
    auto id = classes.size() + 1;
    return classes.try_emplace(ArrayAttr::get(context, key), id).first->second;
  }

  MLIRContext *context;
  IntegerType indexType;
  llvm::DenseMap<Value, unsigned> values;
  llvm::DenseMap<ArrayAttr, unsigned> classes;
};

class SharedStaticGroups {
public:
  SharedStaticGroups(MLIRContext *context, llvm::StringRef name)
      : name(StringAttr::get(context, name)),
        indexType(IntegerType::get(context, 64)) {}

  void add(Operation *op, unsigned signature) {
    auto [first, inserted] = representatives.try_emplace(signature, op);
    if (inserted)
      return;
    auto group = dyn_cast_or_null<IntegerAttr>(
        first->second->getDiscardableAttr(name));
    if (!group) {
      group = IntegerAttr::get(indexType, nextGroup++);
      first->second->setDiscardableAttr(name, group);
    }
    op->setDiscardableAttr(name, group);
  }

private:
  StringAttr name;
  IntegerType indexType;
  unsigned nextGroup = 0;
  llvm::DenseMap<unsigned, Operation *> representatives;
};

struct CacheStaticGradients
    : impl::XrCacheStaticGradientsBase<CacheStaticGradients> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.static_gradient");
    auto group = StringAttr::get(&getContext(), "xr.static_gradient_group");
    auto marker = StringAttr::get(&getContext(), "true");
    llvm::DenseMap<Value, bool> values;
    StaticSignatures signatures(&getContext());
    auto indexType = IntegerType::get(&getContext(), 64);
    getOperation()->walk([&](Operation *op) {
      if (!isa<LinearGradientOp, RadialGradientOp>(op))
        return;
      clearMarker(op, name);
      clearMarker(op, group);
      if (staticValue(op->getResult(0), values)) {
        auto signature = signatures.value(op->getResult(0));
        op->setDiscardableAttr(name, marker);
        op->setDiscardableAttr(group, IntegerAttr::get(indexType, signature));
      }
    });
  }
};

struct StaticSceneAnalysis {
  llvm::DenseSet<Operation *> subtrees;
  llvm::DenseMap<Operation *, int64_t> repeats;

  explicit StaticSceneAnalysis(Operation *module) {
    llvm::DenseMap<Value, bool> values;
    llvm::DenseMap<Operation *, llvm::StringSet<>> observed;
    auto observe = [&](Operation *op, llvm::StringRef key) {
      if (auto component = op->getParentOfType<ComponentOp>())
        observed[component].insert(key);
    };
    module->walk([&](Operation *op) {
      if (auto geometry = dyn_cast<GeometryReadOp>(op))
        observe(op, geometry.getOwner());
      if (auto focus = dyn_cast<FocusReadOp>(op))
        observe(op, focus.getOwner());
      if (auto command = dyn_cast<FocusCommandOp>(op))
        observe(op, command.getTarget());
      if (auto target = dyn_cast<FocusTargetOp>(op))
        observe(op, target.getTarget());
    });
    module->walk<WalkOrder::PostOrder>([&](ElementOp element) {
      if (!passiveElement(element.getKind()))
        return;
      auto component = observed.find(element->getParentOfType<ComponentOp>());
      auto key = element.getKey().value_or(element.getId());
      if (component != observed.end() && component->second.contains(key))
        return;
      RepeatOp repeat;
      for (auto &child : element.getBody().front()) {
        if (auto binding = dyn_cast<BindOp>(child)) {
          if (!staticValue(binding.getInput(), values))
            return;
        } else if (auto model = dyn_cast<RepeatOp>(child); model && !repeat) {
          repeat = model;
        } else if (!subtrees.contains(&child) && !isa<EndOp>(child) &&
                   (child.getNumResults() != 1 ||
                    !staticValue(child.getResult(0), values))) {
          // Events, properties, conditional children, repeats and controls
          // are left to the normal renderer and state machinery.
          return;
        }
      }
      if (!repeat) {
        subtrees.insert(element);
        return;
      }
      // Bound the size retained by each lazily built snapshot. Large and
      // virtualized repeats continue to use their usual row machinery.
      constexpr int64_t maxStaticRows = 128;
      auto count = repeat.getInput().getDefiningOp<LiteralOp>();
      int64_t rows;
      if (count && isa<IntType>(repeat.getInput().getType()) &&
          !count.getValue().getAsInteger(10, rows) && rows > 0 &&
          rows <= maxStaticRows && ordinaryPassiveRepeat(element))
        repeats.try_emplace(element, rows);
    });
  }
};

struct CacheStaticSubtrees
    : impl::XrCacheStaticSubtreesBase<CacheStaticSubtrees> {
  void runOnOperation() override {
    const auto &analysis = getAnalysis<StaticSceneAnalysis>();
    auto name = StringAttr::get(&getContext(), "xr.static_subtree");
    auto marker = StringAttr::get(&getContext(), "true");
    getOperation()->walk([&](ElementOp element) {
      clearMarker(element, name);
      auto *parent = element->getParentOp();
      if (analysis.subtrees.contains(element) &&
          isa_and_nonnull<ElementOp, InstanceOp>(parent) &&
          !analysis.subtrees.contains(parent) && !analysis.repeats.contains(parent))
        element->setDiscardableAttr(name, marker);
    });
    markAnalysesPreserved<StaticSceneAnalysis>();
  }
};

struct SpecializeStaticRepeats
    : impl::XrSpecializeStaticRepeatsBase<SpecializeStaticRepeats> {
  void runOnOperation() override {
    const auto &analysis = getAnalysis<StaticSceneAnalysis>();
    auto name = StringAttr::get(&getContext(), "xr.static_repeat_count");
    auto subtree = StringAttr::get(&getContext(), "xr.static_subtree");
    auto marker = StringAttr::get(&getContext(), "true");
    auto indexType = IntegerType::get(&getContext(), 64);
    getOperation()->walk([&](ElementOp element) {
      clearMarker(element, name);
      if (auto rows = analysis.repeats.lookup(element)) {
        element->setDiscardableAttr(name, IntegerAttr::get(indexType, rows));
        element->setDiscardableAttr(subtree, marker);
      }
    });
  }
};

struct ShareStaticSubtrees
    : impl::XrShareStaticSubtreesBase<ShareStaticSubtrees> {
  void runOnOperation() override {
    auto group = StringAttr::get(&getContext(), "xr.static_subtree_group");
    StaticSignatures signatures(&getContext());
    SharedStaticGroups groups(&getContext(), group.getValue());
    getOperation()->walk([&](ElementOp element) {
      clearMarker(element, group);
      if (!element->getDiscardableAttr("xr.static_subtree"))
        return;
      if (auto signature = signatures.subtree(element))
        groups.add(element, signature);
    });
  }
};
} // namespace

void addScenePlanPasses(OpPassManager &passes) {
  passes.addPass(std::make_unique<SpecializeEmptyRepeats>());
  passes.addPass(std::make_unique<PrecompileStaticPaths>());
  passes.addPass(std::make_unique<PlanStaticGrid>());
  passes.addPass(std::make_unique<CacheStaticGradients>());
  passes.addPass(std::make_unique<PlanUpdateEffects>());
  passes.addPass(std::make_unique<PlanFixedTextUpdates>());
  passes.addPass(std::make_unique<PlanFixedImageUpdates>());
  passes.addPass(std::make_unique<PlanStaticZOrder>());
  passes.addPass(std::make_unique<PlanRepeatRows>());
  passes.addPass(std::make_unique<CacheStaticSubtrees>());
  passes.addPass(std::make_unique<SpecializeStaticRepeats>());
  passes.addPass(std::make_unique<ShareStaticSubtrees>());
}
} // namespace xrack::dialect
