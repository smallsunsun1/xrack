#include "mlir/xrack_passes.h"

#include "mlir/IR/BuiltinAttributes.h"
#include "mlir/IR/BuiltinTypes.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/DenseMap.h"
#include "llvm/ADT/SmallVector.h"
#include <string>

namespace xrack::dialect {
#define GEN_PASS_DEF_XRSHAREVISUALBINDINGS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
constexpr llvm::StringLiteral cacheGroup = "xr.visual_cache_group";

Operation *visualScope(Operation *binding) {
  auto *parent = binding->getParentOp();
  if (auto element = dyn_cast_or_null<ElementOp>(parent)) {
    if (element.getKind() == "Action" || !isa<BindOp, ShowIfOp>(binding))
      return nullptr;
  } else if (!isa_and_nonnull<InstanceOp>(parent) ||
             !isa<LayoutBindOp, ShowIfOp>(binding)) {
    return nullptr;
  }

  // The backend has a cache per component instance and a separate cache in
  // each row of the nearest repeat. Never share across those boundaries.
  for (auto *op = parent; op; op = op->getParentOp()) {
    if (isa<ComponentOp>(op))
      return op;
    if (isa<ElementOp, InstanceOp>(op) &&
        !op->getRegion(0).front().getOps<RepeatOp>().empty())
      return op;
  }
  return nullptr;
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
    if (!op || !isa<IntType, FloatType, BoolType, StringType, LengthType,
                    PercentType, ColorType>(value.getType()))
      return 0;
    if (auto read = dyn_cast<ReadOp>(op)) {
      if (read.getOwner().value_or("root") != "root")
        return 0;
    } else if (isa<ParamOp>(op)) {
      if (!inRow)
        return 0;
    } else if (!isa<LiteralOp, UnaryOp, BinaryOp, CastOp, ToRatioOp,
                    StringifyOp, SelectOp>(op)) {
      // In particular, callback invocations and navigation reads retain their
      // independent evaluation, even if they print the same way in MLIR.
      return 0;
    }

    auto *context = op->getContext();
    llvm::SmallVector<Attribute, 6> key{
        StringAttr::get(context, op->getName().getStringRef()),
        TypeAttr::get(value.getType()), op->getAttrDictionary()};
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
      if (!isa<BindOp, LayoutBindOp, ShowIfOp>(op))
        return;
      auto *scope = visualScope(op);
      if (!scope)
        return;
      Value input = op->getOperand(0);
      // These leaves already bypass the backend's visual binding cache.
      if (isa_and_nonnull<LiteralOp, ReadOp, ParamOp>(input.getDefiningOp()))
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
} // namespace

void addShareVisualBindingsPass(OpPassManager &passes) {
  passes.addPass(std::make_unique<ShareVisualBindings>());
}
} // namespace xrack::dialect
