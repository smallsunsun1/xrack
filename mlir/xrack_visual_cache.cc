#include "mlir/xrack_passes.h"

#include "mlir/IR/BuiltinAttributes.h"
#include "mlir/IR/BuiltinTypes.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/DenseSet.h"
#include "llvm/ADT/SmallVector.h"
#include <string>

namespace xrack::dialect {
#define GEN_PASS_DEF_XRSHAREVISUALBINDINGS
#define GEN_PASS_DEF_XRHOISTREPEATINVARIANTS
#define GEN_PASS_DEF_XRHOISTREPEATSUBEXPRESSIONS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
constexpr llvm::StringLiteral cacheGroup = "xr.visual_cache_group";
constexpr llvm::StringLiteral repeatInvariantGroup =
    "xr.repeat_invariant_group";
constexpr llvm::StringLiteral repeatSubexpressionGroup =
    "xr.repeat_subexpression_group";

bool cacheValue(Type type) {
  return isa<IntType, FloatType, BoolType, StringType, LengthType, PercentType,
             ColorType, BrushType>(type);
}

bool shareableExpression(Operation *op) {
  return isa_and_nonnull<UnaryOp, BinaryOp, CastOp, ToRatioOp, StringifyOp,
                         SelectOp, GetFieldOp, DateFieldOp, TimeFieldOp,
                         FontFieldOp, TranslateOp, SolidBrushOp, GradientStopOp,
                         LinearGradientOp, RadialGradientOp>(op);
}

bool isStaticGradient(Operation *op) {
  return isa_and_nonnull<LinearGradientOp, RadialGradientOp>(op) &&
         op->getDiscardableAttr("xr.static_gradient");
}

Operation *visualScope(Operation *binding) {
  auto *parent = binding->getParentOp();
  if (auto element = dyn_cast_or_null<ElementOp>(parent)) {
    if (element.getKind() == "Action" || element.getKind() == "ActionGroup" ||
        !isa<BindOp, ShowIfOp>(binding))
      return nullptr;
  } else if (!isa_and_nonnull<InstanceOp>(parent) ||
             !isa<LayoutBindOp, ShowIfOp>(binding)) {
    return nullptr;
  }

  // The backend has a cache per component instance and a separate cache in
  // each row of the nearest repeat. Never share across those boundaries.
  for (auto *op = parent; op; op = op->getParentOp()) {
    if (auto element = dyn_cast<ElementOp>(op);
        element && (element.getKind() == "DayCell" || element.getKind() == "SplitHandle"))
      return nullptr; // The runtime caches each template instance separately.
    if (isa<ComponentOp>(op))
      return op;
    if (isa<ElementOp, InstanceOp>(op) &&
        !op->getRegion(0).front().getOps<RepeatOp>().empty())
      return op;
  }
  return nullptr;
}

Value cacheableInput(Operation *op) {
  if (!isa<BindOp, LayoutBindOp, ShowIfOp>(op))
    return {};
  Value input = op->getOperand(0);
  // These leaves already bypass the backend's visual binding cache.
  auto *definition = input.getDefiningOp();
  if (isa_and_nonnull<LiteralOp, ReadOp, ParamOp>(definition) ||
      isStaticGradient(definition))
    return {};
  return input;
}

bool insideSpecializedVisual(Operation *binding) {
  for (auto *op = binding->getParentOp();
       isa_and_nonnull<ElementOp, InstanceOp>(op); op = op->getParentOp()) {
    if (op->getDiscardableAttr("xr.static_subtree"))
      return true;
    for (auto repeat : op->getRegion(0).front().getOps<RepeatOp>())
      if (repeat->getDiscardableAttr("xr.empty_repeat"))
        return true;
    for (auto guard : op->getRegion(0).front().getOps<ShowIfOp>()) {
      if (guard->getDiscardableAttr("xr.static_hidden"))
        return true;
    }
  }
  return false;
}

struct ScopePlan {
  // Zero means the value cannot share a cache. Memoized class numbers keep
  // keys linear in the size of the SSA graph, even for repeated operands.
  llvm::DenseMap<Value, unsigned> classes;
  llvm::DenseMap<ArrayAttr, unsigned> expressions;
  llvm::DenseMap<unsigned, Operation *> firstUse;

  unsigned classify(Value value, bool inRow) {
    if (auto found = classes.find(value); found != classes.end())
      return found->second;
    auto id = classifyDefinition(value, inRow);
    classes.try_emplace(value, id);
    return id;
  }

private:
  unsigned classifyDefinition(Value value, bool inRow) {
    auto *op = value.getDefiningOp();
    if (!op)
      return 0;
    auto type = value.getType();
    // Aggregate projections and stops are intermediates in the signature.
    if (!cacheValue(type) && !isa<GradientStopOp>(op) &&
        !(isa<RecordType, DateType, TimeType, FontType>(type) &&
          isa<ReadOp, ParamOp, GetFieldOp>(op)))
      return 0;
    if (auto read = dyn_cast<ReadOp>(op)) {
      if (read.getOwner().value_or("root") != "root")
        return 0;
    } else if (isa<ParamOp>(op)) {
      if (!inRow)
        return 0;
    } else if (!isa<LiteralOp>(op) && !shareableExpression(op)) {
      // In particular, callback invocations and navigation reads retain their
      // independent evaluation, even if they print the same way in MLIR.
      return 0;
    }

    auto *context = op->getContext();
    auto attributes = op->getAttrDictionary();
    if (op->getDiscardableAttr(repeatSubexpressionGroup)) {
      NamedAttrList semantic(attributes);
      semantic.erase(repeatSubexpressionGroup);
      attributes = semantic.getDictionary(context);
    }
    llvm::SmallVector<Attribute, 6> key{
        StringAttr::get(context, op->getName().getStringRef()),
        TypeAttr::get(value.getType()), attributes};
    auto indexType = IntegerType::get(context, 64);
    for (Value operand : op->getOperands()) {
      auto id = classify(operand, inRow);
      if (!id)
        return 0;
      key.push_back(IntegerAttr::get(indexType, id));
    }
    auto signature = ArrayAttr::get(context, key);
    auto next = static_cast<unsigned>(expressions.size()) + 1;
    return expressions.try_emplace(signature, next).first->second;
  }
};

struct ShareVisualBindings
    : impl::XrShareVisualBindingsBase<ShareVisualBindings> {
  void runOnOperation() override {
    llvm::DenseMap<Operation *, ScopePlan> plans;
    unsigned nextGroup = 0;
    auto attribute = StringAttr::get(&getContext(), cacheGroup);
    getOperation()->walk([&](Operation *op) {
      // Recompute annotations even when the input was previously optimized.
      if (op->getDiscardableAttr(attribute))
        op->removeDiscardableAttr(attribute);
      Value input = cacheableInput(op);
      if (!input || !cacheValue(input.getType()) ||
          op->getDiscardableAttr(repeatInvariantGroup) ||
          insideSpecializedVisual(op))
        return;
      auto *scope = visualScope(op);
      if (!scope)
        return;
      auto &plan = plans[scope];
      auto id = plan.classify(input, !isa<ComponentOp>(scope));
      if (!id)
        return;
      auto [first, inserted] = plan.firstUse.try_emplace(id, op);
      if (inserted)
        return;
      auto *original = first->second;
      auto group =
          dyn_cast_or_null<StringAttr>(original->getDiscardableAttr(attribute));
      if (!group) {
        group = StringAttr::get(&getContext(), std::to_string(nextGroup++));
        original->setDiscardableAttr(attribute, group);
      }
      op->setDiscardableAttr(attribute, group);
    });
  }
};

struct HoistRepeatInvariants
    : impl::XrHoistRepeatInvariantsBase<HoistRepeatInvariants> {
  void runOnOperation() override {
    struct ComponentPlan {
      ScopePlan expressions;
      llvm::DenseSet<unsigned> hoisted;
    };
    llvm::DenseMap<Operation *, ComponentPlan> components;
    auto attribute = StringAttr::get(&getContext(), repeatInvariantGroup);
    auto mark = [&](Operation *op, unsigned id) {
      op->setDiscardableAttr(
          attribute, StringAttr::get(&getContext(), std::to_string(id)));
    };
    getOperation()->walk([&](Operation *op) {
      if (op->getDiscardableAttr(attribute))
        op->removeDiscardableAttr(attribute);
      Value input = cacheableInput(op);
      if (!input || !cacheValue(input.getType()))
        return;
      auto *scope = visualScope(op);
      if (!scope || isa<ComponentOp>(scope) || insideSpecializedVisual(op))
        return;
      auto component = scope->getParentOfType<ComponentOp>();
      if (!component)
        return;
      // Reject row parameters, local reads and calls anywhere in the graph.
      // The backend will evaluate the shared cache only at a visual use site.
      auto &plan = components[component.getOperation()];
      if (auto id = plan.expressions.classify(input, false)) {
        plan.hoisted.insert(id);
        mark(op, id);
      }
    });
    // An equivalent use outside the repeat can use the same component slot.
    getOperation()->walk([&](Operation *op) {
      Value input = cacheableInput(op);
      if (!input || !cacheValue(input.getType()))
        return;
      auto *scope = visualScope(op);
      if (!isa_and_nonnull<ComponentOp>(scope) || insideSpecializedVisual(op))
        return;
      auto found = components.find(scope);
      if (found == components.end() || found->second.hoisted.empty())
        return;
      auto &plan = found->second;
      auto id = plan.expressions.classify(input, false);
      if (plan.hoisted.contains(id))
        mark(op, id);
    });
  }
};

void markRepeatSubexpressions(Value value, ScopePlan &plan,
                              llvm::DenseSet<Value> &visited, StringAttr name) {
  auto *definition = value.getDefiningOp();
  if (!shareableExpression(definition) || isStaticGradient(definition) ||
      !visited.insert(value).second)
    return;
  if (auto id = plan.classify(value, false); id && cacheValue(value.getType())) {
    definition->setDiscardableAttr(
        name, StringAttr::get(definition->getContext(), std::to_string(id)));
    return;
  }
  // Keep the original use graph and visit children only when the entire
  // expression is not invariant. The backend caches at the original use site.
  for (Value operand : definition->getOperands())
    markRepeatSubexpressions(operand, plan, visited, name);
}

struct HoistRepeatSubexpressions
    : impl::XrHoistRepeatSubexpressionsBase<HoistRepeatSubexpressions> {
  void runOnOperation() override {
    llvm::DenseMap<Operation *, ScopePlan> components;
    llvm::DenseSet<Value> visited;
    auto attribute = StringAttr::get(&getContext(), repeatSubexpressionGroup);
    getOperation()->walk([&](Operation *op) {
      if (op->getDiscardableAttr(attribute))
        op->removeDiscardableAttr(attribute);
    });
    getOperation()->walk([&](Operation *op) {
      auto input = cacheableInput(op);
      if (!input || op->getDiscardableAttr(repeatInvariantGroup))
        return;
      auto *scope = visualScope(op);
      if (!scope || isa<ComponentOp>(scope) || insideSpecializedVisual(op))
        return;
      if (auto component = scope->getParentOfType<ComponentOp>())
        markRepeatSubexpressions(input, components[component.getOperation()],
                                 visited, attribute);
    });
  }
};
} // namespace

void addVisualCachePasses(OpPassManager &passes) {
  passes.addPass(std::make_unique<HoistRepeatInvariants>());
  passes.addPass(std::make_unique<HoistRepeatSubexpressions>());
  passes.addPass(std::make_unique<ShareVisualBindings>());
}
} // namespace xrack::dialect
