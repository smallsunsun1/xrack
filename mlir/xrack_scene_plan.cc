#include "mlir/xrack_passes.h"

#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/StringSwitch.h"

namespace xrack::dialect {
#define GEN_PASS_DEF_XRPLANUPDATEEFFECTS
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

struct PlanUpdateEffects : impl::XrPlanUpdateEffectsBase<PlanUpdateEffects> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.update_effect");
    auto paint = StringAttr::get(&getContext(), "paint");
    getOperation()->walk([&](BindOp binding) {
      clearMarker(binding, name);
      // Rectangle background is consumed by drawing, independently of layout,
      // hit testing and accessibility. Other bindings keep the general path.
      auto element = dyn_cast_or_null<ElementOp>(binding->getParentOp());
      if (element && element.getKind() == "Rectangle" &&
          binding.getName() == "background")
        binding->setDiscardableAttr(name, paint);
    });
  }
};

bool passiveElement(llvm::StringRef kind) {
  return llvm::StringSwitch<bool>(kind)
      .Cases({"Rectangle", "Text", "StyledText", "Image", "Path", "HorizontalBox",
              "VerticalBox", "HorizontalLayout", "VerticalLayout", "GridLayout",
              "FlowLayout", "FlexboxLayout", "Row"},
             true)
      .Default(false);
}

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
  passes.addPass(std::make_unique<CacheStaticSubtrees>());
}
} // namespace xrack::dialect
