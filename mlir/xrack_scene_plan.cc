#include "mlir/xrack_passes.h"

#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/StringSwitch.h"
#include <algorithm>
#include <cmath>
#include <numeric>
#include <optional>

namespace xrack::dialect {
#define GEN_PASS_DEF_XRPLANUPDATEEFFECTS
#define GEN_PASS_DEF_XRPLANSTATICZORDER
#define GEN_PASS_DEF_XRPLANREPEATROWS
#define GEN_PASS_DEF_XRCACHESTATICSUBTREES
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
      .Cases({"Rectangle", "Text", "StyledText", "Image", "Path", "HorizontalBox",
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
      if (auto order = staticZOrder(element); !order.empty())
        zOrders.try_emplace(element, std::move(order));
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
  static llvm::SmallVector<int64_t> staticZOrder(ElementOp parent) {
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
    if (std::is_sorted(z.begin(), z.end()))
      return {};
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
      isa_and_nonnull<LiteralOp, ImageUrlOp, ImageDimensionOp, NineSliceOp, MarkdownOp, SolidBrushOp, GradientStopOp,
                      LinearGradientOp, RadialGradientOp>(definition) &&
      llvm::all_of(definition->getOperands(),
                   [&](Value operand) { return staticValue(operand, known); });
  known.try_emplace(value, result);
  return result;
}

struct CacheStaticGradients
    : impl::XrCacheStaticGradientsBase<CacheStaticGradients> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.static_gradient");
    auto marker = StringAttr::get(&getContext(), "true");
    llvm::DenseMap<Value, bool> values;
    getOperation()->walk([&](Operation *op) {
      if (!isa<LinearGradientOp, RadialGradientOp>(op))
        return;
      clearMarker(op, name);
      if (staticValue(op->getResult(0), values))
        op->setDiscardableAttr(name, marker);
    });
  }
};

struct CacheStaticSubtrees
    : impl::XrCacheStaticSubtreesBase<CacheStaticSubtrees> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.static_subtree");
    auto marker = StringAttr::get(&getContext(), "true");
    llvm::DenseSet<Operation *> staticElements;
    llvm::DenseMap<Value, bool> values;
    getOperation()->walk<WalkOrder::PostOrder>([&](ElementOp element) {
      clearMarker(element, name);
      if (!passiveElement(element.getKind()))
        return;
      for (auto &child : element.getBody().front()) {
        if (auto binding = dyn_cast<BindOp>(child)) {
          if (!staticValue(binding.getInput(), values))
            return;
        } else if (!staticElements.contains(&child) && !isa<EndOp>(child) &&
                   (child.getNumResults() != 1 ||
                    !staticValue(child.getResult(0), values))) {
          // Events, properties, conditional children, repeats and controls
          // are left to the normal renderer and state machinery.
          return;
        }
      }
      staticElements.insert(element);
    });
    getOperation()->walk([&](ElementOp element) {
      auto *parent = element->getParentOp();
      if (staticElements.contains(element) &&
          isa_and_nonnull<ElementOp, InstanceOp>(parent) &&
          !staticElements.contains(parent))
        element->setDiscardableAttr(name, marker);
    });
  }
};
} // namespace

void addScenePlanPasses(OpPassManager &passes) {
  passes.addPass(std::make_unique<PrecompileStaticPaths>());
  passes.addPass(std::make_unique<PlanStaticGrid>());
  passes.addPass(std::make_unique<CacheStaticGradients>());
  passes.addPass(std::make_unique<PlanUpdateEffects>());
  passes.addPass(std::make_unique<PlanStaticZOrder>());
  passes.addPass(std::make_unique<PlanRepeatRows>());
  passes.addPass(std::make_unique<CacheStaticSubtrees>());
}
} // namespace xrack::dialect
