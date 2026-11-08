#include "mlir/xrack_passes.h"

#include "mlir/IR/PatternMatch.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/Transforms/GreedyPatternRewriteDriver.h"
#include "mlir/Transforms/Passes.h"
#include "mlir/xrack_dialect.h"
#include "llvm/ADT/APInt.h"
#include "llvm/ADT/StringSwitch.h"
#include <optional>
#include <string>

namespace xrack::dialect {
#define GEN_PASS_DEF_XRFOLDCONSTANTS
#include "mlir/XrackPasses.h.inc"

using namespace ::mlir;

namespace {
void replaceLiteral(PatternRewriter &rewriter, Operation *op,
                    llvm::StringRef spelling) {
  rewriter.replaceOpWithNewOp<LiteralOp>(op, op->getResult(0).getType(),
                                         rewriter.getStringAttr(spelling));
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
    int64_t left, right;
    if (!rhs || !isa<IntType>(op.getLhs().getType()) ||
        !isa<IntType>(op.getRhs().getType()) ||
        lhs.getValue().getAsInteger(10, left) ||
        rhs.getValue().getAsInteger(10, right))
      return failure();
    return foldInt(op, rewriter, left, right);
  }
};

struct FoldConstants : impl::XrFoldConstantsBase<FoldConstants> {
  void runOnOperation() override {
    RewritePatternSet patterns(&getContext());
    patterns.add<FoldUnary, FoldBinary>(&getContext());
    GreedyRewriteConfig config;
    config.setRegionSimplificationLevel(GreedySimplifyRegionLevel::Disabled);
    if (failed(
            applyPatternsGreedily(getOperation(), std::move(patterns), config)))
      signalPassFailure();
  }
};
} // namespace

void buildPipeline(OpPassManager &passes) {
  passes.addPass(std::make_unique<FoldConstants>());
  passes.addPass(createCanonicalizerPass());
  passes.addPass(createCSEPass());
}
} // namespace xrack::dialect
