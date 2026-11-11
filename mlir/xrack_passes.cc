#include "mlir/xrack_passes.h"

#include "mlir/IR/Iterators.h"
#include "mlir/IR/PatternMatch.h"
#include "mlir/Interfaces/SideEffectInterfaces.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/Transforms/GreedyPatternRewriteDriver.h"
#include "mlir/Transforms/Passes.h"
#include "mlir/xrack_analysis.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/APFloat.h"
#include "llvm/ADT/APInt.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/ADT/StringSwitch.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/raw_ostream.h"
#include <optional>
#include <string>

namespace xrack::dialect {
void addVisualCachePasses(mlir::OpPassManager &passes);
void addScenePlanPasses(mlir::OpPassManager &passes);

#define GEN_PASS_DEF_XRFOLDCONSTANTS
#define GEN_PASS_DEF_XRSIMPLIFYEXPRESSIONS
#define GEN_PASS_DEF_XRSIMPLIFYVISUALS
#define GEN_PASS_DEF_XRPRUNEEXPRESSIONS
#define GEN_PASS_DEF_XRPRUNECONSTANTSTATES
#define GEN_PASS_DEF_XRREMOVEUNUSEDPROPERTIES
#define GEN_PASS_DEF_XRSPECIALIZEHIDDENVISUALS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
void replaceLiteral(PatternRewriter &rewriter, Operation *op,
                    llvm::StringRef spelling) {
  rewriter.replaceOpWithNewOp<LiteralOp>(op, op->getResult(0).getType(),
                                         rewriter.getStringAttr(spelling));
}

LogicalResult replaceFloatLiteral(PatternRewriter &rewriter, Operation *op,
                                  const llvm::APFloat &value) {
  // XR's literal verifier accepts finite values. Keep operations that produce
  // infinities or NaN so their IEEE behavior remains in the generated Rust.
  if (!value.isFinite())
    return failure();
  llvm::SmallString<32> spelling;
  value.toString(spelling);
  replaceLiteral(rewriter, op, spelling);
  return success();
}

void replaceStringLiteral(PatternRewriter &rewriter, Operation *op,
                          llvm::StringRef value) {
  std::string spelling;
  llvm::raw_string_ostream stream(spelling);
  stream << llvm::json::Value(value);
  replaceLiteral(rewriter, op, stream.str());
}

std::optional<std::string> stringLiteral(LiteralOp literal) {
  if (!literal || !isa<StringType>(literal.getResult().getType()))
    return std::nullopt;
  auto parsed = llvm::json::parse(literal.getValue());
  if (!parsed) {
    llvm::consumeError(parsed.takeError());
    return std::nullopt;
  }
  auto value = parsed->getAsString();
  return value ? std::optional(value->str()) : std::nullopt;
}

struct FoldUnary : OpRewritePattern<UnaryOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(UnaryOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    if (!literal)
      return failure();
    if (isa<BoolType>(op.getResult().getType()) && op.getValue() == "!") {
      replaceLiteral(rewriter, op,
                     literal.getValue() == "true" ? "false" : "true");
      return success();
    }
    if (isa<FloatType, LengthType>(op.getResult().getType()) &&
        op.getValue() == "-") {
      double number;
      if (literal.getValue().getAsDouble(number))
        return failure();
      llvm::APFloat value(number);
      value.changeSign();
      return replaceFloatLiteral(rewriter, op, value);
    }
    int64_t value;
    if (!isa<IntType>(op.getResult().getType()) ||
        literal.getValue().getAsInteger(10, value))
      return failure();
    if (op.getValue() == "+") {
      rewriter.replaceOp(op, literal.getResult());
      return success();
    }
    if (op.getValue() != "-" || value == INT64_MIN)
      return failure();
    replaceLiteral(rewriter, op, std::to_string(-value));
    return success();
  }
};

LogicalResult foldInt(BinaryOp op, PatternRewriter &rewriter, int64_t left,
                      int64_t right) {
  auto spelling = op.getValue();
  if (isa<BoolType>(op.getResult().getType())) {
    auto value = llvm::StringSwitch<std::optional<bool>>(spelling)
                     .Case("==", left == right)
                     .Case("!=", left != right)
                     .Case("<", left < right)
                     .Case("<=", left <= right)
                     .Case(">", left > right)
                     .Case(">=", left >= right)
                     .Default(std::nullopt);
    if (!value)
      return failure();
    replaceLiteral(rewriter, op, *value ? "true" : "false");
    return success();
  }
  if (!isa<IntType>(op.getResult().getType()))
    return failure();
  llvm::APInt lhs(64, static_cast<uint64_t>(left));
  llvm::APInt rhs(64, static_cast<uint64_t>(right));
  llvm::APInt value;
  bool overflow = false;
  if (spelling == "+")
    value = lhs.sadd_ov(rhs, overflow);
  else if (spelling == "-")
    value = lhs.ssub_ov(rhs, overflow);
  else if (spelling == "*")
    value = lhs.smul_ov(rhs, overflow);
  else if ((spelling == "/" || spelling == "%") && right != 0 &&
           !(left == INT64_MIN && right == -1))
    value = spelling == "/" ? lhs.sdiv(rhs) : lhs.srem(rhs);
  else
    return failure();
  // Keep the generated Rust's overflow behavior for operations that may trap.
  if (overflow)
    return failure();
  replaceLiteral(rewriter, op, std::to_string(value.getSExtValue()));
  return success();
}

LogicalResult foldFloat(BinaryOp op, PatternRewriter &rewriter, LiteralOp lhs,
                        LiteralOp rhs) {
  double left, right;
  if (lhs.getValue().getAsDouble(left) || rhs.getValue().getAsDouble(right))
    return failure();
  llvm::APFloat value(left), operand(right);
  auto spelling = op.getValue();
  if (isa<BoolType>(op.getResult().getType())) {
    if (op.getLhs().getType() != op.getRhs().getType())
      return failure();
    auto order = value.compare(operand);
    auto result = llvm::StringSwitch<std::optional<bool>>(spelling)
                      .Case("==", order == llvm::APFloat::cmpEqual)
                      .Case("!=", order != llvm::APFloat::cmpEqual)
                      .Case("<", order == llvm::APFloat::cmpLessThan)
                      .Case("<=", order != llvm::APFloat::cmpGreaterThan)
                      .Case(">", order == llvm::APFloat::cmpGreaterThan)
                      .Case(">=", order != llvm::APFloat::cmpLessThan)
                      .Default(std::nullopt);
    if (!result)
      return failure();
    replaceLiteral(rewriter, op, *result ? "true" : "false");
    return success();
  }
  if (!isa<FloatType, LengthType>(op.getResult().getType()))
    return failure();
  auto rounding = llvm::APFloat::rmNearestTiesToEven;
  if (spelling == "+")
    value.add(operand, rounding);
  else if (spelling == "-")
    value.subtract(operand, rounding);
  else if (spelling == "*")
    value.multiply(operand, rounding);
  else if (spelling == "/")
    value.divide(operand, rounding);
  else
    return failure();
  return replaceFloatLiteral(rewriter, op, value);
}

LogicalResult foldString(BinaryOp op, PatternRewriter &rewriter, LiteralOp lhs,
                         LiteralOp rhs) {
  auto left = stringLiteral(lhs);
  auto right = stringLiteral(rhs);
  if (!left || !right)
    return failure();
  auto spelling = op.getValue();
  auto type = op.getResult().getType();
  if (isa<BoolType>(type) && (spelling == "==" || spelling == "!=")) {
    auto equal = *left == *right;
    replaceLiteral(rewriter, op,
                   equal == (spelling == "==") ? "true" : "false");
    return success();
  }
  // Bound literal growth when a graph repeatedly concatenates shared values.
  if (spelling != "+" || !isa<StringType>(type) ||
      left->size() + right->size() > 64 * 1024)
    return failure();
  *left += *right;
  replaceStringLiteral(rewriter, op, *left);
  return success();
}

struct FoldBinary : OpRewritePattern<BinaryOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(BinaryOp op,
                                PatternRewriter &rewriter) const override {
    auto lhs = op.getLhs().getDefiningOp<LiteralOp>();
    if (!lhs)
      return failure();
    auto rhs = op.getRhs().getDefiningOp<LiteralOp>();
    auto spelling = op.getValue();
    if (isa<BoolType>(op.getLhs().getType()) &&
        op.getLhs().getType() == op.getRhs().getType() &&
        isa<BoolType>(op.getResult().getType())) {
      bool left = lhs.getValue() == "true";
      if (spelling == "&&" || spelling == "||") {
        // The left operand is a literal, so this preserves reads and calls
        // in the right operand precisely when the language evaluates it.
        bool skip = spelling == "&&" ? !left : left;
        rewriter.replaceOp(op, skip ? op.getLhs() : op.getRhs());
        return success();
      }
      if (rhs && (spelling == "==" || spelling == "!=")) {
        bool equal = lhs.getValue() == rhs.getValue();
        replaceLiteral(rewriter, op,
                       (equal == (spelling == "==")) ? "true" : "false");
        return success();
      }
    }
    if (!rhs)
      return failure();
    if (isa<FloatType, LengthType>(op.getLhs().getType()) &&
        isa<FloatType, LengthType>(op.getRhs().getType()))
      return foldFloat(op, rewriter, lhs, rhs);
    if (isa<StringType>(op.getLhs().getType()) &&
        isa<StringType>(op.getRhs().getType()))
      return foldString(op, rewriter, lhs, rhs);
    int64_t left, right;
    if (!isa<IntType>(op.getLhs().getType()) ||
        !isa<IntType>(op.getRhs().getType()) ||
        lhs.getValue().getAsInteger(10, left) ||
        rhs.getValue().getAsInteger(10, right))
      return failure();
    return foldInt(op, rewriter, left, right);
  }
};

struct FoldCast : OpRewritePattern<CastOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(CastOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    int64_t number;
    if (!literal || literal.getValue().getAsInteger(10, number))
      return failure();
    llvm::APFloat value(0.0);
    value.convertFromAPInt(llvm::APInt(64, static_cast<uint64_t>(number)), true,
                           llvm::APFloat::rmNearestTiesToEven);
    return replaceFloatLiteral(rewriter, op, value);
  }
};

struct FoldToRatio : OpRewritePattern<ToRatioOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(ToRatioOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    double number;
    if (!literal || literal.getValue().getAsDouble(number))
      return failure();
    llvm::APFloat value(number);
    value.divide(llvm::APFloat(100.0), llvm::APFloat::rmNearestTiesToEven);
    return replaceFloatLiteral(rewriter, op, value);
  }
};

struct FoldStringify : OpRewritePattern<StringifyOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(StringifyOp op,
                                PatternRewriter &rewriter) const override {
    auto literal = op.getInput().getDefiningOp<LiteralOp>();
    if (!literal)
      return failure();
    auto type = literal.getResult().getType();
    if (isa<BoolType>(type)) {
      replaceStringLiteral(rewriter, op, literal.getValue());
      return success();
    }
    int64_t number;
    if (!isa<IntType>(type) || literal.getValue().getAsInteger(10, number))
      return failure();
    replaceStringLiteral(rewriter, op, std::to_string(number));
    return success();
  }
};

LogicalResult rewriteExpressions(ModuleOp module, RewritePatternSet patterns) {
  GreedyRewriteConfig config;
  config.setRegionSimplificationLevel(GreedySimplifyRegionLevel::Disabled);
  return applyPatternsGreedily(module, std::move(patterns), config);
}

struct FoldConstants : impl::XrFoldConstantsBase<FoldConstants> {
  void runOnOperation() override {
    const auto &properties = getAnalysis<PropertyAnalysis>();
    for (;;) {
      RewritePatternSet patterns(&getContext());
      patterns.add<FoldUnary, FoldBinary, FoldCast, FoldToRatio, FoldStringify>(
          &getContext());
      if (failed(rewriteExpressions(getOperation(), std::move(patterns)))) {
        signalPassFailure();
        return;
      }

      // The declaration and write facts are stable during these rewrites.
      // Named reads are not SSA uses of their property initializer, so visit
      // them again after folding to propagate through chains of properties.
      llvm::SmallVector<std::pair<ReadOp, StringAttr>> constants;
      getOperation()->walk([&](ReadOp read) {
        auto property = properties.resolve(read);
        if (properties.isImmutablePrivateScalar(property))
          if (auto literal = property.getInitial().getDefiningOp<LiteralOp>())
            constants.emplace_back(read, literal.getValueAttr());
      });
      if (constants.empty())
        break;
      IRRewriter rewriter(&getContext());
      for (auto [read, spelling] : constants) {
        rewriter.setInsertionPoint(read);
        rewriter.replaceOpWithNewOp<LiteralOp>(read, read.getResult().getType(),
                                               spelling);
      }
    }
  }
};

bool isIntLiteral(Value value, int64_t expected) {
  auto literal = value.getDefiningOp<LiteralOp>();
  int64_t number;
  return literal && !literal.getValue().getAsInteger(10, number) &&
         number == expected;
}

bool isBoolLiteral(Value value, bool expected) {
  auto literal = value.getDefiningOp<LiteralOp>();
  return literal && isa<BoolType>(value.getType()) &&
         literal.getValue() == (expected ? "true" : "false");
}

Value simplifyIdentity(BinaryOp op) {
  Value lhs = op.getLhs(), rhs = op.getRhs();
  Type type = op.getResult().getType();
  if (lhs.getType() != type || rhs.getType() != type)
    return {};
  auto spelling = op.getValue();
  // Always retain evaluation of the non-identity operand. In particular, x*0
  // and x&&false could discard a getter, callback or a trapping computation.
  if (isa<IntType>(type)) {
    if ((spelling == "+" || spelling == "-") && isIntLiteral(rhs, 0))
      return lhs;
    if (spelling == "+" && isIntLiteral(lhs, 0))
      return rhs;
    if ((spelling == "*" || spelling == "/") && isIntLiteral(rhs, 1))
      return lhs;
    if (spelling == "*" && isIntLiteral(lhs, 1))
      return rhs;
  } else if (isa<BoolType>(type)) {
    if ((spelling == "&&" || spelling == "==") && isBoolLiteral(rhs, true))
      return lhs;
    if ((spelling == "||" || spelling == "!=") && isBoolLiteral(rhs, false))
      return lhs;
    if (spelling == "==" && isBoolLiteral(lhs, true))
      return rhs;
    if (spelling == "!=" && isBoolLiteral(lhs, false))
      return rhs;
  } else if (isa<StringType>(type) && spelling == "+") {
    auto left = stringLiteral(lhs.getDefiningOp<LiteralOp>());
    auto right = stringLiteral(rhs.getDefiningOp<LiteralOp>());
    if (left && left->empty())
      return rhs;
    if (right && right->empty())
      return lhs;
  }
  return {};
}

struct SimplifyBinary : OpRewritePattern<BinaryOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(BinaryOp op,
                                PatternRewriter &rewriter) const override {
    auto kept = simplifyIdentity(op);
    if (!kept)
      return failure();
    rewriter.replaceOp(op, kept);
    return success();
  }
};

struct SimplifyUnary : OpRewritePattern<UnaryOp> {
  using OpRewritePattern::OpRewritePattern;

  LogicalResult matchAndRewrite(UnaryOp op,
                                PatternRewriter &rewriter) const override {
    if (op.getValue() == "+" &&
        isa<IntType, FloatType, LengthType>(op.getInput().getType())) {
      rewriter.replaceOp(op, op.getInput());
      return success();
    }
    auto inner = op.getInput().getDefiningOp<UnaryOp>();
    if (inner && op.getValue() == "!" && inner.getValue() == "!" &&
        isa<BoolType>(op.getInput().getType())) {
      rewriter.replaceOp(op, inner.getInput());
      return success();
    }
    return failure();
  }
};

struct SimplifyExpressions
    : impl::XrSimplifyExpressionsBase<SimplifyExpressions> {
  void runOnOperation() override {
    RewritePatternSet patterns(&getContext());
    patterns.add<SimplifyUnary, SimplifyBinary>(&getContext());
    if (failed(rewriteExpressions(getOperation(), std::move(patterns))))
      signalPassFailure();
  }
};

struct SimplifyVisuals : impl::XrSimplifyVisualsBase<SimplifyVisuals> {
  void runOnOperation() override {
    getOperation()->walk([&](AnimateOp animation) {
      auto enabled = animation.getEnabled();
      if (enabled && isBoolLiteral(enabled, false)) {
        animation.erase();
        return;
      }
      if (enabled && isBoolLiteral(enabled, true))
        animation.getEnabledMutable().clear();
      if (!animation.getEnabled() && isIntLiteral(animation.getInput(), 0) &&
          (!animation.getDelayAttr() || animation.getDelayAttr().getInt() == 0))
        animation.erase();
    });
    getOperation()->walk([&](ShowIfOp guard) {
      // Keep the element and its event path. A false guard also preserves its
      // hidden slot, so only erase a sole guard known to be true.
      if (isBoolLiteral(guard.getInput(), true) &&
          llvm::hasSingleElement(guard->getBlock()->getOps<ShowIfOp>()))
        guard.erase();
    });
  }
};

struct PruneConstantStates
    : impl::XrPruneConstantStatesBase<PruneConstantStates> {
  void runOnOperation() override {
    getOperation()->walk([&](StateSelectOp selector) {
      llvm::SmallVector<Operation *> users;
      for (auto &use : selector.getResult().getUses()) {
        auto *user = use.getOwner();
        // These ops put the selector first. Its numeric value can also be
        // consumed by hand-written IR, where renumbering would change data.
        if (use.getOperandNumber() != 0 ||
            !isa<StateSetOp, InstanceStateOp, StateTimingOp>(user))
          return;
        users.push_back(user);
      }
      llvm::SmallVector<int64_t> indices;
      llvm::SmallVector<Value> conditions;
      llvm::SmallVector<Attribute> names;
      bool unconditional = false;
      for (auto [condition, name] :
           llvm::zip_equal(selector.getConditions(), selector.getNames())) {
        if (unconditional || isBoolLiteral(condition, false)) {
          indices.push_back(-1);
        } else {
          indices.push_back(conditions.size());
          conditions.push_back(condition);
          names.push_back(name);
          unconditional = isBoolLiteral(condition, true);
        }
      }
      if (conditions.size() == indices.size())
        return;
      Builder builder(&getContext());
      for (auto *user : users) {
        auto index = user->getAttrOfType<IntegerAttr>("case_index").getInt();
        if (indices[index] < 0)
          user->erase();
        else
          user->setAttr("case_index",
                        builder.getI64IntegerAttr(indices[index]));
      }
      if (conditions.empty()) {
        selector.erase();
      } else {
        selector.getConditionsMutable().assign(conditions);
        selector.setNamesAttr(builder.getArrayAttr(names));
      }
    });
  }
};

struct RemoveUnusedProperties
    : impl::XrRemoveUnusedPropertiesBase<RemoveUnusedProperties> {
  void runOnOperation() override {
    const auto &properties = getAnalysis<PropertyAnalysis>();
    getOperation()->walk([&](PropertyOp property) {
      if (properties.isImmutablePrivateScalar(property) &&
          !properties.hasRead(property) &&
          property.getInitial().getDefiningOp<LiteralOp>())
        property.erase();
    });
  }
};

struct PruneExpressions : impl::XrPruneExpressionsBase<PruneExpressions> {
  void runOnOperation() override {
    // XR scopes have one block. Visit users before definitions, including uses
    // in nested scopes, so dead expression trees disappear in a single walk.
    getOperation()->walk<WalkOrder::PostOrder, ReverseIterator>(
        [](Operation *op) {
          if (!op->getNumResults() || !op->use_empty())
            return;
          // These expressions are lowered at their use sites. An unused xr.call
          // never executes; standalone calls are represented by xr.invoke.
          if (isa<ReadOp, StackReadOp, TreeQueryOp, ViewIndexOp, TableSortReadOp, TableSelectionReadOp, TableHistoryReadOp, SplitStateReadOp, ParamOp, CallOp, UnaryOp, BinaryOp,
                  CastOp, StringifyOp>(op) ||
              isOpTriviallyDead(op))
            op->erase();
        });
  }
};

struct SpecializeHiddenVisuals
    : impl::XrSpecializeHiddenVisualsBase<SpecializeHiddenVisuals> {
  void runOnOperation() override {
    auto name = StringAttr::get(&getContext(), "xr.static_hidden");
    auto marker = StringAttr::get(&getContext(), "true");
    getOperation()->walk([&](ShowIfOp guard) {
      if (guard->getDiscardableAttr(name))
        guard->removeDiscardableAttr(name);
      auto *block = guard->getBlock();
      if (isBoolLiteral(guard.getInput(), false) &&
          llvm::hasSingleElement(block->getOps<ShowIfOp>()) &&
          block->getOps<RepeatOp>().empty())
        guard->setDiscardableAttr(name, marker);
    });
  }
};
} // namespace

void buildPipeline(OpPassManager &passes) {
  passes.addPass(std::make_unique<FoldConstants>());
  passes.addPass(std::make_unique<SimplifyExpressions>());
  passes.addPass(std::make_unique<SimplifyVisuals>());
  passes.addPass(createCanonicalizerPass());
  passes.addPass(createCSEPass());
  passes.addPass(std::make_unique<PruneConstantStates>());
  passes.addPass(std::make_unique<PruneExpressions>());
  passes.addPass(std::make_unique<RemoveUnusedProperties>());
  passes.addPass(std::make_unique<PruneExpressions>());
  passes.addPass(std::make_unique<SpecializeHiddenVisuals>());
  addScenePlanPasses(passes);
  addVisualCachePasses(passes);
}
} // namespace xrack::dialect
